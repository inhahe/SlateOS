//! `<linux/quota.h>` — disk quota management.
//!
//! Disk quotas limit the amount of disk space and number of inodes
//! that a user or group can consume on a filesystem.

// ---------------------------------------------------------------------------
// Quota types
// ---------------------------------------------------------------------------

/// User quota.
pub const USRQUOTA: i32 = 0;
/// Group quota.
pub const GRPQUOTA: i32 = 1;
/// Project quota.
pub const PRJQUOTA: i32 = 2;

// ---------------------------------------------------------------------------
// Quota commands (second argument to quotactl)
// ---------------------------------------------------------------------------

/// Build quota command from (cmd, type).
pub const fn qcmd(cmd: i32, qtype: i32) -> i32 {
    (cmd << 8) | (qtype & 0xFF)
}

/// Turn on quota accounting.
pub const Q_QUOTAON: i32 = 0x800002;
/// Turn off quota accounting.
pub const Q_QUOTAOFF: i32 = 0x800003;
/// Get disk quota limits and usage.
pub const Q_GETQUOTA: i32 = 0x800007;
/// Set disk quota limits.
pub const Q_SETQUOTA: i32 = 0x800008;
/// Sync quota to disk.
pub const Q_SYNC: i32 = 0x800001;
/// Get quota info.
pub const Q_GETINFO: i32 = 0x800005;
/// Set quota info.
pub const Q_SETINFO: i32 = 0x800006;
/// Get quota format.
pub const Q_GETFMT: i32 = 0x800004;

// ---------------------------------------------------------------------------
// Quota format identifiers
// ---------------------------------------------------------------------------

/// VFS old quota format.
pub const QFMT_VFS_OLD: i32 = 1;
/// VFS v0 quota format.
pub const QFMT_VFS_V0: i32 = 2;
/// VFS v1 quota format.
pub const QFMT_VFS_V1: i32 = 4;

// ---------------------------------------------------------------------------
// Disk quota structure
// ---------------------------------------------------------------------------

/// Disk quota (dqblk).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dqblk {
    /// Block hard limit.
    pub dqb_bhardlimit: u64,
    /// Block soft limit.
    pub dqb_bsoftlimit: u64,
    /// Current space usage (bytes).
    pub dqb_curspace: u64,
    /// Inode hard limit.
    pub dqb_ihardlimit: u64,
    /// Inode soft limit.
    pub dqb_isoftlimit: u64,
    /// Current inode usage.
    pub dqb_curinodes: u64,
    /// Block grace time.
    pub dqb_btime: u64,
    /// Inode grace time.
    pub dqb_itime: u64,
    /// Valid fields mask.
    pub dqb_valid: u32,
    /// Padding.
    _pad: u32,
}

impl Dqblk {
    /// Create a zeroed quota block.
    pub fn zeroed() -> Self {
        // SAFETY: All-zero is valid for this repr(C) struct.
        unsafe { core::mem::zeroed() }
    }
}

// ---------------------------------------------------------------------------
// Valid field flags (for dqb_valid)
// ---------------------------------------------------------------------------

/// Block hard limit is valid.
pub const QIF_BLIMITS: u32 = 1;
/// Space usage is valid.
pub const QIF_SPACE: u32 = 2;
/// Inode limits are valid.
pub const QIF_ILIMITS: u32 = 4;
/// Inode usage is valid.
pub const QIF_INODES: u32 = 8;
/// Block grace time is valid.
pub const QIF_BTIME: u32 = 16;
/// Inode grace time is valid.
pub const QIF_ITIME: u32 = 32;
/// All limits are valid.
pub const QIF_LIMITS: u32 = QIF_BLIMITS | QIF_ILIMITS;
/// All usage is valid.
pub const QIF_USAGE: u32 = QIF_SPACE | QIF_INODES;
/// All times are valid.
pub const QIF_TIMES: u32 = QIF_BTIME | QIF_ITIME;
/// Everything is valid.
pub const QIF_ALL: u32 = QIF_LIMITS | QIF_USAGE | QIF_TIMES;

// ---------------------------------------------------------------------------
// Re-export quotactl
// ---------------------------------------------------------------------------

pub use crate::sys_quota::quotactl;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quota_types() {
        assert_eq!(USRQUOTA, 0);
        assert_eq!(GRPQUOTA, 1);
        assert_eq!(PRJQUOTA, 2);
    }

    #[test]
    fn test_dqblk_size() {
        assert_eq!(core::mem::size_of::<Dqblk>(), 72);
    }

    #[test]
    fn test_dqblk_zeroed() {
        let dq = Dqblk::zeroed();
        assert_eq!(dq.dqb_bhardlimit, 0);
        assert_eq!(dq.dqb_curspace, 0);
        assert_eq!(dq.dqb_curinodes, 0);
    }

    #[test]
    fn test_qif_flags() {
        assert_eq!(QIF_ALL, QIF_LIMITS | QIF_USAGE | QIF_TIMES);
        assert_eq!(QIF_LIMITS, QIF_BLIMITS | QIF_ILIMITS);
        assert_eq!(QIF_USAGE, QIF_SPACE | QIF_INODES);
    }

    #[test]
    fn test_quotactl_stub() {
        let ret = quotactl(0, core::ptr::null(), 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_format_ids_distinct() {
        assert_ne!(QFMT_VFS_OLD, QFMT_VFS_V0);
        assert_ne!(QFMT_VFS_V0, QFMT_VFS_V1);
    }
}
