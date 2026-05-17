//! `<linux/devcoredump.h>` — Device coredump framework constants.
//!
//! The devcoredump framework allows drivers to store crash dumps
//! (firmware/hardware state at time of failure) in a standard
//! location accessible via sysfs. This is used by WiFi, GPU, and
//! other complex device drivers for post-mortem debugging.

// ---------------------------------------------------------------------------
// Coredump states
// ---------------------------------------------------------------------------

/// Coredump is being collected.
pub const DEV_COREDUMP_STATE_COLLECTING: u8 = 0;
/// Coredump stored, available for reading.
pub const DEV_COREDUMP_STATE_STORED: u8 = 1;
/// Coredump was read and is being freed.
pub const DEV_COREDUMP_STATE_FREE: u8 = 2;

// ---------------------------------------------------------------------------
// Coredump flags
// ---------------------------------------------------------------------------

/// Coredump should be compressed.
pub const DEV_COREDUMP_F_COMPRESS: u32 = 1 << 0;
/// Coredump creation is non-blocking.
pub const DEV_COREDUMP_F_NONBLOCK: u32 = 1 << 1;
/// Copy data rather than taking ownership.
pub const DEV_COREDUMP_F_COPY: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Coredump timeout
// ---------------------------------------------------------------------------

/// Default coredump expiry (5 minutes, in seconds).
pub const DEV_COREDUMP_TIMEOUT_DEFAULT: u32 = 300;
/// Disable automatic deletion.
pub const DEV_COREDUMP_TIMEOUT_DISABLED: u32 = 0;

// ---------------------------------------------------------------------------
// Coredump source types (informal / driver convention)
// ---------------------------------------------------------------------------

/// Firmware crash.
pub const DEV_COREDUMP_SRC_FW: u8 = 0;
/// Hardware error.
pub const DEV_COREDUMP_SRC_HW: u8 = 1;
/// Software/driver error.
pub const DEV_COREDUMP_SRC_SW: u8 = 2;
/// User-requested dump.
pub const DEV_COREDUMP_SRC_USER: u8 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            DEV_COREDUMP_STATE_COLLECTING,
            DEV_COREDUMP_STATE_STORED,
            DEV_COREDUMP_STATE_FREE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            DEV_COREDUMP_F_COMPRESS,
            DEV_COREDUMP_F_NONBLOCK,
            DEV_COREDUMP_F_COPY,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            DEV_COREDUMP_F_COMPRESS,
            DEV_COREDUMP_F_NONBLOCK,
            DEV_COREDUMP_F_COPY,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_source_types_distinct() {
        let srcs = [
            DEV_COREDUMP_SRC_FW, DEV_COREDUMP_SRC_HW,
            DEV_COREDUMP_SRC_SW, DEV_COREDUMP_SRC_USER,
        ];
        for i in 0..srcs.len() {
            for j in (i + 1)..srcs.len() {
                assert_ne!(srcs[i], srcs[j]);
            }
        }
    }

    #[test]
    fn test_timeout_defaults() {
        assert!(DEV_COREDUMP_TIMEOUT_DEFAULT > DEV_COREDUMP_TIMEOUT_DISABLED);
    }
}
