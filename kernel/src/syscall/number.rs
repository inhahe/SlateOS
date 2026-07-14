//! Syscall number definitions.
//!
//! Each syscall has a unique number.  Numbers are grouped by subsystem
//! to avoid merge conflicts when multiple sessions work in parallel.
//!
//! Many constants here are defined ahead of being wired into the
//! dispatch table — that's intentional. This file is the source of
//! truth for the ABI numbering, so we allow dead_code at module scope.

#![allow(dead_code)]
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

/// Get the realtime (wall-clock) time: nanoseconds since the Unix epoch
/// (1970-01-01 00:00:00 UTC).
///
/// Backed by [`crate::timekeeping::clock_realtime`] (CMOS RTC read once at
/// boot plus TSC-based elapsed time, with NTP/manual adjustments).  This is
/// what POSIX `CLOCK_REALTIME`, `gettimeofday`, and `time` must use —
/// `SYS_CLOCK_MONOTONIC` is boot-relative and unsuitable for wall-clock
/// timestamps (file mtimes, TLS validity, logs, `make`, …).
pub const SYS_CLOCK_REALTIME: u64 = 14;

/// Set the realtime (wall-clock) time to a specific Unix epoch timestamp.
///
/// `arg0`: target time in nanoseconds since the Unix epoch.
///
/// Backed by [`crate::timekeeping::set_realtime`], which stores the
/// adjustment needed to make [`crate::timekeeping::clock_realtime`] return
/// the requested value.  This is what POSIX `clock_settime(CLOCK_REALTIME)`
/// and `settimeofday` use.  Returns `EINVAL` for a zero/uninitialized clock
/// base (the RTC has not been read yet) so callers do not silently lock in a
/// nonsensical offset.
pub const SYS_CLOCK_SETTIME: u64 = 15;

/// Adjust the realtime (wall-clock) time by a signed nanosecond delta.
///
/// `arg0`: signed nanosecond offset (reinterpret the `u64` as `i64`).
///         Positive advances the clock, negative steps it back.
///
/// Backed by [`crate::timekeeping::adjust_realtime`], which atomically adds
/// the delta to the standing realtime adjustment (`fetch_add`) — unlike
/// `SYS_CLOCK_SETTIME`, there is no read-modify-write race because the shift
/// is relative.  This is what POSIX `adjtimex`/`clock_adjtime` use to apply
/// an `ADJ_SETOFFSET` clock step (the abrupt correction chrony/ntpd issue).
/// Returns `EINVAL` when the realtime clock base is uninitialized (the RTC
/// has not been read yet), mirroring `SYS_CLOCK_SETTIME`, so callers never
/// lock in an adjustment against a meaningless base.
pub const SYS_CLOCK_ADJTIME: u64 = 16;

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
/// Requires: DeviceIrq capability (per-IRQ or type-level).
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
/// Requires: PortIo capability (per-port or type-level) with READ.
pub const SYS_PORT_READ: u64 = 40;

/// Write to an I/O port.
///
/// `arg0`: port number (0–65535).
/// `arg1`: access width (1 = byte, 2 = word, 4 = dword).
/// `arg2`: value to write.
///
/// Returns: 0 on success.
///
/// Requires: PortIo capability (per-port or type-level) with WRITE.
pub const SYS_PORT_WRITE: u64 = 41;

/// Allocate a DMA buffer and map it into the calling process.
///
/// `arg0`: size in bytes (minimum; actual allocation may be larger).
/// `arg1`: constraint (0 = none/64-bit, 1 = below 4 GiB, 2 = below 16 MiB).
///
/// Returns (packed in value + value2):
/// - `value`: user-space virtual address of the buffer.
/// - `value2`: physical address (for programming device DMA descriptors).
///
/// The buffer is zeroed and mapped with write-through caching
/// suitable for DMA.  The caller must call `SYS_DMA_FREE` when done.
///
/// Requires: PortIo or DeviceIrq capability (driver privilege).
pub const SYS_DMA_ALLOC: u64 = 42;

/// Free a DMA buffer previously allocated with `SYS_DMA_ALLOC`.
///
/// `arg0`: user-space virtual address of the buffer.
///
/// Unmaps the buffer from the process and frees the physical memory.
/// The device must not be actively DMA-ing to this buffer.
///
/// Returns: 0 on success, negative error.
pub const SYS_DMA_FREE: u64 = 43;

/// Create an IOMMU DMA remapping domain.
///
/// Returns: domain ID on success (> 0), negative error.
///
/// A domain is an isolated DMA address space.  Devices attached to a
/// domain can only DMA to physical pages explicitly mapped into that
/// domain.  Unmapped addresses cause a DMA fault.
///
/// Requires: PortIo capability (driver privilege).
pub const SYS_DMA_DOMAIN_CREATE: u64 = 44;

/// Destroy an IOMMU DMA remapping domain.
///
/// `arg0`: domain ID.
///
/// All devices must be detached before destroying a domain.
///
/// Returns: 0 on success, negative error.
pub const SYS_DMA_DOMAIN_DESTROY: u64 = 45;

/// Map a physical address range into an IOMMU domain.
///
/// `arg0`: domain ID.
/// `arg1`: bus address (device-visible address, page-aligned).
/// `arg2`: physical address (host physical, page-aligned).
/// `arg3`: size in bytes (rounded up to 4 KiB pages).
/// `arg4`: permissions (1 = read, 2 = write, 3 = read+write).
///
/// After this call, devices in the domain can DMA to `bus_addr`
/// and it will be translated to `phys_addr`.
///
/// Returns: 0 on success, negative error.
pub const SYS_DMA_MAP: u64 = 46;

/// Unmap a bus address range from an IOMMU domain.
///
/// `arg0`: domain ID.
/// `arg1`: bus address (start of range to unmap).
/// `arg2`: size in bytes.
///
/// Returns: 0 on success, negative error.
pub const SYS_DMA_UNMAP: u64 = 47;

/// Attach a PCI device to an IOMMU domain.
///
/// `arg0`: domain ID.
/// `arg1`: PCI bus number (0–255).
/// `arg2`: PCI device number (0–31).
/// `arg3`: PCI function number (0–7).
///
/// After attachment, all DMA from this device goes through the
/// domain's page table.  Unauthorized DMA causes a fault.
///
/// Returns: 0 on success, negative error.
pub const SYS_DMA_ATTACH: u64 = 48;

/// Detach a PCI device from an IOMMU domain.
///
/// `arg0`: domain ID.
/// `arg1`: PCI bus number.
/// `arg2`: PCI device number.
/// `arg3`: PCI function number.
///
/// Returns: 0 on success, negative error.
pub const SYS_DMA_DETACH: u64 = 49;

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

/// Get the number of online CPUs.
///
/// Returns the number of CPUs the scheduler is currently using.  Used
/// by userspace runtimes to size worker thread pools, by libc's
/// `sysconf(_SC_NPROCESSORS_ONLN)` / `get_nprocs()`, and by
/// `sched_getaffinity()` to populate a meaningful default affinity
/// mask.
///
/// Returns: number of online CPUs (always ≥ 1).
pub const SYS_CPU_COUNT: u64 = 55;

/// Get the total number of physical pages on the system.
///
/// Page size matches our hardware page size (16 KiB), so this is the
/// total count of 16 KiB frames managed by the kernel's frame
/// allocator.  Used by libc's `sysconf(_SC_PHYS_PAGES)` /
/// `get_phys_pages()`, by `/proc/meminfo` consumers, and by language
/// runtimes that size caches based on RAM footprint.
///
/// Returns: total physical pages (≥ 1 on any working system).
pub const SYS_PHYS_PAGES_TOTAL: u64 = 56;

/// Get the number of available (free) physical pages.
///
/// Returns the snapshot of currently-free frames in the buddy
/// allocator at the time of the call.  Free count is racy by design
/// — callers should treat the result as a hint, not a guarantee.
/// Used by libc's `sysconf(_SC_AVPHYS_PAGES)` / `get_avphys_pages()`
/// and by memory-pressure monitors.
///
/// Returns: free physical pages.
pub const SYS_PHYS_PAGES_AVAIL: u64 = 57;

/// Read one of the three EWMA load averages.
///
/// `arg0`: which average to read.
///   - 0 = 1-minute
///   - 1 = 5-minute
///   - 2 = 15-minute
///
/// Returns the fixed-point value with FSHIFT=11 (i.e., the integer
/// load × 2048, plus fractional bits).  Userspace divides by 2048.0
/// to get the conventional decimal form.  Used by libc's
/// `getloadavg()` and by `uptime`-like utilities to read the
/// scheduler's tracked load.
///
/// Returns: load value in fixed-point.  `InvalidArgument` if `arg0`
/// is not 0/1/2.
pub const SYS_LOADAVG: u64 = 58;

