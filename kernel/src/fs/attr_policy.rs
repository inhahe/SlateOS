//! What the immutable and append-only attributes forbid: Linux's rules, in
//! one place.
//!
//! [`FileAttr::IMMUTABLE`] and [`FileAttr::APPEND_ONLY`] are `chattr +i` and
//! `chattr +a` (`FS_IMMUTABLE_FL`, `FS_APPEND_FL`). Linux spreads what they
//! forbid across its VFS -- `may_delete`, `may_create`, `vfs_link`,
//! `may_open`, `setfl`, `may_setattr`, `vfs_fallocate`, `xattr_permission` --
//! and the filesystems' write paths (`ext4_generic_write_checks`). Here the
//! rules are the functions below, and two layers apply them:
//!
//! - **The VFS**, for everything that changes a name, the metadata or the
//!   allocation, and for an open: on the objects as they stand under the
//!   filesystem's lock, just before it asks the filesystem. So every
//!   filesystem that reports the attributes is held to them -- memfs, ext4
//!   and FAT alike -- and a filesystem added later is too, without knowing
//!   these rules exist.
//! - **The filesystems**, for what changes the contents -- a write, an
//!   append, a truncate, a whole-file replacement -- on the inode the write
//!   already holds. The VFS write path holds no metadata: a check there would
//!   cost a lookup per write, and open a window between the check and the
//!   write in which the attribute could be set.
//!
//! Every refusal is `NotPermitted` (`EPERM`, "Operation not permitted"), as
//! on Linux. Until 2026-10-02 the filesystems answered `PermissionDenied`
//! (`EACCES`, "Permission denied"), which is the answer to a permission bit,
//! and only some operations were refused at all: a rename onto an immutable
//! file replaced it, `chmod` changed one, and on FAT nothing was refused.
//!
//! Root is refused too. The attributes bind everyone; what root alone can do
//! is clear them (`Vfs::set_attributes`).
//!
//! Extended attributes are [`super::xattr_policy`]'s, which applies the same
//! rule: a change to an immutable or append-only file is `EPERM`.

use super::vfs::FileAttr;
use crate::error::{KernelError, KernelResult};

/// Either attribute. Each freezes a name and the metadata; they differ only
/// in what they allow the contents (nothing, or growth at the end).
const FROZEN: u32 = FileAttr::IMMUTABLE.bits() | FileAttr::APPEND_ONLY.bits();

fn immutable(attrs: FileAttr) -> bool {
    attrs.contains(FileAttr::IMMUTABLE)
}

fn append_only(attrs: FileAttr) -> bool {
    attrs.contains(FileAttr::APPEND_ONLY)
}

fn frozen(attrs: FileAttr) -> bool {
    attrs.bits() & FROZEN != 0
}

fn refuse_if(refused: bool) -> KernelResult<()> {
    if refused {
        Err(KernelError::NotPermitted)
    } else {
        Ok(())
    }
}

/// May a name be removed from the directory `dir` when the name is
/// `victim`'s? `unlink`, `rmdir`, the source of a `rename`, and the name a
/// `rename` replaces or exchanges.
///
/// Linux's `may_delete`: not from an immutable or append-only directory, and
/// not the name of an immutable or append-only object.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_delete(dir: FileAttr, victim: FileAttr) -> KernelResult<()> {
    refuse_if(frozen(dir) || frozen(victim))
}

/// May a name be added to the directory `dir`? A new file, directory,
/// symlink or socket, the new name of a `link`, the destination of a
/// `rename` that replaces nothing, and an unnamed file made in it
/// (`O_TMPFILE`).
///
/// Linux's `may_create`, by way of `inode_permission(dir, MAY_WRITE)`: an
/// immutable directory takes no new names. An append-only one does -- it
/// keeps the names it has, which is what makes it a log directory.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_create(dir: FileAttr) -> KernelResult<()> {
    refuse_if(immutable(dir))
}

/// May the object `source` be given another name?
///
/// Linux's `vfs_link`: not an immutable or append-only one.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_link(source: FileAttr) -> KernelResult<()> {
    refuse_if(frozen(source))
}

