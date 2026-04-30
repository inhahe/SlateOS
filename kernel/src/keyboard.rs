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

/// Process a keyboard scan code from the ISR.
///
/// Reads the scan code byte from port 0x60, updates modifier state,
/// and pushes any resulting ASCII character into the ring buffer.
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

    if extended {
        handle_extended(code, pressed);
    } else {
        handle_normal(code, pressed);
    }
}

// ---------------------------------------------------------------------------
// Scan code processing
// ---------------------------------------------------------------------------

/// Handle a normal (non-extended) scan code.
fn handle_normal(code: u8, pressed: bool) {
    match code {
        // Modifier keys — update state, no character output.
        0x2A => { LEFT_SHIFT.store(pressed, Ordering::Release); }
        0x36 => { RIGHT_SHIFT.store(pressed, Ordering::Release); }
        0x1D => { LEFT_CTRL.store(pressed, Ordering::Release); }
        0x38 => { LEFT_ALT.store(pressed, Ordering::Release); }
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
        0x1D => { RIGHT_CTRL.store(pressed, Ordering::Release); }
        0x38 => { RIGHT_ALT.store(pressed, Ordering::Release); }
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
    let shift = LEFT_SHIFT.load(Ordering::Acquire)
        || RIGHT_SHIFT.load(Ordering::Acquire);
    let caps = CAPS_LOCK.load(Ordering::Acquire);
    let ctrl = LEFT_CTRL.load(Ordering::Acquire)
        || RIGHT_CTRL.load(Ordering::Acquire);

    // Determine effective shift state for letters: XOR of shift and caps.
    let upper = shift ^ caps;

    // Scan code set 1 normal key table.
    // Index: scan code (0x02-0x39, plus a few others).
    let ch: u8 = match code {
        // Number row
        0x02 => if shift { b'!' } else { b'1' },
        0x03 => if shift { b'@' } else { b'2' },
        0x04 => if shift { b'#' } else { b'3' },
        0x05 => if shift { b'$' } else { b'4' },
        0x06 => if shift { b'%' } else { b'5' },
        0x07 => if shift { b'^' } else { b'6' },
        0x08 => if shift { b'&' } else { b'7' },
        0x09 => if shift { b'*' } else { b'8' },
        0x0A => if shift { b'(' } else { b'9' },
        0x0B => if shift { b')' } else { b'0' },
        0x0C => if shift { b'_' } else { b'-' },
        0x0D => if shift { b'+' } else { b'=' },

        0x0E => b'\x08', // Backspace
        0x0F => b'\t',   // Tab
        0x1C => b'\n',   // Enter

        // QWERTY row
        0x10 => if upper { b'Q' } else { b'q' },
        0x11 => if upper { b'W' } else { b'w' },
        0x12 => if upper { b'E' } else { b'e' },
        0x13 => if upper { b'R' } else { b'r' },
        0x14 => if upper { b'T' } else { b't' },
        0x15 => if upper { b'Y' } else { b'y' },
        0x16 => if upper { b'U' } else { b'u' },
        0x17 => if upper { b'I' } else { b'i' },
        0x18 => if upper { b'O' } else { b'o' },
        0x19 => if upper { b'P' } else { b'p' },
        0x1A => if shift { b'{' } else { b'[' },
        0x1B => if shift { b'}' } else { b']' },

        // Home row
        0x1E => if upper { b'A' } else { b'a' },
        0x1F => if upper { b'S' } else { b's' },
        0x20 => if upper { b'D' } else { b'd' },
        0x21 => if upper { b'F' } else { b'f' },
        0x22 => if upper { b'G' } else { b'g' },
        0x23 => if upper { b'H' } else { b'h' },
        0x24 => if upper { b'J' } else { b'j' },
        0x25 => if upper { b'K' } else { b'k' },
        0x26 => if upper { b'L' } else { b'l' },
        0x27 => if shift { b':' } else { b';' },
        0x28 => if shift { b'"' } else { b'\'' },
        0x29 => if shift { b'~' } else { b'`' },

        0x2B => if shift { b'|' } else { b'\\' },

        // Bottom row
        0x2C => if upper { b'Z' } else { b'z' },
        0x2D => if upper { b'X' } else { b'x' },
        0x2E => if upper { b'C' } else { b'c' },
        0x2F => if upper { b'V' } else { b'v' },
        0x30 => if upper { b'B' } else { b'b' },
        0x31 => if upper { b'N' } else { b'n' },
        0x32 => if upper { b'M' } else { b'm' },
        0x33 => if shift { b'<' } else { b',' },
        0x34 => if shift { b'>' } else { b'.' },
        0x35 => if shift { b'?' } else { b'/' },

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
fn extended_to_ascii(code: u8) -> Option<u8> {
    match code {
        0x1C => Some(b'\n'), // Keypad Enter
        0x35 => Some(b'/'),  // Keypad /
        0x53 => Some(0x7F),  // Delete → DEL character

        // Arrow keys, Home, End, PgUp, PgDn, Insert → no single ASCII.
        // A future input system will emit richer key events.  For now,
        // these are dropped.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Ring buffer operations
// ---------------------------------------------------------------------------

/// Push a character into the ring buffer (called from ISR).
///
/// If the buffer is full, the character is silently dropped.
fn push_char(ch: u8) {
    let head = INPUT_HEAD.load(Ordering::Acquire);
    let tail = INPUT_TAIL.load(Ordering::Acquire);

    // Check if buffer is full (head is one slot behind tail after wrap).
    let next_head = head.wrapping_add(1);
    if (next_head & INPUT_BUF_MASK as u32) == (tail & INPUT_BUF_MASK as u32) {
        // Buffer full — drop the character.
        return;
    }

    let idx = (head as usize) & INPUT_BUF_MASK;
    INPUT_BUF[idx].store(ch, Ordering::Release);
    INPUT_HEAD.store(next_head, Ordering::Release);

    // Also echo the character to the framebuffer console for immediate
    // visual feedback.  This is a temporary measure until a proper
    // terminal emulator exists.
    match ch {
        b'\x08' => {
            // Backspace: move cursor back, overwrite with space, move back again.
            // The console doesn't natively support cursor-back, so for now
            // just print the character and let the future shell handle it.
        }
        0x1B => {} // Don't echo ESC
        _ => crate::console::putchar(ch),
    }
}

/// Try to read one character from the ring buffer without blocking.
///
/// Returns `Some(ch)` if a character is available, `None` if the buffer
/// is empty.
pub fn try_read_char() -> Option<u8> {
    let head = INPUT_HEAD.load(Ordering::Acquire);
    let tail = INPUT_TAIL.load(Ordering::Acquire);

    if (head & INPUT_BUF_MASK as u32) == (tail & INPUT_BUF_MASK as u32)
        && head == tail
    {
        return None; // Empty.
    }

    let idx = (tail as usize) & INPUT_BUF_MASK;
    let ch = INPUT_BUF[idx].load(Ordering::Acquire);
    INPUT_TAIL.store(tail.wrapping_add(1), Ordering::Release);
    Some(ch)
}

/// Read one character, blocking if the buffer is empty.
///
/// This spins in a loop yielding the CPU (via HLT) until a character
/// becomes available.  In the future this will use proper scheduler
/// blocking with an eventfd or similar mechanism.
pub fn read_char() -> u8 {
    loop {
        if let Some(ch) = try_read_char() {
            return ch;
        }
        // Yield CPU until next interrupt (the keyboard IRQ or timer
        // will wake us).
        crate::cpu::hlt();
    }
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
        if head == tail { "empty, OK" } else { "non-empty" }
    );

    crate::serial_println!("[keyboard] Self-test PASSED");
    Ok(())
}
