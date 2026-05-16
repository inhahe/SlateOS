//! `<linux/debugfs.h>` — debugfs virtual filesystem constants.
//!
//! debugfs is a simple RAM-based filesystem for kernel debugging.
//! Typically mounted at /sys/kernel/debug, it provides ad-hoc
//! files for exposing kernel internals during development and
//! debugging. Not intended for stable APIs.

// ---------------------------------------------------------------------------
// File permissions (typical debugfs modes)
// ---------------------------------------------------------------------------

/// Read-only for owner.
pub const DEBUGFS_MODE_RONLY: u32 = 0o400;
/// Read-write for owner.
pub const DEBUGFS_MODE_RW: u32 = 0o600;
/// Read-only for all.
pub const DEBUGFS_MODE_RONLY_ALL: u32 = 0o444;
/// Read-write for owner, read for group.
pub const DEBUGFS_MODE_RW_GRP: u32 = 0o640;

// ---------------------------------------------------------------------------
// debugfs mount point
// ---------------------------------------------------------------------------

/// Default debugfs mount point.
pub const DEBUGFS_MOUNT: &str = "/sys/kernel/debug";

// ---------------------------------------------------------------------------
// debugfs blob wrapper limits
// ---------------------------------------------------------------------------

/// Maximum blob size (practical limit for debugfs binary files).
pub const DEBUGFS_MAX_BLOB_SIZE: usize = 1024 * 1024;

// ---------------------------------------------------------------------------
// Common debugfs directory names
// ---------------------------------------------------------------------------

/// DRM debug directory.
pub const DEBUGFS_DRM: &str = "dri";
/// IEEE 802.11 (WiFi) debug directory.
pub const DEBUGFS_IEEE80211: &str = "ieee80211";
/// Block debug directory.
pub const DEBUGFS_BLOCK: &str = "block";
/// Tracing debug directory.
pub const DEBUGFS_TRACING: &str = "tracing";
/// USB debug directory.
pub const DEBUGFS_USB: &str = "usb";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            DEBUGFS_MODE_RONLY, DEBUGFS_MODE_RW,
            DEBUGFS_MODE_RONLY_ALL, DEBUGFS_MODE_RW_GRP,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_mount_point() {
        assert_eq!(DEBUGFS_MOUNT, "/sys/kernel/debug");
    }

    #[test]
    fn test_max_blob_size() {
        assert_eq!(DEBUGFS_MAX_BLOB_SIZE, 1024 * 1024);
    }

    #[test]
    fn test_dir_names_distinct() {
        let dirs = [
            DEBUGFS_DRM, DEBUGFS_IEEE80211, DEBUGFS_BLOCK,
            DEBUGFS_TRACING, DEBUGFS_USB,
        ];
        for i in 0..dirs.len() {
            for j in (i + 1)..dirs.len() {
                assert_ne!(dirs[i], dirs[j]);
            }
        }
    }
}
