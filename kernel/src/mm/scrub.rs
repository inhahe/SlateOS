//! Memory scrubber — proactive ECC error detection.
//!
//! Periodically reads all physical memory to trigger hardware error
//! detection (ECC memory reports correctable/uncorrectable errors to the
//! CPU's Machine Check Architecture).  By scrubbing memory in the
//! background, we detect bit-rot and failing DIMMs BEFORE the corrupted
//! data is actually used — allowing the system to take corrective action
//! (e.g., page offlining, DIMM replacement warning).
//!
//! ## How It Works
//!
//! 1. The scrubber maintains a cursor through physical address space.
//! 2. Each `scrub_step()` call reads a batch of cache lines from the
//!    current cursor position.
//! 3. If an uncorrectable error exists, the CPU's MCE handler fires.
//!    If a correctable error exists, it's logged via CMCI.
//! 4. After scrubbing all physical memory, the cycle counter increments
//!    and the cursor resets.
//!
//! ## Scheduling
//!
//! The scrubber is designed to run during idle time (called from the
//! idle loop) or as a low-priority periodic task.  Each step scrubs a
//! small region (~64 KiB) to avoid monopolizing memory bandwidth.
//!
//! ## Performance Impact
//!
//! Reading memory at ~1 GB/s (limited by memory bandwidth sharing):
//! - 4 GiB system: full scrub in ~4 seconds of idle time
//! - With 64 KiB per step at 1000 steps/second: ~4 seconds wall-clock
//! - Actual impact is minimal because scrub_step() only runs when idle
//!
//! ## References
//!
//! - Linux `drivers/edac/` — Error Detection And Correction subsystem
//! - Linux `mm/hwpoison.c` — handling memory hardware errors
//! - Intel SDM Vol. 3B §15.10 — Machine Check Architecture
//! - ECC Memory Scrubbing: JEDEC DDR4/DDR5 scrub recommendations

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;
use crate::mm::frame::{self, FRAME_SIZE};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Number of bytes to scrub per step (64 KiB = 4 frames).
///
/// Balances progress speed vs memory bandwidth impact.
/// At 64 KiB per step with ~1000 steps per second of idle time,
/// a 4 GiB system is fully scrubbed in about 65 seconds.
const SCRUB_STEP_BYTES: usize = 64 * 1024;

/// Number of cache lines per step (64 KiB / 64 bytes per line = 1024).
const _LINES_PER_STEP: usize = SCRUB_STEP_BYTES / 64;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Whether the scrubber is enabled (can be disabled for benchmarking).
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Current physical address cursor (where the next scrub_step starts).
static CURSOR: AtomicU64 = AtomicU64::new(0);

/// End of scrubable physical address range.
static SCRUB_END: AtomicU64 = AtomicU64::new(0);

/// Total bytes scrubbed since boot.
static TOTAL_BYTES_SCRUBBED: AtomicU64 = AtomicU64::new(0);

/// Number of complete scrub cycles (full passes over all memory).
static CYCLES_COMPLETED: AtomicU64 = AtomicU64::new(0);

/// Number of scrub steps executed.
static STEPS_EXECUTED: AtomicU64 = AtomicU64::new(0);

/// Number of errors detected (MCE handlers would set this).
static ERRORS_DETECTED: AtomicU64 = AtomicU64::new(0);

/// Whether a scrub pass is currently in progress.
static IN_PROGRESS: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Scrubber statistics snapshot.
#[derive(Debug, Clone, Copy)]
pub struct ScrubStats {
    /// Whether the scrubber is enabled.
    pub enabled: bool,
    /// Total bytes scrubbed since boot.
    pub total_bytes: u64,
    /// Number of full scrub cycles completed.
    pub cycles: u64,
    /// Number of scrub steps executed.
    pub steps: u64,
    /// Errors detected (should be 0 on healthy hardware).
    pub errors: u64,
    /// Current cursor position (progress in current cycle).
    pub cursor: u64,
    /// End of scrubable range.
    pub range_end: u64,
    /// Progress percentage in current cycle (0-100).
    pub progress_pct: u8,
}

