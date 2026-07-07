use core::convert::Infallible;

// ── Base addresses ────────────────────────────────────────────────────────────
const DPORT_BASE: u32    = 0x3FF00000;
const GPIO_BASE: u32     = 0x3FF44000;
const IO_MUX_BASE: u32   = 0x3FF49000;
const SPI3_BASE: u32     = 0x3FF65000;
const RTC_CNTL_BASE: u32 = 0x3FF48000;

// ── DPORT peripheral clock / reset ───────────────────────────────────────────
const DPORT_PERIP_CLK_EN_REG:  *mut u32 = (DPORT_BASE + 0x0C0) as *mut u32;
const DPORT_PERIP_RST_EN_REG:  *mut u32 = (DPORT_BASE + 0x0C4) as *mut u32;
const SPI3_CLK_BIT: u32 = 1 << 21;

// ── GPIO registers ────────────────────────────────────────────────────────────
const GPIO_OUT_W1TS:   *mut u32 = (GPIO_BASE + 0x008) as *mut u32;
const GPIO_OUT_W1TC:   *mut u32 = (GPIO_BASE + 0x00C) as *mut u32;
const GPIO_ENABLE_W1TS: *mut u32 = (GPIO_BASE + 0x024) as *mut u32;
const GPIO_ENABLE_W1TC: *mut u32 = (GPIO_BASE + 0x028) as *mut u32;

fn gpio_func_out(gpio: u32) -> *mut u32 { (GPIO_BASE + 0x530 + gpio * 4) as *mut u32 }
fn gpio_func_in(signal: u32) -> *mut u32 { (GPIO_BASE + 0x130 + signal * 4) as *mut u32 }

// ── IO_MUX registers ──────────────────────────────────────────────────────────
const IO_MUX_GPIO5:  *mut u32 = (IO_MUX_BASE + 0x06C) as *mut u32;
const IO_MUX_GPIO18: *mut u32 = (IO_MUX_BASE + 0x070) as *mut u32;
const IO_MUX_GPIO23: *mut u32 = (IO_MUX_BASE + 0x08C) as *mut u32;
const IO_MUX_GPIO19: *mut u32 = (IO_MUX_BASE + 0x074) as *mut u32;

const IOMUX_NATIVE: u32      = 1 << 12;
const IOMUX_GPIO_MATRIX: u32 = 2 << 12;
const IOMUX_IE: u32          = 1 << 9;
const IOMUX_WPU: u32         = 1 << 8;

// ── SPI3 (VSPI) registers ─────────────────────────────────────────────────────
const SPI_CMD:       *mut u32 = (SPI3_BASE + 0x000) as *mut u32;
const SPI_CTRL:      *mut u32 = (SPI3_BASE + 0x008) as *mut u32;
const SPI_CLOCK:     *mut u32 = (SPI3_BASE + 0x018) as *mut u32;
const SPI_USER:      *mut u32 = (SPI3_BASE + 0x01C) as *mut u32;
const SPI_USER1:     *mut u32 = (SPI3_BASE + 0x020) as *mut u32;
const SPI_USER2:     *mut u32 = (SPI3_BASE + 0x024) as *mut u32;
const SPI_CTRL2:     *mut u32 = (SPI3_BASE + 0x014) as *mut u32;
const SPI_MOSI_DLEN: *mut u32 = (SPI3_BASE + 0x028) as *mut u32;
const SPI_MISO_DLEN: *mut u32 = (SPI3_BASE + 0x02C) as *mut u32;
const SPI_PIN:       *mut u32 = (SPI3_BASE + 0x034) as *mut u32;
const SPI_SLAVE:     *mut u32 = (SPI3_BASE + 0x038) as *mut u32;

const SPI_CMD_USR: u32    = 1 << 18;
const SPI_USER_MOSI: u32  = 1 << 27;
const SPI_USER_MISO: u32  = 1 << 28;
const SPI_USER_DOUTDIN: u32 = 1 << 0;

const CS_BIT: u32 = 1 << 5;
const GPIO_MATRIX_GPIO_OUT: u32 = 256;

// ── RTC strapping pin release ─────────────────────────────────────────────────
const RTC_GPIO_XTAL_32K_PAD: *mut u32 = (RTC_CNTL_BASE + 0x408) as *mut u32;
const RTC_GPIO_PAD_SLP_SEL:  *mut u32 = (RTC_CNTL_BASE + 0x40C) as *mut u32;

