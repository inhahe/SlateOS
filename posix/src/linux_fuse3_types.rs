//! `<linux/fuse.h>` — Additional FUSE constants (part 3).
//!
//! Supplementary FUSE constants covering init flags,
//! notification codes, and write flags.

// ---------------------------------------------------------------------------
// FUSE init flags (additional)
// ---------------------------------------------------------------------------

/// Handle killpriv.
pub const FUSE_HANDLE_KILLPRIV: u32 = 1 << 0;
/// Posix file locks.
pub const FUSE_POSIX_LOCKS: u32 = 1 << 1;
/// File operations on directory fd.
pub const FUSE_FLOCK_LOCKS: u32 = 1 << 2;
/// Atomic O_TRUNC.
pub const FUSE_ATOMIC_O_TRUNC: u32 = 1 << 3;
/// Export support.
pub const FUSE_EXPORT_SUPPORT: u32 = 1 << 4;
/// Don't send partial write results.
pub const FUSE_BIG_WRITES: u32 = 1 << 5;
/// Auto invalidate data cache.
pub const FUSE_AUTO_INVAL_DATA: u32 = 1 << 12;
/// Do READDIRPLUS.
pub const FUSE_DO_READDIRPLUS: u32 = 1 << 13;
/// Adaptive READDIRPLUS.
pub const FUSE_READDIRPLUS_AUTO: u32 = 1 << 14;
/// Async DIO.
pub const FUSE_ASYNC_DIO: u32 = 1 << 15;
/// Writeback cache.
pub const FUSE_WRITEBACK_CACHE: u32 = 1 << 16;
/// No open support.
pub const FUSE_NO_OPEN_SUPPORT: u32 = 1 << 17;
/// Parallel directory operations.
pub const FUSE_PARALLEL_DIROPS: u32 = 1 << 18;
/// Handle killpriv v2.
pub const FUSE_HANDLE_KILLPRIV_V2: u32 = 1 << 19;

// ---------------------------------------------------------------------------
// FUSE notification codes
// ---------------------------------------------------------------------------

/// Poll notification.
pub const FUSE_NOTIFY_POLL: u32 = 1;
/// Invalidate inode.
pub const FUSE_NOTIFY_INVAL_INODE: u32 = 2;
/// Invalidate directory entry.
pub const FUSE_NOTIFY_INVAL_ENTRY: u32 = 3;
/// Store data.
pub const FUSE_NOTIFY_STORE: u32 = 4;
/// Retrieve data.
pub const FUSE_NOTIFY_RETRIEVE: u32 = 5;
/// Delete notification.
pub const FUSE_NOTIFY_DELETE: u32 = 6;
/// Resend notification.
pub const FUSE_NOTIFY_RESEND: u32 = 7;

// ---------------------------------------------------------------------------
// FUSE write flags
// ---------------------------------------------------------------------------

/// Write cache.
pub const FUSE_WRITE_CACHE: u32 = 1;
/// Write lockowner.
pub const FUSE_WRITE_LOCKOWNER: u32 = 2;
/// Write killpriv.
pub const FUSE_WRITE_KILL_SUIDGID: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_flags_power_of_two() {
        let flags = [
            FUSE_HANDLE_KILLPRIV,
            FUSE_POSIX_LOCKS,
            FUSE_FLOCK_LOCKS,
            FUSE_ATOMIC_O_TRUNC,
            FUSE_EXPORT_SUPPORT,
            FUSE_BIG_WRITES,
            FUSE_AUTO_INVAL_DATA,
            FUSE_DO_READDIRPLUS,
            FUSE_READDIRPLUS_AUTO,
            FUSE_ASYNC_DIO,
            FUSE_WRITEBACK_CACHE,
            FUSE_NO_OPEN_SUPPORT,
            FUSE_PARALLEL_DIROPS,
            FUSE_HANDLE_KILLPRIV_V2,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_init_flags_no_overlap() {
        let flags = [
            FUSE_HANDLE_KILLPRIV,
            FUSE_POSIX_LOCKS,
            FUSE_FLOCK_LOCKS,
            FUSE_ATOMIC_O_TRUNC,
            FUSE_EXPORT_SUPPORT,
            FUSE_BIG_WRITES,
            FUSE_AUTO_INVAL_DATA,
            FUSE_DO_READDIRPLUS,
            FUSE_READDIRPLUS_AUTO,
            FUSE_ASYNC_DIO,
            FUSE_WRITEBACK_CACHE,
            FUSE_NO_OPEN_SUPPORT,
            FUSE_PARALLEL_DIROPS,
            FUSE_HANDLE_KILLPRIV_V2,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_notify_codes_distinct() {
        let codes = [
            FUSE_NOTIFY_POLL,
            FUSE_NOTIFY_INVAL_INODE,
            FUSE_NOTIFY_INVAL_ENTRY,
            FUSE_NOTIFY_STORE,
            FUSE_NOTIFY_RETRIEVE,
            FUSE_NOTIFY_DELETE,
            FUSE_NOTIFY_RESEND,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_write_flags_no_overlap() {
        let flags = [
            FUSE_WRITE_CACHE,
            FUSE_WRITE_LOCKOWNER,
            FUSE_WRITE_KILL_SUIDGID,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
