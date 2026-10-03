//! `install` — copy files and set their attributes.
//!
//! ```text
//! install [OPTION]... [-T] SOURCE DEST
//! install [OPTION]... SOURCE... DIRECTORY
//! install [OPTION]... -t DIRECTORY SOURCE...
//! install [OPTION]... -d DIRECTORY...
//! ```
//!
//! A port of GNU coreutils 9.4's `install.c`, onto the two pieces of the
//! library it is built from upstream too: the copy engine `cp` and `mv`
//! share ([`coreutils::copy`], upstream's `copy.c`) and the directory maker
//! ([`coreutils::mkdirp`], gnulib's `mkdir-p.c` and `mkancesdirs.c`).
//!
//! It replaces `userspace/install`, a separate crate that had reimplemented
//! the program beside a library that already held most of it. Measured with
//! `scripts/install-diff.sh` before it was deleted, it agreed with GNU on 22
//! of 173 cases; see `known-issues.md` ->
//! `TD-B-INSTALL-REIMPLEMENTS-A-BACKUP-POLICY-COREUTILS-ALREADY-HAS`, and
//! design decision §1005 (coreutils is the one home).
//!
//! # What `install` does that `cp` does not
//!
//! * **The process's umask is cleared first** (`umask (0)`), so every mode it
//!   creates a file or directory with is the mode it asked for: `0755`
//!   ancestors under any umask, and a `-m 0640` that means `0640`.
//! * **Every copy is created at `0600`** -- [`copy::Opts::set_mode`] -- so
//!   that the strip program can write to it, and given its final owner and
//!   mode afterwards, owner first: a `chown` can clear the set-user-ID bit, so
//!   a mode set before it would not survive it.
//! * **The destination is always unlinked before it is written**
//!   (`unlink_dest_before_opening`), never written through: replacing a
//!   running binary must not change the one that is running, and a
//!   destination that is a symbolic link is replaced rather than followed.
//! * **A source is always followed**, and a directory source is refused
//!   (`omitting directory`), with no `-r`.
//!
//! # SELinux
//!
//! SlateOS has no SELinux, and neither does the reference these were measured
//! against: `-Z` is accepted and does nothing, `--context=CTX` and
//! `--preserve-context` warn that the kernel lacks it, as upstream does on such
//! a kernel.
//!
//! # `--debug`
//!
//! Refused, as `cp` refuses it: it reports how the copy engine copied
//! (`copy offload`, `reflink`, `sparse detection`), and this engine does not
//! record which of those it used.

use coreutils::backup::{self, Backup, BackupType};
use coreutils::copy::{
    self, Deref, Placed, place_entity, remove_destination_first, stat_destination,
};
use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::fileid::{Copied, FileId, file_id, nlink, same_entry, same_inode};
use coreutils::fsattr::{self, Link, On, Owner};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::mkdirp::{self, Target};
use coreutils::overwrite::Interactive;
use coreutils::pathname::{file_name_concat, last_component};
use coreutils::quote::{os_bytes, os_from_bytes, quote_os, quoteaf_os};
use coreutils::stdfd::{self, Stream};
use coreutils::xnum;
use coreutils::yesno::Answers;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// `install`'s usage status is 1.
const INSTALL: Program = Program::new("install", 1);

/// `DEFAULT_MODE`: `rwxr-xr-x`, for files and directories alike, and for every
/// ancestor `-D` and `-d` make.
const DEFAULT_MODE: u32 = 0o755;

/// The mode every copy is created with and keeps until its final one is set
/// (`install.c:289`), writable by its owner so the strip program can work.
const WORKING_MODE: u32 = 0o600;

/// The set-user-ID, set-group-ID and sticky bits, and the file-type bits,
/// which `extra_mode` asks about.
const S_IRWXUGO: u32 = 0o777;
const S_IFMT: u32 = 0o170_000;

/// `bcCsDdg:m:o:pt:TvS:Z`, upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "bcCsDdg:m:o:pt:TvS:Z";

/// Upstream's `long_options[]`, **in its declaration order**, which is
/// observable: an ambiguous prefix lists its candidates in table order.
/// `context` is `GETOPT_SELINUX_CONTEXT_OPTION_DECL`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("backup", Takes::Optional),
    ("compare", Takes::Nothing),
    ("context", Takes::Optional),
    ("debug", Takes::Nothing),
    ("directory", Takes::Nothing),
    ("group", Takes::Required),
    ("mode", Takes::Required),
    ("no-target-directory", Takes::Nothing),
    ("owner", Takes::Required),
    ("preserve-timestamps", Takes::Nothing),
    ("preserve-context", Takes::Nothing),
    ("strip", Takes::Nothing),
    ("strip-program", Takes::Required),
    ("suffix", Takes::Required),
    ("target-directory", Takes::Required),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// What the command line asked for, before anything was looked up.
#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Flags {
    /// `-b`, `--backup`, or `-S`: whether backups are made at all.
    make_backups: bool,
    /// `--backup=CONTROL`'s word.
    version_control: Option<OsString>,
    /// `-S`/`--suffix`.
    backup_suffix: Option<OsString>,
    /// `-C`: copy only if the content, owner or mode would change.
    compare: bool,
    /// `-s`.
    strip: bool,
    /// `--strip-program`.
    strip_program: Option<OsString>,
    /// `-d`: the operands are directories to make.
    dir_arg: bool,
    /// `-D`: make the destination's missing ancestors first.
    mkdir_and_install: bool,
    /// `-v`.
    verbose: bool,
    /// `-g`.
    group_name: Option<OsString>,
    /// `-m`.
    mode: Option<OsString>,
    /// `-o`.
    owner_name: Option<OsString>,
    /// `-p`.
    preserve_timestamps: bool,
    /// `-t`.
    target_directory: Option<OsString>,
    /// `-T`.
    no_target_directory: bool,
}

