//! Preemptive round-robin scheduler for Espresso OS
//!
//! 8 static task slots (4 kernel services + 4 user apps per CLAUDE.md spec).
//! 10ms tick, Xtensa context switch via switch.S.

pub mod schedule;

pub const MAX_TASKS: usize = 8;
pub const TICKS_PER_SECOND: u32 = 100;
pub const TIMER_INTERVAL_US: u32 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskState {
    Dead,
    Ready,
    Running,
    Blocked,
}

#[repr(C)]
pub struct TaskControlBlock {
    pub pid: usize,
    pub state: TaskState,
    pub stack_base: usize,
    pub stack_size: usize,
    pub sp: usize,
    pub entry: usize,
    pub caps: u32,
    pub ps: u32,
}

const EMPTY_TCB: TaskControlBlock = TaskControlBlock {
    pid: 0,
    state: TaskState::Dead,
    stack_base: 0,
    stack_size: 0,
    sp: 0,
    entry: 0,
    caps: 0,
    ps: 0,
};

pub static mut TASKS: [TaskControlBlock; MAX_TASKS] = [EMPTY_TCB; MAX_TASKS];

#[no_mangle]
pub static mut CURRENT_TASK: usize = 0;
pub static mut TICK_COUNT: u32 = 0;

pub fn task_count() -> usize {
    unsafe { TASKS.iter().filter(|t| t.state != TaskState::Dead).count() }
}

pub fn next_ready_task(current: usize) -> usize {
    unsafe {
        let start = (current + 1) % MAX_TASKS;
        for i in 0..MAX_TASKS {
            let idx = (start + i) % MAX_TASKS;
            if TASKS[idx].state == TaskState::Ready {
                return idx;
            }
        }
        current
    }
}

pub fn spawn_task(entry: usize, stack_size: usize, caps: u32) -> Result<usize, &'static str> {
    unsafe {
        crate::println!("[SPAWN] Spawning task: entry=0x{:08X}, stack_size={}, caps=0x{:08X}", entry, stack_size, caps);
        let slot = TASKS.iter_mut().find(|t| t.state == TaskState::Dead)
             .ok_or("ERR_NO_TASKS")?;

        let pages_needed = (stack_size + 4095) / 4096;
        let stack_base = crate::mem::pool::alloc_stack_pages(pages_needed)
            .ok_or("ERR_NO_MEMORY")?;

        let sp = stack_base + pages_needed * 4096;

        let idx = slot as *mut TaskControlBlock as usize;
        let task_idx = (idx - (&raw mut TASKS as *mut [TaskControlBlock; MAX_TASKS] as usize)) / core::mem::size_of::<TaskControlBlock>();

        slot.pid = task_idx;
        slot.state = TaskState::Ready;
        slot.stack_base = stack_base;
        slot.stack_size = pages_needed * 4096;
        slot.entry = entry;
        slot.caps = caps;
        slot.ps = 0x20;

        let frame_sp = sp - 32;
        core::ptr::write_volatile(frame_sp as *mut u32, entry as u32);
        slot.sp = frame_sp;

        Ok(task_idx)
    }
}

pub fn kill_task(pid: usize) -> Result<(), &'static str> {
    unsafe {
        let slot = TASKS.iter_mut()
            .find(|t| t.pid == pid && t.state != TaskState::Dead)
            .ok_or("ERR_NOT_FOUND")?;

        if pid == 0 {
            return Err("ERR_CANNOT_KILL_IDLE");
        }

        let stack = slot.stack_base;
        slot.state = TaskState::Dead;
        slot.pid = 0;
        slot.stack_base = 0;
        slot.stack_size = 0;
        slot.sp = 0;
        slot.entry = 0;
        slot.caps = 0;
        slot.ps = 0;

        crate::mem::pool::free_page(stack);

        Ok(())
    }
}

/// Called once per tick interrupt. Note: WDT feed is done in tick ISR directly, not here.
pub fn scheduler_tick() {
    unsafe {
        TICK_COUNT = TICK_COUNT.wrapping_add(1);

        let current = CURRENT_TASK;

        if task_count() <= 1 {
            if TASKS[current].state == TaskState::Dead {
                let next = next_ready_task(current);
                if next != current {
                    TASKS[next].state = TaskState::Running;
                    CURRENT_TASK = next;

                    let cur = &mut TASKS[current] as *mut TaskControlBlock;
                    let nxt = &mut TASKS[next]    as *mut TaskControlBlock;
                    switch_context(cur, nxt);
                }
            }
            return;
        }

        let next = next_ready_task(current);

        if next != current && TASKS[next].state == TaskState::Ready {
            if TASKS[current].state != TaskState::Dead {
                TASKS[current].state = TaskState::Ready;
            }
            TASKS[next].state = TaskState::Running;
            CURRENT_TASK = next;

            let current_ptr = &mut TASKS[current] as *mut TaskControlBlock;
            let next_ptr    = &mut TASKS[next]    as *mut TaskControlBlock;
            switch_context(current_ptr, next_ptr);
        }
    }
}

extern "C" {
    fn switch(current: *mut TaskControlBlock, next: *mut TaskControlBlock);
}

fn switch_context(current: *mut TaskControlBlock, next: *mut TaskControlBlock) {
    unsafe { switch(current, next); }
}

#[no_mangle]
pub extern "C" fn kill_and_switch() -> ! {
    unsafe {
        let current = CURRENT_TASK;
        let pid = TASKS[current].pid;

        if pid != 0 {
            crate::event_log::log_task_kill(pid as u8);
            TASKS[current].state = TaskState::Dead;
            TASKS[current].pid = 0;
            crate::mem::pool::free_page(TASKS[current].stack_base);
            TASKS[current].stack_base = 0;
            TASKS[current].stack_size = 0;
            TASKS[current].sp = 0;
            TASKS[current].entry = 0;
            TASKS[current].caps = 0;
            TASKS[current].ps = 0;
        }

        let next = next_ready_task(current);
        if next != current {
            TASKS[next].state = TaskState::Running;
            CURRENT_TASK = next;
            let cur = &mut TASKS[current] as *mut TaskControlBlock;
            let nxt = &mut TASKS[next]    as *mut TaskControlBlock;
            switch_context(cur, nxt);
        }

        loop { core::arch::asm!("nop"); }
    }
}

pub fn init_scheduler() {
    unsafe {
        CURRENT_TASK = 0;
        TICK_COUNT = 0;

        for task in TASKS.iter_mut() {
            task.state = TaskState::Dead;
            task.pid = 0;
            task.stack_base = 0;
            task.stack_size = 0;
            task.sp = 0;
            task.entry = 0;
            task.caps = 0;
        }

        // Slot 0: Idle task
        TASKS[0].pid = 0;
        TASKS[0].state = TaskState::Running;
        TASKS[0].caps = crate::caps::CAP_ALL;
        TASKS[0].stack_base = 0x3FFB5400;
        TASKS[0].stack_size = 10240;
        TASKS[0].sp = 0x3FFB5400 + 10240;
        TASKS[0].ps = 0;
    }
}
