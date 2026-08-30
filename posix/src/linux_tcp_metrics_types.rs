//! `<linux/tcp_metrics.h>` — TCP metrics-cache netlink constants.
//!
//! Constants for the `ip tcp_metrics` userspace tool to inspect and
//! flush the kernel's per-destination TCP RTT/congestion-window cache.
//! Carried over generic-netlink.

// ---------------------------------------------------------------------------
// Generic-netlink family
// ---------------------------------------------------------------------------

/// Family-name string registered with genl.
pub const TCP_METRICS_GENL_NAME: &str = "tcp_metrics";
/// Family version.
pub const TCP_METRICS_GENL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Generic-netlink commands
// ---------------------------------------------------------------------------

/// Get one (or all) cached metric entries.
pub const TCP_METRICS_CMD_GET: u32 = 1;
/// Delete a single cached entry.
pub const TCP_METRICS_CMD_DEL: u32 = 2;

// ---------------------------------------------------------------------------
// Top-level attribute IDs (TCP_METRICS_ATTR_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCP_METRICS_ATTR_UNSPEC: u16 = 0;
/// IPv4 address.
pub const TCP_METRICS_ATTR_ADDR_IPV4: u16 = 1;
/// IPv6 address.
pub const TCP_METRICS_ATTR_ADDR_IPV6: u16 = 2;
/// Cookie age (seconds since cookie).
pub const TCP_METRICS_ATTR_AGE: u16 = 3;
/// Source address IPv4 (for source-specific entries).
pub const TCP_METRICS_ATTR_TW_TSVAL: u16 = 4;
/// Source address IPv6.
pub const TCP_METRICS_ATTR_TW_TS_STAMP: u16 = 5;
/// Per-metric vals nested attribute.
pub const TCP_METRICS_ATTR_VALS: u16 = 6;
/// Fast-open cookie.
pub const TCP_METRICS_ATTR_FOPEN_MSS: u16 = 7;
/// Fast-open syn-loss counter.
pub const TCP_METRICS_ATTR_FOPEN_SYN_DROPS: u16 = 8;
/// Fast-open syn-loss timestamp.
pub const TCP_METRICS_ATTR_FOPEN_SYN_DROP_TS: u16 = 9;
/// Fast-open cookie bytes.
pub const TCP_METRICS_ATTR_FOPEN_COOKIE: u16 = 10;
/// Source-address used for the entry.
pub const TCP_METRICS_ATTR_SADDR_IPV4: u16 = 11;
/// Source-address IPv6 used for the entry.
pub const TCP_METRICS_ATTR_SADDR_IPV6: u16 = 12;

// ---------------------------------------------------------------------------
// Per-metric value IDs (TCP_METRIC_*) nested inside VALS
// ---------------------------------------------------------------------------

/// Smoothed RTT (microseconds).
pub const TCP_METRIC_RTT: u32 = 0;
/// RTT variance.
pub const TCP_METRIC_RTTVAR: u32 = 1;
/// Slow-start threshold.
pub const TCP_METRIC_SSTHRESH: u32 = 2;
/// Congestion window.
pub const TCP_METRIC_CWND: u32 = 3;
/// Reordering threshold.
pub const TCP_METRIC_REORDERING: u32 = 4;
/// RTT in microseconds (high precision).
pub const TCP_METRIC_RTT_US: u32 = 5;
/// RTT variance in microseconds.
pub const TCP_METRIC_RTTVAR_US: u32 = 6;
/// Total metric count.
pub const TCP_METRIC_MAX_KERNEL: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_family_name_and_version() {
        assert_eq!(TCP_METRICS_GENL_NAME, "tcp_metrics");
        assert_eq!(TCP_METRICS_GENL_VERSION, 1);
    }

    #[test]
    fn test_cmds_distinct() {
        assert_ne!(TCP_METRICS_CMD_GET, TCP_METRICS_CMD_DEL);
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            TCP_METRICS_ATTR_UNSPEC,
            TCP_METRICS_ATTR_ADDR_IPV4,
            TCP_METRICS_ATTR_ADDR_IPV6,
            TCP_METRICS_ATTR_AGE,
            TCP_METRICS_ATTR_TW_TSVAL,
            TCP_METRICS_ATTR_TW_TS_STAMP,
            TCP_METRICS_ATTR_VALS,
            TCP_METRICS_ATTR_FOPEN_MSS,
            TCP_METRICS_ATTR_FOPEN_SYN_DROPS,
            TCP_METRICS_ATTR_FOPEN_SYN_DROP_TS,
            TCP_METRICS_ATTR_FOPEN_COOKIE,
            TCP_METRICS_ATTR_SADDR_IPV4,
            TCP_METRICS_ATTR_SADDR_IPV6,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_metrics_distinct_and_max_correct() {
        let metrics = [
            TCP_METRIC_RTT,
            TCP_METRIC_RTTVAR,
            TCP_METRIC_SSTHRESH,
            TCP_METRIC_CWND,
            TCP_METRIC_REORDERING,
            TCP_METRIC_RTT_US,
            TCP_METRIC_RTTVAR_US,
        ];
        for i in 0..metrics.len() {
            for j in (i + 1)..metrics.len() {
                assert_ne!(metrics[i], metrics[j]);
            }
            assert!(metrics[i] < TCP_METRIC_MAX_KERNEL);
        }
        // MAX must equal (highest_index + 1) so arrays sized by it
        // have a slot for every metric.
        assert_eq!(TCP_METRIC_MAX_KERNEL as usize, metrics.len());
    }
}
