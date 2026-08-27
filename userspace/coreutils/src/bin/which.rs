//! `which` -- locate a command, the way GNU which 2.21 does.
//!
//! # What was here before
//!
//! 157 lines with nine defects, four of which made it answer wrongly rather
//! than merely answer less:
//!
//! 1. **`env::args()` collected into `Vec<String>`**, which is a literal
//!    `unwrap` on a non-UTF-8 argument. `which $'\xff'` aborted before the
//!    first statement of the program. That is what brought the file into the
//!    argv sweep; the rest was found on the way.
//! 2. **`env::var("PATH").unwrap_or_default()`**, which turns a `PATH` that is
//!    not valid UTF-8 into the *empty* one. Every lookup then reports "not
//!    found" and the diagnostic names an empty search list, so the failure
//!    reads as "your command does not exist" rather than "I could not read
//!    your PATH". A path element here may hold every byte but `/` and NUL.
//! 3. **The probe was `Path::exists`**, so a *directory* named `ls` on the
//!    search path, or a non-executable file, was reported as the command.
//!    Measured: GNU rejects both -- a directory is never a match, and mode
//!    `0644` is not, while mode `0111` is, and a named pipe with `+x` is.
//!    The test is `access(X_OK)` on something that is not a directory, not
//!    "the name exists".
//! 4. **A command containing `/` was "treated as a literal path"**, per its own
//!    comment. It is not: GNU splits it at the *last* slash and searches the
//!    left half for the right half, which is why `which bin/nosuch` says
//!    `no nosuch in (./bin)` and not `no bin/nosuch`.
//! 5. **A bare `which` exited 1 in silence.** GNU writes the whole help to
//!    stderr and exits **255**.
//! 6. **The status was 1 however many commands were missing.** GNU's is the
//!    *count*: two missing is 2, and 300 missing is 44, because `exit` keeps
//!    the low byte.
//! 7. **`println!`**, which panics on a closed stdout; the panic message then
//!    fails to print for the same reason and the runtime aborts, so
//!    `which ls >&-` exited 134 where GNU exits 1.
//! 8. **No options at all** -- not `--help`, not `--version`, not `-a`.
//! 9. The `split_path` helper split on `:` and handed back `&str`, so an empty
//!    element stayed empty rather than meaning the current directory.
//!
//! # The reference
//!
//! GNU which 2.21 (`gnu-which` 2.21+dfsg-4build1), measured under WSL. It is
//! *not* the `which` most Linux distributions install by default -- Debian's
//! is debianutils' shell script, which spells its diagnostics differently and
//! exits 2 -- and the message shape this file has always produced is the GNU
//! one, so the GNU one is what it is held to.
//!
//! The behaviours that are not guessable, all measured:
//!
//! | Command line | Answer |
//! |---|---|
//! | `PATH=` (empty string) | *no* directories searched; `in ()` |
//! | `PATH=:` | *two* directories, both the current one |
//! | `PATH` unset | no directories; `in ((null))` -- glibc's null `%s` |
//! | `PATH=bin`, cwd `/w` | found, printed **absolute**: `/w/bin/cmd` |
//! | `PATH=~/bin` | tilde expanded from `$HOME` |
//! | `PATH=/w/./bin` | printed cleaned: `/w/bin/cmd` |
//! | `PATH=//w/bin` | leading `//` **kept** (POSIX reserves exactly two) |
//! | `--show-dot`, `PATH=./bin` | printed `./bin/cmd`; `PATH=bin` still absolute |
//! | `--skip-dot` | skips every **relative** element, not just dotted ones |
//! | `--skip-tilde` | skips `~…` elements *and* elements under `$HOME` |
//! | `--show-tilde` | prints `~/…` for `$HOME`, and is ignored for root |
//! | `--tty-only`, not a tty | freezes only the four show/skip options to its right |
//! | `-a` | prints every match, duplicates included, no de-duplication |
//! | `which cmd --help` | help wins wherever it appears: the scan permutes |
//!
//! # Where this deliberately diverges, and why
//!
//! Four places. Each is a case where GNU which answers a question it was not
//! asked, and copying it would mean shipping a `which` that lies.
//!
//! 1. **`which /some/abs/path` with `PATH` unset.** GNU reports
//!    `no /some/abs/path in ((null))` -- it skips the slash-splitting branch
//!    entirely when `PATH` is absent, so an absolute command that plainly
//!    exists is reported missing. Executing an absolute path does not consult
//!    `PATH`, so neither does this. Here `PATH` is only consulted for a
//!    command with no `/` in it.
//! 2. **`which /init`.** GNU splits it into the directory `""` and the name
//!    `init`, searches nothing, and prints `no init in ()` -- even though
//!    `/init` exists and is executable. The directory part of `/init` is `/`,
//!    and that is what is searched here.
//! 3. **`--skip-tilde` compares whole components.** GNU's `$HOME` test is a
//!    plain `strncmp`, so `HOME=/home/ann` also skips `/home/annex`. Here the
//!    prefix must end at a `/` or at the end of the path.
//! 4. **An unknown option is fatal.** GNU's `getopt` prints `invalid option
//!    -- 'Z'` and the loop carries on, so `which -Z ls` silently answers as
//!    though `-Z` had not been typed. Ignoring an option the user asked for is
//!    the same class of defect as the four above, so this stops with the usual
//!    `Try 'which --help'` referral and upstream's usage status, 255. An
//!    ambiguous long prefix (`--s`) stops for the same reason.
//!
//! # What is not implemented
//!
//! `--read-alias`/`-i` and `--read-functions` read `(alias; declare -f)` from
//! stdin and report shell aliases and functions, optionally resolving the
//! commands used *inside* them. That needs shell integration this build does
//! not have. All four alias/function options are accepted and have no effect,
//! which is exactly the documented behaviour of `--skip-alias
//! --skip-functions`, so a caller that passes them still gets a correct -- if
//! less complete -- answer rather than a usage error. `~user` in a `PATH`
//! element is likewise left unexpanded, since resolving it needs `getpwnam`.
//! Both are recorded in `known-issues.md`.

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes};
use coreutils::stdfd::{self, Stream};
use std::ffi::OsString;
use std::io::Write;
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `which`'s usage status is 255 -- measured: a bare `which` prints the help
/// to stderr and exits 255. Upstream reaches that by returning `-1` from
/// `main`; there is no `EXIT_FAILURE` anywhere in it.
const WHICH: Program = Program::new("which", 255);

