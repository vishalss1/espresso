//! Forth interpreter — threaded, 16KB dictionary, interactive + compile-to-.espr
//!
//! Data stack (cells) and return stack live in the static 18KB Forth region.
//! Words are dictionary entries: link → flags → name → body.
//! Native words have a Rust fn pointer; colon words have a sequence of XTs.

pub const DICTIONARY_SIZE: usize = 14_336;
pub const DATA_STACK_SIZE: usize = 512;
pub const RETURN_STACK_SIZE: usize = 512;

pub struct Forth {
    dict: [u8; DICTIONARY_SIZE],
    data_stack: [u32; DATA_STACK_SIZE],
    return_stack: [u32; RETURN_STACK_SIZE],
}

impl Forth {
    pub const fn new() -> Self {
        Self {
            dict: [0; DICTIONARY_SIZE],
            data_stack: [0; DATA_STACK_SIZE],
            return_stack: [0; RETURN_STACK_SIZE],
        }
    }
}

// ── Global Forth state ───────────────────────────────────────────────────────

use core::sync::atomic::{AtomicUsize, Ordering};

static DSP: AtomicUsize = AtomicUsize::new(0); // data stack pointer (index into stack)
static RSP: AtomicUsize = AtomicUsize::new(0); // return stack pointer
static DICT_HERE: AtomicUsize = AtomicUsize::new(0); // next free byte in dictionary
static COMPILING: AtomicUsize = AtomicUsize::new(0); // 0=interpret, 1=compile

// Word input buffer
static mut INPUT_BUF: [u8; 256] = [0u8; 256];
static mut INPUT_POS: usize = 0;
static mut INPUT_LEN: usize = 0;

// Dictionary pointer to the latest word
static mut LATEST: u32 = 0;

fn forth_buf() -> &'static mut Forth {
    // Forth struct placed at start of FORTH_DICT region (linker: 0x3FFB9800)
    unsafe { &mut *(0x3FFB9800 as *mut Forth) }
}

// ── Stack primitives ─────────────────────────────────────────────────────────

fn ds_push(val: u32) {
    let sp = DSP.load(Ordering::Relaxed);
    let f = forth_buf();
    if sp < DATA_STACK_SIZE {
        f.data_stack[sp] = val;
        DSP.store(sp + 1, Ordering::Relaxed);
    }
}

fn ds_pop() -> u32 {
    let sp = DSP.load(Ordering::Relaxed);
    if sp > 0 {
        let f = forth_buf();
        let sp2 = sp - 1;
        DSP.store(sp2, Ordering::Relaxed);
        f.data_stack[sp2]
    } else {
        0
    }
}

fn ds_peek() -> u32 {
    let sp = DSP.load(Ordering::Relaxed);
    if sp > 0 {
        let f = forth_buf();
        f.data_stack[sp - 1]
    } else {
        0
    }
}

fn ds_pick(n: usize) -> u32 {
    let sp = DSP.load(Ordering::Relaxed);
    let f = forth_buf();
    if sp > n {
        f.data_stack[sp - 1 - n]
    } else {
        0
    }
}

fn rs_push(val: u32) {
    let sp = RSP.load(Ordering::Relaxed);
    let f = forth_buf();
    if sp < RETURN_STACK_SIZE {
        f.return_stack[sp] = val;
        RSP.store(sp + 1, Ordering::Relaxed);
    }
}

fn rs_pop() -> u32 {
    let sp = RSP.load(Ordering::Relaxed);
    if sp > 0 {
        let f = forth_buf();
        let sp2 = sp - 1;
        RSP.store(sp2, Ordering::Relaxed);
        f.return_stack[sp2]
    } else {
        0
    }
}

// ── Dictionary ───────────────────────────────────────────────────────────────

const IMMEDIATE: u8 = 0x01;
const HIDDEN: u8 = 0x02;

/// Dictionary entry layout:
/// [link:u32][flags:u8][namelen:u8][name:namelen][body...]
const HDR_SIZE: usize = 4 + 1 + 1; // link + flags + namelen

