#![no_std]
//! System call dispatch module - handles all syscalls from user programs
//! 
//! Implemented as jump table through Xtensa SYSCALL instruction.
//! Each syscall saves state, calls handler, restores execution.

use crate::syscall::SyscallTable;

pub const SYSCALL_TABLE: SyscallTable = SyscallTable::new();