/// GNU which's `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "aivV";

/// GNU which's `longopts[]`, in its own order -- which matters, because our
/// parser reports an ambiguous prefix by listing the candidates in table
/// order, and `which --s` lists them as `--skip-dot --skip-tilde --show-dot
/// --show-tilde --skip-alias --skip-functions`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
    ("skip-dot", Takes::Nothing),
    ("skip-tilde", Takes::Nothing),
    ("show-dot", Takes::Nothing),
    ("show-tilde", Takes::Nothing),
    ("tty-only", Takes::Nothing),
    ("all", Takes::Nothing),
    ("read-alias", Takes::Nothing),
    ("skip-alias", Takes::Nothing),
    ("read-functions", Takes::Nothing),
    ("skip-functions", Takes::Nothing),
];

/// What glibc's `printf("%s", NULL)` writes, which is what upstream's message
/// shows when `PATH` is absent. Reproduced as the literal it is, rather than
/// left to a formatting accident.
const NULL_PATH: &[u8] = b"(null)";

// --------------------------------------------------------------- requests ---

/// The four flags that change *what is searched* or *how a hit is printed*.
///
/// They are the only ones `--tty-only` freezes, which is why they are one
/// struct and `-a` is not in it.
#[derive(Clone, Copy, Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Options {
    all: bool,
    skip_dot: bool,
    skip_tilde: bool,
    show_dot: bool,
    show_tilde: bool,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// No command operands. Upstream prints the whole help to *stderr* and
    /// exits 255, which is neither a `--help` nor a getopt error, so it is its
    /// own answer rather than either of those.
    Usage,
    Run(Options, Vec<OsString>),
}

/// GNU which's help, transcribed, less the `Report bugs to …` footer and with
/// the invocation name fixed at `which` rather than `argv[0]` -- upstream
/// prints the path it was invoked by, which would make the text depend on
/// where the binary happens to live.
///
/// The `--skip-dot` line is upstream's wording and is not quite what the
/// option does (it skips *every* relative element, dotted or not). It is
/// transcribed rather than corrected because 25 years of users have read that
/// sentence; the accurate statement is in this file's own docs.
fn help_text() -> String {
    "\
Usage: which [options] [--] COMMAND [...]
Write the full path of COMMAND(s) to standard output.

  --version, -[vV] Print version and exit successfully.
  --help,          Print this help and exit successfully.
  --skip-dot       Skip directories in PATH that start with a dot.
  --skip-tilde     Skip directories in PATH that start with a tilde.
  --show-dot       Don't expand a dot to current directory in output.
  --show-tilde     Output a tilde for HOME directory for non-root.
  --tty-only       Stop processing options on the right if not on tty.
  --all, -a        Print all matches in PATH, not just the first
  --read-alias, -i Read list of aliases from stdin.
  --skip-alias     Ignore option --read-alias; don't read stdin.
  --read-functions Read shell functions from stdin.
  --skip-functions Ignore option --read-functions; don't read stdin.

This build has no shell integration, so the four alias and function options
above are accepted and do nothing: it always behaves as though --skip-alias
and --skip-functions had been given.
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `which`'s argv.
///
/// `stdout_is_tty` is a parameter rather than a call to `isatty` so that the
/// `--tty-only` rule can be tested both ways. Upstream asks about descriptor
/// 1, not 2 or 0.
///
/// # Errors
///
/// An unknown short option, an unrecognised or ambiguous long one, or a value
/// given to a flag that takes none. See this file's docs for why those are
/// fatal here and are not upstream.
fn parse_args(args: &[OsString], stdout_is_tty: bool) -> Result<Request, getopt::Error> {
    let mut options = Options::default();
    let mut commands: Vec<OsString> = Vec::new();
    // `--tty-only` freezes the four show/skip options that follow it, and only
    // those: measured, `--tty-only -a` still sets `-a`, and `--tty-only
    // --help` still prints the help. It is a running state rather than a flag
    // because the freeze applies to the right of where it was typed --
    // `--show-dot --tty-only` keeps the `--show-dot`.
    let mut frozen = false;

    for item in WHICH.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Long("help", _) => return Ok(Request::Help),
            // Upstream's `-v` and `-V` are both the version, and either wins
            // wherever it appears -- measured: `which cmd --version` prints
            // the version and never looks for `cmd`.
            Opt::Long("version", _) | Opt::Short(b'v' | b'V', _) => return Ok(Request::Version),
            Opt::Long("all", _) | Opt::Short(b'a', _) => options.all = true,
            Opt::Long("tty-only", _) => frozen = !stdout_is_tty,
            Opt::Long("skip-dot", _) => options.skip_dot |= !frozen,
            Opt::Long("skip-tilde", _) => options.skip_tilde |= !frozen,
            Opt::Long("show-dot", _) => options.show_dot |= !frozen,
            Opt::Long("show-tilde", _) => options.show_tilde |= !frozen,
            // Accepted and ignored; see this file's docs. Silently, because
            // "ignored" is what `--skip-alias` *means* and a warning on every
            // invocation from a shell wrapper would be noise, not news.
            Opt::Long("read-alias" | "skip-alias" | "read-functions" | "skip-functions", _)
            | Opt::Short(b'i', _) => {}
            // Unreachable: the parser yields only names from the table above,
            // and every one is handled. Refusing rather than ignoring, so that
            // a table entry added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(WHICH.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(WHICH.invalid_option(other)),
            Opt::Operand(command) => commands.push(command.clone()),
        }
    }

    if commands.is_empty() {
        return Ok(Request::Usage);
    }
    Ok(Request::Run(options, commands))
}

// ----------------------------------------------------------------- lookup ---

