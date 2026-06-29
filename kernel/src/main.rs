#[no_std]
#[deny(unsafe_op_in_unsafe_fn)]
#[deny(clippy::all)]

//! Espresso OS Kernel
//! 
//! The core operating system kernel for Espresso, a DOS-style single-user OS for ESP32.
//! 
//! This module contains the fundamental components:
//! - Context switch with Xtensa ASM
//! - Preemptive scheduler
//! - Executable loader
//! - System call dispatch
//! - Inter-process communication
//! - TTY abstraction
//! - Display driver (SSD1306)
//! - Keyboard driver (PS/2)
//! - Shell and Forth interpreter
//! - Memory management with exec pool allocator
//! 
//! Target: xtensa-esp32-none-elf
//! Architecture: No MMU, flat memory model

pub mod arch;
pub mod drivers;
pub mod loader;
pub mod mem;
pub mod scheduler;
pub mod shell;
pub mod syscall;
pub mod tty;
pub mod forth;
pub mod ipc;
pub mod display;
pub mod keyboard;

mod main;