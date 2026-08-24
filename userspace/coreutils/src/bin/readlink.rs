//! readlink — print a symbolic link's target, or a file name's canonical form.
//!
//! # Why this was rewritten
//!
//! Six defects, of which the first two are the ones that changed answers.
//!
//! ## 1. `-f` and `-m` did not exist; all three modes ran `-e`
//!
//! The old parser folded `-f`, `-e` and `-m` into a single `canonicalize: bool`
//! and answered it with [`std::fs::canonicalize`]. That function is exactly
//! *one* of the three modes — `-e`, the one where every component must already
//! exist. So the other two silently answered a question nobody asked:
//!
//! | Command | GNU | This program, before |
//! |---|---|---|
//! | `readlink -f d/missing` | prints `…/d/missing`, exit 0 | fails, exit 1 |
//! | `readlink -m no/such/thing` | prints `…/no/such/thing`, exit 0 | fails, exit 1 |
//! | `readlink -f dangling` | prints the target, exit 0 | fails, exit 1 |
//!
//! `-f` on a name whose last component does not exist yet is not an edge case;
//! it is the whole reason `-f` is used. `mkdir -p "$(dirname "$(readlink -f
//! "$out")")"` is the idiom, and it failed here on every path that did not
//! already exist. The three modes now come from [`coreutils::canon`], which is
//! gnulib's `canonicalize_filename_mode` — see that module for the measurements.
//!
//! On the development host there was a second, quieter wrongness in the same
//! call: [`std::fs::canonicalize`] returns a `\\?\C:\…` extended-length path on
//! Windows, so the answer was not even a name the rest of the program could use.
//!
//! ## 2. It printed a diagnostic GNU does not print, and hid the one it does
//!
//! `readlink` is **quiet by default** — `-q`/`-s` name the default, and `-v`
//! turns messages on. Measured:
//!
//! ```text
//! $ readlink d/real ; echo $?          # a regular file, not a link
//! 1
//! $ readlink -v d/real
//! readlink: d/real: Invalid argument
//! ```
//!
//! The old code had no `-v` at all and printed unconditionally, so a shell loop
//! doing `readlink "$f" || continue` over a directory produced a diagnostic per
//! non-link — which is precisely what the default silence exists to prevent. It
//! printed the *host's* wording too (`The system cannot find the file
//! specified. (os error 2)`), not POSIX's; see [`coreutils::errmsg`].
//!
//! `-q`, `-s` and `-v` are last-wins, which is `getopt`'s behaviour and not an
//! extra rule: measured, `-v -q` is silent and `-q -v` is not.
//!
//! ## 3. Argv was `Vec<String>`, so a legal file name crashed it
//!
//! `env::args()` panics on an argument that is not valid UTF-8, and on this OS
//! a path may hold every byte but `/` and NUL (`design.txt`). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`. Argv is now
//! `OsString` and stays bytes all the way to the syscall and back out to stdout
//! — which matters at the *output* end here as much as the input end, since a
//! link's target is also an arbitrary byte string.
//!
//! ## 4. Unknown options became file names
//!
//! Anything that was not `-f`, `-e` or `-m` was pushed onto the operand list.
//! So `readlink -x foo` looked for a file called `-x`, `--help` looked for a
//! file called `--help`, and `--` was not an end-of-options marker — meaning a
//! link whose name begins with a dash could not be read at all.
//!
//! ## 5. `-n` and `-z` were missing, and they are not cosmetic
//!
//! `-z` terminates each name with NUL instead of a newline, which is the only
//! safe way to pass a list of paths to `xargs -0`, since a newline is a legal
//! byte in a name. `-n` omits the trailing delimiter, for `x=$(readlink -n f)`.
//!
//! `-n` has one rule that is easy to miss and is measured here: with more than
//! one operand it is *refused* rather than obeyed, with a warning on stderr and
//! the delimiter used anyway — because otherwise the outputs would run together
//! with nothing between them. The warning is **not** suppressed by `-q`;
//! measured, `readlink -q -n a b` still prints it. Upstream this is a plain
//! `error (0, 0, …)` before the loop, followed by `no_newline = false`.
//!
//! ## 6. `missing operand` carried no referral
//!
//! GNU follows it with `Try 'readlink --help' for more information.`, and
//! `readlink --` — no operands after the marker — is that same case.
//!
//! # The long-option table is in GNU's declaration order
//!
//! Order is observable: `getopt_long` lists an ambiguous prefix's candidates in
//! table order. Measured, with `--c`, which is the abbreviation a user is most
//! likely to try:
//!
//! ```text
//! readlink: option '--c' is ambiguous; possibilities: '--canonicalize'
//! '--canonicalize-existing' '--canonicalize-missing'
//! ```
//!
//! There are no aliases in this table — unlike `rmdir`'s, every entry has a
//! distinct `val` upstream, `--quiet` and `--silent` included (`'q'` and `'s'`).
//! So [`coreutils::getopt::Program::resolve_long`] is enough and
//! `resolve_long_aliased` is not needed.
//!
//! Every option in the table is implemented, so unlike the other rewritten bins
//! there is no `unimplemented_*` diagnostic here.

