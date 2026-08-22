//! mkdir — make directories.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a directory name holding a byte
//! that is not valid UTF-8 — which on this OS is a legal name, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `mkdir` is
//! the fifth of the bins listed there, after `rm`, `mv`, `cp` and `ln`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Four further defects, in the lines this rewrite replaced
//!
//! 1. **No long option worked, including `--help`, and `--` was not an
//!    end-of-options marker.** The parser compared each whole argument against
//!    the literal string `"-p"` and treated everything else beginning with `-`
//!    as unknown. So `--parents` — the spelling the option is documented under
//!    almost everywhere — was refused, `--help` was refused, and a directory
//!    whose name begins with a dash could not be created at all, because there
//!    was no way to stop option parsing. Short options could not be bundled
//!    either: `mkdir -pv` failed as one unknown option even though `-p` is the
//!    one option this program has.
//!
//! 2. **The diagnostic used the wrong quoting style.** It reached for
//!    [`coreutils::quote::quoteaf_os`], which produces straight `'a'`, where GNU
//!    `mkdir` produces curly `‘a’`. That is not a nitpick about typography, it
//!    is a fact worth writing down because it is so easy to get backwards:
//!    **the quoting style is a property of the individual message, not of the
//!    utility**, and `mkdir` is the odd one out among its neighbours. Measured
//!    under `LANG=C.UTF-8`, GNU coreutils 9.4:
//!
//!    | Message | Marks |
//!    |---|---|
//!    | ``mkdir: cannot create directory ‘a’: File exists`` | curly ([`quote`][coreutils::quote::quote]) |
//!    | ``mkdir: created directory 'v1'`` (`-v`) | straight ([`quoteaf`][coreutils::quote::quoteaf]) |
//!    | ``rmdir: failed to remove 'nosuch'`` | straight |
//!    | ``cp: cannot stat 'nosuch'`` | straight |
//!    | ``rm: cannot remove 'nosuch'`` | straight |
//!    | ``touch: cannot touch '/nope/x'`` | straight |
//!    | ``ln: failed to create hard link 'g'`` | straight |
//!
//!    The two `mkdir` rows are the point: one program, two styles, in the same
//!    run. Anyone "fixing" this file for consistency with `rm` and `cp` will
//!    make it wrong again, which is why the measurement is recorded here rather
//!    than left implicit in a call.
//!
//! 3. **`missing operand` carried no referral.** GNU follows it with
//!    `Try 'mkdir --help' for more information.`, which was the only pointer a
//!    user had to a `--help` that — see defect 1 — did not work anyway.
//!
//! 4. **Unknown options were reported in a shape no other utility uses.**
//!    `mkdir: unknown option: -q`, against GNU's `mkdir: invalid option -- 'q'`
//!    plus the referral. Going through [`coreutils::getopt`] fixes this for the
//!    same reason it fixes the ambiguity handling: the wording is the library's,
//!    measured once, rather than each bin's own guess.
//!
//! # Options this implementation does not have
//!
//! Everything except `-p`/`--parents`. They are recognised and rejected with a
//! message saying they are not implemented, rather than ignored, and they are
//! listed in [`LONG_OPTIONS`] anyway because the table is what decides whether
//! an abbreviation is ambiguous — `--v` must stay ambiguous between `--verbose`
//! and `--version`, which is measured GNU behaviour and is exactly what a table
//! pruned to the one implemented option would get wrong: `mkdir --v` would print
//! a version instead of refusing.
//!
//! `-m`/`--mode` is the one where ignoring would actually be harmful, and it is
//! the reason none of them are ignored. It asks for the new directory to be
//! created with a specific permission mode, and the usual reason to ask is that
//! the default is too permissive — `mkdir -m 700 ~/.ssh`. Ignored, the directory
//! is created 0755 and the user is told nothing, so a directory they asked to be
//! private is world-readable. That is the same class of defect as `cp`'s
//! attribute handling, recorded in this crate's `known-issues.md`. Implementing
//! it properly needs a symbolic-mode parser (`u=rwx,go=`); the only one in the
//! tree is private to `chmod.rs`, so `-m` waits for that to be lifted into a
//! shared module rather than getting a second, subtly different copy here.

