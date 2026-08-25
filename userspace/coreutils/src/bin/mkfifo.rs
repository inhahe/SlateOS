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
//! # Options this implementation does not have
//!
//! `-Z` and `--context`, which set an SELinux/SMACK security context. They are
//! recognised and refused by name rather than ignored — silently dropping a
//! security context is the same class of defect as defect 1 above — and they
//! stay in [`LONG_OPTIONS`] because the table is what decides whether an
//! abbreviation is ambiguous.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quoteaf_os};
use coreutils::stdfd::{self, Stream};
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::process::ExitCode;

// The file-mode creation mask. POSIX offers no way to read it without writing
// it, which is why there is no read-only spelling and why `take_umask` has to
// zero it to find out what it was.
#[cfg(unix)]
unsafe extern "C" {
    fn umask(mask: u32) -> u32;
    fn mkfifo(path: *const u8, mode: u32) -> i32;
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
            println!("mkfifo (SlateOS coreutils) 0.1.0");
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

/// Parse `mkfifo`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `mkfifo a -m 600 b` is
/// `mkfifo -m 600 a b` — which is `getopt_long`'s default permuting behaviour.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, a
/// long option given a value it does not take, or `-m` with no value.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = MkfifoFlags::default();
    let mut names: Vec<OsString> = Vec::new();

    for item in MKFIFO.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'm', value) | Opt::Long("mode", value) => flags.mode = value,
            // Refused rather than ignored: see the module docs.
            Opt::Short(b'Z', _) => return Err(unimplemented_short(b'Z')),
            Opt::Long(name @ "context", _) => return Err(unimplemented_long(name)),
            Opt::Short(other, _) => return Err(MKFIFO.invalid_option(other)),
            Opt::Long(other, _) => return Err(unimplemented_long(other)),
            // A lone `-` arrives here, not as an option: `mkfifo` has no
            // standard-input operand for it to mean anything else, so it is a
            // FIFO called `-`.
            Opt::Operand(name) => names.push(name.clone()),
        }
    }

    Ok(Request::Run(flags, names))
}

/// The diagnostic for an option that GNU `mkfifo` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-Z` is not a typo, and telling
/// the user it is invalid sends them to check the spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    MKFIFO.usage_referring(format!(
        "option -{} is not implemented by this mkfifo",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    MKFIFO.usage_referring(format!(
        "option '--{name}' is not implemented by this mkfifo"
    ))
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

/// Read the file-mode creation mask **and zero it**, once per process.
///
/// Zeroing is GNU's, and it only happens when `-m` was given: having applied the
/// mask itself in [`mode_for`], `mkfifo` must stop the kernel applying it a
/// second time to the same mode. Without `-m` the mask is left alone and the
/// kernel is the only thing that applies it — which is why `mkfifo q` is `0644`
/// under `umask 022` while `mkfifo -m 666 q` is `0666`.
///
/// Doing it exactly once is this implementation's. A second call would read the
/// zero the first one wrote; the real binary calls [`make_all`] once, so only a
/// test could make that happen, and a cached value makes it impossible either
/// way.
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
/// A Windows host still runs every parsing and diagnostic test in this file, and
/// the arithmetic in [`mode_for`] is pure, so it can be checked there against
/// every umask by passing one in.
#[cfg(unix)]
fn current_umask() -> u32 {
    take_umask()
}

#[cfg(not(unix))]
fn current_umask() -> u32 {
    0
}

/// Resolve `-m`'s argument against the umask in force.
fn resolve_mode(spec: &OsStr) -> Result<u32, ModeError> {
    mode_for(spec, current_umask())
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

    // SAFETY: `c_path` is NUL-terminated, holds no interior NUL, and outlives
    // the call. `mkfifo` is POSIX, takes a borrowed C string it does not retain,
    // and reports failure through `errno` — which is what `last_os_error` reads
    // immediately below, before anything else can overwrite it.
    let ret = unsafe { mkfifo(c_path.as_ptr(), mode) };
    if ret == 0 {
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

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (MkfifoFlags, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, n) => (
                f,
                n.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
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
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
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
        // `--c` prefixes only `--context`, so it resolves — and is then refused
        // as unimplemented rather than as unrecognised.
        assert!(fail(&["--c"]).sentence.contains("not implemented"));
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

    /// Ignored, `-Z` would silently drop a security context — the same class of
    /// defect as `-m` silently falling back to `0666`.
    #[test]
    fn the_security_context_options_are_refused_by_name() {
        assert!(fail(&["-Z", "p"]).sentence.contains("not implemented"));
        assert!(
            fail(&["--context", "p"])
                .sentence
                .contains("not implemented")
        );
        assert!(
            fail(&["--context=x", "p"])
                .sentence
                .contains("not implemented")
        );
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
        match parse_args(&[OsString::from("-m"), OsString::from("600"), bad.clone()]).unwrap() {
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
        match parse_args(&[OsString::from("-m"), OsString::from("600"), bad.clone()]).unwrap() {
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
