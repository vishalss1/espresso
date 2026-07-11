//! I2C driver — bit-banged master on GPIO21 (SDA) / GPIO22 (SCL)
//!
//! Dedicated bus for SSD1306 display at 0x3C. No bus sharing.
//! Uses open-drain outputs with internal pull-ups.

const SDA_PIN: u8 = 21;
const SCL_PIN: u8 = 22;

const GPIO_BASE: u32 = 0x3FF44000;
const GPIO_OUT_W1TS: u32 = GPIO_BASE + 0x008;
const GPIO_OUT_W1TC: u32 = GPIO_BASE + 0x00C;
const GPIO_IN_REG: u32  = GPIO_BASE + 0x038;
const GPIO_ENABLE_W1TS: u32 = GPIO_BASE + 0x024;
const GPIO_ENABLE_W1TC: u32 = GPIO_BASE + 0x028;

const IO_MUX_BASE: u32 = 0x3FF49000;
const IO_MUX_GPIO21: u32 = IO_MUX_BASE + 0x07C;
const IO_MUX_GPIO22: u32 = IO_MUX_BASE + 0x080;

#[inline(always)]
fn delay_half() {
    // ~1.7us at 240MHz. For ~150KHz I2C, period ≈ 6.6us, half ≈ 3.3us.
    // ~400 NOPs ≈ 1.7us; with loop overhead ≈ 3.3us per half-period.
    let mut n: u32 = 400;
    unsafe { core::arch::asm!("1: addi {0}, {0}, -1; bnez {0}, 1b", inout(reg) n); }
}

/// Open-drain I2C: "high" = release bus (disable output, pull-up brings pin high).
/// "low" = drive bus low (set output to 0, enable output).
/// Using GPIO_OUT_W1TC to pre-clear output bit so enabling output drives low.

#[inline(always)]
fn sda_low() {
    unsafe {
        core::ptr::write_volatile(GPIO_OUT_W1TC as *mut u32, 1 << SDA_PIN);
        core::ptr::write_volatile(GPIO_ENABLE_W1TS as *mut u32, 1 << SDA_PIN);
    }
}

#[inline(always)]
fn sda_high() {
    unsafe { core::ptr::write_volatile(GPIO_ENABLE_W1TC as *mut u32, 1 << SDA_PIN); }
}

#[inline(always)]
fn scl_low() {
    unsafe {
        core::ptr::write_volatile(GPIO_OUT_W1TC as *mut u32, 1 << SCL_PIN);
        core::ptr::write_volatile(GPIO_ENABLE_W1TS as *mut u32, 1 << SCL_PIN);
    }
}

#[inline(always)]
fn scl_high() {
    unsafe { core::ptr::write_volatile(GPIO_ENABLE_W1TC as *mut u32, 1 << SCL_PIN); }
}

#[inline(always)]
fn sda_read() -> bool {
    unsafe {
        let val = core::ptr::read_volatile(GPIO_IN_REG as *const u32);
        (val & (1 << SDA_PIN)) != 0
    }
}

pub fn init() {
    unsafe {
        // GPIO21/22 → function 0 (GPIO via IO_MUX), pull-up enabled
        core::ptr::write_volatile(IO_MUX_GPIO21 as *mut u32, (1 << 8) | 0);
        core::ptr::write_volatile(IO_MUX_GPIO22 as *mut u32, (1 << 8) | 0);

        // Pre-clear output values (so when output is enabled, it drives low)
        core::ptr::write_volatile(GPIO_OUT_W1TC as *mut u32,
            (1 << SDA_PIN) | (1 << SCL_PIN));

        // Both lines released (output disabled → pulled high by pull-ups)
        // No GPIO_ENABLE_W1TS here — open-drain: high = output disabled
    }
}

fn start() {
    sda_high();
    scl_high();
    delay_half();
    sda_low();
    delay_half();
    scl_low();
    delay_half();
}

fn stop() {
    sda_low();
    delay_half();
    scl_high();
    delay_half();
    sda_high();
    delay_half();
}

fn write_bit(bit: bool) {
    if bit { sda_high(); } else { sda_low(); }
    delay_half();
    scl_high();
    delay_half();
    scl_low();
    delay_half();
}

/// Write one byte. Returns true if ACK received.
fn write_byte(b: u8) -> bool {
    for i in (0..8).rev() {
        write_bit((b >> i) & 1 != 0);
    }
    // Read ACK
    sda_high(); // float SDA for slave ACK
    delay_half();
    scl_high();
    delay_half();
    let ack = !sda_read(); // ACK = low
    scl_low();
    delay_half();
    ack
}

/// Write buffer to a 7-bit I2C address. Returns true on success.
pub fn write(addr7: u8, data: &[u8]) -> bool {
    start();
    let ack1 = write_byte(addr7 << 1); // write address
    if !ack1 {
        stop();
        return false;
    }
    for &b in data {
        if !write_byte(b) {
            stop();
            return false;
        }
    }
    stop();
    true
}

/// Write with repeated start + read (for SSD1306 status queries).
pub fn write_read(addr7: u8, tx: &[u8], rx: &mut [u8]) -> bool {
    start();
    if !write_byte(addr7 << 1) { stop(); return false; }
    for &b in tx {
        if !write_byte(b) { stop(); return false; }
    }
    // Repeated start
    start();
    if !write_byte((addr7 << 1) | 1) { stop(); return false; }
    for i in 0..rx.len() {
        sda_high();
        delay_half();
        scl_high();
        delay_half();
        rx[i] = if sda_read() { 0xFF } else { 0x00 };
        scl_low();
        delay_half();
        // NACK last byte, ACK others
        if i == rx.len() - 1 {
            // NACK
        } else {
            // ACK — pull SDA low
            sda_low();
            delay_half();
            scl_high();
            delay_half();
            scl_low();
            delay_half();
        }
    }
    stop();
    true
}
