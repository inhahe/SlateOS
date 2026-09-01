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
//!    ordinary glob (`mv */.. dst`).
//!
//!    The target name is now built by [`target_in_directory`], which appends the
//!    last component's *bytes* — `.` and `..` included — and so has no empty
//!    name to collapse. See [`coreutils::fileid`] for why the split is done on
//!    bytes rather than through `Path::file_name`.
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
//! # And seventeen more, found by measurement rather than by reading
//!
//! The four above were found by reading the code. That method had reached its
//! limit — the remaining bugs were all in behaviour that *looked* right. So
//! `scripts/mv-diff.sh` runs this `mv` and GNU coreutils 9.4 over the same 178
//! fixtures and compares exit status, both streams, and the resulting tree
//! byte-for-byte. It found **seventeen** differences on its first run, none of
//! which had been suspected.
//!
//! Nearly all of them came from one structural mistake: the old code decided
//! *first* whether the destination was a directory, then computed a target, then
//! renamed. GNU inverts this. It renames **speculatively** first
//! (`mv.c:466`) — `RENAME_NOREPLACE`, so it cannot clobber — and only asks any
//! further question if that fails. The order is not an optimisation; it is what
//! makes the answers come out right, because a rename that succeeded proves the
//! destination was free and every "is the destination …" check is then moot. The
//! tri-state that carries this is [`Renamed`], GNU's `x.rename_errno`.
//!
//! The differences it exposed, grouped:
//!
//! - **Refusals that were not made at all.** Moving a file onto itself
//!   (`mv a a`), onto a hard link to itself, or through a symlink to itself
//!   destroyed the file and left nothing — `mv link file`, where `link` points
//!   at `file`, deleted `file`. [`same_file_ok`] is GNU's check, reduced to this
//!   `mv`'s option set and then measured case by case against GNU, including the
//!   pair upstream documents at `copy.c:1907`: with `l` a hard link to `f` and
//!   `s` a symlink to `f`, `mv s f` must fail and `mv s l` must succeed.
//! - **Two sources with the same basename silently ate each other.**
//!   `mv one/same two/same dir` moved both to `dir/same` and reported success:
//!   two files in, one file out, no message. GNU keeps a set of
//!   already-written destinations ([`DestInfo`]) and refuses the second with
//!   `will not overwrite just-created`.
//! - **Directory-vs-non-directory collisions.** Overwriting a directory with a
//!   file, or a file with a directory, produced the kernel's bare `Is a
//!   directory` rather than the sentence naming both operands.
//! - **The wrong operand was named.** A failure caused by the *destination* —
//!   it is a non-empty directory, it is a running binary, the disk is full —
//!   named the source too, which `copy.c:2851` says "is more likely to confuse
//!   the user than be helpful". See [`blames_the_destination`].
//! - **Diagnostics that were this file's own sentences** rather than the ones
//!   scripts and tests actually match on: `target 'c' is not a directory` for
//!   `target 'c': Not a directory`, and a bare `Invalid argument` where a
//!   directory had been asked to become a subdirectory of itself.
//!
//! The harness is the artifact to keep, not the fix list: it is 178 cases, it
//! runs in about a minute, and it is how the next seventeen get found. Sixty-one
//! of its cases are marked as differing on purpose — every one is an option this
//! file does not implement yet, so implementing one is expected to *promote* a
//! case rather than to add one.
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
use coreutils::fileid::{FileId, file_id, nlink, same_entry, same_inode, split_entry};
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::{quoteaf_os, quotef_os};
use coreutils::stdfd::{self, Stream};
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

/// What the speculative rename left behind — GNU's `x.rename_errno`
/// (`copy.h:277`), whose three states drive everything below.
///
/// The tri-state is not an implementation detail that could be flattened into a
/// `Result`: which of the three it is decides *what is even checked*. `Done`
/// means the move already happened and nothing may be looked at again;
/// `Failed(EEXIST)` means something is in the way, which is where every refusal
/// lives; any other `Failed` is reported without ever consulting the
/// destination.
enum Renamed {
    /// GNU's `-1`. No attempt yet, so [`move_one`] makes it.
    NotTried,
    /// GNU's `0`. The source is at the destination already; there was nothing
    /// there to overwrite, so no question of overwriting arose.
    Done,
    /// A failed attempt, carrying the reason.
    Failed(io::Error),
}

