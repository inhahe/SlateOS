//! PS/2 keyboard driver.
//!
//! Reads scan codes from the i8042 controller (ports 0x60/0x64), translates
//! them from scan code set 1 (the default after i8042 translation) to ASCII
//! characters, and pushes them into a lock-free ring buffer.  A task can
//! read characters via [`try_read_char`] (non-blocking) or [`read_char`]
//! (blocking via scheduler).
//!
//! ## Architecture
//!
//! The keyboard uses IRQ 1, which arrives through the IOAPIC.  The ISR
//! calls [`handle_scancode`] to read the scan code byte and push the
//! resulting character (if any) into the ring buffer.  All ISR-side code
//! uses only atomic operations (no locks).
//!
//! ## Scan code set
//!
//! QEMU's i8042 emulation enables scan code set 2 → set 1 translation by
//! default (controller configuration byte bit 6).  This means the CPU sees
//! scan code set 1, which is what we decode here.
//!
//! ## Thread safety
//!
//! The ring buffer is single-producer (ISR) / multi-consumer (tasks) using
//! atomic head/tail.  Modifier state is maintained atomically.  The module
//! is safe to call from interrupt and task contexts.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};

use crate::port;

// ---------------------------------------------------------------------------
// PS/2 controller ports
// ---------------------------------------------------------------------------

/// Data port — read scan codes, send commands to keyboard.
const DATA_PORT: u16 = 0x60;
/// Status register (read) / command register (write).
const STATUS_PORT: u16 = 0x64;

// Status register bits
/// Output buffer full — data ready to read from port 0x60.
const STATUS_OUTPUT_FULL: u8 = 1 << 0;
/// Input buffer full — controller busy, don't write to 0x60/0x64.
const STATUS_INPUT_FULL: u8 = 1 << 1;

// Controller commands (written to port 0x64)
/// Read the controller configuration byte.
const CMD_READ_CONFIG: u8 = 0x20;
/// Write the controller configuration byte.
const CMD_WRITE_CONFIG: u8 = 0x60;
/// Enable the first PS/2 port (keyboard).
const CMD_ENABLE_PORT1: u8 = 0xAE;
/// Disable the first PS/2 port.
const CMD_DISABLE_PORT1: u8 = 0xAD;
/// Disable the second PS/2 port (mouse).
const CMD_DISABLE_PORT2: u8 = 0xA7;
/// Self-test the controller.
const CMD_SELF_TEST: u8 = 0xAA;
/// Self-test port 1.
const CMD_TEST_PORT1: u8 = 0xAB;

// Keyboard commands (written to port 0x60)
/// Enable scanning (keyboard starts sending scancodes).
const KB_CMD_ENABLE_SCAN: u8 = 0xF4;

// Keyboard responses
/// Command acknowledged.
const KB_ACK: u8 = 0xFA;

// ---------------------------------------------------------------------------
// Ring buffer for input characters
// ---------------------------------------------------------------------------

/// Size of the input character ring buffer (must be a power of two).
const INPUT_BUF_SIZE: usize = 256;
const INPUT_BUF_MASK: usize = INPUT_BUF_SIZE - 1;

/// Character ring buffer.
///
/// Written by the ISR (single producer), read by tasks (consumers).
/// Uses atomic head (write) and tail (read) indices.  Each element
/// is an `AtomicU8` to avoid data races; only valid between tail and head.
static INPUT_BUF: [AtomicU8; INPUT_BUF_SIZE] = {
    // const-init 256 AtomicU8s to 0.
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; INPUT_BUF_SIZE]
};

/// Write index (next slot the ISR will write to).
static INPUT_HEAD: AtomicU32 = AtomicU32::new(0);
/// Read index (next slot a consumer will read from).
static INPUT_TAIL: AtomicU32 = AtomicU32::new(0);

/// When false, the keyboard driver does not echo characters to the console.
///
/// The kshell sets this to false and handles all display output itself,
/// enabling cursor-aware line editing (insert/delete at any position).
static ECHO_ENABLED: AtomicBool = AtomicBool::new(true);

// ---------------------------------------------------------------------------
// Echo ring — rendering deferred out of hard-IRQ context
// ---------------------------------------------------------------------------

/// Size of the echo ring (must be a power of two).
///
/// Sized to match [`INPUT_BUF_SIZE`] deliberately: the echo ring can never
/// need more slots than the input ring it shadows, because a byte is only
/// queued for echo on the same path that admits it to the input ring.
const ECHO_BUF_SIZE: usize = 256;
const ECHO_BUF_MASK: usize = ECHO_BUF_SIZE - 1;

/// Bytes awaiting echo to the console.
///
/// Single-producer (the IRQ 1 handler), single-consumer (the workqueue
/// worker task). Same head/tail discipline as [`INPUT_BUF`].
static ECHO_BUF: [AtomicU8; ECHO_BUF_SIZE] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; ECHO_BUF_SIZE]
};

/// Write index (next slot the ISR will write to).
static ECHO_HEAD: AtomicU32 = AtomicU32::new(0);
/// Read index (next slot the worker will read from).
static ECHO_TAIL: AtomicU32 = AtomicU32::new(0);

/// Whether a drain work item is already queued.
///
/// Coalesces the submissions: a burst of keystrokes enqueues one work item,
/// not one per character. Without this a fast typist (or a key-repeat storm)
/// would exhaust the workqueue's 64-item capacity and start dropping *other*
/// subsystems' work, which is a far worse failure than slow echo.
static ECHO_DRAIN_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Bytes dropped because the echo ring was full, for diagnostics.
///
/// Non-zero means the console could not keep up with the keyboard, which
/// should not happen in practice — it would take the worker being starved for
/// 256 keystrokes. Counted rather than ignored because a silent drop here
/// shows up to the user as randomly missing characters on screen while the
/// input ring still holds the byte, i.e. the shell acts on input that was
/// never displayed.
static ECHO_DROPPED: AtomicU32 = AtomicU32::new(0);

/// Whether the driver has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Modifier key state (maintained atomically)
// ---------------------------------------------------------------------------

static LEFT_SHIFT: AtomicBool = AtomicBool::new(false);
static RIGHT_SHIFT: AtomicBool = AtomicBool::new(false);
static CAPS_LOCK: AtomicBool = AtomicBool::new(false);
static LEFT_CTRL: AtomicBool = AtomicBool::new(false);
static RIGHT_CTRL: AtomicBool = AtomicBool::new(false);
static LEFT_ALT: AtomicBool = AtomicBool::new(false);
static RIGHT_ALT: AtomicBool = AtomicBool::new(false);

