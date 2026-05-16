//! I/O cgroup controller extended constants.
//!
//! Extended constants for the I/O cgroup controller, covering the
//! io.cost (iocost) QoS model which provides latency-aware I/O
//! control. iocost measures device costs and adjusts allocations
//! to meet latency targets.

// ---------------------------------------------------------------------------
// iocost model parameters
// ---------------------------------------------------------------------------

/// Sequential read cost coefficient.
pub const IOCOST_COEF_SEQR: &str = "seqr";
/// Sequential write cost coefficient.
pub const IOCOST_COEF_SEQW: &str = "seqw";
/// Random read cost coefficient.
pub const IOCOST_COEF_RANDR: &str = "randr";
/// Random write cost coefficient.
pub const IOCOST_COEF_RANDW: &str = "randw";

// ---------------------------------------------------------------------------
// iocost QoS parameters
// ---------------------------------------------------------------------------

/// Enable iocost QoS.
pub const IOCOST_QOS_ENABLE: u32 = 1;
/// Disable iocost QoS.
pub const IOCOST_QOS_DISABLE: u32 = 0;

/// Read latency percentage target (permille).
pub const IOCOST_QOS_RLAT: &str = "rlat";
/// Write latency percentage target (permille).
pub const IOCOST_QOS_WLAT: &str = "wlat";
/// Minimum percentage.
pub const IOCOST_QOS_MIN: &str = "min";
/// Maximum percentage.
pub const IOCOST_QOS_MAX: &str = "max";

// ---------------------------------------------------------------------------
// I/O priority classes (ioprio in cgroup context)
// ---------------------------------------------------------------------------

/// None / best-effort default.
pub const IOPRIO_CLASS_NONE: u32 = 0;
/// Real-time I/O class.
pub const IOPRIO_CLASS_RT: u32 = 1;
/// Best-effort I/O class.
pub const IOPRIO_CLASS_BE: u32 = 2;
/// Idle I/O class.
pub const IOPRIO_CLASS_IDLE: u32 = 3;

/// Number of I/O priority levels per class.
pub const IOPRIO_NR_LEVELS: u32 = 8;
/// Bits for priority class.
pub const IOPRIO_CLASS_SHIFT: u32 = 13;

// ---------------------------------------------------------------------------
// BFQ (Budget Fair Queueing) weight
// ---------------------------------------------------------------------------

/// BFQ minimum weight.
pub const BFQ_WEIGHT_MIN: u32 = 1;
/// BFQ default weight.
pub const BFQ_WEIGHT_DEFAULT: u32 = 100;
/// BFQ maximum weight.
pub const BFQ_WEIGHT_MAX: u32 = 1000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coefs_distinct() {
        let coefs = [
            IOCOST_COEF_SEQR, IOCOST_COEF_SEQW,
            IOCOST_COEF_RANDR, IOCOST_COEF_RANDW,
        ];
        for i in 0..coefs.len() {
            for j in (i + 1)..coefs.len() {
                assert_ne!(coefs[i], coefs[j]);
            }
        }
    }

    #[test]
    fn test_qos_enable_disable() {
        assert_ne!(IOCOST_QOS_ENABLE, IOCOST_QOS_DISABLE);
    }

    #[test]
    fn test_qos_params_distinct() {
        let params = [
            IOCOST_QOS_RLAT, IOCOST_QOS_WLAT,
            IOCOST_QOS_MIN, IOCOST_QOS_MAX,
        ];
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                assert_ne!(params[i], params[j]);
            }
        }
    }

    #[test]
    fn test_ioprio_classes_distinct() {
        let classes = [
            IOPRIO_CLASS_NONE, IOPRIO_CLASS_RT,
            IOPRIO_CLASS_BE, IOPRIO_CLASS_IDLE,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_ioprio_levels() {
        assert_eq!(IOPRIO_NR_LEVELS, 8);
        assert_eq!(IOPRIO_CLASS_SHIFT, 13);
    }

    #[test]
    fn test_bfq_weight_range() {
        assert!(BFQ_WEIGHT_MIN < BFQ_WEIGHT_DEFAULT);
        assert!(BFQ_WEIGHT_DEFAULT < BFQ_WEIGHT_MAX);
    }
}
