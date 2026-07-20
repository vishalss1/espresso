//! EDLIN-style line editor — shell built-in.
//!
//! Stores up to 64 lines in a static buffer. Commands:
//!   (empty line)  — insert text at current position
//!   :l            — list all lines
//!   :n            — go to line n
//!   :a            — append mode (cursor at end)
//!   :d            — delete current line
//!   :w            — write file to SD
//!   :q            — quit editor
//!   :r            — read file from SD
//!   :h            — help

const MAX_LINES: usize = 32;
const MAX_LINE_LEN: usize = 64;

static mut LINES: [[u8; MAX_LINE_LEN]; MAX_LINES] = [[0u8; MAX_LINE_LEN]; MAX_LINES];
static mut LINE_LENS: [u8; MAX_LINES] = [0u8; MAX_LINES];
static mut LINE_COUNT: usize = 0;
static mut CURSOR: usize = 0;
static mut DIRTY: bool = false;
static mut EDIT_FILE: [u8; 64] = [0u8; 64];
static mut EDIT_FILE_LEN: usize = 0;

fn write_both(b: u8) {
    crate::tty::write_both(b);
}

fn write_str(s: &str) {
    crate::tty::write_str_both(s);
}

fn fill_line() -> bool {
    let mut buf = [0u8; 128];
    let mut len = 0;
    loop {
        if let Some(b) = crate::tty::poll_read() {
            if b == b'\r' || b == b'\n' {
                write_both(b'\r');
                write_both(b'\n');
                break;
            } else if b == 8 || b == 127 {
                if len > 0 {
                    len -= 1;
                    write_both(8);
                    write_both(b' ');
                    write_both(8);
                }
            } else if b >= 32 && b <= 126 && len < buf.len() {
                buf[len] = b;
                len += 1;
                write_both(b);
            }
            unsafe { crate::wdt_feed(); }
        }
    }
    unsafe {
        let pos = CURSOR;
        if pos < MAX_LINES {
            let copy_len = core::cmp::min(len, MAX_LINE_LEN);
            for i in 0..copy_len {
                LINES[pos][i] = buf[i];
            }
            LINE_LENS[pos] = copy_len as u8;
            if pos >= LINE_COUNT {
                LINE_COUNT = pos + 1;
            }
            DIRTY = true;
        }
    }
    true
}

fn show_lines() {
    unsafe {
        for i in 0..LINE_COUNT {
            let marker = if i == CURSOR { b'>' } else { b' ' };
            write_both(marker);
            write_u32((i + 1) as u32);
            write_str(": ");
            let len = LINE_LENS[i] as usize;
            for j in 0..len {
                write_both(LINES[i][j]);
            }
            write_str("\r\n");
        }
        write_str("--- ");
        write_u32(LINE_COUNT as u32);
        write_str(" lines, cursor=");
        write_u32((CURSOR + 1) as u32);
        if DIRTY { write_str(" (modified)"); }
        write_str(" ---\r\n");
    }
}

fn delete_line() {
    unsafe {
        if LINE_COUNT == 0 || CURSOR >= LINE_COUNT { return; }
        let del = CURSOR;
        for i in del..(LINE_COUNT - 1) {
            LINES[i] = LINES[i + 1];
            LINE_LENS[i] = LINE_LENS[i + 1];
        }
        LINE_COUNT -= 1;
        if CURSOR >= LINE_COUNT && LINE_COUNT > 0 {
            CURSOR = LINE_COUNT - 1;
        }
        DIRTY = true;
    }
}

fn go_to_line(n: usize) {
    unsafe {
        if n > 0 && n <= LINE_COUNT {
            CURSOR = n - 1;
        } else if n > LINE_COUNT {
            CURSOR = LINE_COUNT;
        }
    }
}

fn parse_num(s: &str) -> Option<u32> {
    let mut val: u32 = 0;
    for b in s.as_bytes() {
        if *b >= b'0' && *b <= b'9' {
            val = val.checked_mul(10)?.checked_add((*b - b'0') as u32)?;
        } else {
            return None;
        }
    }
    Some(val)
}

fn format_u32_str(mut v: u32) -> [u8; 10] {
    let mut buf = [0u8; 10];
    let mut i = 0;
    if v == 0 {
        buf[0] = b'0';
        return buf;
    }
    let mut digits = [0u8; 10];
    let mut d = 0;
    while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
    let mut j = d;
    while j > 0 { j -= 1; buf[i] = digits[j]; i += 1; }
    buf
}

fn write_u32(v: u32) {
    let buf = format_u32_str(v);
    let mut i = 0;
    while i < buf.len() && buf[i] != 0 {
        write_both(buf[i]);
        i += 1;
    }
}

fn save_to_sd() {
    unsafe {
        if EDIT_FILE_LEN == 0 {
            write_str("No file name set. Use :w <path>\r\n");
            return;
        }
        let path = core::str::from_utf8(&EDIT_FILE[..EDIT_FILE_LEN]).unwrap_or("");
        let mut content = [0u8; 4096];
        let mut pos = 0;
        for i in 0..LINE_COUNT {
            let len = LINE_LENS[i] as usize;
            for j in 0..len {
                if pos < content.len() {
                    content[pos] = LINES[i][j];
                    pos += 1;
                }
            }
            if pos < content.len() {
                content[pos] = b'\n';
                pos += 1;
            }
        }
        match crate::drivers::sd::write_file(path, &content[..pos]) {
            Ok(()) => {
                DIRTY = false;
                write_str("Saved: ");
                write_str(path);
                write_str("\r\n");
            }
            Err(e) => {
                write_str("Save error: ");
                write_str(e);
                write_str("\r\n");
            }
        }
    }
}

