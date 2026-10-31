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
//! be on a read-only filesystem to be one -- the boot medium's firmware is,
//! sealed once placed. A writable one is not: with no file permissions to
//! stop it, anyone who may write there could link any file into it, or put a
//! symbolic link where the directory was. And a tree is pinned to the
//! directory it was registered on -- its filesystem and its inode -- which
//! every lookup checks again ([`beneath`]): a filesystem mounted over the
//! path later is another directory, and the tree reads nothing there.
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

/// The id naming the directory `dir`, for a `FileTree` capability: the one
/// it was given before, else a new one -- pinned either way to the
/// directory there now.
///
/// # Errors
///
/// `InvalidArgument` for a `dir` that is not absolute or that holds a `.` or
/// `..` component -- a tree is named as the walk will find it, so that
/// [`beneath`] compares like with like; `NotFound` or `NotADirectory` for no
/// directory there; `NotSupported` for one on a filesystem with no identity
/// to pin it by; `PermissionDenied` for one on a writable filesystem;
/// `OutOfMemory` when the table is full.
pub fn register(dir: &Path) -> KernelResult<u64> {
    let (dir, pinned) = pin(dir)?;
    register_in(&mut TREES.lock(), dir, pinned, MOST)
}

/// `dir` named plainly -- absolute, every component a name, each separator
/// once -- and pinned: what it is now, a directory on a read-only
/// filesystem with an identity.
fn pin(dir: &Path) -> KernelResult<(PathBuf, FileId)> {
    if !dir.is_absolute() || !dir.has_no_dot_components() {
        return Err(KernelError::InvalidArgument);
    }
    let mut plain = PathBuf::from("/");
    for c in dir.components() {
        plain.push(c);
    }
    let pinned = Vfs::pin_dir(&plain)?.id.ok_or(KernelError::NotSupported)?;
    if !Vfs::mount_options(&plain)?.read_only {
        return Err(KernelError::PermissionDenied);
    }
    Ok((plain, pinned))
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

/// Where `path` lies in the tree `id` names: the tree's directory and the
/// rest of `path` beneath it, for `open_beneath` to walk without leaving it;
/// `None` when `id` names no tree, when its directory is no longer the one
/// it was pinned on -- another filesystem mounted there, another directory
/// put there, a filesystem no longer read-only -- or when `path`, which must
/// be absolute, is not beneath it, component by component (`/ab` is not
/// beneath `/a`; a `.` component is no component). The rest is empty for
/// the directory itself.
#[must_use]
pub fn beneath(id: u64, path: &Path) -> Option<(PathBuf, PathBuf)> {
    let at = usize::try_from(id.checked_sub(1)?).ok()?;
    let tree = TREES.lock().get(at)?.clone();
    if !path.is_absolute() {
        return None;
    }
    let mut plain = PathBuf::from("/");
    for c in path.components().filter(|c| c.as_bytes() != b".") {
        plain.push(c);
    }
    let rest = plain.strip_prefix(&tree.dir)?;
    match pin(&tree.dir) {
        Ok((_, now)) if now == tree.pinned => Some((tree.dir, rest)),
        _ => None,
    }
}

/// The registry at its edges: a directory registered again keeps its id; a
/// relative one, one with `.` or `..` in it, one that is not there and one
/// on a writable filesystem refused; the table's bound; and [`beneath`] --
/// component by component, `.` passed over, absolute paths only, no tree
/// for id 0, one never given, or one whose directory is no longer the one
/// pinned.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right; the VFS's, for a
/// fixture it could not make.
pub fn self_test() -> KernelResult<()> {
    const ROOT: &str = "/.trees-test";
    let fail = |what: &str| {
        crate::serial_println!("[trees]   FAIL: {what}");
        KernelError::InternalError
    };
    // A read-only filesystem of two directories, as the boot medium's
    // firmware is placed.
    crate::fs::memfs::mount(ROOT)?;
    let made = (|| {
        Vfs::mkdir_all(alloc::format!("{ROOT}/a/fw").as_str())?;
        Vfs::mkdir_all(alloc::format!("{ROOT}/b").as_str())?;
        let mut options = Vfs::mount_options(ROOT)?;
        options.read_only = true;
        Vfs::remount(ROOT, options)
    })();
    let result = made.and_then(|()| {
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
        let refusals: [(&str, KernelError); 5] = [
            ("relative/dir", KernelError::InvalidArgument),
            ("/a/./b", KernelError::InvalidArgument),
            ("/a/../b", KernelError::InvalidArgument),
            ("/.trees-test/none", KernelError::NotFound),
            // The root filesystem is writable.
            ("/", KernelError::PermissionDenied),
        ];
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
        let rest = |id: u64, path: &str| beneath(id, Path::new(path)).map(|(_, rest)| rest);
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
        Ok(a)
    });
    // Ignored on purpose: the fixture's going is what the last check reads,
    // and a failure there is said by it.
    let _ = Vfs::unmount(ROOT);
    let a = result?;
    if beneath(a, Path::new("/.trees-test/a/fw/x.bin")).is_some() {
        return Err(fail(
            "a tree whose filesystem was unmounted still placed a path in it",
        ));
    }
    crate::serial_println!(
        "[trees]   a tree's id kept, its directory named plainly and read-only, the table bounded, a path beneath it found component by component, and a tree whose directory went placing nothing: OK"
    );
    Ok(())
}
