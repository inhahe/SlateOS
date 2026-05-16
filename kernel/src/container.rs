//! Container lifecycle manager — unified container abstraction.
//!
//! Ties together all four namespace types (PID, user, network, mount)
//! and a cgroup to provide Docker-style container isolation.
//!
//! ## Design
//!
//! A container is a coordinated bundle of kernel isolation primitives:
//!
//! - **PID namespace**: isolated PID number space (PID 1 inside container)
//! - **User namespace**: UID/GID remapping (rootless containers)
//! - **Network namespace**: isolated network stack (IP, routing, firewall)
//! - **Mount namespace**: isolated filesystem view (already in fs::mount_ns)
//! - **Cgroup**: CPU, memory, and I/O resource limits
//!
//! The container manager creates and destroys these as a unit, ensuring
//! consistent lifecycle.  When a container is destroyed, all its
//! namespaces and cgroup are cleaned up atomically.
//!
//! ## Container States
//!
//! ```text
//! Created → Running → Stopped → (deleted)
//!                  ↘ Failed ↗
//! ```
//!
//! - **Created**: all namespaces and cgroup allocated, no process yet
//! - **Running**: init process spawned inside the container
//! - **Stopped**: init process exited (can be restarted)
//! - **Failed**: init process crashed or resource setup error
//!
//! ## References
//!
//! - Linux: `runc` container runtime, `unshare(2)`, `clone(2)` with
//!   CLONE_NEWPID | CLONE_NEWUSER | CLONE_NEWNET | CLONE_NEWNS
//! - OCI Runtime Spec (container lifecycle)
//! - Design spec: "Docker: yes, eventually — it needs container
//!   primitives (namespaces, cgroups equivalent)."

use alloc::string::String;
use alloc::vec::Vec;
use crate::error::{KernelError, KernelResult};
use crate::serial_println;
use spin::Mutex;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of containers.
pub const MAX_CONTAINERS: usize = 32;

/// Container name maximum length.
pub const MAX_NAME_LEN: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Unique identifier for a container.
pub type ContainerId = u32;

/// Container state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    /// Namespaces and cgroup allocated, no process yet.
    Created,
    /// Init process running inside the container.
    Running,
    /// Init process exited normally.
    Stopped,
    /// Init process crashed or setup failed.
    Failed,
}

impl core::fmt::Display for ContainerState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Created => write!(f, "created"),
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Configuration for creating a container.
#[derive(Debug, Clone)]
pub struct ContainerConfig {
    /// Container name (for human identification).
    pub name: String,
    /// UID mapping ranges: (inner_start, outer_start, count).
    pub uid_mappings: Vec<(u32, u32, u32)>,
    /// GID mapping ranges: (inner_start, outer_start, count).
    pub gid_mappings: Vec<(u32, u32, u32)>,
    /// CPU quota (0 = unlimited, in ticks per period).
    pub cpu_quota: u64,
    /// Memory limit in frames (0 = unlimited).
    pub mem_limit: u64,
    /// I/O ops limit per period (0 = unlimited).
    pub io_ops_limit: u64,
    /// I/O bytes limit per period (0 = unlimited).
    pub io_bytes_limit: u64,
    /// Network interface configuration (optional).
    pub net_ip: Option<[u8; 4]>,
    pub net_mask: Option<[u8; 4]>,
    pub net_gateway: Option<[u8; 4]>,
    pub net_dns: Option<[u8; 4]>,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            uid_mappings: Vec::new(),
            gid_mappings: Vec::new(),
            cpu_quota: 0,
            mem_limit: 0,
            io_ops_limit: 0,
            io_bytes_limit: 0,
            net_ip: None,
            net_mask: None,
            net_gateway: None,
            net_dns: None,
        }
    }
}

impl ContainerConfig {
    /// Create a minimal container config with a name.
    pub fn new(name: &str) -> Self {
        let mut cfg = Self::default();
        cfg.name = String::from(
            if name.len() > MAX_NAME_LEN { &name[..MAX_NAME_LEN] } else { name }
        );
        cfg
    }

    /// Add a UID mapping range.
    pub fn uid_map(mut self, inner: u32, outer: u32, count: u32) -> Self {
        self.uid_mappings.push((inner, outer, count));
        self
    }

