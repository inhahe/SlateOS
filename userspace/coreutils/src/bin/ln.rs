//! ln — create links between files.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte that
//! is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `ln` is the
//! fourth of the bins listed there, after `rm`, `mv` and `cp`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Four further defects, in the lines this rewrite replaced
//!
//! Unlike `rm`, `mv` and `cp`, none of these produced a *silently* wrong
//! result — every one of them was a refusal or a mangled message rather than a
//! link pointing somewhere the user did not ask for. That is worth stating
//! plainly rather than dressing up: `ln` is the first of the four where the
//! argv defect was not also a marker for something that loses data.
//!
//! 1. **No long option worked at all, including `--help`.** The parser treated
//!    any argument beginning with `-` as a bundle of short options and iterated
//!    its characters, so `--help` tripped on its own second `-` and answered
//!    `unknown option: --` — naming an option nobody typed and giving no hint
//!    that `--help`, `--version` and `--symbolic` were simply unreachable. The
//!    same line meant `--` was not an end-of-options marker, so a file whose
//!    name begins with a dash could not be linked either.
//!
//! 2. **Filenames went into diagnostics unquoted.** The failure message
//!    interpolated the two paths between literal `'` marks, so a file called
//!    `a⏎ln: /etc/shadow: Permission denied` made `ln` appear to print a second
//!    line it never wrote. Every name now goes through [`coreutils::quote`],
//!    which is the same fix, for the same reason, as the tree-wide sweep that
//!    put file names in diagnostics through it.
//!
//! 3. **Three of GNU's four operand forms were refused.** Only
//!    `ln TARGET LINK_NAME` worked; `ln TARGET` (link in the current directory)
//!    and `ln TARGET... DIRECTORY` both answered `expected exactly two
//!    arguments`. The third form is not exotic — `ln -s ../lib/libfoo.so .` is
//!    it — and a user who typed it got a message implying they had made a
//!    mistake. The first three forms are implemented now; the fourth (`-t
//!    DIRECTORY`) needs `-t`, which this `ln` does not have and refuses by name.
//!
//! 4. **A relative symlink target was misjudged on Windows.** The `#[cfg(windows)]`
//!    arm must decide at creation time whether to make a file link or a
//!    directory link, and asked `Path::new(target).is_dir()` — which resolves
//!    `target` against the *current* directory. A symlink's text is resolved
//!    against the *link's own* directory, so `ln -s ../thing sub/link` asked the
//!    wrong question and could make a file link to a directory. This affects the
//!    development host only — the shipping target is `unix` and takes the branch
//!    above it — but it is a bug in code that exists, and the fix is one `join`.
//!
//! # Options this implementation does not have
//!
//! Everything except `-s`/`--symbolic`. They are recognised and rejected with a
//! message saying they are not implemented, rather than ignored, and they are
//! listed in [`LONG_OPTIONS`] anyway because the table is what decides whether
//! an abbreviation is ambiguous — `--s` must stay ambiguous between `--suffix`
//! and `--symbolic`, which is measured GNU behaviour and is exactly what a table
//! pruned to the one implemented option would get wrong.
//!
//! Ignoring them would be worse than refusing. `-f` asks for an existing
//! destination to be *removed*; ignored, the link is not created and the user is
//! told the destination exists, which is at least loud. `-n` is the dangerous
//! one: it asks for a `LINK_NAME` that is itself a symlink to a directory to be
//! treated as a plain file, so ignoring it puts the new link *inside* the
//! pointed-at directory instead of replacing the link — a link in a place the
//! user never named, with no message.

use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quoteaf_os;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

/// `ln`'s usage status is 1 — measured: `ln --zzz; echo $?` prints 1. See
/// [`coreutils::getopt::Error`] for the handful of utilities that differ.
const LN: Program = Program::new("ln", 1);

