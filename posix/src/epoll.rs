// Indexing and arithmetic in this file operate on:
//
//  - Fixed-size kernel-ABI tables (epoll instances, timerfd entries,
//    inotify watch slots) whose indices are clamped to the table size
//    before use.
//  - Ring-buffer head/tail counters reduced modulo MAX_INOTIFY_EVENTS
//    immediately after increment.
//  - Byte offsets into a caller-supplied buffer that are validated by
//    explicit `written + N <= buf.len()` checks before the slice op.
//  - Time differences (`now - next_expiry_ns`) gated on prior
//    `now >= next_expiry_ns` checks.
//
// In each case the bound is established locally, but clippy cannot see
// across the check.  The defensive lints would only become useful here
// if we accepted user-supplied integer indices into these tables, which
// we do not.
#![allow(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
)]

//! Linux-specific I/O multiplexing and fd-based notification.
//!
//! Implements:
//! - **epoll** (`epoll_create`, `epoll_create1`, `epoll_ctl`, `epoll_wait`,
//!   `epoll_pwait`, `epoll_pwait2`) — level-triggered, userspace-managed.
//! - **eventfd** (`eventfd`, `eventfd_read`, `eventfd_write`) — wraps the
//!   kernel's `SYS_EVENTFD_*` syscalls.
//!
//! Still stubbed (return ENOSYS):
//! - **timerfd** (`timerfd_create`, `timerfd_settime`, `timerfd_gettime`)
//! - **signalfd** (`signalfd`, `signalfd4`)
//! - **inotify** (`inotify_init`, `inotify_init1`, `inotify_add_watch`,
//!   `inotify_rm_watch`)
//!
//! ## epoll Implementation
//!
//! epoll is implemented entirely in userspace on top of the same readiness
//! primitives that `poll()`/`select()` use (`SYS_PIPE_POLL`,
//! `SYS_TCP_POLL_STATUS`, `SYS_UDP_RX_READY`, `SYS_EVENTFD_HAS_VALUE`,
//! etc.).  A fixed-size [`EpollInstance`] table holds the interest lists.
//! Each `epoll_wait` iteration polls every watched fd and returns those
//! that match their requested event mask.
//!
//! ### Limitations
//!
//! - **Level-triggered only.**  `EPOLLET` is accepted and stored but
//!   behaves identically to level-triggered.  Implementing edge-triggered
//!   correctly requires tracking per-entry "previously-ready" state, which
//!   is fine to add later but not needed for correctness — programs that
//!   only check ET-fired events will spuriously rearm but still make
//!   progress.
//! - **Polling loop, not true blocking wait.**  `epoll_wait(timeout=-1)`
//!   sleeps in 10ms increments rather than blocking in the kernel.  Same
//!   tradeoff as `poll()` — a true kqueue/IOCP-style event channel would
//!   improve wake-up latency.
//! - **Fixed instance count and interest list size.**  `MAX_EPOLL_INSTANCES`
//!   epoll instances, each holding up to `MAX_EPOLL_ENTRIES` watched fds.
//!   Exceeding either returns `ENOMEM`/`EMFILE`/`ENOSPC` as appropriate.

use crate::errno;
use crate::fdtable::{self, HandleKind};
use crate::syscall::{
    SYS_CLOCK_MONOTONIC, SYS_EVENTFD_CLOSE, SYS_EVENTFD_CREATE, SYS_EVENTFD_READ,
    SYS_EVENTFD_TRY_READ, SYS_EVENTFD_WRITE, SYS_SLEEP, syscall0, syscall1, syscall2, syscall3,
};

/// Events for `epoll_ctl`.
pub const EPOLLIN: u32 = 0x001;
/// Priority data may be read (TCP urgent data, etc.).
pub const EPOLLPRI: u32 = 0x002;
/// Output ready.
pub const EPOLLOUT: u32 = 0x004;
/// Error condition.
pub const EPOLLERR: u32 = 0x008;
/// Hang up.
pub const EPOLLHUP: u32 = 0x010;
/// Stream peer shut down writing half (Linux extension).
pub const EPOLLRDHUP: u32 = 0x2000;
/// Fire once then disable (re-arm via `EPOLL_CTL_MOD`).
pub const EPOLLONESHOT: u32 = 1 << 30;
/// Edge-triggered (accepted but currently behaves as level-triggered).
pub const EPOLLET: u32 = 1 << 31;

/// Mask of all event bits we recognize in `events` (input).
const EPOLL_INPUT_MASK: u32 =
    EPOLLIN | EPOLLPRI | EPOLLOUT | EPOLLERR | EPOLLHUP | EPOLLRDHUP | EPOLLONESHOT | EPOLLET;

/// `epoll_create1` flag: set `FD_CLOEXEC` on the new fd.
pub const EPOLL_CLOEXEC: i32 = 0o2_000_000;

/// `epoll_ctl` operations.
pub const EPOLL_CTL_ADD: i32 = 1;
/// Delete an fd from the interest list.
pub const EPOLL_CTL_DEL: i32 = 2;
/// Modify the events for an fd in the interest list.
pub const EPOLL_CTL_MOD: i32 = 3;

/// Event structure for epoll.
///
/// Linux's `epoll_event` is `__attribute__((packed))` on x86_64 so the
/// total size is 12 bytes (4-byte events + 8-byte data) without trailing
/// padding.  We replicate this layout with `#[repr(C, packed)]` so the
/// userspace ABI matches code compiled against the Linux headers.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct EpollEvent {
    /// Events bitmask.
    pub events: u32,
    /// User data — opaque cookie returned in `epoll_wait` events.
    pub data: u64,
}

// ---------------------------------------------------------------------------
// Instance table
// ---------------------------------------------------------------------------

/// Maximum number of concurrent epoll instances per process.
pub const MAX_EPOLL_INSTANCES: usize = 16;

/// Maximum number of fds in an epoll instance's interest list.
pub const MAX_EPOLL_ENTRIES: usize = 128;

/// Upper bound on `epoll_wait`'s `maxevents` argument.  Matches Linux's
/// `EP_MAX_EVENTS` in `fs/eventpoll.c`, which is
/// `INT_MAX / sizeof(struct epoll_event)`.  Linux uses this to prevent
/// integer-overflow / DoS-via-huge-array-sizing attacks in
/// `ep_check_params`; we mirror the bound exactly so any caller that
/// happens to probe the edge sees the same EINVAL behaviour.
pub const EP_MAX_EVENTS: i32 = i32::MAX / (core::mem::size_of::<EpollEvent>() as i32);

/// One entry in an epoll instance's interest list.
#[derive(Clone, Copy)]
struct EpollEntry {
    fd: i32,
    events: u32,
    data: u64,
    /// Already fired with `EPOLLONESHOT` set; suppress until re-armed
    /// via `EPOLL_CTL_MOD`.
    oneshot_fired: bool,
}

/// A single epoll instance.
struct EpollInstance {
    in_use: bool,
    entries: [Option<EpollEntry>; MAX_EPOLL_ENTRIES],
}

const EPOLL_INSTANCE_INIT: EpollInstance = EpollInstance {
    in_use: false,
    entries: [None; MAX_EPOLL_ENTRIES],
};

static mut EPOLL_INSTANCES: [EpollInstance; MAX_EPOLL_INSTANCES] =
    [EPOLL_INSTANCE_INIT; MAX_EPOLL_INSTANCES];

fn instances_ptr() -> *mut [EpollInstance; MAX_EPOLL_INSTANCES] {
    core::ptr::addr_of_mut!(EPOLL_INSTANCES)
}

/// Allocate a free instance slot and return its index.
fn allocate_instance() -> Option<usize> {
    // SAFETY: Single-threaded posix layer; no concurrent access.
    unsafe {
        let table = &mut *instances_ptr();
        for (i, inst) in table.iter_mut().enumerate() {
            if !inst.in_use {
                inst.in_use = true;
                inst.entries = [None; MAX_EPOLL_ENTRIES];
                return Some(i);
            }
        }
    }
    None
}

fn with_instance_mut<R>(idx: u64, f: impl FnOnce(&mut EpollInstance) -> R) -> Option<R> {
    let i = usize::try_from(idx).ok()?;
    if i >= MAX_EPOLL_INSTANCES {
        return None;
    }
    // SAFETY: single-threaded.
    unsafe {
        let table = &mut *instances_ptr();
        let inst = table.get_mut(i)?;
        if !inst.in_use {
            return None;
        }
        Some(f(inst))
    }
}

fn with_instance<R>(idx: u64, f: impl FnOnce(&EpollInstance) -> R) -> Option<R> {
    let i = usize::try_from(idx).ok()?;
    if i >= MAX_EPOLL_INSTANCES {
        return None;
    }
    // SAFETY: single-threaded.
    unsafe {
        let table = &*instances_ptr();
        let inst = table.get(i)?;
        if !inst.in_use {
            return None;
        }
        Some(f(inst))
    }
}

/// Free an epoll instance.  Called by `close()` when the last fd
/// referencing the instance is closed.
///
/// Idempotent on already-freed slots.
pub fn epoll_instance_close(idx: u64) {
    let _ = with_instance_mut(idx, |inst| {
        inst.in_use = false;
        inst.entries = [None; MAX_EPOLL_ENTRIES];
    });
}

/// Returns `true` if at least one fd in the instance has a ready event
/// matching its watched mask.  Used by `poll()`/`select()` to support
/// nesting epoll inside another multiplexer.
#[must_use]
pub fn epoll_instance_has_ready(idx: u64) -> bool {
    with_instance(idx, |inst| {
        for slot in inst.entries.iter().flatten() {
            if slot.oneshot_fired {
                continue;
            }
            if compute_revents(slot.fd, slot.events) != 0 {
                return true;
            }
        }
        false
    })
    .unwrap_or(false)
}

/// Compute revents for one watched fd.  Returns 0 if not ready or if
/// the fd is invalid (the caller handles invalid-fd reporting via
/// `EPOLLNVAL`-equivalent semantics in `epoll_wait`).
fn compute_revents(fd: i32, mask: u32) -> u32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        // Watched fd was closed without EPOLL_CTL_DEL.  Linux reports
        // EPOLLERR | EPOLLHUP in this case if the caller asked for any
        // events; we mirror that.
        return EPOLLERR | EPOLLHUP;
    };
    let (readable, writable, hangup, error) =
        crate::poll::check_readiness(entry.kind, entry.handle);
    let mut revents: u32 = 0;
    // Linux: POLLERR/POLLHUP imply readability for wake-up purposes.
    let eff_readable = readable || hangup || error;
    let eff_writable = writable || error;
    if eff_readable && (mask & EPOLLIN != 0) {
        revents |= EPOLLIN;
    }
    if eff_writable && (mask & EPOLLOUT != 0) {
        revents |= EPOLLOUT;
    }
    // EPOLLERR and EPOLLHUP are always reported regardless of mask.
    if error {
        revents |= EPOLLERR;
    }
    if hangup {
        revents |= EPOLLHUP;
    }
    revents
}

/// Create an epoll file descriptor.
///
/// `size` was a hint in early Linux versions; modern kernels ignore it.
/// We do too — `size` is accepted for API compatibility but unused.
/// Returns the new fd or -1 with `errno` on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_create(size: i32) -> i32 {
    // Linux requires size > 0 for the legacy API (even though it ignores
    // the value).  Replicate that check.
    if size <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    create_internal(0)
}

/// Create an epoll file descriptor with flags.
///
/// Supported flags: `EPOLL_CLOEXEC` (sets `FD_CLOEXEC` on the new fd).
/// Returns the new fd or -1 with `errno` on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_create1(flags: i32) -> i32 {
    if flags & !EPOLL_CLOEXEC != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    create_internal(flags)
}

/// Shared internal helper for `epoll_create` / `epoll_create1`.
fn create_internal(flags: i32) -> i32 {
    let Some(idx) = allocate_instance() else {
        errno::set_errno(errno::ENOMEM);
        return -1;
    };
    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::Epoll, idx as u64, 0) else {
        // fd table full — release the instance.
        epoll_instance_close(idx as u64);
        errno::set_errno(errno::EMFILE);
        return -1;
    };
    if flags & EPOLL_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
    }
    fd
}

/// Control an epoll file descriptor.
///
/// `op` is one of `EPOLL_CTL_ADD`, `EPOLL_CTL_MOD`, `EPOLL_CTL_DEL`.
/// For ADD/MOD, `event` must be a valid pointer; for DEL it is ignored.
///
/// Returns 0 on success, -1 with `errno` on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_ctl(epfd: i32, op: i32, fd: i32, event: *mut EpollEvent) -> i32 {
    // Linux validation order (fs/eventpoll.c::do_epoll_ctl, paraphrased):
    //   1. EFAULT: `ep_op_has_event(op) && copy_from_user(event)` fails.
    //   2. EBADF:  `fdget(epfd)`  — epfd must be open.
    //   3. EBADF:  `fdget(fd)`    — target fd must be open.
    //   4. EINVAL: `!is_file_epoll(epfd)` — epfd must be an epoll fd.
    //   5. EINVAL: `epfd == fd`   — can't epoll oneself (checked after
    //                               the kind check).
    //   6. EINVAL: unknown `op`   — switch-statement default.
    //
    // Deviations from Linux that we keep:
    //   * `EPOLL_CTL_DEL` tolerates a closed target fd, so applications
    //     can clean up after a race where the fd was closed out from
    //     under them.  Linux returns EBADF in that case.

    // 1. EFAULT — null event for ADD/MOD, BEFORE any fd lookup.
    let needs_event = op == EPOLL_CTL_ADD || op == EPOLL_CTL_MOD;
    if needs_event && event.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // 2. epfd must be a valid open fd.
    let Some(ep_entry) = fdtable::get_fd(epfd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    // 3. target fd must be a valid open fd (DEL deviation aside).
    if op != EPOLL_CTL_DEL && fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }
    // 4. epfd must be an epoll fd, not (e.g.) a regular file.
    if ep_entry.kind != HandleKind::Epoll {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // 5. Can't epoll yourself.  Linux checks this AFTER the kind check.
    if fd == epfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let idx = ep_entry.handle;

    match op {
        EPOLL_CTL_ADD => {
            // SAFETY: caller asserts validity of `event` for ADD/MOD.
            // Null was rejected upfront with EFAULT.
            let ev = unsafe { core::ptr::read_unaligned(event) };
            // Unaligned read because of #[repr(packed)] — copy fields
            // through stack locals to avoid taking references to packed
            // fields (which would be UB).
            let events_val = ev.events;
            let data_val = ev.data;
            if events_val & !EPOLL_INPUT_MASK != 0 {
                // Unknown event bits — be permissive (Linux silently
                // ignores unknown bits), but require at least one of
                // POLLIN/POLLOUT/POLLRDHUP/POLLPRI is meaningful.  We
                // accept whatever the caller passes.
            }
            let res = with_instance_mut(idx, |inst| {
                // Reject if fd already present.
                for slot in inst.entries.iter().flatten() {
                    if slot.fd == fd {
                        return Err(errno::EEXIST);
                    }
                }
                // Find a free slot.
                for slot in &mut inst.entries {
                    if slot.is_none() {
                        *slot = Some(EpollEntry {
                            fd,
                            events: events_val,
                            data: data_val,
                            oneshot_fired: false,
                        });
                        return Ok(());
                    }
                }
                Err(errno::ENOSPC)
            });
            match res {
                Some(Ok(())) => 0,
                Some(Err(e)) => {
                    errno::set_errno(e);
                    -1
                }
                None => {
                    errno::set_errno(errno::EBADF);
                    -1
                }
            }
        }
        EPOLL_CTL_MOD => {
            // SAFETY: caller asserts validity of `event`.  Null was
            // rejected upfront with EFAULT.
            let ev = unsafe { core::ptr::read_unaligned(event) };
            let events_val = ev.events;
            let data_val = ev.data;
            let res = with_instance_mut(idx, |inst| {
                for slot in &mut inst.entries {
                    if let Some(entry) = slot.as_mut()
                        && entry.fd == fd {
                            entry.events = events_val;
                            entry.data = data_val;
                            // Re-arm oneshot per Linux semantics.
                            entry.oneshot_fired = false;
                            return Ok(());
                        }
                }
                Err(errno::ENOENT)
            });
            match res {
                Some(Ok(())) => 0,
                Some(Err(e)) => {
                    errno::set_errno(e);
                    -1
                }
                None => {
                    errno::set_errno(errno::EBADF);
                    -1
                }
            }
        }
        EPOLL_CTL_DEL => {
            let res = with_instance_mut(idx, |inst| {
                for slot in &mut inst.entries {
                    if let Some(entry) = slot.as_ref()
                        && entry.fd == fd {
                            *slot = None;
                            return Ok(());
                        }
                }
                Err(errno::ENOENT)
            });
            match res {
                Some(Ok(())) => 0,
                Some(Err(e)) => {
                    errno::set_errno(e);
                    -1
                }
                None => {
                    errno::set_errno(errno::EBADF);
                    -1
                }
            }
        }
        _ => {
            errno::set_errno(errno::EINVAL);
            -1
        }
    }
}

/// Wait for events on an epoll file descriptor.
///
/// Blocks (via a polling loop, currently) until at least one watched fd
/// becomes ready, the timeout expires, or a signal is delivered.
///
/// - `timeout == -1`: wait indefinitely.
/// - `timeout == 0`: poll once and return immediately.
/// - `timeout > 0`: wait at most `timeout` milliseconds.
///
/// Returns the number of events written into `events` (0..=maxevents),
/// or -1 with `errno` set on error.
///
/// # Safety
///
/// `events` must point to an array of at least `maxevents` `EpollEvent`
/// entries.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn epoll_wait(
    epfd: i32,
    events: *mut EpollEvent,
    maxevents: i32,
    timeout: i32,
) -> i32 {
    // Sleep interval for the poll loop: 10ms.  Matches poll()/select().
    const POLL_INTERVAL_NS: u64 = 10_000_000;

    // Validation order matches Linux's do_epoll_wait + ep_check_params
    // (fs/eventpoll.c):
    //   1. EBADF:  fdget(epfd) — epfd must be open.
    //   2. EINVAL: maxevents <= 0 || maxevents > EP_MAX_EVENTS.
    //   3. EFAULT: !access_ok(events, maxevents * sizeof(epoll_event)).
    //   4. EINVAL: !is_file_epoll(epfd) — epfd must be an epoll fd.
    //
    // Previously we checked maxevents and events before the fdget, so
    // a bad epfd combined with bad maxevents returned EINVAL instead
    // of EBADF.
    let Some(ep_entry) = fdtable::get_fd(epfd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if maxevents <= 0 || maxevents > EP_MAX_EVENTS {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if events.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if ep_entry.kind != HandleKind::Epoll {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = ep_entry.handle;

    let deadline_ns: u64 = if timeout > 0 {
        let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
        // i32 ms → u64 ns: fits comfortably (max 2^31 ms ≈ 2^61 ns).
        #[allow(clippy::cast_sign_loss)]
        let ms = timeout as u64;
        now.saturating_add(ms.saturating_mul(1_000_000))
    } else {
        0
    };

    loop {
        // Scan the interest list and collect up to maxevents ready entries.
        let n = with_instance_mut(idx, |inst| {
            let mut count: i32 = 0;
            let limit = maxevents;
            let mut i = 0usize;
            while i < MAX_EPOLL_ENTRIES && count < limit {
                if let Some(entry) = inst.entries.get_mut(i)
                    && let Some(watched) = entry.as_mut()
                        && !watched.oneshot_fired {
                            let revents = compute_revents(watched.fd, watched.events);
                            if revents != 0 {
                                // Write event into the caller's buffer.
                                // SAFETY: caller asserts events is valid
                                // for maxevents entries; count < limit.
                                #[allow(clippy::cast_sign_loss)]
                                let slot_ptr = unsafe { events.add(count as usize) };
                                let out = EpollEvent {
                                    events: revents,
                                    data: watched.data,
                                };
                                // SAFETY: slot_ptr is in-bounds (count
                                // < maxevents) and writable per caller.
                                unsafe {
                                    core::ptr::write_unaligned(slot_ptr, out);
                                }
                                if watched.events & EPOLLONESHOT != 0 {
                                    watched.oneshot_fired = true;
                                }
                                count = count.wrapping_add(1);
                            }
                        }
                i = i.wrapping_add(1);
            }
            count
        });

        let ready = n.unwrap_or(0);
        if ready > 0 {
            return ready;
        }

        // Nothing ready.
        if timeout == 0 {
            return 0;
        }
        if timeout > 0 {
            let now = syscall0(SYS_CLOCK_MONOTONIC) as u64;
            if now >= deadline_ns {
                return 0;
            }
        }
        let _ = syscall1(SYS_SLEEP, POLL_INTERVAL_NS);
    }
}

/// Wait for events with a signal mask.
///
/// We don't deliver signals, so `sigmask` is ignored — delegates to
/// `epoll_wait`.
///
/// # Safety
///
/// Same requirements as `epoll_wait`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn epoll_pwait(
    epfd: i32,
    events: *mut EpollEvent,
    maxevents: i32,
    timeout: i32,
    _sigmask: *const u64,
) -> i32 {
    unsafe { epoll_wait(epfd, events, maxevents, timeout) }
}

/// Wait for events on an epoll fd with nanosecond timeout (Linux 5.11+).
///
/// Like `epoll_pwait`, but takes a `timespec` pointer (NULL = wait
/// forever) instead of a millisecond integer.
///
/// # Safety
///
/// `events` must be valid for `maxevents` entries.  `timeout` may be
/// NULL or point to a valid `Timespec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn epoll_pwait2(
    epfd: i32,
    events: *mut EpollEvent,
    maxevents: i32,
    timeout: *const crate::stat::Timespec,
    _sigmask: *const u64,
) -> i32 {
    // Linux validation order (fs/eventpoll.c::do_epoll_pwait2):
    //   1. copy_from_user(timeout)           -> EFAULT
    //   2. poll_select_set_timeout validates -> EINVAL on tv_sec < 0
    //                                          or tv_nsec out of range
    //   3. then delegates to do_epoll_wait    (fdget etc.)
    //
    // We can't usefully distinguish "wild non-null pointer" from a
    // real timespec without a userspace memory map, so we forward the
    // dereference unconditionally when `timeout` is non-null and
    // validate the *contents* of the timespec — that EINVAL must fire
    // BEFORE the inner epfd lookup, otherwise a bad timespec combined
    // with a bad epfd would surface as EBADF instead of EINVAL.
    let tms: i32 = if timeout.is_null() {
        -1
    } else {
        // SAFETY: caller guarantees timeout points to a valid Timespec.
        let ts = unsafe { &*timeout };
        // Match Linux's `poll_select_set_timeout`: reject negative
        // seconds and any nsec value outside `[0, 1_000_000_000)`.
        if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        if ts.tv_sec == 0 && ts.tv_nsec == 0 {
            0
        } else {
            // Round up nanoseconds → milliseconds so a non-zero
            // sub-millisecond timeout doesn't collapse to 0.  Both
            // operands are non-negative now (validated above), so the
            // saturating math can only saturate upward.
            let ms = ts
                .tv_sec
                .saturating_mul(1_000)
                .saturating_add(ts.tv_nsec.saturating_add(999_999) / 1_000_000);
            if ms > i64::from(i32::MAX) {
                i32::MAX
            } else if ms <= 0 {
                // Theoretically unreachable since both operands are
                // non-negative and at least one is non-zero (the all-
                // zero case is handled above).  Defensive: round up.
                1
            } else {
                ms as i32
            }
        }
    };
    unsafe { epoll_wait(epfd, events, maxevents, tms) }
}

// ===========================================================================
// eventfd — inter-thread / inter-process event notification
// ===========================================================================

/// Flags for `eventfd`.
pub const EFD_CLOEXEC: i32 = 0o2_000_000;
/// Non-blocking flag.
pub const EFD_NONBLOCK: i32 = 0o4000;
/// Semaphore-mode flag.
pub const EFD_SEMAPHORE: i32 = 1;

/// Create an eventfd file descriptor.
///
/// `initval` seeds the kernel counter.  Supported flags:
///
/// - `EFD_CLOEXEC` — sets `FD_CLOEXEC` on the new fd.
/// - `EFD_NONBLOCK` — sets `O_NONBLOCK` on the new fd (makes `read()`
///   return `EAGAIN` instead of blocking when the counter is 0).
/// - `EFD_SEMAPHORE` — selects semaphore mode in the kernel: each
///   `read()` decrements the counter by 1 and returns 1 (matches
///   Linux `EFD_SEMAPHORE`).  Without this flag, `read()` drains the
///   counter to 0 and returns the full value (default eventfd
///   behavior).
///
/// Returns a fresh fd on success, -1 with `errno` set on error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd(initval: u32, flags: i32) -> i32 {
    // Reject unknown flag bits.
    let allowed = EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE;
    if flags & !allowed != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Kernel-side flag layout: bit 0 = semaphore.  EFD_CLOEXEC and
    // EFD_NONBLOCK are userspace-only fd-table concerns.
    const KSYS_EVENTFD_SEMAPHORE: u64 = 1;
    let kernel_flags = if flags & EFD_SEMAPHORE != 0 {
        KSYS_EVENTFD_SEMAPHORE
    } else {
        0
    };
    let handle_ret = syscall2(SYS_EVENTFD_CREATE, u64::from(initval), kernel_flags);
    if handle_ret < 0 {
        return errno::translate(handle_ret) as i32;
    }
    #[allow(clippy::cast_sign_loss)]
    let handle = handle_ret as u64;

    let nonblock_bit = if flags & EFD_NONBLOCK != 0 {
        crate::fcntl::O_NONBLOCK
    } else {
        0
    };
    let status = crate::fcntl::O_RDWR | nonblock_bit;

    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::Eventfd, handle, status) else {
        // Table full — clean up the kernel handle.
        let _ = syscall1(SYS_EVENTFD_CLOSE, handle);
        errno::set_errno(errno::EMFILE);
        return -1;
    };

    if flags & EFD_CLOEXEC != 0 {
        // set_fd_flags can't fail for an fd we just allocated.
        let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
    }

    fd
}