    /// Add a GID mapping range.
    pub fn gid_map(mut self, inner: u32, outer: u32, count: u32) -> Self {
        self.gid_mappings.push((inner, outer, count));
        self
    }

    /// Set CPU quota.
    pub fn cpu(mut self, quota: u64) -> Self {
        self.cpu_quota = quota;
        self
    }

    /// Set memory limit in frames.
    pub fn memory(mut self, frames: u64) -> Self {
        self.mem_limit = frames;
        self
    }

    /// Set I/O limits.
    pub fn io(mut self, ops: u64, bytes: u64) -> Self {
        self.io_ops_limit = ops;
        self.io_bytes_limit = bytes;
        self
    }

    /// Configure network with IPv4 address and optional mask/gateway/DNS.
    ///
    /// When set, a veth pair is automatically created connecting the
    /// container to the host namespace.
    pub fn network(
        mut self,
        ip: [u8; 4],
        mask: Option<[u8; 4]>,
        gateway: Option<[u8; 4]>,
        dns: Option<[u8; 4]>,
    ) -> Self {
        self.net_ip = Some(ip);
        self.net_mask = mask;
        self.net_gateway = gateway;
        self.net_dns = dns;
        self
    }
}

// ---------------------------------------------------------------------------
// Per-container data
// ---------------------------------------------------------------------------

/// Tracks all the kernel objects that make up a container.
struct Container {
    /// Whether this slot is active.
    active: bool,
    /// Human-readable name.
    name: String,
    /// Container state.
    state: ContainerState,
    /// PID namespace ID (from pidns module).
    pid_ns: u32,
    /// User namespace ID (from userns module).
    user_ns: u32,
    /// Network namespace ID (from netns module).
    net_ns: u32,
    /// Cgroup ID (from cgroup module).
    cgroup_id: u32,
    /// Veth pair connecting this container's namespace to the host.
    ///
    /// End A stays in ROOT_NS (host side), end B is moved to the
    /// container's net namespace.  `None` if no network was configured.
    veth_pair: Option<crate::net::veth::VethPairId>,
    /// Process IDs running in this container (global PIDs).
    pids: Vec<u64>,
}

