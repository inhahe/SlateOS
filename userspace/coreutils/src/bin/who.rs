//! `who` -- print information about users who are currently logged in.
//!
//! A port of GNU coreutils 9.4's `src/who.c`, measured against the real binary
//! by `scripts/who-diff.sh`. The entries come from [`coreutils::utmp`], which
//! is gnulib's `readutmp` -- which entries a caller gets, and the boot entry
//! made up when a Linux utmp has none -- shared with `users` and `pinky`; this
//! file is what `who` does with them.
//!
//! It replaces the `who` of `userspace/who`, written from the manual: it had
//! a `--json` upstream does not, printed `system boot` from an invented time
//! when utmp had none, and read session files of its own besides utmp. The
//! `w` that crate also answered to is procps' program, not this one, and
//! moves to a port of its own (`userspace/coreutils/src/bin/w.rs`).
//!
//! # One line per entry, in upstream's columns
//!
//! Every line is `print_line`'s: the name in 8 columns, the message status
//! with `-T`, the line in 12, the time, then idle, pid, comment and exit
//! status as the options ask -- and trailing spaces cut. Which entries are
//! lines is `scan_entries`': user processes by default; boot, run level,
//! clock changes, init's processes, logins waiting and dead processes as the
//! options add them; and with `-m`, or two operands (`who am i`), only the
//! entries for the terminal on standard input.
//!
//! # The time
//!
//! `%Y-%m-%d %H:%M` unless `LC_TIME` is the C locale by name, where it is
//! `%b %e %H:%M` -- gnulib's `hard_locale`, which `pinky` and `ls` share
//! through [`coreutils::locale`]. In the zone `TZ` names, through the
//! `localtime` crate.
//!
//! # Idle, and the message status
//!
//! From the terminal device: its access time for idle (`  .  ` under a
//! minute, `HH:MM` under a day, ` old ` otherwise or when it predates the last
//! boot entry seen), group-write permission for `+`/`-`, and `?` when it
//! cannot be examined. Upstream's build checks the device's group too when
//! `TTY_GROUP_NAME` is defined; coreutils 9.4 on Linux defines it nowhere, so
//! neither does this.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use coreutils::quote::{os_bytes, quote};
use coreutils::utmp::{UTMP_FILE, WTMP_FILE};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const WHO: Program = Program::new("who", 1);

/// Upstream's `"abdlmpqrstuwHT"`.
const SHORT_OPTIONS: &str = "abdlmpqrstuwHT";

/// Upstream's `longopts`, in its order, so that an abbreviation resolves as
/// it does there.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("all", Takes::Nothing),
    ("boot", Takes::Nothing),
    ("count", Takes::Nothing),
    ("dead", Takes::Nothing),
    ("heading", Takes::Nothing),
    ("login", Takes::Nothing),
    ("lookup", Takes::Nothing),
    ("message", Takes::Nothing),
    ("mesg", Takes::Nothing),
    ("process", Takes::Nothing),
    ("runlevel", Takes::Nothing),
    ("short", Takes::Nothing),
    ("time", Takes::Nothing),
    ("users", Takes::Nothing),
    ("writable", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The three spellings of `-T`. glibc calls an abbreviation ambiguous only
/// when its matches differ in what they do, so `--me` is `--message`, not an
/// error: these say which names are one option.
const ALIASES: &[(&str, &str)] = &[("message", "mesg"), ("writable", "mesg")];

/// Upstream's file-scope switches, which the options set.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's fifteen independent switches, one per option; a bitset would only rename them"
)]
struct Show {
    do_lookup: bool,
    short_list: bool,
    short_output: bool,
    include_idle: bool,
    include_heading: bool,
    include_mesg: bool,
    include_exit: bool,
    need_boottime: bool,
    need_deadprocs: bool,
    need_login: bool,
    need_initspawn: bool,
    need_clockchange: bool,
    need_runlevel: bool,
    need_users: bool,
    my_line_only: bool,
}

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    /// What to show, and the FILE named -- `None` for the live utmp.
    Run(Show, Option<OsString>),
}

