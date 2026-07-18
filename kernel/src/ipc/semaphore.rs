//! IPC Semaphore — counting semaphore for userspace synchronization.
//!
//! An IPC semaphore is a kernel-managed counting resource that tasks
//! can signal (increment) and wait on (decrement).  Unlike the
//! kernel-internal [`Semaphore`](crate::sched::sync::Semaphore), this
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

use alloc::collections::{BTreeMap, VecDeque};
use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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
static SEM_TABLE: Mutex<BTreeMap<SemId, Semaphore>> =
    Mutex::new(BTreeMap::new());

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
    let max = if max_count == 0 { DEFAULT_MAX_COUNT } else { max_count };
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

    let mut to_wake: [Option<TaskId>; 16] = [None; 16];
    let mut wake_count = 0usize;

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

        // Wake waiters first (one unit per waiter).
        let mut units_left = count;
        while units_left > 0 && !sem.waiters.is_empty() {
            if let Some(task_id) = sem.waiters.pop_front() {
                if wake_count < to_wake.len() {
                    to_wake[wake_count] = Some(task_id);
                    wake_count = wake_count.saturating_add(1);
                } else {
                    // More than 16 to wake — put it back, remaining goes to count.
                    sem.waiters.push_front(task_id);
                    break;
                }
                units_left = units_left.saturating_sub(1);
            }
        }

        // Remaining units go to the count.
        sem.count = sem.count.saturating_add(units_left);
    }

    crate::ktrace::record(
        crate::ktrace::Category::Ipc,
        crate::ktrace::event::SEM_SIGNAL,
        handle.raw(),
        count,
    );

    // Wake collected tasks outside the lock.
    for task_id in to_wake.iter().flatten() {
        sched::wake(*task_id);
    }

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

    sched::block_current();

    // We were woken.  Two possibilities:
    // 1. signal() removed us from the waiter queue and woke us — a
    //    unit has been consumed on our behalf → return Ok.
    // 2. close() drained all waiters and woke us — sem is gone.
    //
    // Distinguish by re-checking whether the semaphore still exists.
    // If it's been removed from the table (close removes it),
    // return ChannelClosed.  If it still exists, we were woken by
    // signal (which removed us from the queue — we won't be in it).
    {
        let table = SEM_TABLE.lock();
        match table.get(&handle.id()) {
            None => Err(KernelError::ChannelClosed),
            Some(sem) if sem.closed => Err(KernelError::ChannelClosed),
            Some(_) => Ok(()), // Woken by signal.
        }
    }
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

    // Block loop.
    loop {
        {
            let mut table = SEM_TABLE.lock();
            let sem = table
                .get_mut(&handle.id())
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::ChannelClosed
                })?;

            if sem.closed {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            if sem.count > 0 {
                sem.count = sem.count.saturating_sub(1);
                crate::hrtimer::cancel(timer_handle);
                return Ok(());
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                // Remove ourselves from the waiter queue if present.
                if let Some(pos) = sem.waiters.iter().position(|&id| id == task_id) {
                    sem.waiters.remove(pos);
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Register as a waiter.
            if sem.waiters.len() >= MAX_WAITERS {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::WouldBlock);
            }
            // Only add if not already in the queue (re-entry from loop).
            if !sem.waiters.iter().any(|&id| id == task_id) {
                sem.waiters.push_back(task_id);
            }
        }

        sched::block_current();

        // Woken by either signal() or timer.
        // If signal() woke us, it already removed us from the queue
        // and consumed a unit on our behalf → check the table.
        {
            let table = SEM_TABLE.lock();
            match table.get(&handle.id()) {
                None => {
                    crate::hrtimer::cancel(timer_handle);
                    return Err(KernelError::ChannelClosed);
                }
                Some(sem) if sem.closed => {
                    crate::hrtimer::cancel(timer_handle);
                    return Err(KernelError::ChannelClosed);
                }
                Some(sem) => {
                    // If we're no longer in the waiter queue, signal()
                    // consumed a unit for us.
                    if !sem.waiters.iter().any(|&id| id == task_id) {
                        crate::hrtimer::cancel(timer_handle);
                        return Ok(());
                    }
                    // Otherwise, timer woke us — loop back to check timeout.
                }
            }
        }
    }
}

/// Get the current count (non-consuming peek).
///
/// Returns the current count, or `Err(InvalidHandle)`.
#[allow(dead_code)] // Used in self-test; also useful API for userspace query.
pub fn count(handle: SemHandle) -> KernelResult<u64> {
    let table = SEM_TABLE.lock();
    let sem = table
        .get(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;
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

/// Task entry: blocks on sem wait, expects ChannelClosed.
extern "C" fn sem_close_waiter_task(h_raw: u64) {
    let handle = SemHandle::from_raw(h_raw);
    match wait(handle) {
        Err(KernelError::ChannelClosed) => {
            // Expected — woken by close.
        }
        other => {
            serial_println!(
                "[sem]   FAIL: waiter got {:?} instead of ChannelClosed",
                other
            );
        }
    }
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

        let h = create(0, 10);

        // Spawn a task that will block on wait.
        let _ = sched::spawn(
            b"sem-close-test",
            16,
            sem_close_waiter_task,
            h.raw(),
            0,
        );

        // Give the waiter time to block.
        sched::yield_now();
        sched::yield_now();

        // Close should wake the waiter.
        close(h);

        // Wait for the waiter task to exit.
        for _ in 0..10 {
            sched::yield_now();
        }
        serial_println!("[sem]   Close wakes waiter: OK");
    }

    // Test 5: Signal wakes blocked waiter.
    {
        use crate::sched;

        SEM_TEST_ACQUIRED.store(false, AtOrd::Release);

        let h = create(0, 10);

        // Spawn a task that blocks on wait.
        let _ = sched::spawn(
            b"sem-signal-test",
            16,
            sem_signal_waiter_task,
            h.raw(),
            0,
        );

        // Yield to let waiter block.
        sched::yield_now();
        sched::yield_now();

        // Signal should wake the waiter.
        let _ = signal(h, 1);
        for _ in 0..10 {
            sched::yield_now();
        }

        if !SEM_TEST_ACQUIRED.load(AtOrd::Acquire) {
            serial_println!("[sem]   FAIL: waiter not woken by signal");
            close(h);
            return Err(KernelError::InternalError);
        }

        close(h);
        serial_println!("[sem]   Signal wakes waiter: OK");
    }

    serial_println!("[sem] IPC semaphore self-test PASSED");
    Ok(())
}
