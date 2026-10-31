//! Directory trees a capability can name: what a [`ResourceType::FileTree`]
//! capability's `resource_id` is.
//!
//! A `File` capability is authority over every file there is; a process that
//! must read one directory's files -- a Linux driver process its firmware,
//! say -- would otherwise have to be given all of them. A `FileTree`
//! capability is the narrower right: to open, to read only, a file beneath
//! one directory, the walk confined beneath it (`fs::handle::open_beneath`)
//! so that neither `..` nor a symbolic link leads out.
//!
//! A tree is only as narrow as its directory is fixed, so a directory must
//! be on a filesystem that is read-only as a whole to be one -- not merely
//! through one mount of it -- as the boot medium's firmware is, sealed once
//! placed: with no file permissions to stop it, anyone who may write a
//! filesystem could link any file into a directory on it. A tree is pinned
//! to the directory it was registered on -- its filesystem and its inode --
//! and named as a walk finds it, through no symbolic link; every lookup
//! ([`beneath`]) checks all of that again, as the caller sees the path, so
//! that a filesystem mounted over it, a link or a namespace's binding put on
//! its way, or the filesystem made writable, leaves the tree reading
//! nothing. And what is opened through a tree must be on the tree's own
//! filesystem ([`Route::dev`]): a filesystem mounted beneath it is not the
//! tree.
//!
//! The directories are the kernel's, kept here: a capability names a tree by
//! the id [`register`] gave it, never by a path or a hash of one that a
//! process could make collide with another. An id is never given to another
//! directory, and a directory registered again keeps its id (pinned anew to
//! what is there then), so a driver started many times does not grow the
//! table.
//!
//! [`ResourceType::FileTree`]: crate::cap::ResourceType::FileTree

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::fs::vfs::{FileId, Vfs};
use crate::sync::Mutex;

/// A registered tree: its directory, named plainly, and what it was pinned
/// on.
#[derive(Clone)]
struct Tree {
    dir: PathBuf,
    pinned: FileId,
}

/// The registered trees: the one at index `i` has id `i + 1`, so that id 0
/// names none.
static TREES: Mutex<Vec<Tree>> = Mutex::new(Vec::new());

/// The most directories the table holds: far more than the kernel's own
/// grants need, and a bound on what a bug calling [`register`] in a loop
/// could take.
const MOST: usize = 64;

/// Where a path lies in a tree ([`beneath`]): what `open_beneath` walks.
pub struct Route {
    /// The tree's directory.
    pub base: PathBuf,
    /// The rest of the path beneath it; empty for the directory itself.
    pub rest: PathBuf,
    /// The device number (`st_dev`) of the tree's filesystem, which a file
    /// opened through the tree must have: one on another filesystem was
    /// reached through a mount beneath the tree, which is not the tree.
    pub dev: u32,
}

/// The id naming the directory `dir`, for a `FileTree` capability: the one
/// it was given before, else a new one -- pinned either way to the
/// directory there now.
///
/// # Errors
///
/// `InvalidArgument` for a `dir` that is not absolute, holds a `.` or `..`
/// component, or is not named as a walk finds it -- through a symbolic
/// link, say -- so that [`beneath`] compares like with like; `NotFound` or
/// `NotADirectory` for no directory there; `NotSupported` for one on a
/// filesystem with no identity to pin it by; `PermissionDenied` for one on
/// a filesystem that is not read-only as a whole; `OutOfMemory` when the
/// table is full.
pub fn register(dir: &Path) -> KernelResult<u64> {
    if !dir.is_absolute() || !dir.has_no_dot_components() {
        return Err(KernelError::InvalidArgument);
    }
    let (dir, pinned) = pin(&plain(dir))?;
    register_in(&mut TREES.lock(), dir, pinned, MOST)
}

/// `path`, absolute, named plainly: each separator once, and no `.`
/// component -- the one spelling both [`register`] and [`beneath`] compare.
fn plain(path: &Path) -> PathBuf {
    let mut plain = PathBuf::from("/");
    for c in path.components().filter(|c| c.as_bytes() != b".") {
        plain.push(c);
    }
    plain
}

/// The directory `dir`, named plainly, pinned: what it is now, as the
/// caller sees it -- a directory, named as a walk finds it, on a filesystem
/// read-only as a whole, with an identity.
fn pin(dir: &Path) -> KernelResult<(PathBuf, FileId)> {
    if Vfs::resolve_path(dir)?.as_path() != dir {
        return Err(KernelError::InvalidArgument);
    }
    let pinned = Vfs::pin_dir(dir)?.id.ok_or(KernelError::NotSupported)?;
    if !Vfs::filesystem_read_only(dir)? {
        return Err(KernelError::PermissionDenied);
    }
    Ok((dir.to_path_buf(), pinned))
}

