//! nologin -- politely refuse a login.
//!
//! shadow 4.13's `src/nologin.c`, ported, as Ubuntu 24.04 ships it in the
//! `login` package: the shell a system account is given, so that logging in
//! as it is refused. It writes the attempt to the system log -- who, with
//! what uid, on which terminal, and the command an SSH client asked for --
//! at `LOG_CRIT` under `LOG_AUTH`, prints one line on standard output, and
//! exits 1.
//!
//! It takes no options: upstream's `main (void)` never looks at its
//! arguments, so `nologin --help` refuses like any other invocation.
//! (util-linux has a `nologin` too, which reads `/etc/nologin.txt` and has
//! options; Ubuntu installs shadow's.) Measured against Ubuntu's by
//! `scripts/nologin-diff.sh`, the log line included.
//!
//! This replaces a program written here that printed to standard error,
//! read `/etc/nologin.txt`, answered `--help` and `--version`, and logged
//! nothing.

stdfdguard::guard_std_fds!();

use std::io::Write;
use std::process::ExitCode;

/// syslog's `LOG_CONS`: on the console when the log cannot be reached.
const LOG_CONS: i32 = 0x02;
/// syslog's `LOG_AUTH` facility.
const LOG_AUTH: i32 = 4 << 3;
/// syslog's `LOG_CRIT` level.
const LOG_CRIT: i32 = 2;

/// `UT_NAMESIZE + 1`: the buffer glibc's `getlogin` keeps the name in, so a
/// longer name is no name, as it is there.
const LOGIN_NAME_BUF: usize = 33;

/// `getlogin ()`: the name the session was opened under, as the C library
/// answers it; `None` where it answers `NULL`.
fn getlogin() -> Option<Vec<u8>> {
    let mut buf = [0u8; LOGIN_NAME_BUF];
    let n = libcall::login_name_into(&mut buf).ok()?;
    buf.get(..n).map(<[u8]>::to_vec)
}

/// `ttyname (0)`: the terminal on standard input; `None` where it is none.
fn ttyname0() -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 4096];
    let n = libcall::termios::ttyname_into(0, &mut buf).ok()?;
    buf.truncate(n);
    Some(buf)
}

/// The log line: `Attempted login by %s (UID: %d) on %s%s%s`.
fn attempt(user: &[u8], uid: u32, tty: &[u8], ssh_origcmd: Option<&[u8]>) -> Vec<u8> {
    let mut line = b"Attempted login by ".to_vec();
    line.extend_from_slice(user);
    // `%d` of a `uid_t`: an int.
    line.extend_from_slice(format!(" (UID: {}) on ", uid.cast_signed()).as_bytes());
    line.extend_from_slice(tty);
    if let Some(cmd) = ssh_origcmd {
        line.extend_from_slice(b" SSH_ORIGINAL_COMMAND=");
        line.extend_from_slice(cmd);
    }
    line
}

fn main() -> ExitCode {
    // Before anything touches standard I/O: a descriptor the program was
    // started without stays closed, as upstream would find it.
    stdfdguard::restore();
    let tty = ttyname0().unwrap_or_else(|| b"UNKNOWN".to_vec());
    let user = getlogin().unwrap_or_else(|| b"UNKNOWN".to_vec());
    let ssh_origcmd =
        std::env::var_os("SSH_ORIGINAL_COMMAND").map(|v| quoting::os_bytes(&v).into_owned());
    let uid = libcall::process::getuid();
    libcsyslog::openlog(b"nologin", LOG_CONS, LOG_AUTH);
    libcsyslog::syslog(LOG_CRIT, &attempt(&user, uid, &tty, ssh_origcmd.as_deref()));
    libcsyslog::closelog();

    // `printf`, flushed by `exit`; a failed write is not looked at.
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"This account is currently not available.\n");
    let _ = out.flush();
    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_log_line_is_upstreams() {
        assert_eq!(
            attempt(b"root", 0, b"/dev/pts/3", None),
            b"Attempted login by root (UID: 0) on /dev/pts/3".to_vec()
        );
        assert_eq!(
            attempt(b"UNKNOWN", 1000, b"UNKNOWN", Some(b"ls -l")),
            b"Attempted login by UNKNOWN (UID: 1000) on UNKNOWN SSH_ORIGINAL_COMMAND=ls -l"
                .to_vec()
        );
        assert_eq!(
            attempt(b"x", u32::MAX, b"t", Some(b"")),
            b"Attempted login by x (UID: -1) on t SSH_ORIGINAL_COMMAND=".to_vec(),
            "%d of a uid, and an empty command still named"
        );
    }
}