/// One command's lookup, reduced to the three things the search needs.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Plan {
    /// The directories to try, in order, spelled as the command line spelled
    /// them -- tilde expansion and absolutisation happen later, because
    /// `--skip-tilde` and `--show-dot` both key off the original spelling.
    dirs: Vec<Vec<u8>>,
    /// The file name looked for in each directory.
    name: Vec<u8>,
    /// What `no NAME in (…)` shows. Not derived from `dirs`: for a `PATH`
    /// search it is `$PATH` verbatim, empty elements and all.
    shown: Vec<u8>,
}

/// Work out where to look for `command`.
///
/// A command containing `/` is split at the **last** one and only its own
/// directory is searched. That split is `strrchr`, not gnulib's `dirname`, and
/// the difference is visible: `dirname "a/"` is `.` but this yields `a` with
/// an empty name, which is why `which a/` answers `no  in (./a)`.
fn plan(command: &[u8], path: Option<&[u8]>) -> Plan {
    if let Some(cut) = command.iter().rposition(|&b| b == b'/') {
        let name = command.get(cut.saturating_add(1)..).unwrap_or_default();
        let raw = command.get(..cut).unwrap_or_default();
        // Divergence 2: upstream leaves this empty and so searches nothing.
        let dir: Vec<u8> = if raw.is_empty() {
            b"/".to_vec()
        } else {
            raw.to_vec()
        };
        // Upstream renders a relative search directory with a `./` on the
        // front -- `which bin/x` says `(./bin)` -- but leaves one that already
        // starts with a dot alone: `which ./x` says `(.)`.
        let shown = match dir.first() {
            Some(b'/' | b'.') => dir.clone(),
            _ => {
                let mut shown = b"./".to_vec();
                shown.extend_from_slice(&dir);
                shown
            }
        };
        return Plan {
            dirs: vec![dir],
            name: name.to_vec(),
            shown,
        };
    }

    match path {
        // Divergence 1 lives in the caller: a slashed command never reaches
        // here, so an absolute path is still resolved with `PATH` unset.
        None => Plan {
            dirs: Vec::new(),
            name: command.to_vec(),
            shown: NULL_PATH.to_vec(),
        },
        // An empty `PATH` is *no* directories, not one empty one. Measured:
        // `PATH= which cmd` says `in ()` and does not find a `cmd` sitting in
        // the current directory, while `PATH=: which cmd` does.
        Some([]) => Plan {
            dirs: Vec::new(),
            name: command.to_vec(),
            shown: Vec::new(),
        },
        Some(path) => Plan {
            dirs: path
                .split(|&b| b == b':')
                .map(|element| {
                    if element.is_empty() {
                        b".".to_vec()
                    } else {
                        element.to_vec()
                    }
                })
                .collect(),
            name: command.to_vec(),
            shown: path.to_vec(),
        },
    }
}

/// Everything the search asks of the machine it is running on.
///
/// A trait rather than four free functions so the search can be tested against
/// a filesystem that is a list of names -- the rules being reproduced here are
/// about `PATH` and rendering, and a test that had to `mkdir` to check that
/// `--show-dot` re-relativises would be testing `mkdir`.
trait System {
    /// The current directory, or `.` if it cannot be read -- in which case
    /// nothing can be made absolute and relative answers stand.
    fn cwd(&self) -> Vec<u8>;
    /// `$HOME`, absent if unset *or empty*. An empty `$HOME` is a prefix of
    /// every path, which would make `--skip-tilde` skip the whole `PATH`.
    fn home(&self) -> Option<Vec<u8>>;
    /// Whether the effective user is root, which is the only thing
    /// `--show-tilde` asks.
    fn is_root(&self) -> bool;
    /// Upstream's test for a match: not a directory, and executable by the
    /// effective user.
    fn is_executable(&self, path: &[u8]) -> bool;
}

/// Expand a leading `~` from `$HOME`.
///
/// `~user` is left alone: resolving it needs the password database, and a
/// wrong home directory is worse than an unexpanded one. See this file's docs.
fn expand_tilde(dir: &[u8], home: Option<&[u8]>) -> Vec<u8> {
    if dir.first() != Some(&b'~') {
        return dir.to_vec();
    }
    let Some(home) = home else {
        return dir.to_vec();
    };
    match dir.get(1) {
        None => home.to_vec(),
        Some(b'/') => {
            let mut out = home.to_vec();
            out.extend_from_slice(dir.get(1..).unwrap_or_default());
            out
        }
        // `~user`.
        Some(_) => dir.to_vec(),
    }
}

/// `dir` and `name` with exactly one `/` between them.
fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut out = dir.to_vec();
    if out.last() != Some(&b'/') {
        out.push(b'/');
    }
    out.extend_from_slice(name);
    out
}

/// Whether `path` is `prefix` or lies inside it.
///
/// The boundary test is divergence 3: upstream compares `prefix.len()` bytes
/// and stops, so `/home/annex` counts as inside `/home/ann`.
fn under(path: &[u8], prefix: &[u8]) -> bool {
    if prefix.is_empty() {
        return false;
    }
    path.starts_with(prefix) && matches!(path.get(prefix.len()), None | Some(b'/'))
}

/// Remove `.` and `..` components and runs of `/`, without touching the disk.
///
/// Lexical, so it is wrong where a component is a symlink -- `a/b/..` is not
/// `a` if `b` points elsewhere. Upstream cleans the same way and for the same
/// reason: the answer has to be printable before anything is opened.
///
/// A leading `//` survives exactly as it is. POSIX reserves a path beginning
/// with exactly two slashes for the implementation, and `//host/share` means
/// something on systems that use it; three or more collapse to one.
fn clean(path: &[u8]) -> Vec<u8> {
    let leading = path.iter().take_while(|&&b| b == b'/').count();
    let absolute = leading > 0;

    let mut parts: Vec<&[u8]> = Vec::new();
    for part in path.split(|&b| b == b'/') {
        if part.is_empty() || part == b"." {
            continue;
        }
        if part == b".." {
            match parts.last() {
                Some(last) if *last != b".." => {
                    parts.pop();
                }
                // Above the root is the root. Above a relative start, `..` is
                // the only honest answer and has to stay.
                _ => {
                    if !absolute {
                        parts.push(part);
                    }
                }
            }
            continue;
        }
        parts.push(part);
    }

    let mut out: Vec<u8> = if leading == 2 {
        b"//".to_vec()
    } else if absolute {
        b"/".to_vec()
    } else {
        Vec::new()
    };
    for (at, part) in parts.iter().enumerate() {
        if at > 0 {
            out.push(b'/');
        }
        out.extend_from_slice(part);
    }
    if out.is_empty() {
        out.push(b'.');
    }
    out
}

