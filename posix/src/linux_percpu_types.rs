//! `<linux/percpu.h>` — Per-CPU data constants.
//!
//! Per-CPU data provides each CPU with its own private copy of a
//! variable, eliminating the need for locking in the common case.
//! Each CPU reads/writes its own copy without contention. This is
//! used for high-frequency counters (statistics), allocator free
//! lists, scheduler run queues, and any data accessed on every
//! syscall/interrupt. Access requires disabling preemption (to
//! prevent migration to another CPU mid-access).

// ---------------------------------------------------------------------------
// Per-CPU allocation types
// ---------------------------------------------------------------------------

/// Static per-CPU variable (defined at compile time).
pub const PERCPU_ALLOC_STATIC: u32 = 0;
/// Dynamic per-CPU allocation (alloc_percpu at runtime).
pub const PERCPU_ALLOC_DYNAMIC: u32 = 1;

// ---------------------------------------------------------------------------
// Per-CPU area layout (x86_64)
// ---------------------------------------------------------------------------

/// Per-CPU area base offset (from GS segment base).
pub const PERCPU_BASE_OFFSET: u32 = 0;
/// Minimum per-CPU area size per CPU (64 KiB).
pub const PERCPU_MIN_SIZE: u32 = 65536;
/// Default chunk size for dynamic per-CPU allocation (128 KiB).
pub const PERCPU_CHUNK_SIZE_DEFAULT: u32 = 131072;
/// Maximum per-CPU allocation size (32 KiB per single variable).
pub const PERCPU_MAX_ALLOC_SIZE: u32 = 32768;

// ---------------------------------------------------------------------------
// Per-CPU memory sections
// ---------------------------------------------------------------------------

/// First section: static per-CPU data.
pub const PERCPU_SECTION_FIRST: u32 = 0;
/// Reserved section: early boot per-CPU data.
pub const PERCPU_SECTION_RESERVED: u32 = 1;
/// Dynamic section: runtime allocations.
pub const PERCPU_SECTION_DYNAMIC: u32 = 2;
/// Number of per-CPU sections.
pub const PERCPU_NR_SECTIONS: u32 = 3;

// ---------------------------------------------------------------------------
// Per-CPU counter types
// ---------------------------------------------------------------------------

/// Exact counter (expensive, cross-CPU summation for read).
pub const PERCPU_COUNTER_EXACT: u32 = 0;
/// Approximate counter (cheap reads, batch updates).
pub const PERCPU_COUNTER_BATCH: u32 = 1;

// ---------------------------------------------------------------------------
// Per-CPU counter batch size
// ---------------------------------------------------------------------------

/// Default batch size (accumulate locally before updating global).
pub const PERCPU_COUNTER_BATCH_DEFAULT: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_types_distinct() {
        assert_ne!(PERCPU_ALLOC_STATIC, PERCPU_ALLOC_DYNAMIC);
    }

    #[test]
    fn test_area_sizes() {
        assert!(PERCPU_MIN_SIZE > 0);
        assert!(PERCPU_CHUNK_SIZE_DEFAULT >= PERCPU_MIN_SIZE);
        assert!(PERCPU_MAX_ALLOC_SIZE <= PERCPU_MIN_SIZE);
    }

    #[test]
    fn test_sections_distinct() {
        let secs = [
            PERCPU_SECTION_FIRST,
            PERCPU_SECTION_RESERVED,
            PERCPU_SECTION_DYNAMIC,
        ];
        assert_eq!(secs.len(), PERCPU_NR_SECTIONS as usize);
        for i in 0..secs.len() {
            for j in (i + 1)..secs.len() {
                assert_ne!(secs[i], secs[j]);
            }
        }
    }

    #[test]
    fn test_counter_types_distinct() {
        assert_ne!(PERCPU_COUNTER_EXACT, PERCPU_COUNTER_BATCH);
    }

    #[test]
    fn test_batch_default() {
        assert!(PERCPU_COUNTER_BATCH_DEFAULT > 0);
    }
}
