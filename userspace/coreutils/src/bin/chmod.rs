//! `chmod` — change file mode bits.
//!
//! # What was here before
//!
//! A `Vec<String>` argv, a hand-written symbolic-mode parser, and a walk that
//! re-parsed the mode string once per file. The parser is gone: it is
//! [`modechange`] now, whose module docs list the thirteen measured ways it and
//! its three siblings were wrong. What is left here is argv, the walk, and the
//! diagnostics — and each of those had defects of its own:
//!
//! 1. **`env::args()` panicked on a non-UTF-8 argument**, which on this OS is a
//!    legal file name.
//! 2. **`-c`, `-f`, `-v`, `--reference`, `--preserve-root`, `--help` and
//!    `--version` did not exist.** `chmod --reference=f g` treated
//!    `--reference=f` as an unrecognised option and `chmod --help` printed a
//!    usage error.
//! 3. **The mode was recompiled for every file** — which is not merely wasteful:
//!    it is why the setuid-on-a-directory rule could not be expressed, since
//!    "did the string *mention* setgid" is a property of the string that has to
//!    outlive the parse.
//! 4. **The umask was never read**, so `chmod +w f` granted `a+w`.
//! 5. **Every error was worded `chmod: PATH: MESSAGE`**, where GNU words each
//!    failure for what it was: `cannot access`, `cannot read directory`,
//!    `changing permissions of`. A script grepping for one of those found
//!    nothing.
//! 6. **The mode operand's leading `-` was guessed at** with a table of
//!    characters that "may follow a `-` in a mode". GNU does not guess: every
//!    one of those characters is a *declared option letter* taking an optional
//!    argument, which is why `chmod -w` works and `chmod -Rw` is an error rather
//!    than "recurse, remove write".
//!
//! # `-R` is recursion and `-r` is a mode
//!
//! POSIX allows the mode operand to begin with `-`, so a leading dash is not
//! proof of an option. GNU resolves this inside `getopt` rather than beside it:
//! its option string is
//!
//! ```text
//! Rcfvr::w::x::X::s::t::u::g::o::a::,::+::=::0::1::2::3::4::5::6::7::
//! ```
//!
//! — every permission letter, every `who` letter, every operator and every
//! octal digit is an option taking an *optional* argument. `chmod -w f` is
//! therefore the option `w` with no value, and the mode string is rebuilt from
//! **the whole argv word** it came out of (`argv[optind - 1]`), several such
//! words being joined with commas. That last detail is why this file asks the
//! parser for [`current_word`](coreutils::getopt::Parser::current_word):
//! reconstructing `-` + `w` would turn `chmod -Rw d` into a silent recursive
//! `-w`, where GNU answers `chmod: invalid mode: ‘-Rw’`. Measured.
//!
//! Getting the `-r`/`-R` distinction wrong is not cosmetic: `chmod -r f` is the
//! ordinary way to make a file unreadable, and reading its `-r` as "recurse"
//! leaves the file readable while reporting a usage error.
//!
//! # Symbolic links, and why `-R` never touches one
//!
//! Under `-R`, every path after the operand was chosen by whoever wrote the
//! files, not by the caller — which under `/tmp`, a download directory, or a
//! user's home is not the same person. A symbolic link met during the walk is
//! therefore an instruction from a stranger, and this program obeys none of
//! them (`known-issues.md` → `B-chmod-FOLLOWS-SYMLINKS-WHILE-RECURSING`):
//!
//! 1. **It is not descended into.** The recursion used to test `path.is_dir()`,
//!    which follows links, so `srv/x -> /etc` turned `chmod -R 777 srv/` into
//!    `chmod -R 777 /etc`. It now tests the link's own type.
//! 2. **It is skipped entirely**, as GNU chmod skips it (`process_file` acts
//!    only `if (! S_ISLNK (…))`). A symlink's own mode bits are never consulted
//!    by anything, so the only effect a `chmod` on one can have is on its
//!    target — and `srv/x -> /etc/shadow` would otherwise make `/etc/shadow`
//!    world-writable.
//!
//! A link named directly on the command line *is* followed, also matching GNU
//! (`fts` with `FTS_COMFOLLOW`): the caller typed that name and can see what it
//! is. Measured: `chmod 755 s/link` changes `s/real`.
//!
//! Built only on unix-family targets (our x86_64-slateos presents as
//! linux-musl, so `cfg(unix)` matches). On non-unix hosts — Windows, where
//! `cargo test --workspace` runs — a stub `main` keeps the workspace
//! compile-clean, and everything that does not touch a file is still compiled
//! and tested there.

#![cfg_attr(not(unix), allow(dead_code))]

#[cfg(not(unix))]
use coreutils::diag;
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote, quoteaf_os};
use modechange::{Changes, compile, permission_string};
use std::ffi::{OsStr, OsString};