/// May the mode, owner or group of `target` be changed, or its times be set
/// to given values?
///
/// Linux's `may_setattr` for `ATTR_MODE`, `ATTR_UID`, `ATTR_GID` and
/// `ATTR_TIMES_SET`: not on an immutable or append-only object.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_change_metadata(target: FileAttr) -> KernelResult<()> {
    refuse_if(frozen(target))
}

/// May the times of `target` be set to now -- `utimensat` with no times, or
/// with both `UTIME_NOW`, which is what `touch` asks?
///
/// Linux's `may_setattr` for `ATTR_TOUCH`: not on an immutable object. An
/// append-only one may be touched, since every append moves its times
/// anyway; what it may not have is times set to given values, which could
/// backdate a log ([`may_change_metadata`]).
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_touch(target: FileAttr) -> KernelResult<()> {
    refuse_if(immutable(target))
}

/// May `target` be opened for writing (`writable`), for appending
/// (`append`) and with truncation (`truncate`)?
///
/// Linux's `may_open`: an immutable object is not opened to be written or
/// truncated; an append-only one is opened to be written only for
/// appending, and is never truncated. Refused at the open, as Linux refuses
/// it, rather than at the first write -- `echo x > file` fails at its
/// redirection, and a program checks its `open`.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_open(
    target: FileAttr,
    writable: bool,
    append: bool,
    truncate: bool,
) -> KernelResult<()> {
    if immutable(target) && (writable || truncate) {
        return Err(KernelError::NotPermitted);
    }
    refuse_if(append_only(target) && ((writable && !append) || truncate))
}

/// May an open file of `target` start or stop appending (`fcntl(F_SETFL)`
/// changing `O_APPEND`)?
///
/// Linux's `setfl`: not an append-only one, either way. Stopping would undo
/// [`may_open`]'s rule at the first `fcntl`; starting is refused with it,
/// as Linux refuses any change to the flag on such a file.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_change_appending(target: FileAttr) -> KernelResult<()> {
    refuse_if(append_only(target))
}

/// May space be allocated to `target` (`fallocate`)?
///
/// Linux's `vfs_fallocate`: not to an immutable file. Allocating to an
/// append-only one is allowed -- only the modes that punch a hole or
/// collapse a range would change what is already written, and this kernel
/// offers neither.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_allocate(target: FileAttr) -> KernelResult<()> {
    refuse_if(immutable(target))
}

/// May data be written at `offset` into `target`, a file of `size` bytes?
///
/// Not into an immutable file (ext4's `ext4_generic_write_checks`), and
/// into an append-only one only at its end -- where every write through a
/// descriptor [`may_open`] allowed lands, since such a descriptor appends.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_write_at(target: FileAttr, offset: u64, size: u64) -> KernelResult<()> {
    refuse_if(immutable(target) || (append_only(target) && offset != size))
}

/// May the contents of `target` be replaced or cut -- a truncate, or a write
/// of the whole file?
///
/// Not an immutable file, and not an append-only one: cutting it would
/// rewrite what it has recorded. Linux's `do_truncate` and `do_sys_ftruncate`
/// (`IS_APPEND`), and `may_open` for `O_TRUNC`.
///
/// # Errors
///
/// `NotPermitted`.
pub fn may_rewrite(target: FileAttr) -> KernelResult<()> {
    refuse_if(frozen(target))
}

