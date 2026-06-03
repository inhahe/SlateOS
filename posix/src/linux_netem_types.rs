//! `<linux/pkt_sched.h>` — netem (network emulator) qdisc constants.
//!
//! Constants used to configure the `netem` queueing discipline from
//! userspace. tc-netem, network-emulation test rigs, and CI fault
//! injection tools consume these.

// ---------------------------------------------------------------------------
// Netem netlink attribute types (TCA_NETEM_*)
// ---------------------------------------------------------------------------

/// Unspecified attribute.
pub const TCA_NETEM_UNSPEC: u16 = 0;
/// Distribution table (delay jitter shape).
pub const TCA_NETEM_CORR: u16 = 1;
/// Delay distribution table contents.
pub const TCA_NETEM_DELAY_DIST: u16 = 2;
/// Reordering rate.
pub const TCA_NETEM_REORDER: u16 = 3;
/// Packet corruption rate.
pub const TCA_NETEM_CORRUPT: u16 = 4;
/// Burst loss model.
pub const TCA_NETEM_LOSS: u16 = 5;
/// Rate (per-interval byte rate).
pub const TCA_NETEM_RATE: u16 = 6;
/// ECN (Explicit Congestion Notification) mark on loss.
pub const TCA_NETEM_ECN: u16 = 7;
/// Configurable jitter distribution (64-bit).
pub const TCA_NETEM_RATE64: u16 = 8;
/// Delay (64-bit).
pub const TCA_NETEM_LATENCY64: u16 = 9;
/// Jitter (64-bit).
pub const TCA_NETEM_JITTER64: u16 = 10;
/// Number of slot configurations.
pub const TCA_NETEM_SLOT: u16 = 11;
/// Slot-distribution table.
pub const TCA_NETEM_SLOT_DIST: u16 = 12;
/// Prng configuration.
pub const TCA_NETEM_PRNG_SEED: u16 = 13;

// ---------------------------------------------------------------------------
// Loss-model selection (TCA_NETEM_LOSS payload type)
// ---------------------------------------------------------------------------

/// Gilbert-Elliot loss model selected.
pub const NETEM_LOSS_UNSPEC: u32 = 0;
/// Gilbert-Elliot (2-state) loss model.
pub const NETEM_LOSS_GI: u32 = 1;
/// 4-state Markov loss model.
pub const NETEM_LOSS_GE: u32 = 2;

// ---------------------------------------------------------------------------
// Distribution-table dimensions
// ---------------------------------------------------------------------------

/// Maximum number of entries in a netem delay-distribution table.
pub const NETEM_DIST_SCALE: u32 = 8192;
/// Maximum number of slots in the netem slot configuration array.
pub const NETEM_DIST_MAX: u32 = 16384;

// ---------------------------------------------------------------------------
// Rate-table cell size (bytes)
// ---------------------------------------------------------------------------

/// Default cell size used by netem when no rate-cell is specified.
pub const NETEM_RATE_CELL_DEFAULT: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCA_NETEM_UNSPEC,
            TCA_NETEM_CORR,
            TCA_NETEM_DELAY_DIST,
            TCA_NETEM_REORDER,
            TCA_NETEM_CORRUPT,
            TCA_NETEM_LOSS,
            TCA_NETEM_RATE,
            TCA_NETEM_ECN,
            TCA_NETEM_RATE64,
            TCA_NETEM_LATENCY64,
            TCA_NETEM_JITTER64,
            TCA_NETEM_SLOT,
            TCA_NETEM_SLOT_DIST,
            TCA_NETEM_PRNG_SEED,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_loss_models_distinct_and_unspec_zero() {
        assert_eq!(NETEM_LOSS_UNSPEC, 0);
        assert_ne!(NETEM_LOSS_GI, NETEM_LOSS_GE);
        assert_ne!(NETEM_LOSS_GI, NETEM_LOSS_UNSPEC);
    }

    #[test]
    fn test_distribution_sizes_sane() {
        // Both table sizes must be powers of two for the table-index
        // computation to use a simple bit-mask.
        assert!(NETEM_DIST_SCALE.is_power_of_two());
        assert!(NETEM_DIST_MAX.is_power_of_two());
        assert!(NETEM_DIST_SCALE <= NETEM_DIST_MAX);
    }
}
