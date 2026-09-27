//! util-linux's `include/closestream.h`: standard output as glibc's stdio
//! holds it, diagnostics as `warn`/`warnx` write them, and `close_stdout`'s
//! verdict at the end.
//!
//! Every util-linux program registers `close_stdout` with `atexit`, and what
//! a caller can see of it depends on details of stdio that Rust's own
//! `stdout` does not have:
//!
//! * **Whether a write was attempted before the end.** glibc holds output in
//!   a buffer of `st_blksize` bytes when stdout is not a terminal -- 4096
//!   for a pipe, a file or `/dev/full`, and `BUFSIZ`, 8192, on a closed
//!   descriptor, which `fstat` cannot size -- and line by line when it is.
//!   `lsmem >
//!   /dev/full` prints `lsmem: write error: No space left on device`: nothing
//!   reached the descriptor until the final flush, whose `errno` is
//!   reported. `lsmem -a > /dev/full` prints `lsmem: write error` with no
//!   reason: its output outgrew the buffer, a write failed on the way, and
//!   `close_stdout` finds only the stream's error flag, with `errno` reset.
//! * **A closed stdout is forgiven at the end.** The final flush failing
//!   with `EBADF` is not an error to `flush_standard_stream`, so `lsmem >&-`
//!   succeeds silently -- while `lsmem -a >&-`, whose output was written (and
//!   failed) before the end, is a write error. Seeing the closed descriptor
//!   at all needs `stdfdguard`, since the runtime reopens it on `/dev/null`
//!   before `main`; and writes go to descriptors 1 and 2 directly, because
//!   Rust's `Stdout` and `Stderr` turn `EBADF` into success.
//! * **A lost diagnostic decides the status.** `close_stdout` then flushes
//!   stderr, and if a write to it failed -- `2>/dev/full`, or `2>&-` with a
//!   message to print -- exits with `CLOSE_EXIT_CODE`, silently, whatever the
//!   program was about to return.
//!
//! Measured against util-linux 2.39.3 through `lsmem` (`scripts/lsmem-diff.sh`).
//! This is util-linux's rule, not gnulib's: coreutils' `stdfd` implements
//! gnulib's `close_stdout`, which reports a closed stdout that still held
//! output where util-linux forgives it.
//!
//! One convention is the tree's rather than upstream's: a reader that went
//! away (`EPIPE`) is never reported and never changes the status. Upstream
//! dies of `SIGPIPE` at the first such write; SlateOS does not send it, and
//! the tree's answer to that case everywhere is silence (coreutils'
//! `stdfd::reader_gone`).

use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};

/// glibc's `BUFSIZ`: the buffer when `fstat` cannot say better.
const BUFSIZ: usize = 8192;

/// `ferror(stderr)`: a diagnostic could not be written. Process-global, as
/// stderr is.
static STDERR_FAILED: AtomicBool = AtomicBool::new(false);

/// How glibc buffers a stream: chosen by `_IO_file_doallocate` when the
/// stream is first written to, from what descriptor 1 is at that moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Buffering {
    /// A terminal: out at each newline.
    Line,
    /// Anything else: out when this many bytes are exceeded.
    Full(usize),
}

/// Standard output, buffered as glibc buffers it.
pub struct Stdout {
    /// Chosen at the first write, not before: a program that opens a file
    /// with its stdout closed has that file on descriptor 1 by then, and
    /// glibc sizes the buffer by *its* `fstat` -- upstream lsmem's `/sys`
    /// directory, 4096, where a closed descriptor would have given 8192.
    buffering: Option<Buffering>,
    /// Written, not yet flushed.
    held: Vec<u8>,
    /// How much of the buffer `held` fills: its bytes, or on a wide stream
    /// its characters.
    held_units: usize,
    /// `fwide(stdout, 1)`: written with `fputws`/`putwchar`, whose buffer
    /// holds characters (see [`Stdout::orient_wide`]).
    wide: bool,
    /// `ferror(stdout)`: a flush before the end failed, and why.
    failed: Option<io::Error>,
    /// The program's `CLOSE_EXIT_CODE`: `EXIT_FAILURE` unless it defines its
    /// own (`getopt`'s is 3).
    close_exit_code: u8,
}

impl Stdout {
    /// stdout as the program starts with it. Call `stdfdguard::restore`
    /// first, or a closed stdout reads as `/dev/null`.
    #[must_use]
    pub fn new(close_exit_code: u8) -> Self {
        Stdout {
            buffering: None,
            held: Vec::new(),
            held_units: 0,
            wide: false,
            failed: None,
            close_exit_code,
        }
    }