/// True if the next scan code byte is part of an extended (0xE0) sequence.
static EXTENDED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the PS/2 keyboard controller and unmask IRQ 1.
///
/// After this call, keypresses generate IRQ 1 and scan codes appear
/// in the ring buffer as ASCII characters.
///
/// # Safety
///
/// - Must be called after IOAPIC and IDT are initialized.
/// - Must be called with interrupts disabled (or at least before the
///   keyboard IRQ can fire).
/// - Called exactly once.
#[allow(clippy::cast_possible_truncation)]
pub unsafe fn init() {
    crate::serial_println!("[keyboard] Initializing PS/2 keyboard...");

    // Forget which keys we believed were held.  The controller reset below
    // discards any release byte that was in flight, so a key held across it
    // would stay marked down forever and its *next* press would be filtered
    // as a hardware auto-repeat and never reach an evdev client.  Clearing
    // here trades a possible missing release (which a client resolves on its
    // next SYN) for a permanently dead key, which it cannot.
    clear_key_state();

    // Disable both PS/2 ports during setup.
    // SAFETY: Standard i8042 commands, always safe during init.
    unsafe {
        controller_cmd(CMD_DISABLE_PORT1);
        controller_cmd(CMD_DISABLE_PORT2);
    }

    // Flush the output buffer (discard any pending data).
    flush_output_buffer();

    // Read, modify, and write the controller configuration byte.
    // We want: port 1 interrupt enabled (bit 0), translation on (bit 6).
    // SAFETY: Standard i8042 config sequence.
    unsafe {
        controller_cmd(CMD_READ_CONFIG);
    }
    let config = wait_read_data();

    // Bit 0: port 1 interrupt enable
    // Bit 1: port 2 interrupt enable (disable — no mouse yet)
    // Bit 4: disable port 1 clock (0 = enable)
    // Bit 5: disable port 2 clock (1 = disable)
    // Bit 6: port 1 translation (1 = set2→set1 translation, keep on)
    let new_config = (config | 0x01 | 0x40) & !0x02;
    // SAFETY: Writing a valid configuration byte.
    unsafe {
        controller_cmd(CMD_WRITE_CONFIG);
        wait_write_data(new_config);
    }

    // Self-test the controller.
    // SAFETY: Standard diagnostic command.
    unsafe {
        controller_cmd(CMD_SELF_TEST);
    }
    let test_result = wait_read_data();
    if test_result != 0x55 {
        crate::serial_println!(
            "[keyboard] WARNING: controller self-test returned {:#x} (expected 0x55)",
            test_result
        );
        // Continue anyway — some controllers fail self-test but work fine.
    }

    // The self-test may reset the config byte, so re-write it.
    // SAFETY: Same config write as above.
    unsafe {
        controller_cmd(CMD_WRITE_CONFIG);
        wait_write_data(new_config);
    }

    // Test port 1 (keyboard port).
    // SAFETY: Standard diagnostic command.
    unsafe {
        controller_cmd(CMD_TEST_PORT1);
    }
    let port_test = wait_read_data();
    if port_test != 0x00 {
        crate::serial_println!(
            "[keyboard] WARNING: port 1 test returned {:#x} (expected 0x00)",
            port_test
        );
    }

    // Enable port 1.
    // SAFETY: Enabling the keyboard port.
    unsafe {
        controller_cmd(CMD_ENABLE_PORT1);
    }

    // Tell the keyboard to start scanning.
    // SAFETY: Standard keyboard command.
    unsafe {
        wait_write_data(KB_CMD_ENABLE_SCAN);
    }
    // Wait for ACK (0xFA).  Discard any other bytes.
    let ack = wait_read_data();
    if ack != KB_ACK {
        crate::serial_println!(
            "[keyboard] WARNING: enable-scan ACK was {:#x} (expected 0xFA)",
            ack
        );
    }

    // Unmask IRQ 1 on the IOAPIC so keyboard interrupts reach the CPU.
    // SAFETY: IOAPIC is initialized, IRQ 1 is the keyboard line.
    unsafe {
        crate::ioapic::unmask_irq(1);
    }

    INITIALIZED.store(true, Ordering::Release);
    crate::serial_println!("[keyboard] PS/2 keyboard initialized (IRQ 1 unmasked)");
}

// ---------------------------------------------------------------------------
// ISR entry point — called from handle_device_irq when IRQ == 1
// ---------------------------------------------------------------------------

/// Which keycodes are currently held down, one bit per Linux keycode.
///
/// The PS/2 controller's own typematic repeat re-sends the make code of a held
/// key several times a second. Those must not reach `/dev/input/event0` as
/// fresh presses: Linux suppresses hardware repeat and synthesises its own,
/// because the repeat delay and rate are a user preference and a client that
/// receives the hardware's cannot retime or disable it. This bitmap is what
/// lets the ISR tell "pressed again" from "still pressed".
///
/// 256 bits covers every keycode either translation table can produce (the
/// highest is `KEY_SEARCH`, 217).
static KEY_DOWN_BITS: [core::sync::atomic::AtomicU64; 4] =
    [const { core::sync::atomic::AtomicU64::new(0) }; 4];

/// Record a key transition and report whether it changed anything.
///
/// Returns `false` for a make code of an already-held key (hardware repeat) so
/// the caller can drop it, and `false` for a break code of a key that was not
/// held — which happens routinely after a focus change or an `0xE0` prefix
/// that was consumed by a reset, and which would otherwise deliver a release
/// with no matching press.
fn note_key_transition(keycode: u16, pressed: bool) -> bool {
    let idx = (keycode >> 6) as usize;
    let bit = 1_u64 << (keycode & 63);
    let Some(word) = KEY_DOWN_BITS.get(idx) else {
        // keycode >= 256; neither table produces one, so this is unreachable.
        // Reporting "changed" would be the riskier direction: it would let an
        // untracked key emit unbounded repeats.
        return false;
    };
    if pressed {
        let prev = word.fetch_or(bit, Ordering::AcqRel);
        prev & bit == 0
    } else {
        let prev = word.fetch_and(!bit, Ordering::AcqRel);
        prev & bit != 0
    }
}

/// Forget all held keys.
///
/// Used when the keyboard is reinitialised: any key held across the reset has
/// a make code the driver never saw and a break code it will, so without this
/// the bitmap would suppress that key's next genuine press.
pub fn clear_key_state() {
    for word in &KEY_DOWN_BITS {
        word.store(0, Ordering::Release);
    }
}

/// Process a keyboard scan code from the ISR.
///
/// Reads the scan code byte from port 0x60, updates modifier state, pushes any
/// resulting ASCII character into the console ring buffer, and publishes the
/// raw transition to `/dev/input/event0`.
///
/// The two consumers are deliberately independent. The console ring carries
/// decoded characters and only for keys that have one, which is what the shell
/// and the TTY layer want; the evdev ring carries every press *and release* as
/// a keycode, which is what a display server needs and what the ASCII path
/// structurally cannot express. Neither is derived from the other, so a change
/// to the keymap cannot silently alter what a compositor sees.
///
/// # Safety note
///
/// This is called from interrupt context.  It uses only atomic operations
/// and port I/O — no locks.
pub fn handle_scancode() {
    if !INITIALIZED.load(Ordering::Acquire) {
        return;
    }

    // Read the scan code byte.  Must be read promptly or the controller
    // won't fire another IRQ.
    //
    // SAFETY: Port 0x60 is the i8042 data port; reading it is always
    // safe when an IRQ fires (the output buffer is guaranteed full).
    let scancode = unsafe { port::inb(DATA_PORT) };

    // Handle the 0xE0 extended prefix.
    if scancode == 0xE0 {
        EXTENDED.store(true, Ordering::Release);
        return;
    }

    let extended = EXTENDED.load(Ordering::Acquire);
    EXTENDED.store(false, Ordering::Release);

    // Bit 7 distinguishes press (0) from release (1).
    let pressed = scancode & 0x80 == 0;
    let code = scancode & 0x7F;

    publish_evdev(code, extended, pressed);

    if extended {
        handle_extended(code, pressed);
    } else {
        handle_normal(code, pressed);
    }
}

/// Translate one scan code to a Linux keycode and publish it to
/// `/dev/input/event0`, unless it is hardware repeat.
///
/// A code with no keycode in either table is dropped entirely rather than
/// reported as `MSC_SCAN` alone. A bare `MSC_SCAN` with no `EV_KEY` is legal
/// evdev, but it is indistinguishable from a key the kernel *does* know and
/// merely failed to map, and a client that acts on it would be acting on a
/// scancode it has no way to interpret. Silence is the honest answer; the
/// scancode is still recoverable from the `MSC_SCAN` of any key that *is*
/// mapped, so no diagnostic ability is lost.
fn publish_evdev(code: u8, extended: bool, pressed: bool) {
    let keycode = if extended {
        crate::evdev::set1_extended_to_keycode(code)
    } else {
        crate::evdev::set1_to_keycode(code)
    };
    let Some(keycode) = keycode else {
        return;
    };
    if !note_key_transition(keycode, pressed) {
        return;
    }
    // The scancode reported is the one that arrived, extended prefix folded
    // into the high byte the way `MSC_SCAN` conventionally carries it, so a
    // client doing its own keymapping can tell E0-48 from 48.
    let raw = if extended {
        0xE000_u16 | u16::from(code)
    } else {
        u16::from(code)
    };
    crate::evdev::push_key(crate::evdev::InputDevice::Keyboard, keycode, raw, pressed);
}

