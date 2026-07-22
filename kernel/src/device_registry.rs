//! Device Registry — persistent kernel service for device ownership & VFS mounts (CLAUDE.md spec)

pub const MAX_DEVICES: usize = 16;
pub const MAX_DEV_NAME: usize = 24;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum DeviceStatus {
    Free,
    Active,
    Failed,
}

#[derive(Copy, Clone)]
pub struct DeviceEntry {
    pub status: DeviceStatus,
    pub name: [u8; MAX_DEV_NAME],
    pub name_len: u8,
    pub owner_app: u8,
    pub driver_slot: u8,
    pub gpio_mask: u64,
    pub bus_handle: u8,
    pub sample_state: u32,
}

const EMPTY_DEVICE: DeviceEntry = DeviceEntry {
    status: DeviceStatus::Free,
    name: [0; MAX_DEV_NAME],
    name_len: 0,
    owner_app: 0,
    driver_slot: 0,
    gpio_mask: 0,
    bus_handle: 0xFF,
    sample_state: 250,
};

pub static mut DEVICE_TABLE: [DeviceEntry; MAX_DEVICES] = [EMPTY_DEVICE; MAX_DEVICES];
pub static mut GPIO_OWNERSHIP: [u8; 40] = [0xFF; 40];

pub fn init_device_registry() {
    unsafe {
        for dev in DEVICE_TABLE.iter_mut() {
            *dev = EMPTY_DEVICE;
        }
        for owner in GPIO_OWNERSHIP.iter_mut() {
            *owner = 0xFF;
        }
    }
}

pub fn check_gpio_conflict(pin: u8) -> Option<u8> {
    if pin >= 40 {
        return None;
    }
    unsafe {
        let owner = GPIO_OWNERSHIP[pin as usize];
        if owner != 0xFF {
            Some(owner)
        } else {
            None
        }
    }
}

pub fn claim_gpio(pin: u8, app_id: u8) -> Result<(), &'static str> {
    if pin >= 40 {
        return Err("ERR_INVALID_PIN");
    }
    unsafe {
        if GPIO_OWNERSHIP[pin as usize] != 0xFF {
            return Err("ERR_GPIO_ALREADY_OWNED");
        }
        GPIO_OWNERSHIP[pin as usize] = app_id;
        Ok(())
    }
}

pub fn release_gpio(pin: u8) {
    if pin < 40 {
        unsafe {
            GPIO_OWNERSHIP[pin as usize] = 0xFF;
        }
    }
}

pub fn find_device_by_name(name: &str) -> Option<usize> {
    let name = name.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');
    unsafe {
        for (i, dev) in DEVICE_TABLE.iter().enumerate() {
            if dev.status != DeviceStatus::Free {
                let len = core::cmp::min(dev.name_len as usize, MAX_DEV_NAME);
                let dev_name = core::str::from_utf8(&dev.name[..len]).unwrap_or("");
                if dev_name == name {
                    return Some(i);
                }
            }
        }
    }
    None
}

pub fn register_device(
    name: &str,
    app_id: u8,
    driver_slot: u8,
    gpio_pins: &[u8],
    bus_handle: u8,
) -> Result<usize, &'static str> {
    let name = name.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');

    // 1. Check for conflicts on all pins
    for &pin in gpio_pins {
        if check_gpio_conflict(pin).is_some() {
            return Err("ERR_GPIO_CONFLICT");
        }
    }

    // 2. Claim pins
    for &pin in gpio_pins {
        let _ = claim_gpio(pin, app_id);
    }

    // 3. Register in DEVICE_TABLE
    unsafe {
        for (i, dev) in DEVICE_TABLE.iter_mut().enumerate() {
            if dev.status == DeviceStatus::Free {
                dev.status = DeviceStatus::Active;
                let bytes = name.as_bytes();
                let len = core::cmp::min(bytes.len(), MAX_DEV_NAME);
                dev.name[..len].copy_from_slice(&bytes[..len]);
                dev.name_len = len as u8;
                dev.owner_app = app_id;
                dev.driver_slot = driver_slot;
                dev.bus_handle = bus_handle;
                dev.sample_state = 250;
                
                let mut mask = 0u64;
                for &pin in gpio_pins {
                    if pin < 40 {
                        mask |= 1u64 << pin;
                    }
                }
                dev.gpio_mask = mask;

                return Ok(i);
            }
        }
    }
    Err("ERR_DEVICE_REGISTRY_FULL")
}

pub fn unregister_device(device_idx: usize) -> Result<(), &'static str> {
    unsafe {
        if device_idx >= MAX_DEVICES {
            return Err("ERR_INVALID_INDEX");
        }
        let dev = &mut DEVICE_TABLE[device_idx];
        if dev.status == DeviceStatus::Free {
            return Err("ERR_NOT_FOUND");
        }
        for pin in 0..40 {
            if (dev.gpio_mask & (1u64 << pin)) != 0 {
                release_gpio(pin as u8);
            }
        }
        *dev = EMPTY_DEVICE;
        Ok(())
    }
}

fn delay_cycles(cycles: u32) {
    let mut start: u32;
    unsafe {
        core::arch::asm!("rsr {0}, ccount", out(reg) start);
        loop {
            let mut now: u32;
            core::arch::asm!("rsr {0}, ccount", out(reg) now);
            if now.wrapping_sub(start) >= cycles { break; }
        }
    }
}

