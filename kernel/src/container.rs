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
// The preempt-aware `crate::sync::Mutex` (NOT raw `spin::Mutex`): the container
// TABLE is contended across many tasks and its critical sections walk/mutate the
// container list, so it must never be held across an involuntary preemption. A
// raw spin lock does not disable preemption, so a holder could be preempted
// mid-critical-section and another task (e.g. a process exiting via
// `notify_init_exit`) would spin on the lock forever while the Ready holder never
// gets scheduled on a single CPU — a holder-preemption deadlock (observed
// 2026-07-15, soak iter03: sys_exit→notify_init_exit spinning on TABLE.lock()
// while the prio-31 holder sat Ready). The tracked Mutex calls preempt_disable on
// acquire, closing that window, and adds lockdep + owner tracking as a bonus.
use crate::sync::Mutex;

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

/// Maximum number of published (forwarded) ports per container — the Docker
/// `-p host:container` mechanism.  A small cap: each entry installs a global
/// host-port NAT rule, and a container rarely publishes more than a handful.
pub const MAX_PUBLISHED_PORTS: usize = 32;

/// Host VFS directory under which per-container tmpfs (in-memory) mountpoints
/// are created.  Each `--tmpfs /guest` mount gets a unique backing mountpoint
/// `<TMPFS_ROOT>/<id>-<index>` where a fresh [`crate::fs::memfs`] is mounted;
/// the container owns these and [`delete`] unmounts + removes them.
const TMPFS_ROOT: &str = "/var/lib/slate/tmpfs";

/// Host VFS directory under which per-container stdout+stderr capture logs are
/// written (Docker's json-file log driver equivalent, minus the JSON framing).
/// Each container's log lives at `<LOG_DIR>/<id>.log`; [`run`] creates the
/// directory tree lazily and redirects the init process's fd 1/2 there, and
/// [`logs`] reads it back.
const LOG_DIR: &str = "/var/log/containers";

/// A container's published-port forward spec: `(proto, host_port,
/// container_port)`.  The Docker `-p host:container[/proto]` mechanism.
pub type PublishedPort = (crate::net::nat::NatProto, u16, u16);

/// A container's volume (bind) mount spec: `(guest_prefix, host_target,
/// read_only)`.  The Docker `-v host_target:guest_prefix[:ro]` mechanism.
/// `read_only == true` makes the mount reject writes (mapped to `EROFS`).
pub type VolumeSpec = (String, String, bool);

/// Snapshot of the data [`run`] needs to install a container's published-port
/// NAT rules, taken under the table lock: `(net_ns, container_ip, ports)`.
type PortInstall = (u32, [u8; 4], Vec<PublishedPort>);

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

/// Health status of a container's healthcheck (Docker `HEALTHCHECK`).
///
/// A container without a configured healthcheck is [`None`](Self::None) and no
/// health is surfaced.  When a healthcheck *is* configured the status begins at
/// [`Starting`](Self::Starting) (Docker's "health: starting") and transitions to
/// [`Healthy`](Self::Healthy)/[`Unhealthy`](Self::Unhealthy) as probes run — see
/// [`apply_probe_result`], which implements Docker's start-period / retry
/// semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthStatus {
    /// No healthcheck configured for this container.
    #[default]
    None,
    /// Healthcheck configured; still in the start period or awaiting the first
    /// passing probe (Docker "health: starting").
    Starting,
    /// The most recent probe passed (Docker "healthy").
    Healthy,
    /// The failing streak reached the retry count (Docker "unhealthy").
    Unhealthy,
}

impl core::fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::None => write!(f, "none"),
            Self::Starting => write!(f, "starting"),
            Self::Healthy => write!(f, "healthy"),
            Self::Unhealthy => write!(f, "unhealthy"),
        }
    }
}

/// Pure state-machine step for a single healthcheck probe result (Docker
/// `HEALTHCHECK` semantics).
///
/// Given the container's current health `status`, its consecutive-failure
/// `streak`, the wall-clock time the container's init started (`started_ns`),
/// the current time (`now_ns`), the healthcheck `cfg`, and the probe's
/// `exit_code` (0 = pass, non-zero or a synthesized timeout code = fail), this
/// returns the container's `(new_status, new_streak)`.
///
/// The rules mirror Docker's `handleProbeResult`:
///
/// * A passing probe resets the streak to 0 and marks the container
///   [`Healthy`](HealthStatus::Healthy).
/// * A failing probe *during the start period* while the container is still
///   [`Starting`](HealthStatus::Starting) does **not** count against the retry
///   budget (Docker's start-period grace — see moby/moby#44348). The container
///   stays `Starting`.
/// * Any other failing probe increments the streak (saturating at the retry
///   count) and, once the streak reaches the effective retry count, marks the
///   container [`Unhealthy`](HealthStatus::Unhealthy).
///
/// This function is pure (no I/O, no locking) so it can be exhaustively unit
/// tested; the live supervisor calls it with real timings and probe exit codes.
#[must_use]
pub fn apply_probe_result(
    status: HealthStatus,
    streak: u32,
    started_ns: u64,
    now_ns: u64,
    cfg: &crate::oci::HealthcheckConfig,
    exit_code: i32,
) -> (HealthStatus, u32) {
    let retries = cfg.effective_retries().max(1);

    if exit_code == 0 {
        // Passing probe: healthy, streak cleared. A pass at any time (even in
        // the start period) promotes the container out of "starting".
        return (HealthStatus::Healthy, 0);
    }

    // Failing probe. During the start period, failures are not counted while
    // the container is still Starting — this gives slow-booting services time
    // to come up without accruing an unhealthy verdict.
    let in_start_period = now_ns.saturating_sub(started_ns) < cfg.start_period_ns;
    if in_start_period && status == HealthStatus::Starting {
        return (HealthStatus::Starting, streak);
    }

    let new_streak = if streak < retries {
        streak.saturating_add(1)
    } else {
        streak
    };

    if new_streak >= retries {
        (HealthStatus::Unhealthy, new_streak)
    } else {
        // Not yet at the retry threshold. Preserve the current status, except
        // that an unconfigured `None` coming in defensively becomes `Starting`.
        let carried = if status == HealthStatus::None {
            HealthStatus::Starting
        } else {
            status
        };
        (carried, new_streak)
    }
}

/// Restart policy for a container's init process (Docker `--restart`).
///
/// Evaluated automatically when the init process exits (see
/// [`notify_init_exit`]): if the policy calls for a restart, the container's
/// recorded launch command is replayed via a deferred workqueue job so the
/// respawn runs in a full task context (the exit path itself must not spawn).
///
/// A *graceful* [`stop`] is treated as a user request and suppresses **all**
/// policies (Docker: "the restart policy is not honored when a container is
/// stopped via `docker stop`"). A force [`kill`] is **not** a user stop, so
/// `Always`/`UnlessStopped`/`OnFailure` still restart after a kill — matching
/// Docker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartPolicy {
    /// Never restart automatically (Docker `no`). The default.
    #[default]
    No,
    /// Restart only if the init exited with a non-zero code (a failure),
    /// up to `max_retries` times. `0` means unlimited retries (Docker
    /// `on-failure` with no count).
    OnFailure(u32),
    /// Always restart the init when it exits, regardless of exit code
    /// (Docker `always`). A graceful `stop` still suppresses it.
    Always,
    /// Like [`Always`](Self::Always), but a user stop is remembered so the
    /// container is not auto-restarted after it (Docker `unless-stopped`).
    /// In our single-session model (no daemon restart to replay across) this
    /// behaves identically to `Always`.
    UnlessStopped,
}

impl core::fmt::Display for RestartPolicy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::No => write!(f, "no"),
            Self::OnFailure(0) => write!(f, "on-failure"),
            Self::OnFailure(n) => write!(f, "on-failure:{n}"),
            Self::Always => write!(f, "always"),
            Self::UnlessStopped => write!(f, "unless-stopped"),
        }
    }
}

/// Parse a Docker `--restart` policy string: `no`, `always`, `unless-stopped`,
/// `on-failure`, or `on-failure:N`. Returns `None` for an unrecognised value.
#[must_use]
pub fn parse_restart_policy(s: &str) -> Option<RestartPolicy> {
    match s {
        "no" | "" => Some(RestartPolicy::No),
        "always" => Some(RestartPolicy::Always),
        "unless-stopped" => Some(RestartPolicy::UnlessStopped),
        "on-failure" => Some(RestartPolicy::OnFailure(0)),
        other => {
            let count = other.strip_prefix("on-failure:")?;
            count.parse::<u32>().ok().map(RestartPolicy::OnFailure)
        }
    }
}

/// Decide whether a container's init exit should trigger an automatic
/// restart, per its [`RestartPolicy`]. Pure function (no I/O, no locks) so it
/// can be unit-tested exhaustively.
///
/// - `exit_code` is the init's recorded exit code (negative = crash).
/// - `user_stopped` is `true` when a graceful [`stop`] requested the halt;
///   it suppresses every policy.
/// - `restart_count` is how many automatic restarts have already happened
///   since the last manual (re)start; it caps `OnFailure(N)`.
#[must_use]
pub fn should_auto_restart(
    policy: RestartPolicy,
    exit_code: i32,
    user_stopped: bool,
    restart_count: u32,
) -> bool {
    if user_stopped {
        return false;
    }
    match policy {
        RestartPolicy::No => false,
        RestartPolicy::Always | RestartPolicy::UnlessStopped => true,
        RestartPolicy::OnFailure(max) => {
            // Only failures (non-zero exit) count, capped at `max` retries
            // (0 = unlimited).
            exit_code != 0 && (max == 0 || restart_count < max)
        }
    }
}

/// Base delay for the auto-restart exponential back-off (100 ms).
const RESTART_BACKOFF_BASE_NS: u64 = 100_000_000;

/// Cap for the auto-restart exponential back-off (30 s).  A container that
/// keeps crashing backs off up to this ceiling rather than restarting faster
/// and faster forever.
const RESTART_BACKOFF_CAP_NS: u64 = 30_000_000_000;

/// Compute the auto-restart back-off delay for the `restart_count`-th
/// consecutive automatic restart (Docker's crash-loop back-off).
///
/// `restart_count` is the (already-incremented) attempt number, so the first
/// auto-restart is `1`.  The delay doubles each consecutive attempt —
/// 100 ms, 200 ms, 400 ms, … — capped at [`RESTART_BACKOFF_CAP_NS`].  This
/// prevents an `always`-policy container that crashes immediately from
/// spinning the CPU in a tight respawn loop.
///
/// Pure and overflow-safe: the shift is clamped and saturated to the cap.
#[must_use]
pub fn restart_backoff_ns(restart_count: u32) -> u64 {
    // Clamp the shift so `checked_shl` never wraps; anything past the cap is
    // pinned to the ceiling regardless.
    let shift = restart_count.saturating_sub(1).min(15);
    RESTART_BACKOFF_BASE_NS
        .checked_shl(shift)
        .unwrap_or(RESTART_BACKOFF_CAP_NS)
        .min(RESTART_BACKOFF_CAP_NS)
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
    /// When `true`, the container's root filesystem is mounted read-only
    /// (Docker `--read-only`): writes that resolve into the rootfs (i.e. not
    /// into a writable volume) are denied with `EROFS`.
    pub read_only_root: bool,
    /// UTS hostname for the container (Docker `--hostname`). Empty means no
    /// override — the container's init process sees the global system
    /// hostname. When set, the init process's `uname(2)`/`gethostname(2)`
    /// report this name instead.
    pub hostname: String,
    /// I/O ops limit per period (0 = unlimited).
    pub io_ops_limit: u64,
    /// I/O bytes limit per period (0 = unlimited).
    pub io_bytes_limit: u64,
    /// Network interface configuration (optional).
    pub net_ip: Option<[u8; 4]>,
    pub net_mask: Option<[u8; 4]>,
    pub net_gateway: Option<[u8; 4]>,
    pub net_dns: Option<[u8; 4]>,
    /// Arbitrary user metadata as `(key, value)` pairs (Docker `--label`).
    /// Labels carry no runtime behavior; they are stored on the container and
    /// surfaced by inspection. Keys are unique — setting an existing key
    /// replaces its value (last-write-wins, matching Docker).
    pub labels: Vec<(String, String)>,
    /// Automatic restart policy for the init process (Docker `--restart`).
    /// Defaults to [`RestartPolicy::No`].
    pub restart_policy: RestartPolicy,
    /// Automatically remove the container when its init process exits (Docker
    /// `--rm`). Mutually exclusive with a restart policy in Docker; here a
    /// restart takes precedence (a container that is going to restart is not
    /// removed). Defaults to `false`.
    pub auto_remove: bool,
    /// The image the container was created from — either an OCI-layout
    /// directory path or a `name:tag` store reference (as passed to `oci run`).
    /// Empty when the container was created directly from a bind rootfs (no
    /// image). Recorded so `oci commit` / `docker commit` can carry the base
    /// image's config and layers forward when authoring a new image from the
    /// container's filesystem changes.
    pub image_source: String,
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

    /// Mark the container's root filesystem as read-only (Docker `--read-only`).
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only_root = read_only;
        self
    }

    /// Set the container's automatic restart policy (Docker `--restart`).
    #[must_use]
    pub fn restart_policy(mut self, policy: RestartPolicy) -> Self {
        self.restart_policy = policy;
        self
    }

    /// Automatically remove the container when its init exits (Docker `--rm`).
    #[must_use]
    pub fn auto_remove(mut self, auto_remove: bool) -> Self {
        self.auto_remove = auto_remove;
        self
    }

    /// Record the image the container was created from (OCI-layout dir path or
    /// `name:tag` store reference), used later by `oci commit`/`docker commit`.
    #[must_use]
    pub fn image_source(mut self, source: &str) -> Self {
        self.image_source = String::from(source);
        self
    }

    /// Set the container's UTS hostname (Docker `--hostname`).
    ///
    /// An empty string leaves the container with the global system hostname.
    /// A name longer than 64 bytes (the UTS field width) is truncated.
    #[must_use]
    pub fn hostname(mut self, name: &str) -> Self {
        let name = if name.len() > 64 { name.get(..64).unwrap_or("") } else { name };
        self.hostname = String::from(name);
        self
    }

    /// Add or replace a metadata label (Docker `--label key=value`).
    ///
    /// If `key` already exists, its value is replaced (last-write-wins,
    /// matching Docker). Empty keys are ignored.
    #[must_use]
    pub fn label(mut self, key: &str, value: &str) -> Self {
        if key.is_empty() {
            return self;
        }
        if let Some(slot) = self.labels.iter_mut().find(|(k, _)| k == key) {
            slot.1 = String::from(value);
        } else {
            self.labels.push((String::from(key), String::from(value)));
        }
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

/// A container's membership in one user-defined network (§60).
///
/// Each membership is a distinct interface into the container's netns: its
/// own veth pair (host end attaches to the network's L2 bridge) and its own
/// IPAM-assigned address. Docker allows a container to be on many networks at
/// once; this generalises the old one-veth-per-container model.
#[derive(Clone)]
struct NetworkMembership {
    /// The user-defined network's name (unique within the container).
    network_name: String,
    /// The veth pair providing this network's interface into the container's
    /// netns. For the create-time primary network this equals the container's
    /// [`Container::veth_pair`]; runtime attachments own a fresh pair.
    veth_pair: crate::net::veth::VethPairId,
    /// The container's IPv4 address on this network.
    ip: [u8; 4],
    /// The network's subnet address (host bits zero) and prefix length, kept so
    /// the connected route can be removed on detach.
    subnet: [u8; 4],
    prefix_len: u8,
}

/// Read-only view of one network membership (for `inspect`/`ps`, §60).
#[derive(Debug, Clone)]
pub struct NetworkAttachment {
    /// The user-defined network's name.
    pub network_name: String,
    /// The veth pair backing this membership's interface.
    pub veth_pair: crate::net::veth::VethPairId,
    /// The container's IPv4 address on this network.
    pub ip: [u8; 4],
}

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
    /// This is the container's *primary* interface (created at [`create`]
    /// time from `ContainerConfig::net_ip`); additional user-defined-network
    /// interfaces attached at runtime live in [`memberships`](Self::memberships).
    veth_pair: Option<crate::net::veth::VethPairId>,
    /// User-defined-network memberships (Docker multi-network parity, §60).
    ///
    /// A container can be attached to N user-defined networks, each with its
    /// own veth interface into the container's netns, its own IPAM address, and
    /// its own embedded-DNS scope. The create-time primary network (if any) is
    /// recorded here too, reusing [`veth_pair`](Self::veth_pair); runtime
    /// `network connect` appends a membership with a fresh veth. Empty when the
    /// container is on no user-defined network.
    memberships: Vec<NetworkMembership>,
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
    /// Overlay id backing this container's copy-on-write rootfs, if one was
    /// created for it. `None` when the container is jailed directly at a plain
    /// (non-overlay) directory. Recorded at run time so introspection that
    /// needs the writable scratch layer — [`diff`] (Docker `diff`) — can locate
    /// the upper layer and whiteouts without relying on the (rename-fragile)
    /// overlay name.
    overlay_id: Option<crate::fs::overlay::OverlayId>,
    /// Volume (bind) mounts as `(guest_prefix, host_target, read_only)`
    /// triples — the Docker `-v host_target:guest_prefix[:ro]` mechanism.
    /// Each is installed on every process launched by [`run`] via
    /// [`crate::ipc::namespace::add_volume`], so a guest path under
    /// `guest_prefix` resolves to `host_target` instead of under the
    /// container rootfs.  A `read_only` volume rejects writes with `EROFS`.
    /// Empty for a container with no volumes.
    volumes: Vec<VolumeSpec>,
    /// VFS mountpoints of this container's tmpfs (in-memory) mounts — the
    /// Docker `--tmpfs /guest` mechanism.  Each entry is a host mountpoint
    /// (under [`TMPFS_ROOT`]) where a fresh [`crate::fs::memfs`] was mounted
    /// at configure time and bind-mounted into the container as a writable
    /// volume at the requested guest path.  Unlike [`volumes`] (whose host
    /// targets the container does not own), these mountpoints ARE owned by the
    /// container: [`delete`] unmounts each and removes its (now-empty)
    /// backing directory, so the tmpfs contents are ephemeral — freed when
    /// the container is removed.  Empty for a container with no tmpfs mounts.
    tmpfs_mounts: Vec<String>,
    /// When `true`, the container's root filesystem is read-only (Docker
    /// `--read-only`): writes resolving into the rootfs are denied with
    /// `EROFS`, while writable (`:rw`) volumes remain writable.  Installed on
    /// each process launched by [`run`] via
    /// [`crate::ipc::namespace::set_root_read_only`].
    read_only_root: bool,
    /// UTS hostname for the container (Docker `--hostname`).  Empty means the
    /// container's processes see the global system hostname.  When set, each
    /// process launched by [`run`] is given this hostname via
    /// [`crate::ipc::namespace::set_hostname`], so `uname(2)`/`gethostname(2)`
    /// inside the container report it.
    hostname: String,
    /// The container's own IPv4 address inside its network namespace, captured
    /// from [`ContainerConfig::net_ip`] at create time.  `None` when no
    /// network was configured.  Needed as the *target* of published-port NAT
    /// rules (`-p host:container` forwards host traffic to `container_ip:port`).
    container_ip: Option<[u8; 4]>,
    /// Published (forwarded) ports as `(proto, host_port, container_port)` —
    /// the Docker `-p host:container[/proto]` mechanism.  Configured while the
    /// container is in `Created` state and installed as host-port NAT rules at
    /// [`run`] time (see [`crate::net::nat::add_port_forward`]).  Flushed on
    /// [`stop`]/[`delete`].  Empty for a container that publishes no ports.
    published_ports: Vec<PublishedPort>,
    /// Arbitrary user metadata `(key, value)` pairs (Docker `--label`).
    /// Captured from [`ContainerConfig::labels`] at create time; carries no
    /// runtime behavior and is surfaced only via [`info`].
    labels: Vec<(String, String)>,
    /// Exit code of the container's init process, recorded automatically when
    /// the init exits and the container transitions to `Stopped` (Docker's
    /// "Exited (N)").  `None` while the container has never stopped (Created
    /// or Running) — once set, it persists until the slot is reused.  A
    /// negative value indicates a crash (negated exception code), mirroring
    /// the kernel's process exit-code convention.
    exit_code: Option<i32>,
    /// Whether the container is frozen (Docker `pause`).  When `true`, all of
    /// the container's threads are suspended and any process subsequently
    /// joined to the container (via [`add_process_task`]) is suspended on
    /// entry, so the whole container's execution is halted until [`unpause`]
    /// thaws it.  A frozen container's state stays `Running` (Docker reports
    /// "paused" as a sub-state of running); freezing is orthogonal to the
    /// Created/Running/Stopped lifecycle.
    frozen: bool,
    /// Host VFS path of the init binary last launched via [`run_path`], saved
    /// so the container can be re-launched (Docker `restart`).  Empty until the
    /// container has been run via [`run_path`] (a bare [`run`] with raw ELF
    /// bytes records no path and so cannot be restarted).
    init_exe_path: String,
    /// Extra arguments (after the binary path) of the init command last
    /// launched via [`run_path`], replayed verbatim on [`restart`].  These
    /// originate from the shell command line (already UTF-8), so they are
    /// stored as `String`s.
    init_args: Vec<String>,
    /// Host VFS path of this container's captured stdout+stderr log (Docker
    /// `logs`).  Set by [`run`] when it redirects the init process's fd 1 and
    /// fd 2 to a capture file before the process first runs; read back by
    /// [`logs`].  Empty until the container has been run (or when the capture
    /// file could not be opened — capture is best-effort and its failure never
    /// blocks the container from starting).  Removed on [`delete`].
    log_path: String,
    /// Automatic restart policy for the init process (Docker `--restart`).
    /// Captured from [`ContainerConfig::restart_policy`] at create time and
    /// evaluated by [`notify_init_exit`] when the init exits.
    restart_policy: RestartPolicy,
    /// Number of automatic restarts performed since the last *manual*
    /// (re)start. Incremented by [`notify_init_exit`] when it schedules an
    /// auto-restart; caps [`RestartPolicy::OnFailure`]; reset to 0 by manual
    /// [`start`]/[`restart`].
    restart_count: u32,
    /// Whether the container was halted by a *graceful* user [`stop`] (as
    /// opposed to a crash or a force [`kill`]). A user stop suppresses every
    /// restart policy (Docker semantics). Cleared on every (re)launch.
    user_stopped: bool,
    /// Automatically delete the container when its init exits (Docker `--rm`).
    /// Captured from [`ContainerConfig::auto_remove`] at create time; honoured
    /// by [`notify_init_exit`] *only* when no restart is scheduled.
    auto_remove: bool,
    /// Monotonic creation sequence (from [`ContainerTable::next_seq`]), used to
    /// order listings by creation time (Docker `ps -n`/`-l`). Slots are reused,
    /// so this — not the slot id — is the true creation order.
    created_seq: u64,
    /// The image the container was created from — an OCI-layout directory path
    /// or a `name:tag` store reference (from [`ContainerConfig::image_source`]).
    /// Empty for a container created from a bind rootfs (no image). Recorded so
    /// `oci commit`/`docker commit` can carry the base image forward.
    image_source: String,
    /// Healthcheck configuration (Docker `HEALTHCHECK`), if the image or the
    /// create request specified one. `None` means no healthcheck — the
    /// container's [`health_status`](Self::health_status) stays
    /// [`HealthStatus::None`] and no probes are scheduled.
    healthcheck: Option<crate::oci::HealthcheckConfig>,
    /// Current health status (Docker's health sub-state of running). Driven by
    /// the healthcheck supervisor via [`apply_probe_result`]; always
    /// [`HealthStatus::None`] when [`healthcheck`](Self::healthcheck) is `None`.
    health_status: HealthStatus,
    /// Consecutive healthcheck failure streak, capped at the effective retry
    /// count. Reset to 0 by any passing probe. Meaningful only while a
    /// healthcheck is configured.
    health_fail_streak: u32,
    /// Wall-clock time (`hrtimer::now_ns`) marking the start of the healthcheck
    /// start-period grace, stamped when a runnable healthcheck is installed via
    /// [`set_healthcheck`]. Used by [`apply_probe_result`] to decide whether a
    /// failing probe falls within the start period.
    health_started_ns: u64,
    /// Global PID of the currently in-flight healthcheck probe process, or
    /// `None` when no probe is running. Set by the supervisor tick when it
    /// launches a probe via [`exec_path`] and cleared when the probe completes
    /// (or is reaped after a timeout kill).
    health_probe_pid: Option<u64>,
    /// Initial-thread task id of the in-flight probe (for
    /// [`remove_process_task`] teardown). Meaningful only while
    /// [`health_probe_pid`](Self::health_probe_pid) is `Some`.
    health_probe_task: u64,
    /// Deadline (`hrtimer::now_ns`) after which the in-flight probe is killed
    /// for exceeding the healthcheck timeout. Meaningful only while a probe is
    /// in flight.
    health_probe_deadline_ns: u64,
    /// Whether the in-flight probe was killed for exceeding its timeout. When
    /// set, the probe's exit is scored as a failure regardless of the exit code
    /// the kill produces.
    health_probe_timed_out: bool,
    /// Wall-clock time (`hrtimer::now_ns`) at which the next probe should be
    /// launched. The supervisor tick launches a probe once `now >= next_due`
    /// and re-arms it to `now + interval` after each probe completes.
    health_next_due_ns: u64,
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
            memberships: Vec::new(),
            pids: Vec::new(),
            init_pid: None,
            root_path: String::new(),
            rootfs_mount: String::new(),
            overlay_id: None,
            volumes: Vec::new(),
            tmpfs_mounts: Vec::new(),
            read_only_root: false,
            hostname: String::new(),
            container_ip: None,
            published_ports: Vec::new(),
            labels: Vec::new(),
            exit_code: None,
            frozen: false,
            init_exe_path: String::new(),
            init_args: Vec::new(),
            log_path: String::new(),
            restart_policy: RestartPolicy::No,
            restart_count: 0,
            user_stopped: false,
            auto_remove: false,
            created_seq: 0,
            image_source: String::new(),
            healthcheck: None,
            health_status: HealthStatus::None,
            health_fail_streak: 0,
            health_started_ns: 0,
            health_probe_pid: None,
            health_probe_task: 0,
            health_probe_deadline_ns: 0,
            health_probe_timed_out: false,
            health_next_due_ns: 0,
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
    /// User-defined-network memberships (Docker multi-network parity, §60).
    /// Empty when the container is attached to no user-defined network.
    pub memberships: Vec<NetworkAttachment>,
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
    /// Volume (bind) mounts as `(guest_prefix, host_target, read_only)`.
    pub volumes: Vec<VolumeSpec>,
    /// Whether the container's root filesystem is read-only (`--read-only`).
    pub read_only_root: bool,
    /// The container's UTS hostname (`--hostname`), or empty if it sees the
    /// global system hostname.
    pub hostname: String,
    /// The container's own IPv4 address, or `None` if no network configured.
    pub container_ip: Option<[u8; 4]>,
    /// Published ports as `(proto, host_port, container_port)`.
    pub published_ports: Vec<PublishedPort>,
    /// Arbitrary user metadata `(key, value)` pairs (Docker `--label`).
    pub labels: Vec<(String, String)>,
    /// Exit code of the container's init process, or `None` if the container
    /// has not stopped yet (Docker's "Exited (N)"). Negative means a crash.
    pub exit_code: Option<i32>,
    /// Whether the container is frozen (Docker `pause`): all its threads are
    /// suspended. A frozen container's `state` stays `Running` — pause is a
    /// sub-state of running, surfaced separately so callers can show "paused".
    pub frozen: bool,
    /// Automatic restart policy for the init process (Docker `--restart`).
    pub restart_policy: RestartPolicy,
    /// Number of automatic restarts performed since the last manual (re)start.
    pub restart_count: u32,
    /// Auto-remove on init exit (Docker `--rm`).
    pub auto_remove: bool,
    /// Monotonic creation sequence (for ordering by creation time).
    pub created_seq: u64,
    /// Current healthcheck status (Docker health sub-state). [`HealthStatus::None`]
    /// when no healthcheck is configured.
    pub health_status: HealthStatus,
    /// Whether a healthcheck is configured for this container.
    pub has_healthcheck: bool,
    /// Consecutive healthcheck failure streak (0 when healthy or no healthcheck).
    pub health_fail_streak: u32,
}

