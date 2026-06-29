#![no_std]
//! SPI driver module - serial peripheral interface
//! 
//! Provides SPI master functionality for SD card communication.
//! Dedicated HSPI controller on ESP32.

pub struct Spi {
    cs: u32,
    sck: u32,
    mosi: u32,
    miso: u32,
}

impl Spi {
    pub fn init() {
        // TODO: Initialize SPI on GPIO15, GPIO14, GPIO13, GPIO12(or 19)
    }
}