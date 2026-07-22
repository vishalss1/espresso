//! GPIO driver module — read/write/mode for ESP32 pins (CLAUDE.md spec)

const GPIO_BASE: u32 = 0x3FF44000;
const GPIO_IN_REG: u32 = GPIO_BASE + 0x038;
const GPIO_OUT_W1TS: u32 = GPIO_BASE + 0x008;
const GPIO_OUT_W1TC: u32 = GPIO_BASE + 0x00C;
const GPIO_ENABLE_W1TS: u32 = GPIO_BASE + 0x024;
const GPIO_ENABLE_W1TC: u32 = GPIO_BASE + 0x028;

pub const GPIO_MODE_INPUT: u8 = 0;
pub const GPIO_MODE_OUTPUT: u8 = 1;

pub enum PinCheckResult {
    Valid,
    InputOnly,
    Strapping,
    Reserved,
}

pub fn check_pin_validity(pin: u8, mode: u8) -> PinCheckResult {
    match pin {
        // Internal Flash SPI pins (never assignable)
        6..=11 | 0 => PinCheckResult::Reserved,
        // Input-only pins
        34..=39 => {
            if mode == GPIO_MODE_OUTPUT {
                PinCheckResult::InputOnly
            } else {
                PinCheckResult::Valid
            }
        }
        // Strapping pins (assignable but flagged)
        2 | 5 | 12 | 15 => PinCheckResult::Strapping,
        // Valid assignable GPIOs
        4 | 13..=19 | 21..=23 | 25..=27 | 32 | 33 => PinCheckResult::Valid,
        _ => PinCheckResult::Reserved,
    }
}

pub unsafe fn gpio_read(pin: u8) -> u8 {
    if pin > 39 {
        return 0;
    }
    let val = core::ptr::read_volatile(GPIO_IN_REG as *const u32);
    ((val >> pin) & 1) as u8
}

pub unsafe fn gpio_write(pin: u8, val: u8) {
    if pin > 39 || (34..=39).contains(&pin) {
        return; // Input-only pins cannot be written
    }
    let bit = 1u32 << pin;
    if val != 0 {
        core::ptr::write_volatile(GPIO_OUT_W1TS as *mut u32, bit);
    } else {
        core::ptr::write_volatile(GPIO_OUT_W1TC as *mut u32, bit);
    }
}

fn pin_to_io_mux_reg(pin: u8) -> Option<u32> {
    const IO_MUX_BASE: u32 = 0x3FF49000;
    let offset = match pin {
        36 => Some(0x04),
        37 => Some(0x08),
        38 => Some(0x0C),
        39 => Some(0x10),
        34 => Some(0x14),
        35 => Some(0x18),
        32 => Some(0x1C),
        33 => Some(0x20),
        25 => Some(0x24),
        26 => Some(0x28),
        27 => Some(0x2C),
        14 => Some(0x30),
        12 => Some(0x34),
        13 => Some(0x38),
        15 => Some(0x3C),
        2  => Some(0x40),
        0  => Some(0x44),
        4  => Some(0x48),
        16 => Some(0x4C),
        17 => Some(0x50),
        9  => Some(0x54),
        10 => Some(0x58),
        11 => Some(0x5C),
        6  => Some(0x60),
        7  => Some(0x64),
        8  => Some(0x68),
        5  => Some(0x6C),
        18 => Some(0x70),
        19 => Some(0x74),
        21 => Some(0x7C),
        22 => Some(0x80),
        23 => Some(0x8C),
        _ => None,
    };
    offset.map(|off| IO_MUX_BASE + off)
}

pub unsafe fn gpio_mode(pin: u8, mode: u8) {
    if pin > 39 {
        return;
    }
    match check_pin_validity(pin, mode) {
        PinCheckResult::Reserved | PinCheckResult::InputOnly => return,
        _ => {}
    }

    if pin == 25 || pin == 26 {
        // Disable DAC analog override to allow digital GPIO function
        let dac_reg = if pin == 25 { 0x3FF48484 } else { 0x3FF48488 };
        let mut dac_val = core::ptr::read_volatile(dac_reg as *const u32);
        dac_val &= !(1 << 18); // Clear PDACx_XPD_DAC (bit 18) to power down DAC
        dac_val |= 1 << 10;   // Set PDACx_DAC_XPD_FORCE (bit 10) to force power down
        dac_val |= 1 << 17;   // Set PDACx_MUX_SEL (bit 17) to select digital function
        core::ptr::write_volatile(dac_reg as *mut u32, dac_val);
    }

    let bit = 1u32 << pin;
    if mode == GPIO_MODE_OUTPUT {
        core::ptr::write_volatile(GPIO_ENABLE_W1TS as *mut u32, bit);
    } else {
        core::ptr::write_volatile(GPIO_ENABLE_W1TC as *mut u32, bit);
    }
    
    // Direct GPIO output routing
    if pin < 40 {
        let func_out_reg = 0x3FF44530 + (pin as u32) * 4;
        core::ptr::write_volatile(func_out_reg as *mut u32, 0x100);
    }

    if let Some(reg) = pin_to_io_mux_reg(pin) {
        let mut val = core::ptr::read_volatile(reg as *const u32);
        val &= !0x7000;
        val |= 2 << 12; // GPIO function
        if mode == GPIO_MODE_INPUT {
            val |= 1 << 9; // Enable input buffer
        }
        core::ptr::write_volatile(reg as *const u32 as *mut u32, val);
    }
}
