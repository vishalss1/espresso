#![no_std]
//! UART driver module - serial communication interface
//! 
//! Provides UART0 peripheral for terminal I/O.
//! Baud rate: 115200, 8N1 configuration.

pub struct Uart {
    tx: u32,
    rx: u32,
}

impl Uart {
    pub fn init() {
        // TODO: Initialize UART0 at 115200 baud
    }
}