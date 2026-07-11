//! PS/2 keyboard driver — polled decode, scancode Set 1 → ASCII
//!
//! CLK = GPIO34, DATA = GPIO35. Both input-only, active-low, open-drain.
//! PS/2 sends 11-bit frames: start(0) + 8 data(LSB first) + parity(odd) + stop(1).
//! Keyboard drives CLK; data is stable when CLK is low.

pub const PS2_CLK_GPIO: u8 = 34;
pub const PS2_DATA_GPIO: u8 = 35;

// ── Ring buffer for decoded bytes ────────────────────────────────────────────

const KBD_BUF_SIZE: usize = 16;
static mut KBD_BUF: [u8; KBD_BUF_SIZE] = [0; KBD_BUF_SIZE];
static mut KBD_HEAD: usize = 0;
static mut KBD_TAIL: usize = 0;

fn push_byte(b: u8) {
    unsafe {
        let next = (KBD_HEAD + 1) % KBD_BUF_SIZE;
        if next != KBD_TAIL {
            KBD_BUF[KBD_HEAD] = b;
            KBD_HEAD = next;
        }
    }
}

/// Pop a decoded byte from the keyboard buffer. Returns None if empty.
pub fn read_byte() -> Option<u8> {
    unsafe {
        if KBD_HEAD == KBD_TAIL {
            None
        } else {
            let b = KBD_BUF[KBD_TAIL];
            KBD_TAIL = (KBD_TAIL + 1) % KBD_BUF_SIZE;
            Some(b)
        }
    }
}

/// Returns true if there's a byte available.
pub fn has_byte() -> bool {
    unsafe { KBD_HEAD != KBD_TAIL }
}

// ── PS/2 bit-bang state machine ──────────────────────────────────────────────

static mut PREV_CLK: u8 = 1;
static mut SHIFT_REG: u16 = 0;
static mut BIT_COUNT: u8 = 0;

// Modifier state (set by scan code processing, not bit-bang)
static mut MOD_SHIFT_L: bool = false;
static mut MOD_SHIFT_R: bool = false;
static mut MOD_CTRL_L: bool = false;
static mut MOD_ALT: bool = false;

/// Called from the poll loop. Reads CLK+DATA GPIO, detects falling edge,
/// shifts in data bits, and decodes complete bytes.
pub fn poll() {
    unsafe {
        let clk = crate::gpio::gpio_read(PS2_CLK_GPIO);
        let data = crate::gpio::gpio_read(PS2_DATA_GPIO);

        // Detect falling edge: CLK was high, now low
        if PREV_CLK == 1 && clk == 0 {
            if BIT_COUNT == 0 {
                // Expecting start bit (should be 0)
                if data == 0 {
                    SHIFT_REG = 0;
                    BIT_COUNT = 1;
                }
                // else: glitch, stay in idle
            } else if BIT_COUNT <= 8 {
                // Data bits: LSB first
                if data != 0 {
                    SHIFT_REG |= 1 << (BIT_COUNT - 1);
                }
                BIT_COUNT += 1;
            } else if BIT_COUNT == 9 {
                // Parity bit (odd parity) — skip for now, just count
                BIT_COUNT += 1;
            } else if BIT_COUNT == 10 {
                // Stop bit (should be 1)
                if data != 0 {
                    let raw = (SHIFT_REG & 0xFF) as u8;
                    process_scancode(raw);
                }
                // Reset regardless
                BIT_COUNT = 0;
                SHIFT_REG = 0;
            }
        }

        PREV_CLK = clk;
    }
}

// ── Scancode Set 1 → ASCII translation ───────────────────────────────────────

/// Process a complete PS/2 scan code byte. Handles make/break codes,
/// E0-prefixed extended keys, and modifier tracking.
fn process_scancode(raw: u8) {
    unsafe {
    static mut EXTENDED: bool = false;

    if raw == 0xE0 {
        EXTENDED = true;
        return;
    }

    let is_break = (raw & 0x80) != 0;
    let code = if EXTENDED { raw | 0x80 } else { raw & 0x7F };
    EXTENDED = false;

    if is_break {
        match code {
            0x12 => MOD_SHIFT_L = false,
            0x59 => MOD_SHIFT_R = false,
            0x14 => MOD_CTRL_L = false,
            0x11 => MOD_ALT = false,
            _ => {}
        }
        return;
    }

    // Make code
    match code {
        0x12 => { MOD_SHIFT_L = true; return; }
        0x59 => { MOD_SHIFT_R = true; return; }
        0x14 => { MOD_CTRL_L = true; return; }
        0x11 => { MOD_ALT = true; return; }
        0x5A => { push_byte(b'\r'); return; } // Enter
        0x66 => { push_byte(8); return; }      // Backspace
        0x0D => { push_byte(b'\t'); return; }  // Tab
        _ => {}
    }

    let shifted = MOD_SHIFT_L || MOD_SHIFT_R;

    // E0-prefixed keys
    if code & 0x80 != 0 {
        match code {
            0x80 | 0xC8 => { // Up arrow
                push_byte(0x1B);
                push_byte(b'[');
                push_byte(b'A');
            }
            0x81 | 0xD0 => { // Down arrow
                push_byte(0x1B);
                push_byte(b'[');
                push_byte(b'B');
            }
            0x83 | 0xC9 => { // PgUp
                push_byte(0x1B);
                push_byte(b'[');
                push_byte(b'5');
                push_byte(b'~');
            }
            0x86 | 0xD1 => { // PgDn
                push_byte(0x1B);
                push_byte(b'[');
                push_byte(b'6');
                push_byte(b'~');
            }
            _ => {}
        }
        return;
    }

    // Regular keys
    let ch = scancode_to_ascii(code, shifted);
    if ch != 0 {
        push_byte(ch);
    }
    } // unsafe
}

