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

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

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

/// Maximum number of volume (bind) mounts per container.  Kept at the
/// per-process namespace cap so a container can never queue more volumes
/// than [`crate::ipc::namespace::add_volume`] will accept.
pub const MAX_VOLUMES_PER_CONTAINER: usize = 64;

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
#[derive(Default)]
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


impl ContainerConfig {
    /// Create a minimal container config with a name.
    pub fn new(name: &str) -> Self {
        let name = String::from(
            if name.len() > MAX_NAME_LEN { &name[..MAX_NAME_LEN] } else { name }
        );
        Self {
            name,
            ..Self::default()
        }
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
    /// The container's init process (PID 1 inside the container), i.e. the
    /// process launched by [`run`].  `None` until the container has been
    /// run.  When the init process exits, the container is considered
    /// stopped (Docker semantics: the container lives as long as its
    /// init process).
    init_pid: Option<u64>,
    /// Filesystem root (chroot) for processes in this container.
    ///
    /// An absolute host path (e.g. the container's overlay rootfs
    /// `/containers/<id>/rootfs`) that every process launched by [`run`] is
    /// jailed to via [`crate::ipc::namespace::set_root`].  Empty string
    /// means "no jail" — processes see the host root (used by tests and by
    /// containers whose rootfs has not been configured).
    root_path: String,
    /// VFS mountpoint of this container's overlay rootfs, if one was mounted
    /// for copy-on-write isolation (e.g. `/containers/<name>/rootfs`).
    ///
    /// When non-empty, [`delete`] unmounts this path from the VFS so the
    /// per-container `OverlayFs` adapter is released.  Empty means the
    /// container's jail (if any) points at a plain host directory that the
    /// container module does not own and must not unmount.
    rootfs_mount: String,
    /// Volume (bind) mounts as `(guest_prefix, host_target)` pairs — the
    /// Docker `-v host_target:guest_prefix` mechanism.  Each is installed on
    /// every process launched by [`run`] via
    /// [`crate::ipc::namespace::add_volume`], so a guest path under
    /// `guest_prefix` resolves to `host_target` instead of under the
    /// container rootfs.  Empty for a container with no volumes.
    volumes: Vec<(String, String)>,
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
            init_pid: None,
            root_path: String::new(),
            rootfs_mount: String::new(),
            volumes: Vec::new(),
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
    /// The container's init process (global PID), or `None` if the
    /// container has not been run yet.
    pub init_pid: Option<u64>,
    /// Filesystem root (chroot) for the container, or empty if processes
    /// see the host root (no rootfs configured).
    pub root_path: String,
    /// VFS mountpoint of the container's overlay rootfs, or empty if the
    /// container does not own a mounted overlay (the jail, if any, points at
    /// a plain host directory). Unmounted by [`delete`].
    pub rootfs_mount: String,
    /// Volume (bind) mounts as `(guest_prefix, host_target)` pairs.
    pub volumes: Vec<(String, String)>,
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

/// Check whether the container subsystem has been initialized.
pub fn is_initialized() -> bool {
    TABLE.lock().is_some()
}

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
        .inspect_err(|&e| {
            serial_println!("[container] Failed to create PID namespace: {:?}", e);
        })?;

    // 2b: User namespace.
    let user_ns = crate::userns::create(crate::userns::ROOT_NS, 0)
        .inspect_err(|&e| {
            serial_println!("[container] Failed to create user namespace: {:?}", e);
            let _ = crate::pidns::delete(pid_ns);
        })?;

    // 2c: Network namespace.
    let net_ns = crate::netns::create()
        .inspect_err(|&e| {
            serial_println!("[container] Failed to create network namespace: {:?}", e);
            let _ = crate::userns::delete(user_ns);
            let _ = crate::pidns::delete(pid_ns);
        })?;

    // 2d: Cgroup.
    let cgroup_id = crate::cgroup::create(crate::cgroup::ROOT_CGROUP)
        .inspect_err(|&e| {
            serial_println!("[container] Failed to create cgroup: {:?}", e);
            let _ = crate::netns::delete(net_ns);
            let _ = crate::userns::delete(user_ns);
            let _ = crate::pidns::delete(pid_ns);
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
    let (pid_ns, user_ns, net_ns, cgroup_id, veth_pair, name, rootfs_mount) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state == ContainerState::Running {
            return Err(KernelError::InvalidArgument);
        }

        let ct = &table.containers[idx];
        let result = (ct.pid_ns, ct.user_ns, ct.net_ns, ct.cgroup_id,
                      ct.veth_pair, ct.name.clone(), ct.rootfs_mount.clone());

        // Mark slot as inactive.
        table.containers[idx].active = false;
        table.containers[idx].name.clear();
        table.containers[idx].veth_pair = None;
        table.containers[idx].pids.clear();
        table.containers[idx].init_pid = None;
        table.containers[idx].root_path.clear();
        table.containers[idx].rootfs_mount.clear();
        table.containers[idx].volumes.clear();

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
    // Flush NAT entries and port-forward rules before tearing down namespace.
    crate::net::nat::flush_namespace(net_ns);
    crate::net::nat::flush_port_forwards(net_ns);
    let _ = crate::cgroup::delete(cgroup_id);
    let _ = crate::netns::delete(net_ns);
    let _ = crate::userns::delete(user_ns);
    let _ = crate::pidns::delete(pid_ns);

    // Release the container's overlay rootfs mount, if it owns one.  Done
    // outside the table lock (VFS has its own per-mount locking) and only
    // when the container actually mounted an overlay — when `rootfs_mount`
    // is empty the jail (if any) points at a plain host directory we don't
    // own and must not unmount.
    if !rootfs_mount.is_empty() {
        let _ = crate::fs::Vfs::unmount(&rootfs_mount);
    }

    serial_println!("[container] Deleted '{}' (id={})", name, id);

    Ok(())
}

// ---------------------------------------------------------------------------
// Public API: process tracking
// ---------------------------------------------------------------------------

/// Register a process as belonging to a container.
///
/// Convenience wrapper over [`add_process_task`] for callers that do not
/// distinguish the global PID from the initial-thread task id (e.g.
/// binding the *current* task, where the two coincide).  Prefer
/// [`add_process_task`] when launching a fresh process whose PID and
/// task id are distinct allocations (see [`run`]).
pub fn add_process(id: ContainerId, global_pid: u64) -> KernelResult<()> {
    add_process_task(id, global_pid, global_pid)
}

/// Register an already-spawned process in a container, distinguishing the
/// global process id from the process's initial-thread task id.
///
/// - `pid` — the global process id.  It is tracked in the container's
///   process list and mapped into the container's PID namespace.
/// - `task_id` — the process's *initial thread* (scheduler task).  The
///   cgroup assignment (Q14 resource billing) and network-namespace
///   assignment are keyed on the task, not the process: threads the
///   process spawns later inherit the cgroup automatically on
///   creation (`sched::spawn` copies the creator's `cgroup_id`).
///
/// The two ids are independent allocations — for a freshly
/// [`spawn`](crate::proc::spawn::spawn_process)ed process they generally
/// differ — so binding the scheduler resources to the *process id* (as a
/// naive wrapper would) silently no-ops when no task carries that id.
/// [`run`] always uses this entry point with both ids from the spawn
/// result.
pub fn add_process_task(id: ContainerId, pid: u64, task_id: u64) -> KernelResult<()> {
    let (pid_ns, user_ns, net_ns, cgroup_id, root_path, volumes) =
        with_table(|table| {
            let idx = id as usize;
            if idx >= MAX_CONTAINERS || !table.containers[idx].active {
                return Err(KernelError::InvalidArgument);
            }
            table.containers[idx].pids.push(pid);
            Ok((
                table.containers[idx].pid_ns,
                table.containers[idx].user_ns,
                table.containers[idx].net_ns,
                table.containers[idx].cgroup_id,
                table.containers[idx].root_path.clone(),
                table.containers[idx].volumes.clone(),
            ))
        })?;

    // Track in sub-resources.
    // pidns uses alloc_pid (maps global PID into namespace).
    let _ = crate::pidns::alloc_pid(pid_ns, pid);
    let _ = crate::userns::attach_process(user_ns);
    let _ = crate::netns::attach_process(net_ns);

    // Assign the *task* to the container's cgroup.  `set_task_cgroup` both
    // sets the task's `cgroup_id` (so the frame allocator and scheduler
    // bill the container's group — the assignment that was previously
    // missing, D-CGROUP-TASK-UNASSIGNED) and maintains the group's task
    // count; it supersedes a bare `cgroup::attach_task`, which only
    // bumped the counter without ever pointing the task at the group.
    let _ = crate::sched::set_task_cgroup(task_id, cgroup_id);

    // Set the task's net_ns field so syscall handlers automatically use
    // this container's network namespace for socket operations.
    let _ = crate::sched::set_task_net_ns(task_id, net_ns);

    // Jail the process to the container's filesystem root, if one is
    // configured.  The jail is keyed on the *global PID* (not the task id):
    // VFS path resolution looks the root up via the current task's owning
    // process, and child threads share the process, so they inherit the
    // jail automatically.  An empty `root_path` means no jail.
    if !root_path.is_empty() {
        let _ = crate::ipc::namespace::set_root(pid, &root_path);
    }

    // Install the container's volume (bind) mounts on the process, keyed on
    // the same global PID as the chroot.  Each maps a guest path prefix to a
    // host target that escapes the rootfs.  A malformed pair (rejected by
    // `add_volume`) is skipped rather than failing the whole bind — the
    // volume list is validated at `add_volume_mount` time, so this is purely
    // defensive.
    for (guest_prefix, host_target) in &volumes {
        let _ = crate::ipc::namespace::add_volume(pid, guest_prefix, host_target);
    }

    Ok(())
}

/// Unregister a process from a container.
///
/// Convenience wrapper over [`remove_process_task`] for the
/// pid==task_id case (symmetric with [`add_process`]).
pub fn remove_process(id: ContainerId, global_pid: u64) -> KernelResult<()> {
    remove_process_task(id, global_pid, global_pid)
}

/// Unregister a process from a container, distinguishing the global PID
/// (untracked / unmapped from the PID namespace) from the initial-thread
/// task id (whose cgroup and network namespace are reset to the host).
///
/// Symmetric counterpart of [`add_process_task`].
pub fn remove_process_task(id: ContainerId, pid: u64, task_id: u64) -> KernelResult<()> {
    let (pid_ns, user_ns, net_ns) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].pids.retain(|&p| p != pid);
        if table.containers[idx].init_pid == Some(pid) {
            table.containers[idx].init_pid = None;
        }
        Ok((
            table.containers[idx].pid_ns,
            table.containers[idx].user_ns,
            table.containers[idx].net_ns,
        ))
    })?;

