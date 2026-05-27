//! System load average tracking.
//!
//! Computes exponential weighted moving averages (EWMA) of the number
//! of runnable tasks, sampled every 5 seconds.  Provides the classic
//! 1-minute, 5-minute, and 15-minute load averages familiar from Unix
//! systems (`uptime`, `/proc/loadavg`).
//!
//! ## Algorithm
//!
//! Linux uses fixed-point (11.11 or 11.21 format) EWMA:
//!
//! ```text
//! load(t) = load(t-1) * exp(-5/period) + runnable * (1 - exp(-5/period))
//! ```
//!
//! Where `period` is 60, 300, or 900 seconds.  We pre-compute the
//! decay factors in fixed-point (shift 11 bits = multiply by 2048).
//!
//! ## Sampling
//!
//! The timer softirq calls [`sample`] every [`SAMPLE_INTERVAL_TICKS`]
//! ticks (500 ticks = 5 seconds at 100 Hz).  Each sample reads the
//! current number of runnable tasks from the scheduler and updates
//! the three EWMAs.
//!
//! ## Usage
//!
//! ```ignore
//! let (one, five, fifteen) = loadavg::get();
//! // Values are fixed-point (shift 11).  To display:
//! let whole = one >> FSHIFT;
//! let frac = ((one & FMASK) * 100) >> FSHIFT;
//! println!("{}.{:02}", whole, frac);
//! ```
//!
//! ## References
//!
//! - Linux `kernel/sched/loadavg.c`
//! - Neil Gunther, "UNIX Load Average Part 1" (1993)
//! - Brendan Gregg, "Linux Load Averages: Solving the Mystery" (2017)

use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Fixed-point constants
// ---------------------------------------------------------------------------

/// Fixed-point shift (number of fractional bits).
/// 11 bits gives ~0.0005 precision — matches Linux's FSHIFT.
const FSHIFT: u32 = 11;

/// Fixed-point scale factor (2^FSHIFT = 2048).
const FIXED_1: u64 = 1 << FSHIFT;

/// Mask for extracting the fractional part.
#[allow(dead_code)]
const FMASK: u64 = FIXED_1 - 1;

/// Decay factor for 1-minute load average.
///
/// exp(-5/60) ≈ 0.9200 → 0.9200 * 2048 ≈ 1884
const EXP_1: u64 = 1884;

/// Decay factor for 5-minute load average.
///
/// exp(-5/300) ≈ 0.9835 → 0.9835 * 2048 ≈ 2014
const EXP_5: u64 = 2014;

/// Decay factor for 15-minute load average.
///
/// exp(-5/900) ≈ 0.9945 → 0.9945 * 2048 ≈ 2037
const EXP_15: u64 = 2037;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Sample interval in APIC timer ticks (at 100 Hz).
/// 500 ticks = 5 seconds (matches Linux's LOAD_FREQ).
pub const SAMPLE_INTERVAL_TICKS: u64 = 500;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// 1-minute load average (fixed-point, shift 11).
static LOAD_1: AtomicU64 = AtomicU64::new(0);

/// 5-minute load average (fixed-point, shift 11).
static LOAD_5: AtomicU64 = AtomicU64::new(0);

/// 15-minute load average (fixed-point, shift 11).
static LOAD_15: AtomicU64 = AtomicU64::new(0);

/// Total number of samples taken since boot.
static SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// EWMA update
// ---------------------------------------------------------------------------

/// Update a single load average value using EWMA.
///
/// Formula: `load = load * exp_factor / FIXED_1 + active * (FIXED_1 - exp_factor) / FIXED_1`
///
/// Simplified (avoids overflow for reasonable task counts < 2^20):
/// `load = (load * exp + active * (FIXED_1 - exp)) / FIXED_1`
#[inline]
fn calc_load(old: u64, exp: u64, active: u64) -> u64 {
    // Convert active count to fixed-point.
    let active_fixed = active.saturating_mul(FIXED_1);

    if active_fixed >= old {
        // Load is increasing: use the complement formula to avoid underflow.
        // new = old * exp / FIXED_1 + active_fixed - active_fixed * exp / FIXED_1
        //     = old * exp / FIXED_1 + active_fixed * (FIXED_1 - exp) / FIXED_1
        let decay = old.saturating_mul(exp) / FIXED_1;
        let growth = active_fixed.saturating_sub(active_fixed.saturating_mul(exp) / FIXED_1);
        decay.saturating_add(growth)
    } else {
        // Load is decreasing.
        // new = old - (old - active_fixed) * (FIXED_1 - exp) / FIXED_1
        let diff = old.saturating_sub(active_fixed);
        let decrease = diff.saturating_mul(FIXED_1.saturating_sub(exp)) / FIXED_1;
        old.saturating_sub(decrease)
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Sample the current system load and update the averages.
///
/// Called from the timer softirq every [`SAMPLE_INTERVAL_TICKS`] ticks.
/// Reads the number of runnable tasks from the scheduler.
pub fn sample() {
    // Count runnable tasks (Ready + Running states).
    let sched = crate::sched::sched_stats();
    let runnable = sched.total_tasks_spawned
        .saturating_sub(sched.total_tasks_exited);

    // Update the three load averages.
    let old_1 = LOAD_1.load(Ordering::Relaxed);
    let old_5 = LOAD_5.load(Ordering::Relaxed);
    let old_15 = LOAD_15.load(Ordering::Relaxed);

    let new_1 = calc_load(old_1, EXP_1, runnable);
    let new_5 = calc_load(old_5, EXP_5, runnable);
    let new_15 = calc_load(old_15, EXP_15, runnable);

    LOAD_1.store(new_1, Ordering::Relaxed);
    LOAD_5.store(new_5, Ordering::Relaxed);
    LOAD_15.store(new_15, Ordering::Relaxed);

    SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Get the current load averages as fixed-point values (shift 11).
///
/// Returns `(load_1, load_5, load_15)` — the 1-minute, 5-minute,
/// and 15-minute exponential moving averages.
#[must_use]
pub fn get() -> (u64, u64, u64) {
    (
        LOAD_1.load(Ordering::Relaxed),
        LOAD_5.load(Ordering::Relaxed),
        LOAD_15.load(Ordering::Relaxed),
    )
}

/// Format a fixed-point load average value as "X.XX".
///
/// Returns the whole and fractional (hundredths) parts.
#[must_use]
pub fn format_load(val: u64) -> (u64, u64) {
    let whole = val >> FSHIFT;
    let frac = (val & (FIXED_1 - 1)).saturating_mul(100) >> FSHIFT;
    (whole, frac)
}

/// Total number of samples taken since boot.
#[must_use]
pub fn sample_count() -> u64 {
    SAMPLE_COUNT.load(Ordering::Relaxed)
}

/// Get the number of currently runnable tasks (snapshot).
///
/// This is the instantaneous value, not an average.
#[must_use]
pub fn nr_running() -> u64 {
    let sched = crate::sched::sched_stats();
    sched.total_tasks_spawned
        .saturating_sub(sched.total_tasks_exited)
}
