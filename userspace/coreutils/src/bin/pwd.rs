//! `pwd` -- print the full filename of the current working directory.
//!
//! # What was here before
//!
//! Eleven lines over `env::current_dir()`, and five defects:
//!
//! 1. **No options at all** -- no `-L`, no `-P`, no `--help`, no `--version`.
//!    `pwd --help` printed the working directory and exited 0, and `pwd -x`
//!    did the same where GNU refuses with `invalid option -- 'x'`.
//! 2. **No logical mode.** `-L` (and the default under `POSIXLY_CORRECT`)
//!    answers with `$PWD` when that names the same directory `.` does, so a
//!    shell that reached `/tmp/link` through a symlink is told `/tmp/link`
//!    rather than `/tmp/real`. Measured: `PWD=/tmp/pwdt/link pwd -L` prints
//!    `/tmp/pwdt/link`, `pwd -P` prints `/tmp/pwdt/real`.
//! 3. **An operand was silently accepted.** GNU warns
//!    `pwd: ignoring non-option arguments` on stderr and still exits 0.
//! 4. **The path was rendered with `Path::display`**, which substitutes U+FFFD
//!    for any byte that is not UTF-8 -- the silent corruption CLAUDE.md forbids
//!    by name. A directory called `/tmp/na\xffme` was printed as `/tmp/na?me`,
//!    which is not a name that exists. It is written as bytes now.
//! 5. **A failed write was reported as success.** `pwd >&-` exited 0, having
//!    printed nowhere; `pwd >/dev/full` did the same. GNU reports
//!    `pwd: write error: Bad file descriptor` and exits 1. Found by the
//!    closed-descriptor sweep, which is what brought this file into scope.
//!
//! # Why there is a fallback for a call that cannot fail
//!
//! `getcwd(3)` can fail, and when it does GNU does not give up: it walks up
//! the tree by hand, reading each `..` for the entry whose i-node matches the
//! child, and assembles the name a component at a time. That is
//! [`robust_getcwd`] below, transcribed from `pwd.c`. It exists upstream for
//! systems whose `getcwd` could not return a path longer than `PATH_MAX`,
//! which is not our problem -- but it is also the only path that produces
//! GNU's diagnostic for a *deleted* working directory, which is very much a
//! real case:
//!
//! ```text
//! $ mkdir /tmp/gone && cd /tmp/gone && rmdir /tmp/gone && pwd
//! pwd: couldn't find directory entry in ‘..’ with matching i-node
//! ```
//!
//! Reporting the raw `ENOENT` from `getcwd` instead would be a different
//! sentence for the same situation, so the walk is here.
//!
//! # Why these diagnostics quote curly
//!
//! Almost every file name coreutils prints goes through `quotef`/`quoteaf`,
//! which keep straight marks in every locale because the name is meant to be
//! pasted back into a shell. `pwd.c` is an exception: its diagnostics use
//! gnulib's *locale* `quote()`, so under a UTF-8 locale they come out `‘..’`
//! and not `'..'`. Measured, GNU 9.4, `LC_ALL=C.UTF-8` -- both of the two
//! messages this walk can actually be driven to produce:
//!
//! ```text
//! pwd: couldn't find directory entry in ‘..’ with matching i-node
//! pwd: cannot open directory ‘..’: Permission denied
//! ```
//!
//! (the second from a deleted working directory whose parent is `chmod 000`).
//! The names quoted here are only ever `/`, `.` and `..`, so nothing is at
//! stake beyond the marks -- but the marks are what a caller greps for, and
//! `pwd-diff.sh` compares stderr byte for byte.

use coreutils::diag;
use coreutils::errmsg;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd::{self, Stream};
use std::env;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `pwd`'s usage status is 1 -- measured: `pwd -x; echo $?` prints 1.
const PWD: Program = Program::new("pwd", 1);

/// GNU `pwd`'s `getopt_long` string, exactly.
///
/// No leading `+`, so an option after an operand is still an option --
/// measured: `pwd foo --help` prints the help text rather than treating
/// `--help` as a second thing to ignore.
const SHORT_OPTIONS: &str = "LP";

