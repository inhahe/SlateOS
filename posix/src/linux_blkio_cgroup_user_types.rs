//! `block/blk-cgroup.c` — cgroup-v1 `blkio` controller interface
//! (legacy, preserved for migration).
//!
//! cgroup-v1 split the I/O controller across many flat files
//! (`blkio.throttle.*`, `blkio.weight`, `blkio.bfq.*`). cgroup-v2
//! collapsed all of these into `io.max` / `io.weight`, but legacy
//! containers still write the v1 names — `runc`, `crun`, and
//! `kubelet` translate between them.

// ---------------------------------------------------------------------------
// blkio.weight bounds (the proportional-share knob)
// ---------------------------------------------------------------------------

/// Minimum proportional weight a cgroup may take.
pub const BLKIO_WEIGHT_MIN: u32 = 10;

/// Maximum proportional weight.
pub const BLKIO_WEIGHT_MAX: u32 = 1_000;

/// Default proportional weight (matches the kernel default).
pub const BLKIO_WEIGHT_DEFAULT: u32 = 500;

// ---------------------------------------------------------------------------
// Throttle policy bounds (`blkio.throttle.*`)
// ---------------------------------------------------------------------------

/// Sentinel meaning "no throttle" — written as "0" by user space.
pub const BLKIO_THROTTLE_UNLIMITED: u64 = 0;

/// Maximum representable throttle value.
pub const BLKIO_THROTTLE_MAX: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// cgroup-v1 attribute names
// ---------------------------------------------------------------------------

pub const BLKIO_WEIGHT: &str = "blkio.weight";
pub const BLKIO_WEIGHT_DEVICE: &str = "blkio.weight_device";
pub const BLKIO_LEAF_WEIGHT: &str = "blkio.leaf_weight";
pub const BLKIO_LEAF_WEIGHT_DEVICE: &str = "blkio.leaf_weight_device";

pub const BLKIO_THROTTLE_READ_BPS_DEVICE: &str =
    "blkio.throttle.read_bps_device";
pub const BLKIO_THROTTLE_WRITE_BPS_DEVICE: &str =
    "blkio.throttle.write_bps_device";
pub const BLKIO_THROTTLE_READ_IOPS_DEVICE: &str =
    "blkio.throttle.read_iops_device";
pub const BLKIO_THROTTLE_WRITE_IOPS_DEVICE: &str =
    "blkio.throttle.write_iops_device";

pub const BLKIO_IO_SERVICED: &str = "blkio.io_serviced";
pub const BLKIO_IO_SERVICE_BYTES: &str = "blkio.io_service_bytes";
pub const BLKIO_IO_QUEUED: &str = "blkio.io_queued";
pub const BLKIO_IO_MERGED: &str = "blkio.io_merged";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weight_bounds() {
        assert_eq!(BLKIO_WEIGHT_MIN, 10);
        assert_eq!(BLKIO_WEIGHT_MAX, 1_000);
        assert_eq!(BLKIO_WEIGHT_DEFAULT, 500);
        assert!(BLKIO_WEIGHT_MIN <= BLKIO_WEIGHT_DEFAULT);
        assert!(BLKIO_WEIGHT_DEFAULT <= BLKIO_WEIGHT_MAX);
        // Default sits at the midpoint of the range.
        assert_eq!(BLKIO_WEIGHT_DEFAULT, BLKIO_WEIGHT_MAX / 2);
        // 100x range from min to max.
        assert_eq!(BLKIO_WEIGHT_MAX / BLKIO_WEIGHT_MIN, 100);
    }

    #[test]
    fn test_throttle_sentinels() {
        assert_eq!(BLKIO_THROTTLE_UNLIMITED, 0);
        assert_eq!(BLKIO_THROTTLE_MAX, u64::MAX);
    }

    #[test]
    fn test_v1_attr_names_in_blkio_namespace() {
        let a = [
            BLKIO_WEIGHT,
            BLKIO_WEIGHT_DEVICE,
            BLKIO_LEAF_WEIGHT,
            BLKIO_LEAF_WEIGHT_DEVICE,
            BLKIO_THROTTLE_READ_BPS_DEVICE,
            BLKIO_THROTTLE_WRITE_BPS_DEVICE,
            BLKIO_THROTTLE_READ_IOPS_DEVICE,
            BLKIO_THROTTLE_WRITE_IOPS_DEVICE,
            BLKIO_IO_SERVICED,
            BLKIO_IO_SERVICE_BYTES,
            BLKIO_IO_QUEUED,
            BLKIO_IO_MERGED,
        ];
        for (i, &x) in a.iter().enumerate() {
            assert!(x.starts_with("blkio."));
            for &y in &a[i + 1..] {
                assert_ne!(x, y);
            }
        }
    }

    #[test]
    fn test_throttle_attrs_form_four_corners() {
        // 4 throttle dimensions: {read,write} x {bps,iops}.
        for v in [
            BLKIO_THROTTLE_READ_BPS_DEVICE,
            BLKIO_THROTTLE_WRITE_BPS_DEVICE,
            BLKIO_THROTTLE_READ_IOPS_DEVICE,
            BLKIO_THROTTLE_WRITE_IOPS_DEVICE,
        ] {
            assert!(v.starts_with("blkio.throttle."));
            assert!(v.ends_with("_device"));
        }
        // Pair: read_bps / write_bps.
        assert!(BLKIO_THROTTLE_READ_BPS_DEVICE.contains("read_bps"));
        assert!(BLKIO_THROTTLE_WRITE_BPS_DEVICE.contains("write_bps"));
        // Pair: read_iops / write_iops.
        assert!(BLKIO_THROTTLE_READ_IOPS_DEVICE.contains("read_iops"));
        assert!(BLKIO_THROTTLE_WRITE_IOPS_DEVICE.contains("write_iops"));
    }

    #[test]
    fn test_stat_attrs_distinguish_serviced_vs_bytes() {
        // The io_serviced/io_service_bytes pair shares the io_service stem.
        assert!(BLKIO_IO_SERVICED.starts_with("blkio.io_service"));
        assert!(BLKIO_IO_SERVICE_BYTES.starts_with("blkio.io_service"));
        assert_ne!(BLKIO_IO_SERVICED, BLKIO_IO_SERVICE_BYTES);
        // io_queued and io_merged are simpler stat counters.
        assert_eq!(BLKIO_IO_QUEUED, "blkio.io_queued");
        assert_eq!(BLKIO_IO_MERGED, "blkio.io_merged");
    }
}
