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
/// Mmap flag: lazy (demand-paged) allocation.
///
/// Instead of allocating and mapping physical frames immediately,
/// registers a VMA for the region.  Physical frames are allocated
/// on first access (page fault).  Without this flag, mmap performs
/// committed allocation (the default per design spec: "committed
/// memory by default, lazy allocation opt-in").
pub const MAP_LAZY: u64    = 1 << 6;

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

/// Register to receive interrupts from an IOAPIC IRQ line.
///
/// `arg0`: IRQ number (0–23).
///
/// The IRQ line is unmasked on the IOAPIC and the calling task is
/// registered to receive wakeups when the IRQ fires.  Only one
/// task may be registered per IRQ line.
///
/// Returns: 0 on success, negative error.
///
/// TODO: Require a DeviceIrq capability once the cap system covers
/// hardware resources.
pub const SYS_IRQ_REGISTER: u64 = 30;

/// Wait for an interrupt on a registered IRQ line (blocking).
///
/// `arg0`: IRQ number (0–23).
///
/// If the pending counter is already > 0, consumes it and returns
/// immediately.  Otherwise, blocks the calling task until the IRQ
/// fires.
///
/// Returns: number of interrupts consumed (>= 1).
pub const SYS_IRQ_WAIT: u64 = 31;

/// Release a previously registered IRQ line.
///
/// `arg0`: IRQ number (0–23).
///
/// Masks the IRQ on the IOAPIC and unregisters the task.
///
/// Returns: 0 on success.
pub const SYS_IRQ_RELEASE: u64 = 32;

/// Read from an I/O port.
///
/// `arg0`: port number (0–65535).
/// `arg1`: access width (1 = byte, 2 = word, 4 = dword).
///
/// Returns: the value read from the port.
///
/// TODO: Require a PortIo capability once the cap system covers
/// hardware resources.
pub const SYS_PORT_READ: u64 = 40;

/// Write to an I/O port.
///
/// `arg0`: port number (0–65535).
/// `arg1`: access width (1 = byte, 2 = word, 4 = dword).
/// `arg2`: value to write.
///
/// Returns: 0 on success.
///
/// TODO: Require a PortIo capability once the cap system covers
/// hardware resources.
pub const SYS_PORT_WRITE: u64 = 41;

/// Create a kernel timer.
///
/// `arg0`: duration in nanoseconds until first expiry.
/// `arg1`: flags (bit 0 = periodic).
///
/// Returns: timer handle on success, 0 on failure (table full).
///
/// Timers can be registered with completion ports via
/// `SYS_CP_REGISTER(cp, 5, timer_handle, user_data)`.
pub const SYS_TIMER_CREATE: u64 = 12;

/// Cancel and destroy a timer.
///
/// `arg0`: timer handle.
///
/// Returns: 0 on success, negative error if handle not found.
pub const SYS_TIMER_CANCEL: u64 = 13;

/// Timer flag: periodic (re-arms after each expiry).
pub const TIMER_PERIODIC: u64 = 1 << 0;

/// Set the time slice (in timer ticks) for a scheduler priority level.
///
/// `arg0`: priority level (0–31, where 0 = highest priority).
/// `arg1`: time slice in ticks (must be >= 1).
///
/// Returns: 0 on success, `InvalidArgument` if level is out of range
/// or ticks is 0.
///
/// Requires no special capability currently (will be gated behind a
/// scheduler-admin capability once the capability system covers
/// scheduler configuration).
pub const SYS_SCHED_SET_TIMESLICE: u64 = 50;

/// Get the time slice (in timer ticks) for a scheduler priority level.
///
/// `arg0`: priority level (0–31).
///
/// Returns: time slice in ticks on success, `InvalidArgument` if level
/// is out of range.
pub const SYS_SCHED_GET_TIMESLICE: u64 = 51;

/// Reconfigure all scheduler time slices with a base and increment.
///
/// `arg0`: base time slice in ticks (must be >= 1).
/// `arg1`: increment per priority level.
///
/// Formula: `time_slice[level] = base + level * increment`.
/// Higher-priority levels (lower numbers) get shorter slices for
/// lower latency; lower-priority levels get longer slices for
/// better throughput.
///
/// Returns: 0 on success, `InvalidArgument` if base is 0.
pub const SYS_SCHED_RECONFIGURE: u64 = 52;

/// Apply a named workload profile preset to the scheduler.
///
/// `arg0`: profile ID.
///   - 0 = Desktop (balanced interactivity)
///   - 1 = Server (throughput-oriented)
///   - 2 = Development (quick context switches)
///   - 3 = Gaming (minimal foreground latency)
///
/// Returns: 0 on success, `InvalidArgument` if ID is unknown.
pub const SYS_SCHED_SET_PROFILE: u64 = 53;

/// Query the current workload profile.
///
/// Returns: profile ID (0–3) if the current time slices match a
/// known profile, or `InvalidArgument` if the configuration has been
/// manually tuned.
pub const SYS_SCHED_GET_PROFILE: u64 = 54;

/// Read a kernel tunable parameter (sysctl-like interface).
///
/// `arg0`: parameter ID (e.g., 0 = mm.max_stack_frames).
///
/// Returns: parameter value on success, `InvalidArgument` if the ID
/// is unknown.
pub const SYS_SYSCTL_GET: u64 = 60;

