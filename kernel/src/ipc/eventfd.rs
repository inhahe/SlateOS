//! Eventfd — lightweight kernel-managed wake-up counter.
//!
//! An eventfd is a simple signaling mechanism: a kernel-managed 64-bit
//! unsigned integer counter.  Tasks can:
//!
//! - **Write (signal)**: atomically add a value to the counter.
//! - **Read (wait)**: block until the counter is non-zero, then
//!   atomically read and reset it.
//!
//! Eventfds are lighter than channels (no message allocation, no
//! queue management) and ideal for simple wake-up notifications
//! where the only question is "did something happen?" — not "what
//! happened?"
//!
//! ## Use Cases
//!
//! - Producer→consumer event notification (producer signals, consumer
//!   wakes and processes work from a shared queue).
//! - Thread-pool wake-up (signal N workers to check for new tasks).
//! - Timer expiration notification.
//! - Integration with IOCP/completion port (eventfd handles are
//!   waitable objects).
//!
//! ## Semantics
//!
//! - **`eventfd_write(handle, value)`**: adds `value` to the counter.
//!   If the result would overflow `u64::MAX - 1`, the write blocks
//!   (or returns `WouldBlock` in non-blocking mode).  Value of 0 is
//!   a no-op.
//!
//! - **`eventfd_read(handle)`**: if counter > 0, returns the counter
//!   value and resets it to 0.  If counter == 0, blocks until a write
//!   occurs.
//!
//! - **`eventfd_try_read(handle)`**: same but returns `WouldBlock`
//!   instead of blocking.
//!
//! ## Performance Target
//!
//! Wake latency: 0.5–1 µs (comparable to Linux eventfd).
//!
//! ## Lock Ordering
//!
//! `EVENTFD_TABLE` → `SCHED` (read/write may call `sched::wake()`).

use alloc::collections::BTreeMap;
use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum counter value.  Writes that would exceed this block.
/// `u64::MAX` is reserved as a sentinel (matches Linux semantics).
#[allow(clippy::arithmetic_side_effects)]
const MAX_COUNTER: u64 = u64::MAX - 1;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for an eventfd.
type EventFdId = u64;

