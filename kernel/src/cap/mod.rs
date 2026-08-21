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
//! ## `resource_id == 0` means *the class*, never *an instance*
//!
//! This is an ABI promise, not an accident of the current call sites, and it
//! is load-bearing for anyone deciding what a capability actually authorises.
//! Read it as: **a capability whose `resource_id` is 0 names no particular
//! object — it is authority over the resource type as a whole. A non-zero id
//! names exactly one object and nothing else.**
//!
//! The distinction is invisible while nothing looks at the id, and every
//! `has_capability_type` check ignores it. It stops being invisible the
//! moment a *predicate* is written on top: `posix`'s `CAP_KILL` projection
//! wanted "may signal any process", and the only thing separating that from
//! "may signal pid 4271" is this field. Both `fork` (step 8) and `spawn`
//! (step 5b) hand every parent a `(Process, <child pid>, SIGNAL)` capability,
//! so a rule that ignored the id would report universal kill authority for
//! any process that had ever forked. See
//! `requests/b-a-does-resource-id-zero-mean-the-class-or-just-an-unknown-pid.md`.
//!
//! Two properties make 0 safe to use as that sentinel, and both are enforced
//! rather than assumed:
//!
//! * **0 is not a reachable instance id for the types that have one.** PIDs
//!   are allocated from 1 upward (`pcb::NEXT_PID`; 0 is the kernel, which has
//!   implicit authority and is never granted a capability), and the same
//!   holds for thread ids and every handle-shaped id. So a class-wide entry
//!   can never be mistaken for an instance one, in either direction.
//! * **0 is chosen, not defaulted.** `SpawnOptions.capabilities` carries the
//!   id as an explicit field of every triple, so a grant site that writes 0
//!   is stating "the class", not omitting a value it did not have yet. The
//!   automatic parent grants above pass a real pid precisely because they
//!   *do* mean one instance.
//!
//! `verify_resource_id_zero_is_class_wide` in this module is the test that
//! keeps the first property true.
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

