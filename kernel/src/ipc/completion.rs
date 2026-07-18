//! Completion port — unified wait on heterogeneous kernel objects.
//!
//! A completion port (CP) is the kernel's primary multiplexing mechanism.
//! Instead of separate wait APIs for each object type (channels, pipes,
//! eventfds, timers), a task registers multiple waitable objects with a
//! single completion port and waits for any of them to become ready.
//!
//! ## Design (IOCP-inspired)
//!
//! The API is modeled after Windows I/O Completion Ports (IOCP) and
//! Linux epoll, with the "arbitrary user-data" feature from the design
//! spec.  Key operations:
//!
//! - **`create()`**: create a new completion port.
//! - **`register(cp, source, interest, user_data)`**: register a
//!   waitable object with the port.  `user_data` is an arbitrary u64
//!   returned with completions, so the application can dispatch without
//!   a lookup table.
//! - **`unregister(cp, source)`**: remove a registration.
//! - **`wait(cp)`**: block until at least one registered object is
//!   ready, then return the events.
//! - **`try_wait(cp)`**: non-blocking poll.
//!
//! ## Waitable Object Types
//!
//! The completion port can wait on:
//!
//! - **Channels**: message available for receive.
//! - **Pipes**: data available for read, or space available for write.
//! - **Eventfds**: counter is non-zero.
//!
//! Future additions (when those subsystems exist):
//! - Timers (monotonic/wall-clock expiry).
//! - Process/thread exit.
//! - I/O completion.
//!
//! ## Notification Model
//!
//! When a registered source becomes ready, the owning IPC subsystem
//! calls [`notify()`] to post a completion event to the port.  This
//! wakes any task blocked in `wait()`.
//!
//! For the initial implementation, `wait()` uses a poll-then-block
//! approach: it checks all registered sources, and if none are ready,
//! blocks.  Sources call `notify()` when they transition to ready.
//!
//! ## Performance Target
//!
//! Sub-microsecond for ready events (no syscall needed for the poll
//! path when events are already queued).
//!
//! ## Lock Ordering
//!
//! `CP_TABLE` → individual IPC table locks (to poll sources).
//! `CP_TABLE` → `SCHED` (to wake blocked waiters).

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

// ---------------------------------------------------------------------------
// Source types
// ---------------------------------------------------------------------------

/// A waitable object that can be registered with a completion port.
///
/// Each variant identifies the kind of source and carries the handle
/// needed to poll it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitSource {
    /// A channel endpoint — ready when a message is available.
    Channel(u64),
    /// A pipe read end — ready when data is available (or EOF).
    PipeRead(u64),
    /// A pipe write end — ready when buffer has space.
    PipeWrite(u64),
    /// An eventfd — ready when counter > 0.
    EventFd(u64),
    /// A process — ready when the process exits (becomes zombie).
    ProcessExit(u64),
    /// A kernel timer — ready when the deadline has passed.
    /// The u64 is the timer handle from `timer::create()`.
    Timer(u64),
    /// An IPC semaphore — ready when count > 0.
    /// The u64 is the semaphore handle from `semaphore::create()`.
    Semaphore(u64),
    /// An io_ring — ready when the completion queue has pending entries.
    /// The u64 is the ring handle from `io_ring::setup()`.
    IoCompletion(u64),
}

impl WaitSource {
    /// Get the raw handle value for this source.
    #[allow(dead_code)] // Used by future per-source operations.
    fn raw_handle(self) -> u64 {
        match self {
            Self::Channel(h)
            | Self::PipeRead(h)
            | Self::PipeWrite(h)
            | Self::EventFd(h)
            | Self::ProcessExit(h)
            | Self::Timer(h)
            | Self::Semaphore(h)
            | Self::IoCompletion(h) => h,
        }
    }

    /// A key that uniquely identifies this source (type + handle).
    /// Used for deduplication in registrations.
    fn key(self) -> (u8, u64) {
        match self {
            Self::Channel(h) => (0, h),
            Self::PipeRead(h) => (1, h),
            Self::PipeWrite(h) => (2, h),
            Self::EventFd(h) => (3, h),
            Self::ProcessExit(h) => (4, h),
            Self::Timer(h) => (5, h),
            Self::Semaphore(h) => (6, h),
            Self::IoCompletion(h) => (7, h),
        }
    }
}

