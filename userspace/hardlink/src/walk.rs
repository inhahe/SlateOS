//! glibc 2.39's `nftw (path, fn, 20, FTW_PHYS)` (`io/ftw.c`), as `hardlink`
//! walks the trees it is given: a directory reported before what is in it,
//! its entries in the order `readdir` returns them, symbolic links never
//! followed -- and, with `FTW_PHYS`, no record of the directories already
//! walked: a directory reached twice (a bind mount of another) is walked
//! twice.
//!
//! As glibc does, an entry is `lstat`ed and a directory opened relative to
//! its parent's descriptor while that is open, so no path is limited by
//! `PATH_MAX`; and at most 20 directories are open at once (`nopenfd`), in
//! a ring: opening a 21st reads the rest of the oldest one's entries into
//! memory and closes it, and those entries are then `lstat`ed by their whole
//! path.

use libcall::file::{
    AT_FDCWD, AT_SYMLINK_NOFOLLOW, Directory, O_CLOEXEC, O_DIRECTORY, O_NONBLOCK, O_RDONLY,
    S_IFDIR, S_IFLNK, S_IFMT, Stat,
};
use std::ffi::CString;

/// The `typeflag` the callback is handed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `FTW_F`: not a directory or a symbolic link.
    File,
    /// `FTW_D`: a directory, before its entries.
    Dir,
    /// `FTW_DNR`: a directory that could not be read (`EACCES`).
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
/// and place. Non-zero stops the walk, and is what `nftw` returns.
pub type Callback<'a> = dyn FnMut(&[u8], Option<&Stat>, Kind, Ftw) -> i32 + 'a;

/// `nopenfd`: how many directories `hardlink` lets the walk keep open.
const NOPENFD: usize = 20;

/// `ENOENT` and `EACCES`, which a failed `lstat` of an entry reports as
/// `FTW_NS` rather than ending the walk.
const ENOENT: i32 = libcall::ENOENT;
const EACCES: i32 = libcall::EACCES;

/// `struct dir_data`: a directory being walked -- open, or with the rest of
/// its entries read and its stream closed to make room.
#[derive(Default)]
struct DirData {
    /// `stream`, and its descriptor (`streamfd`).
    stream: Option<(Directory, i32)>,
    /// `content`: the entries left when the stream was closed.
    content: Option<Vec<Vec<u8>>>,
}

/// `struct ftw_data`: the walk's state.
struct Walker<'a, 'b> {
    /// `dirbuf`: the current path.
    dirbuf: Vec<u8>,
    ftw: Ftw,
    /// The directories being walked, outermost first: the recursion's
    /// `struct dir_data`s.
    dirs: Vec<DirData>,
    /// `dirstreams` and `actdir`: the ring of open directories, by their
    /// place in `dirs`, and where the next one goes.
    dirstreams: [Option<usize>; NOPENFD],
    actdir: usize,
    func: &'a mut Callback<'b>,
}

/// A path's bytes as a C string, cut at a NUL as C would read it.
fn c_string(bytes: &[u8]) -> CString {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    // No NUL is left, so the empty fallback is never taken.
    CString::new(bytes.get(..end).unwrap_or_default()).unwrap_or_default()
}

/// The kind an `lstat` says a file is.
fn kind_of(st: &Stat) -> Kind {
    match st.st_mode & S_IFMT {
        S_IFDIR => Kind::Dir,
        S_IFLNK => Kind::Symlink,
        _ => Kind::File,
    }
}