/// GNU `pwd`'s `longopts[]`, in its declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("logical", Takes::Nothing),
    ("physical", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run {
        /// `-L`: answer from `$PWD` when it is usable.
        logical: bool,
        /// Whether anything was left over after the options, which GNU
        /// mentions once and then ignores.
        extra_operands: bool,
    },
}

fn main() -> ExitCode {
    // Upstream registers `close_stdout` with `atexit`, so its verdict is
    // reached on every exit path, not just the last statement of `main`. One
    // value leaves this function; funnelling it here is the same guarantee.
    stdfd::close_stderr(run_main(), 1)
}

/// Everything the utility does, so that [`main`] is only the exit path --
/// upstream's `main` minus the `atexit` handler it registers.
fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Decided before the stream exists: upstream's `usage (EXIT_FAILURE)`
    // reaches `atexit (close_stdout)` with nothing buffered on stdout, so a bad
    // option prints only its own diagnostic and no write error after it.
    let request = match parse_args(&args, posixly_correct()) {
        Ok(request) => request,
        Err(e) => {
            diag!("pwd: {e}");
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let mut out = Stream::stdout();
    // A `Stream` and not `io::stderr()`: the diagnostics below are threaded
    // through an `impl Write` so the tests can read them out of a `Vec`, and
    // `io::stderr()` would answer `Ok` to a write that never happened -- its
    // `EBADF` is swallowed by the runtime, and its `ENOSPC` by the `let _ =`.
    // A `Stream` on descriptor 2 records the failure in the same crate-wide
    // flag [`diag!`] sets, which is what turns `pwd foo 2>&-` into the 1 GNU
    // exits with rather than the 0 the run had earned.
    let mut err = Stream::stderr();

    // `--help` and `--version` are writes like any other, so they fail like any
    // other: measured, `pwd --help >&-` is `pwd: write error: Bad file
    // descriptor` and exits 1.
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"pwd (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Request::Run {
            logical,
            extra_operands,
        } => {
            if extra_operands {
                let _ = writeln!(err, "pwd: ignoring non-option arguments");
            }
            run(logical, &mut out, &mut err)
        }
    };
    stdfd::close_stdout("pwd", out, earned)
}

/// Whether `POSIXLY_CORRECT` is set, which is the *only* thing that decides
/// `-L` versus `-P` when neither was given.
///
/// It has a second effect upstream that is not implemented here: gnulib's
/// `getopt` stops permuting when it is set, so `POSIXLY_CORRECT=1 pwd foo -L`
/// would treat `-L` as another ignored operand. That gap is crate-wide rather
/// than `pwd`'s -- see `known-issues.md:29600` -- and `pwd` is the one utility
/// where it cannot change the output, since every operand is ignored anyway
/// and the warning is printed either way.
fn posixly_correct() -> bool {
    env::var_os("POSIXLY_CORRECT").is_some()
}

fn help_text() -> String {
    "\
Usage: pwd [OPTION]...
Print the full filename of the current working directory.

  -L, --logical   use PWD from environment, even if it contains symlinks
  -P, --physical  avoid all symlinks
      --help        display this help and exit
      --version     output version information and exit

If no option is specified, -P is assumed.

NOTE: your shell may have its own version of pwd, which usually supersedes
the version described here.  Please refer to your shell's documentation
for details about the options it supports.
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `pwd`'s argv.
///
/// `posixly_correct` is the initial value of `logical`, matching upstream's
/// `bool logical = !!getenv ("POSIXLY_CORRECT");`. A later `-L` or `-P`
/// overrides it, and the *last* of the two wins -- measured: `pwd -L -P`
/// prints the physical name and `pwd -P -L` the logical one.
///
/// # Errors
///
/// An unknown option, a long option resolving to none or to more than one of
/// the table's entries, or a long option given a value it does not take.
fn parse_args(args: &[OsString], posixly_correct: bool) -> Result<Request, getopt::Error> {
    let mut logical = posixly_correct;
    let mut extra_operands = false;

    for item in PWD.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(_) => extra_operands = true,
            Opt::Short(b'L', _) | Opt::Long("logical", _) => logical = true,
            Opt::Short(b'P', _) | Opt::Long("physical", _) => logical = false,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(PWD.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(PWD.invalid_option(other)),
        }
    }

    Ok(Request::Run {
        logical,
        extra_operands,
    })
}

