//! `pinky` — a lightweight `finger`: who is logged in, or what is known about
//! named users.
//!
//! ```text
//! Usage: pinky [OPTION]... [USER]...
//! ```
//!
//! A port of GNU coreutils 9.4's `src/pinky.c`. The login records come from the
//! shared `utmpfile` crate and the user records from `pwdb`, as for `users`
//! and `id`; this file is what `pinky` does with them. It replaces the `pinky`
//! personality of `userspace/finger`, which nothing installed under that name
//! (`scripts/multicall-aliases-baseline.txt`), and which printed finger's
//! layout under pinky's options -- `-b` hid the plan where it should hide the
//! home directory, and `-w` and `-i` were accepted and ignored.
//!
//! # The two formats
//!
//! **Short** (the default, `-s`): a heading, then one line per login session
//! -- every `USER_PROCESS` record with a name in `/var/run/utmp`, or only those
//! of the USERs named:
//!
//! ```text
//! Login    Name                 TTY      Idle   When             Where
//! alice    Alice Liddell       *pts/0    00:07  2026-09-25 09:14 10.0.0.2
//! ```
//!
//! The name is the GECOS field up to its first comma, each `&` in it replaced
//! by the login name with its first letter capitalised. The character before
//! the terminal is ` ` if the terminal accepts messages (group-writable), `*`
//! if it does not, `?` if it cannot be examined. Idle is the time since the
//! terminal was last read: blank under a minute, `HH:MM` under a day, else
//! `Nd`. When is the login time, `%Y-%m-%d %H:%M` unless `LC_TIME` is exactly
//! `C` or `POSIX` ([`coreutils::locale`]), where it is `%b %e %H:%M`. Where is
//! the remote host, canonicalised through the resolver, with an X display
//! suffix kept.
//!
//! **Long** (`-l`, which needs USERs): for each, whether logged in or not, the
//! login name, real name, home directory and shell, then `~/.project` and
//! `~/.plan` verbatim when they can be opened.
//!
//! # Reading the login records
//!
//! As `users` reads them, and for the reason its docs give: gnulib's own
//! reader, not glibc's `getutxent`, which cannot report a failure. A missing
//! `/var/run/utmp` is a machine with nobody logged in; one that is there and
//! cannot be read is `pinky: /var/run/utmp: <reason>`, status 1, where GNU on
//! glibc would print the heading alone. Unlike `users`, a session whose
//! process has died is still listed: `pinky` asks gnulib for user processes,
//! not for live ones.
//!
//! # Checked against GNU
//!
//! `scripts/pinky-diff.sh`.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{self, Opt, Program, Takes};
use std::ffi::OsString;

coreutils::guard_std_fds!();

const PINKY: Program = Program::new("pinky", 1);

/// glibc's `_PATH_UTMP`, which gnulib's `UTMP_FILE` is.
const UTMP_FILE: &str = "/var/run/utmp";

/// `parse_gnu_standard_options_only`'s table, which is upstream's `longopts`.
const LONG_OPTIONS: &[(&str, Takes)] = &[("help", Takes::Nothing), ("version", Takes::Nothing)];

/// Upstream's file-scope `include_*` and `do_short_format` switches.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's eight independent switches, one per option letter; a bitset would only rename them"
)]
struct Show {
    idle: bool,
    heading: bool,
    fullname: bool,
    project: bool,
    plan: bool,
    home_and_shell: bool,
    short: bool,
    where_: bool,
}

