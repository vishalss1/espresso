#![no_std]
//! Kernel arch module - Xtensa-specific platform glue
//!
//! Provides architecture-specific low-level operations for the Espresso OS kernel.
//! Handles interrupt management, context switch interface, and hardware features.

pub const CPU_FREQ_HZ: u32 = 240_000_000;
pub const TIMER_FREQ_HZ: u32 = 80_000_000;

pub fn disable_interrupts() {
    unsafe {
        core::arch::asm!("wsr %ps, {}", in(reg) 0xFu32 << 4);
    }
}

pub fn enable_interrupts() {
    unsafe {
        core::arch::asm!("rsr %ps, {}", out(reg) _);
    }
}
