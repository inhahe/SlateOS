//! flock -- manage file locks from shell scripts.
//!
//! A port of util-linux 2.39.3's `sys-utils/flock.c`, function by function
//! and with upstream's names; measured against `flock from util-linux 2.39.3`
//! by `scripts/flock-diff.sh`.
//!
//! This replaces a program that never called `flock(2)` at all: it created
//! `FILE.lock` beside the file and called that a lock, so it excluded only
//! other copies of itself, left the file behind when killed, and could not
//! lock a descriptor. `lockfile`, which was a personality of it, is its own
//! crate now.
//!
//! # What is not upstream's
//!
//! * **How `-w` waits.** Upstream blocks in `flock()` and has a POSIX timer
//!   interrupt it with a signal. SlateOS's blocking `flock()` cannot be
//!   interrupted -- it is a loop in the C library that retries until the lock
//!   is free (known-issues TD-B-FLOCK-WAIT-POLLS) -- so `-w` here tries
//!   `LOCK_NB` until the deadline, sleeping at most 25 ms between tries. What
//!   a caller sees is the same: the lock as soon as it is free, or the
//!   conflict exit status at the deadline; `-w 0` is `-n`; a negative or
//!   unrepresentable timeout is refused as upstream's timer refuses it. Only
//!   the latency of noticing a release differs, by at most a few ms. A lock
//!   with no `-w` blocks in `flock()` exactly as upstream's does.
//! * **A name in a diagnostic** has its unprintable bytes escaped, where
//!   upstream pastes it: a file name holding a newline must not print a line
//!   of its own (design-decisions §370, §1033).

mod sys;

use getoptlong::{Opt, Program, Takes};
use quoting::{escape_unprintable, os_bytes};
use std::ffi::{OsStr, OsString};
use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;
use std::time::{Duration, Instant};
use ulstrutils::{num_error_message, strtotimeval, ul_strtos32};

/// `<sysexits.h>`.
const EX_OK: u8 = 0;
const EX_USAGE: u8 = 64;
const EX_DATAERR: u8 = 65;
const EX_NOINPUT: u8 = 66;
const EX_UNAVAILABLE: u8 = 69;
const EX_OSERR: u8 = 71;
const EX_CANTCREAT: u8 = 73;
/// `close_stdout`'s status when stdout cannot be written: flock.c does not
/// define `CLOSE_EXIT_CODE`, so it is `EXIT_FAILURE`.
const CLOSE_EXIT_CODE: u8 = 1;

/// Only the sentences of its errors are used; each is printed after argv[0].
const FLOCK: Program = Program::new("flock", EX_USAGE as i32);

/// Upstream's option string: `+`, so the first operand -- the file or the
/// descriptor -- ends the options, and `?` is an option of its own that asks
/// only for the referral to `--help`.
const SHORTS: &str = "+sexnoFuw:E:hV?";

