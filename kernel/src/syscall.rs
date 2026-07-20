//! System call dispatch module
//!
//! Xtensa SYSCALL instruction: num in A2, args A3-A6, ret A2.

pub const SYS_EXIT: u32 = 0x00;
pub const SYS_YIELD: u32 = 0x01;
pub const SYS_SLEEP_MS: u32 = 0x02;
pub const SYS_GPIO_READ: u32 = 0x03;
pub const SYS_GPIO_WRITE: u32 = 0x04;
pub const SYS_GPIO_MODE: u32 = 0x05;
pub const SYS_UART_READ: u32 = 0x06;
pub const SYS_UART_WRITE: u32 = 0x07;
pub const SYS_I2C_READ: u32 = 0x08;
pub const SYS_I2C_WRITE: u32 = 0x09;
pub const SYS_SPI_TRANSFER: u32 = 0x0A;
pub const SYS_ADC_READ: u32 = 0x0B;
pub const SYS_FILE_OPEN: u32 = 0x0C;
pub const SYS_FILE_READ: u32 = 0x0D;
pub const SYS_FILE_WRITE: u32 = 0x0E;
pub const SYS_FILE_CLOSE: u32 = 0x0F;
pub const SYS_FILE_SEEK: u32 = 0x10;
pub const SYS_DIR_LIST: u32 = 0x11;
pub const SYS_FILE_DELETE: u32 = 0x12;
pub const SYS_MSG_OPEN: u32 = 0x13;
pub const SYS_MSG_SEND: u32 = 0x14;
pub const SYS_MSG_RECV: u32 = 0x15;
pub const SYS_MSG_CLOSE: u32 = 0x16;
pub const SYS_TASK_SPAWN: u32 = 0x17;
pub const SYS_TASK_KILL: u32 = 0x18;
pub const SYS_MEM_INFO: u32 = 0x19;
pub const SYS_TTY_WRITE: u32 = 0x1A;
pub const SYS_TTY_READ: u32 = 0x1B;
pub const SYS_PKG_INSTALL: u32 = 0x1D;
pub const SYS_SNAPSHOT_SAVE: u32 = 0x1E;
pub const SYS_SNAPSHOT_RESTORE: u32 = 0x1F;
pub const SYS_EVENT_LOG_READ: u32 = 0x20;
pub const SYS_DRIVER_LOAD: u32 = 0x21;
pub const SYS_CAP_QUERY: u32 = 0x22;
pub const SYS_WDT_FEED: u32 = 0x23;
pub const SYS_CRASH_LOG_READ: u32 = 0x24;

pub const ERR_OK: i32 = 0;
pub const ERR_PERM: i32 = -1;
pub const ERR_NOT_FOUND: i32 = -2;
pub const ERR_NO_SD: i32 = -3;
pub const ERR_IO: i32 = -4;
pub const ERR_NO_MEMORY: i32 = -5;
pub const ERR_BAD_FD: i32 = -6;

pub struct SyscallContext {
    pub num: u32,
    pub arg1: u32,
    pub arg2: u32,
    pub arg3: u32,
    pub arg4: u32,
    pub pid: usize,
}

// ── Sub-dispatch functions (small to avoid LLVM register spill) ──────────

#[inline(never)]
fn dispatch_gpio(ctx: &SyscallContext) -> i32 {
    if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_GPIO) {
        crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
        return ERR_PERM;
    }
    match ctx.num {
        SYS_GPIO_READ => unsafe { crate::gpio::gpio_read(ctx.arg1 as u8) as i32 },
        SYS_GPIO_WRITE => unsafe { crate::gpio::gpio_write(ctx.arg1 as u8, ctx.arg2 as u8); ERR_OK },
        SYS_GPIO_MODE => unsafe { crate::gpio::gpio_mode(ctx.arg1 as u8, ctx.arg2 as u8); ERR_OK },
        SYS_ADC_READ => ERR_NOT_FOUND,
        _ => ERR_NOT_FOUND,
    }
}

