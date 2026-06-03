//! `<linux/filelock.h>` — File locking constants.
//!
//! Linux supports multiple file locking mechanisms: POSIX locks
//! (fcntl F_SETLK, per-process, range-based), flock locks
//! (per-open-file-description, whole-file), and Open File
//! Description (OFD) locks (per-fd, range-based, added in 3.15).
//! Locks can be advisory (not enforced by filesystem) or mandatory
//! (enforced, deprecated). Lease locks support break notifications
//! for delegation-like semantics.

// ---------------------------------------------------------------------------
// Lock types
// ---------------------------------------------------------------------------

/// Read (shared) lock — multiple holders allowed.
pub const F_RDLCK: u32 = 0;
/// Write (exclusive) lock — single holder only.
pub const F_WRLCK: u32 = 1;
/// Unlock (release a previously held lock).
pub const F_UNLCK: u32 = 2;

// ---------------------------------------------------------------------------
// flock lock operations
// ---------------------------------------------------------------------------

/// Shared lock (flock).
pub const LOCK_SH: u32 = 1;
/// Exclusive lock (flock).
pub const LOCK_EX: u32 = 2;
/// Non-blocking flag (combine with SH or EX).
pub const LOCK_NB: u32 = 4;
/// Unlock (flock).
pub const LOCK_UN: u32 = 8;

// ---------------------------------------------------------------------------
// fcntl lock commands
// ---------------------------------------------------------------------------

/// Get lock info (POSIX advisory).
pub const F_GETLK: u32 = 5;
/// Set lock, blocking (POSIX advisory).
pub const F_SETLK: u32 = 6;
/// Set lock, non-blocking (POSIX advisory).
pub const F_SETLKW: u32 = 7;
/// Get lock info (OFD lock).
pub const F_OFD_GETLK: u32 = 36;
/// Set OFD lock, non-blocking.
pub const F_OFD_SETLK: u32 = 37;
/// Set OFD lock, blocking.
pub const F_OFD_SETLKW: u32 = 38;

// ---------------------------------------------------------------------------
// Lease lock types
// ---------------------------------------------------------------------------

/// Read lease (break on write by others).
pub const F_RDLEASE: u32 = 1;
/// Write lease (break on any access by others).
pub const F_WRLEASE: u32 = 2;
/// Unlock lease.
pub const F_UNLEASE: u32 = 3;

// ---------------------------------------------------------------------------
// Lock whence values (for flock struct l_whence)
// ---------------------------------------------------------------------------

/// Offset relative to start of file.
pub const SEEK_SET_LOCK: u32 = 0;
/// Offset relative to current position.
pub const SEEK_CUR_LOCK: u32 = 1;
/// Offset relative to end of file.
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
    fn test_flock_ops_distinct() {
        let ops = [LOCK_SH, LOCK_EX, LOCK_NB, LOCK_UN];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
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
    fn test_lease_types_distinct() {
        let leases = [F_RDLEASE, F_WRLEASE, F_UNLEASE];
        for i in 0..leases.len() {
            for j in (i + 1)..leases.len() {
                assert_ne!(leases[i], leases[j]);
            }
        }
    }

    #[test]
    fn test_whence_distinct() {
        let ws = [SEEK_SET_LOCK, SEEK_CUR_LOCK, SEEK_END_LOCK];
        for i in 0..ws.len() {
            for j in (i + 1)..ws.len() {
                assert_ne!(ws[i], ws[j]);
            }
        }
    }
}
