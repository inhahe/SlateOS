//! `<sys/quota.h>` — disk quota definitions.
//!
//! Provides constants, structures, and the `quotactl()` entry point
//! for managing filesystem disk quotas.
//!
//! ## What `quotactl` answers
//!
//! No filesystem here supports quotas, so the call can only fail -- and it
//! fails where Linux 6.6's does, in its order (fs/quota/quota.c):
//!
//! 1. A quota type past `PRJQUOTA` (the low byte of `cmd`) is `EINVAL`, for
//!    every subcommand, `Q_SYNC` included.
//! 2. With no `special`, `Q_SYNC` syncs every filesystem with quotas -- none
//!    -- and returns 0; any other subcommand is `ENODEV`.
//! 3. `special` is looked up as a block device (`lookup_bdev`): a name that
//!    cannot be read is `EFAULT`, one that does not resolve fails as `stat`
//!    fails, and anything but a block device is `ENOTBLK`.
//! 4. A block device's filesystem has no quota operations: `ENOSYS`, which is
//!    `do_quotactl`'s first answer -- before the subcommand, the caller's
//!    capability or `addr` is looked at, so none of them is.
//!
//! Until 2026-09-26 this validated in an order of its own: unknown
//! subcommands and `addr` before the device, a NULL `special` as `EFAULT` (a
//! `getname` that Linux never reaches), `CAP_SYS_ADMIN` before `ENOSYS`, and
//! `Q_SYNC`'s type not at all (`B-D-QUOTACTL-WAS-NOT-LINUXS`).

use crate::errno;

// ---------------------------------------------------------------------------
// Quota commands
// ---------------------------------------------------------------------------

/// Sync disk copy of quotas.
pub const Q_SYNC: i32 = 0x800001;

/// Enable quota enforcement.
pub const Q_QUOTAON: i32 = 0x800002;

/// Disable quota enforcement.
pub const Q_QUOTAOFF: i32 = 0x800003;

/// Get quota format.
pub const Q_GETFMT: i32 = 0x800004;

/// Get quota information.
pub const Q_GETINFO: i32 = 0x800005;

/// Set quota information.
pub const Q_SETINFO: i32 = 0x800006;

/// Get disk quota limits and current usage.
pub const Q_GETQUOTA: i32 = 0x800007;

/// Set disk quota limits.
pub const Q_SETQUOTA: i32 = 0x800008;

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
// Quota limits
// ---------------------------------------------------------------------------

/// Number of supported quota types (matches Linux `MAXQUOTAS`).
pub const MAXQUOTAS: i32 = 3;

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
pub const QIF_ALL: u32 = QIF_BLIMITS | QIF_SPACE | QIF_ILIMITS | QIF_INODES | QIF_BTIME | QIF_ITIME;

/// All limit fields valid (combination flag used by `Q_SETQUOTA`).
pub const QIF_LIMITS: u32 = QIF_BLIMITS | QIF_ILIMITS;

/// All usage fields valid.
pub const QIF_USAGE: u32 = QIF_SPACE | QIF_INODES;

/// All time-limit fields valid.
pub const QIF_TIMES: u32 = QIF_BTIME | QIF_ITIME;

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Decode the user-visible `cmd` word into (subcommand, quota-type).
///
/// Mirrors the `QCMD(cmd, type) = ((cmd) << 8) | ((type) & 0xff)`
/// macro from `<sys/quota.h>` — `subcmd = cmd >> 8`, `qtype = cmd & 0xff`.
const fn split_cmd(cmd: i32) -> (i32, i32) {
    let u = cmd as u32;
    let subcmd = (u >> 8) as i32;
    let qtype = (u & 0xFFu32) as i32;
    (subcmd, qtype)
}

/// `QCMD(cmd, type)` — encode a user-facing quotactl `cmd` word.
///
/// Exposed as a `const fn` so test code and callers don't need to
/// open-code the shift.
#[must_use]
pub const fn qcmd(cmd: i32, qtype: i32) -> i32 {
    let u = ((cmd as u32) << 8) | ((qtype as u32) & 0xFFu32);
    u as i32
}

/// Is `qtype` a valid quota type for the commands that require one?
const fn is_valid_qtype(qtype: i32) -> bool {
    qtype >= 0 && qtype < MAXQUOTAS
}

// ---------------------------------------------------------------------------
// quotactl()
// ---------------------------------------------------------------------------

