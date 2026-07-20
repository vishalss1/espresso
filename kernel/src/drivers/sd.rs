use core::cell::RefCell;
use embedded_hal::delay::DelayNs;
use embedded_hal::spi::{Operation, SpiDevice};
use embedded_sdmmc::{Block, BlockCount, BlockDevice, BlockIdx, TimeSource, Timestamp, VolumeIdx, VolumeManager};

use crate::drivers::delay::RawDelay;
use crate::drivers::spi::RawSpi;

// ── Time source ────────────────────────────────────────────────────────────────

pub struct DummyTimeSource;

impl TimeSource for DummyTimeSource {
    fn get_timestamp(&self) -> Timestamp {
        Timestamp {
            year_since_1970: 56,
            zero_indexed_month: 6,
            zero_indexed_day: 4,
            hours: 9,
            minutes: 30,
            seconds: 0,
        }
    }
}

// ── SD protocol constants ──────────────────────────────────────────────────────

const CMD0: u8 = 0x00;
const CMD8: u8 = 0x08;
const CMD9: u8 = 0x09;
const CMD17: u8 = 0x11;
const CMD24: u8 = 0x18;
const CMD55: u8 = 0x37;
const CMD58: u8 = 0x3A;
const ACMD41: u8 = 0x29;

const R1_IDLE: u8 = 0x01;
const R1_READY: u8 = 0x00;
const R1_ILLEGAL_CMD: u8 = 0x04;

const DATA_TOKEN: u8 = 0xFE;

fn crc7(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for mut d in data.iter().cloned() {
        for _ in 0..8 {
            crc <<= 1;
            if ((d & 0x80) ^ (crc & 0x80)) != 0 {
                crc ^= 0x09;
            }
            d <<= 1;
        }
    }
    (crc << 1) | 1
}

// ── Error type ─────────────────────────────────────────────────────────────────

#[derive(Debug, Copy, Clone)]
pub enum SdError {
    Timeout,
    CardNotFound,
    ReadError,
    WriteError,
    CmdError,
}

// ── Card type ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum CardType {
    Sd1,
    Sd2,
    Sdhc,
}

// ── RawSdCard: custom BlockDevice with correct CS management ─────────────────

pub struct RawSdCard {
    card_type: RefCell<Option<CardType>>,
}

impl RawSdCard {
    pub fn new() -> RawSdCard {
        RawSdCard {
            card_type: RefCell::new(None),
        }
    }

    /// Send a command and read response, using SpiDevice::transaction()
    /// with Write + Transfer operations (same pattern as embedded-sdmmc).
    ///
    /// Returns (R1, optional extra bytes). For CMD8/CMD58, extra = 4 bytes
    /// (R7/R3). Everything in a single CS transaction.
    fn cmd_transaction(command: u8, arg: u32) -> Result<(u8, Option<[u8; 4]>), SdError> {
        let mut buf = [
            0x40 | command,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8) as u8,
            arg as u8,
            0,
        ];
        buf[5] = crc7(&buf[0..5]);

        let mut resp = [0xFFu8; 16];

        let mut spi = RawSpi;
        spi.transaction(&mut [
            Operation::Write(&buf),
            Operation::Transfer(&mut resp, &[0xFFu8; 16]),
        ])
        .ok();

