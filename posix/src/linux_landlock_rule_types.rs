//! `<linux/landlock.h>` — Landlock ruleset and rule type constants.
//!
//! A Landlock ruleset is a collection of rules that restrict access
//! rights. Rules are added by type (filesystem path, network port)
//! and enforce restrictions when the ruleset is applied to the
//! calling thread.

// ---------------------------------------------------------------------------
// Landlock ruleset creation flags
// ---------------------------------------------------------------------------

/// Ruleset attribute version 1 (handled_access_fs).
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Landlock rule types (landlock_rule_type)
// ---------------------------------------------------------------------------

/// Rule applies to a filesystem path.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Rule applies to a network port.
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Landlock restrict_self flags
// ---------------------------------------------------------------------------

/// No flags defined yet (must be 0).
pub const LANDLOCK_RESTRICT_SELF_FLAGS_NONE: u32 = 0;

// ---------------------------------------------------------------------------
// Landlock ABI versions and handled_access masks
// ---------------------------------------------------------------------------

/// ABI version 1: original fs access rights.
pub const LANDLOCK_ABI_V1: u32 = 1;
/// ABI version 2: added REFER.
pub const LANDLOCK_ABI_V2: u32 = 2;
/// ABI version 3: added TRUNCATE.
pub const LANDLOCK_ABI_V3: u32 = 3;
/// ABI version 4: added NET_BIND/CONNECT.
pub const LANDLOCK_ABI_V4: u32 = 4;
/// ABI version 5: added IOCTL_DEV.
pub const LANDLOCK_ABI_V5: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rule_types_distinct() {
        assert_ne!(LANDLOCK_RULE_PATH_BENEATH, LANDLOCK_RULE_NET_PORT);
    }

    #[test]
    fn test_rule_path_is_one() {
        assert_eq!(LANDLOCK_RULE_PATH_BENEATH, 1);
    }

    #[test]
    fn test_abi_versions_sequential() {
        assert_eq!(LANDLOCK_ABI_V1, 1);
        assert_eq!(LANDLOCK_ABI_V2, 2);
        assert_eq!(LANDLOCK_ABI_V3, 3);
        assert_eq!(LANDLOCK_ABI_V4, 4);
        assert_eq!(LANDLOCK_ABI_V5, 5);
    }

    #[test]
    fn test_abi_versions_distinct() {
        let versions = [
            LANDLOCK_ABI_V1, LANDLOCK_ABI_V2, LANDLOCK_ABI_V3,
            LANDLOCK_ABI_V4, LANDLOCK_ABI_V5,
        ];
        for i in 0..versions.len() {
            for j in (i + 1)..versions.len() {
                assert_ne!(versions[i], versions[j]);
            }
        }
    }

    #[test]
    fn test_restrict_self_flags() {
        assert_eq!(LANDLOCK_RESTRICT_SELF_FLAGS_NONE, 0);
    }

    #[test]
    fn test_create_ruleset_version_flag() {
        assert_eq!(LANDLOCK_CREATE_RULESET_VERSION, 1);
    }
}
