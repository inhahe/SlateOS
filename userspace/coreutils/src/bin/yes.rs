//! `yes` — output a string repeatedly until killed.
//!
//! A port of GNU coreutils 9.4's `src/yes.c`, measured against the real binary
//! rather than recalled. The shipped version joined `argv` with spaces and
//! printed the result forever, which is the right *output* and almost nothing
//! else. Three things were wrong with it, in rising order of how often they
//! bite:
//!
//! 1. **`argv` was `Vec<String>`.** `env::args()` panics on the first byte that
//!    is not valid UTF-8, and this OS allows every byte but `/` and NUL. The
//!    program whose entire job is to repeat the bytes it was handed could not
//!    be handed arbitrary bytes.
//! 2. **`--help`, `--version` and option rejection did not exist.** `yes
//!    --help` printed `--help` forever, and so did `yes --oops`. Upstream calls
//!    `parse_gnu_standard_options_only`, which accepts exactly those two and
//!    treats anything else beginning with `-` as an error — `yes -x` is
//!    `invalid option -- 'x'`, status 1, not an operand.
//! 3. **One `write` syscall per line.** For the one utility in the tree whose
//!    purpose is throughput. Upstream measures 1.9 GB/s here; a line at a time
//!    is a kernel round trip per two bytes.
//!
//! # Why the buffer may be repeated but not truncated
//!
//! The output is an endless repetition of one record, so any buffer holding a
//! *whole number* of records writes the identical byte stream — which is what
//! makes upstream's trick sound. [`fill`] doubles the record in place until it
//! reaches [`BUFSIZ`], so the buffer is always `record.len() * 2^k` and the
//! doubling can never split a record across a write. Rounding to exactly
//! `BUFSIZ` instead would be the obvious optimisation and would be wrong at
//! every buffer boundary.
//!
//! # A broken pipe is the normal ending, not a failure
//!
//! `yes | head -1` is what this program is for. GNU dies there of `SIGPIPE`,
//! printing nothing; SlateOS has no Unix signals for process control
//! (`design.txt`) and Rust masks the signal in any case, so the same situation
//! arrives as `EPIPE` and has to be recognised and kept quiet — exactly as
//! `cut`, `head`, `tail` and `uniq` in this tree already do. Any *other* write
//! failure is upstream's `error (EXIT_FAILURE, errno, _("standard output"))`,
//! which is why `yes > /dev/full` says
//! `yes: standard output: No space left on device` and exits 1.
//!
//! The one visible difference from GNU: a shell reports 141 for a
//! `SIGPIPE`-killed `yes` and 0 for ours. There is no signal to report.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::process::ExitCode;

// Before `main`, so that `yes >&-` still has a closed descriptor to fail
// against. Without it this program does not merely misreport — it never ends:
// the runtime hands it /dev/null and it writes `y` into the void for as long as
// the machine is on. See `coreutils::stdfd`.
coreutils::guard_std_fds!();

/// `yes -x; echo $?` is 1. Measured, not assumed: `ls`, `sort` and `grep` are 2.
const YES: Program = Program::new("yes", 1);

/// Upstream has no short options at all — but it still *parses* them, which is
/// why `yes -x` is a diagnostic rather than a string to repeat.
const SHORT_OPTIONS: &str = "";

/// The two `parse_gnu_standard_options_only` accepts, in the order it registers
/// them — observable through `yes --=x`, which lists
/// `'--help' '--version'`. `scripts/getopt-ambiguity-check.py` compares this
/// table against the real binary's.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// glibc's `BUFSIZ`, the size upstream grows its buffer towards.
const BUFSIZ: usize = 8192;

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    /// The operands as they were typed. Empty means the default `y`.
    ///
    /// Kept as `OsString` rather than converted to bytes here so that the
    /// conversion happens once, at the point of output. On the target the two
    /// are the same thing; on a Windows *development host* `os_bytes` is
    /// documented-lossy, and converting early would mean the parser's own tests
    /// could not tell a preserved argument from a mangled one.
    Run(Vec<OsString>),
}

fn main() -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    stdfd::close_stderr(run_main(), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => say(help_text().as_bytes()),
        Ok(Request::Version) => say(b"yes (SlateOS coreutils) 0.1.0\n"),
        Ok(Request::Run(operands)) => repeat(&fill(&record(&operands))),
        Err(e) => {
            YES.report(&e);
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

/// Read the command line.
///
/// Options are recognised wherever they appear, operands and all: `yes a
/// --help` prints the help, because glibc permutes and upstream does not pass
/// `+`. Only `--` stops that.
///
/// # Errors
///
/// Any getopt diagnostic — an unknown option, an ambiguous abbreviation, or an
/// argument given to an option that takes none.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut operands: Vec<OsString> = Vec::new();
    for item in YES.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => operands.push(word.clone()),
            // `SHORT_OPTIONS` is empty and every long option is handled above;
            // anything else arrives as an `Err` from `parse`.
            Opt::Short(..) | Opt::Long(..) => {}
        }
    }
    Ok(Request::Run(operands))
}

