//! Where a torrent's bytes go: its files below a save folder, and the pieces
//! that run across them.
//!
//! A torrent is one stream of bytes cut into pieces of `piece_length`; its
//! files are that stream cut again, at other places. A piece can end inside
//! one file and begin inside the next, so reading or writing a piece is a
//! walk over the files it spans ([`Storage::spans`]).
//!
//! **Only whole, checked pieces are written.** A peer's blocks are gathered
//! in memory and the piece is hashed before any of it reaches the disk, so a
//! file never holds bytes that failed their hash, and a half-fetched piece
//! costs nothing to throw away.
//!
//! **Nothing is written outside the save folder.** Every part of every path
//! was checked when the torrent was read (`checked_part`: no `..`, no `/`, no
//! empty part), and here a folder or file already at a path is refused if it
//! is a symbolic link -- one planted in the save folder would otherwise carry
//! the write wherever it points.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::TorrentMetainfo;

/// One of the torrent's files, where it lies on disk and in the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StoredFile {
    /// Its place on disk: below the save folder.
    path: PathBuf,
    /// Where it starts in the torrent's stream of bytes.
    offset: u64,
    length: u64,
}

/// A part of a piece that lies in one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    /// Which file, as an index into the torrent's files.
    pub file: usize,
    /// Where in that file.
    pub offset: u64,
    /// How many bytes.
    pub length: u64,
}

/// A torrent's files below a save folder.
#[derive(Debug, Clone)]
pub struct Storage {
    /// The save folder: nothing is written outside it.
    root: PathBuf,
    files: Vec<StoredFile>,
    piece_length: u64,
    total: u64,
    hashes: Vec<[u8; 20]>,
}

/// `bytes` as one part of a path on this system.
///
/// On SlateOS (and any Unix) a name is bytes, and any bytes the torrent
/// checker let through will do. Elsewhere -- the Windows host the tests run
/// on -- a name must be text, and a part that is not UTF-8 cannot be written.
fn os_part(bytes: &[u8]) -> Result<OsString, String> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Ok(std::ffi::OsStr::from_bytes(bytes).to_os_string())
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(bytes.to_vec())
            .map(OsString::from)
            .map_err(|_| {
                format!(
                    "\"{}\" cannot be a file name on this system",
                    crate::shown_bytes(bytes)
                )
            })
    }
}

impl Storage {
    /// The files of `meta` below `save_dir`: a single-file torrent's file is
    /// `save_dir/name`, a multi-file torrent's are `save_dir/name/...`.
    ///
    /// # Errors
    ///
    /// A path part this system cannot hold as a name.
    pub fn new(meta: &TorrentMetainfo, save_dir: &Path) -> Result<Self, String> {
        let mut files = Vec::with_capacity(meta.files.len());
        let mut offset = 0_u64;
        for file in &meta.files {
            let mut path = save_dir.to_path_buf();
            if meta.multi_file {
                path.push(os_part(&meta.name_bytes)?);
            }
            for part in &file.parts {
                path.push(os_part(part)?);
            }
            files.push(StoredFile {
                path,
                offset,
                length: file.length,
            });
            offset = offset
                .checked_add(file.length)
                .ok_or("the files add up to more bytes than can be counted")?;
        }
        Ok(Self {
            root: save_dir.to_path_buf(),
            files,
            piece_length: meta.piece_length,
            total: offset,
            hashes: meta.pieces.clone(),
        })
    }

    /// How many pieces there are.
    #[must_use]
    pub fn piece_count(&self) -> usize {
        self.hashes.len()
    }

    /// How long piece `index` is: `piece_length`, or less for the last.
    #[must_use]
    pub fn piece_len(&self, index: usize) -> u64 {
        let start = self.piece_length.saturating_mul(index as u64);
        self.total.saturating_sub(start).min(self.piece_length)
    }

    /// Where file `index` is on disk.
    #[must_use]
    pub fn file_path(&self, index: usize) -> Option<&Path> {
        self.files.get(index).map(|f| f.path.as_path())
    }

    /// The parts of the files that piece `index` covers, in order. Empty for
    /// a piece past the end.
    #[must_use]
    pub fn spans(&self, index: usize) -> Vec<Span> {
        let start = self.piece_length.saturating_mul(index as u64);
        let end = start.saturating_add(self.piece_len(index));
        self.files
            .iter()
            .enumerate()
            .filter_map(|(i, f)| {
                let from = start.max(f.offset);
                let to = end.min(f.offset.saturating_add(f.length));
                (from < to).then(|| Span {
                    file: i,
                    offset: from - f.offset,
                    length: to - from,
                })
            })
            .collect()
    }

    /// Whether `data` is piece `index`: its length and its SHA-1.
    #[must_use]
    pub fn matches(&self, index: usize, data: &[u8]) -> bool {
        self.hashes
            .get(index)
            .is_some_and(|h| data.len() as u64 == self.piece_len(index) && sha1::sha1(data) == *h)
    }

