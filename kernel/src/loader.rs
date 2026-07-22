//! Loader module - loads and executes .espr relocatable binaries
//!
//! ESP32 Memory Bus Architecture:
//!   The ESP32 has separate instruction and data buses. The internal SRAM is
//!   "dual-mapped" — accessible via both buses at different addresses:
//!     Data bus (DRAM):  0x3FFB0000 – 0x3FFEFFFF  (load/store)
//!     Instr bus (IRAM): 0x40080000 – 0x400BFFFF  (CPU instruction fetch)
//!
//!   Code written to DRAM addresses (0x3FFBxxxx) cannot be directly executed.
//!   The entry point must be translated to its IRAM alias (addr + 0xD0000)
//!   before being used as a task entry point.

use crate::mem::pool;

/// ESP32 dual-mapped SRAM (DIRAM) data bus base address.
/// Physical internal SRAM is accessible via the data bus starting here.
const DIRAM_DBUS_BASE: usize = 0x3FFB0000;
/// ESP32 dual-mapped SRAM (DIRAM) instruction bus base address.
/// The same physical SRAM is accessible via the instruction bus starting here.
/// Offset: IRAM base (0x40080000) - DRAM base (0x3FFB0000) = 0xD0000
const DIRAM_IBUS_BASE: usize = 0x40080000;
/// Offset to translate a DRAM address to its IRAM instruction-bus alias.
/// This is the SOC_IRAM_DRAM_OFFSET constant for the ESP32.
const DIRAM_OFFSET: usize = 0x000D0000; // = DIRAM_IBUS_BASE - DIRAM_DBUS_BASE
/// Size of the dual-mapped internal SRAM region (256 KB).
/// DRAM view: 0x3FFB0000 – 0x3FFEFFFF
/// IRAM view: 0x40080000 – 0x400BFFFF
const DIRAM_SIZE: usize = 0x40000; // 256 KB

/// Translate a DRAM (data bus) address within the ESP32 dual-mapped SRAM to its
/// instruction-bus (IRAM) equivalent so the CPU can execute code from it.
///
/// Returns the same address unchanged if it is already an IRAM address or
/// outside the dual-mapped DIRAM region.
///
/// # ESP32 Memory Architecture
/// The internal SRAM is dual-mapped:
///   Data bus (DRAM):  0x3FFB0000 – 0x3FFCFFFF  (use for load/store)
///   Instr bus (IRAM): 0x40080000 – 0x4009FFFF  (CPU fetches instructions here)
/// Writing code bytes to a DRAM address and then jumping to (DRAM_addr + 0xD0000)
/// is how bare-metal ESP32 software executes dynamically-loaded code.
pub fn dram_to_iram(addr: usize) -> usize {
    if addr >= DIRAM_DBUS_BASE && addr < DIRAM_DBUS_BASE + DIRAM_SIZE {
        addr + DIRAM_OFFSET
    } else {
        addr
    }
}

pub fn iram_to_dram(addr: usize) -> usize {
    if addr >= DIRAM_IBUS_BASE && addr < DIRAM_IBUS_BASE + DIRAM_SIZE {
        addr - DIRAM_OFFSET
    } else {
        addr
    }
}

pub const MAGIC: u32 = 0x45535052; // "ESPR"
pub const HEADER_SIZE: usize = 0x20;

#[repr(C)]
struct EsprHeader {
    magic: u32,
    code_size: u32,
    data_size: u32,
    bss_size: u32,
    entry_offset: u32,
    reloc_offset: u32,
    reloc_count: u32,
    stack_size: u32,
}

#[derive(Debug, Copy, Clone)]
pub enum LoaderError {
    BadMagic,
    TooLarge,
    InvalidEntry,
    InvalidReloc,
    NoMemory,
    ReadError(&'static str),
}

impl core::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoaderError::BadMagic => write!(f, "ERR_BAD_MAGIC"),
            LoaderError::TooLarge => write!(f, "ERR_TOO_LARGE"),
            LoaderError::InvalidEntry => write!(f, "ERR_INVALID_ENTRY"),
            LoaderError::InvalidReloc => write!(f, "ERR_INVALID_RELOC"),
            LoaderError::NoMemory => write!(f, "ERR_NO_MEMORY"),
            LoaderError::ReadError(s) => {
                write!(f, "ERR_READ_FAILED(")?;
                write!(f, "{}", s)?;
                write!(f, ")")
            }
        }
    }
}

pub struct LoadedProgram {
    pub base: usize,
    pub code_size: usize,
    pub data_size: usize,
    pub bss_size: usize,
    pub entry: usize,
    pub stack_size: usize,
}

fn read_header(data: &[u8]) -> Result<EsprHeader, LoaderError> {
    if data.len() < HEADER_SIZE { return Err(LoaderError::BadMagic); }
    Ok(EsprHeader {
        magic: u32::from_le_bytes([data[0], data[1], data[2], data[3]]),
        code_size: u32::from_le_bytes([data[4], data[5], data[6], data[7]]),
        data_size: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        bss_size: u32::from_le_bytes([data[12], data[13], data[14], data[15]]),
        entry_offset: u32::from_le_bytes([data[16], data[17], data[18], data[19]]),
        reloc_offset: u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
        reloc_count: u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
        stack_size: u32::from_le_bytes([data[28], data[29], data[30], data[31]]),
    })
}

