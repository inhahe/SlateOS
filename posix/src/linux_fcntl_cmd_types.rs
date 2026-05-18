//! `<fcntl.h>` — fcntl command constants.
//!
//! `fcntl()` performs various operations on open file descriptors.
//! These constants define the command codes for duplicating fds,
//! getting/setting flags, managing locks, and controlling leases.

// ---------------------------------------------------------------------------
// Duplicate fd commands
// ---------------------------------------------------------------------------

/// Duplicate a file descriptor.
pub const F_DUPFD: u32 = 0;
/// Duplicate with close-on-exec set.
pub const F_DUPFD_CLOEXEC: u32 = 1030;

// ---------------------------------------------------------------------------
// File descriptor flags
// ---------------------------------------------------------------------------

/// Get file descriptor flags.
pub const F_GETFD: u32 = 1;
/// Set file descriptor flags.
pub const F_SETFD: u32 = 2;
/// Close-on-exec flag bit.
pub const FD_CLOEXEC: u32 = 1;

// ---------------------------------------------------------------------------
// File status flags
// ---------------------------------------------------------------------------

/// Get file status flags.
pub const F_GETFL: u32 = 3;
/// Set file status flags.
pub const F_SETFL: u32 = 4;

// ---------------------------------------------------------------------------
// Advisory locking
// ---------------------------------------------------------------------------

/// Get advisory lock.
pub const F_GETLK: u32 = 5;
/// Set advisory lock (blocking).
pub const F_SETLK: u32 = 6;
/// Set advisory lock (wait).
pub const F_SETLKW: u32 = 7;
/// Get lock (64-bit offsets).
pub const F_GETLK64: u32 = 12;
/// Set lock (64-bit offsets).
pub const F_SETLK64: u32 = 13;
/// Set lock wait (64-bit offsets).
pub const F_SETLKW64: u32 = 14;

// ---------------------------------------------------------------------------
// Open file description locks (Linux-specific)
// ---------------------------------------------------------------------------

/// Get OFD lock.
pub const F_OFD_GETLK: u32 = 36;
/// Set OFD lock.
pub const F_OFD_SETLK: u32 = 37;
/// Set OFD lock (wait).
pub const F_OFD_SETLKW: u32 = 38;

// ---------------------------------------------------------------------------
// Lease commands
// ---------------------------------------------------------------------------

/// Get lease.
pub const F_GETLEASE: u32 = 1025;
/// Set lease.
pub const F_SETLEASE: u32 = 1024;

// ---------------------------------------------------------------------------
// Signal/owner commands
// ---------------------------------------------------------------------------

/// Get owner (PID).
pub const F_GETOWN: u32 = 9;
/// Set owner (PID).
pub const F_SETOWN: u32 = 8;
/// Get owner (extended).
pub const F_GETOWN_EX: u32 = 16;
/// Set owner (extended).
pub const F_SETOWN_EX: u32 = 15;
/// Get signal sent on I/O.
pub const F_GETSIG: u32 = 11;
/// Set signal sent on I/O.
pub const F_SETSIG: u32 = 10;

// ---------------------------------------------------------------------------
// Seal commands (memfd)
// ---------------------------------------------------------------------------

/// Add seals to a file.
pub const F_ADD_SEALS: u32 = 1033;
/// Get seals on a file.
pub const F_GET_SEALS: u32 = 1034;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmds_distinct() {
        let cmds = [
            F_DUPFD, F_GETFD, F_SETFD, F_GETFL, F_SETFL,
            F_GETLK, F_SETLK, F_SETLKW,
            F_GETOWN, F_SETOWN, F_GETSIG, F_SETSIG,
            F_GETLK64, F_SETLK64, F_SETLKW64,
            F_SETOWN_EX, F_GETOWN_EX,
            F_SETLEASE, F_GETLEASE,
            F_DUPFD_CLOEXEC, F_ADD_SEALS, F_GET_SEALS,
            F_OFD_GETLK, F_OFD_SETLK, F_OFD_SETLKW,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dupfd_is_zero() {
        assert_eq!(F_DUPFD, 0);
    }

    #[test]
    fn test_fd_cloexec() {
        assert_eq!(FD_CLOEXEC, 1);
    }

    #[test]
    fn test_ofd_lock_cmds() {
        assert_eq!(F_OFD_GETLK, 36);
        assert_eq!(F_OFD_SETLK, 37);
        assert_eq!(F_OFD_SETLKW, 38);
    }
}
