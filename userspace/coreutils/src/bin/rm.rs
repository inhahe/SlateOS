//! rm — remove files or directories.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte
//! that is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). `rm` is the
//! worst place in the tree for that failure: `rm -r` on a directory containing
//! one oddly-named file died part-way, having already deleted everything it
//! reached first, and printed a Rust backtrace rather than a diagnostic. See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based; that is the structural half of
//! the fix, and the reason the already-clean binaries were clean.
//!
//! # Three further bugs, in the lines this rewrite replaced
//!
//! 1. **`--` was not an end-of-options marker.** `rm -- -foo` — the documented
//!    way to remove a file whose name begins with a dash — answered
//!    `unknown option: --` and removed nothing.
//! 2. **`-f` suppressed every error, not just absence.** `rm -f` on a file it
//!    could not unlink (permission denied, read-only filesystem, a non-empty
//!    directory) printed nothing and exited 0. `-f` is documented to ignore
//!    *nonexistent* files; silently reporting success for a file that is still
//!    there is the failure direction that matters, because the caller is
//!    usually a script that goes on to assume the file is gone.
//! 3. **Symlinks were followed when deciding what a name was.** The old code
//!    asked `Path::exists` and `Path::is_dir`, both of which follow symlinks.
//!    So `rm dangling-link` reported `No such file or directory` and left the
//!    link in place — the link existed and was removable — and `rm -r
//!    link-to-dir` took the recursive branch on the basis of the *target*
//!    being a directory. `rm` never recurses through a symlink; it unlinks it.
//!    Both now go through `symlink_metadata`, which does not follow.
//!
//! # Options this implementation does not have
//!
//! `-i`, `-I`, `--interactive`, `-d`/`--dir`, `-v`/`--verbose`,
//! `--one-file-system`, `--preserve-root`, `--no-preserve-root` and
//! `---presume-input-tty` (three dashes; the name itself begins with one) are
//! recognised and rejected with a message saying they are not implemented,
//! rather than ignored. They are listed in [`LONG_OPTIONS`] anyway because the
//! table is what decides whether an abbreviation is ambiguous: drop
//! `--verbose` from it and `--v` resolves to `--version`, so `rm --v file`
//! would print a version banner and delete nothing instead of failing.
//!
//! Note that the absence of `--preserve-root` is not merely a missing flag:
//! this `rm` has **no root failsafe at all**, where GNU's refuses `rm -rf /`
//! by default. Logged in `known-issues.md`.

use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quoteaf_os;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// `rm`'s usage status is 1 — measured, and the majority case. The utilities
/// that use 2 (`ls`, `sort`, `grep`) are the ones that already spent 1 on a
/// real answer, which `rm` does not.
const RM: Program = Program::new("rm", 1);

