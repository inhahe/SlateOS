//! `basename` — strip the directory and an optional suffix from a name.
//!
//! # What was here before
//!
//! A twelve-line `main` over `std::path::Path::file_name`, and six defects:
//!
//! 1. **`to_string_lossy`** on the name — a name that is not UTF-8 came back
//!    with U+FFFD in place of the bytes that were there, which is the silent
//!    corruption the project forbids by name.
//! 2. **`Vec<String>` argv**, which panics before reaching that on the same
//!    input.
//! 3. **No options.** `-a`, `-s`, `-z`, `--help` and `--version` were all file
//!    names, so `basename --help` printed `--help`.
//! 4. **Extra operands were ignored.** `basename a b c` printed `a` and said
//!    nothing about `c`; GNU makes it a usage error.
//! 5. **Host path semantics** — `Path::file_name` splits on `\`, which is an
//!    ordinary byte in one of our names.
//! 6. **`missing operand` had no `Try 'basename --help'` referral.**
//!
//! The splitting is now [`coreutils::pathname`]; this file is argv, suffix
//! removal, and output.
//!
//! # Two usages, and the `+` that separates them
//!
//! ```text
//! basename NAME [SUFFIX]
//! basename OPTION... NAME...
//! ```
//!
//! GNU's `getopt_long` string is `"+as:z"`, and the leading `+` is the whole
//! reason the first form can exist: it stops option parsing at the first
//! operand, so in `basename x/y.txt -s .txt` the `-s` is *not* an option. It is
//! a second operand, `.txt` is a third, and the result is
//! `basename: extra operand ‘.txt’` — measured. Without the `+` the same line
//! would quietly mean something else entirely.
//!
//! This is exactly where `basename` and `dirname` part company: `dirname`'s
//! string is plain `"z"`, so `dirname a/b -z` *does* set the flag. Two
//! utilities, one page of documentation, opposite answers to "may an option
//! follow an operand" — which is why neither is guessed here.
//!
//! Consequences of the two forms:
//!
//! - Without `-a`/`-s`, at most two operands are accepted and the second is the
//!   suffix. A third is `extra operand`, naming the *third* — `argv[optind+2]`.
//! - `-s` implies `-a`, by falling through to it in upstream's `switch`. So
//!   `basename -s .h a.h b.h` is two names, not a name and an extra operand.
//! - With `-a` and no `-s` there is no suffix at all, however many operands.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::pathname::{base_name, is_relative};
use coreutils::quote::{os_bytes, quote};
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `basename`'s usage status is 1 — measured: `basename; echo $?` prints 1.
const BASENAME: Program = Program::new("basename", 1);

/// GNU `basename`'s `getopt_long` string, exactly — the `+` included.
///
/// See the module docs: the `+` stops option parsing at the first operand, and
/// that is what makes `basename NAME [SUFFIX]` unambiguous.
const SHORT_OPTIONS: &str = "+as:z";

/// GNU `basename`'s `longopts[]`, in its declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("multiple", Takes::Nothing),
    ("suffix", Takes::Required),
    ("zero", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Flags {
    /// `-a`, or `-s` implying it.
    multiple: bool,
    /// `-s`. Distinct from `Some("")`: an empty suffix is a suffix, and
    /// removing it is a no-op rather than nothing at all.
    suffix: Option<OsString>,
    zero: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Flags, Vec<OsString>),
}

fn main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // The usage error is decided before the stream exists, because upstream's
    // `usage (EXIT_FAILURE)` never reaches `atexit (close_stdout)` with
    // anything buffered: measured, `basename >&-` prints only
    // `basename: missing operand` and exits 1, with no write error after it.
    let request = match parse_args(&args) {
        Ok(request) => request,
        Err(e) => {
            eprintln!("basename: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    // `--help` and `--version` are writes like any other, so they fail like any
    // other: `basename --help >&-` is `basename: write error: Bad file
    // descriptor` and exits 1.
    let mut out = Stream::stdout();
    match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
        }
        Request::Version => {
            let _ = out.write_all(b"basename (SlateOS coreutils) 0.1.0\n");
        }
        // A closed stdout must not be reported as success: `-z` output is
        // usually piped into `xargs -0`, and a pipe that went away mid-list
        // would otherwise look like a complete list.
        Request::Run(flags, names) => run(&flags, &names, &mut out),
    }
    stdfd::close_stdout("basename", out, ExitCode::SUCCESS)
}

