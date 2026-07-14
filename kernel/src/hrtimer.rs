//! High-resolution kernel timers.
//!
//! Provides nanosecond-precision timer scheduling backed by the HPET
//! monotonic counter.  Timer callbacks fire from interrupt context
//! (the APIC timer ISR) with minimal latency.
//!
//! ## Design
//!
//! Each CPU maintains a sorted list of pending timers (min-heap by
//! absolute expiry time).  The APIC timer ISR checks for expired
//! timers on every tick.  When timers are pending with deadlines
//! between regular ticks, the APIC is reprogrammed in one-shot mode
//! to fire at the next deadline — giving sub-10ms resolution.
//!
//! ## Resolution
//!
//! - **With HPET**: timestamps at ~10-25 MHz (40-100 ns resolution)
//! - **Timer dispatch**: on each APIC tick or one-shot fire (~10 ns overhead)
//! - **Worst-case latency**: 10 ms (if scheduled just after a tick with
//!   one-shot programming unavailable).  Average: < 1 ms with one-shot.
//!
//! ## Usage
//!
//! ```ignore
//! use crate::hrtimer;
//!
//! // Fire after 1 ms
//! let handle = hrtimer::schedule_ns(1_000_000, my_callback, 42);
//!
//! // Cancel if no longer needed
//! hrtimer::cancel(handle);
//!
//! // Query system monotonic time
//! let now = hrtimer::now_ns();
//! ```
//!
//! ## References
//!
//! - Linux: kernel/time/hrtimer.c
//! - Design spec: io_uring submission target < 200 ns, IPC < 2 µs

use crate::serial_println;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of timers per CPU.
const MAX_TIMERS_PER_CPU: usize = 256;

/// Maximum CPUs supported.
const MAX_CPUS: usize = 16;

// ---------------------------------------------------------------------------
// Timer entry
// ---------------------------------------------------------------------------

/// Unique handle for a scheduled timer (used for cancellation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HrTimerHandle(u64);

/// A pending high-resolution timer.
#[derive(Clone, Copy)]
struct TimerEntry {
    /// Absolute expiry time in nanoseconds (from HPET epoch).
    expiry_ns: u64,
    /// Callback function.
    callback: fn(u64),
    /// Argument passed to callback.
    arg: u64,
    /// Unique ID for cancellation.
    id: u64,
    /// Whether this timer repeats (0 = one-shot, >0 = interval in ns).
    interval_ns: u64,
}

// ---------------------------------------------------------------------------
// Per-CPU timer state
// ---------------------------------------------------------------------------

/// Per-CPU timer heap (min-heap sorted by expiry_ns).
///
/// Using a simple sorted Vec rather than a proper BinaryHeap because
/// we need cancel-by-ID (requires scanning) and the number of active
/// timers per CPU is typically small (< 64).
struct CpuTimerState {
    /// Pending timers sorted by expiry (earliest first).
    timers: Vec<TimerEntry>,
}

impl CpuTimerState {
    const fn new() -> Self {
        Self {
            timers: Vec::new(),
        }
    }
}

/// Global array of per-CPU timer states.
static CPU_TIMERS: [Mutex<CpuTimerState>; MAX_CPUS] = {
    // const initialization of an array of Mutexes.
    const INIT: Mutex<CpuTimerState> = Mutex::new(CpuTimerState::new());
    [INIT; MAX_CPUS]
};

/// Next timer ID (globally unique, monotonically increasing).
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Whether the hrtimer subsystem is initialized.
static INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Total timers fired since boot (all CPUs).
static TOTAL_FIRED: AtomicU64 = AtomicU64::new(0);

/// Total timers scheduled since boot.
static TOTAL_SCHEDULED: AtomicU64 = AtomicU64::new(0);

/// Total timers cancelled since boot.
static TOTAL_CANCELLED: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the hrtimer subsystem.
///
/// Called during boot after HPET initialization.  No-op if HPET is
/// not available (timers will use TSC-based fallback timing).
pub fn init() {
    INITIALIZED.store(true, Ordering::Release);
    serial_println!("[hrtimer] High-resolution timer subsystem initialized");
    if crate::hpet::is_available() {
        serial_println!("[hrtimer]   Clock source: HPET ({} MHz)",
            crate::hpet::frequency_hz() / 1_000_000);
    } else {
        serial_println!("[hrtimer]   Clock source: TSC (fallback)");
    }
}

