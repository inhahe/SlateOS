//! `mesg` -- control write access of other users to your terminal:
//! util-linux 2.39.3's, ported.
//!
//! ```text
//! mesg [options] [y | n]
//! ```
//!
//! A transcription of `term-utils/mesg.c`. The terminal is the first of
//! standard input, output and error that is one; its group-write bit is what
//! `write` and `wall` honour. What upstream does and this keeps:
//!
//! - **With no operand**, `is y` (status 0) when the terminal is writable by
//!   its group or by others, `is n` (status 1) when it is not.
//! - **`y` or `n`**, read by `rpmatch` -- the C locale's `^[yY]` and `^[nN]`,
//!   so `yes` and `no` and anything else beginning so -- sets the group- and
//!   other-write bits, as Ubuntu builds it (without `USE_TTY_GROUP`: measured,
//!   `mesg y` makes a 620 terminal 622), or clears both; anything else is
//!   `invalid argument`, status 1.
//! - **Errors are status 2**: no terminal at all (`no tty` only with `-v`), a
//!   terminal that cannot be stat'ed, opened or changed. When `ttyname`
//!   fails, the terminal is reached as `/proc/self/fd/N`.
//! - **`-v`** says what it did.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - Names and arguments echoed in a diagnostic have their unprintable bytes
//!   escaped (`quoting::escape_unprintable`); upstream prints the bytes.

use std::ffi::OsString;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_bytes, os_from_bytes};
use coreutils::stdfd;

coreutils::guard_std_fds!();

/// The parser. glibc's getopt names the program by `argv[0]` in its own
/// complaints, util-linux by `argv[0]`'s last component.
const MESG: Program = Program::new("mesg", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "vVh";

/// Upstream's `longopts`.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("verbose", Takes::Nothing),
    ("version", Takes::Nothing),
    ("help", Takes::Nothing),
];

/// `IS_ALLOWED`, `IS_NOT_ALLOWED` and `MESG_EXIT_FAILURE`.
const IS_ALLOWED: u8 = 0;
const IS_NOT_ALLOWED: u8 = 1;
const MESG_EXIT_FAILURE: u8 = 2;

/// `S_IWGRP` and `S_IWOTH`.
const S_IWGRP: u32 = 0o020;
const S_IWOTH: u32 = 0o002;

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
    s.extend_from_slice(b" [options] [y | n]\n");
    s.extend_from_slice(b"\nControl write access of other users to your terminal.\n");
    s.extend_from_slice(b"\nOptions:\n");
    s.extend_from_slice(b" -v, --verbose  explain what is being done\n");
    s.extend_from_slice(format!("{:<16}{}\n", " -h, --help", "display this help").as_bytes());
    s.extend_from_slice(format!("{:<16}{}\n", " -V, --version", "display version").as_bytes());
    s.extend_from_slice(b"\nFor more details see mesg(1).\n");
    s
}

/// `rpmatch` in the C locale: `Some(true)` for `^[yY]`, `Some(false)` for
/// `^[nN]`, `None` for anything else.
fn rpmatch(answer: &[u8]) -> Option<bool> {
    match answer.first() {
        Some(b'y' | b'Y') => Some(true),
        Some(b'n' | b'N') => Some(false),
        _ => None,
    }
}

/// `st_mode`.
#[cfg(unix)]
fn mode_of(st: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    st.mode()
}

#[cfg(not(unix))]
fn mode_of(_st: &std::fs::Metadata) -> u32 {
    0
}

/// `fchmod (fd, mode)`, the type bits left off as the kernel ignores them.
#[cfg(unix)]
fn fchmod(file: &std::fs::File, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(mode & 0o7777))
}

#[cfg(not(unix))]
fn fchmod(_file: &std::fs::File, _mode: u32) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
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
    let mut verbose = false;
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in MESG.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
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
            Ok(Opt::Short(b'v', _) | Opt::Long("verbose", _)) => verbose = true,
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
            Ok(Opt::Operand(v)) => operands.push(os_bytes(v).into_owned()),
            Ok(Opt::Short(..) | Opt::Long(..)) => {
                try_help(&prog);
                return Err(Die(1));
            }
        }
    }

    // `get_terminal_stdfd`.
    let Some(fd) = [0, 1, 2].into_iter().find(|&fd| stdfd::is_tty(fd)) else {
        if verbose {
            warn(&prog, &[b"no tty"], None);
        }
        return Err(Die(MESG_EXIT_FAILURE));
    };
    let mut buf = [0u8; 4096];
    let tty: Vec<u8> = match libcall::termios::ttyname_into(fd, &mut buf) {
        Ok(n) => buf.get(..n).unwrap_or_default().to_vec(),
        Err(_) => {
            let t = format!("/proc/self/fd/{fd}").into_bytes();
            if verbose {
                warn(
                    &prog,
                    &[b"ttyname() failed, attempting to go around using: ", &t],
                    None,
                );
            }
            t
        }
    };

    let Some(answer) = operands.first() else {
        let st = std::fs::metadata(os_from_bytes(&tty)).map_err(|e| {
            warn(&prog, &[b"stat of ", &shown(&tty), b" failed"], Some(&e));
            Die(MESG_EXIT_FAILURE)
        })?;
        if mode_of(&st) & (S_IWGRP | S_IWOTH) != 0 {
            out.write(b"is y\n");
            return Ok(IS_ALLOWED);
        }
        out.write(b"is n\n");
        return Ok(IS_NOT_ALLOWED);
    };

    let file = std::fs::File::open(os_from_bytes(&tty)).map_err(|e| {
        warn(&prog, &[b"cannot open ", &shown(&tty)], Some(&e));
        Die(MESG_EXIT_FAILURE)
    })?;
    let st = file.metadata().map_err(|e| {
        warn(&prog, &[b"stat of ", &shown(&tty), b" failed"], Some(&e));
        Die(MESG_EXIT_FAILURE)
    })?;
    let mode = mode_of(&st);
    let ret = match rpmatch(answer) {
        Some(true) => {
            fchmod(&file, mode | S_IWGRP | S_IWOTH).map_err(|e| {
                warn(
                    &prog,
                    &[b"change ", &shown(&tty), b" mode failed"],
                    Some(&e),
                );
                Die(MESG_EXIT_FAILURE)
            })?;
            if verbose {
                out.write(b"write access to your terminal is allowed\n");
            }
            IS_ALLOWED
        }
        Some(false) => {
            fchmod(&file, mode & !(S_IWGRP | S_IWOTH)).map_err(|e| {
                warn(
                    &prog,
                    &[b"change ", &shown(&tty), b" mode failed"],
                    Some(&e),
                );
                Die(MESG_EXIT_FAILURE)
            })?;
            if verbose {
                out.write(b"write access to your terminal is denied\n");
            }
            IS_NOT_ALLOWED
        }
        None => {
            warn(&prog, &[b"invalid argument: ", &shown(answer)], None);
            try_help(&prog);
            return Err(Die(1));
        }
    };
    // `close (fd)`, unchecked: the file was only read and its mode changed.
    drop(stdfd::close(file));
    Ok(ret)
}

#[cfg(test)]
mod tests {
    use super::rpmatch;

    #[test]
    fn answers_are_the_c_locales() {
        assert_eq!(rpmatch(b"y"), Some(true));
        assert_eq!(rpmatch(b"Yes"), Some(true));
        assert_eq!(rpmatch(b"n"), Some(false));
        assert_eq!(rpmatch(b"nope"), Some(false));
        assert_eq!(rpmatch(b""), None);
        assert_eq!(rpmatch(b"1"), None);
        assert_eq!(rpmatch(b"maybe"), None);
    }
}
