//! Interactive TTY Shell for Espresso OS.
//! Supports line editing, input echo, backspace, and command dispatch.
//! Input from UART. Output to both display and UART.

use crate::drivers::sd::{list_dir, cat_file, is_mounted, touch_file, write_file};

pub const PROMPT: &str = "espresso# ";

pub fn start_shell() {
    let mut input_buf = [0u8; 128];
    let mut input_len = 0;
    let mut wdt_counter = 0u32;
    let mut yield_counter = 0u32;

    crate::tty::write_str_both("\n");
    crate::tty::write_str_both("Welcome to Espresso OS Shell!\n");
    crate::tty::write_str_both("Type 'help' for commands.\n\n");
    crate::tty::write_str_both(PROMPT);

    loop {
        wdt_counter += 1;
        if wdt_counter >= 10000 {
            wdt_counter = 0;
            unsafe { crate::wdt_feed(); }
        }

        // Yield to other tasks periodically (round-robin)
        yield_counter += 1;
        if yield_counter >= 200 {
            yield_counter = 0;
            crate::scheduler::scheduler_tick();
        }

        let opt = crate::tty::poll_read();

        if let Some(b) = opt {
            if b == b'\r' || b == b'\n' {
                crate::tty::write_str_both("\r\n");
                if input_len > 0 {
                    if let Ok(cmd_str) = core::str::from_utf8(&input_buf[..input_len]) {
                        execute_command(cmd_str);
                    }
                    input_len = 0;
                }
                crate::tty::write_str_both(PROMPT);
            } else if b == 8 || b == 127 {
                // Backspace
                if input_len > 0 {
                    input_len -= 1;
                    crate::tty::write_both(8);
                    crate::tty::write_both(b' ');
                    crate::tty::write_both(8);
                }
            } else if b >= 32 && b <= 126 {
                if input_len < input_buf.len() {
                    input_buf[input_len] = b;
                    input_len += 1;
                    crate::tty::write_both(b);
                }
            }
        }

        crate::display::flush_display();
    }
}

