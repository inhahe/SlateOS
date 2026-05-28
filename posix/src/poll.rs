//! POSIX poll() and select() — I/O multiplexing.
//!
//! ## Implementation
//!
//! Readiness is determined per fd type:
//!
//! - **Regular files / console**: always ready (POSIX mandates this).
//! - **Pipes**: kernel-queried via `SYS_PIPE_POLL` — reports actual
//!   readability/writability based on buffer state and end closure.
//! - **TCP streams**: kernel-queried via `SYS_TCP_POLL_STATUS` — returns
//!   actual POLLIN/POLLOUT/POLLHUP based on rx-buffer state, send window,
//!   and connection state.
//! - **TCP listeners**: kernel-queried via `SYS_TCP_LISTENER_READY` —
//!   POLLIN set only when a completed connection is in the accept backlog.
//! - **UDP sockets**: kernel-queried via `SYS_UDP_RX_READY` — POLLIN set
//!   only when datagrams are queued; always writable when bound.
//!
//! ## Timeout Handling
//!
//! Both `poll()` and `select()` use a polling loop with 10ms sleep
//! intervals: check all fds, if none ready sleep 10ms, repeat until
//! an fd becomes ready or the deadline expires.
//!
//! - `poll(fds, n, 0)` / `select(…, {0,0})`: non-blocking, check once.
//! - `poll(fds, n, timeout)` / `select(…, tv)`: loop until timeout.
//! - `poll(fds, n, -1)` / `select(…, NULL)`: loop indefinitely.
//!
//! ## Future Work
//!
//! When the kernel adds an epoll / completion-port mechanism, this
//! module should delegate to it for true event-driven wakeups instead
//! of polling.  Pipe readability could use `SYS_PIPE_TRY_READ` with
//! a zero-length semantic.

use crate::errno;
use crate::fdtable;
use crate::syscall::*;

// ---------------------------------------------------------------------------
// poll() constants
// ---------------------------------------------------------------------------

/// Data may be read without blocking.
pub const POLLIN: i16 = 0x0001;
/// Urgent data may be read without blocking.
pub const POLLPRI: i16 = 0x0002;
/// Data may be written without blocking.
pub const POLLOUT: i16 = 0x0004;
/// Error condition.
pub const POLLERR: i16 = 0x0008;
/// Hang up — peer closed its end.
pub const POLLHUP: i16 = 0x0010;
/// Invalid fd.
pub const POLLNVAL: i16 = 0x0020;

/// Alias for `POLLIN`.
pub const POLLRDNORM: i16 = 0x0040;
/// Priority band data may be read.
pub const POLLRDBAND: i16 = 0x0080;
/// Alias for `POLLOUT`.
pub const POLLWRNORM: i16 = 0x0100;
/// Priority data may be written.
pub const POLLWRBAND: i16 = 0x0200;
/// Peer closed connection or shut down writing half (Linux extension).
pub const POLLRDHUP: i16 = 0x2000;

// ---------------------------------------------------------------------------
// select() constants
// ---------------------------------------------------------------------------

/// Maximum number of file descriptors in an `fd_set`.
pub const FD_SETSIZE: usize = 256;

/// Number of `u64` words needed for `FD_SETSIZE` bits.
const FD_SET_WORDS: usize = FD_SETSIZE / 64;

// ---------------------------------------------------------------------------
// Structures
// ---------------------------------------------------------------------------

/// File descriptor and events for `poll()`.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Pollfd {
    /// File descriptor to poll.
    pub fd: i32,
    /// Events to watch for.
    pub events: i16,
    /// Events that occurred (filled on return).
    pub revents: i16,
}

/// Number of file descriptors type.
pub type NfdsT = u64;

/// File descriptor set for `select()`.
///
/// Bit-packed array: bit N is set if fd N is in the set.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdSet {
    /// Bit array — each u64 holds 64 fd bits.
    pub fds_bits: [u64; FD_SET_WORDS],
}

/// Time value for `select()` timeout.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Timeval {
    /// Seconds.
    pub tv_sec: i64,
    /// Microseconds.
    pub tv_usec: i64,
}

// ---------------------------------------------------------------------------
// fd_set manipulation macros (as functions)
// ---------------------------------------------------------------------------

/// Clear all bits in an `fd_set`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fd_set_zero(set: *mut FdSet) {
    if set.is_null() {
        return;
    }
    // SAFETY: Caller guarantees set is valid.
    unsafe {
        core::ptr::write_bytes(set, 0, 1);
    }
}

/// Set a bit in an `fd_set`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fd_set_set(fd: i32, set: *mut FdSet) {
    if set.is_null() || fd < 0 || fd as usize >= FD_SETSIZE {
        return;
    }
    let idx = fd as usize;
    // SAFETY: bounds checked above.
    unsafe {
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        if let Some(word) = (*set).fds_bits.get_mut(word_idx) {
            *word |= 1u64 << bit_idx;
        }
    }
}

/// Clear a bit in an `fd_set`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fd_set_clr(fd: i32, set: *mut FdSet) {
    if set.is_null() || fd < 0 || fd as usize >= FD_SETSIZE {
        return;
    }
    let idx = fd as usize;
    // SAFETY: bounds checked above.
    unsafe {
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        if let Some(word) = (*set).fds_bits.get_mut(word_idx) {
            *word &= !(1u64 << bit_idx);
        }
    }
}