/// Find a word in the dictionary. Returns body pointer, or 0 if not found.
fn dict_find(name: &[u8]) -> u32 {
    let buf = forth_buf();
    let mut link = unsafe { LATEST };
    while link != 0 {
        let base = link as usize;
        let entry_flags = buf.dict[base + 4];
        let entry_namelen = buf.dict[base + 5] as usize;
        let entry_name = &buf.dict[base + 6..base + 6 + entry_namelen];
        if (entry_flags & HIDDEN) == 0 && entry_name == name {
            return base as u32;
        }
        link = u32::from_le_bytes([
            buf.dict[base],
            buf.dict[base + 1],
            buf.dict[base + 2],
            buf.dict[base + 3],
        ]);
    }
    0
}

/// Check if a word is immediate.
fn dict_is_immediate(entry: u32) -> bool {
    let buf = forth_buf();
    (buf.dict[entry as usize + 4] & IMMEDIATE) != 0
}

/// Get pointer to body of a dictionary entry.
fn dict_body(entry: u32) -> u32 {
    let buf = forth_buf();
    let namelen = buf.dict[entry as usize + 5] as usize;
    (entry as usize + HDR_SIZE + namelen) as u32
}

/// Get the code-field pointer at the start of a colon word's body.
/// For colon words: body[0] = pointer to native DOCOL, body[1..] = sequence of XTs.
fn dict_xt(entry: u32) -> u32 {
    let body = dict_body(entry);
    let buf = forth_buf();
    u32::from_le_bytes([
        buf.dict[body as usize],
        buf.dict[body as usize + 1],
        buf.dict[body as usize + 2],
        buf.dict[body as usize + 3],
    ])
}

/// Allocate space in the dictionary. Returns the address of the allocated region.
fn dict_allot(size: usize) -> u32 {
    let here = DICT_HERE.load(Ordering::Relaxed);
    let new_here = here + size;
    if new_here <= DICTIONARY_SIZE {
        DICT_HERE.store(new_here, Ordering::Relaxed);
        here as u32
    } else {
        0 // out of dictionary space
    }
}

/// Append a u32 to the dictionary.
fn dict_append_u32(val: u32) {
    let addr = dict_allot(4) as usize;
    let buf = forth_buf();
    let b = val.to_le_bytes();
    buf.dict[addr] = b[0];
    buf.dict[addr + 1] = b[1];
    buf.dict[addr + 2] = b[2];
    buf.dict[addr + 3] = b[3];
}

/// Append a byte to the dictionary.
fn dict_append_byte(val: u8) {
    let addr = dict_allot(1) as usize;
    forth_buf().dict[addr] = val;
}

/// Create a new dictionary header. Returns body address.
fn dict_create(name: &[u8], flags: u8) -> u32 {
    let prev_latest = unsafe { LATEST };
    let name_len = core::cmp::min(name.len(), 31);

    // Link
    dict_append_u32(prev_latest);
    // Flags
    dict_append_byte(flags);
    // Name length
    dict_append_byte(name_len as u8);
    // Name bytes
    for &b in name.iter().take(name_len) {
        dict_append_byte(b);
    }

    let entry_addr = (DICT_HERE.load(Ordering::Relaxed) - HDR_SIZE - name_len) as u32;
    unsafe { LATEST = entry_addr; }
    dict_body(entry_addr)
}

// ── Input ────────────────────────────────────────────────────────────────────

fn fill_input() {
    unsafe {
        INPUT_LEN = 0;
        INPUT_POS = 0;
        loop {
            crate::keyboard::poll();
            if let Some(b) = crate::tty::poll_read() {
                if b == b'\r' || b == b'\n' {
                    crate::tty::write_both(b'\r');
                    crate::tty::write_both(b'\n');
                    break;
                } else if b == 8 || b == 127 {
                    if INPUT_LEN > 0 {
                        INPUT_LEN -= 1;
                        crate::tty::write_both(8);
                        crate::tty::write_both(b' ');
                        crate::tty::write_both(8);
                    }
                } else if b >= 32 && b <= 126 && INPUT_LEN < INPUT_BUF.len() {
                    INPUT_BUF[INPUT_LEN] = b;
                    INPUT_LEN += 1;
                    crate::tty::write_both(b);
                }
                unsafe { crate::wdt_feed(); }
            }
        }
    }
}

