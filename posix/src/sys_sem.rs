//! `<sys/sem.h>` — System V semaphore definitions.
//!
//! Re-exports semaphore structures, constants, and functions from
//! the `sysv_sem` module.

pub use crate::sysv_sem::Sembuf;
pub use crate::sysv_sem::semget;
pub use crate::sysv_sem::semop;
pub use crate::sysv_sem::semtimedop;
pub use crate::sysv_sem::semctl;

// Re-use IPC constants from sysv_msg (they are shared across SysV IPC).
pub use crate::sysv_msg::IPC_CREAT;
pub use crate::sysv_msg::IPC_EXCL;
pub use crate::sysv_msg::IPC_NOWAIT;
pub use crate::sysv_msg::IPC_RMID;
pub use crate::sysv_msg::IPC_SET;
pub use crate::sysv_msg::IPC_STAT;
pub use crate::sysv_msg::IPC_PRIVATE;

/// Get value of semaphore.
pub const GETVAL: i32 = 12;

/// Get all semaphore values.
pub const GETALL: i32 = 13;

/// Set value of semaphore.
pub const SETVAL: i32 = 16;

/// Set all semaphore values.
pub const SETALL: i32 = 17;

/// Get process ID of last semop.
pub const GETPID: i32 = 11;

/// Get number of processes waiting for increase.
pub const GETNCNT: i32 = 14;

/// Get number of processes waiting for zero.
pub const GETZCNT: i32 = 15;

/// Undo flag: automatically undo operation at process exit.
pub const SEM_UNDO: i32 = 0x1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sembuf_struct_size() {
        assert!(core::mem::size_of::<Sembuf>() > 0);
    }

    #[test]
    fn test_sem_cmd_distinct() {
        let cmds = [GETVAL, GETALL, SETVAL, SETALL, GETPID, GETNCNT, GETZCNT];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_sem_undo() {
        assert_ne!(SEM_UNDO, 0);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IPC_CREAT, crate::sysv_msg::IPC_CREAT);
        assert_eq!(
            core::mem::size_of::<Sembuf>(),
            core::mem::size_of::<crate::sysv_sem::Sembuf>()
        );
    }
}
