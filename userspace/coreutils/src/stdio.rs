//! glibc's stdio `FILE`, as far as a port needs its buffer arithmetic.
//!
//! [`crate::stdfd::Stream`] is the writer most utilities here want: it records
//! a failure and reports it once, at the end, which is all gnulib's
//! `close_stdout` asks. Some upstreams ask more of stdio than that. GNU sed
//! checks *every* `fwrite` and names the one that failed --
//! `couldn't write 3 items to stdout: No space left on device` -- so which
//! call fails, and with how many bytes in hand, is part of its output. That
//! depends on exactly where glibc's buffer fills and flushes, which this module
//! reproduces:
//!
//! * **The buffer is the file's block size**, chosen at the first write the
//!   way `_IO_file_doallocate` chooses it: `st_blksize` when it is positive and
//!   below `BUFSIZ` (8192), `BUFSIZ` otherwise -- including when `fstat` fails.
//!   A terminal is line-buffered.
//! * **A write is `_IO_new_file_xsputn`**: what fits is copied; if anything is
//!   left, the full buffer is written out, then as many whole buffers' worth
//!   of the rest as there are are written directly, and the remainder starts
//!   the next buffer. A write that exactly fills the buffer does not flush it;
//!   the next write does. Where the buffer is under 128 bytes, glibc does not
//!   bother keeping the writes aligned, and all of the rest goes directly.
//! * **The first write finds no buffer at all**, so nothing "fits": the
//!   buffer is allocated empty and the write goes straight to the second step.
//!   A first write of a whole block or more therefore reaches the descriptor at
//!   once, where a later one of the same size would only fill the buffer --
//!   which decides which of GNU sed's writes is the one that fails on a full
//!   disk. Measured: `sed -u '1e head -c 5000 big'` to `/dev/full` fails on
//!   its first 4096-byte piece, not its second.
//! * **A failed flush discards the buffer**, as `new_do_write` resets the
//!   pointers whether or not `write(2)` succeeded, and sets the error flag
//!   (`ferror`) until [`StdioFile::clear_error`] (`clearerr`).
//!
//! What it does not reproduce: reading, seeking, `ungetc`, wide streams, and
//! the line-buffered corner where a failed flush of a complete line is
//! reported to `fwrite`'s caller as success -- a terminal that rejects writes
//! is not a case anyone has measured.

use std::io;

use crate::stdfd;

/// glibc's `BUFSIZ`, the most a `FILE` buffer is unless the file's block size
/// is smaller.
const BUFSIZ: usize = 8192;

/// `EBADF`, what a closed stream answers.
const EBADF: i32 = 9;

/// How a stream holds its output, decided when its buffer is allocated.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Buffering {
    /// Not yet decided: nothing has been written.
    Unallocated,
    /// Held until the buffer is full.
    Full(usize),
    /// Held until a newline, or until the buffer is full.
    Line(usize),
    /// Written as it comes. stdio's `stderr`.
    Unbuffered,
}

/// Where a stream's bytes go.
enum Sink {
    /// A standard descriptor, which this stream does not own.
    Descriptor(i32),
    /// A file the stream opened, and closes.
    File(std::fs::File),
    /// Closed by [`StdioFile::close`]: every later write is `EBADF`.
    Closed,
    /// A disk of a given size, recording every `write(2)` made to it.
    #[cfg(test)]
    Memory(Memory),
}

/// The test sink: takes `room` bytes, then answers `ENOSPC`, and remembers
/// the size of every write it was asked for. `block` is its `st_blksize`,
/// what a stream onto it chooses as its buffer size.
#[cfg(test)]
struct Memory {
    room: usize,
    block: usize,
    writes: Vec<usize>,
}

#[cfg(test)]
impl Memory {
    fn accept(&mut self, data: &[u8]) -> io::Result<usize> {
        self.writes.push(data.len());
        if self.room == 0 {
            return Err(io::Error::from_raw_os_error(28));
        }
        let n = data.len().min(self.room);
        self.room = self.room.saturating_sub(n);
        Ok(n)
    }
}

/// One stdio `FILE` opened for writing. See the module docs.
pub struct StdioFile {
    sink: Sink,
    buf: Vec<u8>,
    buffering: Buffering,
    /// `ferror`: a write failed and nothing has cleared it since.
    error: bool,
}