use coreutils::canon::{self, Fs, Mode, RealFs};
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quotef_os};
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

/// `readlink`'s usage status is 1 — measured: `readlink -x; echo $?` prints 1.
const READLINK: Program = Program::new("readlink", 1);

/// GNU `readlink`'s `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "efmnqsvz";

/// GNU `readlink`'s `longopts[]`, in its declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("canonicalize", Takes::Nothing),
    ("canonicalize-existing", Takes::Nothing),
    ("canonicalize-missing", Takes::Nothing),
    ("no-newline", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("silent", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Clone, Copy, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Flags {
    /// Which canonicalisation `-f`/`-e`/`-m` asked for, or `None` for plain
    /// `readlink(2)`. Last one wins — measured, `readlink -e -m d/nope`
    /// succeeds, so `-m` overrode the `-e` before it.
    mode: Option<Mode>,
    no_newline: bool,
    /// `-v`. The default is silence; see module docs, defect 2.
    verbose: bool,
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
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("readlink (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, files)) => {
            let mut out = io::stdout().lock();
            let mut err = io::stderr().lock();
            let ok = read_all(&flags, &files, &RealFs, &mut out, &mut err);
            // A closed stdout must not be reported as success. `-z` output is
            // usually piped into `xargs -0`, and a pipe that goes away mid-list
            // would otherwise look like a complete list.
            let flushed = out.flush().is_ok();
            if ok && flushed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("readlink: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: readlink [OPTION]... FILE...
Print value of a symbolic link or canonical file name.

  -f, --canonicalize            canonicalize by following every symlink in
                                every component of the given name recursively;
                                all but the last component must exist
  -e, --canonicalize-existing   canonicalize by following every symlink in
                                every component of the given name recursively,
                                all components must exist
  -m, --canonicalize-missing    canonicalize by following every symlink in
                                every component of the given name recursively,
                                without requirements on components existence
  -n, --no-newline              do not output the trailing delimiter
  -q, --quiet
  -s, --silent                  suppress most error messages (on by default)
  -v, --verbose                 report error messages
  -z, --zero                    end each output line with NUL, not newline
      --help                    display this help and exit
      --version                 output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `readlink`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `readlink a -f b` is
/// `readlink -f a b` — which is `getopt_long`'s default permuting behaviour.
///
/// # Errors
///
/// An unknown option, a long option resolving to none or to more than one of
/// the table's entries, or a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = Flags::default();
    let mut files: Vec<OsString> = Vec::new();

    for item in READLINK.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // A lone `-` arrives as an operand: it is a file called `-`, and
            // `readlink` has no standard-input operand for it to mean anything
            // else.
            Opt::Operand(file) => files.push(file.clone()),
            Opt::Short(b'f', _) | Opt::Long("canonicalize", _) => {
                flags.mode = Some(Mode::AllButLast);
            }
            Opt::Short(b'e', _) | Opt::Long("canonicalize-existing", _) => {
                flags.mode = Some(Mode::Existing);
            }
            Opt::Short(b'm', _) | Opt::Long("canonicalize-missing", _) => {
                flags.mode = Some(Mode::Missing);
            }
            Opt::Short(b'n', _) | Opt::Long("no-newline", _) => flags.no_newline = true,
            Opt::Short(b'q' | b's', _) | Opt::Long("quiet" | "silent", _) => flags.verbose = false,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            Opt::Short(b'z', _) | Opt::Long("zero", _) => flags.zero = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(READLINK.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(READLINK.invalid_option(other)),
        }
    }

    Ok(Request::Run(flags, files))
}

// -------------------------------------------------------------- resolving ---

/// Answer one operand: the link's target, or the name's canonical form.
///
/// The plain case goes through [`canon::Fs::read_link`] rather than
/// [`std::fs::read_link`] so that both paths share one workaround: Windows
/// answers a non-symlink with `ERROR_NOT_A_REPARSE_POINT`, which std leaves
/// uncategorised, where POSIX answers `EINVAL` — and `EINVAL` is what makes
/// `readlink -v` on a regular file say `Invalid argument`.
///
/// # Errors
///
/// Whatever the filesystem said, or `ELOOP` from the canonicaliser.
fn resolve<F: Fs + ?Sized>(mode: Option<Mode>, name: &[u8], fs: &F) -> io::Result<Vec<u8>> {
    match mode {
        Some(m) => canon::canonicalize(fs, name, m),
        None => fs.read_link(name),
    }
}

/// Answer every operand, writing results to `out` and diagnostics to `err`.
///
/// Returns `true` if every operand was answered. Takes both sinks as parameters
/// so the output — bytes, delimiters and all — can be asserted on byte for byte;
/// the old file had no test of anything past argument parsing.
///
/// One failure does not abandon the rest: measured, `readlink nosuch link`
/// prints the second one's target and exits 1.
fn read_all<F: Fs + ?Sized, W: Write, E: Write>(
    flags: &Flags,
    files: &[OsString],
    fs: &F,
    out: &mut W,
    err: &mut E,
) -> bool {
    if files.is_empty() {
        let _ = writeln!(
            err,
            "readlink: {}",
            READLINK.usage_referring("missing operand".into())
        );
        return false;
    }

    // Module docs, defect 5. Upstream warns and then clears the flag, so the
    // delimiter is written after every name including the last.
    let mut no_newline = flags.no_newline;
    if no_newline && files.len() > 1 {
        // Deliberately not gated on `flags.verbose`: measured, `-q -n a b`
        // still prints this. It reports that an option was *not obeyed*, which
        // is a different thing from a per-operand failure, and silencing it
        // would leave the user believing the delimiter had been suppressed.
        let _ = writeln!(
            err,
            "readlink: ignoring --no-newline with multiple arguments"
        );
        no_newline = false;
    }

    let delimiter = if flags.zero { b'\0' } else { b'\n' };
    let mut ok = true;
    for file in files {
        match resolve(flags.mode, &os_bytes(file.as_os_str()), fs) {
            Ok(answer) => {
                // Raw bytes. A link's target is an arbitrary byte string, and
                // rendering it as text would corrupt any name that is not UTF-8
                // — the same defect at the output end that `OsString` fixes at
                // the input end.
                let _ = out.write_all(&answer);
                if !no_newline {
                    let _ = out.write_all(&[delimiter]);
                }
            }
            Err(e) => {
                if flags.verbose {
                    // `quotef`, not `quoteaf`: measured, `readlink -v d/real`
                    // leaves the name bare and `readlink -v ''` prints `''`.
                    let _ = writeln!(err, "readlink: {}: {}", quotef_os(file), strerror(&e));
                }
                ok = false;
            }
        }
    }
    ok
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::expect_used
)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (Flags, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, d) => (
                f,
                d.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --

    #[test]
    fn no_options_is_plain_readlink() {
        let (f, d) = run_parse(&["foo", "bar"]);
        assert_eq!(f, Flags::default());
        assert!(f.mode.is_none(), "no -f/-e/-m means readlink(2)");
        assert!(!f.verbose, "quiet is the default");
        assert_eq!(d, vec!["foo", "bar"]);
    }

    /// Defect 1: these three were one boolean, and all three ran `-e`.
    #[test]
    fn the_three_modes_are_three_different_things() {
        for (typed, want) in [
            ("-f", Mode::AllButLast),
            ("--canonicalize", Mode::AllButLast),
            ("-e", Mode::Existing),
            ("--canonicalize-existing", Mode::Existing),
            ("-m", Mode::Missing),
            ("--canonicalize-missing", Mode::Missing),
        ] {
            assert_eq!(run_parse(&[typed, "x"]).0.mode, Some(want), "{typed}");
        }
    }

    /// Measured: `readlink -e -m d/nope` succeeds, so `-m` overrode `-e`.
    #[test]
    fn the_last_mode_wins() {
        assert_eq!(run_parse(&["-e", "-m", "x"]).0.mode, Some(Mode::Missing));
        assert_eq!(run_parse(&["-m", "-e", "x"]).0.mode, Some(Mode::Existing));
        assert_eq!(run_parse(&["-f", "-e", "x"]).0.mode, Some(Mode::Existing));
        // Bundled, which is the same rule read left to right.
        assert_eq!(run_parse(&["-em", "x"]).0.mode, Some(Mode::Missing));
    }

    /// Measured: `-v -q` is silent, `-q -v` is not. Silence is the default, so
    /// `-q` and `-s` are only meaningful as an *undo* of an earlier `-v`.
    #[test]
    fn quiet_and_verbose_are_last_wins() {
        assert!(run_parse(&["-v", "x"]).0.verbose);
        assert!(!run_parse(&["-v", "-q", "x"]).0.verbose);
        assert!(run_parse(&["-q", "-v", "x"]).0.verbose);
        assert!(!run_parse(&["-v", "-s", "x"]).0.verbose);
        assert!(!run_parse(&["-v", "--quiet", "x"]).0.verbose);
        assert!(!run_parse(&["-v", "--silent", "x"]).0.verbose);
        assert!(run_parse(&["-q", "--verbose", "x"]).0.verbose);
        assert!(!run_parse(&["-vq", "x"]).0.verbose);
    }

    #[test]
    fn n_and_z_are_flags() {
        assert!(run_parse(&["-n", "x"]).0.no_newline);
        assert!(run_parse(&["--no-newline", "x"]).0.no_newline);
        assert!(run_parse(&["-z", "x"]).0.zero);
        assert!(run_parse(&["--zero", "x"]).0.zero);
        let (f, _) = run_parse(&["-nz", "x"]);
        assert!(f.no_newline && f.zero, "bundling must set both");
    }

    #[test]
    fn options_may_follow_operands() {
        let (f, d) = run_parse(&["foo", "-f"]);
        assert_eq!(f.mode, Some(Mode::AllButLast));
        assert_eq!(d, vec!["foo"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-"]).1, vec!["-"]);
    }

    /// Defect 4: `--` used to be looked up as a file name, so a link whose name
    /// begins with a dash could not be read at all.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-f", "-v"]).1, vec!["-f", "-v"]);
        let (f, _) = run_parse(&["--", "-f"]);
        assert!(f.mode.is_none(), "-f after -- is a file name");
    }

    /// Also defect 4. `readlink --help` used to look for a file called
    /// `--help`.
    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    /// The abbreviation a user is most likely to try, and the one the table's
    /// order decides. Measured verbatim.
    #[test]
    fn the_canonicalize_prefix_is_ambiguous_among_three() {
        for typed in ["--c", "--can"] {
            let e = fail(&[typed, "x"]);
            assert_eq!(
                e.sentence,
                "option '".to_string()
                    + typed
                    + "' is ambiguous; possibilities: '--canonicalize' \
                       '--canonicalize-existing' '--canonicalize-missing'",
                "{typed}"
            );
        }
        // The exact name is *not* ambiguous with the two that extend it —
        // glibc takes an exact match before considering prefixes. Measured:
        // `readlink --canonicalize x` prints a path rather than complaining.
        assert_eq!(
            run_parse(&["--canonicalize", "x"]).0.mode,
            Some(Mode::AllButLast)
        );
        // And one byte further in, the exact match is gone and only the two
        // longer names remain — so the candidate list shrinks to two. Measured
        // verbatim; this is the row that catches a resolver which treats "is a
        // prefix of" and "is equal to" as the same relation.
        assert_eq!(
            fail(&["--canonicalize-", "x"]).sentence,
            "option '--canonicalize-' is ambiguous; possibilities: \
             '--canonicalize-existing' '--canonicalize-missing'"
        );
    }

    #[test]
    fn the_version_prefix_is_ambiguous_with_verbose() {
        assert_eq!(
            fail(&["--v"]).sentence,
            "option '--v' is ambiguous; possibilities: '--verbose' '--version'"
        );
    }

    /// The whole table, in GNU's declaration order, as `readlink --=x` prints
    /// it. An empty prefix matches every entry, so this pins the order itself.
    #[test]
    fn the_empty_prefix_lists_the_table_in_order() {
        assert_eq!(
            fail(&["--=x"]).sentence,
            "option '--=x' is ambiguous; possibilities: '--canonicalize' \
             '--canonicalize-existing' '--canonicalize-missing' '--no-newline' \
             '--quiet' '--silent' '--verbose' '--zero' '--help' '--version'"
        );
    }

    /// Defect 4: `-x` used to become a file name silently.
    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-x", "a"]);
        assert_eq!(e.sentence, "invalid option -- 'x'");
        assert_eq!(e.status, 1);
        assert!(e.message().contains("Try 'readlink --help'"), "{e:?}");
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--bogus", "a"]);
        assert_eq!(e.sentence, "unrecognized option '--bogus'");
        assert_eq!(e.status, 1);
    }

    /// `--no` resolves — it prefixes only `--no-newline` — but `--no=1` must
    /// still be refused, because the option takes nothing.
    #[test]
    fn a_value_on_an_option_that_takes_none() {
        assert!(run_parse(&["--no", "x"]).0.no_newline);
        assert_eq!(
            fail(&["--no=1", "x"]).sentence,
            "option '--no-newline' doesn't allow an argument"
        );
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// Defect 3, on the input side. Byte `0x80` alone is not valid UTF-8, so an
    /// operand containing it cannot be a `String` — `env::args()` would have
    /// panicked before `readlink` saw it.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(bad.to_str().is_none(), "the fixture must not be a String");
        match parse_args(&[OsString::from("-f"), bad.clone()]).unwrap() {
            Request::Run(f, d) => {
                assert_eq!(f.mode, Some(Mode::AllButLast));
                assert_eq!(d, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    /// The same blind spot `rmdir.rs` closes: on the development host the
    /// `cfg(unix)` test above does not run, so Windows gets its own argument no
    /// `String` can hold — an unpaired surrogate.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(bad.to_str().is_none(), "the fixture must not be a String");
        match parse_args(&[OsString::from("-f"), bad.clone()]).unwrap() {
            Request::Run(f, d) => {
                assert_eq!(f.mode, Some(Mode::AllButLast));
                assert_eq!(d, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'-', b'-', 0x80]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(e.sentence.starts_with("unrecognized option"), "{e:?}");
    }

    #[test]
    #[cfg(windows)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x002D, 0x002D, 0xD800]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(e.sentence.starts_with("unrecognized option"), "{e:?}");
    }

    // ------------------------------------------------------ the whole run --

    /// The same shape of fake filesystem `canon`'s tests use, so these tests
    /// run identically on every host and cover the cases the host cannot build
    /// — a name that is not UTF-8, and a symlink at all, which an unprivileged
    /// Windows process cannot create.
    struct FakeFs(BTreeMap<&'static [u8], Option<&'static [u8]>>);

    impl FakeFs {
        /// `None` is a regular file or directory; `Some(target)` is a symlink.
        fn new() -> Self {
            let mut m: BTreeMap<&'static [u8], Option<&'static [u8]>> = BTreeMap::new();
            m.insert(b"/", None);
            m.insert(b"/w", None);
            m.insert(b"/w/real", None);
            m.insert(b"/w/link", Some(b"real"));
            m.insert(b"/w/link2", Some(b"real"));
            // A target that is not valid UTF-8, which is the case the output
            // path has to survive; see defect 3's output half.
            m.insert(b"/w/oddlink", Some(b"od\xffd"));
            Self(m)
        }

        /// Look a name up, resolving a relative one against [`Fs::cwd`].
        ///
        /// The prepending is not scaffolding — it is the one job a kernel does
        /// here that [`canon`] does not. `canon` builds an absolute `rname`
        /// before its first `read_link`, so it never asks about a relative
        /// name; the *plain* `readlink` path hands the operand straight
        /// through, exactly as `readlink(2)` receives it. A fake that only
        /// understood absolute names would answer `ENOENT` for every
        /// non-canonicalising call and quietly test nothing.
        fn lookup(&self, path: &[u8]) -> Option<&Option<&'static [u8]>> {
            if path.first() == Some(&b'/') {
                return self.0.get(path);
            }
            let mut abs = b"/w/".to_vec();
            abs.extend_from_slice(path);
            self.0.get(abs.as_slice())
        }
    }

    impl Fs for FakeFs {
        fn cwd(&self) -> io::Result<Vec<u8>> {
            Ok(b"/w".to_vec())
        }
        fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
            match self.lookup(path) {
                // EINVAL is how "exists and is not a symlink" is reported, and
                // it is what makes `-v` on a regular file say `Invalid
                // argument` rather than something about links.
                Some(None) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
                Some(Some(t)) => Ok((*t).to_vec()),
                None => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }
        fn dir_check(&self, path: &[u8]) -> io::Result<()> {
            match self.lookup(path) {
                Some(None) => Ok(()),
                Some(Some(_)) => Err(io::Error::from(io::ErrorKind::NotADirectory)),
                None => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }
        /// `readlink` never asks — it canonicalises with [`Links::Follow`]
        /// only, where existence is proved by `read_link`'s `EINVAL`. Answered
        /// honestly anyway, because a fake that lied here would be a trap for
        /// whoever next adds a flag to this file. Follows the link, as
        /// `faccessat(…, F_OK)` does; the map above has no link cycle for the
        /// recursion to fall into.
        fn exists(&self, path: &[u8]) -> io::Result<()> {
            match self.lookup(path) {
                Some(None) => Ok(()),
                Some(Some(target)) => self.exists(target),
                None => Err(io::Error::from(io::ErrorKind::NotFound)),
            }
        }
    }

    /// Run `read_all` over the fake filesystem, returning
    /// `(ok, stdout bytes, stderr text)`.
    fn run(flags: Flags, files: &[&str]) -> (bool, Vec<u8>, String) {
        let owned: Vec<OsString> = files.iter().map(OsString::from).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let ok = read_all(&flags, &owned, &FakeFs::new(), &mut out, &mut err);
        (ok, out, String::from_utf8_lossy(&err).into_owned())
    }

    /// Defect 6: the referral used to be missing, and `readlink --` reaches the
    /// same case.
    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, out, msg) = run(Flags::default(), &[]);
        assert!(!ok);
        assert!(out.is_empty());
        assert!(msg.contains("missing operand"), "{msg}");
        assert!(msg.contains("Try 'readlink --help'"), "{msg}");
        // `readlink --` parses to zero operands, which is this same path.
        assert_eq!(run_parse(&["--"]).1.len(), 0);
    }

    #[test]
    fn a_link_prints_its_target_and_a_newline() {
        let (ok, out, msg) = run(Flags::default(), &["link"]);
        assert!(ok, "{msg}");
        assert_eq!(out, b"real\n");
        assert!(msg.is_empty(), "{msg}");
    }

    /// Defect 2, both halves: silent by default, and the POSIX wording rather
    /// than the host's when `-v` asks for it.
    #[test]
    fn a_regular_file_is_refused_silently_unless_verbose() {
        let quiet = run(Flags::default(), &["real"]);
        assert!(!quiet.0, "a non-link is a failure");
        assert!(quiet.1.is_empty(), "nothing on stdout");
        assert!(quiet.2.is_empty(), "and nothing on stderr by default");

        let loud = run(
            Flags {
                verbose: true,
                ..Flags::default()
            },
            &["real"],
        );
        assert_eq!(loud.2, "readlink: real: Invalid argument\n");
        assert!(
            !loud.2.contains("os error"),
            "host wording leaked: {}",
            loud.2
        );
    }

    /// Measured: `readlink -v ''` prints `readlink: '': No such file or
    /// directory`. The empty name is the one case `quotef` does quote.
    #[test]
    fn the_empty_operand_is_quoted_in_the_diagnostic() {
        let (ok, _, msg) = run(
            Flags {
                verbose: true,
                ..Flags::default()
            },
            &[""],
        );
        assert!(!ok);
        assert_eq!(msg, "readlink: '': No such file or directory\n");
    }

    /// Defect 1, as an end-to-end run rather than a flag assertion: the three
    /// modes give three different answers for one name, where before they gave
    /// `-e`'s answer three times.
    #[test]
    fn the_three_modes_answer_a_missing_name_differently() {
        let mode = |m: Mode| Flags {
            mode: Some(m),
            ..Flags::default()
        };
        // `-e`: the last component must exist. It does not.
        let e = run(mode(Mode::Existing), &["nope"]);
        assert!(!e.0);
        assert!(e.1.is_empty());
        // `-f`: all but the last must exist. They do.
        let f = run(mode(Mode::AllButLast), &["nope"]);
        assert!(f.0, "{}", f.2);
        assert_eq!(f.1, b"/w/nope\n");
        // `-m`: nothing need exist.
        let m = run(mode(Mode::Missing), &["no/such/thing"]);
        assert!(m.0, "{}", m.2);
        assert_eq!(m.1, b"/w/no/such/thing\n");
        // And `-f` on that same name fails, because `no` is not a directory
        // that exists — which is the whole distinction between `-f` and `-m`.
        assert!(!run(mode(Mode::AllButLast), &["no/such/thing"]).0);
    }

    /// A canonicalising mode follows the link, where the default prints it.
    #[test]
    fn canonicalizing_resolves_where_plain_reports() {
        assert_eq!(run(Flags::default(), &["link"]).1, b"real\n");
        assert_eq!(
            run(
                Flags {
                    mode: Some(Mode::Existing),
                    ..Flags::default()
                },
                &["link"]
            )
            .1,
            b"/w/real\n"
        );
    }

    /// Defect 5: `-z` exists so a list of names can be handed to `xargs -0`,
    /// which means the NUL goes after *every* entry including the last.
    /// Measured: `readlink -z a b` writes `real\0real\0`.
    #[test]
    fn zero_terminates_every_entry_including_the_last() {
        let (ok, out, msg) = run(
            Flags {
                zero: true,
                ..Flags::default()
            },
            &["link", "link2"],
        );
        assert!(ok, "{msg}");
        assert_eq!(out, b"real\0real\0");
    }

    /// `-n` with one operand omits the delimiter entirely — including the NUL,
    /// measured: `readlink -nz link` writes `real` and nothing else.
    #[test]
    fn no_newline_omits_the_delimiter_for_a_single_operand() {
        let plain = run(
            Flags {
                no_newline: true,
                ..Flags::default()
            },
            &["link"],
        );
        assert_eq!(plain.1, b"real");
        assert!(
            plain.2.is_empty(),
            "no warning for one operand: {}",
            plain.2
        );

        let zeroed = run(
            Flags {
                no_newline: true,
                zero: true,
                ..Flags::default()
            },
            &["link"],
        );
        assert_eq!(zeroed.1, b"real");
    }

    /// Defect 5's rule that is easy to miss: with more than one operand, `-n`
    /// is *refused*, not obeyed. Without this the two answers would run
    /// together into one unusable string.
    #[test]
    fn no_newline_with_several_operands_warns_and_is_ignored() {
        let (ok, out, msg) = run(
            Flags {
                no_newline: true,
                ..Flags::default()
            },
            &["link", "link2"],
        );
        assert!(ok, "{msg}");
        assert_eq!(out, b"real\nreal\n", "the delimiter comes back");
        assert_eq!(
            msg,
            "readlink: ignoring --no-newline with multiple arguments\n"
        );
    }

    /// And the warning is not a per-operand diagnostic, so `-q` does not
    /// silence it. Measured: `readlink -q -n a b` still prints it.
    #[test]
    fn the_no_newline_warning_survives_quiet() {
        let (_, _, msg) = run(
            Flags {
                no_newline: true,
                verbose: false,
                zero: true,
                mode: None,
            },
            &["link", "link2"],
        );
        assert!(msg.contains("ignoring --no-newline"), "{msg}");
    }

    /// Measured: `readlink nosuch link` prints the second one's target and
    /// exits 1. A failure must not swallow the operands after it.
    #[test]
    fn one_failure_does_not_abandon_the_others() {
        let (ok, out, msg) = run(
            Flags {
                verbose: true,
                ..Flags::default()
            },
            &["nosuch", "link"],
        );
        assert!(!ok, "the missing one counts against the status");
        assert_eq!(out, b"real\n", "the good one is still printed");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
    }

    /// Defect 3's output half, which no amount of `OsString` on the input side
    /// would have fixed. A link's target is an arbitrary byte string; rendering
    /// it as text would replace `0xFF` with U+FFFD and hand the caller a name
    /// that does not exist.
    #[test]
    fn a_target_that_is_not_utf8_is_printed_byte_for_byte() {
        let (ok, out, msg) = run(Flags::default(), &["oddlink"]);
        assert!(ok, "{msg}");
        assert_eq!(out, b"od\xffd\n");
        assert!(
            String::from_utf8(out).is_err(),
            "the fixture must not be valid UTF-8, or it tests nothing"
        );
    }

    /// A newline in a file name must not be able to add a line that looks like
    /// a second diagnostic from `readlink`.
    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        let (ok, _, msg) = run(
            Flags {
                verbose: true,
                ..Flags::default()
            },
            &["a\nreadlink: /etc: Permission denied"],
        );
        assert!(!ok);
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.contains(r"\n"), "the newline must be escaped: {msg:?}");
    }
}
