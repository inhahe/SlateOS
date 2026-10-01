//! `fgets` on `stdin`, as glibc 2.39 does it: what uudecode reads its input
//! with, and therefore where its lines end.
//!
//! A line is at most one byte short of the buffer (16 383 bytes into
//! uudecode's 16 384); a longer one arrives in pieces, each read as a line of
//! its own. The line and a NUL are stored at the start of the caller's
//! buffer and **nothing else in it changes**: uudecode decodes past the end
//! of a short line into whatever an earlier, longer line left there, and the
//! caller keeps one buffer for that reason. End of file is sticky, as since
//! glibc 2.28. A read error during a call makes it return nothing, even
//! after bytes were copied -- except `EAGAIN`, which returns what there is.

use std::fs::File;
use std::io::{self, Read};

/// Where the lines come from.
pub enum Source {
    /// `freopen(FILE, "r", stdin)`.
    File(File),
    /// Descriptor 0 as the program was given it, read directly: Rust's
    /// `Stdin` reads a closed descriptor as end of file, glibc as `EBADF`.
    Stdin,
}

impl Source {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Source::File(f) => f.read(buf),
            Source::Stdin => read_fd0(buf),
        }
    }
}

#[cfg(unix)]
fn read_fd0(buf: &mut [u8]) -> io::Result<usize> {
    use std::mem::ManuallyDrop;
    use std::os::fd::FromRawFd;
    // SAFETY: descriptor 0 is borrowed for this one read and never closed
    // here -- `ManuallyDrop` keeps the `File` from closing it. A closed
    // descriptor 0 is a defined `EBADF` from `read`, which is the point.
    let mut f = ManuallyDrop::new(unsafe { File::from_raw_fd(0) });
    f.read(buf)
}

#[cfg(not(unix))]
fn read_fd0(buf: &mut [u8]) -> io::Result<usize> {
    io::stdin().read(buf)
}

/// glibc's `BUFSIZ`-sized input buffer over a [`Source`].
pub struct Lines {
    src: Source,
    held: Vec<u8>,
    pos: usize,
    eof: bool,
    /// `_IO_ERR_SEEN`, with whether the error was `EAGAIN`.
    error: Option<bool>,
}

/// `EAGAIN`, on Linux and in the SlateOS C library.
const EAGAIN: i32 = 11;

impl Lines {
    pub fn new(src: Source) -> Self {
        Lines {
            src,
            held: Vec::new(),
            pos: 0,
            eof: false,
            error: None,
        }
    }

    /// The next buffered byte, reading more when the buffer is empty.
    /// `None` at end of file or on an error (which is recorded).
    fn next_byte(&mut self) -> Option<u8> {
        if self.pos >= self.held.len() {
            if self.eof {
                return None;
            }
            let mut chunk = vec![0u8; 4096];
            loop {
                match self.src.read(&mut chunk) {
                    Ok(0) => {
                        self.eof = true;
                        return None;
                    }
                    Ok(n) => {
                        chunk.truncate(n);
                        self.held = chunk;
                        self.pos = 0;
                        break;
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                    Err(e) => {
                        self.error = Some(e.raw_os_error() == Some(EAGAIN));
                        return None;
                    }
                }
            }
        }
        let b = self.held.get(self.pos).copied();
        self.pos = self.pos.saturating_add(1);
        b
    }

    /// `fgets(buf, buf.len(), stdin)`: the line's length, or `None` for
    /// `NULL`.
    pub fn fgets(&mut self, buf: &mut [u8]) -> Option<usize> {
        let n = buf.len();
        if n == 0 {
            return None;
        }
        if n == 1 {
            if let Some(b) = buf.first_mut() {
                *b = 0;
            }
            return Some(0);
        }
        let old = self.error.take();
        let mut count = 0usize;
        while count < n.saturating_sub(1) {
            let Some(b) = self.next_byte() else {
                break;
            };
            if let Some(slot) = buf.get_mut(count) {
                *slot = b;
            }
            count = count.saturating_add(1);
            if b == b'\n' {
                break;
            }
        }
        let failed = matches!(self.error, Some(false));
        let result = if count == 0 || failed {
            None
        } else {
            if let Some(slot) = buf.get_mut(count) {
                *slot = 0;
            }
            Some(count)
        };
        if self.error.is_none() {
            self.error = old;
        }
        result
    }
}
