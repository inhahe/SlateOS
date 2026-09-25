//! Per-file state kept outside the filesystem, and the lifecycle events that
//! keep it attached to the right file.
//!
//! Four kernel tables hold state about files in memory rather than in the
//! filesystem that holds the files: ACLs ([`super::acl`]), the `chattr`-style
//! flags ([`super::immutable`]), seals ([`super::sealing`]) and the indexed
//! attributes of [`super::queryable`]. Since 2026-09-21 each keys its entries
//! on the file's [`FileId`] -- `(fs_id, ino)` -- so that two names for one
//! file share one entry, falling back to the path where a filesystem has no
//! inode numbers.
//!
//! A `FileId` names a *live* file. Its `fs_id` half is never reused, but the
//! inode number is: ext4 gives a freed inode to the next file it creates. An
//! entry that outlives its file is therefore inherited by a stranger -- the new
//! file finds itself carrying the old one's ACL, its flags, its seals and its
//! search attributes. Nothing removed an entry when its file went until
//! 2026-09-25 (the seal table even had a `remove_on_delete` that nothing
//! called), and no boot noticed because `/tmp` is memfs, which counts inode
//! numbers up and never reuses one. `known-issues.md`
//! `A-PER-FILE-STATE-OUTLIVED-ITS-FILE` has the history.
//!
//! So the VFS reports the events that change what a key means, and every such
//! table is listed in [`TABLES`] to hear them:
//!
//! | Event | Sent when | What a table does |
//! |---|---|---|
//! | [`object_unlinked`] | a name was removed and it was the object's last | drops the entry: the file is gone and its number is free |
//! | [`names_moved`] | `from` was renamed to `to` | moves every stored name at or under `from` |
//! | [`names_exchanged`] | `RENAME_EXCHANGE` swapped `a` and `b` | swaps the names at or under each |
//! | [`filesystem_unmounted`] | a mount went away | drops every entry on it: its `fs_id` cannot match again |
//!
//! A rename changes no identity -- the file is the same file -- so for an
//! identity-keyed entry it moves only the name the table keeps to *report*
//! the entry by (`/proc` listings, query results). For a path-keyed entry the
//! name is the key, and it moves with the file.
//!
//! Removing a name that is not the last -- one of two hard links -- ends
//! nothing: the file lives on under the other name, and so does its state.
//!
//! ## Why the last name, and not the last close
//!
//! POSIX frees an inode when its last name *and* its last open handle are
//! gone, so a file unlinked while open is still a file. The state here ends
//! at the last name regardless, because every consumer reaches it through a
//! name: ACLs are consulted at path resolution, the flags and seals on path
//! operations, and the attributes by path queries. With the last name gone,
//! nothing can consult any of it for this file again.
//!
//! ## Known limits
//!
//! - The name an entry is *reported* under is the one it was set through, as
//!   moved by later renames. If that name is removed or replaced while the
//!   file lives on under another, the report names the old one until the file
//!   goes: nothing maps an identity back to its surviving names.
//! - A rename rewrites names by scanning each table: nothing when the tables
//!   are empty, which is almost always, and a few milliseconds at the 65,536
//!   entries `queryable` and `immutable` allow.
//!
//! ## Lock order
//!
//! Every event is sent after the VFS has released the filesystem lock -- the
//! place the page cache is invalidated -- and each table takes only its own
//! lock and calls nothing back in the VFS. Filesystem lock, then module state,
//! is the kernel's order (`scripts/check-vfs-under-lock.py`).
//!
//! ## The long-term home
//!
//! Linux keeps this state in the inode -- ACLs as `system.posix_acl_access`
//! xattrs, the flags in the inode itself -- so it ends with the inode by
//! construction, and persists across reboots, which these in-memory tables do
//! not. The VFS has working xattrs, ext4's included. Moving the tables there
//! would retire most of this module; until then it is what keeps them honest.

use super::path::{Path, PathBuf};
use super::vfs::{EntryType, FileId, FileMeta};
use crate::error::KernelResult;
use crate::serial_println;

