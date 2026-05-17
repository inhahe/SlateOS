//! `<linux/reset-controller.h>` — Reset controller framework constants.
//!
//! Reset controllers manage hardware reset signals for SoC
//! peripherals. Each peripheral typically has a dedicated reset
//! line that can be asserted (held in reset) or deasserted
//! (released from reset). The reset framework provides a consumer
//! API (reset_control_assert/deassert) and a provider API for
//! reset controller drivers. Devices may share reset lines or
//! have dedicated ones.

// ---------------------------------------------------------------------------
// Reset control types
// ---------------------------------------------------------------------------

/// Exclusive reset control (one consumer only).
pub const RESET_TYPE_EXCLUSIVE: u32 = 0;
/// Shared reset control (multiple consumers, last deassert wins).
pub const RESET_TYPE_SHARED: u32 = 1;

// ---------------------------------------------------------------------------
// Reset control states
// ---------------------------------------------------------------------------

/// Reset is deasserted (device is out of reset, operational).
pub const RESET_STATE_DEASSERTED: u32 = 0;
/// Reset is asserted (device is held in reset).
pub const RESET_STATE_ASSERTED: u32 = 1;

// ---------------------------------------------------------------------------
// Reset control flags
// ---------------------------------------------------------------------------

/// Reset is acquired (consumer has a reference).
pub const RESET_FLAG_ACQUIRED: u32 = 1 << 0;
/// Reset may be shared (allow multiple consumers).
pub const RESET_FLAG_SHARED: u32 = 1 << 1;
/// Reset was asserted by this consumer.
pub const RESET_FLAG_ASSERTED: u32 = 1 << 2;
/// Reset deassert is deferred (will be deasserted later).
pub const RESET_FLAG_DEFERRED: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Reset operations
// ---------------------------------------------------------------------------

/// Assert the reset signal (enter reset).
pub const RESET_OP_ASSERT: u32 = 0;
/// Deassert the reset signal (exit reset).
pub const RESET_OP_DEASSERT: u32 = 1;
/// Pulse reset (assert then deassert with small delay).
pub const RESET_OP_RESET: u32 = 2;
/// Get reset status.
pub const RESET_OP_STATUS: u32 = 3;

// ---------------------------------------------------------------------------
// Reset lookup flags
// ---------------------------------------------------------------------------

/// Lookup by index (Nth reset for the device).
pub const RESET_LOOKUP_INDEX: u32 = 0;
/// Lookup by name (named reset from DT/ACPI).
pub const RESET_LOOKUP_NAME: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_types_distinct() {
        assert_ne!(RESET_TYPE_EXCLUSIVE, RESET_TYPE_SHARED);
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(RESET_STATE_DEASSERTED, RESET_STATE_ASSERTED);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            RESET_FLAG_ACQUIRED, RESET_FLAG_SHARED,
            RESET_FLAG_ASSERTED, RESET_FLAG_DEFERRED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_operations_distinct() {
        let ops = [
            RESET_OP_ASSERT, RESET_OP_DEASSERT,
            RESET_OP_RESET, RESET_OP_STATUS,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_lookup_distinct() {
        assert_ne!(RESET_LOOKUP_INDEX, RESET_LOOKUP_NAME);
    }
}
