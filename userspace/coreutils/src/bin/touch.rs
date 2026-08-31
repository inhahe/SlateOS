//! touch — create files, and set their access and modification times.
//!
//! # Why this was rewritten
//!
//! The file it replaces was 62 lines long and had four defects, one of which is
//! a bug in this OS that nothing else had noticed.
//!
//! ## 1. It parsed no options at all
//!
//! `parse_args` checked only that argv was non-empty and returned every word as
//! a file name, with a comment saying "touch takes no flags". GNU's takes eight.
//! So `touch -a f` did not change only the access time — it **created a file
//! called `-a`**, and `touch --help` created one called `--help`. Every option
//! is now parsed through [`coreutils::getopt`], and the ones this
//! implementation does not have are refused by name rather than turned into
//! files.
//!
//! ## 2. It read argv as `String`, so a legal file name crashed it
//!
//! `env::args()` panics on an argument that is not valid UTF-8, and on this OS a
//! file name may hold every byte but `/` and NUL (`design.txt`). Argv is now
//! `OsString` and stays bytes all the way to the syscall. See `known-issues.md`
//! → `B-COREUTILS-PANIC-ON-A-NON-UTF-8-ARGUMENT`; `touch` is the seventh bin
//! converted, after `rm`, `mv`, `cp`, `ln`, `mkdir` and `rmdir`.
//!
//! ## 3. It printed the *host's* error text
//!
//! `eprintln!("touch: cannot touch {}: {e}", …)` displays `io::Error`, which on
//! the Windows development host reads `The system cannot find the file
//! specified. (os error 2)` and on SlateOS reads `No such file or directory`.
//! Diagnostics now go through [`coreutils::errmsg::strerror`]. See
//! `known-issues.md` → `TD-B-COREUTILS-PRINT-THE-HOSTS-ERROR-TEXT`.
//!
//! ## 4. `touch existing_file` was a silent no-op on SlateOS
//!
//! This is the one worth reading. The old `touch_one` bumped the modification
//! time like this:
//!
//! ```text
//! let len = file.metadata()?.len();
//! file.set_len(len)?;   // "a portable trick to update mtime"
//! ```
//!
//! Truncating a file to the length it already has is **not** required to touch
//! its timestamps. POSIX says `ftruncate` marks the file for update "if the file
//! size is changed" — and when it is not, it says nothing at all. Linux and
//! Windows both update it anyway, which is why the trick appears to work and why
//! no test caught this.
//!
//! Our own ext4 does not. `kernel/src/fs/ext4/vfs_impl.rs`'s `truncate` opens
//! with
//!
//! ```text
//! if size == current_size {
//!     return Ok(());
//! }
//! ```
//!
//! — no `stamp_inode_mtime` before the early return. So on the one operating
//! system this program is actually for, `touch` on an existing file did
//! nothing: no error, exit 0, timestamp unchanged. `make` would not rebuild,
//! `find -newer` would not find it, and a lock file refreshed by `touch` would
//! look stale forever. The bug was invisible on the development host **by
//! construction**, because both hosts implement the thing POSIX does not
//! promise.
//!
//! The fix is not to make `ftruncate` stamp the inode — that would be a change
//! to another lane's kernel to prop up a trick that should not be used. It is to
//! ask for what we actually want: [`std::fs::File::set_times`], which reaches
//! `futimens(2)` on this target (`toolchain/x86_64-slateos.json` sets
//! `os = linux`, `env = musl`, `target-family = ['unix']`, so std's unix backend
//! applies) and which `posix/src/file.rs` already implements over
//! `SYS_FS_SET_TIMES`.
//!
//! # What GNU does that a reimplementation would not guess
//!
//! Every row below was measured against GNU coreutils 9.4 under
//! `LC_ALL=C.UTF-8`.
//!
//! | Command | GNU | Why |
//! |---|---|---|
//! | `touch /tmp` (a directory) | **exit 0** | the open fails with `EISDIR`, and `utimensat` on the path succeeds anyway |
//! | `touch ro` (mode 444, yours) | **exit 0** | the open fails with `EACCES`, `utimensat` succeeds because you own it |
//! | `touch -` | `setting times of '-': Permission denied`, exit 1, **no file called `-`** | `-` names standard *output*, not a file |
//! | `touch -- -` | the same | `--` ends options; it does not stop `-` meaning standard output |
//! | `touch -c nosuch` | exit 0, silent | `-c` plus `ENOENT` is success, not a suppressed error |
//! | `touch -r /nope` (no operands) | `failed to get attributes of '/nope'` | the reference is read before the missing-operand check |
//!
//! The first two rows are one rule, and it is the rule the old code could not
//! have followed: GNU **always** calls `utimensat` on the path, whether or not
//! the open worked, and reports the *open's* error only when the stamp failed
//! too. That is why `touch` on a file you may not write still works.
//!
//! ## Why the stamp is a path call and not a handle call
//!
//! `std` has no path form of `utimensat` — [`File::set_times`] takes a handle —
//! so the tempting shape is "open something, anything, and stamp *that*". It
//! does not work, and the following was measured (Linux 6.6, glibc) by running
//! the create-open and the stamp in the same order this program does:
//!
//! | Path | the create-open says | `utimensat` says | so `touch` |
//! |---|---|---|---|
//! | ordinary file | ok | ok | exit 0 |
//! | a directory | `Is a directory` | **ok** | exit 0 |
//! | a file of mode 000 you own | `Permission denied` | **ok** | exit 0 |
//! | a FIFO with no reader | `No such device or address` | **ok** | exit 0 |
//! | a unix-domain socket | `No such device or address` | **ok** | exit 0 |
//!
//! Four of those five cannot be opened *at all*, in any mode — a mode-000 file
//! refuses `O_RDONLY` as firmly as `O_WRONLY`, and a socket refuses `open`
//! outright. A handle-based `touch` fails all four where GNU exits 0. So
//! [`fsattr::set_times`] calls `utimensat` directly on the target, the same
//! call gnulib makes, which our own libc already exports (`posix/src/file.rs`,
//! over `SYS_FS_SET_TIMES`).
//!
//! That call, the `timespec` conversion behind it and the Windows arm below all
//! live in [`coreutils::fsattr`] rather than here. `cp -p` needs the same
//! three, and a second `extern "C"` block declaring `timespec` is a second
//! chance to get its field widths wrong — silently, because a wrong layout
//! writes a wrong time rather than failing to compile.
//!
//! The same run also confirms the two properties the rest of the file leans on:
//! `UTIME_OMIT` genuinely leaves the other timestamp untouched (`-a` does not
//! disturb mtime), and all nine digits of the nanosecond field survive
//! (`atime=1000000000.123456789` read back exactly).
//!
//! On the Windows development host there is no path-based equivalent, so that
//! arm opens a handle asking for the least access a stamp needs. It reaches
//! directories and ordinary files — enough for the whole test suite to run —
//! but not a path that **stats and does not open**, which on unix would be a
//! socket or a device node you may not open. See `known-issues.md` →
//! `TD-B-TOUCH-CANNOT-STAMP-A-PATH-IT-CANNOT-OPEN`.
//!
//! # Options this implementation does not have
//!
//! `-d`/`--date`, `-t`, and `-h`/`--no-dereference`. Each is blocked by
//! something this crate genuinely lacks rather than by effort:
//!
//! | Option | What it needs that is not here |
//! |---|---|
//! | `-d STRING` | a full `parse_datetime` — `"next Thursday"`, `"2 hours ago"`, `"@1700000000"` |
//! | `-t STAMP` | civil-time-to-epoch conversion in the *local* zone, including its history |
//! | `-h` | `lutimes`/`AT_SYMLINK_NOFOLLOW`, which `std` does not expose |
//!
//! They are refused by name — `option -d is not implemented by this touch` —
//! rather than ignored, because ignoring any of the three silently does the
//! *opposite* of what was asked: `-d` ignored stamps the file with now instead
//! of the requested time, and `-h` ignored follows a symlink the caller asked
//! not to follow and stamps the wrong file.
//!
//! `-f` is not in that list, because GNU documents it as accepted and ignored —
//! it exists for a BSD `touch` that once had it. Ignoring it *is* the
//! implementation.
//!
//! [`LONG_OPTIONS`] carries GNU's whole table regardless of what is implemented,
//! because the table — not the set of options acted on — is what decides whether
//! an abbreviation is ambiguous. Drop `--no-dereference` and `touch --no`
//! silently becomes `--no-create`, where GNU refuses it.
//!
//! # The obsolescent `touch MMDDhhmm file` form is deliberately absent
//!
//! GNU accepts a leading bare timestamp, but only when `_POSIX2_VERSION` is
//! below 200112 — which it is not on any system built this decade. Reproducing
//! it would mean a date-shaped first operand sometimes being a date and
//! sometimes being a file name.

