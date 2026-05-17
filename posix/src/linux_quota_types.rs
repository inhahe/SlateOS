//! `<linux/quota.h>` — Disk quota constants.
//!
//! Linux disk quotas limit filesystem usage (blocks and inodes) per
//! user or group. The quota subsystem tracks usage, enforces soft/hard
//! limits, and provides grace periods before enforcement.

// ---------------------------------------------------------------------------
// Quota types
// ---------------------------------------------------------------------------

/// User quota.
pub const USRQUOTA: u8 = 0;
/// Group quota.
pub const GRPQUOTA: u8 = 1;
/// Project quota.
pub const PRJQUOTA: u8 = 2;

// ---------------------------------------------------------------------------
// Quota format versions
// ---------------------------------------------------------------------------

/// Original quota format (v1).
pub const QFMT_VFS_OLD: u32 = 1;
/// VFS v0 quota format (32-bit).
pub const QFMT_VFS_V0: u32 = 2;
/// VFS v1 quota format (64-bit).
pub const QFMT_VFS_V1: u32 = 4;

// ---------------------------------------------------------------------------
// Quotactl commands (subcmd component)
// ---------------------------------------------------------------------------

/// Turn quotas on.
pub const Q_QUOTAON: u32 = 0x800002;
/// Turn quotas off.
pub const Q_QUOTAOFF: u32 = 0x800003;
/// Get quota info (limits).
pub const Q_GETQUOTA: u32 = 0x800007;
/// Set quota limits.
pub const Q_SETQUOTA: u32 = 0x800008;
/// Get quota format info.
pub const Q_GETINFO: u32 = 0x800005;
/// Set quota format info.
pub const Q_SETINFO: u32 = 0x800006;
/// Sync quota data to disk.
pub const Q_SYNC: u32 = 0x800001;

// ---------------------------------------------------------------------------
// DQF flags (dquot flags)
// ---------------------------------------------------------------------------

/// Softlimit exceeded.
pub const DQ_BLKS_SOFT: u8 = 1 << 0;
/// Hardlimit exceeded.
pub const DQ_BLKS_HARD: u8 = 1 << 1;
/// Inode soft limit exceeded.
pub const DQ_INODES_SOFT: u8 = 1 << 2;
/// Inode hard limit exceeded.
pub const DQ_INODES_HARD: u8 = 1 << 3;

// ---------------------------------------------------------------------------
// Quota state flags
// ---------------------------------------------------------------------------

/// Quota accounting enabled.
pub const DQST_ENABLED: u8 = 0;
/// Quota suspended.
pub const DQST_SUSPENDED: u8 = 1;
/// Quota enforcement on.
pub const DQST_ENFORCED: u8 = 2;

// ---------------------------------------------------------------------------
// Special limits
// ---------------------------------------------------------------------------

/// No limit marker.
pub const QUOTA_NO_LIMIT: u64 = 0xFFFF_FFFF_FFFF_FFFF;
/// Default grace period (7 days in seconds).
pub const QUOTA_DEFAULT_GRACE: u64 = 7 * 24 * 60 * 60;

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
    fn test_formats_distinct() {
        let fmts = [QFMT_VFS_OLD, QFMT_VFS_V0, QFMT_VFS_V1];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_quotactl_commands_distinct() {
        let cmds = [
            Q_QUOTAON, Q_QUOTAOFF, Q_GETQUOTA, Q_SETQUOTA,
            Q_GETINFO, Q_SETINFO, Q_SYNC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_dq_flags_no_overlap() {
        let flags = [DQ_BLKS_SOFT, DQ_BLKS_HARD, DQ_INODES_SOFT, DQ_INODES_HARD];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_grace_period() {
        assert_eq!(QUOTA_DEFAULT_GRACE, 604800);
    }
}
