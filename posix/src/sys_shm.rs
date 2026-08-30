//! `<sys/shm.h>` — System V shared memory definitions.
//!
//! Re-exports shared memory structures, constants, and functions
//! from the `sysv_shm` module.

pub use crate::sysv_shm::ShmidDs;
pub use crate::sysv_shm::shmat;
pub use crate::sysv_shm::shmctl;
pub use crate::sysv_shm::shmdt;
pub use crate::sysv_shm::shmget;

// Re-use IPC constants from sysv_msg (they are shared across SysV IPC).
pub use crate::sysv_msg::IPC_CREAT;
pub use crate::sysv_msg::IPC_EXCL;
pub use crate::sysv_msg::IPC_PRIVATE;
pub use crate::sysv_msg::IPC_RMID;
pub use crate::sysv_msg::IPC_SET;
pub use crate::sysv_msg::IPC_STAT;

/// Attach read-only.
pub const SHM_RDONLY: i32 = 0o10000;

/// Round attach address to SHMLBA.
pub const SHM_RND: i32 = 0o20000;

/// Take-over region on attach.
pub const SHM_REMAP: i32 = 0o40000;

/// Lock segment in memory.
pub const SHM_LOCK: i32 = 11;

/// Unlock segment.
pub const SHM_UNLOCK: i32 = 12;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shmid_struct_size() {
        assert!(core::mem::size_of::<ShmidDs>() > 0);
    }

    #[test]
    fn test_shm_flags_distinct() {
        let flags = [SHM_RDONLY, SHM_RND, SHM_REMAP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_shm_lock_unlock() {
        assert_ne!(SHM_LOCK, SHM_UNLOCK);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(IPC_CREAT, crate::sysv_msg::IPC_CREAT);
        assert_eq!(
            core::mem::size_of::<ShmidDs>(),
            core::mem::size_of::<crate::sysv_shm::ShmidDs>()
        );
    }
}
