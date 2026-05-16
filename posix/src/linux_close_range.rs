//! `<linux/close_range.h>` — close_range() flags.
//!
//! Re-exports `close_range()` from the `file` module and provides
//! the flag constants.

pub use crate::file::close_range;

// ---------------------------------------------------------------------------
// close_range flags
// ---------------------------------------------------------------------------

/// Unshare the file descriptor table before closing.
///
/// Creates a new fd table (like `CLONE_FILES` in reverse),
/// then closes the specified range in the new table.
pub const CLOSE_RANGE_UNSHARE: u32 = 1 << 1;

/// Set close-on-exec on the range instead of closing.
pub const CLOSE_RANGE_CLOEXEC: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_close_range_flags() {
        assert_eq!(CLOSE_RANGE_UNSHARE, 2);
        assert_eq!(CLOSE_RANGE_CLOEXEC, 4);
        assert_eq!(CLOSE_RANGE_UNSHARE & CLOSE_RANGE_CLOEXEC, 0);
    }

    #[test]
    fn test_close_range_stub() {
        // Closing a range that's almost certainly not open.
        let ret = close_range(500, 600, 0);
        // Should succeed (nothing to close).
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_close_range_max() {
        // Common pattern: close everything from fd 3 onward.
        let _ = close_range(3, u32::MAX, 0);
    }
}
