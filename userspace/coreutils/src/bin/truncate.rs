//! `truncate` — shrink or extend the size of a file to the specified size.
//!
//! ```text
//! Usage: truncate OPTION... FILE...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/truncate.c`. There was no `truncate`
//! on the system; scripts that preallocate, empty or trim a file with it --
//! `truncate -s 0 log`, `truncate -s 1G disk.img` -- got `command not found`.
//!
//! # The SIZE grammar
//!
//! `[<>/%][+-]NUMBER[SUFFIX]`, parsed as upstream parses it:
//!
//! | prefix | meaning |
//! |---|---|
//! | none | exactly |
//! | `+` / `-` | extend / reduce by |
//! | `<` / `>` | at most / at least |
//! | `/` / `%` | round down / up to a multiple of |
//!
//! The number goes through gnulib's `xdectoimax` with dd's suffixes
//! (`EgGkKmMPQRtTYZ0`: `K` is 1024, `KB` 1000, `KiB` 1024, and so on), which is
//! [`coreutils::xnum::xdectoimax`] here.
//!
//! # Quirks kept because they are observable
//!
//! * **The relative mode outlives its `-s`.** Upstream never resets
//!   `rel_mode` between options, so `-s +5 -s 3` extends by 3, and `-s +5 -s
//!   +3` is `multiple relative modifiers specified`.
//! * **`division by zero`** is refused the moment `-s /0` or `-s %0` is read,
//!   with no pointer to `--help` and before any other check.
//! * **`-c` silences only `ENOENT`.** `truncate -c -s0 missing` is quiet and
//!   succeeds; `truncate -c -s0 nodir/f`, where the directory is what is
//!   missing, is also `ENOENT` and also quiet -- upstream's test is on errno,
//!   not on which component was absent.
//! * **The four argument checks run in a fixed order** -- no size or
//!   reference, a reference with an absolute size, `-o` without `-s`, no file
//!   -- so a line breaking several rules names the first.
//! * **Every file is attempted**, and a failure on one does not stop the
//!   next.
//!
//! # Checked against GNU
//!
//! `scripts/truncate-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::xnum::xdectoimax;
use std::ffi::OsString;

coreutils::guard_std_fds!();

const TRUNCATE: Program = Program::new("truncate", 1);

/// Upstream's `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "cor:s:";

/// Upstream's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("no-create", Takes::Nothing),
    ("io-blocks", Takes::Nothing),
    ("reference", Takes::Required),
    ("size", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The suffixes `-s` accepts: dd's block-size set, plus lower-case `g`, `m`
/// and `t` for BSD's sake, and `0` for the `B`/`iB` second letter.
const SIZE_SUFFIXES: &[u8] = b"EgGkKmMPQRtTYZ0";

/// upstream's `rel_mode_t`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rel {
    /// Exactly the size given.
    Abs,
    /// `+`/`-`: by the size given.
    By,
    /// `>`: at least.
    Min,
    /// `<`: at most.
    Max,
    /// `/`: round down to a multiple.
    RoundDown,
    /// `%`: round up to a multiple.
    RoundUp,
}

/// Everything the command line decided.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Plan {
    no_create: bool,
    block_mode: bool,
    reference: Option<OsString>,
    /// `-s`'s number, when there was one.
    size: Option<i64>,
    rel: Rel,
    files: Vec<OsString>,
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Plan),
}

/// How a command-line problem is reported: getopt's and upstream's
/// `error (0, …); usage (1)` both refer to `--help`; `division by zero` is an
/// `error (EXIT_FAILURE, …)` and does not.
#[derive(Debug, PartialEq, Eq)]
enum Refusal {
    Referring(getopt::Error),
    Bare(String),
}

fn help_text() -> String {
    "\
Usage: truncate OPTION... FILE...
Shrink or extend the size of each FILE to the specified size

A FILE argument that does not exist is created.

If a FILE is larger than the specified size, the extra data is lost.
If a FILE is shorter, it is extended and the sparse extended part (hole)
reads as zero bytes.

Mandatory arguments to long options are mandatory for short options too.
  -c, --no-create        do not create any files
  -o, --io-blocks        treat SIZE as number of IO blocks instead of bytes
  -r, --reference=RFILE  base size on RFILE
  -s, --size=SIZE        set or adjust the file size by SIZE bytes
      --help        display this help and exit
      --version     output version information and exit

The SIZE argument is an integer and optional unit (example: 10K is 10*1024).
Units are K,M,G,T,P,E,Z,Y,R,Q (powers of 1024) or KB,MB,... (powers of 1000).
Binary prefixes can be used, too: KiB=K, MiB=M, and so on.

SIZE may also be prefixed by one of the following modifying characters:
'+' extend by, '-' reduce by, '<' at most, '>' at least,
'/' round down to multiple of, '%' round up to multiple of.
"
    .to_string()
}