fn next_word(buf: &mut [u8]) -> usize {
    unsafe {
        // Skip whitespace
        while INPUT_POS < INPUT_LEN && INPUT_BUF[INPUT_POS] <= b' ' {
            INPUT_POS += 1;
        }
        let start = INPUT_POS;
        while INPUT_POS < INPUT_LEN && INPUT_BUF[INPUT_POS] > b' ' {
            INPUT_POS += 1;
        }
        let len = INPUT_POS - start;
        if len > 0 && len < buf.len() {
            for i in 0..len {
                // Force uppercase for dictionary lookup
                let b = INPUT_BUF[start + i];
                buf[i] = if b >= b'a' && b <= b'z' { b - 32 } else { b };
            }
        }
        len
    }
}

// ── Number parsing ───────────────────────────────────────────────────────────

fn parse_number(s: &[u8]) -> Option<i32> {
    if s.is_empty() {
        return None;
    }
    let neg = s[0] == b'-';
    let start = if neg { 1 } else { 0 };
    let mut val: i32 = 0;
    for &b in &s[start..] {
        if b >= b'0' && b <= b'9' {
            val = val.checked_mul(10)?.checked_add((b - b'0') as i32)?;
        } else {
            return None;
        }
    }
    Some(if neg { -val } else { val })
}

// ── Output helpers ───────────────────────────────────────────────────────────

fn emit_num(n: i32) {
    let mut tmp = [0u8; 12];
    let mut i = 0;
    let neg = n < 0;
    let mut v = if neg { -(n as i64) } else { n as i64 };
    if v == 0 {
        tmp[0] = b'0';
        i = 1;
    } else {
        while v > 0 {
            tmp[i] = b'0' + (v % 10) as u8;
            v /= 10;
            i += 1;
        }
    }
    if neg {
        crate::tty::write_both(b'-');
    }
    let mut j = i;
    while j > 0 {
        j -= 1;
        crate::tty::write_both(tmp[j]);
    }
    crate::tty::write_both(b' ');
}

// ── Built-in native words ────────────────────────────────────────────────────

// Each native word is: name, flags, code-pointer
// We store native words as entries where the code field points to a "DOCOL"-like
// trampoline. For simplicity, native words are identified by a small integer ID
// and dispatched by the interpreter.

const NATIVE_DOVAR: u32 = 0xFFFF_FFF0;
const NATIVE_DOCON: u32 = 0xFFFF_FFF1;
const NATIVE_DOBRANCH: u32 = 0xFFFF_FFF2;
const NATIVE_DOZBRANCH: u32 = 0xFFFF_FFF3;
const NATIVE_DOEXIT: u32 = 0xFFFF_FFF4;
const NATIVE_DOLOOP: u32 = 0xFFFF_FFF5;

fn is_native(xt: u32) -> bool {
    xt >= 0xFFFF_FFF0
}

fn execute_native(id: u32) {
    match id {
        // These are handled inline by the colon-word executor, not standalone
        _ => {}
    }
}