fn help_text() -> String {
    format!(
        "\
Usage: who [OPTION]... [ FILE | ARG1 ARG2 ]
Print information about users who are currently logged in.

  -a, --all         same as -b -d --login -p -r -t -T -u
  -b, --boot        time of last system boot
  -d, --dead        print dead processes
  -H, --heading     print line of column headings
  -l, --login       print system login processes
      --lookup      attempt to canonicalize hostnames via DNS
  -m                only hostname and user associated with stdin
  -p, --process     print active processes spawned by init
  -q, --count       all login names and number of users logged on
  -r, --runlevel    print current runlevel
  -s, --short       print only name, line, and time (default)
  -t, --time        print last system clock change
  -T, -w, --mesg    add user's message status as +, - or ?
  -u, --users       list users logged in
      --message     same as -T
      --writable    same as -T
      --help        display this help and exit
      --version     output version information and exit

If FILE is not specified, use {UTMP_FILE}.  {WTMP_FILE} as FILE is common.
If ARG1 ARG2 given, -m presumed: 'am i' or 'mom likes' are usual.
"
    )
}

/// Upstream's option loop, its two defaults, and its operand count.
///
/// # Errors
///
/// An unknown option, or a third operand (`extra operand`, naming it).
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut show = Show::default();
    let mut assumptions = true;
    let mut operands: Vec<OsString> = Vec::new();
    for item in WHO.parse_aliased(args, SHORT_OPTIONS, LONG_OPTIONS, ALIASES) {
        match item? {
            Opt::Short(b'a', _) | Opt::Long("all", _) => {
                show.need_boottime = true;
                show.need_deadprocs = true;
                show.need_login = true;
                show.need_initspawn = true;
                show.need_runlevel = true;
                show.need_clockchange = true;
                show.need_users = true;
                show.include_mesg = true;
                show.include_idle = true;
                show.include_exit = true;
                assumptions = false;
            }
            Opt::Short(b'b', _) | Opt::Long("boot", _) => {
                show.need_boottime = true;
                assumptions = false;
            }
            Opt::Short(b'd', _) | Opt::Long("dead", _) => {
                show.need_deadprocs = true;
                show.include_idle = true;
                show.include_exit = true;
                assumptions = false;
            }
            Opt::Short(b'H', _) | Opt::Long("heading", _) => show.include_heading = true,
            Opt::Short(b'l', _) | Opt::Long("login", _) => {
                show.need_login = true;
                show.include_idle = true;
                assumptions = false;
            }
            Opt::Short(b'm', _) => show.my_line_only = true,
            Opt::Short(b'p', _) | Opt::Long("process", _) => {
                show.need_initspawn = true;
                assumptions = false;
            }
            Opt::Short(b'q', _) | Opt::Long("count", _) => show.short_list = true,
            Opt::Short(b'r', _) | Opt::Long("runlevel", _) => {
                show.need_runlevel = true;
                show.include_idle = true;
                assumptions = false;
            }
            Opt::Short(b's', _) | Opt::Long("short", _) => show.short_output = true,
            Opt::Short(b't', _) | Opt::Long("time", _) => {
                show.need_clockchange = true;
                assumptions = false;
            }
            Opt::Short(b'T' | b'w', _) | Opt::Long("message" | "mesg" | "writable", _) => {
                show.include_mesg = true;
            }
            Opt::Short(b'u', _) | Opt::Long("users", _) => {
                show.need_users = true;
                show.include_idle = true;
                assumptions = false;
            }
            Opt::Long("lookup", _) => show.do_lookup = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => operands.push(word.clone()),
            // Unreachable: every letter and name above is handled. Refused
            // rather than ignored, so one added without a handler fails loudly.
            Opt::Long(other, _) => {
                return Err(WHO.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(WHO.invalid_option(c)),
        }
    }
    if assumptions {
        show.need_users = true;
        show.short_output = true;
    }
    if show.include_exit {
        show.short_output = false;
    }
    let mut operands = operands.into_iter();
    let first = operands.next();
    let second = operands.next();
    if let Some(extra) = operands.next() {
        return Err(WHO.usage_referring(format!("extra operand {}", quote(&os_bytes(&extra)))));
    }
    let file = match (first, second) {
        // `who <blurf> <glop>`: "-m presumed", and the live utmp.
        (Some(_), Some(_)) => {
            show.my_line_only = true;
            None
        }
        (file, _) => file,
    };
    Ok(Request::Run(show, file))
}

/// `printf ("%-Ns", s)` for bytes: `s`, then spaces up to `width`; never cut.
fn pad(out: &mut Vec<u8>, s: &[u8], width: usize) {
    out.extend_from_slice(s);
    for _ in s.len()..width {
        out.push(b' ');
    }
}

/// `printf ("%Ns", s)`: spaces up to `width`, then `s`; never cut.
fn pad_left(out: &mut Vec<u8>, s: &[u8], width: usize) {
    for _ in s.len()..width {
        out.push(b' ');
    }
    out.extend_from_slice(s);
}

/// The fields of one line, as upstream's `print_line` takes them.
struct Line<'a> {
    user: &'a [u8],
    state: u8,
    line: &'a [u8],
    time: &'a [u8],
    idle: &'a [u8],
    pid: &'a [u8],
    comment: &'a [u8],
    exit: &'a [u8],
}