    /// Make stdout a wide stream, as the first `fputws` or `putwchar` does:
    /// glibc then buffers `wchar_t`s, as many as its byte buffer holds
    /// bytes, and converts them only when that buffer overflows -- so text
    /// that is not ASCII reaches the descriptor later than the same bytes
    /// written with `fputs` would. What is written must be UTF-8 (ASCII in
    /// the C locale), each character counted once.
    pub fn orient_wide(&mut self) {
        self.wide = true;
        self.held_units = self.units(&self.held);
    }

    /// How much of the buffer `bytes` take.
    fn units(&self, bytes: &[u8]) -> usize {
        if self.wide {
            // A UTF-8 character is one byte that does not continue another.
            bytes.iter().filter(|&&b| b & 0xc0 != 0x80).count()
        } else {
            bytes.len()
        }
    }

    /// What `exec` does to the buffer: drops it, unwritten. A program that
    /// replaces itself (`flock -F`) loses what it had printed and not yet
    /// flushed, as upstream's does.
    pub fn discard(&mut self) {
        self.held.clear();
        self.held_units = 0;
    }

    /// `fputs`, `printf`: into the buffer, and out of it when glibc would
    /// write -- at a newline on a terminal, when the buffer overflows
    /// otherwise.
    pub fn write(&mut self, data: &[u8]) {
        self.held.extend_from_slice(data);
        self.held_units = self.held_units.saturating_add(self.units(data));
        let buffering = *self.buffering.get_or_insert_with(allocate);
        let due = match buffering {
            Buffering::Line => self
                .held
                .iter()
                .rposition(|&b| b == b'\n')
                .map(|nl| nl.saturating_add(1)),
            Buffering::Full(size) => (self.held_units > size).then_some(self.held.len()),
        };
        if let Some(n) = due {
            let chunk: Vec<u8> = self.held.drain(..n).collect();
            self.held_units = self.units(&self.held);
            if let Err(e) = sys::write_all(sys::STDOUT, &chunk)
                && self.failed.is_none()
            {
                // glibc drops what it could not write and sets the flag.
                self.failed = Some(e);
            }
        }
    }

    /// `fflush(stdout)`: what is held, written now. A failure sets the
    /// stream's error flag, as glibc's does, for `close_stdout` to report.
    pub fn flush(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let chunk = std::mem::take(&mut self.held);
        self.held_units = 0;
        if let Err(e) = sys::write_all(sys::STDOUT, &chunk)
            && self.failed.is_none()
        {
            // glibc drops what it could not write and sets the flag.
            self.failed = Some(e);
        }
    }

    /// `exit` in a program that never registers `close_stdout` (`lsirq`):
    /// glibc's own flush of what is held, whose failure nobody hears of --
    /// no message, and the status is the program's.
    pub fn flush_at_exit(mut self) {
        let held = std::mem::take(&mut self.held);
        // Unreported by glibc's exit, and so here.
        let _ = sys::write_all(sys::STDOUT, &held);
    }

    /// `close_stdout`: flush what is held, report a failure as util-linux
    /// does, then judge stderr. Returns the status to exit with -- `status`,
    /// or `CLOSE_EXIT_CODE`.
    pub fn close(mut self, status: u8, short: &[u8]) -> u8 {
        let earlier = self.failed.take();
        // `ferror(stdout) || fflush(stdout)`: with the flag set, nothing more
        // is written.
        let at_close = if earlier.is_some() {
            Ok(())
        } else {
            let held = std::mem::take(&mut self.held);
            sys::write_all(sys::STDOUT, &held).and_then(|()| sys::dup_close(sys::STDOUT))
        };
        let stdout = judge(earlier, at_close);
        match &stdout {
            Outcome::Fine => {}
            Outcome::FailedBefore => warnx(short, "write error"),
            Outcome::FailedAtClose(e) => warn(short, "write error", e),
        }
        verdict(
            &stdout,
            STDERR_FAILED.load(Ordering::Relaxed),
            status,
            self.close_exit_code,
        )
    }
}

/// What `close_stdout` makes of standard output.
#[derive(Debug)]
enum Outcome {
    /// Written -- or forgiven: a closed stdout at the final flush, a reader
    /// that went away.
    Fine,
    /// A write before the end failed: `flush_standard_stream` finds only the
    /// error flag, with errno reset, and `close_stdout` says `write error`.
    FailedBefore,
    /// The final flush failed: `write error: REASON`.
    FailedAtClose(io::Error),
}

/// `flush_standard_stream(stdout)` as `close_stdout` reads it, from the
/// failure of a write before the end, if any, and the final flush's result.
fn judge(earlier: Option<io::Error>, at_close: io::Result<()>) -> Outcome {
    if let Some(e) = earlier {
        return if e.kind() == io::ErrorKind::BrokenPipe {
            Outcome::Fine
        } else {
            Outcome::FailedBefore
        };
    }
    match at_close {
        Ok(()) => Outcome::Fine,
        Err(e) if is_ebadf(&e) || e.kind() == io::ErrorKind::BrokenPipe => Outcome::Fine,
        Err(e) => Outcome::FailedAtClose(e),
    }
}

