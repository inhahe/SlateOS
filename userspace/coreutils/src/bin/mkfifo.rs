//! mkfifo — make FIFOs (named pipes).
//!
//! # Why this was rewritten
//!
//! The version this replaces had six defects, and the first is the reason the
//! rewrite could not wait:
//!
//! 1. **`-m` with a mode it could not parse silently fell back to `0666`.** The
//!    parse was `u32::from_str_radix(v, 8).unwrap_or(0o666)`, and the comment
//!    above it said this matched "the existing implementation" — so a typo in
//!    `mkfifo -m 600 secret.pipe` produced a *world-writable* FIFO and said
//!    nothing at all. That is the exact failure mode this crate's `mkdir` refused
//!    to have: an option whose whole purpose is to make something **less**
//!    permissive must never quietly do the opposite. Measured, GNU refuses the
//!    command outright — `mkfifo: invalid mode`, status 1, nothing created.
//!
//! 2. **Only octal modes existed.** `mkfifo -m u=rw,go= p` set `0666`, because
//!    `from_str_radix` rejected it and the fallback swallowed the rejection.
//!    Symbolic modes now go through [`modechange`], the same parser `chmod`,
//!    `mkdir` and the shell's `umask` use.
//!
//! 3. **The failure message had no reason on the end of it.** `mkfifo: cannot
//!    create fifo 'a'` against GNU's ``mkfifo: cannot create fifo 'a': File
//!    exists``. Which of "it is already there", "the directory does not exist"
//!    and "you may not write here" happened was left for the user to guess.
//!
//! 4. **Argv was read as `String`**, so a FIFO name holding a byte that is not
//!    valid UTF-8 — legal on this OS by design, `design.txt`: every byte but `/`
//!    and NUL — *panicked* in `env::args()` before `mkfifo` saw it. See
//!    `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
//!
//! 5. **There was no option parser.** No long options, so no `--mode`, no
//!    `--help` and no `--version`; no `--` to end options, so a FIFO whose name
//!    begins with a dash could not be created; no `-m700` bundled form; and `-m`
//!    as the last argument became a *file name* rather than an error, because
//!    the loop's else-branch caught it.
//!
//! 6. **`missing operand` carried no referral**, where GNU follows it with
//!    `Try 'mkfifo --help' for more information.`
//!
//! # What `mkfifo`'s mode does *not* share with `mkdir`'s
//!
//! Both compile the same syntax with the same [`modechange`], and the two
//! differ in every parameter, which is why a single shared "apply -m" helper
//! would be wrong. All measured against GNU coreutils 9.4:
//!
//! | | `mkdir -m` | `mkfifo -m` |
//! |---|---|---|
//! | Base mode | `0777` | `0666` |
//! | `dir` for `X` | `true` | `false` |
//! | `-m +` | `0777` | `0666` |
//! | `-m 'a=,+X'` | `0111` | `0000` |
//! | `-m 2755` | `2755` | refused |
//! | Bad mode | `mkdir: invalid mode ‘zzz’` | `mkfifo: invalid mode` |
//!
//! The last row is worth stating plainly: **GNU's `mkfifo` does not tell you
//! which mode it rejected.** That is unhelpful, and it is still what this
//! implements, because a message is an interface and a script that greps for it
//! is entitled to the one its author measured. The same reasoning appears in
//! `mkdir`'s docs about quoting style.
//!
//! # `-Z` and `--context`
//!
//! They set an SELinux/SMACK security context, and are taken as upstream takes
//! them on a kernel with neither, which SlateOS is (design-decisions §1064):
//! `-Z`, or `--context` without a value, is accepted in silence -- there is no
//! default context to set -- and `--context=CTX` is `mkfifo: warning: ignoring
//! --context; it requires an SELinux/SMACK-enabled kernel`, printed from inside
//! the option loop, after which the FIFOs are made. They used to be refused by
//! name, which broke scripts that pass `-Z` portably and work on every Linux
//! without SELinux.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quoteaf_os};
use coreutils::stdfd::{self, Stream};
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::process::ExitCode;

coreutils::guard_std_fds!();

