//! glibc's `nftw (path, fn, nopenfd, FTW_PHYS)` (`io/ftw.c`), as `hardlink`
//! walks the trees it is given: a directory reported before what is in it,
//! its entries in the order `readdir` returns them, symbolic links never
//! followed, and a directory already walked (another bind mount of it) not
//! walked again.
//!
//! glibc keeps at most `nopenfd` directories open and reads the rest of one
//! into memory when it must close it; this reads each directory whole before
//! walking into its entries, which is the same order and never runs out of
//! descriptors.

use std::ffi::{CString, OsStr};
use std::fs::Metadata;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

/// The `typeflag` the callback is handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `FTW_F`: not a directory or a symbolic link.
    File,
    /// `FTW_D`: a directory, before its entries.
    Dir,
    /// `FTW_DNR`: a directory that could not be read.
    DirNotRead,
    /// `FTW_NS`: something `lstat` failed on, and the `errno` it failed
    /// with (`EACCES` or `ENOENT`).
    NoStat(i32),
    /// `FTW_SL`: a symbolic link (never followed).
    Symlink,
}

/// What the callback is handed besides the path: `struct FTW`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ftw {
    /// `base`: where the last component starts in the path.
    pub base: usize,
    /// `level`: how deep below the starting path.
    pub level: usize,
}

/// The callback: the path, its status (none for [`Kind::NoStat`]), its kind
/// and place. A [`Kind::DirNotRead`] directory failed to open with
/// `EACCES`. Non-zero stops the walk, and is what `nftw` returns.
pub type Callback<'a> = dyn FnMut(&[u8], Option<&Metadata>, Kind, Ftw) -> i32 + 'a;

/// `lstat (path)`: its status, or the `errno`.
fn lstat(path: &[u8]) -> Result<Metadata, i32> {
    std::fs::symlink_metadata(OsStr::from_bytes(path))
        .map_err(|e| e.raw_os_error().unwrap_or(libcall::EINVAL))
}

/// `EACCES` and `ENOENT`, which a failed `lstat` of an entry reports as
/// `FTW_NS` rather than ending the walk.
const EACCES: i32 = 13;

/// A directory's entries, `.` and `..` left out, in `readdir`'s order --
/// or the `errno` of the failed open.
fn read_dir(path: &[u8]) -> Result<Vec<Vec<u8>>, i32> {
    use libcall::file::{AT_FDCWD, Directory, O_CLOEXEC, O_DIRECTORY, O_NONBLOCK, O_RDONLY};
    let c = CString::new(path).map_err(|_| libcall::ENOENT)?;
    // What `opendir` opens with.
    let fd = libcall::file::openat(
        AT_FDCWD,
        &c,
        O_RDONLY | O_NONBLOCK | O_DIRECTORY | O_CLOEXEC,
        0,
    )?;
    let mut dir = Directory::from_fd(fd).inspect_err(|_| {
        // Not made into a stream: closing it is all there is to do.
        let _ = libcall::fd::close(fd);
    })?;
    let mut names = Vec::new();
    let mut buf = [0u8; 256];
    // A read error ends the directory, as `readdir`'s NULL does.
    while let Some(Ok(name)) = dir.next_name(&mut buf) {
        if name != b"." && name != b".." {
            names.push(name.to_vec());
        }
    }
    // The stream was only read: closing it has nothing to report.
    let _ = dir.close();
    Ok(names)
}

/// The walk's state: the directories already walked.
struct Walker<'a, 'b> {
    seen: Vec<(u64, u64)>,
    func: &'a mut Callback<'b>,
}

impl Walker<'_, '_> {
    /// `ftw_dir`: a directory, then everything in it. Non-zero ends the
    /// walk; -1 is an error, with its `errno`.
    fn dir(&mut self, path: &[u8], st: &Metadata, ftw: Ftw) -> Result<i32, i32> {
        let names = match read_dir(path) {
            Ok(n) => n,
            Err(e) if e == EACCES => {
                // "We cannot read the directory. Signal this with a special
                // flag."
                return Ok((self.func)(path, Some(st), Kind::DirNotRead, ftw));
            }
            Err(e) => return Err(e),
        };
        let r = (self.func)(path, Some(st), Kind::Dir, ftw);
        if r != 0 {
            return Ok(r);
        }
        let level = ftw.level.saturating_add(1);
        for name in names {
            let mut child = path.to_vec();
            if child.last() != Some(&b'/') {
                child.push(b'/');
            }
            let base = child.len();
            child.extend_from_slice(&name);
            let r = self.entry(&child, Ftw { base, level })?;
            if r != 0 {
                return Ok(r);
            }
        }
        Ok(0)
    }

