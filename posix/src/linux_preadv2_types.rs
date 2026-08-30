//! `<sys/uio.h>` — preadv2/pwritev2 flag constants.
//!
//! `preadv2()` and `pwritev2()` extend the scatter/gather I/O
//! interface with per-call flags.  These constants define the
//! flag bits and related iovec limits.

// ---------------------------------------------------------------------------
// preadv2/pwritev2 flags (RWF_*)
// ---------------------------------------------------------------------------

/// High-priority I/O (attempt block-layer polling).
pub const RWF_HIPRI: u32 = 0x00000001;
/// Per-I/O O_DSYNC (data integrity write).
pub const RWF_DSYNC: u32 = 0x00000002;
/// Per-I/O O_SYNC (file integrity write).
pub const RWF_SYNC: u32 = 0x00000004;
/// Do not wait for I/O completion (non-blocking).
pub const RWF_NOWAIT: u32 = 0x00000008;
/// Per-I/O O_APPEND (write at end of file).
pub const RWF_APPEND: u32 = 0x00000010;

// ---------------------------------------------------------------------------
// iovec limits
// ---------------------------------------------------------------------------

/// Maximum number of iovec entries per readv/writev call.
pub const UIO_MAXIOV: u32 = 1024;
/// Alias for UIO_MAXIOV (POSIX name).
pub const IOV_MAX: u32 = 1024;

// ---------------------------------------------------------------------------
// struct iovec layout (Linux x86_64)
// ---------------------------------------------------------------------------

/// Offset of iov_base in struct iovec.
pub const IOVEC_OFF_BASE: u32 = 0;
/// Offset of iov_len in struct iovec.
pub const IOVEC_OFF_LEN: u32 = 8;
/// Size of struct iovec on x86_64 (bytes).
pub const IOVEC_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// readv/writev total byte limits
// ---------------------------------------------------------------------------

/// Maximum total bytes per readv/writev (2 GiB - 1 page, Linux).
pub const READV_MAX_BYTES: u64 = 0x7FFFF000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rwf_flags_powers_of_two() {
        let flags = [RWF_HIPRI, RWF_DSYNC, RWF_SYNC, RWF_NOWAIT, RWF_APPEND];
        for f in flags {
            assert!(f.is_power_of_two());
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
    fn test_hipri_is_one() {
        assert_eq!(RWF_HIPRI, 1);
    }

    #[test]
    fn test_uio_maxiov() {
        assert_eq!(UIO_MAXIOV, 1024);
    }

    #[test]
    fn test_iov_max_matches() {
        assert_eq!(IOV_MAX, UIO_MAXIOV);
    }

    #[test]
    fn test_iovec_layout() {
        assert_eq!(IOVEC_OFF_BASE, 0);
        assert_eq!(IOVEC_OFF_LEN, 8);
        assert_eq!(IOVEC_SIZE, 16);
    }

    #[test]
    fn test_iovec_offsets_ascending() {
        assert!(IOVEC_OFF_LEN > IOVEC_OFF_BASE);
    }

    #[test]
    fn test_readv_max_bytes() {
        assert_eq!(READV_MAX_BYTES, 0x7FFFF000);
    }
}
