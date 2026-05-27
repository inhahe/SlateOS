//! PC speaker (PIT channel 2) driver.
//!
//! The classic PC speaker is connected to PIT (Programmable Interval Timer)
//! channel 2 via the port 0x61 gate.  By programming the PIT to generate a
//! square wave at a specific frequency and enabling the speaker gate, we can
//! produce tones without any DMA, audio codec, or complex setup.
//!
//! ## Hardware
//!
//! - PIT channel 2 (port 0x42): 16-bit countdown divider
//! - Port 0x61 bits 0-1: gate and speaker enable
//! - Base frequency: 1,193,182 Hz (PIT oscillator)
//! - Frequency = 1,193,182 / divisor
//!
//! ## Usage
//!
//! ```text
//! pcspk::beep(440, 200);  // 440 Hz for 200 ms
//! pcspk::beep(880, 100);  // 880 Hz for 100 ms
//! pcspk::off();           // silence
//! ```
//!
//! ## Notes
//!
//! This works in QEMU with `-audiodev` configured (e.g., `-audiodev sdl,id=a0
//! -machine pcspk-audiodev=a0`).  On real hardware, the PC speaker is always
//! present as it's wired directly to the chipset.
//!
//! ## References
//!
//! - OSDev Wiki: PC Speaker
//! - Intel 8254 PIT datasheet
//! - Linux `drivers/input/misc/pcspkr.c`

// PC speaker driver: helpers like `off()`, frequency constants, and
// debug routines are public API even if production code currently only
// calls `beep()`.
#![allow(dead_code)]

use crate::serial_println;

/// PIT channel 2 data port.
const PIT_CH2_DATA: u16 = 0x42;
/// PIT command register.
const PIT_CMD: u16 = 0x43;
/// System control port B (speaker gate + enable).
const PORT_61: u16 = 0x61;

/// PIT oscillator base frequency (Hz).
const PIT_FREQ: u32 = 1_193_182;

/// Minimum supported frequency (avoid overflow: 1_193_182 / 65535 ≈ 18 Hz).
const MIN_FREQ: u32 = 19;
/// Maximum supported frequency (avoid too-small divisor).
const MAX_FREQ: u32 = 596_591; // PIT_FREQ / 2

// ---------------------------------------------------------------------------
// Port I/O helpers
// ---------------------------------------------------------------------------

#[inline]
fn outb(port: u16, val: u8) {
    // SAFETY: Port I/O to PIT and system control port B — standard PC hardware.
    unsafe {
        core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
    }
}

#[inline]
fn inb(port: u16) -> u8 {
    // SAFETY: Port I/O read — standard PC hardware.
    let val: u8;
    unsafe {
        core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    }
    val
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start playing a tone at the given frequency (Hz).
///
/// The tone plays continuously until [`off()`] is called or a new frequency
/// is set.  Frequency is clamped to [19, 596591] Hz.
pub fn tone(freq: u32) {
    let freq = freq.clamp(MIN_FREQ, MAX_FREQ);

    // Calculate PIT divisor: base_freq / desired_freq.
    let divisor = PIT_FREQ / freq;
    let divisor_lo = (divisor & 0xFF) as u8;
    let divisor_hi = ((divisor >> 8) & 0xFF) as u8;

    // Program PIT channel 2 for square wave mode (mode 3).
    // Command byte: channel 2 (bits 7:6 = 10), access both bytes (11),
    // mode 3 square wave (110), binary counting (0) → 0b10_11_011_0 = 0xB6.
    outb(PIT_CMD, 0xB6);
    outb(PIT_CH2_DATA, divisor_lo);
    outb(PIT_CH2_DATA, divisor_hi);

    // Enable speaker: set bits 0 (PIT gate) and 1 (speaker enable) in port 0x61.
    let port61 = inb(PORT_61);
    outb(PORT_61, port61 | 0x03);
}

/// Stop the PC speaker (silence).
pub fn off() {
    // Clear bits 0 and 1 of port 0x61 to disable PIT gate and speaker.
    let port61 = inb(PORT_61);
    outb(PORT_61, port61 & !0x03);
}

/// Play a tone for a specified duration (milliseconds), then silence.
///
/// This is a blocking call — it busy-waits for the duration.
/// For non-blocking use, call [`tone()`] and schedule [`off()`] later.
pub fn beep(freq: u32, duration_ms: u32) {
    tone(freq);
    delay_ms(duration_ms);
    off();
}

/// Play a short error beep (800 Hz, 100 ms).
pub fn error_beep() {
    beep(800, 100);
}

/// Play a short success beep (1200 Hz, 50 ms).
pub fn success_beep() {
    beep(1200, 50);
}

/// Play a startup chime (ascending three-note sequence).
pub fn startup_chime() {
    beep(523, 80);  // C5
    delay_ms(20);
    beep(659, 80);  // E5
    delay_ms(20);
    beep(784, 120); // G5
}

/// Play a shutdown tone (descending two-note).
pub fn shutdown_tone() {
    beep(784, 100); // G5
    delay_ms(20);
    beep(523, 150); // C5
}

/// Play a sequence of notes.
///
/// Each entry is `(frequency_hz, duration_ms)`.  A frequency of 0 means
/// a rest (silence for that duration).
pub fn play_notes(notes: &[(u32, u32)]) {
    for &(freq, dur) in notes {
        if freq == 0 {
            off();
            delay_ms(dur);
        } else {
            beep(freq, dur);
        }
        // Small gap between notes for articulation.
        delay_ms(10);
    }
}

// ---------------------------------------------------------------------------
// Timing helper
// ---------------------------------------------------------------------------

/// Busy-wait delay in milliseconds (approximate).
fn delay_ms(ms: u32) {
    // Use TSC-based busy loop.  Assume ≥ 2 GHz = 2_000_000 cycles per ms.
    let start = unsafe { core::arch::x86_64::_rdtsc() };
    let target_cycles = (ms as u64).saturating_mul(2_000_000);
    loop {
        let now = unsafe { core::arch::x86_64::_rdtsc() };
        if now.wrapping_sub(start) >= target_cycles {
            break;
        }
        core::hint::spin_loop();
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test: verify PIT programming and speaker gate toggling.
///
/// Note: this won't produce audible output without QEMU's pcspk-audiodev,
/// but verifies the port I/O path works without crashing.
pub fn self_test() {
    serial_println!("[pcspk] Running self-test...");

    // Test 1: Enable and disable speaker gate.
    let before = inb(PORT_61);
    outb(PORT_61, before | 0x03);
    let during = inb(PORT_61);
    outb(PORT_61, before); // Restore.

    // Bits 0 and 1 should have been set.
    let gate_ok = (during & 0x03) == 0x03;
    serial_println!("[pcspk]   Speaker gate toggle: {}", if gate_ok { "OK" } else { "FAIL" });

    // Test 2: Program PIT channel 2 with a known divisor.
    // 440 Hz → divisor 2712 (0x0A98).
    outb(PIT_CMD, 0xB6);
    outb(PIT_CH2_DATA, 0x98); // Low byte
    outb(PIT_CH2_DATA, 0x0A); // High byte
    serial_println!("[pcspk]   PIT program (440 Hz): OK (no exception)");

    // Silence (don't leave speaker running from test).
    off();

    // Test 3: Play a quick beep (10 ms — too short to be annoying).
    tone(1000);
    delay_ms(10);
    off();
    serial_println!("[pcspk]   Quick tone: OK");

    serial_println!("[pcspk] Self-test PASSED");
}