/// Counter for generating unique IDs.
static NEXT_EVENTFD_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_eventfd_id() -> EventFdId {
    NEXT_EVENTFD_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to an eventfd counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventFdHandle(u64);

impl EventFdHandle {
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

    /// The eventfd ID (the handle IS the ID).
    fn id(self) -> EventFdId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Eventfd internals
// ---------------------------------------------------------------------------

/// A kernel eventfd: a 64-bit counter with wait/wake semantics.
struct EventFd {
    /// The counter value.
    counter: u64,
    /// Task blocked waiting to read (counter is 0).
    reader_waiter: Option<TaskId>,
    /// Task blocked waiting to write (counter would overflow).
    writer_waiter: Option<TaskId>,
    /// Whether the eventfd has been closed.
    closed: bool,
    /// Semaphore mode: `read()` decrements the counter by 1 and returns
    /// 1 (instead of draining the counter to 0 and returning its full
    /// value).  Matches Linux `EFD_SEMAPHORE` semantics.
    semaphore: bool,
    /// Reference count.  Each successful `create()` or `dup()` adds 1;
    /// each `close()` subtracts 1.  The entry is removed from the
    /// global table only when this drops to 0.  Allows multiple PCBs
    /// to hold the same eventfd handle (e.g. spawn-time fd inheritance)
    /// without one process's `close()` invalidating another's handle.
    refcount: u32,
}

impl EventFd {
    fn new(initial: u64, semaphore: bool) -> Self {
        Self {
            counter: initial.min(MAX_COUNTER),
            reader_waiter: None,
            writer_waiter: None,
            closed: false,
            semaphore,
            refcount: 1,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all live eventfds.
///
/// Lock ordering: `EVENTFD_TABLE` → `SCHED`.
static EVENTFD_TABLE: Mutex<BTreeMap<EventFdId, EventFd>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Signal-interruptible blocking helpers
// ---------------------------------------------------------------------------
//
// An eventfd is a slow object: a blocking `read`/`write` that is interrupted
// by a deliverable signal must wake and return so the signal's handler can run
// (mapped to ERESTARTSYS at the Linux syscall layer).  Without this a Linux
// process blocked on an empty/full eventfd could never be interrupted —
// exactly the hang-bug class fixed for pipes and stream sockets.  These
// mirror the helpers in `ipc/pipe.rs`.

/// The owning user process id of the current task, or `0` for a kernel task.
///
/// Eventfd waits are interruptible by signals only for user processes; kernel
/// tasks (`pid == 0`) have no signal state and park uninterruptibly, as before.
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

/// Park the current task for an eventfd wait, interruptibly for user processes.
///
/// For a user process this registers a signal-waiter (so `set_pending` wakes the
/// park when a deliverable signal arrives) using the register-then-recheck idiom
/// to close the post-before-park race, blocks, then deregisters.  Kernel tasks
/// park uninterruptibly.  The caller's surrounding loop must, after this
/// returns, re-acquire the table lock and re-evaluate both the eventfd state and
/// [`deliverable_signal_pending`] — a signal wake is reported by the latter, not
/// by this function.
fn park_for_eventfd(pid: u64, task: TaskId) {
    if pid == 0 {
        sched::block_current();
        return;
    }
    let deliverable = !crate::proc::signal::blocked(pid);
    crate::proc::signal::register_signalfd_waiter(pid, task, deliverable);
    if crate::proc::signal::has_pending_in_mask(pid, deliverable) {
        // A signal arrived between enqueue and registration — don't block; the
        // caller's loop will observe the pending signal and return Interrupted.
        crate::proc::signal::deregister_signalfd_waiter(pid, task);
        return;
    }
    sched::block_current();
    crate::proc::signal::deregister_signalfd_waiter(pid, task);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new eventfd with an initial counter value.
///
/// The initial value is typically 0 (event not signaled).  The eventfd
/// is created in default (non-semaphore) mode — `read()` drains the
/// entire counter to 0.  Use [`create_with_flags`] for semaphore mode.
pub fn create(initial: u64) -> EventFdHandle {
    create_with_flags(initial, false)
}

/// Create a new eventfd with an initial counter value and a semaphore
/// flag.
///
/// When `semaphore` is `true`, `read()` decrements the counter by 1
/// and returns 1 (matching Linux `EFD_SEMAPHORE` semantics).  When
/// `false`, `read()` returns the full counter value and resets it to
/// 0 (default eventfd behavior).
pub fn create_with_flags(initial: u64, semaphore: bool) -> EventFdHandle {
    let id = alloc_eventfd_id();
    let efd = EventFd::new(initial, semaphore);

    let mut table = EVENTFD_TABLE.lock();
    table.insert(id, efd);

    super::stats::eventfd_created();
    EventFdHandle(id)
}

/// Write (signal) — add `value` to the counter.
///
/// If `value` is 0, this is a no-op.  If the addition would overflow
/// `MAX_COUNTER`, the write blocks until the reader drains the counter.
///
/// # Returns
///
/// - `Ok(())` — value added.
/// - `Err(ChannelClosed)` — eventfd was closed.
/// - `Err(InvalidHandle)` — handle not found.
pub fn write(handle: EventFdHandle, value: u64) -> KernelResult<()> {
    if value == 0 {
        return Ok(());
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut table = EVENTFD_TABLE.lock();
            let efd = table
                .get_mut(&handle.id())
                .ok_or(KernelError::InvalidHandle)?;

            if efd.closed {
                return Err(KernelError::ChannelClosed);
            }

            // Check if we can add without overflow.
            if let Some(new_val) = efd.counter.checked_add(value)
                && new_val <= MAX_COUNTER
            {
                efd.counter = new_val;

                // Wake blocked reader.
                let reader = efd.reader_waiter.take();
                drop(table);

                super::stats::eventfd_signal();
                if let Some(task_id) = reader {
                    super::stats::eventfd_wakeup();
                    sched::wake(task_id);
                }
                return Ok(());
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  An interrupted indefinite
            // eventfd write is restartable (ERESTARTSYS) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if efd.writer_waiter == Some(task) {
                    efd.writer_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            // Would overflow — block until reader drains.
            efd.writer_waiter = Some(task);
        }

        park_for_eventfd(pid, task);
    }
}

/// Non-blocking write.
///
/// # Returns
///
/// - `Ok(())` — value added.
/// - `Err(WouldBlock)` — counter would overflow.
/// - `Err(ChannelClosed)` — eventfd closed.
pub fn try_write(handle: EventFdHandle, value: u64) -> KernelResult<()> {
    if value == 0 {
        return Ok(());
    }

    let wake_reader;

    {
        let mut table = EVENTFD_TABLE.lock();
        let efd = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if efd.closed {
            return Err(KernelError::ChannelClosed);
        }

        if let Some(new_val) = efd.counter.checked_add(value)
            && new_val <= MAX_COUNTER
        {
            efd.counter = new_val;
            wake_reader = efd.reader_waiter.take();
        } else {
            return Err(KernelError::WouldBlock);
        }
    }

    super::stats::eventfd_signal();
    if let Some(task_id) = wake_reader {
        super::stats::eventfd_wakeup();
        sched::wake(task_id);
    }
    Ok(())
}

/// Write (signal) with a timeout (nanoseconds).
///
/// Adds `value` to the counter.  If the result would overflow
/// `MAX_COUNTER`, blocks up to `timeout_ns` nanoseconds waiting for
/// a reader to drain the counter.
///
/// `timeout_ns = 0` is equivalent to `try_write()` (returns `TimedOut`
/// instead of `WouldBlock` on overflow).
///
/// # Returns
///
/// - `Ok(())` — value added.
/// - `Err(TimedOut)` — no space within the deadline.
/// - `Err(ChannelClosed)` — eventfd closed.
/// - `Err(InvalidHandle)` — not found.
pub fn write_timeout(handle: EventFdHandle, value: u64, timeout_ns: u64) -> KernelResult<()> {
    if value == 0 {
        return Ok(());
    }

    // Fast path.
    {
        let mut table = EVENTFD_TABLE.lock();
        let efd = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if efd.closed {
            return Err(KernelError::ChannelClosed);
        }

        if let Some(new_val) = efd.counter.checked_add(value)
            && new_val <= MAX_COUNTER
        {
            efd.counter = new_val;
            let reader = efd.reader_waiter.take();
            drop(table);
            super::stats::eventfd_signal();
            if let Some(task_id) = reader {
                super::stats::eventfd_wakeup();
                sched::wake(task_id);
            }
            return Ok(());
        }
    }

    // Non-blocking mode.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule timer.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let timer_handle = crate::hrtimer::schedule_ns(
        timeout_ns,
        timeout_wake,
        task,
    );

    // Block loop.
    loop {
        {
            let mut table = EVENTFD_TABLE.lock();
            let efd = table
                .get_mut(&handle.id())
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::InvalidHandle
                })?;

            if efd.closed {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            if let Some(new_val) = efd.counter.checked_add(value)
                && new_val <= MAX_COUNTER
            {
                efd.counter = new_val;
                let reader = efd.reader_waiter.take();
                crate::hrtimer::cancel(timer_handle);
                drop(table);
                super::stats::eventfd_signal();
                if let Some(task_id) = reader {
                    super::stats::eventfd_wakeup();
                    sched::wake(task_id);
                }
                return Ok(());
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  A timed wait maps the
            // interruption to EINTR (no restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if efd.writer_waiter == Some(task) {
                    efd.writer_waiter = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::Interrupted);
            }

            // Register as waiter.
            efd.writer_waiter = Some(task);
        }

        park_for_eventfd(pid, task);
    }
}

/// Read (wait) — consume the counter value.
///
/// Blocks until the counter is non-zero, then returns the value and
/// resets the counter to 0.
///
/// # Returns
///
/// - `Ok(value)` where `value > 0` — the counter was consumed.
/// - `Err(ChannelClosed)` — eventfd was closed while waiting.
/// - `Err(InvalidHandle)` — handle not found.
pub fn read(handle: EventFdHandle) -> KernelResult<u64> {
    let pid = current_user_pid();
    let task = sched::current_task_id();
    loop {
        {
            let mut table = EVENTFD_TABLE.lock();
            let efd = table
                .get_mut(&handle.id())
                .ok_or(KernelError::InvalidHandle)?;

            if efd.counter > 0 {
                let val = if efd.semaphore {
                    // Semaphore mode: decrement by 1, return 1.
                    // `counter > 0` guarantees the subtraction can't underflow.
                    efd.counter = efd.counter.saturating_sub(1);
                    1
                } else {
                    // Default mode: drain the counter.
                    let v = efd.counter;
                    efd.counter = 0;
                    v
                };

                // Wake blocked writer (now that counter has room again).
                let writer = efd.writer_waiter.take();
                drop(table);

                super::stats::eventfd_read();
                if let Some(task_id) = writer {
                    sched::wake(task_id);
                }
                return Ok(val);
            }

            if efd.closed {
                return Err(KernelError::ChannelClosed);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  An interrupted indefinite
            // eventfd read is restartable (ERESTARTSYS) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if efd.reader_waiter == Some(task) {
                    efd.reader_waiter = None;
                }
                return Err(KernelError::Interrupted);
            }

            // Counter is 0 — block.
            efd.reader_waiter = Some(task);
        }

        park_for_eventfd(pid, task);
    }
}

/// Non-blocking read.
///
/// # Returns
///
/// - `Ok(value)` — counter consumed.
/// - `Err(WouldBlock)` — counter is 0.
/// - `Err(ChannelClosed)` — eventfd closed.
pub fn try_read(handle: EventFdHandle) -> KernelResult<u64> {
    let wake_writer;
    let result;

    {
        let mut table = EVENTFD_TABLE.lock();
        let efd = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if efd.counter > 0 {
            if efd.semaphore {
                // Semaphore mode: decrement by 1, return 1.
                efd.counter = efd.counter.saturating_sub(1);
                result = Ok(1);
            } else {
                result = Ok(efd.counter);
                efd.counter = 0;
            }
            wake_writer = efd.writer_waiter.take();
        } else if efd.closed {
            return Err(KernelError::ChannelClosed);
        } else {
            return Err(KernelError::WouldBlock);
        }
    }

    super::stats::eventfd_read();
    if let Some(task_id) = wake_writer {
        sched::wake(task_id);
    }
    result
}

/// Read with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` nanoseconds waiting for the counter to
/// become non-zero.  Returns `Err(TimedOut)` if the deadline expires.
///
/// `timeout_ns = 0` is equivalent to `try_read()` (returns `TimedOut`
/// instead of `WouldBlock` when counter is 0).
///
/// # Returns
///
/// - `Ok(value)` — counter consumed.
/// - `Err(TimedOut)` — no signal within the deadline.
/// - `Err(ChannelClosed)` — eventfd closed.
/// - `Err(InvalidHandle)` — not found.
pub fn read_timeout(handle: EventFdHandle, timeout_ns: u64) -> KernelResult<u64> {
    // Fast path.
    {
        let mut table = EVENTFD_TABLE.lock();
        let efd = table
            .get_mut(&handle.id())
            .ok_or(KernelError::InvalidHandle)?;

        if efd.counter > 0 {
            let val = if efd.semaphore {
                efd.counter = efd.counter.saturating_sub(1);
                1
            } else {
                let v = efd.counter;
                efd.counter = 0;
                v
            };
            let writer = efd.writer_waiter.take();
            drop(table);
            super::stats::eventfd_read();
            if let Some(task_id) = writer {
                sched::wake(task_id);
            }
            return Ok(val);
        }

        if efd.closed {
            return Err(KernelError::ChannelClosed);
        }
    }

    // Non-blocking mode.
    if timeout_ns == 0 {
        return Err(KernelError::TimedOut);
    }

    // Schedule timer.
    let deadline_ns = crate::hrtimer::now_ns().saturating_add(timeout_ns);

    fn timeout_wake(tid: u64) {
        if !sched::try_wake(tid) {
            sched::defer_wake(tid);
        }
    }

    let pid = current_user_pid();
    let task = sched::current_task_id();
    let timer_handle = crate::hrtimer::schedule_ns(
        timeout_ns,
        timeout_wake,
        task,
    );

    // Block loop.
    loop {
        {
            let mut table = EVENTFD_TABLE.lock();
            let efd = table
                .get_mut(&handle.id())
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::InvalidHandle
                })?;

            if efd.counter > 0 {
                let val = if efd.semaphore {
                    efd.counter = efd.counter.saturating_sub(1);
                    1
                } else {
                    let v = efd.counter;
                    efd.counter = 0;
                    v
                };
                let writer = efd.writer_waiter.take();
                crate::hrtimer::cancel(timer_handle);
                drop(table);
                super::stats::eventfd_read();
                if let Some(task_id) = writer {
                    sched::wake(task_id);
                }
                return Ok(val);
            }

            if efd.closed {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            // Check timeout.
            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            // Honour a deliverable signal before parking; clear any stale
            // waiter slot from a prior signal wake.  A timed wait maps the
            // interruption to EINTR (no restart) at the syscall layer.
            if deliverable_signal_pending(pid) {
                if efd.reader_waiter == Some(task) {
                    efd.reader_waiter = None;
                }
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::Interrupted);
            }

            // Register as waiter.
            efd.reader_waiter = Some(task);
        }

        park_for_eventfd(pid, task);
    }
}