/// GNU's `--help`, minus the project's `Report bugs to:` block, as every
/// converted utility here omits it.
fn help_text() -> String {
    "\
Usage: yes [STRING]...
  or:  yes OPTION
Repeatedly output a line with all specified STRING(s), or 'y'.

      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ------------------------------------------------------------------ output ---

/// The line that will be repeated: the operands joined by one space, then a
/// newline. No operands is `y`.
///
/// Note this is a join and not a "print each": an empty operand contributes
/// nothing but still brings its separator, so `yes a "" b` repeats `a  b`.
fn record(operands: &[OsString]) -> Vec<u8> {
    let mut line = Vec::new();
    if operands.is_empty() {
        line.extend_from_slice(b"y");
    } else {
        for (i, word) in operands.iter().enumerate() {
            if i > 0 {
                line.push(b' ');
            }
            line.extend_from_slice(&os_bytes(word));
        }
    }
    line.push(b'\n');
    line
}

/// Repeat `record` until the buffer is at least [`BUFSIZ`], so that one write
/// carries many lines.
///
/// Doubling — rather than filling to a byte count — is what keeps the buffer an
/// exact multiple of the record, which is the invariant that makes writing the
/// buffer over and over produce the same stream as writing the record over and
/// over. A record already at or over `BUFSIZ` is returned as it is: there is
/// nothing to amortise, and doubling a large record only wastes memory.
fn fill(record: &[u8]) -> Vec<u8> {
    let mut buf = record.to_vec();
    if buf.is_empty() {
        return buf; // unreachable: `record` always ends in `\n`.
    }
    while buf.len() < BUFSIZ {
        buf.extend_from_within(..);
    }
    buf
}

/// Say one thing and stop — `--help` and `--version`.
///
/// These two go through a *buffered* stream and are diagnosed by
/// `close_stdout`'s wording, not the loop's, which is why they say
/// `yes: write error: …` where `yes >&-` says `yes: standard output: …`.
/// Upstream has the same split for the same reason: `usage` prints through
/// stdio and the atexit hook reports it, while the loop below writes to the
/// descriptor itself and reports its own failure.
fn say(bytes: &[u8]) -> ExitCode {
    let mut out = Stream::stdout();
    // `Stream::write_all` records rather than returns; the verdict is `finish`.
    let _ = out.write_all(bytes);
    if let Err(e) = out.finish() {
        stdfd::write_error("yes", &e);
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Write `buf` forever.
///
/// Not reachable from the tests below — what it does is make syscalls — which
/// is why everything that decides *what* it writes is a separate function.
///
/// Upstream is `while (full_write (STDOUT_FILENO, buf, bufused) == bufused)
/// continue;` — the descriptor, not the stream, and no buffer of its own,
/// because `buf` is already `BUFSIZ` bytes and buffering it again would only
/// copy it. [`stdfd::write_all`] is that `full_write`, and using it rather than
/// `io::stdout()` is what makes this loop *stop*: std reopens a closed
/// descriptor on /dev/null and then reports `EBADF` as a completed write, so
/// `yes >&-` through std has no failure to end on.
fn repeat(buf: &[u8]) -> ExitCode {
    loop {
        if let Err(e) = stdfd::write_all(1, buf) {
            // The normal ending: the reader went away. GNU is killed by
            // SIGPIPE here and prints nothing, so neither do we.
            if e.kind() == ErrorKind::BrokenPipe {
                return ExitCode::SUCCESS;
            }
            diag!("yes: standard output: {}", strerror(&e));
            return ExitCode::from(1);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn run_of(items: &[&str]) -> Vec<OsString> {
        match parse_args(&argv(items)).unwrap() {
            Request::Run(v) => v,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn line(items: &[&str]) -> String {
        String::from_utf8(record(&run_of(items))).unwrap()
    }

    // ---------------- the line that gets repeated ----------------

    #[test]
    fn default_is_y() {
        assert_eq!(line(&[]), "y\n");
    }

    #[test]
    fn single_word_used_directly() {
        assert_eq!(line(&["no"]), "no\n");
    }

    #[test]
    fn multiple_args_joined_with_space() {
        assert_eq!(line(&["a", "b", "c"]), "a b c\n");
    }

    #[test]
    fn empty_string_arg_still_brings_its_separator() {
        // Measured: `yes a "" b` repeats `a  b` — two spaces.
        assert_eq!(line(&["a", "", "b"]), "a  b\n");
    }

    #[test]
    fn a_single_empty_arg_is_a_blank_line() {
        // And not the default `y`: what is empty is the *operand*, not the
        // operand list.
        assert_eq!(line(&[""]), "\n");
    }

    #[test]
    fn args_with_spaces_inside_kept() {
        assert_eq!(line(&["hello world", "x"]), "hello world x\n");
    }

    #[test]
    fn unicode_passed_through() {
        assert_eq!(line(&["κόσμε"]), "κόσμε\n");
    }

    #[test]
    fn a_lone_dash_is_an_operand() {
        assert_eq!(line(&["a", "-", "b"]), "a - b\n");
    }

    // ---------------- options ----------------

    #[test]
    fn help_and_version_are_recognised() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn options_are_recognised_after_operands() {
        // glibc permutes and upstream does not pass `+`, so `yes a --help`
        // prints the help rather than repeating `a --help`.
        assert_eq!(parse_args(&argv(&["a", "--help"])).unwrap(), Request::Help);
    }

    #[test]
    fn unambiguous_abbreviations_resolve() {
        // `--h` and `--v` each match exactly one entry.
        assert_eq!(parse_args(&argv(&["--h"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--v"])).unwrap(), Request::Version);
    }

    #[test]
    fn double_dash_ends_the_options() {
        assert_eq!(line(&["--", "--help"]), "--help\n");
        assert_eq!(line(&["--", "-x"]), "-x\n");
        // A bare `--` leaves no operands at all, so the default returns.
        assert_eq!(line(&["--"]), "y\n");
    }

    #[test]
    fn an_unknown_short_option_is_an_error_not_a_string() {
        // The one that mattered: the old version repeated `-x` forever.
        let e = parse_args(&argv(&["-x"])).unwrap_err();
        assert!(e.message().contains("invalid option -- 'x'"), "{e}");
        assert!(e.message().contains("Try 'yes --help'"), "{e}");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn an_unknown_long_option_is_an_error() {
        let e = parse_args(&argv(&["--nope"])).unwrap_err();
        assert!(e.message().contains("unrecognized option '--nope'"), "{e}");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn an_ambiguous_prefix_lists_the_table_in_order() {
        // `yes --=x` is the probe that makes the table's order observable.
        let e = parse_args(&argv(&["--=x"])).unwrap_err();
        assert!(
            e.message()
                .contains("option '--=x' is ambiguous; possibilities: '--help' '--version'"),
            "{e}"
        );
    }

    #[test]
    fn help_takes_no_argument() {
        let e = parse_args(&argv(&["--help=x"])).unwrap_err();
        assert!(e.message().contains("doesn't allow an argument"), "{e}");
    }

    // ---------------- bytes, not text ----------------

    /// The whole reason for the conversion: an operand that is not valid UTF-8
    /// must reach the output unchanged rather than aborting the program.
    ///
    /// Only on the target, because only there can a byte survive the trip: a
    /// Windows host has no byte view of an `OsStr` that round-trips, so
    /// `os_bytes` is documented-lossy and `\xff` comes back as U+FFFD. The
    /// Windows twin below tests as much of this as that host can hold.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_reaches_the_output_unchanged() {
        use std::os::unix::ffi::OsStringExt;
        let arg = OsString::from_vec(b"na\xffme".to_vec());
        assert_eq!(record(&run_from(&[arg])), b"na\xffme\n");
    }

    /// The test above is `#[cfg(unix)]`, so on the development host the
    /// regression test for the bug this file was rewritten to fix would not run
    /// at all — which is the same blind spot that let the bug survive. Windows
    /// has its own argument no `String` can hold: an unpaired surrogate, which
    /// reaches the same `unwrap` in `env::args()` by a different route. What is
    /// checkable there is that the parser carries it through untouched; what
    /// the bytes become afterwards is a property of the host, not of `yes`.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let arg = OsString::from_wide(&[0x006e, 0x0061, 0xd800, 0x006d, 0x0065]);
        assert!(
            arg.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        assert_eq!(run_from(std::slice::from_ref(&arg)), vec![arg]);
    }

    /// [`run_of`] for arguments that cannot be written as `&str`.
    fn run_from(args: &[OsString]) -> Vec<OsString> {
        match parse_args(args).unwrap() {
            Request::Run(v) => v,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    // ---------------- the buffer ----------------

    #[test]
    fn the_buffer_is_a_whole_number_of_records() {
        for words in [&[][..], &["no"][..], &["a", "b", "c"][..], &[""][..]] {
            let rec = record(&argv(words));
            let buf = fill(&rec);
            assert_eq!(
                buf.len() % rec.len(),
                0,
                "buffer {} is not a multiple of record {}",
                buf.len(),
                rec.len()
            );
            for chunk in buf.chunks(rec.len()) {
                assert_eq!(chunk, &rec[..], "a copy in the buffer differs");
            }
        }
    }

    #[test]
    fn the_buffer_reaches_bufsiz() {
        assert!(fill(&record(&[])).len() >= BUFSIZ);
        assert!(fill(&record(&argv(&["hello"]))).len() >= BUFSIZ);
    }

    /// A record already larger than the target is written as it is. Doubling it
    /// would allocate twice the size for no fewer syscalls per byte.
    #[test]
    fn an_oversized_record_is_not_doubled() {
        let big = OsString::from("x".repeat(BUFSIZ * 2));
        let rec = record(&[big]);
        assert_eq!(fill(&rec).len(), rec.len());
    }
}
