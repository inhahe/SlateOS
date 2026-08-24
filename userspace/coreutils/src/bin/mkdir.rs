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
//! Everything except `-p`/`--parents`, `-m`/`--mode` and `-v`/`--verbose` —
//! which leaves only `-Z`/`--context`, SELinux security contexts, for which
//! this system has no equivalent to report. It is recognised and rejected with
//! a message saying it is not implemented, rather than ignored, and it is
//! listed in [`LONG_OPTIONS`] anyway because the table is what decides whether
//! an abbreviation is ambiguous — `--v` must stay ambiguous between
//! `--verbose` and `--version`, which is measured GNU behaviour and is exactly
//! what a table pruned to the implemented options would get wrong: `mkdir --v`
//! would print a version instead of refusing.
//!
//! `-m`/`--mode` used to be in that list, and was the reason none of them are
//! ignored: it asks for the new directory to be created with a specific
//! permission mode, and the usual reason to ask is that the default is *too
//! permissive* — `mkdir -m 700 ~/.ssh`. Ignored, the directory would be created
//! 0755 and the user told nothing, so a directory they asked to be private would
//! be world-readable. Implementing it needs a symbolic-mode parser (`u=rwx,go=`),
//! which now exists as [`modechange`] rather than being private to `chmod.rs`;
//! see the [mode section](#mode) below for the four behaviours that had to be
//! measured to use it correctly here.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{quote_os, quoteaf_os};
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

// The file-mode creation mask. There is no read-only spelling of it in POSIX —
// reading it means setting it — and `std` exposes no wrapper, so this is the
// libc call itself.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
}

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

/// GNU `mkdir`'s `getopt_long` short-option string, verbatim. `m` is the only
/// one that takes a value.
const SHORT_OPTIONS: &str = "pm:vZ";

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MkdirFlags {
    parents: bool,
    /// `-v`: name each directory on **stdout** as it is created.
    ///
    /// Stdout, not stderr, and that is measured rather than assumed —
    /// `mkdir -v d 2>/dev/null | cat` shows the line and `mkdir -v d 1>/dev/null`
    /// shows nothing. It matters because it is the opposite of the utility's
    /// other message: a `-v` run that half fails writes the successes to one
    /// stream and the failure to the other, so a caller can keep the log and
    /// discard the noise, or the reverse.
    verbose: bool,
    /// `-m`'s argument **uncompiled**, because the order in which `mkdir`
    /// reports two different mistakes is observable and is the opposite of
    /// `chmod`'s. Measured: `mkdir -m zzz` with no operands answers `missing
    /// operand`, not `invalid mode` — so the mode cannot be compiled while the
    /// command line is still being read.
    mode: Option<OsString>,
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
            let mut out = io::stdout().lock();
            let mut err = io::stderr().lock();
            if make_all(&flags, &dirs, &mut out, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("mkdir: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: mkdir [OPTION]... DIRECTORY...
Create the DIRECTORY(ies), if they do not already exist.

  -m, --mode=MODE set file mode (as in chmod), not a=rwx - umask
  -p, --parents   no error if existing, make parent directories as needed
  -v, --verbose   print a message for each created directory
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

    // The shared driver, rather than the byte loop this file used to carry.
    // That loop was correct as far as it went, but it had no way to express an
    // option that takes a *value*, which is exactly what `-m` is: `-m 700`,
    // `-m700`, `--mode=700` and `--mode 700` are four spellings of one thing
    // and the driver already knows all four.
    for item in MKDIR.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'p', _) | Opt::Long("parents", _) => flags.parents = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            Opt::Short(b'm', value) | Opt::Long("mode", value) => flags.mode = value,
            // GNU `mkdir`'s remaining options. Rejected rather than ignored:
            // see the module docs.
            Opt::Short(flag @ b'Z', _) => return Err(unimplemented_short(flag)),
            Opt::Long(name @ "context", _) => {
                return Err(unimplemented_long(name));
            }
            Opt::Short(other, _) => return Err(MKDIR.invalid_option(other)),
            Opt::Long(other, _) => return Err(unimplemented_long(other)),
            // A lone `-` arrives here, not as an option: `mkdir` has no
            // standard-input operand for it to mean anything else, so it is a
            // directory called `-`.
            Opt::Operand(dir) => dirs.push(dir.clone()),
        }
    }

    Ok(Request::Run(flags, dirs))
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
    MKDIR.usage_referring(format!(
        "option '--{name}' is not implemented by this mkdir"
    ))
}

