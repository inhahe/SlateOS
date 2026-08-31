//! cp — copy files and directories.
//!
//! # Why this was rewritten
//!
//! It read argv as `String`, so it *panicked* on a filename holding a byte that
//! is not valid UTF-8 — which on this OS is a legal filename, by design
//! (`design.txt`: a path may hold every byte but `/` and NUL). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `cp` is the
//! third of the 49 bins listed there, after `rm` and `mv`.
//!
//! Argv is now `OsString` and stays that way to the syscall. Options go through
//! [`coreutils::getopt`], which is byte-based.
//!
//! # Six further bugs, in the lines this rewrite replaced
//!
//! 1. **A symlink inside a recursive copy was followed, so `cp -r` could not
//!    terminate.** `copy_dir_recursive` asked `src_path.is_dir()`, which follows
//!    symlinks, and then `fs::copy`, which also follows. A directory containing
//!    a link to any of its own ancestors — `ln -s .. loop`, which is ordinary —
//!    made `cp -r` descend for ever, writing an ever-deepening tree until the
//!    disk filled or the path length gave out. Even without a loop, every
//!    symlink in the tree silently became a full copy of whatever it pointed at,
//!    so copying a tree of a hundred links to one big file produced a hundred
//!    big files. GNU's `-r` does not dereference; neither does this one now.
//!    `DirEntry::file_type` is the non-following call and is what the walk uses.
//!
//! 2. **`cp -r` would copy a directory into itself, without limit.** `cp -r a a`
//!    and `cp -r a .` both resolve the destination to a path *inside* the
//!    source, so the walk copied what it had just written, for ever. GNU refuses
//!    with `cannot copy a directory into itself`; so does this, after resolving
//!    both paths as far as they exist.
//!
//! 3. **A copied directory came out world-readable.** `fs::create_dir_all` makes
//!    a directory with the process umask's default mode, so `cp -r private dst`
//!    — where `private` is mode 0700 — produced a 0755 `dst`, publishing every
//!    file inside it. POSIX says the new directory takes the source's
//!    permission bits. The mode is now applied *after* the contents are copied,
//!    because applying a mode like 0500 first would lock out the copy itself.
//!    (Regular files were never affected: `fs::copy` carries the mode over.)
//!
//! 4. **`--` was not an end-of-options marker.** `cp -- -foo bar` answered
//!    `unknown option: --`, so a file whose name begins with a dash could not be
//!    copied at all.
//!
//! 5. **A source ending in `..` or `/` copied into the wrong place.** The target
//!    was `dest.join(src.file_name().unwrap_or_default())`, and `Path::file_name`
//!    is `None` for such a path — so `unwrap_or_default()` gave an *empty* name
//!    and `dest.join("")` collapsed back to `dest` itself. `cp -r a/.. dst` then
//!    emptied `a`'s parent *into* `dst` rather than into `dst/<name>`, silently
//!    merging it with whatever was already there. The old test suite asserted
//!    this behaviour (`target_source_with_no_filename_into_dir`), which is how it
//!    lasted; that test now asserts the refusal.
//!
//! 6. **One unreadable file abandoned the rest of the copy.** `copy_dir_recursive`
//!    propagated the first error with `?`, so a single permission denial part-way
//!    through a large tree stopped the walk, reported one message, and left a
//!    partial copy that looked complete to anything that did not check the exit
//!    status. Each entry is now attempted, each failure reported, and the worst
//!    outcome returned — which is what `cp` is specified to do and what makes the
//!    diagnostics worth reading.
//!
//! # A seventh, found later, by measurement rather than by reading
//!
//! 7. **`cp a a` emptied `a`, silently, and exited 0.** The destination is
//!    opened with `O_TRUNC` before the source is read, so naming one file
//!    twice truncated it to nothing and then copied the nothing back over
//!    itself. It said nothing and reported success, so a shell loop that did
//!    it could destroy a directory's worth of files without a single
//!    diagnostic. Every one of these reached it — `cp a a`, `cp a ./a`,
//!    `cp a dir/../a`, `cp a hard-link-to-a`, `cp a symlink-to-a`, and
//!    `cp -r a .` — because there is no string comparison that catches the
//!    last four, and GNU does not attempt one: it compares the two `stat`
//!    results, device and inode, and so does [`is_same_file`] now.
//!
//!    This one is worth separating from bugs 1–6 for a reason that has nothing
//!    to do with `cp`. Those six were found by *reading* the code it replaced.
//!    This was found by `scripts/cp-diff.sh` on its first run, against a file
//!    that had already been rewritten once with the defect in it — the rewrite
//!    swapped a hand-rolled walk for `fs::copy` and never asked what `fs::copy`
//!    does when handed one file twice. Reading finds the bugs you thought to
//!    look for. See `known-issues.md` ->
//!    `B-CP-COPYING-A-FILE-ONTO-ITSELF-EMPTIED-IT`.
//!
//! # An eighth, from the same harness: three wrong answers about permissions
//!
//! 8. **`fs::copy` gave the destination the source's mode, exactly, in every
//!    case.** That is wrong three separate ways, and two of them publish a file
//!    that was private:
//!
//!    * *A new file ignored the umask.* `fs::copy` creates the destination and
//!      then `chmod`s it to the source's mode, so a 0777 source produced a 0777
//!      copy. GNU passes the mode to `open` and lets the kernel subtract the
//!      umask, so under the ordinary 022 the copy is 0755. Measured, both ways,
//!      across three umasks — see [`mode_of_a_new_file_is_narrowed_by_umask`].
//!    * *An existing destination had its mode overwritten.* `cp public private`
//!      — a 0777 source over somebody's 0600 file — left that file 0777. GNU
//!      reopens an existing destination **without** a mode argument, so its
//!      permissions are not touched at all; only its contents are. This is the
//!      one that is a security bug rather than a cosmetic one, and no amount of
//!      reading the old code would have suggested looking for it, because the
//!      old code mentioned modes for files nowhere at all.
//!    * *A directory ignored the umask too, and had a window.* `cp -r` of a
//!      1777 directory produced 1777 where GNU produces 1755, and the copy was
//!      made group- and other-writable *before* its contents were written —
//!      a window in which anyone could add a file to a directory that is about
//!      to look like a faithful copy. [`copy_tree`] now does GNU's dance:
//!      withhold group/other write at `mkdir`, force owner-rwx on if the source
//!      lacked it, and put both back at the end, less the umask.
//!
//!    Bug 3 above was the same subject and did not go far enough: it noticed
//!    that a copied directory came out *wider* than its source and fixed that
//!    by copying the mode over verbatim, which is a different wrong answer. The
//!    lesson is bug 7's — a fix derived by reading is worth what the reading
//!    was worth, and only measurement says whether it was right.
//!
//! # Options this implementation does not have
//!
//! Everything except `-r`/`-R`/`--recursive`. They are recognised and rejected
//! with a message saying they are not implemented, rather than ignored, and they
//! are listed in [`LONG_OPTIONS`] anyway because the table is what decides
//! whether an abbreviation is ambiguous.
//!
//! Ignoring them would be worse than refusing in almost every case: `-n` asks
//! for an existing file to be left alone, `-p` asks for ownership and timestamps
//! to survive, `-l` and `-s` ask for a link rather than a copy, and `-P`/`-L`
//! choose whether a symlink or its target is copied. Every one of those, ignored,
//! produces a destination that looks right and is not.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quoteaf_os;
use coreutils::stdfd::{self, Stream};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// The file-mode creation mask. There is no read-only spelling of it in POSIX —
// reading it means setting it — and `std` exposes no wrapper, so this is the
// libc call itself. `mkdir.rs` declares it the same way for the same reason.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

/// `cp`'s usage status is 1, like almost every utility's; see
/// [`coreutils::getopt::Error`] for the two that differ and why.
const CP: Program = Program::new("cp", 1);

