//! `<linux/perf_event.h>` — PMU (Performance Monitoring Unit) constants.
//!
//! Constants for hardware PMU types, event encoding,
//! and PMU capability flags.

// ---------------------------------------------------------------------------
// PMU types (perf_type_id)
// ---------------------------------------------------------------------------

/// Hardware event.
pub const PERF_TYPE_HARDWARE: u32 = 0;
/// Software event.
pub const PERF_TYPE_SOFTWARE: u32 = 1;
/// Tracepoint event.
pub const PERF_TYPE_TRACEPOINT: u32 = 2;
/// Hardware cache event.
pub const PERF_TYPE_HW_CACHE: u32 = 3;
/// Raw event.
pub const PERF_TYPE_RAW: u32 = 4;
/// Breakpoint event.
pub const PERF_TYPE_BREAKPOINT: u32 = 5;

// ---------------------------------------------------------------------------
// Hardware cache event encoding
// ---------------------------------------------------------------------------

/// L1 data cache.
pub const PERF_COUNT_HW_CACHE_L1D: u32 = 0;
/// L1 instruction cache.
pub const PERF_COUNT_HW_CACHE_L1I: u32 = 1;
/// Last-level cache.
pub const PERF_COUNT_HW_CACHE_LL: u32 = 2;
/// Data TLB.
pub const PERF_COUNT_HW_CACHE_DTLB: u32 = 3;
/// Instruction TLB.
pub const PERF_COUNT_HW_CACHE_ITLB: u32 = 4;
/// Branch prediction unit.
pub const PERF_COUNT_HW_CACHE_BPU: u32 = 5;
/// Node (NUMA).
pub const PERF_COUNT_HW_CACHE_NODE: u32 = 6;

/// Cache read operation.
pub const PERF_COUNT_HW_CACHE_OP_READ: u32 = 0;
/// Cache write operation.
pub const PERF_COUNT_HW_CACHE_OP_WRITE: u32 = 1;
/// Cache prefetch operation.
pub const PERF_COUNT_HW_CACHE_OP_PREFETCH: u32 = 2;

/// Cache access result.
pub const PERF_COUNT_HW_CACHE_RESULT_ACCESS: u32 = 0;
/// Cache miss result.
pub const PERF_COUNT_HW_CACHE_RESULT_MISS: u32 = 1;

// ---------------------------------------------------------------------------
// PMU capability flags
// ---------------------------------------------------------------------------

/// PMU supports user-space reading.
pub const PERF_PMU_CAP_NO_INTERRUPT: u32 = 1 << 0;
/// PMU supports exclusive groups.
pub const PERF_PMU_CAP_EXCLUSIVE: u32 = 1 << 1;
/// PMU supports ITRACE.
pub const PERF_PMU_CAP_ITRACE: u32 = 1 << 2;
/// PMU is heterogeneous.
pub const PERF_PMU_CAP_HETEROGENEOUS: u32 = 1 << 3;
/// PMU supports no-NMI.
pub const PERF_PMU_CAP_NO_NMI: u32 = 1 << 4;
/// PMU supports AUX output.
pub const PERF_PMU_CAP_AUX_OUTPUT: u32 = 1 << 5;
/// PMU supports extended hardware events.
pub const PERF_PMU_CAP_EXTENDED_HW_TYPE: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pmu_types_distinct() {
        let types = [
            PERF_TYPE_HARDWARE, PERF_TYPE_SOFTWARE,
            PERF_TYPE_TRACEPOINT, PERF_TYPE_HW_CACHE,
            PERF_TYPE_RAW, PERF_TYPE_BREAKPOINT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_cache_ids_distinct() {
        let ids = [
            PERF_COUNT_HW_CACHE_L1D, PERF_COUNT_HW_CACHE_L1I,
            PERF_COUNT_HW_CACHE_LL, PERF_COUNT_HW_CACHE_DTLB,
            PERF_COUNT_HW_CACHE_ITLB, PERF_COUNT_HW_CACHE_BPU,
            PERF_COUNT_HW_CACHE_NODE,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
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
    fn test_cache_results_distinct() {
        assert_ne!(
            PERF_COUNT_HW_CACHE_RESULT_ACCESS,
            PERF_COUNT_HW_CACHE_RESULT_MISS
        );
    }

    #[test]
    fn test_cap_flags_power_of_two() {
        let flags = [
            PERF_PMU_CAP_NO_INTERRUPT, PERF_PMU_CAP_EXCLUSIVE,
            PERF_PMU_CAP_ITRACE, PERF_PMU_CAP_HETEROGENEOUS,
            PERF_PMU_CAP_NO_NMI, PERF_PMU_CAP_AUX_OUTPUT,
            PERF_PMU_CAP_EXTENDED_HW_TYPE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }

    #[test]
    fn test_cap_flags_no_overlap() {
        let flags = [
            PERF_PMU_CAP_NO_INTERRUPT, PERF_PMU_CAP_EXCLUSIVE,
            PERF_PMU_CAP_ITRACE, PERF_PMU_CAP_HETEROGENEOUS,
            PERF_PMU_CAP_NO_NMI, PERF_PMU_CAP_AUX_OUTPUT,
            PERF_PMU_CAP_EXTENDED_HW_TYPE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
