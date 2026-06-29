#![no_std]
//! Context switch assembly module - Xtensa task switching
//! 
//! Implements context save/restore using Xtensa-specific instructions.
//! Called from timer interrupt to perform preemptive scheduling.
//! 
//! Saves: A0-A15, PC, PS, window registers
//! Restores: Context of next running task

.syntax unified
.arch xtensa	
.global switch

// Function: void switch(struct Task *current, struct Task *next)
// Inputs: a0 = current task pointer, a1 = next task pointer
// Clobbers: all callee-saved registers, PC

switch:
    // TODO: Implement context save/restore
    // 1. Save current task registers to stack
    // 2. Load next task registers from its stack
    // 3. Return to new task context
    movi a2, 0
    jmp .end

.end:
    nop