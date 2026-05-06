//! Service Registry — named service discovery and connection brokering.
//!
//! The service registry is the primary mechanism for processes to find
//! and connect to system services in this microkernel.  It replaces
//! the role of D-Bus/COM/Mach ports for service discovery.
//!
//! ## Architecture
//!
//! 1. A service calls `register("display.compositor")` → receives a
//!    *listener handle* (opaque ID for accepting connections).
//! 2. A client calls `connect("display.compositor")` → the kernel
//!    creates a fresh channel pair, queues the server endpoint in the
//!    service's pending-connections queue, and returns the client
//!    endpoint immediately.
//! 3. The service calls `accept(listener)` → dequeues and returns the
//!    server-side channel endpoint.  The service and client now have a
//!    dedicated private channel.
//!
//! ## Design Rationale
//!
//! - **Kernel-mediated**: unlike D-Bus which is a userspace daemon,
//!   the registry lives in kernel space.  This avoids an extra IPC hop
//!   for every connection and makes it available from the earliest boot.
//! - **Channel-based**: each accepted connection IS a standard channel.
//!   No new IPC primitive — services use the same recv/send they already
//!   know.
//! - **Name-based**: services are discovered by a hierarchical name
//!   (e.g., "fs.vfs", "net.dns", "display.compositor").  Names are
//!   opaque byte sequences (no forced UTF-8).
//! - **Capability-gated** (future): registering a name will require a
//!   ServiceRegister capability to prevent name squatting.
//!
//! ## Lock Ordering
//!
//! `SERVICE_REGISTRY` → `SCHED` (accept may call sched::wake/block).
//!
//! ## Performance
//!
//! Connect latency: one channel_create (~100-200ns) + one queue push +
//! one wake.  Total < 1 µs for the fast path.

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::ipc::channel::{self, ChannelHandle};
use crate::sched::{self, task::TaskId};
use crate::serial_println;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum length of a service name (bytes).
const MAX_NAME_LEN: usize = 256;

/// Maximum number of pending connections queued before connect fails.
const MAX_PENDING_CONNECTIONS: usize = 128;

/// Maximum number of registered services.
const MAX_SERVICES: usize = 1024;

// ---------------------------------------------------------------------------
// Handle
// ---------------------------------------------------------------------------

/// Unique ID for a service listener.
type ListenerId = u64;

/// Counter for generating unique listener IDs.
static NEXT_LISTENER_ID: AtomicU64 = AtomicU64::new(1);

fn alloc_listener_id() -> ListenerId {
    NEXT_LISTENER_ID.fetch_add(1, Ordering::Relaxed)
}

/// Handle to a service listener (returned by `register`).
///
/// The service uses this handle to `accept` incoming connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ServiceListenerHandle(u64);

impl ServiceListenerHandle {
    /// Reconstruct from raw u64.
    #[must_use]
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Get the raw u64 representation.
    #[must_use]
    pub const fn raw(self) -> u64 {
        self.0
    }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// A registered service entry.
struct ServiceEntry {
    /// The service name (opaque bytes).
    name: Vec<u8>,

    /// Queue of pending connection endpoints waiting for `accept`.
    ///
    /// Each entry is a server-side channel endpoint that the service
    /// hasn't picked up yet.
    pending: VecDeque<ChannelHandle>,

    /// Task blocked on `accept` (if any).
    accept_waiter: Option<TaskId>,

    /// Whether the service has been unregistered (closed).
    closed: bool,
}

/// Global service registry.
///
/// Lock ordering: `SERVICE_REGISTRY` → `SCHED`.
static SERVICE_REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

/// Registry internal state.
struct Registry {
    /// Listener ID → service entry.
    listeners: BTreeMap<ListenerId, ServiceEntry>,

