//! `<linux/perf_event.h>` — Perf event output format constants.
//!
//! These constants control the layout of data records in the
//! perf event ring buffer, including which fields are present
//! in sample records and header flags.

// ---------------------------------------------------------------------------
// Sample format flags (perf_event_attr.sample_type)
// ---------------------------------------------------------------------------

/// Include instruction pointer.
pub const PERF_SAMPLE_IP: u64 = 1 << 0;
/// Include thread ID.
pub const PERF_SAMPLE_TID: u64 = 1 << 1;
/// Include timestamp.
pub const PERF_SAMPLE_TIME: u64 = 1 << 2;
/// Include virtual address.
pub const PERF_SAMPLE_ADDR: u64 = 1 << 3;
/// Include counter group values.
pub const PERF_SAMPLE_READ: u64 = 1 << 4;
/// Include call chain.
pub const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
/// Include event ID.
pub const PERF_SAMPLE_ID: u64 = 1 << 6;
/// Include CPU number.
pub const PERF_SAMPLE_CPU: u64 = 1 << 7;
/// Include period value.
pub const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
/// Include stream ID.
pub const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
/// Include raw record data.
pub const PERF_SAMPLE_RAW: u64 = 1 << 10;
/// Include branch stack.
pub const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
/// Include user register state.
pub const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
/// Include user stack data.
pub const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
/// Include weight (cost).
pub const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
/// Include data source (memory hierarchy).
pub const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
/// Include unique event identifier.
pub const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
/// Include transaction flags.
pub const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
/// Include interrupt register state.
pub const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
/// Include physical address.
pub const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;
/// Include cgroup ID.
pub const PERF_SAMPLE_CGROUP: u64 = 1 << 21;
/// Include data page size.
pub const PERF_SAMPLE_DATA_PAGE_SIZE: u64 = 1 << 22;
/// Include code page size.
pub const PERF_SAMPLE_CODE_PAGE_SIZE: u64 = 1 << 23;
/// Include weight struct (extended).
pub const PERF_SAMPLE_WEIGHT_STRUCT: u64 = 1 << 24;

// ---------------------------------------------------------------------------
// Read format flags (perf_event_attr.read_format)
// ---------------------------------------------------------------------------

/// Include total time enabled.
pub const PERF_FORMAT_TOTAL_TIME_ENABLED: u64 = 1 << 0;
/// Include total time running.
pub const PERF_FORMAT_TOTAL_TIME_RUNNING: u64 = 1 << 1;
/// Include event ID in group read.
pub const PERF_FORMAT_ID: u64 = 1 << 2;
/// Include group members.
pub const PERF_FORMAT_GROUP: u64 = 1 << 3;
/// Include lost events count.
pub const PERF_FORMAT_LOST: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_flags_no_overlap() {
        let flags = [
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
            PERF_SAMPLE_CGROUP,
            PERF_SAMPLE_DATA_PAGE_SIZE,
            PERF_SAMPLE_CODE_PAGE_SIZE,
            PERF_SAMPLE_WEIGHT_STRUCT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sample_flags_power_of_two() {
        let flags = [
            PERF_SAMPLE_IP,
            PERF_SAMPLE_TID,
            PERF_SAMPLE_TIME,
            PERF_SAMPLE_ADDR,
            PERF_SAMPLE_READ,
            PERF_SAMPLE_CALLCHAIN,
            PERF_SAMPLE_ID,
            PERF_SAMPLE_CPU,
            PERF_SAMPLE_PERIOD,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_read_format_no_overlap() {
        let flags = [
            PERF_FORMAT_TOTAL_TIME_ENABLED,
            PERF_FORMAT_TOTAL_TIME_RUNNING,
            PERF_FORMAT_ID,
            PERF_FORMAT_GROUP,
            PERF_FORMAT_LOST,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sample_ip() {
        assert_eq!(PERF_SAMPLE_IP, 1);
    }

    #[test]
    fn test_format_group() {
        assert_eq!(PERF_FORMAT_GROUP, 8);
    }
}