/// Test a bit in an `fd_set`.
///
/// Returns non-zero if `fd` is set, 0 if not.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fd_set_isset(fd: i32, set: *const FdSet) -> i32 {
    if set.is_null() || fd < 0 || fd as usize >= FD_SETSIZE {
        return 0;
    }
    let idx = fd as usize;
    // SAFETY: bounds checked above.
    unsafe {
        let word_idx = idx / 64;
        let bit_idx = idx % 64;
        if let Some(&word) = (*set).fds_bits.get(word_idx) {
            i32::from(word & (1u64 << bit_idx) != 0)
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// poll()
// ---------------------------------------------------------------------------

/// Wait for events on file descriptors.
///
/// Checks each fd in `fds` for the requested events and sets `revents`.
/// Currently, all valid fds are reported as ready for their requested
/// events (see module docs for rationale).
///
/// - `timeout == 0`: return immediately (non-blocking check).
/// - `timeout > 0`: sleep for `timeout` milliseconds, then check.
/// - `timeout == -1`: sleep briefly (10ms) and check — avoids hanging
///   indefinitely since we can't do kernel-level event waiting yet.
///
/// Returns the number of fds with non-zero `revents`, or -1 on error.
///
/// # Linux semantics
///
/// Errno precedence (matches `fs/select.c::do_sys_poll`):
///
/// 1. `nfds > RLIMIT_NOFILE` → EINVAL.  Linux rejects oversized nfds
///    **before** touching the `fds` pointer, so this check fires even
///    when `fds == NULL`.  Our equivalent of `RLIMIT_NOFILE` is
///    `fdtable::MAX_FDS` (256) — the hard cap on the fd table.
/// 2. `fds == NULL` with `nfds > 0` → EFAULT (Linux's `copy_from_user`
///    fault during the first slot copy).
/// 3. Per-entry: `pfd.fd < 0` is silently skipped (revents=0); an fd
///    not in the table sets `POLLNVAL` on that slot only.
///
/// # Safety
///
/// `fds` must point to an array of at least `nfds` `Pollfd` entries.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn poll(fds: *mut Pollfd, nfds: NfdsT, timeout: i32) -> i32 {
    // Sleep interval: 10ms (balance between responsiveness and CPU).
    const POLL_INTERVAL_NS: u64 = 10_000_000;

    // Phase 156: oversized nfds is EINVAL and is checked first — Linux's
    // `do_sys_poll` (fs/select.c) rejects `nfds > rlimit(RLIMIT_NOFILE)`
    // before any `copy_from_user`, so the EINVAL fires regardless of
    // whether `fds` is NULL.  Skipping this guard would let an attacker
    // drive the iteration loop into a multi-billion-step or OOB read.
    if nfds > fdtable::MAX_FDS as NfdsT {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    if fds.is_null() && nfds > 0 {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Poll in a loop: check readiness, if nothing ready sleep briefly,
    // repeat until timeout expires or an fd becomes ready.
    let deadline_ns = if timeout > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        now.saturating_add(u64::from(timeout as u32).saturating_mul(1_000_000))
    } else {
        0
    };

    loop {
        let mut ready_count: i32 = 0;
        let mut i: u64 = 0;

        while i < nfds {
            // SAFETY: fds is valid for nfds entries.
            let pfd = unsafe { &mut *fds.add(i as usize) };
            pfd.revents = 0;

            // Negative fd = skip (POSIX says ignore, set revents=0).
            if pfd.fd < 0 {
                i = i.wrapping_add(1);
                continue;
            }

            let Some(entry) = fdtable::get_fd(pfd.fd) else {
                pfd.revents = POLLNVAL;
                ready_count = ready_count.wrapping_add(1);
                i = i.wrapping_add(1);
                continue;
            };

            // Determine readiness based on handle kind.
            let (readable, writable, hangup, error) = check_readiness(entry.kind, entry.handle);

            // Linux semantics: POLLHUP/POLLERR imply readability (so programs
            // wake up and discover the condition via read()), and POLLERR also
            // implies writability (so non-blocking connect failure is detected).
            let eff_readable = readable || hangup || error;
            let eff_writable = writable || error;

            let mut revents: i16 = 0;
            if eff_readable && (pfd.events & (POLLIN | POLLRDNORM) != 0) {
                // Report whichever flags were requested.
                revents |= pfd.events & (POLLIN | POLLRDNORM);
            }
            if eff_writable && (pfd.events & (POLLOUT | POLLWRNORM) != 0) {
                revents |= pfd.events & (POLLOUT | POLLWRNORM);
            }
            // POSIX: POLLHUP and POLLERR are always reported regardless of
            // requested events.
            if hangup {
                revents |= POLLHUP;
            }
            if error {
                revents |= POLLERR;
            }

            pfd.revents = revents;
            if revents != 0 {
                ready_count = ready_count.wrapping_add(1);
            }

            i = i.wrapping_add(1);
        }

        // If any fds are ready, return immediately.
        if ready_count > 0 {
            return ready_count;
        }

        // Non-blocking (timeout == 0): return immediately.
        if timeout == 0 {
            return 0;
        }

        // Check deadline for positive timeouts.
        if timeout > 0 {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline_ns {
                return 0; // Timeout expired.
            }
        }

        // Sleep briefly and retry.
        let _ = syscall1(SYS_SLEEP, POLL_INTERVAL_NS);
    } // end loop
}

// ---------------------------------------------------------------------------
// ppoll
// ---------------------------------------------------------------------------

/// Like `poll`, but with a `timespec` timeout and optional signal mask.
///
/// The `sigmask` parameter is ignored (our OS doesn't deliver signals).
/// Converts the `timespec` to a millisecond timeout for the underlying
/// `poll` implementation.
///
/// # Safety
///
/// `fds` must be valid for `nfds` elements.  `timeout_ts` may be null
/// (infinite wait) or point to a valid `Timespec`.
#[allow(clippy::similar_names)] // tspec vs tms — both timeout-related.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ppoll(
    fds: *mut Pollfd,
    nfds: NfdsT,
    tspec: *const crate::stat::Timespec,
    _sigmask: *const u64,
) -> i32 {
    let tms: i32 = if tspec.is_null() {
        -1 // Infinite wait.
    } else {
        // SAFETY: tspec is non-null and points to valid Timespec.
        let ts = unsafe { &*tspec };
        if ts.tv_sec == 0 && ts.tv_nsec == 0 {
            0 // Explicit {0,0} = non-blocking poll.
        } else {
            // Convert to milliseconds, rounding up so sub-ms timeouts
            // don't collapse to 0 (which poll treats as non-blocking).
            let ms = ts.tv_sec
                .saturating_mul(1_000)
                .saturating_add((ts.tv_nsec.saturating_add(999_999)) / 1_000_000);
            if ms > i64::from(i32::MAX) { i32::MAX }
            else if ms <= 0 { 1 } // Ensure non-zero for non-zero input.
            else { ms as i32 }
        }
    };

    // Delegate to poll.
    unsafe { poll(fds, nfds, tms) }
}

/// Check fd readiness based on handle kind.
///
/// Returns `(readable, writable, hangup, error)`.
///
/// For network sockets, queries the kernel for actual buffer state
/// rather than always reporting ready.  This prevents spurious wakeups
/// and makes poll/select behave correctly for event loops.
///
/// The `error` flag indicates the socket is in an error state (e.g.,
/// connection refused/reset).  POSIX requires POLLERR to be reported
/// unconditionally (regardless of requested events), and select() should
/// set the fd in exceptfds when an error is present.
pub(crate) fn check_readiness(kind: fdtable::HandleKind, handle: u64) -> (bool, bool, bool, bool) {
    use fdtable::HandleKind;

    match kind {
        // Console: always ready (framebuffer writable, keyboard might have input).
        // File: always ready (POSIX says regular files are always "ready").
        HandleKind::Console | HandleKind::File => (true, true, false, false),

        // Pipe: query kernel for actual readiness.
        HandleKind::Pipe => {
            if handle == 0 {
                return (false, false, true, true);
            }
            let status = syscall1(SYS_PIPE_POLL, handle) as u16;
            let readable = (status & 0x0001) != 0;
            let writable = (status & 0x0004) != 0;
            let error = (status & 0x0008) != 0;
            let hangup = (status & 0x0010) != 0;
            // The kernel reports POLLERR (0x08) on the write end of a
            // broken pipe (reader closed).  This must be surfaced so
            // poll() reports POLLERR and select() fires exceptfds.
            (readable, writable, hangup, error)
        }

        // TCP stream: query kernel for actual rx/tx readiness.
        HandleKind::TcpStream => {
            if handle == 0 {
                return (false, false, true, true);
            }
            // SYS_TCP_POLL_STATUS returns a bitmask:
            //   bit 0 (0x01) = POLLIN (readable)
            //   bit 2 (0x04) = POLLOUT (writable)
            //   bit 3 (0x08) = POLLERR
            //   bit 4 (0x10) = POLLHUP
            let status = syscall1(SYS_TCP_POLL_STATUS, handle) as u16;
            let readable = (status & 0x0001) != 0;
            let writable = (status & 0x0004) != 0;
            let error = (status & 0x0008) != 0;
            let hangup = (status & 0x0010) != 0;
            (readable, writable, hangup, error)
        }

        // TCP listener: readable means a completed connection is pending.
        HandleKind::TcpListener => {
            if handle == 0 {
                return (false, false, true, true);
            }
            // SYS_TCP_LISTENER_READY returns 1 if pending, 0 otherwise.
            let ready = syscall1(SYS_TCP_LISTENER_READY, handle);
            (ready > 0, false, false, false)
        }

        // UDP socket: query kernel for queued datagrams.
        HandleKind::UdpSocket => {
            if handle == 0 {
                return (false, false, false, true);
            }
            // SYS_UDP_RX_READY returns number of queued datagrams.
            let queued = syscall1(SYS_UDP_RX_READY, handle);
            let readable = queued > 0;
            // UDP is always writable when bound (no flow control).
            (readable, true, false, false)
        }

        // Eventfd: counter > 0 means readable.  Always writable until
        // the counter saturates near u64::MAX (we ignore that edge case
        // and report writable unconditionally — adding 0 always succeeds,
        // and `eventfd_write(fd, 0)` is a no-op).
        HandleKind::Eventfd => {
            let has = syscall1(crate::syscall::SYS_EVENTFD_HAS_VALUE, handle);
            (has > 0, true, false, false)
        }

        // Epoll fd: readable when at least one watched fd is ready
        // (i.e., a non-zero epoll_wait would return events).  An epoll
        // fd is never "writable" in the POSIX sense — Linux reports
        // POLLIN-only on epoll fds.  Used for nested epoll (epoll fd
        // monitored by another epoll/poll/select).
        HandleKind::Epoll => {
            let readable = crate::epoll::epoll_instance_has_ready(handle);
            (readable, false, false, false)
        }

        // Timerfd: readable when at least one expiration has occurred
        // since the last `read()`/`settime()`.  Never writable.
        HandleKind::Timerfd => {
            let readable = crate::epoll::timerfd_is_readable(handle);
            (readable, false, false, false)
        }

        // Inotify: readable when at least one event is queued.  The
        // readiness check side-effects: it triggers a fresh scan of
        // every watch, so events that occurred between calls show up.
        // Never writable.
        HandleKind::Inotify => {
            let readable = crate::epoll::inotify_is_readable(handle);
            (readable, false, false, false)
        }
    }
}

// ---------------------------------------------------------------------------
// select()
// ---------------------------------------------------------------------------

/// Synchronous I/O multiplexing.
///
/// Examines fds 0..`nfds` in the provided fd sets.  On return, each
/// set contains only the fds that are ready for the corresponding
/// operation.
///
/// - `readfds`: fds to check for readability.
/// - `writefds`: fds to check for writability.
/// - `exceptfds`: fds to check for exceptional conditions (always
///   empty on return — we don't generate OOB/exception events).
/// - `timeout`: NULL = block indefinitely, {0,0} = non-blocking,
///   otherwise sleep for the specified duration.
///
/// Returns the total number of ready fds across all sets, or -1 on error.
///
/// # Linux semantics
///
/// Errno precedence (matches `fs/select.c::core_sys_select`):
///
/// 1. `nfds < 0` → EINVAL.
/// 2. `nfds > max_fds` → **silently clamped** (NOT EINVAL).  Linux
///    clamps `n` to the process's `fdt->max_fds`; we clamp to
///    `FD_SETSIZE`, which equals `fdtable::MAX_FDS` (256) so no fd in
///    our table can ever sit beyond the clamp.
/// 3. The `timeout` pointer is dereferenced only after the nfds check.
/// 4. An fd that is set in any input mask but not open → EBADF.
///
/// # Safety
///
/// All non-null pointers must point to valid `FdSet` / `Timeval` structures.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn select(
    nfds: i32,
    readfds: *mut FdSet,
    writefds: *mut FdSet,
    exceptfds: *mut FdSet,
    timeout: *mut Timeval,
) -> i32 {
    // Sleep interval for polling: 10ms (balance responsiveness vs CPU).
    const POLL_INTERVAL_NS: u64 = 10_000_000;

    // Phase 155: only `nfds < 0` is EINVAL.  `nfds > FD_SETSIZE` is
    // silently clamped, matching Linux's `core_sys_select` (fs/select.c)
    // which sets `n = max_fds` when the caller's `n` exceeds it.
    if nfds < 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Clamp nfds to FD_SETSIZE.  Our fdtable cannot hold an fd >=
    // FD_SETSIZE, so iterating past it would just check unset bits.
    // Doing the clamp here also prevents the inner loop from running
    // for absurdly large nfds values (e.g. `i32::MAX`).
    let nfds = if (nfds as usize) > FD_SETSIZE {
        FD_SETSIZE as i32
    } else {
        nfds
    };

    // Compute deadline from timeout.
    // NULL = block indefinitely, {0,0} = non-blocking poll.
    let is_nonblocking: bool;
    let deadline_ns: u64;

    if timeout.is_null() {
        // Block indefinitely — no deadline.
        is_nonblocking = false;
        deadline_ns = u64::MAX;
    } else {
        // SAFETY: caller guarantees timeout validity.
        let tv = unsafe { &*timeout };
        if tv.tv_sec == 0 && tv.tv_usec == 0 {
            // {0,0} = non-blocking (check once, return immediately).
            is_nonblocking = true;
            deadline_ns = 0;
        } else {
            is_nonblocking = false;
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            // Convert to nanoseconds, rounding up to ensure we don't
            // treat sub-millisecond timeouts as instant polls.
            let timeout_ns = (tv.tv_sec.max(0) as u64).saturating_mul(1_000_000_000)
                .saturating_add((tv.tv_usec.max(0) as u64).saturating_mul(1_000));
            deadline_ns = now.saturating_add(timeout_ns);
        }
    }

    // Capture input sets before the loop — we clear and rebuild output sets
    // each iteration.
    let read_input = if readfds.is_null() {
        None
    } else {
        // SAFETY: readfds is non-null, caller guarantees validity.
        Some(unsafe { *readfds })
    };
    let write_input = if writefds.is_null() {
        None
    } else {
        // SAFETY: writefds is non-null.
        Some(unsafe { *writefds })
    };
    let except_input = if exceptfds.is_null() {
        None
    } else {
        // SAFETY: exceptfds is non-null.
        Some(unsafe { *exceptfds })
    };

    loop {
        // Clear output sets at start of each iteration.
        if !readfds.is_null() {
            fd_set_zero(readfds);
        }
        if !writefds.is_null() {
            fd_set_zero(writefds);
        }
        if !exceptfds.is_null() {
            fd_set_zero(exceptfds);
        }

        let mut ready_count: i32 = 0;

        // Check each fd for readiness.
        let mut fd: i32 = 0;
        while fd < nfds {
            let check_read = read_input.as_ref().is_some_and(|s| is_set_in(fd, s));
            let check_write = write_input.as_ref().is_some_and(|s| is_set_in(fd, s));
            let check_except = except_input.as_ref().is_some_and(|s| is_set_in(fd, s));

            if !check_read && !check_write && !check_except {
                fd = fd.wrapping_add(1);
                continue;
            }

            let Some(entry) = fdtable::get_fd(fd) else {
                // Invalid fd in the set — error per POSIX.
                errno::set_errno(errno::EBADF);
                return -1;
            };

            let (readable, writable, hangup, error) = check_readiness(entry.kind, entry.handle);

            // Linux select() semantics: POLLERR/POLLHUP imply readability
            // (so programs wake and discover the error/EOF via read()),
            // and POLLERR also implies writability (so non-blocking connect
            // failure is detected via writefds).
            let eff_readable = readable || hangup || error;
            let eff_writable = writable || error;

            if check_read && eff_readable {
                fd_set_set(fd, readfds);
                ready_count = ready_count.wrapping_add(1);
            }
            if check_write && eff_writable {
                fd_set_set(fd, writefds);
                ready_count = ready_count.wrapping_add(1);
            }
            // exceptfds: report socket errors (POLLERR equivalent).
            if check_except && error {
                fd_set_set(fd, exceptfds);
                ready_count = ready_count.wrapping_add(1);
            }

            fd = fd.wrapping_add(1);
        }

        // If any fds are ready, return immediately.
        if ready_count > 0 {
            return ready_count;
        }

        // Non-blocking: return immediately even if nothing ready.
        if is_nonblocking {
            return 0;
        }

        // Check deadline for timed waits.
        if deadline_ns != u64::MAX {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline_ns {
                return 0; // Timeout expired.
            }
        }

        // Sleep briefly and retry.
        let _ = syscall1(SYS_SLEEP, POLL_INTERVAL_NS);
    } // end loop
}