use crate::error::{KernelError, KernelResult};
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
    ///
    /// `resource_id` is the target PID, or **0 for authority over processes
    /// as a class** — see the module-level "`resource_id == 0` means *the
    /// class*" section, which this type is the reason for. PIDs start at 1,
    /// so the two readings cannot collide.
    ///
    /// The per-child `SIGNAL` capability every parent receives from `fork`
    /// and `spawn` carries the child's PID, so it is an *instance* grant and
    /// must not be read as "may signal anything".
    Process = 6,
    /// A thread (for suspend, resume, priority change).
    ///
    /// `resource_id` is the target thread id, or 0 for the class. Thread ids
    /// likewise start at 1.
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
    /// An AF_INET/AF_INET6 `SOCK_STREAM` socket backed by the userspace
    /// `net.stack` daemon (Path B userspace-netstack cutover — see
    /// design-decisions.md §63/§66).
    ///
    /// A refcounted reference into [`crate::net::socket`] holding one daemon
    /// connection (SHM ring + daemon TCP session); no capability is required to
    /// create one — the handle itself is the authority (the [`Socket`]
    /// capability gates *creating* an AF_INET socket, this type tracks the
    /// per-open resource).  Tracked per-process so the connection is torn down
    /// when an owning process dies, and so `fork()` knows to bump the refcount
    /// in the child.
    ///
    /// [`Socket`]: ResourceType::Socket
    NetSocket = 25,
    /// One end of a pseudo-terminal (`SYS_PTY_CREATE`).
    ///
    /// A refcounted reference into [`crate::tty::pty`]; no capability is
    /// required to create a pty — creating one grants authority over nothing
    /// that existed before it, as with [`Pipe`] and [`StreamSocket`].  Tracked
    /// per-process so the end is closed when an owning process dies, which is
    /// what hangs up a shell whose terminal emulator was killed, and so
    /// `fork()` knows to bump the refcount in the child.
    ///
    /// **Unlike the other handle types here, ownership is actually checked.**
    /// The raw value is `(tty_id << 1) | end`, so it is enumerable, and a
    /// master handle is the authority to *type* into whatever shell is on the
    /// other end — a shell prompt with attacker-chosen input, not a data leak.
    /// Every pty syscall therefore consults
    /// [`crate::proc::pcb::owns_ipc_handle`]; see its doc comment.
    ///
    /// [`Pipe`]: ResourceType::Pipe
    /// [`StreamSocket`]: ResourceType::StreamSocket
    Pty = 26,

    /// The system clock, as an object.
    ///
    /// There is exactly one, so `resource_id` is reserved and is 0.  A process
    /// needs this type with `Rights::WRITE` to set the absolute time
    /// (`clock_settime`, `settimeofday`) or to slew it (`adjtimex`).  Reading
    /// the clock needs nothing and never will.
    ///
    /// Unlike most types here this names no per-open instance: it exists so
    /// that "may set the time" is derived from a held object rather than
    /// asserted.  §312 projects `CAP_SYS_TIME` from it; see design-decisions.md
    /// §269 for why this is its own type, and §350 for the projection (lane B's
    /// request `b-a-three-resource-types-for-clock-ports-and-rlimits`).
    SystemClock = 27,

    /// Authority to bind a local port that the system reserves.
    ///
    /// `resource_id` is a specific port number, or **0 for the class** — the
    /// `resource_id == 0` convention this file documents for
    /// [`Process`](ResourceType::Process)/[`Thread`](ResourceType::Thread).
    /// Port 0 is "pick one for me" in the sockets API and is never a bindable
    /// address, so the two readings cannot collide, exactly as PIDs starting
    /// at 1 keeps `Process` unambiguous.
    ///
    /// The per-port form is what makes this worth having over a boolean — a
    /// web server should hold port 80 and not port 22 — even though the class
    /// grant is what `init` uses today, because a daemon that reads its port
    /// from a config file does not know it at spawn time.  Keeping
    /// `resource_id` *meaning* the port from the start makes the fine-grained
    /// form a grant change later rather than an ABI change.  §312 projects
    /// `CAP_NET_BIND_SERVICE` from it.
    PrivilegedPort = 28,

    /// A process's own resource limits.
    ///
    /// `resource_id` is the target PID, or 0 for the class.  Needed with
    /// `Rights::WRITE` to raise a *hard* limit; lowering a soft limit is
    /// unprivileged and is not gated.  With
    /// [`Rights::MEMORY_LOCK`](crate::cap::Rights::MEMORY_LOCK) it is also the
    /// preimage of `CAP_IPC_LOCK` — locking memory past the per-process quota,
    /// which is not a write to anything and so does not fit `WRITE`.  §312
    /// projects `CAP_SYS_RESOURCE` and `CAP_IPC_LOCK` from it.
    ResourceLimit = 29,
}

impl ResourceType {
    /// The highest discriminant in use.
    ///
    /// Discriminants are contiguous from `Channel = 1`, so this doubles as the
    /// variant count. Consumers that need "every type" iterate `1..=LAST`
    /// rather than keeping their own list — see
    /// [`groups::test_admin_grants_every_resource_type`](crate::cap::groups).
    pub const LAST: u16 = Self::ResourceLimit as u16;

