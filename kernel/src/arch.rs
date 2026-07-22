//! Kernel arch module — Xtensa-specific platform glue (CLAUDE.md spec)

pub const CPU_FREQ_HZ: u32 = 240_000_000;
pub const TIMER_FREQ_HZ: u32 = 80_000_000;

pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("rsil {0}, 15", out(reg) _);
    }
}

pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("wsr {0}, ps", in(reg) 0);
    }
}
