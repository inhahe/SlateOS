//! `<linux/memfd.h>` — memfd_create() constants.
//!
//! memfd_create() creates an anonymous file living in RAM (backed by
//! tmpfs). It returns a file descriptor that can be shared between
//! processes for IPC, used for dynamic code loading without touching
//! the filesystem, or sealed to prevent modification (F_SEAL_*).
//! Commonly used by Wayland compositors, dynamic linkers, and JITs.

// ---------------------------------------------------------------------------
// memfd_create flags
// ---------------------------------------------------------------------------

/// Set close-on-exec on the fd.
pub const MFD_CLOEXEC: u32 = 0x0001;
/// Allow sealing operations (F_ADD_SEALS).
pub const MFD_ALLOW_SEALING: u32 = 0x0002;
/// Use hugetlb pages for the backing store.
pub const MFD_HUGETLB: u32 = 0x0004;
/// Disable exec permission (W^X enforcement).
pub const MFD_NOEXEC_SEAL: u32 = 0x0008;
/// Allow exec permission (override default noexec).
pub const MFD_EXEC: u32 = 0x0010;

// ---------------------------------------------------------------------------
// Hugetlb page size encoding (combined with MFD_HUGETLB)
// ---------------------------------------------------------------------------

/// Shift for encoding hugetlb page size in flags.
pub const MFD_HUGE_SHIFT: u32 = 26;
/// Mask for hugetlb page size bits.
pub const MFD_HUGE_MASK: u32 = 0x3F;
/// 64KB huge pages.
pub const MFD_HUGE_64KB: u32 = 16 << MFD_HUGE_SHIFT;
/// 512KB huge pages.
pub const MFD_HUGE_512KB: u32 = 19 << MFD_HUGE_SHIFT;
/// 1MB huge pages.
pub const MFD_HUGE_1MB: u32 = 20 << MFD_HUGE_SHIFT;
/// 2MB huge pages (common x86_64).
pub const MFD_HUGE_2MB: u32 = 21 << MFD_HUGE_SHIFT;
/// 8MB huge pages.
pub const MFD_HUGE_8MB: u32 = 23 << MFD_HUGE_SHIFT;
/// 16MB huge pages.
pub const MFD_HUGE_16MB: u32 = 24 << MFD_HUGE_SHIFT;
/// 32MB huge pages.
pub const MFD_HUGE_32MB: u32 = 25 << MFD_HUGE_SHIFT;
/// 256MB huge pages.
pub const MFD_HUGE_256MB: u32 = 28 << MFD_HUGE_SHIFT;
/// 512MB huge pages.
pub const MFD_HUGE_512MB: u32 = 29 << MFD_HUGE_SHIFT;
/// 1GB huge pages (x86_64 PDPE).
pub const MFD_HUGE_1GB: u32 = 30 << MFD_HUGE_SHIFT;
/// 2GB huge pages.
pub const MFD_HUGE_2GB: u32 = 31 << MFD_HUGE_SHIFT;
/// 16GB huge pages.
pub const MFD_HUGE_16GB: u32 = 34 << MFD_HUGE_SHIFT;

// ---------------------------------------------------------------------------
// File seal constants (fcntl F_ADD_SEALS / F_GET_SEALS)
// ---------------------------------------------------------------------------

/// Prevent further seals from being added.
pub const F_SEAL_SEAL: u32 = 0x0001;
/// Prevent file from shrinking.
pub const F_SEAL_SHRINK: u32 = 0x0002;
/// Prevent file from growing.
pub const F_SEAL_GROW: u32 = 0x0004;
/// Prevent writes to the file.
pub const F_SEAL_WRITE: u32 = 0x0008;
/// Prevent writes via future shared mappings.
pub const F_SEAL_FUTURE_WRITE: u32 = 0x0010;
/// Allow only writes via exec (W^X).
pub const F_SEAL_EXEC: u32 = 0x0020;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memfd_flags_no_overlap() {
        let flags = [
            MFD_CLOEXEC,
            MFD_ALLOW_SEALING,
            MFD_HUGETLB,
            MFD_NOEXEC_SEAL,
            MFD_EXEC,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_huge_sizes_distinct() {
        let sizes = [
            MFD_HUGE_64KB,
            MFD_HUGE_512KB,
            MFD_HUGE_1MB,
            MFD_HUGE_2MB,
            MFD_HUGE_8MB,
            MFD_HUGE_16MB,
            MFD_HUGE_32MB,
            MFD_HUGE_256MB,
            MFD_HUGE_512MB,
            MFD_HUGE_1GB,
            MFD_HUGE_2GB,
            MFD_HUGE_16GB,
        ];
        for i in 0..sizes.len() {
            for j in (i + 1)..sizes.len() {
                assert_ne!(sizes[i], sizes[j]);
            }
        }
    }

    #[test]
    fn test_huge_size_encoding() {
        // 2MB = order 21
        assert_eq!(MFD_HUGE_2MB >> MFD_HUGE_SHIFT, 21);
        // 1GB = order 30
        assert_eq!(MFD_HUGE_1GB >> MFD_HUGE_SHIFT, 30);
    }

    #[test]
    fn test_seal_flags_no_overlap() {
        let seals = [
            F_SEAL_SEAL,
            F_SEAL_SHRINK,
            F_SEAL_GROW,
            F_SEAL_WRITE,
            F_SEAL_FUTURE_WRITE,
            F_SEAL_EXEC,
        ];
        for i in 0..seals.len() {
            assert!(seals[i].is_power_of_two());
            for j in (i + 1)..seals.len() {
                assert_eq!(seals[i] & seals[j], 0);
            }
        }
    }

    #[test]
    fn test_huge_shift_and_mask() {
        assert_eq!(MFD_HUGE_SHIFT, 26);
        assert_eq!(MFD_HUGE_MASK, 0x3F);
    }
}
