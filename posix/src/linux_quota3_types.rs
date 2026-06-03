//! `<linux/quota.h>` — Additional disk quota constants (part 3).
//!
//! Supplementary disk quota constants covering quota types,
//! format IDs, and quota state flags.

// ---------------------------------------------------------------------------
// Quota types
// ---------------------------------------------------------------------------

/// User quota.
pub const USRQUOTA: u32 = 0;
/// Group quota.
pub const GRPQUOTA: u32 = 1;
/// Project quota.
pub const PRJQUOTA: u32 = 2;

// ---------------------------------------------------------------------------
// Quota format IDs
// ---------------------------------------------------------------------------

/// Original quota format.
pub const QFMT_VFS_OLD: u32 = 1;
/// VFS v0 quota format.
pub const QFMT_VFS_V0: u32 = 2;
/// VFS v1 quota format.
pub const QFMT_VFS_V1: u32 = 4;

// ---------------------------------------------------------------------------
// Quota flags (dqi_flags)
// ---------------------------------------------------------------------------

/// Root squash.
pub const DQF_ROOT_SQUASH: u32 = 1 << 0;
/// Quota is in admin state.
pub const DQF_SYS_FILE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Quota state flags (for Q_GETINFO)
// ---------------------------------------------------------------------------

/// Enforcing user quota.
pub const DQS_USER_ENABLED: u32 = 1 << 0;
/// Enforcing group quota.
pub const DQS_GROUP_ENABLED: u32 = 1 << 1;
/// Enforcing project quota.
pub const DQS_PROJECT_ENABLED: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Quota commands (for quotactl)
// ---------------------------------------------------------------------------

/// Turn on quota.
pub const Q_QUOTAON: u32 = 0x800002;
/// Turn off quota.
pub const Q_QUOTAOFF: u32 = 0x800003;
/// Get quota info.
pub const Q_GETINFO: u32 = 0x800005;
/// Set quota info.
pub const Q_SETINFO: u32 = 0x800006;
/// Get disk quota.
pub const Q_GETQUOTA: u32 = 0x800007;
/// Set disk quota.
pub const Q_SETQUOTA: u32 = 0x800008;
/// Sync quotas.
pub const Q_SYNC: u32 = 0x800001;
/// Get format.
pub const Q_GETFMT: u32 = 0x800004;

// ---------------------------------------------------------------------------
// Quota block size
// ---------------------------------------------------------------------------

/// Quota block size.
pub const QIF_DQBLKSIZE_BITS: u32 = 10;
/// Quota block size (1024 bytes).
pub const QIF_DQBLKSIZE: u32 = 1 << QIF_DQBLKSIZE_BITS;

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
    fn test_format_ids_distinct() {
        let fmts = [QFMT_VFS_OLD, QFMT_VFS_V0, QFMT_VFS_V1];
        for i in 0..fmts.len() {
            for j in (i + 1)..fmts.len() {
                assert_ne!(fmts[i], fmts[j]);
            }
        }
    }

    #[test]
    fn test_dqf_flags_no_overlap() {
        assert_eq!(DQF_ROOT_SQUASH & DQF_SYS_FILE, 0);
    }

    #[test]
    fn test_state_flags_no_overlap() {
        let flags = [DQS_USER_ENABLED, DQS_GROUP_ENABLED, DQS_PROJECT_ENABLED];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_commands_distinct() {
        let cmds = [
            Q_QUOTAON, Q_QUOTAOFF, Q_GETINFO, Q_SETINFO, Q_GETQUOTA, Q_SETQUOTA, Q_SYNC, Q_GETFMT,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_block_size() {
        assert_eq!(QIF_DQBLKSIZE, 1024);
    }
}