/// Try to rename, but only onto a name that does not exist: GNU's
/// `renameatu (…, RENAME_NOREPLACE)` (`mv.c:466`).
///
/// The point of doing this *first*, before `mv` has decided whether the last
/// operand is a directory, is that the overwhelmingly common case — a rename
/// onto a free name — then costs one syscall and skips every check, and the
/// checks are only reached when there is something to check.
///
/// `RENAME_NOREPLACE` is a `renameat2` flag that `std` does not expose, so this
/// is gnulib's own fallback for a host that lacks the syscall
/// (`lib/renameatu.c:134`): look first, then rename. That has a race, and
/// gnulib's comment says so — between the look and the rename someone else may
/// create the destination, and it is then overwritten. Upstream accepts the
/// race on such hosts, and the alternative here would be to guess at a raw
/// syscall number.
fn rename_noreplace(src: &Path, dst: &Path) -> io::Result<()> {
    // `symlink_metadata`: a *dangling* symlink at the destination still occupies
    // the name, so it is "exists" for this question.
    match fs::symlink_metadata(dst) {
        Ok(_) => return Err(io::Error::from(io::ErrorKind::AlreadyExists)),
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        Err(_) => {}
    }
    fs::rename(src, dst)
}

/// Is this the errno that means "the destination is already there"?
///
/// Compared as a *kind* rather than as a number because [`rename_noreplace`]
/// synthesises it rather than receiving it from the kernel, and the two must
/// answer alike.
fn is_exists(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::AlreadyExists
}

/// Can this operand be used as a directory to move things into? gnulib's
/// `target_directory_operand` (`lib/targetdir.c`).
///
/// Upstream opens it `O_PATH | O_DIRECTORY` and keeps the descriptor for the
/// `*at` calls that follow; we have no such calls, so the question reduces to
/// the one the open answers. It *follows* symlinks, which is why
/// `mv a link-to-dir` puts `a` inside the directory.
///
/// # Errors
///
/// The failure the caller reports as `target 'x': …` — `ENOENT` when the operand
/// is absent (including a dangling symlink, since this follows), `ENOTDIR` when
/// it exists and is not a directory.
///
/// The `ENOTDIR` case is synthesised rather than observed: upstream gets it from
/// the `O_DIRECTORY` open, whereas the `metadata` call here *succeeds* and the
/// `is_dir` test is what fails. It is built from the [`io::ErrorKind`] and not
/// from the number 20, because that number is `ENOTDIR` only on a host where it
/// is an errno at all — on the Windows development host `from_raw_os_error(20)`
/// is a Win32 code and prints `The system cannot find the device specified.`
/// The kind is what [`strerror`] reads, and it yields `Not a directory` on both.
fn target_directory_operand(path: &Path) -> io::Result<()> {
    let meta = fs::metadata(path)?;
    if meta.is_dir() {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotADirectory))
    }
}

/// The destinations already written by *this* command, as GNU's `dest_info`
/// (`copy.h:289`) — a set of `(name, file)` pairs, not just names.
///
/// It has to be pairs. The question it answers is not "did two operands share a
/// basename" but "is the thing sitting at that name the thing I just put
/// there": if the name held something else all along, overwriting it is an
/// ordinary overwrite and GNU performs it silently.
type DestInfo = std::collections::HashSet<(OsString, FileId)>;

