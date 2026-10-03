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
//! Reading is [`StdioReader`]: a block at a time (one byte unbuffered),
//! `getdelim`, a one-byte look-ahead, sticky end of file, and the seek back
//! over unread bytes that `exit` makes -- which is what decides where the
//! *next* reader of a shared standard input starts. See its docs.
//!
//! What it does not reproduce: seeking by the caller, wide streams, and the
//! line-buffered corner where a failed flush of a complete line is reported
//! to `fwrite`'s caller as success -- a terminal that rejects writes is not a
//! case anyone has measured.

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

    /// The file under a stream made by [`StdioFile::from_file`], while it is
    /// open -- `fileno`, for the `fchown` and `fchmod` a caller makes before
    /// closing it. `None` for a standard descriptor and after the close.
    #[must_use]
    pub fn file(&self) -> Option<&std::fs::File> {
        match &self.sink {
            Sink::File(f) => Some(f),
            _ => None,
        }
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

// ------------------------------------------------------------------ reading

/// How a reading stream fills its buffer, decided at its first read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ReadBuffering {
    /// Nothing has been read yet.
    Unallocated,
    /// A buffer of this size, filled by one `read(2)` at a time.
    Full(usize),
    /// `setvbuf (fp, NULL, _IONBF, 0)`: a one-byte buffer, so every `read(2)`
    /// asks for a single byte and nothing is taken that is not used.
    Unbuffered,
}

/// Where a reading stream's bytes come from.
enum Source {
    /// A standard descriptor, which this stream does not own.
    Descriptor(i32),
    /// A file the stream opened, and closes when it is dropped.
    File(std::fs::File),
    /// Bytes in memory: see [`MemorySource`].
    #[cfg(test)]
    Memory(MemorySource),
}

/// The test source: hands out `data` at most `chunk` bytes per read, then end
/// of file -- or the error `fail` -- and remembers the size of every read it
/// was asked for. `seekable` decides whether a seek back is `ESPIPE`.
#[cfg(test)]
struct MemorySource {
    data: Vec<u8>,
    at: usize,
    chunk: usize,
    fail: Option<i32>,
    seekable: bool,
    reads: Vec<usize>,
}

#[cfg(test)]
impl MemorySource {
    fn give(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reads.push(buf.len());
        let rest = self.data.get(self.at..).unwrap_or_default();
        if rest.is_empty() {
            return match self.fail {
                Some(errno) => Err(io::Error::from_raw_os_error(errno)),
                None => Ok(0),
            };
        }
        let n = rest.len().min(buf.len()).min(self.chunk);
        if let (Some(to), Some(from)) = (buf.get_mut(..n), rest.get(..n)) {
            to.copy_from_slice(from);
        }
        self.at = self.at.saturating_add(n);
        Ok(n)
    }

    fn seek_back(&mut self, n: usize) -> io::Result<()> {
        if !self.seekable {
            return Err(io::Error::from_raw_os_error(ESPIPE));
        }
        self.at = self.at.saturating_sub(n);
        Ok(())
    }
}

/// `ESPIPE`: what `lseek` answers on a pipe, which glibc's sync ignores.
const ESPIPE: i32 = 29;

/// One stdio `FILE` opened for reading: glibc's `getdelim`, `getc` with
/// `ungetc`, `feof`, `ferror`, `clearerr` and `setvbuf (_IONBF)`, and the
/// `lseek` that `exit` makes for a stream with unread bytes in its buffer.
///
/// What makes any of it visible is a descriptor shared with someone else --
/// standard input, inherited from a shell that hands it to the next program,
/// or handed on to a child. glibc reads a block at a time, so whatever it has
/// buffered and not used is gone from the descriptor; at `exit` it seeks back
/// over that much (`_IO_unbuffer_all`), so `{ sed 1q; cat; } < file` gives
/// `cat` the rest of the file -- but only for a stream that can seek, and only
/// one that is buffered: an unbuffered one holds at most the byte `getc`
/// looked at, and that byte is lost. All measured against GNU sed 4.9.
///
/// Not reproduced: glibc flushes a line-buffered standard output before
/// reading an unbuffered or line-buffered stream, which only a terminal on
/// both ends could show.
pub struct StdioReader {
    source: Source,
    buf: Vec<u8>,
    /// `_IO_read_ptr`: `buf[pos..]` has been read from the source and not yet
    /// handed out.
    pos: usize,
    buffering: ReadBuffering,
    /// `_IO_EOF_SEEN`: sticky until [`StdioReader::clear_error`].
    eof: bool,
    /// `_IO_ERR_SEEN`.
    error: bool,
    /// Whether anything has been read -- glibc's `_mode != 0`, which is what
    /// `exit` asks before syncing a stream.
    used: bool,
}