/// C's `isspace` in the C locale: space, `\t`, `\n`, `\v`, `\f`, `\r`.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// One `-s` argument, as upstream's `case 's'` reads it. `rel` is the mode so
/// far, which the argument may change -- and which it must not change twice.
fn parse_size(arg: &[u8], rel: &mut Rel) -> Result<i64, Refusal> {
    let mut at = arg.iter().take_while(|&&b| is_c_space(b)).count();
    let modifier = match arg.get(at) {
        Some(b'<') => Some(Rel::Max),
        Some(b'>') => Some(Rel::Min),
        Some(b'/') => Some(Rel::RoundDown),
        Some(b'%') => Some(Rel::RoundUp),
        _ => None,
    };
    if let Some(m) = modifier {
        *rel = m;
        at = at.saturating_add(1);
    }
    let rest = arg.get(at..).unwrap_or_default();
    at = at.saturating_add(rest.iter().take_while(|&&b| is_c_space(b)).count());
    let number = arg.get(at..).unwrap_or_default();
    if matches!(number.first(), Some(b'+' | b'-')) {
        if *rel != Rel::Abs {
            return Err(Refusal::Referring(TRUNCATE.usage_referring(
                "multiple relative modifiers specified".to_string(),
            )));
        }
        *rel = Rel::By;
    }
    let size = xdectoimax(
        number,
        i64::MIN,
        i64::MAX,
        Some(SIZE_SUFFIXES),
        "Invalid number",
    )
    .map_err(Refusal::Bare)?;
    if matches!(*rel, Rel::RoundUp | Rel::RoundDown) && size == 0 {
        return Err(Refusal::Bare("division by zero".to_string()));
    }
    Ok(size)
}

/// Parse and check the command line, in upstream's order.
fn parse_args(args: &[OsString]) -> Result<Request, Refusal> {
    let mut plan = Plan {
        no_create: false,
        block_mode: false,
        reference: None,
        size: None,
        rel: Rel::Abs,
        files: Vec::new(),
    };
    for item in TRUNCATE.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item.map_err(Refusal::Referring)? {
            Opt::Short(b'c', _) | Opt::Long("no-create", _) => plan.no_create = true,
            Opt::Short(b'o', _) | Opt::Long("io-blocks", _) => plan.block_mode = true,
            Opt::Short(b'r', Some(v)) | Opt::Long("reference", Some(v)) => {
                plan.reference = Some(v.clone());
            }
            Opt::Short(b's', Some(v)) | Opt::Long("size", Some(v)) => {
                plan.size = Some(parse_size(&os_bytes(&v), &mut plan.rel)?);
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(f) => plan.files.push(f.clone()),
            // Unreachable: the parser supplies a value for every option that
            // takes one, and yields only names from the tables.
            Opt::Long(other, _) => {
                return Err(Refusal::Referring(
                    TRUNCATE.usage_referring(format!("option '--{other}' is unhandled")),
                ));
            }
            Opt::Short(c, _) => return Err(Refusal::Referring(TRUNCATE.invalid_option(c))),
        }
    }
    let q = |s: &str| quote(s.as_bytes());
    let referring = |msg: String| Err(Refusal::Referring(TRUNCATE.usage_referring(msg)));
    if plan.reference.is_none() && plan.size.is_none() {
        return referring(format!(
            "you must specify either {} or {}",
            q("--size"),
            q("--reference")
        ));
    }
    if plan.reference.is_some() && plan.size.is_some() && plan.rel == Rel::Abs {
        return referring(format!(
            "you must specify a relative {} with {}",
            q("--size"),
            q("--reference")
        ));
    }
    if plan.block_mode && plan.size.is_none() {
        return referring(format!(
            "{} was specified but {} was not",
            q("--io-blocks"),
            q("--size")
        ));
    }
    if plan.files.is_empty() {
        return referring("missing file operand".to_string());
    }
    Ok(Request::Run(plan))
}