/// Get aggregate per-CPU time accounting fields (summed across all CPUs).
///
/// `arg0`: field selector:
///   - 0 = system_ns (kernel/user code time, all non-IRQ/softirq/idle)
///   - 1 = irq_ns (hardware interrupt handlers)
///   - 2 = softirq_ns (deferred interrupt work)
///   - 3 = idle_ns (HLT/MWAIT time)
///   - 4 = total_ns (wall time × number of online CPUs)
///
/// Returns: the selected aggregate field in nanoseconds.
/// `InvalidArgument` if `arg0` is not 0..=4.
pub const SYS_CPU_TIMES: u64 = 59;

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
/// `arg0`: flags (bit 0 = `CHANNEL_FLAG_SYNC` — synchronous/rendezvous
///   mode with no internal message buffer).
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

/// Send a message with a timeout (nanoseconds).
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: length of message data.
/// `arg3`: timeout in nanoseconds (0 = return TimedOut if full).
///
/// Unlike `SYS_CHANNEL_SEND` which returns `ChannelFull` immediately,
/// this variant blocks until queue space is available or the deadline
/// expires.
///
/// Returns: 0 on success, `TimedOut` if deadline expires.
pub const SYS_CHANNEL_SEND_TIMEOUT: u64 = 208;

/// Send a message (blocking when queue is full).
///
/// `arg0`: channel handle.
/// `arg1`: pointer to message data.
/// `arg2`: length of message data.
///
/// Unlike `SYS_CHANNEL_SEND` which returns `ChannelFull` immediately,
/// this variant blocks until queue space is available.
///
/// Returns: 0 on success.
pub const SYS_CHANNEL_SEND_BLOCKING: u64 = 209;

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

/// Wake up to `max_wake` waiters on one futex, then requeue up to
/// `max_requeue` of the remaining waiters onto a second futex.
///
/// `arg0`: source futex address (`addr1`).
/// `arg1`: destination futex address (`addr2`; 0 = wake-only).
/// `arg2`: maximum number of waiters to wake from `addr1`.
/// `arg3`: maximum number of waiters to requeue to `addr2`.
///
/// Returns: total tasks affected (woken + requeued).  This is the
/// primitive behind condition-variable broadcast (`pthread_cond_*`):
/// it moves waiters from the condvar futex to the mutex futex instead
/// of waking them all at once.
pub const SYS_FUTEX_REQUEUE: u64 = 215;

/// Try to lock a PI futex without blocking.
///
/// `arg0`: pointer to a 32-bit futex word (must be 4-byte aligned).
///
/// Returns: 0 on success.  `WouldBlock` (`EAGAIN`) if held by another
/// task; `Deadlock` (`EDEADLK`) if the caller already owns it.
pub const SYS_FUTEX_TRYLOCK_PI: u64 = 216;

/// Lock a PI futex with a relative timeout (nanoseconds).
///
/// `arg0`: pointer to a 32-bit futex word (must be 4-byte aligned).
/// `arg1`: timeout in nanoseconds (0 = try once, never block).
///
/// Returns: 0 on success; `TimedOut` (`ETIMEDOUT`) if the deadline
/// expires before acquisition.
pub const SYS_FUTEX_LOCK_PI_TIMEOUT: u64 = 217;

/// Wait on a condvar futex, to be requeued onto a PI mutex on wake.
///
/// Backs the condvar→PI-mutex handoff (`pthread_cond_wait` on a
/// priority-inheriting mutex).  The caller parks on the condvar word until
/// a [`SYS_FUTEX_CMP_REQUEUE_PI`] grants or transfers ownership of the PI
/// mutex to it.
///
/// `arg0`: condvar futex word pointer (readable, 4-byte aligned).
/// `arg1`: expected condvar value (`u32`).
/// `arg2`: PI mutex futex word pointer (writable, 4-byte aligned).
/// `arg3`: timeout in nanoseconds (used only when `arg4` is non-zero).
/// `arg4`: timeout flag — 0 = wait indefinitely, non-zero = use `arg3`.
///
/// Returns: 0 on success (now owns the PI mutex); `WouldBlock` (`EAGAIN`)
/// on value mismatch; `TimedOut` (`ETIMEDOUT`) if the deadline expires.
pub const SYS_FUTEX_WAIT_REQUEUE_PI: u64 = 218;

/// Signal a PI condvar: wake/requeue waiters onto a PI mutex.
///
/// `arg0`: condvar futex word pointer (readable, 4-byte aligned).
/// `arg1`: PI mutex futex word pointer (writable, 4-byte aligned).
/// `arg2`: maximum number of waiters to requeue (`u32`).
/// `arg3`: expected condvar value (`u32`); mismatch → `EAGAIN`.
///
/// Returns: number of waiters affected (woken + requeued).
pub const SYS_FUTEX_CMP_REQUEUE_PI: u64 = 219;

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

/// Write to a pipe with a timeout (nanoseconds).
///
/// `arg0`: pipe handle (write end).
/// `arg1`: pointer to data buffer.
/// `arg2`: data length.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: bytes written, `TimedOut` if deadline expires.
pub const SYS_PIPE_WRITE_TIMEOUT: u64 = 227;

/// `SYS_PIPE_POLL` — query pipe readiness for poll/select.
///
/// `arg0`: pipe handle (either read or write end).
///
/// Returns a bitmask:
/// - bit 0 (0x01): readable (data available, or write-end closed)
/// - bit 2 (0x04): writable (buffer has space, or read-end closed)
/// - bit 4 (0x10): hangup (other end closed)
///
/// The caller should check the appropriate bit based on whether
/// this is a read-end or write-end handle.
pub const SYS_PIPE_POLL: u64 = 228;

/// `SYS_PIPE_READABLE_BYTES` — return the number of bytes buffered in a pipe.
///
/// `arg0`: pipe handle (read end or write end).
///
/// For a read-end handle: returns the number of bytes available to read.
/// For a write-end handle: returns the amount of free space in the buffer.
/// Returns 0 if the pipe handle is invalid.
pub const SYS_PIPE_READABLE_BYTES: u64 = 229;

// The pipe family's original contiguous block (220–229) is full — 230 begins
// the shared-memory family — so these two later additions take numbers from the
// free extension range (657+) while staying grouped with the pipe family here in
// source. Syscall numbers are a flat namespace; contiguity per family is a
// source-organization nicety, not an ABI requirement.

/// `SYS_PIPE_PEEK` — copy buffered bytes out of a pipe WITHOUT consuming them.
///
/// `arg0`: pipe handle (read end).
/// `arg1`: byte offset into the buffered data to start copying from.
/// `arg2`: pointer to the caller's receive buffer.
/// `arg3`: buffer capacity.
///
/// Returns: number of bytes copied (0 once `offset` is at or past the buffered
/// length), or a negative error code. This is the primitive behind `tee(2)`:
/// the caller peeks successive offsets and writes the copies into another pipe,
/// leaving the source pipe's contents intact.
pub const SYS_PIPE_PEEK: u64 = 657;

/// `SYS_PIPE_WAIT_READABLE` — block until a pipe has data or hits EOF, without
/// consuming any bytes.
///
/// `arg0`: pipe handle (read end).
///
/// Returns: 1 if data is now available to peek/read, 0 if the write end closed
/// and no data remains (EOF), or a negative error code. This is the blocking
/// primitive `tee(2)` uses to wait for input on an empty source before
/// duplicating it.
pub const SYS_PIPE_WAIT_READABLE: u64 = 658;

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

/// Map a shared memory region into the calling process's address space.
///
/// `arg0`: shared memory handle.
/// `arg1`: flags (`MAP_READ` | `MAP_WRITE`; execute is never granted).
///
/// Returns: user virtual address of the mapping on success. The region's
/// physical frames are ref-counted, so the mapping keeps them alive even
/// after every handle is closed; unmapping (via `munmap`/`SYS_SHM_UNMAP`
/// or process exit) drops the reference.
pub const SYS_SHM_MAP: u64 = 233;

/// Unmap a shared memory region previously mapped with [`SYS_SHM_MAP`].
///
/// `arg0`: user virtual address returned by `SYS_SHM_MAP`.
/// `arg1`: size in bytes (the region size, rounded up to frame boundary).
///
/// Returns: 0 on success. Idempotent-ish: unmapping frames that are not
/// present is not an error.
pub const SYS_SHM_UNMAP: u64 = 234;

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

/// Read an eventfd with a timeout (nanoseconds).
///
/// `arg0`: eventfd handle.
/// `arg1`: timeout in nanoseconds (0 = non-blocking try).
///
/// Blocks until counter > 0 or the deadline expires.  Returns the
/// counter value and resets it to 0 on success.
///
/// Returns: counter value (> 0), `TimedOut` if deadline expires.
pub const SYS_EVENTFD_READ_TIMEOUT: u64 = 245;

/// Write (signal) an eventfd with a timeout (nanoseconds).
///
/// `arg0`: eventfd handle.
/// `arg1`: value to add (must be > 0).
/// `arg2`: timeout in nanoseconds (0 = non-blocking try).
///
/// Blocks up to the timeout if the addition would overflow
/// `u64::MAX - 1`.  Returns `TimedOut` if the deadline expires.
///
/// Returns: 0 on success, `TimedOut` if deadline expires.
pub const SYS_EVENTFD_WRITE_TIMEOUT: u64 = 246;