/// Duplicate an eventfd handle reference.
///
/// Increments the reference count on the eventfd and returns the same
/// handle.  The caller must `close()` the handle when done — only the
/// final `close()` (refcount → 0) tears down the eventfd and wakes any
/// waiters.
///
/// Used at spawn time so a parent and child can each hold the same
/// eventfd without one's `close()` invalidating the other's handle.
///
/// # Returns
///
/// - `Ok(handle)` — refcount incremented; same handle returned.
/// - `Err(InvalidHandle)` — handle not found (already fully closed)
///   or the refcount would overflow `u32::MAX`.
pub fn dup(handle: EventFdHandle) -> KernelResult<EventFdHandle> {
    let mut table = EVENTFD_TABLE.lock();
    let efd = table
        .get_mut(&handle.id())
        .ok_or(KernelError::InvalidHandle)?;

    // Refcount overflow is a kernel bug or a hostile caller — refuse.
    efd.refcount = efd
        .refcount
        .checked_add(1)
        .ok_or(KernelError::InvalidHandle)?;

    Ok(handle)
}

/// Close (drop one reference to) an eventfd handle.
///
/// Decrements the refcount.  Only the final close (refcount → 0)
/// removes the entry from the table and wakes any blocked reader or
/// writer (they will see `ChannelClosed`).
pub fn close(handle: EventFdHandle) {
    let mut wake_tasks: [Option<TaskId>; 2] = [None, None];

    {
        let mut table = EVENTFD_TABLE.lock();
        if let Some(efd) = table.get_mut(&handle.id()) {
            // Decrement refcount.  `saturating_sub` guards against an
            // accidental double-close pushing it below zero, but in
            // practice each close must pair with a successful create
            // or dup — so this should never saturate.
            efd.refcount = efd.refcount.saturating_sub(1);
            if efd.refcount > 0 {
                // Still referenced — keep the entry alive.
                return;
            }

            // Final close: mark closed, drain waiters, remove entry.
            efd.closed = true;
            wake_tasks[0] = efd.reader_waiter.take();
            wake_tasks[1] = efd.writer_waiter.take();
            table.remove(&handle.id());
        }
    }

    for task_id in wake_tasks.iter().flatten() {
        sched::wake(*task_id);
    }
}

