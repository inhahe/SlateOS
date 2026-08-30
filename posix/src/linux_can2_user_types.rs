//! `<linux/can/raw.h>` — CAN_RAW socket options.
//!
//! `CAN_RAW` is the lowest-level CAN protocol: userspace sees every
//! frame on the bound interface, can install filters, and can opt in
//! to error reporting and loopback semantics.

// ---------------------------------------------------------------------------
// Socket-option level
// ---------------------------------------------------------------------------

/// `SOL_CAN_BASE` — base for all CAN-protocol option levels.
pub const SOL_CAN_BASE: u32 = 100;

/// `SOL_CAN_RAW` — CAN_RAW option level (SOL_CAN_BASE + CAN_RAW).
pub const SOL_CAN_RAW: u32 = 101;

// ---------------------------------------------------------------------------
// `CAN_RAW_*` socket options
// ---------------------------------------------------------------------------

pub const CAN_RAW_FILTER: u32 = 1;
pub const CAN_RAW_ERR_FILTER: u32 = 2;
pub const CAN_RAW_LOOPBACK: u32 = 3;
pub const CAN_RAW_RECV_OWN_MSGS: u32 = 4;
pub const CAN_RAW_FD_FRAMES: u32 = 5;
pub const CAN_RAW_JOIN_FILTERS: u32 = 6;
pub const CAN_RAW_XL_FRAMES: u32 = 7;

// ---------------------------------------------------------------------------
// Default behaviour
// ---------------------------------------------------------------------------

/// Default: loopback is enabled (you see your own transmitted frames
/// on every other CAN_RAW socket bound to the same interface).
pub const CAN_RAW_LOOPBACK_DEFAULT: u32 = 1;

/// Default: do not deliver back to the sending socket.
pub const CAN_RAW_RECV_OWN_MSGS_DEFAULT: u32 = 0;

/// Maximum number of `struct can_filter` entries in a single setsockopt.
pub const CAN_RAW_FILTER_MAX: usize = 512;

// ---------------------------------------------------------------------------
// `struct can_filter` field offsets
// ---------------------------------------------------------------------------

pub const CAN_FILTER_OFF_ID: usize = 0;
pub const CAN_FILTER_OFF_MASK: usize = 4;
pub const CAN_FILTER_SIZE: usize = 8;

/// "Invert filter" flag bit in `can_id` of a filter entry.
pub const CAN_INV_FILTER: u32 = 0x2000_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sol_can_raw_offset_from_base() {
        // SOL_CAN_RAW = SOL_CAN_BASE + CAN_RAW(=1).
        assert_eq!(SOL_CAN_BASE, 100);
        assert_eq!(SOL_CAN_RAW, 101);
        assert_eq!(SOL_CAN_RAW - SOL_CAN_BASE, 1);
    }

    #[test]
    fn test_raw_socket_options_dense_1_to_7() {
        let o = [
            CAN_RAW_FILTER,
            CAN_RAW_ERR_FILTER,
            CAN_RAW_LOOPBACK,
            CAN_RAW_RECV_OWN_MSGS,
            CAN_RAW_FD_FRAMES,
            CAN_RAW_JOIN_FILTERS,
            CAN_RAW_XL_FRAMES,
        ];
        for (i, &v) in o.iter().enumerate() {
            assert_eq!(v as usize, i + 1);
        }
    }

    #[test]
    fn test_defaults_match_kernel_behavior() {
        // The kernel ships loopback ON, recv-own OFF.
        assert_eq!(CAN_RAW_LOOPBACK_DEFAULT, 1);
        assert_eq!(CAN_RAW_RECV_OWN_MSGS_DEFAULT, 0);
        assert_ne!(CAN_RAW_LOOPBACK_DEFAULT, CAN_RAW_RECV_OWN_MSGS_DEFAULT);
    }

    #[test]
    fn test_filter_layout_packed_two_u32s() {
        assert_eq!(CAN_FILTER_OFF_ID, 0);
        assert_eq!(CAN_FILTER_OFF_MASK, 4);
        assert_eq!(CAN_FILTER_SIZE, 8);
        assert_eq!(CAN_FILTER_OFF_MASK - CAN_FILTER_OFF_ID, 4);
        assert_eq!(CAN_FILTER_SIZE, 2 * 4);
    }

    #[test]
    fn test_invert_filter_bit_is_single_bit() {
        assert!(CAN_INV_FILTER.is_power_of_two());
        // Bit 29 — sits below CAN_RTR_FLAG (bit 30) and CAN_EFF_FLAG (bit 31).
        assert_eq!(CAN_INV_FILTER, 1 << 29);
    }

    #[test]
    fn test_filter_max_count() {
        assert_eq!(CAN_RAW_FILTER_MAX, 512);
        // 512 entries * 8 bytes = 4 KiB — one page of filter table.
        assert_eq!(CAN_RAW_FILTER_MAX * CAN_FILTER_SIZE, 4_096);
    }
}