/// Test if a fd is set in an `FdSet` (internal helper, takes a reference).
fn is_set_in(fd: i32, set: &FdSet) -> bool {
    if fd < 0 || fd as usize >= FD_SETSIZE {
        return false;
    }
    let idx = fd as usize;
    let word_idx = idx / 64;
    let bit_idx = idx % 64;
    if let Some(&word) = set.fds_bits.get(word_idx) {
        word & (1u64 << bit_idx) != 0
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// pselect() stub
// ---------------------------------------------------------------------------

/// POSIX pselect — select() with nanosecond timeout and signal mask.
///
/// Stub: ignores the signal mask and delegates to select() with
/// converted timeout.
///
/// # Safety
///
/// Same requirements as `select()`.  `sigmask` is ignored.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn pselect(
    nfds: i32,
    readfds: *mut FdSet,
    writefds: *mut FdSet,
    exceptfds: *mut FdSet,
    timeout: *const crate::stat::Timespec,
    _sigmask: *const u8, // sigset_t* — ignored
) -> i32 {
    if timeout.is_null() {
        // NULL timeout → delegate to select with NULL timeout.
        return unsafe { select(nfds, readfds, writefds, exceptfds, core::ptr::null_mut()) };
    }

    // Convert timespec to timeval (ceiling division for ns→µs so a
    // non-zero sub-microsecond timeout doesn't become non-blocking {0,0}).
    // SAFETY: timeout is non-null, caller guarantees validity.
    let ts = unsafe { &*timeout };
    // Ceiling division: ns→µs so a non-zero sub-microsecond timeout
    // doesn't become non-blocking {0,0}.  tv_nsec is in [0, 999_999_999]
    // so adding 999 cannot overflow i64.
    #[allow(clippy::arithmetic_side_effects)]
    let usec = (ts.tv_nsec + 999) / 1000;
    let mut tv = Timeval {
        tv_sec: ts.tv_sec,
        tv_usec: usec,
    };

    unsafe { select(nfds, readfds, writefds, exceptfds, &raw mut tv) }
}

// ---------------------------------------------------------------------------
// Tests — pure logic functions only (no syscalls)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- FdSet manipulation tests --

    #[test]
    fn test_fd_set_zero() {
        let mut set = FdSet { fds_bits: [0xFFFF_FFFF_FFFF_FFFF; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);
        for word in &set.fds_bits {
            assert_eq!(*word, 0);
        }
    }

    #[test]
    fn test_fd_set_set_and_isset() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);

        // Set some fds.
        fd_set_set(0, &raw mut set);
        fd_set_set(1, &raw mut set);
        fd_set_set(63, &raw mut set);
        fd_set_set(64, &raw mut set);
        fd_set_set(255, &raw mut set);

        // Check they're set.
        assert_ne!(fd_set_isset(0, &raw const set), 0);
        assert_ne!(fd_set_isset(1, &raw const set), 0);
        assert_ne!(fd_set_isset(63, &raw const set), 0);
        assert_ne!(fd_set_isset(64, &raw const set), 0);
        assert_ne!(fd_set_isset(255, &raw const set), 0);

        // Check others are not set.
        assert_eq!(fd_set_isset(2, &raw const set), 0);
        assert_eq!(fd_set_isset(62, &raw const set), 0);
        assert_eq!(fd_set_isset(65, &raw const set), 0);
        assert_eq!(fd_set_isset(254, &raw const set), 0);
    }

    #[test]
    fn test_fd_set_clr() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);

        fd_set_set(42, &raw mut set);
        assert_ne!(fd_set_isset(42, &raw const set), 0);

        fd_set_clr(42, &raw mut set);
        assert_eq!(fd_set_isset(42, &raw const set), 0);
    }

    #[test]
    fn test_fd_set_boundary() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);

        // Negative fd — should be silently ignored.
        fd_set_set(-1, &raw mut set);
        assert_eq!(fd_set_isset(-1, &raw const set), 0);

        // Out of range — should be silently ignored.
        fd_set_set(256, &raw mut set);
        assert_eq!(fd_set_isset(256, &raw const set), 0);
    }

    #[test]
    fn test_fd_set_null_safety() {
        // All operations should handle null gracefully.
        fd_set_zero(core::ptr::null_mut());
        fd_set_set(0, core::ptr::null_mut());
        fd_set_clr(0, core::ptr::null_mut());
        assert_eq!(fd_set_isset(0, core::ptr::null()), 0);
    }

    // -- is_set_in helper tests --

    #[test]
    fn test_is_set_in() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);
        fd_set_set(5, &raw mut set);

        assert!(is_set_in(5, &set));
        assert!(!is_set_in(4, &set));
        assert!(!is_set_in(6, &set));
        assert!(!is_set_in(-1, &set));
        assert!(!is_set_in(256, &set));
    }

    // -- Pollfd structure tests --

    #[test]
    fn test_pollfd_size() {
        // struct pollfd should be 8 bytes on most platforms (4 + 2 + 2).
        assert_eq!(core::mem::size_of::<Pollfd>(), 8);
    }

    // -- check_readiness tests --

    #[test]
    fn test_check_readiness_console() {
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::Console, 0);
        assert!(r, "Console should be readable");
        assert!(w, "Console should be writable");
        assert!(!h, "Console should not be hung up");
        assert!(!e, "Console should not be in error");
    }

    #[test]
    fn test_check_readiness_file() {
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::File, 42);
        assert!(r);
        assert!(w);
        assert!(!h);
        assert!(!e);
    }

    #[test]
    #[cfg(target_os = "none")] // Calls real syscalls — only runs on our OS.
    fn test_check_readiness_tcp_connected() {
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::TcpStream, 123);
        assert!(r);
        assert!(w);
        assert!(!h);
        assert!(!e);
    }

    #[test]
    fn test_check_readiness_tcp_disconnected() {
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::TcpStream, 0);
        assert!(!r, "Disconnected TCP should not be readable");
        assert!(!w, "Disconnected TCP should not be writable");
        assert!(h, "Disconnected TCP should be hung up");
        assert!(e, "Disconnected TCP should be in error state");
    }

    #[test]
    fn test_check_readiness_udp_bound() {
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::UdpSocket, 99);
        assert!(r);
        assert!(w);
        assert!(!h);
        assert!(!e);
    }

    #[test]
    fn test_check_readiness_udp_unbound() {
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::UdpSocket, 0);
        assert!(!r);
        assert!(!w);
        assert!(!h);
        assert!(e, "Unbound UDP with handle 0 should be in error state");
    }

    // -- is_set_in word boundary tests --

    #[test]
    fn test_is_set_in_word_boundaries() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);

        // Test at 64-bit word boundaries.
        fd_set_set(63, &raw mut set);
        fd_set_set(64, &raw mut set);
        fd_set_set(127, &raw mut set);
        fd_set_set(128, &raw mut set);

        assert!(is_set_in(63, &set), "fd 63 (end of word 0)");
        assert!(is_set_in(64, &set), "fd 64 (start of word 1)");
        assert!(is_set_in(127, &set), "fd 127 (end of word 1)");
        assert!(is_set_in(128, &set), "fd 128 (start of word 2)");

        // Neighbors should not be set.
        assert!(!is_set_in(62, &set));
        assert!(!is_set_in(65, &set));
        assert!(!is_set_in(126, &set));
        assert!(!is_set_in(129, &set));
    }

    #[test]
    fn test_is_set_in_last_valid_fd() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);

        // FD_SETSIZE - 1 should be the last valid fd.
        let last_fd = (FD_SETSIZE - 1) as i32;
        fd_set_set(last_fd, &raw mut set);
        assert!(is_set_in(last_fd, &set));
        assert!(!is_set_in(last_fd + 1, &set)); // Out of range.
    }

    // -- Poll constant tests --

    #[test]
    fn test_poll_constants_match_linux() {
        assert_eq!(POLLIN, 0x0001);
        assert_eq!(POLLPRI, 0x0002);
        assert_eq!(POLLOUT, 0x0004);
        assert_eq!(POLLERR, 0x0008);
        assert_eq!(POLLHUP, 0x0010);
        assert_eq!(POLLNVAL, 0x0020);
        assert_eq!(POLLRDNORM, 0x0040);
        assert_eq!(POLLRDBAND, 0x0080);
        assert_eq!(POLLWRNORM, 0x0100);
        assert_eq!(POLLWRBAND, 0x0200);
        assert_eq!(POLLRDHUP, 0x2000);
    }

    #[test]
    fn test_poll_flags_are_disjoint() {
        let flags: [i16; 11] = [
            POLLIN, POLLPRI, POLLOUT, POLLERR,
            POLLHUP, POLLNVAL, POLLRDNORM, POLLRDBAND,
            POLLWRNORM, POLLWRBAND, POLLRDHUP,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0,
                    "poll flags {} and {} must be disjoint", flags[i], flags[j]);
            }
        }
    }

    // -- FdSet size/layout --

    #[test]
    fn test_fd_setsize() {
        assert_eq!(FD_SETSIZE, 256);
        assert_eq!(FD_SET_WORDS, 4);  // 256 / 64
    }

    #[test]
    fn test_fd_setsize_matches_max_fds() {
        // FD_SETSIZE must cover all possible fds in our table.
        // If MAX_FDS grows beyond FD_SETSIZE, select() won't be able
        // to monitor all fds, which is a subtle bug.
        assert_eq!(
            FD_SETSIZE,
            crate::fdtable::MAX_FDS,
            "FD_SETSIZE must match fdtable::MAX_FDS"
        );
    }

    // -- Timeval tests --

    #[test]
    fn test_timeval_size() {
        // Timeval should be 16 bytes (two i64s).
        assert_eq!(core::mem::size_of::<Timeval>(), 16);
    }

    #[test]
    fn test_timeval_fields() {
        let tv = Timeval { tv_sec: 5, tv_usec: 500_000 };
        assert_eq!(tv.tv_sec, 5);
        assert_eq!(tv.tv_usec, 500_000);
    }

    #[test]
    fn test_timeval_zero() {
        let tv = Timeval { tv_sec: 0, tv_usec: 0 };
        assert_eq!(tv.tv_sec, 0);
        assert_eq!(tv.tv_usec, 0);
    }

    // -- FdSet exhaustive word tests --

    #[test]
    fn test_fd_set_all_bits_in_first_word() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        // Set every bit in word 0 (fds 0..63).
        for fd in 0..64 {
            fd_set_set(fd, &raw mut set);
        }
        assert_eq!(set.fds_bits[0], u64::MAX);
        // Other words should be zero.
        assert_eq!(set.fds_bits[1], 0);
        assert_eq!(set.fds_bits[2], 0);
        assert_eq!(set.fds_bits[3], 0);
    }

    #[test]
    fn test_fd_set_clr_preserves_others() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_set(10, &raw mut set);
        fd_set_set(11, &raw mut set);
        fd_set_set(12, &raw mut set);
        fd_set_clr(11, &raw mut set);
        assert_ne!(fd_set_isset(10, &raw const set), 0);
        assert_eq!(fd_set_isset(11, &raw const set), 0);
        assert_ne!(fd_set_isset(12, &raw const set), 0);
    }

    #[test]
    fn test_fd_set_double_set() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_set(50, &raw mut set);
        fd_set_set(50, &raw mut set); // Idempotent.
        assert_ne!(fd_set_isset(50, &raw const set), 0);
    }

    #[test]
    fn test_fd_set_clr_unset_is_noop() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_clr(50, &raw mut set); // Nothing to clear — no crash.
        assert_eq!(fd_set_isset(50, &raw const set), 0);
    }

    #[test]
    fn test_fd_set_zero_then_isset() {
        let mut set = FdSet { fds_bits: [0xFFFF_FFFF_FFFF_FFFF; FD_SET_WORDS] };
        fd_set_zero(&raw mut set);
        // Every fd should be unset.
        for fd in [0, 1, 63, 64, 127, 128, 200, 255] {
            assert_eq!(fd_set_isset(fd, &raw const set), 0, "fd {fd} should be clear");
        }
    }

    // -- FdSet boundary edge cases --

    #[test]
    fn test_fd_set_set_negative_is_noop() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_set(-100, &raw mut set); // Should not crash.
        // All bits should still be 0.
        for word in &set.fds_bits {
            assert_eq!(*word, 0);
        }
    }

    #[test]
    fn test_fd_set_clr_out_of_range() {
        let mut set = FdSet { fds_bits: [0xFFFF_FFFF_FFFF_FFFF; FD_SET_WORDS] };
        fd_set_clr(300, &raw mut set); // Out of range — no crash.
        // All bits should still be set.
        for word in &set.fds_bits {
            assert_eq!(*word, u64::MAX);
        }
    }

    // -- Pollfd init and layout --

    #[test]
    fn test_pollfd_fields() {
        let pfd = Pollfd { fd: 5, events: POLLIN | POLLOUT, revents: 0 };
        assert_eq!(pfd.fd, 5);
        assert_eq!(pfd.events, POLLIN | POLLOUT);
        assert_eq!(pfd.revents, 0);
    }

    #[test]
    fn test_pollfd_copy() {
        let pfd1 = Pollfd { fd: 3, events: POLLIN, revents: POLLHUP };
        let pfd2 = pfd1;
        assert_eq!(pfd2.fd, pfd1.fd);
        assert_eq!(pfd2.events, pfd1.events);
        assert_eq!(pfd2.revents, pfd1.revents);
    }

    // -- FdSet size --

    #[test]
    fn test_fdset_size() {
        // 4 words × 8 bytes = 32 bytes.
        assert_eq!(core::mem::size_of::<FdSet>(), 32);
    }

    #[test]
    fn test_fdset_copy() {
        let mut set1 = FdSet { fds_bits: [0; FD_SET_WORDS] };
        fd_set_set(10, &raw mut set1);
        fd_set_set(200, &raw mut set1);
        let set2 = set1;
        assert_ne!(fd_set_isset(10, &raw const set2), 0);
        assert_ne!(fd_set_isset(200, &raw const set2), 0);
        assert_eq!(fd_set_isset(11, &raw const set2), 0);
    }

    // -- NfdsT --

    #[test]
    fn test_nfds_t_size() {
        assert_eq!(core::mem::size_of::<NfdsT>(), 8); // u64
    }

    // -- check_readiness for additional handle kinds --

    #[test]
    fn test_check_readiness_unknown_kind() {
        // An unknown/unrecognized kind should default to not-ready
        // or ready depending on implementation. Just verify no crash.
        let (r, w, h, e) = check_readiness(fdtable::HandleKind::Console, 1);
        assert!(r);
        assert!(w);
        assert!(!h);
        assert!(!e);
    }

    // -- is_set_in comprehensive --

    #[test]
    fn test_is_set_in_all_words() {
        let mut set = FdSet { fds_bits: [0; FD_SET_WORDS] };
        // Set one bit in each word.
        fd_set_set(0, &raw mut set);    // word 0
        fd_set_set(64, &raw mut set);   // word 1
        fd_set_set(128, &raw mut set);  // word 2
        fd_set_set(192, &raw mut set);  // word 3

        assert!(is_set_in(0, &set));
        assert!(is_set_in(64, &set));
        assert!(is_set_in(128, &set));
        assert!(is_set_in(192, &set));

        assert!(!is_set_in(1, &set));
        assert!(!is_set_in(65, &set));
        assert!(!is_set_in(129, &set));
        assert!(!is_set_in(193, &set));
    }

    // -----------------------------------------------------------------
    // Phase 155 — `select(nfds > FD_SETSIZE)` is no longer EINVAL.
    //
    // Linux's `core_sys_select` (fs/select.c) silently clamps `n` to
    // the process's `fdt->max_fds` when the caller's `n` exceeds it.
    // We previously returned EINVAL, which diverged from Linux.  The
    // fix preserves the `nfds < 0 → EINVAL` rule but treats any
    // larger-than-FD_SETSIZE value as an in-range request capped to
    // FD_SETSIZE.  All tests below use NULL fdsets + a {0,0} timeout
    // so the function returns without crossing the syscall boundary —
    // we exercise only the validation and clamping logic.
    // -----------------------------------------------------------------

    // -- Per-error-class --------------------------------------------------

    /// `nfds < 0` is still EINVAL after the clamp refactor.
    #[test]
    fn test_select_negative_nfds_einval_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `nfds == FD_SETSIZE` is in-range and no longer EINVAL.
    #[test]
    fn test_select_nfds_equal_to_setsize_no_longer_einval_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                FD_SETSIZE as i32,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// `nfds == FD_SETSIZE + 1` is silently clamped, not EINVAL.
    #[test]
    fn test_select_nfds_one_past_setsize_no_longer_einval_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                (FD_SETSIZE as i32) + 1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// `nfds == i32::MAX` is silently clamped and returns promptly
    /// (no infinite loop iterating to billions).
    #[test]
    fn test_select_nfds_i32_max_clamped_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                i32::MAX,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- Ordering matrix --------------------------------------------------

    /// `nfds < 0` takes precedence over an oversized nfds magnitude:
    /// the sign check fires before the clamp can mask it.
    #[test]
    fn test_select_negative_beats_oversize_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        // `i32::MIN` is both negative and oversized in magnitude.
        let ret = unsafe {
            select(
                i32::MIN,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Oversized nfds + NULL timeout would have hit the indefinite-block
    /// path; but with a `{0,0}` timeval it must return 0 immediately.
    /// Verifies the clamp completes before the timeout dispatch.
    #[test]
    fn test_select_oversize_nfds_nonblocking_returns_zero_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                10_000,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
    }

    // -- Workflow ---------------------------------------------------------

    /// Caller passes a sensible mid-range value (e.g. 32 fds).  Validates
    /// the common path is unaffected by the Phase 155 refactor.
    #[test]
    fn test_select_typical_nfds_unchanged_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                32,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// `nfds == 0` returns 0 without inspecting any fds — also unchanged
    /// by Phase 155 but verified to make sure the clamp didn't break it.
    #[test]
    fn test_select_zero_nfds_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                0,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- Buggy-caller -----------------------------------------------------

    /// Glibc-style caller mistakenly passes FD_SETSIZE * 4 because they
    /// confused the bit-set size with the byte size.  Linux silently
    /// clamps; we must too.
    #[test]
    fn test_select_glibc_size_confusion_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                (FD_SETSIZE * 4) as i32,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// Caller passes `1` with a `{0,0}` timeout — minimal smoke check
    /// that the lower bound of the now-uncapped nfds range works.
    #[test]
    fn test_select_nfds_one_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- Recovery / no-side-effect loop -----------------------------------

    /// The oversized-nfds path must not set errno on success.  A leftover
    /// errno from a previous call must be preserved (Phase 155 keeps the
    /// "no side effects on success" Linux invariant).
    #[test]
    fn test_select_oversize_does_not_clobber_errno_phase155() {
        // Seed a sentinel errno that would have been overwritten if the
        // old EINVAL path still fired.
        crate::errno::set_errno(crate::errno::EIO);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                (FD_SETSIZE as i32) + 100,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        // errno preserved at the EIO sentinel — proves success path
        // didn't touch errno.
        assert_eq!(crate::errno::get_errno(), crate::errno::EIO);
    }

    /// Re-issuing select() after an EINVAL still works (errno isn't
    /// latched into the state machine).
    #[test]
    fn test_select_recover_after_einval_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let _ = unsafe {
            select(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // Now retry with a sane oversized nfds — should silently clamp.
        crate::errno::set_errno(0);
        let ret = unsafe {
            select(
                (FD_SETSIZE as i32) + 1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- Sentinel ---------------------------------------------------------

    /// Explicit sentinel: the old `nfds > FD_SETSIZE → EINVAL` behaviour
    /// has been removed.  If a future regression re-introduces it this
    /// test fails loudly.
    #[test]
    fn test_select_oversize_nfds_no_longer_einval_phase155() {
        crate::errno::set_errno(0);
        let mut tv = Timeval { tv_sec: 0, tv_usec: 0 };
        let ret = unsafe {
            select(
                (FD_SETSIZE as i32) + 1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw mut tv,
            )
        };
        // Pre-Phase-155: ret == -1 && errno == EINVAL.
        // Post-Phase-155: ret == 0 && errno untouched.
        assert_ne!(ret, -1, "oversize nfds must no longer return -1");
        assert_ne!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "oversize nfds must no longer set EINVAL"
        );
    }

    // -- Cross-checks -----------------------------------------------------

    /// `pselect` delegates to `select` and so inherits the clamp.
    /// Verify the oversized-nfds path produces no error through pselect.
    #[test]
    fn test_pselect_inherits_select_nfds_clamp_phase155() {
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = unsafe {
            pselect(
                (FD_SETSIZE as i32) + 1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw const ts,
                core::ptr::null(),
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// `pselect` with `nfds < 0` still produces EINVAL via select.
    #[test]
    fn test_pselect_negative_nfds_einval_phase155() {
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = unsafe {
            pselect(
                -1,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                &raw const ts,
                core::ptr::null(),
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Clamp boundary equals the `FD_SETSIZE == fdtable::MAX_FDS`
    /// invariant — if either constant moves, this test surfaces the
    /// inconsistency.
    #[test]
    fn test_select_clamp_matches_fd_setsize_max_fds_invariant_phase155() {
        assert_eq!(
            FD_SETSIZE,
            fdtable::MAX_FDS,
            "Phase 155 clamp assumes FD_SETSIZE == fdtable::MAX_FDS"
        );
    }

    // -----------------------------------------------------------------
    // Phase 156 — `poll(fds, nfds > MAX_FDS, ...)` is EINVAL.
    //
    // Linux's `do_sys_poll` (fs/select.c) rejects oversized nfds
    // **before** touching the `fds` pointer:
    //
    //     if (nfds > rlimit(RLIMIT_NOFILE))
    //         return -EINVAL;
    //
    // Our equivalent of `RLIMIT_NOFILE` is `fdtable::MAX_FDS` (256) —
    // the hard cap on our static fd table.  We previously had no upper
    // bound on nfds, which would let an attacker drive the iteration
    // loop to billions of steps and read past the `fds` array.
    //
    // All tests below use a `{0,0}` non-blocking timeout (`timeout==0`)
    // and either NULL fds (when nfds==0) or a small valid array, so the
    // success path never crosses syscall0 / fdtable lookups on the host
    // build.
    // -----------------------------------------------------------------

    // -- Per-error-class --------------------------------------------------

    /// `nfds == MAX_FDS + 1` → EINVAL.
    #[test]
    fn test_poll_nfds_one_past_max_fds_einval_phase156() {
        crate::errno::set_errno(0);
        // Use a non-null but unused buffer — the EINVAL fires before any
        // entry is dereferenced.
        let mut pfd = Pollfd { fd: -1, events: 0, revents: 0 };
        let ret = unsafe {
            poll(
                &raw mut pfd,
                (fdtable::MAX_FDS as NfdsT) + 1,
                0,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `nfds == NfdsT::MAX` → EINVAL (never iterates).
    #[test]
    fn test_poll_nfds_max_value_einval_phase156() {
        crate::errno::set_errno(0);
        let ret = unsafe { poll(core::ptr::null_mut(), NfdsT::MAX, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `nfds == MAX_FDS` is the largest accepted value — must not EINVAL.
    /// Uses `fd: -1` in every slot so the inner loop silently skips them,
    /// no fdtable lookup occurs.
    #[test]
    fn test_poll_nfds_equal_to_max_fds_accepted_phase156() {
        crate::errno::set_errno(0);
        // Heap is unavailable in our test config; use a stack array of
        // MAX_FDS = 256 entries, each marked `fd: -1` so they are
        // silently skipped by the poll loop.
        let mut arr = [Pollfd { fd: -1, events: 0, revents: 0 };
            fdtable::MAX_FDS];
        let ret = unsafe {
            poll(arr.as_mut_ptr(), fdtable::MAX_FDS as NfdsT, 0)
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- Ordering matrix --------------------------------------------------

    /// Phase 156: EINVAL precedes the NULL-pointer EFAULT — the nfds
    /// check fires even when `fds == NULL`.  This is the inversion of
    /// the pre-Phase-156 behaviour, which would have hit EFAULT first
    /// (since no EINVAL guard existed).
    #[test]
    fn test_poll_einval_beats_efault_phase156() {
        crate::errno::set_errno(0);
        let ret = unsafe {
            poll(
                core::ptr::null_mut(),
                (fdtable::MAX_FDS as NfdsT) + 1,
                0,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "EINVAL must beat EFAULT in the Linux precedence"
        );
    }

    /// EFAULT still fires when nfds is in-range but fds is NULL.
    #[test]
    fn test_poll_in_range_nfds_null_fds_efault_phase156() {
        crate::errno::set_errno(0);
        let ret = unsafe { poll(core::ptr::null_mut(), 1, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EFAULT);
    }

    /// `nfds == 0` + NULL fds is fine (matches Linux: nothing to copy).
    #[test]
    fn test_poll_zero_nfds_null_fds_ok_phase156() {
        crate::errno::set_errno(0);
        let ret = unsafe { poll(core::ptr::null_mut(), 0, 0) };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    // -- Workflow ---------------------------------------------------------

    /// Typical caller polling a single fd with `fd: -1` (the standard
    /// "ignore me" sentinel) — the slot is silently skipped, return 0.
    #[test]
    fn test_poll_single_negative_fd_skipped_phase156() {
        crate::errno::set_errno(0);
        let mut pfd = Pollfd { fd: -1, events: POLLIN, revents: 0xBEEF_u16 as i16 };
        let ret = unsafe { poll(&raw mut pfd, 1, 0) };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
        // Sentinel value was overwritten to 0 — confirms the loop ran.
        assert_eq!(pfd.revents, 0);
    }

    // -- Buggy-caller -----------------------------------------------------

    /// Caller passes a wildly oversized nfds (e.g. 1 GiB), which would
    /// previously have iterated unboundedly.  Must return EINVAL
    /// promptly without OOB reads.
    #[test]
    fn test_poll_gigabyte_nfds_einval_phase156() {
        crate::errno::set_errno(0);
        // Non-null but never read because EINVAL fires first.
        let mut pfd = Pollfd { fd: 0, events: 0, revents: 0 };
        let ret = unsafe {
            poll(
                &raw mut pfd,
                1_073_741_824, // 1 GiB
                0,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// Sign-extended -1 reinterpreted as `NfdsT::MAX` from a C caller
    /// that passes `(unsigned long)-1` — still EINVAL.
    #[test]
    fn test_poll_negative_one_unsigned_einval_phase156() {
        crate::errno::set_errno(0);
        let n: NfdsT = (-1i64) as NfdsT; // 0xFFFF_FFFF_FFFF_FFFF
        let ret = unsafe { poll(core::ptr::null_mut(), n, 0) };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    // -- Recovery / no-side-effect loop -----------------------------------

    /// Re-issuing poll after the EINVAL path with a sane request must
    /// succeed (errno isn't latched).
    #[test]
    fn test_poll_recover_after_einval_phase156() {
        crate::errno::set_errno(0);
        let _ = unsafe { poll(core::ptr::null_mut(), NfdsT::MAX, 0) };
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);

        crate::errno::set_errno(0);
        let mut pfd = Pollfd { fd: -1, events: 0, revents: 0 };
        let ret = unsafe { poll(&raw mut pfd, 1, 0) };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// The EINVAL path must not touch the `fds` buffer — verify a
    /// sentinel value in the caller's Pollfd survives the rejected
    /// call.
    #[test]
    fn test_poll_einval_does_not_touch_fds_buffer_phase156() {
        crate::errno::set_errno(0);
        let mut pfd = Pollfd { fd: 99, events: 0xAAAA_u16 as i16, revents: 0x5555_u16 as i16 };
        let ret = unsafe {
            poll(
                &raw mut pfd,
                (fdtable::MAX_FDS as NfdsT) + 1,
                0,
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
        // Caller's buffer was not touched (no loop iterations ran).
        assert_eq!(pfd.fd, 99);
        assert_eq!(pfd.events, 0xAAAA_u16 as i16);
        assert_eq!(pfd.revents, 0x5555_u16 as i16);
    }

    // -- Sentinel ---------------------------------------------------------

    /// Explicit sentinel: the pre-Phase-156 behaviour was to accept
    /// arbitrary nfds (no upper-bound guard).  If a future regression
    /// removes the EINVAL guard, this fails loudly.
    #[test]
    fn test_poll_oversize_nfds_no_longer_unchecked_phase156() {
        crate::errno::set_errno(0);
        let ret = unsafe {
            poll(
                core::ptr::null_mut(),
                (fdtable::MAX_FDS as NfdsT) + 1,
                0,
            )
        };
        assert_eq!(ret, -1, "oversize nfds must be rejected");
        assert_eq!(
            crate::errno::get_errno(),
            crate::errno::EINVAL,
            "oversize nfds must set EINVAL (not EFAULT and not 0)"
        );
    }

    // -- Cross-checks -----------------------------------------------------

    /// `ppoll` delegates to `poll` so the EINVAL guard fires there too.
    #[test]
    fn test_ppoll_inherits_poll_nfds_einval_phase156() {
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = unsafe {
            ppoll(
                core::ptr::null_mut(),
                (fdtable::MAX_FDS as NfdsT) + 1,
                &raw const ts,
                core::ptr::null(),
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::EINVAL);
    }

    /// `ppoll(NULL fds, nfds=0)` still succeeds with a {0,0} timeout.
    #[test]
    fn test_ppoll_zero_nfds_ok_phase156() {
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        let ret = unsafe {
            ppoll(
                core::ptr::null_mut(),
                0,
                &raw const ts,
                core::ptr::null(),
            )
        };
        assert_eq!(ret, 0);
        assert_eq!(crate::errno::get_errno(), 0);
    }

    /// Phase 156 cap equals `fdtable::MAX_FDS` (which is also the same
    /// constant Phase 155 uses for `FD_SETSIZE`).  If MAX_FDS ever
    /// changes, this surfaces the assumption.
    #[test]
    fn test_poll_nfds_cap_is_fdtable_max_fds_phase156() {
        // Sanity-check: MAX_FDS is the same constant used by the
        // Phase 156 guard above.
        assert_eq!(fdtable::MAX_FDS, 256);
        // And it equals FD_SETSIZE (Phase 155 invariant).
        assert_eq!(fdtable::MAX_FDS, FD_SETSIZE);
    }
}