/// Where `src` lands inside directory `dir`, and under what name.
///
/// GNU's `mv.c:540`: `file_name_concat (target_directory, last_component
/// (source), &dst_relname)`, followed by `strip_trailing_slashes (dst_relname)`.
/// [`split_entry`] already does the stripping, so the two halves of its answer
/// are the two halves of this one.
///
/// **The last component is appended verbatim, `.` and `..` included.** This is
/// the one place `mv` and `cp` genuinely differ: `cp` has an
/// `arg_base += STREQ (arg_base, "..")` bump (`cp.c:678`) and `mv` has no such
/// line, so `cp a/.. d` targets `d/a` while `mv a/.. d` targets `d/..`. Reading
/// that as "`mv` forgot" and adding the bump here would be wrong twice over: it
/// would move the wrong file, and it would do so silently, where the verbatim
/// name reliably fails `EEXIST` or `EBUSY` and says so.
fn target_in_directory(dir: &Path, src: &Path) -> (PathBuf, OsString) {
    let (_, base) = split_entry(src);
    (dir.join(&base), base)
}

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
///
/// The shape follows GNU's `main` (`mv.c:440-550`), and the order is
/// load-bearing rather than stylistic — see [`Renamed`].
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

    let last = Path::new(dest);
    let mut state = if sources.len() == 1 {
        match rename_noreplace(Path::new(&sources[0]), last) {
            Ok(()) => Renamed::Done,
            Err(e) => Renamed::Failed(e),
        }
    } else {
        Renamed::NotTried
    };

    // Only now — and only if that did not already settle it — is the last
    // operand asked whether it is a directory.
    let mut into: Option<&Path> = None;
    if !matches!(state, Renamed::Done) {
        match target_directory_operand(last) {
            Ok(()) => {
                state = Renamed::NotTried;
                into = Some(last);
            }
            Err(e) => {
                // With two operands the last one is simply the new name, and
                // not being a directory is unremarkable. With three or more it
                // *had* to be a directory, and this is fatal for the whole
                // command rather than for one source: GNU's
                // `error (EXIT_FAILURE, …)` at `mv.c:495`.
                if sources.len() > 1 {
                    let why = strerror(&e);
                    let _ = writeln!(err, "mv: target {}: {why}", quoteaf_os(dest));
                    return false;
                }
            }
        }
    }

    let Some(dir) = into else {
        // Two operands, last operand not a directory: one move, to that name.
        return move_one(
            Path::new(&sources[0]),
            last,
            dest,
            state,
            true,
            &mut None,
            err,
        );
    };

    // The set is built only when it can matter — GNU's comment at `mv.c:526`:
    // "the problem it is used to detect can arise only if there are two or more
    // files to move."
    let mut seen: Option<DestInfo> = (sources.len() >= 2).then(DestInfo::default);

    let mut ok = true;
    for (i, src) in sources.iter().enumerate() {
        let src_path = Path::new(src);
        let (target, base) = target_in_directory(dir, src_path);
        // The last operand is exempt from being recorded, because nothing that
        // follows could collide with it (`copy.c:2779`).
        let last_file = i.saturating_add(1) == sources.len();
        if !move_one(
            src_path,
            &target,
            &base,
            Renamed::NotTried,
            last_file,
            &mut seen,
            err,
        ) {
            ok = false;
        }
    }
    ok
}

