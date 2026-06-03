//! `<linux/pkt_sched.h>` — `netem` traffic-control qdisc ABI.
//!
//! `netem` is the kernel's network emulation qdisc — it adds latency,
//! loss, duplication, reordering, and corruption to outbound traffic.
//! `tc qdisc add ... netem delay 100ms loss 1%` is the canonical
//! integration test for distributed systems. The netlink attribute
//! types below define the qdisc's configuration vocabulary.

// ---------------------------------------------------------------------------
// Qdisc identifier
// ---------------------------------------------------------------------------

pub const NETEM_QDISC_KIND: &str = "netem";
pub const NETEM_DIST_SCALE: u32 = 8192;
pub const NETEM_DIST_MAX: u32 = 16384;

// ---------------------------------------------------------------------------
// `TCA_NETEM_*` netlink attribute types
// ---------------------------------------------------------------------------

pub const TCA_NETEM_UNSPEC: u32 = 0;
pub const TCA_NETEM_CORR: u32 = 1;
pub const TCA_NETEM_DELAY_DIST: u32 = 2;
pub const TCA_NETEM_REORDER: u32 = 3;
pub const TCA_NETEM_CORRUPT: u32 = 4;
pub const TCA_NETEM_LOSS: u32 = 5;
pub const TCA_NETEM_RATE: u32 = 6;
pub const TCA_NETEM_ECN: u32 = 7;
pub const TCA_NETEM_RATE64: u32 = 8;
pub const TCA_NETEM_PAD: u32 = 9;
pub const TCA_NETEM_LATENCY64: u32 = 10;
pub const TCA_NETEM_JITTER64: u32 = 11;
pub const TCA_NETEM_SLOT: u32 = 12;
pub const TCA_NETEM_SLOT_DIST: u32 = 13;

// ---------------------------------------------------------------------------
// Loss model selectors (`NETEM_LOSS_*`)
// ---------------------------------------------------------------------------

pub const NETEM_LOSS_UNSPEC: u32 = 0;
pub const NETEM_LOSS_GI: u32 = 1;
pub const NETEM_LOSS_GE: u32 = 2;

// ---------------------------------------------------------------------------
// Delay distribution presets accepted by the userspace `tc netem` parser
// ---------------------------------------------------------------------------

pub const NETEM_DIST_UNIFORM: &str = "uniform";
pub const NETEM_DIST_NORMAL: &str = "normal";
pub const NETEM_DIST_PARETO: &str = "pareto";
pub const NETEM_DIST_PARETONORMAL: &str = "paretonormal";

// ---------------------------------------------------------------------------
// `netem`-specific compiled-in default limits
// ---------------------------------------------------------------------------

/// Default queue length (packets).
pub const NETEM_DEFAULT_LIMIT: u32 = 1000;
/// Default jitter (microseconds).
pub const NETEM_DEFAULT_JITTER_US: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qdisc_kind_string() {
        assert_eq!(NETEM_QDISC_KIND, "netem");
    }

    #[test]
    fn test_attribute_types_dense_0_to_13() {
        let a = [
            TCA_NETEM_UNSPEC,
            TCA_NETEM_CORR,
            TCA_NETEM_DELAY_DIST,
            TCA_NETEM_REORDER,
            TCA_NETEM_CORRUPT,
            TCA_NETEM_LOSS,
            TCA_NETEM_RATE,
            TCA_NETEM_ECN,
            TCA_NETEM_RATE64,
            TCA_NETEM_PAD,
            TCA_NETEM_LATENCY64,
            TCA_NETEM_JITTER64,
            TCA_NETEM_SLOT,
            TCA_NETEM_SLOT_DIST,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_loss_model_dense() {
        assert_eq!(NETEM_LOSS_UNSPEC, 0);
        assert_eq!(NETEM_LOSS_GI, 1);
        assert_eq!(NETEM_LOSS_GE, 2);
    }

    #[test]
    fn test_distribution_strings_distinct() {
        let d = [
            NETEM_DIST_UNIFORM,
            NETEM_DIST_NORMAL,
            NETEM_DIST_PARETO,
            NETEM_DIST_PARETONORMAL,
        ];
        for i in 0..d.len() {
            for j in (i + 1)..d.len() {
                assert_ne!(d[i], d[j]);
            }
        }
        // "paretonormal" extends "pareto".
        assert!(NETEM_DIST_PARETONORMAL.starts_with(NETEM_DIST_PARETO));
    }

    #[test]
    fn test_scaling_constants() {
        // 8192 is the fixed-point scaling factor used by the distribution table.
        assert_eq!(NETEM_DIST_SCALE, 8192);
        assert!(NETEM_DIST_SCALE.is_power_of_two());
        // The maximum sampled value matches twice the scale (signed-range).
        assert_eq!(NETEM_DIST_MAX, 16384);
        assert_eq!(NETEM_DIST_MAX, 2 * NETEM_DIST_SCALE);
    }

    #[test]
    fn test_default_limits() {
        assert_eq!(NETEM_DEFAULT_LIMIT, 1000);
        assert_eq!(NETEM_DEFAULT_JITTER_US, 0);
    }
}
