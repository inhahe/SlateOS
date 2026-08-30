//! `<linux/keyctl.h>` — Kernel key management constants.
//!
//! The kernel keyring facility provides secure storage for
//! authentication tokens, encryption keys, and other security
//! data. Keys are organized in keyrings and accessed via the
//! keyctl(2) syscall.

// ---------------------------------------------------------------------------
// keyctl commands
// ---------------------------------------------------------------------------

/// Get keyring ID.
pub const KEYCTL_GET_KEYRING_ID: u32 = 0;
/// Join a session keyring.
pub const KEYCTL_JOIN_SESSION_KEYRING: u32 = 1;
/// Update a key's payload.
pub const KEYCTL_UPDATE: u32 = 2;
/// Revoke a key.
pub const KEYCTL_REVOKE: u32 = 3;
/// Set owner/group on a key.
pub const KEYCTL_CHOWN: u32 = 4;
/// Set permissions on a key.
pub const KEYCTL_SETPERM: u32 = 5;
/// Describe a key.
pub const KEYCTL_DESCRIBE: u32 = 6;
/// Clear a keyring.
pub const KEYCTL_CLEAR: u32 = 7;
/// Link a key into a keyring.
pub const KEYCTL_LINK: u32 = 8;
/// Unlink a key from a keyring.
pub const KEYCTL_UNLINK: u32 = 9;
/// Search a keyring.
pub const KEYCTL_SEARCH: u32 = 10;
/// Read a key's payload.
pub const KEYCTL_READ: u32 = 11;
/// Instantiate a key (from construction).
pub const KEYCTL_INSTANTIATE: u32 = 12;
/// Negate a key (mark as non-existent).
pub const KEYCTL_NEGATE: u32 = 13;
/// Set default request-key keyring.
pub const KEYCTL_SET_REQKEY_KEYRING: u32 = 14;
/// Set timeout on a key.
pub const KEYCTL_SET_TIMEOUT: u32 = 15;
/// Assume authority over a key.
pub const KEYCTL_ASSUME_AUTHORITY: u32 = 16;
/// Get key security label.
pub const KEYCTL_GET_SECURITY: u32 = 17;
/// Restrict keyring.
pub const KEYCTL_RESTRICT_KEYRING: u32 = 29;

// ---------------------------------------------------------------------------
// Special keyring IDs
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
/// Request-key auth keyring.
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;

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
// Common key types
// ---------------------------------------------------------------------------

/// User-defined key type.
pub const KEY_TYPE_USER: &str = "user";
/// Logon key type (cannot be read by userspace).
pub const KEY_TYPE_LOGON: &str = "logon";
/// Keyring type.
pub const KEY_TYPE_KEYRING: &str = "keyring";
/// Big key type (for large blobs).
pub const KEY_TYPE_BIG_KEY: &str = "big_key";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            KEYCTL_GET_KEYRING_ID,
            KEYCTL_JOIN_SESSION_KEYRING,
            KEYCTL_UPDATE,
            KEYCTL_REVOKE,
            KEYCTL_CHOWN,
            KEYCTL_SETPERM,
            KEYCTL_DESCRIBE,
            KEYCTL_CLEAR,
            KEYCTL_LINK,
            KEYCTL_UNLINK,
            KEYCTL_SEARCH,
            KEYCTL_READ,
            KEYCTL_INSTANTIATE,
            KEYCTL_NEGATE,
            KEYCTL_SET_REQKEY_KEYRING,
            KEYCTL_SET_TIMEOUT,
            KEYCTL_ASSUME_AUTHORITY,
            KEYCTL_GET_SECURITY,
            KEYCTL_RESTRICT_KEYRING,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_special_keyrings_distinct() {
        let keyrings = [
            KEY_SPEC_THREAD_KEYRING,
            KEY_SPEC_PROCESS_KEYRING,
            KEY_SPEC_SESSION_KEYRING,
            KEY_SPEC_USER_KEYRING,
            KEY_SPEC_USER_SESSION_KEYRING,
            KEY_SPEC_REQKEY_AUTH_KEY,
        ];
        for i in 0..keyrings.len() {
            for j in (i + 1)..keyrings.len() {
                assert_ne!(keyrings[i], keyrings[j]);
            }
        }
        // All negative
        for k in &keyrings {
            assert!(*k < 0);
        }
    }

    #[test]
    fn test_pos_perms_no_overlap() {
        let perms = [
            KEY_POS_VIEW,
            KEY_POS_READ,
            KEY_POS_WRITE,
            KEY_POS_SEARCH,
            KEY_POS_LINK,
            KEY_POS_SETATTR,
        ];
        for i in 0..perms.len() {
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_pos_all_covers_all() {
        let combined = KEY_POS_VIEW
            | KEY_POS_READ
            | KEY_POS_WRITE
            | KEY_POS_SEARCH
            | KEY_POS_LINK
            | KEY_POS_SETATTR;
        assert_eq!(KEY_POS_ALL, combined);
    }

    #[test]
    fn test_key_types_distinct() {
        let types = [
            KEY_TYPE_USER,
            KEY_TYPE_LOGON,
            KEY_TYPE_KEYRING,
            KEY_TYPE_BIG_KEY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
