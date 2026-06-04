//! `<linux/close_range.h>` — close_range(2) syscall constants.
//!
//! `close_range(first, last, flags)` closes every file descriptor in
//! the inclusive range [first, last]. It supersedes the older
//! "close all FDs above N" idiom that required walking /proc/self/fd
//! or using fork+exec workarounds.

// ---------------------------------------------------------------------------
// close_range() flags
// ---------------------------------------------------------------------------

/// Unshare the FD table before closing (so other threads keep theirs).
pub const CLOSE_RANGE_UNSHARE: u32 = 1 << 1;
/// Mark the range close-on-exec instead of closing.
pub const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;

/// Bitmask of all valid close_range flags.
pub const CLOSE_RANGE_FLAGS_MASK: u32 = CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC;

// ---------------------------------------------------------------------------
// "Close all from N onward" sentinel
// ---------------------------------------------------------------------------

/// Use `~0u` as `last` to mean "all remaining FDs above first".
pub const CLOSE_RANGE_MAX: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Syscall numbers
// ---------------------------------------------------------------------------

pub const NR_CLOSE_RANGE_X86_64: u32 = 436;
pub const NR_CLOSE_RANGE_AARCH64: u32 = 436;
pub const NR_CLOSE_RANGE_RISCV: u32 = 436;

// ---------------------------------------------------------------------------
// Glibc minimum kernel version
// ---------------------------------------------------------------------------

/// close_range() was added in kernel 5.9 (2020).
pub const CLOSE_RANGE_MIN_KERNEL_MAJOR: u32 = 5;
pub const CLOSE_RANGE_MIN_KERNEL_MINOR: u32 = 9;

/// CLOSE_RANGE_CLOEXEC was added in kernel 5.11.
pub const CLOSE_RANGE_CLOEXEC_MIN_KERNEL_MINOR: u32 = 11;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct_single_bit() {
        assert!(CLOSE_RANGE_UNSHARE.is_power_of_two());
        assert!(CLOSE_RANGE_CLOEXEC.is_power_of_two());
        assert_eq!(CLOSE_RANGE_UNSHARE & CLOSE_RANGE_CLOEXEC, 0);
        assert_eq!(CLOSE_RANGE_UNSHARE, 1 << 1);
        assert_eq!(CLOSE_RANGE_CLOEXEC, 1 << 2);
    }

    #[test]
    fn test_flags_mask_is_or_of_all() {
        assert_eq!(
            CLOSE_RANGE_FLAGS_MASK,
            CLOSE_RANGE_UNSHARE | CLOSE_RANGE_CLOEXEC,
        );
        assert_eq!(CLOSE_RANGE_FLAGS_MASK.count_ones(), 2);
    }

    #[test]
    fn test_max_sentinel_is_u32_max() {
        assert_eq!(CLOSE_RANGE_MAX, u32::MAX);
        // ~0u in C maps to UINT_MAX = 0xFFFFFFFF.
        assert_eq!(CLOSE_RANGE_MAX, 0xFFFF_FFFF);
    }

    #[test]
    fn test_syscall_number_436_across_arches() {
        // close_range() was added uniformly at 436 on modern arches.
        assert_eq!(NR_CLOSE_RANGE_X86_64, 436);
        assert_eq!(NR_CLOSE_RANGE_AARCH64, 436);
        assert_eq!(NR_CLOSE_RANGE_RISCV, 436);
    }

    #[test]
    fn test_min_kernel_version_5_9() {
        assert_eq!(CLOSE_RANGE_MIN_KERNEL_MAJOR, 5);
        assert_eq!(CLOSE_RANGE_MIN_KERNEL_MINOR, 9);
        // CLOEXEC came two releases later.
        assert!(
            CLOSE_RANGE_CLOEXEC_MIN_KERNEL_MINOR > CLOSE_RANGE_MIN_KERNEL_MINOR
        );
    }

    #[test]
    fn test_bit_0_unused() {
        // Bit 0 is reserved (close_range starts at bit 1).
        assert_eq!(CLOSE_RANGE_FLAGS_MASK & 1, 0);
    }
}
