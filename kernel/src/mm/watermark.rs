//! Memory watermarks — track peak (high-water-mark) usage per subsystem.
//!
//! Each kernel subsystem can register a "meter" with the watermark system.
//! The meter tracks current usage and peak usage, letting operators and
//! developers see the maximum memory demand that occurred since boot.
//!
//! ## Use Cases
//!
//! - **Capacity planning**: know the actual peak memory usage of each
//!   subsystem to right-size limits.
//! - **Leak detection**: if current usage keeps climbing toward the peak
//!   without ever receding, something is probably leaking.
//! - **Performance tuning**: identify which subsystem dominates memory
//!   consumption under real workloads.
//!
//! ## Design
//!
//! Up to 32 named meters are supported.  Each meter is a pair of atomics
//! (current, peak) — no locks needed for updates.  The `charge()`/`uncharge()`
//! API is meant to be called on every alloc/free path for the subsystem.
//!
//! ## References
//!
//! - Linux `include/linux/memcontrol.h` — memory cgroup watermarks
//! - Linux `/proc/meminfo` — `HardwareCorrupted`, `Committed_AS` as watermarks
//! - Windows Performance Counters — Peak Working Set, Peak Commit Charge

use core::sync::atomic::{AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of watermark meters.
const MAX_METERS: usize = 32;

/// Maximum meter name length.
const MAX_NAME_LEN: usize = 20;

// ---------------------------------------------------------------------------
// Meter storage
// ---------------------------------------------------------------------------

/// A single watermark meter.
struct Meter {
    /// Name of the subsystem (zero-padded).
    name: [u8; MAX_NAME_LEN],
    /// Name length.
    name_len: u8,
    /// Whether this slot is in use.
    active: bool,
    /// Current usage (bytes or frames, depending on subsystem).
    current: AtomicU64,
    /// Peak (high-water-mark) usage since boot or last reset.
    peak: AtomicU64,
}

impl Meter {
    const fn empty() -> Self {
        Self {
            name: [0; MAX_NAME_LEN],
            name_len: 0,
            active: false,
            current: AtomicU64::new(0),
            peak: AtomicU64::new(0),
        }
    }
}

/// All meters, statically allocated.
static mut METERS: [Meter; MAX_METERS] = {
    const EMPTY: Meter = Meter::empty();
    [EMPTY; MAX_METERS]
};

/// Number of registered meters.
static METER_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A handle to a registered watermark meter.
///
/// Lightweight (just an index).  Clone/Copy for convenience.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MeterHandle(u8);

/// Register a new watermark meter with the given name.
///
/// Returns a handle used for subsequent charge/uncharge calls.
/// Returns `None` if the meter table is full.
pub fn register(name: &str) -> Option<MeterHandle> {
    let idx = METER_COUNT.fetch_add(1, Ordering::Relaxed) as usize;
    if idx >= MAX_METERS {
        METER_COUNT.fetch_sub(1, Ordering::Relaxed);
        return None;
    }

    // SAFETY: We have exclusive access to this slot via atomic index allocation.
    let meter = unsafe { &mut METERS[idx] };
    let name_bytes = name.as_bytes();
    let copy_len = name_bytes.len().min(MAX_NAME_LEN);
    meter.name[..copy_len].copy_from_slice(&name_bytes[..copy_len]);
    meter.name_len = copy_len as u8;
    meter.active = true;

    Some(MeterHandle(idx as u8))
}

/// Charge (increase) the current usage of a meter.
///
/// Automatically updates the peak if the new current exceeds it.
/// `amount` is in whatever unit the subsystem uses (bytes, frames, objects).
#[inline]
pub fn charge(handle: MeterHandle, amount: u64) {
    let idx = handle.0 as usize;
    if idx >= MAX_METERS {
        return;
    }
    // SAFETY: Index is valid and meter is initialized (handle came from register).
    let meter = unsafe { &METERS[idx] };
    let new_current = meter.current.fetch_add(amount, Ordering::Relaxed)
        .saturating_add(amount);

    // Update peak if needed (relaxed CAS loop for concurrent updates).
    loop {
        let old_peak = meter.peak.load(Ordering::Relaxed);
        if new_current <= old_peak {
            break;
        }
        // On success we're done; on failure (Err) retry the loop.
        if meter.peak.compare_exchange_weak(
            old_peak, new_current,
            Ordering::Relaxed, Ordering::Relaxed
        ).is_ok() {
            break;
        }
    }
}

/// Uncharge (decrease) the current usage of a meter.
#[inline]
pub fn uncharge(handle: MeterHandle, amount: u64) {
    let idx = handle.0 as usize;
    if idx >= MAX_METERS {
        return;
    }
    // SAFETY: Index is valid.
    let meter = unsafe { &METERS[idx] };
    meter.current.fetch_sub(amount.min(meter.current.load(Ordering::Relaxed)),
        Ordering::Relaxed);
}

/// Get current and peak values for a meter.
#[must_use]
pub fn read(handle: MeterHandle) -> (u64, u64) {
    let idx = handle.0 as usize;
    if idx >= MAX_METERS {
        return (0, 0);
    }
    // SAFETY: Index is valid.
    let meter = unsafe { &METERS[idx] };
    (meter.current.load(Ordering::Relaxed), meter.peak.load(Ordering::Relaxed))
}

/// Reset the peak of a meter to the current value.
pub fn reset_peak(handle: MeterHandle) {
    let idx = handle.0 as usize;
    if idx >= MAX_METERS {
        return;
    }
    // SAFETY: Index is valid.
    let meter = unsafe { &METERS[idx] };
    let current = meter.current.load(Ordering::Relaxed);
    meter.peak.store(current, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Statistics / dump
// ---------------------------------------------------------------------------

/// Snapshot of a single meter.
#[derive(Debug, Clone, Copy)]
pub struct MeterSnapshot {
    /// Meter name.
    pub name: [u8; MAX_NAME_LEN],
    /// Name length.
    pub name_len: u8,
    /// Current usage.
    pub current: u64,
    /// Peak usage.
    pub peak: u64,
}

/// Get snapshots of all registered meters.
///
/// Fills `out` with up to `out.len()` meters.  Returns the number filled.
pub fn snapshot_all(out: &mut [MeterSnapshot]) -> usize {
    let count = METER_COUNT.load(Ordering::Relaxed) as usize;
    let fill = count.min(out.len()).min(MAX_METERS);

    for i in 0..fill {
        // SAFETY: Index < METER_COUNT.
        let meter = unsafe { &METERS[i] };
        if !meter.active {
            continue;
        }
        out[i] = MeterSnapshot {
            name: meter.name,
            name_len: meter.name_len,
            current: meter.current.load(Ordering::Relaxed),
            peak: meter.peak.load(Ordering::Relaxed),
        };
    }
    fill
}

/// Total number of registered meters.
#[must_use]
pub fn meter_count() -> usize {
    (METER_COUNT.load(Ordering::Relaxed) as usize).min(MAX_METERS)
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the watermark system.
pub fn self_test() {
    serial_println!("[watermark] Running self-test...");

    // Test 1: Register a meter.
    let handle = register("test_subsystem").expect("register should succeed");
    serial_println!("[watermark]   Register: OK");

    // Test 2: Charge and verify.
    charge(handle, 1000);
    let (current, peak) = read(handle);
    assert_eq!(current, 1000);
    assert_eq!(peak, 1000);
    serial_println!("[watermark]   Charge 1000: OK (current={}, peak={})", current, peak);

    // Test 3: Charge more — peak should update.
    charge(handle, 500);
    let (current, peak) = read(handle);
    assert_eq!(current, 1500);
    assert_eq!(peak, 1500);
    serial_println!("[watermark]   Charge +500: OK (current={}, peak={})", current, peak);

    // Test 4: Uncharge — peak should NOT decrease.
    uncharge(handle, 800);
    let (current, peak) = read(handle);
    assert_eq!(current, 700);
    assert_eq!(peak, 1500); // Peak stays.
    serial_println!("[watermark]   Uncharge 800: OK (current={}, peak={})", current, peak);

    // Test 5: Reset peak.
    reset_peak(handle);
    let (current, peak) = read(handle);
    assert_eq!(current, 700);
    assert_eq!(peak, 700); // Peak reset to current.
    serial_println!("[watermark]   Reset peak: OK");

    // Test 6: Multiple meters.
    let h2 = register("meter_two").expect("second register should succeed");
    charge(h2, 42);
    let (c2, p2) = read(h2);
    assert_eq!(c2, 42);
    assert_eq!(p2, 42);
    serial_println!("[watermark]   Multiple meters: OK");

    // Test 7: Meter count.
    let count = meter_count();
    assert!(count >= 2);
    serial_println!("[watermark]   meter_count={}: OK", count);

    // Cleanup: uncharge test meters.
    uncharge(handle, 700);
    uncharge(h2, 42);

    serial_println!("[watermark] Self-test PASSED");
}