    /// Name → listener ID (for name-based lookups).
    names: BTreeMap<Vec<u8>, ListenerId>,
}

impl Registry {
    const fn new() -> Self {
        Self {
            listeners: BTreeMap::new(),
            names: BTreeMap::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a named service.
///
/// Creates a listener and associates it with the given name.  The caller
/// (service process) uses the returned handle to `accept` connections.
///
/// # Errors
///
/// - [`InvalidArgument`] — name is empty or too long.
/// - [`AlreadyExists`] — a service with this name is already registered.
/// - [`ResourceExhausted`] — too many services registered.
///
/// [`InvalidArgument`]: KernelError::InvalidArgument
/// [`AlreadyExists`]: KernelError::AlreadyExists
/// [`ResourceExhausted`]: KernelError::ResourceExhausted
pub fn register(name: &[u8]) -> KernelResult<ServiceListenerHandle> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }

    let id = alloc_listener_id();

    let mut reg = SERVICE_REGISTRY.lock();

    if reg.listeners.len() >= MAX_SERVICES {
        return Err(KernelError::ResourceExhausted);
    }

    // Check for duplicate name.
    if reg.names.contains_key(name) {
        return Err(KernelError::AlreadyExists);
    }

    let entry = ServiceEntry {
        name: name.to_vec(),
        pending: VecDeque::new(),
        accept_waiter: None,
        closed: false,
    };

    reg.listeners.insert(id, entry);
    reg.names.insert(name.to_vec(), id);

    Ok(ServiceListenerHandle(id))
}

/// Connect to a named service.
///
/// Creates a new channel pair.  The server-side endpoint is queued for
/// the service to `accept`.  The client-side endpoint is returned to
/// the caller immediately.
///
/// # Errors
///
/// - [`NotFound`] — no service registered with this name.
/// - [`InvalidArgument`] — name is empty or too long.
/// - [`ResourceExhausted`] — the service's pending queue is full.
///
/// [`NotFound`]: KernelError::NotFound
/// [`InvalidArgument`]: KernelError::InvalidArgument
/// [`ResourceExhausted`]: KernelError::ResourceExhausted
pub fn connect(name: &[u8]) -> KernelResult<ChannelHandle> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(KernelError::InvalidArgument);
    }

    // Create a fresh channel pair: client_ep ↔ server_ep.
    let (client_ep, server_ep) = channel::create();

    let wake_task: Option<TaskId>;

    {
        let mut reg = SERVICE_REGISTRY.lock();

        let listener_id = reg.names.get(name)
            .copied()
            .ok_or_else(|| {
                // Clean up the channel we just created.
                channel::close(client_ep);
                channel::close(server_ep);
                KernelError::NotFound
            })?;

        let entry = reg.listeners.get_mut(&listener_id)
            .ok_or_else(|| {
                channel::close(client_ep);
                channel::close(server_ep);
                KernelError::NotFound
            })?;

        if entry.closed {
            channel::close(client_ep);
            channel::close(server_ep);
            return Err(KernelError::NotFound);
        }

        if entry.pending.len() >= MAX_PENDING_CONNECTIONS {
            channel::close(client_ep);
            channel::close(server_ep);
            return Err(KernelError::ResourceExhausted);
        }

        // Queue the server endpoint for the service to accept.
        entry.pending.push_back(server_ep);

        // Wake the service if it's blocked on accept.
        wake_task = entry.accept_waiter.take();
    }

    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }

    Ok(client_ep)
}

/// Accept a pending connection (blocking).
///
/// Returns the server-side channel endpoint for the next pending
/// connection.  Blocks if no connections are waiting.
///
/// # Errors
///
/// - [`InvalidHandle`] — listener handle not found.
/// - [`ChannelClosed`] — listener was unregistered while waiting.
///
/// [`InvalidHandle`]: KernelError::InvalidHandle
/// [`ChannelClosed`]: KernelError::ChannelClosed
pub fn accept(listener: ServiceListenerHandle) -> KernelResult<ChannelHandle> {
    loop {
        {
            let mut reg = SERVICE_REGISTRY.lock();
            let entry = reg.listeners.get_mut(&listener.0)
                .ok_or(KernelError::InvalidHandle)?;

            if let Some(handle) = entry.pending.pop_front() {
                return Ok(handle);
            }

            if entry.closed {
                return Err(KernelError::ChannelClosed);
            }

            // No pending connections — block.
            entry.accept_waiter = Some(sched::current_task_id());
        }

        sched::block_current();
    }
}

