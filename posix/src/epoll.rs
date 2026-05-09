//! Linux `epoll` stubs.
//!
//! Our OS doesn't implement epoll (we use `poll` instead).  These stubs
//! allow programs that probe for epoll at runtime to get a clean "not
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
/// Modify.
pub const EPOLL_CTL_DEL: i32 = 2;
/// Delete.
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
#[unsafe(no_mangle)]
pub extern "C" fn epoll_create(_size: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Create an epoll file descriptor with flags.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn epoll_create1(_flags: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Control an epoll file descriptor.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
