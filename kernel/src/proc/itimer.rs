//! `ITIMER_REAL` — the per-process real-time interval timer.
//!
//! Backs `setitimer(ITIMER_REAL)`, `getitimer(ITIMER_REAL)`, and the legacy
//! `alarm()` syscall (which Linux implements as
//! `setitimer(ITIMER_REAL, value=seconds, interval=0)`). When the timer
//! expires it posts `SIGALRM` to the owning process via
//! [`crate::proc::signal::set_pending`], which both (a) makes the signal
//! deliverable to a registered trampoline at the next syscall-return
//! checkpoint and (b) wakes any thread parked in `pause()` /
//! `rt_sigtimedwait` / a `signalfd` read on the process.
//!
//! ## Why a dedicated module (not alarm-only state)
//!
//! `alarm()` and `setitimer(ITIMER_REAL)` are the *same* timer in POSIX —
//! arming one cancels the other. Modelling them as one shared per-process
//! `RealTimer` (rather than separate alarm/itimer state) is the only correct
//! design: `alarm(0)` must report and clear a timer previously armed by
//! `setitimer`, and vice versa.
//!
//! ## Interrupt-context safety
//!
//! The expiry callback ([`real_fire`]) runs in the `hrtimer` softirq / APIC
//! timer-ISR context. It touches two locks: [`REAL_TIMERS`] (this module,
//! taken under `without_interrupts` via [`with_real`]) and — through
//! `set_pending` — the signal registries, which were made IRQ-safe for
//! exactly this purpose. The lock-ordering is one-directional:
//! `set_real`/`cancel_real` take `REAL_TIMERS` and may then call into the
//! `hrtimer` layer (its own `CPU_TIMERS` lock), but `real_fire` only ever
//! takes `REAL_TIMERS` *after* the `hrtimer` layer has released `CPU_TIMERS`
//! (callbacks fire outside that lock), so there is no `REAL_TIMERS ↔
//! CPU_TIMERS` cycle.
//!
//! ## No-handler behaviour
//!
//! If the process has **no** signal trampoline registered when `SIGALRM`
//! fires, the kernel applies the default action at the next syscall-return
//! checkpoint: `SIGALRM`'s default is to terminate, so the process exits with
//! status `128 + SIGALRM` (see `handlers::deliver_pending_signal` →
//! `terminate_current_process_for_signal`). This matches Linux's behaviour
//! for `alarm()` with no installed handler. The common case — a process that
//! installs a `SIGALRM` handler before calling `alarm()` — delivers to that
//! handler instead.

use crate::error::{KernelError, KernelResult};
use crate::hrtimer::{self, HrTimerHandle};
use crate::proc::pcb::ProcessId;
use crate::proc::signal;
use crate::serial_println;
use alloc::collections::BTreeMap;
use spin::Mutex;

/// `SIGALRM` signal number (Linux ABI). ITIMER_REAL delivers via this.
const SIGALRM: u32 = 14;

/// Nanoseconds per second.
const NS_PER_SEC: u64 = 1_000_000_000;

/// A process's armed `ITIMER_REAL` timer.
#[derive(Clone, Copy)]
struct RealTimer {
    /// Absolute expiry time in the `hrtimer::now_ns()` domain. For a
    /// periodic timer this is updated to the next firing each time it
    /// expires (in [`real_fire`]) so [`get_real`] reports the correct
    /// remaining time.
    expiry_ns: u64,
    /// Re-arm interval in nanoseconds (0 = one-shot).
    interval_ns: u64,
    /// Live `hrtimer` handle, used to cancel the timer when it is replaced
    /// or the process exits. For a one-shot timer this becomes stale after
    /// it fires (the entry is removed by [`real_fire`]); handle IDs are
    /// monotonic so a stale handle only ever makes a later `cancel` a
    /// harmless no-op.
    handle: HrTimerHandle,
}

/// All armed `ITIMER_REAL` timers, keyed by owning process.
///
/// Absent key ⇒ the process has no real timer armed.
static REAL_TIMERS: Mutex<BTreeMap<ProcessId, RealTimer>> =
    Mutex::new(BTreeMap::new());

