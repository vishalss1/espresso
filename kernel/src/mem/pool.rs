//! Memory management module — exec pool allocator (CLAUDE.md spec)
//!
//! 107 pages * 4KB = 428KB exec pool (0x3FFBF400 - 0x3FFF8800).
//! First-fit, 4xu32 bitmap. No heap.

pub const PAGE_SIZE: usize = 4096;
pub const TOTAL_PAGES: usize = 107; // 428KB (0x3FFBF400)
pub const POOL_START: usize = 0x3FFBF400;

#[repr(C)]
pub struct ExecPool {
    bitmap: [u32; 4],
}

// 107 bits initialized to 1 (free)
pub static mut EXEC_POOL: ExecPool = ExecPool {
    bitmap: [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0x07FFFFFF],
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
        for word_idx in 0..4u32 {
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

pub fn alloc_exec_pages(count: usize) -> Option<usize> {
    alloc_pages_in_range(count, 0, 30)
}

pub fn alloc_stack_pages(count: usize) -> Option<usize> {
    alloc_pages_in_range(count, 30, TOTAL_PAGES)
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

pub fn init_bitmap() {
    unsafe {
        let p = pool_mut_ptr();
        (*p).bitmap = [0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0x07FFFFFF];
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
