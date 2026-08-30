//! `<linux/fcntl.h>` — File locking constants (flock/F_SETLK).
//!
//! Linux supports two file locking mechanisms: BSD flock() for
//! whole-file advisory locks, and POSIX fcntl() F_SETLK/F_GETLK
//! for byte-range locks. Additionally, Open File Description (OFD)
//! locks (F_OFD_SETLK) fix the per-process vs per-fd semantics
//! issue of traditional POSIX locks.

// ---------------------------------------------------------------------------
// flock() operation types
// ---------------------------------------------------------------------------

/// Shared (read) lock.
pub const LOCK_SH: u32 = 1;
/// Exclusive (write) lock.
pub const LOCK_EX: u32 = 2;
/// Unlock.
pub const LOCK_UN: u32 = 8;
/// Non-blocking flag (OR with LOCK_SH or LOCK_EX).
pub const LOCK_NB: u32 = 4;

// ---------------------------------------------------------------------------
// fcntl() lock types (l_type field in struct flock)
// ---------------------------------------------------------------------------

/// Read (shared) lock.
pub const F_RDLCK: u16 = 0;
/// Write (exclusive) lock.
pub const F_WRLCK: u16 = 1;
/// Unlock.
pub const F_UNLCK: u16 = 2;

// ---------------------------------------------------------------------------
// fcntl() lock commands
// ---------------------------------------------------------------------------

/// Get lock (POSIX).
pub const F_GETLK: u32 = 5;
/// Set lock (POSIX, blocking).
pub const F_SETLK: u32 = 6;
/// Set lock and wait (POSIX, blocking).
pub const F_SETLKW: u32 = 7;

/// Get lock (OFD — Open File Description locks).
pub const F_OFD_GETLK: u32 = 36;
/// Set lock (OFD, non-blocking).
pub const F_OFD_SETLK: u32 = 37;
/// Set lock and wait (OFD, blocking).
pub const F_OFD_SETLKW: u32 = 38;

// ---------------------------------------------------------------------------
// l_whence values (for struct flock)
// ---------------------------------------------------------------------------

/// Offset from beginning of file.
pub const SEEK_SET: u32 = 0;
/// Offset from current position.
pub const SEEK_CUR: u32 = 1;
/// Offset from end of file.
pub const SEEK_END: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flock_ops_distinct() {
        let ops = [LOCK_SH, LOCK_EX, LOCK_NB, LOCK_UN];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_lock_nb_combinable() {
        // NB can be ORed with SH or EX without collision
        assert_eq!(LOCK_SH & LOCK_NB, 0);
        assert_eq!(LOCK_EX & LOCK_NB, 0);
    }

    #[test]
    fn test_fcntl_lock_types_distinct() {
        let types = [F_RDLCK, F_WRLCK, F_UNLCK];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_fcntl_commands_distinct() {
        let cmds = [
            F_GETLK,
            F_SETLK,
            F_SETLKW,
            F_OFD_GETLK,
            F_OFD_SETLK,
            F_OFD_SETLKW,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_seek_values_distinct() {
        let vals = [SEEK_SET, SEEK_CUR, SEEK_END];
        for i in 0..vals.len() {
            for j in (i + 1)..vals.len() {
                assert_ne!(vals[i], vals[j]);
            }
        }
    }
}
