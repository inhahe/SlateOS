//! `<linux/ila.h>` — Additional ILA (Identifier-Locator Addressing) constants.
//!
//! Supplementary ILA constants covering address types,
//! lookup modes, and csum modes.

// ---------------------------------------------------------------------------
// ILA identifier types
// ---------------------------------------------------------------------------

/// ILA locator match type.
pub const ILA_LOCATOR_MATCH_LOCATOR: u32 = 0;
/// ILA identifier type: locator hash.
pub const ILA_LOCATOR_MATCH_IFINDEX: u32 = 1;

// ---------------------------------------------------------------------------
// ILA command types
// ---------------------------------------------------------------------------

/// No operation.
pub const ILA_CMD_UNSPEC: u32 = 0;
/// Add ILA mapping.
pub const ILA_CMD_ADD: u32 = 1;
/// Delete ILA mapping.
pub const ILA_CMD_DEL: u32 = 2;
/// Get ILA mapping.
pub const ILA_CMD_GET: u32 = 3;
/// Flush all ILA mappings.
pub const ILA_CMD_FLUSH: u32 = 4;

// ---------------------------------------------------------------------------
// ILA checksum modes
// ---------------------------------------------------------------------------

/// No checksum adjust.
pub const ILA_CSUM_ADJUST_NONE: u32 = 0;
/// Transport layer checksum adjust.
pub const ILA_CSUM_ADJUST_TRANSPORT: u32 = 1;
/// Neutral checksum adjust.
pub const ILA_CSUM_ADJUST_NEUTRAL_MAP: u32 = 2;
/// Auto neutral map.
pub const ILA_CSUM_ADJUST_NEUTRAL_MAP_AUTO: u32 = 3;

// ---------------------------------------------------------------------------
// ILA hook types
// ---------------------------------------------------------------------------

/// Input hook.
pub const ILA_HOOK_ROUTE_INPUT: u32 = 0;
/// Output hook.
pub const ILA_HOOK_ROUTE_OUTPUT: u32 = 1;

// ---------------------------------------------------------------------------
// ILA direction
// ---------------------------------------------------------------------------

/// Direction in (locator to identifier).
pub const ILA_DIR_IN: u32 = 0;
/// Direction out (identifier to locator).
pub const ILA_DIR_OUT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locator_match_distinct() {
        assert_ne!(ILA_LOCATOR_MATCH_LOCATOR, ILA_LOCATOR_MATCH_IFINDEX);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            ILA_CMD_UNSPEC,
            ILA_CMD_ADD,
            ILA_CMD_DEL,
            ILA_CMD_GET,
            ILA_CMD_FLUSH,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_csum_modes_distinct() {
        let modes = [
            ILA_CSUM_ADJUST_NONE,
            ILA_CSUM_ADJUST_TRANSPORT,
            ILA_CSUM_ADJUST_NEUTRAL_MAP,
            ILA_CSUM_ADJUST_NEUTRAL_MAP_AUTO,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_hooks_distinct() {
        assert_ne!(ILA_HOOK_ROUTE_INPUT, ILA_HOOK_ROUTE_OUTPUT);
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(ILA_DIR_IN, ILA_DIR_OUT);
    }
}