/// Write a kernel tunable parameter.
///
/// `arg0`: parameter ID.
/// `arg1`: new value.
///
/// The value must be within the parameter's valid range.
///
/// Returns: the old value on success, `InvalidArgument` if the ID is
/// unknown or the value is out of range.
pub const SYS_SYSCTL_SET: u64 = 61;

/// Apply a memory workload profile preset.
///
/// `arg0`: profile ID.
///   - 0 = Desktop (committed, moderate stack, kill-largest OOM)
///   - 1 = Server (lazy, large stack, return-error OOM)
///   - 2 = Development (committed, large stack, kill-largest OOM)
///   - 3 = Gaming (committed, large stack, kill-largest OOM, zero-on-free)
///
/// Sets all mm.* sysctl parameters to the profile's preset values.
///
/// Returns: 0 on success, `InvalidArgument` if ID is unknown.
pub const SYS_MM_SET_PROFILE: u64 = 70;

/// Query the current memory workload profile.
///
/// Returns: profile ID (0–3) if the current mm.* parameters match a
/// known profile, or `InvalidArgument` if the configuration has been
/// manually tuned.
pub const SYS_MM_GET_PROFILE: u64 = 71;

/// Apply a unified system workload profile (scheduler + memory).
///
/// `arg0`: profile ID (0–3).
///
/// Configures both scheduler time slices and mm.* sysctl parameters
/// in one call.  Equivalent to calling `SYS_SCHED_SET_PROFILE` and
/// `SYS_MM_SET_PROFILE` with the same profile ID.
///
/// Returns: 0 on success, `InvalidArgument` if ID is unknown.
pub const SYS_SYSTEM_SET_PROFILE: u64 = 80;

/// Debug print (temporary — write a byte string to serial).
///
/// `arg0`: pointer to bytes.
/// `arg1`: length.
///
/// This is a debug-only syscall for early development.  It will be
/// removed or capability-gated once a proper logging service exists.
pub const SYS_DEBUG_PRINT: u64 = 99;

/// Read kernel log entries (JSON-lines) from the ring buffer.
///
/// `arg0`: sequence number — read entries newer than this value.
///         Pass `u64::MAX` (0xFFFF_FFFF_FFFF_FFFF) to start from
///         the oldest available entry.
/// `arg1`: pointer to output buffer.
/// `arg2`: buffer capacity in bytes.
///
/// Returns: number of entries read (in `value`).  The newest
/// sequence number is returned in `value2` — pass it as `arg0`
/// on the next call to read only new entries.
///
/// Each entry is a single JSON object followed by `\n`:
/// ```json
/// {"t":1234,"l":"info","m":"sched","msg":"Task 5 spawned"}
/// ```
pub const SYS_LOG_READ: u64 = 102;

/// Write bytes to the framebuffer console.
///
/// `arg0`: pointer to byte buffer.
/// `arg1`: length of buffer.
///
/// Writes to the framebuffer console and mirrors to serial.  Handles
/// ASCII control characters (`\n`, `\r`, `\t`).  Non-ASCII bytes are
/// rendered as their glyph.
///
/// This is a kernel-provided bootstrap console.  It will be replaced
/// by a userspace console server in the future.
///
/// Returns: number of bytes written.
pub const SYS_CONSOLE_WRITE: u64 = 100;

/// Read one character from the keyboard (blocking).
///
/// `arg0`: pointer to a 1-byte buffer.
///
/// Blocks (via HLT) until a key is pressed.  Returns the ASCII code
/// of the key.  Non-printable keys (function keys, arrows) return 0.
///
/// This is a kernel-provided bootstrap console.  It will be replaced
/// by a userspace console server / terminal emulator.
///
/// Returns: 1 on success (one byte read), or negative error.
pub const SYS_CONSOLE_READ_CHAR: u64 = 101;

/// Non-blocking read of one character from the keyboard.
///
/// `arg0`: pointer to a 1-byte buffer.
///
/// If a keypress is buffered, writes the ASCII code into the buffer
/// and returns 1.  If no key is available, returns `WouldBlock` (-4)
/// immediately without blocking.
///
/// This is a kernel-provided bootstrap console.  It will be replaced
/// by a userspace console server / terminal emulator.
///
/// Returns: 1 on success, `WouldBlock` if no key available.
pub const SYS_CONSOLE_TRY_READ_CHAR: u64 = 103;

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

/// Receive a message with a timeout (nanoseconds).
///
/// `arg0`: channel handle.
/// `arg1`: pointer to caller-provided buffer for the message.
/// `arg2`: buffer size.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: message length on success, `TimedOut` if deadline expires,
/// `ChannelClosed` if peer closed.
pub const SYS_CHANNEL_RECV_TIMEOUT: u64 = 205;

/// Send a message with capability transfer.
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: message data length.
/// `arg3`: pointer to array of capability handle u64s.
/// `arg4`: number of capability handles.
///
/// Caps are moved from the sender's process table into the message
/// (move semantics — sender loses the handles).
///
/// Returns: 0 on success, negative error code on failure.
/// All-or-nothing: if any cap handle is invalid, nothing is sent.
pub const SYS_CHANNEL_SEND_CAPS: u64 = 206;

/// Receive a message with capability transfer (blocking).
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message receive buffer.
/// `arg2`: message buffer capacity.
/// `arg3`: pointer to output array for new capability handle u64s.
/// `arg4`: capacity of the cap handle output array.
///
/// Returns (in rax): message data length.
/// Returns (in rdx): number of capability handles received.
///
/// The received cap handles are new values in the receiver's table.
pub const SYS_CHANNEL_RECV_CAPS: u64 = 207;

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