/// Non-destructive readiness query on an eventfd.
///
/// `arg0`: eventfd handle.
///
/// Used by `poll`/`select`/`epoll` to decide whether the eventfd is
/// readable without consuming its value.  Unlike `SYS_EVENTFD_TRY_READ`,
/// this does not modify the counter.
///
/// Returns: 1 if the counter is > 0 (readable), 0 if it is 0
/// (not readable), or a negative error on bad handle.
pub const SYS_EVENTFD_HAS_VALUE: u64 = 247;

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
// Service registry syscalls (280–289)
// ---------------------------------------------------------------------------

/// Register a named service.
///
/// `arg0`: pointer to service name (bytes).
/// `arg1`: name length.
///
/// Returns: listener handle on success, negative error.
pub const SYS_SERVICE_REGISTER: u64 = 280;

/// Connect to a named service.
///
/// `arg0`: pointer to service name (bytes).
/// `arg1`: name length.
///
/// Creates a channel pair, queues one end for the service to accept,
/// returns the other end to the caller.
///
/// Returns: channel handle for the client endpoint.
pub const SYS_SERVICE_CONNECT: u64 = 281;

/// Accept a pending connection on a service listener (blocking).
///
/// `arg0`: listener handle.
///
/// Returns: channel handle for the server endpoint.
pub const SYS_SERVICE_ACCEPT: u64 = 282;

/// Accept a connection (non-blocking).
///
/// `arg0`: listener handle.
///
/// Returns: channel handle, or `WouldBlock` if no connections pending.
pub const SYS_SERVICE_TRY_ACCEPT: u64 = 283;

/// Accept a connection with a timeout (nanoseconds).
///
/// `arg0`: listener handle.
/// `arg1`: timeout in nanoseconds.
///
/// Returns: channel handle, or `TimedOut` if deadline expires.
pub const SYS_SERVICE_ACCEPT_TIMEOUT: u64 = 284;

/// Unregister a service (close its listener).
///
/// `arg0`: listener handle.
///
/// All pending connections are closed.
///
/// Returns: 0 on success.
pub const SYS_SERVICE_UNREGISTER: u64 = 285;

// ---------------------------------------------------------------------------
// Namespace syscalls (290–299) — per-process filesystem isolation
// ---------------------------------------------------------------------------

/// Create a new namespace.
///
/// `arg0`: clone_from (namespace ID to copy rules from, 0 = empty).
///
/// Returns: new namespace ID on success.
pub const SYS_NS_CREATE: u64 = 290;

/// Add a bind (path remapping) rule to a namespace.
///
/// `arg0`: namespace ID.
/// `arg1`: pointer to source prefix string.
/// `arg2`: source prefix length.
/// `arg3`: pointer to target prefix string.
/// `arg4`: target prefix length.
///
/// Returns: 0 on success.
pub const SYS_NS_BIND: u64 = 291;

/// Remove a bind rule from a namespace.
///
/// `arg0`: namespace ID.
/// `arg1`: pointer to source prefix string.
/// `arg2`: source prefix length.
///
/// Returns: 0 on success.
pub const SYS_NS_UNBIND: u64 = 292;

/// Add a hide rule to a namespace (blocks access to a path prefix).
///
/// `arg0`: namespace ID.
/// `arg1`: pointer to prefix string.
/// `arg2`: prefix length.
///
/// Returns: 0 on success.
pub const SYS_NS_HIDE: u64 = 293;

/// Attach a process to a namespace.
///
/// `arg0`: process ID (0 = current process).
/// `arg1`: namespace ID (0 = root/default namespace).
///
/// Returns: 0 on success.
pub const SYS_NS_ATTACH: u64 = 294;

/// Query which namespace a process belongs to.
///
/// `arg0`: process ID (0 = current process).
///
/// Returns: namespace ID (0 = root namespace).
pub const SYS_NS_QUERY: u64 = 295;

// ---------------------------------------------------------------------------
// Stream socket syscalls (300–310) — bidirectional byte-stream IPC
// (backs POSIX `socketpair(AF_UNIX, SOCK_STREAM, ...)`)
// ---------------------------------------------------------------------------

/// Create a stream socket pair.
///
/// Takes no arguments.  Returns two endpoint handles: the first in the
/// primary result register (`rax`), the second in the secondary register
/// (`rdx`).  Bytes sent on one endpoint are received on the other.
pub const SYS_SOCKETPAIR_CREATE: u64 = 300;

/// Send bytes on a stream socket endpoint (blocking).
///
/// `arg0`: endpoint handle.
/// `arg1`: pointer to data buffer.
/// `arg2`: number of bytes to send.
///
/// Returns: bytes sent (> 0), or a negative error code (`ChannelClosed`
/// if the peer's read side is gone).
pub const SYS_SOCKETPAIR_SEND: u64 = 301;

/// Receive bytes from a stream socket endpoint (blocking).
///
/// `arg0`: endpoint handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
///
/// Returns: bytes received (0 = EOF), or a negative error code.
pub const SYS_SOCKETPAIR_RECV: u64 = 302;

/// Non-blocking send (see [`SYS_SOCKETPAIR_SEND`]).
///
/// Returns `WouldBlock` if the outgoing buffer is full.
pub const SYS_SOCKETPAIR_TRY_SEND: u64 = 303;

/// Non-blocking receive (see [`SYS_SOCKETPAIR_RECV`]).
///
/// Returns `WouldBlock` if the incoming buffer is empty (and not EOF).
pub const SYS_SOCKETPAIR_TRY_RECV: u64 = 304;

/// Close a stream socket endpoint handle.
///
/// `arg0`: endpoint handle.
pub const SYS_SOCKETPAIR_CLOSE: u64 = 305;

/// Send bytes with a timeout (nanoseconds).
///
/// `arg0`: endpoint handle.
/// `arg1`: pointer to data buffer.
/// `arg2`: data length.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: bytes sent, `TimedOut` if the deadline expires.
pub const SYS_SOCKETPAIR_SEND_TIMEOUT: u64 = 306;

/// Receive bytes with a timeout (nanoseconds).
///
/// `arg0`: endpoint handle.
/// `arg1`: pointer to receive buffer.
/// `arg2`: buffer capacity.
/// `arg3`: timeout in nanoseconds (0 = non-blocking try).
///
/// Returns: bytes received, 0 if EOF, `TimedOut` if the deadline expires.
pub const SYS_SOCKETPAIR_RECV_TIMEOUT: u64 = 307;

/// Poll a stream socket endpoint for readiness (for poll/select).
///
/// `arg0`: endpoint handle.
///
/// Returns a bitmask:
/// - bit 0 (0x01): readable (data available, or read-side EOF)
/// - bit 2 (0x04): writable (buffer space, or write would error)
/// - bit 3 (0x08): error (broken pipe — peer read side gone)
/// - bit 4 (0x10): hangup (peer write side gone)
pub const SYS_SOCKETPAIR_POLL: u64 = 308;

/// Return the number of bytes available to receive on an endpoint.
///
/// `arg0`: endpoint handle.
///
/// Returns 0 if the handle is invalid.
pub const SYS_SOCKETPAIR_READABLE_BYTES: u64 = 309;

/// Shut down one or both directions of a stream socket endpoint
/// (`shutdown(2)`).
///
/// `arg0`: endpoint handle.
/// `arg1`: how (0 = `SHUT_RD`, 1 = `SHUT_WR`, 2 = `SHUT_RDWR`).
///
/// Returns: 0 on success, `InvalidArgument` if `how` is out of range.
pub const SYS_SOCKETPAIR_SHUTDOWN: u64 = 310;

// ---------------------------------------------------------------------------
// Security syscalls (400–499)
// ---------------------------------------------------------------------------

/// Query the calling process's capabilities.
pub const SYS_CAP_QUERY: u64 = 400;

/// Request a capability the calling process does not hold.
///
/// Submits a request to the security policy handler (eventually a GUI
/// dialog, initially console-based).  The request includes a reason
/// string displayed to the user for approval/denial.
///
/// `arg0`: resource type (`ResourceType` as u16, zero-extended).
/// `arg1`: rights bitfield (`Rights` as u32, zero-extended).
/// `arg2`: pointer to reason string (UTF-8, user buffer).
/// `arg3`: length of reason string in bytes (max 256).
///
/// Returns: request ID (positive u64) on success, negative error on failure.
///
/// Errors:
/// - `InvalidArgument` — invalid resource type or zero-length reason.
/// - `ResourceExhausted` — too many pending requests.
pub const SYS_CAP_REQUEST: u64 = 401;

