//! Graph the system load average on a terminal: procps-ng 4.0.4's, ported.
//! Ubuntu ships `src/tload.c` unpatched, so this is the program Ubuntu ships
//! too.
//!
//! ```text
//! tload [options] [tty]
//! ```
//!
//! Each frame is the cursor sent home (`ESC [ H`) and the whole screen
//! written over: the one-minute load as a column of `*`, one column a frame,
//! with `-` and `=` marking each whole unit of the scale and the three load
//! averages written over the top-left corner.
//!
//! A transcription. What upstream does and this keeps:
//!
//! - The screen is one buffer of rows run together, and a scroll moves the
//!   whole buffer one byte left -- each row's first column becomes the last
//!   of the row above -- after which only the last column of every row but
//!   the bottom one is cleared.
//! - A frame writes all of the buffer but its last byte, and the byte before
//!   that is a NUL until something is drawn there.
//! - The height of the column is the load times the scale, made an `int` as
//!   C makes one from a `double`. A column that runs off the top halves the
//!   scale and is drawn again; the scale drifts back up, doubling a frame at
//!   a time, to `-s` (or the screen's height).
//! - The size is asked of what is written to -- standard output, or the
//!   terminal named -- and is 80 columns of 25 rows when it will not say. A
//!   change of size (`SIGWINCH`) clears the screen and starts again from the
//!   left, the scale as it was.
//! - Frames come `-d` seconds apart (5 by default), from `SIGALRM`, and the
//!   program runs until something ends it.
//!
//! # Deliberately different
//!
//! - `-V`/`--version` names this build.
//! - A size change is acted on at the wait between frames, not inside the
//!   handler: upstream `longjmp`s out of the handler, which Rust code cannot.
//!   `SIGWINCH` is held while a frame is drawn and let in as the wait begins,
//!   so the screen is started again as soon as upstream's would be --
//!   at once, during a wait; after the frame, where upstream would abandon
//!   it part-written.
//! - An empty `-s` or `-d` is refused without a reason after it. Upstream
//!   appends whatever `errno` the C library's start-up left -- measured on
//!   Ubuntu, `No such file or directory` from its locale files -- which is
//!   not information (`free`'s divergence 3).
//! - The argument in a refusal goes through `quoteaf` (`free`'s divergence 6).
//! - A column whose height is not a finite number -- a load of `inf` or
//!   `nan`, which the kernel never writes, or a scale that makes one: `-s
//!   inf`, `-s nan`, `-s 1e308` -- is drawn empty. Upstream makes the height
//!   an `int` (`INT_MIN`, from x86's conversion), whose decrement wraps to
//!   `INT_MAX`; the column runs off the top, the scale is halved, and an
//!   infinite height halved is still infinite. It never draws a frame, and
//!   spins a processor until it is killed (measured, `scripts/tload-diff.sh`).

use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::{self, LOADAVG_FILE};
use coreutils::quote::{os_bytes, os_from_bytes, quoteaf};
use coreutils::stdfd::{self, Stream};

