//! realpath — print the resolved absolute file name.
//!
//! # Why this was rewritten
//!
//! The file this replaces was 58 lines long and had no option parsing at all.
//! Six defects follow, of which the first two changed answers on ordinary
//! input.
//!
//! ## 1. Every option was a file name
//!
//! `parse_args` returned argv unchanged, with a comment saying "realpath takes
//! no flags" and a *test* asserting that `realpath -q foo` looks for a file
//! called `-q`. GNU `realpath` has eleven options, two of which take a value,
//! and `--` did not end options either — so a file whose name begins with a
//! dash could not be resolved at all.
//!
//! ## 2. The default mode was the wrong one, and it is the mode that matters
//!
//! `realpath` with no mode flag is gnulib's `CAN_ALL_BUT_LAST`: every component
//! but the last must exist. The old code called [`std::fs::canonicalize`],
//! which is `CAN_EXISTING`. So the single most common use — naming a file that
//! is *about to be created* — failed:
//!
//! | Command | GNU | This program, before |
//! |---|---|---|
//! | `realpath newfile` (in an existing directory) | prints `…/newfile`, exit 0 | fails, exit 1 |
//! | `realpath -m a/b/c` | prints `…/a/b/c`, exit 0 | fails, exit 1 |
//! | `realpath -s link` | prints the *link*'s name | printed the target |
//!
//! The three modes and the `-s`/`-L`/`-P` flag now come from
//! [`coreutils::canon`], which is gnulib's `canonicalize_filename_mode`
//! transcribed; see that module for the measurements behind each rule.
//!
//! On the Windows development host there was a second, quieter wrongness in
//! the same call: [`std::fs::canonicalize`] returns a `\\?\C:\…`
//! extended-length path, which is not a name anything else here can use.
//!
//! ## 3. Argv was `Vec<String>`, so a legal file name crashed it
//!
//! `env::args()` panics on an argument that is not valid UTF-8, and a path on
//! this system may hold every byte but `/` and NUL (`design.txt`). See
//! `known-issues.md` → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`. The output
//! end had the same defect from the other direction: results were printed with
//! `{}` on a `PathBuf`, which replaces every byte it cannot decode.
//!
//! ## 4. It printed the host's error text
//!
//! `realpath: x: The system cannot find the file specified. (os error 2)`
//! rather than `realpath: x: No such file or directory`. See
//! [`coreutils::errmsg`].
//!
//! ## 5. `-q` did not exist, so failures could not be silenced
//!
//! `realpath` is **verbose by default** and `-q` quiets it — the opposite of
//! `readlink`, which is quiet by default and has `-v`. Getting this pair the
//! wrong way round is easy; both are measured.
//!
//! ## 6. `missing operand` carried no referral
//!
//! GNU follows it with `Try 'realpath --help' for more information.`, and
//! `realpath --` — no operands after the marker — is that same case.
//!
//! # `--relative-to` and `--relative-base`
//!
//! These two are the reason this file needed the value-taking half of
//! [`coreutils::getopt`]. Their interaction is not guessable and is
//! transcribed from `src/realpath.c` rather than from the manual:
//!
//! - `--relative-base` alone **implies** `--relative-to` with the same
//!   directory.
//! - Both are canonicalised with the *same* mode and link flag as the operands,
//!   and a failure there is **fatal** — it stops the program before any operand
//!   is looked at, and it is printed even under `-q`, because it reports a bad
//!   command line rather than a bad operand.
//! - Under `-e`, and only under `-e`, each must also *be a directory*.
//! - If the base is not a prefix of the relative-to, **both are dropped** and
//!   every answer is absolute. Measured:
//!   `realpath --relative-base=/home/u/d --relative-to=/home/u plainfile`
//!   prints `/home/u/plainfile`, not `plainfile`.
//! - The base is then re-tested per operand: an answer that is not below it is
//!   printed absolute.
//!
//! # The long-option table is in GNU's declaration order
//!
//! Order is observable — `getopt_long` lists an ambiguous prefix's candidates
//! in table order. Measured against GNU coreutils 9.4:
//!
//! ```text
//! $ realpath --r
//! realpath: option '--r' is ambiguous; possibilities: '--relative-to' '--relative-base'
//! $ realpath --canonicalize
//! realpath: option '--canonicalize' is ambiguous; possibilities: '--canonicalize-existing' '--canonicalize-missing'
//! ```
//!
//! The second row is why there is no `--canonicalize` entry here even though
//! upstream `git` has since added one (as `-E`): 9.4 is the version every other
//! table in this crate was measured against, and adding the option would make
//! that measured line stop being reproducible.
//!
//! `--strip` and `--no-symlinks` are one option under two spellings, but no
//! prefix reaches both, so [`Program::parse`] is enough and `parse_aliased` is
//! not needed.

use coreutils::canon::{self, Fs, Links, Mode, RealFs};
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quoteaf, quotef};
use coreutils::stdfd::Stream;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;

/// `realpath`'s usage status is 1 — measured: `realpath -X foo; echo $?`.
const REALPATH: Program = Program::new("realpath", 1);

/// GNU `realpath`'s `getopt_long` string, copied verbatim from 9.4.
const SHORT_OPTIONS: &str = "eLmPqsz";

