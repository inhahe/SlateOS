//! `<sys/quota.h>` — `quotactl(2)` command codes and quota types.
//!
//! Disk quotas track per-user/per-group/per-project block and inode
//! consumption. `quotactl` packs a subcommand and a quota type into
//! its first argument via `QCMD(cmd, type)`. The numbers here are
//! the stable subset used by `quota`, `quotaon`, `repquota`, and
//! the ext4/xfs/f2fs kernel quota subsystems.

// ---------------------------------------------------------------------------
// `QCMD` packing
// ---------------------------------------------------------------------------

/// `QCMD(cmd, type) = (cmd << 8) | (type & 0xff)`
pub const QCMD_SHIFT: u32 = 8;
pub const QCMD_TYPE_MASK: u32 = 0xFF;

#[inline]
pub const fn qcmd(cmd: u32, ty: u32) -> u32 {
    (cmd << QCMD_SHIFT) | (ty & QCMD_TYPE_MASK)
}

// ---------------------------------------------------------------------------
// Quota types (`USRQUOTA`, `GRPQUOTA`, `PRJQUOTA`)
// ---------------------------------------------------------------------------

pub const USRQUOTA: u32 = 0;
pub const GRPQUOTA: u32 = 1;
pub const PRJQUOTA: u32 = 2;
pub const MAXQUOTAS: u32 = 3;

// ---------------------------------------------------------------------------
// `quotactl` subcommands (`Q_*`)
// ---------------------------------------------------------------------------

pub const Q_SYNC: u32 = 0x80_0001;
pub const Q_QUOTAON: u32 = 0x80_0002;
pub const Q_QUOTAOFF: u32 = 0x80_0003;
pub const Q_GETFMT: u32 = 0x80_0004;
pub const Q_GETINFO: u32 = 0x80_0005;
pub const Q_SETINFO: u32 = 0x80_0006;
pub const Q_GETQUOTA: u32 = 0x80_0007;
pub const Q_SETQUOTA: u32 = 0x80_0008;
pub const Q_GETNEXTQUOTA: u32 = 0x80_0009;

// ---------------------------------------------------------------------------
// Quota format ids
// ---------------------------------------------------------------------------

pub const QFMT_VFS_OLD: u32 = 1;
pub const QFMT_VFS_V0: u32 = 2;
pub const QFMT_OCFS2: u32 = 3;
pub const QFMT_VFS_V1: u32 = 4;

// ---------------------------------------------------------------------------
// Quota info / dquot flag bits
// ---------------------------------------------------------------------------

pub const QIF_BLIMITS: u32 = 1 << 0;
pub const QIF_SPACE: u32 = 1 << 1;
pub const QIF_ILIMITS: u32 = 1 << 2;
pub const QIF_INODES: u32 = 1 << 3;
pub const QIF_BTIME: u32 = 1 << 4;
pub const QIF_ITIME: u32 = 1 << 5;
pub const QIF_LIMITS: u32 = QIF_BLIMITS | QIF_ILIMITS;
pub const QIF_USAGE: u32 = QIF_SPACE | QIF_INODES;
pub const QIF_TIMES: u32 = QIF_BTIME | QIF_ITIME;
pub const QIF_ALL: u32 = QIF_LIMITS | QIF_USAGE | QIF_TIMES;

// ---------------------------------------------------------------------------
// Syscall
// ---------------------------------------------------------------------------

pub const NR_QUOTACTL: u32 = 179;
pub const NR_QUOTACTL_FD: u32 = 443;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qcmd_packing() {
        // QCMD packs cmd in high 24 bits, type in low 8.
        let v = qcmd(Q_GETQUOTA, USRQUOTA);
        assert_eq!(v >> 8, Q_GETQUOTA);
        assert_eq!(v & 0xFF, USRQUOTA);
        // Project quota for setquota.
        let v2 = qcmd(Q_SETQUOTA, PRJQUOTA);
        assert_eq!(v2 & 0xFF, 2);
    }

    #[test]
    fn test_quota_types_dense_0_to_2() {
        assert_eq!(USRQUOTA, 0);
        assert_eq!(GRPQUOTA, 1);
        assert_eq!(PRJQUOTA, 2);
        assert_eq!(MAXQUOTAS, 3);
        assert!(USRQUOTA < MAXQUOTAS);
        assert!(GRPQUOTA < MAXQUOTAS);
        assert!(PRJQUOTA < MAXQUOTAS);
    }

    #[test]
    fn test_q_subcommands_in_0x800000_range() {
        let c = [
            Q_SYNC,
            Q_QUOTAON,
            Q_QUOTAOFF,
            Q_GETFMT,
            Q_GETINFO,
            Q_SETINFO,
            Q_GETQUOTA,
            Q_SETQUOTA,
            Q_GETNEXTQUOTA,
        ];
        // All sit in 0x800000.. range used to avoid colliding with old XFS ops.
        for &v in c.iter() {
            assert_eq!(v & 0xFF_0000, 0x80_0000);
        }
        // And the low byte is dense 1..=9.
        for (i, &v) in c.iter().enumerate() {
            assert_eq!(v & 0xFF, (i + 1) as u32);
        }
    }

    #[test]
    fn test_qfmt_ids_distinct() {
        let f = [QFMT_VFS_OLD, QFMT_VFS_V0, QFMT_OCFS2, QFMT_VFS_V1];
        for a in 0..f.len() {
            for b in (a + 1)..f.len() {
                assert_ne!(f[a], f[b]);
            }
        }
        // VFS_V1 is the current default.
        assert_eq!(QFMT_VFS_V1, 4);
    }

    #[test]
    fn test_qif_flags_partition() {
        // The composite masks are disjoint unions of the singletons.
        assert_eq!(QIF_LIMITS, QIF_BLIMITS | QIF_ILIMITS);
        assert_eq!(QIF_USAGE, QIF_SPACE | QIF_INODES);
        assert_eq!(QIF_TIMES, QIF_BTIME | QIF_ITIME);
        // ALL covers exactly the six dense low bits.
        assert_eq!(QIF_ALL, 0x3F);
    }

    #[test]
    fn test_quotactl_syscall_numbers() {
        assert_eq!(NR_QUOTACTL, 179);
        // The fd-based variant was added in Linux 5.14.
        assert_eq!(NR_QUOTACTL_FD, 443);
        assert!(NR_QUOTACTL_FD > NR_QUOTACTL);
    }
}