impl StdioReader {
    /// glibc's `stdin`.
    #[must_use]
    pub fn stdin() -> Self {
        Self::reading(Source::Descriptor(0))
    }

    /// A stream over a file opened for reading, as `fopen (name, "r")` makes
    /// it.
    #[must_use]
    pub fn from_file(file: std::fs::File) -> Self {
        Self::reading(Source::File(file))
    }

    fn reading(source: Source) -> Self {
        Self {
            source,
            buf: Vec::new(),
            pos: 0,
            buffering: ReadBuffering::Unallocated,
            eof: false,
            error: false,
            used: false,
        }
    }

    /// `setvbuf (fp, NULL, _IONBF, 0)`: one byte per `read(2)` from now on.
    ///
    /// Like glibc's, it syncs first -- giving anything already buffered back
    /// to a descriptor that can seek -- and then drops the buffer, so on one
    /// that cannot, what was buffered is lost.
    pub fn set_unbuffered(&mut self) {
        // Ignored, as `setvbuf`'s own sync is: there is nobody to tell.
        drop(self.sync());
        self.buf.clear();
        self.pos = 0;
        self.buffering = ReadBuffering::Unbuffered;
    }

    /// `clearerr`: forget an end of file and an error.
    pub fn clear_error(&mut self) {
        self.eof = false;
        self.error = false;
    }

    /// `feof`.
    #[must_use]
    pub fn at_eof(&self) -> bool {
        self.eof
    }

