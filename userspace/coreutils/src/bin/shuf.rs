//! `shuf` — write a random permutation of the input lines.
//!
//! A port of GNU coreutils 9.4's `src/shuf.c`, with gnulib's `randperm` beside
//! it and `randint` from [`coreutils::randint`]: the same options, messages and
//! order of events, and -- because `--random-source=FILE` makes a run
//! reproducible -- the same consumption of the random bytes, so that one file
//! of "random" bytes shuffles a list the same way here as there.
//!
//! # What this replaced
//!
//! `userspace/shuf`, written by hand. It read its arguments as `String` (a
//! file name that was not UTF-8 killed it before its first statement), and its
//! shuffle was its own, so `--random-source` could not reproduce GNU's output.
//!
//! # What no reading of `--help` suggests
//!
//! **`-n` on a pipe samples a reservoir.** When the input's size cannot be
//! known -- a pipe -- or exceeds 8 MiB, and `-n` limits the output, upstream
//! keeps only `-n` lines, replacing them as it reads with probability falling
//! per line, and draws one more random number after the last line than there
//! were lines to place. So `cat f | shuf -n 2` and `shuf -n 2 f` consume the
//! random source differently and print different lines from it.
//!
//! **The permutation has two algorithms, and they can differ.** For a large
//! range sampled sparsely -- `n >= 131072` and `n / h >= 32`, as in
//! `shuf -i 1-1000000 -n 10` -- gnulib records its swaps in a hash table, and
//! a step that swaps an element with itself after an earlier step moved it
//! loses the moved value (the table refuses the second insertion of one key).
//! That is reproduced, not repaired: the output is the point.
//!
//! **`-i 5-4` is an empty range, not an error;** `-i 6-4` is an error.
//! **`-o FILE` is opened only after the input is read,** so `shuf -o f f`
//! shuffles `f` in place. **`-n` larger than any count is no limit.**
//!
//! # Reference
//!
//! Measured against GNU `shuf` (coreutils 9.4) through WSL; where measurement
//! could not settle a rule, `coreutils-9.4/src/shuf.c` and gnulib's
//! `randperm.c` settled it. `scripts/shuf-diff.sh` is the executable form of
//! every claim here.

use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quotef};
use coreutils::randint::{RandError, RandInt, RandRead};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum::{self, Status};
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{Read, Write as _};
use std::process::ExitCode;

// Before `main`, so that `stdfd::restore` still sees a caller's descriptors.
coreutils::guard_std_fds!();

/// `shuf -Z; echo $?` is 1.
const SHUF: Program = Program::new("shuf", 1);

/// Upstream's `getopt_long` string, verbatim.
const SHORT_OPTIONS: &str = "ei:n:o:rz";

/// Upstream's `long_opts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("echo", Takes::Nothing),
    ("input-range", Takes::Required),
    ("head-count", Takes::Required),
    ("output", Takes::Required),
    ("random-source", Takes::Required),
    ("repeat", Takes::Nothing),
    ("zero-terminated", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `RESERVOIR_MIN_INPUT`: the input size above which `-n` samples a
/// reservoir rather than reading everything.
const RESERVOIR_MIN_INPUT: u64 = 8192 * 1024;

/// glibc's `strerror (EOVERFLOW)`.
const EOVERFLOW_TEXT: &str = "Value too large for defined data type";

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Settings),
}

#[derive(Debug, PartialEq, Eq)]
struct Settings {
    echo: bool,
    /// `-i LO-HI`.
    range: Option<(u64, u64)>,
    /// `-n`; `u64::MAX` (upstream's `SIZE_MAX`) when not given.
    head_lines: u64,
    outfile: Option<Vec<u8>>,
    random_source: Option<Vec<u8>>,
    repeat: bool,
    /// `\n`, or NUL under `-z`.
    eol: u8,
    operands: Vec<OsString>,
}