// ---------------------------------------------------------------------------
// Scan code processing
// ---------------------------------------------------------------------------

/// Handle a normal (non-extended) scan code.
fn handle_normal(code: u8, pressed: bool) {
    match code {
        // Modifier keys — update state, no character output.
        0x2A => {
            LEFT_SHIFT.store(pressed, Ordering::Release);
        }
        0x36 => {
            RIGHT_SHIFT.store(pressed, Ordering::Release);
        }
        0x1D => {
            LEFT_CTRL.store(pressed, Ordering::Release);
        }
        0x38 => {
            LEFT_ALT.store(pressed, Ordering::Release);
        }
        0x3A => {
            // Caps Lock toggles on press only.
            if pressed {
                let old = CAPS_LOCK.load(Ordering::Acquire);
                CAPS_LOCK.store(!old, Ordering::Release);
            }
        }
        _ => {
            // Only produce characters on key press, not release.
            if pressed {
                if let Some(ch) = scancode_to_ascii(code) {
                    push_char(ch);
                }
            }
        }
    }
}

/// Handle an extended (0xE0 prefix) scan code.
fn handle_extended(code: u8, pressed: bool) {
    match code {
        // Extended modifier keys.
        0x1D => {
            RIGHT_CTRL.store(pressed, Ordering::Release);
        }
        0x38 => {
            RIGHT_ALT.store(pressed, Ordering::Release);
        }
        _ => {
            // Only produce characters on key press.
            if pressed {
                if let Some(ch) = extended_to_ascii(code) {
                    push_char(ch);
                }
            }
        }
    }
}

/// Convert a scan code set 1 code to an ASCII character.
///
/// Returns `None` for keys that don't produce visible characters
/// (function keys, modifier keys handled elsewhere, etc.).
fn scancode_to_ascii(code: u8) -> Option<u8> {
    let shift = LEFT_SHIFT.load(Ordering::Acquire) || RIGHT_SHIFT.load(Ordering::Acquire);
    let caps = CAPS_LOCK.load(Ordering::Acquire);
    let ctrl = LEFT_CTRL.load(Ordering::Acquire) || RIGHT_CTRL.load(Ordering::Acquire);

    // Determine effective shift state for letters: XOR of shift and caps.
    let upper = shift ^ caps;

    // Scan code set 1 normal key table.
    // Index: scan code (0x02-0x39, plus a few others).
    let ch: u8 = match code {
        // Number row
        0x02 => {
            if shift {
                b'!'
            } else {
                b'1'
            }
        }
        0x03 => {
            if shift {
                b'@'
            } else {
                b'2'
            }
        }
        0x04 => {
            if shift {
                b'#'
            } else {
                b'3'
            }
        }
        0x05 => {
            if shift {
                b'$'
            } else {
                b'4'
            }
        }
        0x06 => {
            if shift {
                b'%'
            } else {
                b'5'
            }
        }
        0x07 => {
            if shift {
                b'^'
            } else {
                b'6'
            }
        }
        0x08 => {
            if shift {
                b'&'
            } else {
                b'7'
            }
        }
        0x09 => {
            if shift {
                b'*'
            } else {
                b'8'
            }
        }
        0x0A => {
            if shift {
                b'('
            } else {
                b'9'
            }
        }
        0x0B => {
            if shift {
                b')'
            } else {
                b'0'
            }
        }
        0x0C => {
            if shift {
                b'_'
            } else {
                b'-'
            }
        }
        0x0D => {
            if shift {
                b'+'
            } else {
                b'='
            }
        }

        0x0E => b'\x08', // Backspace
        0x0F => b'\t',   // Tab
        0x1C => b'\n',   // Enter

        // QWERTY row
        0x10 => {
            if upper {
                b'Q'
            } else {
                b'q'
            }
        }
        0x11 => {
            if upper {
                b'W'
            } else {
                b'w'
            }
        }
        0x12 => {
            if upper {
                b'E'
            } else {
                b'e'
            }
        }
        0x13 => {
            if upper {
                b'R'
            } else {
                b'r'
            }
        }
        0x14 => {
            if upper {
                b'T'
            } else {
                b't'
            }
        }
        0x15 => {
            if upper {
                b'Y'
            } else {
                b'y'
            }
        }
        0x16 => {
            if upper {
                b'U'
            } else {
                b'u'
            }
        }
        0x17 => {
            if upper {
                b'I'
            } else {
                b'i'
            }
        }
        0x18 => {
            if upper {
                b'O'
            } else {
                b'o'
            }
        }
        0x19 => {
            if upper {
                b'P'
            } else {
                b'p'
            }
        }
        0x1A => {
            if shift {
                b'{'
            } else {
                b'['
            }
        }
        0x1B => {
            if shift {
                b'}'
            } else {
                b']'
            }
        }

        // Home row
        0x1E => {
            if upper {
                b'A'
            } else {
                b'a'
            }
        }
        0x1F => {
            if upper {
                b'S'
            } else {
                b's'
            }
        }
        0x20 => {
            if upper {
                b'D'
            } else {
                b'd'
            }
        }
        0x21 => {
            if upper {
                b'F'
            } else {
                b'f'
            }
        }
        0x22 => {
            if upper {
                b'G'
            } else {
                b'g'
            }
        }
        0x23 => {
            if upper {
                b'H'
            } else {
                b'h'
            }
        }
        0x24 => {
            if upper {
                b'J'
            } else {
                b'j'
            }
        }
        0x25 => {
            if upper {
                b'K'
            } else {
                b'k'
            }
        }
        0x26 => {
            if upper {
                b'L'
            } else {
                b'l'
            }
        }
        0x27 => {
            if shift {
                b':'
            } else {
                b';'
            }
        }
        0x28 => {
            if shift {
                b'"'
            } else {
                b'\''
            }
        }
        0x29 => {
            if shift {
                b'~'
            } else {
                b'`'
            }
        }

        0x2B => {
            if shift {
                b'|'
            } else {
                b'\\'
            }
        }

        // Bottom row
        0x2C => {
            if upper {
                b'Z'
            } else {
                b'z'
            }
        }
        0x2D => {
            if upper {
                b'X'
            } else {
                b'x'
            }
        }
        0x2E => {
            if upper {
                b'C'
            } else {
                b'c'
            }
        }
        0x2F => {
            if upper {
                b'V'
            } else {
                b'v'
            }
        }
        0x30 => {
            if upper {
                b'B'
            } else {
                b'b'
            }
        }
        0x31 => {
            if upper {
                b'N'
            } else {
                b'n'
            }
        }
        0x32 => {
            if upper {
                b'M'
            } else {
                b'm'
            }
        }
        0x33 => {
            if shift {
                b'<'
            } else {
                b','
            }
        }
        0x34 => {
            if shift {
                b'>'
            } else {
                b'.'
            }
        }
        0x35 => {
            if shift {
                b'?'
            } else {
                b'/'
            }
        }

        // Space
        0x39 => b' ',

        // Escape
        0x01 => 0x1B, // ESC character

        // Everything else (F-keys, etc.) → no ASCII.
        _ => return None,
    };

    // Ctrl+letter → control character (ASCII 1-26).
    if ctrl {
        match ch {
            b'a'..=b'z' => return Some(ch - b'a' + 1),
            b'A'..=b'Z' => return Some(ch - b'A' + 1),
            _ => {}
        }
    }

    Some(ch)
}

