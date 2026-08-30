//! `<sys/quota.h>` — Disk quota control constants.
//!
//! Disk quotas limit the amount of disk space and number of
//! inodes a user or group may consume.  These constants define
//! the `quotactl()` commands, quota types, and format identifiers.

// ---------------------------------------------------------------------------
// quotactl() command construction
// ---------------------------------------------------------------------------

/// Shift for encoding the subcommand in quotactl cmd.
pub const SUBCMDSHIFT: u32 = 8;
/// Mask for extracting the subcommand.
pub const SUBCMDMASK: u32 = 0x00FF;

// ---------------------------------------------------------------------------
// quotactl() subcommands (Q_*)
// ---------------------------------------------------------------------------

/// Turn on quotas for a filesystem.
pub const Q_QUOTAON: u32 = 0x0100;
/// Turn off quotas for a filesystem.
pub const Q_QUOTAOFF: u32 = 0x0200;
/// Get disk quota limits and usage.
pub const Q_GETQUOTA: u32 = 0x0300;
/// Set disk quota limits.
pub const Q_SETQUOTA: u32 = 0x0400;
/// Get quota format info.
pub const Q_GETINFO: u32 = 0x0500;
/// Set quota format info.
pub const Q_SETINFO: u32 = 0x0600;
/// Get filesystem quota state.
pub const Q_GETFMT: u32 = 0x0400;
/// Sync quotas to disk.
pub const Q_SYNC: u32 = 0x0600;

// ---------------------------------------------------------------------------
// Quota types (id_type parameter)
// ---------------------------------------------------------------------------

/// User quota.
pub const USRQUOTA: u32 = 0;
/// Group quota.
pub const GRPQUOTA: u32 = 1;
/// Project quota (XFS/ext4).
pub const PRJQUOTA: u32 = 2;

// ---------------------------------------------------------------------------
// Quota format identifiers
// ---------------------------------------------------------------------------

/// VFS old quota format.
pub const QFMT_VFS_OLD: u32 = 1;
/// VFS v0 quota format.
pub const QFMT_VFS_V0: u32 = 2;
/// VFS v1 quota format (ext4 journaled quotas).
pub const QFMT_VFS_V1: u32 = 4;

// ---------------------------------------------------------------------------
// Quota flags (dqi_flags)
// ---------------------------------------------------------------------------

/// Root squash enabled for this quota.
pub const DQF_ROOT_SQUASH: u32 = 1 << 0;
/// System file for quota.
pub const DQF_SYS_FILE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// dqblk validity flags (for Q_GETQUOTA/Q_SETQUOTA)
// ---------------------------------------------------------------------------

/// Block hard limit is valid.
pub const QIF_BLIMITS: u32 = 1 << 0;
/// Block usage is valid.
pub const QIF_SPACE: u32 = 1 << 1;
/// Inode hard limit is valid.
pub const QIF_ILIMITS: u32 = 1 << 2;
/// Inode usage is valid.
pub const QIF_INODES: u32 = 1 << 3;
/// Block grace time is valid.
pub const QIF_BTIME: u32 = 1 << 4;
/// Inode grace time is valid.
pub const QIF_ITIME: u32 = 1 << 5;
/// All limits are valid.
pub const QIF_LIMITS: u32 = QIF_BLIMITS | QIF_ILIMITS;
/// All usage fields are valid.
pub const QIF_USAGE: u32 = QIF_SPACE | QIF_INODES;
/// All times are valid.
pub const QIF_TIMES: u32 = QIF_BTIME | QIF_ITIME;
/// All fields are valid.
pub const QIF_ALL: u32 = QIF_LIMITS | QIF_USAGE | QIF_TIMES;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_types_distinct() {
        let types = [USRQUOTA, GRPQUOTA, PRJQUOTA];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_usrquota_is_zero() {
        assert_eq!(USRQUOTA, 0);
    }

    #[test]
    fn test_format_ids_distinct() {
        let fmts = [QFMT_VFS_OLD, QFMT_VFS_V0, QFMT_VFS_V1];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_validity_flags_no_overlap() {
        let flags = [
            QIF_BLIMITS,
            QIF_SPACE,
            QIF_ILIMITS,
            QIF_INODES,
            QIF_BTIME,
            QIF_ITIME,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_validity_flags_powers_of_two() {
        let flags = [
            QIF_BLIMITS,
            QIF_SPACE,
            QIF_ILIMITS,
            QIF_INODES,
            QIF_BTIME,
            QIF_ITIME,
        ];
        for f in flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_all_covers_all() {
        assert_eq!(
            QIF_ALL,
            QIF_BLIMITS | QIF_SPACE | QIF_ILIMITS | QIF_INODES | QIF_BTIME | QIF_ITIME
        );
    }

    #[test]
    fn test_dqf_flags_no_overlap() {
        assert_eq!(DQF_ROOT_SQUASH & DQF_SYS_FILE, 0);
    }

    #[test]
    fn test_subcmdshift() {
        assert_eq!(SUBCMDSHIFT, 8);
    }
}