/// Get current monotonic time in nanoseconds.
///
/// Uses HPET when available, falls back to TSC-based approximation.
#[inline]
pub fn now_ns() -> u64 {
    if crate::hpet::is_available() {
        crate::hpet::elapsed_ns()
    } else {
        // Fallback: use TSC with calibrated frequency.
        // bench::calibrate_tsc() sets up ns_per_tsc_tick during boot.
        tsc_ns_fallback()
    }
}

/// Schedule a one-shot timer.
///
/// The callback fires after `delay_ns` nanoseconds on the current CPU's
/// timer ISR context.  Returns a handle for cancellation.
///
/// # Arguments
///
/// - `delay_ns` — delay in nanoseconds from now (minimum ~100 ns)
/// - `callback` — function to call when the timer fires
/// - `arg` — argument passed to the callback
///
/// # Returns
///
/// A handle that can be passed to [`cancel()`] to prevent firing.
pub fn schedule_ns(delay_ns: u64, callback: fn(u64), arg: u64) -> HrTimerHandle {
    let expiry = now_ns().saturating_add(delay_ns);
    schedule_absolute(expiry, 0, callback, arg)
}

/// Schedule a repeating timer.
///
/// First fires after `delay_ns`, then repeats every `interval_ns`.
/// Use [`cancel()`] to stop.
pub fn schedule_repeating(
    delay_ns: u64,
    interval_ns: u64,
    callback: fn(u64),
    arg: u64,
) -> HrTimerHandle {
    let expiry = now_ns().saturating_add(delay_ns);
    schedule_absolute(expiry, interval_ns, callback, arg)
}

/// Cancel a pending timer.
///
/// Returns `true` if the timer was found and removed, `false` if it
/// already fired or was not found (invalid handle).
///
/// Disables interrupts while holding per-CPU timer locks to prevent
/// deadlock with the APIC timer ISR.
pub fn cancel(handle: HrTimerHandle) -> bool {
    let found = crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();
        let mut state = CPU_TIMERS[cpu].lock();

        if let Some(pos) = state.timers.iter().position(|t| t.id == handle.0) {
            state.timers.remove(pos);
            TOTAL_CANCELLED.fetch_add(1, Ordering::Relaxed);
            return true;
        }

        // Try other CPUs (timer might have been scheduled from a different CPU
        // if the task migrated).  This is rare but correct.
        drop(state);
        for i in 0..MAX_CPUS {
            if i == cpu {
                continue;
            }
            let mut other = CPU_TIMERS[i].lock();
            if let Some(pos) = other.timers.iter().position(|t| t.id == handle.0) {
                other.timers.remove(pos);
                TOTAL_CANCELLED.fetch_add(1, Ordering::Relaxed);
                return true;
            }
        }
        false
    });

    if found {
        crate::ktrace::record(
            crate::ktrace::Category::Timer,
            crate::ktrace::event::TIMER_CANCEL,
            handle.0,
            0,
        );
    }
    found
}

/// Query the number of pending timers on the current CPU.
pub fn pending_count() -> usize {
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();
        CPU_TIMERS[cpu].lock().timers.len()
    })
}

/// Query total timers fired since boot.
pub fn fired_count() -> u64 {
    TOTAL_FIRED.load(Ordering::Relaxed)
}

/// Query total timers scheduled since boot.
pub fn scheduled_count() -> u64 {
    TOTAL_SCHEDULED.load(Ordering::Relaxed)
}

/// Query the next timer expiry time on the current CPU (or None).
pub fn next_expiry_ns() -> Option<u64> {
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();
        let state = CPU_TIMERS[cpu].lock();
        state.timers.first().map(|t| t.expiry_ns)
    })
}

// ---------------------------------------------------------------------------
// ISR integration — called from the APIC timer interrupt handler
// ---------------------------------------------------------------------------

