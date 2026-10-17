//! util-linux 2.39.3's `lib/fileeq.c`: whether two files of the same size
//! hold the same bytes, read a block at a time so that a file compared with
//! many others is read once.
//!
//! Every file's first 32 bytes (its intro) are compared first. Then, with
//! the `memcmp` method, block after block of both files; with `sha1`,
//! `sha256` or `crc32`, the digest of each block, made by the kernel through
//! an `AF_ALG` socket (`libcall::alg`) and kept, so the file every other is
//! compared with is hashed only once. Where the kernel has no such socket
//! -- SlateOS's has not -- a digest method cannot be set up, and `hardlink`
//! falls back to `memcmp`, as it does on a Linux built without it.

use std::fs::File;
use std::io::{Seek, SeekFrom};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;

use libcall::alg::Hash;

/// `UL_FILEEQ_INTROSIZ`.
const INTROSIZ: usize = 32;

/// `UL_FILEEQ_*`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Id {
    Memcmp,
    Sha1,
    Sha256,
    Crc32,
}

/// `struct ul_fileeq_method`.
#[derive(Debug)]
struct Method {
    id: Id,
    /// The name `--method` takes.
    name: &'static str,
    digsiz: usize,
    kname: &'static [u8],
}

/// `ul_eq_methods[]`.
const METHODS: [Method; 4] = [
    Method {
        id: Id::Memcmp,
        name: "memcmp",
        digsiz: 0,
        kname: b"",
    },
    Method {
        id: Id::Sha1,
        name: "sha1",
        digsiz: 20,
        kname: b"sha1",
    },
    Method {
        id: Id::Sha256,
        name: "sha256",
        digsiz: 32,
        kname: b"sha256",
    },
    Method {
        id: Id::Crc32,
        name: "crc32",
        digsiz: 4,
        kname: b"crc32c",
    },
];

/// `struct ul_fileeq_data`: one file's side of the comparisons.
#[derive(Debug, Default)]
pub struct Data {
    intro: [u8; INTROSIZ],
    blocks: Vec<u8>,
    nblocks: usize,
    file: Option<File>,
    name: Option<Vec<u8>>,
    is_eof: bool,
}

impl Data {
    /// `ul_fileeq_data_associated`.
    pub fn associated(&self) -> bool {
        self.name.is_some()
    }

    /// `ul_fileeq_data_set_file`: made afresh for the file `name`.
    pub fn set_file(&mut self, name: &[u8]) {
        *self = Data {
            name: Some(name.to_vec()),
            ..Data::default()
        };
    }

    /// `ul_fileeq_data_close_file`.
    pub fn close_file(&mut self) {
        self.file = None;
    }

    /// `ul_fileeq_data_deinit`.
    pub fn deinit(&mut self) {
        self.blocks = Vec::new();
        self.nblocks = 0;
        self.is_eof = false;
        self.name = None;
        self.close_file();
    }
}

/// `struct ul_fileeq`: the method and the sizes of one round of
/// comparisons.
#[derive(Debug)]
pub struct FileEq {
    hash: Option<Hash>,
    readsiz: usize,
    filesiz: u64,
    blocksmax: u64,
    method: &'static Method,
}

impl FileEq {
    /// `ul_fileeq_init (eq, method)`: the comparison by `method`, or
    /// `None` for a method there is none of, or one the kernel cannot make
    /// digests for.
    pub fn init(method: &[u8]) -> Option<FileEq> {
        let m = METHODS.iter().find(|m| m.name.as_bytes() == method)?;
        let hash = if m.id == Id::Memcmp {
            None
        } else {
            Some(Hash::open(m.kname).ok()?)
        };
        Some(FileEq {
            hash,
            readsiz: 0,
            filesiz: 0,
            blocksmax: 0,
            method: m,
        })
    }