/// Execute a colon word: body contains DOCOL followed by a sequence of XTs.
fn execute_colon(body: u32) {
    let buf = forth_buf();
    let mut ip = body as usize;

    // DOCOL: push return address onto return stack, set IP to body+4
    rs_push((ip + 4) as u32);
    ip += 4;

    loop {
        // Read next XT from the colon body
        let xt = u32::from_le_bytes([
            buf.dict[ip],
            buf.dict[ip + 1],
            buf.dict[ip + 2],
            buf.dict[ip + 3],
        ]);
        ip += 4;

        if xt == 0 {
            // EXIT: pop return stack
            let ret = rs_pop();
            if ret == 0 {
                break; // done
            }
            ip = ret as usize;
            continue;
        }

        if is_native(xt) {
            match xt {
                NATIVE_DOBRANCH => {
                    let target = u32::from_le_bytes([
                        buf.dict[ip],
                        buf.dict[ip + 1],
                        buf.dict[ip + 2],
                        buf.dict[ip + 3],
                    ]);
                    ip = target as usize;
                }
                NATIVE_DOZBRANCH => {
                    let cond = ds_pop();
                    let target = u32::from_le_bytes([
                        buf.dict[ip],
                        buf.dict[ip + 1],
                        buf.dict[ip + 2],
                        buf.dict[ip + 3],
                    ]);
                    if cond == 0 {
                        ip = target as usize;
                    }
                }
                NATIVE_DOEXIT => {
                    let ret = rs_pop();
                    if ret == 0 { break; }
                    ip = ret as usize;
                }
                NATIVE_DOVAR => {
                    // The XT after DOVAR is the variable's address
                    let addr = u32::from_le_bytes([
                        buf.dict[ip],
                        buf.dict[ip + 1],
                        buf.dict[ip + 2],
                        buf.dict[ip + 3],
                    ]);
                    ds_push(addr);
                    ip += 4;
                }
                NATIVE_DOCON => {
                    let addr = u32::from_le_bytes([
                        buf.dict[ip],
                        buf.dict[ip + 1],
                        buf.dict[ip + 2],
                        buf.dict[ip + 3],
                    ]);
                    let val = u32::from_le_bytes([
                        buf.dict[addr as usize],
                        buf.dict[addr as usize + 1],
                        buf.dict[addr as usize + 2],
                        buf.dict[addr as usize + 3],
                    ]);
                    ds_push(val);
                    ip += 4;
                }
                NATIVE_DOLOOP => {
                    let limit = ds_pop();
                    let idx = ds_pop() + 1;
                    ds_push(idx);
                    if idx < limit {
                        let target = u32::from_le_bytes([
                            buf.dict[ip],
                            buf.dict[ip + 1],
                            buf.dict[ip + 2],
                            buf.dict[ip + 3],
                        ]);
                        ip = target as usize;
                    } else {
                        ip += 4;
                    }
                }
                _ => {}
            }
            continue;
        }

        // Non-native XT: look up in dictionary to find what it does
        if let Some(native_id) = lookup_native_by_xt(xt) {
            execute_builtin(native_id);
        } else {
            // Assume it's a colon word XT — find the entry and recurse
            // For colon words, xt IS the body pointer + 4 (past DOCOL)
            rs_push(ip as u32);
            ip = (xt - 4) as usize; // back up to re-read DOCOL
            // Actually: push current ip, jump to xt
            // xt points to the body which starts with DOCOL. The DOCOL pushes ip.
            // So we just set ip = xt and let the next iteration handle DOCOL.
            ip = xt as usize;
            rs_push((ip) as u32);
            ip += 4;
        }
    }
}

fn lookup_native_by_xt(xt: u32) -> Option<u32> {
    unsafe {
    for entry in NATIVE_TABLE.iter() {
        if entry.xt == xt {
            return Some(entry.id);
        }
    }
    }
    None
}

#[derive(Copy, Clone)]
struct NativeEntry {
    id: u32,
    xt: u32,
}

// We assign each built-in a small ID. The XT is the body pointer where we store
// the native code (for now, a marker value). In practice, we dispatch by name
// during interpretation. The colon compiler emits XTs that point to dictionary
// entries' bodies.

// ── Built-in word dispatch ────────────────────────────────────────────────────

// Native word IDs
const W_DUP: u32 = 1;
const W_DROP: u32 = 2;
const W_SWAP: u32 = 3;
const W_OVER: u32 = 4;
const W_ROT: u32 = 5;
const W_NIP: u32 = 6;
const W_PICK: u32 = 7;
const W_ADD: u32 = 10;
const W_SUB: u32 = 11;
const W_MUL: u32 = 12;
const W_DIV: u32 = 13;
const W_MOD: u32 = 14;
const W_AND: u32 = 15;
const W_OR: u32 = 16;
const W_XOR: u32 = 17;
const W_NOT: u32 = 18;
const W_LSHIFT: u32 = 19;
const W_RSHIFT: u32 = 20;
const W_EQ: u32 = 21;
const W_NEQ: u32 = 22;
const W_LT: u32 = 23;
const W_GT: u32 = 24;
const W_LE: u32 = 25;
const W_GE: u32 = 26;
const W_FETCH: u32 = 30;
const W_STORE: u32 = 31;
const W_CFETCH: u32 = 32;
const W_CSTORE: u32 = 33;
const W_EMIT: u32 = 40;
const W_CR: u32 = 41;
const W_SPACE: u32 = 42;
const W_DOT: u32 = 43;
const W_DOTS: u32 = 44;
const W_KEY: u32 = 45;
const W_DOTSTR: u32 = 46;
const W_BRANCH: u32 = 50;
const W_ZBRANCH: u32 = 51;
const W_EXIT: u32 = 52;
const W_COLON: u32 = 60;
const W_SEMICOLON: u32 = 61;
const W_IMMEDIATE: u32 = 62;
const W_COMPILE: u32 = 63;
const W_LIT: u32 = 64;
const W_IF: u32 = 70;
const W_ELSE: u32 = 71;
const W_THEN: u32 = 72;
const W_BEGIN: u32 = 73;
const W_UNTIL: u32 = 74;
const W_DO: u32 = 75;
const W_LOOP: u32 = 76;
const W_I: u32 = 77;
const W_VARIABLE: u32 = 80;
const W_CONSTANT: u32 = 81;
const W_HERE: u32 = 82;
const W_ALLOT: u32 = 83;
const W_COMMA: u32 = 84;
const W_CCOMMA: u32 = 85;
const W_SEE: u32 = 90;
const W_WORDS: u32 = 91;
const W_LIST: u32 = 92;