/// Lock a PI (Priority Inheritance) futex.
///
/// `arg0`: virtual address of the futex word (must be 4-byte aligned).
///
/// The futex word format: bits 0–29 = owner task ID, bit 30 = waiters
/// flag.  If the lock is free (word == 0), acquires it atomically.
/// If contended, blocks the caller and boosts the lock holder's
/// priority to prevent priority inversion.
///
/// Returns: 0 on success, negative error.
pub const SYS_FUTEX_LOCK_PI: u64 = 212;

/// Unlock a PI (Priority Inheritance) futex.
///
/// `arg0`: virtual address of the futex word (must be 4-byte aligned).
///
/// Releases the lock and transfers ownership to the highest-priority
/// waiter (if any).  Restores the caller's inherited priority.
///
/// Returns: 0 on success, negative error.
pub const SYS_FUTEX_UNLOCK_PI: u64 = 213;

/// Block on a futex with a timeout (nanoseconds).
///
/// `arg0`: pointer to a 32-bit futex word (must be 4-byte aligned).
/// `arg1`: expected value.
/// `arg2`: timeout in nanoseconds (0 = check only, never block).
///
/// Returns: 1 if blocked and woken, 0 if value didn't match,
/// `TimedOut` if timeout expired.
pub const SYS_FUTEX_WAIT_TIMEOUT: u64 = 214;

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

/// Read from a pipe with a timeout (nanoseconds).
///
/// `arg0`: pipe handle (read end).
/// `arg1`: pointer to caller-provided buffer.
/// `arg2`: buffer size.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: bytes read, 0 if EOF, `TimedOut` if deadline expires.
pub const SYS_PIPE_READ_TIMEOUT: u64 = 226;

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
/// `arg1`: source type (0=channel, 1=pipe_read, 2=pipe_write, 3=eventfd,
///         4=process_exit, 5=timer, 6=semaphore, 7=io_completion).
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

/// Create a new io_ring (io_uring-style submission queue).
///
/// `arg0`: number of submission queue entries (rounded up to power of 2).
/// `arg1`: number of completion queue entries (rounded up to power of 2).
///
/// Returns: ring handle in `rax`, header virtual address in `rdx`.
/// The physical frame addresses for user-space mapping can be queried
/// separately if needed.
pub const SYS_IO_RING_SETUP: u64 = 260;

/// Submit and/or reap io_ring entries.
///
/// `arg0`: ring handle.
/// `arg1`: maximum number of SQEs to process (0 = drain all pending).
///
/// Reads up to `to_submit` entries from the submission queue, executes
/// each operation, and posts results to the completion queue.
///
/// Returns: number of SQEs processed.
pub const SYS_IO_RING_ENTER: u64 = 261;

/// Destroy an io_ring and free its resources.
///
/// `arg0`: ring handle.
///
/// Unmaps ring memory and frees physical frames.  The ring must not
/// be in use by another thread.
///
/// Returns: 0 on success.
pub const SYS_IO_RING_DESTROY: u64 = 262;

// ---------------------------------------------------------------------------
// IPC semaphore syscalls (270–275)
// ---------------------------------------------------------------------------

/// Create a new IPC semaphore.
///
/// `arg0`: initial count (0 = empty).
/// `arg1`: maximum count (0 = use default max).
///
/// Returns: semaphore handle (> 0), or negative error.
pub const SYS_SEM_CREATE: u64 = 270;

/// Signal (release) a semaphore — increment count.
///
/// `arg0`: semaphore handle.
/// `arg1`: count to add (typically 1).
///
/// Wakes blocked waiters (up to `count`).
///
/// Returns: 0 on success, or negative error.
pub const SYS_SEM_SIGNAL: u64 = 271;

/// Wait (acquire) a semaphore — decrement by 1.
///
/// `arg0`: semaphore handle.
///
/// Blocks if count is 0 until a signal occurs.
///
/// Returns: 0 on success, or negative error.
pub const SYS_SEM_WAIT: u64 = 272;

/// Try-wait (non-blocking acquire) — decrement if count > 0.
///
/// `arg0`: semaphore handle.
///
/// Returns: 0 on success, or negative error (WouldBlock if count=0).
pub const SYS_SEM_TRY_WAIT: u64 = 273;

/// Close (destroy) a semaphore.
///
/// `arg0`: semaphore handle.
///
/// Wakes all blocked waiters with ChannelClosed.
///
/// Returns: 0 on success.
pub const SYS_SEM_CLOSE: u64 = 274;

/// Wait (acquire) a semaphore with a timeout (nanoseconds).
///
/// `arg0`: semaphore handle.
/// `arg1`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: 0 on success, `TimedOut` if deadline expires.
pub const SYS_SEM_WAIT_TIMEOUT: u64 = 275;

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

/// Non-blocking wait for a process to exit.
///
/// `arg0`: process ID.
///
/// If the process has exited (is a zombie), reaps it and returns
/// the exit code.  If the process is still running, returns
/// `-EAGAIN` (= `-11`) immediately without blocking.
///
/// Returns: exit code on success, `-11` if still running, other
///          negative error if the PID is invalid.
pub const SYS_PROCESS_TRY_WAIT: u64 = 507;

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

