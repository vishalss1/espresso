//! Espresso Host Protocol Service — active kernel service task (CLAUDE.md spec)
//!
//! Listens on UART0 (the only fixed peripheral path) for Host Protocol request frames.
//! Executes host commands (ps, devices, logs, cat, deploy, remove, pal, ping) and returns structured frames.

use crate::drivers::uart::RawUart;

// Binary escape prefixes (0x1B) to prevent ASCII text matching
pub const MAGIC_REQ_0: u8  = 0x1B;
pub const MAGIC_REQ_1: u8  = b'S';

pub const MAGIC_RESP_0: u8 = 0x1B;
pub const MAGIC_RESP_1: u8 = b'R';

pub const CMD_PS: u8      = 0x01;
pub const CMD_DEVICES: u8 = 0x02;
pub const CMD_LOGS: u8    = 0x03;
pub const CMD_CAT: u8     = 0x04;
pub const CMD_DEPLOY: u8  = 0x05;
pub const CMD_REMOVE: u8  = 0x06;
pub const CMD_PAL: u8     = 0x07;
pub const CMD_PING: u8    = 0x08;

pub const STATUS_OK: u8  = 0x00;
pub const STATUS_ERR: u8 = 0x01;

fn send_response(cmd: u8, seq: u8, status: u8, payload: &[u8]) {
    let uart = RawUart;
    let len = payload.len() as u16;
    let header = [
        MAGIC_RESP_0,
        MAGIC_RESP_1,
        cmd,
        seq,
        status,
        (len >> 8) as u8,
        (len & 0xFF) as u8,
    ];
    uart.write_bytes(&header);
    if !payload.is_empty() {
        uart.write_bytes(payload);
    }
}

fn read_byte_timeout(max_spins: u32) -> Option<u8> {
    let uart = RawUart;
    let mut spins = 0;
    while spins < max_spins {
        if let Some(b) = uart.read_byte() {
            return Some(b);
        }
        spins += 1;
        unsafe { core::arch::asm!("nop"); }
    }
    None
}

fn handle_command(cmd: u8, seq: u8, payload: &[u8]) {
    let mut resp_buf = [0u8; 1024];

    match cmd {
        CMD_PING => {
            send_response(cmd, seq, STATUS_OK, b"PONG\n");
        }
        CMD_PS => {
            if let Ok(n) = crate::scheduler::schedule::get_proc_tasks(&mut resp_buf) {
                send_response(cmd, seq, STATUS_OK, &resp_buf[..n]);
            } else {
                send_response(cmd, seq, STATUS_ERR, b"ERR_PS_FAILED\n");
            }
        }
        CMD_DEVICES => {
            let n = crate::device_registry::format_proc_devices(&mut resp_buf);
            send_response(cmd, seq, STATUS_OK, &resp_buf[..n]);
        }
        CMD_PAL => {
            let n = crate::pal::format_proc_pal(&mut resp_buf);
            send_response(cmd, seq, STATUS_OK, &resp_buf[..n]);
        }
        CMD_LOGS => {
            let n = crate::event_log::drain_log(&mut resp_buf);
            send_response(cmd, seq, STATUS_OK, &resp_buf[..n]);
        }
        CMD_CAT => {
            if let Ok(path) = core::str::from_utf8(payload) {
                match crate::vfs::vfs_open(path, 0) {
                    Ok(fd) => {
                        if let Ok(n) = crate::vfs::vfs_read(fd, &mut resp_buf) {
                            let _ = crate::vfs::vfs_close(fd);
                            send_response(cmd, seq, STATUS_OK, &resp_buf[..n]);
                        } else {
                            let _ = crate::vfs::vfs_close(fd);
                            send_response(cmd, seq, STATUS_ERR, b"ERR_READ_FAILED\n");
                        }
                    }
                    Err(_) => send_response(cmd, seq, STATUS_ERR, b"ERR_NOT_FOUND\n"),
                }
            } else {
                send_response(cmd, seq, STATUS_ERR, b"ERR_INVALID_PATH\n");
            }
        }
        CMD_DEPLOY => {
            if let Ok(pkg_name) = core::str::from_utf8(payload) {
                match crate::deploy::deploy_app(pkg_name) {
                    Ok(_) => send_response(cmd, seq, STATUS_OK, b"DEPLOY_OK\n"),
                    Err(_) => send_response(cmd, seq, STATUS_ERR, b"ERR_DEPLOY_FAILED\n"),
                }
            } else {
                send_response(cmd, seq, STATUS_ERR, b"ERR_BAD_NAME\n");
            }
        }
        CMD_REMOVE => {
            if let Ok(pkg_name) = core::str::from_utf8(payload) {
                match crate::deploy::remove_app(pkg_name) {
                    Ok(_) => send_response(cmd, seq, STATUS_OK, b"REMOVE_OK\n"),
                    Err(_) => send_response(cmd, seq, STATUS_ERR, b"ERR_REMOVE_FAILED\n"),
                }
            } else {
                send_response(cmd, seq, STATUS_ERR, b"ERR_BAD_NAME\n");
            }
        }
        _ => send_response(cmd, seq, STATUS_ERR, b"ERR_UNKNOWN_CMD\n"),
    }
}

pub fn process_uart_frame() {
    let uart = RawUart;
    if let Some(b0) = uart.read_byte() {
        if b0 == MAGIC_REQ_0 {
            if let Some(b1) = read_byte_timeout(50000) {
                if b1 == MAGIC_REQ_1 {
                    let cmd = match read_byte_timeout(50000) { Some(c) => c, None => return };
                    let seq = match read_byte_timeout(50000) { Some(s) => s, None => return };
                    let len_hi = match read_byte_timeout(50000) { Some(h) => h as usize, None => return };
                    let len_lo = match read_byte_timeout(50000) { Some(l) => l as usize, None => return };
                    let len = (len_hi << 8) | len_lo;

                    let mut payload_buf = [0u8; 128];
                    let payload_len = core::cmp::min(len, payload_buf.len());
                    for i in 0..payload_len {
                        match read_byte_timeout(50000) {
                            Some(p) => payload_buf[i] = p,
                            None => return,
                        }
                    }

                    handle_command(cmd, seq, &payload_buf[..payload_len]);
                }
            }
        }
    }
}

pub extern "C" fn host_protocol_task() -> ! {
    loop {
        process_uart_frame();
        unsafe { core::arch::asm!("nop"); }
    }
}
