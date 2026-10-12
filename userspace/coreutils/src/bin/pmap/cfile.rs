//! A file read the way `pmap` reads `/proc/PID/maps` and `smaps`: with
//! stdio's `fgets` into a buffer of fixed size, and rewound with `fseek`.
//!
//! The size is not a detail. `fgets (mapbuf, 1024, f)` hands back at most
//! 1023 bytes, so a line longer than that -- a mapping of a file whose path
//! is long -- arrives in pieces, and `pmap` reads each piece as if it were a
//! line of its own. What it prints for the second piece is whatever its
//! `sscanf` made of it, and the port reads the file the same way so that it
//! prints the same.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

/// How much one `read` asks for.
const CHUNK: usize = 4096;

/// An open file, its unread bytes, and whether it has ended.
pub struct CFile {
    file: File,
    buf: Vec<u8>,
    at: usize,
    ended: bool,
}

impl CFile {
    /// `fopen (path, "r")`.
    ///
    /// # Errors
    ///
    /// The file could not be opened.
    pub fn open(path: &std::path::Path) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
            buf: Vec::new(),
            at: 0,
            ended: false,
        })
    }

    /// Read more into the buffer; false when there is nothing more. A read
    /// that fails ends the file, as `fgets` answers an error and the end
    /// alike, with `NULL`.
    fn fill(&mut self) -> bool {
        if self.ended {
            return false;
        }
        if self.at > 0 {
            self.buf.drain(..self.at);
            self.at = 0;
        }
        let mut chunk = [0u8; CHUNK];
        let got = loop {
            match self.file.read(&mut chunk) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Ok(n) => break n,
                Err(_) => break 0,
            }
        };
        if got == 0 {
            self.ended = true;
            return false;
        }
        self.buf
            .extend_from_slice(chunk.get(..got).unwrap_or_default());
        true
    }

    /// `fgets (buf, size, f)`: the next line, through its newline, or its
    /// first `size - 1` bytes when it is longer; `None` at the end with
    /// nothing read.
    pub fn fgets(&mut self, size: usize) -> Option<Vec<u8>> {
        let limit = size.saturating_sub(1);
        let mut line = Vec::new();
        while line.len() < limit {
            if self.at >= self.buf.len() && !self.fill() {
                break;
            }
            let rest = self.buf.get(self.at..).unwrap_or_default();
            let room = limit.saturating_sub(line.len());
            let take = match rest.iter().take(room).position(|&b| b == b'\n') {
                Some(nl) => nl.saturating_add(1),
                None => rest.len().min(room),
            };
            line.extend_from_slice(rest.get(..take).unwrap_or_default());
            self.at = self.at.saturating_add(take);
            if line.last() == Some(&b'\n') {
                break;
            }
        }
        if line.is_empty() { None } else { Some(line) }
    }

    /// `fseek (f, 0, SEEK_SET)`: back to the start, what was buffered
    /// dropped, so the next `fgets` reads the file afresh -- for `/proc`,
    /// the kernel writes it again.
    pub fn rewind(&mut self) {
        self.buf.clear();
        self.at = 0;
        // `pmap` does not look at `fseek`'s answer. A seek that failed
        // leaves nothing to read, which is what the next `fgets` then finds.
        self.ended = self.file.seek(SeekFrom::Start(0)).is_err();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::CFile;

    fn scratch(tag: &str, text: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pmap-cfile-{tag}-{}", std::process::id()));
        std::fs::write(&p, text).unwrap();
        p
    }

    #[test]
    fn lines_come_whole_or_in_pieces_of_size_less_one() {
        let p = scratch("lines", b"ab\ncdefgh\n\nlast");
        let mut f = CFile::open(&p).unwrap();
        assert_eq!(f.fgets(1024).unwrap(), b"ab\n");
        assert_eq!(f.fgets(4).unwrap(), b"cde", "three bytes and no newline");
        assert_eq!(f.fgets(4).unwrap(), b"fgh");
        assert_eq!(f.fgets(4).unwrap(), b"\n", "the rest of that line");
        assert_eq!(f.fgets(1024).unwrap(), b"\n");
        assert_eq!(f.fgets(1024).unwrap(), b"last", "no newline at the end");
        assert!(f.fgets(1024).is_none());
        assert!(f.fgets(1024).is_none(), "and stays ended");
        f.rewind();
        assert_eq!(f.fgets(1024).unwrap(), b"ab\n", "rewound");
        std::fs::remove_file(&p).unwrap();
    }

    #[test]
    fn a_line_longer_than_one_read_is_joined() {
        let mut text = vec![b'x'; 10_000];
        text.push(b'\n');
        let p = scratch("long", &text);
        let mut f = CFile::open(&p).unwrap();
        assert_eq!(f.fgets(1024).unwrap().len(), 1023);
        let mut total = 1023;
        while let Some(piece) = f.fgets(1024) {
            total += piece.len();
        }
        assert_eq!(total, 10_001);
        std::fs::remove_file(&p).unwrap();
    }
}
