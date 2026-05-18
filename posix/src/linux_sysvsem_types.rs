//! `<sys/sem.h>` — System V semaphore constants.
//!
//! System V semaphores (`semget`, `semop`, `semctl`) provide
//! counting semaphore arrays for inter-process synchronization.
//! These constants define control commands, operation flags,
//! and default limits.

// ---------------------------------------------------------------------------
// semget() flags
// ---------------------------------------------------------------------------

/// Create a new semaphore set.
pub const IPC_CREAT_SEM: u32 = 0o1000;
/// Fail if set exists (with IPC_CREAT).
pub const IPC_EXCL_SEM: u32 = 0o2000;

// ---------------------------------------------------------------------------
// semctl() commands
// ---------------------------------------------------------------------------

/// Get value of a single semaphore.
pub const GETVAL: u32 = 12;
/// Set value of a single semaphore.
pub const SETVAL: u32 = 16;
/// Get all semaphore values.
pub const GETALL: u32 = 13;
/// Set all semaphore values.
pub const SETALL: u32 = 17;
/// Get semaphore set info (ipc_perm).
pub const IPC_STAT_SEM: u32 = 2;
/// Set semaphore set info.
pub const IPC_SET_SEM: u32 = 1;
/// Remove semaphore set.
pub const IPC_RMID_SEM: u32 = 0;
/// Get system-wide semaphore info.
pub const IPC_INFO_SEM: u32 = 3;
/// Get semaphore info by index.
pub const SEM_INFO: u32 = 19;
/// Get statistics.
pub const SEM_STAT: u32 = 18;
/// Get statistics, newer version.
pub const SEM_STAT_ANY: u32 = 20;
/// Get number of processes waiting for semaphore increase.
pub const GETNCNT: u32 = 14;
/// Get PID of last semop.
pub const GETPID: u32 = 11;
/// Get number of processes waiting for semaphore zero.
pub const GETZCNT: u32 = 15;

// ---------------------------------------------------------------------------
// semop() flags
// ---------------------------------------------------------------------------

/// Do not block on this operation.
pub const IPC_NOWAIT_SEM: u32 = 0o4000;
/// Undo operation on process exit.
pub const SEM_UNDO: u32 = 0x1000;

// ---------------------------------------------------------------------------
// Semaphore limits (defaults)
// ---------------------------------------------------------------------------

/// Maximum number of semaphore sets system-wide.
pub const SEMMNI_DEFAULT: u32 = 32000;
/// Maximum number of semaphores per set.
pub const SEMMSL_DEFAULT: u32 = 32000;
/// Maximum number of semaphores system-wide.
pub const SEMMNS_DEFAULT: u32 = 1024000000;
/// Maximum number of undo entries per process.
pub const SEMOPM_DEFAULT: u32 = 500;
/// Maximum semaphore value.
pub const SEMVMX_DEFAULT: u32 = 32767;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_flags_no_overlap() {
        assert_eq!(IPC_CREAT_SEM & IPC_EXCL_SEM, 0);
    }

    #[test]
    fn test_ctl_commands_distinct() {
        let cmds = [
            GETVAL, SETVAL, GETALL, SETALL,
            IPC_STAT_SEM, IPC_SET_SEM, IPC_RMID_SEM,
            IPC_INFO_SEM, SEM_INFO, SEM_STAT, SEM_STAT_ANY,
            GETNCNT, GETPID, GETZCNT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_rmid_is_zero() {
        assert_eq!(IPC_RMID_SEM, 0);
    }

    #[test]
    fn test_sem_undo_value() {
        assert_eq!(SEM_UNDO, 0x1000);
    }

    #[test]
    fn test_semmni_default() {
        assert_eq!(SEMMNI_DEFAULT, 32000);
    }

    #[test]
    fn test_semmsl_default() {
        assert_eq!(SEMMSL_DEFAULT, 32000);
    }

    #[test]
    fn test_semvmx_default() {
        assert_eq!(SEMVMX_DEFAULT, 32767);
    }

    #[test]
    fn test_semopm_default() {
        assert_eq!(SEMOPM_DEFAULT, 500);
    }

    #[test]
    fn test_limits_positive() {
        assert!(SEMMNI_DEFAULT > 0);
        assert!(SEMMSL_DEFAULT > 0);
        assert!(SEMMNS_DEFAULT > 0);
        assert!(SEMOPM_DEFAULT > 0);
        assert!(SEMVMX_DEFAULT > 0);
    }
}
