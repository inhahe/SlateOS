//! mv — move (rename) files.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte that
//! is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `mv` is the
//! second of the 49 bins listed there, after `rm`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Four further bugs, in the lines this rewrite replaced
//!
//! 1. **`--` was not an end-of-options marker.** `mv -- -foo bar` answered
//!    `unknown option: --`. `--` is the only portable way to name a source file
//!    whose name begins with a dash, so such a file could not be moved at all.
//!
//! 2. **`-f` suppressed the diagnostic but not the failure.** The old `-f`
//!    branch skipped the `eprintln!` and still set the exit status to 1, so
//!    `mv -f a b` on a failure printed *nothing* and exited non-zero: the
//!    caller was told something went wrong and given no way to find out what.
//!    That is not what `-f` means anywhere. In GNU `mv`, `-f` suppresses the
//!    *prompt* that `-i` would otherwise raise before overwriting; it has never
//!    suppressed errors. This `mv` never prompts, so `-f` is now accepted and
//!    does nothing at all — which is exactly GNU's behaviour in the absence of
//!    `-i`, and is why it records no flag.
//!
//! 3. **A source ending in `..` moved something the user never named.**
//!    `compute_target` did `dest.join(src.file_name().unwrap_or_default())`, and
//!    `Path::file_name` is `None` for a path whose last component is `..` — so
//!    `unwrap_or_default()` produced an *empty* name, `dest.join("")` collapsed
//!    back to `dest` itself, and `mv a/.. dst` asked the kernel to rename `a`'s
//!    **parent directory** to `dst`. If `dst` was an empty directory that
//!    succeeds: the user asks to move something into `dst` and instead the
//!    directory they were standing in is moved *onto* `dst`. Reachable from an
//!    ordinary glob (`mv */.. dst`). A source with no file-name component is now
//!    refused with a diagnostic.
//!
//! 4. **The cross-filesystem fallback silently turned a symlink into a copy of
//!    its target.** When `rename` fails with `EXDEV`, `mv` must copy and then
//!    unlink. The old fallback used `fs::copy`, which *follows* symlinks — so
//!    moving a symlink across a filesystem boundary read the file it pointed at,
//!    wrote those bytes at the destination as an ordinary file, and deleted the
//!    link. A symlink went in and a full copy came out, with no message. The
//!    link is now recreated with `symlink(2)` and only then unlinked. (A
//!    *dangling* symlink hit the same path and failed with `No such file or
//!    directory`, naming the link — which reads as "the link is missing" when
//!    the link was right there.)
//!
//!    The fallback is also no longer entered for *every* rename failure, only
//!    for a genuine cross-device one. Previously a `mv nonexistent dst` failed
//!    `rename`, fell through to `fs::copy`, and reported the *copy's* error,
//!    which happened to read the same but need not have.
//!
//! # Options this implementation does not have
//!
//! `-b`/`--backup`, `-i`/`--interactive`, `-n`/`--no-clobber`,
//! `-t`/`--target-directory`, `-T`/`--no-target-directory`, `-u`/`--update`,
//! `-v`/`--verbose`, `-S`/`--suffix`, `-Z`/`--context`, `--debug`,
//! `--exchange` and `--strip-trailing-slashes` are recognised and rejected with
//! a message saying they are not implemented, rather than ignored. Silently
//! ignoring `-n` would overwrite a file the user asked to be left alone, and
//! ignoring `-i` would skip a confirmation they asked for; for this utility
//! both mistakes are unrecoverable, and an error costs only a retype.
//!
//! They are all listed in [`LONG_OPTIONS`] anyway, because the table is what
//! decides whether an abbreviation is ambiguous — drop `--verbose` and `mv --v`
//! resolves to `--version` and prints a banner instead of failing.
//!
//! Moving a **directory across a filesystem boundary** is also not implemented:
//! it needs a recursive copy that preserves modes, symlinks and hard links, and
//! doing it wrong loses data quietly. It reports that it is not implemented
//! rather than attempting a partial job. Logged in `known-issues.md`.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quoteaf_os;
use coreutils::stdfd::Stream;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `mv`'s usage status is 1, like almost every utility's; see
/// [`coreutils::getopt::Error`] for the two that differ and why.
const MV: Program = Program::new("mv", 1);

