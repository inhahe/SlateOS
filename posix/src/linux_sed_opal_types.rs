//! `<linux/sed-opal.h>` — Self-Encrypting Drive (SED) OPAL constants.
//!
//! TCG Opal is a specification for self-encrypting drives (SEDs).
//! The drive hardware encrypts all data transparently. The OPAL
//! interface allows setting up locking ranges (regions of the disk
//! that lock/unlock independently), managing users and credentials,
//! and performing secure erase by destroying encryption keys. Linux
//! exposes OPAL management through IOCTLs on the block device.

// ---------------------------------------------------------------------------
// OPAL IOCTLs
// ---------------------------------------------------------------------------

/// Save (persist) the locking state.
pub const IOC_OPAL_SAVE: u32 = 0x00;
/// Lock/unlock a locking range.
pub const IOC_OPAL_LOCK_UNLOCK: u32 = 0x01;
/// Take ownership of the drive.
pub const IOC_OPAL_TAKE_OWNERSHIP: u32 = 0x02;
/// Activate the locking SP (Security Provider).
pub const IOC_OPAL_ACTIVATE_LSP: u32 = 0x03;
/// Set a locking range.
pub const IOC_OPAL_SET_LR: u32 = 0x04;
/// Add a user to a locking range.
pub const IOC_OPAL_ADD_USR_TO_LR: u32 = 0x05;
/// Set a new password.
pub const IOC_OPAL_SET_PW: u32 = 0x06;
/// Activate a user.
pub const IOC_OPAL_ACTIVATE_USR: u32 = 0x07;
/// Revert the drive to factory state.
pub const IOC_OPAL_REVERT_TPR: u32 = 0x08;
/// Enable/disable MBR shadowing.
pub const IOC_OPAL_MBR_CTRL: u32 = 0x09;
/// Write to the MBR shadow table.
pub const IOC_OPAL_WRITE_SHADOW_MBR: u32 = 0x0A;
/// Erase a locking range (crypto-erase).
pub const IOC_OPAL_ERASE_LR: u32 = 0x0B;
/// Secure erase (PSID revert).
pub const IOC_OPAL_SECURE_ERASE_LR: u32 = 0x0C;
/// Query drive status.
pub const IOC_OPAL_STATUS: u32 = 0x0E;

// ---------------------------------------------------------------------------
// OPAL lock state
// ---------------------------------------------------------------------------

/// Locking range is read-write (unlocked).
pub const OPAL_RW: u32 = 0;
/// Locking range is read-only.
pub const OPAL_RO: u32 = 1;
/// Locking range is locked (no access).
pub const OPAL_LK: u32 = 2;

// ---------------------------------------------------------------------------
// OPAL user IDs
// ---------------------------------------------------------------------------

/// Admin1 (primary administrator).
pub const OPAL_ADMIN1: u32 = 0;
/// User1.
pub const OPAL_USER1: u32 = 1;
/// User2.
pub const OPAL_USER2: u32 = 2;
/// User3.
pub const OPAL_USER3: u32 = 3;
/// User4.
pub const OPAL_USER4: u32 = 4;

// ---------------------------------------------------------------------------
// OPAL MBR control
// ---------------------------------------------------------------------------

/// Enable MBR shadowing.
pub const OPAL_MBR_ENABLE: u32 = 0;
/// Disable MBR shadowing.
pub const OPAL_MBR_DISABLE: u32 = 1;
/// MBR done (show real disk after auth).
pub const OPAL_MBR_DONE: u32 = 2;

// ---------------------------------------------------------------------------
// OPAL feature flags
// ---------------------------------------------------------------------------

/// Drive supports Opal v1.
pub const OPAL_FEATURE_V1: u32 = 1 << 0;
/// Drive supports Opal v2.
pub const OPAL_FEATURE_V2: u32 = 1 << 1;
/// Drive supports single-user mode.
pub const OPAL_FEATURE_SINGLE_USER: u32 = 1 << 2;
/// Drive supports data store tables.
pub const OPAL_FEATURE_DATA_STORE: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            IOC_OPAL_SAVE,
            IOC_OPAL_LOCK_UNLOCK,
            IOC_OPAL_TAKE_OWNERSHIP,
            IOC_OPAL_ACTIVATE_LSP,
            IOC_OPAL_SET_LR,
            IOC_OPAL_ADD_USR_TO_LR,
            IOC_OPAL_SET_PW,
            IOC_OPAL_ACTIVATE_USR,
            IOC_OPAL_REVERT_TPR,
            IOC_OPAL_MBR_CTRL,
            IOC_OPAL_WRITE_SHADOW_MBR,
            IOC_OPAL_ERASE_LR,
            IOC_OPAL_SECURE_ERASE_LR,
            IOC_OPAL_STATUS,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_lock_states_distinct() {
        let states = [OPAL_RW, OPAL_RO, OPAL_LK];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_user_ids_distinct() {
        let users = [OPAL_ADMIN1, OPAL_USER1, OPAL_USER2, OPAL_USER3, OPAL_USER4];
        for i in 0..users.len() {
            for j in (i + 1)..users.len() {
                assert_ne!(users[i], users[j]);
            }
        }
    }

    #[test]
    fn test_mbr_control_distinct() {
        let mbr = [OPAL_MBR_ENABLE, OPAL_MBR_DISABLE, OPAL_MBR_DONE];
        for i in 0..mbr.len() {
            for j in (i + 1)..mbr.len() {
                assert_ne!(mbr[i], mbr[j]);
            }
        }
    }

    #[test]
    fn test_feature_flags_no_overlap() {
        let flags = [
            OPAL_FEATURE_V1,
            OPAL_FEATURE_V2,
            OPAL_FEATURE_SINGLE_USER,
            OPAL_FEATURE_DATA_STORE,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
