//! `<linux/can.h>` — Controller Area Network (CAN) constants.
//!
//! CAN is a vehicle bus standard used in automotive and industrial
//! applications. These constants define CAN frame formats, socket
//! options, protocol identifiers, and error flags.

// ---------------------------------------------------------------------------
// CAN frame format constants
// ---------------------------------------------------------------------------

/// Standard CAN ID mask (11-bit).
pub const CAN_SFF_MASK: u32 = 0x7FF;
/// Extended CAN ID mask (29-bit).
pub const CAN_EFF_MASK: u32 = 0x1FFF_FFFF;
/// Error frame mask.
pub const CAN_ERR_MASK: u32 = 0x1FFF_FFFF;

/// Extended frame format flag.
pub const CAN_EFF_FLAG: u32 = 0x8000_0000;
/// Remote transmission request flag.
pub const CAN_RTR_FLAG: u32 = 0x4000_0000;
/// Error message frame flag.
pub const CAN_ERR_FLAG: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// CAN data length limits
// ---------------------------------------------------------------------------

/// Classic CAN maximum data length.
pub const CAN_MAX_DLC: u8 = 8;
/// Classic CAN maximum data length (same as DLC).
pub const CAN_MAX_DLEN: u8 = 8;
/// CAN FD maximum data length.
pub const CANFD_MAX_DLC: u8 = 15;
/// CAN FD maximum data length.
pub const CANFD_MAX_DLEN: u8 = 64;

// ---------------------------------------------------------------------------
// CAN FD flags (in canfd_frame.flags)
// ---------------------------------------------------------------------------

/// Bit rate switch (second bit rate for data phase).
pub const CANFD_BRS: u8 = 0x01;
/// Error state indicator.
pub const CANFD_ESI: u8 = 0x02;
/// CAN FD frame (vs classic CAN).
pub const CANFD_FDF: u8 = 0x04;

// ---------------------------------------------------------------------------
// CAN protocol family constants
// ---------------------------------------------------------------------------

/// CAN raw socket protocol.
pub const CAN_RAW: u32 = 1;
/// CAN broadcast manager protocol.
pub const CAN_BCM: u32 = 2;
/// CAN transport protocol (ISO 15765-2).
pub const CAN_ISOTP: u32 = 6;
/// CAN J1939 protocol (SAE J1939).
pub const CAN_J1939: u32 = 7;

// ---------------------------------------------------------------------------
// CAN socket options
// ---------------------------------------------------------------------------

/// Set CAN raw filter.
pub const CAN_RAW_FILTER: u32 = 1;
/// Set CAN raw error filter.
pub const CAN_RAW_ERR_FILTER: u32 = 2;
/// Enable CAN raw loopback.
pub const CAN_RAW_LOOPBACK: u32 = 3;
/// Receive own messages.
pub const CAN_RAW_RECV_OWN_MSGS: u32 = 4;
/// Enable CAN FD frames.
pub const CAN_RAW_FD_FRAMES: u32 = 5;
/// Join CAN filters (logical AND).
pub const CAN_RAW_JOIN_FILTERS: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_flags_no_overlap() {
        let flags = [CAN_EFF_FLAG, CAN_RTR_FLAG, CAN_ERR_FLAG];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_id_flags_power_of_two() {
        assert!(CAN_EFF_FLAG.is_power_of_two());
        assert!(CAN_RTR_FLAG.is_power_of_two());
        assert!(CAN_ERR_FLAG.is_power_of_two());
    }

    #[test]
    fn test_sff_mask() {
        assert_eq!(CAN_SFF_MASK, 0x7FF);
    }

    #[test]
    fn test_eff_mask() {
        assert_eq!(CAN_EFF_MASK, 0x1FFF_FFFF);
    }

    #[test]
    fn test_classic_can_dlen() {
        assert_eq!(CAN_MAX_DLEN, 8);
    }

    #[test]
    fn test_canfd_dlen() {
        assert_eq!(CANFD_MAX_DLEN, 64);
    }

    #[test]
    fn test_canfd_flags_no_overlap() {
        let flags = [CANFD_BRS, CANFD_ESI, CANFD_FDF];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_protocols_distinct() {
        let protos = [CAN_RAW, CAN_BCM, CAN_ISOTP, CAN_J1939];
        for i in 0..protos.len() {
            for j in (i + 1)..protos.len() {
                assert_ne!(protos[i], protos[j]);
            }
        }
    }

    #[test]
    fn test_sockopt_distinct() {
        let opts = [
            CAN_RAW_FILTER, CAN_RAW_ERR_FILTER,
            CAN_RAW_LOOPBACK, CAN_RAW_RECV_OWN_MSGS,
            CAN_RAW_FD_FRAMES, CAN_RAW_JOIN_FILTERS,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
