//! `<sys/quota.h>` — Disk quota command and flag constants.
//!
//! The `quotactl()` syscall manages filesystem disk quotas. These
//! constants define the commands (get/set limits, sync, enable/disable)
//! and the quota type identifiers (user, group, project).

// ---------------------------------------------------------------------------
// Quota types
// ---------------------------------------------------------------------------

/// User quota.
pub const USRQUOTA: u32 = 0;
/// Group quota.
pub const GRPQUOTA: u32 = 1;
/// Project quota (ext4, XFS).
pub const PRJQUOTA: u32 = 2;

// ---------------------------------------------------------------------------
// quotactl() commands (Q_* / QCMD encoding)
// ---------------------------------------------------------------------------

/// Get quota format for filesystem.
pub const Q_GETFMT: u32 = 0x800004;
/// Get quota info (grace periods, flags).
pub const Q_GETINFO: u32 = 0x800005;
/// Set quota info.
pub const Q_SETINFO: u32 = 0x800006;
/// Get quota limits and usage for an ID.
pub const Q_GETQUOTA: u32 = 0x800007;
/// Set quota limits for an ID.
pub const Q_SETQUOTA: u32 = 0x800008;
/// Sync quotas to disk.
pub const Q_SYNC: u32 = 0x800001;
/// Turn on quotas for a filesystem.
pub const Q_QUOTAON: u32 = 0x800002;
/// Turn off quotas for a filesystem.
pub const Q_QUOTAOFF: u32 = 0x800003;
/// Get next quota entry (iterate).
pub const Q_GETNEXTQUOTA: u32 = 0x800009;

// ---------------------------------------------------------------------------
// Quota format identifiers
// ---------------------------------------------------------------------------

/// VFS old quota format (v1).
pub const QFMT_VFS_OLD: u32 = 1;
/// VFS v0 quota format.
pub const QFMT_VFS_V0: u32 = 2;
/// VFS v1 quota format (current).
pub const QFMT_VFS_V1: u32 = 4;

// ---------------------------------------------------------------------------
// Quota flags (dqi_flags)
// ---------------------------------------------------------------------------

/// Enforce quota limits (hard).
pub const DQF_ROOT_SQUASH: u32 = 0x01;
/// Grace period applies.
pub const DQF_SYS_FILE: u32 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_types_distinct() {
        assert_ne!(USRQUOTA, GRPQUOTA);
        assert_ne!(GRPQUOTA, PRJQUOTA);
        assert_ne!(USRQUOTA, PRJQUOTA);
    }

    #[test]
    fn test_quota_type_values() {
        assert_eq!(USRQUOTA, 0);
        assert_eq!(GRPQUOTA, 1);
        assert_eq!(PRJQUOTA, 2);
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            Q_SYNC, Q_QUOTAON, Q_QUOTAOFF, Q_GETFMT,
            Q_GETINFO, Q_SETINFO, Q_GETQUOTA, Q_SETQUOTA,
            Q_GETNEXTQUOTA,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
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
    fn test_dqf_flags() {
        assert_eq!(DQF_ROOT_SQUASH & DQF_SYS_FILE, 0);
    }
}
