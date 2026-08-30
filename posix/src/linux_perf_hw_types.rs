//! `<linux/perf_event.h>` — Hardware performance counter event constants.
//!
//! These constants identify hardware performance monitoring events
//! that map to CPU performance counters (PMCs). The kernel's perf
//! subsystem translates these generic IDs into architecture-specific
//! counter configurations.

// ---------------------------------------------------------------------------
// Hardware event IDs (PERF_COUNT_HW_*)
// ---------------------------------------------------------------------------

/// Total CPU cycles.
pub const PERF_COUNT_HW_CPU_CYCLES: u64 = 0;
/// Retired instructions.
pub const PERF_COUNT_HW_INSTRUCTIONS: u64 = 1;
/// Cache references (accesses).
pub const PERF_COUNT_HW_CACHE_REFERENCES: u64 = 2;
/// Cache misses.
pub const PERF_COUNT_HW_CACHE_MISSES: u64 = 3;
/// Retired branch instructions.
pub const PERF_COUNT_HW_BRANCH_INSTRUCTIONS: u64 = 4;
/// Mispredicted branches.
pub const PERF_COUNT_HW_BRANCH_MISSES: u64 = 5;
/// Bus cycles.
pub const PERF_COUNT_HW_BUS_CYCLES: u64 = 6;
/// Stalled cycles (frontend).
pub const PERF_COUNT_HW_STALLED_CYCLES_FRONTEND: u64 = 7;
/// Stalled cycles (backend).
pub const PERF_COUNT_HW_STALLED_CYCLES_BACKEND: u64 = 8;
/// Reference CPU cycles (unscaled).
pub const PERF_COUNT_HW_REF_CPU_CYCLES: u64 = 9;

// ---------------------------------------------------------------------------
// Hardware cache event IDs (for PERF_TYPE_HW_CACHE)
// ---------------------------------------------------------------------------

/// L1 data cache.
pub const PERF_COUNT_HW_CACHE_L1D: u64 = 0;
/// L1 instruction cache.
pub const PERF_COUNT_HW_CACHE_L1I: u64 = 1;
/// Last-level cache.
pub const PERF_COUNT_HW_CACHE_LL: u64 = 2;
/// Data TLB.
pub const PERF_COUNT_HW_CACHE_DTLB: u64 = 3;
/// Instruction TLB.
pub const PERF_COUNT_HW_CACHE_ITLB: u64 = 4;
/// Branch prediction unit.
pub const PERF_COUNT_HW_CACHE_BPU: u64 = 5;
/// NUMA node cache.
pub const PERF_COUNT_HW_CACHE_NODE: u64 = 6;

// ---------------------------------------------------------------------------
// Hardware cache operation types
// ---------------------------------------------------------------------------

/// Read operation.
pub const PERF_COUNT_HW_CACHE_OP_READ: u64 = 0;
/// Write operation.
pub const PERF_COUNT_HW_CACHE_OP_WRITE: u64 = 1;
/// Prefetch operation.
pub const PERF_COUNT_HW_CACHE_OP_PREFETCH: u64 = 2;

// ---------------------------------------------------------------------------
// Hardware cache result types
// ---------------------------------------------------------------------------

/// Cache access (hit or miss).
pub const PERF_COUNT_HW_CACHE_RESULT_ACCESS: u64 = 0;
/// Cache miss.
pub const PERF_COUNT_HW_CACHE_RESULT_MISS: u64 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hw_events_distinct() {
        let events = [
            PERF_COUNT_HW_CPU_CYCLES,
            PERF_COUNT_HW_INSTRUCTIONS,
            PERF_COUNT_HW_CACHE_REFERENCES,
            PERF_COUNT_HW_CACHE_MISSES,
            PERF_COUNT_HW_BRANCH_INSTRUCTIONS,
            PERF_COUNT_HW_BRANCH_MISSES,
            PERF_COUNT_HW_BUS_CYCLES,
            PERF_COUNT_HW_STALLED_CYCLES_FRONTEND,
            PERF_COUNT_HW_STALLED_CYCLES_BACKEND,
            PERF_COUNT_HW_REF_CPU_CYCLES,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_cycles_is_zero() {
        assert_eq!(PERF_COUNT_HW_CPU_CYCLES, 0);
    }

    #[test]
    fn test_cache_ids_distinct() {
        let caches = [
            PERF_COUNT_HW_CACHE_L1D,
            PERF_COUNT_HW_CACHE_L1I,
            PERF_COUNT_HW_CACHE_LL,
            PERF_COUNT_HW_CACHE_DTLB,
            PERF_COUNT_HW_CACHE_ITLB,
            PERF_COUNT_HW_CACHE_BPU,
            PERF_COUNT_HW_CACHE_NODE,
        ];
        for i in 0..caches.len() {
            for j in (i + 1)..caches.len() {
                assert_ne!(caches[i], caches[j]);
            }
        }
    }

    #[test]
    fn test_cache_ops_distinct() {
        let ops = [
            PERF_COUNT_HW_CACHE_OP_READ,
            PERF_COUNT_HW_CACHE_OP_WRITE,
            PERF_COUNT_HW_CACHE_OP_PREFETCH,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_cache_results() {
        assert_ne!(
            PERF_COUNT_HW_CACHE_RESULT_ACCESS,
            PERF_COUNT_HW_CACHE_RESULT_MISS
        );
    }
}
