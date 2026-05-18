//! `<linux/loop.h>` — Loop device constants (extended).
//!
//! Extended loop device constants covering loop control
//! commands, loop info flags, encryption types, and
//! loop status flags.

// ---------------------------------------------------------------------------
// Loop device IOCTL commands
// ---------------------------------------------------------------------------

/// Set backing file.
pub const LOOP_SET_FD: u32 = 0x4C00;
/// Clear backing file.
pub const LOOP_CLR_FD: u32 = 0x4C01;
/// Set loop status.
pub const LOOP_SET_STATUS: u32 = 0x4C02;
/// Get loop status.
pub const LOOP_GET_STATUS: u32 = 0x4C03;
/// Set loop status (64-bit).
pub const LOOP_SET_STATUS64: u32 = 0x4C04;
/// Get loop status (64-bit).
pub const LOOP_GET_STATUS64: u32 = 0x4C05;
/// Change backing file.
pub const LOOP_CHANGE_FD: u32 = 0x4C06;
/// Set capacity.
pub const LOOP_SET_CAPACITY: u32 = 0x4C07;
/// Set direct I/O mode.
pub const LOOP_SET_DIRECT_IO: u32 = 0x4C08;
/// Set block size.
pub const LOOP_SET_BLOCK_SIZE: u32 = 0x4C09;
/// Configure loop device.
pub const LOOP_CONFIGURE: u32 = 0x4C0A;

// ---------------------------------------------------------------------------
// Loop control IOCTL commands
// ---------------------------------------------------------------------------

/// Add a new loop device.
pub const LOOP_CTL_ADD: u32 = 0x4C80;
/// Remove a loop device.
pub const LOOP_CTL_REMOVE: u32 = 0x4C81;
/// Get a free loop device number.
pub const LOOP_CTL_GET_FREE: u32 = 0x4C82;

// ---------------------------------------------------------------------------
// Loop flags (LO_FLAGS_*)
// ---------------------------------------------------------------------------

/// Read-only.
pub const LO_FLAGS_READ_ONLY: u32 = 1 << 0;
/// Autoclear on last close.
pub const LO_FLAGS_AUTOCLEAR: u32 = 1 << 2;
/// Partition scan.
pub const LO_FLAGS_PARTSCAN: u32 = 1 << 3;
/// Direct I/O.
pub const LO_FLAGS_DIRECT_IO: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Loop encryption types (LO_CRYPT_*)
// ---------------------------------------------------------------------------

/// No encryption.
pub const LO_CRYPT_NONE: u32 = 0;
/// XOR encryption.
pub const LO_CRYPT_XOR: u32 = 1;
/// DES encryption.
pub const LO_CRYPT_DES: u32 = 2;
/// FISH2 encryption.
pub const LO_CRYPT_FISH2: u32 = 3;
/// Blowfish encryption.
pub const LO_CRYPT_BLOW: u32 = 4;
/// CAST128 encryption.
pub const LO_CRYPT_CAST128: u32 = 5;
/// IDEA encryption.
pub const LO_CRYPT_IDEA: u32 = 6;
/// Dummy encryption.
pub const LO_CRYPT_DUMMY: u32 = 9;
/// Skipjack encryption.
pub const LO_CRYPT_SKIPJACK: u32 = 10;
/// Crypto API (kernel crypto).
pub const LO_CRYPT_CRYPTOAPI: u32 = 18;

// ---------------------------------------------------------------------------
// Loop device name length
// ---------------------------------------------------------------------------

/// Maximum file name length.
pub const LO_NAME_SIZE: u32 = 64;
/// Maximum encryption key length.
pub const LO_KEY_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_cmds_distinct() {
        let cmds = [
            LOOP_SET_FD, LOOP_CLR_FD, LOOP_SET_STATUS,
            LOOP_GET_STATUS, LOOP_SET_STATUS64, LOOP_GET_STATUS64,
            LOOP_CHANGE_FD, LOOP_SET_CAPACITY,
            LOOP_SET_DIRECT_IO, LOOP_SET_BLOCK_SIZE,
            LOOP_CONFIGURE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_ctl_cmds_distinct() {
        let cmds = [LOOP_CTL_ADD, LOOP_CTL_REMOVE, LOOP_CTL_GET_FREE];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LO_FLAGS_READ_ONLY, LO_FLAGS_AUTOCLEAR,
            LO_FLAGS_PARTSCAN, LO_FLAGS_DIRECT_IO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [
            LO_FLAGS_READ_ONLY, LO_FLAGS_AUTOCLEAR,
            LO_FLAGS_PARTSCAN, LO_FLAGS_DIRECT_IO,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_crypt_types_distinct() {
        let crypts = [
            LO_CRYPT_NONE, LO_CRYPT_XOR, LO_CRYPT_DES,
            LO_CRYPT_FISH2, LO_CRYPT_BLOW, LO_CRYPT_CAST128,
            LO_CRYPT_IDEA, LO_CRYPT_DUMMY, LO_CRYPT_SKIPJACK,
            LO_CRYPT_CRYPTOAPI,
        ];
        for i in 0..crypts.len() {
            for j in (i + 1)..crypts.len() {
                assert_ne!(crypts[i], crypts[j]);
            }
        }
    }

    #[test]
    fn test_crypt_none_is_zero() {
        assert_eq!(LO_CRYPT_NONE, 0);
    }

    #[test]
    fn test_set_fd_base() {
        assert_eq!(LOOP_SET_FD, 0x4C00);
    }

    #[test]
    fn test_name_size() {
        assert_eq!(LO_NAME_SIZE, 64);
    }

    #[test]
    fn test_key_size() {
        assert_eq!(LO_KEY_SIZE, 32);
    }
}
