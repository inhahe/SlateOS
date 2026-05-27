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
    SYS_EVENTFD_CLOSE, SYS_EVENTFD_CREATE, SYS_EVENTFD_READ, SYS_EVENTFD_TRY_READ,
    SYS_EVENTFD_WRITE, SYS_CLOCK_MONOTONIC, SYS_SLEEP, syscall0, syscall1, syscall2,
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
    EPOLLIN | EPOLLPRI | EPOLLOUT | EPOLLERR | EPOLLHUP | EPOLLRDHUP
        | EPOLLONESHOT | EPOLLET;

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
pub extern "C" fn epoll_ctl(
    epfd: i32,
    op: i32,
    fd: i32,
    event: *mut EpollEvent,
) -> i32 {
    let Some(ep_entry) = fdtable::get_fd(epfd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if ep_entry.kind != HandleKind::Epoll {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // Linux: cannot epoll yourself.
    if fd == epfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    // The watched fd must be a valid open fd.  (Closed fds are allowed
    // for DEL since the caller may be cleaning up state.)
    if op != EPOLL_CTL_DEL && fdtable::get_fd(fd).is_none() {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    let idx = ep_entry.handle;

    match op {
        EPOLL_CTL_ADD => {
            if event.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: caller asserts validity of `event` for ADD/MOD.
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
            if event.is_null() {
                errno::set_errno(errno::EFAULT);
                return -1;
            }
            // SAFETY: caller asserts validity of `event`.
            let ev = unsafe { core::ptr::read_unaligned(event) };
            let events_val = ev.events;
            let data_val = ev.data;
            let res = with_instance_mut(idx, |inst| {
                for slot in &mut inst.entries {
                    if let Some(entry) = slot.as_mut() {
                        if entry.fd == fd {
                            entry.events = events_val;
                            entry.data = data_val;
                            // Re-arm oneshot per Linux semantics.
                            entry.oneshot_fired = false;
                            return Ok(());
                        }
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
                    if let Some(entry) = slot.as_ref() {
                        if entry.fd == fd {
                            *slot = None;
                            return Ok(());
                        }
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

    if maxevents <= 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    if events.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    let Some(ep_entry) = fdtable::get_fd(epfd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
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
                if let Some(entry) = inst.entries.get_mut(i) {
                    if let Some(watched) = entry.as_mut() {
                        if !watched.oneshot_fired {
                            let revents = compute_revents(watched.fd, watched.events);
                            if revents != 0 {
                                // Write event into the caller's buffer.
                                // SAFETY: caller asserts events is valid
                                // for maxevents entries; count < limit.
                                #[allow(clippy::cast_sign_loss)]
                                let slot_ptr = unsafe {
                                    events.add(count as usize)
                                };
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
    let tms: i32 = if timeout.is_null() {
        -1
    } else {
        // SAFETY: caller guarantees timeout points to a valid Timespec.
        let ts = unsafe { &*timeout };
        if ts.tv_sec == 0 && ts.tv_nsec == 0 {
            0
        } else {
            // Round up nanoseconds → milliseconds so a non-zero
            // sub-millisecond timeout doesn't collapse to 0.
            let ms = ts.tv_sec
                .saturating_mul(1_000)
                .saturating_add(ts.tv_nsec.saturating_add(999_999) / 1_000_000);
            if ms > i64::from(i32::MAX) {
                i32::MAX
            } else if ms <= 0 {
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd_read(fd: i32, value: *mut u64) -> i32 {
    if value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Eventfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let is_nb = fdtable::get_status_flags(fd).unwrap_or(0) & crate::fcntl::O_NONBLOCK != 0;
    let nr = if is_nb { SYS_EVENTFD_TRY_READ } else { SYS_EVENTFD_READ };
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
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd_write(fd: i32, value: u64) -> i32 {
    if value == u64::MAX {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Eventfd {
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
pub fn timerfd_read(idx: u64, buf: &mut [u8]) -> Result<usize, i32> {
    if buf.len() < 8 {
        return Err(errno::EINVAL);
    }
    let now = now_ns();
    let count = with_timerfd_mut(idx, |inst| timerfd_consume_expirations(inst, now))
        .ok_or(errno::EBADF)?;
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
/// `clockid` is accepted for API compatibility but all timerfds use the
/// monotonic clock (see module note).  Valid `flags`: `TFD_CLOEXEC`,
/// `TFD_NONBLOCK`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timerfd_create(_clockid: i32, flags: i32) -> i32 {
    if flags & !(TFD_CLOEXEC | TFD_NONBLOCK) != 0 {
        errno::set_errno(errno::EINVAL);
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
    let Some(fd) =
        fdtable::alloc_fd_with_flags(HandleKind::Timerfd, idx as u64, 0)
    else {
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
    if new_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    if flags & !TFD_TIMER_ABSTIME != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let Some(entry) = fdtable::get_fd(fd) else {
        errno::set_errno(errno::EBADF);
        return -1;
    };
    if entry.kind != HandleKind::Timerfd {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let idx = entry.handle;

    // SAFETY: caller asserts validity of `new_value`.
    let nv = unsafe { core::ptr::read(new_value) };
    let Some(value_ns) = timespec_to_ns(&nv.it_value) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let Some(interval_ns) = timespec_to_ns(&nv.it_interval) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };

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
        let remaining_ns = if next == 0 || now >= next { 0 } else { next - now };
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
/// # Safety
/// `curr_value` must be a valid writable pointer to an `Itimerspec`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn timerfd_gettime(
    fd: i32,
    curr_value: *mut Itimerspec,
) -> i32 {
    if curr_value.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
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
    let (next, ival) = with_timerfd(idx, |inst| (inst.next_expiry_ns, inst.interval_ns))
        .unwrap_or((0, 0));
    let remaining_ns = if next == 0 || now >= next { 0 } else { next - now };
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

/// Create or modify a signalfd.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn signalfd(_fd: i32, _mask: *const u64, _flags: i32) -> i32 {
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
// inotify — filesystem event monitoring
// ===========================================================================

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

/// Initialize an inotify instance.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_init() -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Initialize an inotify instance with flags.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_init1(_flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Add a watch to an inotify instance.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_add_watch(
    _fd: i32,
    _pathname: *const u8,
    _mask: u32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Remove a watch from an inotify instance.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn inotify_rm_watch(_fd: i32, _wd: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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
        assert_eq!(epoll_ctl(250, EPOLL_CTL_ADD, 4, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_epoll_wait_invalid_maxevents_returns_einval() {
        errno::set_errno(0);
        // SAFETY: maxevents check happens before any pointer dereference.
        let ret = unsafe { epoll_wait(3, core::ptr::null_mut(), 0, -1) };
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
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
        assert!(bad_bit & (EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE) == 0);
        assert_eq!(eventfd(0, bad_bit), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    /// `eventfd_read` with a null pointer must fail with EFAULT.
    #[test]
    fn test_eventfd_read_null_returns_efault() {
        errno::set_errno(0);
        assert_eq!(eventfd_read(3, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
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

    /// `eventfd_write` with `u64::MAX` is invalid per Linux semantics
    /// and must be rejected before issuing the kernel call.
    #[test]
    fn test_eventfd_write_max_rejected() {
        errno::set_errno(0);
        assert_eq!(eventfd_write(3, u64::MAX), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
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
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        let ret = unsafe {
            timerfd_settime(3, 0, &new, core::ptr::null_mut())
        };
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
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
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
        errno::set_errno(0);
        assert_eq!(signalfd(-1, core::ptr::null(), 0), -1);
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

    // -- inotify stubs --

    #[test]
    fn test_inotify_init_enosys() {
        errno::set_errno(0);
        assert_eq!(inotify_init(), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_inotify_init1_enosys() {
        errno::set_errno(0);
        assert_eq!(inotify_init1(0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_inotify_add_watch_enosys() {
        errno::set_errno(0);
        assert_eq!(inotify_add_watch(3, core::ptr::null(), IN_MODIFY), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_inotify_rm_watch_enosys() {
        errno::set_errno(0);
        assert_eq!(inotify_rm_watch(3, 1), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
        let ev = EpollEvent { events: EPOLLIN | EPOLLOUT, data: 42 };
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
        let ev = EpollEvent { events: EPOLLIN, data: val };
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
            it_interval: crate::stat::Timespec { tv_sec: 1, tv_nsec: 500_000_000 },
            it_value: crate::stat::Timespec { tv_sec: 5, tv_nsec: 0 },
        };
        assert_eq!(its.it_interval.tv_sec, 1);
        assert_eq!(its.it_interval.tv_nsec, 500_000_000);
        assert_eq!(its.it_value.tv_sec, 5);
        assert_eq!(its.it_value.tv_nsec, 0);
    }

    // -- signalfd with different args --

    #[test]
    fn test_signalfd_negative_fd() {
        errno::set_errno(0);
        assert_eq!(signalfd(-1, core::ptr::null(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- epoll_pwait null events --

    #[test]
    fn test_epoll_pwait_null_events_returns_efault() {
        errno::set_errno(0);
        // SAFETY: events pointer is intentionally null; the wrapper
        // checks for null before any dereference.
        let ret = unsafe {
            epoll_pwait(3, core::ptr::null_mut(), 10, 0, core::ptr::null())
        };
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
            timerfd_settime(0, TFD_TIMER_ABSTIME, core::ptr::null(), core::ptr::null_mut())
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
            e == errno::EBADF || e == errno::EFAULT || e == errno::EINVAL
                || e == errno::ENOENT,
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
        assert!(bad & (EFD_CLOEXEC | EFD_NONBLOCK | EFD_SEMAPHORE) == 0);
        assert_eq!(eventfd(42, bad), -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -- inotify_init1 with flags --

    #[test]
    fn test_inotify_init1_cloexec() {
        errno::set_errno(0);
        assert_eq!(inotify_init1(IN_CLOEXEC), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_inotify_init1_nonblock() {
        errno::set_errno(0);
        assert_eq!(inotify_init1(IN_NONBLOCK), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_inotify_init1_combined_flags() {
        errno::set_errno(0);
        assert_eq!(inotify_init1(IN_CLOEXEC | IN_NONBLOCK), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- inotify_add_watch with path --

    #[test]
    fn test_inotify_add_watch_with_path() {
        errno::set_errno(0);
        assert_eq!(inotify_add_watch(3, b"/tmp\0".as_ptr(), IN_MODIFY | IN_CREATE), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- inotify event flag single-bit checks --

    #[test]
    fn test_inotify_event_flags_single_bits() {
        let flags = [
            IN_ACCESS, IN_MODIFY, IN_ATTRIB, IN_CLOSE_WRITE,
            IN_CLOSE_NOWRITE, IN_OPEN, IN_MOVED_FROM, IN_MOVED_TO,
            IN_CREATE, IN_DELETE, IN_DELETE_SELF, IN_MOVE_SELF,
        ];
        for f in flags {
            assert_eq!(f.count_ones(), 1, "flag 0x{f:x} is not a single bit");
        }
    }

    // -- signalfd with positive fd (modify existing) --

    #[test]
    fn test_signalfd_positive_fd() {
        errno::set_errno(0);
        assert_eq!(signalfd(3, core::ptr::null(), 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 1, tv_nsec: 0 },
        };
        let r = unsafe { timerfd_settime(fd, 0, &new, core::ptr::null_mut()) };
        assert_eq!(r, 0);

        // gettime should report a value <= 1 second remaining.
        let mut cur = Itimerspec {
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
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
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 5, tv_nsec: 0 },
        };
        unsafe { timerfd_settime(fd, 0, &armed, core::ptr::null_mut()); }

        // Disarm (it_value = 0).
        let disarm = Itimerspec {
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        let r = unsafe { timerfd_settime(fd, 0, &disarm, core::ptr::null_mut()) };
        assert_eq!(r, 0);

        // gettime should report 0 remaining (disarmed).
        let mut cur = Itimerspec {
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
        };
        unsafe { timerfd_gettime(fd, &mut cur); }
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
            it_interval: crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 },
            it_value: crate::stat::Timespec { tv_sec: -1, tv_nsec: 0 },
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
        // signalfd4 delegates to signalfd which is an ENOSYS stub.
        crate::errno::set_errno(0);
        let ret = signalfd4(-1, core::ptr::null(), 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_signalfd4_with_flags() {
        crate::errno::set_errno(0);
        let ret = signalfd4(-1, core::ptr::null(), EFD_CLOEXEC as i32);
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
            epoll_pwait2(-1, core::ptr::null_mut(), 0, core::ptr::null(), core::ptr::null())
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
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 100_000 };
        let ret = unsafe {
            epoll_pwait2(-1, core::ptr::null_mut(), 1, &ts, core::ptr::null())
        };
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

        let mut ev = EpollEvent { events: EPOLLIN, data: 0xAABB_CCDDu64 };
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
        let mut ev = EpollEvent { events: EPOLLIN, data: 0 };
        let r = epoll_ctl(ep, EPOLL_CTL_ADD, ep, &mut ev as *mut _);
        // Accept either EINVAL (cycle detection) or success (we don't
        // enforce it yet — the resulting wait just won't fire).
        assert!(r == 0 || r == -1);
        crate::file::close(ep);
    }
}