// ---------------------------------------------------------------------------
// Global table
// ---------------------------------------------------------------------------

struct ContainerTable {
    containers: Vec<Container>,
    next_id: u32,
    /// Monotonic creation counter, stamped onto each container at [`create`]
    /// time so listings can order by *creation* rather than slot index (slots
    /// are reused, so id order is not creation order). Never wraps in practice
    /// (u64 at container-create rates).
    next_seq: u64,
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
            next_seq: 0,
        }
    }
}

static TABLE: Mutex<Option<ContainerTable>> = Mutex::named(None, b"container-tbl");

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
// Lifecycle event log (Docker `container events`)
// ---------------------------------------------------------------------------

/// A container lifecycle event kind, mirroring Docker's event actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerEventKind {
    /// Container created (`create`).
    Create,
    /// Container init started (`start`).
    Start,
    /// Init process exited (`die`), carrying the exit code.
    Die,
    /// Graceful stop request (`stop`).
    Stop,
    /// Forced kill (`kill`).
    Kill,
    /// Execution paused (`pause`).
    Pause,
    /// Execution resumed (`unpause`).
    Unpause,
    /// Container relaunched (`restart`) — manual restart or auto-restart.
    Restart,
    /// Container deleted (`destroy`).
    Destroy,
}

impl ContainerEventKind {
    /// Docker's lowercase action string for this event.
    #[must_use]
    pub fn action(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Start => "start",
            Self::Die => "die",
            Self::Stop => "stop",
            Self::Kill => "kill",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Restart => "restart",
            Self::Destroy => "destroy",
        }
    }
}

impl core::fmt::Display for ContainerEventKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.action())
    }
}

/// A recorded container lifecycle event (a snapshot copy handed to callers).
#[derive(Debug, Clone)]
pub struct ContainerEvent {
    /// Monotonic per-boot event sequence number (1-based).
    pub seq: u64,
    /// Monotonic timestamp (nanoseconds since boot) from `hrtimer::now_ns`.
    pub time_ns: u64,
    /// Container id the event refers to.
    pub id: ContainerId,
    /// Container name at the time of the event.
    pub name: String,
    /// Event kind.
    pub kind: ContainerEventKind,
    /// Exit code for `die` events (`None` for all other kinds).
    pub exit_code: Option<i32>,
}

/// Capacity of the lifecycle-event ring buffer.  Older events are dropped
/// once this many newer events have accumulated — `container events` shows a
/// bounded recent window, never an unbounded history.
const EVENT_LOG_CAP: usize = 256;

struct EventLog {
    /// Ring buffer of recent events (front = oldest, back = newest).
    events: alloc::collections::VecDeque<ContainerEvent>,
    /// Monotonic sequence counter; the next event recorded gets `next_seq`.
    next_seq: u64,
}

impl EventLog {
    const fn new() -> Self {
        Self {
            events: alloc::collections::VecDeque::new(),
            next_seq: 1,
        }
    }
}

static EVENT_LOG: Mutex<EventLog> = Mutex::named(EventLog::new(), b"container-evt");

/// Record a container lifecycle event.
///
/// Cheap and lock-local: takes only the event-log lock (never the container
/// table), so it is safe to call from within a `with_table` closure or from
/// the process-exit path.  Drops the oldest event when the ring is full.
fn record_event(
    id: ContainerId,
    name: &str,
    kind: ContainerEventKind,
    exit_code: Option<i32>,
) {
    let time_ns = crate::hrtimer::now_ns();
    let mut log = EVENT_LOG.lock();
    let seq = log.next_seq;
    log.next_seq = log.next_seq.saturating_add(1);
    if log.events.len() >= EVENT_LOG_CAP {
        log.events.pop_front();
    }
    log.events.push_back(ContainerEvent {
        seq,
        time_ns,
        id,
        name: String::from(name),
        kind,
        exit_code,
    });
}

/// Snapshot recent lifecycle events, oldest first.
///
/// - `since_seq`: only return events with `seq > since_seq` (pass 0 for all
///   retained events).
/// - `limit`: cap the result to the most recent `limit` matching events (pass
///   0 for no cap).
/// - `filter_id`: when `Some`, only events for that container id.
///
/// Returns owned copies so the caller never holds the event-log lock while
/// formatting output.
#[must_use]
pub fn events_snapshot(
    since_seq: u64,
    limit: usize,
    filter_id: Option<ContainerId>,
) -> Vec<ContainerEvent> {
    let log = EVENT_LOG.lock();
    let mut out: Vec<ContainerEvent> = log
        .events
        .iter()
        .filter(|e| e.seq > since_seq)
        .filter(|e| filter_id.is_none_or(|fid| e.id == fid))
        .cloned()
        .collect();
    if limit != 0 && out.len() > limit {
        let drop = out.len().saturating_sub(limit);
        out.drain(0..drop);
    }
    out
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

    // Activate this namespace's ARP cache so neighbor learning/resolution on
    // the container's user-defined network is isolated from the host and from
    // other containers (rather than sharing the global cache, which could
    // collide when two container networks reuse the same subnet/IP).
    if let Err(e) = crate::net::arp::ns_init(net_ns) {
        let _ = veth::set_up(pair_id, VethEndId::A, false);
        let _ = veth::set_up(pair_id, VethEndId::B, false);
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
        ct.memberships.clear();
        ct.pids.clear();
        // Record the container's own IP so published-port NAT rules know
        // where to forward (the `-p host:container` target).
        ct.container_ip = config.net_ip;
        ct.read_only_root = config.read_only_root;
        ct.hostname = config.hostname.clone();
        ct.labels = config.labels.clone();
        ct.published_ports.clear();
        // Fresh container has not exited yet (clears any stale value from a
        // reused slot).
        ct.exit_code = None;
        // A fresh container is never frozen (clears a stale flag on reuse).
        ct.frozen = false;
        // A fresh container has no recorded launch spec (clears stale values
        // from a reused slot).
        ct.init_exe_path.clear();
        ct.init_args.clear();
        // Restart policy from the config; a fresh container has done no
        // auto-restarts and was not user-stopped (clears stale values on
        // slot reuse).
        ct.restart_policy = config.restart_policy;
        ct.restart_count = 0;
        ct.user_stopped = false;
        ct.auto_remove = config.auto_remove;
        ct.image_source = config.image_source.clone();
        // Stamp the creation sequence so listings can order by creation time.
        ct.created_seq = table.next_seq;
        table.next_seq = table.next_seq.saturating_add(1);

        #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
        {
            table.next_id = ((slot + 1) % MAX_CONTAINERS) as u32;
        }
    });

    serial_println!(
        "[container] Created '{}' (id={}, pidns={}, userns={}, netns={}, cgroup={}, veth={:?})",
        config.name, slot, pid_ns, user_ns, net_ns, cgroup_id, veth_pair
    );

    let new_id = slot as ContainerId;
    record_event(new_id, &config.name, ContainerEventKind::Create, None);
    Ok(new_id)
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
    let name = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].state = ContainerState::Running;
        // A manual start is a fresh run of the restart-policy clock: clear any
        // remembered user stop and reset the auto-restart counter.
        table.containers[idx].user_stopped = false;
        table.containers[idx].restart_count = 0;
        Ok(table.containers[idx].name.clone())
    })?;
    record_event(id, &name, ContainerEventKind::Start, None);
    Ok(())
}

/// Mark a container as stopped.
///
/// Called when the init process exits.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if container doesn't exist.
pub fn stop(id: ContainerId) -> KernelResult<()> {
    let (net_ns, name) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].state = ContainerState::Stopped;
        // A graceful stop is a user request: remember it so the restart policy
        // is suppressed (Docker: restart is not honored after `docker stop`).
        table.containers[idx].user_stopped = true;
        Ok((table.containers[idx].net_ns, table.containers[idx].name.clone()))
    })?;
    // A stopped container publishes no ports (Docker semantics): tear down its
    // host-port NAT forwards so a dead container can't keep receiving traffic.
    // The publish intents stay recorded in `published_ports`, so a future
    // restart could reinstall them.  Done outside the table lock (the NAT
    // table has its own lock).  Idempotent if the container had no forwards.
    crate::net::nat::flush_port_forwards(net_ns);
    record_event(id, &name, ContainerEventKind::Stop, None);
    Ok(())
}

/// Notify the container layer that process `pid` has exited.
///
/// If `pid` is the init process of a `Running` container, that container
/// transitions to [`Stopped`](ContainerState::Stopped), records `exit_code`
/// (Docker's "Exited (N)"; negative means a crash, per the kernel's process
/// exit-code convention), and tears down its published-port NAT forwards —
/// Docker semantics: *a container lives as long as its init process*. This is
/// the automatic counterpart to the manual [`stop`]; it is called from the
/// process-exit (zombie-transition) path so a container's state tracks its
/// init process without an explicit `stop`.
///
/// Idempotent and cheap: a no-op when `pid` is not a `Running` container's
/// init process (including already-stopped containers and ordinary
/// non-container processes). Takes no locks other than the container table
/// (and the NAT table), so it must be called with no incompatible locks held
/// — in particular **not** while holding `PROCESS_TABLE`.
pub fn notify_init_exit(pid: u64, exit_code: i32) {
    // This runs from the generic process-exit path, which can fire during
    // early boot before `container::init()` has populated the table (e.g. a
    // bootstrap thread exiting). Access the table directly and treat an
    // uninitialized table as "no containers" rather than panicking.
    let mut net_ns_to_flush = None;
    let mut restart_id: Option<ContainerId> = None;
    let mut restart_attempt: u32 = 0;
    let mut remove_id: Option<ContainerId> = None;
    let mut die_event: Option<(ContainerId, String)> = None;
    {
        let mut guard = TABLE.lock();
        let Some(table) = guard.as_mut() else {
            return;
        };
        for (idx, ct) in table.containers.iter_mut().enumerate() {
            if ct.active
                && ct.state == ContainerState::Running
                && ct.init_pid == Some(pid)
            {
                ct.state = ContainerState::Stopped;
                // Record the init's exit code (Docker's "Exited (N)").  A
                // negative value indicates a crash (negated exception code).
                ct.exit_code = Some(exit_code);
                net_ns_to_flush = Some(ct.net_ns);
                if let Ok(cid) = ContainerId::try_from(idx) {
                    die_event = Some((cid, ct.name.clone()));
                }
                // Evaluate the restart policy.  When it calls for a restart we
                // *schedule* it (below, via the workqueue) rather than spawning
                // here — the process-exit path must not spawn (it can hold
                // scheduler state and cannot safely allocate an address space).
                if should_auto_restart(
                    ct.restart_policy,
                    exit_code,
                    ct.user_stopped,
                    ct.restart_count,
                ) {
                    ct.restart_count = ct.restart_count.saturating_add(1);
                    restart_attempt = ct.restart_count;
                    restart_id = ContainerId::try_from(idx).ok();
                } else if ct.auto_remove {
                    // Docker `--rm`: delete the container once its init exits,
                    // but only when it is *not* going to restart (a restart
                    // takes precedence).  Deletion touches the VFS/overlay and
                    // must run in task context, so it is deferred like restart.
                    remove_id = ContainerId::try_from(idx).ok();
                }
                break;
            }
        }
    }
    // Outside the table lock: record the `die` event (Docker emits `die` with
    // the exit code when a container's init exits) and tear down the dead
    // container's host-port NAT forwards (idempotent if it had none), mirroring
    // `stop`.
    if let Some((cid, name)) = die_event {
        record_event(cid, &name, ContainerEventKind::Die, Some(exit_code));
    }
    if let Some(net_ns) = net_ns_to_flush {
        crate::net::nat::flush_port_forwards(net_ns);
    }
    // Schedule the deferred restart with an exponential crash-loop back-off
    // (100 ms, 200 ms, 400 ms, … capped at 30 s), so an `always`-policy
    // container that crashes immediately can't spin the CPU in a tight respawn
    // loop.  The hrtimer fires in ISR context and hands off to the kworker task
    // (where spawning is safe) via the back-off trampoline.  Spawning inline on
    // the exit path is unsafe (it can hold scheduler state and cannot allocate
    // an address space), hence the timer→workqueue two-step.
    if let Some(id) = restart_id {
        let delay_ns = restart_backoff_ns(restart_attempt);
        serial_println!(
            "[container] auto-restart of id={} scheduled in {} ms (attempt {})",
            id,
            delay_ns / 1_000_000,
            restart_attempt
        );
        // schedule_ns returns a one-shot handle we don't need to retain.
        let _ = crate::hrtimer::schedule_ns(delay_ns, restart_backoff_fire, u64::from(id));
    }
    // Schedule the deferred auto-remove (Docker `--rm`).  Deletion requires the
    // container to be non-Running; it is Stopped by the block above, so the
    // reaper will succeed.  A full queue leaves the container Stopped (logged).
    if let Some(id) = remove_id {
        if !crate::workqueue::submit(do_container_autoremove, u64::from(id)) {
            serial_println!(
                "[container] auto-remove of id={} dropped (workqueue full)", id
            );
        }
    }
}

/// Workqueue callback: perform a deferred automatic restart of container `arg`.
///
/// Runs in the `kworker` task context (full privileges — may spawn, allocate,
/// take locks), scheduled by [`notify_init_exit`] when a container's restart
/// policy fires.  Replays the recorded launch command *without* resetting the
/// auto-restart counter, so an `on-failure:N` policy still terminates after
/// `N` attempts.  Errors (the binary vanished, OOM, the container was deleted
/// meanwhile) are logged and dropped — a failed auto-restart simply leaves the
/// container stopped.
/// Timer callback: the crash-loop back-off delay for container `arg` elapsed.
///
/// Runs in the hrtimer ISR context, so it does the minimum — hands the actual
/// relaunch off to the `kworker` task (where spawning is safe) via the
/// workqueue.  A full queue drops the restart (logged); the container stays
/// stopped rather than blocking the ISR.
fn restart_backoff_fire(arg: u64) {
    if !crate::workqueue::submit(do_container_restart, arg) {
        serial_println!(
            "[container] delayed auto-restart of id={} dropped (workqueue full)", arg
        );
    }
}

fn do_container_restart(arg: u64) {
    let Ok(id) = ContainerId::try_from(arg) else {
        return;
    };
    match relaunch_recorded(id, false) {
        Ok(pid) => serial_println!(
            "[container] auto-restarted id={}: new init pid={}", id, pid
        ),
        Err(e) => serial_println!(
            "[container] auto-restart of id={} failed: {:?}", id, e
        ),
    }
}

/// Workqueue callback: perform a deferred automatic removal of container `arg`
/// (Docker `--rm`).
///
/// Runs in the `kworker` task context (may take locks and touch the VFS/overlay
/// during teardown), scheduled by [`notify_init_exit`] when an `--rm` container's
/// init exits and no restart is due.  The container is `Stopped` by that point,
/// so [`delete`] succeeds.  Errors (already deleted, teardown failure) are
/// logged and dropped.
fn do_container_autoremove(arg: u64) {
    let Ok(id) = ContainerId::try_from(arg) else {
        return;
    };
    match delete(id) {
        Ok(()) => serial_println!("[container] auto-removed id={} (--rm)", id),
        Err(e) => serial_println!(
            "[container] auto-remove of id={} failed: {:?}", id, e
        ),
    }
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
    let (pid_ns, user_ns, net_ns, cgroup_id, veth_pairs, name, rootfs_mount, tmpfs_mounts, log_path) =
        with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state == ContainerState::Running {
            return Err(KernelError::InvalidArgument);
        }

        let ct = &table.containers[idx];
        // Collect every veth to destroy: the primary interface plus each
        // user-defined-network membership's interface, de-duplicated (the
        // create-time primary network's membership reuses `veth_pair`, so we
        // must not destroy the same pair twice).
        let mut veth_pairs: Vec<crate::net::veth::VethPairId> = Vec::new();
        if let Some(vp) = ct.veth_pair {
            veth_pairs.push(vp);
        }
        for m in &ct.memberships {
            if !veth_pairs.contains(&m.veth_pair) {
                veth_pairs.push(m.veth_pair);
            }
        }
        let result = (ct.pid_ns, ct.user_ns, ct.net_ns, ct.cgroup_id,
                      veth_pairs, ct.name.clone(), ct.rootfs_mount.clone(),
                      ct.tmpfs_mounts.clone(),
                      ct.log_path.clone());

        // Mark slot as inactive.
        table.containers[idx].active = false;
        table.containers[idx].name.clear();
        table.containers[idx].veth_pair = None;
        table.containers[idx].memberships.clear();
        table.containers[idx].pids.clear();
        table.containers[idx].init_pid = None;
        table.containers[idx].root_path.clear();
        table.containers[idx].rootfs_mount.clear();
        table.containers[idx].volumes.clear();
        table.containers[idx].tmpfs_mounts.clear();
        table.containers[idx].container_ip = None;
        table.containers[idx].published_ports.clear();
        table.containers[idx].log_path.clear();

        Ok(result)
    })?;

    // Clean up sub-resources outside the table lock (each has its own lock).
    // Ignore errors — the sub-resources may have already been cleaned up
    // if a partial failure occurred during create.
    //
    // Destroy veth pairs first (before netns) since the endpoints live
    // in the namespace. This covers the primary interface and every
    // user-defined-network membership (§60).
    for pair_id in veth_pairs {
        let _ = crate::net::veth::destroy_pair(pair_id);
    }
    // Tear down this namespace's ARP cache (idempotent; no-op if never
    // initialized, e.g. a container created without networking).
    crate::net::arp::ns_destroy(net_ns);
    // Flush NAT entries and port-forward rules before tearing down namespace.
    crate::net::nat::flush_namespace(net_ns);
    crate::net::nat::flush_port_forwards(net_ns);
    // Release any user-defined-network IP leases owned by this container so the
    // address returns to its network's IPAM pool (Docker frees a container's
    // network endpoints on removal). Idempotent: a container with no `--network`
    // lease owns nothing, so this is a no-op. `release_container` takes its own
    // lock, so it runs outside the container table lock.
    let _ = crate::cnetwork::release_container(id);
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

    // Release each tmpfs (in-memory) mount the container owns: unmount the
    // memfs (freeing its RAM) and remove the now-empty backing mountpoint dir.
    // Done outside the table lock (VFS has its own per-mount locking). A
    // container with no `--tmpfs` mounts has an empty list, so this is a no-op.
    for mount in &tmpfs_mounts {
        let _ = crate::fs::Vfs::unmount(mount);
        let _ = crate::fs::vfs::Vfs::remove_recursive(mount);
    }

    // Remove the container's captured stdout+stderr log, if it had one.  Done
    // outside the table lock; a missing file (never run, or capture skipped) is
    // fine to ignore.
    if !log_path.is_empty() {
        let _ = crate::fs::vfs::Vfs::remove(&log_path);
    }

    serial_println!("[container] Deleted '{}' (id={})", name, id);

    record_event(id, &name, ContainerEventKind::Destroy, None);
    Ok(())
}

/// Force-remove a container, even if it is running (Docker `rm -f`).
///
/// If the container is [`Running`](ContainerState::Running), its processes are
/// killed and it is transitioned to `Stopped` first, then it is deleted exactly
/// as [`delete`] would.  A non-running container is deleted directly.  This is
/// the only one-step path that can remove a running container — plain [`delete`]
/// deliberately refuses to, to avoid yanking a live container out from under
/// its processes by accident.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if the id is invalid/inactive.
/// - Any error propagated from [`delete`] after the kill/stop.
pub fn force_delete(id: ContainerId) -> KernelResult<()> {
    // Snapshot whether the container is running under the table lock; perform
    // the kill/stop transition outside it, since kill()/stop() re-take the
    // table lock (and kill() also touches the scheduler/process tables).
    let running = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].state == ContainerState::Running)
    })?;
    if running {
        // Best-effort termination: if the processes already exited or stop()
        // races a self-exit, delete() below still runs and reports the result.
        let _ = kill(id);
        let _ = stop(id);
    }
    delete(id)
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
    let (pid_ns, user_ns, net_ns, cgroup_id, root_path, volumes, read_only_root, hostname, frozen) =
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
                table.containers[idx].read_only_root,
                table.containers[idx].hostname.clone(),
                table.containers[idx].frozen,
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
    for (guest_prefix, host_target, read_only) in &volumes {
        let _ = crate::ipc::namespace::add_volume(
            pid, guest_prefix, host_target, *read_only,
        );
    }

    // Apply the read-only-root flag (Docker `--read-only`).  Only meaningful
    // for a jailed process: without a chroot root there is no container rootfs
    // to make read-only, so skip it when `root_path` is empty (matching the
    // `set_root` gate above).  Writable (`:rw`) volumes installed just above
    // still permit writes through the read-only rootfs.
    if read_only_root && !root_path.is_empty() {
        crate::ipc::namespace::set_root_read_only(pid, true);
    }

    // Apply the container's UTS hostname (Docker `--hostname`), if set.  Unlike
    // the chroot/volume/read-only state this is independent of the rootfs jail
    // (a container can override its hostname without a rootfs), so it is keyed
    // only on a non-empty hostname.  A malformed name (rejected by
    // `set_hostname`) is skipped — the value is validated at config time, so
    // this is purely defensive.
    if !hostname.is_empty() {
        let _ = crate::ipc::namespace::set_hostname(pid, &hostname);
    }

    // If the container is currently frozen (Docker `pause`), suspend the newly
    // joined thread immediately so it cannot run while the rest of the
    // container is halted.  Done outside the table lock (the scheduler has its
    // own lock; never hold the container lock across a scheduler call).  The
    // task stays Suspended until `unpause` resumes the whole container.
    if frozen {
        let _ = crate::sched::suspend(task_id);
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

    // Drop the per-process UTS hostname override (keyed on the global PID),
    // symmetric with the `set_hostname` call in `add_process_task`.
    // Idempotent if the container had no hostname configured.
    crate::ipc::namespace::clear_hostname(pid);

    Ok(())
}

/// Build the host VFS path of a container's stdout+stderr capture log:
/// `<LOG_DIR>/<id>.log`.
fn log_path_for(id: ContainerId) -> String {
    alloc::format!("{LOG_DIR}/{id}.log")
}

/// Redirect a freshly-spawned container init process's stdout (fd 1) and stderr
/// (fd 2) to a per-container capture file, returning the host VFS path of that
/// file on success.
///
/// The process must not yet have executed — [`run`] calls this while the child
/// is merely enqueued, so the redirect is in place before its first write.  The
/// log directory is created lazily; the capture file is truncated so each run
/// starts with a fresh log.  fd 1 is pointed at the file, then fd 2 is `dup2`'d
/// onto fd 1 so stdout and stderr share one handle (and thus one append
/// position — writes interleave in order rather than overwriting each other).
///
/// Returns `None` (capture skipped, non-fatal) if the log file cannot be opened
/// or fd 1 cannot be redirected; the container still runs, just without a log.
fn redirect_output_to_capture(id: ContainerId, pid: u64) -> Option<String> {
    use crate::fs::handle;
    use crate::proc::linux_fd::{FdEntry, O_WRONLY};
    use crate::proc::pcb;

    // Create the log directory tree (idempotent) and derive the log path.
    ensure_dir_path("", LOG_DIR.trim_start_matches('/'));
    let path = log_path_for(id);

    // Open (create + truncate) the capture file.  On failure, skip capture.
    let capture_handle = match handle::open(
        &path,
        handle::OpenFlags::READ
            .union(handle::OpenFlags::WRITE)
            .union(handle::OpenFlags::CREATE)
            .union(handle::OpenFlags::TRUNCATE),
    ) {
        Ok(h) => h,
        Err(e) => {
            serial_println!(
                "[container] run id={}: log capture disabled (open {} failed: {:?})",
                id, path, e
            );
            return None;
        }
    };

    // Redirect fd 1 → capture file.  Drop the existing fd-1 entry first (a
    // Console entry owns no kernel resource, so no close is needed).  On
    // install failure ownership never transferred, so we close the handle and
    // skip capture.
    let _ = pcb::linux_fd_take(pid, 1);
    if let Err(e) =
        pcb::linux_fd_install_at(pid, 1, FdEntry::file(capture_handle, O_WRONLY))
    {
        let _ = handle::close(capture_handle);
        serial_println!(
            "[container] run id={}: log capture disabled (redirect fd 1 failed: {:?})",
            id, e
        );
        return None;
    }

    // Point fd 2 at the same handle via dup2 so stderr shares stdout's log.
    // dup2 returns any entry it displaced from fd 2 (the Console stub); it owns
    // no resource, so dropping it is sufficient.  A dup2 failure is non-fatal:
    // stdout is still captured; stderr just keeps its own (console) fd.
    match pcb::linux_fd_dup2(pid, 1, 2) {
        Ok((_newfd, _displaced)) => {}
        Err(e) => serial_println!(
            "[container] run id={}: stderr not captured (dup2 fd 2 failed: {:?})",
            id, e
        ),
    }

    Some(path)
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
    // Auto-detect the init binary's ABI (the default for real containers:
    // Docker images carry glibc/Linux ELFs, which `spawn_process` classifies
    // as Linux from their markers).
    run_with_abi(id, elf_data, options, None)
}

/// [`run`], but with an explicit ABI override for the init process instead of
/// auto-detecting it from the ELF markers.
///
/// `abi` is passed straight through to
/// [`spawn_process_with_abi`](crate::proc::spawn::spawn_process_with_abi) when
/// `Some`; `None` auto-detects (the [`run`] default).  The override exists so
/// callers that already know the binary's ABI — and the container self-test,
/// which needs a Linux-ABI init to exercise the `logs` capture path with the
/// embedded (natively-marked) test ELF — can state it explicitly rather than
/// relying on the marker heuristic.
///
/// # Errors
/// Same as [`run`].
fn run_with_abi(
    id: ContainerId,
    elf_data: &[u8],
    options: &crate::proc::spawn::SpawnOptions<'_>,
    abi: Option<crate::proc::pcb::AbiMode>,
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
    let result = match abi {
        Some(m) => crate::proc::spawn::spawn_process_with_abi(elf_data, options, m)?,
        None => crate::proc::spawn::spawn_process(elf_data, options)?,
    };

    // Step 3: bind it into the container (cgroup billing + namespaces),
    // keyed on the spawn result's task id for the scheduler resources.
    if let Err(e) = add_process_task(id, result.pid, result.task_id) {
        // Roll back the spawn so a failed run leaks nothing.
        crate::proc::thread::kill_process_threads(result.pid);
        crate::proc::pcb::destroy(result.pid);
        return Err(e);
    }

    // Step 3.5: redirect the init process's stdout+stderr (fd 1 and fd 2) to a
    // per-container capture file so `container logs` can read them back.  This
    // runs while the child is still merely enqueued (it does not execute until
    // the scheduler next picks it — see the doc comment above), so the redirect
    // is guaranteed in place before the process writes its first byte.  Capture
    // is best-effort: if the log file cannot be opened or the fds cannot be
    // redirected, the container still runs (just without a captured log),
    // mirroring the non-fatal stdio install in `spawn_process`.
    let captured_log_path = redirect_output_to_capture(id, result.pid);

    // Step 4: record init PID and flip Created → Running atomically under
    // the table lock.  Snapshot the network namespace, container IP, and
    // published ports for step 5 while we hold the lock.
    let port_install: Option<PortInstall> =
        with_table(|table| {
            let idx = id as usize;
            if idx >= MAX_CONTAINERS || !table.containers[idx].active {
                return None;
            }
            table.containers[idx].init_pid = Some(result.pid);
            table.containers[idx].state = ContainerState::Running;
            // A launched container is, by definition, not user-stopped; clear
            // the flag so its restart policy is armed for the new init. (The
            // auto-restart counter is *not* reset here — that would let an
            // `on-failure:N` container loop forever; it is reset only by a
            // manual start/restart.)
            table.containers[idx].user_stopped = false;
            // Record the capture-log path (if the redirect above succeeded) so
            // `logs(id)` knows where to read from.  Empty when capture was
            // skipped.
            table.containers[idx].log_path.clear();
            if let Some(ref p) = captured_log_path {
                table.containers[idx].log_path.push_str(p);
            }
            // Only install port forwards when the container has both an IP
            // (forward target) and at least one published port.
            match table.containers[idx].container_ip {
                Some(ip) if !table.containers[idx].published_ports.is_empty() => Some((
                    table.containers[idx].net_ns,
                    ip,
                    table.containers[idx].published_ports.clone(),
                )),
                _ => None,
            }
        });

    // Step 5: install the container's published-port NAT rules (the
    // `-p host:container` forwards).  Done after the state flip and outside
    // the table lock (the NAT table has its own lock).  Best-effort per rule:
    // a duplicate host port (already claimed by another container) is logged
    // and skipped rather than failing the whole run — the container still
    // starts, just without that one forward.  All rules are flushed when the
    // container stops or is deleted (`flush_port_forwards`).
    if let Some((net_ns, ip, ports)) = port_install {
        let container_ip =
            crate::net::interface::Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]);
        for (proto, host_port, container_port) in ports {
            match crate::net::nat::add_port_forward(
                proto, host_port, container_ip, container_port, net_ns,
            ) {
                Ok(()) => serial_println!(
                    "[container] run id={}: published {:?} :{} -> {}:{}",
                    id, proto, host_port, container_ip, container_port
                ),
                Err(e) => serial_println!(
                    "[container] run id={}: WARNING could not publish {:?} :{} -> :{}: {:?}",
                    id, proto, host_port, container_port, e
                ),
            }
        }
    }

    let run_name = info(id).map_or(String::new(), |ci| ci.name);
    serial_println!(
        "[container] run id={} '{}': init pid={} task={} entry={:#x}",
        id,
        run_name,
        result.pid,
        result.task_id,
        result.entry_point
    );

    // `run` flips the container to `Running` directly (without going through
    // the public `start`), so emit the `start` lifecycle event here.
    record_event(id, &run_name, ContainerEventKind::Start, None);
    Ok(result.pid)
}

