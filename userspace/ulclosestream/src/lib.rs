//! util-linux's `include/closestream.h`: standard output as glibc's stdio
//! holds it, diagnostics as `warn`/`warnx` write them, and `close_stdout`'s
//! verdict at the end.
//!
//! Every util-linux program registers `close_stdout` with `atexit`, and what
//! a caller can see of it depends on details of stdio that Rust's own
//! `stdout` does not have:
//!
//! * **Whether a write was attempted before the end.** glibc holds output in
//!   a 4096-byte buffer when stdout is not a terminal (`st_blksize` of a
//!   pipe, a file, `/dev/full`) and line by line when it is. `lsmem >
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

/// glibc's buffer size for a stream that is not a terminal.
const BUFSIZ: usize = 4096;

/// `ferror(stderr)`: a diagnostic could not be written. Process-global, as
/// stderr is.
static STDERR_FAILED: AtomicBool = AtomicBool::new(false);

/// Standard output, buffered as glibc buffers it.
pub struct Stdout {
    /// Line buffered rather than fully buffered.
    tty: bool,
    /// Written, not yet flushed.
    held: Vec<u8>,
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
            tty: io::stdout().is_terminal(),
            held: Vec::new(),
            failed: None,
            close_exit_code,
        }
    }

    /// `fputs`, `printf`: into the buffer, and out of it when glibc would
    /// write -- at a newline on a terminal, when the buffer overflows
    /// otherwise.
    pub fn write(&mut self, data: &[u8]) {
        self.held.extend_from_slice(data);
        let due = if self.tty {
            self.held
                .iter()
                .rposition(|&b| b == b'\n')
                .map(|nl| nl.saturating_add(1))
        } else {
            (self.held.len() > BUFSIZ).then_some(self.held.len())
        };
        if let Some(n) = due {
            let chunk: Vec<u8> = self.held.drain(..n).collect();
            if let Err(e) = sys::write_all(sys::STDOUT, &chunk)
                && self.failed.is_none()
            {
                // glibc drops what it could not write and sets the flag.
                self.failed = Some(e);
            }
        }
    }

    /// `close_stdout`: flush what is held, report a failure as util-linux
    /// does, then judge stderr. Returns the status to exit with -- `status`,
    /// or `CLOSE_EXIT_CODE`.
    pub fn close(mut self, status: u8, short: &[u8]) -> u8 {
        let verdict = if let Some(e) = self.failed.take() {
            if e.kind() == io::ErrorKind::BrokenPipe {
                None
            } else {
                // `flush_standard_stream` resets errno and finds the flag:
                // `warnx`, no reason.
                warnx(short, "write error");
                Some(self.close_exit_code)
            }
        } else {
            let held = std::mem::take(&mut self.held);
            match sys::write_all(sys::STDOUT, &held).and_then(|()| sys::dup_close(sys::STDOUT)) {
                Ok(()) => None,
                // A closed stdout: forgiven.
                Err(e) if sys::is_ebadf(&e) => None,
                Err(e) if e.kind() == io::ErrorKind::BrokenPipe => None,
                Err(e) => {
                    warn(short, "write error", &e);
                    Some(self.close_exit_code)
                }
            }
        };
        if let Some(code) = verdict {
            return code;
        }
        // `flush_standard_stream(stderr)`: a diagnostic that was lost.
        if STDERR_FAILED.load(Ordering::Relaxed) {
            return self.close_exit_code;
        }
        status
    }
}

/// Bytes to stderr, unbuffered as glibc's stderr is; a failure is recorded
/// for [`Stdout::close`] to find.
pub fn stderr_write(data: &[u8]) {
    if sys::write_all(sys::STDERR, data).is_err() {
        STDERR_FAILED.store(true, Ordering::Relaxed);
    }
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
    /// `EBADF`, on Linux and in the SlateOS C library.
    const EBADF: i32 = 9;

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

    /// Whether a failure is `EBADF`.
    pub fn is_ebadf(e: &io::Error) -> bool {
        e.raw_os_error() == Some(EBADF)
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

    /// The host never reports it: `std` hides it.
    pub fn is_ebadf(_e: &io::Error) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_close_keeps_the_status() {
        let mut out = Stdout::new(1);
        out.write(b"");
        assert_eq!(out.close(0, b"t"), 0);
        let out = Stdout::new(3);
        assert_eq!(out.close(2, b"t"), 2);
    }

    #[test]
    fn an_earlier_broken_pipe_is_not_reported() {
        let mut out = Stdout::new(1);
        out.failed = Some(io::Error::from(io::ErrorKind::BrokenPipe));
        assert_eq!(out.close(0, b"t"), 0);
    }

    #[test]
    fn an_earlier_failure_is_the_close_exit_code() {
        let mut out = Stdout::new(3);
        out.failed = Some(io::Error::from(io::ErrorKind::StorageFull));
        assert_eq!(out.close(0, b"t"), 3);
    }
}
