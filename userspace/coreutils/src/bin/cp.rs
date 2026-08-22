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

use coreutils::getopt::{self, Program, Takes};
use coreutils::quote::quoteaf_os;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

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

fn main() -> ExitCode {
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
            let mut err = io::stderr().lock();
            if copy_all(&flags, &paths, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            eprintln!("cp: {e}");
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
    // `is_dir` follows symlinks, which is right here: `cp a link-to-dir/` puts
    // `a` inside the directory, as GNU does without `-T`.
    let dest_is_dir = dest_path.is_dir();

    if sources.len() > 1 && !dest_is_dir {
        let _ = writeln!(err, "cp: target {} is not a directory", quoteaf_os(dest));
        return false;
    }

    let mut ok = true;
    for src in sources {
        if !copy_one(flags, src, dest_path, dest_is_dir, err) {
            ok = false;
        }
    }
    ok
}

/// Copy one source. Returns `false` if it should count against the exit status.
fn copy_one<W: Write>(
    flags: &CpFlags,
    src: &OsString,
    dest: &Path,
    dest_is_dir: bool,
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
            let _ = writeln!(err, "cp: cannot stat {}: {e}", quoteaf_os(src));
            return false;
        }
    };

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

    if metadata.file_type().is_symlink() {
        // Only reachable under `-r`; see the stat above.
        return match clone_symlink(src_path, &target) {
            Ok(()) => true,
            Err(e) => {
                let _ = writeln!(
                    err,
                    "cp: cannot create symbolic link {}: {e}",
                    quoteaf_os(&target)
                );
                false
            }
        };
    }

    if !metadata.is_dir() {
        return match fs::copy(src_path, &target) {
            Ok(_) => true,
            Err(e) => {
                let _ = writeln!(
                    err,
                    "cp: cannot copy {} to {}: {e}",
                    quoteaf_os(src),
                    quoteaf_os(&target)
                );
                false
            }
        };
    }

    if !flags.recursive {
        let _ = writeln!(
            err,
            "cp: -r not specified; omitting directory {}",
            quoteaf_os(src)
        );
        return false;
    }

    // Module docs, bug 2: without this, `cp -r a a` and `cp -r a .` copy what
    // they have just written, for ever.
    if is_inside(&target, src_path) {
        let _ = writeln!(
            err,
            "cp: cannot copy a directory, {}, into itself, {}",
            quoteaf_os(src),
            quoteaf_os(&target)
        );
        return false;
    }

    copy_tree(src_path, &target, err)
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
        None => Err("the source path ends in '.', '..' or '/', so it names nothing to create there"),
    }
}

/// Would writing at `target` write inside `root`?
///
/// Both are resolved as far as they exist — `target` usually does not exist yet,
/// so its nearest existing ancestor is canonicalised and the rest appended. That
/// makes `cp -r a .` (target `./a`) and `cp -r a a` (target `a/a`) both
/// recognisable as the same directory reached by a different spelling, which a
/// textual comparison would miss.
fn is_inside(target: &Path, root: &Path) -> bool {
    match (resolve_as_far_as_exists(root), resolve_as_far_as_exists(target)) {
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

/// Copy the tree at `src` to `dest`, reporting every failure to `err`.
///
/// Returns `false` if anything failed. A failure on one entry does not abandon
/// the others — module docs, bug 6.
fn copy_tree<W: Write>(src: &Path, dest: &Path, err: &mut W) -> bool {
    if let Err(e) = fs::create_dir_all(dest) {
        let _ = writeln!(
            err,
            "cp: cannot create directory {}: {e}",
            quoteaf_os(dest)
        );
        return false;
    }

    let entries = match fs::read_dir(src) {
        Ok(e) => e,
        Err(e) => {
            let _ = writeln!(err, "cp: cannot read directory {}: {e}", quoteaf_os(src));
            return false;
        }
    };

    let mut ok = true;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                let _ = writeln!(err, "cp: cannot read directory {}: {e}", quoteaf_os(src));
                ok = false;
                continue;
            }
        };
        let from = entry.path();
        let to = dest.join(entry.file_name());

        // `DirEntry::file_type` does **not** follow symlinks, unlike
        // `Path::is_dir`. That is the whole of the fix for bug 1.
        let kind = match entry.file_type() {
            Ok(k) => k,
            Err(e) => {
                let _ = writeln!(err, "cp: cannot stat {}: {e}", quoteaf_os(&from));
                ok = false;
                continue;
            }
        };

        let outcome = if kind.is_symlink() {
            clone_symlink(&from, &to)
        } else if kind.is_dir() {
            if !copy_tree(&from, &to, err) {
                ok = false;
            }
            continue;
        } else {
            fs::copy(&from, &to).map(|_| ())
        };

        if let Err(e) = outcome {
            let _ = writeln!(
                err,
                "cp: cannot copy {} to {}: {e}",
                quoteaf_os(&from),
                quoteaf_os(&to)
            );
            ok = false;
        }
    }

    // Last, not first: a source mode like 0500 applied before the contents were
    // written would lock this process out of the directory it is filling.
    // Module docs, bug 3 — without this a 0700 source becomes a 0755 copy and
    // everything under it is published.
    if let Err(e) = carry_over_mode(src, dest) {
        let _ = writeln!(
            err,
            "cp: cannot set permissions on {}: {e}",
            quoteaf_os(dest)
        );
        ok = false;
    }
    ok
}

/// Give `dest` the permission bits of `src`.
#[cfg(unix)]
fn carry_over_mode(src: &Path, dest: &Path) -> io::Result<()> {
    let mode = fs::metadata(src)?.permissions();
    fs::set_permissions(dest, mode)
}

/// Windows has no mode bits to carry — `set_permissions` there only toggles the
/// read-only flag, and copying that onto a directory is not what POSIX is asking
/// for. The target OS is the `#[cfg(unix)]` branch above.
#[cfg(not(unix))]
fn carry_over_mode(_src: &Path, _dest: &Path) -> io::Result<()> {
    Ok(())
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
                e.sentence.contains(&format!("'{named}' is not implemented")),
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
        assert_eq!(fs::read_link(dst.join("sub/loop")).unwrap(), Path::new(".."));
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
