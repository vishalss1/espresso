use core::convert::Infallible;
use core::sync::atomic::{AtomicBool, Ordering};
use embedded_hal::spi::{ErrorType, Operation, SpiDevice};

// ── Base addresses ─────────────────────────────────────────────────────────────
const DPORT_BASE:    u32 = 0x3FF00000;
const GPIO_BASE:     u32 = 0x3FF44000;
const IO_MUX_BASE:   u32 = 0x3FF49000;
const SPI3_BASE:     u32 = 0x3FF65000;
const RTC_CNTL_BASE: u32 = 0x3FF48000;

// ── DPORT peripheral clock / reset ────────────────────────────────────────────
const DPORT_PERIP_CLK_EN_REG: *mut u32 = (DPORT_BASE + 0x0C0) as *mut u32;
const SPI3_CLK_BIT: u32 = 1 << 16;

// ── GPIO registers ─────────────────────────────────────────────────────────────
const GPIO_OUT_W1TS:    *mut u32 = (GPIO_BASE + 0x008) as *mut u32;
const GPIO_OUT_W1TC:    *mut u32 = (GPIO_BASE + 0x00C) as *mut u32;
const GPIO_ENABLE_W1TS: *mut u32 = (GPIO_BASE + 0x024) as *mut u32;
const GPIO_ENABLE_W1TC: *mut u32 = (GPIO_BASE + 0x028) as *mut u32;

fn gpio_func_out(gpio: u32)  -> *mut u32 { (GPIO_BASE + 0x530 + gpio * 4) as *mut u32 }
fn gpio_func_in(signal: u32) -> *mut u32 { (GPIO_BASE + 0x130 + signal * 4) as *mut u32 }

// ── IO_MUX registers ───────────────────────────────────────────────────────────
const IO_MUX_GPIO5:  *mut u32 = (IO_MUX_BASE + 0x038) as *mut u32;
const IO_MUX_GPIO18: *mut u32 = (IO_MUX_BASE + 0x06C) as *mut u32;
const IO_MUX_GPIO19: *mut u32 = (IO_MUX_BASE + 0x070) as *mut u32;
const IO_MUX_GPIO4:  *mut u32 = (IO_MUX_BASE + 0x034) as *mut u32;

const IOMUX_WPU: u32 = 1 << 8;
const IOMUX_IE:  u32 = 1 << 9;

// ── SPI3 (VSPI) registers ─────────────────────────────────────────────────────
const SPI_CMD:       *mut u32 = (SPI3_BASE + 0x000) as *mut u32;
const SPI_CTRL:      *mut u32 = (SPI3_BASE + 0x008) as *mut u32;
const SPI_CTRL2:     *mut u32 = (SPI3_BASE + 0x014) as *mut u32;
const SPI_CLOCK:     *mut u32 = (SPI3_BASE + 0x018) as *mut u32;
const SPI_USER:      *mut u32 = (SPI3_BASE + 0x01C) as *mut u32;
const SPI_USER1:     *mut u32 = (SPI3_BASE + 0x020) as *mut u32;
const SPI_USER2:     *mut u32 = (SPI3_BASE + 0x024) as *mut u32;
const SPI_MOSI_DLEN: *mut u32 = (SPI3_BASE + 0x028) as *mut u32;
const SPI_MISO_DLEN: *mut u32 = (SPI3_BASE + 0x02C) as *mut u32;
const SPI_PIN:       *mut u32 = (SPI3_BASE + 0x034) as *mut u32;
const SPI_SLAVE:     *mut u32 = (SPI3_BASE + 0x038) as *mut u32;

// ── SPI_CMD bits ────────────────────────────────────────────────────────────────
const SPI_CMD_USR: u32 = 1 << 18;

// ── SPI_USER bits ───────────────────────────────────────────────────────────────
const SPI_USER_MOSI:    u32 = 1 << 27;
const SPI_USER_MISO:    u32 = 1 << 28;
const SPI_USER_DOUTDIN: u32 = 1 << 0;

// ── SPI_PIN: disable all HW CS lines ─────────────────────────────────────────────
const SPI_PIN_ALL_CS_DIS: u32 = (1 << 0) | (1 << 1) | (1 << 2);

// ── GPIO matrix signal indices ────────────────────────────────────────────────────
const GPIO_MATRIX_SW_OUT: u32 = 256;

// ── CS pin ─────────────────────────────────────────────────────────────────────
const CS_BIT: u32 = 1 << 5;

// ── RTC strapping pin release ──────────────────────────────────────────────────
const RTC_GPIO_XTAL_32K_PAD: *mut u32 = (RTC_CNTL_BASE + 0x408) as *mut u32;
const RTC_GPIO_PAD_SLP_SEL:  *mut u32 = (RTC_CNTL_BASE + 0x40C) as *mut u32;