// ----------------------------------------------------------------- output ---

/// Print the working directory. Returns the status the run has earned, which
/// the flush afterwards may still worsen.
fn run(logical: bool, out: &mut impl Write, err: &mut impl Write) -> ExitCode {
    // Falling out of this is not an error: upstream goes straight on to the
    // physical answer when `$PWD` is unset, relative, or names something else.
    if logical && let Some(name) = logical_cwd() {
        emit(&name, out);
        return ExitCode::SUCCESS;
    }
    match env::current_dir() {
        Ok(path) => {
            emit(&os_bytes(path.as_os_str()), out);
            ExitCode::SUCCESS
        }
        Err(_) => match robust_getcwd(err) {
            Some(name) => {
                emit(&name, out);
                ExitCode::SUCCESS
            }
            None => ExitCode::FAILURE,
        },
    }
}

/// Write one directory name and its newline.
///
/// The name goes out as bytes. A directory name is an arbitrary byte string on
/// this system, and `Path::display` would replace whatever is not UTF-8 with
/// U+FFFD -- printing a name that does not exist rather than the one that does.
fn emit(name: &[u8], out: &mut impl Write) {
    let _ = out.write_all(name);
    let _ = out.write_all(b"\n");
}

// ---------------------------------------------------------------- logical ---

/// gnulib's `logical_getcwd`: `$PWD` if it is a usable name for `.`.
///
/// Two tests, in this order, because the cheap one rejects the names that
/// would make the expensive one misleading. See
/// [`logical_name_is_usable`] for the textual half.
#[cfg(unix)]
fn logical_cwd() -> Option<Vec<u8>> {
    use std::os::unix::fs::MetadataExt;

    let wd = env::var_os("PWD")?;
    let wd = os_bytes(&wd).into_owned();
    if !logical_name_is_usable(&wd) {
        return None;
    }
    let named = std::fs::metadata(coreutils::quote::os_from_bytes(&wd)).ok()?;
    let here = std::fs::metadata(".").ok()?;
    (named.dev() == here.dev() && named.ino() == here.ino()).then_some(wd)
}

/// Device and i-node are not reachable through stable `std` on Windows, so the
/// host settles for the textual half plus a canonical-path comparison. It
/// agrees with the real test except where two different names reach one
/// directory without either being a prefix of the other's canonical form --
/// which is a case the host cannot construct anyway.
#[cfg(not(unix))]
fn logical_cwd() -> Option<Vec<u8>> {
    let wd = env::var_os("PWD")?;
    let bytes = os_bytes(&wd).into_owned();
    if !logical_name_is_usable(&bytes) {
        return None;
    }
    let named = std::fs::canonicalize(&wd).ok()?;
    let here = std::fs::canonicalize(".").ok()?;
    (named == here).then_some(bytes)
}

/// The textual half of `logical_getcwd`: whether `$PWD` is even the *shape* of
/// an answer.
///
/// It must be absolute, and it must contain no `.` or `..` component. The
/// second rule is why this is a scan rather than a `starts_with('/')`: a name
/// like `/tmp/../tmp` would pass the i-node test -- it really is this
/// directory -- and still be the wrong thing to print, because `pwd`'s
/// contract is a name with no traversal left in it.
///
/// Note what is *not* rejected: a dotfile. `/a/.config` contains `/.` and is
/// fine, because the character after it is neither `/` nor the end of the
/// string. That is the subtlety upstream spells with `p[2]`, and the reason
/// this is a transcription rather than a `split('/')`.
fn logical_name_is_usable(wd: &[u8]) -> bool {
    if wd.first() != Some(&b'/') {
        return false;
    }
    let mut i = 0;
    while i + 1 < wd.len() {
        if wd[i] != b'/' || wd[i + 1] != b'.' {
            i += 1;
            continue;
        }
        // `wd[i..]` starts `/.`; classify what follows the dot.
        match wd.get(i + 2) {
            // `/.` at the end, or a `/./` component.
            None | Some(b'/') => return false,
            Some(b'.') => match wd.get(i + 3) {
                // `/..` at the end, or a `/../` component.
                None | Some(b'/') => return false,
                _ => {}
            },
            _ => {}
        }
        i += 1;
    }
    true
}