static mut NATIVE_TABLE: [NativeEntry; 80] = [NativeEntry { id: 0, xt: 0 }; 80];

fn register_native(id: u32, name: &[u8]) {
    let body = dict_create(name, 0);
    dict_append_u32(id + 0x8000_0000);
    unsafe {
        let idx = NATIVE_TABLE.iter().position(|e| e.id == 0).unwrap();
        NATIVE_TABLE[idx] = NativeEntry { id, xt: body + 4 };
    }
}

fn register_native_imm(id: u32, name: &[u8]) {
    let body = dict_create(name, IMMEDIATE);
    dict_append_u32(id + 0x8000_0000);
    unsafe {
        let idx = NATIVE_TABLE.iter().position(|e| e.id == 0).unwrap();
        NATIVE_TABLE[idx] = NativeEntry { id, xt: body + 4 };
    }
}

fn execute_builtin(id: u32) {
    match id {
        W_DUP => { let a = ds_pop(); ds_push(a); ds_push(a); }
        W_DROP => { ds_pop(); }
        W_SWAP => { let a = ds_pop(); let b = ds_pop(); ds_push(a); ds_push(b); }
        W_OVER => { let a = ds_pop(); let b = ds_pop(); ds_push(b); ds_push(a); ds_push(b); }
        W_ROT => {
            let a = ds_pop(); let b = ds_pop(); let c = ds_pop();
            ds_push(b); ds_push(a); ds_push(c);
        }
        W_NIP => { let _ = ds_pop(); }
        W_PICK => { let n = ds_pop() as usize; let v = ds_pick(n); ds_push(v); }
        W_ADD => { let b = ds_pop(); let a = ds_pop(); ds_push(a.wrapping_add(b)); }
        W_SUB => { let b = ds_pop(); let a = ds_pop(); ds_push(a.wrapping_sub(b)); }
        W_MUL => { let b = ds_pop(); let a = ds_pop(); ds_push(a.wrapping_mul(b)); }
        W_DIV => { let b = ds_pop(); let a = ds_pop(); ds_push(if b != 0 { a / b } else { 0 }); }
        W_MOD => { let b = ds_pop(); let a = ds_pop(); ds_push(if b != 0 { a % b } else { 0 }); }
        W_AND => { let b = ds_pop(); let a = ds_pop(); ds_push(a & b); }
        W_OR => { let b = ds_pop(); let a = ds_pop(); ds_push(a | b); }
        W_XOR => { let b = ds_pop(); let a = ds_pop(); ds_push(a ^ b); }
        W_NOT => { let a = ds_pop(); ds_push(if a == 0 { 1 } else { 0 }); }
        W_LSHIFT => { let b = ds_pop(); let a = ds_pop(); ds_push(a << b); }
        W_RSHIFT => { let b = ds_pop(); let a = ds_pop(); ds_push(a >> b); }
        W_EQ => { let b = ds_pop(); let a = ds_pop(); ds_push(if a == b { 1 } else { 0 }); }
        W_NEQ => { let b = ds_pop(); let a = ds_pop(); ds_push(if a != b { 1 } else { 0 }); }
        W_LT => { let b = ds_pop(); let a = ds_pop(); ds_push(if (a as i32) < (b as i32) { 1 } else { 0 }); }
        W_GT => { let b = ds_pop(); let a = ds_pop(); ds_push(if (a as i32) > (b as i32) { 1 } else { 0 }); }
        W_LE => { let b = ds_pop(); let a = ds_pop(); ds_push(if (a as i32) <= (b as i32) { 1 } else { 0 }); }
        W_GE => { let b = ds_pop(); let a = ds_pop(); ds_push(if (a as i32) >= (b as i32) { 1 } else { 0 }); }
        W_FETCH => { let a = ds_pop(); let v = unsafe { core::ptr::read_volatile(a as *const u32) }; ds_push(v); }
        W_STORE => { let a = ds_pop(); let v = ds_pop(); unsafe { core::ptr::write_volatile(a as *mut u32, v); } }
        W_CFETCH => { let a = ds_pop(); let v = unsafe { core::ptr::read_volatile(a as *const u8) } as u32; ds_push(v); }
        W_CSTORE => { let a = ds_pop(); let v = ds_pop() as u8; unsafe { core::ptr::write_volatile(a as *mut u8, v); } }
        W_EMIT => { let c = ds_pop() as u8; crate::tty::write_both(c); }
        W_CR => { crate::tty::write_str_both("\r\n"); }
        W_SPACE => { crate::tty::write_both(b' '); }
        W_DOT => { let n = ds_pop() as i32; emit_num(n); }
        W_DOTS => {
            let sp = DSP.load(Ordering::Relaxed);
            let f = forth_buf();
            crate::tty::write_str_both("<");
            emit_num(sp as i32);
            crate::tty::write_str_both("> ");
            for i in 0..sp {
                emit_num(f.data_stack[i] as i32);
            }
        }
        W_KEY => {
            loop {
                crate::keyboard::poll();
                if let Some(b) = crate::tty::poll_read() {
                    ds_push(b as u32);
                    break;
                }
            }
        }
        _ => {}
    }
}

