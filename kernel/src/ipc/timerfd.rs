//! timerfd — Linux-compatible timer instance objects backing
//! `timerfd_create(2)` / `timerfd_settime(2)` / `timerfd_gettime(2)`.
//!
//! A timerfd is a kernel object that delivers **timer expirations via a file
//! descriptor** instead of a signal.  A Linux process creates one with
//! `timerfd_create(2)`, arms it with `timerfd_settime(2)`, and then `read(2)`s
//! an 8-byte `u64` count of how many times the timer has fired since the last
//! read (resetting that count to zero).  The fd is also pollable: it becomes
//! readable exactly when at least one expiration is pending, so it can sit in a
//! `poll`/`select`/`epoll` set alongside other descriptors in an event loop.
//!
//! ## Lazy expiry model — no background firing
//!
//! Real Linux arms an `hrtimer` that fires an interrupt at each expiration and
//! increments a counter.  We deliberately do **not** schedule a background
//! kernel timer per timerfd.  Instead each instance stores only the **absolute
//! time of its next expiration** (`expiry_ns`, in its own clock's domain) and
//! its **interval** (`interval_ns`, 0 = one-shot).  The expiration count is
//! computed *lazily* from the current clock value whenever the fd is read,
//! polled, or queried:
//!
//! * `read`  — count the expirations that have elapsed up to "now", advance
//!   `expiry_ns` past "now" (or disarm a fired one-shot), and return the count.
//! * `poll`  — readable iff armed and `now >= expiry_ns` (non-consuming).
//! * `gettime` — report time remaining to the next expiration and the interval
//!   (non-consuming).
//!
//! This is correct because a timerfd has no observable side effect *between*
//! reads other than "how many times has it fired" — which is a pure function of
//! the arming parameters and the current time.  It avoids per-timer interrupt
//! overhead in the common poll/epoll idiom.
//!
//! ## Blocking reads
//!
//! A *blocking* (`!TFD_NONBLOCK`) `read()` of an unexpired timerfd does park the
//! caller: [`read_expirations_blocking`] arms a one-shot [`crate::hrtimer`] at
//! the next expiry, [`sched::block_current`]s, and re-evaluates on wake (looping
//! for each tick of a periodic timer).  A read of a *disarmed* timer blocks with
//! no armed `hrtimer` and is woken by [`settime`] when the timer is re-armed.
//! The non-blocking [`read_expirations`] still returns `Some(0)` (→ `EAGAIN`)
//! for the `TFD_NONBLOCK` path; the no-background-firing lazy model is unchanged
//! — the wakeup `hrtimer` exists only for the duration of a blocked read.
//!
//! ## Clock domains
//!
//! `clockid` is preserved per instance.  `CLOCK_MONOTONIC` (1) and
//! `CLOCK_BOOTTIME` (7) read from [`crate::hrtimer::now_ns`] (this kernel has
//! no suspend, so the two coincide); `CLOCK_REALTIME` (0) reads from
//! [`crate::timekeeping::clock_realtime`].  The alarm clocks
//! `CLOCK_REALTIME_ALARM` (8) / `CLOCK_BOOTTIME_ALARM` (9) are rejected with
//! `EPERM` at the syscall layer (no caller holds `CAP_WAKE_ALARM`) and so never
//! reach this module.
//!
//! ## Refcounting and `fork`
//!
//! Like [`crate::ipc::signalfd`] / [`crate::ipc::epoll`] / [`crate::ipc::eventfd`],
//! a timerfd is reference counted: `create()` starts the count at 1, `dup()`
//! bumps it (used when `fork` duplicates the inheriting fd so a parent and child
//! share the same timer object), and `close()` drops one reference — only the
//! final `close()` (count → 0) frees the object.  The armed state is **shared**
//! between all holders, matching Linux: a `timerfd_settime()` through one fd is
//! visible through any `dup`-ed fd referring to the same object.
//!
//! ## Lock ordering
//!
//! `TIMERFD_TABLE` is a leaf lock — none of the operations here call into the
//! scheduler or any other subsystem while holding it (the clock reads happen
//! before the lock is taken), so it never participates in a lock-ordering cycle.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;

/// `CLOCK_MONOTONIC`.
pub const CLOCK_MONOTONIC: i32 = 1;
/// `CLOCK_REALTIME`.
pub const CLOCK_REALTIME: i32 = 0;
/// `CLOCK_BOOTTIME` — coincides with `CLOCK_MONOTONIC` (no suspend).
pub const CLOCK_BOOTTIME: i32 = 7;

/// Current time, in nanoseconds, in the domain of the given clock id.
///
/// Unknown clock ids fall back to the monotonic clock; the syscall layer has
/// already validated `clockid` before any timerfd is created, so this is only
/// ever called with `CLOCK_MONOTONIC` / `CLOCK_REALTIME` / `CLOCK_BOOTTIME`.
#[must_use]
pub fn now_for_clock(clockid: i32) -> u64 {
    match clockid {
        CLOCK_REALTIME => crate::timekeeping::clock_realtime(),
        // MONOTONIC and BOOTTIME coincide (no suspend); any other accepted id
        // also uses the monotonic source.
        CLOCK_MONOTONIC | CLOCK_BOOTTIME => crate::hrtimer::now_ns(),
        _ => crate::hrtimer::now_ns(),
    }
}