/// The rules alone, against every combination of the two attributes.
///
/// [`super::vfs::self_test_attr_rules`] is the other half: every VFS
/// operation and every filesystem write path, applying them.
///
/// # Errors
///
/// `InternalError` naming the first rule that answered wrongly.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    let none = FileAttr::NONE;
    let imm = FileAttr::IMMUTABLE;
    let app = FileAttr::APPEND_ONLY;
    let both = imm.union(app);
    // HIDDEN and SYSTEM are attributes too, and must not be mistaken for
    // either of these.
    let other = FileAttr::HIDDEN.union(FileAttr::SYSTEM);
    let no = Err(KernelError::NotPermitted);
    let yes = Ok(());

    let cases: [(&str, KernelResult<()>, KernelResult<()>); 40] = [
        // may_delete: either attribute on either side.
        ("delete: plain", may_delete(none, none), yes),
        ("delete: HIDDEN|SYSTEM", may_delete(other, other), yes),
        ("delete: immutable victim", may_delete(none, imm), no),
        ("delete: append-only victim", may_delete(none, app), no),
        ("delete: from an immutable dir", may_delete(imm, none), no),
        ("delete: from an append-only dir", may_delete(app, none), no),
        ("delete: both", may_delete(both, both), no),
        // may_create: an append-only directory takes new names.
        ("create: plain dir", may_create(none), yes),
        ("create: in an immutable dir", may_create(imm), no),
        ("create: in an append-only dir", may_create(app), yes),
        ("create: in a HIDDEN|SYSTEM dir", may_create(other), yes),
        // may_link
        ("link: plain", may_link(none), yes),
        ("link: immutable", may_link(imm), no),
        ("link: append-only", may_link(app), no),
        // may_change_metadata and may_touch: they differ on append-only.
        ("chmod: plain", may_change_metadata(other), yes),
        ("chmod: immutable", may_change_metadata(imm), no),
        ("chmod: append-only", may_change_metadata(app), no),
        ("touch: plain", may_touch(none), yes),
        ("touch: immutable", may_touch(imm), no),
        ("touch: append-only", may_touch(app), yes),
        // may_open(target, writable, append, truncate)
        (
            "open: immutable, read",
            may_open(imm, false, false, false),
            yes,
        ),
        (
            "open: immutable, write",
            may_open(imm, true, false, false),
            no,
        ),
        (
            "open: immutable, append",
            may_open(imm, true, true, false),
            no,
        ),
        (
            "open: immutable, read+truncate",
            may_open(imm, false, false, true),
            no,
        ),
        (
            "open: append-only, read",
            may_open(app, false, false, false),
            yes,
        ),
        (
            "open: append-only, write",
            may_open(app, true, false, false),
            no,
        ),
        (
            "open: append-only, append",
            may_open(app, true, true, false),
            yes,
        ),
        (
            "open: append-only, append+truncate",
            may_open(app, true, true, true),
            no,
        ),
        (
            "open: plain, write+truncate",
            may_open(other, true, false, true),
            yes,
        ),
        // may_stop_appending, may_allocate
        (
            "F_SETFL O_APPEND: append-only",
            may_change_appending(app),
            no,
        ),
        (
            "F_SETFL O_APPEND: immutable",
            may_change_appending(imm),
            yes,
        ),
        ("fallocate: immutable", may_allocate(imm), no),
        ("fallocate: append-only", may_allocate(app), yes),
        // may_write_at(target, offset, size)
        ("write: plain, mid-file", may_write_at(none, 3, 10), yes),
        (
            "write: immutable, at the end",
            may_write_at(imm, 10, 10),
            no,
        ),
        (
            "write: append-only, at the end",
            may_write_at(app, 10, 10),
            yes,
        ),
        ("write: append-only, mid-file", may_write_at(app, 3, 10), no),
        (
            "write: append-only, past the end",
            may_write_at(app, 11, 10),
            no,
        ),
        // may_rewrite
        ("truncate: append-only", may_rewrite(app), no),
        ("truncate: plain", may_rewrite(other), yes),
    ];
    for (what, got, want) in cases {
        if got != want {
            serial_println!(
                "[attr_policy]   FAIL: {}: got {:?}, want {:?}",
                what,
                got,
                want
            );
            return Err(KernelError::InternalError);
        }
    }
    // The one immutable rewrite not in the table, so the table's count is
    // not the only thing standing between a dropped row and a pass.
    if may_rewrite(imm) != no {
        serial_println!("[attr_policy]   FAIL: truncating an immutable file is allowed");
        return Err(KernelError::InternalError);
    }
    serial_println!(
        "[attr_policy]   41 cases: delete, create, link, chmod, touch, open, F_SETFL, fallocate, write, truncate: OK"
    );
    Ok(())
}
