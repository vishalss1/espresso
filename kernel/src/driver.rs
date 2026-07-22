//! Driver Manager — loads/unloads .espr driver binaries into 8 static driver slots (CLAUDE.md spec)
//!
//! Enforces capability bitmask, interface provisions, reference-counted RAM residency, and manifest.txt parsing.

use crate::loader::LoadedProgram;

pub const MAX_SLOTS: usize = 8;
pub const MAX_DRIVER_NAME: usize = 24;
pub const MAX_INTERFACE_NAME: usize = 24;
pub const MAX_ROLES_LEN: usize = 64;

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SlotState {
    Free,
    Loaded,
    Active,
}

#[derive(Copy, Clone)]
pub struct DriverManifest {
    pub name: [u8; MAX_DRIVER_NAME],
    pub name_len: u8,
    pub interface: [u8; MAX_INTERFACE_NAME],
    pub interface_len: u8,
    pub roles: [u8; MAX_ROLES_LEN],
    pub roles_len: u8,
    pub permissions: u32,
    pub memory_bytes: u32,
    pub threads: u8,
}

const EMPTY_MANIFEST: DriverManifest = DriverManifest {
    name: [0; MAX_DRIVER_NAME],
    name_len: 0,
    interface: [0; MAX_INTERFACE_NAME],
    interface_len: 0,
    roles: [0; MAX_ROLES_LEN],
    roles_len: 0,
    permissions: crate::caps::CAP_GPIO,
    memory_bytes: 4096,
    threads: 0,
};

#[derive(Copy, Clone)]
pub struct DriverSlot {
    pub state: SlotState,
    pub manifest: DriverManifest,
    pub base: usize,
    pub entry: usize,
    pub stack_size: usize,
    pub code_size: usize,
    pub data_size: usize,
    pub ref_count: usize,
}

const EMPTY_SLOT: DriverSlot = DriverSlot {
    state: SlotState::Free,
    manifest: EMPTY_MANIFEST,
    base: 0,
    entry: 0,
    stack_size: 0,
    code_size: 0,
    data_size: 0,
    ref_count: 0,
};

pub static mut DRIVER_SLOTS: [DriverSlot; MAX_SLOTS] = [EMPTY_SLOT; MAX_SLOTS];

pub fn init_slots() {
    unsafe {
        for slot in DRIVER_SLOTS.iter_mut() {
            *slot = EMPTY_SLOT;
        }
    }
}

pub fn alloc_slot() -> Option<usize> {
    unsafe {
        for (i, slot) in DRIVER_SLOTS.iter().enumerate() {
            if slot.state == SlotState::Free {
                return Some(i);
            }
        }
    }
    None
}

pub fn find_driver_by_name(name: &str) -> Option<usize> {
    let name = name.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');
    unsafe {
        for (i, slot) in DRIVER_SLOTS.iter().enumerate() {
            if slot.state != SlotState::Free {
                let len = core::cmp::min(slot.manifest.name_len as usize, MAX_DRIVER_NAME);
                let slot_name = core::str::from_utf8(&slot.manifest.name[..len]).unwrap_or("");
                if slot_name == name {
                    return Some(i);
                }
            }
        }
    }
    None
}

