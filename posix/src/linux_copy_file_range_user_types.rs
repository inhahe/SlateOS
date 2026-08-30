//! `<unistd.h>` — copy_file_range(2) syscall constants.
//!
//! `copy_file_range(in, &off_in, out, &off_out, len, flags)` performs
//! an in-kernel copy between two open file descriptors. On filesystems
//! that support it (XFS, btrfs, NFS), the copy can be reflinked (no
//! data movement at all).

// ---------------------------------------------------------------------------
// Flags argument — currently reserved, must be 0.
// ---------------------------------------------------------------------------

/// `flags` must be 0 (kernel currently rejects any non-zero value).
pub const COPY_FILE_RANGE_FLAGS_RESERVED: u32 = 0;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_COPY_FILE_RANGE_X86_64: u32 = 326;
pub const NR_COPY_FILE_RANGE_AARCH64: u32 = 285;
pub const NR_COPY_FILE_RANGE_I386: u32 = 377;
pub const NR_COPY_FILE_RANGE_RISCV: u32 = 285;

// ---------------------------------------------------------------------------
// Minimum kernel version with copy_file_range
// ---------------------------------------------------------------------------

/// Added in kernel 4.5 (2016).
pub const COPY_FILE_RANGE_MIN_KERNEL_MAJOR: u32 = 4;
pub const COPY_FILE_RANGE_MIN_KERNEL_MINOR: u32 = 5;

/// Cross-filesystem support added in 5.3.
pub const COPY_FILE_RANGE_CROSS_FS_MIN_MINOR: u32 = 3;
pub const COPY_FILE_RANGE_CROSS_FS_MIN_MAJOR: u32 = 5;

// ---------------------------------------------------------------------------
// Practical chunk size for userspace fallback loops
// ---------------------------------------------------------------------------

/// Recommended chunk size for fallback copy loops (1 MiB).
pub const COPY_FILE_RANGE_FALLBACK_CHUNK: usize = 1024 * 1024;

/// Maximum bytes the kernel will copy in one call (SSIZE_MAX-ish).
pub const COPY_FILE_RANGE_MAX_BYTES: isize = isize::MAX;

// ---------------------------------------------------------------------------
// Errors specific to copy_file_range (errno values returned)
// ---------------------------------------------------------------------------

/// EXDEV — source and dest on different filesystems (pre-5.3).
pub const COPY_FILE_RANGE_EXDEV: i32 = 18;
/// EINVAL — flags non-zero or invalid offsets.
pub const COPY_FILE_RANGE_EINVAL: i32 = 22;
/// ENOSPC — destination out of space.
pub const COPY_FILE_RANGE_ENOSPC: i32 = 28;
/// EOPNOTSUPP — filesystem doesn't implement it.
pub const COPY_FILE_RANGE_EOPNOTSUPP: i32 = 95;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_reserved_zero() {
        assert_eq!(COPY_FILE_RANGE_FLAGS_RESERVED, 0);
    }

    #[test]
    fn test_syscall_numbers_per_arch_distinct() {
        let n = [
            NR_COPY_FILE_RANGE_X86_64,
            NR_COPY_FILE_RANGE_AARCH64,
            NR_COPY_FILE_RANGE_I386,
            NR_COPY_FILE_RANGE_RISCV,
        ];
        // x86_64 and i386 differ; aarch64 == riscv (both use generic).
        assert_eq!(NR_COPY_FILE_RANGE_AARCH64, NR_COPY_FILE_RANGE_RISCV);
        assert_ne!(NR_COPY_FILE_RANGE_X86_64, NR_COPY_FILE_RANGE_AARCH64);
        assert_ne!(NR_COPY_FILE_RANGE_X86_64, NR_COPY_FILE_RANGE_I386);
        for v in n {
            assert!(v > 0);
        }
    }

    #[test]
    fn test_kernel_versions_ordered() {
        // Initial 4.5; cross-FS in 5.3.
        assert_eq!(COPY_FILE_RANGE_MIN_KERNEL_MAJOR, 4);
        assert_eq!(COPY_FILE_RANGE_MIN_KERNEL_MINOR, 5);
        assert_eq!(COPY_FILE_RANGE_CROSS_FS_MIN_MAJOR, 5);
        assert_eq!(COPY_FILE_RANGE_CROSS_FS_MIN_MINOR, 3);
        // 5.3 > 4.5 as a (major, minor) tuple.
        assert!(COPY_FILE_RANGE_CROSS_FS_MIN_MAJOR > COPY_FILE_RANGE_MIN_KERNEL_MAJOR);
    }

    #[test]
    fn test_fallback_chunk_is_1mib() {
        assert_eq!(COPY_FILE_RANGE_FALLBACK_CHUNK, 1024 * 1024);
        assert!(COPY_FILE_RANGE_FALLBACK_CHUNK.is_power_of_two());
    }

    #[test]
    fn test_max_bytes_is_ssize_max() {
        assert_eq!(COPY_FILE_RANGE_MAX_BYTES, isize::MAX);
    }

    #[test]
    fn test_errno_values_distinct() {
        let e = [
            COPY_FILE_RANGE_EXDEV,
            COPY_FILE_RANGE_EINVAL,
            COPY_FILE_RANGE_ENOSPC,
            COPY_FILE_RANGE_EOPNOTSUPP,
        ];
        for (i, &x) in e.iter().enumerate() {
            for &y in &e[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // Standard errno values from <errno.h>.
        assert_eq!(COPY_FILE_RANGE_EXDEV, 18);
        assert_eq!(COPY_FILE_RANGE_EINVAL, 22);
        assert_eq!(COPY_FILE_RANGE_ENOSPC, 28);
        assert_eq!(COPY_FILE_RANGE_EOPNOTSUPP, 95);
    }
}