        for i in 0..resp.len() {
            if resp[i] & 0x80 == 0 {
                let r1 = resp[i];
                let extra = if (command == CMD8 || command == CMD58) && i + 4 < resp.len() {
                    Some([resp[i + 1], resp[i + 2], resp[i + 3], resp[i + 4]])
                } else {
                    None
                };
                return Ok((r1, extra));
            }
        }
        Err(SdError::Timeout)
    }

    /// Initialize card using transaction-based commands.
    pub fn init(&self) -> Result<(), SdError> {
        let mut delay = RawDelay;
        delay.delay_ms(500);

        // 80 dummy clocks with CS HIGH (use transfer() directly, no CS toggle)
        RawSpi::cs_high();
        for _ in 0..10 {
            RawSpi::transfer(Some(&[0xFF]), None::<&mut [u8]>, 1);
        }

        // CMD0
        Self::cmd_transaction(CMD0, 0)?;

        // CMD8
        let (r, maybe_r7) = Self::cmd_transaction(CMD8, 0x1AA)?;
        let card_type: CardType;
        let acmd41_arg: u32;
        if r == (R1_IDLE | R1_ILLEGAL_CMD) {
            card_type = CardType::Sd1;
            acmd41_arg = 0;
        } else {
            let r7 = maybe_r7.ok_or(SdError::CardNotFound)?;
            if r7[3] != 0xAA {
                return Err(SdError::CardNotFound);
            }
            card_type = CardType::Sd2;
            acmd41_arg = 0x4000_0000;
        }

        // ACMD41 loop
        for _ in 0..10000 {
            let (r, _) = Self::cmd_transaction(CMD55, 0)?;
            if r & 0x80 != 0 {
                return Err(SdError::Timeout);
            }
            let (r, _) = Self::cmd_transaction(ACMD41, acmd41_arg)?;
            if r == R1_READY {
                break;
            }
            delay.delay_us(10);
        }

        // CMD58 for SDHC/SDXC detection
        if card_type == CardType::Sd2 {
            let (r1, maybe_ocr) = Self::cmd_transaction(CMD58, 0)?;
            if r1 != 0 {
                return Err(SdError::CmdError);
            }
            let ocr = maybe_ocr.ok_or(SdError::CmdError)?;
            if (ocr[0] & 0xC0) == 0xC0 {
                *self.card_type.borrow_mut() = Some(CardType::Sdhc);
            } else {
                *self.card_type.borrow_mut() = Some(CardType::Sd2);
            }
        } else {
            *self.card_type.borrow_mut() = Some(card_type);
        }
        Ok(())
    }

    pub fn is_sdhc(&self) -> bool {
        matches!(*self.card_type.borrow(), Some(CardType::Sdhc))
    }

    /// Send a command with CS held LOW (used during block read/write).
    fn cmd_sticky(command: u8, arg: u32) -> Result<u8, SdError> {
        let mut buf = [
            0x40 | command,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8) as u8,
            arg as u8,
            0,
        ];
        buf[5] = crc7(&buf[0..5]);

        spi_write(&buf);
        for _ in 0..10000 {
            let r = spi_read_byte();
            if r & 0x80 == 0 {
                return Ok(r);
            }
        }
        Err(SdError::Timeout)
    }

    /// Read 512-byte data block + 2 CRC bytes. CS must already be held LOW.
    fn read_data_block(buf: &mut [u8; 512]) -> Result<(), SdError> {
        for _ in 0..10000 {
            let s = spi_read_byte();
            if s != 0xFF {
                if s == DATA_TOKEN {
                    break;
                }
                return Err(SdError::ReadError);
            }
        }
        for b in buf.iter_mut() {
            *b = 0xFF;
        }
        spi_transfer_in_place(buf);
        let _ = spi_read_byte();
        let _ = spi_read_byte();
        Ok(())
    }

    /// Write 512-byte data block + 2 dummy CRC bytes. CS must already be held LOW.
    /// CMD24 must have already been sent and its R1 verified.
    fn write_data_block(buf: &[u8; 512]) -> Result<(), SdError> {
        spi_write(&[DATA_TOKEN]);
        for chunk in buf.chunks(64) {
            spi_write(chunk);
        }
        spi_write(&[0xFF, 0xFF]);

        let resp = spi_read_byte();
        if (resp & 0x1F) != 0x05 {
            return Err(SdError::WriteError);
        }

        for _ in 0..100000 {
            if spi_read_byte() == 0xFF {
                return Ok(());
            }
        }
        Err(SdError::Timeout)
    }

    fn adjust_block_idx(&self, idx: BlockIdx) -> u32 {
        match *self.card_type.borrow() {
            Some(CardType::Sd1 | CardType::Sd2) => idx.0 * 512,
            Some(CardType::Sdhc) => idx.0,
            None => idx.0,
        }
    }

    fn read_csd(&self) -> Result<[u8; 16], SdError> {
        let mut buf = [0x40 | CMD9, 0, 0, 0, 0, 0];
        buf[5] = crc7(&buf[0..5]);

        RawSpi::cs_low();
        spi_write(&buf);
        for _ in 0..10000 {
            let r = spi_read_byte();
            if r & 0x80 == 0 {
                break;
            }
        }
        for _ in 0..10000 {
            let s = spi_read_byte();
            if s != 0xFF {
                if s == DATA_TOKEN {
                    break;
                }
                RawSpi::cs_high();
                return Err(SdError::ReadError);
            }
        }
        let mut csd = [0xFFu8; 16];
        spi_transfer_in_place(&mut csd);
        let _ = spi_read_byte();
        let _ = spi_read_byte();
        RawSpi::cs_high();
        let _ = spi_read_byte();
        Ok(csd)
    }
}

impl BlockDevice for RawSdCard {
    type Error = SdError;

    fn read(
        &self,
        blocks: &mut [Block],
        start_block_idx: BlockIdx,
        _reason: &str,
    ) -> Result<(), Self::Error> {
        if self.card_type.borrow().is_none() {
            return Err(SdError::CardNotFound);
        }

        let start_idx = self.adjust_block_idx(start_block_idx);

        RawSpi::cs_low();

        if blocks.len() == 1 {
            Self::cmd_sticky(CMD17, start_idx)?;
            Self::read_data_block(&mut blocks[0].contents)?;
        } else {
            Self::cmd_sticky(0x12, start_idx)?;
            for block in blocks.iter_mut() {
                Self::read_data_block(&mut block.contents)?;
            }
            Self::cmd_sticky(0x0C, 0)?;
        }

        RawSpi::cs_high();
        let _ = spi_read_byte();
        Ok(())
    }

