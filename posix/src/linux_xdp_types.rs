//! `<linux/if_xdp.h>` — XDP (eXpress Data Path) socket constants.
//!
//! XDP enables high-performance packet processing by running eBPF
//! programs at the earliest point in the network stack (driver level).
//! AF_XDP sockets (XSK) provide a zero-copy path from the NIC to
//! userspace via shared UMEM rings. Used by high-frequency trading,
//! DDoS mitigation, and custom protocol stacks.

// ---------------------------------------------------------------------------
// XDP actions (return values from XDP programs)
// ---------------------------------------------------------------------------

/// Drop the packet.
pub const XDP_ABORTED: u32 = 0;
/// Drop the packet (explicit).
pub const XDP_DROP: u32 = 1;
/// Pass to normal network stack.
pub const XDP_PASS: u32 = 2;
/// Forward to another interface.
pub const XDP_TX: u32 = 3;
/// Redirect (to AF_XDP socket, another device, or CPU).
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// XDP attach flags
// ---------------------------------------------------------------------------

/// Attach in SKB (generic) mode.
pub const XDP_FLAGS_SKB_MODE: u32 = 1 << 1;
/// Attach in driver (native) mode.
pub const XDP_FLAGS_DRV_MODE: u32 = 1 << 2;
/// Attach in hardware offload mode.
pub const XDP_FLAGS_HW_MODE: u32 = 1 << 3;
/// Replace existing program.
pub const XDP_FLAGS_REPLACE: u32 = 1 << 4;
/// Attach mode mask.
pub const XDP_FLAGS_MODES: u32 = XDP_FLAGS_SKB_MODE | XDP_FLAGS_DRV_MODE | XDP_FLAGS_HW_MODE;

// ---------------------------------------------------------------------------
// AF_XDP socket options
// ---------------------------------------------------------------------------

/// Register UMEM.
pub const XDP_UMEM_REG: u32 = 3;
/// Fill ring setup.
pub const XDP_UMEM_FILL_RING: u32 = 5;
/// Completion ring setup.
pub const XDP_UMEM_COMPLETION_RING: u32 = 6;
/// RX ring setup.
pub const XDP_RX_RING: u32 = 1;
/// TX ring setup.
pub const XDP_TX_RING: u32 = 2;
/// MMAP offsets query.
pub const XDP_MMAP_OFFSETS: u32 = 4;
/// Get/set statistics.
pub const XDP_STATISTICS: u32 = 7;
/// Set XDP options.
pub const XDP_OPTIONS: u32 = 8;

// ---------------------------------------------------------------------------
// XDP bind flags
// ---------------------------------------------------------------------------

/// Shared UMEM between sockets.
pub const XDP_SHARED_UMEM: u16 = 1 << 0;
/// Copy mode (no zero-copy).
pub const XDP_COPY: u16 = 1 << 1;
/// Zero-copy mode.
pub const XDP_ZEROCOPY: u16 = 1 << 2;
/// Use need wakeup flag.
pub const XDP_USE_NEED_WAKEUP: u16 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_attach_flags_no_overlap() {
        let flags = [
            XDP_FLAGS_SKB_MODE, XDP_FLAGS_DRV_MODE,
            XDP_FLAGS_HW_MODE, XDP_FLAGS_REPLACE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_bind_flags_no_overlap() {
        let flags = [XDP_SHARED_UMEM, XDP_COPY, XDP_ZEROCOPY, XDP_USE_NEED_WAKEUP];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_socket_options_distinct() {
        let opts = [
            XDP_RX_RING, XDP_TX_RING, XDP_UMEM_REG,
            XDP_MMAP_OFFSETS, XDP_UMEM_FILL_RING,
            XDP_UMEM_COMPLETION_RING, XDP_STATISTICS, XDP_OPTIONS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
