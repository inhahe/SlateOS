//! `write` -- send a message to another user: util-linux 2.39.3's, ported.
//!
//! ```text
//! write [options] <user> [<ttyname>]
//! ```
//!
//! A transcription of `term-utils/write.c`. The sender's terminal is the
//! first of standard input, output and error that is one; the recipient's is
//! named, or chosen from utmp. Standard output is then reopened on it, a
//! greeting written, and each line of standard input copied through
//! `fputs_careful`, then `EOF`. What upstream does and this keeps:
//!
//! - **The sender's terminal must take messages**: unless root, the
//!   effective gid must own it and it must be group-writable -- `you have
//!   write permission turned off` -- and with no terminal at all the sender
//!   is `<no tty>`.
//! - **The recipient's terminal**: named, it must be in utmp under that user
//!   (any entry type); chosen, it is the user's `USER_PROCESS` entry with the
//!   most recent access time among those that take messages, the sender's
//!   own terminal skipped unless it is the only one -- and a terminal whose
//!   group is not the sender's effective gid is complained about even while
//!   searching.
//! - **The arguments are counted as `argc` counts them**, after glibc's
//!   `getopt` has permuted them: a `--` is one of them, and is moved in front,
//!   so `write alice --` names the user `--` on the terminal `alice`.
//! - **The greeting** names `getlogin ()` and, when the real uid's name
//!   differs, that one too: `Message from LOGIN@HOST (as USER) on TTY at
//!   HH:MM ...`.
//! - **`SIGINT` and `SIGHUP`** end the copying, without restarting the read
//!   they interrupt, and `EOF` is still written.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - Names and arguments echoed in a diagnostic have their unprintable bytes
//!   escaped (`quoting::escape_unprintable`), so none can forge a line of its
//!   own; upstream prints the bytes. The message itself is upstream's.

use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::atomic::{AtomicI32, Ordering};

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_bytes, os_from_bytes};
use coreutils::stdfd;
use coreutils::stdio::StdioReader;

coreutils::guard_std_fds!();