/// Upstream's size arithmetic, once the file's current size and block size
/// are known: what to `ftruncate` to, or the complaint that stops it.
///
/// `fsize` is the size the relative modes are relative to -- the reference
/// file's, when there is one. `blksize` is `ST_BLKSIZE`, already defaulted.
fn new_size(ssize: i64, rel: Rel, fsize: i64, block_mode: Option<i64>) -> Result<i64, NewSize> {
    let mut ssize = ssize;
    if let Some(blksize) = block_mode {
        ssize = ssize
            .checked_mul(blksize)
            .ok_or(NewSize::BlockOverflow { ssize, blksize })?;
    }
    let nsize = match rel {
        Rel::Abs => ssize,
        Rel::Min => fsize.max(ssize),
        Rel::Max => fsize.min(ssize),
        // `ssize` is never 0 here: `parse_size` refused a zero divisor.
        Rel::RoundDown => fsize
            .checked_sub(fsize.checked_rem(ssize).unwrap_or(0))
            .unwrap_or(fsize),
        Rel::RoundUp => {
            let r = fsize.checked_rem(ssize).unwrap_or(0);
            let up = if r == 0 { 0 } else { ssize.saturating_sub(r) };
            fsize.checked_add(up).ok_or(NewSize::ExtendOverflow)?
        }
        Rel::By => fsize.checked_add(ssize).ok_or(NewSize::ExtendOverflow)?,
    };
    Ok(nsize.max(0))
}

/// The two overflow complaints of `do_ftruncate`.
#[derive(Debug, PartialEq, Eq)]
enum NewSize {
    BlockOverflow { ssize: i64, blksize: i64 },
    ExtendOverflow,
}

#[cfg(unix)]
mod imp {
    use super::{NewSize, Plan, Rel, Request, help_text, new_size, parse_args};
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::quoteaf_os;
    use coreutils::stdfd::{self, Stream};
    use std::ffi::{OsStr, OsString};
    use std::fs::{File, Metadata, OpenOptions};
    use std::io::{self, Seek, SeekFrom, Write};
    use std::os::fd::IntoRawFd;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    use std::process::ExitCode;

    /// `O_NONBLOCK`, which is the same number on Linux and here.
    const O_NONBLOCK: i32 = 0o4000;
    /// gnulib's `DEV_BSIZE`, the block size when `st_blksize` is unusable.
    const DEV_BSIZE: i64 = 512;

    unsafe extern "C" {
        fn close(fd: i32) -> i32;
    }

    /// gnulib's `usable_st_size`: only a regular file's `st_size` is its length.
    fn usable_st_size(m: &Metadata) -> bool {
        m.file_type().is_file()
    }

    /// gnulib's `ST_BLKSIZE`: `st_blksize` when it is sane, else `DEV_BSIZE`.
    fn st_blksize(m: &Metadata) -> i64 {
        match i64::try_from(m.blksize()) {
            Ok(b) if b > 0 && b.unsigned_abs() <= (usize::MAX / 8).saturating_add(1) as u64 => b,
            _ => DEV_BSIZE,
        }
    }

    /// The reference file's size, as upstream's `main` reads it.
    fn reference_size(rfile: &OsStr) -> Result<i64, ExitCode> {
        let meta = match std::fs::metadata(rfile) {
            Ok(m) => m,
            Err(e) => {
                diag!(
                    "truncate: cannot stat {}: {}",
                    quoteaf_os(rfile),
                    strerror(&e)
                );
                return Err(ExitCode::FAILURE);
            }
        };
        if usable_st_size(&meta) {
            if let Ok(size) = i64::try_from(meta.size()) {
                return Ok(size);
            }
        }
        let end = File::open(rfile).and_then(|mut f| f.seek(SeekFrom::End(0)));
        match end.map(i64::try_from) {
            Ok(Ok(size)) => Ok(size),
            Ok(Err(_)) => {
                diag!("truncate: cannot get the size of {}", quoteaf_os(rfile));
                Err(ExitCode::FAILURE)
            }
            Err(e) => {
                diag!(
                    "truncate: cannot get the size of {}: {}",
                    quoteaf_os(rfile),
                    strerror(&e)
                );
                Err(ExitCode::FAILURE)
            }
        }
    }