/// Check the status of a pending capability request.
///
/// `arg0`: request ID (from `SYS_CAP_REQUEST`).
///
/// Returns: status code on success (0=Pending, 1=Approved, 2=Denied,
///          3=TimedOut, 4=Cancelled), negative error on failure.
///
/// Errors:
/// - `NotFound` — no request with that ID exists.
pub const SYS_CAP_REQUEST_STATUS: u64 = 402;

/// Cancel a pending capability request.
///
/// Only the process that submitted the request can cancel it.
///
/// `arg0`: request ID (from `SYS_CAP_REQUEST`).
///
/// Returns: 0 on success, negative error on failure.
///
/// Errors:
/// - `NotFound` — no request with that ID from this process.
/// - `InvalidArgument` — request is not in Pending state.
pub const SYS_CAP_REQUEST_CANCEL: u64 = 403;

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
/// `arg2`: pointer to packed null-terminated argv data (0 = no args).
/// `arg3`: total byte length of the packed argv data.
/// `arg4`: pointer to packed null-terminated envp data (0 = no env).
/// `arg5`: total byte length of the packed envp data.
///
/// The packed argv/envp format is the same as `SYS_PROCESS_SPAWN_EX`:
/// null-terminated strings concatenated without padding.
///
/// The new process image reads argv/envp via `SYS_PROCESS_GET_ARGS`.
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

/// Retrieve crash information for a zombie child process.
///
/// `arg0`: child PID.
/// `arg1`: pointer to a 4×u64 output buffer in userspace:
///         [0] exception_code
///         [1] faulting_rip
///         [2] aux (e.g., page fault address)
///         [3] thread_id
///
/// Returns: 1 if crash info is available (process crashed),
///          0 if no crash info (normal exit), or negative error.
///
/// Must be called before reaping — crash info is destroyed when the
/// process is reaped.  The parent should call this after
/// `SYS_PROCESS_TRY_WAIT` returns an exit code (especially if
/// exit_code < 0, indicating a crash).
pub const SYS_PROCESS_CRASH_INFO: u64 = 516;

/// Spawn a new process with extended options (fd inheritance, argv, envp).
///
/// `arg0`: pointer to a `SpawnExArgs` struct in user memory.
///
/// The `SpawnExArgs` struct bundles all spawn parameters:
/// - ELF data pointer and length
/// - Process name
/// - FdMapEntry array (for fd inheritance)
/// - Packed null-terminated argv strings + count
/// - Packed null-terminated envp strings + count
///
/// Fd inheritance: each `FdMapEntry` is `{ fd: i32, _pad: i32,
/// handle: u64 }` (16 bytes, C repr).  The kernel duplicates the
/// parent's `handle` and stores it in the child's PCB.
///
/// argv/envp: packed null-terminated strings (e.g., `"ls\0-la\0"`).
/// The kernel stores them in the child's PCB.  The child reads them
/// via `SYS_PROCESS_GET_ARGS` (519).
///
/// Returns: process ID on success, negative error on failure.
pub const SYS_PROCESS_SPAWN_EX: u64 = 517;

/// Retrieve initial file descriptor mappings for the current process.
///
/// Called by the child process's POSIX layer during startup to discover
/// which file descriptors were inherited from the parent.
///
/// `arg0`: pointer to output buffer (array of `FdMapEntry`).
/// `arg1`: capacity of the output buffer (in entries, not bytes).
///
/// Returns: number of entries written on success, or negative error.
/// The entries are consumed (one-shot) — subsequent calls return 0.
pub const SYS_PROCESS_GET_INITIAL_FDS: u64 = 518;

/// Retrieve initial argv/envp for the current process.
///
/// Called by the child process's POSIX layer during startup to read
/// the command-line arguments and environment variables that the
/// parent passed via `SYS_PROCESS_SPAWN_EX`.
///
/// `arg0`: pointer to output buffer.
/// `arg1`: output buffer capacity (bytes).
///
/// The output buffer receives a `SpawnArgsHeader` (16 bytes) followed
/// by packed null-terminated argv strings, then packed null-terminated
/// envp strings.
///
/// Returns: total bytes needed (may exceed capacity — caller should
/// realloc and retry).  0 if no args were set.  The data is consumed
/// (one-shot) — subsequent calls return 0.
pub const SYS_PROCESS_GET_ARGS: u64 = 519;

/// Get the parent process ID of the calling process.
///
/// Returns: parent PID on success.  Returns 0 if the calling task is
/// not associated with a process (kernel thread), if the process has
/// no recorded parent (e.g. init/pid 1), or if the process has been
/// reparented after the parent exited.  This is the same "no Unix
/// concept of orphan re-parenting to init" convention used elsewhere
/// in our process table — userspace `getppid()` should treat 0 as
/// "no parent" and use it as a sentinel.
pub const SYS_PROCESS_PARENT_ID: u64 = 520;

/// Get the count of live processes managed by the kernel.
///
/// Takes no arguments.  Returns the current number of entries in the
/// process table (including the kernel-creator pid 0 / init, processes
/// in any state — Creating/Ready/Running/Sleeping/Zombie).  This count
/// is used by `sysinfo()` to populate `struct sysinfo.procs`.
///
/// Returns: number of processes as a non-negative i64 (saturating at
/// `i64::MAX` in the unlikely event the table grows past that).  Never
/// fails — there is no error path.
pub const SYS_PROCESS_COUNT: u64 = 521;

// ---------------------------------------------------------------------------
// POSIX signal-shim syscalls (522–526)
// ---------------------------------------------------------------------------
//
// Our OS does not use Unix signals for process control (design.txt: "No
// Unix signals for process control. Use IPC messages.").  These syscalls
// exist *only* to back the POSIX compatibility layer's `signal()`/
// `sigaction()`/`kill()` shim so that ported programs (bash, coreutils,
// Python) that rely on asynchronous signal delivery work.
//
// Delivery model (mirrors the SEH-style exception delivery in
// `proc/exception.rs`):
//   1. A process registers a single userspace *trampoline* via
//      `SYS_SIGNAL_REGISTER`.  The POSIX runtime registers this at
//      startup; it dispatches to the per-signal handler table that lives
//      entirely in userspace.
//   2. A signal is posted to a target process's pending set via
//      `SYS_SIGNAL_SEND` (kill/raise).
//   3. On return to userspace from a syscall, the kernel checks the
//      current process's pending&~blocked set.  If a deliverable signal
//      exists and a trampoline is registered, the kernel builds a
//      `SignalContext` on the user stack, sets up arguments, and
//      redirects RIP to the trampoline.
//   4. The trampoline invokes the userspace handler, then calls
//      `SYS_SIGNAL_RETURN` to restore the interrupted context.
//
// The kernel deliberately knows nothing about per-signal handler
// disposition — userspace owns that table and decides terminate/ignore/
// invoke.  The kernel only tracks the pending set, the blocked mask, and
// the trampoline address.

/// Register the process-wide signal trampoline.
///
/// `arg0`: virtual address of the userspace trampoline function, or 0 to
/// unregister (revert to "no asynchronous delivery").  The trampoline is
/// invoked as `trampoline(signum: u64 /* rdi */, ctx: *mut SignalContext
/// /* rsi */)`.
///
/// Returns: 0 on success, negative `KernelError` code on failure (e.g.
/// the caller is not associated with a process).
pub const SYS_SIGNAL_REGISTER: u64 = 522;

/// Post a signal to a target process's pending set.
///
/// `arg0`: target process ID.
/// `arg1`: signal number (1..=64).
///
/// Sets the corresponding bit in the target's pending set.  Delivery
/// happens lazily the next time the target returns to userspace.  If the
/// target has no trampoline registered, the kernel applies the default
/// action (terminating signals kill the process; others are dropped).
///
/// Returns: 0 on success, negative `KernelError` code on failure
/// (`NoSuchProcess` if the PID is unknown, `InvalidArgument` for an
/// out-of-range signal number).
pub const SYS_SIGNAL_SEND: u64 = 523;

/// Return from a signal handler (sigreturn).
///
/// `arg0`: pointer to the `SignalContext` the kernel placed on the user
/// stack when it delivered the signal (the handler may have modified it).
///
/// Restores the saved CPU state and resumes the interrupted code.  Like
/// `SYS_EXCEPTION_RETURN`, this modifies the syscall frame directly and
/// is handled as a special case in `syscall_handler_inner`.  It does not
/// return to the caller.
pub const SYS_SIGNAL_RETURN: u64 = 524;

/// Set the calling process's blocked-signal mask.
///
/// `arg0`: new 64-bit blocked mask (bit `n-1` blocks signal `n`).
/// `arg1`: pointer to a `u64` that receives the previous mask, or 0 if
///         the previous mask is not wanted.
///
/// Blocked signals remain pending but are not delivered until unblocked.
/// `SIGKILL` and `SIGSTOP` cannot be blocked (their bits are ignored).
/// The previous mask is returned via the out-pointer rather than the
/// return value to avoid sign ambiguity (a blocked signal 64 sets bit
/// 63, which would look like a negative error code).
///
/// Returns: 0 on success, negative `KernelError` code on failure.
pub const SYS_SIGNAL_MASK: u64 = 525;