impl Container {
    fn new_empty() -> Self {
        Self {
            active: false,
            name: String::new(),
            state: ContainerState::Created,
            pid_ns: 0,
            user_ns: 0,
            net_ns: 0,
            cgroup_id: 0,
            veth_pair: None,
            pids: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot type
// ---------------------------------------------------------------------------

/// Read-only snapshot of a container's state.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public API — fields read by kshell and syscall handlers.
pub struct ContainerInfo {
    /// Container ID.
    pub id: ContainerId,
    /// Container name.
    pub name: String,
    /// Container state.
    pub state: ContainerState,
    /// PID namespace ID.
    pub pid_ns: u32,
    /// User namespace ID.
    pub user_ns: u32,
    /// Network namespace ID.
    pub net_ns: u32,
    /// Cgroup ID.
    pub cgroup_id: u32,
    /// Veth pair ID connecting to the host (None if no network configured).
    pub veth_pair: Option<crate::net::veth::VethPairId>,
    /// Number of processes.
    pub nr_procs: usize,
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

struct ContainerTable {
    containers: Vec<Container>,
    next_id: u32,
}

impl ContainerTable {
    fn new() -> Self {
        let mut containers = Vec::with_capacity(MAX_CONTAINERS);
        for _ in 0..MAX_CONTAINERS {
            containers.push(Container::new_empty());
        }
        Self {
            containers,
            next_id: 0,
        }
    }
}

static TABLE: Mutex<Option<ContainerTable>> = Mutex::new(None);

/// Initialize the container manager.
pub fn init() {
    let mut table = TABLE.lock();
    *table = Some(ContainerTable::new());
    serial_println!("[container] Initialized ({} max containers)", MAX_CONTAINERS);
}

fn with_table<F, R>(f: F) -> R
where
    F: FnOnce(&mut ContainerTable) -> R,
{
    let mut guard = TABLE.lock();
    let table = guard.as_mut().expect("[container] not initialized");
    f(table)
}

fn with_table_ref<F, R>(f: F) -> R
where
    F: FnOnce(&ContainerTable) -> R,
{
    let guard = TABLE.lock();
    let table = guard.as_ref().expect("[container] not initialized");
    f(table)
}

// ---------------------------------------------------------------------------
// Public API: lifecycle
// ---------------------------------------------------------------------------

/// Set up a veth pair for container networking.
///
/// Creates a pair, moves end B to the container's namespace, and
/// brings both ends up.  End A stays in ROOT_NS (host side).
///
/// On any failure, partially-created resources are cleaned up.
fn setup_container_veth(net_ns: u32) -> KernelResult<crate::net::veth::VethPairId> {
    use crate::net::veth::{self, VethEndId};

    // Create the pair (both ends start in ROOT_NS, both down).
    let pair_id = veth::create_pair()?;

    // Move end B to the container's namespace.
    if let Err(e) = veth::move_end(pair_id, VethEndId::B, net_ns) {
        let _ = veth::destroy_pair(pair_id);
        return Err(e);
    }

    // Bring up both ends.
    if let Err(e) = veth::set_up(pair_id, VethEndId::A, true) {
        let _ = veth::destroy_pair(pair_id);
        return Err(e);
    }
    if let Err(e) = veth::set_up(pair_id, VethEndId::B, true) {
        let _ = veth::set_up(pair_id, VethEndId::A, false); // Best-effort rollback.
        let _ = veth::destroy_pair(pair_id);
        return Err(e);
    }

    Ok(pair_id)
}

/// Create a new container with the given configuration.
///
/// Allocates all four namespace types and a cgroup, applies
/// configuration (UID/GID mappings, resource limits, network config).
/// When a network IP is configured, a veth pair is automatically
/// created connecting the container to the host.
///
/// The container starts in `Created` state — call [`start`] to
/// attach processes.
///
/// # Errors
///
/// - [`KernelError::ResourceExhausted`] if no container slots or
///   any sub-resource is exhausted.
/// - [`KernelError::InvalidArgument`] on invalid configuration.
///
/// On error, all partially-created resources are rolled back.
pub fn create(config: &ContainerConfig) -> KernelResult<ContainerId> {
    // --- Phase 1: Find a free container slot. ---

    let slot = with_table(|table| {
        let start = table.next_id as usize;
        for offset in 0..MAX_CONTAINERS {
            #[allow(clippy::arithmetic_side_effects)]
            let idx = (start + offset) % MAX_CONTAINERS;
            if !table.containers[idx].active {
                return Ok(idx);
            }
        }
        Err(KernelError::ResourceExhausted)
    })?;

    // --- Phase 2: Create sub-resources (with rollback on failure). ---

    // 2a: PID namespace.
    let pid_ns = crate::pidns::create(crate::pidns::ROOT_NS)
        .map_err(|e| {
            serial_println!("[container] Failed to create PID namespace: {:?}", e);
            e
        })?;

    // 2b: User namespace.
    let user_ns = crate::userns::create(crate::userns::ROOT_NS, 0)
        .map_err(|e| {
            serial_println!("[container] Failed to create user namespace: {:?}", e);
            let _ = crate::pidns::delete(pid_ns);
            e
        })?;

    // 2c: Network namespace.
    let net_ns = crate::netns::create()
        .map_err(|e| {
            serial_println!("[container] Failed to create network namespace: {:?}", e);
            let _ = crate::userns::delete(user_ns);
            let _ = crate::pidns::delete(pid_ns);
            e
        })?;

    // 2d: Cgroup.
    let cgroup_id = crate::cgroup::create(crate::cgroup::ROOT_CGROUP)
        .map_err(|e| {
            serial_println!("[container] Failed to create cgroup: {:?}", e);
            let _ = crate::netns::delete(net_ns);
            let _ = crate::userns::delete(user_ns);
            let _ = crate::pidns::delete(pid_ns);
            e
        })?;

    // --- Phase 3: Apply configuration. ---

    // 3a: UID mappings.
    for &(inner, outer, count) in &config.uid_mappings {
        if let Err(e) = crate::userns::add_uid_mapping(user_ns, inner, outer, count) {
            serial_println!("[container] Failed to add UID mapping: {:?}", e);
            // Rollback.
            let _ = crate::cgroup::delete(cgroup_id);
            let _ = crate::netns::delete(net_ns);
            let _ = crate::userns::delete(user_ns);
            let _ = crate::pidns::delete(pid_ns);
            return Err(e);
        }
    }

    // 3b: GID mappings.
    for &(inner, outer, count) in &config.gid_mappings {
        if let Err(e) = crate::userns::add_gid_mapping(user_ns, inner, outer, count) {
            serial_println!("[container] Failed to add GID mapping: {:?}", e);
            let _ = crate::cgroup::delete(cgroup_id);
            let _ = crate::netns::delete(net_ns);
            let _ = crate::userns::delete(user_ns);
            let _ = crate::pidns::delete(pid_ns);
            return Err(e);
        }
    }

    // 3c: Resource limits.
    if config.cpu_quota > 0 {
        let _ = crate::cgroup::set_cpu_limit(
            cgroup_id,
            crate::cgroup::CpuLimit::from_percent(config.cpu_quota),
        );
    }
    if config.mem_limit > 0 {
        let _ = crate::cgroup::set_mem_limit(
            cgroup_id,
            crate::cgroup::MemLimit::frames(config.mem_limit),
        );
    }
    if config.io_ops_limit > 0 || config.io_bytes_limit > 0 {
        let _ = crate::cgroup::set_io_limit(
            cgroup_id,
            crate::cgroup::IoLimit::new(config.io_ops_limit, config.io_bytes_limit),
        );
    }

    // 3d: Network interface + veth pair.
    //
    // When a container has a network IP configured, we automatically
    // create a veth pair connecting the container's namespace to the
    // host (ROOT_NS).  End A stays in the host namespace; end B is
    // moved to the container's namespace.  Both ends are brought up.
    //
    // This mirrors `ip link add veth0 type veth peer name veth1;
    // ip link set veth1 netns <ns>; ip link set veth0 up; ip link set veth1 up`.
    let mut veth_pair: Option<crate::net::veth::VethPairId> = None;

    if let Some(ip) = config.net_ip {
        let ip = crate::netns::Ipv4Addr(ip);
        let mask = config.net_mask.map(crate::netns::Ipv4Addr)
            .unwrap_or(crate::netns::Ipv4Addr::new(255, 255, 255, 0));
        let gw = config.net_gateway.map(crate::netns::Ipv4Addr)
            .unwrap_or(crate::netns::Ipv4Addr::UNSPECIFIED);
        let dns = config.net_dns.map(crate::netns::Ipv4Addr)
            .unwrap_or(crate::netns::Ipv4Addr::UNSPECIFIED);
        let _ = crate::netns::configure_interface(net_ns, ip, mask, gw, dns);

        // Create a veth pair and wire it up.
        match setup_container_veth(net_ns) {
            Ok(pair_id) => {
                veth_pair = Some(pair_id);
                serial_println!(
                    "[container] '{}': veth pair {} (host <-> ns {})",
                    config.name, pair_id, net_ns
                );
            }
            Err(e) => {
                // Non-fatal: container works but without host connectivity.
                // This can happen if all veth slots are exhausted.
                serial_println!(
                    "[container] '{}': veth setup failed: {:?} (no host link)",
                    config.name, e
                );
            }
        }
    }

    // --- Phase 4: Record the container. ---

    with_table(|table| {
        let ct = &mut table.containers[slot];
        ct.active = true;
        ct.name = config.name.clone();
        ct.state = ContainerState::Created;
        ct.pid_ns = pid_ns;
        ct.user_ns = user_ns;
        ct.net_ns = net_ns;
        ct.cgroup_id = cgroup_id;
        ct.veth_pair = veth_pair;
        ct.pids.clear();

        #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
        {
            table.next_id = ((slot + 1) % MAX_CONTAINERS) as u32;
        }
    });

    serial_println!(
        "[container] Created '{}' (id={}, pidns={}, userns={}, netns={}, cgroup={}, veth={:?})",
        config.name, slot, pid_ns, user_ns, net_ns, cgroup_id, veth_pair
    );

    Ok(slot as ContainerId)
}

/// Mark a container as running.
///
/// Called after the init process has been spawned inside the container.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if container doesn't exist.
/// - [`KernelError::InvalidArgument`] if not in Created state.
pub fn start(id: ContainerId) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].state = ContainerState::Running;
        Ok(())
    })
}

