//! System V semaphores — `<sys/sem.h>`.
//!
//! Stubs for `semget`, `semop`, `semctl`.
//!
//! Our OS does not implement System V IPC.  These stubs return
//! ENOSYS and satisfy link-time references.  Programs should use
//! POSIX semaphores (`sem_init`, `sem_open`, etc.) instead.

use crate::errno;

// ---------------------------------------------------------------------------
// Constants (shared with sysv_msg)
// ---------------------------------------------------------------------------

/// Create if key doesn't exist.
pub const IPC_CREAT: i32 = 0o1000;
/// Fail if key exists.
pub const IPC_EXCL: i32 = 0o2000;
/// No wait.
pub const IPC_NOWAIT: i32 = 0o4000;

/// Remove identifier.
pub const IPC_RMID: i32 = 0;
/// Set options.
pub const IPC_SET: i32 = 1;
/// Get options.
pub const IPC_STAT: i32 = 2;

/// Private key.
pub const IPC_PRIVATE: i32 = 0;

// Semaphore control commands.
/// Get value of semaphore.
pub const GETVAL: i32 = 12;
/// Set value of semaphore.
pub const SETVAL: i32 = 16;
/// Get all semaphore values.
pub const GETALL: i32 = 13;
/// Set all semaphore values.
pub const SETALL: i32 = 17;
/// Get number of processes waiting for increase.
pub const GETNCNT: i32 = 14;
/// Get number of processes waiting for zero.
pub const GETZCNT: i32 = 15;
/// Get PID of last operation.
pub const GETPID: i32 = 11;

/// Undo flag — semaphore operations are undone on process exit.
pub const SEM_UNDO: i32 = 0x1000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// `struct sembuf` — semaphore operation.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Sembuf {
    /// Semaphore number (index in the set).
    pub sem_num: u16,
    /// Semaphore operation: positive (release), negative (acquire), or zero (wait).
    pub sem_op: i16,
    /// Operation flags (e.g., `IPC_NOWAIT`, `SEM_UNDO`).
    pub sem_flg: i16,
}

// ---------------------------------------------------------------------------
// semget
// ---------------------------------------------------------------------------

/// `semget` — get a semaphore set identifier.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semget(_key: i32, _nsems: i32, _semflg: i32) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// semop
// ---------------------------------------------------------------------------

/// `semop` — perform semaphore operations.
///
/// Stub: always fails with ENOSYS.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semop(
    _semid: i32,
    _sops: *const Sembuf,
    _nsops: usize,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

/// `semtimedop` — perform semaphore operations with timeout.
///
/// Stub: always fails with ENOSYS.
///
/// `timeout` points to a `struct timespec` (tv_sec, tv_nsec).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semtimedop(
    _semid: i32,
    _sops: *const Sembuf,
    _nsops: usize,
    _timeout: *const u8, // *const timespec — opaque since we never read it.
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// semctl
// ---------------------------------------------------------------------------

/// `semctl` — semaphore control operations.
///
/// Stub: always fails with ENOSYS.
///
/// Note: The real `semctl` is variadic (takes an optional `union semun`
/// argument for commands like `SETVAL`).  Our stub ignores extra args.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn semctl(
    _semid: i32,
    _semnum: i32,
    _cmd: i32,
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
    fn test_sem_commands() {
        assert_ne!(GETVAL, SETVAL);
        assert_ne!(GETALL, SETALL);
        assert_ne!(GETNCNT, GETZCNT);
    }

    #[test]
    fn test_sem_undo() {
        assert_ne!(SEM_UNDO, 0);
    }

    // -----------------------------------------------------------------------
    // Sembuf
    // -----------------------------------------------------------------------

    #[test]
    fn test_sembuf_size() {
        // sembuf is 3 × i16 = 6 bytes.
        assert_eq!(core::mem::size_of::<Sembuf>(), 6);
    }

    #[test]
    fn test_sembuf_fields() {
        let sb = Sembuf {
            sem_num: 0,
            sem_op: -1,
            sem_flg: SEM_UNDO as i16,
        };
        assert_eq!(sb.sem_num, 0);
        assert_eq!(sb.sem_op, -1);
        assert_eq!(sb.sem_flg, SEM_UNDO as i16);
    }

    // -----------------------------------------------------------------------
    // semget
    // -----------------------------------------------------------------------

    #[test]
    fn test_semget_enosys() {
        crate::errno::set_errno(0);
        let ret = semget(0x5678, 1, IPC_CREAT | 0o666);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_semget_private() {
        let ret = semget(IPC_PRIVATE, 3, IPC_CREAT | 0o666);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_semget_zero_nsems() {
        let ret = semget(1234, 0, 0);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // semop
    // -----------------------------------------------------------------------

    #[test]
    fn test_semop_enosys() {
        crate::errno::set_errno(0);
        let ops = [Sembuf {
            sem_num: 0,
            sem_op: -1,
            sem_flg: 0,
        }];
        let ret = semop(0, ops.as_ptr(), 1);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_semop_null_sops() {
        let ret = semop(0, core::ptr::null(), 0);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_semop_multiple_ops() {
        let ops = [
            Sembuf { sem_num: 0, sem_op: -1, sem_flg: SEM_UNDO as i16 },
            Sembuf { sem_num: 1, sem_op: 1, sem_flg: 0 },
        ];
        let ret = semop(0, ops.as_ptr(), 2);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // semtimedop
    // -----------------------------------------------------------------------

    #[test]
    fn test_semtimedop_enosys() {
        crate::errno::set_errno(0);
        let ops = [Sembuf {
            sem_num: 0,
            sem_op: -1,
            sem_flg: 0,
        }];
        let ret = semtimedop(0, ops.as_ptr(), 1, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    // -----------------------------------------------------------------------
    // semctl
    // -----------------------------------------------------------------------

    #[test]
    fn test_semctl_stat() {
        crate::errno::set_errno(0);
        let ret = semctl(0, 0, IPC_STAT);
        assert_eq!(ret, -1);
        assert_eq!(crate::errno::get_errno(), crate::errno::ENOSYS);
    }

    #[test]
    fn test_semctl_rmid() {
        let ret = semctl(0, 0, IPC_RMID);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_semctl_getval() {
        let ret = semctl(0, 0, GETVAL);
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_semctl_setval() {
        let ret = semctl(0, 0, SETVAL);
        assert_eq!(ret, -1);
    }

    // -----------------------------------------------------------------------
    // Workflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_workflow() {
        // Typical: create set → P operation → V operation → remove.
        let semid = semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600);
        assert_eq!(semid, -1);

        // P (acquire).
        let p_op = [Sembuf { sem_num: 0, sem_op: -1, sem_flg: SEM_UNDO as i16 }];
        assert_eq!(semop(semid, p_op.as_ptr(), 1), -1);

        // V (release).
        let v_op = [Sembuf { sem_num: 0, sem_op: 1, sem_flg: SEM_UNDO as i16 }];
        assert_eq!(semop(semid, v_op.as_ptr(), 1), -1);

        // Remove.
        assert_eq!(semctl(semid, 0, IPC_RMID), -1);
    }
}