/// A rename, as a table sees it: the new name for a stored name that moved,
/// `None` for one that did not.
///
/// The lifetime is not decoration: the VFS builds this as a closure over the
/// rename's own two paths, so it borrows them, and an alias without one would
/// default to `'static` and admit no such closure.
pub(crate) type NameMap<'a> = dyn Fn(&Path) -> Option<PathBuf> + 'a;

/// One table of per-file state, as the VFS sees it.
pub(crate) struct Table {
    /// For diagnostics and the self-test.
    pub name: &'static str,
    /// The file is gone: drop its entry. `id` is the identity it had, read by
    /// the VFS while its last name still resolved; `path` is that name, which
    /// is the key where `id` is `None`.
    pub forget: fn(Option<FileId>, &Path),
    /// Rewrite every stored name through `rename`, which gives the new name for
    /// a name that moved and `None` for one that did not.
    pub rename: fn(&NameMap<'_>),
    /// The filesystem `fs_id` was unmounted: drop every entry keyed on it.
    pub unmounted: fn(u64),
    /// Self-test support: give the file at `path` a harmless entry -- one that
    /// changes no behaviour of the file while the test holds it.
    pub plant: fn(&Path) -> KernelResult<()>,
    /// Self-test support: whether a lookup through `path` finds that entry.
    pub finds: fn(&Path) -> bool,
    /// Self-test support: whether the table reports an entry under `name`.
    pub reports: fn(&Path) -> bool,
}

/// Every table of per-file state. A new table keyed on [`FileId`] belongs
/// here, or it will outlive its files as these four did; the self-test then
/// exercises it with the rest.
pub(crate) const TABLES: &[Table] = &[
    super::acl::PER_FILE_STATE,
    super::immutable::PER_FILE_STATE,
    super::sealing::PER_FILE_STATE,
    super::queryable::PER_FILE_STATE,
];

/// What removing one name of an object ends.
///
/// Read by the VFS under the same filesystem lock as the removal, while the
/// name still resolves: afterwards the inode, and its number, may already be
/// someone else's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Unlinked {
    /// The object's identity; `None` where its filesystem has no inode numbers.
    pub id: Option<FileId>,
    /// Whether the name being removed is the object's last, so that the object
    /// goes with it.
    pub last_name: bool,
}

impl Unlinked {
    /// From the object's own metadata. The VFS passes `lmetadata`, not
    /// `metadata`: removing a symlink removes the link, and following it would
    /// end the state of the file it points to.
    pub(crate) fn from_meta(fs_id: u64, meta: &FileMeta) -> Self {
        let id = (meta.ino != 0).then_some(FileId {
            fs_id,
            ino: meta.ino,
        });
        // A directory has exactly one name -- there are no hard links to
        // directories -- and its `nlinks` counts `.` and each child's `..`,
        // not names, so it says nothing here.
        let last_name = meta.entry_type == EntryType::Directory || meta.nlinks <= 1;
        Self { id, last_name }
    }
}

/// The name `path` was removed; `unlinked` is what the VFS read of it
/// beforehand.
///
/// `None` means that read failed and the removal succeeded anyway, so any
/// state the object had is left behind -- the defect this module exists to
/// prevent. It is rare (the read and the removal consult the same name under
/// the same lock), and it is said aloud rather than passed over.
pub(crate) fn name_removed(unlinked: Option<Unlinked>, path: &Path) {
    match unlinked {
        Some(unlinked) => object_unlinked(unlinked, path),
        None => serial_println!(
            "[perfile] WARNING: '{}' was removed, but its metadata could not be read \
             first; any ACL, flags, seals or attributes it had are left behind",
            path.display()
        ),
    }
}

/// A name was removed from the object `unlinked` describes, and `path` was
/// that name. Ends the object's state if it was the last name.
pub(crate) fn object_unlinked(unlinked: Unlinked, path: &Path) {
    if !unlinked.last_name {
        return;
    }
    for table in TABLES {
        (table.forget)(unlinked.id, path);
    }
}

