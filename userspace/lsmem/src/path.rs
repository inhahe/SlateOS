//! util-linux's `lib/path.c`, as much of it as `lsmem` uses: one directory,
//! optionally under a `--sysroot` prefix, and the reads it makes of the files
//! in it -- each with upstream's edges, because they show.
//!
//! * [`SysPath::read_buffer`] drops the last byte of a value that does not
//!   end in a newline (`8000000` reads as `800000`): upstream overwrites it
//!   with the terminator.
//! * [`SysPath::read_string`] strips one newline, and is cut at a NUL, as
//!   `strdup` cuts it.
//! * [`SysPath::read_s32`] is `fscanf("%d")`: leading white space, a sign,
//!   digits, and a value `strtol` saturates and `int` then truncates.
//!
//! # Paths where upstream has a descriptor
//!
//! `path.c` opens the directory once and reads everything under it with
//! `openat` and `faccessat`. `std` has neither, so this names each file by
//! its whole path. The directory is still opened once and held for the run,
//! as upstream holds its descriptor: opening it asks for the permission to
//! read it that upstream's `open` asks for, so a directory that can be
//! searched but not read is refused the same way; and the descriptor it
//! holds is the one a closed stdout's number goes to, as upstream's is.
//! Two differences remain, and neither can arise on
//! a real `/sys`: a tree replaced while `lsmem` reads it is read anew here
//! where upstream keeps reading the old one, and a `--sysroot` so long that
//! the memory directory's path is within a few dozen bytes of `PATH_MAX`
//! fails here where upstream's shorter relative names do not.

use std::ffi::OsString;
use std::fs::{self, File, ReadDir};
use std::io::{self, BufRead, BufReader, Read};
use std::time::Duration;

/// `PATH_MAX`: the size of `struct path_cxt`'s path buffer.
const PATH_MAX: usize = 4096;

/// `BUFSIZ`: [`SysPath::read_string`] reads at most one byte less.
const BUFSIZ: usize = 8192;

/// `struct path_cxt` for one directory.
pub struct SysPath {
    /// `dir_path`.
    dir: Vec<u8>,
    /// `prefix`: `--sysroot`.
    prefix: Option<Vec<u8>>,
    /// `dir_fd`: the directory, once opened, held open for the rest of the
    /// run as upstream holds it. Nothing is read through it; what matters
    /// is the descriptor it occupies. With stdout closed, upstream's lands on
    /// descriptor 1, and glibc then sizes stdout's buffer by the directory's
    /// `st_blksize` and fails its first write with `EBADF` -- observable, as
    /// `lsmem -a -o STATE,SIZE,BLOCK >&-` is a write error upstream.
    dir_fd: Option<ReadDir>,
}

impl SysPath {
    /// `ul_new_path(dir)`.
    pub fn new(dir: &[u8]) -> Self {
        SysPath {
            dir: dir.to_vec(),
            prefix: None,
            dir_fd: None,
        }
    }

    /// `ul_path_set_prefix(pc, prefix)`.
    pub fn set_prefix(&mut self, prefix: &[u8]) {
        self.prefix = Some(prefix.to_vec());
    }

    /// `get_absdir`: the prefix, a `/`, and the directory without its own
    /// leading `/` -- refused as `ENAMETOOLONG` when that does not fit in
    /// `PATH_MAX` with its terminator.
    ///
    /// # Errors
    ///
    /// `ENAMETOOLONG`.
    pub fn absdir(&self) -> io::Result<Vec<u8>> {
        let Some(prefix) = &self.prefix else {
            return Ok(self.dir.clone());
        };
        let dir = self.dir.strip_prefix(b"/").unwrap_or(&self.dir);
        let path = [prefix.as_slice(), b"/", dir].concat();
        if path.len() >= PATH_MAX {
            return Err(io::Error::from(io::ErrorKind::InvalidFilename));
        }
        Ok(path)
    }

    /// The whole path of `rel`, a name under the directory; a leading `/` is
    /// dropped, as upstream drops it before `openat`.
    fn path_of(&self, rel: &[u8]) -> io::Result<OsString> {
        let rel = rel.strip_prefix(b"/").unwrap_or(rel);
        let mut path = self.absdir()?;
        path.push(b'/');
        path.extend_from_slice(rel);
        Ok(quoting::os_from_bytes(&path))
    }

    /// `ul_path_is_accessible`: the directory has been opened, or
    /// `access(path, F_OK)` finds it.
    ///
    /// # Errors
    ///
    /// `access`'s, or `ENAMETOOLONG` from [`SysPath::absdir`].
    pub fn is_accessible(&self) -> io::Result<()> {
        if self.dir_fd.is_some() {
            return Ok(());
        }
        let path = self.absdir()?;
        fs::metadata(quoting::os_from_bytes(&path)).map(drop)
    }

