//! `<linux/can/netlink.h>` — CAN bus netlink configuration constants.
//!
//! The CAN (Controller Area Network) netlink interface configures CAN
//! network devices via rtnetlink. It sets bitrate, sample point,
//! error reporting, bus-off recovery, and CAN FD (Flexible Data-Rate)
//! parameters. CAN is the standard bus in automotive, industrial
//! automation, and embedded systems. SocketCAN provides the Linux
//! network stack integration; the netlink API configures the physical
//! layer parameters.

// ---------------------------------------------------------------------------
// CAN netlink interface types (IFLA_CAN_*)
// ---------------------------------------------------------------------------

/// CAN bit timing parameters.
pub const IFLA_CAN_BITTIMING: u32 = 1;
/// CAN bit timing constants (hardware limits).
pub const IFLA_CAN_BITTIMING_CONST: u32 = 2;
/// CAN clock frequency.
pub const IFLA_CAN_CLOCK: u32 = 3;
/// CAN device state (active/warning/passive/bus-off).
pub const IFLA_CAN_STATE: u32 = 4;
/// CAN control mode (loopback, listen-only, etc.).
pub const IFLA_CAN_CTRLMODE: u32 = 5;
/// CAN restart delay (ms) for bus-off recovery.
pub const IFLA_CAN_RESTART_MS: u32 = 6;
/// CAN restart (trigger restart from bus-off).
pub const IFLA_CAN_RESTART: u32 = 7;
/// CAN bus error counters (TEC/REC).
pub const IFLA_CAN_BERR_COUNTER: u32 = 8;
/// CAN FD data bit timing parameters.
pub const IFLA_CAN_DATA_BITTIMING: u32 = 9;
/// CAN FD data bit timing constants.
pub const IFLA_CAN_DATA_BITTIMING_CONST: u32 = 10;
/// CAN termination resistance.
pub const IFLA_CAN_TERMINATION: u32 = 11;
/// CAN available termination values.
pub const IFLA_CAN_TERMINATION_CONST: u32 = 12;
/// CAN bit rate switching ratio.
pub const IFLA_CAN_BITRATE_CONST: u32 = 13;
/// CAN data bit rate constants.
pub const IFLA_CAN_DATA_BITRATE_CONST: u32 = 14;
/// CAN maximum bit rate.
pub const IFLA_CAN_BITRATE_MAX: u32 = 15;
/// CAN TDC (Transmitter Delay Compensation).
pub const IFLA_CAN_TDC: u32 = 16;
/// CAN control mode extended.
pub const IFLA_CAN_CTRLMODE_EXT: u32 = 17;

// ---------------------------------------------------------------------------
// CAN device states
// ---------------------------------------------------------------------------

/// Error-active state (normal operation).
pub const CAN_STATE_ERROR_ACTIVE: u32 = 0;
/// Error-warning state (error count threshold exceeded).
pub const CAN_STATE_ERROR_WARNING: u32 = 1;
/// Error-passive state (high error count).
pub const CAN_STATE_ERROR_PASSIVE: u32 = 2;
/// Bus-off state (device disconnected from bus).
pub const CAN_STATE_BUS_OFF: u32 = 3;
/// Device is stopped.
pub const CAN_STATE_STOPPED: u32 = 4;
/// Device is sleeping.
pub const CAN_STATE_SLEEPING: u32 = 5;

// ---------------------------------------------------------------------------
// CAN control mode flags
// ---------------------------------------------------------------------------

/// Enable loopback mode.
pub const CAN_CTRLMODE_LOOPBACK: u32 = 1 << 0;
/// Enable listen-only mode (no ACK, no transmit).
pub const CAN_CTRLMODE_LISTENONLY: u32 = 1 << 1;
/// Enable triple sampling.
pub const CAN_CTRLMODE_3_SAMPLES: u32 = 1 << 2;
/// Enable one-shot transmit (no retransmit on error).
pub const CAN_CTRLMODE_ONE_SHOT: u32 = 1 << 3;
/// Enable bus error reporting.
pub const CAN_CTRLMODE_BERR_REPORTING: u32 = 1 << 4;
/// Enable CAN FD mode.
pub const CAN_CTRLMODE_FD: u32 = 1 << 5;
/// Presume ACK (don't check for ACK).
pub const CAN_CTRLMODE_PRESUME_ACK: u32 = 1 << 6;
/// Enable non-ISO CAN FD mode.
pub const CAN_CTRLMODE_FD_NON_ISO: u32 = 1 << 7;
/// Enable classic CAN DLC (data length code) checking.
pub const CAN_CTRLMODE_CC_LEN8_DLC: u32 = 1 << 8;
/// Enable TDC (Transmitter Delay Compensation).
pub const CAN_CTRLMODE_TDC_AUTO: u32 = 1 << 9;
/// Enable manual TDC.
pub const CAN_CTRLMODE_TDC_MANUAL: u32 = 1 << 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ifla_attrs_distinct() {
        let attrs = [
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
            IFLA_CAN_TERMINATION_CONST,
            IFLA_CAN_BITRATE_CONST,
            IFLA_CAN_DATA_BITRATE_CONST,
            IFLA_CAN_BITRATE_MAX,
            IFLA_CAN_TDC,
            IFLA_CAN_CTRLMODE_EXT,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        let states = [
            CAN_STATE_ERROR_ACTIVE,
            CAN_STATE_ERROR_WARNING,
            CAN_STATE_ERROR_PASSIVE,
            CAN_STATE_BUS_OFF,
            CAN_STATE_STOPPED,
            CAN_STATE_SLEEPING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_ctrlmode_flags_no_overlap() {
        let flags = [
            CAN_CTRLMODE_LOOPBACK,
            CAN_CTRLMODE_LISTENONLY,
            CAN_CTRLMODE_3_SAMPLES,
            CAN_CTRLMODE_ONE_SHOT,
            CAN_CTRLMODE_BERR_REPORTING,
            CAN_CTRLMODE_FD,
            CAN_CTRLMODE_PRESUME_ACK,
            CAN_CTRLMODE_FD_NON_ISO,
            CAN_CTRLMODE_CC_LEN8_DLC,
            CAN_CTRLMODE_TDC_AUTO,
            CAN_CTRLMODE_TDC_MANUAL,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_state_ordering() {
        // States follow error severity ordering
        assert!(CAN_STATE_ERROR_ACTIVE < CAN_STATE_ERROR_WARNING);
        assert!(CAN_STATE_ERROR_WARNING < CAN_STATE_ERROR_PASSIVE);
        assert!(CAN_STATE_ERROR_PASSIVE < CAN_STATE_BUS_OFF);
    }
}