/// A failure upstream reports with `error (EXIT_FAILURE, ...)`: the message
/// after `shuf: `.
#[derive(Debug)]
struct Fatal(String);

/// Why the run stopped short.
enum Stop {
    Fatal(Fatal),
    /// A write failed and has been reported, as coreutils' `write_error`
    /// reports one.
    Written,
    /// The reader went away. Upstream died of `SIGPIPE` at that write, saying
    /// nothing; here the run ends as quietly, keeping the status it had
    /// earned (`stdfd::reader_gone`).
    ReaderGone,
}

impl From<Fatal> for Stop {
    fn from(f: Fatal) -> Stop {
        Stop::Fatal(f)
    }
}

impl From<RandError> for Stop {
    fn from(e: RandError) -> Stop {
        Stop::Fatal(Fatal::from(e))
    }
}

impl From<RandError> for Fatal {
    /// gnulib's `randread_error`.
    fn from(e: RandError) -> Fatal {
        Fatal(match e {
            RandError::EndOfFile(name) => format!("{}: end of file", quote(&name)),
            RandError::Read(name, err) => format!(
                "{}: read error: {}",
                quote(&name),
                coreutils::errmsg::strerror(&err)
            ),
            RandError::System => {
                "getrandom: the system random number generator is unavailable".to_string()
            }
        })
    }
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            diag!("shuf: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };
    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            // A failed write is `close_stdout`'s to report, below.
            let _ = out.write_all(HELP.as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"shuf (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Run(settings) => match run(&settings, &mut out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(Stop::Fatal(Fatal(message))) => {
                diag!("shuf: {message}");
                // Upstream's `error (EXIT_FAILURE, ...)` exits through
                // `close_stdout` too, which reports a failed write after it.
                return stdfd::close_stdout("shuf", out, ExitCode::FAILURE);
            }
            Err(Stop::Written) => return ExitCode::FAILURE,
            Err(Stop::ReaderGone) => return ExitCode::SUCCESS,
        },
    };
    stdfd::close_stdout("shuf", out, earned)
}

/// Upstream checks every write: `if (fwrite (...) != len) return -1;` and
/// then `write_error ()` -- which flushes, discards what is buffered, clears
/// the error so that `close_stdout` says nothing more, and reports the failure
/// *with* its reason. A reader that went away is reported by no one: upstream
/// died of `SIGPIPE` at that write, and stays quiet doing so.
fn checked(out: &mut Stream) -> Result<(), Stop> {
    if !out.errored() {
        return Ok(());
    }
    let err = out.error();
    out.abandon();
    match err {
        Some(e) if stdfd::reader_gone(&e) => Err(Stop::ReaderGone),
        Some(e) => {
            stdfd::write_error("shuf", &e);
            Err(Stop::Written)
        }
        None => Err(Stop::Written),
    }
}