use coreutils::diag;
use coreutils::errmsg::strerror;
use coreutils::fsattr::{self, Times, When};
use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quoteaf_os};
use coreutils::stdfd::{self, Stream};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::mem::ManuallyDrop;
use std::path::Path;
use std::process::ExitCode;
use std::time::SystemTime;

/// Measured: `touch -q f; echo $?` prints 1.
const TOUCH: Program = Program::new("touch", 1);

/// GNU `touch`'s `longopts[]`, **in its declaration order**, which is
/// observable: `getopt_long` lists an ambiguous prefix's candidates in table
/// order. Measured with an empty prefix, which matches every entry.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("time", Takes::Required),
    ("no-create", Takes::Nothing),
    ("date", Takes::Required),
    ("reference", Takes::Required),
    ("no-dereference", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// Which timestamp an option names. GNU's `CH_ATIME`/`CH_MTIME`.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg_attr(test, derive(Debug))]
enum Which {
    Access,
    Modify,
}

/// `--time`'s words, with the values that decide which spellings are synonyms.
///
/// The grouping is not cosmetic: [`Program::argmatch`] judges an ambiguous
/// abbreviation by *value*, so `--time=a` is ambiguous (it prefixes `atime` and
/// `access`, which agree, but the empty-ish prefix rule still needs the values
/// to compare) while `--time=m` resolves. It is also what renders GNU's list:
///
/// ```text
/// touch: invalid argument ‘x’ for ‘--time’
/// Valid arguments are:
///   - ‘atime’, ‘access’, ‘use’
///   - ‘mtime’, ‘modify’
/// ```
const TIME_WORDS: &[(&str, Which)] = &[
    ("atime", Which::Access),
    ("access", Which::Access),
    ("use", Which::Access),
    ("mtime", Which::Modify),
    ("modify", Which::Modify),
];

/// GNU `touch`'s `getopt_long` string, copied verbatim.
///
/// `-d` and `-t` are declared as taking a value even though they are refused,
/// so that `touch -d` answers `option requires an argument -- 'd'` as GNU does
/// rather than jumping to the refusal — and so that the `2001-01-01` in
/// `touch -d 2001-01-01 f` is not left behind to be created as a file.
const SHORT_OPTIONS: &str = "acd:fhmr:t:";

#[derive(Default, Clone)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct TouchFlags {
    /// `-a`, `--time=atime`.
    change_access: bool,
    /// `-m`, `--time=mtime`.
    change_modify: bool,
    /// `-c`, `--no-create`.
    no_create: bool,
    /// `-r`, `--reference=FILE`.
    reference: Option<OsString>,
}

impl TouchFlags {
    fn select(&mut self, which: Which) {
        match which {
            Which::Access => self.change_access = true,
            Which::Modify => self.change_modify = true,
        }
    }

    /// GNU's `if (change_times == 0) change_times = CH_ATIME | CH_MTIME;`,
    /// applied once options are done rather than at each use, so that what the
    /// rest of the program sees is already resolved.
    fn default_to_both(&mut self) {
        if !self.change_access && !self.change_modify {
            self.change_access = true;
            self.change_modify = true;
        }
    }

