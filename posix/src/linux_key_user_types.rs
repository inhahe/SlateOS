//! `<linux/keyctl.h>` — kernel keyring core types.
//!
//! Linux's keyring subsystem stores credentials (Kerberos tickets,
//! NFSv4 idmap entries, fscrypt master keys, kernel encryption keys
//! for ecryptfs, eCryptfs, Big Key, dm-crypt) inside in-kernel
//! keyrings. The constants below identify the well-known special
//! keyrings and the permission bits visible through `keyctl(1)`.

// ---------------------------------------------------------------------------
// Special keyring serial numbers (negative)
// ---------------------------------------------------------------------------

pub const KEY_SPEC_THREAD_KEYRING: i32 = -1;
pub const KEY_SPEC_PROCESS_KEYRING: i32 = -2;
pub const KEY_SPEC_SESSION_KEYRING: i32 = -3;
pub const KEY_SPEC_USER_KEYRING: i32 = -4;
pub const KEY_SPEC_USER_SESSION_KEYRING: i32 = -5;
pub const KEY_SPEC_GROUP_KEYRING: i32 = -6;
pub const KEY_SPEC_REQKEY_AUTH_KEY: i32 = -7;
pub const KEY_SPEC_REQUESTOR_KEYRING: i32 = -8;

// ---------------------------------------------------------------------------
// Permission bitfield (`key_perm_t` — 32 bits split as 8 × 4 groups)
// ---------------------------------------------------------------------------

pub const KEY_POS_VIEW: u32 = 0x0100_0000;
pub const KEY_POS_READ: u32 = 0x0200_0000;
pub const KEY_POS_WRITE: u32 = 0x0400_0000;
pub const KEY_POS_SEARCH: u32 = 0x0800_0000;
pub const KEY_POS_LINK: u32 = 0x1000_0000;
pub const KEY_POS_SETATTR: u32 = 0x2000_0000;
pub const KEY_POS_ALL: u32 = 0x3F00_0000;

pub const KEY_USR_VIEW: u32 = 0x0001_0000;
pub const KEY_USR_READ: u32 = 0x0002_0000;
pub const KEY_USR_WRITE: u32 = 0x0004_0000;
pub const KEY_USR_SEARCH: u32 = 0x0008_0000;
pub const KEY_USR_LINK: u32 = 0x0010_0000;
pub const KEY_USR_SETATTR: u32 = 0x0020_0000;
pub const KEY_USR_ALL: u32 = 0x003F_0000;

pub const KEY_GRP_VIEW: u32 = 0x0000_0100;
pub const KEY_GRP_READ: u32 = 0x0000_0200;
pub const KEY_GRP_WRITE: u32 = 0x0000_0400;
pub const KEY_GRP_SEARCH: u32 = 0x0000_0800;
pub const KEY_GRP_LINK: u32 = 0x0000_1000;
pub const KEY_GRP_SETATTR: u32 = 0x0000_2000;
pub const KEY_GRP_ALL: u32 = 0x0000_3F00;

pub const KEY_OTH_VIEW: u32 = 0x0000_0001;
pub const KEY_OTH_READ: u32 = 0x0000_0002;
pub const KEY_OTH_WRITE: u32 = 0x0000_0004;
pub const KEY_OTH_SEARCH: u32 = 0x0000_0008;
pub const KEY_OTH_LINK: u32 = 0x0000_0010;
pub const KEY_OTH_SETATTR: u32 = 0x0000_0020;
pub const KEY_OTH_ALL: u32 = 0x0000_003F;

// ---------------------------------------------------------------------------
// Key request status (from `request_key(2)`)
// ---------------------------------------------------------------------------

