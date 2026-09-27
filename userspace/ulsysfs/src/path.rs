//! `lib/path.c`: reads relative to a directory, with util-linux's return
//! conventions.

use std::cell::Cell;
use std::fs::File;
use std::io::Read;
use std::rc::Rc;

use crate::{BUFSIZ, EINVAL, ENAMETOOLONG, ENOENT, PATH_MAX, c_str, errno_of, path_of};

/// One entry of a directory, as `xreaddir` returns it: never `.` or `..`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    /// `d_name`.
    pub name: Vec<u8>,
    /// `d_type` as a file type; `None` for `DT_UNKNOWN`.
    pub file_type: Option<std::fs::FileType>,
}

/// The `sysfs_blkdev` dialect: which device the directory is, and the
/// directory a missing file is looked for in instead.
#[derive(Debug, Default)]
pub(crate) struct Blkdev {
    /// `devno`.
    pub(crate) devno: u64,
    /// `parent`: the whole disk, for a partition.
    pub(crate) parent: Option<Rc<PathCxt>>,
}

/// `struct path_cxt`: a directory, read relative to.
#[derive(Debug, Default)]
pub struct PathCxt {
    /// `dir_path`.
    dir_path: Option<Vec<u8>>,
    /// `prefix`: prepended to `dir_path`, for a fake root.
    prefix: Option<Vec<u8>>,
    /// Whether the directory has been opened (`dir_fd >= 0`). A failed open
    /// is retried by the next call, as upstream's is.
    dir_open: Cell<bool>,
    /// The `sysfs_blkdev` dialect, if [`crate::new_sysfs_path`] made this.
    pub(crate) blk: Option<Blkdev>,
}

/// `read_all(fd, buf, count)`: up to `count` bytes, reading until they are
/// all there or the file ends. An error with nothing read is `Err(errno)`;
/// after some bytes it ends the read and they are returned, as upstream
/// returns the count. `EINTR` and `EAGAIN` are retried five times.
///
/// # Errors
///
/// The `errno` of a read that failed before any byte arrived.
pub fn read_all(f: &mut File, count: usize) -> Result<Vec<u8>, i32> {
    let mut buf = vec![0u8; count];
    let mut c = 0usize;
    let mut tries: u32 = 0;
    while c < count {
        let Some(rest) = buf.get_mut(c..) else {
            break;
        };
        match f.read(rest) {
            Ok(0) => break,
            Ok(n) => {
                tries = 0;
                c = c.saturating_add(n);
            }
            Err(e) => {
                let retry = matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                );
                if retry && tries < 5 {
                    tries = tries.saturating_add(1);
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    continue;
                }
                if c == 0 {
                    return Err(errno_of(&e));
                }
                break;
            }
        }
    }
    buf.truncate(c);
    Ok(buf)
}

/// `access(2)`, through the C library: the effective IDs' permission, as
/// `faccessat(dirfd, path, mode, 0)` checks it.
fn access(path: &[u8], mode: i32) -> Result<(), i32> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        unsafe extern "C" {
            fn access(path: *const std::ffi::c_char, mode: i32) -> i32;
        }
        let c = CString::new(path).map_err(|_| ENOENT)?;
        // SAFETY: `c` is a NUL-terminated string that lives across the call;
        // access(2) only reads it.
        let rc = unsafe { access(c.as_ptr(), mode) };
        if rc == 0 {
            Ok(())
        } else {
            Err(errno_of(&std::io::Error::last_os_error()))
        }
    }
    #[cfg(not(unix))]
    {
        // No permission bits to ask about: existence is the whole answer.
        let _ = mode;
        std::fs::metadata(path_of(path))
            .map(|_| ())
            .map_err(|e| errno_of(&e))
    }
}

impl PathCxt {
    /// `ul_new_path(dir)`.
    #[must_use]
    pub fn new(dir: Option<&[u8]>) -> Self {
        PathCxt {
            dir_path: dir.map(<[u8]>::to_vec),
            ..PathCxt::default()
        }
    }

    /// `ul_path_set_prefix`.
    pub fn set_prefix(&mut self, prefix: Option<&[u8]>) {
        self.prefix = prefix.map(<[u8]>::to_vec);
    }

    /// `ul_path_get_prefix`.
    #[must_use]
    pub fn prefix(&self) -> Option<&[u8]> {
        self.prefix.as_deref()
    }

    /// `ul_path_set_dir`: a new directory, not yet opened.
    pub fn set_dir(&mut self, dir: Option<&[u8]>) {
        self.dir_open.set(false);
        self.dir_path = dir.map(<[u8]>::to_vec);
    }

