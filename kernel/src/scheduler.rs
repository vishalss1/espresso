//! Preemptive round-robin scheduler for Espresso OS
//!
//! 4 static task slots, 10ms tick, Xtensa context switch via switch.S.

pub mod schedule;

pub const MAX_TASKS: usize = 4;
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

pub static mut TASKS: [TaskControlBlock; MAX_TASKS] = [
    TaskControlBlock { pid: 0, state: TaskState::Dead, stack_base: 0, stack_size: 0, sp: 0, entry: 0, caps: 0, ps: 0 },
    TaskControlBlock { pid: 0, state: TaskState::Dead, stack_base: 0, stack_size: 0, sp: 0, entry: 0, caps: 0, ps: 0 },
    TaskControlBlock { pid: 0, state: TaskState::Dead, stack_base: 0, stack_size: 0, sp: 0, entry: 0, caps: 0, ps: 0 },
    TaskControlBlock { pid: 0, state: TaskState::Dead, stack_base: 0, stack_size: 0, sp: 0, entry: 0, caps: 0, ps: 0 },
];

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
        crate::println!("[SPAWN] Spawning new task: entry=0x{:08X}, stack_size={}, caps=0x{:08X}", entry, stack_size, caps);
        let slot = TASKS.iter_mut().find(|t| t.state == TaskState::Dead)
             .ok_or("ERR_NO_TASKS")?;

        let pages_needed = (stack_size + 4095) / 4096;
        crate::println!("[SPAWN] Allocating {} stack pages...", pages_needed);
        let stack_base = crate::mem::pool::alloc_stack_pages(pages_needed)
            .ok_or("ERR_NO_MEMORY")?;
        crate::println!("[SPAWN] Allocated stack_base = 0x{:08X}", stack_base);

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
        crate::println!("[SPAWN] Initializing stack frame: sp=0x{:08X}, writing entry 0x{:08X} to *sp", frame_sp, entry);
        core::ptr::write_volatile(frame_sp as *mut u32, entry as u32);
        slot.sp = frame_sp;

        crate::println!("[SPAWN] Task slot initialized: pid={}, TCB.sp=0x{:08X}, TCB.ps=0x{:02X}", task_idx, slot.sp, slot.ps);

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

pub fn scheduler_tick() {
    unsafe {
        TICK_COUNT = TICK_COUNT.wrapping_add(1);
        crate::wdt_feed();

        let current = CURRENT_TASK;

        if task_count() <= 1 {
            if TASKS[current].state == TaskState::Dead {
                let next = next_ready_task(current);
                if next != current {
                    TASKS[next].state = TaskState::Running;
                    CURRENT_TASK = next;

                    // DIAGNOSTIC TRACE: dead-task yield path
                    let fifo = 0x3FF40000 as *mut u32;
                    core::ptr::write_volatile(fifo, (b'0' + current as u8) as u32);

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

            // DIAGNOSTIC TRACE: which task is yielding
            let fifo = 0x3FF40000 as *mut u32;
            core::ptr::write_volatile(fifo, (b'0' + current as u8) as u32);

            let current_ptr = &mut TASKS[current] as *mut TaskControlBlock;
            let next_ptr    = &mut TASKS[next]    as *mut TaskControlBlock;
            switch_context(current_ptr, next_ptr);

        } else {

        }
    }
}

extern "C" {
    fn switch(current: *mut TaskControlBlock, next: *mut TaskControlBlock);
}

fn switch_context(current: *mut TaskControlBlock, next: *mut TaskControlBlock) {
    // DIAGNOSTIC: switch_context called
    unsafe {
        let fifo = 0x3FF40000 as *mut u32;
        core::ptr::write_volatile(fifo, b'C' as u32);
    }
    unsafe { switch(current, next); }
}

/// Kill the current task and immediately context-switch to the next alive task.
/// Called from the assembly exception handler (_not_syscall) and SYS_EXIT.
/// Never returns — the calling task is dead and its stack is abandoned.
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

        TASKS[0].pid = 0;
        TASKS[0].state = TaskState::Running;
        TASKS[0].caps = 0xFFFFFFFF;
        TASKS[0].stack_base = 0x3FFB5400;
        TASKS[0].stack_size = 10240;
        TASKS[0].sp = 0x3FFB5400 + 10240;
        TASKS[0].ps = 0;
    }
}
