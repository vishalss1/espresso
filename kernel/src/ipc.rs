//! IPC module - named message queues between tasks
//!
//! 8 static queues of 512B each. No shared memory, only serialized data transfer.

pub const QUEUE_COUNT: usize = 8;
pub const QUEUE_SIZE: usize = 512;
pub const MAX_QUEUE_NAME: usize = 16;

pub struct Queue {
    pub name: [u8; MAX_QUEUE_NAME],
    pub name_len: u8,
    pub data: [u8; QUEUE_SIZE],
    pub write_pos: usize,
    pub read_pos: usize,
    pub count: usize,
    pub owner_pid: usize,
}

pub static mut QUEUES: [Queue; QUEUE_COUNT] = [
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
    Queue { name: [0; MAX_QUEUE_NAME], name_len: 0, data: [0; QUEUE_SIZE], write_pos: 0, read_pos: 0, count: 0, owner_pid: 0 },
];

pub fn init_queues() {
    unsafe {
        for q in QUEUES.iter_mut() {
            q.name_len = 0;
            q.write_pos = 0;
            q.read_pos = 0;
            q.count = 0;
            q.owner_pid = 0;
        }
    }
}

pub fn open_queue(name: &str, pid: usize) -> Result<usize, &'static str> {
    unsafe {
        let slot = QUEUES.iter_mut().find(|q| q.name_len == 0).ok_or("ERR_NO_QUEUES")?;
        let bytes = name.as_bytes();
        let len = core::cmp::min(bytes.len(), MAX_QUEUE_NAME - 1);
        slot.name[..len].copy_from_slice(&bytes[..len]);
        slot.name[len] = 0;
        slot.name_len = len as u8;
        slot.write_pos = 0;
        slot.read_pos = 0;
        slot.count = 0;
        slot.owner_pid = pid;
        let idx = slot as *mut Queue as usize;
        let base = &raw mut QUEUES as *mut [Queue; QUEUE_COUNT] as usize;
        Ok((idx - base) / core::mem::size_of::<Queue>())
    }
}

pub fn send_message(handle: usize, data: &[u8]) -> Result<(), &'static str> {
    unsafe {
        if handle >= QUEUE_COUNT { return Err("ERR_INVALID_HANDLE"); }
        let q = &mut QUEUES[handle];
        if q.name_len == 0 { return Err("ERR_QUEUE_CLOSED"); }
        for &byte in data {
            if q.count >= QUEUE_SIZE { return Err("ERR_QUEUE_FULL"); }
            q.data[q.write_pos] = byte;
            q.write_pos = (q.write_pos + 1) % QUEUE_SIZE;
            q.count += 1;
        }
        Ok(())
    }
}

pub fn recv_message(handle: usize, buf: &mut [u8], _timeout_ms: u32) -> Result<usize, &'static str> {
    unsafe {
        if handle >= QUEUE_COUNT { return Err("ERR_INVALID_HANDLE"); }
        let q = &mut QUEUES[handle];
        if q.name_len == 0 { return Err("ERR_QUEUE_CLOSED"); }
        let mut count = 0;
        while count < buf.len() && q.count > 0 {
            buf[count] = q.data[q.read_pos];
            q.read_pos = (q.read_pos + 1) % QUEUE_SIZE;
            q.count -= 1;
            count += 1;
        }
        Ok(count)
    }
}

pub fn close_queue(handle: usize) -> Result<(), &'static str> {
    unsafe {
        if handle >= QUEUE_COUNT { return Err("ERR_INVALID_HANDLE"); }
        let q = &mut QUEUES[handle];
        q.name_len = 0;
        q.write_pos = 0;
        q.read_pos = 0;
        q.count = 0;
        q.owner_pid = 0;
        Ok(())
    }
}
