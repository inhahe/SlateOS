//! `<linux/fanotify.h>` — filesystem-wide notification API.
//!
//! fanotify provides filesystem notification events (similar to inotify
//! but more powerful): it can monitor entire mount points, filter by
//! file type, and support permission events (approve/deny access).
//!
//! # Status
//!
//! `fanotify_init` and `fanotify_mark` now perform full input validation
//! matching Linux's contract — bad flags, bad masks, missing required
//! combinations, and unknown bits all surface clean POSIX errnos
//! (EINVAL/EBADF/EBADFD/EFAULT) instead of "ENOSYS for everything."
//!
//! Once validation passes, `fanotify_init` returns -1 / ENOSYS because
//! the kernel-side filesystem-event hook table doesn't exist yet — and
//! `fanotify_mark` returns -1 / EBADFD because no fanotify ruleset fd
//! can exist with the current kernel. Real callers (auditd, ClamAV
//! on-access scanner, systemd's directory-watch services, AppArmor
//! profile generators, fanotify-rs/inotify-rs crates) detect this exact
//! shape and either fall back to inotify-only mode or disable
//! filesystem watching entirely — same as on a Linux kernel built
//! without `CONFIG_FANOTIFY=y`.

use crate::errno;
use crate::fcntl;

// ---------------------------------------------------------------------------
// fanotify_init() flags (class + additional)
// ---------------------------------------------------------------------------

/// Pre-content class (permission before write).
pub const FAN_CLASS_PRE_CONTENT: u32 = 0x0000_0008;
/// Content class (permission event, blocking).
pub const FAN_CLASS_CONTENT: u32 = 0x0000_0004;
/// Notification class (no permission events).
pub const FAN_CLASS_NOTIF: u32 = 0x0000_0000;

/// Bitmask covering the three class bits (NOTIF/CONTENT/PRE_CONTENT).
///
/// Valid encoded values are `0x0`, `0x4`, and `0x8`; `0xC` is reserved
/// and rejected with EINVAL.
pub const FAN_ALL_CLASS_BITS: u32 = 0x0000_000C;

/// Close-on-exec flag for fanotify fd.
pub const FAN_CLOEXEC: u32 = 0x0000_0001;
/// Non-blocking flag for fanotify fd.
pub const FAN_NONBLOCK: u32 = 0x0000_0002;
/// Unlimited queue.
pub const FAN_UNLIMITED_QUEUE: u32 = 0x0000_0010;
/// Unlimited marks.
pub const FAN_UNLIMITED_MARKS: u32 = 0x0000_0020;
/// Enable fanotify event info (audit).
pub const FAN_ENABLE_AUDIT: u32 = 0x0000_0040;
/// Report a pidfd in place of event->pid (Linux 5.15+).
///
/// Cannot be combined with `FAN_REPORT_TID`: a pidfd of a thread leader
/// is meaningful in the receiving process, but a per-thread id (TID) is
/// process-local and can't be wrapped in a pidfd, so Linux rejects the
/// combination with `EINVAL`.  See Linux commit `a8b13aa20af8`.
pub const FAN_REPORT_PIDFD: u32 = 0x0000_0080;
/// Report the thread id rather than the process id in `event->pid`
/// (Linux 4.20+).
pub const FAN_REPORT_TID: u32 = 0x0000_0100;
/// Report FID instead of fd.
pub const FAN_REPORT_FID: u32 = 0x0000_0200;
/// Report directory FID.
pub const FAN_REPORT_DIR_FID: u32 = 0x0000_0400;
/// Report event name.
pub const FAN_REPORT_NAME: u32 = 0x0000_0800;
/// Report target FID.
pub const FAN_REPORT_TARGET_FID: u32 = 0x0000_1000;
/// Convenience: DIR_FID + NAME.
pub const FAN_REPORT_DFID_NAME: u32 = FAN_REPORT_DIR_FID | FAN_REPORT_NAME;
/// Convenience: DIR_FID + NAME + FID + TARGET_FID.
pub const FAN_REPORT_DFID_NAME_TARGET: u32 =
    FAN_REPORT_DFID_NAME | FAN_REPORT_FID | FAN_REPORT_TARGET_FID;

/// OR of every flag bit `fanotify_init` accepts (excluding class bits).
const FAN_INIT_VALID_FLAGS: u32 = FAN_CLOEXEC
    | FAN_NONBLOCK
    | FAN_UNLIMITED_QUEUE
    | FAN_UNLIMITED_MARKS
    | FAN_ENABLE_AUDIT
    | FAN_REPORT_PIDFD
    | FAN_REPORT_TID
    | FAN_REPORT_FID
    | FAN_REPORT_DIR_FID
    | FAN_REPORT_NAME
    | FAN_REPORT_TARGET_FID;

