#![no_std]
//! Memory management module - exec pool allocator
//! 
//! Implements first-fit contiguous allocation over 328KB exec pool.
//! Provides fixed-size pages for program loading and execution.

pub struct Page {
    start: usize,
    size: usize,
}

pub struct ExecPool {
    // Bitmapped free/used tracking
    bitmap: [u8; 12], // 96 bits for 82 pages
    free_count: usize,
}