/// Upstream's `long_options[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("shared", Takes::Nothing),
    ("exclusive", Takes::Nothing),
    ("unlock", Takes::Nothing),
    ("nonblocking", Takes::Nothing),
    ("nb", Takes::Nothing),
    ("timeout", Takes::Required),
    ("wait", Takes::Required),
    ("conflict-exit-code", Takes::Required),
    ("close", Takes::Nothing),
    ("no-fork", Takes::Nothing),
    ("verbose", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// The two pairs upstream gives one `val`: getopt_long does not count them
/// ambiguous with each other.
const ALIASES: &[(&str, &str)] = &[("nb", "nonblocking"), ("wait", "timeout")];

/// `LOCK_SH`, `LOCK_EX`, `LOCK_UN`, `LOCK_NB`.
const LOCK_SH: i32 = 1;
const LOCK_EX: i32 = 2;
const LOCK_NB: i32 = 4;
const LOCK_UN: i32 = 8;

/// Longest sleep between two tries of a `-w` wait.
const POLL_MAX: Duration = Duration::from_millis(25);

fn main() -> ExitCode {
    let argv: Vec<OsString> = std::env::args_os().collect();
    let mut out = Out::new();
    let status = run(&argv, &mut out);
    ExitCode::from(out.close(status))
}

/// stdout as C's stdio would hold it: line-buffered on a terminal, and
/// otherwise kept until exit -- which is observable, because `--verbose`'s
/// lines then follow the command's output, and vanish if flock `exec`s it.
struct Out {
    tty: bool,
    held: Vec<u8>,
    short: Vec<u8>,
}

impl Out {
    fn new() -> Self {
        Out {
            tty: io::stdout().is_terminal(),
            held: Vec::new(),
            short: b"flock".to_vec(),
        }
    }

    /// `printf` of one whole line.
    fn line(&mut self, text: &[u8]) {
        self.held.extend_from_slice(text);
        if self.tty {
            let held = std::mem::take(&mut self.held);
            // A terminal that cannot be written loses the line, as a failed
            // line-buffered flush does; `close` still reports it.
            if io::stdout().lock().write_all(&held).is_err() {
                self.held = held;
            }
        }
    }

    /// What `exec` does to a buffer: drops it.
    fn discard(&mut self) {
        self.held.clear();
    }

    /// `close_stdout`: write what is held, or report why not.
    fn close(&mut self, status: u8) -> u8 {
        let held = std::mem::take(&mut self.held);
        let mut stdout = io::stdout().lock();
        match stdout.write_all(&held).and_then(|()| stdout.flush()) {
            Ok(()) => status,
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => status,
            Err(e) => {
                warn_msg(
                    &self.short,
                    &format!("write error: {}", errmsg::strerror(&e)),
                );
                CLOSE_EXIT_CODE
            }
        }
    }
}

/// `program_invocation_short_name`: argv[0] past its last `/`.
fn short_name(arg0: &OsStr) -> Vec<u8> {
    let bytes = os_bytes(arg0);
    let start = bytes
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |i| i.saturating_add(1));
    bytes.get(start..).unwrap_or_default().to_vec()
}

/// A name in a diagnostic: upstream's text, unprintable bytes escaped.
fn shown(text: &[u8]) -> String {
    escape_unprintable(text)
}

/// `warnx`: `NAME: MSG`.
fn warn_msg(short: &[u8], msg: &str) {
    let line = format!("{}: {msg}\n", shown(short));
    // A diagnostic that cannot be written has nowhere else to go.
    let _ = io::stderr().lock().write_all(line.as_bytes());
}

/// `warn`: `NAME: MSG: strerror`.
fn warn_err(short: &[u8], msg: &str, e: &io::Error) {
    warn_msg(short, &format!("{msg}: {}", errmsg::strerror(e)));
}

/// `errtryhelp(status)`.
fn errtryhelp(short: &[u8], status: u8) -> u8 {
    let line = format!("Try '{} --help' for more information.\n", shown(short));
    // As in `warn_msg`: stderr is the last resort.
    let _ = io::stderr().lock().write_all(line.as_bytes());
    status
}

/// `usage()`.
fn usage(short: &[u8]) -> Vec<u8> {
    let mut out = b"\nUsage:\n".to_vec();
    for form in [
        &b" [options] <file>|<directory> <command> [<argument>...]\n"[..],
        b" [options] <file>|<directory> -c <command>\n",
        b" [options] <file descriptor number>\n",
    ] {
        out.push(b' ');
        out.extend_from_slice(short);
        out.extend_from_slice(form);
    }
    out.extend_from_slice(
        b"\nManage file locks from shell scripts.\n\
\nOptions:\n\
\x20-s, --shared             get a shared lock\n\
\x20-x, --exclusive          get an exclusive lock (default)\n\
\x20-u, --unlock             remove a lock\n\
\x20-n, --nonblock           fail rather than wait\n\
\x20-w, --timeout <secs>     wait for a limited amount of time\n\
\x20-E, --conflict-exit-code <number>  exit code after conflict or timeout\n\
\x20-o, --close              close file descriptor before running command\n\
\x20-c, --command <command>  run a single command string through the shell\n\
\x20-F, --no-fork            execute command without forking\n\
\x20    --verbose            increase verbosity\n\
\n\
\x20-h, --help               display this help\n\
\x20-V, --version            display version\n\
\nFor more details see flock(1).\n",
    );
    out
}

