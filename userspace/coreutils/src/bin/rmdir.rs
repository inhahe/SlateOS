//! rmdir — remove empty directories.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a directory name holding a byte
//! that is not valid UTF-8 — which on this OS is a legal name, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `rmdir` is
//! the sixth of the bins listed there, after `rm`, `mv`, `cp`, `ln` and `mkdir`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # `-p` was wrong in three separate ways, and all three are fixed here
//!
//! `-p` is this program's only real feature, and the old implementation got the
//! walk, the reporting and the exit status each wrong. Every claim below was
//! measured against GNU coreutils 9.4 under `LC_ALL=C.UTF-8`.
//!
//! 1. **It walked the path structurally, with [`std::path::Path::parent`],
//!    where GNU walks it *textually*.** The two disagree on any name that is
//!    not in canonical form, and the disagreement is visible:
//!
//!    | Command | GNU tries to remove | `Path::parent` would try |
//!    |---|---|---|
//!    | `rmdir -p ./cc` | `./cc`, then `.` | `./cc` only |
//!    | `rmdir -p a//b` | `a//b`, then `a` | `a//b`, then `a` |
//!    | `rmdir -p a/b/` | `a/b/`, then `a` | `a/b/`, then `a` |
//!
//!    The first row is the one that matters, and it is not a curiosity: GNU
//!    really does end `rmdir -p ./cc` with `failed to remove directory '.':
//!    Invalid argument` **and exit 1**, after successfully removing `cc`. A
//!    script that does `rmdir -p "$d"` with a `./`-prefixed `$d` sees a failure
//!    on GNU and a success here, which is exactly the sort of difference that
//!    is found by a user and not by us. The walk is now gnulib's
//!    `remove_parents` byte arithmetic — see [`strip_last_component`].
//!
//! 2. **An ancestor that could not be removed was ignored, silently.** The old
//!    loop was `if fs::remove_dir(&parent).is_err() { break; }` — no message,
//!    and no effect on the exit status. So `rmdir -p a/b` with a non-empty `a`
//!    printed nothing and exited **0**, where GNU prints
//!    `rmdir: failed to remove directory 'a': Directory not empty` and exits
//!    **1**. A `set -e` script could not tell that the thing it asked for had
//!    not happened. This is the defect worth remembering out of the three: the
//!    failure was not merely misreported, it was invisible.
//!
//! 3. **The ancestor message has a different shape from the operand's**, and
//!    the old code had neither. GNU's two are, verbatim:
//!
//!    | Which | Message |
//!    |---|---|
//!    | the operand | ``rmdir: failed to remove 'ne': Directory not empty`` |
//!    | an ancestor | ``rmdir: failed to remove directory 'a': Directory not empty`` |
//!
//!    The extra word `directory` is the only difference, and it is deliberate
//!    upstream — the operand message has to cover "it is not a directory at
//!    all", which is why `rmdir f` on a *file* says `failed to remove 'f': Not
//!    a directory` rather than calling `f` a directory in the same breath.
//!
//! # Three further defects, in the lines this rewrite replaced
//!
//! 4. **No long option worked, including `--help`, and `--` was not an
//!    end-of-options marker.** The parser compared each whole argument against
//!    the literal string `"-p"`, so `--parents` was refused, `--help` was
//!    refused, and a directory whose name begins with a dash could not be
//!    removed at all. Short options could not be bundled either.
//!
//! 5. **`missing operand` carried no referral.** GNU follows it with
//!    `Try 'rmdir --help' for more information.`
//!
//! 6. **Unknown options were reported in a shape no other utility uses** —
//!    `rmdir: unknown option: -q`, against GNU's `rmdir: invalid option -- 'q'`
//!    plus the referral.
//!
//! # `--path` and `--parents` are one option, which is why the table has aliases
//!
//! GNU's table lists both, with the same `val` (`'p'`); `--path` is the
//! deprecated spelling. `getopt_long` judges an abbreviation ambiguous by
//! comparing `val`s, not names, so `rmdir --p` **resolves** — measured, it works
//! — even though `--p` prefixes two entries. A name-only table would refuse it.
//! [`ALIASES`] is what tells [`coreutils::getopt`] the two are one option; see
//! `known-issues.md` → "`getopt` called an alias ambiguous against itself".
//!
//! Which of the two an abbreviation *names* in a diagnostic is table order:
//! `--pa` names `--path`, because `path` is declared first. That is why
//! [`LONG_OPTIONS`] is in GNU's declaration order and not alphabetised.
//!
//! # Options this implementation does not have
//!
//! `-v`/`--verbose` and `--ignore-fail-on-non-empty`. They are recognised and
//! rejected with a message saying they are not implemented, rather than ignored,
//! and they are listed in [`LONG_OPTIONS`] anyway because the table — not the
//! set of options handled — is what decides whether an abbreviation is
//! ambiguous. Drop `--verbose` and `rmdir --v` prints a version banner instead
//! of refusing.
//!
//! `--ignore-fail-on-non-empty` is the one where ignoring would be actively
//! wrong. It asks for a *non-empty* directory to be passed over quietly, exit 0
//! and all; ignoring the flag turns that into a diagnostic and exit 1, so a
//! script written to tolerate a non-empty directory would instead abort on one.
//! Rejecting it says so; implementing it is a small change and a separate one.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes, quoteaf_os};
use coreutils::stdfd::{self, Stream};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