pub struct RawSpi;

use embedded_hal::spi::{ErrorType, SpiDevice, Operation};

impl ErrorType for RawSpi {
    type Error = Infallible;
}

impl SpiDevice<u8> for RawSpi {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        Self::cs_low();
        for op in operations {
            match op {
                Operation::Read(rx) => {
                    let len = rx.len();
                    Self::transfer_bytes(None, Some(rx), len);
                }
                Operation::Write(tx) => {
                    let len = tx.len();
                    Self::transfer_bytes(Some(tx), None, len);
                }
                Operation::Transfer(rx, tx) => {
                    let len = tx.len();
                    Self::transfer_bytes(Some(tx), Some(rx), len);
                }
                Operation::TransferInPlace(words) => {
                    let mut temp = [0xFFu8; 64];
                    let mut offset = 0;
                    while offset < words.len() {
                        let chunk = core::cmp::min(words.len() - offset, 64);
                        temp[..chunk].copy_from_slice(&words[offset..offset+chunk]);
                        Self::transfer_bytes(Some(&temp[..chunk]), Some(&mut words[offset..offset+chunk]), chunk);
                        offset += chunk;
                    }
                }
                Operation::DelayNs(_) => {}
            }
        }
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        Ok(())
    }
}

impl RawSpi {
    #[inline(always)]
    pub fn cs_low() {
        unsafe { core::ptr::write_volatile(GPIO_OUT_W1TC, CS_BIT); }
    }

    #[inline(always)]
    pub fn cs_high() {
        unsafe { core::ptr::write_volatile(GPIO_OUT_W1TS, CS_BIT); }
    }

    pub fn set_speed_low() {
        unsafe {
            // 400 kHz: pre=3, n=49, h=24, l=49 -> divider=200 -> 80MHz/200=400kHz
            core::ptr::write_volatile(SPI_CLOCK, (3 << 18) | (49 << 12) | (24 << 6) | 49);
        }
    }

    pub fn set_speed_high() {
        unsafe {
            // 10 MHz: pre=0, n=7, h=3, l=7 -> divider=8 -> 80MHz/8=10MHz
            core::ptr::write_volatile(SPI_CLOCK, (0 << 18) | (7 << 12) | (3 << 6) | 7);
        }
    }

