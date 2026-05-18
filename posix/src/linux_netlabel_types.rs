//! `<linux/netlabel.h>` — NetLabel CIPSO/CALIPSO constants.
//!
//! NetLabel subsystem constants covering command types,
//! attribute types, domain mapping types, and CIPSO tags.

// ---------------------------------------------------------------------------
// NetLabel generic netlink commands (NLBL_*)
// ---------------------------------------------------------------------------

/// Management: list protocols.
pub const NLBL_MGMT_C_ADD: u32 = 0;
/// Management: remove mapping.
pub const NLBL_MGMT_C_REMOVE: u32 = 1;
/// Management: list mappings.
pub const NLBL_MGMT_C_LISTALL: u32 = 2;
/// Management: add default.
pub const NLBL_MGMT_C_ADDDEF: u32 = 3;
/// Management: remove default.
pub const NLBL_MGMT_C_REMOVEDEF: u32 = 4;
/// Management: list defaults.
pub const NLBL_MGMT_C_LISTDEF: u32 = 5;
/// Management: get protocol versions.
pub const NLBL_MGMT_C_PROTOCOLS: u32 = 6;
/// Management: version.
pub const NLBL_MGMT_C_VERSION: u32 = 7;

// ---------------------------------------------------------------------------
// NetLabel CIPSO/DOI commands
// ---------------------------------------------------------------------------

/// CIPSO: add DOI.
pub const NLBL_CIPSOV4_C_ADD: u32 = 0;
/// CIPSO: remove DOI.
pub const NLBL_CIPSOV4_C_REMOVE: u32 = 1;
/// CIPSO: list DOI.
pub const NLBL_CIPSOV4_C_LIST: u32 = 2;
/// CIPSO: list all DOIs.
pub const NLBL_CIPSOV4_C_LISTALL: u32 = 3;

// ---------------------------------------------------------------------------
// NetLabel CALIPSO commands
// ---------------------------------------------------------------------------

/// CALIPSO: add DOI.
pub const NLBL_CALIPSO_C_ADD: u32 = 0;
/// CALIPSO: remove DOI.
pub const NLBL_CALIPSO_C_REMOVE: u32 = 1;
/// CALIPSO: list DOI.
pub const NLBL_CALIPSO_C_LIST: u32 = 2;
/// CALIPSO: list all DOIs.
pub const NLBL_CALIPSO_C_LISTALL: u32 = 3;

// ---------------------------------------------------------------------------
// NetLabel domain mapping types
// ---------------------------------------------------------------------------

/// Default protocol.
pub const NETLBL_NLTYPE_NONE: u32 = 0;
/// CIPSO v4.
pub const NETLBL_NLTYPE_CIPSOV4: u32 = 1;
/// Unlabeled.
pub const NETLBL_NLTYPE_UNLABELED: u32 = 2;
/// CALIPSO.
pub const NETLBL_NLTYPE_CALIPSO: u32 = 4;

// ---------------------------------------------------------------------------
// CIPSO v4 tag types
// ---------------------------------------------------------------------------

/// Standard tag (type 1).
pub const CIPSO_V4_TAG_STANDARD: u8 = 1;
/// Enumerated tag (type 2).
pub const CIPSO_V4_TAG_ENUM: u8 = 2;
/// Range tag (type 5).
pub const CIPSO_V4_TAG_RANGE: u8 = 5;
/// Permissive tag (type 6).
pub const CIPSO_V4_TAG_PERM: u8 = 6;
/// Local tag (type 128).
pub const CIPSO_V4_TAG_LOCAL: u8 = 128;
/// Invalid tag.
pub const CIPSO_V4_TAG_INVALID: u8 = 0;

// ---------------------------------------------------------------------------
// CIPSO v4 DOI types
// ---------------------------------------------------------------------------

/// Standard DOI type.
pub const CIPSO_V4_MAP_TRANS: u32 = 0;
/// Pass-through DOI type.
pub const CIPSO_V4_MAP_PASS: u32 = 1;
/// Local DOI type.
pub const CIPSO_V4_MAP_LOCAL: u32 = 2;

// ---------------------------------------------------------------------------
// CIPSO v4 limits
// ---------------------------------------------------------------------------

/// Maximum level.
pub const CIPSO_V4_MAX_LEVEL: u32 = 255;
/// Maximum number of categories.
pub const CIPSO_V4_MAX_CATNUM: u32 = 239;
/// Maximum tag length.
pub const CIPSO_V4_MAX_TAG_LEN: u32 = 34;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mgmt_commands_distinct() {
        let cmds = [
            NLBL_MGMT_C_ADD, NLBL_MGMT_C_REMOVE, NLBL_MGMT_C_LISTALL,
            NLBL_MGMT_C_ADDDEF, NLBL_MGMT_C_REMOVEDEF,
            NLBL_MGMT_C_LISTDEF, NLBL_MGMT_C_PROTOCOLS,
            NLBL_MGMT_C_VERSION,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_cipso_commands_distinct() {
        let cmds = [
            NLBL_CIPSOV4_C_ADD, NLBL_CIPSOV4_C_REMOVE,
            NLBL_CIPSOV4_C_LIST, NLBL_CIPSOV4_C_LISTALL,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_nltype_distinct() {
        let types = [
            NETLBL_NLTYPE_NONE, NETLBL_NLTYPE_CIPSOV4,
            NETLBL_NLTYPE_UNLABELED, NETLBL_NLTYPE_CALIPSO,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_cipso_tags_distinct() {
        let tags: [u8; 6] = [
            CIPSO_V4_TAG_STANDARD, CIPSO_V4_TAG_ENUM,
            CIPSO_V4_TAG_RANGE, CIPSO_V4_TAG_PERM,
            CIPSO_V4_TAG_LOCAL, CIPSO_V4_TAG_INVALID,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_doi_types_distinct() {
        let types = [
            CIPSO_V4_MAP_TRANS, CIPSO_V4_MAP_PASS, CIPSO_V4_MAP_LOCAL,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert_eq!(CIPSO_V4_MAX_LEVEL, 255);
        assert!(CIPSO_V4_MAX_CATNUM < CIPSO_V4_MAX_LEVEL);
        assert!(CIPSO_V4_MAX_TAG_LEN > 0);
    }
}