// ---------------------------------------------------------------------------
// Pure expiry math (unit-testable, time passed in explicitly)
// ---------------------------------------------------------------------------

/// Compute how many expirations have elapsed by `now` and the new next-expiry.
///
/// Returns `(count, new_expiry)`:
/// * `count` — number of expirations to deliver (0 if disarmed or not yet due).
/// * `new_expiry` — the next not-yet-consumed expiration time after advancing
///   past `now`; `0` means the timer is now disarmed (a fired one-shot).
///
/// `expiry == 0` denotes a disarmed timer.  For a periodic timer the next
/// expiry is advanced so it is strictly greater than `now`.  All arithmetic is
/// saturating, so a pathologically large interval can never overflow (the timer
/// simply pins its next expiry at `u64::MAX` and effectively stops firing).
#[must_use]
// The bare `now - expiry` and `elapsed / interval` are provably safe here:
// the early return guarantees `now >= expiry` (so the subtraction can't
// underflow) and the `interval == 0` branch guarantees the divisor is
// non-zero.  All growth-direction arithmetic uses saturating ops.
#[allow(clippy::arithmetic_side_effects)]
fn advance(expiry: u64, interval: u64, now: u64) -> (u64, u64) {
    if expiry == 0 || now < expiry {
        return (0, expiry);
    }
    if interval == 0 {
        // One-shot fired: deliver one tick and disarm.
        (1, 0)
    } else {
        let elapsed = now - expiry;
        // Full intervals strictly past the first (already-counted) expiry.
        let extra = elapsed / interval;
        let count = extra.saturating_add(1);
        let new_expiry = expiry.saturating_add(count.saturating_mul(interval));
        (count, new_expiry)
    }
}

