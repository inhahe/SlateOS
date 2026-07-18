//! Service manager — system service lifecycle, dependency tracking, and auto-restart.
//!
//! Manages all system services (daemons) with start/stop control,
//! dependency resolution, and automatic restart on failure.  Used by:
//! - Init system (auto-start services at boot)
//! - Settings panel (service status / enable / disable)
//! - Process explorer (service process mapping)
//!
//! ## Architecture
//!
//! ```text
//! Init boot sequence
//!   → servicemgr::init_defaults()
//!   → registers + starts core services (network, audio, display, …)
//!
//! Service lifecycle
//!   → servicemgr::start_service(id)   → Starting → Running  (pid assigned)
//!   → servicemgr::stop_service(id)    → Stopping → Stopped  (pid cleared)
//!   → servicemgr::restart_service(id) → stop + start, restart_count++
//!
//! Dependency tracking
//!   → servicemgr::add_dependency(id, "network")
//!   → service depends on "network"; stored in depends_on / depended_by
//!
//! Query
//!   → servicemgr::list_services()  — all services
//!   → servicemgr::list_running()   — only Running services
//!   → servicemgr::find_by_name()   — lookup by name string
//!   → servicemgr::stats()          — aggregate counters
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Operational state of a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Service is not running.
    Stopped,
    /// Service is in the process of starting.
    Starting,
    /// Service is running normally.
    Running,
    /// Service is in the process of stopping.
    Stopping,
    /// Service exited with an error.
    Failed,
    /// Service is administratively disabled.
    Disabled,
}

impl ServiceState {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Starting => "Starting",
            Self::Running => "Running",
            Self::Stopping => "Stopping",
            Self::Failed => "Failed",
            Self::Disabled => "Disabled",
        }
    }
}

/// How a service is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupType {
    /// Starts automatically at boot.
    Automatic,
    /// Must be started manually.
    Manual,
    /// Cannot be started.
    Disabled,
    /// Starts automatically after a delay post-boot.
    DelayedAutomatic,
}

impl StartupType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::Manual => "Manual",
            Self::Disabled => "Disabled",
            Self::DelayedAutomatic => "Delayed Automatic",
        }
    }
}

/// Metadata and runtime state for a single service.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Unique service ID.
    pub id: u32,
    /// Short machine-readable name (e.g., "network").
    pub name: String,
    /// Human-readable display name (e.g., "NetworkManager").
    pub display_name: String,
    /// Description of the service's purpose.
    pub description: String,
    /// Current operational state.
    pub state: ServiceState,
    /// How the service is started.
    pub startup_type: StartupType,
    /// Process ID when running; 0 if not running.
    pub pid: u32,
    /// Names of services this service depends on.
    pub depends_on: Vec<String>,
    /// Names of services that depend on this service.
    pub depended_by: Vec<String>,
    /// How many times the service has been restarted.
    pub restart_count: u32,
    /// Timestamp of most recent start (nanoseconds since boot).
    pub last_start_ns: u64,
    /// Timestamp of most recent stop (nanoseconds since boot).
    pub last_stop_ns: u64,
    /// Whether the service should be automatically restarted on failure.
    pub auto_restart: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Maximum services the manager tracks.
const MAX_SERVICES: usize = 200;

struct State {
    services: Vec<ServiceInfo>,
    next_id: u32,
    total_starts: u64,
    total_stops: u64,
    total_failures: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the service manager with the core system-service DEFINITIONS.
///
/// Registers five core services as a compiled-in manifest of the services this
/// OS manages (analogous to shipped systemd unit/preset files). The definitions
/// — names, display names, descriptions, startup types — are configuration, not
/// observations, so they are legitimate defaults.
///
/// Crucially, all services start in the **Stopped** state with **pid 0**: we
/// never fabricate runtime state. A service only becomes `Running` when the
/// init/boot sequence (`svcstart::boot_services`) or an explicit request calls
/// `start_service`. At that point no real OS process is launched yet, so the
/// recorded pid stays 0 (see the DEFERRED note on `start_service`).
///
/// - `network`  (NetworkManager, Automatic)
/// - `audio`    (Audio Server, Automatic)
/// - `display`  (Display Server, Automatic)
/// - `logging`  (System Logger, Automatic)
/// - `cron`     (Task Scheduler, Manual)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let def = |id: u32, name: &str, display_name: &str, description: &str,
               startup_type: StartupType, auto_restart: bool| ServiceInfo {
        id,
        name: String::from(name),
        display_name: String::from(display_name),
        description: String::from(description),
        // Honest initial state: the manager has not started anything yet.
        state: ServiceState::Stopped,
        startup_type,
        pid: 0,
        depends_on: Vec::new(),
        depended_by: Vec::new(),
        restart_count: 0,
        last_start_ns: 0,
        last_stop_ns: 0,
        auto_restart,
    };

