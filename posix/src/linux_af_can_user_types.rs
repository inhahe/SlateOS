//! `<linux/can.h>` — Controller Area Network sockets.
//!
//! `AF_CAN` is Linux's API for talking to automotive CAN buses (cars,
//! industrial control, hobby robotics with USB↔CAN dongles). Userspace
//! tools include `can-utils` (`cansend`, `candump`).

// ---------------------------------------------------------------------------
// Address family / protocol family
// ---------------------------------------------------------------------------

pub const AF_CAN: u32 = 29;
pub const PF_CAN: u32 = AF_CAN;
pub const SOL_CAN_BASE: u32 = 100;

// ---------------------------------------------------------------------------
// Protocol numbers (`socket(PF_CAN, SOCK_RAW, CAN_RAW)` etc.)
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
// Frame-ID flags (top bits of `can_id`)
// ---------------------------------------------------------------------------

/// 29-bit extended frame format (otherwise standard 11-bit).
pub const CAN_EFF_FLAG: u32 = 0x8000_0000;
/// Remote-transmission-request frame.
pub const CAN_RTR_FLAG: u32 = 0x4000_0000;
/// Error-message frame (`can_id` carries error class).
pub const CAN_ERR_FLAG: u32 = 0x2000_0000;

/// Mask off the flag bits to get just the identifier.
pub const CAN_SFF_MASK: u32 = 0x0000_07FF;
pub const CAN_EFF_MASK: u32 = 0x1FFF_FFFF;
pub const CAN_ERR_MASK: u32 = 0x1FFF_FFFF;

// ---------------------------------------------------------------------------
// Frame sizes
// ---------------------------------------------------------------------------

/// Classic CAN payload (1..8).
pub const CAN_MAX_DLEN: usize = 8;
/// CAN-FD payload (1..64).
pub const CANFD_MAX_DLEN: usize = 64;

/// On-wire `struct can_frame` is 16 bytes.
pub const CAN_MTU: usize = 16;
/// `struct canfd_frame` is 72 bytes.
pub const CANFD_MTU: usize = 72;

// ---------------------------------------------------------------------------
// `setsockopt(SOL_CAN_RAW, …)` options
// ---------------------------------------------------------------------------

pub const SOL_CAN_RAW: u32 = SOL_CAN_BASE + CAN_RAW;
pub const CAN_RAW_FILTER: u32 = 1;
pub const CAN_RAW_ERR_FILTER: u32 = 2;
pub const CAN_RAW_LOOPBACK: u32 = 3;
pub const CAN_RAW_RECV_OWN_MSGS: u32 = 4;
pub const CAN_RAW_FD_FRAMES: u32 = 5;
pub const CAN_RAW_JOIN_FILTERS: u32 = 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_af_can_is_29() {
        // PF_CAN/AF_CAN = 29 in include/linux/socket.h.
        assert_eq!(AF_CAN, 29);
        assert_eq!(PF_CAN, AF_CAN);
        assert_eq!(SOL_CAN_BASE, 100);
    }

    #[test]
    fn test_protocols_dense_1_to_7() {
        let p = [
            CAN_RAW, CAN_BCM, CAN_TP16, CAN_TP20, CAN_MCNET, CAN_ISOTP, CAN_J1939,
        ];
        for (i, &v) in p.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
        // NPROTO is the count.
        assert_eq!(CAN_NPROTO as usize, p.len() + 1);
    }

    #[test]
    fn test_flag_bits_top_three_disjoint() {
        let f = [CAN_EFF_FLAG, CAN_RTR_FLAG, CAN_ERR_FLAG];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // The three flags are mutually disjoint and live in bits 29..31.
        assert_eq!(CAN_EFF_FLAG | CAN_RTR_FLAG | CAN_ERR_FLAG, 0xE000_0000);
        // ID masks don't overlap the flags.
        assert_eq!(CAN_EFF_MASK & 0xE000_0000, 0);
        assert_eq!(CAN_SFF_MASK & 0xE000_0000, 0);
    }

    #[test]
    fn test_sff_and_eff_widths() {
        // SFF = 11 bits, EFF = 29 bits.
        assert_eq!(CAN_SFF_MASK.count_ones(), 11);
        assert_eq!(CAN_EFF_MASK.count_ones(), 29);
        assert_eq!(CAN_ERR_MASK, CAN_EFF_MASK);
    }

    #[test]
    fn test_payload_and_mtu_sizes() {
        // CAN-FD payload is 8x classic.
        assert_eq!(CAN_MAX_DLEN, 8);
        assert_eq!(CANFD_MAX_DLEN, 64);
        assert_eq!(CANFD_MAX_DLEN / CAN_MAX_DLEN, 8);
        // MTU = ID(4) + dlc(1) + flags(...) + payload — verify struct sizes.
        assert_eq!(CAN_MTU, 16);
        assert_eq!(CANFD_MTU, 72);
        assert!(CANFD_MTU > CAN_MTU);
    }

    #[test]
    fn test_raw_sockopts_dense_under_sol_can_raw() {
        // SOL_CAN_RAW = SOL_CAN_BASE + CAN_RAW.
        assert_eq!(SOL_CAN_RAW, 101);
        let r = [
            CAN_RAW_FILTER,
            CAN_RAW_ERR_FILTER,
            CAN_RAW_LOOPBACK,
            CAN_RAW_RECV_OWN_MSGS,
            CAN_RAW_FD_FRAMES,
            CAN_RAW_JOIN_FILTERS,
        ];
        for (i, &v) in r.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }
}
