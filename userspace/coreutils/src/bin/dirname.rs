//! `dirname` — output each NAME with its last component removed.
//!
//! # What was here before
//!
//! A nine-line `main` over `std::path::Path::parent`, and five defects:
//!
//! 1. **`dirname /` answered `.`**, where GNU answers `/`. Its own unit test
//!    asserted the wrong value, with a comment explaining that
//!    `Path::new("/").parent()` is `None` — a true statement about the host's
//!    library and no statement at all about what `dirname` prints.
//! 2. **Host path semantics.** `Path::parent` splits on `\` as well as `/` and
//!    understands drive prefixes. Our filesystem forbids only `/` and NUL, so
//!    `a\b` is a one-component name — but the old code answered `a`.
//! 3. **`Vec<String>` argv**, which panics on a name that is not UTF-8.
//! 4. **No options at all.** `-z`, `--help` and `--version` were file names,
//!    so `dirname --help` printed `.`.
//! 5. **`missing operand` had no `Try 'dirname --help'` referral**, which is
//!    the second line GNU prints for every usage error.
//!
//! The splitting itself now lives in [`coreutils::pathname`], which is a
//! transcription of gnulib's `dir_len`; this file is only the argv and output
//! layer around it. See that module for why the rules are less obvious than
//! they look — `dirname a/` is `.`, and `dirname ////a////b////` keeps every
//! one of its leading slashes.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::pathname::dir_name;
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `dirname`'s usage status is 1 — measured: `dirname; echo $?` prints 1.
const DIRNAME: Program = Program::new("dirname", 1);

/// GNU `dirname`'s `getopt_long` string, exactly.
///
/// No leading `+`, so options and operands may be interleaved and
/// `dirname a/b -z` sets the flag rather than asking about a file called
/// `-z`. `basename` differs here, and deliberately — see its module docs.
const SHORT_OPTIONS: &str = "z";

/// GNU `dirname`'s `longopts[]`, in its declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("zero", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// `zero`, then the operands.
    Run(bool, Vec<OsString>),
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Decided before the stream exists: upstream's `usage (EXIT_FAILURE)` never
    // reaches `atexit (close_stdout)` with anything buffered, so `dirname >&-`
    // prints only `dirname: missing operand` and no write error after it.
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("dirname: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `--help` and `--version` are writes like any other, so they fail like any
    // other: `dirname --help >&-` is `dirname: write error: Bad file
    // descriptor` and exits 1.
    let mut out = Stream::stdout();
    match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
        }
        Request::Version => {
            let _ = out.write_all(b"dirname (SlateOS coreutils) 0.1.0\n");
        }
        // A closed stdout must not be reported as success: `-z` output is
        // usually piped into `xargs -0`, and a pipe that went away mid-list
        // would otherwise look like a complete list.
        Request::Run(zero, names) => run(zero, &names, &mut out),
    }
    stdfd::close_stdout("dirname", out, ExitCode::SUCCESS)
}

fn help_text() -> String {
    "\
Usage: dirname [OPTION] NAME...
Output each NAME with its last non-slash component and trailing slashes
removed; if NAME contains no /'s, output '.' (meaning the current directory).

  -z, --zero     end each output line with NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

Examples:
  dirname /usr/bin/          -> \"/usr\"
  dirname dir1/str dir2/str  -> \"dir1\" followed by \"dir2\"
  dirname stdio.h            -> \".\"
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `dirname`'s argv.
///
/// # Errors
///
/// An unknown option, a long option resolving to none or to more than one of
/// the table's entries, a long option given a value it does not take, or no
/// operands at all.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut zero = false;
    let mut names: Vec<OsString> = Vec::new();

    for item in DIRNAME.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(name) => names.push(name.clone()),
            Opt::Short(b'z', _) | Opt::Long("zero", _) => zero = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(DIRNAME.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(DIRNAME.invalid_option(other)),
        }
    }

    if names.is_empty() {
        return Err(DIRNAME.usage_referring("missing operand".into()));
    }
    Ok(Request::Run(zero, names))
}

// ----------------------------------------------------------------- output ---

