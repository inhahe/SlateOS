//! Kernel timers for completion port integration.
//!
//! A timer is a one-shot or periodic object that fires after a deadline.
//! Timers can be registered with completion ports, enabling event-driven
//! programs to multiplex I/O readiness with timeouts.
//!
//! ## Design
//!
//! Timers are scanned by the APIC timer ISR every tick (10ms at 100Hz).
//! When a timer expires:
//! - Its `expired` flag is set atomically.
//! - If it has a registered completion port, `notify()` is called to
//!   wake any blocked waiter.
//! - Periodic timers re-arm automatically by advancing the deadline.
//!
//! ## Performance
//!
//! Timer scan is O(MAX_TIMERS) per tick.  At 256 timers and 100Hz,
//! this is ~25,600 atomic loads per second — trivially fast.
//!
//! ## Syscalls
//!
//! - `SYS_TIMER_CREATE(duration_ns, flags)` — create a one-shot or
//!   periodic timer.  Returns a timer handle.
//! - `SYS_TIMER_CANCEL(handle)` — cancel and destroy a timer.
//!
//! Timers can also be registered with completion ports via
//! `SYS_CP_REGISTER(cp, 5, timer_handle, user_data)`.

// Timer API surface includes counters, configuration constants, and
// debug accessors that aren't all wired up to syscalls yet.
#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crate::serial_println;

// ---------------------------------------------------------------------------
// Timer table — fixed-size, lock-free for ISR access
// ---------------------------------------------------------------------------

/// Maximum number of concurrent timers.
const MAX_TIMERS: usize = 256;

/// Flag bit: timer is periodic (re-arms after firing).
pub const TIMER_PERIODIC: u64 = 1 << 0;

/// A single timer entry.
///
/// All fields are atomic for lock-free ISR access.
struct TimerEntry {
    /// Non-zero when this slot is in use.  Holds the timer handle ID.
    handle: AtomicU64,
    /// Tick count at which the timer fires.  0 = inactive.
    deadline: AtomicU64,
    /// Set to true when the deadline has passed.
    expired: AtomicBool,
    /// Interval in ticks for periodic timers.  0 = one-shot.
    interval: AtomicU64,
    /// Completion port handle to notify on expiry.  0 = no CP.
    cp_handle: AtomicU64,
}

impl TimerEntry {
    const fn new() -> Self {
        Self {
            handle: AtomicU64::new(0),
            deadline: AtomicU64::new(0),
            expired: AtomicBool::new(false),
            interval: AtomicU64::new(0),
            cp_handle: AtomicU64::new(0),
        }
    }
}

/// The global timer table.
static TIMER_TABLE: [TimerEntry; MAX_TIMERS] = {
    const EMPTY: TimerEntry = TimerEntry::new();
    [EMPTY; MAX_TIMERS]
};

