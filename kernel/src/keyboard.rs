#![no_std]
//! Keyboard driver module - PS/2 keyboard interface
//! 
//! Provides PS/2 input interface using GPIO34 (CLK) and GPIO35 (DATA).
//! Converts scan codes to ASCII and feeds into TTY input buffer.

pub const PS2_CLK_GPIO: u8 = 34;
pub const PS2_DATA_GPIO: u8 = 35;

pub fn init() {
    // TODO: Initialize PS/2 input
}