/// `-w`'s time, as `strtotimeval_or_err` leaves `struct timeval`.
enum Wait {
    /// Seconds and microseconds, each truncated toward zero.
    Timeval(i64, i64),
    /// A value `time_t` cannot hold, which upstream's timer then refuses.
    Unrepresentable,
}

/// The settings `main` collects.
struct Ctl {
    /// `LOCK_SH`, `LOCK_EX` or `LOCK_UN`.
    lock: i32,
    /// `LOCK_NB` for `-n`, else 0.
    block: i32,
    /// `-w`, as `strtotimeval_or_err` made it.
    timeout: Option<Wait>,
    conflict_exit_code: u8,
    do_close: bool,
    no_fork: bool,
    verbose: bool,
}

/// `main()`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(argv: &[OsString], out: &mut Out) -> u8 {
    let arg0: &OsStr = argv
        .first()
        .map_or(OsStr::new("flock"), OsString::as_os_str);
    let short = short_name(arg0);
    out.short.clone_from(&short);

    if argv.len() < 2 {
        warn_msg(&short, "not enough arguments");
        return errtryhelp(&short, EX_USAGE);
    }

    let mut ctl = Ctl {
        lock: LOCK_EX,
        block: 0,
        timeout: None,
        conflict_exit_code: 1,
        do_close: false,
        no_fork: false,
        verbose: false,
    };
    let own = argv.get(1..).unwrap_or_default();
    let mut parser = FLOCK.parse_aliased(own, SHORTS, LONGS, ALIASES);
    let mut optind = own.len();
    while let Some(item) = parser.next() {
        let opt = match item {
            Ok(opt) => opt,
            Err(e) => {
                // glibc names the program by argv[0] as given.
                let line = format!("{}: {}\n", shown(&os_bytes(arg0)), e.sentence);
                // stderr is the last resort, as in `warn_msg`.
                let _ = io::stderr().lock().write_all(line.as_bytes());
                return errtryhelp(&short, EX_USAGE);
            }
        };
        let (flag, value) = match opt {
            Opt::Short(c, value) => (c, value),
            Opt::Long(name, value) => (long_flag(name), value),
            Opt::Operand(_) => {
                optind = parser.optind().saturating_sub(1);
                break;
            }
        };
        match flag {
            b's' => ctl.lock = LOCK_SH,
            b'e' | b'x' => ctl.lock = LOCK_EX,
            b'u' => ctl.lock = LOCK_UN,
            b'o' => ctl.do_close = true,
            b'F' => ctl.no_fork = true,
            b'n' => ctl.block = LOCK_NB,
            b'w' => {
                let value = value.unwrap_or_default();
                match strtotimeval(&os_bytes(&value)) {
                    Ok(Some((sec, usec))) => ctl.timeout = Some(Wait::Timeval(sec, usec)),
                    Ok(None) => ctl.timeout = Some(Wait::Unrepresentable),
                    Err(e) => {
                        warn_msg(
                            &short,
                            &num_error_message("invalid timeout value", &value, e),
                        );
                        return EX_USAGE;
                    }
                }
            }
            b'E' => {
                let value = value.unwrap_or_default();
                match ul_strtos32(&os_bytes(&value), 10) {
                    Ok(code) => match u8::try_from(code) {
                        Ok(code) => ctl.conflict_exit_code = code,
                        Err(_) => {
                            warn_msg(&short, "exit code out of range (expected 0 to 255)");
                            return EX_USAGE;
                        }
                    },
                    Err(e) => {
                        warn_msg(&short, &num_error_message("invalid exit code", &value, e));
                        return EX_USAGE;
                    }
                }
            }
            b'v' => ctl.verbose = true,
            b'V' => {
                let mut line = short.clone();
                line.extend_from_slice(b" from util-linux 2.39.3\n");
                out.line(&line);
                return EX_OK;
            }
            b'h' => {
                out.line(&usage(&short));
                return EX_OK;
            }
            _ => return errtryhelp(&short, EX_USAGE),
        }
    }
    if optind == own.len() {
        optind = parser.optind();
    }

    if ctl.no_fork && ctl.do_close {
        warn_msg(&short, "the --no-fork and --close options are incompatible");
        return EX_USAGE;
    }

    let rest = own.get(optind..).unwrap_or_default();
    let mut open_flags = 0;
    let mut filename: Option<&OsString> = None;
    let mut cmd_argv: Option<Vec<OsString>> = None;
    let mut fd = match rest {
        [file, second, more @ ..] => {
            let second_bytes = os_bytes(second);
            if *second_bytes == *b"-c" || *second_bytes == *b"--command" {
                let [command] = more else {
                    warn_msg(
                        &short,
                        &format!(
                            "{} requires exactly one command argument",
                            shown(&second_bytes)
                        ),
                    );
                    return EX_USAGE;
                };
                let shell = std::env::var_os("SHELL")
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| OsString::from("/bin/sh"));
                cmd_argv = Some(vec![shell, OsString::from("-c"), command.clone()]);
            } else {
                cmd_argv = Some(rest.get(1..).unwrap_or_default().to_vec());
            }
            filename = Some(file);
            match open_file(&short, file, &mut open_flags, ctl.do_close) {
                Ok(fd) => fd,
                Err(status) => return status,
            }
        }
        [fd_word] => match ul_strtos32(&os_bytes(fd_word), 10) {
            Ok(fd) => fd,
            Err(e) => {
                warn_msg(
                    &short,
                    &num_error_message("bad file descriptor", fd_word, e),
                );
                return EX_USAGE;
            }
        },
        [] => {
            warn_msg(&short, "requires file descriptor, file or directory");
            return EX_USAGE;
        }
    };

    let mut deadline: Option<Instant> = None;
    let mut waiting = false;
    if let Some(timeout) = ctl.timeout {
        match timeout {
            // `-w 0` is `-n`: a zero itimer would mean "disabled".
            Wait::Timeval(0, 0) => ctl.block = LOCK_NB,
            Wait::Timeval(sec, usec) if sec >= 0 && usec >= 0 => {
                waiting = true;
                let secs = u64::try_from(sec).unwrap_or(u64::MAX);
                let micros = u64::try_from(usec).unwrap_or(0);
                // A deadline too far off to represent is never reached.
                deadline = Instant::now().checked_add(
                    Duration::from_secs(secs).saturating_add(Duration::from_micros(micros)),
                );
            }
            // What `timer_settime` refuses: a negative time, or one
            // `time_t` could not hold.
            _ => {
                warn_err(
                    &short,
                    "cannot set up timer",
                    &io::Error::from_raw_os_error(sys::EINVAL),
                );
                return EX_OSERR;
            }
        }
    }

    let started = Instant::now();
    let mut pause = Duration::from_millis(1);
    loop {
        let op = if waiting && ctl.block == 0 {
            ctl.lock | LOCK_NB
        } else {
            ctl.lock | ctl.block
        };
        let Err(e) = sys::flock(fd, op) else {
            break;
        };
        match e.raw_os_error() {
            Some(sys::EWOULDBLOCK) if waiting && ctl.block == 0 => {
                let now = Instant::now();
                if deadline.is_some_and(|d| now >= d) {
                    if ctl.verbose {
                        warn_msg(&short, "timeout while waiting to get lock");
                    }
                    return ctl.conflict_exit_code;
                }
                let left = deadline.map_or(pause, |d| d.saturating_duration_since(now));
                std::thread::sleep(pause.min(left));
                pause = pause.saturating_mul(2).min(POLL_MAX);
            }
            Some(sys::EWOULDBLOCK) => {
                if ctl.verbose {
                    warn_msg(&short, "failed to get lock");
                }
                return ctl.conflict_exit_code;
            }
            Some(sys::EINTR) => {}
            Some(sys::EIO | sys::EBADF)
                if open_flags & sys::O_RDWR == 0
                    && ctl.lock != LOCK_SH
                    && filename.is_some_and(|f| sys::access_rw(f)) =>
            {
                // Probably NFSv4, where flock() is emulated by fcntl() and
                // wants a descriptor open for writing: reopen read-write.
                sys::close(fd);
                open_flags = sys::O_RDWR;
                let Some(file) = filename else {
                    return EX_DATAERR;
                };
                fd = match open_file(&short, file, &mut open_flags, ctl.do_close) {
                    Ok(fd) => fd,
                    Err(status) => return status,
                };
                if open_flags & sys::O_RDWR == 0 {
                    // Only a directory reopens without write access, and
                    // `errno` is then still the `EISDIR` that sent it there.
                    let why = io::Error::from_raw_os_error(sys::EISDIR);
                    return lock_failed(&short, filename, fd, &why);
                }
            }
            _ => return lock_failed(&short, filename, fd, &e),
        }
    }

    if ctl.verbose {
        let took = started.elapsed();
        out.line(
            format!(
                "{}: getting lock took {}.{:06} seconds\n",
                shown(&short),
                took.as_secs(),
                took.subsec_micros()
            )
            .as_bytes(),
        );
    }

    let Some(cmd_argv) = cmd_argv else {
        return EX_OK;
    };
    let Some((program, args)) = cmd_argv.split_first() else {
        return EX_OK;
    };
    if ctl.verbose {
        out.line(
            format!(
                "{}: executing {}\n",
                shown(&short),
                shown(&os_bytes(program))
            )
            .as_bytes(),
        );
    }
    // "Clear any inherited settings."
    sys::default_sigchld();
    let mut command = std::process::Command::new(program);
    command.args(args);
    if ctl.no_fork {
        // The C buffer does not survive `exec`, and neither does ours.
        out.discard();
        let e = sys::exec(&mut command);
        return exec_failed(&short, program, &e);
    }
    match command.status() {
        Ok(status) => sys::status_code(status),
        // The command ran, and its status could not be had: upstream's
        // `waitpid failed`, and `EXIT_FAILURE`.
        Err(e) if e.raw_os_error() == Some(sys::ECHILD) => {
            warn_err(&short, "waitpid failed", &e);
            1
        }
        Err(e) => exec_failed(&short, program, &e),
    }
}