/// Query the calling process's pending-signal set.
///
/// `arg0`: pointer to a `u64` that receives the pending set (bit `n-1`
///         set means signal `n` is pending), observed without clearing
///         anything.  Used to back POSIX `sigpending()`.
///
/// Returns: 0 on success, negative `KernelError` code on failure.
pub const SYS_SIGNAL_PENDING: u64 = 526;

/// Fork the calling process, creating a copy-on-write child.
///
/// Takes no arguments.  The child inherits a copy-on-write clone of the
/// parent's address space, an independent copy of the parent's
/// capability table, refcount-shared copies of the parent's inheritable
/// handles (files, pipes, eventfds, stream sockets), the parent's
/// signal mask (but not pending signals), and the parent's filesystem
/// namespace.  The child has a single thread that resumes at the same
/// instruction the parent returns to.
///
/// Like `SYS_PROCESS_EXEC`, this syscall reads the saved register frame
/// directly: the parent observes the child's PID as the return value,
/// while the child observes 0.
///
/// Returns: child PID (> 0) to the parent, 0 to the child, or a
///          negative `KernelError` code on failure (parent only).
pub const SYS_PROCESS_FORK: u64 = 527;

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
///     `arg3`: pointer to output buffer for serialized entries.
///     `arg4`: output buffer capacity in bytes.
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

/// Mount a filesystem at a target path.
///
/// `arg0`: pointer to source/device string (may be empty for pseudo-filesystems).
/// `arg1`: source string length (bytes).
/// `arg2`: pointer to target mount-point path string.
/// `arg3`: target path length (bytes).
/// `arg4`: pointer to filesystem-type string (e.g. "ext4", "tmpfs", "vfat").
/// `arg5`: filesystem-type string length (bytes).
///
/// Root-only (requires mount authority). All six argument slots are
/// consumed by the three string pairs, so mount flags/options are
/// deferred to a future versioned extension syscall.
///
/// Returns: 0 on success, negative error code on failure.
pub const SYS_FS_MOUNT: u64 = 652;

/// Unmount the filesystem mounted at a target path.
///
/// `arg0`: pointer to target mount-point path string.
/// `arg1`: target path length (bytes).
///
/// Root-only (requires mount authority). Refuses to unmount the root
/// filesystem and refuses if sub-mounts exist beneath the target
/// (returns DeviceBusy).
///
/// Returns: 0 on success, negative error code on failure.
pub const SYS_FS_UMOUNT: u64 = 653;

/// Format (create a fresh filesystem on) a block device.
///
/// `arg0`: pointer to block-device name string (e.g. "vdb"; a leading
///         "/dev/" is accepted and stripped by the userspace tool).
/// `arg1`: device name length (bytes).
/// `arg2`: pointer to filesystem-type string (currently only the FAT family
///         is supported: "vfat"/"fat"/"fat32"/"fat16"/"msdos").
/// `arg3`: filesystem-type string length (bytes).
/// `arg4`: pointer to optional volume-label string (may be null/empty).
/// `arg5`: volume-label string length (bytes; 0 = no label).
///
/// Root-only (requires format authority). **Destructive** — all data on the
/// target device is lost. Returns 0 on success, negative error code on
/// failure (`NotSupported` for an unsupported fstype).
pub const SYS_FS_FORMAT: u64 = 654;

/// Check (and optionally repair) a filesystem on a block device (fsck).
///
/// `arg0`: pointer to block-device name string (e.g. "vdb"; a leading
///         "/dev/" is accepted and stripped by the userspace tool).
/// `arg1`: device name length (bytes).
/// `arg2`: flags bitfield — bit 0 (`1`) requests repair mode (write corrected
///         metadata); all other bits are reserved and must be 0.
///
/// Root-only (requires fsck authority). Currently only the FAT family is
/// supported (via the in-kernel `fsck_fat` checker). Returns, as a
/// non-negative value, the number of *outstanding* errors — problems detected
/// in check-only mode, or problems remaining after repair in repair mode
/// (0 = clean). A negative return is a `KernelError` (e.g. device not found, or
/// the volume is not a recognised FAT filesystem).
pub const SYS_FS_CHECK: u64 = 655;

/// Discard (TRIM) the free space of a mounted filesystem (fstrim).
///
/// `arg0`: pointer to block-device name string (e.g. "vda"; a leading
///         "/dev/" is accepted and stripped by the userspace tool).
/// `arg1`: device name length (bytes).
///
/// Root-only (requires disk-administration authority). Finds the mounted
/// filesystem backed by the named device and issues discard for every run of
/// free blocks (non-destructive — only free space is trimmed, live file data
/// is never touched). Returns, as a non-negative value, the number of bytes
/// discarded (0 if the device's filesystem cannot trim, e.g. the backing
/// device does not support discard). A negative return is a `KernelError`
/// (e.g. the device is not mounted, or the name is invalid).
pub const SYS_FS_TRIM: u64 = 656;

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
///
/// Record layout (528 bytes):
/// - `[0..256]`   affected path (NUL-padded)
/// - `[256..512]` new path for renames (NUL-padded)
/// - `[512..520]` watch id (`u64`, LE)
/// - `[520..524]` event type (`u32`, LE — the `FsEventType` discriminant)
/// - `[524]`      `is_dir` flag (`u8`: 1 if the subject is a directory, else 0)
/// - `[525..528]` reserved (zero)
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

/// Bind a TCP listener to a local port.
///
/// `arg0`: local port (1–65535).
///
/// Creates a listener socket that can accept incoming connections.
///
/// Returns: listener handle on success, negative error on failure.
pub const SYS_TCP_BIND: u64 = 804;

/// Accept an incoming TCP connection on a listener (blocking).
///
/// `arg0`: listener handle (from `SYS_TCP_BIND`).
///
/// Blocks until a client completes the 3-way handshake.
/// The returned handle is a regular TCP connection handle usable
/// with `SYS_TCP_SEND`, `SYS_TCP_RECV`, and `SYS_TCP_CLOSE`.
///
/// Returns: connection handle on success, negative error on failure.
pub const SYS_TCP_ACCEPT: u64 = 805;

/// Close a TCP listener, releasing the bound port.
///
/// `arg0`: listener handle.
///
/// Any pending connections that haven't been accepted are dropped.
///
/// Returns: 0 on success.
pub const SYS_TCP_CLOSE_LISTENER: u64 = 806;

/// Abort a TCP connection by sending RST.
///
/// `arg0`: socket handle.
///
/// Unlike `SYS_TCP_CLOSE` which performs an orderly FIN shutdown,
/// this immediately sends RST and reclaims the connection.  The
/// peer will see "connection reset" on its next read/write.  Use
/// this for error recovery or process cleanup.
///
/// Returns: 0 on success.
pub const SYS_TCP_ABORT: u64 = 807;

/// Get the remote peer address of a TCP connection.
///
/// `arg0`: connection handle.
/// `arg1`: pointer to 6-byte output buffer for peer address.
///
/// Writes the peer IPv4 address (4 bytes, network byte order)
/// followed by the peer port (2 bytes, network byte order):
///   [0..4]  IPv4 address
///   [4..6]  port
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_TCP_PEER_ADDR: u64 = 808;

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

/// Join a multicast group on a UDP socket (RFC 1112).
///
/// `arg0`: UDP socket handle.
/// `arg1`: multicast group address as a u32 in network byte order.
///
/// The socket will receive datagrams sent to the multicast group
/// address on the socket's bound port.
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_UDP_MCAST_JOIN: u64 = 814;

/// Leave a multicast group on a UDP socket.
///
/// `arg0`: UDP socket handle.
/// `arg1`: multicast group address as a u32 in network byte order.
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_UDP_MCAST_LEAVE: u64 = 815;

/// Set connected peer for a UDP socket (connected-mode filter).
///
/// `arg0`: socket handle.
/// `arg1`: peer IPv4 address (u32, network byte order).
/// `arg2`: peer port (u16, host byte order).
///
/// After connecting, recv/peek only return datagrams from this peer.
/// Pass ip=0, port=0 to disconnect (remove the filter).
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_UDP_CONNECT: u64 = 816;

/// `SYS_UDP_LOCAL_PORT` — query the local port of a UDP socket.
///
/// `arg0`: socket handle.
///
/// Returns the local port number (positive u16 range) on success,
/// or `InvalidArgument` if the handle is invalid or not active.
pub const SYS_UDP_LOCAL_PORT: u64 = 817;

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

/// Reverse-resolve an IPv4 address to a hostname via DNS PTR query.
///
/// `arg0`: IPv4 address as a 32-bit integer in **network byte order**
///         (big-endian, e.g., 192.168.1.1 = 0xC0A80101).
/// `arg1`: pointer to output buffer for the hostname string (not
///         null-terminated).
/// `arg2`: size of the output buffer in bytes.
///
/// Performs a blocking PTR query for the `in-addr.arpa` domain
/// (UDP, ~7s total timeout with retries).
///
/// Returns: number of bytes written to the output buffer on success
/// (the hostname length), or negative error on failure.
pub const SYS_DNS_REVERSE_RESOLVE: u64 = 821;