/// `chmod`'s usage status is 1 — measured: `chmod; echo $?` prints 1.
const CHMOD: Program = Program::new("chmod", 1);

/// GNU `chmod`'s `getopt_long` string, exactly.
///
/// See the module docs for what the twenty-two optional-argument letters after
/// `Rcfv` are doing there. They are what makes `chmod -w f` parse.
const SHORT_OPTIONS: &str = concat!(
    "Rcfvr::w::x::X::s::t::u::g::o::a::,::+::=::",
    "0::1::2::3::4::5::6::7::"
);

/// GNU `chmod`'s `long_options[]`, in its declaration order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("changes", Takes::Nothing),
    ("recursive", Takes::Nothing),
    ("no-preserve-root", Takes::Nothing),
    ("preserve-root", Takes::Nothing),
    ("quiet", Takes::Nothing),
    ("reference", Takes::Required),
    ("silent", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// `--quiet` and `--silent` are one option. Without this the parser would call
/// `--s` ambiguous, which it is not: it resolves to `silent`, an alias of
/// `quiet`, and GNU accepts it.
const LONG_ALIASES: &[(&str, &str)] = &[("silent", "quiet")];

/// How much `chmod` says about each file it visits.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Verbosity {
    /// `-v`: a line for every file, changed or not.
    High,
    /// `-c`: a line only for a file whose mode actually moved.
    ChangesOnly,
    /// The default: nothing.
    Off,
}

/// Where the new mode comes from.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Source {
    /// A mode string, already compiled, along with the umask to apply to any
    /// clause that named no `who`.
    Spec(Changes),
    /// `--reference=RFILE`: whatever mode that file turns out to have. Resolved
    /// late because it needs a `stat`, and this parse touches no filesystem.
    Reference(OsString),
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Settings {
    recursive: bool,
    /// `-f`: suppress the diagnostics, but not the exit status.
    force_silent: bool,
    verbosity: Verbosity,
    /// `--preserve-root`: refuse to recurse from `/`.
    preserve_root: bool,
    /// Set by any mode given as option letters, which is the only way a mode
    /// can be surprising enough to warrant the check: `chmod -w f` under a
    /// umask does less than it looks like it does.
    diagnose_surprises: bool,
    source: Source,
    files: Vec<OsString>,
}

/// What the command line asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Request {
    Help,
    Version,
    Run(Box<Settings>),
}