// The two calls upstream's loop makes per name: the FIFO, then -- under `-m`
// -- its mode set exactly, through a call that will not follow a symbolic link
// someone put in its place meanwhile.
#[cfg(unix)]
unsafe extern "C" {
    fn mkfifo(path: *const u8, mode: u32) -> i32;
    fn lchmod(path: *const u8, mode: u32) -> i32;
}

/// `mkfifo`'s usage status is 1 — measured: `mkfifo -q x; echo $?` prints 1.
const MKFIFO: Program = Program::new("mkfifo", 1);

/// GNU `mkfifo`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order, and an empty prefix matches everything. Measured:
///
/// ```text
/// mkfifo: option '--=x' is ambiguous; possibilities: '--context' '--mode'
/// '--help' '--version'
/// ```
///
/// Note what is *absent*: `mkfifo` has no `--verbose` and no `--parents`. It is
/// a shorter table than `mkdir`'s, and `--m` therefore resolves here as it does
/// there — measured, `mkfifo --m 700 c1` creates `c1` at `0700`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("context", Takes::Optional),
    ("mode", Takes::Required),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// GNU `mkfifo`'s `getopt_long` short-option string, verbatim.
const SHORT_OPTIONS: &str = "m:Z";

/// The mode a FIFO gets when `-m` says nothing: `0666`, *before* the umask,
/// which the kernel then applies itself. Measured: with no `-m`, `mkfifo q`
/// leaves `q` at `666`, `644`, `600` and `664` under umasks 000, 022, 077 and
/// 002 — so the umask is not this program's business unless `-m` is given.
///
/// It is also the base a `-m` clause is compiled against, which is the reason
/// `mkfifo -m + p` is `0666` where `mkdir -m + d` is `0777`.
const BASE_MODE: u32 = 0o666;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MkfifoFlags {
    /// `-m`'s argument **uncompiled**. The order in which two mistakes are
    /// reported is observable: measured, `mkfifo -m zzz` with no operands
    /// answers `missing operand`, not `invalid mode`, so the mode must not be
    /// compiled while the command line is still being read.
    mode: Option<OsString>,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order.
    Run(MkfifoFlags, Vec<OsString>),
}

/// The funnel: upstream's `atexit (close_stdout)`, which checks standard
/// output and then standard error on every exit path at once -- an output or
/// a diagnostic that did not arrive is status 1, as `mkfifo: write error:
/// ...` for the first. The descriptor guard is what lets a closed one be seen
/// at all; the runtime used to answer it with a quiet `/dev/null`. See
/// [`stdfd::close_stdout`].
fn main() -> ExitCode {
    stdfd::restore();
    let mut out = Stream::stdout();
    let earned = run_main(&mut out);
    stdfd::close_stdout("mkfifo", out, earned)
}

