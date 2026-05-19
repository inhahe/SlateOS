//! `<linux/if_xdp.h>` — Additional AF_XDP socket constants.
//!
//! Supplementary AF_XDP constants covering socket options,
//! umem flags, and ring offsets.

// ---------------------------------------------------------------------------
// AF_XDP socket options (XDP_*)
// ---------------------------------------------------------------------------

/// Receive ring offset.
pub const XDP_RX_RING: u32 = 1;
/// Transmit ring offset.
pub const XDP_TX_RING: u32 = 2;
/// UMEM registration.
pub const XDP_UMEM_REG: u32 = 3;
/// UMEM fill ring.
pub const XDP_UMEM_FILL_RING: u32 = 4;
/// UMEM completion ring.
pub const XDP_UMEM_COMPLETION_RING: u32 = 5;
/// Statistics.
pub const XDP_STATISTICS: u32 = 7;
/// Socket options.
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// AF_XDP bind flags
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
// AF_XDP ring flags
// ---------------------------------------------------------------------------

/// Need wakeup flag for rings.
pub const XDP_RING_NEED_WAKEUP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// AF_XDP UMEM flags
// ---------------------------------------------------------------------------

/// UMEM uses unaligned chunks.
pub const XDP_UMEM_UNALIGNED_CHUNK_FLAG: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// AF_XDP ring offset constants
// ---------------------------------------------------------------------------

/// Producer offset in ring struct.
pub const XDP_RING_PRODUCER_OFFSET: u32 = 0;
/// Consumer offset in ring struct.
pub const XDP_RING_CONSUMER_OFFSET: u32 = 4;
/// Flags offset in ring struct.
pub const XDP_RING_FLAGS_OFFSET: u32 = 8;
/// Descriptors offset in ring struct.
pub const XDP_RING_DESC_OFFSET: u32 = 16;

/// XDP descriptor size.
pub const XDP_DESC_SIZE: u32 = 16;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_socket_opts_distinct() {
        let opts = [
            XDP_RX_RING, XDP_TX_RING, XDP_UMEM_REG,
            XDP_UMEM_FILL_RING, XDP_UMEM_COMPLETION_RING,
            XDP_STATISTICS, XDP_OPTIONS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_bind_flags_power_of_two() {
        let flags = [
            XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY,
            XDP_USE_NEED_WAKEUP, XDP_USE_SG,
        ];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:04x} not power of two", f);
        }
    }

    #[test]
    fn test_bind_flags_no_overlap() {
        let flags = [
            XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY,
            XDP_USE_NEED_WAKEUP, XDP_USE_SG,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_ring_offsets_increasing() {
        assert!(XDP_RING_PRODUCER_OFFSET < XDP_RING_CONSUMER_OFFSET);
        assert!(XDP_RING_CONSUMER_OFFSET < XDP_RING_FLAGS_OFFSET);
        assert!(XDP_RING_FLAGS_OFFSET < XDP_RING_DESC_OFFSET);
    }

    #[test]
    fn test_desc_size() {
        assert_eq!(XDP_DESC_SIZE, 16);
    }

    #[test]
    fn test_umem_flag() {
        assert_eq!(XDP_UMEM_UNALIGNED_CHUNK_FLAG, 1);
    }
}
