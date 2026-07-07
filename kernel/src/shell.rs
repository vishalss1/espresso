//! Interactive TTY Shell for Espresso OS.
//! Supports basic line editing, input echo, backspace, and FAT32 commands (ls, cat).

use crate::drivers::uart::RawUart;
use crate::drivers::sd::{list_dir, cat_file, is_mounted};

pub const HISTORY_SIZE: usize = 20;
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
        // Feed WDT periodically
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
            } else if b == 8 || b == 127 { // Backspace
                if input_len > 0 {
                    input_len -= 1;
                    // Visual erase on terminal: backspace, overwrite with space, backspace again
                    uart.write_byte(8);
                    uart.write_byte(b' ');
                    uart.write_byte(8);
                }
            } else if b >= 32 && b <= 126 { // Printable characters
                if input_len < input_buf.len() {
                    input_buf[input_len] = b;
                    input_len += 1;
                    uart.write_byte(b); // Echo back
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
                if is_mounted() {
                    crate::println!("  ls             List root directory files on the SD card");
                    crate::println!("  cat <file>     Display contents of a file on the SD card");
                }
                crate::println!("  help           Show this help message");
            }
            "ls" if is_mounted() => {
                if let Err(e) = list_dir("/") {
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
            _ => {
                crate::println!("Unknown command: '{}'. Type 'help' for help.", cmd);
            }
        }
    }
}