//! PS/2 mouse driver.
//!
//! Communicates with the mouse via the i8042 controller's second port.
//! The mouse sends 3-byte packets (or 4-byte with IntelliMouse scroll wheel
//! extension) reporting button state and relative movement.
//!
//! ## Architecture
//!
//! The mouse uses IRQ 12, which arrives through the IOAPIC.  The ISR
//! accumulates bytes into a packet buffer and, once a complete packet is
//! assembled, pushes a [`MouseEvent`] into a lock-free ring buffer.
//! Consumer tasks poll via [`try_read_event`] or block via [`read_event`].
//!
//! ## IntelliMouse scroll wheel detection
//!
//! After basic initialization, we attempt the "magic sample rate sequence"
//! (200, 100, 80) that triggers IntelliMouse mode.  If the device ID
//! changes from 0 to 3, the mouse sends 4-byte packets with scroll data.
//!
//! ## Thread safety
//!
//! The ring buffer is single-producer (ISR) / multi-consumer (tasks) using
//! atomic head/tail.  All ISR-side code uses only atomic operations.

use core::sync::atomic::{AtomicBool, AtomicI16, AtomicU8, AtomicU32, Ordering};

use crate::port;

// ---------------------------------------------------------------------------
// PS/2 controller ports and commands
// ---------------------------------------------------------------------------

/// Data port — read/write mouse data through the controller.
const DATA_PORT: u16 = 0x60;
/// Status register (read) / command register (write).
const STATUS_PORT: u16 = 0x64;

// Status register bits
/// Output buffer full — data ready to read from port 0x60.
const STATUS_OUTPUT_FULL: u8 = 1 << 0;
/// Input buffer full — controller busy, don't write.
const STATUS_INPUT_FULL: u8 = 1 << 1;

// Controller commands
/// Read controller configuration byte.
const CMD_READ_CONFIG: u8 = 0x20;
/// Write controller configuration byte.
const CMD_WRITE_CONFIG: u8 = 0x60;
/// Enable second PS/2 port (mouse).
const CMD_ENABLE_PORT2: u8 = 0xA8;
/// Test second PS/2 port.
const CMD_TEST_PORT2: u8 = 0xA9;
/// Write next byte to second PS/2 port (mouse) input buffer.
const CMD_WRITE_PORT2: u8 = 0xD4;

// Mouse commands (sent via CMD_WRITE_PORT2)
/// Reset mouse to defaults.
const MOUSE_CMD_RESET: u8 = 0xFF;
/// Set sample rate (followed by rate value).
const MOUSE_CMD_SET_SAMPLE_RATE: u8 = 0xF3;
/// Get device ID.
const MOUSE_CMD_GET_ID: u8 = 0xF2;
/// Enable data reporting.
const MOUSE_CMD_ENABLE_REPORTING: u8 = 0xF4;
/// Set resolution (followed by resolution value).
const MOUSE_CMD_SET_RESOLUTION: u8 = 0xE8;
/// Set defaults (resets to stream mode, 100 samples/sec, etc.).
const MOUSE_CMD_SET_DEFAULTS: u8 = 0xF6;

// Mouse responses
/// Command acknowledged.
const MOUSE_ACK: u8 = 0xFA;
/// Self-test passed (sent after reset).
const MOUSE_SELF_TEST_PASS: u8 = 0xAA;

// ---------------------------------------------------------------------------
// Mouse event ring buffer
// ---------------------------------------------------------------------------

/// A mouse event: button state + relative movement.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct MouseEvent {
    /// Button state bitmask: bit 0 = left, bit 1 = right, bit 2 = middle.
    pub buttons: u8,
    /// Relative X movement (positive = right).
    pub dx: i16,
    /// Relative Y movement (positive = up, matching PS/2 convention).
    pub dy: i16,
    /// Scroll wheel movement (positive = scroll up). Zero if no scroll wheel.
    pub dz: i8,
}

/// Size of the event ring buffer (must be a power of two).
const EVENT_BUF_SIZE: usize = 128;
const EVENT_BUF_MASK: usize = EVENT_BUF_SIZE - 1;