/// GNU `cp`'s `long_opts[]`, **in its declaration order**, which is observable:
/// `getopt_long` lists an ambiguous prefix's candidates in table order. Every
/// entry is here whether or not this implementation acts on it — see the module
/// docs for why leaving one out is a silent wrong answer rather than a missing
/// feature.
///
/// Measured with `cp --=x`, which an empty prefix makes print the whole table.
/// It once also held `("keep-directory-symlink", …)`, which is a `tar` option
/// and has never been a `cp` one; `scripts/getopt-ambiguity-check.py` now
/// compares this list against that readout on every run, because the same
/// mistake had independently reached `mv` as `--exchange`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("archive", Takes::Nothing),
    ("attributes-only", Takes::Nothing),
    ("backup", Takes::Optional),
    ("copy-contents", Takes::Nothing),
    ("debug", Takes::Nothing),
    ("dereference", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("link", Takes::Nothing),
    ("no-clobber", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-preserve", Takes::Required),
    ("no-target-directory", Takes::Nothing),
    ("one-file-system", Takes::Nothing),
    ("parents", Takes::Nothing),
    // Deprecated upstream but still in the table. It is the *same option* as
    // `--parents` — same `val` in GNU's `struct option` — which is why it is
    // named in [`ALIASES`] below and why `cp --pa` resolves rather than being
    // ambiguous. An earlier revision of this file asserted the opposite in a
    // comment here; measuring it (`cp --pa=1` answers `option '--parents'
    // doesn't allow an argument`) settled it the other way.
    ("path", Takes::Nothing),
    ("preserve", Takes::Optional),
    ("recursive", Takes::Nothing),
    ("remove-destination", Takes::Nothing),
    ("sparse", Takes::Required),
    ("reflink", Takes::Optional),
    ("strip-trailing-slashes", Takes::Nothing),
    ("suffix", Takes::Required),
    ("symbolic-link", Takes::Nothing),
    ("target-directory", Takes::Required),
    ("update", Takes::Optional),
    ("verbose", Takes::Nothing),
    ("context", Takes::Optional),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The one pair of spellings in [`LONG_OPTIONS`] that name a single option.
///
/// See [`Program::resolve_long_aliased`]: without this, `--path` would count as
/// a second candidate for the prefix `--pa` and make `--parents` impossible to
/// abbreviate — which GNU allows. It does **not** make `--p` unambiguous, and a
/// test below pins that: `--p` still matches `--preserve`, which is a genuinely
/// different option.
const ALIASES: &[(&str, &str)] = &[("path", "parents")];

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct CpFlags {
    recursive: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order. The last operand is the
    /// destination.
    Run(CpFlags, Vec<OsString>),
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
            println!("cp (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, paths)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            if copy_all(&flags, &paths, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("cp: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: cp [OPTION]... SOURCE DEST
  or:  cp [OPTION]... SOURCE... DIRECTORY
Copy SOURCE to DEST, or multiple SOURCE(s) to DIRECTORY.

  -r, -R, --recursive   copy directories recursively.  Symbolic links are
                          copied as symbolic links, not followed.
      --help            display this help and exit
      --version         output version information and exit

To copy a file whose name starts with a '-', for example '-foo',
use one of these commands:
  cp -- -foo bar
  cp ./-foo bar
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `cp`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `cp a -r b` is `cp -r a b` — which
/// is `getopt_long`'s default permuting behaviour and what the previous
/// hand-written parser did too.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, or
/// a long option given a value it does not take.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = CpFlags::default();
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
            // A lone `-` is a file called `-`. `cp` has no standard-input
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
    flags: &mut CpFlags,
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
    let typed = std::str::from_utf8(typed).map_err(|_| CP.unrecognized_option(whole))?;
    let (name, takes) = CP.resolve_long_aliased(typed, whole, LONG_OPTIONS, ALIASES)?;

    if inline.is_some() && takes == Takes::Nothing {
        return Err(CP.long_unwanted_argument(name));
    }

    match name {
        "help" => Ok(Some(Request::Help)),
        "version" => Ok(Some(Request::Version)),
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
/// A byte that is no option of `cp`'s, or one this implementation lacks.
fn apply_short(flag: u8, flags: &mut CpFlags) -> Result<(), getopt::Error> {
    match flag {
        b'r' | b'R' => flags.recursive = true,
        // GNU `cp`'s remaining short options. Rejected rather than ignored: see
        // the module docs — every one of these, ignored, produces a destination
        // that looks right and is not.
        b'a' | b'b' | b'd' | b'f' | b'H' | b'i' | b'l' | b'L' | b'n' | b'p' | b'P' | b's'
        | b'S' | b't' | b'T' | b'u' | b'v' | b'x' | b'Z' => return Err(unimplemented_short(flag)),
        other => return Err(CP.invalid_option(other)),
    }
    Ok(())
}

/// The diagnostic for an option that GNU `cp` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-p` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    CP.usage_referring(format!(
        "option -{} is not implemented by this cp",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    CP.usage_referring(format!("option '--{name}' is not implemented by this cp"))
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

// ---------------------------------------------------------------- copying ---

/// Copy every source onto the destination, reporting failures to `err`.
///
/// Returns `true` if everything asked for was copied. Takes the error sink as a
/// parameter rather than writing to `stderr` directly so the diagnostics — the
/// part of `cp` a caller actually sees when something goes wrong — can be
/// asserted on in tests. The old file had no test of this path at all, which is
/// how bugs 1–3 and 6 in the module docs survived.
fn copy_all<W: Write>(flags: &CpFlags, paths: &[OsString], err: &mut W) -> bool {
    // Zero and one operand are distinct diagnostics, as in GNU. "missing
    // operand" alone left the user to work out *which*.
    let Some((dest, sources)) = paths.split_last() else {
        let _ = writeln!(
            err,
            "cp: {}",
            CP.usage_referring("missing file operand".into())
        );
        return false;
    };
    if sources.is_empty() {
        let _ = writeln!(
            err,
            "cp: {}",
            CP.usage_referring(format!(
                "missing destination file operand after {}",
                quoteaf_os(dest)
            ))
        );
        return false;
    }

    let dest_path = Path::new(dest);
    // The destination is followed, which is right here: `cp a link-to-dir/`
    // puts `a` inside the directory, as GNU does without `-T`.
    let not_a_dir = dest_directory_error(dest_path);
    let dest_is_dir = not_a_dir.is_none();

    // GNU reports *why* the last operand is not a directory, and the two
    // reasons read differently: `cp a b nosuch` says "No such file or
    // directory" while `cp a b afile` says "Not a directory". One fixed
    // sentence for both loses the distinction that tells a user whether they
    // mistyped the name or forgot to make the directory.
    if sources.len() > 1
        && let Some(e) = not_a_dir
    {
        let why = strerror(&e);
        let _ = writeln!(err, "cp: target {}: {why}", quoteaf_os(dest));
        return false;
    }

    // Both "named twice" problems need two operands to exist at all, so GNU
    // builds the tables only in that case and this follows it — not to save the
    // allocation, but because the tables also decide whether a *repeat* is
    // possible, and with one source it never is.
    let mut seen = (sources.len() > 1).then(Seen::default);

    let mut ok = true;
    for src in sources {
        if !copy_one(flags, src, dest_path, dest_is_dir, seen.as_mut(), err) {
            ok = false;
        }
    }
    ok
}

/// `None` if `dest` is a directory, otherwise the failure that says why not.
///
/// GNU asks this by *opening* the operand with `O_DIRECTORY` and keeping the
/// errno. Asking `stat` gives the same two answers — `ENOENT` for a name that
/// is not there, `ENOTDIR` for one that is something else — without needing
/// `O_PATH`, which is a Linux extension the target does not have. The case the
/// two could part company on is a directory that can be stat'd but not
/// searched; `O_PATH` opens that successfully and so does `stat`, so they
/// agree there too.
fn dest_directory_error(dest: &Path) -> Option<io::Error> {
    match fs::metadata(dest) {
        Ok(m) if m.is_dir() => None,
        Ok(_) => Some(io::Error::from(io::ErrorKind::NotADirectory)),
        Err(e) => Some(e),
    }
}

/// What this command has already copied, and where it put it.
///
/// Three of GNU's refusals need it, and all three exist to stop one operand
/// destroying the result of an earlier one in the same command — `cp a
/// other/a d` would otherwise leave `d/a` holding `other/a`, and the copy of
/// `a` the user asked for would be gone with nothing said. GNU keeps two hash
/// tables for this (`copy.c`'s `src_info` and `dest_info`, plus the
/// `remember_copied` table); the three fields below are the same information.
///
/// Only *command-line* sources go in. A file reached by recursing into a
/// directory cannot be named twice on one command line, so recording it would
/// be work spent on a question that cannot arise.
#[derive(Default)]
struct Seen {
    /// Non-directory sources already copied. Keyed on the file's identity
    /// *and* the entry that named it, which is GNU's `triple_compare`: `cp a
    /// ./a d` is one file named twice, while `cp a hard-link-to-a d` is two
    /// entries that happen to share an inode and is a legitimate request.
    sources: HashSet<(FileId, EntryId)>,
    /// Directory sources already copied, and which entry each was written to.
    /// The destination is part of the answer here and not for files, because
    /// GNU's directory rule asks a different question — see [`copy_one`].
    dirs: HashMap<FileId, EntryId>,
    /// Destinations this command created, by path *and* identity. Both halves
    /// are needed: the path is what a later operand would collide with, and
    /// the identity is what says the thing at that path is still the one we
    /// made.
    dests: HashSet<(PathBuf, FileId)>,
}

impl Seen {
    /// Records a non-directory source and reports whether it was already
    /// there. Recorded even when the copy goes on to fail, which is GNU's
    /// behaviour and the reason `cp f f d` with `d/f` a directory reports the
    /// refusal once and the repeat once rather than the refusal twice.
    fn saw_source(&mut self, id: FileId, entry: EntryId) -> bool {
        !self.sources.insert((id, entry))
    }

    /// Whether `target` is a destination this command created and which is
    /// still the same file.
    fn made(&self, target: &Path, id: FileId) -> bool {
        self.dests.contains(&(target.to_path_buf(), id))
    }

    /// Remember a destination just written. `lstat`, as GNU does: what is
    /// wanted is the identity of the *entry*, so that a symbolic link just
    /// created is recognised as itself rather than as whatever it points at.
    fn record_dest(&mut self, target: &Path) {
        if let Ok(m) = fs::symlink_metadata(target)
            && let Some(id) = file_id(target, &m)
        {
            self.dests.insert((target.to_path_buf(), id));
        }
    }
}

/// Copy one source. Returns `false` if it should count against the exit status.
fn copy_one<W: Write>(
    flags: &CpFlags,
    src: &OsString,
    dest: &Path,
    dest_is_dir: bool,
    mut seen: Option<&mut Seen>,
    err: &mut W,
) -> bool {
    let src_path = Path::new(src);

    // Whether a symlink *operand* is followed depends on `-r`, and this matches
    // GNU: plain `cp link dst` copies what the link points at, while `cp -r`
    // copies the link itself. The recursive case must not follow, because a
    // followed link is what turns a link to an ancestor into an endless descent
    // (module docs, bug 1).
    let metadata = if flags.recursive {
        fs::symlink_metadata(src_path)
    } else {
        fs::metadata(src_path)
    };
    let metadata = match metadata {
        Ok(m) => m,
        Err(e) => {
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`] — on a Windows *host*
            // `{e}` says `The system cannot find the file specified. (os error
            // 2)`, which is neither POSIX's wording nor what this utility prints
            // on the target it ships on.
            let why = strerror(&e);
            let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(src));
            return false;
        }
    };

    // Before anything is worked out about the destination, as in GNU: the
    // refusal is a fact about the source alone, and asking it here is what
    // makes `cp tree/.. dst` say which of its two problems came first.
    if metadata.is_dir() && !flags.recursive {
        let _ = writeln!(
            err,
            "cp: -r not specified; omitting directory {}",
            quoteaf_os(src)
        );
        return false;
    }

    // A non-directory named twice is asked about here, before the destination
    // is worked out at all, and GNU asks it in the same place: `cp f f d` where
    // `d/f` is a directory prints the refusal for the first `f` and this
    // warning for the second, which only happens if the source is recorded
    // even when its copy failed. The repeat is *not* an error — the user asked
    // for a file that is already there, and it is.
    //
    // Identity, not spelling: `cp a ./a d` is the same request twice. But two
    // hard links to one inode are two entries, and copying both is a
    // legitimate thing to ask for, so [`same_entry`] separates them.
    if !metadata.is_dir()
        && let Some(seen) = seen.as_deref_mut()
        && let Some(id) = file_id(src_path, &metadata)
        && let Some(entry) = entry_id(src_path)
        && seen.saw_source(id, entry)
    {
        let _ = writeln!(
            err,
            "cp: warning: source file {} specified more than once",
            quoteaf_os(src)
        );
        return true;
    }

    let target = match compute_target(src_path, dest, dest_is_dir) {
        Ok(t) => t,
        Err(reason) => {
            let _ = writeln!(
                err,
                "cp: cannot copy {} into {}: {reason}",
                quoteaf_os(src),
                quoteaf_os(dest)
            );
            return false;
        }
    };

    // GNU stats the destination here, and a failure that is *not* "it isn't
    // there" ends this operand rather than being rediscovered later while
    // opening it: `cp a b/c` where `b` is a file says `cannot stat 'b/c'`, not
    // `cannot create regular file 'b/c'`.
    //
    // Which stat is used follows GNU as well. A regular file can be written
    // *through* a symlink, so a regular source looks at what the destination
    // resolves to; a directory or a symlink cannot be, so those look at the
    // destination name itself. That distinction is what makes `cp a
    // dangling-link` reach the O_EXCL path in [`create_destination`] and be
    // refused there, rather than being taken for an existing file here.
    let use_lstat = metadata.is_dir() || metadata.file_type().is_symlink();
    let dest_stat = if use_lstat {
        fs::symlink_metadata(&target)
    } else {
        fs::metadata(&target)
    };
    let dest_meta = match dest_stat {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(&target));
            return false;
        }
    };

    if let Some(dest_meta) = &dest_meta {
        // Module docs, bug 7. `stat` results rather than strings, which is the
        // only comparison that catches every spelling; GNU's `same_file_ok`
        // makes the same one, at the same point in the same order. Asked only
        // when the destination exists, again as GNU asks it.
        if is_same_file(src_path, &target, flags.recursive) {
            let _ = writeln!(
                err,
                "cp: {} and {} are the same file",
                quoteaf_os(src),
                quoteaf_os(&target)
            );
            return false;
        }

        // Neither kind can be put where the other is. Without these two the
        // walk would go on and fail somewhere less informative — a directory
        // source would report `cannot create directory … File exists` about a
        // name that is not a directory at all.
        if metadata.is_dir() && !dest_meta.is_dir() {
            let _ = writeln!(
                err,
                "cp: cannot overwrite non-directory {} with directory {}",
                quoteaf_os(&target),
                quoteaf_os(src)
            );
            return false;
        }
        // After the refusal above and before the one below, which is where GNU
        // asks it — inside the "destination is not a directory" arm.
        //
        // This is the one that stops `cp a other/a d` silently throwing away
        // the copy of `a` it made a moment ago. Nothing about the two operands
        // is wrong on its own; what is wrong is the pair, and only a record of
        // what this command already wrote can see it.
        if !dest_meta.is_dir()
            && let Some(seen) = seen.as_deref_mut()
            && let Some(id) = file_id(&target, dest_meta)
            && seen.made(&target, id)
        {
            let _ = writeln!(
                err,
                "cp: will not overwrite just-created {} with {}",
                quoteaf_os(&target),
                quoteaf_os(src)
            );
            return false;
        }

        if !metadata.is_dir() && dest_meta.is_dir() {
            let _ = writeln!(
                err,
                "cp: cannot overwrite directory {} with non-directory",
                quoteaf_os(&target)
            );
            return false;
        }
    }

    // The same guard again, for the case the one above cannot see. A regular
    // source stats its destination *followed*, so when that destination is a
    // symlink this command just created, the identity compared above is the
    // link's target rather than the link — and writing through it would
    // clobber whatever the link points at. GNU asks this separately for the
    // same reason, with its own `lstat`.
    if let Some(seen) = seen.as_deref_mut()
        && let Ok(link_meta) = fs::symlink_metadata(&target)
        && link_meta.file_type().is_symlink()
        && let Some(id) = file_id(&target, &link_meta)
        && seen.made(&target, id)
    {
        let _ = writeln!(
            err,
            "cp: will not copy {} through just-created symlink {}",
            quoteaf_os(src),
            quoteaf_os(&target)
        );
        return false;
    }

    // A directory named twice is asked about here, not up with the file case:
    // GNU reaches it only after the two refusals above, so `cp -r t t d` with
    // `d/t` a plain file reports "cannot overwrite non-directory" for *both*
    // operands rather than warning about the second.
    //
    // And the question asked is a different one. Two operands naming one
    // directory are a repeat only if they were also going to the same place;
    // where they are not, GNU refuses with a message about hard links instead,
    // which this `cp` has no equivalent of yet.
    if metadata.is_dir()
        && let Some(seen) = seen.as_deref_mut()
        && let Some(id) = file_id(src_path, &metadata)
        && let Some(entry) = entry_id(&target)
    {
        match seen.dirs.get(&id) {
            Some(earlier) if *earlier == entry => {
                let _ = writeln!(
                    err,
                    "cp: warning: source directory {} specified more than once",
                    quoteaf_os(src)
                );
                return true;
            }
            Some(_) => {}
            None => {
                seen.dirs.insert(id, entry);
            }
        }
    }

    let ok = place_source(src, src_path, &metadata, &target, dest_meta.is_some(), err);

    // One recording site, reached however the copy was done, and only on
    // success — a destination that was never written is not one a later
    // operand can be accused of overwriting. GNU records in the same single
    // place and under the same condition.
    if ok && let Some(seen) = seen {
        seen.record_dest(&target);
    }
    ok
}

/// Make the copy, now that the destination path is settled and every refusal
/// has been made. Split out of [`copy_one`] so that it has one place to record
/// what it created rather than one per kind of source.
fn place_source<W: Write>(
    src: &OsString,
    src_path: &Path,
    metadata: &fs::Metadata,
    target: &Path,
    dest_exists: bool,
    err: &mut W,
) -> bool {
    if metadata.file_type().is_symlink() {
        // Only reachable under `-r`; see the stat in [`copy_one`].
        //
        // An existing destination is removed first. `symlinkat` has no
        // "replace", and refusing instead would leave `cp -r` unable to update
        // a tree it had already copied once — so GNU unlinks, under exactly
        // this condition (`copy.c`: `dereference == DEREF_NEVER` and the source
        // is not a regular file).
        if dest_exists
            && let Err(e) = fs::remove_file(target)
            && e.kind() != io::ErrorKind::NotFound
        {
            let why = strerror(&e);
            let _ = writeln!(err, "cp: cannot remove {}: {why}", quoteaf_os(target));
            return false;
        }
        return match clone_symlink(src_path, target) {
            Ok(()) => true,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(
                    err,
                    "cp: cannot create symbolic link {}: {why}",
                    quoteaf_os(target)
                );
                false
            }
        };
    }

    if !metadata.is_dir() {
        return copy_regular_file(src_path, metadata, target, err);
    }

    // Module docs, bug 2: without this, `cp -r a a` and `cp -r a .` copy what
    // they have just written, for ever.
    if is_inside(target, src_path) {
        let _ = writeln!(
            err,
            "cp: cannot copy a directory, {}, into itself, {}",
            quoteaf_os(src),
            quoteaf_os(target)
        );
        return false;
    }

    copy_tree(src_path, permission_bits(metadata), target, err)
}

/// Would copying `src` to `dst` write over `src` itself?
///
/// Both are *followed*, so a destination that is a symlink to the source counts
/// — writing through it truncates the source exactly as surely as naming the
/// source directly. The one exception is GNU's, and it is the reason `recursive`
/// is a parameter: under `-r`, two names that are both symlinks are the same
/// file only when they are the same *link*, because replacing one link with a
/// copy of another does not touch what either points at. `cp -r linkA linkB`
/// where both point at one file is therefore allowed, while `cp -r link file`
/// — where `link` resolves to `file` — is not, and GNU makes exactly that
/// distinction in `same_file_ok`.
///
/// `false` when either side cannot be stat'd. A source that is a dangling
/// symlink is not the same file as anything, and a destination that cannot be
/// reached will produce its own diagnostic a moment later.
#[cfg(unix)]
fn is_same_file(src: &Path, dst: &Path, recursive: bool) -> bool {
    use std::os::unix::fs::MetadataExt;
    fn same(a: &fs::Metadata, b: &fs::Metadata) -> bool {
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    if recursive {
        let (sl, dl) = (fs::symlink_metadata(src), fs::symlink_metadata(dst));
        if let (Ok(sl), Ok(dl)) = (&sl, &dl)
            && sl.file_type().is_symlink()
            && dl.file_type().is_symlink()
        {
            return same(sl, dl);
        }
    }
    match (fs::metadata(src), fs::metadata(dst)) {
        (Ok(a), Ok(b)) => same(&a, &b),
        _ => false,
    }
}

/// Windows exposes a file's identity only through `windows_by_handle`, which is
/// unstable, so the development host compares resolved paths instead. That
/// still catches a repeated operand and a `.` or `..` in the middle of one; it
/// misses a hard link, which is why the guarantee is stated on the
/// `#[cfg(unix)]` arm above — the arm the target OS and the certification
/// harness both use. The unit tests that pin the refusal run on both.
#[cfg(not(unix))]
fn is_same_file(src: &Path, dst: &Path, _recursive: bool) -> bool {
    match (fs::canonicalize(src), fs::canonicalize(dst)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Where one source lands.
///
/// # Errors
///
/// The source having no file-name component while the destination is a
/// directory — `cp a/.. dst`. See module docs, bug 5: the previous code turned
/// this into a request to merge `a`'s parent *into* `dst`.
fn compute_target(src: &Path, dest: &Path, dest_is_dir: bool) -> Result<PathBuf, &'static str> {
    if !dest_is_dir {
        return Ok(dest.to_path_buf());
    }
    match src.file_name() {
        Some(name) => Ok(dest.join(name)),
        None => {
            Err("the source path ends in '.', '..' or '/', so it names nothing to create there")
        }
    }
}

/// Would writing at `target` write inside `root`?
///
/// Both are resolved as far as they exist — `target` usually does not exist yet,
/// so its nearest existing ancestor is canonicalised and the rest appended. That
/// makes `cp -r a .` (target `./a`) and `cp -r a a` (target `a/a`) both
/// recognisable as the same directory reached by a different spelling, which a
/// textual comparison would miss.
/// What tells one file from another, for [`Seen`]'s three questions.
///
/// The `(device, inode)` pair, which is the only answer that survives the file
/// being reached by a different name — and reaching it by a different name is
/// exactly what the three questions are about.
#[cfg(unix)]
type FileId = (u64, u64);

/// The portable stand-in: a host with no inode numbers has no cheaper answer
/// than the resolved path. It agrees with the pair above on every question
/// except hard links, which such a host does not have either.
#[cfg(not(unix))]
type FileId = PathBuf;

#[cfg(unix)]
fn file_id(_path: &Path, meta: &fs::Metadata) -> Option<FileId> {
    use std::os::unix::fs::MetadataExt;
    Some((meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn file_id(path: &Path, _meta: &fs::Metadata) -> Option<FileId> {
    fs::canonicalize(path).ok()
}

/// What tells one *directory entry* from another: which directory it is in,
/// and the final component of the name.
///
/// Distinct from [`FileId`], and both are needed. Two hard links to one file
/// share a `FileId` and have different `EntryId`s, and `cp a hard-a d` is a
/// request for two copies rather than a repeat — GNU's `same_nameat` draws the
/// line in the same place. Conversely `a` and `./a` are two spellings of one
/// entry, and `cp a ./a d` is a repeat.
type EntryId = (FileId, OsString);

/// The entry `path` names, or `None` if the directory holding it cannot be
/// identified. `None` means "cannot answer", and every caller treats that as
/// "not the same entry" — the same conclusion GNU reaches when its `stat` of
/// the parent fails.
fn entry_id(path: &Path) -> Option<EntryId> {
    let (dir, name) = split_entry(path);
    let meta = fs::metadata(&dir).ok()?;
    Some((file_id(&dir, &meta)?, name))
}

/// A path's directory and final component, GNU's `dir_name`/`base_name` pair.
///
/// Trailing slashes belong to neither — `tree/` names the entry `tree` — and a
/// path with no slash at all names an entry in the current directory. Done on
/// the bytes rather than through `Path::file_name`, which answers `None` for a
/// name ending in `.` or `..` and so would make `cp -r a/. b/.. d` look like
/// one entry named twice.
#[cfg(unix)]
fn split_entry(path: &Path) -> (PathBuf, OsString) {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    // Everything after the last byte that is not a separator is decoration.
    let end = bytes
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(bytes.len(), |i| i + 1);
    let head = bytes.get(..end).unwrap_or(bytes);
    match head.iter().rposition(|&b| b == b'/') {
        Some(cut) => {
            // An empty directory half means the name was rooted: `/etc` is the
            // entry `etc` in `/`, not in the current directory.
            let dir = head.get(..cut).filter(|d| !d.is_empty()).unwrap_or(b"/");
            let name = head.get(cut.saturating_add(1)..).unwrap_or_default();
            (
                PathBuf::from(OsStr::from_bytes(dir)),
                OsStr::from_bytes(name).to_os_string(),
            )
        }
        None => (PathBuf::from("."), OsStr::from_bytes(head).to_os_string()),
    }
}

/// The same split for the only non-POSIX host this builds on, Windows, where
/// it exists so that `cargo test` on the development machine exercises the
/// same code shape rather than a weaker stand-in. `OsStr` is not bytes there,
/// so the units are UTF-16 and both separators count.
#[cfg(not(unix))]
fn split_entry(path: &Path) -> (PathBuf, OsString) {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    // `b'/' as u16` in a pattern position is not const-evaluable, and the two
    // code units are fixed by ASCII, so they are written out.
    const SLASH: u16 = 0x2F;
    const BACKSLASH: u16 = 0x5C;
    let sep = |c: u16| c == SLASH || c == BACKSLASH;

    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let end = wide
        .iter()
        .rposition(|&c| !sep(c))
        .map_or(wide.len(), |i| i + 1);
    let head = wide.get(..end).unwrap_or(&wide);
    match head.iter().rposition(|&c| sep(c)) {
        Some(cut) => {
            // An empty directory half means the name was rooted, and the
            // separator itself is then the directory.
            let dir = if cut == 0 {
                head.get(..1)
            } else {
                head.get(..cut)
            };
            let name = head.get(cut.saturating_add(1)..).unwrap_or_default();
            (
                PathBuf::from(OsString::from_wide(dir.unwrap_or_default())),
                OsString::from_wide(name),
            )
        }
        None => (PathBuf::from("."), OsString::from_wide(head)),
    }
}

fn is_inside(target: &Path, root: &Path) -> bool {
    match (
        resolve_as_far_as_exists(root),
        resolve_as_far_as_exists(target),
    ) {
        (Some(root), Some(target)) => target.starts_with(&root),
        // If neither can be resolved at all there is nothing useful to say, and
        // refusing a copy on the strength of a failed lookup would be worse than
        // the loop this guards against is likely. The `read_dir` walk still
        // terminates on any real tree; only a self-copy loops.
        _ => false,
    }
}

/// `canonicalize`, but tolerating a path that does not exist yet: the longest
/// existing prefix is canonicalised and the remaining components appended.
fn resolve_as_far_as_exists(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = fs::canonicalize(path) {
        return Some(real);
    }
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut here = path;
    loop {
        let name = here.file_name()?;
        tail.push(name);
        here = here.parent()?;
        // An empty parent means a bare relative name: resolve against the
        // current directory, which is what the kernel would do with it.
        let base = if here.as_os_str().is_empty() {
            fs::canonicalize(Path::new(".")).ok()?
        } else if let Ok(real) = fs::canonicalize(here) {
            real
        } else {
            continue;
        };
        let mut out = base;
        for name in tail.iter().rev() {
            out.push(name);
        }
        return Some(out);
    }
}

/// Copy the tree at `src`, whose permission bits are `src_mode`, to `dest`,
/// reporting every failure to `err`.
///
/// Returns `false` if anything failed. A failure on one entry does not abandon
/// the others — module docs, bug 6.
///
/// The mode is taken as an argument rather than re-stat'd because the caller
/// has already stat'd `src` and a second look could see a different directory.
fn copy_tree<W: Write>(src: &Path, src_mode: u32, dest: &Path, err: &mut W) -> bool {
    let mut ok = true;

    // Group- and other-write are *withheld* at `mkdir` and put back at the end,
    // so that there is no window in which the directory exists, is writable by
    // someone else, and is not yet filled. That is GNU's `omitted_permissions`
    // (`copy.c`), and it is the reason a copy is not simply `mkdir(src_mode)`.
    let mut omitted = src_mode & 0o022;

    // The second adjustment, in the opposite direction: a source that is not
    // owner-rwx — 0500 is perfectly ordinary — would leave this process unable
    // to fill the directory it has just made. So owner-rwx goes on now and the
    // real mode goes back at the end. `dst_mode` is what to go back *to*.
    let mut dst_mode = 0;
    let mut restore = false;

    match make_dir(dest, src_mode & !omitted) {
        Ok(true) => match permission_bits_of(dest) {
            Ok(made) => {
                if made & 0o700 != 0o700 {
                    dst_mode = made;
                    restore = true;
                    if let Err(e) = set_mode(dest, made | 0o700) {
                        let why = strerror(&e);
                        let _ = writeln!(
                            err,
                            "cp: setting permissions for {}: {why}",
                            quoteaf_os(dest)
                        );
                        return false;
                    }
                }
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(dest));
                return false;
            }
        },
        Ok(false) => {
            // The destination directory was already there. GNU leaves its mode
            // alone — exactly as it leaves an existing *file*'s mode alone — so
            // there is nothing to withhold and nothing to put back.
            omitted = 0;
        }
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                err,
                "cp: cannot create directory {}: {why}",
                quoteaf_os(dest)
            );
            return false;
        }
    }

    // An unreadable source directory is *not* a reason to leave the copy
    // early: the directory has already been created, and it must still end up
    // with the mode it is supposed to have. GNU carries the mode over in this
    // case too, and a `dst` left at the forced owner-rwx would be a copy of a
    // 0500 directory that anyone could write into.
    match fs::read_dir(src) {
        Ok(entries) => {
            for entry in entries {
                if !copy_entry(&entry, src, dest, err) {
                    ok = false;
                }
            }
        }
        Err(e) => {
            // GNU's wording, and it is the only one it has for this: `copy_dir`
            // slurps the whole directory with `savedir` and reports every way
            // that can fail as `cannot access`. "cannot read directory" would
            // be the more precise sentence and is what `rm` prints, but a
            // utility that differs from GNU only in the words of a diagnostic
            // is still a utility whose output a script cannot match on.
            let why = strerror(&e);
            let _ = writeln!(err, "cp: cannot access {}: {why}", quoteaf_os(src));
            ok = false;
        }
    }

    // What was withheld goes back on, less the umask — which is the subtraction
    // the kernel would have done had `mkdir` been handed the mode outright, and
    // is why a 1777 source produces a 1755 copy under the ordinary 022. Skipping
    // it would publish group-write on every copy of a 0775 directory made by a
    // process whose umask says otherwise.
    omitted &= !cached_umask();
    if omitted != 0 && !restore {
        // Deducing the mode the directory actually got is not worth attempting
        // — `mkdir` applies implementation-defined rules to the special bits —
        // so it is read back. GNU says the same in the same place.
        match permission_bits_of(dest) {
            Ok(now) => {
                dst_mode = now;
                if omitted & !now != 0 {
                    restore = true;
                }
            }
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(dest));
                ok = false;
            }
        }
    }
    if restore && let Err(e) = set_mode(dest, dst_mode | omitted) {
        let why = strerror(&e);
        let _ = writeln!(
            err,
            "cp: preserving permissions for {}: {why}",
            quoteaf_os(dest)
        );
        ok = false;
    }
    ok
}

/// One entry of a directory being walked. Split out of [`copy_tree`] only to
/// keep the mode bookkeeping either side of the walk readable in one screen.
fn copy_entry<W: Write>(
    entry: &io::Result<fs::DirEntry>,
    src: &Path,
    dest: &Path,
    err: &mut W,
) -> bool {
    let entry = match entry {
        Ok(e) => e,
        Err(e) => {
            // The source directory is what could not be read, so it is what is
            // named — this said `dest` before, which is a directory that was
            // created successfully a moment earlier and had nothing to do with
            // the failure. Same sentence as the whole-directory failure in
            // [`copy_tree`]: GNU reads a directory in one go, so it has no
            // separate wording for giving up part-way through.
            let why = strerror(e);
            let _ = writeln!(err, "cp: cannot access {}: {why}", quoteaf_os(src));
            return false;
        }
    };
    let from = entry.path();
    let to = dest.join(entry.file_name());

    // `DirEntry::metadata` does **not** follow symlinks, unlike `Path::is_dir`.
    // That is the whole of the fix for bug 1, and it also hands over the mode
    // the copy is to be created with, which a second `stat` might not.
    let meta = match entry.metadata() {
        Ok(m) => m,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(err, "cp: cannot stat {}: {why}", quoteaf_os(&from));
            return false;
        }
    };

    if meta.file_type().is_symlink() {
        return match clone_symlink(&from, &to) {
            Ok(()) => true,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(
                    err,
                    "cp: cannot create symbolic link {}: {why}",
                    quoteaf_os(&to)
                );
                false
            }
        };
    }
    if meta.is_dir() {
        return copy_tree(&from, permission_bits(&meta), &to, err);
    }
    copy_regular_file(&from, &meta, &to, err)
}