    let services = alloc::vec![
        def(1, "network", "NetworkManager",
            "Manages network interfaces and connections", StartupType::Automatic, true),
        def(2, "audio", "Audio Server",
            "Audio mixing and output routing", StartupType::Automatic, true),
        def(3, "display", "Display Server",
            "Compositor and display output management", StartupType::Automatic, true),
        def(4, "logging", "System Logger",
            "Structured logging and log aggregation", StartupType::Automatic, true),
        def(5, "cron", "Task Scheduler",
            "Periodic and scheduled task execution", StartupType::Manual, false),
    ];

    *guard = Some(State {
        services,
        next_id: 6,
        total_starts: 0,
        total_stops: 0,
        total_failures: 0,
        ops: 0,
    });
}

/// Register a new service. Returns the assigned service ID.
pub fn register_service(
    name: &str,
    display_name: &str,
    description: &str,
    startup_type: StartupType,
) -> KernelResult<u32> {
    if name.is_empty() || display_name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    with_state(|st| {
        // Reject duplicates.
        if st.services.iter().any(|s| s.name == name) {
            return Err(KernelError::AlreadyExists);
        }
        if st.services.len() >= MAX_SERVICES {
            return Err(KernelError::ResourceExhausted);
        }

        let id = st.next_id;
        st.next_id = st.next_id.saturating_add(1);

        st.services.push(ServiceInfo {
            id,
            name: String::from(name),
            display_name: String::from(display_name),
            description: String::from(description),
            state: ServiceState::Stopped,
            startup_type,
            pid: 0,
            depends_on: Vec::new(),
            depended_by: Vec::new(),
            restart_count: 0,
            last_start_ns: 0,
            last_stop_ns: 0,
            auto_restart: false,
        });

        Ok(id)
    })
}

/// Start a service by ID. Marks the service `Running`.
///
/// DEFERRED PROPER FIX: this marks the service active in the manager's model
/// but does not yet launch a real OS process — the userspace service binaries
/// and the process/ELF spawn path that would record a real PID are not wired
/// to this manager. Until then the recorded `pid` stays 0 (the established
/// sentinel for "no backing process"); we never fabricate a plausible-looking
/// PID, because procfs would surface it as a real process ID. Trigger to do
/// this properly: when the core service binaries are launchable via the
/// process loader, spawn them here and store the actual PID.
pub fn start_service(id: u32) -> KernelResult<()> {
    with_state(|st| {
        let svc = st.services.iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;

        match svc.state {
            ServiceState::Running | ServiceState::Starting => {
                return Err(KernelError::InvalidArgument);
            }
            ServiceState::Disabled => {
                return Err(KernelError::NotSupported);
            }
            _ => {}
        }

        let now = crate::hpet::elapsed_ns();
        // No real process is spawned yet (see DEFERRED note above): pid 0.
        svc.pid = 0;
        svc.state = ServiceState::Running;
        svc.last_start_ns = now;
        st.total_starts += 1;
        Ok(())
    })
}

/// Stop a service by ID. Clears PID and records the stop timestamp.
pub fn stop_service(id: u32) -> KernelResult<()> {
    with_state(|st| {
        let svc = st.services.iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;

        if svc.state != ServiceState::Running && svc.state != ServiceState::Starting {
            return Err(KernelError::InvalidArgument);
        }

        let now = crate::hpet::elapsed_ns();
        svc.pid = 0;
        svc.state = ServiceState::Stopped;
        svc.last_stop_ns = now;
        st.total_stops += 1;
        Ok(())
    })
}