    fn write(&self, blocks: &[Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        if self.card_type.borrow().is_none() {
            return Err(SdError::CardNotFound);
        }

        let start_idx = self.adjust_block_idx(start_block_idx);

        RawSpi::cs_low();

        if blocks.len() == 1 {
            let _r1 = Self::cmd_sticky(CMD24, start_idx)?;
            Self::write_data_block(&blocks[0].contents)?;
        } else {
            for (i, block) in blocks.iter().enumerate() {
                let _r1 = Self::cmd_sticky(CMD24, start_idx + i as u32)?;
                Self::write_data_block(&block.contents)?;
            }
        }

        RawSpi::cs_high();
        let _ = spi_read_byte();
        Ok(())
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        if self.card_type.borrow().is_none() {
            return Err(SdError::CardNotFound);
        }
        let csd = self.read_csd()?;
        let csd_ver = (csd[0] >> 6) & 0x03;
        if csd_ver == 1 {
            let size = ((csd[7] as u32 & 0x3F) << 16) | (csd[8] as u32) << 8 | csd[9] as u32;
            Ok(BlockCount((size + 1) * 1024))
        } else {
            let c_size = ((csd[6] as u32 & 0x03) << 10)
                | (csd[7] as u32) << 2
                | ((csd[8] as u32 >> 6) & 0x03);
            let c_size_mult = ((csd[9] as u32 & 0x03) << 1) | ((csd[10] as u32 >> 7) & 0x01);
            let read_bl_len = (csd[5] & 0x0F) as u32;
            let mult = c_size_mult + read_bl_len - 7;
            Ok(BlockCount((c_size + 1) << mult))
        }
    }
}

// ── SPI helpers (no CS toggling) ───────────────────────────────────────────────

fn spi_write(buf: &[u8]) {
    RawSpi::transfer(Some(buf), None::<&mut [u8]>, buf.len());
}

fn spi_read_byte() -> u8 {
    let mut r = [0xFF];
    RawSpi::transfer(Some(&[0xFF]), Some(&mut r), 1);
    r[0]
}

fn spi_transfer_in_place(buf: &mut [u8]) {
    RawSpi::transfer_in_place(buf, buf.len());
}

// ── FAT32 geometry (parsed from MBR + BPB before VolumeManager takes ownership) ─

#[derive(Clone, Copy)]
pub struct Fat32Info {
    pub bytes_per_sector: u16,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub num_fats: u8,
    pub fat_size_sectors: u32,
    pub total_sectors: u32,
    pub root_cluster: u32,
    pub data_area_start: u32,
    pub part_start_sector: u32,
}

static mut IS_SDHC: bool = false;
static mut FAT32_INFO: Option<Fat32Info> = None;

/// Parse MBR partition table + FAT32 BPB to extract geometry.
/// Must be called BEFORE VolumeManager takes ownership of the SD card.
unsafe fn init_fat32_geometry(is_sdhc: bool) -> Result<(), &'static str> {
    IS_SDHC = is_sdhc;
    let mut sector = [0u8; 512];

    // Read MBR (sector 0)
    raw_read_sector(0, &mut sector)?;

    // MBR partition entry 1 starts at offset 0x1BE
    let part_lba = u32::from_le_bytes([
        sector[0x1C6], sector[0x1C7], sector[0x1C8], sector[0x1C9],
    ]);

    // Read BPB from partition start sector
    raw_read_sector(part_lba, &mut sector)?;

    let bpb_signature = sector[82];
    if &sector[82..87] != b"FAT32" && bpb_signature != 0x29 {
        crate::println!("[SD] Not FAT32 (sig=0x{:02X})", bpb_signature);
        return Err("Not FAT32");
    }

    let info = Fat32Info {
        bytes_per_sector: u16::from_le_bytes([sector[11], sector[12]]),
        sectors_per_cluster: sector[13],
        reserved_sectors: u16::from_le_bytes([sector[14], sector[15]]),
        num_fats: sector[16],
        fat_size_sectors: u32::from_le_bytes([sector[36], sector[37], sector[38], sector[39]]),
        total_sectors: u32::from_le_bytes([sector[32], sector[33], sector[34], sector[35]]),
        root_cluster: u32::from_le_bytes([sector[44], sector[45], sector[46], sector[47]]),
        data_area_start: part_lba + (sector[14] as u32 | ((sector[15] as u32) << 8))
            + (sector[16] as u32) * (sector[36] as u32 | ((sector[37] as u32) << 8)
                | ((sector[38] as u32) << 16) | ((sector[39] as u32) << 24)),
        part_start_sector: part_lba,
    };

    crate::println!("[SD] FAT32: {}B/sector, {} sect/cluster, root_cluster={}, FAT_start={}",
        info.bytes_per_sector, info.sectors_per_cluster, info.root_cluster, info.part_start_sector);

    FAT32_INFO = Some(info);
    Ok(())
}