    /// Upstream's `do_ftruncate`.
    fn do_ftruncate(
        f: &mut File,
        fname: &OsStr,
        plan: &Plan,
        ssize: i64,
        rsize: Option<i64>,
    ) -> bool {
        let need_stat = plan.block_mode || (plan.rel != Rel::Abs && rsize.is_none());
        let meta = if need_stat {
            match f.metadata() {
                Ok(m) => Some(m),
                Err(e) => {
                    diag!(
                        "truncate: cannot fstat {}: {}",
                        quoteaf_os(fname),
                        strerror(&e)
                    );
                    return false;
                }
            }
        } else {
            None
        };

        let fsize = if plan.rel == Rel::Abs {
            0
        } else if let Some(r) = rsize {
            r
        } else if let Some(m) = meta.as_ref().filter(|m| usable_st_size(m)) {
            match i64::try_from(m.size()) {
                Ok(s) => s,
                Err(_) => {
                    diag!(
                        "truncate: {} has unusable, apparently negative size",
                        quoteaf_os(fname)
                    );
                    return false;
                }
            }
        } else {
            match f.seek(SeekFrom::End(0)).map(i64::try_from) {
                Ok(Ok(s)) => s,
                Ok(Err(_)) => {
                    diag!("truncate: cannot get the size of {}", quoteaf_os(fname));
                    return false;
                }
                Err(e) => {
                    diag!(
                        "truncate: cannot get the size of {}: {}",
                        quoteaf_os(fname),
                        strerror(&e)
                    );
                    return false;
                }
            }
        };

        let blocks = if plan.block_mode {
            meta.as_ref().map(st_blksize)
        } else {
            None
        };
        let nsize = match new_size(ssize, plan.rel, fsize, blocks) {
            Ok(n) => n,
            Err(NewSize::BlockOverflow { ssize, blksize }) => {
                diag!(
                    "truncate: overflow in {ssize} * {blksize} byte blocks for file {}",
                    quoteaf_os(fname)
                );
                return false;
            }
            Err(NewSize::ExtendOverflow) => {
                diag!(
                    "truncate: overflow extending size of file {}",
                    quoteaf_os(fname)
                );
                return false;
            }
        };
        if let Err(e) = f.set_len(nsize.unsigned_abs()) {
            diag!(
                "truncate: failed to truncate {} at {nsize} bytes: {}",
                quoteaf_os(fname),
                strerror(&e)
            );
            return false;
        }
        true
    }

    /// `ENOENT`, which `-c` forgives.
    fn is_enoent(e: &io::Error) -> bool {
        e.kind() == io::ErrorKind::NotFound
    }