impl StdioFile {
    /// glibc's `stdout`: buffered as its descriptor decides at the first
    /// write.
    #[must_use]
    pub fn stdout() -> Self {
        Self::new(Sink::Descriptor(1), Buffering::Unallocated)
    }

    /// glibc's `stderr`: unbuffered.
    #[must_use]
    pub fn stderr() -> Self {
        Self::new(Sink::Descriptor(2), Buffering::Unbuffered)
    }

    /// A stream over a file opened for writing, as `fopen` would make it.
    #[must_use]
    pub fn from_file(file: std::fs::File) -> Self {
        Self::new(Sink::File(file), Buffering::Unallocated)
    }

    fn new(sink: Sink, buffering: Buffering) -> Self {
        Self {
            sink,
            buf: Vec::new(),
            buffering,
            error: false,
        }
    }

    /// `ferror`.
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.error
    }

    /// `clearerr`.
    pub fn clear_error(&mut self) {
        self.error = false;
    }

    /// The bytes held, not yet written. For tests and for `__fpending`.
    #[must_use]
    pub fn pending(&self) -> usize {
        self.buf.len()
    }

    /// `fwrite (data, 1, data.len (), fp)`: every byte accepted, or the error
    /// of the `write(2)` that stopped short.
    ///
    /// "Accepted" is stdio's sense -- in the buffer or on the descriptor; a
    /// buffered byte that later fails to go out is the business of the next
    /// write or of [`StdioFile::flush`].
    ///
    /// # Errors
    ///
    /// The failed `write(2)`'s error, after which the buffer is empty and
    /// [`StdioFile::has_error`] is true.
    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        // No buffer yet means no room in one: see the module docs.
        let first = self.buffering == Buffering::Unallocated;
        if first {
            self.buffering = self.choose_buffering();
        }
        match self.buffering {
            Buffering::Unbuffered => self.write_out(data),
            Buffering::Line(size) => self.write_line_buffered(data, size, first),
            Buffering::Full(size) => self.write_fully_buffered(data, size, first),
            // Just chosen above, so never still unallocated; `BUFSIZ` is what
            // it would have been chosen as had nothing been known.
            Buffering::Unallocated => self.write_fully_buffered(data, BUFSIZ, first),
        }
    }

    /// `fflush`: write out whatever is held.
    ///
    /// Nothing held is success whatever the descriptor is -- glibc does not
    /// look at it -- which is why a closed standard output with nothing
    /// written flushes cleanly and only fails at the close.
    ///
    /// # Errors
    ///
    /// The failed `write(2)`'s error; the buffer is discarded either way.
    pub fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let held = std::mem::take(&mut self.buf);
        let result = self.write_out(&held);
        self.buf = held;
        self.buf.clear();
        result
    }

    /// `fclose`: flush, then close the descriptor.
    ///
    /// A standard descriptor is closed too, as `fclose (stdout)` closes 1 --
    /// that close is what reports a descriptor that was never open. Call it
    /// last, on the way out. The stream is left closed, and anything written
    /// to it afterwards fails with `EBADF`.
    ///
    /// # Errors
    ///
    /// The flush's error, or else the close's.
    pub fn close(&mut self) -> io::Result<()> {
        let flushed = self.flush();
        let closed = match std::mem::replace(&mut self.sink, Sink::Closed) {
            Sink::Descriptor(fd) => stdfd::close_descriptor(fd),
            Sink::File(file) => stdfd::close(file),
            Sink::Closed => Err(io::Error::from_raw_os_error(EBADF)),
            #[cfg(test)]
            Sink::Memory(_) => Ok(()),
        };
        flushed.and(closed)
    }

    /// `_IO_file_doallocate`'s choice, made on the first write.
    fn choose_buffering(&self) -> Buffering {
        #[cfg(test)]
        if let Sink::Memory(m) = &self.sink {
            return Buffering::Full(m.block);
        }
        let meta = match &self.sink {
            Sink::Descriptor(fd) => stdfd::metadata(*fd).ok(),
            Sink::File(file) => file.metadata().ok(),
            Sink::Closed => None,
            #[cfg(test)]
            Sink::Memory(_) => None,
        };
        let size = meta
            .as_ref()
            .map(block_size)
            .filter(|&b| b > 0 && b < BUFSIZ)
            .unwrap_or(BUFSIZ);
        let tty = match &self.sink {
            Sink::Descriptor(fd) => meta.as_ref().is_some_and(is_char_device) && stdfd::is_tty(*fd),
            Sink::File(_) | Sink::Closed => false,
            #[cfg(test)]
            Sink::Memory(_) => false,
        };
        if tty {
            Buffering::Line(size)
        } else {
            Buffering::Full(size)
        }
    }

    /// `_IO_new_file_xsputn` for a fully buffered stream. `first` is the write
    /// that allocates the buffer, which finds no room in it.
    fn write_fully_buffered(&mut self, data: &[u8], size: usize, first: bool) -> io::Result<()> {
        let room = if first {
            0
        } else {
            size.saturating_sub(self.buf.len())
        };
        let take = room.min(data.len());
        let (head, rest) = data.split_at(take);
        self.buf.extend_from_slice(head);
        if rest.is_empty() {
            return Ok(());
        }
        // The buffer is full and there is more: out it goes. (On the first
        // write it is empty, and this writes nothing.)
        self.flush()?;
        // Whole buffers' worth of what is left go straight to the descriptor,
        // keeping the writes block-aligned; the remainder starts a new buffer.
        // A buffer under 128 bytes is not worth aligning to, and glibc sends
        // everything.
        let direct = if size >= 128 {
            rest.len()
                .saturating_sub(rest.len().checked_rem(size).unwrap_or(0))
        } else {
            rest.len()
        };
        let (whole, tail) = rest.split_at(direct);
        if !whole.is_empty() {
            self.write_out(whole)?;
        }
        self.buf.extend_from_slice(tail);
        Ok(())
    }

    /// A line-buffered stream: held until a newline, then written through it.
    fn write_line_buffered(&mut self, data: &[u8], size: usize, first: bool) -> io::Result<()> {
        match data.iter().rposition(|&b| b == b'\n') {
            Some(end) => {
                let (through, rest) = data.split_at(end.saturating_add(1));
                self.buf.extend_from_slice(through);
                self.flush()?;
                self.write_fully_buffered(rest, size, false)
            }
            None => self.write_fully_buffered(data, size, first),
        }
    }

    /// `write(2)` until done, the error flag set on failure.
    fn write_out(&mut self, data: &[u8]) -> io::Result<()> {
        let mut rest = data;
        while !rest.is_empty() {
            let wrote = match &mut self.sink {
                Sink::Descriptor(fd) => stdfd::write_some(*fd, rest),
                Sink::File(file) => io::Write::write(file, rest),
                Sink::Closed => Err(io::Error::from_raw_os_error(EBADF)),
                #[cfg(test)]
                Sink::Memory(mem) => mem.accept(rest),
            };
            match wrote {
                Ok(0) => {
                    self.error = true;
                    return Err(io::Error::from(io::ErrorKind::WriteZero));
                }
                Ok(n) => rest = rest.get(n..).unwrap_or_default(),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => {
                    self.error = true;
                    return Err(e);
                }
            }
        }
        Ok(())
    }
}