/// The status to exit with: `CLOSE_EXIT_CODE` for stdout's failure, then for
/// a lost diagnostic (`flush_standard_stream(stderr)`), else the program's.
fn verdict(stdout: &Outcome, stderr_failed: bool, status: u8, close_exit_code: u8) -> u8 {
    match stdout {
        Outcome::FailedBefore | Outcome::FailedAtClose(_) => close_exit_code,
        Outcome::Fine if stderr_failed => close_exit_code,
        Outcome::Fine => status,
    }
}

/// `EBADF`, on Linux and in the SlateOS C library.
const EBADF: i32 = 9;

/// Whether a failure is `EBADF`. On the Windows host `std` never reports it
/// for a standard stream, so this is only ever true where it is meant.
fn is_ebadf(e: &io::Error) -> bool {
    e.raw_os_error() == Some(EBADF)
}

/// `_IO_file_doallocate`: a terminal is line buffered; anything else gets a
/// buffer [`buffer_size`] bytes long.
fn allocate() -> Buffering {
    if io::stdout().is_terminal() {
        Buffering::Line
    } else {
        Buffering::Full(buffer_size(sys::block_size(sys::STDOUT)))
    }
}

/// `_IO_file_doallocate`'s size: `st_blksize` when `fstat` gives one below
/// `BUFSIZ` -- 4096 for a pipe, a file or `/dev/full` -- and `BUFSIZ` when it
/// gives none or `fstat` fails, which it does on a closed descriptor. So
/// `getopt` with 6 KB of output to a closed stdout never writes before the
/// end, and its final `EBADF` is forgiven, where the same output to
/// `/dev/full` fails on the way.
fn buffer_size(block: Option<u64>) -> usize {
    match block.and_then(|b| usize::try_from(b).ok()) {
        Some(b) if b > 0 && b < BUFSIZ => b,
        _ => BUFSIZ,
    }
}

/// Bytes to stderr, unbuffered as glibc's stderr is; a failure is recorded
/// for [`Stdout::close`] to find.
pub fn stderr_write(data: &[u8]) {
    if sys::write_all(sys::STDERR, data).is_err() {
        STDERR_FAILED.store(true, Ordering::Relaxed);
    }
}

/// `write(STDERR_FILENO, ...)` -- or `writev` -- bypassing stdio, as `logger
/// -s` copies each message to stderr: a failure sets no stream's error flag,
/// so it never reaches `close_stdout`'s verdict. The result is the caller's
/// to ignore, as upstream's `ignore_result` does.
///
/// # Errors
///
/// The write's own failure.
pub fn stderr_raw(data: &[u8]) -> io::Result<()> {
    sys::write_all(sys::STDERR, data)
}

/// `warnx(msg)`: `NAME: MSG`, the name with its unprintable bytes escaped
/// (design-decisions §370).
pub fn warnx(short: &[u8], msg: &str) {
    stderr_write(format!("{}: {msg}\n", quoting::escape_unprintable(short)).as_bytes());
}

/// `warn(msg)`: `NAME: MSG: strerror(errno)`.
pub fn warn(short: &[u8], msg: &str, e: &io::Error) {
    warnx(short, &format!("{msg}: {}", errmsg::strerror(e)));
}

/// Whether a diagnostic has failed to write so far -- `ferror(stderr)`.
#[must_use]
pub fn stderr_failed() -> bool {
    STDERR_FAILED.load(Ordering::Relaxed)
}

#[cfg(unix)]
mod sys {
    use std::ffi::{c_int, c_void};
    use std::io;

    mod ffi {
        use std::ffi::{c_int, c_void};

        unsafe extern "C" {
            pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
            pub fn dup(fd: c_int) -> c_int;
            pub fn close(fd: c_int) -> c_int;
        }
    }

    pub const STDOUT: c_int = 1;
    pub const STDERR: c_int = 2;

