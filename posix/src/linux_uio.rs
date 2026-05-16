//! `<linux/uio.h>` — kernel vectored I/O extensions.
//!
//! Re-exports the base `Iovec` from `file` and adds Linux-specific
//! constants for `preadv2`/`pwritev2` flags.

pub use crate::file::Iovec;
pub use crate::file::readv;
pub use crate::file::writev;
pub use crate::file::preadv;
pub use crate::file::pwritev;
pub use crate::file::preadv2;
pub use crate::file::pwritev2;
pub use crate::sys_uio::UIO_MAXIOV;

// ---------------------------------------------------------------------------
// preadv2/pwritev2 flags (RWF_*)
// ---------------------------------------------------------------------------

/// High-priority I/O (may use polling).
pub const RWF_HIPRI: i32 = 0x0001;
/// Issue I/O per-data-segment.
pub const RWF_DSYNC: i32 = 0x0002;
/// Issue I/O with sync semantics.
pub const RWF_SYNC: i32 = 0x0004;
/// Don't wait for I/O to complete.
pub const RWF_NOWAIT: i32 = 0x0008;
/// Append data to file.
pub const RWF_APPEND: i32 = 0x0010;

// ---------------------------------------------------------------------------
// UIO_FASTIOV — number of iovec entries on stack
// ---------------------------------------------------------------------------

/// Number of iovec that can be allocated on the stack before
/// falling back to heap allocation.
pub const UIO_FASTIOV: usize = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rwf_flags_are_bits() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0, "RWF_ flags must not overlap");
            }
        }
    }

    #[test]
    fn test_rwf_combinable() {
        let combined = RWF_DSYNC | RWF_NOWAIT;
        assert_ne!(combined, RWF_DSYNC);
        assert_ne!(combined, RWF_NOWAIT);
    }

    #[test]
    fn test_uio_fastiov() {
        assert_eq!(UIO_FASTIOV, 8);
    }

    #[test]
    fn test_uio_maxiov() {
        assert_eq!(UIO_MAXIOV, 1024);
    }

    #[test]
    fn test_iovec_size() {
        // ptr + usize = 16 bytes on 64-bit.
        assert_eq!(core::mem::size_of::<Iovec>(), 16);
    }

    #[test]
    fn test_cross_module() {
        assert_eq!(UIO_MAXIOV, crate::sys_uio::UIO_MAXIOV);
    }
}