fn help_text() -> String {
    "\
Usage: chmod [OPTION]... MODE[,MODE]... FILE...
  or:  chmod [OPTION]... OCTAL-MODE FILE...
  or:  chmod [OPTION]... --reference=RFILE FILE...
Change the mode of each FILE to MODE.
With --reference, change the mode of each FILE to that of RFILE.

  -c, --changes          like verbose but report only when a change is made
  -f, --silent, --quiet  suppress most error messages
  -v, --verbose          output a diagnostic for every file processed
      --no-preserve-root  do not treat '/' specially (the default)
      --preserve-root    fail to operate recursively on '/'
      --reference=RFILE  use RFILE's mode instead of specifying MODE values.
                         RFILE is always dereferenced if a symbolic link.
  -R, --recursive        change files and directories recursively
      --help        display this help and exit
      --version     output version information and exit

Each MODE is of the form '[ugoa]*([-+=]([rwxXst]*|[ugo]))+|[-+=][0-7]+'.
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Whether `flag` is one of the option letters that are really mode text.
fn is_mode_letter(flag: u8) -> bool {
    matches!(flag, b'r' | b'w' | b'x' | b'X' | b's' | b't')
        || matches!(flag, b'u' | b'g' | b'o' | b'a')
        || matches!(flag, b',' | b'+' | b'=')
        || flag.is_ascii_digit() && flag != b'8' && flag != b'9'
}

/// Parse `chmod`'s argv, compiling the mode as it goes.
///
/// The order of the three failures at the end is upstream's and is observable:
/// `chmod xyz` is `missing operand after ‘xyz’` rather than `invalid mode`,
/// because the operand count is checked before the string is compiled.
///
/// # Errors
///
/// An unknown option, a mode given both as option letters and by
/// `--reference`, no operands, or a mode string that is not one.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut recursive = false;
    let mut force_silent = false;
    let mut verbosity = Verbosity::Off;
    let mut preserve_root = false;
    let mut reference: Option<OsString> = None;
    // The mode as GNU builds it: the whole argv words that carried mode
    // letters, joined with commas. `None` until one is seen, which is what
    // distinguishes `chmod -w` (mode, no file) from `chmod` (nothing at all).
    let mut spec: Option<Vec<u8>> = None;
    let mut operands: Vec<OsString> = Vec::new();

    let mut parser = CHMOD.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, LONG_ALIASES);
    while let Some(item) = parser.next() {
        // Taken before the match: `current_word` describes the item just
        // yielded, and the next `next()` will move it on.
        let word = parser.current_word().map(OsString::as_os_str);
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Short(b'R', _) | Opt::Long("recursive", _) => recursive = true,
            Opt::Short(b'c', _) | Opt::Long("changes", _) => verbosity = Verbosity::ChangesOnly,
            // Both spellings, because an exact long option resolves to the name
            // that was typed rather than to the alias's target — the alias map
            // settles ambiguity and nothing else. See `resolve_long_aliased`.
            Opt::Short(b'f', _) | Opt::Long("quiet" | "silent", _) => force_silent = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => verbosity = Verbosity::High,
            Opt::Long("preserve-root", _) => preserve_root = true,
            Opt::Long("no-preserve-root", _) => preserve_root = false,
            Opt::Long("reference", value) => reference = value,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(flag, _) if is_mode_letter(flag) => {
                // The whole word, not the letter: see the module docs.
                let fragment = word.map(os_bytes).unwrap_or_default();
                let accumulated = spec.get_or_insert_with(Vec::new);
                if !accumulated.is_empty() {
                    accumulated.push(b',');
                }
                accumulated.extend_from_slice(&fragment);
            }
            // Unreachable: the table lists nothing else, and every entry is
            // handled above. Refusing rather than ignoring, so an option added
            // to the table without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(CHMOD.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(other, _) => return Err(CHMOD.invalid_option(other)),
        }
    }

    // Only a mode spelt as option *letters* collides with `--reference`; a mode
    // operand does not, because `--reference` never consumes one.
    if reference.is_some() && spec.is_some() {
        return Err(
            CHMOD.usage_referring("cannot combine mode and --reference options".to_string())
        );
    }
    // Hence the three ways there is no mode operand to take: `--reference`
    // supplied the mode and leaves the operand alone — measured, `chmod
    // --reference=f 644 g` reports `cannot access '644'`, i.e. it treated `644`
    // as a second file; the option letters already spelt one; or there is
    // nothing left on the command line at all.
    let mode_from_operand = if reference.is_some() || spec.is_some() || operands.is_empty() {
        None
    } else {
        Some(operands.remove(0))
    };

    if operands.is_empty() {
        // Which of the two wordings depends on where the mode came from: only
        // an operand can be named, since option letters were spread over words
        // that no longer exist as a unit.
        return Err(match &mode_from_operand {
            Some(mode) => CHMOD.usage_referring(format!(
                "missing operand after {}",
                quote(&os_bytes(mode.as_os_str()))
            )),
            None => CHMOD.usage_referring("missing operand".to_string()),
        });
    }

    let diagnose_surprises = spec.is_some();
    let source = match reference {
        Some(rfile) => Source::Reference(rfile),
        None => {
            let text = match &mode_from_operand {
                Some(mode) => os_bytes(mode.as_os_str()).into_owned(),
                // One of the two is always set here: `reference` is `None`, and
                // a missing mode was caught as `missing operand` above.
                None => spec.clone().unwrap_or_default(),
            };
            let Some(changes) = compile(&text) else {
                return Err(CHMOD.usage_referring(format!("invalid mode: {}", quote(&text))));
            };
            Source::Spec(changes)
        }
    };

    Ok(Request::Run(Box::new(Settings {
        recursive,
        force_silent,
        verbosity,
        preserve_root,
        diagnose_surprises,
        source,
        files: operands,
    })))
}

// ------------------------------------------------------------ diagnostics ---

/// What happened to one file, in the vocabulary `describe_change` speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Outcome {
    /// The mode was set, and it moved.
    Succeeded,
    /// `chmod(2)` failed, or the surprise check below condemned the result.
    Failed,
    /// The mode was set and came out exactly as it already was.
    NoChangeRequested,
    /// A symbolic link met during the walk: neither it nor its target touched.
    NotApplied,
    /// The file could not be stat'ed.
    NoStat,
}