// -------------------------------------------------------------------- mode --

/// What `-m` resolved to: the mode for the directory the user named, and the
/// mode for any parent `-p` has to invent along the way.
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Modes {
    /// The requested mode, already through the umask, for the last component.
    named: u32,
    /// `(0777 & ~umask) | 0300` for every component `-p` creates on the way.
    ///
    /// Measured across six umasks, and the `| 0300` is not decoration: under
    /// `umask 300` — which strips the owner's write and search bits — GNU still
    /// creates the parents `0777` rather than `0477`, because a parent it
    /// cannot write to or descend into is a parent it cannot put the next
    /// component inside.
    parent: u32,
}

/// Read the file-mode creation mask **and zero it**, once per process.
///
/// Zeroing is GNU's: having applied the mask itself, in [`resolve_mode`], it
/// must stop the kernel applying it a second time to the same mode. Doing it
/// exactly once is this implementation's, and it is not a micro-optimisation —
/// the second call would read the zero the first one wrote and compute a parent
/// mode of `0777` for a user whose umask is `077`. The real binary calls
/// [`make_all`] once, so only the tests can make that happen; a cached value
/// makes it impossible either way.
#[cfg(unix)]
fn take_umask() -> u32 {
    use std::sync::OnceLock;
    static UMASK: OnceLock<u32> = OnceLock::new();
    // SAFETY: `umask` is a POSIX call that cannot fail and touches only this
    // process's file-mode creation mask. Reading it requires setting it, which
    // is why there is no read-only spelling; we want it zeroed anyway.
    *UMASK.get_or_init(|| unsafe { umask(0) })
}

/// [`take_umask`] on the target; `0` on a host that has no such thing.
///
/// A Windows host still compiles and runs everything else in this file, which
/// is the point: an invalid `-m` must be refused there with the right sentence,
/// and the arithmetic below is pure and so can be tested there against every
/// umask by calling [`modes_for`] directly.
#[cfg(unix)]
fn current_umask() -> u32 {
    take_umask()
}

#[cfg(not(unix))]
fn current_umask() -> u32 {
    0
}

/// Resolve `-m`'s argument against the umask in force.
fn resolve_mode(spec: &OsStr) -> Option<Modes> {
    modes_for(spec, current_umask())
}

/// The mode arithmetic, with the umask passed in rather than read.
///
/// The mode is compiled against a starting mode of `0777` — not `0755`, and not
/// the umask'd default. Measured: `mkdir -m 700 d` is `0700` under every umask,
/// and `mkdir -m 'a=,+w' d` is `0222`, `0200`, `0200` and `0220` under umasks
/// 000, 022, 077 and 002, so the umask still reaches a clause that names no
/// `who` — which is why it is handed to [`modechange::adjust`] rather than
/// merely being cleared. `dir` is `true`, so `+X` sees a directory: `mkdir -m
/// 'a=,+X' d` is `0111` where `mkfifo -m 'a=,+X' p` is `0000`.
fn modes_for(spec: &OsStr, umask_value: u32) -> Option<Modes> {
    let changes = modechange::compile(&coreutils::quote::os_bytes(spec))?;
    Some(Modes {
        named: modechange::adjust(0o777, true, umask_value, &changes).mode,
        parent: (0o777 & !umask_value) | 0o300,
    })
}