#[inline(never)]
fn dispatch_file(ctx: &SyscallContext) -> i32 {
    if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_FS_SD) {
        crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
        return ERR_PERM;
    }
    match ctx.num {
        SYS_FILE_OPEN => {
            let path = ctx.arg1 as *const u8;
            let path_len = ctx.arg2 as usize;
            let flags = ctx.arg3;
            unsafe {
                let path_str = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(path, path_len)
                );
                match crate::vfs::vfs_open(path_str, flags) {
                    Ok(fd) => fd,
                    Err(_) => ERR_NOT_FOUND,
                }
            }
        }
        SYS_FILE_READ => {
            let fd = ctx.arg1 as i32;
            let buf = ctx.arg2 as *mut u8;
            let len = ctx.arg3 as usize;
            unsafe {
                let buf_slice = core::slice::from_raw_parts_mut(buf, len);
                match crate::vfs::vfs_read(fd, buf_slice) {
                    Ok(n) => n as i32,
                    Err(_) => ERR_IO,
                }
            }
        }
        SYS_FILE_WRITE => {
            let fd = ctx.arg1 as i32;
            let buf = ctx.arg2 as *const u8;
            let len = ctx.arg3 as usize;
            unsafe {
                let buf_slice = core::slice::from_raw_parts(buf, len);
                match crate::vfs::vfs_write(fd, buf_slice) {
                    Ok(n) => n as i32,
                    Err(_) => ERR_IO,
                }
            }
        }
        SYS_FILE_CLOSE => {
            let fd = ctx.arg1 as i32;
            match crate::vfs::vfs_close(fd) {
                Ok(_) => ERR_OK,
                Err(_) => ERR_IO,
            }
        }
        SYS_FILE_SEEK => ERR_NOT_FOUND,
        SYS_DIR_LIST => {
            let path = ctx.arg1 as *const u8;
            let path_len = ctx.arg2 as usize;
            let buf = ctx.arg3 as *mut u8;
            let buf_len = ctx.arg4 as usize;
            unsafe {
                let path_str = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(path, path_len)
                );
                let buf_slice = core::slice::from_raw_parts_mut(buf, buf_len);
                match crate::vfs::vfs_dir_list(path_str, buf_slice) {
                    Ok(n) => n as i32,
                    Err(_) => ERR_NOT_FOUND,
                }
            }
        }
        SYS_FILE_DELETE => {
            let path = ctx.arg1 as *const u8;
            let path_len = ctx.arg2 as usize;
            unsafe {
                let path_str = core::str::from_utf8_unchecked(
                    core::slice::from_raw_parts(path, path_len)
                );
                match crate::vfs::vfs_file_delete(path_str) {
                    Ok(_) => ERR_OK,
                    Err(_) => ERR_NOT_FOUND,
                }
            }
        }
        _ => ERR_NOT_FOUND,
    }
}

#[inline(never)]
fn dispatch_tty(ctx: &SyscallContext) -> i32 {
    if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_TTY) {
        crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
        return ERR_PERM;
    }
    match ctx.num {
        SYS_UART_READ | SYS_TTY_READ => {
            let buf = ctx.arg1 as *mut u8;
            let len = ctx.arg2 as usize;
            unsafe {
                let uart = crate::drivers::uart::RawUart;
                let mut count = 0;
                for i in 0..len {
                    if let Some(b) = uart.read_byte() {
                        core::ptr::write_volatile(buf.add(i), b);
                        count += 1;
                    } else {
                        break;
                    }
                }
                count as i32
            }
        }
        SYS_UART_WRITE | SYS_TTY_WRITE => {
            let buf = ctx.arg1 as *const u8;
            let len = ctx.arg2 as usize;
            unsafe {
                let uart = crate::drivers::uart::RawUart;
                for i in 0..len {
                    let b = core::ptr::read_volatile(buf.add(i));
                    uart.write_byte(b);
                }
            }
            len as i32
        }
        _ => ERR_NOT_FOUND,
    }
}

#[inline(never)]
fn dispatch_task(ctx: &SyscallContext) -> i32 {
    if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_TASK_SPAWN) {
        crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
        return ERR_PERM;
    }
    match ctx.num {
        SYS_TASK_SPAWN => {
            let entry = ctx.arg1 as usize;
            let stack_size = ctx.arg2 as usize;
            match crate::scheduler::spawn_task(entry, stack_size, crate::caps::CAP_ALL) {
                Ok(pid) => {
                    crate::event_log::log_task_spawn(pid as u8);
                    pid as i32
                }
                Err(_) => ERR_NO_MEMORY,
            }
        }
        SYS_TASK_KILL => {
            let pid = ctx.arg1 as usize;
            match crate::scheduler::kill_task(pid) {
                Ok(_) => {
                    crate::event_log::log_task_kill(pid as u8);
                    ERR_OK
                }
                Err(_) => ERR_NOT_FOUND,
            }
        }
        _ => ERR_NOT_FOUND,
    }
}

// ── Main dispatch (small — just routes to sub-dispatchers) ───────────────

