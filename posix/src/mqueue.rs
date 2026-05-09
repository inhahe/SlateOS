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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn mq_close(_mqdes: MqdT) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Remove a message queue.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn mq_unlink(_name: *const u8) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Send a message to a queue.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
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
#[unsafe(no_mangle)]
pub extern "C" fn mq_getattr(_mqdes: MqdT, _attr: *mut MqAttr) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}

/// Set message queue attributes.
///
/// Stub: returns -1 with ENOSYS.
#[unsafe(no_mangle)]
pub extern "C" fn mq_setattr(
    _mqdes: MqdT,
    _newattr: *const MqAttr,
    _oldattr: *mut MqAttr,
) -> i32 {
    crate::errno::set_errno(crate::errno::ENOSYS);
    -1
}