// ---------------------------------------------------------------------------
// Polling helper (for completion port)
// ---------------------------------------------------------------------------

/// Check if the eventfd counter is non-zero (non-consuming).
///
/// Returns `true` if `read()` would not block (counter > 0 or
/// eventfd is closed).  Returns `false` if the counter is 0 and
/// the eventfd is still open.
pub fn has_value(handle: EventFdHandle) -> bool {
    let table = EVENTFD_TABLE.lock();
    let Some(efd) = table.get(&handle.id()) else {
        return false;
    };
    efd.counter > 0 || efd.closed
}

/// Linux `poll(2)`-style readiness bits for an eventfd.
///
/// Bits use the same numeric values as `linux::syscall::poll_bits`:
///   - 0x0001 POLLIN    — counter > 0 (read won't block).
///   - 0x0004 POLLOUT   — counter < MAX_COUNTER (write won't block).
///   - 0x0008 POLLERR   — handle no longer exists in the table.
///   - 0x0010 POLLHUP   — eventfd closed.
///
/// Returns 0 if the handle exists and is mid-state (closed=false,
/// counter=0).  Returns POLLERR for unknown handles.
#[must_use]
pub fn poll_status(handle: EventFdHandle) -> u16 {
    let table = EVENTFD_TABLE.lock();
    let Some(efd) = table.get(&handle.id()) else {
        return 0x0008; // POLLERR — no such eventfd
    };
    let mut bits = 0u16;
    if efd.counter > 0 {
        bits |= 0x0001; // POLLIN
    }
    if efd.counter < MAX_COUNTER {
        bits |= 0x0004; // POLLOUT
    }
    if efd.closed {
        bits |= 0x0010; // POLLHUP
    }
    bits
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run eventfd self-tests.
///
/// Tests:
/// 1. Create with initial value, read it back.
/// 2. Write signals, accumulate counter.
/// 3. Read resets counter to 0.
/// 4. Non-blocking read on empty counter.
/// 5. Blocking read via spawned task.
/// 6. Close wakes blocked reader.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[eventfd] Running eventfd self-test...");

    test_initial_value()?;
    test_write_accumulate()?;
    test_read_resets()?;
    test_nonblocking()?;
    test_blocking_read()?;
    test_semaphore_mode()?;
    test_dup_refcount()?;

    serial_println!("[eventfd] Eventfd self-test PASSED");
    Ok(())
}

