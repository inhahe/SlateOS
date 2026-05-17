//! `<linux/sem.h>` — System V semaphore constants.
//!
//! System V semaphores provide counting semaphore arrays for process
//! synchronization. A semaphore set contains one or more semaphores
//! that can be atomically manipulated via semop(). Each semaphore
//! has a value, a count of processes waiting to increment (semncnt),
//! and a count waiting for zero (semzcnt). Despite being older and
//! less ergonomic than POSIX semaphores, SysV semaphores support
//! atomic operations on multiple semaphores and undo-on-exit.

// ---------------------------------------------------------------------------
// semop() operation flags
// ---------------------------------------------------------------------------

/// Don't wait if operation would block.
pub const SEM_NOWAIT: u32 = 0o004000;
/// Undo semaphore adjustment when process exits.
pub const SEM_UNDO: u32 = 0x1000;

// ---------------------------------------------------------------------------
// semctl() commands
// ---------------------------------------------------------------------------

/// Get value of a single semaphore.
pub const GETVAL: u32 = 12;
/// Get all semaphore values in the set.
pub const GETALL: u32 = 13;
/// Get number of processes waiting for increase.
pub const GETNCNT: u32 = 14;
/// Get PID of last semop() caller.
pub const GETPID: u32 = 11;
/// Get number of processes waiting for zero.
pub const GETZCNT: u32 = 15;
/// Set value of a single semaphore.
pub const SETVAL: u32 = 16;
/// Set all semaphore values in the set.
pub const SETALL: u32 = 17;

// ---------------------------------------------------------------------------
// Semaphore limits
// ---------------------------------------------------------------------------

/// Maximum number of semaphore sets system-wide.
pub const SEMMNI: u32 = 32000;
/// Maximum number of semaphores per set.
pub const SEMMSL: u32 = 32000;
/// Maximum number of semaphores system-wide.
pub const SEMMNS: u32 = 1_024_000_000;
/// Maximum number of operations per semop() call.
pub const SEMOPM: u32 = 500;
/// Maximum semaphore value.
pub const SEMVMX: u32 = 32767;
/// Maximum undo entries per process.
pub const SEMAEM: u32 = 32767;

// ---------------------------------------------------------------------------
// SEM_INFO / SEM_STAT (Linux extensions)
// ---------------------------------------------------------------------------

/// Get system-wide semaphore info.
pub const SEM_INFO: u32 = 19;
/// Get semaphore set status by index (not ID).
pub const SEM_STAT: u32 = 18;
/// Like SEM_STAT but respects permissions.
pub const SEM_STAT_ANY: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_semctl_commands_distinct() {
        let cmds = [
            GETVAL, GETALL, GETNCNT, GETPID, GETZCNT,
            SETVAL, SETALL, SEM_INFO, SEM_STAT, SEM_STAT_ANY,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_limits_positive() {
        assert!(SEMMNI > 0);
        assert!(SEMMSL > 0);
        assert!(SEMMNS > 0);
        assert!(SEMOPM > 0);
        assert!(SEMVMX > 0);
    }

    #[test]
    fn test_sem_flags_no_overlap() {
        assert_eq!(SEM_NOWAIT & SEM_UNDO, 0);
    }

    #[test]
    fn test_semvmx_fits_u16() {
        assert!(SEMVMX <= u16::MAX as u32);
    }
}
