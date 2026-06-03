//! `overlayfs` mount ABI.
//!
//! Overlay's `mount -t overlay` accepts a fixed set of comma-separated
//! options. Container runtimes (docker, podman, containerd) and image
//! builders pass these directly to `mount(2)`. The option names live
//! in `fs/overlayfs/super.c` and the kernel docs at
//! `Documentation/filesystems/overlayfs.rst`.

// ---------------------------------------------------------------------------
// Mount option names
// ---------------------------------------------------------------------------

pub const OVL_OPT_LOWERDIR: &str = "lowerdir";
pub const OVL_OPT_UPPERDIR: &str = "upperdir";
pub const OVL_OPT_WORKDIR: &str = "workdir";
pub const OVL_OPT_DEFAULT_PERMISSIONS: &str = "default_permissions";
pub const OVL_OPT_REDIRECT_DIR: &str = "redirect_dir";
pub const OVL_OPT_INDEX: &str = "index";
pub const OVL_OPT_NFS_EXPORT: &str = "nfs_export";
pub const OVL_OPT_XINO: &str = "xino";
pub const OVL_OPT_METACOPY: &str = "metacopy";
pub const OVL_OPT_VOLATILE: &str = "volatile";
pub const OVL_OPT_USERXATTR: &str = "userxattr";

// ---------------------------------------------------------------------------
// `redirect_dir=` values
// ---------------------------------------------------------------------------

pub const OVL_REDIRECT_OFF: &str = "off";
pub const OVL_REDIRECT_FOLLOW: &str = "follow";
pub const OVL_REDIRECT_NOFOLLOW: &str = "nofollow";
pub const OVL_REDIRECT_ON: &str = "on";

// ---------------------------------------------------------------------------
// `xino=` values
// ---------------------------------------------------------------------------

pub const OVL_XINO_OFF: &str = "off";
pub const OVL_XINO_AUTO: &str = "auto";
pub const OVL_XINO_ON: &str = "on";

// ---------------------------------------------------------------------------
// xattr names overlayfs uses to mark special files
// ---------------------------------------------------------------------------
//
// All overlay metadata lives in `trusted.overlay.*`. The `user.*`
// prefix is the alternate set used when `userxattr` is mounted (for
// rootless containers without `CAP_SYS_ADMIN`).

pub const OVL_XATTR_TRUSTED_PREFIX: &str = "trusted.overlay.";
pub const OVL_XATTR_USER_PREFIX: &str = "user.overlay.";

pub const OVL_XATTR_OPAQUE: &str = "opaque";
pub const OVL_XATTR_REDIRECT: &str = "redirect";
pub const OVL_XATTR_ORIGIN: &str = "origin";
pub const OVL_XATTR_IMPURE: &str = "impure";
pub const OVL_XATTR_NLINK: &str = "nlink";
pub const OVL_XATTR_UPPER: &str = "upper";
pub const OVL_XATTR_METACOPY: &str = "metacopy";
pub const OVL_XATTR_PROTATTR: &str = "protattr";

// ---------------------------------------------------------------------------
// Sysfs/module knobs
// ---------------------------------------------------------------------------

pub const SYSFS_OVERLAY_PARAMS: &str = "/sys/module/overlay/parameters";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_option_names_distinct() {
        let o = [
            OVL_OPT_LOWERDIR,
            OVL_OPT_UPPERDIR,
            OVL_OPT_WORKDIR,
            OVL_OPT_DEFAULT_PERMISSIONS,
            OVL_OPT_REDIRECT_DIR,
            OVL_OPT_INDEX,
            OVL_OPT_NFS_EXPORT,
            OVL_OPT_XINO,
            OVL_OPT_METACOPY,
            OVL_OPT_VOLATILE,
            OVL_OPT_USERXATTR,
        ];
        for i in 0..o.len() {
            for j in (i + 1)..o.len() {
                assert_ne!(o[i], o[j]);
            }
        }
    }

    #[test]
    fn test_redirect_modes_distinct() {
        let r = [
            OVL_REDIRECT_OFF,
            OVL_REDIRECT_FOLLOW,
            OVL_REDIRECT_NOFOLLOW,
            OVL_REDIRECT_ON,
        ];
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
    }

    #[test]
    fn test_xino_modes_distinct() {
        assert_ne!(OVL_XINO_OFF, OVL_XINO_AUTO);
        assert_ne!(OVL_XINO_AUTO, OVL_XINO_ON);
        assert_ne!(OVL_XINO_OFF, OVL_XINO_ON);
    }

    #[test]
    fn test_xattr_prefixes() {
        // The trusted.overlay.* prefix requires CAP_SYS_ADMIN; the user.*
        // form is used by rootless containers.
        assert!(OVL_XATTR_TRUSTED_PREFIX.starts_with("trusted."));
        assert!(OVL_XATTR_USER_PREFIX.starts_with("user."));
        assert!(OVL_XATTR_TRUSTED_PREFIX.ends_with("overlay."));
        assert!(OVL_XATTR_USER_PREFIX.ends_with("overlay."));
    }

    #[test]
    fn test_xattr_suffixes_distinct() {
        let x = [
            OVL_XATTR_OPAQUE,
            OVL_XATTR_REDIRECT,
            OVL_XATTR_ORIGIN,
            OVL_XATTR_IMPURE,
            OVL_XATTR_NLINK,
            OVL_XATTR_UPPER,
            OVL_XATTR_METACOPY,
            OVL_XATTR_PROTATTR,
        ];
        for i in 0..x.len() {
            for j in (i + 1)..x.len() {
                assert_ne!(x[i], x[j]);
            }
        }
    }

    #[test]
    fn test_sysfs_path() {
        assert!(SYSFS_OVERLAY_PARAMS.starts_with("/sys/module/overlay/"));
    }
}
