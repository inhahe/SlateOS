//! `<linux/perf_event.h>` — Performance monitoring event constants (extended).
//!
//! Extended perf_event constants covering sample types, read
//! format flags, breakpoint types, and perf event IOCTLs.

// ---------------------------------------------------------------------------
// Sample types (PERF_SAMPLE_*)
// ---------------------------------------------------------------------------

/// Instruction pointer.
pub const PERF_SAMPLE_IP: u64 = 1 << 0;
/// Thread ID.
pub const PERF_SAMPLE_TID: u64 = 1 << 1;
/// Timestamp.
pub const PERF_SAMPLE_TIME: u64 = 1 << 2;
/// Address.
pub const PERF_SAMPLE_ADDR: u64 = 1 << 3;
/// Read counter values.
pub const PERF_SAMPLE_READ: u64 = 1 << 4;
/// Call chain (stack trace).
pub const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
/// Event ID.
pub const PERF_SAMPLE_ID: u64 = 1 << 6;
/// CPU number.
pub const PERF_SAMPLE_CPU: u64 = 1 << 7;
/// Period.
pub const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
/// Stream ID.
pub const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
/// Raw data.
pub const PERF_SAMPLE_RAW: u64 = 1 << 10;
/// Branch stack.
pub const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
/// User registers.
pub const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
/// User stack.
pub const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
/// Weight.
pub const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
/// Data source.
pub const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
/// Unique event identifier.
pub const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
/// Transaction.
pub const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
/// Interrupt registers.
pub const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
/// Physical address.
pub const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;
/// Cgroup ID.
pub const PERF_SAMPLE_CGROUP: u64 = 1 << 21;
/// Data page size.
pub const PERF_SAMPLE_DATA_PAGE_SIZE: u64 = 1 << 22;
/// Code page size.
pub const PERF_SAMPLE_CODE_PAGE_SIZE: u64 = 1 << 23;
/// Weight struct (v2).
pub const PERF_SAMPLE_WEIGHT_STRUCT: u64 = 1 << 24;

// ---------------------------------------------------------------------------
// Read format flags (PERF_FORMAT_*)
// ---------------------------------------------------------------------------

/// Total time enabled.
pub const PERF_FORMAT_TOTAL_TIME_ENABLED: u64 = 1 << 0;
/// Total time running.
pub const PERF_FORMAT_TOTAL_TIME_RUNNING: u64 = 1 << 1;
/// Event ID in read.
pub const PERF_FORMAT_ID: u64 = 1 << 2;
/// Group read (all events in group).
pub const PERF_FORMAT_GROUP: u64 = 1 << 3;
/// Lost events.
pub const PERF_FORMAT_LOST: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Breakpoint types (HW_BREAKPOINT_*)
// ---------------------------------------------------------------------------

/// Empty (no breakpoint).
pub const HW_BREAKPOINT_EMPTY: u32 = 0;
/// Read breakpoint.
pub const HW_BREAKPOINT_R: u32 = 1;
/// Write breakpoint.
pub const HW_BREAKPOINT_W: u32 = 2;
/// Read/Write breakpoint.
pub const HW_BREAKPOINT_RW: u32 = HW_BREAKPOINT_R | HW_BREAKPOINT_W;
/// Execute breakpoint.
pub const HW_BREAKPOINT_X: u32 = 4;

// ---------------------------------------------------------------------------
// Breakpoint lengths
// ---------------------------------------------------------------------------

/// 1-byte breakpoint.
pub const HW_BREAKPOINT_LEN_1: u32 = 1;
/// 2-byte breakpoint.
pub const HW_BREAKPOINT_LEN_2: u32 = 2;
/// 4-byte breakpoint.
pub const HW_BREAKPOINT_LEN_4: u32 = 4;
/// 8-byte breakpoint.
pub const HW_BREAKPOINT_LEN_8: u32 = 8;

// ---------------------------------------------------------------------------
// Perf event IOCTL commands
// ---------------------------------------------------------------------------

/// Enable event.
pub const PERF_EVENT_IOC_ENABLE: u32 = 0x2400;
/// Disable event.
pub const PERF_EVENT_IOC_DISABLE: u32 = 0x2401;
/// Refresh event.
pub const PERF_EVENT_IOC_REFRESH: u32 = 0x2402;
/// Reset event counters.
pub const PERF_EVENT_IOC_RESET: u32 = 0x2403;
/// Set event period.
pub const PERF_EVENT_IOC_PERIOD: u32 = 0x2404;
/// Set output fd.
pub const PERF_EVENT_IOC_SET_OUTPUT: u32 = 0x2405;
/// Set filter.
pub const PERF_EVENT_IOC_SET_FILTER: u32 = 0x2406;
/// Query BPF programs.
pub const PERF_EVENT_IOC_QUERY_BPF: u32 = 0x240A;
/// Modify attributes.
pub const PERF_EVENT_IOC_MODIFY_ATTRIBUTES: u32 = 0x240B;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_types_are_powers_of_two() {
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
        for s in &samples {
            assert!(s.is_power_of_two(), "sample {s:#018x} not power of two");
        }
    }

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
            for j in (i + 1)..samples.len() {
                assert_eq!(samples[i] & samples[j], 0);
            }
        }
    }

    #[test]
    fn test_format_flags_powers_of_two() {
        let fmts = [
            PERF_FORMAT_TOTAL_TIME_ENABLED, PERF_FORMAT_TOTAL_TIME_RUNNING,
            PERF_FORMAT_ID, PERF_FORMAT_GROUP, PERF_FORMAT_LOST,
        ];
        for f in &fmts {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_format_flags_no_overlap() {
        let fmts = [
            PERF_FORMAT_TOTAL_TIME_ENABLED, PERF_FORMAT_TOTAL_TIME_RUNNING,
            PERF_FORMAT_ID, PERF_FORMAT_GROUP, PERF_FORMAT_LOST,
        ];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_eq!(fmts[i] & fmts[j], 0);
            }
        }
    }

    #[test]
    fn test_breakpoint_rw() {
        assert_eq!(HW_BREAKPOINT_RW, HW_BREAKPOINT_R | HW_BREAKPOINT_W);
    }

    #[test]
    fn test_breakpoint_empty_is_zero() {
        assert_eq!(HW_BREAKPOINT_EMPTY, 0);
    }

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            PERF_EVENT_IOC_ENABLE, PERF_EVENT_IOC_DISABLE,
            PERF_EVENT_IOC_REFRESH, PERF_EVENT_IOC_RESET,
            PERF_EVENT_IOC_PERIOD, PERF_EVENT_IOC_SET_OUTPUT,
            PERF_EVENT_IOC_SET_FILTER, PERF_EVENT_IOC_QUERY_BPF,
            PERF_EVENT_IOC_MODIFY_ATTRIBUTES,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_breakpoint_lens_powers_of_two() {
        let lens = [
            HW_BREAKPOINT_LEN_1, HW_BREAKPOINT_LEN_2,
            HW_BREAKPOINT_LEN_4, HW_BREAKPOINT_LEN_8,
        ];
        for l in &lens {
            assert!(l.is_power_of_two());
        }
    }
}