    /// `ul_path_get_dir`.
    #[must_use]
    pub fn dir(&self) -> Option<&[u8]> {
        self.dir_path.as_deref()
    }

    /// `get_absdir`: the directory under the prefix.
    ///
    /// # Errors
    ///
    /// `ENAMETOOLONG` past `PATH_MAX`; `EINVAL` with no directory at all.
    pub fn absdir(&self) -> Result<Vec<u8>, i32> {
        let Some(prefix) = &self.prefix else {
            return self.dir_path.clone().ok_or(EINVAL);
        };
        let Some(dir) = &self.dir_path else {
            return Ok(prefix.clone());
        };
        let dir = dir.strip_prefix(b"/").unwrap_or(dir);
        let mut out = prefix.clone();
        out.push(b'/');
        out.extend_from_slice(dir);
        if out.len() >= PATH_MAX {
            return Err(ENAMETOOLONG);
        }
        Ok(out)
    }

    /// `ul_path_get_dirfd`: open the directory, if it is not open already.
    ///
    /// # Errors
    ///
    /// The `errno` of the open.
    pub fn get_dirfd(&self) -> Result<Vec<u8>, i32> {
        let abs = self.absdir()?;
        if !self.dir_open.get() {
            open_dir(&abs)?;
            self.dir_open.set(true);
        }
        Ok(abs)
    }

    /// `ul_path_is_accessible`.
    #[must_use]
    pub fn is_accessible(&self) -> bool {
        if self.dir_open.get() {
            return true;
        }
        self.absdir().is_ok_and(|p| access(&p, crate::F_OK).is_ok())
    }

    /// The directory an `ENOENT` is retried in: the dialect's parent, if it
    /// has one and it opens (`sysfs_blkdev_enoent_redirect`).
    fn redirect(&self) -> Option<Vec<u8>> {
        let parent = self.blk.as_ref()?.parent.as_ref()?;
        parent.get_dirfd().ok()
    }

    /// `dir/path`, the leading `/` of `path` dropped as upstream drops it.
    fn at(dir: &[u8], path: &[u8]) -> Vec<u8> {
        let path = path.strip_prefix(b"/").unwrap_or(path);
        let mut p = dir.to_vec();
        p.push(b'/');
        p.extend_from_slice(path);
        p
    }

    /// Run `op` on `path` relative to the directory, and once more relative
    /// to the redirect directory if the first answer was `ENOENT`.
    fn with_redirect<T>(
        &self,
        path: &[u8],
        op: impl Fn(&[u8]) -> Result<T, i32>,
    ) -> Result<T, i32> {
        let dir = self.get_dirfd()?;
        match op(&Self::at(&dir, path)) {
            Err(ENOENT) => match self.redirect() {
                Some(pdir) => op(&Self::at(&pdir, path)),
                None => Err(ENOENT),
            },
            r => r,
        }
    }

    /// `ul_path_access(pc, mode, path)`.
    ///
    /// # Errors
    ///
    /// The `errno`: the directory's open, or `faccessat`'s.
    pub fn access(&self, mode: i32, path: &[u8]) -> Result<(), i32> {
        self.with_redirect(path, |p| access(p, mode))
    }

    /// `ul_path_stat(pc, &st, 0, path)`.
    ///
    /// # Errors
    ///
    /// The `errno`: the directory's open, or `fstatat`'s.
    pub fn stat(&self, path: &[u8]) -> Result<std::fs::Metadata, i32> {
        self.with_redirect(path, |p| {
            std::fs::metadata(path_of(p)).map_err(|e| errno_of(&e))
        })
    }

    /// `ul_path_open(pc, O_RDONLY|O_CLOEXEC, path)`.
    ///
    /// # Errors
    ///
    /// The `errno`: the directory's open, or `openat`'s.
    pub fn open(&self, path: &[u8]) -> Result<File, i32> {
        self.with_redirect(path, |p| {
            File::open(path_of(p)).map_err(|e| errno_of(&e))
        })
    }

    /// `ul_path_readlink(pc, buf, PATH_MAX, path)`: the link `path` names
    /// inside the directory, or -- for `None` -- the directory itself.
    ///
    /// # Errors
    ///
    /// The `errno` of the `readlink`.
    pub fn readlink(&self, path: Option<&[u8]>) -> Result<Vec<u8>, i32> {
        let target = match path {
            None => self.absdir()?,
            Some(p) => Self::at(&self.get_dirfd()?, p),
        };
        let link = std::fs::read_link(path_of(&target)).map_err(|e| errno_of(&e))?;
        let mut bytes = quoting::os_bytes(link.as_os_str()).into_owned();
        // `bufsiz - 1` bytes at most, NUL-terminated.
        bytes.truncate(PATH_MAX.saturating_sub(1));
        Ok(bytes)
    }

