//! `<linux/ila.h>` — Identifier-Locator Addressing (ILA) constants.
//!
//! ILA is an IPv6 network virtualization technique that splits an
//! address into an identifier (who) and a locator (where). The
//! identifier is a stable 64-bit value tied to the workload; the
//! locator is a 64-bit network prefix that changes as the workload
//! moves. ILA translates between SIR (Standard Identifier
//! Representation) addresses used by applications and ILA addresses
//! used on the wire. Used in data center networking for mobility,
//! multi-homing, and address virtualization.

// ---------------------------------------------------------------------------
// ILA netlink commands
// ---------------------------------------------------------------------------

/// Get ILA mapping.
pub const ILA_CMD_GET: u32 = 1;
/// Add ILA mapping.
pub const ILA_CMD_ADD: u32 = 2;
/// Delete ILA mapping.
pub const ILA_CMD_DEL: u32 = 3;
/// Flush all ILA mappings.
pub const ILA_CMD_FLUSH: u32 = 4;

// ---------------------------------------------------------------------------
// ILA netlink attributes
// ---------------------------------------------------------------------------

/// Locator (64-bit network prefix) attribute.
pub const ILA_ATTR_LOCATOR: u32 = 1;
/// Identifier (64-bit workload ID) attribute.
pub const ILA_ATTR_IDENTIFIER: u32 = 2;
/// Locator match (for lookup) attribute.
pub const ILA_ATTR_LOCATOR_MATCH: u32 = 3;
/// Interface index attribute.
pub const ILA_ATTR_IFINDEX: u32 = 4;
/// Direction (input/output) attribute.
pub const ILA_ATTR_DIR: u32 = 5;
/// ILA type (encoding) attribute.
pub const ILA_ATTR_CSUM_MODE: u32 = 6;
/// ILA identifier type attribute.
pub const ILA_ATTR_IDENT_TYPE: u32 = 7;
/// Hook type attribute.
pub const ILA_ATTR_HOOK_TYPE: u32 = 8;

// ---------------------------------------------------------------------------
// ILA checksum modes
// ---------------------------------------------------------------------------

/// No checksum adjustment.
pub const ILA_CSUM_NO_ACTION: u32 = 0;
/// Adjust transport checksum (translate).
pub const ILA_CSUM_ADJUST_TRANSPORT: u32 = 1;
/// Use neutral-map checksum encoding.
pub const ILA_CSUM_NEUTRAL_MAP: u32 = 2;
/// Use neutral-map with auto-detection.
pub const ILA_CSUM_NEUTRAL_MAP_AUTO: u32 = 3;

// ---------------------------------------------------------------------------
// ILA identifier types
// ---------------------------------------------------------------------------

/// IID (Interface Identifier) — lower 64 bits.
pub const ILA_ATYPE_IID: u32 = 0;
/// LUID (Locally Unique Identifier).
pub const ILA_ATYPE_LUID: u32 = 1;
/// Virtual networking identifier (VNET ID).
pub const ILA_ATYPE_VNET_ID: u32 = 2;

// ---------------------------------------------------------------------------
// ILA hook types (where translation happens)
// ---------------------------------------------------------------------------

/// Translate in LWT (lightweight tunnel) input hook.
pub const ILA_HOOK_ROUTE_INPUT: u32 = 0;
/// Translate in LWT output hook.
pub const ILA_HOOK_ROUTE_OUTPUT: u32 = 1;

// ---------------------------------------------------------------------------
// ILA direction
// ---------------------------------------------------------------------------

/// Input direction (wire → host).
pub const ILA_DIR_IN: u32 = 0;
/// Output direction (host → wire).
pub const ILA_DIR_OUT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [ILA_CMD_GET, ILA_CMD_ADD, ILA_CMD_DEL, ILA_CMD_FLUSH];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            ILA_ATTR_LOCATOR, ILA_ATTR_IDENTIFIER,
            ILA_ATTR_LOCATOR_MATCH, ILA_ATTR_IFINDEX,
            ILA_ATTR_DIR, ILA_ATTR_CSUM_MODE,
            ILA_ATTR_IDENT_TYPE, ILA_ATTR_HOOK_TYPE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_csum_modes_distinct() {
        let modes = [
            ILA_CSUM_NO_ACTION, ILA_CSUM_ADJUST_TRANSPORT,
            ILA_CSUM_NEUTRAL_MAP, ILA_CSUM_NEUTRAL_MAP_AUTO,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_ident_types_distinct() {
        let types = [ILA_ATYPE_IID, ILA_ATYPE_LUID, ILA_ATYPE_VNET_ID];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_hook_types_distinct() {
        assert_ne!(ILA_HOOK_ROUTE_INPUT, ILA_HOOK_ROUTE_OUTPUT);
    }

    #[test]
    fn test_directions_distinct() {
        assert_ne!(ILA_DIR_IN, ILA_DIR_OUT);
    }
}