fn help_text() -> String {
    "\
Usage: basename NAME [SUFFIX]
  or:  basename OPTION... NAME...
Print NAME with any leading directory components removed.
If specified, also remove a trailing SUFFIX.

Mandatory arguments to long options are mandatory for short options too.
  -a, --multiple       support multiple arguments and treat each as a NAME
  -s, --suffix=SUFFIX  remove a trailing SUFFIX; implies -a
  -z, --zero           end each output line with NUL, not newline
      --help        display this help and exit
      --version     output version information and exit

Examples:
  basename /usr/bin/sort          -> \"sort\"
  basename include/stdio.h .h     -> \"stdio\"
  basename -s .h include/stdio.h  -> \"stdio\"
  basename -a any/str1 any/str2   -> \"str1\" followed by \"str2\"
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `basename`'s argv, resolving the two usages into one shape.
///
/// The returned operand list is always *names*: in the two-operand form the
/// suffix has been lifted out of it into [`Flags::suffix`], so the caller need
/// not know which form was used.
///
/// # Errors
///
/// An unknown option, a long option resolving to none or to more than one of
/// the table's entries, a long option given a value it does not take or denied
/// one it requires, no operands, or a third operand without `-a`/`-s`.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = Flags::default();
    let mut operands: Vec<OsString> = Vec::new();

    for item in BASENAME.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Short(b'a', _) | Opt::Long("multiple", _) => flags.multiple = true,
            // `-s` implies `-a`: upstream falls through the `case` into it.
            Opt::Short(b's', value) | Opt::Long("suffix", value) => {
                flags.suffix = value;
                flags.multiple = true;
            }
            Opt::Short(b'z', _) | Opt::Long("zero", _) => flags.zero = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(BASENAME.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(BASENAME.invalid_option(other)),
        }
    }

    // Upstream's order: missing first, then extra. `basename -s .x` with no
    // operands is therefore `missing operand`, not a complaint about `-s`.
    let Some(first) = operands.first() else {
        return Err(BASENAME.usage_referring("missing operand".into()));
    };

    if flags.multiple {
        return Ok(Request::Run(flags, operands));
    }

    // The single-name form. A third operand is the error, and it is the one
    // named in the message even if there is a fourth.
    if let Some(extra) = operands.get(2) {
        return Err(BASENAME.usage_referring(format!(
            "extra operand {}",
            quote(&os_bytes(extra.as_os_str()))
        )));
    }
    flags.suffix = operands.get(1).cloned();
    Ok(Request::Run(flags, vec![first.clone()]))
}

// ----------------------------------------------------------------- output ---

/// Remove `suffix` from the end of `name`, unless `name` consists entirely of
/// it.
///
/// coreutils' `remove_suffix`, transcribed. Walking from both ends and
/// returning on the first disagreement is not merely one way to test a suffix —
/// it is what makes "entirely of it" fall out for free: the walk stops when the
/// *name* runs out, and the truncation is then skipped because there is nothing
/// left in front of it. So `basename .txt .txt` answers `.txt`, not an empty
/// line.
#[must_use]
fn remove_suffix<'a>(name: &'a [u8], suffix: &[u8]) -> &'a [u8] {
    let mut np = name.len();
    let mut sp = suffix.len();
    while np > 0 && sp > 0 {
        np = np.saturating_sub(1);
        sp = sp.saturating_sub(1);
        if name.get(np) != suffix.get(sp) {
            return name;
        }
    }
    if np > 0 {
        return name.get(..np).unwrap_or_default();
    }
    name
}