    /// `process_entry`: one entry of a directory.
    fn entry(&mut self, path: &[u8], ftw: Ftw) -> Result<i32, i32> {
        let st = match lstat(path) {
            Ok(st) => st,
            Err(e) if e == EACCES || e == libcall::ENOENT => {
                return Ok((self.func)(path, None, Kind::NoStat(e), ftw));
            }
            Err(e) => return Err(e),
        };
        let ft = st.file_type();
        if ft.is_dir() {
            let id = (st.dev(), st.ino());
            if self.seen.contains(&id) {
                return Ok(0);
            }
            self.seen.push(id);
            self.dir(path, &st, ftw)
        } else if ft.is_symlink() {
            Ok((self.func)(path, Some(&st), Kind::Symlink, ftw))
        } else {
            Ok((self.func)(path, Some(&st), Kind::File, ftw))
        }
    }
}

/// `nftw (path, func, nopenfd, FTW_PHYS)`: 0 when the whole tree was
/// walked, the callback's non-zero answer where it stopped, or -1 with the
/// `errno` that ended it -- `path` itself not there, or a directory that
/// could not be opened for any reason but permission.
pub fn nftw(path: &[u8], func: &mut Callback<'_>) -> Result<i32, i32> {
    let st = lstat(path)?;
    let base = path
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    let ftw = Ftw { base, level: 0 };
    let mut w = Walker {
        seen: Vec::new(),
        func,
    };
    if st.file_type().is_dir() {
        w.seen.push((st.dev(), st.ino()));
        w.dir(path, &st, ftw)
    } else if st.file_type().is_symlink() {
        Ok((w.func)(path, Some(&st), Kind::Symlink, ftw))
    } else {
        Ok((w.func)(path, Some(&st), Kind::File, ftw))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "a test indexes what it just built"
)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_is_walked_directory_first_and_links_are_not_followed() {
        let root = std::env::temp_dir().join(format!("hardlink-walk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("d/e")).unwrap();
        std::fs::write(root.join("d/f"), b"x").unwrap();
        std::fs::write(root.join("d/e/g"), b"y").unwrap();
        std::os::unix::fs::symlink("d", root.join("l")).unwrap();
        let rootb = root.as_os_str().as_bytes().to_vec();
        let mut seen = Vec::new();
        let rc = nftw(&rootb, &mut |p: &[u8],
                                    _st: Option<&Metadata>,
                                    k: Kind,
                                    ftw: Ftw| {
            let rel = p.get(rootb.len()..).unwrap_or_default().to_vec();
            seen.push((
                String::from_utf8(rel).unwrap(),
                k,
                ftw.level,
                p.len() - ftw.base,
            ));
            0
        })
        .unwrap();
        assert_eq!(rc, 0);
        assert_eq!(
            seen[0],
            (
                String::new(),
                Kind::Dir,
                0,
                rootb.len() - rootb.iter().rposition(|&b| b == b'/').unwrap() - 1
            )
        );
        // Each directory before its entries; the link reported, not walked.
        let pos = |s: &str| seen.iter().position(|e| e.0 == s).unwrap();
        assert!(pos("/d") < pos("/d/f") && pos("/d") < pos("/d/e") && pos("/d/e") < pos("/d/e/g"));
        assert_eq!(seen[pos("/l")].1, Kind::Symlink);
        assert_eq!(seen[pos("/d/e/g")], ("/d/e/g".to_owned(), Kind::File, 3, 1));
        assert_eq!(seen.len(), 6);
        // A non-zero answer stops the walk and is returned.
        let mut n = 0;
        assert_eq!(
            nftw(&rootb, &mut |_p: &[u8],
                               _st: Option<&Metadata>,
                               _k: Kind,
                               _f: Ftw| {
                n += 1;
                if n == 2 { 7 } else { 0 }
            }),
            Ok(7)
        );
        assert_eq!(
            nftw(
                b"/nonexistent/x",
                &mut |_p: &[u8], _st: Option<&Metadata>, _k: Kind, _f: Ftw| 0
            ),
            Err(libcall::ENOENT)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