impl Walker<'_, '_> {
    /// `open_dir_stream (dfdp, data, dirp)`: directory `depth` opened --
    /// relative to its parent's descriptor if that is open, else by its
    /// whole path -- making room in the ring first if it is full.
    fn open_dir_stream(&mut self, parent: Option<usize>, depth: usize) -> Result<(), i32> {
        if let Some(old) = self.dirstreams.get(self.actdir).copied().flatten() {
            // "Oh, oh. We must close this stream. Get all remaining entries
            // and store them as a list in the `content' member."
            if let Some(d) = self.dirs.get_mut(old)
                && let Some((mut stream, _)) = d.stream.take()
            {
                let mut names = Vec::new();
                let mut buf = [0u8; 256];
                while let Some(Ok(name)) = stream.next_name(&mut buf) {
                    names.push(name.to_vec());
                }
                // Read to its end: closing it has nothing to report.
                let _ = stream.close();
                d.content = Some(names);
            }
            if let Some(slot) = self.dirstreams.get_mut(self.actdir) {
                *slot = None;
            }
        }
        let parent_fd = parent
            .and_then(|p| self.dirs.get(p))
            .and_then(|d| d.stream.as_ref())
            .map(|(_, fd)| *fd);
        let fd = match parent_fd {
            Some(dfd) => {
                let name = c_string(self.dirbuf.get(self.ftw.base..).unwrap_or_default());
                libcall::file::openat(dfd, &name, O_RDONLY | O_DIRECTORY | O_NONBLOCK, 0)?
            }
            // `opendir (dirbuf)`.
            None => libcall::file::openat(
                AT_FDCWD,
                &c_string(&self.dirbuf),
                O_RDONLY | O_NONBLOCK | O_DIRECTORY | O_CLOEXEC,
                0,
            )?,
        };
        let stream = Directory::from_fd(fd).inspect_err(|_| {
            // Not made into a stream: closing it is all there is to do.
            let _ = libcall::fd::close(fd);
        })?;
        if let Some(d) = self.dirs.get_mut(depth) {
            d.stream = Some((stream, fd));
        }
        if let Some(slot) = self.dirstreams.get_mut(self.actdir) {
            *slot = Some(depth);
        }
        self.actdir = self.actdir.saturating_add(1) % NOPENFD;
        Ok(())
    }

    /// `readdir (dir.stream)` while directory `depth`'s stream is open: the
    /// next name, or `None` at its end, on an error (`readdir`'s NULL for
    /// both) or once a deeper open has closed it, its rest kept.
    fn next_name(&mut self, depth: usize) -> Option<Vec<u8>> {
        let (stream, _) = self.dirs.get_mut(depth)?.stream.as_mut()?;
        let mut buf = [0u8; 256];
        stream.next_name(&mut buf)?.ok().map(<[u8]>::to_vec)
    }

    /// Directory `depth`'s stream closed, and its place in the ring freed:
    /// the ring is last in, first out.
    fn close_stream(&mut self, depth: usize) {
        if let Some(d) = self.dirs.get_mut(depth)
            && let Some((stream, _)) = d.stream.take()
        {
            // A stream only read: closing it has nothing to report.
            let _ = stream.close();
        }
        self.actdir = if self.actdir == 0 {
            NOPENFD.saturating_sub(1)
        } else {
            self.actdir.saturating_sub(1)
        };
        if let Some(slot) = self.dirstreams.get_mut(self.actdir) {
            *slot = None;
        }
    }

    /// `ftw_dir (data, st, old_dir)`: the directory `dirbuf` names, then
    /// everything in it. Non-zero stops the walk; `Err` is -1, with its
    /// `errno`.
    fn dir(&mut self, st: &Stat, parent: Option<usize>) -> Result<i32, i32> {
        let depth = self.dirs.len();
        self.dirs.push(DirData::default());
        if let Err(e) = self.open_dir_stream(parent, depth) {
            self.dirs.pop();
            if e == EACCES {
                // "We cannot read the directory. Signal this with a special
                // flag."
                return Ok((self.func)(
                    &self.dirbuf,
                    Some(st),
                    Kind::DirNotRead,
                    self.ftw,
                ));
            }
            return Err(e);
        }
        let r = (self.func)(&self.dirbuf, Some(st), Kind::Dir, self.ftw);
        if r != 0 {
            self.close_stream(depth);
            self.dirs.pop();
            return Ok(r);
        }

        // "Next, update the `struct FTW' information."
        let previous_base = self.ftw.base;
        self.ftw.level = self.ftw.level.saturating_add(1);
        if self.dirbuf.last() != Some(&b'/') {
            self.dirbuf.push(b'/');
        }
        self.ftw.base = self.dirbuf.len();

        let mut result = Ok(0);
        while let Some(name) = self.next_name(depth) {
            result = self.entry(depth, &name);
            if !matches!(result, Ok(0)) {
                break;
            }
        }
        if self.dirs.get(depth).is_some_and(|d| d.stream.is_some()) {
            self.close_stream(depth);
        } else {
            let content = self
                .dirs
                .get_mut(depth)
                .and_then(|d| d.content.take())
                .unwrap_or_default();
            for name in content {
                if !matches!(result, Ok(0)) {
                    break;
                }
                result = self.entry(depth, &name);
            }
        }

        // "Prepare the return, revert the `struct FTW' information."
        self.dirbuf.truncate(self.ftw.base.saturating_sub(1));
        self.ftw.level = self.ftw.level.saturating_sub(1);
        self.ftw.base = previous_base;
        self.dirs.pop();
        result
    }

