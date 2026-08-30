//! `<linux/pm_qos.h>` — Power Management Quality of Service constants.
//!
//! PM QoS allows drivers and userspace to register latency/throughput
//! constraints. The kernel aggregates constraints and ensures the
//! system meets the most demanding requirement. Used for CPU idle
//! state selection, network latency targets, and device PM.

// ---------------------------------------------------------------------------
// PM QoS classes
// ---------------------------------------------------------------------------

/// Reserved / invalid class.
pub const PM_QOS_RESERVED: i32 = 0;
/// CPU DMA latency class (microseconds).
pub const PM_QOS_CPU_DMA_LATENCY: i32 = 1;
/// Number of PM QoS classes.
pub const PM_QOS_NUM_CLASSES: i32 = 2;

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

/// Default CPU DMA latency (no constraint).
pub const PM_QOS_CPU_DMA_LAT_DEFAULT_VALUE: i32 = 2_000_000_000;
/// Default (no constraint) — same as `s32::MAX` equivalent.
pub const PM_QOS_DEFAULT_VALUE: i32 = i32::MAX;
/// Latency tolerance: no constraint.
pub const PM_QOS_LATENCY_TOLERANCE_NO_CONSTRAINT: i32 = 0;
/// Latency tolerance: any value.
pub const PM_QOS_LATENCY_ANY: i32 = i32::MAX;

// ---------------------------------------------------------------------------
// Aggregation types
// ---------------------------------------------------------------------------

/// Minimum value wins.
pub const PM_QOS_MIN: u32 = 0;
/// Maximum value wins.
pub const PM_QOS_MAX: u32 = 1;
/// Sum of all constraints.
pub const PM_QOS_SUM: u32 = 2;

// ---------------------------------------------------------------------------
// Device PM QoS flags
// ---------------------------------------------------------------------------

/// Resume latency flag.
pub const DEV_PM_QOS_RESUME_LATENCY: u32 = 1;
/// Latency tolerance flag.
pub const DEV_PM_QOS_LATENCY_TOLERANCE: u32 = 2;
/// Min frequency flag.
pub const DEV_PM_QOS_MIN_FREQUENCY: u32 = 3;
/// Max frequency flag.
pub const DEV_PM_QOS_MAX_FREQUENCY: u32 = 4;
/// Flags type.
pub const DEV_PM_QOS_FLAGS: u32 = 5;

// ---------------------------------------------------------------------------
// PM QoS flag bits
// ---------------------------------------------------------------------------

/// No power off flag.
pub const PM_QOS_FLAG_NO_POWER_OFF: u32 = 1 << 0;
/// Remote wakeup flag.
pub const PM_QOS_FLAG_REMOTE_WAKEUP: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Frequency constraints
// ---------------------------------------------------------------------------

/// Min frequency: no constraint.
pub const PM_QOS_MIN_FREQUENCY_DEFAULT_VALUE: u32 = 0;
/// Max frequency: no constraint.
pub const PM_QOS_MAX_FREQUENCY_DEFAULT_VALUE: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classes_distinct() {
        let classes = [PM_QOS_RESERVED, PM_QOS_CPU_DMA_LATENCY];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_num_classes() {
        assert_eq!(PM_QOS_NUM_CLASSES, 2);
    }

    #[test]
    fn test_default_values() {
        assert_eq!(PM_QOS_DEFAULT_VALUE, i32::MAX);
        assert_eq!(PM_QOS_LATENCY_ANY, i32::MAX);
        assert_eq!(PM_QOS_LATENCY_TOLERANCE_NO_CONSTRAINT, 0);
    }

    #[test]
    fn test_aggregation_types_distinct() {
        let types = [PM_QOS_MIN, PM_QOS_MAX, PM_QOS_SUM];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dev_pm_qos_types_distinct() {
        let types = [
            DEV_PM_QOS_RESUME_LATENCY,
            DEV_PM_QOS_LATENCY_TOLERANCE,
            DEV_PM_QOS_MIN_FREQUENCY,
            DEV_PM_QOS_MAX_FREQUENCY,
            DEV_PM_QOS_FLAGS,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_flag_bits_powers_of_two() {
        let flags = [PM_QOS_FLAG_NO_POWER_OFF, PM_QOS_FLAG_REMOTE_WAKEUP];
        for flag in &flags {
            assert!(flag.is_power_of_two());
        }
    }

    #[test]
    fn test_flag_bits_no_overlap() {
        assert_eq!(PM_QOS_FLAG_NO_POWER_OFF & PM_QOS_FLAG_REMOTE_WAKEUP, 0);
    }

    #[test]
    fn test_frequency_defaults() {
        assert_eq!(PM_QOS_MIN_FREQUENCY_DEFAULT_VALUE, 0);
        assert_eq!(PM_QOS_MAX_FREQUENCY_DEFAULT_VALUE, u32::MAX);
    }
}
