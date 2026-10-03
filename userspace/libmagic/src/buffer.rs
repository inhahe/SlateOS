//! libmagic's `buffer.c`: what is being identified -- the bytes read from the
//! start of the file, the file's `stat`, and, read only when a rule asks for an
//! offset from the end, the bytes at the end.

use std::cell::OnceCell;
use std::fs::File;

/// The parts of `struct stat` libmagic reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub mode: u32,
    pub size: u64,
    pub rdev: u64,
    pub nlink: u64,
    /// Seconds since the epoch, for `-p` to put back.
    pub atime: i64,
    pub mtime: i64,
}

/// `S_IFMT` and the file types under it.
pub const S_IFMT: u32 = 0o170_000;
pub const S_IFSOCK: u32 = 0o140_000;
pub const S_IFLNK: u32 = 0o120_000;
pub const S_IFREG: u32 = 0o100_000;
pub const S_IFBLK: u32 = 0o060_000;
pub const S_IFDIR: u32 = 0o040_000;
pub const S_IFCHR: u32 = 0o020_000;
pub const S_IFIFO: u32 = 0o010_000;
pub const S_ISUID: u32 = 0o4000;
pub const S_ISGID: u32 = 0o2000;
pub const S_ISVTX: u32 = 0o1000;

impl Stat {
    #[must_use]
    pub fn is_reg(&self) -> bool {
        self.mode & S_IFMT == S_IFREG
    }

    #[must_use]
    pub fn is_fifo(&self) -> bool {
        self.mode & S_IFMT == S_IFIFO
    }

    /// The fields from a `std::fs::Metadata`.
    #[must_use]
    pub fn from_metadata(md: &std::fs::Metadata) -> Stat {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Stat {
                mode: md.mode(),
                size: md.size(),
                rdev: md.rdev(),
                nlink: md.nlink(),
                atime: md.atime(),
                mtime: md.mtime(),
            }
        }
        #[cfg(not(unix))]
        {
            // The host build: the type, and permissions as Windows can say.
            let ft = md.file_type();
            let kind = if ft.is_dir() {
                S_IFDIR
            } else if ft.is_symlink() {
                S_IFLNK
            } else {
                S_IFREG
            };
            let perm = if md.permissions().readonly() { 0o444 } else { 0o644 };
            let secs = |t: std::io::Result<std::time::SystemTime>| {
                t.ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .and_then(|d| i64::try_from(d.as_secs()).ok())
                    .unwrap_or(0)
            };
            Stat {
                mode: kind | perm,
                size: md.len(),
                rdev: 0,
                nlink: 1,
                atime: secs(md.accessed()),
                mtime: secs(md.modified()),
            }
        }
    }
}

/// The tail of the file, read on first use: the bytes and the offset they
/// start at, or `None` when they could not be read (`elen == FILE_BADSIZE`).
type Tail = Option<(u64, Vec<u8>)>;

/// `struct buffer`.
pub struct Buffer<'a> {
    /// The open file, when there is one (`fd`): the tail is read from it, and
    /// so are an ELF file's headers.
    pub fd: Option<&'a File>,
    pub st: Stat,
    /// The bytes read from the start (`fbuf`, `flen`).
    pub fbuf: &'a [u8],
    /// `ebuf`, `elen` and `eoff`, filled by [`Buffer::fill`].
    tail: OnceCell<Tail>,
}

impl<'a> Buffer<'a> {
    /// `buffer_init`: when no `stat` is given, the file's own, or zeros.
    #[must_use]
    pub fn new(fd: Option<&'a File>, st: Option<Stat>, data: &'a [u8]) -> Buffer<'a> {
        let st = st.unwrap_or_else(|| {
            fd.and_then(|f| f.metadata().ok())
                .map(|md| Stat::from_metadata(&md))
                .unwrap_or_default()
        });
        Buffer {
            fd,
            st,
            fbuf: data,
            tail: OnceCell::new(),
        }
    }

    /// A buffer over other bytes with this one's file and `stat`: what
    /// `bb = *b; bb.fbuf = ...` makes for an `indirect` rule.
    #[must_use]
    pub fn with_data(&self, data: &'a [u8]) -> Buffer<'a> {
        Buffer {
            fd: self.fd,
            st: self.st,
            fbuf: data,
            tail: OnceCell::new(),
        }
    }

    /// `flen`.
    #[must_use]
    pub fn flen(&self) -> usize {
        self.fbuf.len()
    }

    /// `buffer_fill`: read the last `min(st_size, flen)` bytes of a regular
    /// file, once. `None` is -1.
    pub fn fill(&self) -> Option<&[u8]> {
        self.tail
            .get_or_init(|| self.read_tail())
            .as_ref()
            .map(|(_, v)| v.as_slice())
    }

    fn read_tail(&self) -> Tail {
        if !self.st.is_reg() {
            return None;
        }
        let size = self.st.size;
        let elen = usize::try_from(size).map_or(self.fbuf.len(), |s| s.min(self.fbuf.len()));
        if elen == 0 {
            return Some((size, Vec::new()));
        }
        let eoff = size.saturating_sub(elen as u64);
        let fd = self.fd?;
        let mut v = vec![0u8; elen];
        // `pread` returns what it read; C does not check that it read it all,
        // and neither does this -- the rest stays zero, as malloc'd memory
        // would not, but a short read of a regular file is not a case that
        // arises.
        read_at(fd, &mut v, eoff).ok()?;
        Some((eoff, v))
    }
}

#[cfg(unix)]
fn read_at(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<usize> {
    use std::os::unix::fs::FileExt;
    f.read_at(buf, off)
}

#[cfg(windows)]
fn read_at(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<usize> {
    use std::os::windows::fs::FileExt;
    f.seek_read(buf, off)
}

#[cfg(not(any(unix, windows)))]
fn read_at(_f: &File, _buf: &mut [u8], _off: u64) -> std::io::Result<usize> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Read at an offset from the open file: `pread`, for the readers that seek
/// around in it (ELF headers, CDF sectors).
pub fn pread(f: &File, buf: &mut [u8], off: u64) -> std::io::Result<usize> {
    read_at(f, buf, off)
}
