//! `renice` — alter the priority of running processes.
//!
//! A transcription of util-linux 2.39.3's `sys-utils/renice.c`, measured
//! against the system binary of that exact version (`renice from util-linux
//! 2.39.3`). The reference is util-linux rather than BSD for the reasons
//! recorded in `design-decisions.md` §622: it is the `renice` a script ported
//! to this OS was written against.
//!
//! # What was wrong with the implementation this replaces
//!
//! 1. **`argv` was read as `Vec<String>`**, so an argument holding a byte that
//!    is not valid UTF-8 aborted the process. That is the defect this sweep
//!    exists to remove, and on this OS such a byte is a legal filename.
//! 2. **`-n` was always relative.** Upstream's `-n` is *absolute* unless
//!    `POSIXLY_CORRECT` is set in the environment — a documented wart, and the
//!    opposite of what POSIX says, but what every script written since 2009
//!    depends on. Measured: with the shell at niceness 5, `renice -n 1 $$`
//!    fails (it would need privilege to reach 1) while `POSIXLY_CORRECT=1
//!    renice -n 1 $$` reaches 6.
//! 3. **The priority operand was optional.** `renice foo` read `foo` as a PID.
//!    Upstream requires a priority word first and refuses a non-numeric one.
//! 4. **No `-g`/`--pgrp`**: process groups could not be reniced at all.
//! 5. **No long options.** `--priority`, `--relative`, `--pid`, `--user`,
//!    `--help` and `--version` were all unknown, and because a bare word was
//!    read as a PID, `renice 5 --pid 100` failed with `invalid PID: --pid`.
//! 6. **No `-h`, `-v`, `-V`, `--help` or `--version`.**
//! 7. **`-u alice` demanded a numeric UID** and printed a two-line apology
//!    saying `/etc/passwd` lookup was not implemented. `pwdb` has done that for
//!    a long time; `id` and `chown` both use it.
//! 8. **The success line printed the new priority twice** under two labels and
//!    never showed the old one: `PID 42: old priority -> new priority 5`.
//!    Upstream prints `42 (process ID) old priority 0, new priority 5`.
//! 9. **`errno` was discarded.** Every failure became the one invented
//!    sentence `failed to set priority (permission denied or no such
//!    process)`, so "no such process" and "not permitted" — which call for
//!    completely different responses — were indistinguishable.
//! 10. **Priorities were clamped to [−20, 19] before the syscall**, so a
//!     request the kernel would have refused was silently rewritten into one it
//!     accepts. Upstream passes the number through and lets the kernel clamp,
//!     which is why `renice 2147483647 $$` reports `new priority 19` rather
//!     than failing.
//! 11. **Operands went through `str::parse::<u32>()`**, which is not
//!     `strtol`: `" 5"` and `+5` were refused where upstream accepts them, and
//!     `4294967296` was refused where upstream truncates it to `0` and reniced
//!     process 0.
//! 12. **`renice` with no arguments printed an invented usage line** instead
//!     of `not enough arguments` and the `Try 'renice --help'` referral.
//! 13. **`println!` panicked on `EPIPE`** — `renice 0 -p 1 | true` aborted.
//! 14. **No `guard_std_fds!()`**, so `renice 0 -p 1 >&-` could write its
//!     success line onto whatever file happened to open as descriptor 1.
//! 15. **`renice -n` said `option -n requires an argument`.** Upstream has no
//!     such message, because `-n` is not an option with an argument — it is a
//!     flag that changes how the *next positional word* is read, so `renice -n`
//!     is `not enough arguments`.
//!
//! # Measured against util-linux 2.39.3
//!
//! | Command | Result |
//! |---|---|
//! | `renice` | `renice: not enough arguments` + referral, status 1 |
//! | `renice 5` | the same — the check is on the count, not on what is missing |
//! | `renice -n 5` | the same again; `-n` consumed one of the two |
//! | `renice -h` | the usage on **stdout**, status 0 |
//! | `renice -h -p 1` | `invalid priority '-h'` — `-h` is honoured only when it is the *only* argument |
//! | `renice -v` / `-V` / `--version` | `renice from util-linux 2.39.3`, status 0 |
//! | `renice abc 1` | `invalid priority 'abc'` + referral |
//! | `renice '' 1` | **accepted**, priority 0 — `strtol("")` performs no conversion and leaves `endptr` on the terminator |
//! | `renice ' ' 1` | `invalid priority ' '` — no conversion, and `endptr` is left on the space |
//! | `renice ' 5' $$` | accepted, priority 5 — `strtol` skips leading whitespace |
//! | `renice +5 $$` | accepted, priority 5 |
//! | `renice 0x10 1` | `invalid priority '0x10'` — the base is 10, so the scan stops at `x` |
//! | `renice 99999999999999999999 $$` | accepted: `strtol` saturates to `LONG_MAX`, which truncates to `int` −1 |
//! | `renice 2147483647 $$` | `new priority 19` — the kernel clamps, not us |
//! | `renice 5 abc` | `bad process ID value: abc`, no referral, status 1 |
//! | `renice 5 -g abc` | `bad process group ID value: abc` |
//! | `renice 5 -u abc` | `unknown user abc` |
//! | `renice 0 -1` | `bad process ID value: -1` — a negative target is refused |
//! | `renice 0 4294967296` | renices process **0**: the target is an `int`, and the low 32 bits are zero |
//! | `renice 0 2147483648` | `bad process ID value: 2147483648` — the same truncation, but to a negative |
//! | `renice 0 -g -p 410` | process 410 — the mode words are sticky and the last one wins |
//! | `renice 0 99999999` | `failed to get priority for 99999999 (process ID): No such process` |
//! | `renice 0 410 999999` | reports both, continues past the first, status 1 |
//! | `renice -- 0 $$` | `invalid priority '--'` — `--` is not special here |
//!
//! # Where this deliberately diverges
//!
//! 1. **Upstream's stale-`endptr` bug is fixed, not reproduced.** `endptr` is
//!    one variable reused by every `strtol` in the program, and the
//!    `getpwnam` path never assigns to it — so a *successful* user lookup is
//!    validated against whatever byte an *earlier, unrelated* operand left
//!    behind. Measured: `renice 0 -u root` reaches root, but `renice 0 -p abc
//!    -u root` says `unknown user root`. This is declined for the same reason
//!    `which`'s two upstream bugs were (`B-WHICH-DIVERGES-FROM-GNU-IN-FOUR-
//!    MEASURED-PLACES`): it reports a thing that exists as missing. `cal`'s
//!    reproduced bugs are cosmetic misalignment; this one is a wrong answer.
//!    The rule that draws the line is design decision 623.
//! 2. **Diagnostics always say `renice:`,** never `argv[0]` as typed. House
//!    rule across this coreutils.
//! 3. **The password database is `pwdb`** — `/etc/passwd` and `/etc/group`,
//!    the same files `id` and `chown` read — rather than glibc's NSS.
//! 4. **Operands echoed back in a diagnostic go through
//!    `quote::escape_unprintable`,** so a target name made of arbitrary bytes
//!    cannot smuggle escape sequences into the terminal. Upstream prints the
//!    bytes.
//! 5. **The kernel is reached through a [`Sched`] trait rather than called
//!    directly** from the reporting code, so that the exact wording and
//!    ordering of the success and failure lines is unit-tested against a fake
//!    kernel instead of against whatever the build host happens to permit.

