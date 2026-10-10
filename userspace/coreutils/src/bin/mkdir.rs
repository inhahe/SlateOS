//! mkdir — make directories.
//!
//! # What it is
//!
//! A port of GNU coreutils 9.4's `mkdir.c`: its `main`, its `make_ancestor`
//! and its `announce_mkdir`, over gnulib's `make_dir_parents`,
//! `mkancesdirs` and `dirchownmod`, which live in [`coreutils::mkdirp`]
//! because `install -d` is built on them too. `scripts/mkdir-diff.sh` holds
//! it to GNU 9.4 built from source, comparing for every case the tree each
//! side leaves -- every path's type and mode -- as well as what it said.
//!
//! # The two umasks, and the bits `-m` mentions
//!
//! This is the part that is easy to get nearly right, and the harness's first
//! run found 14 cases where a hand-written walk had. Upstream reads the umask
//! once and then runs under two others:
//!
//! * **`umask_ancestor`** -- the umask with the owner's write and search bits
//!   taken out (`umask & ~0300`), in force while `-p` makes an ancestor at
//!   `0777`. An ancestor the owner cannot write into or step through cannot
//!   hold the next component, so `umask 300; mkdir -p a/b` makes `a` at
//!   `0777` and `a/b` at `0477`. It applies whether or not `-m` was given.
//! * **`umask_self`** -- `umask & ~mode`, in force while the operand itself is
//!   made: a bit `-m` asked for is not masked away by `mkdir(2)`.
//!
//! The operand's mode is then finished by `dirchownmod`, which enforces only
//! the bits `-m` *mentioned* (gnulib's `mode_bits`) and leaves the rest as the
//! kernel made them. That is observable in a set-group-ID parent, where the
//! kernel gives every new directory the bit: `mkdir -m 755 sgid/d` keeps it
//! (a four-digit mode does not mention it), `mkdir -m 00755 sgid/d` clears it
//! (five digits mention every bit), and `mkdir -m g-s sgid/d` is `0777` --
//! the mode was computed from `0777`, and the umask had no say because the
//! bit the clause names is not a permission bit.
//!
//! The umask is set only from `main`. It belongs to the whole process, and
//! under `cargo test` the process is every test in this file, so [`make_all`]
//! takes the setter as a parameter and its tests hand it one that changes
//! nothing; their plans are made from the umask in force, under which the two
//! umasks coincide, so nothing needed changing.
//!
//! # `-Z` and `--context`
//!
//! As upstream on a kernel with neither SELinux nor SMACK, which SlateOS is:
//! `-Z`, or `--context` without a value, asks for the default context and is
//! accepted in silence -- there is no context to set -- and `--context=CTX`
//! names one, so it is warned about and the directory made regardless.
//! Measured: `mkdir --context=x d` prints `mkdir: warning: ignoring --context;
//! it requires an SELinux/SMACK-enabled kernel` and exits 0. These used to be
//! refused by name, on the argument that dropping a context silently is a
//! defect; but nothing is dropped silently -- the named context is warned
//! about -- and the refusal broke the scripts that pass `-Z` portably, which
//! work on every Linux without SELinux.
//!
//! # Earlier defects, recorded so they are not reintroduced
//!
//! 1. **It read argv as `String`**, so it *panicked* on a directory name
//!    holding a byte that is not valid UTF-8 -- which on this OS is a legal
//!    name, by design (`design.txt`: a path may hold every byte but `/` and
//!    NUL). See `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.
//!    Argv is `OsString` and stays that way to the syscall.
//!
//! 2. **No long option worked, including `--help`, and `--` was not an
//!    end-of-options marker**, so a directory whose name begins with a dash
//!    could not be made at all; short options could not be bundled either.
//!    Options go through [`coreutils::getopt`] now.
//!
//! 3. **The diagnostic used the wrong quoting style.** The quoting style is a
//!    property of the individual message, not of the utility, and `mkdir` is
//!    the odd one out among its neighbours. Measured under `LANG=C.UTF-8`, GNU
//!    coreutils 9.4:
//!
//!    | Message | Marks |
//!    |---|---|
//!    | ``mkdir: cannot create directory ‘a’: File exists`` | curly ([`quote`][coreutils::quote::quote]) |
//!    | ``mkdir: created directory 'v1'`` (`-v`) | straight ([`quoteaf`][coreutils::quote::quoteaf]) |
//!    | ``rmdir: failed to remove 'nosuch'`` | straight |
//!    | ``cp: cannot stat 'nosuch'`` | straight |
//!
//!    The two `mkdir` rows are the point: one program, two styles, in the same
//!    run. Anyone "fixing" this file for consistency with `rm` and `cp` will
//!    make it wrong again.
//!
//! 4. **`missing operand` carried no referral, and unknown options were
//!    reported in a shape no other utility uses** (`mkdir: unknown option:
//!    -q`). Both are the option library's wording now.
//!
//! 5. **`-p` was a walk of its own**, by [`Path::ancestors`], and it was
//!    wrong in four ways the harness found at once: `mkdir -p f/x` with `f` a
//!    file said `File exists` where upstream says `Not a directory` (the step
//!    into `f` fails, not the `mkdir`); an ancestor without search permission
//!    was not where the walk stopped; `mkdir -p ./a/./b/.` failed; and the
//!    umask had no say over the ancestors unless `-m` was given.

