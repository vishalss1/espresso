//! Capability model - per-task u32 bitmask

pub const CAP_GPIO: u32 = 1 << 0;
pub const CAP_SPI: u32 = 1 << 1;
pub const CAP_I2C: u32 = 1 << 2;
pub const CAP_FS_SD: u32 = 1 << 3;
pub const CAP_FS_PROC: u32 = 1 << 4;
pub const CAP_IPC: u32 = 1 << 5;
pub const CAP_TASK_SPAWN: u32 = 1 << 6;
pub const CAP_DRIVER_LOAD: u32 = 1 << 7;
pub const CAP_TTY: u32 = 1 << 8;
pub const CAP_SNAPSHOT: u32 = 1 << 9;
pub const CAP_EVENT_LOG: u32 = 1 << 10;
pub const CAP_ALL: u32 = 0xFFFFFFFF;
pub const CAP_NONE: u32 = 0;

pub const MAX_TASKS: usize = 4;
pub static mut CAP_TABLE: [u32; MAX_TASKS] = [CAP_NONE; MAX_TASKS];

pub fn init_caps() {
    unsafe {
        CAP_TABLE[0] = CAP_ALL;
        CAP_TABLE[1] = CAP_GPIO | CAP_FS_SD | CAP_FS_PROC | CAP_IPC | CAP_TASK_SPAWN | CAP_TTY | CAP_EVENT_LOG;
        CAP_TABLE[2] = CAP_GPIO | CAP_FS_SD | CAP_IPC;
        CAP_TABLE[3] = CAP_GPIO | CAP_FS_SD | CAP_IPC;
    }
}

pub fn check_cap(pid: usize, cap: u32) -> bool {
    unsafe { if pid < MAX_TASKS { CAP_TABLE[pid] & cap == cap } else { false } }
}

pub fn query_caps(pid: usize, buf: &mut [u8]) -> usize {
    unsafe {
        if pid < MAX_TASKS && buf.len() >= 4 {
            let caps = CAP_TABLE[pid];
            buf[0..4].copy_from_slice(&caps.to_le_bytes());
            4
        } else {
            0
        }
    }
}

pub fn get_caps(pid: usize) -> u32 {
    unsafe { if pid < MAX_TASKS { CAP_TABLE[pid] } else { 0 } }
}
