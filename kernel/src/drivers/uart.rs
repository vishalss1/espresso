use core::fmt::Write;

// UART0 register constants (ESP32 Technical Reference Manual Section 14)
const UART0_FIFO_REG: *mut u32 = 0x3FF40000 as *mut u32;
const UART0_CLKDIV_REG: *mut u32 = 0x3FF40014 as *mut u32;
const UART0_STATUS_REG: *mut u32 = 0x3FF4001C as *mut u32;

pub struct RawUart;

impl RawUart {
    pub fn init() {
        unsafe {
            // Baud rate divisor for 115200 baud with 80MHz APB clock:
            // Divisor = 80,000,000 / 115,200 = 694.444
            // Integer part = 694 (0x2B6)
            // Fractional part = 0.444 * 16 = 7.1 -> 7 (0x7)
            // Register value = 694 | (7 << 20)
            core::ptr::write_volatile(UART0_CLKDIV_REG, 694 | (7 << 20));
        }
    }

    pub fn write_byte(&self, b: u8) {
        unsafe {
            // Wait until there is space in the TX FIFO (txfifo_cnt < 127)
            // txfifo_cnt is bits 16..23 of UART0_STATUS_REG
            loop {
                let status = core::ptr::read_volatile(UART0_STATUS_REG);
                let tx_count = (status >> 16) & 0xFF;
                if tx_count < 127 {
                    break;
                }
            }
            core::ptr::write_volatile(UART0_FIFO_REG, b as u32);
        }
    }

    pub fn write_bytes(&self, buf: &[u8]) {
        for &b in buf {
            self.write_byte(b);
        }
    }

    pub fn read_byte(&self) -> Option<u8> {
        unsafe {
            // Check if there are bytes in the RX FIFO (rxfifo_cnt > 0)
            // rxfifo_cnt is bits 0..7 of UART0_STATUS_REG
            let status = core::ptr::read_volatile(UART0_STATUS_REG);
            let rx_count = status & 0xFF;
            if rx_count > 0 {
                Some(core::ptr::read_volatile(UART0_FIFO_REG) as u8)
            } else {
                None
            }
        }
    }
}

impl Write for RawUart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for &b in s.as_bytes() {
            self.write_byte(b);
        }
        Ok(())
    }
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        {
            let mut uart = $crate::drivers::uart::RawUart;
            let _ = core::fmt::Write::write_fmt(&mut uart, format_args!($($arg)*));
        }
    };
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::print!("\r\n");
    };
    ($($arg:tt)*) => {
        {
            let mut uart = $crate::drivers::uart::RawUart;
            let _ = core::fmt::Write::write_fmt(&mut uart, format_args!($($arg)*));
            $crate::print!("\r\n");
        }
    };
}