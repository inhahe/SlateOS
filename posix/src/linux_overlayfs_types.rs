//! `<linux/fs.h>` — OverlayFS flag and ioctl constants.
//!
//! OverlayFS is a union filesystem that layers a writable upper
//! directory on top of one or more read-only lower directories.
//! These constants define mount options, file attributes, and
//! ioctl numbers for redirect and copy-up operations.

// ---------------------------------------------------------------------------
// OverlayFS mount flags / options
// ---------------------------------------------------------------------------

/// Redirect directory operations (rename support).
pub const OVL_REDIRECT_DIR: u32 = 0x01;
/// Enable NFS export support.
pub const OVL_NFS_EXPORT: u32 = 0x02;
/// Enable xattr metacopy (lazy copy-up).
pub const OVL_METACOPY: u32 = 0x04;
/// Index directory for inode consistency.
pub const OVL_INDEX: u32 = 0x08;
/// Enable volatile mode (skip sync on crash).
pub const OVL_VOLATILE: u32 = 0x10;
/// Enable userxattr (user.* namespace).
pub const OVL_USERXATTR: u32 = 0x20;

// ---------------------------------------------------------------------------
// OverlayFS file attributes (from getxattr)
// ---------------------------------------------------------------------------

/// File is opaque (hides lower layers).
pub const OVL_XATTR_OPAQUE_VAL: u8 = b'y';
/// Redirect xattr value prefix (relative path).
pub const OVL_XATTR_REDIRECT_REL: u8 = b'/';
/// Metacopy xattr indicates lazy copy-up.
pub const OVL_XATTR_METACOPY_VAL: u8 = 0;

// ---------------------------------------------------------------------------
// OverlayFS whiteout constants
// ---------------------------------------------------------------------------

/// Whiteout device major number.
pub const OVL_WHITEOUT_DEV_MAJOR: u32 = 0;
/// Whiteout device minor number.
pub const OVL_WHITEOUT_DEV_MINOR: u32 = 0;
/// Whiteout file mode (character device).
pub const OVL_WHITEOUT_MODE: u32 = 0o20000;

// ---------------------------------------------------------------------------
// Copy-up reasons
// ---------------------------------------------------------------------------

/// Copy-up triggered by write.
pub const OVL_COPYUP_WRITE: u32 = 0;
/// Copy-up triggered by chmod/chown.
pub const OVL_COPYUP_ATTR: u32 = 1;
/// Copy-up triggered by xattr set.
pub const OVL_COPYUP_XATTR: u32 = 2;
/// Copy-up triggered by rename.
pub const OVL_COPYUP_RENAME: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_flags_power_of_two() {
        let flags = [
            OVL_REDIRECT_DIR,
            OVL_NFS_EXPORT,
            OVL_METACOPY,
            OVL_INDEX,
            OVL_VOLATILE,
            OVL_USERXATTR,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_mount_flags_no_overlap() {
        let flags = [
            OVL_REDIRECT_DIR,
            OVL_NFS_EXPORT,
            OVL_METACOPY,
            OVL_INDEX,
            OVL_VOLATILE,
            OVL_USERXATTR,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_whiteout_constants() {
        assert_eq!(OVL_WHITEOUT_DEV_MAJOR, 0);
        assert_eq!(OVL_WHITEOUT_DEV_MINOR, 0);
    }

    #[test]
    fn test_copyup_reasons_distinct() {
        let reasons = [
            OVL_COPYUP_WRITE,
            OVL_COPYUP_ATTR,
            OVL_COPYUP_XATTR,
            OVL_COPYUP_RENAME,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_opaque_val() {
        assert_eq!(OVL_XATTR_OPAQUE_VAL, b'y');
    }
}