    /// The timestamps to write, given the reference file's if there is one.
    ///
    /// A time left [`When::Omit`] means "leave this one alone" — `UTIME_OMIT`
    /// under `utimensat`, a zero `FILETIME` under `SetFileTime` — which is what
    /// makes `-a` able to advance the access time without disturbing the
    /// modification time. Reading the old value and writing it back would not:
    /// it rounds to whatever the two clocks agree on, and it races anyone else
    /// writing the file.
    fn times(&self, reference: Option<Stamp>) -> Times {
        // No `-r` means "now", read per file rather than once for the whole
        // run, because GNU passes `UTIME_NOW` and lets the kernel stamp each
        // call separately.
        let stamp = reference.unwrap_or_else(Stamp::now);
        Times {
            accessed: if self.change_access {
                When::Set(stamp.accessed)
            } else {
                When::Omit
            },
            modified: if self.change_modify {
                When::Set(stamp.modified)
            } else {
                When::Omit
            },
        }
    }
}

/// A pair of timestamps: what to write, or what a reference file holds.
#[derive(Clone, Copy)]
struct Stamp {
    accessed: SystemTime,
    modified: SystemTime,
}

impl Stamp {
    fn now() -> Self {
        let now = SystemTime::now();
        Stamp {
            accessed: now,
            modified: now,
        }
    }
}

/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    Help,
    Version,
    /// The flags, and every operand in order.
    Run(TouchFlags, Vec<OsString>),
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
            println!("touch (SlateOS coreutils) 0.1.0");
            ExitCode::SUCCESS
        }
        Ok(Request::Run(flags, files)) => {
            // `Stream` and not `io::stderr()`, whose failures the runtime hides: a
            // diagnostic that never arrived has to reach `close_stderr`'s flag.
            let mut err = Stream::stderr();
            if touch_all(&flags, &files, &mut err) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            diag!("touch: {e}");
            ExitCode::from(u8::try_from(e.status).unwrap_or(1))
        }
    }
}

fn help_text() -> String {
    "\
Usage: touch [OPTION]... FILE...
Update the access and modification times of each FILE to the current time.

A FILE argument that does not exist is created empty, unless -c is supplied.

A FILE argument string of - is handled specially and causes touch to
change the times of the file associated with standard output.

Mandatory arguments to long options are mandatory for short options too.
  -a                     change only the access time
  -c, --no-create        do not create any files
  -f                     (ignored)
  -m                     change only the modification time
  -r, --reference=FILE   use this file's times instead of current time
      --time=WORD        change the specified time:
                           WORD is access, atime, or use: equivalent to -a
                           WORD is modify or mtime: equivalent to -m
      --help             display this help and exit
      --version          output version information and exit
"
    .to_string()
}

// ---------------------------------------------------------------- parsing ---

/// Parse `touch`'s argv into `(flags, operands)`.
///
/// The walk over argv is [`Program::parse`], shared with every other converted
/// utility; what is left here is only what `touch` does with each item. Options
/// and operands may be interleaved — `touch a -c b` is `touch -c a b` — which is
/// `getopt_long`'s default permuting behaviour.
///
/// `--help` and `--version` return immediately rather than setting a flag, which
/// is why this consumes the walk in a `for` loop instead of collecting it.
/// Measured: `touch --help --bogus` prints the help and exits 0, while
/// `touch --bogus --help` is an error — so an option after `--help` must never
/// be looked at.
///
/// # Errors
///
/// An unknown option, a recognised option this implementation does not have, an
/// option missing its argument, or a bad `--time` word.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut flags = TouchFlags::default();
    let mut files: Vec<OsString> = Vec::new();

    for item in TOUCH.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item? {
            // A lone `-` arrives here as an operand and survives to
            // [`touch_one`], which is where it becomes standard output rather
            // than a file. Measured: `touch -- -` behaves exactly like
            // `touch -`, so `--` must not turn it into a name.
            Opt::Operand(file) => files.push(file.clone()),

            Opt::Short(b'a', _) => flags.select(Which::Access),
            Opt::Short(b'm', _) => flags.select(Which::Modify),
            Opt::Short(b'c', _) | Opt::Long("no-create", _) => flags.no_create = true,
            // Accepted and discarded, which is what GNU's `--help` means by
            // `-f  (ignored)`. It is compatibility ballast for a BSD `touch`, so
            // ignoring it is the implementation rather than the absence of one.
            Opt::Short(b'f', _) => {}
            Opt::Short(b'r', value) | Opt::Long("reference", value) => flags.reference = value,
            Opt::Long("time", value) => {
                // `--time` is `Takes::Required`, so this is always `Some`. An
                // empty word is nonetheless the right thing to fall back to: it
                // is what GNU answers `ambiguous argument ‘’ for ‘--time’` to,
                // which is exactly what a missing one deserves.
                let word = value.unwrap_or_default();
                flags.select(TOUCH.argmatch(&os_bytes(&word), "--time", TIME_WORDS)?);
            }

            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),

            // The value of `-d`/`-t` was consumed before the refusal on purpose:
            // it means `touch -d` still reports a *missing argument*, and it
            // means the `2001-01-01` in `touch -d 2001-01-01 f` cannot be left
            // behind to be created as a file if the refusal is ever softened.
            Opt::Short(flag @ (b'd' | b'h' | b't'), _) => return Err(unimplemented_short(flag)),
            Opt::Long(name, _) => return Err(unimplemented_long(name)),
            // Unreachable: every letter of [`SHORT_OPTIONS`] is matched above,
            // and one that is not in it never gets this far — the walk answers
            // `invalid option` itself. It is spelled out rather than left to a
            // catch-all so that adding a letter to the string without handling
            // it is a compile-time gap and not a silent no-op.
            Opt::Short(other, _) => return Err(TOUCH.invalid_option(other)),
        }
    }

    flags.default_to_both();
    Ok(Request::Run(flags, files))
}

/// The diagnostic for an option that GNU `touch` has and this one does not.
///
/// Deliberately not [`Program::invalid_option`]: `-d` is not a typo, and telling
/// the user it is invalid sends them to check their spelling of a flag they
/// spelled correctly.
fn unimplemented_short(flag: u8) -> getopt::Error {
    TOUCH.usage_referring(format!(
        "option -{} is not implemented by this touch",
        char::from(flag)
    ))
}

fn unimplemented_long(name: &str) -> getopt::Error {
    TOUCH.usage_referring(format!(
        "option '--{name}' is not implemented by this touch"
    ))
}

// --------------------------------------------------------------- touching ---

