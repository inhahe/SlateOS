//! Syscall number definitions.
//!
//! Each syscall has a unique number.  Numbers are grouped by subsystem
//! to avoid merge conflicts when multiple sessions work in parallel.
//!
//! ## Ranges
//!
//! | Range     | Owner           | Purpose                        |
//! |-----------|-----------------|--------------------------------|
//! | 0–199     | kernel-core     | Memory, scheduler, time, misc  |
//! | 200–399   | kernel-ipc      | Channels, pipes, shm, eventfd  |
//! | 400–499   | kernel-security | Capabilities, namespaces       |
//! | 500–599   | kernel-process  | Process/thread lifecycle        |
//! | 600–799   | filesystem      | VFS, file I/O                  |
//! | 800–999   | networking      | Sockets, DNS                   |
//!
//! ## Versioning
//!
//! The syscall table is versioned.  An application declares which API
//! version it targets.  The kernel maintains a dispatch table per
//! version.  When a syscall is deprecated, it remains in older version
//! tables but is absent from newer ones.
//!
//! Current version: **1** (initial).

// ---------------------------------------------------------------------------
// Kernel-core syscalls (0–199)
// ---------------------------------------------------------------------------

/// Yield the current task's time slice.
pub const SYS_YIELD: u64 = 0;

/// Exit the current task.
///
/// `arg0`: exit code.
pub const SYS_EXIT: u64 = 1;

/// Get the current task's ID.
pub const SYS_TASK_ID: u64 = 2;

/// Get the monotonic clock (nanoseconds since boot).
pub const SYS_CLOCK_MONOTONIC: u64 = 10;

/// Sleep for a specified number of nanoseconds.
///
/// `arg0`: duration in nanoseconds.
pub const SYS_SLEEP: u64 = 11;

/// Map memory into the caller's address space.
///
/// `arg0`: virtual address hint (0 = kernel picks).
/// `arg1`: size in bytes (rounded up to frame boundary).
/// `arg2`: flags (`MAP_*` bitfield).
/// `arg3`: physical address (only used with `MAP_MMIO`, must be
///         frame-aligned).
///
/// ## Flags
///
/// | Flag          | Bit | Meaning                                  |
/// |---------------|-----|------------------------------------------|
/// | `MAP_READ`    | 0   | Pages are readable                       |
/// | `MAP_WRITE`   | 1   | Pages are writable                       |
/// | `MAP_EXEC`    | 2   | Pages are executable                     |
/// | `MAP_NOCACHE` | 3   | Disable CPU caching (for MMIO)           |
/// | `MAP_MMIO`    | 4   | Map specific phys addr from `arg3`       |
/// | `MAP_FIXED`   | 5   | Use exact vaddr from `arg0` (must be set)|
///
/// Returns: virtual address of the mapped region, or negative error.
pub const SYS_MMAP: u64 = 20;

/// Mmap flag: pages are readable.
pub const MAP_READ: u64    = 1 << 0;
/// Mmap flag: pages are writable.
pub const MAP_WRITE: u64   = 1 << 1;
/// Mmap flag: pages are executable.
pub const MAP_EXEC: u64    = 1 << 2;
/// Mmap flag: disable CPU caching (for device MMIO).
pub const MAP_NOCACHE: u64 = 1 << 3;
/// Mmap flag: map specific physical address (from `arg3`).
pub const MAP_MMIO: u64    = 1 << 4;
/// Mmap flag: use exact virtual address from `arg0`.
pub const MAP_FIXED: u64   = 1 << 5;

/// Unmap a previously mapped region.
///
/// `arg0`: virtual address (must be frame-aligned).
/// `arg1`: size in bytes (rounded up to frame boundary).
///
/// Frees anonymous mapping frames.  MMIO mapping frames are not
/// freed (they belong to device hardware, not the allocator).
///
/// Returns: 0 on success, negative error.
pub const SYS_MUNMAP: u64 = 21;

/// Debug print (temporary — write a byte string to serial).
///
/// `arg0`: pointer to bytes.
/// `arg1`: length.
///
/// This is a debug-only syscall for early development.  It will be
/// removed or capability-gated once a proper logging service exists.
pub const SYS_DEBUG_PRINT: u64 = 99;

// ---------------------------------------------------------------------------
// IPC syscalls (200–399)
// ---------------------------------------------------------------------------

/// Create a new IPC channel.
///
/// Returns two handles packed into a single u128:
/// - bits 0–63:  endpoint 0 handle
/// - bits 64–127: endpoint 1 handle
///
/// (In practice, returned in rax and rdx.)
pub const SYS_CHANNEL_CREATE: u64 = 200;

