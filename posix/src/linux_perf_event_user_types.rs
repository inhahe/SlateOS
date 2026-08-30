//! `<linux/perf_event.h>` — `perf_event_open(2)` userspace ABI.
//!
//! `perf record`, BPF profilers, Intel PT viewers, and language-runtime
//! profilers all start with `perf_event_open`. The constants below
//! pick the event class, what samples to record, and the read format
//! of the per-event counters.

// ---------------------------------------------------------------------------
// `perf_type_id`
// ---------------------------------------------------------------------------

pub const PERF_TYPE_HARDWARE: u32 = 0;
pub const PERF_TYPE_SOFTWARE: u32 = 1;
pub const PERF_TYPE_TRACEPOINT: u32 = 2;
pub const PERF_TYPE_HW_CACHE: u32 = 3;
pub const PERF_TYPE_RAW: u32 = 4;
pub const PERF_TYPE_BREAKPOINT: u32 = 5;

// ---------------------------------------------------------------------------
// `perf_hw_id` — `PERF_TYPE_HARDWARE` event selectors
// ---------------------------------------------------------------------------

pub const PERF_COUNT_HW_CPU_CYCLES: u32 = 0;
pub const PERF_COUNT_HW_INSTRUCTIONS: u32 = 1;
pub const PERF_COUNT_HW_CACHE_REFERENCES: u32 = 2;
pub const PERF_COUNT_HW_CACHE_MISSES: u32 = 3;
pub const PERF_COUNT_HW_BRANCH_INSTRUCTIONS: u32 = 4;
pub const PERF_COUNT_HW_BRANCH_MISSES: u32 = 5;
pub const PERF_COUNT_HW_BUS_CYCLES: u32 = 6;
pub const PERF_COUNT_HW_STALLED_CYCLES_FRONTEND: u32 = 7;
pub const PERF_COUNT_HW_STALLED_CYCLES_BACKEND: u32 = 8;
pub const PERF_COUNT_HW_REF_CPU_CYCLES: u32 = 9;

// ---------------------------------------------------------------------------
// `perf_sw_ids` — `PERF_TYPE_SOFTWARE` event selectors
// ---------------------------------------------------------------------------

pub const PERF_COUNT_SW_CPU_CLOCK: u32 = 0;
pub const PERF_COUNT_SW_TASK_CLOCK: u32 = 1;
pub const PERF_COUNT_SW_PAGE_FAULTS: u32 = 2;
pub const PERF_COUNT_SW_CONTEXT_SWITCHES: u32 = 3;
pub const PERF_COUNT_SW_CPU_MIGRATIONS: u32 = 4;
pub const PERF_COUNT_SW_PAGE_FAULTS_MIN: u32 = 5;
pub const PERF_COUNT_SW_PAGE_FAULTS_MAJ: u32 = 6;
pub const PERF_COUNT_SW_ALIGNMENT_FAULTS: u32 = 7;
pub const PERF_COUNT_SW_EMULATION_FAULTS: u32 = 8;
pub const PERF_COUNT_SW_DUMMY: u32 = 9;
pub const PERF_COUNT_SW_BPF_OUTPUT: u32 = 10;
pub const PERF_COUNT_SW_CGROUP_SWITCHES: u32 = 11;

// ---------------------------------------------------------------------------
// `perf_event_sample_format` — `attr.sample_type` bits
// ---------------------------------------------------------------------------

pub const PERF_SAMPLE_IP: u64 = 1 << 0;
pub const PERF_SAMPLE_TID: u64 = 1 << 1;
pub const PERF_SAMPLE_TIME: u64 = 1 << 2;
pub const PERF_SAMPLE_ADDR: u64 = 1 << 3;
pub const PERF_SAMPLE_READ: u64 = 1 << 4;
pub const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
pub const PERF_SAMPLE_ID: u64 = 1 << 6;
pub const PERF_SAMPLE_CPU: u64 = 1 << 7;
pub const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
pub const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
pub const PERF_SAMPLE_RAW: u64 = 1 << 10;
pub const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
pub const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
pub const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
pub const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
pub const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
pub const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
pub const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
pub const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
pub const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;