/// The short option a long one stands for (`OPT_VERBOSE` as `v`).
fn long_flag(name: &str) -> u8 {
    match name {
        "shared" => b's',
        "exclusive" => b'x',
        "unlock" => b'u',
        "nonblocking" | "nb" => b'n',
        "timeout" | "wait" => b'w',
        "conflict-exit-code" => b'E',
        "close" => b'o',
        "no-fork" => b'F',
        "verbose" => b'v',
        "help" => b'h',
        "version" => b'V',
        // Every name in LONGS is above; this is upstream's `default:`.
        _ => b'?',
    }
}

/// `open_file`: read-only unless `flags` says otherwise, created if absent;
/// a directory, which cannot be opened with `O_CREAT`, read-only without it.
/// `cloexec` is `-o`: the descriptor is closed as the command is `exec`ed,
/// which is what upstream's close in the child before `execvp` amounts to.
///
/// # Errors
///
/// The status to exit with, the reason already printed.
fn open_file(short: &[u8], filename: &OsStr, flags: &mut i32, cloexec: bool) -> Result<i32, u8> {
    let cloexec_flag = if cloexec { sys::O_CLOEXEC } else { 0 };
    let mut fl = if *flags == 0 { sys::O_RDONLY } else { *flags };
    fl |= sys::O_NOCTTY | sys::O_CREAT;
    let mut opened = sys::open(filename, fl | cloexec_flag, 0o666);
    if opened
        .as_ref()
        .is_err_and(|e| e.raw_os_error() == Some(sys::EISDIR))
    {
        fl = sys::O_RDONLY | sys::O_NOCTTY;
        opened = sys::open(filename, fl | cloexec_flag, 0);
    }
    match opened {
        Ok(fd) => {
            *flags = fl;
            Ok(fd)
        }
        Err(e) => {
            warn_err(
                short,
                &format!("cannot open lock file {}", shown(&os_bytes(filename))),
                &e,
            );
            Err(match e.raw_os_error() {
                Some(sys::ENOMEM | sys::EMFILE | sys::ENFILE) => EX_OSERR,
                Some(sys::EROFS | sys::ENOSPC) => EX_CANTCREAT,
                _ => EX_NOINPUT,
            })
        }
    }
}