    /// `ul_fileeq_set_size (eq, filesiz, readsiz, memsiz)`: the files to be
    /// compared are `filesiz` bytes, read `readsiz` at a time, and their
    /// digests may take `memsiz` bytes -- more is read at a time if they
    /// would take more.
    pub fn set_size(&mut self, filesiz: u64, readsiz: usize, memsiz: usize) {
        // "align file size": up past its end to a whole number of reads.
        let align = |r: usize| {
            let r = u64::try_from(r).unwrap_or(u64::MAX);
            filesiz
                .saturating_add(r)
                .checked_div(r)
                .unwrap_or(0)
                .saturating_mul(r)
        };
        let per = |total: u64, part: usize| {
            total
                .checked_div(u64::try_from(part).unwrap_or(u64::MAX))
                .unwrap_or(0)
        };
        self.filesiz = filesiz;
        let mut readsiz = readsiz;
        let aligned = if self.method.id == Id::Memcmp {
            align(readsiz)
        } else {
            let digsiz = self.method.digsiz;
            if readsiz < digsiz {
                readsiz = digsiz;
            }
            let aligned = align(readsiz);
            // "calculate limits"
            let maxdigs = u64::try_from(memsiz.checked_div(digsiz).unwrap_or(0))
                .unwrap_or(u64::MAX)
                .max(1);
            let nreads = per(aligned, readsiz);
            // "enlarge readsize for large files"
            if nreads > maxdigs {
                readsiz = usize::try_from(aligned.checked_div(maxdigs).unwrap_or(0))
                    .unwrap_or(usize::MAX);
            }
            aligned
        };
        self.readsiz = readsiz;
        self.blocksmax = per(aligned, readsiz);
    }

    /// `get_cached_offset (eq, data)`: where in the file the next block
    /// starts, given what has been read of it.
    fn cached_offset(&self, data: &Data) -> u64 {
        if data.nblocks == 0 {
            0
        } else {
            let cached = u64::try_from(data.nblocks.saturating_sub(1)).unwrap_or(u64::MAX);
            (INTROSIZ as u64).saturating_add(cached.saturating_mul(self.readsiz as u64))
        }
    }

    /// `get_fd (eq, data, &off)`: the file, opened (and moved to where its
    /// next block starts) if it was not open; and that offset. `None` when
    /// it cannot be opened.
    fn get_fd(&self, data: &mut Data) -> Option<u64> {
        let o = self.cached_offset(data);
        if data.file.is_none() {
            let name = data.name.clone()?;
            let mut f = File::open(std::ffi::OsStr::from_bytes(&name)).ok()?;
            // `posix_fadvise (POSIX_FADV_SEQUENTIAL)` is advice, which
            // changes nothing anyone can see.
            if o != 0 && f.seek(SeekFrom::Start(o)).is_err() {
                return None;
            }
            data.file = Some(f);
        }
        Some(o)
    }

    /// `memcmp_reset (eq, data)`: only the intro kept; the file back to just
    /// after it.
    fn memcmp_reset(&self, data: &mut Data) {
        if data.nblocks != 0 {
            data.nblocks = 1;
        }
        let off = self.cached_offset(data);
        if let Some(f) = data.file.as_mut() {
            // As upstream's `lseek`, whose answer is not looked at.
            let _ = f.seek(SeekFrom::Start(off));
        }
        data.is_eof = false;
    }

    /// `read_block (eq, data, n, &block)`: the file's next block, or empty
    /// at its end; `None` for an error.
    fn read_block(&self, data: &mut Data, n: u64) -> Option<Vec<u8>> {
        if data.is_eof || n > self.blocksmax {
            return Some(Vec::new());
        }
        let off = self.get_fd(data)?;
        let mut block = vec![0u8; self.readsiz];
        let fd = data.file.as_ref()?.as_raw_fd();
        let rsz = ulallio::read_all(fd, &mut block).ok()?;
        block.truncate(rsz);
        data.nblocks = data.nblocks.saturating_add(1);
        if rsz == 0 || off.saturating_add(rsz as u64) >= self.filesiz {
            data.is_eof = true;
            data.close_file();
        }
        Some(block)
    }

    /// `get_digest (eq, data, n, &block)`: the digest of the file's block
    /// `n`, from those already made or by sending the next block to the
    /// kernel; empty at the file's end, `None` for an error.
    fn get_digest(&self, data: &mut Data, n: u64) -> Option<Vec<u8>> {
        let digsiz = self.method.digsiz;
        if n > self.blocksmax {
            return Some(Vec::new());
        }
        let at = usize::try_from(n).ok()?.saturating_mul(digsiz);
        // "return already cached if available"
        if usize::try_from(n).ok()? < data.nblocks.saturating_sub(1) {
            return data
                .blocks
                .get(at..at.saturating_add(digsiz))
                .map(<[u8]>::to_vec);
        }
        if data.is_eof {
            return Some(Vec::new());
        }
        let off = self.get_fd(data)?;
        if data.blocks.is_empty() {
            let max = usize::try_from(self.blocksmax).unwrap_or(0);
            data.blocks = vec![0u8; max.saturating_mul(digsiz)];
        }
        let hash = self.hash.as_ref()?;
        let fd = data.file.as_ref()?.as_raw_fd();
        let rsz = hash.send_file(fd, self.readsiz).ok()?;
        let off = off.saturating_add(rsz as u64);
        if data.blocks.len() < at.saturating_add(digsiz) {
            data.blocks.resize(at.saturating_add(digsiz), 0);
        }
        let slot = data.blocks.get_mut(at..at.saturating_add(digsiz))?;
        let got = ulallio::read_all(hash.op(), slot).ok();
        if got.is_some_and(|g| g > 0) {
            data.nblocks = data.nblocks.saturating_add(1);
        }
        if got == Some(0) || off >= self.filesiz {
            data.is_eof = true;
            data.close_file();
        }
        data.blocks
            .get(at..at.saturating_add(got?))
            .map(<[u8]>::to_vec)
    }