use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quote_os;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// `mkdir`'s usage status is 1 — measured: `mkdir -q z; echo $?` prints 1. See
/// [`coreutils::getopt::Error`] for the handful of utilities that differ.
const MKDIR: Program = Program::new("mkdir", 1);

/// GNU `mkdir`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Measured with the instrument described in
/// [`Program::resolve_long`] — an empty prefix matches everything, so
/// `mkdir --=x` prints the whole table:
///
/// ```text
/// mkdir: option '--=x' is ambiguous; possibilities: '--context' '--mode'
/// '--parents' '--verbose' '--help' '--version'
/// ```
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("context", Takes::Optional),
    ("mode", Takes::Required),
    ("parents", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MkdirFlags {
    parents: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order.
    Run(MkdirFlags, Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("mkdir (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, dirs)) => {
            let mut err = io::stderr().lock();
            if make_all(&flags, &dirs, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("mkdir: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: mkdir [OPTION]... DIRECTORY...
Create the DIRECTORY(ies), if they do not already exist.

  -p, --parents   no error if existing, make parent directories as needed
      --help      display this help and exit
      --version   output version information and exit

To create a directory whose name starts with a '-', for example '-foo',
use one of these commands:
  mkdir -- -foo
  mkdir ./-foo
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `mkdir`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `mkdir a -p b` is `mkdir -p a b` —
/// which is `getopt_long`'s default permuting behaviour and what the previous
/// hand-written parser did too, being the one thing it got right.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, or
/// a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = MkdirFlags::default();
    let mut dirs: Vec<OsString> = Vec::new();
    let mut only_operands = false;

    for arg in args {
        if only_operands {
            dirs.push(arg.clone());
            continue;
        }
        let bytes = arg_bytes(arg);

        if bytes == b"--" {
            only_operands = true;
        } else if bytes == b"-" || bytes.first() != Some(&b'-') {
            // A lone `-` is a directory called `-`. `mkdir` has no
            // standard-input operand for it to mean anything else.
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
    flags: &mut MkdirFlags,
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
    let typed = std::str::from_utf8(typed).map_err(|_| MKDIR.unrecognized_option(whole))?;
    let (name, takes) = MKDIR.resolve_long(typed, whole, LONG_OPTIONS)?;

    if inline.is_some() && takes == Takes::Nothing {
        return Err(MKDIR.long_unwanted_argument(name));
    }

    match name {
        "help" => Ok(Some(Request::Help)),
        "version" => Ok(Some(Request::Version)),
        "parents" => {
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
/// A byte that is no option of `mkdir`'s, or one this implementation lacks.
fn apply_short(flag: u8, flags: &mut MkdirFlags) -> Result<(), getopt::Error> {
    match flag {
        b'p' => flags.parents = true,
        // GNU `mkdir`'s remaining short options, from its `getopt_long` string.
        // Rejected rather than ignored: see the module docs — `-m` ignored
        // publishes a directory the user asked to be private.
        b'm' | b'v' | b'Z' => return Err(unimplemented_short(flag)),
        other => return Err(MKDIR.invalid_option(other)),
    }
    Ok(())
}

/// The diagnostic for an option that GNU `mkdir` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-m` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    MKDIR.usage_referring(format!(
        "option -{} is not implemented by this mkdir",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    MKDIR.usage_referring(format!("option '--{name}' is not implemented by this mkdir"))
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

// ---------------------------------------------------------------- creating --

/// Create every directory the command line asked for, reporting failures to
/// `err`.
///
/// Returns `true` if every one was created. Takes the error sink as a parameter
/// rather than writing to `stderr` directly so the diagnostics — the part of
/// `mkdir` a caller actually sees when something goes wrong — can be asserted on
/// in tests. The old file had no test of this path at all.
///
/// One failure does not abandon the rest. Measured: `mkdir a g` with `a` already
/// present reports `a`, still creates `g`, and exits 1.
fn make_all<W: Write>(flags: &MkdirFlags, dirs: &[OsString], err: &mut W) -> bool {
    if dirs.is_empty() {
        // Module docs, defect 3: GNU follows this with the referral, and
        // `usage_referring` is what adds it.
        let _ = writeln!(
            err,
            "mkdir: {}",
            MKDIR.usage_referring("missing operand".into())
        );
        return false;
    }

    let mut ok = true;
    for dir in dirs {
        let result = if flags.parents {
            // `create_dir_all` is silent when the path is already a directory,
            // which is `-p`'s whole point, and still fails when it is an
            // existing *file* — matching GNU, which reports `File exists` for
            // `mkdir -p f`.
            fs::create_dir_all(Path::new(dir))
        } else {
            fs::create_dir(Path::new(dir))
        };
        if let Err(e) = result {
            // `quote_os`, not `quoteaf_os`: curly marks. Module docs, defect 2 —
            // this one message is the exception among its neighbours, and the
            // table there is the evidence.
            let _ = writeln!(
                err,
                "mkdir: cannot create directory {}: {e}",
                quote_os(dir)
            );
            ok = false;
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
    use std::path::PathBuf;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (MkdirFlags, Vec<String>) {
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
        // Measured: `mkdir --p q` works, so `--p` must resolve rather than be
        // ambiguous — `parents` is the only option beginning with `p`.
        assert!(run_parse(&["--p", "a/b"]).0.parents);
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

    /// Defect 1 in the module docs: this used to answer `unknown option: --`,
    /// so a directory whose name begins with a dash could not be created.
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

    /// Also defect 1: every long option was refused, `--help` included.
    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    /// `--v` must stay ambiguous between `--verbose` and `--version`. This is
    /// the test that fails if someone prunes the table down to the one option
    /// this implementation actually acts on — which would silently turn
    /// `mkdir --v` from an error into a version banner.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--v"]);
        assert_eq!(
            e.sentence,
            "option '--v' is ambiguous; possibilities: '--verbose' '--version'"
        );
        assert_eq!(e.status, 1);
    }

    /// The whole table, in GNU's declaration order, as `mkdir --=x` prints it.
    /// An empty prefix matches every entry, so this pins the order itself —
    /// which is observable and which the ambiguity message above depends on.
    #[test]
    fn the_empty_prefix_lists_the_table_in_order() {
        assert_eq!(
            fail(&["--=x"]).sentence,
            "option '--=x' is ambiguous; possibilities: '--context' '--mode' \
             '--parents' '--verbose' '--help' '--version'"
        );
    }

    #[test]
    fn unambiguous_abbreviations_still_resolve() {
        // `--c` prefixes only `--context`, so it resolves — and is then refused
        // as unimplemented rather than as unrecognised.
        assert!(fail(&["--c"]).sentence.contains("not implemented"));
        assert!(fail(&["--m"]).sentence.contains("not implemented"));
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

    /// `-m` ignored would create a world-readable directory the user asked to
    /// be private; `-Z` ignored would drop a security context.
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in ["-m", "-v", "-Z"] {
            let e = fail(&[flag, "a"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{flag}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        for name in ["--context", "--mode", "--verbose"] {
            let e = fail(&[name, "a"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{name}: {:?}",
                e.sentence
            );
        }
        // With a value attached, too: `--mode` takes one, so the value is
        // accepted by the parser and the refusal still has to come from the
        // name.
        assert!(fail(&["--mode=700", "a"]).sentence.contains("not implemented"));
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--parents=yes", "a"]);
        assert_eq!(e.sentence, "option '--parents' doesn't allow an argument");
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// The regression test for the reason this file was rewritten. On this OS a
    /// directory name may hold any byte but `/` and NUL, and byte `0x80` alone
    /// is not valid UTF-8, so an operand containing it cannot be a `String` at
    /// all — `env::args()` would have panicked before `mkdir` saw it.
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

    /// The two tests above are `#[cfg(unix)]`, so on the development host —
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

    // ----------------------------------------------------------- creating --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("mkdir_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Run `make_all`, returning `(ok, diagnostics)`.
    fn run(parents: bool, dirs: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = dirs.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = make_all(&MkdirFlags { parents }, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    /// Defect 3: the referral used to be missing.
    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, msg) = run(false, &[]);
        assert!(!ok);
        assert!(msg.contains("missing operand"), "{msg}");
        assert!(msg.contains("Try 'mkdir --help'"), "{msg}");
    }

    #[test]
    fn one_directory_is_created() {
        let d = scratch("one");
        let a = d.join("a");
        let (ok, msg) = run(false, &[&a]);
        assert!(ok, "{msg}");
        assert!(a.is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `mkdir b/c` with no `b` fails, and `-p` is what makes it work.
    #[test]
    fn a_missing_parent_needs_dash_p() {
        let d = scratch("parent");
        let deep = d.join("b").join("c");
        let (ok, msg) = run(false, &[&deep]);
        assert!(!ok);
        assert!(msg.contains("cannot create directory"), "{msg}");
        assert!(!deep.exists());

        let (ok, msg) = run(true, &[&deep]);
        assert!(ok, "{msg}");
        assert!(deep.is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `mkdir -p a` on an existing directory exits 0 and says nothing;
    /// without `-p` it is `File exists` and exit 1.
    #[test]
    fn dash_p_is_silent_about_an_existing_directory() {
        let d = scratch("existing");
        let a = d.join("a");
        fs::create_dir(&a).unwrap();

        let (ok, msg) = run(true, &[&a]);
        assert!(ok, "{msg}");
        assert!(msg.is_empty(), "{msg}");

        let (ok, msg) = run(false, &[&a]);
        assert!(!ok);
        assert!(msg.contains("cannot create directory"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `mkdir -p f` where `f` is a *file* still fails. `-p` excuses an
    /// existing directory, not an existing anything.
    #[test]
    fn dash_p_does_not_excuse_an_existing_file() {
        let d = scratch("file");
        let f = d.join("f");
        fs::write(&f, b"x").unwrap();
        let (ok, msg) = run(true, &[&f]);
        assert!(!ok, "{msg}");
        assert!(msg.contains("cannot create directory"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 2. The marks are curly here and straight in `rm`, `cp`, `ln` and
    /// `rmdir`; this is the assertion that stops someone "harmonising" them.
    #[test]
    fn the_failure_message_uses_curly_marks() {
        let d = scratch("quoting");
        let a = d.join("a");
        fs::create_dir(&a).unwrap();
        let (ok, msg) = run(false, &[&a]);
        assert!(!ok);
        assert!(msg.contains('\u{2018}'), "no opening mark: {msg:?}");
        assert!(msg.contains('\u{2019}'), "no closing mark: {msg:?}");
        assert!(!msg.contains('\''), "straight marks crept back in: {msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// A newline in a directory name must not be able to add a line that looks
    /// like a second diagnostic from `mkdir`.
    ///
    /// The name is put under a parent that does not exist, so the creation fails
    /// on its own without the test having to *make* a file with this name first
    /// — which it could not do on the development host, where `:` and `/` are
    /// not legal in a Windows filename. The failing path is the one under test
    /// anyway: it is the only one that prints a name.
    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        let d = scratch("forge");
        let evil = d
            .join("nosuchparent")
            .join("a\nmkdir: /etc: Permission denied");
        let (ok, msg) = run(false, &[&evil]);
        assert!(!ok);
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.contains(r"\n"), "the newline must be escaped: {msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// One failure must not abandon the rest — measured: `mkdir a g` with `a`
    /// present still creates `g` and exits 1.
    #[test]
    fn one_failure_does_not_abandon_the_others() {
        let d = scratch("partial");
        let a = d.join("a");
        fs::create_dir(&a).unwrap();
        let g = d.join("g");
        let (ok, msg) = run(false, &[&a, &g]);
        assert!(!ok, "the existing one must count against the status");
        assert!(g.is_dir(), "{msg}");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        let _ = fs::remove_dir_all(&d);
    }
}
