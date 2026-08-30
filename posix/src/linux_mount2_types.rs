//! `<linux/mount.h>` — Additional mount constants.
//!
//! Supplementary mount constants covering mount flags,
//! propagation types, and mount_setattr flags.

// ---------------------------------------------------------------------------
// Mount flags (MS_*)
// ---------------------------------------------------------------------------

/// Read-only mount.
pub const MS_RDONLY: u32 = 1;
/// No setuid.
pub const MS_NOSUID: u32 = 2;
/// No device access.
pub const MS_NODEV: u32 = 4;
/// No exec.
pub const MS_NOEXEC: u32 = 8;
/// Synchronous.
pub const MS_SYNCHRONOUS: u32 = 16;
/// Remount.
pub const MS_REMOUNT: u32 = 32;
/// Allow mandatory locks.
pub const MS_MANDLOCK: u32 = 64;
/// Directory sync.
pub const MS_DIRSYNC: u32 = 128;
/// No shell.
pub const MS_NOSYMFOLLOW: u32 = 256;
/// No atime.
pub const MS_NOATIME: u32 = 1024;
/// No diratime.
pub const MS_NODIRATIME: u32 = 2048;
/// Bind mount.
pub const MS_BIND: u32 = 4096;
/// Move subtree.
pub const MS_MOVE: u32 = 8192;
/// Recursive mount.
pub const MS_REC: u32 = 16384;
/// Silent.
pub const MS_SILENT: u32 = 32768;
/// Posixacl.
pub const MS_POSIXACL: u32 = 1 << 16;
/// Unbindable.
pub const MS_UNBINDABLE: u32 = 1 << 17;
/// Private.
pub const MS_PRIVATE: u32 = 1 << 18;
/// Slave.
pub const MS_SLAVE: u32 = 1 << 19;
/// Shared.
pub const MS_SHARED: u32 = 1 << 20;
/// Relatime.
pub const MS_RELATIME: u32 = 1 << 21;
/// Kernel mount.
pub const MS_KERNMOUNT: u32 = 1 << 22;
/// Iversion.
pub const MS_I_VERSION: u32 = 1 << 23;
/// Strictatime.
pub const MS_STRICTATIME: u32 = 1 << 24;
/// Lazytime.
pub const MS_LAZYTIME: u32 = 1 << 25;

// ---------------------------------------------------------------------------
// Mount propagation types
// ---------------------------------------------------------------------------

/// Make shared.
pub const MOUNT_ATTR_RDONLY: u32 = 0x00000001;
/// No suid.
pub const MOUNT_ATTR_NOSUID: u32 = 0x00000002;
/// No dev.
pub const MOUNT_ATTR_NODEV: u32 = 0x00000004;
/// No exec.
pub const MOUNT_ATTR_NOEXEC: u32 = 0x00000008;
/// Atime: relative.
pub const MOUNT_ATTR__ATIME: u32 = 0x00000070;
/// Relative atime.
pub const MOUNT_ATTR_RELATIME: u32 = 0x00000000;
/// No atime.
pub const MOUNT_ATTR_NOATIME: u32 = 0x00000010;
/// Strict atime.
pub const MOUNT_ATTR_STRICTATIME: u32 = 0x00000020;
/// No diratime.
pub const MOUNT_ATTR_NODIRATIME: u32 = 0x00000080;
/// ID map.
pub const MOUNT_ATTR_IDMAP: u32 = 0x00100000;
/// No symlink follow.
pub const MOUNT_ATTR_NOSYMFOLLOW: u32 = 0x00200000;

// ---------------------------------------------------------------------------
// Umount flags (MNT_*)
// ---------------------------------------------------------------------------

/// Force unmount.
pub const MNT_FORCE: u32 = 0x00000001;
/// Detach mount.
pub const MNT_DETACH: u32 = 0x00000002;
/// Expire mount.
pub const MNT_EXPIRE: u32 = 0x00000004;
/// Don't follow symlinks.
pub const UMOUNT_NOFOLLOW: u32 = 0x00000008;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ms_flags_distinct() {
        let flags = [
            MS_RDONLY,
            MS_NOSUID,
            MS_NODEV,
            MS_NOEXEC,
            MS_SYNCHRONOUS,
            MS_REMOUNT,
            MS_MANDLOCK,
            MS_DIRSYNC,
            MS_NOSYMFOLLOW,
            MS_NOATIME,
            MS_NODIRATIME,
            MS_BIND,
            MS_MOVE,
            MS_REC,
            MS_SILENT,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_propagation_flags() {
        let props = [MS_UNBINDABLE, MS_PRIVATE, MS_SLAVE, MS_SHARED];
        for i in 0..props.len() {
            for j in (i + 1)..props.len() {
                assert_eq!(props[i] & props[j], 0);
            }
        }
    }

    #[test]
    fn test_mount_attr_distinct() {
        let attrs = [
            MOUNT_ATTR_RDONLY,
            MOUNT_ATTR_NOSUID,
            MOUNT_ATTR_NODEV,
            MOUNT_ATTR_NOEXEC,
            MOUNT_ATTR_NODIRATIME,
            MOUNT_ATTR_IDMAP,
            MOUNT_ATTR_NOSYMFOLLOW,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_umount_flags_power_of_two() {
        let flags = [MNT_FORCE, MNT_DETACH, MNT_EXPIRE, UMOUNT_NOFOLLOW];
        for f in &flags {
            assert!(f.is_power_of_two(), "0x{:08x} not power of two", f);
        }
    }
}