/// What the command line asks for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// Boxed: the flags dwarf the other two variants, and more so on a
    /// Windows host, whose `OsString` is larger.
    Run(Box<Flags>, Vec<OsString>),
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut warnings = Vec::new();
    let parsed = parse_args(&args, &mut warnings);
    // Printed in the order they arose, before whatever ended the parse:
    // upstream prints each from inside its option loop.
    for w in &warnings {
        diag!("install: {w}");
    }
    match parsed {
        Ok(Request::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(Request::Version) => {
            println!("install (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, operands)) => {
            // Before anything is created: `install.c:808`.
            let _ = coreutils::umask::clear();
            let mut out = Stream::stdout();
            let mut err = Stream::stderr();
            let earned = run(&flags, &operands, &mut out, &mut err);
            stdfd::close_stdout("install", out, earned)
        }
        Err(e) => {
            diag!("install: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: install [OPTION]... [-T] SOURCE DEST
  or:  install [OPTION]... SOURCE... DIRECTORY
  or:  install [OPTION]... -t DIRECTORY SOURCE...
  or:  install [OPTION]... -d DIRECTORY...

In the first three forms, copy SOURCE to DEST or multiple SOURCE(s) to
the existing DIRECTORY, while setting permission modes and owner/group.
In the 4th form, create all components of the given DIRECTORY(ies).

Mandatory arguments to long options are mandatory for short options too.
      --backup[=CONTROL]  make a backup of each existing destination file
  -b                  like --backup but does not accept an argument
  -c                  (ignored)
  -C, --compare       compare content of source and destination files, and
                        if no change to content, ownership, and permissions,
                        do not modify the destination at all
  -d, --directory     treat all arguments as directory names; create all
                        components of the specified directories
  -D                  create all leading components of DEST except the last,
                        or all components of --target-directory,
                        then copy SOURCE to DEST
  -g, --group=GROUP   set group ownership, instead of process' current group
  -m, --mode=MODE     set permission mode (as in chmod), instead of rwxr-xr-x
  -o, --owner=OWNER   set ownership (super-user only)
  -p, --preserve-timestamps   apply access/modification times of SOURCE files
                        to corresponding destination files
  -s, --strip         strip symbol tables
      --strip-program=PROGRAM  program used to strip binaries
  -S, --suffix=SUFFIX  override the usual backup suffix
  -t, --target-directory=DIRECTORY  copy all SOURCE arguments into DIRECTORY
  -T, --no-target-directory  treat DEST as a normal file
  -v, --verbose       print the name of each created file or directory
      --preserve-context  preserve SELinux security context (SlateOS has
                            none: warns and does nothing)
  -Z                      set SELinux security context of destination
                            file and each created directory to default type
                            (accepted, and does nothing)
      --context[=CTX]     like -Z, or if CTX is specified then set the
                            SELinux or SMACK security context to CTX
                            (warns and does nothing)
      --help        display this help and exit
      --version     output version information and exit

The backup suffix is '~', unless set with --suffix or SIMPLE_BACKUP_SUFFIX.
The version control method may be selected via the --backup option or through
the VERSION_CONTROL environment variable.  Here are the values:

  none, off       never make backups (even if --backup is given)
  numbered, t     make numbered backups
  existing, nil   numbered if numbered backups exist, simple otherwise
  simple, never   always make simple backups
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse argv. Warnings upstream prints from inside its option loop are
/// appended to `warnings`, in order, for the caller to print before acting on
/// the result -- so a warning still precedes the error that ends the parse.
///
/// # Errors
///
/// An option getopt refuses, a second `-t`, and `--debug`, which is not
/// implemented.
fn parse_args(args: &[OsString], warnings: &mut Vec<String>) -> Result<Request, getopt::Error> {
    let mut flags = Flags::default();
    let mut operands = Vec::new();
    for item in INSTALL.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            Opt::Operand(name) => operands.push(name.clone()),
            Opt::Short(b'b', _) => flags.make_backups = true,
            Opt::Long("backup", value) => {
                flags.make_backups = true;
                if value.is_some() {
                    flags.version_control = value;
                }
            }
            // "(ignored)", as upstream's own help says.
            Opt::Short(b'c', _) => {}
            Opt::Short(b'C', _) | Opt::Long("compare", _) => flags.compare = true,
            Opt::Short(b's', _) | Opt::Long("strip", _) => flags.strip = true,
            Opt::Long("strip-program", value) => flags.strip_program = value,
            Opt::Short(b'd', _) | Opt::Long("directory", _) => flags.dir_arg = true,
            Opt::Short(b'D', _) => flags.mkdir_and_install = true,
            Opt::Short(b'v', _) | Opt::Long("verbose", _) => flags.verbose = true,
            Opt::Short(b'g', value) | Opt::Long("group", value) => flags.group_name = value,
            Opt::Short(b'm', value) | Opt::Long("mode", value) => flags.mode = value,
            Opt::Short(b'o', value) | Opt::Long("owner", value) => flags.owner_name = value,
            Opt::Short(b'p', _) | Opt::Long("preserve-timestamps", _) => {
                flags.preserve_timestamps = true;
            }
            Opt::Short(b'S', value) | Opt::Long("suffix", value) => {
                flags.make_backups = true;
                flags.backup_suffix = value;
            }
            Opt::Short(b't', value) | Opt::Long("target-directory", value) => {
                // `error (EXIT_FAILURE, …)`, from inside the loop: no referral,
                // and nothing after it on the command line is read.
                if flags.target_directory.is_some() {
                    return Err(INSTALL.usage("multiple target directories specified".into()));
                }
                flags.target_directory = value;
            }
            Opt::Short(b'T', _) | Opt::Long("no-target-directory", _) => {
                flags.no_target_directory = true;
            }
            Opt::Long("preserve-context", _) => warnings.push(
                "WARNING: ignoring --preserve-context; this kernel is not SELinux-enabled".into(),
            ),
            // `-Z` alone asks for the default context, which on a kernel
            // without SELinux upstream skips in silence; a context *named*
            // is warned about.
            Opt::Short(b'Z', _) => {}
            Opt::Long("context", value) => {
                if value.is_some() {
                    warnings.push(
                        "warning: ignoring --context; it requires an SELinux-enabled kernel".into(),
                    );
                }
            }
            Opt::Long("debug", _) => {
                return Err(INSTALL.usage_referring(
                    "option '--debug' is not implemented by this install".into(),
                ));
            }
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            // Unreachable: every option in the two tables is handled above.
            Opt::Short(other, _) => return Err(INSTALL.invalid_option(other)),
            Opt::Long(other, _) => {
                return Err(INSTALL.unrecognized_option(format!("--{other}").as_bytes()));
            }
        }
    }
    Ok(Request::Run(Box::new(flags), operands))
}

// ------------------------------------------------------------------ setup ---

/// Everything settled before the first file is touched: the modes, the
/// owner, the backup policy, and where the files are going.
struct Job<'a, O: Write, E: Write> {
    flags: &'a Flags,
    /// The mode a file gets, and a directory's with the bits it enforces.
    mode: u32,
    dir_mode: u32,
    dir_mode_bits: u32,
    /// `owner_id` and `group_id`; `None` leaves that half alone.
    owner: Owner,
    backup: Backup,
    /// `dest_info`: the destinations this command has written, which another
    /// operand may not then overwrite. Kept only when several sources go into
    /// one directory, as upstream keeps it (`dest_info_init`).
    dest_info: Option<HashSet<(PathBuf, FileId)>>,
    /// The record of which inode went where, which the engine wants for
    /// `--preserve=links` and which `install` never asks it to use.
    copied: Copied,
    out: &'a mut O,
    err: &'a mut E,
}

/// No question is ever asked: `install`'s `interactive` is `I_UNSPECIFIED`
/// and it is not a move, so the engine's prompt is unreachable.
struct NeverAsked;

impl Answers for NeverAsked {
    fn line(&mut self) -> Option<Vec<u8>> {
        None
    }
}

/// The whole run after the command line parsed, with `main`'s exit status.
fn run<O: Write, E: Write>(
    flags: &Flags,
    operands: &[OsString],
    out: &mut O,
    err: &mut E,
) -> ExitCode {
    match prepare(flags, operands, out, err) {
        Ok(Ready::Directories(job)) => finish(make_directories(job, operands)),
        Ok(Ready::Files {
            mut job,
            target_dir,
            sources,
        }) => finish(install_files(&mut job, target_dir, sources)),
        Err(status) => status,
    }
}

fn finish(ok: bool) -> ExitCode {
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// What [`prepare`] settled: a job and what it is to do.
enum Ready<'a, O: Write, E: Write> {
    /// `-d`: make each operand.
    Directories(Job<'a, O, E>),
    /// Copy `sources`, into `target_dir` when there is one (and it is
    /// whether it was usable when looked at), else onto the last operand.
    Files {
        job: Job<'a, O, E>,
        target_dir: Option<(PathBuf, bool)>,
        sources: &'a [OsString],
    },
}

/// Upstream's `main` from the end of the option loop to the first copy, every
/// check in its order. A failure here has been printed already.
fn prepare<'a, O: Write, E: Write>(
    flags: &'a Flags,
    operands: &'a [OsString],
    out: &'a mut O,
    err: &'a mut E,
) -> Result<Ready<'a, O, E>, ExitCode> {
    let fail = |err: &mut E, message: String| {
        let _ = writeln!(err, "install: {message}");
        ExitCode::from(1)
    };
    let usage = |err: &mut E, message: String| {
        let _ = writeln!(
            err,
            "install: {message}\nTry 'install --help' for more information."
        );
        ExitCode::from(1)
    };

    if flags.dir_arg && flags.strip {
        return Err(fail(
            err,
            "the strip option may not be used when installing a directory".into(),
        ));
    }
    if flags.dir_arg && flags.target_directory.is_some() {
        return Err(fail(
            err,
            "target directory not allowed when installing a directory".into(),
        ));
    }

    let backup = if flags.make_backups {
        match backup::control(INSTALL, flags.version_control.as_deref()) {
            Ok(kind) => Backup::new(kind, backup::suffix(flags.backup_suffix.as_deref())),
            Err(e) => {
                let _ = writeln!(err, "install: {e}");
                return Err(ExitCode::from(u8::try_from(e.status).unwrap_or(1)));
            }
        }
    } else {
        Backup::disabled()
    };

    // `n_files <= ! (dir_arg || target_directory)`: one operand is enough
    // when it need not also be the destination.
    let least = usize::from(!(flags.dir_arg || flags.target_directory.is_some()));
    if operands.len() <= least {
        let message = match operands.first() {
            None => "missing file operand".to_string(),
            Some(first) => format!(
                "missing destination file operand after {}",
                quoteaf_os(first)
            ),
        };
        return Err(usage(err, message));
    }

    let mut sources: &[OsString] = operands;
    let mut target_dir: Option<(PathBuf, bool)> = None;
    if flags.no_target_directory {
        if flags.target_directory.is_some() {
            return Err(fail(
                err,
                "cannot combine --target-directory (-t) and --no-target-directory (-T)".into(),
            ));
        }
        if let Some(extra) = operands.get(2) {
            return Err(usage(err, format!("extra operand {}", quoteaf_os(extra))));
        }
    } else if let Some(dir) = &flags.target_directory {
        // A `-t` directory that does not exist is allowed under `-D`, which
        // makes it.
        match target_directory_operand(Path::new(dir)) {
            Ok(()) => target_dir = Some((PathBuf::from(dir), true)),
            Err(e) if flags.mkdir_and_install && e.kind() == io::ErrorKind::NotFound => {
                target_dir = Some((PathBuf::from(dir), false));
            }
            Err(e) => {
                return Err(fail(
                    err,
                    format!("failed to access {}: {}", quoteaf_os(dir), strerror(&e)),
                ));
            }
        }
    } else if !flags.dir_arg
        && let Some((last, rest)) = operands.split_last()
    {
        match target_directory_operand(Path::new(last)) {
            Ok(()) => {
                target_dir = Some((PathBuf::from(last), true));
                sources = rest;
            }
            Err(e) if operands.len() > 2 => {
                return Err(fail(
                    err,
                    format!("target {}: {}", quoteaf_os(last), strerror(&e)),
                ));
            }
            Err(_) => {}
        }
    }

    let (mut mode, mut dir_mode, mut dir_mode_bits) = (DEFAULT_MODE, DEFAULT_MODE, 0o7777);
    if let Some(spec) = &flags.mode {
        let Some(changes) = modechange::compile(&os_bytes(spec)) else {
            return Err(fail(err, format!("invalid mode {}", quote_os(spec))));
        };
        // From nothing, with no umask: the string describes the finished
        // mode rather than editing one.
        mode = modechange::adjust(0, false, 0, &changes).mode;
        let for_dirs = modechange::adjust(0, true, 0, &changes);
        dir_mode = for_dirs.mode;
        dir_mode_bits = for_dirs.mode_bits;
    }

    if flags.strip_program.is_some() && !flags.strip {
        let _ = writeln!(
            err,
            "install: WARNING: ignoring --strip-program option as -s option was not specified"
        );
    }
    if flags.compare && flags.preserve_timestamps {
        return Err(usage(
            err,
            "options --compare (-C) and --preserve-timestamps are mutually exclusive".into(),
        ));
    }
    if flags.compare && flags.strip {
        return Err(usage(
            err,
            "options --compare (-C) and --strip are mutually exclusive".into(),
        ));
    }
    if flags.compare && extra_mode(mode) {
        let _ = writeln!(
            err,
            "install: the --compare (-C) option is ignored when you specify a mode with \
             non-permission bits"
        );
    }

    let owner = match get_ids(flags) {
        Ok(owner) => owner,
        Err(message) => return Err(fail(err, message)),
    };

    let job = Job {
        flags,
        mode,
        dir_mode,
        dir_mode_bits,
        owner,
        backup,
        // `dest_info_init` runs only on the path with a target directory.
        dest_info: target_dir.is_some().then(HashSet::new),
        copied: Copied::default(),
        out,
        err,
    };
    if flags.dir_arg {
        Ok(Ready::Directories(job))
    } else {
        Ok(Ready::Files {
            job,
            target_dir,
            sources,
        })
    }
}

/// gnulib's `target_directory_operand`, as `mv` asks it: `Ok` when `path`
/// names a directory (through a symlink, too), else why not -- `ENOTDIR`
/// for something that is there and is not one.
fn target_directory_operand(path: &Path) -> io::Result<()> {
    if fs::metadata(path)?.is_dir() {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotADirectory))
    }
}