/// Read back a container's captured stdout+stderr log (Docker `logs`).
///
/// Returns the raw bytes written to fd 1/2 by the container's init process
/// since it was last launched by [`run`] (the capture file is truncated on each
/// run, so this reflects the current run only).  Output is returned as bytes —
/// container programs may emit arbitrary (non-UTF-8) data, and OS-boundary data
/// is never forced through UTF-8.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist.
/// - [`KernelError::NotFound`] if the container has no capture log — it was
///   never run, or capture was skipped because the log file could not be
///   opened at run time.
/// - Any VFS error encountered reading the log file back.
pub fn logs(id: ContainerId) -> KernelResult<Vec<u8>> {
    // Snapshot the log path under the table lock, then read the file outside it
    // (VFS reads must not run while holding the container table lock).
    let path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].log_path.clone())
    })?;

    if path.is_empty() {
        return Err(KernelError::NotFound);
    }

    crate::fs::vfs::Vfs::read_file(&path)
}

/// Launch a container's init process from a host VFS path, recording the
/// launch spec so the container can later be restarted (Docker `restart`).
///
/// This is the path-based counterpart to [`run`]: it reads the ELF image from
/// the host VFS at `vfs_path`, builds the argv (`argv[0]` = `vfs_path`, then
/// `extra_args`), spawns the init process via [`run`], and — on success —
/// stores `vfs_path` and `extra_args` on the container so [`restart`] can
/// replay the exact same command.  Callers that already have ELF bytes in hand
/// and do not need restartability can use [`run`] directly.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist, is not in
///   `Created` state, or `vfs_path` cannot be read from the VFS.
/// - Any error from [`run`]/`spawn_process` (invalid ELF, out of memory).
pub fn run_path(
    id: ContainerId,
    vfs_path: &str,
    extra_args: &[&str],
) -> KernelResult<u64> {
    // Read the ELF image from the host VFS.  A read failure (missing file,
    // permission, I/O) maps to InvalidArgument — the caller passed a bad path.
    let elf = crate::fs::vfs::Vfs::read_file(vfs_path)
        .map_err(|_| KernelError::InvalidArgument)?;

    // Build argv: argv[0] is the binary path, then the extra args.  Own the
    // byte buffers, then borrow them for SpawnOptions.
    let argv_owned: Vec<Vec<u8>> = core::iter::once(vfs_path.as_bytes().to_vec())
        .chain(extra_args.iter().map(|s| s.as_bytes().to_vec()))
        .collect();
    let argv_refs: Vec<&[u8]> = argv_owned.iter().map(Vec::as_slice).collect();

    let opts = crate::proc::spawn::SpawnOptions::new(vfs_path)
        .argv(&argv_refs)
        .exe_path(vfs_path.as_bytes());

    let pid = run(id, &elf, &opts)?;

    // Record the launch spec for restart.  run() already verified the
    // container, so the slot is valid here; still guard defensively.
    with_table(|table| {
        let idx = id as usize;
        if idx < MAX_CONTAINERS && table.containers[idx].active {
            table.containers[idx].init_exe_path.clear();
            table.containers[idx].init_exe_path.push_str(vfs_path);
            table.containers[idx].init_args =
                extra_args.iter().map(|s| String::from(*s)).collect();
        }
    });

    Ok(pid)
}

/// Restart a container by re-launching its recorded init command (Docker
/// `restart`).
///
/// Re-runs the exact command last launched via [`run_path`]: if the container
/// is still running it is first force-killed and stopped, then the container is
/// reset to a fresh `Created` state (init PID and process list cleared, exit
/// code and freeze flag reset — its namespaces, cgroup, rootfs, volumes,
/// published ports, and labels are *preserved*), and the stored command is
/// launched again via [`run_path`].
///
/// Returns the global PID of the newly-launched init process.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist or has no
///   recorded launch spec (it was never run via [`run_path`], so there is
///   nothing to replay).
/// - Any error from [`run_path`] (the binary vanished from the VFS, OOM, …).
pub fn restart(id: ContainerId) -> KernelResult<u64> {
    // A manual restart resets the auto-restart clock (Docker restarts the
    // policy counter when the user intervenes).
    relaunch_recorded(id, true)
}

/// Replay a container's recorded init command (the shared core of the manual
/// [`restart`] and the automatic [`do_container_restart`]).
///
/// `reset_restart_count` distinguishes the two callers: a manual restart
/// resets the auto-restart counter to 0, whereas an automatic restart
/// preserves it so an [`RestartPolicy::OnFailure`] cap is honoured across the
/// series of automatic attempts.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist or has no
///   recorded launch spec.
/// - Any error from [`run_path`] (the binary vanished from the VFS, OOM, …).
fn relaunch_recorded(id: ContainerId, reset_restart_count: bool) -> KernelResult<u64> {
    // Fetch the stored launch spec and current state under the table lock.  A
    // container with no recorded path was never run via run_path and cannot be
    // restarted.
    let (exe_path, args, is_running) = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        let ct = &table.containers[idx];
        if ct.init_exe_path.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        Ok((
            ct.init_exe_path.clone(),
            ct.init_args.clone(),
            ct.state == ContainerState::Running,
        ))
    })?;

    // If still running, stop the container *first* (flips it out of the
    // `Running` state and flushes its published-port forwards), *then*
    // force-kill the live processes.  The stop-before-kill ordering is
    // deliberate: it moves the container out of `Running` before the old init
    // dies, so the [`notify_init_exit`] fired by that death sees a non-`Running`
    // state and does *not* schedule a second (nested) restart.  kill()/stop()
    // both take the table lock internally, so they run outside the snapshot
    // above.
    if is_running {
        let _ = stop(id);
        let _ = kill(id);
    }

    // Reset the container to a fresh Created state, preserving its
    // configuration (namespaces, cgroup, rootfs, volumes, ports, labels) but
    // clearing the previous run's process bookkeeping.  Done under the table
    // lock; run_path()'s internal run() requires the Created state.
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        let ct = &mut table.containers[idx];
        ct.state = ContainerState::Created;
        ct.init_pid = None;
        ct.pids.clear();
        ct.exit_code = None;
        ct.frozen = false;
        // A relaunch clears any user-stop mark (set by the stop() above);
        // the container is about to run again.  The auto-restart counter is
        // reset only for a manual restart.
        ct.user_stopped = false;
        if reset_restart_count {
            ct.restart_count = 0;
        }
        Ok(())
    })?;

    // Replay the recorded command.  Borrow the owned arg strings as &str.
    // `run_path` -> `run` emits the `start` event; layer a `restart` event on
    // top so `container events` shows the relaunch (Docker emits `restart` in
    // addition to the underlying start).
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let pid = run_path(id, &exe_path, &arg_refs)?;
    let name = info(id).map_or(String::new(), |ci| ci.name);
    record_event(id, &name, ContainerEventKind::Restart, None);
    Ok(pid)
}

/// Resolve a container-relative path to its host VFS path under the container's
/// rootfs, rejecting any attempt to escape the jail.
///
/// `container_path` is interpreted as absolute inside the container (a leading
/// `/` is optional and stripped).  The result is `root_path` joined with the
/// cleaned container path.  Any `..` component is rejected (it could escape the
/// rootfs), as is a NUL byte — consistent with the kernel's path rules (all
/// bytes allowed except `/` separator and NUL).
///
/// Returns the host VFS path, or [`KernelError::InvalidArgument`] if the
/// container has no rootfs configured or the path is unsafe.
fn resolve_in_rootfs(root_path: &str, container_path: &str) -> KernelResult<String> {
    if root_path.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if container_path.contains('\0') {
        return Err(KernelError::InvalidArgument);
    }
    let rel = container_path.trim_start_matches('/');
    // Reject any `..` path component (jail escape) and empty/`.`-only paths.
    let mut has_real_component = false;
    for comp in rel.split('/') {
        if comp == ".." {
            return Err(KernelError::InvalidArgument);
        }
        if !comp.is_empty() && comp != "." {
            has_real_component = true;
        }
    }
    if !has_real_component {
        // Refuse to copy the rootfs root itself (must name a file).
        return Err(KernelError::InvalidArgument);
    }
    let base = root_path.trim_end_matches('/');
    Ok(alloc::format!("{base}/{rel}"))
}

/// Copy a file's bytes *into* a container's filesystem (Docker `cp` host →
/// container).
///
/// `container_path` is resolved under the container's rootfs (see
/// [`resolve_in_rootfs`]) and the `data` is written there via the VFS.  The
/// container must have a rootfs configured (`container rootfs`), and the
/// destination's parent directory must already exist.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive, has
///   no rootfs, or `container_path` is unsafe (escapes the jail / names the
///   root).
/// - Any VFS write error (missing parent directory, read-only fs, …).
pub fn copy_to_container(
    id: ContainerId,
    container_path: &str,
    data: &[u8],
) -> KernelResult<()> {
    let root_path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].root_path.clone())
    })?;
    let host_path = resolve_in_rootfs(&root_path, container_path)?;
    crate::fs::vfs::Vfs::write_file(&host_path, data)
}

/// Copy a file's bytes *out of* a container's filesystem (Docker `cp`
/// container → host).
///
/// `container_path` is resolved under the container's rootfs (see
/// [`resolve_in_rootfs`]) and its bytes are read via the VFS and returned.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive, has
///   no rootfs, or `container_path` is unsafe.
/// - Any VFS read error (file not found, …).
pub fn copy_from_container(
    id: ContainerId,
    container_path: &str,
) -> KernelResult<Vec<u8>> {
    let root_path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].root_path.clone())
    })?;
    let host_path = resolve_in_rootfs(&root_path, container_path)?;
    crate::fs::vfs::Vfs::read_file(&host_path)
}

/// Report the entry kind (file/directory/symlink) of a path inside a
/// container's rootfs, so a caller (e.g. the `cp` command) can choose between a
/// single-file and a recursive-directory copy.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive, has
///   no rootfs, or `container_path` is unsafe.
/// - Any VFS `stat` error (e.g. path not found).
pub fn entry_kind_in_container(
    id: ContainerId,
    container_path: &str,
) -> KernelResult<crate::fs::vfs::EntryType> {
    let root_path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].root_path.clone())
    })?;
    let host_path = resolve_in_rootfs(&root_path, container_path)?;
    Ok(crate::fs::vfs::Vfs::stat(&host_path)?.entry_type)
}

/// Recursively copy a directory *out of* a container's filesystem as a tar
/// archive (Docker `cp` of a directory, container → host side).
///
/// `container_path` is resolved under the container's rootfs (see
/// [`resolve_in_rootfs`]) and its subtree is packed via [`tar_tree`] with names
/// relative to that directory.  The caller extracts the archive on the host
/// side (e.g. via [`untar_tree`]).
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive, has
///   no rootfs, or `container_path` is unsafe.
/// - Any error propagated from [`tar_tree`].
pub fn copy_dir_from_container(
    id: ContainerId,
    container_path: &str,
) -> KernelResult<Vec<u8>> {
    let root_path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].root_path.clone())
    })?;
    let host_path = resolve_in_rootfs(&root_path, container_path)?;
    tar_tree(&host_path)
}

/// Recursively copy a tar archive *into* a container's filesystem under a
/// directory (Docker `cp` of a directory, host → container side).
///
/// `container_path` is resolved under the container's rootfs (see
/// [`resolve_in_rootfs`]) and `archive` is extracted there via [`untar_tree`].
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive, has
///   no rootfs, or `container_path` is unsafe.
/// - Any error propagated from [`untar_tree`].
pub fn copy_dir_to_container(
    id: ContainerId,
    container_path: &str,
    archive: &[u8],
) -> KernelResult<()> {
    let root_path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].root_path.clone())
    })?;
    let host_path = resolve_in_rootfs(&root_path, container_path)?;
    untar_tree(&host_path, archive)
}

/// Upper bound on the number of filesystem objects packed into a single
/// tar archive by [`tar_tree`].  Bounds memory/time against a pathological or
/// adversarially-deep tree so the traversal can never run unbounded.
const MAX_EXPORT_ENTRIES: usize = 65_536;

/// Pack an arbitrary host VFS directory subtree into an uncompressed `ustar`
/// tar archive, with member names **relative to `base`**.
///
/// Walks the subtree iteratively (an explicit work stack, never kernel-stack
/// recursion) and packs every directory, regular file and symlink.  Permissions,
/// owner uid/gid and mtime are preserved from the source metadata where the
/// underlying filesystem tracks them, with conventional defaults otherwise.
/// This is the shared primitive behind both [`export_rootfs`] and the recursive
/// container→host `cp` of a directory.
///
/// # Errors
/// - [`KernelError::ResourceExhausted`] if the subtree exceeds
///   [`MAX_EXPORT_ENTRIES`] objects.
/// - Any VFS error encountered while reading the tree.
pub fn tar_tree(base: &str) -> KernelResult<Vec<u8>> {
    use crate::fs::tar::{EntryKind, TarWriteEntry};
    use crate::fs::vfs::{EntryType, Vfs};

    let mut entries: Vec<TarWriteEntry> = Vec::new();
    // Explicit work stack of (host_path, archive_rel) directories still to
    // visit; an iterative walk avoids unbounded kernel-stack recursion on a
    // deeply-nested tree.
    let mut dirs: Vec<(String, String)> =
        alloc::vec![(String::from(base.trim_end_matches('/')), String::new())];
    while let Some((host_dir, rel_dir)) = dirs.pop() {
        let listing = Vfs::readdir(&host_dir)?;
        for de in listing {
            if de.name == "." || de.name == ".." {
                continue;
            }
            if entries.len() >= MAX_EXPORT_ENTRIES {
                return Err(KernelError::ResourceExhausted);
            }
            let host_child = alloc::format!("{host_dir}/{}", de.name);
            let rel_child = if rel_dir.is_empty() {
                de.name.clone()
            } else {
                alloc::format!("{rel_dir}/{}", de.name)
            };
            // Best-effort metadata for permissions/owner/mtime; fall back to
            // conventional defaults when the FS doesn't track them or the
            // entry can't be stat'd (e.g. a dangling symlink).
            let meta = Vfs::metadata(&host_child).ok();
            let (mode, uid, gid, mtime) = meta
                .as_ref()
                .map(|m| {
                    (
                        u32::from(m.permissions),
                        m.uid,
                        m.gid,
                        m.modified_ns.checked_div(1_000_000_000).unwrap_or(0),
                    )
                })
                .unwrap_or((0, 0, 0, 0));
            match de.entry_type {
                EntryType::Directory => {
                    entries.push(TarWriteEntry {
                        name: alloc::format!("{rel_child}/"),
                        data: Vec::new(),
                        kind: EntryKind::Directory,
                        link_target: String::new(),
                        mode: if mode == 0 { 0o755 } else { mode },
                        uid,
                        gid,
                        mtime,
                    });
                    dirs.push((host_child, rel_child));
                }
                EntryType::File => {
                    let data = Vfs::read_file(&host_child)?;
                    entries.push(TarWriteEntry {
                        name: rel_child,
                        data,
                        kind: EntryKind::File,
                        link_target: String::new(),
                        mode: if mode == 0 { 0o644 } else { mode },
                        uid,
                        gid,
                        mtime,
                    });
                }
                EntryType::Symlink => {
                    let target = Vfs::readlink(&host_child).unwrap_or_default();
                    entries.push(TarWriteEntry {
                        name: rel_child,
                        data: Vec::new(),
                        kind: EntryKind::Symlink,
                        link_target: target,
                        mode: if mode == 0 { 0o777 } else { mode },
                        uid,
                        gid,
                        mtime,
                    });
                }
                EntryType::VolumeLabel => {
                    // FAT volume labels have no portable tar representation.
                }
            }
        }
    }
    Ok(crate::fs::tar::create(&entries))
}

/// Export a container's filesystem as an uncompressed `ustar` tar archive
/// (Docker `container export`).
///
/// Thin wrapper over [`tar_tree`] rooted at the container's configured rootfs:
/// the archive holds the whole rootfs with names relative to its root, so it
/// unpacks to the same layout under any target directory.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive or
///   has no rootfs configured (nothing to export).
/// - Any error propagated from [`tar_tree`].
pub fn export_rootfs(id: ContainerId) -> KernelResult<Vec<u8>> {
    let root_path = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        let rp = table.containers[idx].root_path.clone();
        if rp.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        Ok(rp)
    })?;
    tar_tree(&root_path)
}

/// Idempotently create every prefix directory of `rel` underneath `base`.
///
/// `base/<comp1>`, `base/<comp1>/<comp2>`, … are each `mkdir`'d in order
/// (already-exists is ignored), so a later file write into a deep path never
/// fails for a missing parent regardless of the source tar's entry ordering.
/// Empty and `.` components are skipped; callers must have already rejected
/// `..` components.
fn ensure_dir_path(base: &str, rel: &str) {
    let mut acc = String::from(base);
    for comp in rel.split('/') {
        if comp.is_empty() || comp == "." {
            continue;
        }
        acc.push('/');
        acc.push_str(comp);
        // Already-exists is the expected steady state; any real error here
        // (e.g. a parent that is a file) surfaces later as a write failure.
        let _ = crate::fs::vfs::Vfs::mkdir(&acc);
    }
}

/// Extract a `ustar` tar archive into the host VFS directory `base`.
///
/// Parses and validates the archive *before* writing anything (a malformed tar
/// or a `..`-escaping member name fails without partial extraction), then
/// creates `base` and writes every directory, regular file and symlink with
/// names relative to `base`, creating parent directories as needed independent
/// of the archive's entry ordering.  This is the shared primitive behind both
/// [`import_rootfs`] and the recursive host→container `cp` of a directory.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if `base` is empty/contains NUL or an
///   archive member name contains a `..` component (jail escape).
/// - Any tar-parse or VFS error encountered while extracting.
pub fn untar_tree(base: &str, archive: &[u8]) -> KernelResult<()> {
    if base.is_empty() || base.contains('\0') {
        return Err(KernelError::InvalidArgument);
    }
    let base = base.trim_end_matches('/');
    if base.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    // Parse (and thereby validate) the archive before mutating any state.
    let entries = crate::fs::tar::parse(archive)?;

    // Validate every member name up front (reject `..` escapes) before writing
    // anything to disk.
    for entry in &entries {
        let rel = entry.name.trim_start_matches('/');
        if rel.split('/').any(|c| c == "..") {
            return Err(KernelError::InvalidArgument);
        }
    }

    use crate::fs::tar::EntryKind;
    use crate::fs::vfs::Vfs;

    // Create the destination root (idempotent).
    let _ = Vfs::mkdir(base);

    for entry in &entries {
        let rel = entry.name.trim_start_matches('/');
        if rel.is_empty() || rel == "." {
            continue;
        }
        let dest = alloc::format!("{base}/{rel}");
        match entry.kind {
            EntryKind::Directory => {
                let rel_dir = rel.trim_end_matches('/');
                ensure_dir_path(base, rel_dir);
            }
            EntryKind::File => {
                if let Some((parent, _)) = rel.rsplit_once('/') {
                    ensure_dir_path(base, parent);
                }
                let data = crate::fs::tar::entry_data(archive, entry)?;
                Vfs::write_file(&dest, data)?;
            }
            EntryKind::Symlink => {
                if let Some((parent, _)) = rel.rsplit_once('/') {
                    ensure_dir_path(base, parent);
                }
                // A pre-existing symlink/file at this path is tolerated; the
                // archive's view wins where it can be applied.
                let _ = Vfs::symlink(&dest, &entry.link_target);
            }
            EntryKind::Other(_) => {
                // Devices, FIFOs, hardlinks, etc. have no rootfs analogue here.
            }
        }
    }
    Ok(())
}

/// Import a tar archive into a fresh container's rootfs (Docker `import`).
///
/// Extracts `archive` into `dest_dir` on the host VFS (via [`untar_tree`], which
/// validates the archive and rejects `..`-escaping member names), then creates a
/// new container named `name` whose rootfs is `dest_dir`.  Returns the new
/// container's id.
///
/// The archive is extracted (and validated) *before* any container state is
/// created, so a malformed tar fails without leaving a half-built container.
/// If the rootfs cannot be attached after creation, the container is rolled
/// back so no orphan is left behind.
///
/// # Errors
/// - [`KernelError::InvalidArgument`] if `dest_dir` is empty/contains NUL, an
///   archive name escapes the destination, or `name` is invalid.
/// - Any tar-parse or VFS error encountered while extracting.
pub fn import_rootfs(
    name: &str,
    archive: &[u8],
    dest_dir: &str,
) -> KernelResult<ContainerId> {
    // Extract the archive into the destination first; this also validates
    // `dest_dir` and the archive (a `..` escape or malformed tar fails here,
    // before any container state is created).
    untar_tree(dest_dir, archive)?;
    let base = dest_dir.trim_end_matches('/');

    // Materialise the container and attach the freshly-populated rootfs.  Roll
    // back the container if the rootfs can't be attached, leaving no orphan.
    let id = create(&ContainerConfig::new(name))?;
    if let Err(e) = set_root_path(id, base) {
        let _ = delete(id);
        return Err(e);
    }
    Ok(id)
}

/// Snapshot a container's current rootfs into a new, independent container
/// (Docker `commit`).
///
/// Captures the source container's filesystem as a tar archive (via
/// [`export_rootfs`]) and re-extracts it into `dest_dir` as the rootfs of a
/// freshly-created container named `new_name` (via [`import_rootfs`]).  The
/// result is a deep, point-in-time copy: subsequent writes to either the source
/// or the new container's rootfs do not affect the other.  Unlike Docker, this
/// produces a runnable container rather than a separate image — our container
/// model has no standalone image store, so the snapshot *is* a new container.
///
/// The source may be in any state; its filesystem is read, not modified.
///
/// # Errors
/// - Any error from [`export_rootfs`] (e.g. source invalid or rootfs-less).
/// - Any error from [`import_rootfs`] (e.g. invalid `dest_dir`/`new_name`).
pub fn commit(
    src_id: ContainerId,
    new_name: &str,
    dest_dir: &str,
) -> KernelResult<ContainerId> {
    let archive = export_rootfs(src_id)?;
    import_rootfs(new_name, &archive, dest_dir)
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

/// Mark a container's root filesystem read-only (Docker `--read-only`) before
/// it is run.
///
/// When set, writes by the container's processes that resolve into the
/// container rootfs (i.e. not into a writable `:rw` volume) are denied with
/// `EROFS`.  The flag is installed on every process launched by [`run`] via
/// [`crate::ipc::namespace::set_root_read_only`].  Read-only enforcement only
/// applies if the container also has a rootfs ([`set_root_path`]); without a
/// jail there is no container rootfs to protect.
///
/// Like [`set_root_path`], this only takes effect while the container is still
/// in [`Created`](ContainerState::Created) state — a running container's
/// already-jailed processes would not be retroactively affected.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist or is not
///   in `Created` state.
pub fn set_read_only_root(id: ContainerId, read_only: bool) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].read_only_root = read_only;
        Ok(())
    })
}

/// Set the container's UTS hostname (Docker `--hostname`).
///
/// When non-empty, every process launched by [`run`] is given this hostname
/// via [`crate::ipc::namespace::set_hostname`], so `uname(2)`/`gethostname(2)`
/// inside the container report it instead of the global system hostname.
/// Passing an empty string clears the override (the container sees the global
/// hostname).  Unlike the read-only-root flag this is independent of the
/// rootfs jail.
///
/// Like [`set_root_path`], this only takes effect while the container is still
/// in [`Created`](ContainerState::Created) state.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist, is not in
///   `Created` state, or `name` is longer than 64 bytes (the UTS field width)
///   or contains a NUL byte.
pub fn set_hostname(id: ContainerId, name: &str) -> KernelResult<()> {
    if name.len() > 64 || name.as_bytes().contains(&0) {
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
        table.containers[idx].hostname = String::from(name);
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

/// Record the overlay id backing this container's copy-on-write rootfs.
///
/// Stored at run time (alongside [`set_rootfs_mount`]) so introspection that
/// needs the writable scratch layer — [`diff`] — can locate the overlay's
/// upper layer and whiteouts. Only takes effect while the container is still
/// in `Created` state, matching the other rootfs setters. `None` clears it.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist or is not
///   in `Created` state.
pub fn set_overlay_id(
    id: ContainerId,
    overlay_id: Option<crate::fs::overlay::OverlayId>,
) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].overlay_id = overlay_id;
        Ok(())
    })
}

/// A single filesystem change reported by [`diff`] (Docker `diff`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffEntry {
    /// The kind of change (added / changed / deleted).
    pub kind: DiffKind,
    /// Absolute guest path (leading `/`) of the changed entry.
    pub path: String,
}

/// The three change classes Docker `diff` reports, matching its `A`/`C`/`D`
/// prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    /// Added: exists only in the writable upper layer (new file/dir).
    Added,
    /// Changed: exists in both layers — a lower-layer entry that was copied up
    /// (modified) or a directory whose contents changed.
    Changed,
    /// Deleted: whited out — present in the image but removed in the container.
    Deleted,
}

impl DiffKind {
    /// The single-character Docker `diff` prefix (`A`/`C`/`D`).
    #[must_use]
    pub fn prefix(self) -> char {
        match self {
            DiffKind::Added => 'A',
            DiffKind::Changed => 'C',
            DiffKind::Deleted => 'D',
        }
    }
}