// ---------------------------------------------------------------------------
// fanotify event mask bits
// ---------------------------------------------------------------------------

/// File was accessed.
pub const FAN_ACCESS: u64 = 0x0000_0001;
/// File was modified.
pub const FAN_MODIFY: u64 = 0x0000_0002;
/// Metadata changed.
pub const FAN_ATTRIB: u64 = 0x0000_0004;
/// Writable file was closed.
pub const FAN_CLOSE_WRITE: u64 = 0x0000_0008;
/// Non-writable file was closed.
pub const FAN_CLOSE_NOWRITE: u64 = 0x0000_0010;
/// File was opened.
pub const FAN_OPEN: u64 = 0x0000_0020;
/// File was moved from this directory.
pub const FAN_MOVED_FROM: u64 = 0x0000_0040;
/// File was moved to this directory.
pub const FAN_MOVED_TO: u64 = 0x0000_0080;
/// Subfile was created.
pub const FAN_CREATE: u64 = 0x0000_0100;
/// Subfile was deleted.
pub const FAN_DELETE: u64 = 0x0000_0200;
/// Self was deleted.
pub const FAN_DELETE_SELF: u64 = 0x0000_0400;
/// Self was moved.
pub const FAN_MOVE_SELF: u64 = 0x0000_0800;
/// File was opened for exec.
pub const FAN_OPEN_EXEC: u64 = 0x0000_1000;

/// Convenience: close = CLOSE_WRITE | CLOSE_NOWRITE.
pub const FAN_CLOSE: u64 = FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE;
/// Convenience: move = MOVED_FROM | MOVED_TO.
pub const FAN_MOVE: u64 = FAN_MOVED_FROM | FAN_MOVED_TO;

// Permission events
/// Permission: file opened.
pub const FAN_OPEN_PERM: u64 = 0x0001_0000;
/// Permission: file accessed.
pub const FAN_ACCESS_PERM: u64 = 0x0002_0000;
/// Permission: file opened for exec.
pub const FAN_OPEN_EXEC_PERM: u64 = 0x0004_0000;

/// Overflow: event queue overflowed (report-only — never valid in a mark mask).
pub const FAN_Q_OVERFLOW: u64 = 0x0000_4000;
/// Event on a child.
pub const FAN_ONDIR: u64 = 0x4000_0000;
/// Event occurred against dir.
pub const FAN_EVENT_ON_CHILD: u64 = 0x0800_0000;

/// OR of every event mask bit valid as a target in `fanotify_mark`
/// (i.e. excluding the report-only `FAN_Q_OVERFLOW`).
const FAN_MARK_VALID_MASK: u64 = FAN_ACCESS
    | FAN_MODIFY
    | FAN_ATTRIB
    | FAN_CLOSE_WRITE
    | FAN_CLOSE_NOWRITE
    | FAN_OPEN
    | FAN_MOVED_FROM
    | FAN_MOVED_TO
    | FAN_CREATE
    | FAN_DELETE
    | FAN_DELETE_SELF
    | FAN_MOVE_SELF
    | FAN_OPEN_EXEC
    | FAN_OPEN_PERM
    | FAN_ACCESS_PERM
    | FAN_OPEN_EXEC_PERM
    | FAN_ONDIR
    | FAN_EVENT_ON_CHILD;

/// Subset of `FAN_MARK_VALID_MASK` that represents permission events
/// (would require a `FAN_CLASS_CONTENT` or `FAN_CLASS_PRE_CONTENT`
/// listener to be meaningful — we don't track per-fd class because no
/// real fanotify fds exist yet, so this is currently only used by tests
/// to assert the constant is consistent with `FAN_MARK_VALID_MASK`).
#[cfg(test)]
const FAN_PERMISSION_EVENTS: u64 =
    FAN_OPEN_PERM | FAN_ACCESS_PERM | FAN_OPEN_EXEC_PERM;

// ---------------------------------------------------------------------------
// fanotify_mark() flags
// ---------------------------------------------------------------------------

/// Add to mark mask.
pub const FAN_MARK_ADD: u32 = 0x0000_0001;
/// Remove from mark mask.
pub const FAN_MARK_REMOVE: u32 = 0x0000_0002;
/// Don't follow symlinks.
pub const FAN_MARK_DONT_FOLLOW: u32 = 0x0000_0004;
/// Only create on directories.
pub const FAN_MARK_ONLYDIR: u32 = 0x0000_0008;
/// Mark the inode (default — value is 0, so it's just "no target bit set").
pub const FAN_MARK_INODE: u32 = 0x0000_0000;
/// Mark the mount.
pub const FAN_MARK_MOUNT: u32 = 0x0000_0010;
/// Mark the filesystem.
pub const FAN_MARK_FILESYSTEM: u32 = 0x0000_0100;
/// Remove all marks.
pub const FAN_MARK_FLUSH: u32 = 0x0000_0080;
/// Ignore mask (events to ignore).
pub const FAN_MARK_IGNORED_MASK: u32 = 0x0000_0020;
/// Survive modify events.
pub const FAN_MARK_IGNORED_SURV_MODIFY: u32 = 0x0000_0040;
/// Mark may be evicted under memory pressure.
pub const FAN_MARK_EVICTABLE: u32 = 0x0000_0200;