/// The default case of the lock loop: name the file, or the descriptor.
fn lock_failed(short: &[u8], filename: Option<&OsString>, fd: i32, e: &io::Error) -> u8 {
    let name = match filename {
        Some(f) => shown(&os_bytes(f)),
        None => fd.to_string(),
    };
    warn_err(short, &name, e);
    match e.raw_os_error() {
        Some(sys::ENOLCK | sys::ENOMEM) => EX_OSERR,
        _ => EX_DATAERR,
    }
}

/// `run_program`'s failure: `failed to execute NAME`, and `EX_OSERR` for
/// `ENOMEM`, `EX_UNAVAILABLE` otherwise.
fn exec_failed(short: &[u8], program: &OsStr, e: &io::Error) -> u8 {
    warn_err(
        short,
        &format!("failed to execute {}", shown(&os_bytes(program))),
        e,
    );
    if e.raw_os_error() == Some(sys::ENOMEM) {
        EX_OSERR
    } else {
        EX_UNAVAILABLE
    }
}

/// How `strtos32_or_err` refuses, for the tests: the message after `flock: `.
#[cfg(test)]
fn refusal(errmesg: &str, arg: &str, e: ulstrutils::NumErr) -> String {
    num_error_message(errmesg, OsStr::new(arg), e)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn every_long_option_has_its_letter() {
        for (name, _) in LONGS {
            assert_ne!(long_flag(name), b'?', "{name}");
        }
    }

    #[test]
    fn the_aliases_are_not_ambiguous_with_each_other() {
        let argv: Vec<OsString> = ["--nonb", "--wai", "1", "f"]
            .iter()
            .map(OsString::from)
            .collect();
        let items: Vec<_> = FLOCK
            .parse_aliased(&argv, SHORTS, LONGS, ALIASES)
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            items,
            vec![
                Opt::Long("nonblocking", None),
                Opt::Long("wait", Some("1".into())),
                Opt::Operand(&argv[3]),
            ]
        );
        // `--n` is nonblocking, nb or no-fork: the last is another option.
        let argv: Vec<OsString> = ["--n"].iter().map(OsString::from).collect();
        let err = FLOCK
            .parse_aliased(&argv, SHORTS, LONGS, ALIASES)
            .next()
            .unwrap()
            .unwrap_err();
        assert_eq!(
            err.sentence,
            "option '--n' is ambiguous; possibilities: '--nonblocking' '--no-fork'"
        );
    }

    use ulstrutils::NumErr;

    #[test]
    fn the_refusals_are_upstreams() {
        assert_eq!(
            refusal("invalid exit code", "abc", NumErr::Invalid),
            "invalid exit code: 'abc'"
        );
        assert_eq!(
            refusal("bad file descriptor", "99999999999", NumErr::Range),
            "bad file descriptor: '99999999999': Numerical result out of range"
        );
    }

    #[test]
    fn the_short_name_is_argv0_past_its_last_slash() {
        assert_eq!(short_name(OsStr::new("/usr/bin/flock")), b"flock");
    }

    #[test]
    fn usage_names_the_program_as_invoked() {
        let text = String::from_utf8(usage(b"myflock")).unwrap();
        assert!(text.starts_with("\nUsage:\n myflock [options] <file>|<directory> <command>"));
        assert!(text.ends_with("For more details see flock(1).\n"));
    }
}