// ── Speed control ──────────────────────────────────────────────────────────────
static SPEED_HIGH: AtomicBool = AtomicBool::new(false);

pub struct RawSpi;

impl ErrorType for RawSpi {
    type Error = Infallible;
}

impl SpiDevice<u8> for RawSpi {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        unsafe {
            if SPEED_HIGH.load(Ordering::Relaxed) {
                core::ptr::write_volatile(SPI_CLOCK, (0 << 18) | (7 << 12) | (3 << 6) | 7);
            } else {
                core::ptr::write_volatile(SPI_CLOCK, (3 << 18) | (49 << 12) | (24 << 6) | 49);
            }
        }

        Self::cs_low();
        for op in operations {
            match op {
                Operation::Read(buf) => {
                    let len = buf.len();
                    Self::transfer(None::<&[u8]>, Some(buf), len);
                }
                Operation::Write(buf) => {
                    let len = buf.len();
                    Self::transfer(Some(buf), None::<&mut [u8]>, len);
                }
                Operation::Transfer(read, write) => {
                    let len = core::cmp::min(read.len(), write.len());
                    Self::transfer(Some(write), Some(read), len);
                }
                Operation::TransferInPlace(buf) => {
                    let len = buf.len();
                    Self::transfer_in_place(buf, len);
                }
                Operation::DelayNs(_) => {}
            }
        }
        Self::cs_high();
        Self::transfer(Some(&[0xFF]), None::<&mut [u8]>, 1);
        Ok(())
    }
}

impl RawSpi {
    pub fn cs_low() {
        unsafe { core::ptr::write_volatile(GPIO_OUT_W1TC, CS_BIT); }
    }

    pub fn cs_high() {
        unsafe { core::ptr::write_volatile(GPIO_OUT_W1TS, CS_BIT); }
    }

    /// Set SPI clock to 10 MHz for data transfers (post-init)
    pub fn set_speed_high() {
        SPEED_HIGH.store(true, Ordering::SeqCst);
    }

    /// Initialize all VSPI3 hardware registers and GPIO matrix routing.
    ///
    /// Pins: CS=GPIO5, SCK=GPIO18, MOSI=GPIO4 (-> VSPID), MISO=GPIO19
    pub fn init() {
        unsafe {
            // ── Step 1: Enable SPI3 peripheral clock ────────────────────────────
            let clk = core::ptr::read_volatile(DPORT_PERIP_CLK_EN_REG);
            core::ptr::write_volatile(DPORT_PERIP_CLK_EN_REG, clk | SPI3_CLK_BIT);

            // ── Step 2: Release GPIO5 from RTC strapping domain ─────────────────
            let v = core::ptr::read_volatile(RTC_GPIO_XTAL_32K_PAD);
            core::ptr::write_volatile(RTC_GPIO_XTAL_32K_PAD, v & !(1 << 5));
            let v = core::ptr::read_volatile(RTC_GPIO_PAD_SLP_SEL);
            core::ptr::write_volatile(RTC_GPIO_PAD_SLP_SEL, v & !(1 << 5));

            // ── Step 3: IO_MUX pin configuration ────────────────────────────────
            // GPIO5 (CS) — MCU_SEL=0, pull-up (software GPIO)
            core::ptr::write_volatile(IO_MUX_GPIO5, IOMUX_WPU);
            // GPIO18 (SCK) — MCU_SEL=0, pull-up
            core::ptr::write_volatile(IO_MUX_GPIO18, IOMUX_WPU);
            // GPIO4 (MOSI) — MCU_SEL=0, pull-up, input enable
            core::ptr::write_volatile(IO_MUX_GPIO4, IOMUX_WPU | IOMUX_IE);
            // GPIO19 (MISO) — MCU_SEL=0, pull-up, input enable (MUST have IE=1)
            core::ptr::write_volatile(IO_MUX_GPIO19, IOMUX_WPU | IOMUX_IE);

            // ── Step 4: GPIO matrix routing ─────────────────────────────────────
            // GPIO5 → SW-controlled CS (signal 256 = GPIO mode, OEN from GPIO_ENABLE)
            core::ptr::write_volatile(gpio_func_out(5), GPIO_MATRIX_SW_OUT);
            // GPIO18 → VSPICLK output (signal 63, OEN from VSPI peripheral)
            core::ptr::write_volatile(gpio_func_out(18), 63);
            // GPIO4 → VSPID output (signal 65, OEN from VSPI peripheral)
            core::ptr::write_volatile(gpio_func_out(4), 65);
            // GPIO19 → no output (signal 256 = GPIO mode, OEN from GPIO_ENABLE)
            // actual GPIO enable is cleared with W1TC below, so this is input-only
            core::ptr::write_volatile(gpio_func_out(19), GPIO_MATRIX_SW_OUT);
            // GPIO19 → VSPIQ input (signal 64) via GPIO matrix
            core::ptr::write_volatile(gpio_func_in(64), (1 << 8) | 19);

            // ── Step 5: GPIO direction enable ───────────────────────────────────
            core::ptr::write_volatile(GPIO_ENABLE_W1TS, CS_BIT | (1 << 4) | (1 << 18));
            core::ptr::write_volatile(GPIO_ENABLE_W1TC, 1 << 19);

            // ── Step 6: CS idle HIGH ────────────────────────────────────────────
            Self::cs_high();

            // ── Step 7: SPI3 register config ────────────────────────────────────
            core::ptr::write_volatile(SPI_USER,      0);
            core::ptr::write_volatile(SPI_CTRL,      0);
            core::ptr::write_volatile(SPI_SLAVE,     0);
            core::ptr::write_volatile(SPI_CTRL2,     0);
            core::ptr::write_volatile(SPI_USER1,     0);
            core::ptr::write_volatile(SPI_USER2,     0);
            core::ptr::write_volatile(SPI_MOSI_DLEN, 0);
            core::ptr::write_volatile(SPI_MISO_DLEN, 0);
            core::ptr::write_volatile(SPI_PIN, SPI_PIN_ALL_CS_DIS);
            core::ptr::write_volatile(SPI_CTRL, 0);
            core::ptr::write_volatile(SPI_USER, SPI_USER_MOSI | SPI_USER_MISO | SPI_USER_DOUTDIN);
            core::ptr::write_volatile(SPI_CTRL2, 0);

            // ── Step 8: Clock = 400 kHz for SD card init ────────────────────────
            core::ptr::write_volatile(SPI_CLOCK, (3 << 18) | (49 << 12) | (24 << 6) | 49);
        }
    }

