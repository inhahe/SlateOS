//! `<linux/can/netlink.h>` — CAN device link configuration via rtnetlink.
//!
//! CAN controllers are configured through rtnetlink's IFLA_CAN_* link
//! attributes: bit-timing, control-mode flags (loopback, triple-sampling,
//! BERR-reporting), and operational state.

// ---------------------------------------------------------------------------
// CAN operational state (`enum can_state`)
// ---------------------------------------------------------------------------

pub const CAN_STATE_ERROR_ACTIVE: u32 = 0;
pub const CAN_STATE_ERROR_WARNING: u32 = 1;
pub const CAN_STATE_ERROR_PASSIVE: u32 = 2;
pub const CAN_STATE_BUS_OFF: u32 = 3;
pub const CAN_STATE_STOPPED: u32 = 4;
pub const CAN_STATE_SLEEPING: u32 = 5;
pub const CAN_STATE_MAX: u32 = 6;

// ---------------------------------------------------------------------------
// `IFLA_CAN_*` netlink attribute IDs
// ---------------------------------------------------------------------------

pub const IFLA_CAN_UNSPEC: u32 = 0;
pub const IFLA_CAN_BITTIMING: u32 = 1;
pub const IFLA_CAN_BITTIMING_CONST: u32 = 2;
pub const IFLA_CAN_CLOCK: u32 = 3;
pub const IFLA_CAN_STATE: u32 = 4;
pub const IFLA_CAN_CTRLMODE: u32 = 5;
pub const IFLA_CAN_RESTART_MS: u32 = 6;
pub const IFLA_CAN_RESTART: u32 = 7;
pub const IFLA_CAN_BERR_COUNTER: u32 = 8;
pub const IFLA_CAN_DATA_BITTIMING: u32 = 9;
pub const IFLA_CAN_DATA_BITTIMING_CONST: u32 = 10;
pub const IFLA_CAN_TERMINATION: u32 = 11;

// ---------------------------------------------------------------------------
// Control-mode flag bits (`CAN_CTRLMODE_*`)
// ---------------------------------------------------------------------------

pub const CAN_CTRLMODE_LOOPBACK: u32 = 1 << 0;
pub const CAN_CTRLMODE_LISTENONLY: u32 = 1 << 1;
pub const CAN_CTRLMODE_3_SAMPLES: u32 = 1 << 2;
pub const CAN_CTRLMODE_ONE_SHOT: u32 = 1 << 3;
pub const CAN_CTRLMODE_BERR_REPORTING: u32 = 1 << 4;
pub const CAN_CTRLMODE_FD: u32 = 1 << 5;
pub const CAN_CTRLMODE_PRESUME_ACK: u32 = 1 << 6;
pub const CAN_CTRLMODE_FD_NON_ISO: u32 = 1 << 7;
pub const CAN_CTRLMODE_CC_LEN8_DLC: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_state_dense_0_to_5() {
        let s = [
            CAN_STATE_ERROR_ACTIVE,
            CAN_STATE_ERROR_WARNING,
            CAN_STATE_ERROR_PASSIVE,
            CAN_STATE_BUS_OFF,
            CAN_STATE_STOPPED,
            CAN_STATE_SLEEPING,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        assert_eq!(CAN_STATE_MAX, 6);
    }

    #[test]
    fn test_error_states_ordered_severity() {
        // ACTIVE < WARNING < PASSIVE < BUS_OFF (worsening severity).
        assert!(CAN_STATE_ERROR_ACTIVE < CAN_STATE_ERROR_WARNING);
        assert!(CAN_STATE_ERROR_WARNING < CAN_STATE_ERROR_PASSIVE);
        assert!(CAN_STATE_ERROR_PASSIVE < CAN_STATE_BUS_OFF);
    }

    #[test]
    fn test_ifla_can_dense_0_to_11() {
        let a = [
            IFLA_CAN_UNSPEC,
            IFLA_CAN_BITTIMING,
            IFLA_CAN_BITTIMING_CONST,
            IFLA_CAN_CLOCK,
            IFLA_CAN_STATE,
            IFLA_CAN_CTRLMODE,
            IFLA_CAN_RESTART_MS,
            IFLA_CAN_RESTART,
            IFLA_CAN_BERR_COUNTER,
            IFLA_CAN_DATA_BITTIMING,
            IFLA_CAN_DATA_BITTIMING_CONST,
            IFLA_CAN_TERMINATION,
        ];
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_ctrlmode_bits_disjoint_and_dense() {
        let m = [
            CAN_CTRLMODE_LOOPBACK,
            CAN_CTRLMODE_LISTENONLY,
            CAN_CTRLMODE_3_SAMPLES,
            CAN_CTRLMODE_ONE_SHOT,
            CAN_CTRLMODE_BERR_REPORTING,
            CAN_CTRLMODE_FD,
            CAN_CTRLMODE_PRESUME_ACK,
            CAN_CTRLMODE_FD_NON_ISO,
            CAN_CTRLMODE_CC_LEN8_DLC,
        ];
        for (i, &v) in m.iter().enumerate() {
            assert_eq!(v, 1 << i);
        }
        // OR of all = (1 << 9) - 1 (9 single-bit flags).
        let all: u32 = m.iter().fold(0, |acc, &v| acc | v);
        assert_eq!(all, (1u32 << 9) - 1);
    }

    #[test]
    fn test_data_bittiming_attrs_after_basic_bittiming() {
        // CAN-FD adds DATA_BITTIMING(_CONST) after the classical pair.
        assert!(IFLA_CAN_DATA_BITTIMING > IFLA_CAN_BITTIMING_CONST);
        assert!(IFLA_CAN_DATA_BITTIMING_CONST > IFLA_CAN_DATA_BITTIMING);
    }
}