/// Convert an extended (0xE0-prefixed) scan code to ASCII.
///
/// Most extended keys don't produce standard ASCII.  We map arrow keys
/// and a few others to escape sequences or special codes.
/// Special byte codes for extended keys (above ASCII range).
///
/// These are emitted by `extended_to_ascii` for keys that don't map to
/// standard ASCII.  The kshell interprets them for command history
/// (up/down) and cursor movement (left/right).
pub const KEY_UP: u8 = 0x80;
pub const KEY_DOWN: u8 = 0x81;
pub const KEY_LEFT: u8 = 0x82;
pub const KEY_RIGHT: u8 = 0x83;
pub const KEY_HOME: u8 = 0x84;
pub const KEY_END: u8 = 0x85;

fn extended_to_ascii(code: u8) -> Option<u8> {
    match code {
        0x1C => Some(b'\n'),     // Keypad Enter
        0x35 => Some(b'/'),      // Keypad /
        0x53 => Some(0x7F),      // Delete → DEL character
        0x48 => Some(KEY_UP),    // Up arrow
        0x50 => Some(KEY_DOWN),  // Down arrow
        0x4B => Some(KEY_LEFT),  // Left arrow
        0x4D => Some(KEY_RIGHT), // Right arrow
        0x47 => Some(KEY_HOME),  // Home
        0x4F => Some(KEY_END),   // End
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Ring buffer operations
// ---------------------------------------------------------------------------

/// Push a byte into the input ring, with no echo. Returns `false` if the ring
/// was full and the byte was dropped.
///
/// Split out of [`push_char`] so the self-test can stage a byte without also
/// painting it on the framebuffer — and so "did the byte land?" is answerable,
/// which [`push_char`]'s `()` return hides.
fn push_char_raw(ch: u8) -> bool {
    let head = INPUT_HEAD.load(Ordering::Acquire);
    let tail = INPUT_TAIL.load(Ordering::Acquire);

    // Check if buffer is full (head is one slot behind tail after wrap).
    let next_head = head.wrapping_add(1);
    if (next_head & INPUT_BUF_MASK as u32) == (tail & INPUT_BUF_MASK as u32) {
        return false;
    }

    let idx = (head as usize) & INPUT_BUF_MASK;
    INPUT_BUF[idx].store(ch, Ordering::Release);
    INPUT_HEAD.store(next_head, Ordering::Release);
    true
}

/// Push a character into the ring buffer (called from ISR).
///
/// If the buffer is full, the character is silently dropped.
fn push_char(ch: u8) {
    if !push_char_raw(ch) {
        // Buffer full — drop the character, and drop its echo with it. Echoing
        // a byte no reader will ever receive would put a character on screen
        // that the program cannot see.
        return;
    }

    // Echo to the framebuffer console for immediate visual feedback,
    // unless the consumer has disabled echo (e.g., kshell handles its
    // own display for cursor-aware line editing).
    if !ECHO_ENABLED.load(Ordering::Relaxed) {
        return;
    }
    queue_echo(ch);
}

/// Queue one byte for echo, or render it directly if the workqueue is not up.
///
/// Called from hard-IRQ context, so it must not render: see
/// [`drain_echo`] for why the rendering moved to a worker task.
fn queue_echo(ch: u8) {
    match ch {
        0x1B => return, // Don't echo ESC
        // Don't echo extended key codes (arrow keys, home/end) — the
        // kshell handles their visual effect by redrawing the line.
        KEY_UP | KEY_DOWN | KEY_LEFT | KEY_RIGHT | KEY_HOME | KEY_END => return,
        _ => {}
    }

    // Before the worker exists there is nothing to defer to, so echo inline.
    // `keyboard::init` runs ~700 lines of boot ahead of `workqueue::init`, so
    // this window is real, but it is also the one stretch where rendering from
    // an ISR is harmless: there is no userspace, no scheduler-visible latency
    // budget to blow, and nothing else contending for the console lock.
    // `is_running` is monotonic (set once at init and never cleared), so this
    // cannot interleave with the deferred path and reorder output.
    if !crate::workqueue::is_running() {
        render_echo(ch);
        return;
    }

    try_push_echo(ch);

    // Submit only on the false -> true edge; `drain_echo` clears the flag
    // before it drains, so a byte pushed after that store always finds the
    // flag clear and schedules a fresh drain. No byte can be stranded.
    if !ECHO_DRAIN_SCHEDULED.swap(true, Ordering::AcqRel)
        && !crate::workqueue::submit(drain_echo, 0)
    {
        // Queue full: nothing will drain us, so allow the next byte to retry
        // rather than leaving the flag latched and echo dead forever.
        ECHO_DRAIN_SCHEDULED.store(false, Ordering::Release);
    }
}

/// Push one byte into the echo ring. Returns false if the ring was full.
///
/// Pure ring operation: no rendering and no work submission, so the self-test
/// can exercise it without painting the screen.
fn try_push_echo(ch: u8) -> bool {
    let head = ECHO_HEAD.load(Ordering::Acquire);
    let tail = ECHO_TAIL.load(Ordering::Acquire);
    let next_head = head.wrapping_add(1);
    if (next_head & ECHO_BUF_MASK as u32) == (tail & ECHO_BUF_MASK as u32) {
        ECHO_DROPPED.fetch_add(1, Ordering::Relaxed);
        return false;
    }
    let idx = (head as usize) & ECHO_BUF_MASK;
    let Some(slot) = ECHO_BUF.get(idx) else {
        return false;
    };
    slot.store(ch, Ordering::Release);
    ECHO_HEAD.store(next_head, Ordering::Release);
    true
}

/// Pop one byte from the echo ring, or `None` when empty.
fn pop_echo() -> Option<u8> {
    let head = ECHO_HEAD.load(Ordering::Acquire);
    let tail = ECHO_TAIL.load(Ordering::Acquire);
    if (head & ECHO_BUF_MASK as u32) == (tail & ECHO_BUF_MASK as u32) {
        return None;
    }
    let idx = (tail as usize) & ECHO_BUF_MASK;
    let ch = ECHO_BUF.get(idx)?.load(Ordering::Acquire);
    ECHO_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    Some(ch)
}

/// Render one echoed byte to the console. Task context only.
fn render_echo(ch: u8) {
    if ch == b'\x08' {
        // Backspace: erase the previous glyph (backspace, space, backspace).
        // Consumers that drive cursor-aware editing themselves (e.g. kshell)
        // run with echo disabled, so this only affects the default echo-on
        // path (the canonical TTY line discipline), where it gives the
        // expected visual erase.
        crate::console::putchar(b'\x08');
        crate::console::putchar(b' ');
        crate::console::putchar(b'\x08');
    } else {
        crate::console::putchar(ch);
    }
}

/// Workqueue handler: render every byte the ISR queued for echo.
///
/// Why echo is deferred at all
/// ---------------------------
/// `console::putchar` is the full console pipeline — escape-sequence state
/// machine, glyph blit, and on the last column a whole-screen scroll, which
/// at 1024x768x32bpp is a ~3 MiB `memmove` plus a scrollback `Vec::push` that
/// can reach the heap allocator. Running that from the IRQ 1 handler put a
/// millisecond-scale operation inside a hard interrupt, against CLAUDE.md's
/// 10 us total-ISR-latency budget, and stalled the timer tick and every other
/// device on any keystroke that landed on the bottom line.
///
/// Why a workqueue and not a softirq
/// ---------------------------------
/// A softirq runs on the interrupted task's kernel stack, so it cannot block
/// on a lock that task might already hold — `softirq.rs` requires handlers to
/// use `try_lock`. Echo needs the console lock unconditionally, so a softirq
/// would only convert the ISR deadlock into a softirq deadlock. The workqueue
/// runs in a real task context where taking the lock is legal. This is also
/// what Linux does: `tty_flip_buffer_push` defers to `flush_to_ldisc` on a
/// workqueue rather than to a softirq.
fn drain_echo(_arg: u64) {
    // Clear before draining, not after. A producer that pushes after this
    // store observes the flag clear and submits a fresh work item; the worst
    // case is one redundant drain that finds the ring already empty, whereas
    // clearing afterwards could strand a byte until the next keystroke.
    ECHO_DRAIN_SCHEDULED.store(false, Ordering::Release);
    while let Some(ch) = pop_echo() {
        render_echo(ch);
    }
}

/// Try to read one character from the ring buffer without blocking.
///
/// Returns `Some(ch)` if a character is available, `None` if the buffer
/// is empty.
///
/// Once [`start_usb_hid_poller`] has run this is a pure ring read, which is
/// what finally makes it mean what its name says on USB hardware. It used to
/// poll the device itself, and that made a readiness check unreliable in a way
/// no caller could see: one poll fetches at most one report, so a program
/// spinning on this function saw a key only if a report happened to be sitting
/// in the event ring at that instant, and never saw one typed a moment
/// earlier while nothing was calling.
pub fn try_read_char() -> Option<u8> {
    poll_usb_keyboard_if_unpolled();

    try_read_char_raw()
}

/// Read from the ring buffer without polling USB.
///
/// Used internally to avoid recursion when the USB poll itself
/// pushes characters via `push_char`.
fn try_read_char_raw() -> Option<u8> {
    let head = INPUT_HEAD.load(Ordering::Acquire);
    let tail = INPUT_TAIL.load(Ordering::Acquire);

    if (head & INPUT_BUF_MASK as u32) == (tail & INPUT_BUF_MASK as u32) && head == tail {
        return None; // Empty.
    }

    let idx = (tail as usize) & INPUT_BUF_MASK;
    let ch = INPUT_BUF[idx].load(Ordering::Acquire);
    INPUT_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    Some(ch)
}

/// How a blocking console read ended.
///
/// Three outcomes rather than `Option<u8>` because a signal-interrupted read
/// and a timed-out read must reach userspace as different things — `EINTR`
/// versus a short read of zero bytes — and collapsing them into `None` is
/// exactly the information loss that made `sys_console_read_char`
/// uninterruptible in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOutcome {
    /// A character was read.
    Byte(u8),
    /// A signal is deliverable to the calling process; the read must unwind
    /// so the signal can run.  No character was consumed.
    Interrupted,
    /// The deadline passed with no character available.
    TimedOut,
}

/// The one blocking-read loop, shared by all four public entry points.
///
/// `deadline_ns = None` blocks indefinitely; `Some(t)` gives up once
/// [`crate::hrtimer::now_ns`] reaches `t`.
///
/// `pid` is the *user* process on whose behalf we are reading, or `0` for a
/// kernel task.  It selects whether the loop is interruptible at all:
/// `deliverable_signal_pending(0)` is unconditionally `false`, so passing `0`
/// reproduces the historical uninterruptible behaviour exactly, with no
/// second copy of this loop to keep in sync.
///
/// **Why this is still a `HLT` poll and not a real park.**  The reason it was
/// wrong *before* is now gone: [`start_usb_hid_poller`] drives HID polling
/// from a timer, so a reader that stopped spinning no longer takes the USB
/// keyboard down with it.  That was the blocking half of
/// `known-issues.md` → `BUG-CONSOLE-READ-UNINTERRUPTIBLE` stage 2, and it is
/// unblocked.
///
/// What stage 2 still needs is the wake side, which is a separate piece of
/// work and is not done here: the poller pushes into the ring but wakes
/// nobody, so a genuinely parked reader would sleep through its own keystroke.
/// Converting this loop to `park_interruptible` therefore requires
/// `push_char` to wake the waiting reader — with the ISR-safe idiom
/// (`sched::try_wake`, falling back to `sched::defer_wake`), not
/// `WaitQueue::try_wake_one`, which loses the wake on a lost `try_lock`.
/// Until then the `HLT` spin is what makes the ring get looked at.
///
/// [`usb_hid_poller_armed`] is the precondition to check before making that
/// change; do not assume it, because the poller does not arm on a machine
/// with no xHCI controller.
fn read_char_inner(deadline_ns: Option<u64>, pid: u64) -> ReadOutcome {
    loop {
        // A no-op once the periodic poller is armed; the fallback for when it
        // is not.  See `poll_usb_keyboard_if_unpolled`.
        poll_usb_keyboard_if_unpolled();

        if let Some(ch) = try_read_char_raw() {
            return ReadOutcome::Byte(ch);
        }

        // Check for a deliverable signal *before* the deadline, so a process
        // that is both signalled and timed out reports EINTR — the signal is
        // the more informative of the two, and POSIX lets either win.
        if crate::ipc::waiters::deliverable_signal_pending(pid) {
            return ReadOutcome::Interrupted;
        }

        if let Some(deadline) = deadline_ns
            && crate::hrtimer::now_ns() >= deadline
        {
            return ReadOutcome::TimedOut;
        }

        // Yield CPU until next interrupt (the keyboard IRQ or the periodic
        // timer tick, which bounds how long we sleep past a deadline and how
        // long a signal waits to be noticed).
        crate::cpu::hlt();
    }
}

/// Read one character, blocking if the buffer is empty.
///
/// This spins in a loop yielding the CPU (via HLT) until a character
/// becomes available.  Polls both PS/2 (interrupt-driven) and USB HID
/// (polled) keyboard inputs.
///
/// **Not interruptible by signals** — it is the kernel-task entry point
/// (kshell and the boot console), which have no signal context to check.
/// A userspace read must go through [`read_char_interruptible`] instead.
pub fn read_char() -> u8 {
    match read_char_inner(None, 0) {
        ReadOutcome::Byte(ch) => ch,
        // Unreachable: with `deadline_ns = None` there is no timeout, and
        // `deliverable_signal_pending(0)` is always false for a kernel task.
        // Returning NUL rather than panicking keeps a kernel bug from
        // becoming a kernel panic on the console read path.
        ReadOutcome::Interrupted | ReadOutcome::TimedOut => 0,
    }
}

/// Read one character on behalf of user process `pid`, blocking until either
/// a character arrives or a signal becomes deliverable to `pid`.
///
/// This is the entry point for `sys_console_read_char` and the TTY layer:
/// unlike [`read_char`] it unwinds with [`ReadOutcome::Interrupted`] so the
/// caller can return `EINTR` and let the signal run.  Passing `pid == 0`
/// degrades to the uninterruptible behaviour of [`read_char`].
pub fn read_char_interruptible(pid: u64) -> ReadOutcome {
    read_char_inner(None, pid)
}

/// Read one character, blocking until either a character is available or the
/// monotonic clock reaches `deadline_ns` (an [`crate::hrtimer::now_ns`]
/// timestamp).  Returns `Some(ch)` on input, `None` on timeout.
///
/// Like [`read_char`] this yields the CPU via `HLT` between polls (waking on
/// the keyboard IRQ or the timer tick), so it does not hot-spin.  It is the
/// primitive behind the terminal `VTIME` read timeout: a `VMIN=0,VTIME>0`
/// bounded read and the inter-byte timer of a `VMIN>0,VTIME>0` read.
///
/// A `deadline_ns` already in the past returns immediately — `Some(ch)` if a
/// character happens to be buffered, else `None` — so callers can use it as a
/// non-blocking poll with `deadline_ns = now`.
///
/// **Not interruptible by signals**; see [`read_char_timeout_interruptible`].
pub fn read_char_timeout(deadline_ns: u64) -> Option<u8> {
    match read_char_inner(Some(deadline_ns), 0) {
        ReadOutcome::Byte(ch) => Some(ch),
        ReadOutcome::Interrupted | ReadOutcome::TimedOut => None,
    }
}

/// [`read_char_timeout`] on behalf of user process `pid`, distinguishing a
/// signal ([`ReadOutcome::Interrupted`]) from the deadline expiring
/// ([`ReadOutcome::TimedOut`]).
pub fn read_char_timeout_interruptible(deadline_ns: u64, pid: u64) -> ReadOutcome {
    read_char_inner(Some(deadline_ns), pid)
}

/// Enable or disable keyboard echo.
///
/// When echo is disabled, the keyboard driver pushes characters into the
/// ring buffer but does not print them to the console.  The consumer
/// (e.g., kshell) is responsible for all display output, enabling
/// cursor-aware line editing.
pub fn set_echo(enabled: bool) {
    ECHO_ENABLED.store(enabled, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// i8042 controller helpers
// ---------------------------------------------------------------------------

/// Send a command byte to the controller (port 0x64).
///
/// Waits for the input buffer to be clear before writing.
///
/// # Safety
///
/// The command must be a valid i8042 controller command.
unsafe fn controller_cmd(cmd: u8) {
    wait_input_clear();
    // SAFETY: Caller guarantees cmd is valid.
    unsafe {
        port::outb(STATUS_PORT, cmd);
    }
}

/// Write a data byte to port 0x60.
///
/// Waits for the input buffer to be clear before writing.
///
/// # Safety
///
/// Must only be called when the controller expects a data byte
/// (after a command that takes a parameter, or as a keyboard command).
unsafe fn wait_write_data(data: u8) {
    wait_input_clear();
    // SAFETY: Caller guarantees the controller is expecting data.
    unsafe {
        port::outb(DATA_PORT, data);
    }
}

/// Read a data byte from port 0x60.
///
/// Waits (with timeout) for the output buffer to become full.
fn wait_read_data() -> u8 {
    // Timeout: ~100ms (10_000 iterations of port reads at ~10us each).
    for _ in 0..10_000u32 {
        // SAFETY: Reading status port is always safe.
        let status = unsafe { port::inb(STATUS_PORT) };
        if status & STATUS_OUTPUT_FULL != 0 {
            // SAFETY: Output buffer is full, data is available.
            return unsafe { port::inb(DATA_PORT) };
        }
    }
    // Timeout — return 0 (no data).
    0
}

/// Wait for the controller's input buffer to be clear.
///
/// The controller drops writes if the input buffer is full.
fn wait_input_clear() {
    for _ in 0..10_000u32 {
        // SAFETY: Reading status port is always safe.
        let status = unsafe { port::inb(STATUS_PORT) };
        if status & STATUS_INPUT_FULL == 0 {
            return;
        }
    }
}

/// Discard any pending data in the controller's output buffer.
fn flush_output_buffer() {
    for _ in 0..64u32 {
        // SAFETY: Reading status/data ports is safe.
        let status = unsafe { port::inb(STATUS_PORT) };
        if status & STATUS_OUTPUT_FULL == 0 {
            break;
        }
        let _ = unsafe { port::inb(DATA_PORT) };
    }
}

// ---------------------------------------------------------------------------
// USB HID keyboard integration
// ---------------------------------------------------------------------------

/// USB HID modifier bitmask constants (boot protocol report byte 0).
const USB_MOD_LEFT_CTRL: u8 = 1 << 0;
const USB_MOD_LEFT_SHIFT: u8 = 1 << 1;
const USB_MOD_LEFT_ALT: u8 = 1 << 2;
#[allow(dead_code)]
const USB_MOD_LEFT_GUI: u8 = 1 << 3;
const USB_MOD_RIGHT_CTRL: u8 = 1 << 4;
const USB_MOD_RIGHT_SHIFT: u8 = 1 << 5;
const USB_MOD_RIGHT_ALT: u8 = 1 << 6;
#[allow(dead_code)]
const USB_MOD_RIGHT_GUI: u8 = 1 << 7;

/// Previous USB HID keyboard report state for detecting press/release.
///
/// USB HID boot protocol sends a full snapshot of pressed keys each
/// report.  To detect individual key presses and releases, we compare
/// each report against the previous one.
#[allow(clippy::declare_interior_mutable_const)]
static USB_PREV_KEYCODES: [AtomicU8; 6] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; 6]
};
static USB_PREV_MODIFIERS: AtomicU8 = AtomicU8::new(0);

/// Process a USB HID boot protocol keyboard report.
///
/// Detects newly-pressed keys by comparing against the previous report,
/// converts them to PS/2 scan codes via the xHCI HID-to-scancode table,
/// and feeds the resulting characters into the shared ring buffer.
///
/// This allows USB keyboards to work identically to PS/2 keyboards
/// from the kshell's perspective.
///
/// # Arguments
///
/// * `modifiers` — HID modifier bitmask (byte 0 of boot report)
/// * `keycodes` — six keycode slots (bytes 2-7 of boot report)
pub fn handle_usb_hid_report(modifiers: u8, keycodes: [u8; 6]) {
    // Update modifier state from HID modifier byte.
    let prev_mods = USB_PREV_MODIFIERS.swap(modifiers, Ordering::AcqRel);
    update_usb_modifiers(modifiers, prev_mods);

    // Load previous keycodes.
    let mut prev = [0u8; 6];
    for (slot, prev_slot) in USB_PREV_KEYCODES.iter().zip(prev.iter_mut()) {
        *prev_slot = slot.load(Ordering::Acquire);
    }

    // Detect released keys (in prev but not in new) — used to clear
    // modifier/state if needed; no character output for releases.
    // (Modifier releases are already handled above via the modifier byte.)

    // Detect newly pressed keys (in new but not in prev).
    for &keycode in &keycodes {
        if keycode == 0 || keycode == 1 {
            // 0 = no key, 1 = error rollover (phantom keys).
            continue;
        }
        // Check if this key was already pressed in the previous report.
        let was_pressed = prev.contains(&keycode);
        if !was_pressed {
            // New key press — convert to PS/2 scan code and process.
            if let Some(scancode) = usb_hid_to_scancode(keycode) {
                // Feed through the existing PS/2 scan code → ASCII pipeline.
                handle_usb_scancode(scancode, modifiers);
            }
        }
    }

    // Store current keycodes as previous for next comparison.
    for (slot, &kc) in USB_PREV_KEYCODES.iter().zip(keycodes.iter()) {
        slot.store(kc, Ordering::Release);
    }
}

/// Update atomic modifier state from USB HID modifier bitmask changes.
fn update_usb_modifiers(current: u8, _prev: u8) {
    // USB HID modifier byte gives us the complete modifier state each
    // report.  We update the global atomic modifier booleans directly
    // (shared with PS/2 path).
    LEFT_SHIFT.store(current & USB_MOD_LEFT_SHIFT != 0, Ordering::Release);
    RIGHT_SHIFT.store(current & USB_MOD_RIGHT_SHIFT != 0, Ordering::Release);
    LEFT_CTRL.store(current & USB_MOD_LEFT_CTRL != 0, Ordering::Release);
    RIGHT_CTRL.store(current & USB_MOD_RIGHT_CTRL != 0, Ordering::Release);
    LEFT_ALT.store(current & USB_MOD_LEFT_ALT != 0, Ordering::Release);
    RIGHT_ALT.store(current & USB_MOD_RIGHT_ALT != 0, Ordering::Release);
}

/// Convert a USB HID usage code to a PS/2 scan code set 1 value.
///
/// Returns None for unmapped or reserved HID usage codes.
fn usb_hid_to_scancode(hid_usage: u8) -> Option<u8> {
    // Use the xhci module's HID_TO_SCANCODE table via the public API.
    // Since we're in the same kernel, we can call it directly.
    let report = crate::xhci::HidKeyboardReport {
        modifiers: 0,
        reserved: 0,
        keycodes: [hid_usage, 0, 0, 0, 0, 0],
    };
    crate::xhci::hid_report_to_scancode(&report)
}

/// Process a PS/2 scan code generated from a USB HID keycode.
///
/// Uses the current modifier state (already updated from the HID
/// modifier byte) to translate the scan code to an ASCII character
/// and push it into the ring buffer.
fn handle_usb_scancode(scancode: u8, _hid_modifiers: u8) {
    // Handle Caps Lock toggle (HID usage 0x39 → PS/2 0x3A).
    if scancode == 0x3A {
        let old = CAPS_LOCK.load(Ordering::Acquire);
        CAPS_LOCK.store(!old, Ordering::Release);
        return;
    }

    // Convert to ASCII using the existing PS/2 scan code table.
    // Modifier state has already been updated from the HID modifier byte.
    if let Some(ch) = scancode_to_ascii(scancode) {
        push_char(ch);
    } else if let Some(ch) = extended_to_ascii(scancode) {
        // Some HID keys (arrows, home, end, delete) map to "extended"
        // PS/2 scan codes that produce special key constants.
        push_char(ch);
    }
}

/// Poll the USB keyboard for input (called from the main keyboard poll path).
///
/// This non-blocking check reads any pending USB HID keyboard reports
/// and processes them into the ring buffer.
pub fn poll_usb_keyboard() {
    if let Some(report) = crate::xhci::poll_keyboard() {
        handle_usb_hid_report(report.modifiers, report.keycodes);
    }
}

// ---------------------------------------------------------------------------
// Periodic HID polling
// ---------------------------------------------------------------------------

/// How often the HID poller runs, in nanoseconds.
///
/// 8 ms is the `bInterval` a USB boot keyboard advertises for its interrupt
/// endpoint — the rate the device itself asks to be asked at — so polling
/// faster buys nothing but wasted event-ring reads, and polling slower starts
/// to be felt: at 16 ms a fast typist can outrun the poller within a single
/// report window, and the 6-key boot report has no room to queue.
///
/// This is a *floor* on latency, not a cap on throughput. Each tick drains
/// whatever the ring holds, so a burst typed between two ticks is not lost,
/// merely delivered together.
const USB_HID_POLL_INTERVAL_NS: u64 = 8_000_000;

/// Whether the periodic poller is actually running.
///
/// Read by the console read paths to decide whether they still have to poll
/// the device themselves. This is not belt-and-braces: [`crate::hrtimer`]
/// refuses to schedule past a hard per-CPU ceiling and returns a handle that
/// never fires, so "the timer was requested" and "the timer runs" are
/// genuinely different facts. If the poller did not arm, a reader that stopped
/// polling would make a USB keyboard permanently dead rather than merely
/// laggy — so the fallback stays, gated on the truth rather than on the
/// attempt.
static USB_HID_POLLER_ARMED: AtomicBool = AtomicBool::new(false);

/// The periodic poll itself, called from the APIC timer ISR.
///
/// Everything downstream of here is already ISR-shaped, because IRQ 1 has
/// always driven exactly this path for PS/2: `handle_usb_hid_report` touches
/// nothing but atomics, and `push_char` defers its echo to a worker rather
/// than rendering inline. The one thing that is *not* safe from an ISR is
/// waiting for the controller, which is why this calls
/// [`crate::xhci::try_poll_keyboard`] and not `poll_keyboard`.
fn usb_hid_tick(_arg: u64) {
    if let Some(report) = crate::xhci::try_poll_keyboard() {
        handle_usb_hid_report(report.modifiers, report.keycodes);
    }
}

/// Arm the periodic USB HID poller.
///
/// Call once, after both [`crate::hrtimer::init`] and [`crate::xhci::init`].
/// Idempotent — a second call is a no-op rather than a second timer, so a
/// `rescan` that discovers a keyboard later cannot end up with two.
///
/// **Why this exists at all.** Until 2026-08-21 the only thing that ever
/// fetched a USB HID report was a console read, from inside its own poll loop.
/// That made the device visible only to a caller already blocked waiting for
/// it, which is the one situation where polling is redundant. Everything else
/// — type-ahead while the system is busy, a non-blocking readiness check, a
/// hotkey, Ctrl-C to a program that is not reading stdin — saw nothing,
/// because nothing asked. PS/2 never had the problem (IRQ 1 pushes into the
/// same ring), and QEMU's default keyboard is PS/2, which is why every boot
/// test to date exercised only the working half. See `known-issues.md` →
/// `A-USB-KEYSTROKES-ARE-ONLY-FETCHED-WHILE-SOMEBODY-IS-BLOCKED-READING`.
pub fn start_usb_hid_poller() {
    if USB_HID_POLLER_ARMED.load(Ordering::Acquire) {
        return;
    }
    // Arming when no controller is present would be harmless — `try_poll_keyboard`
    // returns `None` on a `None` controller — but it would also be a lie to the
    // read paths, which use ARMED to decide they no longer need to poll.  With
    // no xHCI there is nothing to poll either way; keeping the flag false costs
    // one atomic load per read and keeps the two facts aligned.
    if !crate::xhci::is_available() {
        crate::serial_println!("[keyboard] No xHCI controller; USB HID poller not started");
        return;
    }

    // Fire the first tick a full period out rather than immediately: this runs
    // during boot, and a callback that fires inside `schedule_repeating` would
    // reach into the controller before the caller's own initialisation has
    // finished settling.
    let _handle = crate::hrtimer::schedule_repeating(
        USB_HID_POLL_INTERVAL_NS,
        USB_HID_POLL_INTERVAL_NS,
        usb_hid_tick,
        0,
    );

    // The handle is deliberately dropped: this timer runs for the life of the
    // system and there is no caller who could ever want to cancel it.  Losing
    // the handle is therefore not a leak of anything cancellable.
    USB_HID_POLLER_ARMED.store(true, Ordering::Release);
    crate::serial_println!(
        "[keyboard] USB HID poller armed ({} ms period)",
        USB_HID_POLL_INTERVAL_NS / 1_000_000
    );
}

/// Poll the USB keyboard from a read path, but only if nothing else does.
///
/// With the periodic poller running this is a single relaxed load, and the
/// read paths become symmetric with the PS/2 path: they read the ring and
/// nothing else. Without it — no xHCI, or an hrtimer that refused the
/// schedule — the old inline poll is still the only thing keeping a USB
/// keyboard alive, so it stays.
fn poll_usb_keyboard_if_unpolled() {
    if USB_HID_POLLER_ARMED.load(Ordering::Relaxed) {
        return;
    }
    poll_usb_keyboard();
}

/// Whether the periodic USB HID poller is running.
///
/// Exposed for the self-test and for whoever implements stage 2 of
/// `BUG-CONSOLE-READ-UNINTERRUPTIBLE`: a blocking console read may only be
/// converted from a `HLT` spin into a real park once this is `true`, because a
/// parked task does not spin and so cannot poll for its own wakeup.
#[must_use]
pub fn usb_hid_poller_armed() -> bool {
    USB_HID_POLLER_ARMED.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify keyboard initialization by checking state.
pub fn self_test() -> Result<(), &'static str> {
    crate::serial_println!("[keyboard] Running self-test...");

    if !INITIALIZED.load(Ordering::Acquire) {
        return Err("keyboard not initialized");
    }

    // Verify the ring buffer starts empty.
    let head = INPUT_HEAD.load(Ordering::Acquire);
    let tail = INPUT_TAIL.load(Ordering::Acquire);
    crate::serial_println!(
        "[keyboard]   Ring buffer: head={}, tail={} ({})",
        head,
        tail,
        if head == tail {
            "empty, OK"
        } else {
            "non-empty"
        }
    );

    echo_ring_self_test()?;
    read_outcome_self_test()?;
    usb_hid_poller_self_test()?;

    crate::serial_println!("[keyboard] Self-test PASSED");
    Ok(())
}

/// Check that USB HID input does not depend on somebody being blocked on it.
///
/// This cannot press a key, so it does not try to. What it *can* check is the
/// property whose absence was the bug: that a poller exists at all whenever
/// there is a USB keyboard to poll. The old defect would sail through any test
/// that merely read a character, because a blocking read polls the device on
/// its own — which is exactly why it went unnoticed for as long as it did.
///
/// The boot test attaches `-device qemu-xhci -device usb-kbd`, so the
/// interesting branch is the one that runs in CI; the `else` arms cover a
/// machine without USB and are not failures.
fn usb_hid_poller_self_test() -> Result<(), &'static str> {
    let has_ctrl = crate::xhci::is_available();
    let has_kbd = has_ctrl && crate::xhci::has_keyboard();
    let armed = usb_hid_poller_armed();

    if has_kbd && !armed {
        crate::serial_println!(
            "[keyboard]   FAIL: a USB keyboard is enumerated but no HID poller is armed — \
             keystrokes will only be seen while something is already blocked reading"
        );
        return Err("USB keyboard present but HID poller not armed");
    }
    if armed && !has_ctrl {
        // Would mean ARMED and the controller disagree, and the read paths
        // trust ARMED to decide they need not poll.
        crate::serial_println!("[keyboard]   FAIL: HID poller armed with no xHCI controller");
        return Err("HID poller armed without a controller");
    }

    // Run one tick synchronously.  This exercises the whole ISR path --
    // `try_lock`, endpoint drain, re-post, `handle_usb_hid_report` -- not just
    // the lock acquisition, so a `try_lock` that mishandles the uncontended
    // case, or a re-post that kills the endpoint, shows up here rather than as
    // a keyboard that wedges after its first keypress on real hardware.
    //
    // Calling the tick rather than discarding `try_poll_keyboard()` also means
    // a report that happens to be waiting is *delivered* instead of dropped: a
    // self-test has no business eating a keystroke, and this runs late enough
    // in boot that there could be one.
    usb_hid_tick(0);

    crate::serial_println!(
        "[keyboard]   USB HID poller: {} (xhci {}, keyboard {}): OK",
        if armed { "armed" } else { "not needed" },
        if has_ctrl { "present" } else { "absent" },
        if has_kbd { "present" } else { "absent" },
    );
    Ok(())
}