use std::ffi::{OsStr, OsString};
use std::io;
use std::io::Write;
use std::process::ExitCode;

use coreutils::errmsg::strerror;
use coreutils::getopt::{Error, Program};
use coreutils::quote::{escape_unprintable, os_bytes};
use coreutils::stdfd::{self, Stream};

/// `renice`'s name and the status its usage errors carry.
///
/// Upstream's `errtryhelp(EXIT_FAILURE)` and `warnx` + `errs = 1` both end at
/// 1, so there is only ever one number.
const RENICE: Program = Program::new("renice", 1);

/// `PRIO_PROCESS` — the first argument to `getpriority`/`setpriority`.
const PRIO_PROCESS: i32 = 0;
/// `PRIO_PGRP`.
const PRIO_PGRP: i32 = 1;
/// `PRIO_USER`.
const PRIO_USER: i32 = 2;

/// Which kind of thing an operand names.
///
/// Upstream keeps this in an `int which` that indexes the `idtype[]` table,
/// and the two are always used together — the number goes to the syscall and
/// the string goes into the message — which is why they are one type here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Which {
    Process,
    Pgrp,
    User,
}

impl Which {
    /// `idtype[which]`: the words that appear in `%d (%s) old priority …` and
    /// in `bad %s value: %s`.
    const fn label(self) -> &'static str {
        match self {
            Self::Process => "process ID",
            Self::Pgrp => "process group ID",
            Self::User => "user ID",
        }
    }

    /// The `which` argument the syscalls take.
    const fn code(self) -> i32 {
        match self {
            Self::Process => PRIO_PROCESS,
            Self::Pgrp => PRIO_PGRP,
            Self::User => PRIO_USER,
        }
    }
}

/// What a command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// `renice -h`, `renice --help` — and only when it is the sole argument.
    Help,
    /// `renice -v`, `-V`, `--version`, likewise sole.
    Version,
    /// Everything else.
    Run {
        /// Whether the priority is added to the target's current one.
        relative: bool,
        /// The priority, already truncated to `int` the way upstream's
        /// `int prio = strtol (…)` truncates it.
        priority: i32,
        /// The operands, mode words included — they are interpreted in the
        /// loop, not here, because upstream's `-p`/`-g`/`-u` are positional
        /// and may repeat.
        targets: Vec<OsString>,
    },
}

// ------------------------------------------------------------- strtol ----

/// C's `strtol(nptr, &endptr, 10)`: the value, and how many bytes it consumed.
///
/// This is not `str::parse`, and every difference below is observable in
/// `renice`'s behaviour. Upstream never checks `errno`, so an out-of-range
/// number is not an error — it is `LONG_MAX`/`LONG_MIN` on its way to being
/// truncated to `int`.
///
/// * Leading whitespace is skipped, so `renice ' 5' $$` is priority 5.
/// * A single `+` or `-` sign is accepted.
/// * The base is 10, so the scan stops at the `x` of `0x10`.
/// * **If no digit is found, nothing is consumed** — `endptr` is left equal
///   to `nptr`. That is what makes the empty string succeed (`endptr` is
///   already on the terminator) while a lone space fails.
///
/// The returned length is what upstream tests as `*endptr`: the argument is
/// wholly consumed exactly when the length equals the input's, because an
/// `argv` string cannot contain an interior NUL.
fn strtol(bytes: &[u8]) -> (i64, usize) {
    let mut at = 0usize;
    while bytes
        .get(at)
        .is_some_and(|c| c.is_ascii_whitespace() || *c == 0x0b)
    {
        at = at.saturating_add(1);
    }
    let negative = match bytes.get(at) {
        Some(b'-') => {
            at = at.saturating_add(1);
            true
        }
        Some(b'+') => {
            at = at.saturating_add(1);
            false
        }
        _ => false,
    };

    let start = at;
    let mut value: i64 = 0;
    let mut saturated = false;
    while let Some(&c) = bytes.get(at) {
        if !c.is_ascii_digit() {
            break;
        }
        let digit = i64::from(c.wrapping_sub(b'0'));
        if !saturated {
            match value
                .checked_mul(10)
                .and_then(|v| v.checked_add(if negative { -digit } else { digit }))
            {
                Some(v) => value = v,
                None => saturated = true,
            }
        }
        at = at.saturating_add(1);
    }

    if at == start {
        // No conversion performed: `endptr = nptr`, so *nothing* is consumed —
        // not even the whitespace and sign that were scanned past.
        return (0, 0);
    }
    if saturated {
        value = if negative { i64::MIN } else { i64::MAX };
    }
    (value, at)
}