/// Run `f` with the [`REAL_TIMERS`] lock held and interrupts disabled.
///
/// Interrupts are masked because [`real_fire`] touches this map from the
/// timer ISR; masking on the syscall-context side prevents a same-CPU
/// re-entrancy deadlock (identical rationale to the signal registries).
#[inline]
fn with_real<R>(f: impl FnOnce(&mut BTreeMap<ProcessId, RealTimer>) -> R) -> R {
    crate::cpu::without_interrupts(|| {
        let mut map = REAL_TIMERS.lock();
        f(&mut map)
    })
}

/// `hrtimer` expiry callback: post `SIGALRM` to the owning process.
///
/// `arg` is the owning [`ProcessId`]. Runs in timer-ISR / softirq context.
fn real_fire(arg: u64) {
    let pid: ProcessId = arg;
    // Update bookkeeping under the IRQ-safe lock, and decide whether to
    // actually post the signal. If the entry has been removed (the timer
    // was cancelled after it was already queued to fire), we post nothing —
    // avoiding a spurious SIGALRM after `alarm(0)` / process teardown.
    let should_post = with_real(|map| match map.get_mut(&pid) {
        Some(entry) => {
            if entry.interval_ns > 0 {
                // Periodic: the hrtimer layer has already re-armed itself
                // (same handle id). Track the next expiry for get_real.
                entry.expiry_ns =
                    hrtimer::now_ns().saturating_add(entry.interval_ns);
            } else {
                // One-shot: the hrtimer entry is gone; drop ours too.
                map.remove(&pid);
            }
            true
        }
        None => false,
    });
    if should_post {
        // SIGALRM post is IRQ-safe (signal registries mask interrupts).
        signal::set_pending(pid, SIGALRM);
    }
}

/// Arm (or, if `value_ns == 0`, disarm) the process's `ITIMER_REAL`.
///
/// Returns the previous timer's `(remaining_ns, interval_ns)` so the caller
/// can report the old value (`setitimer`'s `old_value`, `alarm`'s return).
/// A previously-disarmed timer reports `(0, 0)`.
///
/// `value_ns` is the time until first expiry; `interval_ns` is the re-arm
/// period (0 = one-shot). Arming always cancels and replaces any existing
/// timer for `pid` (POSIX: a process has exactly one ITIMER_REAL).
pub fn set_real(
    pid: ProcessId,
    value_ns: u64,
    interval_ns: u64,
) -> (u64, u64) {
    let now = hrtimer::now_ns();
    with_real(|map| {
        // Read + remove the old timer, cancelling its hrtimer.
        let prev = map.remove(&pid);
        let (prev_remaining, prev_interval) = match prev {
            Some(old) => {
                hrtimer::cancel(old.handle);
                let remaining = old.expiry_ns.saturating_sub(now);
                (remaining, old.interval_ns)
            }
            None => (0, 0),
        };

        if value_ns > 0 {
            // Arm a fresh timer. schedule_repeating with interval 0 is a
            // plain one-shot; the hrtimer layer re-arms periodic timers
            // itself, so real_fire never re-schedules.
            let handle = hrtimer::schedule_repeating(
                value_ns, interval_ns, real_fire, pid,
            );
            map.insert(
                pid,
                RealTimer {
                    expiry_ns: now.saturating_add(value_ns),
                    interval_ns,
                    handle,
                },
            );
        }

        (prev_remaining, prev_interval)
    })
}

/// Read the process's `ITIMER_REAL` as `(remaining_ns, interval_ns)`.
///
/// A disarmed (or never-armed) timer reports `(0, 0)`.
#[must_use]
pub fn get_real(pid: ProcessId) -> (u64, u64) {
    let now = hrtimer::now_ns();
    with_real(|map| match map.get(&pid) {
        Some(t) => (t.expiry_ns.saturating_sub(now), t.interval_ns),
        None => (0, 0),
    })
}