/// `IDLESTR_LEN`: the room an idle string has.
const IDLESTR_LEN: usize = 6;

/// `INT_STRLEN_BOUND (pid_t)`: the longest a pid can print, sign included.
const PID_STRLEN_BOUND: usize = 11;

/// Upstream's `print_line`: one line, trailing spaces cut.
fn print_line(out: &mut Vec<u8>, show: &Show, time_width: usize, f: &Line<'_>) {
    let mut buf = Vec::new();
    pad(&mut buf, f.user, 8);
    if show.include_mesg {
        buf.push(b' ');
        buf.push(f.state);
    }
    buf.push(b' ');
    pad(&mut buf, f.line, 12);
    buf.push(b' ');
    pad(&mut buf, f.time, time_width);
    // `strlen (idle) < sizeof x_idle - 1`: room for six and a space.
    if show.include_idle && !show.short_output && f.idle.len() < IDLESTR_LEN.saturating_add(1) {
        buf.push(b' ');
        pad(&mut buf, f.idle, 6);
    }
    if !show.short_output && f.pid.len() < PID_STRLEN_BOUND.saturating_add(1) {
        buf.push(b' ');
        pad_left(&mut buf, f.pid, 10);
    }
    buf.push(b' ');
    pad(&mut buf, f.comment, 8);
    if show.include_exit {
        buf.push(b' ');
        pad(&mut buf, f.exit, 12);
    }
    // "Remove any trailing spaces."
    let keep = buf
        .iter()
        .rposition(|&b| b != b' ')
        .map_or(0, |last| last.saturating_add(1));
    buf.truncate(keep);
    buf.push(b'\n');
    out.extend_from_slice(&buf);
}

/// Upstream's `idle_string`: `  .  ` under a minute, `HH:MM` under a day, and
/// ` old ` for anything older -- or from before the last boot, or the future.
fn idle_string(now: i64, when: i64, boottime: i64) -> Vec<u8> {
    if boottime < when && when <= now {
        // `ckd_sub` into an `int`: a difference too large for one is old.
        if let Some(idle) = now
            .checked_sub(when)
            .and_then(|d| i32::try_from(d).ok())
            .filter(|d| *d < 24 * 60 * 60)
        {
            if idle < 60 {
                return b"  .  ".to_vec();
            }
            return format!("{:02}:{:02}", idle / (60 * 60), (idle % (60 * 60)) / 60).into_bytes();
        }
    }
    b" old ".to_vec()
}