/// Read a single raw sector (512 bytes) using CMD17, bypassing VolumeManager.
unsafe fn raw_read_sector(sector_idx: u32, buf: &mut [u8; 512]) -> Result<(), &'static str> {
    let block_idx = if IS_SDHC { sector_idx } else { sector_idx * 512 };

    RawSpi::cs_low();
    RawSdCard::cmd_sticky(CMD17, block_idx).map_err(|_| "CMD17 fail")?;
    RawSdCard::read_data_block(buf).map_err(|_| "Data read fail")?;
    RawSpi::cs_high();
    let _ = spi_read_byte();
    Ok(())
}

/// Follow the FAT32 cluster chain, return the sector index for a given cluster.
fn cluster_to_sector(cluster: u32, info: &Fat32Info) -> u32 {
    info.data_area_start + (cluster - 2) as u32 * info.sectors_per_cluster as u32
}

/// Read the next cluster number from the FAT for a given cluster.
fn next_cluster(cluster: u32, info: &Fat32Info) -> Result<u32, &'static str> {
    // Each FAT entry is 4 bytes. Byte offset = cluster * 4.
    let fat_offset = cluster * 4;
    let fat_sector = info.part_start_sector + info.reserved_sectors as u32 + (fat_offset / 512);
    let entry_offset = (fat_offset % 512) as usize;

    let mut sector = [0u8; 512];
    unsafe { raw_read_sector(fat_sector, &mut sector)?; }

    let raw = u32::from_le_bytes([
        sector[entry_offset], sector[entry_offset+1],
        sector[entry_offset+2], sector[entry_offset+3],
    ]);
    let next = raw & 0x0FFFFFFF; // top 4 bits reserved
    if next >= 0x0FFFFFF8 {
        Ok(0) // end of chain
    } else {
        Ok(next)
    }
}

/// Compute the FAT checksum for a short filename (8.3).
fn lfn_checksum(sfn11: &[u8; 11]) -> u8 {
    let mut sum = 0u8;
    for &b in sfn11.iter() {
        sum = sum.rotate_right(1).wrapping_add(b);
    }
    sum
}

/// Extract 13 LFN chars from one LFN directory entry directly into lfn_buf.
/// lfn_pos is the starting position in lfn_buf (seq-1)*13.
/// Returns the number of valid chars written.
fn lfn_extract(e: &[u8], lfn_buf: &mut [u8], lfn_pos: usize) -> usize {
    // LFN char offsets in entry: 1,3,5,7,9 (5), 14,16,18,20,22,24 (6), 28,30 (2)
    const OFFS: [usize; 13] = [1,3,5,7,9, 14,16,18,20,22,24, 28,30];
    let mut count = 0;
    for &o in OFFS.iter() {
        let lo = e[o];
        let hi = e[o + 1];
        if lo == 0x00 && hi == 0x00 { break; }
        if lo == 0xFF && hi == 0xFF { break; }
        if lfn_pos + count < lfn_buf.len() {
            lfn_buf[lfn_pos + count] = if hi != 0 { b'?' } else { lo };
        }
        count += 1;
    }
    count
}

/// Check if LFN matches name (case-insensitive ASCII).
fn lfn_matches(lfn_buf: &[u8], lfn_len: usize, name: &str) -> bool {
    let nb = name.as_bytes();
    if lfn_len != nb.len() { return false; }
    for i in 0..lfn_len {
        if lfn_buf[i].to_ascii_uppercase() != nb[i].to_ascii_uppercase() { return false; }
    }
    true
}

/// Build an 11-byte SFN from a name like "MANIFEST.TXT" or "LED_BLINK".
/// Handles long names by generating tilde notation like FAT32 does.
fn make_sfn_upper(name: &str) -> [u8; 11] {
    let mut buf = [b' '; 11];
    let b = name.as_bytes();
    let dot = b.iter().rposition(|&c| c == b'.');
    match dot {
        Some(dp) => {
            let base_len = dp;
            let ext_len = b.len() - dp - 1;
            if base_len <= 8 && ext_len <= 3 {
                for i in 0..base_len { buf[i] = b[i].to_ascii_uppercase(); }
                for i in 0..ext_len { buf[8 + i] = b[dp + 1 + i].to_ascii_uppercase(); }
            } else {
                let take = base_len.min(6);
                for i in 0..take { buf[i] = b[i].to_ascii_uppercase(); }
                buf[6] = b'~';
                buf[7] = b'1';
                let ext_take = ext_len.min(3);
                for i in 0..ext_take { buf[8 + i] = b[dp + 1 + i].to_ascii_uppercase(); }
            }
        }
        None => {
            let n = b.len();
            if n <= 8 {
                for i in 0..n { buf[i] = b[i].to_ascii_uppercase(); }
            } else {
                for i in 0..6 { buf[i] = b[i].to_ascii_uppercase(); }
                buf[6] = b'~';
                buf[7] = b'1';
            }
        }
    }
    buf
}