    pub fn init() {
        crate::println!("[SPI3] VSPI init start (base 0x{:08X})", SPI3_BASE);
        unsafe {
            // 1. Enable SPI3 peripheral clock, clear reset
            let clk = core::ptr::read_volatile(DPORT_PERIP_CLK_EN_REG);
            core::ptr::write_volatile(DPORT_PERIP_CLK_EN_REG, clk | SPI3_CLK_BIT);
            let rst = core::ptr::read_volatile(DPORT_PERIP_RST_EN_REG);
            core::ptr::write_volatile(DPORT_PERIP_RST_EN_REG, rst & !SPI3_CLK_BIT);

            // 2. Release GPIO5 from RTC strapping
            let v = core::ptr::read_volatile(RTC_GPIO_XTAL_32K_PAD);
            core::ptr::write_volatile(RTC_GPIO_XTAL_32K_PAD, v & !(1 << 5));
            let v = core::ptr::read_volatile(RTC_GPIO_PAD_SLP_SEL);
            core::ptr::write_volatile(RTC_GPIO_PAD_SLP_SEL, v & !(1 << 5));

            // 3. IO_MUX: route pins natively for VSPI (CS=5 via GPIO matrix)
            core::ptr::write_volatile(IO_MUX_GPIO5,  IOMUX_GPIO_MATRIX | IOMUX_WPU);
            core::ptr::write_volatile(IO_MUX_GPIO18, IOMUX_NATIVE | IOMUX_IE | IOMUX_WPU);
            core::ptr::write_volatile(IO_MUX_GPIO23, IOMUX_NATIVE | IOMUX_WPU);
            core::ptr::write_volatile(IO_MUX_GPIO19, IOMUX_NATIVE | IOMUX_IE | IOMUX_WPU);

            // 4. GPIO matrix routing: GPIO5 = software CS, others bypass
            core::ptr::write_volatile(gpio_func_out(5), GPIO_MATRIX_GPIO_OUT);
            core::ptr::write_volatile(gpio_func_out(18), 0x100);
            core::ptr::write_volatile(gpio_func_out(23), 0x100);
            core::ptr::write_volatile(gpio_func_in(64), 0x3C);

            // 5. Enable outputs: CS(5), MOSI(23), SCK(18). Disable MISO(19) output.
            core::ptr::write_volatile(GPIO_ENABLE_W1TS, CS_BIT | (1 << 23) | (1 << 18));
            core::ptr::write_volatile(GPIO_ENABLE_W1TC, 1 << 19);

            // 6. CS idle high
            Self::cs_high();

            // 7. SPI3 registers: wipe then configure
            core::ptr::write_volatile(SPI_USER, 0);
            core::ptr::write_volatile(SPI_CTRL, 0);
            core::ptr::write_volatile(SPI_CTRL2, 0);
            core::ptr::write_volatile(SPI_SLAVE, 0);
            core::ptr::write_volatile(SPI_CLOCK, 0);
            
            // SPI_PIN: disable all hw CS lines, CPOL=0
            core::ptr::write_volatile(SPI_PIN, (1<<0) | (1<<1) | (1<<2));

            // SPI_CTRL: MSB-first, clear WP bit
            core::ptr::write_volatile(SPI_CTRL, 0);

            // SPI_USER: full-duplex, MOSI/MISO both enabled, cs_hold, usr_dummy_idle
            let expected_user = SPI_USER_MOSI | SPI_USER_MISO | SPI_USER_DOUTDIN | (1 << 4) | (1 << 26);
            core::ptr::write_volatile(SPI_USER, expected_user);
            core::ptr::write_volatile(SPI_USER1, 0);
            core::ptr::write_volatile(SPI_SLAVE, 0);

            // MISO delay = 0 (Mode 0)
            core::ptr::write_volatile(SPI_CTRL2, 0);

            // 8. Clock: 400 kHz for SD init
            Self::set_speed_low();

            // Verification readbacks:
            let r_user = core::ptr::read_volatile(SPI_USER);
            let r_ctrl = core::ptr::read_volatile(SPI_CTRL);
            let r_pin = core::ptr::read_volatile(SPI_PIN);
            let r_clock = core::ptr::read_volatile(SPI_CLOCK);
            let r_slave = core::ptr::read_volatile(SPI_SLAVE);
            let r_ctrl2 = core::ptr::read_volatile(SPI_CTRL2);
            crate::println!("  [SPI3 VERIFY] SPI_USER=0x{:08X} (expected 0x{:08X})", r_user, expected_user);
            crate::println!("  [SPI3 VERIFY] SPI_CTRL=0x{:08X} (expected 0x00000000)", r_ctrl);
            crate::println!("  [SPI3 VERIFY] SPI_PIN=0x{:08X} (expected 0x00000007)", r_pin);
            crate::println!("  [SPI3 VERIFY] SPI_CLOCK=0x{:08X}", r_clock);
            crate::println!("  [SPI3 VERIFY] SPI_SLAVE=0x{:08X}", r_slave);
            crate::println!("  [SPI3 VERIFY] SPI_CTRL2=0x{:08X}", r_ctrl2);
        }
        crate::println!("[SPI3] VSPI init OK");
    }