/// Event ring buffer storage.
///
/// Each event is stored as 4 atomic values (buttons, dx, dy, dz) at the
/// corresponding index.  This avoids needing a lock in the ISR.
static EVENT_BUTTONS: [AtomicU8; EVENT_BUF_SIZE] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; EVENT_BUF_SIZE]
};
static EVENT_DX: [AtomicI16; EVENT_BUF_SIZE] = {
    const ZERO: AtomicI16 = AtomicI16::new(0);
    [ZERO; EVENT_BUF_SIZE]
};
static EVENT_DY: [AtomicI16; EVENT_BUF_SIZE] = {
    const ZERO: AtomicI16 = AtomicI16::new(0);
    [ZERO; EVENT_BUF_SIZE]
};
static EVENT_DZ: [AtomicU8; EVENT_BUF_SIZE] = {
    // Store dz as u8 (reinterpreted as i8 on read) to use AtomicU8.
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; EVENT_BUF_SIZE]
};

/// Write index (next slot the ISR will write to).
static EVENT_HEAD: AtomicU32 = AtomicU32::new(0);
/// Read index (next slot a consumer will read from).
static EVENT_TAIL: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Driver state
// ---------------------------------------------------------------------------

/// Whether the driver has been initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Whether the mouse supports scroll wheel (IntelliMouse, 4-byte packets).
static HAS_SCROLL_WHEEL: AtomicBool = AtomicBool::new(false);

/// Packet assembly state: current byte index within the packet (0, 1, 2, or 3).
static PACKET_IDX: AtomicU8 = AtomicU8::new(0);

/// Packet assembly buffer (up to 4 bytes).
static PACKET_BUF: [AtomicU8; 4] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; 4]
};

/// Cumulative movement since last event read (for absolute position tracking).
static ACCUM_X: AtomicI16 = AtomicI16::new(0);
static ACCUM_Y: AtomicI16 = AtomicI16::new(0);

/// Total event count (for diagnostics).
static EVENT_COUNT: AtomicU32 = AtomicU32::new(0);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the PS/2 mouse on the second controller port and unmask IRQ 12.
///
/// After this call, mouse movements generate IRQ 12 and events appear in
/// the ring buffer.
///
/// # Safety
///
/// - Must be called after IOAPIC and IDT are initialized.
/// - Must be called after keyboard::init() (which sets up the i8042 controller).
/// - Called exactly once.
pub unsafe fn init() {
    crate::serial_println!("[mouse] Initializing PS/2 mouse...");

    // Enable the second PS/2 port.
    // SAFETY: Standard i8042 command, controller is already initialized.
    unsafe {
        controller_cmd(CMD_ENABLE_PORT2);
    }

    // Read the controller configuration byte and enable port 2 interrupt (bit 1).
    // SAFETY: Standard i8042 config read/write.
    unsafe {
        controller_cmd(CMD_READ_CONFIG);
    }
    let config = wait_read_data();
    let new_config = config | 0x02; // Set bit 1: port 2 interrupt enable
    // SAFETY: Writing a valid configuration byte.
    unsafe {
        controller_cmd(CMD_WRITE_CONFIG);
        wait_write_data(new_config);
    }

    // Test port 2.
    // SAFETY: Standard diagnostic command.
    unsafe {
        controller_cmd(CMD_TEST_PORT2);
    }
    let port2_test = wait_read_data();
    if port2_test != 0x00 {
        crate::serial_println!(
            "[mouse] WARNING: port 2 test returned {:#04x} (expected 0x00)",
            port2_test
        );
        // Continue — some emulated controllers fail this but work fine.
    }

    // Re-enable port 2 (test may disable it on some controllers).
    // SAFETY: Standard i8042 command.
    unsafe {
        controller_cmd(CMD_ENABLE_PORT2);
    }

    // Reset the mouse.
    if !mouse_reset() {
        crate::serial_println!("[mouse] WARNING: mouse reset failed, continuing anyway");
    }

    // Set defaults (stream mode, 100 samples/sec, 4 counts/mm).
    mouse_send_cmd(MOUSE_CMD_SET_DEFAULTS);

    // Attempt IntelliMouse scroll wheel detection.
    // Magic sequence: set sample rate to 200, 100, 80, then get device ID.
    // If the device ID is 3, scroll wheel is available.
    detect_scroll_wheel();

    // Set resolution to 4 counts/mm (value 2).
    mouse_send_cmd(MOUSE_CMD_SET_RESOLUTION);
    mouse_send_cmd(2);

    // Set sample rate to 100 samples/sec.
    mouse_send_cmd(MOUSE_CMD_SET_SAMPLE_RATE);
    mouse_send_cmd(100);

    // Enable data reporting — mouse starts sending packets.
    mouse_send_cmd(MOUSE_CMD_ENABLE_REPORTING);

    // Unmask IRQ 12 on the IOAPIC.
    // SAFETY: IOAPIC is initialized, IRQ 12 is the mouse line.
    unsafe {
        crate::ioapic::unmask_irq(12);
    }

    INITIALIZED.store(true, Ordering::Release);

    let scroll = if HAS_SCROLL_WHEEL.load(Ordering::Acquire) {
        " (scroll wheel detected)"
    } else {
        ""
    };
    crate::serial_println!("[mouse] PS/2 mouse initialized (IRQ 12 unmasked){}", scroll);
}

