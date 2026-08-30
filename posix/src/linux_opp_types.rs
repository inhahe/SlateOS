//! `<linux/pm_opp.h>` — Operating Performance Points (OPP) constants.
//!
//! OPP tables define the valid (frequency, voltage) operating points
//! for a device. The OPP framework manages these tables and provides
//! an API for governors and drivers to query available frequencies,
//! associated voltages, and power levels. OPPs can be defined in
//! device tree or registered dynamically. The framework supports
//! sharing OPP tables across devices and genpd (power domain)
//! integration.

// ---------------------------------------------------------------------------
// OPP table flags
// ---------------------------------------------------------------------------

/// OPP table is shared between multiple devices.
pub const OPP_TABLE_SHARED: u32 = 1 << 0;
/// OPP table supports gear switching (bandwidth levels).
pub const OPP_TABLE_GEARS: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// OPP entry flags
// ---------------------------------------------------------------------------

/// OPP is available (can be selected).
pub const OPP_AVAILABLE: u32 = 1 << 0;
/// OPP is a turbo/boost entry.
pub const OPP_TURBO: u32 = 1 << 1;
/// OPP was dynamically added (not from DT).
pub const OPP_DYNAMIC: u32 = 1 << 2;
/// OPP is suspended (temporarily unavailable).
pub const OPP_SUSPENDED: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// OPP event notifications
// ---------------------------------------------------------------------------

/// OPP table was added.
pub const OPP_EVENT_ADD: u32 = 0;
/// OPP table was removed.
pub const OPP_EVENT_REMOVE: u32 = 1;
/// OPP was enabled.
pub const OPP_EVENT_ENABLE: u32 = 2;
/// OPP was disabled.
pub const OPP_EVENT_DISABLE: u32 = 3;
/// OPP adjusted (frequency/voltage changed).
pub const OPP_EVENT_ADJUST: u32 = 4;

// ---------------------------------------------------------------------------
// OPP search directions
// ---------------------------------------------------------------------------

/// Find exact matching OPP.
pub const OPP_SEARCH_EXACT: u32 = 0;
/// Find OPP with frequency >= target (ceiling).
pub const OPP_SEARCH_CEIL: u32 = 1;
/// Find OPP with frequency <= target (floor).
pub const OPP_SEARCH_FLOOR: u32 = 2;

// ---------------------------------------------------------------------------
// OPP supply (voltage/current) indices
// ---------------------------------------------------------------------------

/// Primary supply (main voltage rail).
pub const OPP_SUPPLY_PRIMARY: u32 = 0;
/// Secondary supply (e.g., memory voltage).
pub const OPP_SUPPLY_SECONDARY: u32 = 1;
/// Maximum supplies per OPP.
pub const OPP_MAX_SUPPLIES: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_flags_no_overlap() {
        let flags = [OPP_TABLE_SHARED, OPP_TABLE_GEARS];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_entry_flags_no_overlap() {
        let flags = [OPP_AVAILABLE, OPP_TURBO, OPP_DYNAMIC, OPP_SUSPENDED];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            OPP_EVENT_ADD,
            OPP_EVENT_REMOVE,
            OPP_EVENT_ENABLE,
            OPP_EVENT_DISABLE,
            OPP_EVENT_ADJUST,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_search_directions_distinct() {
        let dirs = [OPP_SEARCH_EXACT, OPP_SEARCH_CEIL, OPP_SEARCH_FLOOR];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }

    #[test]
    fn test_supply_indices() {
        assert_ne!(OPP_SUPPLY_PRIMARY, OPP_SUPPLY_SECONDARY);
        assert!(OPP_MAX_SUPPLIES > OPP_SUPPLY_SECONDARY);
    }
}