    /// Low-level SPI transfer.
    /// TX data at `tx` (or 0xFF if None), receives into `rx` (if Some).
    /// Returns false on MISO timeout.
    pub fn transfer_bytes(tx: Option<&[u8]>, mut rx: Option<&mut [u8]>, len: usize) -> bool {
        let mut offset = 0;
        while offset < len {
            let chunk = core::cmp::min(len - offset, 64);

            unsafe {
                // Wait for previous transaction to complete (USR cleared)
                let mut timeout = 500_000u32;
                while (core::ptr::read_volatile(SPI_CMD) & SPI_CMD_USR) != 0 {
                    timeout -= 1;
                    if timeout == 0 {
                        crate::println!("[SPI] TIMEOUT: USR stuck high at offset {}", offset);
                        return false;
                    }
                }

                let bits = (chunk * 8 - 1) as u32;
                core::ptr::write_volatile(SPI_MOSI_DLEN, bits);
                core::ptr::write_volatile(SPI_MISO_DLEN, bits);

                // Write TX data into W0..W15 (little-endian packing per ESP32 TRM)
                let words = (chunk + 3) / 4;
                for i in 0..words {
                    let mut word = 0u32;
                    for j in 0..4 {
                        let idx = offset + i * 4 + j;
                        let byte = if idx < offset + chunk {
                            tx.as_ref().map_or(0xFF, |s| s[idx])
                        } else {
                            0xFF
                        };
                        word |= (byte as u32) << (j * 8);
                    }
                    let wreg = (SPI3_BASE + 0x080 + (i as u32) * 4) as *mut u32;
                    core::ptr::write_volatile(wreg, word);
                }

                // Trigger transaction
                let v = core::ptr::read_volatile(SPI_CMD);
                core::ptr::write_volatile(SPI_CMD, v | SPI_CMD_USR);

                // Wait for transaction to complete (SPI_CMD_USR clears)
                let mut timeout = 5_000_000u32;
                while (core::ptr::read_volatile(SPI_CMD) & SPI_CMD_USR) != 0 {
                    timeout -= 1;
                    if timeout == 0 {
                        let miso_pin = (core::ptr::read_volatile(0x3FF4403C as *const u32) >> 19) & 1;
                        crate::println!("[SPI] MISO/USR TIMEOUT at offset {} ({} bytes), GPIO19_IN={}",
                            offset, chunk, miso_pin);
                        return false;
                    }
                }

                // Read RX data from W0..W15
                if let Some(ref mut slice) = rx {
                    for i in 0..words {
                        let wreg = (SPI3_BASE + 0x080 + (i as u32) * 4) as *mut u32;
                        let word = core::ptr::read_volatile(wreg);
                        for j in 0..4 {
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
        true
    }

    /// Send a raw SD command (6 bytes) and read the R1 response.
    /// `resp_extra` = number of additional response bytes beyond R1 (0, 2, or 4).
    /// On success returns `Ok((r1, extra_bytes_shifted))` where extra_bytes is
    /// 0 for R1, or a big-endian u32 for R3/R7.
    pub fn send_cmd(cmd_idx: u8, arg: u32, crc: u8, resp_extra: usize) -> Result<(u8, u32), &'static str> {
        let mut cmd = [0u8; 6];
        cmd[0] = 0x40 | cmd_idx;
        cmd[1] = (arg >> 24) as u8;
        cmd[2] = (arg >> 16) as u8;
        cmd[3] = (arg >> 8) as u8;
        cmd[4] = arg as u8;
        cmd[5] = crc;

        // Print debug trace
        crate::println!("  [SPI cmd] Sending CMD{} arg=0x{:08X} crc=0x{:02X}...", cmd_idx, arg, crc);

        if !Self::transfer_bytes(Some(&cmd), None, 6) {
            return Err("SPI timeout sending command bytes");
        }

        // Read dummy bytes until card asserts response (first byte != 0xFF)
        let mut resp = [0xFFu8; 8];
        let mut tries = 1000;
        let mut found = false;
        while tries > 0 {
            if !Self::transfer_bytes(Some(&[0xFF]), Some(&mut resp[..1]), 1) {
                return Err("SPI timeout during response poll");
            }
            if resp[0] != 0xFF {
                found = true;
                break;
            }
            tries -= 1;
        }
        if !found {
            return Err("card did not assert MISO (stuck high)");
        }

        let r1 = resp[0];
        crate::println!("  [SPI cmd] CMD{} R1 response: 0x{:02X}", cmd_idx, r1);

        if resp_extra > 0 {
            let extra_len = core::cmp::min(resp_extra, 4);
            if !Self::transfer_bytes(Some(&[0xFF; 4]), Some(&mut resp[..extra_len]), extra_len) {
                return Err("SPI timeout reading extra response bytes");
            }
            if cmd_idx == 8 {
                crate::println!("CMD8 raw bytes:");
                crate::println!("  b0 = 0x{:02X}", resp[0]);
                crate::println!("  b1 = 0x{:02X}", resp[1]);
                crate::println!("  b2 = 0x{:02X}", resp[2]);
                crate::println!("  b3 = 0x{:02X}", resp[3]);
            }
            let mut val = 0u32;
            for i in 0..extra_len {
                val = (val << 8) | resp[i] as u32;
            }
            crate::println!("  [SPI cmd] CMD{} extra bytes: 0x{:08X}", cmd_idx, val);
            Ok((r1, val))
        } else {
            Ok((r1, 0))
        }
    }

    /// Full SD card initialization sequence over SPI.
    /// Resets the card into SPI mode, detects SDv1/SDv2/SDHC, and powers it on.
    pub fn card_reset() -> Result<(), &'static str> {
        crate::println!("[SD reset] Starting card reset...");

        // 1. Send 80 dummy clocks with CS high (card power-up stabilization)
        Self::cs_high();
        let dummy = [0xFFu8; 10];
        if !Self::transfer_bytes(Some(&dummy), None, 10) {
            return Err("SPI timeout during init clocks");
        }

        // 2. CMD0 — GO_IDLE_STATE (enter SPI mode)
        Self::cs_low();
        let r = Self::send_cmd(0, 0x0000_0000, 0x95, 0);
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        let (r1, _) = r?;
        if r1 & 0x01 == 0 {
            return Err("CMD0: card not in idle state (R1 did not show idle bit)");
        }
        crate::println!("[SD reset] CMD0 OK (R1 = 0x{:02X})", r1);

        // 3. CMD8 — SEND_IF_COND (check SDv2 voltage support)
        Self::cs_low();
        let r = Self::send_cmd(8, 0x0000_01AA, 0x87, 4);
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        let (r1, extra) = r?;

        let reserved = ((extra >> 24) & 0xFF) as u8;
        let voltage_accepted = ((extra >> 8) & 0xFF) as u8;
        let check_pattern = (extra & 0xFF) as u8;

        crate::println!("  CMD8:");
        crate::println!("    R1 = 0x{:02X}", r1);
        crate::println!("    Reserved = 0x{:02X}", reserved);
        crate::println!("    Voltage Accepted = 0x{:02X}", voltage_accepted);
        crate::println!("    Check Pattern = 0x{:02X}", check_pattern);

        let is_sdhc = if r1 == 0x01 && voltage_accepted == 0x01 && check_pattern == 0xAA {
            crate::println!("  [SD reset] CMD8 OK -> SDv2/SDHC/SDXC");
            true
        } else if r1 & 0x04 != 0 {
            crate::println!("  [SD reset] CMD8 got illegal command -> SDv1 or MMC");
            false
        } else {
            return Err("CMD8 voltage mismatch or failure");
        };

        // 4. ACMD41 — SD_SEND_OP_COND (init with or without HCS)
        let hcs_arg = if is_sdhc { 0x4000_0000 } else { 0x0000_0000 };
        crate::println!("[SD reset] Sending ACMD41 loop (HCS=0x{:08X})...", hcs_arg);
        let mut retries = 2000;
        loop {
            // CMD55 (APP_CMD prefix)
            Self::cs_low();
            let r = Self::send_cmd(55, 0x0000_0000, 0x01, 0);
            Self::cs_high();
            Self::transfer_bytes(Some(&[0xFF]), None, 1);
            let (r1, _) = r?;

            if r1 & 0x80 != 0 {
                return Err("ACMD41 loop: CMD55 returned invalid state");
            }

            // ACMD41
            Self::cs_low();
            let r = Self::send_cmd(41, hcs_arg, 0x01, 0);
            Self::cs_high();
            Self::transfer_bytes(Some(&[0xFF]), None, 1);
            let (r1, _) = r?;

            if r1 == 0x00 {
                crate::println!("  [SD reset] ACMD41 ready (R1=0x00)");
                break;
            }
            if r1 & 0x01 == 0 {
                crate::println!("  [SD reset] ACMD41 error: R1=0x{:02X}", r1);
                return Err("ACMD41 unexpected error");
            }
            retries -= 1;
            if retries == 0 {
                return Err("ACMD41 timeout — card never left idle state");
            }
        }
        crate::println!("[SD reset] ACMD41 complete (card active)");

        // 5. CMD58 — READ_OCR
        Self::cs_low();
        let r = Self::send_cmd(58, 0x0000_0000, 0x01, 4);
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        let (_r1, ocr) = r?;
        if ocr & 0x8000_0000 != 0 {
            crate::println!("  [SD reset] CMD58 OCR=0x{:08X} -> card type: SDHC/SDXC", ocr);
        } else {
            crate::println!("  [SD reset] CMD58 OCR=0x{:08X} -> card type: SDSC", ocr);
        }

        crate::println!("[SD reset] Raw SD init OK");
        Ok(())
    }
}

pub fn spi_init() {
    RawSpi::init();
}