fn execute_command(cmd_str: &str) {
    let mut parts = cmd_str.split_whitespace();
    if let Some(cmd) = parts.next() {
        match cmd {
            "help" => {
                crate::tty::write_str_both("Available commands:\n");
                crate::tty::write_str_both("  ls [path]              List directory\n");
                crate::tty::write_str_both("  cat <file>             Display file contents\n");
                crate::tty::write_str_both("  touch <file>           Create empty file\n");
                crate::tty::write_str_both("  echo \"...\" > <file>    Write text to file\n");
                crate::tty::write_str_both("  run <path>             Load and run .espr program\n");
                crate::tty::write_str_both("  kill <pid>             Kill a task\n");
                crate::tty::write_str_both("  ps                     List running tasks\n");
                crate::tty::write_str_both("  mem                    Show memory info\n");
                crate::tty::write_str_both("  gpio <pin> [out|0|1]   GPIO read/write\n");
                crate::tty::write_str_both("  log                    Show event log\n");
                crate::tty::write_str_both("  caps <pid>             Show task capabilities\n");
                crate::tty::write_str_both("  crashlog               Show crash log\n");
                crate::tty::write_str_both("  install <pkg>          Install package (hash-verified)\n");
                crate::tty::write_str_both("  update <pkg>           Re-verify and install package\n");
                crate::tty::write_str_both("  packages               List installed packages\n");
                crate::tty::write_str_both("  verify <pkg>           Verify package hash\n");
                crate::tty::write_str_both("  forth                  Enter Forth REPL\n");
                crate::tty::write_str_both("  edit <file>            Line editor\n");
                crate::tty::write_str_both("  modload <path>         Load driver module\n");
                crate::tty::write_str_both("  drivers                List loaded drivers\n");
                crate::tty::write_str_both("  reboot                 Reboot device\n");
                crate::tty::write_str_both("  help                   Show this help\n");
            }

            // ── File commands ───────────────────────────────────────────
            "ls" => {
                if !is_mounted() {
                    crate::tty::write_str_both("ls error: no SD card\n");
                } else {
                    let path = parts.next().unwrap_or("/");
                    if let Err(e) = list_dir(path) {
                        crate::tty::write_str_both("ls error: ");
                        crate::tty::write_str_both(e);
                        crate::tty::write_str_both("\n");
                    }
                }
            }
            "cat" => {
                if !is_mounted() {
                    crate::tty::write_str_both("cat error: no SD card\n");
                } else if let Some(filename) = parts.next() {
                    if let Err(e) = cat_file(filename) {
                        crate::tty::write_str_both("cat error: ");
                        crate::tty::write_str_both(e);
                        crate::tty::write_str_both("\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: cat <filename>\n");
                }
            }
            "touch" => {
                if !is_mounted() {
                    crate::tty::write_str_both("touch error: no SD card\n");
                } else if let Some(filename) = parts.next() {
                    if let Err(e) = touch_file(filename) {
                        crate::tty::write_str_both("touch error: ");
                        crate::tty::write_str_both(e);
                        crate::tty::write_str_both("\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: touch <filename>\n");
                }
            }
            "echo" => {
                if !is_mounted() {
                    crate::tty::write_str_both("echo error: no SD card\n");
                } else {
                    let rest = cmd_str[4..].trim_start();
                    if rest.starts_with('"') {
                        if let Some(end_quote) = rest[1..].find('"') {
                            let text = &rest[1..1 + end_quote];
                            let after_quote = rest[2 + end_quote..].trim_start();
                            if after_quote.starts_with('>') {
                                let path = after_quote[1..].trim_start();
                                if !path.is_empty() {
                                    if let Err(e) = write_file(path, text.as_bytes()) {
                                        crate::tty::write_str_both("echo error: ");
                                        crate::tty::write_str_both(e);
                                        crate::tty::write_str_both("\n");
                                    }
                                } else {
                                    crate::tty::write_str_both("Usage: echo \"<text>\" > <path>\n");
                                }
                            } else {
                                crate::tty::write_str_both("Usage: echo \"<text>\" > <path>\n");
                            }
                        } else {
                            crate::tty::write_str_both("echo: unclosed quote\n");
                        }
                    } else {
                        crate::tty::write_str_both("Usage: echo \"<text>\" > <path>\n");
                    }
                }
            }

            // ── Process commands ────────────────────────────────────────
            "run" => {
                if let Some(path) = parts.next() {
                    if !is_mounted() {
                        crate::tty::write_str_both("run error: no SD card\n");
                    } else {
                        crate::run_program(path);
                    }
                } else {
                    crate::tty::write_str_both("Usage: run <path>\n");
                }
            }
            "kill" => {
                if let Some(pid_str) = parts.next() {
                    if let Ok(pid) = parse_u32(pid_str) {
                        match crate::scheduler::kill_task(pid as usize) {
                            Ok(()) => {
                                crate::tty::write_str_both("Killed task ");
                                crate::tty::write_str_both(pid_str);
                                crate::tty::write_str_both("\n");
                            }
                            Err(e) => {
                                crate::tty::write_str_both("kill error: ");
                                crate::tty::write_str_both(e);
                                crate::tty::write_str_both("\n");
                            }
                        }
                    } else {
                        crate::tty::write_str_both("Usage: kill <pid>\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: kill <pid>\n");
                }
            }
            "ps" => {
                crate::tty::write_str_both("PID  STATE  STACK\n");
                unsafe {
                    for task in crate::scheduler::TASKS.iter() {
                        if task.state != crate::scheduler::TaskState::Dead {
                            let state = match task.state {
                                crate::scheduler::TaskState::Running => "RUN",
                                crate::scheduler::TaskState::Ready => "RDY",
                                crate::scheduler::TaskState::Blocked => "BLK",
                                crate::scheduler::TaskState::Dead => "---",
                            };
                            // Format: " P   STATE  SIZEB\n"
                            let mut line = [0u8; 32];
                            let mut pos = 0;

                            // PID
                            let mut v = task.pid;
                            if v == 0 {
                                line[pos] = b'0'; pos += 1;
                            } else {
                                let mut digits = [0u8; 10];
                                let mut d = 0;
                                while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
                                let mut i = d;
                                while i > 0 { i -= 1; line[pos] = digits[i]; pos += 1; }
                            }
                            // Pad to 5
                            while pos < 5 { line[pos] = b' '; pos += 1; }

                            // State
                            for &b in state.as_bytes() { line[pos] = b; pos += 1; }
                            while pos < 12 { line[pos] = b' '; pos += 1; }

                            // Stack size
                            let mut v = task.stack_size;
                            if v == 0 {
                                line[pos] = b'0'; pos += 1;
                            } else {
                                let mut digits = [0u8; 10];
                                let mut d = 0;
                                while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
                                let mut i = d;
                                while i > 0 { i -= 1; line[pos] = digits[i]; pos += 1; }
                            }
                            while pos < 20 { line[pos] = b' '; pos += 1; }
                            line[pos] = b'B'; pos += 1;
                            line[pos] = b'\n'; pos += 1;

                            for i in 0..pos {
                                crate::tty::write_both(line[i]);
                            }
                        }
                    }
                }
            }

            // ── Memory ─────────────────────────────────────────────────
            "mem" => {
                let total = (crate::mem::pool::TOTAL_PAGES * crate::mem::pool::PAGE_SIZE) as u32;
                let free = (crate::mem::pool::free_count() * crate::mem::pool::PAGE_SIZE) as u32;
                let used = total - free;
                write_u32_dual(total);
                crate::tty::write_str_both("B total, ");
                write_u32_dual(free);
                crate::tty::write_str_both("B free, ");
                write_u32_dual(used);
                crate::tty::write_str_both("B used\n");
                let (w0, w1, w2) = crate::mem::pool::bitmap_words();
                crate::tty::write_str_both("bitmap: [");
                write_hex32_dual(w0);
                crate::tty::write_both(b' ');
                write_hex32_dual(w1);
                crate::tty::write_both(b' ');
                write_hex32_dual(w2);
                crate::tty::write_str_both("]\n");
            }

            // ── GPIO ───────────────────────────────────────────────────
            "gpio" => {
                if let Some(pin_str) = parts.next() {
                    if let Ok(pin) = parse_u32(pin_str) {
                        if pin > 39 {
                            crate::tty::write_str_both("gpio error: pin must be 0-39\n");
                        } else {
                            match parts.next() {
                                Some("out") => {
                                    unsafe { crate::gpio::gpio_mode(pin as u8, 1); }
                                    crate::tty::write_str_both("GPIO set to output\n");
                                }
                                Some("in") => {
                                    unsafe { crate::gpio::gpio_mode(pin as u8, 0); }
                                    crate::tty::write_str_both("GPIO set to input\n");
                                }
                                Some("0") => {
                                    unsafe { crate::gpio::gpio_write(pin as u8, 0); }
                                    crate::tty::write_str_both("GPIO = LOW\n");
                                }
                                Some("1") => {
                                    unsafe { crate::gpio::gpio_write(pin as u8, 1); }
                                    crate::tty::write_str_both("GPIO = HIGH\n");
                                }
                                None => {
                                    let val = unsafe { crate::gpio::gpio_read(pin as u8) };
                                    crate::tty::write_str_both("GPIO = ");
                                    crate::tty::write_both(b'0' + val);
                                    crate::tty::write_str_both("\n");
                                }
                                _ => {
                                    crate::tty::write_str_both("Usage: gpio <pin> [in|out|0|1]\n");
                                }
                            }
                        }
                    } else {
                        crate::tty::write_str_both("Usage: gpio <pin> [in|out|0|1]\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: gpio <pin> [in|out|0|1]\n");
                }
            }

            // ── Debug / log ────────────────────────────────────────────
            "log" => {
                let mut buf = [0u8; 512];
                let n = crate::event_log::drain_log(&mut buf);
                if n == 0 {
                    crate::tty::write_str_both("(no events)\n");
                } else {
                    for i in 0..n {
                        crate::tty::write_both(buf[i]);
                    }
                    crate::tty::write_str_both("\n");
                }
            }
            "caps" => {
                if let Some(pid_str) = parts.next() {
                    if let Ok(pid) = parse_u32(pid_str) {
                        let caps = crate::caps::get_caps(pid as usize);
                        crate::tty::write_str_both("Task caps = 0x");
                        write_hex32_dual(caps);
                        crate::tty::write_str_both("\n");
                    } else {
                        crate::tty::write_str_both("Usage: caps <pid>\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: caps <pid>\n");
                }
            }
            "crashlog" => {
                let mut buf = [0u8; 512];
                let n = crate::panic_policy::read_crash_log(&mut buf);
                if n == 0 {
                    crate::tty::write_str_both("(no crash records)\n");
                } else {
                    for i in 0..n {
                        crate::tty::write_both(buf[i]);
                    }
                    crate::tty::write_str_both("\n");
                }
            }

            // ── Package manager ────────────────────────────────────────
            "install" => {
                if !is_mounted() {
                    crate::tty::write_str_both("install error: no SD card\n");
                } else if let Some(name) = parts.next() {
                    if let Err(e) = crate::pkg::pkg_install(name) {
                        crate::tty::write_str_both("install error: ");
                        crate::tty::write_str_both(e);
                        crate::tty::write_str_both("\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: install <package>\n");
                }
            }
            "update" => {
                if !is_mounted() {
                    crate::tty::write_str_both("update error: no SD card\n");
                } else if let Some(name) = parts.next() {
                    if let Err(e) = crate::pkg::pkg_update(name) {
                        crate::tty::write_str_both("update error: ");
                        crate::tty::write_str_both(e);
                        crate::tty::write_str_both("\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: update <package>\n");
                }
            }
            "packages" => {
                if !is_mounted() {
                    crate::tty::write_str_both("packages error: no SD card\n");
                } else {
                    crate::pkg::pkg_list();
                }
            }
            "verify" => {
                if !is_mounted() {
                    crate::tty::write_str_both("verify error: no SD card\n");
                } else if let Some(name) = parts.next() {
                    if let Err(e) = crate::pkg::pkg_verify(name) {
                        crate::tty::write_str_both("verify error: ");
                        crate::tty::write_str_both(e);
                        crate::tty::write_str_both("\n");
                    }
                } else {
                    crate::tty::write_str_both("Usage: verify <package>\n");
                }
            }

            // ── Driver hot-load ─────────────────────────────────────
            "modload" => {
                if !is_mounted() {
                    crate::tty::write_str_both("modload error: no SD card\n");
                } else if let Some(path) = parts.next() {
                    crate::tty::write_str_both("Loading driver: ");
                    crate::tty::write_str_both(path);
                    crate::tty::write_str_both("\n");
                    match crate::loader::load_from_sd(path) {
                        Ok(prog) => {
                            crate::println!("Loaded '{}' @ 0x{:08X}, entry=0x{:08X}, stack={}B",
                                path, prog.base, prog.entry, prog.stack_size);
                            let name = path.rsplit('/').next().unwrap_or(path);
                            match crate::driver::load_driver(name, &prog) {
                                Ok(slot) => {
                                    crate::tty::write_str_both("Driver registered in slot ");
                                    crate::tty::write_both(b'0' + slot as u8);
                                    crate::tty::write_str_both("\n");
                                    // Call driver_init if entry point is set
                                    if prog.entry != 0 {
                                        unsafe {
                                            let init_fn: extern "C" fn() -> i32 =
                                                core::mem::transmute(prog.entry);
                                            let result = init_fn();
                                            crate::tty::write_str_both("driver_init() returned ");
                                            let mut buf = [0u8; 12];
                                            let s = i32_to_str(result, &mut buf);
                                            crate::tty::write_str_both(s);
                                            crate::tty::write_str_both("\n");
                                        }
                                    }
                                }
                                Err(e) => {
                                    crate::tty::write_str_both("modload error: ");
                                    crate::tty::write_str_both(e);
                                    crate::tty::write_str_both("\n");
                                }
                            }
                        }
                        Err(e) => {
                            crate::tty::write_str_both("modload error: ");
                            crate::tty::write_str_both(&efmt(e));
                            crate::tty::write_str_both("\n");
                        }
                    }
                } else {
                    crate::tty::write_str_both("Usage: modload <path>\n");
                }
            }
            "drivers" => {
                crate::driver::list_drivers();
            }

            // ── Forth ─────────────────────────────────────────────────
            "forth" => {
                crate::forth::run_interpreter();
                crate::tty::write_str_both("Forth exited.\n");
            }

            // ── Editor ────────────────────────────────────────────────
            "edit" => {
                let path = parts.next().unwrap_or("");
                if !is_mounted() {
                    crate::tty::write_str_both("edit error: no SD card\n");
                } else {
                    crate::editor::run_editor(path);
                }
            }

            // ── System ─────────────────────────────────────────────────
            "reboot" => {
                crate::tty::write_str_both("Rebooting...\n");
                crate::panic_policy::reset_system();
            }

            _ => {
                crate::tty::write_str_both("Unknown command: '");
                crate::tty::write_str_both(cmd);
                crate::tty::write_str_both("'. Type 'help' for help.\n");
            }
        }
    }
}

fn efmt(e: crate::loader::LoaderError) -> &'static str {
    match e {
        crate::loader::LoaderError::BadMagic => "ERR_BAD_MAGIC",
        crate::loader::LoaderError::TooLarge => "ERR_TOO_LARGE",
        crate::loader::LoaderError::InvalidEntry => "ERR_INVALID_ENTRY",
        crate::loader::LoaderError::InvalidReloc => "ERR_INVALID_RELOC",
        crate::loader::LoaderError::NoMemory => "ERR_NO_MEMORY",
        crate::loader::LoaderError::ReadError(_) => "ERR_READ_FAILED",
    }
}

fn parse_u32(s: &str) -> Result<u32, ()> {
    let mut val: u32 = 0;
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return Err(());
    }
    for &b in bytes {
        if b < b'0' || b > b'9' {
            return Err(());
        }
        val = val.checked_mul(10).ok_or(())? + ((b - b'0') as u32);
    }
    Ok(val)
}

fn write_u32_dual(mut v: u32) {
    if v == 0 {
        crate::tty::write_both(b'0');
        return;
    }
    let mut digits = [0u8; 10];
    let mut d = 0;
    while v > 0 { digits[d] = b'0' + (v % 10) as u8; v /= 10; d += 1; }
    let mut i = d;
    while i > 0 { i -= 1; crate::tty::write_both(digits[i]); }
}

fn write_hex32_dual(v: u32) {
    const HEX: [u8; 16] = *b"0123456789ABCDEF";
    for shift in (0..32).step_by(4).rev() {
        crate::tty::write_both(HEX[((v >> shift) & 0xF) as usize]);
    }
}

fn i32_to_str(val: i32, buf: &mut [u8]) -> &str {
    let neg = val < 0;
    let mut v = if neg { (-(val as i64)) as u64 } else { val as u64 };
    if v == 0 {
        if neg {
            buf[0] = b'-';
            return "-0";
        } else {
            buf[0] = b'0';
            return "0";
        }
    }
    let mut digits = [0u8; 20];
    let mut d = 0;
    while v > 0 {
        digits[d] = b'0' + (v % 10) as u8;
        v /= 10;
        d += 1;
    }
    let mut pos = 0;
    if neg { buf[pos] = b'-'; pos += 1; }
    let mut i = d;
    while i > 0 {
        i -= 1;
        buf[pos] = digits[i];
        pos += 1;
    }
    core::str::from_utf8(&buf[..pos]).unwrap_or("?")
}