/// The parser. Its name is never printed: getopt's complaints carry
/// `argv[0]`.
const TLOAD: Program = Program::new("tload", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "s:d:Vh";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("scale", Takes::Required),
    ("delay", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The usage text after the program's name.
const USAGE_REST: &str = concat!(
    " [options] [tty]\n",
    "\n",
    "Options:\n",
    " -d, --delay <secs>  update delay in seconds\n",
    " -s, --scale <num>   vertical scale\n",
    "\n",
    " -h, --help     display this help and exit\n",
    " -V, --version  output version information and exit\n",
    "\n",
    "For more details see tload(1).\n",
);

/// `ENOENT`.
const ENOENT: i32 = 2;

/// `dly`, for the `SIGALRM` handler to set the next alarm with.
static DELAY: AtomicU32 = AtomicU32::new(5);
/// A `SIGWINCH` came: the screen is to be measured and cleared again.
static RESIZED: AtomicBool = AtomicBool::new(false);

/// `alrm`: the next frame's alarm. `signal` keeps the handler installed.
extern "C" fn on_alarm(_sig: i32) {
    // The alarm's leftover is nothing to anyone: none is pending here.
    let _ = libcall::signal::alarm(DELAY.load(Ordering::Relaxed));
}

/// `setsize`'s work, deferred to the main loop (see "Deliberately
/// different").
extern "C" fn on_winch(_sig: i32) {
    RESIZED.store(true, Ordering::Relaxed);
}

/// `program_invocation_short_name`: what follows `argv[0]`'s last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or(argv0)
}

/// The screen: `nrows` rows of `ncols` bytes, run together.
struct Screen {
    nrows: i32,
    ncols: i32,
    buf: Vec<u8>,
}

impl Screen {
    /// Index of `(row, col)`, which the drawing keeps on the screen.
    fn at(&self, row: i32, col: i32) -> Option<usize> {
        let i = row.checked_mul(self.ncols)?.checked_add(col)?;
        usize::try_from(i).ok().filter(|&i| i < self.buf.len())
    }

    fn get(&self, row: i32, col: i32) -> u8 {
        self.at(row, col)
            .and_then(|i| self.buf.get(i).copied())
            .unwrap_or(0)
    }

    fn set(&mut self, row: i32, col: i32, b: u8) {
        if let Some(slot) = self.at(row, col).and_then(|i| self.buf.get_mut(i)) {
            *slot = b;
        }
    }
}

/// The program's state.
struct Tload {
    name: Vec<u8>,
    /// What is written to: standard output, or the terminal named.
    fd: i32,
    /// The terminal named, kept open for the life of the program.
    _tty: Option<std::fs::File>,
    screen: Screen,
}

impl Tload {
    /// `xerrx`/`xerr`: `NAME: MESSAGE[: REASON]`, standard output delivered
    /// first, and the status.
    fn fail(&self, msg: &[u8], errno: Option<i32>) -> u8 {
        let mut m = self.name.clone();
        m.extend_from_slice(b": ");
        m.extend_from_slice(msg);
        if let Some(e) = errno {
            m.extend_from_slice(b": ");
            m.extend_from_slice(
                coreutils::errmsg::strerror(&std::io::Error::from_raw_os_error(e)).as_bytes(),
            );
        }
        m.push(b'\n');
        stdfd::diag_bytes(&m);
        1
    }

    /// `setsize`: the screen measured, made, and filled with blanks but for
    /// the NUL before its last byte. `Err` is the status of a refusal.
    fn setsize(&mut self) -> Result<(), u8> {
        let (mut nrows, mut ncols) = (self.screen.nrows, self.screen.ncols);
        if let Ok(w) = libcall::pty::window_size(self.fd) {
            if w.cols > 0 {
                ncols = i32::from(w.cols);
            }
            if w.rows > 0 {
                nrows = i32::from(w.rows);
            }
        }
        // `ncols` is 2 or more by the time the division is made.
        if ncols < 2 || nrows < 2 || nrows >= i32::MAX.checked_div(ncols).unwrap_or(0) {
            return Err(self.fail(b"screen too small or too large", None));
        }
        let scr_size = nrows.saturating_mul(ncols);
        let size = usize::try_from(scr_size).unwrap_or(0);
        // `memset (screen, ' ', scr_size - 1)`. Upstream leaves the last byte
        // as the allocation had it; it is never shown before the column that
        // reaches it has been drawn, and a scroll is what would show it, so
        // a blank is as good as anything.
        let mut buf = vec![b' '; size];
        if let Some(nul) = size.checked_sub(2).and_then(|i| buf.get_mut(i)) {
            *nul = 0;
        }
        self.screen = Screen { nrows, ncols, buf };
        Ok(())
    }

    /// Draw one frame and write it. `Err` is the status of a failure.
    #[allow(clippy::too_many_lines)]
    fn frame(&mut self, col: &mut i32, scale_fact: &mut f64, max_scale: f64) -> Result<(), u8> {
        if *scale_fact < max_scale {
            // Help it drift back up.
            *scale_fact *= 2.0;
        }
        let av = match std::fs::read(LOADAVG_FILE) {
            Err(e) if e.raw_os_error() == Some(ENOENT) => {
                return Err(self.fail(b"Load average file /proc/loadavg does not exist", None));
            }
            Err(_) => return Err(self.fail(b"Unable to get load average", None)),
            Ok(text) => {
                let v = procps::scan_doubles(&text, 3);
                if v.len() < 3 {
                    return Err(self.fail(b"Unable to get load average", None));
                }
                (
                    v.first().copied().unwrap_or(0.0),
                    v.get(1).copied().unwrap_or(0.0),
                    v.get(2).copied().unwrap_or(0.0),
                )
            }
        };

        let nrows = self.screen.nrows;
        let ncols = self.screen.ncols;
        let mut row;
        loop {
            let height = av.0 * *scale_fact;
            row = nrows.saturating_sub(1);
            if !height.is_finite() {
                // Drawn empty: see "Deliberately different".
                break;
            }
            let mut lines = procps::c_int(height);
            let mut ran_off = false;
            loop {
                lines = lines.wrapping_sub(1);
                if lines < 0 {
                    break;
                }
                self.screen.set(row, *col, b'*');
                row = row.saturating_sub(1);
                if row < 0 {
                    *scale_fact /= 2.0;
                    ran_off = true;
                    break;
                }
            }
            if !ran_off {
                break;
            }
        }
        while row >= 0 {
            self.screen.set(row, *col, b' ');
            row = row.saturating_sub(1);
        }

        let mut i: i32 = 1;
        loop {
            row = procps::c_int(f64::from(nrows) - f64::from(i) * *scale_fact);
            if row < 0 || row >= nrows {
                break;
            }
            let mark = if self.screen.get(row, *col) == b' ' {
                b'-'
            } else {
                b'='
            };
            self.screen.set(row, *col, mark);
            i = i.saturating_add(1);
        }

        *col = col.saturating_add(1);
        if *col == ncols {
            *col = col.saturating_sub(1);
            let len = self.screen.buf.len();
            self.screen.buf.copy_within(1..len, 0);
            let mut r = nrows.saturating_sub(2);
            while r >= 0 {
                self.screen.set(r, *col, b' ');
                r = r.saturating_sub(1);
            }
        }

        // `snprintf (screen, scr_size, " %.2f, %.2f, %.2f", …)`, and its NUL
        // made a blank when it fell inside the screen.
        let text = format!(
            " {}, {}, {}",
            procps::fixed2(av.0),
            procps::fixed2(av.1),
            procps::fixed2(av.2)
        );
        let size = self.screen.buf.len();
        let n = text.len().min(size.saturating_sub(1));
        if let Some(dst) = self.screen.buf.get_mut(..n) {
            dst.copy_from_slice(text.as_bytes().get(..n).unwrap_or_default());
        }
        if let Some(nul) = self.screen.buf.get_mut(n) {
            *nul = if text.len() < size { b' ' } else { 0 };
        }

        let body = self
            .screen
            .buf
            .get(..size.saturating_sub(1))
            .unwrap_or_default();
        for part in [&b"\x1b[H"[..], body] {
            if let Err(e) = write_once(self.fd, part) {
                return Err(self.fail(b"writing to tty failed", Some(e)));
            }
        }
        Ok(())
    }
}

/// One `write (2)`, as upstream makes it: a short write is not looked at,
/// a failure is.
fn write_once(fd: i32, bytes: &[u8]) -> Result<(), i32> {
    stdfd::write_some(fd, bytes)
        .map(|_| ())
        .map_err(|e| e.raw_os_error().unwrap_or(5))
}

/// `usage (out)`, and the status `exit` is then given.
fn usage(name: &[u8], to_stdout: bool, out: &mut Stream) -> u8 {
    let mut text = b"\nUsage:\n ".to_vec();
    text.extend_from_slice(name);
    text.extend_from_slice(USAGE_REST.as_bytes());
    if to_stdout {
        // `Stream` never fails a write; a failure is `close_stdout`'s to say.
        let _ = out.write_all(&text);
        0
    } else {
        stdfd::diag_bytes_ahead_of_stdout(&text);
        1
    }
}

/// `"failed to parse argument: 'ARG'"`, and a reason when there is one.
fn parse_failure(name: &[u8], arg: &[u8], reason: &str) -> u8 {
    let mut m = name.to_vec();
    m.extend_from_slice(b": failed to parse argument: ");
    m.extend_from_slice(quoteaf(arg).as_bytes());
    m.extend_from_slice(reason.as_bytes());
    m.push(b'\n');
    stdfd::diag_bytes(&m);
    1
}

/// `strtod_or_err`: all of `arg` a number as glibc's `strtod` reads one.
fn strtod_or_err(name: &[u8], arg: &[u8]) -> Result<f64, u8> {
    let r = ulstrutils::strtod(arg);
    if arg.is_empty() || r.end == 0 || r.end != arg.len() {
        return Err(parse_failure(name, arg, ""));
    }
    if r.erange {
        return Err(parse_failure(name, arg, ": Numerical result out of range"));
    }
    Ok(r.value)
}

/// `strtol_or_err`.
fn strtol_or_err(name: &[u8], arg: &[u8]) -> Result<i64, u8> {
    coreutils::procps::strutils::strtol(arg)
        .map_err(|fault| parse_failure(name, arg, fault.suffix()))
}

/// Everything `main` does: the status of a refusal, or never.
fn run(argv: &[OsString], out: &mut Stream) -> u8 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let name = short_name(&argv0).to_vec();
    let words = argv.get(1..).unwrap_or(&[]);
    let mut max_scale: f64 = 0.0;
    let mut dly: u32 = 5;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in TLOAD.parse(words, SHORT_OPTIONS, LONG_OPTIONS) {
        match item {
            Ok(Opt::Short(b's', Some(v)) | Opt::Long("scale", Some(v))) => {
                match strtod_or_err(&name, &os_bytes(&v)) {
                    Ok(s) => max_scale = s,
                    Err(status) => return status,
                }
                if max_scale < 0.0 {
                    let mut m = name.clone();
                    m.extend_from_slice(b": scale cannot be negative\n");
                    stdfd::diag_bytes(&m);
                    return 1;
                }
            }
            Ok(Opt::Short(b'd', Some(v)) | Opt::Long("delay", Some(v))) => {
                let tmpdly = match strtol_or_err(&name, &os_bytes(&v)) {
                    Ok(d) => d,
                    Err(status) => return status,
                };
                let refusal: Option<&[u8]> = if tmpdly < 1 {
                    Some(b": delay must be positive integer\n")
                } else if tmpdly > i64::from(u32::MAX) {
                    Some(b": too large delay value\n")
                } else {
                    None
                };
                if let Some(r) = refusal {
                    let mut m = name.clone();
                    m.extend_from_slice(r);
                    stdfd::diag_bytes(&m);
                    return 1;
                }
                dly = u32::try_from(tmpdly).unwrap_or(u32::MAX);
            }
            Ok(Opt::Short(b'V', _) | Opt::Long("version", _)) => {
                let mut v = name.clone();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                // `Stream` never fails a write; see `usage`.
                let _ = out.write_all(&v);
                return 0;
            }
            Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => return usage(&name, true, out),
            Ok(Opt::Operand(o)) => operands.push(os_bytes(o).into_owned()),
            Ok(Opt::Short(..) | Opt::Long(..)) => return usage(&name, false, out),
            Err(e) => {
                let mut m = argv0.clone();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                stdfd::diag_bytes_ahead_of_stdout(&m);
                return usage(&name, false, out);
            }
        }
    }

    let mut t = Tload {
        name,
        fd: 1,
        _tty: None,
        screen: Screen {
            nrows: 25,
            ncols: 80,
            buf: Vec::new(),
        },
    };
    if let Some(tty) = operands.first() {
        match std::fs::OpenOptions::new()
            .write(true)
            .open(os_from_bytes(tty))
        {
            Ok(f) => {
                t.fd = raw_fd(&f);
                t._tty = Some(f);
            }
            Err(e) => return t.fail(b"can not open tty", Some(e.raw_os_error().unwrap_or(5))),
        }
    }
    if let Err(status) = t.setsize() {
        return status;
    }
    if max_scale == 0.0 {
        max_scale = f64::from(t.screen.nrows);
    }
    let mut scale_fact = max_scale;

    DELAY.store(dly, Ordering::Relaxed);
    // Handlers that stay installed, as `signal` installs them; a failure to
    // install leaves the program drawing one frame and waiting, which is what
    // upstream's unchecked `signal` would leave too.
    let _ = libcall::signal::set_handler(libcall::signal::SIGWINCH, on_winch, true);
    let _ = libcall::signal::set_handler(libcall::signal::SIGALRM, on_alarm, true);
    // `SIGWINCH` is held while a frame is drawn and let in only while
    // waiting, the wait and the letting-in one step (`sigsuspend`): a resize
    // in the middle of a frame then ends the very next wait, where upstream
    // jumps out of the frame at once. The mask waited with is the one the
    // program was given, so a signal that was blocked there stays blocked,
    // as it would for upstream's `pause`.
    let mut winch = libcall::signal::SigSet::empty();
    // Neither call fails on a unix C library given a real signal; were one
    // to, the wait would let everything in, which is `pause` with nothing
    // held back.
    let given = winch
        .add(libcall::signal::SIGWINCH)
        .and_then(|()| libcall::signal::block(&winch))
        .unwrap_or_else(|_| libcall::signal::SigSet::empty());
    loop {
        // `setjmp (jb)` returns here, the first time and after each resize.
        let mut col: i32 = 0;
        on_alarm(0);
        loop {
            if let Err(status) = t.frame(&mut col, &mut scale_fact, max_scale) {
                return status;
            }
            // An answer other than "a handler ran" cannot come.
            let _ = libcall::signal::suspend(&given);
            if RESIZED.swap(false, Ordering::Relaxed) {
                if let Err(status) = t.setsize() {
                    return status;
                }
                break;
            }
        }
    }
}