/// Print the last component of every name, less the suffix.
///
/// Like `dirname`, this consults no filesystem and so cannot fail; the exit
/// status depends only on the flush.
fn run<W: Write>(flags: &Flags, names: &[OsString], out: &mut W) {
    let delimiter = if flags.zero { b'\0' } else { b'\n' };
    for name in names {
        let bytes = os_bytes(name.as_os_str());
        let mut base = base_name(&bytes);

        // Upstream guards the suffix removal with `IS_RELATIVE_FILE_NAME`, so
        // that POSIX's `basename // /` can answer `//` rather than being eaten.
        // `base_name` returns an absolute answer only for a filesystem root, so
        // the guard fires only for `/` — where, as it happens, `remove_suffix`
        // would refuse anyway. Kept because it is upstream's rule and because
        // that coincidence is not one to rely on.
        if let Some(suffix) = flags.suffix.as_ref()
            && is_relative(base)
        {
            base = remove_suffix(base, &os_bytes(suffix.as_os_str()));
        }

        // Raw bytes: a file name is an arbitrary byte string, and rendering it
        // as text would corrupt any name that is not UTF-8.
        let _ = out.write_all(base);
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
            Request::Run(flags, names) => run(&flags, &names, &mut out),
            other => panic!("expected a run, got {other:?}"),
        }
        out
    }

    /// The one-operand answer as text, for the readable cases.
    fn one(args: &[&str]) -> String {
        let out = go(args);
        String::from_utf8(out).unwrap().trim_end().to_string()
    }

    // ------------------------------------------------------------- naming ---

    #[test]
    fn the_last_component_is_what_is_printed() {
        assert_eq!(one(&["/usr/bin/sort"]), "sort");
        assert_eq!(one(&["foo.txt"]), "foo.txt");
        assert_eq!(one(&["/home/user/.bashrc"]), ".bashrc");
    }

    #[test]
    fn a_root_is_its_own_base_name() {
        assert_eq!(one(&["/"]), "/");
        assert_eq!(one(&["//"]), "/");
        assert_eq!(one(&["///"]), "/");
    }

    #[test]
    fn trailing_slashes_are_dropped() {
        assert_eq!(one(&["/usr/bin/"]), "bin");
        assert_eq!(one(&["a//b//"]), "b");
    }

    #[test]
    fn the_empty_name_prints_an_empty_line() {
        // An operand, not a missing one: GNU prints just the delimiter.
        assert_eq!(go(&[""]), b"\n");
    }

    #[test]
    fn a_backslash_is_part_of_the_name() {
        assert_eq!(one(&["a\\b"]), "a\\b");
        assert_eq!(one(&["d/a\\b"]), "a\\b");
    }

    // ------------------------------------------------------------ suffixes ---

    #[test]
    fn a_matching_suffix_is_removed() {
        assert_eq!(one(&["include/stdio.h", ".h"]), "stdio");
        assert_eq!(one(&["file.txt", ".txt"]), "file");
        assert_eq!(one(&["/usr/lib.so", ".so"]), "lib");
        assert_eq!(one(&["/a/b/c/d/e/f.tar.gz", ".gz"]), "f.tar");
    }

    #[test]
    fn only_the_last_occurrence_is_a_suffix() {
        assert_eq!(one(&["lib.so.so", ".so"]), "lib.so");
    }

    #[test]
    fn a_suffix_that_does_not_match_is_left_alone() {
        assert_eq!(one(&["file.txt", ".c"]), "file.txt");
        assert_eq!(one(&["readme", ".md"]), "readme");
        // Longer than the name, so the walk runs out of name first.
        assert_eq!(one(&["ab", "xyz"]), "ab");
    }

    /// The rule that makes `remove_suffix` worth transcribing rather than
    /// writing as `ends_with`: a name consisting entirely of the suffix keeps
    /// it, because the answer would otherwise be empty.
    #[test]
    fn a_name_that_is_only_the_suffix_keeps_it() {
        assert_eq!(one(&[".txt", ".txt"]), ".txt");
        assert_eq!(one(&["x", "x"]), "x");
        assert_eq!(one(&["foo", "foo"]), "foo");
    }

    #[test]
    fn an_empty_suffix_removes_nothing() {
        assert_eq!(one(&["file.txt", ""]), "file.txt");
        assert_eq!(one(&["s", ""]), "s");
    }

    /// POSIX's case: a root is never eaten by a suffix.
    #[test]
    fn a_root_survives_a_suffix() {
        assert_eq!(one(&["/", "/"]), "/");
    }

    // ---------------------------------------------------------- the forms ---

    #[test]
    fn without_multiple_the_second_operand_is_the_suffix() {
        let Request::Run(flags, names) = parse_args(&argv(&["a/b.h", ".h"])).unwrap() else {
            panic!("expected a run");
        };
        assert_eq!(names, argv(&["a/b.h"]));
        assert_eq!(flags.suffix, Some(OsString::from(".h")));
        assert!(!flags.multiple);
    }

    #[test]
    fn a_third_operand_is_a_usage_error_naming_the_third() {
        let e = parse_args(&argv(&["a", "b", "c"])).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "extra operand \u{2018}c\u{2019}\nTry 'basename --help' for more information."
        );
        // A fourth does not change which one is named.
        let e = parse_args(&argv(&["a", "b", "c", "d"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}c\u{2019}\nTry 'basename --help' for more information."
        );
    }

    #[test]
    fn multiple_treats_every_operand_as_a_name() {
        assert_eq!(go(&["-a", "any/str1", "any/str2"]), b"str1\nstr2\n");
        assert_eq!(go(&["--multiple", "a/b", "c/d", "e/f"]), b"b\nd\nf\n");
    }

    #[test]
    fn suffix_implies_multiple() {
        assert_eq!(go(&["-s", ".txt", "a/x.txt", "b/y.txt"]), b"x\ny\n");
        assert_eq!(go(&["--suffix=.txt", "a/x.txt"]), b"x\n");
        // Which is to say the third operand is a name, not an error.
        let Request::Run(flags, names) = parse_args(&argv(&["-s", ".h", "a.h", "b.h"])).unwrap()
        else {
            panic!("expected a run");
        };
        assert!(flags.multiple);
        assert_eq!(names, argv(&["a.h", "b.h"]));
    }

    #[test]
    fn multiple_without_a_suffix_removes_nothing() {
        assert_eq!(go(&["-a", "a/x.txt"]), b"x.txt\n");
    }

    /// The `+`: option parsing stops at the first operand, so `-z` here is a
    /// name. Measured — GNU prints `y` and a literal `-z`, both newline-ended.
    #[test]
    fn an_option_after_an_operand_is_an_operand() {
        assert_eq!(go(&["-a", "x/y", "-z"]), b"y\n-z\n");
    }

    /// The same rule seen from the failing side: `-s` after a name is operand
    /// two, and its value operand three.
    #[test]
    fn a_trailing_option_becomes_the_extra_operand() {
        let e = parse_args(&argv(&["x/y.txt", "-s", ".txt"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}.txt\u{2019}\nTry 'basename --help' for more information."
        );
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(one(&["--", "-z"]), "-z");
        assert_eq!(go(&["-a", "--", "-z", "a/b"]), b"-z\nb\n");
    }

    // --------------------------------------------------------- diagnostics ---

    #[test]
    fn no_operands_names_the_missing_thing_and_refers_on() {
        let e = parse_args(&argv(&[])).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "missing operand\nTry 'basename --help' for more information."
        );
    }

    /// Missing is checked before extra, so a `-s` with nothing to apply it to
    /// is the missing-operand case rather than a complaint about the option.
    #[test]
    fn a_suffix_with_no_names_is_missing_operand() {
        let e = parse_args(&argv(&["-s", ".x"])).unwrap_err();
        assert_eq!(
            e.message(),
            "missing operand\nTry 'basename --help' for more information."
        );
    }

    #[test]
    fn a_suffix_option_at_the_end_of_argv_is_an_error() {
        let e = parse_args(&argv(&["-s"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option requires an argument -- 's'\n\
             Try 'basename --help' for more information."
        );
        let e = parse_args(&argv(&["--suffix"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--suffix' requires an argument\n\
             Try 'basename --help' for more information."
        );
    }

    #[test]
    fn an_unknown_option_is_refused_rather_than_taken_as_a_file() {
        let e = parse_args(&argv(&["-x", "a/b"])).unwrap_err();
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'basename --help' for more information."
        );
        let e = parse_args(&argv(&["--nope", "a/b"])).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'basename --help' for more information."
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
    }

    /// `--suffix` and (nothing else) share no prefix, but `--m`/`--z` are
    /// unambiguous and `--` prefixes of `help`/`version` behave as usual.
    #[test]
    fn unambiguous_prefixes_are_accepted() {
        assert_eq!(go(&["--mu", "a/b", "c/d"]), b"b\nd\n");
        assert_eq!(go(&["--su=.h", "a/x.h"]), b"x\n");
        assert_eq!(go(&["--z", "a/b"]), b"b\0");
    }

    // ------------------------------------------------------------- output ---

    #[test]
    fn zero_terminates_with_nul_including_the_last() {
        assert_eq!(go(&["-z", "a/b"]), b"b\0");
        assert_eq!(go(&["-az", "a/b", "c/d"]), b"b\0d\0");
    }

    /// A name that is not UTF-8 must reach the output unchanged, suffix removal
    /// included. On the host `OsString` is WTF-8 and cannot hold an arbitrary
    /// byte, so this is a Unix-only check.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_name_survives_byte_for_byte() {
        use std::os::unix::ffi::OsStringExt;
        let flags = Flags {
            multiple: true,
            suffix: Some(OsString::from_vec(b"\xff".to_vec())),
            zero: false,
        };
        let name = OsString::from_vec(b"/d/\x80\xfe\xff".to_vec());
        let mut out: Vec<u8> = Vec::new();
        run(&flags, &[name], &mut out);
        assert_eq!(out, b"\x80\xfe\n");
    }

    #[test]
    fn remove_suffix_matches_upstream() {
        for (name, suffix, want) in [
            (&b"stdio.h"[..], &b".h"[..], &b"stdio"[..]),
            (b"lib.so.so", b".so", b"lib.so"),
            (b".txt", b".txt", b".txt"),
            (b"x", b"x", b"x"),
            (b"ab", b"xyz", b"ab"),
            (b"file.txt", b".c", b"file.txt"),
            (b"s", b"", b"s"),
            (b"", b"", b""),
            (b"", b".c", b""),
        ] {
            assert_eq!(
                remove_suffix(name, suffix),
                want,
                "remove_suffix({}, {})",
                String::from_utf8_lossy(name),
                String::from_utf8_lossy(suffix)
            );
        }
    }
}