    // pidns uses free_pid (removes global PID mapping from namespace).
    let _ = crate::pidns::free_pid(pid_ns, pid);
    let _ = crate::userns::detach_process(user_ns);
    let _ = crate::netns::detach_process(net_ns);

    // Move the task back to the root cgroup.  `set_task_cgroup` detaches
    // it from the container's group (decrementing that group's task
    // count) and re-points it at the root — the symmetric counterpart of
    // the `set_task_cgroup` in `add_process_task`.
    let _ = crate::sched::set_task_cgroup(task_id, crate::cgroup::ROOT_CGROUP);

    // Reset the task's net_ns to root so any remaining socket operations
    // revert to the host namespace.
    let _ = crate::sched::set_task_net_ns(task_id, crate::netns::ROOT_NS);

    // Drop the filesystem-root jail and volume mounts (keyed on the global
    // PID), symmetric with the `set_root`/`add_volume` calls in
    // `add_process_task`.  Idempotent if the container had no rootfs or
    // volumes configured.
    crate::ipc::namespace::clear_root(pid);
    crate::ipc::namespace::clear_mounts(pid);

    Ok(())
}

/// Launch an init process inside a container and start it running.
///
/// This is the orchestration entry point that turns a `Created`
/// container into a `Running` one — the kernel-side equivalent of
/// `docker run` / `runc start`.  It:
///
/// 1. Verifies the container exists and is in [`Created`](ContainerState::Created)
///    state (a container can only be run once).
/// 2. Spawns the process from `elf_data`.  The new process's initial
///    thread is enqueued but does **not** execute until the scheduler
///    next picks it, so the cgroup/namespace binding in step 3 is
///    guaranteed to be in place before the process runs its first
///    instruction.
/// 3. Binds the process into the container via [`add_process_task`]:
///    cgroup resource billing (Q14), PID-namespace mapping, and the
///    user/network namespaces.  Because the binding uses the spawn
///    result's *task id* for the scheduler resources, the process is
///    correctly charged to the container's cgroup.
/// 4. Records the process as the container's init PID and transitions
///    the container to [`Running`](ContainerState::Running).
///
/// On any failure after the spawn, the just-created process is torn down
/// (threads killed, address space freed) so a failed `run` never leaks
/// an un-billed process.
///
/// Returns the global PID of the launched init process.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist or
///   is not in `Created` state.
/// - Any error from [`spawn_process`](crate::proc::spawn::spawn_process)
///   (invalid ELF, out of memory).
pub fn run(
    id: ContainerId,
    elf_data: &[u8],
    options: &crate::proc::spawn::SpawnOptions<'_>,
) -> KernelResult<u64> {
    // Step 1: container must exist and be freshly created.
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        Ok(())
    })?;

    // Step 2: spawn the init process.  It is enqueued but not yet run.
    let result = crate::proc::spawn::spawn_process(elf_data, options)?;

    // Step 3: bind it into the container (cgroup billing + namespaces),
    // keyed on the spawn result's task id for the scheduler resources.
    if let Err(e) = add_process_task(id, result.pid, result.task_id) {
        // Roll back the spawn so a failed run leaks nothing.
        crate::proc::thread::kill_process_threads(result.pid);
        crate::proc::pcb::destroy(result.pid);
        return Err(e);
    }

    // Step 4: record init PID and flip Created → Running atomically under
    // the table lock.
    with_table(|table| {
        let idx = id as usize;
        if idx < MAX_CONTAINERS && table.containers[idx].active {
            table.containers[idx].init_pid = Some(result.pid);
            table.containers[idx].state = ContainerState::Running;
        }
    });

    serial_println!(
        "[container] run id={} '{}': init pid={} task={} entry={:#x}",
        id,
        info(id).map_or(String::new(), |ci| ci.name),
        result.pid,
        result.task_id,
        result.entry_point
    );

    Ok(result.pid)
}

