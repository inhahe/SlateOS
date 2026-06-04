//! `block/blk-throttle.c` — cgroup-v2 I/O throttling (`io.max`,
//! `io.low`, `io.bfq.weight`) user-visible constants.
//!
//! The block-throttle layer enforces per-cgroup bandwidth (bytes/s)
//! and IOPS caps. Userspace writes these limits as space-separated
//! key=value pairs into `io.max`; the kernel parses them with the
//! constants below.

// ---------------------------------------------------------------------------
// Throttle slice timing (ms)
// ---------------------------------------------------------------------------

/// Default throttle slice — accounting window over which BPS/IOPS
/// budgets are tallied.
pub const THROTL_DEFAULT_SLICE_MS: u32 = 100;

/// Minimum slice (1 ms) — below this, accounting is too coarse.
pub const THROTL_MIN_SLICE_MS: u32 = 1;

/// Maximum slice (1 s) — above this, latency spikes hide.
pub const THROTL_MAX_SLICE_MS: u32 = 1_000;

// ---------------------------------------------------------------------------
// Bandwidth (BPS) and IOPS bounds
// ---------------------------------------------------------------------------

/// Minimum bandwidth a cgroup may be capped to (1 byte/s).
pub const THROTL_MIN_BPS: u64 = 1;

/// Maximum bandwidth (sentinel meaning "no cap").
pub const THROTL_MAX_BPS: u64 = u64::MAX;

/// Minimum IOPS cap (1 op/s).
pub const THROTL_MIN_IOPS: u32 = 1;

/// Maximum IOPS cap (sentinel "no cap").
pub const THROTL_MAX_IOPS: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// `io.max` key names (parsed from /proc/cgroups io.max writes)
// ---------------------------------------------------------------------------

pub const IO_MAX_KEY_RBPS: &str = "rbps";
pub const IO_MAX_KEY_WBPS: &str = "wbps";
pub const IO_MAX_KEY_RIOPS: &str = "riops";
pub const IO_MAX_KEY_WIOPS: &str = "wiops";

/// Sentinel value (string) accepted in any of the four keys meaning
/// "no cap".
pub const IO_MAX_VALUE_MAX: &str = "max";

// ---------------------------------------------------------------------------
// `io.bfq.weight` bounds
// ---------------------------------------------------------------------------

pub const BFQ_WEIGHT_MIN: u32 = 1;
pub const BFQ_WEIGHT_MAX: u32 = 1_000;
pub const BFQ_WEIGHT_DEFAULT: u32 = 100;

// ---------------------------------------------------------------------------
// sysfs control attribute names
// ---------------------------------------------------------------------------

pub const SYSFS_IO_MAX: &str = "io.max";
pub const SYSFS_IO_LOW: &str = "io.low";
pub const SYSFS_IO_LATENCY: &str = "io.latency";
pub const SYSFS_IO_STAT: &str = "io.stat";
pub const SYSFS_IO_WEIGHT: &str = "io.weight";
pub const SYSFS_IO_BFQ_WEIGHT: &str = "io.bfq.weight";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slice_bounds_inclusive() {
        assert_eq!(THROTL_MIN_SLICE_MS, 1);
        assert_eq!(THROTL_DEFAULT_SLICE_MS, 100);
        assert_eq!(THROTL_MAX_SLICE_MS, 1_000);
        assert!(THROTL_MIN_SLICE_MS < THROTL_DEFAULT_SLICE_MS);
        assert!(THROTL_DEFAULT_SLICE_MS < THROTL_MAX_SLICE_MS);
        // Three orders of magnitude span.
        assert_eq!(THROTL_MAX_SLICE_MS / THROTL_MIN_SLICE_MS, 1_000);
    }

    #[test]
    fn test_rate_sentinels_are_max_typed() {
        assert_eq!(THROTL_MAX_BPS, u64::MAX);
        assert_eq!(THROTL_MAX_IOPS, u32::MAX);
        assert_eq!(THROTL_MIN_BPS, 1);
        assert_eq!(THROTL_MIN_IOPS, 1);
    }

    #[test]
    fn test_io_max_key_pairs() {
        // rbps/wbps form the BPS pair; riops/wiops form the IOPS pair.
        let bps = [IO_MAX_KEY_RBPS, IO_MAX_KEY_WBPS];
        let iops = [IO_MAX_KEY_RIOPS, IO_MAX_KEY_WIOPS];
        for &k in &bps {
            assert!(k.ends_with("bps"));
            assert!(!k.contains("iops"));
        }
        for &k in &iops {
            assert!(k.ends_with("iops"));
        }
        // Read keys start with 'r', write keys with 'w'.
        assert!(IO_MAX_KEY_RBPS.starts_with('r'));
        assert!(IO_MAX_KEY_WBPS.starts_with('w'));
        assert!(IO_MAX_KEY_RIOPS.starts_with('r'));
        assert!(IO_MAX_KEY_WIOPS.starts_with('w'));
        // "max" sentinel is short and lowercase.
        assert_eq!(IO_MAX_VALUE_MAX, "max");
    }

    #[test]
    fn test_bfq_weight_bounds() {
        assert_eq!(BFQ_WEIGHT_MIN, 1);
        assert_eq!(BFQ_WEIGHT_MAX, 1_000);
        assert_eq!(BFQ_WEIGHT_DEFAULT, 100);
        assert!(BFQ_WEIGHT_MIN <= BFQ_WEIGHT_DEFAULT);
        assert!(BFQ_WEIGHT_DEFAULT <= BFQ_WEIGHT_MAX);
        // Default sits at the geometric mean (10*MIN, MAX/10).
        assert_eq!(BFQ_WEIGHT_DEFAULT, BFQ_WEIGHT_MAX / 10);
    }

    #[test]
    fn test_sysfs_attrs_all_in_io_namespace() {
        let a = [
            SYSFS_IO_MAX,
            SYSFS_IO_LOW,
            SYSFS_IO_LATENCY,
            SYSFS_IO_STAT,
            SYSFS_IO_WEIGHT,
            SYSFS_IO_BFQ_WEIGHT,
        ];
        for (i, &x) in a.iter().enumerate() {
            assert!(x.starts_with("io."));
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
        // bfq.weight is a sub-namespace of weight.
        assert!(SYSFS_IO_BFQ_WEIGHT.ends_with(".weight"));
        assert!(SYSFS_IO_WEIGHT.ends_with(".weight"));
    }
}