/// `strtol` plus upstream's `*endptr` test, as one answer.
///
/// `None` means a trailing byte was left over, which is the only thing
/// upstream treats as a bad number.
fn strtol_whole(bytes: &[u8]) -> Option<i64> {
    let (value, consumed) = strtol(bytes);
    (consumed == bytes.len()).then_some(value)
}

/// How an operand is shown back to the reader in a diagnostic.
///
/// Upstream is `%s` on the raw bytes; see divergence 4 in the module docs.
fn shown(arg: &OsStr) -> String {
    escape_unprintable(&os_bytes(arg))
}

// -------------------------------------------------------------- parsing ----

/// Walk the command line the way `renice.c`'s `main` does.
///
/// The shape is unusual and is transcribed rather than tidied: `-h` and
/// `--version` are recognised only when they are the *whole* command line, at
/// most one of `-n`/`--relative`/`--priority` is consumed and only in first
/// position, and the priority is a positional word rather than an option
/// argument. Everything after it is left for the operand loop, because
/// `-p`/`-g`/`-u` are sticky mode switches that may appear anywhere and any
/// number of times.
///
/// # Errors
///
/// `not enough arguments` or `invalid priority '…'`, both of which upstream
/// ends with `errtryhelp`, so both carry the `Try 'renice --help'` referral.
fn scan(args: &[OsString], posixly_correct: bool) -> Result<Request, Error> {
    if let [only] = args {
        let word = os_bytes(only);
        if word.as_ref() == b"-h" || word.as_ref() == b"--help" {
            return Ok(Request::Help);
        }
        if word.as_ref() == b"-v" || word.as_ref() == b"-V" || word.as_ref() == b"--version" {
            return Ok(Request::Version);
        }
    }

    // Exactly one leading mode word, and only if it is first. `renice -n -n 5
    // $$` therefore reads the second `-n` as the priority and refuses it.
    let (relative, rest) = match args.split_first() {
        // Upstream's comment: "Fully conform to posix only if POSIXLY_CORRECT
        // is set in the environment. If not, use the absolute value as it's
        // been used (incorrectly) since 2009."
        Some((first, tail)) if os_bytes(first).as_ref() == b"-n" => (posixly_correct, tail),
        Some((first, tail)) if os_bytes(first).as_ref() == b"--relative" => (true, tail),
        Some((first, tail)) if os_bytes(first).as_ref() == b"--priority" => (false, tail),
        _ => (false, args),
    };

    // `argc < 2`: a priority *and* at least one target. The count is what is
    // tested, which is why `renice 5` and `renice -n 5` give the same
    // complaint as `renice` with nothing at all.
    let Some((word, targets)) = rest.split_first() else {
        return Err(RENICE.usage_referring("not enough arguments".to_owned()));
    };
    if targets.is_empty() {
        return Err(RENICE.usage_referring("not enough arguments".to_owned()));
    }

    let Some(value) = strtol_whole(&os_bytes(word)) else {
        return Err(RENICE.usage_referring(format!("invalid priority '{}'", shown(word))));
    };

    Ok(Request::Run {
        relative,
        // `int prio = strtol (…)`. The truncation is load-bearing: it is why
        // a priority of 99999999999999999999 becomes −1.
        #[expect(
            clippy::cast_possible_truncation,
            reason = "upstream stores a `long` in an `int`; the wrap is the measured behaviour"
        )]
        priority: value as i32,
        targets: targets.to_vec(),
    })
}

/// What one operand turned out to mean.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Target {
    /// A mode word: `-p`, `-g`, `-u` and their long spellings.
    Mode(Which),
    /// A thing to renice.
    Id(i32),
    /// A word that named nothing: `bad process ID value: …` or `unknown user
    /// …`, already worded for the current mode.
    Bad(String),
}

/// Read one operand under the current mode — upstream's loop body.
///
/// `lookup` is the password database, consulted only under `-u` and only
/// before the number is tried, which is what makes an account genuinely named
/// `1000` reachable.
fn read_target(word: &OsStr, which: Which, lookup: &dyn Fn(&[u8]) -> Option<u32>) -> Target {
    let bytes = os_bytes(word);
    match bytes.as_ref() {
        b"-g" | b"--pgrp" => return Target::Mode(Which::Pgrp),
        b"-u" | b"--user" => return Target::Mode(Which::User),
        b"-p" | b"--pid" => return Target::Mode(Which::Process),
        _ => {}
    }

    if which == Which::User
        && let Some(uid) = lookup(&bytes)
    {
        // Upstream reaches `if (who < 0 || *endptr)` here without having
        // assigned `endptr`, which is the bug documented as divergence 1. We
        // test only what this branch actually produced.
        #[expect(
            clippy::cast_possible_wrap,
            reason = "`who` is an `int` upstream; a uid above INT_MAX wraps there too"
        )]
        let who = uid as i32;
        return if who < 0 {
            Target::Bad(format!("unknown user {}", shown(word)))
        } else {
            Target::Id(who)
        };
    }

    #[expect(
        clippy::cast_possible_truncation,
        reason = "upstream stores a `long` in an `int`; `renice 0 4294967296` renices process 0"
    )]
    let who = strtol_whole(&bytes).map(|v| v as i32);
    match who {
        Some(who) if who >= 0 => Target::Id(who),
        _ if which == Which::User => Target::Bad(format!("unknown user {}", shown(word))),
        _ => Target::Bad(format!("bad {} value: {}", which.label(), shown(word))),
    }
}

