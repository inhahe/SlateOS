//! `<linux/cachestat.h>` — `cachestat()` syscall (sys 451 on x86-64).
//!
//! `cachestat()` reports per-range page-cache statistics for a file
//! descriptor: how many pages are cached, dirty, writeback,
//! evicted, and recently evicted. It is the userspace-visible
//! replacement for parsing `/proc/<pid>/pagemap` to estimate cache
//! residency.

// ---------------------------------------------------------------------------
// Syscall number (x86-64)
// ---------------------------------------------------------------------------

/// `__NR_cachestat` on x86-64.
pub const NR_CACHESTAT_X86_64: u32 = 451;

// ---------------------------------------------------------------------------
// `struct cachestat_range` field offsets
// ---------------------------------------------------------------------------

pub const CACHESTAT_RANGE_OFF_OFF: usize = 0;
pub const CACHESTAT_RANGE_OFF_LEN: usize = 8;
pub const CACHESTAT_RANGE_SIZE: usize = 16;

// ---------------------------------------------------------------------------
// `struct cachestat` field offsets (six packed u64 fields)
// ---------------------------------------------------------------------------

pub const CACHESTAT_OFF_NR_CACHE: usize = 0;
pub const CACHESTAT_OFF_NR_DIRTY: usize = 8;
pub const CACHESTAT_OFF_NR_WRITEBACK: usize = 16;
pub const CACHESTAT_OFF_NR_EVICTED: usize = 24;
pub const CACHESTAT_OFF_NR_RECENTLY_EVICTED: usize = 32;
pub const CACHESTAT_SIZE: usize = 40;

// ---------------------------------------------------------------------------
// Flags
// ---------------------------------------------------------------------------

/// All flag bits — currently the kernel rejects any non-zero flags.
pub const CACHESTAT_FLAGS_RESERVED: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_number_x86_64() {
        assert_eq!(NR_CACHESTAT_X86_64, 451);
    }

    #[test]
    fn test_range_layout_two_u64s() {
        assert_eq!(CACHESTAT_RANGE_OFF_OFF, 0);
        assert_eq!(CACHESTAT_RANGE_OFF_LEN, 8);
        assert_eq!(CACHESTAT_RANGE_SIZE, 16);
        // off and len are u64, packed.
        assert_eq!(CACHESTAT_RANGE_OFF_LEN - CACHESTAT_RANGE_OFF_OFF, 8);
        assert_eq!(CACHESTAT_RANGE_SIZE, 2 * 8);
    }

    #[test]
    fn test_struct_layout_five_packed_u64s() {
        let o = [
            CACHESTAT_OFF_NR_CACHE,
            CACHESTAT_OFF_NR_DIRTY,
            CACHESTAT_OFF_NR_WRITEBACK,
            CACHESTAT_OFF_NR_EVICTED,
            CACHESTAT_OFF_NR_RECENTLY_EVICTED,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v, i * 8);
        }
        assert_eq!(CACHESTAT_SIZE, 5 * 8);
    }

    #[test]
    fn test_eviction_fields_at_end_of_struct() {
        // Eviction stats sit at the tail (so a future v2 struct can
        // append without ABI break).
        assert!(CACHESTAT_OFF_NR_EVICTED > CACHESTAT_OFF_NR_WRITEBACK);
        assert!(CACHESTAT_OFF_NR_RECENTLY_EVICTED > CACHESTAT_OFF_NR_EVICTED);
    }

    #[test]
    fn test_flags_currently_reserved() {
        // The kernel rejects any non-zero flags.
        assert_eq!(CACHESTAT_FLAGS_RESERVED, 0);
    }

    #[test]
    fn test_size_relations() {
        // Range is exactly two u64s.
        assert_eq!(CACHESTAT_RANGE_SIZE, 16);
        // Output struct is five u64s.
        assert_eq!(CACHESTAT_SIZE, 40);
        // The output is larger than the input range.
        assert!(CACHESTAT_SIZE > CACHESTAT_RANGE_SIZE);
    }
}
