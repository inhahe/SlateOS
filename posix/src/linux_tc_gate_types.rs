//! `<linux/tc_act/tc_gate.h>` — TC gate action constants.
//!
//! Traffic control gate action constants covering attribute types
//! and gate entry attribute types for time-aware scheduling.

// ---------------------------------------------------------------------------
// TC gate attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_GATE_UNSPEC: u32 = 0;
/// Timestamp.
pub const TCA_GATE_TM: u32 = 1;
/// Parameters.
pub const TCA_GATE_PARMS: u32 = 2;
/// Priority.
pub const TCA_GATE_PRIORITY: u32 = 3;
/// Entry list.
pub const TCA_GATE_ENTRY_LIST: u32 = 4;
/// Base time.
pub const TCA_GATE_BASE_TIME: u32 = 5;
/// Cycle time.
pub const TCA_GATE_CYCLE_TIME: u32 = 6;
/// Cycle time extension.
pub const TCA_GATE_CYCLE_TIME_EXT: u32 = 7;
/// Flags.
pub const TCA_GATE_FLAGS: u32 = 8;
/// Clock ID.
pub const TCA_GATE_CLOCKID: u32 = 9;

// ---------------------------------------------------------------------------
// TC gate entry attribute types
// ---------------------------------------------------------------------------

/// Unspec.
pub const TCA_GATE_ENTRY_UNSPEC: u32 = 0;
/// Entry.
pub const TCA_GATE_ENTRY: u32 = 1;
/// Gate state.
pub const TCA_GATE_ENTRY_GATE: u32 = 2;
/// Interval.
pub const TCA_GATE_ENTRY_INTERVAL: u32 = 3;
/// Max octets.
pub const TCA_GATE_ENTRY_MAX_OCTETS: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gate_attrs_distinct() {
        let attrs = [
            TCA_GATE_UNSPEC, TCA_GATE_TM, TCA_GATE_PARMS,
            TCA_GATE_PRIORITY, TCA_GATE_ENTRY_LIST,
            TCA_GATE_BASE_TIME, TCA_GATE_CYCLE_TIME,
            TCA_GATE_CYCLE_TIME_EXT, TCA_GATE_FLAGS,
            TCA_GATE_CLOCKID,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_entry_attrs_distinct() {
        let attrs = [
            TCA_GATE_ENTRY_UNSPEC, TCA_GATE_ENTRY,
            TCA_GATE_ENTRY_GATE, TCA_GATE_ENTRY_INTERVAL,
            TCA_GATE_ENTRY_MAX_OCTETS,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }
}