/// Print an 8.3 short filename from an 11-byte SFN, stripping trailing spaces.
fn print_sfn(sfn: &[u8]) {
    let uart = crate::drivers::uart::RawUart;
    let mut end = 8;
    while end > 0 && sfn[end - 1] == b' ' { end -= 1; }
    for i in 0..end { uart.write_byte(sfn[i]); }
    let mut ext_end = 11;
    while ext_end > 8 && sfn[ext_end - 1] == b' ' { ext_end -= 1; }
    if ext_end > 8 {
        uart.write_byte(b'.');
        for i in 8..ext_end { uart.write_byte(sfn[i]); }
    }
}

/// Navigate from root to a path component, returning the starting cluster.
fn find_cluster_for_path(path: &str) -> Result<u32, &'static str> {
    let info = unsafe { FAT32_INFO.ok_or("ERR_NO_SD")? };
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    if trimmed.is_empty() { return Ok(info.root_cluster); }
    let mut cluster = info.root_cluster;
    
    crate::print!("[SD] find_cluster_for_path: trimmed='{}' bytes=[", trimmed);
    for &b in trimmed.as_bytes() {
        crate::print!("{}, ", b);
    }
    crate::println!("]");

    for component in trimmed.split('/') {
        if component.is_empty() { continue; }
        
        crate::print!("[SD]   component: '{}' bytes=[", component);
        for &b in component.as_bytes() {
            crate::print!("{}, ", b);
        }
        crate::println!("]");

        match find_in_dir(cluster, component, &info) {
            Ok(c) => {
                crate::println!("[SD]   found component '{}' -> cluster={}", component, c);
                cluster = c;
            }
            Err(e) => {
                crate::println!("[SD]   failed to find component '{}': {}", component, e);
                return Err(e);
            }
        }
    }
    Ok(cluster)
}

/// Compare an SFN entry against a name (LFN match or direct SFN match).
fn sfn_entry_matches(e: &[u8], name: &str, lfn_buf: &[u8], lfn_len: usize, lfn_ck: u8) -> bool {
    let attr = e[11];
    if attr == 0x0F { return false; }
    let mut sfn = [0u8; 11];
    sfn.copy_from_slice(&e[0..11]);
    // LFN match
    if lfn_len > 0 && lfn_ck == lfn_checksum(&sfn) {
        if lfn_matches(lfn_buf, lfn_len, name) { return true; }
    }
    // Direct SFN match
    let target = make_sfn_upper(name);
    sfn == target
}

/// Get cluster number from a 32-byte directory entry.
fn entry_cluster(e: &[u8]) -> u32 {
    (e[20] as u32) << 16 | (e[26] as u32)
}

/// Get file size from a 32-byte directory entry.
fn entry_size(e: &[u8]) -> u32 {
    u32::from_le_bytes([e[28], e[29], e[30], e[31]])
}

/// Process one 32-byte dir entry for LFN accumulation.
/// Updates lfn_len and lfn_ck as needed. Returns true if this was an LFN entry.
fn process_lfn_entry(e: &[u8], lfn_buf: &mut [u8], lfn_len: &mut usize, lfn_ck: &mut u8) -> bool {
    if e[11] != 0x0F || e[0] == 0xE5 || e[0] == 0x00 { return false; }
    let order = e[0];
    if order & 0x40 != 0 {
        *lfn_len = 0;
        *lfn_ck = e[13];
    }
    let seq = (order & 0x3F) as usize;
    let pos = (seq - 1) * 13;
    let written = lfn_extract(e, lfn_buf, pos);
    let end = pos + written;
    if end > *lfn_len { *lfn_len = end; }
    true
}

/// Search a directory cluster for an entry matching `name`.
fn find_in_dir(cluster: u32, name: &str, info: &Fat32Info) -> Result<u32, &'static str> {
    let mut current = cluster;
    let mut lfn_buf = [0u8; 256];
    let mut lfn_len: usize = 0;
    let mut lfn_ck: u8 = 0;
    let spc = info.sectors_per_cluster as u32;

    loop {
        let sec0 = cluster_to_sector(current, info);
        let mut end_hit = false;
        for s in 0..spc {
            let mut sector = [0u8; 512];
            unsafe { raw_read_sector(sec0 + s, &mut sector)?; }
            for i in 0..16u32 {
                let o = (i * 32) as usize;
                let e = &sector[o..o + 32];
                if e[0] == 0x00 { end_hit = true; break; }
                if e[0] == 0xE5 { lfn_len = 0; continue; }
                if process_lfn_entry(e, &mut lfn_buf, &mut lfn_len, &mut lfn_ck) { continue; }
                if sfn_entry_matches(e, name, &lfn_buf, lfn_len, lfn_ck) {
                    return Ok(entry_cluster(e));
                }
                lfn_len = 0;
            }
            if end_hit { return Err("Not found"); }
        }
        current = next_cluster(current, info)?;
        if current == 0 { return Err("Not found"); }
    }
}

