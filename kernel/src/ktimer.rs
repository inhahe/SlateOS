//! Kernel timers — delayed and periodic callback execution.
//!
//! Provides a mechanism to schedule a function call after a specified
//! delay (in ticks), without blocking the calling context.  When a
//! timer fires, the callback is submitted to the workqueue for
//! execution in process context (where sleeping and allocation are
//! permitted).
//!
//! ## When to Use
//!
//! - **`sleep_until_tick`**: Blocks the *calling task* until the deadline.
//!   Only useful if you want the current task to wait.
//! - **`ipc::timer`**: Userspace-facing timers that deliver events to
//!   completion ports.  Not for internal kernel use.
//! - **`ktimer`**: Fire-and-forget kernel callbacks.  The caller
//!   continues immediately; the callback runs later on the workqueue
//!   worker task.  Ideal for retries, deferred cleanup, periodic
//!   housekeeping, and timeout handling.
//!
//! ## Design
//!
//! A fixed array of timer entries, scanned lock-free by the TIMER
//! softirq on every tick.  When an entry's deadline is reached, its
//! callback is submitted to the workqueue and the entry is cleared
//! (one-shot) or rearmed (periodic).
//!
//! The array is lock-free: entries use atomic fields for the deadline,
//! function pointer, argument, and interval.  Allocation uses CAS on
//! the deadline field (0 = free slot).  This means `schedule()` is
//! safe to call from any context (ISR, softirq, normal task).
//!
//! ## Capacity
//!
//! Up to [`MAX_TIMERS`] concurrent timers.  If all slots are full,
//! `schedule()` returns `None`.  The caller should handle this —
//! either retry later or use a different approach.
//!
//! ## Accuracy
//!
//! Timer resolution is 1 tick (10 ms at 100 Hz).  A timer scheduled
//! with delay=1 will fire on the *next* tick (within 0-10 ms).  This
//! is adequate for kernel housekeeping; sub-millisecond timing requires
//! the HPET directly.
//!
//! ## References
//!
//! - Linux `kernel/time/timer.c` — `add_timer()`, `mod_timer()`,
//!   `del_timer()`
//! - Linux `include/linux/workqueue.h` — `schedule_delayed_work()`
//! - FreeBSD `sys/kern/kern_timeout.c` — callout framework

use core::sync::atomic::{AtomicU64, Ordering};

use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of concurrent kernel timers.
///
/// 128 entries × 24 bytes = 3 KiB.  Sufficient for kernel housekeeping
/// timers (cache writeback, watchdog rearm, retry loops, periodic stats).
const MAX_TIMERS: usize = 128;

// ---------------------------------------------------------------------------
// Timer handle
// ---------------------------------------------------------------------------

/// Opaque handle to a scheduled timer.
///
/// Used to cancel a timer before it fires.  The handle is a generation
/// counter that distinguishes reuse of the same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerHandle(u64);

/// Generation counter for handles.  Monotonically increasing.
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Timer entry
// ---------------------------------------------------------------------------

/// A single timer slot in the global table.
///
/// All fields are atomic for lock-free access from ISR/softirq context.
///
/// Slot lifecycle:
/// - **Free**: `deadline == 0`
/// - **Armed**: `deadline > 0`, waiting for tick_count to reach it
/// - **Fired**: callback submitted to workqueue, slot cleared (one-shot)
///   or rearmed (periodic: deadline += interval)
struct TimerEntry {
    /// Tick count at which this timer fires.  0 = slot is free.
    deadline: AtomicU64,
    /// The function to call (stored as a raw u64 for atomic access).
    func: AtomicU64,
    /// Argument passed to the function.
    arg: AtomicU64,
    /// Interval for periodic timers (in ticks).  0 = one-shot.
    interval: AtomicU64,
    /// Handle generation — identifies this particular timer instance.
    handle: AtomicU64,
}