    /// Write checked piece `index` into the files it spans, making the
    /// folders it needs.
    ///
    /// # Errors
    ///
    /// `data` is not the piece's length; a folder or file on the way is a
    /// symbolic link; or the write failed.
    pub fn write_piece(&self, index: usize, data: &[u8]) -> Result<(), String> {
        if data.len() as u64 != self.piece_len(index) {
            return Err(format!(
                "piece {index} is {} bytes, not {}",
                data.len(),
                self.piece_len(index)
            ));
        }
        let mut at = 0_usize;
        for span in self.spans(index) {
            let file = self
                .files
                .get(span.file)
                .ok_or("a piece spans a file that is not there")?;
            let len = usize::try_from(span.length).map_err(|_| "a span too long to hold")?;
            let bytes = data
                .get(at..at.saturating_add(len))
                .ok_or("a piece shorter than its spans")?;
            self.write_at(&file.path, span.offset, bytes)
                .map_err(|e| format!("could not write {}: {e}", file.path.display()))?;
            at = at.saturating_add(len);
        }
        Ok(())
    }

    /// Read piece `index` back from disk: `None` if any of it is missing --
    /// a file not there, or shorter than the piece needs.
    ///
    /// # Errors
    ///
    /// A read that failed for a reason other than the file being absent.
    pub fn read_piece(&self, index: usize) -> Result<Option<Vec<u8>>, String> {
        let want =
            usize::try_from(self.piece_len(index)).map_err(|_| "a piece too long to hold")?;
        let mut out = Vec::with_capacity(want);
        for span in self.spans(index) {
            let Some(file) = self.files.get(span.file) else {
                return Ok(None);
            };
            let mut handle = match File::open(&file.path) {
                Ok(handle) => handle,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(format!("could not read {}: {e}", file.path.display())),
            };
            let len = usize::try_from(span.length).map_err(|_| "a span too long to hold")?;
            let start = out.len();
            out.resize(start.saturating_add(len), 0);
            let read = handle
                .seek(SeekFrom::Start(span.offset))
                .and_then(|_| read_full(&mut handle, out.get_mut(start..).unwrap_or_default()));
            match read {
                Ok(n) if n == len => {}
                Ok(_) => return Ok(None),
                Err(e) => return Err(format!("could not read {}: {e}", file.path.display())),
            }
        }
        Ok(Some(out))
    }

    /// Which pieces are already on disk and whole: what a download started
    /// again need not fetch.
    ///
    /// # Errors
    ///
    /// As [`Self::read_piece`].
    pub fn have(&self) -> Result<Vec<bool>, String> {
        (0..self.piece_count())
            .map(|i| {
                Ok(self
                    .read_piece(i)?
                    .is_some_and(|data| self.matches(i, &data)))
            })
            .collect()
    }

    /// Write `bytes` at `offset` of the file at `path`, creating it and its
    /// folders -- none of which may be a symbolic link.
    fn write_at(&self, path: &Path, offset: u64, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            self.make_folders(parent)?;
        }
        refuse_link(path)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(bytes)?;
        file.flush()
    }

    /// Make `folder` and every folder between it and the save folder,
    /// refusing any that is already a symbolic link.
    fn make_folders(&self, folder: &Path) -> io::Result<()> {
        let below = folder.strip_prefix(&self.root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "a folder outside the save folder",
            )
        })?;
        let mut at = self.root.clone();
        for part in below.components() {
            at.push(part);
            refuse_link(&at)?;
            match fs::create_dir(&at) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists && at.is_dir() => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

/// An error if `path` is a symbolic link; nothing there, or anything else,
/// is fine.
fn refuse_link(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(md) if md.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a link, and is not written through", path.display()),
        )),
        _ => Ok(()),
    }
}

