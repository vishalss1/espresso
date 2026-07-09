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

// ── Volume manager type alias ──────────────────────────────────────────────────

type VolumeManagerType = VolumeManager<RawSdCard, DummyTimeSource>;

pub static mut VOLUME_MGR: Option<VolumeManagerType> = None;

// ── Public API ─────────────────────────────────────────────────────────────────

pub fn init_fs() -> Result<(), &'static str> {
    crate::println!("[SD] Initializing RawSdCard...");
    let sdcard = RawSdCard::new();
    sdcard.init().map_err(|e| {
        crate::println!("[SD] Card init failed: {:?}", e);
        "ERR_CARD_NOT_FOUND"
    })?;

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

pub fn list_dir(_path: &str) -> Result<(), &'static str> {
    unsafe {
        let mgr = (*(&raw mut VOLUME_MGR)).as_mut().ok_or("ERR_NO_SD")?;
        let mut volume = mgr.open_volume(VolumeIdx(0)).map_err(|e| {
            crate::println!("  [SD] open_volume error: {:?}", e);
            "Failed to open Volume 0"
        })?;
        let mut root_dir = volume.open_root_dir().map_err(|_| "Failed to open root directory")?;

        crate::println!("Files on SD card root:");
        root_dir
            .iterate_dir(|entry| {
                let dir_indicator = if entry.attributes.is_directory() { "/" } else { "" };
                crate::println!("  {}{:<24}  {} bytes", entry.name, dir_indicator, entry.size);
            })
            .map_err(|_| "Failed to iterate directory")?;

        Ok(())
    }
}

pub fn cat_file(path: &str) -> Result<(), &'static str> {
    unsafe {
        let mgr = (*(&raw mut VOLUME_MGR)).as_mut().ok_or("ERR_NO_SD")?;
        let mut volume = mgr.open_volume(VolumeIdx(0)).map_err(|e| {
            crate::println!("  [SD] open_volume error: {:?}", e);
            "Failed to open Volume 0"
        })?;
        let mut root_dir = volume.open_root_dir().map_err(|_| "Failed to open root directory")?;
        let mut file = root_dir
            .open_file_in_dir(path, embedded_sdmmc::Mode::ReadOnly)
            .map_err(|_| "Failed to open file")?;

        let mut buf = [0u8; 128];
        loop {
            let n = file.read(&mut buf).map_err(|_| "Failed to read file")?;
            if n == 0 {
                break;
            }
            let uart = crate::drivers::uart::RawUart;
            for &b in &buf[..n] {
                if b == b'\n' {
                    uart.write_byte(b'\r');
                }
                uart.write_byte(b);
            }
        }
        crate::println!();
        Ok(())
    }
}
