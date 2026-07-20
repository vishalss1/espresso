//! Memory management module - exec pool allocator
//!
//! 82 pages * 4KB = 328KB, first-fit, 3xu32 bitmap.
//! No heap. Every allocation is a static, fixed-size page from this pool.
//! free_count is computed from the bitmap, not stored (avoids struct init issues).
//!
//! ESP32 SRAM bus layout (TRM 1.3.2.3-1.3.2.4):
//!   SRAM0: DRAM 0x3FFB0000-0x3FFCFFFF → IRAM 0x40080000-0x4009FFFF (NORMAL word order)
//!   SRAM1: DRAM 0x3FFE0000-0x3FFFFFFF → IRAM 0x400A0000-0x400BFFFF (REVERSED word order!)
//!
//! Exec pages MUST use SRAM0 (pages 0..20, DRAM 0x3FFBC800-0x3FFD07FF) so the
//! loader's simple +0xD0000 DRAM→IRAM offset works. Stack/data pages use SRAM1
//! (pages 20..82) where reversed IRAM ordering is irrelevant (data bus only).

pub const PAGE_SIZE: usize = 4096;
pub const TOTAL_PAGES: usize = 82; // 328KB (entire internal SRAM region starting from 0x3FFBC800)
pub const POOL_START: usize = 0x3FFBC800; // Start of SRAM2 DRAM pool

#[repr(C)]
pub struct ExecPool {
    bitmap: [u32; 3],
}

pub static mut EXEC_POOL: ExecPool = ExecPool {
    bitmap: [0xFF8FFFFF, 0xFFFFFFFF, 0x0003FFFF], // First 82 bits are 1, but pages 20-22 are reserved for static large_bss (MBUF, EBUF, FILE_DATA)
};

fn pool_ptr() -> *const ExecPool {
    &raw const EXEC_POOL
}

fn pool_mut_ptr() -> *mut ExecPool {
    &raw mut EXEC_POOL
}

pub fn free_count() -> usize {
    unsafe {
        let p = pool_ptr();
        let mut count: usize = 0;
        let mut page: usize = 0;
        for word_idx in 0..3u32 {
            let word = (*p).bitmap[word_idx as usize];
            let mut bits = word;
            while bits != 0 && page < TOTAL_PAGES {
                if bits & 1 != 0 {
                    count += 1;
                }
                bits >>= 1;
                page += 1;
            }
        }
        count
    }
}

pub fn alloc_page() -> Option<usize> {
    alloc_exec_pages(1)
}

pub fn alloc_pages_in_range(count: usize, start_page: usize, end_page: usize) -> Option<usize> {
    if count == 0 || count > (end_page - start_page) {
        return None;
    }
    unsafe {
        let p = pool_mut_ptr();
        let bitmap = &mut (*p).bitmap;

        let mut run_start: Option<usize> = None;
        let mut run_len: usize = 0;
        for page in start_page..end_page {
            let word_idx = page / 32;
            let bit = page % 32;
            let is_free = bitmap[word_idx] & (1 << bit) != 0;

            if is_free {
                if run_len == 0 {
                    run_start = Some(page);
                }
                run_len += 1;
                if run_len == count {
                    let start = run_start.unwrap();
                    for p in start..start + count {
                        let wi = p / 32;
                        let bi = p % 32;
                        bitmap[wi] &= !(1 << bi);
                    }
                    return Some(POOL_START + start * PAGE_SIZE);
                }
            } else {
                run_start = None;
                run_len = 0;
            }
        }
        None
    }
}

/// Allocate executable pages from SRAM0 (DRAM 0x3FFBC800 - 0x3FFD07FF, pages 0..20).
///
/// SRAM0 has NORMAL word ordering between DRAM and IRAM buses, so the simple
/// `+0xD0000` offset correctly translates DRAM addresses to their IRAM aliases.
/// SRAM1 (pages 20+) has REVERSED word ordering (TRM 1.3.2.4) — code written
/// sequentially to SRAM1 DRAM appears in reverse order on the IRAM bus, causing
/// the CPU to execute garbage. Executable pages MUST stay in SRAM0.
pub fn alloc_exec_pages(count: usize) -> Option<usize> {
    alloc_pages_in_range(count, 0, 20)
}

/// Allocate stack/data pages from SRAM1 (pages 20..82).
/// SRAM1 has reversed IRAM word ordering but this only affects instruction fetch.
/// Stacks and data are accessed via the data bus, where SRAM1 works normally.
pub fn alloc_stack_pages(count: usize) -> Option<usize> {
    alloc_pages_in_range(count, 20, 82)
}

pub fn alloc_pages(count: usize) -> Option<usize> {
    alloc_exec_pages(count)
}

pub fn free_page(addr: usize) {
    let offset = addr.wrapping_sub(POOL_START);
    if offset >= TOTAL_PAGES * PAGE_SIZE {
        return;
    }
    let page_idx = offset / PAGE_SIZE;
    if page_idx >= TOTAL_PAGES {
        return;
    }
    unsafe {
        let p = pool_mut_ptr();
        let bitmap = &mut (*p).bitmap;
        let word_idx = page_idx / 32;
        let bit = page_idx % 32;
        bitmap[word_idx] |= 1 << bit;
    }
}

/// Explicitly (re)initialize the bitmap. Call after init_memory()
/// in case .data section was clobbered by the bootloader or BSS zeroing.
pub fn init_bitmap() {
    unsafe {
        let p = pool_mut_ptr();
        (*p).bitmap = [0xFF8FFFFF, 0xFFFFFFFF, 0x0003FFFF]; // Reserve pages 20-22 for static large_bss buffers
    }
}

/// Dump raw bitmap words for diagnostics.
pub fn bitmap_words() -> (u32, u32, u32) {
    unsafe {
        let p = pool_ptr();
        ((*p).bitmap[0], (*p).bitmap[1], (*p).bitmap[2])
    }
}

pub fn mem_info(buf: &mut [u8]) {
    let free = free_count();
    let total = TOTAL_PAGES;
    let total_bytes = (total * PAGE_SIZE) as u32;
    let free_bytes = (free * PAGE_SIZE) as u32;
    if buf.len() >= 8 {
        buf[0..4].copy_from_slice(&total_bytes.to_le_bytes());
        buf[4..8].copy_from_slice(&free_bytes.to_le_bytes());
    }
}
