//! `<linux/can.h>` — Controller Area Network (CAN bus) protocol.
//!
//! Provides structures and constants for CAN socket communication.

// ---------------------------------------------------------------------------
// CAN protocol family
// ---------------------------------------------------------------------------

/// CAN address family.
pub const AF_CAN: i32 = 29;
/// CAN protocol family (alias).
pub const PF_CAN: i32 = AF_CAN;

// ---------------------------------------------------------------------------
// CAN protocols
// ---------------------------------------------------------------------------

/// Raw CAN protocol.
pub const CAN_RAW: i32 = 1;
/// CAN BCM (broadcast manager) protocol.
pub const CAN_BCM: i32 = 2;
/// CAN transport protocol (ISO-TP, ISO 15765-2).
pub const CAN_ISOTP: i32 = 6;
/// CAN J1939 protocol.
pub const CAN_J1939: i32 = 7;

// ---------------------------------------------------------------------------
// CAN socket options
// ---------------------------------------------------------------------------

/// Set/get raw CAN filter.
pub const CAN_RAW_FILTER: i32 = 1;
/// Set/get error mask.
pub const CAN_RAW_ERR_FILTER: i32 = 2;
/// Enable/disable loopback.
pub const CAN_RAW_LOOPBACK: i32 = 3;
/// Receive own messages.
pub const CAN_RAW_RECV_OWN_MSGS: i32 = 4;
/// Enable/disable CAN FD.
pub const CAN_RAW_FD_FRAMES: i32 = 5;
/// Join filters.
pub const CAN_RAW_JOIN_FILTERS: i32 = 6;

// ---------------------------------------------------------------------------
// CAN ID flags
// ---------------------------------------------------------------------------

/// Extended frame format (29-bit).
pub const CAN_EFF_FLAG: u32 = 0x8000_0000;
/// Remote transmission request.
pub const CAN_RTR_FLAG: u32 = 0x4000_0000;
/// Error frame.
pub const CAN_ERR_FLAG: u32 = 0x2000_0000;

/// Standard frame ID mask (11-bit).
pub const CAN_SFF_MASK: u32 = 0x0000_07FF;
/// Extended frame ID mask (29-bit).
pub const CAN_EFF_MASK: u32 = 0x1FFF_FFFF;
/// Error class mask.
pub const CAN_ERR_MASK: u32 = 0x1FFF_FFFF;

/// Maximum CAN data length (classic CAN).
pub const CAN_MAX_DLEN: usize = 8;
/// Maximum CAN FD data length.
pub const CANFD_MAX_DLEN: usize = 64;

// ---------------------------------------------------------------------------
// CAN frame
// ---------------------------------------------------------------------------

/// Classic CAN frame (compatible with `struct can_frame`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CanFrame {
    /// CAN identifier + EFF/RTR/ERR flags.
    pub can_id: u32,
    /// Data length code [0..8].
    pub can_dlc: u8,
    /// Padding.
    _pad: u8,
    /// Reserved.
    _res0: u8,
    /// Length (redundant with dlc in kernel >= 5.11).
    _len8_dlc: u8,
    /// Data payload.
    pub data: [u8; CAN_MAX_DLEN],
}

/// CAN FD (Flexible Data-rate) frame.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CanFdFrame {
    /// CAN identifier + EFF/RTR/ERR flags.
    pub can_id: u32,
    /// Data length [0..64].
    pub len: u8,
    /// CAN FD flags.
    pub flags: u8,
    /// Reserved.
    _res0: u8,
    /// Reserved.
    _res1: u8,
    /// Data payload.
    pub data: [u8; CANFD_MAX_DLEN],
}

/// CAN filter for raw sockets.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct CanFilter {
    /// CAN identifier to match.
    pub can_id: u32,
    /// CAN mask (which bits to check).
    pub can_mask: u32,
}

/// Socket address for CAN.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct SockaddrCan {
    /// Address family (AF_CAN).
    pub can_family: u16,
    /// CAN interface index.
    pub can_ifindex: i32,
    /// Protocol-specific address (union in Linux, we use tp for most cases).
    pub tp_rx_id: u32,
    /// Transport protocol TX ID.
    pub tp_tx_id: u32,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_frame_size() {
        // 4 + 1 + 1 + 1 + 1 + 8 = 16 bytes.
        assert_eq!(core::mem::size_of::<CanFrame>(), 16);
    }

    #[test]
    fn test_canfd_frame_size() {
        // 4 + 1 + 1 + 1 + 1 + 64 = 72 bytes.
        assert_eq!(core::mem::size_of::<CanFdFrame>(), 72);
    }

    #[test]
    fn test_can_filter_size() {
        assert_eq!(core::mem::size_of::<CanFilter>(), 8);
    }

    #[test]
    fn test_id_flags_are_bits() {
        assert_eq!(CAN_EFF_FLAG & CAN_RTR_FLAG, 0);
        assert_eq!(CAN_RTR_FLAG & CAN_ERR_FLAG, 0);
        assert_eq!(CAN_EFF_FLAG & CAN_ERR_FLAG, 0);
    }

    #[test]
    fn test_id_masks() {
        // Standard ID fits in 11 bits.
        assert_eq!(CAN_SFF_MASK, 0x7FF);
        // Extended ID fits in 29 bits.
        assert_eq!(CAN_EFF_MASK, 0x1FFF_FFFF);
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
    fn test_af_can() {
        assert_eq!(AF_CAN, 29);
        assert_eq!(PF_CAN, AF_CAN);
    }

    #[test]
    fn test_max_data_lengths() {
        assert_eq!(CAN_MAX_DLEN, 8);
        assert_eq!(CANFD_MAX_DLEN, 64);
    }
}
