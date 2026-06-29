#![no_std]
//! IPC module - named message queues between tasks
//! 
//! 8 static queues of 512B each. Blocks sending/receiving tasks.
//! No shared memory, only serialized data transfer.

pub const QUEUE_COUNT: usize = 8;
pub const QUEUE_SIZE: usize = 512;

pub struct Queue {
    data: [u8; QUEUE_SIZE],
    available: usize,
}