/// Create one directory, with an exact mode when `-m` asked for one.
///
/// The special bits need the second step. `mkdir(2)`'s mode argument is
/// advisory for `S_ISUID`/`S_ISGID`/`S_ISVTX` — the kernel may decline to store
/// them, and on a setgid parent it may add one nobody asked for — so a mode
/// carrying any of them is set again explicitly afterwards. Measured: `mkdir -m
/// 2755 d` is `2755`, and `mkdir -p -m 2755 a/b` leaves `a` at `755` and `a/b`
/// at `2755`.
#[cfg(unix)]
fn create_dir_with_mode(path: &Path, mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    let Some(mode) = mode else {
        return fs::create_dir(path);
    };
    fs::DirBuilder::new().mode(mode & 0o777).create(path)?;
    if mode & !0o777 != 0 {
        fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(mode))?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_dir_with_mode(path: &Path, _mode: Option<u32>) -> io::Result<()> {
    fs::create_dir(path)
}

/// A failure, paired with **the component that failed** rather than the operand
/// the user typed.
///
/// The distinction is measured, and `-p` is the only place it can be seen:
/// `mkdir -p f/g/h` where `f` is a file answers ``cannot create directory ‘f’:
/// Not a directory``, naming `f`. Returning a bare [`io::Error`] would leave the
/// caller with nothing to name but `f/g/h`, which is a path that was never
/// attempted and which sends the reader to look at the wrong component.
type WalkError = (std::path::PathBuf, io::Error);

/// `-p`'s walk, done by hand because each component needs a different mode.
///
/// [`fs::create_dir_all`] cannot express that, and with the umask zeroed it
/// would create every parent `0777`. Measured: `umask 022; mkdir -p -m 700
/// a/b/c` leaves `a` and `a/b` at `755` and only `a/b/c` at `700`.
///
/// A component that already exists is not an error and its mode is not touched
/// — measured: `mkdir -p x; chmod 755 x; mkdir -p -m 700 x` exits 0 and leaves
/// `x` at `0755`. So an existing directory is skipped rather than attempted:
/// that is also what keeps the walk off `/` and off a Windows drive root, which
/// [`Path::ancestors`] yields and which no `create_dir` should ever be handed.
///
/// Every component actually created is appended to `created`, in creation order,
/// which is what `-v` reports. Only components this call brought into being go
/// in: one that was skipped for already existing is not reported, measured —
/// `mkdir -p -v x/y/z` twice prints three lines and then none. `created` is
/// filled even when the walk goes on to fail, because the components made
/// before the failure really were made and GNU names them: `mkdir -p -v a/b`
/// where `a` can be created and `b` cannot still prints `a`.
fn create_dir_all_with_modes(
    path: &Path,
    modes: Option<Modes>,
    created: &mut Vec<std::path::PathBuf>,
) -> Result<(), WalkError> {
    let mut ancestors: Vec<&Path> = path.ancestors().collect();
    ancestors.reverse();
    let Some((named, parents)) = ancestors.split_last() else {
        return Ok(());
    };
    for parent in parents {
        // `ancestors` also yields `""` for a relative path, which is the current
        // directory said in the least useful way.
        if parent.as_os_str().is_empty() || parent.is_dir() {
            continue;
        }
        create_one(parent, modes.map(|m| m.parent), created)?;
    }
    match create_one(named, modes.map(|m| m.named), created) {
        // The one case `-p` exists to swallow, and only for a *directory*: GNU
        // still reports `File exists` for `mkdir -p f` where `f` is a file.
        Err((_, e)) if e.kind() == io::ErrorKind::AlreadyExists && named.is_dir() => Ok(()),
        other => other,
    }
}

/// [`create_dir_with_mode`], with the path attached to any error and the path
/// appended to `created` on success.
fn create_one(
    path: &Path,
    mode: Option<u32>,
    created: &mut Vec<std::path::PathBuf>,
) -> Result<(), WalkError> {
    match create_dir_with_mode(path, mode) {
        Ok(()) => {
            created.push(path.to_path_buf());
            Ok(())
        }
        Err(e) => Err((path.to_path_buf(), e)),
    }
}

// ---------------------------------------------------------------- creating --

/// Create every directory the command line asked for, reporting `-v` lines to
/// `out` and failures to `err`.
///
/// Returns `true` if every one was created. Takes both sinks as parameters
/// rather than writing to the real streams so the diagnostics — the part of
/// `mkdir` a caller actually sees when something goes wrong — can be asserted on
/// in tests. The old file had no test of this path at all.
///
/// Two sinks rather than one because GNU uses two: `-v` goes to **stdout** and
/// every error to **stderr**, so a half-failing run splits across them. A single
/// sink would test as one interleaved transcript and would hide a `-v` line sent
/// to the wrong stream, which is the mistake worth catching here — it is the
/// only line this utility writes to stdout at all.
///
/// One failure does not abandon the rest. Measured: `mkdir a g` with `a` already
/// present reports `a`, still creates `g`, and exits 1.
fn make_all<O: Write, W: Write>(
    flags: &MkdirFlags,
    dirs: &[OsString],
    out: &mut O,
    err: &mut W,
) -> bool {
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

    // *After* the operand check, and that order is measured, not incidental:
    // `mkdir -m zzz` with no operands answers `missing operand`, where `chmod
    // xyz` — which has the mode as an operand rather than an option — answers
    // `invalid mode`. Compiling the mode during parsing would reverse this one.
    let modes = match &flags.mode {
        None => None,
        Some(spec) => match resolve_mode(spec) {
            Some(m) => Some(m),
            None => {
                // Four utilities in this tree print four different sentences for
                // this, all measured against GNU 9.4: `chmod: invalid mode:
                // ‘zzz’` (with a colon), `install: invalid mode ‘zzz’`,
                // `mkfifo: invalid mode` (operand dropped entirely) and this
                // one. There is also no referral — `chmod`'s has none either.
                let _ = writeln!(err, "mkdir: invalid mode {}", quote_os(spec));
                return false;
            }
        },
    };

    let mut ok = true;
    let mut created: Vec<std::path::PathBuf> = Vec::new();
    for dir in dirs {
        created.clear();
        let result = if flags.parents {
            // Silent when the path is already a directory, which is `-p`'s whole
            // point, and still failing when it is an existing *file* — matching
            // GNU, which reports `File exists` for `mkdir -p f`.
            create_dir_all_with_modes(Path::new(dir), modes, &mut created)
        } else {
            create_one(Path::new(dir), modes.map(|m| m.named), &mut created)
        };
        // Before the error, not after: under `-p` the components made on the way
        // to a failure were still made, and GNU names them. Reported per operand
        // rather than once at the end so that a run over several operands
        // interleaves in the order the work happened.
        //
        // `quoteaf_os` here and `quote_os` below — straight marks for this line
        // and curly for the error, in the same run and the same loop body. That
        // pairing is the module docs' defect 2 made concrete; see the table
        // there before making the two agree.
        if flags.verbose {
            for path in &created {
                let _ = writeln!(
                    out,
                    "mkdir: created directory {}",
                    quoteaf_os(path.as_os_str())
                );
            }
        }
        // `failed` is the component that failed, which under `-p` need not be
        // the operand: see [`WalkError`].
        if let Err((failed, e)) = result {
            // `quote_os`, not `quoteaf_os`: curly marks. Module docs, defect 2 —
            // this one message is the exception among its neighbours, and the
            // table there is the evidence.
            //
            // `strerror`, not `{e}`: why it failed has to read the same
            // wherever it is printed. See [`coreutils::errmsg`] — on a Windows
            // *host* `{e}` says `The system cannot find the file specified.
            // (os error 2)`, which is neither POSIX's wording nor what this
            // utility prints on the target it ships on.
            let why = strerror(&e);
            let _ = writeln!(
                err,
                "mkdir: cannot create directory {}: {why}",
                quote_os(failed.as_os_str())
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
        // `--m` prefixes only `--mode`, which is implemented. Measured: `mkdir
        // --m 700 z` creates `z` at 0700.
        assert_eq!(
            run_parse(&["--m", "700", "z"]).0.mode,
            Some(OsString::from("700"))
        );
    }

    /// The two sentences glibc uses for a missing value, which are different
    /// from each other and are the library's rather than this file's. Measured:
    /// `mkdir --mode` and `mkdir -m` say these, each with the referral.
    #[test]
    fn the_mode_option_needs_a_value() {
        assert_eq!(
            fail(&["--mode"]).sentence,
            "option '--mode' requires an argument"
        );
        assert_eq!(fail(&["-m"]).sentence, "option requires an argument -- 'm'");
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

    /// `-Z` ignored would silently drop a security context. `-m` and `-v` used
    /// to be on this list — `-m` for the same reason, since ignored it would
    /// create world-readable the directory a user asked to be private — and
    /// both are now implemented; see [`the_four_spellings_of_the_mode_option`]
    /// and [`the_two_spellings_of_the_verbose_option`].
    #[test]
    fn unimplemented_short_options_are_rejected_by_name() {
        let e = fail(&["-Z", "a"]);
        assert!(e.sentence.contains("not implemented"), "{:?}", e.sentence);
    }

    #[test]
    fn the_two_spellings_of_the_verbose_option() {
        for spelling in [&["-v", "a"][..], &["--verbose", "a"][..]] {
            let (f, d) = run_parse(spelling);
            assert!(f.verbose, "{spelling:?}");
            assert_eq!(d, vec!["a"]);
        }
        // Bundled with the other short option, which is the spelling the old
        // parser could not read at all — see module docs, defect 1.
        let (f, d) = run_parse(&["-pv", "a"]);
        assert!(f.parents && f.verbose);
        assert_eq!(d, vec!["a"]);
        // And absent unless asked for.
        assert!(!run_parse(&["a"]).0.verbose);
    }

    #[test]
    fn unimplemented_long_options_are_rejected_by_name() {
        let e = fail(&["--context", "a"]);
        assert!(e.sentence.contains("not implemented"), "{:?}", e.sentence);
    }

    /// `-m 700`, `-m700`, `--mode=700` and `--mode 700` are one option, and the
    /// driver is what knows all four — which is the whole reason this file gave
    /// up its hand-written byte loop, since that loop had no way to express an
    /// option with a *value* at all.
    #[test]
    fn the_four_spellings_of_the_mode_option() {
        for spelling in [
            &["-m", "700", "a"][..],
            &["-m700", "a"][..],
            &["--mode=700", "a"][..],
            &["--mode", "700", "a"][..],
        ] {
            let (f, d) = run_parse(spelling);
            assert_eq!(f.mode, Some(OsString::from("700")), "{spelling:?}");
            assert_eq!(d, vec!["a"], "{spelling:?}");
        }
    }

    /// The mode is carried **uncompiled** out of the parser, because the order
    /// in which two mistakes are reported is observable. Measured: `mkdir -m
    /// zzz` with no operands answers `missing operand`, not `invalid mode` —
    /// the opposite of `chmod xyz`, which answers `missing operand after ‘xyz’`
    /// and so must have compiled nothing either. A parser that validated `-m`
    /// as it read it could not produce GNU's ordering here.
    #[test]
    fn an_invalid_mode_is_not_diagnosed_during_parsing() {
        let (f, d) = run_parse(&["-m", "zzz"]);
        assert_eq!(f.mode, Some(OsString::from("zzz")));
        assert!(d.is_empty());
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
        run_with_mode(parents, None, dirs)
    }

    fn run_with_mode(parents: bool, mode: Option<&str>, dirs: &[&Path]) -> (bool, String) {
        let (ok, out, err) = run_all(parents, false, mode, dirs);
        assert_eq!(out, "", "a run without -v must write nothing to stdout");
        (ok, err)
    }

    /// `make_all` with both streams captured separately, which is the only way
    /// to tell a `-v` line on stdout from one misdirected to stderr.
    fn run_all(
        parents: bool,
        verbose: bool,
        mode: Option<&str>,
        dirs: &[&Path],
    ) -> (bool, String, String) {
        let owned: Vec<OsString> = dirs.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let flags = MkdirFlags {
            parents,
            verbose,
            mode: mode.map(OsString::from),
        };
        let ok = make_all(&flags, &owned, &mut out, &mut err);
        (
            ok,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    /// The `-v` line for `path`, built the way the code builds it, so a test
    /// asserts on the *set and order* of reported paths without also restating
    /// how a path is quoted or hard-coding a separator this file does not choose.
    fn verbose_line(path: &Path) -> String {
        format!(
            "mkdir: created directory {}\n",
            quoteaf_os(path.as_os_str())
        )
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

    /// Under `-p`, GNU names **the component that failed**, not the operand.
    /// Measured: `touch f; mkdir -p f/g/h` answers ``cannot create directory
    /// ‘f’: Not a directory`` — and says nothing about `f/g/h`, which was never
    /// attempted. This is the assertion that fails if the walk ever goes back to
    /// returning a bare `io::Error`.
    #[test]
    fn dash_p_names_the_component_that_failed() {
        let d = scratch("component");
        let f = d.join("f");
        fs::write(&f, b"x").unwrap();
        let deep = f.join("g").join("h");
        let (ok, msg) = run(true, &[&deep]);
        assert!(!ok, "{msg}");
        // Compared through `quote_os`, the same rendering the message itself
        // uses: on the Windows host a path's backslashes come back escaped, so
        // the raw `Display` form is not a substring of the diagnostic.
        assert!(
            msg.contains(&coreutils::quote::quote_os(&f)),
            "must name f: {msg:?}"
        );
        assert!(
            !msg.contains(&coreutils::quote::quote_os(&deep)),
            "must not name the operand: {msg:?}"
        );
        let _ = fs::remove_dir_all(&d);
    }

    // ------------------------------------------------------------ verbose --

    /// The line itself, and the stream it goes to.
    ///
    /// Measured against GNU 9.4: `mkdir -v d 2>/dev/null | cat` shows the line,
    /// so it is stdout — the opposite of every other line this utility writes.
    #[test]
    fn verbose_names_each_directory_on_stdout() {
        let d = scratch("v_one");
        let a = d.join("a");
        let (ok, out, err) = run_all(false, true, None, &[&a]);
        assert!(ok, "{err}");
        assert_eq!(out, verbose_line(&a));
        assert_eq!(err, "", "the -v line must not reach stderr");
        let _ = fs::remove_dir_all(&d);
    }

    /// `-p` reports **every** component it creates, innermost last, and names
    /// each by the path leading to it rather than by its own last component.
    ///
    /// Measured: `mkdir -p -v x/y/z` prints `'x'`, `'x/y'`, `'x/y/z'` — three
    /// lines, growing. A version that reported only the operand would print one
    /// line, and one that reported basenames would print `'x'`, `'y'`, `'z'`,
    /// which reads as three siblings rather than a chain.
    #[test]
    fn verbose_under_parents_reports_every_component_it_creates() {
        let d = scratch("v_chain");
        let x = d.join("x");
        let y = x.join("y");
        let z = y.join("z");
        let (ok, out, err) = run_all(true, true, None, &[&z]);
        assert!(ok, "{err}");
        assert_eq!(
            out,
            format!(
                "{}{}{}",
                verbose_line(&x),
                verbose_line(&y),
                verbose_line(&z)
            )
        );

        // And a component that already existed is not reported: the second run
        // creates only `w`, so only `w` is named. This is what makes `-v` a log
        // of work done rather than of paths mentioned.
        let w = y.join("w");
        let (ok, out, err) = run_all(true, true, None, &[&w]);
        assert!(ok, "{err}");
        assert_eq!(out, verbose_line(&w));

        // Re-creating the whole chain reports nothing at all, and still succeeds.
        let (ok, out, err) = run_all(true, true, None, &[&z]);
        assert!(ok, "{err}");
        assert_eq!(out, "");
        let _ = fs::remove_dir_all(&d);
    }

    /// A run that half fails writes the successes to stdout and the failure to
    /// stderr, and keeps going.
    ///
    /// Measured: `mkdir -v ok1 /nope/nah ok2` prints `ok1` and `ok2` as created,
    /// reports `/nope/nah`, and exits 1. The two streams are the point — a
    /// caller can keep one and discard the other, which it could not do if the
    /// `-v` lines were mixed into stderr.
    #[test]
    fn verbose_splits_successes_and_failures_across_the_two_streams() {
        let d = scratch("v_split");
        let ok1 = d.join("ok1");
        let nope = d.join("absent").join("nah");
        let ok2 = d.join("ok2");
        let (ok, out, err) = run_all(false, true, None, &[&ok1, &nope, &ok2]);
        assert!(!ok, "a failed operand must still fail the run");
        assert_eq!(out, format!("{}{}", verbose_line(&ok1), verbose_line(&ok2)));
        assert!(
            err.contains(&coreutils::quote::quote_os(&nope)),
            "stderr must name the failure: {err:?}"
        );
        assert!(ok1.is_dir() && ok2.is_dir(), "the run must not abandon");
        let _ = fs::remove_dir_all(&d);
    }

    /// Under `-p`, the components made on the way to a failure were still made,
    /// so they are reported even though the operand as a whole failed.
    ///
    /// Measured: with a 300-character leaf, `mkdir -p -v newmid/<long>` prints
    /// ``created directory 'newmid'`` and *then* the `File name too long`
    /// error, and `newmid` is still there afterwards. Reporting only on overall
    /// success would lose that line and leave a directory on disk that nothing
    /// said had been made — the one case where a silent `-v` is actively
    /// misleading rather than merely quiet.
    ///
    /// A leaf too long for `NAME_MAX` is the scenario because it is the only
    /// portable way to make the *last* component fail while an earlier one
    /// succeeds: blocking a component with a planted file needs its parent to
    /// exist already, which is exactly the case where nothing new gets created.
    #[test]
    fn verbose_reports_what_was_built_before_a_failure() {
        let d = scratch("v_partial");
        let mid = d.join("newmid");
        let leaf = mid.join("z".repeat(300));

        let (ok, out, err) = run_all(true, true, None, &[&leaf]);
        assert!(!ok, "the over-long leaf must fail: {out}");
        assert_eq!(out, verbose_line(&mid), "the intermediate must be reported");
        assert!(!err.is_empty(), "the failure must be reported");
        assert!(mid.is_dir(), "and the intermediate really was created");

        // The contrasting case: when the *first* component is the one that
        // fails, nothing was created and nothing is reported.
        let (ok, out, err) = run_all(true, true, None, &[&d.join("y".repeat(300))]);
        assert!(!ok);
        assert_eq!(out, "");
        assert!(!err.is_empty());
        let _ = fs::remove_dir_all(&d);
    }

    /// `-v` and `-m` are independent: the mode is applied and the line printed.
    #[test]
    fn verbose_and_mode_together() {
        let d = scratch("v_mode");
        let a = d.join("a");
        let (ok, out, err) = run_all(false, true, Some("700"), &[&a]);
        assert!(ok, "{err}");
        assert_eq!(out, verbose_line(&a));
        assert!(a.is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// An invalid mode is refused before anything is created, so `-v` has
    /// nothing to report — the bad-mode message must not be preceded by a line
    /// claiming a directory was made.
    #[test]
    fn a_bad_mode_leaves_verbose_silent() {
        let d = scratch("v_badmode");
        let a = d.join("a");
        let (ok, out, err) = run_all(false, true, Some("zzz"), &[&a]);
        assert!(!ok);
        assert_eq!(out, "");
        assert_eq!(err, "mkdir: invalid mode \u{2018}zzz\u{2019}\n");
        let _ = fs::remove_dir_all(&d);
    }

    // --------------------------------------------------------------- mode --

    /// Four utilities, four sentences. This one has **no colon** after `mode`
    /// and **no referral** — measured, `mkdir -m zzz q` prints exactly
    /// ``mkdir: invalid mode ‘zzz’`` and nothing else. `chmod`'s has the colon,
    /// `mkfifo`'s drops the operand entirely.
    #[test]
    fn an_invalid_mode_is_refused() {
        let d = scratch("badmode");
        let a = d.join("a");
        let (ok, msg) = run_with_mode(false, Some("zzz"), &[&a]);
        assert!(!ok);
        assert_eq!(msg, "mkdir: invalid mode \u{2018}zzz\u{2019}\n");
        assert!(!a.exists(), "nothing may be created after a bad mode");
        let _ = fs::remove_dir_all(&d);
    }

    /// The measured ordering, and the reason [`MkdirFlags::mode`] holds an
    /// uncompiled `OsString`: with no operands the missing operand is the
    /// complaint, even though the mode is also wrong.
    #[test]
    fn missing_operand_is_reported_before_an_invalid_mode() {
        let (ok, msg) = run_with_mode(false, Some("zzz"), &[]);
        assert!(!ok);
        assert!(msg.contains("missing operand"), "{msg}");
        assert!(!msg.contains("invalid mode"), "{msg}");
    }

    #[test]
    fn a_valid_mode_creates_the_directory() {
        let d = scratch("goodmode");
        let a = d.join("a");
        let (ok, msg) = run_with_mode(false, Some("u=rwx,go="), &[&a]);
        assert!(ok, "{msg}");
        assert!(a.is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `mkdir -p x; chmod 755 x; mkdir -p -m 700 x` exits 0 and leaves
    /// `x` at `0755`. `-m` applies to what is created, not to what is found.
    #[test]
    fn an_existing_directory_keeps_its_mode() {
        let d = scratch("keepmode");
        let a = d.join("a");
        fs::create_dir(&a).unwrap();
        let (ok, msg) = run_with_mode(true, Some("700"), &[&a]);
        assert!(ok, "{msg}");
        assert!(msg.is_empty(), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// Applying the mode is the *system's* job; what this file has to get right
    /// is the arithmetic, and that is pure, so it is checked here against every
    /// umask rather than only on a unix host. Every row was measured against
    /// GNU coreutils 9.4.
    #[test]
    fn the_measured_mode_arithmetic() {
        let named = |spec: &str, umask_value: u32| {
            modes_for(OsStr::new(spec), umask_value)
                .expect("valid spec")
                .named
        };

        // An octal mode is the mode, whatever the umask. This is the one people
        // assume, and it is only true because the base is 0777 and `=` is
        // implied — a base of 0755 would answer 0755 for `mkdir -m 777 d`.
        for umask_value in [0o000, 0o022, 0o077, 0o002] {
            assert_eq!(named("700", umask_value), 0o700);
            assert_eq!(named("777", umask_value), 0o777);
        }

        // …but a *symbolic* clause that names no `who` is masked, which is what
        // proves the umask is passed through rather than merely zeroed. The four
        // answers are all different, so a stubbed umask cannot pass this.
        assert_eq!(named("a=,+w", 0o000), 0o222);
        assert_eq!(named("a=,+w", 0o022), 0o200);
        assert_eq!(named("a=,+w", 0o077), 0o200);
        assert_eq!(named("a=,+w", 0o002), 0o220);

        // A `who` of its own is *not* masked: `u+w` is the owner's write bit
        // even under a umask that would have taken it.
        assert_eq!(named("a=,u+w", 0o077), 0o200);
        assert_eq!(named("a=,o+w", 0o002), 0o002);

        // `X` consults the mode as accumulated so far *and* the fact that this
        // is a directory. After `a=` no execute bit survives, so on a file `+X`
        // would be nothing — but a directory always takes it. Measured: `mkdir
        // -m 'a=,+X' d` is 0111 where `mkfifo -m 'a=,+X' p` is 0000.
        assert_eq!(named("a=,+X", 0o000), 0o111);

        // The special bits survive compilation; `create_dir_with_mode` is what
        // then has to set them twice, because `mkdir(2)` may drop them.
        assert_eq!(named("2755", 0o022), 0o2755);
        assert_eq!(named("u=rwx,go=", 0o000), 0o700);

        // The parent mode: `(0777 & ~umask) | 0300`, pinned across six umasks.
        // The `| 0300` is the interesting half — under `umask 300` GNU still
        // makes the parents 0777, because a parent it can neither write to nor
        // descend into cannot hold the next component.
        let parent = |umask_value: u32| {
            modes_for(OsStr::new("700"), umask_value)
                .expect("valid spec")
                .parent
        };
        assert_eq!(parent(0o000), 0o777);
        assert_eq!(parent(0o022), 0o755);
        assert_eq!(parent(0o077), 0o700);
        assert_eq!(parent(0o002), 0o775);
        assert_eq!(parent(0o300), 0o777);
        assert_eq!(parent(0o777), 0o300);
    }

    /// Measured one by one against GNU 9.4, because the boundary is not where
    /// it looks. `8` is refused for being an octal mode with a non-octal digit;
    /// `a` is refused for being a `who` with no operator; `,` and `+r,` are
    /// refused for the empty clause; but `+` and `=` are **accepted**, and are
    /// the rows worth having, because a parser written from intuition refuses
    /// them.
    #[test]
    fn the_boundary_between_a_valid_and_an_invalid_mode() {
        for spec in ["zzz", "8", "u=q", "z+r", ",", "a", "+r,"] {
            assert!(
                modes_for(OsStr::new(spec), 0o022).is_none(),
                "{spec} must not compile"
            );
        }
        // `+` adds nothing, and — this is the part the umask makes visible — it
        // names no bits either, so the umask has nothing to take away from it.
        // Measured under `umask 022`: `mkdir -m + t` leaves `t` at 0777, not
        // 0755, which is the only place in this file where a mode comes out
        // *more* permissive than the default.
        assert_eq!(
            modes_for(OsStr::new("+"), 0o022).expect("+ is valid").named,
            0o777
        );
        // `=` with no `who` clears everything. Measured: `mkdir -m = t` is 0.
        assert_eq!(
            modes_for(OsStr::new("="), 0o022).expect("= is valid").named,
            0
        );
    }

    /// `-p` gives the parents a different mode from the leaf, which is the
    /// reason the walk is hand-written rather than [`fs::create_dir_all`].
    /// Measured: `umask 022; mkdir -p -m 700 a/b/c` leaves `a` and `a/b` at
    /// `755` and only `a/b/c` at `700`.
    #[test]
    #[cfg(unix)]
    fn dash_p_gives_the_parents_the_parent_mode() {
        use std::os::unix::fs::PermissionsExt;

        let d = scratch("parentmode");
        let deep = d.join("a").join("b").join("c");
        let (ok, msg) = run_with_mode(true, Some("700"), &[&deep]);
        assert!(ok, "{msg}");

        let expected_parent = modes_for(OsStr::new("700"), current_umask())
            .unwrap()
            .parent;
        let mode_of = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode_of(&d.join("a")), expected_parent);
        assert_eq!(mode_of(&d.join("a").join("b")), expected_parent);
        assert_eq!(mode_of(&deep), 0o700);
        let _ = fs::remove_dir_all(&d);
    }
}
