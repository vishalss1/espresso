#![no_std]
//! Shell module - command-line interface and user interaction
//! 
//! Provides line editing, command history, and built-in command execution.
//! Interacts with TTY for input/output and IPC for task management.

pub const HISTORY_SIZE: usize = 20;
pub const PROMPT: &[u8] = b"espress# ";

pub fn start_shell() {
    todo!("Shell initialization")
}