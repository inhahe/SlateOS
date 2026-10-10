//! ln — create links between files.
//!
//! GNU coreutils 9.4's `ln`, every option:
//!
//! ```text
//! ln [OPTION]... [-T] TARGET LINK_NAME
//! ln [OPTION]... TARGET
//! ln [OPTION]... TARGET... DIRECTORY
//! ln [OPTION]... -t DIRECTORY TARGET...
//! ```
//!
//! Until 2026-10-03 this `ln` had `-s` and nothing else: `-f`, `-i`, `-n`,
//! `-r`, `-t`, `-T`, `-v`, `-b`, `-S`, `-L`, `-P` and `-d` were refused by
//! name. So `ln -sf new link`, the way a link is pointed somewhere else, did
//! not work at all. Each is now GNU's, following `ln.c` and gnulib's
//! `force-link.c` function by function:
//!
//! * **The operand forms** are GNU `main`'s: with two operands the link is
//!   tried first, and only if that fails with `EEXIST`, `ENOTDIR` or
//!   `EINVAL` is the second operand opened as a directory to put the link in
//!   (not when it is a symbolic link and `-n` was given). `-T` never does
//!   that, `-t` always does, and one operand links into `.` -- spelled `./a`,
//!   as GNU's messages spell it.
//! * **Replacing a destination** (`-f`, `-i`, `-b`) never removes it first:
//!   the link is made under a fresh `CuXXXXXX` name in the destination's
//!   directory and renamed over it, so the name always holds one file or the
//!   other, and `ln -f nosuch b` keeps `b`. Before that, the checks GNU makes:
//!   a directory is never overwritten, a file is never replaced by a link to
//!   itself, and under `-f` a hard link made earlier in the same run is not
//!   replaced by a later one with the same name (`dest_set`).
//! * **`-r`** writes the path from the link's directory to the target, both
//!   made canonical first with missing components allowed -- `canon::relpath`,
//!   shared with `realpath --relative-to`.
//! * **`-v`** prints `'link' -> 'target'` for a symbolic link and `=>` for a
//!   hard one, after `'backup' ~ ` when a backup was made.
//!
//! Every name in a diagnostic goes through [`coreutils::quote`], and argv is
//! bytes to the syscall: on this OS a file name may hold any byte but `/` and
//! NUL. (The first rewrite of this file was for that: it read argv as
//! `String` and panicked on such a name. See `known-issues.md` →
//! `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`.)

use coreutils::backup::{self, Backup, BackupType};
use coreutils::canon::{self, Mode, RealFs};
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, os_from_bytes, quoteaf, quotef};
use coreutils::stdfd::{self, Stream};
use coreutils::yesno::{self, Answers, StdinAnswers};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

coreutils::guard_std_fds!();

/// `ln`'s usage status is 1 — measured: `ln --zzz; echo $?` prints 1. See
/// [`coreutils::getopt::Error`] for the handful of utilities that differ.
const LN: Program = Program::new("ln", 1);

/// GNU `ln`'s `getopt_long` string.
const SHORT_OPTIONS: &str = "bdfinrst:vFLPS:T";

/// GNU `ln`'s `long_options[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Measured with the instrument described in
/// [`Program::resolve_long`](getoptlong::Program::resolve_long) — an empty prefix matches everything, so
/// `ln --=x` prints the whole table:
///
/// ```text
/// ln: option '--=x' is ambiguous; possibilities: '--backup' '--directory'
/// '--no-dereference' '--no-target-directory' '--force' '--interactive'
/// '--suffix' '--target-directory' '--logical' '--physical' '--relative'
/// '--symbolic' '--verbose' '--help' '--version'
/// ```
///
/// Note that it is neither alphabetical nor grouped by short-option letter.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("backup", Takes::Optional),
    ("directory", Takes::Nothing),
    ("no-dereference", Takes::Nothing),
    ("no-target-directory", Takes::Nothing),
    ("force", Takes::Nothing),
    ("interactive", Takes::Nothing),
    ("suffix", Takes::Required),
    ("target-directory", Takes::Required),
    ("logical", Takes::Nothing),
    ("physical", Takes::Nothing),
    ("relative", Takes::Nothing),
    ("symbolic", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the options asked for. GNU keeps these as globals of `ln.c`.
#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct LnFlags {
    /// `-s`.
    symbolic: bool,
    /// `-f`: replace an existing destination. Cleared by a later `-i`.
    remove_existing: bool,
    /// `-i`: ask before replacing one. Cleared by a later `-f`.
    interactive: bool,
    /// `-d`/`-F`: let a hard link name a directory -- which Linux refuses
    /// anyway, as the help says.
    hard_dir_link: bool,
    /// `-L`: a hard link to a symbolic link links what it points at. `-P`,
    /// the default, links the symbolic link itself.
    logical: bool,
    /// `-n`: a `LINK_NAME` that is a symbolic link to a directory is not that
    /// directory. GNU's `dereference_dest_dir_symlinks`, inverted.
    no_dereference: bool,
    /// `-r`: a symbolic link's text is relative to where the link is.
    relative: bool,
    /// `-v`.
    verbose: bool,
    /// `-T`.
    no_target_directory: bool,
    /// `-t DIRECTORY`, already checked to be a directory.
    target_directory: Option<Vec<u8>>,
    /// `-b`, `--backup`, `-S`: whether, and the word `--backup` was given.
    make_backups: bool,
    version_control: Option<OsString>,
    backup_suffix: Option<OsString>,
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order.
    Run(LnFlags, Vec<Vec<u8>>),
}

/// The funnel: upstream's `atexit (close_stdin)`, which closes standard input
/// -- read for `-i`'s answers -- and then does `close_stdout`'s work, standard
/// output and then standard error, on every exit path at once. An answer that
/// could not be read, an output or a diagnostic that did not arrive is status
/// 1. See [`stdfd::close_stdin_and_stdout`].
fn main() -> ExitCode {
    stdfd::restore();
    let mut out = Stream::stdout();
    let mut answers = StdinAnswers::default();
    let earned = run_main(&mut out, &mut answers);
    stdfd::close_stdin_and_stdout("ln", answers.into_stream(), out, earned)
}

