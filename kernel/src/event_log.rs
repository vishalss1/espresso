//! Event recorder - 4KB ring buffer, fixed-size events, overwrite-oldest
//!
//! Logs syscalls, errors, ERR_PERM denials, WDT feed gaps.

pub const EVENT_RING_SIZE: usize = 4096;
pub const EVENT_SIZE: usize = 32;
pub const MAX_EVENTS: usize = EVENT_RING_SIZE / EVENT_SIZE;

pub const EVT_SYSCALL: u8 = 0x01;
pub const EVT_ERROR: u8 = 0x02;
pub const EVT_PERM_DENIED: u8 = 0x03;
pub const EVT_WDT_GAP: u8 = 0x04;
pub const EVT_PANIC: u8 = 0x05;
pub const EVT_BOOT: u8 = 0x06;
pub const EVT_SHELL_CMD: u8 = 0x07;
pub const EVT_TASK_SPAWN: u8 = 0x08;
pub const EVT_TASK_KILL: u8 = 0x09;

pub struct EventRing {
    buf: [u8; EVENT_RING_SIZE],
    write_pos: usize,
    count: usize,
}

impl EventRing {
    pub const fn new() -> Self {
        Self { buf: [0; EVENT_RING_SIZE], write_pos: 0, count: 0 }
    }

    pub fn push(&mut self, event_type: u8, pid: u8, timestamp: u32, data: &[u8; 28]) {
        let pos = self.write_pos;
        let b = &mut self.buf;

        b[pos]     = (timestamp) as u8;
        b[pos + 1] = (timestamp >> 8) as u8;
        b[pos + 2] = (timestamp >> 16) as u8;
        b[pos + 3] = (timestamp >> 24) as u8;
        b[pos + 4] = event_type;
        b[pos + 5] = pid;
        for i in 0..28 {
            b[pos + 6 + i] = data[i];
        }

        self.write_pos = (pos + EVENT_SIZE) % EVENT_RING_SIZE;
        if self.count < MAX_EVENTS {
            self.count += 1;
        }
    }

    /// Read event at logical index (0 = oldest) without consuming.
    fn read_event(&self, idx: usize) -> Option<(u32, u8, u8, [u8; 28])> {
        if idx >= self.count {
            return None;
        }
        let oldest = if self.count < MAX_EVENTS { 0 } else { self.write_pos };
        let abs_idx = (oldest + idx) % MAX_EVENTS;
        let pos = abs_idx * EVENT_SIZE;
        let b = &self.buf;

        let timestamp = b[pos] as u32
            | (b[pos + 1] as u32) << 8
            | (b[pos + 2] as u32) << 16
            | (b[pos + 3] as u32) << 24;
        let event_type = b[pos + 4];
        let pid = b[pos + 5];
        let mut data = [0u8; 28];
        for i in 0..28 {
            data[i] = b[pos + 6 + i];
        }
        Some((timestamp, event_type, pid, data))
    }

    pub fn event_count(&self) -> usize {
        self.count
    }
}

pub static mut EVENT_RING: EventRing = EventRing::new();

fn event_name(et: u8) -> &'static str {
    match et {
        EVT_SYSCALL => "SCALL",
        EVT_ERROR => "ERROR",
        EVT_PERM_DENIED => "DENIED",
        EVT_WDT_GAP => "WDT_GAP",
        EVT_PANIC => "PANIC",
        EVT_BOOT => "BOOT",
        EVT_SHELL_CMD => "SHELL",
        EVT_TASK_SPAWN => "SPAWN",
        EVT_TASK_KILL => "KILL",
        _ => "???",
    }
}

fn write_str(buf: &mut [u8], pos: &mut usize, s: &[u8]) {
    for &b in s {
        if *pos < buf.len() { buf[*pos] = b; *pos += 1; }
    }
}

fn write_num(buf: &mut [u8], pos: &mut usize, val: u32) {
    if val == 0 {
        if *pos < buf.len() { buf[*pos] = b'0'; *pos += 1; }
        return;
    }
    let mut digits = [0u8; 10];
    let mut d = 0;
    let mut v = val;
    while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
    while d > 0 { d -= 1; if *pos < buf.len() { buf[*pos] = digits[d]; *pos += 1; } }
}

pub fn log_event(event_type: u8, pid: u8, data: &[u8; 28]) {
    unsafe {
        let tick = crate::scheduler::TICK_COUNT;
        EVENT_RING.push(event_type, pid, tick, data);
    }
}

pub fn log_syscall(syscall_num: u8, pid: u8) {
    let mut data = [0u8; 28];
    data[0] = syscall_num;
    log_event(EVT_SYSCALL, pid, &data);
}

pub fn log_error(error_code: u8, pid: u8) {
    let mut data = [0u8; 28];
    data[0] = error_code;
    log_event(EVT_ERROR, pid, &data);
}

pub fn log_perm_denied(syscall_num: u8, pid: u8) {
    let mut data = [0u8; 28];
    data[0] = syscall_num;
    log_event(EVT_PERM_DENIED, pid, &data);
}

pub fn log_wdt_gap() {
    log_event(EVT_WDT_GAP, 0, &[0u8; 28]);
}

pub fn log_panic(pid: u8) {
    log_event(EVT_PANIC, pid, &[0u8; 28]);
}

pub fn log_boot() {
    log_event(EVT_BOOT, 0, &[0u8; 28]);
}

pub fn log_task_spawn(pid: u8) {
    log_event(EVT_TASK_SPAWN, pid, &[0u8; 28]);
}

pub fn log_task_kill(pid: u8) {
    log_event(EVT_TASK_KILL, pid, &[0u8; 28]);
}

/// Format all events as human-readable text into `out`. Returns bytes written.
pub fn format_log(out: &mut [u8]) -> usize {
    unsafe {
        let n = EVENT_RING.event_count();
        let mut pos = 0;

        for idx in 0..n {
            if let Some((ts, et, pid, _data)) = EVENT_RING.read_event(idx) {
                write_str(out, &mut pos, b"[");
                write_num(out, &mut pos, ts);
                write_str(out, &mut pos, b"] ");
                write_str(out, &mut pos, event_name(et).as_bytes());
                write_str(out, &mut pos, b" pid=");
                write_num(out, &mut pos, pid as u32);
                write_str(out, &mut pos, b"\n");
            }
        }
        pos
    }
}

pub fn drain_log(out: &mut [u8]) -> usize {
    format_log(out)
}

pub fn event_count() -> usize {
    unsafe { EVENT_RING.event_count() }
}