/// `candidate` as a cleaned absolute path, resolved against `cwd` when it is
/// relative -- the same resolution the kernel performs on every relative path
/// it is handed.
///
/// The probe itself passes the relative path through untouched, because the
/// kernel resolves it; this exists because the *printing* has to say where the
/// hit is without assuming the reader shares our working directory, and
/// because a test double has no working directory to resolve against.
fn absolute(candidate: &[u8], cwd: &[u8]) -> Vec<u8> {
    if candidate.first() == Some(&b'/') {
        clean(candidate)
    } else {
        clean(&join(cwd, candidate))
    }
}

/// `absolute` written as `./…` relative to `cwd`, when it is inside it.
fn relative_to(absolute: &[u8], cwd: &[u8]) -> Option<Vec<u8>> {
    let rest = if cwd == b"/" {
        absolute.strip_prefix(b"/".as_slice())?
    } else {
        absolute.strip_prefix(cwd)?.strip_prefix(b"/".as_slice())?
    };
    let mut out = b"./".to_vec();
    out.extend_from_slice(rest);
    Some(out)
}

/// How a hit is printed.
///
/// The default is absolute and cleaned, which is why `PATH=bin which cmd`
/// answers `/w/bin/cmd` and not `bin/cmd`. `--show-dot` puts it back as `./…`,
/// but *only* when the `PATH` element was written with a leading dot: measured,
/// `PATH=bin --show-dot` still answers absolutely.
fn render<S: System>(
    as_typed: &[u8],
    candidate: &[u8],
    options: Options,
    system: &S,
    home: Option<&[u8]>,
) -> Vec<u8> {
    let full = absolute(candidate, &system.cwd());

    if options.show_dot
        && as_typed.first() == Some(&b'.')
        && let Some(relative) = relative_to(&full, &clean(&system.cwd()))
    {
        return relative;
    }

    // Root is excluded upstream and here: `~` for root's home would render
    // `/root/bin/x` as `~/bin/x` in a context where the reader is most likely
    // to be reading it as somebody else's.
    if options.show_tilde
        && !system.is_root()
        && let Some(home) = home
    {
        let home = clean(home);
        if under(&full, &home) {
            let mut out = b"~".to_vec();
            out.extend_from_slice(full.get(home.len()..).unwrap_or_default());
            return out;
        }
    }

    full
}

/// Every match for `plan`, in `PATH` order -- at most one unless `-a`.
///
/// No de-duplication: measured, `PATH=/b:/b which -a cmd` prints the same path
/// twice, and so does a `PATH` whose two elements are spelled differently but
/// name one directory.
fn look_up<S: System>(plan: &Plan, options: Options, system: &S) -> Vec<Vec<u8>> {
    let home = system.home();
    let mut hits: Vec<Vec<u8>> = Vec::new();

    for dir in &plan.dirs {
        let expanded = expand_tilde(dir, home.as_deref());
        // Upstream's `--skip-dot` test is `*path != '/'`, so it drops every
        // relative element and not only the dotted ones its help describes.
        if options.skip_dot && expanded.first() != Some(&b'/') {
            continue;
        }
        if options.skip_tilde
            && (dir.first() == Some(&b'~')
                || home.as_deref().is_some_and(|home| under(&expanded, home)))
        {
            continue;
        }
        let candidate = join(&expanded, &plan.name);
        if !system.is_executable(&candidate) {
            continue;
        }
        hits.push(render(dir, &candidate, options, system, home.as_deref()));
        if !options.all {
            break;
        }
    }

    hits
}

// ------------------------------------------------------------ this machine ---

/// The real one.
struct Machine;

impl System for Machine {
    fn cwd(&self) -> Vec<u8> {
        std::env::current_dir().map_or_else(
            |_| b".".to_vec(),
            |dir| os_bytes(dir.as_os_str()).into_owned(),
        )
    }

    fn home(&self) -> Option<Vec<u8>> {
        let home = std::env::var_os("HOME")?;
        let home = os_bytes(home.as_os_str()).into_owned();
        if home.is_empty() { None } else { Some(home) }
    }

    fn is_root(&self) -> bool {
        effective_uid_is_root()
    }

    fn is_executable(&self, path: &[u8]) -> bool {
        executable_here(path)
    }
}

#[cfg(unix)]
unsafe extern "C" {
    /// The *effective*-uid form, as `test -x` uses: `which` is answering "could
    /// I run this", and under a setuid binary the two uids differ.
    fn euidaccess(path: *const u8, mode: i32) -> i32;
    fn geteuid() -> u32;
}

#[cfg(unix)]
fn effective_uid_is_root() -> bool {
    // SAFETY: `geteuid` takes no arguments, reads no memory through a pointer
    // and cannot fail. It is always safe to call.
    unsafe { geteuid() == 0 }
}

/// The development host has no POSIX user model, and `--show-tilde` is the
/// only thing that asks. Answering "not root" gives the option its effect,
/// which is the branch worth exercising there.
#[cfg(not(unix))]
fn effective_uid_is_root() -> bool {
    false
}

/// Not a directory, and executable by the effective user.
///
/// `metadata` follows symlinks, which is what upstream's `stat` does: a
/// symlink to an executable is a match and a broken one is not. A named pipe
/// with the execute bit *is* a match -- measured -- so the test is not
/// `S_ISREG`, it is "not a directory".
#[cfg(unix)]
fn executable_here(path: &[u8]) -> bool {
    if !is_non_directory(path) {
        return false;
    }
    let mut c_path = path.to_vec();
    if c_path.contains(&0) {
        return false;
    }
    c_path.push(0);
    // SAFETY: `c_path` is a NUL-terminated byte string with no interior NUL and
    // outlives the call, which reads it and nothing else. `1` is `X_OK`.
    unsafe { euidaccess(c_path.as_ptr(), 1) == 0 }
}