    fn run(plan: &Plan) -> ExitCode {
        let (ssize, rsize) = match (&plan.reference, plan.size) {
            (Some(r), size) => match reference_size(r) {
                Ok(fsize) => match size {
                    Some(s) => (s, Some(fsize)),
                    None => (fsize, None),
                },
                Err(code) => return code,
            },
            (None, Some(s)) => (s, None),
            // Unreachable: `parse_args` refused a line with neither.
            (None, None) => return ExitCode::FAILURE,
        };
        let mut errors = false;
        for fname in &plan.files {
            let opened = OpenOptions::new()
                .write(true)
                .create(!plan.no_create)
                .custom_flags(O_NONBLOCK)
                .open(fname);
            match opened {
                Err(e) => {
                    if !(plan.no_create && is_enoent(&e)) {
                        diag!(
                            "truncate: cannot open {} for writing: {}",
                            quoteaf_os(fname),
                            strerror(&e)
                        );
                        errors = true;
                    }
                }
                Ok(mut f) => {
                    errors |= !do_ftruncate(&mut f, fname, plan, ssize, rsize);
                    // SAFETY: `into_raw_fd` gives up `f`'s ownership, so this
                    // is the one close of the descriptor.
                    if unsafe { close(f.into_raw_fd()) } != 0 {
                        diag!(
                            "truncate: failed to close {}: {}",
                            quoteaf_os(fname),
                            strerror(&io::Error::last_os_error())
                        );
                        errors = true;
                    }
                }
            }
        }
        if errors {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(super::Refusal::Referring(e)) => {
                super::TRUNCATE.report(&e);
                return ExitCode::FAILURE;
            }
            Err(super::Refusal::Bare(msg)) => {
                diag!("truncate: {msg}");
                return ExitCode::FAILURE;
            }
        };
        let mut out = Stream::stdout();
        let earned = match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
                ExitCode::SUCCESS
            }
            Request::Version => {
                let _ = out.write_all(b"truncate (SlateOS coreutils) 0.1.0\n");
                ExitCode::SUCCESS
            }
            Request::Run(plan) => run(&plan),
        };
        stdfd::close_stdout("truncate", out, earned)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("truncate: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn plan(args: &[&str]) -> Plan {
        match parse_args(&argv(args)).unwrap() {
            Request::Run(p) => p,
            other => panic!("{other:?}"),
        }
    }

    fn refusal(args: &[&str]) -> String {
        match parse_args(&argv(args)).unwrap_err() {
            Refusal::Referring(e) => e.message(),
            Refusal::Bare(m) => format!("BARE {m}"),
        }
    }

    #[test]
    fn size_prefixes_set_the_mode() {
        for (arg, rel, size) in [
            ("5", Rel::Abs, 5),
            ("+5", Rel::By, 5),
            ("-5", Rel::By, -5),
            ("<5", Rel::Max, 5),
            (">5", Rel::Min, 5),
            ("/5", Rel::RoundDown, 5),
            ("%5", Rel::RoundUp, 5),
            (" > 5", Rel::Min, 5),
            ("1K", Rel::Abs, 1024),
            ("1KB", Rel::Abs, 1000),
            ("1KiB", Rel::Abs, 1024),
            ("2m", Rel::Abs, 2 << 20),
        ] {
            let p = plan(&["-s", arg, "f"]);
            assert_eq!((p.rel, p.size), (rel, Some(size)), "{arg:?}");
        }
    }

    #[test]
    fn the_relative_mode_outlives_its_option() {
        let p = plan(&["-s", "+5", "-s", "3", "f"]);
        assert_eq!((p.rel, p.size), (Rel::By, Some(3)));
        assert_eq!(
            refusal(&["-s", "+5", "-s", "+3", "f"]),
            "multiple relative modifiers specified\nTry 'truncate --help' for more information."
        );
        assert_eq!(
            refusal(&["-s", "<+3", "f"]),
            "multiple relative modifiers specified\nTry 'truncate --help' for more information."
        );
    }

    #[test]
    fn a_zero_divisor_is_refused_at_once_and_bare() {
        assert_eq!(refusal(&["-s", "/0"]), "BARE division by zero");
        assert_eq!(refusal(&["-s", "%0", "f"]), "BARE division by zero");
    }

    #[test]
    fn a_bad_number_is_xdectoimaxs_wording() {
        assert_eq!(
            refusal(&["-s", "abc", "f"]),
            "BARE Invalid number: \u{2018}abc\u{2019}"
        );
    }

    #[test]
    fn the_argument_checks_run_in_upstreams_order() {
        assert_eq!(
            refusal(&["f"]),
            "you must specify either \u{2018}--size\u{2019} or \u{2018}--reference\u{2019}\n\
             Try 'truncate --help' for more information."
        );
        assert_eq!(
            refusal(&["-r", "r", "-s", "5", "f"]),
            "you must specify a relative \u{2018}--size\u{2019} with \u{2018}--reference\u{2019}\n\
             Try 'truncate --help' for more information."
        );
        assert_eq!(
            refusal(&["-o", "-r", "r", "f"]),
            "\u{2018}--io-blocks\u{2019} was specified but \u{2018}--size\u{2019} was not\n\
             Try 'truncate --help' for more information."
        );
        assert_eq!(
            refusal(&["-s", "5"]),
            "missing file operand\nTry 'truncate --help' for more information."
        );
    }

    #[test]
    fn the_size_arithmetic() {
        assert_eq!(new_size(5, Rel::Abs, 100, None), Ok(5));
        assert_eq!(new_size(5, Rel::By, 100, None), Ok(105));
        assert_eq!(
            new_size(-500, Rel::By, 100, None),
            Ok(0),
            "never below zero"
        );
        assert_eq!(new_size(50, Rel::Min, 100, None), Ok(100));
        assert_eq!(new_size(50, Rel::Max, 100, None), Ok(50));
        assert_eq!(new_size(30, Rel::RoundDown, 100, None), Ok(90));
        assert_eq!(new_size(30, Rel::RoundUp, 100, None), Ok(120));
        assert_eq!(
            new_size(25, Rel::RoundUp, 100, None),
            Ok(100),
            "already a multiple"
        );
        assert_eq!(new_size(3, Rel::Abs, 0, Some(512)), Ok(1536));
        assert_eq!(
            new_size(i64::MAX, Rel::Abs, 0, Some(512)),
            Err(NewSize::BlockOverflow {
                ssize: i64::MAX,
                blksize: 512
            })
        );
        assert_eq!(
            new_size(1, Rel::By, i64::MAX, None),
            Err(NewSize::ExtendOverflow)
        );
    }
}
