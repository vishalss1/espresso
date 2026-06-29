#![no_std]
//! Scheduler module - preemptive round-robin task scheduler
//! 
//! Manages 4 static task slots, handles timer interrupts, implements round-robin scheduling.
//! Coordinates with context switch, IPC, and blocking primitives.

pub mod schedule;

pub const MAX_TASKS: usize = 4;
pub const TICKS_PER_SECOND: u32 = 100;
pub const TIMER_INTERVAL_US: u32 = 10_000;