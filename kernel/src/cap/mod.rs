//! Capability system — unforgeable handles to kernel objects.
//!
//! Every kernel object is accessed via capability handles stored in a
//! per-task (eventually per-process) capability table.  No ambient
//! authority — if a task doesn't hold a capability, it can't access
//! the resource.
//!
//! ## Design
//!
//! Modeled after Fuchsia handles and seL4 capabilities:
//!
//! - A **capability** is a (`resource_type`, `resource_id`, `rights`)
//!   triple stored in a kernel-managed table.
//! - A **capability handle** (`CapHandle`) is an opaque index into the
//!   table.  The handle value itself conveys no information — it's just
//!   a per-task integer.
//! - **Rights** are a bitfield specifying what operations the holder
//!   can perform (read, write, create, delete, etc.).
//! - **Delegation**: a task can grant a subset of its rights to another
//!   task.  You can't create capabilities you don't have.
//! - **Revocation**: the kernel can revoke a capability at any time
//!   (e.g., when a resource is destroyed).
//!
//! ## Capability Types (namespaces)
//!
//! - `fs.*`       — filesystem (read, write, create, delete, execute, metadata)
//! - `net.*`      — networking (connect, listen, `socket_rw`)
//! - `proc.*`     — process management (launch, threads, priority, signal)
//! - `ipc.*`      — IPC (channels, shared memory, pipes, driver comm)
//! - `audio.*`    — audio (play, system sounds, volume)
//! - `ui.*`       — window/display (notifications, fullscreen, always-on-top)
//! - `access.*`   — automation/accessibility (input emulation, screen read)
//! - `resource.*` — resource limits (RAM, CPU, disk, I/O priority)
//! - `admin.*`    — system administration (users, caps, cross-user)
//! - `lib.*`      — library/plugin loading
//! - `push.*`     — push notification registration
//! - `hook.*`     — event hooks (filesystem, process, network, etc.)
//! - `debug.*`    — debugging (attach, memory R/W, breakpoints, tracing)
//!
//! ## Current Scope
//!
//! This module implements the core infrastructure:
//! - Capability handle type and rights bitfield.
//! - Per-task capability table (global for now, per-process later).
//! - Grant, revoke, and check operations.
//! - Self-tests verifying the basic flow.
//!
//! Typed capabilities for each namespace (fs, net, proc, etc.) will
//! be added as those subsystems are implemented.
//!
//! ## Lock Ordering
//!
//! `CAP_TABLE` does not call into the scheduler or other IPC locks.

pub mod audit;
pub mod file_tags;
pub mod groups;
#[allow(dead_code)] // API functions for future syscall interface and timer expiry.
pub mod request;
pub mod rights;
pub mod table;

pub use rights::Rights;
pub use table::CapTable;

use crate::error::KernelResult;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Resource types
// ---------------------------------------------------------------------------

