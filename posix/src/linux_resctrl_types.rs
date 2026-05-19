//! `<linux/resctrl.h>` — Resource Control (resctrl) constants.
//!
//! Resource control constants covering monitoring IDs,
//! cache allocation types, and memory bandwidth control.

// ---------------------------------------------------------------------------
// Resource monitoring IDs
// ---------------------------------------------------------------------------

/// LLC occupancy.
pub const RESCTRL_MON_L3_OCCUPANCY: u32 = 0;
/// Total memory bandwidth.
pub const RESCTRL_MON_TOTAL_MBM: u32 = 1;
/// Local memory bandwidth.
pub const RESCTRL_MON_LOCAL_MBM: u32 = 2;

// ---------------------------------------------------------------------------
// Resource types
// ---------------------------------------------------------------------------

/// L3 cache allocation.
pub const RESCTRL_RESOURCE_L3: u32 = 0;
/// L2 cache allocation.
pub const RESCTRL_RESOURCE_L2: u32 = 1;
/// Memory bandwidth.
pub const RESCTRL_RESOURCE_MBA: u32 = 2;
/// Slow memory bandwidth.
pub const RESCTRL_RESOURCE_SMBA: u32 = 3;

// ---------------------------------------------------------------------------
// Configuration schema flags
// ---------------------------------------------------------------------------

/// CDP (Code Data Prioritization) enabled.
pub const RESCTRL_SCHEMA_FLAG_CDP: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Monitor group event IDs
// ---------------------------------------------------------------------------

/// Monitor unspec.
pub const RESCTRL_MON_EVENT_UNSPEC: u32 = 0;
/// LLC occupancy event.
pub const RESCTRL_MON_EVENT_L3_OCCUPANCY: u32 = 1;
/// Total bandwidth event.
pub const RESCTRL_MON_EVENT_TOTAL_BYTES: u32 = 2;
/// Local bandwidth event.
pub const RESCTRL_MON_EVENT_LOCAL_BYTES: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mon_ids_distinct() {
        let ids = [
            RESCTRL_MON_L3_OCCUPANCY, RESCTRL_MON_TOTAL_MBM,
            RESCTRL_MON_LOCAL_MBM,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_resource_types_distinct() {
        let types = [
            RESCTRL_RESOURCE_L3, RESCTRL_RESOURCE_L2,
            RESCTRL_RESOURCE_MBA, RESCTRL_RESOURCE_SMBA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            RESCTRL_MON_EVENT_UNSPEC, RESCTRL_MON_EVENT_L3_OCCUPANCY,
            RESCTRL_MON_EVENT_TOTAL_BYTES, RESCTRL_MON_EVENT_LOCAL_BYTES,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }

    #[test]
    fn test_cdp_flag() {
        assert!(RESCTRL_SCHEMA_FLAG_CDP.is_power_of_two());
    }
}