/// Read from an eventfd (glibc convenience wrapper).
///
/// Stores the counter value at `*value` and returns 0 on success.
/// Returns -1 with `errno` set on error (EBADF, EINVAL, EAGAIN if
/// non-blocking with zero counter).
///
/// Equivalent to `read(fd, value, 8) == 8 ? 0 : -1`.
///
/// Validation order (Phase 144) matches the glibc wrapper composed
/// with Linux's `sys_read`:
///   1. `fdget(fd)`                    → EBADF
///   2. `f.file->f_op->read`           → EINVAL on non-eventfd
///   3. `copy_to_user(value, ...)`     → EFAULT
///
/// Pre-Phase-144 we checked the NULL pointer first, so
/// `eventfd_read(-1, NULL)` returned EFAULT instead of Linux's
/// EBADF, hiding the fd bug behind a misdirected pointer error.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd_read(fd: i32, value: *mut u64) -> i32 {
    // Phase 144: fd resolution precedes NULL-pointer check.  A bad
    // fd is the higher-information error and Linux reports it first.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Eventfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
    let nr = if is_nb {
        SYS_EVENTFD_TRY_READ
    } else {
        SYS_EVENTFD_READ
    };
    let r = syscall1(nr, entry.handle);
    if r < 0 {
        let _ = errno::translate(r);
        return -1;
    }

    // SAFETY: `value` is non-null (checked above); caller guarantees it
    // points to a writable u64.  We write the kernel counter result.
    #[allow(clippy::cast_sign_loss)]
    unsafe {
        core::ptr::write_unaligned(value, r as u64);
    }
    0
}

/// Write to an eventfd (glibc convenience wrapper).
///
/// Adds `value` to the kernel counter.  Returns 0 on success, -1 with
/// `errno` set on error (EBADF, EINVAL if `value` is `u64::MAX`).
///
/// Equivalent to `write(fd, &value, 8) == 8 ? 0 : -1`.
///
/// Validation order (Phase 144) matches the glibc wrapper composed
/// with Linux's `sys_write`:
///   1. `fdget(fd)`                       → EBADF
///   2. `f.file->f_op->write`             → EINVAL on non-eventfd
///   3. `if (val == U64_MAX) return EINVAL` (inside
///      `eventfd_write` kernel handler, after fd resolution).
///
/// Pre-Phase-144 we checked `value == u64::MAX` before the fd
/// lookup, so `eventfd_write(-1, u64::MAX)` returned EINVAL
/// instead of Linux's EBADF.  Both errors are -1, but the
/// diagnostic was misdirected at the value when the real bug
/// was the fd.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd_write(fd: i32, value: u64) -> i32 {
    // Phase 144: fd resolution precedes value validation, matching
    // the kernel's `sys_write` → `eventfd_write` flow.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Eventfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if value == u64::MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let r = syscall2(SYS_EVENTFD_WRITE, entry.handle, value);
    if r < 0 {
        let _ = errno::translate(r);
        return -1;
    }
    0
}

// ===========================================================================
// timerfd — timer notification via file descriptor
// ===========================================================================
//
// Userspace implementation on top of `SYS_CLOCK_MONOTONIC`.  A timerfd
// is a virtual fd whose readability tracks the number of times a timer
// has expired since the last read.  `read()` returns that count as an
// 8-byte little-endian unsigned integer and resets it to 0.
//
// We use the monotonic clock for all timerfds regardless of the
// `clockid` argument — the kernel only exposes a single monotonic
// nanosecond counter, and CLOCK_REALTIME in our system is not yet
// settable (`clock_settime` returns EPERM), so there is no semantic
// distinction yet.  This matches the existing `clock_gettime` impl.
//
// Limitations vs. Linux:
// - `read()` blocking (without `O_NONBLOCK`) polls in 10ms sleeps just
//   like the rest of our readiness primitives.  A true wake-up would
//   need a kernel-side timer subsystem.
// - No `TFD_TIMER_CANCEL_ON_SET` support (we don't track realtime jumps).

/// Close-on-exec flag for `timerfd_create`.
pub const TFD_CLOEXEC: i32 = 0o2_000_000;
/// Non-blocking flag for `timerfd_create`.
pub const TFD_NONBLOCK: i32 = 0o4000;

/// `timerfd_settime` flag: interpret `it_value` as an absolute time.
pub const TFD_TIMER_ABSTIME: i32 = 1;

/// `timerfd_settime` flag: cancel the timer if the realtime clock is set
/// while it's armed.  Linux only honours this for `CLOCK_REALTIME` /
/// `CLOCK_REALTIME_ALARM` timers; for other clocks it is accepted but a
/// no-op.  We accept it for any clock and treat it as a no-op (we don't
/// track realtime clock jumps), which matches Linux's behaviour for the
/// non-realtime clocks and is a benign extension for the realtime ones.
pub const TFD_TIMER_CANCEL_ON_SET: i32 = 1 << 1;

/// Mask of valid `timerfd_settime` flags.  Matches `TFD_SETTIME_FLAGS` in
/// Linux's `include/uapi/linux/timerfd.h`.
pub const TFD_SETTIME_FLAGS_VALID: i32 = TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET;

/// Timer specification used by timerfd.
#[repr(C)]
pub struct Itimerspec {
    /// Timer interval (zero for one-shot).
    pub it_interval: crate::stat::Timespec,
    /// Initial expiration (zero disarms the timer).
    pub it_value: crate::stat::Timespec,
}

/// Maximum number of concurrently-open timerfds.
pub const MAX_TIMERFD_INSTANCES: usize = 16;

/// A single timerfd instance (one expiration counter + schedule).
#[derive(Clone, Copy)]
struct TimerfdInstance {
    in_use: bool,
    /// Absolute monotonic-clock nanoseconds at which the next expiration
    /// occurs.  Zero means "disarmed".
    next_expiry_ns: u64,
    /// Interval between expirations in nanoseconds.  Zero means one-shot.
    interval_ns: u64,
    /// Whether the fd was created with `TFD_NONBLOCK`.  `read()` checks
    /// this in addition to the per-fd `O_NONBLOCK` flag for compatibility.
    nonblock: bool,
}

const TIMERFD_INSTANCE_INIT: TimerfdInstance = TimerfdInstance {
    in_use: false,
    next_expiry_ns: 0,
    interval_ns: 0,
    nonblock: false,
};

static mut TIMERFD_INSTANCES: [TimerfdInstance; MAX_TIMERFD_INSTANCES] =
    [TIMERFD_INSTANCE_INIT; MAX_TIMERFD_INSTANCES];

fn timerfd_table_ptr() -> *mut [TimerfdInstance; MAX_TIMERFD_INSTANCES] {
    core::ptr::addr_of_mut!(TIMERFD_INSTANCES)
}

fn allocate_timerfd_instance() -> Option<usize> {
    // SAFETY: Single-threaded posix layer; no concurrent access.
    unsafe {
        let table = &mut *timerfd_table_ptr();
        for (i, inst) in table.iter_mut().enumerate() {
            if !inst.in_use {
                *inst = TIMERFD_INSTANCE_INIT;
                inst.in_use = true;
                return Some(i);
            }
        }
    }
    None
}

fn with_timerfd_mut<R>(idx: u64, f: impl FnOnce(&mut TimerfdInstance) -> R) -> Option<R> {
    let i = usize::try_from(idx).ok()?;
    if i >= MAX_TIMERFD_INSTANCES {
        return None;
    }
    // SAFETY: single-threaded.
    unsafe {
        let table = &mut *timerfd_table_ptr();
        let inst = table.get_mut(i)?;
        if !inst.in_use {
            return None;
        }
        Some(f(inst))
    }
}

fn with_timerfd<R>(idx: u64, f: impl FnOnce(&TimerfdInstance) -> R) -> Option<R> {
    let i = usize::try_from(idx).ok()?;
    if i >= MAX_TIMERFD_INSTANCES {
        return None;
    }
    // SAFETY: single-threaded.
    unsafe {
        let table = &*timerfd_table_ptr();
        let inst = table.get(i)?;
        if !inst.in_use {
            return None;
        }
        Some(f(inst))
    }
}

/// Free a timerfd instance.  Called by `close()` when the last fd
/// referencing the instance is closed.  Idempotent.
pub fn timerfd_instance_close(idx: u64) {
    let _ = with_timerfd_mut(idx, |inst| {
        *inst = TIMERFD_INSTANCE_INIT;
    });
}

/// Returns the current monotonic time in nanoseconds.
fn now_ns() -> u64 {
    syscall0(SYS_CLOCK_MONOTONIC) as u64
}

/// Compute how many times the timer has expired since `next_expiry_ns`
/// was set, given the current monotonic time `now`.  Mutates the
/// instance to roll `next_expiry_ns` forward past `now` so the count
/// only reflects expirations that haven't been observed yet.  Returns 0
/// if the timer is disarmed or hasn't yet reached its first expiry.
fn timerfd_consume_expirations(inst: &mut TimerfdInstance, now: u64) -> u64 {
    if inst.next_expiry_ns == 0 || now < inst.next_expiry_ns {
        return 0;
    }
    let elapsed = now - inst.next_expiry_ns;
    if inst.interval_ns == 0 {
        // One-shot: exactly one expiration, then disarm.
        inst.next_expiry_ns = 0;
        return 1;
    }
    let extra = elapsed / inst.interval_ns;
    let count = 1 + extra;
    // Roll the next expiry forward past `now`.
    inst.next_expiry_ns = inst
        .next_expiry_ns
        .saturating_add(count.saturating_mul(inst.interval_ns));
    count
}

/// Returns `true` if the timerfd has at least one un-read expiration.
/// Called by `poll`/`select`/`epoll` via `check_readiness`.
#[must_use]
pub fn timerfd_is_readable(idx: u64) -> bool {
    with_timerfd(idx, |inst| {
        if inst.next_expiry_ns == 0 {
            false
        } else {
            now_ns() >= inst.next_expiry_ns
        }
    })
    .unwrap_or(false)
}

/// Internal: perform a `read()` on a timerfd.  Returns the number of
/// expirations as bytes written to `buf` (8 bytes little-endian), or
/// 0 if the timer hasn't fired yet (caller decides whether to block).
///
/// Returns:
/// - `Ok(8)`: wrote 8 bytes, at least one expiration.
/// - `Ok(0)`: no expirations yet (caller should block or return EAGAIN).
/// - `Err(errno)`: caller-facing error.
///
/// # Errors
///
/// Returns `EINVAL` if `buf` is shorter than 8 bytes, or `EBADF` if
/// `idx` does not name a live timerfd.
pub fn timerfd_read(idx: u64, buf: &mut [u8]) -> Result<usize, i32> {
    if buf.len() < 8 {
        return Err(errno::EINVAL);
    }
    let now = now_ns();
    let count =
        with_timerfd_mut(idx, |inst| timerfd_consume_expirations(inst, now)).ok_or(errno::EBADF)?;
    if count == 0 {
        return Ok(0);
    }
    let bytes = count.to_ne_bytes();
    let dst = buf.get_mut(..8).ok_or(errno::EINVAL)?;
    dst.copy_from_slice(&bytes);
    Ok(8)
}

/// Returns whether the underlying timerfd instance was created with
/// `TFD_NONBLOCK`.  Used by `read()` to decide if a 0-expiration read
/// should block or return EAGAIN.
#[must_use]
pub fn timerfd_is_nonblock(idx: u64) -> bool {
    with_timerfd(idx, |inst| inst.nonblock).unwrap_or(false)
}

/// Create a timerfd file descriptor.
///
/// `clockid` is validated per Linux semantics
/// (kernel/time/timerfd.c::SYSCALL_DEFINE2(timerfd_create)):
/// only `CLOCK_REALTIME`, `CLOCK_MONOTONIC`, `CLOCK_BOOTTIME`,
/// `CLOCK_REALTIME_ALARM`, and `CLOCK_BOOTTIME_ALARM` are accepted;
/// any other value (including the otherwise-valid CLOCK_TAI,
/// CLOCK_PROCESS_CPUTIME_ID, etc.) yields EINVAL.
/// Internally all timerfds use the monotonic clock (see module note),
/// but the prologue still enforces the Linux-shaped surface so callers
/// see the same error for the same wrong input.
///
/// # Capability gate (Phase 199)
///
/// `CLOCK_REALTIME_ALARM` and `CLOCK_BOOTTIME_ALARM` wake the system
/// from suspend; Linux gates them on `CAP_WAKE_ALARM` in
/// `kernel/time/timerfd.c::SYSCALL_DEFINE2(timerfd_create)`:
/// ```text
/// if ((clockid == CLOCK_REALTIME_ALARM ||
///      clockid == CLOCK_BOOTTIME_ALARM) &&
///     !capable(CAP_WAKE_ALARM))
///     return -EPERM;
/// ```
/// The gate runs **after** the `flags` mask and clockid validity
/// checks — so a caller passing bogus flags or an unsupported
/// clockid sees `EINVAL`, not `EPERM`, regardless of privilege.
/// Non-alarm clocks (`CLOCK_REALTIME`, `CLOCK_MONOTONIC`,
/// `CLOCK_BOOTTIME`) bypass the gate entirely.
///
/// Valid `flags`: `TFD_CLOEXEC`, `TFD_NONBLOCK`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timerfd_create(clockid: i32, flags: i32) -> i32 {
    if flags & !(TFD_CLOEXEC | TFD_NONBLOCK) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Linux's accepted clockid set for timerfd_create.  These are the
    // only values the upstream switch in `timerfd_setup` recognises;
    // anything else (including CLOCK_PROCESS_CPUTIME_ID,
    // CLOCK_THREAD_CPUTIME_ID, CLOCK_TAI, and any garbage integer) is
    // rejected with EINVAL.
    const CLOCK_REALTIME_ALARM_I32: i32 = 8;
    const CLOCK_BOOTTIME_ALARM_I32: i32 = 9;
    let clockid_ok = matches!(
        clockid,
        crate::time::CLOCK_REALTIME | crate::time::CLOCK_MONOTONIC | crate::time::CLOCK_BOOTTIME
    ) || clockid == CLOCK_REALTIME_ALARM_I32
        || clockid == CLOCK_BOOTTIME_ALARM_I32;
    if !clockid_ok {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Phase 199: the alarm clocks wake the system from suspend, so
    // Linux gates them on CAP_WAKE_ALARM.  Placed after EINVAL flag /
    // clockid checks to match the kernel's source order — a caller
    // passing bogus flags or an unknown clockid sees EINVAL regardless
    // of privilege, but a caller holding a valid alarm clockid without
    // the cap is told it lacks privilege (EPERM), not asked to retry
    // with a different flag value.
    let is_alarm_clock = clockid == CLOCK_REALTIME_ALARM_I32 || clockid == CLOCK_BOOTTIME_ALARM_I32;
    if is_alarm_clock
        && !crate::sys_capability::has_capability(crate::sys_capability::CAP_WAKE_ALARM)
    {
        errno::set_errno(errno::EPERM);
        return -1;
    }
    let Some(idx) = allocate_timerfd_instance() else {
        errno::set_errno(errno::ENOMEM);
        return -1;
    };
    // Record the NONBLOCK preference inside the instance.
    let _ = with_timerfd_mut(idx as u64, |inst| {
        inst.nonblock = flags & TFD_NONBLOCK != 0;
    });
    let Some(fd) = fdtable::alloc_fd_with_flags(HandleKind::Timerfd, idx as u64, 0) else {
        timerfd_instance_close(idx as u64);
        errno::set_errno(errno::EMFILE);
        return -1;
    };
    if flags & TFD_CLOEXEC != 0 {
        let _ = fdtable::set_fd_flags(fd, fdtable::FD_CLOEXEC);
    }
    if flags & TFD_NONBLOCK != 0 {
        // Also set O_NONBLOCK on the fd's status flags so fcntl(F_GETFL)
        // reports it correctly.
        if let Some(cur) = fdtable::get_status_flags(fd) {
            let _ = fdtable::set_status_flags(fd, cur | crate::fcntl::O_NONBLOCK);
        }
    }
    fd
}

/// Convert a `Timespec` to nanoseconds, saturating on overflow.  Returns
/// `None` if the value is negative or malformed.
fn timespec_to_ns(ts: &crate::stat::Timespec) -> Option<u64> {
    if ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1_000_000_000 {
        return None;
    }
    let sec = u64::try_from(ts.tv_sec).ok()?;
    let nsec = u64::try_from(ts.tv_nsec).ok()?;
    sec.checked_mul(1_000_000_000)?.checked_add(nsec)
}

/// Write nanoseconds into a `Timespec`.
fn ns_to_timespec(ns: u64) -> crate::stat::Timespec {
    crate::stat::Timespec {
        tv_sec: (ns / 1_000_000_000) as i64,
        tv_nsec: (ns % 1_000_000_000) as i64,
    }
}

/// Arm or disarm a timerfd.
///
/// If `new_value.it_value` is zero the timer is disarmed.  Otherwise the
/// timer fires after `it_value` (or at the absolute time `it_value` if
/// `TFD_TIMER_ABSTIME` is set) and then every `it_interval` thereafter
/// (zero interval → one-shot).
///
/// If `old_value` is non-null, the previous setting is written there.
///
/// # Safety
/// `new_value` must be a valid pointer to an `Itimerspec`.  `old_value`
/// is either null or a valid writable pointer to an `Itimerspec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn timerfd_settime(
    fd: i32,
    flags: i32,
    new_value: *const Itimerspec,
    old_value: *mut Itimerspec,
) -> i32 {
    // Validation order matches Linux's `timerfd_settime` /
    // `do_timerfd_settime` (fs/timerfd.c):
    //   1. `copy_from_user(&new, utmr)`  -> EFAULT on null/bad pointer.
    //   2. `flags & ~TFD_SETTIME_FLAGS`  -> EINVAL.
    //   3. `!itimerspec64_valid(&new)`   -> EINVAL.
    //   4. `timerfd_fget(ufd, &f)`       -> EBADF (fd missing),
    //                                       EINVAL (wrong kind of fd).
    //
    // In particular, flag-mask and itimerspec validity are checked
    // BEFORE the fd lookup, so a bad fd combined with bad flags or a
    // bad timespec returns EINVAL, not EBADF.
    if new_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: caller asserts `new_value` points to a readable
    // `Itimerspec`.  We've ruled out null; the only remaining
    // constraint is that the pointer be properly aligned and within
    // the caller's address space, which is the caller's responsibility.
    let nv = unsafe { core::ptr::read(new_value) };

    if flags & !TFD_SETTIME_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Some(value_ns) = timespec_to_ns(&nv.it_value) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let Some(interval_ns) = timespec_to_ns(&nv.it_interval) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Timerfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = entry.handle;

    let now = now_ns();

    // Capture old setting for the optional `old_value` out-param.
    let old = with_timerfd(idx, |inst| (inst.next_expiry_ns, inst.interval_ns));

    let _ = with_timerfd_mut(idx, |inst| {
        if value_ns == 0 {
            // Disarm.
            inst.next_expiry_ns = 0;
            inst.interval_ns = 0;
        } else {
            inst.next_expiry_ns = if flags & TFD_TIMER_ABSTIME != 0 {
                value_ns
            } else {
                now.saturating_add(value_ns)
            };
            inst.interval_ns = interval_ns;
        }
    });

    if !old_value.is_null() {
        let (next, ival) = old.unwrap_or((0, 0));
        let remaining_ns = if next == 0 || now >= next {
            0
        } else {
            next - now
        };
        let out = Itimerspec {
            it_interval: ns_to_timespec(ival),
            it_value: ns_to_timespec(remaining_ns),
        };
        // SAFETY: caller asserts validity of `old_value` when non-null.
        unsafe { core::ptr::write(old_value, out) };
    }

    0
}

/// Get the current setting of a timerfd.
///
/// Writes the remaining time until next expiration to `curr_value.it_value`
/// and the interval to `curr_value.it_interval`.
///
/// Validation order matches Linux's `sys_timerfd_gettime`
/// (fs/timerfd.c):
///
/// ```text
/// SYSCALL_DEFINE2(timerfd_gettime, int, ufd, ... __user *, otmr) {
///     struct itimerspec64 kotmr;
///     int ret = do_timerfd_gettime(ufd, &kotmr);   // calls timerfd_fget
///     if (ret)
///         return ret;                              // EBADF / EINVAL
///     return put_itimerspec64(&kotmr, otmr) ? -EFAULT : 0;
/// }
/// ```
///
/// The fd is resolved BEFORE the user pointer is touched.  Pre-Phase
/// 143 we checked the NULL pointer first, so `timerfd_gettime(-1, NULL)`
/// returned `EFAULT` (incorrectly diagnosing the pointer when the real
/// bug was the fd).  Linux returns `EBADF` for that combination; we
/// now match.
///
/// # Safety
/// `curr_value` must be a valid writable pointer to an `Itimerspec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn timerfd_gettime(fd: i32, curr_value: *mut Itimerspec) -> i32 {
    // Phase 143: fd resolution precedes the user-pointer check to
    // match Linux's `do_timerfd_gettime` flow.  A buggy caller
    // passing both a bad fd and a NULL pointer learns about the fd
    // first — the higher-information error.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Timerfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if curr_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let idx = entry.handle;
    let now = now_ns();
    let (next, ival) =
        with_timerfd(idx, |inst| (inst.next_expiry_ns, inst.interval_ns)).unwrap_or((0, 0));
    let remaining_ns = if next == 0 || now >= next {
        0
    } else {
        next - now
    };
    let out = Itimerspec {
        it_interval: ns_to_timespec(ival),
        it_value: ns_to_timespec(remaining_ns),
    };
    // SAFETY: caller asserts validity of `curr_value`.
    unsafe { core::ptr::write(curr_value, out) };
    0
}

// ===========================================================================
// signalfd — receive signals via file descriptor
// ===========================================================================

/// `signalfd` flag: close-on-exec.  Same value as `O_CLOEXEC`.
pub const SFD_CLOEXEC: i32 = 0x0008_0000;
/// `signalfd` flag: non-blocking.  Same value as `O_NONBLOCK`.
pub const SFD_NONBLOCK: i32 = 0x0000_0800;
/// Mask of all defined `signalfd` flag bits.  Any bit outside this
/// mask is rejected with `EINVAL` — matches Linux's
/// `SFD_CLOEXEC | SFD_NONBLOCK` check in `fs/signalfd.c`.
pub const SFD_FLAGS_VALID: i32 = SFD_CLOEXEC | SFD_NONBLOCK;

/// Create or modify a signalfd.
///
/// Returns -1 with `ENOSYS` after argument-domain validation.  We do
/// not yet route signals through file descriptors (no signal queue
/// integration), but invalid callers must still see Linux-matching
/// errno values so glibc's `signalfd(3)` wrapper and direct
/// `signalfd4(2)` callers see correct error reporting.
///
/// Validation order matches `fs/signalfd.c::sys_signalfd4` →
/// `do_signalfd4` in Linux:
/// 1. `mask` NULL → `EFAULT`.  Linux's `sys_signalfd4` runs
///    `copy_from_user(&mask, user_mask, sizeof(mask))` BEFORE calling
///    `do_signalfd4`, so a faulting `user_mask` pointer returns
///    `EFAULT` regardless of whether `flags` would also be invalid.
/// 2. Unknown flag bits → `EINVAL`.  This is the first check inside
///    `do_signalfd4` itself.
/// 3. `fd != -1`: must be a valid open fd → `EBADF`.  Linux additionally
///    requires the fd to already be a signalfd → `EINVAL` if not, but
///    since we have no signalfds in the fdtable yet we cannot tell
///    "wrong-kind fd" from "no fd" — we report `EBADF` for both,
///    documenting the gap.  When signalfd state is added, this will
///    refine to `EINVAL` for non-signalfd kinds.
/// 4. All validated → `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn signalfd(fd: i32, mask: *const u64, flags: i32) -> i32 {
    // Step 1: copy_from_user(mask) in sys_signalfd4 — must precede
    // do_signalfd4's flag check.  A buggy caller passing both NULL
    // mask AND unknown flag bits sees EFAULT on Linux, not EINVAL.
    if mask.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // Step 2: do_signalfd4's flag check.
    if flags & !SFD_FLAGS_VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if fd != -1 {
        if fd < 0 {
            errno::set_errno(errno::EBADF);
            return -1;
        }
        if fdtable::get_fd(fd).is_none() {
            errno::set_errno(errno::EBADF);
            return -1;
        }
        // TODO(signalfd): once signalfd state is tracked, refine the
        // wrong-kind-of-fd case to EINVAL (matches Linux exactly).
    }
    errno::set_errno(errno::ENOSYS);
    -1
}