    /// This type's wire discriminant, as sent to userspace.
    ///
    /// Written as an exhaustive match rather than `self as u16` — which would
    /// be one line and generate identical code — so that it doubles as a
    /// **compile-time tripwire**: adding a variant makes this fail to compile,
    /// which lands whoever added it *here*, reading the list below. Two other
    /// exhaustive matches (`ipc::cleanup_handles`, `proc::fork::dup_one`) would
    /// also break, but neither of them can tell you about the four items below,
    /// and a compile error that only says "handle this in fork" invites
    /// handling it in fork and stopping.
    ///
    /// **When this breaks, update — in this order:**
    ///
    /// 1. [`Self::LAST`], above. Everything that iterates the types reads it.
    /// 2. `cap::groups::init`'s `admin_grants` list. The `admin` group is
    ///    documented to grant *every* type and is boot-tested against
    ///    `1..=LAST`, so omitting it turns a green boot red — which is the
    ///    intended outcome, not an inconvenience: it forces the "should admin
    ///    have this?" question to be answered rather than defaulted.
    /// 3. `test_cap_entry_info_abi`'s discriminant pin, if the new variant is
    ///    now the last one.
    /// 4. Lane B's mirrored copy in `posix/src/sys_capability.rs`, which no
    ///    compiler here can see. File a request; do not assume they will
    ///    notice.
    ///
    /// The single or-pattern arm is deliberate — per-variant arms would invite
    /// someone to give the new one a body and consider the matter closed.
    #[must_use]
    pub const fn discriminant(self) -> u16 {
        match self {
            Self::Channel
            | Self::Pipe
            | Self::SharedMemory
            | Self::EventFd
            | Self::CompletionPort
            | Self::Process
            | Self::Thread
            | Self::PortIo
            | Self::DeviceIrq
            | Self::File
            | Self::Socket
            | Self::Timer
            | Self::IoScheduler
            | Self::Service
            | Self::Namespace
            | Self::StreamSocket
            | Self::MemFd
            | Self::Epoll
            | Self::SignalFd
            | Self::Timerfd
            | Self::Inotify
            | Self::AlsaPcm
            | Self::Drm
            | Self::NetRaw
            | Self::NetSocket
            | Self::Pty
            | Self::SystemClock
            | Self::PrivilegedPort
            | Self::ResourceLimit => self as u16,
        }
    }
}

// ---------------------------------------------------------------------------
// Enumeration ABI
// ---------------------------------------------------------------------------

/// One capability as reported to userspace by `SYS_CAP_QUERY`.
///
/// Layout must match the userspace definition exactly (24 bytes, C ABI,
/// 8-aligned).  The explicit `_reserved` array is what makes that true: it
/// fills what would otherwise be implicit padding after `resource_type`, so
/// every byte the kernel copies out is a byte the kernel wrote.
///
/// # What is deliberately absent
///
/// The **handle value**.  Lane B, the requesting consumer, asked not to
/// receive it, and the reasoning generalises: an enumeration answers *what
/// authority exists*, not *which slot holds it*.  A handle is a token that can
/// be used, so putting one into an informational list invites someone to use
/// it — and a list is exactly where a stale one survives longest.
///
/// # What is present beyond the request
///
/// `resource_id`.  The request asked only for type and rights, but the id
/// names the *object* (which channel, which process), not the slot, so it
/// leaks nothing the handle exclusion was protecting.  It is included now
/// because widening a `#[repr(C)]` struct later is an ABI break for every
/// caller, and eight bytes is a cheap premium against one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct CapEntryInfo {
    /// [`ResourceType`] discriminant (the enum is `#[repr(u16)]`).
    pub resource_type: u16,
    /// Reserved; always written as zero.
    pub _reserved: [u16; 3],
    /// [`Rights`] bits, as `Rights::raw()`.
    ///
    /// A `u64`, not a `u32`: `Rights` is a `u64` bitmask.  Twelve bits are
    /// defined today, which is exactly why a caller must not infer the
    /// narrower type — the width is the ABI, not the current occupancy.
    pub rights: u64,
    /// Kernel-internal identifier of the resource (channel id, pid, ...).
    ///
    /// Meaningful only together with `resource_type`, and not usable as a
    /// handle: it names the object, not the caller's access to it.
    pub resource_id: u64,
}

