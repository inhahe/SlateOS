//! `<linux/xattr.h>` — User namespace extended attribute constants.
//!
//! The "user." xattr namespace is the only namespace accessible to
//! unprivileged processes. Attributes can be set on regular files
//! and directories (not symlinks or device nodes) by the file owner.

// ---------------------------------------------------------------------------
// User xattr limits
// ---------------------------------------------------------------------------

/// Maximum user xattr name length (after "user." prefix).
pub const USER_XATTR_NAME_MAX: u32 = 250;
/// Maximum user xattr value size (same as XATTR_SIZE_MAX).
pub const USER_XATTR_VALUE_MAX: u32 = 65536;

// ---------------------------------------------------------------------------
// Well-known user.* attribute names
// ---------------------------------------------------------------------------

/// MIME type attribute (used by file managers).
pub const XATTR_USER_MIME_TYPE: &[u8] = b"user.mime_type";
/// Character encoding attribute.
pub const XATTR_USER_CHARSET: &[u8] = b"user.charset";
/// Creator application attribute.
pub const XATTR_USER_CREATOR: &[u8] = b"user.creator";
/// Checksum/hash attribute.
pub const XATTR_USER_CHECKSUM: &[u8] = b"user.checksum";
/// Comment/description attribute.
pub const XATTR_USER_COMMENT: &[u8] = b"user.comment";
/// Freedesktop Dublin Core title.
pub const XATTR_USER_XDGTITLE: &[u8] = b"user.xdg.title";
/// Freedesktop Dublin Core publisher.
pub const XATTR_USER_XDGPUBLISHER: &[u8] = b"user.xdg.publisher";

// ---------------------------------------------------------------------------
// User xattr inode restrictions
// ---------------------------------------------------------------------------

/// User xattrs allowed on regular files.
pub const USER_XATTR_INODE_FILE: u32 = 1;
/// User xattrs allowed on directories.
pub const USER_XATTR_INODE_DIR: u32 = 2;
/// User xattrs NOT allowed on symlinks.
pub const USER_XATTR_INODE_SYMLINK: u32 = 0;
/// User xattrs NOT allowed on device nodes.
pub const USER_XATTR_INODE_DEVICE: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_xattr_name_max() {
        // 255 (XATTR_NAME_MAX) minus 5 ("user.") = 250
        assert_eq!(USER_XATTR_NAME_MAX, 250);
    }

    #[test]
    fn test_user_xattr_value_max() {
        assert_eq!(USER_XATTR_VALUE_MAX, 65536);
    }

    #[test]
    fn test_well_known_attrs_start_with_user() {
        let attrs = [
            XATTR_USER_MIME_TYPE,
            XATTR_USER_CHARSET,
            XATTR_USER_CREATOR,
            XATTR_USER_CHECKSUM,
            XATTR_USER_COMMENT,
            XATTR_USER_XDGTITLE,
            XATTR_USER_XDGPUBLISHER,
        ];
        for attr in &attrs {
            assert!(attr.starts_with(b"user."));
        }
    }

    #[test]
    fn test_well_known_attrs_distinct() {
        let attrs: [&[u8]; 7] = [
            XATTR_USER_MIME_TYPE,
            XATTR_USER_CHARSET,
            XATTR_USER_CREATOR,
            XATTR_USER_CHECKSUM,
            XATTR_USER_COMMENT,
            XATTR_USER_XDGTITLE,
            XATTR_USER_XDGPUBLISHER,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_inode_file_allowed() {
        assert_ne!(USER_XATTR_INODE_FILE, 0);
    }

    #[test]
    fn test_inode_dir_allowed() {
        assert_ne!(USER_XATTR_INODE_DIR, 0);
    }

    #[test]
    fn test_inode_symlink_disallowed() {
        assert_eq!(USER_XATTR_INODE_SYMLINK, 0);
    }

    #[test]
    fn test_inode_device_disallowed() {
        assert_eq!(USER_XATTR_INODE_DEVICE, 0);
    }
}