/// Accept a pending connection (non-blocking).
///
/// Returns `Ok(Some(handle))` if a connection was waiting, `Ok(None)`
/// if no connections are pending.
///
/// # Errors
///
/// - [`InvalidHandle`] — listener handle not found.
/// - [`ChannelClosed`] — listener was unregistered.
pub fn try_accept(listener: ServiceListenerHandle) -> KernelResult<Option<ChannelHandle>> {
    let mut reg = SERVICE_REGISTRY.lock();
    let entry = reg.listeners.get_mut(&listener.0)
        .ok_or(KernelError::InvalidHandle)?;

    if let Some(handle) = entry.pending.pop_front() {
        return Ok(Some(handle));
    }

    if entry.closed {
        return Err(KernelError::ChannelClosed);
    }

    Ok(None)
}

/// Accept a pending connection with a timeout (nanoseconds).
///
/// Blocks up to `timeout_ns` nanoseconds waiting for a connection.
/// Returns `Err(TimedOut)` if the deadline expires.
///
/// `timeout_ns = 0` is equivalent to `try_accept` (returns `TimedOut`
/// instead of `Ok(None)` when no connection is pending).
pub fn accept_timeout(
    listener: ServiceListenerHandle,
    timeout_ns: u64,
) -> KernelResult<ChannelHandle> {
    // Fast path.
    {
        let mut reg = SERVICE_REGISTRY.lock();
        let entry = reg.listeners.get_mut(&listener.0)
            .ok_or(KernelError::InvalidHandle)?;

        if let Some(handle) = entry.pending.pop_front() {
            return Ok(handle);
        }

        if entry.closed {
            return Err(KernelError::ChannelClosed);
        }
    }

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

    let timer_handle = crate::hrtimer::schedule_ns(
        timeout_ns,
        timeout_wake,
        sched::current_task_id(),
    );

    loop {
        {
            let mut reg = SERVICE_REGISTRY.lock();
            let entry = reg.listeners.get_mut(&listener.0)
                .ok_or_else(|| {
                    crate::hrtimer::cancel(timer_handle);
                    KernelError::InvalidHandle
                })?;

            if let Some(handle) = entry.pending.pop_front() {
                crate::hrtimer::cancel(timer_handle);
                return Ok(handle);
            }

            if entry.closed {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::ChannelClosed);
            }

            if crate::hrtimer::now_ns() >= deadline_ns {
                crate::hrtimer::cancel(timer_handle);
                return Err(KernelError::TimedOut);
            }

            entry.accept_waiter = Some(sched::current_task_id());
        }

        sched::block_current();
    }
}

/// Unregister a service and close its listener.
///
/// All pending (unaccepted) connections are closed.  If the service
/// is blocked on `accept`, it is woken with `ChannelClosed`.
///
/// # Errors
///
/// - [`InvalidHandle`] — listener handle not found.
pub fn unregister(listener: ServiceListenerHandle) -> KernelResult<()> {
    let wake_task: Option<TaskId>;
    let pending_handles: VecDeque<ChannelHandle>;

    {
        let mut reg = SERVICE_REGISTRY.lock();
        let entry = reg.listeners.get_mut(&listener.0)
            .ok_or(KernelError::InvalidHandle)?;

        entry.closed = true;
        wake_task = entry.accept_waiter.take();

        // Drain pending connections — we'll close them outside the lock.
        pending_handles = core::mem::take(&mut entry.pending);

        // Clone name before removing (avoids borrow conflict).
        let name = entry.name.clone();

        // Remove from name index.
        reg.names.remove(&name);

        // Remove the listener entry.
        reg.listeners.remove(&listener.0);
    }

    // Wake blocked acceptor.
    if let Some(task_id) = wake_task {
        sched::wake(task_id);
    }

    // Close all unaccepted connection endpoints.
    for handle in pending_handles {
        channel::close(handle);
    }

    Ok(())
}

