//! `<linux/fpga/fpga-mgr.h>` — FPGA manager constants.
//!
//! The Linux FPGA subsystem manages field-programmable gate arrays,
//! providing a unified interface for bitstream loading, partial
//! reconfiguration, and status monitoring via sysfs.

// ---------------------------------------------------------------------------
// FPGA manager states
// ---------------------------------------------------------------------------

/// Unknown state.
pub const FPGA_MGR_STATE_UNKNOWN: u32 = 0;
/// Powered off.
pub const FPGA_MGR_STATE_POWER_OFF: u32 = 1;
/// Powered up.
pub const FPGA_MGR_STATE_POWER_UP: u32 = 2;
/// Reset state.
pub const FPGA_MGR_STATE_RESET: u32 = 3;
/// Firmware request in progress.
pub const FPGA_MGR_STATE_FIRMWARE_REQ: u32 = 4;
/// Firmware request error.
pub const FPGA_MGR_STATE_FIRMWARE_REQ_ERR: u32 = 5;
/// Write init.
pub const FPGA_MGR_STATE_WRITE_INIT: u32 = 6;
/// Write init error.
pub const FPGA_MGR_STATE_WRITE_INIT_ERR: u32 = 7;
/// Write in progress.
pub const FPGA_MGR_STATE_WRITE: u32 = 8;
/// Write error.
pub const FPGA_MGR_STATE_WRITE_ERR: u32 = 9;
/// Write complete.
pub const FPGA_MGR_STATE_WRITE_COMPLETE: u32 = 10;
/// Write complete error.
pub const FPGA_MGR_STATE_WRITE_COMPLETE_ERR: u32 = 11;
/// Operating (configured and active).
pub const FPGA_MGR_STATE_OPERATING: u32 = 12;

// ---------------------------------------------------------------------------
// FPGA manager flags
// ---------------------------------------------------------------------------

/// Partial reconfiguration.
pub const FPGA_MGR_PARTIAL_RECONFIG: u32 = 1 << 0;
/// External configuration done.
pub const FPGA_MGR_EXTERNAL_CONFIG: u32 = 1 << 1;
/// Encrypted bitstream.
pub const FPGA_MGR_ENCRYPTED_BITSTREAM: u32 = 1 << 2;
/// Bitstream is compressed.
pub const FPGA_MGR_BITSTREAM_LSB_FIRST: u32 = 1 << 3;
/// Compressed bitstream.
pub const FPGA_MGR_COMPRESSED_BITSTREAM: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// FPGA bridge operations
// ---------------------------------------------------------------------------

/// Bridge is disabled (disconnected).
pub const FPGA_BRIDGE_DISABLED: u32 = 0;
/// Bridge is enabled (connected).
pub const FPGA_BRIDGE_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// FPGA region flags
// ---------------------------------------------------------------------------

/// Full reconfiguration supported.
pub const FPGA_REGION_FULL_RECONFIG: u32 = 0;
/// Partial reconfiguration supported.
pub const FPGA_REGION_PARTIAL_RECONFIG: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            FPGA_MGR_STATE_UNKNOWN, FPGA_MGR_STATE_POWER_OFF,
            FPGA_MGR_STATE_POWER_UP, FPGA_MGR_STATE_RESET,
            FPGA_MGR_STATE_FIRMWARE_REQ, FPGA_MGR_STATE_FIRMWARE_REQ_ERR,
            FPGA_MGR_STATE_WRITE_INIT, FPGA_MGR_STATE_WRITE_INIT_ERR,
            FPGA_MGR_STATE_WRITE, FPGA_MGR_STATE_WRITE_ERR,
            FPGA_MGR_STATE_WRITE_COMPLETE, FPGA_MGR_STATE_WRITE_COMPLETE_ERR,
            FPGA_MGR_STATE_OPERATING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_are_powers_of_two() {
        let flags = [
            FPGA_MGR_PARTIAL_RECONFIG, FPGA_MGR_EXTERNAL_CONFIG,
            FPGA_MGR_ENCRYPTED_BITSTREAM, FPGA_MGR_BITSTREAM_LSB_FIRST,
            FPGA_MGR_COMPRESSED_BITSTREAM,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x} is not a power of two", flag);
        }
    }

    #[test]
    fn test_bridge_states() {
        assert_eq!(FPGA_BRIDGE_DISABLED, 0);
        assert_eq!(FPGA_BRIDGE_ENABLED, 1);
    }

    #[test]
    fn test_region_flags() {
        assert_ne!(FPGA_REGION_FULL_RECONFIG, FPGA_REGION_PARTIAL_RECONFIG);
    }

    #[test]
    fn test_state_values() {
        assert_eq!(FPGA_MGR_STATE_UNKNOWN, 0);
        assert_eq!(FPGA_MGR_STATE_OPERATING, 12);
    }
}