/// Create `dest` as a directory with mode `mode`, before the umask is applied.
///
/// `Ok(true)` if it was created, `Ok(false)` if a directory was already there —
/// a distinction the caller needs, because an existing directory's mode is left
/// alone. Plain `create_dir` and not `create_dir_all`: GNU's single `mkdirat`
/// does not invent missing parents either, and `cp -r a no/such/dir` must fail
/// rather than quietly build the path.
fn make_dir(dest: &Path, mode: u32) -> io::Result<bool> {
    match create_dir_with_mode(dest, mode) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            // Only "already there" if it is a *directory*. A regular file under
            // that name is a failure, and reporting it as one is what stops the
            // walk from writing a directory's contents into whatever it found.
            if fs::metadata(dest).is_ok_and(|m| m.is_dir()) {
                Ok(false)
            } else {
                Err(e)
            }
        }
        Err(e) => Err(e),
    }
}

/// Copy the regular file `src` to `dst`.
///
/// This does by hand what `fs::copy` does in one call, for two reasons that are
/// not about speed:
///
/// * **`fs::copy` reports four different failures as one error.** The source
///   not opening, the destination not being creatable, a read fault and a write
///   fault all arrive as a single `io::Error` with nothing to say which
///   happened. GNU has a different sentence for each, and which sentence is
///   printed is the difference between knowing that a disk is full and knowing
///   that a file is unreadable.
/// * **`fs::copy` ends by giving the destination the source's exact mode.**
///   That is wrong twice: it ignores the umask on a file it has just created,
///   so a 0777 source lands as 0777 where GNU lands it as 0755; and it
///   overwrites the mode of a destination that *already existed*, so copying a
///   0777 file over somebody's 0600 one published it. See module docs, bug 8.
fn copy_regular_file<W: Write>(
    src: &Path,
    src_meta: &fs::Metadata,
    dst: &Path,
    err: &mut W,
) -> bool {
    let mut input = match fs::File::open(src) {
        Ok(f) => f,
        Err(e) => {
            let why = strerror(&e);
            let _ = writeln!(
                err,
                "cp: cannot open {} for reading: {why}",
                quoteaf_os(src)
            );
            return false;
        }
    };

    let mut output = match create_destination(src_meta, dst) {
        Ok(f) => f,
        Err(DestError::Dangling) => {
            let _ = writeln!(
                err,
                "cp: not writing through dangling symlink {}",
                quoteaf_os(dst)
            );
            return false;
        }
        Err(DestError::Io(e)) => {
            let why = strerror(&e);
            let _ = writeln!(
                err,
                "cp: cannot create regular file {}: {why}",
                quoteaf_os(dst)
            );
            return false;
        }
    };

    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match input.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // A signal arriving mid-read is not a read failure, and reporting
            // it as one would make `cp` unreliable under any job control.
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                let why = strerror(&e);
                let _ = writeln!(err, "cp: error reading {}: {why}", quoteaf_os(src));
                return false;
            }
        };
        let Some(chunk) = buf.get(..n) else {
            // Unreachable: `read` returns at most the buffer's length. Handled
            // rather than indexed so the crate's `indexing_slicing` lint has
            // nothing to complain about and a broken `Read` cannot panic here.
            break;
        };
        if let Err(e) = output.write_all(chunk) {
            let why = strerror(&e);
            let _ = writeln!(err, "cp: error writing {}: {why}", quoteaf_os(dst));
            return false;
        }
    }
    true
}