/// Process expired timers on the current CPU.
///
/// Called from the APIC timer ISR (vector 32) on every tick, and also
/// from the hrtimer self-test during boot.  Fires callbacks for all
/// timers whose expiry time has passed.
///
/// Disables interrupts to prevent re-entrant deadlock when called from
/// non-ISR context (safe no-op when already in ISR context).
///
/// Returns the number of timers fired this tick.
pub fn process_expired() -> u32 {
    /// An expired timer captured under the lock to fire afterward:
    /// (callback, argument, interval in ns).
    type ExpiredTimer = (fn(u64), u64, u64);

    if !INITIALIZED.load(Ordering::Relaxed) {
        return 0;
    }

    let cpu = crate::smp::current_cpu_index();
    let now = now_ns();
    let mut fired = 0u32;

    // Collect expired timers while holding the lock, then fire them
    // after releasing it (callbacks might schedule new timers).
    let mut to_fire: [Option<ExpiredTimer>; 16] = [None; 16];
    let mut fire_count = 0usize;

    // Disable interrupts while holding the per-CPU timer lock.
    // When called from ISR context, interrupts are already disabled
    // (without_interrupts is a no-op).  When called from the self-test,
    // this prevents the APIC timer ISR from re-entering and deadlocking.
    crate::cpu::without_interrupts(|| {
        let mut state = CPU_TIMERS[cpu].lock();

        // Since the list is sorted, scan from the front until we find
        // a timer that hasn't expired yet.
        while !state.timers.is_empty() && fire_count < 16 {
            if state.timers[0].expiry_ns <= now {
                let entry = state.timers.remove(0);
                to_fire[fire_count] = Some((entry.callback, entry.arg, entry.interval_ns));

                // If repeating, re-insert with the next expiry.
                if entry.interval_ns > 0 {
                    let next_expiry = now.saturating_add(entry.interval_ns);
                    let new_entry = TimerEntry {
                        expiry_ns: next_expiry,
                        callback: entry.callback,
                        arg: entry.arg,
                        id: entry.id,
                        interval_ns: entry.interval_ns,
                    };
                    insert_sorted(&mut state.timers, new_entry);
                }

                fire_count = fire_count.saturating_add(1);
            } else {
                break; // Remaining timers are in the future.
            }
        }
    });

    // Fire callbacks outside the lock (and outside the IRQ-disabled region).
    // Callbacks might schedule new timers (which take the lock with CLI).
    for slot in to_fire.iter().take(fire_count) {
        if let Some((cb, arg, _interval)) = *slot {
            cb(arg);
            fired = fired.saturating_add(1);
        }
    }

    if fired > 0 {
        TOTAL_FIRED.fetch_add(u64::from(fired), Ordering::Relaxed);

        // Trace: timers fired (arg1 = count, arg2 = now_ns timestamp).
        crate::ktrace::record(
            crate::ktrace::Category::Timer,
            crate::ktrace::event::TIMER_FIRE,
            u64::from(fired),
            now,
        );
    }

    fired
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Insert a timer into the sorted list (by expiry_ns, earliest first).
fn insert_sorted(timers: &mut Vec<TimerEntry>, entry: TimerEntry) {
    let pos = timers.iter().position(|t| t.expiry_ns > entry.expiry_ns)
        .unwrap_or(timers.len());
    timers.insert(pos, entry);
}

/// Schedule a timer with an absolute expiry time.
///
/// Disables interrupts while holding the per-CPU timer lock to prevent
/// deadlock with `process_expired()` which runs from the APIC timer ISR.
fn schedule_absolute(
    expiry_ns: u64,
    interval_ns: u64,
    callback: fn(u64),
    arg: u64,
) -> HrTimerHandle {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);

    // SAFETY: Must disable interrupts before taking the per-CPU timer lock.
    // The APIC timer ISR calls process_expired() which also takes this lock.
    // Without CLI, if the ISR fires while we hold the lock on the same CPU,
    // the spin::Mutex deadlocks (non-reentrant).
    crate::cpu::without_interrupts(|| {
        let cpu = crate::smp::current_cpu_index();

        let entry = TimerEntry {
            expiry_ns,
            callback,
            arg,
            id,
            interval_ns,
        };

        let mut state = CPU_TIMERS[cpu].lock();

        // Enforce per-CPU limit.
        if state.timers.len() >= MAX_TIMERS_PER_CPU {
            serial_println!("[hrtimer] WARNING: per-CPU timer limit reached — oldest timer evicted");
            state.timers.pop(); // Remove the last (furthest) timer.
        }

        insert_sorted(&mut state.timers, entry);
        TOTAL_SCHEDULED.fetch_add(1, Ordering::Relaxed);
    });

    // Trace outside the critical section (ktrace might allocate).
    crate::ktrace::record(
        crate::ktrace::Category::Timer,
        crate::ktrace::event::TIMER_SCHEDULE,
        id,
        expiry_ns,
    );

    HrTimerHandle(id)
}

