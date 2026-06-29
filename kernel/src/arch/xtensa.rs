#![no_std]
//! Kernel arch module - Xtensa-specific platform glue
//! 
//! Provides architecture-specific low-level operations for the Espresso OS kernel.
//! Handles interrupt management, context switch interface, and hardware features.

pub mod xtensa;