    /// `process_entry (data, dir, name, namlen, d_type)`: one entry of
    /// directory `depth` -- `lstat`ed relative to it while it is open, else
    /// by its whole path.
    fn entry(&mut self, depth: usize, name: &[u8]) -> Result<i32, i32> {
        if name == b"." || name == b".." {
            return Ok(0);
        }
        self.dirbuf.truncate(self.ftw.base);
        self.dirbuf.extend_from_slice(name);
        let dir_fd = self
            .dirs
            .get(depth)
            .and_then(|d| d.stream.as_ref())
            .map(|(_, fd)| *fd);
        let st = match dir_fd {
            Some(fd) => libcall::file::fstatat(fd, &c_string(name), AT_SYMLINK_NOFOLLOW),
            None => libcall::file::fstatat(AT_FDCWD, &c_string(&self.dirbuf), AT_SYMLINK_NOFOLLOW),
        };
        let st = match st {
            Ok(st) => st,
            Err(e) if e == EACCES || e == ENOENT => {
                return Ok((self.func)(&self.dirbuf, None, Kind::NoStat(e), self.ftw));
            }
            Err(e) => return Err(e),
        };
        match kind_of(&st) {
            Kind::Dir => self.dir(&st, Some(depth)),
            kind => Ok((self.func)(&self.dirbuf, Some(&st), kind, self.ftw)),
        }
    }
}