    /// `get_intro (eq, data, &block)`: the file's first 32 bytes, read once
    /// and kept -- zeros past a shorter file's end.
    fn get_intro(&self, data: &mut Data) -> Option<Vec<u8>> {
        if data.nblocks == 0 {
            self.get_fd(data)?;
            let mut intro = [0u8; INTROSIZ];
            let fd = data.file.as_ref()?.as_raw_fd();
            let rsz = ulallio::read_all(fd, &mut intro).ok()?;
            if rsz == 0 {
                return None;
            }
            data.intro = intro;
            data.nblocks = 1;
        }
        Some(data.intro.to_vec())
    }

    /// `get_cmp_data (eq, data, blockno, &block)`.
    fn get_cmp_data(&self, data: &mut Data, blockno: u64) -> Option<Vec<u8>> {
        if blockno == 0 {
            return self.get_intro(data);
        }
        let n = blockno.saturating_sub(1);
        if self.method.id == Id::Memcmp {
            self.read_block(data, n)
        } else {
            self.get_digest(data, n)
        }
    }

    /// `ul_fileeq (eq, a, b)`: whether the two files are the same, block by
    /// block -- and both read to their end, so that a file that grew or
    /// shrank since it was measured is not taken for its twin.
    pub fn equal(&self, a: &mut Data, b: &mut Data) -> bool {
        if self.method.id == Id::Memcmp {
            self.memcmp_reset(a);
            self.memcmp_reset(b);
        }
        let mut n = 0u64;
        loop {
            // b is read only when a could be.
            let Some(da) = self.get_cmp_data(a, n) else {
                return false;
            };
            let Some(db) = self.get_cmp_data(b, n) else {
                return false;
            };
            if da.len() != db.len() {
                return false;
            }
            // Both at their end: the same so far, and nothing more.
            if da.is_empty() {
                break;
            }
            if da != db {
                return false;
            }
            n = n.saturating_add(1);
        }
        // "filesize changed?"
        a.is_eof && b.is_eof
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

    fn tmp(tag: &str, bytes: &[u8]) -> Vec<u8> {
        let p = std::env::temp_dir().join(format!("hardlink-fileeq-{tag}-{}", std::process::id()));
        std::fs::write(&p, bytes).unwrap();
        p.as_os_str().as_bytes().to_vec()
    }

    fn same(method: &[u8], x: &[u8], y: &[u8], io: usize) -> Option<bool> {
        let mut eq = FileEq::init(method)?;
        let (pa, pb) = (tmp("a", x), tmp("b", y));
        eq.set_size(x.len() as u64, io, 1024);
        let (mut a, mut b) = (Data::default(), Data::default());
        a.set_file(&pa);
        b.set_file(&pb);
        let r = eq.equal(&mut a, &mut b);
        // A second comparison of the same pair, as against a master kept.
        b.close_file();
        let r2 = eq.equal(&mut a, &mut b);
        assert_eq!(r, r2, "the second round agrees");
        Some(r)
    }

    #[test]
    fn files_are_equal_exactly_when_their_bytes_are() {
        for method in [&b"memcmp"[..], b"sha256", b"crc32", b"sha1"] {
            let big: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
            let mut other = big.clone();
            other[9_999] ^= 1;
            let cases: [(&[u8], &[u8], bool); 5] = [
                (b"hello", b"hello", true),
                (b"hello", b"hellp", false),
                (&big, &big, true),
                (&big, &other, false),
                (&[7u8; 33], &[7u8; 33], true),
            ];
            for (x, y, want) in cases {
                // The kernel's digests are not everywhere.
                if let Some(got) = same(method, x, y, 64) {
                    assert_eq!(
                        got,
                        want,
                        "{} {:?}",
                        String::from_utf8_lossy(method),
                        x.len()
                    );
                }
            }
        }
        assert!(FileEq::init(b"md5").is_none(), "no such method");
    }
}