// ------------------------------------------------------------- the work ----

/// The two syscalls `donice` makes, behind a seam.
///
/// Not for portability — the shipped target is Linux and calls straight
/// through — but so that the exact text and ordering of the report is tested
/// against a kernel the test controls. See divergence 5.
trait Sched {
    /// `getpriority(which, who)`, with the `errno` dance already done.
    ///
    /// # Errors
    ///
    /// Whatever `getpriority` reported. A priority of −1 with `errno`
    /// untouched is a *value*, not a failure, and must come back as `Ok(-1)`.
    fn get(&self, which: i32, who: u32) -> io::Result<i32>;

    /// `setpriority(which, who, prio)`.
    ///
    /// # Errors
    ///
    /// Whatever `setpriority` reported.
    fn set(&mut self, which: i32, who: u32, prio: i32) -> io::Result<()>;
}

/// Upstream's `donice`: read, set, read back, report. Returns its `errs` bit.
///
/// The second read is not redundant — the kernel clamps, so it is the only way
/// the printed "new priority" can be the priority the process actually has.
/// That is why `renice 2147483647 $$` reports 19.
fn donice<O: Write, E: Write>(
    sched: &mut dyn Sched,
    out: &mut O,
    err: &mut E,
    which: Which,
    who: i32,
    prio: i32,
    relative: bool,
) -> u8 {
    #[expect(
        clippy::cast_sign_loss,
        reason = "`who` is passed to an `id_t` parameter; upstream's int→unsigned conversion"
    )]
    let id = who as u32;

    let old = match sched.get(which.code(), id) {
        Ok(v) => v,
        Err(e) => return report_syscall(err, "get", which, who, &e),
    };

    // `newprio = oldprio + prio` — plain `int` addition, which upstream lets
    // overflow. Wrapping rather than saturating keeps the same answer.
    let new = if relative {
        old.wrapping_add(prio)
    } else {
        prio
    };

    if let Err(e) = sched.set(which.code(), id, new) {
        return report_syscall(err, "set", which, who, &e);
    }

    let settled = match sched.get(which.code(), id) {
        Ok(v) => v,
        Err(e) => return report_syscall(err, "get", which, who, &e),
    };

    let _ = writeln!(
        out,
        "{who} ({}) old priority {old}, new priority {settled}",
        which.label()
    );
    0
}

/// `warn(_("failed to %s priority for %d (%s)"))` — the `warn` family, so the
/// sentence carries `errno`'s text.
fn report_syscall<E: Write>(err: &mut E, verb: &str, which: Which, who: i32, e: &io::Error) -> u8 {
    let _ = writeln!(
        err,
        "renice: failed to {verb} priority for {who} ({}): {}",
        which.label(),
        strerror(e)
    );
    1
}

/// `warnx` — one line, no `errno`, no referral.
fn report_plain<E: Write>(err: &mut E, sentence: &str) -> u8 {
    let _ = writeln!(err, "renice: {sentence}");
    1
}

/// The operand loop.
fn run<O: Write, E: Write>(
    request_relative: bool,
    priority: i32,
    targets: &[OsString],
    sched: &mut dyn Sched,
    lookup: &dyn Fn(&[u8]) -> Option<u32>,
    out: &mut O,
    err: &mut E,
) -> u8 {
    let mut which = Which::Process;
    let mut errs = 0u8;

    for word in targets {
        match read_target(word, which, lookup) {
            Target::Mode(mode) => which = mode,
            Target::Bad(sentence) => errs |= report_plain(err, &sentence),
            Target::Id(who) => {
                errs |= donice(sched, out, err, which, who, priority, request_relative);
            }
        }
    }

    errs
}

// ---------------------------------------------------------------- output ----

/// `renice --help`, byte for byte, from util-linux 2.39.3.
fn help_text() -> &'static str {
    concat!(
        "\n",
        "Usage:\n",
        " renice [-n|--priority|--relative] <priority> [-p|--pid] <pid>...\n",
        " renice [-n|--priority|--relative] <priority>  -g|--pgrp <pgid>...\n",
        " renice [-n|--priority|--relative] <priority>  -u|--user <user>...\n",
        "\n",
        "Alter the priority of running processes.\n",
        "\n",
        "Options:\n",
        " -n <num>               specify the nice value\n",
        "                          If POSIXLY_CORRECT flag is set in environment\n",
        "                          then the priority is 'relative' to current\n",
        "                          process priority. Otherwise it is 'absolute'.\n",
        " --priority <num>       specify the 'absolute' nice value\n",
        " --relative <num>       specify the 'relative' nice value\n",
        " -p, --pid              interpret arguments as process ID (default)\n",
        " -g, --pgrp             interpret arguments as process group ID\n",
        " -u, --user             interpret arguments as username or user ID\n",
        "\n",
        " -h, --help             display this help\n",
        " -V, --version          display version\n",
        "\n",
        "For more details see renice(1).\n",
    )
}