/// Why a file could not be touched, and so which of GNU's two sentences says so.
enum Failure {
    /// The file could not be opened **and** its times could not be set either.
    ///
    /// GNU reports the open's error rather than the stamp's, and the wording is
    /// vague on purpose: upstream's comment says it has to cover both "the file
    /// does not exist and the parent directory is unwritable" and "the file
    /// exists and is unwritable".
    CannotTouch(io::Error),
    /// There was no open to fail, or it succeeded, and the stamp is what did
    /// not.
    SettingTimes(io::Error),
}

impl Failure {
    fn describe(&self, file: &OsStr) -> String {
        let name = quoteaf_os(file);
        match self {
            Failure::CannotTouch(e) => format!("cannot touch {name}: {}", strerror(e)),
            Failure::SettingTimes(e) => format!("setting times of {name}: {}", strerror(e)),
        }
    }
}

/// Touch every operand, reporting failures to `err`.
///
/// Returns `true` if everything asked for succeeded. Takes the error sink as a
/// parameter rather than writing to `stderr` directly so the diagnostics can be
/// asserted on in tests; the file it replaces had no test of this path at all.
///
/// One failure does not abandon the rest — but a bad `-r` does, because there is
/// then no time to write and every operand would fail identically.
fn touch_all<W: Write>(flags: &TouchFlags, files: &[OsString], err: &mut W) -> bool {
    // Before the operand check, which is GNU's order: measured, `touch -r /nope`
    // with no operands at all reports the reference and not the missing operand.
    let reference = match flags.reference.as_ref() {
        None => None,
        Some(path) => match reference_times(path) {
            Ok(stamp) => Some(stamp),
            Err(e) => {
                let _ = writeln!(
                    err,
                    "touch: failed to get attributes of {}: {}",
                    quoteaf_os(path),
                    strerror(&e)
                );
                return false;
            }
        },
    };

    if files.is_empty() {
        let _ = writeln!(
            err,
            "touch: {}",
            TOUCH.usage_referring("missing file operand".into())
        );
        return false;
    }

    let mut ok = true;
    for file in files {
        if let Err(failure) = touch_one(flags, file, reference) {
            let _ = writeln!(err, "touch: {}", failure.describe(file));
            ok = false;
        }
    }
    ok
}

/// The access and modification times of `-r`'s file.
///
/// Follows symlinks, which is right because the only option that would not —
/// `-h` — is refused.
fn reference_times(path: &OsStr) -> io::Result<Stamp> {
    let meta = fs::metadata(Path::new(path))?;
    Ok(Stamp {
        accessed: meta.accessed()?,
        modified: meta.modified()?,
    })
}

/// Create `file` if it is missing, then stamp it.
///
/// This is gnulib `touch.c`'s `touch()`, and the order of the two steps is the
/// part worth keeping: the stamp is attempted **whether or not the open
/// worked**, and the open's error is reported only if the stamp failed too. See
/// the module docs — that is what makes `touch` succeed on a directory and on a
/// file you may not write.
///
/// # Errors
///
/// The file could not be created, or its times could not be set. `-c` plus "no
/// such file" is not an error: it is what `-c` asks for.
fn touch_one(flags: &TouchFlags, file: &OsStr, reference: Option<Stamp>) -> Result<(), Failure> {
    let times = flags.times(reference);

    if *os_bytes(file) == *b"-" {
        // `-` names standard output. Measured: `touch -` prints
        // `setting times of '-': Permission denied` on a terminal and creates no
        // file, and `touch -- -` does the same — so this is not an option-parsing
        // rule that `--` could switch off.
        return stdout_as_file()
            .set_times(times.to_file_times())
            .map_err(Failure::SettingTimes);
    }

    let path = Path::new(file);
    let mut open_error: Option<io::Error> = None;
    if !flags.no_create {
        // The handle is deliberately not kept. GNU stamps the *path*, not the
        // descriptor, so holding this open would buy nothing — and dropping it
        // here means the file is closed before the stamp rather than after,
        // which is one fewer thing to reason about.
        if let Err(e) = create_open(path) {
            open_error = Some(e);
        }
    }

    match fsattr::set_times(path, times) {
        Ok(()) => Ok(()),
        Err(e) => match open_error {
            Some(open) => Err(Failure::CannotTouch(open)),
            // `-c` means "do not create", not "create quietly": a file that is
            // simply not there is the case `-c` exists for, and it is a success.
            None if flags.no_create && e.kind() == io::ErrorKind::NotFound => Ok(()),
            None => Err(Failure::SettingTimes(e)),
        },
    }
}

/// The create-if-missing open, with the flags gnulib passes.
///
/// `touch.c` opens with `O_WRONLY | O_CREAT | O_NONBLOCK | O_NOCTTY`, and the
/// last two are not decoration:
///
/// - **`O_NONBLOCK`** — opening a FIFO for writing **blocks until a reader
///   arrives**. Without it, `touch some_fifo` does not fail and does not
///   succeed; it hangs, forever, and no timeout in this program would save it.
///   With it the open returns `ENXIO` immediately, the stamp is attempted
///   anyway, and `touch` behaves as it should on a FIFO.
/// - **`O_NOCTTY`** — opening a terminal device that is nobody's controlling
///   terminal would otherwise *make it* this process's controlling terminal, so
///   `touch /dev/tty3` would quietly rearrange the session it runs in.
///
/// Neither flag is reachable through portable [`OpenOptions`], so this is where
/// the unix and non-unix arms part. The values are Linux's, and match
/// `posix/src/fcntl.rs`.
///
/// # Errors
///
/// The file could not be created or opened for writing. The caller records the
/// error rather than reporting it — see [`touch_one`].
#[cfg(unix)]
fn create_open(path: &Path) -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;

    /// `O_NOCTTY` — do not adopt a terminal as our controlling terminal.
    const O_NOCTTY: i32 = 0o400;
    /// `O_NONBLOCK` — do not wait for a FIFO's reader.
    const O_NONBLOCK: i32 = 0o4000;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .custom_flags(O_NOCTTY | O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn create_open(path: &Path) -> io::Result<File> {
    // Windows has neither flag, and neither hazard: a named pipe is not opened
    // by this path, and there is no controlling-terminal concept to acquire.
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
}

/// Write `times` to the file `path` names, without opening it for I/O.
///
/// Standard output as a [`File`] that will not be closed.
///
/// [`File::set_times`] is the only route from `std` to `futimens`, and it is a
/// method on `File` — there is no free function taking a descriptor. So `touch
/// -` has to wrap the descriptor it already has. [`ManuallyDrop`] is what stops
/// the wrapper from closing standard output when it goes out of scope, which
/// would otherwise happen at the end of the very first `touch -`.
#[cfg(unix)]
fn stdout_as_file() -> ManuallyDrop<File> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // SAFETY: standard output's descriptor is open for the life of the process,
    // and the `ManuallyDrop` guarantees this `File` never closes it — so the
    // descriptor stays valid for every other holder, and this borrow ends
    // without a side effect.
    ManuallyDrop::new(unsafe { File::from_raw_fd(io::stdout().as_raw_fd()) })
}