// --------------------------------------------------------------- fallback ---

/// One diagnostic in upstream's `error (EXIT_FAILURE, errno, …)` shape.
fn diagnose(err: &mut impl Write, what: &str, cause: Option<&io::Error>) {
    match cause {
        Some(e) => {
            let _ = writeln!(err, "pwd: {what}: {}", errmsg::strerror(e));
        }
        None => {
            let _ = writeln!(err, "pwd: {what}");
        }
    }
}

/// gnulib's `robust_getcwd`: build the name by walking to the root.
///
/// Returns `None` having already printed the diagnostic, because every failure
/// here is upstream's `error (EXIT_FAILURE, …)` -- there is no partial answer
/// worth printing.
///
/// This *changes the process's working directory* as it goes, exactly as
/// upstream does. That is safe only because `pwd` exits immediately after, and
/// is why this function is not offered to anyone else.
#[cfg(unix)]
fn robust_getcwd(err: &mut impl Write) -> Option<Vec<u8>> {
    use coreutils::quote::quote;
    use std::os::unix::fs::MetadataExt;

    let root = match std::fs::metadata("/") {
        Ok(m) => (m.dev(), m.ino()),
        Err(e) => {
            diagnose(
                err,
                &format!("failed to get attributes of {}", quote(b"/")),
                Some(&e),
            );
            return None;
        }
    };
    let mut dot = match std::fs::metadata(".") {
        Ok(m) => (m.dev(), m.ino()),
        Err(e) => {
            diagnose(err, &format!("failed to stat {}", quote(b".")), Some(&e));
            return None;
        }
    };

    let mut name: Vec<u8> = Vec::new();
    while dot != root {
        dot = find_dir_entry(dot, &mut name, err)?;
    }
    // Every component prepended its own leading `/`, so the only name that can
    // still be empty is the root's -- which is `/`.
    if name.is_empty() {
        name.push(b'/');
    }
    Some(name)
}

/// One step of [`robust_getcwd`]: name the current directory within its parent,
/// prepend it, move up, and report the parent's identity.
#[cfg(unix)]
fn find_dir_entry(dot: (u64, u64), name: &mut Vec<u8>, err: &mut impl Write) -> Option<(u64, u64)> {
    use coreutils::quote::quote;
    use std::os::unix::fs::{DirEntryExt, MetadataExt};

    // Opened before the `chdir`, so the handle keeps referring to this parent
    // even though the name `..` will mean something else a line later.
    let entries = match std::fs::read_dir("..") {
        Ok(d) => d,
        Err(e) => {
            diagnose(
                err,
                &format!("cannot open directory {}", quote(b"..")),
                Some(&e),
            );
            return None;
        }
    };
    if let Err(e) = env::set_current_dir("..") {
        diagnose(
            err,
            &format!("failed to chdir to {}", quote(b"..")),
            Some(&e),
        );
        return None;
    }
    let parent = match std::fs::metadata(".") {
        Ok(m) => (m.dev(), m.ino()),
        Err(e) => {
            diagnose(err, &format!("failed to stat {}", quote(b"..")), Some(&e));
            return None;
        }
    };

    // Across a mount point the parent's directory entries carry the *mounted*
    // filesystem's i-node numbers, which are meaningless against ours, so the
    // number has to come from an `lstat` instead and the device compared too.
    let crossed = parent.0 != dot.0;

    let mut found: Option<OsString> = None;
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                diagnose(
                    err,
                    &format!("reading directory {}", quote(b"..")),
                    Some(&e),
                );
                return None;
            }
        };
        let leaf = entry.file_name();
        let mut ino = entry.ino();
        let mut dev = None;
        // A `d_ino` of zero is the "no i-node number here" marker, not an
        // i-node; `lstat` is the only way to learn the real one.
        if crossed || ino == 0 {
            // Relative to the *new* cwd, which is this parent -- so the bare
            // leaf name is right and `entry.path()` (which says `../leaf`)
            // would not be.
            match std::fs::symlink_metadata(&leaf) {
                Ok(m) => {
                    ino = m.ino();
                    dev = Some(m.dev());
                }
                // Skip anything that cannot be stat'd; it is not us.
                Err(_) => continue,
            }
        }
        if ino != dot.1 {
            continue;
        }
        if !crossed || dev == Some(dot.0) {
            found = Some(leaf);
            break;
        }
    }

    let Some(leaf) = found else {
        diagnose(
            err,
            &format!(
                "couldn't find directory entry in {} with matching i-node",
                quote(b"..")
            ),
            None,
        );
        return None;
    };

    let mut prefix = vec![b'/'];
    prefix.extend_from_slice(&os_bytes(&leaf));
    prefix.append(name);
    *name = prefix;
    Some(parent)
}

