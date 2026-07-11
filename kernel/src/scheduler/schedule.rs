//! Schedule module - core scheduler implementation
//!
//! Helper functions for task management.

use crate::scheduler::{TaskState};

pub fn get_proc_tasks(buf: &mut [u8]) -> Result<usize, &'static str> {
    let mut written = 0;
    unsafe {
        for task in crate::scheduler::TASKS.iter() {
            if task.state != TaskState::Dead {
                let state_char = match task.state {
                    TaskState::Dead => b'D',
                    TaskState::Ready => b'R',
                    TaskState::Running => b'S',
                    TaskState::Blocked => b'B',
                };

                let header = b"PID=";
                for &b in header {
                    if written < buf.len() { buf[written] = b; written += 1; }
                }

                let mut v = task.pid;
                if v == 0 {
                    if written < buf.len() { buf[written] = b'0'; written += 1; }
                } else {
                    let mut digits = [0u8; 10];
                    let mut d = 0;
                    while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
                    let mut i = d;
                    while i > 0 { i -= 1; if written < buf.len() { buf[written] = digits[i]; written += 1; } }
                }

                if written < buf.len() { buf[written] = b' '; written += 1; }
                if written < buf.len() { buf[written] = state_char; written += 1; }
                if written < buf.len() { buf[written] = b'\n'; written += 1; }
            }
        }
    }
    Ok(written)
}

pub fn get_proc_mem(buf: &mut [u8]) -> Result<usize, &'static str> {
    let mut written = 0;
    let total = crate::mem::pool::TOTAL_PAGES * crate::mem::pool::PAGE_SIZE;
    let free = crate::mem::pool::free_count() * crate::mem::pool::PAGE_SIZE;

    let label = b"total=";
    for &b in label { if written < buf.len() { buf[written] = b; written += 1; } }

    let mut v = total;
    if v == 0 {
        if written < buf.len() { buf[written] = b'0'; written += 1; }
    } else {
        let mut digits = [0u8; 10];
        let mut d = 0;
        while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
        let mut i = d;
        while i > 0 { i -= 1; if written < buf.len() { buf[written] = digits[i]; written += 1; } }
    }

    let sep = b" free=";
    for &b in sep { if written < buf.len() { buf[written] = b; written += 1; } }

    let mut v = free;
    if v == 0 {
        if written < buf.len() { buf[written] = b'0'; written += 1; }
    } else {
        let mut digits = [0u8; 10];
        let mut d = 0;
        while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
        let mut i = d;
        while i > 0 { i -= 1; if written < buf.len() { buf[written] = digits[i]; written += 1; } }
    }

    if written < buf.len() { buf[written] = b'\n'; written += 1; }
    Ok(written)
}
