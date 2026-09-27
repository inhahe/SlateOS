//! `lib/path.c`: a directory -- `/sys/devices/system/cpu`, `/proc`, `/` --
//! under an optional prefix (`--sysroot`), and the ways upstream reads the
//! files in it.
//!
//! The readers keep upstream's quirks because each is visible in what
//! `lscpu` prints for an odd tree:
//!
//! * [`PathCxt::read_buffer`] drops a trailing newline -- or, when there is
//!   none, the file's *last byte*: a `type` file holding `Data` without a
//!   newline reads as `Dat`.
//! * [`PathCxt::read_string`] reads at most 8191 bytes, stops at a NUL, and
//!   reports the bytes read (not the string's length), so a file holding
//!   only a NUL is a success with an empty string.
//! * The number readers are `fscanf("%d")` and friends: leading white space
//!   and a sign are accepted, trailing junk ignored, and a number too large
//!   for the type is clamped and cut, not refused.
//! * A CPU list or mask is only the file's first line, `7 * maxcpus - 1`
//!   bytes of it at most.
//!
//! Upstream opens the directory once and works relative to it; a
//! directory that will not open fails every read with the error that open
//! gave. Here the path is joined instead, after checking that the directory
//! opens -- the same answers, without holding a descriptor.

use crate::cpuset::{self, CpuSet};
use crate::cstr::{self, Scanned, StreamSource};
use std::cell::Cell;
use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::PathBuf;

/// `PATH_MAX`: the longest path, NUL included, upstream builds.
const PATH_MAX: usize = 4096;

/// `BUFSIZ`.
pub const BUFSIZ: usize = 8192;

/// `ENAMETOOLONG`, on Linux and in the SlateOS C library.
const ENAMETOOLONG: i32 = 36;
/// `EIO`.
pub const EIO: i32 = 5;
/// `EINVAL`.
pub const EINVAL: i32 = 22;

/// `struct path_cxt`.
#[derive(Debug)]
pub struct PathCxt {
    /// `dir_path`.
    dir: Vec<u8>,
    /// `prefix`: `--sysroot`'s directory.
    prefix: Option<Vec<u8>>,
    /// Whether the directory has opened: upstream keeps the descriptor once
    /// it has one, and retries every time it has not.
    dir_open: Cell<bool>,
}

/// A path from bytes.
fn path_of(bytes: &[u8]) -> PathBuf {
    PathBuf::from(quoting::os_from_bytes(bytes))
}

impl PathCxt {
    /// `ul_new_path(dir)`.
    #[must_use]
    pub fn new(dir: &[u8]) -> Self {
        PathCxt {
            dir: dir.to_vec(),
            prefix: None,
            dir_open: Cell::new(false),
        }
    }

    /// `ul_path_set_prefix(pc, prefix)`.
    pub fn set_prefix(&mut self, prefix: Option<&[u8]>) {
        self.prefix = prefix.map(<[u8]>::to_vec);
        self.dir_open.set(false);
    }

    /// `get_absdir(pc)`: `PREFIX/DIR`, the directory's own leading slash
    /// dropped -- refused, as `ENAMETOOLONG`, when it would not fit in
    /// `PATH_MAX`.
    fn absdir(&self) -> io::Result<Vec<u8>> {
        let Some(prefix) = &self.prefix else {
            return Ok(self.dir.clone());
        };
        let dir = self.dir.strip_prefix(b"/").unwrap_or(&self.dir);
        let mut path = prefix.clone();
        path.push(b'/');
        path.extend_from_slice(dir);
        if path.len() >= PATH_MAX {
            return Err(io::Error::from_raw_os_error(ENAMETOOLONG));
        }
        Ok(path)
    }

    /// `ul_path_get_dirfd(pc)`: the directory, which must open.
    fn dirfd(&self) -> io::Result<Vec<u8>> {
        let dir = self.absdir()?;
        if !self.dir_open.get() {
            open_dir(&path_of(&dir))?;
            self.dir_open.set(true);
        }
        Ok(dir)
    }