/// Read the command line: upstream's option loop, then its checks of the
/// operands.
///
/// # Errors
/// Any getopt diagnostic, and upstream's own usage errors.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut set = Settings {
        echo: false,
        range: None,
        head_lines: u64::MAX,
        outfile: None,
        random_source: None,
        repeat: false,
        eol: b'\n',
        operands: Vec::new(),
    };
    for item in SHUF.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Short(b'e', _) | Opt::Long("echo", _) => set.echo = true,
            Opt::Short(b'i', Some(v)) | Opt::Long("input-range", Some(v)) => {
                if set.range.is_some() {
                    return Err(SHUF.usage("multiple -i options specified".to_string()));
                }
                set.range = Some(input_range(&os_bytes(&v))?);
            }
            Opt::Short(b'n', Some(v)) | Opt::Long("head-count", Some(v)) => {
                let arg = os_bytes(&v);
                match xnum::xstrtoumax(&arg, Some(b"")) {
                    (n, Status::Ok) => set.head_lines = set.head_lines.min(n),
                    // A count past every count is no limit at all.
                    (_, Status::Overflow) => {}
                    _ => {
                        return Err(SHUF.usage(format!("invalid line count: {}", quote(&arg))));
                    }
                }
            }
            Opt::Short(b'o', Some(v)) | Opt::Long("output", Some(v)) => {
                let file = os_bytes(&v).into_owned();
                if set.outfile.as_ref().is_some_and(|f| *f != file) {
                    return Err(SHUF.usage("multiple output files specified".to_string()));
                }
                set.outfile = Some(file);
            }
            Opt::Long("random-source", Some(v)) => {
                let source = os_bytes(&v).into_owned();
                if set.random_source.as_ref().is_some_and(|s| *s != source) {
                    return Err(SHUF.usage("multiple random sources specified".to_string()));
                }
                set.random_source = Some(source);
            }
            Opt::Short(b'r', _) | Opt::Long("repeat", _) => set.repeat = true,
            Opt::Short(b'z', _) | Opt::Long("zero-terminated", _) => set.eol = 0,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => set.operands.push(word.clone()),
            // Every option in the tables is handled above; an unknown one
            // arrives as an `Err`, and a missing value cannot parse.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    if set.echo && set.range.is_some() {
        return Err(SHUF.usage_referring("cannot combine -e and -i options".to_string()));
    }
    let extra = if set.range.is_some() {
        set.operands.first()
    } else if !set.echo {
        set.operands.get(1)
    } else {
        None
    };
    if let Some(extra) = extra {
        return Err(SHUF.usage_referring(format!("extra operand {}", quote(&os_bytes(extra)))));
    }
    Ok(Request::Run(set))
}

/// `-i LO-HI`, read as upstream reads it: two `xstrtoumax` numbers and a
/// dash, and a range whose count wraps to 0 only when it is the one empty
/// range `LO-(LO-1)`.
fn input_range(arg: &[u8]) -> Result<(u64, u64), getopt::Error> {
    let (mut lo, mut hi) = (u64::MAX, 0u64);
    let (u, mut status, end) = xnum::xstrtoumax_end(arg, 10, None);
    if status == Status::Ok {
        lo = u;
        if arg.get(end) == Some(&b'-') {
            let rest = arg.get(end.saturating_add(1)..).unwrap_or_default();
            let (u, s) = xnum::xstrtoumax(rest, Some(b""));
            status = s;
            if s == Status::Ok {
                hi = u;
            }
        } else {
            status = Status::Invalid;
        }
    }
    let n_lines = hi.wrapping_sub(lo).wrapping_add(1);
    if status != Status::Ok || (lo <= hi) == (n_lines == 0) {
        let mut message = format!("invalid input range: {}", quote(arg));
        if status == Status::Overflow {
            message.push_str(": ");
            message.push_str(EOVERFLOW_TEXT);
        }
        return Err(SHUF.usage(message));
    }
    Ok((lo, hi))
}

/// GNU's `--help`, minus the ancillary block of URLs.
const HELP: &str = "\
Usage: shuf [OPTION]... [FILE]
  or:  shuf -e [OPTION]... [ARG]...
  or:  shuf -i LO-HI [OPTION]...
Write a random permutation of the input lines to standard output.

With no FILE, or when FILE is -, read standard input.

Mandatory arguments to long options are mandatory for short options too.
  -e, --echo                treat each ARG as an input line
  -i, --input-range=LO-HI   treat each number LO through HI as an input line
  -n, --head-count=COUNT    output at most COUNT lines
  -o, --output=FILE         write result to FILE instead of standard output
      --random-source=FILE  get random bytes from FILE
  -r, --repeat              output lines can be repeated
  -z, --zero-terminated     line delimiter is NUL, not newline
      --help        display this help and exit
      --version     output version information and exit
";

/// Where the lines come from.
enum Lines {
    /// Lines, each with its terminator: from `-e`, or a whole input read.
    Text(Vec<Vec<u8>>),
    /// `-i LO-HI`: the numbers, never stored.
    Range(u64),
}