/// Counter for generating unique timer handles.
/// Starts at 1 so 0 means "no timer".
static NEXT_TIMER_ID: AtomicU64 = AtomicU64::new(1);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new timer.
///
/// `duration_ns`: time until first expiry (nanoseconds).
/// `flags`: `TIMER_PERIODIC` for repeating timers, 0 for one-shot.
///
/// Returns the timer handle on success, or `ResourceExhausted` if
/// the timer table is full.
pub fn create(duration_ns: u64, flags: u64) -> crate::error::KernelResult<u64> {
    use crate::error::KernelError;

    // Convert nanoseconds to ticks (100Hz = 10ms per tick), rounding up.
    let ticks = duration_ns
        .saturating_add(9_999_999)
        .saturating_div(10_000_000)
        .max(1);

    let now = crate::apic::tick_count();
    let deadline = now.saturating_add(ticks);
    let interval = if flags & TIMER_PERIODIC != 0 { ticks } else { 0 };

    let handle = NEXT_TIMER_ID.fetch_add(1, Ordering::Relaxed);

    // Find an empty slot.
    for entry in &TIMER_TABLE {
        if entry
            .handle
            .compare_exchange(0, handle, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
        {
            entry.deadline.store(deadline, Ordering::Release);
            entry.expired.store(false, Ordering::Release);
            entry.interval.store(interval, Ordering::Release);
            entry.cp_handle.store(0, Ordering::Release);
            return Ok(handle);
        }
    }

    // No slots available.
    serial_println!("[timer] WARNING: timer table full ({} timers)", MAX_TIMERS);
    Err(KernelError::ResourceExhausted)
}

/// Cancel and destroy a timer.
///
/// Returns `true` if the timer was found and cancelled.
pub fn cancel(handle: u64) -> bool {
    if handle == 0 {
        return false;
    }

    for entry in &TIMER_TABLE {
        if entry.handle.load(Ordering::Acquire) == handle {
            // Clear the slot.
            entry.deadline.store(0, Ordering::Release);
            entry.expired.store(false, Ordering::Release);
            entry.interval.store(0, Ordering::Release);
            entry.cp_handle.store(0, Ordering::Release);
            entry.handle.store(0, Ordering::Release);
            return true;
        }
    }

    false
}

/// Check if a timer has expired.
///
/// Used by the completion port polling path.
pub fn is_expired(handle: u64) -> bool {
    if handle == 0 {
        return false;
    }

    for entry in &TIMER_TABLE {
        if entry.handle.load(Ordering::Acquire) == handle {
            return entry.expired.load(Ordering::Acquire);
        }
    }

    // Timer not found — treat as expired (gone = done).
    true
}

/// Associate a completion port with a timer.
///
/// When the timer expires, the completion port will be notified.
/// Called internally when a timer is registered with a CP via
/// `CP_REGISTER`.
pub fn set_cp(handle: u64, cp_handle: u64) {
    for entry in &TIMER_TABLE {
        if entry.handle.load(Ordering::Acquire) == handle {
            entry.cp_handle.store(cp_handle, Ordering::Release);
            return;
        }
    }
}

/// Reset a timer's expired flag (consume the expiry).
///
/// For one-shot timers, this is called after the CP event is delivered.
/// For periodic timers, the ISR resets it automatically.
pub fn acknowledge(handle: u64) {
    for entry in &TIMER_TABLE {
        if entry.handle.load(Ordering::Acquire) == handle {
            entry.expired.store(false, Ordering::Release);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// ISR path — called from APIC timer interrupt
// ---------------------------------------------------------------------------

/// Scan all timers and fire any that have expired.
///
/// Called from the APIC timer ISR on every tick.  Lock-free: only
/// atomic loads/stores + completion port notify (which acquires its
/// own lock via try_lock pattern).
///
/// For timers with an associated completion port, posts a notification
/// event so any task blocked in `CP_WAIT` will be woken.
pub fn process_timer_expirations() {
    let now = crate::apic::tick_count();

    for entry in &TIMER_TABLE {
        let handle = entry.handle.load(Ordering::Acquire);
        if handle == 0 {
            continue; // Empty slot.
        }

        let deadline = entry.deadline.load(Ordering::Acquire);
        if deadline == 0 || now < deadline {
            continue; // Not yet or inactive.
        }

        // Timer has fired.  We run in softirq (timer-ISR) context, so we must
        // NOT block on `CP_TABLE` / `SCHED` here — a blocking wake would spin
        // forever if this softirq preempted a task that already holds `SCHED`
        // (see `known-issues.md` B-COMPLETION-TIMER-IRQ-DEADLOCK).  Deliver the
        // completion-port notification with non-blocking locks first, and only
        // advance/expire the timer once it succeeds.  If a lock was contended,
        // leave the timer un-advanced so the next tick retries — this avoids
        // both a lost wakeup and a duplicated event.
        let cp_raw = entry.cp_handle.load(Ordering::Acquire);
        if cp_raw != 0 {
            let cp = super::completion::CpHandle::from_raw(cp_raw);
            let source = super::completion::WaitSource::Timer(handle);
            if !super::completion::try_notify(cp, source) {
                continue; // Contended — retry next tick; don't advance/expire.
            }
        }

        let interval = entry.interval.load(Ordering::Acquire);
        if interval > 0 {
            // Periodic: advance the deadline and mark expired.
            let next_deadline = now.saturating_add(interval);
            entry.deadline.store(next_deadline, Ordering::Release);
        } else {
            // One-shot: clear the deadline (won't fire again).
            entry.deadline.store(0, Ordering::Release);
        }

        entry.expired.store(true, Ordering::Release);
    }
}

// ---------------------------------------------------------------------------
// Syscalls
// ---------------------------------------------------------------------------

/// Syscall number for SYS_TIMER_CREATE.
pub const SYS_TIMER_CREATE: u64 = 12;

/// Syscall number for SYS_TIMER_CANCEL.
pub const SYS_TIMER_CANCEL: u64 = 13;

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run timer self-tests.
pub fn self_test() -> crate::error::KernelResult<()> {
    serial_println!("[timer] Running timer self-test...");

    // Test 1: Create a one-shot timer with a very short deadline.
    let h = create(1_000_000, 0)?; // 1ms → 1 tick minimum

    // Timer should not be expired immediately (it fires on the next tick).
    // But since the APIC timer ISR fires at 100Hz, we can't rely on
    // timing in a self-test.  Just verify creation succeeded.
    serial_println!("[timer]   Create one-shot timer: OK (handle={})", h);

    // Test 2: Cancel the timer.
    if !cancel(h) {
        serial_println!("[timer]   FAIL: cancel returned false");
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[timer]   Cancel timer: OK");

    // Test 3: Create a periodic timer.
    let h2 = create(50_000_000, TIMER_PERIODIC)?; // 50ms
    serial_println!("[timer]   Create periodic timer: OK (handle={})", h2);

    // Clean up.
    cancel(h2);

    // Test 4: is_expired on non-existent timer → true.
    if !is_expired(99999) {
        serial_println!("[timer]   FAIL: is_expired on bad handle should be true");
        return Err(crate::error::KernelError::InternalError);
    }
    serial_println!("[timer]   is_expired on bad handle: OK (true)");

    serial_println!("[timer] Timer self-test PASSED");
    Ok(())
}