/// `extra_mode`: whether a mode has bits beyond the permissions and the file
/// type -- set-user-ID, set-group-ID or sticky.
fn extra_mode(mode: u32) -> bool {
    mode & !(S_IRWXUGO | S_IFMT) != 0
}

/// `get_ids`: the owner and group asked for, as numbers. A name is looked up
/// first, and only a name nobody has is read as a number -- in any base
/// `strtoumax` reads, `0x` and leading `0` included, which is `install`'s own
/// rule and not `chown`'s.
fn get_ids(flags: &Flags) -> Result<Owner, String> {
    if flags.owner_name.is_none() && flags.group_name.is_none() {
        return Ok(Owner::default());
    }
    let db = pwdb::Db::load();
    let mut owner = Owner::default();
    if let Some(name) = &flags.owner_name {
        let bytes = os_bytes(name);
        owner.uid = Some(match db.user_by_name(&bytes) {
            Some(user) => user.uid,
            None => {
                numeric_id(&bytes).ok_or_else(|| format!("invalid user {}", quoteaf_os(name)))?
            }
        });
    }
    if let Some(name) = &flags.group_name {
        let bytes = os_bytes(name);
        owner.gid = Some(match db.group_by_name(&bytes) {
            Some(group) => group.gid,
            None => {
                numeric_id(&bytes).ok_or_else(|| format!("invalid group {}", quoteaf_os(name)))?
            }
        });
    }
    Ok(owner)
}