/// `main` after the options: read the input, draw the permutation, open the
/// output, write.
fn run(set: &Settings, out: &mut Stream) -> Result<(), Stop> {
    // Prepare input.
    let mut reservoir_input: Option<Box<dyn Read>> = None;
    let mut n_lines: u64;
    let mut lines = Lines::Text(Vec::new());
    if set.head_lines == 0 {
        n_lines = 0;
    } else if set.echo {
        let echoed: Vec<Vec<u8>> = set
            .operands
            .iter()
            .map(|w| {
                let mut line = os_bytes(w).into_owned();
                line.push(set.eol);
                line
            })
            .collect();
        n_lines = as_u64(echoed.len());
        lines = Lines::Text(echoed);
    } else if let Some((lo, hi)) = set.range {
        n_lines = hi.wrapping_sub(lo).wrapping_add(1);
        lines = Lines::Range(lo);
    } else {
        let (mut input, size) = imp::open_input(set.operands.first())?;
        if set.repeat || set.head_lines == u64::MAX || size <= RESERVOIR_MIN_INPUT {
            let read = read_input(&mut input, set.eol)?;
            n_lines = as_u64(read.len());
            lines = Lines::Text(read);
        } else {
            // Unknown, for now.
            n_lines = u64::MAX;
            reservoir_input = Some(input);
        }
    }

    // The adjusted head count; less than `-n` when the input is.
    let mut ahead = if set.repeat || set.head_lines < n_lines {
        set.head_lines
    } else {
        n_lines
    };

    let source = RandRead::open(set.random_source.as_deref()).map_err(|e| {
        let name = set.random_source.as_deref().unwrap_or(b"getrandom");
        Fatal(format!(
            "{}: {}",
            quotef(name),
            coreutils::errmsg::strerror(&e)
        ))
    })?;
    let mut rand = RandInt::new(source);

    if let Some(input) = reservoir_input {
        let mut input = std::io::BufReader::new(input);
        let kept = reservoir_sample(&mut input, set.eol, ahead, &mut rand)?;
        n_lines = as_u64(kept.len());
        ahead = n_lines;
        lines = Lines::Text(kept);
    }

    let permutation = if set.repeat {
        Vec::new()
    } else {
        randperm(&mut rand, ahead, n_lines)?
    };

    if let Some(outfile) = &set.outfile {
        imp::redirect_stdout(outfile)?;
    }

    if set.repeat {
        if set.head_lines == 0 {
            return Ok(());
        }
        if n_lines == 0 {
            return Err(Fatal("no lines to repeat".to_string()).into());
        }
        let mut written: u64 = 0;
        while written < ahead {
            let j = rand.choose(n_lines)?;
            write_line(out, &lines, j, set.eol);
            checked(out)?;
            written = written.saturating_add(1);
        }
        return Ok(());
    }

    for &j in &permutation {
        write_line(out, &lines, j, set.eol);
        checked(out)?;
    }
    Ok(())
}

/// Line `j` of the input, or the number `LO + j`. A failed write is recorded
/// by the stream, for [`checked`].
fn write_line(out: &mut Stream, lines: &Lines, j: u64, eol: u8) {
    match lines {
        Lines::Range(lo) => write_number(out, lo.wrapping_add(j), eol),
        Lines::Text(text) => {
            let line = usize::try_from(j).ok().and_then(|j| text.get(j));
            let _ = out.write_all(line.map_or(&[][..], Vec::as_slice));
        }
    }
}

/// `printf ("%lu%c", n, eolbyte)`. A failed write is `close_stdout`'s to report.
fn write_number(out: &mut Stream, n: u64, eol: u8) {
    let _ = out.write_all(n.to_string().as_bytes());
    let _ = out.write_all(&[eol]);
}