/// GNU `mv`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Every entry is here whether or not this implementation acts on it —
/// see the module docs for why leaving one out is a silent wrong answer rather
/// than a missing feature.
///
/// Measured with `mv --=x`, which an empty prefix makes print the whole table:
///
/// ```text
/// mv: option '--=x' is ambiguous; possibilities: '--backup' '--context'
/// '--debug' '--force' '--interactive' '--no-clobber' '--no-copy'
/// '--no-target-directory' '--strip-trailing-slashes' '--suffix'
/// '--target-directory' '--update' '--verbose' '--help' '--version'
/// ```
///
/// **This table was originally written from recall and was wrong in both
/// directions**, which is the reason `scripts/getopt-ambiguity-check.py` now
/// exists — it found this by asking GNU about every prefix. It carried an
/// `("exchange", …)` that the reference does not have (it is a later upstream
/// addition) and lacked `("no-copy", …)` that it does, so `mv --no-c` resolved
/// to `--no-clobber` here where GNU calls it ambiguous. Nothing user-visible
/// went wrong only because this `mv` refuses `--no-clobber` anyway; the day it
/// implements it, `mv --no-c` would have silently meant `--no-clobber`.
///
/// The rule the mistake teaches: **the table tracks the reference we can
/// measure, not the newest upstream we can remember.** A table half from one
/// release and half from another matches no getopt anywhere.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("backup", Takes::Optional),
    ("context", Takes::Optional),
    ("debug", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("no-clobber", Takes::Nothing),
    ("no-copy", Takes::Nothing),
    ("no-target-directory", Takes::Nothing),
    ("strip-trailing-slashes", Takes::Nothing),
    ("suffix", Takes::Required),
    ("target-directory", Takes::Required),
    ("update", Takes::Optional),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
///
/// There is no flags struct: the only option this `mv` implements is `-f`, and
/// `-f` only suppresses a prompt that this `mv` never raises. Recording a field
/// nothing reads would suggest it changes something. See module docs, bug 2.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// Every operand, in order. The last is the destination.
    Run(Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("mv (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(paths)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            if move_all(&paths, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("mv: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: mv [OPTION]... SOURCE DEST
  or:  mv [OPTION]... SOURCE... DIRECTORY
Rename SOURCE to DEST, or move SOURCE(s) to DIRECTORY.

  -f, --force   do not prompt before overwriting (accepted; this mv never
                  prompts, so it has no effect)
      --help    display this help and exit
      --version output version information and exit

To move a file whose name starts with a '-', for example '-foo',
use one of these commands:
  mv -- -foo bar
  mv ./-foo bar
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `mv`'s argv into its operands.
///
/// Options and operands may be interleaved — `mv a -f b` is `mv a b` — which is
/// `getopt_long`'s default permuting behaviour and what the previous
/// hand-written parser did too.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, or
/// a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
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
            // A lone `-` is a file called `-`. `mv` has no standard-input
            // operand for it to mean anything else.
            paths.push(arg.clone());
        } else if let Some(body) = bytes.strip_prefix(b"--") {
            match parse_long(body, &bytes)? {
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
                apply_short(b)?;
            }
        }
    }

    Ok(Request::Run(paths))
}

/// Handle one `--name[=value]` argument.
///
/// Returns `Some(request)` for the two options that end parsing immediately, and
/// `None` for one that does not.
///
/// # Errors
///
/// The name resolving to nothing or to more than one option, a value given to an
/// option that takes none, or an option this implementation lacks.
fn parse_long(body: &[u8], whole: &[u8]) -> Result<Option<Request>, getopt::Error> {
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
    let typed = std::str::from_utf8(typed).map_err(|_| MV.unrecognized_option(whole))?;
    let (name, takes) = MV.resolve_long(typed, whole, LONG_OPTIONS)?;

    if inline.is_some() && takes == Takes::Nothing {
        return Err(MV.long_unwanted_argument(name));
    }

    match name {
        "help" => Ok(Some(Request::Help)),
        "version" => Ok(Some(Request::Version)),
        // Accepted and deliberately inert; see module docs, bug 2.
        "force" => Ok(None),
        other => Err(unimplemented_long(other)),
    }
}

