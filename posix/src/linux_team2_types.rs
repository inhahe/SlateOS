//! `<linux/if_team.h>` — Additional team device constants.
//!
//! Supplementary team device constants covering team modes,
//! port change types, and option types.

// ---------------------------------------------------------------------------
// Team modes
// ---------------------------------------------------------------------------

/// Round-robin mode.
pub const TEAM_MODE_ROUNDROBIN: u32 = 0;
/// Active-backup mode.
pub const TEAM_MODE_ACTIVEBACKUP: u32 = 1;
/// Loadbalance mode.
pub const TEAM_MODE_LOADBALANCE: u32 = 2;
/// Broadcast mode.
pub const TEAM_MODE_BROADCAST: u32 = 3;
/// Random mode.
pub const TEAM_MODE_RANDOM: u32 = 4;

// ---------------------------------------------------------------------------
// Team port change types
// ---------------------------------------------------------------------------

/// Port added.
pub const TEAM_PORT_CHANGE_ADDED: u32 = 0;
/// Port removed.
pub const TEAM_PORT_CHANGE_REMOVED: u32 = 1;
/// Port link state changed.
pub const TEAM_PORT_CHANGE_LINKUP: u32 = 2;
/// Port link down.
pub const TEAM_PORT_CHANGE_LINKDOWN: u32 = 3;

// ---------------------------------------------------------------------------
// Team option types
// ---------------------------------------------------------------------------

/// Option type: unsigned 32-bit.
pub const TEAM_OPTION_TYPE_U32: u32 = 0;
/// Option type: string.
pub const TEAM_OPTION_TYPE_STRING: u32 = 1;
/// Option type: binary data.
pub const TEAM_OPTION_TYPE_BINARY: u32 = 2;
/// Option type: boolean.
pub const TEAM_OPTION_TYPE_BOOL: u32 = 3;
/// Option type: signed 32-bit.
pub const TEAM_OPTION_TYPE_S32: u32 = 4;

// ---------------------------------------------------------------------------
// Team genetlink command IDs
// ---------------------------------------------------------------------------

/// No operation.
pub const TEAM_CMD_NOOP: u32 = 0;
/// Get options.
pub const TEAM_CMD_OPTIONS_GET: u32 = 1;
/// Set options.
pub const TEAM_CMD_OPTIONS_SET: u32 = 2;
/// Get port list.
pub const TEAM_CMD_PORT_LIST_GET: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            TEAM_MODE_ROUNDROBIN, TEAM_MODE_ACTIVEBACKUP,
            TEAM_MODE_LOADBALANCE, TEAM_MODE_BROADCAST,
            TEAM_MODE_RANDOM,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_port_changes_distinct() {
        let changes = [
            TEAM_PORT_CHANGE_ADDED, TEAM_PORT_CHANGE_REMOVED,
            TEAM_PORT_CHANGE_LINKUP, TEAM_PORT_CHANGE_LINKDOWN,
        ];
        for i in 0..changes.len() {
            for j in (i + 1)..changes.len() {
                assert_ne!(changes[i], changes[j]);
            }
        }
    }

    #[test]
    fn test_option_types_distinct() {
        let types = [
            TEAM_OPTION_TYPE_U32, TEAM_OPTION_TYPE_STRING,
            TEAM_OPTION_TYPE_BINARY, TEAM_OPTION_TYPE_BOOL,
            TEAM_OPTION_TYPE_S32,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            TEAM_CMD_NOOP, TEAM_CMD_OPTIONS_GET,
            TEAM_CMD_OPTIONS_SET, TEAM_CMD_PORT_LIST_GET,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }
}