// ── Interpreter ──────────────────────────────────────────────────────────────

fn find_builtin(name: &[u8]) -> Option<u32> {
    // Try dictionary lookup first
    let entry = dict_find(name);
    if entry != 0 {
        let body = dict_body(entry);
        let buf = forth_buf();
        let marker = u32::from_le_bytes([
            buf.dict[body as usize],
            buf.dict[body as usize + 1],
            buf.dict[body as usize + 2],
            buf.dict[body as usize + 3],
        ]);
        if marker & 0x8000_0000 != 0 {
            return Some(marker & 0x7FFF_FFFF);
        }
        return None; // colon word, handled separately
    }
    None
}

/// Process one word during interpretation or compilation.
fn process_word(word: &[u8]) {
    if word.is_empty() {
        return;
    }

    let compiling = COMPILING.load(Ordering::Relaxed) != 0;

    // Is it a number?
    if let Some(n) = parse_number(word) {
        if compiling {
            // Compile LIT + literal value
            let lit_entry = dict_find(b"LIT");
            if lit_entry != 0 {
                let body = dict_body(lit_entry);
                dict_append_u32(body);
            }
            dict_append_u32(n as u32);
        } else {
            ds_push(n as u32);
        }
        return;
    }

    // Look up in dictionary
    let entry = dict_find(word);
    if entry != 0 {
        let is_imm = dict_is_immediate(entry);
        if compiling && !is_imm {
            // Compilation mode: append the word's XT to the current definition
            let body = dict_body(entry);
            let buf = forth_buf();
            let marker = u32::from_le_bytes([
                buf.dict[body as usize],
                buf.dict[body as usize + 1],
                buf.dict[body as usize + 2],
                buf.dict[body as usize + 3],
            ]);
            if marker & 0x8000_0000 != 0 {
                // Native word — emit its body XT directly
                dict_append_u32(body + 4);
            } else {
                // Colon word — emit a call to its body
                dict_append_u32(body + 4); // +4 to skip DOCOL pointer... no, include it
                // Actually for colon words, body points to DOCOL. We need to call it.
                // The executor will see the XT and jump to it.
                // Let's fix: we want to push the body address as the XT.
                // Remove what we just wrote and rewrite
                let here = DICT_HERE.load(Ordering::Relaxed);
                DICT_HERE.store(here - 4, Ordering::Relaxed);
                dict_append_u32(body); // include DOCOL
            }
        } else {
            // Immediate or interpret mode: execute it
            let body = dict_body(entry);
            let buf = forth_buf();
            let marker = u32::from_le_bytes([
                buf.dict[body as usize],
                buf.dict[body as usize + 1],
                buf.dict[body as usize + 2],
                buf.dict[body as usize + 3],
            ]);
            if marker & 0x8000_0000 != 0 {
                // Native word
                let native_id = marker & 0x7FFF_FFFF;
                match native_id {
                    W_COLON => {
                        COMPILING.store(1, Ordering::Relaxed);
                        // Read next word as the name
                        let mut name_buf = [0u8; 32];
                        let len = next_word(&mut name_buf);
                        if len > 0 {
                            let _body = dict_create(&name_buf[..len], 0);
                            // No DOCOL needed; we store XTs directly
                        }
                    }
                    W_SEMICOLON => {
                        // Append EXIT marker
                        let exit_entry = dict_find(b"EXIT");
                        if exit_entry != 0 {
                            let exit_body = dict_body(exit_entry);
                            dict_append_u32(exit_body);
                        } else {
                            dict_append_u32(0); // null = exit
                        }
                        COMPILING.store(0, Ordering::Relaxed);
                    }
                    W_IMMEDIATE => {
                        // Make the last-defined word immediate
                        unsafe {
                            let entry = LATEST;
                            let buf = forth_buf();
                            buf.dict[entry as usize + 4] |= IMMEDIATE;
                        }
                    }
                    W_IF => {
                        // Compilation: emit ZBRANCH placeholder, push address
                        let zbranch_entry = dict_find(b"ZBRANCH");
                        if zbranch_entry != 0 {
                            let body = dict_body(zbranch_entry);
                            dict_append_u32(body + 4);
                        }
                        rs_push(DICT_HERE.load(Ordering::Relaxed) as u32);
                        dict_append_u32(0); // placeholder for branch target
                    }
                    W_ELSE => {
                        // Patch IF's placeholder, emit BRANCH, push new placeholder
                        let here = DICT_HERE.load(Ordering::Relaxed) as u32;
                        let patch_addr = rs_pop() as usize;
                        let buf = forth_buf();
                        // Patch the ZBRANCH target to point to here+4 (after the BRANCH we're about to emit)
                        let patch_val = (here + 4) as u32;
                        buf.dict[patch_addr] = patch_val.to_le_bytes()[0];
                        buf.dict[patch_addr + 1] = patch_val.to_le_bytes()[1];
                        buf.dict[patch_addr + 2] = patch_val.to_le_bytes()[2];
                        buf.dict[patch_addr + 3] = patch_val.to_le_bytes()[3];
                        // Emit BRANCH
                        let branch_entry = dict_find(b"BRANCH");
                        if branch_entry != 0 {
                            let body = dict_body(branch_entry);
                            dict_append_u32(body + 4);
                        }
                        rs_push(DICT_HERE.load(Ordering::Relaxed) as u32);
                        dict_append_u32(0);
                    }
                    W_THEN => {
                        // Patch previous IF or ELSE placeholder
                        let here = DICT_HERE.load(Ordering::Relaxed) as u32;
                        let patch_addr = rs_pop() as usize;
                        let buf = forth_buf();
                        let patch_val = here as u32;
                        buf.dict[patch_addr] = patch_val.to_le_bytes()[0];
                        buf.dict[patch_addr + 1] = patch_val.to_le_bytes()[1];
                        buf.dict[patch_addr + 2] = patch_val.to_le_bytes()[2];
                        buf.dict[patch_addr + 3] = patch_val.to_le_bytes()[3];
                    }
                    W_BEGIN => {
                        rs_push(DICT_HERE.load(Ordering::Relaxed) as u32);
                    }
                    W_UNTIL => {
                        let _here = DICT_HERE.load(Ordering::Relaxed) as u32;
                        let begin_addr = rs_pop();
                        // Emit ZBRANCH back to begin
                        let zbranch_entry = dict_find(b"ZBRANCH");
                        if zbranch_entry != 0 {
                            let body = dict_body(zbranch_entry);
                            dict_append_u32(body + 4);
                        }
                        dict_append_u32(begin_addr);
                    }
                    _ => {
                        execute_builtin(native_id);
                    }
                }
            } else {
                // Colon word — execute it
                execute_colon(body);
            }
        }
        return;
    }

    // Unknown word
    crate::tty::write_str_both("'");
    crate::tty::write_str_both(core::str::from_utf8(word).unwrap_or("?"));
    crate::tty::write_str_both("' ?\n");
}

