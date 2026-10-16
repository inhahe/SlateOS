//! IPC Semaphore — counting semaphore for userspace synchronization.
//!
//! An IPC semaphore is a kernel-managed counting resource that tasks
//! can signal (increment) and wait on (decrement).  Unlike the
//! kernel-internal [`Semaphore`](crate::sched::semaphore::Semaphore), this
//! version:
//!
//! - Has a handle-based API suitable for userspace via syscalls.
//! - Integrates with completion ports (waitable when count > 0).
//! - Supports multiple waiters with FIFO wake ordering.
//! - Has a configurable maximum count (capacity).
//!
//! ## Semantics
//!
//! - **`signal(handle, count)`**: increment the semaphore by `count`.
//!   Wakes up to `count` blocked waiters (one per unit).  Returns
//!   `Err(Overflow)` if the increment would exceed `max_count`.
//!
//! - **`wait(handle)`**: decrement by 1.  If the count is 0, blocks
//!   until a signal occurs.
//!
//! - **`try_wait(handle)`**: non-blocking decrement.  Returns
//!   `Err(WouldBlock)` if count is 0.
//!
//! - **`close(handle)`**: destroy the semaphore.  All blocked waiters
//!   are woken with `Err(ChannelClosed)`.
//!
//! ## Completion Port Integration
//!
//! Register a semaphore with a completion port via:
//! ```ignore
//! completion::register(cp, WaitSource::Semaphore(sem_handle.raw()), user_data)
//! ```
//!
//! The completion port will fire whenever the semaphore count is > 0,
//! enabling event-driven multiplexing of semaphore readiness with
//! other I/O sources.
//!
//! ## Lock Ordering
//!
//! `SEM_TABLE` → `SCHED` (signal may call `sched::wake()`).

use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::collections::{BTreeMap, VecDeque};
use core::sync::atomic::{AtomicU64, Ordering};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default maximum count if not specified.
const DEFAULT_MAX_COUNT: u64 = u64::MAX >> 1;

/// Maximum number of waiters queued on a single semaphore.
const MAX_WAITERS: usize = 256;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a semaphore.
type SemId = u64;