fn run_main(out: &mut Stream) -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut warnings = Vec::new();
    let parsed = parse_args(&args, &mut warnings);
    // In the order they arose and before whatever ended the parse: upstream
    // prints each from inside its option loop.
    for w in &warnings {
        diag!("mkfifo: {w}");
    }
    match parsed {
        Ok(Request::Help) => {
            // Never an error: the stream records it for the funnel.
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            let _ = out.write_all(b"mkfifo (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, names)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            if make_all(&flags, &names, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("mkfifo: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: mkfifo [OPTION]... NAME...
Create named pipes (FIFOs) with the given NAMEs.

  -m, --mode=MODE set file permission bits to MODE, not a=rw - umask
  -Z              accepted and ignored: SlateOS has no security contexts
      --context[=CTX]  accepted; a CTX is warned about and ignored
      --help      display this help and exit
      --version   output version information and exit

To create a FIFO whose name starts with a '-', for example '-foo',
use one of these commands:
  mkfifo -- -foo
  mkfifo ./-foo
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Upstream's warning for a context *named* on a kernel with neither SELinux
/// nor SMACK -- the sentence `mkdir.c`, `mkfifo.c` and `mknod.c` share.
const IGNORING_CONTEXT: &str =
    "warning: ignoring --context; it requires an SELinux/SMACK-enabled kernel";

/// Parse `mkfifo`'s argv into `(flags, operands)`, appending to `warnings`,
/// in order, what upstream's option loop would have printed on the way.
///
/// Options and operands may be interleaved — `mkfifo a -m 600 b` is
/// `mkfifo -m 600 a b` — which is `getopt_long`'s default permuting behaviour.
///
/// # Errors
///
/// An unknown option, a long option given a value it does not take, or `-m`
/// with no value.
fn parse_args(args: &[OsString], warnings: &mut Vec<String>) -> Result<Request, getopt::Error> {
    let mut flags = MkfifoFlags::default();
    let mut names: Vec<OsString> = Vec::new();

    for item in MKFIFO.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'm', value) | Opt::Long("mode", value) => flags.mode = value,
            // The default context, on a kernel that has none: nothing to set,
            // and upstream says nothing. See the module docs.
            Opt::Short(b'Z', _) => {}
            Opt::Long("context", value) => {
                // `--context=` names one too: an empty `optarg` is not null.
                if value.is_some() {
                    warnings.push(IGNORING_CONTEXT.to_string());
                }
            }
            // Unreachable: every option in the two tables is handled above.
            Opt::Short(other, _) => return Err(MKFIFO.invalid_option(other)),
            Opt::Long(other, _) => {
                return Err(MKFIFO.unrecognized_option(format!("--{other}").as_bytes()));
            }
            // A lone `-` arrives here, not as an option: `mkfifo` has no
            // standard-input operand for it to mean anything else, so it is a
            // FIFO called `-`.
            Opt::Operand(name) => names.push(name.clone()),
        }
    }

    Ok(Request::Run(flags, names))
}

// -------------------------------------------------------------------- mode --

/// Why a `-m` argument was refused. The two sentences are different and neither
/// carries a referral; both were measured.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum ModeError {
    /// The spec is not a mode at all. `mkfifo -m zzz p` → `mkfifo: invalid mode`
    /// — with **no mention of `zzz`**, unlike every neighbour. See the module
    /// docs for why that is reproduced rather than improved.
    Invalid,
    /// The spec is a mode, but it sets setuid, setgid or the sticky bit.
    /// Measured: `-m 2755`, `-m 1755`, `-m 4755`, `-m 7777`, `-m u+s`, `-m g+s`
    /// and `-m +t` all answer `mkfifo: mode must specify only file permission
    /// bits`. `mkfifo(2)` would have dropped them silently, which is the reason
    /// GNU checks rather than letting the kernel decide.
    NotPermissionBits,
}

impl ModeError {
    fn sentence(self) -> &'static str {
        match self {
            Self::Invalid => "invalid mode",
            Self::NotPermissionBits => "mode must specify only file permission bits",
        }
    }
}

/// Resolve `-m`'s argument against the umask in force.
///
/// Upstream reads the mask as `umask (0)` then `umask (old)` -- putting it
/// back -- and the FIFO is then made under it and `lchmod`ed to exactly the
/// mode `-m` asked for (see [`make_all`]). This reads it without the write
/// ([`coreutils::umask::current`] asks `/proc`), which is the same answer with
/// no window in which the mask is wrong. It used to zero the mask for the rest
/// of the process instead and skip the `lchmod`: the same final mode, but
/// under `cargo test` a zeroed mask is every other test's mask too.
fn resolve_mode(spec: &OsStr) -> Result<u32, ModeError> {
    mode_for(spec, coreutils::umask::current())
}

/// The mode arithmetic, with the umask passed in rather than read.
///
/// [`BASE_MODE`] is the starting mode and `dir` is `false`, which together are
/// the whole difference from `mkdir`'s otherwise identical call. The umask is
/// *passed* rather than merely cleared because it still reaches a clause that
/// names no `who` — measured: `mkfifo -m 'a=,+w' p` is `0222`, `0200`, `0200`
/// and `0220` under umasks 000, 022, 077 and 002.
///
/// # Errors
///
/// [`ModeError::Invalid`] if the spec does not compile, or
/// [`ModeError::NotPermissionBits`] if it compiles to a mode outside `0777`.
fn mode_for(spec: &OsStr, umask_value: u32) -> Result<u32, ModeError> {
    let changes = modechange::compile(&os_bytes(spec)).ok_or(ModeError::Invalid)?;
    let mode = modechange::adjust(BASE_MODE, false, umask_value, &changes).mode;
    if mode & !0o777 != 0 {
        return Err(ModeError::NotPermissionBits);
    }
    Ok(mode)
}