impl Default for Show {
    fn default() -> Self {
        Show {
            idle: true,
            heading: true,
            fullname: true,
            project: true,
            plan: true,
            home_and_shell: true,
            short: true,
            where_: true,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Run(Show, Vec<OsString>),
}

fn help_text() -> String {
    format!(
        "\
Usage: pinky [OPTION]... [USER]...

  -l              produce long format output for the specified USERs
  -b              omit the user's home directory and shell in long format
  -h              omit the user's project file in long format
  -p              omit the user's plan file in long format
  -s              do short format output, this is the default
  -f              omit the line of column headings in short format
  -w              omit the user's full name in short format
  -i              omit the user's full name and remote host in short format
  -q              omit the user's full name, remote host and idle time
                  in short format
      --help        display this help and exit
      --version     output version information and exit

A lightweight 'finger' program;  print user information.
The utmp file will be {UTMP_FILE}.
"
    )
}

/// Upstream's option loop over `"sfwiqbhlp"`, then its check that `-l` has
/// somebody to describe.
///
/// # Errors
///
/// An unknown option, or `-l` with no USER.
fn parse_args(args: &[OsString]) -> Result<Request, getopt::Error> {
    let mut show = Show::default();
    let mut users = Vec::new();
    for item in PINKY.parse(args, "sfwiqbhlp", LONG_OPTIONS) {
        match item? {
            Opt::Short(b's', _) => show.short = true,
            Opt::Short(b'l', _) => show.short = false,
            Opt::Short(b'f', _) => show.heading = false,
            Opt::Short(b'w', _) => show.fullname = false,
            Opt::Short(b'i', _) => {
                show.fullname = false;
                show.where_ = false;
            }
            Opt::Short(b'q', _) => {
                show.fullname = false;
                show.where_ = false;
                show.idle = false;
            }
            Opt::Short(b'h', _) => show.project = false,
            Opt::Short(b'p', _) => show.plan = false,
            Opt::Short(b'b', _) => show.home_and_shell = false,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Operand(word) => users.push(word.clone()),
            // Unreachable: every letter of the spec and both names of the
            // table are handled above.
            Opt::Long(other, _) => {
                return Err(PINKY.usage_referring(format!("option '--{other}' is unhandled")));
            }
            Opt::Short(c, _) => return Err(PINKY.invalid_option(c)),
        }
    }
    if !show.short && users.is_empty() {
        return Err(PINKY.usage_referring(
            "no username specified; at least one must be specified when using -l".to_string(),
        ));
    }
    Ok(Request::Run(show, users))
}

/// Upstream's `create_fullname`: `gecos` (already cut at its first comma) with
/// each `&` replaced by the login name, whose first letter is capitalised if
/// it is a lower-case ASCII letter.
fn create_fullname(gecos: &[u8], user: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(gecos.len());
    for &c in gecos {
        if c == b'&' {
            let mut rest = user;
            if let Some((&first, tail)) = user.split_first()
                && first.is_ascii_lowercase()
            {
                out.push(first.to_ascii_uppercase());
                rest = tail;
            }
            out.extend_from_slice(rest);
        } else {
            out.push(c);
        }
    }
    out
}

/// The GECOS field up to its first comma: the part upstream hands to
/// [`create_fullname`] after writing a NUL over the comma.
fn gecos_name(gecos: &[u8]) -> &[u8] {
    gecos.split(|&c| c == b',').next().unwrap_or(gecos)
}

/// Upstream's `idle_string`: blank under a minute, `HH:MM` under a day, else
/// whole days and `d`. A terminal touched after `now` counts as under a minute.
fn idle_string(now: i64, when: i64) -> String {
    let idle = now.saturating_sub(when);
    if idle < 60 {
        "     ".to_string()
    } else if idle < 24 * 60 * 60 {
        format!("{:02}:{:02}", idle / 3600, idle % 3600 / 60)
    } else {
        format!("{}d", idle / (24 * 60 * 60))
    }
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

/// `printf ("%-N.Ns", s)`: cut at `width` bytes -- a precision counts bytes --
/// and padded to it.
fn pad_cut(out: &mut Vec<u8>, s: &[u8], width: usize) {
    pad(out, s.get(..width).unwrap_or(s), width);
}

/// Upstream's `print_heading`. `time_width` is the width of a login time in the
/// format in force, so that `When` heads its column exactly.
fn heading(out: &mut Vec<u8>, show: &Show, time_width: usize) {
    pad(out, b"Login", 8);
    if show.fullname {
        out.push(b' ');
        pad(out, b"Name", 19);
    }
    out.push(b' ');
    pad(out, b" TTY", 9);
    if show.idle {
        out.push(b' ');
        pad(out, b"Idle", 6);
    }
    out.push(b' ');
    pad(out, b"When", time_width);
    if show.where_ {
        out.extend_from_slice(b" Where");
    }
    out.push(b'\n');
}

/// The device a `ut_line` names, relative to `/dev` unless absolute: the part
/// after the first space if there is one ("If ut_line contains a space, the
/// device name starts after the space"). `None` for an empty name, which
/// upstream's `fstatat` rejects -- joined to `/dev/` it would stat `/dev`.
fn tty_device(line: &[u8]) -> Option<Vec<u8>> {
    let name = match line.iter().position(|&c| c == b' ') {
        Some(at) => line.get(at.saturating_add(1)..).unwrap_or_default(),
        None => line,
    };
    if name.is_empty() {
        return None;
    }
    if name.first() == Some(&b'/') {
        return Some(name.to_vec());
    }
    let mut path = b"/dev/".to_vec();
    path.extend_from_slice(name);
    Some(path)
}

/// A `ut_host` split at its first `:` into the host and the X display after it.
fn split_display(host: &[u8]) -> (&[u8], Option<&[u8]>) {
    match host.iter().position(|&c| c == b':') {
        Some(at) => (
            host.get(..at).unwrap_or_default(),
            Some(host.get(at.saturating_add(1)..).unwrap_or_default()),
        ),
        None => (host, None),
    }
}

#[cfg(unix)]
mod imp {
    use super::{
        PINKY, Request, Show, UTMP_FILE, create_fullname, gecos_name, heading, help_text,
        idle_string, pad, pad_cut, pad_left, parse_args, split_display, tty_device,
    };
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::locale::{Category, hard_locale};
    use coreutils::quote::{os_bytes, os_from_bytes, quotef};
    use coreutils::stdfd::{self, Stream};
    use std::ffi::OsString;
    use std::io::{Read, Write};
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;
    use std::process::ExitCode;
    use utmpfile::{Record, USER_PROCESS};

    /// `S_IWGRP`: a terminal whose group may write to it accepts `write(1)`.
    const S_IWGRP: u32 = 0o020;

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let request = match parse_args(&args) {
            Ok(r) => r,
            Err(e) => {
                PINKY.report(&e);
                return ExitCode::FAILURE;
            }
        };
        let mut out = Stream::stdout();
        // Each write is deliberately unread: a failed write is `Stream`'s to
        // remember and `close_stdout`'s to report, once.
        let (show, users) = match request {
            Request::Help => {
                let _ = out.write_all(help_text().as_bytes());
                return stdfd::close_stdout("pinky", out, ExitCode::SUCCESS);
            }
            Request::Version => {
                let _ = out.write_all(b"pinky (SlateOS coreutils) 0.1.0\n");
                return stdfd::close_stdout("pinky", out, ExitCode::SUCCESS);
            }
            Request::Run(show, users) => (show, users),
        };
        let users: Vec<Vec<u8>> = users.iter().map(|u| os_bytes(u).into_owned()).collect();
        let db = pwdb::Db::load();

        if show.short {
            // A system with no utmp has nobody logged in; one whose utmp is
            // there and cannot be read does not.
            let data = match optionalfile::read_bytes_or_empty(Path::new(UTMP_FILE)) {
                Ok(d) => d,
                Err(e) => {
                    diag!("pinky: {}: {}", quotef(UTMP_FILE.as_bytes()), strerror(&e));
                    return ExitCode::FAILURE;
                }
            };
            let records = utmpfile::parse(&data);
            let _ = out.write_all(&short_pinky(&show, &db, &records, &users));
        } else {
            for user in &users {
                let _ = out.write_all(&long_entry(&show, &db, user));
            }
        }
        stdfd::close_stdout("pinky", out, ExitCode::SUCCESS)
    }

    /// Upstream's `scan_entries`: the heading, then a line for each named
    /// user's sessions, or for every session when nobody is named.
    fn short_pinky(show: &Show, db: &pwdb::Db, records: &[Record], users: &[Vec<u8>]) -> Vec<u8> {
        let (time_format, time_width): (&[u8], usize) = if hard_locale(Category::Time) {
            (b"%Y-%m-%d %H:%M", 16)
        } else {
            (b"%b %e %H:%M", 12)
        };
        let mut out = Vec::new();
        if show.heading {
            heading(&mut out, show, time_width);
        }
        let zone = localtime::Zone::from_env();
        let now = now();
        for r in records {
            // gnulib's `IS_USER_PROCESS`.
            if r.record_type != USER_PROCESS || r.user.is_empty() {
                continue;
            }
            if !users.is_empty() && !users.contains(&r.user) {
                continue;
            }
            entry(&mut out, show, db, r, &zone, now, time_format);
        }
        out
    }

    /// Seconds since the epoch, as upstream's `time (&now)`.
    fn now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
    }

    /// Upstream's `print_entry`: one session's line.
    fn entry(
        out: &mut Vec<u8>,
        show: &Show,
        db: &pwdb::Db,
        r: &Record,
        zone: &localtime::Zone,
        now: i64,
        time_format: &[u8],
    ) {
        let stat = tty_device(&r.tty).and_then(|dev| std::fs::metadata(os_from_bytes(&dev)).ok());
        let (mesg, last_change) = match stat {
            Some(m) => (if m.mode() & S_IWGRP != 0 { b' ' } else { b'*' }, m.atime()),
            None => (b'?', 0),
        };

        // `%-8s`, never cut: a longer name pushes the rest of the line over.
        pad(out, &r.user, 8);

        if show.fullname {
            out.push(b' ');
            match db.user_by_name(&r.user) {
                // `" %19s"`: right-aligned, unlike the name it stands in for.
                None => pad_left(out, b"        ???", 19),
                Some(pw) => pad_cut(out, &create_fullname(gecos_name(&pw.gecos), &pw.name), 19),
            }
        }

        out.push(b' ');
        out.push(mesg);
        pad(out, &r.tty, 8);

        if show.idle {
            out.push(b' ');
            if last_change == 0 {
                pad(out, b"?????", 6);
            } else {
                pad(out, idle_string(now, last_change).as_bytes(), 6);
            }
        }

        out.push(b' ');
        // `utmpfile` gives a negative `ut_tv.tv_sec` as 0; see its docs.
        let login = i64::try_from(r.login_time).unwrap_or(i64::MAX);
        out.extend_from_slice(&localtime::strftime(time_format, &zone.local(login, 0)));

        if show.where_ && !r.host.is_empty() {
            let (host, display) = split_display(&r.host);
            let canon = if host.is_empty() {
                None
            } else {
                canon_host(host)
            };
            out.push(b' ');
            out.extend_from_slice(canon.as_deref().unwrap_or(host));
            if let Some(display) = display {
                out.push(b':');
                out.extend_from_slice(display);
            }
        }
        out.push(b'\n');
    }

    /// gnulib's `canon_host`: the resolver's canonical name for `host`, or
    /// `None` when it has none to give -- upstream then prints `host` as it is.
    ///
    /// Through the C library's `getaddrinfo` because that is the resolver:
    /// `/etc/hosts`, DNS and whatever `nsswitch.conf` adds. On SlateOS the
    /// answer is currently the query echoed back (`known-issues.md` ->
    /// `B-HOSTNAME-RESOLVES-THE-DOMAIN-WITHOUT-ETC-HOSTS`), which prints the
    /// host unchanged -- the same line as not asking.
    fn canon_host(host: &[u8]) -> Option<Vec<u8>> {
        /// `struct addrinfo`, as glibc lays it out and as `posix::socket`
        /// declares it (`scripts/check-libc-abi.py` holds the two together).
        #[repr(C)]
        struct AddrInfo {
            ai_flags: i32,
            ai_family: i32,
            ai_socktype: i32,
            ai_protocol: i32,
            ai_addrlen: u32,
            ai_addr: *mut u8,
            ai_canonname: *mut u8,
            ai_next: *mut AddrInfo,
        }
        /// `AI_CANONNAME`: 2 in glibc and in `posix::socket`.
        const AI_CANONNAME: i32 = 2;
        unsafe extern "C" {
            fn getaddrinfo(
                node: *const u8,
                service: *const u8,
                hints: *const AddrInfo,
                res: *mut *mut AddrInfo,
            ) -> i32;
            fn freeaddrinfo(res: *mut AddrInfo);
        }

        // A name with a NUL in it cannot be passed as a C string; `ut_host`
        // is cut at its first NUL, so this cannot happen, but it is checked
        // rather than assumed because the pointer below depends on it.
        if host.contains(&0) {
            return None;
        }
        let mut node = host.to_vec();
        node.push(0);
        let hints = AddrInfo {
            ai_flags: AI_CANONNAME,
            ai_family: 0,
            ai_socktype: 0,
            ai_protocol: 0,
            ai_addrlen: 0,
            ai_addr: std::ptr::null_mut(),
            ai_canonname: std::ptr::null_mut(),
            ai_next: std::ptr::null_mut(),
        };
        let mut res: *mut AddrInfo = std::ptr::null_mut();
        // SAFETY: `node` is NUL-terminated and outlives the call; `service` may
        // be null when `node` is not; `hints` is a valid `addrinfo` with only
        // the flags set; `res` is a valid place for the result pointer.
        let rc = unsafe {
            getaddrinfo(
                node.as_ptr(),
                std::ptr::null(),
                &raw const hints,
                &raw mut res,
            )
        };
        if rc != 0 || res.is_null() {
            return None;
        }
        // SAFETY: `res` is the non-null list `getaddrinfo` returned. Its first
        // entry's `ai_canonname`, when set, is a NUL-terminated string owned by
        // the list; it is copied out before the list is freed, exactly once.
        unsafe {
            let canon = (*res).ai_canonname;
            let copied = if canon.is_null() {
                None
            } else {
                // `CStr` rather than a declared `strlen`: rustc's
                // `suspicious_runtime_symbol_definitions` rejects a
                // redeclaration of a symbol the runtime itself links whose
                // pointer type differs from the runtime's own.
                Some(std::ffi::CStr::from_ptr(canon.cast()).to_bytes().to_vec())
            };
            freeaddrinfo(res);
            copied
        }
    }

    /// Upstream's `print_long_entry`: everything known about one user.
    fn long_entry(show: &Show, db: &pwdb::Db, name: &[u8]) -> Vec<u8> {
        let mut out = b"Login name: ".to_vec();
        pad(&mut out, name, 28);
        out.extend_from_slice(b"In real life: ");
        let Some(pw) = db.user_by_name(name) else {
            out.extend_from_slice(b" ???\n");
            return out;
        };
        out.push(b' ');
        out.extend_from_slice(&create_fullname(gecos_name(&pw.gecos), &pw.name));
        out.push(b'\n');

        if show.home_and_shell {
            out.extend_from_slice(b"Directory: ");
            pad(&mut out, &pw.dir, 29);
            out.extend_from_slice(b"Shell: ");
            out.push(b' ');
            out.extend_from_slice(&pw.shell);
            out.push(b'\n');
        }
        if show.project {
            dump(&mut out, &pw.dir, b"/.project", b"Project: ");
        }
        if show.plan {
            dump(&mut out, &pw.dir, b"/.plan", b"Plan:\n");
        }
        out.push(b'\n');
        out
    }

    /// Upstream's `fopen`-then-`fread` of one of the user's files: `label` as
    /// soon as the file opens, then as much of it as can be read. A file that
    /// will not open prints nothing; one that opens and then fails -- a
    /// directory, an I/O error part way -- keeps its label and what was read
    /// before the failure, as upstream's loop, which stops at the first short
    /// `fread` without asking why, does.
    fn dump(out: &mut Vec<u8>, dir: &[u8], file: &[u8], label: &[u8]) {
        let mut path = dir.to_vec();
        path.extend_from_slice(file);
        let Ok(mut f) = std::fs::File::open(os_from_bytes(&path)) else {
            return;
        };
        out.extend_from_slice(label);
        // Ignored on purpose, per the doc comment: `read_to_end` keeps what it
        // read before an error, and upstream says nothing about the error.
        let _ = f.read_to_end(out);
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
    coreutils::diag!("pinky: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn show_for(words: &[&str]) -> Show {
        match parse_args(&args(words)).unwrap() {
            Request::Run(show, _) => show,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn fullname_replaces_ampersands_with_the_capitalised_login() {
        assert_eq!(create_fullname(b"& Liddell", b"alice"), b"Alice Liddell");
        assert_eq!(create_fullname(b"&&", b"bob"), b"BobBob");
        assert_eq!(create_fullname(b"x&", b"Zed"), b"xZed");
        assert_eq!(create_fullname(b"plain", b"u"), b"plain");
        // Only an ASCII lower-case letter is capitalised.
        assert_eq!(create_fullname(b"&", b"\xc3\xa9t"), b"\xc3\xa9t");
        assert_eq!(create_fullname(b"&", b""), b"");
    }

    #[test]
    fn gecos_stops_at_the_first_comma() {
        assert_eq!(gecos_name(b"Name,Room,Phone"), b"Name");
        assert_eq!(gecos_name(b",Room"), b"");
        assert_eq!(gecos_name(b""), b"");
    }

    #[test]
    fn idle_strings() {
        assert_eq!(idle_string(100, 90), "     ");
        assert_eq!(idle_string(100, 200), "     ", "a future access time");
        assert_eq!(idle_string(4000, 0), "01:06");
        assert_eq!(idle_string(86_399, 0), "23:59");
        assert_eq!(idle_string(86_400, 0), "1d");
        assert_eq!(idle_string(3 * 86_400 + 5, 0), "3d");
    }

    #[test]
    fn the_three_paddings() {
        let mut out = Vec::new();
        pad(&mut out, b"ab", 4);
        pad(&mut out, b"abcdef", 4);
        assert_eq!(out, b"ab  abcdef");
        let mut out = Vec::new();
        pad_left(&mut out, b"ab", 4);
        pad_left(&mut out, b"abcdef", 4);
        assert_eq!(out, b"  ababcdef");
        let mut out = Vec::new();
        pad_cut(&mut out, b"ab", 4);
        pad_cut(&mut out, b"abcdef", 4);
        assert_eq!(out, b"ab  abcd");
    }

    /// The unknown-user filler is right-aligned: upstream's `" %19s"`.
    #[test]
    fn an_unknown_name_is_right_aligned() {
        let mut out = Vec::new();
        pad_left(&mut out, b"        ???", 19);
        assert_eq!(out, b"                ???");
    }

    #[test]
    fn the_heading_follows_the_switches() {
        let mut out = Vec::new();
        heading(&mut out, &Show::default(), 16);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Login    Name                 TTY      Idle   When             Where\n"
        );
        let mut out = Vec::new();
        heading(&mut out, &show_for(&["-q"]), 12);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "Login     TTY      When        \n"
        );
    }

    #[test]
    fn the_device_is_under_dev_unless_absolute() {
        assert_eq!(tty_device(b"pts/0"), Some(b"/dev/pts/0".to_vec()));
        assert_eq!(tty_device(b"/dev/tty1"), Some(b"/dev/tty1".to_vec()));
        assert_eq!(tty_device(b"x pts/3"), Some(b"/dev/pts/3".to_vec()));
        // Not `/dev` itself, which upstream's `fstatat (fd, "")` never stats.
        assert_eq!(tty_device(b""), None);
        assert_eq!(tty_device(b"x "), None);
    }

    #[test]
    fn a_display_is_split_off_the_host() {
        assert_eq!(split_display(b"host"), (&b"host"[..], None));
        assert_eq!(
            split_display(b"host:0.0"),
            (&b"host"[..], Some(&b"0.0"[..]))
        );
        assert_eq!(split_display(b":0"), (&b""[..], Some(&b"0"[..])));
    }

    #[test]
    fn long_format_needs_users() {
        let e = parse_args(&args(&["-l"])).unwrap_err();
        assert!(e.sentence.contains("no username specified"));
        assert_eq!(e.status, 1);
        // ...and `-s` after it restores the short format, which does not.
        assert!(parse_args(&args(&["-l", "-s"])).is_ok());
    }

    #[test]
    fn the_column_switches() {
        let q = show_for(&["-q"]);
        assert!(!q.fullname && !q.where_ && !q.idle && q.heading);
        let i = show_for(&["-i"]);
        assert!(!i.fullname && !i.where_ && i.idle);
        let w = show_for(&["-w"]);
        assert!(!w.fullname && w.where_);
        assert!(!show_for(&["-f"]).heading);
        let long = show_for(&["-lbhp", "root"]);
        assert!(!long.short && !long.home_and_shell && !long.project && !long.plan);
    }

    #[test]
    fn help_and_version_win_where_they_appear() {
        assert_eq!(parse_args(&args(&["-l", "--help"])).unwrap(), Request::Help);
        assert_eq!(parse_args(&args(&["--version"])).unwrap(), Request::Version);
        assert!(parse_args(&args(&["-x"])).is_err());
    }
}