/// Report the filesystem changes in a container relative to its image
/// (Docker `diff`).
///
/// Walks the container's overlay **upper** (writable) layer and classifies
/// every entry: an entry present in both layers is `Changed` (a copied-up file
/// or a directory whose contents changed), one present only in the upper layer
/// is `Added`. Whiteouts (entries deleted from the image) are reported as
/// `Deleted`. The result is sorted by path, so parent directories precede their
/// children — matching Docker's ordering.
///
/// The walk is iterative (an explicit work stack), not recursive, to bound
/// kernel stack use on deep container trees.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if the container id is invalid/inactive.
/// - [`KernelError::InvalidArgument`] if the container has no overlay (it was
///   jailed directly at a plain directory, so there is no writable layer to
///   diff).
/// - Propagates overlay errors from [`crate::fs::overlay`].
pub fn diff(id: ContainerId) -> KernelResult<Vec<DiffEntry>> {
    // Resolve the overlay id under the table lock. `Some(None)` means the
    // container exists but owns no overlay; `None` means no such container.
    let overlay_id = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some(table.containers[idx].overlay_id)
    })
    .ok_or(KernelError::NotFound)?;
    let Some(ov_id) = overlay_id else {
        return Err(KernelError::InvalidArgument);
    };

    let upper = crate::fs::overlay::upper_path(ov_id)?;
    let upper_base = upper.trim_end_matches('/');
    let mut out: Vec<DiffEntry> = Vec::new();

    // Iterative walk of the upper layer. Each work item is a normalized rel
    // directory ("" == the upper root).
    let mut stack: Vec<String> = alloc::vec![String::new()];
    while let Some(rel_dir) = stack.pop() {
        let dir_abs = if rel_dir.is_empty() {
            String::from(upper_base)
        } else {
            alloc::format!("{}/{}", upper_base, rel_dir)
        };
        // A directory that vanished mid-walk (concurrent teardown) is skipped
        // rather than failing the whole diff.
        let Ok(entries) = crate::fs::vfs::Vfs::readdir(&dir_abs) else {
            continue;
        };
        for e in entries {
            if e.name == "." || e.name == ".." {
                continue;
            }
            let child_rel = if rel_dir.is_empty() {
                e.name.clone()
            } else {
                alloc::format!("{}/{}", rel_dir, e.name)
            };
            let kind = match crate::fs::overlay::which_layer(ov_id, &child_rel)? {
                crate::fs::overlay::Layer::Both => DiffKind::Changed,
                crate::fs::overlay::Layer::Upper => DiffKind::Added,
                // Whited-out or (racily) vanished — not an upper-layer change.
                crate::fs::overlay::Layer::Lower | crate::fs::overlay::Layer::None => {
                    continue;
                }
            };
            out.push(DiffEntry {
                kind,
                path: alloc::format!("/{child_rel}"),
            });
            if e.entry_type == crate::fs::vfs::EntryType::Directory {
                stack.push(child_rel);
            }
        }
    }

    // Whiteouts → deleted entries (present in the image, removed here).
    for w in crate::fs::overlay::whiteouts(ov_id)? {
        out.push(DiffEntry {
            kind: DiffKind::Deleted,
            path: alloc::format!("/{w}"),
        });
    }

    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Author a new OCI image from a container's filesystem changes (Docker
/// `commit`).
///
/// Captures the container's overlay **upper** (writable) layer — the files it
/// added or changed since it started — plus its whiteouts (deletions), and
/// layers them on top of the container's base image (recorded at `oci run`
/// time in [`ContainerConfig::image_source`]). The base image's runtime config
/// (Env/Cmd/Entrypoint/…) and existing layers are carried forward verbatim; a
/// `commit`-style history entry is appended. The result is written as a
/// standalone OCI layout at `dest_dir`; the caller may then tag it into the
/// image store. Returns the new image's manifest digest (`"sha256:…"`).
///
/// This is **image production** (a new image from a container's writes) and is
/// distinct from [`commit`] (which clones a running container into a new
/// container). Only the `docker commit` / `oci commit` shell path routes here.
///
/// # Errors
///
/// - [`KernelError::NotFound`] if the container id is invalid/inactive.
/// - [`KernelError::InvalidArgument`] if the container has no overlay (jailed
///   directly at a plain directory — no writable layer to capture) or was not
///   created from an image (empty `image_source`, so there is no base to
///   extend).
/// - Propagates overlay/VFS/OCI errors.
pub fn commit_image(id: ContainerId, dest_dir: &str) -> KernelResult<String> {
    // Resolve the container's overlay id and base-image source under the lock.
    let (overlay_id, image_source) = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some((
            table.containers[idx].overlay_id,
            table.containers[idx].image_source.clone(),
        ))
    })
    .ok_or(KernelError::NotFound)?;

    let Some(ov_id) = overlay_id else {
        return Err(KernelError::InvalidArgument);
    };
    if image_source.is_empty() {
        return Err(KernelError::InvalidArgument);
    }

    let upper = crate::fs::overlay::upper_path(ov_id)?;
    let whiteouts = crate::fs::overlay::whiteouts(ov_id)?;

    let desc = crate::oci::commit_image(&image_source, &upper, &whiteouts, dest_dir)?;
    Ok(desc.digest)
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
///
/// `read_only == true` makes the volume reject writes once the container runs
/// (writes to the mount and its subtree fail with `EROFS`), matching Docker
/// `-v host:guest:ro`.
pub fn add_volume_mount(
    id: ContainerId,
    host_target: &str,
    guest_prefix: &str,
    read_only: bool,
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
        // wins), mirroring `namespace::add_volume` semantics. Both the host
        // target and the read-only flag are overwritten.
        if let Some(existing) =
            vols.iter_mut().find(|(g, _, _)| g == guest_prefix)
        {
            existing.1 = String::from(host_target);
            existing.2 = read_only;
            return Ok(());
        }
        if vols.len() >= MAX_VOLUMES_PER_CONTAINER {
            return Err(KernelError::ResourceExhausted);
        }
        vols.push((
            String::from(guest_prefix),
            String::from(host_target),
            read_only,
        ));
        Ok(())
    })
}

/// Add a tmpfs (in-memory) mount to a container — the Docker `--tmpfs /guest`
/// mechanism.
///
/// Mounts a fresh [`crate::fs::memfs`] at a unique host mountpoint under
/// [`TMPFS_ROOT`] and records it as a writable volume at `guest_prefix`, so the
/// container sees an ephemeral, in-memory writable filesystem at that path —
/// commonly used to give a `--read-only` container scratch space (e.g.
/// `--tmpfs /tmp`) without persisting anything to the image. The mountpoint is
/// owned by the container: [`delete`] unmounts it and removes its backing
/// directory, so the contents are freed when the container is removed.
///
/// Must be called while the container is still in
/// [`Created`](ContainerState::Created) state (the mount is installed on each
/// process at [`run`] time via the volume list). Re-adding the same
/// `guest_prefix` is rejected — unlike a bind volume there is no meaningful
/// "replace" for an owned mountpoint (the old memfs would leak); remove and
/// recreate the container instead.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if `guest_prefix` is not an absolute path,
///   is `/`, the container doesn't exist, is not in `Created` state, or already
///   has a tmpfs/volume at `guest_prefix`.
/// - [`KernelError::ResourceExhausted`] if the container already has
///   [`MAX_VOLUMES_PER_CONTAINER`] volume/tmpfs entries.
/// - Any VFS error from creating or mounting the tmpfs backing directory.
pub fn add_tmpfs_mount(id: ContainerId, guest_prefix: &str) -> KernelResult<()> {
    if !guest_prefix.starts_with('/') || guest_prefix == "/" {
        return Err(KernelError::InvalidArgument);
    }

    // Reserve an index and validate state/uniqueness under the table lock,
    // WITHOUT touching the VFS (which has its own locking — never nest).
    let index = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        let ct = &table.containers[idx];
        if ct.state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        // Reject a duplicate guest prefix (a plain volume or an existing tmpfs).
        if ct.volumes.iter().any(|(g, _, _)| g == guest_prefix) {
            return Err(KernelError::InvalidArgument);
        }
        if ct.volumes.len() >= MAX_VOLUMES_PER_CONTAINER {
            return Err(KernelError::ResourceExhausted);
        }
        Ok(ct.tmpfs_mounts.len())
    })?;

    // Build a unique host mountpoint and mount a fresh in-memory filesystem
    // there, outside the table lock.
    let host_mount = alloc::format!("{TMPFS_ROOT}/{id}-{index}");
    crate::fs::vfs::Vfs::mkdir_all(&host_mount)?;
    // On mount failure, leave the (empty) mountpoint dir behind — harmless, and
    // removing it here would race a concurrent mount attempt. Propagate the
    // mount error so the caller can surface it.
    crate::fs::memfs::mount(&host_mount)?;

    // Record the mapping. If the container vanished or left Created state
    // between the checks above and here (single session: it won't), roll the
    // mount back so we don't leak an unowned memfs.
    let recorded = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Created {
            return Err(KernelError::InvalidArgument);
        }
        let ct = &mut table.containers[idx];
        ct.volumes.push((String::from(guest_prefix), host_mount.clone(), false));
        ct.tmpfs_mounts.push(host_mount.clone());
        Ok(())
    });
    if recorded.is_err() {
        let _ = crate::fs::vfs::Vfs::unmount(&host_mount);
        let _ = crate::fs::vfs::Vfs::remove_recursive(&host_mount);
    }
    recorded
}

/// Publish a container port to the host — the Docker `-p host:container[/proto]`
/// mechanism.
///
/// Records a port-forward intent so that, when the container is [`run`], a NAT
/// rule is installed forwarding host traffic arriving at `host_port` to the
/// container's `container_port` inside its network namespace.  The forward
/// target is the container's own IP (captured at create time), so the
/// container must have been created with a network IP
/// ([`ContainerConfig::network`]) — publishing a port on a network-less
/// container is rejected.
///
/// Must be called while the container is still in
/// [`Created`](ContainerState::Created) state — the NAT rules are installed at
/// `run` time.  Re-publishing the same `(proto, host_port)` replaces the
/// container-port target (last-writer-wins), matching the volume/rootfs
/// configuration semantics.  `host_port`/`container_port` of 0 are rejected
/// (port 0 is not a valid forwarding endpoint).
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container doesn't exist, is not in
///   `Created` state, has no network IP configured, or either port is 0.
/// - [`KernelError::ResourceExhausted`] if the container already publishes
///   [`MAX_PUBLISHED_PORTS`] ports.
pub fn add_port_publish(
    id: ContainerId,
    proto: crate::net::nat::NatProto,
    host_port: u16,
    container_port: u16,
) -> KernelResult<()> {
    if host_port == 0 || container_port == 0 {
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
        // A published port forwards to the container's own IP — without one
        // there is no forward target, so reject rather than silently no-op.
        if table.containers[idx].container_ip.is_none() {
            return Err(KernelError::InvalidArgument);
        }
        let ports = &mut table.containers[idx].published_ports;
        // Replace an existing publish at the same (proto, host_port)
        // (last-writer-wins) — a host port can map to only one target.
        if let Some(existing) =
            ports.iter_mut().find(|(p, h, _)| *p == proto && *h == host_port)
        {
            existing.2 = container_port;
            return Ok(());
        }
        if ports.len() >= MAX_PUBLISHED_PORTS {
            return Err(KernelError::ResourceExhausted);
        }
        ports.push((proto, host_port, container_port));
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
            memberships: ct.memberships.iter().map(|m| NetworkAttachment {
                network_name: m.network_name.clone(),
                veth_pair: m.veth_pair,
                ip: m.ip,
            }).collect(),
            nr_procs: ct.pids.len(),
            init_pid: ct.init_pid,
            root_path: ct.root_path.clone(),
            rootfs_mount: ct.rootfs_mount.clone(),
            volumes: ct.volumes.clone(),
            read_only_root: ct.read_only_root,
            hostname: ct.hostname.clone(),
            container_ip: ct.container_ip,
            published_ports: ct.published_ports.clone(),
            labels: ct.labels.clone(),
            exit_code: ct.exit_code,
            frozen: ct.frozen,
            restart_policy: ct.restart_policy,
            restart_count: ct.restart_count,
            auto_remove: ct.auto_remove,
            created_seq: ct.created_seq,
            health_status: ct.health_status,
            has_healthcheck: ct.healthcheck.is_some(),
            health_fail_streak: ct.health_fail_streak,
        })
    })
}

// ---------------------------------------------------------------------------
// Public API: user-defined-network membership (Docker multi-network, §60)
// ---------------------------------------------------------------------------

/// Record the container's *create-time primary* network membership.
///
/// The primary interface (`ContainerConfig::net_ip`) is created inside
/// [`create`] and its veth pair stored in [`Container::veth_pair`]; this call
/// registers the corresponding user-defined-network membership so `inspect`/`ps`
/// list it alongside any runtime-attached networks (§60). It reuses the existing
/// primary `veth_pair` rather than creating a new interface.
///
/// Idempotent per network name: re-recording the same network updates the
/// address in place rather than duplicating the membership.
///
/// # Errors
/// - [`KernelError::NotFound`] if the id is invalid or its slot is inactive, or
///   the container has no primary veth pair to associate.
pub fn record_primary_membership(
    id: ContainerId,
    network_name: &str,
    ip: [u8; 4],
    subnet: [u8; 4],
    prefix_len: u8,
) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::NotFound);
        }
        let ct = &mut table.containers[idx];
        let vp = ct.veth_pair.ok_or(KernelError::NotFound)?;
        if let Some(m) = ct.memberships.iter_mut().find(|m| m.network_name == network_name) {
            m.ip = ip;
            m.subnet = subnet;
            m.prefix_len = prefix_len;
            m.veth_pair = vp;
        } else {
            ct.memberships.push(NetworkMembership {
                network_name: String::from(network_name),
                veth_pair: vp,
                ip,
                subnet,
                prefix_len,
            });
        }
        Ok(())
    })
}

/// Whether the container is already a member of `network_name`.
#[must_use]
pub fn is_member_of(id: ContainerId, network_name: &str) -> bool {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return false;
        }
        table.containers[idx]
            .memberships
            .iter()
            .any(|m| m.network_name == network_name)
    })
}

/// Look up the container's membership on `network_name`.
///
/// Returns `(ip, is_primary)` where `is_primary` is true when this membership
/// reuses the create-time primary interface (see [`detach_network`], which
/// refuses to detach the primary). Returns `None` if the id/slot is invalid or
/// the container is not a member of the network.
#[must_use]
pub fn network_membership(id: ContainerId, network_name: &str) -> Option<([u8; 4], bool)> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        let ct = &table.containers[idx];
        let primary = ct.veth_pair;
        ct.memberships
            .iter()
            .find(|m| m.network_name == network_name)
            .map(|m| (m.ip, Some(m.veth_pair) == primary))
    })
}

/// Runtime-attach the container to another user-defined network (`network
/// connect`, §60).
///
/// Creates a *fresh* veth pair into the container's netns (distinct from the
/// primary interface and from any other network's interface), brings it up,
/// installs a directly-connected route for the network's subnet, and records
/// the membership. The returned veth pair id is the host-side interface the
/// caller attaches to the network's L2 bridge (via
/// [`crate::cnetwork::attach_container_veth`]).
///
/// On any failure the partially-created veth is torn down so no interface is
/// leaked. If the container is already a member of `network_name`,
/// [`KernelError::AlreadyExists`] is returned (Docker rejects a duplicate
/// connect).
///
/// # Errors
/// - [`KernelError::NotFound`] if the id is invalid or its slot is inactive.
/// - [`KernelError::AlreadyExists`] if already a member of the network.
/// - Propagates veth/netns setup errors (e.g. [`KernelError::ResourceExhausted`]
///   if veth slots are exhausted).
pub fn attach_network(
    id: ContainerId,
    network_name: &str,
    ip: [u8; 4],
    subnet: [u8; 4],
    prefix_len: u8,
    gateway: [u8; 4],
) -> KernelResult<crate::net::veth::VethPairId> {
    // Read the container's netns while validating membership state.
    let net_ns = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::NotFound);
        }
        if table.containers[idx].memberships.iter().any(|m| m.network_name == network_name) {
            return Err(KernelError::AlreadyExists);
        }
        Ok(table.containers[idx].net_ns)
    })?;

    // Create + wire up a fresh interface into the container's netns.
    let vp = setup_container_veth(net_ns)?;

    // Install a directly-connected route for the network's subnet so traffic
    // to peers on this network is sent out the new interface (gateway 0.0.0.0 =
    // on-link). Non-fatal: a route-table-full condition should not fail the
    // whole attach, but we roll the veth back on a genuine namespace error.
    let dest = crate::netns::Ipv4Addr(subnet);
    let mask = crate::netns::Ipv4Addr(prefix_to_mask(prefix_len));
    let gw = crate::netns::Ipv4Addr(gateway);
    if let Err(e) = crate::netns::add_route(net_ns, dest, mask, gw, 1) {
        // Only unwind on a hard namespace error; a full route table is tolerable.
        if e == KernelError::InvalidArgument {
            let _ = crate::net::veth::destroy_pair(vp);
            return Err(e);
        }
    }

    // Record the membership (re-checking the slot is still valid under the lock).
    let recorded = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return false;
        }
        table.containers[idx].memberships.push(NetworkMembership {
            network_name: String::from(network_name),
            veth_pair: vp,
            ip,
            subnet,
            prefix_len,
        });
        true
    });
    if !recorded {
        let _ = crate::netns::remove_route(net_ns, dest, mask);
        let _ = crate::net::veth::destroy_pair(vp);
        return Err(KernelError::NotFound);
    }
    Ok(vp)
}

/// Runtime-detach the container from a user-defined network (`network
/// disconnect`, §60).
///
/// Removes the membership for `network_name`, tears down its connected route,
/// and destroys its veth pair. The caller must first detach the veth from the
/// network's L2 bridge and release its IPAM lease (see
/// [`crate::cnetwork::disconnect_container`], which orchestrates the full
/// sequence). Returns the container's address that was on that network.
///
/// Detaching the create-time *primary* network is refused with
/// [`KernelError::InvalidArgument`]: the primary interface is owned by the
/// container lifecycle and torn down at [`delete`], not by `disconnect` (this
/// matches Docker, which will not disconnect a container from the network it
/// was `run` on if that would leave it unreachable — here we simply protect the
/// primary veth from being destroyed out from under the container).
///
/// # Errors
/// - [`KernelError::NotFound`] if the id/slot is invalid or the container is not
///   a member of `network_name`.
/// - [`KernelError::InvalidArgument`] if `network_name` is the primary network.
pub fn detach_network(id: ContainerId, network_name: &str) -> KernelResult<[u8; 4]> {
    let (net_ns, veth_pair, ip, subnet, prefix_len) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::NotFound);
        }
        let ct = &mut table.containers[idx];
        let primary = ct.veth_pair;
        let pos = ct
            .memberships
            .iter()
            .position(|m| m.network_name == network_name)
            .ok_or(KernelError::NotFound)?;
        // Refuse to detach the primary interface (its veth is destroyed at delete).
        if Some(ct.memberships[pos].veth_pair) == primary {
            return Err(KernelError::InvalidArgument);
        }
        let m = ct.memberships.remove(pos);
        Ok((ct.net_ns, m.veth_pair, m.ip, m.subnet, m.prefix_len))
    })?;

    // Tear down the connected route and the interface (best-effort; the
    // membership is already gone, so these must not fail the operation).
    let dest = crate::netns::Ipv4Addr(subnet);
    let mask = crate::netns::Ipv4Addr(prefix_to_mask(prefix_len));
    let _ = crate::netns::remove_route(net_ns, dest, mask);
    let _ = crate::net::veth::destroy_pair(veth_pair);
    Ok(ip)
}

/// Expand an IPv4 prefix length (0..=32) into a dotted netmask.
fn prefix_to_mask(prefix_len: u8) -> [u8; 4] {
    let n = prefix_len.min(32);
    // 32-bit mask with the top `n` bits set. Guard the `>> 32` UB when n == 0.
    let bits: u32 = if n == 0 { 0 } else { u32::MAX << (32u8.saturating_sub(n)) };
    bits.to_be_bytes()
}

/// Configure (or clear) a container's healthcheck (Docker `HEALTHCHECK`).
///
/// Records `cfg` on the container so the healthcheck supervisor can drive
/// periodic probes.  Passing a config whose test is disabled (`NONE`) or that
/// is not runnable clears the healthcheck instead, matching Docker's
/// `HEALTHCHECK NONE`.  The container's health status is (re)initialised to
/// [`HealthStatus::Starting`] when a runnable check is installed, or
/// [`HealthStatus::None`] when cleared, and the failure streak is reset.
///
/// Returns [`KernelError::NotFound`] if the id is invalid or its slot is
/// inactive.
pub fn set_healthcheck(
    id: ContainerId,
    cfg: Option<crate::oci::HealthcheckConfig>,
) -> Result<(), KernelError> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::NotFound);
        }
        let ct = &mut table.containers[idx];
        // Any previously in-flight probe is abandoned on reconfiguration; the
        // supervisor keys new probes off the fresh config. (A live probe pid is
        // left to exit on its own — it is not the container's init, so it will
        // simply zombie-and-reap; the next tick will not poll it because the
        // in-flight slot is cleared here.)
        ct.health_probe_pid = None;
        ct.health_probe_task = 0;
        ct.health_probe_deadline_ns = 0;
        ct.health_probe_timed_out = false;
        // Probe as soon as the supervisor next ticks.
        ct.health_next_due_ns = 0;
        match cfg {
            Some(hc) if hc.is_runnable() => {
                ct.healthcheck = Some(hc);
                ct.health_status = HealthStatus::Starting;
                ct.health_fail_streak = 0;
                // Stamp the start-period reference from the current time so a
                // healthcheck installed after the container is already running
                // still honours its start-period grace.
                ct.health_started_ns = crate::hrtimer::now_ns();
            }
            _ => {
                ct.healthcheck = None;
                ct.health_status = HealthStatus::None;
                ct.health_fail_streak = 0;
            }
        }
        Ok(())
    })
}

/// Current healthcheck status of a container (Docker health sub-state).
///
/// Returns [`HealthStatus::None`] when no healthcheck is configured. `None`
/// (the option) if the container id is invalid or its slot is inactive.
#[must_use]
pub fn health_status(id: ContainerId) -> Option<HealthStatus> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some(table.containers[idx].health_status)
    })
}

// ---------------------------------------------------------------------------
// Healthcheck supervisor (Docker `HEALTHCHECK`)
// ---------------------------------------------------------------------------
//
// The supervisor drives periodic probes for every running container that has a
// runnable healthcheck.  It runs entirely non-blockingly on the *shared*
// workqueue worker: a single repeating hrtimer fires in ISR context and submits
// [`health_tick`] to the workqueue, which polls all containers.  Because one
// worker drains the whole deferred-work FIFO, the tick must never block — a
// blocking `wait_process` inside it would stall *all* deferred work for up to a
// probe timeout.  Instead each probe is launched (via [`exec_path`]) and then
// *polled*: subsequent ticks observe the probe process's zombie transition,
// score the result with [`apply_probe_result`], and reap it.  A probe that
// overruns its timeout is killed and scored as a failure.

/// Base cadence of the healthcheck supervisor tick (250 ms).  This bounds the
/// granularity at which per-container probe intervals are honoured; the actual
/// probe period is the container's configured `Interval` (default 30 s), gated
/// by `health_next_due_ns`.
const HEALTH_TICK_INTERVAL_NS: u64 = 250_000_000;

/// Guards one-time arming of the supervisor's repeating hrtimer.
static HEALTH_MONITOR_ARMED: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Arm the container healthcheck supervisor.
///
/// Schedules a single repeating hrtimer (every [`HEALTH_TICK_INTERVAL_NS`]) that
/// drives [`health_tick`] on the workqueue.  Idempotent — arms at most once for
/// the lifetime of the system (guarded by [`HEALTH_MONITOR_ARMED`]).  Must be
/// called after both the hrtimer and the workqueue worker are initialised.
pub fn start_health_monitor() {
    use core::sync::atomic::Ordering;
    // Arm exactly once.
    if HEALTH_MONITOR_ARMED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    let _ = crate::hrtimer::schedule_repeating(
        HEALTH_TICK_INTERVAL_NS,
        HEALTH_TICK_INTERVAL_NS,
        health_timer_fire,
        0,
    );
    serial_println!(
        "[container] healthcheck supervisor armed ({} ms tick)",
        HEALTH_TICK_INTERVAL_NS / 1_000_000
    );
}

/// hrtimer ISR callback: hand the healthcheck poll off to the workqueue.
///
/// Runs in ISR context, so it does the minimum — submits [`health_tick_job`] to
/// the workqueue (where exec/reap/kill are safe).  A full queue drops this tick
/// (the next one will catch up); never blocks the ISR.
fn health_timer_fire(_arg: u64) {
    // Best-effort: if the queue is momentarily full, skip this tick.
    let _ = crate::workqueue::submit(health_tick_job, 0);
}

/// Workqueue callback wrapper for [`health_tick`].
fn health_tick_job(_arg: u64) {
    health_tick();
}

/// Poll every container's healthcheck once (the supervisor's unit of work).
///
/// Non-blocking: for each running container with a runnable healthcheck, this
/// launches a probe when one is due, polls an in-flight probe for completion,
/// and kills a probe that overran its timeout.  Safe to call directly (the boot
/// self-test does so deterministically) as well as from the workqueue.
pub fn health_tick() {
    let now = crate::hrtimer::now_ns();
    for idx in 0..MAX_CONTAINERS {
        health_tick_one(idx, now);
    }
}

/// Snapshot of a container's healthcheck state, read under the table lock so the
/// per-probe exec/poll/kill work below can run *without* holding it.
struct HealthProbeState {
    runnable: bool,
    running: bool,
    cfg: crate::oci::HealthcheckConfig,
    status: HealthStatus,
    streak: u32,
    started_ns: u64,
    probe_pid: Option<u64>,
    probe_task: u64,
    deadline_ns: u64,
    timed_out: bool,
    next_due_ns: u64,
}

/// Drive one container's healthcheck by a single step.
///
/// Reads a snapshot under the table lock, performs any exec/poll/kill work
/// outside the lock (those helpers take the table lock themselves), then writes
/// the resulting health state back.  The supervisor is single-threaded (one
/// workqueue worker; the self-test calls this from one task), so no two steps
/// for the same container overlap.
#[allow(clippy::too_many_lines)]
fn health_tick_one(idx: usize, now: u64) {
    // --- Phase A: snapshot under the lock ---
    let snap = with_table_ref(|table| {
        let ct = table.containers.get(idx)?;
        if !ct.active {
            return None;
        }
        let runnable = ct.healthcheck.as_ref().is_some_and(|c| c.is_runnable());
        // Nothing to do for a container with no healthcheck and no stray probe.
        if !runnable && ct.health_probe_pid.is_none() {
            return None;
        }
        Some(HealthProbeState {
            runnable,
            running: ct.state == ContainerState::Running,
            cfg: ct.healthcheck.clone().unwrap_or_default(),
            status: ct.health_status,
            streak: ct.health_fail_streak,
            started_ns: ct.health_started_ns,
            probe_pid: ct.health_probe_pid,
            probe_task: ct.health_probe_task,
            deadline_ns: ct.health_probe_deadline_ns,
            timed_out: ct.health_probe_timed_out,
            next_due_ns: ct.health_next_due_ns,
        })
    });
    let Some(snap) = snap else { return };

    let Ok(cid) = ContainerId::try_from(idx) else { return };

    // Mutable working copy of the fields we may change.
    let mut status = snap.status;
    let mut streak = snap.streak;
    let mut probe_pid = snap.probe_pid;
    let mut probe_task = snap.probe_task;
    let mut deadline_ns = snap.deadline_ns;
    let mut timed_out = snap.timed_out;
    let mut next_due_ns = snap.next_due_ns;

    // --- Phase B: act outside the lock ---
    if let Some(pid) = probe_pid {
        // A probe is in flight: poll it.
        match crate::proc::pcb::state(pid) {
            None | Some(crate::proc::pcb::ProcessState::Zombie) => {
                // Finished. Reap it (fast path — already a zombie, no blocking)
                // and score the result. A timed-out probe is always a failure.
                let reaped = wait_process(pid);
                let code = if timed_out {
                    1
                } else {
                    reaped.unwrap_or(1)
                };
                let (s, k) = apply_probe_result(
                    status, streak, snap.started_ns, now, &snap.cfg, code,
                );
                status = s;
                streak = k;
                // Unbind the probe process from the container.
                let _ = remove_process_task(cid, pid, probe_task);
                probe_pid = None;
                probe_task = 0;
                timed_out = false;
                next_due_ns = now.saturating_add(snap.cfg.effective_interval_ns());
            }
            Some(_) => {
                // Still running: enforce the per-probe timeout.
                if now >= deadline_ns && !timed_out {
                    crate::proc::thread::kill_process_threads(pid);
                    timed_out = true;
                    // Leave it in flight; a later tick reaps the zombie and
                    // scores the failure.
                }
            }
        }
    } else if snap.runnable && snap.running && now >= next_due_ns {
        // No probe in flight and one is due: launch it.
        match health_launch_probe(cid, &snap.cfg) {
            Ok(spawn) => {
                probe_pid = Some(spawn.pid);
                probe_task = spawn.task_id;
                deadline_ns = now.saturating_add(snap.cfg.effective_timeout_ns());
                timed_out = false;
            }
            Err(_) => {
                // Could not even launch the probe (binary missing, OOM): score
                // it as a failure and try again next interval.
                let (s, k) = apply_probe_result(
                    status, streak, snap.started_ns, now, &snap.cfg, 1,
                );
                status = s;
                streak = k;
                next_due_ns = now.saturating_add(snap.cfg.effective_interval_ns());
            }
        }
    } else if !snap.running {
        // Container stopped while a probe was outstanding: tear it down.
        if let Some(pid) = probe_pid {
            crate::proc::thread::kill_process_threads(pid);
            let _ = wait_process(pid);
            let _ = remove_process_task(cid, pid, probe_task);
            probe_pid = None;
            probe_task = 0;
            timed_out = false;
        }
    }

    // --- Phase C: write the results back under the lock ---
    with_table(|table| {
        let Some(ct) = table.containers.get_mut(idx) else { return };
        if !ct.active {
            return;
        }
        ct.health_status = status;
        ct.health_fail_streak = streak;
        ct.health_probe_pid = probe_pid;
        ct.health_probe_task = probe_task;
        ct.health_probe_deadline_ns = deadline_ns;
        ct.health_probe_timed_out = timed_out;
        ct.health_next_due_ns = next_due_ns;
    });
}

