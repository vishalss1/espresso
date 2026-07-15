//! Driver slot table — hot-load .espr binaries into 8 static slots.
//! Each loaded driver is registered here and can be queried/listed.

use crate::loader::LoadedProgram;

pub const MAX_SLOTS: usize = 8;

#[derive(Copy, Clone, PartialEq)]
pub enum SlotState {
    Free,
    Loaded,
}

#[derive(Copy, Clone)]
pub struct DriverSlot {
    pub state: SlotState,
    pub name: [u8; 32],
    pub name_len: u8,
    pub base: usize,
    pub entry: usize,
    pub stack_size: usize,
    pub code_size: usize,
    pub data_size: usize,
}

pub static mut DRIVER_SLOTS: [DriverSlot; MAX_SLOTS] = [
    DriverSlot { state: SlotState::Free, name: [0; 32], name_len: 0, base: 0, entry: 0, stack_size: 0, code_size: 0, data_size: 0 };
    MAX_SLOTS
];

pub fn init_slots() {
    unsafe {
        for slot in DRIVER_SLOTS.iter_mut() {
            slot.state = SlotState::Free;
            slot.name_len = 0;
            slot.base = 0;
            slot.entry = 0;
        }
    }
}

fn alloc_slot() -> Option<usize> {
    unsafe {
        for i in 0..MAX_SLOTS {
            if DRIVER_SLOTS[i].state == SlotState::Free {
                return Some(i);
            }
        }
    }
    None
}

pub fn load_driver(name: &str, prog: &LoadedProgram) -> Result<usize, &'static str> {
    let slot_idx = alloc_slot().ok_or("ERR_NO_SLOTS")?;

    let name_bytes = name.as_bytes();
    let copy_len = core::cmp::min(name_bytes.len(), 31);

    unsafe {
        let slot = &mut DRIVER_SLOTS[slot_idx];
        slot.state = SlotState::Loaded;
        slot.name_len = copy_len as u8;
        slot.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
        slot.base = prog.base;
        slot.entry = prog.entry;
        slot.stack_size = prog.stack_size;
        slot.code_size = prog.code_size;
        slot.data_size = prog.data_size;
    }

    Ok(slot_idx)
}

pub fn unload_driver(slot_idx: usize) -> Result<(), &'static str> {
    if slot_idx >= MAX_SLOTS {
        return Err("ERR_BAD_SLOT");
    }
    unsafe {
        let slot = &mut DRIVER_SLOTS[slot_idx];
        if slot.state == SlotState::Free {
            return Err("ERR_NOT_LOADED");
        }
        crate::loader::unload(&LoadedProgram {
            base: slot.base,
            code_size: slot.code_size,
            data_size: slot.data_size,
            bss_size: 0,
            entry: slot.entry,
            stack_size: slot.stack_size,
        });
        slot.state = SlotState::Free;
        slot.name_len = 0;
        slot.base = 0;
        slot.entry = 0;
    }
    Ok(())
}

pub fn list_drivers() {
    use crate::tty;

    let mut found = false;
    unsafe {
        for i in 0..MAX_SLOTS {
            let slot = &DRIVER_SLOTS[i];
            if slot.state == SlotState::Loaded {
                found = true;
                let mut line = [0u8; 80];
                let mut pos = 0;

                line[pos] = b'['; pos += 1;
                line[pos] = b'0' + i as u8; pos += 1;
                line[pos] = b']'; pos += 1;
                line[pos] = b' '; pos += 1;

                for j in 0..slot.name_len as usize {
                    line[pos] = slot.name[j]; pos += 1;
                }

                line[pos] = b' '; pos += 1;
                line[pos] = b'@'; pos += 1;
                line[pos] = b' '; pos += 1;

                let addr = slot.base as u32;
                const HEX: [u8; 16] = *b"0123456789ABCDEF";
                line[pos] = b'0'; pos += 1;
                line[pos] = b'x'; pos += 1;
                for shift in (0..32).step_by(4).rev() {
                    line[pos] = HEX[((addr >> shift) & 0xF) as usize]; pos += 1;
                }

                line[pos] = b'\n'; pos += 1;

                for k in 0..pos {
                    tty::write_both(line[k]);
                }
            }
        }
    }
    if !found {
        tty::write_str_both("(no drivers loaded)\n");
    }
}
