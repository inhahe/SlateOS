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
    if !parsed.no_header {
        if parsed.full_format {
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
        // `-p` selects; without it every process is shown.
        if parsed
            .select_pids
            .as_ref()
            .is_some_and(|w| !w.contains(&pid))
        {
            continue;
        }
        let Ok(Some(info)) = read_one(&procfs, pid, parsed.full_format, &ctx) else {
            // A process that exits between the listing and the read is the
            // normal case for anything walking /proc, not a failure.
            continue;
        };
        // `-u` filters on a value only the read can supply, so unlike `-p` it
        // cannot skip the read first.
        if parsed
            .select_uids
            .as_ref()
            .is_some_and(|w| !w.contains(&info.uid))
        {
            continue;
        }
        let pid32 = u32::try_from(pid).unwrap_or(0);

        if parsed.full_format {
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
    if (parsed.select_pids.is_some() || parsed.select_uids.is_some()) && !matched {
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