/// Set a container's filesystem root (rootfs) before it is run.
///
/// `root` is an absolute host path (e.g. the container's extracted/overlay
/// rootfs `/containers/<id>/rootfs`).  Every process subsequently launched
/// by [`run`] (and registered via [`add_process_task`]) is jailed to this
/// root via the per-process chroot in [`crate::ipc::namespace`], so the
/// container's processes resolve `/bin/sh`, `/lib/...`, etc. against their
/// own rootfs rather than the host filesystem.
///
/// Must be called while the container is still in
/// [`Created`](ContainerState::Created) state — changing the root of a
/// already-running container would not retroactively re-jail its live
/// processes, so it is rejected.  Passing an empty string clears the root
/// (processes see the host filesystem).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist, is
///   not in `Created` state, or `root` is non-empty but not an absolute
///   path.
pub fn set_root_path(id: ContainerId, root: &str) -> KernelResult<()> {
    if !root.is_empty() && !root.starts_with('/') {
        return Err(KernelError::InvalidArgument);
    }
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].root_path = String::from(root);
        Ok(())
    })
}

/// Record the VFS mountpoint of the container's overlay rootfs.
///
/// Stored so that [`delete`] can unmount the per-container `OverlayFs`
/// adapter when the container is torn down.  Like [`set_root_path`], this
/// only takes effect for a container still in `Created` state — a running
/// container's mounts are fixed.  Passing an empty string clears the
/// recorded mount (the container then owns no overlay and `delete` will not
/// unmount anything).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist, is
///   not in `Created` state, or `mount` is non-empty but not an absolute
///   path.
pub fn set_rootfs_mount(id: ContainerId, mount: &str) -> KernelResult<()> {
    if !mount.is_empty() && !mount.starts_with('/') {
        return Err(KernelError::InvalidArgument);
    }
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].rootfs_mount = String::from(mount);
        Ok(())
    })
}