/// Print a 32-byte dir entry using LFN if available, else SFN.
fn print_dir_entry(e: &[u8], lfn_buf: &[u8], lfn_len: usize, lfn_ck: u8) {
    let mut sfn = [0u8; 11];
    sfn.copy_from_slice(&e[0..11]);
    let is_dir = e[11] & 0x10 != 0;
    crate::tty::write_both(b' ');
    if lfn_len > 0 && lfn_ck == lfn_checksum(&sfn) {
        let uart = crate::drivers::uart::RawUart;
        for i in 0..lfn_len { uart.write_byte(lfn_buf[i]); }
    } else {
        print_sfn(&sfn);
    }
    if is_dir {
        crate::tty::write_str_both("/\n");
    } else {
        crate::tty::write_str_both("  ");
        write_u32_to_tty(entry_size(e));
        crate::tty::write_str_both(" bytes\n");
    }
}

pub fn list_dir(path: &str) -> Result<(), &'static str> {
    let info = unsafe { FAT32_INFO.ok_or("ERR_NO_SD")? };
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    let cluster = find_cluster_for_path(trimmed)?;

    crate::println!("Files on {}:", if trimmed.is_empty() { "/" } else { trimmed });

    let mut current = cluster;
    let mut lfn_buf = [0u8; 256];
    let mut lfn_len: usize = 0;
    let mut lfn_ck: u8 = 0;
    let mut total: u32 = 0;
    let spc = info.sectors_per_cluster as u32;

    'clusters: loop {
        let sec0 = cluster_to_sector(current, &info);
        for s in 0..spc {
            let mut sector = [0u8; 512];
            unsafe { raw_read_sector(sec0 + s, &mut sector)?; }
            for i in 0..16u32 {
                let o = (i * 32) as usize;
                let e = &sector[o..o + 32];
                if e[0] == 0x00 { break 'clusters; }
                if e[0] == 0xE5 { lfn_len = 0; continue; }
                if process_lfn_entry(e, &mut lfn_buf, &mut lfn_len, &mut lfn_ck) { continue; }
                if e[11] == 0x0F { lfn_len = 0; continue; }
                print_dir_entry(e, &lfn_buf, lfn_len, lfn_ck);
                total += 1;
                lfn_len = 0;
            }
        }
        current = next_cluster(current, &info)?;
        if current == 0 { break; }
    }

    crate::tty::write_str_both("(");
    write_u32_to_tty(total);
    crate::tty::write_str_both(" items)\n");
    Ok(())
}

fn write_u32_to_tty(mut v: u32) {
    if v == 0 {
        crate::tty::write_both(b'0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut d = 0;
    while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
    let mut i = d;
    while i > 0 { i -= 1; crate::tty::write_both(digits[i]); }
}

// ── Volume manager type alias ──────────────────────────────────────────────────

type VolumeManagerType = VolumeManager<RawSdCard, DummyTimeSource>;

pub static mut VOLUME_MGR: Option<VolumeManagerType> = None;

// ── Windows 8.3 short filename generation ──────────────────────────────────────

fn generate_sfn(name: &str) -> Result<embedded_sdmmc::ShortFileName, &'static str> {
    let name_bytes = name.as_bytes();
    let len = name_bytes.len();
    if len == 0 || len > 255 {
        return Err("Invalid filename");
    }
    let mut upper = [0u8; 256];
    for i in 0..len {
        upper[i] = name_bytes[i].to_ascii_uppercase();
    }
    let dot_pos = upper[..len].iter().rposition(|&b| b == b'.');
    let (base_part, ext_part) = match dot_pos {
        Some(pos) => (&upper[..pos], &upper[pos + 1..len]),
        None => (&upper[..len], &[] as &[u8]),
    };
    let base_len = base_part.len();
    let ext_len = ext_part.len();
    let mut sfn_bytes = [b' '; 11];
    if base_len <= 8 && ext_len <= 3 && dot_pos.is_some() {
        sfn_bytes[..base_len].copy_from_slice(&base_part[..base_len]);
        sfn_bytes[8..8 + ext_len].copy_from_slice(&ext_part[..ext_len]);
    } else {
        let take = base_len.min(6);
        sfn_bytes[..take].copy_from_slice(&base_part[..take]);
        sfn_bytes[6] = b'~';
        sfn_bytes[7] = b'1';
        let ext_take = ext_len.min(3);
        sfn_bytes[8..8 + ext_take].copy_from_slice(&ext_part[..ext_take]);
    }
    Ok(unsafe { core::mem::transmute(sfn_bytes) })
}

fn try_open_dir(
    mgr: &mut VolumeManagerType,
    parent_dir: embedded_sdmmc::RawDirectory,
    name: &str,
) -> Result<embedded_sdmmc::RawDirectory, &'static str> {
    if let Ok(dir) = mgr.open_dir(parent_dir, name) {
        return Ok(dir);
    }
    if let Ok(sfn) = generate_sfn(name) {
        if let Ok(dir) = mgr.open_dir(parent_dir, &sfn) {
            return Ok(dir);
        }
    }
    Err("Directory not found")
}