/// Launch a single healthcheck probe process inside a running container.
///
/// Builds the probe argv from the healthcheck config — a `CMD` probe execs its
/// argv directly, a `CMD-SHELL` probe execs `/bin/sh -c "<cmdline>"` — and hands
/// off to [`exec_path`], which binds the process into the container's namespaces
/// and cgroup.  Returns the spawned process's pid/task.
fn health_launch_probe(
    id: ContainerId,
    cfg: &crate::oci::HealthcheckConfig,
) -> KernelResult<ExecSpawn> {
    // Build owned argv byte-vectors, then a slice-of-slices for exec_path.
    let argv_owned: Vec<Vec<u8>> = if cfg.is_shell() {
        let cmdline = cfg
            .probe_args()
            .first()
            .map(|s| s.as_bytes().to_vec())
            .unwrap_or_default();
        alloc::vec![b"/bin/sh".to_vec(), b"-c".to_vec(), cmdline]
    } else {
        cfg.probe_args().iter().map(|s| s.as_bytes().to_vec()).collect()
    };
    let guest_cmd: &[u8] = argv_owned
        .first()
        .map(Vec::as_slice)
        .ok_or(KernelError::InvalidArgument)?;
    let argv_refs: Vec<&[u8]> = argv_owned.iter().map(Vec::as_slice).collect();
    exec_path(id, guest_cmd, &argv_refs)
}

/// List the global PIDs of the processes currently tracked in a container
/// (Docker `top`).
///
/// Returns the container's live process set in registration order — the init
/// process first (it is registered by [`run`] before any `exec`-ed children),
/// followed by processes added via [`add_process`]/[`add_process_task`].  A
/// process is removed from this set when it exits (see [`remove_process`]),
/// so the list reflects only currently-tracked PIDs; callers that want each
/// process's name/state look them up via the process table (which may race a
/// concurrent exit, in which case the lookup simply returns `None`).
///
/// `None` if the container id is invalid or its slot is inactive.
#[must_use]
pub fn pids(id: ContainerId) -> Option<Vec<u64>> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some(table.containers[idx].pids.clone())
    })
}

/// List a container's published (forwarded) port mappings (Docker `port`).
///
/// Returns the container's `(proto, host_port, container_port)` publish specs
/// in the order they were added via [`add_port_publish`] (the `-p` mechanism).
/// The list is what `run` installs as host-port NAT rules when the container
/// starts; it is a static property of the container's configuration, so it is
/// available in any state (including before the container has run).
///
/// `None` if the container id is invalid or its slot is inactive; `Some(empty)`
/// if the container publishes no ports.
#[must_use]
pub fn published_ports(id: ContainerId) -> Option<Vec<PublishedPort>> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some(table.containers[idx].published_ports.clone())
    })
}

/// Report whether a container has reached a terminal state, with its recorded
/// exit code (Docker `wait`).
///
/// A container is *terminal* once it is [`Stopped`](ContainerState::Stopped) or
/// [`Failed`](ContainerState::Failed): its init process has exited (or it was
/// never started but explicitly stopped) and it will not run again without a
/// fresh `run`.  [`Created`](ContainerState::Created) and
/// [`Running`](ContainerState::Running) are non-terminal — a caller polling
/// for completion should keep waiting.
///
/// Returns `Some((is_terminal, exit_code))`, where `exit_code` is the init
/// process's recorded code (`None` if the container was stopped manually before
/// any init exit recorded one). `None` if the container id is invalid/inactive.
///
/// This is the pure state-query primitive behind the blocking `container wait`
/// CLI, which loops on it (yielding between polls) until `is_terminal` is true.
#[must_use]
pub fn wait_status(id: ContainerId) -> Option<(bool, Option<i32>)> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        let ct = &table.containers[idx];
        let terminal = matches!(
            ct.state,
            ContainerState::Stopped | ContainerState::Failed
        );
        Some((terminal, ct.exit_code))
    })
}

/// Outcome of a blocking [`wait`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// The container reached a terminal state; carries the init process's
    /// recorded exit code (`0` when none was recorded — e.g. a manual stop of
    /// a never-run container).
    Exited(i32),
    /// The container was deleted out from under the waiter mid-wait.
    Removed,
}

/// Atomic snapshot for the blocking-wait loop: `(terminal, exit_code,
/// init_pid)`.  `None` when the container id is invalid/inactive (removed).
fn wait_snapshot(id: ContainerId) -> Option<(bool, Option<i32>, Option<u64>)> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        let ct = &table.containers[idx];
        let terminal = matches!(
            ct.state,
            ContainerState::Stopped | ContainerState::Failed
        );
        Some((terminal, ct.exit_code, ct.init_pid))
    })
}

/// Block the calling task until the container reaches a terminal state
/// (Docker `wait`), then return its init exit code.
///
/// This is the proper, event-driven counterpart to the pure [`wait_status`]
/// query: instead of spinning on `wait_status` and yielding (a busy-wait that
/// burns CPU while the init runs), the caller **blocks** on the container's
/// init process and is woken by the scheduler when that process zombifies.
/// The wake is delivered via the same [`pcb::set_wait_task`] mechanism
/// `wait4`/`waitpid` use: when the init thread exits, `remove_thread` takes the
/// registered wait-task (having *already* run [`notify_init_exit`], so the
/// container is `Stopped` with its `exit_code` recorded by the time we wake)
/// and the scheduler unblocks it.
///
/// The loop is lost-wakeup-safe two ways: (1) it re-checks the terminal state
/// *after* registering the wake, and (2) the scheduler's sticky `pending_wake`
/// flag makes a wake delivered between the re-check and `block_current`
/// short-circuit the next block. A container with a restart policy re-runs a
/// fresh init after each exit; this call keeps re-registering on the new init
/// and only returns when it observes an actually-terminal state (a stop not
/// followed by a restart) — event-driven throughout, never a spin.
///
/// # Concurrency
/// Only one waiter per init process is supported (`set_wait_task` holds a
/// single slot). A second concurrent `wait` on the *same* container would
/// overwrite the first's registration; the first would then miss its wake and
/// only make progress on the next init exit. In practice `container wait` is
/// driven from the single interactive shell task, so this is not exercised.
///
/// # Errors
/// Returns [`KernelError::NotFound`] if `id` is invalid/inactive at entry.
pub fn wait(id: ContainerId) -> KernelResult<WaitOutcome> {
    // Validate up front so a bad id is a clean error rather than a "removed".
    let Some((terminal, exit_code, mut init_pid)) = wait_snapshot(id) else {
        return Err(KernelError::NotFound);
    };
    if terminal {
        return Ok(WaitOutcome::Exited(exit_code.unwrap_or(0)));
    }

    let task_id = crate::sched::current_task_id();
    loop {
        // Register to be woken when the current init process zombifies.  If the
        // init pid is unknown (Created but not yet run, or a transient window
        // during restart), fall back to a short yield-and-recheck — there is no
        // process to block on yet.  This is bounded (it only applies before the
        // init exists) and is not the steady-state path.
        match init_pid {
            Some(pid) => match crate::proc::pcb::set_wait_task(pid, task_id) {
                Ok(()) => {}
                // The init was already reaped between the snapshot and here;
                // re-snapshot below will observe the terminal state (or the
                // next init if it restarted).
                Err(KernelError::NoSuchProcess) => {}
                Err(e) => return Err(e),
            },
            None => {
                crate::sched::yield_now();
            }
        }

        // Re-check AFTER registering (closes the register-before-exit race).
        match wait_snapshot(id) {
            None => return Ok(WaitOutcome::Removed),
            Some((true, code, _)) => return Ok(WaitOutcome::Exited(code.unwrap_or(0))),
            Some((false, _, pid)) => {
                init_pid = pid;
                // Only park when we actually registered on a live init; if the
                // init pid was unknown we already yielded above, so loop back to
                // re-register once the init exists.
                if init_pid.is_some() {
                    crate::sched::block_current();
                }
            }
        }
    }
}

/// Block the calling task until an arbitrary spawned process `pid` exits, then
/// reap it and return its exit code.
///
/// This generalises the block-on-exit mechanism proven by [`wait`] (which is
/// specialised to a container's *init* process) to any kernel-spawned
/// (parent-0) process — the primitive [`exec_path`] builds on to run a command
/// inside a container in the foreground and capture its exit status. It is the
/// missing "synchronous waitpid/join from kernel task context" that
/// known-issues.md D-CONTAINER-EXEC-WAIT called out as blocking real
/// `docker exec` and healthchecks.
///
/// The wait is delivered via the same [`pcb::set_wait_task`] slot `wait4` and
/// [`wait`] use: when `pid`'s last thread exits, `remove_thread` takes the
/// registered wait-task and the scheduler unblocks this caller. The loop is
/// lost-wakeup-safe two ways, identical to [`wait`]: (1) it re-checks the
/// process state *after* registering the wake, and (2) the scheduler's sticky
/// `pending_wake` flag makes a wake delivered between the re-check and
/// `block_current` short-circuit the next block.
///
/// On observing the zombie, the caller reads its recorded exit code and reaps
/// it (`try_reap`), so an exec'd non-init process does not linger as a zombie
/// with no reaping owner.
///
/// # Concurrency
/// As with [`wait`], only one waiter per process is supported (the
/// `set_wait_task` slot is single-valued). `exec` is driven from the single
/// interactive shell task, so this is not exercised concurrently.
///
/// # Errors
/// Returns [`KernelError::NoSuchProcess`] if `pid` does not name a live or
/// zombie process at entry (already reaped / never existed), or any error from
/// registering the wait.
pub fn wait_process(pid: u64) -> KernelResult<i32> {
    use crate::proc::pcb::{self, ProcessState};

    // The process must currently exist (still running or a zombie awaiting
    // reap). A pid that is already gone has no retrievable exit code.
    if pcb::state(pid).is_none() {
        return Err(KernelError::NoSuchProcess);
    }

    let task_id = crate::sched::current_task_id();
    loop {
        // Register to be woken when `pid` zombifies BEFORE re-checking its
        // state — this closes the register-before-exit race (an exit between
        // the check and the register would otherwise be missed).
        match pcb::set_wait_task(pid, task_id) {
            Ok(()) => {}
            // Reaped/removed between the entry check and here — nothing to
            // wait on; surface it rather than blocking forever.
            Err(KernelError::NoSuchProcess) => return Err(KernelError::NoSuchProcess),
            Err(e) => return Err(e),
        }

        match pcb::state(pid) {
            None => return Err(KernelError::NoSuchProcess),
            Some(ProcessState::Zombie) => {
                let code = pcb::exit_code(pid).unwrap_or(0);
                // Reap the zombie so it does not linger. Reap against its real
                // parent (0 for the kernel-spawned exec case). Best-effort: if
                // it was already reaped elsewhere the exit code is still valid.
                let parent = pcb::parent(pid).unwrap_or(0);
                let _ = pcb::try_reap(parent, pid);
                return Ok(code);
            }
            // Still running: park until the exit wake fires, then loop to
            // re-register and re-check.
            Some(_) => crate::sched::block_current(),
        }
    }
}

/// A process launched into a running container by [`exec_path`].
#[derive(Debug, Clone, Copy)]
pub struct ExecSpawn {
    /// Global PID of the launched process.
    pub pid: u64,
    /// Initial-thread task id (for teardown / cgroup accounting).
    pub task_id: u64,
}

/// Launch a command as a real process inside an already-running container
/// (the kernel-side of `docker exec`).
///
/// Unlike the old net-namespace-switch facade (which ran a *kshell builtin* in
/// the container's network namespace), this reads the target ELF from the
/// container's rootfs, spawns it as a genuine process, and binds it into the
/// container's cgroup + PID/user/network namespaces + rootfs jail via
/// [`add_process_task`] — exactly the wiring [`run`] uses for the init process,
/// minus flipping container state or recording an `init_pid`. The returned
/// process is enqueued but has not executed yet (so the namespace binding is in
/// place before its first instruction, same guarantee as [`run`]).
///
/// The caller drives the foreground/detached policy: [`wait_process`] blocks on
/// the returned pid for a foreground exec and returns its exit code; a detached
/// exec returns immediately with the pid.
///
/// `guest_cmd` is the absolute path of the executable *inside* the container
/// (resolved under the rootfs via [`resolve_in_rootfs`], so `..` cannot escape
/// the jail). `argv` is the full argument vector (argv[0] conventionally the
/// program name); it is passed straight to the spawned process.
///
/// Stdio is left at the spawn default (the console), so a foreground exec's
/// output appears live on the shell's console — better UX than the deferred
/// capture-file readback [`run`] uses for background `logs`.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container is invalid/inactive, not
///   in `Running` state, has no rootfs configured, or `guest_cmd` is unsafe
///   (escapes the jail / not valid UTF-8).
/// - [`KernelError::NotFound`] if the executable does not exist in the rootfs.
/// - Any error from [`spawn_process`](crate::proc::spawn::spawn_process)
///   (invalid ELF, out of memory) or from [`add_process_task`].
pub fn exec_path(
    id: ContainerId,
    guest_cmd: &[u8],
    argv: &[&[u8]],
) -> KernelResult<ExecSpawn> {
    exec_path_env(id, guest_cmd, argv, &[], None)
}

/// Like [`exec_path`], but launches the process with an explicit environment
/// (`envp`, a list of `KEY=VALUE` byte strings) and an optional initial working
/// directory (`cwd`). Used by the OCI build-time `RUN` executor (Q17/§58),
/// which must run the command with the image's accumulated `ENV` so `PATH`/etc.
/// resolve as in Docker, and at the image's `WORKDIR` so a `RUN` using a
/// relative path (e.g. `RUN ./configure`) resolves against the working
/// directory rather than `/`. An empty `envp` / `None` `cwd` each leave the
/// spawn default (identical to [`exec_path`]).
///
/// `cwd`, when `Some`, must be an absolute path; [`pcb::set_cwd`] rejects a
/// relative/too-long/NUL-bearing value (logged, and the child simply stays at
/// the default `/` — never a hard failure). Docker's `WORKDIR` normalises to an
/// absolute path and materialises the directory as a layer, so by the time a
/// `RUN` executes the directory exists in the merged rootfs.
///
/// # Errors
/// Identical to [`exec_path`].
pub fn exec_path_env(
    id: ContainerId,
    guest_cmd: &[u8],
    argv: &[&[u8]],
    envp: &[&[u8]],
    cwd: Option<&[u8]>,
) -> KernelResult<ExecSpawn> {
    // 1. Container must exist and be running.
    let (running, root_path) = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok((
            table.containers[idx].state == ContainerState::Running,
            table.containers[idx].root_path.clone(),
        ))
    })?;
    if !running {
        return Err(KernelError::InvalidArgument);
    }

    // 2. Resolve the guest command under the rootfs and read the ELF bytes.
    //    The command path is supplied by the container CLI as a str; require
    //    valid UTF-8 (general paths are bytes, but a command name is text).
    let guest_str =
        core::str::from_utf8(guest_cmd).map_err(|_| KernelError::InvalidArgument)?;
    let host_path = resolve_in_rootfs(&root_path, guest_str)?;
    let elf = crate::fs::vfs::Vfs::read_file(&host_path)
        .map_err(|_| KernelError::NotFound)?;

    // 3. Spawn the process (enqueued, not yet run) with the requested argv and
    //    an `exe_path` of the *guest* command (backs /proc/<pid>/exe). ABI is
    //    auto-detected from the ELF markers, matching `run`.
    let mut opts = crate::proc::spawn::SpawnOptions::new(guest_str);
    opts.argv = argv;
    // Only override the default (empty) env when the caller supplied one, so
    // `exec_path` callers keep the spawn default.
    if !envp.is_empty() {
        opts.envp = envp;
    }
    // Honor the image WORKDIR (or an explicit exec `-w`): set the child's initial
    // cwd so a relative-path command resolves against it, not `/`. `set_cwd`
    // (invoked inside `spawn_process`) validates and logs a bad value without
    // failing the spawn.
    if let Some(dir) = cwd {
        opts.cwd = Some(dir);
    }
    opts.exe_path = Some(guest_cmd);
    let result = crate::proc::spawn::spawn_process(&elf, &opts)?;

    // 4. Bind it into the container. On failure, tear the spawn down so a
    //    failed exec leaks nothing (same rollback as `run`).
    if let Err(e) = add_process_task(id, result.pid, result.task_id) {
        crate::proc::thread::kill_process_threads(result.pid);
        crate::proc::pcb::destroy(result.pid);
        return Err(e);
    }

    serial_println!(
        "[container] exec id={} '{}': pid={} task={} entry={:#x}",
        id, guest_str, result.pid, result.task_id, result.entry_point
    );

    Ok(ExecSpawn { pid: result.pid, task_id: result.task_id })
}

/// Rename a container (Docker `rename`).
///
/// Replaces the container's human-readable name. The new name is truncated to
/// [`MAX_NAME_LEN`] bytes (matching [`ContainerConfig::new`]); an empty name is
/// rejected. Names are not required to be unique (consistent with [`create`],
/// which does not enforce uniqueness either), so callers wanting Docker's
/// unique-name guarantee must check beforehand.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container id is invalid/inactive
///   or `new_name` is empty.
pub fn rename(id: ContainerId, new_name: &str) -> KernelResult<()> {
    if new_name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let truncated = if new_name.len() > MAX_NAME_LEN {
        new_name.get(..MAX_NAME_LEN).unwrap_or("")
    } else {
        new_name
    };
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].name.clear();
        table.containers[idx].name.push_str(truncated);
        Ok(())
    })
}

/// Change a container's restart policy in place (Docker `update --restart`).
///
/// Takes effect the next time the container's init exits: [`notify_init_exit`]
/// reads the current policy at that moment, so updating a running container
/// re-arms (or disarms) its auto-restart without disturbing the live process.
/// The restart counter is left untouched — switching to `on-failure:N` does not
/// grant a fresh budget mid-flight; a manual `start`/`restart` still resets it.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container id is invalid/inactive.
pub fn set_restart_policy(id: ContainerId, policy: RestartPolicy) -> KernelResult<()> {
    with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].restart_policy = policy;
        Ok(())
    })
}

/// Forcibly terminate a container by killing all of its tracked processes
/// (Docker `kill`).
///
/// Each process is killed (all of its threads are marked dead and the process
/// is zombified). The container's init process exiting trips the automatic
/// [`notify_init_exit`] transition, so the container moves to `Stopped` and
/// records a SIGKILL-style exit code (128 + SIGKILL = 137, matching Docker's
/// "Exited (137)"). Remaining (non-init) processes are killed too so the
/// container does not leak orphans into the host's init.
///
/// Best-effort: a process that has already exited (no live threads) is simply
/// skipped. Returns the number of processes that actually had threads killed.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container id is invalid/inactive.
pub fn kill(id: ContainerId) -> KernelResult<usize> {
    // Snapshot the tracked PIDs (init first) under the table lock, then do the
    // killing outside it — kill_process_threads touches the process/scheduler
    // tables and trips notify_init_exit (which re-takes the container table),
    // so it must not run while we hold the table lock.
    let process_ids = pids(id).ok_or(KernelError::InvalidArgument)?;
    // Capture the name before killing (the container stays active/named after a
    // kill, but snapshot it now so the `kill` event is recorded even if a later
    // reaper removes the container).  Docker emits `kill` in addition to the
    // `die` that `notify_init_exit` records when the init process exits.
    let name = info(id).map_or(String::new(), |ci| ci.name);
    let mut killed = 0usize;
    for pid in process_ids {
        // Record a SIGKILL-style exit code before the process zombifies so
        // notify_init_exit reports "Exited (137)". Ignore the error: a process
        // that already vanished simply has no exit code to set, and the kill
        // below will no-op for it.
        let _ = crate::proc::pcb::set_exit_code(pid, 137);
        if crate::proc::thread::kill_process_threads(pid) > 0 {
            killed = killed.saturating_add(1);
        }
    }
    record_event(id, &name, ContainerEventKind::Kill, None);
    Ok(killed)
}

/// Freeze a container, suspending all of its threads (Docker `pause`).
///
/// Marks the container frozen and suspends every thread of every tracked
/// process, so the whole container stops executing. While frozen, any process
/// subsequently joined to the container (via [`add_process_task`] — e.g. an
/// `exec` or a fresh `run`) is suspended on entry, so the freeze is complete:
/// no thread of the container can run until [`unpause`] thaws it. The
/// container's lifecycle state stays `Running` (pause is a sub-state of
/// running, orthogonal to Created/Running/Stopped).
///
/// Returns the number of threads suspended.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container id is invalid/inactive,
///   is not in the `Running` state, or is already frozen.
pub fn pause(id: ContainerId) -> KernelResult<usize> {
    // Set the frozen flag and snapshot the tracked PIDs under the table lock,
    // then suspend threads outside it — `sched::suspend` takes the scheduler
    // lock, which must never be held under the container table lock.
    let (process_ids, name) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].state != ContainerState::Running {
            return Err(KernelError::InvalidArgument);
        }
        if table.containers[idx].frozen {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].frozen = true;
        Ok((table.containers[idx].pids.clone(), table.containers[idx].name.clone()))
    })?;

    let mut suspended = 0usize;
    for pid in process_ids {
        if let Some(threads) = crate::proc::pcb::get_threads(pid) {
            for task_id in threads {
                if crate::sched::suspend(task_id) {
                    suspended = suspended.saturating_add(1);
                }
            }
        }
    }
    record_event(id, &name, ContainerEventKind::Pause, None);
    Ok(suspended)
}

/// Thaw a frozen container, resuming all of its threads (Docker `unpause`).
///
/// Clears the frozen flag and resumes every suspended thread of every tracked
/// process, the inverse of [`pause`]. Only threads that were suspended by the
/// freeze transition back to ready; a thread that was independently suspended
/// for another reason would also be resumed, but in practice the freezer owns
/// the suspend state of a frozen container's threads.
///
/// Returns the number of threads resumed.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container id is invalid/inactive
///   or is not currently frozen.
pub fn unpause(id: ContainerId) -> KernelResult<usize> {
    let (process_ids, name) = with_table(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        if !table.containers[idx].frozen {
            return Err(KernelError::InvalidArgument);
        }
        table.containers[idx].frozen = false;
        Ok((table.containers[idx].pids.clone(), table.containers[idx].name.clone()))
    })?;

    let mut resumed = 0usize;
    for pid in process_ids {
        if let Some(threads) = crate::proc::pcb::get_threads(pid) {
            for task_id in threads {
                if crate::sched::resume(task_id) {
                    resumed = resumed.saturating_add(1);
                }
            }
        }
    }
    record_event(id, &name, ContainerEventKind::Unpause, None);
    Ok(resumed)
}

/// Report whether a container is currently frozen (Docker `pause` sub-state).
///
/// `None` if the container id is invalid/inactive.
#[must_use]
pub fn is_frozen(id: ContainerId) -> Option<bool> {
    with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return None;
        }
        Some(table.containers[idx].frozen)
    })
}