/// Print the directory part of every name.
///
/// `dirname` has no failure mode once argv is parsed — it consults no
/// filesystem, so every operand has an answer — which is why this returns
/// nothing and the exit status depends only on the flush.
fn run<W: Write>(zero: bool, names: &[OsString], out: &mut W) {
    let delimiter = if zero { b'\0' } else { b'\n' };
    for name in names {
        // Raw bytes: a file name is an arbitrary byte string, and rendering it
        // as text would corrupt any name that is not UTF-8.
        let _ = out.write_all(dir_name(&os_bytes(name.as_os_str())));
        let _ = out.write_all(&[delimiter]);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// Run and return stdout as bytes.
    fn go(args: &[&str]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        match parse_args(&argv(args)).unwrap() {
            Request::Run(zero, names) => run(zero, &names, &mut out),
            other => panic!("expected a run, got {other:?}"),
        }
        out
    }

    #[test]
    fn each_operand_gets_a_line() {
        assert_eq!(go(&["/usr/bin/cat"]), b"/usr/bin\n");
        assert_eq!(go(&["dir1/str", "dir2/str"]), b"dir1\ndir2\n");
        assert_eq!(go(&["stdio.h"]), b".\n");
    }

    /// The row the previous implementation got wrong, and asserted wrong.
    #[test]
    fn the_root_is_its_own_parent() {
        assert_eq!(go(&["/"]), b"/\n");
        assert_eq!(go(&["//"]), b"/\n");
        assert_eq!(go(&["/etc"]), b"/\n");
    }

    #[test]
    fn a_trailing_slash_is_ignored_not_stripped() {
        // Not `a`: the last component is `a`, and nothing precedes it.
        assert_eq!(go(&["a/"]), b".\n");
        assert_eq!(go(&["/usr/bin/"]), b"/usr\n");
    }

    #[test]
    fn a_backslash_is_part_of_the_name() {
        assert_eq!(go(&["a\\b"]), b".\n");
    }

    #[test]
    fn zero_terminates_with_nul_including_the_last() {
        assert_eq!(go(&["-z", "/a/b"]), b"/a\0");
        assert_eq!(go(&["-z", "/a/b", "/c/d"]), b"/a\0/c\0");
        assert_eq!(go(&["--zero", "/a/b"]), b"/a\0");
    }

    /// GNU's `getopt_long` string has no `+`, so an option after an operand is
    /// still an option — measured: `dirname a/b -z` prints `a\0` and nothing
    /// else, so the `-z` was not a second operand.
    #[test]
    fn options_may_follow_operands() {
        assert_eq!(go(&["a/b", "-z"]), b"a\0");
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(go(&["--", "-z"]), b".\n");
        assert_eq!(go(&["--", "a/-z"]), b"a\n");
    }

    #[test]
    fn no_operands_names_the_missing_thing_and_refers_on() {
        let e = parse_args(&argv(&[])).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "missing operand\nTry 'dirname --help' for more information."
        );
    }

    /// An operand is still an operand when it is empty, so this is *not* the
    /// missing-operand case: GNU prints `.` for it.
    #[test]
    fn the_empty_name_is_an_operand() {
        assert_eq!(go(&[""]), b".\n");
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
        // And they win over an operand, rather than being one.
        assert_eq!(
            parse_args(&argv(&["a/b", "--help"])).unwrap(),
            Request::Help
        );
    }

    #[test]
    fn an_unknown_option_is_refused_rather_than_taken_as_a_file() {
        let e = parse_args(&argv(&["-x", "a/b"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'dirname --help' for more information."
        );
        let e = parse_args(&argv(&["--nope", "a/b"])).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'dirname --help' for more information."
        );
    }

    /// `--zero` is the only long option that is not `--help`/`--version`, so a
    /// prefix of it is unambiguous however short.
    #[test]
    fn an_unambiguous_prefix_is_accepted() {
        assert_eq!(go(&["--z", "/a/b"]), b"/a\0");
        assert_eq!(go(&["--ze", "/a/b"]), b"/a\0");
    }

    #[test]
    fn a_value_given_to_a_flag_is_refused() {
        let e = parse_args(&argv(&["--zero=3", "a"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--zero' doesn't allow an argument\n\
             Try 'dirname --help' for more information."
        );
    }

    /// A name that is not UTF-8 must reach the output unchanged. On the host
    /// `OsString` is WTF-8 and cannot hold an arbitrary byte, so this is a
    /// Unix-only check.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_name_survives_byte_for_byte() {
        use std::os::unix::ffi::OsStringExt;
        let name = OsString::from_vec(b"/d/\x80\xff/leaf".to_vec());
        let mut out: Vec<u8> = Vec::new();
        run(false, &[name], &mut out);
        assert_eq!(out, b"/d/\x80\xff\n");
    }
}