/// Exercise the three exits of [`read_char_inner`] without needing a keypress.
///
/// The point is the *shared loop*: all four public read entry points funnel
/// through one body, so the way to be sure `read_char_timeout` still returns
/// `None` on a lapsed deadline — and that the added signal check did not turn
/// a timeout into an early return or vice versa — is to drive the body itself
/// at each of its three exits.
///
/// A signal-interrupted exit cannot be provoked from here: it needs a real
/// user process to own a pending signal, and this runs on a kernel task
/// (`pid == 0`), for which `deliverable_signal_pending` is false by
/// construction. What *is* checked here is the property that makes the
/// pid-0 path safe — that a kernel-task read never reports `Interrupted` —
/// which is the invariant [`read_char`]'s unreachable arm depends on.
/// End-to-end EINTR delivery belongs in a ring-3 Path-Z rung.
fn read_outcome_self_test() -> Result<(), &'static str> {
    // Interrupts masked so a keystroke arriving mid-test cannot satisfy a
    // read that is supposed to time out. Same reasoning as the echo ring:
    // the producer under test is IRQ 1.
    crate::cpu::without_interrupts(|| {
        // Drain anything buffered, so "empty" below means empty.
        while try_read_char_raw().is_some() {}

        // A deadline already in the past, with nothing buffered, must return
        // TimedOut immediately rather than blocking. `now_ns()` itself is a
        // valid past-or-present deadline.
        let now = crate::hrtimer::now_ns();
        match read_char_inner(Some(now), 0) {
            ReadOutcome::TimedOut => {}
            ReadOutcome::Byte(_) => return Err("lapsed deadline returned a byte from an empty ring"),
            ReadOutcome::Interrupted => {
                return Err("kernel task (pid 0) reported Interrupted — signal check ignored pid");
            }
        }

        // With a byte buffered, the same lapsed deadline must yield the byte:
        // available input outranks an expired deadline, which is what makes
        // `read_char_timeout(now)` usable as a non-blocking poll.
        if !push_char_raw(b'K') {
            return Err("could not stage a byte in the input ring");
        }
        match read_char_inner(Some(now), 0) {
            ReadOutcome::Byte(b'K') => {}
            ReadOutcome::Byte(_) => return Err("read_char_inner returned the wrong byte"),
            ReadOutcome::TimedOut => return Err("buffered byte lost to an expired deadline"),
            ReadOutcome::Interrupted => return Err("kernel task reported Interrupted"),
        }

        // And the public wrapper agrees, which is the contract callers see.
        if read_char_timeout(crate::hrtimer::now_ns()).is_some() {
            return Err("read_char_timeout invented a byte from an empty ring");
        }

        Ok(())
    })?;

    crate::serial_println!("[keyboard]   read_char_inner exits (byte/timeout): OK");
    Ok(())
}