#[cfg(unix)]
fn raw_fd(f: &std::fs::File) -> i32 {
    use std::os::fd::AsRawFd;
    f.as_raw_fd()
}

/// The host build has no terminals to write to.
#[cfg(not(unix))]
fn raw_fd(_f: &std::fs::File) -> i32 {
    -1
}

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    if !cfg!(unix) {
        // The host build is for the unit tests: no signals to draw by.
        coreutils::diag!("tload: not supported on this host");
        return ExitCode::FAILURE;
    }
    let argv: Vec<OsString> = std::env::args_os().collect();
    let name = argv
        .first()
        .map_or_else(Vec::new, |a| short_name(&os_bytes(a)).to_vec());
    let mut out = Stream::stdout();
    let status = run(&argv, &mut out);
    stdfd::close_stdout_procps(&name, out, ExitCode::from(status))
}

#[cfg(test)]
mod tests {
    use super::Screen;

    #[test]
    fn the_screen_is_rows_run_together() {
        let mut s = Screen {
            nrows: 2,
            ncols: 3,
            buf: vec![b' '; 6],
        };
        s.set(1, 2, b'*');
        assert_eq!(s.buf, b"     *");
        assert_eq!(s.get(1, 2), b'*');
        s.set(2, 0, b'x');
        assert_eq!(s.buf, b"     *", "off the screen: nothing");
        assert_eq!(s.get(-1, 0), 0);
    }
}