/// Move one source to one already-computed target: GNU's `copy_internal`
/// reduced to the options this `mv` has.
///
/// `relname` is the target's name *within the destination directory*, which is
/// the key [`DestInfo`] is built on; with two operands it is the whole
/// destination operand, and is then never consulted because `seen` is `None`.
///
/// Returns `false` if this source should count against the exit status.
#[allow(clippy::too_many_lines)]
fn move_one<W: Write>(
    src: &Path,
    target: &Path,
    relname: &OsString,
    state: Renamed,
    last_file: bool,
    seen: &mut Option<DestInfo>,
    err: &mut W,
) -> bool {
    let mut failure = match state {
        // Already moved, and with `last_file` the recording is skipped too, so
        // there is nothing left to do. This is the common case.
        Renamed::Done => return record_move(target, relname, last_file, seen),
        Renamed::NotTried => match rename_noreplace(src, target) {
            Ok(()) => return record_move(target, relname, last_file, seen),
            Err(e) => e,
        },
        Renamed::Failed(e) => e,
    };

    // The source is stat'd only now, which is why a missing source is reported
    // as `cannot stat` rather than as a rename failure. `symlink_metadata`, not
    // `exists`/`is_dir`: `mv` moves a symlink as itself, whatever it points at
    // — including nothing.
    let src_meta = match fs::symlink_metadata(src) {
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

    // Whatever the rename said, a destination that exists makes this the
    // "something is in the way" case (`copy.c:2322`) — so `mv a/. d`, which
    // fails `EBUSY`, is examined as an overwrite and only *then* fails `EBUSY`
    // for real.
    let dst_meta = fs::symlink_metadata(target).ok();
    if dst_meta.is_some() {
        failure = io::Error::from(io::ErrorKind::AlreadyExists);
    }

    // The refusals are asked only of a destination that is actually there —
    // there is nothing to refuse to overwrite otherwise.
    if let Some(dst_meta) = &dst_meta
        && !refuse_overwrite_checks(src, &src_meta, target, dst_meta, relname, seen, err)
    {
        return false;
    }

    // Now the real rename, the one allowed to replace what is there. Keyed on
    // the errno rather than on `dst_meta`, which is `copy.c:2757` exactly and
    // is not the same condition: between the speculative rename above and the
    // stat, something else can *remove* the destination. GNU retries and
    // succeeds; reporting `File exists` for a name that is now free would be
    // wrong. (When `dst_meta` is `Some` the assignment above has already made
    // this true, so the ordinary overwrite still passes through here.)
    if is_exists(&failure) {
        match fs::rename(src, target) {
            Ok(()) => return record_move(target, relname, last_file, seen),
            Err(e) => failure = e,
        }
    }

    // A directory asked to become a subdirectory of itself. GNU keys on this
    // one errno and says so is fragile (`copy.c:2798`); there is no better
    // signal, and the alternative is the unhelpfully bare `Invalid argument`.
    if is_subdirectory_of_itself(&failure) {
        let _ = writeln!(
            err,
            "mv: cannot move {} to a subdirectory of itself, {}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return false;
    }

    if !is_cross_device(&failure) {
        let why = strerror(&failure);
        // When the destination is what went wrong, naming the source as well
        // "is more likely to confuse the user than be helpful"
        // (`copy.c:2851`).
        if blames_the_destination(&failure) {
            let _ = writeln!(err, "mv: cannot overwrite {}: {why}", quoteaf_os(target));
        } else {
            let _ = writeln!(
                err,
                "mv: cannot move {} to {}: {why}",
                quoteaf_os(src),
                quoteaf_os(target)
            );
        }
        return false;
    }

    if let Err(e) = copy_across_devices(src, target, &src_meta) {
        let why = strerror(&e);
        let _ = writeln!(
            err,
            "mv: cannot move {} to {}: {why}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return false;
    }
    record_move(target, relname, last_file, seen)
}

/// Note that the file just moved now sits at `relname`, so a later source that
/// lands on the same name can be told apart from an ordinary overwrite.
///
/// # Why this stats the destination and not the source
///
/// The source is *gone*: the rename that this is recording the success of has
/// just moved it away, so there is nothing left at that name to stat. GNU stats
/// the destination for exactly this reason — `copy.c:2246` picks the name to
/// stat with `rename_errno == 0 ? dst_name : src_name`, and the variable it
/// fills is called `src_sb` only because the *other* branch fills it from the
/// source. A rename does not change a file's device or inode, so the two are the
/// same identity, and only one of them is still readable.
///
/// Getting this backwards is silent rather than loud, which is what makes it
/// worth a comment: the stat simply fails, the set stays empty, and the
/// just-created check it exists to feed never fires. `mv one/same two/same dir`
/// then overwrites `dir/same` and reports success — two files in, one file out.
///
/// Always returns `true`: it is called only on success paths, and a set that
/// could not be updated costs the next source its refusal but never invents one.
fn record_move(
    target: &Path,
    relname: &OsString,
    last_file: bool,
    seen: &mut Option<DestInfo>,
) -> bool {
    // The last source is exempt: nothing follows it that could collide
    // (`copy.c:2779`), and GNU does not even take the stat.
    if last_file {
        return true;
    }
    if let Some(set) = seen {
        // `symlink_metadata`: `mv` is `DEREF_NEVER`, so a moved symlink is
        // recorded as itself rather than as whatever it points at.
        if let Ok(meta) = fs::symlink_metadata(target)
            && let Some(id) = file_id(target, &meta)
        {
            set.insert((relname.clone(), id));
        }
    }
    true
}

/// The refusals that stand between "something is at the destination" and the
/// rename that would replace it. Returns `false` once one has been reported.
///
/// The order is GNU's, and it is observable: a request that trips two of these
/// gets the first one's wording.
fn refuse_overwrite_checks<W: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    target: &Path,
    dst_meta: &fs::Metadata,
    relname: &OsString,
    seen: &Option<DestInfo>,
    err: &mut W,
) -> bool {
    // 1. Is the destination the source? (`copy.c:2345`)
    if !same_file_ok(src, src_meta, target, dst_meta) {
        let _ = writeln!(
            err,
            "mv: {} and {} are the same file",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return false;
    }

    let (src_dir, dst_dir) = (src_meta.is_dir(), dst_meta.is_dir());

    // 2. A directory onto a non-directory (`copy.c:2455`). The destination is
    //    named first, which reads oddly until you notice the sentence is about
    //    what is being destroyed.
    if !dst_dir && src_dir {
        let _ = writeln!(
            err,
            "mv: cannot overwrite non-directory {} with directory {}",
            quoteaf_os(target),
            quoteaf_os(src)
        );
        return false;
    }

    // 3. A destination this same command line just created (`copy.c:2473`).
    //    GNU's comment: "Don't let the user destroy their data, even if they
    //    try hard: this mv command must fail: mv a/f b/f c".
    if !dst_dir
        && let Some(set) = seen
        && let Some(id) = file_id(target, dst_meta)
        && set.contains(&(relname.clone(), id))
    {
        let _ = writeln!(
            err,
            "mv: will not overwrite just-created {} with {}",
            quoteaf_os(target),
            quoteaf_os(src)
        );
        return false;
    }

    // 4. A non-directory onto a directory (`copy.c:2485`), which unlike 2 does
    //    not name the source at all.
    if !src_dir && dst_dir {
        let _ = writeln!(
            err,
            "mv: cannot overwrite directory {} with non-directory",
            quoteaf_os(target)
        );
        return false;
    }

    // 5. `copy.c:2504`. Unreachable while 2 stands above it — it is GNU's
    //    belt-and-braces for the `--backup` path that lets 2 through — and kept
    //    so that adding `-b` does not silently lose the guard.
    if src_dir && !dst_dir {
        let _ = writeln!(
            err,
            "mv: cannot move directory onto non-directory: {} -> {}",
            quotef_os(src),
            quotef_os(target)
        );
        return false;
    }

    true
}

/// Would moving `src` onto `target` destroy the very thing being moved? GNU's
/// `same_file_ok` (`copy.c:1739`), reduced to `mv`'s option set — `move_mode`,
/// `DEREF_NEVER`, no backups, no hard/symbolic linking.
///
/// `true` means "go ahead". The reduction drops three whole branches
/// (`x->hard_link`, the `dereference != DEREF_NEVER` arm, and the backup block),
/// and what is left is genuinely subtle, so each surviving step says which
/// question it answers.
///
/// The case that makes this worth its length is the one GNU spells out in its
/// own comment (`copy.c:1907`):
///
/// ```text
/// touch f && ln f l && ln -s f s
/// mv s f   must fail — `f` is the only thing `s` names, and moving the link
///          onto it leaves a link pointing at itself
/// mv s l   must succeed — `f` survives as the other name for the data
/// ```
///
/// Measured against GNU 9.4 both ways; this `mv` previously performed the first
/// one and destroyed the file.
fn same_file_ok(src: &Path, src_meta: &fs::Metadata, dst: &Path, dst_meta: &fs::Metadata) -> bool {
    let same = same_inode((src, src_meta), (dst, dst_meta));
    let (src_link, dst_link) = (
        src_meta.file_type().is_symlink(),
        dst_meta.file_type().is_symlink(),
    );

    // Two symlinks: what matters is whether they are the same *link*, because
    // replacing one link with another touches nothing either points at.
    if src_link && dst_link {
        let same_name = same_entry(src, dst);
        // Unless they are two hard links to one symlink, where the rename would
        // do nothing at all and silently report success.
        if !same_name && same {
            return false;
        }
        return !same_name;
    }

    // Moving onto a symlink is fine: the rename replaces the link itself, so
    // whatever it pointed at is untouched.
    if dst_link {
        return true;
    }

    // Two hard links to one file, reached by different names. The rename would
    // remove one of them, and which one is a race.
    if same && nlink(dst_meta) > 1 && !same_entry(src, dst) {
        return false;
    }

    // Neither is a symlink, so the only way to be the same file is to be it.
    if !src_link && !same {
        return true;
    }

    // A symlink onto a file that has another name: the data survives under that
    // other name, so this is allowed. `canonicalize`, because the question is
    // where the link *ends up*, not what one hop of it says.
    if src_link
        && nlink(dst_meta) > 1
        && let Ok(resolved) = fs::canonicalize(src)
    {
        return !same_entry(&resolved, dst);
    }

    // Last: follow both sides all the way and compare. This is what catches
    // `mv link file` where `link` resolves to `file` — the two are different
    // *entries* and different *links*, and the same file.
    let followed = |path: &Path, meta: &fs::Metadata, is_link: bool| {
        if is_link {
            fs::metadata(path).ok()
        } else {
            Some(meta.clone())
        }
    };
    let (Some(s), Some(d)) = (
        followed(src, src_meta, src_link),
        followed(dst, dst_meta, dst_link),
    ) else {
        // A dangling link is not the same file as anything.
        return true;
    };
    !same_inode((src, &s), (dst, &d))
}

/// `EXDEV` — the kernel refusing to rename across a filesystem boundary, which
/// is the one `rename` failure `mv` is supposed to work around rather than
/// report.
#[cfg(unix)]
const CROSS_DEVICE_ERRNO: i32 = 18;
/// `ERROR_NOT_SAME_DEVICE`, the same condition on the development host.
#[cfg(windows)]
const CROSS_DEVICE_ERRNO: i32 = 17;

/// `EINVAL`, which `rename` reports for "the destination is inside the source".
///
/// `mv` gives this its own diagnostic (`copy.c:2798`) rather than the generic
/// one, because "Invalid argument" tells the user nothing about which of the two
/// paths was the problem.
#[cfg(unix)]
const SUBDIRECTORY_OF_ITSELF_ERRNO: i32 = 22;

/// Is this the `rename` failure that means "you asked me to put a directory
/// inside itself"?
///
/// Only asked of a number, and only on a host where that number is an errno.
/// `ErrorKind` has no variant for this, and the kind std *does* map `EINVAL` to
/// — `InvalidInput` — is far too broad to key a specific diagnostic on: on the
/// development host it would claim every rejected rename was this case. GNU
/// itself notes at `copy.c:2798` that keying on the errno is fragile; keying on
/// a coarser classification would be worse.
#[cfg(unix)]
fn is_subdirectory_of_itself(e: &io::Error) -> bool {
    e.raw_os_error() == Some(SUBDIRECTORY_OF_ITSELF_ERRNO)
}

/// On a host where that number is not an errno there is nothing to key on, so
/// the request falls through to the generic `cannot move` diagnostic.
#[cfg(not(unix))]
fn is_subdirectory_of_itself(_e: &io::Error) -> bool {
    false
}

/// Does this `rename` failure blame the destination rather than the move?
///
/// `copy.c:2848` — the switch that picks between `cannot overwrite %s`, naming
/// only the destination, and `cannot move %s to %s`, naming both. Every code
/// here is one the kernel can only be reporting *about* the existing
/// destination: it is a directory, it is not empty, it is a running binary, it
/// is out of space or quota, it already has the maximum link count. Naming the
/// source in those cases would point at the wrong file.
///
/// The values are Linux's. This runs on the development host too, where the
/// numbers differ and nothing here matches — the fallback diagnostic is the
/// less specific one, which is safe; the target is where it must be right.
fn blames_the_destination(e: &io::Error) -> bool {
    /// `EEXIST`, `EISDIR`, `ENOTEMPTY`, `ETXTBSY`, `EDQUOT`, `EMLINK`,
    /// `ENOSPC` — in the order `copy.c` lists them.
    const DESTINATION_CODES: &[i32] = &[
        122, // EDQUOT
        17,  // EEXIST
        21,  // EISDIR
        31,  // EMLINK
        28,  // ENOSPC
        26,  // ETXTBSY
        39,  // ENOTEMPTY
    ];
    if cfg!(unix)
        && e.raw_os_error()
            .is_some_and(|n| DESTINATION_CODES.contains(&n))
    {
        return true;
    }
    // The two the standard library classifies for us, so that the development
    // host reaches the same branch for the cases it can actually produce.
    matches!(
        e.kind(),
        io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty
    )
}

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
    use scratchdir::ScratchDir;

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

    // ------------------------------------------------ target_in_directory --

    #[test]
    fn target_file_into_dir() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("src/a.txt"));
        assert_eq!(t, PathBuf::from("dst").join("a.txt"));
        assert_eq!(rel, OsString::from("a.txt"));
    }

    #[test]
    fn target_nested_source_into_dir() {
        let (t, rel) = target_in_directory(Path::new("/tmp"), Path::new("a/b/c.txt"));
        assert_eq!(t, PathBuf::from("/tmp").join("c.txt"));
        assert_eq!(rel, OsString::from("c.txt"));
    }

    /// Trailing slashes are decoration on the source, and GNU strips them from
    /// the relname (`strip_trailing_slashes`, `mv.c:541`) so that the set of
    /// already-written destinations is keyed on the name and not on how the
    /// operand was typed. `mv d/ x` and `mv d x` must collide with each other.
    #[test]
    fn a_trailing_slash_on_the_source_is_not_part_of_the_name() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("a/b///"));
        assert_eq!(t, PathBuf::from("dst").join("b"));
        assert_eq!(rel, OsString::from("b"));
    }

    /// Bug 3 in the module docs, now fixed the way GNU fixes it — which is by
    /// *not* special-casing it at all.
    ///
    /// The old code called `Path::file_name`, which answers `None` for a name
    /// ending in `..`, and `unwrap_or_default()` turned that into
    /// `dst.join("")` == `dst`: a silent request to rename `a`'s **parent**
    /// onto `dst`. The fix that followed refused the operand outright, which
    /// was safe but still not GNU: GNU appends the component verbatim, so the
    /// target is the literal `dst/..`. That name then fails on its own merits —
    /// `EEXIST`, and with `-T` `EBUSY` — with a diagnostic naming a path the
    /// user can recognise.
    #[test]
    fn a_source_ending_in_dotdot_appends_dotdot_verbatim() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("a/.."));
        assert_eq!(t, PathBuf::from("dst").join(".."));
        assert_eq!(rel, OsString::from(".."));

        let (t, rel) = target_in_directory(Path::new("dst"), Path::new(".."));
        assert_eq!(t, PathBuf::from("dst").join(".."));
        assert_eq!(rel, OsString::from(".."));
    }

    /// And the same for `.`, which `Path::file_name` also answers `None` for.
    #[test]
    fn a_source_ending_in_dot_appends_dot_verbatim() {
        let (t, rel) = target_in_directory(Path::new("dst"), Path::new("a/."));
        assert_eq!(t, PathBuf::from("dst").join("."));
        assert_eq!(rel, OsString::from("."));
    }

    // ------------------------------------------------------------ moving --

    /// A private directory for one test, removed when the binding drops.
    ///
    /// Delegated to `scratchdir` rather than hand-rolled, for the reason spelled
    /// out at `cp.rs`'s copy of this helper: the hand-rolled version built child
    /// paths with `Path::join`, which uses the host's `\` on this development
    /// box, and this file's own [`split_entry`] — like every path function in
    /// the tree — treats `/` as the only separator and `\` as an ordinary byte
    /// in a filename.
    fn scratch(stem: &str) -> ScratchDir {
        ScratchDir::new(&format!("mv_test_{stem}"))
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
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, err) = mv(&[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"hello");
    }

    #[test]
    fn moves_a_file_into_a_directory() {
        let dir = scratch("into_dir");
        let a = dir.path("a");
        let sub = dir.path("sub");
        fs::write(&a, b"x").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = mv(&[&a, &sub]);
        assert!(ok, "{err}");
        assert!(sub.join("a").is_file());
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

    /// The wording is GNU's `error (EXIT_FAILURE, err, _("target %s"), …)`
    /// (`mv.c:495`) — `target 'c': Not a directory`, the operand named and the
    /// reason appended by the same `errno`-printing path every other diagnostic
    /// uses. This file used to compose its own sentence, `target 'c' is not a
    /// directory`, which reads better and is not what anything greps for.
    #[test]
    fn several_sources_need_a_directory() {
        let dir = scratch("not_a_dir");
        let a = dir.path("a");
        let b = dir.path("b");
        let c = dir.path("c");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();
        fs::write(&c, b"z").unwrap();
        let (ok, err) = mv(&[&a, &b, &c]);
        assert!(!ok);
        assert!(err.contains("Not a directory"), "{err}");
        // Nothing was touched.
        assert!(a.is_file() && b.is_file() && c.is_file());
    }

    /// Bug 2 in the module docs: with `-f` the old code printed nothing here and
    /// still exited 1. `-f` is not even a parameter any more, so the only way to
    /// get silence would be to lose the diagnostic for everyone.
    #[test]
    fn a_missing_source_is_reported() {
        let dir = scratch("missing_src");
        let (ok, err) = mv(&[&dir.path("nope"), &dir.path("dst")]);
        assert!(!ok);
        assert!(err.contains("cannot stat"), "{err}");
        assert!(err.contains("nope"), "{err}");
    }

    #[test]
    fn a_failure_does_not_abort_the_rest() {
        let dir = scratch("partial");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        let a = dir.path("a");
        let c = dir.path("c");
        fs::write(&a, b"a").unwrap();
        fs::write(&c, b"c").unwrap();
        let (ok, err) = mv(&[&a, &dir.path("gone"), &c, &sub]);
        assert!(!ok, "the missing source must count against the status");
        assert!(err.contains("gone"), "{err}");
        assert!(sub.join("a").is_file(), "the first source must still move");
        assert!(
            sub.join("c").is_file(),
            "and so must the one after the error"
        );
    }

    /// Bug 3, end to end. Before the fix this asked the kernel to rename the
    /// scratch directory itself onto `sub`.
    ///
    /// The wording is GNU's, and it is worth saying why it is *this* wording
    /// rather than a refusal of the operand. `inner/..` and `sub/..` are both
    /// the scratch directory, so the two operands name one file, and "are the
    /// same file" is both true and the most informative thing available. It is
    /// also not a special case anywhere in the code: the target is built by
    /// appending `..` verbatim, and the ordinary same-file check then catches
    /// it. Measured against GNU coreutils 9.4, which prints exactly this.
    #[test]
    fn a_dotdot_source_does_not_move_the_parent() {
        let dir = scratch("dotdot");
        let inner = dir.path("inner");
        let sub = dir.path("sub");
        fs::create_dir(&inner).unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = mv(&[&inner.join(".."), &sub]);
        assert!(!ok);
        assert!(err.contains("are the same file"), "{err}");
        assert!(dir.dir().is_dir(), "the parent must still be where it was");
        assert!(inner.is_dir());
    }

    /// A dangling symlink is a thing that exists and can be renamed. The old
    /// code's `fs::copy` fallback read *through* it, so this reported "No such
    /// file or directory" about a link that was plainly there.
    #[test]
    #[cfg(unix)]
    fn moves_a_dangling_symlink() {
        let dir = scratch("dangling");
        let link = dir.path("link");
        std::os::unix::fs::symlink(dir.path("nowhere"), &link).unwrap();
        let moved = dir.path("moved");
        let (ok, err) = mv(&[&link, &moved]);
        assert!(ok, "{err}");
        assert!(
            fs::symlink_metadata(&moved)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::symlink_metadata(&link).is_err());
    }

    /// Bug 4's unit: the cross-device fallback must reproduce a symlink *as* a
    /// symlink. `fs::rename` would not have gone through here, so the fallback
    /// is called directly — there is no portable way to make two filesystems
    /// appear in a unit test.
    #[test]
    #[cfg(unix)]
    fn the_cross_device_fallback_relinks_rather_than_copying_the_target() {
        let dir = scratch("xdev_symlink");
        let real = dir.path("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.path("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let moved = dir.path("moved");

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
    }

    #[test]
    fn the_cross_device_fallback_moves_a_plain_file() {
        let dir = scratch("xdev_file");
        let a = dir.path("a");
        let b = dir.path("b");
        fs::write(&a, b"bytes").unwrap();
        let meta = fs::symlink_metadata(&a).unwrap();
        copy_across_devices(&a, &b, &meta).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read(&b).unwrap(), b"bytes");
    }

    /// Not implemented, and it says so rather than moving part of the tree.
    #[test]
    fn the_cross_device_fallback_refuses_a_directory() {
        let dir = scratch("xdev_dir");
        let sub = dir.path("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("inside"), b"x").unwrap();
        let meta = fs::symlink_metadata(&sub).unwrap();
        let e = copy_across_devices(&sub, &dir.path("elsewhere"), &meta).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
        assert!(sub.join("inside").is_file(), "nothing may be moved");
    }

    /// A file whose name is not valid UTF-8 — the case the whole rewrite is
    /// about — must move like any other.
    #[test]
    #[cfg(unix)]
    fn moves_a_file_whose_name_is_not_utf8() {
        use std::os::unix::ffi::OsStringExt;
        let dir = scratch("nonutf8");
        let mut name = dir.dir().to_path_buf().into_os_string().into_vec();
        name.extend_from_slice(b"/\x80bad");
        let src = PathBuf::from(OsString::from_vec(name));
        fs::write(&src, b"x").unwrap();
        let dst = dir.path("ok");
        let (ok, err) = mv(&[&src, &dst]);
        assert!(ok, "{err}");
        assert!(!src.exists());
        assert_eq!(fs::read(&dst).unwrap(), b"x");
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