/// A count as the width upstream counts it in.
fn as_u64(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// `read_input`: all of it, a terminator added to a last line that lacks one,
/// split after each terminator.
fn read_input(input: &mut dyn Read, eol: u8) -> Result<Vec<Vec<u8>>, Fatal> {
    let mut buf = Vec::new();
    if let Err(e) = input.read_to_end(&mut buf) {
        return Err(Fatal(format!(
            "read error: {}",
            coreutils::errmsg::strerror(&e)
        )));
    }
    if buf.last().is_some_and(|&b| b != eol) {
        buf.push(eol);
    }
    Ok(buf
        .split_inclusive(|&b| b == eol)
        .map(<[u8]>::to_vec)
        .collect())
}

/// `readlinebuffer_delim`: the next line with its terminator, one added if
/// the input ends without it; `None` at the end.
fn read_line(input: &mut dyn std::io::BufRead, eol: u8) -> Result<Option<Vec<u8>>, Fatal> {
    let mut line = Vec::new();
    match input.read_until(eol, &mut line) {
        Ok(0) => Ok(None),
        Ok(_) => {
            if line.last() != Some(&eol) {
                line.push(eol);
            }
            Ok(Some(line))
        }
        Err(e) => Err(Fatal(format!(
            "read error: {}",
            coreutils::errmsg::strerror(&e)
        ))),
    }
}

/// `read_input_reservoir_sampling`: keep `k` lines of the input, each later
/// line replacing a kept one with falling probability.
///
/// One random number is drawn for every line past the first `k` *and one more*:
/// upstream draws before it reads, so the draw for the line after the last is
/// made and wasted. With `--random-source`, that is a byte the permutation
/// does not get.
fn reservoir_sample(
    input: &mut dyn std::io::BufRead,
    eol: u8,
    k: u64,
    rand: &mut RandInt,
) -> Result<Vec<Vec<u8>>, Fatal> {
    let mut kept: Vec<Vec<u8>> = Vec::new();
    let mut n_lines: u64 = 0;
    let mut last_read = true;
    while n_lines < k {
        match read_line(input, eol)? {
            Some(line) => {
                kept.push(line);
                n_lines = n_lines.saturating_add(1);
            }
            None => {
                last_read = false;
                break;
            }
        }
    }
    if last_read {
        loop {
            let j = rand.choose(n_lines.wrapping_add(1))?;
            let Some(line) = read_line(input, eol)? else {
                break;
            };
            if j < k {
                if let Some(slot) = usize::try_from(j).ok().and_then(|j| kept.get_mut(j)) {
                    *slot = line;
                }
            }
            n_lines = n_lines.wrapping_add(1);
            if n_lines == 0 {
                return Err(Fatal(format!("too many input lines: {EOVERFLOW_TEXT}")));
            }
        }
    }
    Ok(kept)
}

/// gnulib's `randperm_new`: the first `h` elements of a random permutation of
/// `0..n`, by Fisher-Yates -- dense, or, for a large `n` sampled sparsely,
/// with the swaps kept in a hash table, quirk and all (see the module docs).
fn randperm(rand: &mut RandInt, h: u64, n: u64) -> Result<Vec<u64>, Fatal> {
    match h {
        0 => Ok(Vec::new()),
        1 => Ok(vec![rand.choose(n)?]),
        _ => {
            // `h` is at least 2 here, so the division has a divisor.
            let sparse = n >= 128 * 1024 && n.checked_div(h).is_some_and(|ratio| ratio >= 32);
            let h_len = usize::try_from(h).map_err(|_| memory_exhausted())?;
            if sparse {
                let mut map: HashMap<u64, u64> = HashMap::new();
                let mut v: Vec<u64> = Vec::new();
                v.try_reserve_exact(h_len).map_err(|_| memory_exhausted())?;
                v.resize(h_len, 0);
                for i in 0..h {
                    let j = i.saturating_add(rand.choose(n.saturating_sub(i))?);
                    // `sparse_swap`: each side's value is its entry's, or its
                    // index; the two are exchanged and both entries put back --
                    // except that when `i == j`, the second removal found
                    // nothing, the second entry is a fresh one, and putting it
                    // back is refused as a duplicate key.
                    let at_i = map.remove(&i).unwrap_or(i);
                    let at_j = map.remove(&j).unwrap_or(j);
                    map.insert(i, at_j);
                    if i != j {
                        map.insert(j, at_i);
                    }
                    if let Some(slot) = usize::try_from(i).ok().and_then(|i| v.get_mut(i)) {
                        *slot = at_j;
                    }
                }
                Ok(v)
            } else {
                // `xnmalloc (n, ...)`: a range too large to hold is "memory
                // exhausted", not an abort.
                let n_len = usize::try_from(n).map_err(|_| memory_exhausted())?;
                let mut v: Vec<u64> = Vec::new();
                v.try_reserve_exact(n_len).map_err(|_| memory_exhausted())?;
                v.extend(0..n);
                for i in 0..h {
                    let j = i.saturating_add(rand.choose(n.saturating_sub(i))?);
                    if let (Ok(i), Ok(j)) = (usize::try_from(i), usize::try_from(j)) {
                        if i < v.len() && j < v.len() {
                            v.swap(i, j);
                        }
                    }
                }
                v.truncate(h_len);
                Ok(v)
            }
        }
    }
}

fn memory_exhausted() -> Fatal {
    Fatal("memory exhausted".to_string())
}

#[cfg(unix)]
mod imp {
    use super::Fatal;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{os_bytes, os_from_bytes, quotef};
    use std::ffi::OsString;
    use std::fs::File;
    use std::io::{self, Read, Seek};
    use std::mem::ManuallyDrop;
    use std::os::fd::{AsRawFd, FromRawFd};

    unsafe extern "C" {
        /// `dup2(2)`, for `freopen (outfile, "w", stdout)`: the output file
        /// becomes descriptor 1, where standard output is written.
        #[link_name = "dup2"]
        fn libc_dup2(old: i32, new: i32) -> i32;
    }

    /// Standard input, borrowed: the descriptor is the process's.
    struct Stdin(ManuallyDrop<File>);

    impl Read for Stdin {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            (&*self.0).read(buf)
        }
    }

    /// The input -- the one operand that is not `-`, or standard input -- and
    /// upstream's `input_size`: what is left of it, when it is a regular file,
    /// and `u64::MAX` (`OFF_T_MAX`) when that cannot be known.
    pub fn open_input(operand: Option<&OsString>) -> Result<(Box<dyn Read>, u64), Fatal> {
        let named = operand
            .map(|w| os_bytes(w).into_owned())
            .filter(|n| n != b"-");
        let file = match named {
            Some(name) => File::open(os_from_bytes(&name))
                .map(ManuallyDrop::new)
                .map_err(|e| {
                    // Upstream opens it with `freopen (name, "r", stdin)`, and
                    // glibc's `freopen` closes the stream's descriptor when
                    // the open fails, so `errno` is whatever that close left:
                    // `EBADF` when standard input was already closed.
                    // Measured: `shuf nosuch <&-` says
                    // `shuf: nosuch: Bad file descriptor`. The process exits
                    // next, so closing descriptor 0 here changes nothing else.
                    let e = coreutils::stdfd::close_descriptor(0).err().unwrap_or(e);
                    Fatal(format!("{}: {}", quotef(&name), strerror(&e)))
                })?,
            // SAFETY: descriptor 0 is standard input, open for the life of the
            // process; it stays in the `ManuallyDrop` and is never closed here.
            None => ManuallyDrop::new(unsafe { File::from_raw_fd(0) }),
        };
        let size = input_size(&file);
        Ok((Box::new(Stdin(file)), size))
    }

    fn input_size(file: &File) -> u64 {
        let Ok(meta) = file.metadata() else {
            return u64::MAX;
        };
        if !meta.is_file() {
            return u64::MAX;
        }
        match (&*file).stream_position() {
            Ok(at) => meta.len().saturating_sub(at),
            Err(_) => u64::MAX,
        }
    }

    /// `freopen (outfile, "w", stdout)`.
    pub fn redirect_stdout(name: &[u8]) -> Result<(), Fatal> {
        let fail = |e: &io::Error| Fatal(format!("{}: {}", quotef(name), strerror(e)));
        let file = File::create(os_from_bytes(name)).map_err(|e| fail(&e))?;
        // SAFETY: `file` is open and owned here; `dup2` makes descriptor 1 a
        // second reference to it, and `file` then closes its own.
        if unsafe { libc_dup2(file.as_raw_fd(), 1) } < 0 {
            return Err(fail(&io::Error::last_os_error()));
        }
        Ok(())
    }
}

