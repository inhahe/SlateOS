//! `<linux/blk-cgroup.h>` — Block I/O cgroup controller constants.
//!
//! The blkio/io cgroup controller limits and tracks block I/O
//! per cgroup. It provides weight-based proportional I/O
//! (BFQ scheduler) and absolute bandwidth/IOPS limits.

// ---------------------------------------------------------------------------
// I/O controller file names (cgroup v2)
// ---------------------------------------------------------------------------

/// I/O weight.
pub const IO_WEIGHT: &str = "io.weight";
/// I/O bandwidth max.
pub const IO_MAX: &str = "io.max";
/// I/O latency target.
pub const IO_LATENCY: &str = "io.latency";
/// I/O statistics.
pub const IO_STAT: &str = "io.stat";
/// I/O pressure.
pub const IO_PRESSURE: &str = "io.pressure";
/// I/O cost model parameters.
pub const IO_COST_MODEL: &str = "io.cost.model";
/// I/O cost QoS.
pub const IO_COST_QOS: &str = "io.cost.qos";

// ---------------------------------------------------------------------------
// Weight range
// ---------------------------------------------------------------------------

/// Minimum I/O weight.
pub const IO_WEIGHT_MIN: u32 = 1;
/// Default I/O weight.
pub const IO_WEIGHT_DEFAULT: u32 = 100;
/// Maximum I/O weight.
pub const IO_WEIGHT_MAX: u32 = 10000;

// ---------------------------------------------------------------------------
// I/O limit types
// ---------------------------------------------------------------------------

/// Read bandwidth limit (bytes/sec).
pub const IO_LIMIT_RBPS: u32 = 0;
/// Write bandwidth limit (bytes/sec).
pub const IO_LIMIT_WBPS: u32 = 1;
/// Read IOPS limit.
pub const IO_LIMIT_RIOPS: u32 = 2;
/// Write IOPS limit.
pub const IO_LIMIT_WIOPS: u32 = 3;

// ---------------------------------------------------------------------------
// I/O stat types
// ---------------------------------------------------------------------------

/// Read bytes.
pub const IO_STAT_RBYTES: &str = "rbytes";
/// Write bytes.
pub const IO_STAT_WBYTES: &str = "wbytes";
/// Read I/O operations.
pub const IO_STAT_RIOS: &str = "rios";
/// Write I/O operations.
pub const IO_STAT_WIOS: &str = "wios";
/// Discard bytes.
pub const IO_STAT_DBYTES: &str = "dbytes";
/// Discard I/O operations.
pub const IO_STAT_DIOS: &str = "dios";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_names_distinct() {
        let files = [
            IO_WEIGHT,
            IO_MAX,
            IO_LATENCY,
            IO_STAT,
            IO_PRESSURE,
            IO_COST_MODEL,
            IO_COST_QOS,
        ];
        for i in 0..files.len() {
            for j in (i + 1)..files.len() {
                assert_ne!(files[i], files[j]);
            }
        }
    }

    #[test]
    fn test_weight_range() {
        assert!(IO_WEIGHT_MIN < IO_WEIGHT_DEFAULT);
        assert!(IO_WEIGHT_DEFAULT < IO_WEIGHT_MAX);
    }

    #[test]
    fn test_limit_types_distinct() {
        let types = [IO_LIMIT_RBPS, IO_LIMIT_WBPS, IO_LIMIT_RIOPS, IO_LIMIT_WIOPS];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_stat_names_distinct() {
        let names = [
            IO_STAT_RBYTES,
            IO_STAT_WBYTES,
            IO_STAT_RIOS,
            IO_STAT_WIOS,
            IO_STAT_DBYTES,
            IO_STAT_DIOS,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }
}