/// GNU's `describe_change`, verbatim in wording.
///
/// The old and new modes are printed both in octal and as `rwxrwxrwx`, which is
/// the whole reason `-v` is worth having: `4644` and `0644` differ by a bit that
/// the octal makes easy to miss and that the `S` in `rwSr--r--` does not.
fn describe_change(file: &OsStr, outcome: Outcome, old_mode: u32, new_mode: u32) -> Option<String> {
    let quoted = quoteaf_os(file);
    let old_m = old_mode & modechange::CHMOD_MODE_BITS;
    let new_m = new_mode & modechange::CHMOD_MODE_BITS;
    Some(match outcome {
        Outcome::NotApplied => {
            format!("neither symbolic link {quoted} nor referent has been changed")
        }
        Outcome::NoStat => format!("{quoted} could not be accessed"),
        Outcome::NoChangeRequested => format!(
            "mode of {quoted} retained as {new_m:04o} ({})",
            permission_string(new_mode)
        ),
        Outcome::Succeeded => format!(
            "mode of {quoted} changed from {old_m:04o} ({}) to {new_m:04o} ({})",
            permission_string(old_mode),
            permission_string(new_mode)
        ),
        Outcome::Failed => format!(
            "failed to change mode of {quoted} from {old_m:04o} ({}) to {new_m:04o} ({})",
            permission_string(old_mode),
            permission_string(new_mode)
        ),
    })
}