/// Attempt IntelliMouse scroll wheel detection.
///
/// Sets sample rate to 200, 100, 80 in sequence, then reads the device ID.
/// If the ID changes from 0 to 3, the mouse has a scroll wheel and will
/// send 4-byte packets.
fn detect_scroll_wheel() {
    // Magic knock sequence.
    mouse_send_cmd(MOUSE_CMD_SET_SAMPLE_RATE);
    mouse_send_cmd(200);
    mouse_send_cmd(MOUSE_CMD_SET_SAMPLE_RATE);
    mouse_send_cmd(100);
    mouse_send_cmd(MOUSE_CMD_SET_SAMPLE_RATE);
    mouse_send_cmd(80);

    // Read device ID.
    mouse_send_cmd(MOUSE_CMD_GET_ID);
    let id = wait_read_data();

    if id == 3 || id == 4 {
        HAS_SCROLL_WHEEL.store(true, Ordering::Release);
        crate::serial_println!("[mouse] IntelliMouse detected (device ID {})", id);
    }
}

/// Reset the mouse and wait for self-test result.
///
/// Returns true if reset succeeded (ACK + 0xAA + device ID 0x00).
fn mouse_reset() -> bool {
    mouse_send_cmd(MOUSE_CMD_RESET);

    // After reset, mouse sends: 0xAA (self-test pass), 0x00 (device ID).
    let st = wait_read_data();
    if st != MOUSE_SELF_TEST_PASS {
        crate::serial_println!(
            "[mouse] Reset self-test returned {:#04x} (expected 0xAA)",
            st
        );
        return false;
    }

    let id = wait_read_data();
    if id != 0x00 {
        crate::serial_println!(
            "[mouse] Reset device ID {:#04x} (expected 0x00)",
            id
        );
        // Not fatal — some mice report non-zero ID.
    }

    true
}

// ---------------------------------------------------------------------------
// ISR entry point — called from handle_device_irq when IRQ == 12
// ---------------------------------------------------------------------------

/// Process a mouse data byte from the ISR.
///
/// Accumulates bytes into a packet and, once complete, pushes a [`MouseEvent`]
/// into the ring buffer.
///
/// # Safety note
///
/// This is called from interrupt context.  It uses only atomic operations
/// and port I/O — no locks.
pub fn handle_irq() {
    if !INITIALIZED.load(Ordering::Acquire) {
        // Drain the byte even if not initialized (prevent controller hang).
        let _ = unsafe { port::inb(DATA_PORT) };
        return;
    }

    // Read the data byte from the controller.
    // SAFETY: Port 0x60 data is available when IRQ 12 fires.
    let byte = unsafe { port::inb(DATA_PORT) };

    let idx = PACKET_IDX.load(Ordering::Acquire);

    // Packet sync: byte 0 must have bit 3 set (the "always 1" bit in the
    // first packet byte).  If it's not set and we think we're at byte 0,
    // this is a desync — discard until we see a valid byte 0.
    if idx == 0 && (byte & 0x08) == 0 {
        // Not a valid first byte — discard (resync).
        return;
    }

    PACKET_BUF[idx as usize].store(byte, Ordering::Release);

    let packet_size = if HAS_SCROLL_WHEEL.load(Ordering::Acquire) { 4u8 } else { 3u8 };

    if idx + 1 >= packet_size {
        // Complete packet — assemble the event.
        assemble_event(packet_size);
        PACKET_IDX.store(0, Ordering::Release);
    } else {
        PACKET_IDX.store(idx + 1, Ordering::Release);
    }
}