/// Mark a container as stopped.
///
/// Called when the init process exits.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if container doesn't exist.
pub fn stop(id: ContainerId) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].state = ContainerState::Stopped;
        Ok(())
    })
}

/// Mark a container as failed.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if container doesn't exist.
pub fn mark_failed(id: ContainerId) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].state = ContainerState::Failed;
        Ok(())
    })
}

/// Delete a container and all its sub-resources.
///
/// Cleans up the PID namespace, user namespace, network namespace,
/// and cgroup.  The container must be in Stopped or Failed state
/// (no running processes).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if container doesn't exist.
/// - [`KernelError::InvalidArgument`] if container is Running.
pub fn delete(id: ContainerId) -> KernelResult<()> {
    // Extract sub-resource IDs while holding the table lock.
    let (pid_ns, user_ns, net_ns, cgroup_id, veth_pair, name) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state == ContainerState::Running {
            return Err(KernelError::InvalidArgument);
        }

        let ct = &table.containers[idx];
        let result = (ct.pid_ns, ct.user_ns, ct.net_ns, ct.cgroup_id,
                      ct.veth_pair, ct.name.clone());

        // Mark slot as inactive.
        table.containers[idx].active = false;
        table.containers[idx].name.clear();
        table.containers[idx].veth_pair = None;
        table.containers[idx].pids.clear();

        Ok(result)
    })?;

    // Clean up sub-resources outside the table lock (each has its own lock).
    // Ignore errors — the sub-resources may have already been cleaned up
    // if a partial failure occurred during create.
    //
    // Destroy veth pair first (before netns) since the endpoint lives
    // in the namespace.
    if let Some(pair_id) = veth_pair {
        let _ = crate::net::veth::destroy_pair(pair_id);
    }
    let _ = crate::cgroup::delete(cgroup_id);
    let _ = crate::netns::delete(net_ns);
    let _ = crate::userns::delete(user_ns);
    let _ = crate::pidns::delete(pid_ns);

    serial_println!("[container] Deleted '{}' (id={})", name, id);

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: process tracking
// ---------------------------------------------------------------------------

