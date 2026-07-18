//! Socket activation — on-demand service startup triggered by network/IPC sockets.
//!
//! When a client connects to a socket whose service is not running, the kernel
//! holds the connection and starts the corresponding service.  Once the service
//! signals ready (via [`crate::svcstart::signal_ready`]), the held connections
//! are passed to the service through its file descriptor table.
//!
//! ## Architecture
//!
//! ```text
//! Socket activation flow
//!   1. sockact::register(service_id, SocketSpec { ... })
//!      → associates a listening socket with a service
//!   2. Client connects to socket while service is stopped
//!      → connection queued, service start triggered
//!   3. Service starts, calls sockact::claim(service_id)
//!      → queued connections handed to service
//!
//! Socket types supported:
//!   - TCP (port-based activation)
//!   - Unix domain socket (path-based activation)
//!   - IPC channel (capability-based activation)
//! ```
//!
//! ## Design reference
//!
//! - systemd socket activation (sd_listen_fds, sd_notify)
//! - launchd on-demand sockets
//! - inetd/xinetd (classic Unix)
//!
//! This module manages the socket→service mapping and activation state.
//! Actual socket I/O is handled by the IPC/network subsystem; this module
//! provides the policy layer.

#![allow(dead_code)]

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Type of socket used for activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    /// TCP socket on a given port.
    Tcp,
    /// UDP socket on a given port.
    Udp,
    /// Unix domain socket at a filesystem path.
    Unix,
    /// IPC channel endpoint.
    IpcChannel,
}

impl SocketType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Tcp => "TCP",
            Self::Udp => "UDP",
            Self::Unix => "Unix",
            Self::IpcChannel => "IPC",
        }
    }
}

/// Socket specification for activation.
#[derive(Debug, Clone)]
pub struct SocketSpec {
    /// Socket type.
    pub socket_type: SocketType,
    /// For TCP/UDP: port number.
    pub port: u16,
    /// For Unix: socket path. For IPC: channel name.
    pub path: String,
    /// Optional bind address for TCP/UDP (empty = any).
    pub bind_addr: String,
    /// Maximum pending connections before refusing.
    pub backlog: u32,
}

/// State of a socket activation entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationState {
    /// Listening for connections, service not yet triggered.
    Listening,
    /// Connection received, service is being started.
    Activating,
    /// Service is running and has claimed the socket.
    Active,
    /// Socket is disabled (won't trigger activation).
    Disabled,
    /// Activation failed (service wouldn't start).
    Failed,
}

impl ActivationState {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Listening => "Listening",
            Self::Activating => "Activating",
            Self::Active => "Active",
            Self::Disabled => "Disabled",
            Self::Failed => "Failed",
        }
    }
}

