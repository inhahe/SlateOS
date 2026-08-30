//! `<linux/bpf.h>` — XDP (eXpress Data Path) action and metadata constants.
//!
//! XDP allows high-performance packet processing at the network
//! driver level using BPF programs.  These constants define XDP
//! actions, attachment modes, and metadata flags.

// ---------------------------------------------------------------------------
// XDP actions (return values from XDP programs)
// ---------------------------------------------------------------------------

/// Abort (error, drop with trace).
pub const XDP_ABORTED: u32 = 0;
/// Drop the packet silently.
pub const XDP_DROP: u32 = 1;
/// Pass to the normal network stack.
pub const XDP_PASS: u32 = 2;
/// Forward/transmit out the same interface.
pub const XDP_TX: u32 = 3;
/// Redirect to another interface/CPU/socket.
pub const XDP_REDIRECT: u32 = 4;

// ---------------------------------------------------------------------------
// XDP attachment flags
// ---------------------------------------------------------------------------

/// Default (driver mode if available, else generic).
pub const XDP_FLAGS_UPDATE_IF_NOEXIST: u32 = 1 << 0;
/// Use SKB (generic) mode.
pub const XDP_FLAGS_SKB_MODE: u32 = 1 << 1;
/// Use driver (native) mode.
pub const XDP_FLAGS_DRV_MODE: u32 = 1 << 2;
/// Use hardware offload mode.
pub const XDP_FLAGS_HW_MODE: u32 = 1 << 3;
/// Replace existing program.
pub const XDP_FLAGS_REPLACE: u32 = 1 << 4;
/// Mask of all mode flags.
pub const XDP_FLAGS_MODES: u32 = XDP_FLAGS_SKB_MODE | XDP_FLAGS_DRV_MODE | XDP_FLAGS_HW_MODE;

// ---------------------------------------------------------------------------
// XDP metadata
// ---------------------------------------------------------------------------

/// Maximum XDP metadata size (bytes).
pub const XDP_METADATA_MAX: u32 = 256;
/// XDP frame headroom (bytes, default).
pub const XDP_PACKET_HEADROOM: u32 = 256;

// ---------------------------------------------------------------------------
// XDP socket (AF_XDP) constants
// ---------------------------------------------------------------------------

/// UMEM frame size (default, 4 KiB).
pub const XDP_UMEM_FRAME_SIZE_DEFAULT: u32 = 4096;
/// Fill ring descriptor size.
pub const XDP_FILL_RING: u32 = 5;
/// Completion ring descriptor size.
pub const XDP_COMPLETION_RING: u32 = 6;
/// RX ring.
pub const XDP_RX_RING: u32 = 1;
/// TX ring.
pub const XDP_TX_RING: u32 = 2;

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
    fn test_aborted_is_zero() {
        assert_eq!(XDP_ABORTED, 0);
    }

    #[test]
    fn test_attachment_flags_powers_of_two() {
        let flags = [
            XDP_FLAGS_UPDATE_IF_NOEXIST,
            XDP_FLAGS_SKB_MODE,
            XDP_FLAGS_DRV_MODE,
            XDP_FLAGS_HW_MODE,
            XDP_FLAGS_REPLACE,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_modes_mask() {
        assert_eq!(
            XDP_FLAGS_MODES,
            XDP_FLAGS_SKB_MODE | XDP_FLAGS_DRV_MODE | XDP_FLAGS_HW_MODE
        );
    }

    #[test]
    fn test_headroom() {
        assert_eq!(XDP_PACKET_HEADROOM, 256);
    }

    #[test]
    fn test_umem_frame_size() {
        assert_eq!(XDP_UMEM_FRAME_SIZE_DEFAULT, 4096);
    }

    #[test]
    fn test_rings_distinct() {
        let rings = [XDP_RX_RING, XDP_TX_RING, XDP_FILL_RING, XDP_COMPLETION_RING];
        for i in 0..rings.len() {
            for j in (i + 1)..rings.len() {
                assert_ne!(rings[i], rings[j]);
            }
        }
    }
}