/// Read into `buf` until it is full or the file ends; how much was read.
fn read_full(file: &mut File, buf: &mut [u8]) -> io::Result<usize> {
    let mut done = 0;
    while done < buf.len() {
        match file.read(buf.get_mut(done..).unwrap_or_default()) {
            Ok(0) => break,
            Ok(n) => done = done.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(done)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::TorrentFile;

    /// A scratch folder, removed when dropped.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU64, Ordering};
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "slateos-torrent-storage-{tag}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    /// A torrent of `files` (path, content) in pieces of `piece_length`,
    /// its hashes those of the content.
    fn torrent(
        files: &[(&str, &[u8])],
        piece_length: u64,
        multi: bool,
    ) -> (TorrentMetainfo, Vec<u8>) {
        let stream: Vec<u8> = files.iter().flat_map(|(_, c)| c.iter().copied()).collect();
        let pieces = stream
            .chunks(usize::try_from(piece_length).unwrap())
            .map(sha1::sha1)
            .collect();
        let mut meta = crate::create_sample_torrent("Set", stream.len() as u64, piece_length, "");
        meta.pieces = pieces;
        meta.multi_file = multi;
        meta.name_bytes = b"Set".to_vec();
        meta.files = files
            .iter()
            .map(|(p, c)| TorrentFile::named(p, c.len() as u64))
            .collect();
        (meta, stream)
    }

    /// A piece that runs from one file into the next is split where the
    /// files meet.
    #[test]
    fn a_piece_spans_the_files_it_covers() {
        let (meta, _) = torrent(&[("a", &[1; 250]), ("b", &[2; 150])], 100, true);
        let dir = Scratch::new("spans");
        let st = Storage::new(&meta, &dir.0).unwrap();
        assert_eq!(st.piece_count(), 4);
        assert_eq!(
            st.spans(0),
            vec![Span {
                file: 0,
                offset: 0,
                length: 100
            }]
        );
        assert_eq!(
            st.spans(2),
            vec![
                Span {
                    file: 0,
                    offset: 200,
                    length: 50
                },
                Span {
                    file: 1,
                    offset: 0,
                    length: 50
                },
            ]
        );
        assert_eq!(
            st.spans(3),
            vec![Span {
                file: 1,
                offset: 50,
                length: 100
            }]
        );
        assert_eq!(st.spans(4), vec![], "past the end");
        assert_eq!(st.file_path(0), Some(dir.0.join("Set").join("a").as_path()));
    }

    /// Pieces written in any order make the files; read back, each matches
    /// its hash; a piece that does not is refused by `matches`.
    #[test]
    fn pieces_written_make_the_files() {
        let (meta, stream) = torrent(&[("sub/a", &[1; 250]), ("b", &[2; 150])], 100, true);
        let dir = Scratch::new("write");
        let st = Storage::new(&meta, &dir.0).unwrap();
        assert_eq!(st.have().unwrap(), vec![false; 4]);
        for i in [3, 0, 2, 1] {
            let piece = &stream[i * 100..(i * 100 + 100).min(stream.len())];
            assert!(st.matches(i, piece));
            st.write_piece(i, piece).unwrap();
        }
        assert_eq!(
            fs::read(dir.0.join("Set").join("sub").join("a")).unwrap(),
            vec![1; 250]
        );
        assert_eq!(fs::read(dir.0.join("Set").join("b")).unwrap(), vec![2; 150]);
        assert_eq!(st.have().unwrap(), vec![true; 4]);
        assert!(!st.matches(0, &[0; 100]), "the wrong bytes");
        assert!(!st.matches(0, &stream[..99]), "the wrong length");
        assert!(st.write_piece(0, &stream[..99]).is_err());
    }

    /// A single-file torrent's file is the name itself, in the save folder.
    #[test]
    fn a_single_file_is_named_by_the_torrent() {
        let (mut meta, stream) = torrent(&[("Set", &[9; 30])], 16, false);
        meta.name_bytes = b"Set".to_vec();
        let dir = Scratch::new("single");
        let st = Storage::new(&meta, &dir.0).unwrap();
        st.write_piece(0, &stream[..16]).unwrap();
        st.write_piece(1, &stream[16..]).unwrap();
        assert_eq!(fs::read(dir.0.join("Set")).unwrap(), vec![9; 30]);
    }

    /// A piece whose file is short or missing is not had.
    #[test]
    fn a_missing_or_short_file_is_not_had() {
        let (meta, stream) = torrent(&[("a", &[1; 100]), ("b", &[2; 100])], 100, true);
        let dir = Scratch::new("short");
        let st = Storage::new(&meta, &dir.0).unwrap();
        st.write_piece(0, &stream[..100]).unwrap();
        assert_eq!(st.have().unwrap(), vec![true, false]);
        fs::create_dir_all(dir.0.join("Set")).unwrap();
        fs::write(dir.0.join("Set").join("b"), [2; 60]).unwrap();
        assert_eq!(st.read_piece(1).unwrap(), None);
        assert_eq!(st.have().unwrap(), vec![true, false]);
    }

    /// A link in the save folder is not written through.
    #[cfg(unix)]
    #[test]
    fn a_link_in_the_way_is_refused() {
        let (meta, stream) = torrent(&[("a", &[1; 10])], 16, true);
        let dir = Scratch::new("link");
        let elsewhere = Scratch::new("elsewhere");
        std::os::unix::fs::symlink(&elsewhere.0, dir.0.join("Set")).unwrap();
        let st = Storage::new(&meta, &dir.0).unwrap();
        let err = st.write_piece(0, &stream).unwrap_err();
        assert!(err.contains("link"), "{err}");
        assert!(
            !elsewhere.0.join("a").exists(),
            "the write went through the link"
        );
    }

    /// A name that is not UTF-8 is written as its bytes on SlateOS; the host
    /// the tests run on cannot hold it, and says so.
    #[test]
    fn a_name_that_is_not_utf8() {
        let (mut meta, _) = torrent(&[("a", &[1; 10])], 16, true);
        meta.files[0].parts = vec![b"caf\xe9".to_vec()];
        let dir = Scratch::new("bytes");
        let made = Storage::new(&meta, &dir.0);
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let st = made.unwrap();
            let name = st.file_path(0).unwrap().file_name().unwrap();
            assert_eq!(name.as_bytes(), b"caf\xe9");
        }
        #[cfg(not(unix))]
        assert!(made.unwrap_err().contains("cannot be a file name"));
    }
}
