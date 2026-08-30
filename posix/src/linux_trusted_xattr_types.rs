//! `<linux/xattr.h>` — Trusted namespace extended attribute constants.
//!
//! The "trusted." xattr namespace is accessible only to processes
//! with `CAP_SYS_ADMIN`. These attributes are used by filesystem
//! utilities, overlayfs, and container runtimes for metadata that
//! unprivileged users should not be able to read or modify.

// ---------------------------------------------------------------------------
// Well-known trusted.* attributes
// ---------------------------------------------------------------------------

/// OverlayFS opaque directory marker.
pub const XATTR_TRUSTED_OVL_OPAQUE: &[u8] = b"trusted.overlay.opaque";
/// OverlayFS redirect attribute.
pub const XATTR_TRUSTED_OVL_REDIRECT: &[u8] = b"trusted.overlay.redirect";
/// OverlayFS origin attribute.
pub const XATTR_TRUSTED_OVL_ORIGIN: &[u8] = b"trusted.overlay.origin";
/// OverlayFS impure directory marker.
pub const XATTR_TRUSTED_OVL_IMPURE: &[u8] = b"trusted.overlay.impure";
/// OverlayFS nlink attribute.
pub const XATTR_TRUSTED_OVL_NLINK: &[u8] = b"trusted.overlay.nlink";
/// OverlayFS upper attribute.
pub const XATTR_TRUSTED_OVL_UPPER: &[u8] = b"trusted.overlay.upper";
/// OverlayFS metacopy attribute.
pub const XATTR_TRUSTED_OVL_METACOPY: &[u8] = b"trusted.overlay.metacopy";
/// OverlayFS protected symlink attribute.
pub const XATTR_TRUSTED_OVL_PROTATTR: &[u8] = b"trusted.overlay.protattr";

// ---------------------------------------------------------------------------
// OverlayFS opaque values
// ---------------------------------------------------------------------------

/// Value indicating a directory is opaque (hides lower layers).
pub const OVL_OPAQUE_TRUE: &[u8] = b"y";
/// Value indicating a directory is not opaque.
pub const OVL_OPAQUE_FALSE: &[u8] = b"n";

// ---------------------------------------------------------------------------
// Trusted namespace limits
// ---------------------------------------------------------------------------

/// Maximum trusted xattr name length (after "trusted." prefix).
pub const TRUSTED_XATTR_NAME_MAX: u32 = 247;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ovl_attrs_start_with_trusted() {
        let attrs = [
            XATTR_TRUSTED_OVL_OPAQUE,
            XATTR_TRUSTED_OVL_REDIRECT,
            XATTR_TRUSTED_OVL_ORIGIN,
            XATTR_TRUSTED_OVL_IMPURE,
            XATTR_TRUSTED_OVL_NLINK,
            XATTR_TRUSTED_OVL_UPPER,
            XATTR_TRUSTED_OVL_METACOPY,
            XATTR_TRUSTED_OVL_PROTATTR,
        ];
        for attr in &attrs {
            assert!(attr.starts_with(b"trusted."));
        }
    }

    #[test]
    fn test_ovl_attrs_start_with_overlay() {
        let attrs = [
            XATTR_TRUSTED_OVL_OPAQUE,
            XATTR_TRUSTED_OVL_REDIRECT,
            XATTR_TRUSTED_OVL_ORIGIN,
            XATTR_TRUSTED_OVL_IMPURE,
            XATTR_TRUSTED_OVL_NLINK,
            XATTR_TRUSTED_OVL_UPPER,
            XATTR_TRUSTED_OVL_METACOPY,
            XATTR_TRUSTED_OVL_PROTATTR,
        ];
        for attr in &attrs {
            assert!(attr.starts_with(b"trusted.overlay."));
        }
    }

    #[test]
    fn test_ovl_attrs_distinct() {
        let attrs: [&[u8]; 8] = [
            XATTR_TRUSTED_OVL_OPAQUE,
            XATTR_TRUSTED_OVL_REDIRECT,
            XATTR_TRUSTED_OVL_ORIGIN,
            XATTR_TRUSTED_OVL_IMPURE,
            XATTR_TRUSTED_OVL_NLINK,
            XATTR_TRUSTED_OVL_UPPER,
            XATTR_TRUSTED_OVL_METACOPY,
            XATTR_TRUSTED_OVL_PROTATTR,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_opaque_values() {
        assert_eq!(OVL_OPAQUE_TRUE, b"y");
        assert_eq!(OVL_OPAQUE_FALSE, b"n");
        assert_ne!(OVL_OPAQUE_TRUE, OVL_OPAQUE_FALSE);
    }

    #[test]
    fn test_trusted_name_max() {
        // 255 (XATTR_NAME_MAX) minus 8 ("trusted.") = 247
        assert_eq!(TRUSTED_XATTR_NAME_MAX, 247);
    }
}
