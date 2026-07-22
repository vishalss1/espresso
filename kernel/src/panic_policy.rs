//! Panic policy + crash-loop backoff + brownout handling (CLAUDE.md spec)

pub const CRASH_LOG_SIZE: usize = 4096;
pub const CRASH_RECORD_MAX: usize = 128;
pub const CRASH_RECORD_SIZE: usize = 136;
pub const CRASH_LOOP_THRESHOLD: u32 = 3;
pub const CRASH_LOOP_WINDOW_SECS: u32 = 30;

// RTC slow memory (survives software reset)
const RTC_SLOW_CRASH_COUNT: *mut u32 = 0x50001FF0 as *mut u32;
const RTC_SLOW_LAST_TIME:   *mut u32 = 0x50001FF4 as *mut u32;

// RTC_CNTL reset state register
const RTC_CNTL_RESET_STATE_REG: *const u32 = 0x3FF48034 as *const u32;

#[repr(C, packed)]
pub struct CrashRecord {
    pub timestamp: u32,
    pub pc: u32,
    pub pid: u8,
    pub msg_len: u8,
    pub msg: [u8; CRASH_RECORD_MAX],
}

pub static mut CRASH_LOG: [u8; CRASH_LOG_SIZE] = [0; CRASH_LOG_SIZE];
pub static mut CRASH_LOG_WRITE_POS: usize = 0;

pub unsafe fn get_reset_reason() -> u32 {
    let val = core::ptr::read_volatile(RTC_CNTL_RESET_STATE_REG);
    val & 0x1F
}

pub fn init_crash_log() {
    unsafe {
        CRASH_LOG_WRITE_POS = 0;
    }
}

pub fn write_crash_record(msg: &str, pc: u32, pid: u8) {
    unsafe {
        let truncated_len = core::cmp::min(msg.len(), CRASH_RECORD_MAX);
        if CRASH_LOG_WRITE_POS + CRASH_RECORD_SIZE > CRASH_LOG_SIZE {
            CRASH_LOG_WRITE_POS = 0;
        }
        let pos = CRASH_LOG_WRITE_POS;
        let base = CRASH_LOG.as_mut_ptr().add(pos);
        let timestamp = crate::scheduler::TICK_COUNT;
        core::ptr::write_volatile(base as *mut u32, timestamp);
        core::ptr::write_volatile(base.add(4) as *mut u32, pc);
        core::ptr::write_volatile(base.add(8), pid);
        core::ptr::write_volatile(base.add(9), truncated_len as u8);
        for i in 0..truncated_len {
            core::ptr::write_volatile(base.add(10 + i), msg.as_bytes()[i]);
        }
        CRASH_LOG_WRITE_POS += CRASH_RECORD_SIZE;
    }
}

pub fn read_crash_log(out: &mut [u8]) -> usize {
    unsafe {
        let mut written = 0;
        let mut pos = 0;
        while pos + CRASH_RECORD_SIZE <= CRASH_LOG_SIZE && written < out.len() {
            let base = CRASH_LOG.as_ptr().add(pos);
            let timestamp = core::ptr::read_volatile(base as *const u32);
            let pc = core::ptr::read_volatile(base.add(4) as *const u32);
            let pid = core::ptr::read_volatile(base.add(8));
            let _msg_len = core::ptr::read_volatile(base.add(9)) as usize;
            if timestamp == 0 && pc == 0 { break; }

            let header = b"[ts=";
            for &b in header { if written < out.len() { out[written] = b; written += 1; } }

            let mut digits = [0u8; 10];
            let mut v = timestamp;
            let mut d = 0;
            while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
            let mut i = d;
            while i > 0 { i -= 1; if written < out.len() { out[written] = digits[i]; written += 1; } }

            let sep = b" pc=";
            for &b in sep { if written < out.len() { out[written] = b; written += 1; } }

            v = pc; d = 0;
            while v > 0 { digits[d] = b"0123456789ABCDEF"[(v & 0xF) as usize]; v >>= 4; d += 1; }
            if d == 0 { digits[0] = b'0'; d = 1; }
            i = d;
            while i > 0 { i -= 1; if written < out.len() { out[written] = digits[i]; written += 1; } }

            let sep_pid = b" pid=";
            for &b in sep_pid { if written < out.len() { out[written] = b; written += 1; } }
            if written < out.len() { out[written] = b'0' + pid; written += 1; }

            if written < out.len() { out[written] = b'\n'; written += 1; }
            pos += CRASH_RECORD_SIZE;
        }
        written
    }
}

pub fn check_crash_loop() -> bool {
    unsafe {
        let reason = get_reset_reason();
        let count = core::ptr::read_volatile(RTC_SLOW_CRASH_COUNT);

        // Reset reason 1 = POWERON_RESET. On cold boot or uninitialized RAM (>10), reset crash counter.
        if reason == 1 || count > 10 {
            core::ptr::write_volatile(RTC_SLOW_CRASH_COUNT, 0);
            return false;
        }

        count >= CRASH_LOOP_THRESHOLD
    }
}

pub fn record_crash() {
    unsafe {
        let now = crate::scheduler::TICK_COUNT / 100;
        let count = core::ptr::read_volatile(RTC_SLOW_CRASH_COUNT);
        let last = core::ptr::read_volatile(RTC_SLOW_LAST_TIME);

        if count == 0 || (now.saturating_sub(last)) < CRASH_LOOP_WINDOW_SECS {
            core::ptr::write_volatile(RTC_SLOW_CRASH_COUNT, count + 1);
        } else {
            core::ptr::write_volatile(RTC_SLOW_CRASH_COUNT, 1);
        }
        core::ptr::write_volatile(RTC_SLOW_LAST_TIME, now);
    }
}

pub fn clear_crash_loop() {
    unsafe {
        core::ptr::write_volatile(RTC_SLOW_CRASH_COUNT, 0);
    }
}

pub fn reset_system() -> ! {
    unsafe {
        const RTC_CNTL_OPTIONS0: *mut u32 = 0x3FF48000 as *mut u32;
        core::ptr::write_volatile(RTC_CNTL_OPTIONS0, 1 << 31);
    }
    loop {
        unsafe { core::arch::asm!("nop"); }
    }
}