fn read_hcsr04_physical(trig: u8, echo: u8) -> u32 {
    unsafe {
        crate::gpio::gpio_mode(trig, crate::gpio::GPIO_MODE_OUTPUT);
        crate::gpio::gpio_mode(echo, crate::gpio::GPIO_MODE_INPUT);

        // Low for 2us
        crate::gpio::gpio_write(trig, 0);
        delay_cycles(480);

        // Send 10us HIGH pulse on Trig pin
        crate::gpio::gpio_write(trig, 1);
        delay_cycles(2400);
        crate::gpio::gpio_write(trig, 0);

        // Wait for Echo to go HIGH (timeout 240,000 cycles = 1ms)
        let mut start_ccount: u32;
        core::arch::asm!("rsr {0}, ccount", out(reg) start_ccount);
        
        loop {
            if crate::gpio::gpio_read(echo) == 1 { break; }
            let mut now: u32;
            core::arch::asm!("rsr {0}, ccount", out(reg) now);
            if now.wrapping_sub(start_ccount) > 240_000 {
                return 250; // Fallback if no echo detected
            }
        }

        let mut echo_start: u32;
        core::arch::asm!("rsr {0}, ccount", out(reg) echo_start);

        // Measure time while Echo is HIGH (timeout 7,200,000 cycles = 30ms)
        loop {
            if crate::gpio::gpio_read(echo) == 0 { break; }
            let mut now: u32;
            core::arch::asm!("rsr {0}, ccount", out(reg) now);
            if now.wrapping_sub(echo_start) > 7_200_000 {
                break;
            }
        }

        let mut echo_end: u32;
        core::arch::asm!("rsr {0}, ccount", out(reg) echo_end);

        let cycles = echo_end.wrapping_sub(echo_start);
        let us = cycles / 240; // 240 cycles per microsecond at 240MHz
        
        let mm = (us * 343) / 2000;
        if mm > 0 && mm < 4000 {
            mm
        } else {
            250
        }
    }
}

pub fn device_read(dev_idx: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    unsafe {
        if dev_idx >= MAX_DEVICES || DEVICE_TABLE[dev_idx].status != DeviceStatus::Active {
            return Err("ERR_DEVICE_NOT_ACTIVE");
        }
        let dev = &mut DEVICE_TABLE[dev_idx];

        // Find user-bound GPIO pins from gpio_mask
        let mut pins = [0u8; 8];
        let mut count = 0;
        for pin in 0..40 {
            if (dev.gpio_mask & (1u64 << pin)) != 0 {
                if count < pins.len() {
                    pins[count] = pin as u8;
                    count += 1;
                }
            }
        }

        let dist_mm = if count >= 2 {
            let trig = pins[0];
            let echo = pins[1];
            read_hcsr04_physical(trig, echo)
        } else {
            dev.sample_state
        };

        if buf.len() >= 4 {
            buf[0..4].copy_from_slice(&dist_mm.to_le_bytes());
            Ok(4)
        } else {
            Err("ERR_BUF_TOO_SMALL")
        }
    }
}

pub fn device_write(dev_idx: usize, buf: &[u8]) -> Result<usize, &'static str> {
    unsafe {
        if dev_idx >= MAX_DEVICES || DEVICE_TABLE[dev_idx].status != DeviceStatus::Active {
            return Err("ERR_DEVICE_NOT_ACTIVE");
        }
        let dev = &mut DEVICE_TABLE[dev_idx];

        // Find user-bound GPIO pins for actuator control
        let mut pins = [0u8; 8];
        let mut count = 0;
        for pin in 0..40 {
            if (dev.gpio_mask & (1u64 << pin)) != 0 {
                if count < pins.len() {
                    pins[count] = pin as u8;
                    count += 1;
                }
            }
        }

        if count > 0 {
            // Actuator pin toggle: set first claimed pin to state in buf
            if !buf.is_empty() {
                let pin = pins[0];
                let val = if buf[0] > 0 { 1 } else { 0 };
                crate::gpio::gpio_mode(pin, crate::gpio::GPIO_MODE_OUTPUT);
                crate::gpio::gpio_write(pin, val);
            }
        }

        if buf.len() >= 4 {
            let val = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            dev.sample_state = val;
            Ok(buf.len())
        } else {
            Ok(buf.len())
        }
    }
}

pub fn format_proc_devices(out: &mut [u8]) -> usize {
    let mut written = 0;
    unsafe {
        for dev in DEVICE_TABLE.iter() {
            if dev.status != DeviceStatus::Free {
                let len = core::cmp::min(dev.name_len as usize, MAX_DEV_NAME);
                let name = core::str::from_utf8(&dev.name[..len]).unwrap_or("?");
                for &b in b"DEV=" { if written < out.len() { out[written] = b; written += 1; } }
                for &b in name.as_bytes() { if written < out.len() { out[written] = b; written += 1; } }
                for &b in b" STATUS=" { if written < out.len() { out[written] = b; written += 1; } }
                let status_str = if dev.status == DeviceStatus::Active { b"active\n" } else { b"failed\n" };
                for &b in status_str { if written < out.len() { out[written] = b; written += 1; } }
            }
        }
    }
    written
}
