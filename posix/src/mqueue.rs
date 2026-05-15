//! POSIX message queues — stubs.
//!
//! Our OS doesn't implement POSIX message queues (we use IPC channels
//! instead).  These stubs return `ENOSYS` so programs that check for
//! mqueue support at runtime get a clear "not available" response.

/// Message queue descriptor type.
pub type MqdT = i32;

/// Message queue attributes.
#[repr(C)]
pub struct MqAttr {
    /// Flags (O_NONBLOCK, etc.).
    pub mq_flags: i64,
    /// Maximum number of messages.
    pub mq_maxmsg: i64,
    /// Maximum message size.
    pub mq_msgsize: i64,
    /// Number of messages currently queued.
    pub mq_curmsgs: i64,
    /// Padding for future use.
    _pad: [i64; 4],
}

/// Open a message queue.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_open(
    _name: *const u8,
    _oflag: i32,
) -> MqdT {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Close a message queue.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_close(_mqdes: MqdT) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Remove a message queue.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_unlink(_name: *const u8) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Send a message to a queue.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_send(
    _mqdes: MqdT,
    _msg_ptr: *const u8,
    _msg_len: usize,
    _msg_prio: u32,
) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Receive a message from a queue.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_receive(
    _mqdes: MqdT,
    _msg_ptr: *mut u8,
    _msg_len: usize,
    _msg_prio: *mut u32,
) -> isize {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Get message queue attributes.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_getattr(_mqdes: MqdT, _attr: *mut MqAttr) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Set message queue attributes.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_setattr(
    _mqdes: MqdT,
    _newattr: *const MqAttr,
    _oldattr: *mut MqAttr,
) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Send a message with a timeout.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_timedsend(
    _mqdes: MqdT,
    _msg_ptr: *const u8,
    _msg_len: usize,
    _msg_prio: u32,
    _abs_timeout: *const crate::stat::Timespec,
) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Receive a message with a timeout.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_timedreceive(
    _mqdes: MqdT,
    _msg_ptr: *mut u8,
    _msg_len: usize,
    _msg_prio: *mut u32,
    _abs_timeout: *const crate::stat::Timespec,
) -> isize {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Request notification when a message arrives.
///
/// Stub: returns -1 with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn mq_notify(_mqdes: MqdT, _sevp: *const u8) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // All mqueue functions are stubs returning -1 with ENOSYS.

    #[test]
    fn test_mq_open_returns_error() {
        let ret = mq_open(b"/test_queue\0".as_ptr(), 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_mq_close_returns_error() {
        assert_eq!(mq_close(0), -1);
    }

    #[test]
    fn test_mq_unlink_returns_error() {
        assert_eq!(mq_unlink(b"/test_queue\0".as_ptr()), -1);
    }

    #[test]
    fn test_mq_send_returns_error() {
        assert_eq!(mq_send(0, b"hello\0".as_ptr(), 5, 0), -1);
    }

    #[test]
    fn test_mq_receive_returns_error() {
        let mut buf = [0u8; 64];
        let mut prio: u32 = 0;
        assert_eq!(mq_receive(0, buf.as_mut_ptr(), 64, &raw mut prio), -1);
    }

    #[test]
    fn test_mq_getattr_returns_error() {
        let mut attr = MqAttr {
            mq_flags: 0,
            mq_maxmsg: 0,
            mq_msgsize: 0,
            mq_curmsgs: 0,
            _pad: [0; 4],
        };
        assert_eq!(mq_getattr(0, &raw mut attr), -1);
    }

    #[test]
    fn test_mq_setattr_returns_error() {
        let attr = MqAttr {
            mq_flags: 0,
            mq_maxmsg: 0,
            mq_msgsize: 0,
            mq_curmsgs: 0,
            _pad: [0; 4],
        };
        assert_eq!(mq_setattr(0, &raw const attr, core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_mq_timedsend_returns_error() {
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(mq_timedsend(0, b"hello\0".as_ptr(), 5, 0, &raw const ts), -1);
    }

    #[test]
    fn test_mq_timedreceive_returns_error() {
        let mut buf = [0u8; 64];
        let mut prio: u32 = 0;
        let ts = crate::stat::Timespec { tv_sec: 0, tv_nsec: 0 };
        assert_eq!(
            mq_timedreceive(0, buf.as_mut_ptr(), 64, &raw mut prio, &raw const ts),
            -1,
        );
    }

    #[test]
    fn test_mq_notify_returns_error() {
        assert_eq!(mq_notify(0, core::ptr::null()), -1);
    }

    // -- MqAttr layout --

    #[test]
    fn test_mq_attr_size() {
        // 4 i64 fields + 4 i64 padding = 8 * 8 = 64 bytes
        assert_eq!(core::mem::size_of::<MqAttr>(), 64);
    }

    // -- errno is set to ENOSYS for all stubs --

    #[test]
    fn test_mq_open_sets_errno() {
        crate::errno::set_errno(0);
        let _ = mq_open(b"/q\0".as_ptr(), 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mq_close_sets_errno() {
        crate::errno::set_errno(0);
        let _ = mq_close(0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mq_unlink_sets_errno() {
        crate::errno::set_errno(0);
        let _ = mq_unlink(b"/q\0".as_ptr());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mq_send_sets_errno() {
        crate::errno::set_errno(0);
        let _ = mq_send(0, b"x\0".as_ptr(), 1, 0);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mq_receive_sets_errno() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 8];
        let mut prio: u32 = 0;
        let _ = mq_receive(0, buf.as_mut_ptr(), 8, &raw mut prio);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_mq_notify_sets_errno() {
        crate::errno::set_errno(0);
        let _ = mq_notify(0, core::ptr::null());
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -- MqAttr field access --

    #[test]
    fn test_mq_attr_fields() {
        let attr = MqAttr {
            mq_flags: 1,
            mq_maxmsg: 10,
            mq_msgsize: 256,
            mq_curmsgs: 5,
            _pad: [0; 4],
        };
        assert_eq!(attr.mq_flags, 1);
        assert_eq!(attr.mq_maxmsg, 10);
        assert_eq!(attr.mq_msgsize, 256);
        assert_eq!(attr.mq_curmsgs, 5);
    }

    #[test]
    fn test_mqd_t_is_i32() {
        assert_eq!(core::mem::size_of::<MqdT>(), 4);
        let neg: MqdT = -1;
        assert!(neg < 0, "MqdT must be signed (error values are -1)");
    }

    // -- All stubs return -1 regardless of inputs --

    #[test]
    fn test_mq_open_null_name() {
        assert_eq!(mq_open(core::ptr::null(), 0), -1);
    }

    #[test]
    fn test_mq_unlink_null_name() {
        assert_eq!(mq_unlink(core::ptr::null()), -1);
    }

    #[test]
    fn test_mq_send_null_msg() {
        assert_eq!(mq_send(0, core::ptr::null(), 0, 0), -1);
    }

    #[test]
    fn test_mq_receive_null_buf() {
        assert_eq!(mq_receive(0, core::ptr::null_mut(), 0, core::ptr::null_mut()), -1);
    }

    #[test]
    fn test_mq_getattr_null_attr() {
        assert_eq!(mq_getattr(0, core::ptr::null_mut()), -1);
    }
}