fn run_main(out: &mut Stream, answers: &mut StdinAnswers) -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse_args(&args, getopt::posixly_correct()) {
        // The stream records a failure for the funnel; it never returns one.
        Ok(Request::Help) => {
            let _ = out.write_all(help_text().as_bytes());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            let _ = out.write_all(b"ln (SlateOS coreutils) 0.1.0\n");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, files)) => {
            let mut err = Stream::stderr();
            let ok = Ln {
                flags: &flags,
                backup: Backup::disabled(),
                dest_set: None,
                out,
                err: &mut err,
                answers,
            }
            .run(&files);
            match ok {
                Ok(true) => ExitCode::SUCCESS,
                Ok(false) | Err(Fatal) => ExitCode::from(1),
            }
        }
        Err(e) => {
            diag!("ln: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: ln [OPTION]... [-T] TARGET LINK_NAME
  or:  ln [OPTION]... TARGET
  or:  ln [OPTION]... TARGET... DIRECTORY
  or:  ln [OPTION]... -t DIRECTORY TARGET...
In the 1st form, create a link to TARGET with the name LINK_NAME.
In the 2nd form, create a link to TARGET in the current directory.
In the 3rd and 4th forms, create links to each TARGET in DIRECTORY.
Create hard links by default, symbolic links with --symbolic.
By default, each destination (name of new link) should not already exist.
When creating hard links, each TARGET must exist.  Symbolic links
can hold arbitrary text; if later resolved, a relative link is
interpreted in relation to its parent directory.

Mandatory arguments to long options are mandatory for short options too.
      --backup[=CONTROL]      make a backup of each existing destination file
  -b                          like --backup but does not accept an argument
  -d, -F, --directory         allow the superuser to attempt to hard link
                                directories (note: will probably fail due to
                                system restrictions, even for the superuser)
  -f, --force                 remove existing destination files
  -i, --interactive           prompt whether to remove destinations
  -L, --logical               dereference TARGETs that are symbolic links
  -n, --no-dereference        treat LINK_NAME as a normal file if
                                it is a symbolic link to a directory
  -P, --physical              make hard links directly to symbolic links
  -r, --relative              with -s, create links relative to link location
  -s, --symbolic              make symbolic links instead of hard links
  -S, --suffix=SUFFIX         override the usual backup suffix
  -t, --target-directory=DIRECTORY  specify the DIRECTORY in which to create
                                the links
  -T, --no-target-directory   treat LINK_NAME as a normal file always
  -v, --verbose               print name of each linked file
      --help        display this help and exit
      --version     output version information and exit

The backup suffix is '~', unless set with --suffix or SIMPLE_BACKUP_SUFFIX.
The version control method may be selected via the --backup option or through
the VERSION_CONTROL environment variable.  Here are the values:

  none, off       never make backups (even if --backup is given)
  numbered, t     make numbered backups
  existing, nil   numbered if numbered backups exist, simple otherwise
  simple, never   always make simple backups

Using -s ignores -L and -P.  Otherwise, the last option specified controls
behavior when a TARGET is a symbolic link, defaulting to -P.
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `ln`'s argv: GNU's option loop, which checks `-t`'s directory as it
/// goes -- so `ln -t nosuch` fails there, before anything about operands.
///
/// `posixly_correct` is [`getopt::posixly_correct`], passed in so that a test
/// can choose it: when it is set, the first operand ends option parsing, as it
/// does in glibc's getopt.
///
/// # Errors
///
/// An unknown option, a long option given a value it does not take, or one of
/// `-t`'s three refusals.
fn parse_args(args: &[OsString], posixly_correct: bool) -> Result<Request, getopt::Error> {
    let mut flags = LnFlags::default();
    let mut files: Vec<Vec<u8>> = Vec::new();
    for item in LN
        .parse(args, SHORT_OPTIONS, LONG_OPTIONS)
        .posixly_correct(posixly_correct)
    {
        match item? {
            // A lone `-` arrives here too: `ln` has no standard-input operand.
            Opt::Operand(name) => files.push(os_bytes(name).into_owned()),
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'b', value) | Opt::Long("backup", value) => {
                flags.make_backups = true;
                if let Some(word) = value {
                    flags.version_control = Some(word);
                }
            }
            Opt::Short(b'd' | b'F', _) | Opt::Long("directory", _) => flags.hard_dir_link = true,
            // Each of these two cancels the other: the last one given wins.
            Opt::Short(b'f', _) | Opt::Long("force", _) => {
                flags.remove_existing = true;
                flags.interactive = false;
            }
            Opt::Short(b'i', _) | Opt::Long("interactive", _) => {
                flags.remove_existing = false;
                flags.interactive = true;
            }
            Opt::Short(b'L', _) | Opt::Long("logical", _) => flags.logical = true,
            Opt::Short(b'n', _) | Opt::Long("no-dereference", _) => flags.no_dereference = true,
            Opt::Short(b'P', _) | Opt::Long("physical", _) => flags.logical = false,
            Opt::Short(b'r', _) | Opt::Long("relative", _) => flags.relative = true,
            Opt::Short(b's', _) | Opt::Long("symbolic", _) => flags.symbolic = true,
            Opt::Short(b't', value) | Opt::Long("target-directory", value) => {
                // Plain diagnostics, with no referral: upstream raises all
                // three with `error (EXIT_FAILURE, …)`, not through `usage`.
                if flags.target_directory.is_some() {
                    return Err(LN.usage("multiple target directories specified".into()));
                }
                let Some(dir) = value else {
                    return Err(LN.short_missing_argument(b't'));
                };
                let dir = os_bytes(&dir).into_owned();
                match fs::metadata(os_from_bytes(&dir)) {
                    Err(e) => {
                        return Err(LN.usage(format!(
                            "failed to access {}: {}",
                            quoteaf(&dir),
                            strerror(&e)
                        )));
                    }
                    Ok(m) if !m.is_dir() => {
                        return Err(
                            LN.usage(format!("target {} is not a directory", quoteaf(&dir)))
                        );
                    }
                    Ok(_) => {}
                }
                flags.target_directory = Some(dir);
            }
            Opt::Short(b'T', _) | Opt::Long("no-target-directory", _) => {
                flags.no_target_directory = true;
            }
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            Opt::Short(b'S', value) | Opt::Long("suffix", value) => {
                let Some(given) = value else {
                    return Err(LN.short_missing_argument(b'S'));
                };
                flags.make_backups = true;
                flags.backup_suffix = Some(given);
            }
            Opt::Short(other, _) => return Err(LN.invalid_option(other)),
            Opt::Long(other, _) => return Err(LN.unrecognized_option(other.as_bytes())),
        }
    }
    Ok(Request::Run(flags, files))
}

// ------------------------------------------------------------ the names ---

/// gnulib's `last_component`: the last name in `file`, with any slashes after
/// it. A name of slashes only has none, and answers the empty tail.
fn last_component(file: &[u8]) -> &[u8] {
    let mut start = file.iter().position(|&c| c != b'/').unwrap_or(file.len());
    let mut last_was_slash = false;
    for (i, &c) in file.iter().enumerate().skip(start) {
        if c == b'/' {
            last_was_slash = true;
        } else if last_was_slash {
            start = i;
            last_was_slash = false;
        }
    }
    file.get(start..).unwrap_or_default()
}

/// gnulib's `base_len`: `name` without its trailing slashes, but never empty
/// for a name that is all slashes.
fn base_len(name: &[u8]) -> usize {
    let mut len = name.len();
    while len > 1 && name.get(len.saturating_sub(1)) == Some(&b'/') {
        len = len.saturating_sub(1);
    }
    len
}

/// gnulib's `file_name_concat`: `dir` and `base` joined with one slash -- or
/// none where `dir` already ends in one -- and where in the result `base`
/// begins. `.` is kept, so `ln a` makes `./a`, which is how GNU's messages
/// name it.
fn file_name_concat(dir: &[u8], base: &[u8]) -> (Vec<u8>, usize) {
    let dirbase = last_component(dir);
    let dirbase_at = dir.len().saturating_sub(dirbase.len());
    let dirbaselen = base_len(dirbase);
    let dirlen = dirbase_at.saturating_add(dirbaselen).min(dir.len());
    let mut out = dir.get(..dirlen).unwrap_or_default().to_vec();
    if dirbaselen > 0 {
        if out.last() != Some(&b'/') && base.first() != Some(&b'/') {
            out.push(b'/');
        }
    } else if base.first() == Some(&b'/') {
        out.push(b'.');
    }
    let at = out.len();
    out.extend_from_slice(base);
    (out, at)
}

/// gnulib's `dir_name`: everything before the last component, without the
/// slashes that separate them -- or `.` when there is nothing, and `/` for a
/// name at the root.
fn dir_name(file: &[u8]) -> Vec<u8> {
    let base = last_component(file);
    let mut len = file.len().saturating_sub(base.len());
    while len > 1 && file.get(len.saturating_sub(1)) == Some(&b'/') {
        len = len.saturating_sub(1);
    }
    match file.get(..len) {
        Some(d) if !d.is_empty() => d.to_vec(),
        _ => b".".to_vec(),
    }
}

// ------------------------------------------------------------ the links ---

/// A refusal that ends the whole run, already reported.
struct Fatal;

/// `ENOENT`, `EEXIST`, `ENOTDIR`, `EINVAL`, `EMLINK`, `ENAMETOOLONG`,
/// `ENOSPC`, `EROFS`, `EDQUOT`: the errnos `ln.c` branches on.
const ENOENT: i32 = 2;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EINVAL: i32 = 22;
const EMLINK: i32 = 31;
const ENAMETOOLONG: i32 = 36;
const ENOSPC: i32 = 28;
const EROFS: i32 = 30;
const EDQUOT: i32 = 122;

/// What a link attempt came to: not tried yet, made (possibly over an
/// existing file), or failed with an errno. GNU folds these into one `int`
/// (`-1`, `0` or negative, positive).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tried {
    Not,
    Made,
    Failed(i32),
}