/// `rmdir`'s usage status is 1 — measured: `rmdir -q x; echo $?` prints 1.
const RMDIR: Program = Program::new("rmdir", 1);

/// GNU `rmdir`'s `longopts[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Measured with an empty prefix, which matches every entry:
///
/// ```text
/// rmdir: option '--=x' is ambiguous; possibilities: '--ignore-fail-on-non-empty'
/// '--path' '--parents' '--verbose' '--help' '--version'
/// ```
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("ignore-fail-on-non-empty", Takes::Nothing),
    // Deprecated upstream, and kept for the same reason it is kept upstream:
    // removing it would change how `--p` and `--pa` resolve.
    ("path", Takes::Nothing),
    ("parents", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--path` is `--parents` under another name — both carry `val == 'p'` in GNU's
/// table. Without this, `rmdir --p` would be refused as ambiguous; with it, it
/// resolves, as measured.
const ALIASES: &[(&str, &str)] = &[("path", "parents")];

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct RmdirFlags {
    parents: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order.
    Run(RmdirFlags, Vec<OsString>),
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("rmdir (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, dirs)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            if remove_all(&flags, &dirs, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("rmdir: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: rmdir [OPTION]... DIRECTORY...
Remove the DIRECTORY(ies), if they are empty.

  -p, --parents   remove DIRECTORY and its ancestors;
                  e.g., 'rmdir -p a/b' is similar to 'rmdir a/b a'
      --help      display this help and exit
      --version   output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `rmdir`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `rmdir a -p b` is `rmdir -p a b` —
/// which is `getopt_long`'s default permuting behaviour, and the one thing the
/// previous hand-written parser got right.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, or
/// a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = RmdirFlags::default();
    let mut dirs: Vec<OsString> = Vec::new();
    let mut only_operands = false;

    for arg in args {
        if only_operands {
            dirs.push(arg.clone());
            continue;
        }
        let bytes = os_bytes(arg.as_os_str());

        if *bytes == *b"--" {
            only_operands = true;
        } else if *bytes == *b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is a directory called `-`. `rmdir` has no
            // standard-input operand for it to mean anything else — measured,
            // GNU answers `failed to remove '-': No such file or directory`.
            dirs.push(arg.clone());
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            match parse_long(body, &bytes, &mut flags)? {
                Some(request) => return Ok(request),
                None => continue,
            }
        } else {
            // Bytes, not `char`s. `-é` is two bytes in UTF-8, and iterating
            // `char`s would answer `invalid option -- 'é'` — an option nobody
            // typed, and one that cannot be typed, since options are single
            // bytes. It also would not survive an argument that is not UTF-8 at
            // all, which is the whole point of this rewrite.
            for &b in bytes.get(1..).unwrap_or_default() {
                apply_short(b, &mut flags)?;
            }
        }
    }

    Ok(Request::Run(flags, dirs))
}

/// Handle one `--name[=value]` argument.
///
/// Returns `Some(request)` for the two options that end parsing immediately, and
/// `None` for one that only sets a flag.
///
/// # Errors
///
/// The name resolving to nothing or to more than one option, a value given to an
/// option that takes none, or an option this implementation lacks.
fn parse_long(
    body: &[u8],
    whole: &[u8],
    flags: &mut RmdirFlags,
) -> Result<Option<Request>, getopt::Error> {
    // Split before resolving: the name is what gets matched, and the argument
    // *as typed* — `=VALUE` included — is what gets echoed back if it resolves
    // to nothing.
    let (typed, inline) = match body.iter().position(|&c| c == b'=') {
        Some(at) => (
            body.get(..at).unwrap_or_default(),
            Some(body.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (body, None),
    };
    // Every option name is ASCII, so a name that is not UTF-8 can match none of
    // them. It takes the unrecognised path — reported as the bytes typed —
    // rather than failing in some third way.
    let typed = std::str::from_utf8(typed).map_err(|_| RMDIR.unrecognized_option(whole))?;
    let (name, takes) = RMDIR.resolve_long_aliased(typed, whole, LONG_OPTIONS, ALIASES)?;

    if inline.is_some() && takes == Takes::Nothing {
        return Err(RMDIR.long_unwanted_argument(name));
    }

    match name {
        "help" => Ok(Some(Request::Help)),
        "version" => Ok(Some(Request::Version)),
        // Both spellings, because `resolve_long_aliased` returns the entry that
        // actually matched rather than the alias's target — deliberately, so
        // that a diagnostic names the option the user's abbreviation reached.
        "parents" | "path" => {
            flags.parents = true;
            Ok(None)
        }
        other => Err(unimplemented_long(other)),
    }
}

/// Handle one short option byte.
///
/// # Errors
///
/// A byte that is no option of `rmdir`'s, or one this implementation lacks.
fn apply_short(flag: u8, flags: &mut RmdirFlags) -> Result<(), getopt::Error> {
    match flag {
        b'p' => flags.parents = true,
        // GNU `rmdir`'s `getopt_long` string is exactly `"pv"`.
        b'v' => return Err(unimplemented_short(flag)),
        other => return Err(RMDIR.invalid_option(other)),
    }
    Ok(())
}

/// The diagnostic for an option that GNU `rmdir` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-v` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    RMDIR.usage_referring(format!(
        "option -{} is not implemented by this rmdir",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    RMDIR.usage_referring(format!(
        "option '--{name}' is not implemented by this rmdir"
    ))
}

// --------------------------------------------------------------- removing ---

/// Remove every directory the command line asked for, reporting failures to
/// `err`.
///
/// Returns `true` if everything asked for was removed. Takes the error sink as a
/// parameter rather than writing to `stderr` directly so the diagnostics — the
/// part of `rmdir` a caller actually sees when something goes wrong — can be
/// asserted on in tests. The old file had no test of this path at all.
///
/// One failure does not abandon the rest: measured, `rmdir nosuch g` reports
/// `nosuch`, still removes `g`, and exits 1.
fn remove_all<W: Write>(flags: &RmdirFlags, dirs: &[OsString], err: &mut W) -> bool {
    if dirs.is_empty() {
        // Module docs, defect 5: GNU follows this with the referral, and
        // `usage_referring` is what adds it.
        let _ = writeln!(
            err,
            "rmdir: {}",
            RMDIR.usage_referring("missing operand".into())
        );
        return false;
    }

    let mut ok = true;
    for dir in dirs {
        if let Err(e) = fs::remove_dir(Path::new(dir)) {
            // Straight marks. `mkdir`'s `cannot create directory ‘a’` is curly
            // and is the odd one out among its neighbours; the table in
            // `mkdir.rs`'s module docs is the measurement.
            let _ = writeln!(
                err,
                "rmdir: failed to remove {}: {}",
                quoteaf_os(dir),
                strerror(&e)
            );
            ok = false;
            // GNU reaches the ancestor walk only when the operand itself went,
            // which is why this is `continue` and not a fallthrough: there is
            // no point walking up from a directory that is still there.
            continue;
        }
        if flags.parents && !remove_parents(dir.as_os_str(), err) {
            ok = false;
        }
    }
    ok
}

/// Remove the ancestors of `operand`, innermost first, stopping at the first one
/// that will not go.
///
/// This is gnulib `rmdir.c`'s `remove_parents`, and the fidelity is the point —
/// see module docs, defect 1. Returns `true` if the walk ran to the end without
/// a failure, which includes the common case of there being no ancestors at all
/// (`rmdir -p foo`).
fn remove_parents<W: Write>(operand: &OsStr, err: &mut W) -> bool {
    let mut dir = strip_trailing_slashes(&os_bytes(operand)).to_vec();
    while strip_last_component(&mut dir) {
        let path = os_from_bytes(&dir);
        if let Err(e) = fs::remove_dir(Path::new(&path)) {
            // Module docs, defect 3: the ancestor message carries the extra word
            // `directory`, and the operand's does not.
            let _ = writeln!(
                err,
                "rmdir: failed to remove directory {}: {}",
                quoteaf_os(&path),
                strerror(&e)
            );
            return false;
        }
    }
    true
}

/// Drop a name's trailing `/`s, as gnulib's `strip_trailing_slashes` does.
///
/// A name that is *nothing but* slashes is the root, and the root is one slash
/// rather than none — `///` must not strip to the empty string, which names
/// nothing at all.
fn strip_trailing_slashes(dir: &[u8]) -> &[u8] {
    match dir.iter().rposition(|&b| b != b'/') {
        Some(last) => dir.get(..last.saturating_add(1)).unwrap_or(dir),
        None if dir.is_empty() => dir,
        None => dir.get(..1).unwrap_or(dir),
    }
}

/// Cut the last component off `dir` in place, returning whether there was one.
///
/// gnulib does this with `slash = strrchr (dir, '/')`, then backs up over a run
/// of slashes and writes a NUL after the byte before them — which is why
/// `a//b` yields `a` and not `a/`. The backing-up is not decoration: without it
/// the next iteration would find the leftover `/` and hand `rmdir` the same
/// directory again with a trailing slash.
///
/// The `new_len` guard has no counterpart upstream, where the loop is bounded
/// only by the removal failing. It is unreachable in practice — the one name
/// that shrinks to itself is `/`, and reaching a second step from `/` would
/// require having *successfully removed the root* — but a pure function that can
/// spin forever on an input is worse than one line of arithmetic.
fn strip_last_component(dir: &mut Vec<u8>) -> bool {
    let Some(slash) = dir.iter().rposition(|&b| b == b'/') else {
        return false;
    };
    let mut keep = slash;
    while keep > 0 && dir.get(keep) == Some(&b'/') {
        keep = keep.saturating_sub(1);
    }
    let new_len = keep.saturating_add(1);
    if new_len >= dir.len() {
        return false;
    }
    dir.truncate(new_len);
    true
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
    use std::path::PathBuf;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (RmdirFlags, Vec<String>) {
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
    fn no_args() {
        let (f, d) = run_parse(&[]);
        assert!(!f.parents);
        assert!(d.is_empty());
    }

    #[test]
    fn operands_only() {
        let (f, d) = run_parse(&["foo", "bar"]);
        assert!(!f.parents);
        assert_eq!(d, vec!["foo", "bar"]);
    }

    #[test]
    fn parents_flag() {
        assert!(run_parse(&["-p", "a/b"]).0.parents);
        assert!(run_parse(&["--parents", "a/b"]).0.parents);
        // The deprecated spelling is the same option, not a second one.
        assert!(run_parse(&["--path", "a/b"]).0.parents);
    }

    /// The measurement that motivated the alias support in
    /// [`coreutils::getopt`]. `--p` prefixes *two* table entries, and GNU still
    /// accepts it, because the two are one option. Measured: `rmdir --p p1/p2`
    /// removes both and exits 0.
    #[test]
    fn the_shared_prefix_of_path_and_parents_resolves() {
        for typed in ["--p", "--pa", "--par", "--pat", "--pare"] {
            assert!(
                run_parse(&[typed, "a/b"]).0.parents,
                "{typed} must resolve to the one -p option"
            );
        }
    }

    #[test]
    fn flag_may_follow_operands() {
        let (f, d) = run_parse(&["foo", "-p"]);
        assert!(f.parents);
        assert_eq!(d, vec!["foo"]);
    }

    #[test]
    fn repeating_the_flag_is_idempotent() {
        assert!(run_parse(&["-p", "-p", "foo"]).0.parents);
        assert!(run_parse(&["-pp", "foo"]).0.parents);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-"]).1, vec!["-"]);
    }

    /// Defect 4 in the module docs: this used to answer `unknown option: --`,
    /// so a directory whose name begins with a dash could not be removed.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]).1, vec!["-foo", "bar"]);
        let (f, d) = run_parse(&["--", "-p"]);
        assert!(!f.parents, "-p after -- is a directory name, not a flag");
        assert_eq!(d, vec!["-p"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).1.is_empty());
    }

    /// Also defect 4: every long option was refused, `--help` included.
    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    /// `--v` must stay ambiguous between `--verbose` and `--version`. This is
    /// the test that fails if someone prunes the table down to the one option
    /// this implementation acts on — which would silently turn `rmdir --v` from
    /// an error into a version banner.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--v"]);
        assert_eq!(
            e.sentence,
            "option '--v' is ambiguous; possibilities: '--verbose' '--version'"
        );
        assert_eq!(e.status, 1);
    }

    /// The whole table, in GNU's declaration order, as `rmdir --=x` prints it.
    /// An empty prefix matches every entry, so this pins the order itself —
    /// which is observable, and which decides that `--pa` names `--path` rather
    /// than `--parents`.
    #[test]
    fn the_empty_prefix_lists_the_table_in_order() {
        assert_eq!(
            fail(&["--=x"]).sentence,
            "option '--=x' is ambiguous; possibilities: \
             '--ignore-fail-on-non-empty' '--path' '--parents' '--verbose' \
             '--help' '--version'"
        );
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "a"]);
        assert_eq!(e.sentence, "invalid option -- 'q'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz=1", "a"]);
        assert_eq!(e.sentence, "unrecognized option '--zzz=1'");
        assert_eq!(e.status, 1);
    }

    /// `--ignore-fail-on-non-empty` ignored would abort a script written to
    /// tolerate a non-empty directory; `-v` ignored would drop output a caller
    /// asked for.
    #[test]
    fn unimplemented_options_are_rejected_by_name() {
        for typed in ["-v", "--verbose", "--ignore-fail-on-non-empty", "--i"] {
            let e = fail(&[typed, "a"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{typed}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--parents=yes", "a"]);
        assert_eq!(e.sentence, "option '--parents' doesn't allow an argument");
        // The alias keeps its own name in the diagnostic rather than being
        // canonicalised to the option it is an alias *for*.
        assert_eq!(
            fail(&["--path=yes", "a"]).sentence,
            "option '--path' doesn't allow an argument"
        );
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// The regression test for the reason this file was rewritten. On this OS a
    /// directory name may hold any byte but `/` and NUL, and byte `0x80` alone
    /// is not valid UTF-8, so an operand containing it cannot be a `String` at
    /// all — `env::args()` would have panicked before `rmdir` saw it.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-p"), bad.clone()]).unwrap() {
            Request::Run(f, d) => {
                assert!(f.parents);
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
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    /// The ancestor walk must cut a non-UTF-8 name on a byte boundary and hand
    /// back a name the syscall will accept — the reason
    /// [`coreutils::quote::os_from_bytes`] exists. `0x80` here sits in the
    /// *parent* component, so the parent that comes out is the one that cannot
    /// be a `String`.
    #[test]
    #[cfg(unix)]
    fn the_ancestor_walk_keeps_a_non_utf8_parent_intact() {
        use std::os::unix::ffi::OsStringExt;
        let mut dir = vec![b'a', 0x80, b'/', b'b'];
        assert!(strip_last_component(&mut dir));
        assert_eq!(dir, vec![b'a', 0x80]);
        let back = os_from_bytes(&dir);
        assert_eq!(back, OsString::from_vec(vec![b'a', 0x80]));
        assert!(back.to_str().is_none(), "the fixture must stay non-UTF-8");
    }

    /// The three tests above are `#[cfg(unix)]`, so on the development host —
    /// Windows — the regression tests for the bug this file was rewritten to fix
    /// **do not run at all**. That is the same blind spot that let the bug
    /// survive, so it is closed rather than noted. Windows has its own argument
    /// that no `String` can hold: an unpaired surrogate (a UTF-16 code unit in
    /// `0xD800..=0xDFFF` with no partner), which reaches the same `unwrap` in
    /// `env::args()` by a different route.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-p"), bad.clone()]).unwrap() {
            Request::Run(f, d) => {
                assert!(f.parents);
                assert_eq!(d, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x002D, 0x002D, 0xD800]);
        let e = parse_args(&[bad]).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    // ------------------------------------------------------ the path walk --

    /// Every ancestor of `operand`, in the order the walk produces them.
    fn ancestors(operand: &str) -> Vec<String> {
        let mut dir = strip_trailing_slashes(operand.as_bytes()).to_vec();
        let mut out = Vec::new();
        while strip_last_component(&mut dir) {
            out.push(String::from_utf8_lossy(&dir).into_owned());
        }
        out
    }

    #[test]
    fn trailing_slashes_come_off_but_the_root_survives() {
        assert_eq!(strip_trailing_slashes(b"a/b/"), b"a/b");
        assert_eq!(strip_trailing_slashes(b"a//"), b"a");
        assert_eq!(strip_trailing_slashes(b"a"), b"a");
        assert_eq!(strip_trailing_slashes(b"/"), b"/");
        assert_eq!(strip_trailing_slashes(b"///"), b"/");
        assert_eq!(strip_trailing_slashes(b""), b"");
    }

    /// Measured against GNU with `-pv`, which names every directory it tries.
    /// The `./cc` row is defect 1: `Path::parent` yields nothing there, and GNU
    /// yields `.` — and then fails on it.
    #[test]
    fn the_walk_matches_gnus() {
        // `rmdir -pv a3/b3/c3` → 'a3/b3/c3', 'a3/b3', 'a3'
        assert_eq!(ancestors("a3/b3/c3"), vec!["a3/b3", "a3"]);
        // `rmdir -pv aa/bb/` → 'aa/bb/', 'aa'
        assert_eq!(ancestors("aa/bb/"), vec!["aa"]);
        // `rmdir -pv a4//b4` → 'a4//b4', 'a4'
        assert_eq!(ancestors("a4//b4"), vec!["a4"]);
        // `rmdir -pv ./cc` → './cc', '.'  — then `failed to remove directory '.'`
        assert_eq!(ancestors("./cc"), vec!["."]);
        // A single component has no ancestors at all.
        assert!(ancestors("s1").is_empty());
        assert!(ancestors("s1/").is_empty());
        // Absolute names stop at the root rather than at the empty string.
        assert_eq!(ancestors("/x/y"), vec!["/x", "/"]);
        assert_eq!(ancestors("/x"), vec!["/"]);
    }

    /// The guard in [`strip_last_component`]. Upstream this input spins until
    /// the removal fails; here it terminates on its own.
    #[test]
    fn the_root_does_not_walk_forever() {
        let mut dir = b"/".to_vec();
        assert!(!strip_last_component(&mut dir));
        assert_eq!(dir, b"/");
    }

    // ---------------------------------------------------------- removing --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("rmdir_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Run `remove_all`, returning `(ok, diagnostics)`.
    fn run(parents: bool, dirs: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = dirs.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = remove_all(&RmdirFlags { parents }, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    /// [`run`], with the current directory moved to `dir` so the operands can be
    /// relative.
    ///
    /// Every `-p` test needs this, for two independent reasons:
    ///
    /// * **An absolute operand really does walk to the root.** Measured:
    ///   `rmdir -p /tmp/x/y` ends by trying to remove `/tmp`, and fails there
    ///   with `Permission denied`. So an absolute fixture would be testing the
    ///   host's permissions on `/tmp`, not this code, and could never assert a
    ///   clean run.
    /// * **The development host's absolute paths are not made of `/`.**
    ///   `std::env::temp_dir()` on Windows yields `C:\Users\…`, and the walk
    ///   looks for `/` only — correctly, because `/` is this OS's one separator
    ///   (`design.txt`). An absolute fixture there finds no ancestors at all and
    ///   the test passes for the wrong reason, or fails for one.
    ///
    /// The current directory is process-wide, so the lock serialises these
    /// against each other. A poisoned lock is taken anyway: it means an earlier
    /// test panicked, and the directory was restored before it did.
    fn run_in(dir: &Path, parents: bool, operands: &[&str]) -> (bool, String) {
        use std::sync::{Mutex, PoisonError};
        static CWD: Mutex<()> = Mutex::new(());
        let _guard = CWD.lock().unwrap_or_else(PoisonError::into_inner);

        let saved = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir).unwrap();
        let owned: Vec<OsString> = operands.iter().map(OsString::from).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = remove_all(&RmdirFlags { parents }, &owned, &mut err);
        // Before the caller's assertions, so a failing assertion cannot leave
        // the whole test binary in a scratch directory that is about to go.
        std::env::set_current_dir(saved).unwrap();
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    /// Defect 5: the referral used to be missing.
    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, msg) = run(false, &[]);
        assert!(!ok);
        assert!(msg.contains("missing operand"), "{msg}");
        assert!(msg.contains("Try 'rmdir --help'"), "{msg}");
    }

    #[test]
    fn an_empty_directory_goes() {
        let d = scratch("one");
        let a = d.join("a");
        fs::create_dir(&a).unwrap();
        let (ok, msg) = run(false, &[&a]);
        assert!(ok, "{msg}");
        assert!(!a.exists());
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `rmdir ne` with a file in it says `Directory not empty`, and
    /// the wording is POSIX's rather than the host's — see
    /// [`coreutils::errmsg`].
    #[test]
    fn a_non_empty_directory_is_refused_in_posix_words() {
        let d = scratch("nonempty");
        let ne = d.join("ne");
        fs::create_dir(&ne).unwrap();
        fs::write(ne.join("f"), b"x").unwrap();
        let (ok, msg) = run(false, &[&ne]);
        assert!(!ok);
        assert!(msg.contains("failed to remove"), "{msg}");
        assert!(msg.contains("Directory not empty"), "{msg}");
        assert!(!msg.contains("os error"), "host wording leaked: {msg}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_directory_is_refused_in_posix_words() {
        let d = scratch("missing");
        let (ok, msg) = run(false, &[&d.join("nosuch")]);
        assert!(!ok);
        assert!(msg.contains("No such file or directory"), "{msg}");
        assert!(!msg.contains("os error"), "host wording leaked: {msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// The operand message must *not* carry the word `directory` — a file is
    /// not one, and GNU's wording is careful about that. See defect 3.
    #[test]
    fn the_operand_message_does_not_call_the_operand_a_directory() {
        let d = scratch("file");
        let f = d.join("f");
        fs::write(&f, b"x").unwrap();
        let (ok, msg) = run(false, &[&f]);
        assert!(!ok);
        assert!(
            msg.contains("failed to remove ") && !msg.contains("failed to remove directory"),
            "{msg}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn dash_p_removes_the_ancestors_too() {
        let d = scratch("parents");
        fs::create_dir_all(d.join("a").join("b").join("c")).unwrap();
        let (ok, msg) = run_in(&d, true, &["a/b/c"]);
        assert!(ok, "{msg}");
        assert!(msg.is_empty(), "{msg}");
        assert!(!d.join("a").exists(), "the whole chain should be gone");
        // `a` has no `/` left in it, so the walk ends there rather than climbing
        // into the scratch directory, which is not ours to remove.
        assert!(d.is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// A relative operand with no separator at all has no ancestors, so `-p` is
    /// a no-op beyond the operand itself — and must not be an error.
    #[test]
    fn dash_p_on_a_single_component_removes_just_it() {
        let d = scratch("single");
        fs::create_dir(d.join("s1")).unwrap();
        let (ok, msg) = run_in(&d, true, &["s1"]);
        assert!(ok, "{msg}");
        assert!(msg.is_empty(), "{msg}");
        assert!(!d.join("s1").exists());
        assert!(d.is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 2, the one that mattered: this used to print nothing and exit 0.
    #[test]
    fn an_ancestor_that_will_not_go_is_reported_and_counts() {
        let d = scratch("ancestor");
        let a = d.join("a");
        fs::create_dir_all(a.join("b")).unwrap();
        fs::write(a.join("keep"), b"x").unwrap();

        let (ok, msg) = run_in(&d, true, &["a/b"]);
        assert!(!ok, "a non-empty ancestor must count against the status");
        assert!(!a.join("b").exists(), "the operand itself still goes");
        assert!(a.is_dir(), "the non-empty ancestor stays");
        assert!(
            msg.contains("failed to remove directory"),
            "the ancestor message carries the extra word: {msg}"
        );
        assert!(msg.contains("Directory not empty"), "{msg}");
        assert!(!msg.contains("os error"), "host wording leaked: {msg}");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// The walk stops at the *first* ancestor that will not go, rather than
    /// carrying on past it — otherwise a single obstruction would produce one
    /// message per level all the way to the root.
    #[test]
    fn the_walk_stops_at_the_first_obstruction() {
        let d = scratch("stop");
        fs::create_dir_all(d.join("a").join("b").join("c")).unwrap();
        fs::write(d.join("a").join("b").join("keep"), b"x").unwrap();

        let (ok, msg) = run_in(&d, true, &["a/b/c"]);
        assert!(!ok);
        assert!(d.join("a").join("b").is_dir());
        assert_eq!(
            msg.lines().count(),
            1,
            "one message, not one per level: {msg:?}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 1: `rmdir -p ./cc` removes `cc` and then tries to remove `.`,
    /// which fails — measured, GNU prints
    /// `rmdir: failed to remove directory '.': Invalid argument` and exits 1.
    /// A `Path::parent`-based walk yields no ancestor here at all and would
    /// report success, which is the disagreement this test pins.
    #[test]
    fn a_dot_prefixed_operand_walks_into_dot_and_fails_there() {
        let d = scratch("dot");
        fs::create_dir(d.join("cc")).unwrap();
        let (ok, msg) = run_in(&d, true, &["./cc"]);
        assert!(!ok, "removing '.' cannot succeed: {msg}");
        assert!(!d.join("cc").exists(), "the operand itself still goes");
        assert!(
            msg.contains("failed to remove directory '.'"),
            "the walk must reach '.': {msg}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    /// A trailing slash is passed to the syscall as typed — measured,
    /// `rmdir -pv aa/bb/` names `'aa/bb/'` — but it is stripped before the
    /// parent is computed, so the ancestor is `aa` and not `aa/`.
    #[test]
    fn a_trailing_slash_does_not_produce_a_slashed_ancestor() {
        let d = scratch("slash");
        fs::create_dir_all(d.join("aa").join("bb")).unwrap();
        let (ok, msg) = run_in(&d, true, &["aa/bb/"]);
        assert!(ok, "{msg}");
        assert!(!d.join("aa").exists());
        let _ = fs::remove_dir_all(&d);
    }

    /// Without `-p` the ancestors are nobody's business, even when they would
    /// have gone.
    #[test]
    fn without_dash_p_the_ancestors_stay() {
        let d = scratch("noparents");
        let deep = d.join("a").join("b");
        fs::create_dir_all(&deep).unwrap();
        let (ok, msg) = run(false, &[&deep]);
        assert!(ok, "{msg}");
        assert!(d.join("a").is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `rmdir nosuch g2` reports `nosuch`, still removes `g2`, and
    /// exits 1.
    #[test]
    fn one_failure_does_not_abandon_the_others() {
        let d = scratch("partial");
        let g = d.join("g");
        fs::create_dir(&g).unwrap();
        let (ok, msg) = run(false, &[&d.join("nosuch"), &g]);
        assert!(!ok, "the missing one must count against the status");
        assert!(!g.exists(), "{msg}");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// A newline in a directory name must not be able to add a line that looks
    /// like a second diagnostic from `rmdir`. The name is one that does not
    /// exist, so no file has to be created with it — which could not be done on
    /// the development host anyway.
    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        let d = scratch("forge");
        let evil = d.join("a\nrmdir: /etc: Permission denied");
        let (ok, msg) = run(false, &[&evil]);
        assert!(!ok);
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.contains(r"\n"), "the newline must be escaped: {msg:?}");
        let _ = fs::remove_dir_all(&d);
    }
}
