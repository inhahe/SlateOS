//! `<linux/keyctl.h>` — Kernel keyring service constants.
//!
//! The Linux keyring facility provides in-kernel storage for
//! authentication tokens, encryption keys, and other security data.
//! Keys are organized in keyrings (which are themselves keys) and
//! accessed via the keyctl() syscall. Used by fscrypt, NFS, CIFS,
//! Kerberos, and encrypted disks.

// ---------------------------------------------------------------------------
// Key types (well-known type names)
// ---------------------------------------------------------------------------

/// User key type.
pub const KEY_TYPE_USER: &str = "user";
/// Logon key type (like user, but never readable from userspace).
pub const KEY_TYPE_LOGON: &str = "logon";
/// Keyring type.
pub const KEY_TYPE_KEYRING: &str = "keyring";
/// Big key type (stored in shmem if large).
pub const KEY_TYPE_BIG_KEY: &str = "big_key";
/// Encrypted key type.
pub const KEY_TYPE_ENCRYPTED: &str = "encrypted";
/// Trusted key type (sealed by TPM).
pub const KEY_TYPE_TRUSTED: &str = "trusted";

// ---------------------------------------------------------------------------
// Special keyring IDs
// ---------------------------------------------------------------------------

/// Thread-specific keyring.
pub const KEY_SPEC_THREAD_KEYRING: i32 = -1;
/// Process-specific keyring.
pub const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
/// Session keyring.
pub const KEY_SPEC_SESSION_KEYRING: i32 = -3;
/// User-specific keyring.
pub const KEY_SPEC_USER_KEYRING: i32 = -4;
/// User session keyring.
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
/// Group keyring.
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
/// Requestor's keyring.
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;

// ---------------------------------------------------------------------------
// keyctl() commands
// ---------------------------------------------------------------------------

/// Get key attributes.
pub const KEYCTL_GET_KEYRING_ID: u32 = 0;
/// Join a session keyring.
pub const KEYCTL_JOIN_SESSION_KEYRING: u32 = 1;
/// Update a key.
pub const KEYCTL_UPDATE: u32 = 2;
/// Revoke a key.
pub const KEYCTL_REVOKE: u32 = 3;
/// Set key permissions.
pub const KEYCTL_SETPERM: u32 = 5;
/// Describe a key.
pub const KEYCTL_DESCRIBE: u32 = 6;
/// Clear a keyring.
pub const KEYCTL_CLEAR: u32 = 7;
/// Link a key to a keyring.
pub const KEYCTL_LINK: u32 = 8;
/// Unlink a key from a keyring.
pub const KEYCTL_UNLINK: u32 = 9;
/// Search for a key.
pub const KEYCTL_SEARCH: u32 = 10;
/// Read a key's payload.
pub const KEYCTL_READ: u32 = 11;
/// Instantiate a key.
pub const KEYCTL_INSTANTIATE: u32 = 12;
/// Set timeout on a key.
pub const KEYCTL_SET_TIMEOUT: u32 = 15;
/// Invalidate a key.
pub const KEYCTL_INVALIDATE: u32 = 21;
/// Restrict a keyring.
pub const KEYCTL_RESTRICT_KEYRING: u32 = 29;

// ---------------------------------------------------------------------------
// Key permissions
// ---------------------------------------------------------------------------

/// Possessor can view.
pub const KEY_POS_VIEW: u32 = 0x0100_0000;
/// Possessor can read.
pub const KEY_POS_READ: u32 = 0x0200_0000;
/// Possessor can write.
pub const KEY_POS_WRITE: u32 = 0x0400_0000;
/// Possessor can search.
pub const KEY_POS_SEARCH: u32 = 0x0800_0000;
/// Possessor can link.
pub const KEY_POS_LINK: u32 = 0x1000_0000;
/// Possessor can setattr.
pub const KEY_POS_SETATTR: u32 = 0x2000_0000;
/// Possessor has all permissions.
pub const KEY_POS_ALL: u32 = 0x3F00_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_names_distinct() {
        let types = [
            KEY_TYPE_USER,
            KEY_TYPE_LOGON,
            KEY_TYPE_KEYRING,
            KEY_TYPE_BIG_KEY,
            KEY_TYPE_ENCRYPTED,
            KEY_TYPE_TRUSTED,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_special_keyring_ids_distinct() {
        let ids = [
            KEY_SPEC_THREAD_KEYRING,
            KEY_SPEC_PROCESS_KEYRING,
            KEY_SPEC_SESSION_KEYRING,
            KEY_SPEC_USER_KEYRING,
            KEY_SPEC_USER_SESSION_KEYRING,
            KEY_SPEC_GROUP_KEYRING,
            KEY_SPEC_REQKEY_AUTH_KEY,
        ];
        for i in 0..ids.len() {
            assert!(ids[i] < 0);
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_keyctl_commands_distinct() {
        let cmds = [
            KEYCTL_GET_KEYRING_ID,
            KEYCTL_JOIN_SESSION_KEYRING,
            KEYCTL_UPDATE,
            KEYCTL_REVOKE,
            KEYCTL_SETPERM,
            KEYCTL_DESCRIBE,
            KEYCTL_CLEAR,
            KEYCTL_LINK,
            KEYCTL_UNLINK,
            KEYCTL_SEARCH,
            KEYCTL_READ,
            KEYCTL_INSTANTIATE,
            KEYCTL_SET_TIMEOUT,
            KEYCTL_INVALIDATE,
            KEYCTL_RESTRICT_KEYRING,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_pos_permissions_no_overlap() {
        let perms = [
            KEY_POS_VIEW,
            KEY_POS_READ,
            KEY_POS_WRITE,
            KEY_POS_SEARCH,
            KEY_POS_LINK,
            KEY_POS_SETATTR,
        ];
        for i in 0..perms.len() {
            assert!(perms[i].is_power_of_two());
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_pos_all_combines() {
        let combined = KEY_POS_VIEW
            | KEY_POS_READ
            | KEY_POS_WRITE
            | KEY_POS_SEARCH
            | KEY_POS_LINK
            | KEY_POS_SETATTR;
        assert_eq!(KEY_POS_ALL, combined);
    }
}
