//! `<linux/if_xdp.h>` — AF_XDP (eXpress Data Path) socket constants.
//!
//! AF_XDP provides a high-performance path for network packet
//! processing, bypassing most of the kernel network stack. Packets
//! are delivered directly to userspace via shared UMEM rings.

// ---------------------------------------------------------------------------
// XDP socket bind flags
// ---------------------------------------------------------------------------

/// Share the UMEM with another XDP socket.
pub const XDP_SHARED_UMEM: u16 = 1 << 0;
/// Copy mode (kernel copies packets).
pub const XDP_COPY: u16 = 1 << 1;
/// Zero-copy mode (kernel maps device memory).
pub const XDP_ZEROCOPY: u16 = 1 << 2;
/// Use need wakeup flag.
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// XDP ring offset constants
// ---------------------------------------------------------------------------

/// RX ring descriptor offset.
pub const XDP_RX_RING: u32 = 1;
/// TX ring descriptor offset.
pub const XDP_TX_RING: u32 = 2;
/// UMEM fill ring offset.
pub const XDP_UMEM_FILL_RING: u32 = 5;
/// UMEM completion ring offset.
pub const XDP_UMEM_COMPLETION_RING: u32 = 6;

// ---------------------------------------------------------------------------
// XDP statistics counters
// ---------------------------------------------------------------------------

/// Packets dropped due to invalid descriptor.
pub const XDP_STATISTICS_RX_DROPPED: u32 = 0;
/// Packets dropped due to invalid ring entry.
pub const XDP_STATISTICS_RX_INVALID_DESCS: u32 = 1;
/// TX packets dropped.
pub const XDP_STATISTICS_TX_DROPPED: u32 = 2;
/// TX invalid descriptors.
pub const XDP_STATISTICS_TX_INVALID_DESCS: u32 = 3;
/// RX ring full events.
pub const XDP_STATISTICS_RX_RING_FULL: u32 = 4;
/// Fill ring empty events.
pub const XDP_STATISTICS_RX_FILL_RING_EMPTY: u32 = 5;
/// TX ring empty events.
pub const XDP_STATISTICS_TX_RING_EMPTY: u32 = 6;

// ---------------------------------------------------------------------------
// XDP socket options (SOL_XDP level)
// ---------------------------------------------------------------------------

/// Get/set XDP mmap offsets.
pub const XDP_MMAP_OFFSETS: u32 = 1;
/// Get/set RX ring size.
pub const XDP_RX_RING_SIZE: u32 = 2;
/// Get/set TX ring size.
pub const XDP_TX_RING_SIZE: u32 = 3;
/// Register UMEM.
pub const XDP_UMEM_REG: u32 = 4;
/// Get/set UMEM fill ring size.
pub const XDP_UMEM_FILL_SIZE: u32 = 5;
/// Get/set UMEM completion ring size.
pub const XDP_UMEM_COMPLETION_SIZE: u32 = 6;
/// Get statistics.
pub const XDP_STATISTICS: u32 = 7;
/// Get options.
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// XDP program actions (return values from BPF)
// ---------------------------------------------------------------------------

/// Abort processing (drop + trace).
pub const XDP_ABORTED: u32 = 0;
/// Drop packet silently.
pub const XDP_DROP: u32 = 1;
/// Pass to normal network stack.
pub const XDP_PASS: u32 = 2;
/// Transmit back out the same interface.
pub const XDP_TX: u32 = 3;
/// Redirect to another interface/socket.
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bind_flags_no_overlap() {
        let flags = [XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY, XDP_USE_NEED_WAKEUP];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bind_flags_power_of_two() {
        assert!(XDP_SHARED_UMEM.is_power_of_two());
        assert!(XDP_COPY.is_power_of_two());
        assert!(XDP_ZEROCOPY.is_power_of_two());
        assert!(XDP_USE_NEED_WAKEUP.is_power_of_two());
    }

    #[test]
    fn test_ring_offsets_distinct() {
        let rings = [XDP_RX_RING, XDP_TX_RING, XDP_UMEM_FILL_RING, XDP_UMEM_COMPLETION_RING];
        for i in 0..rings.len() {
            for j in (i + 1)..rings.len() {
                assert_ne!(rings[i], rings[j]);
            }
        }
    }

    #[test]
    fn test_statistics_distinct() {
        let stats = [
            XDP_STATISTICS_RX_DROPPED, XDP_STATISTICS_RX_INVALID_DESCS,
            XDP_STATISTICS_TX_DROPPED, XDP_STATISTICS_TX_INVALID_DESCS,
            XDP_STATISTICS_RX_RING_FULL, XDP_STATISTICS_RX_FILL_RING_EMPTY,
            XDP_STATISTICS_TX_RING_EMPTY,
        ];
        for i in 0..stats.len() {
            for j in (i + 1)..stats.len() {
                assert_ne!(stats[i], stats[j]);
            }
        }
    }

    #[test]
    fn test_actions_distinct() {
        let actions = [XDP_ABORTED, XDP_DROP, XDP_PASS, XDP_TX, XDP_REDIRECT];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_xdp_aborted_is_zero() {
        assert_eq!(XDP_ABORTED, 0);
    }

    #[test]
    fn test_sockopt_distinct() {
        let opts = [
            XDP_MMAP_OFFSETS, XDP_RX_RING_SIZE, XDP_TX_RING_SIZE,
            XDP_UMEM_REG, XDP_UMEM_FILL_SIZE, XDP_UMEM_COMPLETION_SIZE,
            XDP_STATISTICS, XDP_OPTIONS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
