//! `<linux/gen_stats.h>` — Generic statistics for TC (Traffic Control).
//!
//! These nested attributes carry statistics within netlink messages
//! for qdiscs, classes, and filters in the Linux traffic control
//! subsystem. Used by `tc -s qdisc show`, etc.

// ---------------------------------------------------------------------------
// Stats types (TCA_STATS_*)
// ---------------------------------------------------------------------------

/// Unspecified.
pub const TCA_STATS_UNSPEC: u16 = 0;
/// Basic stats (GnetStatsBasic).
pub const TCA_STATS_BASIC: u16 = 1;
/// Rate estimator stats.
pub const TCA_STATS_RATE_EST: u16 = 2;
/// Queue stats.
pub const TCA_STATS_QUEUE: u16 = 3;
/// Application-specific stats.
pub const TCA_STATS_APP: u16 = 4;
/// Rate estimator v2.
pub const TCA_STATS_RATE_EST64: u16 = 5;
/// Pad.
pub const TCA_STATS_PAD: u16 = 6;
/// Basic hardware stats.
pub const TCA_STATS_BASIC_HW: u16 = 7;
/// Per-CPU stats.
pub const TCA_STATS_PKT64: u16 = 8;

// ---------------------------------------------------------------------------
// GnetStatsBasic (basic byte/packet counters)
// ---------------------------------------------------------------------------

/// Basic statistics (16 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GnetStatsBasic {
    /// Bytes transmitted.
    pub bytes: u64,
    /// Packets transmitted.
    pub packets: u32,
    /// Padding.
    _pad: u32,
}

impl GnetStatsBasic {
    /// Create zeroed basic stats.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// GnetStatsQueue (queue depth stats)
// ---------------------------------------------------------------------------

/// Queue statistics (20 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GnetStatsQueue {
    /// Current queue length (bytes).
    pub qlen: u32,
    /// Backlog (bytes).
    pub backlog: u32,
    /// Total drops.
    pub drops: u32,
    /// Total requeues.
    pub requeues: u32,
    /// Total overlimits.
    pub overlimits: u32,
}

impl GnetStatsQueue {
    /// Create zeroed queue stats.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// GnetStatsRateEst (rate estimator)
// ---------------------------------------------------------------------------

/// Rate estimation (8 bytes).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GnetStatsRateEst {
    /// Bytes per second.
    pub bps: u32,
    /// Packets per second.
    pub pps: u32,
}

impl GnetStatsRateEst {
    /// Create zeroed rate estimation.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stats_types_distinct() {
        let types = [
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
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_basic_size() {
        assert_eq!(core::mem::size_of::<GnetStatsBasic>(), 16);
    }

    #[test]
    fn test_queue_size() {
        assert_eq!(core::mem::size_of::<GnetStatsQueue>(), 20);
    }

    #[test]
    fn test_rate_est_size() {
        assert_eq!(core::mem::size_of::<GnetStatsRateEst>(), 8);
    }
}