/// Time remaining until the next expiration, **without** consuming anything.
///
/// Matches `timerfd_gettime`'s `it_value`: the forward-looking time to the next
/// tick.  Returns 0 for a disarmed timer and for an already-fired one-shot
/// (Linux reports `it_value == 0` once a one-shot has expired).
#[must_use]
// Safe arithmetic by construction: `expiry - now` runs only under `now <
// expiry`, and the `% interval` / `interval - into` arithmetic runs only in
// the `interval != 0` branch with `into < interval`.
#[allow(clippy::arithmetic_side_effects)]
fn remaining(expiry: u64, interval: u64, now: u64) -> u64 {
    if expiry == 0 {
        return 0;
    }
    if now < expiry {
        return expiry - now;
    }
    if interval == 0 {
        // One-shot already overdue: it_value reads as 0.
        0
    } else {
        let into = (now - expiry) % interval;
        interval - into
    }
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a timerfd instance (the handle IS the ID).
type TimerFdId = u64;

/// Monotonic ID generator.  Starts at 1 so 0 is never a valid handle.
static NEXT_TIMERFD_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_timerfd_id() -> TimerFdId {
    NEXT_TIMERFD_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to a timerfd instance.
///
/// Wraps the instance ID.  Stored in a Linux `FdEntry` as a raw `u64` (the
/// `HandleKind::Timerfd` variant); the syscall layer reconstructs it with
/// [`TimerFdHandle::from_raw`] on each read / settime / gettime / poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerFdHandle(u64);

impl TimerFdHandle {
    /// Reconstruct a handle from its raw `u64` representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// The raw `u64` representation (what gets stored in an `FdEntry`).
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    fn id(self) -> TimerFdId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Instance
// ---------------------------------------------------------------------------

/// A kernel timerfd instance: clock id, armed state, and a reference count.
struct TimerFd {
    /// The clock this timer measures against (`CLOCK_MONOTONIC` etc.).
    clockid: i32,
    /// Absolute time of the next expiration in `clockid`'s domain; 0 = disarmed.
    expiry_ns: u64,
    /// Interval between expirations in nanoseconds; 0 = one-shot.
    interval_ns: u64,
    /// Reference count: `create` = 1, each `dup` +1, each `close` −1.
    refcount: u32,
    /// Task parked in a *blocking* `read()` waiting for the next expiration,
    /// if any.  Set by [`read_expirations_blocking`] under the table lock and
    /// taken by [`settime`] (so re-arming a disarmed timer wakes a reader that
    /// is blocked with no armed deadline).  A single slot, matching
    /// [`crate::ipc::eventfd`]'s single-reader model: the *armed* case is
    /// robust to slot overwrite because each blocked reader is also woken
    /// directly by its own per-read `hrtimer` (which captures the task id at
    /// arm time, independent of this field); only the rare *disarmed*
    /// blocked-forever case depends on this slot, and concurrent blocking
    /// readers on one shared timerfd are vanishingly rare.
    reader_waiter: Option<TaskId>,
    /// Whether this timer was armed with `TFD_TIMER_CANCEL_ON_SET` (only
    /// honoured for an absolute `CLOCK_REALTIME` timer — see [`settime`]).
    /// While true, a discontinuous step of the realtime clock cancels the
    /// next read with `ECANCELED`.  Cleared whenever the timer is re-armed
    /// without the flag, or disarmed.
    cancel_on_set: bool,
    /// Snapshot of [`crate::timekeeping::realtime_generation`] captured when
    /// the timer was armed with `cancel_on_set`.  A read/poll observing a
    /// newer generation means the realtime clock was stepped since arming,
    /// so the timer is "cancelled".
    armed_gen: u64,
}

impl TimerFd {
    const fn new(clockid: i32) -> Self {
        Self {
            clockid,
            expiry_ns: 0,
            interval_ns: 0,
            refcount: 1,
            reader_waiter: None,
            cancel_on_set: false,
            armed_gen: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live timerfd instances, keyed by ID.
///
/// Leaf lock — no nested locking.
static TIMERFD_TABLE: Mutex<BTreeMap<TimerFdId, TimerFd>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Lifetime API
// ---------------------------------------------------------------------------

/// Create a new (disarmed) timerfd instance for the given clock.
///
/// The returned handle owns one reference; the caller must `close()` it
/// (directly or via process-exit cleanup) exactly once for that reference.
#[must_use]
pub fn create(clockid: i32) -> TimerFdHandle {
    let id = alloc_timerfd_id();
    TIMERFD_TABLE.lock().insert(id, TimerFd::new(clockid));
    TimerFdHandle(id)
}

/// Add one reference to a timerfd instance, returning the same handle.
///
/// Used when `fork` duplicates the inheriting fd: parent and child then each
/// hold a reference to the *same* instance (shared armed state), and neither
/// one's `close()` invalidates the other's.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the instance no longer exists (already
/// fully closed) or the reference count would overflow `u32::MAX`.
pub fn dup(handle: TimerFdHandle) -> KernelResult<TimerFdHandle> {
    let mut table = TIMERFD_TABLE.lock();
    let tfd = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
    tfd.refcount = tfd
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(handle)
}

/// Drop one reference to a timerfd instance.
///
/// Only the final `close()` (refcount → 0) removes the instance.  A
/// double-close is harmless: the saturating decrement floors at 0 and an
/// unknown handle is simply ignored.
pub fn close(handle: TimerFdHandle) {
    let mut table = TIMERFD_TABLE.lock();
    if let Some(tfd) = table.get_mut(&handle.id()) {
        tfd.refcount = tfd.refcount.saturating_sub(1);
        if tfd.refcount == 0 {
            table.remove(&handle.id());
        }
    }
}

/// Does this handle refer to a live timerfd instance?
#[must_use]
pub fn exists(handle: TimerFdHandle) -> bool {
    TIMERFD_TABLE.lock().contains_key(&handle.id())
}

/// The clock id of a timerfd instance, or `None` if the handle is stale.
#[must_use]
pub fn clockid(handle: TimerFdHandle) -> Option<i32> {
    TIMERFD_TABLE.lock().get(&handle.id()).map(|t| t.clockid)
}

// ---------------------------------------------------------------------------
// Arm / disarm / query API
// ---------------------------------------------------------------------------

/// Arm or disarm a timerfd, returning its **previous** `(it_value, it_interval)`.
///
/// This is the kernel half of `timerfd_settime`:
/// * `value_ns == 0` disarms the timer (the new interval is still recorded but
///   the timer will not fire until re-armed with a non-zero value).
/// * otherwise the timer is armed.  When `abstime` is true `value_ns` is the
///   absolute expiry in the clock's domain; when false it is relative to "now".
///
/// The returned `old_value` is the time that *was* remaining until the next
/// expiration (0 if it was disarmed or a fired one-shot), and `old_interval` is
/// the interval that was in effect — exactly what Linux writes back through the
/// `old_value` pointer.
///
/// `cancel_on_set` requests `TFD_TIMER_CANCEL_ON_SET` semantics.  The caller
/// must already have validated the Linux preconditions (only honoured for an
/// **absolute** `CLOCK_REALTIME` timer); when those hold and the timer is being
/// armed, a subsequent discontinuous step of the realtime clock cancels the
/// next read with `ECANCELED`.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if `handle` is not a live instance.
pub fn settime(
    handle: TimerFdHandle,
    abstime: bool,
    value_ns: u64,
    interval_ns: u64,
    cancel_on_set: bool,
) -> KernelResult<(u64, u64)> {
    // Read the clock *before* taking the lock (leaf-lock discipline: no foreign
    // calls while holding TIMERFD_TABLE).
    let cid = clockid(handle).ok_or(KernelError::InvalidHandle)?;
    let now = now_for_clock(cid);
    // Snapshot the realtime-clock-step generation before the lock (same leaf-
    // lock discipline): an armed cancel-on-set timer remembers it to detect a
    // later step.
    let gen_now = crate::timekeeping::realtime_generation();

    let waiter;
    let old;
    {
        let mut table = TIMERFD_TABLE.lock();
        let tfd = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        old = (
            remaining(tfd.expiry_ns, tfd.interval_ns, now),
            tfd.interval_ns,
        );

        if value_ns == 0 {
            // Disarm.  Record the interval (harmless while disarmed) to match
            // Linux, which keeps it_interval in the ctx even when it_value is
            // zeroed.  A disarmed timer cannot be cancelled.
            tfd.expiry_ns = 0;
            tfd.interval_ns = interval_ns;
            tfd.cancel_on_set = false;
        } else {
            tfd.expiry_ns = if abstime {
                value_ns
            } else {
                now.saturating_add(value_ns)
            };
            tfd.interval_ns = interval_ns;
            // Re-arming resets the cancel-on-set state: honour the flag only
            // when set on this call, snapshotting the current generation so a
            // step *after* this arm (not a step that predated it) cancels.
            tfd.cancel_on_set = cancel_on_set;
            tfd.armed_gen = gen_now;
        }

        // Re-arming changes the deadline a blocked reader is waiting on; wake it
        // so it re-evaluates (a reader blocked on a previously-disarmed timer
        // has no `hrtimer` of its own and depends entirely on this wakeup).
        waiter = tfd.reader_waiter.take();
    }

    // Wake outside the table lock (leaf-lock discipline).
    if let Some(tid) = waiter {
        sched::wake(tid);
    }

    Ok(old)
}

/// If this timerfd was armed with `TFD_TIMER_CANCEL_ON_SET` and the realtime
/// clock has been discontinuously stepped since it was armed, consume and
/// report the cancellation, returning `true`.  Reporting resyncs the armed
/// generation to the current one, so a single clock step is reported exactly
/// once (the timer otherwise stays armed at its absolute expiry, now
/// interpreted against the new clock — matching Linux's `ECANCELED`-then-
/// resume behaviour).  Returns `false` for a timer without the flag, a timer
/// armed after the most recent step, or a stale handle.
#[must_use]
pub fn take_cancellation(handle: TimerFdHandle) -> bool {
    // Leaf-lock discipline: read the generation before taking the lock.
    let gen_now = crate::timekeeping::realtime_generation();
    let mut table = TIMERFD_TABLE.lock();
    match table.get_mut(&handle.id()) {
        Some(tfd) if tfd.cancel_on_set && tfd.armed_gen != gen_now => {
            tfd.armed_gen = gen_now;
            true
        }
        _ => false,
    }
}

/// Notify timerfd that the realtime clock was discontinuously stepped
/// (`clock_settime`/`settimeofday`/`ADJ_SETOFFSET`).  Wakes any reader parked
/// in a blocking `read()` on a `TFD_TIMER_CANCEL_ON_SET` timer so it can
/// re-check and return `ECANCELED` promptly instead of sleeping until the
/// timer's absolute expiry.  Poll/epoll readiness is level-triggered
/// ([`is_readable`] consults the generation directly), so pollers need no
/// explicit wake here.
///
/// Called from the `clock_settime` / `clock_adjtime` syscall handlers *after*
/// the step (and thus the generation bump) has been applied.
pub fn clock_was_set() {
    let gen_now = crate::timekeeping::realtime_generation();
    let mut waiters: Vec<TaskId> = Vec::new();
    {
        let mut table = TIMERFD_TABLE.lock();
        for tfd in table.values_mut() {
            if tfd.cancel_on_set && tfd.armed_gen != gen_now {
                if let Some(tid) = tfd.reader_waiter.take() {
                    waiters.push(tid);
                }
            }
        }
    }
    // Wake outside the table lock (leaf-lock discipline).
    for tid in waiters {
        sched::wake(tid);
    }
}

/// Query a timerfd without consuming expirations: `(it_value, it_interval)`.
///
/// `it_value` is the time remaining until the next expiration (0 if disarmed or
/// a fired one-shot); `it_interval` is the configured interval.  This is the
/// kernel half of `timerfd_gettime`.
///
/// Returns `None` if `handle` is not a live instance.
#[must_use]
pub fn gettime(handle: TimerFdHandle) -> Option<(u64, u64)> {
    let cid = clockid(handle)?;
    let now = now_for_clock(cid);
    let table = TIMERFD_TABLE.lock();
    let tfd = table.get(&handle.id())?;
    Some((remaining(tfd.expiry_ns, tfd.interval_ns, now), tfd.interval_ns))
}

/// Consume and return the number of expirations since the last read.
///
/// Advances the timer's next-expiry past "now" (or disarms a fired one-shot).
/// Returns `Some(0)` when the timer is disarmed or has not yet fired — the
/// syscall layer turns a zero count into `EAGAIN` on the `TFD_NONBLOCK` path.
/// Blocking reads instead go through [`read_expirations_blocking`], which parks
/// the caller.  Returns `None` if the handle is stale.
#[must_use]
pub fn read_expirations(handle: TimerFdHandle) -> Option<u64> {
    let cid = clockid(handle)?;
    let now = now_for_clock(cid);
    let mut table = TIMERFD_TABLE.lock();
    let tfd = table.get_mut(&handle.id())?;
    let (count, new_expiry) = advance(tfd.expiry_ns, tfd.interval_ns, now);
    if count > 0 {
        tfd.expiry_ns = new_expiry;
    }
    Some(count)
}

/// `hrtimer` callback used by [`read_expirations_blocking`] to wake a parked
/// reader when its next expiration is due.  Mirrors the eventfd timeout idiom:
/// prefer a direct wake, falling back to a deferred wake if the target is not
/// yet parked (closing the wake-before-block race).
fn timerfd_wake(tid: u64) {
    if !sched::try_wake(tid) {
        sched::defer_wake(tid);
    }
}

/// The owning user process id of the current task, or `0` for a kernel task.
///
/// Timerfd reads are interruptible by signals only for user processes; kernel
/// tasks (`pid == 0`) have no signal state and park uninterruptibly.
fn current_user_pid() -> u64 {
    crate::proc::thread::owner_process(sched::current_task_id()).unwrap_or(0)
}

/// `true` if a deliverable (unblocked) signal is pending for `pid`.
///
/// Always `false` for `pid == 0` (kernel task — no signal context).
fn deliverable_signal_pending(pid: u64) -> bool {
    pid != 0
        && crate::proc::signal::has_pending_in_mask(pid, !crate::proc::signal::blocked(pid))
}

/// Park the current task for a blocking timerfd read, interruptibly for user
/// processes.
///
/// The caller has already armed the expiry [`crate::hrtimer`] (if the timer is
/// armed) — that is one wake source.  This adds the signal-waiter registration
/// (so [`crate::proc::signal::set_pending`] wakes the park when a deliverable
/// signal arrives), using the register-then-recheck idiom to close the
/// post-before-park race, blocks, then deregisters.  Kernel tasks park
/// uninterruptibly.  After this returns the caller's loop must re-evaluate both
/// the timerfd state and [`deliverable_signal_pending`] — a signal wake is
/// reported by the latter, not by this function.
fn park_for_timerfd(pid: u64, task: u64) {
    if pid == 0 {
        sched::block_current();
        return;
    }
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::register_signalfd_waiter(pid, task, deliverable);
    if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
        // A signal arrived between waiter registration and the signal-waiter
        // registration — don't block; the caller's loop observes it and
        // returns Interrupted.
        crate::proc::signal::deregister_signalfd_waiter(pid, task);
        return;
    }
    sched::block_current();
    crate::proc::signal::deregister_signalfd_waiter(pid, task);
}

/// Outcome of a blocking timerfd read ([`read_expirations_blocking`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockingRead {
    /// `n` (> 0) expirations are ready to deliver to the reader.
    Expirations(u64),
    /// The timer was armed with `TFD_TIMER_CANCEL_ON_SET` and the realtime
    /// clock was stepped while the reader waited (or before it entered the
    /// read): the read must return `ECANCELED`.
    Cancelled,
}

/// Blocking variant of [`read_expirations`]: park the caller until at least one
/// expiration is pending, then consume and return the count (always `> 0`).
///
/// This is the kernel half of a **blocking** (`!TFD_NONBLOCK`) `timerfd` read.
/// Unlike the lazy non-consuming queries, a blocking read cannot simply report
/// "nothing yet" — Linux sleeps the reader until the timer fires.  We realise
/// that here by arming a one-shot [`crate::hrtimer`] at the next expiry that
/// wakes us, then [`sched::block_current`]-ing; on a periodic timer the loop
/// re-arms for each subsequent tick.  A read of a *disarmed* timer blocks with
/// no armed `hrtimer` and is woken by [`settime`] when the timer is (re-)armed,
/// exactly as Linux blocks an unarmed-timerfd reader until it is armed and
/// fires.
///
/// A `TFD_TIMER_CANCEL_ON_SET` timer cancelled by a realtime-clock step
/// returns `Ok(BlockingRead::Cancelled)` (the syscall layer maps this to
/// `ECANCELED`); the reader is woken promptly by [`clock_was_set`] rather than
/// sleeping until the absolute expiry.
///
/// # Errors
///
/// [`KernelError::InvalidHandle`] if the handle becomes stale (e.g. the last
/// reference is closed) while blocked.
pub fn read_expirations_blocking(handle: TimerFdHandle) -> KernelResult<BlockingRead> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        // Read the clock before taking the lock (leaf-lock discipline).
        let cid = clockid(handle).ok_or(KernelError::InvalidHandle)?;
        let now = now_for_clock(cid);
        let gen_now = crate::timekeeping::realtime_generation();

        // Relative ns to the next expiry to arm a wakeup for; `None` = disarmed
        // (block until `settime` wakes us).
        let next_remaining;
        {
            let mut table = TIMERFD_TABLE.lock();
            let tfd = table
                .get_mut(&handle.id())
                .ok_or(KernelError::InvalidHandle)?;

            // Cancellation takes priority over an ordinary expiration: a
            // CANCEL_ON_SET timer whose generation is stale must report
            // ECANCELED.  Resync the generation so the cancel is reported once.
            if tfd.cancel_on_set && tfd.armed_gen != gen_now {
                tfd.armed_gen = gen_now;
                tfd.reader_waiter = None;
                return Ok(BlockingRead::Cancelled);
            }

            let (count, new_expiry) = advance(tfd.expiry_ns, tfd.interval_ns, now);
            if count > 0 {
                tfd.expiry_ns = new_expiry;
                // We are returning, not parking — clear any stale registration.
                tfd.reader_waiter = None;
                return Ok(BlockingRead::Expirations(count));
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  A timerfd is a slow object
            // with no inherent deadline at this point (the wait may be
            // indefinite when disarmed), so an interrupted blocking read is
            // restartable (SA_RESTART) — the syscall layer maps Interrupted to
            // ERESTARTSYS.
            if deliverable_signal_pending(pid) {
                if tfd.reader_waiter == Some(task) {
                    tfd.reader_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            // Not yet due — register as the parked reader and capture the
            // deadline (if armed) before dropping the lock.
            tfd.reader_waiter = Some(task);
            next_remaining = if tfd.expiry_ns == 0 {
                None
            } else {
                // `advance` returned 0 with a non-zero expiry ⇒ now < expiry, so
                // `remaining` is strictly positive here.
                Some(remaining(tfd.expiry_ns, tfd.interval_ns, now))
            };
        }

        // Arm a wakeup `hrtimer` for the armed case; the disarmed case relies on
        // `settime` waking `reader_waiter`.
        let timer = next_remaining
            .map(|rem| crate::hrtimer::schedule_ns(rem.max(1), timerfd_wake, task));

        park_for_timerfd(pid, task);

        // Woken (timer fired, settime re-armed, signal, or spurious) — cancel
        // any pending wakeup timer (harmless if it already fired) and
        // re-evaluate.
        if let Some(th) = timer {
            crate::hrtimer::cancel(th);
        }
    }
}

/// Is the timerfd readable right now?
///
/// Readable when at least one expiration is pending, **or** when a
/// `TFD_TIMER_CANCEL_ON_SET` timer has been cancelled by a realtime-clock
/// step (in which case the read returns `ECANCELED`, which Linux signals as
/// `POLLIN` readiness, not `POLLERR`).  Non-consuming — used by the
/// `poll`/`select`/`epoll` readiness engine.  `false` for a stale handle.
#[must_use]
pub fn is_readable(handle: TimerFdHandle) -> bool {
    let cid = match clockid(handle) {
        Some(c) => c,
        None => return false,
    };
    let now = now_for_clock(cid);
    let gen_now = crate::timekeeping::realtime_generation();
    let table = TIMERFD_TABLE.lock();
    match table.get(&handle.id()) {
        Some(tfd) => {
            let expired = tfd.expiry_ns != 0 && now >= tfd.expiry_ns;
            let cancelled = tfd.cancel_on_set && tfd.armed_gen != gen_now;
            expired || cancelled
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Boot-time self-test for the timerfd instance object.
///
/// The kernel is `#![no_std]` / `#![no_main]`, so host `#[test]` functions never
/// run; verification happens here and returns `Err` (after a `[timerfd] FAIL: …`
/// line) instead of panicking.  Covers the pure expiry math (one-shot and
/// periodic, including overdue multi-tick), refcount lifetime with shared armed
/// state, settime/gettime old-value reporting, and stale-handle safety.
#[allow(clippy::too_many_lines)]
pub fn self_test() -> KernelResult<()> {
    serial_println!("[timerfd] Running timerfd instance self-test...");

    // 1. Pure expiry math — `advance`.
    // Disarmed: no ticks.
    if advance(0, 0, 1000) != (0, 0) {
        serial_println!("[timerfd]   FAIL: advance(disarmed) ticked");
        return Err(KernelError::InternalError);
    }
    // Not yet due.
    if advance(100, 0, 50) != (0, 100) {
        serial_println!("[timerfd]   FAIL: advance(not-due) ticked");
        return Err(KernelError::InternalError);
    }
    // One-shot exactly due -> 1 tick, disarm.
    if advance(100, 0, 100) != (1, 0) {
        serial_println!("[timerfd]   FAIL: advance(one-shot due) wrong");
        return Err(KernelError::InternalError);
    }
    // Periodic: expiry 100, interval 10, now 125 -> ticks at 100/110/120 = 3,
    // next expiry 130 (> now).
    if advance(100, 10, 125) != (3, 130) {
        serial_println!("[timerfd]   FAIL: advance(periodic overdue) wrong");
        return Err(KernelError::InternalError);
    }
    // Periodic exactly on a boundary: expiry 100, interval 10, now 100 -> 1 tick,
    // next 110.
    if advance(100, 10, 100) != (1, 110) {
        serial_println!("[timerfd]   FAIL: advance(periodic boundary) wrong");
        return Err(KernelError::InternalError);
    }

    // 2. Pure remaining-time math — `remaining`.
    if remaining(0, 0, 5) != 0 {
        serial_println!("[timerfd]   FAIL: remaining(disarmed) != 0");
        return Err(KernelError::InternalError);
    }
    if remaining(100, 0, 60) != 40 {
        serial_println!("[timerfd]   FAIL: remaining(pending one-shot) != 40");
        return Err(KernelError::InternalError);
    }
    if remaining(100, 0, 150) != 0 {
        serial_println!("[timerfd]   FAIL: remaining(overdue one-shot) != 0");
        return Err(KernelError::InternalError);
    }
    // Periodic overdue: expiry 100, interval 10, now 125 -> 5ns into the
    // 120..130 window, 5 remaining.
    if remaining(100, 10, 125) != 5 {
        serial_println!("[timerfd]   FAIL: remaining(periodic overdue) != 5");
        return Err(KernelError::InternalError);
    }

    // 3. Instance lifetime + arm/disarm/query via the real API.
    let t = create(CLOCK_MONOTONIC);
    if !exists(t) {
        serial_println!("[timerfd]   FAIL: fresh instance does not exist");
        return Err(KernelError::InternalError);
    }
    if clockid(t) != Some(CLOCK_MONOTONIC) {
        serial_println!("[timerfd]   FAIL: clockid mismatch");
        close(t);
        return Err(KernelError::InternalError);
    }
    // Fresh timer is disarmed: not readable, gettime all-zero.
    if is_readable(t) {
        serial_println!("[timerfd]   FAIL: disarmed timer readable");
        close(t);
        return Err(KernelError::InternalError);
    }
    match gettime(t) {
        Some((0, 0)) => {}
        other => {
            serial_println!("[timerfd]   FAIL: gettime(disarmed) = {:?}", other);
            close(t);
            return Err(KernelError::InternalError);
        }
    }

    // Arm a one-shot far in the future (relative); old value should be (0, 0).
    let far = 1_000_000_000_000u64; // ~1000s out — won't fire during the test.
    match settime(t, false, far, 0, false) {
        Ok((0, 0)) => {}
        other => {
            serial_println!("[timerfd]   FAIL: settime(arm) old != (0,0): {:?}", other);
            close(t);
            return Err(KernelError::InternalError);
        }
    }
    // Now gettime should report a positive (but <= far) remaining, interval 0.
    match gettime(t) {
        Some((rem, 0)) if rem > 0 && rem <= far => {}
        other => {
            serial_println!("[timerfd]   FAIL: gettime(armed) = {:?}", other);
            close(t);
            return Err(KernelError::InternalError);
        }
    }
    if is_readable(t) {
        serial_println!("[timerfd]   FAIL: far-future timer already readable");
        close(t);
        return Err(KernelError::InternalError);
    }
    // read on an unexpired timer yields 0 (syscall layer maps to EAGAIN).
    if read_expirations(t) != Some(0) {
        serial_println!("[timerfd]   FAIL: read(unexpired) != 0");
        close(t);
        return Err(KernelError::InternalError);
    }

    // Re-arm with an absolute expiry in the past -> immediately readable.
    match settime(t, true, 1, 0, false) {
        Ok((old, 0)) if old <= far => {}
        other => {
            serial_println!("[timerfd]   FAIL: settime(abs-past) old wrong: {:?}", other);
            close(t);
            return Err(KernelError::InternalError);
        }
    }
    if !is_readable(t) {
        serial_println!("[timerfd]   FAIL: past one-shot not readable");
        close(t);
        return Err(KernelError::InternalError);
    }
    // Consuming read delivers exactly 1 and disarms the one-shot.
    if read_expirations(t) != Some(1) {
        serial_println!("[timerfd]   FAIL: read(past one-shot) != 1");
        close(t);
        return Err(KernelError::InternalError);
    }
    if read_expirations(t) != Some(0) || is_readable(t) {
        serial_println!("[timerfd]   FAIL: one-shot not disarmed after read");
        close(t);
        return Err(KernelError::InternalError);
    }

    // 3b. Blocking read FAST PATH: with an expiration already pending,
    // `read_expirations_blocking` must consume and return it *without* parking
    // (it would otherwise hang the single-threaded boot CPU).  The actual
    // parking path is exercised only by real userspace (a blocked reader needs
    // another runnable task to wake it) and is verified by construction against
    // the battle-tested sched/hrtimer primitives it reuses.
    settime(t, true, 1, 0, false)?; // abs expiry in the past → immediately pending.
    if !is_readable(t) {
        serial_println!("[timerfd]   FAIL: re-armed past one-shot not readable");
        close(t);
        return Err(KernelError::InternalError);
    }
    match read_expirations_blocking(t) {
        Ok(BlockingRead::Expirations(1)) => {}
        other => {
            serial_println!("[timerfd]   FAIL: blocking read fast path = {:?}", other);
            close(t);
            return Err(KernelError::InternalError);
        }
    }
    // The one-shot is now disarmed again, and no reader is left registered.
    if is_readable(t) {
        serial_println!("[timerfd]   FAIL: one-shot still readable after blocking read");
        close(t);
        return Err(KernelError::InternalError);
    }

    // 4. Refcount lifetime + shared armed state across dup.
    let t2 = dup(t)?;
    if t2 != t {
        serial_println!("[timerfd]   FAIL: dup returned a different handle");
        close(t);
        close(t);
        return Err(KernelError::InternalError);
    }
    // Arm through t2 (abs past) — visible through t.
    settime(t2, true, 1, 0, false)?;
    if !is_readable(t) {
        serial_println!("[timerfd]   FAIL: armed state not shared across dup");
        close(t);
        close(t);
        return Err(KernelError::InternalError);
    }
    close(t); // 2 -> 1: survives.
    if !exists(t) {
        serial_println!("[timerfd]   FAIL: freed after first of two closes");
        close(t);
        return Err(KernelError::InternalError);
    }
    close(t); // 1 -> 0: freed.
    if exists(t) {
        serial_println!("[timerfd]   FAIL: instance still exists after final close");
        return Err(KernelError::InternalError);
    }

    // 5. Stale-handle safety: every op fails cleanly, no panic.
    if gettime(t).is_some() {
        serial_println!("[timerfd]   FAIL: gettime on stale handle not None");
        return Err(KernelError::InternalError);
    }
    if read_expirations(t).is_some() {
        serial_println!("[timerfd]   FAIL: read on stale handle not None");
        return Err(KernelError::InternalError);
    }
    // Blocking read on a stale handle must fail fast (InvalidHandle), not park.
    if read_expirations_blocking(t).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[timerfd]   FAIL: blocking read on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    if is_readable(t) {
        serial_println!("[timerfd]   FAIL: is_readable on stale handle not false");
        return Err(KernelError::InternalError);
    }
    if clockid(t).is_some() {
        serial_println!("[timerfd]   FAIL: clockid on stale handle not None");
        return Err(KernelError::InternalError);
    }
    if settime(t, false, 1, 0, false).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[timerfd]   FAIL: settime on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    if dup(t).err() != Some(KernelError::InvalidHandle) {
        serial_println!("[timerfd]   FAIL: dup on stale handle not InvalidHandle");
        return Err(KernelError::InternalError);
    }
    // close() on a stale handle must be a harmless no-op.
    close(t);

    // 6. TFD_TIMER_CANCEL_ON_SET (TD15): an absolute CLOCK_REALTIME timer armed
    // with cancel_on_set is "cancelled" when the realtime clock is stepped
    // discontinuously.  We simulate a step with `adjust_realtime(0)`, which bumps
    // the realtime-clock-step generation without actually moving the clock value.
    let tc = create(CLOCK_REALTIME);
    // Arm far in the future (absolute) so it does NOT expire on its own; the only
    // readiness we expect must come from the clock-step cancellation, not expiry.
    let abs_far = crate::timekeeping::clock_realtime().saturating_add(1_000_000_000_000);
    settime(tc, true, abs_far, 0, true)?;
    if is_readable(tc) || take_cancellation(tc) {
        serial_println!("[timerfd]   FAIL: cancel-on-set timer ready before clock step");
        close(tc);
        return Err(KernelError::InternalError);
    }
    // Step the realtime clock (discontinuity).
    crate::timekeeping::adjust_realtime(0);
    if !is_readable(tc) {
        serial_println!("[timerfd]   FAIL: cancel-on-set timer not readable after clock step");
        close(tc);
        return Err(KernelError::InternalError);
    }
    if !take_cancellation(tc) {
        serial_println!("[timerfd]   FAIL: take_cancellation false after clock step");
        close(tc);
        return Err(KernelError::InternalError);
    }
    // Cancellation is one-shot per step: after consuming it the timer resyncs to
    // the current generation, so it is neither cancelled nor (yet) expired.
    if take_cancellation(tc) || is_readable(tc) {
        serial_println!("[timerfd]   FAIL: cancellation re-reported after consume");
        close(tc);
        return Err(KernelError::InternalError);
    }
    // A re-armed timer WITHOUT cancel_on_set must ignore a subsequent clock step.
    settime(tc, true, abs_far, 0, false)?;
    crate::timekeeping::adjust_realtime(0);
    if take_cancellation(tc) || is_readable(tc) {
        serial_println!("[timerfd]   FAIL: non-cancel-on-set timer affected by clock step");
        close(tc);
        return Err(KernelError::InternalError);
    }
    close(tc);

    serial_println!("[timerfd]   timerfd instance object (create/arm/read/dup/close): OK");
    serial_println!("[timerfd]   TFD_TIMER_CANCEL_ON_SET (TD15): OK");
    Ok(())
}