impl TimerEntry {
    const fn new() -> Self {
        Self {
            deadline: AtomicU64::new(0),
            func: AtomicU64::new(0),
            arg: AtomicU64::new(0),
            interval: AtomicU64::new(0),
            handle: AtomicU64::new(0),
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// The global timer table.  Scanned on every tick by the softirq handler.
static TIMERS: [TimerEntry; MAX_TIMERS] = [const { TimerEntry::new() }; MAX_TIMERS];

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Total timers scheduled since boot.
static TIMERS_SCHEDULED: AtomicU64 = AtomicU64::new(0);

/// Total timers that fired (callback submitted to workqueue).
static TIMERS_FIRED: AtomicU64 = AtomicU64::new(0);

/// Total timers cancelled before firing.
static TIMERS_CANCELLED: AtomicU64 = AtomicU64::new(0);

/// Total times schedule() failed (table full).
static SCHEDULE_FAILURES: AtomicU64 = AtomicU64::new(0);

/// Total times a fired timer couldn't be submitted (workqueue full).
static SUBMIT_FAILURES: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Schedule a one-shot timer.
///
/// After `delay_ticks` timer ticks (each ~10 ms), `func(arg)` will be
/// called on the workqueue worker task.  The caller continues immediately.
///
/// Returns a [`TimerHandle`] that can be used with [`cancel()`], or
/// `None` if the timer table is full.
///
/// # Parameters
///
/// - `func`: Function to call when the timer fires.
/// - `arg`: Argument passed to `func`.
/// - `delay_ticks`: Number of ticks to wait.  Minimum 1 (fires on next
///   tick).  0 is treated as 1.
///
/// # Safety note
///
/// Safe to call from any context (ISR, softirq, or normal task).
pub fn schedule(func: fn(u64), arg: u64, delay_ticks: u64) -> Option<TimerHandle> {
    schedule_internal(func, arg, delay_ticks, 0)
}

/// Schedule a one-shot timer using a delay in **nanoseconds**.
///
/// The kernel's own deadlines are almost all expressed in nanoseconds (the
/// HPET's unit), while this table counts ticks. Converting at each call site
/// means every caller carries a copy of the tick rate, and every one of those
/// copies is wrong the day [`crate::apic::TICK_RATE_HZ`] changes. Doing it here
/// keeps that arithmetic in the one place that owns the tick.
///
/// The delay is rounded **up** to the next whole tick, and never below one:
/// firing early would defeat the purpose of every caller that has one (a
/// backoff that expires sooner than requested is not a backoff), and a delay
/// short enough to round to zero still means "not now".
#[must_use]
pub fn schedule_after_ns(func: fn(u64), arg: u64, delay_ns: u64) -> Option<TimerHandle> {
    schedule(func, arg, ns_to_ticks(delay_ns))
}

/// Convert a nanosecond duration to whole ticks, rounding up, minimum 1.
#[must_use]
pub fn ns_to_ticks(delay_ns: u64) -> u64 {
    const NS_PER_TICK: u64 = 1_000_000_000 / crate::apic::TICK_RATE_HZ as u64;
    // Round up: `(n + d - 1) / d`. Saturating on the add so a near-`u64::MAX`
    // delay yields a huge tick count rather than wrapping to a tiny one --
    // "effectively never" must not become "immediately".
    let ticks = delay_ns
        .saturating_add(NS_PER_TICK.saturating_sub(1))
        .checked_div(NS_PER_TICK)
        .unwrap_or(1);
    ticks.max(1)
}

/// Schedule a periodic timer.
///
/// Fires `func(arg)` every `interval_ticks` ticks, starting after the
/// first interval.  Continues until cancelled via [`cancel()`].
///
/// Returns a [`TimerHandle`] or `None` if the table is full.
///
/// # Parameters
///
/// - `func`: Function to call on each firing.
/// - `arg`: Argument passed to `func`.
/// - `interval_ticks`: Period in ticks.  Minimum 1.
pub fn schedule_periodic(func: fn(u64), arg: u64, interval_ticks: u64) -> Option<TimerHandle> {
    let interval = if interval_ticks == 0 {
        1
    } else {
        interval_ticks
    };
    schedule_internal(func, arg, interval, interval)
}

/// Cancel a timer before it fires.
///
/// Returns `true` if the timer was found and cancelled, `false` if it
/// had already fired or the handle is invalid.
///
/// Safe to call from any context.
pub fn cancel(handle: TimerHandle) -> bool {
    for entry in &TIMERS {
        if entry.handle.load(Ordering::Acquire) == handle.0 {
            // Found it.  Clear the deadline to mark the slot as free.
            // Use CAS to avoid racing with process_expirations().
            let deadline = entry.deadline.load(Ordering::Acquire);
            if deadline == 0 {
                return false; // Already fired.
            }
            // Try to clear it.  If someone else (softirq) cleared it
            // first, that's fine — it means it just fired.
            if entry
                .deadline
                .compare_exchange(deadline, 0, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                // Successfully cancelled.  Clear other fields.
                entry.func.store(0, Ordering::Relaxed);
                entry.arg.store(0, Ordering::Relaxed);
                entry.interval.store(0, Ordering::Relaxed);
                entry.handle.store(0, Ordering::Release);
                TIMERS_CANCELLED.fetch_add(1, Ordering::Relaxed);
                return true;
            }
            // CAS failed — timer fired between our load and CAS.
            return false;
        }
    }
    false // Handle not found.
}

/// Number of currently armed timers.
#[must_use]
#[allow(dead_code)]
pub fn active_count() -> usize {
    let mut count = 0usize;
    for entry in &TIMERS {
        if entry.deadline.load(Ordering::Relaxed) != 0 {
            count = count.saturating_add(1);
        }
    }
    count
}

/// Total timers scheduled since boot.
#[must_use]
#[allow(dead_code)]
pub fn scheduled_count() -> u64 {
    TIMERS_SCHEDULED.load(Ordering::Relaxed)
}

/// Total timers that fired since boot.
#[must_use]
#[allow(dead_code)]
pub fn fired_count() -> u64 {
    TIMERS_FIRED.load(Ordering::Relaxed)
}

/// Total timers cancelled since boot.
#[must_use]
#[allow(dead_code)]
pub fn cancelled_count() -> u64 {
    TIMERS_CANCELLED.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------------------
// Internal
// ---------------------------------------------------------------------------

/// Core scheduling logic shared by one-shot and periodic timers.
fn schedule_internal(
    func: fn(u64),
    arg: u64,
    delay_ticks: u64,
    interval: u64,
) -> Option<TimerHandle> {
    let delay = if delay_ticks == 0 { 1 } else { delay_ticks };
    let now = crate::apic::tick_count();
    let deadline = now.saturating_add(delay);

    // Allocate a handle.
    let handle_val = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);

    // Find a free slot (deadline == 0) via CAS.
    for entry in &TIMERS {
        if entry.deadline.load(Ordering::Relaxed) == 0 {
            // Try to claim this slot.
            if entry
                .deadline
                .compare_exchange(0, deadline, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
            {
                // Slot claimed.  Fill in the other fields.
                // SAFETY: We hold the slot (deadline != 0), so no other
                // allocator will touch these fields.  The softirq scanner
                // only reads func/arg *after* it sees deadline != 0, and
                // we use Release ordering on the deadline CAS above, so
                // the stores below will be visible when the scanner reads
                // the deadline with Acquire.
                entry
                    .func
                    .store(func as *const () as u64, Ordering::Relaxed);
                entry.arg.store(arg, Ordering::Relaxed);
                entry.interval.store(interval, Ordering::Relaxed);
                entry.handle.store(handle_val, Ordering::Release);

                TIMERS_SCHEDULED.fetch_add(1, Ordering::Relaxed);
                return Some(TimerHandle(handle_val));
            }
            // CAS failed — another thread grabbed this slot.  Keep looking.
        }
    }

    // Table full.
    SCHEDULE_FAILURES.fetch_add(1, Ordering::Relaxed);
    None
}

// ---------------------------------------------------------------------------
// Tick processing — called from TIMER softirq
// ---------------------------------------------------------------------------

/// Process expired kernel timers.
///
/// Called from the TIMER softirq handler on every tick.  Scans the
/// timer table and submits expired timers' callbacks to the workqueue.
///
/// For periodic timers, the deadline is advanced by the interval (the
/// slot remains occupied).  For one-shot timers, the slot is freed.
///
/// This function is lock-free and safe to call from softirq context.
#[allow(clippy::arithmetic_side_effects)]
pub fn process_expirations() {
    let now = crate::apic::tick_count();

    for entry in &TIMERS {
        let deadline = entry.deadline.load(Ordering::Acquire);
        if deadline == 0 || now < deadline {
            continue; // Free slot or not yet due.
        }

        // Timer has expired.  Read the callback info.
        let func_raw = entry.func.load(Ordering::Acquire);
        let arg = entry.arg.load(Ordering::Relaxed);
        let interval = entry.interval.load(Ordering::Relaxed);

        if func_raw == 0 {
            // Slot is being set up or was cancelled.  Skip.
            continue;
        }

        // Defense-in-depth: reject a non-zero-but-implausible callback (not a
        // `.text` address).  A validly-scheduled `fn(u64)` always points into
        // kernel code; a value that isn't means the slot was corrupted (heap
        // overrun / torn store).  Submitting it to the workqueue would later
        // jump the worker to a wild address — the B-KNULLJUMP-SIGNAL class.
        // Log which subsystem's arg was involved, free the slot, and skip.
        if !crate::idt::is_kernel_text(func_raw) {
            serial_println!(
                "[ktimer] CRITICAL: refusing to submit corrupt timer callback \
                 addr={:#x} arg={:#x} — slot corruption; freeing (see B-KNULLJUMP-SIGNAL)",
                func_raw,
                entry.arg.load(Ordering::Relaxed)
            );
            entry.deadline.store(0, Ordering::Release);
            entry.func.store(0, Ordering::Relaxed);
            entry.arg.store(0, Ordering::Relaxed);
            entry.interval.store(0, Ordering::Relaxed);
            entry.handle.store(0, Ordering::Release);
            continue;
        }

        if interval > 0 {
            // Periodic: advance deadline.  Don't free the slot.
            let new_deadline = now.saturating_add(interval);
            entry.deadline.store(new_deadline, Ordering::Release);
            // A re-arm *is* an arming, and is counted as one — otherwise
            // `TIMERS_FIRED` counts every firing of a periodic timer while
            // `TIMERS_SCHEDULED` counts only its first arming, and the two
            // stop being commensurable: boots printed `scheduled=5, fired=8`,
            // which reads as three wakeups conjured from nowhere.  hrtimer had
            // the identical asymmetry and there it was worse than misleading —
            // it structurally disabled a `scheduled - fired - cancelled -
            // pending` tripwire, since the `saturating_sub` chain floors at 0
            // once `fired` outruns `scheduled` (see hrtimer.rs's re-arm site
            // and Test 10).  ktimer has no such tripwire *today*; count the
            // re-arm anyway, so that adding one later cannot inherit a
            // guard that can never fire.
            TIMERS_SCHEDULED.fetch_add(1, Ordering::Relaxed);
        } else {
            // One-shot: free the slot.
            entry.deadline.store(0, Ordering::Release);
            entry.func.store(0, Ordering::Relaxed);
            entry.arg.store(0, Ordering::Relaxed);
            entry.interval.store(0, Ordering::Relaxed);
            entry.handle.store(0, Ordering::Release);
        }

        // Submit to workqueue.
        // SAFETY: func_raw was stored from a valid fn(u64) pointer.
        let func: fn(u64) = unsafe { core::mem::transmute(func_raw) };
        let submitted = crate::workqueue::submit(func, arg);
        if submitted {
            TIMERS_FIRED.fetch_add(1, Ordering::Relaxed);
        } else {
            SUBMIT_FAILURES.fetch_add(1, Ordering::Relaxed);
            // Workqueue full — the callback is lost for one-shot timers.
            // For periodic, it will try again on the next interval.
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the kernel timer subsystem.
///
/// Verifies:
/// 1. One-shot timers fire after the correct delay.
/// 2. Periodic timers fire repeatedly.
/// 3. Cancellation works.
/// 4. Statistics are accurate.
pub fn self_test() {
    use core::sync::atomic::AtomicU64;

    serial_println!("[ktimer] Running self-test...");

    // --- 1. One-shot timer ---
    static TEST_ONESHOT: AtomicU64 = AtomicU64::new(0);

    fn oneshot_cb(arg: u64) {
        TEST_ONESHOT.fetch_add(arg, Ordering::Relaxed);
    }

    TEST_ONESHOT.store(0, Ordering::Relaxed);
    let before_sched = scheduled_count();

    let h = schedule(oneshot_cb, 42, 2);
    assert!(h.is_some(), "schedule() should succeed");
    assert!(
        scheduled_count() > before_sched,
        "scheduled count should increase",
    );
    serial_println!("[ktimer]   One-shot scheduled: OK");

    // Wait for it to fire (2 ticks = ~20ms, plus workqueue latency).
    // Use sleep_until_tick to actually wait for real timer ticks to pass,
    // then yield to let the workqueue worker execute the callback.
    let now = crate::apic::tick_count();
    crate::sched::sleep_until_tick(now.saturating_add(5));
    for _ in 0..10 {
        crate::sched::yield_now();
    }

    let val = TEST_ONESHOT.load(Ordering::Relaxed);
    if val == 42 {
        serial_println!("[ktimer]   One-shot fired: OK (val=42)");
    } else {
        // Timing-dependent — may need more yields under heavy load.
        serial_println!(
            "[ktimer]   One-shot fired: pending (val={}, may need more time)",
            val,
        );
    }

    // --- 2. Periodic timer ---
    static TEST_PERIODIC: AtomicU64 = AtomicU64::new(0);

    fn periodic_cb(_arg: u64) {
        TEST_PERIODIC.fetch_add(1, Ordering::Relaxed);
    }

    TEST_PERIODIC.store(0, Ordering::Relaxed);
    let h_periodic = schedule_periodic(periodic_cb, 0, 2);
    assert!(h_periodic.is_some(), "schedule_periodic() should succeed");

    // Wait for several firings (2-tick interval, wait 12 ticks = ~120ms).
    let now2 = crate::apic::tick_count();
    crate::sched::sleep_until_tick(now2.saturating_add(12));
    for _ in 0..10 {
        crate::sched::yield_now();
    }

    let count = TEST_PERIODIC.load(Ordering::Relaxed);
    serial_println!("[ktimer]   Periodic timer: {} firings", count);

    // Cancel the periodic timer.
    let cancelled = cancel(h_periodic.unwrap());
    // Note: may have already been cancelled by natural slot reuse, so
    // we check that cancellation returns true OR the timer has stopped.
    if cancelled {
        serial_println!("[ktimer]   Periodic cancel: OK");
    } else {
        serial_println!("[ktimer]   Periodic cancel: already fired/done");
    }

    // --- 3. Cancel before firing ---
    static TEST_CANCEL: AtomicU64 = AtomicU64::new(0);

    fn cancel_cb(_arg: u64) {
        TEST_CANCEL.fetch_add(1, Ordering::Relaxed);
    }

    TEST_CANCEL.store(0, Ordering::Relaxed);
    let h_cancel = schedule(cancel_cb, 0, 100); // Fire in 1 second.
    assert!(h_cancel.is_some());

    // Cancel immediately.
    let did_cancel = cancel(h_cancel.unwrap());
    assert!(did_cancel, "Should cancel a timer with distant deadline");
    serial_println!("[ktimer]   Cancel before firing: OK");

    // Verify it doesn't fire.
    for _ in 0..10 {
        crate::sched::yield_now();
    }
    assert_eq!(
        TEST_CANCEL.load(Ordering::Relaxed),
        0,
        "Cancelled timer should not fire",
    );
    serial_println!("[ktimer]   Cancelled timer did not fire: OK");

    // --- 4. Stats ---
    //
    // Read `fired`/`cancelled` **before** `scheduled`, and never the other way
    // round.  A firing increments `TIMERS_SCHEDULED` (the re-arm) before it
    // increments `TIMERS_FIRED` (the workqueue submit), so sampling the
    // consequence first and the cause last makes a concurrent expiry on
    // another CPU only ever bias the check *safe* — the reverse order would
    // manufacture a spurious failure whenever a periodic timer fired between
    // the two loads.
    let stats_fired = fired_count();
    let stats_cancel = cancelled_count();
    let stats_sched = scheduled_count();
    assert!(stats_sched > 0);
    // Conservation: every firing and every cancellation is preceded by exactly
    // one arming, so armings can never be outnumbered by their own outcomes.
    //
    // This is a real tripwire and not decoration: until 2026-08-21 a periodic
    // timer's re-arm was not counted as an arming, boots printed
    // `scheduled=5, fired=8`, and *this assertion would have failed* — which is
    // the only kind of evidence worth having that a guard can fire at all.
    // hrtimer had the same asymmetry, and there it silently disabled a guard
    // written specifically to catch a bug that had already happened once,
    // because that one was phrased as a `saturating_sub` chain and so read 0
    // on a healthy boot and on a broken one alike.  Phrase conservation checks
    // as a comparison, never as a clamped subtraction.
    let outcomes = stats_fired.saturating_add(stats_cancel);
    assert!(
        stats_sched >= outcomes,
        "ktimer accounting: scheduled={stats_sched} but fired+cancelled={outcomes} — \
         re-arms of periodic timers are not being counted as armings"
    );
    serial_println!(
        "[ktimer]   Stats: scheduled={}, fired={}, cancelled={}",
        stats_sched,
        stats_fired,
        stats_cancel,
    );

    serial_println!("[ktimer] Self-test PASSED");
}