/// A registered socket activation entry.
#[derive(Debug, Clone)]
struct SocketEntry {
    /// Unique entry ID.
    id: u32,
    /// Associated service ID in servicemgr.
    service_id: u32,
    /// Service name (cached for convenience).
    service_name: String,
    /// Socket specification.
    spec: SocketSpec,
    /// Current activation state.
    state: ActivationState,
    /// Number of times this socket has triggered activation.
    activation_count: u64,
    /// Number of pending connections currently queued.
    pending_connections: u32,
    /// Timestamp of last activation trigger (ns since boot).
    last_activation_ns: u64,
    /// Whether the service should be stopped when the socket is idle
    /// for a configurable timeout.
    idle_stop: bool,
    /// Idle timeout before stopping service (ns). 0 = never stop.
    idle_timeout_ns: u64,
    /// Timestamp of last activity (ns since boot).
    last_activity_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Maximum socket activation entries.
const MAX_ENTRIES: usize = 128;

/// Default idle timeout (5 minutes).
const DEFAULT_IDLE_TIMEOUT_NS: u64 = 300_000_000_000;

struct State {
    entries: Vec<SocketEntry>,
    next_id: u32,
    total_activations: u64,
    total_claims: u64,
    total_failed: u64,
    initialized: bool,
}

impl State {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            total_activations: 0,
            total_claims: 0,
            total_failed: 0,
            initialized: false,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize the socket activation subsystem.
pub fn init() {
    let mut state = STATE.lock();
    if state.initialized {
        return;
    }
    state.initialized = true;
}

/// Register a socket for activation.
///
/// When a connection arrives on this socket and the service is not running,
/// the service will be started automatically.
pub fn register(service_id: u32, spec: SocketSpec) -> KernelResult<u32> {
    let info = crate::fs::servicemgr::get_service(service_id)?;

    let mut state = STATE.lock();
    if !state.initialized {
        return Err(KernelError::NotSupported);
    }

    if state.entries.len() >= MAX_ENTRIES {
        return Err(KernelError::ResourceExhausted);
    }

    // Check for duplicate: same socket type + port or path.
    let dup = state.entries.iter().any(|e| {
        e.spec.socket_type == spec.socket_type
            && ((spec.port > 0 && e.spec.port == spec.port)
                || (!spec.path.is_empty() && e.spec.path == spec.path))
    });
    if dup {
        return Err(KernelError::AlreadyExists);
    }

    let id = state.next_id;
    state.next_id = state.next_id.saturating_add(1);

    state.entries.push(SocketEntry {
        id,
        service_id,
        service_name: info.name.clone(),
        spec,
        state: ActivationState::Listening,
        activation_count: 0,
        pending_connections: 0,
        last_activation_ns: 0,
        idle_stop: false,
        idle_timeout_ns: DEFAULT_IDLE_TIMEOUT_NS,
        last_activity_ns: 0,
    });

    crate::syslog!("service.sockact", Info,
        "Socket activation registered: service '{}' (id={}), entry={}",
        info.name, service_id, id);

    Ok(id)
}

/// Unregister a socket activation entry.
pub fn unregister(entry_id: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.entries.iter().position(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;
    let entry = state.entries.remove(idx);

    crate::syslog!("service.sockact", Info,
        "Socket activation unregistered: service '{}', entry={}",
        entry.service_name, entry_id);

    Ok(())
}

/// Trigger activation: a connection arrived on the socket.
///
/// This is called by the network/IPC layer when a connection is received
/// on a socket-activated port/path.
///
/// Returns `Ok(true)` if the service was started (or is starting),
/// `Ok(false)` if the service is already running.
pub fn trigger(entry_id: u32) -> KernelResult<bool> {
    let mut state = STATE.lock();
    let entry = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;

    match entry.state {
        ActivationState::Disabled => {
            return Err(KernelError::NotSupported);
        }
        ActivationState::Failed => {
            return Err(KernelError::InvalidArgument);
        }
        ActivationState::Active => {
            // Service is already running, just update activity timestamp.
            entry.last_activity_ns = crate::hpet::elapsed_ns();
            entry.pending_connections = entry.pending_connections.saturating_add(1);
            return Ok(false);
        }
        ActivationState::Activating => {
            // Service is already being started, queue the connection.
            entry.pending_connections = entry.pending_connections.saturating_add(1);
            return Ok(true);
        }
        ActivationState::Listening => {
            // Start the service.
        }
    }

    let now = crate::hpet::elapsed_ns();
    let svc_id = entry.service_id;
    let svc_name = entry.service_name.clone();
    entry.state = ActivationState::Activating;
    entry.pending_connections = entry.pending_connections.saturating_add(1);
    entry.last_activation_ns = now;
    entry.last_activity_ns = now;
    #[allow(clippy::arithmetic_side_effects)]
    { entry.activation_count += 1; }
    #[allow(clippy::arithmetic_side_effects)]
    { state.total_activations += 1; }

    // Drop the lock before calling servicemgr (avoids potential deadlock).
    drop(state);

    crate::syslog!("service.sockact", Info,
        "Socket activation triggered for service '{}' (id={})",
        svc_name, svc_id);

    // Start the service.
    match crate::fs::servicemgr::start_service(svc_id) {
        Ok(()) => Ok(true),
        Err(e) => {
            // Mark the entry as failed.
            let mut state = STATE.lock();
            if let Some(entry) = state.entries.iter_mut().find(|e| e.id == entry_id) {
                entry.state = ActivationState::Failed;
            }
            #[allow(clippy::arithmetic_side_effects)]
            { state.total_failed += 1; }

            crate::syslog!("service.sockact", Error,
                "Socket activation failed for '{}': {:?}", svc_name, e);
            Err(e)
        }
    }
}

/// Claim: a service signals that it's ready to accept connections on its socket.
///
/// Called after the service starts and signals ready. Returns the number
/// of pending connections to hand over.
pub fn claim(service_id: u32) -> u32 {
    let mut state = STATE.lock();
    let mut total_pending: u32 = 0;
    let mut claims: u64 = 0;

    for entry in &mut state.entries {
        if entry.service_id == service_id
            && (entry.state == ActivationState::Activating
                || entry.state == ActivationState::Listening)
        {
            entry.state = ActivationState::Active;
            entry.last_activity_ns = crate::hpet::elapsed_ns();
            total_pending = total_pending.saturating_add(entry.pending_connections);
            entry.pending_connections = 0;
            #[allow(clippy::arithmetic_side_effects)]
            { claims += 1; }
        }
    }

    // Deferred update — entries borrow released.
    #[allow(clippy::arithmetic_side_effects)]
    { state.total_claims += claims; }

    if total_pending > 0 {
        crate::syslog!("service.sockact", Info,
            "Service id={} claimed {} pending connections",
            service_id, total_pending);
    }

    total_pending
}

/// Release: a service has stopped, return sockets to listening state.
pub fn release(service_id: u32) {
    let mut state = STATE.lock();
    for entry in &mut state.entries {
        if entry.service_id == service_id && entry.state == ActivationState::Active {
            entry.state = ActivationState::Listening;
        }
    }
}

/// Enable or disable a socket activation entry.
pub fn set_enabled(entry_id: u32, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let entry = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;

    if enabled {
        if entry.state == ActivationState::Disabled || entry.state == ActivationState::Failed {
            entry.state = ActivationState::Listening;
        }
    } else {
        entry.state = ActivationState::Disabled;
    }

    Ok(())
}

/// Configure idle stop: auto-stop the service after idle_timeout_ns of no activity.
pub fn set_idle_stop(entry_id: u32, enabled: bool, timeout_ns: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let entry = state.entries.iter_mut().find(|e| e.id == entry_id)
        .ok_or(KernelError::NotFound)?;

    entry.idle_stop = enabled;
    if timeout_ns > 0 {
        entry.idle_timeout_ns = timeout_ns;
    }

    Ok(())
}

/// Check for idle services that should be stopped.
///
/// Returns a list of service IDs that should be stopped.
pub fn check_idle() -> Vec<u32> {
    let now = crate::hpet::elapsed_ns();
    let state = STATE.lock();
    let mut to_stop = Vec::new();

    for entry in &state.entries {
        if entry.state == ActivationState::Active
            && entry.idle_stop
            && entry.idle_timeout_ns > 0
            && entry.last_activity_ns > 0
        {
            let idle_duration = now.saturating_sub(entry.last_activity_ns);
            if idle_duration >= entry.idle_timeout_ns {
                to_stop.push(entry.service_id);
            }
        }
    }

    to_stop
}

/// List all socket activation entries.
pub fn list() -> Vec<SocketEntryInfo> {
    let state = STATE.lock();
    state.entries.iter().map(|e| SocketEntryInfo {
        id: e.id,
        service_id: e.service_id,
        service_name: e.service_name.clone(),
        socket_type: e.spec.socket_type,
        port: e.spec.port,
        path: e.spec.path.clone(),
        state: e.state,
        activation_count: e.activation_count,
        pending_connections: e.pending_connections,
        idle_stop: e.idle_stop,
    }).collect()
}

/// Public view of a socket entry.
pub struct SocketEntryInfo {
    pub id: u32,
    pub service_id: u32,
    pub service_name: String,
    pub socket_type: SocketType,
    pub port: u16,
    pub path: String,
    pub state: ActivationState,
    pub activation_count: u64,
    pub pending_connections: u32,
    pub idle_stop: bool,
}

// ---------------------------------------------------------------------------
// Statistics and procfs
// ---------------------------------------------------------------------------

/// Aggregate statistics.
pub struct SocketActStats {
    pub total_entries: usize,
    pub listening: usize,
    pub activating: usize,
    pub active: usize,
    pub disabled: usize,
    pub failed: usize,
    pub total_activations: u64,
    pub total_claims: u64,
    pub total_failed: u64,
}

/// Get socket activation statistics.
pub fn stats() -> SocketActStats {
    let state = STATE.lock();
    let mut st = SocketActStats {
        total_entries: state.entries.len(),
        listening: 0,
        activating: 0,
        active: 0,
        disabled: 0,
        failed: 0,
        total_activations: state.total_activations,
        total_claims: state.total_claims,
        total_failed: state.total_failed,
    };

    for entry in &state.entries {
        match entry.state {
            ActivationState::Listening => { st.listening += 1; }
            ActivationState::Activating => { st.activating += 1; }
            ActivationState::Active => { st.active += 1; }
            ActivationState::Disabled => { st.disabled += 1; }
            ActivationState::Failed => { st.failed += 1; }
        }
    }

    st
}

/// Generate content for /proc/sockact.
pub fn procfs_content() -> String {
    let st = stats();
    let entries = list();
    let mut out = String::with_capacity(1024);

    out.push_str("Socket Activation\n");
    out.push_str("=================\n");
    out.push_str(&format!("Total entries:   {}\n", st.total_entries));
    out.push_str(&format!("  Listening:     {}\n", st.listening));
    out.push_str(&format!("  Activating:    {}\n", st.activating));
    out.push_str(&format!("  Active:        {}\n", st.active));
    out.push_str(&format!("  Disabled:      {}\n", st.disabled));
    out.push_str(&format!("  Failed:        {}\n", st.failed));
    out.push_str(&format!("Total triggers:  {}\n", st.total_activations));
    out.push_str(&format!("Total claims:    {}\n", st.total_claims));
    out.push_str(&format!("Total failures:  {}\n", st.total_failed));

    if !entries.is_empty() {
        out.push_str(&format!("\n{:>3} {:>4} {:4} {:>6} {:20} {:12} {:>6} {:>4}\n",
            "ID", "SvcID", "Type", "Port", "Path/Service", "State", "Activ", "Pend"));
        for e in &entries {
            let display = if e.path.is_empty() {
                e.service_name.to_string()
            } else {
                e.path.clone()
            };
            out.push_str(&format!("{:>3} {:>5} {:4} {:>6} {:20} {:12} {:>6} {:>4}\n",
                e.id, e.service_id, e.socket_type.label(),
                if e.port > 0 { format!("{}", e.port) } else { String::from("-") },
                display, e.state.label(), e.activation_count, e.pending_connections));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run socket activation self-tests.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[sockact] Running socket activation self-tests...");

    // Clean slate.
    {
        let mut state = STATE.lock();
        *state = State::new();
    }
    crate::fs::servicemgr::clear_all();
    crate::fs::servicemgr::init_defaults();
    init();

    // Test 1: Register a TCP socket for the network service.
    let net = crate::fs::servicemgr::find_by_name("network")?;
    let entry_id = register(net.id, SocketSpec {
        socket_type: SocketType::Tcp,
        port: 80,
        path: String::new(),
        bind_addr: String::new(),
        backlog: 128,
    })?;
    {
        let state = STATE.lock();
        if state.entries.len() != 1 {
            crate::serial_println!("[sockact]   FAIL: expected 1 entry");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[sockact]   1. Register TCP socket: OK");

    // Test 2: Register a Unix socket for the logging service.
    let log = crate::fs::servicemgr::find_by_name("logging")?;
    let log_entry = register(log.id, SocketSpec {
        socket_type: SocketType::Unix,
        port: 0,
        path: String::from("/run/log.sock"),
        bind_addr: String::new(),
        backlog: 32,
    })?;
    crate::serial_println!("[sockact]   2. Register Unix socket: OK");

    // Test 3: Duplicate rejection.
    let dup = register(net.id, SocketSpec {
        socket_type: SocketType::Tcp,
        port: 80,
        path: String::new(),
        bind_addr: String::new(),
        backlog: 64,
    });
    if dup.is_ok() {
        crate::serial_println!("[sockact]   FAIL: duplicate not rejected");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[sockact]   3. Duplicate rejection: OK");

    // Test 4: Trigger activation (service already running → returns false).
    let triggered = trigger(entry_id)?;
    if triggered {
        crate::serial_println!("[sockact]   FAIL: expected false for already-running service");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[sockact]   4. Trigger (already running): OK");

    // Test 5: Stop service, then trigger (should start it).
    crate::fs::servicemgr::stop_service(net.id)?;
    // Reset entry state to Listening since we manually stopped the service.
    release(net.id);

    let triggered = trigger(entry_id)?;
    if !triggered {
        crate::serial_println!("[sockact]   FAIL: expected true for stopped service activation");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[sockact]   5. Trigger (start service): OK");

    // Test 6: Claim pending connections.
    let pending = claim(net.id);
    if pending != 1 {
        crate::serial_println!("[sockact]   FAIL: expected 1 pending, got {}", pending);
        return Err(KernelError::InternalError);
    }
    {
        let state = STATE.lock();
        let entry = state.entries.iter().find(|e| e.id == entry_id);
        if let Some(e) = entry {
            if e.state != ActivationState::Active {
                crate::serial_println!("[sockact]   FAIL: expected Active state after claim");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[sockact]   6. Claim connections: OK");

    // Test 7: Release (service stopped, back to Listening).
    release(net.id);
    {
        let state = STATE.lock();
        let entry = state.entries.iter().find(|e| e.id == entry_id);
        if let Some(e) = entry {
            if e.state != ActivationState::Listening {
                crate::serial_println!("[sockact]   FAIL: expected Listening after release");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[sockact]   7. Release → Listening: OK");

    // Test 8: Disable/enable.
    set_enabled(entry_id, false)?;
    {
        let state = STATE.lock();
        let entry = state.entries.iter().find(|e| e.id == entry_id);
        if let Some(e) = entry {
            if e.state != ActivationState::Disabled {
                crate::serial_println!("[sockact]   FAIL: expected Disabled");
                return Err(KernelError::InternalError);
            }
        }
    }
    // Trigger on disabled should fail.
    let result = trigger(entry_id);
    if result.is_ok() {
        crate::serial_println!("[sockact]   FAIL: trigger should fail on disabled socket");
        return Err(KernelError::InternalError);
    }
    set_enabled(entry_id, true)?;
    crate::serial_println!("[sockact]   8. Disable/enable: OK");

    // Test 9: Idle stop configuration.
    set_idle_stop(entry_id, true, 60_000_000_000)?; // 60 seconds
    {
        let state = STATE.lock();
        let entry = state.entries.iter().find(|e| e.id == entry_id);
        if let Some(e) = entry {
            if !e.idle_stop || e.idle_timeout_ns != 60_000_000_000 {
                crate::serial_println!("[sockact]   FAIL: idle stop not configured");
                return Err(KernelError::InternalError);
            }
        }
    }
    crate::serial_println!("[sockact]   9. Idle stop config: OK");

    // Test 10: Unregister.
    unregister(log_entry)?;
    {
        let state = STATE.lock();
        if state.entries.len() != 1 {
            crate::serial_println!("[sockact]   FAIL: expected 1 entry after unregister");
            return Err(KernelError::InternalError);
        }
    }
    crate::serial_println!("[sockact]   10. Unregister: OK");

    // Test 11: Stats.
    let st = stats();
    if st.total_activations == 0 {
        crate::serial_println!("[sockact]   FAIL: expected > 0 activations");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[sockact]   11. Stats: OK (activations={})", st.total_activations);

    // Test 12: Procfs content.
    let content = procfs_content();
    if !content.contains("Socket Activation") {
        crate::serial_println!("[sockact]   FAIL: procfs content missing header");
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("[sockact]   12. Procfs content: OK");

    // Clean up.
    crate::fs::servicemgr::clear_all();
    {
        let mut state = STATE.lock();
        *state = State::new();
    }

    crate::serial_println!("[sockact] All 12 self-tests passed.");
    Ok(())
}