/// The errno of an `io::Error`, or `EINVAL` where the platform gave none.
fn errno_of(e: &io::Error) -> i32 {
    e.raw_os_error().unwrap_or(EINVAL)
}

struct Ln<'a> {
    flags: &'a LnFlags,
    backup: Backup,
    /// GNU's `dest_set`: the destinations just made as hard links, by name
    /// and by the identity of the file they now are -- so that `ln -f x/a y/a
    /// dir` does not replace the `dir/a` it has just made with the second.
    dest_set: Option<HashSet<(Vec<u8>, u64, u64)>>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
    answers: &'a mut dyn Answers,
}

impl Ln<'_> {
    /// One diagnostic, `ln: ` and all.
    fn say(&mut self, message: &str) {
        // A diagnostic that cannot be written is the funnel's to notice.
        let _ = writeln!(self.err, "ln: {message}");
    }

    /// GNU's `emit_try_help`: the referral, without the program's prefix.
    fn try_help(&mut self) {
        let _ = writeln!(self.err, "Try 'ln --help' for more information.");
    }

    /// GNU's `main` after the option loop.
    fn run(&mut self, files: &[Vec<u8>]) -> Result<bool, Fatal> {
        let flags = self.flags;
        let Some(first) = files.first() else {
            self.say("missing file operand");
            self.try_help();
            return Err(Fatal);
        };
        if flags.relative && !flags.symbolic {
            self.say("cannot do --relative without --symbolic");
            return Err(Fatal);
        }
        let mut n_files = files.len();
        let mut target_directory = flags.target_directory.clone();
        let mut link_errno = Tried::Not;
        if flags.no_target_directory {
            if target_directory.is_some() {
                self.say("cannot combine --target-directory and --no-target-directory");
                return Err(Fatal);
            }
            if n_files != 2 {
                if n_files < 2 {
                    self.say(&format!(
                        "missing destination file operand after {}",
                        quoteaf(first)
                    ));
                } else if let Some(extra) = files.get(2) {
                    self.say(&format!("extra operand {}", quoteaf(extra)));
                }
                self.try_help();
                return Err(Fatal);
            }
        } else if n_files < 2 && target_directory.is_none() {
            target_directory = Some(b".".to_vec());
        } else {
            if n_files == 2
                && target_directory.is_none()
                && let (Some(source), Some(dest)) = (files.first(), files.get(1))
            {
                link_errno = self.atomic_link(source, dest);
            }
            if matches!(
                link_errno,
                Tried::Not | Tried::Failed(EEXIST | ENOTDIR | EINVAL)
            ) {
                let d = match &target_directory {
                    Some(d) => d.clone(),
                    None => files.last().cloned().unwrap_or_default(),
                };
                match self.open_directory(&d) {
                    Ok(()) => {
                        if target_directory.is_none() {
                            n_files = n_files.saturating_sub(1);
                        }
                        target_directory = Some(d);
                    }
                    Err(e) if !(n_files == 2 && target_directory.is_none()) => {
                        self.say(&format!("target {}: {}", quoteaf(&d), strerror(&e)));
                        return Err(Fatal);
                    }
                    Err(_) => {}
                }
            }
        }

        // The backup type is settled only now, after every check above --
        // so `ln --backup=bogus` with no operand is `missing file operand`.
        self.backup = if flags.make_backups {
            match backup::control(LN, flags.version_control.as_deref()) {
                Ok(kind) => Backup::new(kind, backup::suffix(flags.backup_suffix.as_deref())),
                Err(e) => {
                    self.say(&e.to_string());
                    return Err(Fatal);
                }
            }
        } else {
            Backup::disabled()
        };

        let Some(dir) = target_directory else {
            let (Some(source), Some(dest)) = (files.first(), files.get(1)) else {
                return Ok(false);
            };
            return Ok(self.do_link(source, dest, link_errno));
        };
        if n_files >= 2
            && flags.remove_existing
            && !flags.symbolic
            && self.backup.kind() != BackupType::Numbered
        {
            self.dest_set = Some(HashSet::new());
        }
        let mut ok = true;
        for file in files.iter().take(n_files) {
            let (mut dest, at) = file_name_concat(&dir, last_component(file));
            // `strip_trailing_slashes (dest_base)`: `ln a/ d` makes `d/a`.
            let keep = at.saturating_add(base_len(dest.get(at..).unwrap_or_default()));
            if dest.get(at..).is_some_and(|b| !b.is_empty()) {
                dest.truncate(keep);
            }
            ok &= self.do_link(file, &dest, Tried::Not);
        }
        Ok(ok)
    }

    /// GNU's `openat_safer (AT_FDCWD, d, O_PATHSEARCH | O_DIRECTORY | …)`:
    /// whether `d` is a directory to put links in -- not when it is a
    /// symbolic link and `-n` was given.
    fn open_directory(&self, d: &[u8]) -> io::Result<()> {
        open_directory(&os_from_bytes(d), self.flags.no_dereference)
    }

    /// GNU's `atomic_link`: the link made in one call, or not tried -- `-r`
    /// needs the relative name worked out first.
    fn atomic_link(&self, source: &[u8], dest: &[u8]) -> Tried {
        let made = if self.flags.symbolic {
            if self.flags.relative {
                return Tried::Not;
            }
            sys::symlink(source, dest)
        } else {
            sys::link(source, dest, self.flags.logical)
        };
        match made {
            Ok(()) => Tried::Made,
            Err(e) => Tried::Failed(errno_of(&e)),
        }
    }

    /// GNU's `do_link`, path by path. Returns whether the link is there.
    #[allow(clippy::too_many_lines)] // One upstream function, kept whole so it reads as one.
    fn do_link(&mut self, source: &[u8], dest: &[u8], tried: Tried) -> bool {
        let flags = self.flags;
        let mut link_errno = match tried {
            Tried::Not => self.atomic_link(source, dest),
            t => t,
        };
        let mut source = source.to_vec();
        let mut source_stats: Option<fs::Metadata> = None;
        // `SOURCE_STATS`, when later code will want it -- if only for sharper
        // diagnostics. `-L` follows a symbolic link here; the default does not.
        if (link_errno != Tried::Made || self.dest_set.is_some()) && !flags.symbolic {
            let stat = if flags.logical {
                fs::metadata(os_from_bytes(&source))
            } else {
                fs::symlink_metadata(os_from_bytes(&source))
            };
            match stat {
                Ok(m) => source_stats = Some(m),
                Err(e) => {
                    self.say(&format!(
                        "failed to access {}: {}",
                        quoteaf(&source),
                        strerror(&e)
                    ));
                    return false;
                }
            }
        }

        let mut backup_made: Option<Vec<u8>> = None;
        if link_errno != Tried::Made {
            if !flags.symbolic
                && !flags.hard_dir_link
                && source_stats.as_ref().is_some_and(fs::Metadata::is_dir)
            {
                self.say(&format!(
                    "{}: hard link not allowed for directory",
                    quotef(&source)
                ));
                return false;
            }
            if flags.relative {
                source = convert_abs_rel(&source, dest);
            }

            let mut force = flags.remove_existing || flags.interactive || self.backup.enabled();
            if force {
                match fs::symlink_metadata(os_from_bytes(dest)) {
                    Err(e)
                        if e.raw_os_error() == Some(ENOENT)
                            || e.kind() == io::ErrorKind::NotFound =>
                    {
                        force = false;
                    }
                    Err(e) => {
                        self.say(&format!(
                            "failed to access {}: {}",
                            quoteaf(dest),
                            strerror(&e)
                        ));
                        return false;
                    }
                    Ok(dest_stats) if dest_stats.is_dir() => {
                        self.say(&format!("{}: cannot overwrite directory", quotef(dest)));
                        return false;
                    }
                    Ok(dest_stats) if self.seen_file(dest, &dest_stats) => {
                        self.say(&format!(
                            "will not overwrite just-created {} with {}",
                            quoteaf(dest),
                            quoteaf(&source)
                        ));
                        return false;
                    }
                    Ok(dest_stats) => {
                        // Removing DEST must not remove SOURCE: with backups,
                        // the worry is a hard link (a backup covers a symbolic
                        // one); without, it is `-f`'s.
                        let worry = if self.backup.enabled() {
                            !flags.symbolic
                        } else {
                            flags.remove_existing
                        };
                        if worry {
                            let source_now = match &source_stats {
                                Some(m) => Some(m.clone()),
                                None => fs::metadata(os_from_bytes(&source)).ok(),
                            };
                            if let Some(sm) = source_now
                                && same_inode(&sm, &dest_stats)
                                && (nlink(&sm) == 1 || same_name(&source, dest))
                            {
                                self.say(&format!(
                                    "{} and {} are the same file",
                                    quoteaf(&source),
                                    quoteaf(dest)
                                ));
                                return false;
                            }
                        }
                        if matches!(link_errno, Tried::Not | Tried::Failed(EEXIST)) {
                            if flags.interactive {
                                let _ = write!(self.err, "ln: replace {}? ", quoteaf(dest));
                                let _ = self.err.flush();
                                if !yesno::yesno(&mut *self.answers) {
                                    return false;
                                }
                            }
                            if self.backup.enabled() {
                                match self.backup.rename(Path::new(&os_from_bytes(dest))) {
                                    Ok(made) => {
                                        backup_made = Some(os_bytes(made.as_os_str()).into_owned());
                                    }
                                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                                        force = false;
                                    }
                                    Err(e) => {
                                        self.say(&format!(
                                            "cannot backup {}: {}",
                                            quoteaf(dest),
                                            strerror(&e)
                                        ));
                                        return false;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Try the link -- again, if it was tried before -- and only when
            // that fails with `EEXIST` and the destination may be replaced,
            // replace it: POSIX 2008 lets `ln -f a b` fail early rather than
            // remove `b` first, which keeps `b` if `a` does not exist.
            link_errno = if flags.symbolic {
                force_symlink(&source, dest, force, link_errno)
            } else {
                force_link(&source, dest, flags.logical, force, link_errno)
            };
        }

        if let Tried::Failed(errno) = link_errno {
            let why = strerror(&io::Error::from_raw_os_error(errno));
            let message = if flags.symbolic {
                if errno != ENAMETOOLONG && !source.is_empty() {
                    format!("failed to create symbolic link {}: {why}", quoteaf(dest))
                } else {
                    format!(
                        "failed to create symbolic link {} -> {}: {why}",
                        quoteaf(dest),
                        quoteaf(&source)
                    )
                }
            } else if errno == EMLINK {
                format!("failed to create hard link to {}: {why}", quoteaf(&source))
            } else if matches!(errno, EDQUOT | EEXIST | ENOSPC | EROFS) {
                format!("failed to create hard link {}: {why}", quoteaf(dest))
            } else {
                format!(
                    "failed to create hard link {} => {}: {why}",
                    quoteaf(dest),
                    quoteaf(&source)
                )
            };
            self.say(&message);
            if let Some(backup) = &backup_made
                && let Err(e) = fs::rename(os_from_bytes(backup), os_from_bytes(dest))
            {
                self.say(&format!(
                    "cannot un-backup {}: {}",
                    quoteaf(dest),
                    strerror(&e)
                ));
            }
            return false;
        }

        // Made. A hard link is recorded under the name it now has, with the
        // identity of the file it now is.
        if !flags.symbolic
            && let Some(sm) = &source_stats
        {
            self.record_file(dest, sm);
        }
        if flags.verbose {
            let mut line = Vec::new();
            if let Some(backup) = &backup_made {
                line.extend_from_slice(quoteaf(backup).as_bytes());
                line.extend_from_slice(b" ~ ");
            }
            line.extend_from_slice(quoteaf(dest).as_bytes());
            line.extend_from_slice(if flags.symbolic { b" -> " } else { b" => " });
            line.extend_from_slice(quoteaf(&source).as_bytes());
            line.push(b'\n');
            // Recorded by the stream for the funnel.
            let _ = self.out.write_all(&line);
        }
        true
    }

    /// GNU's `seen_file`: whether `dest` is a hard link this run has made.
    fn seen_file(&self, dest: &[u8], stats: &fs::Metadata) -> bool {
        self.dest_set
            .as_ref()
            .is_some_and(|set| set.contains(&(dest.to_vec(), dev(stats), ino(stats))))
    }

    /// GNU's `record_file`.
    fn record_file(&mut self, dest: &[u8], stats: &fs::Metadata) {
        if let Some(set) = self.dest_set.as_mut() {
            set.insert((dest.to_vec(), dev(stats), ino(stats)));
        }
    }
}

/// GNU's `convert_abs_rel`: `from` written relative to the directory `target`
/// will be in, both made canonical first -- missing components allowed -- or
/// `from` unchanged when they share nothing to be relative to.
fn convert_abs_rel(from: &[u8], target: &[u8]) -> Vec<u8> {
    let targetdir = dir_name(target);
    let realdest = canon::canonicalize(&RealFs, &targetdir, Mode::Missing).ok();
    let realfrom = canon::canonicalize(&RealFs, from, Mode::Missing).ok();
    match (realdest, realfrom) {
        (Some(d), Some(f)) => canon::relpath(&f, &d).unwrap_or_else(|| from.to_vec()),
        _ => from.to_vec(),
    }
}

/// gnulib's `same_nameat`: whether `source` and `dest` are the same entry in
/// the same directory -- the same last name, and parents that are one
/// directory.
fn same_name(source: &[u8], dest: &[u8]) -> bool {
    let sb = last_component(source);
    let db = last_component(dest);
    let sb = sb.get(..base_len(sb)).unwrap_or_default();
    let db = db.get(..base_len(db)).unwrap_or_default();
    if sb != db {
        return false;
    }
    match (
        fs::symlink_metadata(os_from_bytes(&dir_name(source))),
        fs::symlink_metadata(os_from_bytes(&dir_name(dest))),
    ) {
        (Ok(a), Ok(b)) => same_inode(&a, &b),
        _ => false,
    }
}

/// gnulib's `force_linkat`: the hard link, and -- if `force` and the name was
/// taken -- the link made under a temporary name beside it and renamed over
/// the old one, so the name always holds one file or the other.
fn force_link(source: &[u8], dest: &[u8], follow: bool, force: bool, tried: Tried) -> Tried {
    let tried = match tried {
        Tried::Not => match sys::link(source, dest, follow) {
            Ok(()) => Tried::Made,
            Err(e) => Tried::Failed(errno_of(&e)),
        },
        t => t,
    };
    if !force || tried != Tried::Failed(EEXIST) {
        return tried;
    }
    replace_via_temp(dest, |temp| sys::link(source, temp, follow), true)
}

/// gnulib's `force_symlinkat`, likewise.
fn force_symlink(source: &[u8], dest: &[u8], force: bool, tried: Tried) -> Tried {
    let tried = match tried {
        Tried::Not => match sys::symlink(source, dest) {
            Ok(()) => Tried::Made,
            Err(e) => Tried::Failed(errno_of(&e)),
        },
        t => t,
    };
    if !force || tried != Tried::Failed(EEXIST) {
        return tried;
    }
    replace_via_temp(dest, |temp| sys::symlink(source, temp), false)
}

/// Make the link at a fresh `CuXXXXXX` name in `dest`'s directory, rename it
/// over `dest`, and tidy up. `unlink_after` is the hard-link case, where the
/// temporary is removed even after a successful rename: if it and `dest`
/// were already one file, the rename did nothing and left both names.
fn replace_via_temp(
    dest: &[u8],
    make: impl Fn(&[u8]) -> io::Result<()>,
    unlink_after: bool,
) -> Tried {
    let dir = dir_name(dest);
    let mut made: Option<Vec<u8>> = None;
    for attempt in 0..TEMP_TRIES {
        let (temp, _) = file_name_concat(&dir, &temp_base(attempt));
        match make(&temp) {
            Ok(()) => {
                made = Some(temp);
                break;
            }
            Err(e) if e.raw_os_error() == Some(EEXIST) => {}
            Err(e) => return Tried::Failed(errno_of(&e)),
        }
    }
    let Some(temp) = made else {
        return Tried::Failed(EEXIST);
    };
    let renamed = fs::rename(os_from_bytes(&temp), os_from_bytes(dest));
    if unlink_after || renamed.is_err() {
        // Ignored: the temporary is ours, and whether it is still there is
        // exactly what the rename decided.
        let _ = fs::remove_file(os_from_bytes(&temp));
    }
    match renamed {
        Ok(()) => Tried::Made,
        Err(e) => Tried::Failed(errno_of(&e)),
    }
}

/// How many temporary names [`replace_via_temp`] tries.
const TEMP_TRIES: u32 = 64;

/// gnulib's `CuXXXXXX`, with the six letters taken from the process id, the
/// clock and the attempt -- all that is needed is a name nobody else has.
fn temp_base(attempt: u32) -> Vec<u8> {
    const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut seed = u64::from(std::process::id())
        ^ std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::from(d.subsec_nanos()))
        ^ u64::from(attempt).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut name = b"Cu".to_vec();
    for _ in 0..6 {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let pick = usize::try_from(seed >> 58)
            .unwrap_or(0)
            .checked_rem(LETTERS.len())
            .unwrap_or(0);
        name.push(LETTERS.get(pick).copied().unwrap_or(b'X'));
    }
    name
}

// ------------------------------------------------------------ the system ---

/// Whether two files are one: the same device and inode.
#[cfg(unix)]
fn same_inode(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev() && a.ino() == b.ino()
}

#[cfg(unix)]
fn nlink(m: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.nlink()
}

#[cfg(unix)]
fn dev(m: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.dev()
}

#[cfg(unix)]
fn ino(m: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    m.ino()
}

/// No inode numbers off unix: no two files are known to be one, which makes
/// the checks that ask fall through to the link attempt.
#[cfg(not(unix))]
fn same_inode(_a: &fs::Metadata, _b: &fs::Metadata) -> bool {
    false
}

#[cfg(not(unix))]
fn nlink(_m: &fs::Metadata) -> u64 {
    1
}

#[cfg(not(unix))]
fn dev(_m: &fs::Metadata) -> u64 {
    0
}

#[cfg(not(unix))]
fn ino(_m: &fs::Metadata) -> u64 {
    0
}

/// GNU's `openat_safer (AT_FDCWD, d, O_PATHSEARCH | O_DIRECTORY |
/// (no_follow ? O_NOFOLLOW : 0))`, for its answer only: whether `d` is a
/// directory, and if not, the errno that says why -- `ENOTDIR`, `ENOENT`, or
/// `ELOOP` for `-n` and a symbolic link.
#[cfg(unix)]
fn open_directory(d: &std::ffi::OsStr, no_follow: bool) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    /// The Linux values, which the x86_64-slateos ABI shares.
    const O_DIRECTORY: i32 = 0o200_000;
    const O_NOFOLLOW: i32 = 0o400_000;
    const O_PATH: i32 = 0o10_000_000;
    let mut flags = O_PATH | O_DIRECTORY;
    if no_follow {
        flags |= O_NOFOLLOW;
    }
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags)
        .open(d)
        .map(drop)
}

/// Off unix, the same question asked of the metadata.
#[cfg(not(unix))]
fn open_directory(d: &std::ffi::OsStr, no_follow: bool) -> io::Result<()> {
    let m = if no_follow {
        fs::symlink_metadata(d)?
    } else {
        fs::metadata(d)?
    };
    if m.is_dir() {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(ENOTDIR))
    }
}

/// The two link calls, on names as bytes.
mod sys {
    use super::os_from_bytes;
    use std::io;

    /// `linkat (AT_FDCWD, source, AT_FDCWD, dest, follow ?
    /// AT_SYMLINK_FOLLOW : 0)`. `std::fs::hard_link` cannot follow: it passes
    /// no flags, which is `-P`.
    #[cfg(unix)]
    pub fn link(source: &[u8], dest: &[u8], follow: bool) -> io::Result<()> {
        use std::ffi::CString;
        unsafe extern "C" {
            fn linkat(
                olddirfd: i32,
                oldpath: *const core::ffi::c_char,
                newdirfd: i32,
                newpath: *const core::ffi::c_char,
                flags: i32,
            ) -> i32;
        }
        /// `AT_FDCWD` and `AT_SYMLINK_FOLLOW`: the Linux values, which the
        /// x86_64-slateos ABI shares.
        const AT_FDCWD: i32 = -100;
        const AT_SYMLINK_FOLLOW: i32 = 0x400;
        let source = CString::new(source).map_err(|_| io::Error::from_raw_os_error(22))?;
        let dest = CString::new(dest).map_err(|_| io::Error::from_raw_os_error(22))?;
        let flags = if follow { AT_SYMLINK_FOLLOW } else { 0 };
        // SAFETY: both strings are NUL-terminated and live across the call;
        // `linkat` reads them and keeps no pointer to either.
        let r = unsafe { linkat(AT_FDCWD, source.as_ptr(), AT_FDCWD, dest.as_ptr(), flags) };
        if r == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Off unix, the standard library's, which cannot follow either way.
    #[cfg(not(unix))]
    pub fn link(source: &[u8], dest: &[u8], _follow: bool) -> io::Result<()> {
        std::fs::hard_link(os_from_bytes(source), os_from_bytes(dest))
    }

    /// `symlinkat (source, AT_FDCWD, dest)`: the text written verbatim.
    #[cfg(unix)]
    pub fn symlink(source: &[u8], dest: &[u8]) -> io::Result<()> {
        std::os::unix::fs::symlink(os_from_bytes(source), os_from_bytes(dest))
    }

    /// Windows needs to know at creation time whether the link points at a
    /// directory, and asks about the path the link will resolve to -- relative
    /// to the link's own directory, as the link's text will be read.
    #[cfg(windows)]
    pub fn symlink(source: &[u8], dest: &[u8]) -> io::Result<()> {
        use std::path::Path;
        let target = os_from_bytes(source);
        let link = os_from_bytes(dest);
        let base = Path::new(&link).parent().unwrap_or_else(|| Path::new(""));
        if base.join(&target).is_dir() {
            std::os::windows::fs::symlink_dir(&target, &link)
        } else {
            std::os::windows::fs::symlink_file(&target, &link)
        }
    }

    #[cfg(not(any(unix, windows)))]
    pub fn symlink(_source: &[u8], _dest: &[u8]) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symbolic links are not supported on this platform",
        ))
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
    use coreutils::yesno::Canned;
    use scratchdir::ScratchDir;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `parse_args` with `POSIXLY_CORRECT` pinned off.
    fn parse(items: &[&str]) -> Result<Request, getopt::Error> {
        parse_args(&args(items), false)
    }

    fn run_parse(items: &[&str]) -> (LnFlags, Vec<String>) {
        match parse(items).unwrap() {
            Request::Run(f, p) => (
                f,
                p.iter()
                    .map(|o| String::from_utf8_lossy(o).into_owned())
                    .collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse(items).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --

    /// Measured against GNU on 2026-09-25: `POSIXLY_CORRECT=1 ln a b -s` takes
    /// `-s` for a third operand, where without the variable it is an option.
    #[test]
    fn posixly_correct_makes_an_option_after_an_operand_an_operand() {
        let argv = args(&["a", "b", "-s"]);
        let Ok(Request::Run(flags, paths)) = parse_args(&argv, true) else {
            panic!("expected a run");
        };
        assert!(!flags.symbolic);
        assert_eq!(paths.len(), 3);
        let (flags, paths) = run_parse(&["a", "b", "-s"]);
        assert!(flags.symbolic);
        assert_eq!(paths, ["a", "b"]);
    }

    #[test]
    fn every_option_sets_its_flag() {
        let (f, _) = run_parse(&["-s", "-v", "-n", "-r", "-T", "-L", "-d", "a", "b"]);
        assert!(f.symbolic && f.verbose && f.no_dereference && f.relative);
        assert!(f.no_target_directory && f.logical && f.hard_dir_link);
        assert!(
            !run_parse(&["-L", "-P", "a"]).0.logical,
            "the last of -L/-P wins"
        );
        let (f, _) = run_parse(&["-S", ".old", "a", "b"]);
        assert!(f.make_backups, "-S asks for backups as well as naming them");
        assert_eq!(f.backup_suffix, Some(OsString::from(".old")));
        let (f, _) = run_parse(&["--backup=numbered", "-b", "a", "b"]);
        assert_eq!(f.version_control, Some(OsString::from("numbered")));
    }

    /// One field, two spellings, the last one wins: `ln -if` does not ask.
    #[test]
    fn force_and_interactive_cancel_each_other() {
        let (f, _) = run_parse(&["-i", "-f", "a", "b"]);
        assert!(f.remove_existing && !f.interactive);
        let (f, _) = run_parse(&["-f", "-i", "a", "b"]);
        assert!(!f.remove_existing && f.interactive);
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse(&["--help"]).unwrap(), Request::Help);
        assert_eq!(parse(&["--version"]).unwrap(), Request::Version);
    }

    /// `--s` must stay ambiguous between `--suffix` and `--symbolic`, and the
    /// other two ambiguities keep GNU's table order.
    #[test]
    fn ambiguous_abbreviations_are_refused_in_table_order() {
        assert_eq!(
            fail(&["--s"]).sentence,
            "option '--s' is ambiguous; possibilities: '--suffix' '--symbolic'"
        );
        assert_eq!(
            fail(&["--n"]).sentence,
            "option '--n' is ambiguous; possibilities: '--no-dereference' '--no-target-directory'"
        );
        assert_eq!(
            fail(&["--v"]).sentence,
            "option '--v' is ambiguous; possibilities: '--verbose' '--version'"
        );
        assert!(run_parse(&["--f", "a", "b"]).0.remove_existing);
    }

    #[test]
    fn unknown_options_are_refused() {
        assert_eq!(fail(&["-q", "a", "b"]).sentence, "invalid option -- 'q'");
        assert_eq!(
            fail(&["--zzz=1", "a", "b"]).sentence,
            "unrecognized option '--zzz=1'"
        );
        assert_eq!(
            fail(&["--symbolic=yes", "a", "b"]).sentence,
            "option '--symbolic' doesn't allow an argument"
        );
    }

    /// `-t` is checked as it is read, with plain diagnostics.
    #[test]
    fn target_directory_is_checked_while_parsing() {
        let dir = ScratchDir::new("ln-t-parse");
        let d = dir.path("d");
        fs::create_dir(&d).unwrap();
        let f = dir.path("f");
        fs::write(&f, b"x").unwrap();
        let d = d.to_string_lossy().into_owned();
        let f = f.to_string_lossy().into_owned();
        assert_eq!(
            run_parse(&["-t", &d, "a"]).0.target_directory,
            Some(d.as_bytes().to_vec())
        );
        let e = fail(&["-t", &d, "-t", &d, "a"]);
        assert_eq!(e.sentence, "multiple target directories specified");
        assert!(e.referral.is_none());
        let e = fail(&["-t", &f, "a"]);
        assert_eq!(
            e.sentence,
            format!("target {} is not a directory", quoteaf(f.as_bytes()))
        );
        let missing = dir.path("nosuch").to_string_lossy().into_owned();
        assert!(
            fail(&["-t", &missing])
                .sentence
                .starts_with("failed to access ")
        );
    }

    /// The reason this file was first rewritten: an operand that is not UTF-8.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        match parse_args(&[OsString::from("-s"), bad, OsString::from("d")], false).unwrap() {
            Request::Run(f, p) => {
                assert!(f.symbolic);
                assert_eq!(p, vec![vec![b'a', 0x80, b'b'], b"d".to_vec()]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    // ------------------------------------------------------------ names --

    #[test]
    fn names_are_gnulibs() {
        assert_eq!(last_component(b"a/b"), b"b");
        assert_eq!(last_component(b"a/b/"), b"b/");
        assert_eq!(last_component(b"/"), b"");
        assert_eq!(last_component(b"b"), b"b");
        assert_eq!(base_len(b"b//"), 1);
        assert_eq!(base_len(b"/"), 1);
        assert_eq!(file_name_concat(b".", b"a"), (b"./a".to_vec(), 2));
        assert_eq!(file_name_concat(b"d/", b"a"), (b"d/a".to_vec(), 2));
        assert_eq!(file_name_concat(b"/", b"a"), (b"/a".to_vec(), 1));
        assert_eq!(dir_name(b"a"), b".");
        assert_eq!(dir_name(b"d/a"), b"d");
        assert_eq!(dir_name(b"d//a"), b"d");
        assert_eq!(dir_name(b"/a"), b"/");
    }

    // ------------------------------------------------------------ linking --

    /// Run `ln` with `items`, in `dir`, the way `main` would; the outcome, and
    /// what went to standard output and standard error.
    fn ln_in(dir: &ScratchDir, items: &[&str], answers: &[&str]) -> (bool, String, String) {
        let rooted: Vec<String> = items
            .iter()
            .map(|a| {
                if a.starts_with('-') || a.is_empty() {
                    (*a).to_string()
                } else {
                    dir.path(a).to_string_lossy().into_owned()
                }
            })
            .collect();
        let rooted: Vec<&str> = rooted.iter().map(String::as_str).collect();
        let Request::Run(flags, files) = parse(&rooted).unwrap() else {
            panic!("expected a run");
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut canned = Canned::new(answers);
        let ok = Ln {
            flags: &flags,
            backup: Backup::disabled(),
            dest_set: None,
            out: &mut out,
            err: &mut err,
            answers: &mut canned,
        }
        .run(&files);
        let base = dir.dir().to_string_lossy().into_owned();
        let tidy = |b: Vec<u8>| String::from_utf8_lossy(&b).replace(&format!("{base}/"), "");
        (matches!(ok, Ok(true)), tidy(out), tidy(err))
    }

    #[test]
    #[cfg(unix)]
    fn force_replaces_a_symbolic_link_in_one_rename() {
        let dir = ScratchDir::new("ln-sf");
        fs::write(dir.path("a"), b"A").unwrap();
        fs::write(dir.path("b"), b"B").unwrap();
        let (ok, _, err) = ln_in(&dir, &["-s", "a", "l"], &[]);
        assert!(ok, "{err}");
        // Without -f the name is taken.
        let (ok, _, err) = ln_in(&dir, &["-s", "b", "l"], &[]);
        assert!(!ok);
        assert!(
            err.contains("failed to create symbolic link 'l': File exists"),
            "{err}"
        );
        let (ok, _, err) = ln_in(&dir, &["-sf", "b", "l"], &[]);
        assert!(ok, "{err}");
        assert_eq!(
            fs::read_link(dir.path("l")).unwrap(),
            dir.path("b"),
            "the link now names b"
        );
        // Nothing left lying about from the temporary name.
        let names: Vec<_> = fs::read_dir(dir.dir()).unwrap().collect();
        assert_eq!(names.len(), 3);
    }

    #[test]
    #[cfg(unix)]
    fn verbose_names_the_link_and_any_backup() {
        let dir = ScratchDir::new("ln-v");
        fs::write(dir.path("a"), b"A").unwrap();
        fs::write(dir.path("b"), b"B").unwrap();
        let (ok, out, _) = ln_in(&dir, &["-v", "a", "c"], &[]);
        assert!(ok);
        assert_eq!(out, "'c' => 'a'\n");
        let (ok, out, err) = ln_in(&dir, &["-svb", "a", "b"], &[]);
        assert!(ok, "{err}");
        assert_eq!(out, "'b~' ~ 'b' -> 'a'\n");
        assert_eq!(fs::read(dir.path("b~")).unwrap(), b"B");
    }

    #[test]
    #[cfg(unix)]
    fn a_hard_link_onto_itself_is_refused_before_anything_is_removed() {
        let dir = ScratchDir::new("ln-same");
        fs::write(dir.path("a"), b"A").unwrap();
        let (ok, _, err) = ln_in(&dir, &["-f", "a", "a"], &[]);
        assert!(!ok);
        assert!(err.contains("'a' and 'a' are the same file"), "{err}");
        assert_eq!(fs::read(dir.path("a")).unwrap(), b"A");
    }

    #[test]
    #[cfg(unix)]
    fn directories_are_neither_hard_linked_nor_overwritten() {
        let dir = ScratchDir::new("ln-dirs");
        fs::create_dir(dir.path("d")).unwrap();
        fs::write(dir.path("a"), b"A").unwrap();
        let (ok, _, err) = ln_in(&dir, &["d", "x"], &[]);
        assert!(!ok);
        assert!(err.contains("hard link not allowed for directory"), "{err}");
        // `-T`: the directory is the destination itself, not where to put it.
        let (ok, _, err) = ln_in(&dir, &["-Tf", "a", "d"], &[]);
        assert!(!ok);
        assert!(err.contains("cannot overwrite directory"), "{err}");
    }

    #[test]
    #[cfg(unix)]
    fn interactive_asks_and_a_no_keeps_the_destination() {
        let dir = ScratchDir::new("ln-i");
        fs::write(dir.path("a"), b"A").unwrap();
        fs::write(dir.path("b"), b"B").unwrap();
        let (ok, _, err) = ln_in(&dir, &["-i", "a", "b"], &["n\n"]);
        assert!(!ok);
        assert_eq!(err, "ln: replace 'b'? ");
        assert_eq!(fs::read(dir.path("b")).unwrap(), b"B");
        let (ok, _, _) = ln_in(&dir, &["-i", "a", "b"], &["y\n"]);
        assert!(ok);
        assert_eq!(fs::read(dir.path("b")).unwrap(), b"A");
    }

    #[test]
    #[cfg(unix)]
    fn relative_writes_the_path_from_the_links_directory() {
        let dir = ScratchDir::new("ln-r");
        fs::create_dir(dir.path("sub")).unwrap();
        fs::write(dir.path("a"), b"A").unwrap();
        let (ok, _, err) = ln_in(&dir, &["-sr", "a", "sub/l"], &[]);
        assert!(ok, "{err}");
        assert_eq!(fs::read_link(dir.path("sub/l")).unwrap(), Path::new("../a"));
        let (ok, _, err) = ln_in(&dir, &["-r", "a", "x"], &[]);
        assert!(!ok);
        assert_eq!(err, "ln: cannot do --relative without --symbolic\n");
    }

    #[test]
    #[cfg(unix)]
    fn no_dereference_replaces_a_link_to_a_directory_instead_of_entering_it() {
        let dir = ScratchDir::new("ln-n");
        fs::create_dir(dir.path("d")).unwrap();
        fs::write(dir.path("a"), b"A").unwrap();
        std::os::unix::fs::symlink(dir.path("d"), dir.path("ld")).unwrap();
        // Without -n the link to a directory is that directory: `ld/a`.
        let (ok, _, err) = ln_in(&dir, &["-s", "a", "ld"], &[]);
        assert!(ok, "{err}");
        assert!(dir.path("d/a").symlink_metadata().is_ok());
        // With -n and -f, `ld` itself is replaced.
        let (ok, _, err) = ln_in(&dir, &["-snf", "a", "ld"], &[]);
        assert!(ok, "{err}");
        assert_eq!(fs::read_link(dir.path("ld")).unwrap(), dir.path("a"));
    }

    #[test]
    #[cfg(unix)]
    fn a_just_created_hard_link_is_not_overwritten() {
        let dir = ScratchDir::new("ln-dest-set");
        fs::create_dir(dir.path("x")).unwrap();
        fs::create_dir(dir.path("y")).unwrap();
        fs::create_dir(dir.path("d")).unwrap();
        fs::write(dir.path("x/a"), b"X").unwrap();
        fs::write(dir.path("y/a"), b"Y").unwrap();
        let (ok, _, err) = ln_in(&dir, &["-f", "x/a", "y/a", "d"], &[]);
        assert!(!ok);
        assert!(
            err.contains("will not overwrite just-created 'd/a' with 'y/a'"),
            "{err}"
        );
        assert_eq!(fs::read(dir.path("d/a")).unwrap(), b"X");
    }

    #[test]
    fn operand_errors_are_gnus() {
        let dir = ScratchDir::new("ln-operands");
        let (ok, _, err) = ln_in(&dir, &[], &[]);
        assert!(!ok);
        assert_eq!(
            err,
            "ln: missing file operand\nTry 'ln --help' for more information.\n"
        );
        let (ok, _, err) = ln_in(&dir, &["-T", "a"], &[]);
        assert!(!ok);
        assert!(
            err.starts_with("ln: missing destination file operand after 'a'\n"),
            "{err}"
        );
        let (ok, _, err) = ln_in(&dir, &["-T", "a", "b", "c"], &[]);
        assert!(!ok);
        assert!(err.starts_with("ln: extra operand 'c'\n"), "{err}");
        let (ok, _, err) = ln_in(&dir, &["a", "b", "nosuch"], &[]);
        assert!(!ok);
        assert_eq!(err, "ln: target 'nosuch': No such file or directory\n");
    }
}
