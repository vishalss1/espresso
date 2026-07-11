//! GPIO driver module - read/write/mode for ESP32 pins
//!
//! Free GPIO: 2,12,15,16,17,18,23,25,26,27,32,33

const GPIO_BASE: u32 = 0x3FF44000;
const GPIO_IN_REG: u32 = GPIO_BASE + 0x038;
const GPIO_OUT_W1TS: u32 = GPIO_BASE + 0x008;
const GPIO_OUT_W1TC: u32 = GPIO_BASE + 0x00C;
const GPIO_ENABLE_W1TS: u32 = GPIO_BASE + 0x024;
const GPIO_ENABLE_W1TC: u32 = GPIO_BASE + 0x028;

const GPIO_MODE_INPUT: u8 = 0;
const GPIO_MODE_OUTPUT: u8 = 1;

pub unsafe fn gpio_read(pin: u8) -> u8 {
    if pin > 39 {
        return 0;
    }
    let val = core::ptr::read_volatile(GPIO_IN_REG as *const u32);
    ((val >> pin) & 1) as u8
}

pub unsafe fn gpio_write(pin: u8, val: u8) {
    if pin > 39 {
        return;
    }
    let bit = 1u32 << pin;
    if val != 0 {
        core::ptr::write_volatile(GPIO_OUT_W1TS as *mut u32, bit);
    } else {
        core::ptr::write_volatile(GPIO_OUT_W1TC as *mut u32, bit);
    }
}

pub unsafe fn gpio_mode(pin: u8, mode: u8) {
    if pin > 39 {
        return;
    }
    let bit = 1u32 << pin;
    if mode == GPIO_MODE_OUTPUT {
        core::ptr::write_volatile(GPIO_ENABLE_W1TS as *mut u32, bit);
    } else {
        core::ptr::write_volatile(GPIO_ENABLE_W1TC as *mut u32, bit);
    }
}
