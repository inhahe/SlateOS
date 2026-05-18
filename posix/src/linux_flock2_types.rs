//! `<fcntl.h>` — File lock type and flock constants.
//!
//! These constants define lock types for both POSIX advisory locks
//! (`fcntl(F_SETLK)`) and BSD-style locks (`flock()`), along with
//! the lock operation flags.

// ---------------------------------------------------------------------------
// Lock types (l_type field in struct flock)
// ---------------------------------------------------------------------------

/// Read (shared) lock.
pub const F_RDLCK: u16 = 0;
/// Write (exclusive) lock.
pub const F_WRLCK: u16 = 1;
/// Unlock.
pub const F_UNLCK: u16 = 2;

// ---------------------------------------------------------------------------
// flock() operations
// ---------------------------------------------------------------------------

/// Place a shared lock.
pub const LOCK_SH: u32 = 1;
/// Place an exclusive lock.
pub const LOCK_EX: u32 = 2;
/// Remove an existing lock.
pub const LOCK_UN: u32 = 8;
/// Non-blocking lock request (OR'd with LOCK_SH or LOCK_EX).
pub const LOCK_NB: u32 = 4;

// ---------------------------------------------------------------------------
// Lock whence values (l_whence in struct flock)
// ---------------------------------------------------------------------------

/// Offset is relative to the start of the file.
pub const SEEK_SET_LOCK: u32 = 0;
/// Offset is relative to the current position.
pub const SEEK_CUR_LOCK: u32 = 1;
/// Offset is relative to the end of the file.
pub const SEEK_END_LOCK: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_types_distinct() {
        let types = [F_RDLCK, F_WRLCK, F_UNLCK];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_rdlck_is_zero() {
        assert_eq!(F_RDLCK, 0);
    }

    #[test]
    fn test_flock_ops_distinct() {
        let ops = [LOCK_SH, LOCK_EX, LOCK_UN, LOCK_NB];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_lock_nb_combinable() {
        // LOCK_NB should not overlap with LOCK_SH or LOCK_EX
        assert_eq!(LOCK_NB & LOCK_SH, 0);
        assert_eq!(LOCK_NB & LOCK_EX, 0);
    }

    #[test]
    fn test_whence_distinct() {
        let whence = [SEEK_SET_LOCK, SEEK_CUR_LOCK, SEEK_END_LOCK];
        for i in 0..whence.len() {
            for j in (i + 1)..whence.len() {
                assert_ne!(whence[i], whence[j]);
            }
        }
    }
}