/// Why a destination could not be opened for writing.
enum DestError {
    Io(io::Error),
    /// The name is a symlink that points at nothing. Resolving it to a
    /// (directory, name) pair to write through is racy by construction, so GNU
    /// refuses and says so rather than creating the link's target.
    Dangling,
}

/// Open `dst` for writing, creating it with the source's mode if it is new and
/// leaving its mode entirely alone if it is not.
///
/// The order is GNU's: try `O_CREAT|O_EXCL` with the mode, and treat `EEXIST`
/// as "it was already there, reopen it without a mode". That is what keeps the
/// umask in the kernel's hands for a new file — the only place it can be
/// applied without a window in which the file exists at the wider mode — and
/// what leaves an existing file's permissions untouched.
fn create_destination(src_meta: &fs::Metadata, dst: &Path) -> Result<fs::File, DestError> {
    match open_new(dst, permission_bits(src_meta)) {
        Ok(f) => return Ok(f),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(DestError::Io(e)),
    }

    // `symlink_metadata` sees the link itself; `metadata` follows it, so
    // failing there is exactly "points at nothing".
    if fs::symlink_metadata(dst).is_ok_and(|m| m.file_type().is_symlink())
        && fs::metadata(dst).is_err()
    {
        return Err(DestError::Dangling);
    }

    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(dst)
        .map_err(DestError::Io)
}

