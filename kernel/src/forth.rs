#[no_std]
//! Forth interpreter module - ANS Forth subset implementation
//! 
//! Minimal embedded Forth with 16KB dictionary and native Xtensa code generation.
//! Compiles to .espr files for direct kernel execution.

pub const DICTIONARY_SIZE: usize = 16_384;
pub const DATA_STACK_SIZE: usize = 512;
pub const RETURN_STACK_SIZE: usize = 512;

pub struct Forth {
    dict: [u8; DICTIONARY_SIZE],
    data_stack: [u32; DATA_STACK_SIZE],
    return_stack: [u32; RETURN_STACK_SIZE],
}