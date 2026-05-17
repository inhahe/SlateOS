//! `<linux/pm_qos.h>` — PM Quality of Service (PM QoS) constants.
//!
//! PM QoS allows drivers and userspace to express performance/latency
//! constraints that the power management subsystem must respect. For
//! example, a real-time audio driver can request that CPU idle states
//! with >1ms exit latency not be used. The framework aggregates
//! requests from all sources and provides the effective constraint
//! to governors and drivers. Supports per-device QoS (resume latency,
//! active-state latency tolerance) and system-wide QoS (CPU DMA
//! latency).

// ---------------------------------------------------------------------------
// PM QoS classes
// ---------------------------------------------------------------------------

/// CPU DMA latency (system-wide, microseconds).
pub const PM_QOS_CPU_DMA_LATENCY: u32 = 1;
/// Network latency (deprecated but still in API).
pub const PM_QOS_NETWORK_LATENCY: u32 = 2;
/// Network throughput (deprecated but still in API).
pub const PM_QOS_NETWORK_THROUGHPUT: u32 = 3;
/// Memory bandwidth (system-wide).
pub const PM_QOS_MEMORY_BANDWIDTH: u32 = 4;

// ---------------------------------------------------------------------------
// PM QoS default/special values
// ---------------------------------------------------------------------------

/// Default value (no constraint, expressed as max latency).
pub const PM_QOS_DEFAULT_VALUE: i32 = -1;
/// Latency tolerance: no constraint.
pub const PM_QOS_LATENCY_TOLERANCE_NO_CONSTRAINT: i32 = -1;
/// Latency tolerance: any value acceptable.
pub const PM_QOS_LATENCY_ANY: i32 = 0;
/// CPU DMA latency default (no constraint).
pub const PM_QOS_CPU_DMA_LAT_DEFAULT_VALUE: u32 = 0x7FFF_FFFF;
/// Resume latency: no constraint.
pub const PM_QOS_RESUME_LATENCY_NO_CONSTRAINT: u32 = 0x7FFF_FFFF;
/// Resume latency: no power off allowed.
pub const PM_QOS_RESUME_LATENCY_NO_CONSTRAINT_NS: u32 = 0x7FFF_FFFE;

// ---------------------------------------------------------------------------
// PM QoS request types
// ---------------------------------------------------------------------------

/// Request not yet allocated.
pub const PM_QOS_REQ_UNINIT: u32 = 0;
/// Active request (affecting aggregation).
pub const PM_QOS_REQ_ACTIVE: u32 = 1;
/// Inactive request (registered but not participating).
pub const PM_QOS_REQ_INACTIVE: u32 = 2;

// ---------------------------------------------------------------------------
// PM QoS aggregation types
// ---------------------------------------------------------------------------

/// Minimum value wins (for latency: tightest constraint).
pub const PM_QOS_MIN: u32 = 1;
/// Maximum value wins (for throughput: highest demand).
pub const PM_QOS_MAX: u32 = 2;
/// Sum of all values (for bandwidth: total demand).
pub const PM_QOS_SUM: u32 = 3;

// ---------------------------------------------------------------------------
// Per-device PM QoS flags
// ---------------------------------------------------------------------------

/// Device flags: no power off.
pub const PM_QOS_FLAG_NO_POWER_OFF: u32 = 1 << 0;
/// Device flags: remote wakeup.
pub const PM_QOS_FLAG_REMOTE_WAKEUP: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
        let classes = [
            PM_QOS_CPU_DMA_LATENCY, PM_QOS_NETWORK_LATENCY,
            PM_QOS_NETWORK_THROUGHPUT, PM_QOS_MEMORY_BANDWIDTH,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_request_types_distinct() {
        let types = [PM_QOS_REQ_UNINIT, PM_QOS_REQ_ACTIVE, PM_QOS_REQ_INACTIVE];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_aggregation_types_distinct() {
        let agg = [PM_QOS_MIN, PM_QOS_MAX, PM_QOS_SUM];
        for i in 0..agg.len() {
            for j in (i + 1)..agg.len() {
                assert_ne!(agg[i], agg[j]);
            }
        }
    }

    #[test]
    fn test_device_flags_no_overlap() {
        let flags = [PM_QOS_FLAG_NO_POWER_OFF, PM_QOS_FLAG_REMOTE_WAKEUP];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_no_constraint_values() {
        assert_eq!(PM_QOS_CPU_DMA_LAT_DEFAULT_VALUE, 0x7FFF_FFFF);
        assert_eq!(PM_QOS_RESUME_LATENCY_NO_CONSTRAINT, 0x7FFF_FFFF);
        assert_ne!(
            PM_QOS_RESUME_LATENCY_NO_CONSTRAINT,
            PM_QOS_RESUME_LATENCY_NO_CONSTRAINT_NS
        );
    }
}
