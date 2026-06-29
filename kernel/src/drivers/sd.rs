#![no_std]
//! SD driver module - Secure Digital memory card interface
//! 
//! Provides SPI-based SD card communication for storage.
//! Uses embedded-sdmmc crate for FAT32 filesystem access.
//! CS pin on GPIO15 via dedicated HSPI peripheral.

pub struct SDCard {
    // SPI peripheral state
    spi: u32,
    // Card detection and initialization state
    initialized: bool,
}