/// Query network interface statistics.
///
/// `arg0`: pointer to output buffer (48 bytes for `InterfaceStats`).
///
/// Writes 6 × u64 values (little-endian) into the buffer:
///   [0..8]   tx_bytes
///   [8..16]  tx_packets
///   [16..24] tx_errors
///   [24..32] rx_bytes
///   [32..40] rx_packets
///   [40..48] rx_drops
///
/// Returns: 0 on success, negative error on failure.
pub const SYS_NET_STAT: u64 = 825;

/// Send an ICMP Echo Request (ping) to an IPv4 address.
///
/// `arg0`: IPv4 address as a 32-bit integer in **network byte order**.
///
/// Sends a single ICMP Echo Request to the target and immediately
/// returns the sequence number used.  Use `SYS_ICMP_PING_WAIT` to
/// block until the reply arrives.
///
/// Returns: sequence number (u16) on success, negative error on failure.
pub const SYS_ICMP_PING: u64 = 830;

/// Wait for an ICMP Echo Reply for a given sequence number.
///
/// `arg0`: sequence number from `SYS_ICMP_PING`.
/// `arg1`: timeout in milliseconds (0 = default 2000ms).
///
/// Blocks (polling the NIC) until the reply arrives or the timeout
/// expires.
///
/// Returns: RTT in nanoseconds on success, or negative error if the
/// ping timed out (`TimedOut`).
pub const SYS_ICMP_PING_WAIT: u64 = 831;

// Diagnostic / listing syscalls (840-849)

/// List active TCP connections.
///
/// Writes an array of 20-byte connection records to the caller's buffer.
///
/// Each record layout:
/// ```text
/// [0..4]   = local IP (network order, always our IP)
/// [4..6]   = local port (network order)
/// [6..10]  = remote IP (network order)
/// [10..12] = remote port (network order)
/// [12]     = state (TcpState as u8)
/// [13..16] = rx_buffered (u24 LE, capped at 0xFFFFFF)
/// [16..19] = tx_buffered (u24 LE, capped at 0xFFFFFF)
/// [19]     = flags (bit 0=keepalive, bit 1=nagle, bit 2=ecn, bit 3=sack)
/// ```
///
/// - `arg0`: pointer to output buffer
/// - `arg1`: buffer length in bytes
///
/// Returns: number of connections written (may be < total if buffer too small).
pub const SYS_TCP_LIST: u64 = 840;

/// Query network interface configuration (IP, mask, gateway, DNS, MAC).
///
/// Writes a 24-byte configuration record to the caller's buffer:
///
/// ```text
/// [0..4]   = IPv4 address (network order)
/// [4..8]   = subnet mask (network order)
/// [8..12]  = gateway (network order)
/// [12..16] = DNS server (network order)
/// [16..22] = MAC address (6 bytes)
/// [22]     = flags (bit 0 = up)
/// [23]     = reserved (0)
/// ```
///
/// - `arg0`: pointer to output buffer (must be >= 24 bytes)
/// - `arg1`: buffer length in bytes
///
/// Returns 0 on success.
pub const SYS_NET_IF_INFO: u64 = 842;

/// List active TCP listeners.
///
/// Writes an array of 4-byte listener records to the caller's buffer.
///
/// Each record layout:
/// ```text
/// [0..2] = local port (network order)
/// [2]    = backlog used count
/// [3]    = backlog max capacity
/// ```
///
/// - `arg0`: pointer to output buffer
/// - `arg1`: buffer length in bytes
///
/// Returns: number of listeners written.
pub const SYS_TCP_LISTENER_LIST: u64 = 841;

/// Query the ARP cache.
///
/// Writes an array of 12-byte ARP entry records to the caller's buffer.
///
/// Each record layout:
/// ```text
/// [0..4]  = IPv4 address (network order)
/// [4..10] = MAC address (6 bytes)
/// [10..12] = TTL in seconds (u16 LE)
/// ```
///
/// - `arg0`: pointer to output buffer
/// - `arg1`: buffer length in bytes
///
/// Returns: number of entries written.
pub const SYS_ARP_TABLE: u64 = 843;

/// Query DNS cache statistics.
///
/// Writes 40 bytes of statistics:
///
/// ```text
/// [0..8]   = cache hits (u64 LE)
/// [8..16]  = cache misses (u64 LE)
/// [16..24] = evictions (u64 LE)
/// [24..28] = current entries (u32 LE)
/// [28..32] = capacity (u32 LE)
/// [32..40] = reserved (0)
/// ```
///
/// - `arg0`: pointer to output buffer (>= 40 bytes)
/// - `arg1`: buffer length in bytes
///
/// Returns 0 on success.
pub const SYS_DNS_CACHE_STATS: u64 = 844;

/// Query poll readiness of a TCP connection (for poll/select).
///
/// `arg0`: connection handle.
///
/// Returns: bitmask of readiness flags (POLLIN=1, POLLOUT=4, POLLERR=8,
/// POLLHUP=16), or negative error code if handle is invalid.
pub const SYS_TCP_POLL_STATUS: u64 = 845;

/// Check if a TCP listener has pending connections.
///
/// `arg0`: listener handle.
///
/// Returns: 1 if there are pending connections ready to accept,
/// 0 if the queue is empty, or negative error code.
pub const SYS_TCP_LISTENER_READY: u64 = 846;

/// Check if a UDP socket has datagrams ready to receive.
///
/// `arg0`: socket handle.
///
/// Returns: number of queued datagrams (≥0), or negative error code.
pub const SYS_UDP_RX_READY: u64 = 847;

/// Get the byte size of the first deliverable UDP datagram.
///
/// `arg0`: socket handle.
///
/// Returns the payload size (in bytes) of the front datagram that
/// would be returned by the next recv().  In connected mode, skips
/// non-matching datagrams.  Returns 0 if the queue is empty.
/// Used for FIONREAD on UDP sockets.
pub const SYS_UDP_RX_FRONT_BYTES: u64 = 848;

/// Shut down part of a TCP connection (half-close).
///
/// `arg0`: connection handle.
/// `arg1`: how — 0 = SHUT_RD, 1 = SHUT_WR, 2 = SHUT_RDWR.
///
/// SHUT_WR sends a FIN to the peer (half-close); further sends fail.
/// SHUT_RD discards incoming data; further reads return EOF.
///
/// Returns: 0 on success, negative error code.
pub const SYS_TCP_SHUTDOWN: u64 = 855;

/// Query detailed TCP connection information (for getsockopt TCP_INFO).
///
/// `arg0`: connection handle.
/// `arg1`: pointer to output buffer (at least 48 bytes).
/// `arg2`: buffer length.
///
/// Writes a packed structure:
/// ```text
/// [0..1]   u8   state (TcpState discriminant)
/// [1..2]   u8   flags (bit 0: keepalive, bit 1: nagle, bit 2: ecn,
///                       bit 3: sack, bit 4: wscale, bit 5: timestamps)
/// [2..4]   u16  effective MSS
/// [4..8]   u32  SRTT (nanoseconds / 1000 → microseconds)
/// [8..12]  u32  RTO (nanoseconds / 1000 → microseconds)
/// [12..16] u32  cwnd (bytes)
/// [16..20] u32  ssthresh (bytes)
/// [20..24] u32  snd_wnd (bytes)
/// [24..28] u32  rx_buffered (bytes)
/// [28..32] u32  tx_buffered (bytes, unacknowledged)
/// [32..36] u32  peer_mss
/// [36..40] u32  reserved
/// [40..48] u64  total_rx_bytes (0 for now)
/// ```
///
/// Returns: 0 on success, negative error code.
pub const SYS_TCP_INFO: u64 = 849;

/// `SYS_TCP_SET_NODELAY` — enable or disable TCP Nagle algorithm.
///
/// `arg0`: socket handle.
/// `arg1`: 0 = enable Nagle (default), 1 = disable Nagle (TCP_NODELAY).
pub const SYS_TCP_SET_NODELAY: u64 = 850;

/// `SYS_TCP_SET_KEEPALIVE` — enable or disable TCP keepalive probes.
///
/// `arg0`: socket handle.
/// `arg1`: 0 = disable, 1 = enable.
pub const SYS_TCP_SET_KEEPALIVE: u64 = 851;

/// `SYS_TCP_SET_KEEPALIVE_PARAMS` — configure TCP keepalive timing.
///
/// `arg0`: socket handle.
/// `arg1`: idle time in seconds (0 = use default).
/// `arg2`: probe interval in seconds (0 = use default).
/// `arg3`: max probe count (0 = use default).
pub const SYS_TCP_SET_KEEPALIVE_PARAMS: u64 = 852;