/// `xstrtoumax (name, nullptr, 0, &tmp, "")`, then the `UID_T_MAX` bound:
/// the whole string a number in any base, and one that fits an id. `-1`
/// does not: it is the "leave it alone" sentinel, never an id.
fn numeric_id(text: &[u8]) -> Option<u32> {
    let (value, status) = xnum::xstrtoumax_base(text, 0, Some(b""));
    if status != xnum::Status::Ok {
        return None;
    }
    u32::try_from(value).ok().filter(|&id| id != u32::MAX)
}

// ------------------------------------------------------------ directories ---

/// `-d`: each operand made, with its ancestors, its owner and its mode.
fn make_directories<O: Write, E: Write>(job: Job<'_, O, E>, operands: &[OsString]) -> bool {
    let target = Target {
        mode: job.dir_mode,
        mode_bits: job.dir_mode_bits,
        owner: job.owner,
        preserve_existing: false,
    };
    let verbose = job.flags.verbose;
    let mut ok = true;
    for dir in operands {
        let dir = Path::new(dir);
        // Both callbacks announce, in the order the directories are made, so
        // they share one buffer; it reaches standard output before any
        // diagnostic about this operand does.
        let announced = std::cell::RefCell::new(Vec::new());
        let result = mkdirp::make_dir_parents(
            dir,
            Some(&mut |p: &Path| make_ancestor(p, verbose, &mut *announced.borrow_mut())),
            target,
            &mut |p: &Path| announce_mkdir(p, verbose, &mut *announced.borrow_mut()),
        );
        let _ = job.out.write_all(&announced.into_inner());
        if let Err(failure) = result {
            let subject = quote_os(failure.subject(dir));
            let _ = writeln!(job.err, "install: {}", failure.sentence(&subject));
            ok = false;
        }
    }
    ok
}

