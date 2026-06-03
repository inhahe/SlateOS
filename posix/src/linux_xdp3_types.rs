//! `<linux/if_xdp.h>` — Additional XDP (eXpress Data Path) constants.
//!
//! Supplementary XDP constants covering ring flags,
//! descriptor flags, and umem configuration options.

// ---------------------------------------------------------------------------
// XDP ring flags
// ---------------------------------------------------------------------------

/// Need wakeup flag — ring requires explicit wakeup.
pub const XDP_RING_NEED_WAKEUP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// XDP bind flags
// ---------------------------------------------------------------------------

/// Shared UMEM.
pub const XDP_SHARED_UMEM: u16 = 1 << 0;
/// Copy mode.
pub const XDP_COPY: u16 = 1 << 1;
/// Zero-copy mode.
pub const XDP_ZEROCOPY: u16 = 1 << 2;
/// Use need wakeup.
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;
/// Use SG (scatter-gather).
pub const XDP_USE_SG: u16 = 1 << 4;

// ---------------------------------------------------------------------------
// XDP UMEM registration flags
// ---------------------------------------------------------------------------

/// Unaligned chunk mode.
pub const XDP_UMEM_UNALIGNED_CHUNK_FLAG: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// XDP descriptor options
// ---------------------------------------------------------------------------

/// TX metadata available.
pub const XDP_TX_METADATA: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// XDP socket options (for setsockopt)
// ---------------------------------------------------------------------------

/// Receive ring.
pub const XDP_RX_RING: i32 = 1;
/// Transmit ring.
pub const XDP_TX_RING: i32 = 2;
/// UMEM registration.
pub const XDP_UMEM_REG: i32 = 3;
/// UMEM fill ring.
pub const XDP_UMEM_FILL_RING: i32 = 4;
/// UMEM completion ring.
pub const XDP_UMEM_COMPLETION_RING: i32 = 5;
/// Statistics.
pub const XDP_STATISTICS: i32 = 7;
/// Socket options.
pub const XDP_OPTIONS: i32 = 8;

// ---------------------------------------------------------------------------
// XDP statistics fields
// ---------------------------------------------------------------------------

/// Dropped packets.
pub const XDP_STATS_RX_DROPPED: u32 = 0;
/// Invalid descriptors.
pub const XDP_STATS_RX_INVALID_DESCS: u32 = 1;
/// TX invalid descriptors.
pub const XDP_STATS_TX_INVALID_DESCS: u32 = 2;
/// RX ring full.
pub const XDP_STATS_RX_RING_FULL: u32 = 3;
/// RX fill ring empty.
pub const XDP_STATS_RX_FILL_RING_EMPTY: u32 = 4;
/// TX ring empty.
pub const XDP_STATS_TX_RING_EMPTY: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_flags_power_of_two() {
        assert!(XDP_SHARED_UMEM.is_power_of_two());
        assert!(XDP_COPY.is_power_of_two());
        assert!(XDP_ZEROCOPY.is_power_of_two());
        assert!(XDP_USE_NEED_WAKEUP.is_power_of_two());
        assert!(XDP_USE_SG.is_power_of_two());
    }

    #[test]
    fn test_bind_flags_no_overlap() {
        let flags = [
            XDP_SHARED_UMEM,
            XDP_COPY,
            XDP_ZEROCOPY,
            XDP_USE_NEED_WAKEUP,
            XDP_USE_SG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sockopt_distinct() {
        let opts = [
            XDP_RX_RING,
            XDP_TX_RING,
            XDP_UMEM_REG,
            XDP_UMEM_FILL_RING,
            XDP_UMEM_COMPLETION_RING,
            XDP_STATISTICS,
            XDP_OPTIONS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_stats_fields_distinct() {
        let fields = [
            XDP_STATS_RX_DROPPED,
            XDP_STATS_RX_INVALID_DESCS,
            XDP_STATS_TX_INVALID_DESCS,
            XDP_STATS_RX_RING_FULL,
            XDP_STATS_RX_FILL_RING_EMPTY,
            XDP_STATS_TX_RING_EMPTY,
        ];
        for i in 0..fields.len() {
            for j in (i + 1)..fields.len() {
                assert_ne!(fields[i], fields[j]);
            }
        }
    }

    #[test]
    fn test_ring_need_wakeup() {
        assert!(XDP_RING_NEED_WAKEUP.is_power_of_two());
    }

    #[test]
    fn test_umem_flag() {
        assert!(XDP_UMEM_UNALIGNED_CHUNK_FLAG.is_power_of_two());
    }
}
