#![allow(dead_code)] // Public API — callers will be added incrementally.

//! System timekeeping — wall clock and monotonic time.
//!
//! Combines the battery-backed RTC (read once at boot) with the TSC
//! (continuous cycle counter) to provide accurate, low-overhead time
//! queries without re-reading CMOS I/O ports on every call.
//!
//! ## Clocks Provided
//!
//! - **Monotonic**: nanoseconds since boot.  Never goes backwards,
//!   unaffected by wall-clock adjustments.  Used for timeouts, intervals,
//!   performance measurement.
//!
//! - **Realtime** (wall clock): nanoseconds since Unix epoch
//!   (1970-01-01 00:00:00 UTC).  Initialized from RTC at boot, then
//!   maintained via TSC offset.  May be adjusted by NTP or manual set.
//!
//! ## Design
//!
//! ```text
//! ┌──────────┐     boot-time read     ┌──────────────────┐
//! │   CMOS   │  ─────────────────►    │  boot_epoch_ns   │
//! │   RTC    │                        │  boot_tsc        │
//! └──────────┘                        └──────────────────┘
//!                                              │
//!              TSC delta since boot             ▼
//!     now_tsc - boot_tsc  ──► cycles_to_ns ──► + boot_epoch_ns
//!                                              │
//!                                              ▼
//!                                       clock_realtime()
//! ```
//!
//! ## Accuracy
//!
//! - TSC drift: < 1 ppm on modern Intel/AMD.  Over 24 hours, drift is
//!   < 100 ms.  For an OS that hasn't implemented NTP yet, this is fine.
//! - RTC accuracy: depends on the CMOS battery oscillator (~20 ppm typical).
//!   We only read it once at boot, so RTC drift doesn't compound.
//!
//! ## Thread Safety
//!
//! All state is atomic.  Reads are lock-free (one `rdtsc` + arithmetic).
//! Time adjustments use CAS on the offset field.
//!
//! ## References
//!
//! - Linux `kernel/time/timekeeping.c` — struct timekeeper, ktime_get()
//! - Linux `arch/x86/kernel/tsc.c` — TSC-based clocksource
//! - POSIX `clock_gettime(CLOCK_REALTIME)` / `CLOCK_MONOTONIC`

use core::sync::atomic::{AtomicI64, AtomicU64, Ordering};

use crate::bench;
use crate::rtc;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// TSC value at boot-time initialization.
static BOOT_TSC: AtomicU64 = AtomicU64::new(0);

/// Unix epoch nanoseconds corresponding to `BOOT_TSC`.
///
/// This is the RTC time converted to ns-since-epoch at the moment
/// `init()` was called.
static BOOT_EPOCH_NS: AtomicU64 = AtomicU64::new(0);

/// Manual wall-clock adjustment (nanoseconds, signed).
///
/// Added to realtime calculations.  Allows NTP or manual corrections
/// without touching the boot reference point.
static ADJUSTMENT_NS: AtomicI64 = AtomicI64::new(0);

/// Whether timekeeping has been initialized.
static INITIALIZED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize timekeeping from the CMOS RTC.
///
/// Call once during boot, after TSC calibration.  Reads the RTC,
/// converts to Unix epoch nanoseconds, and records the corresponding
/// TSC value.
///
/// After this call, `clock_realtime()` and `clock_monotonic()` are
/// available.
#[allow(clippy::arithmetic_side_effects)]
pub fn init() {
    let tsc_now = bench::rdtsc();
    let dt = rtc::read_datetime();

    // Convert DateTime to Unix epoch seconds.
    let epoch_secs = datetime_to_epoch(&dt);
    // Convert to nanoseconds.
    let epoch_ns = epoch_secs.saturating_mul(1_000_000_000);

    BOOT_TSC.store(tsc_now, Ordering::Release);
    BOOT_EPOCH_NS.store(epoch_ns, Ordering::Release);
    INITIALIZED.store(true, Ordering::Release);

    crate::serial_println!(
        "[time] Timekeeping initialized: {} (epoch {}s)",
        dt, epoch_secs
    );
}

// ---------------------------------------------------------------------------
// Clock queries
// ---------------------------------------------------------------------------

/// Get monotonic time: nanoseconds since boot.
///
/// Never decreases, unaffected by wall-clock adjustments.
/// Cost: one `rdtsc` + division (~25-40 cycles).
#[inline]
#[must_use]
pub fn clock_monotonic() -> u64 {
    let freq = bench::tsc_freq();
    if freq == 0 {
        // TSC not calibrated — fall back to tick count.
        return crate::apic::tick_count().saturating_mul(10_000_000);
    }
    let boot_tsc = BOOT_TSC.load(Ordering::Relaxed);
    let now = bench::rdtsc();
    let elapsed_cycles = now.saturating_sub(boot_tsc);
    cycles_to_ns(elapsed_cycles, freq)
}