/// Cancel and drop the process's `ITIMER_REAL`, if any.
///
/// Wired into process-exit teardown so a pending timer can never fire
/// `SIGALRM` into a dead PID. (Not called on `exec`: POSIX preserves
/// `ITIMER_REAL` across `execve`, and the PID is unchanged.)
pub fn cancel_real(pid: ProcessId) {
    with_real(|map| {
        if let Some(t) = map.remove(&pid) {
            hrtimer::cancel(t.handle);
        }
    });
}

/// Convert seconds + microseconds (a `struct timeval`) to nanoseconds,
/// saturating on overflow.
///
/// Caller must have validated `usec` ∈ [0, 1_000_000) and `sec >= 0`.
#[must_use]
pub fn timeval_to_ns(sec: u64, usec: u64) -> u64 {
    sec.saturating_mul(NS_PER_SEC)
        .saturating_add(usec.saturating_mul(1_000))
}

/// Split a nanosecond count into `(seconds, microseconds)` for writing a
/// `struct timeval` back to userspace (truncating sub-microsecond remainder,
/// matching Linux which stores itimers at microsecond granularity).
#[must_use]
pub fn ns_to_timeval(ns: u64) -> (u64, u64) {
    let sec = ns / NS_PER_SEC;
    let usec = (ns % NS_PER_SEC) / 1_000;
    (sec, usec)
}