    /// Full-duplex SPI transfer of up to 64 bytes per call.
    pub(crate) fn transfer(tx: Option<&[u8]>, mut rx: Option<&mut [u8]>, len: usize) {
        let mut offset = 0usize;
        while offset < len {
            let chunk = core::cmp::min(len - offset, 64);
            unsafe {
                let mut timeout = 500_000u32;
                while (core::ptr::read_volatile(SPI_CMD) & SPI_CMD_USR) != 0 {
                    timeout -= 1;
                    if timeout == 0 { return; }
                }

                let bits = (chunk * 8 - 1) as u32;
                core::ptr::write_volatile(SPI_MOSI_DLEN, bits);
                core::ptr::write_volatile(SPI_MISO_DLEN, bits);

                let words = (chunk + 3) / 4;
                for i in 0..words {
                    let mut word = 0u32;
                    for j in 0..4usize {
                        let idx = offset + i * 4 + j;
                        let byte = if idx < offset + chunk {
                            tx.map_or(0xFF, |s| s[idx])
                        } else {
                            0xFF
                        };
                        word |= (byte as u32) << (j * 8);
                    }
                    let wreg = (SPI3_BASE + 0x080 + (i as u32) * 4) as *mut u32;
                    core::ptr::write_volatile(wreg, word);
                }

                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
                let v = core::ptr::read_volatile(SPI_CMD);
                core::ptr::write_volatile(SPI_CMD, v | SPI_CMD_USR);

                let mut timeout = 2_000_000u32;
                while (core::ptr::read_volatile(SPI_CMD) & SPI_CMD_USR) != 0 {
                    timeout -= 1;
                    if timeout == 0 { return; }
                }

                if let Some(ref mut slice) = rx {
                    for i in 0..words {
                        let wreg = (SPI3_BASE + 0x080 + (i as u32) * 4) as *mut u32;
                        let word = core::ptr::read_volatile(wreg);
                        for j in 0..4usize {
                            let idx = offset + i * 4 + j;
                            if idx < offset + chunk {
                                slice[idx] = ((word >> (j * 8)) & 0xFF) as u8;
                            }
                        }
                    }
                }
            }
            offset += chunk;
        }
    }

    /// In-place transfer (used by TransferInPlace operation).
    pub(crate) fn transfer_in_place(buf: &mut [u8], len: usize) {
        let mut tmp = [0xFFu8; 64];
        let mut offset = 0usize;
        while offset < len {
            let chunk = core::cmp::min(len - offset, 64);
            let src = &buf[offset..offset + chunk];
            tmp[..chunk].copy_from_slice(src);
            Self::transfer(Some(&tmp[..chunk]), Some(&mut buf[offset..offset + chunk]), chunk);
            offset += chunk;
        }
    }
}

pub fn spi_init() {
    RawSpi::init();
}