/// `st_blksize`.
#[cfg(unix)]
fn block_size(meta: &std::fs::Metadata) -> usize {
    use std::os::unix::fs::MetadataExt;
    usize::try_from(meta.blksize()).unwrap_or(BUFSIZ)
}

/// No block size to ask for off unix: glibc's default.
#[cfg(not(unix))]
fn block_size(_meta: &std::fs::Metadata) -> usize {
    BUFSIZ
}

/// `S_ISCHR`, which glibc asks before it asks `isatty`.
#[cfg(unix)]
fn is_char_device(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::FileTypeExt;
    meta.file_type().is_char_device()
}

#[cfg(not(unix))]
fn is_char_device(_meta: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::{Buffering, Memory, Sink, StdioFile};

    fn memory(block: usize, room: usize) -> Sink {
        Sink::Memory(Memory {
            room,
            block,
            writes: Vec::new(),
        })
    }

    /// A stream whose `size`-byte buffer already exists -- something has been
    /// written and flushed -- onto a disk with `room` bytes free.
    fn disk(size: usize, room: usize) -> StdioFile {
        StdioFile::new(memory(size, room), Buffering::Full(size))
    }

    /// A stream nothing has been written to yet, onto a disk whose block size
    /// is `block`: its buffer is allocated by the first write.
    fn fresh(block: usize, room: usize) -> StdioFile {
        StdioFile::new(memory(block, room), Buffering::Unallocated)
    }

    /// The `write(2)` calls the stream has made, by size.
    fn writes(f: &StdioFile) -> Vec<usize> {
        match &f.sink {
            Sink::Memory(m) => m.writes.clone(),
            _ => Vec::new(),
        }
    }

    #[test]
    fn what_fits_is_held_and_a_full_buffer_waits_for_the_next_write() {
        let mut f = disk(128, 1000);
        f.write(&[b'a'; 100]).unwrap();
        assert_eq!((f.pending(), writes(&f)), (100, vec![]));
        // Exactly full: still held. glibc flushes only when a byte needs room.
        f.write(&[b'b'; 28]).unwrap();
        assert_eq!((f.pending(), writes(&f)), (128, vec![]));
        f.write(b"c").unwrap();
        assert_eq!((f.pending(), writes(&f)), (1, vec![128]));
    }

    #[test]
    fn a_long_write_goes_out_in_whole_buffers_and_keeps_the_remainder() {
        let mut f = disk(128, 10_000);
        f.write(&[b'a'; 4]).unwrap();
        // 124 more fill the buffer, which goes; 256 of the remaining 276 go
        // directly, block-aligned; 20 start the next buffer.
        f.write(&[b'b'; 400]).unwrap();
        assert_eq!((f.pending(), writes(&f)), (20, vec![128, 256]));
    }

    #[test]
    fn the_first_write_finds_no_room_and_sends_its_whole_blocks_at_once() {
        // Nothing fits in a buffer that does not exist yet, so the 256 bytes
        // of whole blocks go straight out and only the 44 left over are held.
        let mut f = fresh(128, 10_000);
        f.write(&[b'a'; 300]).unwrap();
        assert_eq!((f.pending(), writes(&f)), (44, vec![256]));
        // Less than a block is held, first write or not...
        let mut g = fresh(128, 10_000);
        g.write(&[b'a'; 100]).unwrap();
        assert_eq!((g.pending(), writes(&g)), (100, vec![]));
        // ...and from then on the buffer exists and fills first.
        g.write(&[b'b'; 300]).unwrap();
        assert_eq!((g.pending(), writes(&g)), (16, vec![128, 256]));
    }

    #[test]
    fn a_first_write_of_a_whole_block_onto_a_full_disk_fails_itself() {
        // The case GNU sed's messages expose: the first 4096-byte piece of an
        // `e` command's output is the write that fails, not the one after it.
        let mut f = fresh(128, 0);
        let e = f.write(&[b'a'; 128]).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(28));
        assert_eq!(writes(&f), vec![128]);
        // Where the buffer already exists, the same write only fills it.
        let mut g = disk(128, 0);
        g.write(&[b'a'; 128]).unwrap();
        assert_eq!((g.pending(), writes(&g)), (128, vec![]));
    }

    #[test]
    fn a_buffer_under_128_bytes_is_not_kept_aligned() {
        let mut f = disk(16, 1000);
        f.write(&[b'a'; 4]).unwrap();
        // 12 fill the buffer, which goes, and then all 28 of the rest: glibc
        // keeps whole-block writes only for a buffer of at least 128.
        f.write(&[b'b'; 40]).unwrap();
        assert_eq!((f.pending(), writes(&f)), (0, vec![16, 28]));
    }

    #[test]
    fn a_failed_flush_reports_the_write_that_needed_it_and_empties_the_buffer() {
        let mut f = disk(128, 10);
        f.write(&[b'a'; 120]).unwrap();
        // Room for 8 more fills the buffer; the 128 that must go then meet a
        // disk with room for 10.
        let e = f.write(&[b'b'; 16]).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(28));
        assert!(f.has_error());
        assert_eq!(f.pending(), 0);
        f.clear_error();
        assert!(!f.has_error());
    }

    #[test]
    fn nothing_held_flushes_cleanly_even_onto_a_full_disk() {
        let mut f = disk(128, 0);
        f.flush().unwrap();
        f.write(b"x").unwrap();
        assert!(f.flush().is_err());
        // And the failed flush took the byte with it.
        assert_eq!(f.pending(), 0);
        f.flush().unwrap();
    }
}