/// Query the last error code for a TCP connection.
///
/// Used by `getsockopt(SO_ERROR)` to report the correct POSIX error
/// after a connection failure (refused, reset, timed out).
///
/// Returns:
///   0 = no error (normal close)
///   1 = connection refused (ECONNREFUSED)
///   2 = connection reset (ECONNRESET)
///   3 = connection timed out (ETIMEDOUT)
///
/// `arg0`: connection handle.
pub const SYS_TCP_LAST_ERROR: u64 = 853;

/// `SYS_TCP_LOCAL_PORT` — query the local port of a TCP connection.
///
/// Returns the local port number (positive) on success, or a negative
/// error code if the handle is invalid.
///
/// `arg0`: connection handle.
pub const SYS_TCP_LOCAL_PORT: u64 = 854;

/// `SYS_NET_IF_CONFIG` — configure the primary network interface (write side
/// of [`SYS_NET_IF_INFO`]).
///
/// This is the native syscall behind `ifconfig`/`ip addr`/`ip link`/`route`
/// address configuration: it applies IPv4 address/mask/gateway/DNS and/or the
/// interface up/down state to the physical NIC (root network namespace). It is
/// **root-gated** (`CAP_NET_ADMIN`-class authority) because interface
/// reconfiguration is a system-wide side effect.
///
/// `arg0`: pointer to an 18-byte config record.
/// `arg1`: record length in bytes (must be >= 18).
///
/// Record layout (little-endian octet order, matching [`SYS_NET_IF_INFO`] for
/// the address fields):
/// ```text
/// [0..4]   IPv4 address
/// [4..8]   subnet mask
/// [8..12]  default gateway
/// [12..16] DNS server
/// [16]     up flag (0 = down, non-zero = up) — applied only if bit 4 of the mask is set
/// [17]     field mask: which fields to apply
///            bit0 = set IPv4 address
///            bit1 = set subnet mask
///            bit2 = set gateway
///            bit3 = set DNS server
///            bit4 = set up/down flag
/// ```
///
/// Unmasked fields are left unchanged (read-modify-write against the current
/// config), so `ip link set up` (mask = bit4 only) and `ip addr add`
/// (mask = bits 0|1) each touch only what they mean to. A mask of 0 is a
/// no-op success.
///
/// Returns 0 on success, or a negative `KernelError` (`PermissionDenied` if
/// the caller is not root, `InvalidArgument` on a bad pointer/length/mask).
pub const SYS_NET_IF_CONFIG: u64 = 856;

/// `SYS_NET_ROUTE_ADD` — add an IPv4 route to the caller's routing table.
///
/// Native syscall behind `ip route add <net>/<prefix> via <gw>` and
/// `route add -net <net> gw <gw>` for **non-default** routes. Operates on the
/// caller's network namespace (root netns for an unnamespaced process). The
/// *default* route (`0.0.0.0/0`) is not stored here — it remains the interface
/// gateway set via [`SYS_NET_IF_CONFIG`] (see design-decisions §52).
///
/// **Root-gated** (`CAP_NET_ADMIN`-class): the routing table is a system-wide
/// side effect.
///
/// `arg0`: pointer to a 16-byte route record.
/// `arg1`: record length in bytes (must be >= 16).
///
/// Record layout (network octet order for addresses, little-endian for metric):
/// ```text
/// [0..4]   destination network address
/// [4..8]   destination subnet mask (e.g. 255.255.255.0 for /24)
/// [8..12]  next-hop gateway (0.0.0.0 = directly connected)
/// [12..16] metric (u32 LE; lower = preferred)
/// ```
///
/// Rejects a `0.0.0.0/0` destination (`InvalidArgument`) — use
/// [`SYS_NET_IF_CONFIG`] for the default gateway. Returns 0 on success, or a
/// negative `KernelError` (`PermissionDenied`, `InvalidArgument`, or
/// `ResourceExhausted` if the table is full).
pub const SYS_NET_ROUTE_ADD: u64 = 857;

/// `SYS_NET_ROUTE_DEL` — remove an IPv4 route from the caller's routing table.
///
/// Native syscall behind `ip route del <net>/<prefix>` and
/// `route del -net <net>` for **non-default** routes. Removes the first route
/// matching `destination`+`mask` in the caller's network namespace.
///
/// **Root-gated** (`CAP_NET_ADMIN`-class).
///
/// `arg0`: pointer to an 8-byte record.
/// `arg1`: record length in bytes (must be >= 8).
///
/// Record layout:
/// ```text
/// [0..4]   destination network address
/// [4..8]   destination subnet mask
/// ```
///
/// Returns 0 on success, or a negative `KernelError` (`PermissionDenied`,
/// `InvalidArgument`, or `NotFound` if no route matches).
pub const SYS_NET_ROUTE_DEL: u64 = 858;

/// `SYS_NET_ROUTE_LIST` — enumerate the caller's IPv4 routing table.
///
/// Read-only (not gated): the routing table is not sensitive to read. Backs the
/// non-default rows of `ip route show` / `route -n`; the tools union these with
/// the interface default gateway from [`SYS_NET_IF_INFO`].
///
/// `arg0`: pointer to an output buffer.
/// `arg1`: buffer length in bytes.
///
/// Writes 16-byte records (same layout as [`SYS_NET_ROUTE_ADD`]) up to the
/// buffer capacity. Returns the number of records written (>= 0).
pub const SYS_NET_ROUTE_LIST: u64 = 859;

// ---------------------------------------------------------------------------
// Firewall control (860–864)
//
// Write-path syscalls for the packet-filtering firewall. Each operates on the
// caller's network namespace: the root namespace (ID 0) uses the global
// firewall state; child namespaces use their own per-namespace table (the same
// split the packet path uses via `check_inbound_ns`/`check_outbound_ns`). All
// are root-gated (`require_netadmin_authority`). Reads (status, rule listing)
// remain served by the firewall procfs file, so no read syscall is defined.
// ---------------------------------------------------------------------------

/// `SYS_NET_FW_ENABLE` — enable or disable the firewall for the caller's netns.
///
/// `arg0`: `1` to enable, `0` to disable.
/// Root-gated. Returns 0 on success.
pub const SYS_NET_FW_ENABLE: u64 = 860;

/// `SYS_NET_FW_SET_POLICY` — set the default policy (applied when no rule
/// matches) for the caller's netns.
///
/// `arg0`: `0` = accept, `1` = drop.
/// Root-gated. Returns 0 on success.
pub const SYS_NET_FW_SET_POLICY: u64 = 861;

/// `SYS_NET_FW_ADD_RULE` — add an IPv4 firewall rule to the caller's netns.
///
/// `arg0`: pointer to a 12-byte rule record.
/// `arg1`: record length in bytes (must be >= 12).
///
/// Record layout:
/// - `[0]`   direction (`0`=In, `1`=Out, `2`=Both)
/// - `[1]`   action (`0`=Allow, `1`=Deny)
/// - `[2]`   protocol (`0`=Any, `1`=TCP, `2`=UDP, `3`=ICMP)
/// - `[3]`   source prefix length (0..=32)
/// - `[4..6]`   destination port (u16, little-endian; 0 = any)
/// - `[6..8]`   priority (u16, little-endian; lower = evaluated first)
/// - `[8..12]`  source IPv4 address (network order; 0.0.0.0 = any)
///
/// Root-gated. Returns the assigned rule index (>= 0) on success.
pub const SYS_NET_FW_ADD_RULE: u64 = 862;

/// `SYS_NET_FW_DEL_RULE` — remove an IPv4 firewall rule by index from the
/// caller's netns.
///
/// `arg0`: rule index (as returned by [`SYS_NET_FW_ADD_RULE`] or shown in the
/// firewall procfs listing).
/// Root-gated. Returns 0 on success.
pub const SYS_NET_FW_DEL_RULE: u64 = 863;

/// `SYS_NET_FW_FLUSH` — remove all firewall rules from the caller's netns.
///
/// Root-gated. Returns 0 on success. Does not change the enabled state or the
/// default policy.
pub const SYS_NET_FW_FLUSH: u64 = 864;

// Raw layer-2 NIC access (865-868) — foundation of the userspace network
// stack migration (design-decisions.md §63, Path B).  Requires a
// `ResourceType::NetRaw` capability with WRITE rights.  Grants unfiltered
// Ethernet frame send/receive to a single exclusive owner (the `netstack`
// daemon); while a raw handle is held, the in-kernel stack stops draining the
// physical NIC (see `net::raw`).

/// Open (claim) exclusive raw layer-2 access to the physical NIC.
///
/// Requires a `ResourceType::NetRaw` capability with `WRITE` rights.  The claim
/// is exclusive: a second live process attempting to open fails with
/// `DeviceBusy`.  Re-opening from the same process is idempotent.  The claim
/// self-heals if the owner dies without closing.
///
/// - `arg0`: reserved (interface index; currently must be 0 = the primary NIC).
///
/// Returns: 0 on success (the raw handle is the process's NetRaw claim itself,
/// referenced implicitly by subsequent `SYS_NET_RAW_*` calls); negative error
/// otherwise.
pub const SYS_NET_RAW_OPEN: u64 = 865;