// ---------------------------------------------------------------- creating --

/// Create one FIFO.
///
/// The name is passed to the syscall as **bytes**, not through `str`: on this OS
/// a path may hold any byte but `/` and NUL (`design.txt`), and the previous
/// version's `&str` could not express that.
///
/// # Errors
///
/// Whatever `mkfifo(2)` reported, or `InvalidInput` for a name containing a NUL
/// — which no C string can carry, and which is therefore this layer's to refuse
/// rather than the kernel's to see a truncated version of.
#[cfg(unix)]
fn make_one(name: &OsStr, mode: u32) -> io::Result<()> {
    // SAFETY: `mkfifo` is POSIX, takes a borrowed C string it does not retain,
    // and reports failure through `errno`, which `with_c_path` reads at once.
    with_c_path(name, |p| unsafe { mkfifo(p, mode) })
}

/// `lchmod (name, mode)`: the FIFO just made, given exactly `-m`'s mode -- the
/// kernel narrowed what `mkfifo(2)` was asked for by the umask.
#[cfg(unix)]
fn set_mode(name: &OsStr, mode: u32) -> io::Result<()> {
    // SAFETY: as in `make_one`; `lchmod` reads the path and keeps nothing.
    with_c_path(name, |p| unsafe { lchmod(p, mode) })
}

/// Call `f` with `name` as a NUL-terminated C string, mapping its `-1` to the
/// `errno` it left.
///
/// # Errors
///
/// What `f` reported, or `InvalidInput` for a name containing a NUL -- which
/// no C string can carry, and which is therefore this layer's to refuse rather
/// than the kernel's to see a truncated version of.
#[cfg(unix)]
fn with_c_path(name: &OsStr, f: impl FnOnce(*const u8) -> i32) -> io::Result<()> {
    let bytes = os_bytes(name);
    if bytes.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains a NUL byte",
        ));
    }
    let mut c_path: Vec<u8> = Vec::with_capacity(bytes.len().saturating_add(1));
    c_path.extend_from_slice(&bytes);
    c_path.push(0);
    if f(c_path.as_ptr()) == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// The non-unix arm. The host build exists to run the parsing, mode and
/// diagnostic tests; SlateOS is unix-family, so this is never the shipped path.
#[cfg(not(unix))]
fn make_one(_name: &OsStr, _mode: u32) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "mkfifo is not supported on this platform",
    ))
}

/// No modes on the host: there is nothing to set.
#[cfg(not(unix))]
#[allow(clippy::unnecessary_wraps)] // the unix arm's signature, which `make_all` relies on
fn set_mode(_name: &OsStr, _mode: u32) -> io::Result<()> {
    Ok(())
}

