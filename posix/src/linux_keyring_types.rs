//! `<linux/keyctl.h>` — Kernel keyring management constants.
//!
//! The Linux keyring service manages cryptographic keys, authentication
//! tokens, and other security data. Keys are stored in keyrings
//! (which are themselves keys of type "keyring"). Processes have
//! several default keyrings: session, process, thread, user.

// ---------------------------------------------------------------------------
// Special keyring IDs
// ---------------------------------------------------------------------------

/// Thread keyring (per-thread, not inherited).
pub const KEY_SPEC_THREAD_KEYRING: i32 = -1;
/// Process keyring (shared by all threads in process).
pub const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
/// Session keyring (shared by login session).
pub const KEY_SPEC_SESSION_KEYRING: i32 = -3;
/// User keyring (per-UID, persistent).
pub const KEY_SPEC_USER_KEYRING: i32 = -4;
/// User session keyring.
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
/// Group keyring.
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
/// Requestor auth key.
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;

// ---------------------------------------------------------------------------
// keyctl() operations
// ---------------------------------------------------------------------------

/// Get the key's security context.
pub const KEYCTL_GET_SECURITY: u32 = 17;
/// Set a timeout on a key.
pub const KEYCTL_SET_TIMEOUT: u32 = 15;
/// Describe a key.
pub const KEYCTL_DESCRIBE: u32 = 6;
/// Read key payload.
pub const KEYCTL_READ: u32 = 11;
/// Update a key's payload.
pub const KEYCTL_UPDATE: u32 = 2;
/// Revoke a key.
pub const KEYCTL_REVOKE: u32 = 3;
/// Search for a key.
pub const KEYCTL_SEARCH: u32 = 10;
/// Link a key to a keyring.
pub const KEYCTL_LINK: u32 = 8;
/// Unlink a key from a keyring.
pub const KEYCTL_UNLINK: u32 = 9;
/// Clear all keys from a keyring.
pub const KEYCTL_CLEAR: u32 = 7;
/// Set key permissions.
pub const KEYCTL_SETPERM: u32 = 5;
/// Change key ownership.
pub const KEYCTL_CHOWN: u32 = 4;
/// Instantiate a key.
pub const KEYCTL_INSTANTIATE: u32 = 12;
/// Negate a key (mark as negative).
pub const KEYCTL_NEGATE: u32 = 13;
/// Join/create a named session keyring.
pub const KEYCTL_JOIN_SESSION_KEYRING: u32 = 1;
/// Invalidate a key.
pub const KEYCTL_INVALIDATE: u32 = 21;

// ---------------------------------------------------------------------------
// Key permissions (bitmask, per-possessor/user/group/other)
// ---------------------------------------------------------------------------

/// Possessor can view key attributes.
pub const KEY_POS_VIEW: u32 = 0x0100_0000;
/// Possessor can read key payload.
pub const KEY_POS_READ: u32 = 0x0200_0000;
/// Possessor can write/update key.
pub const KEY_POS_WRITE: u32 = 0x0400_0000;
/// Possessor can search for key.
pub const KEY_POS_SEARCH: u32 = 0x0800_0000;
/// Possessor can link key.
pub const KEY_POS_LINK: u32 = 0x1000_0000;
/// Possessor can set key attributes.
pub const KEY_POS_SETATTR: u32 = 0x2000_0000;
/// Possessor: all permissions.
pub const KEY_POS_ALL: u32 = 0x3F00_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_keyrings_negative() {
        let rings = [
            KEY_SPEC_THREAD_KEYRING,
            KEY_SPEC_PROCESS_KEYRING,
            KEY_SPEC_SESSION_KEYRING,
            KEY_SPEC_USER_KEYRING,
            KEY_SPEC_USER_SESSION_KEYRING,
            KEY_SPEC_GROUP_KEYRING,
            KEY_SPEC_REQKEY_AUTH_KEY,
        ];
        for r in rings {
            assert!(r < 0);
        }
    }

    #[test]
    fn test_special_keyrings_distinct() {
        let rings = [
            KEY_SPEC_THREAD_KEYRING,
            KEY_SPEC_PROCESS_KEYRING,
            KEY_SPEC_SESSION_KEYRING,
            KEY_SPEC_USER_KEYRING,
            KEY_SPEC_USER_SESSION_KEYRING,
            KEY_SPEC_GROUP_KEYRING,
            KEY_SPEC_REQKEY_AUTH_KEY,
        ];
        for i in 0..rings.len() {
            for j in (i + 1)..rings.len() {
                assert_ne!(rings[i], rings[j]);
            }
        }
    }

    #[test]
    fn test_keyctl_ops_distinct() {
        let ops = [
            KEYCTL_GET_SECURITY,
            KEYCTL_SET_TIMEOUT,
            KEYCTL_DESCRIBE,
            KEYCTL_READ,
            KEYCTL_UPDATE,
            KEYCTL_REVOKE,
            KEYCTL_SEARCH,
            KEYCTL_LINK,
            KEYCTL_UNLINK,
            KEYCTL_CLEAR,
            KEYCTL_SETPERM,
            KEYCTL_CHOWN,
            KEYCTL_INSTANTIATE,
            KEYCTL_NEGATE,
            KEYCTL_JOIN_SESSION_KEYRING,
            KEYCTL_INVALIDATE,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
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
            assert!(perms[i].is_power_of_two());
            for j in (i + 1)..perms.len() {
                assert_eq!(perms[i] & perms[j], 0);
            }
        }
    }

    #[test]
    fn test_pos_all_combines_all() {
        let combined = KEY_POS_VIEW
            | KEY_POS_READ
            | KEY_POS_WRITE
            | KEY_POS_SEARCH
            | KEY_POS_LINK
            | KEY_POS_SETATTR;
        assert_eq!(KEY_POS_ALL, combined);
    }
}