/// The parser. glibc's getopt names the program by `argv[0]` in its own
/// complaints, util-linux by `argv[0]`'s last component.
const WRITE: Program = Program::new("write", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "Vh";

/// Upstream's `longopts`.
const LONG_OPTIONS: &[(&str, Takes)] = &[("version", Takes::Nothing), ("help", Takes::Nothing)];

/// `_PATH_UTMP`, which `utmpxname` is given.
const PATH_UTMP: &str = "/var/run/utmp";

/// `sizeof (u->ut_line) + 6`: the buffer the chosen terminal's path is
/// built in.
const PATH_LEN: usize = 38;

/// `S_IWGRP`.
const S_IWGRP: u32 = 0o020;

/// `signal_received`: the signal that ended the copying, or 0.
static SIGNAL_RECEIVED: AtomicI32 = AtomicI32::new(0);

/// A run that has ended with this status, its diagnostic already out.
#[derive(Debug)]
struct Die(u8);

/// Bytes from the user or the file system, made safe to print in a
/// diagnostic.
fn shown(s: &[u8]) -> Vec<u8> {
    escape_unprintable(s).into_bytes()
}

/// util-linux's `warn`/`warnx`.
fn warn(prog: &[u8], parts: &[&[u8]], reason: Option<&std::io::Error>) {
    let mut m = prog.to_vec();
    m.extend_from_slice(b": ");
    for p in parts {
        m.extend_from_slice(p);
    }
    if let Some(e) = reason {
        m.extend_from_slice(b": ");
        m.extend_from_slice(coreutils::errmsg::strerror(e).as_bytes());
    }
    m.push(b'\n');
    ulclosestream::stderr_write(&m);
}

/// `errtryhelp (EXIT_FAILURE)`'s referral.
fn try_help(prog: &[u8]) {
    let mut m = b"Try '".to_vec();
    m.extend_from_slice(prog);
    m.extend_from_slice(b" --help' for more information.\n");
    ulclosestream::stderr_write(&m);
}

/// `usage`, to standard output.
fn usage(prog: &[u8]) -> Vec<u8> {
    let mut s = b"\nUsage:\n ".to_vec();
    s.extend_from_slice(prog);
    s.extend_from_slice(b" [options] <user> [<ttyname>]\n");
    s.extend_from_slice(b"\nSend a message to another user.\n");
    s.extend_from_slice(b"\nOptions:\n");
    s.extend_from_slice(format!("{:<16}{}\n", " -h, --help", "display this help").as_bytes());
    s.extend_from_slice(format!("{:<16}{}\n", " -V, --version", "display version").as_bytes());
    s.extend_from_slice(b"\nFor more details see write(1).\n");
    s
}

/// The real and effective ids: `getuid`, `getegid`.
fn ids() -> (u32, u32) {
    #[cfg(unix)]
    {
        let i = coreutils::grouplist::current_ids();
        (i.ruid, i.egid)
    }
    #[cfg(not(unix))]
    {
        (0, 0)
    }
}

/// `ttyname (fd)`, or `None`.
fn ttyname(fd: i32) -> Option<Vec<u8>> {
    let mut buf = [0u8; 4096];
    let n = libcall::termios::ttyname_into(fd, &mut buf).ok()?;
    buf.get(..n).map(<[u8]>::to_vec)
}

/// util-linux's `get_terminal_name`: the path of the first of standard
/// input, output and error that is a terminal.
fn get_terminal_name() -> Option<Vec<u8>> {
    let fd = [0, 1, 2].into_iter().find(|&fd| stdfd::is_tty(fd))?;
    ttyname(fd)
}

/// `path` less a leading `/dev/`.
fn after_dev(path: &[u8]) -> &[u8] {
    path.strip_prefix(b"/dev/").unwrap_or(path)
}

/// `check_tty`: the terminal exists, may be written by this sender, and when
/// it was last read -- or `None`, its complaint already made where upstream
/// makes one.
fn check_tty(prog: &[u8], tty: &[u8], showerror: bool) -> Option<(bool, i64)> {
    let st = match std::fs::metadata(os_from_bytes(tty)) {
        Ok(st) => st,
        Err(e) => {
            if showerror {
                warn(prog, &[&shown(tty)], Some(&e));
            }
            return None;
        }
    };
    let (uid, egid) = ids();
    let writeable = if uid == 0 {
        // "root can always write"
        true
    } else {
        if egid != gid_of(&st) {
            warn(
                prog,
                &[b"effective gid does not match group of ", &shown(tty)],
                None,
            );
            return None;
        }
        mode_of(&st) & S_IWGRP != 0
    };
    Some((writeable, atime_of(&st)))
}

#[cfg(unix)]
fn gid_of(st: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    st.gid()
}

#[cfg(not(unix))]
fn gid_of(_st: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn mode_of(st: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    st.mode()
}

#[cfg(not(unix))]
fn mode_of(_st: &std::fs::Metadata) -> u32 {
    0
}

#[cfg(unix)]
fn atime_of(st: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    st.atime()
}

#[cfg(not(unix))]
fn atime_of(_st: &std::fs::Metadata) -> i64 {
    0
}

/// `strncmp (a, b, n) == 0`, a missing byte counting as a NUL.
fn strneq(a: &[u8], b: &[u8], n: usize) -> bool {
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        if x != b.get(i).copied().unwrap_or(0) {
            return false;
        }
        if x == 0 {
            return true;
        }
    }
    true
}

/// The utmp entries, as `getutxent` hands them out after `utmpxname
/// (_PATH_UTMP)`: none when the file cannot be read.
fn utmp() -> Vec<utmpfile::Record> {
    std::fs::read(PATH_UTMP)
        .map(|d| utmpfile::parse(&d))
        .unwrap_or_default()
}

/// What the run knows of the two ends: `struct write_control`.
struct Ctl {
    src_uid: u32,
    src_tty_path: Option<Vec<u8>>,
    src_tty_name: Vec<u8>,
    dst_login: Vec<u8>,
    dst_tty_path: Option<Vec<u8>>,
}

impl Ctl {
    fn dst_tty_name(&self) -> &[u8] {
        self.dst_tty_path
            .as_deref()
            .map_or(&b""[..], |p| p.get(5..).unwrap_or_default())
    }
}

/// `search_utmp`: the recipient's best terminal, or the run's end.
fn search_utmp(prog: &[u8], ctl: &mut Ctl) -> Result<(), Die> {
    let mut best_atime: i64 = 0;
    let mut num_ttys = 0u32;
    let mut valid_ttys = 0u32;
    let mut user_is_me = false;

    for u in utmp() {
        if !strneq(&ctl.dst_login, &u.user, utmpfile::UT_USER_SIZE) {
            continue;
        }
        num_ttys = num_ttys.saturating_add(1);
        // `snprintf (path, sizeof (path), "/dev/%s", u->ut_line)`.
        let mut path = b"/dev/".to_vec();
        path.extend_from_slice(&u.tty);
        path.truncate(PATH_LEN.saturating_sub(1));
        let Some((writeable, tty_atime)) = check_tty(prog, &path, false) else {
            // "bad term? skip"
            continue;
        };
        if ctl.src_uid != 0 && !writeable {
            // "skip ttys with msgs off"
            continue;
        }
        // `memcmp (u->ut_line, src_tty_name, strlen (src_tty_name) + 1)`:
        // the whole name, its terminator included.
        if u.tty == ctl.src_tty_name {
            user_is_me = true;
            // "don't write to yourself"
            continue;
        }
        if u.record_type != utmpfile::USER_PROCESS {
            // "it's not a valid entry"
            continue;
        }
        valid_ttys = valid_ttys.saturating_add(1);
        if best_atime < tty_atime {
            best_atime = tty_atime;
            ctl.dst_tty_path = Some(path);
        }
    }

    if num_ttys == 0 {
        warn(prog, &[&shown(&ctl.dst_login), b" is not logged in"], None);
        return Err(Die(1));
    }
    if valid_ttys == 0 {
        if user_is_me {
            // "ok, so write to yourself!"
            let Some(src) = ctl.src_tty_path.clone() else {
                warn(prog, &[b"can't find your tty's name"], None);
                return Err(Die(1));
            };
            ctl.dst_tty_path = Some(src);
            return Ok(());
        }
        warn(
            prog,
            &[&shown(&ctl.dst_login), b" has messages disabled"],
            None,
        );
        return Err(Die(1));
    }
    if valid_ttys > 1 {
        warn(
            prog,
            &[
                &shown(&ctl.dst_login),
                b" is logged in more than once; writing to ",
                &shown(ctl.dst_tty_name()),
            ],
            None,
        );
    }
    Ok(())
}

/// `check_utmp`: whether the user is in utmp on the terminal named.
fn check_utmp(ctl: &Ctl) -> bool {
    utmp().iter().any(|u| {
        strneq(&ctl.dst_login, &u.user, utmpfile::UT_USER_SIZE)
            && strneq(ctl.dst_tty_name(), &u.tty, utmpfile::UT_LINE_SIZE)
    })
}

/// `signal_handler`.
extern "C" fn signal_handler(signo: i32) {
    SIGNAL_RECEIVED.store(signo, Ordering::Relaxed);
}

/// `xgethostname`: `gethostname` into `get_hostname_max () + 1` bytes -- 65
/// on Linux, 256 on SlateOS -- the last made 0, or `None`.
fn xgethostname() -> Option<Vec<u8>> {
    let sz = libcall::conf::hostname_max().saturating_add(1);
    let mut buf = vec![0u8; sz];
    let n = libcall::hostname_into(&mut buf).ok()?;
    // `name[sz - 1] = '\0'`: a library that cuts the name short without
    // saying so still gives a terminated one.
    Some(buf.get(..n.min(sz.saturating_sub(1)))?.to_vec())
}

/// `getlogin ()`.
fn getlogin() -> Option<Vec<u8>> {
    let mut buf = [0u8; 256];
    let n = libcall::login_name_into(&mut buf).ok()?;
    buf.get(..n).map(<[u8]>::to_vec)
}

/// `do_write`.
fn do_write(prog: &[u8], ctl: &Ctl, out: &mut ulclosestream::Stdout) -> Result<(), Die> {
    // "Determine our login name(s) before the we reopen() stdout"
    let db = pwdb::Db::load();
    let pwuid = db
        .user_by_uid(ctl.src_uid)
        .map_or_else(|| b"???".to_vec(), |u| u.name.clone());
    let login = getlogin().unwrap_or_else(|| pwuid.clone());

    // `freopen (dst_tty_path, "w", stdout)`; with no path -- a terminal
    // chosen whose access time was 0 -- glibc reopens standard output itself.
    let path = ctl
        .dst_tty_path
        .clone()
        .unwrap_or_else(|| b"/proc/self/fd/1".to_vec());
    if let Err(e) = stdfd::freopen(os_from_bytes(&path), stdfd::Reopen::Write, 1) {
        warn(prog, &[&shown(&path)], Some(&e));
        return Err(Die(1));
    }

    // `sa_flags = 0`: a read the signal interrupts is not restarted.
    let mask = libcall::signal::SigSet::empty();
    let _ = libcall::signal::set_handler_masked(libcall::signal::SIGINT, signal_handler, &mask);
    let _ = libcall::signal::set_handler_masked(libcall::signal::SIGHUP, signal_handler, &mask);

    let host = xgethostname().unwrap_or_else(|| b"???".to_vec());
    let zone = localtime::Zone::from_env();
    let tm = zone.localtime(wall_clock(), 0);

    // "print greeting"
    let mut greeting = b"\r\n\x07\x07\x07".to_vec();
    greeting.extend_from_slice(b"Message from ");
    greeting.extend_from_slice(&login);
    greeting.push(b'@');
    greeting.extend_from_slice(&host);
    if login != pwuid {
        greeting.extend_from_slice(b" (as ");
        greeting.extend_from_slice(&pwuid);
        greeting.push(b')');
    }
    greeting.extend_from_slice(b" on ");
    greeting.extend_from_slice(&ctl.src_tty_name);
    greeting.extend_from_slice(format!(" at {:02}:{:02} ...", tm.hour, tm.minute).as_bytes());
    greeting.extend_from_slice(b"\r\n");
    out.write(&greeting);

    let mut input = StdioReader::stdin();
    loop {
        let mut line = Vec::new();
        match input.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if SIGNAL_RECEIVED.load(Ordering::Relaxed) != 0 {
            break;
        }
        out.write(&ulstrutils::fputs_careful(&line, b'^', true, 0));
        if let Some(e) = out.error() {
            let e = std::io::Error::new(e.kind(), e.to_string());
            warn(prog, &[b"carefulputc failed"], Some(&e));
            return Err(Die(1));
        }
    }
    out.write(b"EOF\r\n");
    Ok(())
}

/// `time (NULL)`.
fn wall_clock() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => i64::try_from(e.duration().as_secs())
            .unwrap_or(i64::MAX)
            .saturating_neg(),
    }
}

