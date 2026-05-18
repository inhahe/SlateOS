//! `<linux/perf_event.h>` — Performance event sample type flag constants.
//!
//! When a perf event fires, the kernel can record various pieces of
//! information about the execution context. These flags specify
//! which fields to include in each sample record.

// ---------------------------------------------------------------------------
// Sample type flags (perf_event_attr.sample_type)
// ---------------------------------------------------------------------------

/// Include instruction pointer.
pub const PERF_SAMPLE_IP: u64 = 1 << 0;
/// Include thread/process IDs.
pub const PERF_SAMPLE_TID: u64 = 1 << 1;
/// Include timestamp.
pub const PERF_SAMPLE_TIME: u64 = 1 << 2;
/// Include address (for memory events).
pub const PERF_SAMPLE_ADDR: u64 = 1 << 3;
/// Include event counter value.
pub const PERF_SAMPLE_READ: u64 = 1 << 4;
/// Include callchain (stack trace).
pub const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
/// Include event ID.
pub const PERF_SAMPLE_ID: u64 = 1 << 6;
/// Include CPU number.
pub const PERF_SAMPLE_CPU: u64 = 1 << 7;
/// Include sampling period.
pub const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
/// Include stream ID.
pub const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
/// Include raw PMU data.
pub const PERF_SAMPLE_RAW: u64 = 1 << 10;
/// Include branch stack.
pub const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
/// Include user register values.
pub const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
/// Include user stack data.
pub const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
/// Include weight (latency info).
pub const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
/// Include data source (memory hierarchy).
pub const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
/// Include unique event identifier.
pub const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
/// Include transaction abort info.
pub const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
/// Include intr register values.
pub const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
/// Include physical address.
pub const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;
/// Include cgroup ID.
pub const PERF_SAMPLE_CGROUP: u64 = 1 << 21;
/// Include data page size.
pub const PERF_SAMPLE_DATA_PAGE_SIZE: u64 = 1 << 22;
/// Include code page size.
pub const PERF_SAMPLE_CODE_PAGE_SIZE: u64 = 1 << 23;
/// Include weight (structured format).
pub const PERF_SAMPLE_WEIGHT_STRUCT: u64 = 1 << 24;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_types_power_of_two() {
        let types = [
            PERF_SAMPLE_IP, PERF_SAMPLE_TID, PERF_SAMPLE_TIME,
            PERF_SAMPLE_ADDR, PERF_SAMPLE_READ, PERF_SAMPLE_CALLCHAIN,
            PERF_SAMPLE_ID, PERF_SAMPLE_CPU, PERF_SAMPLE_PERIOD,
            PERF_SAMPLE_STREAM_ID, PERF_SAMPLE_RAW,
            PERF_SAMPLE_BRANCH_STACK, PERF_SAMPLE_REGS_USER,
            PERF_SAMPLE_STACK_USER, PERF_SAMPLE_WEIGHT,
            PERF_SAMPLE_DATA_SRC, PERF_SAMPLE_IDENTIFIER,
            PERF_SAMPLE_TRANSACTION, PERF_SAMPLE_REGS_INTR,
            PERF_SAMPLE_PHYS_ADDR, PERF_SAMPLE_CGROUP,
            PERF_SAMPLE_DATA_PAGE_SIZE, PERF_SAMPLE_CODE_PAGE_SIZE,
            PERF_SAMPLE_WEIGHT_STRUCT,
        ];
        for t in &types {
            assert!(t.is_power_of_two());
        }
    }

    #[test]
    fn test_sample_types_no_overlap() {
        let types = [
            PERF_SAMPLE_IP, PERF_SAMPLE_TID, PERF_SAMPLE_TIME,
            PERF_SAMPLE_ADDR, PERF_SAMPLE_READ, PERF_SAMPLE_CALLCHAIN,
            PERF_SAMPLE_ID, PERF_SAMPLE_CPU, PERF_SAMPLE_PERIOD,
            PERF_SAMPLE_STREAM_ID, PERF_SAMPLE_RAW,
            PERF_SAMPLE_BRANCH_STACK, PERF_SAMPLE_REGS_USER,
            PERF_SAMPLE_STACK_USER, PERF_SAMPLE_WEIGHT,
            PERF_SAMPLE_DATA_SRC, PERF_SAMPLE_IDENTIFIER,
            PERF_SAMPLE_TRANSACTION, PERF_SAMPLE_REGS_INTR,
            PERF_SAMPLE_PHYS_ADDR, PERF_SAMPLE_CGROUP,
            PERF_SAMPLE_DATA_PAGE_SIZE, PERF_SAMPLE_CODE_PAGE_SIZE,
            PERF_SAMPLE_WEIGHT_STRUCT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_eq!(types[i] & types[j], 0);
            }
        }
    }

    #[test]
    fn test_ip_is_bit0() {
        assert_eq!(PERF_SAMPLE_IP, 1);
    }
}
