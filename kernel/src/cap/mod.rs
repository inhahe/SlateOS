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

pub mod rights;
pub mod table;

pub use rights::Rights;
pub use table::{CapHandle, CapEntry, CapTable};

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
