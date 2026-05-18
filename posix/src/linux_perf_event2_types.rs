//! `<linux/perf_event.h>` (extended subset) — perf event sampling constants.
//!
//! This module extends the base perf event types with sampling
//! configuration, output format flags, and read format options.
//! These control what data is recorded in perf samples (IP, callchain,
//! timestamps, branch records) and how counter groups are read.

// ---------------------------------------------------------------------------
// Sample type bits (perf_event_attr.sample_type)
// ---------------------------------------------------------------------------

/// Record instruction pointer.
pub const PERF_SAMPLE_IP: u64 = 1 << 0;
/// Record thread ID (pid/tid).
pub const PERF_SAMPLE_TID: u64 = 1 << 1;
/// Record timestamp.
pub const PERF_SAMPLE_TIME: u64 = 1 << 2;
/// Record address (for memory events).
pub const PERF_SAMPLE_ADDR: u64 = 1 << 3;
/// Record counter values in group.
pub const PERF_SAMPLE_READ: u64 = 1 << 4;
/// Record callchain (stack trace).
pub const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
/// Record event ID.
pub const PERF_SAMPLE_ID: u64 = 1 << 6;
/// Record CPU number.
pub const PERF_SAMPLE_CPU: u64 = 1 << 7;
/// Record period.
pub const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
/// Record stream ID.
pub const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
/// Record raw event data.
pub const PERF_SAMPLE_RAW: u64 = 1 << 10;
/// Record branch stack (LBR).
pub const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
/// Record user register state.
pub const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
/// Record user stack data.
pub const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
/// Record weight (latency).
pub const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
/// Record data source (memory hierarchy).
pub const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
/// Record unique sample ID.
pub const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
/// Record transaction info.
pub const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
/// Record kernel register state.
pub const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
/// Record physical address.
pub const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;
/// Record cgroup ID.
pub const PERF_SAMPLE_CGROUP: u64 = 1 << 21;
/// Record data page size.
pub const PERF_SAMPLE_DATA_PAGE_SIZE: u64 = 1 << 22;
/// Record code page size.
pub const PERF_SAMPLE_CODE_PAGE_SIZE: u64 = 1 << 23;
/// Record weight struct (extended latency info).
pub const PERF_SAMPLE_WEIGHT_STRUCT: u64 = 1 << 24;

// ---------------------------------------------------------------------------
// Read format bits (perf_event_attr.read_format)
// ---------------------------------------------------------------------------

/// Include total time enabled.
pub const PERF_FORMAT_TOTAL_TIME_ENABLED: u64 = 1 << 0;
/// Include total time running.
pub const PERF_FORMAT_TOTAL_TIME_RUNNING: u64 = 1 << 1;
/// Include event ID.
pub const PERF_FORMAT_ID: u64 = 1 << 2;
/// Read group of counters together.
pub const PERF_FORMAT_GROUP: u64 = 1 << 3;
/// Include lost event count.
pub const PERF_FORMAT_LOST: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_types_no_overlap() {
        let samples = [
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
        for i in 0..samples.len() {
            assert!(samples[i].is_power_of_two());
            for j in (i + 1)..samples.len() {
                assert_eq!(samples[i] & samples[j], 0);
            }
        }
    }

    #[test]
    fn test_read_format_no_overlap() {
        let fmts = [
            PERF_FORMAT_TOTAL_TIME_ENABLED,
            PERF_FORMAT_TOTAL_TIME_RUNNING,
            PERF_FORMAT_ID, PERF_FORMAT_GROUP,
            PERF_FORMAT_LOST,
        ];
        for i in 0..fmts.len() {
            assert!(fmts[i].is_power_of_two());
            for j in (i + 1)..fmts.len() {
                assert_eq!(fmts[i] & fmts[j], 0);
            }
        }
    }

    #[test]
    fn test_sample_composable() {
        let combined = PERF_SAMPLE_IP | PERF_SAMPLE_TID | PERF_SAMPLE_TIME;
        assert_ne!(combined & PERF_SAMPLE_IP, 0);
        assert_ne!(combined & PERF_SAMPLE_TID, 0);
        assert_eq!(combined & PERF_SAMPLE_ADDR, 0);
    }
}