/// Handle one short option byte.
///
/// # Errors
///
/// A byte that is no option of `mv`'s, or one this implementation lacks.
fn apply_short(flag: u8) -> Result<(), getopt::Error> {
    match flag {
        // Accepted and deliberately inert; see module docs, bug 2.
        b'f' => Ok(()),
        // GNU `mv`'s remaining short options.
        b'b' | b'i' | b'n' | b't' | b'T' | b'u' | b'v' | b'S' | b'Z' => {
            Err(unimplemented_short(flag))
        }
        other => Err(MV.invalid_option(other)),
    }
}

/// The diagnostic for an option that GNU `mv` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-n` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    MV.usage_referring(format!(
        "option -{} is not implemented by this mv",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    MV.usage_referring(format!("option '--{name}' is not implemented by this mv"))
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

// ----------------------------------------------------------------- moving ---

/// Move every source onto the destination, reporting failures to `err`.
///
/// Returns `true` if every source was moved. Takes the error sink as a parameter
/// rather than writing to `stderr` directly so the diagnostics — the part of
/// `mv` a caller actually sees when something goes wrong — can be asserted on in
/// tests. The old file had no test of this path at all, which is how bugs 2–4 in
/// the module docs survived.
///
/// A failure on one source does not stop the others: `mv a b c dir/` with `b`
/// unmovable still moves `a` and `c`, and exits 1.
fn move_all<W: Write>(paths: &[OsString], err: &mut W) -> bool {
    // Zero and one operand are distinct diagnostics, as in GNU. "missing
    // operand" alone left the user to work out *which*.
    let Some((dest, sources)) = paths.split_last() else {
        let _ = writeln!(
            err,
            "mv: {}",
            MV.usage_referring("missing file operand".into())
        );
        return false;
    };
    if sources.is_empty() {
        let _ = writeln!(
            err,
            "mv: {}",
            MV.usage_referring(format!(
                "missing destination file operand after {}",
                quoteaf_os(dest)
            ))
        );
        return false;
    }

    let dest_path = Path::new(dest);
    // `is_dir` follows symlinks, and that is correct here: `mv a link-to-dir/`
    // puts `a` inside the directory, which is what GNU does without `-T`.
    let dest_is_dir = dest_path.is_dir();

    if sources.len() > 1 && !dest_is_dir {
        let _ = writeln!(err, "mv: target {} is not a directory", quoteaf_os(dest));
        return false;
    }

    let mut ok = true;
    for src in sources {
        if !move_one(src, dest_path, dest_is_dir, err) {
            ok = false;
        }
    }
    ok
}

/// Move one source. Returns `false` if it should count against the exit status.
fn move_one<W: Write>(src: &OsString, dest: &Path, dest_is_dir: bool, err: &mut W) -> bool {
    let src_path = Path::new(src);

    // `symlink_metadata`, not `exists`/`is_dir`: both follow symlinks, and `mv`
    // moves a symlink as itself, whatever it points at — including nothing.
    // Statting first also means a missing source is reported here, by name,
    // instead of surfacing later as whatever the fallback copy happened to say.
    let metadata = match fs::symlink_metadata(src_path) {
        Ok(m) => m,
        Err(e) => {
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`] — on a Windows *host*
            // `{e}` says `The system cannot find the file specified. (os error
            // 2)`, which is neither POSIX's wording nor what this utility prints
            // on the target it ships on.
            let why = strerror(&e);
            let _ = writeln!(err, "mv: cannot stat {}: {why}", quoteaf_os(src));
            return false;
        }
    };

    let target = match compute_target(src_path, dest, dest_is_dir) {
        Ok(t) => t,
        Err(reason) => {
            let _ = writeln!(
                err,
                "mv: cannot move {} into {}: {reason}",
                quoteaf_os(src),
                quoteaf_os(dest)
            );
            return false;
        }
    };

    match fs::rename(src_path, &target) {
        Ok(()) => return true,
        Err(e) if is_cross_device(&e) => {}
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                err,
                "mv: cannot move {} to {}: {why}",
                quoteaf_os(src),
                quoteaf_os(&target)
            );
            return false;
        }
    }

    if let Err(e) = copy_across_devices(src_path, &target, &metadata) {
        let why = strerror(&e);
        let _ = writeln!(
            err,
            "mv: cannot move {} to {}: {why}",
            quoteaf_os(src),
            quoteaf_os(&target)
        );
        return false;
    }
    true
}

