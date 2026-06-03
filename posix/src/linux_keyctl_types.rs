//! `<linux/keyctl.h>` — Kernel key management (keyctl) constants.
//!
//! The kernel keyring subsystem provides in-kernel storage for
//! authentication tokens, encryption keys, and other security
//! credentials. Keys are organised in keyrings (which are themselves
//! keys) and accessed via keyctl() syscall operations.

// ---------------------------------------------------------------------------
// keyctl operations
// ---------------------------------------------------------------------------

/// Get a key's payload.
pub const KEYCTL_GET_KEYRING_ID: u32 = 0;
/// Join/create a named session keyring.
pub const KEYCTL_JOIN_SESSION_KEYRING: u32 = 1;
/// Update a key's payload.
pub const KEYCTL_UPDATE: u32 = 2;
/// Revoke a key.
pub const KEYCTL_REVOKE: u32 = 3;
/// Change key ownership.
pub const KEYCTL_CHOWN: u32 = 4;
/// Set key permissions.
pub const KEYCTL_SETPERM: u32 = 5;
/// Describe a key.
pub const KEYCTL_DESCRIBE: u32 = 6;
/// Clear a keyring.
pub const KEYCTL_CLEAR: u32 = 7;
/// Link a key into a keyring.
pub const KEYCTL_LINK: u32 = 8;
/// Unlink a key from a keyring.
pub const KEYCTL_UNLINK: u32 = 9;
/// Search for a key.
pub const KEYCTL_SEARCH: u32 = 10;
/// Read a key's payload.
pub const KEYCTL_READ: u32 = 11;
/// Instantiate a key.
pub const KEYCTL_INSTANTIATE: u32 = 12;
/// Negate a key (instantiation rejection).
pub const KEYCTL_NEGATE: u32 = 13;
/// Set key timeout.
pub const KEYCTL_SET_TIMEOUT: u32 = 15;
/// Assume authority of a key.
pub const KEYCTL_ASSUME_AUTHORITY: u32 = 16;
/// Get key security label.
pub const KEYCTL_GET_SECURITY: u32 = 17;
/// Set session keyring on parent process.
pub const KEYCTL_SESSION_TO_PARENT: u32 = 18;
/// Reject a key with specific error.
pub const KEYCTL_REJECT: u32 = 19;
/// Instantiate with an iovec payload.
pub const KEYCTL_INSTANTIATE_IOV: u32 = 20;
/// Invalidate a key immediately.
pub const KEYCTL_INVALIDATE: u32 = 21;
/// Get persistent keyring for a UID.
pub const KEYCTL_GET_PERSISTENT: u32 = 22;
/// Restrict keyring (DH compute).
pub const KEYCTL_DH_COMPUTE: u32 = 23;
/// Restrict keyring linkage.
pub const KEYCTL_RESTRICT_KEYRING: u32 = 29;

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
/// User session keyring (default).
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
/// Group keyring.
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
/// Requestor's authorisation keyring.
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;

// ---------------------------------------------------------------------------
// Key permission bits
// ---------------------------------------------------------------------------

/// Possessor may view.
pub const KEY_POS_VIEW: u32 = 0x0100_0000;
/// Possessor may read.
pub const KEY_POS_READ: u32 = 0x0200_0000;
/// Possessor may write.
pub const KEY_POS_WRITE: u32 = 0x0400_0000;
/// Possessor may search.
pub const KEY_POS_SEARCH: u32 = 0x0800_0000;
/// Possessor may link.
pub const KEY_POS_LINK: u32 = 0x1000_0000;
/// Possessor may set attribute.
pub const KEY_POS_SETATTR: u32 = 0x2000_0000;
/// All possessor permissions.
pub const KEY_POS_ALL: u32 = 0x3F00_0000;

/// User may view.
pub const KEY_USR_VIEW: u32 = 0x0001_0000;
/// User may read.
pub const KEY_USR_READ: u32 = 0x0002_0000;
/// User may write.
pub const KEY_USR_WRITE: u32 = 0x0004_0000;
/// User may search.
pub const KEY_USR_SEARCH: u32 = 0x0008_0000;
/// User may link.
pub const KEY_USR_LINK: u32 = 0x0010_0000;
/// User may set attribute.
pub const KEY_USR_SETATTR: u32 = 0x0020_0000;
/// All user permissions.
pub const KEY_USR_ALL: u32 = 0x003F_0000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyctl_ops_distinct() {
        let ops = [
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
            KEYCTL_SET_TIMEOUT,
            KEYCTL_ASSUME_AUTHORITY,
            KEYCTL_GET_SECURITY,
            KEYCTL_SESSION_TO_PARENT,
            KEYCTL_REJECT,
            KEYCTL_INSTANTIATE_IOV,
            KEYCTL_INVALIDATE,
            KEYCTL_GET_PERSISTENT,
            KEYCTL_DH_COMPUTE,
            KEYCTL_RESTRICT_KEYRING,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
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
            assert!(rings[i] < 0); // All are negative sentinel values.
            for j in (i + 1)..rings.len() {
                assert_ne!(rings[i], rings[j]);
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
    fn test_pos_all_covers_all_pos() {
        let combined = KEY_POS_VIEW
            | KEY_POS_READ
            | KEY_POS_WRITE
            | KEY_POS_SEARCH
            | KEY_POS_LINK
            | KEY_POS_SETATTR;
        assert_eq!(KEY_POS_ALL, combined);
    }

    #[test]
    fn test_usr_all_covers_all_usr() {
        let combined = KEY_USR_VIEW
            | KEY_USR_READ
            | KEY_USR_WRITE
            | KEY_USR_SEARCH
            | KEY_USR_LINK
            | KEY_USR_SETATTR;
        assert_eq!(KEY_USR_ALL, combined);
    }
}
