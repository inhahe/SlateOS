//! `<sys/quota.h>` — disk quota definitions.
//!
//! Provides constants, structures, and the `quotactl()` entry point
//! for managing filesystem disk quotas.
//!
//! ## Backend status
//!
//! Our kernel does not implement disk quotas yet, so the runtime
//! behaviour of `quotactl()` is "validate the call carefully, then
//! report 'no quota backend'".  The validation itself is real and
//! matches Linux:
//!
//! * Cleanly decodes `cmd` into (subcommand, quota-type).
//! * Rejects unknown subcommands with `EINVAL`.
//! * Rejects quota types outside `USRQUOTA`/`GRPQUOTA`/`PRJQUOTA`
//!   with `EINVAL` (the `quota_type` byte is ignored only by
//!   `Q_SYNC`).
//! * Rejects `NULL` `addr` for commands that read or write through
//!   it with `EFAULT`.
//! * Rejects `NULL` `special` for commands that name a filesystem
//!   with `EFAULT`.  Linux's `quotactl()` reaches `user_path_at()`
//!   which calls `getname()` → `strncpy_from_user(NULL)` → `-EFAULT`,
//!   so the NULL-pointer signal is `EFAULT`, not `ENODEV` (the
//!   latter is reserved for the case where the path resolves but is
//!   not a quota-enabled filesystem — `quotactl_block` returning
//!   `-ENODEV`).
//! * `Q_SYNC` with `special == NULL` is the "sync every filesystem"
//!   form; we return `0` immediately because there are no quota
//!   files to flush.
//! * Every other validated call returns `-1` with `errno = ENOSYS`,
//!   matching a kernel built without `CONFIG_QUOTA`.  Programs that
//!   already gracefully fall back on `ENOSYS` (essentially every
//!   real-world quota consumer — `quotaon(8)`, `quota(1)`,
//!   `repquota(8)`, NFS mount helpers) keep working.
//!
//! When we add a real quota backend, the post-validation arm of
//! `quotactl()` is the only thing that needs replacing.

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
pub const QIF_ALL: u32 = QIF_BLIMITS | QIF_SPACE | QIF_ILIMITS
    | QIF_INODES | QIF_BTIME | QIF_ITIME;

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

/// Is `subcmd` one of the eight defined quotactl operations?
const fn is_known_subcmd(subcmd: i32) -> bool {
    matches!(
        subcmd,
        Q_SYNC | Q_QUOTAON | Q_QUOTAOFF | Q_GETFMT
            | Q_GETINFO | Q_SETINFO | Q_GETQUOTA | Q_SETQUOTA,
    )
}

/// Is `qtype` a valid quota type for the commands that require one?
const fn is_valid_qtype(qtype: i32) -> bool {
    qtype >= 0 && qtype < MAXQUOTAS
}

/// Does this subcommand read or write through `addr`?
const fn needs_addr(subcmd: i32) -> bool {
    matches!(
        subcmd,
        Q_QUOTAON | Q_GETFMT | Q_GETINFO | Q_SETINFO | Q_GETQUOTA | Q_SETQUOTA,
    )
}

/// Does this subcommand name a specific filesystem via `special`?
const fn needs_special(subcmd: i32) -> bool {
    // Q_SYNC with NULL special is the well-defined "sync everything"
    // form; every other subcommand operates on a specific filesystem.
    !matches!(subcmd, Q_SYNC)
}

// ---------------------------------------------------------------------------
// quotactl()
// ---------------------------------------------------------------------------

