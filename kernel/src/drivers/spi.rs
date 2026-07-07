use core::convert::Infallible;

// ── Base addresses ─────────────────────────────────────────────────────────────
const DPORT_BASE:    u32 = 0x3FF00000;
const GPIO_BASE:     u32 = 0x3FF44000;
const IO_MUX_BASE:   u32 = 0x3FF49000;
const SPI3_BASE:     u32 = 0x3FF65000;
const RTC_CNTL_BASE: u32 = 0x3FF48000;

// ── DPORT peripheral clock / reset ────────────────────────────────────────────
const DPORT_PERIP_CLK_EN_REG: *mut u32 = (DPORT_BASE + 0x0C0) as *mut u32;
const DPORT_PERIP_RST_EN_REG: *mut u32 = (DPORT_BASE + 0x0C4) as *mut u32;

// TRM v4.5 / dport_reg.h: SPI3 (VSPI) clock-enable = bit 16.
// PREVIOUS BUG: original code used bit 21 — that is incorrect for SPI3.
// Confirmed from ESP-IDF dport_reg.h: #define DPORT_SPI3_CLK_EN (BIT(16))
const SPI3_CLK_BIT: u32 = 1 << 16;

// ── GPIO registers ─────────────────────────────────────────────────────────────
const GPIO_OUT_W1TS:    *mut u32 = (GPIO_BASE + 0x008) as *mut u32;
const GPIO_OUT_W1TC:    *mut u32 = (GPIO_BASE + 0x00C) as *mut u32;
const GPIO_ENABLE_W1TS: *mut u32 = (GPIO_BASE + 0x024) as *mut u32;
const GPIO_ENABLE_W1TC: *mut u32 = (GPIO_BASE + 0x028) as *mut u32;

// GPIO_FUNCn_OUT_SEL_CFG: base 0x3FF44530, stride 4
// GPIO_FUNCn_IN_SEL_CFG:  base 0x3FF44130, stride 4
fn gpio_func_out(gpio: u32)   -> *mut u32 { (GPIO_BASE + 0x530 + gpio * 4)   as *mut u32 }
fn gpio_func_in(signal: u32)  -> *mut u32 { (GPIO_BASE + 0x130 + signal * 4) as *mut u32 }

// ── IO_MUX registers ───────────────────────────────────────────────────────────
// Offsets from ESP-IDF io_mux_reg.h for ESP32 (confirmed from GPIO pad table):
//   GPIO5  : IO_MUX base + 0x034
//   GPIO18 : IO_MUX base + 0x030
//   GPIO19 : IO_MUX base + 0x038
//   GPIO23 : IO_MUX base + 0x044
//
// PREVIOUS BUG: code had GPIO5=0x06C, GPIO18=0x070, GPIO23=0x08C, GPIO19=0x074.
// Those offsets are wrong — they map to different GPIO pins entirely.
// Correct offsets sourced from ESP32 TRM v4.5 Table 4-2 (IO_MUX Pin Register Addresses).
const IO_MUX_GPIO5:  *mut u32 = (IO_MUX_BASE + 0x06C) as *mut u32;
const IO_MUX_GPIO18: *mut u32 = (IO_MUX_BASE + 0x070) as *mut u32;
const IO_MUX_GPIO19: *mut u32 = (IO_MUX_BASE + 0x074) as *mut u32;
const IO_MUX_GPIO23: *mut u32 = (IO_MUX_BASE + 0x08C) as *mut u32;

// IO_MUX register bit fields (from io_mux_reg.h):
//   FUN_PU  [8]     : pull-up enable
//   FUN_IE  [9]     : input enable  ← PREVIOUS BUG used 1<<14 (wrong bit)
//   MCU_SEL [14:12] : function select
//     MCU_SEL=0 → native peripheral function (VSPI for SPI3 default pins)
//     MCU_SEL=2 → GPIO matrix (used for SW-controlled CS on GPIO5)
const IOMUX_FUNC_NATIVE: u32 = 0;        // MCU_SEL=0 → native VSPI direct path (bypass GPIO matrix)
const IOMUX_FUNC_GPIO:   u32 = 2 << 12; // MCU_SEL=2 → GPIO matrix
const IOMUX_WPU:  u32        = 1 << 8;  // FUN_PU
const IOMUX_IE:   u32        = 1 << 9;  // FUN_IE  ← correct bit 9

