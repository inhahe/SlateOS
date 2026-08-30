//! `<linux/surface_aggregator/controller.h>` — Surface Aggregator Module constants.
//!
//! The Surface Aggregator Module (SAM) is the embedded controller on
//! Microsoft Surface devices, managing battery, thermal, keyboard,
//! touchpad, and other subsystems via a serial protocol.

// ---------------------------------------------------------------------------
// Target categories (device subsystems)
// ---------------------------------------------------------------------------

/// SAM subsystem.
pub const SSAM_SSH_TC_SAM: u8 = 0x01;
/// Battery subsystem.
pub const SSAM_SSH_TC_BAT: u8 = 0x02;
/// Thermal subsystem.
pub const SSAM_SSH_TC_TMP: u8 = 0x03;
/// Performance mode.
pub const SSAM_SSH_TC_PMC: u8 = 0x04;
/// Fan control.
pub const SSAM_SSH_TC_FAN: u8 = 0x05;
/// Touchpad/keyboard.
pub const SSAM_SSH_TC_HID: u8 = 0x15;
/// Base (keyboard cover) connection.
pub const SSAM_SSH_TC_BAS: u8 = 0x11;
/// DTX (detach).
pub const SSAM_SSH_TC_KIP: u8 = 0x1E;

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

/// Request type: SSH event.
pub const SSAM_SSH_FRAME_TYPE_DATA: u8 = 0x80;
/// Request type: SSH ACK.
pub const SSAM_SSH_FRAME_TYPE_ACK: u8 = 0x04;
/// Request type: SSH NAK.
pub const SSAM_SSH_FRAME_TYPE_NAK: u8 = 0x05;

// ---------------------------------------------------------------------------
// Event flags
// ---------------------------------------------------------------------------

/// Sequenced event (has completion notification).
pub const SSAM_EVENT_SEQUENCED: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// DTX (detach) states
// ---------------------------------------------------------------------------

/// Base attached.
pub const SSAM_BAS_STATE_ATTACHED: u32 = 0;
/// Base detach ready.
pub const SSAM_BAS_STATE_DETACH_READY: u32 = 1;
/// Base detached.
pub const SSAM_BAS_STATE_DETACHED: u32 = 2;

// ---------------------------------------------------------------------------
// Performance modes
// ---------------------------------------------------------------------------

/// Default mode.
pub const SSAM_PERF_MODE_NORMAL: u32 = 1;
/// Battery saver.
pub const SSAM_PERF_MODE_BATTERY_SAVER: u32 = 2;
/// Better performance.
pub const SSAM_PERF_MODE_BETTER_PERF: u32 = 3;
/// Best performance.
pub const SSAM_PERF_MODE_BEST_PERF: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_categories_distinct() {
        let tcs = [
            SSAM_SSH_TC_SAM,
            SSAM_SSH_TC_BAT,
            SSAM_SSH_TC_TMP,
            SSAM_SSH_TC_PMC,
            SSAM_SSH_TC_FAN,
            SSAM_SSH_TC_HID,
            SSAM_SSH_TC_BAS,
            SSAM_SSH_TC_KIP,
        ];
        for i in 0..tcs.len() {
            for j in (i + 1)..tcs.len() {
                assert_ne!(tcs[i], tcs[j]);
            }
        }
    }

    #[test]
    fn test_frame_types_distinct() {
        let types = [
            SSAM_SSH_FRAME_TYPE_DATA,
            SSAM_SSH_FRAME_TYPE_ACK,
            SSAM_SSH_FRAME_TYPE_NAK,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dtx_states_distinct() {
        let states = [
            SSAM_BAS_STATE_ATTACHED,
            SSAM_BAS_STATE_DETACH_READY,
            SSAM_BAS_STATE_DETACHED,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_perf_modes_distinct() {
        let modes = [
            SSAM_PERF_MODE_NORMAL,
            SSAM_PERF_MODE_BATTERY_SAVER,
            SSAM_PERF_MODE_BETTER_PERF,
            SSAM_PERF_MODE_BEST_PERF,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