// ---------------------------------------------------------------------------
// `perf_event_open` flags
// ---------------------------------------------------------------------------

pub const PERF_FLAG_FD_NO_GROUP: u64 = 1 << 0;
pub const PERF_FLAG_FD_OUTPUT: u64 = 1 << 1;
pub const PERF_FLAG_PID_CGROUP: u64 = 1 << 2;
pub const PERF_FLAG_FD_CLOEXEC: u64 = 1 << 3;

// ---------------------------------------------------------------------------
// Syscall
// ---------------------------------------------------------------------------

pub const NR_PERF_EVENT_OPEN: u32 = 298;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_perf_type_dense_0_to_5() {
        let t = [
            PERF_TYPE_HARDWARE,
            PERF_TYPE_SOFTWARE,
            PERF_TYPE_TRACEPOINT,
            PERF_TYPE_HW_CACHE,
            PERF_TYPE_RAW,
            PERF_TYPE_BREAKPOINT,
        ];
        for (i, &v) in t.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_hw_event_ids_dense_0_to_9() {
        let h = [
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
        for (i, &v) in h.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_sw_event_ids_dense_0_to_11() {
        let s = [
            PERF_COUNT_SW_CPU_CLOCK,
            PERF_COUNT_SW_TASK_CLOCK,
            PERF_COUNT_SW_PAGE_FAULTS,
            PERF_COUNT_SW_CONTEXT_SWITCHES,
            PERF_COUNT_SW_CPU_MIGRATIONS,
            PERF_COUNT_SW_PAGE_FAULTS_MIN,
            PERF_COUNT_SW_PAGE_FAULTS_MAJ,
            PERF_COUNT_SW_ALIGNMENT_FAULTS,
            PERF_COUNT_SW_EMULATION_FAULTS,
            PERF_COUNT_SW_DUMMY,
            PERF_COUNT_SW_BPF_OUTPUT,
            PERF_COUNT_SW_CGROUP_SWITCHES,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_sample_bits_dense_0_to_19() {
        let s = [
            PERF_SAMPLE_IP,
            PERF_SAMPLE_TID,
            PERF_SAMPLE_TIME,
            PERF_SAMPLE_ADDR,
            PERF_SAMPLE_READ,
            PERF_SAMPLE_CALLCHAIN,
            PERF_SAMPLE_ID,
            PERF_SAMPLE_CPU,
            PERF_SAMPLE_PERIOD,
            PERF_SAMPLE_STREAM_ID,
            PERF_SAMPLE_RAW,
            PERF_SAMPLE_BRANCH_STACK,
            PERF_SAMPLE_REGS_USER,
            PERF_SAMPLE_STACK_USER,
            PERF_SAMPLE_WEIGHT,
            PERF_SAMPLE_DATA_SRC,
            PERF_SAMPLE_IDENTIFIER,
            PERF_SAMPLE_TRANSACTION,
            PERF_SAMPLE_REGS_INTR,
            PERF_SAMPLE_PHYS_ADDR,
        ];
        let mut or = 0u64;
        for (i, &v) in s.iter().enumerate() {
            assert!(v.is_power_of_two());
            assert_eq!(v, 1u64 << i);
            or |= v;
        }
        // Twenty consecutive low bits = 0xF_FFFF.
        assert_eq!(or, 0xF_FFFF);
    }

    #[test]
    fn test_flags_dense_0_to_3() {
        let f = [
            PERF_FLAG_FD_NO_GROUP,
            PERF_FLAG_FD_OUTPUT,
            PERF_FLAG_PID_CGROUP,
            PERF_FLAG_FD_CLOEXEC,
        ];
        let mut or = 0u64;
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v, 1u64 << i);
            or |= v;
        }
        assert_eq!(or, 0xF);
    }

    #[test]
    fn test_syscall_number() {
        assert_eq!(NR_PERF_EVENT_OPEN, 298);
    }
}