/// `nftw (path, func, 20, FTW_PHYS)`: 0 when the whole tree was walked, the
/// callback's non-zero answer where it stopped, or -1 with the `errno` that
/// ended it -- `path` itself not there, or an entry or a directory that
/// failed for any reason but permission (`ENAMETOOLONG`, `EIO`, ...).
pub fn nftw(path: &[u8], func: &mut Callback<'_>) -> Result<i32, i32> {
    // "Strip trailing slashes", but for a first one; then find the base.
    let mut end = path.len();
    while end > 1 && path.get(end.saturating_sub(1)) == Some(&b'/') {
        end = end.saturating_sub(1);
    }
    let dirbuf = path.get(..end).unwrap_or_default().to_vec();
    let base = dirbuf
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    let mut w = Walker {
        dirbuf,
        ftw: Ftw { base, level: 0 },
        dirs: Vec::new(),
        dirstreams: [None; NOPENFD],
        actdir: 0,
        func,
    };
    let st = libcall::file::fstatat(AT_FDCWD, &c_string(&w.dirbuf), AT_SYMLINK_NOFOLLOW)?;
    match kind_of(&st) {
        Kind::Dir => w.dir(&st, None),
        kind => Ok((w.func)(&w.dirbuf, Some(&st), kind, w.ftw)),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test indexes and measures what it just built"
)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    /// What a walk reported: each path below the root, its kind, level and
    /// last component's length.
    type Seen = Vec<(Vec<u8>, Kind, usize, usize)>;

    /// What a walk of `root` reported: each path below it, its kind, level
    /// and last component's length.
    fn walk(root: &[u8]) -> (Result<i32, i32>, Seen) {
        let mut seen = Vec::new();
        let rc = nftw(root, &mut |p: &[u8],
                                  _st: Option<&Stat>,
                                  k: Kind,
                                  ftw: Ftw| {
            let rel = p.get(root.len()..).unwrap_or_default().to_vec();
            seen.push((rel, k, ftw.level, p.len() - ftw.base));
            0
        });
        (rc, seen)
    }

    /// `sh -c SCRIPT` in `dir`: the tests' way to build a tree a level at
    /// a time, relative, as no whole path past `PATH_MAX` can be named.
    fn sh(dir: &std::path::Path, script: &str) {
        let ok = std::process::Command::new("sh")
            .arg("-c")
            .arg(script)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "{script}");
    }

    fn scratch(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!("hardlink-{tag}-{}", std::process::id()));
        let _ = std::process::Command::new("rm")
            .arg("-rf")
            .arg(&root)
            .status();
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_tree_is_walked_directory_first_and_links_are_not_followed() {
        let root = scratch("walk");
        sh(
            &root,
            "mkdir -p d/e && echo x > d/f && echo y > d/e/g && ln -s d l",
        );
        let rootb = root.as_os_str().as_bytes().to_vec();
        let (rc, seen) = walk(&rootb);
        assert_eq!(rc, Ok(0));
        assert_eq!((seen[0].0.as_slice(), seen[0].1), (&b""[..], Kind::Dir));
        let pos = |s: &[u8]| seen.iter().position(|e| e.0 == s).unwrap();
        assert!(pos(b"/d") < pos(b"/d/f") && pos(b"/d") < pos(b"/d/e"));
        assert!(pos(b"/d/e") < pos(b"/d/e/g"));
        assert_eq!(seen[pos(b"/l")].1, Kind::Symlink);
        assert_eq!(seen[pos(b"/d/e/g")], (b"/d/e/g".to_vec(), Kind::File, 3, 1));
        assert_eq!(seen.len(), 6);
        // Trailing slashes are the same root.
        let mut slashed = rootb.clone();
        slashed.extend_from_slice(b"//");
        assert_eq!(walk(&slashed).1.len(), 6);
        // A non-zero answer stops the walk and is returned.
        let mut n = 0;
        let stopped =
            nftw(&rootb, &mut |_p: &[u8],
                               _st: Option<&Stat>,
                               _k: Kind,
                               _f: Ftw| {
                n += 1;
                if n == 2 { 7 } else { 0 }
            });
        assert_eq!(stopped, Ok(7));
        assert_eq!(walk(b"/nonexistent/x").0, Err(libcall::ENOENT));
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// Deeper than the 20 directories the walk keeps open: the ring closes
    /// the outer ones and walks their rest from memory, nothing lost or
    /// reported twice.
    #[test]
    fn a_tree_deeper_than_twenty_directories_is_walked_whole() {
        let root = scratch("deep");
        sh(
            &root,
            "for i in $(seq 0 29); do echo x > f$i; mkdir d$i; cd d$i; done",
        );
        let (rc, seen) = walk(root.as_os_str().as_bytes());
        assert_eq!(rc, Ok(0));
        // The root, 30 directories and 30 files, each once.
        assert_eq!(seen.len(), 61);
        let mut paths: Vec<_> = seen.iter().map(|e| e.0.clone()).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 61);
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A path longer than `PATH_MAX`, reached relative to the directories
    /// above it, as glibc reaches it.
    #[test]
    fn a_path_longer_than_path_max_is_reached() {
        let root = scratch("long");
        let name = "n".repeat(250);
        // The shell's own `cd` stops short of PATH_MAX, but `mkdir` goes a
        // level further: the last directory's path is 4267 bytes.
        sh(
            &root,
            &format!("for i in $(seq 1 16); do mkdir {name} && cd {name}; done; mkdir {name}"),
        );
        let (rc, seen) = walk(root.as_os_str().as_bytes());
        assert_eq!(rc, Ok(0));
        // The root and 17 directories.
        assert_eq!(seen.len(), 18);
        let deepest = seen.iter().map(|e| e.0.len()).max().unwrap_or(0);
        assert!(deepest > 4096, "{deepest}");
        // `rm -r` removes it relative as well.
        let _ = std::process::Command::new("rm")
            .arg("-rf")
            .arg(&root)
            .status();
    }
}
