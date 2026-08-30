//! `<linux/devlink.h>` — Devlink (device management) netlink constants.
//!
//! Devlink is the infrastructure for managing physical network devices
//! and their resources via netlink. It exposes device-level operations
//! (firmware flashing, health reporting, port splitting, resource
//! tuning, trap configuration) that don't fit into the per-netdev
//! model. Used by `devlink` CLI tool (iproute2) for managing NICs,
//! SmartNICs, and network switch ASICs from vendors like Mellanox,
//! Intel, Broadcom, and Marvell.

// ---------------------------------------------------------------------------
// Devlink netlink commands
// ---------------------------------------------------------------------------

/// Get devlink device info.
pub const DEVLINK_CMD_GET: u32 = 1;
/// Set devlink device parameters.
pub const DEVLINK_CMD_SET: u32 = 2;
/// Get devlink port info.
pub const DEVLINK_CMD_PORT_GET: u32 = 5;
/// Set devlink port parameters.
pub const DEVLINK_CMD_PORT_SET: u32 = 6;
/// Split a port into multiple sub-ports.
pub const DEVLINK_CMD_PORT_SPLIT: u32 = 9;
/// Unsplit a previously split port.
pub const DEVLINK_CMD_PORT_UNSPLIT: u32 = 10;
/// Get shared buffer info.
pub const DEVLINK_CMD_SB_GET: u32 = 11;
/// Get device parameters.
pub const DEVLINK_CMD_PARAM_GET: u32 = 38;
/// Set device parameters.
pub const DEVLINK_CMD_PARAM_SET: u32 = 39;
/// Get health reporter info.
pub const DEVLINK_CMD_HEALTH_REPORTER_GET: u32 = 52;
/// Set health reporter parameters.
pub const DEVLINK_CMD_HEALTH_REPORTER_SET: u32 = 53;
/// Recover from health reporter error.
pub const DEVLINK_CMD_HEALTH_REPORTER_RECOVER: u32 = 54;
/// Diagnose via health reporter.
pub const DEVLINK_CMD_HEALTH_REPORTER_DIAGNOSE: u32 = 55;
/// Dump health reporter data.
pub const DEVLINK_CMD_HEALTH_REPORTER_DUMP_GET: u32 = 56;
/// Flash firmware update.
pub const DEVLINK_CMD_FLASH_UPDATE: u32 = 58;
/// Get trap info.
pub const DEVLINK_CMD_TRAP_GET: u32 = 61;
/// Set trap action.
pub const DEVLINK_CMD_TRAP_SET: u32 = 62;
/// Get trap group info.
pub const DEVLINK_CMD_TRAP_GROUP_GET: u32 = 63;
/// Set trap group parameters.
pub const DEVLINK_CMD_TRAP_GROUP_SET: u32 = 64;
/// Get rate (tx bandwidth) info.
pub const DEVLINK_CMD_RATE_GET: u32 = 76;
/// Set rate parameters.
pub const DEVLINK_CMD_RATE_SET: u32 = 77;
/// Get device selftests info.
pub const DEVLINK_CMD_SELFTESTS_GET: u32 = 82;
/// Run device selftests.
pub const DEVLINK_CMD_SELFTESTS_RUN: u32 = 83;

// ---------------------------------------------------------------------------
// Port types
// ---------------------------------------------------------------------------

/// Port type not set.
pub const DEVLINK_PORT_TYPE_NOTSET: u32 = 0;
/// Ethernet port type.
pub const DEVLINK_PORT_TYPE_ETH: u32 = 1;
/// InfiniBand port type.
pub const DEVLINK_PORT_TYPE_IB: u32 = 2;

// ---------------------------------------------------------------------------
// Port flavours
// ---------------------------------------------------------------------------

