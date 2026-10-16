//! The C library's `FILE`s as dc meets them.
//!
//! What dc writes and reads goes through stdio, and stdio's buffering is
//! visible: standard output is written in blocks, flushed where dc says
//! `fflush` (after each value printed) and not before a `!` command runs a
//! shell -- so a "... unimplemented" note, which dc writes to standard output
//! without a flush, comes out after the shell command's own output. Standard
//! input is read a block at a time, so a command run by `!` reads from where
//! dc's last block ended, not from the next command. Standard error has no
//! buffer. These are modelled here, glibc's way:
//!
//! - output is fully buffered in [`BUF_SIZE`] blocks, or line-buffered on a
//!   terminal; a write that fills the block flushes it;
//! - input is read [`BUF_SIZE`] bytes at a time; its end, once seen, stays
//!   seen (glibc since 2.28) until `ungetc` clears it;
//! - a failed write or read sets the stream's error flag (`ferror`) and
//!   keeps its `errno` for the `perror` that reports it.

/// glibc's buffer for a file or a pipe: `st_blksize`, which is 4096 on the
/// files and pipes dc meets.
pub const BUF_SIZE: usize = 4096;

/// Where an [`Output`] writes: a descriptor, or (for tests) memory.
#[derive(Debug)]
pub enum Sink {
    /// A file descriptor.
    Fd(i32),
    /// A vector, which a test reads back.
    Memory(Vec<u8>),
}

/// A `FILE` written to.
#[derive(Debug)]
pub struct Output {
    sink: Sink,
    buf: Vec<u8>,
    /// `_IONBF`: every write goes straight out (standard error).
    unbuffered: bool,
    /// `_IOLBF`: flushed after a write that holds a newline (a terminal).
    line_buffered: bool,
    error: bool,
    errno: i32,
}

impl Output {
    /// Standard output: line-buffered on a terminal, else fully buffered.
    #[must_use]
    pub fn stdout() -> Self {
        Self::new(Sink::Fd(1), false, libcall::fd::is_terminal(1))
    }

    /// Standard error: unbuffered.
    #[must_use]
    pub fn stderr() -> Self {
        Self::new(Sink::Fd(2), true, false)
    }

    /// An output to `sink`, buffered as the two flags say.
    #[must_use]
    pub fn new(sink: Sink, unbuffered: bool, line_buffered: bool) -> Self {
        Self {
            sink,
            buf: Vec::new(),
            unbuffered,
            line_buffered,
            error: false,
            errno: 0,
        }
    }

    /// `fwrite`, `putchar`, `fprintf`: `bytes` into the buffer, and out when
    /// the buffering says.
    pub fn write(&mut self, mut bytes: &[u8]) {
        if self.unbuffered {
            self.send(bytes);
            return;
        }
        let newline = bytes.contains(&b'\n');
        while !bytes.is_empty() {
            let room = BUF_SIZE.saturating_sub(self.buf.len());
            if bytes.len() <= room {
                self.buf.extend_from_slice(bytes);
                break;
            }
            let (now, rest) = bytes.split_at(room);
            self.buf.extend_from_slice(now);
            self.flush();
            bytes = rest;
        }
        if self.line_buffered && newline {
            self.flush();
        }
    }

    /// `fflush`: whether every buffered byte went out.
    pub fn flush(&mut self) -> bool {
        let pending = std::mem::take(&mut self.buf);
        if pending.is_empty() {
            return true;
        }
        self.send(&pending)
    }

    /// Write `bytes` out now; a failure sets the error flag, and the bytes
    /// are lost, as glibc loses them.
    fn send(&mut self, bytes: &[u8]) -> bool {
        match &mut self.sink {
            Sink::Memory(kept) => {
                kept.extend_from_slice(bytes);
                true
            }
            Sink::Fd(fd) => match libcall::fd::write_all(*fd, bytes, false) {
                Ok(()) => true,
                Err(e) => {
                    self.error = true;
                    self.errno = e;
                    false
                }
            },
        }
    }

    /// `ferror`.
    #[must_use]
    pub const fn error(&self) -> bool {
        self.error
    }

    /// The `errno` of the failure that set [`Output::error`].
    #[must_use]
    pub const fn errno(&self) -> i32 {
        self.errno
    }

    /// What has been written to a memory sink, buffered bytes included, and
    /// the sink emptied: for tests.
    pub fn take_written(&mut self) -> Vec<u8> {
        self.flush();
        match &mut self.sink {
            Sink::Memory(kept) => std::mem::take(kept),
            Sink::Fd(_) => Vec::new(),
        }
    }
}

/// Where an [`Input`] reads from: a descriptor, or (for tests) memory.
#[derive(Debug)]
pub enum Source {
    /// A file descriptor.
    Fd(i32),
    /// Bytes, read through from the front.
    Memory(Vec<u8>, usize),
}