/// Upstream's `print_runlevel`: the line and the comment for a run-level entry,
/// whose pid holds the previous level times 256 plus the current one.
fn runlevel_fields(pid: i32) -> (Vec<u8>, Vec<u8>) {
    // C's `/` and `%` truncate toward zero, as Rust's do, and the narrowing
    // to `unsigned char` keeps the low byte, as `as u8` does.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (last, curr) = ((pid / 256) as u8, (pid % 256) as u8);
    let mut line = b"run-level ".to_vec();
    // `"%c"` of a NUL ends the string there.
    if curr != 0 {
        line.push(curr);
    }
    let comment = if (0x20..=0x7e).contains(&last) {
        let mut c = b"last=".to_vec();
        c.push(if last == b'N' { b'S' } else { last });
        c
    } else {
        Vec::new()
    };
    (line, comment)
}

/// The `e_termination` and `e_exit` halves of a dead process's exit status:
/// two C `short`s, the termination in the low half.
fn exit_halves(exit_status: u32) -> (i16, i16) {
    let bytes = exit_status.to_le_bytes();
    (
        i16::from_le_bytes([bytes[0], bytes[1]]),
        i16::from_le_bytes([bytes[2], bytes[3]]),
    )
}

#[cfg(unix)]
mod imp {
    use super::{
        Line, Request, Show, WHO, exit_halves, help_text, idle_string, parse_args, print_line,
        runlevel_fields,
    };
    use coreutils::errmsg::strerror;
    use coreutils::getopt::Report;
    use coreutils::locale::{Category, hard_locale};
    use coreutils::quote::{os_bytes, os_from_bytes, quotef};
    use coreutils::stdfd::{self, Stream};
    use coreutils::utmp::{
        self, BOOT_TIME, DEAD_PROCESS, INIT_PROCESS, LOGIN_PROCESS, NEW_TIME, RUN_LVL, Record,
        UTMP_FILE, Want,
    };
    use std::ffi::{CStr, OsString};
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;
    use std::process::ExitCode;

    /// `S_IWGRP`: a terminal its group may write to accepts messages.
    const S_IWGRP: u32 = 0o020;

    unsafe extern "C" {
        fn ttyname(fd: i32) -> *const std::ffi::c_char;
    }

    /// The name of the terminal on descriptor 0, or `None`.
    fn terminal_name() -> Option<Vec<u8>> {
        // SAFETY: `ttyname` takes a descriptor and returns either null or a
        // pointer to a NUL-terminated string in static storage, valid until
        // the next call to it; it is copied before anything else runs.
        unsafe {
            let p = ttyname(0);
            if p.is_null() {
                return None;
            }
            Some(CStr::from_ptr(p).to_bytes().to_vec())
        }
    }

