//! `<linux/uio.h>` — Process VM and I/O vector constants.
//!
//! Constants for process_vm_readv/writev, readv/writev,
//! and preadv2/pwritev2 operations.

// ---------------------------------------------------------------------------
// preadv2/pwritev2 flags (RWF_*)
// ---------------------------------------------------------------------------

/// High priority I/O.
pub const RWF_HIPRI: u32 = 0x00000001;
/// Per-I/O O_DSYNC.
pub const RWF_DSYNC: u32 = 0x00000002;
/// Per-I/O O_SYNC.
pub const RWF_SYNC: u32 = 0x00000004;
/// Per-I/O non-blocking.
pub const RWF_NOWAIT: u32 = 0x00000008;
/// Per-I/O O_APPEND.
pub const RWF_APPEND: u32 = 0x00000010;

// ---------------------------------------------------------------------------
// IOV limits
// ---------------------------------------------------------------------------

/// Maximum iovec count for readv/writev.
pub const UIO_MAXIOV: u32 = 1024;
/// Fast IOV count (on-stack allocation).
pub const UIO_FASTIOV: u32 = 8;

// ---------------------------------------------------------------------------
// Process VM operation flags
// ---------------------------------------------------------------------------

/// No flags.
pub const PROCESS_VM_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Vectored I/O direction
// ---------------------------------------------------------------------------

/// Read direction.
pub const UIO_READ: u32 = 0;
/// Write direction.
pub const UIO_WRITE: u32 = 1;

// ---------------------------------------------------------------------------
// Scatter-gather limits
// ---------------------------------------------------------------------------

/// Max segments for SG I/O.
pub const SG_MAX_SEGMENTS: u32 = 128;
/// Max single segment size.
pub const SG_MAX_SINGLE_ALLOC: u32 = 65536;

// ---------------------------------------------------------------------------
// copy_file_range flags
// ---------------------------------------------------------------------------

/// No flags.
pub const COPY_FILE_RANGE_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// madvise hint constants
// ---------------------------------------------------------------------------

/// Normal behavior.
pub const MADV_NORMAL: u32 = 0;
/// Random access.
pub const MADV_RANDOM: u32 = 1;
/// Sequential access.
pub const MADV_SEQUENTIAL: u32 = 2;
/// Will need pages.
pub const MADV_WILLNEED: u32 = 3;
/// Don't need pages.
pub const MADV_DONTNEED: u32 = 4;
/// Free pages.
pub const MADV_FREE: u32 = 8;
/// Remove pages.
pub const MADV_REMOVE: u32 = 9;
/// Don't fork.
pub const MADV_DONTFORK: u32 = 10;
/// Do fork.
pub const MADV_DOFORK: u32 = 11;
/// Mergeable (KSM).
pub const MADV_MERGEABLE: u32 = 12;
/// Unmergeable.
pub const MADV_UNMERGEABLE: u32 = 13;
/// Hugepage.
pub const MADV_HUGEPAGE: u32 = 14;
/// No hugepage.
pub const MADV_NOHUGEPAGE: u32 = 15;
/// Don't dump.
pub const MADV_DONTDUMP: u32 = 16;
/// Do dump.
pub const MADV_DODUMP: u32 = 17;
/// Cold pages.
pub const MADV_COLD: u32 = 20;
/// Pageout.
pub const MADV_PAGEOUT: u32 = 21;
/// Populate read.
pub const MADV_POPULATE_READ: u32 = 22;
/// Populate write.
pub const MADV_POPULATE_WRITE: u32 = 23;
/// Dontneed locked.
pub const MADV_DONTNEED_LOCKED: u32 = 24;
/// Collapse.
pub const MADV_COLLAPSE: u32 = 25;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rwf_flags_power_of_two() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_rwf_flags_no_overlap() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_iov_limits() {
        assert_eq!(UIO_MAXIOV, 1024);
        assert!(UIO_FASTIOV < UIO_MAXIOV);
    }

    #[test]
    fn test_uio_direction() {
        assert_eq!(UIO_READ, 0);
        assert_eq!(UIO_WRITE, 1);
    }

    #[test]
    fn test_madvise_distinct() {
        let hints = [
            MADV_NORMAL,
            MADV_RANDOM,
            MADV_SEQUENTIAL,
            MADV_WILLNEED,
            MADV_DONTNEED,
            MADV_FREE,
            MADV_REMOVE,
            MADV_DONTFORK,
            MADV_DOFORK,
            MADV_MERGEABLE,
            MADV_UNMERGEABLE,
            MADV_HUGEPAGE,
            MADV_NOHUGEPAGE,
            MADV_DONTDUMP,
            MADV_DODUMP,
            MADV_COLD,
            MADV_PAGEOUT,
            MADV_POPULATE_READ,
            MADV_POPULATE_WRITE,
            MADV_DONTNEED_LOCKED,
            MADV_COLLAPSE,
        ];
        for i in 0..hints.len() {
            for j in (i + 1)..hints.len() {
                assert_ne!(hints[i], hints[j]);
            }
        }
    }
}