fn try_open_file(
    mgr: &mut VolumeManagerType,
    parent_dir: embedded_sdmmc::RawDirectory,
    name: &str,
    mode: embedded_sdmmc::Mode,
) -> Result<embedded_sdmmc::RawFile, &'static str> {
    if let Ok(f) = mgr.open_file_in_dir(parent_dir, name, mode) {
        return Ok(f);
    }
    if let Ok(sfn) = generate_sfn(name) {
        if let Ok(f) = mgr.open_file_in_dir(parent_dir, &sfn, mode) {
            return Ok(f);
        }
    }
    Err("File not found")
}

fn split_path(path: &str) -> (&str, &str) {
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
        None => ("", trimmed),
    }
}

// ── Public API ─────────────────────────────────────────────────────────────────

pub fn init_fs() -> Result<(), &'static str> {
    crate::println!("[SD] Initializing RawSdCard...");
    let sdcard = RawSdCard::new();
    sdcard.init().map_err(|e| {
        crate::println!("[SD] Card init failed: {:?}", e);
        "ERR_CARD_NOT_FOUND"
    })?;
    let is_sdhc = sdcard.is_sdhc();
    unsafe { init_fat32_geometry(is_sdhc)?; }
    crate::println!("[SD] Card initialized, creating VolumeManager...");
    let volume_mgr = VolumeManager::new(sdcard, DummyTimeSource);
    unsafe {
        VOLUME_MGR = Some(volume_mgr);
        let mgr = (*(&raw mut VOLUME_MGR)).as_mut().unwrap();
        crate::println!("[SD] Opening volume 0...");
        match mgr.open_volume(VolumeIdx(0)) {
            Ok(_) => {
                crate::println!("[SD] Volume 0 mounted OK");
                RawSpi::set_speed_high();
                crate::println!("[SD] Switched to 10 MHz");
                Ok(())
            }
            Err(e) => {
                crate::println!("[SD] open_volume(0) failed: {:?}", e);
                VOLUME_MGR = None;
                Err("ERR_VOLUME_MOUNT_FAILED")
            }
        }
    }
}

pub fn is_mounted() -> bool {
    unsafe {
        let ptr = &raw const VOLUME_MGR;
        (*ptr).is_some()
    }
}

pub fn touch_file(path: &str) -> Result<(), &'static str> {
    unsafe {
        let mgr = (*(&raw mut VOLUME_MGR)).as_mut().ok_or("ERR_NO_SD")?;
        let volume = mgr.open_volume(VolumeIdx(0)).map_err(|_| "Failed to open Volume 0")?;
        let raw_vol = volume.to_raw_volume();
        let mut raw_dir = mgr.open_root_dir(raw_vol).map_err(|_| "Failed to open root directory")?;
        let (parent, name) = split_path(path);
        if !parent.is_empty() {
            for component in parent.split('/') {
                if component.is_empty() { continue; }
                let next = try_open_dir(mgr, raw_dir, component)?;
                let _ = mgr.close_dir(raw_dir);
                raw_dir = next;
            }
        }
        let raw_file = try_open_file(mgr, raw_dir, name, embedded_sdmmc::Mode::ReadWriteCreateOrTruncate)
            .map_err(|_| "Failed to create file")?;
        let _ = mgr.close_file(raw_file);
        let _ = mgr.close_dir(raw_dir);
        let _ = mgr.close_volume(raw_vol);
        Ok(())
    }
}

pub fn write_file(path: &str, content: &[u8]) -> Result<(), &'static str> {
    unsafe {
        let mgr = (*(&raw mut VOLUME_MGR)).as_mut().ok_or("ERR_NO_SD")?;
        let volume = mgr.open_volume(VolumeIdx(0)).map_err(|_| "Failed to open Volume 0")?;
        let raw_vol = volume.to_raw_volume();
        let mut raw_dir = mgr.open_root_dir(raw_vol).map_err(|_| "Failed to open root directory")?;
        let (parent, name) = split_path(path);
        if !parent.is_empty() {
            for component in parent.split('/') {
                if component.is_empty() { continue; }
                let next = try_open_dir(mgr, raw_dir, component)?;
                let _ = mgr.close_dir(raw_dir);
                raw_dir = next;
            }
        }
        let raw_file = try_open_file(mgr, raw_dir, name, embedded_sdmmc::Mode::ReadWriteCreateOrTruncate)
            .map_err(|_| "Failed to open file for writing")?;
        mgr.write(raw_file, content).map_err(|_| "Failed to write to file")?;
        let _ = mgr.close_file(raw_file);
        let _ = mgr.close_dir(raw_dir);
        let _ = mgr.close_volume(raw_vol);
        Ok(())
    }
}

