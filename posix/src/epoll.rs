//! Linux-specific I/O multiplexing and fd-based notification stubs.
//!
//! Implements stubs for:
//! - **epoll**: `epoll_create`, `epoll_create1`, `epoll_ctl`,
//!   `epoll_wait`, `epoll_pwait`
//! - **eventfd**: `eventfd`, `eventfd_read`, `eventfd_write`
//! - **timerfd**: `timerfd_create`, `timerfd_settime`, `timerfd_gettime`
//! - **signalfd**: `signalfd`, `signalfd4`
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

    // -- epoll stubs return -1 / ENOSYS --

    #[test]
    fn test_epoll_create_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_create(1), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_epoll_create1_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_create1(0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_epoll_ctl_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_ctl(3, EPOLL_CTL_ADD, 4, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_epoll_wait_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_wait(3, core::ptr::null_mut(), 10, -1), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_epoll_pwait_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_pwait(3, core::ptr::null_mut(), 10, -1, core::ptr::null()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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

    // -- eventfd stubs --

    #[test]
    fn test_eventfd_enosys() {
        errno::set_errno(0);
        assert_eq!(eventfd(0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_eventfd_read_enosys() {
        errno::set_errno(0);
        let mut val: u64 = 0;
        assert_eq!(eventfd_read(3, &raw mut val), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_eventfd_write_enosys() {
        errno::set_errno(0);
        assert_eq!(eventfd_write(3, 1), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
    fn test_timerfd_create_enosys() {
        errno::set_errno(0);
        assert_eq!(timerfd_create(0, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_timerfd_settime_enosys() {
        errno::set_errno(0);
        assert_eq!(timerfd_settime(3, 0, core::ptr::null(), core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_timerfd_gettime_enosys() {
        errno::set_errno(0);
        assert_eq!(timerfd_gettime(3, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
        assert_eq!(ev.events, EPOLLIN | EPOLLOUT);
        assert_eq!(ev.data, 42);
    }

    #[test]
    fn test_epoll_event_data_holds_pointer() {
        // data field is often used to hold a pointer cast to u64.
        let val: u64 = 0x7FFE_0000_1234;
        let ev = EpollEvent { events: EPOLLIN, data: val };
        assert_eq!(ev.data, val);
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
    fn test_epoll_pwait_null_events() {
        errno::set_errno(0);
        let ret = epoll_pwait(3, core::ptr::null_mut(), 10, 0, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- timerfd_gettime with fd 0 --

    #[test]
    fn test_timerfd_gettime_fd_zero() {
        errno::set_errno(0);
        assert_eq!(timerfd_gettime(0, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- timerfd_settime with abstime flag --

    #[test]
    fn test_timerfd_settime_abstime() {
        errno::set_errno(0);
        assert_eq!(
            timerfd_settime(0, TFD_TIMER_ABSTIME, core::ptr::null(), core::ptr::null_mut()),
            -1,
        );
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
        errno::set_errno(0);
        assert_eq!(epoll_create(0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_epoll_create_large_size() {
        errno::set_errno(0);
        assert_eq!(epoll_create(1000), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- epoll_create1 with flags --

    #[test]
    fn test_epoll_create1_cloexec() {
        errno::set_errno(0);
        assert_eq!(epoll_create1(crate::fcntl::O_CLOEXEC), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- epoll_ctl all operations --

    #[test]
    fn test_epoll_ctl_del_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_ctl(3, EPOLL_CTL_DEL, 4, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_epoll_ctl_mod_enosys() {
        errno::set_errno(0);
        assert_eq!(epoll_ctl(3, EPOLL_CTL_MOD, 4, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- eventfd with different initval --

    #[test]
    fn test_eventfd_nonzero_initval() {
        errno::set_errno(0);
        assert_eq!(eventfd(42, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_eventfd_with_flags() {
        errno::set_errno(0);
        assert_eq!(eventfd(0, EFD_CLOEXEC | EFD_NONBLOCK), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_eventfd_semaphore() {
        errno::set_errno(0);
        assert_eq!(eventfd(0, EFD_SEMAPHORE), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -- eventfd_read with null --

    #[test]
    fn test_eventfd_read_null() {
        errno::set_errno(0);
        assert_eq!(eventfd_read(3, core::ptr::null_mut()), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
        errno::set_errno(0);
        assert_eq!(timerfd_create(1, 0), -1); // CLOCK_MONOTONIC = 1
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_timerfd_create_with_flags() {
        errno::set_errno(0);
        assert_eq!(timerfd_create(0, TFD_CLOEXEC | TFD_NONBLOCK), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
        errno::set_errno(0);
        assert_eq!(epoll_wait(3, core::ptr::null_mut(), 10, 0), -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
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
}
