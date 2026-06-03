//! `<linux/keyctl.h>` — `keyctl(2)` operation numbers.
//!
//! `keyctl(2)` is the single multiplexed syscall every userspace
//! keyring tool drives. `keyctl(1)`, `gnome-keyring-daemon`, `nfs-
//! idmapd`, `cifsd`, and the systemd credential layer all dispatch
//! through these op numbers.

// ---------------------------------------------------------------------------
// Syscall numbers on x86_64
// ---------------------------------------------------------------------------

pub const NR_ADD_KEY: u32 = 248;
pub const NR_REQUEST_KEY: u32 = 249;
pub const NR_KEYCTL: u32 = 250;

// ---------------------------------------------------------------------------
// `keyctl(2)` op numbers (dense, frozen since Linux 2.6.10)
// ---------------------------------------------------------------------------

pub const KEYCTL_GET_KEYRING_ID: u32 = 0;
pub const KEYCTL_JOIN_SESSION_KEYRING: u32 = 1;
pub const KEYCTL_UPDATE: u32 = 2;
pub const KEYCTL_REVOKE: u32 = 3;
pub const KEYCTL_CHOWN: u32 = 4;
pub const KEYCTL_SETPERM: u32 = 5;
pub const KEYCTL_DESCRIBE: u32 = 6;
pub const KEYCTL_CLEAR: u32 = 7;
pub const KEYCTL_LINK: u32 = 8;
pub const KEYCTL_UNLINK: u32 = 9;
pub const KEYCTL_SEARCH: u32 = 10;
pub const KEYCTL_READ: u32 = 11;
pub const KEYCTL_INSTANTIATE: u32 = 12;
pub const KEYCTL_NEGATE: u32 = 13;
pub const KEYCTL_SET_REQKEY_KEYRING: u32 = 14;
pub const KEYCTL_SET_TIMEOUT: u32 = 15;
pub const KEYCTL_ASSUME_AUTHORITY: u32 = 16;
pub const KEYCTL_GET_SECURITY: u32 = 17;
pub const KEYCTL_SESSION_TO_PARENT: u32 = 18;
pub const KEYCTL_REJECT: u32 = 19;
pub const KEYCTL_INSTANTIATE_IOV: u32 = 20;
pub const KEYCTL_INVALIDATE: u32 = 21;
pub const KEYCTL_GET_PERSISTENT: u32 = 22;
pub const KEYCTL_DH_COMPUTE: u32 = 23;
pub const KEYCTL_PKEY_QUERY: u32 = 24;
pub const KEYCTL_PKEY_ENCRYPT: u32 = 25;
pub const KEYCTL_PKEY_DECRYPT: u32 = 26;
pub const KEYCTL_PKEY_SIGN: u32 = 27;
pub const KEYCTL_PKEY_VERIFY: u32 = 28;
pub const KEYCTL_RESTRICT_KEYRING: u32 = 29;
pub const KEYCTL_MOVE: u32 = 30;
pub const KEYCTL_CAPABILITIES: u32 = 31;
pub const KEYCTL_WATCH_KEY: u32 = 32;

// ---------------------------------------------------------------------------
// `KEYCTL_CAPABILITIES` reply bits
// ---------------------------------------------------------------------------

pub const KEYCTL_CAPS0_CAPABILITIES: u8 = 0x01;
pub const KEYCTL_CAPS0_PERSISTENT_KEYRINGS: u8 = 0x02;
pub const KEYCTL_CAPS0_DIFFIE_HELLMAN: u8 = 0x04;
pub const KEYCTL_CAPS0_PUBLIC_KEY: u8 = 0x08;
pub const KEYCTL_CAPS0_BIG_KEY: u8 = 0x10;
pub const KEYCTL_CAPS0_INVALIDATE: u8 = 0x20;
pub const KEYCTL_CAPS0_RESTRICT_KEYRING: u8 = 0x40;
pub const KEYCTL_CAPS0_MOVE: u8 = 0x80;

// ---------------------------------------------------------------------------
// `KEYCTL_MOVE` flags
// ---------------------------------------------------------------------------

/// Replace an existing link with the same target.
pub const KEYCTL_MOVE_EXCL: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_numbers() {
        // x86_64 numbering: add_key=248, request_key=249, keyctl=250.
        assert_eq!(NR_ADD_KEY, 248);
        assert_eq!(NR_REQUEST_KEY, 249);
        assert_eq!(NR_KEYCTL, 250);
    }

    #[test]
    fn test_ops_dense_0_to_32() {
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
            KEYCTL_SET_REQKEY_KEYRING,
            KEYCTL_SET_TIMEOUT,
            KEYCTL_ASSUME_AUTHORITY,
            KEYCTL_GET_SECURITY,
            KEYCTL_SESSION_TO_PARENT,
            KEYCTL_REJECT,
            KEYCTL_INSTANTIATE_IOV,
            KEYCTL_INVALIDATE,
            KEYCTL_GET_PERSISTENT,
            KEYCTL_DH_COMPUTE,
            KEYCTL_PKEY_QUERY,
            KEYCTL_PKEY_ENCRYPT,
            KEYCTL_PKEY_DECRYPT,
            KEYCTL_PKEY_SIGN,
            KEYCTL_PKEY_VERIFY,
            KEYCTL_RESTRICT_KEYRING,
            KEYCTL_MOVE,
            KEYCTL_CAPABILITIES,
            KEYCTL_WATCH_KEY,
        ];
        for (i, &v) in ops.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_capability_bits_pow2_and_cover_byte() {
        let b = [
            KEYCTL_CAPS0_CAPABILITIES,
            KEYCTL_CAPS0_PERSISTENT_KEYRINGS,
            KEYCTL_CAPS0_DIFFIE_HELLMAN,
            KEYCTL_CAPS0_PUBLIC_KEY,
            KEYCTL_CAPS0_BIG_KEY,
            KEYCTL_CAPS0_INVALIDATE,
            KEYCTL_CAPS0_RESTRICT_KEYRING,
            KEYCTL_CAPS0_MOVE,
        ];
        let mut or = 0u8;
        for &x in &b {
            assert!(x.is_power_of_two());
            or |= x;
        }
        // All 8 single-bit flags OR together = 0xFF.
        assert_eq!(or, 0xFF);
    }

    #[test]
    fn test_move_flag() {
        assert_eq!(KEYCTL_MOVE_EXCL, 1);
    }
}