#[inline(never)]
fn dispatch_inner(ctx: &SyscallContext) -> i32 {
    match ctx.num {
        SYS_EXIT => {
            crate::scheduler::kill_and_switch()
        }
        SYS_YIELD => {
            crate::scheduler::scheduler_tick();
            ERR_OK
        }
        SYS_SLEEP_MS => sleep_ms_handler(ctx.arg1),

        SYS_GPIO_READ | SYS_GPIO_WRITE | SYS_GPIO_MODE | SYS_ADC_READ => {
            dispatch_gpio(ctx)
        }
        SYS_I2C_READ | SYS_I2C_WRITE | SYS_SPI_TRANSFER => ERR_NOT_FOUND,

        SYS_FILE_OPEN | SYS_FILE_READ | SYS_FILE_WRITE | SYS_FILE_CLOSE
        | SYS_FILE_SEEK | SYS_DIR_LIST | SYS_FILE_DELETE => dispatch_file(ctx),

        SYS_UART_READ | SYS_UART_WRITE | SYS_TTY_READ | SYS_TTY_WRITE => {
            dispatch_tty(ctx)
        }

        SYS_TASK_SPAWN | SYS_TASK_KILL => dispatch_task(ctx),

        SYS_MEM_INFO => {
            let out = ctx.arg1 as *mut u8;
            unsafe {
                let buf = core::slice::from_raw_parts_mut(out, 16);
                crate::mem::pool::mem_info(buf);
            }
            ERR_OK
        }
        SYS_MSG_OPEN | SYS_MSG_SEND | SYS_MSG_RECV | SYS_MSG_CLOSE => {
            if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_IPC) {
                crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
                return ERR_PERM;
            }
            ERR_NOT_FOUND
        }
        SYS_PKG_INSTALL => ERR_NOT_FOUND,
        SYS_SNAPSHOT_SAVE | SYS_SNAPSHOT_RESTORE => {
            if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_SNAPSHOT) {
                crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
                return ERR_PERM;
            }
            ERR_NOT_FOUND
        }
        SYS_EVENT_LOG_READ => {
            if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_EVENT_LOG) {
                crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
                return ERR_PERM;
            }
            let buf = ctx.arg1 as *mut u8;
            let len = ctx.arg2 as usize;
            unsafe {
                let buf_slice = core::slice::from_raw_parts_mut(buf, len);
                crate::event_log::drain_log(buf_slice) as i32
            }
        }
        SYS_DRIVER_LOAD => {
            if !crate::caps::check_cap(ctx.pid, crate::caps::CAP_DRIVER_LOAD) {
                crate::event_log::log_perm_denied(ctx.num as u8, ctx.pid as u8);
                return ERR_PERM;
            }
            ERR_NOT_FOUND
        }
        SYS_CAP_QUERY => {
            let pid = ctx.arg1 as usize;
            crate::caps::get_caps(pid) as i32
        }
        SYS_WDT_FEED => {
            unsafe { crate::wdt_feed(); }
            ERR_OK
        }
        SYS_CRASH_LOG_READ => {
            let buf = ctx.arg1 as *mut u8;
            let len = ctx.arg2 as usize;
            unsafe {
                let buf_slice = core::slice::from_raw_parts_mut(buf, len);
                crate::panic_policy::read_crash_log(buf_slice) as i32
            }
        }
        _ => {
            crate::event_log::log_error(0xFF, ctx.pid as u8);
            ERR_NOT_FOUND
        }
    }
}

#[inline(never)]
pub fn dispatch(ctx: &SyscallContext) -> i32 {
    crate::event_log::log_syscall(ctx.num as u8, ctx.pid as u8);
    dispatch_inner(ctx)
}

#[inline(never)]
fn sleep_ms_handler(ms: u32) -> i32 {
    let mut start: u32;
    unsafe { core::arch::asm!("rsr {0}, ccount", out(reg) start); }
    let cycles = ms.wrapping_mul(240_000);
    loop {
        let mut now: u32;
        unsafe { core::arch::asm!("rsr {0}, ccount", out(reg) now); }
        let diff = now.wrapping_sub(start);
        if diff >= cycles { 
            break; 
        }
        crate::scheduler::scheduler_tick();
    }
    ERR_OK
}

/// extern "C" entry point called from the assembly exception handler in switch.S.
#[no_mangle]
#[inline(never)]
pub extern "C" fn syscall_dispatch_c(ctx: &mut SyscallContext) -> i32 {
    dispatch(ctx)
}
