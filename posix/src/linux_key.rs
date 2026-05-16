//! `<linux/keyctl.h>` — Linux key management facility constants.
//!
//! The kernel keyring subsystem manages cryptographic keys, authentication
//! tokens, and other security data. Used by `keyctl`, `request-key`,
//! dm-crypt, ecryptfs, NFS, and CIFS.

// ---------------------------------------------------------------------------
// Key types (special serial numbers)
// ---------------------------------------------------------------------------

/// Thread keyring.
pub const KEY_SPEC_THREAD_KEYRING: i32 = -1;
/// Process keyring.
pub const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
/// Session keyring.
pub const KEY_SPEC_SESSION_KEYRING: i32 = -3;
/// User keyring.
pub const KEY_SPEC_USER_KEYRING: i32 = -4;
/// User session keyring.
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
/// Group keyring.
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
/// Requestor keyring.
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;

// ---------------------------------------------------------------------------
// keyctl commands
// ---------------------------------------------------------------------------

/// Get keyring ID.
pub const KEYCTL_GET_KEYRING_ID: u32 = 0;
/// Join session keyring.
pub const KEYCTL_JOIN_SESSION_KEYRING: u32 = 1;
/// Update key.
pub const KEYCTL_UPDATE: u32 = 2;
/// Revoke key.
pub const KEYCTL_REVOKE: u32 = 3;
/// Change key ownership.
pub const KEYCTL_CHOWN: u32 = 4;
/// Set key permissions.
pub const KEYCTL_SETPERM: u32 = 5;
/// Describe key.
pub const KEYCTL_DESCRIBE: u32 = 6;
/// Clear keyring.
pub const KEYCTL_CLEAR: u32 = 7;
/// Link key to keyring.
pub const KEYCTL_LINK: u32 = 8;
/// Unlink key from keyring.
pub const KEYCTL_UNLINK: u32 = 9;
/// Search keyring.
pub const KEYCTL_SEARCH: u32 = 10;
/// Read key.
pub const KEYCTL_READ: u32 = 11;
/// Instantiate key.
pub const KEYCTL_INSTANTIATE: u32 = 12;
/// Negate key.
pub const KEYCTL_NEGATE: u32 = 13;
/// Set timeout.
pub const KEYCTL_SET_TIMEOUT: u32 = 15;
/// Assume authority.
pub const KEYCTL_ASSUME_AUTHORITY: u32 = 16;
/// Get security label.
pub const KEYCTL_GET_SECURITY: u32 = 17;
/// Set key restriction.
pub const KEYCTL_RESTRICT_KEYRING: u32 = 29;

// ---------------------------------------------------------------------------
// Key permissions
// ---------------------------------------------------------------------------

/// Possessor can view.
pub const KEY_POS_VIEW: u32 = 0x01000000;
/// Possessor can read.
pub const KEY_POS_READ: u32 = 0x02000000;
/// Possessor can write.
pub const KEY_POS_WRITE: u32 = 0x04000000;
/// Possessor can search.
pub const KEY_POS_SEARCH: u32 = 0x08000000;
/// Possessor can link.
pub const KEY_POS_LINK: u32 = 0x10000000;
/// Possessor can set attributes.
pub const KEY_POS_SETATTR: u32 = 0x20000000;
/// All possessor permissions.
pub const KEY_POS_ALL: u32 = 0x3F000000;

/// User can view.
pub const KEY_USR_VIEW: u32 = 0x00010000;
/// User can read.
pub const KEY_USR_READ: u32 = 0x00020000;
/// User can write.
pub const KEY_USR_WRITE: u32 = 0x00040000;
/// User can search.
pub const KEY_USR_SEARCH: u32 = 0x00080000;
/// User can link.
pub const KEY_USR_LINK: u32 = 0x00100000;
/// All user permissions.
pub const KEY_USR_ALL: u32 = 0x001F0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_specs_distinct() {
        let specs = [
            KEY_SPEC_THREAD_KEYRING, KEY_SPEC_PROCESS_KEYRING,
            KEY_SPEC_SESSION_KEYRING, KEY_SPEC_USER_KEYRING,
            KEY_SPEC_USER_SESSION_KEYRING, KEY_SPEC_GROUP_KEYRING,
            KEY_SPEC_REQKEY_AUTH_KEY,
        ];
        for i in 0..specs.len() {
            for j in (i + 1)..specs.len() {
                assert_ne!(specs[i], specs[j]);
            }
        }
    }

    #[test]
    fn test_keyctl_cmds_distinct() {
        let cmds = [
            KEYCTL_GET_KEYRING_ID, KEYCTL_JOIN_SESSION_KEYRING,
            KEYCTL_UPDATE, KEYCTL_REVOKE, KEYCTL_CHOWN,
            KEYCTL_SETPERM, KEYCTL_DESCRIBE, KEYCTL_CLEAR,
            KEYCTL_LINK, KEYCTL_UNLINK, KEYCTL_SEARCH,
            KEYCTL_READ, KEYCTL_INSTANTIATE, KEYCTL_NEGATE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_permissions_powers_of_two() {
        let perms = [
            KEY_POS_VIEW, KEY_POS_READ, KEY_POS_WRITE,
            KEY_POS_SEARCH, KEY_POS_LINK, KEY_POS_SETATTR,
        ];
        for p in &perms {
            assert!(p.is_power_of_two(), "perm {p:#x} not power of 2");
        }
    }

    #[test]
    fn test_pos_all_mask() {
        assert_eq!(
            KEY_POS_ALL,
            KEY_POS_VIEW | KEY_POS_READ | KEY_POS_WRITE
                | KEY_POS_SEARCH | KEY_POS_LINK | KEY_POS_SETATTR
        );
    }
}