/// A completion event returned to the waiter.
#[derive(Debug, Clone, Copy)]
pub struct CompletionEvent {
    /// The source that became ready.
    pub source: WaitSource,
    /// The user-data integer provided at registration time.
    pub user_data: u64,
}

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a completion port.
type CpId = u64;

/// Counter for generating unique IDs.
static NEXT_CP_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_cp_id() -> CpId {
    NEXT_CP_ID.fetch_add(1, Ordering::Relaxed)
}

/// A handle to a completion port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CpHandle(u64);

impl CpHandle {
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

    /// The completion port ID (the handle IS the ID).
    fn id(self) -> CpId {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// A single source registration within a completion port.
struct Registration {
    /// What we're waiting on.
    source: WaitSource,
    /// Arbitrary user-data returned with completions.
    user_data: u64,
}

// ---------------------------------------------------------------------------
// Completion port internals
// ---------------------------------------------------------------------------

/// Maximum registrations per completion port.
const MAX_REGISTRATIONS: usize = 256;

/// Maximum events returned per wait call.
const MAX_EVENTS_PER_WAIT: usize = 64;

/// A kernel completion port: unified wait on multiple sources.
struct CompletionPort {
    /// Registered sources.
    registrations: Vec<Registration>,
    /// Queued completion events (from notify calls).
    event_queue: Vec<CompletionEvent>,
    /// Task blocked in `wait()`.
    waiter: Option<TaskId>,
    /// Whether the port has been closed.
    closed: bool,
}

impl CompletionPort {
    fn new() -> Self {
        Self {
            registrations: Vec::new(),
            event_queue: Vec::new(),
            waiter: None,
            closed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

/// Global table of all completion ports.
///
/// Lock ordering: `CP_TABLE` → source tables (channel/pipe/eventfd)
/// → `SCHED`.
static CP_TABLE: Mutex<BTreeMap<CpId, CompletionPort>> =
    Mutex::new(BTreeMap::new());

// ---------------------------------------------------------------------------
// Source polling helpers
// ---------------------------------------------------------------------------

/// Check if a channel has a message available (non-consuming peek).
fn poll_channel(handle_raw: u64) -> bool {
    use crate::ipc::channel::{self, ChannelHandle};
    channel::has_pending(ChannelHandle::from_raw(handle_raw))
}

/// Check if a pipe read end has data or is at EOF.
fn poll_pipe_read(handle_raw: u64) -> bool {
    use crate::ipc::pipe::{self, PipeHandle};
    pipe::readable(PipeHandle::from_raw(handle_raw))
}

/// Check if a pipe write end has buffer space.
fn poll_pipe_write(handle_raw: u64) -> bool {
    use crate::ipc::pipe::{self, PipeHandle};
    pipe::writable(PipeHandle::from_raw(handle_raw))
}

/// Check if an eventfd counter is non-zero.
fn poll_eventfd(handle_raw: u64) -> bool {
    use crate::ipc::eventfd::{self, EventFdHandle};
    eventfd::has_value(EventFdHandle::from_raw(handle_raw))
}

/// Poll a source to check if it's ready.
fn poll_source(source: WaitSource) -> bool {
    match source {
        WaitSource::Channel(h) => poll_channel(h),
        WaitSource::PipeRead(h) => poll_pipe_read(h),
        WaitSource::PipeWrite(h) => poll_pipe_write(h),
        WaitSource::EventFd(h) => poll_eventfd(h),
        WaitSource::ProcessExit(pid) => poll_process_exit(pid),
        WaitSource::Timer(h) => super::timer::is_expired(h),
        WaitSource::Semaphore(h) => {
            super::semaphore::has_value(super::semaphore::SemHandle::from_raw(h))
        }
        WaitSource::IoCompletion(h) => super::io_ring::has_completions_ready(h),
    }
}

/// Poll whether a process has exited (is zombie or gone).
fn poll_process_exit(pid: u64) -> bool {
    use crate::proc::pcb;

    match pcb::state(pid) {
        Some(pcb::ProcessState::Zombie) => true,
        None => true, // Process doesn't exist — already reaped.
        _ => false,   // Still running or creating.
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Create a new completion port.
pub fn create() -> CpHandle {
    let id = alloc_cp_id();
    let cp = CompletionPort::new();

    let mut table = CP_TABLE.lock();
    table.insert(id, cp);

    super::stats::completion_created();
    CpHandle(id)
}

/// Register a waitable source with a completion port.
///
/// `user_data` is an arbitrary u64 that will be returned with any
/// completion events from this source.  Use it to dispatch without
/// a lookup table (e.g., an array index, a pointer, or a type tag).
///
/// # Errors
///
/// - `InvalidHandle` — completion port not found.
/// - `InvalidArgument` — source already registered, or too many
///   registrations.
pub fn register(
    cp: CpHandle,
    source: WaitSource,
    user_data: u64,
) -> KernelResult<()> {
    let mut table = CP_TABLE.lock();
    let port = table
        .get_mut(&cp.id())
        .ok_or(KernelError::InvalidHandle)?;

    if port.closed {
        return Err(KernelError::ChannelClosed);
    }

    // Check for duplicate registration.
    let key = source.key();
    let already = port
        .registrations
        .iter()
        .any(|r| r.source.key() == key);
    if already {
        return Err(KernelError::AlreadyExists);
    }

    // Check capacity.
    if port.registrations.len() >= MAX_REGISTRATIONS {
        return Err(KernelError::InvalidArgument);
    }

    port.registrations.push(Registration { source, user_data });

    // For timer and io_ring sources, tell the subsystem which CP to
    // notify.  This enables push-based notification instead of poll-only.
    match source {
        WaitSource::Timer(timer_handle) => {
            super::timer::set_cp(timer_handle, cp.raw());
        }
        WaitSource::IoCompletion(ring_handle) => {
            super::io_ring::set_cp(ring_handle, cp.raw());
        }
        _ => {}
    }

    Ok(())
}

/// Unregister a source from a completion port.
///
/// Also removes any queued events for this source.
///
/// # Errors
///
/// - `InvalidHandle` — completion port not found.
/// - `NotFound` — source was not registered.
pub fn unregister(cp: CpHandle, source: WaitSource) -> KernelResult<()> {
    let mut table = CP_TABLE.lock();
    let port = table
        .get_mut(&cp.id())
        .ok_or(KernelError::InvalidHandle)?;

    let key = source.key();
    let pos = port
        .registrations
        .iter()
        .position(|r| r.source.key() == key)
        .ok_or(KernelError::NotFound)?;

    port.registrations.swap_remove(pos);

    // Remove any queued events for this source.
    port.event_queue.retain(|e| e.source.key() != key);

    // Clear the CP association for timer and io_ring sources.
    match source {
        WaitSource::Timer(timer_handle) => {
            super::timer::set_cp(timer_handle, 0);
        }
        WaitSource::IoCompletion(ring_handle) => {
            super::io_ring::set_cp(ring_handle, 0);
        }
        _ => {}
    }

    Ok(())
}

/// Post a completion event to a port (called by IPC subsystems).
///
/// This is the notification path: when a channel receives a message,
/// a pipe gets data, or an eventfd is signaled, the subsystem calls
/// this to wake any waiter on the completion port.
///
/// If `cp_handle` is invalid (port was closed), this is a silent no-op.
pub fn notify(cp: CpHandle, source: WaitSource) {
    let wake_task;

    super::stats::completion_post();

    {
        let mut table = CP_TABLE.lock();
        let Some(port) = table.get_mut(&cp.id()) else {
            return; // Port gone — silently ignore.
        };

        // Find the registration to get user_data.
        let key = source.key();
        let Some(reg) = port
            .registrations
            .iter()
            .find(|r| r.source.key() == key)
        else {
            return; // Source was unregistered — ignore.
        };

        let event = CompletionEvent {
            source,
            user_data: reg.user_data,
        };
        port.event_queue.push(event);

        // Wake the waiter if someone is blocked.
        wake_task = port.waiter.take();
    }

    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }
}

/// Softirq/ISR-safe variant of [`notify`] using only non-blocking locks.
///
/// [`notify`] acquires `CP_TABLE` and `SCHED` (via [`sched::wake`]) with
/// blocking spinlocks.  That is correct from task/syscall context but is a
/// deadlock hazard from softirq/interrupt context: a timer softirq runs with
/// interrupts enabled and can preempt a task that already holds `SCHED` (its
/// holders do not disable interrupts), so a blocking `SCHED.lock()` here would
/// spin forever on the same CPU.  This variant is used by the timer-expiration
/// softirq path ([`crate::ipc::timer::process_timer_expirations`]).
///
/// Returns `true` if the notification was fully delivered (event queued and any
/// blocked waiter woken), or if there was nothing to do (port gone / source
/// unregistered).  Returns `false` if `CP_TABLE` or `SCHED` was contended and
/// the caller should retry on the next timer tick.  On the `false` path
/// **nothing is committed** — no event is queued and no waiter is consumed — so
/// a retry neither loses a wakeup nor duplicates an event.
#[must_use]
pub fn try_notify(cp: CpHandle, source: WaitSource) -> bool {
    super::stats::completion_post();

    let Some(mut table) = CP_TABLE.try_lock() else {
        return false; // CP_TABLE contended — retry next tick.
    };
    let Some(port) = table.get_mut(&cp.id()) else {
        return true; // Port gone — nothing to do, don't retry.
    };

    // Find the registration to get user_data.
    let key = source.key();
    let Some(reg) = port
        .registrations
        .iter()
        .find(|r| r.source.key() == key)
    else {
        return true; // Source was unregistered — nothing to do.
    };
    let user_data = reg.user_data;

    // If a waiter is blocked, wake it with `try_wake` (non-blocking) *before*
    // committing the event.  If `SCHED` is contended we bail without queueing
    // anything, so the next tick's retry is a clean redo (no lost wake, no
    // duplicate event).  We hold `CP_TABLE` across the `try_wake`, matching the
    // `CP_TABLE -> SCHED` lock order established by `notify` (which drops
    // `CP_TABLE` before `wake`, but never acquires them in the reverse order).
    if let Some(task_id) = port.waiter {
        if !sched::try_wake(task_id) {
            return false; // SCHED contended — retry next tick, nothing committed.
        }
        port.waiter = None;
    }

    port.event_queue.push(CompletionEvent { source, user_data });
    true
}

/// Wait for completion events (blocking).
///
/// Returns a vector of ready events (up to `MAX_EVENTS_PER_WAIT`).
/// If no events are ready, blocks until at least one source signals.
///
/// # Errors
///
/// - `InvalidHandle` — completion port not found.
/// - `ChannelClosed` — port was closed while waiting.
pub fn wait(cp: CpHandle) -> KernelResult<Vec<CompletionEvent>> {
    loop {
        {
            let mut table = CP_TABLE.lock();
            let port = table
                .get_mut(&cp.id())
                .ok_or(KernelError::InvalidHandle)?;

            if port.closed {
                return Err(KernelError::ChannelClosed);
            }

            // First: drain any queued events from notify().
            if !port.event_queue.is_empty() {
                let drain_count = port.event_queue.len().min(MAX_EVENTS_PER_WAIT);
                let events: Vec<CompletionEvent> =
                    port.event_queue.drain(..drain_count).collect();
                super::stats::completion_wait();
                return Ok(events);
            }

            // Collect registrations so we can poll outside the lock.
            // We must drop the CP table lock before polling to avoid
            // deadlock (poll_source takes source-specific locks).
            let regs: Vec<(WaitSource, u64)> = port
                .registrations
                .iter()
                .map(|r| (r.source, r.user_data))
                .collect();

            // Record that we'll block if nothing is ready.
            port.waiter = Some(sched::current_task_id());
            drop(table);

            // Poll sources outside the CP table lock.
            let mut events = Vec::new();
            for (source, user_data) in &regs {
                if events.len() >= MAX_EVENTS_PER_WAIT {
                    break;
                }
                if poll_source(*source) {
                    events.push(CompletionEvent {
                        source: *source,
                        user_data: *user_data,
                    });
                }
            }

            if !events.is_empty() {
                // Clear the waiter — we're not actually blocking.
                let mut table = CP_TABLE.lock();
                if let Some(port) = table.get_mut(&cp.id()) {
                    // Only clear if it's still us.
                    if port.waiter == Some(sched::current_task_id()) {
                        port.waiter = None;
                    }
                }
                super::stats::completion_wait();
                return Ok(events);
            }

            // Nothing ready — fall through to block.
        }

        super::stats::completion_wait_block();
        sched::block_current();
    }
}

/// Non-blocking wait for completion events.
///
/// Returns ready events without blocking.  If nothing is ready,
/// returns `Err(WouldBlock)`.
///
/// # Errors
///
/// - `InvalidHandle` — completion port not found.
/// - `WouldBlock` — no events ready.
/// - `ChannelClosed` — port closed.
pub fn try_wait(cp: CpHandle) -> KernelResult<Vec<CompletionEvent>> {
    let regs;

    {
        let mut table = CP_TABLE.lock();
        let port = table
            .get_mut(&cp.id())
            .ok_or(KernelError::InvalidHandle)?;

        if port.closed {
            return Err(KernelError::ChannelClosed);
        }

        // Drain queued events first.
        if !port.event_queue.is_empty() {
            let drain_count = port.event_queue.len().min(MAX_EVENTS_PER_WAIT);
            let events: Vec<CompletionEvent> =
                port.event_queue.drain(..drain_count).collect();
            super::stats::completion_wait();
            return Ok(events);
        }

        // Collect registrations to poll outside the lock.
        regs = port
            .registrations
            .iter()
            .map(|r| (r.source, r.user_data))
            .collect::<Vec<_>>();
    }

    // Poll sources outside the CP table lock.
    let mut events = Vec::new();
    for (source, user_data) in &regs {
        if events.len() >= MAX_EVENTS_PER_WAIT {
            break;
        }
        if poll_source(*source) {
            events.push(CompletionEvent {
                source: *source,
                user_data: *user_data,
            });
        }
    }

    if events.is_empty() {
        Err(KernelError::WouldBlock)
    } else {
        super::stats::completion_wait();
        Ok(events)
    }
}

/// Close a completion port.
///
/// Wakes any blocked waiter (they will see `ChannelClosed`).
/// Removes all registrations.
pub fn close(cp: CpHandle) {
    let wake_task;

    {
        let mut table = CP_TABLE.lock();
        if let Some(port) = table.get_mut(&cp.id()) {
            port.closed = true;
            port.registrations.clear();
            port.event_queue.clear();
            wake_task = port.waiter.take();
            table.remove(&cp.id());
        } else {
            wake_task = None;
        }
    }

    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run completion port self-tests.
///
/// Tests:
/// 1. Create, register, poll with ready source.
/// 2. Non-blocking wait when nothing ready.
/// 3. Notify wakes blocked waiter.
/// 4. Unregister removes source.
/// 5. Close wakes blocked waiter.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[completion] Running completion port self-test...");

    test_create_and_poll()?;
    test_try_wait_empty()?;
    test_notify_wakes_waiter()?;
    test_unregister()?;
    test_io_completion()?;

    serial_println!("[completion] Completion port self-test PASSED");
    Ok(())
}

/// Test 1: create a CP, register an eventfd, signal it, poll.
fn test_create_and_poll() -> KernelResult<()> {
    use crate::ipc::eventfd;

    let cp = create();
    let efd = eventfd::create(0);

    // Register the eventfd with user_data = 42.
    register(cp, WaitSource::EventFd(efd.raw()), 42)?;

    // Nothing ready yet.
    match try_wait(cp) {
        Err(KernelError::WouldBlock) => {} // Expected.
        other => {
            serial_println!(
                "[completion]   FAIL: try_wait on empty: {:?}",
                other
            );
            eventfd::close(efd);
            close(cp);
            return Err(KernelError::InternalError);
        }
    }

    // Signal the eventfd.
    eventfd::write(efd, 1)?;

    // Now poll should find it ready.
    let events = try_wait(cp)?;
    if events.is_empty() {
        serial_println!("[completion]   FAIL: try_wait returned empty after signal");
        eventfd::close(efd);
        close(cp);
        return Err(KernelError::InternalError);
    }

    let ev = &events[0];
    if ev.user_data != 42 {
        serial_println!(
            "[completion]   FAIL: user_data {} expected 42",
            ev.user_data
        );
        eventfd::close(efd);
        close(cp);
        return Err(KernelError::InternalError);
    }

    eventfd::close(efd);
    close(cp);
    serial_println!("[completion]   Create + poll: OK");
    Ok(())
}

/// Test 2: `try_wait` returns `WouldBlock` when nothing is registered.
fn test_try_wait_empty() -> KernelResult<()> {
    let cp = create();

    // No registrations at all.
    match try_wait(cp) {
        Err(KernelError::WouldBlock) => {}
        other => {
            serial_println!(
                "[completion]   FAIL: try_wait(empty): {:?}",
                other
            );
            close(cp);
            return Err(KernelError::InternalError);
        }
    }

    close(cp);
    serial_println!("[completion]   Try-wait empty: OK");
    Ok(())
}

/// Atomic result for the blocking wait test.
static CP_TEST_RESULT: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);

/// Task that blocks on a completion port wait.
extern "C" fn cp_waiter_task(cp_raw: u64) {
    let cp = CpHandle::from_raw(cp_raw);
    if let Ok(events) = wait(cp)
        && let Some(ev) = events.first()
    {
        CP_TEST_RESULT.store(ev.user_data, core::sync::atomic::Ordering::SeqCst);
    }
}

/// Test 3: notify wakes a blocked waiter.
fn test_notify_wakes_waiter() -> KernelResult<()> {
    use crate::ipc::eventfd;

    CP_TEST_RESULT.store(0, core::sync::atomic::Ordering::SeqCst);

    let cp = create();
    let efd = eventfd::create(0);

    // Register eventfd with user_data = 99.
    register(cp, WaitSource::EventFd(efd.raw()), 99)?;

    // Spawn a task that will block on wait().
    sched::spawn(b"cp-test", 16, cp_waiter_task, cp.raw(), 0)?;

    // Yield to let the waiter run and block.
    sched::yield_now();

    // Signal the eventfd — this should cause notify() to wake the waiter.
    // But first, we need to actually trigger the notify path.
    // Since our initial implementation polls, we need to signal the eventfd
    // so the waiter's poll loop finds it ready.
    eventfd::write(efd, 1)?;

    // The waiter is blocked.  We need to wake it so it re-polls.
    // In the poll-then-block model, the waiter registered itself in
    // port.waiter before blocking.  Calling notify() will wake it.
    notify(cp, WaitSource::EventFd(efd.raw()));

    // Yield to let the waiter process the event.
    sched::yield_now();
    sched::yield_now();

    let result = CP_TEST_RESULT.load(core::sync::atomic::Ordering::SeqCst);
    if result != 99 {
        serial_println!(
            "[completion]   FAIL: waiter got user_data {}, expected 99",
            result
        );
        eventfd::close(efd);
        close(cp);
        return Err(KernelError::InternalError);
    }

    eventfd::close(efd);
    close(cp);
    serial_println!("[completion]   Notify wakes waiter: OK");
    Ok(())
}

/// Test 4: unregister removes a source.
fn test_unregister() -> KernelResult<()> {
    use crate::ipc::eventfd;

    let cp = create();
    let efd1 = eventfd::create(0);
    let efd2 = eventfd::create(0);

    register(cp, WaitSource::EventFd(efd1.raw()), 1)?;
    register(cp, WaitSource::EventFd(efd2.raw()), 2)?;

    // Signal both.
    eventfd::write(efd1, 1)?;
    eventfd::write(efd2, 1)?;

    // Both should be ready.
    let events = try_wait(cp)?;
    if events.len() != 2 {
        serial_println!(
            "[completion]   FAIL: expected 2 events, got {}",
            events.len()
        );
        eventfd::close(efd1);
        eventfd::close(efd2);
        close(cp);
        return Err(KernelError::InternalError);
    }

    // Consume efd1 so it's no longer ready.
    let _ = eventfd::try_read(efd1);

    // Unregister efd1.
    unregister(cp, WaitSource::EventFd(efd1.raw()))?;

    // Re-signal efd2 (it was consumed by read above... no, try_wait
    // doesn't consume.  efd2 still has value=1 since we didn't read it.)
    // Actually, try_wait polls but doesn't consume.  efd2 still ready.
    let events = try_wait(cp)?;
    if events.len() != 1 {
        serial_println!(
            "[completion]   FAIL: after unregister expected 1 event, got {}",
            events.len()
        );
        eventfd::close(efd1);
        eventfd::close(efd2);
        close(cp);
        return Err(KernelError::InternalError);
    }

    if events[0].user_data != 2 {
        serial_println!(
            "[completion]   FAIL: expected user_data 2, got {}",
            events[0].user_data
        );
        eventfd::close(efd1);
        eventfd::close(efd2);
        close(cp);
        return Err(KernelError::InternalError);
    }

    // Unregister non-existent source should fail.
    match unregister(cp, WaitSource::EventFd(efd1.raw())) {
        Err(KernelError::NotFound) => {} // Expected.
        other => {
            serial_println!(
                "[completion]   FAIL: unregister removed source: {:?}",
                other
            );
            eventfd::close(efd1);
            eventfd::close(efd2);
            close(cp);
            return Err(KernelError::InternalError);
        }
    }

    eventfd::close(efd1);
    eventfd::close(efd2);
    close(cp);
    serial_println!("[completion]   Unregister: OK");
    Ok(())
}

/// Test 5: io_ring integrated with completion port.
///
/// Create an io_ring, register it with a CP, submit NOP SQEs,
/// and verify the CP fires.
fn test_io_completion() -> KernelResult<()> {
    use super::io_ring;
    use core::mem::size_of;

    let cp = create();
    let (ring_handle, base_virt, _frames) = io_ring::setup(8, 16)?;

    // Register the io_ring with user_data = 77.
    register(cp, WaitSource::IoCompletion(ring_handle), 77)?;

    // No CQEs yet — CP should not fire.
    match try_wait(cp) {
        Err(KernelError::WouldBlock) => {} // Expected.
        other => {
            serial_println!(
                "[completion]   FAIL: io_ring try_wait before submit: {:?}",
                other
            );
            io_ring::destroy(ring_handle)?;
            close(cp);
            return Err(KernelError::InternalError);
        }
    }

    // Submit 2 NOP SQEs.
    #[allow(clippy::arithmetic_side_effects)]
    let sq_base = (base_virt + size_of::<io_ring::IoRingHeader>() as u64)
        as *mut io_ring::SqEntry;

    for i in 0u32..2 {
        let sqe = io_ring::SqEntry {
            opcode: io_ring::IO_OP_NOP,
            flags: 0,
            _pad0: [0; 2],
            _pad1: 0,
            user_data: u64::from(i).wrapping_add(200),
            handle: 0,
            addr: 0,
            len: 0,
            _pad2: 0,
            arg1: 0,
            arg2: 0,
        };
        // SAFETY: sq_base points to valid SQ array, i < sq_entries.
        unsafe {
            *sq_base.add(i as usize) = sqe;
        }
    }

    // Advance SQ tail.
    // SAFETY: base_virt is a valid mapped page from setup_ring; header is at offset 0.
    let header = unsafe { &mut *(base_virt as *mut io_ring::IoRingHeader) };
    header.sq_tail.store(2, core::sync::atomic::Ordering::Release);

    // Process — this should post CQEs and notify our CP.
    let processed = io_ring::enter(ring_handle, 0)?;
    if processed != 2 {
        serial_println!(
            "[completion]   FAIL: io_ring processed {} SQEs, expected 2",
            processed
        );
        io_ring::destroy(ring_handle)?;
        close(cp);
        return Err(KernelError::InternalError);
    }

    // CP should now have an event (io_ring has pending CQEs).
    let events = try_wait(cp)?;
    if events.is_empty() {
        serial_println!(
            "[completion]   FAIL: CP try_wait empty after io_ring submit"
        );
        io_ring::destroy(ring_handle)?;
        close(cp);
        return Err(KernelError::InternalError);
    }

    let ev = &events[0];
    if ev.user_data != 77 {
        serial_println!(
            "[completion]   FAIL: io_ring event user_data {}, expected 77",
            ev.user_data
        );
        io_ring::destroy(ring_handle)?;
        close(cp);
        return Err(KernelError::InternalError);
    }

    // Verify the source is IoCompletion.
    match ev.source {
        WaitSource::IoCompletion(h) if h == ring_handle => {}
        _ => {
            serial_println!(
                "[completion]   FAIL: event source not IoCompletion"
            );
            io_ring::destroy(ring_handle)?;
            close(cp);
            return Err(KernelError::InternalError);
        }
    }

    // Unregister and clean up.
    unregister(cp, WaitSource::IoCompletion(ring_handle))?;
    io_ring::destroy(ring_handle)?;
    close(cp);
    serial_println!("[completion]   IO completion: OK");
    Ok(())
}
