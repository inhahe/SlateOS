//! `<linux/fcntl.h>` — file control constants (kernel view).
//!
//! Re-exports standard fcntl operations and O_* flags from the POSIX
//! modules, and adds Linux-specific extensions: OFD locks, file seals
//! (for memfd), and pipe capacity control.

// ---------------------------------------------------------------------------
// Re-exports from fcntl_ops (F_* commands)
// ---------------------------------------------------------------------------

pub use crate::fcntl_ops::F_DUPFD;
pub use crate::fcntl_ops::F_DUPFD_CLOEXEC;
pub use crate::fcntl_ops::F_GETFD;
pub use crate::fcntl_ops::F_GETFL;
pub use crate::fcntl_ops::F_GETLK;
pub use crate::fcntl_ops::F_SETFD;
pub use crate::fcntl_ops::F_SETFL;
pub use crate::fcntl_ops::F_SETLK;
pub use crate::fcntl_ops::F_SETLKW;

// ---------------------------------------------------------------------------
// Re-exports from fcntl (O_* flags)
// ---------------------------------------------------------------------------

pub use crate::fcntl::O_APPEND;
pub use crate::fcntl::O_CREAT;
pub use crate::fcntl::O_EXCL;
pub use crate::fcntl::O_RDONLY;
pub use crate::fcntl::O_RDWR;
pub use crate::fcntl::O_TRUNC;
pub use crate::fcntl::O_WRONLY;

// ---------------------------------------------------------------------------
// Re-exports from fdtable (FD_CLOEXEC)
// ---------------------------------------------------------------------------

pub use crate::fdtable::FD_CLOEXEC;

// ---------------------------------------------------------------------------
// OFD (Open File Description) locks — Linux 3.15+
// ---------------------------------------------------------------------------

/// Get OFD lock.
pub const F_OFD_GETLK: i32 = 36;
/// Set OFD lock (non-blocking).
pub const F_OFD_SETLK: i32 = 37;
/// Set OFD lock (blocking).
pub const F_OFD_SETLKW: i32 = 38;

// ---------------------------------------------------------------------------
// File seals (memfd_create) — Linux 3.17+
// ---------------------------------------------------------------------------

/// Add seals to a file descriptor.
pub const F_ADD_SEALS: i32 = 1033;
/// Get seals from a file descriptor.
pub const F_GET_SEALS: i32 = 1034;

/// Seal: prevent further sealing.
pub const F_SEAL_SEAL: u32 = 0x0001;
/// Seal: prevent shrinking.
pub const F_SEAL_SHRINK: u32 = 0x0002;
/// Seal: prevent growing.
pub const F_SEAL_GROW: u32 = 0x0004;
/// Seal: prevent writes.
pub const F_SEAL_WRITE: u32 = 0x0008;
/// Seal: prevent future writes (Linux 5.1+).
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;
/// Seal: allow exec (Linux 6.3+).
pub const F_SEAL_EXEC: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Pipe capacity — Linux 2.6.35+
// ---------------------------------------------------------------------------

/// Get pipe capacity.
pub const F_GETPIPE_SZ: i32 = 1032;
/// Set pipe capacity.
pub const F_SETPIPE_SZ: i32 = 1031;

// ---------------------------------------------------------------------------
// File lease and notification — Linux extensions
// ---------------------------------------------------------------------------

/// Set file lease.
pub const F_SETLEASE: i32 = 1024;
/// Get file lease.
pub const F_GETLEASE: i32 = 1025;
/// File notification (dnotify).
pub const F_NOTIFY: i32 = 1026;

/// Set file owner (for SIGIO).
pub const F_SETOWN_EX: i32 = 15;
/// Get file owner.
pub const F_GETOWN_EX: i32 = 16;

// ---------------------------------------------------------------------------
// Dnotify events
// ---------------------------------------------------------------------------

/// File accessed.
pub const DN_ACCESS: u32 = 0x00000001;
/// File modified.
pub const DN_MODIFY: u32 = 0x00000002;
/// File created.
pub const DN_CREATE: u32 = 0x00000004;
/// File deleted.
pub const DN_DELETE: u32 = 0x00000008;
/// File renamed.
pub const DN_RENAME: u32 = 0x00000010;
/// File attributes changed.
pub const DN_ATTRIB: u32 = 0x00000020;
/// Use multishot (don't unregister after first event).
pub const DN_MULTISHOT: u32 = 0x80000000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_fcntl_reexports() {
        assert_eq!(F_DUPFD, 0);
        assert_eq!(F_GETFD, 1);
        assert_eq!(F_SETFD, 2);
        assert_eq!(F_GETFL, 3);
        assert_eq!(F_SETFL, 4);
    }

    #[test]
    fn test_ofd_locks() {
        assert_eq!(F_OFD_GETLK, 36);
        assert_eq!(F_OFD_SETLK, 37);
        assert_eq!(F_OFD_SETLKW, 38);
    }

    #[test]
    fn test_seals_powers_of_two() {
        let seals = [
            F_SEAL_SEAL,
            F_SEAL_SHRINK,
            F_SEAL_GROW,
            F_SEAL_WRITE,
            F_SEAL_FUTURE_WRITE,
            F_SEAL_EXEC,
        ];
        for s in &seals {
            assert!(s.is_power_of_two(), "seal {s:#x} not power of 2");
        }
    }

    #[test]
    fn test_seals_distinct() {
        let seals = [
            F_SEAL_SEAL,
            F_SEAL_SHRINK,
            F_SEAL_GROW,
            F_SEAL_WRITE,
            F_SEAL_FUTURE_WRITE,
            F_SEAL_EXEC,
        ];
        for i in 0..seals.len() {
            for j in (i + 1)..seals.len() {
                assert_ne!(seals[i], seals[j]);
            }
        }
    }

    #[test]
    fn test_pipe_sz() {
        assert_ne!(F_GETPIPE_SZ, F_SETPIPE_SZ);
    }

    #[test]
    fn test_dnotify_flags_powers_of_two() {
        let flags = [
            DN_ACCESS, DN_MODIFY, DN_CREATE, DN_DELETE, DN_RENAME, DN_ATTRIB,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "DN flag {f:#x} not power of 2");
        }
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(F_DUPFD, crate::fcntl_ops::F_DUPFD);
        assert_eq!(O_RDONLY, crate::fcntl::O_RDONLY);
        assert_eq!(FD_CLOEXEC, crate::fdtable::FD_CLOEXEC);
    }
}