/// Physical port.
pub const DEVLINK_PORT_FLAVOUR_PHYSICAL: u32 = 0;
/// CPU port (control plane).
pub const DEVLINK_PORT_FLAVOUR_CPU: u32 = 1;
/// DSA (Distributed Switch Architecture) port.
pub const DEVLINK_PORT_FLAVOUR_DSA: u32 = 2;
/// PCI PF (Physical Function) port.
pub const DEVLINK_PORT_FLAVOUR_PCI_PF: u32 = 3;
/// PCI VF (Virtual Function) port.
pub const DEVLINK_PORT_FLAVOUR_PCI_VF: u32 = 4;
/// Virtual port.
pub const DEVLINK_PORT_FLAVOUR_VIRTUAL: u32 = 5;
/// Unused/reserved port.
pub const DEVLINK_PORT_FLAVOUR_UNUSED: u32 = 6;
/// PCI SF (Sub-Function) port.
pub const DEVLINK_PORT_FLAVOUR_PCI_SF: u32 = 7;

// ---------------------------------------------------------------------------
// eswitch mode
// ---------------------------------------------------------------------------

/// Legacy (non-switchdev) eswitch mode.
pub const DEVLINK_ESWITCH_MODE_LEGACY: u32 = 0;
/// Switchdev (offload) eswitch mode.
pub const DEVLINK_ESWITCH_MODE_SWITCHDEV: u32 = 1;

// ---------------------------------------------------------------------------
// Trap actions
// ---------------------------------------------------------------------------

/// Drop the trapped packet.
pub const DEVLINK_TRAP_ACTION_DROP: u32 = 0;
/// Deliver the trapped packet to CPU.
pub const DEVLINK_TRAP_ACTION_TRAP: u32 = 1;
/// Mirror the trapped packet (copy to CPU, forward original).
pub const DEVLINK_TRAP_ACTION_MIRROR: u32 = 2;

// ---------------------------------------------------------------------------
// Trap types
// ---------------------------------------------------------------------------

/// Drop trap (exception causes packet drop).
pub const DEVLINK_TRAP_TYPE_DROP: u32 = 0;
/// Exception trap (unusual condition).
pub const DEVLINK_TRAP_TYPE_EXCEPTION: u32 = 1;
/// Control trap (protocol control packet).
pub const DEVLINK_TRAP_TYPE_CONTROL: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            DEVLINK_CMD_GET,
            DEVLINK_CMD_SET,
            DEVLINK_CMD_PORT_GET,
            DEVLINK_CMD_PORT_SET,
            DEVLINK_CMD_PORT_SPLIT,
            DEVLINK_CMD_PORT_UNSPLIT,
            DEVLINK_CMD_SB_GET,
            DEVLINK_CMD_PARAM_GET,
            DEVLINK_CMD_PARAM_SET,
            DEVLINK_CMD_HEALTH_REPORTER_GET,
            DEVLINK_CMD_HEALTH_REPORTER_SET,
            DEVLINK_CMD_HEALTH_REPORTER_RECOVER,
            DEVLINK_CMD_HEALTH_REPORTER_DIAGNOSE,
            DEVLINK_CMD_HEALTH_REPORTER_DUMP_GET,
            DEVLINK_CMD_FLASH_UPDATE,
            DEVLINK_CMD_TRAP_GET,
            DEVLINK_CMD_TRAP_SET,
            DEVLINK_CMD_TRAP_GROUP_GET,
            DEVLINK_CMD_TRAP_GROUP_SET,
            DEVLINK_CMD_RATE_GET,
            DEVLINK_CMD_RATE_SET,
            DEVLINK_CMD_SELFTESTS_GET,
            DEVLINK_CMD_SELFTESTS_RUN,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_port_types_distinct() {
        let types = [
            DEVLINK_PORT_TYPE_NOTSET,
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
    fn test_eswitch_modes_distinct() {
        assert_ne!(DEVLINK_ESWITCH_MODE_LEGACY, DEVLINK_ESWITCH_MODE_SWITCHDEV);
    }

    #[test]
    fn test_trap_actions_distinct() {
        let actions = [
            DEVLINK_TRAP_ACTION_DROP,
            DEVLINK_TRAP_ACTION_TRAP,
            DEVLINK_TRAP_ACTION_MIRROR,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_trap_types_distinct() {
        let types = [
            DEVLINK_TRAP_TYPE_DROP,
            DEVLINK_TRAP_TYPE_EXCEPTION,
            DEVLINK_TRAP_TYPE_CONTROL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
