//! `<linux/auxiliary_bus.h>` — Auxiliary bus constants.
//!
//! The auxiliary bus allows a single physical device driver to
//! create multiple sub-devices (auxiliary devices) that bind to
//! different sub-drivers. This enables modular driver design where
//! a parent PCI driver can create auxiliary devices for RDMA, VDPA,
//! devlink, etc. Each auxiliary device gets its own driver with
//! independent lifecycle. Added in Linux 5.11 as a replacement for
//! ad-hoc sub-device patterns.

// ---------------------------------------------------------------------------
// Auxiliary device states
// ---------------------------------------------------------------------------

/// Device is registered (available for driver binding).
pub const AUXILIARY_STATE_REGISTERED: u32 = 0;
/// Device is bound (driver probed successfully).
pub const AUXILIARY_STATE_BOUND: u32 = 1;
/// Device is being removed.
pub const AUXILIARY_STATE_REMOVING: u32 = 2;

// ---------------------------------------------------------------------------
// Auxiliary device naming
// ---------------------------------------------------------------------------

/// Maximum auxiliary device name length.
pub const AUXILIARY_NAME_MAX: u32 = 32;
/// Name separator between parent and aux device (dot).
pub const AUXILIARY_NAME_SEPARATOR: u8 = b'.';

// ---------------------------------------------------------------------------
// Auxiliary bus match flags
// ---------------------------------------------------------------------------

/// Match by name (module_name.aux_name).
pub const AUXILIARY_MATCH_NAME: u32 = 1 << 0;
/// Match by ID table.
pub const AUXILIARY_MATCH_ID_TABLE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Auxiliary device event types (for notifier)
// ---------------------------------------------------------------------------

/// Auxiliary device added.
pub const AUXILIARY_EVENT_ADD: u32 = 0;
/// Auxiliary device removed.
pub const AUXILIARY_EVENT_REMOVE: u32 = 1;
/// Auxiliary device driver bound.
pub const AUXILIARY_EVENT_BIND: u32 = 2;
/// Auxiliary device driver unbound.
pub const AUXILIARY_EVENT_UNBIND: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            AUXILIARY_STATE_REGISTERED, AUXILIARY_STATE_BOUND,
            AUXILIARY_STATE_REMOVING,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_name_separator() {
        assert_eq!(AUXILIARY_NAME_SEPARATOR, b'.');
    }

    #[test]
    fn test_match_flags_no_overlap() {
        let flags = [AUXILIARY_MATCH_NAME, AUXILIARY_MATCH_ID_TABLE];
        assert_eq!(flags[0] & flags[1], 0);
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_events_distinct() {
        let events = [
            AUXILIARY_EVENT_ADD, AUXILIARY_EVENT_REMOVE,
            AUXILIARY_EVENT_BIND, AUXILIARY_EVENT_UNBIND,
        ];
        for i in 0..events.len() {
            for j in (i + 1)..events.len() {
                assert_ne!(events[i], events[j]);
            }
        }
    }
}