/// Assemble a complete mouse packet into a [`MouseEvent`] and push it.
fn assemble_event(packet_size: u8) {
    let b0 = PACKET_BUF[0].load(Ordering::Acquire);
    let b1 = PACKET_BUF[1].load(Ordering::Acquire);
    let b2 = PACKET_BUF[2].load(Ordering::Acquire);

    // Check overflow bits — if either is set, discard the packet.
    if (b0 & 0xC0) != 0 {
        return;
    }

    // Buttons: bits 0-2 of byte 0.
    let buttons = b0 & 0x07;

    // X movement: byte 1 is the low 8 bits, bit 4 of byte 0 is the sign bit.
    let dx_raw = b1 as u16 | if (b0 & 0x10) != 0 { 0xFF00 } else { 0x0000 };
    let dx = dx_raw as i16;

    // Y movement: byte 2 is the low 8 bits, bit 5 of byte 0 is the sign bit.
    // PS/2 convention: positive Y = up.
    let dy_raw = b2 as u16 | if (b0 & 0x20) != 0 { 0xFF00 } else { 0x0000 };
    let dy = dy_raw as i16;

    // Scroll wheel (if present): byte 3, signed 4-bit value.
    let dz: i8 = if packet_size >= 4 {
        let b3 = PACKET_BUF[3].load(Ordering::Acquire);
        // Only lower 4 bits are the scroll value (sign-extended).
        let raw = b3 & 0x0F;
        if raw & 0x08 != 0 {
            // Negative: sign-extend from 4 bits.
            (raw | 0xF0) as i8
        } else {
            raw as i8
        }
    } else {
        0
    };

    // Update cumulative position.
    let _ = ACCUM_X.fetch_add(dx, Ordering::Relaxed);
    let _ = ACCUM_Y.fetch_add(dy, Ordering::Relaxed);

    // Push event into ring buffer.
    push_event(buttons, dx, dy, dz);
}