/// A `FILE` read from.
#[derive(Debug)]
pub struct Input {
    source: Source,
    buf: Vec<u8>,
    at: usize,
    /// `ungetc`'s characters, last pushed first out.
    pushback: Vec<u8>,
    eof: bool,
    error: bool,
    errno: i32,
}

impl Input {
    /// Standard input.
    #[must_use]
    pub fn stdin() -> Self {
        Self::new(Source::Fd(0))
    }

    /// An input from `source`.
    #[must_use]
    pub const fn new(source: Source) -> Self {
        Self {
            source,
            buf: Vec::new(),
            at: 0,
            pushback: Vec::new(),
            eof: false,
            error: false,
            errno: 0,
        }
    }

    /// The descriptor it reads, if it reads one.
    #[must_use]
    pub const fn fd(&self) -> Option<i32> {
        match self.source {
            Source::Fd(fd) => Some(fd),
            Source::Memory(..) => None,
        }
    }

    /// `getc`: the next byte, or `None` (EOF) at the end or after an error.
    pub fn getc(&mut self) -> Option<u8> {
        if let Some(c) = self.pushback.pop() {
            return Some(c);
        }
        if let Some(&c) = self.buf.get(self.at) {
            self.at = self.at.saturating_add(1);
            return Some(c);
        }
        if self.eof || self.error {
            return None;
        }
        self.buf.resize(BUF_SIZE, 0);
        self.at = 0;
        let got = match &mut self.source {
            Source::Fd(fd) => libcall::fd::read(*fd, &mut self.buf),
            Source::Memory(bytes, at) => {
                let rest = bytes.get(*at..).unwrap_or_default();
                let n = rest.len().min(BUF_SIZE);
                if let (Some(dst), Some(src)) = (self.buf.get_mut(..n), rest.get(..n)) {
                    dst.copy_from_slice(src);
                }
                *at = at.saturating_add(n);
                Ok(n)
            }
        };
        match got {
            Ok(0) => {
                self.buf.clear();
                self.eof = true;
                None
            }
            Ok(n) => {
                self.buf.truncate(n);
                self.at = 1;
                self.buf.first().copied()
            }
            Err(e) => {
                self.buf.clear();
                self.error = true;
                self.errno = e;
                None
            }
        }
    }

    /// `ungetc (c)`: `c` read again next; `None` (EOF) is not pushed and
    /// changes nothing. Pushing clears the end, as `ungetc` does.
    pub fn ungetc(&mut self, c: Option<u8>) {
        if let Some(c) = c {
            self.pushback.push(c);
            self.eof = false;
        }
    }

    /// `ferror`.
    #[must_use]
    pub const fn error(&self) -> bool {
        self.error
    }

    /// The `errno` of the failure that set [`Input::error`].
    #[must_use]
    pub const fn errno(&self) -> i32 {
        self.errno
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn full_buffering_writes_a_block_when_it_fills() {
        let mut o = Output::new(Sink::Memory(Vec::new()), false, false);
        o.write(&[b'x'; BUF_SIZE - 1]);
        let Sink::Memory(kept) = &o.sink else {
            panic!()
        };
        assert!(kept.is_empty(), "nothing out until the block is full");
        o.write(b"ab");
        let Sink::Memory(kept) = &o.sink else {
            panic!()
        };
        assert_eq!(kept.len(), BUF_SIZE, "the block, and the rest kept");
        assert_eq!(o.take_written().len(), BUF_SIZE + 1, "and the rest on a flush");
    }

    #[test]
    fn a_line_buffered_output_goes_out_at_a_newline() {
        let mut o = Output::new(Sink::Memory(Vec::new()), false, true);
        o.write(b"no newline");
        let Sink::Memory(kept) = &o.sink else {
            panic!()
        };
        assert!(kept.is_empty());
        o.write(b" then\n");
        let Sink::Memory(kept) = &o.sink else {
            panic!()
        };
        assert_eq!(kept.as_slice(), b"no newline then\n");
    }

    #[test]
    fn input_ends_for_good_until_a_character_is_pushed_back() {
        let mut i = Input::new(Source::Memory(b"ab".to_vec(), 0));
        assert_eq!(i.getc(), Some(b'a'));
        i.ungetc(Some(b'z'));
        assert_eq!(i.getc(), Some(b'z'));
        assert_eq!(i.getc(), Some(b'b'));
        assert_eq!(i.getc(), None);
        assert_eq!(i.getc(), None);
        i.ungetc(None);
        assert_eq!(i.getc(), None, "pushing EOF pushes nothing");
        i.ungetc(Some(b'q'));
        assert_eq!(i.getc(), Some(b'q'));
        assert_eq!(i.getc(), None);
    }
}