    /// `ul_path_get_dirfd`: open the directory, once, and keep it open.
    /// Upstream's `open` needs read permission on it; opening it for listing
    /// asks for the same.
    fn open_dir(&mut self) -> io::Result<()> {
        if self.dir_fd.is_none() {
            let path = self.absdir()?;
            self.dir_fd = Some(fs::read_dir(quoting::os_from_bytes(&path))?);
        }
        Ok(())
    }

    /// `ul_path_access(pc, F_OK, rel)`.
    ///
    /// # Errors
    ///
    /// The directory cannot be opened, or `rel` is not there.
    pub fn access(&mut self, rel: &[u8]) -> io::Result<()> {
        self.open_dir()?;
        fs::metadata(self.path_of(rel)?).map(drop)
    }

    /// `ul_path_opendir(pc, rel)`.
    ///
    /// # Errors
    ///
    /// `rel` cannot be opened as a directory.
    pub fn opendir(&mut self, rel: &[u8]) -> io::Result<ReadDir> {
        self.open_dir()?;
        fs::read_dir(self.path_of(rel)?)
    }

    /// `ul_path_read(pc, buf, len, rel)`: at most `len` bytes of the file.
    fn read(&mut self, rel: &[u8], len: usize) -> io::Result<Vec<u8>> {
        self.open_dir()?;
        let mut file = File::open(self.path_of(rel)?)?;
        read_all(&mut file, len)
    }

    /// `ul_path_read_buffer(pc, buf, bufsz, rel)`: the value, as the C
    /// string upstream leaves in `buf`, and the count it returns.
    ///
    /// A trailing newline is removed and the count drops with it; a value
    /// with none loses its last byte instead, and the count stays. A count
    /// of 0 is a value that was empty, or only a newline.
    ///
    /// # Errors
    ///
    /// The file cannot be opened or nothing could be read from it.
    pub fn read_buffer(&mut self, rel: &[u8], bufsz: usize) -> io::Result<(usize, Vec<u8>)> {
        let mut buf = self.read(rel, bufsz.saturating_sub(1))?;
        let rc = buf.len();
        if buf.last() == Some(&b'\n') {
            buf.pop();
            return Ok((rc.saturating_sub(1), c_string(buf)));
        }
        buf.pop();
        Ok((rc, c_string(buf)))
    }

    /// `ul_path_read_string(pc, &str, rel)` as its callers test it -- a
    /// return above 0 and a string: the value without one trailing newline,
    /// cut at a NUL. `None` for a file that cannot be read or holds nothing
    /// else.
    pub fn read_string(&mut self, rel: &[u8]) -> Option<Vec<u8>> {
        let mut buf = self.read(rel, BUFSIZ.saturating_sub(1)).ok()?;
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.is_empty() {
            return None;
        }
        Some(c_string(buf))
    }

    /// `ul_path_read_s32(pc, &x, rel) == 0`: `fscanf(f, "%d", &x)` read a
    /// number. glibc converts the digits with `strtol`, which saturates, and
    /// stores the `long` into the `int`, which truncates.
    pub fn read_s32(&mut self, rel: &[u8]) -> Option<i32> {
        self.open_dir().ok()?;
        let file = File::open(self.path_of(rel).ok()?).ok()?;
        scan_int(&mut BufReader::new(file))
    }
}

