//! TTY module - terminal abstraction layer for display and serial I/O
//!
//! Provides unified interface to both SSD1306 display and UART0 backends.
//! Handles input from PS/2 keyboard and UART RX.

pub mod backend {
    pub enum TtyBackend {
        Uart,
        Display,
    }

    impl TtyBackend {
        pub fn write_byte(&self, b: u8) {
            match self {
                TtyBackend::Uart => {
                    let uart = crate::drivers::uart::RawUart;
                    uart.write_byte(b);
                }
                TtyBackend::Display => {
                    unsafe {
                        crate::display::GRID.put_char(b);
                        crate::display::render(&crate::display::GRID);
                    }
                }
            }
        }
    }
}

pub const TTY_BUF_SIZE: usize = 1024;

/// Write a byte to both backends (UART + display).
pub fn write_both(b: u8) {
    backend::TtyBackend::Uart.write_byte(b);
    backend::TtyBackend::Display.write_byte(b);
}

/// Write a string to both backends.
pub fn write_str_both(s: &str) {
    for &b in s.as_bytes() {
        write_both(b);
    }
}
