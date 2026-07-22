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
}

const EMPTY_DEVICE: DeviceEntry = DeviceEntry {
    status: DeviceStatus::Free,
    name: [0; MAX_DEV_NAME],
    name_len: 0,
    owner_app: 0,
    driver_slot: 0,
    gpio_mask: 0,
    bus_handle: 0xFF,
};

pub static mut DEVICE_TABLE: [DeviceEntry; MAX_DEVICES] = [EMPTY_DEVICE; MAX_DEVICES];
pub static mut GPIO_OWNERSHIP: [u8; 40] = [0xFF; 40]; // 0xFF = unowned, otherwise owner_app ID

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

pub fn register_device(
    name: &str,
    app_id: u8,
    driver_slot: u8,
    gpio_pins: &[u8],
    bus_handle: u8,
) -> Result<usize, &'static str> {
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

pub fn format_proc_devices(out: &mut [u8]) -> usize {
    let mut written = 0;
    unsafe {
        for dev in DEVICE_TABLE.iter() {
            if dev.status != DeviceStatus::Free {
                let name = core::str::from_utf8(&dev.name[..dev.name_len as usize]).unwrap_or("?");
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