/// Counter for generating unique IDs.
static NEXT_SEM_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_sem_id() -> SemId {
    NEXT_SEM_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to an IPC semaphore.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemHandle(u64);

impl SemHandle {
    /// Reconstruct a handle from its raw u64 representation.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw u64 representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// The semaphore ID (the handle IS the ID).
    fn id(self) -> SemId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Semaphore internals
// ---------------------------------------------------------------------------

/// A kernel IPC semaphore.
struct Semaphore {
    /// Current count (resources available).
    count: u64,
    /// Maximum count (capacity).
    max_count: u64,
    /// Tasks blocked waiting for count > 0, in FIFO order.
    waiters: VecDeque<TaskId>,
    /// Whether the semaphore has been closed.
    closed: bool,
}

impl Semaphore {
    fn new(initial: u64, max_count: u64) -> Self {
        Self {
            count: initial.min(max_count),
            max_count,
            waiters: VecDeque::new(),
            closed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live semaphores.
///
/// Lock ordering: `SEM_TABLE` → `SCHED`.
static SEM_TABLE: Mutex<BTreeMap<SemId, Semaphore>> = Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new semaphore.
///
/// `initial`: starting count (0 = nothing available yet).
/// `max_count`: maximum count (0 uses default = `u64::MAX >> 1`).
///
/// Returns a handle to the new semaphore.
pub fn create(initial: u64, max_count: u64) -> SemHandle {
    let max = if max_count == 0 {
        DEFAULT_MAX_COUNT
    } else {
        max_count
    };
    let id = alloc_sem_id();
    let sem = Semaphore::new(initial, max);

    let mut table = SEM_TABLE.lock();
    table.insert(id, sem);

    crate::ktrace::record(
        crate::ktrace::Category::Ipc,
        crate::ktrace::event::SEM_CREATE,
        id,
        initial,
    );

    SemHandle(id)
}

/// Signal (release) — increment the semaphore count.
///
/// Increments by `count` units.  If there are blocked waiters, wakes
/// up to `count` of them (one unit consumed per wake).
///
/// # Returns
///
/// - `Ok(())` — signal succeeded.
/// - `Err(Overflow)` — increment would exceed `max_count`.
/// - `Err(InvalidHandle)` — semaphore not found.
/// - `Err(ChannelClosed)` — semaphore was closed.
pub fn signal(handle: SemHandle, count: u64) -> KernelResult<()> {
    if count == 0 {
        return Ok(());
    }

    {
        let mut table = SEM_TABLE.lock();
        let sem = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if sem.closed {
            return Err(KernelError::ChannelClosed);
        }

        // Check overflow.
        let remaining_capacity = sem.max_count.saturating_sub(sem.count);
        if count > remaining_capacity {
            return Err(KernelError::Overflow);
        }

        // A unit goes to each waiter, in order, while both last: taking a
        // waiter off the queue is its grant. Only units no waiter takes are
        // counted, so the count is never above zero while a task waits.
        // Woken under the lock (`SEM_TABLE` -> `SCHED`, the module's order),
        // so that no number of waiters needs a buffer to be woken from.
        let mut units_left = count;
        while units_left > 0 {
            let Some(task_id) = sem.waiters.pop_front() else {
                break;
            };
            sched::wake(task_id);
            units_left = units_left.saturating_sub(1);
        }
        sem.count = sem.count.saturating_add(units_left);
    }

    crate::ktrace::record(
        crate::ktrace::Category::Ipc,
        crate::ktrace::event::SEM_SIGNAL,
        handle.raw(),
        count,
    );

    Ok(())
}

/// Wait (acquire) — decrement the semaphore by 1.
///
/// If count > 0, decrements and returns immediately.  If count == 0,
/// blocks until a signal occurs.
///
/// # Returns
///
/// - `Ok(())` — unit acquired.
/// - `Err(ChannelClosed)` — semaphore was closed while waiting.
/// - `Err(InvalidHandle)` — semaphore not found.
pub fn wait(handle: SemHandle) -> KernelResult<()> {
    crate::ktrace::record(
        crate::ktrace::Category::Ipc,
        crate::ktrace::event::SEM_WAIT,
        handle.raw(),
        0,
    );

    {
        let mut table = SEM_TABLE.lock();
        let sem = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if sem.count > 0 {
            sem.count = sem.count.saturating_sub(1);
            return Ok(());
        }

        if sem.closed {
            return Err(KernelError::ChannelClosed);
        }

        // Count is 0 — block.
        if sem.waiters.len() >= MAX_WAITERS {
            return Err(KernelError::WouldBlock);
        }
        sem.waiters.push_back(sched::current_task_id());
    }

    let me = sched::current_task_id();
    loop {
        sched::block_current_on(crate::wchan::Wait::new(
            crate::wchan::WaitChannel::Semaphore,
            handle.raw(),
        ));

        // signal() grants a unit by taking a waiter off the queue, then wakes
        // it: off the queue is the grant. close() removes the semaphore. A
        // park can also end for neither -- a wake recorded for an earlier wait
        // ends the next park (`sched::Task::mark_ready`) -- and then this task
        // is still queued, and waits again unless a unit is free.
        let mut table = SEM_TABLE.lock();
        let Some(sem) = table.get_mut(&handle.id()) else {
            return Err(KernelError::ChannelClosed);
        };
        if sem.closed {
            return Err(KernelError::ChannelClosed);
        }
        if holds_unit_after_park(sem, me) {
            return Ok(());
        }
    }
}

/// After a park, for waiter `me` of `sem`: whether it holds a unit now. Off
/// the queue, `signal` granted it one. Still queued, it takes a free unit if
/// there is one, leaving the queue as it does -- `signal` counts units only
/// when no waiter is left to take them, so this is a guard, not the usual
/// path -- and otherwise holds none.
fn holds_unit_after_park(sem: &mut Semaphore, me: TaskId) -> bool {
    let Some(pos) = sem.waiters.iter().position(|&id| id == me) else {
        return true;
    };
    if sem.count == 0 {
        return false;
    }
    sem.waiters.remove(pos);
    sem.count = sem.count.saturating_sub(1);
    true
}

/// Non-blocking wait (try-acquire) — decrement if count > 0.
///
/// # Returns
///
/// - `Ok(())` — unit acquired.
/// - `Err(WouldBlock)` — count is 0.
/// - `Err(ChannelClosed)` — semaphore closed.
/// - `Err(InvalidHandle)` — not found.
pub fn try_wait(handle: SemHandle) -> KernelResult<()> {
    let mut table = SEM_TABLE.lock();
    let sem = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;

    if sem.count > 0 {
        sem.count = sem.count.saturating_sub(1);
        Ok(())
    } else if sem.closed {
        Err(KernelError::ChannelClosed)
    } else {
        Err(KernelError::WouldBlock)
    }
}

/// Wait (acquire) with a timeout — decrement if count > 0, or block
/// up to `timeout_ns` nanoseconds.
///
/// Returns `Err(TimedOut)` if the timeout expires before a signal
/// occurs.  `timeout_ns = 0` is equivalent to `try_wait()` (returns
/// `Err(TimedOut)` instead of `Err(WouldBlock)` when count is 0).
///
/// # Returns
///
/// - `Ok(())` — unit acquired.
/// - `Err(TimedOut)` — no signal within the deadline.
/// - `Err(ChannelClosed)` — semaphore closed while waiting.
/// - `Err(InvalidHandle)` — not found.
pub fn wait_timeout(handle: SemHandle, timeout_ns: u64) -> KernelResult<()> {
    // Fast path: try without blocking.
    {
        let mut table = SEM_TABLE.lock();
        let sem = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if sem.count > 0 {
            sem.count = sem.count.saturating_sub(1);
            return Ok(());
        }

        if sem.closed {
            return Err(KernelError::ChannelClosed);
        }
    }

    // Non-blocking mode.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule a timer to wake us at the deadline.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);
    let task_id = sched::current_task_id();

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let timer_handle = crate::hrtimer::schedule_ns(timeout_ns, timeout_wake, task_id);

    // Queue, then park until granted a unit, closed, or timed out. Each pass
    // decides under the lock. The timeout has come when the timer has fired
    // (`hrtimer::has_expired`), whatever this CPU's clock reads, or when the
    // clock is past the deadline.
    let mut queued = false;
    loop {
        {
            let mut table = SEM_TABLE.lock();
            let Some(sem) = table.get_mut(&handle.id()) else {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            };
            if sem.closed {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }
            let holds_unit = if queued {
                holds_unit_after_park(sem, task_id)
            } else if sem.count > 0 {
                sem.count = sem.count.saturating_sub(1);
                true
            } else {
                false
            };
            if holds_unit {
                crate::hrtimer::cancel(timer_handle);
                return Ok(());
            }
            if crate::hrtimer::has_expired(timer_handle) || crate::hrtimer::now_ns() >= deadline_ns
            {
                if let Some(pos) = sem.waiters.iter().position(|&id| id == task_id) {
                    sem.waiters.remove(pos);
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }
            if !queued {
                if sem.waiters.len() >= MAX_WAITERS {
                    crate::hrtimer::cancel(timer_handle);
                    return Err(KernelError::WouldBlock);
                }
                sem.waiters.push_back(task_id);
                queued = true;
            }
        }

        sched::block_current_on(crate::wchan::Wait::new(
            crate::wchan::WaitChannel::Semaphore,
            handle.raw(),
        ));
    }
}

/// Get the current count (non-consuming peek).
///
/// Returns the current count, or `Err(InvalidHandle)`.
#[allow(dead_code)] // Used in self-test; also useful API for userspace query.
pub fn count(handle: SemHandle) -> KernelResult<u64> {
    let table = SEM_TABLE.lock();
    let sem = table.get(&handle.id()).ok_or(KernelError::InvalidHandle)?;
    Ok(sem.count)
}

/// Close a semaphore.
///
/// All blocked waiters are woken (they will receive `ChannelClosed`).
/// The semaphore is removed from the table.
pub fn close(handle: SemHandle) {
    crate::ktrace::record(
        crate::ktrace::Category::Ipc,
        crate::ktrace::event::SEM_CLOSE,
        handle.raw(),
        0,
    );

    let mut wake_list = VecDeque::new();

    {
        let mut table = SEM_TABLE.lock();
        if let Some(mut sem) = table.remove(&handle.id()) {
            sem.closed = true;
            wake_list = core::mem::take(&mut sem.waiters);
        }
    }

    for task_id in &wake_list {
        sched::wake(*task_id);
    }
}

// ---------------------------------------------------------------------------
// Polling helper (for completion port)
// ---------------------------------------------------------------------------

/// Check if the semaphore has available units (non-consuming).
///
/// Returns `true` if `wait()` would not block (count > 0 or closed).
pub fn has_value(handle: SemHandle) -> bool {
    let table = SEM_TABLE.lock();
    let Some(sem) = table.get(&handle.id()) else {
        return false;
    };
    sem.count > 0 || sem.closed
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Shared handle for test tasks (passed as arg).
use core::sync::atomic::{AtomicBool, Ordering as AtOrd};

/// Flag for signal-wake test.
static SEM_TEST_ACQUIRED: AtomicBool = AtomicBool::new(false);

/// What [`sem_close_waiter_task`]'s wait returned: 0 not yet, 1
/// `ChannelClosed` (the close woke it), 2 anything else.
static SEM_CLOSE_OUTCOME: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(0);

/// Task entry: blocks on sem wait, expects ChannelClosed.
extern "C" fn sem_close_waiter_task(h_raw: u64) {
    let handle = SemHandle::from_raw(h_raw);
    let outcome = match wait(handle) {
        // Expected — woken by close.
        Err(KernelError::ChannelClosed) => 1,
        other => {
            serial_println!(
                "[sem]   FAIL: waiter got {:?} instead of ChannelClosed",
                other
            );
            2
        }
    };
    SEM_CLOSE_OUTCOME.store(outcome, AtOrd::Release);
}

/// For the self-test: wait, by the clock, until task `tid` is blocked
/// (`want_blocked`) or has ended. A fixed number of yields waits for another
/// task only on one CPU, where a yield runs it (`selftest::wait_until`).
fn sem_test_wait_for(tid: TaskId, want_blocked: bool) -> bool {
    use crate::sched::task::TaskState;
    crate::selftest::wait_until(5_000, || match crate::sched::task_state(tid) {
        Some(TaskState::Blocked) => want_blocked,
        None | Some(TaskState::Dead) => !want_blocked,
        Some(_) => false,
    })
}

/// Task entry: blocks on sem wait, sets ACQUIRED flag.
extern "C" fn sem_signal_waiter_task(h_raw: u64) {
    let handle = SemHandle::from_raw(h_raw);
    if wait(handle).is_ok() {
        SEM_TEST_ACQUIRED.store(true, AtOrd::Release);
    }
}

/// Run semaphore self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[sem] Running IPC semaphore self-test...");

    // Test 1: Create with initial count, wait immediately.
    {
        let h = create(3, 10);
        if count(h)? != 3 {
            serial_println!("[sem]   FAIL: initial count not 3");
            return Err(KernelError::InternalError);
        }
        try_wait(h)?; // count → 2
        try_wait(h)?; // count → 1
        try_wait(h)?; // count → 0

        if count(h)? != 0 {
            serial_println!("[sem]   FAIL: count not 0 after 3 waits");
            return Err(KernelError::InternalError);
        }

        // Should fail — count is 0.
        match try_wait(h) {
            Err(KernelError::WouldBlock) => {}
            other => {
                serial_println!("[sem]   FAIL: try_wait on empty: {:?}", other);
                return Err(KernelError::InternalError);
            }
        }
        close(h);
        serial_println!("[sem]   Create + try_wait: OK");
    }

    // Test 2: Signal increments count.
    {
        let h = create(0, 100);
        signal(h, 5)?;
        if count(h)? != 5 {
            serial_println!("[sem]   FAIL: count not 5 after signal(5)");
            return Err(KernelError::InternalError);
        }
        signal(h, 3)?;
        if count(h)? != 8 {
            serial_println!("[sem]   FAIL: count not 8 after signal(3)");
            return Err(KernelError::InternalError);
        }
        close(h);
        serial_println!("[sem]   Signal: OK");
    }

    // Test 3: Overflow detection.
    {
        let h = create(8, 10);
        signal(h, 2)?;
        if count(h)? != 10 {
            serial_println!("[sem]   FAIL: count not 10");
            return Err(KernelError::InternalError);
        }

        // Signal 1 more → overflow.
        match signal(h, 1) {
            Err(KernelError::Overflow) => {}
            other => {
                serial_println!("[sem]   FAIL: overflow not detected: {:?}", other);
                return Err(KernelError::InternalError);
            }
        }
        close(h);
        serial_println!("[sem]   Overflow detection: OK");
    }

    // Test 4: Close wakes blocked waiter.
    {
        use crate::sched;

        SEM_CLOSE_OUTCOME.store(0, AtOrd::Release);
        let h = create(0, 10);

        // Spawn a task that will block on wait.
        let Ok(tid) = sched::spawn(b"sem-close-test", 16, sem_close_waiter_task, h.raw(), 0) else {
            close(h);
            serial_println!("[sem]   FAIL: could not spawn the close-test waiter");
            return Err(KernelError::InternalError);
        };
        if !sem_test_wait_for(tid, true) {
            close(h);
            serial_println!("[sem]   FAIL: the waiter never blocked on an empty semaphore");
            return Err(KernelError::InternalError);
        }

        // Close should wake the waiter, with ChannelClosed.
        close(h);
        let woken =
            crate::selftest::wait_until(5_000, || SEM_CLOSE_OUTCOME.load(AtOrd::Acquire) != 0);
        // Gone before the next test, which may be handed the same handle
        // number: a waiter still running could take that test's signal.
        let ended = sem_test_wait_for(tid, false);
        if !woken || SEM_CLOSE_OUTCOME.load(AtOrd::Acquire) != 1 || !ended {
            serial_println!(
                "[sem]   FAIL: close did not wake the waiter with ChannelClosed (outcome {}, ended {})",
                SEM_CLOSE_OUTCOME.load(AtOrd::Acquire),
                ended
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[sem]   Close wakes waiter: OK");
    }

    // Test 5: Signal wakes blocked waiter.
    {
        use crate::sched;

        SEM_TEST_ACQUIRED.store(false, AtOrd::Release);

        let h = create(0, 10);

        // Spawn a task that blocks on wait.
        let Ok(tid) = sched::spawn(b"sem-signal-test", 16, sem_signal_waiter_task, h.raw(), 0)
        else {
            close(h);
            serial_println!("[sem]   FAIL: could not spawn the signal-test waiter");
            return Err(KernelError::InternalError);
        };
        if !sem_test_wait_for(tid, true) {
            close(h);
            serial_println!("[sem]   FAIL: the signal-test waiter never blocked");
            return Err(KernelError::InternalError);
        }

        // Signal should wake the waiter.
        signal(h, 1)?;
        let acquired =
            crate::selftest::wait_until(5_000, || SEM_TEST_ACQUIRED.load(AtOrd::Acquire));
        close(h);
        let ended = sem_test_wait_for(tid, false);
        if !acquired || !ended {
            serial_println!(
                "[sem]   FAIL: waiter not woken by signal (acquired {}, ended {})",
                acquired,
                ended
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[sem]   Signal wakes waiter: OK");
    }

    // Test 6: one signal grants a unit to every waiter it covers -- more of
    // them than a fixed wake buffer would hold -- and counts none while any
    // waits.
    {
        SEM_MANY_ACQUIRED.store(0, AtOrd::Release);
        let h = create(0, 100);
        let mut tids: [TaskId; SEM_MANY_WAITERS] = [0; SEM_MANY_WAITERS];
        for slot in &mut tids {
            let Ok(tid) = sched::spawn(b"sem-many-test", 16, sem_many_waiter_task, h.raw(), 0)
            else {
                close(h);
                serial_println!("[sem]   FAIL: could not spawn a many-waiters waiter");
                return Err(KernelError::InternalError);
            };
            *slot = tid;
        }
        if !tids.iter().all(|&tid| sem_test_wait_for(tid, true)) {
            close(h);
            serial_println!("[sem]   FAIL: a many-waiters waiter never blocked");
            return Err(KernelError::InternalError);
        }
        signal(h, SEM_MANY_WAITERS as u64)?;
        let all = crate::selftest::wait_until(5_000, || {
            SEM_MANY_ACQUIRED.load(AtOrd::Acquire) == SEM_MANY_WAITERS
        });
        let left = count(h)?;
        close(h);
        let ended = tids.iter().all(|&tid| sem_test_wait_for(tid, false));
        if !all || left != 0 || !ended {
            serial_println!(
                "[sem]   FAIL: signal({}) to {} waiters: {} acquired, count {}, ended {}",
                SEM_MANY_WAITERS,
                SEM_MANY_WAITERS,
                SEM_MANY_ACQUIRED.load(AtOrd::Acquire),
                left,
                ended
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[sem]   One signal wakes {} waiters: OK", SEM_MANY_WAITERS);
    }

    // Test 7: after a park, a waiter off the queue holds the unit `signal`
    // granted; one still queued takes a free unit and leaves the queue, or
    // holds none.
    {
        let me: TaskId = 0xC0FFEE;
        let mut sem = Semaphore::new(0, 10);
        if !holds_unit_after_park(&mut sem, me) {
            serial_println!("[sem]   FAIL: a waiter off the queue was not granted");
            return Err(KernelError::InternalError);
        }
        sem.waiters.push_back(me);
        if holds_unit_after_park(&mut sem, me) || sem.waiters.len() != 1 {
            serial_println!("[sem]   FAIL: a queued waiter with no unit free was granted");
            return Err(KernelError::InternalError);
        }
        sem.count = 1;
        if !holds_unit_after_park(&mut sem, me) || !sem.waiters.is_empty() || sem.count != 0 {
            serial_println!("[sem]   FAIL: a queued waiter did not take the free unit and leave");
            return Err(KernelError::InternalError);
        }
        serial_println!("[sem]   A unit after a park: OK");
    }

    // Test 8: a timed wait that times out leaves no entry behind, which a
    // later signal would spend its unit on.
    {
        let h = create(0, 10);
        let timed_out = matches!(wait_timeout(h, 1_000_000), Err(KernelError::TimedOut));
        signal(h, 1)?;
        let left = count(h)?;
        close(h);
        if !timed_out || left != 1 {
            serial_println!(
                "[sem]   FAIL: a timed-out wait (timed out {}) left the next unit uncounted ({})",
                timed_out,
                left
            );
            return Err(KernelError::InternalError);
        }
        serial_println!("[sem]   Timed-out wait leaves the queue: OK");
    }

    serial_println!("[sem] IPC semaphore self-test PASSED");
    Ok(())
}

/// Waiters in [`self_test`]'s test 6: more than the 16 an earlier `signal`
/// could wake at once.
const SEM_MANY_WAITERS: usize = 20;

/// How many of test 6's waiters have acquired.
static SEM_MANY_ACQUIRED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Task entry for test 6: wait once, count the acquisition.
extern "C" fn sem_many_waiter_task(h_raw: u64) {
    if wait(SemHandle::from_raw(h_raw)).is_ok() {
        SEM_MANY_ACQUIRED.fetch_add(1, AtOrd::AcqRel);
    }
}
