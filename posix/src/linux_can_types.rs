//! `<linux/can.h>` — CAN bus socket constants.
//!
//! Controller Area Network (CAN) is a serial bus for real-time
//! communication, primarily in automotive and industrial control.
//! Linux provides socket-based CAN access via AF_CAN with multiple
//! protocols: raw CAN frames, BCM (Broadcast Manager), and ISO-TP
//! for segmented messages.

// ---------------------------------------------------------------------------
// CAN protocol family constants
// ---------------------------------------------------------------------------

/// CAN raw protocol (send/receive raw frames).
pub const CAN_RAW: u32 = 1;
/// CAN BCM (Broadcast Manager, periodic send/filter).
pub const CAN_BCM: u32 = 2;
/// CAN ISO-TP (ISO 15765-2, segmented transfer).
pub const CAN_ISOTP: u32 = 6;
/// CAN J1939 (SAE J1939 transport protocol).
pub const CAN_J1939: u32 = 7;

// ---------------------------------------------------------------------------
// CAN frame flags (can_id field)
// ---------------------------------------------------------------------------

/// Extended frame format (29-bit ID).
pub const CAN_EFF_FLAG: u32 = 0x8000_0000;
/// Remote transmission request.
pub const CAN_RTR_FLAG: u32 = 0x4000_0000;
/// Error frame.
pub const CAN_ERR_FLAG: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// CAN ID masks
// ---------------------------------------------------------------------------

/// Standard frame ID mask (11 bits).
pub const CAN_SFF_MASK: u32 = 0x0000_07FF;
/// Extended frame ID mask (29 bits).
pub const CAN_EFF_MASK: u32 = 0x1FFF_FFFF;
/// Error mask.
pub const CAN_ERR_MASK: u32 = 0x1FFF_FFFF;

// ---------------------------------------------------------------------------
// CAN frame sizes
// ---------------------------------------------------------------------------

/// Classic CAN maximum data length.
pub const CAN_MAX_DLEN: u32 = 8;
/// CAN FD maximum data length.
pub const CANFD_MAX_DLEN: u32 = 64;
/// CAN XL maximum data length.
pub const CANXL_MAX_DLEN: u32 = 2048;

// ---------------------------------------------------------------------------
// CAN FD flags (canfd_frame.flags)
// ---------------------------------------------------------------------------

/// Bit rate switch (data phase at higher bitrate).
pub const CANFD_BRS: u8 = 0x01;
/// Error state indicator.
pub const CANFD_ESI: u8 = 0x02;
/// CAN FD frame.
pub const CANFD_FDF: u8 = 0x04;

// ---------------------------------------------------------------------------
// CAN raw socket options
// ---------------------------------------------------------------------------

/// Set CAN filter.
pub const CAN_RAW_FILTER: u32 = 1;
/// Set error mask.
pub const CAN_RAW_ERR_FILTER: u32 = 2;
/// Set loopback mode.
pub const CAN_RAW_LOOPBACK: u32 = 3;
/// Receive own messages.
pub const CAN_RAW_RECV_OWN_MSGS: u32 = 4;
/// Enable CAN FD frames.
pub const CAN_RAW_FD_FRAMES: u32 = 5;
/// Enable CAN XL frames.
pub const CAN_RAW_XL_FRAMES: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_frame_flags_no_overlap() {
        assert_eq!(CAN_EFF_FLAG & CAN_RTR_FLAG, 0);
        assert_eq!(CAN_EFF_FLAG & CAN_ERR_FLAG, 0);
        assert_eq!(CAN_RTR_FLAG & CAN_ERR_FLAG, 0);
    }

    #[test]
    fn test_dlen_ordering() {
        assert!(CAN_MAX_DLEN < CANFD_MAX_DLEN);
        assert!(CANFD_MAX_DLEN < CANXL_MAX_DLEN);
    }

    #[test]
    fn test_fd_flags_no_overlap() {
        let flags = [CANFD_BRS, CANFD_ESI, CANFD_FDF];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_raw_options_distinct() {
        let opts = [
            CAN_RAW_FILTER, CAN_RAW_ERR_FILTER, CAN_RAW_LOOPBACK,
            CAN_RAW_RECV_OWN_MSGS, CAN_RAW_FD_FRAMES, CAN_RAW_XL_FRAMES,
        ];
        for i in 0..opts.len() {
            for j in (i + 1)..opts.len() {
                assert_ne!(opts[i], opts[j]);
            }
        }
    }
}