/// Signal that the calling process is fully initialized ("ready").
///
/// Services call this after completing startup to inform the service
/// manager (init) that they are ready to accept requests.  This is
/// the foundation for dependency-based startup ordering.
///
/// Returns: 0 on success.
pub const SYS_NOTIFY_READY: u64 = 508;

/// Query whether a process has signaled readiness.
///
/// `arg0`: process ID to query.
///
/// Returns: 1 if the process has called `SYS_NOTIFY_READY`, 0 if it
/// exists but hasn't, negative error if the PID doesn't exist.
pub const SYS_PROCESS_IS_READY: u64 = 509;

/// Create a new thread in the calling process.
///
/// `arg0`: entry point address (ring 3 RIP).
/// `arg1`: stack pointer (ring 3 RSP, must already be mapped).
/// `arg2`: priority (0–31, or `u64::MAX` for default priority).
///
/// Creates a new thread that shares the calling process's address
/// space.  The thread begins executing at `entry_rip` with stack
/// pointer `user_rsp` in ring 3.  The thread gets its own kernel
/// stack for syscall/interrupt handling.
///
/// Returns: new thread's task ID on success, negative error on failure.
pub const SYS_THREAD_CREATE: u64 = 510;

/// Exit the current thread with an exit value.
///
/// `arg0`: exit value (i64).
///
/// Terminates the calling thread.  If this is the last thread in
/// the process, the process becomes a zombie.  The exit value can
/// be retrieved by another thread via `SYS_THREAD_JOIN`.
///
/// This syscall does NOT return.
pub const SYS_THREAD_EXIT: u64 = 511;

/// Wait for a specific thread to exit and retrieve its exit value.
///
/// `arg0`: task ID of the thread to wait for.
///
/// Blocks the calling thread until the target thread exits.  The
/// target must belong to the same process as the caller.
///
/// Returns: the target thread's exit value on success.
pub const SYS_THREAD_JOIN: u64 = 512;

/// Suspend (pause) a thread.
///
/// `arg0`: task ID of the thread to suspend.
///
/// The target thread must belong to the calling process.  A suspended
/// thread does not execute until resumed via `SYS_THREAD_RESUME`.
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_THREAD_SUSPEND: u64 = 513;

/// Resume a suspended thread.
///
/// `arg0`: task ID of the thread to resume.
///
/// The target thread must belong to the calling process and must be
/// in the Suspended state.
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_THREAD_RESUME: u64 = 514;

/// Change a thread's scheduling priority.
///
/// `arg0`: task ID of the thread (0 = current thread).
/// `arg1`: new priority (0 = highest, 31 = lowest).
///
/// The target thread must belong to the calling process.
///
/// Returns: the old priority on success, negative error on failure.
pub const SYS_THREAD_SET_PRIORITY: u64 = 515;

// ---------------------------------------------------------------------------
// Filesystem syscalls (600–799)
// ---------------------------------------------------------------------------

/// Read an entire file into a userspace buffer.
///
/// `arg0`: pointer to null-terminated path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to destination buffer.
/// `arg3`: buffer capacity.
///
/// Reads the file at `path` and copies up to `capacity` bytes into
/// the buffer.  If the file is larger than the buffer, only `capacity`
/// bytes are copied (no error — partial read).
///
/// Returns: number of bytes read, or negative error code.
pub const SYS_FS_READ_FILE: u64 = 600;

/// Write data to a file (create or overwrite).
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to source data.
/// `arg3`: data length.
///
/// Creates the file if it doesn't exist, overwrites if it does.
///
/// Returns: 0 on success, or negative error code.
pub const SYS_FS_WRITE_FILE: u64 = 601;

/// Delete a file.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
///
/// Returns: 0 on success, or negative error code.
pub const SYS_FS_DELETE: u64 = 602;

/// List directory entries.
///
/// `arg0`: pointer to directory path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to output buffer (for serialized entry data).
/// `arg3`: buffer capacity.
///
/// Writes directory entries as a packed array of
/// `FsDirEntry` structs (see below) into the buffer.
///
/// Returns: number of entries written, or negative error code.
pub const SYS_FS_LIST_DIR: u64 = 603;

/// Create a directory.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
///
/// Returns: 0 on success, or negative error code.
pub const SYS_FS_MKDIR: u64 = 604;

/// Remove an empty directory.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
///
/// Returns: 0 on success, or negative error code.
pub const SYS_FS_RMDIR: u64 = 605;

/// Stat a file or directory.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to output `FsStatResult` buffer (16 bytes).
///
/// Returns: 0 on success, or negative error code.
pub const SYS_FS_STAT: u64 = 606;

/// Create a hard link (new directory entry pointing to existing file).
///
/// `arg0`: pointer to existing path string.
/// `arg1`: existing path length (bytes).
/// `arg2`: pointer to new link path string.
/// `arg3`: new link path length (bytes).
///
/// Both paths must resolve to the same mount point.  The existing
/// path is followed through symlinks (the link points to the underlying
/// file, not the symlink).  Only regular files can be hard-linked;
/// directories return `IsADirectory`.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_LINK: u64 = 607;