/// List all registered service names.
///
/// Returns a vector of service name byte slices (cloned).
pub fn list_services() -> Vec<Vec<u8>> {
    let reg = SERVICE_REGISTRY.lock();
    reg.names.keys().cloned().collect()
}

/// Check if a service with the given name is registered.
pub fn is_registered(name: &[u8]) -> bool {
    let reg = SERVICE_REGISTRY.lock();
    reg.names.contains_key(name)
}

/// Get the number of pending (unaccepted) connections for a listener.
///
/// Useful for monitoring/diagnostics.
pub fn pending_count(listener: ServiceListenerHandle) -> KernelResult<usize> {
    let reg = SERVICE_REGISTRY.lock();
    let entry = reg.listeners.get(&listener.0)
        .ok_or(KernelError::InvalidHandle)?;
    Ok(entry.pending.len())
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run service registry self-tests.
///
/// Tests:
/// 1. Register and discover a service.
/// 2. Connect creates a working channel.
/// 3. Multiple connections (each gets own channel).
/// 4. Unregister closes pending connections.
/// 5. Duplicate name rejected.
/// 6. Blocking accept via spawned task.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[service] Running service registry self-test...");

    test_register_connect()?;
    test_multiple_connections()?;
    test_unregister()?;
    test_duplicate_name()?;
    test_blocking_accept()?;

    serial_println!("[service] Service registry self-test PASSED");
    Ok(())
}

/// Test 1: register, connect, accept — verify channel works.
fn test_register_connect() -> KernelResult<()> {
    let listener = register(b"test.basic")?;

    // Client connects.
    let client_ep = connect(b"test.basic")?;

    // Service accepts.
    let server_ep = try_accept(listener)?
        .ok_or(KernelError::InternalError)?;

    // Verify the channel works: client sends, server receives.
    let msg = channel::Message::from_bytes(b"hello service")?;
    channel::send(client_ep, msg)?;

    let received = channel::recv(server_ep)?;
    if received.data() != b"hello service" {
        serial_println!("[service]   FAIL: message mismatch");
        channel::close(client_ep);
        channel::close(server_ep);
        unregister(listener)?;
        return Err(KernelError::InternalError);
    }

    // Cleanup.
    channel::close(client_ep);
    channel::close(server_ep);
    unregister(listener)?;

    serial_println!("[service]   Register/connect/accept: OK");
    Ok(())
}

/// Test 2: multiple clients connect to the same service.
fn test_multiple_connections() -> KernelResult<()> {
    let listener = register(b"test.multi")?;

    // Three clients connect.
    let c1 = connect(b"test.multi")?;
    let c2 = connect(b"test.multi")?;
    let c3 = connect(b"test.multi")?;

    // Service accepts all three.
    let s1 = try_accept(listener)?.ok_or(KernelError::InternalError)?;
    let s2 = try_accept(listener)?.ok_or(KernelError::InternalError)?;
    let s3 = try_accept(listener)?.ok_or(KernelError::InternalError)?;

    // Verify each pair is independent.
    channel::send(c1, channel::Message::from_bytes(b"from c1")?)?;
    channel::send(c2, channel::Message::from_bytes(b"from c2")?)?;
    channel::send(c3, channel::Message::from_bytes(b"from c3")?)?;

    let r1 = channel::recv(s1)?;
    let r2 = channel::recv(s2)?;
    let r3 = channel::recv(s3)?;

    if r1.data() != b"from c1" || r2.data() != b"from c2" || r3.data() != b"from c3" {
        serial_println!("[service]   FAIL: channel mismatch in multi-connect");
        // Cleanup omitted for brevity in error path.
        return Err(KernelError::InternalError);
    }

    // Cleanup.
    channel::close(c1);
    channel::close(c2);
    channel::close(c3);
    channel::close(s1);
    channel::close(s2);
    channel::close(s3);
    unregister(listener)?;

    serial_println!("[service]   Multiple connections: OK");
    Ok(())
}