/// Manipulate disk quotas.
///
/// Validates the call and routes to the (currently unimplemented)
/// quota backend.  See the module-level documentation for the full
/// error contract.
///
/// Returns `0` on success or `-1` with `errno` set on failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn quotactl(
    cmd: i32,
    special: *const u8,
    _id: i32,
    addr: *mut u8,
) -> i32 {
    let (subcmd, qtype) = split_cmd(cmd);

    // Unknown subcommand → EINVAL.
    if !is_known_subcmd(subcmd) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Q_SYNC ignores qtype; everything else must use a valid type.
    if subcmd != Q_SYNC && !is_valid_qtype(qtype) {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // EFAULT for NULL addr on commands that read/write the payload.
    if needs_addr(subcmd) && addr.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // EFAULT when a specific filesystem is required but `special` is
    // NULL.  Linux returns EFAULT here because `user_path_at` calls
    // `getname`/`strncpy_from_user` on the NULL pointer and bails out
    // with -EFAULT before any path resolution can produce ENODEV.
    if needs_special(subcmd) && special.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }

    // Q_SYNC with NULL `special` means "sync every quota-enabled
    // filesystem".  We have none, so there's nothing to flush.
    if subcmd == Q_SYNC && special.is_null() {
        return 0;
    }

    // All inputs look sane; we just don't have a backend.
    errno::set_errno(errno::ENOSYS);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn clear_errno() {
        errno::set_errno(0);
    }

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
        assert_eq!(PRJQUOTA, 2);
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
    fn test_dqblk_zeroed() {
        // SAFETY: Dqblk is repr(C) with all-numeric fields, safe to zero.
        let dq: Dqblk = unsafe { core::mem::zeroed() };
        assert_eq!(dq.dqb_bhardlimit, 0);
        assert_eq!(dq.dqb_curspace, 0);
        assert_eq!(dq.dqb_valid, 0);
    }

    #[test]
    fn test_split_cmd() {
        // `qcmd(cmd, type) = (cmd << 8) | (type & 0xff)`; split_cmd reverses.
        let composite = qcmd(Q_GETQUOTA, USRQUOTA);
        let (sc, qt) = split_cmd(composite);
        assert_eq!(sc, Q_GETQUOTA);
        assert_eq!(qt, USRQUOTA);

        let composite = qcmd(Q_SETQUOTA, GRPQUOTA);
        let (sc, qt) = split_cmd(composite);
        assert_eq!(sc, Q_SETQUOTA);
        assert_eq!(qt, GRPQUOTA);

        let composite = qcmd(Q_SYNC, 0);
        let (sc, qt) = split_cmd(composite);
        assert_eq!(sc, Q_SYNC);
        assert_eq!(qt, 0);
    }

    #[test]
    fn test_quotactl_unknown_subcmd_einval() {
        // Bogus high bits → not in the Q_* set.
        clear_errno();
        let ret = quotactl(0x900001, c"/dev/sda1".as_ptr().cast::<u8>(), 0, core::ptr::null_mut());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_quotactl_invalid_qtype_einval() {
        // qtype = 7 is outside USRQUOTA/GRPQUOTA/PRJQUOTA.
        clear_errno();
        let mut dq = Dqblk {
            dqb_bhardlimit: 0, dqb_bsoftlimit: 0, dqb_curspace: 0,
            dqb_ihardlimit: 0, dqb_isoftlimit: 0, dqb_curinodes: 0,
            dqb_btime: 0, dqb_itime: 0, dqb_valid: 0, _pad: 0,
        };
        let ret = quotactl(
            qcmd(Q_GETQUOTA, 7),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_quotactl_getquota_null_addr_efault() {
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_setquota_null_addr_efault() {
        clear_errno();
        let ret = quotactl(
            qcmd(Q_SETQUOTA, GRPQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_getinfo_null_addr_efault() {
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETINFO, USRQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_getfmt_null_addr_efault() {
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETFMT, USRQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_quotaon_null_addr_efault() {
        // Q_QUOTAON reads the path to the quota file from addr.
        clear_errno();
        let ret = quotactl(
            qcmd(Q_QUOTAON, USRQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_quotaoff_null_special_efault() {
        // Phase 119: Q_QUOTAOFF needs a `special` path.  Linux's
        // user_path_at(NULL) → getname → -EFAULT, so we now match it
        // (was ENODEV).
        clear_errno();
        let ret = quotactl(
            qcmd(Q_QUOTAOFF, USRQUOTA),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_getquota_null_special_efault() {
        // Phase 119: NULL `special` is EFAULT (Linux: getname on NULL
        // returns -EFAULT), not ENODEV.
        clear_errno();
        let mut dq = Dqblk {
            dqb_bhardlimit: 0, dqb_bsoftlimit: 0, dqb_curspace: 0,
            dqb_ihardlimit: 0, dqb_isoftlimit: 0, dqb_curinodes: 0,
            dqb_btime: 0, dqb_itime: 0, dqb_valid: 0, _pad: 0,
        };
        let ret = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            core::ptr::null(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_sync_null_special_returns_zero() {
        // "Sync every quota-enabled filesystem" — we have none.
        clear_errno();
        let ret = quotactl(qcmd(Q_SYNC, 0), core::ptr::null(), 0, core::ptr::null_mut());
        assert_eq!(ret, 0);
        // errno untouched on success.
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_quotactl_sync_with_special_enosys() {
        // Q_SYNC with a specific filesystem is valid → ENOSYS (no
        // backend), distinct from EINVAL/EFAULT/ENODEV.
        clear_errno();
        let ret = quotactl(
            qcmd(Q_SYNC, 0),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_quotactl_sync_ignores_qtype() {
        // Q_SYNC accepts any qtype because the type byte is unused.
        clear_errno();
        let ret = quotactl(qcmd(Q_SYNC, 99), core::ptr::null(), 0, core::ptr::null_mut());
        assert_eq!(ret, 0);
    }

    #[test]
    fn test_quotactl_getquota_valid_inputs_enosys() {
        clear_errno();
        let mut dq = Dqblk {
            dqb_bhardlimit: 0, dqb_bsoftlimit: 0, dqb_curspace: 0,
            dqb_ihardlimit: 0, dqb_isoftlimit: 0, dqb_curinodes: 0,
            dqb_btime: 0, dqb_itime: 0, dqb_valid: 0, _pad: 0,
        };
        let ret = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            1000,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_quotactl_setquota_valid_inputs_enosys() {
        clear_errno();
        let mut dq = Dqblk {
            dqb_bhardlimit: 100, dqb_bsoftlimit: 80, dqb_curspace: 0,
            dqb_ihardlimit: 1000, dqb_isoftlimit: 800, dqb_curinodes: 0,
            dqb_btime: 0, dqb_itime: 0, dqb_valid: QIF_LIMITS, _pad: 0,
        };
        let ret = quotactl(
            qcmd(Q_SETQUOTA, GRPQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            42,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_quotactl_prjquota_accepted() {
        // PRJQUOTA (type 2) is valid and should reach ENOSYS, not EINVAL.
        clear_errno();
        let mut dq = Dqblk {
            dqb_bhardlimit: 0, dqb_bsoftlimit: 0, dqb_curspace: 0,
            dqb_ihardlimit: 0, dqb_isoftlimit: 0, dqb_curinodes: 0,
            dqb_btime: 0, dqb_itime: 0, dqb_valid: 0, _pad: 0,
        };
        let ret = quotactl(
            qcmd(Q_GETQUOTA, PRJQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            5,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_quotactl_negative_qtype_einval() {
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETQUOTA, 0xFF), // qtype = 255 (out of range)
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            1usize as *mut u8,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_quotactl_does_not_set_errno_on_success() {
        // Plant a bogus errno and verify Q_SYNC success leaves it alone.
        errno::set_errno(errno::EINVAL);
        let ret = quotactl(qcmd(Q_SYNC, 0), core::ptr::null(), 0, core::ptr::null_mut());
        assert_eq!(ret, 0);
        // errno preserved (POSIX: successful calls don't clear errno).
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_qif_limits_helper() {
        // QIF_LIMITS isn't exported as a const in this module but is in
        // linux_quota.rs; verify the bit pattern matches what we use.
        assert_eq!(QIF_BLIMITS | QIF_ILIMITS, 5);
    }

    // -- Phase 119: NULL `special` returns EFAULT, not ENODEV -------------
    //
    // Linux's quotactl reaches `user_path_at(AT_FDCWD, special, ...)`
    // which calls `getname()` → `strncpy_from_user(NULL, ...)` →
    // `-EFAULT`.  ENODEV is reserved for the case where the path
    // resolves but is not a quota-enabled filesystem (`quotactl_block`
    // returning -ENODEV).  Our stub must signal the NULL-pointer case
    // with EFAULT so callers that branch on EFAULT-vs-ENODEV (e.g.
    // `quotaon(8)`'s "missing argument vs. wrong filesystem"
    // diagnostics) see Linux-equivalent behaviour.

    fn zero_dqblk() -> Dqblk {
        Dqblk {
            dqb_bhardlimit: 0, dqb_bsoftlimit: 0, dqb_curspace: 0,
            dqb_ihardlimit: 0, dqb_isoftlimit: 0, dqb_curinodes: 0,
            dqb_btime: 0, dqb_itime: 0, dqb_valid: 0, _pad: 0,
        }
    }

    #[test]
    fn test_quotactl_phase119_getquota_null_special_efault() {
        let mut dq = zero_dqblk();
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            core::ptr::null(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_setquota_null_special_efault() {
        let mut dq = zero_dqblk();
        clear_errno();
        let ret = quotactl(
            qcmd(Q_SETQUOTA, USRQUOTA),
            core::ptr::null(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_quotaon_null_special_with_null_addr_efault() {
        // Q_QUOTAON needs both `special` and `addr`.  Linux precedence:
        // user_path_at(NULL) runs first → -EFAULT (we currently check
        // `addr` first, but the resulting errno is the same EFAULT, so
        // callers can't distinguish the cause from errno alone).
        clear_errno();
        let ret = quotactl(
            qcmd(Q_QUOTAON, USRQUOTA),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_quotaoff_null_special_efault() {
        clear_errno();
        let ret = quotactl(
            qcmd(Q_QUOTAOFF, USRQUOTA),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_getinfo_null_special_efault() {
        let mut info = [0u8; 32];
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETINFO, USRQUOTA),
            core::ptr::null(),
            0,
            info.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_setinfo_null_special_efault() {
        let mut info = [0u8; 32];
        clear_errno();
        let ret = quotactl(
            qcmd(Q_SETINFO, USRQUOTA),
            core::ptr::null(),
            0,
            info.as_mut_ptr(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_getfmt_null_special_efault() {
        let mut fmt = 0u32;
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETFMT, USRQUOTA),
            core::ptr::null(),
            0,
            (&raw mut fmt).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
    }

    #[test]
    fn test_quotactl_phase119_einval_wins_over_null_special() {
        // Unknown subcmd with NULL special: EINVAL must surface before
        // the special-NULL → EFAULT check (Linux validates the cmd
        // word's subcmd in the prologue, before user_path_at).
        clear_errno();
        let bad_subcmd: i32 = 0x800099; // Not in the known list.
        let ret = quotactl(
            qcmd(bad_subcmd, USRQUOTA),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_quotactl_phase119_qtype_einval_wins_over_null_special() {
        // Bad qtype with NULL special: EINVAL still wins (qtype check
        // runs before special check).
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETQUOTA, 99), // Invalid qtype.
            core::ptr::null(),
            0,
            1usize as *mut u8,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_quotactl_phase119_sync_null_special_still_zero() {
        // Q_SYNC keeps the "sync all" semantics: NULL special is OK
        // (not EFAULT) because Linux's Q_SYNC bypasses user_path_at.
        errno::set_errno(0);
        let ret = quotactl(
            qcmd(Q_SYNC, 0),
            core::ptr::null(),
            0,
            core::ptr::null_mut(),
        );
        assert_eq!(ret, 0);
        assert_eq!(errno::get_errno(), 0);
    }

    #[test]
    fn test_quotactl_phase119_recovery_after_efault() {
        // After a NULL-special EFAULT, the next call with a valid
        // special must still reach the ENOSYS arm — no sticky state.
        let mut dq = zero_dqblk();
        clear_errno();
        let r1 = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            core::ptr::null(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(r1, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);

        clear_errno();
        let r2 = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            c"/dev/sda1".as_ptr().cast::<u8>(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(r2, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_quotactl_phase119_buggy_caller_swapped_special_and_addr() {
        // Buggy code passing `addr` as `special` (and vice versa) for
        // Q_GETQUOTA: special is non-NULL (so passes our check) and
        // addr is non-NULL too — reaches ENOSYS arm.  The point is
        // that EFAULT only fires for NULL, not "wrong-looking but
        // non-NULL" pointers.
        let mut dq = zero_dqblk();
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            (&raw mut dq).cast::<u8>().cast_const(),
            0,
            c"/dev/sda1".as_ptr().cast::<u8>().cast_mut(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_quotactl_phase119_no_side_effect_on_efault_buffer() {
        // EFAULT path with a valid addr buffer: the buffer must be
        // untouched (we bail before any quota backend would write).
        let mut dq = zero_dqblk();
        dq.dqb_bhardlimit = 0xDEAD_BEEF_DEAD_BEEFu64;
        clear_errno();
        let ret = quotactl(
            qcmd(Q_GETQUOTA, USRQUOTA),
            core::ptr::null(),
            0,
            (&raw mut dq).cast::<u8>(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // Buffer untouched.
        assert_eq!(dq.dqb_bhardlimit, 0xDEAD_BEEF_DEAD_BEEFu64);
    }
}