/// Register a process as belonging to a container.
///
/// Increments process counts in all the container's namespaces.
pub fn add_process(id: ContainerId, global_pid: u64) -> KernelResult<()> {
    let (pid_ns, user_ns, net_ns, cgroup_id) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].pids.push(global_pid);
        Ok((
            table.containers[idx].pid_ns,
            table.containers[idx].user_ns,
            table.containers[idx].net_ns,
            table.containers[idx].cgroup_id,
        ))
    })?;

    // Track in sub-resources.
    // pidns uses alloc_pid (maps global PID into namespace).
    let _ = crate::pidns::alloc_pid(pid_ns, global_pid);
    let _ = crate::userns::attach_process(user_ns);
    let _ = crate::netns::attach_process(net_ns);
    let _ = crate::cgroup::attach_task(cgroup_id);

    Ok(())
}

/// Unregister a process from a container.
pub fn remove_process(id: ContainerId, global_pid: u64) -> KernelResult<()> {
    let (pid_ns, user_ns, net_ns, cgroup_id) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].pids.retain(|&p| p != global_pid);
        Ok((
            table.containers[idx].pid_ns,
            table.containers[idx].user_ns,
            table.containers[idx].net_ns,
            table.containers[idx].cgroup_id,
        ))
    })?;

    // pidns uses free_pid (removes global PID mapping from namespace).
    let _ = crate::pidns::free_pid(pid_ns, global_pid);
    let _ = crate::userns::detach_process(user_ns);
    let _ = crate::netns::detach_process(net_ns);
    let _ = crate::cgroup::detach_task(cgroup_id);

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: queries
// ---------------------------------------------------------------------------

/// Get container information.
#[must_use]
pub fn info(id: ContainerId) -> Option<ContainerInfo> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        let ct = &table.containers[idx];
        Some(ContainerInfo {
            id,
            name: ct.name.clone(),
            state: ct.state,
            pid_ns: ct.pid_ns,
            user_ns: ct.user_ns,
            net_ns: ct.net_ns,
            cgroup_id: ct.cgroup_id,
            veth_pair: ct.veth_pair,
            nr_procs: ct.pids.len(),
        })
    })
}

