//! Linux-specific I/O multiplexing and fd-based notification stubs.
//!
//! Implements stubs for:
//! - **epoll**: `epoll_create`, `epoll_create1`, `epoll_ctl`,
//!   `epoll_wait`, `epoll_pwait`
//! - **eventfd**: `eventfd`, `eventfd_read`, `eventfd_write`
//! - **timerfd**: `timerfd_create`, `timerfd_settime`, `timerfd_gettime`
//! - **signalfd**: `signalfd`
//! - **inotify**: `inotify_init`, `inotify_init1`, `inotify_add_watch`,
//!   `inotify_rm_watch`
//!
//! Our OS doesn't implement these Linux-specific APIs.  These stubs
//! allow programs that probe for them at runtime to get a clean "not
//! supported" response.

use crate::errno;

/// Events for `epoll_ctl`.
pub const EPOLLIN: u32 = 0x001;
/// Output ready.
pub const EPOLLOUT: u32 = 0x004;
/// Error condition.
pub const EPOLLERR: u32 = 0x008;
/// Hang up.
pub const EPOLLHUP: u32 = 0x010;
/// Edge-triggered.
pub const EPOLLET: u32 = 1 << 31;

/// `epoll_ctl` operations.
pub const EPOLL_CTL_ADD: i32 = 1;
/// Delete an fd from the interest list.
pub const EPOLL_CTL_DEL: i32 = 2;
/// Modify the events for an fd in the interest list.
pub const EPOLL_CTL_MOD: i32 = 3;

/// Event structure for epoll.
#[repr(C)]
pub struct EpollEvent {
    /// Events bitmask.
    pub events: u32,
    /// User data.
    pub data: u64,
}

/// Create an epoll file descriptor.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_create(_size: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Create an epoll file descriptor with flags.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_create1(_flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Control an epoll file descriptor.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_ctl(
    _epfd: i32,
    _op: i32,
    _fd: i32,
    _event: *mut EpollEvent,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Wait for events on an epoll file descriptor.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_wait(
    _epfd: i32,
    _events: *mut EpollEvent,
    _maxevents: i32,
    _timeout: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Wait for events with a signal mask.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn epoll_pwait(
    _epfd: i32,
    _events: *mut EpollEvent,
    _maxevents: i32,
    _timeout: i32,
    _sigmask: *const u64,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd(_initval: u32, _flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Read from an eventfd (glibc convenience wrapper).
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd_read(_fd: i32, _value: *mut u64) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Write to an eventfd (glibc convenience wrapper).
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn eventfd_write(_fd: i32, _value: u64) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ===========================================================================
// timerfd — timer notification via file descriptor
// ===========================================================================

/// Clock IDs for timerfd_create.
pub const TFD_CLOEXEC: i32 = 0o2_000_000;
/// Non-blocking flag.
pub const TFD_NONBLOCK: i32 = 0o4000;

/// Timerfd settime flags.
pub const TFD_TIMER_ABSTIME: i32 = 1;

/// Timer specification used by timerfd.
#[repr(C)]
pub struct Itimerspec {
    /// Timer interval.
    pub it_interval: crate::stat::Timespec,
    /// Initial expiration.
    pub it_value: crate::stat::Timespec,
}

/// Create a timerfd file descriptor.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timerfd_create(_clockid: i32, _flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Arm or disarm a timerfd.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timerfd_settime(
    _fd: i32,
    _flags: i32,
    _new_value: *const Itimerspec,
    _old_value: *mut Itimerspec,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Get the current setting of a timerfd.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn timerfd_gettime(_fd: i32, _curr_value: *mut Itimerspec) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
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