/// `make_ancestor`: one missing ancestor, at the default mode, announced.
fn make_ancestor(dir: &Path, verbose: bool, out: &mut dyn Write) -> io::Result<()> {
    copy::create_dir_with_mode(dir, DEFAULT_MODE)?;
    announce_mkdir(dir, verbose, out);
    Ok(())
}

/// `announce_mkdir`: under `-v`, on stdout, with the program's name in front
/// as a diagnostic has it -- upstream's `prog_fprintf`.
fn announce_mkdir(dir: &Path, verbose: bool, out: &mut dyn Write) {
    if verbose {
        let _ = writeln!(out, "install: creating directory {}", quoteaf_os(dir));
    }
}

// ------------------------------------------------------------------ files ---

/// The two copy forms: into a directory, or onto the one destination named.
fn install_files<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    target_dir: Option<(PathBuf, bool)>,
    sources: &[OsString],
) -> bool {
    match target_dir {
        None => {
            // Exactly two operands remain here: `prepare` refused fewer, and
            // more without a directory to put them in.
            let (Some(from), Some(to)) = (sources.first(), sources.get(1)) else {
                return false;
            };
            let to = Path::new(to);
            if job.flags.mkdir_and_install {
                mkancesdirs_reporting(job, to) && install_file_in_file(job, Path::new(from), to)
            } else {
                install_file_in_file(job, Path::new(from), to)
            }
        }
        Some((dir, mut usable)) => {
            let mut ok = true;
            for (i, from) in sources.iter().enumerate() {
                if !install_file_in_dir(job, Path::new(from), &dir, i == 0, &mut usable) {
                    ok = false;
                }
            }
            ok
        }
    }
}

/// `install_file_in_dir`: `from` into `to_dir` under its own last component.
/// `usable` is upstream's "the target's descriptor is valid": false only for a
/// `-t` directory that did not exist, which the first source under `-D`
/// makes. A later source finding it still unusable fails in silence, as
/// upstream's does.
fn install_file_in_dir<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    from: &Path,
    to_dir: &Path,
    first: bool,
    usable: &mut bool,
) -> bool {
    let base = last_component(&os_bytes(from.as_os_str())).to_vec();
    let to = PathBuf::from(os_from_bytes(&file_name_concat(
        &os_bytes(to_dir.as_os_str()),
        &base,
    )));
    if !*usable {
        let mkdir_and_install = first && job.flags.mkdir_and_install;
        if !mkdir_and_install || !mkancesdirs_reporting(job, &to) {
            return false;
        }
        if let Err(e) = target_directory_operand(to_dir) {
            let _ = writeln!(
                job.err,
                "install: cannot open {}: {}",
                quoteaf_os(&to),
                strerror(&e)
            );
            return false;
        }
        *usable = true;
    }
    install_file_in_file(job, from, &to)
}

/// `mkancesdirs_safe_wd`: the missing ancestors of `to`, announced, and the
/// failure reported -- with `to`'s whole name, as upstream reports it.
fn mkancesdirs_reporting<O: Write, E: Write>(job: &mut Job<'_, O, E>, to: &Path) -> bool {
    let verbose = job.flags.verbose;
    let out = &mut *job.out;
    match mkdirp::mkancesdirs(to, &mut |p: &Path| make_ancestor(p, verbose, out)) {
        Ok(()) => true,
        // Upstream's walk truncates `to` at the ancestor it stopped at, and the
        // message names what is left.
        Err(stopped) => {
            let _ = writeln!(
                job.err,
                "install: cannot create directory {}: {}",
                quoteaf_os(&stopped.at),
                strerror(&stopped.err)
            );
            false
        }
    }
}

/// `install_file_in_file`: copy `from` onto `to` and give it its attributes.
fn install_file_in_file<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    from: &Path,
    to: &Path,
) -> bool {
    // `-p`'s source times are read before the copy, which is why a source
    // that cannot be read fails here and not in the copy.
    let from_meta = if job.flags.preserve_timestamps {
        match fs::metadata(from) {
            Ok(m) => Some(m),
            Err(e) => {
                let _ = writeln!(
                    job.err,
                    "install: cannot stat {}: {}",
                    quoteaf_os(from),
                    strerror(&e)
                );
                return false;
            }
        }
    } else {
        None
    };
    if !copy_file(job, from, to) {
        return false;
    }
    if job.flags.strip && !strip(job, to) {
        if let Err(e) = fs::remove_file(to) {
            // `error (EXIT_FAILURE, …)`: the run ends here.
            let _ = writeln!(
                job.err,
                "install: cannot unlink {}: {}",
                quoteaf_os(to),
                strerror(&e)
            );
            // Upstream's `exit` flushes standard output on its way out; this
            // one does not, so it is done here.
            let _ = job.out.flush();
            std::process::exit(1);
        }
        return false;
    }
    // The copy carried the times already; they are carried again after a
    // strip, which wrote the file, and for a source that was not a regular
    // file, whose times the copy did not stamp.
    if let Some(meta) = &from_meta
        && (job.flags.strip || !meta.is_file())
        && !change_timestamps(job, meta, to)
    {
        return false;
    }
    change_attributes(job, to)
}

/// `copy_file`: skip the copy under `-C` when nothing would change.
fn copy_file<O: Write, E: Write>(job: &mut Job<'_, O, E>, from: &Path, to: &Path) -> bool {
    if job.flags.compare && !need_copy(job, from, to) {
        return true;
    }
    copy_one(job, from, to)
}