/// Operation bits — exactly one must be set in `flags`.
const FAN_MARK_OP_BITS: u32 = FAN_MARK_ADD | FAN_MARK_REMOVE | FAN_MARK_FLUSH;
/// Target bits — at most one may be set (default `INODE` = 0).
const FAN_MARK_TARGET_BITS: u32 = FAN_MARK_MOUNT | FAN_MARK_FILESYSTEM;
/// OR of every flag bit `fanotify_mark` accepts.
const FAN_MARK_VALID_FLAGS: u32 = FAN_MARK_OP_BITS
    | FAN_MARK_TARGET_BITS
    | FAN_MARK_DONT_FOLLOW
    | FAN_MARK_ONLYDIR
    | FAN_MARK_IGNORED_MASK
    | FAN_MARK_IGNORED_SURV_MODIFY
    | FAN_MARK_EVICTABLE;

// ---------------------------------------------------------------------------
// fanotify response (for permission events)
// ---------------------------------------------------------------------------

/// Allow the file operation.
pub const FAN_ALLOW: u32 = 0x01;
/// Deny the file operation.
pub const FAN_DENY: u32 = 0x02;
/// Audit the access decision.
pub const FAN_AUDIT: u32 = 0x10;

// ---------------------------------------------------------------------------
// fanotify event metadata
// ---------------------------------------------------------------------------

/// Fanotify event metadata (24 bytes on 64-bit).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct FanotifyEventMetadata {
    /// Total event length.
    pub event_len: u32,
    /// Metadata version.
    pub vers: u8,
    /// Reserved.
    pub reserved: u8,
    /// Metadata length.
    pub metadata_len: u16,
    /// Event mask.
    pub mask: u64,
    /// File descriptor.
    pub fd: i32,
    /// Process ID.
    pub pid: i32,
}

