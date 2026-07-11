//! Interactive TTY Shell for Espresso OS.
//! Supports line editing, input echo, backspace, and command dispatch.

use crate::drivers::uart::RawUart;
use crate::drivers::sd::{list_dir, cat_file, is_mounted, touch_file, write_file};

pub const PROMPT: &str = "espresso# ";

pub fn start_shell() {
    let mut input_buf = [0u8; 128];
    let mut input_len = 0;
    let mut wdt_counter = 0u32;

    crate::println!("");
    crate::println!("Welcome to Espresso OS Shell!");
    crate::println!("Type 'help' for a list of available commands.");
    crate::println!("");
    crate::print!("{}", PROMPT);

    let uart = RawUart;

    loop {
        wdt_counter += 1;
        if wdt_counter >= 10000 {
            wdt_counter = 0;
            unsafe { crate::wdt_feed(); }
        }

        if let Some(b) = uart.read_byte() {
            if b == b'\r' || b == b'\n' {
                crate::println!();
                if input_len > 0 {
                    if let Ok(cmd_str) = core::str::from_utf8(&input_buf[..input_len]) {
                        execute_command(cmd_str);
                    }
                    input_len = 0;
                }
                crate::print!("{}", PROMPT);
            } else if b == 8 || b == 127 {
                if input_len > 0 {
                    input_len -= 1;
                    uart.write_byte(8);
                    uart.write_byte(b' ');
                    uart.write_byte(8);
                }
            } else if b >= 32 && b <= 126 {
                if input_len < input_buf.len() {
                    input_buf[input_len] = b;
                    input_len += 1;
                    uart.write_byte(b);
                }
            }
        }
    }
}