    /// `ferror`.
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.error
    }

    /// `__underflow`: make sure a byte is buffered, reading if none is.
    /// `Ok(false)` at the end of the file, which then stays ended.
    fn underflow(&mut self) -> io::Result<bool> {
        self.used = true;
        if self.pos < self.buf.len() {
            return Ok(true);
        }
        if self.eof {
            return Ok(false);
        }
        if self.buffering == ReadBuffering::Unallocated {
            self.buffering = ReadBuffering::Full(self.block_size());
        }
        let size = match self.buffering {
            ReadBuffering::Full(n) => n,
            ReadBuffering::Unallocated | ReadBuffering::Unbuffered => 1,
        };
        self.buf.clear();
        self.buf.resize(size, 0);
        self.pos = 0;
        let got = loop {
            let r = match &mut self.source {
                Source::Descriptor(fd) => stdfd::read(*fd, &mut self.buf),
                Source::File(f) => io::Read::read(f, &mut self.buf),
                #[cfg(test)]
                Source::Memory(m) => m.give(&mut self.buf),
            };
            match r {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                other => break other,
            }
        };
        match got {
            Ok(0) => {
                self.buf.clear();
                self.eof = true;
                Ok(false)
            }
            Ok(n) => {
                self.buf.truncate(n);
                Ok(true)
            }
            Err(e) => {
                self.buf.clear();
                self.error = true;
                Err(e)
            }
        }
    }

    /// `_IO_file_doallocate`'s size: `st_blksize` when it is positive and
    /// under `BUFSIZ`, `BUFSIZ` otherwise.
    fn block_size(&self) -> usize {
        let meta = match &self.source {
            Source::Descriptor(fd) => stdfd::metadata(*fd).ok(),
            Source::File(f) => f.metadata().ok(),
            #[cfg(test)]
            Source::Memory(m) => return m.chunk,
        };
        meta.as_ref()
            .map(block_size)
            .filter(|&b| b > 0 && b < BUFSIZ)
            .unwrap_or(BUFSIZ)
    }

    /// `getdelim`: the bytes up to and including the next `delim`, or to the
    /// end of the file, appended to `out`. `Ok(0)` is the end of the file with
    /// nothing read.
    ///
    /// # Errors
    ///
    /// The `read(2)` that failed, after which [`StdioReader::has_error`] is
    /// true. As with glibc, what was read before the failure is in `out`.
    pub fn read_until(&mut self, delim: u8, out: &mut Vec<u8>) -> io::Result<usize> {
        let start = out.len();
        if !self.underflow()? {
            return Ok(0);
        }
        loop {
            let avail = self.buf.get(self.pos..).unwrap_or_default();
            if let Some(i) = avail.iter().position(|&b| b == delim) {
                let end = i.saturating_add(1);
                out.extend_from_slice(avail.get(..end).unwrap_or_default());
                self.pos = self.pos.saturating_add(end);
                return Ok(out.len().saturating_sub(start));
            }
            out.extend_from_slice(avail);
            self.pos = self.buf.len();
            if !self.underflow()? {
                return Ok(out.len().saturating_sub(start));
            }
        }
    }

    /// `getc` then `ungetc`: the next byte, left where it is. `None` at the
    /// end of the file.
    ///
    /// # Errors
    ///
    /// As [`StdioReader::read_until`].
    pub fn peek(&mut self) -> io::Result<Option<u8>> {
        if !self.underflow()? {
            return Ok(None);
        }
        Ok(self.buf.get(self.pos).copied())
    }

    /// `rewind`: back to the start of the file, with the end of file and any
    /// error forgotten. A stream over something that cannot seek stays where
    /// it is, buffer and all, as glibc's failed seek leaves it.
    pub fn rewind(&mut self) {
        let sought = match &mut self.source {
            Source::File(f) => io::Seek::seek(f, io::SeekFrom::Start(0)).map(drop),
            // A standard descriptor is not this stream's to move to an
            // absolute position, and nothing asks it to be.
            Source::Descriptor(_) => Err(io::Error::from_raw_os_error(ESPIPE)),
            #[cfg(test)]
            Source::Memory(m) => m.seek_back(m.at),
        };
        if sought.is_ok() {
            self.buf.clear();
            self.pos = 0;
        }
        self.eof = false;
        self.error = false;
    }

    /// `fclose` of a stream that opened its file: the descriptor closed, and
    /// the close's failure -- which dropping it would discard -- returned. A
    /// standard descriptor is left open.
    ///
    /// # Errors
    ///
    /// Whatever `close(2)` reports.
    pub fn close(self) -> io::Result<()> {
        match self.source {
            Source::File(f) => stdfd::close(f),
            Source::Descriptor(_) => Ok(()),
            #[cfg(test)]
            Source::Memory(_) => Ok(()),
        }
    }

    /// What `exit` does to a stream nobody closed: glibc's
    /// `_IO_unbuffer_all` syncs every buffered stream that has been read,
    /// which seeks the descriptor back over what was buffered and not used.
    /// An unbuffered stream is passed over. Nothing is reported.
    pub fn exit_sync(&mut self) {
        if self.used && self.buffering != ReadBuffering::Unbuffered {
            // `exit` checks nothing.
            drop(self.sync());
        }
    }

    /// `_IO_new_file_sync` for a stream being read: seek back over the
    /// unread bytes and drop them. A descriptor that cannot seek keeps them.
    fn sync(&mut self) -> io::Result<()> {
        let unread = self.buf.len().saturating_sub(self.pos);
        if unread == 0 {
            return Ok(());
        }
        let back = i64::try_from(unread).unwrap_or(i64::MAX).saturating_neg();
        let sought = match &mut self.source {
            Source::Descriptor(fd) => stdfd::seek_current(*fd, back).map(drop),
            Source::File(f) => io::Seek::seek(f, io::SeekFrom::Current(back)).map(drop),
            #[cfg(test)]
            Source::Memory(m) => m.seek_back(unread),
        };
        match sought {
            Ok(()) => {
                self.buf.truncate(self.pos);
                Ok(())
            }
            Err(e) if e.raw_os_error() == Some(ESPIPE) => Ok(()),
            Err(e) => Err(e),
        }
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
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod tests {
    use super::{Buffering, Memory, MemorySource, Sink, Source, StdioFile, StdioReader};

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

    // ------------------------------------------------------------ reading

    /// A reader over `data`, handed out at most `chunk` bytes per `read(2)`
    /// -- which is also the block size it buffers by.
    fn source(data: &[u8], chunk: usize, seekable: bool) -> StdioReader {
        StdioReader::reading(Source::Memory(MemorySource {
            data: data.to_vec(),
            at: 0,
            chunk,
            fail: None,
            seekable,
            reads: Vec::new(),
        }))
    }

    fn mem(r: &mut StdioReader) -> &mut MemorySource {
        match &mut r.source {
            Source::Memory(m) => m,
            _ => panic!("not a memory source"),
        }
    }

    fn line(r: &mut StdioReader) -> Vec<u8> {
        let mut out = Vec::new();
        r.read_until(b'\n', &mut out).unwrap();
        out
    }

    #[test]
    fn lines_come_out_of_whole_blocks() {
        let mut r = source(b"ab\ncd\nef", 4, true);
        assert_eq!(line(&mut r), b"ab\n");
        assert_eq!(mem(&mut r).reads, vec![4]);
        // `cd\n` straddles two blocks; `ef` is the end of the file.
        assert_eq!(line(&mut r), b"cd\n");
        assert_eq!(line(&mut r), b"ef");
        assert_eq!(line(&mut r), b"");
        assert!(r.at_eof());
        assert_eq!(mem(&mut r).reads, vec![4, 4, 4]);
    }

    #[test]
    fn a_look_ahead_takes_nothing() {
        let mut r = source(b"ab\n", 8, true);
        assert_eq!(r.peek().unwrap(), Some(b'a'));
        assert_eq!(r.peek().unwrap(), Some(b'a'));
        assert_eq!(line(&mut r), b"ab\n");
        assert_eq!(r.peek().unwrap(), None);
        assert!(r.at_eof());
    }

    #[test]
    fn the_end_of_the_file_stays_until_it_is_cleared() {
        let mut r = source(b"a\n", 8, true);
        assert_eq!(line(&mut r), b"a\n");
        assert_eq!(line(&mut r), b"");
        // More arrives -- a terminal, or a file that grew -- but `feof` is
        // sticky: nothing is read until `clearerr`.
        mem(&mut r).data.extend_from_slice(b"b\n");
        assert_eq!(line(&mut r), b"");
        let reads = mem(&mut r).reads.len();
        assert_eq!(line(&mut r), b"");
        assert_eq!(mem(&mut r).reads.len(), reads, "a sticky end reads nothing");
        r.clear_error();
        assert_eq!(line(&mut r), b"b\n");
    }

    #[test]
    fn exit_gives_back_what_was_read_ahead() {
        // The `{ sed 1q; cat; } < file` case: one line used of a block read,
        // and the descriptor sought back to just after it.
        let mut r = source(b"1\n2\n3\n", 4096, true);
        assert_eq!(line(&mut r), b"1\n");
        assert_eq!(mem(&mut r).at, 6);
        r.exit_sync();
        assert_eq!(mem(&mut r).at, 2);
    }

    #[test]
    fn exit_gives_nothing_back_to_a_pipe_and_does_not_complain() {
        let mut r = source(b"1\n2\n3\n", 4096, false);
        assert_eq!(line(&mut r), b"1\n");
        r.exit_sync();
        assert_eq!(mem(&mut r).at, 6);
        // ...and an unused stream is not touched at all.
        let mut unused = source(b"1\n", 4096, true);
        unused.exit_sync();
        assert!(mem(&mut unused).reads.is_empty());
    }

    #[test]
    fn unbuffered_reads_one_byte_at_a_time_and_exit_leaves_the_look_ahead() {
        let mut r = source(b"1\n2\n", 4096, true);
        r.set_unbuffered();
        assert_eq!(line(&mut r), b"1\n");
        assert_eq!(mem(&mut r).reads, vec![1, 1]);
        assert_eq!(mem(&mut r).at, 2);
        // A look-ahead takes one more byte, and `exit` does not give it back:
        // glibc skips unbuffered streams. `{ sed -u '$!q'; cat; } < f` loses
        // the `2` this way -- measured.
        assert_eq!(r.peek().unwrap(), Some(b'2'));
        r.exit_sync();
        assert_eq!(mem(&mut r).at, 3);
    }

    #[test]
    fn going_unbuffered_gives_back_a_block_already_read() {
        let mut r = source(b"1\n2\n", 4096, true);
        assert_eq!(r.peek().unwrap(), Some(b'1'));
        assert_eq!(mem(&mut r).at, 4);
        r.set_unbuffered();
        assert_eq!(mem(&mut r).at, 0);
        assert_eq!(line(&mut r), b"1\n");
    }

    #[test]
    fn a_read_error_is_reported_and_remembered() {
        let mut r = source(b"ab", 8, true);
        mem(&mut r).fail = Some(5);
        let mut out = Vec::new();
        let e = r.read_until(b'\n', &mut out).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(5));
        // What came before the failure is kept, as glibc's `getdelim` keeps it.
        assert_eq!(out, b"ab");
        assert!(r.has_error());
        r.clear_error();
        assert!(!r.has_error());
    }
}