/// Exercise the deferred-echo ring: FIFO order, capacity, and drop accounting.
///
/// Runs with interrupts masked because the producer under test is the IRQ 1
/// handler: a keystroke landing mid-test would push into the same ring and
/// make the assertions describe something other than what they name. Masking
/// is cheap here — this path only moves bytes, it never renders.
fn echo_ring_self_test() -> Result<(), &'static str> {
    let dropped_before = ECHO_DROPPED.load(Ordering::Relaxed);

    let result = crate::cpu::without_interrupts(|| {
        if pop_echo().is_some() {
            return Err("echo ring not empty at start of test");
        }

        // FIFO order must be preserved: echo that reorders is worse than no
        // echo, because the user cannot tell it happened.
        for ch in b"abc" {
            if !try_push_echo(*ch) {
                return Err("echo ring rejected a push while empty");
            }
        }
        for expected in b"abc" {
            match pop_echo() {
                Some(got) if got == *expected => {}
                Some(_) => return Err("echo ring returned bytes out of order"),
                None => return Err("echo ring lost a byte"),
            }
        }
        if pop_echo().is_some() {
            return Err("echo ring had leftover bytes after drain");
        }

        // Capacity: one slot is sacrificed to distinguish full from empty, so
        // exactly SIZE-1 pushes must succeed and the next must be refused and
        // counted rather than silently overwriting an unread byte.
        for _ in 0..ECHO_BUF_SIZE.saturating_sub(1) {
            if !try_push_echo(b'x') {
                return Err("echo ring filled short of its capacity");
            }
        }
        if try_push_echo(b'y') {
            return Err("echo ring accepted a push past capacity");
        }

        let mut drained = 0usize;
        while pop_echo().is_some() {
            drained = drained.saturating_add(1);
        }
        if drained != ECHO_BUF_SIZE.saturating_sub(1) {
            return Err("echo ring drained a different count than it accepted");
        }
        Ok(())
    });
    result?;

    // The refused push must have been counted; an uncounted drop is exactly
    // the silent-corruption case the counter exists to make visible.
    let dropped_after = ECHO_DROPPED.load(Ordering::Relaxed);
    if dropped_after != dropped_before.wrapping_add(1) {
        return Err("echo ring did not count the dropped byte");
    }

    crate::serial_println!(
        "[keyboard]   Echo ring: FIFO order, capacity {}, drop accounting OK \
         (deferred out of IRQ context; {} dropped since boot)",
        ECHO_BUF_SIZE.saturating_sub(1),
        dropped_after,
    );
    Ok(())
}