/// [`register`] on `trees`, holding at most `most`: the id `dir` has there,
/// or a new one.
fn register_in(
    trees: &mut Vec<Tree>,
    dir: PathBuf,
    pinned: FileId,
    most: usize,
) -> KernelResult<u64> {
    let at = match trees.iter().position(|t| t.dir == dir) {
        Some(at) => {
            if let Some(tree) = trees.get_mut(at) {
                tree.pinned = pinned;
            }
            at
        }
        None => {
            if trees.len() >= most {
                return Err(KernelError::OutOfMemory);
            }
            trees.push(Tree { dir, pinned });
            trees.len().saturating_sub(1)
        }
    };
    u64::try_from(at)
        .ok()
        .and_then(|at| at.checked_add(1))
        .ok_or(KernelError::InternalError)
}

/// Where `path` lies in the tree `id` names ([`Route`]), as the calling
/// process sees both; `None` when `id` names no tree, when `path`, which
/// must be absolute, is not beneath its directory, component by component
/// (`/ab` is not beneath `/a`; a `.` component is no component), or when
/// the directory is no longer the one pinned, as the caller resolves it --
/// another filesystem mounted there, another directory, a link or a
/// namespace's binding put on its way, its filesystem no longer read-only.
#[must_use]
pub fn beneath(id: u64, path: &Path) -> Option<Route> {
    let at = usize::try_from(id.checked_sub(1)?).ok()?;
    let tree = TREES.lock().get(at)?.clone();
    if !path.is_absolute() {
        return None;
    }
    let rest = plain(path).strip_prefix(&tree.dir)?;
    match pin(&tree.dir) {
        Ok((_, now)) if now == tree.pinned => Some(Route {
            base: tree.dir,
            rest,
            dev: crate::fs::vfs::dev_of(tree.pinned.fs_id),
        }),
        _ => None,
    }
}