/// Send a message on a channel.
///
/// `arg0`: channel handle (u64).
/// `arg1`: pointer to message data.
/// `arg2`: length of message data.
///
/// Returns: 0 on success, negative error code on failure.
pub const SYS_CHANNEL_SEND: u64 = 201;

/// Receive a message from a channel (blocking).
///
/// `arg0`: channel handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: message length on success, negative error code on failure.
pub const SYS_CHANNEL_RECV: u64 = 202;

/// Try to receive a message (non-blocking).
///
/// Same arguments as [`SYS_CHANNEL_RECV`].
///
/// Returns: message length, 0 if empty, negative error code on failure.
pub const SYS_CHANNEL_TRY_RECV: u64 = 203;

/// Close a channel endpoint.
///
/// `arg0`: channel handle.
pub const SYS_CHANNEL_CLOSE: u64 = 204;

/// Block the current task if `*addr == expected`.
///
/// `arg0`: pointer to a 32-bit futex word (must be 4-byte aligned).
/// `arg1`: expected value.
///
/// Returns: 1 if blocked and woken, 0 if value didn't match.
pub const SYS_FUTEX_WAIT: u64 = 210;

/// Wake up to `max_wake` tasks blocked on a futex address.
///
/// `arg0`: pointer to the futex word.
/// `arg1`: maximum number of tasks to wake.
///
/// Returns: number of tasks actually woken.
pub const SYS_FUTEX_WAKE: u64 = 211;

/// Create a one-way pipe.
///
/// Returns two handles packed into `rax` (read end) and `rdx` (write end).
pub const SYS_PIPE_CREATE: u64 = 220;

/// Write bytes to a pipe (blocking).
///
/// `arg0`: write-end pipe handle.
/// `arg1`: pointer to data buffer.
/// `arg2`: number of bytes to write.
///
/// Returns: number of bytes written on success, negative error code on failure.
pub const SYS_PIPE_WRITE: u64 = 221;

/// Read bytes from a pipe (blocking).
///
/// `arg0`: read-end pipe handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: number of bytes read (0 = EOF), negative error code on failure.
pub const SYS_PIPE_READ: u64 = 222;

/// Non-blocking write to a pipe.
///
/// Same arguments as [`SYS_PIPE_WRITE`].
///
/// Returns: bytes written, or `WouldBlock` if buffer is full.
pub const SYS_PIPE_TRY_WRITE: u64 = 223;

/// Non-blocking read from a pipe.
///
/// Same arguments as [`SYS_PIPE_READ`].
///
/// Returns: bytes read, 0 if EOF, or `WouldBlock` if empty.
pub const SYS_PIPE_TRY_READ: u64 = 224;

/// Close a pipe handle (either end).
///
/// `arg0`: pipe handle.
pub const SYS_PIPE_CLOSE: u64 = 225;

/// Create a shared memory region.
///
/// `arg0`: requested size in bytes (rounded up to frame boundary).
///
/// Returns: shared memory handle on success.
pub const SYS_SHM_CREATE: u64 = 230;

/// Query the size of a shared memory region.
///
/// `arg0`: shared memory handle.
///
/// Returns: size in bytes.
pub const SYS_SHM_SIZE: u64 = 231;

/// Close a shared memory handle.
///
/// `arg0`: shared memory handle.
///
/// When the last handle is closed, the region's physical memory is freed.
pub const SYS_SHM_CLOSE: u64 = 232;

/// Create a new eventfd counter.
///
/// `arg0`: initial counter value (typically 0).
///
/// Returns: eventfd handle.
pub const SYS_EVENTFD_CREATE: u64 = 240;

/// Write (signal) an eventfd — add a value to its counter.
///
/// `arg0`: eventfd handle.
/// `arg1`: value to add (must be > 0).
///
/// If the addition would overflow `u64::MAX - 1`, blocks until a
/// reader drains the counter.
///
/// Returns: 0 on success.
pub const SYS_EVENTFD_WRITE: u64 = 241;

/// Read (wait) an eventfd — consume the counter value.
///
/// `arg0`: eventfd handle.
///
/// Blocks until counter > 0, then returns the value and resets to 0.
///
/// Returns: counter value (> 0).
pub const SYS_EVENTFD_READ: u64 = 242;

/// Non-blocking read on an eventfd.
///
/// `arg0`: eventfd handle.
///
/// Returns: counter value, or `WouldBlock` if counter is 0.
pub const SYS_EVENTFD_TRY_READ: u64 = 243;

/// Close an eventfd handle.
///
/// `arg0`: eventfd handle.
///
/// Wakes any blocked reader or writer (they see `ChannelClosed`).
pub const SYS_EVENTFD_CLOSE: u64 = 244;

/// Create a completion port (unified wait multiplexer).
///
/// Returns: completion port handle.
pub const SYS_CP_CREATE: u64 = 250;