fn load_from_sd(path: &str) {
    unsafe {
        LINE_COUNT = 0;
        CURSOR = 0;
        DIRTY = false;

        let path_bytes = path.as_bytes();
        let copy_len = core::cmp::min(path_bytes.len(), 63);
        for i in 0..copy_len {
            EDIT_FILE[i] = path_bytes[i];
        }
        EDIT_FILE_LEN = copy_len;

        write_str("(read from SD)\r\n");
    }
}

/// Enter the line editor. `path` is the file to edit.
pub fn run_editor(path: &str) {
    unsafe {
        LINE_COUNT = 0;
        CURSOR = 0;
        DIRTY = false;

        if !path.is_empty() {
            let path_bytes = path.as_bytes();
            let copy_len = core::cmp::min(path_bytes.len(), 63);
            for i in 0..copy_len {
                EDIT_FILE[i] = path_bytes[i];
            }
            EDIT_FILE_LEN = copy_len;
        }
    }

    write_str("Espresso Editor v0.1\r\n");
    write_str("Commands: :l list, :n goto, :a append, :d delete\r\n");
    write_str("          :w save, :q quit, :h help\r\n\r\n");

    let mut cmd_buf = [0u8; 128];

    loop {
        write_str(":");
        // Read a line
        let mut cmd_len = 0;
        loop {
            if let Some(b) = crate::tty::poll_read() {
                if b == b'\r' || b == b'\n' {
                    write_both(b'\r');
                    write_both(b'\n');
                    break;
                } else if b == 8 || b == 127 {
                    if cmd_len > 0 {
                        cmd_len -= 1;
                        write_both(8);
                        write_both(b' ');
                        write_both(8);
                    }
                } else if b >= 32 && b <= 126 && cmd_len < cmd_buf.len() {
                    cmd_buf[cmd_len] = b;
                    cmd_len += 1;
                    write_both(b);
                }
                unsafe { crate::wdt_feed(); }
            }
        }

        let cmd = core::str::from_utf8(&cmd_buf[..cmd_len]).unwrap_or("");

        if cmd.is_empty() {
            // Empty line = enter insert mode at cursor
            write_str("Insert mode (empty line to stop):\r\n");
            loop {
                write_str("  ");
                let saved_cursor = unsafe { CURSOR };
                unsafe {
                    CURSOR = LINE_COUNT;
                }
                fill_line();
                unsafe {
                    if LINE_LENS[LINE_COUNT - 1] == 0 {
                        // Empty line = exit insert mode
                        // Remove the empty line
                        LINE_COUNT -= 1;
                        CURSOR = saved_cursor;
                        break;
                    }
                }
            }
        } else if cmd.starts_with(':') {
            let arg = &cmd[1..];
            if arg == "l" || arg == "L" {
                show_lines();
            } else if arg == "a" || arg == "A" {
                unsafe { CURSOR = LINE_COUNT; }
                write_str("Append mode (empty line to stop):\r\n");
                loop {
                    write_str("  ");
                    unsafe { CURSOR = LINE_COUNT; }
                    fill_line();
                    unsafe {
                        if LINE_LENS[LINE_COUNT - 1] == 0 {
                            LINE_COUNT -= 1;
                            break;
                        }
                    }
                }
            } else if arg == "d" || arg == "D" {
                delete_line();
            } else if arg == "w" || arg == "W" {
                save_to_sd();
            } else if arg == "q" || arg == "Q" {
                unsafe {
                    if DIRTY {
                        write_str("Unsaved changes! :w to save, :q! to force quit\r\n");
                    } else {
                        break;
                    }
                }
            } else if arg == "q!" {
                break;
            } else if arg == "h" || arg == "H" || arg == "?" {
                write_str("Commands:\r\n");
                write_str("  (empty)  Insert text at cursor\r\n");
                write_str("  :l       List all lines\r\n");
                write_str("  :n       Go to line n\r\n");
                write_str("  :a       Append mode\r\n");
                write_str("  :d       Delete current line\r\n");
                write_str("  :w       Write file to SD\r\n");
                write_str("  :w <p>   Write to path <p>\r\n");
                write_str("  :q       Quit (checks for unsaved)\r\n");
                write_str("  :q!      Force quit\r\n");
            } else if arg.starts_with('w') || arg.starts_with('W') {
                let path_part = arg[1..].trim_start();
                if !path_part.is_empty() {
                    unsafe {
                        let path_bytes = path_part.as_bytes();
                        let copy_len = core::cmp::min(path_bytes.len(), 63);
                        for i in 0..copy_len {
                            EDIT_FILE[i] = path_bytes[i];
                        }
                        EDIT_FILE_LEN = copy_len;
                    }
                    save_to_sd();
                } else {
                    save_to_sd();
                }
            } else if let Some(n) = parse_num(arg) {
                go_to_line(n as usize);
            } else {
                write_str("Unknown command. :h for help\r\n");
            }
        } else {
            write_str("Commands start with ':'. :h for help\r\n");
        }
    }
}