/// GNU's surprise check, for a mode written as option letters.
///
/// `chmod -w f` looks like it removes write from everybody, and under a umask it
/// does not: a clause with no `who` is masked by the umask, so under `umask 022`
/// only the owner's bit goes. Rather than silently doing less than asked, GNU
/// compares the result against what the same string would have done with no
/// umask, and complains if any bit survived that the naive reading would have
/// cleared. The complaint is also a *failure* — it changes the exit status —
/// because the file is not in the state the command line described.
///
/// Returns the message body, without the file name that precedes it.
fn surprise(new_mode: u32, naively_expected: u32) -> Option<String> {
    if new_mode & !naively_expected == 0 {
        return None;
    }
    Some(format!(
        "new permissions are {}, not {}",
        permission_string(new_mode),
        permission_string(naively_expected)
    ))
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    diag!("chmod: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

// ------------------------------------------------------------------- unix ---

#[cfg(unix)]
mod imp {
    use super::{
        Outcome, Request, Settings, Source, Verbosity, describe_change, help_text, parse_args,
        surprise,
    };
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::quote::{quoteaf_os, quotef_os};
    use modechange::{Changes, adjust, from_reference};
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io::{self, Write};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;

    // SAFETY (declaration): `umask` is POSIX, takes and returns `mode_t`, and
    // has no failure mode. `mode_t` is `u32` on Linux and on x86_64-slateos.
    unsafe extern "C" {
        fn umask(mask: u32) -> u32;
    }

    /// Read the process umask.
    ///
    /// POSIX offers no way to read it without writing it, so this does what GNU
    /// does: set it to 0 and keep the answer. Not restoring it is deliberate and
    /// upstream's — `chmod` creates no files, and the process is about to exit.
    fn read_umask() -> u32 {
        // SAFETY: no arguments to get wrong, no pointers, no failure.
        unsafe { umask(0) }
    }

    /// Everything the walk needs that does not change from file to file.
    struct Job {
        settings: Settings,
        changes: Changes,
        umask_value: u32,
        /// `(dev, ino)` of `/`, when `--preserve-root` and `-R` are both on.
        root_dev_ino: Option<(u64, u64)>,
        status: u8,
    }

    impl Job {
        /// Report a failure, unless `-f` said not to. The status moves either
        /// way: silence is about the message, not about the answer.
        fn fail(&mut self, message: &str) {
            if !self.settings.force_silent {
                diag!("chmod: {message}");
            }
            self.status = 1;
        }

        fn say(&self, line: &str) {
            println!("{line}");
        }
    }

    pub fn main() -> ExitCode {
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let settings = match parse_args(&args) {
            Ok(Request::Help) => {
                print!("{}", help_text());
                return ExitCode::SUCCESS;
            }
            Ok(Request::Version) => {
                println!("chmod (SlateOS coreutils) 0.1.0");
                return ExitCode::SUCCESS;
            }
            Ok(Request::Run(settings)) => *settings,
            Err(e) => {
                diag!("chmod: {e}");
                return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
            }
        };

        // The umask matters only to a mode string; `--reference` copies bits
        // that are already decided. GNU reads it in exactly that branch.
        let (changes, umask_value) = match &settings.source {
            Source::Spec(changes) => (changes.clone(), read_umask()),
            Source::Reference(rfile) => {
                // Dereferenced: `metadata`, not `symlink_metadata`. GNU's help
                // text promises this in as many words.
                match fs::metadata(Path::new(rfile)) {
                    Ok(meta) => (from_reference(meta.permissions().mode()), 0),
                    Err(e) => {
                        diag!(
                            "chmod: failed to get attributes of {}: {}",
                            quoteaf_os(rfile),
                            strerror(&e)
                        );
                        return ExitCode::from(1);
                    }
                }
            }
        };

        let root_dev_ino = if settings.recursive && settings.preserve_root {
            match fs::metadata(Path::new("/")) {
                Ok(meta) => Some((meta.dev(), meta.ino())),
                Err(e) => {
                    diag!(
                        "chmod: failed to get attributes of {}: {}",
                        quoteaf_os("/"),
                        strerror(&e)
                    );
                    return ExitCode::from(1);
                }
            }
        } else {
            None
        };

        let mut job = Job {
            settings,
            changes,
            umask_value,
            root_dev_ino,
            status: 0,
        };

        for file in job.settings.files.clone() {
            // Level 0 follows a symbolic link, as `FTS_COMFOLLOW` does: the
            // caller typed this name. Every path below it does not.
            visit(&mut job, &PathBuf::from(&file), true);
        }

        // A closed stdout must not pass for success when `-v` had things to say.
        if io::stdout().flush().is_err() {
            job.status = 1;
        }
        ExitCode::from(job.status)
    }

    /// Apply the mode to one path, and — under `-R`, and only for a real
    /// directory — to everything beneath it.
    ///
    /// Errors are reported and recorded rather than returned, because a walk
    /// that stops at the first failure leaves the caller with a tree in an
    /// unknown state: `chmod -R 700 ~` hitting one unreadable subdirectory used
    /// to abandon every sibling after it, silently leaving them world-readable.
    fn visit(job: &mut Job, path: &Path, top_level: bool) {
        // `metadata` follows, `symlink_metadata` does not. Which one is right is
        // the whole symlink policy in one line: at the top the caller named this
        // path, below it a stranger did.
        let meta = if top_level {
            fs::metadata(path)
        } else {
            fs::symlink_metadata(path)
        };
        let meta = match meta {
            Ok(m) => m,
            Err(e) => {
                job.fail(&format!(
                    "cannot access {}: {}",
                    quoteaf_os(path),
                    strerror(&e)
                ));
                if job.settings.verbosity == Verbosity::High
                    && let Some(line) = describe_change(path.as_os_str(), Outcome::NoStat, 0, 0)
                {
                    job.say(&line);
                }
                return;
            }
        };

        // A link below the top level is left alone entirely — see the module
        // docs. `-v` still says so, as GNU does.
        if !top_level && meta.file_type().is_symlink() {
            if job.settings.verbosity == Verbosity::High
                && let Some(line) = describe_change(path.as_os_str(), Outcome::NotApplied, 0, 0)
            {
                job.say(&line);
            }
            return;
        }

        if let Some((dev, ino)) = job.root_dev_ino
            && meta.dev() == dev
            && meta.ino() == ino
        {
            warn_about_root(job, path);
            return;
        }

        let old_mode = meta.permissions().mode();
        let is_dir = meta.file_type().is_dir();
        let new_mode = adjust(old_mode, is_dir, job.umask_value, &job.changes).mode;

        let mut outcome = match fs::set_permissions(path, fs::Permissions::from_mode(new_mode)) {
            Ok(()) => Outcome::Succeeded,
            Err(e) => {
                job.fail(&format!(
                    "changing permissions of {}: {}",
                    quoteaf_os(path),
                    strerror(&e)
                ));
                Outcome::Failed
            }
        };

        // Whether anything moved can only be answered by looking: the kernel
        // silently drops setuid and setgid in cases the caller cannot predict,
        // so a `chmod` that "succeeded" may have changed nothing at all.
        if job.settings.verbosity != Verbosity::Off && outcome == Outcome::Succeeded {
            let stored = if new_mode & 0o7000 != 0 {
                fs::metadata(path).map_or(new_mode, |m| m.permissions().mode())
            } else {
                new_mode
            };
            if (old_mode ^ stored) & modechange::CHMOD_MODE_BITS == 0 {
                outcome = Outcome::NoChangeRequested;
            }
        }

        if matches!(outcome, Outcome::Succeeded | Outcome::NoChangeRequested)
            && job.settings.diagnose_surprises
        {
            // The same string with no umask: what the command line looked like
            // it asked for.
            let naive = adjust(old_mode, is_dir, 0, &job.changes).mode;
            if let Some(body) = surprise(new_mode, naive) {
                job.fail(&format!("{}: {body}", quotef_os(path)));
                outcome = Outcome::Failed;
            }
        }

        if (job.settings.verbosity == Verbosity::High
            || (job.settings.verbosity == Verbosity::ChangesOnly && outcome == Outcome::Succeeded))
            && let Some(line) = describe_change(path.as_os_str(), outcome, old_mode, new_mode)
        {
            job.say(&line);
        }

        if !job.settings.recursive || !is_dir {
            return;
        }
        descend(job, path);
    }

    /// Read one directory and visit every name in it.
    fn descend(job: &mut Job, dir: &Path) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) => {
                job.fail(&format!(
                    "cannot read directory {}: {}",
                    quoteaf_os(dir),
                    strerror(&e)
                ));
                return;
            }
        };
        for entry in entries {
            match entry {
                Ok(entry) => visit(job, &entry.path(), false),
                Err(e) => job.fail(&format!(
                    "cannot read directory {}: {}",
                    quoteaf_os(dir),
                    strerror(&e)
                )),
            }
        }
    }

    /// gnulib's `ROOT_DEV_INO_WARN`, which is two messages rather than one so
    /// that the second names the escape hatch.
    fn warn_about_root(job: &mut Job, path: &Path) {
        let named: &OsStr = path.as_os_str();
        if named == OsStr::new("/") {
            job.fail(&format!(
                "it is dangerous to operate recursively on {}",
                quoteaf_os(named)
            ));
        } else {
            job.fail(&format!(
                "it is dangerous to operate recursively on {} (same as {})",
                quoteaf_os(named),
                quoteaf_os("/")
            ));
        }
        // Deliberately routed through `fail` as well, so `-f` silences both
        // halves together; a lone "use --no-preserve-root" would be baffling.
        job.fail("use --no-preserve-root to override this failsafe");
    }
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`coreutils::stdfd::close_stderr`].
#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    // Applying a mode is `imp`'s job, not this module's, so the top level does
    // not import it; the tests do it directly to check what `imp` would compute.
    use modechange::adjust;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn settings(args: &[&str]) -> Settings {
        match parse_args(&argv(args)).unwrap() {
            Request::Run(settings) => *settings,
            other => panic!("expected a run, got {other:?}"),
        }
    }

    /// The mode a command line resolves to, applied to `start`.
    fn resolves_to(args: &[&str], start: u32, dir: bool, umask: u32) -> u32 {
        match settings(args).source {
            Source::Spec(changes) => adjust(start, dir, umask, &changes).mode,
            Source::Reference(_) => panic!("expected a compiled mode"),
        }
    }

    fn err(args: &[&str]) -> String {
        parse_args(&argv(args)).unwrap_err().message()
    }

    // ------------------------------------------------------------- operands ---

    #[test]
    fn the_first_operand_is_the_mode_and_the_rest_are_files() {
        let s = settings(&["644", "a", "b", "c"]);
        assert_eq!(s.files, argv(&["a", "b", "c"]));
        assert_eq!(resolves_to(&["644", "a"], 0o777, false, 0), 0o644);
    }

    #[test]
    fn recursion_is_an_option_wherever_it_appears() {
        assert!(settings(&["-R", "755", "d"]).recursive);
        assert!(settings(&["--recursive", "755", "d"]).recursive);
        assert!(settings(&["755", "-R", "d"]).recursive);
        assert!(!settings(&["755", "d"]).recursive);
    }

    #[test]
    fn double_dash_ends_options() {
        // Without `--` there is no way to name a file `-R`.
        let s = settings(&["--", "644", "-R"]);
        assert!(!s.recursive);
        assert_eq!(s.files, argv(&["-R"]));
    }

    // --------------------------------------------------- a mode spelt as flags ---

    /// The rule the old parser guessed at: `-r` is a mode, `-R` is recursion.
    #[test]
    fn lowercase_r_is_a_mode_and_uppercase_r_is_recursion() {
        assert!(!settings(&["-r", "f"]).recursive);
        assert_eq!(resolves_to(&["-r", "f"], 0o777, false, 0), 0o333);
        assert!(settings(&["-R", "755", "d"]).recursive);
    }

    #[test]
    fn the_other_letters_behave_the_same_way() {
        for (spec, want) in [
            ("-r", 0o333),
            ("-w", 0o555),
            ("-x", 0o666),
            ("-rw", 0o111),
            ("-rwx", 0o000),
        ] {
            assert_eq!(
                resolves_to(&[spec, "f"], 0o777, false, 0),
                want,
                "{spec} from 0777"
            );
        }
    }

    /// Several such words are joined with commas, exactly as upstream does when
    /// it concatenates `argv[optind - 1]`.
    #[test]
    fn several_flag_words_make_one_comma_separated_mode() {
        // `-s` then `-w`, which is `-s,-w`: strip setuid and setgid, then write.
        assert_eq!(resolves_to(&["-s", "-w", "f"], 0o6777, false, 0), 0o555);
    }

    /// The reason this file asks the parser which *word* an option came from.
    /// Reconstructing the option would give `-w` and silently recurse.
    #[test]
    fn a_mode_letter_bundled_behind_a_real_option_is_an_invalid_mode() {
        assert_eq!(
            err(&["-Rw", "d"]),
            "invalid mode: \u{2018}-Rw\u{2019}\nTry 'chmod --help' for more information."
        );
        assert_eq!(
            err(&["-vw", "d"]),
            "invalid mode: \u{2018}-vw\u{2019}\nTry 'chmod --help' for more information."
        );
    }

    /// A mode given as flags sets the surprise check; one given as an operand
    /// does not. `chmod -w f` can do less than it says; `chmod 644 f` cannot.
    #[test]
    fn only_a_mode_spelt_as_flags_is_checked_for_surprises() {
        assert!(settings(&["-w", "f"]).diagnose_surprises);
        assert!(!settings(&["644", "f"]).diagnose_surprises);
        assert!(!settings(&["u+w", "f"]).diagnose_surprises);
    }

    // ------------------------------------------------------------ the umask ---

    /// A clause with no `who` is masked by the umask — the rule none of the
    /// four hand-written parsers had. Here it is reaching `chmod` end to end.
    #[test]
    fn a_clause_with_no_who_is_masked_by_the_umask() {
        assert_eq!(resolves_to(&["+w", "f"], 0o444, false, 0o022), 0o644);
        assert_eq!(resolves_to(&["+w", "f"], 0o444, false, 0o000), 0o666);
        assert_eq!(resolves_to(&["a+w", "f"], 0o444, false, 0o022), 0o666);
    }

    /// And an octal is never masked, however the umask is set.
    #[test]
    fn an_octal_mode_ignores_the_umask() {
        assert_eq!(resolves_to(&["666", "f"], 0o000, false, 0o077), 0o666);
    }

    // ------------------------------------------------------------ reference ---

    #[test]
    fn reference_is_kept_unresolved_for_the_caller_to_stat() {
        let s = settings(&["--reference=rfile", "a", "b"]);
        assert_eq!(s.source, Source::Reference(OsString::from("rfile")));
        assert_eq!(s.files, argv(&["a", "b"]));
    }

    /// With `--reference` the mode operand is not consumed, so what looks like
    /// a mode is a file. Measured: GNU reports `cannot access '644'`.
    #[test]
    fn reference_does_not_eat_the_first_operand() {
        let s = settings(&["--reference=rfile", "644", "g"]);
        assert_eq!(s.files, argv(&["644", "g"]));
    }

    #[test]
    fn a_mode_in_flags_cannot_be_combined_with_reference() {
        assert_eq!(
            err(&["--reference=rfile", "-w", "g"]),
            "cannot combine mode and --reference options\n\
             Try 'chmod --help' for more information."
        );
    }

    // ---------------------------------------------------------- diagnostics ---

    #[test]
    fn no_arguments_at_all_is_missing_operand() {
        assert_eq!(
            err(&[]),
            "missing operand\nTry 'chmod --help' for more information."
        );
    }

    /// The two wordings, and what picks between them: a mode that came from an
    /// operand can be named, one that came from flags cannot.
    #[test]
    fn a_mode_with_no_files_names_the_mode_only_when_it_was_an_operand() {
        assert_eq!(
            err(&["644"]),
            "missing operand after \u{2018}644\u{2019}\n\
             Try 'chmod --help' for more information."
        );
        assert_eq!(
            err(&["-w"]),
            "missing operand\nTry 'chmod --help' for more information."
        );
        assert_eq!(
            err(&["--reference=rfile"]),
            "missing operand\nTry 'chmod --help' for more information."
        );
    }

    /// The operand count is checked before the mode is compiled, so this is not
    /// `invalid mode`. Upstream's order, and observable.
    #[test]
    fn a_nonsense_mode_with_no_files_is_still_missing_operand() {
        assert_eq!(
            err(&["xyz"]),
            "missing operand after \u{2018}xyz\u{2019}\n\
             Try 'chmod --help' for more information."
        );
    }

    #[test]
    fn a_mode_that_is_not_one_is_refused_with_the_string_quoted() {
        assert_eq!(
            err(&["999", "f"]),
            "invalid mode: \u{2018}999\u{2019}\nTry 'chmod --help' for more information."
        );
        assert_eq!(
            err(&["u+rZZZ", "f"]),
            "invalid mode: \u{2018}u+rZZZ\u{2019}\nTry 'chmod --help' for more information."
        );
    }

    #[test]
    fn an_unknown_option_is_refused_rather_than_taken_as_a_mode() {
        assert_eq!(
            err(&["-z", "f"]),
            "invalid option -- 'z'\nTry 'chmod --help' for more information."
        );
        assert_eq!(
            err(&["--nope", "f"]),
            "unrecognized option '--nope'\nTry 'chmod --help' for more information."
        );
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&argv(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&argv(&["--version"])).unwrap(), Request::Version);
    }

    /// `--silent` and `--quiet` are one option, so `--s` is unambiguous.
    #[test]
    fn silent_and_quiet_are_the_same_option() {
        assert!(settings(&["--silent", "644", "f"]).force_silent);
        assert!(settings(&["--quiet", "644", "f"]).force_silent);
        assert!(settings(&["--s", "644", "f"]).force_silent);
        assert!(settings(&["-f", "644", "f"]).force_silent);
    }

    #[test]
    fn verbosity_is_the_last_of_c_and_v_that_was_given() {
        assert_eq!(settings(&["644", "f"]).verbosity, Verbosity::Off);
        assert_eq!(
            settings(&["-c", "644", "f"]).verbosity,
            Verbosity::ChangesOnly
        );
        assert_eq!(settings(&["-v", "644", "f"]).verbosity, Verbosity::High);
        assert_eq!(
            settings(&["-v", "-c", "644", "f"]).verbosity,
            Verbosity::ChangesOnly
        );
        assert_eq!(
            settings(&["-c", "-v", "644", "f"]).verbosity,
            Verbosity::High
        );
    }

    #[test]
    fn preserve_root_is_off_by_default_and_the_last_word_wins() {
        assert!(!settings(&["-R", "755", "d"]).preserve_root);
        assert!(settings(&["--preserve-root", "-R", "755", "d"]).preserve_root);
        assert!(
            !settings(&["--preserve-root", "--no-preserve-root", "-R", "755", "d"]).preserve_root
        );
    }

    // ---------------------------------------------------------- descriptions ---

    #[test]
    fn a_change_is_described_in_octal_and_in_letters() {
        assert_eq!(
            describe_change(OsStr::new("f"), Outcome::Succeeded, 0o644, 0o755).unwrap(),
            "mode of 'f' changed from 0644 (rw-r--r--) to 0755 (rwxr-xr-x)"
        );
        assert_eq!(
            describe_change(OsStr::new("f"), Outcome::NoChangeRequested, 0o755, 0o755).unwrap(),
            "mode of 'f' retained as 0755 (rwxr-xr-x)"
        );
        assert_eq!(
            describe_change(OsStr::new("f"), Outcome::Failed, 0o644, 0o755).unwrap(),
            "failed to change mode of 'f' from 0644 (rw-r--r--) to 0755 (rwxr-xr-x)"
        );
    }

    /// The case `-v` exists for: a setuid bit that the octal alone would hide.
    #[test]
    fn a_setuid_bit_shows_as_an_s_in_the_execute_column() {
        assert_eq!(
            describe_change(OsStr::new("f"), Outcome::Succeeded, 0o644, 0o4644).unwrap(),
            "mode of 'f' changed from 0644 (rw-r--r--) to 4644 (rwSr--r--)"
        );
    }

    #[test]
    fn a_link_and_an_unreadable_file_have_their_own_wordings() {
        assert_eq!(
            describe_change(OsStr::new("l"), Outcome::NotApplied, 0, 0).unwrap(),
            "neither symbolic link 'l' nor referent has been changed"
        );
        assert_eq!(
            describe_change(OsStr::new("f"), Outcome::NoStat, 0, 0).unwrap(),
            "'f' could not be accessed"
        );
    }

    // ------------------------------------------------------------- surprises ---

    /// `chmod -w f` under `umask 022` from `0666`: only the owner's write bit
    /// goes, so group and other keep theirs and the naive reading is betrayed.
    #[test]
    fn a_umask_that_held_bits_back_is_reported() {
        let changes = compile(b"-w").unwrap();
        let masked = adjust(0o666, false, 0o022, &changes).mode;
        let naive = adjust(0o666, false, 0, &changes).mode;
        assert_eq!(masked, 0o466);
        assert_eq!(naive, 0o444);
        assert_eq!(
            surprise(masked, naive).unwrap(),
            "new permissions are r--rw-rw-, not r--r--r--"
        );
    }

    /// And when the umask changed nothing, nothing is said. `0644` has no group
    /// or other write bit to keep, so `-w` under `umask 022` lands where the
    /// naive reading expected.
    #[test]
    fn no_surprise_when_the_umask_made_no_difference() {
        let changes = compile(b"-w").unwrap();
        let masked = adjust(0o644, false, 0o022, &changes).mode;
        let naive = adjust(0o644, false, 0, &changes).mode;
        assert_eq!(masked, naive);
        assert_eq!(surprise(masked, naive), None);
    }

    // ------------------------------------------------------- mode letter set ---

    /// The letters that are really mode text, against the ones that are not.
    /// `8` and `9` are digits that no octal contains, and `R`, `c`, `f` and `v`
    /// are the real options.
    #[test]
    fn the_mode_letters_are_exactly_the_grammars_alphabet() {
        for yes in b"rwxXstugoa,+=01234567" {
            assert!(is_mode_letter(*yes), "{} should be mode text", *yes as char);
        }
        for no in b"89Rcfvz" {
            assert!(!is_mode_letter(*no), "{} should be an option", *no as char);
        }
    }
}