#[cfg(windows)]
fn stdout_as_file() -> ManuallyDrop<File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    // SAFETY: as for the unix arm — the handle is the process's own standard
    // output, valid for its lifetime, and `ManuallyDrop` stops this `File` from
    // closing it.
    ManuallyDrop::new(unsafe { File::from_raw_handle(io::stdout().as_raw_handle()) })
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
    use std::time::Duration;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    /// `(flags, operands)` from a successful parse, or a panic naming the error.
    fn run_parse(items: &[&str]) -> (TouchFlags, Vec<String>) {
        match parse_args(&args(items)).unwrap() {
            Request::Run(f, files) => (
                f,
                files
                    .iter()
                    .map(|o| o.to_string_lossy().into_owned())
                    .collect(),
            ),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    fn fail(items: &[&str]) -> getopt::Error {
        parse_args(&args(items)).unwrap_err()
    }

    // ------------------------------------------------------------ parsing --
    //
    // The walk over argv now lives in `coreutils::getopt`, and its own tests
    // there are the specification of the four value spellings, the bundle that
    // ends in a value-taking letter, the missing value, and the rest. What is
    // tested here is one layer up and is not the same thing: that `touch` wires
    // each item to the right field. `-r ref` arriving as `Opt::Short(b'r', …)`
    // is the driver's business; that it ends up in `flags.reference` rather
    // than being pushed onto `files` is `touch`'s, and is exactly what the bug
    // these tests were written for got wrong.

    /// The old parser returned every word as a file name, so `touch -a f` made a
    /// file called `-a`. This is the assertion that would have caught it.
    #[test]
    fn an_option_is_not_a_file_name() {
        for typed in ["-a", "-m", "-c", "-f", "--no-create"] {
            let (_, files) = run_parse(&[typed, "f"]);
            assert_eq!(files, vec!["f"], "{typed} was taken as an operand");
        }
    }

    #[test]
    fn operands_only() {
        let (f, files) = run_parse(&["one", "two"]);
        assert_eq!(files, vec!["one", "two"]);
        assert!(!f.no_create);
        assert_eq!(f.reference, None);
    }

    /// GNU's `if (change_times == 0) change_times = CH_ATIME | CH_MTIME;`.
    #[test]
    fn neither_a_nor_m_means_both() {
        let (f, _) = run_parse(&["f"]);
        assert!(f.change_access && f.change_modify);
    }

    #[test]
    fn a_and_m_select_one_time_each() {
        let (a, _) = run_parse(&["-a", "f"]);
        assert!(a.change_access && !a.change_modify);
        let (m, _) = run_parse(&["-m", "f"]);
        assert!(!m.change_access && m.change_modify);
        // Both named is both, and is not the same code path as naming neither.
        let (both, _) = run_parse(&["-am", "f"]);
        assert!(both.change_access && both.change_modify);
    }

    #[test]
    fn the_time_option_is_a_spelling_of_a_and_m() {
        for word in ["atime", "access", "use"] {
            let (f, _) = run_parse(&[&format!("--time={word}"), "f"]);
            assert!(f.change_access && !f.change_modify, "{word}");
        }
        for word in ["mtime", "modify"] {
            let (f, _) = run_parse(&[&format!("--time={word}"), "f"]);
            assert!(!f.change_access && f.change_modify, "{word}");
        }
    }

    /// Measured: `touch --time modify f` and `touch --t=modify f` both work.
    #[test]
    fn the_time_option_takes_its_word_either_way() {
        assert!(run_parse(&["--time", "modify", "f"]).0.change_modify);
        assert!(run_parse(&["--t=modify", "f"]).0.change_modify);
        // The word abbreviates too, exactly as an option name does.
        assert!(run_parse(&["--time=mod", "f"]).0.change_modify);
        assert!(run_parse(&["--time=u", "f"]).0.change_access);
    }

    /// The gnulib `argmatch` message, verbatim from GNU 9.4 under
    /// `LC_ALL=C.UTF-8` — curly marks and all, because this is the one
    /// diagnostic family gnulib writes rather than glibc.
    #[test]
    fn a_bad_time_word_lists_the_valid_ones() {
        let e = fail(&["--time=x", "f"]);
        assert_eq!(
            e.sentence,
            "invalid argument ‘x’ for ‘--time’\nValid arguments are:\n  \
             - ‘atime’, ‘access’, ‘use’\n  - ‘mtime’, ‘modify’"
        );
        assert_eq!(e.status, 1);
        // An empty word matches every entry, and the entries disagree.
        assert!(
            fail(&["--time=", "f"])
                .sentence
                .starts_with("ambiguous argument ‘’ for ‘--time’")
        );
    }

    #[test]
    fn reference_takes_its_file_in_all_four_spellings() {
        for typed in [
            &["-r", "ref", "f"][..],
            &["-rref", "f"][..],
            &["--reference=ref", "f"][..],
            &["--reference", "ref", "f"][..],
        ] {
            let (flags, files) = run_parse(typed);
            assert_eq!(
                flags.reference.as_deref(),
                Some(OsStr::new("ref")),
                "{typed:?}"
            );
            assert_eq!(files, vec!["f"], "{typed:?}");
        }
    }

    /// Measured: `touch -cr ref zz` is `touch -c -r ref zz`.
    #[test]
    fn a_bundle_ending_in_an_argument_option_still_bundles() {
        let (flags, files) = run_parse(&["-cr", "ref", "zz"]);
        assert!(flags.no_create);
        assert_eq!(flags.reference.as_deref(), Some(OsStr::new("ref")));
        assert_eq!(files, vec!["zz"]);
    }

    #[test]
    fn an_option_that_wants_a_value_and_has_none() {
        assert_eq!(fail(&["-r"]).sentence, "option requires an argument -- 'r'");
        assert_eq!(
            fail(&["--reference"]).sentence,
            "option '--reference' requires an argument"
        );
        // The refused ones report the missing value first, as GNU does: the
        // refusal is about the option, and there is not yet a whole option.
        assert_eq!(fail(&["-d"]).sentence, "option requires an argument -- 'd'");
        assert_eq!(fail(&["-t"]).sentence, "option requires an argument -- 't'");
    }

    /// A value-taking option must swallow its value, or the value becomes a
    /// file — which is exactly the class of bug the old parser had wholesale.
    #[test]
    fn a_refused_option_still_swallows_its_value() {
        let e = fail(&["-d", "2001-01-01", "f"]);
        assert!(e.sentence.contains("not implemented"), "{:?}", e.sentence);
    }

    #[test]
    fn help_and_version_are_requests() {
        assert_eq!(parse_args(&args(&["--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
    }

    /// The whole table, in GNU's declaration order, as `touch --=x` prints it.
    /// An empty prefix matches every entry, so this pins the order itself.
    #[test]
    fn the_empty_prefix_lists_the_table_in_order() {
        assert_eq!(
            fail(&["--=x"]).sentence,
            "option '--=x' is ambiguous; possibilities: '--time' '--no-create' \
             '--date' '--reference' '--no-dereference' '--help' '--version'"
        );
    }

    /// The test that fails if someone prunes the table down to the options this
    /// implementation acts on — which would silently turn `touch --no` from an
    /// error into `--no-create`. Both sentences are measured.
    #[test]
    fn no_prefixed_abbreviations_stay_ambiguous() {
        for typed in ["--n", "--no"] {
            assert_eq!(
                fail(&[typed, "f"]).sentence,
                format!(
                    "option '{typed}' is ambiguous; possibilities: \
                     '--no-create' '--no-dereference'"
                )
            );
        }
        // …and an abbreviation that is *not* ambiguous still resolves.
        assert!(run_parse(&["--no-c", "f"]).0.no_create);
    }

    #[test]
    fn unknown_short_is_invalid_option() {
        let e = fail(&["-q", "f"]);
        assert_eq!(e.sentence, "invalid option -- 'q'");
        assert_eq!(e.status, 1);
    }

    #[test]
    fn unrecognized_long_echoes_what_was_typed() {
        assert_eq!(
            fail(&["--zzz=1", "f"]).sentence,
            "unrecognized option '--zzz=1'"
        );
    }

    #[test]
    fn value_on_an_option_that_takes_none() {
        assert_eq!(
            fail(&["--no-create=yes", "f"]).sentence,
            "option '--no-create' doesn't allow an argument"
        );
    }

    /// Ignoring any of these silently does the *opposite* of what was asked —
    /// see the module docs — so they are refused by name.
    #[test]
    fn unimplemented_options_are_rejected_by_name() {
        for typed in [
            &["-h", "f"][..],
            &["--no-dereference", "f"][..],
            &["-d", "now", "f"][..],
            &["--date=now", "f"][..],
            &["-t", "202001010000", "f"][..],
        ] {
            let e = parse_args(&args(typed)).unwrap_err();
            assert!(e.sentence.contains("not implemented"), "{typed:?}: {e:?}");
        }
    }

    /// GNU's `--help` says `-f  (ignored)`, so ignoring it is the
    /// implementation. It must not join the refused list by accident.
    #[test]
    fn dash_f_is_accepted_and_does_nothing() {
        let (with, _) = run_parse(&["-f", "x"]);
        let (without, _) = run_parse(&["x"]);
        assert_eq!(with, without);
    }

    #[test]
    fn double_dash_ends_options() {
        assert_eq!(run_parse(&["--", "-a", "f"]).1, vec!["-a", "f"]);
        let (f, files) = run_parse(&["--", "-c"]);
        assert!(!f.no_create, "-c after -- is a file name, not a flag");
        assert_eq!(files, vec!["-c"]);
    }

    #[test]
    fn bare_dash_is_an_operand() {
        assert_eq!(run_parse(&["-"]).1, vec!["-"]);
        // …and `--` does not turn it into an ordinary name, because it never was
        // an option. Measured: `touch -- -` still stamps standard output.
        assert_eq!(run_parse(&["--", "-"]).1, vec!["-"]);
    }

    #[test]
    fn options_may_follow_operands() {
        let (f, files) = run_parse(&["one", "-c", "two"]);
        assert!(f.no_create);
        assert_eq!(files, vec!["one", "two"]);
    }

    // --------------------------------------------------- non-UTF-8 argv --

    /// The regression test for defect 2. Byte `0x80` alone is not valid UTF-8,
    /// so an operand containing it cannot be a `String` at all — `env::args()`
    /// would have panicked before `touch` saw it.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_operand_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'a', 0x80, b'b']);
        assert!(
            bad.to_str().is_none(),
            "the fixture must be un-representable as String, or it tests nothing"
        );
        match parse_args(&[OsString::from("-c"), bad.clone()]).unwrap() {
            Request::Run(f, files) => {
                assert!(f.no_create);
                assert_eq!(files, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    /// `-r`'s value is a file name too, and it travels through the option
    /// parser rather than straight into the operand list — a second place the
    /// bytes could be lost.
    #[test]
    #[cfg(unix)]
    fn a_non_utf8_reference_survives_parsing() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'r', 0xff]);
        for typed in [vec![OsString::from("-r"), bad.clone()], {
            let mut inline = OsString::from("-r");
            inline.push(&bad);
            vec![inline]
        }] {
            match parse_args(&typed).unwrap() {
                Request::Run(f, _) => assert_eq!(f.reference.as_ref(), Some(&bad)),
                other => panic!("expected Run, got {other:?}"),
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'-', b'-', 0x80]);
        assert!(
            parse_args(&[bad])
                .unwrap_err()
                .sentence
                .starts_with("unrecognized option")
        );
    }

    /// The three tests above are `#[cfg(unix)]`, so on the development host —
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
        match parse_args(&[OsString::from("-c"), bad.clone()]).unwrap() {
            Request::Run(f, files) => {
                assert!(f.no_create);
                assert_eq!(files, vec![bad]);
            }
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn a_non_utf8_reference_survives_parsing() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x0072, 0xD800]);
        match parse_args(&[OsString::from("-r"), bad.clone()]).unwrap() {
            Request::Run(f, _) => assert_eq!(f.reference.as_ref(), Some(&bad)),
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    #[cfg(windows)]
    fn a_non_utf8_long_option_is_unrecognised_not_a_panic() {
        use std::os::windows::ffi::OsStringExt;
        let bad = OsString::from_wide(&[0x002D, 0x002D, 0xD800]);
        assert!(
            parse_args(&[bad])
                .unwrap_err()
                .sentence
                .starts_with("unrecognized option")
        );
    }

    // --------------------------------------------------------- touching --

    fn scratch(stem: &str) -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("touch_test_{stem}_{pid}_{n}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Run `touch_all`, returning `(ok, diagnostics)`.
    fn run(flags: &TouchFlags, files: &[&Path]) -> (bool, String) {
        let owned: Vec<OsString> = files.iter().map(|p| p.as_os_str().to_owned()).collect();
        let mut err: Vec<u8> = Vec::new();
        let ok = touch_all(flags, &owned, &mut err);
        (ok, String::from_utf8_lossy(&err).into_owned())
    }

    /// The default flags, as `parse_args` would leave them for `touch FILE`.
    fn plain() -> TouchFlags {
        let mut f = TouchFlags::default();
        f.default_to_both();
        f
    }

    /// Move a file's timestamps back an hour, so a later assertion that they
    /// advanced cannot pass on clock granularity alone.
    fn backdate(path: &Path) -> SystemTime {
        let old = SystemTime::now()
            .checked_sub(Duration::from_secs(3600))
            .expect("an hour before now is representable");
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(Times::both(old).to_file_times())
            .unwrap();
        old
    }

    fn mtime(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().modified().unwrap()
    }

    fn atime(path: &Path) -> SystemTime {
        fs::metadata(path).unwrap().accessed().unwrap()
    }

    #[test]
    fn a_missing_file_is_created_empty() {
        let d = scratch("create");
        let f = d.join("new");
        let (ok, msg) = run(&plain(), &[&f]);
        assert!(ok, "{msg}");
        assert!(f.is_file());
        assert_eq!(fs::read(&f).unwrap(), b"");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn an_existing_file_keeps_its_contents() {
        let d = scratch("preserve");
        let f = d.join("f");
        fs::write(&f, b"hello").unwrap();
        let (ok, msg) = run(&plain(), &[&f]);
        assert!(ok, "{msg}");
        assert_eq!(fs::read(&f).unwrap(), b"hello");
        let _ = fs::remove_dir_all(&d);
    }

    /// **Defect 4.** The whole point of this program is that the timestamp
    /// moves, and there was no test that it did — which is how the old
    /// `set_len(len)` trick survived being a no-op on SlateOS's ext4. This test
    /// passes on the development host either way, because Linux and Windows both
    /// update `mtime` on a no-op truncate; what closes the hole is that the
    /// implementation now *asks* for a stamp rather than hoping for a side
    /// effect. The test is here so that any future implementation that stops
    /// stamping is caught on every host.
    #[test]
    fn touching_an_existing_file_advances_its_mtime() {
        let d = scratch("mtime");
        let f = d.join("f");
        fs::write(&f, b"hello").unwrap();
        let old = backdate(&f);

        let (ok, msg) = run(&plain(), &[&f]);
        assert!(ok, "{msg}");
        assert!(
            mtime(&f) > old,
            "touch must move the modification time forward"
        );
        assert!(atime(&f) > old, "and the access time with it");
        assert_eq!(fs::read(&f).unwrap(), b"hello", "without touching the data");
        let _ = fs::remove_dir_all(&d);
    }

    /// A directory can be touched, and the *creation* attempt failing is not an
    /// error. This is the rule the whole two-step shape of [`touch_one`] exists
    /// for: `open(O_WRONLY|O_CREAT)` on a directory fails (`EISDIR` on the
    /// target, "Access is denied" on the development host) and the stamp is
    /// attempted regardless, so the open's error is never reported.
    ///
    /// Measured: GNU `touch /tmp` exits 0 and moves `/tmp`'s mtime.
    ///
    /// This test is what pins [`fsattr::set_times`]'s Windows arm in place. It
    /// lives here rather than beside that function because a directory is a
    /// `touch` behaviour before it is an `fsattr` one. The obvious
    /// `File::open` spelling fails it twice over on the host — a directory will
    /// not open without `FILE_FLAG_BACKUP_SEMANTICS`, and a `GENERIC_READ`
    /// handle has no `FILE_WRITE_ATTRIBUTES` for `SetFileTime`.
    #[test]
    fn a_directory_can_be_touched() {
        let d = scratch("adir");
        let sub = d.join("sub");
        fs::create_dir(&sub).unwrap();

        let old = SystemTime::now()
            .checked_sub(Duration::from_secs(3600))
            .unwrap();
        // Backdate the directory itself, through the same door the program uses,
        // so "it moved forward" cannot pass on clock granularity.
        fsattr::set_times(
            &sub,
            Times {
                accessed: When::Set(old),
                modified: When::Set(old),
            },
        )
        .unwrap();

        let (ok, msg) = run(&plain(), &[&sub]);
        assert!(ok, "touching a directory must succeed: {msg}");
        assert_eq!(msg, "", "and say nothing");
        assert!(mtime(&sub) > old, "the directory's mtime must advance");
        assert!(sub.is_dir(), "and it is still a directory");
        assert!(
            !d.join("sub").is_file(),
            "no file may have been created in its place"
        );
        let _ = fs::remove_dir_all(&d);
    }

    /// `-a` and `-m` each leave the other timestamp exactly where it was.
    /// Measured with `stat -c '%X %Y'` either side of a GNU `touch -a`.
    ///
    /// The untouched half must be *equal*, not merely "not advanced": an
    /// implementation that read the old value and wrote it back would pass a
    /// "did not advance" check while quietly rounding the value.
    #[test]
    fn a_and_m_leave_the_other_time_alone() {
        let d = scratch("select");
        for (flag, name) in [(Which::Access, "a"), (Which::Modify, "m")] {
            let f = d.join(name);
            fs::write(&f, b"x").unwrap();
            let old = backdate(&f);

            let mut flags = TouchFlags::default();
            flags.select(flag);
            let (ok, msg) = run(&flags, &[&f]);
            assert!(ok, "{msg}");

            match flag {
                Which::Access => {
                    assert!(atime(&f) > old, "-a must advance the access time");
                    assert_eq!(mtime(&f), old, "-a must not touch the modification time");
                }
                Which::Modify => {
                    assert!(mtime(&f) > old, "-m must advance the modification time");
                    assert_eq!(atime(&f), old, "-m must not touch the access time");
                }
            }
        }
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `touch -c nosuch; echo $?` prints 0 and creates nothing.
    #[test]
    fn no_create_on_a_missing_file_is_a_silent_success() {
        let d = scratch("nocreate");
        let f = d.join("nosuch");
        let mut flags = plain();
        flags.no_create = true;
        let (ok, msg) = run(&flags, &[&f]);
        assert!(ok, "{msg}");
        assert!(msg.is_empty(), "{msg}");
        assert!(!f.exists(), "-c must not create the file");
        let _ = fs::remove_dir_all(&d);
    }

    /// `-c` suppresses the *creation*, not the stamp: an existing file still
    /// gets its times set.
    #[test]
    fn no_create_still_stamps_a_file_that_is_there() {
        let d = scratch("nocreate2");
        let f = d.join("f");
        fs::write(&f, b"x").unwrap();
        let old = backdate(&f);
        let mut flags = plain();
        flags.no_create = true;
        let (ok, msg) = run(&flags, &[&f]);
        assert!(ok, "{msg}");
        assert!(mtime(&f) > old);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_reference_files_times_are_copied() {
        let d = scratch("reference");
        let reference = d.join("ref");
        let target = d.join("t");
        fs::write(&reference, b"r").unwrap();
        fs::write(&target, b"t").unwrap();
        let old = backdate(&reference);

        let mut flags = plain();
        flags.reference = Some(reference.as_os_str().to_owned());
        let (ok, msg) = run(&flags, &[&target]);
        assert!(ok, "{msg}");
        assert_eq!(mtime(&target), old, "the target takes the reference's time");
        assert_eq!(atime(&target), old);
        let _ = fs::remove_dir_all(&d);
    }

    /// Measured: `touch -r /nope` reports the reference and exits 1 **even with
    /// no operands at all**, which pins the order of the two checks.
    #[test]
    fn a_missing_reference_is_fatal_and_is_checked_first() {
        let d = scratch("badref");
        let target = d.join("t");
        let mut flags = plain();
        flags.reference = Some(d.join("nosuch").into_os_string());

        let (ok, msg) = run(&flags, &[&target]);
        assert!(!ok);
        assert!(msg.contains("failed to get attributes of"), "{msg}");
        assert!(msg.contains("No such file or directory"), "{msg}");
        assert!(!msg.contains("os error"), "host wording leaked: {msg}");
        assert!(
            !target.exists(),
            "nothing is touched once the reference fails"
        );

        // With no operands, the reference is still what gets reported.
        let (ok, msg) = run(&flags, &[]);
        assert!(!ok);
        assert!(msg.contains("failed to get attributes of"), "{msg}");
        assert!(!msg.contains("missing file operand"), "{msg}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn no_operands_names_the_missing_thing() {
        let (ok, msg) = run(&plain(), &[]);
        assert!(!ok);
        assert!(msg.contains("missing file operand"), "{msg}");
        assert!(msg.contains("Try 'touch --help'"), "{msg}");
    }

    /// **Defect 3.** The wording is POSIX's rather than the host's, which on
    /// Windows is the difference between `No such file or directory` and
    /// `The system cannot find the path specified. (os error 3)`.
    #[test]
    fn an_uncreatable_file_is_refused_in_posix_words() {
        let d = scratch("uncreatable");
        let f = d.join("nosuchdir").join("f");
        let (ok, msg) = run(&plain(), &[&f]);
        assert!(!ok);
        assert!(msg.contains("cannot touch"), "{msg}");
        assert!(msg.contains("No such file or directory"), "{msg}");
        assert!(!msg.contains("os error"), "host wording leaked: {msg}");
        let _ = fs::remove_dir_all(&d);
    }

    /// The two sentences are different, and which one appears says whether the
    /// *open* or the *stamp* is what failed — so a file that could not be
    /// created must not be reported as one whose times could not be set.
    #[test]
    fn a_failed_creation_is_not_reported_as_a_failed_stamp() {
        let d = scratch("shape");
        let (_, msg) = run(&plain(), &[&d.join("nosuchdir").join("f")]);
        assert!(!msg.contains("setting times of"), "{msg}");
    }

    #[test]
    fn one_failure_does_not_abandon_the_others() {
        let d = scratch("partial");
        let good = d.join("good");
        let bad = d.join("nosuchdir").join("f");
        let (ok, msg) = run(&plain(), &[&bad, &good]);
        assert!(!ok, "the bad one must count against the status");
        assert!(good.is_file(), "the good one is still touched: {msg}");
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        let _ = fs::remove_dir_all(&d);
    }

    /// A file name may hold a newline, so a raw one in a diagnostic lets whoever
    /// chose the name write a line that looks like a second message from
    /// `touch`.
    #[test]
    fn a_name_cannot_forge_a_second_diagnostic_line() {
        let d = scratch("forge");
        let evil = d
            .join("nosuchdir")
            .join("a\ntouch: /etc: Permission denied");
        let (ok, msg) = run(&plain(), &[&evil]);
        assert!(!ok);
        assert_eq!(msg.lines().count(), 1, "{msg:?}");
        assert!(msg.contains(r"\n"), "the newline must be escaped: {msg:?}");
        let _ = fs::remove_dir_all(&d);
    }
}
