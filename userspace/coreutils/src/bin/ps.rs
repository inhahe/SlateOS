//! ps — report process status.
//!
//! Usage: ps [-e] [-f]
//!   -e  show all processes (not just current session)
//!   -f  full listing format
//!
//! Reads from /proc filesystem. Each directory under /proc/<pid>/
//! contains process information files: stat, cmdline, status.

use coreutils::diag;
use coreutils::stdfd;
use localtime::{Zone, strftime};
use std::env;
use std::io::{self, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct PsArgs {
    all_procs: bool,
    full_format: bool,
    /// `--no-header`: print the rows and not the column titles.
    no_header: bool,
    /// `-p`: show only these PIDs. `None` means "no selection", which is not
    /// the same as an empty list -- an empty selection would match nothing and
    /// `-p` with no valid PID is a syntax error before it gets here.
    select_pids: Option<Vec<u64>>,
    /// `-u`: show only processes with these effective UIDs.
    select_uids: Option<Vec<u32>>,
    /// `-o`: print exactly these columns. Repeated `-o` accumulates rather
    /// than replaces, which is procps' behaviour: `-o pid -o comm` is the
    /// same as `-o pid,comm`.
    columns: Vec<Spec>,
    /// `-t`: show only processes on these terminals, as `format_tty` renders
    /// them. `?` and `-` both mean "no controlling terminal" and both arrive
    /// here as `?`.
    select_ttys: Option<Vec<String>>,
    /// `-l`: the long format.
    long_format: bool,
}

/// Parse ps's argv.  BSD-style and POSIX-style flags are accepted via
/// the same clustered short-flag syntax used by the rest of these
/// utilities.  Unknown short flags are silently ignored, matching the
/// previous behaviour.
/// What the command line asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
enum Request {
    /// Print the table.
    List(PsArgs),
    /// `--help`.
    Help,
}

/// Parse `ps`'s argv.
///
/// # Errors
///
/// An unknown option, short or long. procps distinguishes the two and so does
/// this; the messages are its own, measured.
///
/// # What this used to do, and why a test was protecting it
///
/// The body was one loop with `_ => {}` at the bottom, and a test named
/// `parse_unknown_silently_ignored` asserting that it stayed that way --
/// "Preserves previous behaviour: no error, no panic." It preserved a defect.
/// Measured against procps-ng:
///
/// | | procps | here, before |
/// |---|---|---|
/// | `ps -Q` | `error: unsupported SysV option`, exit 1 | the default table, exit 0 |
/// | `ps --nosuchoption` | `error: unknown gnu long option`, exit 1 | the default table, exit 0 |
/// | `ps --help` | usage, exit 0 | **the process table**, exit 0 |
///
/// The `--help` row is the one that shows how bad the shape was. There was no
/// long-option branch at all: `--help` had its first `-` stripped and the rest
/// was walked a character at a time, so it was read as `-h -e -l -p`, the `e`
/// matched, and `ps --help` turned on "show all processes". Any long option
/// containing `e`, `A` or `f` silently set that flag -- `--full` would have
/// set `-f` by accident and `--version` would have set `-e`.
///
/// `scripts/check-argv-ignored.py` did not catch it and is not wrong to have
/// missed it: that gate finds a program ignoring its command line ENTIRELY,
/// which is the defect `uptime` had. This one read `argv`, honoured `-e` and
/// `-f`, and discarded the rest -- the same hole one notch finer.
///
/// Found by `scripts/ps-diff.sh`.
fn parse_args(args: &[String]) -> Result<Request, String> {
    let mut out = PsArgs::default();
    let mut i = 0;
    while i < args.len() {
        let Some(arg) = args.get(i) else { break };
        i = i.saturating_add(1);

        // Long options are matched WHOLE. Splitting them into characters is
        // what made `--help` mean `-e`.
        if let Some(long) = arg.strip_prefix("--") {
            match long {
                "help" => return Ok(Request::Help),
                "no-header" | "no-heading" => {
                    out.no_header = true;
                    continue;
                }
                _ => return Err("error: unknown gnu long option".to_string()),
            }
        }
        if let Some(flags) = arg.strip_prefix('-') {
            let mut rest = flags.chars();
            while let Some(c) = rest.next() {
                match c {
                    'e' | 'A' => out.all_procs = true,
                    'f' => out.full_format = true,
                    'l' => out.long_format = true,
                    't' => {
                        let glued: String = rest.by_ref().collect();
                        let list = if glued.is_empty() {
                            let next = args.get(i).cloned().unwrap_or_default();
                            i = i.saturating_add(1);
                            next
                        } else {
                            glued
                        };
                        out.select_ttys = Some(parse_tty_list(&list)?);
                    }
                    'o' => {
                        let glued: String = rest.by_ref().collect();
                        let list = if glued.is_empty() {
                            let next = args.get(i).cloned().unwrap_or_default();
                            i = i.saturating_add(1);
                            next
                        } else {
                            glued
                        };
                        parse_columns(&list, &mut out.columns)?;
                    }
                    'u' => {
                        let glued: String = rest.by_ref().collect();
                        let list = if glued.is_empty() {
                            let next = args.get(i).cloned().unwrap_or_default();
                            i = i.saturating_add(1);
                            next
                        } else {
                            glued
                        };
                        out.select_uids = Some(parse_user_list(&list)?);
                    }
                    'p' => {
                        // `-p` takes a list, and it may be glued on (`-p1`) or
                        // be the next argument (`-p 1`). procps accepts both.
                        let glued: String = rest.by_ref().collect();
                        let list = if glued.is_empty() {
                            let next = args.get(i).cloned().unwrap_or_default();
                            i = i.saturating_add(1);
                            next
                        } else {
                            glued
                        };
                        out.select_pids = Some(parse_pid_list(&list)?);
                    }
                    _ => return Err("error: unsupported SysV option".to_string()),
                }
            }
        }
        // A bare operand is still ignored: procps takes PID lists in that
        // position and this build does not implement them, so refusing here
        // would reject a command line procps accepts. Unchanged, and still
        // covered by `parse_bare_args_ignored`.
    }
    Ok(Request::List(out))
}

/// `-t`'s argument: terminal names, separated by commas or spaces.
///
/// # Errors
///
/// A name with no device behind it. procps' message, measured:
/// `ps -t nosuchtty` prints `error: TTY could not be found` and exits 1,
/// while `ps -t pts/0` is ACCEPTED even when no process is on it -- it simply
/// matches nothing and exits 1 through the no-match path. So the test is
/// whether the terminal exists, not whether anything is using it, which is
/// why this asks the filesystem rather than the process table.
///
/// `?` and `-` both name the absence of a terminal and are not looked up.
fn parse_tty_list(list: &str) -> Result<Vec<String>, String> {
    parse_tty_list_with(list, &|name| {
        std::path::Path::new("/dev").join(name).exists()
    })
}

/// The half of `parse_tty_list` that does not touch the filesystem.
///
/// Split BEFORE writing the test rather than after one failed, because the
/// obvious test -- `-t pts/0` is accepted -- is a statement about the machine.
/// There is no `/dev` on the Windows host these tests run on, so every name
/// would be "could not be found" and the assertion would hold in exactly the
/// environments where it proves nothing. `-u root` taught this the other way
/// round, by going red after it was written.
fn parse_tty_list_with(list: &str, exists: &dyn Fn(&str) -> bool) -> Result<Vec<String>, String> {
    let missing = || "error: TTY could not be found".to_string();
    let mut out = Vec::new();
    for field in list.split([',', ' ']).filter(|f| !f.is_empty()) {
        if field == "?" || field == "-" {
            out.push("?".to_string());
            continue;
        }
        // `/dev/pts/0` for `pts/0`, `/dev/tty1` for `tty1`. procps accepts
        // both the bare name and a `/dev/`-prefixed one.
        let bare = field.strip_prefix("/dev/").unwrap_or(field);
        if !exists(bare) {
            return Err(missing());
        }
        out.push(bare.to_string());
    }
    if out.is_empty() {
        return Err(missing());
    }
    Ok(out)
}

/// `-u`'s argument: user names or numeric UIDs, separated by commas or spaces.
///
/// # Errors
///
/// A name with no passwd entry. Numbers are NOT checked for existence, which
/// is measured rather than assumed: `ps -u 99999` does not complain, it
/// selects nothing and exits 1 through the no-match path. So a number is a
/// UID, and only a name can fail to resolve.
fn parse_user_list(list: &str) -> Result<Vec<u32>, String> {
    // Only a NAME needs the passwd file, so `-u 0` must not open it. The
    // pre-scan is what keeps that true while still handing the database in as
    // a plain parameter -- the first attempt passed a closure that returned
    // `&Db` on demand, which the borrow checker refuses outright because the
    // reference escapes the `FnMut` body.
    let db = list
        .split([',', ' '])
        .filter(|f| !f.is_empty())
        .any(|f| f.parse::<u32>().is_err())
        .then(pwdb::Db::load);
    parse_user_list_with(list, db.as_ref())
}

/// The half of `parse_user_list` that does not read the filesystem.
///
/// Split out because the obvious unit test -- `-u root` resolves to 0 -- is
/// not a test of this code. It passes under WSL and FAILED on the Windows
/// host, where `Db::load` finds no `/etc/passwd`, so every name is "does not
/// exist". The assertion was about the machine wearing the shape of one about
/// the parser, and it would have passed in every environment where the answer
/// did not matter.
///
/// `None` means the caller decided no name was present and did not open the
/// passwd file; a name reaching here with `None` in hand is "does not exist",
/// which is the same answer an empty database would give.
fn parse_user_list_with(list: &str, db: Option<&pwdb::Db>) -> Result<Vec<u32>, String> {
    let missing = || "error: user name does not exist".to_string();
    let mut out = Vec::new();
    for field in list.split([',', ' ']).filter(|f| !f.is_empty()) {
        if let Ok(uid) = field.parse::<u32>() {
            out.push(uid);
            continue;
        }
        let user = db
            .and_then(|d| d.user_by_name(field.as_bytes()))
            .ok_or_else(missing)?;
        out.push(user.uid);
    }
    if out.is_empty() {
        return Err(missing());
    }
    Ok(out)
}

/// `-p`'s argument: PIDs separated by commas or spaces.
///
/// # Errors
///
/// Anything that is not a number, and the empty list. procps' message,
/// measured: `ps -p notanumber` prints `error: process ID list syntax error`
/// and exits 1 -- which is a DIFFERENT complaint from the one it makes about
/// an unknown option, and the difference is the whole reason `-p` is parsed
/// here rather than rejected earlier.
fn parse_pid_list(list: &str) -> Result<Vec<u64>, String> {
    let syntax = || "error: process ID list syntax error".to_string();
    let mut out = Vec::new();
    for field in list.split([',', ' ']).filter(|f| !f.is_empty()) {
        out.push(field.parse::<u64>().map_err(|_| syntax())?);
    }
    if out.is_empty() {
        return Err(syntax());
    }
    Ok(out)
}

/// One column `-o` can name.
///
/// Every width here was measured from procps, one column at a time, because
/// none of them follows from the column's name and two are actively
/// surprising: `comm`'s title is **COMMAND**, not COMM, and `tty`'s is **TT**,
/// not TTY -- the same field that the default format heads `TTY`.
///
/// The widths are FIXED, not sized to content. `ps -e -o user,pid` on a host
/// whose only user is `root` still pads USER to eight, and `comm` to fifteen,
/// which is the kernel's own cap on a task name.
struct Column {
    /// What `-o` calls it.
    name: &'static str,
    /// The heading, when the user does not supply one.
    title: &'static str,
    width: usize,
    right: bool,
}

const COLUMNS: &[Column] = &[
    Column {
        name: "pid",
        title: "PID",
        width: 7,
        right: true,
    },
    Column {
        name: "ppid",
        title: "PPID",
        width: 7,
        right: true,
    },
    Column {
        name: "uid",
        title: "UID",
        width: 5,
        right: true,
    },
    Column {
        name: "user",
        title: "USER",
        width: 8,
        right: false,
    },
    Column {
        name: "comm",
        title: "COMMAND",
        width: 15,
        right: false,
    },
    // `args` is the full command line and is always last in practice, so its
    // width never shows. Zero rather than a guess: an invented width would be
    // wrong the first time someone puts a column after it.
    Column {
        name: "args",
        title: "COMMAND",
        width: 0,
        right: false,
    },
    Column {
        name: "tty",
        title: "TT",
        width: 8,
        right: false,
    },
    Column {
        name: "time",
        title: "TIME",
        width: 8,
        right: true,
    },
    Column {
        name: "stime",
        title: "STIME",
        width: 5,
        right: true,
    },
    Column {
        name: "c",
        title: "C",
        width: 2,
        right: true,
    },
];

/// A column the caller asked for, with the heading they asked for.
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct Spec {
    /// Index into `COLUMNS`.
    col: usize,
    /// The heading. Empty when the spec ended in `=` with nothing after it,
    /// which procps uses to suppress that column's title.
    title: String,
}

/// Parse one `-o` argument: a comma-separated list of `name` or `name=TITLE`.
///
/// # Errors
///
/// A name not in `COLUMNS`. procps' message, measured:
/// `error: unknown user-defined format specifier "nosuchcolumn"`.
fn parse_columns(list: &str, into: &mut Vec<Spec>) -> Result<(), String> {
    for field in list.split(',').filter(|f| !f.is_empty()) {
        let (name, title) = match field.split_once('=') {
            Some((n, t)) => (n, Some(t.to_string())),
            None => (field, None),
        };
        let Some(col) = COLUMNS.iter().position(|c| c.name == name) else {
            return Err(format!(
                "error: unknown user-defined format specifier {name:?}"
            ));
        };
        let title = title.unwrap_or_else(|| COLUMNS[col].title.to_string());
        into.push(Spec { col, title });
    }
    Ok(())
}

/// Join one row's cells the way procps does.
///
/// **Every field is padded to its width EXCEPT THE LAST, which is emitted as
/// it is.** That single rule accounts for all of this, measured byte for byte:
///
/// ```text
/// ps -o user        "USER\nroot\n"          no padding at all
/// ps -o user,pid    "USER     " + "    PID"  USER padded to 8
/// ps -o comm        "COMMAND\nps\n"         no padding
/// ps -o pid,comm=   "    PID \n      1 ps"   header ends at the SEPARATOR
/// ```
///
/// The last line is the one that pins the rule down. `comm=` has an empty
/// title, so the header's final field is the empty string -- unpadded, which
/// leaves the line ending in the separator that precedes it. Trimming the
/// whole line instead would have eaten that space, and padding the last field
/// would have added fourteen more.
fn render_row(cells: &[String], specs: &[Spec]) -> String {
    let mut out = String::new();
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let last = i + 1 == cells.len();
        let Some(spec) = specs.get(i) else { continue };
        let Some(col) = COLUMNS.get(spec.col) else {
            continue;
        };
        // A RIGHT-aligned column is padded even when it is LAST, because its
        // padding lands on the left and is therefore not trailing whitespace.
        // `ps -o pid` prints "    PID" over "      1", not "PID" over "1".
        // Only a left-aligned final column drops its padding, which is what
        // makes `ps -o user` print a bare "USER".
        //
        // The first version of this treated "last" as "unpadded" for both, and
        // the harness put six cases against it: -o pid, -o pid=MYPID, -o time,
        // -o c, -o user,pid and -o uid,pid.
        if last && !col.right {
            out.push_str(cell);
        } else if col.right {
            out.push_str(&format!("{cell:>width$}", width = col.width));
        } else {
            out.push_str(&format!("{cell:<width$}", width = col.width));
        }
    }
    out
}