use coreutils::diag;
use coreutils::fsattr::Owner;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::mkdirp::{self, Target};
use coreutils::quote::{os_bytes, quote_os, quoteaf_os};
use coreutils::stdfd::{self, Stream};
use std::cell::RefCell;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `mkdir`'s usage status is 1 — measured: `mkdir -q z; echo $?` prints 1. See
/// [`coreutils::getopt::Error`] for the handful of utilities that differ.
const MKDIR: Program = Program::new("mkdir", 1);

/// GNU `mkdir`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. An empty prefix matches everything, so `mkdir --=x` prints the
/// whole table:
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

/// Upstream's warning for a context *named* on a kernel with neither SELinux
/// nor SMACK, printed from inside its option loop -- so before a `missing
/// operand`, and before whatever option comes next.
const IGNORING_CONTEXT: &str =
    "warning: ignoring --context; it requires an SELinux/SMACK-enabled kernel";

/// The default mode, `S_IRWXUGO`: what the umask narrows when `-m` is absent,
/// what `-m` is computed from when it is present, and what every ancestor is
/// asked for.
const S_IRWXUGO: u32 = 0o777;

/// The owner's write and search bits, which `umask_ancestor` never takes away.
const S_IWUSR_IXUSR: u32 = 0o300;

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MkdirFlags {
    /// `-p`: upstream's `make_ancestor_function`.
    parents: bool,
    /// `-v`: name each directory on **stdout** as it is created.
    ///
    /// Stdout, not stderr, and that is measured rather than assumed —
    /// `mkdir -v d 2>/dev/null | cat` shows the line and `mkdir -v d 1>/dev/null`
    /// shows nothing. A `-v` run that half fails writes the successes to one
    /// stream and the failure to the other.
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

/// The funnel: upstream's `atexit (close_stdout)`, which checks standard
/// output and then standard error on every exit path at once -- an output or
/// a diagnostic that did not arrive is status 1, as `mkdir: write error:
/// ...` for the first. The descriptor guard is what lets a closed one be seen
/// at all; the runtime used to answer it with a quiet `/dev/null`. See
/// [`stdfd::close_stdout`].
fn main() -> ExitCode {
    stdfd::restore();
    let mut out = Stream::stdout();
    let earned = run_main(&mut out);
    stdfd::close_stdout("mkdir", out, earned)
}

fn run_main(out: &mut Stream) -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut warnings = Vec::new();
    let parsed = parse_args(&args, &mut warnings);
    // In the order they arose and before whatever ended the parse: upstream
    // prints each from inside its option loop.
    for w in &warnings {
        diag!("mkdir: {w}");
    }
    match parsed {
        Ok(Request::Help) => {
            // Never an error: the stream records it for the funnel.
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            // As above.
            let _ = out.write_all(b"mkdir (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, dirs)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            // Read without being written: `umask::current` asks `/proc`, so a
            // run that needs neither `-p` nor `-m` leaves the mask untouched,
            // as upstream's does.
            let Some(plan) = prepare(&flags, &dirs, coreutils::umask::current(), &mut err) else {
                return ExitCode::from(1);
            };
            if plan.sets_umask {
                // Upstream's `umask (options.umask_self)`; the mask it
                // replaced is the one `plan` was made from.
                let _ = coreutils::umask::set(plan.umask_self);
            }
            let mut set_umask = |mask: u32| {
                // As above: the replaced mask is `plan`'s to know already.
                let _ = coreutils::umask::set(mask);
            };
            if make_all(&plan, &dirs, out, &mut err, &mut set_umask) {
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

  -m, --mode=MODE   set file mode (as in chmod), not a=rwx - umask
  -p, --parents     no error if existing, make parent directories as needed,
                    with their file modes unaffected by any -m option.
  -v, --verbose     print a message for each created directory
  -Z                accepted and ignored: SlateOS has no security contexts
      --context[=CTX]  accepted; a CTX is warned about and ignored
      --help        display this help and exit
      --version     output version information and exit

To create a directory whose name starts with a '-', for example '-foo',
use one of these commands:
  mkdir -- -foo
  mkdir ./-foo
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `mkdir`'s argv into `(flags, operands)`, appending to `warnings`, in
/// order, what upstream's option loop would have printed on the way.
///
/// Options and operands may be interleaved — `mkdir a -p b` is `mkdir -p a b` —
/// which is `getopt_long`'s default permuting behaviour.
///
/// # Errors
///
/// An unknown option, or a long option given a value it does not take or not
/// given one it needs.
fn parse_args(args: &[OsString], warnings: &mut Vec<String>) -> Result<Request, getopt::Error> {
    let mut flags = MkdirFlags::default();
    let mut dirs: Vec<OsString> = Vec::new();

    // The shared driver: `-m 700`, `-m700`, `--mode=700` and `--mode 700` are
    // four spellings of one thing and the driver already knows all four.
    for item in MKDIR.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'p', _) | Opt::Long("parents", _) => flags.parents = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            Opt::Short(b'm', value) | Opt::Long("mode", value) => flags.mode = value,
            // The default context, on a kernel that has none: nothing to set,
            // and upstream says nothing. See the module docs.
            Opt::Short(b'Z', _) => {}
            Opt::Long("context", value) => {
                // `--context=` names a context too: an empty `optarg` is
                // still not a null one. Measured.
                if value.is_some() {
                    warnings.push(IGNORING_CONTEXT.to_string());
                }
            }
            // Unreachable: every option in the two tables is handled above.
            Opt::Short(other, _) => return Err(MKDIR.invalid_option(other)),
            Opt::Long(other, _) => {
                return Err(MKDIR.unrecognized_option(format!("--{other}").as_bytes()));
            }
            // A lone `-` arrives here, not as an option: `mkdir` has no
            // standard-input operand for it to mean anything else, so it is a
            // directory called `-`.
            Opt::Operand(dir) => dirs.push(dir.clone()),
        }
    }

    Ok(Request::Run(flags, dirs))
}

// -------------------------------------------------------------------- plan --

/// Upstream's `struct mkdir_options`, less the security context: everything
/// `main` works out before the first directory is made.
#[derive(Clone, Copy)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Plan {
    /// `-p`.
    parents: bool,
    /// `-v`.
    verbose: bool,
    /// The mode for each operand's own directory: `0777`, or `-m`'s mode
    /// computed from `0777` under the umask.
    mode: u32,
    /// Which bits of [`Self::mode`] `-m` had an opinion about -- `mode_adjust`'s
    /// `*pmode_bits`, `0` without `-m`. `dirchownmod` enforces these and no
    /// others.
    mode_bits: u32,
    /// The umask while an ancestor is made: the umask less `0300`.
    umask_ancestor: u32,
    /// The umask while the operand itself is made: the umask less the bits
    /// `-m` asked for.
    umask_self: u32,
    /// Whether upstream touches the umask at all: only under `-p` or `-m`.
    sets_umask: bool,
}

/// The arithmetic of upstream's `main` after its option loop, with the umask
/// passed in rather than read -- pure, so it is tested under every umask
/// without touching the process's. `None` for a mode that does not compile.
fn plan_for(flags: &MkdirFlags, umask_value: u32) -> Option<Plan> {
    let mut plan = Plan {
        parents: flags.parents,
        verbose: flags.verbose,
        mode: S_IRWXUGO,
        mode_bits: 0,
        umask_ancestor: umask_value,
        umask_self: umask_value,
        sets_umask: false,
    };
    if flags.parents || flags.mode.is_some() {
        plan.sets_umask = true;
        plan.umask_ancestor = umask_value & !S_IWUSR_IXUSR;
        if let Some(spec) = &flags.mode {
            let changes = modechange::compile(&os_bytes(spec))?;
            // From `0777`, as a directory -- so `+X` always sees an execute
            // bit to keep -- under the umask, which a clause naming no `who`
            // still answers to: `mkdir -m 'a=,+w' d` is `0200` under umask
            // 022 and `0222` under 000.
            let adjusted = modechange::adjust(S_IRWXUGO, true, umask_value, &changes);
            plan.mode = adjusted.mode;
            plan.mode_bits = adjusted.mode_bits;
            plan.umask_self = umask_value & !adjusted.mode;
        }
    }
    Some(plan)
}

/// The checks upstream makes between its option loop and the first
/// directory, in its order: `missing operand` (with the referral), then an
/// `-m` that does not compile (without one). The plan, or `None` once the
/// complaint is written.
fn prepare(
    flags: &MkdirFlags,
    dirs: &[OsString],
    umask_value: u32,
    err: &mut dyn Write,
) -> Option<Plan> {
    if dirs.is_empty() {
        let _ = writeln!(
            err,
            "mkdir: {}",
            MKDIR.usage_referring("missing operand".into())
        );
        return None;
    }
    let plan = plan_for(flags, umask_value);
    if plan.is_none()
        && let Some(spec) = &flags.mode
    {
        // Four utilities in this tree print four different sentences for
        // this, all measured against GNU 9.4: `chmod: invalid mode: ‘zzz’`
        // (with a colon), `install: invalid mode ‘zzz’`, `mkfifo: invalid
        // mode` (operand dropped entirely) and this one. No referral.
        let _ = writeln!(err, "mkdir: invalid mode {}", quote_os(spec));
    }
    plan
}

// ---------------------------------------------------------------- creating --

/// Every operand, in order: upstream's `savewd_process_files` calling
/// `process_dir`. `-v` lines go to `out` as each directory is made and
/// failures to `err`; one failure does not abandon the rest -- measured,
/// `mkdir a g` with `a` present reports `a`, still makes `g`, and exits 1.
///
/// `set_umask` is how an ancestor is made under `umask_ancestor`: `main`'s
/// sets the process's mask, a test's does not -- see the module docs.
fn make_all(
    plan: &Plan,
    dirs: &[OsString],
    out: &mut dyn Write,
    err: &mut dyn Write,
    set_umask: &mut dyn FnMut(u32),
) -> bool {
    let target = Target {
        mode: plan.mode,
        mode_bits: plan.mode_bits,
        owner: Owner::default(),
        preserve_existing: true,
    };
    // Shared by the two callbacks, which `make_dir_parents` holds at once and
    // calls one at a time.
    let out = RefCell::new(out);
    let mut ok = true;
    for dir in dirs {
        let dir = Path::new(dir);
        let mut announce = |made: &Path| announce_mkdir(made, plan.verbose, *out.borrow_mut());
        let result = if plan.parents {
            let mut make =
                |ancestor: &Path| make_ancestor(ancestor, plan, set_umask, *out.borrow_mut());
            mkdirp::make_dir_parents(dir, Some(&mut make), target, &mut announce)
        } else {
            mkdirp::make_dir_parents(dir, None, target, &mut announce)
        };
        if let Err(failure) = result {
            // `quote`: curly marks, unlike the `-v` line's. Module docs,
            // defect 3. The subject is the ancestor the walk stopped at, when
            // that is where it stopped: `mkdir -p f/g/h` with `f` a file names
            // `f`, a name `f/g/h` was never reached through.
            let subject = quote_os(failure.subject(dir).as_os_str());
            let _ = writeln!(err, "mkdir: {}", failure.sentence(&subject));
            ok = false;
        }
    }
    ok
}

/// Upstream's `make_ancestor`: one missing ancestor, asked for at `0777` under
/// `umask_ancestor`, and announced once it exists.
///
/// The umask is switched only when the two differ, as upstream switches it,
/// and put back whether or not the `mkdir` worked.
fn make_ancestor(
    dir: &Path,
    plan: &Plan,
    set_umask: &mut dyn FnMut(u32),
    out: &mut dyn Write,
) -> io::Result<()> {
    let switch = plan.umask_ancestor != plan.umask_self;
    if switch {
        set_umask(plan.umask_ancestor);
    }
    let made = coreutils::copy::create_dir_with_mode(dir, S_IRWXUGO);
    if switch {
        set_umask(plan.umask_self);
    }
    made?;
    announce_mkdir(dir, plan.verbose, out);
    Ok(())
}

/// Upstream's `announce_mkdir`: under `-v`, `prog_fprintf (stdout, "created
/// directory %s", quoteaf (dir))`. Straight marks; module docs, defect 3.
///
/// A failed write is the stream's to remember and `close_stdout`'s to report;
/// the directories are still made, as upstream's are.
fn announce_mkdir(dir: &Path, verbose: bool, out: &mut dyn Write) {
    if verbose {
        let _ = writeln!(
            out,
            "mkdir: created directory {}",
            quoteaf_os(dir.as_os_str())
        );
    }
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
    use std::fs;
    use std::path::PathBuf;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// The parse, and the warnings it made on the way.
    fn parse_with_warnings(items: &[&str]) -> (Result<Request, getopt::Error>, Vec<String>) {
        let mut warnings = Vec::new();
        let parsed = parse_args(&args(items), &mut warnings);
        (parsed, warnings)
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (MkdirFlags, Vec<String>) {
        match parse_with_warnings(items).0.unwrap() {
            Request::Run(f, d) => (
                f,
                d.iter().map(|o| o.to_string_lossy().into_owned()).collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_with_warnings(items).0.unwrap_err()
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

    /// Defect 2 in the module docs: this used to answer `unknown option: --`,
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

    /// Also defect 2: every long option was refused, `--help` included.
    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_with_warnings(&["--help"]).0.unwrap(), Request::Help);
        assert_eq!(
            parse_with_warnings(&["--version"]).0.unwrap(),
            Request::Version
        );
    }

    /// `--v` must stay ambiguous between `--verbose` and `--version`, which is
    /// why the table keeps every option upstream has.
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
        // `--c` prefixes only `--context`: accepted, and with no value, silent.
        let (parsed, warnings) = parse_with_warnings(&["--c", "z"]);
        assert!(matches!(parsed, Ok(Request::Run(..))), "{parsed:?}");
        assert!(warnings.is_empty(), "{warnings:?}");
        // `--m` prefixes only `--mode`. Measured: `mkdir --m 700 z` creates
        // `z` at 0700.
        assert_eq!(
            run_parse(&["--m", "700", "z"]).0.mode,
            Some(OsString::from("700"))
        );
    }

    /// The two sentences glibc uses for a missing value. Measured: `mkdir
    /// --mode` and `mkdir -m` say these, each with the referral.
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

    /// `-Z`, and `--context` without a value, ask for the default context,
    /// which a kernel without SELinux does not have: accepted in silence, as
    /// upstream accepts them there. Measured: `mkdir -Z d` makes `d`, exit 0.
    #[test]
    fn the_default_context_is_accepted_in_silence() {
        for spelling in [
            &["-Z", "a"][..],
            &["--context", "a"][..],
            &["-pvZ", "a"][..],
        ] {
            let (parsed, warnings) = parse_with_warnings(spelling);
            match parsed {
                Ok(Request::Run(_, d)) => assert_eq!(d, vec![OsString::from("a")], "{spelling:?}"),
                other => panic!("{spelling:?}: {other:?}"),
            }
            assert!(warnings.is_empty(), "{spelling:?}: {warnings:?}");
        }
    }

    /// A context *named* is warned about, once per naming, and the run goes
    /// on. `--context=` counts: an empty `optarg` is not a null one. Measured.
    #[test]
    fn a_named_context_is_warned_about_and_ignored() {
        for spelling in [
            &["--context=user_u:object_r:tmp_t:s0", "a"][..],
            &["--context=", "a"][..],
        ] {
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

    #[test]
    fn the_two_spellings_of_the_verbose_option() {
        for spelling in [&["-v", "a"][..], &["--verbose", "a"][..]] {
            let (f, d) = run_parse(spelling);
            assert!(f.verbose, "{spelling:?}");
            assert_eq!(d, vec!["a"]);
        }
        let (f, d) = run_parse(&["-pv", "a"]);
        assert!(f.parents && f.verbose);
        assert_eq!(d, vec!["a"]);
        assert!(!run_parse(&["a"]).0.verbose);
    }

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
    /// in which two mistakes are reported is observable: `mkdir -m zzz` with
    /// no operands answers `missing operand`, not `invalid mode`.
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

    /// The regression test for defect 1. Byte `0x80` alone is not valid UTF-8,
    /// so an operand containing it cannot be a `String` at all.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_with_warnings_os(&[OsString::from("-p"), bad.clone()]) {
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
        let e = parse_args(&[bad], &mut Vec::new()).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    /// On the Windows development host the argument no `String` can hold is an
    /// unpaired surrogate, which reaches the same `unwrap` in `env::args()` by
    /// a different route -- so the regression test runs there too.
    #[test]
    #[cfg(windows)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0061, 0xD800, 0x0062]);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_with_warnings_os(&[OsString::from("-p"), bad.clone()]) {
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
        let e = parse_args(&[bad], &mut Vec::new()).unwrap_err();
        assert!(
            e.sentence.starts_with("unrecognized option"),
            "{:?}",
            e.sentence
        );
    }

    fn parse_with_warnings_os(argv: &[OsString]) -> Request {
        parse_args(argv, &mut Vec::new()).unwrap()
    }

    // --------------------------------------------------------------- plan --

    fn flags(parents: bool, mode: Option<&str>) -> MkdirFlags {
        MkdirFlags {
            parents,
            verbose: false,
            mode: mode.map(OsString::from),
        }
    }

    fn plan(mode: &str, umask_value: u32) -> Plan {
        plan_for(&flags(false, Some(mode)), umask_value).expect("a valid mode")
    }

    /// Without `-p` or `-m` the umask is not touched at all, and the mode is
    /// `0777` for the kernel to narrow. Upstream leaves both umask fields
    /// unset there; these are the values that make "unset" harmless.
    #[test]
    fn neither_option_leaves_the_umask_alone() {
        let p = plan_for(&flags(false, None), 0o022).unwrap();
        assert!(!p.sets_umask);
        assert_eq!((p.mode, p.mode_bits), (0o777, 0));
        assert_eq!((p.umask_ancestor, p.umask_self), (0o022, 0o022));
    }

    /// `-p` alone: the ancestors' umask gives the owner write and search back,
    /// and the operand's is the umask itself. Measured under `umask 300`:
    /// `mkdir -p a/b` makes `a` at `0777` and `a/b` at `0477`.
    #[test]
    fn dash_p_gives_the_ancestors_their_own_umask() {
        for (umask_value, ancestor) in [
            (0o000, 0o000),
            (0o022, 0o022),
            (0o077, 0o077),
            (0o300, 0o000),
            (0o700, 0o400),
            (0o777, 0o477),
        ] {
            let p = plan_for(&flags(true, None), umask_value).unwrap();
            assert!(p.sets_umask);
            assert_eq!(p.umask_ancestor, ancestor, "umask {umask_value:03o}");
            assert_eq!(p.umask_self, umask_value, "umask {umask_value:03o}");
            // The mode an ancestor ends up with, as `ls` would show it.
            assert_eq!(
                0o777 & !p.umask_ancestor,
                (0o777 & !umask_value) | 0o300,
                "umask {umask_value:03o}"
            );
        }
    }

    /// The mode `-m` computes, every row measured against GNU 9.4.
    #[test]
    fn the_measured_mode_arithmetic() {
        // An octal mode is the mode, whatever the umask: the base is 0777 and
        // `=` is implied, and `umask_self` takes the bits it asked for out of
        // the umask so `mkdir(2)` cannot mask them away again.
        for umask_value in [0o000, 0o022, 0o077, 0o002] {
            assert_eq!(plan("700", umask_value).mode, 0o700);
            assert_eq!(plan("777", umask_value).mode, 0o777);
            assert_eq!(plan("777", umask_value).umask_self, 0);
            assert_eq!(plan("700", umask_value).umask_self, umask_value & 0o077);
        }

        // ...but a *symbolic* clause that names no `who` is masked, which is
        // what proves the umask is passed through rather than merely zeroed.
        assert_eq!(plan("a=,+w", 0o000).mode, 0o222);
        assert_eq!(plan("a=,+w", 0o022).mode, 0o200);
        assert_eq!(plan("a=,+w", 0o077).mode, 0o200);
        assert_eq!(plan("a=,+w", 0o002).mode, 0o220);

        // A `who` of its own is *not* masked.
        assert_eq!(plan("a=,u+w", 0o077).mode, 0o200);
        assert_eq!(plan("a=,o+w", 0o002).mode, 0o002);

        // `X` on a directory always finds an execute bit to keep. Measured:
        // `mkdir -m 'a=,+X' d` is 0111 where `mkfifo -m 'a=,+X' p` is 0000.
        assert_eq!(plan("a=,+X", 0o000).mode, 0o111);

        // The special bits, and which of them a mode *mentions*: a four-digit
        // mode mentions the set-ID bits only when it sets them, five digits
        // mention every bit. That is what decides whether a set-group-ID
        // parent's bit survives.
        assert_eq!(plan("2755", 0o022).mode, 0o2755);
        assert_eq!(plan("755", 0o022).mode_bits & 0o6000, 0);
        assert_eq!(plan("00755", 0o022).mode_bits & 0o6000, 0o6000);
        assert_eq!(plan("g-s", 0o022).mode, 0o777);
        assert_eq!(plan("g-s", 0o022).mode_bits, 0o2000);
        assert_eq!(plan("u=rwx,go=", 0o000).mode, 0o700);
    }

    /// Measured one by one against GNU 9.4, because the boundary is not where
    /// it looks: `+` and `=` are **accepted**.
    #[test]
    fn the_boundary_between_a_valid_and_an_invalid_mode() {
        for spec in ["zzz", "8", "u=q", "z+r", ",", "a", "+r,"] {
            assert!(
                plan_for(&flags(false, Some(spec)), 0o022).is_none(),
                "{spec} must not compile"
            );
        }
        // `+` names no bits, so the umask has nothing to take from it and
        // `umask_self` is clear: `mkdir -m + t` under `umask 022` is 0777.
        let p = plan("+", 0o022);
        assert_eq!((p.mode, p.umask_self), (0o777, 0));
        // `=` with no `who` clears everything. Measured: `mkdir -m = t` is 0.
        assert_eq!(plan("=", 0o022).mode, 0);
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

    /// `base/rel`, joined with `/` whatever the host: the walk splits on `/`,
    /// as upstream's does, so a test that wants to exercise it says so.
    fn under(base: &Path, rel: &str) -> PathBuf {
        let mut s = base.as_os_str().to_owned();
        s.push("/");
        s.push(rel);
        PathBuf::from(s)
    }

    /// Run [`prepare`] and [`make_all`] under the umask in force, returning
    /// `(ok, stdout, stderr)`. The umask setter must not be called: the plan
    /// is made from the umask in force, under which no switch is needed --
    /// and if one were, a test that changed the process's mask would be
    /// changing every other test's too.
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
        let ok = match prepare(&flags, &owned, coreutils::umask::current(), &mut err) {
            Some(plan) => make_all(&plan, &owned, &mut out, &mut err, &mut |m| {
                panic!("the umask would have been set to {m:03o}")
            }),
            None => false,
        };
        (
            ok,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    }

    fn run(parents: bool, dirs: &[&Path]) -> (bool, String) {
        run_with_mode(parents, None, dirs)
    }

    fn run_with_mode(parents: bool, mode: Option<&str>, dirs: &[&Path]) -> (bool, String) {
        let (ok, out, err) = run_all(parents, false, mode, dirs);
        assert_eq!(out, "", "a run without -v must write nothing to stdout");
        (ok, err)
    }

    /// The `-v` line for `path`, built the way the code builds it.
    fn verbose_line(path: &Path) -> String {
        format!(
            "mkdir: created directory {}\n",
            quoteaf_os(path.as_os_str())
        )
    }

    /// Defect 4: the referral used to be missing.
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
        let deep = under(&d, "b/c");
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

    /// Defect 3. The marks are curly here and straight in `rm`, `cp`, `ln` and
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
    /// like a second diagnostic from `mkdir`. The name goes under a parent that
    /// does not exist, so the creation fails on its own.
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

    /// Under `-p`, GNU names **the component that failed**, not the operand,
    /// and says why the walk could not go on rather than why `mkdir` could
    /// not: `touch f; mkdir -p f/g/h` answers ``cannot create directory ‘f’:
    /// Not a directory``. The `mkdir` of `f` said `File exists`; the step into
    /// it said `ENOTDIR`, and that is the one upstream keeps.
    #[test]
    fn dash_p_names_the_component_that_failed_and_why_the_walk_stopped() {
        let d = scratch("component");
        let f = d.join("f");
        fs::write(&f, b"x").unwrap();
        let deep = under(&d, "f/g/h");
        let (ok, msg) = run(true, &[&deep]);
        assert!(!ok, "{msg}");
        // Compared through `quote_os`, the same rendering the message itself
        // uses: on the Windows host a path's backslashes come back escaped.
        let named_f = format!(
            "cannot create directory {}: ",
            coreutils::quote::quote_os(under(&d, "f").as_os_str())
        );
        assert!(msg.contains(&named_f), "must name f: {msg:?}");
        assert!(
            !msg.contains(&coreutils::quote::quote_os(deep.as_os_str())),
            "must not name the operand: {msg:?}"
        );
        #[cfg(unix)]
        assert!(msg.ends_with("Not a directory\n"), "{msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// `.` components are skipped and a final `.` is a directory that exists,
    /// which `-p` excuses. Measured: `mkdir -p ./a/./b/.` exits 0, silent, and
    /// makes `a` and `a/b`.
    #[test]
    fn dot_components_are_walked_through() {
        let d = scratch("dots");
        let (ok, msg) = run(true, &[&under(&d, "./a/./b/.")]);
        assert!(ok, "{msg}");
        assert!(msg.is_empty(), "{msg}");
        assert!(d.join("a").join("b").is_dir());
        let _ = fs::remove_dir_all(&d);
    }

    // ------------------------------------------------------------ verbose --

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

    /// `-p` reports **every** component it creates, innermost last, each by the
    /// path leading to it. Measured: `mkdir -p -v x/y/z` prints `'x'`,
    /// `'x/y'`, `'x/y/z'`. A component that already existed is not reported.
    #[test]
    fn verbose_under_parents_reports_every_component_it_creates() {
        let d = scratch("v_chain");
        let x = under(&d, "x");
        let y = under(&d, "x/y");
        let z = under(&d, "x/y/z");
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

        let w = under(&d, "x/y/w");
        let (ok, out, err) = run_all(true, true, None, &[&w]);
        assert!(ok, "{err}");
        assert_eq!(out, verbose_line(&w));

        let (ok, out, err) = run_all(true, true, None, &[&z]);
        assert!(ok, "{err}");
        assert_eq!(out, "");
        let _ = fs::remove_dir_all(&d);
    }

    /// A run that half fails writes the successes to stdout and the failure to
    /// stderr, and keeps going. Measured: `mkdir -v ok1 /nope/nah ok2`.
    #[test]
    fn verbose_splits_successes_and_failures_across_the_two_streams() {
        let d = scratch("v_split");
        let ok1 = d.join("ok1");
        let nope = under(&d, "absent/nah");
        let ok2 = d.join("ok2");
        let (ok, out, err) = run_all(false, true, None, &[&ok1, &nope, &ok2]);
        assert!(!ok, "a failed operand must still fail the run");
        assert_eq!(out, format!("{}{}", verbose_line(&ok1), verbose_line(&ok2)));
        assert!(
            err.contains(&coreutils::quote::quote_os(nope.as_os_str())),
            "stderr must name the failure: {err:?}"
        );
        assert!(ok1.is_dir() && ok2.is_dir(), "the run must not abandon");
        let _ = fs::remove_dir_all(&d);
    }

    /// Under `-p`, the components made on the way to a failure were still made,
    /// so they are reported. Measured: with a 300-character leaf, `mkdir -p -v
    /// newmid/<long>` prints ``created directory 'newmid'`` and *then* the
    /// `File name too long` error.
    #[test]
    fn verbose_reports_what_was_built_before_a_failure() {
        let d = scratch("v_partial");
        let mid = under(&d, "newmid");
        let leaf = under(&d, &format!("newmid/{}", "z".repeat(300)));

        let (ok, out, err) = run_all(true, true, None, &[&leaf]);
        assert!(!ok, "the over-long leaf must fail: {out}");
        assert_eq!(out, verbose_line(&mid), "the intermediate must be reported");
        assert!(!err.is_empty(), "the failure must be reported");
        assert!(mid.is_dir(), "and the intermediate really was created");

        let (ok, out, err) = run_all(true, true, None, &[&under(&d, &"y".repeat(300))]);
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
    /// nothing to report.
    #[test]
    fn a_bad_mode_leaves_verbose_silent() {
        let d = scratch("v_badmode");
        let a = d.join("a");
        let (ok, out, err) = run_all(false, true, Some("zzz"), &[&a]);
        assert!(!ok);
        assert_eq!(out, "");
        assert_eq!(err, "mkdir: invalid mode \u{2018}zzz\u{2019}\n");
        assert!(!a.exists(), "nothing may be created after a bad mode");
        let _ = fs::remove_dir_all(&d);
    }

    // --------------------------------------------------------------- mode --

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
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let before = 0o777 & !coreutils::umask::current();
            assert_eq!(
                fs::metadata(&a).unwrap().permissions().mode() & 0o7777,
                before
            );
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// `-p` gives the ancestors their mode and the operand `-m`'s. Measured:
    /// `umask 022; mkdir -p -m 700 a/b/c` leaves `a` and `a/b` at `755` and
    /// only `a/b/c` at `700`.
    #[test]
    #[cfg(unix)]
    fn dash_p_gives_the_ancestors_their_mode() {
        use std::os::unix::fs::PermissionsExt;

        let d = scratch("parentmode");
        let deep = under(&d, "a/b/c");
        let (ok, msg) = run_with_mode(true, Some("700"), &[&deep]);
        assert!(ok, "{msg}");

        let umask_value = coreutils::umask::current();
        let ancestor = (0o777 & !umask_value) | 0o300;
        let mode_of = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o7777;
        assert_eq!(mode_of(&under(&d, "a")), ancestor);
        assert_eq!(mode_of(&under(&d, "a/b")), ancestor);
        assert_eq!(mode_of(&deep), 0o700);
        let _ = fs::remove_dir_all(&d);
    }

    /// The tests above run under the process's umask, so they cannot reach a
    /// plan whose two umasks differ. This one checks what `make_ancestor`
    /// asks of the setter when they do: the ancestors' mask before the
    /// `mkdir`, the operand's after it -- and back again even when the
    /// `mkdir` fails.
    #[test]
    fn make_ancestor_switches_the_umask_and_puts_it_back() {
        let d = scratch("switch");
        let p = plan_for(&flags(true, None), 0o300).unwrap();
        assert_ne!(p.umask_ancestor, p.umask_self);
        let mut calls = Vec::new();
        let mut sink = Vec::new();
        make_ancestor(&d.join("a"), &p, &mut |m| calls.push(m), &mut sink).unwrap();
        assert_eq!(calls, vec![0o000, 0o300]);

        calls.clear();
        // `a` exists now, so the `mkdir` fails -- and the mask is still put back.
        assert!(make_ancestor(&d.join("a"), &p, &mut |m| calls.push(m), &mut sink).is_err());
        assert_eq!(calls, vec![0o000, 0o300]);

        // Equal masks: no switch at all, as upstream.
        let same = plan_for(&flags(true, None), 0o022).unwrap();
        calls.clear();
        make_ancestor(&d.join("b"), &same, &mut |m| calls.push(m), &mut sink).unwrap();
        assert!(calls.is_empty(), "{calls:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// `prepare`'s two refusals, and that a good mode gets a plan.
    #[test]
    fn prepare_refuses_in_upstreams_order() {
        let mut err = Vec::new();
        let dirs = vec![OsString::from("d")];
        assert!(prepare(&flags(false, Some("u=rwx,go=")), &dirs, 0o022, &mut err).is_some());
        assert!(err.is_empty());
        assert!(prepare(&flags(false, Some("zzz")), &dirs, 0o022, &mut err).is_none());
        assert_eq!(
            String::from_utf8_lossy(&err),
            "mkdir: invalid mode \u{2018}zzz\u{2019}\n"
        );
    }
}