/// `need_copy`: whether `-C` must copy -- anything about the destination
/// that the copy would change, or a mode `-C` cannot vouch for.
fn need_copy<O: Write, E: Write>(job: &Job<'_, O, E>, from: &Path, to: &Path) -> bool {
    if extra_mode(job.mode) {
        return true;
    }
    // `lstat` on both: a symbolic link is not the regular file it may name.
    let (Ok(src), Ok(dst)) = (fs::symlink_metadata(from), fs::symlink_metadata(to)) else {
        return true;
    };
    let (src_mode, dst_mode) = (full_mode(&src), full_mode(&dst));
    if !src.is_file() || !dst.is_file() || extra_mode(src_mode) || extra_mode(dst_mode) {
        return true;
    }
    if src.len() != dst.len() || dst_mode & 0o7777 != job.mode {
        return true;
    }
    let now = fsattr::owner_of(&dst);
    let want_uid = job.owner.uid.or_else(real_uid);
    let want_gid = job.owner.gid.or_else(real_gid);
    if now.uid != want_uid || now.gid != want_gid {
        return true;
    }
    let (Ok(mut a), Ok(mut b)) = (fs::File::open(from), fs::File::open(to)) else {
        return true;
    };
    !have_same_content(&mut a, &mut b)
}

/// `have_same_content`, a block at a time.
fn have_same_content(a: &mut impl Read, b: &mut impl Read) -> bool {
    let mut a_buf = [0u8; 4096];
    let mut b_buf = [0u8; 4096];
    loop {
        let size = full_read(a, &mut a_buf);
        if size == 0 {
            return true;
        }
        if size != full_read(b, &mut b_buf) {
            return false;
        }
        if a_buf.get(..size) != b_buf.get(..size) {
            return false;
        }
    }
}

/// gnulib's `full_read`: as many bytes as fit, short only at the end. A read
/// error ends it as the end would, which `have_same_content` then reads as
/// "different", the safe answer.
fn full_read(src: &mut impl Read, buf: &mut [u8]) -> usize {
    let mut got = 0usize;
    while got < buf.len() {
        match src.read(buf.get_mut(got..).unwrap_or_default()) {
            Ok(0) | Err(_) => break,
            Ok(n) => got = got.saturating_add(n),
        }
    }
    got
}

/// The whole `st_mode`, type bits included, which `extra_mode` reads.
#[cfg(unix)]
fn full_mode(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.mode()
}

#[cfg(not(unix))]
fn full_mode(meta: &fs::Metadata) -> u32 {
    fsattr::permission_bits(meta)
}

/// The real user and group, which a `-C` destination must already be owned
/// by when no `-o` or `-g` was given.
#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)]
fn real_uid() -> Option<u32> {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    // SAFETY: `getuid` takes nothing, touches no memory and cannot fail.
    Some(unsafe { getuid() })
}

#[cfg(unix)]
#[allow(clippy::unnecessary_wraps)]
fn real_gid() -> Option<u32> {
    unsafe extern "C" {
        fn getgid() -> u32;
    }
    // SAFETY: as `getuid`.
    Some(unsafe { getgid() })
}

#[cfg(not(unix))]
fn real_uid() -> Option<u32> {
    None
}

#[cfg(not(unix))]
fn real_gid() -> Option<u32> {
    None
}

/// The engine's options for an `install` copy: `cp_option_init`, line for
/// line.
fn opts<'b>(flags: &'b Flags, backup: &'b Backup) -> copy::Opts<'b> {
    copy::Opts {
        prog: "install",
        recursive: false,
        verbose: flags.verbose,
        dereference: Deref::Always,
        interactive: Interactive::Unspecified,
        unlink_dest_after_failed_open: false,
        unlink_dest_before_opening: true,
        preserve_links: false,
        backup,
        preserve_mode: false,
        preserve_timestamps: flags.preserve_timestamps,
        preserve_ownership: false,
        preserve_xattr: false,
        require_preserve: false,
        require_preserve_xattr: false,
        reduce_diagnostics: false,
        explicit_no_preserve_mode: false,
        set_mode: Some(WORKING_MODE),
        move_mode: false,
        // Cleared at startup, so there is nothing for a mode to lose.
        umask: 0,
    }
}

/// `copy (from, to, …)` for one operand: `copy_internal`'s checks in their
/// order, then the engine's copy. The checks are the ones `install`'s options
/// can reach -- no prompt, no move, no hard link.
fn copy_one<O: Write, E: Write>(job: &mut Job<'_, O, E>, src: &Path, dst: &Path) -> bool {
    let meta = match fs::metadata(src) {
        Ok(m) => m,
        Err(e) => {
            let _ = writeln!(
                job.err,
                "install: cannot stat {}: {}",
                quoteaf_os(src),
                strerror(&e)
            );
            return false;
        }
    };
    if meta.is_dir() {
        let _ = writeln!(job.err, "install: omitting directory {}", quoteaf_os(src));
        return false;
    }

    let mut dest_state = match stat_destination(&meta, dst, opts(job.flags, &job.backup)) {
        Ok(d) => d,
        Err(e) => {
            let _ = writeln!(
                job.err,
                "install: cannot stat {}: {}",
                quoteaf_os(dst),
                strerror(&e)
            );
            return false;
        }
    };

    if let Some(dest_meta) = dest_state.metadata() {
        if !same_file_ok(src, &meta, dst, dest_meta, &job.backup) {
            let _ = writeln!(
                job.err,
                "install: {} and {} are the same file",
                quoteaf_os(src),
                quoteaf_os(dst)
            );
            return false;
        }
        if !dest_meta.is_dir()
            && job.backup.kind() != BackupType::Numbered
            && seen(job, dst, dest_meta)
        {
            let _ = writeln!(
                job.err,
                "install: will not overwrite just-created {} with {}",
                quoteaf_os(dst),
                quoteaf_os(src)
            );
            return false;
        }
        if dest_meta.is_dir() {
            let _ = writeln!(
                job.err,
                "install: cannot overwrite directory {} with non-directory",
                quoteaf_os(dst)
            );
            return false;
        }
    }

    let mut answers = NeverAsked;
    {
        let mut run = copy::Run {
            opts: opts(job.flags, &job.backup),
            err: &mut *job.err,
            out: &mut *job.out,
            copied: &mut job.copied,
            answers: &mut answers,
        };
        if !remove_destination_first(src, dst, &mut dest_state, &mut run) {
            return false;
        }
    }

    // Never write through a symbolic link this command made a moment ago.
    if !job.backup.enabled()
        && let Ok(link_meta) = fs::symlink_metadata(dst)
        && link_meta.file_type().is_symlink()
        && seen(job, dst, &link_meta)
    {
        let _ = writeln!(
            job.err,
            "install: will not copy {} through just-created symlink {}",
            quoteaf_os(src),
            quoteaf_os(dst)
        );
        return false;
    }

    let placed = {
        let mut run = copy::Run {
            opts: opts(job.flags, &job.backup),
            err: &mut *job.err,
            out: &mut *job.out,
            copied: &mut job.copied,
            answers: &mut answers,
        };
        place_entity(src, &meta, dst, &dest_state, true, &mut run)
    };
    if placed == Placed::Copied
        && let Some(info) = job.dest_info.as_mut()
        && let Ok(now) = fs::symlink_metadata(dst)
        && let Some(id) = file_id(dst, &now)
    {
        info.insert((dst.to_path_buf(), id));
    }
    placed.is_ok()
}