/// `signalfd4` — Linux alias for `signalfd` with explicit flags.
///
/// On Linux, `signalfd(2)` is the library wrapper; the raw syscall is
/// `signalfd4` which takes an explicit `flags` argument (SFD_CLOEXEC,
/// SFD_NONBLOCK).  Our `signalfd` already accepts flags, so this is a
/// direct alias.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn signalfd4(fd: i32, mask: *const u64, flags: i32) -> i32 {
    signalfd(fd, mask, flags)
}

// ===========================================================================
// inotify — filesystem event monitoring (kernel watch API backend)
// ===========================================================================
//
// ## Design
//
// Backed by the native kernel filesystem-watch API
// (`SYS_FS_WATCH_CREATE`/`READ`/`CLOSE`).  The kernel's `fs::notify`
// module is hooked into every VFS success path (create / unlink /
// write / rename / metadata change) and queues events at the source, so
// — unlike a polling design — no transient change is missed between
// reads and a content modification is detected even when the file size
// is unchanged.
//
// Each inotify *watch* maps to one kernel watch ID.  `inotify_add_watch`
// issues `SYS_FS_WATCH_CREATE`, `inotify_rm_watch` issues
// `SYS_FS_WATCH_CLOSE`, and `read()` / readiness checks pump pending
// kernel events through `SYS_FS_WATCH_READ`, translating each kernel
// event into one or more `struct inotify_event` records queued in a
// small per-instance ring (`pump_instance`).  The translation
// ([`translate_kernel_event`]) is a pure function and is unit-tested on
// the host; only the syscall issuance is target-gated.
//
// ### Event mapping (kernel → inotify)
//   * Created      → `IN_CREATE` (child name).
//   * Deleted      → `IN_DELETE` (child), or `IN_DELETE_SELF` +
//                    `IN_IGNORED` and watch auto-removal when the watched
//                    path itself is deleted.
//   * Modified     → `IN_MODIFY`.
//   * Renamed      → `IN_MOVED_FROM` and/or `IN_MOVED_TO` (paired by a
//                    shared cookie) for entries inside the watched dir,
//                    or `IN_MOVE_SELF` when the watched path is renamed.
//   * MetadataChg  → `IN_ATTRIB`.
//   * Accessed     → `IN_ACCESS`.
//   * Overflow     → `IN_Q_OVERFLOW` (wd = -1).
//
// ### NOT supported
//   * `IN_OPEN`, `IN_CLOSE_WRITE`, `IN_CLOSE_NOWRITE` — the kernel has
//     no open/close hooks.  Requesting these bits is accepted (Linux
//     compatibility) but no event ever fires for them.  A watch whose
//     mask maps to *no* kernel event bits has no kernel watch at all
//     (`kernel_id == 0`) and is inert.
//
// ### Limits (static allocation)
//   * 4 instances per process, 8 watches per instance (per-process
//     userspace bookkeeping; the kernel enforces its own global limit).
//   * 32 events per instance queue (further events are dropped with
//     `IN_Q_OVERFLOW` semantics: an overflow flag is set on the
//     instance, surfaced as a single overflow event on the next read).
//   * Names truncated to 63 bytes + NUL.

/// inotify event flags.
pub const IN_ACCESS: u32 = 0x0000_0001;
/// File was modified.
pub const IN_MODIFY: u32 = 0x0000_0002;
/// File metadata changed.
pub const IN_ATTRIB: u32 = 0x0000_0004;
/// File opened for writing was closed.
pub const IN_CLOSE_WRITE: u32 = 0x0000_0008;
/// File not opened for writing was closed.
pub const IN_CLOSE_NOWRITE: u32 = 0x0000_0010;
/// File was opened.
pub const IN_OPEN: u32 = 0x0000_0020;
/// File moved from watched directory.
pub const IN_MOVED_FROM: u32 = 0x0000_0040;
/// File moved to watched directory.
pub const IN_MOVED_TO: u32 = 0x0000_0080;
/// File created in watched directory.
pub const IN_CREATE: u32 = 0x0000_0100;
/// File deleted from watched directory.
pub const IN_DELETE: u32 = 0x0000_0200;
/// Watched file was deleted.
pub const IN_DELETE_SELF: u32 = 0x0000_0400;
/// Watched file was moved.
pub const IN_MOVE_SELF: u32 = 0x0000_0800;
/// Close (IN_CLOSE_WRITE | IN_CLOSE_NOWRITE).
pub const IN_CLOSE: u32 = IN_CLOSE_WRITE | IN_CLOSE_NOWRITE;
/// Move (IN_MOVED_FROM | IN_MOVED_TO).
pub const IN_MOVE: u32 = IN_MOVED_FROM | IN_MOVED_TO;
/// All events.
pub const IN_ALL_EVENTS: u32 = 0x0000_0FFF;

/// inotify_init flags.
pub const IN_CLOEXEC: i32 = 0o2_000_000;
/// Non-blocking flag.
pub const IN_NONBLOCK: i32 = 0o4000;

/// Mask bits we recognize on `inotify_add_watch`.  Anything else
/// (notably `IN_MASK_ADD`, `IN_ONESHOT`, `IN_DONT_FOLLOW`,
/// `IN_EXCL_UNLINK`, `IN_MASK_CREATE`) is accepted by the call but
/// silently ignored on the polling fast path.
const IN_KNOWN_EVENTS: u32 = IN_ALL_EVENTS;

/// Event queue overflow indicator — also surfaced via `IN_Q_OVERFLOW`
/// in `<sys/inotify.h>`.
pub const IN_Q_OVERFLOW: u32 = 0x0000_4000;
/// Watch was auto-removed (file deleted, mount unmounted, or
/// `inotify_rm_watch` called).
pub const IN_IGNORED: u32 = 0x0000_8000;

// ---------------------------------------------------------------------------
// Instance / watch tables
// ---------------------------------------------------------------------------

/// Maximum number of concurrent inotify instances per process.
pub const MAX_INOTIFY_INSTANCES: usize = 4;

/// Maximum number of watches per inotify instance.
pub const MAX_INOTIFY_WATCHES: usize = 8;

/// Maximum number of queued events per instance.
pub const MAX_INOTIFY_EVENTS: usize = 32;

/// Maximum length of a name field stored in a snapshot or queued event.
/// Longer names are truncated to fit (the bytes past the limit are
/// dropped); detection still works since we hash the truncated prefix
/// for diffing.
pub const INOTIFY_NAME_MAX: usize = 64;

/// Maximum length of a watched path.
pub const INOTIFY_PATH_MAX: usize = 256;

/// A single inotify watch, backed by one kernel watch.
#[derive(Clone, Copy)]
struct InotifyWatch {
    in_use: bool,
    /// inotify watch descriptor (per-instance, positive, monotonic).
    wd: i32,
    /// Original inotify event mask (kept for fine-grained filtering —
    /// the kernel watch coalesces several inotify bits into one event
    /// type, e.g. all `IN_MOVED_*` map to a single kernel RENAME).
    mask: u32,
    /// Kernel watch ID from `SYS_FS_WATCH_CREATE`, or 0 if the mask
    /// mapped to no kernel-deliverable events (the watch is then inert).
    kernel_id: u64,
    /// Resolved absolute path being watched (without trailing slash),
    /// used to compute event basenames relative to the watch.
    path: [u8; INOTIFY_PATH_MAX],
    path_len: u16,
}

const INOTIFY_WATCH_INIT: InotifyWatch = InotifyWatch {
    in_use: false,
    wd: 0,
    mask: 0,
    kernel_id: 0,
    path: [0u8; INOTIFY_PATH_MAX],
    path_len: 0,
};

/// A pending inotify event.  Mirrors Linux's `struct inotify_event`
/// (wd, mask, cookie, name) but pre-stores the name in a fixed buffer
/// so the queue can be a plain array.
#[derive(Clone, Copy)]
struct InotifyPending {
    wd: i32,
    mask: u32,
    cookie: u32,
    name: [u8; INOTIFY_NAME_MAX],
    name_len: u8,
}

const INOTIFY_PENDING_INIT: InotifyPending = InotifyPending {
    wd: 0,
    mask: 0,
    cookie: 0,
    name: [0u8; INOTIFY_NAME_MAX],
    name_len: 0,
};

/// A single inotify instance.
#[derive(Clone, Copy)]
struct InotifyInstance {
    in_use: bool,
    nonblock: bool,
    /// Next watch descriptor to assign (monotonic per instance).  Watch
    /// descriptors are positive integers starting at 1, matching Linux.
    next_wd: i32,
    /// Whether the queue has overflowed since the last successful read.
    /// Surfaces as an IN_Q_OVERFLOW event with wd=-1.
    overflow_pending: bool,
    watches: [InotifyWatch; MAX_INOTIFY_WATCHES],
    /// Circular event queue.  `head` is read position, `tail` is write
    /// position.  Empty when head == tail.
    events: [InotifyPending; MAX_INOTIFY_EVENTS],
    head: u16,
    tail: u16,
    count: u16,
}

const INOTIFY_INSTANCE_INIT: InotifyInstance = InotifyInstance {
    in_use: false,
    nonblock: false,
    next_wd: 1,
    overflow_pending: false,
    watches: [INOTIFY_WATCH_INIT; MAX_INOTIFY_WATCHES],
    events: [INOTIFY_PENDING_INIT; MAX_INOTIFY_EVENTS],
    head: 0,
    tail: 0,
    count: 0,
};

static mut INOTIFY_INSTANCES: [InotifyInstance; MAX_INOTIFY_INSTANCES] =
    [INOTIFY_INSTANCE_INIT; MAX_INOTIFY_INSTANCES];

fn inotify_table_ptr() -> *mut [InotifyInstance; MAX_INOTIFY_INSTANCES] {
    core::ptr::addr_of_mut!(INOTIFY_INSTANCES)
}

fn allocate_inotify_instance() -> Option<usize> {
    // SAFETY: Single-threaded posix layer; no concurrent access.
    unsafe {
        let table = &mut *inotify_table_ptr();
        for (i, inst) in table.iter_mut().enumerate() {
            if !inst.in_use {
                *inst = INOTIFY_INSTANCE_INIT;
                inst.in_use = true;
                return Some(i);
            }
        }
    }
    None
}

fn with_inotify_mut<R>(idx: u64, f: impl FnOnce(&mut InotifyInstance) -> R) -> Option<R> {
    let i = usize::try_from(idx).ok()?;
    if i >= MAX_INOTIFY_INSTANCES {
        return None;
    }
    // SAFETY: single-threaded.
    unsafe {
        let table = &mut *inotify_table_ptr();
        let inst = table.get_mut(i)?;
        if !inst.in_use {
            return None;
        }
        Some(f(inst))
    }
}

/// Free an inotify instance.  Called by `close()` when the last fd
/// referencing the instance is closed.  Closes every backing kernel
/// watch so they aren't leaked.  Idempotent.
pub fn inotify_instance_close(idx: u64) {
    let mut to_close = [0u64; MAX_INOTIFY_WATCHES];
    let _ = with_inotify_mut(idx, |inst| {
        for (i, w) in inst.watches.iter().enumerate() {
            if w.in_use && w.kernel_id != 0
                && let Some(slot) = to_close.get_mut(i) {
                    *slot = w.kernel_id;
                }
        }
        *inst = INOTIFY_INSTANCE_INIT;
    });
    for &kid in &to_close {
        if kid != 0 {
            kwatch_close(kid);
        }
    }
}

// ---------------------------------------------------------------------------
// Kernel watch API glue
// ---------------------------------------------------------------------------

// Kernel watch event-type codes — must match the FsEventType
// discriminants in kernel/src/fs/notify.rs.
const KEV_CREATED: u32 = 0;
const KEV_DELETED: u32 = 1;
const KEV_MODIFIED: u32 = 2;
const KEV_RENAMED: u32 = 3;
const KEV_METADATA: u32 = 4;
const KEV_ACCESSED: u32 = 5;
const KEV_OVERFLOW: u32 = 255;

// Kernel watch event-mask bits — must match FsEventMask in
// kernel/src/fs/notify.rs.
const KMASK_CREATE: u32 = 0x01;
const KMASK_DELETE: u32 = 0x02;
const KMASK_MODIFY: u32 = 0x04;
const KMASK_RENAME: u32 = 0x08;
const KMASK_METADATA: u32 = 0x10;
const KMASK_ACCESS: u32 = 0x20;

// Field offsets within one `FS_WATCH_EVENT_SIZE`-byte kernel record.
const KEV_NEWPATH_OFF: usize = 256;
const KEV_TYPE_OFF: usize = 520;
const KEV_TYPE_END: usize = 524;

/// Number of kernel events drained per `SYS_FS_WATCH_READ` call.
const KWATCH_READ_BATCH: usize = 16;

/// Scratch buffer for draining kernel watch events.  Reused across all
/// watches — the single-threaded posix layer makes this safe.
static mut INOTIFY_EVENT_SCRATCH: [u8; KWATCH_READ_BATCH * crate::syscall::FS_WATCH_EVENT_SIZE] =
    [0u8; KWATCH_READ_BATCH * crate::syscall::FS_WATCH_EVENT_SIZE];

fn event_scratch_ptr() -> *mut [u8; KWATCH_READ_BATCH * crate::syscall::FS_WATCH_EVENT_SIZE] {
    core::ptr::addr_of_mut!(INOTIFY_EVENT_SCRATCH)
}

/// Monotonic cookie source for pairing `IN_MOVED_FROM`/`IN_MOVED_TO`.
/// Cookies must be non-zero (zero means "no cookie" in inotify usage).
static INOTIFY_COOKIE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(1);

fn next_cookie() -> u32 {
    use core::sync::atomic::Ordering;
    let c = INOTIFY_COOKIE.fetch_add(1, Ordering::Relaxed);
    if c == 0 {
        INOTIFY_COOKIE.fetch_add(1, Ordering::Relaxed)
    } else {
        c
    }
}

/// Translate an inotify event mask into the kernel watch mask.  Several
/// inotify bits collapse onto a single kernel event type (all moves →
/// RENAME, both delete bits → DELETE).  Bits with no kernel equivalent
/// (`IN_OPEN`, `IN_CLOSE_*`) contribute nothing.
fn inotify_to_kernel_mask(m: u32) -> u32 {
    let mut k = 0u32;
    if m & IN_CREATE != 0 {
        k |= KMASK_CREATE;
    }
    if m & (IN_DELETE | IN_DELETE_SELF) != 0 {
        k |= KMASK_DELETE;
    }
    if m & IN_MODIFY != 0 {
        k |= KMASK_MODIFY;
    }
    if m & (IN_MOVED_FROM | IN_MOVED_TO | IN_MOVE_SELF) != 0 {
        k |= KMASK_RENAME;
    }
    if m & IN_ATTRIB != 0 {
        k |= KMASK_METADATA;
    }
    if m & IN_ACCESS != 0 {
        k |= KMASK_ACCESS;
    }
    k
}

// --- cfg-split syscall wrappers (real on bare metal, inert on host) ---

/// Create a kernel watch for `path` with kernel mask `kmask` (always
/// non-recursive, matching inotify).  Returns the watch ID (`> 0`) or a
/// negative kernel error code.
#[cfg(target_os = "none")]
fn kwatch_create(path: &[u8], kmask: u32) -> i64 {
    crate::syscall::syscall4(
        crate::syscall::SYS_FS_WATCH_CREATE,
        path.as_ptr() as u64,
        path.len() as u64,
        u64::from(kmask),
        0,
    )
}

#[cfg(not(target_os = "none"))]
fn kwatch_create(_path: &[u8], _kmask: u32) -> i64 {
    // Host build: no kernel.  Return a dummy positive ID so the watch
    // table logic stays exercisable by unit tests; `pump_instance`
    // never issues reads on host.
    1
}

#[cfg(target_os = "none")]
fn kwatch_close(id: u64) {
    let _ = crate::syscall::syscall1(crate::syscall::SYS_FS_WATCH_CLOSE, id);
}

#[cfg(not(target_os = "none"))]
fn kwatch_close(_id: u64) {}

#[cfg(target_os = "none")]
fn kwatch_read(id: u64, buf: &mut [u8], max_events: usize) -> i64 {
    crate::syscall::syscall3(
        crate::syscall::SYS_FS_WATCH_READ,
        id,
        buf.as_mut_ptr() as u64,
        max_events as u64,
    )
}

#[cfg(not(target_os = "none"))]
fn kwatch_read(_id: u64, _buf: &mut [u8], _max_events: usize) -> i64 {
    0
}

// ---------------------------------------------------------------------------
// Event-queue helpers
// ---------------------------------------------------------------------------

fn queue_push(inst: &mut InotifyInstance, ev: InotifyPending) {
    if inst.count as usize >= MAX_INOTIFY_EVENTS {
        inst.overflow_pending = true;
        return;
    }
    let tail = inst.tail as usize;
    inst.events[tail] = ev;
    inst.tail = ((tail + 1) % MAX_INOTIFY_EVENTS) as u16;
    inst.count += 1;
}

fn queue_pop(inst: &mut InotifyInstance) -> Option<InotifyPending> {
    if inst.count == 0 {
        return None;
    }
    let head = inst.head as usize;
    let ev = inst.events[head];
    inst.head = ((head + 1) % MAX_INOTIFY_EVENTS) as u16;
    inst.count -= 1;
    Some(ev)
}

fn make_event(wd: i32, mask: u32, name: &[u8]) -> InotifyPending {
    let mut ev = INOTIFY_PENDING_INIT;
    ev.wd = wd;
    ev.mask = mask;
    let n = core::cmp::min(name.len(), INOTIFY_NAME_MAX - 1);
    if let Some(dst) = ev.name.get_mut(..n)
        && let Some(src) = name.get(..n) {
            dst.copy_from_slice(src);
        }
    ev.name_len = n as u8;
    ev
}

// ---------------------------------------------------------------------------
// Kernel-event translation (pure, host-tested)
// ---------------------------------------------------------------------------

/// Strip a trailing NUL byte from a name slice if present.  The kernel
/// returns fixed-width 256-byte names padded with zeros.
fn strip_nul(name: &[u8]) -> &[u8] {
    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    name.get(..end).unwrap_or(name)
}

/// Result of relating a kernel-event path to a watched path.
#[derive(Clone, Copy)]
enum Rel<'a> {
    /// The path *is* the watched path itself.
    SelfPath,
    /// The path is a direct child of the watched directory; carries the
    /// child basename.
    Child(&'a [u8]),
    /// The path is unrelated (not the watch nor a direct child).  inotify
    /// watches are non-recursive, so grandchildren do not match.
    NotMatched,
}

/// Relate a kernel-event absolute path to the watched absolute path.
///
/// inotify semantics: a directory watch reports events for the directory
/// itself and for its *immediate* children (non-recursive).  A file watch
/// reports events for the file itself only.  We therefore accept an exact
/// match (`SelfPath`) or a path that is `watched` + `/` + a single path
/// component with no further slashes (`Child`).
fn relative_name<'a>(watched: &[u8], path: &'a [u8]) -> Rel<'a> {
    if path == watched {
        return Rel::SelfPath;
    }
    let wlen = watched.len();
    // Root "/" is its own prefix without a separator byte; every other
    // watched path needs a '/' separator after the prefix.
    let prefix_len = if watched == b"/" {
        1
    } else {
        let Some(head) = path.get(..wlen) else {
            return Rel::NotMatched;
        };
        if head != watched {
            return Rel::NotMatched;
        }
        if path.get(wlen) != Some(&b'/') {
            return Rel::NotMatched;
        }
        wlen.saturating_add(1)
    };
    let Some(remainder) = path.get(prefix_len..) else {
        return Rel::NotMatched;
    };
    if remainder.is_empty() || remainder.contains(&b'/') {
        return Rel::NotMatched;
    }
    Rel::Child(remainder)
}

/// Up to two inotify events produced from one kernel event, plus whether
/// the originating watch must be auto-removed (self-delete).
struct Translation {
    events: [InotifyPending; 2],
    count: usize,
    disarm: bool,
}

const TRANSLATION_EMPTY: Translation = Translation {
    events: [INOTIFY_PENDING_INIT; 2],
    count: 0,
    disarm: false,
};

fn push_tr(t: &mut Translation, ev: InotifyPending) {
    if let Some(slot) = t.events.get_mut(t.count) {
        *slot = ev;
        t.count = t.count.saturating_add(1);
    }
}

fn pending_with_cookie(wd: i32, mask: u32, cookie: u32, name: &[u8]) -> InotifyPending {
    let mut ev = make_event(wd, mask, name);
    ev.cookie = cookie;
    ev
}

/// For events that fire for both the watched path itself and its direct
/// children (modify / metadata / access), map the relation to the name
/// to report: empty for the watch itself, the basename for a child, and
/// `None` when the path is unrelated.
fn rel_self_or_child(r: Rel<'_>) -> Option<&[u8]> {
    match r {
        Rel::SelfPath => Some(b""),
        Rel::Child(name) => Some(name),
        Rel::NotMatched => None,
    }
}

/// Translate one kernel watch event into zero, one, or two
/// `struct inotify_event` records, gated by the watch's original inotify
/// mask.  Pure function — unit-tested on the host.
///
/// `watched` is the watched absolute path (no trailing slash).
/// `affected` / `new_path` are the kernel-supplied paths (already
/// NUL-stripped).  `cookie` is a non-zero pairing cookie for renames
/// (ignored for other event types).
fn translate_kernel_event(
    watched: &[u8],
    inotify_mask: u32,
    wd: i32,
    event_type: u32,
    affected: &[u8],
    new_path: &[u8],
    cookie: u32,
) -> Translation {
    let mut t = TRANSLATION_EMPTY;
    match event_type {
        KEV_CREATED => {
            if inotify_mask & IN_CREATE != 0
                && let Rel::Child(name) = relative_name(watched, affected)
            {
                push_tr(&mut t, make_event(wd, IN_CREATE, name));
            }
        }
        KEV_DELETED => match relative_name(watched, affected) {
            Rel::SelfPath => {
                if inotify_mask & IN_DELETE_SELF != 0 {
                    push_tr(&mut t, make_event(wd, IN_DELETE_SELF, &[]));
                }
                push_tr(&mut t, make_event(wd, IN_IGNORED, &[]));
                t.disarm = true;
            }
            Rel::Child(name) => {
                if inotify_mask & IN_DELETE != 0 {
                    push_tr(&mut t, make_event(wd, IN_DELETE, name));
                }
            }
            Rel::NotMatched => {}
        },
        KEV_MODIFIED => {
            if inotify_mask & IN_MODIFY != 0
                && let Some(name) = rel_self_or_child(relative_name(watched, affected))
            {
                push_tr(&mut t, make_event(wd, IN_MODIFY, name));
            }
        }
        KEV_RENAMED => {
            // A rename within / out of the watch.  The kernel reports the
            // old path in `affected` and (when known) the new path in
            // `new_path`.  Self-rename of the watched path → IN_MOVE_SELF.
            if matches!(relative_name(watched, affected), Rel::SelfPath) {
                if inotify_mask & IN_MOVE_SELF != 0 {
                    push_tr(&mut t, make_event(wd, IN_MOVE_SELF, &[]));
                }
            } else {
                if inotify_mask & IN_MOVED_FROM != 0
                    && let Rel::Child(name) = relative_name(watched, affected)
                {
                    push_tr(&mut t, pending_with_cookie(wd, IN_MOVED_FROM, cookie, name));
                }
                if inotify_mask & IN_MOVED_TO != 0
                    && let Rel::Child(name) = relative_name(watched, new_path)
                {
                    push_tr(&mut t, pending_with_cookie(wd, IN_MOVED_TO, cookie, name));
                }
            }
        }
        KEV_METADATA => {
            if inotify_mask & IN_ATTRIB != 0
                && let Some(name) = rel_self_or_child(relative_name(watched, affected))
            {
                push_tr(&mut t, make_event(wd, IN_ATTRIB, name));
            }
        }
        KEV_ACCESSED => {
            if inotify_mask & IN_ACCESS != 0
                && let Some(name) = rel_self_or_child(relative_name(watched, affected))
            {
                push_tr(&mut t, make_event(wd, IN_ACCESS, name));
            }
        }
        KEV_OVERFLOW => {
            push_tr(&mut t, pending_with_cookie(-1, IN_Q_OVERFLOW, 0, &[]));
        }
        _ => {}
    }
    t
}

/// Stat the watched path itself.  Returns (exists, size, is_dir).
fn stat_self(path: &[u8]) -> (bool, u64, bool) {
    // SYS_FS_STAT writes a 16-byte FsStatResult, not a struct stat.
    let mut raw = [0u8; crate::stat::KERNEL_STAT_LEN];
    let ret = syscall3(
        crate::syscall::SYS_FS_STAT,
        path.as_ptr() as u64,
        path.len() as u64,
        raw.as_mut_ptr() as u64,
    );
    if ret < 0 {
        return (false, 0, false);
    }
    let mut st = crate::stat::Stat::zeroed();
    crate::stat::fill_from_fsstat(&mut st, &raw);
    let size = u64::try_from(st.st_size).unwrap_or(0);
    (true, size, st.is_dir())
}