/// The registry at its edges: a directory registered again keeps its id; a
/// relative one, one with `.` or `..` in it, one reached through a symbolic
/// link, one that is not there, one on a writable filesystem and one on a
/// read-only bind mount of a writable filesystem refused; the table's
/// bound; and [`beneath`] -- component by component, `.` passed over,
/// absolute paths only, no tree for id 0 or one never given, its route
/// carrying the tree's filesystem, and none once that filesystem is made
/// writable or unmounted.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right; the VFS's, for a
/// fixture it could not make.
pub fn self_test() -> KernelResult<()> {
    const ROOT: &str = "/.trees-test";
    const WRITABLE: &str = "/.trees-test-rw";
    const BOUND: &str = "/.trees-test/bound";
    let fail = |what: &str| {
        crate::serial_println!("[trees]   FAIL: {what}");
        KernelError::InternalError
    };
    let sealed = |read_only: bool| {
        let mut options = Vfs::mount_options(ROOT)?;
        options.read_only = read_only;
        Vfs::remount(ROOT, options)
    };
    // A filesystem read-only as a whole, as the boot medium's firmware is
    // placed; a writable one beside it, with a link into the first; and a
    // read-only bind mount of the writable one.
    crate::fs::memfs::mount(ROOT)?;
    let made = crate::fs::memfs::mount(WRITABLE).and_then(|()| {
        Vfs::mkdir_all(alloc::format!("{ROOT}/a/fw").as_str())?;
        Vfs::mkdir_all(alloc::format!("{ROOT}/b").as_str())?;
        Vfs::mkdir(BOUND)?;
        Vfs::mkdir(alloc::format!("{WRITABLE}/d").as_str())?;
        sealed(true)?;
        Vfs::bind_mount(alloc::format!("{WRITABLE}/d").as_str(), BOUND, false)?;
        let mut options = Vfs::mount_options(BOUND)?;
        options.read_only = true;
        Vfs::remount_bind(BOUND, options)?;
        // A filesystem without symbolic links leaves that one case out.
        Ok(Vfs::symlink(alloc::format!("{WRITABLE}/link").as_str(), "/.trees-test/a").is_ok())
    });
    let result = made.and_then(|linked| {
        let a = register(Path::new("/.trees-test/a"))?;
        let b = register(Path::new("/.trees-test//b/"))?;
        if a == 0 || b == a || register(Path::new("/.trees-test/a"))? != a {
            return Err(fail("a directory registered again did not keep its id"));
        }
        if register(Path::new("/.trees-test/b"))? != b {
            return Err(fail(
                "a directory named with a doubled or trailing separator got an id of its own",
            ));
        }
        let mut refusals: Vec<(&str, KernelError)> = alloc::vec![
            ("relative/dir", KernelError::InvalidArgument),
            ("/a/./b", KernelError::InvalidArgument),
            ("/a/../b", KernelError::InvalidArgument),
            ("/.trees-test/none", KernelError::NotFound),
            (WRITABLE, KernelError::PermissionDenied),
            (BOUND, KernelError::PermissionDenied),
        ];
        if linked {
            refusals.push(("/.trees-test-rw/link", KernelError::InvalidArgument));
        }
        for (dir, want) in refusals {
            if register(Path::new(dir)) != Err(want) {
                crate::serial_println!(
                    "[trees]   FAIL: registering {dir} answered {:?}, not {want:?}",
                    register(Path::new(dir))
                );
                return Err(KernelError::InternalError);
            }
        }
        let (_, pinned) = pin(Path::new("/.trees-test/a"))?;
        let mut small = Vec::new();
        let first = register_in(&mut small, PathBuf::from("/x"), pinned, 2)?;
        let second = register_in(&mut small, PathBuf::from("/y"), pinned, 2)?;
        let again = register_in(&mut small, PathBuf::from("/x"), pinned, 2)?;
        let third = register_in(&mut small, PathBuf::from("/z"), pinned, 2);
        if (first, second, again, third) != (1, 2, 1, Err(KernelError::OutOfMemory)) {
            return Err(fail(
                "a full table took another directory, or ids ran wrong",
            ));
        }
        let rest = |id: u64, path: &str| beneath(id, Path::new(path)).map(|r| r.rest);
        let cases: [(u64, &str, Option<&str>); 9] = [
            (a, "/.trees-test/a/fw/x.bin", Some("fw/x.bin")),
            (a, "/.trees-test/a/./fw/x.bin", Some("fw/x.bin")),
            (a, "/.trees-test/a", Some("")),
            (a, "/.trees-test/ab/x.bin", None),
            (a, "/.trees-test/b/x.bin", None),
            (a, ".trees-test/a/x.bin", None),
            (a, "/.trees-test/a/../b/x.bin", Some("../b/x.bin")),
            (0, "/.trees-test/a/x.bin", None),
            (u64::MAX, "/.trees-test/a/x.bin", None),
        ];
        for (id, path, want) in cases {
            let got = rest(id, path);
            if got.as_ref().map(|r| r.as_path()) != want.map(Path::new) {
                crate::serial_println!(
                    "[trees]   FAIL: the tree {id} placed {path} at {:?}, not {want:?}",
                    got.as_ref().map(|r| r.as_path())
                );
                return Err(KernelError::InternalError);
            }
        }
        if beneath(a, Path::new("/.trees-test/a/fw")).map(|r| r.dev)
            != Some(crate::fs::vfs::dev_of(pinned.fs_id))
        {
            return Err(fail("a route did not carry its tree's filesystem"));
        }
        // Writable again, the tree is none; sealed again, it is.
        sealed(false)?;
        let while_writable = beneath(a, Path::new("/.trees-test/a/fw")).is_some();
        sealed(true)?;
        if while_writable || beneath(a, Path::new("/.trees-test/a/fw")).is_none() {
            return Err(fail(
                "a tree placed a path while its filesystem was writable, or not once sealed again",
            ));
        }
        Ok(a)
    });
    // Ignored on purpose: the fixtures' going is what the last check reads,
    // and a failure there is said by it.
    let _ = Vfs::unmount(BOUND);
    let _ = Vfs::unmount(WRITABLE);
    let _ = Vfs::unmount(ROOT);
    let a = result?;
    if beneath(a, Path::new("/.trees-test/a/fw/x.bin")).is_some() {
        return Err(fail(
            "a tree whose filesystem was unmounted still placed a path in it",
        ));
    }
    crate::serial_println!(
        "[trees]   a tree's id kept; its directory named plainly, as a walk finds it, on a filesystem read-only as a whole -- not a read-only bind of a writable one; the table bounded; a path beneath it found component by component, its route carrying the tree's filesystem; and a tree whose filesystem was made writable or went placing nothing: OK"
    );
    Ok(())
}