/// Query filesystem space and configuration info (statvfs).
///
/// `arg0`: pointer to path string (any path on the target filesystem).
/// `arg1`: path length (bytes).
/// `arg2`: pointer to output `FsStatvfsResult` buffer (64 bytes).
///
/// ## Output layout (64 bytes, all little-endian)
///
/// | Offset | Size | Field         | Description                          |
/// |--------|------|---------------|--------------------------------------|
/// | 0      | 8    | block_size    | Fundamental block size (bytes)       |
/// | 8      | 8    | total_blocks  | Total blocks on filesystem           |
/// | 16     | 8    | free_blocks   | Free (available) blocks              |
/// | 24     | 8    | total_inodes  | Total inodes (0 if N/A)              |
/// | 32     | 8    | free_inodes   | Free inodes (0 if N/A)               |
/// | 40     | 8    | max_name_len  | Maximum filename length              |
/// | 48     | 1    | read_only     | 1 if mounted read-only, 0 otherwise  |
/// | 49     | 15   | reserved      | Padding (zeros)                      |
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_STATVFS: u64 = 608;

/// Size of the output buffer for `SYS_FS_STATVFS`.
pub const FS_STATVFS_SIZE: usize = 64;

/// Acquire an advisory file lock (flock).
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: lock type (0 = shared/read, 1 = exclusive/write).
/// `arg3`: owner ID (typically the process/task ID of the caller).
///
/// ## Semantics
///
/// - Shared locks are compatible with other shared locks but not
///   exclusive locks.
/// - Exclusive locks are incompatible with all other locks.
/// - If the owner already holds a lock, it is upgraded or downgraded.
///
/// Returns: 0 on success, `WOULD_BLOCK` if the lock is held by
/// another process, or negative error code.
pub const SYS_FS_FLOCK: u64 = 609;

/// Release an advisory file lock.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: owner ID.
///
/// If the owner doesn't hold a lock on this file, this is a no-op.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_FUNLOCK: u64 = 640;

/// Flush all mounted filesystems to stable storage (sync).
///
/// No arguments.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SYNC: u64 = 641;

/// Copy a file from one path to another (works cross-mount).
///
/// `arg0`: pointer to source path string.
/// `arg1`: source path length (bytes).
/// `arg2`: pointer to destination path string.
/// `arg3`: destination path length (bytes).
///
/// Reads the entire source file and writes it to the destination.
/// If the destination exists, it is overwritten.  Works across
/// different mount points (unlike rename/link).
///
/// Returns: number of bytes copied on success, negative error code.
pub const SYS_FS_COPY: u64 = 642;

/// Append data to a file (create if it doesn't exist).
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to data buffer.
/// `arg3`: data length.
///
/// Atomically writes data at the end of the file.  Creates the
/// file if it doesn't exist.  More efficient than open+seek+write
/// for log files and append-only workloads.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_APPEND: u64 = 643;

/// Truncate an open file handle to a given size.
///
/// `arg0`: file handle.
/// `arg1`: new size in bytes.
///
/// The handle must have been opened with WRITE permission.
/// If the file is being shrunk, data past the new size is lost.
/// If the offset was past the new end, it is clamped.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_FTRUNCATE: u64 = 644;

/// Duplicate an open file handle.
///
/// `arg0`: source file handle.
///
/// Creates a new handle referring to the same file with the same
/// flags.  The new handle has an independent cursor position
/// (initially set to the source's current offset).
///
/// Returns: new file handle on success, negative error code.
pub const SYS_FS_DUP: u64 = 645;

/// Get the VFS path of an open file handle.
///
/// `arg0`: file handle.
/// `arg1`: pointer to output buffer.
/// `arg2`: buffer capacity.
///
/// Writes the null-terminated path string into the buffer.
/// Useful for diagnostics and `/proc/<pid>/fd` equivalent.
///
/// Returns: path length in bytes (excluding null terminator),
/// or negative error code.
pub const SYS_FS_HANDLE_PATH: u64 = 646;

/// List directory entries with pagination.
///
/// `arg0`: pointer to directory path string.
/// `arg1`: path length (bytes).
/// `arg2`: packed `(offset << 32) | count`.
///   - Bits 63..32: offset (0-based index of first entry to return).
///   - Bits 31..0:  count  (maximum number of entries to return).
/// `arg3`: pointer to output buffer for serialized entries.
/// `arg4`: output buffer capacity in bytes.
///
/// Each entry is serialized as:
///   `u8 entry_type | u32 name_len | u8[name_len] name | u64 size`
///   (entry_type: 0=file, 1=dir, 2=symlink, 3=volume_label)
///
/// Returns: packed `(total_entries << 32) | entries_written`.
/// If the buffer is too small, entries are truncated (not an error).
pub const SYS_FS_READDIR_AT: u64 = 647;

/// Create a temporary file (no directory entry).
///
/// `arg0`: pointer to directory path string (where to create).
/// `arg1`: path length (bytes).
/// `arg2`: open flags bitfield.
///
/// Creates an unnamed temporary file in the specified directory.
/// The file is automatically deleted when the handle is closed.
///
/// Returns: file handle on success, negative error code on failure.
pub const SYS_FS_TMPFILE: u64 = 648;

/// Pre-allocate disk space for a file.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: size in bytes to pre-allocate.
///
/// The file's logical size is unchanged — only the allocated space
/// grows.  Useful for databases and log files to avoid fragmentation.
///
/// Returns: 0 on success, negative error code on failure.
pub const SYS_FS_FALLOCATE: u64 = 649;

/// Seek to the next data region in a file (SEEK_DATA).
///
/// `arg0`: file handle.
/// `arg1`: offset to start searching from.
///
/// Returns: offset of next data region, or negative error if past EOF.
pub const SYS_FS_SEEK_DATA: u64 = 650;