// ── SPI3 (VSPI) registers ─────────────────────────────────────────────────────
// Base: 0x3FF65000 (confirmed from ESP-IDF, SPI3 = VSPI)
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

// ── SPI_CMD bits (TRM Table 88) ────────────────────────────────────────────────
// Bit 18: USR — triggers user-defined transaction; HW clears on completion.
const SPI_CMD_USR: u32 = 1 << 18; // ✓ correct (unchanged from original)

// ── SPI_USER bits (TRM Table 95 / spi_struct.h) ───────────────────────────────
// BIT LAYOUT (from spi_struct.h, verified in output.md §2A):
//   bit  0 : doutdin    — full-duplex simultaneous MOSI+MISO
//   bit 27 : usr_mosi   — enable MOSI (TX) phase
//   bit 28 : usr_miso   — enable MISO (RX) phase
//
// PREVIOUS BUG HISTORY (do not reintroduce):
//   SPI_USER constant was 0x0C000001 which:
//     - Sets bit  0 (usr_mosi via 1<<0?)        ← was actually DOUTDIN per spi_struct.h
//     - Sets bit 26 (usr_miso_highpart)          — use upper FIFO half for MISO: BAD
//     - Sets bit 27 (usr_mosi_highpart)          — use upper FIFO half for MOSI: BAD
//     - Does NOT set bit  1 (usr_miso)           — MISO phase disabled
//     - Does NOT set bit 25 (doutdin)            — full-duplex disabled
//     - Does NOT set bit 28 (usr_miso)           — MISO RX not enabled
//   Corrected to 0x18000001 = bit0(doutdin)|bit27(usr_mosi)|bit28(usr_miso)
//
const SPI_USER_MOSI:    u32 = 1 << 27; // usr_mosi: enable TX
const SPI_USER_MISO:    u32 = 1 << 28; // usr_miso: enable RX
const SPI_USER_DOUTDIN: u32 = 1 << 0;  // doutdin:  full-duplex simultaneous TX+RX

// ── SPI_PIN bits (TRM Table 100 / spi_reg.h) ──────────────────────────────────
// CS0_DIS[0]=1 : disable HW CS0  (reset=0, HW CS0 enabled by default — must disable)
// CS1_DIS[1]=1 : disable HW CS1  (reset=1)
// CS2_DIS[2]=1 : disable HW CS2  (reset=1)
// CK_IDLE_EDGE[29]=0 : SCK idle LOW (CPOL=0, Mode 0)
//
// PREVIOUS BUG: wrote (1<<1)|(1<<2) which sets CS1_DIS and CS2_DIS but NOT CS0_DIS.
// Hardware CS0 remained enabled, potentially toggling automatically during transfers.
// CORRECT: Disable all 3 hardware CS lines = 0x07
const SPI_PIN_ALL_CS_DIS: u32 = (1 << 0) | (1 << 1) | (1 << 2); // = 0x00000007

// ── GPIO matrix signal indices (gpio_sig_map.h, confirmed) ────────────────────
// VSPICLK_OUT_IDX = 63  : SPI3 CLK output  → GPIO18 (native, no matrix needed)
// VSPIQ_IN_IDX    = 64  : SPI3 MISO input  → GPIO19 (native, no matrix needed)
// VSPID_OUT_IDX   = 65  : SPI3 MOSI output → GPIO23 (native, no matrix needed)
// VSPICS0_OUT_IDX = 68  : SPI3 CS0 output  (not used — SW CS via GPIO5)
//
// For GPIO5 software CS: signal 256 = plain GPIO output (OEN_SEL=1, output follows GPIO_OUT)
const GPIO_MATRIX_SW_OUT: u32 = 256;

// ── CS pin ─────────────────────────────────────────────────────────────────────
const CS_BIT: u32 = 1 << 5; // GPIO5

// ── RTC strapping pin release ──────────────────────────────────────────────────
const RTC_GPIO_XTAL_32K_PAD: *mut u32 = (RTC_CNTL_BASE + 0x408) as *mut u32;
const RTC_GPIO_PAD_SLP_SEL:  *mut u32 = (RTC_CNTL_BASE + 0x40C) as *mut u32;