/// Register a waitable source with a completion port.
///
/// `arg0`: completion port handle.
/// `arg1`: source type (0=channel, 1=`pipe_read`, 2=`pipe_write`, 3=eventfd).
/// `arg2`: source handle (raw u64).
/// `arg3`: `user_data` — arbitrary u64 returned with events.
///
/// Returns: 0 on success.
pub const SYS_CP_REGISTER: u64 = 251;

/// Unregister a source from a completion port.
///
/// `arg0`: completion port handle.
/// `arg1`: source type.
/// `arg2`: source handle.
///
/// Returns: 0 on success.
pub const SYS_CP_UNREGISTER: u64 = 252;

/// Wait for events on a completion port (blocking).
///
/// `arg0`: completion port handle.
/// `arg1`: pointer to event buffer (array of `CpEventRaw`).
/// `arg2`: buffer capacity (max events to return).
///
/// Returns: number of events written to buffer.
pub const SYS_CP_WAIT: u64 = 253;

/// Non-blocking poll for events on a completion port.
///
/// Same arguments as [`SYS_CP_WAIT`].
///
/// Returns: number of events, or `WouldBlock` if none ready.
pub const SYS_CP_TRY_WAIT: u64 = 254;

/// Close a completion port.
///
/// `arg0`: completion port handle.
pub const SYS_CP_CLOSE: u64 = 255;

/// Post a completion event to a port (manual notification).
///
/// `arg0`: completion port handle.
/// `arg1`: source type.
/// `arg2`: source handle.
///
/// This allows userspace to manually wake a waiter.
pub const SYS_CP_NOTIFY: u64 = 256;

// ---------------------------------------------------------------------------
// Security syscalls (400–499)
// ---------------------------------------------------------------------------

/// Query the calling process's capabilities.
pub const SYS_CAP_QUERY: u64 = 400;

// ---------------------------------------------------------------------------
// Process syscalls (500–599)
// ---------------------------------------------------------------------------

/// Spawn a new process from an ELF binary.
///
/// `arg0`: pointer to path.
/// `arg1`: path length.
/// `arg2`: pointer to argument array.
/// `arg3`: argument count.
///
/// Returns: process ID on success, negative error on failure.
pub const SYS_PROCESS_SPAWN: u64 = 500;

/// Wait for a process to exit.
///
/// `arg0`: process ID (0 = any child).
///
/// Returns: exit code of the reaped process.
pub const SYS_PROCESS_WAIT: u64 = 501;

/// Get the current process ID.
pub const SYS_PROCESS_ID: u64 = 502;

/// Replace the current process image with a new ELF binary.
///
/// `arg0`: pointer to ELF data in user memory.
/// `arg1`: length of the ELF data (bytes).
///
/// On success this syscall does NOT return — the process begins
/// executing the new binary's entry point.  On failure (e.g., invalid
/// ELF, out of memory), returns a negative error code.
pub const SYS_PROCESS_EXEC: u64 = 503;

/// Register an exception handler for the current process.
///
/// `arg0`: handler function address (userspace), or 0 to unregister.
///
/// The handler is called when a hardware exception (divide error,
/// access violation, invalid opcode, etc.) occurs in ring 3.  It
/// receives a pointer to an `ExceptionContext` as its first argument.
///
/// Returns: 0 on success.
pub const SYS_SET_EXCEPTION_HANDLER: u64 = 504;

/// Return from an exception handler, resuming at the saved context.
///
/// `arg0`: pointer to the `ExceptionContext` on the user stack.
///
/// The kernel restores the CPU state from the context and resumes
/// execution at the saved RIP.  If the handler modified the context
/// (e.g., changed RIP to skip the faulting instruction), the new
/// values take effect.
///
/// This syscall does NOT return to the caller — it resumes at the
/// context's RIP.
pub const SYS_EXCEPTION_RETURN: u64 = 505;

/// Force-terminate a process and all its threads.
///
/// `arg0`: target process ID.
/// `arg1`: exit code to set for the process.
///
/// Authority: the caller must be the parent of the target process,
/// or PID 0 (kernel).  Cannot kill PID 0 or the caller's own
/// process — use `SYS_EXIT` for self-termination.
///
/// Returns: number of threads killed on success.
pub const SYS_PROCESS_KILL: u64 = 506;

// ---------------------------------------------------------------------------
// Version info
// ---------------------------------------------------------------------------

/// Current syscall ABI version.
///
/// When the application declares a target version, the kernel uses the
/// corresponding dispatch table.  Version 1 is the initial set.
pub const CURRENT_VERSION: u32 = 1;

/// Maximum supported syscall number.
///
/// The dispatch table is a flat array of this size for O(1) lookup.
/// Sparse — most entries are `None`.
pub const MAX_SYSCALL_NR: usize = 1000;