/// Seek to the next hole in a file (SEEK_HOLE).
///
/// `arg0`: file handle.
/// `arg1`: offset to start searching from.
///
/// Returns: offset of next hole, or EOF if no holes.
pub const SYS_FS_SEEK_HOLE: u64 = 651;

/// Open a file and return a handle.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: open flags bitfield (see `OpenFlags`).
///
/// ## Flags
///
/// | Flag       | Bit | Meaning                              |
/// |------------|-----|--------------------------------------|
/// | `O_READ`   | 0   | Open for reading                     |
/// | `O_WRITE`  | 1   | Open for writing                     |
/// | `O_CREATE` | 2   | Create file if it doesn't exist      |
/// | `O_TRUNC`  | 3   | Truncate to zero length on open      |
/// | `O_APPEND` | 4   | All writes go to end of file         |
///
/// Returns: file handle on success, negative error code on failure.
pub const SYS_FS_OPEN: u64 = 610;

/// Close an open file handle.
///
/// `arg0`: file handle.
///
/// Returns: 0 on success, negative error code on failure.
pub const SYS_FS_CLOSE: u64 = 611;

/// Read from an open file handle at the current offset.
///
/// `arg0`: file handle.
/// `arg1`: pointer to destination buffer.
/// `arg2`: buffer capacity (max bytes to read).
///
/// Advances the file offset by the number of bytes read.
///
/// Returns: number of bytes read (0 = EOF), negative error code.
pub const SYS_FS_READ: u64 = 612;

/// Write to an open file handle at the current offset.
///
/// `arg0`: file handle.
/// `arg1`: pointer to source data.
/// `arg2`: data length.
///
/// Advances the file offset by the number of bytes written.
/// Extends the file if writing past the end.
///
/// Returns: number of bytes written, negative error code.
pub const SYS_FS_WRITE: u64 = 613;

/// Seek to a new position in an open file.
///
/// `arg0`: file handle.
/// `arg1`: offset (interpreted according to `whence`).
/// `arg2`: whence (0 = from start, 1 = from current, 2 = from end).
///
/// Returns: new absolute offset, negative error code.
pub const SYS_FS_SEEK: u64 = 614;

/// Truncate a file to a given size.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: new size in bytes.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_TRUNCATE: u64 = 615;

/// Rename or move a file or directory.
///
/// `arg0`: pointer to source path string.
/// `arg1`: source path length (bytes).
/// `arg2`: pointer to destination path string.
/// `arg3`: destination path length (bytes).
///
/// Both paths must be on the same mount point.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_RENAME: u64 = 616;

/// Stat a file by handle (avoid redundant path lookup).
///
/// `arg0`: file handle.
/// `arg1`: pointer to 16-byte output buffer (same format as `SYS_FS_STAT`).
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_FSTAT: u64 = 617;

/// Open-flag constant: open for reading.
pub const O_READ: u64 = 1 << 0;
/// Open-flag constant: open for writing.
pub const O_WRITE: u64 = 1 << 1;
/// Open-flag constant: create file if it doesn't exist.
pub const O_CREATE: u64 = 1 << 2;
/// Open-flag constant: truncate file to zero on open.
pub const O_TRUNC: u64 = 1 << 3;
/// Open-flag constant: all writes go to end of file.
pub const O_APPEND: u64 = 1 << 4;

/// Move a file to the recycle bin (trash-capable delete).
///
/// `arg0`: pointer to file path string.
/// `arg1`: path length (bytes).
///
/// The file is moved to `/.trash/<name>` on the same filesystem.
/// Companion `.ORI` file records the original path for restoration.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_TRASH: u64 = 618;

/// List items in the recycle bin.
///
/// `arg0`: pointer to output buffer for [`TrashListEntry`] array.
/// `arg1`: buffer capacity (max number of entries).
///
/// Each entry is 528 bytes: 256 (trash name) + 256 (original path) + 8 (size) + 8 (flags).
///
/// Returns: number of entries written, or negative error code.
pub const SYS_FS_TRASH_LIST: u64 = 619;

/// Restore a file from the recycle bin to its original location.
///
/// `arg0`: pointer to trash filename string (as shown in `SYS_FS_TRASH_LIST`).
/// `arg1`: trash filename length (bytes).
/// `arg2`: pointer to output buffer for restored path (256 bytes).
///
/// Returns: length of restored path in bytes, or negative error code.
pub const SYS_FS_TRASH_RESTORE: u64 = 620;

/// Permanently delete all items in the recycle bin.
///
/// No arguments.
///
/// Returns: number of items deleted, or negative error code.
pub const SYS_FS_TRASH_EMPTY: u64 = 621;

/// Size of a trash list entry as returned by `SYS_FS_TRASH_LIST`.
///
/// Layout (528 bytes):
/// - `[0..256]`: trash filename (null-terminated UTF-8)
/// - `[256..512]`: original path (null-terminated UTF-8)
/// - `[512..520]`: file size (u64, little-endian)
/// - `[520..524]`: flags (u32: bit 0 = is_directory)
/// - `[524..528]`: padding (zeros)
pub const FS_TRASH_ENTRY_SIZE: usize = 528;