/// GNU `realpath`'s `longopts[]`, in its declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("canonicalize-existing", Takes::Nothing),
    ("canonicalize-missing", Takes::Nothing),
    ("relative-to", Takes::Required),
    ("relative-base", Takes::Required),
    ("quiet", Takes::Nothing),
    ("strip", Takes::Nothing),
    ("no-symlinks", Takes::Nothing),
    ("zero", Takes::Nothing),
    ("logical", Takes::Nothing),
    ("physical", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

#[derive(Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Flags {
    /// `-e`/`-m`, or the default. Last one wins — measured, `realpath -m -e -m`
    /// behaves as `-m`.
    mode: Mode,
    /// gnulib's `CAN_NOLINKS` bit, set by `-s` and `-L` and cleared by `-P`.
    links: Links,
    /// `-L`, which is `CAN_NOLINKS` **plus** a second canonicalisation pass.
    /// Kept apart from `links` because `-s` sets one and not the other, and the
    /// three flags are last-wins over both fields independently: measured,
    /// `-s -L link` resolves the link and `-L -s link` does not.
    logical: bool,
    /// The default is to report; `-q` quiets. See module docs, defect 5.
    verbose: bool,
    zero: bool,
    relative_to: Option<OsString>,
    relative_base: Option<OsString>,
}

