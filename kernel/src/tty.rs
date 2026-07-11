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
                    // Phase 2: SSD1306 display backend
                }
            }
        }
    }
}

pub const TTY_BUF_SIZE: usize = 1024;