/// Create a filesystem watch for change notifications.
///
/// `arg0`: pointer to directory path string to watch.
/// `arg1`: path length (bytes).
/// `arg2`: event mask (bitmask of event types to monitor).
/// `arg3`: flags (bit 0 = recursive).
///
/// Event mask bits:
/// - bit 0: CREATE (file/dir created)
/// - bit 1: DELETE (file/dir deleted)
/// - bit 2: MODIFY (file contents changed)
/// - bit 3: RENAME (file/dir renamed/moved)
/// - bit 4: METADATA (metadata changed)
/// - bit 5: ACCESS (file read — high frequency, usually off)
///
/// Returns: watch ID on success, negative error code on failure.
pub const SYS_FS_WATCH_CREATE: u64 = 622;

/// Read pending events from a filesystem watch.
///
/// `arg0`: watch ID.
/// `arg1`: pointer to output buffer for events.
/// `arg2`: maximum number of events to read.
///
/// Each event is 528 bytes:
/// - `[0..256]`: affected path (null-terminated UTF-8)
/// - `[256..512]`: new path for rename events (null-terminated UTF-8, empty otherwise)
/// - `[512..520]`: watch ID (u64)
/// - `[520..524]`: event type (u32: 0=created, 1=deleted, 2=modified, 3=renamed, 255=overflow)
/// - `[524..528]`: padding
///
/// Returns: number of events read, or negative error code.
pub const SYS_FS_WATCH_READ: u64 = 623;

/// Close a filesystem watch.
///
/// `arg0`: watch ID.
///
/// All pending events are discarded.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_WATCH_CLOSE: u64 = 624;

/// Size of a watch event as returned by `SYS_FS_WATCH_READ`.
pub const FS_WATCH_EVENT_SIZE: usize = 528;

// -- Change journal syscalls (625–626) --

/// Get the current journal cursor (latest sequence number).
///
/// No arguments.
///
/// Returns: current sequence number (0 if no events recorded yet).
pub const SYS_FS_JOURNAL_CURSOR: u64 = 625;

/// Read journal entries since a given sequence number.
///
/// `arg0`: sequence number to read from (exclusive — returns entries with seq > arg0).
/// `arg1`: pointer to output buffer.
/// `arg2`: buffer size in bytes.
///
/// Each entry is `FS_JOURNAL_ENTRY_SIZE` bytes:
/// - `[0..8]`: sequence number (u64 LE)
/// - `[8..16]`: timestamp_ns (u64 LE)
/// - `[16]`: event type (0=create, 1=modify, 2=delete, 3=rename)
/// - `[17..273]`: path (256 bytes, null-terminated UTF-8)
/// - `[273..529]`: old_path for renames (256 bytes, null-terminated UTF-8)
///
/// Returns: number of entries written, or negative error code.
pub const SYS_FS_JOURNAL_READ: u64 = 626;

/// Size of a single journal entry as returned by `SYS_FS_JOURNAL_READ`.
pub const FS_JOURNAL_ENTRY_SIZE: usize = 529;

/// Flush the change journal to disk.
///
/// No arguments.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_JOURNAL_FLUSH: u64 = 627;

// ---------------------------------------------------------------------------
// Metadata syscalls (628–639)
// ---------------------------------------------------------------------------

/// Get rich file metadata.
///
/// `arg0`: pointer to null-terminated path string.
/// `arg1`: pointer to output buffer (`FS_META_SIZE` bytes).
///
/// Output layout (see `FS_META_SIZE`):
/// - `[0..8]`:   file size (u64 LE)
/// - `[8]`:      entry type (0=file, 1=dir, 2=vol, 3=symlink)
/// - `[9..16]`:  padding
/// - `[16..24]`: created_ns (u64 LE)
/// - `[24..32]`: modified_ns (u64 LE)
/// - `[32..40]`: accessed_ns (u64 LE)
/// - `[40..48]`: changed_ns (u64 LE)
/// - `[48..52]`: uid (u32 LE)
/// - `[52..56]`: gid (u32 LE)
/// - `[56..58]`: permissions (u16 LE)
/// - `[58..62]`: attributes (u32 LE, FileAttr bits)
/// - `[62..64]`: padding
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_METADATA: u64 = 628;

/// Size of metadata output buffer for `SYS_FS_METADATA`.
pub const FS_META_SIZE: usize = 64;

/// Set file attributes (immutable, append-only, etc.).
///
/// `arg0`: pointer to null-terminated path string.
/// `arg1`: attribute bits (FileAttr::bits()).
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SET_ATTR: u64 = 629;

/// Set file ownership (uid/gid).
///
/// `arg0`: pointer to null-terminated path string.
/// `arg1`: uid (u32).
/// `arg2`: gid (u32).
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SET_OWNER: u64 = 630;

/// Set Unix-style permission bits.
///
/// `arg0`: pointer to null-terminated path string.
/// `arg1`: permission bits (u16, rwxrwxrwx).
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SET_PERMS: u64 = 631;

/// Set file timestamps.
///
/// `arg0`: pointer to null-terminated path string.
/// `arg1`: accessed_ns (0 = leave unchanged).
/// `arg2`: modified_ns (0 = leave unchanged).
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SET_TIMES: u64 = 632;

/// Get an extended attribute value.
///
/// `arg0`: pointer to null-terminated file path.
/// `arg1`: pointer to null-terminated attribute key.
/// `arg2`: pointer to output buffer.
/// `arg3`: buffer capacity.
///
/// Returns: number of bytes written to buffer, or negative error.
pub const SYS_FS_GET_XATTR: u64 = 633;