/// Restart a service (stop then start), incrementing restart_count.
pub fn restart_service(id: u32) -> KernelResult<()> {
    with_state(|st| {
        let svc = st.services.iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;

        // If running, stop first.
        if svc.state == ServiceState::Running || svc.state == ServiceState::Starting {
            let now = crate::hpet::elapsed_ns();
            svc.pid = 0;
            svc.state = ServiceState::Stopped;
            svc.last_stop_ns = now;
            st.total_stops += 1;
        }

        if svc.state == ServiceState::Disabled {
            return Err(KernelError::NotSupported);
        }

        // Start. No real process is spawned yet (see start_service DEFERRED
        // note): pid stays 0 rather than a fabricated value.
        let now = crate::hpet::elapsed_ns();
        svc.pid = 0;
        svc.state = ServiceState::Running;
        svc.last_start_ns = now;
        svc.restart_count = svc.restart_count.saturating_add(1);
        st.total_starts += 1;
        Ok(())
    })
}

/// Change a service's startup type.
pub fn set_startup_type(id: u32, startup_type: StartupType) -> KernelResult<()> {
    with_state(|st| {
        let svc = st.services.iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;

        svc.startup_type = startup_type;

        // If the startup type is Disabled, also mark the service state.
        if startup_type == StartupType::Disabled && svc.state == ServiceState::Stopped {
            svc.state = ServiceState::Disabled;
        }
        Ok(())
    })
}

/// Record that service `id` depends on the service named `depends_on_name`.
///
/// Updates both the `depends_on` list on `id` and the `depended_by` list
/// on the target.
pub fn add_dependency(id: u32, depends_on_name: &str) -> KernelResult<()> {
    if depends_on_name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    with_state(|st| {
        // Verify the dependency target exists and is not self.
        let self_name = {
            let svc = st.services.iter()
                .find(|s| s.id == id)
                .ok_or(KernelError::NotFound)?;
            if svc.name == depends_on_name {
                return Err(KernelError::InvalidArgument);
            }
            svc.name.clone()
        };

        // Verify the target service exists.
        if !st.services.iter().any(|s| s.name == depends_on_name) {
            return Err(KernelError::NotFound);
        }

        // Add to depends_on (avoid duplicates).
        let svc = st.services.iter_mut()
            .find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        let dep_name = String::from(depends_on_name);
        if !svc.depends_on.contains(&dep_name) {
            svc.depends_on.push(dep_name);
        }

        // Add to the target's depended_by (avoid duplicates).
        let target = st.services.iter_mut()
            .find(|s| s.name == depends_on_name)
            .ok_or(KernelError::NotFound)?;
        if !target.depended_by.contains(&self_name) {
            target.depended_by.push(self_name);
        }

        Ok(())
    })
}