/// `O_WRONLY|O_CREAT|O_EXCL` with `mode`, which the kernel narrows by the umask.
#[cfg(unix)]
fn open_new(dst: &Path, mode: u32) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(dst)
}

/// The development host has no mode to give, so the file is created with
/// whatever Windows would have given it. The target OS is the `#[cfg(unix)]`
/// arm above; see [`permission_bits`].
#[cfg(not(unix))]
fn open_new(dst: &Path, _mode: u32) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dst)
}

/// The permission and special bits — `07777` — of `meta`.
#[cfg(unix)]
fn permission_bits(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode() & 0o7777
}

/// Windows has no mode bits. `0o777` rather than `0` so that the arithmetic in
/// [`copy_tree`] — withhold group/other write, put it back if the directory did
/// not get it — cancels out to no change at all, which is the right answer on a
/// host where every `chmod` is a no-op anyway.
#[cfg(not(unix))]
fn permission_bits(_meta: &fs::Metadata) -> u32 {
    0o777
}

/// `permission_bits` of the name `path`, without following a final symlink.
fn permission_bits_of(path: &Path) -> io::Result<u32> {
    fs::symlink_metadata(path).map(|m| permission_bits(&m))
}

/// `mkdir(path, mode)`; the kernel narrows `mode` by the umask.
#[cfg(unix)]
fn create_dir_with_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new().mode(mode).create(path)
}

/// See [`open_new`]'s non-unix arm.
#[cfg(not(unix))]
fn create_dir_with_mode(path: &Path, _mode: u32) -> io::Result<()> {
    fs::create_dir(path)
}

/// `chmod(path, mode)`.
#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// `set_permissions` on Windows only toggles the read-only flag, which is not
/// what POSIX is asking for; doing nothing is the honest answer. The target OS
/// is the `#[cfg(unix)]` arm above.
#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// The process's file-mode creation mask.
///
/// There is no read-only spelling of it in POSIX — reading it means setting it
/// — so this sets it to deny everything and immediately puts the old value
/// back. That is GNU's `cached_umask` (`copy.c`) verbatim, and it is safe for
/// the reason it is safe there: `cp` is single-threaded and creates nothing
/// between the two calls, so nothing can observe the wider-denying mask.
#[cfg(unix)]
fn read_umask() -> u32 {
    // SAFETY: `umask` cannot fail, takes and returns a plain integer, and
    // touches no memory. Setting it back to the value just read leaves the
    // process's mask exactly as it was found.
    unsafe {
        let old = umask(0o777);
        umask(old);
        old
    }
}

/// [`read_umask`], remembered, as GNU remembers it: a deep `cp -r` should open
/// that momentary all-denying window once, not once per directory.
///
/// A real `cp` is one process with one umask for its whole life, so caching
/// changes no answer. The **test build does not cache** — `cargo test` runs
/// dozens of copies inside one process, and the mode tests set the umask around
/// each one, so a value remembered from the first would make every later row
/// assert against the wrong mask. That is the cache being wrong about the test
/// harness, not the tests being wrong about `cp`.
#[cfg(all(unix, not(test)))]
fn cached_umask() -> u32 {
    use std::sync::OnceLock;
    static CACHE: OnceLock<u32> = OnceLock::new();
    *CACHE.get_or_init(read_umask)
}

/// See the caching note above.
#[cfg(all(unix, test))]
fn cached_umask() -> u32 {
    read_umask()
}

/// Windows has no umask. Zero makes [`copy_tree`]'s subtraction a no-op.
#[cfg(not(unix))]
fn cached_umask() -> u32 {
    0
}

/// Reproduce the symlink at `src` as a symlink at `at`.
///
/// The link's *text* is copied verbatim, so a relative link keeps meaning
/// whatever it means relative to its new directory — which is what makes copying
/// a self-consistent tree of relative links produce another self-consistent
/// tree.
#[cfg(unix)]
fn clone_symlink(src: &Path, at: &Path) -> io::Result<()> {
    let points_at = fs::read_link(src)?;
    std::os::unix::fs::symlink(points_at, at)
}