/// util-linux's `print_version` shape, with our own provenance.
///
/// The `NAME from PACKAGE VERSION` form is kept because that is what scripts
/// grep for on a util-linux tool; claiming to *be* util-linux would not be.
/// See `B-CAL-IS-NARROWER-THAN-UPSTREAM-IN-FIVE-PLACES`, which made the same
/// call for `cal`.
fn version_text() -> &'static str {
    "renice from SlateOS coreutils 0.1.0\n"
}

// ------------------------------------------------------------------ main ----

fn main() -> ExitCode {
    coreutils::guard_std_fds!();
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();

    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let posixly_correct = std::env::var_os("POSIXLY_CORRECT").is_some();

    let request = match scan(&args, posixly_correct) {
        Ok(request) => request,
        Err(e) => {
            RENICE.report(&e);
            return ExitCode::from(u8::try_from(e.status).unwrap_or(1));
        }
    };

    let mut out = Stream::stdout();
    let mut err = Stream::stderr();

    let status = match request {
        // `usage()` writes to stdout and exits 0; `print_version` likewise.
        Request::Help => {
            let _ = out.write_all(help_text().as_bytes());
            0
        }
        Request::Version => {
            let _ = out.write_all(version_text().as_bytes());
            0
        }
        Request::Run {
            relative,
            priority,
            targets,
        } => {
            let db = pwdb::Db::load();
            let lookup = |name: &[u8]| db.user_by_name(name).map(|u| u.uid);
            let mut sched = imp::Kernel;
            run(
                relative, priority, &targets, &mut sched, &lookup, &mut out, &mut err,
            )
        }
    };

    // `close_stdout_atexit()`: a success line that could not be written is not
    // a success.
    stdfd::close_stdout("renice", out, ExitCode::from(status))
}

// ------------------------------------------------------------------ unix ----

/// The real kernel.
///
/// The shipped target (`toolchain/x86_64-slateos.json`) is `"os": "linux"`, so
/// this is the arm that ships; the Windows development host gets the stub
/// below. Everything above this line is unit-tested on both.
#[cfg(target_os = "linux")]
mod imp {
    use std::io;

    unsafe extern "C" {
        fn getpriority(which: i32, who: u32) -> i32;
        fn setpriority(which: i32, who: u32, prio: i32) -> i32;
        fn __errno_location() -> *mut i32;
    }

    fn errno_slot() -> *mut i32 {
        // SAFETY: `__errno_location` is defined to return a valid pointer to
        // this thread's `errno` and never fails.
        unsafe { __errno_location() }
    }

    fn clear_errno() {
        // SAFETY: the pointer is this thread's `errno`, a live `int` for the
        // whole life of the thread.
        unsafe { *errno_slot() = 0 };
    }

    fn errno() -> i32 {
        // SAFETY: as above.
        unsafe { *errno_slot() }
    }

    /// Calls straight through, with upstream's `errno` dance.
    pub struct Kernel;

    impl super::Sched for Kernel {
        fn get(&self, which: i32, who: u32) -> io::Result<i32> {
            // −1 is a legitimate priority, so upstream's `getprio` clears
            // `errno` first and reads it after; that is the only way to tell
            // the value from the failure.
            clear_errno();
            // SAFETY: `getpriority` takes no pointers and only reads
            // scheduling state.
            let value = unsafe { getpriority(which, who) };
            let e = errno();
            if value == -1 && e != 0 {
                return Err(io::Error::from_raw_os_error(e));
            }
            Ok(value)
        }

