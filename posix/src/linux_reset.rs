//! `<linux/reset.h>` — Reset controller constants.
//!
//! The reset controller framework manages hardware reset lines
//! (block-level resets within SoCs). Drivers can assert/deassert
//! reset lines to bring IP blocks to a known state during
//! initialization or error recovery.

// ---------------------------------------------------------------------------
// Reset control flags
// ---------------------------------------------------------------------------

/// Shared reset — multiple consumers can hold.
pub const RESET_SHARED: u32 = 1 << 0;
/// Exclusive reset — only one consumer.
pub const RESET_EXCLUSIVE: u32 = 0;

// ---------------------------------------------------------------------------
// Reset status
// ---------------------------------------------------------------------------

/// Reset line is asserted (device held in reset).
pub const RESET_ASSERTED: u32 = 0;
/// Reset line is deasserted (device running).
pub const RESET_DEASSERTED: u32 = 1;

// ---------------------------------------------------------------------------
// Reset types (for reset_control_ops)
// ---------------------------------------------------------------------------

/// Self-deasserting reset (pulse).
pub const RESET_TYPE_SELF_DEASSERT: u32 = 0;
/// Latched reset (stays asserted until explicit deassert).
pub const RESET_TYPE_LATCHED: u32 = 1;
/// Level-triggered reset.
pub const RESET_TYPE_LEVEL: u32 = 2;

// ---------------------------------------------------------------------------
// Reset lookup flags
// ---------------------------------------------------------------------------

/// Acquire reset in asserted state.
pub const RESET_LOOKUP_ACQUIRED: u32 = 0;
/// Acquire reset in shared mode.
pub const RESET_LOOKUP_SHARED: u32 = 1;
/// Reset is optional (don't fail if missing).
pub const RESET_LOOKUP_OPTIONAL: u32 = 2;

// ---------------------------------------------------------------------------
// Common reset IDs (driver convention)
// ---------------------------------------------------------------------------

/// Reset the entire block.
pub const RESET_ID_FULL: u32 = 0;
/// Reset only the data path.
pub const RESET_ID_DATA: u32 = 1;
/// Reset only the control path.
pub const RESET_ID_CTRL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_vs_exclusive() {
        assert_ne!(RESET_SHARED, RESET_EXCLUSIVE);
        assert!(RESET_SHARED.is_power_of_two());
    }

    #[test]
    fn test_status_values() {
        assert_eq!(RESET_ASSERTED, 0);
        assert_eq!(RESET_DEASSERTED, 1);
        assert_ne!(RESET_ASSERTED, RESET_DEASSERTED);
    }

    #[test]
    fn test_reset_types_distinct() {
        let types = [
            RESET_TYPE_SELF_DEASSERT,
            RESET_TYPE_LATCHED,
            RESET_TYPE_LEVEL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_lookup_flags_distinct() {
        let flags = [
            RESET_LOOKUP_ACQUIRED,
            RESET_LOOKUP_SHARED,
            RESET_LOOKUP_OPTIONAL,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_reset_ids_distinct() {
        let ids = [RESET_ID_FULL, RESET_ID_DATA, RESET_ID_CTRL];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }
}
