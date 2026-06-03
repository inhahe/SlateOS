//! New file-system API — `fsopen(2)` / `fsconfig(2)` / `fsmount(2)` / `fspick(2)`.
//!
//! Added in Linux 5.2, this is the cleaner replacement for the classic
//! `mount(2)` API. A filesystem context is created with `fsopen`,
//! configured incrementally with `fsconfig`, materialised with
//! `fsmount`, then attached to the namespace with `move_mount`.
//! systemd and container runtimes adopted it because each step takes
//! file descriptors, making the whole sequence racefree.

// ---------------------------------------------------------------------------
// `fsopen(2)` flags
// ---------------------------------------------------------------------------

/// Set FD_CLOEXEC on the returned fd.
pub const FSOPEN_CLOEXEC: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// `fspick(2)` flags
// ---------------------------------------------------------------------------

pub const FSPICK_CLOEXEC: u32 = 0x0000_0001;
pub const FSPICK_SYMLINK_NOFOLLOW: u32 = 0x0000_0002;
pub const FSPICK_NO_AUTOMOUNT: u32 = 0x0000_0004;
pub const FSPICK_EMPTY_PATH: u32 = 0x0000_0008;

// ---------------------------------------------------------------------------
// `fsmount(2)` flags
// ---------------------------------------------------------------------------

/// CLOEXEC on the resulting mount fd.
pub const FSMOUNT_CLOEXEC: u32 = 0x0000_0001;

// ---------------------------------------------------------------------------
// `fsmount(2)` attribute flags (passed in `attr_flags`)
// ---------------------------------------------------------------------------

pub const MOUNT_ATTR_RDONLY: u32 = 0x0000_0001;
pub const MOUNT_ATTR_NOSUID: u32 = 0x0000_0002;
pub const MOUNT_ATTR_NODEV: u32 = 0x0000_0004;
pub const MOUNT_ATTR_NOEXEC: u32 = 0x0000_0008;

/// Mask covering the time-attribute field (values below).
pub const MOUNT_ATTR__ATIME: u32 = 0x0000_0070;
pub const MOUNT_ATTR_RELATIME: u32 = 0x0000_0000;
pub const MOUNT_ATTR_NOATIME: u32 = 0x0000_0010;
pub const MOUNT_ATTR_STRICTATIME: u32 = 0x0000_0020;
pub const MOUNT_ATTR_NODIRATIME: u32 = 0x0000_0080;

// ---------------------------------------------------------------------------
// `fsconfig(2)` commands
// ---------------------------------------------------------------------------

pub const FSCONFIG_SET_FLAG: u32 = 0;
pub const FSCONFIG_SET_STRING: u32 = 1;
pub const FSCONFIG_SET_BINARY: u32 = 2;
pub const FSCONFIG_SET_PATH: u32 = 3;
pub const FSCONFIG_SET_PATH_EMPTY: u32 = 4;
pub const FSCONFIG_SET_FD: u32 = 5;
pub const FSCONFIG_CMD_CREATE: u32 = 6;
pub const FSCONFIG_CMD_RECONFIGURE: u32 = 7;
pub const FSCONFIG_CMD_CREATE_EXCL: u32 = 8;

// ---------------------------------------------------------------------------
// `open_tree(2)` flags
// ---------------------------------------------------------------------------

pub const OPEN_TREE_CLONE: u32 = 1;
pub const OPEN_TREE_CLOEXEC: u32 = 0o2_000_000; // 524288 = 0x80000

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fspick_flags_dense() {
        let f = [
            FSPICK_CLOEXEC,
            FSPICK_SYMLINK_NOFOLLOW,
            FSPICK_NO_AUTOMOUNT,
            FSPICK_EMPTY_PATH,
        ];
        for v in f {
            assert!(v.is_power_of_two());
        }
        // Four dense bits.
        assert_eq!(
            FSPICK_CLOEXEC | FSPICK_SYMLINK_NOFOLLOW | FSPICK_NO_AUTOMOUNT | FSPICK_EMPTY_PATH,
            0xF
        );
    }

    #[test]
    fn test_mount_attr_security_low_nibble() {
        // Security bits mirror MS_* layout (bits 0..3).
        assert_eq!(
            MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV | MOUNT_ATTR_NOEXEC,
            0xF
        );
    }

    #[test]
    fn test_atime_field_layout() {
        // The atime selector is a 3-bit field in bits 4..6.
        assert_eq!(MOUNT_ATTR__ATIME, 0x70);
        // RELATIME (default) is zero — encoded by absence.
        assert_eq!(MOUNT_ATTR_RELATIME, 0);
        // Each named value sits within the mask.
        for v in [MOUNT_ATTR_NOATIME, MOUNT_ATTR_STRICTATIME] {
            assert_eq!(v & MOUNT_ATTR__ATIME, v);
        }
        // NODIRATIME is a separate bit above the field.
        assert_eq!(MOUNT_ATTR_NODIRATIME & MOUNT_ATTR__ATIME, 0);
    }

    #[test]
    fn test_fsconfig_commands_dense_0_to_8() {
        let c = [
            FSCONFIG_SET_FLAG,
            FSCONFIG_SET_STRING,
            FSCONFIG_SET_BINARY,
            FSCONFIG_SET_PATH,
            FSCONFIG_SET_PATH_EMPTY,
            FSCONFIG_SET_FD,
            FSCONFIG_CMD_CREATE,
            FSCONFIG_CMD_RECONFIGURE,
            FSCONFIG_CMD_CREATE_EXCL,
        ];
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_open_tree_flags() {
        assert_eq!(OPEN_TREE_CLONE, 1);
        // CLOEXEC is reused from O_CLOEXEC (0x80000).
        assert_eq!(OPEN_TREE_CLOEXEC, 0x80000);
    }

    #[test]
    fn test_cloexec_values_share_bit_zero() {
        // Every "open the result with CLOEXEC" flag is bit 0.
        assert_eq!(FSOPEN_CLOEXEC, 1);
        assert_eq!(FSPICK_CLOEXEC, 1);
        assert_eq!(FSMOUNT_CLOEXEC, 1);
    }
}