/// PS/2 Set 1 scancode → ASCII lookup. Returns 0 if no mapping.
fn scancode_to_ascii(code: u8, shifted: bool) -> u8 {
    if shifted {
        match code {
            0x01 => b'~',   // ` ~
            0x02 => b'!',   // 1 !
            0x03 => b'@',   // 2 @
            0x04 => b'#',   // 3 #
            0x05 => b'$',   // 4 $
            0x06 => b'%',   // 5 %
            0x07 => b'^',   // 6 ^
            0x08 => b'&',   // 7 &
            0x09 => b'*',   // 8 *
            0x0A => b'(',   // 9 (
            0x0B => b')',   // 0 )
            0x0C => b'_',   // - _
            0x0D => b'+',   // = +
            0x10 => b'Q',
            0x11 => b'W',
            0x12 => b'E',
            0x13 => b'R',
            0x14 => b'T',
            0x15 => b'Y',
            0x16 => b'U',
            0x17 => b'I',
            0x18 => b'O',
            0x19 => b'P',
            0x1A => b'{',   // [ {
            0x1B => b'}',   // ] }
            0x1E => b'A',
            0x1F => b'S',
            0x20 => b'D',
            0x21 => b'F',
            0x22 => b'G',
            0x23 => b'H',
            0x24 => b'J',
            0x25 => b'K',
            0x26 => b'L',
            0x27 => b':',   // ; :
            0x28 => b'"',   // ' "
            0x2B => b'|',   // \ |
            0x2C => b'Z',
            0x2D => b'X',
            0x2E => b'C',
            0x2F => b'V',
            0x30 => b'B',
            0x31 => b'N',
            0x32 => b'M',
            0x33 => b'<',   // , <
            0x34 => b'>',   // . >
            0x35 => b'?',   // / ?
            0x39 => b' ',   // Space
            _ => 0,
        }
    } else {
        match code {
            0x01 => b'`',
            0x02 => b'1',
            0x03 => b'2',
            0x04 => b'3',
            0x05 => b'4',
            0x06 => b'5',
            0x07 => b'6',
            0x08 => b'7',
            0x09 => b'8',
            0x0A => b'9',
            0x0B => b'0',
            0x0C => b'-',
            0x0D => b'=',
            0x10 => b'q',
            0x11 => b'w',
            0x12 => b'e',
            0x13 => b'r',
            0x14 => b't',
            0x15 => b'y',
            0x16 => b'u',
            0x17 => b'i',
            0x18 => b'o',
            0x19 => b'p',
            0x1A => b'[',
            0x1B => b']',
            0x2B => b'\\',
            0x1E => b'a',
            0x1F => b's',
            0x20 => b'd',
            0x21 => b'f',
            0x22 => b'g',
            0x23 => b'h',
            0x24 => b'j',
            0x25 => b'k',
            0x26 => b'l',
            0x27 => b';',
            0x28 => b'\'',
            0x2C => b'z',
            0x2D => b'x',
            0x2E => b'c',
            0x2F => b'v',
            0x30 => b'b',
            0x31 => b'n',
            0x32 => b'm',
            0x33 => b',',
            0x34 => b'.',
            0x35 => b'/',
            0x39 => b' ',
            _ => 0,
        }
    }
}

/// Initialize PS/2 keyboard GPIO pins.
pub fn init() {
    unsafe {
        // GPIO34 and GPIO35 are input-only on ESP32 — no output enable needed.
        // Ensure they're configured for GPIO function (IO_MUX function 0).
        // Set both as inputs (clear output enable, set input enable).
        crate::gpio::gpio_mode(PS2_CLK_GPIO, 0);  // input
        crate::gpio::gpio_mode(PS2_DATA_GPIO, 0); // input

        PREV_CLK = 1;
        SHIFT_REG = 0;
        BIT_COUNT = 0;
        KBD_HEAD = 0;
        KBD_TAIL = 0;
    }
}