#[cfg(not(unix))]
mod imp {
    use super::Fatal;
    use std::ffi::OsString;
    use std::io::Read;

    pub fn open_input(_operand: Option<&OsString>) -> Result<(Box<dyn Read>, u64), Fatal> {
        Err(Fatal(
            "unix-only utility; not supported on this platform".to_string(),
        ))
    }

    pub fn redirect_stdout(_name: &[u8]) -> Result<(), Fatal> {
        Err(Fatal(
            "unix-only utility; not supported on this platform".to_string(),
        ))
    }
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

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_range_is_two_numbers_and_may_be_empty() {
        assert_eq!(input_range(b"1-10").unwrap(), (1, 10));
        assert_eq!(input_range(b"5-4").unwrap(), (5, 4));
        assert!(input_range(b"6-4").is_err());
        assert!(input_range(b"1").is_err());
        assert!(input_range(b"1-").is_err());
        assert!(input_range(b"1-2x").is_err());
        assert!(input_range(b"-1-2").is_err());
        assert!(input_range(b"0-18446744073709551615").is_err());
        let e = input_range(b"1-99999999999999999999999").unwrap_err();
        assert!(e.sentence.ends_with(EOVERFLOW_TEXT), "{}", e.sentence);
    }

    #[test]
    fn the_operand_checks_are_upstreams() {
        let e = parse_args(&argv(&["-e", "-i", "1-2"])).unwrap_err();
        assert_eq!(e.sentence, "cannot combine -e and -i options");
        assert!(e.referral.is_some());
        let e = parse_args(&argv(&["a", "b"])).unwrap_err();
        assert!(e.sentence.starts_with("extra operand "), "{}", e.sentence);
        let e = parse_args(&argv(&["-i", "1-2", "a"])).unwrap_err();
        assert!(e.sentence.starts_with("extra operand "), "{}", e.sentence);
        assert!(parse_args(&argv(&["-e", "a", "b", "c"])).is_ok());
        let e = parse_args(&argv(&["-o", "x", "-o", "y"])).unwrap_err();
        assert_eq!(e.sentence, "multiple output files specified");
        assert!(parse_args(&argv(&["-o", "x", "-o", "x"])).is_ok());
        let Request::Run(s) =
            parse_args(&argv(&["-n", "5", "-n", "3", "-n", "99999999999999999999"])).unwrap()
        else {
            panic!()
        };
        assert_eq!(s.head_lines, 3);
    }

    #[test]
    fn a_line_without_its_terminator_gains_one() {
        let lines = read_input(&mut &b"a\nb"[..], b'\n').unwrap();
        assert_eq!(lines, vec![b"a\n".to_vec(), b"b\n".to_vec()]);
        assert!(read_input(&mut &b""[..], b'\n').unwrap().is_empty());
        let lines = read_input(&mut &b"x\0y\0"[..], 0).unwrap();
        assert_eq!(lines, vec![b"x\0".to_vec(), b"y\0".to_vec()]);
    }
}
