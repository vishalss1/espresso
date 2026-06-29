#![no_std]
//! Display driver module - SSD1306 OLED display driver
//! 
//! Provides 128x64 monochrome display functionality over I2C.
//! Implements character tile rendering with scroll buffer.

use crate::display::controller::SSD1306;

pub const SCREEN_WIDTH: u8 = 128;
pub const SCREEN_HEIGHT: u8 = 64;
pub const CHAR_WIDTH: u8 = 6;
pub const CHAR_HEIGHT: u8 = 8;