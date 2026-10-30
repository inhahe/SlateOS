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
//! The directories are the kernel's, kept here: a capability names a tree by
//! the id [`register`] gave it, never by a path or a hash of one that a
//! process could make collide with another. An id is never given to another
//! directory, and a directory registered again keeps its id, so a driver
//! started many times does not grow the table.
//!
//! [`ResourceType::FileTree`]: crate::cap::ResourceType::FileTree

use alloc::vec::Vec;

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::sync::Mutex;

/// The registered directories: the one at index `i` has id `i + 1`, so that
/// id 0 names none.
static TREES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The most directories the table holds: far more than the kernel's own
/// grants need, and a bound on what a bug calling [`register`] in a loop
/// could take.
const MOST: usize = 64;

/// The id naming the directory `dir`, for a `FileTree` capability: the one
/// it was given before, else a new one.
///
/// # Errors
///
/// `InvalidArgument` for a `dir` that is not absolute or that holds a `.` or
/// `..` component -- a tree is named as the walk will find it, so that
/// [`beneath`] compares like with like; `OutOfMemory` when the table is full.
pub fn register(dir: &Path) -> KernelResult<u64> {
    let plain = dir.is_absolute()
        && dir
            .components()
            .all(|c| c.as_bytes() != b"." && c.as_bytes() != b"..");
    if !plain {
        return Err(KernelError::InvalidArgument);
    }
    let mut trees = TREES.lock();
    let at = match trees.iter().position(|t| t.as_path() == dir) {
        Some(at) => at,
        None => {
            if trees.len() >= MOST {
                return Err(KernelError::OutOfMemory);
            }
            trees.push(dir.to_path_buf());
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
/// `None` when `id` names no tree, or `path` -- which must be absolute -- is
/// not beneath its directory, component by component (`/ab` is not beneath
/// `/a`). The rest is empty for the directory itself.
#[must_use]
pub fn beneath(id: u64, path: &Path) -> Option<(PathBuf, PathBuf)> {
    let at = usize::try_from(id.checked_sub(1)?).ok()?;
    let dir = TREES.lock().get(at)?.clone();
    if !path.is_absolute() {
        return None;
    }
    let rest = path.strip_prefix(&dir)?;
    Some((dir, rest))
}

/// The registry at its edges: ids from 1 and kept for a directory registered
/// again, refusals of a relative directory or one with `.` or `..` in it,
/// and [`beneath`] -- component by component, absolute paths only, and no
/// tree for id 0 or one never given.
///
/// # Errors
///
/// `InternalError`, said, for an answer that is not right.
pub fn self_test() -> KernelResult<()> {
    let fail = |what: &str| {
        crate::serial_println!("[trees]   FAIL: {what}");
        KernelError::InternalError
    };
    let a = register(Path::new("/.trees-test/a"))?;
    let b = register(Path::new("/.trees-test/b"))?;
    if a == 0 || b == a || register(Path::new("/.trees-test/a"))? != a {
        return Err(fail("a directory registered again did not keep its id"));
    }
    for refused in ["relative/dir", "/a/./b", "/a/../b", ""] {
        if register(Path::new(refused)) != Err(KernelError::InvalidArgument) {
            return Err(fail("a directory not named plainly was registered"));
        }
    }
    let rest = |id: u64, path: &str| beneath(id, Path::new(path)).map(|(_, rest)| rest);
    let cases: [(u64, &str, Option<&str>); 8] = [
        (a, "/.trees-test/a/fw/x.bin", Some("fw/x.bin")),
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
    crate::serial_println!(
        "[trees]   a tree's id kept, its directory named plainly, and a path beneath it found component by component: OK"
    );
    Ok(())
}
