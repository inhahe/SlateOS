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

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::collections::VecDeque;
use alloc::string::String;
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

/// Maximum socket activation entries.
const MAX_SOCKET_ACTIVATIONS: usize = 64;

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

    /// Namespace in which this service was registered.
    ///
    /// Services in the root namespace (0) are visible to all processes.
    /// Services in other namespaces are only visible to processes in the
    /// same namespace.  This prevents a sandboxed process from connecting
    /// to services outside its isolation boundary.
    namespace_id: u64,

    /// PID of the process that registered (provides) this service.
    ///
    /// `0` when the service was registered from a kernel task (kernel is the
    /// TCB).  Used to attribute a service to its backing process — e.g. the
    /// SHM-region authorization path grants the `net.stack` daemon the right
    /// to `SYS_SHM_MAP` the kernel-created ring regions handed to it (see
    /// `ipc/shm.rs::authorize` and `syscall/handlers.rs::sys_shm_map`).
    provider_pid: u64,
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
// Socket activation
// ---------------------------------------------------------------------------

/// Status of a socket-activated service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationStatus {
    /// Service is registered but hasn't been triggered yet.
    Idle,
    /// Service is being started (spawn triggered, waiting for register).
    Starting,
    /// Service has registered and is running.
    Running,
    /// Service failed to start or crashed.
    Failed,
}

/// A socket activation entry.
///
/// Describes a service that should be started on-demand when a client
/// first connects to its name.
struct SocketActivationEntry {
    /// The service name (must match what the service will `register` with).
    name: Vec<u8>,
    /// Path to the ELF binary or kernel task name to spawn.
    spawn_path: String,
    /// Current status.
    status: ActivationStatus,
    /// Number of times the service has been started.
    start_count: u32,
    /// Connections queued before the service registered.
    ///
    /// When the service calls `register()`, these are transferred to its
    /// pending queue.
    pre_queue: VecDeque<ChannelHandle>,
}

