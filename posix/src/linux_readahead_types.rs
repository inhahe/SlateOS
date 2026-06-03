//! `<linux/mm.h>` (readahead subset) — Read-ahead constants.
//!
//! The kernel read-ahead subsystem speculatively reads file pages
//! into the page cache before they are explicitly requested. This
//! hides I/O latency for sequential access patterns. The kernel
//! automatically detects sequential reads and grows the read-ahead
//! window. Applications can also trigger explicit read-ahead via
//! the `readahead()` syscall or `madvise(MADV_WILLNEED)`.

// ---------------------------------------------------------------------------
// Read-ahead window sizes (in pages)
// ---------------------------------------------------------------------------

/// Initial read-ahead size for a new file stream (4 pages).
pub const RA_INIT_PAGES: u32 = 4;
/// Read-ahead grows geometrically until this maximum (256 pages).
pub const RA_MAX_PAGES_DEFAULT: u32 = 256;
/// Async read-ahead trigger point (fire read-ahead when this many
/// pages remain unread in the current window).
pub const RA_ASYNC_TRIGGER: u32 = 4;

// ---------------------------------------------------------------------------
// Read-ahead states (kernel internal)
// ---------------------------------------------------------------------------

/// Initial state (no read-ahead pattern detected yet).
pub const RA_STATE_INITIAL: u32 = 0;
/// Sequential pattern detected, read-ahead active.
pub const RA_STATE_SEQUENTIAL: u32 = 1;
/// Interleaved sequential streams detected.
pub const RA_STATE_INTERLEAVED: u32 = 2;
/// Read-ahead was triggered by mmap fault.
pub const RA_STATE_MMAP: u32 = 3;
/// Read-ahead disabled (random access detected).
pub const RA_STATE_DISABLED: u32 = 4;

// ---------------------------------------------------------------------------
// Read-ahead control flags
// ---------------------------------------------------------------------------

/// Mark read-ahead pages (for detection of sequential access).
pub const RA_FLAG_MARK: u32 = 0x01;
/// Asynchronous read-ahead (don't wait for I/O).
pub const RA_FLAG_ASYNC: u32 = 0x02;
/// Read-ahead triggered by mmap page fault.
pub const RA_FLAG_MMAP: u32 = 0x04;
/// Read-ahead miss detected (shrink window).
pub const RA_FLAG_MISS: u32 = 0x08;
/// Read-ahead was explicitly requested (readahead syscall).
pub const RA_FLAG_EXPLICIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_sizes_ordered() {
        assert!(RA_INIT_PAGES > 0);
        assert!(RA_MAX_PAGES_DEFAULT > RA_INIT_PAGES);
        assert!(RA_ASYNC_TRIGGER > 0);
        assert!(RA_ASYNC_TRIGGER <= RA_INIT_PAGES);
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            RA_STATE_INITIAL,
            RA_STATE_SEQUENTIAL,
            RA_STATE_INTERLEAVED,
            RA_STATE_MMAP,
            RA_STATE_DISABLED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            RA_FLAG_MARK,
            RA_FLAG_ASYNC,
            RA_FLAG_MMAP,
            RA_FLAG_MISS,
            RA_FLAG_EXPLICIT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