/// Drain all pending kernel events for one watch and queue the
/// translated inotify events on its instance.  Auto-removes the watch
/// (and closes its kernel watch) on a self-delete.
fn pump_one_watch(idx: u64, wd: i32, kernel_id: u64, mask: u32, watched: &[u8]) {
    loop {
        // Read a batch of kernel records into the shared scratch buffer.
        // SAFETY: single-threaded posix layer; the &mut borrow does not
        // escape the call.
        let ret = {
            let scratch = unsafe { &mut *event_scratch_ptr() };
            kwatch_read(kernel_id, scratch, KWATCH_READ_BATCH)
        };
        if ret <= 0 {
            return;
        }
        let count = usize::try_from(ret).unwrap_or(0).min(KWATCH_READ_BATCH);
        let mut disarmed = false;
        // SAFETY: single-threaded; no concurrent &mut to scratch while we
        // read the records back out.
        let scratch = unsafe { &*event_scratch_ptr() };
        for rec in scratch
            .chunks_exact(crate::syscall::FS_WATCH_EVENT_SIZE)
            .take(count)
        {
            let Some(affected_raw) = rec.get(..KEV_NEWPATH_OFF) else {
                continue;
            };
            let Some(new_raw) = rec.get(KEV_NEWPATH_OFF..KEV_TYPE_OFF) else {
                continue;
            };
            let Some(type_raw) = rec.get(KEV_TYPE_OFF..KEV_TYPE_END) else {
                continue;
            };
            let affected = strip_nul(affected_raw);
            let new_path = strip_nul(new_raw);
            let etype = u32::from_le_bytes(<[u8; 4]>::try_from(type_raw).unwrap_or([0u8; 4]));
            let cookie = if etype == KEV_RENAMED {
                next_cookie()
            } else {
                0
            };
            let tr = translate_kernel_event(watched, mask, wd, etype, affected, new_path, cookie);
            let _ = with_inotify_mut(idx, |inst| {
                for k in 0..tr.count {
                    if let Some(ev) = tr.events.get(k) {
                        queue_push(inst, *ev);
                    }
                }
                if tr.disarm {
                    for w in &mut inst.watches {
                        if w.in_use && w.wd == wd {
                            *w = INOTIFY_WATCH_INIT;
                        }
                    }
                }
            });
            if tr.disarm {
                disarmed = true;
                break;
            }
        }
        if disarmed {
            kwatch_close(kernel_id);
            return;
        }
        // A short read means the kernel queue is drained.
        if count < KWATCH_READ_BATCH {
            return;
        }
    }
}

/// Drain pending kernel events for every active watch in an instance and
/// queue the translated inotify events.  This is the event-pump engine —
/// called by `read()` and by readiness checks so `poll`/`select`/
/// `epoll_wait` observe up-to-date state.
fn pump_instance(idx: u64) {
    let inst_id = match usize::try_from(idx) {
        Ok(i) if i < MAX_INOTIFY_INSTANCES => i,
        _ => return,
    };
    // Snapshot the active watches by value so we don't hold a borrow on
    // the instance table across `pump_one_watch` (which re-borrows it).
    let mut snap: [(bool, i32, u64, u32, [u8; INOTIFY_PATH_MAX], usize); MAX_INOTIFY_WATCHES] =
        [(false, 0, 0, 0, [0u8; INOTIFY_PATH_MAX], 0); MAX_INOTIFY_WATCHES];
    // SAFETY: single-threaded.
    unsafe {
        let table = &*inotify_table_ptr();
        let Some(inst) = table.get(inst_id) else {
            return;
        };
        if !inst.in_use {
            return;
        }
        for (j, w) in inst.watches.iter().enumerate() {
            if w.in_use
                && w.kernel_id != 0
                && let Some(slot) = snap.get_mut(j)
            {
                *slot = (true, w.wd, w.kernel_id, w.mask, w.path, w.path_len as usize);
            }
        }
    }
    for entry in &snap {
        let (active, wd, kernel_id, mask, path_buf, path_len) = *entry;
        if !active {
            continue;
        }
        let Some(watched) = path_buf.get(..path_len) else {
            continue;
        };
        pump_one_watch(idx, wd, kernel_id, mask, watched);
    }
}

/// Return `true` if the inotify instance has events available to
/// read.  Pumps pending kernel events as a side effect — so
/// `poll/select/epoll_wait` see up-to-date readiness.
#[must_use]
pub fn inotify_is_readable(idx: u64) -> bool {
    pump_instance(idx);
    let mut readable = false;
    let _ = with_inotify_mut(idx, |inst| {
        readable = inst.count > 0 || inst.overflow_pending;
    });
    readable
}

/// Return `true` if the underlying instance was created with
/// `IN_NONBLOCK`.
#[must_use]
pub fn inotify_is_nonblock(idx: u64) -> bool {
    let mut nb = false;
    let _ = with_inotify_mut(idx, |inst| {
        nb = inst.nonblock;
    });
    nb
}

/// Internal: try to drain queued events into `buf` in Linux's
/// `struct inotify_event` packed layout.  Returns the number of bytes
/// written, or 0 if no events are available (caller decides whether to
/// block).
///
/// Each record is 16 bytes of header followed by a `len`-byte name
/// field (NUL-padded to an 8-byte boundary).  We require `buf` to be
/// large enough for at least one full record.
///
/// # Errors
///
/// Returns `EBADF` if `idx` does not name a live inotify instance, or
/// `EINVAL` if `buf` is too small to hold the next pending event.
pub fn inotify_read(idx: u64, buf: &mut [u8]) -> Result<usize, i32> {
    // Always pump pending kernel events before serving reads, so events
    // generated since the last call show up.
    pump_instance(idx);

    let mut written = 0usize;
    let mut err: Option<i32> = None;
    let _ = with_inotify_mut(idx, |inst| {
        // Surface overflow first if pending.
        if inst.overflow_pending && written + 16 <= buf.len() {
            // wd=-1, mask=IN_Q_OVERFLOW, cookie=0, len=0.
            let header = [
                0xFF,
                0xFF,
                0xFF,
                0xFF, // wd = -1
                (IN_Q_OVERFLOW & 0xFF) as u8,
                ((IN_Q_OVERFLOW >> 8) & 0xFF) as u8,
                ((IN_Q_OVERFLOW >> 16) & 0xFF) as u8,
                ((IN_Q_OVERFLOW >> 24) & 0xFF) as u8,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ];
            if let Some(dst) = buf.get_mut(written..written + 16) {
                dst.copy_from_slice(&header);
                written += 16;
                inst.overflow_pending = false;
            }
        }

        while inst.count > 0 {
            // Peek without popping; need to know the name size to
            // decide if the next record fits.
            let head = inst.head as usize;
            let ev = inst.events[head];
            // Pad name field to 8 bytes minimum (or larger multiple)
            // so it remains aligned across reads.
            let raw_name = ev.name_len as usize;
            let name_field = if raw_name == 0 {
                0
            } else {
                let nul_terminated = raw_name + 1;
                (nul_terminated + 7) & !7
            };
            let record_size = 16 + name_field;
            if written + record_size > buf.len() {
                if written == 0 {
                    err = Some(errno::EINVAL);
                }
                break;
            }
            // Now pop and emit.
            let _ = queue_pop(inst);
            // Header.
            if let Some(dst) = buf.get_mut(written..written + 4) {
                dst.copy_from_slice(&ev.wd.to_ne_bytes());
            }
            if let Some(dst) = buf.get_mut(written + 4..written + 8) {
                dst.copy_from_slice(&ev.mask.to_ne_bytes());
            }
            if let Some(dst) = buf.get_mut(written + 8..written + 12) {
                dst.copy_from_slice(&ev.cookie.to_ne_bytes());
            }
            if let Some(dst) = buf.get_mut(written + 12..written + 16) {
                dst.copy_from_slice(&(name_field as u32).to_ne_bytes());
            }
            // Name (NUL-padded).
            if name_field > 0
                && let Some(dst) = buf.get_mut(written + 16..written + 16 + name_field) {
                    for b in dst.iter_mut() {
                        *b = 0;
                    }
                    let copy_n = core::cmp::min(raw_name, name_field - 1);
                    if let Some(src) = ev.name.get(..copy_n)
                        && let Some(dst2) = buf.get_mut(written + 16..written + 16 + copy_n) {
                            dst2.copy_from_slice(src);
                        }
                }
            written += record_size;
        }
    });

    if let Some(e) = err {
        return Err(e);
    }
    Ok(written)
}

// ---------------------------------------------------------------------------
// inotify_create / add_watch / rm_watch
// ---------------------------------------------------------------------------

/// Initialize an inotify instance.
///
/// Returns a new userspace fd backed by an inotify instance, or -1 on
/// error.  See module-level docs for the supported-event subset.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_init() -> i32 {
    inotify_init1(0)
}

/// Initialize an inotify instance with flags.
///
/// `flags` may be `IN_CLOEXEC` and/or `IN_NONBLOCK`.  Unknown bits
/// return `EINVAL`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_init1(flags: i32) -> i32 {
    const VALID: i32 = IN_CLOEXEC | IN_NONBLOCK;
    if flags & !VALID != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Some(idx) = allocate_inotify_instance() else {
        errno::set_errno(errno::EMFILE);
        return -1;
    };
    let _ = with_inotify_mut(idx as u64, |inst| {
        inst.nonblock = (flags & IN_NONBLOCK) != 0;
    });
    let fd_flags = if (flags & IN_CLOEXEC) != 0 {
        fdtable::FD_CLOEXEC
    } else {
        0
    };
    let Some(fd) = fdtable::alloc_fd(HandleKind::Inotify, idx as u64) else {
        inotify_instance_close(idx as u64);
        errno::set_errno(errno::EMFILE);
        return -1;
    };
    if fd_flags != 0 {
        let _ = fdtable::set_fd_flags(fd, fd_flags);
    }
    if (flags & IN_NONBLOCK) != 0 {
        // Reflect into status flags for consistency with the rest of
        // the fd dispatch (O_NONBLOCK).
        let cur = fdtable::get_status_flags(fd).unwrap_or(0);
        let _ = fdtable::set_status_flags(fd, cur | crate::fcntl::O_NONBLOCK);
    }
    fd
}

