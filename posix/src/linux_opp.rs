//! `<linux/pm_opp.h>` — Operating Performance Points constants.
//!
//! OPP (Operating Performance Point) tables define valid
//! voltage–frequency pairs for a device. The OPP framework
//! is used by cpufreq, devfreq, and thermal governors to
//! select operating points that balance performance and power.

// ---------------------------------------------------------------------------
// OPP table types
// ---------------------------------------------------------------------------

/// OPP is available.
pub const OPP_AVAILABLE: u32 = 1;
/// OPP is not available (disabled).
pub const OPP_UNAVAILABLE: u32 = 0;

// ---------------------------------------------------------------------------
// OPP flags
// ---------------------------------------------------------------------------

/// OPP is turbo/boost mode.
pub const OPP_TURBO: u32 = 1 << 0;
/// OPP is a suspend OPP.
pub const OPP_SUSPEND: u32 = 1 << 1;
/// OPP is shared across CPUs.
pub const OPP_SHARED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// OPP event types (for notifiers)
// ---------------------------------------------------------------------------

/// OPP added.
pub const OPP_EVENT_ADD: u32 = 0;
/// OPP removed.
pub const OPP_EVENT_REMOVE: u32 = 1;
/// OPP enabled.
pub const OPP_EVENT_ENABLE: u32 = 2;
/// OPP disabled.
pub const OPP_EVENT_DISABLE: u32 = 3;
/// OPP table adjusted.
pub const OPP_EVENT_ADJUST_VOLTAGE: u32 = 4;

// ---------------------------------------------------------------------------
// Voltage supply names (common)
// ---------------------------------------------------------------------------

/// Default supply name.
pub const OPP_SUPPLY_DEFAULT: &str = "vdd";
/// Memory supply name.
pub const OPP_SUPPLY_MEM: &str = "vdd-mem";
/// I/O supply name.
pub const OPP_SUPPLY_IO: &str = "vdd-io";

// ---------------------------------------------------------------------------
// Required OPP direction
// ---------------------------------------------------------------------------

/// Scale up (increase frequency/voltage first).
pub const OPP_SCALING_UP: u32 = 0;
/// Scale down (decrease frequency/voltage first).
pub const OPP_SCALING_DOWN: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_availability() {
        assert_ne!(OPP_AVAILABLE, OPP_UNAVAILABLE);
        assert_eq!(OPP_AVAILABLE, 1);
        assert_eq!(OPP_UNAVAILABLE, 0);
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [OPP_TURBO, OPP_SUSPEND, OPP_SHARED];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [OPP_TURBO, OPP_SUSPEND, OPP_SHARED];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            OPP_EVENT_ADD, OPP_EVENT_REMOVE, OPP_EVENT_ENABLE,
            OPP_EVENT_DISABLE, OPP_EVENT_ADJUST_VOLTAGE,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_supply_names_distinct() {
        let names = [OPP_SUPPLY_DEFAULT, OPP_SUPPLY_MEM, OPP_SUPPLY_IO];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_scaling_directions_distinct() {
        assert_ne!(OPP_SCALING_UP, OPP_SCALING_DOWN);
    }
}