/// Where one source lands.
///
/// # Errors
///
/// The source having no file-name component while the destination is a
/// directory — `mv a/.. dst`. See module docs, bug 3: the previous code turned
/// this into a request to move `a`'s parent onto `dst`.
fn compute_target(src: &Path, dest: &Path, dest_is_dir: bool) -> Result<PathBuf, &'static str> {
    if !dest_is_dir {
        return Ok(dest.to_path_buf());
    }
    match src.file_name() {
        Some(name) => Ok(dest.join(name)),
        None => Err("the source path ends in '.' or '..', so it names nothing to create there"),
    }
}

/// `EXDEV` — the kernel refusing to rename across a filesystem boundary, which
/// is the one `rename` failure `mv` is supposed to work around rather than
/// report.
#[cfg(unix)]
const CROSS_DEVICE_ERRNO: i32 = 18;
/// `ERROR_NOT_SAME_DEVICE`, the same condition on the development host.
#[cfg(windows)]
const CROSS_DEVICE_ERRNO: i32 = 17;

fn is_cross_device(e: &io::Error) -> bool {
    #[cfg(any(unix, windows))]
    if e.raw_os_error() == Some(CROSS_DEVICE_ERRNO) {
        return true;
    }
    // Checked second, not first: our own target's libstd may not yet map EXDEV
    // onto this variant, and a rename that *is* cross-device must not be
    // reported as a hard failure just because the classification is missing.
    e.kind() == io::ErrorKind::CrossesDevices
}

/// The `EXDEV` fallback: reproduce the source at `target`, then remove it.
///
/// # Errors
///
/// Any failure of the copy or the removal, and the two cases this does not
/// implement: a directory (which needs a recursive copy preserving modes,
/// symlinks and hard links) and recreating a symlink on a host without
/// `symlink(2)`.
fn copy_across_devices(src: &Path, target: &Path, metadata: &fs::Metadata) -> io::Result<()> {
    let kind = metadata.file_type();

    if kind.is_symlink() {
        // NOT `fs::copy`, which follows the link — see module docs, bug 4. The
        // link's *text* is reproduced verbatim, so a relative link keeps meaning
        // whatever it means relative to its new directory, exactly as `rename`
        // would have left it.
        let points_at = fs::read_link(src)?;
        symlink(&points_at, target)?;
        return fs::remove_file(src);
    }

    if kind.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "moving a directory across filesystems is not implemented by this mv",
        ));
    }

    fs::copy(src, target)?;
    fs::remove_file(src)
}

#[cfg(unix)]
fn symlink(points_at: &Path, at: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(points_at, at)
}