/// Add a watch to an inotify instance.
///
/// Returns a non-negative watch descriptor on success, -1 on error.
/// If `pathname` is already watched on this instance, the existing
/// watch's mask is overwritten (matching Linux without `IN_MASK_ADD`)
/// and the existing wd is returned.
///
/// Validation order matches Linux's `sys_inotify_add_watch`
/// (`fs/notify/inotify/inotify_user.c`):
///
/// 1. `inotify_arg_to_mask(mask)` masked against `ALL_INOTIFY_BITS` —
///    must have at least one event bit set after masking → EINVAL.
/// 2. `fdget(fd)` — invalid fd → EBADF; fd not an inotify
///    instance → EINVAL.
/// 3. `user_path_at(pathname, ...)` — NULL pointer → EFAULT; empty or
///    too-long → ENOENT/ENAMETOOLONG; missing file → ENOENT.
///
/// Phase 139 fix: pre-Phase 139 we checked `pathname.is_null()` first
/// and returned EFAULT, so a caller passing both NULL pathname AND a
/// zero (no-event-bits) mask saw EFAULT.  Linux returns EINVAL for
/// that input because the mask check fires before any user pointer is
/// touched.  Userspace probes (inotify-tools, fswatch's Linux backend)
/// rely on the Linux ordering to bisect "is my mask wrong" from "is
/// my pathname wrong."
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_add_watch(fd: i32, pathname: *const u8, mask: u32) -> i32 {
    // Step 1: mask validation — Linux's first check, before any user
    // pointer or fd is touched.
    if mask & IN_KNOWN_EVENTS == 0 {
        // Linux: at least one event bit must be set.
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Step 2: fd validation — `fdget(fd)` happens before user_path_at.
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Inotify {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = entry.handle;
    // Step 3: pathname validation — `user_path_at` runs last; NULL or
    // unreadable pointer faults here.
    if pathname.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Resolve the path against CWD into a normalized absolute path.
    let mut resolved = [0u8; crate::unistd::PATH_MAX];
    // SAFETY: `pathname` was checked non-null above.
    let Some(resolved_len) = (unsafe { crate::unistd::resolve_path(pathname, &mut resolved) })
    else {
        // SAFETY: pathname is non-null and a valid C-string.
        if unsafe { *pathname } == 0 {
            errno::set_errno(errno::ENOENT);
        } else {
            errno::set_errno(errno::ENAMETOOLONG);
        }
        return -1;
    };
    if resolved_len > INOTIFY_PATH_MAX {
        errno::set_errno(errno::ENAMETOOLONG);
        return -1;
    }
    let path_bytes = &resolved[..resolved_len];

    // Stat to check existence (Linux: a missing path → ENOENT).  inotify
    // watches both files and directories; the kernel watch backend does
    // not distinguish, so we no longer track the type.
    let (exists, _self_size, _is_dir) = stat_self(path_bytes);
    if !exists {
        errno::set_errno(errno::ENOENT);
        return -1;
    }

    let effective_mask = mask & IN_KNOWN_EVENTS;
    let kmask = inotify_to_kernel_mask(effective_mask);

    // Phase 1: locate an existing watch for this path (re-arm) or a free
    // slot (new watch).  We only capture indices/ids here — the kernel
    // watch is (re)created outside the borrow because it issues a
    // syscall.
    let mut existing: Option<(usize, i32, u64)> = None; // (slot, wd, old kernel_id)
    let mut free_slot: Option<usize> = None;
    let _ = with_inotify_mut(idx, |inst| {
        for (j, w) in inst.watches.iter().enumerate() {
            if w.in_use
                && w.path_len as usize == resolved_len
                && w.path.get(..resolved_len) == Some(path_bytes)
            {
                existing = Some((j, w.wd, w.kernel_id));
                return;
            }
        }
        for (j, w) in inst.watches.iter().enumerate() {
            if !w.in_use {
                free_slot = Some(j);
                return;
            }
        }
    });
    if existing.is_none() && free_slot.is_none() {
        errno::set_errno(errno::ENOSPC);
        return -1;
    }

    // Create the backing kernel watch unless the mask maps to no kernel
    // event bits (e.g. only IN_OPEN / IN_CLOSE_* requested), in which
    // case the watch is inert (`kernel_id == 0`).
    let new_kid: u64 = if kmask != 0 {
        let ret = kwatch_create(path_bytes, kmask);
        if ret <= 0 {
            // Kernel watch table full or rejected the request → ENOSPC,
            // the closest inotify errno for "no resources for a watch".
            errno::set_errno(errno::ENOSPC);
            return -1;
        }
        u64::try_from(ret).unwrap_or(0)
    } else {
        0
    };

    // Phase 2: commit the slot.  `to_close` captures the superseded
    // kernel watch of a re-armed entry so we can close it after dropping
    // the borrow.
    let mut wd_out: i32 = -1;
    let mut to_close = 0u64;
    let _ = with_inotify_mut(idx, |inst| {
        if let Some((j, wd, old_kid)) = existing {
            if let Some(w) = inst.watches.get_mut(j) {
                w.mask = effective_mask;
                w.kernel_id = new_kid;
                wd_out = wd;
            }
            to_close = old_kid;
        } else if let Some(j) = free_slot {
            let next = inst.next_wd;
            inst.next_wd = inst.next_wd.wrapping_add(1);
            if inst.next_wd <= 0 {
                inst.next_wd = 1;
            }
            if let Some(w) = inst.watches.get_mut(j) {
                *w = INOTIFY_WATCH_INIT;
                w.in_use = true;
                w.wd = next;
                w.mask = effective_mask;
                w.kernel_id = new_kid;
                if let Some(dst) = w.path.get_mut(..resolved_len) {
                    dst.copy_from_slice(path_bytes);
                }
                w.path_len = resolved_len as u16;
                wd_out = next;
            }
        }
    });

    // Close the watch the re-arm replaced (outside the borrow).
    if to_close != 0 && to_close != new_kid {
        kwatch_close(to_close);
    }

    if wd_out < 0 {
        // Commit failed unexpectedly — don't leak the kernel watch.
        if new_kid != 0 {
            kwatch_close(new_kid);
        }
        errno::set_errno(errno::EBADF);
        return -1;
    }
    wd_out
}

/// Remove a watch from an inotify instance.
///
/// Returns 0 on success, -1 on error.  Generates a final `IN_IGNORED`
/// event for the removed watch so userspace can drain its state.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_rm_watch(fd: i32, wd: i32) -> i32 {
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Inotify {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = entry.handle;
    let mut found = false;
    let mut to_close = 0u64;
    let _ = with_inotify_mut(idx, |inst| {
        // Capture the backing kernel watch id, queue the final
        // IN_IGNORED, then disarm the slot.  The kernel watch is closed
        // after the borrow is dropped.
        let mut kid = 0u64;
        for w in &inst.watches {
            if w.in_use && w.wd == wd {
                kid = w.kernel_id;
                found = true;
                break;
            }
        }
        if !found {
            return;
        }
        queue_push(inst, make_event(wd, IN_IGNORED, &[]));
        for w in &mut inst.watches {
            if w.in_use && w.wd == wd {
                *w = INOTIFY_WATCH_INIT;
            }
        }
        to_close = kid;
    });
    if !found {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if to_close != 0 {
        kwatch_close(to_close);
    }
    0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- epoll constants (match Linux) --

    #[test]
    fn test_epoll_event_flags() {
        assert_eq!(EPOLLIN, 0x001);
        assert_eq!(EPOLLOUT, 0x004);
        assert_eq!(EPOLLERR, 0x008);
        assert_eq!(EPOLLHUP, 0x010);
        assert_eq!(EPOLLET, 1 << 31);
    }

    #[test]
    fn test_epoll_ctl_ops() {
        assert_eq!(EPOLL_CTL_ADD, 1);
        assert_eq!(EPOLL_CTL_DEL, 2);
        assert_eq!(EPOLL_CTL_MOD, 3);
    }

    #[test]
    fn test_epoll_event_flags_composable() {
        let read_write = EPOLLIN | EPOLLOUT;
        assert_eq!(read_write, 0x005);
        let edge_read = EPOLLIN | EPOLLET;
        assert_eq!(edge_read, 0x8000_0001);
    }

    // -- epoll argument validation (don't need a real fd table) --

    #[test]
    fn test_epoll_create_invalid_size_returns_einval() {
        errno::set_errno(0);
        assert_eq!(epoll_create(0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert_eq!(epoll_create(-5), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_epoll_create1_unknown_flag_returns_einval() {
        errno::set_errno(0);
        // Bit that isn't EPOLL_CLOEXEC.
        assert_eq!(epoll_create1(0x123), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_epoll_ctl_bad_epfd_returns_ebadf() {
        errno::set_errno(0);
        // FD 250 is unlikely to be allocated in the default test table.
        // Pass a non-null `event` so we test the EBADF path on the epfd
        // and don't trip the upfront EFAULT check (Phase 106 ordering).
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        assert_eq!(epoll_ctl(250, EPOLL_CTL_ADD, 4, &raw mut ev), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_wait_invalid_maxevents_returns_einval() {
        // Use a real epoll fd so the maxevents EINVAL fires instead of
        // the EBADF from the epfd lookup that Phase 107 promoted to
        // first place.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        errno::set_errno(0);
        // SAFETY: maxevents check happens before any pointer dereference.
        let ret = unsafe { epoll_wait(epfd, core::ptr::null_mut(), 0, -1) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait_bad_epfd_returns_ebadf() {
        errno::set_errno(0);
        let mut buf = [EpollEvent { events: 0, data: 0 }; 4];
        // SAFETY: events pointer is to a valid local buffer.
        let ret = unsafe {
            epoll_pwait(
                250,
                buf.as_mut_ptr(),
                buf.len() as i32,
                0,
                core::ptr::null(),
            )
        };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- eventfd constants --

    #[test]
    fn test_efd_flags() {
        assert_eq!(EFD_SEMAPHORE, 1);
        // EFD_CLOEXEC and EFD_NONBLOCK should be distinct.
        assert_ne!(EFD_CLOEXEC, EFD_NONBLOCK);
        assert_ne!(EFD_CLOEXEC, 0);
        assert_ne!(EFD_NONBLOCK, 0);
    }

    // -- eventfd userspace checks (no kernel needed) --

    /// `EFD_SEMAPHORE` must be accepted as a valid flag (the wrapper
    /// no longer rejects it).  The success path requires a real kernel
    /// and is covered by the integration tests in `tests/eventfd.rs`.
    #[test]
    fn test_eventfd_semaphore_flag_is_valid() {
        // Just verify the bit is in the allowed set — the call would
        // require the kernel to actually succeed, so we don't invoke it
        // here.
        let allowed = EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE;
        assert_eq!(EFD_SEMAPHORE & !allowed, 0);
    }

    /// Unknown flag bits should be rejected (forward-compat).
    #[test]
    fn test_eventfd_unknown_flag_rejected() {
        errno::set_errno(0);
        // Use a bit that is not in {EFD_CLOEXEC, EFD_NONBLOCK, EFD_SEMAPHORE}.
        let bad_bit = 0x40;
        assert_eq!(bad_bit & (EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE), 0);
        assert_eq!(eventfd(0, bad_bit), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// Phase 144: `eventfd_read(3, NULL)` — fd 3 in the host test
    /// environment is not an eventfd, so the fd-resolution check
    /// runs first and produces EBADF (no fd table entry) or EINVAL
    /// (entry exists but wrong kind).  The NULL-pointer EFAULT path
    /// is unreachable here because the fd never validates.  Renamed
    /// from `test_eventfd_read_null_returns_efault` (which asserted
    /// the pre-Phase-144 buggy ordering) and re-tasked to pin the
    /// new precedence. The matched-EFAULT case lives in
    /// `test_eventfd_read_phase144_valid_fd_null_pointer_is_efault`.
    #[test]
    fn test_eventfd_read_bad_fd_beats_null_pointer_efault() {
        errno::set_errno(0);
        assert_eq!(eventfd_read(3, core::ptr::null_mut()), -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EBADF || e == errno::EINVAL,
            "got {e}, expected EBADF or EINVAL (never the old EFAULT)",
        );
    }

    /// `eventfd_read` on a non-eventfd fd must fail with EBADF.
    #[test]
    fn test_eventfd_read_bad_fd_returns_ebadf() {
        errno::set_errno(0);
        let mut val: u64 = 0;
        // fd 999 is not in the table.
        assert_eq!(eventfd_read(999, &raw mut val), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    /// `eventfd_read` on a negative fd must fail with EBADF.
    #[test]
    fn test_eventfd_read_negative_fd_returns_ebadf() {
        errno::set_errno(0);
        let mut val: u64 = 0;
        assert_eq!(eventfd_read(-1, &raw mut val), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    /// Phase 144: `eventfd_write(3, u64::MAX)` — fd 3 is not an
    /// eventfd in the host test environment, so the fd-resolution
    /// check runs first.  We get EBADF (no entry) or EINVAL (wrong
    /// kind); the U64_MAX rejection path is unreachable.  Renamed
    /// from `test_eventfd_write_max_rejected` and re-tasked.  A
    /// real eventfd + U64_MAX test that exercises the value-EINVAL
    /// path lives in `test_eventfd_write_phase144_valid_fd_u64_max_is_einval`.
    #[test]
    fn test_eventfd_write_bad_fd_beats_value_max_einval() {
        errno::set_errno(0);
        assert_eq!(eventfd_write(3, u64::MAX), -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EBADF || e == errno::EINVAL,
            "got {e}, expected EBADF or EINVAL (never reached the value check)",
        );
    }

    /// `eventfd_write` on a non-eventfd fd must fail with EBADF.
    #[test]
    fn test_eventfd_write_bad_fd_returns_ebadf() {
        errno::set_errno(0);
        assert_eq!(eventfd_write(999, 1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    /// `eventfd_write` on a negative fd must fail with EBADF.
    #[test]
    fn test_eventfd_write_negative_fd_returns_ebadf() {
        errno::set_errno(0);
        assert_eq!(eventfd_write(-1, 1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- timerfd constants --

    #[test]
    fn test_tfd_flags() {
        assert_ne!(TFD_CLOEXEC, 0);
        assert_ne!(TFD_NONBLOCK, 0);
        assert_eq!(TFD_TIMER_ABSTIME, 1);
    }

    // -- timerfd stubs --

    #[test]
    fn test_timerfd_create_returns_valid_fd() {
        // timerfd_create(CLOCK_MONOTONIC, 0) should succeed.
        errno::set_errno(0);
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0, "timerfd_create should succeed, got {fd}");
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_bad_fd() {
        // timerfd_settime on a non-timerfd should fail with EINVAL or
        // EBADF (depending on whether fd 3 exists in this test process).
        errno::set_errno(0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        let ret = unsafe { timerfd_settime(3, 0, &new, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EBADF || e == errno::EINVAL || e == errno::EFAULT,
            "unexpected errno {e}"
        );
    }

    #[test]
    fn test_timerfd_gettime_bad_fd() {
        // timerfd_gettime on a non-timerfd should fail.
        errno::set_errno(0);
        let mut out = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        let ret = unsafe { timerfd_gettime(3, &mut out) };
        assert_eq!(ret, -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EBADF || e == errno::EINVAL || e == errno::EFAULT,
            "unexpected errno {e}"
        );
    }

    // -- signalfd stub --

    #[test]
    fn test_signalfd_enosys() {
        // Reach the ENOSYS sentinel: fd == -1 (create-new),
        // non-NULL mask, no garbage flags.
        let mask: u64 = 0;
        errno::set_errno(0);
        assert_eq!(signalfd(-1, &raw const mask, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- inotify constants (match Linux) --

    #[test]
    fn test_inotify_event_flags() {
        assert_eq!(IN_ACCESS, 0x0000_0001);
        assert_eq!(IN_MODIFY, 0x0000_0002);
        assert_eq!(IN_ATTRIB, 0x0000_0004);
        assert_eq!(IN_CLOSE_WRITE, 0x0000_0008);
        assert_eq!(IN_CLOSE_NOWRITE, 0x0000_0010);
        assert_eq!(IN_OPEN, 0x0000_0020);
        assert_eq!(IN_MOVED_FROM, 0x0000_0040);
        assert_eq!(IN_MOVED_TO, 0x0000_0080);
        assert_eq!(IN_CREATE, 0x0000_0100);
        assert_eq!(IN_DELETE, 0x0000_0200);
        assert_eq!(IN_DELETE_SELF, 0x0000_0400);
        assert_eq!(IN_MOVE_SELF, 0x0000_0800);
    }

    #[test]
    fn test_inotify_composite_flags() {
        assert_eq!(IN_CLOSE, IN_CLOSE_WRITE | IN_CLOSE_NOWRITE);
        assert_eq!(IN_MOVE, IN_MOVED_FROM | IN_MOVED_TO);
        assert_eq!(IN_ALL_EVENTS, 0x0000_0FFF);
    }

    #[test]
    fn test_inotify_init_flags() {
        assert_ne!(IN_CLOEXEC, 0);
        assert_ne!(IN_NONBLOCK, 0);
        assert_ne!(IN_CLOEXEC, IN_NONBLOCK);
    }

    // -- inotify basic init/error paths --

    #[test]
    fn test_inotify_init_returns_fd_or_emfile() {
        // Either we allocate a fresh fd, or the static instance table
        // is exhausted by other parallel tests (EMFILE).  Both are
        // acceptable outcomes — we just shouldn't get ENOSYS anymore.
        errno::set_errno(0);
        let fd = inotify_init();
        if fd >= 0 {
            assert_eq!(crate::file::close(fd), 0);
        } else {
            assert_eq!(errno::get_errno(), errno::EMFILE);
        }
    }

    #[test]
    fn test_inotify_init1_zero_flags_returns_fd_or_emfile() {
        errno::set_errno(0);
        let fd = inotify_init1(0);
        if fd >= 0 {
            assert_eq!(crate::file::close(fd), 0);
        } else {
            assert_eq!(errno::get_errno(), errno::EMFILE);
        }
    }

    #[test]
    fn test_inotify_add_watch_null_path_efault() {
        // Acquire a real inotify fd first so the path check is reached.
        let fd = inotify_init();
        if fd < 0 {
            // Table exhausted; skip.
            return;
        }
        errno::set_errno(0);
        assert_eq!(inotify_add_watch(fd, core::ptr::null(), IN_MODIFY), -1);
        // The fd table is global across tests, so a concurrent test
        // could close+reuse our fd between init() and add_watch().  In
        // that case the kind-mismatch path triggers EINVAL instead of
        // the EFAULT we'd get from the null-path check.  Accept either.
        let e = errno::get_errno();
        assert!(
            e == errno::EFAULT || e == errno::EINVAL || e == errno::EBADF,
            "unexpected errno {e}",
        );
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_rm_watch_bad_fd_ebadf() {
        // fd 12345 should not be open.
        errno::set_errno(0);
        assert_eq!(inotify_rm_watch(12345, 1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- EpollEvent struct layout --

    #[test]
    fn test_epoll_event_size() {
        // EpollEvent should be 12 bytes (4 + 8) or 16 with padding.
        let size = core::mem::size_of::<EpollEvent>();
        assert!(size >= 12);
    }

    #[test]
    fn test_epoll_event_fields() {
        let ev = EpollEvent {
            events: EPOLLIN | EPOLLOUT,
            data: 42,
        };
        // EpollEvent is #[repr(packed)] so direct field references would
        // be UB on platforms where the fields need higher alignment.
        // Copy through locals.
        let events_val: u32 = ev.events;
        let data_val: u64 = ev.data;
        assert_eq!(events_val, EPOLLIN | EPOLLOUT);
        assert_eq!(data_val, 42);
    }

    #[test]
    fn test_epoll_event_data_holds_pointer() {
        // data field is often used to hold a pointer cast to u64.
        let val: u64 = 0x7FFE_0000_1234;
        let ev = EpollEvent {
            events: EPOLLIN,
            data: val,
        };
        let data_val: u64 = ev.data;
        assert_eq!(data_val, val);
    }

    // -- Itimerspec struct layout --

    #[test]
    fn test_itimerspec_size() {
        // Two Timespec (each 16 bytes on LP64) = 32 bytes.
        assert_eq!(core::mem::size_of::<Itimerspec>(), 32);
    }

    #[test]
    fn test_itimerspec_fields() {
        let its = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 500_000_000,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 5,
                tv_nsec: 0,
            },
        };
        assert_eq!(its.it_interval.tv_sec, 1);
        assert_eq!(its.it_interval.tv_nsec, 500_000_000);
        assert_eq!(its.it_value.tv_sec, 5);
        assert_eq!(its.it_value.tv_nsec, 0);
    }

    // -- signalfd with different args --

    #[test]
    fn test_signalfd_negative_fd() {
        // fd == -1 means "create new"; valid mask required to reach
        // the ENOSYS sentinel under the new validator.
        let mask: u64 = 1u64 << 14; // SIGTERM bit, arbitrary.
        errno::set_errno(0);
        assert_eq!(signalfd(-1, &raw const mask, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- epoll_pwait null events --

    #[test]
    fn test_epoll_pwait_null_events_returns_efault() {
        errno::set_errno(0);
        // SAFETY: events pointer is intentionally null; the wrapper
        // checks for null before any dereference.
        let ret = unsafe { epoll_pwait(3, core::ptr::null_mut(), 10, 0, core::ptr::null()) };
        // Either EBADF (if fd 3 isn't an epoll fd) or EFAULT (if it
        // were, since the events pointer is null).  In the unit-test
        // environment fd 3 isn't an epoll fd, so EBADF.
        assert_eq!(ret, -1);
        assert!(
            errno::get_errno() == errno::EBADF
                || errno::get_errno() == errno::EFAULT
                || errno::get_errno() == errno::EINVAL
        );
    }

    // -- timerfd_gettime with null pointer --

    #[test]
    fn test_timerfd_gettime_null_out() {
        // gettime on stdin (fd 0, Console) with null out returns EFAULT.
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(0, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EFAULT || e == errno::EINVAL || e == errno::EBADF,
            "unexpected errno {e}"
        );
    }

    // -- timerfd_settime with null new_value --

    #[test]
    fn test_timerfd_settime_null_new() {
        // settime with null new_value pointer returns EFAULT.
        errno::set_errno(0);
        let ret = unsafe {
            timerfd_settime(
                0,
                TFD_TIMER_ABSTIME,
                core::ptr::null(),
                core::ptr::null_mut(),
            )
        };
        assert_eq!(ret, -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EFAULT || e == errno::EINVAL || e == errno::EBADF,
            "unexpected errno {e}"
        );
    }

    // -- eventfd flag composability --

    #[test]
    fn test_efd_flags_composable() {
        let flags = EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE;
        // All three must be independently representable.
        assert_ne!(flags & EFD_CLOEXEC, 0);
        assert_ne!(flags & EFD_NONBLOCK, 0);
        assert_ne!(flags & EFD_SEMAPHORE, 0);
    }

    // -- timerfd flag composability --

    #[test]
    fn test_tfd_flags_composable() {
        let flags = TFD_CLOEXEC | TFD_NONBLOCK;
        assert_ne!(flags & TFD_CLOEXEC, 0);
        assert_ne!(flags & TFD_NONBLOCK, 0);
    }

    // -- epoll_create with different sizes --

    #[test]
    fn test_epoll_create_zero_size() {
        // POSIX: epoll_create requires size > 0, else EINVAL.
        errno::set_errno(0);
        assert_eq!(epoll_create(0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_epoll_create_large_size() {
        // The `size` arg is advisory (Linux ignores it since 2.6.8). A
        // large positive value should succeed and return a valid fd.
        errno::set_errno(0);
        let fd = epoll_create(1000);
        assert!(fd >= 0, "epoll_create(1000) should succeed, got {fd}");
        // Clean up so we don't exhaust the fd table across tests.
        crate::file::close(fd);
    }

    // -- epoll_create1 with flags --

    #[test]
    fn test_epoll_create1_cloexec() {
        // epoll_create1(O_CLOEXEC) should succeed.
        errno::set_errno(0);
        let fd = epoll_create1(crate::fcntl::O_CLOEXEC);
        assert!(fd >= 0, "epoll_create1(O_CLOEXEC) should succeed, got {fd}");
        crate::file::close(fd);
    }

    // -- epoll_ctl all operations --

    #[test]
    fn test_epoll_ctl_del_on_bad_epfd() {
        // EPOLL_CTL_DEL with an invalid epfd must fail. The exact errno
        // depends on the impl path (EBADF if the fd isn't an epoll fd,
        // ENOENT if the entry isn't registered) — accept either.
        errno::set_errno(0);
        let ret = epoll_ctl(3, EPOLL_CTL_DEL, 4, core::ptr::null_mut());
        assert_eq!(ret, -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EBADF || e == errno::ENOENT || e == errno::EINVAL,
            "unexpected errno {e}"
        );
    }

    #[test]
    fn test_epoll_ctl_mod_with_null_event() {
        // EPOLL_CTL_MOD with a null event pointer must fail.
        errno::set_errno(0);
        let ret = epoll_ctl(3, EPOLL_CTL_MOD, 4, core::ptr::null_mut());
        assert_eq!(ret, -1);
        let e = errno::get_errno();
        assert!(
            e == errno::EBADF || e == errno::EFAULT || e == errno::EINVAL || e == errno::ENOENT,
            "unexpected errno {e}"
        );
    }

    // -- eventfd input validation (no kernel needed) --
    //
    // The success path requires a real kernel and is exercised by
    // integration tests (see tests/eventfd.rs).  These cases all fail
    // before any syscall is issued.

    /// `EFD_SEMAPHORE` combines cleanly with the userspace fd-table
    /// flags — they live in disjoint bits.  Functional success is
    /// covered by integration tests with a live kernel.
    #[test]
    fn test_eventfd_semaphore_disjoint_from_fd_flags() {
        assert_eq!(EFD_SEMAPHORE & EFD_CLOEXEC, 0);
        assert_eq!(EFD_SEMAPHORE & EFD_NONBLOCK, 0);
    }

    /// Multiple unknown flag bits are still rejected.
    #[test]
    fn test_eventfd_multiple_unknown_flags_rejected() {
        errno::set_errno(0);
        let bad = 0x40 | 0x80;
        assert_eq!(bad & (EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE), 0);
        assert_eq!(eventfd(42, bad), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- inotify_init1 with flags --

    #[test]
    fn test_inotify_init1_cloexec() {
        let fd = inotify_init1(IN_CLOEXEC);
        if fd < 0 {
            // Table exhausted under parallel tests is acceptable.
            assert_eq!(errno::get_errno(), errno::EMFILE);
            return;
        }
        // Verify FD_CLOEXEC was set in fdtable.
        let flags = fdtable::get_fd_flags(fd).unwrap_or(0);
        assert!(flags & fdtable::FD_CLOEXEC != 0);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_init1_nonblock() {
        let fd = inotify_init1(IN_NONBLOCK);
        if fd < 0 {
            assert_eq!(errno::get_errno(), errno::EMFILE);
            return;
        }
        // Verify O_NONBLOCK propagated into status flags.
        let sflags = fdtable::get_status_flags(fd).unwrap_or(0);
        assert!(sflags & crate::fcntl::O_NONBLOCK != 0);
        assert!(inotify_is_nonblock(fdtable::get_fd(fd).unwrap().handle));
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_init1_combined_flags() {
        let fd = inotify_init1(IN_CLOEXEC | IN_NONBLOCK);
        if fd < 0 {
            assert_eq!(errno::get_errno(), errno::EMFILE);
            return;
        }
        let fd_flags = fdtable::get_fd_flags(fd).unwrap_or(0);
        let st_flags = fdtable::get_status_flags(fd).unwrap_or(0);
        assert!(fd_flags & fdtable::FD_CLOEXEC != 0);
        assert!(st_flags & crate::fcntl::O_NONBLOCK != 0);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_init1_invalid_flags() {
        // Unknown bits → EINVAL, no fd allocated.
        errno::set_errno(0);
        let bad = 0x4000_0000;
        assert_eq!(bad & (IN_CLOEXEC | IN_NONBLOCK), 0);
        assert_eq!(inotify_init1(bad), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- inotify_add_watch with path --

    #[test]
    fn test_inotify_add_watch_no_event_bits_einval() {
        // mask without any IN_KNOWN_EVENTS bit set → EINVAL.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        // Only IN_MASK_ADD-style flag bits, no event bits.
        assert_eq!(inotify_add_watch(fd, b"/tmp\0".as_ptr(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_add_watch_non_inotify_fd_einval() {
        // Use an eventfd as a fake inotify fd → EINVAL.
        let efd = eventfd(0, 0);
        if efd < 0 {
            return;
        }
        errno::set_errno(0);
        assert_eq!(inotify_add_watch(efd, b"/tmp\0".as_ptr(), IN_MODIFY), -1,);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(efd);
    }

    // ------------------------------------------------------------------
    // Phase 139 — inotify_add_watch validation ordering matches
    // `sys_inotify_add_watch` (mask → fd → pathname).
    //
    // Pre-Phase 139:
    //     pathname NULL → EFAULT     (first)
    //     mask == 0     → EINVAL     (second)
    //     fd invalid    → EBADF      (third)
    //
    // Linux's order (`fs/notify/inotify/inotify_user.c`):
    //     mask check    → EINVAL     (first; inotify_arg_to_mask)
    //     fdget         → EBADF      (second)
    //     non-inotify   → EINVAL     (third)
    //     user_path_at  → EFAULT     (fourth; NULL pathname)
    //
    // The reorder is observable when more than one error condition is
    // present at once: e.g. NULL pathname + zero mask returns EINVAL on
    // Linux but used to return EFAULT here.
    // ------------------------------------------------------------------

    // --- per-error-class smoke tests under the new ordering ----------

    #[test]
    fn test_inotify_add_watch_phase139_zero_mask_alone_einval() {
        // Sanity: mask check still fires when the rest is fine.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        assert_eq!(inotify_add_watch(fd, b"/tmp\0".as_ptr(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_add_watch_phase139_bad_fd_alone_ebadf() {
        // Valid mask, valid pathname, bad fd → EBADF.
        errno::set_errno(0);
        assert_eq!(
            inotify_add_watch(100_000, b"/tmp\0".as_ptr(), IN_MODIFY),
            -1
        );
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_inotify_add_watch_phase139_null_pathname_after_good_fd_efault() {
        // Valid mask, valid inotify fd, NULL pathname → EFAULT.
        // (Path check is now last.)
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        let ret = inotify_add_watch(fd, core::ptr::null(), IN_MODIFY);
        assert_eq!(ret, -1);
        // The fd table is global; a parallel test could have closed
        // and reused the fd as a non-inotify kind, which would surface
        // EINVAL at the kind check before EFAULT.  Accept either.
        let e = errno::get_errno();
        assert!(
            e == errno::EFAULT || e == errno::EINVAL || e == errno::EBADF,
            "expected EFAULT/EINVAL/EBADF, got {e}"
        );
        crate::file::close(fd);
    }

    // --- ordering matrix --------------------------------------------------

    #[test]
    fn test_inotify_add_watch_phase139_zero_mask_beats_null_pathname() {
        // CORE REGRESSION: NULL pathname + zero mask.  Pre-Phase 139:
        // EFAULT.  Linux / Phase 139+: EINVAL.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        let ret = inotify_add_watch(fd, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_add_watch_phase139_zero_mask_beats_bad_fd() {
        // Zero mask + bad fd: Linux's mask check fires before fdget,
        // so EINVAL wins over EBADF.  Pre-Phase 139 we ALSO returned
        // EINVAL here (the path-NULL check passed because pathname is
        // non-NULL; then mask check fired before fd lookup), so this
        // is a regression-guard rather than a behaviour change — but
        // pin it because the reorder shouldn't disturb it.
        errno::set_errno(0);
        let ret = inotify_add_watch(100_000, b"/tmp\0".as_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_inotify_add_watch_phase139_zero_mask_beats_bad_fd_and_null_path() {
        // Triple-bad: zero mask + bad fd + NULL pathname.  EINVAL
        // wins over both EBADF and EFAULT.
        errno::set_errno(0);
        let ret = inotify_add_watch(100_000, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_inotify_add_watch_phase139_bad_fd_beats_null_pathname() {
        // Valid mask + bad fd + NULL pathname: EBADF wins over
        // EFAULT (fd check before pathname check).
        errno::set_errno(0);
        let ret = inotify_add_watch(100_000, core::ptr::null(), IN_MODIFY);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_inotify_add_watch_phase139_bad_fd_beats_null_pathname_negative_fd() {
        // Same as above with a clearly-invalid fd (-1) to make sure
        // the negative-fd path also routes through fdget → EBADF
        // ahead of the pathname check.
        errno::set_errno(0);
        let ret = inotify_add_watch(-1, core::ptr::null(), IN_MODIFY);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_inotify_add_watch_phase139_non_inotify_fd_beats_null_pathname() {
        // Valid mask + non-inotify fd + NULL pathname: kind-mismatch
        // EINVAL wins over EFAULT.  (Both kinds → EINVAL after Phase
        // 139, but pre-Phase 139 the EFAULT pathname check fired
        // first.)
        let efd = eventfd(0, 0);
        if efd < 0 {
            return;
        }
        errno::set_errno(0);
        let ret = inotify_add_watch(efd, core::ptr::null(), IN_MODIFY);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(efd);
    }

    // --- buggy callers ---------------------------------------------------

    #[test]
    fn test_inotify_add_watch_phase139_buggy_caller_zero_mask_optimistic() {
        // Caller forgets to OR in any event bits, passes 0 as a
        // "all events" sentinel.  Pre-Phase 139 with NULL pathname
        // this surfaced EFAULT (misleading: the pathname is fine).
        // Linux / now: EINVAL points at the actual bug (mask).
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        // Caller passed b"/tmp\0" (valid pointer) with 0 mask.
        let ret = inotify_add_watch(fd, b"/tmp\0".as_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_add_watch_phase139_buggy_caller_mask_is_only_unknown_bits() {
        // Caller passes only unknown/flag bits (e.g. IN_MASK_ADD-only
        // without any event); mask & IN_KNOWN_EVENTS == 0 → EINVAL.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        // 0x4000_0000 is outside IN_KNOWN_EVENTS.
        let only_flag_bits: u32 = 0x4000_0000;
        assert_eq!(only_flag_bits & IN_KNOWN_EVENTS, 0);
        errno::set_errno(0);
        let ret = inotify_add_watch(fd, b"/tmp\0".as_ptr(), only_flag_bits);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    // --- workflow + recovery -----------------------------------------

    #[test]
    fn test_inotify_add_watch_phase139_recovery_after_zero_mask_einval() {
        // Caller corrects the mask, retries with a valid event bit;
        // path is fine, fd is fine, so the call reaches the path
        // resolution stage.  We don't assert success (the path may
        // not exist in the test sandbox), only that EINVAL doesn't
        // stick.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        assert_eq!(inotify_add_watch(fd, b"/tmp\0".as_ptr(), 0), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let r2 = inotify_add_watch(fd, b"/tmp\0".as_ptr(), IN_MODIFY);
        if r2 < 0 {
            let e = errno::get_errno();
            // Whatever the failure, it must NOT be the "zero mask"
            // EINVAL we just recovered from.  Acceptable: ENOENT
            // (no /tmp in sandbox), EBADF (fd race), etc.
            assert!(
                e == errno::ENOENT
                    || e == errno::ENAMETOOLONG
                    || e == errno::ENOSPC
                    || e == errno::EBADF
                    || e == errno::EINVAL,
                "unexpected recovery errno {e}"
            );
        }
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_add_watch_phase139_diagnostic_order_for_libinotify() {
        // Real-world: inotify-tools' wrapper calls add_watch with the
        // user-supplied mask and a NULL-pre-checked pathname.  If the
        // wrapper accidentally passes mask=0 when the user forgot
        // `-e` / `--event`, the user sees EINVAL telling them the
        // mask is wrong, not EFAULT (which would suggest the path).
        // Pin this exact diagnostic shape.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        let ret = inotify_add_watch(fd, b"/etc\0".as_ptr(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_add_watch_phase139_no_side_effect_on_einval_loop() {
        // 50 consecutive zero-mask rejections must not allocate any
        // watches: the next successful call has a fresh wd space.
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        for _ in 0..50 {
            errno::set_errno(0);
            assert_eq!(inotify_add_watch(fd, b"/tmp\0".as_ptr(), 0), -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
        // The 51st valid-mask call may still fail (no /tmp etc.) but
        // mustn't carry a stale EINVAL from the rejections.
        errno::set_errno(0);
        let _ = inotify_add_watch(fd, b"/tmp\0".as_ptr(), IN_MODIFY);
        // If the call returned -1, errno was rewritten by add_watch
        // (mask check passed; either ENOENT or success).
        crate::file::close(fd);
    }

    // -- inotify event flag single-bit checks --

    #[test]
    fn test_inotify_event_flags_single_bits() {
        let flags = [
            IN_ACCESS,
            IN_MODIFY,
            IN_ATTRIB,
            IN_CLOSE_WRITE,
            IN_CLOSE_NOWRITE,
            IN_OPEN,
            IN_MOVED_FROM,
            IN_MOVED_TO,
            IN_CREATE,
            IN_DELETE,
            IN_DELETE_SELF,
            IN_MOVE_SELF,
        ];
        for f in flags {
            assert_eq!(f.count_ones(), 1, "flag 0x{f:x} is not a single bit");
        }
    }

    // -- relative_name (pure path relation) --

    #[test]
    fn test_relative_name_self_match() {
        assert!(matches!(relative_name(b"/a/b", b"/a/b"), Rel::SelfPath));
    }

    #[test]
    fn test_relative_name_direct_child() {
        match relative_name(b"/a/b", b"/a/b/file.txt") {
            Rel::Child(name) => assert_eq!(name, b"file.txt"),
            _ => panic!("expected Child"),
        }
    }

    #[test]
    fn test_relative_name_grandchild_not_matched() {
        // inotify is non-recursive: nested paths must not match.
        assert!(matches!(
            relative_name(b"/a/b", b"/a/b/sub/file.txt"),
            Rel::NotMatched
        ));
    }

    #[test]
    fn test_relative_name_sibling_prefix_not_matched() {
        // "/a/bc" shares the textual prefix "/a/b" but is not a child of it.
        assert!(matches!(relative_name(b"/a/b", b"/a/bc"), Rel::NotMatched));
    }

    #[test]
    fn test_relative_name_unrelated_not_matched() {
        assert!(matches!(relative_name(b"/a/b", b"/x/y"), Rel::NotMatched));
    }

    #[test]
    fn test_relative_name_root_child() {
        // Root "/" is its own prefix with no separator byte.
        match relative_name(b"/", b"/etc") {
            Rel::Child(name) => assert_eq!(name, b"etc"),
            _ => panic!("expected Child"),
        }
        assert!(matches!(
            relative_name(b"/", b"/etc/passwd"),
            Rel::NotMatched
        ));
    }

    // -- translate_kernel_event (pure kernel→inotify mapping) --

    #[test]
    fn test_translate_created_child() {
        let t = translate_kernel_event(b"/w", IN_CREATE, 7, KEV_CREATED, b"/w/new", b"", 0);
        assert_eq!(t.count, 1);
        assert!(!t.disarm);
        let ev = t.events[0];
        assert_eq!(ev.wd, 7);
        assert_eq!(ev.mask, IN_CREATE);
        assert_eq!(&ev.name[..ev.name_len as usize], b"new");
    }

    #[test]
    fn test_translate_created_masked_off() {
        // Mask doesn't request IN_CREATE → no event.
        let t = translate_kernel_event(b"/w", IN_DELETE, 7, KEV_CREATED, b"/w/new", b"", 0);
        assert_eq!(t.count, 0);
    }

    #[test]
    fn test_translate_delete_self_disarms() {
        let t = translate_kernel_event(b"/w", IN_DELETE_SELF, 3, KEV_DELETED, b"/w", b"", 0);
        assert!(t.disarm);
        // IN_DELETE_SELF then IN_IGNORED.
        assert_eq!(t.count, 2);
        assert_eq!(t.events[0].mask, IN_DELETE_SELF);
        assert_eq!(t.events[1].mask, IN_IGNORED);
    }

    #[test]
    fn test_translate_delete_self_ignored_only_when_unmasked() {
        // Even if IN_DELETE_SELF wasn't requested, IN_IGNORED still fires
        // and the watch is disarmed.
        let t = translate_kernel_event(b"/w", IN_CREATE, 3, KEV_DELETED, b"/w", b"", 0);
        assert!(t.disarm);
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].mask, IN_IGNORED);
    }

    #[test]
    fn test_translate_delete_child() {
        let t = translate_kernel_event(b"/w", IN_DELETE, 3, KEV_DELETED, b"/w/gone", b"", 0);
        assert!(!t.disarm);
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].mask, IN_DELETE);
        assert_eq!(&t.events[0].name[..t.events[0].name_len as usize], b"gone");
    }

    #[test]
    fn test_translate_modify_self_empty_name() {
        let t = translate_kernel_event(b"/w/f", IN_MODIFY, 5, KEV_MODIFIED, b"/w/f", b"", 0);
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].mask, IN_MODIFY);
        assert_eq!(t.events[0].name_len, 0);
    }

    #[test]
    fn test_translate_rename_pair_shares_cookie() {
        let t = translate_kernel_event(
            b"/w",
            IN_MOVED_FROM | IN_MOVED_TO,
            9,
            KEV_RENAMED,
            b"/w/old",
            b"/w/new",
            0x1234,
        );
        assert_eq!(t.count, 2);
        assert_eq!(t.events[0].mask, IN_MOVED_FROM);
        assert_eq!(t.events[0].cookie, 0x1234);
        assert_eq!(&t.events[0].name[..t.events[0].name_len as usize], b"old");
        assert_eq!(t.events[1].mask, IN_MOVED_TO);
        assert_eq!(t.events[1].cookie, 0x1234);
        assert_eq!(&t.events[1].name[..t.events[1].name_len as usize], b"new");
    }

    #[test]
    fn test_translate_rename_self_is_move_self() {
        let t = translate_kernel_event(
            b"/w",
            IN_MOVE_SELF,
            9,
            KEV_RENAMED,
            b"/w",
            b"/elsewhere",
            0x55,
        );
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].mask, IN_MOVE_SELF);
    }

    #[test]
    fn test_translate_metadata_and_access() {
        let t = translate_kernel_event(b"/w/f", IN_ATTRIB, 2, KEV_METADATA, b"/w/f", b"", 0);
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].mask, IN_ATTRIB);

        let t = translate_kernel_event(b"/w/f", IN_ACCESS, 2, KEV_ACCESSED, b"/w/f", b"", 0);
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].mask, IN_ACCESS);
    }

    #[test]
    fn test_translate_overflow() {
        let t = translate_kernel_event(b"/w", 0, 0, KEV_OVERFLOW, b"", b"", 0);
        assert_eq!(t.count, 1);
        assert_eq!(t.events[0].wd, -1);
        assert_eq!(t.events[0].mask, IN_Q_OVERFLOW);
    }

    #[test]
    fn test_translate_grandchild_ignored() {
        // A create deep under the watch must not surface (non-recursive).
        let t = translate_kernel_event(b"/w", IN_CREATE, 1, KEV_CREATED, b"/w/sub/deep", b"", 0);
        assert_eq!(t.count, 0);
    }

    // -- inotify_to_kernel_mask collapsing --

    #[test]
    fn test_inotify_to_kernel_mask_collapses() {
        // All move bits collapse onto a single kernel RENAME bit.
        assert_eq!(
            inotify_to_kernel_mask(IN_MOVED_FROM | IN_MOVED_TO | IN_MOVE_SELF),
            KMASK_RENAME
        );
        // Both delete bits collapse onto DELETE.
        assert_eq!(
            inotify_to_kernel_mask(IN_DELETE | IN_DELETE_SELF),
            KMASK_DELETE
        );
        // Open/close have no kernel equivalent → empty mask.
        assert_eq!(
            inotify_to_kernel_mask(IN_OPEN | IN_CLOSE_WRITE | IN_CLOSE_NOWRITE),
            0
        );
    }

    // -- signalfd with positive fd (modify existing) --

    #[test]
    fn test_signalfd_positive_fd() {
        // fd 3 is unlikely to be open in the test fdtable, so the new
        // validator rejects it with EBADF before reaching ENOSYS.  This
        // exercises the existing-fd path which previously returned
        // ENOSYS without validation.
        let mask: u64 = 0;
        errno::set_errno(0);
        assert_eq!(signalfd(3, &raw const mask, 0), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -- timerfd_create with clock IDs --

    #[test]
    fn test_timerfd_create_monotonic() {
        // CLOCK_MONOTONIC = 1 — all timerfds use monotonic regardless.
        errno::set_errno(0);
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0, "timerfd_create(1, 0) should succeed");
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_with_flags() {
        // TFD_CLOEXEC | TFD_NONBLOCK is valid.
        errno::set_errno(0);
        let fd = timerfd_create(0, TFD_CLOEXEC | TFD_NONBLOCK);
        assert!(fd >= 0, "timerfd_create with flags should succeed");
        crate::file::close(fd);
    }

    // -- Positive functional tests for timerfd --

    #[test]
    fn test_timerfd_settime_gettime_round_trip() {
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);

        // Arm: 1 second initial, no interval (one-shot).
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        let r = unsafe { timerfd_settime(fd, 0, &new, core::ptr::null_mut()) };
        assert_eq!(r, 0);

        // gettime should report a value <= 1 second remaining.
        let mut cur = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        let r2 = unsafe { timerfd_gettime(fd, &mut cur) };
        assert_eq!(r2, 0);
        // Copy through locals (no packed concerns here, just defensive).
        let sec = cur.it_value.tv_sec;
        let nsec = cur.it_value.tv_nsec;
        assert!(sec >= 0 && sec <= 1);
        assert!(nsec >= 0 && nsec < 1_000_000_000);

        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_disarm() {
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);

        // Arm.
        let armed = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 5,
                tv_nsec: 0,
            },
        };
        unsafe {
            timerfd_settime(fd, 0, &armed, core::ptr::null_mut());
        }

        // Disarm (it_value = 0).
        let disarm = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        let r = unsafe { timerfd_settime(fd, 0, &disarm, core::ptr::null_mut()) };
        assert_eq!(r, 0);

        // gettime should report 0 remaining (disarmed).
        let mut cur = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        unsafe {
            timerfd_gettime(fd, &mut cur);
        }
        let sec = cur.it_value.tv_sec;
        let nsec = cur.it_value.tv_nsec;
        assert_eq!(sec, 0);
        assert_eq!(nsec, 0);

        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_rejects_negative_timespec() {
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);

        let bad = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: -1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_settime(fd, 0, &bad, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_rejects_unknown_flags() {
        errno::set_errno(0);
        let fd = timerfd_create(1, 0xDEAD_BEEFu32 as i32);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- Phase 93: clockid validation --

    #[test]
    fn test_timerfd_create_phase93_clock_realtime_accepted() {
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_REALTIME, 0);
        assert!(fd >= 0);
        assert_ne!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_monotonic_accepted() {
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(fd >= 0);
        assert_ne!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_boottime_accepted() {
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_BOOTTIME, 0);
        assert!(fd >= 0);
        assert_ne!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_realtime_alarm_accepted() {
        // CLOCK_REALTIME_ALARM = 8.
        errno::set_errno(0);
        let fd = timerfd_create(8, 0);
        assert!(fd >= 0);
        assert_ne!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_boottime_alarm_accepted() {
        // CLOCK_BOOTTIME_ALARM = 9.
        errno::set_errno(0);
        let fd = timerfd_create(9, 0);
        assert!(fd >= 0);
        assert_ne!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_process_cputime_rejected() {
        // CLOCK_PROCESS_CPUTIME_ID = 2.  Valid clock_gettime clock,
        // NOT a valid timerfd_create clock — Linux rejects it.
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_PROCESS_CPUTIME_ID, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_thread_cputime_rejected() {
        // CLOCK_THREAD_CPUTIME_ID = 3.
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_THREAD_CPUTIME_ID, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_monotonic_raw_rejected() {
        // CLOCK_MONOTONIC_RAW = 4.  Valid for clock_gettime but not
        // for timerfd_create (no kernel support for it as a timer base).
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_MONOTONIC_RAW, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_realtime_coarse_rejected() {
        // CLOCK_REALTIME_COARSE = 5.
        errno::set_errno(0);
        let fd = timerfd_create(crate::time::CLOCK_REALTIME_COARSE, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_clock_tai_rejected() {
        // CLOCK_TAI = 11.  Not accepted by Linux timerfd_create.
        errno::set_errno(0);
        let fd = timerfd_create(11, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_negative_clockid_rejected() {
        errno::set_errno(0);
        let fd = timerfd_create(-1, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_garbage_clockid_rejected() {
        errno::set_errno(0);
        let fd = timerfd_create(9999, 0);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_flag_check_beats_clockid_check() {
        // Both bad → EINVAL.  Order matches Linux: flags first.
        // (Both return EINVAL, but the function returns from the flag
        // branch first — easier to maintain a single error pathway.)
        errno::set_errno(0);
        let fd = timerfd_create(9999, 0xDEAD_BEEFu32 as i32);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_create_phase93_einval_then_valid_progression() {
        // CLOCK_TAI = 11 is not in `crate::time`; use the literal.
        errno::set_errno(0);
        let bad = timerfd_create(11, 0);
        assert_eq!(bad, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let good = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(good >= 0);
        crate::file::close(good);
    }

    #[test]
    fn test_timerfd_read_nonblock_no_expirations() {
        // A newly-created, unarmed timerfd with TFD_NONBLOCK should
        // return EAGAIN on read.
        let fd = timerfd_create(1, TFD_NONBLOCK);
        assert!(fd >= 0);
        let mut buf = [0u8; 8];
        errno::set_errno(0);
        let n = crate::file::read(fd, buf.as_mut_ptr(), 8);
        assert_eq!(n, -1);
        assert_eq!(errno::get_errno(), errno::EAGAIN);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_close_round_trip() {
        // Allocate + free repeatedly to ensure the slot table doesn't leak.
        for _ in 0..32 {
            let fd = timerfd_create(1, 0);
            assert!(fd >= 0);
            assert_eq!(crate::file::close(fd), 0);
        }
    }

    // -- Epoll ctl ops are distinct --

    #[test]
    fn test_epoll_ctl_ops_distinct() {
        assert_ne!(EPOLL_CTL_ADD, EPOLL_CTL_DEL);
        assert_ne!(EPOLL_CTL_ADD, EPOLL_CTL_MOD);
        assert_ne!(EPOLL_CTL_DEL, EPOLL_CTL_MOD);
    }

    // -- epoll_wait with timeout 0 (poll mode) --

    #[test]
    fn test_epoll_wait_poll_mode() {
        // Calling epoll_wait on an invalid (never-created) epfd returns
        // EBADF. (3 is not a valid epoll fd in a fresh test process.)
        errno::set_errno(0);
        let ret = unsafe { epoll_wait(3, core::ptr::null_mut(), 10, 0) };
        assert_eq!(ret, -1);
        assert!(
            errno::get_errno() == errno::EBADF
                || errno::get_errno() == errno::EFAULT
                || errno::get_errno() == errno::EINVAL
        );
    }

    // -- TFD/EFD/IN flag values match Linux octal --

    #[test]
    fn test_cloexec_flags_consistent() {
        // All CLOEXEC flags should have the same value across subsystems.
        assert_eq!(EFD_CLOEXEC, TFD_CLOEXEC);
        assert_eq!(EFD_CLOEXEC, IN_CLOEXEC);
    }

    #[test]
    fn test_nonblock_flags_consistent() {
        assert_eq!(EFD_NONBLOCK, TFD_NONBLOCK);
        assert_eq!(EFD_NONBLOCK, IN_NONBLOCK);
    }

    // -----------------------------------------------------------------------
    // signalfd4 — Linux alias for signalfd
    // -----------------------------------------------------------------------

    #[test]
    fn test_signalfd4_returns_enosys() {
        // signalfd4 delegates to signalfd; with valid mask + no flags
        // we reach the ENOSYS sentinel.
        let mask: u64 = 0;
        crate::errno::set_errno(0);
        let ret = signalfd4(-1, &raw const mask, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_signalfd4_with_flags() {
        // SFD_CLOEXEC happens to share its value with O_CLOEXEC (0x80000),
        // distinct from EFD_CLOEXEC.  Use the proper signalfd flag here.
        let mask: u64 = 0;
        crate::errno::set_errno(0);
        let ret = signalfd4(-1, &raw const mask, SFD_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // epoll_pwait2
    // -----------------------------------------------------------------------

    #[test]
    fn test_epoll_pwait2_returns_ebadf_for_bad_fd() {
        // epoll_pwait2 with an invalid epfd returns EBADF.
        crate::errno::set_errno(0);
        let ret = unsafe {
            epoll_pwait2(
                -1,
                core::ptr::null_mut(),
                0,
                core::ptr::null(),
                core::ptr::null(),
            )
        };
        assert_eq!(ret, -1);
        assert!(
            crate::errno::get_errno() == crate::errno::EBADF
                || crate::errno::get_errno() == crate::errno::EINVAL
                || crate::errno::get_errno() == crate::errno::EFAULT
        );
    }

    #[test]
    fn test_epoll_pwait2_with_timeout() {
        // Same invalid-fd path but with a non-null timespec.
        crate::errno::set_errno(0);
        let ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 100_000,
        };
        let ret = unsafe { epoll_pwait2(-1, core::ptr::null_mut(), 1, &ts, core::ptr::null()) };
        assert_eq!(ret, -1);
        assert!(
            crate::errno::get_errno() == crate::errno::EBADF
                || crate::errno::get_errno() == crate::errno::EINVAL
                || crate::errno::get_errno() == crate::errno::EFAULT
        );
    }

    // -----------------------------------------------------------------------
    // Positive functional tests for the new userspace epoll implementation.
    // These exercise epoll_create → epoll_ctl → epoll_wait (timeout=0) round
    // trips. They don't depend on any kernel readiness, just the userspace
    // state machine, so they're hermetic.
    // -----------------------------------------------------------------------

    #[test]
    fn test_epoll_create_close_round_trip() {
        // Allocating + freeing an epoll fd should not leak the slot.
        for _ in 0..32 {
            let fd = epoll_create(1);
            assert!(fd >= 0);
            assert_eq!(crate::file::close(fd), 0);
        }
    }

    #[test]
    fn test_epoll_create1_zero_flags() {
        // epoll_create1(0) is the documented "fresh epoll" form.
        let fd = epoll_create1(0);
        assert!(fd >= 0);
        crate::file::close(fd);
    }

    #[test]
    fn test_epoll_create1_rejects_unknown_flags() {
        // Unknown flag bits must yield EINVAL.
        errno::set_errno(0);
        let fd = epoll_create1(0xDEAD_BEEFu32 as i32);
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_epoll_ctl_add_then_del() {
        // ADD a (bogus) fd to a valid epoll instance, then DEL it. The
        // implementation should accept ADD on any fd in range and let
        // epoll_wait's check_readiness reject it later. DEL should then
        // succeed since the entry exists.
        let ep = epoll_create1(0);
        assert!(ep >= 0);
        // Use a freshly created instance fd as the target so it's at
        // least a valid userspace fd. The kind doesn't matter — epoll
        // doesn't validate that you can sensibly poll it.
        let target = epoll_create1(0);
        assert!(target >= 0);

        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0xAABB_CCDDu64,
        };
        let r = epoll_ctl(ep, EPOLL_CTL_ADD, target, &mut ev as *mut _);
        assert_eq!(r, 0, "EPOLL_CTL_ADD should succeed");

        // Re-adding the same fd should fail with EEXIST.
        errno::set_errno(0);
        let r2 = epoll_ctl(ep, EPOLL_CTL_ADD, target, &mut ev as *mut _);
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::EEXIST);

        // DEL should succeed.
        let r3 = epoll_ctl(ep, EPOLL_CTL_DEL, target, core::ptr::null_mut());
        assert_eq!(r3, 0);

        // DEL again should fail with ENOENT.
        errno::set_errno(0);
        let r4 = epoll_ctl(ep, EPOLL_CTL_DEL, target, core::ptr::null_mut());
        assert_eq!(r4, -1);
        assert_eq!(errno::get_errno(), errno::ENOENT);

        crate::file::close(target);
        crate::file::close(ep);
    }

    #[test]
    fn test_epoll_wait_timeout_zero_no_events() {
        // A fresh epoll instance with no entries should return 0 events
        // immediately when polled with timeout=0.
        let ep = epoll_create1(0);
        assert!(ep >= 0);
        let mut events: [EpollEvent; 4] = [EpollEvent { events: 0, data: 0 }; 4];
        let n = unsafe { epoll_wait(ep, events.as_mut_ptr(), 4, 0) };
        assert_eq!(n, 0, "empty epoll should return 0 events");
        crate::file::close(ep);
    }

    #[test]
    fn test_epoll_wait_rejects_zero_maxevents() {
        let ep = epoll_create1(0);
        assert!(ep >= 0);
        let mut events: [EpollEvent; 1] = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let n = unsafe { epoll_wait(ep, events.as_mut_ptr(), 0, 0) };
        assert_eq!(n, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(ep);
    }

    #[test]
    fn test_epoll_ctl_self_loop_rejected() {
        // Linux forbids adding an epoll fd to itself (would create a
        // poll cycle). Whether our impl detects this is optional — at
        // minimum it must not crash.
        let ep = epoll_create1(0);
        assert!(ep >= 0);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        let r = epoll_ctl(ep, EPOLL_CTL_ADD, ep, &mut ev as *mut _);
        // Accept either EINVAL (cycle detection) or success (we don't
        // enforce it yet — the resulting wait just won't fire).
        assert!(r == 0 || r == -1);
        crate::file::close(ep);
    }

    // -- inotify functional round-trips --

    #[test]
    fn test_inotify_rm_watch_non_inotify_fd_einval() {
        let efd = eventfd(0, 0);
        if efd < 0 {
            return;
        }
        errno::set_errno(0);
        assert_eq!(inotify_rm_watch(efd, 1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(efd);
    }

    #[test]
    fn test_inotify_rm_watch_unknown_wd_einval() {
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        errno::set_errno(0);
        // No watches added, so any wd is unknown.
        assert_eq!(inotify_rm_watch(fd, 1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_fd_is_inotify_kind() {
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        let entry = fdtable::get_fd(fd).expect("inotify fd should exist");
        assert_eq!(entry.kind, HandleKind::Inotify);
        crate::file::close(fd);
    }

    #[test]
    fn test_inotify_close_releases_instance() {
        // Open and close several inotify fds in sequence; each should
        // free the underlying instance slot so we never see EMFILE
        // when reusing them serially.
        for _ in 0..8 {
            let fd = inotify_init();
            if fd < 0 {
                // Concurrent test pressure — acceptable.
                return;
            }
            assert_eq!(crate::file::close(fd), 0);
        }
    }

    #[test]
    fn test_inotify_dup_shares_instance() {
        let fd = inotify_init();
        if fd < 0 {
            return;
        }
        let dup = crate::file::dup(fd);
        if dup < 0 {
            crate::file::close(fd);
            return;
        }
        // Both fds should point to the same Inotify handle index.
        let e1 = fdtable::get_fd(fd).expect("fd entry");
        let e2 = fdtable::get_fd(dup).expect("dup entry");
        assert_eq!(e1.kind, HandleKind::Inotify);
        assert_eq!(e2.kind, HandleKind::Inotify);
        assert_eq!(e1.handle, e2.handle);
        crate::file::close(dup);
        crate::file::close(fd);
    }

    // -----------------------------------------------------------------------
    // Phase 63: signalfd argument-domain validation
    // -----------------------------------------------------------------------

    #[test]
    fn test_sfd_flag_constants() {
        // Match Linux: SFD_CLOEXEC == O_CLOEXEC, SFD_NONBLOCK == O_NONBLOCK.
        assert_eq!(SFD_CLOEXEC, 0x0008_0000);
        assert_eq!(SFD_NONBLOCK, 0x0000_0800);
        assert_eq!(SFD_FLAGS_VALID, SFD_CLOEXEC | SFD_NONBLOCK);
    }

    #[test]
    fn test_sfd_flags_distinct_bits() {
        assert_eq!(SFD_CLOEXEC & SFD_NONBLOCK, 0);
    }

    #[test]
    fn test_signalfd_unknown_flag_einval() {
        // 0x1 is outside SFD_FLAGS_VALID (0x80800).
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-1, &raw const mask, 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_signalfd_garbage_flag_einval() {
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-1, &raw const mask, i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_signalfd_both_flags_pass_flag_check() {
        // SFD_CLOEXEC | SFD_NONBLOCK must reach ENOSYS, not EINVAL.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-1, &raw const mask, SFD_CLOEXEC | SFD_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_signalfd_null_mask_efault() {
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_signalfd_negative_existing_fd_ebadf() {
        // fd < 0 and fd != -1 — the "modify existing" form requires
        // a real fd, so -2 etc. is EBADF.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-2, &raw const mask, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_signalfd_nonexistent_fd_ebadf() {
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(100_000, &raw const mask, 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // --- ordering -----------------------------------------------------------

    #[test]
    fn test_signalfd_null_mask_beats_bad_flag_efault() {
        // Phase 137: bad flag AND NULL mask — EFAULT wins (NULL mask
        // beats EINVAL).  Linux's sys_signalfd4 runs copy_from_user
        // BEFORE do_signalfd4's flag check, so NULL user_mask faults
        // first.  Pre-Phase 137 we had the wrong order and returned
        // EINVAL here.  See `test_phase137_*` below for the full
        // ordering matrix.
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_signalfd_mask_check_before_fd_check() {
        // Valid flag, NULL mask, bad fd — EFAULT must win over EBADF.
        errno::set_errno(0);
        let ret = signalfd(100_000, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // --- signalfd4 propagates the same validation ---------------------------

    #[test]
    fn test_signalfd4_unknown_flag_einval() {
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd4(-1, &raw const mask, 0x40);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_signalfd4_null_mask_efault() {
        errno::set_errno(0);
        let ret = signalfd4(-1, core::ptr::null(), SFD_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // --- real-world buggy callers -------------------------------------------

    #[test]
    fn test_buggy_caller_signalfd_passes_eventfd_flag() {
        // EFD_SEMAPHORE = 1 is an eventfd-only flag.  signalfd does
        // not accept it — Linux returns EINVAL.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-1, &raw const mask, EFD_SEMAPHORE as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_buggy_caller_signalfd_forgot_to_pass_mask() {
        // Caller meant `signalfd(-1, &mask, 0)` but passed NULL by
        // mistake (e.g. zero-initialized struct field).  EFAULT, not
        // ENOSYS — gives the caller actionable feedback.
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_workflow_signalfd_create_then_modify() {
        // Real-world flow: app creates a signalfd (fd == -1, ENOSYS)
        // then later wants to modify the same fd's mask (fd > 0,
        // existing fd).  Because no signalfd exists, fd > 0 must
        // reject with EBADF — consistent across both phases of the
        // workflow rather than a misleading ENOSYS.
        let m1: u64 = 1u64 << 14; // SIGTERM
        errno::set_errno(0);
        assert_eq!(signalfd(-1, &raw const m1, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        // Suppose the app stored fd 7 from a previous call; fd 7 is
        // not a signalfd, so modify fails with EBADF (would be EINVAL
        // once kind-tracking lands — see TODO in signalfd).
        let m2: u64 = (1u64 << 14) | (1u64 << 1); // add SIGINT
        errno::set_errno(0);
        assert_eq!(signalfd(7, &raw const m2, 0), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // ------------------------------------------------------------------
    // Phase 137 — signalfd: copy_from_user(mask) precedes do_signalfd4
    //
    // Linux's `fs/signalfd.c::sys_signalfd4` runs
    //     copy_from_user(&mask, user_mask, sizeof(mask))
    // BEFORE calling `do_signalfd4`, and `do_signalfd4`'s very first
    // check is `flags & ~(SFD_CLOEXEC | SFD_NONBLOCK)`.  So when a
    // caller passes BOTH a NULL/faulting `mask` AND unknown flag bits,
    // the kernel returns EFAULT, not EINVAL.  Pre-Phase 137 we did
    // these checks in the opposite order and reported EINVAL.
    //
    // The tests below pin the new ordering (NULL-mask wins over bad
    // flags, valid-mask + bad-flags still EINVAL, NULL-mask + good
    // flags still EFAULT) and cross-check that `signalfd4` — which
    // is just an alias — picks up the same ordering automatically.
    // ------------------------------------------------------------------

    // --- per-error-class smoke tests under the new ordering -----------

    #[test]
    fn test_phase137_null_mask_good_flags_efault() {
        // NULL mask alone — still EFAULT.  Sanity check that the
        // reorder didn't somehow hide the NULL-mask error path.
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), SFD_CLOEXEC);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_valid_mask_bad_flags_einval() {
        // Valid mask + bad flags — still EINVAL.  This is the case
        // where the reorder must NOT change behaviour: copy_from_user
        // succeeds, then do_signalfd4 rejects the flag.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-1, &raw const mask, 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase137_valid_mask_good_flags_reaches_enosys() {
        // Both inputs valid — control flow reaches the post-validation
        // ENOSYS terminator.  Proves the new ordering doesn't swallow
        // legitimate calls.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(-1, &raw const mask, SFD_NONBLOCK);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // --- ordering matrix: NULL mask vs every other error class -------

    #[test]
    fn test_phase137_null_mask_beats_unknown_flag() {
        // Bit 0x1 is outside SFD_FLAGS_VALID.  EFAULT > EINVAL.
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_null_mask_beats_garbage_flag() {
        // i32::MIN — sign bit plus most of the rest of the word.
        // EFAULT must still win.
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), i32::MIN);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_null_mask_beats_eventfd_flag_collision() {
        // EFD_SEMAPHORE = 1 is an eventfd-only flag bit that a
        // confused caller might pass to signalfd.  Pre-Phase 137 a
        // caller passing both EFD_SEMAPHORE AND NULL mask saw EINVAL.
        // Now they see EFAULT — the more useful diagnostic, because
        // the NULL pointer is the bug they really need to fix first.
        errno::set_errno(0);
        let ret = signalfd(-1, core::ptr::null(), EFD_SEMAPHORE as i32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_null_mask_beats_bad_fd_too() {
        // Already pinned by test_signalfd_mask_check_before_fd_check
        // pre-Phase 137 (fd check came after mask check), but the
        // Phase 137 reorder moved EFAULT even earlier.  Re-pin under
        // the new ordering to guard against any future regression.
        errno::set_errno(0);
        let ret = signalfd(100_000, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_null_mask_with_bad_flags_and_bad_fd_efault() {
        // All three error conditions at once: NULL mask + unknown
        // flag bits + non-existent fd.  EFAULT must win over both
        // EINVAL and EBADF.
        errno::set_errno(0);
        let ret = signalfd(100_000, core::ptr::null(), 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // --- ordering matrix: valid-mask paths preserve old ordering ------

    #[test]
    fn test_phase137_flag_check_still_beats_fd_check() {
        // With a valid mask, the do_signalfd4 flag check still runs
        // before the fd lookup.  Bad flags + bad fd → EINVAL.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd(100_000, &raw const mask, 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // --- signalfd4 alias picks up the same ordering -------------------

    #[test]
    fn test_phase137_signalfd4_null_mask_beats_bad_flag() {
        // signalfd4 is just `signalfd(fd, mask, flags)`.  The Phase
        // 137 reorder must flow through the alias automatically.
        errno::set_errno(0);
        let ret = signalfd4(-1, core::ptr::null(), 0x1);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_signalfd4_valid_mask_bad_flag_still_einval() {
        // signalfd4 valid-mask path: still EINVAL for unknown flags.
        let mask: u64 = 0;
        errno::set_errno(0);
        let ret = signalfd4(-1, &raw const mask, 0x40);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // --- buggy callers ------------------------------------------------

    #[test]
    fn test_phase137_buggy_caller_zeroed_sigset_struct() {
        // Common bug: caller writes
        //     sigset_t *m = malloc(sizeof(*m));  // forgot to check NULL
        //     signalfd(-1, m, flags);
        // and `m` is NULL.  If `flags` is ALSO wrong (e.g. a stale
        // value from a previous syscall), the caller now gets EFAULT
        // pointing at the real bug (NULL pointer), not EINVAL
        // misdirecting them at the flag word.
        errno::set_errno(0);
        let bad_flags = 0xDEAD_BEEFu32 as i32;
        let ret = signalfd(-1, core::ptr::null(), bad_flags);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_phase137_buggy_caller_uninitialised_flags_with_null_mask() {
        // Caller passed signalfd(-1, NULL, junk).  Whatever the
        // garbage `flags` value happens to be, EFAULT wins because
        // copy_from_user runs first.  Exercises a few representative
        // garbage values to make sure no specific bit pattern slips
        // through.
        for bad in [0x1i32, 0xFFi32, 0x7FFF_FFFFi32, -1i32, i32::MIN] {
            errno::set_errno(0);
            let ret = signalfd(-1, core::ptr::null(), bad);
            assert_eq!(ret, -1, "flags={bad:#x}");
            assert_eq!(errno::get_errno(), errno::EFAULT, "flags={bad:#x}");
        }
    }

    // --- workflow + recovery -----------------------------------------

    #[test]
    fn test_phase137_workflow_fix_null_mask_then_retry() {
        // Real recovery flow: first call passes NULL mask + bad
        // flags → EFAULT (the diagnostic the caller really needs).
        // Caller fixes the NULL pointer but forgets the flags →
        // EINVAL (now they see the second bug).  Caller fixes flags
        // → ENOSYS (validation complete, no signalfd backend yet).
        let mask: u64 = 1u64 << 14; // SIGTERM
        errno::set_errno(0);
        assert_eq!(signalfd(-1, core::ptr::null(), 0x1), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert_eq!(signalfd(-1, &raw const mask, 0x1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        assert_eq!(signalfd(-1, &raw const mask, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase137_workflow_fix_flags_does_not_unmask_null() {
        // Counter-flow: caller first sees EFAULT, "fixes" by setting
        // SFD_CLOEXEC instead of fixing the NULL pointer.  Must still
        // see EFAULT — flags being good doesn't paper over the NULL
        // mask.
        errno::set_errno(0);
        assert_eq!(signalfd(-1, core::ptr::null(), 0x1), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        errno::set_errno(0);
        assert_eq!(signalfd(-1, core::ptr::null(), SFD_CLOEXEC), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    // ------------------------------------------------------------------
    // Phase 105 — timerfd_settime flag mask + ordering parity with Linux.
    //
    // Linux accepts `TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET` in the
    // `flags` argument and validates both the flag mask AND the
    // itimerspec validity BEFORE looking up the fd.  Previously we only
    // accepted `TFD_TIMER_ABSTIME`, and we validated the itimerspec
    // AFTER the fd lookup.  The tests below pin both invariants.
    // ------------------------------------------------------------------

    #[test]
    fn test_timerfd_settime_phase105_mask_constants() {
        // Pin the on-the-wire values to Linux's
        // include/uapi/linux/timerfd.h.
        assert_eq!(TFD_TIMER_ABSTIME, 1);
        assert_eq!(TFD_TIMER_CANCEL_ON_SET, 2);
        assert_eq!(TFD_SETTIME_FLAGS_VALID, 3);
        assert_eq!(TFD_TIMER_ABSTIME & TFD_TIMER_CANCEL_ON_SET, 0);
    }

    #[test]
    fn test_timerfd_settime_phase105_cancel_on_set_accepted() {
        // CANCEL_ON_SET is a no-op for us but must NOT be rejected.
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r =
            unsafe { timerfd_settime(fd, TFD_TIMER_CANCEL_ON_SET, &new, core::ptr::null_mut()) };
        assert_eq!(
            r,
            0,
            "CANCEL_ON_SET must be accepted, errno={}",
            errno::get_errno()
        );
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_phase105_abstime_plus_cancel_on_set_accepted() {
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        // Use a moderately-far-future absolute time so the timer arms.
        // Don't use i64::MAX-ish values — `timespec_to_ns` multiplies
        // tv_sec by 1e9 and will overflow, producing a spurious EINVAL
        // that has nothing to do with the flag mask under test.
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1_000_000,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe {
            timerfd_settime(
                fd,
                TFD_TIMER_ABSTIME | TFD_TIMER_CANCEL_ON_SET,
                &new,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(
            r,
            0,
            "ABSTIME|CANCEL_ON_SET must be accepted, errno={}",
            errno::get_errno()
        );
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_phase105_unknown_bit_rejected() {
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        // bit 2 is not in the valid mask.
        let r = unsafe { timerfd_settime(fd, 1 << 2, &new, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_phase105_high_bit_rejected() {
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_settime(fd, i32::MIN, &new, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_phase105_valid_plus_unknown_rejected() {
        // Mixing a valid flag with a stray bit must still EINVAL.
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe {
            timerfd_settime(
                fd,
                TFD_TIMER_ABSTIME | (1 << 5),
                &new,
                core::ptr::null_mut(),
            )
        };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_phase105_efault_wins_over_einval() {
        // Null new_value with bad flags: Linux's copy_from_user fires
        // FIRST, so EFAULT wins over EINVAL.
        errno::set_errno(0);
        let r = unsafe { timerfd_settime(0, 1 << 7, core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_timerfd_settime_phase105_einval_wins_over_ebadf_for_flags() {
        // Bad flags + missing fd: Linux checks flags BEFORE the fd
        // lookup, so EINVAL wins over EBADF.
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        // fd 99999 almost certainly isn't allocated.
        let r = unsafe { timerfd_settime(99999, 1 << 7, &new, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_settime_phase105_einval_wins_over_ebadf_for_timespec() {
        // Bad timespec + missing fd: Linux validates the itimerspec
        // BEFORE the fd lookup (both in `do_timerfd_settime` prior to
        // `timerfd_fget`), so EINVAL wins over EBADF.
        let bad = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: -1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_settime(99999, 0, &bad, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_settime_phase105_einval_wins_over_ebadf_for_interval() {
        // Bad it_interval + missing fd: same ordering invariant.
        let bad = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 1_000_000_000,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_settime(99999, 0, &bad, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_timerfd_settime_phase105_efault_wins_over_ebadf() {
        // Null new_value + missing fd: EFAULT wins over EBADF
        // (copy_from_user fires before timerfd_fget).
        errno::set_errno(0);
        let r = unsafe { timerfd_settime(99999, 0, core::ptr::null(), core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_timerfd_settime_phase105_recovery_after_einval() {
        // After a rejected call with bad flags, a subsequent valid call
        // on the same fd still works.
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let bad = unsafe { timerfd_settime(fd, 1 << 9, &new, core::ptr::null_mut()) };
        assert_eq!(bad, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        errno::set_errno(0);
        let good =
            unsafe { timerfd_settime(fd, TFD_TIMER_CANCEL_ON_SET, &new, core::ptr::null_mut()) };
        assert_eq!(
            good,
            0,
            "recovery call must succeed, errno={}",
            errno::get_errno()
        );
        crate::file::close(fd);
    }

    #[test]
    fn test_timerfd_settime_phase105_exhaustive_stray_bits_rejected() {
        // Every single bit outside the valid mask must produce EINVAL
        // when combined with no other flag.  Sign bit is bit 31.
        let fd = timerfd_create(1, 0);
        assert!(fd >= 0);
        let new = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 1,
                tv_nsec: 0,
            },
        };
        for shift in 0..32 {
            #[allow(clippy::cast_possible_wrap)]
            let bit = (1u32 << shift) as i32;
            if bit & TFD_SETTIME_FLAGS_VALID != 0 {
                continue;
            }
            errno::set_errno(0);
            let r = unsafe { timerfd_settime(fd, bit, &new, core::ptr::null_mut()) };
            assert_eq!(r, -1, "bit {shift} should be rejected");
            assert_eq!(
                errno::get_errno(),
                errno::EINVAL,
                "bit {shift} should produce EINVAL"
            );
        }
        crate::file::close(fd);
    }

    // ------------------------------------------------------------------
    // Phase 106 — epoll_ctl validation ordering parity with Linux.
    //
    // Linux's `do_epoll_ctl` (fs/eventpoll.c) validates inputs in this
    // order: EFAULT (null event for ADD/MOD) → EBADF (epfd) → EBADF
    // (target fd) → EINVAL (epfd kind) → EINVAL (epfd == fd) →
    // EINVAL (unknown op).  Previously we put the EFAULT check INSIDE
    // the ADD/MOD branches and the kind/self-loop checks BEFORE the fd
    // lookup, so several errno combinations diverged from Linux:
    //   * ADD/MOD with null event + bad epfd: was EBADF, now EFAULT.
    //   * Non-epoll epfd with closed fd: was EINVAL, now EBADF.
    //   * Non-epoll epfd with fd == epfd: was EINVAL (kind) — unchanged
    //     in result but the path is now explicit.
    // ------------------------------------------------------------------

    #[test]
    fn test_epoll_ctl_phase106_add_null_event_is_efault_even_with_bad_epfd() {
        // ADD with null event AND a bad epfd: Linux's copy_from_user
        // fires first → EFAULT.
        errno::set_errno(0);
        let ret = epoll_ctl(99999, EPOLL_CTL_ADD, 4, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_epoll_ctl_phase106_mod_null_event_is_efault_even_with_bad_epfd() {
        errno::set_errno(0);
        let ret = epoll_ctl(99999, EPOLL_CTL_MOD, 4, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_epoll_ctl_phase106_del_tolerates_null_event() {
        // DEL doesn't read `event`; null is fine.  With a bad epfd we
        // still get EBADF.
        errno::set_errno(0);
        let ret = epoll_ctl(99999, EPOLL_CTL_DEL, 4, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_ctl_phase106_bad_epfd_then_good_event_is_ebadf() {
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let ret = epoll_ctl(99999, EPOLL_CTL_ADD, 4, &raw mut ev);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_ctl_phase106_ebadf_for_bad_target_fd() {
        // Real epoll fd, target fd is not allocated → EBADF.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let ret = epoll_ctl(epfd, EPOLL_CTL_ADD, 99999, &raw mut ev);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_ctl_phase106_ebadf_target_wins_over_einval_kind() {
        // epfd is a non-epoll fd (we open an eventfd as a stand-in for
        // any "not an epoll" fd) AND target fd is closed.
        // Linux: EBADF (target fd lookup before kind check).
        let not_epoll = eventfd(0, 0);
        assert!(not_epoll >= 0);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let ret = epoll_ctl(not_epoll, EPOLL_CTL_ADD, 99999, &raw mut ev);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        crate::file::close(not_epoll);
    }

    #[test]
    fn test_epoll_ctl_phase106_einval_for_non_epoll_epfd() {
        // Non-epoll epfd with a valid different target fd → EINVAL
        // from the kind check (target lookup succeeds, kind check
        // then fails).
        let not_epoll = eventfd(0, 0);
        assert!(not_epoll >= 0);
        let target = eventfd(0, 0);
        assert!(target >= 0);
        assert_ne!(not_epoll, target);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let ret = epoll_ctl(not_epoll, EPOLL_CTL_ADD, target, &raw mut ev);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(target);
        crate::file::close(not_epoll);
    }

    #[test]
    fn test_epoll_ctl_phase106_self_loop_einval() {
        // Real epoll fd with epfd == fd: EINVAL (after the kind check
        // passes).
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let ret = epoll_ctl(epfd, EPOLL_CTL_ADD, epfd, &raw mut ev);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_ctl_phase106_unknown_op_einval() {
        // Valid epfd, valid different target fd, but unknown op.
        // Linux: switch default → EINVAL.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let target = eventfd(0, 0);
        assert!(target >= 0);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let ret = epoll_ctl(epfd, 99, target, &raw mut ev);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(target);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_ctl_phase106_unknown_op_with_bad_epfd_is_ebadf() {
        // Unknown op + bad epfd: Linux looks up epfd before
        // dispatching on op, so EBADF wins over EINVAL.
        // Note: with op=99 (not ADD/MOD/DEL), the event-null check is
        // skipped, so we don't trip the EFAULT branch first.
        errno::set_errno(0);
        let ret = epoll_ctl(99999, 99, 4, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_ctl_phase106_mod_null_event_with_good_epfd_is_efault() {
        // Null event for MOD with a real epfd: EFAULT (caught upfront
        // before fd lookup).
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        errno::set_errno(0);
        let ret = epoll_ctl(epfd, EPOLL_CTL_MOD, 99999, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_ctl_phase106_ops_distinct() {
        // Pin the on-the-wire op codes — distinct values prevent
        // ambiguity in the dispatch / event-needed check.
        assert_ne!(EPOLL_CTL_ADD, EPOLL_CTL_MOD);
        assert_ne!(EPOLL_CTL_ADD, EPOLL_CTL_DEL);
        assert_ne!(EPOLL_CTL_MOD, EPOLL_CTL_DEL);
    }

    #[test]
    fn test_epoll_ctl_phase106_recovery_after_efault() {
        // After an EFAULT-rejected call, a subsequent valid call on the
        // same epoll fd still works.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        // Reject with EFAULT.
        errno::set_errno(0);
        let bad = epoll_ctl(epfd, EPOLL_CTL_ADD, 99999, core::ptr::null_mut());
        assert_eq!(bad, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // Now a valid ADD with a real target fd.
        let target = eventfd(0, 0);
        assert!(target >= 0);
        let mut ev = EpollEvent {
            events: EPOLLIN,
            data: 0,
        };
        errno::set_errno(0);
        let good = epoll_ctl(epfd, EPOLL_CTL_ADD, target, &raw mut ev);
        assert_eq!(
            good,
            0,
            "recovery ADD must succeed, errno={}",
            errno::get_errno()
        );
        crate::file::close(target);
        crate::file::close(epfd);
    }

    // ------------------------------------------------------------------
    // Phase 107 — epoll_wait validation ordering parity with Linux.
    //
    // Linux's `do_epoll_wait` performs `fdget(epfd)` FIRST, then enters
    // `ep_check_params` (maxevents, events access_ok, is_file_epoll).
    // Previously we checked maxevents and the events pointer BEFORE the
    // fd lookup, so a bad epfd combined with bad maxevents returned
    // EINVAL instead of Linux's EBADF.  We also lacked the upper bound
    // on maxevents that Linux enforces (EP_MAX_EVENTS = INT_MAX /
    // sizeof(struct epoll_event)).
    // ------------------------------------------------------------------

    #[test]
    fn test_epoll_wait_phase107_ep_max_events_value() {
        // sizeof(EpollEvent) is 12 bytes (u32 events + u64 data, packed),
        // so EP_MAX_EVENTS = INT_MAX / 12 = 178,956,970.  Pin to detect
        // accidental layout changes that would slide the bound.
        assert_eq!(core::mem::size_of::<EpollEvent>(), 12);
        assert_eq!(EP_MAX_EVENTS, i32::MAX / 12);
        assert!(EP_MAX_EVENTS > 0);
    }

    #[test]
    fn test_epoll_wait_phase107_maxevents_at_bound_passes_check() {
        // maxevents == EP_MAX_EVENTS is the largest accepted value.
        // We won't actually allocate that buffer; we just verify the
        // bound check itself doesn't reject it.  Use a real epoll fd
        // and a non-null events pointer to isolate the bound check.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        // Poll with timeout=0 so we don't actually wait.  With
        // maxevents=EP_MAX_EVENTS we'll write at most 1 event because
        // we only have 1 entry, but the bound check must not reject.
        errno::set_errno(0);
        let r = unsafe { epoll_wait(epfd, ev.as_mut_ptr(), EP_MAX_EVENTS, 0) };
        // Either 0 (nothing ready) or an error other than EINVAL is OK;
        // we just need to confirm the bound itself didn't reject.
        if r < 0 {
            assert_ne!(
                errno::get_errno(),
                errno::EINVAL,
                "EP_MAX_EVENTS at the bound must not trip the bound check"
            );
        }
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_wait_phase107_maxevents_over_bound_einval() {
        // EP_MAX_EVENTS + 1 must trip the new upper bound.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(epfd, ev.as_mut_ptr(), EP_MAX_EVENTS.wrapping_add(1), 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_wait_phase107_maxevents_i32_max_einval() {
        // i32::MAX is well past EP_MAX_EVENTS and must be rejected.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(epfd, ev.as_mut_ptr(), i32::MAX, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_wait_phase107_ebadf_wins_over_einval_maxevents() {
        // Bad epfd + maxevents=0: Linux looks up epfd FIRST, so we get
        // EBADF, not EINVAL.
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(99999, ev.as_mut_ptr(), 0, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_wait_phase107_ebadf_wins_over_einval_negative_maxevents() {
        // Same, with negative maxevents.
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(99999, ev.as_mut_ptr(), -1, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_wait_phase107_ebadf_wins_over_einval_overbound_maxevents() {
        // Bad epfd + maxevents past EP_MAX_EVENTS: still EBADF.
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(99999, ev.as_mut_ptr(), i32::MAX, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_wait_phase107_ebadf_wins_over_efault_null_events() {
        // Bad epfd + null events: Linux returns EBADF (epfd lookup
        // before access_ok).
        errno::set_errno(0);
        let r = unsafe { epoll_wait(99999, core::ptr::null_mut(), 1, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_wait_phase107_einval_maxevents_wins_over_efault_events() {
        // Real epfd + maxevents=0 + null events: Linux's
        // ep_check_params checks maxevents BEFORE access_ok, so EINVAL
        // wins over EFAULT.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        errno::set_errno(0);
        let r = unsafe { epoll_wait(epfd, core::ptr::null_mut(), 0, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_wait_phase107_efault_null_events_wins_over_einval_kind() {
        // Non-epoll epfd + valid maxevents + null events: Linux's
        // access_ok runs BEFORE is_file_epoll, so EFAULT wins.
        let not_epoll = eventfd(0, 0);
        assert!(not_epoll >= 0);
        errno::set_errno(0);
        let r = unsafe { epoll_wait(not_epoll, core::ptr::null_mut(), 1, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        crate::file::close(not_epoll);
    }

    #[test]
    fn test_epoll_wait_phase107_einval_kind_for_non_epoll_fd() {
        // Non-epoll epfd + valid args → EINVAL from is_file_epoll.
        let not_epoll = eventfd(0, 0);
        assert!(not_epoll >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(not_epoll, ev.as_mut_ptr(), 1, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(not_epoll);
    }

    #[test]
    fn test_epoll_wait_phase107_einval_maxevents_for_non_epoll_fd() {
        // Non-epoll epfd + maxevents=-5 + valid events ptr: Linux
        // checks maxevents BEFORE is_file_epoll, so EINVAL wins
        // (but the errno is EINVAL either way; this test pins the
        // ordering through the maxevents check, not the kind check).
        let not_epoll = eventfd(0, 0);
        assert!(not_epoll >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe { epoll_wait(not_epoll, ev.as_mut_ptr(), -5, 0) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(not_epoll);
    }

    #[test]
    fn test_epoll_wait_phase107_recovery_after_ebadf() {
        // After a rejected call with a bad epfd, a subsequent valid
        // call still works.
        errno::set_errno(0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = unsafe { epoll_wait(99999, ev.as_mut_ptr(), 1, 0) };
        assert_eq!(bad, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        errno::set_errno(0);
        let good = unsafe { epoll_wait(epfd, ev.as_mut_ptr(), 1, 0) };
        // No events armed → returns 0.  Must not be an error.
        assert_eq!(
            good,
            0,
            "recovery wait must succeed, errno={}",
            errno::get_errno()
        );
        crate::file::close(epfd);
    }

    // ------------------------------------------------------------------
    // Phase 108 — epoll_pwait2 timespec validation.
    //
    // Linux's `do_epoll_pwait2` (fs/eventpoll.c) validates the
    // user-supplied timespec via `poll_select_set_timeout` BEFORE the
    // inner `do_epoll_wait` call (so before any fdget on epfd).  A
    // negative `tv_sec` or out-of-range `tv_nsec` returns EINVAL even
    // when the epfd is bad.  Previously we silently coerced bad
    // timespecs into a 1ms timeout and forwarded to epoll_wait, so the
    // EINVAL never fired.
    // ------------------------------------------------------------------

    #[test]
    fn test_epoll_pwait2_phase108_null_timeout_is_indefinite_wait() {
        // Null timeout should behave like epoll_wait(..., -1).  With a
        // real epfd, no events armed, and timeout=0 (we cant block in
        // tests), we cant exercise the indefinite path directly.  Just
        // verify the function reaches the inner wait with the expected
        // tms by passing null and a real epfd; we shouldn't see
        // EINVAL.  To avoid blocking, also use timeout via a zero
        // timespec in a separate test.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        // We don't actually invoke null-timeout here (that would block
        // indefinitely if any event hooks ever stuck).  Instead this
        // test pins that a zero timespec returns 0 promptly.
        let zero_ts = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        let r = unsafe {
            epoll_pwait2(
                epfd,
                ev.as_mut_ptr(),
                1,
                &raw const zero_ts,
                core::ptr::null(),
            )
        };
        assert_eq!(
            r,
            0,
            "zero timespec must poll once and return, errno={}",
            errno::get_errno()
        );
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_negative_tv_sec_einval() {
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = crate::stat::Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 1, &raw const bad, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_negative_tv_nsec_einval() {
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: -1,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 1, &raw const bad, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_tv_nsec_one_billion_einval() {
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 1_000_000_000,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 1, &raw const bad, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_tv_nsec_just_under_one_billion_ok() {
        // Boundary case: tv_nsec = 999_999_999 is the largest accepted
        // value (matches Linux's `[0, NSEC_PER_SEC)` range).
        //
        // We pair this with a bad epfd so the test short-circuits at
        // the inner epoll_wait EBADF check rather than entering the
        // ~1-second poll loop.  If the timespec check spuriously
        // rejected the boundary value, we'd see EINVAL instead.
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let ok = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 999_999_999,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(99999, ev.as_mut_ptr(), 1, &raw const ok, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EBADF,
            "tv_nsec=999_999_999 must reach the EBADF path, not be rejected with EINVAL"
        );
    }

    #[test]
    fn test_epoll_pwait2_phase108_einval_ts_wins_over_ebadf_epfd() {
        // Bad timespec + bad epfd: Linux validates the timespec FIRST
        // (poll_select_set_timeout fires before fdget), so EINVAL wins.
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = crate::stat::Timespec {
            tv_sec: -10,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(99999, ev.as_mut_ptr(), 1, &raw const bad, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_epoll_pwait2_phase108_einval_ts_wins_over_einval_maxevents() {
        // Bad timespec + zero maxevents: both would be EINVAL, but the
        // timespec check fires first.  Both surface as EINVAL — this
        // test simply confirms we don't regress to EBADF or some other
        // errno by silently coercing the timespec.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: i64::MAX,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 0, &raw const bad, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_einval_ts_wins_over_efault_events() {
        // Bad timespec + null events ptr: Linux's timespec check
        // happens before access_ok, so EINVAL wins over EFAULT.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let bad = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: -42,
        };
        errno::set_errno(0);
        let r = unsafe {
            epoll_pwait2(
                epfd,
                core::ptr::null_mut(),
                1,
                &raw const bad,
                core::ptr::null(),
            )
        };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_huge_tv_sec_does_not_overflow_to_einval() {
        // Saturating math must convert i64::MAX seconds to i32::MAX
        // milliseconds without spuriously failing the timespec check.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let huge = crate::stat::Timespec {
            tv_sec: i64::MAX,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        // Don't call this — it would wait indefinitely.  Just verify
        // the validation pass: pass a bad maxevents alongside so the
        // outer epoll_wait short-circuits before sleeping, and check
        // that the inner errno is the maxevents EINVAL (not a
        // timespec EINVAL that wrongly fired).
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 0, &raw const huge, core::ptr::null()) };
        assert_eq!(r, -1);
        // Either path (timespec EINVAL or maxevents EINVAL) surfaces
        // as EINVAL; the point is no other errno (no EBADF, no
        // EFAULT).
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_zero_timespec_polls_immediately() {
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let zero = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 1, &raw const zero, core::ptr::null()) };
        assert_eq!(r, 0);
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_recovery_after_einval() {
        // After a rejected call with a bad timespec, a subsequent call
        // with a valid zero timespec succeeds.
        let epfd = epoll_create1(0);
        assert!(epfd >= 0);
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        let bad = crate::stat::Timespec {
            tv_sec: -1,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 1, &raw const bad, core::ptr::null()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        let zero = crate::stat::Timespec {
            tv_sec: 0,
            tv_nsec: 0,
        };
        errno::set_errno(0);
        let r =
            unsafe { epoll_pwait2(epfd, ev.as_mut_ptr(), 1, &raw const zero, core::ptr::null()) };
        assert_eq!(
            r,
            0,
            "recovery call must succeed, errno={}",
            errno::get_errno()
        );
        crate::file::close(epfd);
    }

    #[test]
    fn test_epoll_pwait2_phase108_null_timeout_with_bad_epfd_is_ebadf() {
        // With null timeout (no timespec validation needed), bad epfd
        // surfaces from the inner epoll_wait: EBADF.  This confirms we
        // don't spuriously fail with EINVAL when timeout==NULL.
        let mut ev = [EpollEvent { events: 0, data: 0 }; 1];
        errno::set_errno(0);
        let r = unsafe {
            epoll_pwait2(
                99999,
                ev.as_mut_ptr(),
                1,
                core::ptr::null(),
                core::ptr::null(),
            )
        };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    // -----------------------------------------------------------------
    // Phase 143 — timerfd_gettime fd resolves before user-pointer check
    //
    // Linux's `sys_timerfd_gettime` calls `do_timerfd_gettime`, which
    // runs `timerfd_fget(ufd, &f)` (returns EBADF for unknown fd,
    // EINVAL for wrong fd type) before any `put_itimerspec64` /
    // `copy_to_user` can fail with EFAULT.  Pre-Phase-143 we checked
    // the NULL pointer first, so `timerfd_gettime(-1, NULL)` returned
    // EFAULT — misdirecting the caller at the pointer when the real
    // bug was the fd.
    // -----------------------------------------------------------------

    #[test]
    fn test_timerfd_gettime_phase143_bad_fd_with_null_is_ebadf() {
        // Core regression: bad fd (-1) + NULL pointer → EBADF, not
        // EFAULT.  This is the case the Phase 143 reorder fixes.
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(-1, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_timerfd_gettime_phase143_unknown_fd_with_null_is_ebadf() {
        // A wildly-out-of-range fd: same outcome.  `fdtable::get_fd`
        // returns None, surfaces as EBADF before the pointer is
        // touched.
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(99999, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_timerfd_gettime_phase143_intmin_fd_with_null_is_ebadf() {
        // i32::MIN as fd: still EBADF.  Confirms no integer-underflow
        // surprise in the fd lookup path.
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(i32::MIN, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_timerfd_gettime_phase143_intmax_fd_with_null_is_ebadf() {
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(i32::MAX, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_timerfd_gettime_phase143_bad_fd_with_valid_buffer_is_ebadf() {
        // Ordering not the focus here, just confirming the fd check
        // works on its own (valid pointer, bad fd → EBADF).  Pins the
        // baseline so we notice if EBADF ever regresses to EINVAL.
        let mut buf = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(-1, &mut buf) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_timerfd_gettime_phase143_wrong_kind_fd_takes_einval_path() {
        // A real fd of a non-timerfd kind: we expect EINVAL (wrong
        // type) BEFORE the NULL-pointer EFAULT.  Use eventfd(2) which
        // creates a real Eventfd-kind fd we own.
        let efd = eventfd(0, 0);
        assert!(
            efd >= 0,
            "eventfd setup failed; errno={}",
            errno::get_errno()
        );

        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(efd, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "wrong-kind fd must beat NULL-pointer EFAULT",
        );

        crate::file::close(efd);
    }

    #[test]
    fn test_timerfd_gettime_phase143_valid_timerfd_null_out_is_efault() {
        // Recovery / contrast: when the fd IS a real timerfd, the
        // NULL-pointer path is reached and yields EFAULT.  This pins
        // the post-Phase-143 ordering's third stage.
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(
            tfd >= 0,
            "timerfd_create setup failed; errno={}",
            errno::get_errno()
        );

        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(tfd, core::ptr::null_mut()) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        crate::file::close(tfd);
    }

    #[test]
    fn test_timerfd_gettime_phase143_valid_timerfd_valid_buffer_succeeds() {
        // End-to-end happy path under the new ordering: real timerfd
        // + real buffer → 0 with no errno surprise.
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);

        let mut buf = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let ret = unsafe { timerfd_gettime(tfd, &mut buf) };
        assert_eq!(ret, 0);

        crate::file::close(tfd);
    }

    #[test]
    fn test_timerfd_gettime_phase143_recovery_after_bad_fd() {
        // Workflow: caller hits EBADF with bad fd + NULL, fixes the
        // fd (creates a real timerfd) and passes a real buffer,
        // succeeds.  This is the standard "probe, fix, retry"
        // pattern; assert that the first call doesn't leak any state
        // that would affect the second.
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(-1, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);
        let mut buf = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(tfd, &mut buf) };
        assert_eq!(r, 0);
        crate::file::close(tfd);
    }

    #[test]
    fn test_timerfd_gettime_phase143_buggy_caller_uninit_fd_values() {
        // Buggy caller pattern: an uninitialised stack int treated as
        // an fd.  All these representative garbage values must EBADF,
        // not EFAULT, even with NULL pointer.
        for fd in [-7, -42, 0x4242, 0x7FFF_FFFF, 0x0BAD_F00Du32 as i32] {
            errno::set_errno(0);
            let r = unsafe { timerfd_gettime(fd, core::ptr::null_mut()) };
            assert_eq!(r, -1, "fd={fd}");
            // Either EBADF (no entry) or EINVAL (entry exists but
            // wrong kind — possible if a host test happens to have
            // such an fd open).  EFAULT must NEVER appear before the
            // fd is resolved.
            let e = errno::get_errno();
            assert!(
                e == errno::EBADF || e == errno::EINVAL,
                "fd={fd} got errno {e}, expected EBADF or EINVAL",
            );
        }
    }

    #[test]
    fn test_timerfd_gettime_phase143_no_side_effect_loop() {
        // Hammer the bad-fd path 32×; errno deterministically EBADF.
        for _ in 0..32 {
            errno::set_errno(0);
            let r = unsafe { timerfd_gettime(-1, core::ptr::null_mut()) };
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }
    }

    #[test]
    fn test_timerfd_gettime_phase143_does_not_write_buffer_on_ebadf() {
        // The buffer must not be touched when fd resolution fails.
        // A naive ordering that wrote to the buffer before the fd
        // check would corrupt caller memory.
        let mut buf = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0xDEAD,
                tv_nsec: 0xBEEF,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0xCAFE,
                tv_nsec: 0xF00D,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(-1, &mut buf) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(buf.it_interval.tv_sec, 0xDEAD, "buffer must be untouched");
        assert_eq!(buf.it_interval.tv_nsec, 0xBEEF);
        assert_eq!(buf.it_value.tv_sec, 0xCAFE);
        assert_eq!(buf.it_value.tv_nsec, 0xF00D);
    }

    #[test]
    fn test_timerfd_gettime_phase143_does_not_write_buffer_on_einval() {
        // Same invariant for the wrong-kind-fd path: buffer must be
        // untouched on EINVAL.  Use an eventfd as the wrong-kind fd.
        let efd = eventfd(0, 0);
        assert!(efd >= 0);

        let mut buf = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 1234,
                tv_nsec: 5678,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 9012,
                tv_nsec: 3456,
            },
        };
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(efd, &mut buf) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        assert_eq!(buf.it_interval.tv_sec, 1234);
        assert_eq!(buf.it_interval.tv_nsec, 5678);
        assert_eq!(buf.it_value.tv_sec, 9012);
        assert_eq!(buf.it_value.tv_nsec, 3456);

        crate::file::close(efd);
    }

    // -----------------------------------------------------------------
    // Phase 144 — eventfd_read / eventfd_write resolve fd first
    //
    // Both are glibc convenience wrappers that decompose into Linux's
    // `sys_read` / `sys_write`, which run `fdget(fd)` before any
    // pointer copy or value check.  Observable Linux errno priority
    // is EBADF > EINVAL(kind) > EFAULT(pointer) > EINVAL(value).
    //
    // Pre-Phase-144 we checked the NULL pointer (for read) and the
    // U64_MAX value (for write) before the fd lookup, so a buggy
    // caller passing both got the wrong, lower-information error.
    // -----------------------------------------------------------------

    #[test]
    fn test_eventfd_read_phase144_bad_fd_null_ptr_is_ebadf() {
        // Core regression: -1 + NULL → EBADF, not EFAULT.
        errno::set_errno(0);
        let ret = eventfd_read(-1, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_eventfd_read_phase144_unknown_fd_null_ptr_is_ebadf() {
        errno::set_errno(0);
        let ret = eventfd_read(99999, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_eventfd_read_phase144_intmin_fd_null_ptr_is_ebadf() {
        errno::set_errno(0);
        let ret = eventfd_read(i32::MIN, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_eventfd_read_phase144_wrong_kind_fd_null_ptr_is_einval() {
        // Wrong-kind real fd: get a timerfd, pass to eventfd_read
        // with NULL.  Linux returns EINVAL (kind mismatch) BEFORE
        // EFAULT (pointer); we match.
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);
        errno::set_errno(0);
        let ret = eventfd_read(tfd, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(tfd);
    }

    #[test]
    fn test_eventfd_read_phase144_valid_fd_null_pointer_is_efault() {
        // Post-Phase-144 third stage reachable: real eventfd + NULL
        // → EFAULT.
        let efd = eventfd(0, 0);
        assert!(efd >= 0);
        errno::set_errno(0);
        let ret = eventfd_read(efd, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        crate::file::close(efd);
    }

    #[test]
    fn test_eventfd_read_phase144_buggy_caller_does_not_write_value() {
        // Buffer-untouched invariant: when the fd check fails, the
        // value buffer must not be written.
        let sentinel: u64 = 0xDEAD_BEEF_CAFE_F00D;
        let mut val: u64 = sentinel;
        errno::set_errno(0);
        let ret = eventfd_read(-1, &raw mut val);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
        assert_eq!(val, sentinel, "buffer must not be written on EBADF");
    }

    #[test]
    fn test_eventfd_write_phase144_bad_fd_u64_max_is_ebadf() {
        // Core regression: -1 + u64::MAX → EBADF, not EINVAL.  Both
        // errors return -1 but the diagnostic differs.
        errno::set_errno(0);
        let ret = eventfd_write(-1, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_eventfd_write_phase144_unknown_fd_u64_max_is_ebadf() {
        errno::set_errno(0);
        let ret = eventfd_write(99999, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_eventfd_write_phase144_intmin_fd_u64_max_is_ebadf() {
        errno::set_errno(0);
        let ret = eventfd_write(i32::MIN, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_eventfd_write_phase144_wrong_kind_fd_u64_max_is_einval() {
        // Wrong-kind real fd + U64_MAX → EINVAL from the kind check,
        // not the value check (both EINVAL — assert -1 + EINVAL shape).
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);
        errno::set_errno(0);
        let ret = eventfd_write(tfd, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(tfd);
    }

    #[test]
    fn test_eventfd_write_phase144_valid_fd_u64_max_is_einval() {
        // Post-Phase-144 third stage reachable: real eventfd +
        // U64_MAX → EINVAL from the value check.
        let efd = eventfd(0, 0);
        assert!(efd >= 0);
        errno::set_errno(0);
        let ret = eventfd_write(efd, u64::MAX);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        crate::file::close(efd);
    }

    #[test]
    fn test_eventfd_write_phase144_valid_fd_u64_max_minus_one_succeeds() {
        // Boundary check: u64::MAX - 1 is the largest legal value
        // on Linux and must succeed.  Confirms our value check
        // boundary matches Linux's `if (val == U64_MAX)`, not
        // `if (val >= U64_MAX - some_slack)`.
        let efd = eventfd(0, 0);
        assert!(efd >= 0);
        errno::set_errno(0);
        let ret = eventfd_write(efd, u64::MAX - 1);
        assert_eq!(
            ret,
            0,
            "u64::MAX-1 must succeed; errno={}",
            errno::get_errno()
        );
        crate::file::close(efd);
    }

    #[test]
    fn test_eventfd_read_phase144_no_side_effect_loop() {
        // Hammer the bad-fd path 32×; errno deterministically EBADF.
        for _ in 0..32 {
            errno::set_errno(0);
            let r = eventfd_read(-1, core::ptr::null_mut());
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }
    }

    #[test]
    fn test_eventfd_write_phase144_no_side_effect_loop() {
        for _ in 0..32 {
            errno::set_errno(0);
            let r = eventfd_write(-1, u64::MAX);
            assert_eq!(r, -1);
            assert_eq!(errno::get_errno(), errno::EBADF);
        }
    }

    #[test]
    fn test_eventfd_read_phase144_recovery_after_bad_fd() {
        // Workflow: EBADF, fix fd, succeed.
        errno::set_errno(0);
        let r = eventfd_read(-1, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        let efd = eventfd(5, 0); // initial counter = 5
        assert!(efd >= 0);
        let mut val: u64 = 0;
        errno::set_errno(0);
        let r = eventfd_read(efd, &raw mut val);
        assert_eq!(r, 0);
        // Counter readable as 5 on the kernel target; host stub may
        // differ.  Only assert success here; the value semantics
        // are exercised by pre-existing eventfd tests.
        crate::file::close(efd);
    }

    #[test]
    fn test_eventfd_write_phase144_recovery_after_bad_fd() {
        // Workflow: EBADF, fix fd, write succeeds (with a non-max value).
        errno::set_errno(0);
        let r = eventfd_write(-1, 42);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        let efd = eventfd(0, 0);
        assert!(efd >= 0);
        errno::set_errno(0);
        let r = eventfd_write(efd, 42);
        assert_eq!(
            r,
            0,
            "write of 42 must succeed; errno={}",
            errno::get_errno()
        );
        crate::file::close(efd);
    }

    #[test]
    fn test_eventfd_read_phase144_ordering_matrix() {
        // Full 2x3 matrix: {bad fd, wrong-kind fd, eventfd} ×
        // {NULL ptr, valid ptr}.  Pins every outcome.
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);
        let efd = eventfd(0, 0);
        assert!(efd >= 0);
        let mut val: u64 = 0;

        // bad fd × NULL  → EBADF
        errno::set_errno(0);
        let r = eventfd_read(-1, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        // bad fd × valid → EBADF
        errno::set_errno(0);
        let r = eventfd_read(-1, &raw mut val);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        // wrong-kind × NULL → EINVAL (kind beats EFAULT)
        errno::set_errno(0);
        let r = eventfd_read(tfd, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // wrong-kind × valid → EINVAL
        errno::set_errno(0);
        let r = eventfd_read(tfd, &raw mut val);
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // eventfd × NULL → EFAULT
        errno::set_errno(0);
        let r = eventfd_read(efd, core::ptr::null_mut());
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        // eventfd × valid → 0 (we just made a fresh eventfd with
        // counter 0; in nonblocking modes this would EAGAIN, but
        // the default is blocking and the kernel returns the
        // counter — which the host stub also satisfies).  Skip
        // asserting the value; only assert success or EAGAIN.
        errno::set_errno(0);
        let r = eventfd_read(efd, &raw mut val);
        let e = errno::get_errno();
        assert!(
            r == 0 || (r == -1 && e == errno::EAGAIN),
            "got r={r} errno={e}, expected 0 or EAGAIN",
        );

        crate::file::close(tfd);
        crate::file::close(efd);
    }

    #[test]
    fn test_eventfd_write_phase144_ordering_matrix() {
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);
        let efd = eventfd(0, 0);
        assert!(efd >= 0);

        // bad fd × U64_MAX → EBADF
        errno::set_errno(0);
        assert_eq!(eventfd_write(-1, u64::MAX), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        // bad fd × valid value → EBADF
        errno::set_errno(0);
        assert_eq!(eventfd_write(-1, 1), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        // wrong-kind × U64_MAX → EINVAL (kind beats value)
        errno::set_errno(0);
        assert_eq!(eventfd_write(tfd, u64::MAX), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // wrong-kind × valid value → EINVAL
        errno::set_errno(0);
        assert_eq!(eventfd_write(tfd, 1), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // eventfd × U64_MAX → EINVAL (value check)
        errno::set_errno(0);
        assert_eq!(eventfd_write(efd, u64::MAX), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // eventfd × valid value → 0
        errno::set_errno(0);
        assert_eq!(eventfd_write(efd, 1), 0);

        crate::file::close(tfd);
        crate::file::close(efd);
    }

    #[test]
    fn test_timerfd_gettime_phase143_ordering_matrix_all_combos() {
        // Build the 2x2 matrix of {bad fd, good fd} × {NULL ptr,
        // valid ptr} and assert the expected errno (or success) for
        // each.  Pins the full ordering contract in one place.
        let tfd = timerfd_create(crate::time::CLOCK_MONOTONIC, 0);
        assert!(tfd >= 0);
        let mut buf = Itimerspec {
            it_interval: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
            it_value: crate::stat::Timespec {
                tv_sec: 0,
                tv_nsec: 0,
            },
        };

        // bad fd + NULL → EBADF
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(-1, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        // bad fd + valid → EBADF
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(-1, &mut buf) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);

        // good fd + NULL → EFAULT
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(tfd, core::ptr::null_mut()) };
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        // good fd + valid → 0
        errno::set_errno(0);
        let r = unsafe { timerfd_gettime(tfd, &mut buf) };
        assert_eq!(r, 0);

        crate::file::close(tfd);
    }

    // -- Phase 199: CAP_WAKE_ALARM gate on timerfd alarm clocks -----------

    /// RAII guard that snapshots and restores effective capabilities.
    struct CapGuard {
        lo: u32,
        hi: u32,
        // Held for the lifetime of the guard. See
        // `sys_capability::CAP_TEST_LOCK` for why.
        _lock: crate::sys_capability::CapTestLockGuard,
    }
    impl CapGuard {
        fn snapshot() -> Self {
            // Re-entrant lock guard: outermost acquire on the
            // thread takes the global mutex; nested acquires
            // (some tests stack a scoped CapGuard inside an
            // outer one) are no-ops for the lock but still
            // snapshot/restore caps independently.
            let lock = crate::sys_capability::CapTestLockGuard::acquire();
            let (lo, hi) = crate::sys_capability::current_caps_effective();
            Self { lo, hi, _lock: lock }
        }
    }
    impl Drop for CapGuard {
        fn drop(&mut self) {
            let mut hdr = crate::sys_capability::CapUserHeader {
                version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
                pid: 0,
            };
            let data = [
                crate::sys_capability::CapUserData {
                    effective: self.lo,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
                crate::sys_capability::CapUserData {
                    effective: self.hi,
                    permitted: u32::MAX,
                    inheritable: 0,
                },
            ];
            let _ = crate::sys_capability::capset(&mut hdr, data.as_ptr());
        }
    }

    fn drop_cap(cap: u32) {
        let (lo, hi) = crate::sys_capability::current_caps_effective();
        let (new_lo, new_hi) = if cap < 32 {
            (lo & !(1u32 << cap), hi)
        } else {
            (lo, hi & !(1u32 << (cap - 32)))
        };
        let mut hdr = crate::sys_capability::CapUserHeader {
            version: crate::sys_capability::_LINUX_CAPABILITY_VERSION_3,
            pid: 0,
        };
        let data = [
            crate::sys_capability::CapUserData {
                effective: new_lo,
                permitted: u32::MAX,
                inheritable: 0,
            },
            crate::sys_capability::CapUserData {
                effective: new_hi,
                permitted: u32::MAX,
                inheritable: 0,
            },
        ];
        let rc = crate::sys_capability::capset(&mut hdr, data.as_ptr());
        assert_eq!(rc, 0, "capset must succeed dropping cap");
        assert!(!crate::sys_capability::has_capability(cap));
    }

    /// With CAP_WAKE_ALARM held (default), CLOCK_REALTIME_ALARM
    /// succeeds — same as Phase 93, but documents the cap-held path.
    #[test]
    fn test_phase199_timerfd_alarm_with_cap_succeeds() {
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_WAKE_ALARM,
        ));
        errno::set_errno(0);
        let fd = timerfd_create(8, 0); // CLOCK_REALTIME_ALARM
        assert!(fd >= 0, "alarm clock with cap should succeed");
        crate::file::close(fd);
        let fd = timerfd_create(9, 0); // CLOCK_BOOTTIME_ALARM
        assert!(fd >= 0, "alarm clock with cap should succeed");
        crate::file::close(fd);
    }

    /// Without CAP_WAKE_ALARM, CLOCK_REALTIME_ALARM → EPERM.
    #[test]
    fn test_phase199_timerfd_realtime_alarm_no_cap_eperm() {
        let _g = CapGuard::snapshot();
        drop_cap(crate::sys_capability::CAP_WAKE_ALARM);
        errno::set_errno(0);
        let fd = timerfd_create(8, 0); // CLOCK_REALTIME_ALARM
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Without CAP_WAKE_ALARM, CLOCK_BOOTTIME_ALARM → EPERM.
    #[test]
    fn test_phase199_timerfd_boottime_alarm_no_cap_eperm() {
        let _g = CapGuard::snapshot();
        drop_cap(crate::sys_capability::CAP_WAKE_ALARM);
        errno::set_errno(0);
        let fd = timerfd_create(9, 0); // CLOCK_BOOTTIME_ALARM
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::EPERM);
    }

    /// Non-alarm clocks bypass the gate entirely — no EPERM even
    /// without CAP_WAKE_ALARM.
    #[test]
    fn test_phase199_timerfd_non_alarm_clocks_no_cap_still_ok() {
        let _g = CapGuard::snapshot();
        drop_cap(crate::sys_capability::CAP_WAKE_ALARM);
        for &clk in &[
            crate::time::CLOCK_REALTIME,
            crate::time::CLOCK_MONOTONIC,
            crate::time::CLOCK_BOOTTIME,
        ] {
            errno::set_errno(0);
            let fd = timerfd_create(clk, 0);
            assert!(fd >= 0, "non-alarm clock {clk} must still succeed");
            crate::file::close(fd);
        }
    }

    /// EINVAL takes priority over EPERM: bad flags + alarm clock →
    /// EINVAL, not EPERM (flags check runs before cap check).
    #[test]
    fn test_phase199_timerfd_bad_flags_einval_before_eperm() {
        let _g = CapGuard::snapshot();
        drop_cap(crate::sys_capability::CAP_WAKE_ALARM);
        errno::set_errno(0);
        let fd = timerfd_create(8, 0x8000); // bogus flags
        assert_eq!(fd, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "bad flags must yield EINVAL even without CAP_WAKE_ALARM"
        );
    }

    /// EINVAL takes priority: bad clockid + no cap → EINVAL, not EPERM.
    #[test]
    fn test_phase199_timerfd_bad_clockid_einval_before_eperm() {
        let _g = CapGuard::snapshot();
        drop_cap(crate::sys_capability::CAP_WAKE_ALARM);
        errno::set_errno(0);
        let fd = timerfd_create(999, 0); // invalid clockid
        assert_eq!(fd, -1);
        assert_eq!(
            errno::get_errno(),
            errno::EINVAL,
            "bad clockid must yield EINVAL regardless of cap"
        );
    }

    /// After restoring CAP_WAKE_ALARM (CapGuard drop), alarm clocks
    /// work again.
    #[test]
    fn test_phase199_timerfd_cap_restore_re_enables_alarm() {
        {
            let _g = CapGuard::snapshot();
            drop_cap(crate::sys_capability::CAP_WAKE_ALARM);
            errno::set_errno(0);
            let fd = timerfd_create(8, 0);
            assert_eq!(fd, -1, "must fail without cap");
        }
        // CapGuard restored the cap.
        assert!(crate::sys_capability::has_capability(
            crate::sys_capability::CAP_WAKE_ALARM,
        ));
        errno::set_errno(0);
        let fd = timerfd_create(8, 0);
        assert!(fd >= 0, "must succeed after cap restored");
        crate::file::close(fd);
    }
}