/// `read_all(fd, buf, count)` from `include/all-io.h`: read until `count`
/// bytes or the end, retrying `EINTR` and `EAGAIN` up to five times with a
/// quarter-second pause. A failure after some bytes returns those bytes.
fn read_all(file: &mut File, count: usize) -> io::Result<Vec<u8>> {
    let mut buf = vec![0u8; count];
    let mut got = 0usize;
    let mut tries = 0u32;
    while got < count {
        let Some(rest) = buf.get_mut(got..) else {
            break;
        };
        match file.read(rest) {
            Ok(0) => break,
            Ok(n) => {
                got = got.saturating_add(n);
                tries = 0;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) && tries < 5 =>
            {
                tries = tries.saturating_add(1);
                std::thread::sleep(Duration::from_millis(250));
            }
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

/// A buffer read as C reads it: up to its first NUL.
fn c_string(mut buf: Vec<u8>) -> Vec<u8> {
    if let Some(nul) = buf.iter().position(|&b| b == 0) {
        buf.truncate(nul);
    }
    buf
}

/// `fscanf(f, "%d", &x) == 1`, and `x`.
fn scan_int(input: &mut impl BufRead) -> Option<i32> {
    let mut next = || -> Option<u8> {
        let buf = input.fill_buf().ok()?;
        let b = *buf.first()?;
        input.consume(1);
        Some(b)
    };
    let mut peeked = next();
    while peeked.is_some_and(ulstrutils::c_isspace) {
        peeked = next();
    }
    let mut negative = false;
    if let Some(sign @ (b'+' | b'-')) = peeked {
        negative = sign == b'-';
        peeked = next();
    }
    // `strtol` saturates at LONG_MAX / LONG_MIN; the magnitude only has to
    // be followed that far.
    let limit = u128::from(i64::MAX.unsigned_abs()).saturating_add(1);
    let mut magnitude: u128 = 0;
    let mut digits = 0usize;
    while let Some(d) = peeked.filter(u8::is_ascii_digit) {
        magnitude = magnitude
            .saturating_mul(10)
            .saturating_add(u128::from(d.saturating_sub(b'0')))
            .min(limit);
        digits = digits.saturating_add(1);
        peeked = next();
    }
    if digits == 0 {
        return None;
    }
    let long: i64 = if negative {
        i64::try_from(magnitude)
            .map(i64::wrapping_neg)
            .unwrap_or(i64::MIN)
    } else {
        i64::try_from(magnitude).unwrap_or(i64::MAX)
    };
    // The store into `int`: the low 32 bits.
    Some(long as i32)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn scan(s: &[u8]) -> Option<i32> {
        scan_int(&mut &s[..])
    }

    #[test]
    fn a_number_is_read_as_scanf_reads_it() {
        assert_eq!(scan(b"1\n"), Some(1));
        assert_eq!(scan(b"  \n\t 1abc"), Some(1));
        assert_eq!(scan(b"+1"), Some(1));
        assert_eq!(scan(b"-7"), Some(-7));
        assert_eq!(scan(b"01"), Some(1));
        assert_eq!(scan(b"0x1"), Some(0));
        assert_eq!(scan(b""), None);
        assert_eq!(scan(b"  "), None);
        assert_eq!(scan(b"-"), None);
        assert_eq!(scan(b"x1"), None);
    }

    #[test]
    fn a_number_too_big_for_int_is_truncated_as_glibc_stores_it() {
        // 2^32 + 1 fits `long`; its low 32 bits are 1.
        assert_eq!(scan(b"4294967297"), Some(1));
        // Beyond `long`, strtol's LONG_MAX, whose low 32 bits are -1.
        assert_eq!(scan(b"99999999999999999999"), Some(-1));
        // LONG_MIN's low 32 bits are 0.
        assert_eq!(scan(b"-99999999999999999999"), Some(0));
    }

    #[test]
    fn a_c_string_ends_at_its_nul() {
        assert_eq!(c_string(b"online\0junk".to_vec()), b"online");
        assert_eq!(c_string(b"\0".to_vec()), b"");
        assert_eq!(c_string(b"x".to_vec()), b"x");
    }

    #[test]
    fn the_directory_is_joined_under_the_prefix() {
        let mut p = SysPath::new(b"/sys/devices/system/memory");
        assert_eq!(p.absdir().unwrap(), b"/sys/devices/system/memory");
        p.set_prefix(b"/tmp/root");
        assert_eq!(p.absdir().unwrap(), b"/tmp/root/sys/devices/system/memory");
        p.set_prefix(b"");
        assert_eq!(p.absdir().unwrap(), b"/sys/devices/system/memory");
        p.set_prefix(&[b'x'; PATH_MAX]);
        assert_eq!(
            p.absdir().unwrap_err().kind(),
            io::ErrorKind::InvalidFilename
        );
    }

    #[test]
    fn values_are_read_with_upstreams_edges() {
        let dir = scratchdir::ScratchDir::new("lsmem_path_values");
        fs::create_dir_all(dir.path("sys/devices/system/memory")).unwrap();
        let put = |name: &str, text: &str| {
            fs::write(dir.path(&format!("sys/devices/system/memory/{name}")), text).unwrap();
        };
        put("nl", "8000000\n");
        put("bare", "8000000");
        put("empty", "");
        put("newline", "\n");
        put("state", "online\n");
        put("with_nul", "online\0\n");
        put("removable", " 1\n");

        let mut p = SysPath::new(b"/sys/devices/system/memory");
        p.set_prefix(&quoting::os_bytes(dir.dir().as_os_str()));
        p.is_accessible().unwrap();
        assert_eq!(p.read_buffer(b"nl", 128).unwrap(), (7, b"8000000".to_vec()));
        assert_eq!(
            p.read_buffer(b"bare", 128).unwrap(),
            (7, b"800000".to_vec())
        );
        assert_eq!(p.read_buffer(b"empty", 128).unwrap(), (0, Vec::new()));
        assert_eq!(p.read_buffer(b"newline", 128).unwrap(), (0, Vec::new()));
        assert!(p.read_buffer(b"missing", 128).is_err());
        assert_eq!(p.read_string(b"state"), Some(b"online".to_vec()));
        assert_eq!(p.read_string(b"with_nul"), Some(b"online".to_vec()));
        assert_eq!(p.read_string(b"empty"), None);
        assert_eq!(p.read_string(b"newline"), None);
        assert_eq!(p.read_s32(b"removable"), Some(1));
        assert_eq!(p.read_s32(b"missing"), None);
        assert!(p.access(b"state").is_ok());
        assert!(p.access(b"/state").is_ok());
        assert!(p.access(b"missing").is_err());
    }
}