/// `seen_file (x->dest_info, …)`: whether this command wrote `dst` and it
/// still holds what it wrote.
fn seen<O: Write, E: Write>(job: &Job<'_, O, E>, dst: &Path, meta: &fs::Metadata) -> bool {
    job.dest_info
        .as_ref()
        .zip(file_id(dst, meta))
        .is_some_and(|(info, id)| info.contains(&(dst.to_path_buf(), id)))
}

/// `same_file_ok` for `install`'s options: the source is always followed, the
/// destination always replaced rather than written through, nothing is
/// hard-linked and nothing moved. `dst_meta` is an `lstat`.
fn same_file_ok(
    src: &Path,
    src_meta: &fs::Metadata,
    dst: &Path,
    dst_meta: &fs::Metadata,
    backup: &Backup,
) -> bool {
    if !same_inode((src, src_meta), (dst, dst_meta)) {
        return true;
    }
    let (Ok(src_link), Ok(dst_link)) = (fs::symlink_metadata(src), fs::symlink_metadata(dst))
    else {
        return true;
    };
    let same_link = same_inode((src, &src_link), (dst, &dst_link));
    let (src_is_link, dst_is_link) = (
        src_link.file_type().is_symlink(),
        dst_link.file_type().is_symlink(),
    );
    // Two links to one file: fine, since the destination link is unlinked
    // rather than written through.
    if src_is_link && dst_is_link {
        return true;
    }
    if backup.enabled() {
        if !same_link {
            // Backing the destination up would leave the source a dangling
            // link, and the copy would then fail confusingly.
            return !src_is_link || dst_is_link;
        }
        return !same_entry(src, dst);
    }
    if dst_is_link {
        return true;
    }
    if same_link && nlink(&dst_link) > 1 && !same_entry(src, dst) {
        return true;
    }
    !src_is_link && !dst_is_link && !same_inode((src, &src_link), (dst, &dst_link))
}

/// `strip`: run the strip program on `name`, and say why when it fails.
///
/// A name starting with `-` is handed over as `./-name`, so that the program
/// cannot read it as an option.
fn strip<O: Write, E: Write>(job: &mut Job<'_, O, E>, name: &Path) -> bool {
    let program = job
        .flags
        .strip_program
        .clone()
        .unwrap_or_else(|| OsString::from("strip"));
    let raw = os_bytes(name.as_os_str());
    let safe = if raw.first() == Some(&b'-') {
        PathBuf::from(os_from_bytes(&file_name_concat(b".", &raw)))
    } else {
        name.to_path_buf()
    };
    // Upstream's child has written its stdout copy of anything buffered by
    // the time it could fail, so flush ours first.
    let _ = job.out.flush();
    match std::process::Command::new(&program).arg(&safe).status() {
        Ok(status) if status.success() => true,
        Ok(_) => {
            let _ = writeln!(job.err, "install: strip process terminated abnormally");
            false
        }
        Err(e) => {
            // Upstream's child says why it could not run the program, and
            // the parent then sees it exit 1.
            let _ = writeln!(
                job.err,
                "install: cannot run {}: {}",
                quoteaf_os(&program),
                strerror(&e)
            );
            let _ = writeln!(job.err, "install: strip process terminated abnormally");
            false
        }
    }
}

/// `change_timestamps`: the source's access and modification times onto `to`.
fn change_timestamps<O: Write, E: Write>(
    job: &mut Job<'_, O, E>,
    src: &fs::Metadata,
    to: &Path,
) -> bool {
    match fsattr::times_of(src).and_then(|t| fsattr::set_times(On::Path(to, Link::Follow), t)) {
        Ok(()) => true,
        Err(e) => {
            let _ = writeln!(
                job.err,
                "install: cannot set timestamps for {}: {}",
                quoteaf_os(to),
                strerror(&e)
            );
            false
        }
    }
}

