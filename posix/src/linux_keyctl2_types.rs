//! `<linux/keyctl.h>` — Additional keyctl constants.
//!
//! Supplementary keyctl constants covering key permission bits,
//! key restriction types, and Diffie-Hellman parameter types.

// ---------------------------------------------------------------------------
// Key permissions (KEY_POS_*, KEY_USR_*, KEY_GRP_*, KEY_OTH_*)
// ---------------------------------------------------------------------------

/// Possessor: view key.
pub const KEY_POS_VIEW: u32 = 0x01000000;
/// Possessor: read key.
pub const KEY_POS_READ: u32 = 0x02000000;
/// Possessor: write key.
pub const KEY_POS_WRITE: u32 = 0x04000000;
/// Possessor: search key.
pub const KEY_POS_SEARCH: u32 = 0x08000000;
/// Possessor: link key.
pub const KEY_POS_LINK: u32 = 0x10000000;
/// Possessor: set attribute.
pub const KEY_POS_SETATTR: u32 = 0x20000000;
/// Possessor: all permissions.
pub const KEY_POS_ALL: u32 = 0x3F000000;

/// User: view key.
pub const KEY_USR_VIEW: u32 = 0x00010000;
/// User: read key.
pub const KEY_USR_READ: u32 = 0x00020000;
/// User: write key.
pub const KEY_USR_WRITE: u32 = 0x00040000;
/// User: search key.
pub const KEY_USR_SEARCH: u32 = 0x00080000;
/// User: link key.
pub const KEY_USR_LINK: u32 = 0x00100000;
/// User: set attribute.
pub const KEY_USR_SETATTR: u32 = 0x00200000;
/// User: all permissions.
pub const KEY_USR_ALL: u32 = 0x003F0000;

/// Group: view key.
pub const KEY_GRP_VIEW: u32 = 0x00000100;
/// Group: read key.
pub const KEY_GRP_READ: u32 = 0x00000200;
/// Group: write key.
pub const KEY_GRP_WRITE: u32 = 0x00000400;
/// Group: search key.
pub const KEY_GRP_SEARCH: u32 = 0x00000800;
/// Group: link key.
pub const KEY_GRP_LINK: u32 = 0x00001000;
/// Group: set attribute.
pub const KEY_GRP_SETATTR: u32 = 0x00002000;
/// Group: all permissions.
pub const KEY_GRP_ALL: u32 = 0x00003F00;

/// Other: view key.
pub const KEY_OTH_VIEW: u32 = 0x00000001;
/// Other: read key.
pub const KEY_OTH_READ: u32 = 0x00000002;
/// Other: write key.
pub const KEY_OTH_WRITE: u32 = 0x00000004;
/// Other: search key.
pub const KEY_OTH_SEARCH: u32 = 0x00000008;
/// Other: link key.
pub const KEY_OTH_LINK: u32 = 0x00000010;
/// Other: set attribute.
pub const KEY_OTH_SETATTR: u32 = 0x00000020;
/// Other: all permissions.
pub const KEY_OTH_ALL: u32 = 0x0000003F;

// ---------------------------------------------------------------------------
// Key restriction types
// ---------------------------------------------------------------------------

/// Restrict key by type.
pub const KEY_RESTRICTION_BUILTIN_TRUSTED: u32 = 0;
/// Restrict key by signature.
pub const KEY_RESTRICTION_BUILTIN_AND_SECONDARY_TRUSTED: u32 = 1;
/// Restrict key by asymmetric key type.
pub const KEY_RESTRICTION_KEY_OR_KEYRING: u32 = 2;

// ---------------------------------------------------------------------------
// DH parameter encoding
// ---------------------------------------------------------------------------

/// DH prime (p).
pub const KEYCTL_DH_PRIME: u32 = 0;
/// DH base (g).
pub const KEYCTL_DH_BASE: u32 = 1;
/// DH private (x).
pub const KEYCTL_DH_PRIVATE: u32 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pos_permissions_no_overlap() {
        let perms = [
            KEY_POS_VIEW, KEY_POS_READ, KEY_POS_WRITE,
            KEY_POS_SEARCH, KEY_POS_LINK, KEY_POS_SETATTR,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_pos_all_covers_all() {
        let all = KEY_POS_VIEW | KEY_POS_READ | KEY_POS_WRITE
            | KEY_POS_SEARCH | KEY_POS_LINK | KEY_POS_SETATTR;
        assert_eq!(KEY_POS_ALL, all);
    }

    #[test]
    fn test_usr_all_covers_all() {
        let all = KEY_USR_VIEW | KEY_USR_READ | KEY_USR_WRITE
            | KEY_USR_SEARCH | KEY_USR_LINK | KEY_USR_SETATTR;
        assert_eq!(KEY_USR_ALL, all);
    }

    #[test]
    fn test_grp_all_covers_all() {
        let all = KEY_GRP_VIEW | KEY_GRP_READ | KEY_GRP_WRITE
            | KEY_GRP_SEARCH | KEY_GRP_LINK | KEY_GRP_SETATTR;
        assert_eq!(KEY_GRP_ALL, all);
    }

    #[test]
    fn test_oth_all_covers_all() {
        let all = KEY_OTH_VIEW | KEY_OTH_READ | KEY_OTH_WRITE
            | KEY_OTH_SEARCH | KEY_OTH_LINK | KEY_OTH_SETATTR;
        assert_eq!(KEY_OTH_ALL, all);
    }

    #[test]
    fn test_permission_groups_no_overlap() {
        // Each group occupies a different byte
        assert_eq!(KEY_POS_ALL & KEY_USR_ALL, 0);
        assert_eq!(KEY_USR_ALL & KEY_GRP_ALL, 0);
        assert_eq!(KEY_GRP_ALL & KEY_OTH_ALL, 0);
    }

    #[test]
    fn test_restriction_types_distinct() {
        let types = [
            KEY_RESTRICTION_BUILTIN_TRUSTED,
            KEY_RESTRICTION_BUILTIN_AND_SECONDARY_TRUSTED,
            KEY_RESTRICTION_KEY_OR_KEYRING,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_dh_params_distinct() {
        let params = [KEYCTL_DH_PRIME, KEYCTL_DH_BASE, KEYCTL_DH_PRIVATE];
        for i in 0..params.len() {
            for j in (i + 1)..params.len() {
                assert_ne!(params[i], params[j]);
            }
        }
    }
}