/// `argv` as glibc's `getopt` leaves it once every option has been read --
/// which, here, means every argument that is not an option: a `--` ends the
/// options and is moved in front of the operands before it, as `getopt`'s
/// permutation moves it; under `POSIXLY_CORRECT` nothing moves.
fn permuted(args: &[Vec<u8>]) -> Vec<Vec<u8>> {
    if std::env::var_os("POSIXLY_CORRECT").is_some() {
        return args.to_vec();
    }
    match args.iter().position(|a| a.as_slice() == b"--") {
        Some(k) => {
            let mut v = vec![b"--".to_vec()];
            v.extend(args.get(..k).unwrap_or_default().iter().cloned());
            v.extend(
                args.get(k.saturating_add(1)..)
                    .unwrap_or_default()
                    .iter()
                    .cloned(),
            );
            v
        }
        None => args.to_vec(),
    }
}

fn main() -> ExitCode {
    stdfd::close_stderr(run_main(), 1)
}

fn run_main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let short = argv0
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(argv0.as_slice(), |i| {
            argv0.get(i.saturating_add(1)..).unwrap_or_default()
        })
        .to_vec();
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(argv.get(1..).unwrap_or_default(), &argv0, &short, &mut out) {
        Ok(code) | Err(Die(code)) => code,
    };
    ExitCode::from(out.close(status, &short))
}