/// Retrieve a clone of a service by ID.
pub fn get_service(id: u32) -> KernelResult<ServiceInfo> {
    with_state(|st| {
        st.services.iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Find a service by its short name.
pub fn find_by_name(name: &str) -> KernelResult<ServiceInfo> {
    with_state(|st| {
        st.services.iter()
            .find(|s| s.name == name)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all registered services.
pub fn list_services() -> Vec<ServiceInfo> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(st) => st.services.clone(),
        None => Vec::new(),
    }
}

/// List only services in the Running state.
pub fn list_running() -> Vec<ServiceInfo> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(st) => st.services.iter()
            .filter(|s| s.state == ServiceState::Running)
            .cloned()
            .collect(),
        None => Vec::new(),
    }
}

/// Aggregate statistics.
///
/// Returns `(total_count, running_count, total_starts, total_stops, total_failures, ops)`.
pub fn stats() -> (usize, usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(st) => {
            let total = st.services.len();
            let running = st.services.iter()
                .filter(|s| s.state == ServiceState::Running)
                .count();
            (total, running, st.total_starts, st.total_stops, st.total_failures, st.ops)
        }
        None => (0, 0, 0, 0, 0, 0),
    }
}

/// Reset all state (used by tests).
pub fn clear_all() {
    let mut guard = STATE.lock();
    *guard = None;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the service manager.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: init_defaults registers five services.
    {
        init_defaults();
        let all = list_services();
        assert_eq!(all.len(), 5);
        serial_println!("[servicemgr] test 1 passed: init_defaults registers 5 services");
    }

    // Test 2: no services run by default — the manager never fabricates
    // runtime state; everything starts Stopped until explicitly started.
    {
        let running = list_running();
        assert_eq!(running.len(), 0);
        serial_println!("[servicemgr] test 2 passed: 0 services running by default");
    }

    // Test 3: find_by_name. All defaults are Stopped at init.
    {
        let net = find_by_name("network")?;
        assert_eq!(net.display_name, "NetworkManager");
        assert_eq!(net.state, ServiceState::Stopped);
        assert_eq!(net.pid, 0);

        let cron = find_by_name("cron")?;
        assert_eq!(cron.state, ServiceState::Stopped);
        serial_println!("[servicemgr] test 3 passed: find_by_name");
    }

    // Test 4: get_service by ID.
    {
        let svc = get_service(1)?;
        assert_eq!(svc.name, "network");
        assert!(get_service(999).is_err());
        serial_println!("[servicemgr] test 4 passed: get_service");
    }

    // Test 5: register_service.
    {
        let id = register_service(
            "test-svc",
            "Test Service",
            "A test service for self-test",
            StartupType::Manual,
        )?;
        assert_eq!(id, 6);
        let svc = get_service(id)?;
        assert_eq!(svc.name, "test-svc");
        assert_eq!(svc.state, ServiceState::Stopped);
        serial_println!("[servicemgr] test 5 passed: register_service");
    }

    // Test 6: start_service. State becomes Running; pid stays 0 because no
    // real process is spawned yet (DEFERRED: wire to the process loader).
    {
        start_service(6)?;
        let svc = get_service(6)?;
        assert_eq!(svc.state, ServiceState::Running);
        assert_eq!(svc.pid, 0);
        assert!(svc.last_start_ns > 0);
        serial_println!("[servicemgr] test 6 passed: start_service");
    }

    // Test 7: stop_service.
    {
        stop_service(6)?;
        let svc = get_service(6)?;
        assert_eq!(svc.state, ServiceState::Stopped);
        assert_eq!(svc.pid, 0);
        assert!(svc.last_stop_ns > 0);
        serial_println!("[servicemgr] test 7 passed: stop_service");
    }

    // Test 8: restart_service increments restart_count.
    {
        start_service(6)?;
        restart_service(6)?;
        let svc = get_service(6)?;
        assert_eq!(svc.state, ServiceState::Running);
        assert_eq!(svc.restart_count, 1);
        serial_println!("[servicemgr] test 8 passed: restart_service");
    }

    // Test 9: set_startup_type.
    {
        set_startup_type(6, StartupType::Automatic)?;
        let svc = get_service(6)?;
        assert_eq!(svc.startup_type, StartupType::Automatic);
        serial_println!("[servicemgr] test 9 passed: set_startup_type");
    }

    // Test 10: add_dependency.
    {
        add_dependency(6, "network")?;
        let svc = get_service(6)?;
        assert!(svc.depends_on.contains(&String::from("network")));
        let net = find_by_name("network")?;
        assert!(net.depended_by.contains(&String::from("test-svc")));
        serial_println!("[servicemgr] test 10 passed: add_dependency");
    }

    // Test 11: stats.
    {
        // Only the test-svc (id 6) was started; the five defaults stay Stopped.
        let (total, running, starts, stops, _failures, ops) = stats();
        assert_eq!(total, 6);
        assert_eq!(running, 1);
        assert!(starts > 0);
        assert!(stops > 0);
        assert!(ops > 0);
        serial_println!("[servicemgr] test 11 passed: stats");
    }

    // Test 12: duplicate name rejection.
    {
        let dup = register_service("network", "Dup", "dup", StartupType::Manual);
        assert!(dup.is_err());
        serial_println!("[servicemgr] test 12 passed: duplicate name rejected");
    }

    // Test 13: label methods.
    {
        assert_eq!(ServiceState::Running.label(), "Running");
        assert_eq!(ServiceState::Failed.label(), "Failed");
        assert_eq!(StartupType::DelayedAutomatic.label(), "Delayed Automatic");
        assert_eq!(StartupType::Disabled.label(), "Disabled");
        serial_println!("[servicemgr] test 13 passed: label methods");
    }

    clear_all();

    serial_println!("[servicemgr] all 13 self-tests passed");
    Ok(())
}