/// Recreating a symlink needs a distinction between file and directory links on
/// Windows, and a privilege the test host does not necessarily have. Refusing is
/// the only answer that does not silently produce something other than a
/// symlink; the target OS is the `#[cfg(unix)]` branch above.
#[cfg(not(unix))]
fn symlink(_points_at: &Path, _at: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "recreating a symlink is not supported on this host",
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

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// The operands of a successful parse, or a panic naming what came back.
    fn run_parse(items: &[&str]) -> Vec<String> {
        match parse_args(&args(items)).unwrap() {
            Request::Run(p) => p.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --

    #[test]
    fn no_args() {
        assert!(run_parse(&[]).is_empty());
    }

    #[test]
    fn simple_rename() {
        assert_eq!(run_parse(&["a", "b"]), vec!["a", "b"]);
    }

    #[test]
    fn force_is_accepted_and_changes_nothing() {
        assert_eq!(run_parse(&["-f", "a", "b"]), vec!["a", "b"]);
        assert_eq!(run_parse(&["--force", "a", "b"]), vec!["a", "b"]);
    }

    #[test]
    fn force_clustered_and_repeated() {
        assert_eq!(run_parse(&["-ff", "a", "b"]), vec!["a", "b"]);
    }

    #[test]
    fn flag_may_follow_operands() {
        assert_eq!(run_parse(&["a", "b", "-f"]), vec!["a", "b"]);
    }

    #[test]
    fn multiple_sources() {
        assert_eq!(run_parse(&["a", "b", "c", "d"]), vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-", "dest"]), vec!["-", "dest"]);
    }

    /// Bug 1 in the module docs: this used to answer `unknown option: --`, so a
    /// file named `-foo` could not be moved at all.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]), vec!["-foo", "bar"]);
        assert_eq!(run_parse(&["--", "-f"]), vec!["-f"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).is_empty());
    }

    #[test]
    fn long_options_abbreviate() {
        assert_eq!(run_parse(&["--for", "a", "b"]), vec!["a", "b"]);
    }

    /// `--v` must stay ambiguous between `--verbose` and `--version`. It only
    /// does so because `--verbose` is in the table despite being unimplemented;
    /// this is the test that fails if someone prunes the table to what is
    /// actually handled.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--v"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--verbose"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--version"), "{:?}", e.sentence);
    }

    /// Likewise `--n`, across all three `no-` options.
    #[test]
    fn ambiguous_no_prefix_is_refused() {
        let e = fail(&["--n"]);
        assert_eq!(
            e.sentence,
            "option '--n' is ambiguous; possibilities: '--no-clobber' \
             '--no-copy' '--no-target-directory'"
        );
    }

    /// The prefix that caught the table being wrong. `--no-c` reaches
    /// `--no-clobber` and `--no-copy`; before `("no-copy", …)` was added it
    /// resolved here and was ambiguous in GNU, which is the exact shape of
    /// silently acting on an option the user did not unambiguously name.
    #[test]
    fn ambiguous_no_c_prefix_is_refused() {
        let e = fail(&["--no-c"]);
        assert_eq!(
            e.sentence,
            "option '--no-c' is ambiguous; possibilities: '--no-clobber' \
             '--no-copy'"
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-z", "a", "b"]);
        assert!(e.sentence.contains("invalid option"), "{:?}", e.sentence);
        assert!(e.sentence.contains('z'), "{:?}", e.sentence);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz=1", "a", "b"]);
        assert!(
            e.sentence.contains("unrecognized option"),
            "{:?}",
            e.sentence
        );
        assert!(e.sentence.contains("--zzz=1"), "{:?}", e.sentence);
    }

    /// Unimplemented options are rejected *by name*, not as typos. `-n` asks
    /// for an existing file to be left alone; answering "invalid option" sends
    /// the user to check a spelling that was right, and ignoring it would
    /// overwrite the file they were protecting.
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in ["-b", "-i", "-n", "-t", "-T", "-u", "-v", "-S", "-Z"] {
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
            "--interactive",
            "--no-clobber",
            "--no-target-directory",
            "--strip-trailing-slashes",
            "--update",
            "--verbose",
            "--no-copy",
            "--debug",
            "--context",
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
        let e = fail(&["--force=yes", "a", "b"]);
        assert!(e.sentence.contains("doesn't allow"), "{:?}", e.sentence);
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
        match parse_args(&[OsString::from("-f"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(p) => assert_eq!(p, vec![bad, OsString::from("d")]),
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
    /// survive, so it is closed rather than noted.
    ///
    /// Windows has its own argument that no `String` can hold: an unpaired
    /// surrogate (a UTF-16 code unit in `0xD800..=0xDFFF` with no partner).
    /// `OsString` stores it as WTF-8, `String` cannot represent it, and
    /// `env::args()` unwraps on exactly it — the same `unwrap`, in the same std
    /// function, reached by a different route.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-f"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(p) => assert_eq!(p, vec![bad, OsString::from("d")]),
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

    // ----------------------------------------------------- compute_target --

    #[test]
    fn target_file_to_file() {
        let t = compute_target(Path::new("a.txt"), Path::new("b.txt"), false).unwrap();
        assert_eq!(t, PathBuf::from("b.txt"));
    }

    #[test]
    fn target_file_into_dir() {
        let t = compute_target(Path::new("src/a.txt"), Path::new("dst"), true).unwrap();
        assert_eq!(t, PathBuf::from("dst").join("a.txt"));
    }

    #[test]
    fn target_rename_within_dir() {
        let t = compute_target(Path::new("./old"), Path::new("new"), false).unwrap();
        assert_eq!(t, PathBuf::from("new"));
    }

    #[test]
    fn target_nested_source_into_dir() {
        let t = compute_target(Path::new("a/b/c.txt"), Path::new("/tmp"), true).unwrap();
        assert_eq!(t, PathBuf::from("/tmp").join("c.txt"));
    }

    /// Bug 3 in the module docs. `Path::file_name` is `None` here, and the old
    /// `unwrap_or_default()` turned that into `dst.join("")` == `dst`, i.e. a
    /// request to rename `a`'s **parent** onto `dst`.
    #[test]
    fn a_source_ending_in_dotdot_is_refused_not_collapsed() {
        let e = compute_target(Path::new("a/.."), Path::new("dst"), true).unwrap_err();
        assert!(e.contains("names nothing"), "{e}");
        let e = compute_target(Path::new(".."), Path::new("dst"), true).unwrap_err();
        assert!(e.contains("names nothing"), "{e}");
    }

    /// Same source, but the destination is not a directory: there is no name to
    /// append, so nothing collapses and the rename is the user's to make.
    #[test]
    fn a_source_ending_in_dotdot_is_fine_when_dest_is_not_a_dir() {
        let t = compute_target(Path::new("a/.."), Path::new("dst"), false).unwrap();
        assert_eq!(t, PathBuf::from("dst"));
    }

    // ------------------------------------------------------------ moving --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("mv_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// `move_all` plus whatever it wrote to its error sink.
    fn mv(paths: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = move_all(&owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    #[test]
    fn renames_a_file() {
        let dir = scratch("rename");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, err) = mv(&[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn moves_a_file_into_a_directory() {
        let dir = scratch("into_dir");
        let a = dir.join("a");
        let sub = dir.join("sub");
        fs::write(&a, b"x").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = mv(&[&a, &sub]);
        assert!(ok, "{err}");
        assert!(sub.join("a").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, err) = mv(&[]);
        assert!(!ok);
        assert!(err.contains("missing file operand"), "{err}");
        let _ = err;
    }

    /// GNU distinguishes "no operands" from "one operand" and names the one it
    /// got; the old code printed `missing operand` for both.
    #[test]
    fn one_operand_names_it() {
        let (ok, err) = mv(&[Path::new("solo")]);
        assert!(!ok);
        assert!(err.contains("missing destination file operand"), "{err}");
        assert!(err.contains("solo"), "{err}");
    }

    #[test]
    fn several_sources_need_a_directory() {
        let dir = scratch("not_a_dir");
        let a = dir.join("a");
        let b = dir.join("b");
        let c = dir.join("c");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();
        fs::write(&c, b"z").unwrap();
        let (ok, err) = mv(&[&a, &b, &c]);
        assert!(!ok);
        assert!(err.contains("is not a directory"), "{err}");
        // Nothing was touched.
        assert!(a.is_file() && b.is_file() && c.is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 2 in the module docs: with `-f` the old code printed nothing here and
    /// still exited 1. `-f` is not even a parameter any more, so the only way to
    /// get silence would be to lose the diagnostic for everyone.
    #[test]
    fn a_missing_source_is_reported() {
        let dir = scratch("missing_src");
        let (ok, err) = mv(&[&dir.join("nope"), &dir.join("dst")]);
        assert!(!ok);
        assert!(err.contains("cannot stat"), "{err}");
        assert!(err.contains("nope"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("partial");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        let a = dir.join("a");
        let c = dir.join("c");
        fs::write(&a, b"a").unwrap();
        fs::write(&c, b"c").unwrap();
        let (ok, err) = mv(&[&a, &dir.join("gone"), &c, &sub]);
        assert!(!ok, "the missing source must count against the status");
        assert!(err.contains("gone"), "{err}");
        assert!(sub.join("a").is_file(), "the first source must still move");
        assert!(
            sub.join("c").is_file(),
            "and so must the one after the error"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 3, end to end. Before the fix this asked the kernel to rename the
    /// scratch directory itself onto `sub`.
    #[test]
    fn a_dotdot_source_does_not_move_the_parent() {
        let dir = scratch("dotdot");
        let inner = dir.join("inner");
        let sub = dir.join("sub");
        fs::create_dir(&inner).unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = mv(&[&inner.join(".."), &sub]);
        assert!(!ok);
        assert!(err.contains("names nothing"), "{err}");
        assert!(dir.is_dir(), "the parent must still be where it was");
        assert!(inner.is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A dangling symlink is a thing that exists and can be renamed. The old
    /// code's `fs::copy` fallback read *through* it, so this reported "No such
    /// file or directory" about a link that was plainly there.
    #[test]
    #[cfg(unix)]
    fn moves_a_dangling_symlink() {
        let dir = scratch("dangling");
        let link = dir.join("link");
        std::os::unix::fs::symlink(dir.join("nowhere"), &link).unwrap();
        let moved = dir.join("moved");
        let (ok, err) = mv(&[&link, &moved]);
        assert!(ok, "{err}");
        assert!(
            fs::symlink_metadata(&moved)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::symlink_metadata(&link).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 4's unit: the cross-device fallback must reproduce a symlink *as* a
    /// symlink. `fs::rename` would not have gone through here, so the fallback
    /// is called directly — there is no portable way to make two filesystems
    /// appear in a unit test.
    #[test]
    #[cfg(unix)]
    fn the_cross_device_fallback_relinks_rather_than_copying_the_target() {
        let dir = scratch("xdev_symlink");
        let real = dir.join("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let moved = dir.join("moved");

        let meta = fs::symlink_metadata(&link).unwrap();
        copy_across_devices(&link, &moved, &meta).unwrap();

        let moved_meta = fs::symlink_metadata(&moved).unwrap();
        assert!(
            moved_meta.file_type().is_symlink(),
            "a symlink must arrive as a symlink, not as a copy of its target"
        );
        assert_eq!(fs::read_link(&moved).unwrap(), real);
        assert!(fs::symlink_metadata(&link).is_err(), "source must be gone");
        assert_eq!(fs::read(&real).unwrap(), b"contents", "target untouched");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cross_device_fallback_moves_a_plain_file() {
        let dir = scratch("xdev_file");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"bytes").unwrap();
        let meta = fs::symlink_metadata(&a).unwrap();
        copy_across_devices(&a, &b, &meta).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"bytes");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Not implemented, and it says so rather than moving part of the tree.
    #[test]
    fn the_cross_device_fallback_refuses_a_directory() {
        let dir = scratch("xdev_dir");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("inside"), b"x").unwrap();
        let meta = fs::symlink_metadata(&sub).unwrap();
        let e = copy_across_devices(&sub, &dir.join("elsewhere"), &meta).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
        assert!(sub.join("inside").is_file(), "nothing may be moved");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A file whose name is not valid UTF-8 — the case the whole rewrite is
    /// about — must move like any other.
    #[test]
    #[cfg(unix)]
    fn moves_a_file_whose_name_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let dir = scratch("nonutf8");
        let mut name = dir.clone().into_os_string().into_vec();
        name.extend_from_slice(b"/\x80bad");
        let src = PathBuf::from(OsString::from_vec(name));
        fs::write(&src, b"x").unwrap();
        let dst = dir.join("ok");
        let (ok, err) = mv(&[&src, &dst]);
        assert!(ok, "{err}");
        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"x");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn is_cross_device_does_not_fire_on_an_ordinary_error() {
        assert!(!is_cross_device(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
        assert!(!is_cross_device(&io::Error::from(io::ErrorKind::NotFound)));
    }

    #[test]
    fn is_cross_device_fires_on_the_platform_errno() {
        assert!(is_cross_device(&io::Error::from_raw_os_error(
            CROSS_DEVICE_ERRNO
        )));
    }
}