/// The walk needs `dev`/`ino` and a `chdir`, neither of which is reachable
/// through stable `std` on Windows. The host cannot reach this path anyway:
/// `GetCurrentDirectory` does not fail for a deleted directory the way
/// `getcwd` does, because the handle keeps the directory alive.
#[cfg(not(unix))]
fn robust_getcwd(err: &mut impl Write) -> Option<Vec<u8>> {
    diagnose(err, "failed to get current directory", None);
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn parse(args: &[&str]) -> Request {
        parse_args(&argv(args), false).unwrap()
    }

    #[test]
    fn physical_is_the_default() {
        assert_eq!(
            parse(&[]),
            Request::Run {
                logical: false,
                extra_operands: false
            }
        );
    }

    /// The one thing `POSIXLY_CORRECT` decides here.
    #[test]
    fn posixly_correct_makes_logical_the_default() {
        assert_eq!(
            parse_args(&argv(&[]), true).unwrap(),
            Request::Run {
                logical: true,
                extra_operands: false
            }
        );
        // And an explicit `-P` still overrides it.
        assert_eq!(
            parse_args(&argv(&["-P"]), true).unwrap(),
            Request::Run {
                logical: false,
                extra_operands: false
            }
        );
    }

    /// Measured: `pwd -L -P` prints the physical name, `pwd -P -L` the logical
    /// one, so neither flag wins by rank -- the later one wins.
    #[test]
    fn the_last_of_l_and_p_wins() {
        assert!(matches!(
            parse(&["-L", "-P"]),
            Request::Run { logical: false, .. }
        ));
        assert!(matches!(
            parse(&["-P", "-L"]),
            Request::Run { logical: true, .. }
        ));
        assert!(matches!(
            parse(&["--physical", "--logical"]),
            Request::Run { logical: true, .. }
        ));
    }

    #[test]
    fn an_operand_is_noted_and_otherwise_ignored() {
        assert_eq!(
            parse(&["foo"]),
            Request::Run {
                logical: false,
                extra_operands: true
            }
        );
        // Several operands are still one warning.
        assert_eq!(
            parse(&["a", "b"]),
            Request::Run {
                logical: false,
                extra_operands: true
            }
        );
    }

    /// GNU's `getopt_long` string has no `+`, so an option after an operand is
    /// still an option -- measured: `pwd foo --help` prints the help text.
    #[test]
    fn options_may_follow_operands() {
        assert_eq!(parse(&["foo", "--help"]), Request::Help);
        assert!(matches!(
            parse(&["foo", "-L"]),
            Request::Run {
                logical: true,
                extra_operands: true
            }
        ));
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse(&["--help"]), Request::Help);
        assert_eq!(parse(&["--version"]), Request::Version);
    }

    #[test]
    fn an_unknown_option_is_refused() {
        let e = parse_args(&argv(&["-x"]), false).unwrap_err();
        assert_eq!(e.status, 1);
        assert_eq!(
            e.message(),
            "invalid option -- 'x'\nTry 'pwd --help' for more information."
        );
        let e = parse_args(&argv(&["--nope"]), false).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'pwd --help' for more information."
        );
    }

    #[test]
    fn a_value_given_to_a_flag_is_refused() {
        let e = parse_args(&argv(&["--logical=3"]), false).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--logical' doesn't allow an argument\n\
             Try 'pwd --help' for more information."
        );
    }

    // ------------------------------------------------ logical_name_is_usable ---

    #[test]
    fn a_relative_pwd_is_not_usable() {
        assert!(!logical_name_is_usable(b""));
        assert!(!logical_name_is_usable(b"relative"));
        assert!(!logical_name_is_usable(b"./a"));
    }

    #[test]
    fn a_plain_absolute_pwd_is_usable() {
        assert!(logical_name_is_usable(b"/"));
        assert!(logical_name_is_usable(b"/tmp"));
        assert!(logical_name_is_usable(b"/tmp/pwdt/link"));
        // Doubled slashes are not traversal, and upstream does not reject them.
        assert!(logical_name_is_usable(b"//tmp//a"));
    }

    #[test]
    fn a_traversal_component_is_not_usable() {
        assert!(!logical_name_is_usable(b"/tmp/./a"));
        assert!(!logical_name_is_usable(b"/tmp/../a"));
        assert!(!logical_name_is_usable(b"/tmp/."));
        assert!(!logical_name_is_usable(b"/tmp/.."));
        assert!(!logical_name_is_usable(b"/."));
        assert!(!logical_name_is_usable(b"/.."));
    }

    /// The `p[2]` subtlety: a dotfile contains `/.` and is perfectly fine.
    #[test]
    fn a_dotfile_component_is_usable() {
        assert!(logical_name_is_usable(b"/home/me/.config"));
        assert!(logical_name_is_usable(b"/home/me/.config/app"));
        assert!(logical_name_is_usable(b"/a/..b"));
        assert!(logical_name_is_usable(b"/a/...."));
    }

    /// A directory name is an arbitrary byte string, and the check must not
    /// assume otherwise.
    #[test]
    fn a_non_utf8_pwd_is_judged_by_its_bytes() {
        assert!(logical_name_is_usable(b"/tmp/na\xffme"));
        assert!(!logical_name_is_usable(b"/tmp/na\xffme/.."));
        assert!(!logical_name_is_usable(b"\xffnot-absolute"));
    }

    // ------------------------------------------------------------- output ---

    #[test]
    fn emit_writes_the_bytes_and_a_newline() {
        let mut out = Vec::new();
        emit(b"/some/cwd", &mut out);
        assert_eq!(out, b"/some/cwd\n");
    }

    #[test]
    fn emit_does_not_corrupt_a_name_that_is_not_utf8() {
        let mut out = Vec::new();
        emit(b"/tmp/na\xffme", &mut out);
        assert_eq!(out, b"/tmp/na\xffme\n");
    }

    #[test]
    fn a_diagnostic_with_no_errno_has_no_trailing_colon() {
        let mut err = Vec::new();
        diagnose(&mut err, "couldn't find it", None);
        assert_eq!(err, b"pwd: couldn't find it\n");
    }

    #[test]
    fn a_diagnostic_with_an_errno_names_it() {
        let mut err = Vec::new();
        diagnose(
            &mut err,
            "failed to stat '.'",
            Some(&io::Error::from(io::ErrorKind::PermissionDenied)),
        );
        let s = String::from_utf8(err).unwrap();
        assert!(s.starts_with("pwd: failed to stat '.': "), "got {s:?}");
        assert!(s.ends_with('\n'));
    }
}