/// procps-ng's `ps --help` with no topic, byte for byte.
///
/// Captured with `ps --help | cat -A`. The leading blank line is procps' and
/// so is the trailing `For more details see ps(1).` -- `uptime`'s help text
/// was missing exactly those two things for exactly the same reason, which is
/// that they are invisible when you retype a help message instead of
/// measuring it.
fn help_text() -> &'static str {
    "
Usage:
 ps [options]

 Try 'ps --help <simple|list|output|threads|misc|all>'
  or 'ps --help <s|l|o|t|m|a>'
 for additional help text.

For more details see ps(1).
"
}

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct ProcInfo {
    comm: String,
    ppid: u32,
    /// The raw effective UID. It left this struct when the `STAT` column did
    /// and came back for `-u`, which selects on the number rather than the
    /// name -- `ps -u 0` and `ps -u root` must pick the same processes.
    uid: u32,
    /// The UID resolved through `/etc/passwd`, or the number if it does not
    /// resolve. procps prints `root`, not `0`.
    ///
    /// The raw `uid` and the process `state` used to be carried here too.
    /// `state` was the `STAT` column this printed under `-f`, which procps
    /// does not have there -- it belongs to `-l`, which this build does not
    /// implement. Both are dropped rather than kept unread: a field nobody
    /// reads is indistinguishable from one whose reader was deleted by
    /// mistake, and `stat.state` is one line away if `-l` ever arrives.
    user: String,
    /// procps' `C` column: integer percent of CPU over the process's life.
    cpu_pct: u64,
    /// `-l`'s `S`: the one-character state.
    state: String,
    /// `-l`'s `F`: `(flags >> 6) & 7`, in octal. Measured -- a default task
    /// carries `flags` 4194560 and procps prints `4`.
    flag: u64,
    /// `-l`'s `PRI`. **`stat`'s priority PLUS 60**, which is measured, not
    /// derived: with nice 0, 5, 10 and 19 procps prints 80, 85, 90 and 99
    /// while the file says 20, 25, 30 and 39.
    pri: i64,
    /// `-l`'s `NI`.
    nice: i64,
    /// `-l`'s `SZ`: virtual size in 4096-byte pages.
    size_pages: u64,
    /// `-l`'s `WCHAN`, truncated to six characters as procps does. `-` when
    /// the process is running rather than blocked.
    wchan: String,
    /// procps' `STIME`: `HH:MM` if the process started today, else `MMM DD`.
    stime: String,
    tty: String,
    time_str: String,
    /// The `-f` command line, already rendered. Empty when `-f` was not asked
    /// for, which is also when it was never read.
    cmd: String,
}