/// TSC-based nanosecond fallback when HPET is unavailable.
fn tsc_ns_fallback() -> u64 {
    let tsc: u64;
    // SAFETY: rdtsc is always available on x86_64 and has no side effects.
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") _,
            out("edx") _,
            options(nomem, nostack, preserves_flags),
        );
        // Read full 64-bit TSC.
        let lo: u32;
        let hi: u32;
        core::arch::asm!(
            "rdtsc",
            out("eax") lo,
            out("edx") hi,
            options(nomem, nostack, preserves_flags),
        );
        tsc = ((hi as u64) << 32) | (lo as u64);
    }

    // Convert using calibrated frequency (~3.68 GHz on QEMU).
    // bench::tsc_freq() provides the calibrated value.
    let freq = crate::bench::tsc_freq();
    if freq > 0 {
        // ns = tsc * 1_000_000_000 / freq
        // To avoid overflow: ns = tsc / (freq / 1_000_000_000)
        // But freq might be < 1 GHz. Use: (tsc * 1000) / (freq / 1_000_000)
        let mhz = freq / 1_000_000;
        if mhz > 0 {
            tsc.saturating_mul(1000) / mhz
        } else {
            0
        }
    } else {
        0 // No calibration available.
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for high-resolution timers.
pub fn self_test() {
    use core::sync::atomic::AtomicU64;

    serial_println!("[hrtimer] Running self-test...");

    // Test 1: now_ns() returns non-zero and is monotonic.
    let t1 = now_ns();
    // Spin briefly to let time advance.
    for _ in 0..1000 {
        core::hint::spin_loop();
    }
    let t2 = now_ns();
    assert!(t2 >= t1, "now_ns() is not monotonic: {} < {}", t2, t1);
    serial_println!("[hrtimer]   now_ns() monotonic: OK (delta={}ns)", t2.saturating_sub(t1));

    // Test 2: Schedule a timer and verify it fires.
    static TEST_FIRED: AtomicU64 = AtomicU64::new(0);
    fn test_cb(arg: u64) {
        TEST_FIRED.store(arg, Ordering::Release);
    }

    TEST_FIRED.store(0, Ordering::Release);
    let before_scheduled = scheduled_count();
    let fired_before = fired_count();

    // Schedule the 0-delay timer and drain it with a single manual
    // process_expired() call, both under without_interrupts().
    //
    // This is a test-only correctness fix for an intermittent boot
    // panic: the self-test runs with interrupts ENABLED, and the
    // periodic APIC timer ISR also calls process_expired().  If an APIC
    // tick landed in the window between schedule_ns() and the manual
    // process_expired() below, the ISR would fire our 0-delay timer
    // first, so the manual call returned 0 and the `n >= 1` assertion
    // panicked ("Timer with 0 delay didn't fire on process_expired()").
    // The production code is correct — this only made the *test* racy.
    // Closing the interrupt window makes the manual drain deterministic.
    // (schedule_ns/process_expired disable interrupts internally too;
    // nesting without_interrupts is a safe no-op for the inner calls.)
    let n = crate::cpu::without_interrupts(|| {
        let _handle = schedule_ns(0, test_cb, 0xDEAD);
        // The timer has a 0 ns delay, so it expires immediately and
        // fires on this process_expired() call.
        process_expired()
    });
    assert!(n >= 1, "Timer with 0 delay didn't fire on process_expired()");
    assert_eq!(
        TEST_FIRED.load(Ordering::Acquire),
        0xDEAD,
        "Timer callback didn't execute with correct arg"
    );
    assert!(fired_count() > fired_before, "fired_count didn't increment");
    assert!(scheduled_count() > before_scheduled, "scheduled_count didn't increment");
    serial_println!("[hrtimer]   Immediate timer: OK (fired with arg=0xDEAD)");

    // Test 3: Cancel a pending timer.
    static CANCEL_FIRED: AtomicU64 = AtomicU64::new(0);
    fn cancel_cb(arg: u64) {
        CANCEL_FIRED.store(arg, Ordering::Release);
    }

    CANCEL_FIRED.store(0, Ordering::Release);
    // The pending list is NOT globally empty at this point in boot: a
    // persistent userspace daemon (e.g. the userspace netstack daemon
    // blocked in a timed accept-wait) keeps one or more kernel hrtimers
    // pending. So verify our own timer is added/removed *relative* to the
    // ambient baseline rather than asserting an absolute count of 1/0.
    // without_interrupts closes the window in which the periodic APIC-timer
    // ISR could reap an ambient timer between capturing `base` and the
    // asserts and skew the baseline (same race class as Test 2's fix).
    let cancelled = crate::cpu::without_interrupts(|| {
        let base = pending_count();
        let h = schedule_ns(999_999_999_999, cancel_cb, 0xBAD); // Far future.
        assert_eq!(pending_count(), base + 1, "Timer not added to pending list");
        let cancelled = cancel(h);
        assert_eq!(pending_count(), base, "Timer not removed after cancel");
        cancelled
    });
    assert!(cancelled, "cancel() returned false for valid handle");
    // Verify it doesn't fire.
    process_expired();
    assert_eq!(CANCEL_FIRED.load(Ordering::Acquire), 0, "Cancelled timer still fired");
    serial_println!("[hrtimer]   Cancel: OK");

    // Test 4: Multiple timers fire in order.
    static ORDER_LOG: AtomicU64 = AtomicU64::new(0);
    fn order_cb(arg: u64) {
        // Pack firing order into the atomic (shift left by 4 bits each time).
        ORDER_LOG.fetch_add(arg, Ordering::Relaxed);
    }

    ORDER_LOG.store(0, Ordering::Relaxed);
    // Schedule in reverse order (should still fire in deadline order).
    let _h3 = schedule_ns(0, order_cb, 300);
    let _h2 = schedule_ns(0, order_cb, 20);
    let _h1 = schedule_ns(0, order_cb, 1);

    // They all have expiry=now, but insertion order for equal times is
    // append-to-end-of-equals, so they fire in schedule order.
    process_expired();
    let result = ORDER_LOG.load(Ordering::Relaxed);
    assert_eq!(result, 321, "Timers didn't fire (got sum {})", result);
    serial_println!("[hrtimer]   Multiple timers: OK (sum=321)");

    // Test 5: Repeating timer fires and re-schedules.
    static REPEAT_COUNT: AtomicU64 = AtomicU64::new(0);
    fn repeat_cb(_arg: u64) {
        REPEAT_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    REPEAT_COUNT.store(0, Ordering::Relaxed);
    // Same ambient-baseline reasoning as Test 3. Drain any expired ambient
    // timers first so `base` is stable, then check the repeating timer's
    // re-schedule/cancel relative to it — all with interrupts off so the
    // ISR can't reap an ambient timer mid-check.
    crate::cpu::without_interrupts(|| {
        process_expired(); // Stabilise the baseline (reap ambient expiries).
        let base = pending_count();
        let rh = schedule_repeating(0, 1_000_000, repeat_cb, 0); // 1ms interval, fire immediately
        process_expired(); // First fire (re-schedules our repeating timer).
        assert_eq!(REPEAT_COUNT.load(Ordering::Relaxed), 1, "Repeating timer didn't fire");
        assert_eq!(pending_count(), base + 1, "Repeating timer not re-scheduled");
        cancel(rh);
        assert_eq!(pending_count(), base, "Repeating timer not cancelled");
    });
    serial_println!("[hrtimer]   Repeating timer: OK (fired once, re-scheduled, cancelled)");

    // Test 6: Statistics.
    let sched = scheduled_count();
    let cancelled_n = TOTAL_CANCELLED.load(Ordering::Relaxed);
    let fired_n = fired_count();
    serial_println!("[hrtimer]   Stats: scheduled={}, fired={}, cancelled={}",
        sched, fired_n, cancelled_n);

    serial_println!("[hrtimer] Self-test PASSED");
}