/// Current metadata version.
pub const FANOTIFY_METADATA_VERSION: u8 = 3;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// `event_f_flags` for `fanotify_init` must be a valid `open(2)` flags
/// word: low two bits in `{O_RDONLY, O_WRONLY, O_RDWR}` plus an
/// optional bag of well-known supplementary open flags. Matches
/// Linux's validation in `fs/notify/fanotify/fanotify_user.c`.
fn validate_event_f_flags(event_f_flags: u32) -> Result<(), i32> {
    let access_mode = (event_f_flags as i32) & 0o3;
    if access_mode != fcntl::O_RDONLY
        && access_mode != fcntl::O_WRONLY
        && access_mode != fcntl::O_RDWR
    {
        return Err(errno::EINVAL);
    }

    // Mask covering everything we recognize. `0o3` is the access-mode
    // pair already vetted above. `O_LARGEFILE` (0o100_000) doesn't have
    // a libc-style constant in our fcntl module because we're 64-bit-only,
    // but Linux's fanotify_init still accepts the bit pattern so we
    // hard-code it here.
    const O_LARGEFILE_BIT: u32 = 0o100_000;
    let allowed: u32 = 0o3
        | O_LARGEFILE_BIT
        | (fcntl::O_CLOEXEC as u32)
        | (fcntl::O_APPEND as u32)
        | (fcntl::O_NONBLOCK as u32)
        | (fcntl::O_SYNC as u32)
        | (fcntl::O_DSYNC as u32)
        | (fcntl::O_NOATIME as u32);
    if (event_f_flags & !allowed) != 0 {
        return Err(errno::EINVAL);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize a fanotify group.
///
/// Validates `flags` and `event_f_flags` against Linux's
/// `fanotify_init(2)` contract. Returns `-1` with errno set to:
///
/// * `EINVAL` — invalid class encoding (`0xC`), unknown flag bits,
///   `FAN_REPORT_NAME` without `FAN_REPORT_DIR_FID`,
///   `FAN_REPORT_TARGET_FID` without the full FID+DIR_FID+NAME triple,
///   `FAN_REPORT_PIDFD` together with `FAN_REPORT_TID` (the two are
///   mutually exclusive — per-thread ids can't be wrapped in a pidfd),
///   or any unsupported `event_f_flags` bit.
/// * `ENOSYS` — all inputs valid, but the kernel can't yet allocate a
///   fanotify group fd (no on-disk inode-watch infrastructure exists).
///
/// Matches the externally-observable behavior of a Linux kernel built
/// without `CONFIG_FANOTIFY=y`: real callers detect ENOSYS and fall
/// back to inotify-only paths.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fanotify_init(flags: u32, event_f_flags: u32) -> i32 {
    // Class bits must encode exactly one of NOTIF/CONTENT/PRE_CONTENT.
    // Linux rejects 0xC (both CONTENT and PRE_CONTENT set) with EINVAL.
    let class = flags & FAN_ALL_CLASS_BITS;
    if class != FAN_CLASS_NOTIF
        && class != FAN_CLASS_CONTENT
        && class != FAN_CLASS_PRE_CONTENT
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Remaining flag bits must all be ones we recognize.
    let non_class = flags & !FAN_ALL_CLASS_BITS;
    if (non_class & !FAN_INIT_VALID_FLAGS) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // FAN_REPORT_NAME requires FAN_REPORT_DIR_FID (no name without a
    // directory FID to anchor it). Linux: EINVAL.
    if (flags & FAN_REPORT_NAME) != 0 && (flags & FAN_REPORT_DIR_FID) == 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // FAN_REPORT_TARGET_FID requires FAN_REPORT_FID, FAN_REPORT_DIR_FID,
    // and FAN_REPORT_NAME (Linux 5.17+ contract for rename source/target
    // tracking). EINVAL otherwise.
    if (flags & FAN_REPORT_TARGET_FID) != 0
        && (flags & (FAN_REPORT_FID | FAN_REPORT_DIR_FID | FAN_REPORT_NAME))
            != (FAN_REPORT_FID | FAN_REPORT_DIR_FID | FAN_REPORT_NAME)
    {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Phase 141: FAN_REPORT_PIDFD and FAN_REPORT_TID are mutually
    // exclusive.  A pidfd targets a process-leader; a TID identifies a
    // single thread, so wrapping a TID in a pidfd is meaningless.
    // Linux's `do_fanotify_init` rejects the combination with EINVAL
    // (kernel commit a8b13aa20af8, 5.15+).
    if (flags & FAN_REPORT_PIDFD) != 0 && (flags & FAN_REPORT_TID) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // event_f_flags shape: must be a valid open(2) flag word.
    if let Err(e) = validate_event_f_flags(event_f_flags) {
        errno::set_errno(e);
        return -1;
    }

    // Privilege check would normally require CAP_SYS_ADMIN here; we
    // have no security model yet, so anyone passes (matches every
    // other syscall in this layer).
    //
    // All inputs valid — but we have no kernel-side fanotify group
    // table to allocate from. Real callers treat this exactly like a
    // kernel built without CONFIG_FANOTIFY.
    errno::set_errno(errno::ENOSYS);
    -1
}

/// Add, remove, or flush a mark on a fanotify group.
///
/// Validates every flag, mask, and target combination per the
/// `fanotify_mark(2)` contract. Returns `-1` with errno set to:
///
/// * `EINVAL` — `flags` lacks exactly one of `ADD|REMOVE|FLUSH`, both
///   `MOUNT` and `FILESYSTEM` target bits set, unknown flag bits,
///   unknown mask bits, `ADD|REMOVE` with empty mask, `FLUSH` with
///   non-empty mask.
/// * `EBADF` — `fanotify_fd < 0`.
/// * `EFAULT` — `pathname` is non-NULL but the validation infrastructure
///   can't read it (currently never reached — pathname is treated as
///   opaque by validation).
/// * `EBADFD` — every preceding check passes but the kernel doesn't
///   recognize `fanotify_fd` as a fanotify group fd (there are no such
///   fds in our current kernel).
///
/// `dirfd`-relative path resolution is deferred until a real fanotify
/// group fd can exist; for now the function reaches EBADFD before any
/// path lookup happens, which matches the behavior real callers expect
/// from a `CONFIG_FANOTIFY`-disabled kernel.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn fanotify_mark(
    fanotify_fd: i32,
    flags: u32,
    mask: u64,
    _dirfd: i32,
    _pathname: *const u8,
) -> i32 {
    // Exactly one of ADD/REMOVE/FLUSH must be set. Linux explicitly
    // checks `hweight32(flags & FAN_MARK_OP_BITS) == 1`.
    let op_bits = flags & FAN_MARK_OP_BITS;
    if op_bits.count_ones() != 1 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Target may specify at most one of MOUNT or FILESYSTEM (default
    // is the inode itself — value 0).
    let target_bits = flags & FAN_MARK_TARGET_BITS;
    if target_bits.count_ones() > 1 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // Any flag bit we don't recognize → EINVAL.
    if (flags & !FAN_MARK_VALID_FLAGS) != 0 {
        errno::set_errno(errno::EINVAL);
        return -1;
    }

    // FLUSH ignores the mask in Linux but explicitly requires it to be
    // zero to avoid silently dropping bits a caller intended to apply.
    // ADD/REMOVE with an empty mask is meaningless (matches Linux).
    if (flags & FAN_MARK_FLUSH) != 0 {
        if mask != 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    } else {
        if mask == 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
        if (mask & !FAN_MARK_VALID_MASK) != 0 {
            errno::set_errno(errno::EINVAL);
            return -1;
        }
    }

    // Now look at the fd. Anything obviously bad → EBADF; anything
    // syntactically OK but unknown to the kernel → EBADFD (the
    // standard "fd is open but isn't the right kind of object"
    // signal, distinguishable from EBADF by callers that maintain a
    // fanotify_fd cache invariant).
    if fanotify_fd < 0 {
        errno::set_errno(errno::EBADF);
        return -1;
    }

    errno::set_errno(errno::EBADFD);
    -1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------
    // Constant invariants
    // -----------------------------------------------------------------

    #[test]
    fn test_event_metadata_size() {
        assert_eq!(core::mem::size_of::<FanotifyEventMetadata>(), 24);
    }

    #[test]
    fn test_class_flags() {
        assert_eq!(FAN_CLASS_NOTIF, 0);
        assert_ne!(FAN_CLASS_CONTENT, FAN_CLASS_PRE_CONTENT);
        assert_ne!(FAN_CLASS_CONTENT, FAN_CLASS_NOTIF);
        assert_eq!(FAN_ALL_CLASS_BITS, 0x0C);
    }

    #[test]
    fn test_event_masks_distinct() {
        let masks = [
            FAN_ACCESS, FAN_MODIFY, FAN_ATTRIB,
            FAN_CLOSE_WRITE, FAN_CLOSE_NOWRITE, FAN_OPEN,
            FAN_MOVED_FROM, FAN_MOVED_TO, FAN_CREATE,
            FAN_DELETE, FAN_DELETE_SELF, FAN_MOVE_SELF,
            FAN_OPEN_EXEC, FAN_OPEN_PERM, FAN_ACCESS_PERM,
            FAN_OPEN_EXEC_PERM,
        ];
        for i in 0..masks.len() {
            for j in (i + 1)..masks.len() {
                assert_ne!(masks[i], masks[j]);
            }
        }
    }

    #[test]
    fn test_convenience_masks() {
        assert_eq!(FAN_CLOSE, FAN_CLOSE_WRITE | FAN_CLOSE_NOWRITE);
        assert_eq!(FAN_MOVE, FAN_MOVED_FROM | FAN_MOVED_TO);
        assert_eq!(FAN_REPORT_DFID_NAME, FAN_REPORT_DIR_FID | FAN_REPORT_NAME);
        assert_eq!(
            FAN_REPORT_DFID_NAME_TARGET,
            FAN_REPORT_DIR_FID | FAN_REPORT_NAME | FAN_REPORT_FID | FAN_REPORT_TARGET_FID,
        );
    }

    #[test]
    fn test_mark_flags_distinct() {
        let flags = [
            FAN_MARK_ADD, FAN_MARK_REMOVE, FAN_MARK_DONT_FOLLOW,
            FAN_MARK_ONLYDIR, FAN_MARK_MOUNT, FAN_MARK_FILESYSTEM,
            FAN_MARK_FLUSH, FAN_MARK_IGNORED_MASK,
            FAN_MARK_IGNORED_SURV_MODIFY, FAN_MARK_EVICTABLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_response_values() {
        assert_eq!(FAN_ALLOW, 0x01);
        assert_eq!(FAN_DENY, 0x02);
        assert_eq!(FAN_AUDIT, 0x10);
    }

    #[test]
    fn test_permission_events_are_subset_of_mark_mask() {
        assert_eq!(FAN_PERMISSION_EVENTS & FAN_MARK_VALID_MASK, FAN_PERMISSION_EVENTS);
    }

    #[test]
    fn test_q_overflow_not_in_mark_mask() {
        // FAN_Q_OVERFLOW is report-only — must never appear in a mark mask.
        assert_eq!(FAN_Q_OVERFLOW & FAN_MARK_VALID_MASK, 0);
    }

    // -----------------------------------------------------------------
    // fanotify_init: class validation
    // -----------------------------------------------------------------

    #[test]
    fn test_init_class_notif_with_valid_event_f_flags_reaches_enosys() {
        errno::set_errno(0);
        let ret = fanotify_init(FAN_CLASS_NOTIF | FAN_CLOEXEC, fcntl::O_RDONLY as u32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_class_content_reaches_enosys() {
        errno::set_errno(0);
        let ret = fanotify_init(FAN_CLASS_CONTENT, fcntl::O_RDONLY as u32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_class_pre_content_reaches_enosys() {
        errno::set_errno(0);
        let ret = fanotify_init(FAN_CLASS_PRE_CONTENT, fcntl::O_RDONLY as u32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_invalid_class_0xc_einval() {
        errno::set_errno(0);
        // Both CONTENT and PRE_CONTENT set — reserved combination.
        let ret = fanotify_init(FAN_CLASS_CONTENT | FAN_CLASS_PRE_CONTENT, fcntl::O_RDONLY as u32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // fanotify_init: flag bits
    // -----------------------------------------------------------------

    #[test]
    fn test_init_unknown_flag_bit_einval() {
        errno::set_errno(0);
        // 0x8000_0000 is unused — must surface EINVAL.
        let ret = fanotify_init(FAN_CLASS_NOTIF | 0x8000_0000, fcntl::O_RDONLY as u32);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_init_report_name_without_dir_fid_einval() {
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_FID | FAN_REPORT_NAME,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_init_report_name_with_dir_fid_ok() {
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_DIR_FID | FAN_REPORT_NAME,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_report_target_fid_requires_full_triple() {
        // Missing FAN_REPORT_NAME — EINVAL.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_FID | FAN_REPORT_DIR_FID | FAN_REPORT_TARGET_FID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // Missing FAN_REPORT_FID — EINVAL.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_DIR_FID | FAN_REPORT_NAME | FAN_REPORT_TARGET_FID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        // All three present — reaches ENOSYS.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_DFID_NAME_TARGET,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // fanotify_init: event_f_flags
    // -----------------------------------------------------------------

    #[test]
    fn test_init_bad_access_mode_einval() {
        // Access mode 3 is reserved.
        errno::set_errno(0);
        let ret = fanotify_init(FAN_CLASS_NOTIF, 0o3);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_init_unknown_event_f_flag_einval() {
        // 0x4000_0000 is an unused open-flag bit.
        errno::set_errno(0);
        let ret = fanotify_init(FAN_CLASS_NOTIF, fcntl::O_RDONLY as u32 | 0x4000_0000);
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_init_o_rdwr_o_cloexec_o_nonblock_ok() {
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF,
            (fcntl::O_RDWR | fcntl::O_CLOEXEC | fcntl::O_NONBLOCK) as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_init_o_largefile_o_noatime_o_sync_ok() {
        // These flags don't have const Rust analogues in fcntl in some
        // libcs, but Linux fanotify_init accepts them. We accept the
        // bit pattern via the validate_event_f_flags allowlist.
        errno::set_errno(0);
        let large_file_bit: u32 = 0o100_000;
        let ret = fanotify_init(
            FAN_CLASS_NOTIF,
            fcntl::O_RDONLY as u32 | large_file_bit | (fcntl::O_NOATIME as u32),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // Phase 141 — FAN_REPORT_PIDFD / FAN_REPORT_TID mutual exclusion
    //
    // Linux 5.15 added FAN_REPORT_PIDFD; Linux 4.20 added
    // FAN_REPORT_TID.  do_fanotify_init returns EINVAL when both are
    // requested because a pidfd refers to a process leader and a TID
    // identifies a single thread; the two cannot coexist on the same
    // event header.  This block pins our errno priority to Linux.
    // -----------------------------------------------------------------

    #[test]
    fn test_phase141_pidfd_tid_constants_distinct() {
        // Sanity check: the bit positions must match Linux's UAPI and
        // not collide with any other init-time flag.
        assert_eq!(FAN_REPORT_PIDFD, 0x0000_0080);
        assert_eq!(FAN_REPORT_TID, 0x0000_0100);
        assert_ne!(FAN_REPORT_PIDFD, FAN_REPORT_TID);
        // They must not overlap any pre-existing FAN_REPORT_* bit.
        let others = FAN_REPORT_FID
            | FAN_REPORT_DIR_FID
            | FAN_REPORT_NAME
            | FAN_REPORT_TARGET_FID;
        assert_eq!(FAN_REPORT_PIDFD & others, 0);
        assert_eq!(FAN_REPORT_TID & others, 0);
    }

    #[test]
    fn test_phase141_pidfd_and_tid_distinct_from_class_and_aux_flags() {
        // PIDFD / TID must not collide with the class word or with
        // CLOEXEC/NONBLOCK/etc.; otherwise a buggy caller flipping
        // FAN_CLOEXEC could accidentally trip the new mutex.
        let aux = FAN_CLOEXEC
            | FAN_NONBLOCK
            | FAN_UNLIMITED_QUEUE
            | FAN_UNLIMITED_MARKS
            | FAN_ENABLE_AUDIT;
        assert_eq!(FAN_REPORT_PIDFD & FAN_ALL_CLASS_BITS, 0);
        assert_eq!(FAN_REPORT_TID & FAN_ALL_CLASS_BITS, 0);
        assert_eq!(FAN_REPORT_PIDFD & aux, 0);
        assert_eq!(FAN_REPORT_TID & aux, 0);
    }

    #[test]
    fn test_phase141_pidfd_alone_reaches_enosys() {
        // PIDFD on its own is a valid request; no mutex trips.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_tid_alone_reaches_enosys() {
        // TID on its own is a valid request; no mutex trips.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_pidfd_and_tid_einval() {
        // The core regression: both bits together must be rejected.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase141_pidfd_with_cloexec_ok() {
        // Common real-world pattern: caller wants pidfd reports and an
        // O_CLOEXEC fanotify fd.  Must reach ENOSYS, not EINVAL.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD | FAN_CLOEXEC,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_pidfd_with_fid_combo_ok() {
        // PIDFD + FID + DIR_FID + NAME is a documented combination
        // (filesystem watcher tracking the originating pidfd as well as
        // the rename source FID); must reach ENOSYS.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF
                | FAN_REPORT_PIDFD
                | FAN_REPORT_FID
                | FAN_REPORT_DIR_FID
                | FAN_REPORT_NAME,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_tid_with_cloexec_nonblock_ok() {
        // TID + CLOEXEC + NONBLOCK — common open(2)-style trio.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_TID | FAN_CLOEXEC | FAN_NONBLOCK,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_class_check_beats_pidfd_tid_mutex() {
        // Ordering matrix: invalid class (0xC) AND both PIDFD+TID set.
        // The class check runs first, so EINVAL is observed — but it
        // would have been EINVAL either way, so we only verify that
        // the call still fails with EINVAL and -1.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_CONTENT
                | FAN_CLASS_PRE_CONTENT
                | FAN_REPORT_PIDFD
                | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase141_unknown_bit_check_beats_pidfd_tid_mutex() {
        // Ordering matrix: unknown flag bit (0x8000_0000) AND both
        // PIDFD+TID.  The unknown-bit check runs before the mutex —
        // observable errno is EINVAL either way; this test pins the
        // outcome shape rather than the internal source.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | 0x8000_0000 | FAN_REPORT_PIDFD | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase141_pidfd_tid_mutex_beats_event_f_flags_check() {
        // Ordering matrix: mutex runs BEFORE event_f_flags validation,
        // so even a bogus access mode (0o3) cannot mask the mutex.
        // Both produce EINVAL; this test pins the negative behavior
        // shape and ensures we do not return EFAULT/ENOSYS by mistake.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD | FAN_REPORT_TID,
            0o3,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase141_target_fid_check_beats_pidfd_tid_mutex() {
        // FAN_REPORT_TARGET_FID without the full triple AND both
        // PIDFD+TID — TARGET_FID check runs first, both produce
        // EINVAL.  Confirms the function still rejects malformed
        // multi-bug inputs cleanly.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF
                | FAN_REPORT_TARGET_FID
                | FAN_REPORT_FID
                | FAN_REPORT_PIDFD
                | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_phase141_buggy_caller_recovers_after_dropping_tid() {
        // Workflow regression: caller hits EINVAL with PIDFD+TID, then
        // drops TID, then succeeds (ENOSYS).  This is the recovery
        // path for libcs that probe-then-fall-back.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_buggy_caller_recovers_after_dropping_pidfd() {
        // Symmetric recovery: drop PIDFD instead, keep TID.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);

        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    #[test]
    fn test_phase141_no_side_effect_loop() {
        // Hammer the mutex-trip path 32 times; errno must always be
        // EINVAL afterwards, return value always -1.  No global state
        // should leak between calls (fanotify_init is supposed to be
        // pure at this layer until a real group fd table exists).
        for _ in 0..32 {
            errno::set_errno(0);
            let ret = fanotify_init(
                FAN_CLASS_NOTIF | FAN_REPORT_PIDFD | FAN_REPORT_TID,
                fcntl::O_RDONLY as u32,
            );
            assert_eq!(ret, -1);
            assert_eq!(errno::get_errno(), errno::EINVAL);
        }
    }

    #[test]
    fn test_phase141_init_valid_flags_admits_pidfd_and_tid() {
        // Regression: before Phase 141, a caller passing
        // FAN_REPORT_PIDFD alone would hit the "unknown flag bit"
        // EINVAL — observable divergence from Linux, which would
        // return ENOSYS (no kernel support) or 0 (group fd created).
        // Both bits must now be valid alone.
        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_PIDFD,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);

        errno::set_errno(0);
        let ret = fanotify_init(
            FAN_CLASS_NOTIF | FAN_REPORT_TID,
            fcntl::O_RDONLY as u32,
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
    }

    // -----------------------------------------------------------------
    // fanotify_mark: op-bit validation
    // -----------------------------------------------------------------

    #[test]
    fn test_mark_no_op_bit_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(0, 0, FAN_OPEN, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_add_and_remove_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD | FAN_MARK_REMOVE,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_add_and_flush_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD | FAN_MARK_FLUSH,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_remove_and_flush_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_REMOVE | FAN_MARK_FLUSH,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // fanotify_mark: target-bit validation
    // -----------------------------------------------------------------

    #[test]
    fn test_mark_mount_and_filesystem_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD | FAN_MARK_MOUNT | FAN_MARK_FILESYSTEM,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_mount_alone_ok_reaches_ebadfd() {
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD | FAN_MARK_MOUNT,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    #[test]
    fn test_mark_filesystem_alone_ok_reaches_ebadfd() {
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD | FAN_MARK_FILESYSTEM,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    // -----------------------------------------------------------------
    // fanotify_mark: unknown flag bits and mask bits
    // -----------------------------------------------------------------

    #[test]
    fn test_mark_unknown_flag_bit_einval() {
        errno::set_errno(0);
        // 0x8000_0000 is unused in the fanotify_mark flag space.
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD | 0x8000_0000,
            FAN_OPEN,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_unknown_mask_bit_einval() {
        errno::set_errno(0);
        // 0x0000_8000 is the FAN_Q_OVERFLOW bit (report-only) — not
        // allowed as a mark target.
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD,
            FAN_OPEN | FAN_Q_OVERFLOW,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_q_overflow_alone_einval() {
        // FAN_Q_OVERFLOW alone is also rejected — and "mask must be
        // non-zero" is satisfied since the bit is set, so the only
        // way this fails is the valid-mask check.
        errno::set_errno(0);
        let ret = fanotify_mark(
            0,
            FAN_MARK_ADD,
            FAN_Q_OVERFLOW,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    // -----------------------------------------------------------------
    // fanotify_mark: ADD/REMOVE vs FLUSH mask requirements
    // -----------------------------------------------------------------

    #[test]
    fn test_mark_add_empty_mask_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(0, FAN_MARK_ADD, 0, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_remove_empty_mask_einval() {
        errno::set_errno(0);
        let ret = fanotify_mark(0, FAN_MARK_REMOVE, 0, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_flush_with_mask_einval() {
        errno::set_errno(0);
        // Linux is lenient (silently ignores mask), but we explicitly
        // require it to be zero so callers can't quietly lose bits.
        let ret = fanotify_mark(0, FAN_MARK_FLUSH, FAN_OPEN, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
    }

    #[test]
    fn test_mark_flush_empty_mask_ok_reaches_ebadfd() {
        errno::set_errno(0);
        let ret = fanotify_mark(0, FAN_MARK_FLUSH, 0, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    // -----------------------------------------------------------------
    // fanotify_mark: fd validation
    // -----------------------------------------------------------------

    #[test]
    fn test_mark_negative_fd_ebadf() {
        errno::set_errno(0);
        let ret = fanotify_mark(-1, FAN_MARK_ADD, FAN_OPEN, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }

    #[test]
    fn test_mark_zero_fd_reaches_ebadfd() {
        // fd=0 is syntactically valid but isn't a fanotify fd.
        errno::set_errno(0);
        let ret = fanotify_mark(0, FAN_MARK_ADD, FAN_OPEN, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    #[test]
    fn test_mark_positive_fd_reaches_ebadfd() {
        errno::set_errno(0);
        let ret = fanotify_mark(42, FAN_MARK_ADD, FAN_OPEN, crate::file::AT_FDCWD, core::ptr::null());
        assert_eq!(ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADFD);
    }

    // -----------------------------------------------------------------
    // Workflow: typical fanotify probe-and-fall-back
    // -----------------------------------------------------------------

    #[test]
    fn test_typical_init_then_mark_workflow_fails_cleanly() {
        // Realistic flow that every fanotify-aware program runs:
        //   1. fanotify_init() with the flags it wants
        //   2. If it returns -1 with ENOSYS, fall back to inotify-only.
        //
        // We verify the ENOSYS shape (not the bare -1), so callers can
        // distinguish "kernel doesn't support fanotify" from "kernel
        // does but our request was malformed."
        errno::set_errno(0);
        let fd = fanotify_init(
            FAN_CLASS_CONTENT | FAN_CLOEXEC | FAN_NONBLOCK,
            (fcntl::O_RDONLY | fcntl::O_CLOEXEC) as u32,
        );
        assert_eq!(fd, -1);
        assert_eq!(errno::get_errno(), errno::ENOSYS);
        // Caller falls back to inotify — no further fanotify_mark calls
        // happen. But if it did hand a stale fd back in:
        errno::set_errno(0);
        let mark_ret = fanotify_mark(
            -1,
            FAN_MARK_ADD | FAN_MARK_MOUNT,
            FAN_OPEN | FAN_CLOSE,
            crate::file::AT_FDCWD,
            core::ptr::null(),
        );
        assert_eq!(mark_ret, -1);
        assert_eq!(errno::get_errno(), errno::EBADF);
    }
}