    /// `ul_path_read(pc, buf, len, path)`: up to `len` bytes of the file.
    ///
    /// # Errors
    ///
    /// The `errno` of the open, or of a read that returned nothing.
    pub fn read(&self, len: usize, path: &[u8]) -> Result<Vec<u8>, i32> {
        let mut f = self.open(path)?;
        read_all(&mut f, len)
    }

    /// `ul_path_read_string(pc, &str, path)`: the file's text, one trailing
    /// newline off, as a C string -- `None` when that leaves nothing.
    ///
    /// # Errors
    ///
    /// As [`PathCxt::read`].
    pub fn read_string(&self, path: &[u8]) -> Result<Option<Vec<u8>>, i32> {
        let mut buf = self.read(BUFSIZ - 1, path)?;
        if buf.last() == Some(&b'\n') {
            buf.pop();
        }
        if buf.is_empty() {
            return Ok(None);
        }
        Ok(Some(c_str(&buf).to_vec()))
    }

    /// `ul_path_read_buffer(pc, buf, bufsz, path)`: the count read and the C
    /// string left in the buffer. A trailing newline is cut off; without one,
    /// upstream overwrites the *last byte read* with the terminator instead,
    /// which this keeps.
    ///
    /// # Errors
    ///
    /// As [`PathCxt::read`].
    pub fn read_buffer(&self, bufsz: usize, path: &[u8]) -> Result<(usize, Vec<u8>), i32> {
        let mut buf = self.read(bufsz.saturating_sub(1), path)?;
        let rc = buf.len();
        if rc > 0 {
            if buf.last() == Some(&b'\n') {
                buf.pop();
                return Ok((rc.saturating_sub(1), c_str(&buf).to_vec()));
            }
            buf.pop();
        }
        Ok((rc, c_str(&buf).to_vec()))
    }

    /// The whole file, for an `fscanf`: `ul_path_fopen` then read.
    fn scan_text(&self, path: &[u8]) -> Option<Vec<u8>> {
        let mut f = self.open(path).ok()?;
        let mut text = Vec::new();
        f.read_to_end(&mut text).ok()?;
        Some(text)
    }

    /// `ul_path_read_s64`: `fscanf("%ld")`.
    /// `None` when the file cannot be opened or holds no number.
    pub fn read_s64(&self, path: &[u8]) -> Option<i64> {
        let text = self.scan_text(path)?;
        scan_long(&text, &mut 0)
    }

    /// `ul_path_read_u64`: `fscanf("%lu")` -- a `-` wraps, as `strtoul`'s
    /// does.
    /// `None` when the file cannot be opened or holds no number.
    pub fn read_u64(&self, path: &[u8]) -> Option<u64> {
        let text = self.scan_text(path)?;
        scan_ulong(&text, &mut 0)
    }

    /// `ul_path_read_s32`: `fscanf("%d")`, the `long` cut to an `int`.
    /// `None` when the file cannot be opened or holds no number.
    pub fn read_s32(&self, path: &[u8]) -> Option<i32> {
        let text = self.scan_text(path)?;
        scan_int(&text, &mut 0)
    }

    /// `ul_path_read_u32`: `fscanf("%u")`.
    /// `None` when the file cannot be opened or holds no number.
    pub fn read_u32(&self, path: &[u8]) -> Option<u32> {
        let text = self.scan_text(path)?;
        let v = scan_ulong(&text, &mut 0)?;
        // `*ARG (unsigned int *) = num.ul`: the low 32 bits.
        Some(v as u32)
    }

    /// `ul_path_read_majmin`: `fscanf("%d:%d")`.
    /// `None` unless both numbers are there.
    pub fn read_majmin(&self, path: &[u8]) -> Option<u64> {
        let text = self.scan_text(path)?;
        scan_majmin(&text)
    }

    /// `ul_path_opendir(pc, path)`: the entries of `path` inside the
    /// directory, or -- for `None` -- of the directory itself.
    ///
    /// # Errors
    ///
    /// The `errno` of the open.
    pub fn opendir(&self, path: Option<&[u8]>) -> Result<Vec<DirEntry>, i32> {
        let dir = match path {
            Some(p) => self.with_redirect(p, |full| {
                std::fs::metadata(path_of(full))
                    .map(|_| full.to_vec())
                    .map_err(|e| errno_of(&e))
            })?,
            None => {
                if self.dir_path.is_none() {
                    return Err(EINVAL);
                }
                self.get_dirfd()?
            }
        };
        read_dir(&dir)
    }