/// Round a nanosecond count up to whole seconds (for `alarm()`'s return,
/// which reports the remaining seconds of the previous alarm rounded up).
#[must_use]
pub fn ns_to_secs_ceil(ns: u64) -> u64 {
    ns.saturating_add(NS_PER_SEC.saturating_sub(1)) / NS_PER_SEC
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Synthetic PID base for self-tests (well outside any real PID range).
const TEST_PID_BASE: ProcessId = 0xFFFF_5170_0000;

/// Helper: assert a condition, logging and returning an error on failure.
fn check(cond: bool, what: &str) -> KernelResult<()> {
    if cond {
        Ok(())
    } else {
        serial_println!("[itimer]   FAIL: {}", what);
        Err(KernelError::InternalError)
    }
}

/// `ITIMER_REAL` self-tests — arm/cancel bookkeeping and the fire path.
///
/// The arm path schedules a *real* hrtimer; every test cancels what it arms
/// (or fires it synthetically) so no stray timer survives into normal boot.
/// The fire path is exercised by calling [`real_fire`] directly (a plain
/// function call) rather than waiting for the ISR, which would be
/// non-deterministic on the single-threaded boot CPU.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[itimer] Running ITIMER_REAL self-test...");

    // --- conversion helpers ---
    check(timeval_to_ns(0, 0) == 0, "timeval_to_ns(0,0)")?;
    check(timeval_to_ns(1, 0) == NS_PER_SEC, "timeval_to_ns(1s)")?;
    check(
        timeval_to_ns(2, 500_000) == 2 * NS_PER_SEC + 500_000_000,
        "timeval_to_ns(2.5s)",
    )?;
    check(ns_to_timeval(0) == (0, 0), "ns_to_timeval(0)")?;
    check(
        ns_to_timeval(2 * NS_PER_SEC + 500_000_000) == (2, 500_000),
        "ns_to_timeval(2.5s)",
    )?;
    // Sub-microsecond remainder truncates (Linux stores at usec resolution).
    check(ns_to_timeval(1_999) == (0, 1), "ns_to_timeval truncates ns")?;
    check(ns_to_secs_ceil(0) == 0, "ceil(0)==0")?;
    check(ns_to_secs_ceil(1) == 1, "ceil(1ns)==1")?;
    check(ns_to_secs_ceil(NS_PER_SEC) == 1, "ceil(1s)==1")?;
    check(
        ns_to_secs_ceil(NS_PER_SEC + 1) == 2,
        "ceil(1s+1ns)==2",
    )?;
    serial_println!("[itimer]   timeval conversions: OK");

    // --- arm + get + replace + cancel (one-shot) ---
    let p = TEST_PID_BASE + 1;
    check(get_real(p) == (0, 0), "initially disarmed")?;
    // Arm a long one-shot (1000 s) so it can't fire during the test.
    let (prev_r, prev_i) = set_real(p, 1000 * NS_PER_SEC, 0);
    check((prev_r, prev_i) == (0, 0), "arm reports no previous")?;
    let (rem, iv) = get_real(p);
    // Remaining should be close to 1000 s (a few ms elapsed at most).
    check(iv == 0, "one-shot interval 0")?;
    check(
        rem > 999 * NS_PER_SEC && rem <= 1000 * NS_PER_SEC,
        "remaining ~1000s",
    )?;
    // Re-arm: should report the previous remaining (~1000 s), interval 0.
    let (prev_r2, prev_i2) = set_real(p, 5 * NS_PER_SEC, 2 * NS_PER_SEC);
    check(prev_i2 == 0, "replace reports prev interval 0")?;
    check(
        prev_r2 > 999 * NS_PER_SEC && prev_r2 <= 1000 * NS_PER_SEC,
        "replace reports prev remaining ~1000s",
    )?;
    let (rem3, iv3) = get_real(p);
    check(iv3 == 2 * NS_PER_SEC, "new interval 2s")?;
    check(rem3 > 4 * NS_PER_SEC && rem3 <= 5 * NS_PER_SEC, "new remaining ~5s")?;
    // Cancel and confirm disarmed.
    cancel_real(p);
    check(get_real(p) == (0, 0), "cancelled -> disarmed")?;
    serial_println!("[itimer]   arm/get/replace/cancel: OK");

    // --- fire path (synthetic) posts SIGALRM, one-shot disarms ---
    let pf = TEST_PID_BASE + 2;
    // Arm a long one-shot, then drive its callback by hand (don't wait for
    // the ISR — that would be non-deterministic on the boot CPU).
    set_real(pf, 1000 * NS_PER_SEC, 0);
    // Capture the live hrtimer handle first: real_fire removes the map entry
    // for a one-shot, after which cancel_real can no longer reach the
    // underlying hrtimer, so we must cancel it directly to avoid leaking it
    // into the hrtimer pending list (the hrtimer self-test would later flag
    // the stray timer).
    let pf_handle = with_real(|m| m.get(&pf).map(|t| t.handle));
    check(signal::pending(pf) & (1 << (SIGALRM - 1)) == 0, "no SIGALRM yet")?;
    real_fire(pf);
    check(
        signal::pending(pf) & (1 << (SIGALRM - 1)) != 0,
        "SIGALRM pending after fire",
    )?;
    check(get_real(pf) == (0, 0), "one-shot disarmed after fire")?;
    // A second fire on the now-empty entry must NOT post again (no entry).
    signal::remove(pf); // clear the pending SIGALRM we just posted
    real_fire(pf);
    check(
        signal::pending(pf) & (1 << (SIGALRM - 1)) == 0,
        "no spurious SIGALRM after disarm",
    )?;
    signal::remove(pf);
    // Cancel the orphaned hrtimer from the first set_real.
    if let Some(h) = pf_handle {
        hrtimer::cancel(h);
    }
    serial_println!("[itimer]   fire path (SIGALRM post + disarm): OK");

    // --- periodic fire re-arms bookkeeping ---
    let pp = TEST_PID_BASE + 3;
    set_real(pp, 1000 * NS_PER_SEC, 10 * NS_PER_SEC);
    real_fire(pp);
    let (rem_p, iv_p) = get_real(pp);
    check(iv_p == 10 * NS_PER_SEC, "periodic interval preserved")?;
    // After firing, a periodic timer's remaining resets to ~interval.
    check(rem_p > 9 * NS_PER_SEC && rem_p <= 10 * NS_PER_SEC, "periodic re-armed ~10s")?;
    cancel_real(pp);
    signal::remove(pp);
    check(get_real(pp) == (0, 0), "periodic cancelled")?;
    serial_println!("[itimer]   periodic re-arm bookkeeping: OK");

    serial_println!("[itimer] ITIMER_REAL self-test PASSED (4 groups)");
    Ok(())
}