/// Check if a container exists.
#[must_use]
pub fn exists(id: ContainerId) -> bool {
    with_table_ref(|table| {
        let idx = id as usize;
        idx < MAX_CONTAINERS && table.containers[idx].active
    })
}

/// Count active containers.
#[must_use]
pub fn active_count() -> usize {
    with_table_ref(|table| {
        table.containers.iter().filter(|c| c.active).count()
    })
}

/// List all active container IDs and names.
#[must_use]
pub fn list() -> Vec<(ContainerId, String, ContainerState)> {
    with_table_ref(|table| {
        let mut result = Vec::new();
        for (i, ct) in table.containers.iter().enumerate() {
            if ct.active {
                result.push((i as ContainerId, ct.name.clone(), ct.state));
            }
        }
        result
    })
}

/// Get the namespace IDs for a container (for process spawning).
#[must_use]
#[allow(dead_code)] // Future: used by process spawn to set up namespace context.
pub fn namespace_ids(id: ContainerId) -> Option<(u32, u32, u32)> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        let ct = &table.containers[idx];
        Some((ct.pid_ns, ct.user_ns, ct.net_ns))
    })
}

/// Get the cgroup ID for a container (for task attachment).
#[must_use]
#[allow(dead_code)] // Future: used by process spawn for cgroup attachment.
pub fn cgroup(id: ContainerId) -> Option<u32> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some(table.containers[idx].cgroup_id)
    })
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Comprehensive self-test for the container lifecycle manager.
pub fn self_test() {
    serial_println!("[container] Running self-test...");

    // Test 1: No containers initially.
    assert_eq!(active_count(), 0);
    serial_println!("[container]   Initial state: OK");

    // Test 2: Create a basic container.
    let cfg = ContainerConfig::new("test-ct1");
    let ct1 = create(&cfg).expect("create container");
    assert!(exists(ct1));
    assert_eq!(active_count(), 1);
    serial_println!("[container]   Create basic: OK");

    // Test 3: Container info.
    let ci = info(ct1).unwrap();
    assert_eq!(ci.name, "test-ct1");
    assert_eq!(ci.state, ContainerState::Created);
    assert_eq!(ci.nr_procs, 0);
    // Verify sub-resources were allocated.
    assert!(crate::pidns::exists(ci.pid_ns));
    assert!(crate::userns::exists(ci.user_ns));
    assert!(crate::netns::exists(ci.net_ns));
    serial_println!("[container]   Container info: OK");

    // Test 4: State transitions.
    start(ct1).expect("start");
    assert_eq!(info(ct1).unwrap().state, ContainerState::Running);
    // Can't start twice.
    assert!(start(ct1).is_err());
    stop(ct1).expect("stop");
    assert_eq!(info(ct1).unwrap().state, ContainerState::Stopped);
    serial_println!("[container]   State transitions: OK");

    // Test 5: Can't delete running container.
    let cfg2 = ContainerConfig::new("test-ct2");
    let ct2 = create(&cfg2).expect("create ct2");
    start(ct2).expect("start ct2");
    assert!(delete(ct2).is_err(), "can't delete running");
    stop(ct2).expect("stop ct2");
    serial_println!("[container]   Delete protection: OK");

    // Test 6: Create with UID mapping and resource limits.
    let cfg3 = ContainerConfig::new("test-ct3")
        .uid_map(0, 100_000, 1000)
        .gid_map(0, 200_000, 500)
        .cpu(50)
        .memory(1024);
    let ct3 = create(&cfg3).expect("create ct3 with config");
    let ci3 = info(ct3).unwrap();
    // Verify UID mapping was applied.
    assert_eq!(crate::userns::uid_to_outer(ci3.user_ns, 0), 100_000);
    assert_eq!(crate::userns::uid_to_outer(ci3.user_ns, 999), 100_999);
    // Verify GID mapping.
    assert_eq!(crate::userns::gid_to_outer(ci3.user_ns, 0), 200_000);
    serial_println!("[container]   Config with mappings + limits: OK");

    // Test 7: Process tracking.
    start(ct3).expect("start ct3");
    add_process(ct3, 42).expect("add process");
    add_process(ct3, 43).expect("add process");
    assert_eq!(info(ct3).unwrap().nr_procs, 2);
    remove_process(ct3, 42).expect("remove process");
    assert_eq!(info(ct3).unwrap().nr_procs, 1);
    remove_process(ct3, 43).expect("remove process");
    serial_println!("[container]   Process tracking: OK");

    // Test 8: List containers.
    let all = list();
    assert_eq!(all.len(), 3);
    serial_println!("[container]   List: OK");

    // Test 9: Namespace IDs.
    let (pid_ns, user_ns, net_ns) = namespace_ids(ct3).unwrap();
    assert!(pid_ns > 0);
    assert!(user_ns > 0);
    assert!(net_ns > 0);
    serial_println!("[container]   Namespace IDs: OK");

    // Test 10: Cgroup ID.
    let cg = cgroup(ct3).unwrap();
    assert!(cg > 0);
    serial_println!("[container]   Cgroup ID: OK");

    // Test 11: Delete container + verify sub-resources freed.
    let ci1 = info(ct1).unwrap();
    let saved_pid_ns = ci1.pid_ns;
    let saved_user_ns = ci1.user_ns;
    let saved_net_ns = ci1.net_ns;
    delete(ct1).expect("delete ct1");
    assert!(!exists(ct1));
    // Sub-resources should be freed.
    assert!(!crate::pidns::exists(saved_pid_ns));
    assert!(!crate::userns::exists(saved_user_ns));
    assert!(!crate::netns::exists(saved_net_ns));
    serial_println!("[container]   Delete + cleanup: OK");

    // Test 12: Failed state.
    let cfg4 = ContainerConfig::new("test-fail");
    let ct4 = create(&cfg4).expect("create ct4");
    start(ct4).expect("start ct4");
    mark_failed(ct4).expect("mark failed");
    assert_eq!(info(ct4).unwrap().state, ContainerState::Failed);
    delete(ct4).expect("delete failed container");
    serial_println!("[container]   Failed state: OK");

    // Test 13: Invalid container operations.
    assert!(!exists(99));
    assert!(info(99).is_none());
    assert!(start(99).is_err());
    assert!(delete(99).is_err());
    serial_println!("[container]   Invalid operations rejected: OK");

    // Test 14: Container name.
    let cfg5 = ContainerConfig::new("my-container-with-a-long-name");
    let ct5 = create(&cfg5).expect("create ct5");
    assert_eq!(info(ct5).unwrap().name, "my-container-with-a-long-name");
    serial_println!("[container]   Container naming: OK");

    // Test 15: Container with network config gets automatic veth pair.
    {
        let net_cfg = ContainerConfig::new("test-veth-ct")
            .uid_map(0, 300_000, 1)
            .gid_map(0, 300_000, 1);
        // Set network config manually (builder doesn't have a net() method).
        let mut net_cfg = net_cfg;
        net_cfg.net_ip = Some([10, 88, 0, 2]);
        net_cfg.net_mask = Some([255, 255, 255, 0]);
        net_cfg.net_gateway = Some([10, 88, 0, 1]);

        let ct_net = create(&net_cfg).expect("create networked container");
        let ci_net = info(ct_net).unwrap();

        // Should have a veth pair assigned.
        assert!(ci_net.veth_pair.is_some(),
            "networked container should have veth pair");

        // Container without network should NOT have a veth pair.
        let plain_cfg = ContainerConfig::new("test-no-net");
        let ct_plain = create(&plain_cfg).expect("create plain container");
        let ci_plain = info(ct_plain).unwrap();
        assert!(ci_plain.veth_pair.is_none(),
            "non-networked container should have no veth pair");

        // Clean up: delete destroys the veth pair too.
        delete(ct_net).expect("delete networked ct");
        delete(ct_plain).expect("delete plain ct");
    }
    serial_println!("[container]   Veth auto-setup: OK");

    // Cleanup.
    stop(ct2).ok(); // may already be stopped
    stop(ct3).ok();
    delete(ct2).expect("cleanup ct2");
    delete(ct3).expect("cleanup ct3");
    delete(ct5).expect("cleanup ct5");
    assert_eq!(active_count(), 0);
    serial_println!("[container]   Cleanup: OK");

    serial_println!("[container] Self-test PASSED (15 tests)");
}