/// On the development host there is no execute bit to consult, so the
/// extension stands in for one -- the same approximation `test -x` makes here,
/// and for the same reason. This branch never runs on SlateOS.
#[cfg(not(unix))]
fn executable_here(path: &[u8]) -> bool {
    if !is_non_directory(path) {
        return false;
    }
    let lower = path.to_ascii_lowercase();
    [b".exe".as_slice(), b".bat", b".cmd", b".com"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn is_non_directory(path: &[u8]) -> bool {
    std::fs::metadata(os_from_bytes(path)).is_ok_and(|meta| !meta.is_dir())
}

// ------------------------------------------------------------------- main ---

fn main() -> ExitCode {
    // Upstream has no `atexit (close_stdout)`, but it also never checks a write
    // -- `which ls >&-` exits 0 there having printed nowhere. One value leaves
    // this function, and it accounts for the writes.
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();

    // Decided before the stream exists, so a usage error prints nothing to a
    // stdout that may be closed.
    let request = match parse_args(&args, stdfd::is_tty(1)) {
        Ok(request) => request,
        Err(e) => {
            WHICH.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let mut out = Stream::stdout();
    let earned = match request {
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Request::Version => {
            let _ = out.write_all(b"which (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        // Measured: the whole help, on *stderr*, status 255.
        Request::Usage => {
            stdfd::diag_bytes(help_text().as_bytes());
            ExitCode::from(255)
        }
        Request::Run(options, commands) => run(&mut out, options, &commands),
    };
    stdfd::close_stdout("which", out, earned)
}

fn run(out: &mut Stream, options: Options, commands: &[OsString]) -> ExitCode {
    let path = std::env::var_os("PATH").map(|path| os_bytes(path.as_os_str()).into_owned());
    let machine = Machine;
    // Upstream's status is the number of commands it could not find, and
    // `exit` keeps only the low byte -- measured, 300 missing exits 44. So 256
    // missing exits 0, which is upstream's behaviour and not an oversight here.
    let mut missing: u8 = 0;

    for command in commands {
        let command = os_bytes(command.as_os_str());
        let plan = plan(&command, path.as_deref());
        let hits = look_up(&plan, options, &machine);
        if hits.is_empty() {
            missing = missing.wrapping_add(1);
            stdfd::diag_bytes(&not_found(&plan));
            continue;
        }
        for hit in hits {
            let _ = out.write_all(&hit);
            let _ = out.write_all(b"\n");
        }
    }

    ExitCode::from(missing)
}

/// `which: no NAME in (DIRS)`, as bytes.
///
/// Assembled rather than formatted because both halves come from argv or the
/// environment and so need not be UTF-8. Upstream escapes neither, and neither
/// does this: `which` prints what it was given.
fn not_found(plan: &Plan) -> Vec<u8> {
    let mut line = b"which: no ".to_vec();
    line.extend_from_slice(&plan.name);
    line.extend_from_slice(b" in (");
    line.extend_from_slice(&plan.shown);
    line.extend_from_slice(b")\n");
    line
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    /// A machine whose filesystem is a list of executable names.
    struct Fake {
        cwd: &'static [u8],
        home: Option<&'static [u8]>,
        root: bool,
        executables: Vec<&'static [u8]>,
    }

    impl Fake {
        fn new(cwd: &'static [u8], executables: &[&'static [u8]]) -> Self {
            Fake {
                cwd,
                home: None,
                root: false,
                executables: executables.to_vec(),
            }
        }
        fn with_home(mut self, home: &'static [u8]) -> Self {
            self.home = Some(home);
            self
        }
        fn with_root_euid(mut self) -> Self {
            self.root = true;
            self
        }
    }

    impl System for Fake {
        fn cwd(&self) -> Vec<u8> {
            self.cwd.to_vec()
        }
        fn home(&self) -> Option<Vec<u8>> {
            self.home.map(<[u8]>::to_vec)
        }
        fn is_root(&self) -> bool {
            self.root
        }
        /// The kernel resolves a relative probe against the working directory,
        /// so a double that compared the raw strings would answer `false` for
        /// every `PATH=bin` case the real one answers `true` for.
        fn is_executable(&self, path: &[u8]) -> bool {
            let wanted = absolute(path, self.cwd);
            self.executables
                .iter()
                .any(|name| absolute(name, self.cwd) == wanted)
        }
    }

    fn found(path: Option<&[u8]>, command: &[u8], options: Options, fake: &Fake) -> Vec<String> {
        look_up(&plan(command, path), options, fake)
            .iter()
            .map(|hit| String::from_utf8_lossy(hit).into_owned())
            .collect()
    }

    // ------------------------------------------------------------ parsing ---

    #[test]
    fn a_bare_invocation_asks_for_the_usage() {
        assert_eq!(parse_args(&argv(&[]), false).unwrap(), Request::Usage);
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(
            parse_args(&argv(&["--help"]), false).unwrap(),
            Request::Help
        );
        for spelling in ["--version", "-v", "-V"] {
            assert_eq!(
                parse_args(&argv(&[spelling]), false).unwrap(),
                Request::Version,
                "{spelling}"
            );
        }
    }

    /// The scan permutes, so an option after an operand still wins -- measured:
    /// `which cmd --version` prints the version and never looks for `cmd`.
    #[test]
    fn version_wins_over_an_operand_that_precedes_it() {
        assert_eq!(
            parse_args(&argv(&["ls", "--version"]), false).unwrap(),
            Request::Version
        );
    }

    #[test]
    fn operands_are_collected_in_order() {
        let Request::Run(options, commands) = parse_args(&argv(&["a", "b"]), false).unwrap() else {
            panic!("expected a run");
        };
        assert_eq!(commands, argv(&["a", "b"]));
        assert_eq!(options, Options::default());
    }

    #[test]
    fn all_is_spelled_two_ways() {
        for spelling in ["-a", "--all", "--al"] {
            let Request::Run(options, _) = parse_args(&argv(&[spelling, "x"]), false).unwrap()
            else {
                panic!("expected a run");
            };
            assert!(options.all, "{spelling}");
        }
    }

    /// The four alias and function options are accepted and change nothing.
    #[test]
    fn the_alias_options_are_accepted_and_ignored() {
        for spelling in [
            "-i",
            "--read-alias",
            "--skip-alias",
            "--read-functions",
            "--skip-functions",
        ] {
            let Request::Run(options, commands) =
                parse_args(&argv(&[spelling, "x"]), false).unwrap()
            else {
                panic!("expected a run");
            };
            assert_eq!(options, Options::default(), "{spelling}");
            assert_eq!(commands, argv(&["x"]), "{spelling}");
        }
    }

    /// Measured: with stdout not a tty, `--tty-only --show-dot` drops the
    /// `--show-dot` and `--show-dot --tty-only` keeps it. On a tty neither is
    /// affected.
    #[test]
    fn tty_only_freezes_what_follows_it_and_only_off_a_tty() {
        let frozen = parse_args(&argv(&["--tty-only", "--show-dot", "x"]), false).unwrap();
        let Request::Run(options, _) = frozen else {
            panic!("expected a run")
        };
        assert!(!options.show_dot);

        let kept = parse_args(&argv(&["--show-dot", "--tty-only", "x"]), false).unwrap();
        let Request::Run(options, _) = kept else {
            panic!("expected a run")
        };
        assert!(options.show_dot);

        let on_a_tty = parse_args(&argv(&["--tty-only", "--show-dot", "x"]), true).unwrap();
        let Request::Run(options, _) = on_a_tty else {
            panic!("expected a run")
        };
        assert!(options.show_dot);
    }

    /// `-a`, `--help` and `--version` are outside the freeze -- measured.
    #[test]
    fn tty_only_does_not_freeze_all_or_help() {
        let Request::Run(options, _) =
            parse_args(&argv(&["--tty-only", "-a", "x"]), false).unwrap()
        else {
            panic!("expected a run");
        };
        assert!(options.all);
        assert_eq!(
            parse_args(&argv(&["--tty-only", "--help"]), false).unwrap(),
            Request::Help
        );
    }

    #[test]
    fn an_unknown_option_is_refused_with_upstreams_status() {
        let e = parse_args(&argv(&["-Z", "ls"]), false).unwrap_err();
        assert_eq!(e.status, 255);
        assert_eq!(
            e.message(),
            "invalid option -- 'Z'\nTry 'which --help' for more information."
        );
        let e = parse_args(&argv(&["--nope", "ls"]), false).unwrap_err();
        assert_eq!(
            e.message(),
            "unrecognized option '--nope'\nTry 'which --help' for more information."
        );
    }

    /// Measured: upstream lists the six candidates in table order.
    #[test]
    fn an_ambiguous_prefix_names_the_candidates() {
        let e = parse_args(&argv(&["--s", "ls"]), false).unwrap_err();
        assert!(
            e.sentence.starts_with(
                "option '--s' is ambiguous; possibilities: '--skip-dot' '--skip-tilde' \
                 '--show-dot' '--show-tilde' '--skip-alias' '--skip-functions'"
            ),
            "got {:?}",
            e.sentence
        );
    }

    #[test]
    fn a_double_dash_turns_an_option_into_an_operand() {
        let Request::Run(_, commands) = parse_args(&argv(&["--", "--help"]), false).unwrap() else {
            panic!("expected a run");
        };
        assert_eq!(commands, argv(&["--help"]));
    }

    /// A command name need not be UTF-8, and must reach the search and the
    /// diagnostic as the bytes it was given.
    #[cfg(unix)]
    #[test]
    fn a_non_utf8_command_survives_parsing_and_the_diagnostic() {
        use std::os::unix::ffi::OsStringExt;
        let args = vec![OsString::from_vec(b"bad\xffname".to_vec())];
        let Request::Run(_, commands) = parse_args(&args, false).unwrap() else {
            panic!("expected a run");
        };
        assert_eq!(commands, args);
        let line = not_found(&plan(b"bad\xffname", Some(b"/b")));
        assert_eq!(line, b"which: no bad\xffname in (/b)\n".to_vec());
    }

    // ------------------------------------------------------------ the plan ---

    #[test]
    fn an_unset_path_searches_nothing_and_shows_null() {
        let plan = plan(b"ls", None);
        assert!(plan.dirs.is_empty());
        assert_eq!(plan.shown, b"(null)".to_vec());
        assert_eq!(plan.name, b"ls".to_vec());
    }

    /// An empty `PATH` is no directories; a `PATH` of one colon is two, both
    /// the current one. Measured, and the pair is the reason the old
    /// `split_path` was wrong.
    #[test]
    fn an_empty_path_is_no_directories_but_a_colon_is_two() {
        let empty = plan(b"ls", Some(b""));
        assert!(empty.dirs.is_empty());
        assert_eq!(empty.shown, Vec::<u8>::new());

        let colon = plan(b"ls", Some(b":"));
        assert_eq!(colon.dirs, vec![b".".to_vec(), b".".to_vec()]);
        assert_eq!(colon.shown, b":".to_vec());
    }

    #[test]
    fn a_path_element_that_is_empty_means_the_current_directory() {
        let plan = plan(b"ls", Some(b"/a::/b:"));
        assert_eq!(
            plan.dirs,
            vec![b"/a".to_vec(), b".".to_vec(), b"/b".to_vec(), b".".to_vec()]
        );
        // The diagnostic shows `$PATH` verbatim, empty elements and all.
        assert_eq!(plan.shown, b"/a::/b:".to_vec());
    }

    /// The split is `strrchr`, not `dirname`: trailing slashes are not ignored
    /// and a run of them is not collapsed.
    #[test]
    fn a_slashed_command_is_split_at_its_last_slash() {
        let split = plan(b"bin/nosuch", Some(b"/ignored"));
        assert_eq!(split.dirs, vec![b"bin".to_vec()]);
        assert_eq!(split.name, b"nosuch".to_vec());
        // Relative, and not already dotted, so upstream shows it as `./bin`.
        assert_eq!(split.shown, b"./bin".to_vec());

        let dotted = plan(b"./nosuch", Some(b"/ignored"));
        assert_eq!(dotted.shown, b".".to_vec());

        let trailing = plan(b"cmd/", Some(b"/ignored"));
        assert_eq!(trailing.name, Vec::<u8>::new());
        assert_eq!(trailing.shown, b"./cmd".to_vec());
    }

    /// Divergence 2: upstream would search nothing here and report `in ()`.
    #[test]
    fn a_command_directly_under_the_root_searches_the_root() {
        let plan = plan(b"/init", None);
        assert_eq!(plan.dirs, vec![b"/".to_vec()]);
        assert_eq!(plan.name, b"init".to_vec());
        assert_eq!(plan.shown, b"/".to_vec());
    }

    /// Divergence 1: a slashed command never consults `PATH`, so an absolute
    /// one resolves with `PATH` unset.
    #[test]
    fn an_absolute_command_resolves_without_a_path() {
        let fake = Fake::new(b"/w", &[b"/s/bin/cmd"]);
        assert_eq!(
            found(None, b"/s/bin/cmd", Options::default(), &fake),
            vec!["/s/bin/cmd".to_string()]
        );
    }

    // -------------------------------------------------------------- search ---

    #[test]
    fn the_first_match_wins_and_all_gives_every_one() {
        let fake = Fake::new(b"/w", &[b"/a/cmd", b"/b/cmd"]);
        assert_eq!(
            found(Some(b"/a:/b"), b"cmd", Options::default(), &fake),
            vec!["/a/cmd".to_string()]
        );
        let all = Options {
            all: true,
            ..Options::default()
        };
        assert_eq!(
            found(Some(b"/a:/b"), b"cmd", all, &fake),
            vec!["/a/cmd".to_string(), "/b/cmd".to_string()]
        );
    }

    /// Measured: no de-duplication, even when two elements name one directory.
    #[test]
    fn all_does_not_deduplicate() {
        let fake = Fake::new(b"/w", &[b"/a/cmd"]);
        let all = Options {
            all: true,
            ..Options::default()
        };
        assert_eq!(
            found(Some(b"/a:/a/.:/a//"), b"cmd", all, &fake),
            vec![
                "/a/cmd".to_string(),
                "/a/cmd".to_string(),
                "/a/cmd".to_string()
            ]
        );
    }

    #[test]
    fn nothing_is_found_when_the_path_is_empty_or_absent() {
        let fake = Fake::new(b"/w", &[b"/w/cmd"]);
        assert!(found(Some(b""), b"cmd", Options::default(), &fake).is_empty());
        assert!(found(None, b"cmd", Options::default(), &fake).is_empty());
        // …but one empty element is the current directory, and does find it.
        assert_eq!(
            found(Some(b":"), b"cmd", Options::default(), &fake),
            vec!["/w/cmd".to_string()]
        );
    }

    /// A relative element is searched relative to the current directory and
    /// printed absolute. Measured: `PATH=bin`, cwd `/w` answers `/w/bin/cmd`.
    #[test]
    fn a_relative_element_is_printed_absolute() {
        let fake = Fake::new(b"/w", &[b"/w/bin/cmd"]);
        for path in [&b"bin"[..], b"./bin", b"./../w/bin"] {
            assert_eq!(
                found(Some(path), b"cmd", Options::default(), &fake),
                vec!["/w/bin/cmd".to_string()],
                "{}",
                String::from_utf8_lossy(path)
            );
        }
    }

    /// `--show-dot` keeps a dotted element relative and leaves an undotted one
    /// absolute -- measured, and the asymmetry is the whole of the option.
    #[test]
    fn show_dot_keeps_a_dotted_element_relative() {
        let fake = Fake::new(b"/w", &[b"/w/bin/cmd", b"/w/cmd"]);
        let show_dot = Options {
            show_dot: true,
            ..Options::default()
        };
        assert_eq!(
            found(Some(b"./bin"), b"cmd", show_dot, &fake),
            vec!["./bin/cmd".to_string()]
        );
        assert_eq!(
            found(Some(b"bin"), b"cmd", show_dot, &fake),
            vec!["/w/bin/cmd".to_string()]
        );
        assert_eq!(
            found(Some(b"."), b"cmd", show_dot, &fake),
            vec!["./cmd".to_string()]
        );
        // An empty element became `.`, so it is dotted too.
        assert_eq!(
            found(Some(b":"), b"cmd", show_dot, &fake),
            vec!["./cmd".to_string()]
        );
    }

    /// Measured: cwd `/w/bin`, `PATH=./../bin --show-dot` answers `./cmd`,
    /// because the cleaned path lands back on the current directory.
    #[test]
    fn show_dot_re_relativises_against_the_cleaned_path() {
        let fake = Fake::new(b"/w/bin", &[b"/w/bin/cmd"]);
        let show_dot = Options {
            show_dot: true,
            ..Options::default()
        };
        assert_eq!(
            found(Some(b"./../bin"), b"cmd", show_dot, &fake),
            vec!["./cmd".to_string()]
        );
    }

    #[test]
    fn a_tilde_element_is_expanded_from_home() {
        let fake = Fake::new(b"/w", &[b"/h/bin/cmd", b"/h/cmd"]).with_home(b"/h");
        assert_eq!(
            found(Some(b"~/bin"), b"cmd", Options::default(), &fake),
            vec!["/h/bin/cmd".to_string()]
        );
        assert_eq!(
            found(Some(b"~"), b"cmd", Options::default(), &fake),
            vec!["/h/cmd".to_string()]
        );
    }

    /// `~user` needs the password database, so it is left literal -- and with
    /// no `$HOME` even `~/` is.
    #[test]
    fn a_user_tilde_and_a_missing_home_are_left_alone() {
        assert_eq!(expand_tilde(b"~bob/bin", Some(b"/h")), b"~bob/bin".to_vec());
        assert_eq!(expand_tilde(b"~/bin", None), b"~/bin".to_vec());
        assert_eq!(expand_tilde(b"/a/~/b", Some(b"/h")), b"/a/~/b".to_vec());
    }

    /// Upstream's `--skip-dot` drops every relative element, not only the
    /// dotted ones. Measured: `PATH=bin --skip-dot` finds nothing.
    #[test]
    fn skip_dot_drops_every_relative_element() {
        let fake = Fake::new(b"/w", &[b"/w/bin/cmd", b"/a/cmd"]);
        let skip_dot = Options {
            skip_dot: true,
            ..Options::default()
        };
        assert!(found(Some(b"bin"), b"cmd", skip_dot, &fake).is_empty());
        assert!(found(Some(b"./bin"), b"cmd", skip_dot, &fake).is_empty());
        assert!(found(Some(b":"), b"cmd", skip_dot, &fake).is_empty());
        assert_eq!(
            found(Some(b"/a"), b"cmd", skip_dot, &fake),
            vec!["/a/cmd".to_string()]
        );
    }

    /// It drops both spellings of "under `$HOME`" -- the `~` one and the
    /// already-absolute one. Measured on both.
    #[test]
    fn skip_tilde_drops_home_however_it_is_spelled() {
        let fake = Fake::new(b"/w", &[b"/h/bin/cmd", b"/a/cmd"]).with_home(b"/h");
        let skip_tilde = Options {
            skip_tilde: true,
            ..Options::default()
        };
        assert!(found(Some(b"~/bin"), b"cmd", skip_tilde, &fake).is_empty());
        assert!(found(Some(b"/h/bin"), b"cmd", skip_tilde, &fake).is_empty());
        assert_eq!(
            found(Some(b"/a"), b"cmd", skip_tilde, &fake),
            vec!["/a/cmd".to_string()]
        );
    }

    /// Divergence 3: upstream's prefix test is a bare `strncmp`, so it would
    /// skip `/hx` for `HOME=/h`.
    #[test]
    fn skip_tilde_compares_whole_components() {
        let fake = Fake::new(b"/w", &[b"/hx/cmd"]).with_home(b"/h");
        let skip_tilde = Options {
            skip_tilde: true,
            ..Options::default()
        };
        assert_eq!(
            found(Some(b"/hx"), b"cmd", skip_tilde, &fake),
            vec!["/hx/cmd".to_string()]
        );
    }

    #[test]
    fn show_tilde_abbreviates_home_for_everyone_but_root() {
        let show_tilde = Options {
            show_tilde: true,
            ..Options::default()
        };
        let user = Fake::new(b"/w", &[b"/h/bin/cmd"]).with_home(b"/h");
        assert_eq!(
            found(Some(b"/h/bin"), b"cmd", show_tilde, &user),
            vec!["~/bin/cmd".to_string()]
        );
        let root = Fake::new(b"/w", &[b"/h/bin/cmd"])
            .with_home(b"/h")
            .with_root_euid();
        assert_eq!(
            found(Some(b"/h/bin"), b"cmd", show_tilde, &root),
            vec!["/h/bin/cmd".to_string()]
        );
        // Outside `$HOME` it changes nothing.
        let outside = Fake::new(b"/w", &[b"/a/cmd"]).with_home(b"/h");
        assert_eq!(
            found(Some(b"/a"), b"cmd", show_tilde, &outside),
            vec!["/a/cmd".to_string()]
        );
    }

    // ------------------------------------------------------------ cleaning ---

    #[test]
    fn cleaning_removes_dots_and_runs_of_slashes() {
        assert_eq!(clean(b"/a/./b"), b"/a/b".to_vec());
        assert_eq!(clean(b"/a//b//"), b"/a/b".to_vec());
        assert_eq!(clean(b"/a/b/.."), b"/a".to_vec());
        assert_eq!(clean(b"/a/b/../b/c"), b"/a/b/c".to_vec());
        assert_eq!(clean(b"/a/."), b"/a".to_vec());
        assert_eq!(clean(b"/"), b"/".to_vec());
        assert_eq!(clean(b"/.."), b"/".to_vec());
        assert_eq!(clean(b"/../.."), b"/".to_vec());
    }

    /// POSIX reserves a leading `//`; three or more slashes do not get the same
    /// treatment. Measured: `PATH=//w/bin` prints `//w/bin/cmd`.
    #[test]
    fn exactly_two_leading_slashes_survive() {
        assert_eq!(clean(b"//w/bin/cmd"), b"//w/bin/cmd".to_vec());
        assert_eq!(clean(b"///w/bin/cmd"), b"/w/bin/cmd".to_vec());
        assert_eq!(clean(b"//"), b"//".to_vec());
    }

    #[test]
    fn a_relative_path_keeps_the_dot_dots_it_cannot_resolve() {
        assert_eq!(clean(b"../a"), b"../a".to_vec());
        assert_eq!(clean(b"a/../../b"), b"../b".to_vec());
        assert_eq!(clean(b"./."), b".".to_vec());
        assert_eq!(clean(b""), b".".to_vec());
    }

    #[test]
    fn joining_never_doubles_the_separator() {
        assert_eq!(join(b"/a", b"cmd"), b"/a/cmd".to_vec());
        assert_eq!(join(b"/a/", b"cmd"), b"/a/cmd".to_vec());
        assert_eq!(join(b"/", b"cmd"), b"/cmd".to_vec());
    }

    #[test]
    fn under_needs_a_component_boundary() {
        assert!(under(b"/h", b"/h"));
        assert!(under(b"/h/a", b"/h"));
        assert!(!under(b"/hx", b"/h"));
        assert!(!under(b"/", b""));
    }

    // ------------------------------------------------------------- the text ---

    #[test]
    fn the_not_found_line_names_the_basename_and_the_path() {
        assert_eq!(
            not_found(&plan(b"nosuch", Some(b"/a:/b"))),
            b"which: no nosuch in (/a:/b)\n".to_vec()
        );
        assert_eq!(
            not_found(&plan(b"nosuch", None)),
            b"which: no nosuch in ((null))\n".to_vec()
        );
        assert_eq!(
            not_found(&plan(b"bin/nosuch", Some(b"/a"))),
            b"which: no nosuch in (./bin)\n".to_vec()
        );
    }

    #[test]
    fn the_help_text_is_upstreams_wording() {
        let text = help_text();
        assert!(text.starts_with("Usage: which [options] [--] COMMAND [...]\n"));
        assert!(text.contains("Write the full path of COMMAND(s) to standard output.\n"));
        assert!(
            text.contains("  --all, -a        Print all matches in PATH, not just the first\n")
        );
    }
}
