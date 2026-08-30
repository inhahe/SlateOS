//! `<linux/can.h>` — CAN bus constants (extended).
//!
//! Extended CAN bus constants covering CAN frame flags,
//! error flags, CAN FD flags, and CAN raw socket options.

// ---------------------------------------------------------------------------
// CAN frame flags (in can_id field)
// ---------------------------------------------------------------------------

/// Extended Frame Format (29-bit ID).
pub const CAN_EFF_FLAG: u32 = 0x80000000;
/// Remote Transmission Request.
pub const CAN_RTR_FLAG: u32 = 0x40000000;
/// Error message frame.
pub const CAN_ERR_FLAG: u32 = 0x20000000;

// ---------------------------------------------------------------------------
// CAN ID masks
// ---------------------------------------------------------------------------

/// Standard Frame Format mask (11-bit).
pub const CAN_SFF_MASK: u32 = 0x000007FF;
/// Extended Frame Format mask (29-bit).
pub const CAN_EFF_MASK: u32 = 0x1FFFFFFF;
/// Error mask.
pub const CAN_ERR_MASK: u32 = 0x1FFFFFFF;
/// Inverted filter flag.
pub const CAN_INV_FILTER: u32 = 0x20000000;

// ---------------------------------------------------------------------------
// CAN frame sizes
// ---------------------------------------------------------------------------

/// Maximum CAN 2.0 data length.
pub const CAN_MAX_DLEN: u32 = 8;
/// Maximum CAN FD data length.
pub const CANFD_MAX_DLEN: u32 = 64;
/// CAN 2.0 frame size (struct can_frame).
pub const CAN_MTU: u32 = 16;
/// CAN FD frame size (struct canfd_frame).
pub const CANFD_MTU: u32 = 72;

// ---------------------------------------------------------------------------
// CAN FD flags (in canfd_frame.flags)
// ---------------------------------------------------------------------------

/// Bit rate switch (second bitrate for data phase).
pub const CANFD_BRS: u8 = 0x01;
/// Error state indicator.
pub const CANFD_ESI: u8 = 0x02;
/// CAN FD frame (not classical CAN).
pub const CANFD_FDF: u8 = 0x04;

// ---------------------------------------------------------------------------
// CAN error classes (in can_id when CAN_ERR_FLAG set)
// ---------------------------------------------------------------------------

/// TX timeout error.
pub const CAN_ERR_TX_TIMEOUT: u32 = 0x00000001;
/// Lost arbitration.
pub const CAN_ERR_LOSTARB: u32 = 0x00000002;
/// Controller error.
pub const CAN_ERR_CRTL: u32 = 0x00000004;
/// Protocol violation.
pub const CAN_ERR_PROT: u32 = 0x00000008;
/// Transceiver error.
pub const CAN_ERR_TRX: u32 = 0x00000010;
/// No ACK on transmission.
pub const CAN_ERR_ACK: u32 = 0x00000020;
/// Bus off.
pub const CAN_ERR_BUSOFF: u32 = 0x00000040;
/// Bus error.
pub const CAN_ERR_BUSERROR: u32 = 0x00000080;
/// Controller restarted.
pub const CAN_ERR_RESTARTED: u32 = 0x00000100;

// ---------------------------------------------------------------------------
// CAN raw socket options
// ---------------------------------------------------------------------------

/// Set CAN raw filter.
pub const CAN_RAW_FILTER: u32 = 1;
/// Set error filter.
pub const CAN_RAW_ERR_FILTER: u32 = 2;
/// Loopback mode.
pub const CAN_RAW_LOOPBACK: u32 = 3;
/// Receive own messages.
pub const CAN_RAW_RECV_OWN_MSGS: u32 = 4;
/// CAN FD support.
pub const CAN_RAW_FD_FRAMES: u32 = 5;
/// Join filters.
pub const CAN_RAW_JOIN_FILTERS: u32 = 6;
/// XL frames.
pub const CAN_RAW_XL_FRAMES: u32 = 7;

// ---------------------------------------------------------------------------
// CAN protocol family
// ---------------------------------------------------------------------------

/// CAN protocol family.
pub const PF_CAN: u32 = 29;
/// CAN address family.
pub const AF_CAN: u32 = PF_CAN;

// ---------------------------------------------------------------------------
// CAN protocols
// ---------------------------------------------------------------------------

/// Raw CAN protocol.
pub const CAN_RAW: u32 = 1;
/// Broadcast Manager.
pub const CAN_BCM: u32 = 2;
/// Transport Protocol (ISO 15765-2).
pub const CAN_TP16: u32 = 3;
/// Transport Protocol (ISO 15765-2).
pub const CAN_TP20: u32 = 4;
/// ISO TP.
pub const CAN_ISOTP: u32 = 6;
/// J1939 protocol.
pub const CAN_J1939: u32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_flags_powers_of_two() {
        // These use top bits, check they don't overlap
        let flags = [CAN_EFF_FLAG, CAN_RTR_FLAG, CAN_ERR_FLAG];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_sff_mask() {
        assert_eq!(CAN_SFF_MASK, 0x7FF);
    }

    #[test]
    fn test_eff_mask() {
        assert_eq!(CAN_EFF_MASK, 0x1FFFFFFF);
    }

    #[test]
    fn test_frame_sizes() {
        assert_eq!(CAN_MAX_DLEN, 8);
        assert_eq!(CANFD_MAX_DLEN, 64);
        assert!(CANFD_MTU > CAN_MTU);
    }

    #[test]
    fn test_fd_flags_powers_of_two() {
        let flags = [CANFD_BRS, CANFD_ESI, CANFD_FDF];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_fd_flags_no_overlap() {
        let flags = [CANFD_BRS, CANFD_ESI, CANFD_FDF];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_error_classes_no_overlap() {
        let errs = [
            CAN_ERR_TX_TIMEOUT,
            CAN_ERR_LOSTARB,
            CAN_ERR_CRTL,
            CAN_ERR_PROT,
            CAN_ERR_TRX,
            CAN_ERR_ACK,
            CAN_ERR_BUSOFF,
            CAN_ERR_BUSERROR,
            CAN_ERR_RESTARTED,
        ];
        for i in 0..errs.len() {
            for j in (i + 1)..errs.len() {
                assert_eq!(errs[i] & errs[j], 0);
            }
        }
    }

    #[test]
    fn test_raw_options_distinct() {
        let opts = [
            CAN_RAW_FILTER,
            CAN_RAW_ERR_FILTER,
            CAN_RAW_LOOPBACK,
            CAN_RAW_RECV_OWN_MSGS,
            CAN_RAW_FD_FRAMES,
            CAN_RAW_JOIN_FILTERS,
            CAN_RAW_XL_FRAMES,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }

    #[test]
    fn test_af_can() {
        assert_eq!(AF_CAN, PF_CAN);
        assert_eq!(AF_CAN, 29);
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [CAN_RAW, CAN_BCM, CAN_TP16, CAN_TP20, CAN_ISOTP, CAN_J1939];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }
}
