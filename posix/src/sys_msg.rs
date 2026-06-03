//! `<sys/msg.h>` — System V message queue definitions.
//!
//! Re-exports message queue structures, constants, and functions
//! from the `sysv_msg` module.

pub use crate::sysv_msg::MsqidDs;
pub use crate::sysv_msg::msgctl;
pub use crate::sysv_msg::msgget;
pub use crate::sysv_msg::msgrcv;
pub use crate::sysv_msg::msgsnd;

pub use crate::sysv_msg::IPC_CREAT;
pub use crate::sysv_msg::IPC_EXCL;
pub use crate::sysv_msg::IPC_NOWAIT;
pub use crate::sysv_msg::IPC_PRIVATE;
pub use crate::sysv_msg::IPC_RMID;
pub use crate::sysv_msg::IPC_SET;
pub use crate::sysv_msg::IPC_STAT;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msqid_struct_size() {
        assert!(core::mem::size_of::<MsqidDs>() > 0);
    }

    #[test]
    fn test_ipc_constants() {
        assert_eq!(IPC_CREAT, 0o1000);
        assert_eq!(IPC_EXCL, 0o2000);
        assert_eq!(IPC_NOWAIT, 0o4000);
    }

    #[test]
    fn test_ipc_ctl_constants() {
        assert_eq!(IPC_RMID, 0);
        assert_eq!(IPC_SET, 1);
        assert_eq!(IPC_STAT, 2);
    }

    #[test]
    fn test_ipc_private() {
        assert_eq!(IPC_PRIVATE, 0);
    }

    #[test]
    fn test_ipc_constants_distinct() {
        let vals = [IPC_CREAT, IPC_EXCL, IPC_NOWAIT];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IPC_CREAT, crate::sysv_msg::IPC_CREAT);
        assert_eq!(IPC_RMID, crate::sysv_msg::IPC_RMID);
    }
}
