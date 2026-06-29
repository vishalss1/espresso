#![no_std]
//! I2C driver module - two-wire serial interface
//! 
//! Provides I2C master functionality for SSD1306 display control.
//! Clock speed: 400KHz (fast mode).

pub struct I2c {
    sda: u32,
    scl: u32,
}

impl I2c {
    pub fn init() {
        // TODO: Initialize I2C on GPIO21 (SDA) and GPIO22 (SCL)
    }
}