    /// `write(fd, ...)` until all of `data` is out.
    pub fn write_all(fd: c_int, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            // SAFETY: the pointer and length describe `data`, which is valid
            // for reads of that many bytes for the whole call; `write` only
            // reads through it. A standard descriptor needs no ownership to
            // be named: a closed one fails with EBADF, which is what is
            // wanted.
            let n = unsafe { ffi::write(fd, data.as_ptr().cast::<c_void>(), data.len()) };
            match usize::try_from(n) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::WriteZero)),
                Ok(n) => data = data.get(n..).unwrap_or_default(),
                Err(_) => {
                    let e = io::Error::last_os_error();
                    if e.kind() != io::ErrorKind::Interrupted {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

    /// `close(dup(fd))`: what `flush_standard_stream` does to catch an error
    /// a filesystem reports only on close.
    pub fn dup_close(fd: c_int) -> io::Result<()> {
        // SAFETY: `dup` takes a descriptor number and touches no memory.
        let copy = unsafe { ffi::dup(fd) };
        if copy < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `copy` is the descriptor `dup` just made, owned by nobody
        // else, and closed exactly once, here.
        if unsafe { ffi::close(copy) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// `fstat(fd).st_blksize`, or `None` when `fstat` fails.
    pub fn block_size(fd: c_int) -> Option<u64> {
        use std::mem::ManuallyDrop;
        use std::os::fd::FromRawFd;
        use std::os::unix::fs::MetadataExt;

        // SAFETY: `fd` is 1 or 2, never -1 (which `from_raw_fd` refuses), and
        // `File::metadata` on it is `fstat(2)`, defined for any descriptor
        // number: a closed one reports EBADF. `ManuallyDrop` keeps the
        // borrowed descriptor from being closed here.
        let file = ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(fd) });
        file.metadata().ok().map(|m| m.blksize())
    }
}

#[cfg(not(unix))]
mod sys {
    use std::io::{self, Write};

    pub const STDOUT: i32 = 1;
    pub const STDERR: i32 = 2;

    /// The Windows host the unit tests run on: through `std`, which is all
    /// there is.
    pub fn write_all(fd: i32, data: &[u8]) -> io::Result<()> {
        if fd == STDERR {
            let mut stderr = io::stderr().lock();
            stderr.write_all(data).and_then(|()| stderr.flush())
        } else {
            let mut stdout = io::stdout().lock();
            stdout.write_all(data).and_then(|()| stdout.flush())
        }
    }

    /// Nothing to catch on the host.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "the signature of the unix function this stands in for"
    )]
    pub fn dup_close(_fd: i32) -> io::Result<()> {
        Ok(())
    }

    /// The host's descriptors have no `st_blksize`; a pipe's is assumed.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "the signature of the unix function this stands in for"
    )]
    pub fn block_size(_fd: i32) -> Option<u64> {
        Some(4096)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buffer_is_sized_as_glibc_sizes_it() {
        assert_eq!(buffer_size(Some(4096)), 4096);
        assert_eq!(buffer_size(Some(512)), 512);
        // Not below BUFSIZ, zero, or no fstat at all: BUFSIZ.
        assert_eq!(buffer_size(Some(8192)), 8192);
        assert_eq!(buffer_size(Some(65536)), 8192);
        assert_eq!(buffer_size(Some(0)), 8192);
        assert_eq!(buffer_size(None), 8192);
    }

    fn err(kind: io::ErrorKind) -> io::Error {
        io::Error::from(kind)
    }

    #[test]
    fn an_earlier_failure_is_reported_without_its_reason() {
        let full = Some(err(io::ErrorKind::StorageFull));
        assert!(matches!(judge(full, Ok(())), Outcome::FailedBefore));
        // Even a closed stdout: the flag is what is found, not the EBADF.
        let closed = Some(io::Error::from_raw_os_error(EBADF));
        assert!(matches!(judge(closed, Ok(())), Outcome::FailedBefore));
    }

    #[test]
    fn a_failing_final_flush_is_reported_with_its_reason() {
        let full = Err(err(io::ErrorKind::StorageFull));
        assert!(matches!(judge(None, full), Outcome::FailedAtClose(_)));
    }

    #[test]
    fn a_closed_stdout_at_the_end_and_a_reader_gone_are_forgiven() {
        let closed = Err(io::Error::from_raw_os_error(EBADF));
        assert!(matches!(judge(None, closed), Outcome::Fine));
        let pipe = Err(err(io::ErrorKind::BrokenPipe));
        assert!(matches!(judge(None, pipe), Outcome::Fine));
        let earlier_pipe = Some(err(io::ErrorKind::BrokenPipe));
        assert!(matches!(judge(earlier_pipe, Ok(())), Outcome::Fine));
        assert!(matches!(judge(None, Ok(())), Outcome::Fine));
    }

    #[test]
    fn the_status_is_the_programs_unless_something_was_lost() {
        assert_eq!(verdict(&Outcome::Fine, false, 0, 3), 0);
        assert_eq!(verdict(&Outcome::Fine, false, 2, 3), 2);
        // A lost diagnostic overrides a success and a failure alike.
        assert_eq!(verdict(&Outcome::Fine, true, 0, 3), 3);
        assert_eq!(verdict(&Outcome::Fine, true, 1, 3), 3);
        assert_eq!(verdict(&Outcome::FailedBefore, false, 0, 1), 1);
        let reason = Outcome::FailedAtClose(err(io::ErrorKind::StorageFull));
        assert_eq!(verdict(&reason, false, 4, 3), 3);
    }
}