pub fn load(data: &[u8]) -> Result<LoadedProgram, LoaderError> {
    crate::println!("[LOAD] Beginning binary parsing. Size = {} bytes", data.len());
    if data.len() < HEADER_SIZE { return Err(LoaderError::BadMagic); }
    let header = read_header(data)?;
    crate::println!("[LOAD] Parsed EsprHeader: magic=0x{:08X}, code_size={}, data_size={}, bss_size={}, entry_offset={}, reloc_offset={}, reloc_count={}, stack_size={}",
        header.magic, header.code_size, header.data_size, header.bss_size, header.entry_offset, header.reloc_offset, header.reloc_count, header.stack_size);
    if header.magic != MAGIC { return Err(LoaderError::BadMagic); }

    let code_size = header.code_size as usize;
    let data_size = header.data_size as usize;
    let bss_size = header.bss_size as usize;
    let reloc_count = header.reloc_count as usize;

    // Ensure the payload size is 4-byte aligned for word-wise loading
    let total = (code_size + data_size + 3) & !3;

    // Allocate memory pages.
    let total_with_bss = total + bss_size;
    let pages_needed = (total_with_bss + pool::PAGE_SIZE - 1) / pool::PAGE_SIZE;
    crate::println!("[LOAD] code_size={}, data_size={}, bss_size={}, pages_needed={}",
        code_size, data_size, bss_size, pages_needed);
    let base_dram = pool::alloc_pages(pages_needed).ok_or(LoaderError::NoMemory)?;
    // Translate the DRAM pool address to its IRAM instruction-bus alias.
    // This is the address the CPU uses to FETCH instructions.
    let iram_base = dram_to_iram(base_dram);
    crate::println!("[LOAD] base_dram=0x{:08X} iram_base=0x{:08X}", base_dram, iram_base);

    unsafe {
        let word_count = total / 4;
        let src_payload = &data[HEADER_SIZE..HEADER_SIZE + code_size + data_size];

        // 1. Write code/data directly to IRAM addresses (instruction bus alias).
        // All stores must be 32-bit word-aligned (IRAM requirement).
        for i in 0..word_count {
            let src_off = i * 4;
            let mut word_bytes = [0u8; 4];
            for b in 0..4 {
                if src_off + b < src_payload.len() {
                    word_bytes[b] = src_payload[src_off + b];
                }
            }
            let src_word = u32::from_le_bytes(word_bytes);
            let dest_iram = iram_base + i * 4;
            core::ptr::write_volatile(dest_iram as *mut u32, src_word);
        }

        // 2. Zero BSS in IRAM (beyond the code/data region).
        let allocated_bytes = pages_needed * pool::PAGE_SIZE;
        for addr in (iram_base..iram_base + allocated_bytes).step_by(4) {
            if addr >= iram_base + total {
                core::ptr::write_volatile(addr as *mut u32, 0);
            }
        }

        // 3. Patch relocations in IRAM.
        if reloc_count > 0 {
            let reloc_off = header.reloc_offset as usize;
            if reloc_off + reloc_count * 4 > data.len() {
                pool::free_page(base_dram);
                return Err(LoaderError::InvalidReloc);
            }
            for i in 0..reloc_count {
                let offset = u32::from_le_bytes([
                    data[reloc_off + i * 4],     data[reloc_off + i * 4 + 1],
                    data[reloc_off + i * 4 + 2], data[reloc_off + i * 4 + 3],
                ]) as usize;

                if offset + 4 > total {
                    pool::free_page(base_dram);
                    return Err(LoaderError::InvalidReloc);
                }

                // Patch the word at IRAM[offset] by adding iram_base.
                let patch_iram = iram_base + offset;
                let old_val = core::ptr::read_volatile(patch_iram as *const u32);
                let new_val = iram_base as u32 + old_val;
                core::ptr::write_volatile(patch_iram as *mut u32, new_val);
            }
        }

        // Calculate the entry point in IRAM
        let iram_entry = iram_base + header.entry_offset as usize;

        // Verify: read back first word from IRAM to confirm the write succeeded.
        let first_word_iram = core::ptr::read_volatile(iram_base as *const u32);
        let expected_word = {
            let mut b = [0u8; 4];
            for i in 0..4 {
                if HEADER_SIZE + i < data.len() { b[i] = data[HEADER_SIZE + i]; }
            }
            u32::from_le_bytes(b)
        };
        crate::println!("[LOAD] IRAM[0]=0x{:08X} expected=0x{:08X} entry=0x{:08X}",
            first_word_iram, expected_word, iram_entry);

        // Flush instruction pipeline after writing new code to IRAM.
        // memw: drain data-bus store buffer
        // dsync: synchronize data side-effects
        // isync: flush CPU instruction prefetch buffer
        core::arch::asm!("memw", "dsync", "isync");

        Ok(LoadedProgram {
            base: base_dram,  // DRAM address — used by unload() to free pages
            code_size,
            data_size,
            bss_size,
            entry: iram_entry,
            stack_size: header.stack_size as usize,
        })
    }
}


pub fn unload(prog: &LoadedProgram) {
    let total_with_bss = ((prog.code_size + prog.data_size + 3) & !3) + prog.bss_size;
    let pages_needed = (total_with_bss + pool::PAGE_SIZE - 1) / pool::PAGE_SIZE;
    for i in 0..pages_needed {
        pool::free_page(prog.base + i * pool::PAGE_SIZE);
    }
}

#[link_section = ".large_bss"]
static mut FILE_DATA: [u8; 4096] = [0u8; 4096];

pub fn load_from_storage(path: &str) -> Result<LoadedProgram, LoaderError> {
    crate::println!("[LOAD] load_from_storage requested for '{}' but storage driver is not in kernel space", path);
    Err(LoaderError::ReadError("storage driver not in kernel"))
}
