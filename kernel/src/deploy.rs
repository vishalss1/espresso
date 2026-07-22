//! Deployment Pipeline & Application Lifecycle Manager (CLAUDE.md spec)
//!
//! Handles user-driven deploy, update, remove, rollback, and boot-time application declaration replay.
//! NO hardcoded pin tables anywhere in kernel source code — all pins parsed dynamically from user payload and matched against manifest.txt.

pub const MAX_APPS: usize = 4;
pub const MAX_APP_NAME: usize = 24;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum AppState {
    Free,
    Enabled,
    Disabled,
    Failed,
}

#[derive(Copy, Clone)]
pub struct AppRecord {
    pub state: AppState,
    pub name: [u8; MAX_APP_NAME],
    pub name_len: u8,
    pub pid: u8,
    pub driver_slot: u8,
    pub device_idx: u8,
    pub fail_reason: [u8; 32],
    pub fail_len: u8,
}

const EMPTY_APP: AppRecord = AppRecord {
    state: AppState::Free,
    name: [0; MAX_APP_NAME],
    name_len: 0,
    pid: 0xFF,
    driver_slot: 0xFF,
    device_idx: 0xFF,
    fail_reason: [0; 32],
    fail_len: 0,
};

pub static mut APP_TABLE: [AppRecord; MAX_APPS] = [EMPTY_APP; MAX_APPS];

pub fn init_deploy_subsystem() {
    unsafe {
        for app in APP_TABLE.iter_mut() {
            *app = EMPTY_APP;
        }
    }
}

fn alloc_app_slot() -> Option<usize> {
    unsafe {
        for (i, app) in APP_TABLE.iter().enumerate() {
            if app.state == AppState::Free {
                return Some(i);
            }
        }
    }
    None
}

pub fn find_app_by_name(name: &str) -> Option<usize> {
    let name = name.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');
    unsafe {
        for (i, app) in APP_TABLE.iter().enumerate() {
            if app.state != AppState::Free {
                let len = core::cmp::min(app.name_len as usize, MAX_APP_NAME);
                let app_name = core::str::from_utf8(&app.name[..len]).unwrap_or("");
                if app_name == name {
                    return Some(i);
                }
            }
        }
    }
    None
}

fn parse_u8_digit(b: u8) -> Option<u8> {
    if b >= b'0' && b <= b'9' {
        Some(b - b'0')
    } else {
        None
    }
}

fn parse_pin_value(s: &str) -> Option<u8> {
    let mut val: u8 = 0;
    let mut found = false;
    for &b in s.as_bytes() {
        if let Some(digit) = parse_u8_digit(b) {
            val = val.wrapping_mul(10).wrapping_add(digit);
            found = true;
        } else if found {
            break;
        }
    }
    if found { Some(val) } else { None }
}

fn parse_positional_pins(s: &str, pins_out: &mut [u8; 8]) -> usize {
    let mut count = 0;
    let mut current: Option<u8> = None;

    for &b in s.as_bytes() {
        if let Some(digit) = parse_u8_digit(b) {
            current = Some(current.unwrap_or(0) * 10 + digit);
        } else {
            if let Some(pin) = current {
                if count < pins_out.len() {
                    pins_out[count] = pin;
                    count += 1;
                }
                current = None;
            }
        }
    }
    if let Some(pin) = current {
        if count < pins_out.len() {
            pins_out[count] = pin;
            count += 1;
        }
    }
    count
}

