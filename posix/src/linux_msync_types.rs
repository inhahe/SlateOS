//! `<sys/mman.h>` — Memory synchronization flag constants.
//!
//! These flags control the `msync()` syscall which flushes changes
//! made to a memory-mapped file region back to the underlying
//! storage device. The flags determine whether the operation is
//! synchronous or asynchronous and whether it invalidates caches.

// ---------------------------------------------------------------------------
// msync() flags
// ---------------------------------------------------------------------------

/// Schedule sync but return immediately.
pub const MS_ASYNC: u32 = 1;
/// Invalidate other file mappings.
pub const MS_INVALIDATE: u32 = 2;
/// Block until sync completes.
pub const MS_SYNC: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ms_flags_no_overlap() {
        let flags = [MS_ASYNC, MS_INVALIDATE, MS_SYNC];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ms_power_of_two() {
        assert!(MS_ASYNC.is_power_of_two());
        assert!(MS_INVALIDATE.is_power_of_two());
        assert!(MS_SYNC.is_power_of_two());
    }

    #[test]
    fn test_ms_values() {
        assert_eq!(MS_ASYNC, 1);
        assert_eq!(MS_INVALIDATE, 2);
        assert_eq!(MS_SYNC, 4);
    }

    #[test]
    fn test_ms_async_and_sync_exclusive() {
        // MS_ASYNC and MS_SYNC are mutually exclusive
        assert_eq!(MS_ASYNC & MS_SYNC, 0);
    }
}