/// Get current scrubber statistics.
#[must_use]
pub fn stats() -> ScrubStats {
    let cursor = CURSOR.load(Ordering::Relaxed);
    let end = SCRUB_END.load(Ordering::Relaxed);
    let progress = if end > 0 {
        ((cursor.saturating_mul(100)) / end).min(100) as u8
    } else {
        0
    };

    ScrubStats {
        enabled: ENABLED.load(Ordering::Relaxed),
        total_bytes: TOTAL_BYTES_SCRUBBED.load(Ordering::Relaxed),
        cycles: CYCLES_COMPLETED.load(Ordering::Relaxed),
        steps: STEPS_EXECUTED.load(Ordering::Relaxed),
        errors: ERRORS_DETECTED.load(Ordering::Relaxed),
        cursor,
        range_end: end,
        progress_pct: progress,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the memory scrubber.
///
/// Sets up the scrub range based on the physical memory layout from
/// the frame allocator.  Does NOT start scrubbing — call `enable()`
/// after init to begin background scrubbing.
pub fn init() {
    // Get total physical memory from the frame allocator.
    if let Some(fstats) = frame::stats() {
        // Scrub range: 0 to (total_frames * FRAME_SIZE).
        let end = (fstats.total_frames as u64).saturating_mul(FRAME_SIZE as u64);
        SCRUB_END.store(end, Ordering::Release);
        CURSOR.store(0, Ordering::Release);
        serial_println!("[scrub] Initialized: range 0..{:#x} ({} MiB)",
            end, end / (1024 * 1024));
    } else {
        serial_println!("[scrub] Warning: frame allocator not ready, scrub range = 0");
    }
}

/// Enable background memory scrubbing.
pub fn enable() {
    ENABLED.store(true, Ordering::Release);
}

/// Disable background memory scrubbing.
pub fn disable() {
    ENABLED.store(false, Ordering::Release);
}

/// Whether the scrubber is currently enabled.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Execute one scrub step.
///
/// Reads `SCRUB_STEP_BYTES` of physical memory starting at the current
/// cursor position.  Call this from the idle loop or a low-priority timer.
///
/// Returns the number of bytes scrubbed in this step (0 if disabled or
/// no HHDM available).
#[allow(clippy::arithmetic_side_effects)]
pub fn scrub_step() -> usize {
    if !ENABLED.load(Ordering::Acquire) {
        return 0;
    }

    let hhdm = match crate::mm::page_table::hhdm() {
        Some(h) => h,
        None => return 0,
    };

    let end = SCRUB_END.load(Ordering::Relaxed);
    if end == 0 {
        return 0;
    }

    // Get and advance the cursor atomically.
    let cursor = CURSOR.fetch_add(SCRUB_STEP_BYTES as u64, Ordering::Relaxed);

    // Check if we've completed a full cycle.
    if cursor >= end {
        CURSOR.store(0, Ordering::Relaxed);
        CYCLES_COMPLETED.fetch_add(1, Ordering::Relaxed);
        IN_PROGRESS.store(false, Ordering::Relaxed);
        return 0;
    }

    IN_PROGRESS.store(true, Ordering::Relaxed);

    // Calculate actual bytes to scrub (may be less at end of range).
    let remaining = (end - cursor) as usize;
    let to_scrub = remaining.min(SCRUB_STEP_BYTES);

    // Read memory through HHDM.  The act of reading triggers ECC checking
    // in the memory controller.  We use volatile reads to prevent the
    // compiler from optimizing them away (the reads have no software-visible
    // side effect, but the hardware-level error detection is the point).
    let base_virt = (hhdm + cursor) as *const u64;
    let count = to_scrub / 8; // Read 8 bytes at a time.

    // SAFETY: cursor < end, and end is within the frame allocator's managed
    // physical range.  The HHDM maps all physical memory.  We're reading
    // 64-bit aligned values from the HHDM.
    unsafe {
        let mut dummy: u64 = 0;
        for i in 0..count {
            // Use volatile read to force the memory access.
            let val = core::ptr::read_volatile(base_virt.add(i));
            // XOR to a dummy variable to prevent the compiler from
            // removing the read as dead code.  The XOR is essentially free.
            dummy ^= val;
        }
        // Write to a black-hole to prevent optimization of dummy.
        core::hint::black_box(dummy);
    }

    TOTAL_BYTES_SCRUBBED.fetch_add(to_scrub as u64, Ordering::Relaxed);
    STEPS_EXECUTED.fetch_add(1, Ordering::Relaxed);

    to_scrub
}

/// Report a memory error detected during scrubbing.
///
/// Called by the MCE handler if an error is detected at the physical
/// address being scrubbed.
pub fn report_error(phys_addr: u64) {
    ERRORS_DETECTED.fetch_add(1, Ordering::Relaxed);
    serial_println!("[scrub] ERROR: memory error detected at {:#x}", phys_addr);
}

/// Get the number of errors detected.
#[must_use]
pub fn error_count() -> u64 {
    ERRORS_DETECTED.load(Ordering::Relaxed)
}

/// Reset the scrubber cursor to start a fresh cycle.
pub fn reset() {
    CURSOR.store(0, Ordering::Release);
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the memory scrubber.
pub fn self_test() {
    serial_println!("[scrub] Running self-test...");

    // Test 1: Init sets up range correctly.
    init();
    let s = stats();
    assert!(s.range_end > 0, "scrub range should be non-zero after init");
    serial_println!("[scrub]   Init: range_end={:#x} ({} MiB)",
        s.range_end, s.range_end / (1024 * 1024));

    // Test 2: Disabled by default.
    assert!(!s.enabled);
    serial_println!("[scrub]   Default disabled: OK");

    // Test 3: Enable/disable.
    enable();
    assert!(is_enabled());
    disable();
    assert!(!is_enabled());
    serial_println!("[scrub]   Enable/disable: OK");

    // Test 4: scrub_step when disabled returns 0.
    let scrubbed = scrub_step();
    assert_eq!(scrubbed, 0);
    serial_println!("[scrub]   Step while disabled: OK (0 bytes)");

    // Test 5: scrub_step when enabled reads memory.
    enable();
    let scrubbed = scrub_step();
    assert_eq!(scrubbed, SCRUB_STEP_BYTES);
    serial_println!("[scrub]   Step while enabled: OK ({} bytes)", scrubbed);

    // Test 6: Cursor advances.
    let s = stats();
    assert!(s.cursor > 0 || s.cycles > 0); // Either advanced or wrapped.
    assert_eq!(s.steps, 1);
    serial_println!("[scrub]   Cursor advanced: OK (cursor={:#x})", s.cursor);

    // Test 7: Multiple steps.
    let _ = scrub_step();
    let _ = scrub_step();
    let s = stats();
    assert_eq!(s.steps, 3);
    assert!(s.total_bytes >= SCRUB_STEP_BYTES as u64 * 3);
    serial_println!("[scrub]   Multiple steps: OK (total={} bytes)", s.total_bytes);

    // Test 8: Error count starts at 0.
    assert_eq!(error_count(), 0);
    serial_println!("[scrub]   No errors: OK");

    // Test 9: Reset.
    reset();
    let s = stats();
    assert_eq!(s.cursor, 0);
    serial_println!("[scrub]   Reset: OK");

    // Cleanup: disable scrubber.
    disable();

    serial_println!("[scrub] Self-test PASSED");
}