pub fn find_driver_by_interface(interface: &str) -> Option<usize> {
    let interface = interface.trim_matches(|c| c == '\r' || c == '\n' || c == ' ' || c == '\0');
    unsafe {
        for (i, slot) in DRIVER_SLOTS.iter().enumerate() {
            if slot.state != SlotState::Free {
                let len = core::cmp::min(slot.manifest.interface_len as usize, MAX_INTERFACE_NAME);
                let slot_if = core::str::from_utf8(&slot.manifest.interface[..len]).unwrap_or("");
                if slot_if == interface {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Parses manifest.txt string content into a DriverManifest struct
pub fn parse_manifest(text: &str) -> DriverManifest {
    let mut m = EMPTY_MANIFEST;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') { continue; }
        if let Some(val) = line.strip_prefix("name=") {
            let val = val.trim();
            let bytes = val.as_bytes();
            let len = core::cmp::min(bytes.len(), MAX_DRIVER_NAME);
            m.name[..len].copy_from_slice(&bytes[..len]);
            m.name_len = len as u8;
        } else if let Some(val) = line.strip_prefix("provides=") {
            let val = val.trim();
            let bytes = val.as_bytes();
            let len = core::cmp::min(bytes.len(), MAX_INTERFACE_NAME);
            m.interface[..len].copy_from_slice(&bytes[..len]);
            m.interface_len = len as u8;
        } else if let Some(val) = line.strip_prefix("roles=") {
            let val = val.trim();
            let bytes = val.as_bytes();
            let len = core::cmp::min(bytes.len(), MAX_ROLES_LEN);
            m.roles[..len].copy_from_slice(&bytes[..len]);
            m.roles_len = len as u8;
        }
    }
    m
}

pub fn load_driver_with_manifest(manifest: &DriverManifest, prog: &LoadedProgram) -> Result<usize, &'static str> {
    let name_len = core::cmp::min(manifest.name_len as usize, MAX_DRIVER_NAME);
    let name_str = core::str::from_utf8(&manifest.name[..name_len]).unwrap_or("");

    if let Some(existing_idx) = find_driver_by_name(name_str) {
        unsafe {
            DRIVER_SLOTS[existing_idx].ref_count += 1;
        }
        return Ok(existing_idx);
    }

    let slot_idx = alloc_slot().ok_or("ERR_NO_DRIVER_SLOTS")?;

    unsafe {
        let slot = &mut DRIVER_SLOTS[slot_idx];
        slot.state = SlotState::Loaded;
        slot.base = prog.base;
        slot.entry = prog.entry;
        slot.stack_size = prog.stack_size;
        slot.code_size = prog.code_size;
        slot.data_size = prog.data_size;
        slot.ref_count = 1;
        slot.manifest = *manifest;
    }

    Ok(slot_idx)
}

pub fn load_driver(name: &str, interface: &str, prog: &LoadedProgram, permissions: u32) -> Result<usize, &'static str> {
    let mut manifest = EMPTY_MANIFEST;
    let name_bytes = name.as_bytes();
    let len = core::cmp::min(name_bytes.len(), MAX_DRIVER_NAME);
    manifest.name[..len].copy_from_slice(&name_bytes[..len]);
    manifest.name_len = len as u8;

    let if_bytes = interface.as_bytes();
    let if_len = core::cmp::min(if_bytes.len(), MAX_INTERFACE_NAME);
    manifest.interface[..if_len].copy_from_slice(&if_bytes[..if_len]);
    manifest.interface_len = if_len as u8;

    // Default roles based on standard interface definitions
    let default_roles: &[u8] = match interface {
        "DistanceSensor" => b"trigger,echo",
        "Motor" => b"pwm,dir1,dir2",
        "Servo" => b"signal",
        "Display" => b"sda,scl",
        _ => b"primary,secondary",
    };
    let r_len = core::cmp::min(default_roles.len(), MAX_ROLES_LEN);
    manifest.roles[..r_len].copy_from_slice(&default_roles[..r_len]);
    manifest.roles_len = r_len as u8;
    manifest.permissions = permissions;

    load_driver_with_manifest(&manifest, prog)
}

pub fn release_driver(slot_idx: usize) -> Result<(), &'static str> {
    if slot_idx >= MAX_SLOTS {
        return Err("ERR_BAD_SLOT");
    }
    unsafe {
        let slot = &mut DRIVER_SLOTS[slot_idx];
        if slot.state == SlotState::Free {
            return Err("ERR_NOT_LOADED");
        }
        if slot.ref_count > 1 {
            slot.ref_count -= 1;
            return Ok(());
        }

        crate::loader::unload(&LoadedProgram {
            base: slot.base,
            code_size: slot.code_size,
            data_size: slot.data_size,
            bss_size: 0,
            entry: slot.entry,
            stack_size: slot.stack_size,
        });

        *slot = EMPTY_SLOT;
    }
    Ok(())
}

pub fn format_drivers(out: &mut [u8]) -> usize {
    let mut written = 0;
    unsafe {
        for (i, slot) in DRIVER_SLOTS.iter().enumerate() {
            if slot.state != SlotState::Free {
                let name_len = core::cmp::min(slot.manifest.name_len as usize, MAX_DRIVER_NAME);
                let if_len = core::cmp::min(slot.manifest.interface_len as usize, MAX_INTERFACE_NAME);
                let name = core::str::from_utf8(&slot.manifest.name[..name_len]).unwrap_or("?");
                let interface = core::str::from_utf8(&slot.manifest.interface[..if_len]).unwrap_or("?");
                
                for &b in b"SLOT=" { if written < out.len() { out[written] = b; written += 1; } }
                if written < out.len() { out[written] = b'0' + (i as u8); written += 1; }
                for &b in b" NAME=" { if written < out.len() { out[written] = b; written += 1; } }
                for &b in name.as_bytes() { if written < out.len() { out[written] = b; written += 1; } }
                for &b in b" IF=" { if written < out.len() { out[written] = b; written += 1; } }
                for &b in interface.as_bytes() { if written < out.len() { out[written] = b; written += 1; } }
                if written < out.len() { out[written] = b'\n'; written += 1; }
            }
        }
    }
    written
}