/// Manipulate disk quotas -- which no filesystem here supports.
///
/// See the module docs for what it answers, and in what order: `EINVAL`,
/// then for no `special` 0 (`Q_SYNC`) or `ENODEV`, then what looking the
/// device up gives (`EFAULT`, `stat`'s errors, `ENOTBLK`), then `ENOSYS`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn quotactl(cmd: i32, special: *const u8, _id: i32, _addr: *mut u8) -> i32 {
    match check(cmd, special, block_device_mode) {
        Ok(()) => 0,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// The mode of the file `special` names, following links -- `lookup_bdev`'s
/// `kern_path` -- or `stat`'s errno.
fn block_device_mode(special: *const u8) -> Result<u32, i32> {
    let mut st = crate::stat::Stat::default();
    if crate::file::stat(special, &raw mut st) != 0 {
        return Err(errno::get_errno());
    }
    Ok(st.st_mode)
}

/// `quotactl`'s checks, with the device lookup as a parameter so the tests
/// can stand in for a filesystem.
fn check(
    cmd: i32,
    special: *const u8,
    lookup: impl FnOnce(*const u8) -> Result<u32, i32>,
) -> Result<(), i32> {
    let (subcmd, qtype) = split_cmd(cmd);
    if !is_valid_qtype(qtype) {
        return Err(errno::EINVAL);
    }
    if special.is_null() {
        // `quota_sync_all` over the filesystems with quotas: none.
        return if subcmd == Q_SYNC {
            Ok(())
        } else {
            Err(errno::ENODEV)
        };
    }
    // `getname(special)`: a name in the kernel half cannot be read.
    if !crate::uio::access_ok(special.addr(), 1) {
        return Err(errno::EFAULT);
    }
    let mode = lookup(special)?;
    if mode & crate::fcntl::S_IFMT != crate::fcntl::S_IFBLK {
        return Err(errno::ENOTBLK);
    }
    // `do_quotactl`: the filesystem has no quota operations.
    Err(errno::ENOSYS)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
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
            Q_QUOTAON, Q_QUOTAOFF, Q_GETQUOTA, Q_SETQUOTA, Q_GETINFO, Q_SETINFO, Q_GETFMT, Q_SYNC,
        ];
        for i in 0..cmds.len() {
            for j in (i + 1)..cmds.len() {
                assert_ne!(cmds[i], cmds[j]);
            }
        }
    }

    #[test]
    fn test_quota_types() {
        assert_eq!((USRQUOTA, GRPQUOTA, PRJQUOTA, MAXQUOTAS), (0, 1, 2, 3));
    }

    #[test]
    fn test_qif_all() {
        assert_eq!(QIF_ALL, 63);
    }

    #[test]
    fn test_qcmd_round_trips() {
        assert_eq!(
            split_cmd(qcmd(Q_GETQUOTA, GRPQUOTA)),
            (Q_GETQUOTA, GRPQUOTA)
        );
    }

    const DEV: *const u8 = b"/dev/vda\0".as_ptr();

    fn never(_: *const u8) -> Result<u32, i32> {
        panic!("the device was looked up")
    }

    fn block(_: *const u8) -> Result<u32, i32> {
        Ok(crate::fcntl::S_IFBLK | 0o660)
    }

    #[test]
    fn the_type_is_checked_first_even_for_q_sync() {
        for sub in [Q_SYNC, Q_GETQUOTA, 0x1234] {
            assert_eq!(
                check(qcmd(sub, MAXQUOTAS), core::ptr::null(), never),
                Err(errno::EINVAL)
            );
            assert_eq!(check(qcmd(sub, 255), DEV, never), Err(errno::EINVAL));
        }
    }

    #[test]
    fn no_device_is_enodev_but_for_q_sync() {
        // THE PASS'S NULL SITE: this was EFAULT, for a getname Linux never
        // reaches.
        assert_eq!(
            check(qcmd(Q_SYNC, USRQUOTA), core::ptr::null(), never),
            Ok(())
        );
        for sub in [Q_QUOTAON, Q_GETQUOTA, Q_SETQUOTA, Q_GETINFO, 0x1234] {
            assert_eq!(
                check(qcmd(sub, USRQUOTA), core::ptr::null(), never),
                Err(errno::ENODEV),
                "{sub:#x}: even an unknown subcommand"
            );
        }
        errno::set_errno(0);
        assert_eq!(
            quotactl(
                qcmd(Q_GETQUOTA, USRQUOTA),
                core::ptr::null(),
                0,
                core::ptr::null_mut()
            ),
            -1
        );
        assert_eq!(errno::get_errno(), errno::ENODEV);
    }

    #[test]
    fn the_device_is_looked_up_as_a_block_device() {
        let kernel_half = 0xFFFF_8000_0000_0000_usize as *const u8;
        assert_eq!(
            check(qcmd(Q_GETQUOTA, USRQUOTA), kernel_half, never),
            Err(errno::EFAULT)
        );
        assert_eq!(
            check(qcmd(Q_GETQUOTA, USRQUOTA), DEV, |_| Err(errno::ENOENT)),
            Err(errno::ENOENT),
            "stat's answer"
        );
        assert_eq!(
            check(qcmd(Q_GETQUOTA, USRQUOTA), DEV, |_| Ok(
                crate::fcntl::S_IFREG | 0o644
            )),
            Err(errno::ENOTBLK)
        );
        assert_eq!(
            check(qcmd(Q_GETQUOTA, USRQUOTA), DEV, |_| Ok(
                crate::fcntl::S_IFDIR | 0o755
            )),
            Err(errno::ENOTBLK)
        );
    }

    #[test]
    fn a_block_device_has_no_quotas_whatever_is_asked() {
        // do_quotactl's ENOSYS comes before the subcommand, the capability
        // and addr, so a NULL addr, an unknown subcommand and Q_SYNC on a
        // device all get it.
        for sub in [Q_GETQUOTA, Q_QUOTAON, Q_SYNC, 0x1234] {
            assert_eq!(
                check(qcmd(sub, GRPQUOTA), DEV, block),
                Err(errno::ENOSYS),
                "{sub:#x}"
            );
        }
    }
}
