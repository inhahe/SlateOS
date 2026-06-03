//! `<linux/mount.h>` — `mount_setattr(2)` ABI (Linux 5.12+).
//!
//! `mount_setattr` is the post-mount tuning syscall in the new mount
//! API. Where `fsconfig` configures a context before mounting,
//! `mount_setattr` flips per-mount flags on an already-attached mount
//! tree — toggling read-only, atime mode, ID mapping, etc. systemd
//! and `nspawn` use it to lock down container mount trees after they
//! are constructed.

// ---------------------------------------------------------------------------
// Attribute flags (set/clear via `mount_attr.attr_set` / `.attr_clr`)
// ---------------------------------------------------------------------------

pub const MOUNT_ATTR_RDONLY: u64 = 0x0000_0001;
pub const MOUNT_ATTR_NOSUID: u64 = 0x0000_0002;
pub const MOUNT_ATTR_NODEV: u64 = 0x0000_0004;
pub const MOUNT_ATTR_NOEXEC: u64 = 0x0000_0008;

/// Bitmask covering the atime selector (3 bits in 0x70).
pub const MOUNT_ATTR__ATIME: u64 = 0x0000_0070;
pub const MOUNT_ATTR_RELATIME: u64 = 0x0000_0000;
pub const MOUNT_ATTR_NOATIME: u64 = 0x0000_0010;
pub const MOUNT_ATTR_STRICTATIME: u64 = 0x0000_0020;

pub const MOUNT_ATTR_NODIRATIME: u64 = 0x0000_0080;
pub const MOUNT_ATTR_IDMAP: u64 = 0x0010_0000;
pub const MOUNT_ATTR_NOSYMFOLLOW: u64 = 0x0020_0000;

// ---------------------------------------------------------------------------
// `propagation` field values (passed in `mount_attr.propagation`)
// ---------------------------------------------------------------------------

pub const MS_UNBINDABLE: u32 = 1 << 17;
pub const MS_PRIVATE: u32 = 1 << 18;
pub const MS_SLAVE: u32 = 1 << 19;
pub const MS_SHARED: u32 = 1 << 20;

// ---------------------------------------------------------------------------
// `mount_setattr` flags
// ---------------------------------------------------------------------------

pub const AT_RECURSIVE: u32 = 0x8000;

// ---------------------------------------------------------------------------
// Syscall number (Linux 5.12+, x86_64)
// ---------------------------------------------------------------------------

pub const NR_MOUNT_SETATTR: u32 = 442;

// ---------------------------------------------------------------------------
// `struct mount_attr` size — kernel-defined, used for the `size` field.
// ---------------------------------------------------------------------------

/// Size of `struct mount_attr` as of Linux 5.12 (4×u64).
pub const MOUNT_ATTR_SIZE_VER0: usize = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_attrs_low_nibble() {
        assert_eq!(
            MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV | MOUNT_ATTR_NOEXEC,
            0xF
        );
        for v in [
            MOUNT_ATTR_RDONLY,
            MOUNT_ATTR_NOSUID,
            MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC,
        ] {
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_atime_field_is_3_bit_at_bit4() {
        assert_eq!(MOUNT_ATTR__ATIME, 0x70);
        assert_eq!(MOUNT_ATTR_RELATIME, 0);
        for v in [MOUNT_ATTR_NOATIME, MOUNT_ATTR_STRICTATIME] {
            assert_eq!(v & MOUNT_ATTR__ATIME, v);
        }
        // NOATIME and STRICTATIME differ — they can't both be set.
        assert_ne!(MOUNT_ATTR_NOATIME, MOUNT_ATTR_STRICTATIME);
    }

    #[test]
    fn test_high_bits_are_distinct_features() {
        // Each of these is a single bit in a different byte.
        for v in [MOUNT_ATTR_IDMAP, MOUNT_ATTR_NOSYMFOLLOW] {
            assert!(v.is_power_of_two());
            assert!(v > MOUNT_ATTR__ATIME);
        }
        assert_ne!(MOUNT_ATTR_IDMAP, MOUNT_ATTR_NOSYMFOLLOW);
    }

    #[test]
    fn test_propagation_bits_in_classic_ms_layout() {
        // The propagation field reuses the classic MS_* propagation bits
        // (17..20). Four bits, dense.
        let p = [MS_UNBINDABLE, MS_PRIVATE, MS_SLAVE, MS_SHARED];
        for v in p {
            assert!(v.is_power_of_two());
        }
        assert_eq!((MS_UNBINDABLE | MS_PRIVATE | MS_SLAVE | MS_SHARED) >> 17, 0xF);
    }

    #[test]
    fn test_syscall_and_struct_size() {
        assert_eq!(NR_MOUNT_SETATTR, 442);
        // 4 × 8-byte fields (attr_set, attr_clr, propagation+pad, userns_fd).
        assert_eq!(MOUNT_ATTR_SIZE_VER0, 32);
        // AT_RECURSIVE doubles as the mount_setattr flag.
        assert_eq!(AT_RECURSIVE, 0x8000);
    }
}
