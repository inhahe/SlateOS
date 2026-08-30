//! `<linux/component.h>` — Driver-model component aggregator constants.
//!
//! The component framework lets a single logical device (e.g., DRM
//! card) be assembled from multiple platform-bound sub-drivers. The
//! "master" waits until every "component" registers, then binds them
//! together. This module exposes the match flags and ordering used.

// ---------------------------------------------------------------------------
// Component match flags
// ---------------------------------------------------------------------------

/// Match exact device pointer (no glob).
pub const COMPONENT_MATCH_EXACT: u32 = 1 << 0;
/// Match by OF (device-tree) node.
pub const COMPONENT_MATCH_OF_NODE: u32 = 1 << 1;
/// Match by ACPI handle.
pub const COMPONENT_MATCH_ACPI: u32 = 1 << 2;
/// Match by device-name string.
pub const COMPONENT_MATCH_DEVNAME: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Master/component states
// ---------------------------------------------------------------------------

pub const COMPONENT_STATE_UNBOUND: u32 = 0;
pub const COMPONENT_STATE_BINDING: u32 = 1;
pub const COMPONENT_STATE_BOUND: u32 = 2;
pub const COMPONENT_STATE_UNBINDING: u32 = 3;

// ---------------------------------------------------------------------------
// Maximum components per master
// ---------------------------------------------------------------------------

/// Practical maximum components per master (no hard kernel limit, but
/// drivers rarely aggregate more than 16 — DRM bridges, CODECs, etc.).
pub const COMPONENT_MAX_PER_MASTER: usize = 16;

// ---------------------------------------------------------------------------
// Sysfs paths for inspecting bound components
// ---------------------------------------------------------------------------

pub const COMPONENT_SYSFS_ROOT: &str = "/sys/devices";
pub const COMPONENT_SYSFS_DRIVER_LINK: &str = "driver";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_match_flags_distinct_single_bit() {
        let f = [
            COMPONENT_MATCH_EXACT,
            COMPONENT_MATCH_OF_NODE,
            COMPONENT_MATCH_ACPI,
            COMPONENT_MATCH_DEVNAME,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
        // OR of all 4 = 0x0F.
        let or_all = f.iter().fold(0u32, |a, &v| a | v);
        assert_eq!(or_all, 0x0F);
    }

    #[test]
    fn test_states_dense_0_to_3() {
        let s = [
            COMPONENT_STATE_UNBOUND,
            COMPONENT_STATE_BINDING,
            COMPONENT_STATE_BOUND,
            COMPONENT_STATE_UNBINDING,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_max_per_master_is_16() {
        assert_eq!(COMPONENT_MAX_PER_MASTER, 16);
        assert!(COMPONENT_MAX_PER_MASTER.is_power_of_two());
    }

    #[test]
    fn test_sysfs_paths_well_formed() {
        assert!(COMPONENT_SYSFS_ROOT.starts_with("/sys/"));
        assert_eq!(COMPONENT_SYSFS_ROOT, "/sys/devices");
        assert_eq!(COMPONENT_SYSFS_DRIVER_LINK, "driver");
    }

    #[test]
    fn test_state_transitions_form_cycle() {
        // unbound -> binding -> bound -> unbinding -> unbound
        // Each state is the predecessor + 1 mod 4.
        let cycle = [
            COMPONENT_STATE_UNBOUND,
            COMPONENT_STATE_BINDING,
            COMPONENT_STATE_BOUND,
            COMPONENT_STATE_UNBINDING,
        ];
        for i in 0..cycle.len() {
            let next = (cycle[i] + 1) % 4;
            assert_eq!(next, cycle[(i + 1) % cycle.len()]);
        }
    }
}
