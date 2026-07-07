use crate::drivers::spi::RawSpi;
use crate::drivers::delay::RawDelay;
use embedded_hal::delay::DelayNs;
use embedded_sdmmc::{SdCard, VolumeManager, VolumeIdx, Mode, TimeSource, Timestamp};

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

use embedded_sdmmc::{Block, BlockIdx, BlockCount, BlockDevice};

pub struct DiagnosticBlockDevice<T: BlockDevice> {
    inner: T,
}

impl<T: BlockDevice> BlockDevice for DiagnosticBlockDevice<T> {
    type Error = T::Error;

    fn read(&self, blocks: &mut [Block], start_block_idx: BlockIdx, reason: &str) -> Result<(), Self::Error> {
        crate::println!("[SD Block Device] read block start={:?} (count={}) for: {}", start_block_idx, blocks.len(), reason);
        let res = self.inner.read(blocks, start_block_idx, reason);
        match &res {
            Ok(()) => crate::println!("[SD Block Device] read block success"),
            Err(e) => crate::println!("[SD Block Device] read block error: {:?}", e),
        }
        res
    }

    fn write(&self, blocks: &[Block], start_block_idx: BlockIdx) -> Result<(), Self::Error> {
        crate::println!("[SD Block Device] write block start={:?} (count={})", start_block_idx, blocks.len());
        let res = self.inner.write(blocks, start_block_idx);
        match &res {
            Ok(()) => crate::println!("[SD Block Device] write block success"),
            Err(e) => crate::println!("[SD Block Device] write block error: {:?}", e),
        }
        res
    }

    fn num_blocks(&self) -> Result<BlockCount, Self::Error> {
        crate::println!("[SD Block Device] num_blocks query...");
        let res = self.inner.num_blocks();
        match &res {
            Ok(count) => crate::println!("[SD Block Device] num_blocks count={:?}", count),
            Err(e) => crate::println!("[SD Block Device] num_blocks error: {:?}", e),
        }
        res
    }
}

type SdCardType = SdCard<RawSpi, RawDelay>;
type DiagBlockDeviceType = DiagnosticBlockDevice<SdCardType>;
type VolumeManagerType = VolumeManager<DiagBlockDeviceType, DummyTimeSource>;

pub static mut FILE_SYSTEM: Option<VolumeManagerType> = None;

pub fn init_fs() -> Result<(), &'static str> {
    let mut delay = RawDelay;
    delay.delay_ms(500);

    // Diagnostic: run the full SD init sequence via raw SPI commands
    // This prints per-command status so we can see exactly where init fails.
    crate::println!("[SD] Initiating card via raw SPI commands...");
    if let Err(e) = RawSpi::card_reset() {
        crate::println!("[SD] Raw card init failed: {}", e);
        return Err("ERR_RAW_INIT_FAILED");
    }
    crate::println!("[SD] Raw card init succeeded");

    // Hand off to embedded-sdmmc for the actual volume manager mount.
    crate::println!("[SD] Calling SdCard::new()...");
    let sdcard = SdCard::new(RawSpi, RawDelay);
    crate::println!("[SD] SdCard::new() returned");

    crate::println!("[SD] Wrapping SdCard in DiagnosticBlockDevice...");
    let diag_card = DiagnosticBlockDevice { inner: sdcard };

    crate::println!("[SD] Calling VolumeManager::new()...");
    let volume_mgr = VolumeManager::new(diag_card, DummyTimeSource);
    crate::println!("[SD] VolumeManager::new() returned");

    unsafe {
        FILE_SYSTEM = Some(volume_mgr);
        let fs = (*(&raw mut FILE_SYSTEM)).as_mut().unwrap();
        crate::println!("[SD] Calling open_volume(VolumeIdx(0))...");
        let mount_res = fs.open_volume(VolumeIdx(0));
        crate::println!("[SD] open_volume(0) returned");
        match mount_res {
            Ok(_) => {
                crate::println!("[SD] Volume 0 mounted OK");
                RawSpi::set_speed_high(); // Switch to 10 MHz high speed post-mount
                Ok(())
            }
            Err(e) => {
                crate::println!("[SD] open_volume(0) failed: {:?}", e);
                FILE_SYSTEM = None;
                Err("ERR_VOLUME_MOUNT_FAILED")
            }
        }
    }
}

pub fn list_dir(_path: &str) -> Result<(), &'static str> {
    unsafe {
        let fs = (*(&raw mut FILE_SYSTEM)).as_mut().ok_or("ERR_NO_SD")?;
        let mut volume = fs.open_volume(VolumeIdx(0)).map_err(|_| "Failed to open Volume 0")?;
        let mut root_dir = volume.open_root_dir().map_err(|_| "Failed to open root directory")?;

        crate::println!("Files on SD card root:");
        root_dir.iterate_dir(|entry| {
            let dir_indicator = if entry.attributes.is_directory() { "/" } else { "" };
            crate::println!("  {}{:<24}  {} bytes", entry.name, dir_indicator, entry.size);
        }).map_err(|_| "Failed to iterate directory")?;

        Ok(())
    }
}

pub fn cat_file(path: &str) -> Result<(), &'static str> {
    unsafe {
        let fs = (*(&raw mut FILE_SYSTEM)).as_mut().ok_or("ERR_NO_SD")?;
        let mut volume = fs.open_volume(VolumeIdx(0)).map_err(|_| "Failed to open Volume 0")?;
        let mut root_dir = volume.open_root_dir().map_err(|_| "Failed to open root directory")?;
        let mut file = root_dir
            .open_file_in_dir(path, Mode::ReadOnly)
            .map_err(|_| "Failed to open file")?;

        let mut buf = [0u8; 128];
        loop {
            let n = file.read(&mut buf).map_err(|_| "Failed to read file")?;
            if n == 0 { break; }
            for &b in &buf[..n] {
                if b == b'\n' {
                    crate::drivers::uart::RawUart.write_byte(b'\r');
                }
                crate::drivers::uart::RawUart.write_byte(b);
            }
        }
        crate::println!();
        Ok(())
    }
}

pub fn is_mounted() -> bool {
    unsafe {
        let ptr = &raw const FILE_SYSTEM;
        (*ptr).is_some()
    }
}