/// `from` was renamed to `to` (for a directory, with everything under it).
///
/// A replacing rename also removes the name `to` from whatever held it; the
/// VFS reports that first, through [`object_unlinked`], so the replaced
/// file's state is gone before the moved file's names arrive.
pub(crate) fn names_moved(from: &Path, to: &Path) {
    let rename = |name: &Path| super::pathutil::rebase(name, from, to);
    for table in TABLES {
        (table.rename)(&rename);
    }
}

/// `a` and `b` swapped places (`RENAME_EXCHANGE`).
///
/// Every name moves at most once: a filesystem refuses to exchange a
/// directory with something inside it, so no name is under both.
pub(crate) fn names_exchanged(a: &Path, b: &Path) {
    let swap = |name: &Path| {
        super::pathutil::rebase(name, a, b).or_else(|| super::pathutil::rebase(name, b, a))
    };
    for table in TABLES {
        (table.rename)(&swap);
    }
}

/// The filesystem `fs_id` was unmounted.
pub(crate) fn filesystem_unmounted(fs_id: u64) {
    for table in TABLES {
        (table.unmounted)(fs_id);
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Where the rungs work. `/tmp` is memfs, which has inode numbers, hard links
/// and `RENAME_EXCHANGE` -- everything the rungs need.
const DIR: &str = "/tmp/perfile";

/// Report a rung's failure for one table and return the error.
fn fail(rung: &str, table: &Table, what: &str) -> crate::error::KernelError {
    serial_println!("[perfile]   FAIL: {}: {} {}", rung, table.name, what);
    crate::error::KernelError::InternalError
}

/// Check `cond(table)` for every table, failing with `what` on the first that
/// does not hold.
fn each(rung: &str, what: &str, cond: impl Fn(&Table) -> bool) -> KernelResult<()> {
    for table in TABLES {
        if !cond(table) {
            return Err(fail(rung, table, what));
        }
    }
    Ok(())
}

/// Plant an entry in every table on the file at `path`, and confirm each one
/// took it -- the control every later "is gone" assertion leans on: a table
/// that never held the entry would pass them all.
fn plant_all(rung: &str, path: &Path) -> KernelResult<()> {
    for table in TABLES {
        if let Err(e) = (table.plant)(path) {
            serial_println!(
                "[perfile]   FAIL: {}: {} refused the planted entry: {:?}",
                rung,
                table.name,
                e
            );
            return Err(e);
        }
    }
    each(rung, "did not find the entry just planted", |t| {
        (t.finds)(path)
    })?;
    each(rung, "did not report the entry just planted", |t| {
        (t.reports)(path)
    })
}

/// Remove whatever the rungs may have left, ignoring what is not there.
fn cleanup(names: &[&[u8]]) {
    use crate::fs::Vfs;
    for name in names {
        let path = Path::new(name);
        // A leftover of either kind; the other call fails with the wrong type,
        // or NotFound, and both are what "already clean" looks like.
        let _ = Vfs::remove(path);
        let _ = Vfs::rmdir(path);
    }
}

/// Removing one of two names ends nothing; removing the last ends everything.
fn rung_last_name(skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    use crate::fs::Vfs;
    use crate::fs::selftest::{Setup, classify};
    const R: &str = "last name";
    let (a, b) = (Path::new(b"/tmp/perfile/a"), Path::new(b"/tmp/perfile/b"));

    Vfs::write_file(a, b"x")?;
    match classify(Vfs::link(a, b)) {
        Setup::Ready => {}
        Setup::Unsupported(e) => {
            serial_println!("[perfile]   SKIP: {}: link() unsupported here: {:?}", R, e);
            skips.record("last name", "link() unsupported on /tmp");
            return Ok(());
        }
        Setup::Failed(e) => {
            serial_println!("[perfile]   FAIL: {}: link() refused with {:?}", R, e);
            return Err(e);
        }
    }
    plant_all(R, a)?;
    each(R, "did not find the entry through a second name", |t| {
        (t.finds)(b)
    })?;

    Vfs::remove(a)?;
    each(
        R,
        "lost the entry when one of two names was removed -- the file still exists",
        |t| (t.finds)(b),
    )?;

    Vfs::remove(b)?;
    each(
        R,
        "still reports the entry after the file's last name was removed",
        |t| !(t.reports)(a) && !(t.reports)(b),
    )?;
    serial_println!("[perfile]   one of two names removed keeps the state; the last ends it: OK");
    Ok(())
}

/// A replacing rename ends the replaced file's state.
fn rung_replace() -> KernelResult<()> {
    use crate::fs::Vfs;
    const R: &str = "replace";
    let (s, t) = (Path::new(b"/tmp/perfile/s"), Path::new(b"/tmp/perfile/t"));

    Vfs::write_file(s, b"x")?;
    Vfs::write_file(t, b"x")?;
    plant_all(R, t)?;
    Vfs::rename(s, t)?;
    each(
        R,
        "still holds the state of the file a rename replaced",
        |table| !(table.reports)(t) && !(table.finds)(t),
    )?;
    Vfs::remove(t)?;
    serial_println!("[perfile]   a replacing rename ends the replaced file's state: OK");
    Ok(())
}

/// A rename moves the reported name, and the state stays with the file.
fn rung_rename() -> KernelResult<()> {
    use crate::fs::Vfs;
    const R: &str = "rename";
    let (x, y) = (Path::new(b"/tmp/perfile/x"), Path::new(b"/tmp/perfile/y"));

    Vfs::write_file(x, b"x")?;
    plant_all(R, x)?;
    Vfs::rename(x, y)?;
    each(R, "lost the entry across a rename", |t| (t.finds)(y))?;
    each(
        R,
        "reports the entry under the name it was renamed from",
        |t| (t.reports)(y) && !(t.reports)(x),
    )?;
    Vfs::remove(y)?;
    each(
        R,
        "still reports the entry after the file was removed",
        |t| !(t.reports)(y),
    )?;
    serial_println!("[perfile]   a rename moves the reported name: OK");
    Ok(())
}

/// Renaming a directory moves the names of everything under it.
fn rung_directory() -> KernelResult<()> {
    use crate::fs::Vfs;
    const R: &str = "directory rename";
    let (d, e) = (Path::new(b"/tmp/perfile/d"), Path::new(b"/tmp/perfile/e"));
    let (df, ef) = (
        Path::new(b"/tmp/perfile/d/f"),
        Path::new(b"/tmp/perfile/e/f"),
    );

    Vfs::mkdir(d)?;
    Vfs::write_file(df, b"x")?;
    plant_all(R, df)?;
    Vfs::rename(d, e)?;
    each(R, "reports a file under its directory's old name", |t| {
        (t.reports)(ef) && !(t.reports)(df)
    })?;
    each(R, "lost the entry when its directory was renamed", |t| {
        (t.finds)(ef)
    })?;
    Vfs::remove(ef)?;
    Vfs::rmdir(e)?;
    serial_println!("[perfile]   a directory rename moves the names beneath it: OK");
    Ok(())
}

/// `RENAME_EXCHANGE` swaps the names, and each file keeps its own state.
fn rung_exchange(skips: &mut crate::fs::selftest::Skips) -> KernelResult<()> {
    use crate::fs::Vfs;
    use crate::fs::selftest::{Setup, classify};
    const R: &str = "exchange";
    let (p, q) = (Path::new(b"/tmp/perfile/p"), Path::new(b"/tmp/perfile/q"));

    Vfs::write_file(p, b"x")?;
    Vfs::write_file(q, b"x")?;
    // Only p carries state, so the two files can be told apart afterwards.
    plant_all(R, p)?;
    match classify(Vfs::rename_exchange(p, q)) {
        Setup::Ready => {}
        Setup::Unsupported(e) => {
            serial_println!("[perfile]   SKIP: {}: unsupported here: {:?}", R, e);
            skips.record("exchange", "RENAME_EXCHANGE unsupported on /tmp");
            return Ok(());
        }
        Setup::Failed(e) => {
            serial_println!("[perfile]   FAIL: {}: refused with {:?}", R, e);
            return Err(e);
        }
    }
    each(R, "did not follow its file across an exchange", |t| {
        (t.finds)(q) && !(t.finds)(p)
    })?;
    each(
        R,
        "reports the entry under the name its file gave up",
        |t| (t.reports)(q) && !(t.reports)(p),
    )?;
    Vfs::remove(p)?;
    Vfs::remove(q)?;
    serial_println!("[perfile]   an exchange swaps the reported names: OK");
    Ok(())
}

/// Unmounting a filesystem ends the state of every file on it.
fn rung_unmount() -> KernelResult<()> {
    use crate::fs::Vfs;
    const R: &str = "unmount";
    let (m, f) = (Path::new(b"/tmp/perfile/m"), Path::new(b"/tmp/perfile/m/f"));

    Vfs::mkdir(m)?;
    crate::fs::memfs::mount(m)?;
    let body = (|| -> KernelResult<()> {
        Vfs::write_file(f, b"x")?;
        plant_all(R, f)?;
        Ok(())
    })();
    // Unmounted whatever the body did: a mount left inside /tmp would outlive
    // the test and shadow the directory under it.
    let unmounted = Vfs::unmount(m);
    body?;
    unmounted?;
    each(
        R,
        "still reports a file on a filesystem that was unmounted",
        |t| !(t.reports)(f),
    )?;
    Vfs::rmdir(m)?;
    serial_println!("[perfile]   an unmount ends the state of every file on it: OK");
    Ok(())
}

/// Every table name is distinct -- a table listed twice would hear every
/// event twice, and the second `forget` of an entry is harmless but a second
/// `rename` of `/a` to `/a/b` would move it again, to `/a/b/b`.
fn check_registry() -> KernelResult<()> {
    for (i, t) in TABLES.iter().enumerate() {
        if TABLES
            .iter()
            .skip(i.saturating_add(1))
            .any(|u| u.name == t.name)
        {
            serial_println!("[perfile]   FAIL: table '{}' is registered twice", t.name);
            return Err(crate::error::KernelError::InternalError);
        }
    }
    Ok(())
}

/// Boot self-test: drives every table in [`TABLES`] through each lifecycle
/// event via the real VFS, on `/tmp`.
///
/// Needs `/tmp` mounted and the VFS up; runs after the four tables' own
/// self-tests.
///
/// # Errors
///
/// [`crate::error::KernelError::InternalError`] (or the VFS error that stopped
/// a rung) when a table keeps state its file no longer has, loses state its
/// file still has, or reports it under the wrong name.
pub fn self_test() -> KernelResult<()> {
    use crate::fs::Vfs;
    serial_println!("[perfile] Running self-test ({} tables)...", TABLES.len());
    let mut skips = crate::fs::selftest::Skips::new();
    check_registry()?;

    const ALL: &[&[u8]] = &[
        b"/tmp/perfile/a",
        b"/tmp/perfile/b",
        b"/tmp/perfile/s",
        b"/tmp/perfile/t",
        b"/tmp/perfile/x",
        b"/tmp/perfile/y",
        b"/tmp/perfile/d/f",
        b"/tmp/perfile/d",
        b"/tmp/perfile/e/f",
        b"/tmp/perfile/e",
        b"/tmp/perfile/p",
        b"/tmp/perfile/q",
        b"/tmp/perfile/m/f",
        b"/tmp/perfile/m",
        b"/tmp/perfile",
    ];
    cleanup(ALL);
    Vfs::mkdir(DIR)?;
    let result = (|| -> KernelResult<()> {
        rung_last_name(&mut skips)?;
        rung_replace()?;
        rung_rename()?;
        rung_directory()?;
        rung_exchange(&mut skips)?;
        rung_unmount()?;
        Ok(())
    })();
    // On failure a rung stops where it failed; take everything down so the
    // next boot's run -- and anything else using /tmp -- starts clean.
    cleanup(ALL);
    result?;

    skips.report("[perfile]");
    serial_println!("[perfile] Self-test passed{}", skips.suffix());
    Ok(())
}
