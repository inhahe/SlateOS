//! `<linux/can/bcm.h>` — CAN Broadcast Manager.
//!
//! BCM lets userspace ask the kernel to transmit repeated CAN frames
//! on a timer, or to watch for incoming frames and only deliver them
//! to userspace when their content changes. Both are essential for
//! automotive applications that would otherwise hammer userspace at
//! 1 kHz per CAN signal.

// ---------------------------------------------------------------------------
// BCM opcodes (`enum`)
// ---------------------------------------------------------------------------

pub const TX_SETUP: u32 = 1;
pub const TX_DELETE: u32 = 2;
pub const TX_READ: u32 = 3;
pub const TX_SEND: u32 = 4;
pub const RX_SETUP: u32 = 5;
pub const RX_DELETE: u32 = 6;
pub const RX_READ: u32 = 7;
pub const TX_STATUS: u32 = 8;
pub const TX_EXPIRED: u32 = 9;
pub const RX_STATUS: u32 = 10;
pub const RX_TIMEOUT: u32 = 11;
pub const RX_CHANGED: u32 = 12;

// ---------------------------------------------------------------------------
// BCM flag bits
// ---------------------------------------------------------------------------

pub const SETTIMER: u32 = 0x0001;
pub const STARTTIMER: u32 = 0x0002;
pub const TX_COUNTEVT: u32 = 0x0004;
pub const TX_ANNOUNCE: u32 = 0x0008;
pub const TX_CP_CAN_ID: u32 = 0x0010;
pub const RX_FILTER_ID: u32 = 0x0020;
pub const RX_CHECK_DLC: u32 = 0x0040;
pub const RX_NO_AUTOTIMER: u32 = 0x0080;
pub const RX_ANNOUNCE_RESUME: u32 = 0x0100;
pub const TX_RESET_MULTI_IDX: u32 = 0x0200;
pub const RX_RTR_FRAME: u32 = 0x0400;
pub const CAN_FD_FRAME: u32 = 0x0800;

// ---------------------------------------------------------------------------
// Sizes / limits
// ---------------------------------------------------------------------------

/// Maximum number of multiplex (multi-frame) entries per BCM op.
pub const MAX_NFRAMES: u32 = 256;

/// `struct bcm_msg_head` size on 64-bit (opcode + flags + count + intervals + ID + nframes).
pub const BCM_MSG_HEAD_SIZE: usize = 56;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bcm_opcodes_dense_1_to_12() {
        let o = [
            TX_SETUP, TX_DELETE, TX_READ, TX_SEND, RX_SETUP, RX_DELETE, RX_READ,
            TX_STATUS, TX_EXPIRED, RX_STATUS, RX_TIMEOUT, RX_CHANGED,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_flag_bits_are_single_bits_and_disjoint() {
        let f = [
            SETTIMER, STARTTIMER, TX_COUNTEVT, TX_ANNOUNCE, TX_CP_CAN_ID,
            RX_FILTER_ID, RX_CHECK_DLC, RX_NO_AUTOTIMER, RX_ANNOUNCE_RESUME,
            TX_RESET_MULTI_IDX, RX_RTR_FRAME, CAN_FD_FRAME,
        ];
        for &v in &f {
            assert!(v.is_power_of_two());
        }
        for (i, &a) in f.iter().enumerate() {
            for &b in &f[i + 1..] {
                assert_eq!(a & b, 0);
            }
        }
    }

    #[test]
    fn test_flag_bits_fit_in_lower_12_bits() {
        let all = SETTIMER | STARTTIMER | TX_COUNTEVT | TX_ANNOUNCE | TX_CP_CAN_ID
            | RX_FILTER_ID | RX_CHECK_DLC | RX_NO_AUTOTIMER | RX_ANNOUNCE_RESUME
            | TX_RESET_MULTI_IDX | RX_RTR_FRAME | CAN_FD_FRAME;
        // All 12 single-bit flags pack into the low 12 bits.
        assert_eq!(all, (1u32 << 12) - 1);
    }

    #[test]
    fn test_max_nframes_byte_boundary() {
        assert_eq!(MAX_NFRAMES, 256);
        // 256 is the boundary of a u8 multiplex index.
        assert_eq!(MAX_NFRAMES, 1 << 8);
    }

    #[test]
    fn test_msg_head_size() {
        // The userspace `struct bcm_msg_head` totals 56 bytes:
        //   opcode(4) + flags(4) + count(4) + ival1(16) + ival2(16) +
        //   can_id(4) + nframes(4) + alignment pad.
        assert_eq!(BCM_MSG_HEAD_SIZE, 56);
        // Power-of-two-friendly alignment leftover.
        assert_eq!(BCM_MSG_HEAD_SIZE % 8, 0);
    }
}
