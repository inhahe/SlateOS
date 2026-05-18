//! `<linux/perf_event.h>` — Additional perf event constants (part 3).
//!
//! Supplementary perf constants covering sample types,
//! read formats, branch sample types, and aux flags.

// ---------------------------------------------------------------------------
// Perf sample types (PERF_SAMPLE_*)
// ---------------------------------------------------------------------------

/// IP.
pub const PERF_SAMPLE_IP: u64 = 1 << 0;
/// TID.
pub const PERF_SAMPLE_TID: u64 = 1 << 1;
/// Time.
pub const PERF_SAMPLE_TIME: u64 = 1 << 2;
/// Address.
pub const PERF_SAMPLE_ADDR: u64 = 1 << 3;
/// Read counters.
pub const PERF_SAMPLE_READ: u64 = 1 << 4;
/// Callchain.
pub const PERF_SAMPLE_CALLCHAIN: u64 = 1 << 5;
/// ID.
pub const PERF_SAMPLE_ID: u64 = 1 << 6;
/// CPU.
pub const PERF_SAMPLE_CPU: u64 = 1 << 7;
/// Period.
pub const PERF_SAMPLE_PERIOD: u64 = 1 << 8;
/// Stream ID.
pub const PERF_SAMPLE_STREAM_ID: u64 = 1 << 9;
/// Raw data.
pub const PERF_SAMPLE_RAW: u64 = 1 << 10;
/// Branch stack.
pub const PERF_SAMPLE_BRANCH_STACK: u64 = 1 << 11;
/// User regs.
pub const PERF_SAMPLE_REGS_USER: u64 = 1 << 12;
/// User stack.
pub const PERF_SAMPLE_STACK_USER: u64 = 1 << 13;
/// Weight.
pub const PERF_SAMPLE_WEIGHT: u64 = 1 << 14;
/// Data source.
pub const PERF_SAMPLE_DATA_SRC: u64 = 1 << 15;
/// Identifier.
pub const PERF_SAMPLE_IDENTIFIER: u64 = 1 << 16;
/// Transaction.
pub const PERF_SAMPLE_TRANSACTION: u64 = 1 << 17;
/// Intr regs.
pub const PERF_SAMPLE_REGS_INTR: u64 = 1 << 18;
/// Physical address.
pub const PERF_SAMPLE_PHYS_ADDR: u64 = 1 << 19;
/// Aux output.
pub const PERF_SAMPLE_AUX: u64 = 1 << 20;
/// Cgroup.
pub const PERF_SAMPLE_CGROUP: u64 = 1 << 21;
/// Data page size.
pub const PERF_SAMPLE_DATA_PAGE_SIZE: u64 = 1 << 22;
/// Code page size.
pub const PERF_SAMPLE_CODE_PAGE_SIZE: u64 = 1 << 23;
/// Weight struct.
pub const PERF_SAMPLE_WEIGHT_STRUCT: u64 = 1 << 24;

// ---------------------------------------------------------------------------
// Perf read format flags (PERF_FORMAT_*)
// ---------------------------------------------------------------------------

/// Total time enabled.
pub const PERF_FORMAT_TOTAL_TIME_ENABLED: u64 = 1 << 0;
/// Total time running.
pub const PERF_FORMAT_TOTAL_TIME_RUNNING: u64 = 1 << 1;
/// ID.
pub const PERF_FORMAT_ID: u64 = 1 << 2;
/// Group.
pub const PERF_FORMAT_GROUP: u64 = 1 << 3;
/// Lost.
pub const PERF_FORMAT_LOST: u64 = 1 << 4;

// ---------------------------------------------------------------------------
// Perf aux flags
// ---------------------------------------------------------------------------

/// Aux output.
pub const PERF_AUX_FLAG_TRUNCATED: u64 = 0x01;
/// Overwrite.
pub const PERF_AUX_FLAG_OVERWRITE: u64 = 0x02;
/// Partial.
pub const PERF_AUX_FLAG_PARTIAL: u64 = 0x04;
/// Collision.
pub const PERF_AUX_FLAG_COLLISION: u64 = 0x08;
/// PMU format list.
pub const PERF_AUX_FLAG_PMU_FORMAT_TYPE_MASK: u64 = 0xFF00;

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
            PERF_SAMPLE_PHYS_ADDR, PERF_SAMPLE_AUX,
            PERF_SAMPLE_CGROUP, PERF_SAMPLE_DATA_PAGE_SIZE,
            PERF_SAMPLE_CODE_PAGE_SIZE, PERF_SAMPLE_WEIGHT_STRUCT,
        ];
        for t in &types {
            assert!(t.is_power_of_two(), "0x{:016x} not power of two", t);
        }
    }

    #[test]
    fn test_format_flags_power_of_two() {
        let flags = [
            PERF_FORMAT_TOTAL_TIME_ENABLED,
            PERF_FORMAT_TOTAL_TIME_RUNNING,
            PERF_FORMAT_ID, PERF_FORMAT_GROUP,
            PERF_FORMAT_LOST,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:016x} not power of two", f);
        }
    }

    #[test]
    fn test_aux_flags_power_of_two() {
        let flags = [
            PERF_AUX_FLAG_TRUNCATED, PERF_AUX_FLAG_OVERWRITE,
            PERF_AUX_FLAG_PARTIAL, PERF_AUX_FLAG_COLLISION,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:016x} not power of two", f);
        }
    }

    #[test]
    fn test_aux_flags_no_overlap() {
        let flags = [
            PERF_AUX_FLAG_TRUNCATED, PERF_AUX_FLAG_OVERWRITE,
            PERF_AUX_FLAG_PARTIAL, PERF_AUX_FLAG_COLLISION,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