    /// Where `rel` is: the directory joined with `rel`, one leading slash of
    /// which is dropped.
    fn resolve(&self, rel: &[u8]) -> io::Result<PathBuf> {
        let mut path = self.dirfd()?;
        path.push(b'/');
        path.extend_from_slice(rel.strip_prefix(b"/").unwrap_or(rel));
        Ok(path_of(&path))
    }

    /// `ul_path_access(pc, F_OK, rel) == 0`.
    #[must_use]
    pub fn exists(&self, rel: &[u8]) -> bool {
        self.resolve(rel).is_ok_and(|p| fs::metadata(p).is_ok())
    }

    /// `ul_path_open(pc, O_RDONLY, rel)`.
    ///
    /// # Errors
    ///
    /// The directory's failure to open, or the file's.
    pub fn open(&self, rel: &[u8]) -> io::Result<File> {
        File::open(self.resolve(rel)?)
    }

    /// `ul_path_opendir(pc, rel)`, or with `None` the directory itself: its
    /// entries' names and whether each is a directory, as `readdir` gives
    /// them -- `.` and `..` left out.
    ///
    /// # Errors
    ///
    /// The directory will not open or is not one.
    pub fn read_dir(&self, rel: Option<&[u8]>) -> io::Result<Vec<(Vec<u8>, bool)>> {
        let path = match rel {
            Some(rel) => self.resolve(rel)?,
            None => path_of(&self.dirfd()?),
        };
        let mut entries = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
            entries.push((quoting::os_bytes(&entry.file_name()).into_owned(), is_dir));
        }
        Ok(entries)
    }

    /// `ul_path_read(pc, buf, len, rel)`: up to `len` bytes, as `read_all`
    /// gathers them -- a read that fails after some bytes arrived ends the
    /// file there.
    ///
    /// # Errors
    ///
    /// The file will not open, or its first read fails.
    pub fn read(&self, rel: &[u8], len: usize) -> io::Result<Vec<u8>> {
        let mut file = self.open(rel)?;
        read_all(&mut file, len)
    }

    /// `ul_path_read_string(pc, &str, rel)`: the bytes read (at most 8191,
    /// one trailing newline not counted) and the string -- `None` when
    /// nothing was read, and cut at a NUL.
    ///
    /// # Errors
    ///
    /// As [`PathCxt::read`].
    pub fn read_string(&self, rel: &[u8]) -> io::Result<(usize, Option<Vec<u8>>)> {
        let mut buf = self.read(rel, BUFSIZ.saturating_sub(1))?;
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.is_empty() {
            return Ok((0, None));
        }
        let n = buf.len();
        Ok((n, Some(cstr::c_str(&buf).to_vec())))
    }

    /// `ul_path_read_buffer(pc, buf, bufsz, rel)`: the bytes read (at most
    /// `bufsz - 1`) and the C string left in `buf` -- a trailing newline
    /// removed, or without one the last byte.
    ///
    /// # Errors
    ///
    /// As [`PathCxt::read`].
    pub fn read_buffer(&self, rel: &[u8], bufsz: usize) -> io::Result<(usize, Vec<u8>)> {
        let mut buf = self.read(rel, bufsz.saturating_sub(1))?;
        let n = buf.len();
        if buf.last() == Some(&b'\n') {
            buf.pop();
            return Ok((n.saturating_sub(1), cstr::c_str(&buf).to_vec()));
        }
        buf.pop();
        Ok((n, cstr::c_str(&buf).to_vec()))
    }

    /// `ul_path_scanf(pc, rel, fmt, ...)`: `None` when the file will not
    /// open, else what `fscanf` stored (`Some(vec![])` for its `EOF` too --
    /// every caller only counts).
    #[must_use]
    pub fn scanf(&self, rel: &[u8], fmt: &[u8]) -> Option<Vec<Scanned>> {
        let file = self.open(rel).ok()?;
        let mut src = StreamSource::new(BufReader::new(file));
        Some(cstr::scanf(&mut src, fmt).unwrap_or_default())
    }

    /// One conversion's value, if exactly one was stored.
    fn scan_one(&self, rel: &[u8], fmt: &[u8]) -> Option<Scanned> {
        let mut values = self.scanf(rel, fmt)?;
        if values.len() == 1 {
            values.pop()
        } else {
            None
        }
    }

    /// `ul_path_read_s32(pc, &x, rel) == 0`: `fscanf("%d")`.
    #[must_use]
    pub fn read_s32(&self, rel: &[u8]) -> Option<i32> {
        self.scan_one(rel, b"%d").map(|v| v.as_int())
    }

    /// `ul_path_read_u32(pc, &x, rel) == 0`: `fscanf("%u")`.
    #[must_use]
    pub fn read_u32(&self, rel: &[u8]) -> Option<u32> {
        self.scan_one(rel, b"%u").map(|v| v.as_uint())
    }

    /// `ul_path_cpuparse`: the first line, `7 * maxcpus - 1` bytes at most,
    /// its newline dropped, as a list (`islist`) or a mask, in a set for
    /// `maxcpus` CPUs.
    ///
    /// # Errors
    ///
    /// The file will not open (its error); it is empty (`EIO`); it does not
    /// parse (`EINVAL`). Upstream then leaves the caller a pointer to the
    /// set it has just freed; here there is no set.
    pub fn read_cpuset(&self, rel: &[u8], maxcpus: usize, islist: bool) -> io::Result<CpuSet> {
        let file = self.open(rel)?;
        let mut reader = BufReader::new(file);
        let Some(line) = cstr::fgets(&mut reader, maxcpus.saturating_mul(7)) else {
            return Err(io::Error::from_raw_os_error(EIO));
        };
        let mut buf = cstr::c_str(&line).to_vec();
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        let parsed = if islist {
            cpuset::cpulist_parse(&buf, maxcpus)
        } else {
            cpuset::cpumask_parse(&buf, maxcpus)
        };
        parsed.map_err(|_| io::Error::from_raw_os_error(EINVAL))
    }
}