/// The type of kernel resource a capability refers to.
///
/// Each variant corresponds to a class of kernel objects.  New
/// variants are added as subsystems are implemented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum ResourceType {
    /// An IPC channel endpoint.
    Channel = 1,
    /// A pipe (read or write end).
    Pipe = 2,
    /// A shared memory region.
    SharedMemory = 3,
    /// An eventfd counter.
    EventFd = 4,
    /// A completion port.
    CompletionPort = 5,
    /// A process (for kill, wait, inspect operations).
    Process = 6,
    /// A thread (for suspend, resume, priority change).
    Thread = 7,
    /// I/O port access (for userspace drivers).
    ///
    /// `resource_id` is the port number for fine-grained control,
    /// or checked via `has_capability_type` for "any port" access.
    PortIo = 8,
    /// Device IRQ line ownership (for userspace drivers).
    ///
    /// `resource_id` is the IRQ number for fine-grained control,
    /// or checked via `has_capability_type` for "any IRQ" access.
    DeviceIrq = 9,
    /// Filesystem access.
    ///
    /// `resource_id` is reserved for future per-file handles.
    /// Currently checked via `has_capability_type` for general FS
    /// access (any File cap with appropriate rights grants access).
    File = 10,
    /// Network socket access.
    ///
    /// `resource_id` is reserved for future per-socket handles.
    /// Currently checked via `has_capability_type` for general
    /// network access.
    Socket = 11,
    /// Timer resource.
    Timer = 12,
    /// I/O scheduler privilege (for realtime I/O priority class).
    ///
    /// A process needs this resource type with `Rights::IO_REALTIME`
    /// to submit I/O requests at the Realtime priority class.
    /// Without it, Realtime requests are downgraded to BestEffort.
    IoScheduler = 13,
    /// Service registry access.
    ///
    /// Required to register named services (prevents name squatting
    /// by untrusted processes).  Connecting to services does NOT
    /// require this capability — any process can connect.
    ///
    /// `resource_id` is reserved (currently 0).
    /// Rights: WRITE = can register services.
    Service = 14,
    /// Namespace management.
    ///
    /// Required to create namespaces or attach processes to them.
    /// Without this, a process can only operate within its inherited
    /// namespace.
    ///
    /// Rights: WRITE = create/modify/attach namespaces.
    Namespace = 15,
    /// A stream socket endpoint (one end of a `socketpair`).
    ///
    /// A bidirectional, byte-stream IPC object.  Like `Pipe`, no
    /// capability is required to create one — the handle itself is the
    /// authority.  Tracked per-process so the endpoint is closed when an
    /// owning process dies.
    StreamSocket = 16,
    /// An anonymous in-memory file (memfd).
    ///
    /// Created via `memfd_create(2)` on the Linux ABI.  The handle is a
    /// refcounted reference into [`crate::ipc::memfd`]; no capability is
    /// required to create one — the handle itself is the authority.
    /// Tracked per-process so the memfd is released when an owning
    /// process dies, and so `fork()` knows to bump the refcount in the
    /// child.
    MemFd = 17,
    /// An epoll instance (Linux `epoll_create`/`epoll_create1`).
    ///
    /// A refcounted reference into [`crate::ipc::epoll`] holding an
    /// interest set; no capability is required to create one — the handle
    /// itself is the authority.  Tracked per-process so the instance is
    /// released when an owning process dies, and so `fork()` knows to bump
    /// the refcount in the child.
    Epoll = 18,
    /// A signalfd instance (Linux `signalfd`/`signalfd4`).
    ///
    /// A refcounted reference into [`crate::ipc::signalfd`] holding a
    /// signal mask; no capability is required to create one — the handle
    /// itself is the authority.  Tracked per-process so the instance is
    /// released when an owning process dies, and so `fork()` knows to bump
    /// the refcount in the child.
    SignalFd = 19,
    /// A timerfd instance (Linux `timerfd_create`/`settime`/`gettime`).
    ///
    /// A refcounted reference into [`crate::ipc::timerfd`] holding an armed
    /// timer (clock id, next expiry, interval); no capability is required to
    /// create one — the handle itself is the authority.  Tracked per-process
    /// so the instance is released when an owning process dies, and so
    /// `fork()` knows to bump the refcount in the child.
    Timerfd = 20,
    /// An inotify instance (Linux `inotify_init`/`inotify_init1`).
    ///
    /// A refcounted reference into [`crate::ipc::inotify`] holding a table of
    /// filesystem watches; no capability is required to create one — the
    /// handle itself is the authority.  Tracked per-process so the instance
    /// (and every native watch it owns) is released when an owning process
    /// dies, and so `fork()` knows to bump the refcount in the child.
    Inotify = 21,
    /// An ALSA PCM substream instance (Linux `/dev/snd/pcmC0D0p`).
    ///
    /// A refcounted reference into [`crate::ipc::alsa_pcm`] holding one open
    /// PCM substream's state-machine state and the software-mixer slot it
    /// feeds; no capability is required to create one — the handle itself is
    /// the authority.  Tracked per-process so the instance (and its mixer
    /// slot) is released when an owning process dies, and so `fork()` knows to
    /// bump the refcount in the child.
    AlsaPcm = 22,
    /// A DRM card / render-node client instance (Linux `/dev/dri/card0`,
    /// `/dev/dri/renderD128`).
    ///
    /// A refcounted reference into [`crate::drm::card_fd`] holding one open
    /// DRM client's per-fd state (target device, render-node flag, and the
    /// `DRM_CLIENT_CAP_*` opt-ins); no capability is required to create one —
    /// the handle itself is the authority.  Tracked per-process so the
    /// instance is released when an owning process dies, and so `fork()` knows
    /// to bump the refcount in the child.
    Drm = 23,
    /// Raw layer-2 network access to the physical NIC (for the userspace
    /// `netstack` daemon — see design-decisions.md §63).
    ///
    /// Grants unfiltered Ethernet frame send/receive, bypassing the entire
    /// protocol stack and firewall, so it is strictly more privileged than an
    /// ordinary [`ResourceType::Socket`].  A process needs this type with
    /// `Rights::WRITE` to open a raw NIC handle (`SYS_NET_RAW_OPEN`).
    ///
    /// `resource_id` is reserved for future per-interface handles; currently
    /// checked via `has_capability_type` for "any NIC" access.
    NetRaw = 24,
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run capability system self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[cap] Running capability system self-test...");

    table::self_test()?;

    serial_println!("[cap] Capability system self-test PASSED");
    Ok(())
}
