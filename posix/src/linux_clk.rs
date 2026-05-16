//! `<linux/clk.h>` — Clock framework constants.
//!
//! The common clock framework (CCF) manages hardware clocks:
//! PLLs, dividers, muxes, and gates. Drivers register clock
//! providers; consumers request clocks by name or device tree
//! binding. This module defines clock flags, types, and
//! notification events.

// ---------------------------------------------------------------------------
// Clock flags (CLK_*)
// ---------------------------------------------------------------------------

/// Set rate propagates to parent.
pub const CLK_SET_RATE_GATE: u64 = 1 << 0;
/// Parent rate must not change.
pub const CLK_SET_PARENT_GATE: u64 = 1 << 1;
/// Set rate propagates up.
pub const CLK_SET_RATE_PARENT: u64 = 1 << 2;
/// Ignore unused (don't disable).
pub const CLK_IGNORE_UNUSED: u64 = 1 << 3;
/// Get rate must not sleep.
pub const CLK_GET_RATE_NOCACHE: u64 = 1 << 6;
/// Set rate ungate.
pub const CLK_SET_RATE_UNGATE: u64 = 1 << 10;
/// Hardware-controlled gating.
pub const CLK_IS_CRITICAL: u64 = 1 << 11;
/// Only recalc rate on explicit set_rate.
pub const CLK_OPS_PARENT_ENABLE: u64 = 1 << 12;
/// Duty cycle control supported.
pub const CLK_DUTY_CYCLE_PARENT: u64 = 1 << 13;

// ---------------------------------------------------------------------------
// Clock notification events
// ---------------------------------------------------------------------------

/// Pre-rate change.
pub const PRE_RATE_CHANGE: u32 = 1;
/// Post-rate change.
pub const POST_RATE_CHANGE: u32 = 2;
/// Abort rate change.
pub const ABORT_RATE_CHANGE: u32 = 3;
/// Pre-enable.
pub const PRE_CLK_ENABLE: u32 = 4;
/// Post-enable.
pub const POST_CLK_ENABLE: u32 = 5;
/// Pre-disable.
pub const PRE_CLK_DISABLE: u32 = 6;
/// Post-disable.
pub const POST_CLK_DISABLE: u32 = 7;

// ---------------------------------------------------------------------------
// Clock types (for registration)
// ---------------------------------------------------------------------------

/// Fixed rate clock.
pub const CLK_TYPE_FIXED_RATE: u32 = 0;
/// Gate clock.
pub const CLK_TYPE_GATE: u32 = 1;
/// Divider clock.
pub const CLK_TYPE_DIVIDER: u32 = 2;
/// Mux clock.
pub const CLK_TYPE_MUX: u32 = 3;
/// Fixed factor clock.
pub const CLK_TYPE_FIXED_FACTOR: u32 = 4;
/// Composite clock.
pub const CLK_TYPE_COMPOSITE: u32 = 5;
/// Fractional divider clock.
pub const CLK_TYPE_FRACTIONAL_DIVIDER: u32 = 6;
/// PLL clock.
pub const CLK_TYPE_PLL: u32 = 7;

// ---------------------------------------------------------------------------
// Divider flags
// ---------------------------------------------------------------------------

/// One-based divider (0 means /1).
pub const CLK_DIVIDER_ONE_BASED: u32 = 1 << 0;
/// Power-of-two divider values.
pub const CLK_DIVIDER_POWER_OF_TWO: u32 = 1 << 1;
/// Allow zero divider value.
pub const CLK_DIVIDER_ALLOW_ZERO: u32 = 1 << 2;
/// HiWord mask write.
pub const CLK_DIVIDER_HIWORD_MASK: u32 = 1 << 3;
/// Round to closest rate.
pub const CLK_DIVIDER_ROUND_CLOSEST: u32 = 1 << 4;
/// Read-only divider.
pub const CLK_DIVIDER_READ_ONLY: u32 = 1 << 5;
/// Max divider value at zero.
pub const CLK_DIVIDER_MAX_AT_ZERO: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clk_flags_powers_of_two() {
        let flags: [u64; 9] = [
            CLK_SET_RATE_GATE, CLK_SET_PARENT_GATE, CLK_SET_RATE_PARENT,
            CLK_IGNORE_UNUSED, CLK_GET_RATE_NOCACHE, CLK_SET_RATE_UNGATE,
            CLK_IS_CRITICAL, CLK_OPS_PARENT_ENABLE, CLK_DUTY_CYCLE_PARENT,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_clk_flags_no_overlap() {
        let flags: [u64; 9] = [
            CLK_SET_RATE_GATE, CLK_SET_PARENT_GATE, CLK_SET_RATE_PARENT,
            CLK_IGNORE_UNUSED, CLK_GET_RATE_NOCACHE, CLK_SET_RATE_UNGATE,
            CLK_IS_CRITICAL, CLK_OPS_PARENT_ENABLE, CLK_DUTY_CYCLE_PARENT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_notification_events_distinct() {
        let events = [
            PRE_RATE_CHANGE, POST_RATE_CHANGE, ABORT_RATE_CHANGE,
            PRE_CLK_ENABLE, POST_CLK_ENABLE,
            PRE_CLK_DISABLE, POST_CLK_DISABLE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_clock_types_distinct() {
        let types = [
            CLK_TYPE_FIXED_RATE, CLK_TYPE_GATE, CLK_TYPE_DIVIDER,
            CLK_TYPE_MUX, CLK_TYPE_FIXED_FACTOR, CLK_TYPE_COMPOSITE,
            CLK_TYPE_FRACTIONAL_DIVIDER, CLK_TYPE_PLL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_divider_flags_powers_of_two() {
        let flags = [
            CLK_DIVIDER_ONE_BASED, CLK_DIVIDER_POWER_OF_TWO,
            CLK_DIVIDER_ALLOW_ZERO, CLK_DIVIDER_HIWORD_MASK,
            CLK_DIVIDER_ROUND_CLOSEST, CLK_DIVIDER_READ_ONLY,
            CLK_DIVIDER_MAX_AT_ZERO,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_divider_flags_no_overlap() {
        let flags = [
            CLK_DIVIDER_ONE_BASED, CLK_DIVIDER_POWER_OF_TWO,
            CLK_DIVIDER_ALLOW_ZERO, CLK_DIVIDER_HIWORD_MASK,
            CLK_DIVIDER_ROUND_CLOSEST, CLK_DIVIDER_READ_ONLY,
            CLK_DIVIDER_MAX_AT_ZERO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
