//! Loader module - loads and executes .espr relocatable binaries

use crate::mem::pool;

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
}

impl core::fmt::Display for LoaderError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LoaderError::BadMagic => write!(f, "ERR_BAD_MAGIC"),
            LoaderError::TooLarge => write!(f, "ERR_TOO_LARGE"),
            LoaderError::InvalidEntry => write!(f, "ERR_INVALID_ENTRY"),
            LoaderError::InvalidReloc => write!(f, "ERR_INVALID_RELOC"),
            LoaderError::NoMemory => write!(f, "ERR_NO_MEMORY"),
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
    if data.len() < HEADER_SIZE { return Err(LoaderError::BadMagic); }
    let header = read_header(data)?;
    if header.magic != MAGIC { return Err(LoaderError::BadMagic); }

    let code_size = header.code_size as usize;
    let data_size = header.data_size as usize;
    let bss_size = header.bss_size as usize;
    let reloc_count = header.reloc_count as usize;
    let total = code_size + data_size;

    let pages_needed = (total + pool::PAGE_SIZE - 1) / pool::PAGE_SIZE;
    let base = pool::alloc_pages(pages_needed).ok_or(LoaderError::NoMemory)?;

    unsafe {
        let dst = core::slice::from_raw_parts_mut(base as *mut u8, total);
        let src_end = HEADER_SIZE + total;
        if src_end > data.len() { pool::free_page(base); return Err(LoaderError::TooLarge); }
        dst.copy_from_slice(&data[HEADER_SIZE..src_end]);

        let bss_start = base + code_size + data_size;
        for i in 0..bss_size {
            core::ptr::write_volatile((bss_start + i) as *mut u8, 0);
        }

        if reloc_count > 0 {
            let reloc_off = header.reloc_offset as usize;
            if reloc_off + reloc_count * 4 > data.len() { pool::free_page(base); return Err(LoaderError::InvalidReloc); }
            for i in 0..reloc_count {
                let offset = u32::from_le_bytes([
                    data[reloc_off + i * 4], data[reloc_off + i * 4 + 1],
                    data[reloc_off + i * 4 + 2], data[reloc_off + i * 4 + 3],
                ]) as usize;
                if offset + 4 > total { pool::free_page(base); return Err(LoaderError::InvalidReloc); }
                let patch_addr = (base + offset) as *mut u32;
                let old_val = core::ptr::read_volatile(patch_addr);
                core::ptr::write_volatile(patch_addr, old_val.wrapping_add(base as u32));
            }
        }

        Ok(LoadedProgram {
            base, code_size, data_size, bss_size,
            entry: base + header.entry_offset as usize,
            stack_size: header.stack_size as usize,
        })
    }
}

pub fn unload(prog: &LoadedProgram) {
    let pages_needed = (prog.code_size + prog.data_size + 4095) / 4096;
    for i in 0..pages_needed {
        pool::free_page(prog.base + i * pool::PAGE_SIZE);
    }
}

pub fn load_from_sd(path: &str) -> Result<LoadedProgram, LoaderError> {
    use crate::drivers::sd;
    use embedded_sdmmc::{VolumeIdx, Mode};

    let mut mgr_ref = unsafe {
        sd::VOLUME_MGR.as_mut().ok_or(LoaderError::NoMemory)?
    };

    let mut volume = mgr_ref.open_volume(VolumeIdx(0)).map_err(|_| LoaderError::NoMemory)?;
    let mut root = volume.open_root_dir().map_err(|_| LoaderError::NoMemory)?;
    let mut file = root.open_file_in_dir(path, Mode::ReadOnly).map_err(|_| LoaderError::NoMemory)?;

    let mut file_data = [0u8; 4096];
    let mut offset = 0usize;

    loop {
        let mut chunk = [0u8; 512];
        let n = file.read(&mut chunk).map_err(|_| LoaderError::NoMemory)?;
        if n == 0 { break; }
        let copy_end = core::cmp::min(offset + n, file_data.len());
        file_data[offset..copy_end].copy_from_slice(&chunk[..copy_end - offset]);
        offset = copy_end;
        if copy_end >= file_data.len() { break; }
    }

    let _ = file.close();
    let _ = root.close();
    let _ = volume.close();

    load(&file_data[..offset])
}
