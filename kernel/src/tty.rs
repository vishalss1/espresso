//! TTY module — unified terminal abstraction.
//!
//! Input: polls UART RX.
//! Output: writes to UART.

pub mod backend {
    pub enum TtyBackend {
        Uart,
    }

    impl TtyBackend {
        pub fn write_byte(&self, b: u8) {
            match self {
                TtyBackend::Uart => {
                    let uart = crate::drivers::uart::RawUart;
                    uart.write_byte(b);
                }
            }
        }
    }
}

pub const TTY_BUF_SIZE: usize = 1024;

/// Write a byte to the TTY backend (UART).
pub fn write_both(b: u8) {
    backend::TtyBackend::Uart.write_byte(b);
}

/// Write a string to the TTY backend.
pub fn write_str_both(s: &str) {
    for &b in s.as_bytes() {
        write_both(b);
    }
}

/// Poll UART for one byte of input.
/// Returns the first available byte, or None if nothing pending.
pub fn poll_read() -> Option<u8> {
    let uart = crate::drivers::uart::RawUart;
    uart.read_byte()
}