/// Recreating a symlink needs a distinction between file and directory links on
/// Windows, and a privilege the test host does not necessarily have. Refusing is
/// the only answer that does not silently produce something other than a
/// symlink — and silently producing something else is precisely bug 1.
#[cfg(not(unix))]
fn clone_symlink(_src: &Path, _at: &Path) -> io::Result<()> {
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

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (CpFlags, Vec<String>) {
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
        assert!(!f.recursive);
        assert!(p.is_empty());
    }

    #[test]
    fn simple_copy() {
        let (f, p) = run_parse(&["a", "b"]);
        assert!(!f.recursive);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn dash_r_sets_recursive() {
        let (f, p) = run_parse(&["-r", "src", "dst"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["src", "dst"]);
    }

    #[test]
    fn capital_r_also_recursive() {
        assert!(run_parse(&["-R", "src", "dst"]).0.recursive);
        assert!(run_parse(&["-rR", "a", "b"]).0.recursive);
        assert!(run_parse(&["--recursive", "a", "b"]).0.recursive);
    }

    #[test]
    fn flag_may_follow_operands() {
        let (f, p) = run_parse(&["a", "b", "-r"]);
        assert!(f.recursive);
        assert_eq!(p, vec!["a", "b"]);
    }

    #[test]
    fn multiple_sources() {
        assert_eq!(run_parse(&["a", "b", "c", "d"]).1, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-", "dest"]).1, vec!["-", "dest"]);
    }

    /// Bug 4 in the module docs: this used to answer `unknown option: --`.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]).1, vec!["-foo", "bar"]);
        let (f, p) = run_parse(&["--", "-r"]);
        assert!(!f.recursive, "-r after -- is a filename, not a flag");
        assert_eq!(p, vec!["-r"]);
    }

    #[test]
    fn double_dash_alone_leaves_no_operands() {
        assert!(run_parse(&["--"]).1.is_empty());
    }

    #[test]
    fn long_options_abbreviate() {
        assert!(run_parse(&["--recur", "a", "b"]).0.recursive);
    }

    /// `--r` must stay ambiguous — `--recursive`, `--reflink` and
    /// `--remove-destination` all begin with it. This is the test that fails if
    /// someone prunes the table to what is actually handled.
    #[test]
    fn ambiguous_abbreviation_is_refused() {
        let e = fail(&["--r"]);
        assert!(e.sentence.contains("ambiguous"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--recursive"), "{:?}", e.sentence);
        assert!(e.sentence.contains("--reflink"), "{:?}", e.sentence);
    }

    /// `--pa` is **not** ambiguous, because `--path` and `--parents` are one
    /// option under two spellings. An earlier revision of this test asserted the
    /// opposite from recall; the measurement says otherwise:
    ///
    /// ```text
    /// $ cp --pa=1 a b
    /// cp: option '--parents' doesn't allow an argument
    /// ```
    ///
    /// which is `getopt_long` having resolved it, then complaining about the
    /// value. So the alias resolves, and the name it resolves to is the first
    /// *table* entry that matched — `--parents`, which precedes `--path` in
    /// `cp`'s table (it is the other way round in `rmdir`'s).
    ///
    /// Only `--pa` actually reaches both spellings; `--pat` is already past
    /// `--parents` and `--paren` already past `--path`. Each is listed with the
    /// name GNU answers with, measured the same way, because the interesting
    /// claim is not "it resolves" but *which* of the two it names:
    ///
    /// ```text
    /// --pa     cp: option '--parents' doesn't allow an argument
    /// --pat    cp: option '--path'    doesn't allow an argument
    /// --paren  cp: option '--parents' doesn't allow an argument
    /// ```
    #[test]
    fn the_deprecated_alias_does_not_make_its_own_option_ambiguous() {
        for (typed, named) in [
            ("--pa", "--parents"),
            ("--pat", "--path"),
            ("--paren", "--parents"),
        ] {
            let e = fail(&[typed, "a", "b"]);
            assert!(
                !e.sentence.contains("ambiguous"),
                "{typed}: {:?}",
                e.sentence
            );
            // It resolves, and is then refused for the separate reason that
            // this `cp` implements neither spelling.
            assert!(
                e.sentence
                    .contains(&format!("'{named}' is not implemented")),
                "{typed}: {:?}",
                e.sentence
            );
        }
    }

    /// The other half of the rule, and the half that a naive "hide the aliases"
    /// implementation gets wrong: `--p` matches `--parents`, `--path` **and**
    /// `--preserve`, and is ambiguous — but the message lists two, not three.
    /// Measured:
    ///
    /// ```text
    /// cp: option '--p' is ambiguous; possibilities: '--parents' '--preserve'
    /// ```
    #[test]
    fn a_prefix_that_reaches_past_the_alias_is_still_ambiguous() {
        assert_eq!(
            fail(&["--p", "a", "b"]).sentence,
            "option '--p' is ambiguous; possibilities: '--parents' '--preserve'"
        );
    }

    /// An exact alias resolves to itself, not to what it aliases — `getopt_long`
    /// returns the entry it matched.
    #[test]
    fn the_exact_alias_spelling_is_reported_as_typed() {
        assert!(
            fail(&["--path", "a", "b"])
                .sentence
                .contains("'--path' is not implemented"),
            "{:?}",
            fail(&["--path", "a", "b"]).sentence
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "a", "b"]);
        assert!(e.sentence.contains("invalid option"), "{:?}", e.sentence);
        assert!(e.sentence.contains('q'), "{:?}", e.sentence);
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

    /// Ignoring any of these would produce a destination that looks right and is
    /// not — `-p` silently drops permissions, `-n` silently overwrites, `-l` and
    /// `-s` silently copy instead of linking.
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        for flag in [
            "-a", "-b", "-d", "-f", "-H", "-i", "-l", "-L", "-n", "-p", "-P", "-s", "-S", "-t",
            "-T", "-u", "-v", "-x", "-Z",
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
            "--archive",
            "--attributes-only",
            "--backup",
            "--copy-contents",
            "--dereference",
            "--force",
            "--interactive",
            "--link",
            "--no-clobber",
            "--no-dereference",
            "--one-file-system",
            "--parents",
            "--preserve",
            "--remove-destination",
            "--strip-trailing-slashes",
            "--symbolic-link",
            "--update",
            "--verbose",
            // Given values inline so the option cannot swallow an operand and
            // turn a rejection test into an arity test.
            "--sparse=always",
            "--reflink=always",
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
        let e = fail(&["--recursive=yes", "a", "b"]);
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
        match parse_args(&[OsString::from("-r"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
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
        match parse_args(&[OsString::from("-r"), bad.clone(), OsString::from("d")]).unwrap() {
            Request::Run(f, p) => {
                assert!(f.recursive);
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
    fn target_dir_into_dir_appends_basename() {
        let t = compute_target(Path::new("src/sub"), Path::new("dst"), true).unwrap();
        assert_eq!(t, PathBuf::from("dst").join("sub"));
    }

    /// Bug 5 in the module docs. The old test here asserted the *broken*
    /// behaviour — that `dst.join("")`, i.e. `dst` itself, was the right answer —
    /// which is why the bug lasted. Merging `a`'s parent into `dst` is not what
    /// `cp -r a/.. dst` asks for.
    #[test]
    fn a_source_with_no_file_name_is_refused_not_collapsed() {
        for src in ["/", "..", "a/..", "."] {
            let e = compute_target(Path::new(src), Path::new("dst"), true).unwrap_err();
            assert!(e.contains("names nothing"), "{src}: {e}");
        }
    }

    #[test]
    fn a_source_with_no_file_name_is_fine_when_dest_is_not_a_dir() {
        let t = compute_target(Path::new("a/.."), Path::new("dst"), false).unwrap();
        assert_eq!(t, PathBuf::from("dst"));
    }

    // ------------------------------------------------------------ copying --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("cp_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const PLAIN: CpFlags = CpFlags { recursive: false };
    const RECURSIVE: CpFlags = CpFlags { recursive: true };

    /// `copy_all` plus whatever it wrote to its error sink.
    fn cp(flags: &CpFlags, paths: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = paths.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = copy_all(flags, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    #[test]
    fn copies_a_file() {
        let dir = scratch("file");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"hello").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&a).unwrap(), b"hello", "the source stays");
        assert_eq!(fs::read(&b).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_a_file_into_a_directory() {
        let dir = scratch("into_dir");
        let a = dir.join("a");
        let sub = dir.join("sub");
        fs::write(&a, b"x").unwrap();
        fs::create_dir(&sub).unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &sub]);
        assert!(ok, "{err}");
        assert!(sub.join("a").is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    // --------------------------------------------- which failure it reports --

    /// GNU names the errno rather than restating the option's requirement, and
    /// the two errnos read differently enough to matter: one says the name is
    /// missing, the other that it is the wrong kind of thing.
    #[test]
    fn the_target_diagnostic_names_the_reason() {
        let dir = scratch("target_why");
        let a = dir.join("a");
        let b = dir.join("b");
        let file = dir.join("plain");
        fs::write(&a, b"1").unwrap();
        fs::write(&b, b"2").unwrap();
        fs::write(&file, b"3").unwrap();

        let missing = dir.join("nosuch");
        let (ok, e) = cp(&PLAIN, &[&a, &b, &missing]);
        assert!(!ok);
        assert!(
            e.ends_with(": No such file or directory\n"),
            "a name that is not there: {e}"
        );

        let (ok, e) = cp(&PLAIN, &[&a, &b, &file]);
        assert!(!ok);
        assert!(
            e.ends_with(": Not a directory\n"),
            "a name that is something else: {e}"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// A destination whose *parent* is not a directory is a failed `stat`, not
    /// a failed create, and GNU says which. Reporting it at the create would
    /// name the wrong operation and, once `cp` grows `-i`, would do so after
    /// having already asked to overwrite something.
    #[cfg(unix)]
    #[test]
    fn a_destination_under_a_plain_file_fails_at_the_stat() {
        let dir = scratch("dst_stat");
        let a = dir.join("a");
        let blocking = dir.join("blocking");
        fs::write(&a, b"1").unwrap();
        fs::write(&blocking, b"2").unwrap();

        let under = blocking.join("under");
        let (ok, e) = cp(&PLAIN, &[&a, &under]);
        assert!(!ok);
        assert!(e.starts_with("cp: cannot stat "), "{e}");
        assert!(e.ends_with(": Not a directory\n"), "{e}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Under `-r` a symlink operand is copied as a link, so its own inode is
    /// what a naive comparison sees — and that inode is never the destination's.
    /// GNU resolves both sides unless *both* are links, which is what makes
    /// `cp -r link file` a refusal where `link` points at `file`.
    #[cfg(unix)]
    #[test]
    fn a_symlink_operand_resolving_to_the_destination_is_refused() {
        let dir = scratch("link_same");
        let file = dir.join("file");
        let link = dir.join("link");
        fs::write(&file, b"kept").unwrap();
        std::os::unix::fs::symlink("file", &link).unwrap();

        let (ok, e) = cp(&RECURSIVE, &[&link, &file]);
        assert!(!ok);
        assert!(e.contains("are the same file"), "{e}");
        assert_eq!(fs::read(&file).unwrap(), b"kept");
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The other half of that rule: two *distinct* links to one file are not
    /// the same file, because replacing one with a copy of the other leaves
    /// what they point at alone.
    #[cfg(unix)]
    #[test]
    fn two_symlinks_to_one_file_are_not_the_same_file() {
        let dir = scratch("two_links");
        let file = dir.join("file");
        let one = dir.join("one");
        let two = dir.join("two");
        fs::write(&file, b"kept").unwrap();
        std::os::unix::fs::symlink("file", &one).unwrap();
        std::os::unix::fs::symlink("file", &two).unwrap();

        let (ok, e) = cp(&RECURSIVE, &[&one, &two]);
        assert!(ok, "{e}");
        assert_eq!(fs::read(&file).unwrap(), b"kept", "the target is untouched");
        let _ = fs::remove_dir_all(&dir);
    }

    // ------------------------------------------- one operand against another --
    //
    // Three refusals that no single operand can be judged by: what is wrong is
    // the *pair*, and only a record of what the command already wrote can see
    // it. The data-loss one is `cp a other/a d` — without the guard, `d/a`
    // ends up holding `other/a` and the copy of `a` the user asked for is gone
    // with nothing printed.

    #[test]
    fn a_second_source_will_not_overwrite_the_copy_the_first_just_made() {
        let dir = scratch("just_created");
        let other = dir.join("other");
        let dest = dir.join("dest");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&dest).unwrap();
        let first = dir.join("f");
        let second = other.join("f");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();

        let (ok, e) = cp(&PLAIN, &[&first, &second, &dest]);
        assert!(!ok, "the pair must count against the exit status");
        // Not asserted against a whole quoted path: the scratch directory is
        // absolute and its spelling differs by host.
        assert!(e.contains("will not overwrite just-created"), "{e}");
        assert_eq!(
            fs::read(dest.join("f")).unwrap(),
            b"first",
            "the copy that was asked for first survives"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same guard, for the case the first cannot see. A regular source
    /// stats its destination *followed*, so a destination that is a symlink
    /// this command just made compares as whatever it points at; without a
    /// second look at the link itself the copy goes through it.
    #[cfg(unix)]
    #[test]
    fn a_second_source_will_not_be_written_through_a_just_created_symlink() {
        let dir = scratch("through_link");
        let other = dir.join("other");
        let dest = dir.join("dest");
        fs::create_dir(&other).unwrap();
        fs::create_dir(&dest).unwrap();
        let pointee = dir.join("pointee");
        fs::write(&pointee, b"untouched").unwrap();
        let link = dir.join("l");
        std::os::unix::fs::symlink(&pointee, &link).unwrap();
        let plain = other.join("l");
        fs::write(&plain, b"second").unwrap();

        let (ok, e) = cp(&RECURSIVE, &[&link, &plain, &dest]);
        assert!(!ok, "{e}");
        assert!(e.contains("through just-created symlink"), "{e}");
        assert_eq!(
            fs::read(&pointee).unwrap(),
            b"untouched",
            "what the link points at is not written to"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_source_named_twice_is_a_warning_and_not_a_failure() {
        let dir = scratch("named_twice");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.join("f");
        fs::write(&f, b"body").unwrap();
        let dotted = dir.join(".").join("f");

        let (ok, e) = cp(&PLAIN, &[&f, &dotted, &dest]);
        assert!(ok, "a repeat is not an error: {e}");
        assert!(e.contains("specified more than once"), "{e}");
        assert_eq!(fs::read(dest.join("f")).unwrap(), b"body");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The line the repeat rule draws. Two hard links share an inode but are
    /// two entries, and asking for a copy of each is a legitimate request —
    /// which is why the rule needs [`entry_id`] and not just [`file_id`].
    #[cfg(unix)]
    #[test]
    fn two_hard_links_to_one_file_are_not_one_source_named_twice() {
        let dir = scratch("two_hard");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let one = dir.join("one");
        let two = dir.join("two");
        fs::write(&one, b"body").unwrap();
        fs::hard_link(&one, &two).unwrap();

        let (ok, e) = cp(&PLAIN, &[&one, &two, &dest]);
        assert!(ok, "{e}");
        assert_eq!(e, "", "nothing to warn about");
        assert_eq!(fs::read(dest.join("one")).unwrap(), b"body");
        assert_eq!(fs::read(dest.join("two")).unwrap(), b"body");
        let _ = fs::remove_dir_all(&dir);
    }

    /// With one source there is no pair, so the tables are never built. This
    /// asserts the case that would otherwise be caught by them wrongly: a
    /// single source copied onto a destination the *previous* run made.
    #[test]
    fn a_lone_source_is_never_a_repeat_of_itself() {
        let dir = scratch("lone");
        let dest = dir.join("dest");
        fs::create_dir(&dest).unwrap();
        let f = dir.join("f");
        fs::write(&f, b"body").unwrap();

        assert!(cp(&PLAIN, &[&f, &dest]).0);
        let (ok, e) = cp(&PLAIN, &[&f, &dest]);
        assert!(ok, "{e}");
        assert_eq!(e, "");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_entry_keeps_dot_and_dotdot_apart() {
        assert_eq!(split_entry(Path::new("a/.")).1, OsString::from("."));
        assert_eq!(split_entry(Path::new("a/..")).1, OsString::from(".."));
        assert_eq!(split_entry(Path::new("a/b/")).1, OsString::from("b"));
        assert_eq!(
            split_entry(Path::new("b")),
            (PathBuf::from("."), "b".into())
        );
    }

    // ------------------------------------------------- modes, module docs 8 --
    //
    // These four run only on a POSIX host, because there is nothing on Windows
    // for them to assert about. `scripts/cp-diff.sh` is what certifies the same
    // behaviour against GNU itself; these exist so that a regression is caught
    // by `cargo test` on the target rather than only by a harness that needs a
    // GNU userland to run.

    /// Set the umask, run `f`, put the umask back.
    ///
    /// The umask is process-wide and `cargo test` runs tests on threads of one
    /// process, so two tests doing this at once would each see the other's
    /// mask. The lock makes them take turns. It does *not* protect against an
    /// unrelated test creating a file while a mask is installed — nothing can,
    /// short of running single-threaded — which is why these are the only tests
    /// in this file that assert a mode at all.
    #[cfg(unix)]
    fn with_umask<T>(mask: u32, f: impl FnOnce() -> T) -> T {
        use std::sync::Mutex;
        static TURN: Mutex<()> = Mutex::new(());
        // A poisoned lock means another umask test panicked; its `old` was
        // restored on unwind only if it got that far, so the mask may be
        // whatever it left. Proceeding is still the right call: the panic will
        // already be reported, and refusing here would hide this test's result
        // behind that one.
        let _guard = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: `umask` cannot fail, takes and returns a plain integer, and
        // touches no memory.
        let old = unsafe { umask(mask) };
        let out = f();
        // SAFETY: as above.
        unsafe { umask(old) };
        out
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }

    #[cfg(unix)]
    fn set_test_mode(p: &Path, m: u32) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(m)).unwrap();
    }

    /// A new destination is created with the source's mode *narrowed by the
    /// umask* — which is what the kernel does when the mode is passed to
    /// `open`, and what `fs::copy`'s trailing `chmod` undid.
    ///
    /// The expectations are measured, not derived: each row was produced by
    /// GNU `cp` under WSL before it was written down here.
    #[cfg(unix)]
    #[test]
    fn mode_of_a_new_file_is_narrowed_by_umask() {
        // (umask, source mode, what GNU produces)
        let rows: &[(u32, u32, u32)] = &[
            (0o022, 0o777, 0o755),
            (0o022, 0o600, 0o600),
            (0o000, 0o777, 0o777),
            (0o077, 0o777, 0o700),
            (0o077, 0o600, 0o600),
        ];
        let dir = scratch("file_mode");
        for (i, &(mask, src_mode, want)) in rows.iter().enumerate() {
            let a = dir.join(format!("a{i}"));
            let b = dir.join(format!("b{i}"));
            fs::write(&a, b"x").unwrap();
            set_test_mode(&a, src_mode);
            let (ok, err) = with_umask(mask, || cp(&PLAIN, &[&a, &b]));
            assert!(ok, "{err}");
            assert_eq!(mode_of(&b), want, "umask {mask:04o}, source {src_mode:04o}");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// The security half of module docs, bug 8: copying a wide file over a
    /// narrow one must not widen the narrow one. GNU reopens an existing
    /// destination with no mode argument at all.
    #[cfg(unix)]
    #[test]
    fn an_existing_destination_keeps_its_own_mode() {
        let dir = scratch("keep_mode");
        let a = dir.join("public");
        let b = dir.join("private");
        fs::write(&a, b"wide").unwrap();
        set_test_mode(&a, 0o777);
        fs::write(&b, b"narrow").unwrap();
        set_test_mode(&b, 0o600);

        let (ok, err) = cp(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(&b).unwrap(), b"wide", "contents are copied");
        assert_eq!(mode_of(&b), 0o600, "permissions are not");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A copied directory ends at `src & 07777 & ~umask`, sticky bit included
    /// — 1777 under 022 is 1755, which is what GNU produces and what the
    /// verbatim mode carry-over got wrong.
    #[cfg(unix)]
    #[test]
    fn mode_of_a_copied_directory_is_narrowed_by_umask() {
        let rows: &[(u32, u32, u32)] = &[
            (0o022, 0o777, 0o755),
            (0o022, 0o1777, 0o1755),
            (0o000, 0o1777, 0o1777),
            (0o077, 0o1777, 0o1700),
            (0o022, 0o700, 0o700),
        ];
        let dir = scratch("dir_mode");
        for (i, &(mask, src_mode, want)) in rows.iter().enumerate() {
            let a = dir.join(format!("s{i}"));
            let b = dir.join(format!("d{i}"));
            fs::create_dir(&a).unwrap();
            fs::write(a.join("inner"), b"x").unwrap();
            set_test_mode(&a, src_mode);
            let (ok, err) = with_umask(mask, || cp(&RECURSIVE, &[&a, &b]));
            assert!(ok, "{err}");
            assert_eq!(mode_of(&b), want, "umask {mask:04o}, source {src_mode:04o}");
            assert!(b.join("inner").is_file(), "and it was actually filled");
        }
        let _ = fs::remove_dir_all(&dir);
    }

    /// A source directory the copy cannot write into must still be filled: the
    /// mode goes on last, not first. 0500 is the case that mattered — it has no
    /// owner-write, so a copy that set the mode at `mkdir` time could not put
    /// anything inside what it had just made.
    #[cfg(unix)]
    #[test]
    fn a_read_only_source_directory_is_copied_whole_and_ends_read_only() {
        let dir = scratch("ro_dir");
        let a = dir.join("src");
        let b = dir.join("dst");
        fs::create_dir(&a).unwrap();
        fs::write(a.join("inner"), b"x").unwrap();
        set_test_mode(&a, 0o500);

        let (ok, err) = cp(&RECURSIVE, &[&a, &b]);
        assert!(ok, "{err}");
        assert!(b.join("inner").is_file(), "contents got in");
        assert_eq!(mode_of(&b), 0o500, "and the mode went on afterwards");

        // Leave both writable again so the scratch directory can be removed.
        set_test_mode(&a, 0o700);
        set_test_mode(&b, 0o700);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Module docs, bug 7. The assertion that matters is not the message but
    /// `fs::read`: the defect this pins reported success and said nothing, so a
    /// test that only checked the diagnostic would have passed against it.
    #[test]
    fn copying_a_file_onto_itself_is_refused_and_leaves_it_whole() {
        let dir = scratch("same_file");
        let a = dir.join("a");
        fs::write(&a, b"contents").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &a]);
        assert!(!ok, "should have been refused");
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"contents", "the file must survive");
        let _ = fs::remove_dir_all(&dir);
    }

    /// The same file reached by a second spelling. A string comparison of the
    /// two operands would let this one through, which is why there is not one.
    #[test]
    fn a_file_onto_itself_by_another_path_is_refused() {
        let dir = scratch("same_file_dotted");
        let a = dir.join("a");
        let sub = dir.join("sub");
        fs::write(&a, b"contents").unwrap();
        fs::create_dir(&sub).unwrap();
        let dotted = sub.join("..").join("a");
        let (ok, err) = cp(&PLAIN, &[&a, &dotted]);
        assert!(!ok, "should have been refused: {err}");
        assert!(err.contains("are the same file"), "{err}");
        assert_eq!(fs::read(&a).unwrap(), b"contents", "the file must survive");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A destination that merely *exists* is not the same file, and must still
    /// be overwritten. Without this the refusal above would be a way of
    /// breaking `cp` for every ordinary overwrite.
    #[test]
    fn an_existing_different_destination_is_still_overwritten() {
        let dir = scratch("same_file_neg");
        let a = dir.join("a");
        let b = dir.join("b");
        fs::write(&a, b"new").unwrap();
        fs::write(&b, b"old").unwrap();
        let (ok, err) = cp(&PLAIN, &[&a, &b]);
        assert!(ok, "{err}");
        assert_eq!(err, "");
        assert_eq!(fs::read(&b).unwrap(), b"new");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, err) = cp(&PLAIN, &[]);
        assert!(!ok);
        assert!(err.contains("missing file operand"), "{err}");
    }

    #[test]
    fn one_operand_names_it() {
        let (ok, err) = cp(&PLAIN, &[Path::new("solo")]);
        assert!(!ok);
        assert!(err.contains("missing destination file operand"), "{err}");
        assert!(err.contains("solo"), "{err}");
    }

    #[test]
    fn a_directory_needs_recursive() {
        let dir = scratch("needs_r");
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        let (ok, err) = cp(&PLAIN, &[&sub, &dir.join("copy")]);
        assert!(!ok);
        assert!(err.contains("omitting directory"), "{err}");
        assert!(!dir.join("copy").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copies_a_tree() {
        let dir = scratch("tree");
        let src = dir.join("src");
        fs::create_dir_all(src.join("deep/deeper")).unwrap();
        fs::write(src.join("top"), b"1").unwrap();
        fs::write(src.join("deep/mid"), b"2").unwrap();
        fs::write(src.join("deep/deeper/bottom"), b"3").unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dst.join("top")).unwrap(), b"1");
        assert_eq!(fs::read(dst.join("deep/mid")).unwrap(), b"2");
        assert_eq!(fs::read(dst.join("deep/deeper/bottom")).unwrap(), b"3");
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
        let (ok, err) = cp(&PLAIN, &[&a, &dir.join("gone"), &c, &sub]);
        assert!(!ok, "the missing source must count against the status");
        assert!(err.contains("gone"), "{err}");
        assert!(sub.join("a").is_file(), "the first source must still copy");
        assert!(
            sub.join("c").is_file(),
            "and so must the one after the error"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 2 in the module docs. Before the fix this filled the disk.
    #[test]
    fn refuses_to_copy_a_directory_into_itself() {
        let dir = scratch("into_itself");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("f"), b"x").unwrap();

        // `cp -r src src` — the target resolves to `src/src`.
        let (ok, err) = cp(&RECURSIVE, &[&src, &src]);
        assert!(!ok);
        assert!(err.contains("into itself"), "{err}");
        assert!(!src.join("src").exists());

        // `cp -r src src/nested` — the same thing spelled differently.
        let (ok, err) = cp(&RECURSIVE, &[&src, &src.join("nested")]);
        assert!(!ok, "{err}");
        assert!(err.contains("into itself"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 5, end to end: the source resolves to no name, so there is nothing to
    /// create inside the destination and the copy is refused rather than merged.
    #[test]
    fn a_dotdot_source_is_refused_rather_than_merged() {
        let dir = scratch("dotdot");
        let inner = dir.join("inner");
        let dst = dir.join("dst");
        fs::create_dir(&inner).unwrap();
        fs::create_dir(&dst).unwrap();
        fs::write(dir.join("sibling"), b"x").unwrap();

        let (ok, err) = cp(&RECURSIVE, &[&inner.join(".."), &dst]);
        assert!(!ok);
        assert!(err.contains("names nothing"), "{err}");
        assert!(
            !dst.join("sibling").exists(),
            "the parent's contents must not be merged into the destination"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 1 in the module docs, the non-terminating half. `loop` points at its
    /// own parent, so following it descends for ever. With `file_type()` the walk
    /// copies the link and stops.
    #[test]
    #[cfg(unix)]
    fn a_symlink_loop_in_the_tree_does_not_recurse_for_ever() {
        let dir = scratch("loop");
        let src = dir.join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("sub/f"), b"x").unwrap();
        std::os::unix::fs::symlink("..", src.join("sub/loop")).unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        assert_eq!(fs::read(dst.join("sub/f")).unwrap(), b"x");
        let link = fs::symlink_metadata(dst.join("sub/loop")).unwrap();
        assert!(
            link.file_type().is_symlink(),
            "the loop must arrive as a link, not as a copied subtree"
        );
        assert_eq!(
            fs::read_link(dst.join("sub/loop")).unwrap(),
            Path::new("..")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 1's other half: a link in the tree used to become a full copy of its
    /// target, so a tree of N links to one file produced N copies of that file.
    #[test]
    #[cfg(unix)]
    fn a_symlink_in_the_tree_is_copied_as_a_symlink() {
        let dir = scratch("tree_link");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("real"), b"contents").unwrap();
        std::os::unix::fs::symlink("real", src.join("link")).unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        let meta = fs::symlink_metadata(dst.join("link")).unwrap();
        assert!(meta.file_type().is_symlink(), "a link must stay a link");
        assert_eq!(fs::read_link(dst.join("link")).unwrap(), Path::new("real"));
        // And the relative link still resolves, in its new directory.
        assert_eq!(fs::read(dst.join("link")).unwrap(), b"contents");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Without `-r`, a symlink operand is followed — that is GNU's behaviour and
    /// it is unchanged. Only the recursive case stopped following.
    #[test]
    #[cfg(unix)]
    fn without_recursive_a_symlink_operand_is_still_followed() {
        let dir = scratch("deref");
        let real = dir.join("real");
        fs::write(&real, b"contents").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let out = dir.join("out");
        let (ok, err) = cp(&PLAIN, &[&link, &out]);
        assert!(ok, "{err}");
        let meta = fs::symlink_metadata(&out).unwrap();
        assert!(
            !meta.file_type().is_symlink(),
            "plain cp copies what the link points at"
        );
        assert_eq!(fs::read(&out).unwrap(), b"contents");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Bug 3 in the module docs: a 0700 source used to produce a 0755 copy,
    /// publishing everything inside it.
    #[test]
    #[cfg(unix)]
    fn a_copied_directory_keeps_the_source_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch("modes");
        let src = dir.join("private");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("secret"), b"x").unwrap();
        fs::set_permissions(&src, fs::Permissions::from_mode(0o700)).unwrap();

        let dst = dir.join("copy");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");
        let mode = fs::metadata(&dst).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "a private directory must stay private");
        let _ = fs::remove_dir_all(&dir);
    }

    /// A file whose name is not valid UTF-8 — the case the whole rewrite is
    /// about — must copy like any other, including through a recursive walk.
    #[test]
    #[cfg(unix)]
    fn copies_a_tree_holding_a_name_that_is_not_utf8() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let dir = scratch("nonutf8");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();

        let mut name = src.clone().into_os_string().into_vec();
        name.extend_from_slice(b"/\x80bad");
        let odd = PathBuf::from(OsString::from_vec(name));
        fs::write(&odd, b"x").unwrap();

        let dst = dir.join("dst");
        let (ok, err) = cp(&RECURSIVE, &[&src, &dst]);
        assert!(ok, "{err}");

        let mut want = dst.clone().into_os_string().into_vec();
        want.extend_from_slice(b"/\x80bad");
        let copied = PathBuf::from(OsString::from_vec(want));
        assert_eq!(fs::read(&copied).unwrap(), b"x");
        assert!(
            copied.as_os_str().as_bytes().ends_with(b"\x80bad"),
            "the name must survive byte for byte"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    // ---------------------------------------------------------- is_inside --

    #[test]
    fn is_inside_sees_through_a_different_spelling() {
        let dir = scratch("inside");
        let src = dir.join("src");
        fs::create_dir(&src).unwrap();

        assert!(is_inside(&src.join("child"), &src));
        assert!(is_inside(&src.join("a/b/c"), &src));
        // `src/./` and `src` are the same directory.
        assert!(is_inside(&src.join(".").join("child"), &src));
        assert!(!is_inside(&dir.join("sibling"), &src));
        // A sibling whose name merely starts with the source's is not inside it.
        assert!(!is_inside(&dir.join("srcery"), &src));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_as_far_as_exists_handles_a_path_that_is_not_there_yet() {
        let dir = scratch("resolve");
        let deep = dir.join("nope/not/here");
        let resolved = resolve_as_far_as_exists(&deep).unwrap();
        assert!(resolved.ends_with("nope/not/here"), "{resolved:?}");
        assert!(resolved.is_absolute(), "{resolved:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