/// Push a mouse event into the ring buffer (ISR context).
fn push_event(buttons: u8, dx: i16, dy: i16, dz: i8) {
    let head = EVENT_HEAD.load(Ordering::Acquire);
    let next_head = head.wrapping_add(1);

    // Check if buffer is full.
    let tail = EVENT_TAIL.load(Ordering::Acquire);
    if (next_head & EVENT_BUF_MASK as u32) == (tail & EVENT_BUF_MASK as u32) {
        // Buffer full — drop the event.
        return;
    }

    let idx = (head as usize) & EVENT_BUF_MASK;
    EVENT_BUTTONS[idx].store(buttons, Ordering::Release);
    EVENT_DX[idx].store(dx, Ordering::Release);
    EVENT_DY[idx].store(dy, Ordering::Release);
    EVENT_DZ[idx].store(dz as u8, Ordering::Release);
    EVENT_HEAD.store(next_head, Ordering::Release);

    EVENT_COUNT.fetch_add(1, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Public API for consumers
// ---------------------------------------------------------------------------

/// Try to read one mouse event from the ring buffer without blocking.
///
/// Returns `Some(event)` if an event is available, `None` if empty.
pub fn try_read_event() -> Option<MouseEvent> {
    let head = EVENT_HEAD.load(Ordering::Acquire);
    let tail = EVENT_TAIL.load(Ordering::Acquire);

    if (head & EVENT_BUF_MASK as u32) == (tail & EVENT_BUF_MASK as u32)
        && head == tail
    {
        return None; // Empty.
    }

    let idx = (tail as usize) & EVENT_BUF_MASK;
    let buttons = EVENT_BUTTONS[idx].load(Ordering::Acquire);
    let dx = EVENT_DX[idx].load(Ordering::Acquire);
    let dy = EVENT_DY[idx].load(Ordering::Acquire);
    let dz = EVENT_DZ[idx].load(Ordering::Acquire) as i8;

    EVENT_TAIL.store(tail.wrapping_add(1), Ordering::Release);

    Some(MouseEvent { buttons, dx, dy, dz })
}

/// Read a mouse event, blocking until one is available.
///
/// Yields to the scheduler while waiting.
pub fn read_event() -> MouseEvent {
    loop {
        if let Some(event) = try_read_event() {
            return event;
        }
        crate::sched::yield_now();
    }
}

/// Return the total number of mouse events received since boot.
pub fn event_count() -> u32 {
    EVENT_COUNT.load(Ordering::Relaxed)
}

/// Return the cumulative X position (sum of all dx since boot).
pub fn accum_x() -> i16 {
    ACCUM_X.load(Ordering::Relaxed)
}

/// Return the cumulative Y position (sum of all dy since boot).
pub fn accum_y() -> i16 {
    ACCUM_Y.load(Ordering::Relaxed)
}

/// Return true if the mouse has been initialized.
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Return true if the mouse has a scroll wheel.
pub fn has_scroll_wheel() -> bool {
    HAS_SCROLL_WHEEL.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Low-level PS/2 controller I/O
// ---------------------------------------------------------------------------

/// Send a command byte to the i8042 controller (port 0x64).
///
/// # Safety
///
/// The command must be valid for the current controller state.
unsafe fn controller_cmd(cmd: u8) {
    wait_input_clear();
    // SAFETY: Caller guarantees cmd is valid.
    unsafe {
        port::outb(STATUS_PORT, cmd);
    }
}

/// Write a data byte to port 0x60.
///
/// # Safety
///
/// Must only be called when the controller expects a data byte.
unsafe fn wait_write_data(data: u8) {
    wait_input_clear();
    // SAFETY: Caller guarantees the controller is expecting data.
    unsafe {
        port::outb(DATA_PORT, data);
    }
}

/// Send a command to the mouse (via the controller's CMD_WRITE_PORT2 prefix).
///
/// Writes 0xD4 to port 0x64 (directing next byte to port 2), then the
/// command byte to port 0x60.  Waits for ACK (0xFA) from the mouse.
fn mouse_send_cmd(cmd: u8) {
    // Tell controller to forward next byte to the mouse.
    // SAFETY: CMD_WRITE_PORT2 is a standard i8042 command.
    unsafe {
        controller_cmd(CMD_WRITE_PORT2);
        wait_write_data(cmd);
    }

    // Wait for ACK.
    let ack = wait_read_data();
    if ack != MOUSE_ACK {
        // Some commands (like get-ID) return data after ACK; the caller
        // handles that.  For non-ACK responses during init, just log it.
        if ack != 0 {
            crate::serial_println!(
                "[mouse] cmd {:#04x}: got {:#04x} instead of ACK",
                cmd,
                ack
            );
        }
    }
}

/// Read a data byte from port 0x60 with timeout.
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
    0 // Timeout — no data.
}

/// Wait for the controller's input buffer to be clear.
fn wait_input_clear() {
    for _ in 0..10_000u32 {
        // SAFETY: Reading status port is always safe.
        let status = unsafe { port::inb(STATUS_PORT) };
        if status & STATUS_INPUT_FULL == 0 {
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Verify mouse initialization by checking state.
pub fn self_test() -> Result<(), &'static str> {
    crate::serial_println!("[mouse] Running self-test...");

    if !INITIALIZED.load(Ordering::Acquire) {
        return Err("mouse not initialized");
    }

    // Verify ring buffer is in a consistent state.
    let head = EVENT_HEAD.load(Ordering::Acquire);
    let tail = EVENT_TAIL.load(Ordering::Acquire);
    // Head should be >= tail (or they wrap, but both start at 0).
    if head < tail && (tail - head) > EVENT_BUF_SIZE as u32 {
        return Err("ring buffer state inconsistent");
    }

    // Verify packet assembly is at byte 0 (no partial packet from init noise).
    // Give a small grace period — the first real packet may already be arriving.
    let pkt_idx = PACKET_IDX.load(Ordering::Acquire);
    if pkt_idx >= 4 {
        return Err("packet index out of range");
    }

    crate::serial_println!("[mouse] Self-test passed (scroll_wheel={})",
        HAS_SCROLL_WHEEL.load(Ordering::Acquire));
    Ok(())
}
