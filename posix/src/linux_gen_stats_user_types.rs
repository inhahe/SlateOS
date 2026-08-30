//! `<linux/gen_stats.h>` — generic statistics over netlink (tc, qdisc).
//!
//! `tc -s` (traffic control), nftables counter rules, and BPF
//! tracepoint scrapers walk the gnet_stats attributes via NETLINK_ROUTE
//! to read per-qdisc/queue/class byte and packet counters.

// ---------------------------------------------------------------------------
// TLV attribute kinds (struct gnet_stats_*; enum)
// ---------------------------------------------------------------------------

/// Reserved sentinel.
pub const TCA_STATS_UNSPEC: u32 = 0;
/// Basic counters (bytes, packets) — gnet_stats_basic.
pub const TCA_STATS_BASIC: u32 = 1;
/// Rate estimator — gnet_stats_rate_est.
pub const TCA_STATS_RATE_EST: u32 = 2;
/// Queue stats — gnet_stats_queue.
pub const TCA_STATS_QUEUE: u32 = 3;
/// Application-private TLV.
pub const TCA_STATS_APP: u32 = 4;
/// 64-bit rate estimator — gnet_stats_rate_est64.
pub const TCA_STATS_RATE_EST64: u32 = 5;
/// Padding marker.
pub const TCA_STATS_PAD: u32 = 6;
/// Basic 64-bit counters — gnet_stats_basic_hw.
pub const TCA_STATS_BASIC_HW: u32 = 7;
/// Per-CPU basic counters.
pub const TCA_STATS_PKT64: u32 = 8;

// ---------------------------------------------------------------------------
// Struct sizes (on the wire)
// ---------------------------------------------------------------------------

/// `struct gnet_stats_basic` size in bytes.
pub const GNET_STATS_BASIC_SIZE: u32 = 16;
/// `struct gnet_stats_rate_est` size.
pub const GNET_STATS_RATE_EST_SIZE: u32 = 8;
/// `struct gnet_stats_rate_est64` size.
pub const GNET_STATS_RATE_EST64_SIZE: u32 = 16;
/// `struct gnet_stats_queue` size.
pub const GNET_STATS_QUEUE_SIZE: u32 = 20;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attribute_ids_distinct_and_dense() {
        let a = [
            TCA_STATS_UNSPEC,
            TCA_STATS_BASIC,
            TCA_STATS_RATE_EST,
            TCA_STATS_QUEUE,
            TCA_STATS_APP,
            TCA_STATS_RATE_EST64,
            TCA_STATS_PAD,
            TCA_STATS_BASIC_HW,
            TCA_STATS_PKT64,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_struct_sizes_match_field_layout() {
        // gnet_stats_basic = u64 bytes + u32 packets + 4 bytes padding.
        assert_eq!(GNET_STATS_BASIC_SIZE, 16);
        // gnet_stats_rate_est = bps:u32 + pps:u32.
        assert_eq!(GNET_STATS_RATE_EST_SIZE, 8);
        // gnet_stats_rate_est64 = bps:u64 + pps:u64.
        assert_eq!(GNET_STATS_RATE_EST64_SIZE, 16);
        // gnet_stats_queue = qlen + backlog + drops + requeues + overlimits.
        assert_eq!(GNET_STATS_QUEUE_SIZE, 20);
    }
}
