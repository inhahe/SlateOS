//! `<linux/ovl_entry.h>` — OverlayFS union mount constants.
//!
//! OverlayFS is a union filesystem that overlays one filesystem on
//! top of another. It's the standard storage driver for Docker and
//! container runtimes, providing copy-on-write semantics with a
//! lower (read-only) and upper (read-write) layer.

// ---------------------------------------------------------------------------
// OverlayFS xattr names (used for metadata)
// ---------------------------------------------------------------------------

/// Opaque directory marker xattr.
pub const OVL_XATTR_OPAQUE: &str = "trusted.overlay.opaque";
/// Redirect xattr (for renamed directories).
pub const OVL_XATTR_REDIRECT: &str = "trusted.overlay.redirect";
/// Metacopy xattr (metadata-only copy-up).
pub const OVL_XATTR_METACOPY: &str = "trusted.overlay.metacopy";
/// Origin xattr (links upper to lower).
pub const OVL_XATTR_ORIGIN: &str = "trusted.overlay.origin";
/// Impure xattr (directory has copied-up children).
pub const OVL_XATTR_IMPURE: &str = "trusted.overlay.impure";
/// Nlink xattr (hardlink count tracking).
pub const OVL_XATTR_NLINK: &str = "trusted.overlay.nlink";

// ---------------------------------------------------------------------------
// OverlayFS whiteout
// ---------------------------------------------------------------------------

/// Whiteout character device major number.
pub const OVL_WHITEOUT_DEV_MAJOR: u32 = 0;
/// Whiteout character device minor number.
pub const OVL_WHITEOUT_DEV_MINOR: u32 = 0;

// ---------------------------------------------------------------------------
// OverlayFS flags / mount options (internal)
// ---------------------------------------------------------------------------

/// Redirect dir enabled.
pub const OVL_REDIRECT_ON: u8 = 1;
/// Redirect dir follow-only.
pub const OVL_REDIRECT_FOLLOW: u8 = 2;
/// NFS export enabled.
pub const OVL_NFS_EXPORT_ON: u8 = 1;
/// Index enabled.
pub const OVL_INDEX_ON: u8 = 1;
/// Metacopy enabled.
pub const OVL_METACOPY_ON: u8 = 1;

// ---------------------------------------------------------------------------
// OverlayFS inode flags
// ---------------------------------------------------------------------------

/// Entry is from upper layer.
pub const OVL_UPPER: u8 = 1 << 0;
/// Entry has whiteouts beneath.
pub const OVL_WHITEOUTS: u8 = 1 << 1;
/// Directory is opaque (hides lower).
pub const OVL_OPAQUE: u8 = 1 << 2;
/// Entry is impure (mixed layers).
pub const OVL_IMPURE: u8 = 1 << 3;
/// Entry was copied up.
pub const OVL_COPY_UP: u8 = 1 << 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xattr_names_distinct() {
        let names = [
            OVL_XATTR_OPAQUE, OVL_XATTR_REDIRECT, OVL_XATTR_METACOPY,
            OVL_XATTR_ORIGIN, OVL_XATTR_IMPURE, OVL_XATTR_NLINK,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_xattr_prefix() {
        assert!(OVL_XATTR_OPAQUE.starts_with("trusted.overlay."));
        assert!(OVL_XATTR_REDIRECT.starts_with("trusted.overlay."));
        assert!(OVL_XATTR_METACOPY.starts_with("trusted.overlay."));
        assert!(OVL_XATTR_ORIGIN.starts_with("trusted.overlay."));
        assert!(OVL_XATTR_IMPURE.starts_with("trusted.overlay."));
        assert!(OVL_XATTR_NLINK.starts_with("trusted.overlay."));
    }

    #[test]
    fn test_inode_flags_no_overlap() {
        let flags = [OVL_UPPER, OVL_WHITEOUTS, OVL_OPAQUE, OVL_IMPURE, OVL_COPY_UP];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_whiteout_dev() {
        assert_eq!(OVL_WHITEOUT_DEV_MAJOR, 0);
        assert_eq!(OVL_WHITEOUT_DEV_MINOR, 0);
    }
}