/// GNU `ln`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Measured with the instrument described in
/// [`Program::resolve_long`] — an empty prefix matches everything, so
/// `ln --=x` prints the whole table:
///
/// ```text
/// ln: option '--=x' is ambiguous; possibilities: '--backup' '--directory'
/// '--no-dereference' '--no-target-directory' '--force' '--interactive'
/// '--suffix' '--target-directory' '--logical' '--physical' '--relative'
/// '--symbolic' '--verbose' '--help' '--version'
/// ```
///
/// Note that it is neither alphabetical nor grouped by short-option letter.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("backup", Takes::Optional),
    ("directory", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-target-directory", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("suffix", Takes::Required),
    ("target-directory", Takes::Required),
    ("logical", Takes::Nothing),
    ("physical", Takes::Nothing),
    ("relative", Takes::Nothing),
    ("symbolic", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct LnFlags {
    symbolic: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order.
    Run(LnFlags, Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("ln (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, paths)) => {
            let mut err = io::stderr().lock();
            if link_all(&flags, &paths, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("ln: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: ln [OPTION]... TARGET LINK_NAME
  or:  ln [OPTION]... TARGET
  or:  ln [OPTION]... TARGET... DIRECTORY
In the 1st form, create a link to TARGET with the name LINK_NAME.
In the 2nd form, create a link to TARGET in the current directory.
In the 3rd form, create links to each TARGET in DIRECTORY.
Create hard links by default, symbolic links with --symbolic.
By default, each destination (name of new link) should not already exist.
When creating hard links, each TARGET must exist.  Symbolic links
can hold arbitrary text; if later resolved, a relative link is
interpreted in relation to its parent directory.

  -s, --symbolic   make symbolic links instead of hard links
      --help       display this help and exit
      --version    output version information and exit

To link a file whose name starts with a '-', for example '-foo',
use one of these commands:
  ln -- -foo bar
  ln ./-foo bar
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `ln`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `ln a -s b` is `ln -s a b` — which
/// is `getopt_long`'s default permuting behaviour and what the previous
/// hand-written parser did too.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, or
/// a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = LnFlags::default();
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
            // A lone `-` is a file called `-`. `ln` has no standard-input
            // operand for it to mean anything else.
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
            // bytes. It also would not survive an argument that is not UTF-8 at
            // all, which is the whole point of this rewrite.
            for &b in bytes.get(1..).unwrap_or_default() {
                apply_short(b, &mut flags)?;
            }
        }
    }

    Ok(Request::Run(flags, paths))
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
    flags: &mut LnFlags,
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
    let typed = std::str::from_utf8(typed).map_err(|_| LN.unrecognized_option(whole))?;
    let (name, takes) = LN.resolve_long(typed, whole, LONG_OPTIONS)?;

    if inline.is_some() && takes == Takes::Nothing {
        return Err(LN.long_unwanted_argument(name));
    }

    match name {
        "help" => Ok(Some(Request::Help)),
        "version" => Ok(Some(Request::Version)),
        "symbolic" => {
            flags.symbolic = true;
            Ok(None)
        }
        other => Err(unimplemented_long(other)),
    }
}

/// Handle one short option byte.
///
/// # Errors
///
/// A byte that is no option of `ln`'s, or one this implementation lacks.
fn apply_short(flag: u8, flags: &mut LnFlags) -> Result<(), getopt::Error> {
    match flag {
        b's' => flags.symbolic = true,
        // GNU `ln`'s remaining short options, from its `getopt_long` string.
        // Rejected rather than ignored: see the module docs — `-n` ignored puts
        // the new link somewhere the user never named.
        b'b' | b'd' | b'f' | b'i' | b'n' | b'r' | b't' | b'v' | b'F' | b'L' | b'P' | b'S'
        | b'T' => return Err(unimplemented_short(flag)),
        other => return Err(LN.invalid_option(other)),
    }
    Ok(())
}

/// The diagnostic for an option that GNU `ln` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-r` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    LN.usage_referring(format!(
        "option -{} is not implemented by this ln",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    LN.usage_referring(format!("option '--{name}' is not implemented by this ln"))
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

// ---------------------------------------------------------------- linking ---

/// Create every link the command line asked for, reporting failures to `err`.
///
/// Returns `true` if everything asked for was created. Takes the error sink as a
/// parameter rather than writing to `stderr` directly so the diagnostics — the
/// part of `ln` a caller actually sees when something goes wrong — can be
/// asserted on in tests. The old file had no test of this path at all.
///
/// The three operand forms are GNU's, measured rather than recalled:
///
/// | Command line | Link created |
/// |---|---|
/// | `ln TARGET` | `./<basename of TARGET>` |
/// | `ln TARGET LINK` | `LINK`, or `LINK/<basename>` if `LINK` is a directory |
/// | `ln TARGET... DIR` | `DIR/<basename>` for each, and `DIR` must be one |
fn link_all<W: Write>(flags: &LnFlags, paths: &[OsString], err: &mut W) -> bool {
    match paths {
        [] => {
            let _ = writeln!(
                err,
                "ln: {}",
                LN.usage_referring("missing file operand".into())
            );
            false
        }
        // Form 2: one operand, link goes in the current directory. GNU spells
        // the result `./name` rather than `name`, which is visible in its
        // diagnostics (`ln: failed to create hard link './a'`), so it is spelled
        // that way here too.
        [only] => link_into(flags, only, Path::new("."), err),
        // Form 1: exactly two operands and the second is not a directory.
        [target, link] if !Path::new(link).is_dir() => {
            link_one(flags, target, Path::new(link), err)
        }
        // Form 3 (and the two-operand case whose destination *is* a directory).
        [targets @ .., dir] => {
            let dir_path = Path::new(dir);
            // Only reachable with three or more operands: the two-operand arm
            // above has already handled a non-directory destination, and one
            // operand never reaches here.
            match fs::metadata(dir_path) {
                Ok(m) if m.is_dir() => {}
                Ok(_) => {
                    let _ = writeln!(err, "ln: target {}: Not a directory", quoteaf_os(dir));
                    return false;
                }
                Err(e) => {
                    let _ = writeln!(err, "ln: target {}: {e}", quoteaf_os(dir));
                    return false;
                }
            }
            let mut ok = true;
            for target in targets {
                if !link_into(flags, target, dir_path, err) {
                    ok = false;
                }
            }
            ok
        }
    }
}

/// Where a link to `target` lands inside `dir`, or `None` if `target` has no
/// file-name component to take a name from.
///
/// The one-operand form passes `.` here, which is why GNU's diagnostic for it
/// reads `'./a'` rather than `'a'`: `Path::join` keeps the `.`, exactly as
/// gnulib's does.
fn link_in_dir(target: &Path, dir: &Path) -> Option<std::path::PathBuf> {
    target.file_name().map(|name| dir.join(name))
}

/// Create a link to `target` inside the directory `dir`, named after `target`.
fn link_into<W: Write>(flags: &LnFlags, target: &OsString, dir: &Path, err: &mut W) -> bool {
    let Some(at) = link_in_dir(Path::new(target), dir) else {
        // GNU reaches this case with gnulib's `base_name`, which answers `..`
        // for `a/..` and `/` for `/`, and so ends up asking to create `dir/..`
        // — which always exists, so GNU fails with `File exists`. Rust's
        // `Path::file_name` answers `None` instead, and refusing outright says
        // the same thing more usefully. Both refuse; only the wording differs.
        let _ = writeln!(
            err,
            "ln: cannot link {} into {}: the target path ends in '.', '..' or '/', \
             so it names nothing to create there",
            quoteaf_os(target),
            quoteaf_os(dir)
        );
        return false;
    };
    link_one(flags, target, &at, err)
}

/// Create one link at exactly `link`.
fn link_one<W: Write>(flags: &LnFlags, target: &OsString, link: &Path, err: &mut W) -> bool {
    let target_path = Path::new(target);

    if !flags.symbolic {
        // A hard link needs its target to exist; a symbolic one does not, and
        // `ln -s /nowhere lnk` making a dangling link is deliberate GNU
        // behaviour, not an oversight. Measured: it exits 0.
        //
        // `symlink_metadata`, not `metadata`: GNU's default is `-P`, so a hard
        // link to a symlink links the symlink itself, and `ln dangling other`
        // succeeds. Asking `metadata` would follow the link and refuse a
        // command GNU accepts.
        if let Err(e) = fs::symlink_metadata(target_path) {
            let _ = writeln!(err, "ln: failed to access {}: {e}", quoteaf_os(target));
            return false;
        }
    }

    let outcome = if flags.symbolic {
        symlink(target_path, link)
    } else {
        fs::hard_link(target_path, link)
    };

    if let Err(e) = outcome {
        let kind = if flags.symbolic { "symbolic" } else { "hard" };
        // Names go through `quoteaf_os`, not between two literal `'` — module
        // docs, defect 2. GNU names only the link here, not the target.
        let _ = writeln!(
            err,
            "ln: failed to create {kind} link {}: {e}",
            quoteaf_os(link)
        );
        return false;
    }
    true
}

/// Create a symbolic link whose text is exactly `target`.
///
/// `target` is written into the link verbatim and is never resolved, which is
/// what makes `ln -s ../lib/libfoo.so .` mean the same thing after the directory
/// is moved. It is a `&Path` rather than a `&str` because it may hold bytes that
/// are not valid UTF-8 — that is the defect this file was rewritten for.
#[cfg(unix)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Windows needs to know at creation time whether the link points at a
/// directory, and there is no way to defer it.
///
/// The question must be asked about the path *the link will resolve to*, not
/// about `target` as written: a symlink's text is interpreted relative to the
/// link's own directory, so `ln -s ../thing sub/link` means `sub/../thing`, not
/// `../thing` from wherever the shell happens to be. Module docs, defect 4.
/// An absolute `target` is unaffected — `join` discards the base for one.
#[cfg(windows)]
fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    let base = link.parent().unwrap_or_else(|| Path::new(""));
    if base.join(target).is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[cfg(not(any(unix, windows)))]
fn symlink(_target: &Path, _link: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "symbolic links are not supported on this platform",
    ))
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
    fn run_parse(items: &[&str]) -> (LnFlags, Vec<String>) {
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
        assert!(!f.symbolic);
        assert!(p.is_empty());
    }

    #[test]
    fn hard_link_two_operands() {
        let (f, p) = run_parse(&["a.txt", "b.txt"]);
        assert!(!f.symbolic);
        assert_eq!(p, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn symbolic_flag() {
        assert!(run_parse(&["-s", "a", "b"]).0.symbolic);
        assert!(run_parse(&["--symbolic", "a", "b"]).0.symbolic);
        assert!(run_parse(&["--symb", "a", "b"]).0.symbolic);
    }

    #[test]
    fn flag_may_follow_operands() {
        let (f, p) = run_parse(&["a", "b", "-s"]);
        assert!(f.symbolic);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn repeating_the_flag_is_idempotent() {
        assert!(run_parse(&["-s", "-s", "a", "b"]).0.symbolic);
        assert!(run_parse(&["-ss", "a", "b"]).0.symbolic);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-", "dest"]).1, vec!["-", "dest"]);
    }

    /// Defect 1 in the module docs: this used to answer `unknown option: --`.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]).1, vec!["-foo", "bar"]);
        let (f, p) = run_parse(&["--", "-s"]);
        assert!(!f.symbolic, "-s after -- is a filename, not a flag");
        assert_eq!(p, vec!["-s"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).1.is_empty());
    }

    /// Also defect 1: `--help` reached the short-option loop and tripped on its
    /// own second dash, so no long option was reachable at all.
    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    /// `--s` must stay ambiguous between `--suffix` and `--symbolic`. This is
    /// the test that fails if someone prunes the table down to the one option
    /// this implementation actually acts on — which would silently turn `ln --s`
    /// from an error into a symbolic link.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--s"]);
        assert_eq!(
            e.sentence,
            "option '--s' is ambiguous; possibilities: '--suffix' '--symbolic'"
        );
    }

    /// The other two ambiguities GNU has, in GNU's table order. `--v` is the
    /// one that matters most: without `--verbose` in the table it would resolve
    /// to `--version` and print a version instead of refusing.
    #[test]
    fn the_other_ambiguities_keep_their_measured_order() {
        assert_eq!(
            fail(&["--n"]).sentence,
            "option '--n' is ambiguous; possibilities: '--no-dereference' '--no-target-directory'"
        );
        assert_eq!(
            fail(&["--v"]).sentence,
            "option '--v' is ambiguous; possibilities: '--verbose' '--version'"
        );
    }

    #[test]
    fn unambiguous_abbreviations_still_resolve() {
        // `--f` prefixes only `--force`, so it resolves — and is then refused
        // as unimplemented rather than as unrecognised.
        assert!(fail(&["--f"]).sentence.contains("not implemented"));
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "a", "b"]);
        assert_eq!(e.sentence, "invalid option -- 'q'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz=1", "a", "b"]);
        assert_eq!(e.sentence, "unrecognized option '--zzz=1'");
        assert_eq!(e.status, 1);
    }

    /// `-n` ignored would put the link inside a directory the user never named;
    /// `-f` ignored would leave a destination the user asked to have replaced.
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in [
            "-b", "-d", "-f", "-i", "-n", "-r", "-t", "-v", "-F", "-L", "-P", "-S", "-T",
        ] {
            let e = fail(&[flag, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{flag}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        for name in [
            "--backup",
            "--directory",
            "--no-dereference",
            "--no-target-directory",
            "--force",
            "--interactive",
            "--suffix",
            "--target-directory",
            "--logical",
            "--physical",
            "--relative",
            "--verbose",
        ] {
            let e = fail(&[name, "a", "b"]);
            assert!(
                e.sentence.contains("not implemented"),
                "{name}: {:?}",
                e.sentence
            );
        }
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        let e = fail(&["--symbolic=yes", "a", "b"]);
        assert_eq!(e.sentence, "option '--symbolic' doesn't allow an argument");
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// The regression test for the reason this file was rewritten. On this OS a
    /// filename may hold any byte but `/` and NUL, and byte `0x80` alone is not
    /// valid UTF-8, so an operand containing it cannot be a `String` at all.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-s"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.symbolic);
                assert_eq!(p, vec![bad, OsString::from("d")]);
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
        match parse_args(&[OsString::from("-s"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.symbolic);
                assert_eq!(p, vec![bad, OsString::from("d")]);
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

    // ------------------------------------------------------------ linking --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("ln_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Run `link_all` with hard links, returning `(ok, diagnostics)`.
    fn hard(paths: &[&Path]) -> (bool, String) {
        run(&LnFlags { symbolic: false }, paths)
    }

    fn run(flags: &LnFlags, paths: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = link_all(flags, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, msg) = hard(&[]);
        assert!(!ok);
        assert!(msg.contains("missing file operand"), "{msg}");
        assert!(msg.contains("Try 'ln --help'"), "{msg}");
    }

    #[test]
    fn two_operands_make_a_hard_link() {
        let d = scratch("hard2");
        let a = d.join("a");
        fs::write(&a, b"contents").unwrap();
        let b = d.join("b");
        let (ok, msg) = hard(&[&a, &b]);
        assert!(ok, "{msg}");
        assert_eq!(fs::read(&b).unwrap(), b"contents");
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 3, form 2: `ln a` used to answer `expected exactly two
    /// arguments`. The link lands in the current directory, and GNU spells the
    /// result `./a` — visible in its own diagnostic, `ln: failed to create hard
    /// link './a': File exists`. This asserts the spelling without touching the
    /// process-wide current directory, which other tests in this binary run
    /// concurrently with.
    #[test]
    fn one_operand_names_the_link_under_dot() {
        assert_eq!(
            link_in_dir(Path::new("src/a"), Path::new(".")).unwrap(),
            PathBuf::from(".").join("a")
        );
        assert_eq!(
            link_in_dir(Path::new("a"), Path::new("dst")).unwrap(),
            PathBuf::from("dst").join("a")
        );
        // A trailing slash still has a basename — GNU's `base_name` agrees.
        assert_eq!(
            link_in_dir(Path::new("src/a/"), Path::new("dst")).unwrap(),
            PathBuf::from("dst").join("a")
        );
        for none in ["..", "a/..", "."] {
            assert!(
                link_in_dir(Path::new(none), Path::new("dst")).is_none(),
                "{none}"
            );
        }
    }

    /// The one-operand arm is reached at all — it used to be a usage error.
    #[test]
    fn one_operand_is_not_a_usage_error() {
        let d = scratch("form2");
        let (ok, msg) = hard(&[&d.join("nope")]);
        assert!(!ok);
        assert!(
            msg.contains("failed to access"),
            "it must get as far as trying: {msg}"
        );
        assert!(!msg.contains("missing file operand"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 3, form 1's directory case: `ln a dir` makes `dir/a`, not a link
    /// literally named `dir`.
    #[test]
    fn two_operands_whose_destination_is_a_directory_link_inside_it() {
        let d = scratch("intodir");
        let a = d.join("a");
        fs::write(&a, b"x").unwrap();
        let sub = d.join("sub");
        fs::create_dir(&sub).unwrap();
        let (ok, msg) = hard(&[&a, &sub]);
        assert!(ok, "{msg}");
        assert_eq!(fs::read(sub.join("a")).unwrap(), b"x");
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 3, form 3. `ln -s ../lib/libfoo.so .` is this form, and it used
    /// to be refused outright.
    #[test]
    fn many_operands_link_each_into_the_directory() {
        let d = scratch("form3");
        for name in ["a", "b", "c"] {
            fs::write(d.join(name), name.as_bytes()).unwrap();
        }
        let sub = d.join("sub");
        fs::create_dir(&sub).unwrap();
        let (ok, msg) = hard(&[&d.join("a"), &d.join("b"), &d.join("c"), &sub]);
        assert!(ok, "{msg}");
        for name in ["a", "b", "c"] {
            assert_eq!(fs::read(sub.join(name)).unwrap(), name.as_bytes());
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured GNU wording: `ln: target 'f': Not a directory`.
    #[test]
    fn many_operands_need_a_directory_last() {
        let d = scratch("notadir");
        for name in ["a", "b", "f"] {
            fs::write(d.join(name), b"x").unwrap();
        }
        let (ok, msg) = hard(&[&d.join("a"), &d.join("b"), &d.join("f")]);
        assert!(!ok);
        assert!(msg.contains("Not a directory"), "{msg}");
        assert!(msg.contains("target "), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn many_operands_report_a_missing_directory_by_name() {
        let d = scratch("nodir");
        fs::write(d.join("a"), b"x").unwrap();
        fs::write(d.join("b"), b"x").unwrap();
        let (ok, msg) = hard(&[&d.join("a"), &d.join("b"), &d.join("nope")]);
        assert!(!ok);
        assert!(msg.contains("target "), "{msg}");
        assert!(msg.contains("nope"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// A hard link needs its target to exist, and GNU says so before it tries.
    #[test]
    fn a_missing_hard_link_target_is_reported_as_access() {
        let d = scratch("noaccess");
        let (ok, msg) = hard(&[&d.join("nope"), &d.join("dst")]);
        assert!(!ok);
        assert!(msg.contains("failed to access"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_existing_destination_is_reported_as_a_creation_failure() {
        let d = scratch("exists");
        fs::write(d.join("a"), b"x").unwrap();
        fs::write(d.join("b"), b"y").unwrap();
        let (ok, msg) = hard(&[&d.join("a"), &d.join("b")]);
        assert!(!ok);
        assert!(msg.contains("failed to create hard link"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// One failure must not abandon the rest — the same rule the `cp` rewrite
    /// established, and the reason `link_all` accumulates instead of returning
    /// on the first error.
    #[test]
    fn one_failure_does_not_abandon_the_others() {
        let d = scratch("partial");
        fs::write(d.join("a"), b"a").unwrap();
        fs::write(d.join("c"), b"c").unwrap();
        let sub = d.join("sub");
        fs::create_dir(&sub).unwrap();
        let (ok, msg) = hard(&[&d.join("a"), &d.join("missing"), &d.join("c"), &sub]);
        assert!(!ok, "the missing one must count against the status");
        assert!(sub.join("a").exists(), "{msg}");
        assert!(sub.join("c").exists(), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// Defect 2. A newline in a filename must not be able to add a line that
    /// looks like a second diagnostic from `ln`.
    #[test]
    fn a_filename_cannot_forge_a_second_diagnostic_line() {
        let d = scratch("forge");
        let evil = d.join("a\nln: /etc/shadow: Permission denied");
        let (ok, msg) = hard(&[&evil, &d.join("dst")]);
        assert!(!ok);
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.contains(r"\n"), "the newline must be escaped: {msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// A target with no basename cannot be given a name inside a directory.
    /// GNU asks for `dir/..`, which always exists, and fails with `File
    /// exists`; refusing outright is the same refusal with a better sentence.
    #[test]
    fn a_target_with_no_file_name_is_refused() {
        let d = scratch("nobasename");
        let sub = d.join("sub");
        fs::create_dir(&sub).unwrap();
        for src in ["..", "a/.."] {
            let mut err: Vec<u8> = Vec::new();
            let ok = link_into(
                &LnFlags { symbolic: false },
                &OsString::from(src),
                &sub,
                &mut err,
            );
            assert!(!ok, "{src}");
            let msg = String::from_utf8_lossy(&err);
            assert!(msg.contains("names nothing"), "{src}: {msg}");
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// A symbolic link's text is stored verbatim and is not resolved, so a
    /// dangling one is created without complaint. Measured: GNU exits 0.
    #[test]
    #[cfg(unix)]
    fn a_symbolic_link_may_dangle() {
        let d = scratch("dangle");
        let link = d.join("lnk");
        let (ok, msg) = run(
            &LnFlags { symbolic: true },
            &[Path::new("/no/such/place"), &link],
        );
        assert!(ok, "{msg}");
        assert_eq!(
            fs::read_link(&link).unwrap(),
            PathBuf::from("/no/such/place")
        );
        assert!(fs::symlink_metadata(&link).is_ok());
        assert!(fs::metadata(&link).is_err(), "it must actually dangle");
        let _ = fs::remove_dir_all(&d);
    }

    /// The link holds exactly what was typed, not a resolved path — which is
    /// what makes a relative link keep meaning the same thing after its
    /// directory is moved.
    #[test]
    #[cfg(unix)]
    fn a_symbolic_links_text_is_stored_verbatim() {
        let d = scratch("verbatim");
        fs::write(d.join("real"), b"x").unwrap();
        let link = d.join("lnk");
        let (ok, msg) = run(&LnFlags { symbolic: true }, &[Path::new("real"), &link]);
        assert!(ok, "{msg}");
        assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from("real"));
        assert_eq!(fs::read(&link).unwrap(), b"x");
        let _ = fs::remove_dir_all(&d);
    }

    /// GNU's default is `-P`: a hard link to a symlink links the *symlink*, so
    /// it succeeds even when the symlink dangles. Asking `metadata` instead of
    /// `symlink_metadata` in the access check would refuse this.
    #[test]
    #[cfg(unix)]
    fn a_hard_link_to_a_dangling_symlink_succeeds() {
        let d = scratch("hardtolink");
        let dangle = d.join("dangle");
        std::os::unix::fs::symlink("/no/such/place", &dangle).unwrap();
        let (ok, msg) = hard(&[&dangle, &d.join("other")]);
        assert!(ok, "{msg}");
        let _ = fs::remove_dir_all(&d);
    }
}
