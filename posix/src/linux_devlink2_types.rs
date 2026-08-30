//! `<linux/devlink.h>` — Additional devlink constants.
//!
//! Supplementary devlink constants covering port types,
//! health reporter states, and reload actions.

// ---------------------------------------------------------------------------
// Devlink port types
// ---------------------------------------------------------------------------

/// Not set.
pub const DEVLINK_PORT_TYPE_NOTSET: u32 = 0;
/// Auto (determined by hardware).
pub const DEVLINK_PORT_TYPE_AUTO: u32 = 1;
/// Ethernet port.
pub const DEVLINK_PORT_TYPE_ETH: u32 = 2;
/// InfiniBand port.
pub const DEVLINK_PORT_TYPE_IB: u32 = 3;

// ---------------------------------------------------------------------------
// Devlink port flavour
// ---------------------------------------------------------------------------

/// Physical port.
pub const DEVLINK_PORT_FLAVOUR_PHYSICAL: u32 = 0;
/// CPU port.
pub const DEVLINK_PORT_FLAVOUR_CPU: u32 = 1;
/// DSA (Distributed Switch Architecture) port.
pub const DEVLINK_PORT_FLAVOUR_DSA: u32 = 2;
/// PCI PF (Physical Function) port.
pub const DEVLINK_PORT_FLAVOUR_PCI_PF: u32 = 3;
/// PCI VF (Virtual Function) port.
pub const DEVLINK_PORT_FLAVOUR_PCI_VF: u32 = 4;
/// Virtual port.
pub const DEVLINK_PORT_FLAVOUR_VIRTUAL: u32 = 5;
/// Unused port.
pub const DEVLINK_PORT_FLAVOUR_UNUSED: u32 = 6;
/// PCI SF (Sub-Function) port.
pub const DEVLINK_PORT_FLAVOUR_PCI_SF: u32 = 7;

// ---------------------------------------------------------------------------
// Devlink health reporter state
// ---------------------------------------------------------------------------

/// Healthy.
pub const DEVLINK_HEALTH_REPORTER_STATE_HEALTHY: u32 = 0;
/// Error.
pub const DEVLINK_HEALTH_REPORTER_STATE_ERROR: u32 = 1;

// ---------------------------------------------------------------------------
// Devlink reload actions
// ---------------------------------------------------------------------------

/// Unspec reload.
pub const DEVLINK_RELOAD_ACTION_UNSPEC: u32 = 0;
/// Driver reinit.
pub const DEVLINK_RELOAD_ACTION_DRIVER_REINIT: u32 = 1;
/// Firmware activate.
pub const DEVLINK_RELOAD_ACTION_FW_ACTIVATE: u32 = 2;

// ---------------------------------------------------------------------------
// Devlink eswitch modes
// ---------------------------------------------------------------------------

/// Legacy (non-switchdev) mode.
pub const DEVLINK_ESWITCH_MODE_LEGACY: u32 = 0;
/// Switchdev mode.
pub const DEVLINK_ESWITCH_MODE_SWITCHDEV: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_port_types_distinct() {
        let types = [
            DEVLINK_PORT_TYPE_NOTSET,
            DEVLINK_PORT_TYPE_AUTO,
            DEVLINK_PORT_TYPE_ETH,
            DEVLINK_PORT_TYPE_IB,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_port_flavours_distinct() {
        let flavours = [
            DEVLINK_PORT_FLAVOUR_PHYSICAL,
            DEVLINK_PORT_FLAVOUR_CPU,
            DEVLINK_PORT_FLAVOUR_DSA,
            DEVLINK_PORT_FLAVOUR_PCI_PF,
            DEVLINK_PORT_FLAVOUR_PCI_VF,
            DEVLINK_PORT_FLAVOUR_VIRTUAL,
            DEVLINK_PORT_FLAVOUR_UNUSED,
            DEVLINK_PORT_FLAVOUR_PCI_SF,
        ];
        for i in 0..flavours.len() {
            for j in (i + 1)..flavours.len() {
                assert_ne!(flavours[i], flavours[j]);
            }
        }
    }

    #[test]
    fn test_health_states_distinct() {
        assert_ne!(
            DEVLINK_HEALTH_REPORTER_STATE_HEALTHY,
            DEVLINK_HEALTH_REPORTER_STATE_ERROR
        );
    }

    #[test]
    fn test_reload_actions_distinct() {
        let actions = [
            DEVLINK_RELOAD_ACTION_UNSPEC,
            DEVLINK_RELOAD_ACTION_DRIVER_REINIT,
            DEVLINK_RELOAD_ACTION_FW_ACTIVATE,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_eswitch_modes_distinct() {
        assert_ne!(DEVLINK_ESWITCH_MODE_LEGACY, DEVLINK_ESWITCH_MODE_SWITCHDEV);
    }
}