impl CapEntryInfo {
    /// Project a stored [`table::CapEntry`] onto the user-visible form.
    ///
    /// Deliberately not `From`: the conversion is lossy on purpose (the handle
    /// is dropped) and a named constructor keeps that visible at the call
    /// site.
    #[must_use]
    pub fn from_entry(entry: &table::CapEntry) -> Self {
        Self {
            resource_type: entry.resource_type.discriminant(),
            _reserved: [0; 3],
            rights: entry.rights.raw(),
            resource_id: entry.resource_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run capability system self-tests.
pub fn self_test() -> KernelResult<()> {
    serial_println!("[cap] Running capability system self-test...");

    table::self_test()?;
    test_cap_entry_info_abi()?;
    verify_resource_id_zero_is_class_wide()?;

    serial_println!("[cap] Capability system self-test PASSED");
    Ok(())
}

/// Keep `resource_id == 0` usable as the "names the class" sentinel by
/// proving nothing can ever be allocated *at* 0.
///
/// The convention documented at the top of this module is only sound while
/// no real instance id is 0. That is true today because PIDs and thread ids
/// are allocated from 1 upward — but "allocated from 1" is one line in
/// another module, and a future change to make ids start at 0 (or to hash
/// them, or to reuse a slot index) would silently turn a class-wide grant
/// into a grant over process 0 and back again, in a predicate that no longer
/// mentions this file.
///
/// So the property is checked rather than commented. A single PID allocation
/// is enough: the counter is monotonic, so if the *first* id it can still
/// hand out is non-zero, no later one can be 0 either.
///
/// This intentionally does not assert anything about which grant sites use
/// 0 — that is a policy each call site owns. It asserts only the thing they
/// all depend on and none of them can check.
fn verify_resource_id_zero_is_class_wide() -> KernelResult<()> {
    let pid = crate::proc::pcb::peek_next_pid();
    if pid == 0 {
        serial_println!(
            "[cap]   FAIL: the next PID is 0, so a (Process, 0) capability can no longer be \
             told apart from authority over one real process — see the module docs on \
             resource_id == 0"
        );
        return Err(KernelError::InternalError);
    }
    serial_println!("[cap]   resource_id 0 is unreachable as an instance id (next PID {pid}): OK");
    Ok(())
}

/// The `SYS_CAP_QUERY` record layout is an ABI, so pin it with a check.
///
/// Userspace (Lane B's POSIX layer) declares this struct independently; nothing
/// in the compiler ties the two declarations together.  A silent change here —
/// adding a field, widening `rights`, letting the compiler insert padding —
/// would not fail to build on either side, it would simply make one side read
/// the other's bytes at the wrong offsets and report authority the process does
/// not hold.  So the sizes and offsets are asserted, not assumed.
fn test_cap_entry_info_abi() -> KernelResult<()> {
    use core::mem::{align_of, size_of};

    if size_of::<CapEntryInfo>() != 24 || align_of::<CapEntryInfo>() != 8 {
        serial_println!(
            "[cap]   FAIL: CapEntryInfo is {} bytes / align {}, ABI says 24 / 8",
            size_of::<CapEntryInfo>(),
            align_of::<CapEntryInfo>()
        );
        return Err(KernelError::InternalError);
    }

    // Round-trip a known entry: every field must survive the projection, and
    // the reserved bytes must be zero rather than whatever was on the stack.
    let entry = table::CapEntry {
        resource_type: ResourceType::Channel,
        resource_id: 0xDEAD_BEEF_0000_0001,
        rights: Rights::READ_WRITE,
        valid: true,
    };
    let info = CapEntryInfo::from_entry(&entry);
    if info.resource_type != ResourceType::Channel as u16
        || info.resource_id != 0xDEAD_BEEF_0000_0001
        || info.rights != Rights::READ_WRITE.raw()
        || info._reserved != [0; 3]
    {
        serial_println!("[cap]   FAIL: CapEntryInfo::from_entry lost or dirtied a field");
        return Err(KernelError::InternalError);
    }

    // The discriminant is the wire value; a renumbering of ResourceType would
    // silently repoint every caller's decode table, so pin both ends of the
    // range actually used plus one interior anchor.  The *last* variant is
    // pinned deliberately: a new type appended without updating this line is
    // exactly the change that would go unnoticed, and lane B mirrors these
    // numbers by hand in `posix/src/sys_capability.rs`.
    if ResourceType::Channel as u16 != 1
        || ResourceType::NetSocket as u16 != 25
        || ResourceType::SystemClock as u16 != 27
        || ResourceType::ResourceLimit as u16 != 29
    {
        serial_println!("[cap]   FAIL: ResourceType discriminants moved");
        return Err(KernelError::InternalError);
    }

    serial_println!("[cap]   CapEntryInfo ABI: OK");
    Ok(())
}