/// Transmit one raw Ethernet frame out the physical NIC.
///
/// The frame egresses verbatim — no protocol processing, no firewall.  The
/// caller must hold the raw claim (via `SYS_NET_RAW_OPEN`).
///
/// - `arg0`: pointer to the frame bytes.
/// - `arg1`: frame length (14..=1514 for standard Ethernet; must be >= 14).
///
/// Returns: 0 on success; `PermissionDenied` if the caller is not the raw
/// owner; `InvalidArgument` for a malformed length; propagates driver errors.
pub const SYS_NET_RAW_TX: u64 = 866;

/// Receive one raw Ethernet frame from the physical NIC (non-blocking).
///
/// - `arg0`: pointer to the output buffer.
/// - `arg1`: buffer capacity in bytes.
///
/// Returns: the frame length copied (> 0) on success; `WouldBlock` if no frame
/// is pending; `PermissionDenied` if the caller is not the raw owner;
/// `InvalidArgument` if the buffer is too small for the pending frame.
pub const SYS_NET_RAW_RX: u64 = 867;

/// Close (release) the raw NIC claim held by the caller.
///
/// Hands the physical NIC back to the in-kernel stack.  Idempotent; a non-owner
/// calling this is a no-op.
///
/// Returns: 0 always.
pub const SYS_NET_RAW_CLOSE: u64 = 868;

// ---------------------------------------------------------------------------
// Version info
// ---------------------------------------------------------------------------

/// Current syscall ABI version.
///
/// When the application declares a target version, the kernel uses the
/// corresponding dispatch table.  Version 1 is the initial set.
pub const CURRENT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// DRM/GPU syscalls (1000–1099)
// ---------------------------------------------------------------------------

/// Open a DRM device.
///
/// `arg0`: device index (0 = primary display device).
/// Returns: device handle (positive), or negative error.
pub const SYS_DRM_OPEN: u64 = 1000;

/// Close a DRM device handle.
///
/// `arg0`: device handle.
pub const SYS_DRM_CLOSE: u64 = 1001;

/// Get display dimensions (width, height) of a DRM device.
///
/// `arg0`: device handle.
/// Returns: `width | (height << 32)`.
pub const SYS_DRM_DISPLAY_SIZE: u64 = 1002;

/// Allocate a GEM (GPU) buffer object.
///
/// `arg0`: device handle.
/// `arg1`: width in pixels.
/// `arg2`: height in pixels.
/// `arg3`: pixel format (DRM fourcc u32, e.g., 0x34325258 for XRGB8888).
/// Returns: GEM handle (positive), or negative error.
pub const SYS_DRM_GEM_CREATE: u64 = 1010;

/// Free a GEM buffer object.
///
/// `arg0`: device handle.
/// `arg1`: GEM handle.
/// Returns: 0 on success, negative error.
pub const SYS_DRM_GEM_DESTROY: u64 = 1011;

/// Map a GEM buffer into the calling process's address space.
///
/// Creates a contiguous virtual mapping of the (potentially non-contiguous)
/// GEM backing frames.  The returned pointer is valid until `SYS_DRM_GEM_DESTROY`.
///
/// `arg0`: device handle.
/// `arg1`: GEM handle.
/// Returns: userspace virtual address, or negative error.
pub const SYS_DRM_GEM_MMAP: u64 = 1012;

/// Create a DRM framebuffer from a GEM handle.
///
/// `arg0`: device handle.
/// `arg1`: GEM handle.
/// `arg2`: `width | (height << 32)`.
/// `arg3`: `pitch | (format_fourcc << 32)`.
/// Returns: framebuffer object ID (positive), or negative error.
pub const SYS_DRM_FB_CREATE: u64 = 1020;

/// Destroy a DRM framebuffer.
///
/// `arg0`: device handle.
/// `arg1`: framebuffer object ID.
/// Returns: 0 on success, negative error.
pub const SYS_DRM_FB_DESTROY: u64 = 1021;

/// Page flip: display a framebuffer on a CRTC.
///
/// `arg0`: device handle.
/// `arg1`: CRTC object ID.
/// `arg2`: framebuffer object ID.
/// Returns: 0 on success, negative error.
pub const SYS_DRM_PAGE_FLIP: u64 = 1030;

/// Flush a dirty sub-region of a framebuffer to the display.
///
/// For paravirtualized GPUs, this triggers a host-side transfer.
/// For direct-scanout hardware, this may be a no-op.
///
/// `arg0`: device handle.
/// `arg1`: framebuffer object ID.
/// `arg2`: `x | (y << 32)`.
/// `arg3`: `w | (h << 32)`.
/// Returns: 0 on success, negative error.
pub const SYS_DRM_FLUSH_REGION: u64 = 1031;

/// Get connector status and info.
///
/// Returns connection status, connector type, and mode count
/// packed into a single u64.
///
/// `arg0`: device handle.
/// `arg1`: connector index (0-based within the device's connector list).
/// Returns: `status | (type << 8) | (mode_count << 16) | (connector_id << 32)`.
///   status: 0=disconnected, 1=connected, 2=unknown.
///   type: ConnectorType discriminant.
///   mode_count: number of supported modes.
///   connector_id: DrmObjectId raw value.
pub const SYS_DRM_CONNECTOR_STATUS: u64 = 1040;

/// Get a display mode's resolution and refresh rate.
///
/// `arg0`: device handle.
/// `arg1`: connector index.
/// `arg2`: mode index (within the connector's mode list).
/// Returns: `hdisplay | (vdisplay << 16) | (vrefresh << 32)`.
pub const SYS_DRM_MODE_GET: u64 = 1041;

/// Get CRTC info.
///
/// `arg0`: device handle.
/// `arg1`: CRTC index (0-based within the device's CRTC list).
/// Returns: `crtc_id | (active << 32) | (has_mode << 33)`.
pub const SYS_DRM_CRTC_INFO: u64 = 1042;

/// Set cursor image on a CRTC.
///
/// `arg0`: device handle.
/// `arg1`: CRTC object ID.
/// `arg2`: GEM handle for cursor image (0 = hide cursor).
/// `arg3`: `width | (height << 16) | (hot_x << 32) | (hot_y << 48)`.
/// Returns: 0 on success, negative error.
pub const SYS_DRM_CURSOR_SET: u64 = 1050;

/// Move cursor position on a CRTC.
///
/// `arg0`: device handle.
/// `arg1`: CRTC object ID.
/// `arg2`: `x` (signed, as i32 in low 32 bits).
/// `arg3`: `y` (signed, as i32 in low 32 bits).
/// Returns: 0 on success, negative error.
pub const SYS_DRM_CURSOR_MOVE: u64 = 1051;

/// Atomic modesetting commit.
///
/// Applies a batch of display state changes atomically.
/// The state description is a serialized buffer in userspace memory.
///
/// `arg0`: device handle.
/// `arg1`: pointer to serialized AtomicState buffer.
/// `arg2`: buffer length in bytes.
/// `arg3`: flags (bit 0 = test_only).
/// Returns: 0 on success, negative error.
///
/// Buffer format (little-endian):
///   [0..4]  u32  number of CRTC changes (N_crtc)
///   [4..8]  u32  number of plane changes (N_plane)
///   [8..12] u32  number of connector changes (N_conn)
///   Then N_crtc × 12 bytes each:
///     [+0..4]  u32  CRTC ID
///     [+4..8]  u32  flags: bit 0 = set_active, bit 1 = active_value,
///                          bit 2 = set_mode, bit 3 = disable_mode
///     [+8..12] u32  mode: hdisplay | (vdisplay << 16) when set_mode && !disable_mode
///   Then N_plane × 28 bytes each:
///     [+0..4]   u32  plane ID
///     [+4..8]   u32  flags: bit 0 = set_fb, bit 1 = set_crtc,
///                           bit 2 = set_src, bit 3 = set_dst
///     [+8..12]  u32  FB ID (or 0 to disable)
///     [+12..16] u32  CRTC ID (or 0 to unbind)
///     [+16..20] u32  src_x | (src_y << 16)
///     [+20..24] u32  src_w | (src_h << 16)
///     [+24..28] u32  dst_x | (dst_y << 16)  (packed as i16+i16)
///     [+28..32] u32  dst_w | (dst_h << 16)
///   Then N_conn × 8 bytes each:
///     [+0..4]  u32  connector ID
///     [+4..8]  u32  CRTC ID (or 0xFFFFFFFF to unbind)
pub const SYS_DRM_ATOMIC_COMMIT: u64 = 1060;

// ---------------------------------------------------------------------------
// Version info
// ---------------------------------------------------------------------------

/// Maximum supported syscall number.
///
/// The dispatch table is a flat array of this size for O(1) lookup.
/// Sparse — most entries are `None`.
pub const MAX_SYSCALL_NR: usize = 1100;