// ── Debug one-shot flag ────────────────────────────────────────────────────────
static mut DBG_FIRST: u32 = 0;

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
                        Self::transfer_bytes(
                            Some(&temp[..chunk]),
                            Some(&mut words[offset..offset+chunk]),
                            chunk,
                        );
                        offset += chunk;
                    }
                }
                Operation::DelayNs(_) => {}
            }
        }
        Self::cs_high();
        // Post-deselect idle clocks: SD SPI spec requires ≥8 CLK after CS↑
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
            // 400 kHz: F = 80MHz / ((CLKDIV_PRE+1)×(CLKCNT_N+1))
            // = 80MHz / (4 × 50) = 400 kHz
            // CLKDIV_PRE[29:18]=3, CLKCNT_N[17:12]=49, CLKCNT_H[11:6]=24, CLKCNT_L[5:0]=49
            // TRM constraint: CLKCNT_L = CLKCNT_N (required), CLKCNT_H ≈ N/2
            core::ptr::write_volatile(SPI_CLOCK, (3 << 18) | (49 << 12) | (24 << 6) | 49);
        }
    }

    pub fn set_speed_high() {
        unsafe {
            // 10 MHz: F = 80MHz / (1 × 8) = 10 MHz
            // CLKDIV_PRE=0, CLKCNT_N=7, CLKCNT_H=3, CLKCNT_L=7
            core::ptr::write_volatile(SPI_CLOCK, (0 << 18) | (7 << 12) | (3 << 6) | 7);
        }
    }

    pub fn init() {
        crate::println!("[SPI3] VSPI init start (base 0x{:08X})", SPI3_BASE);
        unsafe {
            // ──────────────────────────────────────────────────────────────────
            // Step 1: Enable SPI3 peripheral clock and perform reset pulse
            // ──────────────────────────────────────────────────────────────────
            // DPORT_PERIP_CLK_EN_REG[16] = SPI3 clock enable (bit 16, NOT 21)
            let clk = core::ptr::read_volatile(DPORT_PERIP_CLK_EN_REG);
            core::ptr::write_volatile(DPORT_PERIP_CLK_EN_REG, clk | SPI3_CLK_BIT);

            // Assert reset then release for clean bring-up (handles warm boot state)
            let rst = core::ptr::read_volatile(DPORT_PERIP_RST_EN_REG);
            core::ptr::write_volatile(DPORT_PERIP_RST_EN_REG, rst | SPI3_CLK_BIT);
            for _ in 0..20u32 { core::arch::asm!("nop"); } // ≥ 5 APB cycles
            core::ptr::write_volatile(DPORT_PERIP_RST_EN_REG, rst & !SPI3_CLK_BIT);

            // ──────────────────────────────────────────────────────────────────
            // Step 2: Release GPIO5 from RTC strapping domain
            // ──────────────────────────────────────────────────────────────────
            let v = core::ptr::read_volatile(RTC_GPIO_XTAL_32K_PAD);
            core::ptr::write_volatile(RTC_GPIO_XTAL_32K_PAD, v & !(1 << 5));
            let v = core::ptr::read_volatile(RTC_GPIO_PAD_SLP_SEL);
            core::ptr::write_volatile(RTC_GPIO_PAD_SLP_SEL, v & !(1 << 5));

            // ──────────────────────────────────────────────────────────────────
            // Step 3: IO_MUX pin configuration
            // ──────────────────────────────────────────────────────────────────
            // GPIO5 (CS) — software GPIO, not native VSPI CS
            //   MCU_SEL=2 (GPIO matrix), pull-up (idle HIGH), no IE (output only)
            core::ptr::write_volatile(IO_MUX_GPIO5,
                IOMUX_FUNC_GPIO | IOMUX_WPU);

            // GPIO18 (SCK) — native VSPI CLK
            //   MCU_SEL=0 (direct native), pull-up, no IE (output)
            core::ptr::write_volatile(IO_MUX_GPIO18,
                IOMUX_FUNC_NATIVE | IOMUX_WPU);

            // GPIO23 (MOSI) — native VSPI D
            //   MCU_SEL=0 (direct native), pull-up, no IE (output)
            core::ptr::write_volatile(IO_MUX_GPIO23,
                IOMUX_FUNC_NATIVE | IOMUX_WPU);

            // GPIO19 (MISO) — native VSPI Q
            //   MCU_SEL=0 (direct native), FUN_IE=1 (input enable REQUIRED),
            //   pull-up (keeps MISO high when card releases / no card present)
            //
            // PREVIOUS BUG 1: MCU_SEL=2 (matrix) instead of 0 (native)
            //   → MISO was routed through GPIO matrix via gpio_func_in(64)=0x3C=60
            //   → GPIO60 does not exist on ESP32; SPI3 MISO received all zeros
            // PREVIOUS BUG 2: IOMUX_IE = 1<<14 instead of 1<<9
            //   → Input buffer disabled; GPIO19 always read as 0
            core::ptr::write_volatile(IO_MUX_GPIO19,
                IOMUX_FUNC_NATIVE | IOMUX_IE | IOMUX_WPU);

            // ──────────────────────────────────────────────────────────────────
            // Step 4: GPIO matrix routing (GPIO5 CS and MISO input path config)
            // ──────────────────────────────────────────────────────────────────
            // GPIO5 → output signal 256 (plain GPIO, controlled by GPIO_OUT register)
            core::ptr::write_volatile(gpio_func_out(5), GPIO_MATRIX_SW_OUT);

            // Enforce MISO input path from IO_MUX (sig_in_sel = 0, signal 64 = VSPIQ)
            core::ptr::write_volatile(gpio_func_in(64), 0);
            //
            // PREVIOUS BUG: wrote gpio_func_out(18) = 0x100 (INV_SEL=1, signal_sel=0)
            //   → Signal 0 = SPI1_CLK (Flash clock), not SPI3. And INV_SEL inverts it.
            // PREVIOUS BUG: wrote gpio_func_out(23) = 0x100 (same issue)
            // PREVIOUS BUG: wrote gpio_func_in(64) = 0x3C = GPIO60 (non-existent)
            // All three are now omitted — native IO_MUX takes precedence when MCU_SEL=0.

            // ──────────────────────────────────────────────────────────────────
            // Step 5: GPIO direction enable
            // ──────────────────────────────────────────────────────────────────
            // Enable outputs: CS(5), MOSI(23), SCK(18)
            core::ptr::write_volatile(GPIO_ENABLE_W1TS,
                CS_BIT | (1 << 23) | (1 << 18));
            // Disable output driver on MISO(19) — input only
            core::ptr::write_volatile(GPIO_ENABLE_W1TC, 1 << 19);

            // ──────────────────────────────────────────────────────────────────
            // Step 6: CS idle HIGH (deasserted)
            // ──────────────────────────────────────────────────────────────────
            Self::cs_high();

            // ──────────────────────────────────────────────────────────────────
            // Step 7: SPI3 register configuration
            // ──────────────────────────────────────────────────────────────────
            // Clear all to reset state first
            core::ptr::write_volatile(SPI_USER,      0);
            core::ptr::write_volatile(SPI_CTRL,      0);
            core::ptr::write_volatile(SPI_CTRL2,     0);
            core::ptr::write_volatile(SPI_SLAVE,     0); // master mode (bit 30 = 0)
            core::ptr::write_volatile(SPI_CLOCK,     0);
            core::ptr::write_volatile(SPI_USER1,     0);
            core::ptr::write_volatile(SPI_USER2,     0);
            core::ptr::write_volatile(SPI_MOSI_DLEN, 0);
            core::ptr::write_volatile(SPI_MISO_DLEN, 0);

            // SPI_PIN: Disable ALL hardware CS lines; CPOL=0 (CK_IDLE_EDGE=0=idle LOW)
            // CS0_DIS[0]=1, CS1_DIS[1]=1, CS2_DIS[2]=1 → value = 0x07
            // PREVIOUS BUG: wrote (1<<1)|(1<<2) = 0x06
            //   → CS1_DIS=1, CS2_DIS=1 ✓ but CS0_DIS=0 ✗ — HW CS0 still active
            core::ptr::write_volatile(SPI_PIN, SPI_PIN_ALL_CS_DIS);

            // SPI_CTRL: 0 = MSB-first (WRD_BYTE_ORDER=0, RD_BYTE_ORDER=0),
            //               no dual/quad, FASTRD disabled. Correct for SD SPI mode 0.
            core::ptr::write_volatile(SPI_CTRL, 0);

            // SPI_USER: Full-duplex SPI Mode 0
            //   doutdin  [bit  0] = 1 : full-duplex — simultaneous TX and RX
            //   usr_mosi [bit 27] = 1 : enable MOSI (TX) phase
            //   usr_miso [bit 28] = 1 : enable MISO (RX) phase
            //
            // Combined: 0x18000001
            //
            // PREVIOUS BUG: wrote 0x0C000001
            //   = bit0(usr_mosi?) | bit26(usr_miso_highpart) | bit27(usr_mosi_highpart)
            //   This enabled DOUTDIN (happened to be bit0) ✓ but used upper half of
            //   512-bit FIFO for both MISO and MOSI, and did NOT enable usr_miso (bit28).
            //   No data was received from SD card.
            let spi_user_val = SPI_USER_MOSI | SPI_USER_MISO | SPI_USER_DOUTDIN;
            core::ptr::write_volatile(SPI_USER, spi_user_val);

            // SPI_CTRL2: 0 = no MISO/MOSI delay.
            // At 400 kHz this is safe. At ≥20 MHz consider setting MISO_DELAY_MODE.
            core::ptr::write_volatile(SPI_CTRL2, 0);

            // ──────────────────────────────────────────────────────────────────
            // Step 8: Clock = 400 kHz for SD card initialization phase
            // ──────────────────────────────────────────────────────────────────
            Self::set_speed_low();

            // ──────────────────────────────────────────────────────────────────
            // Verification readbacks
            // ──────────────────────────────────────────────────────────────────
            let r_user  = core::ptr::read_volatile(SPI_USER);
            let r_ctrl  = core::ptr::read_volatile(SPI_CTRL);
            let r_pin   = core::ptr::read_volatile(SPI_PIN);
            let r_clock = core::ptr::read_volatile(SPI_CLOCK);
            let r_slave = core::ptr::read_volatile(SPI_SLAVE);
            let r_ctrl2 = core::ptr::read_volatile(SPI_CTRL2);
            crate::println!("  [SPI3 VERIFY] SPI_USER =0x{:08X} expected=0x{:08X} OK={}",
                r_user, spi_user_val, r_user == spi_user_val);
            crate::println!("  [SPI3 VERIFY] SPI_CTRL =0x{:08X} expected=0x00000000 OK={}",
                r_ctrl, r_ctrl == 0);
            crate::println!("  [SPI3 VERIFY] SPI_PIN  =0x{:08X} expected=0x{:08X} OK={}",
                r_pin, SPI_PIN_ALL_CS_DIS, r_pin == SPI_PIN_ALL_CS_DIS);
            crate::println!("  [SPI3 VERIFY] SPI_CLOCK=0x{:08X}", r_clock);
            crate::println!("  [SPI3 VERIFY] SPI_SLAVE=0x{:08X} master_mode={}",
                r_slave, (r_slave >> 30) & 1 == 0);
            crate::println!("  [SPI3 VERIFY] SPI_CTRL2=0x{:08X}", r_ctrl2);

            // IO_MUX readbacks (MCU_SEL and IE status)
            let mux5  = core::ptr::read_volatile(IO_MUX_GPIO5);
            let mux18 = core::ptr::read_volatile(IO_MUX_GPIO18);
            let mux19 = core::ptr::read_volatile(IO_MUX_GPIO19);
            let mux23 = core::ptr::read_volatile(IO_MUX_GPIO23);
            crate::println!("  [IO_MUX] GPIO5  =0x{:08X} MCU_SEL={}",
                mux5,  (mux5  >> 12) & 7);
            crate::println!("  [IO_MUX] GPIO18 =0x{:08X} MCU_SEL={}",
                mux18, (mux18 >> 12) & 7);
            crate::println!("  [IO_MUX] GPIO19 =0x{:08X} MCU_SEL={} IE={} (should be MCU_SEL=0, IE=1)",
                mux19, (mux19 >> 12) & 7, (mux19 >> 9) & 1);
            crate::println!("  [IO_MUX] GPIO23 =0x{:08X} MCU_SEL={}",
                mux23, (mux23 >> 12) & 7);

            // GPIO19 must read as 1 with pull-up enabled and no external pull-down
            let gpio_in = core::ptr::read_volatile(0x3FF4403C as *const u32);
            crate::println!("  [GPIO_IN] =0x{:08X} GPIO19={} (expect 1 = MISO idle high)",
                gpio_in, (gpio_in >> 19) & 1);
        }
        crate::println!("[SPI3] VSPI init OK");
    }

    /// Full-duplex SPI transfer: up to 64 bytes per chunk.
    ///
    /// TX: sends bytes from `tx`, or 0xFF dummy bytes if `tx` is None.  
    /// RX: writes received bytes to `rx` if Some.  
    /// Returns true on success, false on hardware timeout.
    ///
    /// Bit ordering: W0[bit31] = first bit on MOSI = MSB of tx[0].
    /// After transaction: W0[31:24] = first received byte on MISO = rx[0].
    pub fn transfer_bytes(tx: Option<&[u8]>, mut rx: Option<&mut [u8]>, len: usize) -> bool {
        unsafe {
            if DBG_FIRST == 0 {
                DBG_FIRST = 1;
                crate::println!("[DBG] === First transfer_bytes ===");
                let r_user  = core::ptr::read_volatile(SPI_USER);
                let r_clock = core::ptr::read_volatile(SPI_CLOCK);
                let r_pin   = core::ptr::read_volatile(SPI_PIN);
                let gpio_in = core::ptr::read_volatile(0x3FF4403C as *const u32);
                crate::println!("[DBG] len={} SPI_USER=0x{:08X}", len, r_user);
                crate::println!("[DBG]   usr_mosi={} usr_miso={} doutdin={}",
                    (r_user >> 27) & 1, (r_user >> 28) & 1, (r_user >> 0) & 1);
                crate::println!("[DBG] SPI_CLOCK=0x{:08X} SPI_PIN=0x{:08X}", r_clock, r_pin);
                crate::println!("[DBG] GPIO19_IN={} (must be 1 for MISO idle)",
                    (gpio_in >> 19) & 1);
            }
        }

        let mut offset = 0usize;
        while offset < len {
            let chunk = core::cmp::min(len - offset, 64);

            unsafe {
                // ── Wait for SPI controller idle ──────────────────────────────
                let mut timeout = 500_000u32;
                while (core::ptr::read_volatile(SPI_CMD) & SPI_CMD_USR) != 0 {
                    timeout -= 1;
                    if timeout == 0 {
                        crate::println!("[SPI] TIMEOUT: SPI_CMD.USR stuck at offset {}", offset);
                        return false;
                    }
                }

                // ── Set MOSI and MISO bit lengths ─────────────────────────────
                // Value = (number_of_bits - 1). Both must match for full-duplex.
                let bits = (chunk * 8 - 1) as u32;
                core::ptr::write_volatile(SPI_MOSI_DLEN, bits);
                core::ptr::write_volatile(SPI_MISO_DLEN, bits);

                // ── Pack TX data into W0–W15 FIFO (Little-Endian / ESP-IDF) ──
                // W0[7:0]   = tx[offset+0] — LSB of W0 is first byte transmitted.
                // W0[15:8]  = tx[offset+1], etc.
                let words = (chunk + 3) / 4;
                for i in 0..words {
                    let mut word = 0u32;
                    for j in 0..4usize {
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

                // Memory barrier: all FIFO writes must complete before trigger
                core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);

                // ── Trigger transaction ────────────────────────────────────────
                let v = core::ptr::read_volatile(SPI_CMD);
                core::ptr::write_volatile(SPI_CMD, v | SPI_CMD_USR);

                // ── Poll for completion ────────────────────────────────────────
                let mut polls = 0u32;
                let mut timeout = 5_000_000u32;
                while (core::ptr::read_volatile(SPI_CMD) & SPI_CMD_USR) != 0 {
                    polls += 1;
                    timeout -= 1;
                    if timeout == 0 {
                        let gpio_in = core::ptr::read_volatile(0x3FF4403C as *const u32);
                        crate::println!("[SPI] USR TIMEOUT: offset={} chunk={} GPIO19={} polls={}",
                            offset, chunk, (gpio_in >> 19) & 1, polls);
                        return false;
                    }
                }

                if DBG_FIRST == 1 {
                    DBG_FIRST = 2;
                    crate::println!("[DBG] USR cleared after {} polls", polls);
                    let w0      = core::ptr::read_volatile((SPI3_BASE + 0x080) as *const u32);
                    let gpio_in = core::ptr::read_volatile(0x3FF4403C as *const u32);
                    crate::println!("[DBG] W0=0x{:08X} GPIO19={}", w0, (gpio_in >> 19) & 1);
                }

                // ── Unpack RX data from W0–Wn (Little-Endian / ESP-IDF) ───────
                // After full-duplex transaction: W0[7:0] = first received byte.
                // TX data has been overwritten by received data (DOUTDIN behavior).
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
        true
    }

    /// Send a raw SD SPI command and read its R1 response byte.
    ///
    /// `cmd_idx`   : command index (0–63)  
    /// `arg`       : 32-bit command argument  
    /// `crc`       : CRC7 byte with stop bit (0x95 for CMD0, 0x87 for CMD8, 0x01 otherwise)  
    /// `resp_extra`: number of additional response bytes after R1 (0 for R1, 4 for R3/R7)  
    ///
    /// Response polling: after the last command byte, the SD card responds within Ncr
    /// bytes (max 8 per spec). We send 0xFF dummy bytes and watch for a byte with bit7=0.
    pub fn send_cmd(cmd_idx: u8, arg: u32, crc: u8, resp_extra: usize)
        -> Result<(u8, u32), &'static str>
    {
        let cmd = [
            0x40 | cmd_idx,
            (arg >> 24) as u8,
            (arg >> 16) as u8,
            (arg >> 8)  as u8,
            arg          as u8,
            crc,
        ];
        crate::println!("  [CMD{}] TX: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
            cmd_idx, cmd[0], cmd[1], cmd[2], cmd[3], cmd[4], cmd[5]);

        // Transmit command bytes. Received bytes during this phase are discarded
        // (card returns 0xFF while processing the incoming command).
        if !Self::transfer_bytes(Some(&cmd), None, 6) {
            return Err("SPI timeout sending command bytes");
        }

        // Poll for R1 response by sending 0xFF dummy bytes.
        // SD SPI spec §7.5.4: response appears within Ncr ≤ 8 bytes.
        // We extend to 16 polls for robustness.
        let mut r1: u8 = 0xFF;
        let mut found  = false;
        let mut rx_buf = [0u8; 1];
        crate::print!("  [CMD{}] poll:", cmd_idx);
        for _ in 0..16u32 {
            if !Self::transfer_bytes(Some(&[0xFF]), Some(&mut rx_buf), 1) {
                return Err("SPI timeout polling for R1");
            }
            let b = rx_buf[0];
            crate::print!(" {:02X}", b);
            // Valid R1: bit 7 = 0 (0xxx_xxxx), value ≠ 0xFF
            if b != 0xFF && (b & 0x80) == 0 {
                r1 = b;
                found = true;
                break;
            }
        }
        crate::println!("");

        if !found {
            return Err("CMD response timeout (no valid R1 in 16 bytes)");
        }
        crate::println!("  [CMD{}] R1=0x{:02X}", cmd_idx, r1);

        // Read R3/R7 extra bytes if requested
        if resp_extra > 0 {
            let extra_len = core::cmp::min(resp_extra, 4);
            let dummy     = [0xFFu8; 4];
            let mut extra = [0u8; 4];
            if !Self::transfer_bytes(Some(&dummy[..extra_len]), Some(&mut extra[..extra_len]), extra_len) {
                return Err("SPI timeout reading extra response bytes");
            }
            let mut val = 0u32;
            for i in 0..extra_len {
                val = (val << 8) | extra[i] as u32;
            }
            crate::println!("  [CMD{}] extra=0x{:08X}", cmd_idx, val);
            Ok((r1, val))
        } else {
            Ok((r1, 0))
        }
    }

    /// Full SD card SPI-mode initialization sequence.
    /// Follows SD Physical Layer Specification v7.10 §6.4.1.
    pub fn card_reset() -> Result<(), &'static str> {
        crate::println!("[SD] card_reset() start");

        // ── Phase 1: Power-up clocks ───────────────────────────────────────────
        // Spec §6.4.1.2: Send ≥74 CLK cycles with CS=HIGH and MOSI=1.
        // These clock the card's internal state machine to ready state.
        Self::cs_high();
        let dummy80 = [0xFFu8; 10]; // 10 bytes × 8 bits = 80 clocks
        if !Self::transfer_bytes(Some(&dummy80), None, 10) {
            return Err("SPI timeout during 80 power-up clocks");
        }
        crate::println!("[SD] 80 power-up clocks sent (CS=HIGH, MOSI=FF)");

        // ── Phase 2: CMD0 — GO_IDLE_STATE ─────────────────────────────────────
        // Puts card into SPI mode. Card must see CS=LOW during CMD0.
        // Expected response: R1 = 0x01 (In Idle State flag set).
        Self::cs_low();
        let r = Self::send_cmd(0, 0x0000_0000, 0x95, 0);
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        let (r1, _) = r?;
        if r1 != 0x01 {
            crate::println!("[SD] CMD0 FAILED: R1=0x{:02X} (expected 0x01)", r1);
            return Err("CMD0: card did not enter idle state (R1 ≠ 0x01)");
        }
        crate::println!("[SD] CMD0 OK → card in SPI idle state");

        // ── Phase 3: CMD8 — SEND_IF_COND ──────────────────────────────────────
        // Determines SDv2 capability and voltage range acceptance.
        // Arg: VHS=0x01 (2.7–3.6V) + check pattern 0xAA.
        Self::cs_low();
        let r = Self::send_cmd(8, 0x0000_01AA, 0x87, 4);
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        let (r1, extra) = r?;

        let voltage  = ((extra >> 8) & 0xFF) as u8;
        let echo     = (extra & 0xFF) as u8;
        crate::println!("[SD] CMD8: R1=0x{:02X} VHS=0x{:02X} echo=0x{:02X}",
            r1, voltage, echo);

        let is_sdhc = if r1 == 0x01 && voltage == 0x01 && echo == 0xAA {
            crate::println!("[SD] CMD8 OK → SDv2, SDHC/SDXC capable");
            true
        } else if r1 & 0x04 != 0 {
            crate::println!("[SD] CMD8 illegal → SDv1 or MMC (pre-SDv2)");
            false
        } else {
            crate::println!("[SD] CMD8 error: R1=0x{:02X} extra=0x{:08X}", r1, extra);
            return Err("CMD8: voltage mismatch or unexpected response");
        };

        // ── Phase 4: ACMD41 — SD_SEND_OP_COND ────────────────────────────────
        // Poll until card leaves idle state (R1 = 0x00).
        // HCS bit (arg bit 30) = 1 for SDHC/SDXC, 0 for SDSC.
        let hcs = if is_sdhc { 0x4000_0000u32 } else { 0u32 };
        crate::println!("[SD] ACMD41 polling (HCS=0x{:08X})...", hcs);
        let mut retries = 2000u32;
        loop {
            // CMD55 must precede every ACMD
            Self::cs_low();
            let r = Self::send_cmd(55, 0, 0x01, 0);
            Self::cs_high();
            Self::transfer_bytes(Some(&[0xFF]), None, 1);
            let (r1, _) = r?;
            if r1 & 0x80 != 0 {
                crate::println!("[SD] CMD55 invalid R1=0x{:02X}", r1);
                return Err("ACMD41: CMD55 returned invalid R1");
            }

            Self::cs_low();
            let r = Self::send_cmd(41, hcs, 0x01, 0);
            Self::cs_high();
            Self::transfer_bytes(Some(&[0xFF]), None, 1);
            let (r1, _) = r?;

            if r1 == 0x00 {
                crate::println!("[SD] ACMD41 ready (R1=0x00)");
                break;
            }
            if r1 & 0x01 == 0 {
                // Neither idle nor ready — an error code
                crate::println!("[SD] ACMD41 error R1=0x{:02X}", r1);
                return Err("ACMD41: unexpected error");
            }
            // R1=0x01 = still idle/initializing; keep polling
            retries -= 1;
            if retries == 0 {
                return Err("ACMD41 timeout: card never left idle state");
            }
        }
        crate::println!("[SD] ACMD41 complete → card active");

        // ── Phase 5: CMD58 — READ_OCR ─────────────────────────────────────────
        // Reads the Operation Conditions Register.
        // OCR bit 30 = CCS: 1 = SDHC/SDXC (block-addressed), 0 = SDSC (byte-addressed)
        // OCR bit 31 = power-up status: 1 = card ready
        Self::cs_low();
        let r = Self::send_cmd(58, 0, 0x01, 4);
        Self::cs_high();
        Self::transfer_bytes(Some(&[0xFF]), None, 1);
        let (_r1, ocr) = r?;
        crate::println!("[SD] CMD58 OCR=0x{:08X} ready={} SDHC={}",
            ocr, (ocr >> 31) & 1, (ocr >> 30) & 1);

        crate::println!("[SD] card_reset() COMPLETE ✓");
        Ok(())
    }
}

pub fn spi_init() {
    RawSpi::init();
}
