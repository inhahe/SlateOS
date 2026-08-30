//! `<linux/clk.h>` — Common Clock Framework (CCF) constants.
//!
//! The Common Clock Framework manages clock trees on SoCs. Every
//! peripheral has a clock that must be enabled and configured to the
//! correct rate before use. Clocks form hierarchical trees (PLLs →
//! dividers → muxes → gates → peripherals). The CCF provides rate
//! setting, parent selection, enable/disable, and duty cycle control.
//! Consumer drivers use `clk_get`, `clk_prepare_enable`, and
//! `clk_set_rate` to manage their clocks.

// ---------------------------------------------------------------------------
// Clock flags (struct clk_init_data.flags)
// ---------------------------------------------------------------------------

/// Clock rate propagates to parent (set_rate on child changes parent rate).
pub const CLK_SET_RATE_PARENT: u32 = 1 << 0;
/// Ignore unused flag (don't disable during late_initcall orphan cleanup).
pub const CLK_IGNORE_UNUSED: u32 = 1 << 3;
/// Clock is critical (always on, system may fail if disabled).
pub const CLK_IS_CRITICAL: u32 = 1 << 11;
/// Get rate from parent (rate = parent_rate / div, no caching).
pub const CLK_GET_RATE_NOCACHE: u32 = 1 << 6;
/// Rate change propagates to parent (request parent rate change).
pub const CLK_SET_RATE_NO_REPARENT: u32 = 1 << 7;
/// Duty cycle may be modified by this clock.
pub const CLK_DUTY_CYCLE_PARENT: u32 = 1 << 12;
/// Clock's parent is set by hardware/firmware (don't change).
pub const CLK_OPS_PARENT_ENABLE: u32 = 1 << 8;

// ---------------------------------------------------------------------------
// Clock types (clock hardware operations)
// ---------------------------------------------------------------------------

/// Fixed-rate clock (rate never changes).
pub const CLK_TYPE_FIXED: u32 = 0;
/// Gate clock (on/off only, no rate change).
pub const CLK_TYPE_GATE: u32 = 1;
/// Divider clock (rate = parent_rate / N).
pub const CLK_TYPE_DIVIDER: u32 = 2;
/// Mux clock (selects one of multiple parents).
pub const CLK_TYPE_MUX: u32 = 3;
/// PLL clock (phase-locked loop, configurable rate).
pub const CLK_TYPE_PLL: u32 = 4;
/// Fractional divider.
pub const CLK_TYPE_FRAC_DIV: u32 = 5;
/// Composite clock (gate + mux + divider combined).
pub const CLK_TYPE_COMPOSITE: u32 = 6;

// ---------------------------------------------------------------------------
// Clock notifier events
// ---------------------------------------------------------------------------

/// Pre-rate-change notification.
pub const CLK_PRE_RATE_CHANGE: u32 = 1;
/// Post-rate-change notification.
pub const CLK_POST_RATE_CHANGE: u32 = 2;
/// Rate change aborted (rollback).
pub const CLK_ABORT_RATE_CHANGE: u32 = 3;
/// Pre-enable notification.
pub const CLK_PRE_ENABLE: u32 = 4;
/// Post-enable notification.
pub const CLK_POST_ENABLE: u32 = 5;

// ---------------------------------------------------------------------------
// Clock consumer states
// ---------------------------------------------------------------------------

/// Clock is unprepared and disabled.
pub const CLK_STATE_OFF: u32 = 0;
/// Clock is prepared but not enabled.
pub const CLK_STATE_PREPARED: u32 = 1;
/// Clock is prepared and enabled.
pub const CLK_STATE_ENABLED: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CLK_SET_RATE_PARENT,
            CLK_IGNORE_UNUSED,
            CLK_IS_CRITICAL,
            CLK_GET_RATE_NOCACHE,
            CLK_SET_RATE_NO_REPARENT,
            CLK_DUTY_CYCLE_PARENT,
            CLK_OPS_PARENT_ENABLE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_clock_types_distinct() {
        let types = [
            CLK_TYPE_FIXED,
            CLK_TYPE_GATE,
            CLK_TYPE_DIVIDER,
            CLK_TYPE_MUX,
            CLK_TYPE_PLL,
            CLK_TYPE_FRAC_DIV,
            CLK_TYPE_COMPOSITE,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_notifier_events_distinct() {
        let events = [
            CLK_PRE_RATE_CHANGE,
            CLK_POST_RATE_CHANGE,
            CLK_ABORT_RATE_CHANGE,
            CLK_PRE_ENABLE,
            CLK_POST_ENABLE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_consumer_states_distinct() {
        let states = [CLK_STATE_OFF, CLK_STATE_PREPARED, CLK_STATE_ENABLED];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