    /// `ul_path_count_dirents(pc, path)`: 0 when it cannot be read.
    #[must_use]
    pub fn count_dirents(&self, path: Option<&[u8]>) -> usize {
        self.opendir(path).map_or(0, |v| v.len())
    }
}

/// `open(path, O_RDONLY|O_CLOEXEC)` of a directory, closed at once: whether
/// upstream's `ul_path_get_dirfd` would succeed.
fn open_dir(path: &[u8]) -> Result<(), i32> {
    #[cfg(unix)]
    {
        File::open(path_of(path)).map(drop).map_err(|e| errno_of(&e))
    }
    #[cfg(not(unix))]
    {
        // Windows cannot open a directory as a file; its existence is what an
        // open would have established.
        std::fs::metadata(path_of(path))
            .map(drop)
            .map_err(|e| errno_of(&e))
    }
}

/// The entries of a directory, in `readdir` order, without `.` and `..`.
fn read_dir(dir: &[u8]) -> Result<Vec<DirEntry>, i32> {
    let rd = std::fs::read_dir(path_of(dir)).map_err(|e| errno_of(&e))?;
    let mut out = Vec::new();
    for e in rd {
        let Ok(e) = e else {
            // A failed `readdir` ends the listing, as NULL from it does.
            break;
        };
        out.push(DirEntry {
            name: quoting::os_bytes(&e.file_name()).into_owned(),
            file_type: e.file_type().ok(),
        });
    }
    Ok(out)
}

/// `fscanf`'s `%ld`: white space skipped, then `strtol`'s value -- clamped
/// to the `long` range on overflow. `None` when no digit follows.
pub(crate) fn scan_long(text: &[u8], pos: &mut usize) -> Option<i64> {
    let rest = text.get(*pos..)?;
    let sc = ulstrutils::scan_integer(rest, 10)?;
    *pos = pos.saturating_add(sc.end);
    let max = u128::from(i64::MAX.unsigned_abs());
    Some(if sc.negative {
        if sc.saturated || sc.magnitude > max.saturating_add(1) {
            i64::MIN
        } else {
            // In range: magnitude <= 2^63.
            0i64.checked_sub_unsigned(u64::try_from(sc.magnitude).unwrap_or(0))
                .unwrap_or(i64::MIN)
        }
    } else if sc.saturated || sc.magnitude > max {
        i64::MAX
    } else {
        i64::try_from(sc.magnitude).unwrap_or(i64::MAX)
    })
}

/// `fscanf`'s `%d`: `%ld`'s value cut to an `int`, as glibc stores it.
pub(crate) fn scan_int(text: &[u8], pos: &mut usize) -> Option<i32> {
    // Truncation is the point: `*ARG (int *) = num.l`.
    #[allow(clippy::cast_possible_truncation, reason = "C stores the long in an int")]
    scan_long(text, pos).map(|v| v as i32)
}

/// `fscanf`'s `%lu`: `strtoul`'s value -- a leading `-` negates modulo
/// 2^64, overflow clamps to the maximum.
pub(crate) fn scan_ulong(text: &[u8], pos: &mut usize) -> Option<u64> {
    let rest = text.get(*pos..)?;
    let sc = ulstrutils::scan_integer(rest, 10)?;
    *pos = pos.saturating_add(sc.end);
    if sc.saturated || sc.magnitude > u128::from(u64::MAX) {
        return Some(u64::MAX);
    }
    let v = u64::try_from(sc.magnitude).unwrap_or(u64::MAX);
    Some(if sc.negative { v.wrapping_neg() } else { v })
}

/// `fscanf("%d:%d")`: the colon must follow the first number at once; the
/// second number may be preceded by white space.
pub(crate) fn scan_majmin(text: &[u8]) -> Option<u64> {
    let mut pos = 0usize;
    let maj = scan_int(text, &mut pos)?;
    if text.get(pos) != Some(&b':') {
        return None;
    }
    pos = pos.saturating_add(1);
    let min = scan_int(text, &mut pos)?;
    // `makedev(maj, min)` of the two ints, reinterpreted as unsigned.
    #[allow(clippy::cast_sign_loss, reason = "makedev takes the ints as unsigned")]
    Some(crate::makedev(maj as u32, min as u32))
}