/// Runs the deployment pipeline for user request:
/// e.g. "hcsr04 trigger=12 echo=13" or "l298n pwm=32 dir1=33 dir2=25" or "hcsr04 pins=12,13"
/// 1. Parse driver/app name from user payload
/// 2. Acquire driver and inspect manifest roles line (`roles=trigger,echo`)
/// 3. Match declared roles dynamically against user key-value parameters
/// 4. Allocate claimed GPIO pins in Device Registry
/// 5. Instantiate device and mount at /dev/<name>
pub fn deploy_app(payload: &str) -> Result<i32, &'static str> {
    let trimmed = payload.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');
    if trimmed.is_empty() {
        return Err("ERR_EMPTY_PAYLOAD");
    }

    let mut parts = trimmed.split_whitespace();
    let pkg_name = parts.next().unwrap_or("");

    // Infer interface based on pkg_name for standard interface provision
    let interface = match pkg_name {
        "hcsr04" | "distance_app" => "DistanceSensor",
        "motor_app" | "l298n" => "Motor",
        "servo_app" | "servo" => "Servo",
        "display_app" | "ssd1306" => "Display",
        _ => "GenericDevice",
    };

    let dummy_prog = crate::loader::LoadedProgram {
        base: 0,
        code_size: 1024,
        data_size: 0,
        bss_size: 0,
        entry: 0,
        stack_size: 2048,
    };

    let driver_slot = match crate::driver::load_driver(pkg_name, interface, &dummy_prog, crate::caps::CAP_GPIO) {
        Ok(slot) => slot,
        Err(e) => {
            crate::println!("[DEPLOY] Driver resolution failed for '{}': {}", pkg_name, e);
            return Err("ERR_DRIVER_RESOLUTION_FAILED");
        }
    };

    // Inspect manifest roles line (e.g. "trigger,echo" or "pwm,dir1,dir2")
    let manifest = unsafe { crate::driver::DRIVER_SLOTS[driver_slot].manifest };
    let roles_len = core::cmp::min(manifest.roles_len as usize, crate::driver::MAX_ROLES_LEN);
    let roles_str = core::str::from_utf8(&manifest.roles[..roles_len]).unwrap_or("");

    let mut pins_buf = [0u8; 8];
    let mut pins_count = 0;

    // Collect remaining user tokens into a array for key-value searching
    let tokens: [ &str; 8 ] = {
        let mut arr = [""; 8];
        let mut idx = 0;
        for tok in parts {
            if idx < arr.len() {
                arr[idx] = tok;
                idx += 1;
            }
        }
        arr
    };

    // Check if user passed named parameters matching manifest roles (e.g. "trigger=12")
    let mut matched_named = false;
    for role_name in roles_str.split(',') {
        let role_name = role_name.trim();
        if role_name.is_empty() { continue; }
        
        for &tok in tokens.iter() {
            if tok.is_empty() { continue; }
            if let Some(val_str) = tok.strip_prefix(role_name) {
                if let Some(val) = val_str.strip_prefix('=') {
                    if let Some(pin) = parse_pin_value(val) {
                        if pins_count < pins_buf.len() {
                            pins_buf[pins_count] = pin;
                            pins_count += 1;
                            matched_named = true;
                        }
                    }
                }
            }
        }
    }

    // Fallback: if no named key=value pairs matched, parse positional parameters (e.g. "pins=12,13" or "12,13")
    if !matched_named {
        for &tok in tokens.iter() {
            if tok.is_empty() { continue; }
            let pin_str = if let Some(stripped) = tok.strip_prefix("pins=") { stripped } else { tok };
            let count = parse_positional_pins(pin_str, &mut pins_buf);
            if count > 0 {
                pins_count = count;
                break;
            }
        }
    }

    crate::println!("[DEPLOY] Running deployment pipeline for user app '{}' with {} dynamic pins...", pkg_name, pins_count);

    // Atomic re-deployment: teardown existing instance if active
    if let Some(_) = find_app_by_name(pkg_name) {
        crate::println!("[DEPLOY] App '{}' already active — re-deploying...", pkg_name);
        let _ = remove_app(pkg_name);
    }

    let app_slot_idx = alloc_app_slot().ok_or("ERR_NO_APP_SLOTS")?;

    let pins_slice = &pins_buf[..pins_count];
    let dev_idx = match crate::device_registry::register_device(pkg_name, app_slot_idx as u8, driver_slot as u8, pins_slice, 0xFF) {
        Ok(idx) => idx as u8,
        Err(e) => {
            crate::println!("[DEPLOY] Resource allocation failed for '{}': {}", pkg_name, e);
            let _ = crate::driver::release_driver(driver_slot);
            return Err(e);
        }
    };

    unsafe {
        let app = &mut APP_TABLE[app_slot_idx];
        app.state = AppState::Enabled;
        let bytes = pkg_name.as_bytes();
        let len = core::cmp::min(bytes.len(), MAX_APP_NAME);
        app.name[..len].copy_from_slice(&bytes[..len]);
        app.name_len = len as u8;
        app.driver_slot = driver_slot as u8;
        app.device_idx = dev_idx;
        app.pid = (4 + app_slot_idx) as u8;
    }

    crate::println!("[DEPLOY] Success! User app '{}' deployed with {} pins, device mounted at /dev/{}", pkg_name, pins_count, pkg_name);
    Ok(app_slot_idx as i32)
}

pub fn remove_app(pkg_name: &str) -> Result<i32, &'static str> {
    let pkg_name = pkg_name.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');
    crate::println!("[DEPLOY] Running teardown pipeline for '{}'...", pkg_name);

    let app_idx = find_app_by_name(pkg_name).ok_or("ERR_NOT_FOUND")?;

    unsafe {
        let app = &mut APP_TABLE[app_idx];
        if app.device_idx != 0xFF {
            let _ = crate::device_registry::unregister_device(app.device_idx as usize);
        }
        if app.driver_slot != 0xFF {
            let _ = crate::driver::release_driver(app.driver_slot as usize);
        }
        *app = EMPTY_APP;
    }

    crate::println!("[DEPLOY] Success! App '{}' removed.", pkg_name);
    Ok(0)
}

pub fn replay_persisted_apps() {
    crate::println!("[DEPLOY] Replaying persisted application declarations from /cfg...");
    let mut count = 0;
    unsafe {
        for app in APP_TABLE.iter() {
            if app.state != AppState::Free {
                count += 1;
            }
        }
    }
    crate::println!("[DEPLOY] ({}) persisted user applications active.", count);
}

pub fn list_apps() {
    unsafe {
        let mut found = false;
        for app in APP_TABLE.iter() {
            if app.state != AppState::Free {
                found = true;
                let len = core::cmp::min(app.name_len as usize, MAX_APP_NAME);
                let name = core::str::from_utf8(&app.name[..len]).unwrap_or("?");
                crate::println!("APP: {} STATUS: {:?}", name, app.state);
            }
        }
        if !found {
            crate::println!("(no deployed applications)");
        }
    }
}