pub const KEY_REQKEY_DEFL_DEFAULT: i32 = 0;
pub const KEY_REQKEY_DEFL_THREAD_KEYRING: i32 = 1;
pub const KEY_REQKEY_DEFL_PROCESS_KEYRING: i32 = 2;
pub const KEY_REQKEY_DEFL_SESSION_KEYRING: i32 = 3;
pub const KEY_REQKEY_DEFL_USER_KEYRING: i32 = 4;
pub const KEY_REQKEY_DEFL_USER_SESSION_KEYRING: i32 = 5;
pub const KEY_REQKEY_DEFL_GROUP_KEYRING: i32 = 6;
pub const KEY_REQKEY_DEFL_REQUESTOR_KEYRING: i32 = 7;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_special_keyrings_dense_minus_1_to_minus_8() {
        let s = [
            KEY_SPEC_THREAD_KEYRING,
            KEY_SPEC_PROCESS_KEYRING,
            KEY_SPEC_SESSION_KEYRING,
            KEY_SPEC_USER_KEYRING,
            KEY_SPEC_USER_SESSION_KEYRING,
            KEY_SPEC_GROUP_KEYRING,
            KEY_SPEC_REQKEY_AUTH_KEY,
            KEY_SPEC_REQUESTOR_KEYRING,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v, -(i as i32 + 1));
        }
    }

    #[test]
    fn test_perm_groups_disjoint_and_total_is_or() {
        // The four 8-bit lanes (POS, USR, GRP, OTH) never overlap.
        assert_eq!(KEY_POS_ALL & KEY_USR_ALL, 0);
        assert_eq!(KEY_USR_ALL & KEY_GRP_ALL, 0);
        assert_eq!(KEY_GRP_ALL & KEY_OTH_ALL, 0);
        // Each "_ALL" is the OR of its six member bits.
        assert_eq!(
            KEY_POS_VIEW
                | KEY_POS_READ
                | KEY_POS_WRITE
                | KEY_POS_SEARCH
                | KEY_POS_LINK
                | KEY_POS_SETATTR,
            KEY_POS_ALL
        );
        assert_eq!(
            KEY_OTH_VIEW
                | KEY_OTH_READ
                | KEY_OTH_WRITE
                | KEY_OTH_SEARCH
                | KEY_OTH_LINK
                | KEY_OTH_SETATTR,
            KEY_OTH_ALL
        );
    }

    #[test]
    fn test_perm_single_bits_are_pow2() {
        let bits = [
            KEY_POS_VIEW,
            KEY_POS_READ,
            KEY_POS_WRITE,
            KEY_POS_SEARCH,
            KEY_POS_LINK,
            KEY_POS_SETATTR,
            KEY_USR_VIEW,
            KEY_USR_READ,
            KEY_USR_WRITE,
            KEY_USR_SEARCH,
            KEY_USR_LINK,
            KEY_USR_SETATTR,
            KEY_GRP_VIEW,
            KEY_GRP_READ,
            KEY_GRP_WRITE,
            KEY_GRP_SEARCH,
            KEY_GRP_LINK,
            KEY_GRP_SETATTR,
            KEY_OTH_VIEW,
            KEY_OTH_READ,
            KEY_OTH_WRITE,
            KEY_OTH_SEARCH,
            KEY_OTH_LINK,
            KEY_OTH_SETATTR,
        ];
        for b in bits {
            assert!(b.is_power_of_two());
        }
    }

    #[test]
    fn test_requestor_defaults_dense_0_to_7() {
        let d = [
            KEY_REQKEY_DEFL_DEFAULT,
            KEY_REQKEY_DEFL_THREAD_KEYRING,
            KEY_REQKEY_DEFL_PROCESS_KEYRING,
            KEY_REQKEY_DEFL_SESSION_KEYRING,
            KEY_REQKEY_DEFL_USER_KEYRING,
            KEY_REQKEY_DEFL_USER_SESSION_KEYRING,
            KEY_REQKEY_DEFL_GROUP_KEYRING,
            KEY_REQKEY_DEFL_REQUESTOR_KEYRING,
        ];
        for (i, &v) in d.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }
}