/// Search a directory cluster for an entry, returning (cluster, file_size).
fn find_in_dir_with_size(cluster: u32, name: &str, info: &Fat32Info) -> Result<(u32, u32), &'static str> {
    let mut current = cluster;
    let mut lfn_buf = [0u8; 256];
    let mut lfn_len: usize = 0;
    let mut lfn_ck: u8 = 0;
    let spc = info.sectors_per_cluster as u32;

    loop {
        let sec0 = cluster_to_sector(current, info);
        let mut end_hit = false;
        for s in 0..spc {
            let mut sector = [0u8; 512];
            unsafe { raw_read_sector(sec0 + s, &mut sector)?; }
            for i in 0..16u32 {
                let o = (i * 32) as usize;
                let e = &sector[o..o + 32];
                if e[0] == 0x00 { end_hit = true; break; }
                if e[0] == 0xE5 { lfn_len = 0; continue; }
                if process_lfn_entry(e, &mut lfn_buf, &mut lfn_len, &mut lfn_ck) { continue; }
                if sfn_entry_matches(e, name, &lfn_buf, lfn_len, lfn_ck) {
                    return Ok((entry_cluster(e), entry_size(e)));
                }
                lfn_len = 0;
            }
            if end_hit { return Err("Not found"); }
        }
        current = next_cluster(current, info)?;
        if current == 0 { return Err("Not found"); }
    }
}

fn find_file_info(path: &str) -> Result<(u32, u32), &'static str> {
    let info = unsafe { FAT32_INFO.ok_or("ERR_NO_SD")? };
    let trimmed = path.trim_start_matches('/').trim_end_matches('/');
    if trimmed.is_empty() { return Err("ERR_NO_SD"); }

    // Find the parent directory cluster
    let last_slash = trimmed.rfind('/');
    let (parent_path, filename) = match last_slash {
        Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
        None => ("", trimmed),
    };

    crate::println!("[SD] find_file_info: path='{}', parent='{}', filename='{}'",
        path, parent_path, filename);

    let parent_cluster = if parent_path.is_empty() {
        info.root_cluster
    } else {
        match find_cluster_for_path(parent_path) {
            Ok(c) => c,
            Err(e) => {
                crate::println!("[SD] find_cluster_for_path failed: {}", e);
                return Err(e);
            }
        }
    };

    match find_in_dir_with_size(parent_cluster, filename, &info) {
        Ok((c, s)) => {
            crate::println!("[SD] found: cluster={}, size={}", c, s);
            Ok((c, s))
        }
        Err(e) => {
            crate::println!("[SD] find_in_dir_with_size failed: {}", e);
            Err(e)
        }
    }
}

pub fn read_file_to_buf(path: &str, buf: &mut [u8]) -> Result<usize, &'static str> {
    let (cluster, file_size) = find_file_info(path)?;
    let info = unsafe { FAT32_INFO.ok_or("ERR_NO_SD")? };

    let mut current = cluster;
    let spc = info.sectors_per_cluster as u32;
    let mut offset = 0usize;
    let file_size = file_size as usize;

    loop {
        let sec0 = cluster_to_sector(current, &info);
        for s in 0..spc {
            if offset >= buf.len() || offset >= file_size { break; }
            let mut sector = [0u8; 512];
            unsafe { raw_read_sector(sec0 + s, &mut sector)?; }
            let remaining = core::cmp::min(file_size - offset, buf.len() - offset);
            let copy_len = core::cmp::min(512, remaining);
            buf[offset..offset + copy_len].copy_from_slice(&sector[..copy_len]);
            offset += copy_len;
        }
        if offset >= buf.len() || offset >= file_size { break; }
        current = next_cluster(current, &info)?;
        if current == 0 { break; }
    }
    Ok(offset)
}

pub fn delete_file(path: &str) -> Result<(), &'static str> {
    unsafe {
        let mgr = (*(&raw mut VOLUME_MGR)).as_mut().ok_or("ERR_NO_SD")?;
        let volume = mgr.open_volume(VolumeIdx(0)).map_err(|_| "Failed to open Volume 0")?;
        let raw_vol = volume.to_raw_volume();
        let mut raw_dir = mgr.open_root_dir(raw_vol).map_err(|_| "Failed to open root directory")?;
        let (parent, name) = split_path(path);
        if !parent.is_empty() {
            for component in parent.split('/') {
                if component.is_empty() { continue; }
                let next = try_open_dir(mgr, raw_dir, component)?;
                let _ = mgr.close_dir(raw_dir);
                raw_dir = next;
            }
        }
        if mgr.delete_file_in_dir(raw_dir, name).is_err() {
            if let Ok(sfn) = generate_sfn(name) {
                mgr.delete_file_in_dir(raw_dir, &sfn).map_err(|_| "Failed to delete file")?;
            } else {
                return Err("Failed to delete file");
            }
        }
        let _ = mgr.close_dir(raw_dir);
        let _ = mgr.close_volume(raw_vol);
        Ok(())
    }
}

pub fn cat_file(path: &str) -> Result<(), &'static str> {
    let mut buf = [0u8; 4096];
    let n = read_file_to_buf(path, &mut buf)?;
    let uart = crate::drivers::uart::RawUart;
    for &b in &buf[..n] {
        if b == b'\n' {
            uart.write_byte(b'\r');
        }
        uart.write_byte(b);
    }
    crate::println!();
    Ok(())
}