/// Socket activation registry.
static SOCKET_ACTIVATIONS: Mutex<Vec<SocketActivationEntry>> = Mutex::new(Vec::new());

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

    // Determine the registering process's namespace and PID.
    // Services registered from the root namespace (0) are globally visible.
    let provider_pid = {
        let task_id = sched::current_task_id();
        crate::proc::thread::owner_process(task_id).unwrap_or(0)
    };
    let ns_id = if provider_pid != 0 {
        super::namespace::query(provider_pid)
    } else {
        0 // Kernel tasks register in root namespace.
    };

    // Check for pre-queued connections from socket activation.
    let pre_queued = drain_pre_queue(name);

    let entry = ServiceEntry {
        name: name.to_vec(),
        pending: pre_queued,
        accept_waiter: None,
        closed: false,
        namespace_id: ns_id,
        provider_pid,
    };

    reg.listeners.insert(id, entry);
    reg.names.insert(name.to_vec(), id);

    // Mark socket activation as running (if applicable).
    mark_activation_running(name);

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

        let listener_id_opt = reg.names.get(name).copied();

        match listener_id_opt {
            Some(listener_id) => {
                // Service is registered — standard connect path.
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

                // Namespace isolation: a process can only connect to
                // services in its own namespace or the root namespace.
                // Root namespace (0) services are always visible.
                if entry.namespace_id != 0 {
                    let client_ns = {
                        let task_id = sched::current_task_id();
                        match crate::proc::thread::owner_process(task_id) {
                            Some(pid) if pid != 0 => super::namespace::query(pid),
                            _ => 0, // Kernel → root namespace.
                        }
                    };
                    if client_ns != entry.namespace_id {
                        channel::close(client_ep);
                        channel::close(server_ep);
                        return Err(KernelError::NotFound);
                    }
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
            None => {
                // Service not registered — check socket activation.
                drop(reg); // Release registry lock before acquiring activations lock.

                let mut activated = false;
                {
                    let mut activations = SOCKET_ACTIVATIONS.lock();
                    if let Some(entry) = activations.iter_mut()
                        .find(|e| e.name == name)
                    {
                        // Queue the connection for when the service registers.
                        if entry.pre_queue.len() >= MAX_PENDING_CONNECTIONS {
                            channel::close(client_ep);
                            channel::close(server_ep);
                            return Err(KernelError::ResourceExhausted);
                        }
                        entry.pre_queue.push_back(server_ep);

                        // Trigger service start if idle.
                        if entry.status == ActivationStatus::Idle {
                            entry.status = ActivationStatus::Starting;
                            entry.start_count = entry.start_count.saturating_add(1);
                            let path = entry.spawn_path.clone();
                            activated = true;

                            // Drop lock before spawn to avoid deadlock.
                            drop(activations);
                            trigger_service_spawn(&path, name);
                        }
                    } else {
                        // No registration and no socket activation → NotFound.
                        channel::close(client_ep);
                        channel::close(server_ep);
                        return Err(KernelError::NotFound);
                    }
                }
                let _ = activated; // Suppress unused warning.

                // Connection is queued (either in pre_queue or registry).
                return Ok(client_ep);
            }
        }
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

/// Look up the PID of the process that provides a registered service.
///
/// Returns `Some(pid)` for a live (non-closed) service registered by a
/// userspace process, `Some(0)` for a kernel-registered service, or `None`
/// if no such service is registered.  Used to authorize a trusted daemon for
/// resources the kernel hands it (e.g. SHM ring regions — see
/// `ipc/shm.rs::authorize`).  This lookup ignores namespace visibility on
/// purpose: it is a kernel-internal trust query, not a client-facing connect.
#[must_use]
pub fn provider_pid(name: &[u8]) -> Option<u64> {
    let reg = SERVICE_REGISTRY.lock();
    let id = reg.names.get(name).copied()?;
    let entry = reg.listeners.get(&id)?;
    if entry.closed {
        return None;
    }
    Some(entry.provider_pid)
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
// Socket activation API
// ---------------------------------------------------------------------------

/// Register a service for socket activation (on-demand start).
///
/// When a client calls `connect()` for this name and no service is registered,
/// the kernel will:
/// 1. Queue the client connection.
/// 2. Spawn the service using the given path/name.
/// 3. When the service calls `register()`, it inherits the queued connections.
///
/// # Parameters
///
/// - `name`: Service name (what clients connect to).
/// - `spawn_path`: ELF binary path or kernel task name to spawn.
///
/// # Errors
///
/// - [`InvalidArgument`] — name or path is empty/too long.
/// - [`AlreadyExists`] — activation already registered for this name.
/// - [`ResourceExhausted`] — too many activations.
///
/// [`InvalidArgument`]: KernelError::InvalidArgument
/// [`AlreadyExists`]: KernelError::AlreadyExists
/// [`ResourceExhausted`]: KernelError::ResourceExhausted
pub fn register_socket_activation(name: &[u8], spawn_path: &str) -> KernelResult<()> {
    if name.is_empty() || name.len() > MAX_NAME_LEN || spawn_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let mut activations = SOCKET_ACTIVATIONS.lock();

    if activations.len() >= MAX_SOCKET_ACTIVATIONS {
        return Err(KernelError::ResourceExhausted);
    }

    // Check for duplicate.
    if activations.iter().any(|e| e.name == name) {
        return Err(KernelError::AlreadyExists);
    }

    activations.push(SocketActivationEntry {
        name: name.to_vec(),
        spawn_path: String::from(spawn_path),
        status: ActivationStatus::Idle,
        start_count: 0,
        pre_queue: VecDeque::new(),
    });

    serial_println!("[service] Socket activation registered: {:?} → {}",
        core::str::from_utf8(name).unwrap_or("<bin>"), spawn_path);
    Ok(())
}

/// Remove a socket activation entry.
///
/// Any pre-queued connections are closed.
pub fn unregister_socket_activation(name: &[u8]) -> KernelResult<()> {
    let mut activations = SOCKET_ACTIVATIONS.lock();

    if let Some(pos) = activations.iter().position(|e| e.name == name) {
        let entry = activations.remove(pos);
        // Close any pre-queued connections.
        for handle in entry.pre_queue {
            channel::close(handle);
        }
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// List all socket activation entries (for monitoring/kshell).
///
/// Returns (name, spawn_path, status, start_count, pre_queue_len).
pub fn list_socket_activations() -> Vec<(String, String, ActivationStatus, u32, usize)> {
    let activations = SOCKET_ACTIVATIONS.lock();
    activations.iter().map(|e| {
        let name = String::from(core::str::from_utf8(&e.name).unwrap_or("<bin>"));
        (name, e.spawn_path.clone(), e.status, e.start_count, e.pre_queue.len())
    }).collect()
}

/// Mark a socket-activated service as failed (e.g., after crash).
///
/// Called by the process supervisor when a socket-activated service exits
/// abnormally. Resets status to Idle so the next connect triggers a restart.
pub fn mark_activation_failed(name: &[u8]) {
    let mut activations = SOCKET_ACTIVATIONS.lock();
    if let Some(entry) = activations.iter_mut().find(|e| e.name == name) {
        entry.status = ActivationStatus::Failed;
        // Close pre-queued connections — they'll get errors.
        while let Some(handle) = entry.pre_queue.pop_front() {
            channel::close(handle);
        }
    }
}

/// Reset a failed socket activation back to idle (allows retry).
pub fn reset_activation(name: &[u8]) -> KernelResult<()> {
    let mut activations = SOCKET_ACTIVATIONS.lock();
    if let Some(entry) = activations.iter_mut().find(|e| e.name == name) {
        entry.status = ActivationStatus::Idle;
        Ok(())
    } else {
        Err(KernelError::NotFound)
    }
}

/// Check if a service name has socket activation configured.
pub fn is_socket_activated(name: &[u8]) -> bool {
    let activations = SOCKET_ACTIVATIONS.lock();
    activations.iter().any(|e| e.name == name)
}

// ---------------------------------------------------------------------------
// Socket activation helpers (internal)
// ---------------------------------------------------------------------------

/// Drain pre-queued connections for a name from the activation registry.
///
/// Called when a service registers — transfers queued connections to its
/// pending queue.
fn drain_pre_queue(name: &[u8]) -> VecDeque<ChannelHandle> {
    let mut activations = SOCKET_ACTIVATIONS.lock();
    if let Some(entry) = activations.iter_mut().find(|e| e.name == name) {
        
        core::mem::take(&mut entry.pre_queue)
    } else {
        VecDeque::new()
    }
}

/// Mark a socket activation entry as running.
fn mark_activation_running(name: &[u8]) {
    let mut activations = SOCKET_ACTIVATIONS.lock();
    if let Some(entry) = activations.iter_mut().find(|e| e.name == name) {
        entry.status = ActivationStatus::Running;
    }
}

/// Trigger spawning of a socket-activated service.
///
/// Currently logs the spawn request. In the future, this will invoke the
/// process spawner to start the service binary.
fn trigger_service_spawn(path: &str, name: &[u8]) {
    let name_str = core::str::from_utf8(name).unwrap_or("<bin>");
    serial_println!("[service] Socket activation: spawning '{}' for service '{}'",
        path, name_str);

    // In the future, this calls proc::spawn::spawn_process(path, ...) or
    // similar. For now we just log it — actual spawning requires the process
    // subsystem to support on-demand launching from service names.
    //
    // The service will eventually call register(name) which transfers the
    // pre-queued connections.
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
    test_socket_activation()?;

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

/// Test 6: socket activation — pre-queue connection, then register.
fn test_socket_activation() -> KernelResult<()> {
    let name = b"test.socket_activated";

    // Register socket activation.
    register_socket_activation(name, "/sbin/test-service")?;

    // Verify it's activated.
    if !is_socket_activated(name) {
        serial_println!("[service]   FAIL: socket activation not registered");
        unregister_socket_activation(name).ok();
        return Err(KernelError::InternalError);
    }

    // Client connects — service not running yet.
    // This should queue the connection in pre_queue and trigger spawn.
    let client_ep = connect(name)?;

    // Verify the connection was pre-queued (activation should be Starting).
    {
        let activations = SOCKET_ACTIVATIONS.lock();
        let entry = activations.iter().find(|e| e.name == name)
            .ok_or(KernelError::InternalError)?;
        if entry.status != ActivationStatus::Starting {
            serial_println!("[service]   FAIL: expected Starting, got {:?}", entry.status);
            channel::close(client_ep);
            unregister_socket_activation(name).ok();
            return Err(KernelError::InternalError);
        }
        if entry.pre_queue.len() != 1 {
            serial_println!("[service]   FAIL: expected 1 pre-queued, got {}", entry.pre_queue.len());
            channel::close(client_ep);
            unregister_socket_activation(name).ok();
            return Err(KernelError::InternalError);
        }
    }

    // Simulate the service starting: it calls register().
    // This should transfer the pre-queued connection.
    let listener = register(name)?;

    // The service should now have 1 pending connection.
    let count = pending_count(listener)?;
    if count != 1 {
        serial_println!("[service]   FAIL: expected 1 pending, got {}", count);
        channel::close(client_ep);
        unregister(listener).ok();
        unregister_socket_activation(name).ok();
        return Err(KernelError::InternalError);
    }

    // Accept the connection and verify it works.
    let server_ep = try_accept(listener)?
        .ok_or(KernelError::InternalError)?;

    // Send a message through the channel.
    let msg = channel::Message::from_bytes(b"socket activated!")?;
    channel::send(client_ep, msg)?;

    let received = channel::recv(server_ep)?;
    if received.data() != b"socket activated!" {
        serial_println!("[service]   FAIL: message mismatch");
        channel::close(client_ep);
        channel::close(server_ep);
        unregister(listener).ok();
        unregister_socket_activation(name).ok();
        return Err(KernelError::InternalError);
    }

    // Cleanup.
    channel::close(client_ep);
    channel::close(server_ep);
    unregister(listener)?;
    unregister_socket_activation(name)?;

    serial_println!("[service]   Socket activation: OK");
    Ok(())
}