/// `change_attributes`: the owner, then the mode -- and not the mode when the
/// owner could not be given, so a file that did not reach its owner does not
/// get the mode meant for that owner either.
fn change_attributes<O: Write, E: Write>(job: &mut Job<'_, O, E>, to: &Path) -> bool {
    if !job.owner.is_empty()
        && let Err(e) = fsattr::set_owner(On::Path(to, Link::NoFollow), job.owner)
    {
        let _ = writeln!(
            job.err,
            "install: cannot change ownership of {}: {}",
            quoteaf_os(to),
            strerror(&e)
        );
        return false;
    }
    if let Err(e) = fsattr::set_mode(On::Path(to, Link::Follow), job.mode) {
        let _ = writeln!(
            job.err,
            "install: cannot change permissions of {}: {}",
            quoteaf_os(to),
            strerror(&e)
        );
        return false;
    }
    true
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn flags(words: &[&str]) -> (Flags, Vec<OsString>, Vec<String>) {
        let mut warnings = Vec::new();
        match parse_args(&argv(words), &mut warnings) {
            Ok(Request::Run(f, ops)) => (*f, ops, warnings),
            other => panic!("{words:?} did not parse to a run: {other:?}"),
        }
    }

    fn refusal(words: &[&str]) -> getopt::Error {
        let mut warnings = Vec::new();
        match parse_args(&argv(words), &mut warnings) {
            Err(e) => e,
            Ok(other) => panic!("{words:?} parsed: {other:?}"),
        }
    }

    #[test]
    fn options_and_operands_interleave() {
        let (f, ops, _) = flags(&["a", "-m", "644", "b", "-vD", "-o", "root"]);
        assert_eq!(ops, argv(&["a", "b"]));
        assert_eq!(f.mode, Some("644".into()));
        assert!(f.verbose && f.mkdir_and_install);
        assert_eq!(f.owner_name, Some("root".into()));
    }

    #[test]
    fn suffix_alone_turns_backups_on() {
        let (f, _, _) = flags(&["-S", ".bak", "a", "b"]);
        assert!(f.make_backups);
        assert_eq!(f.backup_suffix, Some(".bak".into()));
        // A later bare `--backup` keeps an earlier word, as upstream's
        // `if (optarg)` does.
        let (f, _, _) = flags(&["--backup=numbered", "--backup", "a", "b"]);
        assert_eq!(f.version_control, Some("numbered".into()));
    }

    #[test]
    fn a_second_target_directory_is_refused_without_a_referral() {
        let e = refusal(&["-t", "d", "-t", "d", "a"]);
        assert_eq!(e.sentence, "multiple target directories specified");
        assert!(e.referral.is_none());
        assert_eq!(e.status, 1);
    }

    #[test]
    fn debug_is_refused_by_name() {
        let e = refusal(&["--debug", "a", "b"]);
        assert!(e.sentence.contains("'--debug' is not implemented"), "{e:?}");
    }

    #[test]
    fn selinux_options_warn_only_where_upstream_does() {
        let (_, _, w) = flags(&["-Z", "a", "b"]);
        assert!(w.is_empty(), "{w:?}");
        let (_, _, w) = flags(&["--context", "a", "b"]);
        assert!(w.is_empty(), "{w:?}");
        let (_, _, w) = flags(&["--context=x_t", "--preserve-context", "a", "b"]);
        assert_eq!(
            w,
            vec![
                "warning: ignoring --context; it requires an SELinux-enabled kernel".to_string(),
                "WARNING: ignoring --preserve-context; this kernel is not SELinux-enabled"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn a_numeric_id_is_read_in_any_base_and_must_fit() {
        assert_eq!(numeric_id(b"0"), Some(0));
        assert_eq!(numeric_id(b"1000"), Some(1000));
        assert_eq!(numeric_id(b"0x10"), Some(16));
        assert_eq!(numeric_id(b"010"), Some(8));
        assert_eq!(numeric_id(b"12a"), None);
        assert_eq!(numeric_id(b""), None);
        assert_eq!(numeric_id(b"-1"), None);
        assert_eq!(numeric_id(b"4294967295"), None, "the leave-it sentinel");
        assert_eq!(numeric_id(b"99999999999999999999"), None);
    }

    #[test]
    fn extra_mode_is_the_bits_beyond_permission_and_type() {
        assert!(!extra_mode(0o755));
        assert!(!extra_mode(0o100_644));
        assert!(extra_mode(0o4755));
        assert!(extra_mode(0o1777));
        assert!(extra_mode(0o2000));
    }

    #[test]
    fn content_is_compared_a_block_at_a_time() {
        let a = vec![7u8; 10_000];
        let mut b = a.clone();
        assert!(have_same_content(&mut a.as_slice(), &mut b.as_slice()));
        b[9_999] = 8;
        assert!(!have_same_content(&mut a.as_slice(), &mut b.as_slice()));
        assert!(!have_same_content(&mut a.as_slice(), &mut &a[..9_999]));
        assert!(have_same_content(&mut &[][..], &mut &[][..]));
    }

    #[test]
    fn a_file_is_the_same_file_as_itself_and_not_as_a_link_to_it() {
        let dir = scratchdir::ScratchDir::new("install_same_file");
        let f = dir.path("f");
        fs::write(&f, b"x").unwrap();
        let meta = fs::metadata(&f).unwrap();
        let lmeta = fs::symlink_metadata(&f).unwrap();
        let none = Backup::disabled();
        assert!(!same_file_ok(&f, &meta, &f, &lmeta, &none));
        let other = dir.path("g");
        fs::write(&other, b"y").unwrap();
        let ometa = fs::symlink_metadata(&other).unwrap();
        assert!(same_file_ok(&f, &meta, &other, &ometa, &none));
    }

    #[cfg(unix)]
    #[test]
    fn a_destination_symlink_to_the_source_is_replaced_not_refused() {
        let dir = scratchdir::ScratchDir::new("install_dest_link");
        let f = dir.path("f");
        fs::write(&f, b"x").unwrap();
        let link = dir.path("l");
        std::os::unix::fs::symlink("f", &link).unwrap();
        let meta = fs::metadata(&f).unwrap();
        let lmeta = fs::symlink_metadata(&link).unwrap();
        assert!(same_file_ok(&f, &meta, &link, &lmeta, &Backup::disabled()));
        // ... and a source that is a link to the destination is the same file.
        let src_meta = fs::metadata(&link).unwrap();
        let dst_meta = fs::symlink_metadata(&f).unwrap();
        assert!(!same_file_ok(
            &link,
            &src_meta,
            &f,
            &dst_meta,
            &Backup::disabled()
        ));
    }
}
