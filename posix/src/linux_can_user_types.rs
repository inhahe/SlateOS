//! `<linux/can.h>` — Controller Area Network base socket family.
//!
//! CAN sockets (`AF_CAN`) deliver CAN-bus frames between userspace and
//! kernel CAN drivers. Each frame is an 11-bit (standard) or 29-bit
//! (extended) ID plus 0..8 (CAN 2.0) or 0..64 (CAN-FD) data bytes.

// ---------------------------------------------------------------------------
// Address family
// ---------------------------------------------------------------------------

/// `AF_CAN` — Linux address-family number for CAN.
pub const AF_CAN: u32 = 29;
/// Alias.
pub const PF_CAN: u32 = AF_CAN;

// ---------------------------------------------------------------------------
// Protocol numbers (`enum`)
// ---------------------------------------------------------------------------

pub const CAN_RAW: u32 = 1;
pub const CAN_BCM: u32 = 2;
pub const CAN_TP16: u32 = 3;
pub const CAN_TP20: u32 = 4;
pub const CAN_MCNET: u32 = 5;
pub const CAN_ISOTP: u32 = 6;
pub const CAN_J1939: u32 = 7;
pub const CAN_NPROTO: u32 = 8;

// ---------------------------------------------------------------------------
// Frame ID flag bits (set in `can_id`)
// ---------------------------------------------------------------------------

/// Extended frame format (29-bit ID).
pub const CAN_EFF_FLAG: u32 = 0x8000_0000;

/// Remote-transmission-request frame.
pub const CAN_RTR_FLAG: u32 = 0x4000_0000;

/// Error message frame.
pub const CAN_ERR_FLAG: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// Frame ID masks
// ---------------------------------------------------------------------------

/// Standard-frame-format mask (11 bits).
pub const CAN_SFF_MASK: u32 = 0x0000_07FF;

/// Extended-frame-format mask (29 bits).
pub const CAN_EFF_MASK: u32 = 0x1FFF_FFFF;

/// Error-frame mask = EFF mask.
pub const CAN_ERR_MASK: u32 = 0x1FFF_FFFF;

// ---------------------------------------------------------------------------
// Frame sizes
// ---------------------------------------------------------------------------

/// CAN 2.0 maximum data length.
pub const CAN_MAX_DLEN: usize = 8;

/// CAN-FD maximum data length.
pub const CANFD_MAX_DLEN: usize = 64;

/// Standard CAN frame: 8-byte header + 8-byte payload.
pub const CAN_MTU: usize = 16;

/// CAN-FD frame: 8-byte header + 64-byte payload.
pub const CANFD_MTU: usize = 72;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_address_family() {
        assert_eq!(AF_CAN, 29);
        assert_eq!(PF_CAN, AF_CAN);
    }

    #[test]
    fn test_protocol_numbers_dense_1_to_7() {
        let p = [
            CAN_RAW, CAN_BCM, CAN_TP16, CAN_TP20, CAN_MCNET, CAN_ISOTP, CAN_J1939,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // NPROTO is one past the last protocol.
        assert_eq!(CAN_NPROTO, CAN_J1939 + 1);
    }

    #[test]
    fn test_id_flag_bits_disjoint_single_bits() {
        let f = [CAN_EFF_FLAG, CAN_RTR_FLAG, CAN_ERR_FLAG];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
        // Flags live in the top three bits.
        assert!(CAN_EFF_FLAG >= 1 << 29);
    }

    #[test]
    fn test_id_masks_fit_their_width() {
        // SFF: 11 bits.
        assert_eq!(CAN_SFF_MASK, (1u32 << 11) - 1);
        // EFF: 29 bits.
        assert_eq!(CAN_EFF_MASK, (1u32 << 29) - 1);
        // ERR uses the same width as EFF.
        assert_eq!(CAN_ERR_MASK, CAN_EFF_MASK);
        // SFF fits inside EFF.
        assert_eq!(CAN_SFF_MASK & CAN_EFF_MASK, CAN_SFF_MASK);
    }

    #[test]
    fn test_id_flag_bits_do_not_overlap_value_mask() {
        // Flag bits sit above the EFF mask.
        for f in [CAN_EFF_FLAG, CAN_RTR_FLAG, CAN_ERR_FLAG] {
            assert_eq!(f & CAN_EFF_MASK, 0);
        }
    }

    #[test]
    fn test_frame_sizes_and_mtu() {
        assert_eq!(CAN_MAX_DLEN, 8);
        assert_eq!(CANFD_MAX_DLEN, 64);
        // CAN-FD payload is 8x bigger than classical CAN.
        assert_eq!(CANFD_MAX_DLEN / CAN_MAX_DLEN, 8);
        // MTU = header (8 bytes for can_id+len+pad) + max payload.
        assert_eq!(CAN_MTU, 8 + CAN_MAX_DLEN);
        assert_eq!(CANFD_MTU, 8 + CANFD_MAX_DLEN);
    }
}