/// The funnel. A diagnostic that could not be written turns the earned
/// status into `exit_failure`, which is what upstream's `atexit
/// (close_stdout)` does on every exit path at once. See
/// [`stdfd::close_stderr`].
fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let parsed = match parse_args(&args) {
        Ok(Request::List(p)) => p,
        Ok(Request::Help) => {
            print!("{}", help_text());
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            // procps prints the error AND its whole usage, both on stderr,
            // separated by a blank line -- which `help_text`'s leading newline
            // supplies. Measured: `ps --nosuchoption 2>&1` is the message, an
            // empty line, then the same 170 bytes `--help` prints.
            //
            // This is the opposite of the call made for `uptime`, whose
            // upstream is also procps: there the shared `coreutils::getopt`
            // formatter prints GNU's `Try '… --help'` hint for all 86 bins and
            // changing it for one would make the other 85 wrong. `ps` does not
            // go through that formatter, so matching costs nothing here.
            diag!("{}", message);
            eprint!("{}", help_text());
            return ExitCode::FAILURE;
        }
    };

    let stdout = io::stdout();
    let mut out = stdout.lock();

    // procps' own field widths, derived from its output rather than chosen.
    //
    // Both header and rows go through the same format string, which is what
    // makes them line up; `ps -f` is
    //
    //     {:<8} {:>7} {:>7} {:>2} {:>5} {:<8} {:>8} {}
    //
    // and the default is `{:>5} {:<8} {:>8} {}`. Each was checked against the
    // column offsets of a measured line -- PID ends at 16, PPID at 24, C at
    // 27, STIME spans 29-33, TTY starts at 35, TIME ends at 51, CMD starts at
    // 53 -- because two samples are not enough to infer a width and this is
    // the kind of thing that looks right until a field overflows.
    //
    // The default `PID` column is SEVEN wide, not five. I had it at five
    // first, having counted it off the differential harness's own REPORT
    // line -- which prefixes the output with `  ours (rc=0): ` and shifts
    // every column. Measure the artifact, not a rendering of it:
    // `ps | cat -A` settles it in one line.
    // `--no-header` suppresses the titles and nothing else. The header still
    // prints when `-p` matches nothing -- measured: `ps -p 999999` writes the
    // header and exits 1, so "no rows" and "no header" are independent.
    // `-o` replaces the built-in formats entirely, and suppresses the header
    // by itself when EVERY title is empty -- `ps -o comm=` prints one column
    // and no heading at all, while `ps -o pid,comm=` still prints a heading
    // because `pid` kept its own. Measured.
    let custom = !parsed.columns.is_empty();
    let all_titles_empty = custom && parsed.columns.iter().all(|c| c.title.is_empty());
    if custom {
        if !parsed.no_header && !all_titles_empty {
            let titles: Vec<String> = parsed.columns.iter().map(|c| c.title.clone()).collect();
            let _ = writeln!(out, "{}", render_row(&titles, &parsed.columns));
        }
    } else if !parsed.no_header {
        if parsed.long_format && parsed.full_format {
            // `-l` AND `-f` is a MERGED format, not one of them winning.
            // Measured: it is `-l`'s column set with three substitutions --
            // UID widened to 8 and rendered as a NAME, STIME inserted after
            // WCHAN, and CMD carrying the full command line. The ADDR/SZ pair
            // still abut with no separator, exactly as in `-l`.
            let _ = writeln!(
                out,
                "{:<1} {:<1} {:<8} {:>7} {:>7} {:>2} {:>3} {:>3} {:<4}{:>3} {:<6} {:>5} {:<8} {:>8} CMD",
                "F",
                "S",
                "UID",
                "PID",
                "PPID",
                "C",
                "PRI",
                "NI",
                "ADDR",
                "SZ",
                "WCHAN",
                "STIME",
                "TTY",
                "TIME"
            );
        } else if parsed.long_format {
            // ADDR and SZ ABUT WITH NO SEPARATOR. Every other pair here is
            // joined by one space; these two are not, and the gap in the
            // header is SZ's own right-padding. Computed from the column
            // offsets of a measured line rather than counted by eye --
            // `ADDR SZ` and `-   701` both occupy exactly columns 37-43.
            let _ = writeln!(
                out,
                "{:<1} {:<1} {:>5} {:>7} {:>7} {:>2} {:>3} {:>3} {:<4}{:>3} {:<6} {:<8} {:>8} CMD",
                "F",
                "S",
                "UID",
                "PID",
                "PPID",
                "C",
                "PRI",
                "NI",
                "ADDR",
                "SZ",
                "WCHAN",
                "TTY",
                "TIME"
            );
        } else if parsed.full_format {
            let _ = writeln!(
                out,
                "{:<8} {:>7} {:>7} {:>2} {:>5} {:<8} {:>8} CMD",
                "UID", "PID", "PPID", "C", "STIME", "TTY", "TIME"
            );
        } else {
            let _ = writeln!(out, "{:>7} {:<8} {:>8} CMD", "PID", "TTY", "TIME");
        }
    }

    let procfs = procinfo::ProcFs::new();
    let ctx = ListCtx::new(&procfs);
    let Ok(pids) = procfs.process_ids() else {
        // No /proc — nothing to show.
        return ExitCode::SUCCESS;
    };

    let mut matched = false;
    for pid in pids {
        // `-e`/`-A` OVERRIDES every selection, in either order. Measured:
        // `ps -e -p 2`, `ps -p 2 -e`, `ps -e -u root` and `ps -e -t ?` all
        // list every process, so "show all" is not one filter among several
        // -- it cancels them. Without this, `-e -p 2` printed one row here
        // against procps' two.
        let select = !parsed.all_procs;
        // `-p` selects; without it every process is shown.
        if select
            && parsed
                .select_pids
                .as_ref()
                .is_some_and(|w| !w.contains(&pid))
        {
            continue;
        }
        // `-o args` needs the command line too, so the flag is "does anything
        // ask for it" rather than "was -f given". Reading it costs a second
        // open per process, which is why it is still conditional.
        let wants_cmdline = parsed.full_format
            || parsed
                .columns
                .iter()
                .any(|s| COLUMNS.get(s.col).is_some_and(|c| c.name == "args"));
        let Ok(Some(info)) = read_one(&procfs, pid, wants_cmdline, parsed.long_format, &ctx) else {
            // A process that exits between the listing and the read is the
            // normal case for anything walking /proc, not a failure.
            continue;
        };
        // `-t` matches on the RENDERED terminal, which is the same string the
        // TTY column prints -- so `ps -t ?` and the `?` a reader sees in the
        // table cannot disagree.
        if select
            && parsed
                .select_ttys
                .as_ref()
                .is_some_and(|w| !w.contains(&info.tty))
        {
            continue;
        }
        // `-u` filters on a value only the read can supply, so unlike `-p` it
        // cannot skip the read first.
        if select
            && parsed
                .select_uids
                .as_ref()
                .is_some_and(|w| !w.contains(&info.uid))
        {
            continue;
        }
        let pid32 = u32::try_from(pid).unwrap_or(0);

        if custom {
            let cells: Vec<String> = parsed
                .columns
                .iter()
                .map(|spec| info.cell(COLUMNS.get(spec.col), pid32))
                .collect();
            let _ = writeln!(out, "{}", render_row(&cells, &parsed.columns));
        } else if parsed.long_format && parsed.full_format {
            let _ = writeln!(
                out,
                "{:<1} {:<1} {:<8} {:>7} {:>7} {:>2} {:>3} {:>3} {:<4}{:>3} {:<6} {:>5} {:<8} {:>8} {}",
                info.flag,
                info.state,
                info.user,
                pid32,
                info.ppid,
                info.cpu_pct,
                info.pri,
                info.nice,
                "-",
                info.size_pages,
                info.wchan,
                info.stime,
                info.tty,
                info.time_str,
                info.cmd
            );
        } else if parsed.long_format {
            let _ = writeln!(
                out,
                "{:<1} {:<1} {:>5} {:>7} {:>7} {:>2} {:>3} {:>3} {:<4}{:>3} {:<6} {:<8} {:>8} {}",
                info.flag,
                info.state,
                info.uid,
                pid32,
                info.ppid,
                info.cpu_pct,
                info.pri,
                info.nice,
                "-",
                info.size_pages,
                info.wchan,
                info.tty,
                info.time_str,
                info.comm
            );
        } else if parsed.full_format {
            let _ = writeln!(
                out,
                "{:<8} {:>7} {:>7} {:>2} {:>5} {:<8} {:>8} {}",
                info.user,
                pid32,
                info.ppid,
                info.cpu_pct,
                info.stime,
                info.tty,
                info.time_str,
                info.cmd
            );
        } else {
            let _ = writeln!(
                out,
                "{:>7} {:<8} {:>8} {}",
                pid32, info.tty, info.time_str, info.comm
            );
        }
        matched = true;
    }

    // `ps -p <pid that is not running>` exits 1 having printed only the
    // header. Measured. Without `-p` an empty table is not an error -- there
    // is always at least this process -- so the status only turns on a
    // selection that matched nothing.
    // A selection that matched nothing exits 1 -- but only when there WAS a
    // selection, and `-e` means there was not.
    if !parsed.all_procs
        && (parsed.select_pids.is_some()
            || parsed.select_uids.is_some()
            || parsed.select_ttys.is_some())
        && !matched
    {
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

/// One process, read through [`procinfo`].
///
/// The `/proc/<pid>/stat` parsing this used to do itself now lives in the
/// crate, shared with `userspace/htop` and `apps/procexplorer` -- and with
/// `userspace/ps` until that crate was retired on 2026-09-12, leaving this
/// the only `ps`. Two things it could not do on its own:
///
/// * **the real UID.** This was `uid: 0` with the comment "would need
///   `/proc/<pid>/status` for real UID" — so the `-f` listing showed every
///   process as root. The crate reads `status`, so it is a field now.
/// * **a name that is not UTF-8.** `read_to_string` fails on one, and the
///   process was skipped entirely by `continue`. A `ps` that omits exactly
///   the processes with unusual names is the worst way to be wrong.
///
/// # Errors
///
/// Any read error other than "no such file", which is `Ok(None)`.
fn read_one(
    procfs: &procinfo::ProcFs,
    pid: u64,
    full: bool,
    long: bool,
    ctx: &ListCtx,
) -> std::io::Result<Option<ProcInfo>> {
    let Some(stat) = procfs.process_stat(pid)? else {
        return Ok(None);
    };
    let comm = procinfo::display_bytes(&stat.comm);
    let uid = procfs
        .process_status(pid)?
        .and_then(|st| st.uid)
        .unwrap_or(0);
    // Only `-f` prints the command line, and reading it costs a second open
    // per process.
    let cmd = if full {
        let args = procfs.process_cmdline(pid)?.unwrap_or_default();
        if args.is_empty() {
            format!("[{comm}]")
        } else {
            args.iter()
                .map(|a| procinfo::display_bytes(a))
                .collect::<Vec<_>>()
                .join(" ")
        }
    } else {
        String::new()
    };
    let start_epoch = ctx.start_epoch(stat.starttime_ticks);
    Ok(Some(ProcInfo {
        comm,
        ppid: u32::try_from(stat.ppid).unwrap_or(0),
        uid,
        user: ctx.user_name(uid),
        state: char::from(stat.state).to_string(),
        flag: (stat.flags >> 6) & 7,
        pri: stat.priority.saturating_add(60),
        nice: stat.nice,
        size_pages: stat.vsize_bytes / 4096,
        wchan: if long {
            read_wchan(procfs, pid)
        } else {
            String::new()
        },
        cpu_pct: cpu_percent(
            stat.utime_ticks,
            stat.stime_ticks,
            ctx.now_epoch.saturating_sub(start_epoch),
        ),
        stime: ctx.format_stime(start_epoch),
        tty: format_tty(i32::try_from(stat.tty_nr).unwrap_or(0)),
        time_str: format_cpu_time(stat.utime_ticks, stat.stime_ticks),
        cmd,
    }))
}

impl ProcInfo {
    /// One column's value for this process.
    ///
    /// `args` falls back to the bracketed `comm` when the command line is
    /// empty, which is what the `-f` path already does for a kernel thread --
    /// the same value, reached the same way, rather than a second rule.
    fn cell(&self, col: Option<&Column>, pid: u32) -> String {
        let Some(col) = col else { return String::new() };
        match col.name {
            "pid" => pid.to_string(),
            "ppid" => self.ppid.to_string(),
            "uid" => self.uid.to_string(),
            "user" => self.user.clone(),
            "comm" => self.comm.clone(),
            "args" => {
                if self.cmd.is_empty() {
                    format!("[{}]", self.comm)
                } else {
                    self.cmd.clone()
                }
            }
            "tty" => self.tty.clone(),
            "time" => self.time_str.clone(),
            "stime" => self.stime.clone(),
            "c" => self.cpu_pct.to_string(),
            // `COLUMNS` is the only source of names and every one of them is
            // handled above; an unknown name cannot be constructed because
            // `parse_columns` refuses it.
            _ => String::new(),
        }
    }
}

/// The things every row needs and no row should read for itself.
///
/// `/etc/passwd` and `/proc/stat`'s `btime` are the same for every process in
/// a listing, so they are read once. A per-row lookup would reopen
/// `/etc/passwd` for each of several hundred processes, and -- worse -- could
/// see a different boot time partway down the table.
struct ListCtx {
    db: pwdb::Db,
    zone: Zone,
    /// `btime` from `/proc/stat`: the wall-clock second the system booted.
    boot_epoch: i64,
    /// Read once, so every `STIME` in one listing is judged against the same
    /// "today".
    now_epoch: i64,
}

impl ListCtx {
    fn new(procfs: &procinfo::ProcFs) -> Self {
        // `stat_counters`, not `stat`: `/proc/stat` is the whole-system file
        // and `/proc/<pid>/stat` is the per-process one, and `ProcFs` has an
        // accessor for each. `btime` lives on the former.
        let boot_epoch = procfs
            .stat_counters()
            .ok()
            .flatten()
            .and_then(|st| st.boot_time)
            .and_then(|b| i64::try_from(b).ok())
            .unwrap_or(0);
        let now_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .unwrap_or(0);
        Self {
            db: pwdb::Db::load(),
            zone: Zone::from_env(),
            boot_epoch,
            now_epoch,
        }
    }

    /// When the process started, in epoch seconds.
    fn start_epoch(&self, starttime_ticks: u64) -> i64 {
        let secs_since_boot = starttime_ticks / procinfo::TICKS_PER_SEC;
        self.boot_epoch
            .saturating_add(i64::try_from(secs_since_boot).unwrap_or(0))
    }

    /// procps prints the NAME. A uid with no passwd entry keeps its number,
    /// which is also what procps does -- it does not invent one, and inventing
    /// one is the defect `uptime`'s user count was written to avoid.
    fn user_name(&self, uid: u32) -> String {
        self.db
            .user_by_uid(uid)
            .map_or_else(|| uid.to_string(), |u| procinfo::display_bytes(&u.name))
    }

    /// `HH:MM` when the process started today, `MMM DD` otherwise.
    ///
    /// Measured: a process started this morning prints `09:49`, one from
    /// yesterday prints `Sep11` -- month abbreviation and a ZERO-PADDED day
    /// with no space between them, five characters either way, which is why
    /// the column is exactly five wide.
    fn format_stime(&self, start_epoch: i64) -> String {
        let started = self.zone.local(start_epoch, 0);
        let now = self.zone.local(self.now_epoch, 0);
        let same_day =
            started.year == now.year && started.month == now.month && started.day == now.day;
        let fmt: &[u8] = if same_day { b"%H:%M" } else { b"%b%d" };
        String::from_utf8(strftime(fmt, &started)).unwrap_or_default()
    }
}

/// `/proc/<pid>/wchan`, as `-l` prints it.
///
/// procps truncates to six characters -- `do_wait` shows as `do_wai` -- and
/// prints `-` for a process that is running rather than blocked, which the
/// kernel reports as `0`. Read only under `-l`, because it is a third open
/// per process.
fn read_wchan(procfs: &procinfo::ProcFs, pid: u64) -> String {
    let raw = procfs.process_wchan(pid).ok().flatten().unwrap_or_default();
    let text = procinfo::display_bytes(&raw);
    let text = text.trim();
    if text.is_empty() || text == "0" {
        return "-".to_string();
    }
    text.chars().take(6).collect()
}

/// procps' `C` column: integer percent of CPU used over the process's life.
///
/// Zero elapsed seconds yields 0 rather than a division by zero -- every
/// process is younger than a second at some point, including `ps` itself,
/// which is always in its own listing.
fn cpu_percent(utime: u64, stime: u64, elapsed_secs: i64) -> u64 {
    let elapsed = u64::try_from(elapsed_secs).unwrap_or(0);
    if elapsed == 0 {
        return 0;
    }
    let total_secs = utime.saturating_add(stime) / procinfo::TICKS_PER_SEC;
    total_secs.saturating_mul(100) / elapsed
}

/// Format the `tty_nr` field from /proc/<pid>/stat.  Zero is "?" (no
/// controlling terminal); otherwise we render it as `pts/<minor>`,
/// taking the low 8 bits as the minor number.
fn format_tty(tty_nr: i32) -> String {
    if tty_nr == 0 {
        "?".to_string()
    } else {
        format!("pts/{}", tty_nr & 0xff)
    }
}

/// Format CPU time (user + system clock ticks) as HH:MM:SS.
///
/// The tick rate is `procinfo::TICKS_PER_SEC` rather than a literal 100:
/// it is a fact about the kernel and belongs in one place.
fn format_cpu_time(utime: u64, stime: u64) -> String {
    let total_ticks = utime.saturating_add(stime);
    let total_secs = total_ticks / procinfo::TICKS_PER_SEC;
    let hours = total_secs / 3600;
    let mins = (total_secs / 60) % 60;
    let secs = total_secs % 60;
    format!("{hours:02}:{mins:02}:{secs:02}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    /// A newline, built rather than escaped.
    const NL: &str = "\n";
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    // ---------------- parse_args ----------------

    /// The listing options, unwrapped. Every one of these was valid before and
    /// still is; only the return type moved.
    fn listed(args: &[&str]) -> PsArgs {
        match parse_args(&s(args)) {
            Ok(Request::List(p)) => p,
            other => panic!("expected a listing request, got {other:?}"),
        }
    }

    #[test]
    fn parse_empty() {
        assert_eq!(listed(&[]), PsArgs::default());
    }

    #[test]
    fn parse_dash_e() {
        let a = listed(&["-e"]);
        assert!(a.all_procs);
        assert!(!a.full_format);
    }

    #[test]
    fn parse_dash_a_uppercase_is_alias_for_e() {
        assert!(listed(&["-A"]).all_procs);
    }

    #[test]
    fn parse_dash_f() {
        let a = listed(&["-f"]);
        assert!(a.full_format);
        assert!(!a.all_procs);
    }

    #[test]
    fn parse_clustered_ef() {
        let a = listed(&["-ef"]);
        assert!(a.all_procs);
        assert!(a.full_format);
    }

    /// THIS TEST USED TO ASSERT THE BUG.
    ///
    /// It was called `parse_unknown_silently_ignored` and its comment read
    /// "Preserves previous behaviour — no error, no panic." What it preserved
    /// was `ps -Q` printing the whole process table and exiting 0 where procps
    /// prints `error: unsupported SysV option` and exits 1. A test can hold a
    /// defect in place as firmly as it holds a feature, and the only thing
    /// distinguishing the two is whether anyone measured.
    #[test]
    fn parse_unknown_short_option_is_refused() {
        let err = parse_args(&s(&["-Q"])).expect_err("-Q is not an option here");
        assert_eq!(err, "error: unsupported SysV option");
        // Still refused when clustered behind valid flags, which is where a
        // character-at-a-time parser is most likely to let one through.
        assert!(parse_args(&s(&["-efQ"])).is_err());
    }

    /// `--help` USED TO PRINT THE PROCESS TABLE.
    ///
    /// There was no long-option branch: `--help` had one `-` stripped and the
    /// rest was walked character by character, so it was read as `-h -e -l -p`
    /// and the `e` matched. The bug is not that `--help` was unimplemented, it
    /// is that a long option silently became whichever short flags its letters
    /// happened to spell.
    #[test]
    fn long_options_are_matched_whole_not_letter_by_letter() {
        assert_eq!(parse_args(&s(&["--help"])), Ok(Request::Help));
        // The three that would have set a flag by accident.
        for arg in ["--nosuchoption", "--full", "--version"] {
            let err = parse_args(&s(&[arg])).expect_err(arg);
            assert_eq!(err, "error: unknown gnu long option", "for {arg}");
        }
    }

    /// `-p` takes its list glued or separate, and both spellings are procps'.
    #[test]
    fn dash_p_accepts_glued_and_separate_lists() {
        assert_eq!(listed(&["-p", "1"]).select_pids, Some(vec![1]));
        assert_eq!(listed(&["-p1"]).select_pids, Some(vec![1]));
        assert_eq!(listed(&["-p", "1,2,3"]).select_pids, Some(vec![1, 2, 3]));
        assert_eq!(listed(&["-p", "1 2"]).select_pids, Some(vec![1, 2]));
        // Clustered behind another flag, which is where the character-at-a-
        // time loop hands the remainder over as the argument.
        assert_eq!(listed(&["-ep", "7"]).select_pids, Some(vec![7]));
        assert!(listed(&["-ep", "7"]).all_procs);
    }

    /// A bad PID list is its OWN error, not "unsupported option".
    ///
    /// procps distinguishes the two and the distinction is the point: `-p` is
    /// a known option with a bad argument, and reporting it as an unknown
    /// option would send the reader looking for the wrong mistake.
    #[test]
    fn dash_p_rejects_a_bad_list_with_its_own_message() {
        for bad in ["notanumber", "1,two", "", "-1"] {
            let err = parse_args(&s(&["-p", bad])).expect_err(bad);
            assert_eq!(err, "error: process ID list syntax error", "for {bad:?}");
        }
    }

    /// `-u` takes names OR numbers, and only a name can fail to resolve.
    ///
    /// Measured: `ps -u 99999` does not complain about the uid, it selects
    /// nothing and exits 1 through the no-match path. So a number is taken as
    /// a UID without being checked to exist, and rejecting one would refuse a
    /// command line procps accepts.
    #[test]
    fn dash_u_takes_names_or_numbers() {
        assert_eq!(listed(&["-u", "0"]).select_uids, Some(vec![0]));
        assert_eq!(listed(&["-u0"]).select_uids, Some(vec![0]));
        // A uid that exists as a number but matches nothing is NOT an error.
        assert_eq!(listed(&["-u", "99999"]).select_uids, Some(vec![99999]));
    }

    /// Name resolution, against a passwd file built here rather than the
    /// host's.
    ///
    /// This test began as `-u root` through `parse_args`, which passed under
    /// WSL and failed on the Windows host for want of `/etc/passwd`. An
    /// assertion about the machine is not an assertion about the parser, and
    /// the giveaway is that it would have held in every environment where the
    /// answer did not matter.
    #[test]
    fn dash_u_resolves_names_through_the_passwd_database() {
        let passwd =
            format!("root:x:0:0:root:/root:/bin/sh{NL}bin:x:1:1:bin:/bin:/sbin/nologin{NL}");
        let db = pwdb::Db::from_bytes(passwd.as_bytes(), b"");
        let get = Some(&db);
        assert_eq!(parse_user_list_with("root", get), Ok(vec![0]));
        assert_eq!(parse_user_list_with("bin", get), Ok(vec![1]));
        // Name and number must select identically.
        assert_eq!(
            parse_user_list_with("root", get),
            parse_user_list_with("0", get)
        );
        assert_eq!(parse_user_list_with("root,bin", get), Ok(vec![0, 1]));
        // A number is never looked up, so it resolves with no passwd entry.
        assert_eq!(parse_user_list_with("99999", get), Ok(vec![99999]));
        for bad in ["nosuchuser", "", "root,nosuchuser"] {
            assert_eq!(
                parse_user_list_with(bad, get),
                Err("error: user name does not exist".to_string()),
                "for {bad:?}"
            );
        }
    }

    #[test]
    fn dash_u_rejects_a_name_that_does_not_resolve() {
        let err = parse_args(&s(&["-u", "nosuchuser"])).expect_err("no such user");
        assert_eq!(err, "error: user name does not exist");
        assert!(parse_args(&s(&["-u", ""])).is_err());
    }

    fn cols(list: &str) -> Vec<Spec> {
        let mut out = Vec::new();
        parse_columns(list, &mut out).expect("valid column list");
        out
    }

    /// THE RULE THAT TOOK TWO GOES.
    ///
    /// A right-aligned column is padded even when it is the last on the line,
    /// because its padding lands on the LEFT and so is not trailing
    /// whitespace. A left-aligned one is not. The first version treated
    /// "last" as "unpadded" for both and `scripts/ps-diff.sh` returned six
    /// failures for it, every one a right-aligned column standing alone.
    ///
    /// Each row below is a measured procps output, not a derivation.
    #[test]
    fn a_trailing_right_aligned_column_keeps_its_padding() {
        // `ps -o pid` -> "    PID" / "      1"
        let pid = cols("pid");
        assert_eq!(render_row(&["PID".into()], &pid), "    PID");
        assert_eq!(render_row(&["1".into()], &pid), "      1");
        // `ps -o user` -> "USER" / "root", no padding at all
        let user = cols("user");
        assert_eq!(render_row(&["USER".into()], &user), "USER");
        assert_eq!(render_row(&["root".into()], &user), "root");
        // `ps -o user,pid` -> USER padded to 8, PID right-aligned in 7
        let both = cols("user,pid");
        assert_eq!(
            render_row(&["USER".into(), "PID".into()], &both),
            "USER         PID"
        );
        assert_eq!(
            render_row(&["root".into(), "1".into()], &both),
            "root           1"
        );
    }

    /// `=` empties a title, and the line then ends at the SEPARATOR.
    ///
    /// `ps -o pid,comm=` prints "    PID " with one trailing space -- the
    /// empty final field contributes nothing, but the space before it stays.
    /// Trimming the whole line would eat it; padding the last field would add
    /// fourteen more.
    #[test]
    fn an_empty_title_leaves_the_separator_behind() {
        let specs = cols("pid,comm=");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[1].title, "");
        assert_eq!(
            render_row(&["PID".into(), String::new()], &specs),
            "    PID "
        );
        assert_eq!(render_row(&["1".into(), "ps".into()], &specs), "      1 ps");
    }

    #[test]
    fn column_specs_take_titles_and_accumulate() {
        // A custom title keeps the column's width and alignment.
        let renamed = cols("pid=MYPID");
        assert_eq!(renamed[0].title, "MYPID");
        assert_eq!(render_row(&["MYPID".into()], &renamed), "  MYPID");
        // Two `-o` flags accumulate rather than replace, as procps does.
        let mut acc = Vec::new();
        parse_columns("pid", &mut acc).expect("pid");
        parse_columns("comm", &mut acc).expect("comm");
        assert_eq!(acc.len(), 2);
        // `comm` is titled COMMAND and `tty` is titled TT -- neither follows
        // from the name, and the default format heads the same field `TTY`.
        assert_eq!(cols("comm")[0].title, "COMMAND");
        assert_eq!(cols("tty")[0].title, "TT");
    }

    #[test]
    fn an_unknown_column_is_refused_with_procps_wording() {
        let mut sink = Vec::new();
        let err = parse_columns("nosuchcolumn", &mut sink).expect_err("unknown");
        assert_eq!(
            err,
            "error: unknown user-defined format specifier \"nosuchcolumn\""
        );
    }

    /// `-t` against a `/dev` supplied by the test, not by the host.
    ///
    /// Measured from procps: `?` and `-` both mean "no terminal";
    /// `-t pts/0` is ACCEPTED even with nothing on it, because the test is
    /// whether the terminal exists rather than whether it is in use; and
    /// `-t nosuchtty` is `error: TTY could not be found`.
    #[test]
    fn dash_t_accepts_a_terminal_that_exists_and_refuses_one_that_does_not() {
        let dev = |name: &str| matches!(name, "pts/0" | "tty1");
        let ok = |list: &str| parse_tty_list_with(list, &dev);
        // The absence of a terminal, spelled two ways, normalised to one.
        assert_eq!(ok("?"), Ok(vec!["?".to_string()]));
        assert_eq!(ok("-"), Ok(vec!["?".to_string()]));
        // Existing terminals, bare and /dev-prefixed, both normalise to bare
        // so they can be compared against what the TTY column prints.
        assert_eq!(ok("pts/0"), Ok(vec!["pts/0".to_string()]));
        assert_eq!(ok("/dev/pts/0"), Ok(vec!["pts/0".to_string()]));
        assert_eq!(
            ok("pts/0,tty1"),
            Ok(vec!["pts/0".to_string(), "tty1".to_string()])
        );
        // And the refusals.
        for bad in ["nosuchtty", "", "pts/0,nosuchtty"] {
            assert_eq!(
                ok(bad),
                Err("error: TTY could not be found".to_string()),
                "for {bad:?}"
            );
        }
    }

    #[test]
    fn no_header_is_a_long_option_and_has_an_alias() {
        assert!(listed(&["--no-header"]).no_header);
        assert!(listed(&["--no-heading"]).no_header);
        // It does not disturb the others.
        let a = listed(&["-ef", "--no-header"]);
        assert!(a.all_procs && a.full_format && a.no_header);
    }

    #[test]
    fn parse_bare_args_ignored() {
        // ps doesn't take positional arguments in our minimal build. procps
        // reads a PID list here, so refusing would reject a command line the
        // reference accepts -- deliberately unchanged.
        assert_eq!(listed(&["1234"]), PsArgs::default());
    }

    // ---------------- format_tty ----------------

    #[test]
    fn tty_zero_is_question_mark() {
        assert_eq!(format_tty(0), "?");
    }

    #[test]
    fn tty_nonzero_masked_to_byte() {
        assert_eq!(format_tty(34816), format!("pts/{}", 34816 & 0xff));
        assert_eq!(format_tty(1), "pts/1");
        assert_eq!(format_tty(255), "pts/255");
        assert_eq!(format_tty(256), "pts/0");
    }

    // ---------------- format_cpu_time ----------------

    #[test]
    fn cpu_time_zero() {
        assert_eq!(format_cpu_time(0, 0), "00:00:00");
    }

    #[test]
    fn cpu_time_one_second() {
        // 100 ticks at 100Hz = 1 second.
        assert_eq!(format_cpu_time(100, 0), "00:00:01");
    }

    #[test]
    fn cpu_time_user_plus_system() {
        // 50 + 50 = 100 ticks = 1 second.
        assert_eq!(format_cpu_time(50, 50), "00:00:01");
    }

    #[test]
    fn cpu_time_one_minute() {
        assert_eq!(format_cpu_time(60 * 100, 0), "00:01:00");
    }

    #[test]
    fn cpu_time_one_hour() {
        assert_eq!(format_cpu_time(3600 * 100, 0), "01:00:00");
    }

    #[test]
    fn cpu_time_hms_combined() {
        // 1h 23m 45s = 3600 + 1380 + 45 = 5025 sec = 502500 ticks.
        assert_eq!(format_cpu_time(502500, 0), "01:23:45");
    }

    #[test]
    fn cpu_time_overflow_saturates() {
        // utime + stime overflowing saturates to u64::MAX / 100.
        let s = format_cpu_time(u64::MAX, u64::MAX);
        // Should not panic; just produce a very large hour value.
        assert!(s.contains(':'));
    }

    // ---------------- parse_proc_stat ----------------

    // The `/proc/<pid>/stat` parsing tests that used to live here have moved to
    // `procinfo`, with the parser. They covered the minimal line, a `comm`
    // containing spaces, a `comm` containing parentheses, and pathological
    // input; `procinfo/src/tests.rs` covers all four and adds one this file
    // could not — a `comm` that is not valid UTF-8, which this program used to
    // drop the process for.
    //
    // **One behavioural difference is deliberate and worth naming.** The old
    // parser returned a defaulted `ProcInfo` for a truncated line such as
    // `"1 (a) S"`, so `ps` printed a row of zeros for it. `procinfo` returns
    // `None` for a line with fewer than 22 fields, so the process is skipped.
    // A three-field `stat` is not a process the kernel is describing; printing
    // a confident row of zeros for it is the same class of mistake as the CPU
    // bars' `max(1)`.
    //
    // What stays here is what is still this program's: the two formatters.

    #[test]
    fn no_controlling_terminal_is_a_question_mark() {
        assert_eq!(format_tty(0), "?");
    }

    /// The low eight bits are the minor number, which is why 34816 and 256
    /// both render as `pts/0` — the major is not shown.
    #[test]
    fn a_tty_renders_as_its_minor_number() {
        assert_eq!(format_tty(34816), "pts/0");
        assert_eq!(format_tty(34817), "pts/1");
        assert_eq!(format_tty(0x8_00_ff), "pts/255");
    }

    /// Ticks, not seconds. `procinfo::TICKS_PER_SEC` is the divisor, so this
    /// test also pins that the two agree.
    #[test]
    fn cpu_time_is_ticks_rendered_as_hms() {
        assert_eq!(format_cpu_time(0, 0), "00:00:00");
        assert_eq!(format_cpu_time(200, 100), "00:00:03");
        assert_eq!(procinfo::TICKS_PER_SEC, 100);
        // One hour, one minute, one second.
        let ticks = (3600 + 60 + 1) * procinfo::TICKS_PER_SEC;
        assert_eq!(format_cpu_time(ticks, 0), "01:01:01");
    }

    /// Hours are not wrapped at 24: a process can run for days, and `01:00:00`
    /// after 25 hours would be a lie.
    #[test]
    fn hours_accumulate_past_a_day() {
        let ticks = 25 * 3600 * procinfo::TICKS_PER_SEC;
        assert_eq!(format_cpu_time(ticks, 0), "25:00:00");
    }
}
