//! `<linux/loop.h>` — `/dev/loop*` and `/dev/loop-control` constants.
//!
//! losetup, util-linux's mount, and container runtimes (Docker
//! overlay2/devmapper) talk to the loop driver via the ioctls below
//! to back files with block devices.

// ---------------------------------------------------------------------------
// loop_info name lengths
// ---------------------------------------------------------------------------

/// Length of `lo_name` in `struct loop_info` and `struct loop_info64`.
pub const LO_NAME_SIZE: u32 = 64;
/// Length of `lo_encrypt_key`.
pub const LO_KEY_SIZE: u32 = 32;

// ---------------------------------------------------------------------------
// loop_info64.lo_flags bits
// ---------------------------------------------------------------------------

/// Read-only backing file.
pub const LO_FLAGS_READ_ONLY: u32 = 1 << 0;
/// Auto-clear loopdev when last close happens.
pub const LO_FLAGS_AUTOCLEAR: u32 = 1 << 2;
/// Allow part-table scan on the loop device.
pub const LO_FLAGS_PARTSCAN: u32 = 1 << 3;
/// Use Direct-IO on the backing file.
pub const LO_FLAGS_DIRECT_IO: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// ioctl numbers (legacy 0x4C00 series and modern LOOP_CONFIGURE)
// ---------------------------------------------------------------------------

/// Attach a file descriptor to the loop device.
pub const LOOP_SET_FD: u32 = 0x4C00;
/// Detach the backing file.
pub const LOOP_CLR_FD: u32 = 0x4C01;
/// Set `struct loop_info` parameters (legacy 32-bit).
pub const LOOP_SET_STATUS: u32 = 0x4C02;
/// Get `struct loop_info` parameters.
pub const LOOP_GET_STATUS: u32 = 0x4C03;
/// Set `struct loop_info64` parameters.
pub const LOOP_SET_STATUS64: u32 = 0x4C04;
/// Get `struct loop_info64` parameters.
pub const LOOP_GET_STATUS64: u32 = 0x4C05;
/// Change the backing fd (live).
pub const LOOP_CHANGE_FD: u32 = 0x4C06;
/// Inform the loop driver the backing file changed size.
pub const LOOP_SET_CAPACITY: u32 = 0x4C07;
/// Switch direct-IO mode on/off.
pub const LOOP_SET_DIRECT_IO: u32 = 0x4C08;
/// Set the block size of the loop device.
pub const LOOP_SET_BLOCK_SIZE: u32 = 0x4C09;
/// Modern atomic setup (loop_config struct).
pub const LOOP_CONFIGURE: u32 = 0x4C0A;

// ---------------------------------------------------------------------------
// /dev/loop-control ioctls
// ---------------------------------------------------------------------------

/// Allocate a new loop device.
pub const LOOP_CTL_ADD: u32 = 0x4C80;
/// Free a loop device.
pub const LOOP_CTL_REMOVE: u32 = 0x4C81;
/// Return the first unbound loop device number.
pub const LOOP_CTL_GET_FREE: u32 = 0x4C82;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_and_key_sizes() {
        assert_eq!(LO_NAME_SIZE, 64);
        assert_eq!(LO_KEY_SIZE, 32);
        // Key field shorter than name field — preserved in loop_info64.
        assert!(LO_KEY_SIZE < LO_NAME_SIZE);
    }

    #[test]
    fn test_flag_bits_distinct_pow2() {
        let f = [
            LO_FLAGS_READ_ONLY,
            LO_FLAGS_AUTOCLEAR,
            LO_FLAGS_PARTSCAN,
            LO_FLAGS_DIRECT_IO,
        ];
        for &b in &f {
            assert!(b.is_power_of_two());
        }
        for i in 0..f.len() {
            for j in (i + 1)..f.len() {
                assert_ne!(f[i], f[j]);
            }
        }
    }

    #[test]
    fn test_loop_ioctls_distinct_and_grouped() {
        let ops = [
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
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
            // All sit in the 0x4C00..0x4C80 per-device range.
            assert!(ops[i] >= 0x4C00);
            assert!(ops[i] < 0x4C80);
        }
    }

    #[test]
    fn test_loop_control_ioctls_distinct_and_grouped() {
        let c = [LOOP_CTL_ADD, LOOP_CTL_REMOVE, LOOP_CTL_GET_FREE];
        for i in 0..c.len() {
            for j in (i + 1)..c.len() {
                assert_ne!(c[i], c[j]);
            }
            // Control ioctls sit in the 0x4C80.. range.
            assert!(c[i] >= 0x4C80);
        }
    }
}
