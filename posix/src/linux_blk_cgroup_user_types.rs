//! `<linux/blk-cgroup.h>` — block-IO cgroup v2 control surface.
//!
//! `blkcg` (a.k.a. `io` cgroup v2 controller) exposes per-cgroup
//! weights and bandwidth limits via cgroupfs. Userspace tools
//! (`systemd-cgls`, `iotop -P`, `oomd`) read and write these files.

// ---------------------------------------------------------------------------
// cgroup v2 control files (under `<mount>/io.*`)
// ---------------------------------------------------------------------------

pub const IO_WEIGHT: &str = "io.weight";
pub const IO_MAX: &str = "io.max";
pub const IO_STAT: &str = "io.stat";
pub const IO_PRESSURE: &str = "io.pressure";
pub const IO_LATENCY: &str = "io.latency";
pub const IO_COST_QOS: &str = "io.cost.qos";
pub const IO_COST_MODEL: &str = "io.cost.model";
pub const IO_BFQ_WEIGHT: &str = "io.bfq.weight";

// ---------------------------------------------------------------------------
// io.weight bounds (CFQ/BFQ proportional sharing)
// ---------------------------------------------------------------------------

pub const CGROUP_WEIGHT_MIN: u32 = 1;
pub const CGROUP_WEIGHT_MAX: u32 = 10_000;
pub const CGROUP_WEIGHT_DFL: u32 = 100;

// ---------------------------------------------------------------------------
// io.max throttling keys
// ---------------------------------------------------------------------------

pub const IO_MAX_KEY_RBPS: &str = "rbps";
pub const IO_MAX_KEY_WBPS: &str = "wbps";
pub const IO_MAX_KEY_RIOPS: &str = "riops";
pub const IO_MAX_KEY_WIOPS: &str = "wiops";

/// "max" token — written to clear a limit.
pub const IO_MAX_VALUE_MAX: &str = "max";

// ---------------------------------------------------------------------------
// io.stat output column keys
// ---------------------------------------------------------------------------

pub const IO_STAT_KEY_RBYTES: &str = "rbytes";
pub const IO_STAT_KEY_WBYTES: &str = "wbytes";
pub const IO_STAT_KEY_RIOS: &str = "rios";
pub const IO_STAT_KEY_WIOS: &str = "wios";
pub const IO_STAT_KEY_DBYTES: &str = "dbytes";
pub const IO_STAT_KEY_DIOS: &str = "dios";

// ---------------------------------------------------------------------------
// Per-device disambiguator separator (e.g. "8:0 rbps=1048576")
// ---------------------------------------------------------------------------

pub const BLKCG_DEV_SEP: u8 = b':';

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_control_files_have_io_prefix() {
        let files = [
            IO_WEIGHT,
            IO_MAX,
            IO_STAT,
            IO_PRESSURE,
            IO_LATENCY,
            IO_COST_QOS,
            IO_COST_MODEL,
            IO_BFQ_WEIGHT,
        ];
        for &v in &files {
            assert!(v.starts_with("io."));
        }
        // All distinct.
        for (i, &a) in files.iter().enumerate() {
            for &b in &files[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn test_weight_bounds() {
        assert_eq!(CGROUP_WEIGHT_MIN, 1);
        assert_eq!(CGROUP_WEIGHT_MAX, 10_000);
        assert_eq!(CGROUP_WEIGHT_DFL, 100);
        assert!(CGROUP_WEIGHT_MIN <= CGROUP_WEIGHT_DFL);
        assert!(CGROUP_WEIGHT_DFL <= CGROUP_WEIGHT_MAX);
        // Default sits squarely in the middle of the geometric range.
        assert_eq!(CGROUP_WEIGHT_DFL * CGROUP_WEIGHT_DFL, CGROUP_WEIGHT_MAX);
    }

    #[test]
    fn test_io_max_keys_paired_rw() {
        // Read/write byte-rate pair.
        assert_eq!(IO_MAX_KEY_RBPS, "rbps");
        assert_eq!(IO_MAX_KEY_WBPS, "wbps");
        // Read/write IOPS pair.
        assert_eq!(IO_MAX_KEY_RIOPS, "riops");
        assert_eq!(IO_MAX_KEY_WIOPS, "wiops");
        // R/W variants differ only in their leading letter.
        assert_eq!(&IO_MAX_KEY_RBPS[1..], &IO_MAX_KEY_WBPS[1..]);
        assert_eq!(&IO_MAX_KEY_RIOPS[1..], &IO_MAX_KEY_WIOPS[1..]);
        // "max" sentinel value clears a limit.
        assert_eq!(IO_MAX_VALUE_MAX, "max");
    }

    #[test]
    fn test_io_stat_keys_distinct() {
        let s = [
            IO_STAT_KEY_RBYTES,
            IO_STAT_KEY_WBYTES,
            IO_STAT_KEY_RIOS,
            IO_STAT_KEY_WIOS,
            IO_STAT_KEY_DBYTES,
            IO_STAT_KEY_DIOS,
        ];
        for (i, &a) in s.iter().enumerate() {
            for &b in &s[i + 1..] {
                assert_ne!(a, b);
            }
        }
        // Byte counters end in "bytes"; IOPS counters end in "ios".
        for &v in &[IO_STAT_KEY_RBYTES, IO_STAT_KEY_WBYTES, IO_STAT_KEY_DBYTES] {
            assert!(v.ends_with("bytes"));
        }
        for &v in &[IO_STAT_KEY_RIOS, IO_STAT_KEY_WIOS, IO_STAT_KEY_DIOS] {
            assert!(v.ends_with("ios"));
        }
    }

    #[test]
    fn test_dev_separator_is_colon() {
        assert_eq!(BLKCG_DEV_SEP, b':');
        // The "8:0" form encodes "<major>:<minor>" — the same separator
        // as in /dev/MAJOR:MINOR formatting.
        assert_eq!(BLKCG_DEV_SEP, 0x3A);
    }
}
