#![no_std]
//! TTY module - terminal abstraction layer for display and serial I/O
//! 
//! Provides unified interface to both SSD1306 display and UART0 backends.
//! Handles input from PS/2 keyboard and UART RX.

pub mod backend;

pub const TTY_BUF_SIZE: usize = 1024;