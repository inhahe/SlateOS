//! System V message queues — `<sys/msg.h>`.
//!
//! Stubs for `msgget`, `msgsnd`, `msgrcv`, `msgctl`.
//!
//! Our OS does not implement System V IPC.  These stubs return
//! appropriate errors (`ENOSYS`) and satisfy link-time references
//! from programs that use System V message queues.  Programs should
//! use POSIX message queues (`mq_open`, etc.) instead.

use crate::errno;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;
/// No wait on operations.
pub const IPC_NOWAIT: i32 = 0o4000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key (create new unique queue).
pub const IPC_PRIVATE: i32 = 0;

/// Message type for `msgrcv` — receive any message.
pub const MSG_NOERROR: i32 = 0o10000;
/// Receive message of any type except specified.
pub const MSG_EXCEPT: i32 = 0o20000;
/// Non-blocking receive (same as `IPC_NOWAIT`).
pub const MSG_COPY: i32 = 0o40000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct msqid_ds` — message queue data structure.
///
/// Provides metadata about a message queue.  Since we never create
/// a queue, this is only used as a pointer type in `msgctl`.
#[repr(C)]
pub struct MsqidDs {
    /// Owner's UID.
    pub msg_perm_uid: u32,
    /// Owner's GID.
    pub msg_perm_gid: u32,
    /// Creator's UID.
    pub msg_perm_cuid: u32,
    /// Creator's GID.
    pub msg_perm_cgid: u32,
    /// Permissions mode.
    pub msg_perm_mode: u16,
    /// Padding.
    pub _pad: u16,
    /// Number of bytes currently on queue.
    pub msg_cbytes: usize,
    /// Number of messages currently on queue.
    pub msg_qnum: usize,
    /// Maximum bytes allowed on queue.
    pub msg_qbytes: usize,
    /// PID of last msgsnd.
    pub msg_lspid: i32,
    /// PID of last msgrcv.
    pub msg_lrpid: i32,
    /// Time of last msgsnd.
    pub msg_stime: i64,
    /// Time of last msgrcv.
    pub msg_rtime: i64,
    /// Time of last change.
    pub msg_ctime: i64,
}

// ---------------------------------------------------------------------------
// msgget
// ---------------------------------------------------------------------------

/// `msgget` — get a message queue identifier.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgget(_key: i32, _msgflg: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// msgsnd
// ---------------------------------------------------------------------------

/// `msgsnd` — send a message to a queue.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgsnd(
    _msqid: i32,
    _msgp: *const u8,
    _msgsz: usize,
    _msgflg: i32,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// msgrcv
// ---------------------------------------------------------------------------

/// `msgrcv` — receive a message from a queue.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgrcv(
    _msqid: i32,
    _msgp: *mut u8,
    _msgsz: usize,
    _msgtyp: i64,
    _msgflg: i32,
) -> isize {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// msgctl
// ---------------------------------------------------------------------------

/// `msgctl` — message queue control operations.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn msgctl(
    _msqid: i32,
    _cmd: i32,
    _buf: *mut MsqidDs,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Constants
    // -----------------------------------------------------------------------

    #[test]
    fn test_ipc_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
        assert_eq!(IPC_NOWAIT, 0o4000);
    }

    #[test]
    fn test_ipc_cmd_constants() {
        assert_eq!(IPC_RMID, 0);
        assert_eq!(IPC_SET, 1);
        assert_eq!(IPC_STAT, 2);
    }

    #[test]
    fn test_ipc_private() {
        assert_eq!(IPC_PRIVATE, 0);
    }

    #[test]
    fn test_msg_flags() {
        assert_ne!(MSG_NOERROR, 0);
        assert_ne!(MSG_EXCEPT, 0);
        assert_ne!(MSG_COPY, 0);
    }

    // -----------------------------------------------------------------------
    // msgget
    // -----------------------------------------------------------------------

    #[test]
    fn test_msgget_enosys() {
        crate::errno::set_errno(0);
        let ret = msgget(12345, IPC_CREAT | 0o666);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_msgget_private() {
        crate::errno::set_errno(0);
        let ret = msgget(IPC_PRIVATE, IPC_CREAT | 0o666);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // msgsnd
    // -----------------------------------------------------------------------

    #[test]
    fn test_msgsnd_enosys() {
        crate::errno::set_errno(0);
        let ret = msgsnd(0, b"hello\0".as_ptr(), 6, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_msgsnd_null_msg() {
        let ret = msgsnd(0, core::ptr::null(), 0, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_msgsnd_nowait() {
        let ret = msgsnd(0, b"x\0".as_ptr(), 1, IPC_NOWAIT);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // msgrcv
    // -----------------------------------------------------------------------

    #[test]
    fn test_msgrcv_enosys() {
        crate::errno::set_errno(0);
        let mut buf = [0u8; 64];
        let ret = msgrcv(0, buf.as_mut_ptr(), buf.len(), 0, 0);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_msgrcv_null_buf() {
        let ret = msgrcv(0, core::ptr::null_mut(), 0, 0, 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_msgrcv_specific_type() {
        let mut buf = [0u8; 32];
        let ret = msgrcv(0, buf.as_mut_ptr(), buf.len(), 42, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // msgctl
    // -----------------------------------------------------------------------

    #[test]
    fn test_msgctl_stat() {
        crate::errno::set_errno(0);
        let ret = msgctl(0, IPC_STAT, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_msgctl_rmid() {
        let ret = msgctl(0, IPC_RMID, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_msgctl_set() {
        let ret = msgctl(0, IPC_SET, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    #[test]
    fn test_msqid_ds_layout() {
        // Verify the struct can be instantiated and has expected field types.
        let ds = MsqidDs {
            msg_perm_uid: 0,
            msg_perm_gid: 0,
            msg_perm_cuid: 0,
            msg_perm_cgid: 0,
            msg_perm_mode: 0o666,
            _pad: 0,
            msg_cbytes: 0,
            msg_qnum: 0,
            msg_qbytes: 16384,
            msg_lspid: 0,
            msg_lrpid: 0,
            msg_stime: 0,
            msg_rtime: 0,
            msg_ctime: 0,
        };
        assert_eq!(ds.msg_perm_mode, 0o666);
        assert_eq!(ds.msg_qbytes, 16384);
    }

    // -----------------------------------------------------------------------
    // Workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_workflow() {
        // Typical usage: get → send → receive → control → remove.
        let mqid = msgget(0x1234, IPC_CREAT | 0o666);
        assert_eq!(mqid, -1); // always fails

        let ret = msgsnd(mqid, b"data\0".as_ptr(), 4, 0);
        assert_eq!(ret, -1);

        let mut buf = [0u8; 64];
        let rcv = msgrcv(mqid, buf.as_mut_ptr(), buf.len(), 0, 0);
        assert_eq!(rcv, -1);

        let ctl = msgctl(mqid, IPC_RMID, core::ptr::null_mut());
        assert_eq!(ctl, -1);
    }
}