/// Upstream's `main`, after `setlocale`.
fn run(
    argv: &[OsString],
    argv0: &[u8],
    short: &[u8],
    out: &mut ulclosestream::Stdout,
) -> Result<u8, Die> {
    let prog = shown(short);
    for item in WRITE.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        match item {
            Err(e) => {
                let mut m = shown(argv0);
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                ulclosestream::stderr_write(&m);
                try_help(&prog);
                return Err(Die(1));
            }
            Ok(Opt::Short(b'V', _) | Opt::Long("version", _)) => {
                let mut v = prog.clone();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                out.write(&v);
                return Ok(0);
            }
            Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => {
                out.write(&usage(&prog));
                return Ok(0);
            }
            Ok(Opt::Operand(_)) => {}
            Ok(Opt::Short(..) | Opt::Long(..)) => {
                try_help(&prog);
                return Err(Die(1));
            }
        }
    }

    let mut ctl = Ctl {
        src_uid: 0,
        src_tty_path: None,
        src_tty_name: b"<no tty>".to_vec(),
        dst_login: Vec::new(),
        dst_tty_path: None,
    };
    if let Some(path) = get_terminal_name() {
        // "check that sender has write enabled"
        let Some((writeable, _)) = check_tty(&prog, &path, true) else {
            return Err(Die(1));
        };
        if !writeable {
            warn(&prog, &[b"you have write permission turned off"], None);
            return Err(Die(1));
        }
        ctl.src_tty_name = after_dev(&path).to_vec();
        ctl.src_tty_path = Some(path);
    }
    ctl.src_uid = ids().0;

    let words: Vec<Vec<u8>> = argv.iter().map(|a| os_bytes(a).into_owned()).collect();
    match permuted(&words).as_slice() {
        [user] => {
            ctl.dst_login.clone_from(user);
            search_utmp(&prog, &mut ctl)?;
            do_write(&prog, &ctl, out)?;
        }
        [user, tty] => {
            ctl.dst_login.clone_from(user);
            ctl.dst_tty_path = Some(if tty.starts_with(b"/dev/") {
                tty.clone()
            } else {
                [&b"/dev/"[..], tty].concat()
            });
            if !check_utmp(&ctl) {
                warn(
                    &prog,
                    &[
                        &shown(&ctl.dst_login),
                        b" is not logged in on ",
                        &shown(ctl.dst_tty_name()),
                    ],
                    None,
                );
                return Err(Die(1));
            }
            let path = ctl.dst_tty_path.clone().unwrap_or_default();
            let Some((writeable, _)) = check_tty(&prog, &path, true) else {
                return Err(Die(1));
            };
            if ctl.src_uid != 0 && !writeable {
                warn(
                    &prog,
                    &[
                        &shown(&ctl.dst_login),
                        b" has messages disabled on ",
                        &shown(ctl.dst_tty_name()),
                    ],
                    None,
                );
                return Err(Die(1));
            }
            do_write(&prog, &ctl, out)?;
        }
        _ => {
            try_help(&prog);
            return Err(Die(1));
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(a: &[&str]) -> Vec<Vec<u8>> {
        a.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn a_double_dash_moves_in_front_of_the_operands_before_it() {
        assert_eq!(permuted(&words(&["alice"])), words(&["alice"]));
        assert_eq!(permuted(&words(&["alice", "--"])), words(&["--", "alice"]));
        assert_eq!(permuted(&words(&["--", "alice"])), words(&["--", "alice"]));
        assert_eq!(
            permuted(&words(&["alice", "--", "pts/1"])),
            words(&["--", "alice", "pts/1"])
        );
        // Only the first ends the options; a second is an operand.
        assert_eq!(permuted(&words(&["--", "--"])), words(&["--", "--"]));
    }

    #[test]
    fn names_compare_as_strncmp_compares_them() {
        assert!(strneq(b"alice", b"alice", 32));
        assert!(!strneq(b"alice", b"alic", 32));
        // A name the field cuts short still matches its first 32 bytes.
        let long = [b'a'; 40];
        assert!(strneq(&long, &long[..32], 32));
    }
}
