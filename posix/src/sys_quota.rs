//! `<sys/quota.h>` — disk quota definitions.
//!
//! Provides constants, structures, and the `quotactl()` stub for
//! managing filesystem disk quotas.

use crate::errno;

// ---------------------------------------------------------------------------
// Quota commands
// ---------------------------------------------------------------------------

/// Enable quota enforcement.
pub const Q_QUOTAON: i32 = 0x800002;

/// Disable quota enforcement.
pub const Q_QUOTAOFF: i32 = 0x800003;

/// Get disk quota limits and current usage.
pub const Q_GETQUOTA: i32 = 0x800007;

/// Set disk quota limits.
pub const Q_SETQUOTA: i32 = 0x800008;

/// Get quota information.
pub const Q_GETINFO: i32 = 0x800005;

/// Set quota information.
pub const Q_SETINFO: i32 = 0x800006;

/// Get quota format.
pub const Q_GETFMT: i32 = 0x800004;

/// Sync disk copy of quota.
pub const Q_SYNC: i32 = 0x800001;

// ---------------------------------------------------------------------------
// Quota types
// ---------------------------------------------------------------------------

/// User quota.
pub const USRQUOTA: i32 = 0;

/// Group quota.
pub const GRPQUOTA: i32 = 1;

// ---------------------------------------------------------------------------
// Quota limits
// ---------------------------------------------------------------------------

/// Maximum quota format name length.
pub const MAXQUOTAS: i32 = 2;

// ---------------------------------------------------------------------------
// Disk quota structure
// ---------------------------------------------------------------------------

/// On-disk quota structure (matches Linux `dqblk`).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Dqblk {
    /// Hard limit for disk blocks.
    pub dqb_bhardlimit: u64,
    /// Soft limit for disk blocks.
    pub dqb_bsoftlimit: u64,
    /// Current block count.
    pub dqb_curspace: u64,
    /// Hard limit for inodes.
    pub dqb_ihardlimit: u64,
    /// Soft limit for inodes.
    pub dqb_isoftlimit: u64,
    /// Current inode count.
    pub dqb_curinodes: u64,
    /// Time limit for excessive block use.
    pub dqb_btime: u64,
    /// Time limit for excessive inode use.
    pub dqb_itime: u64,
    /// Valid fields bitmask.
    pub dqb_valid: u32,
    /// Padding.
    _pad: u32,
}

// ---------------------------------------------------------------------------
// Valid field bits for dqb_valid
// ---------------------------------------------------------------------------

/// Block hard limit is valid.
pub const QIF_BLIMITS: u32 = 1;

/// Block usage is valid.
pub const QIF_SPACE: u32 = 2;

/// Inode hard limit is valid.
pub const QIF_ILIMITS: u32 = 4;

/// Inode usage is valid.
pub const QIF_INODES: u32 = 8;

/// Block time limit is valid.
pub const QIF_BTIME: u32 = 16;

/// Inode time limit is valid.
pub const QIF_ITIME: u32 = 32;

/// All fields valid.
pub const QIF_ALL: u32 = QIF_BLIMITS | QIF_SPACE | QIF_ILIMITS
    | QIF_INODES | QIF_BTIME | QIF_ITIME;

// ---------------------------------------------------------------------------
// quotactl()
// ---------------------------------------------------------------------------

/// Manipulate disk quotas.
///
/// Stub — always returns -1 with `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn quotactl(
    _cmd: i32,
    _special: *const u8,
    _id: i32,
    _addr: *mut u8,
) -> i32 {
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dqblk_size() {
        // 8 u64 fields (64 bytes) + 1 u32 + 1 u32 pad = 72 bytes.
        assert_eq!(core::mem::size_of::<Dqblk>(), 72);
    }

    #[test]
    fn test_quota_commands_distinct() {
        let cmds = [
            Q_QUOTAON, Q_QUOTAOFF, Q_GETQUOTA, Q_SETQUOTA,
            Q_GETINFO, Q_SETINFO, Q_GETFMT, Q_SYNC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_quota_types() {
        assert_eq!(USRQUOTA, 0);
        assert_eq!(GRPQUOTA, 1);
        assert_ne!(USRQUOTA, GRPQUOTA);
    }

    #[test]
    fn test_qif_all() {
        assert_eq!(QIF_ALL, 63);
    }

    #[test]
    fn test_qif_bits_powers_of_two() {
        let bits = [QIF_BLIMITS, QIF_SPACE, QIF_ILIMITS, QIF_INODES, QIF_BTIME, QIF_ITIME];
        for &b in &bits {
            assert!(b.is_power_of_two(), "{b} should be power of 2");
        }
    }

    #[test]
    fn test_quotactl_stub() {
        let ret = quotactl(Q_GETQUOTA, core::ptr::null(), 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
    }

    #[test]
    fn test_dqblk_zeroed() {
        // SAFETY: Dqblk is repr(C) with all-numeric fields, safe to zero.
        let dq: Dqblk = unsafe { core::mem::zeroed() };
        assert_eq!(dq.dqb_bhardlimit, 0);
        assert_eq!(dq.dqb_curspace, 0);
        assert_eq!(dq.dqb_valid, 0);
    }
}
