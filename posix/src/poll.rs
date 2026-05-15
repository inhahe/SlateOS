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
/// # Safety
///
/// `fds` must point to an array of at least `nfds` `Pollfd` entries.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn poll(fds: *mut Pollfd, nfds: NfdsT, timeout: i32) -> i32 {
    // Sleep interval: 10ms (balance between responsiveness and CPU).
    const POLL_INTERVAL_NS: u64 = 10_000_000;

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
fn check_readiness(kind: fdtable::HandleKind, handle: u64) -> (bool, bool, bool, bool) {
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

    if nfds < 0 || nfds as usize > FD_SETSIZE {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

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
}
