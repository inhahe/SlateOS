//! `<linux/loop.h>` — Loop device constants.
//!
//! Loop devices present regular files as block devices. They are
//! used for mounting disk images (ISO, filesystem images), creating
//! encrypted containers, and testing filesystem code without
//! dedicated hardware.

// ---------------------------------------------------------------------------
// Loop ioctl commands
// ---------------------------------------------------------------------------

/// Set backing file.
pub const LOOP_SET_FD: u32 = 0x4C00;
/// Clear backing file.
pub const LOOP_CLR_FD: u32 = 0x4C01;
/// Set status (info64).
pub const LOOP_SET_STATUS64: u32 = 0x4C04;
/// Get status (info64).
pub const LOOP_GET_STATUS64: u32 = 0x4C05;
/// Change backing file.
pub const LOOP_CHANGE_FD: u32 = 0x4C06;
/// Set capacity (re-read size).
pub const LOOP_SET_CAPACITY: u32 = 0x4C07;
/// Set direct I/O mode.
pub const LOOP_SET_DIRECT_IO: u32 = 0x4C08;
/// Set block size.
pub const LOOP_SET_BLOCK_SIZE: u32 = 0x4C09;
/// Configure loop device.
pub const LOOP_CONFIGURE: u32 = 0x4C0A;

// ---------------------------------------------------------------------------
// Loop flags (lo_flags)
// ---------------------------------------------------------------------------

/// Read-only.
pub const LO_FLAGS_READ_ONLY: u32 = 1 << 0;
/// Autoclear (remove on last close).
pub const LO_FLAGS_AUTOCLEAR: u32 = 1 << 2;
/// Partition scan on setup.
pub const LO_FLAGS_PARTSCAN: u32 = 1 << 3;
/// Direct I/O mode.
pub const LO_FLAGS_DIRECT_IO: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// Encryption types (legacy, mostly unused)
// ---------------------------------------------------------------------------

/// No encryption.
pub const LO_CRYPT_NONE: u32 = 0;
/// XOR encryption (toy, insecure).
pub const LO_CRYPT_XOR: u32 = 1;
/// DES encryption.
pub const LO_CRYPT_DES: u32 = 2;
/// Cryptoloop (deprecated).
pub const LO_CRYPT_CRYPTOAPI: u32 = 18;

// ---------------------------------------------------------------------------
// Loop device limits
// ---------------------------------------------------------------------------

/// Maximum filename length in loop_info64.
pub const LO_NAME_SIZE: u8 = 64;
/// Maximum key size.
pub const LO_KEY_SIZE: u8 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctl_commands_distinct() {
        let cmds = [
            LOOP_SET_FD,
            LOOP_CLR_FD,
            LOOP_SET_STATUS64,
            LOOP_GET_STATUS64,
            LOOP_CHANGE_FD,
            LOOP_SET_CAPACITY,
            LOOP_SET_DIRECT_IO,
            LOOP_SET_BLOCK_SIZE,
            LOOP_CONFIGURE,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LO_FLAGS_READ_ONLY,
            LO_FLAGS_AUTOCLEAR,
            LO_FLAGS_PARTSCAN,
            LO_FLAGS_DIRECT_IO,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_crypt_types_distinct() {
        let types = [
            LO_CRYPT_NONE,
            LO_CRYPT_XOR,
            LO_CRYPT_DES,
            LO_CRYPT_CRYPTOAPI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