        fn set(&mut self, which: i32, who: u32, prio: i32) -> io::Result<()> {
            clear_errno();
            // SAFETY: `setpriority` takes no pointers and only alters
            // scheduling priority.
            if unsafe { setpriority(which, who, prio) } < 0 {
                return Err(io::Error::from_raw_os_error(errno()));
            }
            Ok(())
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::io;

    pub struct Kernel;

    impl super::Sched for Kernel {
        fn get(&self, _which: i32, _who: u32) -> io::Result<i32> {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }

        fn set(&mut self, _which: i32, _who: u32, _prio: i32) -> io::Result<()> {
            Err(io::Error::from(io::ErrorKind::Unsupported))
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    clippy::panic,
    reason = "panicking on bad data is the point of a test"
)]
mod tests {
    use super::{
        Request, Sched, Target, Which, donice, help_text, read_target, run, scan, strtol,
        strtol_whole,
    };
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::io;

    fn argv(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    /// A kernel the test owns: a table of priorities, and a set of ids that
    /// refuse to be lowered.
    ///
    /// Its failures are named by [`io::ErrorKind`] and not by errno, because
    /// these tests run on the *dev host*, where `from_raw_os_error(3)` is the
    /// Win32 code 3 and not `ESRCH`. `ErrorKind` is std's normalisation across
    /// the two, and it is what `errmsg::strerror` keys on — so a kind is the
    /// only way for a fake to name a failure and get the same sentence on both
    /// hosts. The consequence is that an absent id reports `ENOENT`'s text
    /// rather than `ESRCH`'s; what these tests pin is the *shape* of the
    /// sentence — which call failed, for which id, under which label, with the
    /// system's own text on the end — not the fake's choice of errno.
    struct Fake {
        prio: BTreeMap<(i32, u32), i32>,
        /// The kernel's clamp, applied on `set` the way the real one does.
        clamp: (i32, i32),
        /// Ids for which `set` reports a permission failure.
        refuses: Vec<(i32, u32)>,
    }

    impl Fake {
        fn new(entries: &[((i32, u32), i32)]) -> Self {
            Self {
                prio: entries.iter().copied().collect(),
                clamp: (-20, 19),
                refuses: Vec::new(),
            }
        }
    }

    impl Sched for Fake {
        fn get(&self, which: i32, who: u32) -> io::Result<i32> {
            self.prio
                .get(&(which, who))
                .copied()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
        }

        fn set(&mut self, which: i32, who: u32, prio: i32) -> io::Result<()> {
            if self.refuses.contains(&(which, who)) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            if !self.prio.contains_key(&(which, who)) {
                return Err(io::Error::from(io::ErrorKind::NotFound));
            }
            self.prio
                .insert((which, who), prio.clamp(self.clamp.0, self.clamp.1));
            Ok(())
        }
    }

    /// No password database at all — the common case in these tests.
    fn nobody(_name: &[u8]) -> Option<u32> {
        None
    }

    // ------------------------------------------------------------ strtol ----

    #[test]
    fn a_number_is_read_the_way_strtol_reads_it() {
        // Whitespace and a sign are skipped; the base is 10 throughout.
        assert_eq!(strtol(b" 5"), (5, 2));
        assert_eq!(strtol(b"+5"), (5, 2));
        assert_eq!(strtol(b"-5"), (-5, 2));
        assert_eq!(strtol(b"\t\n\x0b\x0c\r7"), (7, 6));
        assert_eq!(strtol(b"007"), (7, 3));
        // Base 10 stops at the `x`, leaving `x10` behind.
        assert_eq!(strtol(b"0x10"), (0, 1));
        // No digit means *nothing* is consumed — not even the sign that was
        // scanned past. This is what makes "" succeed and " " fail.
        assert_eq!(strtol(b""), (0, 0));
        assert_eq!(strtol(b" "), (0, 0));
        assert_eq!(strtol(b"+"), (0, 0));
        assert_eq!(strtol(b"abc"), (0, 0));
        // Out of range saturates rather than failing; upstream never looks at
        // `errno`, so this is a value like any other.
        assert_eq!(strtol(b"99999999999999999999"), (i64::MAX, 20));
        assert_eq!(strtol(b"-99999999999999999999"), (i64::MIN, 21));
        assert_eq!(strtol(b"9223372036854775807"), (i64::MAX, 19));
        assert_eq!(strtol(b"-9223372036854775808"), (i64::MIN, 20));
    }

    #[test]
    fn the_whole_word_test_is_the_terminator_not_the_length_scanned() {
        assert_eq!(strtol_whole(b""), Some(0));
        assert_eq!(strtol_whole(b"5"), Some(5));
        assert_eq!(strtol_whole(b" 5"), Some(5));
        assert_eq!(strtol_whole(b" "), None);
        assert_eq!(strtol_whole(b"5x"), None);
        assert_eq!(strtol_whole(b"5 "), None);
        assert_eq!(strtol_whole(b"0x10"), None);
    }

    // -------------------------------------------------------------- scan ----

    #[test]
    fn help_and_version_are_honoured_only_as_the_whole_command_line() {
        for word in ["-h", "--help"] {
            assert_eq!(scan(&argv(&[word]), false).unwrap(), Request::Help);
        }
        for word in ["-v", "-V", "--version"] {
            assert_eq!(scan(&argv(&[word]), false).unwrap(), Request::Version);
        }
        // Measured: `renice -h -p 1` reads `-h` as the priority and refuses it.
        let e = scan(&argv(&["-h", "-p", "1"]), false).unwrap_err();
        assert_eq!(e.sentence, "invalid priority '-h'");
        let e = scan(&argv(&["--help", "extra"]), false).unwrap_err();
        assert_eq!(e.sentence, "invalid priority '--help'");
    }

    #[test]
    fn not_enough_arguments_counts_words_rather_than_naming_what_is_missing() {
        for line in [
            vec![],
            vec!["5"],
            vec!["-n"],
            vec!["-n", "5"],
            vec!["--relative"],
            vec!["--priority"],
        ] {
            let e = scan(&argv(&line), false).unwrap_err();
            assert_eq!(e.sentence, "not enough arguments", "for {line:?}");
            assert!(e.referral.is_some(), "for {line:?}");
            assert_eq!(e.status, 1);
        }
    }

    #[test]
    fn a_bad_priority_refers_the_reader_to_the_help() {
        let e = scan(&argv(&["abc", "1"]), false).unwrap_err();
        assert_eq!(e.sentence, "invalid priority 'abc'");
        assert_eq!(
            e.message(),
            "invalid priority 'abc'\nTry 'renice --help' for more information."
        );
        // `--` is not special here — upstream never calls getopt.
        assert_eq!(
            scan(&argv(&["--", "0", "1"]), false).unwrap_err().sentence,
            "invalid priority '--'"
        );
        // But an empty priority is *accepted*, because `strtol` leaves
        // `endptr` on the terminator when it converts nothing.
        assert!(matches!(
            scan(&argv(&["", "1"]), false).unwrap(),
            Request::Run { priority: 0, .. }
        ));
    }

    #[test]
    fn dash_n_is_absolute_unless_posixly_correct_says_otherwise() {
        let relative_of = |line: &[&str], pc: bool| match scan(&argv(line), pc).unwrap() {
            Request::Run { relative, .. } => relative,
            other => panic!("{other:?}"),
        };
        assert!(!relative_of(&["-n", "5", "1"], false));
        assert!(relative_of(&["-n", "5", "1"], true));
        // The long spellings say so outright and ignore the environment.
        assert!(relative_of(&["--relative", "5", "1"], false));
        assert!(!relative_of(&["--priority", "5", "1"], true));
        // A bare priority is absolute either way.
        assert!(!relative_of(&["5", "1"], true));
    }

    #[test]
    fn only_the_first_word_can_be_a_mode_and_only_one_of_them() {
        // The second `-n` is read as the priority.
        assert_eq!(
            scan(&argv(&["-n", "-n", "5", "1"]), false)
                .unwrap_err()
                .sentence,
            "invalid priority '-n'"
        );
        // `--relative` after the priority is an *operand*, and lands in the
        // loop as a bad process ID rather than changing the mode.
        let Request::Run { targets, .. } = scan(&argv(&["5", "--relative", "1"]), false).unwrap()
        else {
            panic!("expected a run")
        };
        assert_eq!(targets, argv(&["--relative", "1"]));
    }

    #[test]
    fn a_priority_too_large_for_an_int_wraps_rather_than_failing() {
        let priority_of = |word: &str| match scan(&argv(&[word, "1"]), false).unwrap() {
            Request::Run { priority, .. } => priority,
            other => panic!("{other:?}"),
        };
        // strtol saturates to LONG_MAX, whose low 32 bits are all ones.
        assert_eq!(priority_of("99999999999999999999"), -1);
        assert_eq!(priority_of("4294967296"), 0);
        assert_eq!(priority_of("2147483648"), i32::MIN);
        assert_eq!(priority_of("+5"), 5);
        assert_eq!(priority_of(" 5"), 5);
    }

    // ------------------------------------------------------- read_target ----

    #[test]
    fn the_mode_words_are_sticky_and_may_repeat() {
        for (word, mode) in [
            ("-p", Which::Process),
            ("--pid", Which::Process),
            ("-g", Which::Pgrp),
            ("--pgrp", Which::Pgrp),
            ("-u", Which::User),
            ("--user", Which::User),
        ] {
            assert_eq!(
                read_target(&OsString::from(word), Which::Process, &nobody),
                Target::Mode(mode)
            );
        }
    }

    #[test]
    fn a_bad_target_is_worded_for_the_mode_it_was_read_under() {
        assert_eq!(
            read_target(&OsString::from("abc"), Which::Process, &nobody),
            Target::Bad("bad process ID value: abc".to_owned())
        );
        assert_eq!(
            read_target(&OsString::from("abc"), Which::Pgrp, &nobody),
            Target::Bad("bad process group ID value: abc".to_owned())
        );
        // Under `-u` the sentence is different, not merely relabelled.
        assert_eq!(
            read_target(&OsString::from("abc"), Which::User, &nobody),
            Target::Bad("unknown user abc".to_owned())
        );
        // A negative target is refused before it reaches the kernel.
        assert_eq!(
            read_target(&OsString::from("-1"), Which::Process, &nobody),
            Target::Bad("bad process ID value: -1".to_owned())
        );
        assert_eq!(
            read_target(&OsString::from("-1"), Which::User, &nobody),
            Target::Bad("unknown user -1".to_owned())
        );
    }

    #[test]
    fn a_target_too_large_for_an_int_wraps_the_way_the_priority_does() {
        // Measured: `renice 0 4294967296` renices process 0.
        assert_eq!(
            read_target(&OsString::from("4294967296"), Which::Process, &nobody),
            Target::Id(0)
        );
        // …but 2147483648 wraps to a negative and is refused, with the
        // *typed* word in the message rather than the wrapped number.
        assert_eq!(
            read_target(&OsString::from("2147483648"), Which::Process, &nobody),
            Target::Bad("bad process ID value: 2147483648".to_owned())
        );
        assert_eq!(
            read_target(
                &OsString::from("99999999999999999999"),
                Which::Process,
                &nobody
            ),
            Target::Bad("bad process ID value: 99999999999999999999".to_owned())
        );
    }

    #[test]
    fn a_name_is_looked_up_before_it_is_read_as_a_number() {
        let db = |name: &[u8]| match name {
            b"alice" => Some(1000),
            // An account genuinely called `1000`, with a different uid — the
            // case that proves the lookup happens first.
            b"1000" => Some(4242),
            _ => None,
        };
        assert_eq!(
            read_target(&OsString::from("alice"), Which::User, &db),
            Target::Id(1000)
        );
        assert_eq!(
            read_target(&OsString::from("1000"), Which::User, &db),
            Target::Id(4242)
        );
        // A number that names no account still works.
        assert_eq!(
            read_target(&OsString::from("7"), Which::User, &db),
            Target::Id(7)
        );
        // And the database is consulted *only* under `-u`.
        assert_eq!(
            read_target(&OsString::from("alice"), Which::Process, &db),
            Target::Bad("bad process ID value: alice".to_owned())
        );
    }

    #[test]
    fn a_successful_lookup_is_not_poisoned_by_an_earlier_bad_operand() {
        // Upstream says `unknown user root` here, because its single `endptr`
        // still points into `abc`. Divergence 1 in the module docs: we do not
        // reproduce it, because it reports an account that exists as missing.
        let db = |name: &[u8]| (name == b"root").then_some(0);
        assert_eq!(
            read_target(&OsString::from("abc"), Which::Process, &db),
            Target::Bad("bad process ID value: abc".to_owned())
        );
        assert_eq!(
            read_target(&OsString::from("root"), Which::User, &db),
            Target::Id(0)
        );
    }

    #[test]
    fn an_operand_of_arbitrary_bytes_is_escaped_rather_than_echoed() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt as _;
            let word = OsString::from_vec(b"a\nb\xff".to_vec());
            let Target::Bad(sentence) = read_target(&word, Which::Process, &nobody) else {
                panic!("expected a refusal")
            };
            assert!(!sentence.contains('\n'), "{sentence}");
            assert!(
                sentence.starts_with("bad process ID value: a"),
                "{sentence}"
            );
        }
    }

    // --------------------------------------------------------- the report ----

    fn captured(f: impl FnOnce(&mut Vec<u8>, &mut Vec<u8>) -> u8) -> (u8, String, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = f(&mut out, &mut err);
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_success_names_the_target_its_kind_and_both_priorities() {
        let mut sched = Fake::new(&[((0, 410), 0)]);
        let (status, out, err) =
            captured(|out, err| donice(&mut sched, out, err, Which::Process, 410, 5, false));
        assert_eq!(status, 0);
        assert_eq!(out, "410 (process ID) old priority 0, new priority 5\n");
        assert_eq!(err, "");
    }

    #[test]
    fn the_reported_new_priority_is_the_one_the_kernel_settled_on() {
        // Measured: `renice 2147483647 $$` prints `new priority 19`. The
        // second read is what makes that true rather than a lie.
        let mut sched = Fake::new(&[((0, 410), 0)]);
        let (status, out, _) =
            captured(|out, err| donice(&mut sched, out, err, Which::Process, 410, i32::MAX, false));
        assert_eq!(status, 0);
        assert_eq!(out, "410 (process ID) old priority 0, new priority 19\n");
    }

    #[test]
    fn a_relative_priority_is_added_to_the_one_already_there() {
        let mut sched = Fake::new(&[((0, 410), 5)]);
        let (_, out, _) =
            captured(|out, err| donice(&mut sched, out, err, Which::Process, 410, 1, true));
        assert_eq!(out, "410 (process ID) old priority 5, new priority 6\n");
    }

    #[test]
    fn each_syscall_failure_names_which_call_failed_and_carries_errno() {
        // Absent id: the *first* read fails, so nothing is set. (The fake
        // reports `NotFound` rather than `ESRCH` — see [`Fake`].)
        let mut sched = Fake::new(&[]);
        let (status, out, err) =
            captured(|out, err| donice(&mut sched, out, err, Which::Process, 999_999, 0, false));
        assert_eq!(status, 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "renice: failed to get priority for 999999 (process ID): No such file or directory\n"
        );

        // Present but refused: the read succeeds and the *set* fails.
        let mut sched = Fake::new(&[((2, 0), 0)]);
        sched.refuses.push((2, 0));
        let (status, out, err) =
            captured(|out, err| donice(&mut sched, out, err, Which::User, 0, -5, false));
        assert_eq!(status, 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "renice: failed to set priority for 0 (user ID): Permission denied\n"
        );
    }

    #[test]
    fn a_refused_target_does_not_stop_the_ones_after_it() {
        // Measured: `renice 0 410 999999` reports both and exits 1.
        let mut sched = Fake::new(&[((0, 410), 0)]);
        let (status, out, err) = captured(|out, err| {
            run(
                false,
                0,
                &argv(&["410", "999999"]),
                &mut sched,
                &nobody,
                out,
                err,
            )
        });
        assert_eq!(status, 1);
        assert_eq!(out, "410 (process ID) old priority 0, new priority 0\n");
        assert_eq!(
            err,
            "renice: failed to get priority for 999999 (process ID): No such file or directory\n"
        );
    }

    #[test]
    fn a_mode_word_changes_every_target_after_it_and_the_last_one_wins() {
        // Measured: `renice 0 -g -p 410` renices *process* 410.
        let mut sched = Fake::new(&[((0, 410), 0), ((1, 410), 3)]);
        let (status, out, err) = captured(|out, err| {
            run(
                false,
                0,
                &argv(&["-g", "-p", "410"]),
                &mut sched,
                &nobody,
                out,
                err,
            )
        });
        assert_eq!(status, 0);
        assert_eq!(out, "410 (process ID) old priority 0, new priority 0\n");
        assert_eq!(err, "");

        // …and once switched, it stays switched for the rest of the line.
        let mut sched = Fake::new(&[((0, 410), 0), ((1, 7), 3)]);
        let (status, out, _) = captured(|out, err| {
            run(
                false,
                1,
                &argv(&["410", "-g", "7"]),
                &mut sched,
                &nobody,
                out,
                err,
            )
        });
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "410 (process ID) old priority 0, new priority 1\n\
             7 (process group ID) old priority 3, new priority 1\n"
        );
    }

    // ---------------------------------------------------------- the help ----

    #[test]
    fn the_help_is_upstreams_to_the_byte() {
        const UPSTREAM_HELP: &str = concat!(
            "\n",
            "Usage:\n",
            " renice [-n|--priority|--relative] <priority> [-p|--pid] <pid>...\n",
            " renice [-n|--priority|--relative] <priority>  -g|--pgrp <pgid>...\n",
            " renice [-n|--priority|--relative] <priority>  -u|--user <user>...\n",
            "\n",
            "Alter the priority of running processes.\n",
            "\n",
            "Options:\n",
            " -n <num>               specify the nice value\n",
            "                          If POSIXLY_CORRECT flag is set in environment\n",
            "                          then the priority is 'relative' to current\n",
            "                          process priority. Otherwise it is 'absolute'.\n",
            " --priority <num>       specify the 'absolute' nice value\n",
            " --relative <num>       specify the 'relative' nice value\n",
            " -p, --pid              interpret arguments as process ID (default)\n",
            " -g, --pgrp             interpret arguments as process group ID\n",
            " -u, --user             interpret arguments as username or user ID\n",
            "\n",
            " -h, --help             display this help\n",
            " -V, --version          display version\n",
            "\n",
            "For more details see renice(1).\n",
        );
        assert_eq!(help_text(), UPSTREAM_HELP);
        assert_eq!(help_text().len(), 951);
    }
}