/// Update a container's live resource limits (Docker `update`).
///
/// Applies new CPU and/or memory limits to the container's cgroup without
/// recreating it, affecting the container's running processes immediately.
/// Each limit is optional — `None` leaves it unchanged:
///
/// - `cpu_percent`: CPU quota as a percentage of one core (`50` = half a
///   core, `200` = two cores). `Some(0)` sets *unlimited*.
/// - `mem_frames`: memory limit in 16 KiB frames. `Some(0)` sets
///   *unlimited*.
///
/// # Errors
///
/// - [`KernelError::InvalidArgument`] if the container id is invalid/inactive,
///   or if the underlying cgroup update fails.
pub fn update_resources(
    id: ContainerId,
    cpu_percent: Option<u64>,
    mem_frames: Option<u64>,
) -> KernelResult<()> {
    let cgroup_id = with_table_ref(|table| {
        let idx = id as usize;
        if idx >= MAX_CONTAINERS || !table.containers[idx].active {
            return Err(KernelError::InvalidArgument);
        }
        Ok(table.containers[idx].cgroup_id)
    })?;

    if let Some(pct) = cpu_percent {
        crate::cgroup::set_cpu_limit(cgroup_id, crate::cgroup::CpuLimit::from_percent(pct))?;
    }
    if let Some(frames) = mem_frames {
        crate::cgroup::set_mem_limit(cgroup_id, crate::cgroup::MemLimit::frames(frames))?;
    }
    Ok(())
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

/// Resolve a DNS `query` on behalf of whatever container owns network
/// namespace `net_ns` (Docker embedded DNS, seen from inside the container).
///
/// Maps `net_ns` → the owning container → its user-defined networks, then asks
/// [`crate::cnetwork::resolve_for_container`] to match the name against the
/// container's same-network peers. Returns `None` for the host namespace
/// (`net_ns == 0`), an unmatched namespace, or a name that no attached network
/// answers to — in every such case the caller falls through to ordinary DNS.
///
/// This is the hook the kernel resolver ([`crate::net::dns::resolve`]) consults
/// *before* querying an upstream server, so a container can reach a peer by
/// name on its shared network exactly as under Docker's 127.0.0.11 resolver.
#[must_use]
pub fn resolve_dns(net_ns: u32, query: &str) -> Option<[u8; 4]> {
    // The host namespace (0) has no embedded resolver.
    if net_ns == 0 || query.is_empty() {
        return None;
    }
    let container_id = with_table_ref(|table| {
        table
            .containers
            .iter()
            .enumerate()
            .find(|(_, ct)| ct.active && ct.net_ns == net_ns)
            .map(|(i, _)| i as ContainerId)
    })?;
    crate::cnetwork::resolve_for_container(container_id, query)
}

/// Remove all terminal (stopped/failed) containers (Docker `container
/// prune`).
///
/// Deletes every container in the [`Stopped`](ContainerState::Stopped) or
/// [`Failed`](ContainerState::Failed) state, freeing each one's namespaces,
/// cgroup, and rootfs mount (see [`delete`]).  `Created` containers are
/// *preserved* — a freshly created container is typically about to be run, so
/// sweeping it would be surprising — as are `Running` (and paused) containers.
///
/// Returns the number of containers actually removed.  A container whose
/// [`delete`] fails (e.g. it transitioned to `Running` between the snapshot and
/// the delete) is skipped and not counted.
pub fn prune() -> usize {
    // Snapshot the terminal container ids first, then delete each — delete()
    // takes the table lock internally, so it must not run while we hold a
    // borrow from list().
    let victims: Vec<ContainerId> = list()
        .into_iter()
        .filter(|(_, _, st)| {
            matches!(st, ContainerState::Stopped | ContainerState::Failed)
        })
        .map(|(id, _, _)| id)
        .collect();
    let mut removed = 0usize;
    for id in victims {
        if delete(id).is_ok() {
            removed = removed.saturating_add(1);
        }
    }
    removed
}

/// Parse a container state name (as printed by [`ContainerState`]'s
/// `Display`) into the corresponding variant, for `docker ps --filter
/// status=...`. Returns `None` for an unrecognised name.
#[must_use]
pub fn parse_state(s: &str) -> Option<ContainerState> {
    match s {
        "created" => Some(ContainerState::Created),
        "running" => Some(ContainerState::Running),
        "stopped" => Some(ContainerState::Stopped),
        "failed" => Some(ContainerState::Failed),
        _ => None,
    }
}

/// Test whether a container's labels satisfy a set of Docker-style label
/// filters (`docker ps --filter label=...`).
///
/// Each filter is `(key, want)`:
/// - `(key, Some(value))` matches only if a label with that exact key and
///   value is present.
/// - `(key, None)` matches if a label with that key is present (any value).
///
/// Returns `true` iff **every** filter is satisfied (Docker AND semantics);
/// an empty filter set always matches.
#[must_use]
pub fn labels_match(labels: &[(String, String)], filters: &[(&str, Option<&str>)]) -> bool {
    filters.iter().all(|(k, want)| {
        labels
            .iter()
            .any(|(lk, lv)| lk == k && want.is_none_or(|w| lv == w))
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

    // Test 15b: multi-network membership (§60) — a container can be attached to
    // N user-defined networks, each with its own veth interface + address.
    {
        let mut mn_cfg = ContainerConfig::new("test-multinet");
        mn_cfg.net_ip = Some([10, 90, 0, 2]);
        mn_cfg.net_mask = Some([255, 255, 255, 0]);
        mn_cfg.net_gateway = Some([10, 90, 0, 1]);
        let ct = create(&mn_cfg).expect("create multinet ct");

        // Record the create-time primary membership (the kshell run flow does
        // this after attaching the primary veth to the bridge).
        record_primary_membership(ct, "primary-net", [10, 90, 0, 2], [10, 90, 0, 0], 24)
            .expect("record primary membership");
        assert!(is_member_of(ct, "primary-net"));
        assert_eq!(info(ct).expect("info").memberships.len(), 1, "one membership after primary");
        // The primary membership reuses the container's primary veth.
        assert_eq!(network_membership(ct, "primary-net").map(|(_, p)| p), Some(true));

        // Attach a second network at runtime — a fresh, distinct interface.
        let vp2 = attach_network(
            ct, "second-net", [10, 91, 0, 5], [10, 91, 0, 0], 24, [10, 91, 0, 1],
        )
        .expect("attach second net");
        assert!(is_member_of(ct, "second-net"));
        assert_eq!(info(ct).expect("info").memberships.len(), 2, "two memberships");
        assert_ne!(
            Some(vp2), info(ct).expect("info").veth_pair,
            "runtime interface distinct from primary veth"
        );
        assert_eq!(network_membership(ct, "second-net").map(|(_, p)| p), Some(false));

        // A duplicate attach to the same network is rejected.
        assert!(matches!(
            attach_network(ct, "second-net", [10, 91, 0, 6], [10, 91, 0, 0], 24, [10, 91, 0, 1]),
            Err(KernelError::AlreadyExists)
        ));

        // The primary network cannot be detached (owned by the lifecycle).
        assert!(matches!(detach_network(ct, "primary-net"), Err(KernelError::InvalidArgument)));

        // Detach the runtime network — membership + interface torn down.
        let freed_ip = detach_network(ct, "second-net").expect("detach second net");
        assert_eq!(freed_ip, [10, 91, 0, 5]);
        assert!(!is_member_of(ct, "second-net"));
        assert_eq!(info(ct).expect("info").memberships.len(), 1, "back to one membership");

        // Detaching a network the container is not on is NotFound.
        assert!(matches!(detach_network(ct, "no-such-net"), Err(KernelError::NotFound)));

        // Delete destroys the primary + any remaining membership veths.
        delete(ct).expect("cleanup multinet ct");
    }
    serial_println!("[container]   Multi-network membership (attach/detach): OK");

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

        // Bracket the entire spawn→teardown window with interrupts disabled.
        // `run()` enqueues a *real, schedulable* init task; with interrupts on,
        // a timer ISR could preempt this boot self-test thread into that task
        // before we tear it down, executing `hello` (which prints one line and
        // exits). The exiting thread's own teardown then races our explicit
        // teardown below — observed as a load-dependent boot HANG (the whole
        // serial log froze mid-test on a heavy boot, BOOT_OK never reached).
        // Disabling interrupts closes that window deterministically: the task
        // is still *registered* (so cgroup billing — the only thing this test
        // verifies that needs a real spawn — is exercised end-to-end), but it
        // can never be *scheduled* before `destroy()` removes it. This is the
        // "synthetic-PID" determinism of Tests 18/19 without giving up the real
        // `run()` path. See known-issues.md B-CONTAINER-JAIL-TESTRACE.
        crate::cpu::without_interrupts(|| {
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
        });

        stop(ct_run).expect("stop run container");
        delete(ct_run).expect("delete run container");
    }
    serial_println!("[container]   Run init process + cgroup billing: OK");

    // Test 18: a container with a configured rootfs jails the processes it
    // registers — `add_process_task` reads the container's `root_path` and
    // re-anchors the registered PID's path resolution under that rootfs, so
    // `..` cannot escape it.
    //
    // This uses a *synthetic*, never-scheduled PID rather than spawning a
    // real init process via `run()`.  The reason is determinism: a real
    // schedulable init process can run and **exit** on another CPU between
    // two of the test's resolves, and on exit its thread teardown calls
    // `namespace::detach(pid)`, which drops `PROCESS_ROOT[pid]` — a later
    // `resolve_path_for(pid, …)` would then return the unjailed input
    // verbatim and the assertion would flake (see known-issues.md
    // B-CONTAINER-JAIL-TESTRACE).  The end-to-end `run()` → cgroup-billing
    // path is already covered by the "Run init process + cgroup billing"
    // test above; the resolution *semantics* (`..` clamping, longest-prefix
    // volume match) are covered deterministically by
    // `namespace::test_process_root` / `test_volume_mounts` (synthetic PIDs).
    // The unique job of this test is to prove the *container layer*
    // (`add_process_task`) installs the configured root onto a registered
    // PID and that `remove_process_task` clears it — neither of which needs
    // a live process.
    {
        const JAIL_PID: u64 = 88890;

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

        // Register a synthetic process: this drives the same wiring path
        // `run()` uses (cgroup/namespace binding + chroot install), but for
        // a PID with no schedulable thread, so it cannot exit mid-test.
        add_process(ct_jail, JAIL_PID).expect("register jailed process");

        // The registered process resolves paths inside its rootfs.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(JAIL_PID, "/bin/sh")
                .expect("resolve jailed path"),
            "/containers/test-jail/rootfs/bin/sh",
        );
        // `..` cannot escape the jail.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(JAIL_PID, "/../../etc/passwd")
                .expect("resolve escape attempt"),
            "/containers/test-jail/rootfs/etc/passwd",
        );

        // Tear down: remove_process_task must also drop the jail.
        remove_process(ct_jail, JAIL_PID).expect("deregister jailed process");
        assert!(
            crate::ipc::namespace::get_root(JAIL_PID).is_none(),
            "jail must be cleared after deregistering the process",
        );

        // Changing the rootfs of a non-Created container is rejected (the
        // `state != Created` guard — exercised here via `stop()` rather than
        // a live process, so the check is deterministic).
        stop(ct_jail).expect("stop jail container");
        assert!(set_root_path(ct_jail, "/other").is_err());
        delete(ct_jail).expect("delete jail container");
    }
    serial_println!("[container]   Rootfs jail (chroot) for init process: OK");

    // Test 19: a container with volume (bind) mounts installs them on the
    // processes it registers, so a guest path under a volume resolves to the
    // host target (escaping the rootfs), while non-volume paths stay jailed.
    //
    // Uses a synthetic, never-scheduled PID for the same determinism reason
    // as Test 18 (see the comment there and B-CONTAINER-JAIL-TESTRACE).
    {
        const VOL_PID: u64 = 88891;

        let vol_cfg = ContainerConfig::new("test-vol-ct").memory(4096);
        let ct_vol = create(&vol_cfg).expect("create vol container");
        set_root_path(ct_vol, "/containers/test-vol/rootfs")
            .expect("set rootfs");
        // Volumes are configurable only before run. `/data` is read-write,
        // `/logs` is read-only (Docker `-v host:guest:ro`).
        add_volume_mount(ct_vol, "/srv/data", "/data", false)
            .expect("add data volume");
        add_volume_mount(ct_vol, "/var/log/test-vol", "/logs", true)
            .expect("add logs volume");
        // Bad args / guest-root volume are rejected.
        assert!(add_volume_mount(ct_vol, "relative", "/x", false).is_err());
        assert!(add_volume_mount(ct_vol, "/host", "rel", false).is_err());
        assert!(add_volume_mount(ct_vol, "/host", "/", false).is_err());
        // Re-adding at an existing guest prefix replaces, not stacks.
        add_volume_mount(ct_vol, "/srv/data2", "/data", false)
            .expect("replace data volume");
        assert_eq!(
            info(ct_vol).unwrap().volumes.len(),
            2,
            "re-mount at /data must replace, not add a third volume",
        );

        // Register a synthetic process via the same wiring path `run()` uses.
        add_process(ct_vol, VOL_PID).expect("register vol process");

        // Volume path escapes the rootfs to the host target.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(VOL_PID, "/data/file.txt")
                .expect("resolve volume path"),
            "/srv/data2/file.txt",
        );
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(VOL_PID, "/logs/app.log")
                .expect("resolve logs volume"),
            "/var/log/test-vol/app.log",
        );
        // Non-volume path stays jailed under the rootfs.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(VOL_PID, "/bin/sh")
                .expect("resolve non-volume path"),
            "/containers/test-vol/rootfs/bin/sh",
        );
        // `..` cannot climb out of a volume into the host.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(VOL_PID, "/data/../escape")
                .expect("resolve escape attempt"),
            "/containers/test-vol/rootfs/escape",
        );

        // Read-only volume enforcement: a write under the read-only `/logs`
        // volume is denied (EROFS), while a write under the read-write `/data`
        // volume and a write to a plain jailed path are allowed.
        assert!(
            crate::ipc::namespace::check_writable_for(VOL_PID, "/logs/app.log")
                .is_err(),
            "write to read-only volume must be denied",
        );
        assert!(
            crate::ipc::namespace::check_writable_for(VOL_PID, "/logs")
                .is_err(),
            "write to the read-only mount point itself must be denied",
        );
        assert!(
            crate::ipc::namespace::check_writable_for(VOL_PID, "/data/file.txt")
                .is_ok(),
            "write to read-write volume must be allowed",
        );
        assert!(
            crate::ipc::namespace::check_writable_for(VOL_PID, "/bin/sh")
                .is_ok(),
            "write to a non-volume jailed path must be allowed",
        );

        // Tear down: remove_process_task must drop the volumes too.
        remove_process(ct_vol, VOL_PID).expect("deregister vol process");
        assert_eq!(
            crate::ipc::namespace::volume_count(VOL_PID),
            0,
            "volumes must be cleared after deregistering the process",
        );

        // Adding a volume to a non-Created container is rejected (the
        // `state != Created` guard, exercised deterministically via stop()).
        stop(ct_vol).expect("stop vol container");
        assert!(add_volume_mount(ct_vol, "/host/x", "/x", false).is_err());
        delete(ct_vol).expect("delete vol container");
    }
    serial_println!("[container]   Volume (bind) mounts for init process: OK");

    // Test 19b: read-only ROOT (Docker `--read-only`).  A container created
    // with `read_only_root` set marks its jailed init process's rootfs
    // read-only: writes into the rootfs are denied (EROFS), while writes into
    // a writable (`:rw`) volume still succeed and reads/path resolution are
    // unaffected.  Uses a synthetic, never-scheduled PID for determinism (same
    // reasoning as Test 19).
    {
        const RO_PID: u64 = 88892;

        let ro_cfg = ContainerConfig::new("test-ro-root").read_only(true);
        assert!(ro_cfg.read_only_root, "builder must set read_only_root");
        let ct_ro = create(&ro_cfg).expect("create ro-root container");
        assert!(
            info(ct_ro).unwrap().read_only_root,
            "ContainerInfo must report read_only_root",
        );
        set_root_path(ct_ro, "/containers/test-ro/rootfs").expect("set rootfs");
        // A writable volume punches a hole through the read-only root.
        add_volume_mount(ct_ro, "/srv/rw", "/scratch", false)
            .expect("add rw volume");

        add_process(ct_ro, RO_PID).expect("register ro process");

        // Writes into the read-only rootfs are denied.
        assert!(
            crate::ipc::namespace::check_writable_for(RO_PID, "/etc/hosts")
                .is_err(),
            "write into a --read-only rootfs must be denied",
        );
        assert!(
            crate::ipc::namespace::check_writable_for(RO_PID, "/bin/sh")
                .is_err(),
            "write into a --read-only rootfs must be denied",
        );
        // The writable volume still permits writes (rw hole through RO root).
        assert!(
            crate::ipc::namespace::check_writable_for(RO_PID, "/scratch/tmp")
                .is_ok(),
            "writable volume must remain writable under --read-only root",
        );
        // Reads / path resolution are unaffected by the read-only flag.
        assert_eq!(
            crate::ipc::namespace::resolve_path_for(RO_PID, "/bin/sh")
                .expect("resolve under ro root"),
            "/containers/test-ro/rootfs/bin/sh",
        );

        // Teardown clears the per-process read-only-root flag (PID-reuse).
        remove_process(ct_ro, RO_PID).expect("deregister ro process");
        assert!(
            !crate::ipc::namespace::is_root_read_only(RO_PID),
            "read-only-root flag must be cleared after deregistering",
        );
        // Toggling the flag is rejected once the container is no longer Created.
        stop(ct_ro).expect("stop ro container");
        assert!(set_read_only_root(ct_ro, false).is_err());
        delete(ct_ro).expect("delete ro container");
    }
    serial_println!("[container]   Read-only root (--read-only) for init process: OK");

    // Test 19c: UTS hostname (Docker `--hostname`).  A container created with a
    // hostname gives its registered processes that name via the per-process UTS
    // override, independent of any rootfs jail.  Uses a synthetic, never-
    // scheduled PID for determinism (same reasoning as Test 19).
    {
        const HN_PID: u64 = 88893;

        let hn_cfg = ContainerConfig::new("test-hostname-ct").hostname("web-01");
        assert_eq!(hn_cfg.hostname, "web-01", "builder must set hostname");
        let ct_hn = create(&hn_cfg).expect("create hostname container");
        assert_eq!(
            info(ct_hn).unwrap().hostname, "web-01",
            "ContainerInfo must report hostname",
        );
        // No rootfs jail set — hostname applies regardless of chroot.
        add_process(ct_hn, HN_PID).expect("register hostname process");
        assert_eq!(
            crate::ipc::namespace::hostname_for(HN_PID).as_deref(),
            Some("web-01"),
            "registered process must see the container hostname",
        );

        // Teardown clears the per-process hostname override (PID-reuse safety).
        remove_process(ct_hn, HN_PID).expect("deregister hostname process");
        assert!(
            crate::ipc::namespace::hostname_for(HN_PID).is_none(),
            "hostname override must be cleared after deregistering",
        );
        // Setting the hostname is rejected once the container is past Created.
        stop(ct_hn).expect("stop hostname container");
        assert!(set_hostname(ct_hn, "late").is_err());
        delete(ct_hn).expect("delete hostname container");
    }
    serial_println!("[container]   UTS hostname (--hostname) for init process: OK");

    // Test 19d: container metadata labels (Docker `--label`).  Labels are pure
    // metadata — stored on the container, surfaced via info(), with no runtime
    // behavior — so this needs no process and stays fully deterministic.
    {
        let lbl_cfg = ContainerConfig::new("test-label-ct")
            .label("role", "web")
            .label("tier", "frontend")
            .label("role", "api"); // last-write-wins replaces "web"
        assert_eq!(lbl_cfg.labels.len(), 2, "duplicate key must not grow the set");
        assert_eq!(
            lbl_cfg.labels.iter().find(|(k, _)| k == "role").map(|(_, v)| v.as_str()),
            Some("api"),
            "last-write-wins must replace the value",
        );
        // Empty keys are ignored.
        let empty_key = ContainerConfig::new("x").label("", "v");
        assert!(empty_key.labels.is_empty(), "empty key must be ignored");

        let ct_lbl = create(&lbl_cfg).expect("create labeled container");
        let got = info(ct_lbl).unwrap().labels;
        assert_eq!(got.len(), 2, "info must report all labels");
        assert!(
            got.iter().any(|(k, v)| k == "tier" && v == "frontend"),
            "info must preserve label values",
        );

        // labels_match: Docker `--filter label=...` AND semantics.
        // got = [("role","api"), ("tier","frontend")].
        assert!(labels_match(&got, &[]), "empty filter matches anything");
        assert!(labels_match(&got, &[("tier", None)]), "key-only match");
        assert!(
            labels_match(&got, &[("tier", Some("frontend"))]),
            "key=value match",
        );
        assert!(
            !labels_match(&got, &[("tier", Some("backend"))]),
            "wrong value must not match",
        );
        assert!(
            !labels_match(&got, &[("missing", None)]),
            "absent key must not match",
        );
        assert!(
            labels_match(&got, &[("role", Some("api")), ("tier", None)]),
            "all filters satisfied (AND) must match",
        );
        assert!(
            !labels_match(&got, &[("role", Some("api")), ("tier", Some("backend"))]),
            "one failing filter must fail the AND",
        );

        // parse_state: round-trips ContainerState Display names; rejects junk.
        assert_eq!(parse_state("created"), Some(ContainerState::Created));
        assert_eq!(parse_state("running"), Some(ContainerState::Running));
        assert_eq!(parse_state("stopped"), Some(ContainerState::Stopped));
        assert_eq!(parse_state("failed"), Some(ContainerState::Failed));
        assert_eq!(parse_state("RUNNING"), None, "case-sensitive");
        assert_eq!(parse_state("bogus"), None);

        delete(ct_lbl).expect("delete labeled container");
    }
    serial_println!("[container]   metadata labels (--label) + filter: OK");

    // Test 19e: notify_init_exit transitions a Running container to Stopped
    // when its init process exits (Docker: a container lives as long as its
    // init).  We drive the state directly via the table rather than scheduling
    // a real process, so the test is deterministic (no run()/spawn race).
    {
        const INIT_PID: u64 = 88894;

        let ct_exit = create(&ContainerConfig::new("test-exit-ct")).expect("create");
        // Force the container into the Running state with a synthetic init PID,
        // simulating a successfully-launched init process.
        with_table(|table| {
            let idx = ct_exit as usize;
            table.containers[idx].state = ContainerState::Running;
            table.containers[idx].init_pid = Some(INIT_PID);
        });
        assert_eq!(info(ct_exit).unwrap().state, ContainerState::Running);
        // A running (never-exited) container has no recorded exit code.
        assert_eq!(info(ct_exit).unwrap().exit_code, None);

        // A non-matching pid must not disturb the container.
        notify_init_exit(INIT_PID.wrapping_add(1), 0);
        assert_eq!(
            info(ct_exit).unwrap().state,
            ContainerState::Running,
            "unrelated pid exit must not stop the container",
        );
        assert_eq!(
            info(ct_exit).unwrap().exit_code, None,
            "unrelated pid exit must not record an exit code",
        );

        // The init pid exiting transitions the container to Stopped and records
        // the init's exit code (Docker's "Exited (N)").
        notify_init_exit(INIT_PID, 7);
        assert_eq!(
            info(ct_exit).unwrap().state,
            ContainerState::Stopped,
            "init exit must stop the container",
        );
        assert_eq!(
            info(ct_exit).unwrap().exit_code, Some(7),
            "init exit must record the exit code",
        );

        // Idempotent: a second notification (or a stale pid) is a harmless
        // no-op now that the container is no longer Running — and must not
        // overwrite the recorded exit code.
        notify_init_exit(INIT_PID, 99);
        assert_eq!(info(ct_exit).unwrap().state, ContainerState::Stopped);
        assert_eq!(
            info(ct_exit).unwrap().exit_code, Some(7),
            "a stale notification must not overwrite the recorded exit code",
        );

        delete(ct_exit).expect("delete exited container");
    }
    serial_println!("[container]   init-exit auto-stop + exit code (notify_init_exit): OK");

    // Test 19f: pids() reports a container's tracked process set (Docker
    // `top`), in registration order, and reflects add/remove.  We use
    // synthetic PIDs with no backing scheduler task — add_process_task pushes
    // the PID into the container's list regardless, and the namespace/cgroup
    // attaches simply no-op for a non-existent task.
    {
        let ct_top = create(&ContainerConfig::new("test-top-ct")).expect("create");
        assert_eq!(pids(ct_top), Some(Vec::new()), "fresh container has no pids");

        add_process(ct_top, 70001).expect("add 70001");
        add_process(ct_top, 70002).expect("add 70002");
        assert_eq!(
            pids(ct_top),
            Some(alloc::vec![70001, 70002]),
            "pids() must report tracked PIDs in registration order",
        );

        remove_process(ct_top, 70001).expect("remove 70001");
        assert_eq!(
            pids(ct_top),
            Some(alloc::vec![70002]),
            "pids() must drop a removed process",
        );

        // Invalid id yields None, distinct from an empty list.
        assert_eq!(pids(MAX_CONTAINERS as ContainerId), None);

        delete(ct_top).expect("delete top container");
        assert_eq!(pids(ct_top), None, "deleted container reports no pids");
    }
    serial_println!("[container]   process listing (pids/top): OK");

    // Test 19g: update_resources() applies new CPU/memory limits to a live
    // container's cgroup (Docker `update`).  We read the limits back through
    // cgroup::stats to confirm they took effect, and verify that a `None`
    // leaves the corresponding limit untouched.
    {
        let ct_upd = create(&ContainerConfig::new("test-update-ct")).expect("create");
        let cg = info(ct_upd).unwrap().cgroup_id;

        // Apply both limits.
        update_resources(ct_upd, Some(50), Some(128)).expect("update both");
        let s = crate::cgroup::stats(cg).expect("cgroup stats");
        assert_eq!(s.cpu_quota, 50, "cpu quota must update to 50%");
        assert_eq!(s.mem_limit, 128, "mem limit must update to 128 frames");

        // Update only CPU (mem unchanged).
        update_resources(ct_upd, Some(150), None).expect("update cpu only");
        let s = crate::cgroup::stats(cg).expect("cgroup stats");
        assert_eq!(s.cpu_quota, 150, "cpu quota must update to 150%");
        assert_eq!(s.mem_limit, 128, "mem limit must remain 128 frames");

        // Update only memory (cpu unchanged).
        update_resources(ct_upd, None, Some(256)).expect("update mem only");
        let s = crate::cgroup::stats(cg).expect("cgroup stats");
        assert_eq!(s.cpu_quota, 150, "cpu quota must remain 150%");
        assert_eq!(s.mem_limit, 256, "mem limit must update to 256 frames");

        // Some(0) means unlimited.
        update_resources(ct_upd, Some(0), Some(0)).expect("update unlimited");
        let s = crate::cgroup::stats(cg).expect("cgroup stats");
        assert_eq!(s.cpu_quota, 0, "cpu quota Some(0) -> unlimited");
        assert_eq!(s.mem_limit, 0, "mem limit Some(0) -> unlimited");

        // Invalid id is rejected.
        assert!(update_resources(MAX_CONTAINERS as ContainerId, Some(1), None).is_err());

        delete(ct_upd).expect("delete update container");
    }
    serial_println!("[container]   live resource update (update_resources): OK");

    // Test 19h: rename() replaces a container's name, truncates to
    // MAX_NAME_LEN, and rejects an empty name / invalid id (Docker `rename`).
    {
        let ct_rn = create(&ContainerConfig::new("old-name")).expect("create");
        assert_eq!(info(ct_rn).unwrap().name, "old-name");

        rename(ct_rn, "new-name").expect("rename");
        assert_eq!(info(ct_rn).unwrap().name, "new-name", "name must update");

        // Empty name is rejected and leaves the old name intact.
        assert!(rename(ct_rn, "").is_err(), "empty name must be rejected");
        assert_eq!(info(ct_rn).unwrap().name, "new-name");

        // Over-long name is truncated to MAX_NAME_LEN bytes.
        let long: alloc::string::String = "x".repeat(MAX_NAME_LEN + 10);
        rename(ct_rn, &long).expect("rename long");
        assert_eq!(
            info(ct_rn).unwrap().name.len(), MAX_NAME_LEN,
            "name must be truncated to MAX_NAME_LEN",
        );

        // Invalid id is rejected.
        assert!(rename(MAX_CONTAINERS as ContainerId, "x").is_err());

        delete(ct_rn).expect("delete renamed container");
    }
    serial_println!("[container]   rename (rename): OK");

    // Test 19i: kill() rejects an invalid id and is a no-op (0 killed) on a
    // container with no tracked processes (Docker `kill`).  The real kill of
    // live processes is exercised by the run/exec integration path, not here
    // (the self-test uses synthetic never-scheduled PIDs).
    {
        // Invalid id is rejected.
        assert!(kill(MAX_CONTAINERS as ContainerId).is_err());

        // A freshly-created container has no processes: nothing to kill.
        let ct_k = create(&ContainerConfig::new("test-kill-ct")).expect("create");
        assert_eq!(kill(ct_k).expect("kill empty"), 0, "no processes to kill");

        // Synthetic tracked PIDs with no backing threads: kill is a no-op for
        // each (kill_process_threads finds no threads), so 0 are killed.
        add_process(ct_k, 71001).expect("add 71001");
        add_process(ct_k, 71002).expect("add 71002");
        assert_eq!(
            kill(ct_k).expect("kill synthetic"), 0,
            "synthetic PIDs have no threads to kill",
        );

        delete(ct_k).expect("delete kill container");
    }
    serial_println!("[container]   kill (kill): OK");

    // Test 19j: published_ports() accessor (Docker `port`) reports a
    // container's `-p` publish specs in insertion order, independent of run
    // state.  Invalid id → None; a container with no publishes → Some(empty).
    {
        use crate::net::nat::NatProto;
        // Invalid id is rejected.
        assert!(published_ports(MAX_CONTAINERS as ContainerId).is_none());

        // A network-capable container with no publishes lists none.
        let ct_pp = create(
            &ContainerConfig::new("test-port-list-ct")
                .memory(4096)
                .network([10, 7, 0, 11], None, None, None),
        )
        .expect("create port-list container");
        assert_eq!(
            published_ports(ct_pp).expect("ports").len(),
            0,
            "a container with no -p publishes lists no ports",
        );

        // After publishing, the accessor reflects the specs in insertion order.
        add_port_publish(ct_pp, NatProto::Tcp, 8080, 80).expect("publish tcp");
        add_port_publish(ct_pp, NatProto::Udp, 5353, 53).expect("publish udp");
        let ports = published_ports(ct_pp).expect("ports after publish");
        assert_eq!(ports.len(), 2, "two published ports expected");
        assert_eq!(ports[0], (NatProto::Tcp, 8080, 80), "first publish: tcp 8080->80");
        assert_eq!(ports[1], (NatProto::Udp, 5353, 53), "second publish: udp 5353->53");

        delete(ct_pp).expect("delete port-list container");
    }
    serial_println!("[container]   published port list (published_ports): OK");

    // Test 19k: wait_status() reports terminal state + exit code (Docker
    // `wait`).  A just-created container is non-terminal; after stop() it
    // becomes terminal.  Invalid id → None.  (The blocking poll loop in the
    // CLI is exercised by the run/exec integration path, not here.)
    {
        // Invalid id is rejected.
        assert!(wait_status(MAX_CONTAINERS as ContainerId).is_none());

        let ct_w = create(&ContainerConfig::new("test-wait-ct")).expect("create");
        // Created is non-terminal: a waiter should keep polling.
        assert_eq!(
            wait_status(ct_w).expect("status"),
            (false, None),
            "a created container is non-terminal with no exit code",
        );

        // After stop(), it is terminal.  Manual stop records no exit code
        // (only an init exit via notify_init_exit does).
        stop(ct_w).expect("stop");
        assert_eq!(
            wait_status(ct_w).expect("status after stop"),
            (true, None),
            "a stopped container is terminal",
        );

        delete(ct_w).expect("delete wait container");
    }
    serial_println!("[container]   wait status (wait_status): OK");

    // Test 19k2: the blocking wait() returns immediately (never parks) when the
    // container is already terminal, and errors on an invalid id.  The actual
    // block-on-init path can't be driven from this boot-thread self-test (it
    // would park the test), but the non-blocking short-circuits are checked
    // here; the parking path is exercised by real `container wait` on running
    // containers during the integration boot.
    {
        // Invalid id → NotFound (not a spurious "Removed").
        assert!(matches!(
            wait(MAX_CONTAINERS as ContainerId),
            Err(KernelError::NotFound)
        ));

        let ct_bw = create(&ContainerConfig::new("test-blockwait-ct")).expect("create");
        stop(ct_bw).expect("stop");
        // Already terminal → Exited immediately, no parking, exit code 0 for a
        // manual stop that recorded no init exit code.
        assert_eq!(
            wait(ct_bw).expect("blocking wait on terminal container"),
            WaitOutcome::Exited(0),
            "wait() on an already-stopped container returns its exit code without blocking",
        );
        delete(ct_bw).expect("delete blockwait container");
    }
    serial_println!("[container]   blocking wait (wait): OK");

    // Test 19k2b: exec_path launches a *real* process inside a Running container
    // and wait_process captures its exit status (Docker `docker exec`
    // end-to-end — D-CONTAINER-EXEC-WAIT).  Unlike Test 17 (which registers a
    // real task but never lets it run, to keep the spawn deterministic), this
    // deliberately runs the process to completion: the exec'd `hello` prints one
    // line and exits 0.  The B-CONTAINER-JAIL-TESTRACE hang came from tearing a
    // *running* task down concurrently with its own exit; that race is closed
    // here by construction — we only reap after observing the zombie (the task's
    // own teardown has already finished), so nothing races it.
    {
        // A real, compiled userspace ELF staged into the container's rootfs.
        static EHELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        // wait_process on a pid that never existed → NoSuchProcess.
        assert!(
            matches!(wait_process(0xDEAD_BEEF), Err(KernelError::NoSuchProcess)),
            "wait_process on a bogus pid must report NoSuchProcess",
        );

        // Build a rootfs under /tmp and stage the exec target at /bin/hello.
        let _ = crate::fs::vfs::Vfs::mkdir("/tmp/ct_exec_root");
        let _ = crate::fs::vfs::Vfs::mkdir("/tmp/ct_exec_root/bin");
        crate::fs::vfs::Vfs::write_file("/tmp/ct_exec_root/bin/hello", EHELLO_ELF)
            .expect("stage exec hello");

        let ct_ex = create(&ContainerConfig::new("test-exec-ct").memory(8192))
            .expect("create exec container");
        set_root_path(ct_ex, "/tmp/ct_exec_root").expect("set exec rootfs");

        // exec on a Created (not-yet-Running) container is rejected.
        assert!(
            matches!(
                exec_path(ct_ex, b"/bin/hello", &[b"/bin/hello"]),
                Err(KernelError::InvalidArgument)
            ),
            "exec on a non-running container must fail",
        );

        // Bring it up as a bare Running container (no init) to exec into.
        start(ct_ex).expect("start exec container");

        // A missing binary in the rootfs → NotFound.
        assert!(
            matches!(
                exec_path(ct_ex, b"/bin/nope", &[b"/bin/nope"]),
                Err(KernelError::NotFound)
            ),
            "exec of an absent binary must report NotFound",
        );

        let cg_ex = cgroup(ct_ex).expect("exec container cgroup");

        // Launch the real process (argv[0] = the guest command path).
        let spawned = exec_path(ct_ex, b"/bin/hello", &[b"/bin/hello"])
            .expect("exec hello into container");

        // Billed to the container cgroup while alive.
        assert_eq!(
            crate::cgroup::stats(cg_ex).map(|s| s.nr_tasks),
            Some(1),
            "exec'd process must be billed to the container cgroup",
        );

        // Let it run to completion.  Single-CPU: yield so the scheduler picks
        // the exec'd task.  Bounded so a stuck process degrades to a test
        // failure, never a boot hang.
        let mut zombified = false;
        for _ in 0..100_000u32 {
            match crate::proc::pcb::state(spawned.pid) {
                Some(crate::proc::pcb::ProcessState::Zombie) | None => {
                    zombified = true;
                    break;
                }
                _ => crate::sched::yield_now(),
            }
        }
        assert!(zombified, "exec'd hello did not exit within the yield budget");

        // wait_process now takes the already-zombie fast path (no parking of the
        // boot thread): it reads and reaps the exit code.  hello exits 0.
        let code = wait_process(spawned.pid).expect("wait_process on exec'd hello");
        assert_eq!(code, 0, "hello exits with status 0");

        // Reaped → the process record is gone.
        assert!(
            crate::proc::pcb::state(spawned.pid).is_none(),
            "reaped process must no longer exist",
        );

        // Force the dead task through reap so the cgroup auto-detach runs, then
        // the container cgroup is empty again (proves teardown accounting is
        // robust to a process that simply exits — see reap_dead_tasks).
        crate::sched::reap_dead_tasks();
        assert_eq!(
            crate::cgroup::stats(cg_ex).map(|s| s.nr_tasks),
            Some(0),
            "cgroup must be empty after the exec'd task is reaped",
        );

        // Unregister the (now-gone) pid from container bookkeeping and clean up.
        let _ = remove_process_task(ct_ex, spawned.pid, spawned.task_id);
        stop(ct_ex).ok();
        delete(ct_ex).expect("delete exec container");
        let _ = crate::fs::vfs::Vfs::remove("/tmp/ct_exec_root/bin/hello");
    }
    serial_println!("[container]   exec + wait (exec_path/wait_process): OK");

    // Test 19k2h: healthcheck state machine (apply_probe_result) + the
    // set_healthcheck/health_status container APIs (Docker `HEALTHCHECK`).
    {
        // start_period = 1s, retries default (3).
        let cfg = crate::oci::HealthcheckConfig {
            test: alloc::vec![
                alloc::string::String::from("CMD"),
                alloc::string::String::from("/bin/health"),
            ],
            interval_ns: 5_000_000_000,
            timeout_ns: 2_000_000_000,
            start_period_ns: 1_000_000_000,
            retries: 3,
        };
        assert!(cfg.is_runnable());
        assert_eq!(cfg.effective_retries(), 3);

        // A pass at any time → Healthy with a cleared streak.
        assert_eq!(
            apply_probe_result(HealthStatus::Starting, 0, 0, 500_000_000, &cfg, 0),
            (HealthStatus::Healthy, 0)
        );

        // A failure *inside* the start period while still Starting is NOT
        // counted (streak preserved, stays Starting).
        assert_eq!(
            apply_probe_result(HealthStatus::Starting, 0, 0, 500_000_000, &cfg, 1),
            (HealthStatus::Starting, 0)
        );

        // Failures after the start period accrue the streak and flip to
        // Unhealthy once the streak reaches the retry count.
        let (s1, k1) = apply_probe_result(HealthStatus::Starting, 0, 0, 2_000_000_000, &cfg, 1);
        assert_eq!((s1, k1), (HealthStatus::Starting, 1));
        let (s2, k2) = apply_probe_result(s1, k1, 0, 2_000_000_000, &cfg, 1);
        assert_eq!((s2, k2), (HealthStatus::Starting, 2));
        let (s3, k3) = apply_probe_result(s2, k2, 0, 2_000_000_000, &cfg, 1);
        assert_eq!((s3, k3), (HealthStatus::Unhealthy, 3));

        // A pass recovers an unhealthy container.
        assert_eq!(
            apply_probe_result(s3, k3, 0, 3_000_000_000, &cfg, 0),
            (HealthStatus::Healthy, 0)
        );

        // Once Healthy, a failure counts immediately even inside the start
        // period (Docker: a pass ends the start-period grace).
        let (fs, fk) =
            apply_probe_result(HealthStatus::Healthy, 0, 0, 500_000_000, &cfg, 1);
        assert_eq!((fs, fk), (HealthStatus::Healthy, 1));

        // set_healthcheck / health_status APIs.
        let cth = create(&ContainerConfig::new("test-health")).expect("create health");
        assert_eq!(health_status(cth), Some(HealthStatus::None));
        set_healthcheck(cth, Some(cfg.clone())).expect("install healthcheck");
        assert_eq!(health_status(cth), Some(HealthStatus::Starting));
        // A disabled (NONE) check clears the healthcheck.
        let none_cfg = crate::oci::HealthcheckConfig {
            test: alloc::vec![alloc::string::String::from("NONE")],
            ..Default::default()
        };
        set_healthcheck(cth, Some(none_cfg)).expect("clear via NONE");
        assert_eq!(health_status(cth), Some(HealthStatus::None));
        // Invalid id → NotFound.
        assert!(matches!(
            set_healthcheck(MAX_CONTAINERS as ContainerId, None),
            Err(KernelError::NotFound)
        ));
        delete(cth).expect("delete health");
    }
    serial_println!("[container]   healthcheck state machine (apply_probe_result): OK");

    // Test 19k2s: live healthcheck supervisor (health_tick) drives a real probe
    // to Healthy.  Deterministic: we call health_tick() directly (the periodic
    // hrtimer is not armed until after this self-test), yielding between ticks so
    // the probe process gets CPU.
    {
        // A real, compiled userspace ELF that exits 0 — the probe target.
        static EHELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );
        // Stage the probe target (exits 0) in a fresh rootfs.
        let _ = crate::fs::vfs::Vfs::mkdir("/tmp/ct_health_root");
        let _ = crate::fs::vfs::Vfs::mkdir("/tmp/ct_health_root/bin");
        crate::fs::vfs::Vfs::write_file("/tmp/ct_health_root/bin/hello", EHELLO_ELF)
            .expect("stage health probe hello");

        let cth2 = create(&ContainerConfig::new("test-health-live").memory(8192))
            .expect("create health-live container");
        set_root_path(cth2, "/tmp/ct_health_root").expect("set health rootfs");
        start(cth2).expect("start health container");

        // A fast-interval, generous-timeout, single-retry CMD probe.
        let hc = crate::oci::HealthcheckConfig {
            test: alloc::vec![
                alloc::string::String::from("CMD"),
                alloc::string::String::from("/bin/hello"),
            ],
            interval_ns: 1_000_000,
            timeout_ns: 10_000_000_000,
            start_period_ns: 0,
            retries: 1,
        };
        set_healthcheck(cth2, Some(hc)).expect("install live healthcheck");
        assert_eq!(health_status(cth2), Some(HealthStatus::Starting));

        // Drive the supervisor: launch, poll, reap → Healthy. Bounded so a
        // stuck probe degrades to a test failure, never a boot hang.
        let mut healthy = false;
        for _ in 0..200_000u32 {
            health_tick();
            if health_status(cth2) == Some(HealthStatus::Healthy) {
                healthy = true;
                break;
            }
            crate::sched::yield_now();
        }
        assert!(healthy, "live healthcheck did not reach Healthy within budget");

        // The probe was reaped and unbound: force any Dead task through reap and
        // confirm the container cgroup is empty (no probe-process leak).
        crate::sched::reap_dead_tasks();
        let cg2 = cgroup(cth2).expect("health container cgroup");
        assert_eq!(
            crate::cgroup::stats(cg2).map(|s| s.nr_tasks),
            Some(0),
            "healthcheck probe must not leak a task in the container cgroup",
        );

        // Clear the check, tear down.
        set_healthcheck(cth2, None).expect("clear live healthcheck");
        assert_eq!(health_status(cth2), Some(HealthStatus::None));
        stop(cth2).ok();
        delete(cth2).expect("delete health-live container");
        let _ = crate::fs::vfs::Vfs::remove("/tmp/ct_health_root/bin/hello");
    }
    serial_println!("[container]   live healthcheck supervisor (health_tick): OK");

    // Test 19k3: diff() reports overlay upper-layer changes (Docker `diff`) —
    // added (upper-only), changed (copied-up/both), and deleted (whiteout)
    // entries, sorted by path.  Builds a real overlay under /tmp (memfs).
    {
        use crate::fs::vfs::Vfs;

        // Invalid id → NotFound.
        assert!(matches!(
            diff(MAX_CONTAINERS as ContainerId),
            Err(KernelError::NotFound)
        ));

        // A container with no overlay → InvalidArgument (nothing to diff).
        let ct_noov = create(&ContainerConfig::new("test-diff-noov")).expect("create");
        assert!(matches!(diff(ct_noov), Err(KernelError::InvalidArgument)));
        delete(ct_noov).expect("delete noov");

        // Build a real overlay: lower has `keep` and `gone`.
        let base = "/tmp/ct_diff_test";
        let lower = "/tmp/ct_diff_test/lower";
        let upper = "/tmp/ct_diff_test/upper";
        let _ = Vfs::mkdir(base);
        let _ = Vfs::mkdir(lower);
        let _ = Vfs::mkdir(upper);
        Vfs::write_file(&alloc::format!("{lower}/keep"), b"orig").expect("write keep");
        Vfs::write_file(&alloc::format!("{lower}/gone"), b"bye").expect("write gone");
        let ov = crate::fs::overlay::create("ct-diff-ov", lower, upper).expect("overlay");
        // Modify keep (copy-up → Both/Changed), add a new file (Upper/Added),
        // delete gone (whiteout → Deleted).
        crate::fs::overlay::write_file(ov, "keep", b"modified").expect("modify keep");
        crate::fs::overlay::write_file(ov, "added.txt", b"new").expect("add file");
        crate::fs::overlay::remove(ov, "gone").expect("remove gone");

        let ct_d = create(&ContainerConfig::new("test-diff-ct")).expect("create");
        set_overlay_id(ct_d, Some(ov)).expect("set overlay id");
        let changes = diff(ct_d).expect("diff");
        assert!(
            changes.iter().any(|c| c.kind == DiffKind::Added && c.path == "/added.txt"),
            "added.txt must be reported as Added",
        );
        assert!(
            changes.iter().any(|c| c.kind == DiffKind::Changed && c.path == "/keep"),
            "keep must be reported as Changed",
        );
        assert!(
            changes.iter().any(|c| c.kind == DiffKind::Deleted && c.path == "/gone"),
            "gone must be reported as Deleted",
        );
        // Output is sorted by path.
        let mut sorted = changes.clone();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(changes, sorted, "diff output must be sorted by path");

        delete(ct_d).expect("delete diff ct");
        let _ = crate::fs::overlay::destroy(ov);
    }
    serial_println!("[container]   diff (overlay changes): OK");

    // Test 19k4: the `oci save`/`oci load` data path — tar a directory tree,
    // write the archive to a host file, read it back, and extract it into a
    // fresh directory — reconstructs the tree byte-for-byte, including nested
    // subdirs.  This exercises the exact tar_tree -> write_file -> read_file ->
    // untar_tree pipeline the save/load commands use (test 19s covers the
    // in-memory tar round-trip but skips the on-disk file write/read hop).
    {
        use crate::fs::vfs::Vfs;

        // Source "image-like" tree: /tmp/ct_saveload_src/{oci-layout, blobs/x}.
        let src = "/tmp/ct_saveload_src";
        let _ = Vfs::mkdir(src);
        let _ = Vfs::mkdir(&alloc::format!("{src}/blobs"));
        Vfs::write_file(&alloc::format!("{src}/oci-layout"), b"{\"v\":\"1.0.0\"}")
            .expect("write oci-layout");
        Vfs::write_file(&alloc::format!("{src}/blobs/x"), b"BLOBDATA")
            .expect("write blob");

        // save: tar the tree, write archive to a host file.
        let archive = tar_tree(src).expect("tar image tree");
        let tar_path = "/tmp/ct_saveload.tar";
        Vfs::write_file(tar_path, &archive).expect("write archive");

        // load: read the archive back, extract into a fresh directory.
        let dst = "/tmp/ct_saveload_dst";
        let back = Vfs::read_file(tar_path).expect("read archive");
        assert_eq!(back, archive, "archive survives the file round-trip");
        untar_tree(dst, &back).expect("extract image tree");

        // The reconstructed tree matches byte-for-byte, nested subdir included.
        assert_eq!(
            Vfs::read_file(&alloc::format!("{dst}/oci-layout")).expect("read layout"),
            b"{\"v\":\"1.0.0\"}",
        );
        assert_eq!(
            Vfs::read_file(&alloc::format!("{dst}/blobs/x")).expect("read blob"),
            b"BLOBDATA",
        );

        // untar_tree rejects a `..`-escaping member without writing (jail guard).
        assert!(untar_tree("", &archive).is_err(), "empty base rejected");

        // Cleanup.
        let _ = Vfs::remove(&alloc::format!("{src}/blobs/x"));
        let _ = Vfs::rmdir(&alloc::format!("{src}/blobs"));
        let _ = Vfs::remove(&alloc::format!("{src}/oci-layout"));
        let _ = Vfs::rmdir(src);
        let _ = Vfs::remove(tar_path);
        let _ = Vfs::remove(&alloc::format!("{dst}/blobs/x"));
        let _ = Vfs::rmdir(&alloc::format!("{dst}/blobs"));
        let _ = Vfs::remove(&alloc::format!("{dst}/oci-layout"));
        let _ = Vfs::rmdir(dst);
    }
    serial_println!("[container]   image save/load data path: OK");

    // Test 19l: pause()/unpause() freeze and thaw a container (Docker
    // `pause`/`unpause`), managing the `frozen` flag and its state-machine
    // guards.  Synthetic PIDs have no backing threads, so the suspended/resumed
    // thread counts are 0 here; the real suspension of live threads is covered
    // by the scheduler's own suspend/resume tests.  This verifies the freezer's
    // lifecycle guards and the `is_frozen` accessor.
    {
        // Invalid id is rejected by every freezer entry point.
        assert!(pause(MAX_CONTAINERS as ContainerId).is_err());
        assert!(unpause(MAX_CONTAINERS as ContainerId).is_err());
        assert!(is_frozen(MAX_CONTAINERS as ContainerId).is_none());

        let ct_pz = create(&ContainerConfig::new("test-pause-ct")).expect("create");
        // A non-running (Created) container cannot be paused.
        assert!(pause(ct_pz).is_err(), "pause requires Running state");
        assert_eq!(is_frozen(ct_pz), Some(false), "not frozen before pause");

        // Make it Running, then pause it.
        start(ct_pz).expect("start");
        add_process(ct_pz, 72001).expect("add 72001"); // synthetic tracked PID
        assert_eq!(pause(ct_pz).expect("pause"), 0, "synthetic PID has no threads");
        assert_eq!(is_frozen(ct_pz), Some(true), "frozen after pause");

        // Double-pause is rejected.
        assert!(pause(ct_pz).is_err(), "already frozen");

        // A process joined while frozen is accepted (and would be suspended).
        add_process(ct_pz, 72002).expect("add 72002 while frozen");

        // Unpause thaws it.
        assert_eq!(unpause(ct_pz).expect("unpause"), 0, "no live threads to resume");
        assert_eq!(is_frozen(ct_pz), Some(false), "not frozen after unpause");
        // Double-unpause is rejected.
        assert!(unpause(ct_pz).is_err(), "not frozen");

        stop(ct_pz).expect("stop");
        delete(ct_pz).expect("delete pause container");
    }
    serial_println!("[container]   pause/unpause (freezer): OK");

    // Test 19m: restart() replays a container's recorded launch command
    // (Docker `restart`).  run_path() records the host VFS path + args; restart
    // resets the container to Created and re-launches, producing a fresh init
    // PID while preserving the container's configuration.  A container that was
    // never run via run_path() has no spec and cannot be restarted.
    {
        static RHELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        // restart with no recorded spec is rejected.
        let ct_nr = create(&ContainerConfig::new("test-norestart-ct")).expect("create");
        assert!(
            restart(ct_nr).is_err(),
            "restart with no recorded launch spec must fail",
        );
        // Invalid id is rejected too.
        assert!(restart(MAX_CONTAINERS as ContainerId).is_err());
        delete(ct_nr).expect("delete norestart container");

        // Stage the init ELF in the VFS so run_path can read it back.
        let elf_path = "/tmp/restart-init.elf";
        crate::fs::vfs::Vfs::write_file(elf_path, RHELLO_ELF).expect("stage init elf");

        let ct_rs = create(&ContainerConfig::new("test-restart-ct").memory(4096))
            .expect("create restart container");
        let pid1 = run_path(ct_rs, elf_path, &[]).expect("run_path initial launch");
        assert_eq!(
            info(ct_rs).unwrap().init_pid, Some(pid1),
            "initial launch records init pid",
        );

        // Restart replays the stored command, yielding a fresh init PID.
        let pid2 = restart(ct_rs).expect("restart");
        assert_ne!(pid1, pid2, "restart must spawn a fresh init process");
        assert_eq!(
            info(ct_rs).unwrap().init_pid, Some(pid2),
            "restart records the new init pid",
        );

        // Teardown: pid1 was force-killed by restart() and is no longer tracked
        // (the reset cleared the pid list); reap its process record.  pid2 is
        // the live init still tracked by the container.
        crate::proc::thread::kill_process_threads(pid1);
        crate::proc::pcb::destroy(pid1);
        if let Some(init_task) = crate::proc::pcb::get_threads(pid2)
            .and_then(|t| t.first().copied())
        {
            let _ = remove_process_task(ct_rs, pid2, init_task);
        }
        crate::proc::thread::kill_process_threads(pid2);
        crate::proc::pcb::destroy(pid2);

        stop(ct_rs).ok();
        delete(ct_rs).expect("delete restart container");
        let _ = crate::fs::vfs::Vfs::remove("/tmp/restart-init.elf");
    }
    serial_println!("[container]   restart (run_path/restart): OK");

    // Test 19n: copy_to_container/copy_from_container move file bytes between
    // the host and a container's rootfs (Docker `cp`), resolving paths under
    // the jail and rejecting escapes.  A container with no rootfs cannot be
    // copied to/from.
    {
        // No-rootfs container: copy is rejected (nothing to resolve against).
        let ct_norf = create(&ContainerConfig::new("test-cp-norootfs")).expect("create");
        assert!(copy_to_container(ct_norf, "/f.txt", b"x").is_err());
        assert!(copy_from_container(ct_norf, "/f.txt").is_err());
        delete(ct_norf).expect("delete no-rootfs container");

        // Use /tmp (a known-writable VFS dir) as the container's rootfs.
        let ct_cp = create(&ContainerConfig::new("test-cp-ct")).expect("create");
        set_root_path(ct_cp, "/tmp").expect("set rootfs");

        // Jail-escape attempts and the root itself are rejected.
        assert!(
            copy_to_container(ct_cp, "/../escape.txt", b"x").is_err(),
            "`..` must not escape the rootfs",
        );
        assert!(
            copy_to_container(ct_cp, "/", b"x").is_err(),
            "the rootfs root is not a file",
        );

        // Round-trip: copy bytes in, read them back out.
        let payload = b"container cp payload";
        copy_to_container(ct_cp, "/cp-test.txt", payload).expect("copy into container");
        let got = copy_from_container(ct_cp, "/cp-test.txt").expect("copy out of container");
        assert_eq!(
            got.as_slice(), payload,
            "round-trip copy must preserve the file bytes",
        );

        let _ = crate::fs::vfs::Vfs::remove("/tmp/cp-test.txt");
        delete(ct_cp).expect("delete cp container");
    }
    serial_println!("[container]   cp (copy_to/from_container): OK");

    // Test 19o: export_rootfs() walks a container's rootfs and packs it into a
    // ustar tar archive (Docker `container export`), preserving the tree layout
    // with names relative to the rootfs root.  A rootfs-less container has
    // nothing to export and is rejected.
    {
        use crate::fs::vfs::Vfs;

        // No-rootfs container: export is rejected.
        let ct_norf = create(&ContainerConfig::new("test-export-norootfs")).expect("create");
        assert!(export_rootfs(ct_norf).is_err(), "no rootfs => no export");
        delete(ct_norf).expect("delete no-rootfs export container");

        // Build a small rootfs under /tmp: a top-level file and a subdir file.
        let root = "/tmp/exp-root";
        let sub = "/tmp/exp-root/sub";
        Vfs::mkdir(root).expect("mkdir export root");
        Vfs::mkdir(sub).expect("mkdir export subdir");
        let top_payload = b"top-level file";
        let nested_payload = b"nested file body";
        Vfs::write_file("/tmp/exp-root/top.txt", top_payload).expect("write top.txt");
        Vfs::write_file("/tmp/exp-root/sub/hello.txt", nested_payload).expect("write hello.txt");

        let ct_exp = create(&ContainerConfig::new("test-export-ct")).expect("create");
        set_root_path(ct_exp, root).expect("set export rootfs");

        let archive = export_rootfs(ct_exp).expect("export rootfs");
        let parsed = crate::fs::tar::parse(&archive).expect("parse exported tar");

        // The subdir directory entry is present (name ends with '/').
        assert!(
            parsed.iter().any(|e| e.name == "sub/"
                && e.kind == crate::fs::tar::EntryKind::Directory),
            "exported archive must contain the 'sub/' directory entry",
        );
        // Both files are present with their original bytes, at relative paths.
        let top = parsed
            .iter()
            .find(|e| e.name == "top.txt")
            .expect("top.txt in archive");
        assert_eq!(
            crate::fs::tar::entry_data(&archive, top).expect("top data"),
            top_payload,
            "top.txt bytes must survive export",
        );
        let nested = parsed
            .iter()
            .find(|e| e.name == "sub/hello.txt")
            .expect("sub/hello.txt in archive");
        assert_eq!(
            crate::fs::tar::entry_data(&archive, nested).expect("nested data"),
            nested_payload,
            "sub/hello.txt bytes must survive export",
        );

        // Cleanup.
        delete(ct_exp).expect("delete export container");
        let _ = Vfs::remove("/tmp/exp-root/sub/hello.txt");
        let _ = Vfs::remove("/tmp/exp-root/top.txt");
        let _ = Vfs::rmdir("/tmp/exp-root/sub");
        let _ = Vfs::rmdir("/tmp/exp-root");
    }
    serial_println!("[container]   export (rootfs -> tar): OK");

    // Test 19p: import_rootfs() extracts a tar archive into a new container's
    // rootfs (Docker `import`), creating parent directories regardless of entry
    // order, rejecting `..` jail-escape names, and rolling forward to a usable
    // container whose root_path points at the extracted tree.
    {
        use crate::fs::tar::{EntryKind, TarWriteEntry};
        use crate::fs::vfs::Vfs;

        // Build an archive with a nested file whose directory entry comes AFTER
        // the file, to prove import creates parents independent of ordering.
        let archive = crate::fs::tar::create(&[
            TarWriteEntry {
                name: String::from("d/a.txt"),
                data: alloc::vec![b'A'; 3],
                kind: EntryKind::File,
                link_target: String::new(),
                mode: 0o644,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
            TarWriteEntry {
                name: String::from("d/"),
                data: Vec::new(),
                kind: EntryKind::Directory,
                link_target: String::new(),
                mode: 0o755,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
            TarWriteEntry {
                name: String::from("b.txt"),
                data: alloc::vec![b'B'; 3],
                kind: EntryKind::File,
                link_target: String::new(),
                mode: 0o644,
                uid: 0,
                gid: 0,
                mtime: 0,
            },
        ]);

        let id = import_rootfs("test-import-ct", &archive, "/tmp/imp-root")
            .expect("import archive into container");
        // Extracted files exist with the right bytes, including the nested one.
        assert_eq!(
            Vfs::read_file("/tmp/imp-root/b.txt").expect("read b.txt"),
            alloc::vec![b'B'; 3],
        );
        assert_eq!(
            Vfs::read_file("/tmp/imp-root/d/a.txt").expect("read d/a.txt"),
            alloc::vec![b'A'; 3],
        );
        // The new container is configured with the extracted rootfs.
        let ci = info(id).expect("imported container exists");
        assert_eq!(ci.root_path, "/tmp/imp-root", "rootfs must be attached");

        // A `..` member name is rejected as a jail escape and leaves no
        // container behind.
        let n_before = active_count();
        let evil = crate::fs::tar::create(&[TarWriteEntry {
            name: String::from("../evil.txt"),
            data: alloc::vec![b'x'; 1],
            kind: EntryKind::File,
            link_target: String::new(),
            mode: 0o644,
            uid: 0,
            gid: 0,
            mtime: 0,
        }]);
        assert!(
            import_rootfs("test-import-evil", &evil, "/tmp/imp-evil").is_err(),
            "`..` archive names must be rejected",
        );
        assert_eq!(active_count(), n_before, "rejected import leaks no container");

        // Cleanup.
        delete(id).expect("delete imported container");
        let _ = Vfs::remove("/tmp/imp-root/b.txt");
        let _ = Vfs::remove("/tmp/imp-root/d/a.txt");
        let _ = Vfs::rmdir("/tmp/imp-root/d");
        let _ = Vfs::rmdir("/tmp/imp-root");
    }
    serial_println!("[container]   import (tar -> rootfs): OK");

    // Test 19q: commit() snapshots a container's rootfs into a new, independent
    // container (Docker `commit`).  The copy is deep — mutating the source after
    // the commit does not change the snapshot.
    {
        use crate::fs::vfs::Vfs;

        // Source rootfs with a single file.
        let src_root = "/tmp/commit-src";
        Vfs::mkdir(src_root).expect("mkdir commit src");
        Vfs::write_file("/tmp/commit-src/data.txt", b"original")
            .expect("write src data");
        let src = create(&ContainerConfig::new("test-commit-src")).expect("create src");
        set_root_path(src, src_root).expect("set src rootfs");

        // Commit into a new container with its own rootfs.
        let snap = commit(src, "test-commit-snap", "/tmp/commit-dst")
            .expect("commit snapshot");
        let ci = info(snap).expect("snapshot container exists");
        assert_eq!(ci.root_path, "/tmp/commit-dst", "snapshot rootfs attached");
        assert_eq!(
            Vfs::read_file("/tmp/commit-dst/data.txt").expect("read snapshot data"),
            b"original",
            "snapshot must capture the source bytes",
        );

        // Mutating the source after the commit must not affect the snapshot.
        Vfs::write_file("/tmp/commit-src/data.txt", b"CHANGED")
            .expect("rewrite src data");
        assert_eq!(
            Vfs::read_file("/tmp/commit-dst/data.txt").expect("re-read snapshot"),
            b"original",
            "snapshot must be independent of later source writes",
        );

        // Cleanup.
        delete(snap).expect("delete snapshot container");
        delete(src).expect("delete src container");
        let _ = Vfs::remove("/tmp/commit-src/data.txt");
        let _ = Vfs::remove("/tmp/commit-dst/data.txt");
        let _ = Vfs::rmdir("/tmp/commit-src");
        let _ = Vfs::rmdir("/tmp/commit-dst");
    }
    serial_println!("[container]   commit (snapshot rootfs): OK");

    // Test 19r: force_delete() removes a running container in one step (Docker
    // `rm -f`), where plain delete() refuses.  A non-running container is still
    // removed, and an invalid id is rejected.
    {
        // A Running container cannot be removed by delete() but can by
        // force_delete().
        let ct_run = create(&ContainerConfig::new("test-forcedel-run")).expect("create");
        start(ct_run).expect("start force-del container");
        assert!(
            delete(ct_run).is_err(),
            "delete() must refuse a running container",
        );
        force_delete(ct_run).expect("force_delete a running container");
        assert!(info(ct_run).is_none(), "force_delete must remove the container");

        // force_delete also works on a non-running (Created) container.
        let ct_new = create(&ContainerConfig::new("test-forcedel-new")).expect("create");
        force_delete(ct_new).expect("force_delete a created container");
        assert!(info(ct_new).is_none(), "force_delete removes a created container");

        // An invalid id is rejected.
        assert!(force_delete(9999).is_err(), "invalid id must be rejected");
    }
    serial_println!("[container]   force_delete (rm -f): OK");

    // Test 19s: recursive cp — a directory subtree round-trips host → container
    // → host through the tar primitives, preserving the nested layout/bytes.
    {
        use crate::fs::vfs::Vfs;

        // Host source directory tree: /tmp/cpdir-src/{a.txt, sub/b.txt}.
        Vfs::mkdir("/tmp/cpdir-src").expect("mkdir cpdir src");
        Vfs::mkdir("/tmp/cpdir-src/sub").expect("mkdir cpdir src/sub");
        Vfs::write_file("/tmp/cpdir-src/a.txt", b"AAA").expect("write a.txt");
        Vfs::write_file("/tmp/cpdir-src/sub/b.txt", b"BBB").expect("write b.txt");

        // Container with /tmp/cpdir-root as rootfs.
        Vfs::mkdir("/tmp/cpdir-root").expect("mkdir cpdir root");
        let ct = create(&ContainerConfig::new("test-cpdir-ct")).expect("create");
        set_root_path(ct, "/tmp/cpdir-root").expect("set rootfs");

        // host dir -> container:/d  (tar the host tree, extract under rootfs).
        let archive = tar_tree("/tmp/cpdir-src").expect("tar host dir");
        copy_dir_to_container(ct, "/d", &archive).expect("copy dir into container");
        // The extracted files land under the rootfs at /tmp/cpdir-root/d/...
        assert_eq!(
            Vfs::read_file("/tmp/cpdir-root/d/a.txt").expect("read extracted a"),
            b"AAA",
        );
        assert_eq!(
            Vfs::read_file("/tmp/cpdir-root/d/sub/b.txt").expect("read extracted b"),
            b"BBB",
        );
        // entry_kind_in_container reports the directory.
        assert_eq!(
            entry_kind_in_container(ct, "/d").expect("kind of /d"),
            crate::fs::vfs::EntryType::Directory,
        );

        // container:/d -> host /tmp/cpdir-out  (tar from container, extract host).
        let back = copy_dir_from_container(ct, "/d").expect("tar container dir");
        untar_tree("/tmp/cpdir-out", &back).expect("extract to host");
        assert_eq!(
            Vfs::read_file("/tmp/cpdir-out/a.txt").expect("read out a"),
            b"AAA",
        );
        assert_eq!(
            Vfs::read_file("/tmp/cpdir-out/sub/b.txt").expect("read out b"),
            b"BBB",
        );

        // Cleanup.
        delete(ct).expect("delete cpdir container");
        let _ = Vfs::remove("/tmp/cpdir-src/sub/b.txt");
        let _ = Vfs::remove("/tmp/cpdir-src/a.txt");
        let _ = Vfs::rmdir("/tmp/cpdir-src/sub");
        let _ = Vfs::rmdir("/tmp/cpdir-src");
        let _ = Vfs::remove("/tmp/cpdir-root/d/sub/b.txt");
        let _ = Vfs::remove("/tmp/cpdir-root/d/a.txt");
        let _ = Vfs::rmdir("/tmp/cpdir-root/d/sub");
        let _ = Vfs::rmdir("/tmp/cpdir-root/d");
        let _ = Vfs::rmdir("/tmp/cpdir-root");
        let _ = Vfs::remove("/tmp/cpdir-out/sub/b.txt");
        let _ = Vfs::remove("/tmp/cpdir-out/a.txt");
        let _ = Vfs::rmdir("/tmp/cpdir-out/sub");
        let _ = Vfs::rmdir("/tmp/cpdir-out");
    }
    serial_println!("[container]   cp -r (recursive directory copy): OK");

    // Test 19t: `logs` — running a container redirects its init process's
    // stdout+stderr to a per-container capture file, and `logs(id)` reads that
    // file back.  As in Test 17 the init process is enqueued but never
    // scheduled (interrupts off + immediate teardown), so this test verifies
    // the capture *wiring* deterministically — run() creates and truncates the
    // log at the expected path, logs() reads its current contents, and delete()
    // removes it — without depending on the child actually executing (the
    // fd-redirect→file delivery itself is covered by the spawn self-tests).
    //
    // The init is spawned with a forced Linux ABI so it gets a Linux fd table
    // for the capture redirect: real containers run glibc/Linux images (the
    // common case), whereas the embedded test ELF is natively marked.  The
    // process never executes, so the forced ABI has no runtime effect here.
    {
        use crate::fs::vfs::Vfs;
        static HELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        let ct = create(&ContainerConfig::new("test-logs-ct").memory(4096))
            .expect("create logs container");

        // Before run: no log recorded yet → NotFound.
        assert!(
            matches!(logs(ct), Err(KernelError::NotFound)),
            "logs on a never-run container must be NotFound",
        );

        let opts = crate::proc::spawn::SpawnOptions::new("logs-init");
        crate::cpu::without_interrupts(|| {
            let pid = run_with_abi(ct, HELLO_ELF, &opts, Some(crate::proc::pcb::AbiMode::Linux))
                .expect("run logs init");

            // run() created and truncated the capture file, so logs() returns
            // Ok with the current (empty) contents.
            let expected_path = log_path_for(ct);
            assert_eq!(
                logs(ct).expect("logs after run"),
                Vec::<u8>::new(),
                "fresh capture log must be empty",
            );
            assert!(
                Vfs::stat(&expected_path).is_ok(),
                "run() must have created the capture file at {expected_path}",
            );

            // Simulate the init process emitting output: write bytes to the
            // capture file and confirm logs() reads them back verbatim (bytes,
            // not UTF-8-forced).
            let payload: &[u8] = b"line1\n\xff\xfebinary\n";
            Vfs::write_file(&expected_path, payload).expect("write simulated log");
            assert_eq!(
                logs(ct).expect("logs after write"),
                payload,
                "logs() must read back the captured bytes verbatim",
            );

            // Tear the never-scheduled init process down (as Test 17 does).
            let init_task = crate::proc::pcb::get_threads(pid)
                .and_then(|t| t.first().copied())
                .expect("logs init has a thread");
            remove_process_task(ct, pid, init_task).expect("detach logs init");
            crate::proc::thread::kill_process_threads(pid);
            crate::proc::pcb::destroy(pid);
        });

        stop(ct).expect("stop logs container");

        // delete() removes the capture file and the container; logs() on the
        // now-gone container is InvalidArgument, and the file is gone.
        let gone_path = log_path_for(ct);
        delete(ct).expect("delete logs container");
        assert!(
            matches!(logs(ct), Err(KernelError::InvalidArgument)),
            "logs on a deleted container must be InvalidArgument",
        );
        assert!(
            Vfs::stat(&gone_path).is_err(),
            "delete() must remove the capture file",
        );
    }
    serial_println!("[container]   logs (stdout/stderr capture): OK");

    // Test 20: a container with published ports (`-p host:container`) installs
    // host-port NAT forwards at run() time, targeting the container's own IP,
    // and tears them down on stop()/delete().  Unlike the jail/volume tests,
    // the forwards are per-netns container state (not per-PID), installed
    // synchronously by run() before it returns and unaffected by the init
    // process's lifetime — so reading them back is deterministic even though
    // run() spawns a real (short-lived) init process.
    {
        use crate::net::nat::NatProto;
        static HELLO_ELF: &[u8] = include_bytes!(
            "../../services/hello/target/x86_64-unknown-none/release/hello"
        );

        // Publishing requires a network IP (the forward target).
        let netless = create(&ContainerConfig::new("test-noport-ct").memory(4096))
            .expect("create netless container");
        assert!(
            add_port_publish(netless, NatProto::Tcp, 8080, 80).is_err(),
            "publishing on a network-less container must fail",
        );
        delete(netless).expect("delete netless container");

        let port_cfg = ContainerConfig::new("test-port-ct")
            .memory(4096)
            .network([10, 7, 0, 9], None, None, None);
        let ct_port = create(&port_cfg).expect("create port container");

        // Publish TCP 8080->80 and UDP 5353->53.
        add_port_publish(ct_port, NatProto::Tcp, 8080, 80).expect("publish tcp");
        add_port_publish(ct_port, NatProto::Udp, 5353, 53).expect("publish udp");
        // Port 0 is rejected on either side.
        assert!(add_port_publish(ct_port, NatProto::Tcp, 0, 80).is_err());
        assert!(add_port_publish(ct_port, NatProto::Tcp, 8081, 0).is_err());
        // Re-publishing the same (proto, host_port) replaces the target.
        add_port_publish(ct_port, NatProto::Tcp, 8080, 8080).expect("replace tcp");
        assert_eq!(
            info(ct_port).unwrap().published_ports.len(),
            2,
            "re-publish at :8080 must replace, not add a third rule",
        );

        // Before run, no NAT rule exists yet.
        assert!(
            crate::net::nat::lookup_port_forward(NatProto::Tcp, 8080).is_none(),
            "forwards must not be installed until run()",
        );

        let opts = crate::proc::spawn::SpawnOptions::new("port-init");
        let pid = run(ct_port, HELLO_ELF, &opts).expect("run port container");

        // After run, the forwards are live, targeting the container IP.
        let tcp = crate::net::nat::lookup_port_forward(NatProto::Tcp, 8080)
            .expect("tcp forward installed");
        assert_eq!(tcp.container_port, 8080);
        assert_eq!(
            tcp.container_ip,
            crate::net::interface::Ipv4Addr::new(10, 7, 0, 9),
        );
        let udp = crate::net::nat::lookup_port_forward(NatProto::Udp, 5353)
            .expect("udp forward installed");
        assert_eq!(udp.container_port, 53);
        // Publishing on a running container is rejected.
        assert!(add_port_publish(ct_port, NatProto::Tcp, 9090, 90).is_err());

        // Tear down the init process (the thread record persists until
        // destroy, so this is safe even if the short-lived init already
        // exited; tolerate a missing thread defensively).
        if let Some(init_task) = crate::proc::pcb::get_threads(pid)
            .and_then(|t| t.first().copied())
        {
            let _ = remove_process_task(ct_port, pid, init_task);
        }
        crate::proc::thread::kill_process_threads(pid);
        crate::proc::pcb::destroy(pid);

        // stop() flushes the forwards (a stopped container publishes nothing).
        stop(ct_port).expect("stop port container");
        assert!(
            crate::net::nat::lookup_port_forward(NatProto::Tcp, 8080).is_none(),
            "stop() must flush published-port forwards",
        );
        assert!(
            crate::net::nat::lookup_port_forward(NatProto::Udp, 5353).is_none(),
            "stop() must flush all of the container's forwards",
        );

        delete(ct_port).expect("delete port container");
    }
    serial_println!("[container]   Published ports (-p) for container: OK");

    // Cleanup.
    stop(ct2).ok(); // may already be stopped
    stop(ct3).ok();
    delete(ct2).expect("cleanup ct2");
    delete(ct3).expect("cleanup ct3");
    delete(ct5).expect("cleanup ct5");
    assert_eq!(active_count(), 0);
    serial_println!("[container]   Cleanup: OK");

    // prune(): remove all Stopped/Failed containers, preserve Created/Running.
    // Runs with active_count()==0 so it can't touch any earlier test fixtures.
    {
        let p1 = create(&ContainerConfig::new("prune-a")).expect("create prune-a");
        let p2 = create(&ContainerConfig::new("prune-b")).expect("create prune-b");
        let p3 = create(&ContainerConfig::new("prune-c")).expect("create prune-c");
        // Bring p1 and p2 to Stopped; leave p3 in Created.
        start(p1).expect("start prune-a");
        stop(p1).expect("stop prune-a");
        start(p2).expect("start prune-b");
        stop(p2).expect("stop prune-b");
        assert_eq!(active_count(), 3);
        let removed = prune();
        assert_eq!(removed, 2, "prune must remove exactly the two stopped");
        assert!(info(p1).is_none(), "stopped prune-a must be gone");
        assert!(info(p2).is_none(), "stopped prune-b must be gone");
        assert!(info(p3).is_some(), "created prune-c must be preserved");
        assert_eq!(active_count(), 1);
        // A second prune with nothing terminal removes nothing.
        assert_eq!(prune(), 0, "prune with no terminal containers is a no-op");
        delete(p3).expect("cleanup prune-c");
        assert_eq!(active_count(), 0);
    }
    serial_println!("[container]   prune (remove stopped): OK");

    // Restart policy: parse round-trips, the pure decision table, and that a
    // created container records + reports its policy via info().
    {
        // parse_restart_policy round-trips for every canonical spelling.
        assert_eq!(parse_restart_policy("no"), Some(RestartPolicy::No));
        assert_eq!(parse_restart_policy(""), Some(RestartPolicy::No));
        assert_eq!(parse_restart_policy("always"), Some(RestartPolicy::Always));
        assert_eq!(
            parse_restart_policy("unless-stopped"),
            Some(RestartPolicy::UnlessStopped),
        );
        assert_eq!(
            parse_restart_policy("on-failure"),
            Some(RestartPolicy::OnFailure(0)),
        );
        assert_eq!(
            parse_restart_policy("on-failure:3"),
            Some(RestartPolicy::OnFailure(3)),
        );
        assert_eq!(parse_restart_policy("bogus"), None);
        assert_eq!(parse_restart_policy("on-failure:x"), None);

        // Display round-trips (used by `container info`).
        assert_eq!(alloc::format!("{}", RestartPolicy::No), "no");
        assert_eq!(alloc::format!("{}", RestartPolicy::Always), "always");
        assert_eq!(
            alloc::format!("{}", RestartPolicy::OnFailure(0)),
            "on-failure"
        );
        assert_eq!(
            alloc::format!("{}", RestartPolicy::OnFailure(5)),
            "on-failure:5"
        );

        // No: never restarts.
        assert!(!should_auto_restart(RestartPolicy::No, 1, false, 0));
        assert!(!should_auto_restart(RestartPolicy::No, 0, false, 0));

        // Always: restarts on any exit code, but a user-stop suppresses it.
        assert!(should_auto_restart(RestartPolicy::Always, 0, false, 9));
        assert!(should_auto_restart(RestartPolicy::Always, 137, false, 0));
        assert!(!should_auto_restart(RestartPolicy::Always, 1, true, 0));

        // UnlessStopped behaves identically to Always in our model.
        assert!(should_auto_restart(RestartPolicy::UnlessStopped, 0, false, 0));
        assert!(!should_auto_restart(
            RestartPolicy::UnlessStopped,
            0,
            true,
            0
        ));

        // OnFailure(N): only non-zero exit, capped at N restarts (0 == ∞).
        assert!(!should_auto_restart(RestartPolicy::OnFailure(2), 0, false, 0));
        assert!(should_auto_restart(RestartPolicy::OnFailure(2), 1, false, 0));
        assert!(should_auto_restart(RestartPolicy::OnFailure(2), 1, false, 1));
        assert!(!should_auto_restart(RestartPolicy::OnFailure(2), 1, false, 2));
        assert!(should_auto_restart(RestartPolicy::OnFailure(0), 1, false, 999));
        assert!(!should_auto_restart(
            RestartPolicy::OnFailure(2),
            1,
            true,
            0
        ));

        // A created container records and reports its policy.
        let rp = create(
            &ContainerConfig::new("restart-policy-ct")
                .restart_policy(RestartPolicy::OnFailure(3)),
        )
        .expect("create restart-policy-ct");
        let inf = info(rp).expect("info restart-policy-ct");
        assert_eq!(inf.restart_policy, RestartPolicy::OnFailure(3));
        assert_eq!(inf.restart_count, 0);

        // set_restart_policy updates it in place (Docker `update --restart`).
        set_restart_policy(rp, RestartPolicy::Always).expect("set_restart_policy");
        assert_eq!(
            info(rp).expect("info after update").restart_policy,
            RestartPolicy::Always,
        );
        assert!(
            set_restart_policy(ContainerId::MAX, RestartPolicy::No).is_err(),
            "set_restart_policy on a bogus id must fail",
        );

        delete(rp).expect("cleanup restart-policy-ct");
        assert_eq!(active_count(), 0);
    }
    serial_println!("[container]   restart policy (parse + decision table + update): OK");

    // Auto-remove (--rm): the config builder records it and info() surfaces it.
    {
        let plain = create(&ContainerConfig::new("rm-off-ct")).expect("create rm-off-ct");
        assert!(
            !info(plain).expect("info rm-off-ct").auto_remove,
            "auto_remove must default to false",
        );
        delete(plain).expect("cleanup rm-off-ct");

        let rm = create(&ContainerConfig::new("rm-on-ct").auto_remove(true))
            .expect("create rm-on-ct");
        assert!(
            info(rm).expect("info rm-on-ct").auto_remove,
            "auto_remove(true) must be recorded",
        );
        delete(rm).expect("cleanup rm-on-ct");
        assert_eq!(active_count(), 0);
    }
    serial_println!("[container]   auto-remove (--rm) config: OK");

    // Creation sequence: strictly increasing across creates, so listings can
    // order by creation time even after slot reuse (Docker `ps -n`/`-l`).
    {
        let a = create(&ContainerConfig::new("seq-a")).expect("create seq-a");
        let b = create(&ContainerConfig::new("seq-b")).expect("create seq-b");
        let sa = info(a).expect("info seq-a").created_seq;
        let sb = info(b).expect("info seq-b").created_seq;
        assert!(sb > sa, "later create must have a higher created_seq");
        // Deleting `a` and creating `c` (which may reuse a's slot) must still
        // yield a created_seq newer than everything before it.
        delete(a).expect("cleanup seq-a");
        let c = create(&ContainerConfig::new("seq-c")).expect("create seq-c");
        let sc = info(c).expect("info seq-c").created_seq;
        assert!(
            sc > sb,
            "created_seq is monotonic across slot reuse (sc={sc} > sb={sb})",
        );
        delete(b).expect("cleanup seq-b");
        delete(c).expect("cleanup seq-c");
        assert_eq!(active_count(), 0);
    }
    serial_println!("[container]   creation sequence (monotonic ordering): OK");

    // Auto-restart crash-loop back-off: exponential from 100 ms, doubling per
    // attempt, capped at 30 s; overflow-safe for large attempt counts.
    {
        assert_eq!(restart_backoff_ns(1), 100_000_000, "attempt 1 = 100 ms");
        assert_eq!(restart_backoff_ns(2), 200_000_000, "attempt 2 = 200 ms");
        assert_eq!(restart_backoff_ns(3), 400_000_000, "attempt 3 = 400 ms");
        assert_eq!(restart_backoff_ns(4), 800_000_000, "attempt 4 = 800 ms");
        // attempt 0 (shouldn't occur — count is >=1 when scheduled) clamps to
        // the base rather than under/overflowing.
        assert_eq!(restart_backoff_ns(0), 100_000_000, "attempt 0 clamps to base");
        // High attempt counts saturate at the 30 s cap (and never overflow).
        assert_eq!(restart_backoff_ns(20), 30_000_000_000, "high count hits cap");
        assert_eq!(restart_backoff_ns(u32::MAX), 30_000_000_000, "u32::MAX hits cap");
        // Monotonically non-decreasing across the whole range.
        let mut prev = 0u64;
        for n in 0..40u32 {
            let d = restart_backoff_ns(n);
            assert!(d >= prev, "back-off must be non-decreasing");
            assert!(d <= 30_000_000_000, "back-off must never exceed the cap");
            prev = d;
        }
    }
    serial_println!("[container]   auto-restart back-off (exponential, capped): OK");

    // Lifecycle event log (Docker `container events`): action strings, ordering,
    // since/limit/filter semantics of `events_snapshot`.
    {
        // 45u: action() strings and Display match Docker's action names.
        assert_eq!(ContainerEventKind::Create.action(), "create");
        assert_eq!(ContainerEventKind::Start.action(), "start");
        assert_eq!(ContainerEventKind::Die.action(), "die");
        assert_eq!(ContainerEventKind::Stop.action(), "stop");
        assert_eq!(ContainerEventKind::Kill.action(), "kill");
        assert_eq!(ContainerEventKind::Pause.action(), "pause");
        assert_eq!(ContainerEventKind::Unpause.action(), "unpause");
        assert_eq!(ContainerEventKind::Restart.action(), "restart");
        assert_eq!(ContainerEventKind::Destroy.action(), "destroy");
        assert_eq!(
            alloc::format!("{}", ContainerEventKind::Die),
            "die",
            "Display must match action()",
        );

        // Baseline: only inspect events we record from here on.
        let base = events_snapshot(0, 0, None)
            .last()
            .map_or(0, |e| e.seq);

        // Record a deterministic burst against synthetic ids.
        let id_x: ContainerId = 30;
        let id_y: ContainerId = 31;
        record_event(id_x, "evt-x", ContainerEventKind::Create, None);
        record_event(id_x, "evt-x", ContainerEventKind::Start, None);
        record_event(id_y, "evt-y", ContainerEventKind::Create, None);
        record_event(id_x, "evt-x", ContainerEventKind::Die, Some(7));

        // since_seq returns exactly the four new events, oldest-first, with
        // strictly increasing seqs.
        let ours = events_snapshot(base, 0, None);
        assert_eq!(ours.len(), 4, "since_seq must return the 4 new events");
        assert_eq!(ours[0].kind, ContainerEventKind::Create);
        assert_eq!(ours[1].kind, ContainerEventKind::Start);
        assert_eq!(ours[3].kind, ContainerEventKind::Die);
        assert_eq!(ours[3].exit_code, Some(7), "die must carry the exit code");
        assert!(ours[0].exit_code.is_none(), "non-die events carry no exit code");
        for w in ours.windows(2) {
            assert!(w[1].seq > w[0].seq, "event seqs must strictly increase");
            assert!(w[1].time_ns >= w[0].time_ns, "event times must be monotonic");
        }

        // limit keeps the most recent N.
        let last_two = events_snapshot(base, 2, None);
        assert_eq!(last_two.len(), 2, "limit must cap the result");
        assert_eq!(last_two[1].kind, ContainerEventKind::Die, "limit keeps newest");

        // filter_id restricts to one container.
        let only_y = events_snapshot(base, 0, Some(id_y));
        assert_eq!(only_y.len(), 1, "filter_id must restrict to one container");
        assert_eq!(only_y[0].id, id_y);
        assert_eq!(only_y[0].name, "evt-y");
        let only_x = events_snapshot(base, 0, Some(id_x));
        assert_eq!(only_x.len(), 3, "filter_id must return all of id_x's events");
    }
    serial_println!("[container]   lifecycle event log (events): OK");

    // Test 46: tmpfs container mounts (Docker `--tmpfs /guest`).  Each spec
    // mounts a fresh in-memory filesystem at a per-container host mountpoint and
    // records it as a writable volume at the guest path.  The backing memfs must
    // be genuinely writable, and `delete` must unmount + remove every owned
    // mountpoint so nothing leaks.
    {
        let tf_cfg = ContainerConfig::new("test-tmpfs-ct").memory(4096);
        let ct_tf = create(&tf_cfg).expect("create tmpfs container");
        set_root_path(ct_tf, "/containers/test-tmpfs/rootfs")
            .expect("set rootfs");

        // Two ephemeral mounts at distinct guest prefixes.
        add_tmpfs_mount(ct_tf, "/tmp").expect("add /tmp tmpfs");
        add_tmpfs_mount(ct_tf, "/run").expect("add /run tmpfs");

        // Bad specs are rejected: relative guest, guest-root, and a duplicate
        // guest prefix (already claimed by the first tmpfs).
        assert!(
            add_tmpfs_mount(ct_tf, "relative").is_err(),
            "tmpfs guest path must be absolute",
        );
        assert!(
            add_tmpfs_mount(ct_tf, "/").is_err(),
            "tmpfs at guest-root must be rejected",
        );
        assert!(
            add_tmpfs_mount(ct_tf, "/tmp").is_err(),
            "duplicate tmpfs guest prefix must be rejected",
        );

        // Each tmpfs is recorded as a writable volume; the two mounts show up in
        // the container's volume list.
        let vols = info(ct_tf).unwrap().volumes;
        assert_eq!(vols.len(), 2, "two tmpfs mounts must appear as volumes");
        let tmp_vol = vols
            .iter()
            .find(|(g, _, _)| g == "/tmp")
            .expect("tmpfs /tmp volume must exist");
        assert!(!tmp_vol.2, "a tmpfs mount is always writable (never read-only)");

        // The backing memfs is genuinely writable: write a file through the host
        // mountpoint and read it back byte-for-byte.
        let tmp_host = &tmp_vol.1;
        let probe = alloc::format!("{tmp_host}/probe.txt");
        crate::fs::vfs::Vfs::write_file(&probe, b"tmpfs-ok")
            .expect("write into tmpfs");
        assert_eq!(
            crate::fs::vfs::Vfs::read_file(&probe).expect("read from tmpfs"),
            b"tmpfs-ok",
            "tmpfs must persist the written bytes in memory",
        );

        // Adding a tmpfs to a non-Created container is rejected (the
        // `state != Created` guard, exercised deterministically via stop()).
        stop(ct_tf).expect("stop tmpfs container");
        assert!(
            add_tmpfs_mount(ct_tf, "/late").is_err(),
            "tmpfs mount on a non-Created container must be rejected",
        );

        // delete() must unmount and remove every owned tmpfs mountpoint.
        let host0 = alloc::format!("{TMPFS_ROOT}/{ct_tf}-0");
        delete(ct_tf).expect("delete tmpfs container");
        assert!(
            !crate::fs::vfs::Vfs::exists(&host0),
            "delete must remove the owned tmpfs mountpoint",
        );
    }
    serial_println!("[container]   tmpfs container mounts (--tmpfs): OK");

    serial_println!("[container] Self-test PASSED (60 tests)");
}