/// Add a volume (bind) mount to a container before it is run — the Docker
/// `-v host_target:guest_prefix` mechanism.
///
/// `host_target` is an absolute host path whose contents become visible
/// inside the container at the absolute guest path `guest_prefix`.  Unlike
/// the rootfs (which clamps `..` and re-anchors *every* path), a volume
/// re-anchors only the `guest_prefix` subtree, letting a container share a
/// host directory (e.g. `-v /srv/data:/data`).  `..`-escape is still
/// prevented: the guest path is normalized within the jail before volume
/// matching, so a guest cannot climb out of a volume into the host (see
/// [`crate::ipc::namespace::add_volume`]).
///
/// Must be called while the container is still in
/// [`Created`](ContainerState::Created) state — volumes are installed on the
/// init process at [`run`] time, so adding one to a running container would
/// not affect its live processes.  Re-adding at an existing `guest_prefix`
/// replaces the target (last-writer-wins).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist, is not
///   in `Created` state, either path is not absolute, or `guest_prefix` is
///   the guest root `/` (that is [`set_root_path`]'s job).
pub fn add_volume_mount(
    id: ContainerId,
    host_target: &str,
    guest_prefix: &str,
) -> KernelResult<()> {
    if !host_target.starts_with('/') || !guest_prefix.starts_with('/') {
        return Err(KernelError::InvalidArgument);
    }
    if guest_prefix == "/" {
        return Err(KernelError::InvalidArgument);
    }
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        let vols = &mut table.containers[idx].volumes;
        // Replace an existing volume at the same guest prefix (last-writer-
        // wins), mirroring `namespace::add_volume` semantics.
        if let Some(existing) =
            vols.iter_mut().find(|(g, _)| g == guest_prefix)
        {
            existing.1 = String::from(host_target);
            return Ok(());
        }
        if vols.len() >= MAX_VOLUMES_PER_CONTAINER {
            return Err(KernelError::ResourceExhausted);
        }
        vols.push((String::from(guest_prefix), String::from(host_target)));
        Ok(())
    })
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
            init_pid: ct.init_pid,
            root_path: ct.root_path.clone(),
            rootfs_mount: ct.rootfs_mount.clone(),
            volumes: ct.volumes.clone(),
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

    // Test 16: add_process sets task's net_ns, remove_process resets it.
    {
        let net_cfg2 = ContainerConfig::new("test-net-ns-propagation")
            .network([10, 99, 0, 2], Some([255, 255, 255, 0]), Some([10, 99, 0, 1]), None);
        let ct_ns = create(&net_cfg2).expect("create ns-propagation ct");
        let ci_ns = info(ct_ns).unwrap();

        // The container's net_ns should be non-root.
        assert!(ci_ns.net_ns > 0, "container should have non-root net_ns");

        // Use the current task as a guinea pig.
        let task_id = crate::sched::current_task_id();
        let original_ns = crate::sched::current_task_net_ns();

        // Add the current task to the container — net_ns should propagate.
        add_process(ct_ns, task_id).expect("add_process");
        let after_add = crate::sched::current_task_net_ns();
        assert_eq!(after_add, ci_ns.net_ns,
            "task net_ns should match container's net_ns after add_process");

        // Remove the process — net_ns should revert to ROOT_NS.
        remove_process(ct_ns, task_id).expect("remove_process");
        let after_remove = crate::sched::current_task_net_ns();
        assert_eq!(after_remove, crate::netns::ROOT_NS,
            "task net_ns should revert to ROOT_NS after remove_process");

        // Restore original ns (should already be ROOT_NS but be explicit).
        let _ = crate::sched::set_task_net_ns(task_id, original_ns);

        delete(ct_ns).expect("cleanup ns-propagation ct");
    }
    serial_println!("[container]   Net NS task propagation: OK");

    // Test 17: `run` launches a real init process inside a container and
    // bills it to the container's cgroup (Q14 enforcement end-to-end).
    {
        // A real, compiled userspace ELF — same binary the init path
        // installs as /bin/hello.  We only need it to be a valid loadable
        // ELF; the process is torn down before it ever executes.
        static HELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        let run_cfg = ContainerConfig::new("test-run-ct").memory(4096);
        let ct_run = create(&run_cfg).expect("create run container");
        let cg = cgroup(ct_run).expect("run container cgroup");

        // Before run: Created, no init pid, cgroup empty.
        assert_eq!(info(ct_run).unwrap().state, ContainerState::Created);
        assert!(info(ct_run).unwrap().init_pid.is_none());
        assert_eq!(
            crate::cgroup::stats(cg).map(|s| s.nr_tasks),
            Some(0),
            "fresh container cgroup must have no tasks"
        );

        let opts = crate::proc::spawn::SpawnOptions::new("hello-init");
        let pid = run(ct_run, HELLO_ELF, &opts).expect("run init process");

        // After run: Running, init pid recorded, one tracked process,
        // and exactly one task billed to the container's cgroup.
        let ci = info(ct_run).unwrap();
        assert_eq!(ci.state, ContainerState::Running);
        assert_eq!(ci.init_pid, Some(pid));
        assert_eq!(ci.nr_procs, 1);
        assert_eq!(
            crate::cgroup::stats(cg).map(|s| s.nr_tasks),
            Some(1),
            "container init process must be billed to the container cgroup"
        );

        // Can't run a container twice.
        assert!(run(ct_run, HELLO_ELF, &opts).is_err(),
            "running an already-running container must fail");

        // Tear down the init process.  Detach from the cgroup/namespaces
        // first (while the task is still alive so the count decrements),
        // then kill its threads and free its address space.  Resolve the
        // real initial-thread task id from the process (PID != task id).
        let init_task = crate::proc::pcb::get_threads(pid)
            .and_then(|t| t.first().copied())
            .expect("init process has a thread");
        remove_process_task(ct_run, pid, init_task).expect("detach init process");
        assert_eq!(
            crate::cgroup::stats(cg).map(|s| s.nr_tasks),
            Some(0),
            "cgroup must be empty after detaching the init process"
        );
        crate::proc::thread::kill_process_threads(pid);
        crate::proc::pcb::destroy(pid);

        stop(ct_run).expect("stop run container");
        delete(ct_run).expect("delete run container");
    }
    serial_println!("[container]   Run init process + cgroup billing: OK");

    // Test 18: a container with a configured rootfs jails its init process
    // to that root — the init PID's path resolution is re-anchored under
    // the rootfs and cannot escape it.
    {
        static HELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        let jail_cfg = ContainerConfig::new("test-jail-ct").memory(4096);
        let ct_jail = create(&jail_cfg).expect("create jail container");

        // Configuring the rootfs is only allowed before run.
        set_root_path(ct_jail, "/containers/test-jail/rootfs")
            .expect("set rootfs");
        assert_eq!(
            info(ct_jail).unwrap().root_path,
            "/containers/test-jail/rootfs",
        );
        // Non-absolute rootfs is rejected.
        assert!(set_root_path(ct_jail, "relative").is_err());

        let opts = crate::proc::spawn::SpawnOptions::new("jail-init");
        let pid = run(ct_jail, HELLO_ELF, &opts).expect("run jailed init");

        // The init process resolves paths inside its rootfs.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(pid, "/bin/sh")
                .expect("resolve jailed path"),
            "/containers/test-jail/rootfs/bin/sh",
        );
        // `..` cannot escape the jail.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(pid, "/../../etc/passwd")
                .expect("resolve escape attempt"),
            "/containers/test-jail/rootfs/etc/passwd",
        );

        // Changing the rootfs of a running container is rejected.
        assert!(set_root_path(ct_jail, "/other").is_err());

        // Tear down: remove_process_task must also drop the jail.
        let init_task = crate::proc::pcb::get_threads(pid)
            .and_then(|t| t.first().copied())
            .expect("jailed init has a thread");
        remove_process_task(ct_jail, pid, init_task)
            .expect("detach jailed init");
        assert!(
            crate::ipc::namespace::get_root(pid).is_none(),
            "jail must be cleared after detaching the init process",
        );
        crate::proc::thread::kill_process_threads(pid);
        crate::proc::pcb::destroy(pid);

        stop(ct_jail).expect("stop jail container");
        delete(ct_jail).expect("delete jail container");
    }
    serial_println!("[container]   Rootfs jail (chroot) for init process: OK");

    // Test 19: a container with volume (bind) mounts installs them on its
    // init process, so a guest path under a volume resolves to the host
    // target (escaping the rootfs), while non-volume paths stay jailed.
    {
        static HELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        let vol_cfg = ContainerConfig::new("test-vol-ct").memory(4096);
        let ct_vol = create(&vol_cfg).expect("create vol container");
        set_root_path(ct_vol, "/containers/test-vol/rootfs")
            .expect("set rootfs");
        // Volumes are configurable only before run.
        add_volume_mount(ct_vol, "/srv/data", "/data")
            .expect("add data volume");
        add_volume_mount(ct_vol, "/var/log/test-vol", "/logs")
            .expect("add logs volume");
        // Bad args / guest-root volume are rejected.
        assert!(add_volume_mount(ct_vol, "relative", "/x").is_err());
        assert!(add_volume_mount(ct_vol, "/host", "rel").is_err());
        assert!(add_volume_mount(ct_vol, "/host", "/").is_err());
        // Re-adding at an existing guest prefix replaces, not stacks.
        add_volume_mount(ct_vol, "/srv/data2", "/data")
            .expect("replace data volume");
        assert_eq!(
            info(ct_vol).unwrap().volumes.len(),
            2,
            "re-mount at /data must replace, not add a third volume",
        );

        let opts = crate::proc::spawn::SpawnOptions::new("vol-init");
        let pid = run(ct_vol, HELLO_ELF, &opts).expect("run vol init");

        // Volume path escapes the rootfs to the host target.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(pid, "/data/file.txt")
                .expect("resolve volume path"),
            "/srv/data2/file.txt",
        );
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(pid, "/logs/app.log")
                .expect("resolve logs volume"),
            "/var/log/test-vol/app.log",
        );
        // Non-volume path stays jailed under the rootfs.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(pid, "/bin/sh")
                .expect("resolve non-volume path"),
            "/containers/test-vol/rootfs/bin/sh",
        );
        // `..` cannot climb out of a volume into the host.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(pid, "/data/../escape")
                .expect("resolve escape attempt"),
            "/containers/test-vol/rootfs/escape",
        );
        // Adding a volume to a running container is rejected.
        assert!(add_volume_mount(ct_vol, "/host/x", "/x").is_err());

        // Tear down: remove_process_task must drop the volumes too.
        let init_task = crate::proc::pcb::get_threads(pid)
            .and_then(|t| t.first().copied())
            .expect("vol init has a thread");
        remove_process_task(ct_vol, pid, init_task)
            .expect("detach vol init");
        assert_eq!(
            crate::ipc::namespace::volume_count(pid),
            0,
            "volumes must be cleared after detaching the init process",
        );
        crate::proc::thread::kill_process_threads(pid);
        crate::proc::pcb::destroy(pid);

        stop(ct_vol).expect("stop vol container");
        delete(ct_vol).expect("delete vol container");
    }
    serial_println!("[container]   Volume (bind) mounts for init process: OK");

    // Cleanup.
    stop(ct2).ok(); // may already be stopped
    stop(ct3).ok();
    delete(ct2).expect("cleanup ct2");
    delete(ct3).expect("cleanup ct3");
    delete(ct5).expect("cleanup ct5");
    assert_eq!(active_count(), 0);
    serial_println!("[container]   Cleanup: OK");

    serial_println!("[container] Self-test PASSED (18 tests)");
}
