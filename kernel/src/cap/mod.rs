//! Capability system.
//!
//! Every kernel object is accessed via unforgeable capability handles
//! stored in a per-process capability table.  No ambient authority —
//! if a process doesn't hold a capability, it can't access the resource.
//!
//! ## Capability Types (namespaces)
//!
//! - `fs.*`       — filesystem (read, write, create, delete, execute, metadata)
//! - `net.*`      — networking (connect, listen, socket_rw)
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

// TODO: Capability handle type.
// TODO: Per-process capability table.
// TODO: Capability delegation and revocation.
// TODO: Capability-gated syscall checks.
// TODO: Typed capability definitions for each namespace.