/// Test 1: create with initial value, read it back.
fn test_initial_value() -> KernelResult<()> {
    let handle = create(5);
    let val = read(handle)?;
    if val != 5 {
        serial_println!("[eventfd]   FAIL: initial read {} expected 5", val);
        close(handle);
        return Err(KernelError::InternalError);
    }
    close(handle);
    serial_println!("[eventfd]   Initial value: OK");
    Ok(())
}

/// Test 2: multiple writes accumulate.
fn test_write_accumulate() -> KernelResult<()> {
    let handle = create(0);
    write(handle, 3)?;
    write(handle, 7)?;
    let val = read(handle)?;
    if val != 10 {
        serial_println!("[eventfd]   FAIL: accumulated {} expected 10", val);
        close(handle);
        return Err(KernelError::InternalError);
    }
    close(handle);
    serial_println!("[eventfd]   Write accumulate: OK");
    Ok(())
}

/// Test 3: read resets counter to 0.
fn test_read_resets() -> KernelResult<()> {
    let handle = create(0);
    write(handle, 42)?;
    let _ = read(handle)?; // Consume.

    // Counter should be 0 now — try_read should return WouldBlock.
    match try_read(handle) {
        Err(KernelError::WouldBlock) => {} // Expected.
        other => {
            serial_println!("[eventfd]   FAIL: after reset, try_read: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    close(handle);
    serial_println!("[eventfd]   Read resets counter: OK");
    Ok(())
}

/// Test 4: non-blocking on empty counter.
fn test_nonblocking() -> KernelResult<()> {
    let handle = create(0);

    // try_read on 0 counter → WouldBlock.
    match try_read(handle) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!("[eventfd]   FAIL: try_read(0): {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    // try_write should succeed.
    try_write(handle, 1)?;
    let val = try_read(handle)?;
    if val != 1 {
        serial_println!("[eventfd]   FAIL: try_read got {}, expected 1", val);
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    serial_println!("[eventfd]   Non-blocking: OK");
    Ok(())
}

/// Run eventfd timeout self-tests.
///
/// Must be called after hrtimer init (uses `hrtimer::schedule_ns`).
///
/// Tests:
/// 1. Timeout on empty counter returns `TimedOut`.
/// 2. Immediate read with timeout when counter > 0 succeeds.
/// 3. Signaled before timeout expires — returns value.
pub fn self_test_timeout() -> KernelResult<()> {
    serial_println!("[eventfd] Running timeout self-test...");

    test_timeout_expires()?;
    test_timeout_fast_path()?;
    test_timeout_signaled()?;

    serial_println!("[eventfd] Timeout self-test PASSED");
    Ok(())
}

/// Timeout test 1: empty counter, timeout expires.
fn test_timeout_expires() -> KernelResult<()> {
    let handle = create(0);

    match read_timeout(handle, 5_000_000) {
        Err(KernelError::TimedOut) => {} // Expected.
        other => {
            serial_println!("[eventfd]   FAIL: timeout_expires: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    close(handle);
    serial_println!("[eventfd]   Timeout expires: OK");
    Ok(())
}

/// Timeout test 2: counter > 0, returns immediately (fast path).
fn test_timeout_fast_path() -> KernelResult<()> {
    let handle = create(0);
    write(handle, 99)?;

    match read_timeout(handle, 100_000_000) {
        Ok(99) => {}
        other => {
            serial_println!("[eventfd]   FAIL: timeout_fast_path: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    close(handle);
    serial_println!("[eventfd]   Timeout fast path: OK");
    Ok(())
}

/// Atomic for the signaled-before-timeout test.
static EVENTFD_TIMEOUT_RESULT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Task that reads with a generous timeout (should be signaled long before).
///
/// The timeout is deliberately large (5 s) relative to the ~5 ms the driver
/// takes to signal: this test verifies the *signaled-before-expiry* path, so
/// the timeout must never fire under normal — or even momentarily starved —
/// boot-time scheduling. A short (500 ms) timeout here made the test flaky:
/// under transient scheduler contention during the busy boot self-test phase
/// the driver task could be delayed past the reader's deadline, so the reader
/// legitimately timed out (returning the `u64::MAX` error sentinel) even though
/// the signal path itself was correct. See known-issues.md (2026-07-15 eventfd
/// timeout self-test flake).
extern "C" fn eventfd_timeout_reader_task(handle_raw: u64) {
    let handle = EventFdHandle::from_raw(handle_raw);
    match read_timeout(handle, 5_000_000_000) {
        Ok(val) => {
            EVENTFD_TIMEOUT_RESULT.store(val, core::sync::atomic::Ordering::SeqCst);
        }
        _ => {
            EVENTFD_TIMEOUT_RESULT.store(u64::MAX, core::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// Timeout test 3: reader blocks with timeout, signaled before expiry.
fn test_timeout_signaled() -> KernelResult<()> {
    EVENTFD_TIMEOUT_RESULT.store(0, core::sync::atomic::Ordering::SeqCst);

    let handle = create(0);

    // Spawn reader that will block with a generous 5 s timeout.
    sched::spawn(b"efd-to-test", 16, eventfd_timeout_reader_task, handle.raw(), 0)?;

    // Let reader run and block.
    sched::yield_now();

    // Signal after a tiny delay (many orders below the 5 s timeout).
    sched::sleep_ms(5);
    write(handle, 42)?;

    // Poll for the reader to wake and store its result, rather than assuming a
    // fixed number of yields/sleeps is enough — the reader may not be scheduled
    // promptly under boot-time contention. Bounded so a genuine signal-path bug
    // still fails the test in ~1 s instead of hanging.
    let mut result = 0u64;
    for _ in 0..200 {
        sched::yield_now();
        sched::sleep_ms(5);
        result = EVENTFD_TIMEOUT_RESULT.load(core::sync::atomic::Ordering::SeqCst);
        if result != 0 {
            break;
        }
    }

    if result != 42 {
        serial_println!("[eventfd]   FAIL: timeout_signaled: got {}", result);
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    serial_println!("[eventfd]   Timeout signaled: OK");
    Ok(())
}

/// Result counter for blocking test.
static EVENTFD_TEST_RESULT: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Task for the blocking read test.
extern "C" fn eventfd_reader_task(handle_raw: u64) {
    let handle = EventFdHandle::from_raw(handle_raw);
    if let Ok(val) = read(handle) {
        #[allow(clippy::cast_possible_truncation)]
        EVENTFD_TEST_RESULT.store(val as u32, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Test 5: blocking read via spawned task.
fn test_blocking_read() -> KernelResult<()> {
    EVENTFD_TEST_RESULT.store(0, core::sync::atomic::Ordering::SeqCst);

    let handle = create(0); // Start at 0 — reader will block.

    // Spawn reader task.
    sched::spawn(b"efd-test", 16, eventfd_reader_task, handle.raw(), 0)?;

    // Yield to let reader run and block.
    sched::yield_now();

    // Signal the eventfd.
    write(handle, 77)?;

    // Yield to let reader wake and process.
    sched::yield_now();
    sched::yield_now();

    let result = EVENTFD_TEST_RESULT.load(core::sync::atomic::Ordering::SeqCst);
    if result != 77 {
        serial_println!("[eventfd]   FAIL: reader got {}, expected 77", result);
        close(handle);
        return Err(KernelError::InternalError);
    }

    close(handle);
    serial_println!("[eventfd]   Blocking read: OK");
    Ok(())
}

/// Test 6: semaphore-mode read decrements by 1 and returns 1.
///
/// Matches Linux `EFD_SEMAPHORE` semantics: each read returns 1 (not
/// the full counter) and decrements the counter by 1.  After N reads
/// of an eventfd seeded with N, subsequent reads block (or return
/// `WouldBlock`).
fn test_semaphore_mode() -> KernelResult<()> {
    let handle = create_with_flags(3, true);

    // Three reads each return 1 (decrementing 3 → 2 → 1 → 0).
    for expected_remaining in (0..3).rev() {
        let val = try_read(handle)?;
        if val != 1 {
            serial_println!(
                "[eventfd]   FAIL: semaphore read (remaining={}) got {}, expected 1",
                expected_remaining,
                val
            );
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    // Fourth read finds counter == 0 → WouldBlock.
    match try_read(handle) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!("[eventfd]   FAIL: semaphore drained try_read: {:?}", other);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    // A write of 5 makes 5 more semaphore reads available.
    write(handle, 5)?;
    for _ in 0..5 {
        let val = try_read(handle)?;
        if val != 1 {
            serial_println!("[eventfd]   FAIL: post-write semaphore read got {}", val);
            close(handle);
            return Err(KernelError::InternalError);
        }
    }
    match try_read(handle) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!(
                "[eventfd]   FAIL: post-write semaphore drained try_read: {:?}",
                other
            );
            close(handle);
            return Err(KernelError::InternalError);
        }
    }

    close(handle);
    serial_println!("[eventfd]   Semaphore mode: OK");
    Ok(())
}

/// Test 7: `dup()` increments the refcount; the entry survives until
/// the final `close()`.
fn test_dup_refcount() -> KernelResult<()> {
    let h = create(0);

    // Dup once — refcount goes 1 → 2.
    let h2 = dup(h)?;
    if h2 != h {
        serial_println!("[eventfd]   FAIL: dup returned a different handle");
        close(h);
        close(h2);
        return Err(KernelError::InternalError);
    }

    // Write/read still works on either handle.
    write(h, 5)?;
    let val = read(h2)?;
    if val != 5 {
        serial_println!("[eventfd]   FAIL: dup'd read got {}, expected 5", val);
        close(h);
        close(h2);
        return Err(KernelError::InternalError);
    }

    // Close once — refcount 2 → 1.  The handle must still be valid.
    close(h);
    match try_read(h2) {
        // Counter is 0 (we drained it above) so try_read returns
        // WouldBlock — _not_ InvalidHandle.  WouldBlock proves the
        // entry survived the first close.
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!(
                "[eventfd]   FAIL: after partial close, try_read: {:?}",
                other
            );
            close(h2);
            return Err(KernelError::InternalError);
        }
    }

    // Final close — refcount 1 → 0.  The entry is removed.
    close(h2);
    match try_read(h2) {
        Err(KernelError::InvalidHandle) => {}
        other => {
            serial_println!(
                "[eventfd]   FAIL: after final close, try_read: {:?}",
                other
            );
            return Err(KernelError::InternalError);
        }
    }

    // dup on a fully-closed handle must fail.
    match dup(h2) {
        Err(KernelError::InvalidHandle) => {}
        other => {
            serial_println!("[eventfd]   FAIL: dup after final close: {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[eventfd]   Dup refcount: OK");
    Ok(())
}
