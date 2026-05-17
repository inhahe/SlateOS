//! `<linux/close_range.h>` — close_range() constants.
//!
//! close_range() closes all file descriptors in a given range
//! [first, last] atomically and efficiently. Introduced in Linux 5.9,
//! it replaces the common pattern of looping close() from 3 to
//! sysconf(_SC_OPEN_MAX). Used by process spawning code and
//! sandboxing to clean up inherited descriptors.

// ---------------------------------------------------------------------------
// close_range flags
// ---------------------------------------------------------------------------

/// Unshare the fd table before closing (CLONE_FILES undo).
pub const CLOSE_RANGE_UNSHARE: u32 = 1 << 1;
/// Set CLOEXEC instead of closing (defer close to exec).
pub const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Common ranges
// ---------------------------------------------------------------------------

/// Close all fds from `first` to this value (meaning "to infinity").
pub const CLOSE_RANGE_MAX: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// close_range syscall number (x86_64)
// ---------------------------------------------------------------------------

/// Syscall number for close_range on x86_64.
pub const SYS_CLOSE_RANGE_X86_64: u32 = 436;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(CLOSE_RANGE_UNSHARE & CLOSE_RANGE_CLOEXEC, 0);
    }

    #[test]
    fn test_flags_are_power_of_two() {
        assert!(CLOSE_RANGE_UNSHARE.is_power_of_two());
        assert!(CLOSE_RANGE_CLOEXEC.is_power_of_two());
    }

    #[test]
    fn test_close_range_max() {
        assert_eq!(CLOSE_RANGE_MAX, u32::MAX);
    }

    #[test]
    fn test_syscall_number() {
        assert_eq!(SYS_CLOSE_RANGE_X86_64, 436);
    }

    #[test]
    fn test_flags_values() {
        assert_eq!(CLOSE_RANGE_UNSHARE, 2);
        assert_eq!(CLOSE_RANGE_CLOEXEC, 4);
    }
}