/// `open(dir, O_RDONLY)` as `ul_path_get_dirfd` does it, which on Linux
/// succeeds for a directory and for a file alike. The Windows host the unit
/// tests also run on cannot open a directory as a file, so there it is
/// asked whether the path exists.
fn open_dir(path: &std::path::Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        File::open(path).map(drop)
    }
    #[cfg(not(unix))]
    {
        fs::metadata(path).map(drop)
    }
}

/// `read_all(fd, buf, count)`: reads until `count` bytes, the end, or an
/// error -- which is only an error if nothing had been read.
///
/// # Errors
///
/// The first read fails.
pub fn read_all(file: &mut impl Read, count: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; count.min(1 << 16)];
    let mut got = 0usize;
    while got < count {
        if got == buf.len() {
            buf.resize(buf.len().saturating_mul(2).min(count), 0);
        }
        let Some(slot) = buf.get_mut(got..) else {
            break;
        };
        match file.read(slot) {
            Ok(0) => break,
            Ok(n) => got = got.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                if got == 0 {
                    return Err(e);
                }
                break;
            }
        }
    }
    buf.truncate(got);
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> (scratchdir::ScratchDir, PathCxt) {
        let dir = scratchdir::ScratchDir::new("lscpu-path");
        let root = quoting::os_bytes(dir.dir().as_os_str()).into_owned();
        fs::create_dir_all(dir.path("sys/devices/system/cpu")).ok();
        let mut pc = PathCxt::new(b"/sys/devices/system/cpu");
        pc.set_prefix(Some(&root));
        (dir, pc)
    }

    fn put(dir: &scratchdir::ScratchDir, rel: &str, data: &[u8]) {
        let path = dir.path(&format!("sys/devices/system/cpu/{rel}"));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::write(path, data).ok();
    }

    #[test]
    fn buffers_lose_their_newline_or_last_byte() {
        let (dir, pc) = tree();
        put(&dir, "a", b"Data\n");
        put(&dir, "b", b"Data");
        put(&dir, "c", b"");
        assert_eq!(pc.read_buffer(b"a", 256).ok(), Some((4, b"Data".to_vec())));
        assert_eq!(pc.read_buffer(b"b", 256).ok(), Some((4, b"Dat".to_vec())));
        assert_eq!(pc.read_buffer(b"c", 256).ok(), Some((0, Vec::new())));
        assert!(pc.read_buffer(b"missing", 256).is_err());
    }

    #[test]
    fn strings_count_bytes_and_stop_at_a_nul() {
        let (dir, pc) = tree();
        put(&dir, "a", b"Mitigation: x\n");
        put(&dir, "b", b"\0abc\n");
        put(&dir, "c", b"\n");
        assert_eq!(
            pc.read_string(b"a").ok(),
            Some((13, Some(b"Mitigation: x".to_vec())))
        );
        assert_eq!(pc.read_string(b"b").ok(), Some((4, Some(Vec::new()))));
        assert_eq!(pc.read_string(b"c").ok(), Some((0, None)));
    }

    #[test]
    fn numbers_are_fscanf() {
        let (dir, pc) = tree();
        put(&dir, "kernel_max", b"8191\n");
        put(&dir, "neg", b"  -3junk");
        put(&dir, "junk", b"x1");
        assert_eq!(pc.read_s32(b"kernel_max"), Some(8191));
        assert_eq!(pc.read_s32(b"neg"), Some(-3));
        assert_eq!(pc.read_s32(b"junk"), None);
        assert_eq!(pc.read_s32(b"missing"), None);
        assert_eq!(pc.read_u32(b"neg"), Some(u32::MAX - 2));
    }

    #[test]
    fn cpusets_are_the_first_line() {
        let (dir, pc) = tree();
        put(&dir, "possible", b"0-3\n4-7\n");
        put(&dir, "empty", b"");
        put(&dir, "bad", b"0-3x\n");
        put(&dir, "mask", b"00000000,0000000f\n");
        let set = pc
            .read_cpuset(b"possible", 64, true)
            .ok()
            .map(|s| s.iter().collect::<Vec<_>>());
        assert_eq!(set, Some(vec![0, 1, 2, 3]));
        let err = pc
            .read_cpuset(b"empty", 64, true)
            .err()
            .and_then(|e| e.raw_os_error());
        assert_eq!(err, Some(EIO));
        let err = pc
            .read_cpuset(b"bad", 64, true)
            .err()
            .and_then(|e| e.raw_os_error());
        assert_eq!(err, Some(EINVAL));
        let set = pc
            .read_cpuset(b"mask", 64, false)
            .ok()
            .map(|s| s.iter().collect::<Vec<_>>());
        assert_eq!(set, Some(vec![0, 1, 2, 3]));
        // One CPU's worth of line: 6 bytes, which this fits exactly...
        put(&dir, "fits", b"10,11\n");
        let set = pc
            .read_cpuset(b"fits", 1, true)
            .ok()
            .map(|s| s.iter().collect::<Vec<_>>());
        assert_eq!(set, Some(vec![10, 11]));
        // ... and this does not: `10,11,` is a list ending in a comma.
        put(&dir, "long", b"10,11,12\n");
        let err = pc
            .read_cpuset(b"long", 1, true)
            .err()
            .and_then(|e| e.raw_os_error());
        assert_eq!(err, Some(EINVAL));
    }

    #[test]
    fn a_missing_root_fails_everything() {
        let mut pc = PathCxt::new(b"/sys/devices/system/cpu");
        pc.set_prefix(Some(b"/nonexistent-lscpu-root"));
        assert!(!pc.exists(b"possible"));
        assert!(pc.read(b"possible", 10).is_err());
        let long = vec![b'x'; PATH_MAX];
        pc.set_prefix(Some(&long));
        let err = pc
            .read(b"possible", 10)
            .err()
            .and_then(|e| e.raw_os_error());
        assert_eq!(err, Some(ENAMETOOLONG));
    }
}