/// Set an extended attribute.
///
/// `arg0`: pointer to null-terminated file path.
/// `arg1`: pointer to null-terminated attribute key.
/// `arg2`: pointer to value data.
/// `arg3`: value length.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SET_XATTR: u64 = 634;

/// Remove an extended attribute.
///
/// `arg0`: pointer to null-terminated file path.
/// `arg1`: pointer to null-terminated attribute key.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_REMOVE_XATTR: u64 = 635;

/// List extended attribute keys.
///
/// `arg0`: pointer to null-terminated file path.
/// `arg1`: pointer to output buffer (null-separated key names).
/// `arg2`: buffer capacity.
///
/// Returns: total bytes of key data, or negative error.
/// Keys are written as null-terminated strings packed sequentially.
pub const SYS_FS_LIST_XATTRS: u64 = 636;

// ---------------------------------------------------------------------------
// Symlink syscalls (637–639)
// ---------------------------------------------------------------------------

/// Create a symbolic link.
///
/// `arg0`: pointer to symlink path string (where to create the link).
/// `arg1`: symlink path length (bytes).
/// `arg2`: pointer to target string (what the link points to).
/// `arg3`: target length (bytes).
///
/// The target string is stored as-is and resolved during path
/// traversal.  It can be absolute or relative.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_SYMLINK: u64 = 637;

/// Read the target of a symbolic link.
///
/// `arg0`: pointer to symlink path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to output buffer for the target string.
/// `arg3`: buffer capacity.
///
/// Returns: length of the target string in bytes, or negative error.
pub const SYS_FS_READLINK: u64 = 638;

/// Stat a path without following the final symbolic link.
///
/// `arg0`: pointer to path string.
/// `arg1`: path length (bytes).
/// `arg2`: pointer to output `FsStatResult` buffer (16 bytes).
///
/// If the final component is a symlink, returns the symlink's own
/// metadata (type = 3 for symlink, size = target path length).
/// Intermediate symlinks in the path are still followed.
///
/// Returns: 0 on success, negative error code.
pub const SYS_FS_LSTAT: u64 = 639;

/// Seek whence: from start of file.
pub const SEEK_SET: u64 = 0;
/// Seek whence: from current offset.
pub const SEEK_CUR: u64 = 1;
/// Seek whence: from end of file.
pub const SEEK_END: u64 = 2;

/// Size of `FsDirEntry` as returned by `SYS_FS_LIST_DIR`.
///
/// Layout (264 bytes):
/// - `[0..256]`: filename (null-terminated UTF-8, padded with zeros)
/// - `[256..260]`: file size (u32, little-endian)
/// - `[260]`: entry type (0=file, 1=directory)
/// - `[261..264]`: padding (zeros)
pub const FS_DIR_ENTRY_SIZE: usize = 264;

// ---------------------------------------------------------------------------
// Networking syscalls (800–999)
// ---------------------------------------------------------------------------

/// Open a TCP connection to a remote host.
///
/// `arg0`: IPv4 address as u32 (network byte order: `a.b.c.d` →
///         `(a << 24) | (b << 16) | (c << 8) | d`).
/// `arg1`: remote port (0–65535).
///
/// Performs a blocking TCP 3-way handshake.
///
/// Returns: socket handle on success, negative error on failure.
pub const SYS_TCP_CONNECT: u64 = 800;

/// Send data on a TCP socket.
///
/// `arg0`: socket handle.
/// `arg1`: pointer to data buffer.
/// `arg2`: data length.
///
/// Returns: number of bytes sent, or negative error.
pub const SYS_TCP_SEND: u64 = 801;

/// Receive data from a TCP socket (blocking).
///
/// `arg0`: socket handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Blocks until data is available or the connection closes.
///
/// Returns: number of bytes received (0 = EOF), or negative error.
pub const SYS_TCP_RECV: u64 = 802;

/// Close a TCP socket.
///
/// `arg0`: socket handle.
///
/// Sends FIN and releases resources.
///
/// Returns: 0 on success.
pub const SYS_TCP_CLOSE: u64 = 803;

/// Bind a UDP socket to a local port.
///
/// `arg0`: local port (0–65535).
///
/// Returns: socket handle on success, negative error on failure.
pub const SYS_UDP_BIND: u64 = 810;

/// Send a UDP datagram.
///
/// `arg0`: socket handle (for source port) OR 0 (use ephemeral port).
/// `arg1`: destination IPv4 address (u32, network byte order).
/// `arg2`: destination port.
/// `arg3`: pointer to data buffer.
/// `arg4`: data length.
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_UDP_SEND: u64 = 811;

/// Receive a UDP datagram (non-blocking).
///
/// `arg0`: socket handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
/// `arg3`: pointer to 6-byte source address output:
///         `[0..4]` = source IPv4 (network byte order),
///         `[4..6]` = source port (little-endian u16).
///
/// Returns: number of bytes received, or `WouldBlock` if no datagrams.
pub const SYS_UDP_RECV: u64 = 812;

/// Close a UDP socket.
///
/// `arg0`: socket handle.
///
/// Returns: 0 on success.
pub const SYS_UDP_CLOSE: u64 = 813;

/// Resolve a hostname to an IPv4 address via DNS.
///
/// `arg0`: pointer to hostname string.
/// `arg1`: hostname length.
/// `arg2`: pointer to 4-byte output buffer for the IPv4 address.
///
/// Performs a blocking DNS query (UDP, ~2s timeout).
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_DNS_RESOLVE: u64 = 820;

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