/// Test 3: unregister closes pending connections.
fn test_unregister() -> KernelResult<()> {
    let listener = register(b"test.unreg")?;

    // Client connects but service doesn't accept.
    let client_ep = connect(b"test.unreg")?;

    // Unregister — pending server_ep should be closed.
    unregister(listener)?;

    // Client should eventually see ChannelClosed when trying to use it.
    // (The server endpoint was closed by unregister, so the client's
    // peer is closed.)
    let msg = channel::Message::from_bytes(b"test")?;
    match channel::send(client_ep, msg) {
        Err(KernelError::ChannelClosed) => {}
        other => {
            serial_println!("[service]   FAIL: expected ChannelClosed, got {:?}", other);
            channel::close(client_ep);
            return Err(KernelError::InternalError);
        }
    }

    channel::close(client_ep);

    // Connecting to unregistered name should fail.
    match connect(b"test.unreg") {
        Err(KernelError::NotFound) => {}
        other => {
            serial_println!("[service]   FAIL: expected NotFound, got {:?}", other);
            return Err(KernelError::InternalError);
        }
    }

    serial_println!("[service]   Unregister: OK");
    Ok(())
}

/// Test 4: duplicate name rejected.
fn test_duplicate_name() -> KernelResult<()> {
    let listener = register(b"test.dup")?;

    match register(b"test.dup") {
        Err(KernelError::AlreadyExists) => {}
        other => {
            serial_println!("[service]   FAIL: expected AlreadyExists, got {:?}", other);
            unregister(listener)?;
            return Err(KernelError::InternalError);
        }
    }

    unregister(listener)?;
    serial_println!("[service]   Duplicate name rejection: OK");
    Ok(())
}

/// Atomic result for blocking accept test.
static ACCEPT_TEST_RESULT: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0);

/// Task that accepts a connection and reads a message.
extern "C" fn accept_task(listener_raw: u64) {
    let listener = ServiceListenerHandle::from_raw(listener_raw);
    if let Ok(server_ep) = accept(listener) {
        if let Ok(msg) = channel::recv(server_ep) {
            if msg.data() == b"async connect" {
                ACCEPT_TEST_RESULT.store(1, Ordering::SeqCst);
            }
        }
        channel::close(server_ep);
    }
}

/// Test 5: blocking accept via spawned task.
fn test_blocking_accept() -> KernelResult<()> {
    ACCEPT_TEST_RESULT.store(0, Ordering::SeqCst);

    let listener = register(b"test.block")?;

    // Spawn a task that will block on accept.
    sched::spawn(b"svc-accept", 16, accept_task, listener.raw(), 0)?;

    // Yield to let accept task run and block.
    sched::yield_now();

    // Now connect — this should wake the accept task.
    let client_ep = connect(b"test.block")?;

    // Send a message on the client endpoint.
    let msg = channel::Message::from_bytes(b"async connect")?;
    channel::send(client_ep, msg)?;

    // Yield to let the accept task process.
    sched::yield_now();
    sched::yield_now();

    let result = ACCEPT_TEST_RESULT.load(Ordering::SeqCst);
    if result != 1 {
        serial_println!("[service]   FAIL: blocking accept result={}", result);
        channel::close(client_ep);
        unregister(listener)?;
        return Err(KernelError::InternalError);
    }

    channel::close(client_ep);
    unregister(listener)?;

    serial_println!("[service]   Blocking accept: OK");
    Ok(())
}