    /// Seconds since the epoch, as upstream's `time (&now)`.
    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }

    /// What every line of one run shares.
    struct Run<'a> {
        show: &'a Show,
        zone: localtime::Zone,
        format: &'static [u8],
        width: usize,
        now: i64,
    }

    impl Run<'_> {
        /// Upstream's `time_string`: an entry's time in the format in force.
        fn time(&self, r: &Record) -> Vec<u8> {
            localtime::strftime(self.format, &self.zone.localtime(r.tv_sec, 0))
        }

        fn line(&self, out: &mut Vec<u8>, line: &Line<'_>) {
            print_line(out, self.show, self.width, line);
        }

        /// Upstream's `print_user`.
        fn user(&self, out: &mut Vec<u8>, r: &Record, boottime: i64) {
            let stat = utmp::tty_device(&r.tty)
                .and_then(|dev| std::fs::metadata(os_from_bytes(&dev)).ok());
            let (state, last_change) = match &stat {
                Some(m) => (if m.mode() & S_IWGRP != 0 { b'+' } else { b'-' }, m.atime()),
                None => (b'?', 0),
            };
            let idle: Vec<u8> = if last_change == 0 {
                b"  ?".to_vec()
            } else {
                idle_string(self.now, last_change, boottime)
            };
            let host = host_string(&r.host, self.show.do_lookup);
            let pid = r.pid.to_string();
            let time = self.time(r);
            self.line(
                out,
                &Line {
                    user: &r.user,
                    state,
                    line: &r.tty,
                    time: &time,
                    idle: &idle,
                    pid: pid.as_bytes(),
                    comment: &host,
                    exit: b"",
                },
            );
        }

        /// The lines that are a time and a label: boot and clock change.
        fn event(&self, out: &mut Vec<u8>, r: &Record, label: &[u8]) {
            let time = self.time(r);
            self.line(
                out,
                &Line {
                    user: b"",
                    state: b' ',
                    line: label,
                    time: &time,
                    idle: b"",
                    pid: b"",
                    comment: b"",
                    exit: b"",
                },
            );
        }

        /// `print_initspawn`, `print_login` and `print_deadprocs`: a process,
        /// its `id=` comment, and for the dead its exit status.
        fn process(&self, out: &mut Vec<u8>, r: &Record, user: &[u8], exit: &[u8]) {
            let mut comment = b"id=".to_vec();
            comment.extend_from_slice(&r.id);
            let pid = r.pid.to_string();
            let time = self.time(r);
            self.line(
                out,
                &Line {
                    user,
                    state: b' ',
                    line: &r.tty,
                    time: &time,
                    idle: b"",
                    pid: pid.as_bytes(),
                    comment: &comment,
                    exit,
                },
            );
        }

        /// `print_runlevel`.
        fn runlevel(&self, out: &mut Vec<u8>, r: &Record) {
            let (line, comment) = runlevel_fields(r.pid);
            let time = self.time(r);
            self.line(
                out,
                &Line {
                    user: b"",
                    state: b' ',
                    line: &line,
                    time: &time,
                    idle: b"",
                    pid: b"",
                    comment: &comment,
                    exit: b"",
                },
            );
        }
    }

    /// The comment `print_user` gives a session: `(host)`, or `(host:display)`
    /// for an X display, the host canonicalised first with `--lookup`.
    fn host_string(ut_host: &[u8], lookup: bool) -> Vec<u8> {
        if ut_host.is_empty() {
            return Vec::new();
        }
        let (host, display) = utmp::split_display(ut_host);
        let canon = if lookup && !host.is_empty() {
            utmp::canon_host(host)
        } else {
            None
        };
        let mut s = b"(".to_vec();
        s.extend_from_slice(canon.as_deref().unwrap_or(host));
        if let Some(display) = display {
            s.push(b':');
            s.extend_from_slice(display);
        }
        s.push(b')');
        s
    }

    /// Upstream's `scan_entries`.
    fn scan_entries(out: &mut Vec<u8>, run: &Run<'_>, records: &[Record]) {
        let show = run.show;
        if show.include_heading {
            run.line(
                out,
                &Line {
                    user: b"NAME",
                    state: b' ',
                    line: b"LINE",
                    time: b"TIME",
                    idle: b"IDLE",
                    pid: b"PID",
                    comment: b"COMMENT",
                    exit: b"EXIT",
                },
            );
        }
        let my_line = if show.my_line_only {
            let Some(name) = terminal_name() else {
                return;
            };
            Some(
                name.strip_prefix(b"/dev/")
                    .map_or_else(|| name.clone(), <[u8]>::to_vec),
            )
        } else {
            None
        };
        let mut boottime = i64::MIN;
        for r in records {
            if my_line.as_ref().is_none_or(|line| *line == r.tty) {
                let t = r.record_type;
                if show.need_users && utmp::is_user_process(r) {
                    run.user(out, r, boottime);
                } else if show.need_runlevel && t == RUN_LVL {
                    run.runlevel(out, r);
                } else if show.need_boottime && t == BOOT_TIME {
                    run.event(out, r, b"system boot");
                } else if show.need_clockchange && t == NEW_TIME {
                    run.event(out, r, b"clock change");
                } else if show.need_initspawn && t == INIT_PROCESS {
                    run.process(out, r, b"", b"");
                } else if show.need_login && t == LOGIN_PROCESS {
                    run.process(out, r, b"LOGIN", b"");
                } else if show.need_deadprocs && t == DEAD_PROCESS {
                    let (term, exit) = exit_halves(r.exit_status);
                    let status = format!("term={term} exit={exit}");
                    run.process(out, r, b"", status.as_bytes());
                }
            }
            if r.record_type == BOOT_TIME {
                boottime = r.tv_sec;
            }
        }
    }

    /// Upstream's `list_entries_who`: the names, then the count.
    fn list_entries(out: &mut Vec<u8>, records: &[Record]) {
        let mut count: usize = 0;
        for r in records.iter().filter(|r| utmp::is_user_process(r)) {
            if count > 0 {
                out.push(b' ');
            }
            out.extend_from_slice(utmp::trimmed_name(r));
            count = count.saturating_add(1);
        }
        out.extend_from_slice(format!("\n# users={count}\n").as_bytes());
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(e) => {
                WHO.report(&e);
                return ExitCode::FAILURE;
            }
        };
        let mut out = Stream::stdout();
        // Each write is deliberately unread: a failed write is `Stream`'s to
        // remember and `close_stdout`'s to report, once.
        let (show, file) = match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
                return stdfd::close_stdout("who", out, ExitCode::SUCCESS);
            }
            Request::Version => {
                let _ = out.write_all(b"who (SlateOS coreutils) 0.1.0\n");
                return stdfd::close_stdout("who", out, ExitCode::SUCCESS);
            }
            Request::Run(show, file) => (show, file),
        };
        // `who (UTMP_FILE, READ_UTMP_CHECK_PIDS)` for the live utmp, `who
        // (file, 0)` for one named; and `-q` asks for user processes only.
        let named = file.is_some();
        let path = file.map_or_else(
            || UTMP_FILE.as_bytes().to_vec(),
            |f| os_bytes(&f).into_owned(),
        );
        let want = Want {
            users_only: show.short_list,
            live_only: !named,
        };
        let records = match utmp::read_utmp(&path, named, want) {
            Ok(records) => records,
            Err(e) => {
                coreutils::diag!("who: {}: {}", quotef(&path), strerror(&e));
                return stdfd::close_stdout("who", out, ExitCode::FAILURE);
            }
        };
        let mut text = Vec::new();
        if show.short_list {
            list_entries(&mut text, &records);
        } else {
            let (format, width): (&'static [u8], usize) = if hard_locale(Category::Time) {
                (b"%Y-%m-%d %H:%M", 16)
            } else {
                (b"%b %e %H:%M", 12)
            };
            let run = Run {
                show: &show,
                zone: localtime::Zone::from_env(),
                format,
                width,
                now: now(),
            };
            scan_entries(&mut text, &run, &records);
        }
        let _ = out.write_all(&text);
        stdfd::close_stdout("who", out, ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no utmp.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("who: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn run(words: &[&str]) -> (Show, Option<OsString>) {
        match parse_args(&argv(words)).unwrap() {
            Request::Run(show, file) => (show, file),
            other => panic!("wanted a run, got {other:?}"),
        }
    }

    #[test]
    fn the_default_is_users_in_the_short_form() {
        let (show, file) = run(&[]);
        assert!(show.need_users && show.short_output);
        assert!(!show.include_idle && !show.include_exit);
        assert_eq!(file, None);
    }

    #[test]
    fn all_is_every_kind_of_entry_and_the_long_form() {
        let (show, _) = run(&["-a"]);
        assert!(show.need_boottime && show.need_deadprocs && show.need_login);
        assert!(show.need_initspawn && show.need_runlevel && show.need_clockchange);
        assert!(show.need_users && show.include_mesg && show.include_idle);
        assert!(show.include_exit && !show.short_output);
    }

    /// `-s` asks for the short form, and the exit column overrides it.
    #[test]
    fn the_exit_column_overrides_short() {
        let (show, _) = run(&["-s", "-d"]);
        assert!(!show.short_output && show.include_exit);
        let (show, _) = run(&["-b", "-s"]);
        assert!(show.short_output && show.need_boottime && !show.need_users);
    }

    #[test]
    fn two_operands_are_am_i_on_the_live_utmp() {
        let (show, file) = run(&["am", "i"]);
        assert!(show.my_line_only);
        assert_eq!(file, None);
        let (show, file) = run(&["/var/log/wtmp"]);
        assert!(!show.my_line_only);
        assert_eq!(file, Some(OsString::from("/var/log/wtmp")));
        let e = parse_args(&argv(&["a", "b", "c"])).unwrap_err();
        assert_eq!(
            e.message(),
            "extra operand \u{2018}c\u{2019}\nTry 'who --help' for more information."
        );
    }

    /// The three names of `-T` are one option, so a prefix of two of them is
    /// not ambiguous; `--l` is, between `--login` and `--lookup`.
    #[test]
    fn the_names_of_mesg_are_one_option() {
        assert!(run(&["--me"]).0.include_mesg);
        assert!(run(&["--w"]).0.include_mesg);
        assert!(run(&["-w"]).0.include_mesg);
        let e = parse_args(&argv(&["--l"])).unwrap_err();
        assert!(
            e.message().starts_with("option '--l' is ambiguous"),
            "{}",
            e.message()
        );
    }

    #[test]
    fn a_line_is_upstream_s_columns_with_trailing_spaces_cut() {
        let show = Show {
            include_mesg: true,
            include_idle: true,
            include_exit: true,
            ..Show::default()
        };
        let mut out = Vec::new();
        print_line(
            &mut out,
            &show,
            16,
            &Line {
                user: b"alice",
                state: b'+',
                line: b"pts/0",
                time: b"2023-11-14 22:16",
                idle: b" old ",
                pid: b"102",
                comment: b"(10.0.0.2)",
                exit: b"",
            },
        );
        assert_eq!(
            out,
            b"alice    + pts/0        2023-11-14 22:16  old          102 (10.0.0.2)\n"
        );
    }

    #[test]
    fn idle_is_dot_hours_and_minutes_or_old() {
        let now = 1_000_000;
        assert_eq!(idle_string(now, now - 30, 0), b"  .  ");
        assert_eq!(idle_string(now, now - 3 * 3600 - 7 * 60, 0), b"03:07");
        assert_eq!(idle_string(now, now - 86_400, 0), b" old ");
        assert_eq!(idle_string(now, now + 5, 0), b" old ", "the future");
        assert_eq!(idle_string(now, now - 30, now), b" old ", "before the boot");
    }

    #[test]
    fn a_runlevel_entry_reads_its_pid_as_two_levels() {
        // 'N' * 256 + '5': no previous level, now 5 -- printed as last=S.
        assert_eq!(
            runlevel_fields(i32::from(b'N') * 256 + i32::from(b'5')),
            (b"run-level 5".to_vec(), b"last=S".to_vec())
        );
        assert_eq!(
            runlevel_fields(i32::from(b'3') * 256 + i32::from(b'5')),
            (b"run-level 5".to_vec(), b"last=3".to_vec())
        );
        // An unprintable previous level has no comment; a NUL level, no digit.
        assert_eq!(
            runlevel_fields(5 * 256),
            (b"run-level ".to_vec(), Vec::new())
        );
    }

    #[test]
    fn a_dead_process_s_status_is_two_signed_shorts() {
        assert_eq!(exit_halves(0x0002_0001), (1, 2));
        assert_eq!(exit_halves(0xffff_ffff), (-1, -1));
    }
}