fn execute_command(cmd_str: &str) {
    let mut parts = cmd_str.split_whitespace();
    if let Some(cmd) = parts.next() {
        match cmd {
            "help" => {
                crate::println!("Available commands:");
                crate::println!("  ls [path]              List directory files");
                crate::println!("  cat <file>             Display file contents");
                crate::println!("  touch <file>           Create an empty file");
                crate::println!("  echo \"...\" > <file>    Write text to a file");
                crate::println!("  run <path>             Load and run .espr program");
                crate::println!("  kill <pid>             Kill a task");
                crate::println!("  ps                     List running tasks");
                crate::println!("  mem                    Show memory info");
                crate::println!("  gpio <pin> [out|0|1]   GPIO read/write");
                crate::println!("  log                    Show event log");
                crate::println!("  caps <pid>             Show task capabilities");
                crate::println!("  crashlog               Show crash log");
                crate::println!("  reboot                 Reboot device");
                crate::println!("  help                   Show this help message");
            }

            // ── File commands ───────────────────────────────────────────
            "ls" if is_mounted() => {
                let path = parts.next().unwrap_or("/");
                if let Err(e) = list_dir(path) {
                    crate::println!("ls error: {}", e);
                }
            }
            "cat" if is_mounted() => {
                if let Some(filename) = parts.next() {
                    if let Err(e) = cat_file(filename) {
                        crate::println!("cat error: {}", e);
                    }
                } else {
                    crate::println!("Usage: cat <filename>");
                }
            }
            "touch" if is_mounted() => {
                if let Some(filename) = parts.next() {
                    if let Err(e) = touch_file(filename) {
                        crate::println!("touch error: {}", e);
                    }
                } else {
                    crate::println!("Usage: touch <filename>");
                }
            }
            "echo" if is_mounted() => {
                let rest = cmd_str[4..].trim_start();
                if rest.starts_with('"') {
                    if let Some(end_quote) = rest[1..].find('"') {
                        let text = &rest[1..1 + end_quote];
                        let after_quote = rest[2 + end_quote..].trim_start();
                        if after_quote.starts_with('>') {
                            let path = after_quote[1..].trim_start();
                            if !path.is_empty() {
                                if let Err(e) = write_file(path, text.as_bytes()) {
                                    crate::println!("echo error: {}", e);
                                }
                            } else {
                                crate::println!("Usage: echo \"<text>\" > <path>");
                            }
                        } else {
                            crate::println!("Usage: echo \"<text>\" > <path>");
                        }
                    } else {
                        crate::println!("echo: unclosed quote");
                    }
                } else {
                    crate::println!("Usage: echo \"<text>\" > <path>");
                }
            }

            // ── Process commands ────────────────────────────────────────
            "run" => {
                if let Some(path) = parts.next() {
                    if !is_mounted() {
                        crate::println!("run error: no SD card");
                    } else {
                        crate::run_program(path);
                    }
                } else {
                    crate::println!("Usage: run <path>");
                }
            }
            "kill" => {
                if let Some(pid_str) = parts.next() {
                    if let Ok(pid) = parse_u32(pid_str) {
                        match crate::scheduler::kill_task(pid as usize) {
                            Ok(()) => crate::println!("Killed task {}", pid),
                            Err(e) => crate::println!("kill error: {}", e),
                        }
                    } else {
                        crate::println!("Usage: kill <pid>");
                    }
                } else {
                    crate::println!("Usage: kill <pid>");
                }
            }
            "ps" => {
                crate::println!("PID  STATE  STACK");
                unsafe {
                    for task in crate::scheduler::TASKS.iter() {
                        if task.state != crate::scheduler::TaskState::Dead {
                            let state = match task.state {
                                crate::scheduler::TaskState::Running => "RUN",
                                crate::scheduler::TaskState::Ready => "RDY",
                                crate::scheduler::TaskState::Blocked => "BLK",
                                crate::scheduler::TaskState::Dead => "---",
                            };
                            crate::println!(" {:<4} {:<6} {}B", task.pid, state, task.stack_size);
                        }
                    }
                }
            }

            // ── Memory ─────────────────────────────────────────────────
            "mem" => {
                let total = crate::mem::pool::TOTAL_PAGES * crate::mem::pool::PAGE_SIZE;
                let free = crate::mem::pool::free_count() * crate::mem::pool::PAGE_SIZE;
                crate::println!("Exec pool: {}B total, {}B free, {}B used",
                    total, free, total - free);
                let (w0, w1, w2) = crate::mem::pool::bitmap_words();
                crate::println!("bitmap: [{:08X} {:08X} {:08X}]", w0, w1, w2);
            }

            // ── GPIO ───────────────────────────────────────────────────
            "gpio" => {
                if let Some(pin_str) = parts.next() {
                    if let Ok(pin) = parse_u32(pin_str) {
                        if pin > 39 {
                            crate::println!("gpio error: pin must be 0-39");
                        } else {
                            match parts.next() {
                                Some("out") => {
                                    unsafe { crate::gpio::gpio_mode(pin as u8, 1); }
                                    crate::println!("GPIO{} set to output", pin);
                                }
                                Some("in") => {
                                    unsafe { crate::gpio::gpio_mode(pin as u8, 0); }
                                    crate::println!("GPIO{} set to input", pin);
                                }
                                Some("0") => {
                                    unsafe { crate::gpio::gpio_write(pin as u8, 0); }
                                    crate::println!("GPIO{} = LOW", pin);
                                }
                                Some("1") => {
                                    unsafe { crate::gpio::gpio_write(pin as u8, 1); }
                                    crate::println!("GPIO{} = HIGH", pin);
                                }
                                None => {
                                    let val = unsafe { crate::gpio::gpio_read(pin as u8) };
                                    crate::println!("GPIO{} = {}", pin, val);
                                }
                                _ => {
                                    crate::println!("Usage: gpio <pin> [in|out|0|1]");
                                }
                            }
                        }
                    } else {
                        crate::println!("Usage: gpio <pin> [in|out|0|1]");
                    }
                } else {
                    crate::println!("Usage: gpio <pin> [in|out|0|1]");
                }
            }

            // ── Debug / log ────────────────────────────────────────────
            "log" => {
                let mut buf = [0u8; 512];
                let n = crate::event_log::drain_log(&mut buf);
                if n == 0 {
                    crate::println!("(no events)");
                } else {
                    let uart = RawUart;
                    for i in 0..n {
                        uart.write_byte(buf[i]);
                    }
                    crate::println!();
                }
            }
            "caps" => {
                if let Some(pid_str) = parts.next() {
                    if let Ok(pid) = parse_u32(pid_str) {
                        let caps = crate::caps::get_caps(pid as usize);
                        crate::println!("Task {}: caps = 0x{:08X}", pid, caps);
                    } else {
                        crate::println!("Usage: caps <pid>");
                    }
                } else {
                    crate::println!("Usage: caps <pid>");
                }
            }
            "crashlog" => {
                let mut buf = [0u8; 512];
                let n = crate::panic_policy::read_crash_log(&mut buf);
                if n == 0 {
                    crate::println!("(no crash records)");
                } else {
                    let uart = RawUart;
                    for i in 0..n {
                        uart.write_byte(buf[i]);
                    }
                    crate::println!();
                }
            }

            // ── System ─────────────────────────────────────────────────
            "reboot" => {
                crate::println!("Rebooting...");
                crate::panic_policy::reset_system();
            }

            _ => {
                crate::println!("Unknown command: '{}'. Type 'help' for help.", cmd);
            }
        }
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