/// Get realtime (wall clock): nanoseconds since Unix epoch.
///
/// Combines boot-time RTC reading with TSC-based elapsed time.
/// May be adjusted via `adjust_realtime()`.
#[inline]
#[must_use]
pub fn clock_realtime() -> u64 {
    let boot_epoch = BOOT_EPOCH_NS.load(Ordering::Relaxed);
    if boot_epoch == 0 {
        // Not initialized yet.
        return 0;
    }
    let mono = clock_monotonic();
    let adj = ADJUSTMENT_NS.load(Ordering::Relaxed);

    // realtime = boot_epoch + monotonic + adjustment
    let base = boot_epoch.saturating_add(mono);
    if adj >= 0 {
        base.saturating_add(adj as u64)
    } else {
        base.saturating_sub(adj.unsigned_abs())
    }
}

/// Get the current wall-clock time as a `DateTime` struct.
///
/// Faster than re-reading CMOS: uses TSC offset from boot RTC reading.
#[must_use]
pub fn now() -> rtc::DateTime {
    let epoch_ns = clock_realtime();
    epoch_ns_to_datetime(epoch_ns)
}

/// Get the boot time as Unix epoch seconds.
#[must_use]
pub fn boot_time_epoch_secs() -> u64 {
    BOOT_EPOCH_NS.load(Ordering::Relaxed) / 1_000_000_000
}

/// Get uptime in seconds (from monotonic clock).
#[must_use]
pub fn uptime_secs() -> u64 {
    clock_monotonic() / 1_000_000_000
}

/// Get uptime with millisecond components.
#[must_use]
pub fn uptime_ms() -> (u64, u32) {
    let mono = clock_monotonic();
    let secs = mono / 1_000_000_000;
    let ms = ((mono % 1_000_000_000) / 1_000_000) as u32;
    (secs, ms)
}

/// Check if timekeeping is initialized.
#[inline]
#[must_use]
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Time adjustment
// ---------------------------------------------------------------------------

/// Adjust the realtime clock by a signed offset (nanoseconds).
///
/// Positive = advance the clock.  Negative = set it back.
/// The adjustment is additive to any previous adjustments.
///
/// Used by NTP or manual `date --set` commands.
pub fn adjust_realtime(delta_ns: i64) {
    ADJUSTMENT_NS.fetch_add(delta_ns, Ordering::Relaxed);
}

/// Set the realtime clock to a specific Unix epoch timestamp.
///
/// Computes the required adjustment to make `clock_realtime()` return
/// the target time and stores it atomically.
pub fn set_realtime(target_epoch_ns: u64) {
    let current = clock_realtime();
    let diff = target_epoch_ns as i64 - current as i64;
    ADJUSTMENT_NS.store(diff, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Epoch conversion helpers
// ---------------------------------------------------------------------------

/// Days in each month (non-leap year).
const DAYS_IN_MONTH: [u16; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Convert a `DateTime` to Unix epoch seconds.
///
/// Assumes UTC (the RTC should be set to UTC on this OS).
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn datetime_to_epoch(dt: &rtc::DateTime) -> u64 {
    // Count days from 1970-01-01 to the given date.
    let mut days: u64 = 0;

    // Full years from 1970 to dt.year - 1.
    for y in 1970..dt.year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }

    // Full months in the current year.
    for m in 1..dt.month {
        days += u64::from(DAYS_IN_MONTH[(m - 1) as usize]);
        if m == 2 && is_leap_year(dt.year) {
            days += 1;
        }
    }

    // Days in the current month (day is 1-based).
    days += u64::from(dt.day.saturating_sub(1));

    // Convert to seconds and add time-of-day.
    days * 86400
        + u64::from(dt.hour) * 3600
        + u64::from(dt.minute) * 60
        + u64::from(dt.second)
}

/// Convert Unix epoch nanoseconds back to a `DateTime`.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn epoch_ns_to_datetime(epoch_ns: u64) -> rtc::DateTime {
    let total_secs = epoch_ns / 1_000_000_000;
    let mut remaining = total_secs;

    let second = (remaining % 60) as u8;
    remaining /= 60;
    let minute = (remaining % 60) as u8;
    remaining /= 60;
    let hour = (remaining % 24) as u8;
    remaining /= 24;

    // `remaining` is now days since 1970-01-01.
    let mut year: u16 = 1970;
    loop {
        let days_in_year = if is_leap_year(year) { 366u64 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }

    // `remaining` is now day-of-year (0-based).
    let mut month: u8 = 1;
    loop {
        let dim = u64::from(DAYS_IN_MONTH[(month - 1) as usize])
            + if month == 2 && is_leap_year(year) { 1 } else { 0 };
        if remaining < dim {
            break;
        }
        remaining -= dim;
        month += 1;
    }

    let day = remaining as u8 + 1; // 1-based

    rtc::DateTime { year, month, day, hour, minute, second }
}

/// Check if a year is a leap year.
#[inline]
#[allow(clippy::arithmetic_side_effects)]
fn is_leap_year(year: u16) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// Convert TSC cycles to nanoseconds.
#[inline]
fn cycles_to_ns(cycles: u64, freq: u64) -> u64 {
    let secs = cycles / freq;
    let rem = cycles % freq;
    secs.saturating_mul(1_000_000_000)
        .saturating_add(rem.saturating_mul(1_000_000_000) / freq)
}
