//! `<linux/loop.h>` — Additional loop device constants (part 3).
//!
//! Supplementary loop device constants covering ioctl commands,
//! loop info flags, and encryption types.

// ---------------------------------------------------------------------------
// Loop device ioctl commands
// ---------------------------------------------------------------------------

/// Set file descriptor.
pub const LOOP_SET_FD: u32 = 0x4C00;
/// Clear file descriptor.
pub const LOOP_CLR_FD: u32 = 0x4C01;
/// Set status.
pub const LOOP_SET_STATUS: u32 = 0x4C02;
/// Get status.
pub const LOOP_GET_STATUS: u32 = 0x4C03;
/// Set status (64-bit).
pub const LOOP_SET_STATUS64: u32 = 0x4C04;
/// Get status (64-bit).
pub const LOOP_GET_STATUS64: u32 = 0x4C05;
/// Change file descriptor.
pub const LOOP_CHANGE_FD: u32 = 0x4C06;
/// Set capacity.
pub const LOOP_SET_CAPACITY: u32 = 0x4C07;
/// Set direct I/O.
pub const LOOP_SET_DIRECT_IO: u32 = 0x4C08;
/// Set block size.
pub const LOOP_SET_BLOCK_SIZE: u32 = 0x4C09;
/// Configure device.
pub const LOOP_CONFIGURE: u32 = 0x4C0A;

// ---------------------------------------------------------------------------
// Loop info flags (lo_flags)
// ---------------------------------------------------------------------------

/// Read-only.
pub const LO_FLAGS_READ_ONLY: u32 = 1;
/// Autoclear on last close.
pub const LO_FLAGS_AUTOCLEAR: u32 = 4;
/// Partition scan.
pub const LO_FLAGS_PARTSCAN: u32 = 8;
/// Direct I/O mode.
pub const LO_FLAGS_DIRECT_IO: u32 = 16;

// ---------------------------------------------------------------------------
// Loop encryption types
// ---------------------------------------------------------------------------

/// No encryption.
pub const LO_CRYPT_NONE: u32 = 0;
/// XOR encryption.
pub const LO_CRYPT_XOR: u32 = 1;
/// DES encryption.
pub const LO_CRYPT_DES: u32 = 2;
/// FISH2 encryption.
pub const LO_CRYPT_FISH2: u32 = 3;
/// BLOW encryption.
pub const LO_CRYPT_BLOW: u32 = 4;
/// CAST128 encryption.
pub const LO_CRYPT_CAST128: u32 = 5;
/// IDEA encryption.
pub const LO_CRYPT_IDEA: u32 = 6;
/// DUMMY encryption.
pub const LO_CRYPT_DUMMY: u32 = 9;
/// SKIPJACK encryption.
pub const LO_CRYPT_SKIPJACK: u32 = 10;
/// Crypto API.
pub const LO_CRYPT_CRYPTOAPI: u32 = 18;

// ---------------------------------------------------------------------------
// Loop name/key sizes
// ---------------------------------------------------------------------------

/// Maximum name length.
pub const LO_NAME_SIZE: u32 = 64;
/// Maximum key size.
pub const LO_KEY_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            LOOP_SET_FD,
            LOOP_CLR_FD,
            LOOP_SET_STATUS,
            LOOP_GET_STATUS,
            LOOP_SET_STATUS64,
            LOOP_GET_STATUS64,
            LOOP_CHANGE_FD,
            LOOP_SET_CAPACITY,
            LOOP_SET_DIRECT_IO,
            LOOP_SET_BLOCK_SIZE,
            LOOP_CONFIGURE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_info_flags_no_overlap() {
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
            LO_CRYPT_FISH2,
            LO_CRYPT_BLOW,
            LO_CRYPT_CAST128,
            LO_CRYPT_IDEA,
            LO_CRYPT_DUMMY,
            LO_CRYPT_SKIPJACK,
            LO_CRYPT_CRYPTOAPI,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_sizes() {
        assert_eq!(LO_NAME_SIZE, 64);
        assert_eq!(LO_KEY_SIZE, 32);
    }
}