/// Create every FIFO the command line asked for, reporting failures to `err`.
///
/// Returns `true` if every one was created. Takes the error sink as a parameter
/// rather than writing to `stderr` directly so the diagnostics can be asserted
/// on in tests; the old file had no test of this path at all, which is how
/// defect 3 — a message with no reason on the end of it — survived.
///
/// The two kinds of failure behave differently, and both were measured. A bad
/// `-m` is fatal *before anything is created*: `mkfifo -m zzz x y` creates
/// neither. A failure on one name is not: `mkfifo a g` with `a` present reports
/// `a`, still creates `g`, and exits 1.
fn make_all<W: Write>(flags: &MkfifoFlags, names: &[OsString], err: &mut W) -> bool {
    if names.is_empty() {
        let _ = writeln!(
            err,
            "mkfifo: {}",
            MKFIFO.usage_referring("missing operand".into())
        );
        return false;
    }

    // *After* the operand check. Measured: `mkfifo -m zzz` with no operands
    // answers `missing operand`, so a parser that validated `-m` as it read it
    // could not produce GNU's ordering.
    let mode = match &flags.mode {
        None => BASE_MODE,
        Some(spec) => match resolve_mode(spec) {
            Ok(mode) => mode,
            Err(e) => {
                // No referral on either sentence, and no mention of the spec:
                // see [`ModeError`].
                let _ = writeln!(err, "mkfifo: {}", e.sentence());
                return false;
            }
        },
    };

    let mut ok = true;
    for name in names {
        if let Err(e) = make_one(name, mode) {
            // `quoteaf_os`, not `quote_os`: straight marks. Measured,
            // ``mkfifo: cannot create fifo 'a': File exists`` — which is the
            // *opposite* of `mkdir`'s one curly message, and the reason that
            // file carries a table of which neighbour uses which.
            //
            // `strerror`, not `{e}`: why it failed has to read the same wherever
            // it is printed. See [`coreutils::errmsg`].
            let why = strerror(&e);
            let _ = writeln!(
                err,
                "mkfifo: cannot create fifo {}: {why}",
                quoteaf_os(name)
            );
            ok = false;
        } else if flags.mode.is_some()
            && let Err(e) = set_mode(name, mode)
        {
            // Upstream's `else if (specified_mode && lchmod (...) != 0)`.
            let _ = writeln!(
                err,
                "mkfifo: cannot set permissions of {}: {}",
                quoteaf_os(name),
                strerror(&e)
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

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// [`parse_args`] for a test that has no warning to look at.
    fn parse_args_quiet(argv: &[OsString]) -> Result<Request, getopt::Error> {
        parse_args(argv, &mut Vec::new())
    }

    /// [`parse_args`], and the warnings it made on the way.
    fn parse_with_warnings(items: &[&str]) -> (Result<Request, getopt::Error>, Vec<String>) {
        let mut warnings = Vec::new();
        let parsed = parse_args(&args(items), &mut warnings);
        (parsed, warnings)
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (MkfifoFlags, Vec<String>) {
        match parse_args_quiet(&args(items)).unwrap() {
            Request::Run(f, n) => (
                f,
                n.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args_quiet(&args(items)).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --

    #[test]
    fn no_args() {
        let (f, n) = run_parse(&[]);
        assert_eq!(f.mode, None);
        assert!(n.is_empty());
    }

    #[test]
    fn operands_only() {
        let (f, n) = run_parse(&["a.fifo", "b.fifo"]);
        assert_eq!(f.mode, None);
        assert_eq!(n, vec!["a.fifo", "b.fifo"]);
    }

    /// Defect 5: none of these worked. `-m700` and `--mode=700` were file
    /// names, and `--mode 700` was two of them.
    #[test]
    fn the_four_spellings_of_the_mode_option() {
        for spelling in [
            &["-m", "700", "p"][..],
            &["-m700", "p"][..],
            &["--mode=700", "p"][..],
            &["--mode", "700", "p"][..],
        ] {
            let (f, n) = run_parse(spelling);
            assert_eq!(f.mode, Some(OsString::from("700")), "{spelling:?}");
            assert_eq!(n, vec!["p"], "{spelling:?}");
        }
    }

    #[test]
    fn the_last_mode_wins() {
        assert_eq!(
            run_parse(&["-m", "600", "-m", "755", "p"]).0.mode,
            Some(OsString::from("755"))
        );
    }

    #[test]
    fn a_flag_may_follow_an_operand() {
        let (f, n) = run_parse(&["p", "-m", "600"]);
        assert_eq!(f.mode, Some(OsString::from("600")));
        assert_eq!(n, vec!["p"]);
    }

    /// Defect 5, the sharpest form of it: `mkfifo -m` used to create a FIFO
    /// called `-m`. Measured, GNU refuses — and the two sentences differ, the
    /// short one naming the option last and the long one first.
    #[test]
    fn the_mode_option_needs_a_value() {
        assert_eq!(fail(&["-m"]).sentence, "option requires an argument -- 'm'");
        assert_eq!(
            fail(&["--mode"]).sentence,
            "option '--mode' requires an argument"
        );
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-"]).1, vec!["-"]);
    }

    /// Defect 5: without a `--`, a FIFO whose name begins with a dash could not
    /// be created at all.
    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-foo", "bar"]).1, vec!["-foo", "bar"]);
        let (f, n) = run_parse(&["--", "-m"]);
        assert_eq!(f.mode, None, "-m after -- is a name, not a flag");
        assert_eq!(n, vec!["-m"]);
    }

    /// Also defect 5: there were no long options, so `--help` was a file name.
    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args_quiet(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(
            parse_args_quiet(&args(&["--version"])).unwrap(),
            Request::Version
        );
    }

    /// The whole table, in GNU's declaration order, as `mkfifo --=x` prints it.
    /// An empty prefix matches every entry, so this pins the order itself. It is
    /// a shorter table than `mkdir`'s — no `--verbose`, no `--parents` — which
    /// is why `--m` resolves here with nothing to be ambiguous with.
    #[test]
    fn the_empty_prefix_lists_the_table_in_order() {
        assert_eq!(
            fail(&["--=x"]).sentence,
            "option '--=x' is ambiguous; possibilities: '--context' '--mode' \
             '--help' '--version'"
        );
    }

    #[test]
    fn unambiguous_abbreviations_resolve() {
        // Measured: `mkfifo --m 700 c1` creates `c1` at 0700.
        assert_eq!(
            run_parse(&["--m", "700", "c1"]).0.mode,
            Some(OsString::from("700"))
        );
        // `--c` prefixes only `--context`: it resolves, and with no value it
        // is accepted in silence.
        let (parsed, warnings) = parse_with_warnings(&["--c", "p"]);
        assert!(matches!(parsed, Ok(Request::Run(..))), "{parsed:?}");
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "p"]);
        assert_eq!(e.sentence, "invalid option -- 'q'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        let e = fail(&["--zzz=1", "p"]);
        assert_eq!(e.sentence, "unrecognized option '--zzz=1'");
        assert_eq!(e.status, 1);
    }

    /// Design-decisions §1064: the default context is accepted in silence, a
    /// named one -- `--context=` included -- warned about once per naming, and
    /// the run goes on. Measured against GNU 9.4 by `mkfifo-diff.sh`.
    #[test]
    fn the_security_context_options_are_taken_as_upstream_takes_them_without_selinux() {
        for spelling in [&["-Z", "p"][..], &["--context", "p"][..]] {
            let (parsed, warnings) = parse_with_warnings(spelling);
            assert!(
                matches!(parsed, Ok(Request::Run(..))),
                "{spelling:?}: {parsed:?}"
            );
            assert!(warnings.is_empty(), "{spelling:?}: {warnings:?}");
        }
        for spelling in [&["--context=x", "p"][..], &["--context=", "p"][..]] {
            let (parsed, warnings) = parse_with_warnings(spelling);
            assert!(
                matches!(parsed, Ok(Request::Run(..))),
                "{spelling:?}: {parsed:?}"
            );
            assert_eq!(warnings, vec![IGNORING_CONTEXT.to_string()], "{spelling:?}");
        }
        // Printed as it is met, so a later bad option still finds it there.
        let (parsed, warnings) = parse_with_warnings(&["--context=x", "-q"]);
        assert!(parsed.is_err());
        assert_eq!(warnings.len(), 1);
    }

    /// Defect 1's regression test at the parser level: the spec comes out
    /// **uncompiled**, so nothing can quietly substitute a default for it.
    #[test]
    fn an_invalid_mode_is_not_diagnosed_or_replaced_during_parsing() {
        let (f, n) = run_parse(&["-m", "zzz"]);
        assert_eq!(f.mode, Some(OsString::from("zzz")));
        assert!(n.is_empty());
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// Defect 4's regression test. On this OS a FIFO name may hold any byte but
    /// `/` and NUL, and byte `0x80` alone is not valid UTF-8, so an operand
    /// containing it cannot be a `String` — `env::args()` would have panicked
    /// before `mkfifo` saw it.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args_quiet(&[OsString::from("-m"), OsString::from("600"), bad.clone()]).unwrap()
        {
            Request::Run(f, n) => {
                assert_eq!(f.mode, Some(OsString::from("600")));
                assert_eq!(n, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    /// The test above is `#[cfg(unix)]`, so on the development host — Windows —
    /// the regression test for the bug would not run at all. That is the same
    /// blind spot that let the bug survive, so it is closed rather than noted:
    /// Windows has its own argument no `String` can hold, an unpaired surrogate,
    /// which reaches the same `unwrap` in `env::args()` by a different route.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args_quiet(&[OsString::from("-m"), OsString::from("600"), bad.clone()]).unwrap()
        {
            Request::Run(f, n) => {
                assert_eq!(f.mode, Some(OsString::from("600")));
                assert_eq!(n, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    // --------------------------------------------------------------- mode --

    /// Every row measured against GNU coreutils 9.4. The arithmetic is pure, so
    /// it is checked on every host rather than only where a FIFO can be made.
    #[test]
    fn the_measured_mode_arithmetic() {
        let m = |spec: &str, umask_value: u32| mode_for(OsStr::new(spec), umask_value);

        // An octal mode is the mode, whatever the umask.
        for umask_value in [0o000, 0o022, 0o077, 0o002] {
            assert_eq!(m("666", umask_value), Ok(0o666));
            assert_eq!(m("700", umask_value), Ok(0o700));
            assert_eq!(m("777", umask_value), Ok(0o777));
        }

        // A symbolic clause that names no `who` *is* masked, which is what
        // proves the umask is passed through rather than merely zeroed.
        assert_eq!(m("a=,+w", 0o000), Ok(0o222));
        assert_eq!(m("a=,+w", 0o022), Ok(0o200));
        assert_eq!(m("a=,+w", 0o077), Ok(0o200));
        assert_eq!(m("a=,+w", 0o002), Ok(0o220));

        // …but one that names a `who` is not.
        assert_eq!(m("a=rwx", 0o077), Ok(0o777));
        assert_eq!(m("u=rw,go=", 0o022), Ok(0o600));

        // The two rows that separate `mkfifo` from `mkdir`. `+` adds nothing and
        // names no bits, so it yields the base — which is 0666 here and 0777
        // there. `+X` sees `dir = false`, and a FIFO with no execute bit already
        // set takes none, so it yields 0 where `mkdir` yields 0111.
        assert_eq!(m("+", 0o022), Ok(0o666));
        for umask_value in [0o000, 0o022, 0o077, 0o002] {
            assert_eq!(m("a=,+X", umask_value), Ok(0));
        }
        assert_eq!(m("=", 0o022), Ok(0));
    }

    /// Defect 1: every one of these used to become `0666` in silence.
    #[test]
    fn an_invalid_mode_is_an_error_not_a_default() {
        for spec in ["zzz", "garbage", "8", "u=q", "z+r", ",", "a", "+r,"] {
            assert_eq!(
                mode_for(OsStr::new(spec), 0o022),
                Err(ModeError::Invalid),
                "{spec}"
            );
        }
    }

    /// `mkfifo(2)` would drop these bits without saying so, which is why GNU
    /// checks for them rather than letting the kernel decide. Measured: all
    /// seven answer `mode must specify only file permission bits`.
    #[test]
    fn a_mode_outside_the_permission_bits_is_refused() {
        for spec in ["2755", "1755", "4755", "7777", "u+s", "g+s", "+t"] {
            assert_eq!(
                mode_for(OsStr::new(spec), 0o022),
                Err(ModeError::NotPermissionBits),
                "{spec}"
            );
        }
        // …and the two sentences are different, which is the whole reason
        // `ModeError` has two variants rather than being a bare `Option`.
        assert_eq!(ModeError::Invalid.sentence(), "invalid mode");
        assert_eq!(
            ModeError::NotPermissionBits.sentence(),
            "mode must specify only file permission bits"
        );
    }

    // ----------------------------------------------------------- creating --

    /// Run `make_all`, returning `(ok, diagnostics)`.
    fn run(mode: Option<&str>, names: &[&str]) -> (bool, String) {
        let owned: Vec<OsString> = names.iter().map(OsString::from).collect();
        let flags = MkfifoFlags {
            mode: mode.map(OsString::from),
        };
        let mut err: Vec<u8> = Vec::new();
        let ok = make_all(&flags, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    /// Defect 6: the referral used to be missing.
    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, msg) = run(None, &[]);
        assert!(!ok);
        assert!(msg.contains("missing operand"), "{msg}");
        assert!(msg.contains("Try 'mkfifo --help'"), "{msg}");
    }

    /// The measured ordering, and the reason [`MkfifoFlags::mode`] holds an
    /// uncompiled `OsString`.
    #[test]
    fn missing_operand_is_reported_before_an_invalid_mode() {
        let (ok, msg) = run(Some("zzz"), &[]);
        assert!(!ok);
        assert!(msg.contains("missing operand"), "{msg}");
        assert!(!msg.contains("invalid mode"), "{msg}");
    }

    /// Defect 1, end to end: the command fails, says so, and — this is the part
    /// that matters — **creates nothing**. The old code would have created two
    /// world-writable FIFOs here.
    #[test]
    fn a_bad_mode_is_fatal_before_anything_is_created() {
        let (ok, msg) = run(Some("zzz"), &["x", "y"]);
        assert!(!ok);
        // Exactly GNU's sentence: no operand named, no referral, one line.
        assert_eq!(msg, "mkfifo: invalid mode\n");
    }

    #[test]
    fn a_special_bit_mode_is_fatal_with_its_own_sentence() {
        let (ok, msg) = run(Some("2755"), &["x", "y"]);
        assert!(!ok);
        assert_eq!(msg, "mkfifo: mode must specify only file permission bits\n");
    }

    /// Defect 3: the reason used to be missing entirely. What the reason *is*
    /// depends on the platform — a host that has no `mkfifo(2)` says so — but
    /// there must always be one, and it must always be on the same line as the
    /// name.
    #[test]
    fn a_failure_names_the_fifo_and_says_why() {
        let dir = std::env::temp_dir().join(format!("mkfifo_test_nodir_{}", std::process::id()));
        let target = dir.join("p");
        let (ok, msg) = run(None, &[&target.to_string_lossy()]);
        assert!(!ok, "{msg}");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.starts_with("mkfifo: cannot create fifo "), "{msg:?}");
        // The name, then a colon, then a reason. The old message stopped at the
        // name.
        let tail = msg
            .rsplit_once("': ")
            .map(|(_, why)| why.trim_end().to_owned())
            .unwrap_or_default();
        assert!(!tail.is_empty(), "no reason on the end: {msg:?}");
    }

    /// Straight marks, not curly — the opposite of `mkdir`'s one message.
    /// Measured: ``mkfifo: cannot create fifo 'a': File exists``.
    #[test]
    fn the_failure_message_uses_straight_marks() {
        let (ok, msg) = run(None, &["/nosuchdir/definitely/not/here"]);
        assert!(!ok);
        assert!(msg.contains('\''), "no straight marks: {msg:?}");
        assert!(
            !msg.contains('\u{2018}') && !msg.contains('\u{2019}'),
            "curly marks crept in: {msg:?}"
        );
    }

    /// A name with a newline in it must not be able to add a line that looks
    /// like a second diagnostic from `mkfifo`.
    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        let (ok, msg) = run(None, &["/nosuchdir/a\nmkfifo: /etc: Permission denied"]);
        assert!(!ok);
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.contains(r"\n"), "the newline must be escaped: {msg:?}");
    }

    /// One failure must not abandon the rest — measured: `mkfifo a g` with `a`
    /// present reports `a`, still creates `g`, and exits 1. On a host with no
    /// `mkfifo(2)` both fail, which still proves the loop does not stop early.
    #[test]
    fn one_failure_does_not_abandon_the_others() {
        let (ok, msg) = run(None, &["/nosuchdir/a", "/nosuchdir/b"]);
        assert!(!ok);
        assert_eq!(msg.lines().count(), 2, "{msg:?}");
    }

    /// A NUL cannot cross into a C string, and truncating at it would create a
    /// FIFO under a *different* name than the one asked for.
    #[test]
    #[cfg(unix)]
    fn a_name_containing_a_nul_is_refused_rather_than_truncated() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(b"/tmp/a\0b".to_vec());
        let e = make_one(&bad, 0o666).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
    }
}
