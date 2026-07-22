//! Virtual filesystem (VFS) — single namespace over all peripherals & system endpoints (CLAUDE.md spec)

pub const MAX_VFS_ENTRIES: usize = 16;
pub const MAX_PATH_LEN: usize = 64;

pub struct VfsEntry {
    pub path: [u8; MAX_PATH_LEN],
    pub path_len: u8,
    pub handler: VfsHandler,
}

#[derive(Clone, Copy)]
pub enum VfsHandler {
    InternalFs,
    ProcTasks,
    ProcMem,
    ProcLog,
    ProcCaps,
    ProcCrash,
    ProcPal,
    ProcDevices,
    DevDevice(u8), // Index in Device Registry
}

const EMPTY_VFS: VfsEntry = VfsEntry {
    path: [0; MAX_PATH_LEN],
    path_len: 0,
    handler: VfsHandler::InternalFs,
};

pub static mut VFS_TABLE: [VfsEntry; MAX_VFS_ENTRIES] = [EMPTY_VFS; MAX_VFS_ENTRIES];

pub fn register_entry(path: &str, handler: VfsHandler) -> Result<(), &'static str> {
    unsafe {
        let slot = VFS_TABLE.iter_mut().find(|e| e.path_len == 0).ok_or("ERR_VFS_FULL")?;
        let bytes = path.as_bytes();
        let len = core::cmp::min(bytes.len(), MAX_PATH_LEN - 1);
        slot.path[..len].copy_from_slice(&bytes[..len]);
        slot.path[len] = 0;
        slot.path_len = len as u8;
        slot.handler = handler;
        Ok(())
    }
}

pub fn resolve(path: &str) -> Option<VfsHandler> {
    unsafe {
        for entry in VFS_TABLE.iter() {
            if entry.path_len == 0 { continue; }
            let entry_path = core::str::from_utf8(&entry.path[..entry.path_len as usize]).unwrap_or("");
            if path == entry_path || path.starts_with(entry_path) {
                return Some(entry.handler);
            }
        }
        None
    }
}

pub fn init_vfs() {
    unsafe {
        for entry in VFS_TABLE.iter_mut() {
            *entry = EMPTY_VFS;
        }
    }
    // Internal Flash Filesystem Mount Points
    let _ = register_entry("/cfg", VfsHandler::InternalFs);
    let _ = register_entry("/drv", VfsHandler::InternalFs);
    let _ = register_entry("/app", VfsHandler::InternalFs);
    let _ = register_entry("/data", VfsHandler::InternalFs);
    let _ = register_entry("/tmp", VfsHandler::InternalFs);

    // System Proc Filesystem Endpoints
    let _ = register_entry("/proc/tasks", VfsHandler::ProcTasks);
    let _ = register_entry("/proc/mem", VfsHandler::ProcMem);
    let _ = register_entry("/proc/log", VfsHandler::ProcLog);
    let _ = register_entry("/proc/caps", VfsHandler::ProcCaps);
    let _ = register_entry("/proc/crash", VfsHandler::ProcCrash);
    let _ = register_entry("/proc/pal", VfsHandler::ProcPal);
    let _ = register_entry("/proc/devices", VfsHandler::ProcDevices);
}

pub fn vfs_open(path: &str, _flags: u32) -> Result<i32, &'static str> {
    match resolve(path) {
        Some(handler) => match handler {
            VfsHandler::InternalFs => Ok(0),
            VfsHandler::ProcTasks => Ok(100),
            VfsHandler::ProcMem => Ok(101),
            VfsHandler::ProcLog => Ok(102),
            VfsHandler::ProcCaps => Ok(103),
            VfsHandler::ProcCrash => Ok(104),
            VfsHandler::ProcPal => Ok(105),
            VfsHandler::ProcDevices => Ok(106),
            VfsHandler::DevDevice(idx) => Ok(200 + idx as i32),
        },
        None => Err("ERR_NOT_FOUND"),
    }
}

pub fn vfs_read(fd: i32, buf: &mut [u8]) -> Result<usize, &'static str> {
    match fd {
        100 => crate::scheduler::schedule::get_proc_tasks(buf),
        101 => crate::scheduler::schedule::get_proc_mem(buf),
        102 => Ok(crate::event_log::drain_log(buf)),
        103 => {
            let pid = unsafe { crate::scheduler::CURRENT_TASK };
            Ok(crate::caps::query_caps(pid, buf))
        }
        104 => Ok(crate::panic_policy::read_crash_log(buf)),
        105 => Ok(crate::pal::format_proc_pal(buf)),
        106 => Ok(crate::device_registry::format_proc_devices(buf)),
        0 => Ok(0),
        _ => Err("ERR_BAD_FD"),
    }
}

pub fn vfs_write(fd: i32, buf: &[u8]) -> Result<usize, &'static str> {
    match fd {
        0 => {
            crate::drivers::uart::RawUart.write_bytes(buf);
            Ok(buf.len())
        }
        _ => Err("ERR_BAD_FD"),
    }
}

pub fn vfs_close(_fd: i32) -> Result<(), &'static str> { Ok(()) }

pub fn vfs_dir_list(path: &str, _buf: &mut [u8]) -> Result<usize, &'static str> {
    if path == "/" || path == "/cfg" || path == "/drv" || path == "/app" || path == "/data" || path == "/tmp" {
        Ok(0)
    } else {
        Err("ERR_FS_DRIVER_NOT_LOADED")
    }
}

pub fn vfs_file_delete(_path: &str) -> Result<(), &'static str> {
    Err("ERR_NOT_SUPPORTED")
}
