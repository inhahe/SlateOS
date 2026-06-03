//! `<linux/clk-provider.h>` — Common Clock Framework provider constants.
//!
//! Constants describing the flag space and rate semantics used by
//! clock providers under the Common Clock Framework (CCF). Device-tree
//! parsers and clock-tree visualisers consume these.

// ---------------------------------------------------------------------------
// Generic CLK_* flag bits (struct clk_init_data.flags)
// ---------------------------------------------------------------------------

/// Parent rate changes automatically propagate to the child.
pub const CLK_SET_RATE_GATE: u32 = 1 << 0;
/// Clock cannot be changed while enabled.
pub const CLK_SET_PARENT_GATE: u32 = 1 << 1;
/// Allow rate-change propagation to parent.
pub const CLK_SET_RATE_PARENT: u32 = 1 << 2;
/// Don't unprepare/disable on usecount==0.
pub const CLK_IGNORE_UNUSED: u32 = 1 << 3;
/// Clock rate is fixed.
pub const CLK_GET_RATE_NOCACHE: u32 = 1 << 6;
/// Allow rate-change determination caching.
pub const CLK_SET_RATE_NO_REPARENT: u32 = 1 << 7;
/// Parent change requires a new accuracy.
pub const CLK_GET_ACCURACY_NOCACHE: u32 = 1 << 8;
/// Recalculate rate via the parent.
pub const CLK_RECALC_NEW_RATES: u32 = 1 << 9;
/// Round-rate not gated by enable state.
pub const CLK_SET_RATE_UNGATE: u32 = 1 << 10;
/// Clock is critical — must never be gated.
pub const CLK_IS_CRITICAL: u32 = 1 << 11;
/// Clock operations are protected by the OPP lock.
pub const CLK_OPS_PARENT_ENABLE: u32 = 1 << 12;
/// Duty-cycle is configurable per provider.
pub const CLK_DUTY_CYCLE_PARENT: u32 = 1 << 13;

// ---------------------------------------------------------------------------
// CLK_DIVIDER_* flags (clock dividers)
// ---------------------------------------------------------------------------

/// Divider value 0 means bypass (no divide).
pub const CLK_DIVIDER_ONE_BASED: u32 = 1 << 0;
/// Divider is a power-of-two index.
pub const CLK_DIVIDER_POWER_OF_TWO: u32 = 1 << 1;
/// Divider supports the kernel's "allow_zero" tristate.
pub const CLK_DIVIDER_ALLOW_ZERO: u32 = 1 << 2;
/// Divider is hiword-masked register.
pub const CLK_DIVIDER_HIWORD_MASK: u32 = 1 << 3;
/// Round closest, not down.
pub const CLK_DIVIDER_ROUND_CLOSEST: u32 = 1 << 4;
/// Read-only divider (cannot be written).
pub const CLK_DIVIDER_READ_ONLY: u32 = 1 << 5;
/// Divider uses the maximum-bit-value semantic.
pub const CLK_DIVIDER_MAX_AT_ZERO: u32 = 1 << 6;

// ---------------------------------------------------------------------------
// CLK_MUX_* flags (clock multiplexers)
// ---------------------------------------------------------------------------

/// Mux is hiword-masked.
pub const CLK_MUX_HIWORD_MASK: u32 = 1 << 2;
/// Mux is read-only.
pub const CLK_MUX_READ_ONLY: u32 = 1 << 3;
/// Mux uses sequential indexing (not table-driven).
pub const CLK_MUX_INDEX_ONE: u32 = 1 << 0;
/// Mux uses bit-shift indexing.
pub const CLK_MUX_INDEX_BIT: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generic_flags_distinct_powers_of_two() {
        let flags = [
            CLK_SET_RATE_GATE,
            CLK_SET_PARENT_GATE,
            CLK_SET_RATE_PARENT,
            CLK_IGNORE_UNUSED,
            CLK_GET_RATE_NOCACHE,
            CLK_SET_RATE_NO_REPARENT,
            CLK_GET_ACCURACY_NOCACHE,
            CLK_RECALC_NEW_RATES,
            CLK_SET_RATE_UNGATE,
            CLK_IS_CRITICAL,
            CLK_OPS_PARENT_ENABLE,
            CLK_DUTY_CYCLE_PARENT,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_divider_flags_distinct_powers_of_two() {
        let flags = [
            CLK_DIVIDER_ONE_BASED,
            CLK_DIVIDER_POWER_OF_TWO,
            CLK_DIVIDER_ALLOW_ZERO,
            CLK_DIVIDER_HIWORD_MASK,
            CLK_DIVIDER_ROUND_CLOSEST,
            CLK_DIVIDER_READ_ONLY,
            CLK_DIVIDER_MAX_AT_ZERO,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_mux_flags_distinct() {
        let flags = [
            CLK_MUX_HIWORD_MASK,
            CLK_MUX_READ_ONLY,
            CLK_MUX_INDEX_ONE,
            CLK_MUX_INDEX_BIT,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }
}