/// Main Forth interpreter loop. Called from the shell `forth` command.
pub fn run_interpreter() {
    // Initialize dictionary
    DICT_HERE.store(0, Ordering::Relaxed);
    unsafe { LATEST = 0; }
    DSP.store(0, Ordering::Relaxed);
    RSP.store(0, Ordering::Relaxed);
    COMPILING.store(0, Ordering::Relaxed);
    unsafe {
        NATIVE_TABLE = [NativeEntry { id: 0, xt: 0 }; 80];
    }

    // Register all built-in words
    register_native(W_DUP, b"DUP");
    register_native(W_DROP, b"DROP");
    register_native(W_SWAP, b"SWAP");
    register_native(W_OVER, b"OVER");
    register_native(W_ROT, b"ROT");
    register_native(W_NIP, b"NIP");
    register_native(W_PICK, b"PICK");
    register_native(W_ADD, b"+");
    register_native(W_SUB, b"-");
    register_native(W_MUL, b"*");
    register_native(W_DIV, b"/");
    register_native(W_MOD, b"MOD");
    register_native(W_AND, b"AND");
    register_native(W_OR, b"OR");
    register_native(W_XOR, b"XOR");
    register_native(W_NOT, b"NOT");
    register_native(W_LSHIFT, b"LSHIFT");
    register_native(W_RSHIFT, b"RSHIFT");
    register_native(W_EQ, b"=");
    register_native(W_NEQ, b"<>");
    register_native(W_LT, b"<");
    register_native(W_GT, b">");
    register_native(W_LE, b"<=");
    register_native(W_GE, b">=");
    register_native(W_FETCH, b"@");
    register_native(W_STORE, b"!");
    register_native(W_CFETCH, b"C@");
    register_native(W_CSTORE, b"C!");
    register_native(W_EMIT, b"EMIT");
    register_native(W_CR, b"CR");
    register_native(W_SPACE, b"SPACE");
    register_native(W_DOT, b".");
    register_native(W_DOTS, b".S");
    register_native(W_KEY, b"KEY");
    register_native(W_LIT, b"LIT");
    register_native(W_BRANCH, b"BRANCH");
    register_native(W_ZBRANCH, b"ZBRANCH");
    register_native(W_EXIT, b"EXIT");
    register_native_imm(W_COLON, b":");
    register_native_imm(W_SEMICOLON, b";");
    register_native_imm(W_IMMEDIATE, b"IMMEDIATE");
    register_native_imm(W_IF, b"IF");
    register_native_imm(W_ELSE, b"ELSE");
    register_native_imm(W_THEN, b"THEN");
    register_native_imm(W_BEGIN, b"BEGIN");
    register_native_imm(W_UNTIL, b"UNTIL");

    crate::tty::write_str_both("Espresso Forth v0.1\n");
    crate::tty::write_str_both("Type words at the prompt. 'quit' to exit.\n\n");

    let mut word_buf = [0u8; 64];

    loop {
        if COMPILING.load(Ordering::Relaxed) != 0 {
            crate::tty::write_str_both("  ");
        } else {
            crate::tty::write_str_both("ok> ");
        }

        fill_input();

        loop {
            let len = next_word(&mut word_buf);
            if len == 0 {
                if COMPILING.load(Ordering::Relaxed) != 0 {
                    // In compile mode, newline ends the current word
                    // Append EXIT and switch to interpret mode
                    let exit_entry = dict_find(b"EXIT");
                    if exit_entry != 0 {
                        let exit_body = dict_body(exit_entry);
                        dict_append_u32(exit_body);
                    } else {
                        dict_append_u32(0);
                    }
                    COMPILING.store(0, Ordering::Relaxed);
                }
                break;
            }

            let word = &word_buf[..len];

            // Check for QUIT / BYE
            if word == b"QUIT" || word == b"BYE" {
                COMPILING.store(0, Ordering::Relaxed);
                return;
            }

            process_word(word);
        }
    }
}