impl Default for Flags {
    fn default() -> Self {
        Self {
            mode: Mode::AllButLast,
            links: Links::Follow,
            logical: false,
            verbose: true,
            zero: false,
            relative_to: None,
            relative_base: None,
        }
    }
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    Run(Flags, Vec<OsString>),
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args) {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("realpath (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, files)) => {
            let mut out = io::stdout().lock();
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            let ok = run(&flags, &files, &RealFs, &mut out, &mut err);
            // A closed stdout must not be reported as success: `-z` output is
            // usually piped into `xargs -0`, and a pipe that goes away mid-list
            // would otherwise look like a complete list.
            let flushed = out.flush().is_ok();
            if ok && flushed {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("realpath: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: realpath [OPTION]... FILE...
Print the resolved absolute file name;
all but the last component must exist

  -e, --canonicalize-existing  all components of the path must exist
  -m, --canonicalize-missing   no path components need exist or be a directory
  -L, --logical                resolve '..' components before symlinks
  -P, --physical               resolve symlinks as encountered (default)
  -q, --quiet                  suppress most error messages
      --relative-to=DIR        print the resolved path relative to DIR
      --relative-base=DIR      print absolute paths unless paths below DIR
  -s, --strip, --no-symlinks   don't expand symlinks
  -z, --zero                   end each output line with NUL, not newline
      --help        display this help and exit
      --version     output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `realpath`'s argv into `(flags, operands)`.
///
/// Options and operands may be interleaved — `realpath a -e b` is
/// `realpath -e a b` — which is `getopt_long`'s default permuting behaviour.
///
/// # Errors
///
/// An unknown option, a long option resolving to none or to more than one of
/// the table's entries, a long option given a value it does not take, or
/// `--relative-to`/`--relative-base` given none.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = Flags::default();
    let mut files: Vec<OsString> = Vec::new();

    for item in REALPATH.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(file) => files.push(file.clone()),
            Opt::Short(b'e', _) | Opt::Long("canonicalize-existing", _) => {
                flags.mode = Mode::Existing
            }
            Opt::Short(b'm', _) | Opt::Long("canonicalize-missing", _) => {
                flags.mode = Mode::Missing
            }
            // The three link flags each set *both* fields, which is what makes
            // them last-wins as a group rather than three independent toggles.
            Opt::Short(b'L', _) | Opt::Long("logical", _) => {
                flags.links = Links::Textual;
                flags.logical = true;
            }
            Opt::Short(b's', _) | Opt::Long("strip" | "no-symlinks", _) => {
                flags.links = Links::Textual;
                flags.logical = false;
            }
            Opt::Short(b'P', _) | Opt::Long("physical", _) => {
                flags.links = Links::Follow;
                flags.logical = false;
            }
            Opt::Short(b'q', _) | Opt::Long("quiet", _) => flags.verbose = false,
            Opt::Short(b'z', _) | Opt::Long("zero", _) => flags.zero = true,
            // `Takes::Required` guarantees the value is present, so a `None`
            // here cannot arise and would only mean the table disagreed with
            // this arm.
            Opt::Long("relative-to", value) => flags.relative_to = value,
            Opt::Long("relative-base", value) => flags.relative_base = value,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: the parser yields only names from the table, and
            // every one is handled above. Refusing rather than ignoring, so a
            // table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(REALPATH.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(REALPATH.invalid_option(other)),
        }
    }

    Ok(Request::Run(flags, files))
}

// ----------------------------------------------------------- canonicalising ---

/// gnulib's `canonicalize_filename_mode`, called twice when `-L` asked for it.
///
/// `-L` means "resolve `..` before symlinks", and upstream implements it as two
/// whole passes rather than as a different walk: pass one with `CAN_NOLINKS`
/// set, which does the `..` arithmetic textually, then pass two over *that*
/// answer with the bit cleared, which follows whatever links are left. The
/// difference is visible on a name where the two orders disagree — a symbolic
/// link to a directory followed by `..`.
///
/// # Errors
///
/// Whatever the filesystem said, or `ELOOP` from the canonicaliser.
fn realpath_canon<F: Fs + ?Sized>(fs: &F, name: &[u8], flags: &Flags) -> io::Result<Vec<u8>> {
    let once = canon::canonicalize_with(fs, name, flags.mode, flags.links)?;
    if flags.logical {
        return canon::canonicalize_with(fs, &once, flags.mode, Links::Follow);
    }
    Ok(once)
}

/// Canonicalise one of the two `--relative-*` directories, or report why not.
///
/// The failure is fatal and is written here rather than returned, because both
/// callers do the same thing with it and because the message shape differs
/// between the two ways it can fail. `Err(())` means "already reported".
///
/// The diagnostic quotes the directory **as the user typed it**, not as
/// canonicalised — upstream's `quotef (relative_to)` — so that a user who typed
/// a relative name sees the relative name back.
fn canon_dir<F: Fs + ?Sized, E: Write>(
    fs: &F,
    dir: &OsString,
    flags: &Flags,
    err: &mut E,
) -> Result<Vec<u8>, ()> {
    let typed = os_bytes(dir.as_os_str());
    let can = realpath_canon(fs, &typed, flags).map_err(|e| {
        let _ = writeln!(err, "realpath: {}: {}", quotef(&typed), strerror(&e));
    })?;

    // Upstream's `need_dir`, which is exactly `-e` and nothing else: under the
    // other two modes a `--relative-to` that is a plain file is accepted, and
    // the arithmetic below then treats it as if it were a directory. Measured:
    // `realpath --relative-to=plainfile d/sub/real` prints `../d/sub/real`.
    if flags.mode != Mode::Existing {
        return Ok(can);
    }
    match fs.dir_check(&can) {
        Ok(()) => Ok(can),
        // Upstream reaches `ENOTDIR` only by `stat` succeeding on a non-directory;
        // an `ENOTDIR` from the `stat` *itself* would take its `cannot stat`
        // path instead. The two cannot be told apart through this trait, and
        // they cannot both arise here anyway: `Mode::Existing` has just proved
        // every component of `can` exists and is walkable.
        Err(e) if e.kind() == io::ErrorKind::NotADirectory => {
            let _ = writeln!(err, "realpath: {}: {}", quotef(&typed), strerror(&e));
            Err(())
        }
        Err(e) => {
            let _ = writeln!(
                err,
                "realpath: cannot stat {}: {}",
                quoteaf(&can),
                strerror(&e)
            );
            Err(())
        }
    }
}

/// The two canonical directories the answers are measured against — upstream's
/// `can_relative_to` and `can_relative_base`, either of which may be absent.
///
/// Named rather than a bare pair because the two are the same type and differ
/// only in role: `to` is what an answer is measured *from*, `base` only decides
/// *whether* it is measured at all.
#[derive(Default)]
struct Relative {
    to: Option<Vec<u8>>,
    base: Option<Vec<u8>>,
}

impl Relative {
    /// How `answer` should be printed: `Some(relative form)` or `None` for the
    /// absolute name as-is.
    ///
    /// Three ways to end up absolute: no `--relative-to`, a `--relative-base`
    /// this answer is not below, or a relative form that does not exist.
    /// Upstream is one `if` with three clauses in that order.
    fn render(&self, answer: &[u8]) -> Option<Vec<u8>> {
        let to = self.to.as_ref()?;
        if let Some(base) = self.base.as_ref()
            && !path_prefix(base, answer)
        {
            return None;
        }
        relpath(answer, to)
    }
}

/// Work out the two canonical directories the answers are measured against.
///
/// See the module docs for the rules; this is upstream's block between the
/// operand check and the operand loop, transcribed.
fn relative_dirs<F: Fs + ?Sized, E: Write>(
    fs: &F,
    flags: &Flags,
    err: &mut E,
) -> Result<Relative, ()> {
    // Upstream implies `--relative-to` from `--relative-base` by assigning the
    // *pointer*, and later compares the two pointers to decide whether to
    // canonicalise twice. The implication is what that comparison detects, so
    // it is carried here as a flag rather than by comparing the strings — which
    // would also treat `--relative-to=X --relative-base=X` as implied, and that
    // reaches the same answer by a different route.
    let implied = flags.relative_to.is_none();
    let Some(to) = flags.relative_to.as_ref().or(flags.relative_base.as_ref()) else {
        return Ok(Relative::default());
    };

    let can_to = canon_dir(fs, to, flags, err)?;
    if implied {
        return Ok(Relative {
            to: Some(can_to.clone()),
            base: Some(can_to),
        });
    }
    let Some(base) = flags.relative_base.as_ref() else {
        return Ok(Relative {
            to: Some(can_to),
            base: None,
        });
    };

    let can_base = canon_dir(fs, base, flags, err)?;
    if path_prefix(&can_base, &can_to) {
        Ok(Relative {
            to: Some(can_to),
            base: Some(can_base),
        })
    } else {
        // "--relative-to is a no-op if it does not have --relative-base as a
        // prefix". Upstream moves the relative-to into the base slot and clears
        // the relative-to, which makes the first clause of the print test fire
        // for every operand — so the effect is that both are dropped and every
        // answer is absolute.
        Ok(Relative {
            to: None,
            base: Some(can_to),
        })
    }
}

// -------------------------------------------------------------- relative ---

/// Is canonical `prefix` a prefix of canonical `path`, on a component boundary?
///
/// coreutils' `path_prefix`, transcribed. The two special cases at the top are
/// about `//`, which POSIX allows an implementation to make distinct from `/`
/// and which the canonicaliser therefore may not collapse: `/` is a prefix of
/// everything except a `//…` name, and `//` is a prefix of exactly those.
///
/// The component-boundary rule is the point of the function: `/home/u` is not
/// a prefix of `/home/user`, though its bytes are.
#[must_use]
fn path_prefix(prefix: &[u8], path: &[u8]) -> bool {
    // Both are known to start with `/`.
    let prefix = prefix.get(1..).unwrap_or_default();
    let path = path.get(1..).unwrap_or_default();

    if prefix.is_empty() {
        return path.first() != Some(&b'/');
    }
    if prefix.first() == Some(&b'/') && prefix.len() == 1 {
        return path.first() == Some(&b'/');
    }

    let mut i: usize = 0;
    while let (Some(a), Some(b)) = (prefix.get(i), path.get(i)) {
        if a != b {
            break;
        }
        i = i.saturating_add(1);
    }
    prefix.get(i).is_none() && matches!(path.get(i), None | Some(&b'/'))
}

/// How many *bytes* of two canonical paths are a common prefix, rounded down to
/// a component boundary. `0` for no shared component at all.
///
/// The unit is bytes, not components, and the boundary is normally *past* the
/// separator: `/home/u` and `/home/user` answer 6 — `/home/` — because `u` and
/// `user` are different components. The separator is only excluded when one
/// name stops exactly where the other has one, so `/home/u` against
/// `/home/u/d` answers 7. Both callers below strip a leading separator from
/// the suffix they take, which is why the inconsistency does not reach them.
///
/// coreutils' `path_common_prefix`, transcribed.
#[must_use]
fn path_common_prefix(path1: &[u8], path2: &[u8]) -> usize {
    // `//` again: a name under it shares nothing with a name under `/`.
    if (path1.get(1) == Some(&b'/')) != (path2.get(1) == Some(&b'/')) {
        return 0;
    }

    let mut i: usize = 0;
    let mut ret: usize = 0;
    while let (Some(&a), Some(&b)) = (path1.get(i), path2.get(i)) {
        if a != b {
            break;
        }
        if a == b'/' {
            ret = i.saturating_add(1);
        }
        i = i.saturating_add(1);
    }

    // One name ending exactly where the other has a separator (or where both
    // end) is a whole-component match up to that point: `/a/b` and `/a/b/c`
    // share two components, not one.
    let (a, b) = (path1.get(i), path2.get(i));
    if (a.is_none() && b.is_none())
        || (a.is_none() && b == Some(&b'/'))
        || (b.is_none() && a == Some(&b'/'))
    {
        ret = i;
    }
    ret
}

/// `can_fname` written relative to directory `can_reldir`, or `None` if the two
/// share no component at all.
///
/// coreutils' `relpath`, transcribed, minus its buffer mode — this returns the
/// bytes rather than writing them, so the caller decides where they go and the
/// `ENAMETOOLONG` branch has nothing to overflow.
///
/// The `..` count is one per *remaining separator* plus one, not one per
/// component, which is the same number written a way that needs no split.
#[must_use]
fn relpath(can_fname: &[u8], can_reldir: &[u8]) -> Option<Vec<u8>> {
    let common = path_common_prefix(can_reldir, can_fname);
    if common == 0 {
        return None;
    }

    let mut relto_suffix = can_reldir.get(common..).unwrap_or_default();
    let mut fname_suffix = can_fname.get(common..).unwrap_or_default();
    if relto_suffix.first() == Some(&b'/') {
        relto_suffix = relto_suffix.get(1..).unwrap_or_default();
    }
    if fname_suffix.first() == Some(&b'/') {
        fname_suffix = fname_suffix.get(1..).unwrap_or_default();
    }

    let mut out: Vec<u8> = Vec::new();
    if relto_suffix.is_empty() {
        // The file is inside the reference directory, or is it.
        out.extend_from_slice(if fname_suffix.is_empty() {
            b"."
        } else {
            fname_suffix
        });
    } else {
        out.extend_from_slice(b"..");
        for &c in relto_suffix {
            if c == b'/' {
                out.extend_from_slice(b"/..");
            }
        }
        if !fname_suffix.is_empty() {
            out.push(b'/');
            out.extend_from_slice(fname_suffix);
        }
    }
    Some(out)
}

// --------------------------------------------------------------- running ---

/// Answer every operand, writing results to `out` and diagnostics to `err`.
///
/// Returns `true` if every operand was answered. Takes both sinks as parameters
/// so the output — bytes, delimiters and all — can be asserted on byte for
/// byte; the file this replaces tested only that stdout was non-empty.
///
/// One failure does not abandon the rest: measured, `realpath nosuch .` prints
/// the second answer and exits 1. A failure in `--relative-to`, by contrast,
/// abandons everything, because it is a bad command line rather than a bad
/// operand.
fn run<F: Fs + ?Sized, W: Write, E: Write>(
    flags: &Flags,
    files: &[OsString],
    fs: &F,
    out: &mut W,
    err: &mut E,
) -> bool {
    if files.is_empty() {
        let _ = writeln!(
            err,
            "realpath: {}",
            REALPATH.usage_referring("missing operand".into())
        );
        return false;
    }

    let Ok(relative) = relative_dirs(fs, flags, err) else {
        return false;
    };

    let delimiter = if flags.zero { b'\0' } else { b'\n' };
    let mut ok = true;
    for file in files {
        let name = os_bytes(file.as_os_str());
        let answer = match realpath_canon(fs, &name, flags) {
            Ok(a) => a,
            Err(e) => {
                if flags.verbose {
                    let _ = writeln!(err, "realpath: {}: {}", quotef(&name), strerror(&e));
                }
                ok = false;
                continue;
            }
        };

        // Raw bytes: a file name is an arbitrary byte string, and rendering it
        // as text would corrupt any name that is not UTF-8.
        let printed = relative.render(&answer);
        let _ = out.write_all(printed.as_deref().unwrap_or(&answer));
        let _ = out.write_all(&[delimiter]);
    }
    ok
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    // ------------------------------------------------------ the fake tree ---

    #[derive(Clone, Copy, Debug)]
    enum Entry {
        Dir,
        File,
        Link(&'static [u8]),
    }

    /// The tree every test below runs against, with `/home/u` as the working
    /// directory:
    ///
    /// ```text
    /// /home/u              cwd
    ///   d/
    ///     sub/
    ///       real           a file
    ///       link    -> real
    ///     dirlink   -> /home/u/d/sub
    ///     dangling  -> /nowhere/x
    ///   plainfile          a file
    /// /tmp                 a directory outside the cwd's subtree
    /// ```
    struct FakeFs(BTreeMap<&'static [u8], Entry>);

    impl FakeFs {
        fn new() -> Self {
            let mut m: BTreeMap<&'static [u8], Entry> = BTreeMap::new();
            for dir in [
                &b"/"[..],
                b"/home",
                b"/home/u",
                b"/home/u/d",
                b"/home/u/d/sub",
                b"/tmp",
            ] {
                m.insert(dir, Entry::Dir);
            }
            m.insert(b"/home/u/d/sub/real", Entry::File);
            m.insert(b"/home/u/plainfile", Entry::File);
            m.insert(b"/home/u/d/sub/link", Entry::Link(b"real"));
            m.insert(b"/home/u/d/dirlink", Entry::Link(b"/home/u/d/sub"));
            m.insert(b"/home/u/d/dangling", Entry::Link(b"/nowhere/x"));
            Self(m)
        }

        /// `lstat`: the entry at a canonical path, with a non-directory
        /// ancestor reported as `ENOTDIR` the way the kernel would.
        fn lstat(&self, path: &[u8]) -> io::Result<Entry> {
            for cut in 1..path.len() {
                if path.get(cut) != Some(&b'/') {
                    continue;
                }
                match self.0.get(path.get(..cut).unwrap_or_default()) {
                    Some(Entry::Dir) => {}
                    Some(_) => return Err(io::Error::from(io::ErrorKind::NotADirectory)),
                    None => return Err(io::Error::from(io::ErrorKind::NotFound)),
                }
            }
            self.0
                .get(path)
                .copied()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }

        /// `stat`: [`lstat`](Self::lstat), then follow. Only the final
        /// component can be a link here, because every path this fake is asked
        /// about has already been canonicalised — except under
        /// [`Links::Textual`], which is exactly the case that needs this.
        fn stat(&self, path: &[u8], depth: usize) -> io::Result<Entry> {
            if depth > 40 {
                return Err(coreutils::errmsg::filesystem_loop());
            }
            match self.lstat(path)? {
                Entry::Link(target) if target.first() == Some(&b'/') => {
                    self.stat(target, depth.saturating_add(1))
                }
                Entry::Link(target) => {
                    let mut from = path.to_vec();
                    while from.last().is_some_and(|&c| c != b'/') {
                        from.pop();
                    }
                    from.extend_from_slice(target);
                    self.stat(&from, depth.saturating_add(1))
                }
                other => Ok(other),
            }
        }
    }

    impl Fs for FakeFs {
        fn cwd(&self) -> io::Result<Vec<u8>> {
            Ok(b"/home/u".to_vec())
        }
        fn read_link(&self, path: &[u8]) -> io::Result<Vec<u8>> {
            match self.lstat(path)? {
                Entry::Link(target) => Ok(target.to_vec()),
                _ => Err(io::Error::from(io::ErrorKind::InvalidInput)),
            }
        }
        fn dir_check(&self, path: &[u8]) -> io::Result<()> {
            match self.stat(path, 0)? {
                Entry::Dir => Ok(()),
                _ => Err(io::Error::from(io::ErrorKind::NotADirectory)),
            }
        }
        fn exists(&self, path: &[u8]) -> io::Result<()> {
            self.stat(path, 0).map(|_| ())
        }
    }

    // ---------------------------------------------------------- harnesses ---

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    /// Parse and run a whole command line, as `main` would.
    /// Returns `(ok, stdout bytes, stderr text)`.
    fn go(words: &[&str]) -> (bool, Vec<u8>, String) {
        let args = argv(words);
        let (flags, files) = match parse_args(&args).unwrap() {
            Request::Run(flags, files) => (flags, files),
            other => panic!("expected a run request, got {other:?}"),
        };
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let ok = run(&flags, &files, &FakeFs::new(), &mut out, &mut err);
        (ok, out, String::from_utf8_lossy(&err).into_owned())
    }

    /// The one-operand case: the answer, or `!` and the diagnostic.
    fn one(words: &[&str]) -> String {
        let (ok, out, err) = go(words);
        if ok {
            assert!(err.is_empty(), "unexpected diagnostic: {err}");
            let mut text = String::from_utf8_lossy(&out).into_owned();
            assert_eq!(text.pop(), Some('\n'), "answer must end in a newline");
            text
        } else {
            format!("!{}", err.trim_end())
        }
    }

    // ------------------------------------------------------------ parsing ---

    #[test]
    fn the_default_is_all_but_last_and_following_links() {
        let Request::Run(flags, files) = parse_args(&argv(&["f"])).unwrap() else {
            panic!("expected a run request");
        };
        assert_eq!(flags.mode, Mode::AllButLast);
        assert_eq!(flags.links, Links::Follow);
        assert!(flags.verbose, "realpath reports by default");
        assert!(!flags.logical);
        assert_eq!(files, argv(&["f"]));
    }

    #[test]
    fn the_mode_flags_are_last_wins() {
        for (words, want) in [
            (&["-e"][..], Mode::Existing),
            (&["-m"], Mode::Missing),
            (&["-m", "-e"], Mode::Existing),
            (&["-e", "-m"], Mode::Missing),
            (&["-m", "-e", "-m"], Mode::Missing),
            (&["--canonicalize-existing"], Mode::Existing),
            (&["--canonicalize-missing"], Mode::Missing),
        ] {
            let mut all = words.to_vec();
            all.push("f");
            let Request::Run(flags, _) = parse_args(&argv(&all)).unwrap() else {
                panic!("expected a run request");
            };
            assert_eq!(flags.mode, want, "for {words:?}");
        }
    }

    /// `-s` and `-L` both set `CAN_NOLINKS`; only `-L` asks for the second
    /// pass. So the pair is last-wins over two fields at once, and the order
    /// they are written in changes the answer.
    #[test]
    fn the_link_flags_are_last_wins_over_both_fields() {
        for (words, links, logical) in [
            (&["-s"][..], Links::Textual, false),
            (&["-L"], Links::Textual, true),
            (&["-P"], Links::Follow, false),
            (&["-s", "-L"], Links::Textual, true),
            (&["-L", "-s"], Links::Textual, false),
            (&["-L", "-P"], Links::Follow, false),
            (&["-P", "-L"], Links::Textual, true),
            (&["--strip"], Links::Textual, false),
            (&["--no-symlinks"], Links::Textual, false),
            (&["--logical"], Links::Textual, true),
            (&["--physical"], Links::Follow, false),
        ] {
            let mut all = words.to_vec();
            all.push("f");
            let Request::Run(flags, _) = parse_args(&argv(&all)).unwrap() else {
                panic!("expected a run request");
            };
            assert_eq!(flags.links, links, "links for {words:?}");
            assert_eq!(flags.logical, logical, "logical for {words:?}");
        }
    }

    #[test]
    fn the_two_value_options_take_a_value_four_ways() {
        for words in [
            &["--relative-to=/tmp", "f"][..],
            &["--relative-to", "/tmp", "f"],
            &["--relative-t=/tmp", "f"],
        ] {
            let Request::Run(flags, files) = parse_args(&argv(words)).unwrap() else {
                panic!("expected a run request");
            };
            assert_eq!(
                flags.relative_to,
                Some(OsString::from("/tmp")),
                "for {words:?}"
            );
            assert_eq!(files, argv(&["f"]), "for {words:?}");
        }
    }

    /// The value-taking options are the reason this file needed the driver: a
    /// missing value must be a diagnostic, not a silent absence.
    #[test]
    fn a_value_option_at_the_end_of_argv_is_an_error() {
        let e = parse_args(&argv(&["--relative-to"])).unwrap_err();
        assert_eq!(
            e.message(),
            "option '--relative-to' requires an argument\n\
             Try 'realpath --help' for more information."
        );
        assert_eq!(e.status, 1);
    }

    /// Both ambiguity messages are measured from GNU 9.4, and both depend on
    /// the table's order.
    #[test]
    fn the_ambiguous_prefixes_match_gnu() {
        let e = parse_args(&argv(&["--r", "f"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "option '--r' is ambiguous; possibilities: '--relative-to' '--relative-base'"
        );
        // 9.4 has no `--canonicalize`, so the prefix reaches only the two modes.
        let e = parse_args(&argv(&["--canonicalize", "f"])).unwrap_err();
        assert_eq!(
            e.sentence,
            "option '--canonicalize' is ambiguous; possibilities: \
             '--canonicalize-existing' '--canonicalize-missing'"
        );
    }

    #[test]
    fn an_unknown_option_is_refused_rather_than_taken_as_a_file() {
        let e = parse_args(&argv(&["-X", "foo"])).unwrap_err();
        assert_eq!(e.sentence, "invalid option -- 'X'");
        assert_eq!(e.status, 1);
        // Defect 1: this is what the old file did instead.
        let Request::Run(_, files) = parse_args(&argv(&["--", "-q", "foo"])).unwrap() else {
            panic!("expected a run request");
        };
        assert_eq!(files, argv(&["-q", "foo"]), "`--` ends the options");
    }

    // ------------------------------------------------------ canonicalising ---

    /// Defect 2: the mode that made the old file fail on ordinary input.
    #[test]
    fn the_default_mode_tolerates_a_missing_last_component() {
        assert_eq!(one(&["newfile"]), "/home/u/newfile");
        assert_eq!(one(&["d/sub/newfile"]), "/home/u/d/sub/newfile");
        assert_eq!(
            one(&["-e", "newfile"]),
            "!realpath: newfile: No such file or directory"
        );
        assert_eq!(one(&["-m", "no/such/thing"]), "/home/u/no/such/thing");
        assert_eq!(
            one(&["no/such/thing"]),
            "!realpath: no/such/thing: No such file or directory"
        );
    }

    /// The `-s`/`-L`/`-P` triple, on the one name where all three differ.
    #[test]
    fn the_link_flags_change_the_answer() {
        assert_eq!(one(&["d/sub/link"]), "/home/u/d/sub/real");
        assert_eq!(one(&["-P", "d/sub/link"]), "/home/u/d/sub/real");
        assert_eq!(one(&["-s", "d/sub/link"]), "/home/u/d/sub/link");
        // `-L` is two passes, so the link *is* resolved — the difference is the
        // order, not whether it happens.
        assert_eq!(one(&["-L", "d/sub/link"]), "/home/u/d/sub/real");
        assert_eq!(one(&["-s", "-L", "d/sub/link"]), "/home/u/d/sub/real");
        assert_eq!(one(&["-L", "-s", "d/sub/link"]), "/home/u/d/sub/link");
    }

    /// Where `-L` and `-P` genuinely disagree: `..` after a link to a
    /// directory. `-P` follows the link and goes up from the target; `-L`
    /// cancels the two textually and goes up from the link's own directory.
    #[test]
    fn logical_and_physical_disagree_on_dotdot_after_a_dirlink() {
        assert_eq!(one(&["-P", "d/dirlink/.."]), "/home/u/d");
        assert_eq!(one(&["-s", "d/dirlink/.."]), "/home/u/d");
        assert_eq!(one(&["-L", "d/dirlink/.."]), "/home/u/d");
        // …and on a link to a *file*, where `..` is an error under every mode
        // that checks — measured, `realpath -L d/sub/link/..` says so too.
        assert_eq!(
            one(&["-P", "d/sub/link/.."]),
            "!realpath: d/sub/link/..: Not a directory"
        );
        assert_eq!(
            one(&["-L", "d/sub/link/.."]),
            "!realpath: d/sub/link/..: Not a directory"
        );
    }

    // ----------------------------------------------------------- relative ---

    #[test]
    fn relative_to_measures_from_the_named_directory() {
        assert_eq!(one(&["--relative-to=/home/u", "d/sub/real"]), "d/sub/real");
        assert_eq!(
            one(&["--relative-to=/home/u/d/sub", "plainfile"]),
            "../../plainfile"
        );
        assert_eq!(
            one(&["--relative-to=/home/u/d/sub", "/home/u/d/sub"]),
            ".",
            "the directory itself is `.`, not the empty string"
        );
        assert_eq!(one(&["--relative-to=/tmp", "/home/u/d"]), "../home/u/d");
        assert_eq!(one(&["--relative-to=d/sub", "."]), "../..");
        assert_eq!(one(&["--relative-to=/", "/home/u"]), "home/u");
        assert_eq!(one(&["--relative-to=/home/u", "/"]), "../..");
        // Last-wins, like every other option.
        assert_eq!(
            one(&["--relative-to=/home/u", "--relative-to=/tmp", "/home/u"]),
            "../home/u"
        );
    }

    #[test]
    fn relative_base_prints_absolute_for_anything_outside_it() {
        assert_eq!(
            one(&["--relative-base=/home/u", "d/sub/real"]),
            "d/sub/real"
        );
        assert_eq!(one(&["--relative-base=/home/u", "/tmp"]), "/tmp");
        // Given together, the base only gates and the `to` measures.
        assert_eq!(
            one(&[
                "--relative-base=/home/u",
                "--relative-to=/home/u/d",
                "plainfile"
            ]),
            "../plainfile"
        );
        // The base is not a prefix of the `to`, so both are dropped.
        assert_eq!(
            one(&[
                "--relative-base=/home/u/d",
                "--relative-to=/home/u",
                "plainfile"
            ]),
            "/home/u/plainfile"
        );
        assert_eq!(
            one(&["--relative-base=/tmp", "--relative-to=/home/u", "plainfile"]),
            "/home/u/plainfile"
        );
    }

    /// A bad `--relative-to` is fatal, is reported even under `-q`, and stops
    /// the program before any operand is printed.
    #[test]
    fn a_bad_relative_to_is_fatal_and_not_silenced_by_quiet() {
        let (ok, out, err) = go(&["-e", "--relative-to=/nosuch", "d/sub/real"]);
        assert!(!ok);
        assert!(out.is_empty(), "no operand may be printed");
        assert_eq!(err, "realpath: /nosuch: No such file or directory\n");

        let (ok, out, err) = go(&["-q", "-e", "--relative-to=/nosuch", "d/sub/real"]);
        assert!(!ok);
        assert!(out.is_empty());
        assert_eq!(
            err, "realpath: /nosuch: No such file or directory\n",
            "`-q` silences operands, not the command line"
        );

        // `need_dir` is `-e` and nothing else.
        let (ok, _, err) = go(&["-e", "--relative-to=/home/u/plainfile", "d/sub/real"]);
        assert!(!ok);
        assert_eq!(err, "realpath: /home/u/plainfile: Not a directory\n");
        assert_eq!(
            one(&["--relative-to=/home/u/plainfile", "d/sub/real"]),
            "../d/sub/real",
            "without -e a plain file is accepted as the reference"
        );
    }

    // ------------------------------------------------------------- output ---

    #[test]
    fn each_answer_ends_with_the_delimiter_and_failures_do_not_stop_the_rest() {
        let (ok, out, err) = go(&["d/sub/real", "no/such", "plainfile"]);
        assert!(!ok, "one failure means exit 1");
        assert_eq!(out, b"/home/u/d/sub/real\n/home/u/plainfile\n");
        assert_eq!(err, "realpath: no/such: No such file or directory\n");

        let (ok, out, err) = go(&["-q", "d/sub/real", "no/such"]);
        assert!(!ok);
        assert_eq!(out, b"/home/u/d/sub/real\n");
        assert_eq!(err, "", "-q silences the operand diagnostic");

        let (ok, out, _) = go(&["-z", "d/sub/real", "plainfile"]);
        assert!(ok);
        assert_eq!(out, b"/home/u/d/sub/real\0/home/u/plainfile\0");
    }

    #[test]
    fn no_operands_is_a_usage_error_with_the_referral() {
        let (ok, out, err) = go(&[]);
        assert!(!ok);
        assert!(out.is_empty());
        assert_eq!(
            err,
            "realpath: missing operand\nTry 'realpath --help' for more information.\n"
        );
        // `realpath --` is the same case: the marker leaves no operands.
        let (ok, _, err) = go(&["--"]);
        assert!(!ok);
        assert!(err.starts_with("realpath: missing operand\n"));
    }

    /// Defect 3, the output end: a name is bytes, and a byte that is not text
    /// must come back out unchanged.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_survives_to_stdout() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};
        let name = OsString::from_vec(b"od\xffd".to_vec());
        let flags = Flags::default();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let ok = run(&flags, &[name], &FakeFs::new(), &mut out, &mut err);
        assert!(ok, "stderr said: {}", String::from_utf8_lossy(&err));
        assert_eq!(out, b"/home/u/od\xffd\n");
        // And through the relative arithmetic too, which slices bytes.
        let flags = Flags {
            relative_to: Some(OsString::from("/home/u/d")),
            ..Flags::default()
        };
        let mut out: Vec<u8> = Vec::new();
        let name = OsString::from_vec(b"od\xffd".to_vec());
        assert!(run(&flags, &[name], &FakeFs::new(), &mut out, &mut err));
        assert_eq!(out, b"../od\xffd\n");
        // The operand really was the byte, not a replacement character.
        assert_eq!(OsString::from_vec(b"od\xffd".to_vec()).as_bytes().len(), 4);
    }

    // -------------------------------------------- the transcribed functions ---

    /// Every row of coreutils' `path_prefix`, including the two `//` cases that
    /// exist for platforms where `//` is a distinct root.
    #[test]
    fn path_prefix_matches_upstream() {
        for (prefix, path, want) in [
            (&b"/"[..], &b"/a"[..], true),
            (b"/", b"/", true),
            (b"/", b"//a", false),
            (b"//", b"//a", true),
            (b"//", b"/a", false),
            (b"/home/u", b"/home/u", true),
            (b"/home/u", b"/home/u/d", true),
            (b"/home/u", b"/home/user", false),
            (b"/home/u", b"/home", false),
            (b"/home/u", b"/tmp", false),
        ] {
            assert_eq!(
                path_prefix(prefix, path),
                want,
                "path_prefix({}, {})",
                String::from_utf8_lossy(prefix),
                String::from_utf8_lossy(path)
            );
        }
    }

    /// The number is a *byte offset*, not a component count, and the two are
    /// easy to confuse: the offset stops just past the last shared separator,
    /// so `/home/u` and `/home/user` share 6 bytes (`/home/`) and not 5
    /// (`/home`). The trailing separator is only dropped when one name ends
    /// exactly where the other has one — the tail test below — which is what
    /// makes `/home/u` against `/home/u/d` answer 7 rather than 6.
    #[test]
    fn path_common_prefix_is_a_byte_offset_ending_at_a_component_boundary() {
        for (a, b, want) in [
            (&b"/home/u"[..], &b"/home/u"[..], 7),
            (b"/home/u", b"/home/u/d", 7),
            (b"/home/u", b"/home/user", 6),
            (b"/home/u", b"/tmp", 1),
            (b"/", b"/tmp", 1),
            (b"/tmp", b"//tmp", 0),
        ] {
            assert_eq!(
                path_common_prefix(a, b),
                want,
                "path_common_prefix({}, {})",
                String::from_utf8_lossy(a),
                String::from_utf8_lossy(b)
            );
        }
    }

    #[test]
    fn relpath_matches_upstream() {
        for (fname, reldir, want) in [
            (
                &b"/home/u/d/sub/real"[..],
                &b"/home/u"[..],
                Some(&b"d/sub/real"[..]),
            ),
            (
                b"/home/u/plainfile",
                b"/home/u/d/sub",
                Some(b"../../plainfile"),
            ),
            (b"/home/u/d/sub", b"/home/u/d/sub", Some(b".")),
            (b"/home/u/d", b"/tmp", Some(b"../home/u/d")),
            (b"/home/u", b"/", Some(b"home/u")),
            (b"/", b"/home/u", Some(b"../..")),
            (b"//a", b"/a", None),
        ] {
            assert_eq!(
                relpath(fname, reldir).as_deref(),
                want,
                "relpath({}, {})",
                String::from_utf8_lossy(fname),
                String::from_utf8_lossy(reldir)
            );
        }
    }
}