/// GNU `rm`'s `long_opts[]`, **in its declaration order**, which is observable:
/// `getopt_long` lists an ambiguous prefix's candidates in table order.
///
/// Every entry is here whether or not this implementation acts on it — see the
/// module docs for why leaving one out is a silent wrong answer rather than a
/// missing feature.
///
/// Measured with `rm --=x`, which an empty prefix makes print the whole table:
///
/// ```text
/// rm: option '--=x' is ambiguous; possibilities: '--force' '--interactive'
/// '--one-file-system' '--no-preserve-root' '--preserve-root'
/// '---presume-input-tty' '--recursive' '--dir' '--verbose' '--help'
/// '--version'
/// ```
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("force", Takes::Nothing),
    ("interactive", Takes::Optional),
    ("one-file-system", Takes::Nothing),
    ("no-preserve-root", Takes::Nothing),
    ("preserve-root", Takes::Optional),
    // The leading hyphen is part of the *name*, not a typo: GNU's table really
    // holds `-presume-input-tty`, so it is typed with three dashes. It is
    // deliberately unspellable-by-accident because it is `rm`'s own internal
    // handle for "pretend stdin is a terminal", not a user-facing option.
    //
    // It earns its place here for the usual reason — it is a candidate that
    // decides ambiguity — but with a twist worth stating: because the name
    // begins with `-`, it is reachable only from a `---` prefix, so it can
    // never collide with a normal `--name`. `rm ---p` resolves to it in GNU,
    // and did not here until this entry existed.
    ("-presume-input-tty", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("dir", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct RmFlags {
    recursive: bool,
    force: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(RmFlags, Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("rm (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, paths)) => {
            let mut err = io::stderr().lock();
            if remove_all(&flags, &paths, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("rm: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: rm [OPTION]... [FILE]...
Remove (unlink) the FILE(s).

  -f, --force           ignore nonexistent files and arguments, never prompt
  -r, -R, --recursive   remove directories and their contents recursively
      --help        display this help and exit
      --version     output version information and exit

By default, rm does not remove directories.  Use the --recursive (-r or -R)
option to remove each listed directory, too, along with all of its contents.

To remove a file whose name starts with a '-', for example '-foo',
use one of these commands:
  rm -- -foo
  rm ./-foo
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `rm`'s argv into `(flags, paths)`.
///
/// Options and operands may be interleaved — `rm a -r b` sets `-r` and removes
/// both — which is `getopt_long`'s default permuting behaviour and what the
/// previous hand-written parser did too.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have,
/// or a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = RmFlags::default();
    let mut paths: Vec<OsString> = Vec::new();
    let mut only_operands = false;

    for arg in args {
        if only_operands {
            paths.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is a file called `-`, not an option. `rm` has no
            // standard-input operand for it to mean anything else.
            paths.push(arg.clone());
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            match parse_long(body, &bytes, &mut flags)? {
                Some(request) => return Ok(request),
                None => continue,
            }
        } else {
            // Bytes, not `char`s. `-é` is two bytes in UTF-8, and iterating
            // `char`s would answer `invalid option -- 'é'` — an option nobody
            // typed, and one that cannot be typed, since options are single
            // bytes. It also would not survive an argument that is not UTF-8
            // at all, which is the whole point of this rewrite.
            for &b in bytes.get(1..).unwrap_or_default() {
                apply_short(b, &mut flags)?;
            }
        }
    }

    Ok(Request::Run(flags, paths))
}

/// Handle one `--name[=value]` argument.
///
/// Returns `Some(request)` for the two options that end parsing immediately,
/// and `None` for one that only sets a flag.
///
/// # Errors
///
/// The name resolving to nothing or to more than one option, a value given to
/// an option that takes none, or an option this implementation lacks.
fn parse_long(
    body: &[u8],
    whole: &[u8],
    flags: &mut RmFlags,
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
    let typed = std::str::from_utf8(typed).map_err(|_| RM.unrecognized_option(whole))?;
    let (name, takes) = RM.resolve_long(typed, whole, LONG_OPTIONS)?;

    if inline.is_some() && takes == Takes::Nothing {
        return Err(RM.long_unwanted_argument(name));
    }

    match name {
        "help" => Ok(Some(Request::Help)),
        "version" => Ok(Some(Request::Version)),
        "force" => {
            flags.force = true;
            Ok(None)
        }
        "recursive" => {
            flags.recursive = true;
            Ok(None)
        }
        other => Err(unimplemented_long(other)),
    }
}

/// Handle one short option byte.
///
/// # Errors
///
/// A byte that is no option of `rm`'s, or one this implementation lacks.
fn apply_short(flag: u8, flags: &mut RmFlags) -> Result<(), getopt::Error> {
    match flag {
        b'r' | b'R' => flags.recursive = true,
        b'f' => flags.force = true,
        // GNU `rm`'s remaining short options. Rejected rather than ignored:
        // ignoring `-i` would turn a request to be *asked* before each deletion
        // into deletion without asking, which is the one direction a user of
        // this utility cannot afford to be surprised in.
        b'd' | b'i' | b'I' | b'v' => return Err(unimplemented_short(flag)),
        other => return Err(RM.invalid_option(other)),
    }
    Ok(())
}

/// The diagnostic for an option that GNU `rm` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-v` is not a typo, and
/// telling the user it is invalid sends them to check their spelling of a flag
/// they spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    RM.usage_referring(format!(
        "option -{} is not implemented by this rm",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    RM.usage_referring(format!("option '--{name}' is not implemented by this rm"))
}

#[cfg(unix)]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    a.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn arg_bytes(a: &OsString) -> Vec<u8> {
    a.to_string_lossy().into_owned().into_bytes()
}

// --------------------------------------------------------------- removal ---

/// Remove every operand, reporting failures to `err`.
///
/// Returns `true` if every operand was dealt with. Takes the error sink as a
/// parameter rather than writing to `stderr` directly so the diagnostics — the
/// part of `rm` a caller actually sees when something goes wrong — can be
/// asserted on in tests.
///
/// A failure on one operand does not stop the others: `rm a b c` with `b`
/// undeletable still removes `a` and `c`, and exits 1.
fn remove_all<W: Write>(flags: &RmFlags, paths: &[OsString], err: &mut W) -> bool {
    if paths.is_empty() {
        // `-f` makes a missing operand not an error, which is what lets
        // `rm -f $maybe_empty` work in a shell script.
        if flags.force {
            return true;
        }
        let _ = writeln!(err, "rm: {}", RM.usage_referring("missing operand".into()));
        return false;
    }

    let mut ok = true;
    for arg in paths {
        if !remove_one(flags, arg, err) {
            ok = false;
        }
    }
    ok
}

/// Remove one operand. Returns `false` if it should count against the exit
/// status.
fn remove_one<W: Write>(flags: &RmFlags, arg: &OsString, err: &mut W) -> bool {
    let path = Path::new(arg);

    // `symlink_metadata`, not `exists`/`is_dir`: both of those follow symlinks,
    // and `rm` must not. A symlink is unlinked as itself, whatever it points at
    // — including nothing.
    let metadata = match fs::symlink_metadata(path) {
        Ok(m) => m,
        // `-f` ignores a file that is *not there*. It is matched on `ErrorKind`
        // rather than on the message, so the two are independent: the branch
        // decides whether to be quiet, `strerror` decides the wording.
        Err(e) if e.kind() == io::ErrorKind::NotFound && flags.force => return true,
        Err(e) => {
            // Reported even under `-f` unless it was the case above: `-f`
            // ignores a file that is not there; a file that *is* there and could
            // not be examined is a real failure, and swallowing it tells a
            // script the file is gone when it is not.
            //
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`] — on a Windows *host*
            // `{e}` says `The system cannot find the file specified. (os error
            // 2)`, which is neither POSIX's wording nor what this utility prints
            // on the target it ships on. It used to be spelled out by hand for
            // this one errno, which fixed the common case and left every other
            // one reading in the host's words.
            let why = strerror(&e);
            let _ = writeln!(err, "rm: cannot remove {}: {why}", quoteaf_os(arg));
            return false;
        }
    };

    let result = if metadata.is_dir() {
        if !flags.recursive {
            let _ = writeln!(
                err,
                "rm: cannot remove {}: Is a directory",
                quoteaf_os(arg)
            );
            return false;
        }
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };

    match result {
        Ok(()) => true,
        // Vanished between the stat above and the unlink here. Under `-f` that
        // is the outcome asked for, so it is not a failure.
        Err(e) if flags.force && e.kind() == io::ErrorKind::NotFound => true,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(err, "rm: cannot remove {}: {why}", quoteaf_os(arg));
            false
        }
    }
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

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, paths)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (RmFlags, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, p) => (
                f,
                p.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
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
        let (f, p) = run_parse(&[]);
        assert!(!f.recursive && !f.force);
        assert!(p.is_empty());
    }

    #[test]
    fn just_paths() {
        let (f, p) = run_parse(&["a", "b"]);
        assert!(!f.recursive && !f.force);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn dash_r_sets_recursive() {
        let (f, _) = run_parse(&["-r", "a"]);
        assert!(f.recursive);
        assert!(!f.force);
    }

    #[test]
    fn capital_r_also_recursive() {
        let (f, _) = run_parse(&["-R", "a"]);
        assert!(f.recursive);
    }

    #[test]
    fn dash_f_sets_force() {
        let (f, _) = run_parse(&["-f", "a"]);
        assert!(!f.recursive);
        assert!(f.force);
    }

    #[test]
    fn combined_rf() {
        let (f, p) = run_parse(&["-rf", "a"]);
        assert!(f.recursive && f.force);
        assert_eq!(p, vec!["a"]);
    }

    #[test]
    fn combined_fr_order_irrelevant() {
        let (f, _) = run_parse(&["-fr", "a"]);
        assert!(f.recursive && f.force);
    }

    #[test]
    fn split_flags() {
        let (f, _) = run_parse(&["-r", "-f", "a"]);
        assert!(f.recursive && f.force);
    }

    #[test]
    fn dash_alone_is_path() {
        let (_, p) = run_parse(&["-"]);
        assert_eq!(p, vec!["-"]);
    }

    #[test]
    fn flag_at_end() {
        let (f, p) = run_parse(&["a", "-r"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["a"]);
    }

    // -------------------------------------------------- parsing: new cases --

    /// The documented way to remove a file whose name begins with a dash. The
    /// hand-written parser answered `unknown option: --` and removed nothing.
    #[test]
    fn double_dash_ends_options() {
        let (f, p) = run_parse(&["-r", "--", "-foo", "-r"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["-foo", "-r"], "after --, everything is an operand");
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        let (_, p) = run_parse(&["--"]);
        assert!(p.is_empty());
    }

    #[test]
    fn long_force_and_recursive() {
        let (f, p) = run_parse(&["--force", "--recursive", "a"]);
        assert!(f.force && f.recursive);
        assert_eq!(p, vec!["a"]);
    }

    #[test]
    fn long_options_abbreviate() {
        let (f, _) = run_parse(&["--rec", "a"]);
        assert!(f.recursive);
    }

    /// `--v` must not resolve to `--version`. `--verbose` is in the table
    /// precisely so that this stays ambiguous rather than silently printing a
    /// version banner and deleting nothing.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--v", "a"]);
        assert!(
            e.sentence.contains("ambiguous"),
            "want ambiguous, got {:?}",
            e.sentence
        );
        assert!(e.sentence.contains("'--verbose'"), "{:?}", e.sentence);
        assert!(e.sentence.contains("'--version'"), "{:?}", e.sentence);
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-x", "a"]);
        assert_eq!(e.sentence, "invalid option -- 'x'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn combined_with_unknown_errors() {
        let e = fail(&["-rx", "a"]);
        assert_eq!(e.sentence, "invalid option -- 'x'");
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz", "a"]);
        assert_eq!(e.sentence, "unrecognized option '--zzz'");
    }

    /// A recognised GNU option we lack must say so, rather than claiming the
    /// user mistyped a flag they spelled correctly — and above all must not be
    /// ignored. Ignoring `-i` would turn "ask me before each deletion" into
    /// "delete without asking".
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in ["-i", "-I", "-d", "-v"] {
            let e = fail(&[flag, "a"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{flag}: {:?}",
                e.sentence
            );
            assert!(
                e.sentence.contains(flag),
                "{flag} should name itself: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        for name in [
            "--interactive",
            "--one-file-system",
            "--no-preserve-root",
            "--preserve-root",
            "--dir",
            "--verbose",
            "---presume-input-tty",
        ] {
            let e = fail(&[name, "a"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{name}: {:?}",
                e.sentence
            );
            assert!(e.sentence.contains(name), "{name}: {:?}", e.sentence);
        }
    }

    /// The hyphen-named entry is reachable only through a `---` prefix, and it
    /// is the sole candidate there, so it resolves from a single letter. GNU
    /// answers `rm ---p` with `missing operand`, i.e. it resolved; the check is
    /// that we resolve it too rather than calling it unrecognised.
    #[test]
    fn a_triple_dash_prefix_reaches_the_hyphen_named_option() {
        let e = fail(&["---p", "a"]);
        assert!(
            e.sentence.contains("'---presume-input-tty' is not implemented"),
            "{:?}",
            e.sentence
        );
    }

    /// And it stays out of the way of the ordinary names: `--p` must still mean
    /// `--preserve-root` alone, because `-presume-input-tty` does not start
    /// with `p`. This is the test that fails if someone "fixes" the table by
    /// dropping the leading hyphen.
    #[test]
    fn the_hyphen_named_option_does_not_disturb_double_dash_prefixes() {
        let e = fail(&["--p", "a"]);
        assert!(
            e.sentence.contains("'--preserve-root' is not implemented"),
            "{:?}",
            e.sentence
        );
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--force=1", "a"]);
        assert_eq!(e.sentence, "option '--force' doesn't allow an argument");
    }

    /// The reason this file was rewritten. A `String`-based parser panics here
    /// rather than returning; reaching the assert at all is the test.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        let parsed = parse_args(&[OsString::from("-r"), bad.clone()]).unwrap();
        match parsed {
            Request::Run(f, p) => {
                assert!(f.recursive);
                assert_eq!(p, vec![bad], "the operand must survive byte-for-byte");
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    /// The same for an *option-shaped* argument that is not UTF-8: it must be
    /// rejected as an option, not panicked on, and not silently treated as a
    /// file.
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

    /// The two tests above are `#[cfg(unix)]`, so on the development host —
    /// Windows — the regression tests for the bug this file was rewritten to
    /// fix **do not run at all**. That is the same blind spot that let the bug
    /// survive in the first place, so it is worth closing rather than noting.
    ///
    /// Windows has its own argument that no `String` can hold: an unpaired
    /// surrogate. `OsString` stores it (as WTF-8), `String` cannot represent
    /// it, and `env::args()` unwraps on exactly it — the same `unwrap`, in the
    /// same std function, reached by a different route. So the host can run a
    /// real regression test after all.
    ///
    /// It is not a full substitute: on Windows `arg_bytes` goes through
    /// `to_string_lossy`, so an option *name* that is not representable arrives
    /// as U+FFFD rather than as its own bytes. Both routes end at
    /// `unrecognized option`, which is what these assert, but only the unix
    /// build carries the bytes through untouched.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-r"), bad.clone()]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
                assert_eq!(p, vec![bad], "the operand must survive unchanged");
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

    // ------------------------------------------------------------ removal --

    fn scratch(stem: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("rm_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `remove_all` plus whatever it wrote to its error sink.
    fn remove(flags: &RmFlags, paths: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = remove_all(flags, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    const PLAIN: RmFlags = RmFlags {
        recursive: false,
        force: false,
    };
    const FORCE: RmFlags = RmFlags {
        recursive: false,
        force: true,
    };
    const RECURSIVE: RmFlags = RmFlags {
        recursive: true,
        force: false,
    };

    #[test]
    fn removes_a_file() {
        let dir = scratch("file");
        let f = dir.join("a");
        fs::write(&f, b"x").unwrap();
        let (ok, err) = remove(&PLAIN, &[&f]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!f.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_operand_without_force_is_an_error() {
        let (ok, err) = remove(&PLAIN, &[]);
        assert!(!ok);
        assert!(err.contains("missing operand"), "{err}");
        assert!(err.contains("Try 'rm --help'"), "{err}");
    }

    #[test]
    fn missing_operand_with_force_is_not() {
        let (ok, err) = remove(&FORCE, &[]);
        assert!(ok);
        assert_eq!(err, "");
    }

    #[test]
    fn absent_file_reports_unless_forced() {
        let dir = scratch("absent");
        let f = dir.join("nope");
        let (ok, err) = remove(&PLAIN, &[&f]);
        assert!(!ok);
        assert!(err.contains("No such file or directory"), "{err}");

        let (ok, err) = remove(&FORCE, &[&f]);
        assert!(ok, "{err}");
        assert_eq!(err, "", "-f must be silent about a file that is not there");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_needs_recursive() {
        let dir = scratch("isdir");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        let (ok, err) = remove(&PLAIN, &[&sub]);
        assert!(!ok);
        assert!(err.contains("Is a directory"), "{err}");
        assert!(sub.is_dir(), "the directory must still be there");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The `-f` bug: a directory is *present* and cannot be unlinked, so `-f`
    /// must not report success. The old code returned 0 and printed nothing.
    #[test]
    fn force_does_not_hide_a_directory_it_cannot_remove() {
        let dir = scratch("forcedir");
        let sub = dir.join("d");
        fs::create_dir(&sub).unwrap();
        let (ok, err) = remove(&FORCE, &[&sub]);
        assert!(!ok, "-f must not claim success for a file still present");
        assert!(err.contains("Is a directory"), "{err}");
        assert!(sub.is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn recursive_removes_a_tree() {
        let dir = scratch("tree");
        let sub = dir.join("d");
        fs::create_dir_all(sub.join("inner")).unwrap();
        fs::write(sub.join("inner").join("f"), b"x").unwrap();
        let (ok, err) = remove(&RECURSIVE, &[&sub]);
        assert!(ok, "{err}");
        assert!(!sub.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    /// One bad operand must not stop the others, and must still be reported.
    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("continue");
        let a = dir.join("a");
        let c = dir.join("c");
        fs::write(&a, b"x").unwrap();
        fs::write(&c, b"x").unwrap();
        let missing = dir.join("b");
        let (ok, err) = remove(&PLAIN, &[&a, &missing, &c]);
        assert!(!ok);
        assert!(err.contains("No such file or directory"), "{err}");
        assert!(!a.exists(), "operands before the failure are removed");
        assert!(!c.exists(), "operands after the failure are removed");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A dangling symlink exists and is removable. The old code asked
    /// `Path::exists`, which follows the link, so it answered "No such file or
    /// directory" and left the link behind.
    #[test]
    #[cfg(unix)]
    fn removes_a_dangling_symlink() {
        let dir = scratch("dangling");
        let link = dir.join("l");
        std::os::unix::fs::symlink(dir.join("does-not-exist"), &link).unwrap();
        let (ok, err) = remove(&PLAIN, &[&link]);
        assert!(ok, "{err}");
        assert!(fs::symlink_metadata(&link).is_err(), "link must be gone");
        let _ = fs::remove_dir_all(&dir);
    }

    /// `rm` unlinks a symlink; it never follows one into a directory. The old
    /// code took the recursive branch on the basis of the *target* being a
    /// directory.
    #[test]
    #[cfg(unix)]
    fn a_symlink_to_a_directory_is_unlinked_not_followed() {
        let dir = scratch("symdir");
        let target = dir.join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), b"x").unwrap();
        let link = dir.join("l");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        // Without -r: a symlink is not a directory, so this must succeed.
        let (ok, err) = remove(&PLAIN, &[&link]);
        assert!(ok, "a symlink is not a directory: {err}");
        assert!(fs::symlink_metadata(&link).is_err(), "link must be gone");
        assert!(
            target.join("keep").exists(),
            "the target's contents must be untouched"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The end-to-end version of the reason for this rewrite: a file whose name
    /// is not valid UTF-8 is created, passed through parsing, and removed.
    #[test]
    #[cfg(unix)]
    fn removes_a_file_whose_name_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let dir = scratch("nonutf8");
        let mut name = dir.as_os_str().to_owned().into_vec();
        name.extend_from_slice(b"/a\x80b");
        let path = std::path::PathBuf::from(OsString::from_vec(name));
        fs::write(&path, b"x").unwrap();
        assert!(fs::symlink_metadata(&path).is_ok(), "setup failed");

        let (ok, err) = remove(&PLAIN, &[&path]);
        assert!(ok, "{err}");
        assert!(fs::symlink_metadata(&path).is_err(), "file must be gone");
        let _ = fs::remove_dir_all(&dir);
    }
}
