//! What systemd-journald does with a message besides storing it: its
//! forwarding, for the two programs here that play journald's part --
//! `syslogd`, which reads `/dev/log`, and `systemd-cat`'s helper, which reads
//! a command's output streams. SlateOS has no journald (its journal is a file
//! those two append to), so without this a message journald would have
//! broadcast is only filed.
//!
//! # `ForwardToWall`
//!
//! With journald's default settings (`ForwardToWall=yes`, `MaxLevelWall=emerg`)
//! a message at `emerg` is written to the terminal of every user who is logged
//! in, as `wall` would -- [`forward_wall`] is journald's `server_forward_wall`,
//! and [`wall`] the `wall()` of systemd 255's `src/shared/wall.c` it calls.
//! The line is `IDENTIFIER[PID]: MESSAGE` (the writer's command name when it
//! gave no identifier), under the banner `Broadcast message from
//! systemd-journald@HOST (DATE):`, sent to each `USER_PROCESS` entry in utmp
//! that names a user, whatever its terminal: the rules are systemd's, which
//! are not util-linux `wall`'s (no X-display exception, no group, a 50 ms
//! write budget per terminal, a line that is not a terminal skipped).
//!
//! Measured by `scripts/journalfwd-diff.sh` against systemd's own `wall()`,
//! called from `libsystemd-shared` with the same utmp, in a namespace whose
//! terminals are the harness's own.
//!
//! # Deliberately different
//!
//! - With no utmp file, systemd asks logind for its sessions' terminals.
//!   SlateOS's logind keeps its sessions to itself
//!   (`B-LOGIND-IMPLEMENTS-THE-WRITE-SIDE-AND-EXPOSES-NONE-OF-IT`), so here
//!   that finds none -- which is also what systemd finds where logind is not
//!   running.
//! - A host with no name gets `localhost` after `$SYSTEMD_DEFAULT_HOSTNAME`
//!   and `/etc/os-release`'s `DEFAULT_HOSTNAME`, the fallback systemd is
//!   built with on Ubuntu.

/// `MaxLevelWall=`'s default: `emerg`.
pub const MAX_LEVEL_WALL: u32 = 0;

/// `_PATH_UTMPX`.
pub const PATH_UTMPX: &str = "/var/run/utmp";

/// `TIMEOUT_USEC`: how long a terminal may hold a write up.
#[cfg(unix)]
const TIMEOUT_MS: u64 = 50;

/// `COMM_MAX_LEN`: how much of an escaped command name `pid_get_comm` keeps.
const COMM_MAX_LEN: usize = 128;

/// `-ENOPROTOOPT`: "no utmp here", which sends `wall` to logind.
const ENOPROTOOPT: i32 = 92;
/// `ENOTTY`.
const ENOTTY: i32 = 25;
/// `ETIME`: a terminal that did not take the message within the budget.
#[cfg(unix)]
const ETIME: i32 = 62;
/// `EIO`, `EINTR` and `EAGAIN`.
#[cfg(unix)]
const EIO: i32 = 5;
#[cfg(unix)]
const EINTR: i32 = 4;
#[cfg(unix)]
const EAGAIN: i32 = 11;
/// `ENOENT`.
const ENOENT: i32 = 2;

/// `LOG_PRI`.
fn log_pri(priority: u32) -> u32 {
    priority & 7
}

/// `-errno`, as systemd returns a failure.
fn neg(errno: i32) -> i32 {
    errno.saturating_neg()
}

/// `server_forward_wall`, with journald's defaults: a message at `emerg`
/// broadcast to every logged-in user's terminal. `pid` is the sender's, from
/// its credentials, when they are known.
///
/// journald only logs a failure, at debug level, so nothing is returned.
pub fn forward_wall(priority: u32, identifier: Option<&[u8]>, message: &[u8], pid: Option<u32>) {
    if log_pri(priority) > MAX_LEVEL_WALL {
        return;
    }
    let line = wall_line(identifier, message, pid, pid_get_comm);
    // As journald: a failure to reach a terminal is no reason to lose the
    // message, which is filed whatever happens here.
    let _ = wall(&line, b"systemd-journald", None);
}

/// The line `server_forward_wall` broadcasts: `IDENT[PID]: MESSAGE` with
/// credentials -- the command name standing in for a missing identifier --
/// else `IDENT: MESSAGE`, or the message alone. Each part ends at a NUL, as
/// the C strings do.
pub fn wall_line(
    identifier: Option<&[u8]>,
    message: &[u8],
    pid: Option<u32>,
    comm: impl Fn(u32) -> Option<Vec<u8>>,
) -> Vec<u8> {
    let message = cstr(message);
    let identifier = identifier.map(cstr);
    let mut line = Vec::with_capacity(message.len().saturating_add(32));
    match (pid, identifier) {
        (Some(pid), ident) => {
            let fallback;
            let ident = match ident {
                Some(i) => i,
                None => {
                    fallback = comm(pid).unwrap_or_default();
                    cstr(&fallback)
                }
            };
            line.extend_from_slice(ident);
            line.extend_from_slice(format!("[{pid}]: ").as_bytes());
        }
        (None, Some(ident)) => {
            line.extend_from_slice(ident);
            line.extend_from_slice(b": ");
        }
        (None, None) => {}
    }
    line.extend_from_slice(message);
    line
}

/// `pid_get_comm`: `/proc/PID/comm`, escaped by `cellescape` -- the command
/// name journald records and names a writer by.
#[must_use]
pub fn pid_get_comm(pid: u32) -> Option<Vec<u8>> {
    let mut comm = std::fs::read(format!("/proc/{pid}/comm")).ok()?;
    // `read_one_line_file`: the first line, its newline dropped.
    if let Some(nl) = comm.iter().position(|&c| c == b'\n') {
        comm.truncate(nl);
    }
    Some(cellescape(cstr(&comm), COMM_MAX_LEN))
}

/// The bytes of `s` before its first NUL.
fn cstr(s: &[u8]) -> &[u8] {
    s.iter()
        .position(|&c| c == 0)
        .map_or(s, |n| s.get(..n).unwrap_or(s))
}

/// `cescape_char`: one byte as C would write it in a string -- the named
/// escapes, `\\`, `\"`, `\'`, and three octal digits for any other byte below
/// a space or from DEL up.
fn cescape_char(c: u8, out: &mut Vec<u8>) {
    let named = match c {
        0x07 => Some(b'a'),
        0x08 => Some(b'b'),
        0x0c => Some(b'f'),
        b'\n' => Some(b'n'),
        b'\r' => Some(b'r'),
        b'\t' => Some(b't'),
        0x0b => Some(b'v'),
        b'\\' | b'"' | b'\'' => Some(c),
        _ => None,
    };
    if let Some(n) = named {
        out.push(b'\\');
        out.push(n);
    } else if c < b' ' || c >= 127 {
        // Three octal digits; each is below 8, so `|` is `+` here.
        out.push(b'\\');
        out.push(b'0' | (c >> 6));
        out.push(b'0' | ((c >> 3) & 7));
        out.push(b'0' | (c & 7));
    } else {
        out.push(c);
    }
}

/// `cellescape (buf, len, s)`: `s` escaped into a buffer of `len` bytes, its
/// NUL included -- each escape whole or not at all, and `...` at the end,
/// eating back through up to four escapes to make room, when it does not
/// fit.
fn cellescape(s: &[u8], len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    let mut widths = [0usize; 4];
    let mut k = 0usize;
    let mut fits = true;
    for &c in s {
        let mut four = Vec::with_capacity(4);
        cescape_char(c, &mut four);
        if out.len().saturating_add(four.len()).saturating_add(1) > len {
            fits = false;
            break;
        }
        out.extend_from_slice(&four);
        if let Some(w) = widths.get_mut(k) {
            *w = four.len();
        }
        k = k.wrapping_add(1) & 3;
    }
    if fits {
        return out;
    }
    // Ellipsis needed: make room for four bytes, if the string allows.
    for _ in 0..widths.len() {
        if out.len().saturating_add(4) <= len {
            break;
        }
        k = k.checked_sub(1).unwrap_or(3);
        let w = widths.get(k).copied().unwrap_or(0);
        if w == 0 {
            break;
        }
        out.truncate(out.len().saturating_sub(w));
    }
    if out.len().saturating_add(4) <= len {
        out.extend_from_slice(b"...");
    } else if out.len().saturating_add(3) <= len {
        out.extend_from_slice(b"..");
    } else if out.len().saturating_add(2) <= len {
        out.push(b'.');
    }
    out
}

/// `gethostname_malloc`: the node name -- or, where there is none, the
/// default host name.
fn hostname() -> Vec<u8> {
    let mut buf = [0u8; 256];
    let name = libcall::hostname_into(&mut buf)
        .ok()
        .and_then(|n| buf.get(..n))
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    if name.is_empty() || name == b"(none)" {
        return default_hostname();
    }
    name
}

/// `get_default_hostname`: `$SYSTEMD_DEFAULT_HOSTNAME`, else os-release's
/// `DEFAULT_HOSTNAME`, else the build's fallback -- `localhost` -- each
/// taken only if it is a valid host name.
fn default_hostname() -> Vec<u8> {
    if let Some(e) = std::env::var_os("SYSTEMD_DEFAULT_HOSTNAME") {
        let e = os_bytes(&e);
        if hostname_is_valid(&e) {
            return e;
        }
    }
    for path in ["/etc/os-release", "/usr/lib/os-release"] {
        let Ok(text) = std::fs::read(path) else {
            continue;
        };
        for line in text.split(|&c| c == b'\n') {
            if let Some(v) = line.strip_prefix(b"DEFAULT_HOSTNAME=") {
                let v = v
                    .strip_prefix(b"\"")
                    .and_then(|v| v.strip_suffix(b"\""))
                    .unwrap_or(v);
                if hostname_is_valid(v) {
                    return v.to_vec();
                }
            }
        }
        break;
    }
    b"localhost".to_vec()
}

/// `hostname_is_valid (s, 0)`: dot-separated labels of letters, digits and
/// hyphens, no empty label, at most 64 bytes.
fn hostname_is_valid(s: &[u8]) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.split(|&c| c == b'.').all(|label| {
            !label.is_empty()
                && label
                    .iter()
                    .all(|c| c.is_ascii_alphanumeric() || *c == b'-')
        })
}

#[cfg(unix)]
fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    s.as_bytes().to_vec()
}

#[cfg(not(unix))]
fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

/// `FORMAT_TIMESTAMP (now)`: `Fri 2026-10-09 05:59:01 EDT` -- the weekday
/// in English whatever the locale, then the zone's abbreviation when there
/// is one and it fits systemd's buffer (13 bytes at most).
#[must_use]
pub fn format_timestamp(sec: i64) -> Vec<u8> {
    const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    // `USEC_TIMESTAMP_FORMATTABLE_MAX`: the start of the year 10000.
    const FORMATTABLE_MAX: i64 = 253_402_300_799;
    if sec > FORMATTABLE_MAX {
        return b"--- XXXX-XX-XX XX:XX:XX".to_vec();
    }
    let zone = localtime::Zone::from_env();
    let tm = zone.localtime(sec, 0);
    let mut out = WEEKDAYS
        .get(usize::try_from(tm.wday).unwrap_or(0))
        .copied()
        .unwrap_or("Sun")
        .as_bytes()
        .to_vec();
    out.extend_from_slice(&localtime::strftime(b" %Y-%m-%d %H:%M:%S", &tm));
    let abbr = tm.abbr.as_bytes();
    // `FORMAT_TIMESTAMP_MAX` is 38: a zone fits when the 23 bytes above, a
    // space, the zone and the NUL do.
    if !abbr.is_empty() && out.len().saturating_add(abbr.len()).saturating_add(2) <= 38 {
        out.push(b' ');
        out.extend_from_slice(abbr);
    }
    out
}

/// `wall (message, username, origin_tty, NULL, NULL)`: the broadcast,
/// written to every logged-in user's terminal. 0, or the first failure as a
/// negative `errno` -- the terminals after a failure are still written to.
pub fn wall(message: &[u8], username: &[u8], origin_tty: Option<&[u8]>) -> i32 {
    let stdin_tty;
    let origin = match origin_tty {
        Some(t) => Some(t),
        None => {
            stdin_tty = stdin_tty_name();
            stdin_tty.as_deref()
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
    let mut text = b"\r\nBroadcast message from ".to_vec();
    text.extend_from_slice(username);
    text.push(b'@');
    text.extend_from_slice(&hostname());
    if let Some(o) = origin {
        text.extend_from_slice(b" on ");
        text.extend_from_slice(o);
    }
    text.extend_from_slice(b" (");
    text.extend_from_slice(&format_timestamp(now));
    text.extend_from_slice(b"):\r\n\r\n");
    text.extend_from_slice(cstr(message));
    text.extend_from_slice(b"\r\n\r\n");

    let r = wall_utmp(&text, PATH_UTMPX);
    // There is no logind to ask for its sessions (see the crate docs): it
    // finds none, as systemd's `wall_logind` does where logind is not running.
    if r == neg(ENOPROTOOPT) { 0 } else { r }
}

/// `getttyname_harder (STDIN_FILENO, ...)`: standard input's terminal,
/// without its `/dev/`, when it is one.
fn stdin_tty_name() -> Option<Vec<u8>> {
    let mut buf = [0u8; 256];
    let n = libcall::termios::ttyname_into(0, &mut buf).ok()?;
    let path = buf.get(..n)?;
    let name = path.strip_prefix(b"/dev/").unwrap_or(path);
    // `tty` names the controlling terminal itself, which systemd resolves
    // further; there is no such path here, so it is no name at all.
    (name != b"tty").then(|| name.to_vec())
}

/// `wall_utmp`: `text` written to the terminal of each `USER_PROCESS` entry
/// with a user in the utmp file at `path` -- `-ENOPROTOOPT` when there is no
/// such file.
pub fn wall_utmp(text: &[u8], path: &str) -> i32 {
    // `access (_PATH_UTMPX, F_OK)`: libc's `setutxent` does not say whether
    // the file exists, so systemd asks first.
    if let Err(e) = std::fs::metadata(path) {
        return match e.raw_os_error() {
            Some(ENOENT) | None => neg(ENOPROTOOPT),
            Some(code) => neg(code),
        };
    }
    // A file that is there but cannot be read gives `getutxent` nothing.
    let data = std::fs::read(path).unwrap_or_default();
    let mut r = 0;
    for rec in utmpfile::parse(&data) {
        if rec.record_type != utmpfile::USER_PROCESS || rec.user.is_empty() {
            continue;
        }
        let tty_path = if path_startswith(&rec.tty, b"/dev/") {
            rec.tty.clone()
        } else {
            let mut p = b"/dev/".to_vec();
            p.extend_from_slice(&rec.tty);
            p
        };
        let q = write_to_terminal(&tty_path, text);
        if r >= 0 && q < 0 {
            r = q;
        }
    }
    r
}

/// The components of a path: what is between its slashes, `.` and empty
/// ones dropped, as systemd's `path_find_first_component` walks it.
fn components(path: &[u8]) -> impl Iterator<Item = &[u8]> {
    path.split(|&c| c == b'/')
        .filter(|c| !c.is_empty() && *c != b".")
}

/// `path_startswith (path, prefix) != NULL`: both absolute or both relative,
/// and each component of `prefix` the component of `path` in its place --
/// so `/dev/pts/0` and `//dev/./pts/0` start with `/dev/`, `/devices/x`
/// does not, and `/dev` itself does.
fn path_startswith(path: &[u8], prefix: &[u8]) -> bool {
    if (path.first() == Some(&b'/')) != (prefix.first() == Some(&b'/')) {
        return false;
    }
    let mut p = components(path);
    components(prefix).all(|want| p.next() == Some(want))
}

/// `write_to_terminal`: the whole of `text` onto `tty`, opened without
/// becoming this process's terminal and without blocking, within 50 ms; 0 or
/// a negative `errno` (`-ENOTTY` for a path that is not a terminal, `-ETIME`
/// for one that would not take it in time).
#[cfg(unix)]
fn write_to_terminal(tty: &[u8], text: &[u8]) -> i32 {
    use std::io::Write;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    /// `O_NONBLOCK | O_NOCTTY`; std adds `O_CLOEXEC` itself.
    const FLAGS: i32 = 0o4000 | 0o400;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .custom_flags(FLAGS)
        .open(std::ffi::OsStr::from_bytes(tty));
    let mut file = match file {
        Ok(f) => f,
        Err(e) => return neg(e.raw_os_error().unwrap_or(ENOENT)),
    };
    if !libcall::fd::is_terminal(file.as_raw_fd()) {
        return neg(ENOTTY);
    }
    // `loop_write_full (fd, message, SIZE_MAX, TIMEOUT_USEC)`.
    let start = std::time::Instant::now();
    let budget = std::time::Duration::from_millis(TIMEOUT_MS);
    let mut rest = text;
    while !rest.is_empty() {
        match file.write(rest) {
            // "Can't really happen", systemd says, and answers it so.
            Ok(0) => return neg(EIO),
            Ok(n) => rest = rest.get(n..).unwrap_or_default(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let Some(left) = budget.checked_sub(start.elapsed()) else {
                    return neg(ETIME);
                };
                if left.is_zero() {
                    return neg(ETIME);
                }
                let ms = i32::try_from(left.as_millis().max(1)).unwrap_or(i32::MAX);
                match libcall::fd::wait_writable(file.as_raw_fd(), ms) {
                    Ok(true) => {}
                    Ok(false) => return neg(ETIME),
                    // Transient: write again, as `ERRNO_IS_NEG_TRANSIENT` has it.
                    Err(e) if e == EINTR || e == EAGAIN => {}
                    Err(e) => return neg(e),
                }
            }
            Err(e) => return neg(e.raw_os_error().unwrap_or(EIO)),
        }
    }
    0
}

#[cfg(not(unix))]
fn write_to_terminal(_tty: &[u8], _text: &[u8]) -> i32 {
    neg(ENOTTY)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_line_names_the_sender_as_journald_does() {
        let comm = |_| Some(b"cmd".to_vec());
        assert_eq!(
            wall_line(Some(b"tag"), b"msg", Some(42), comm),
            b"tag[42]: msg"
        );
        assert_eq!(wall_line(None, b"msg", Some(42), comm), b"cmd[42]: msg");
        assert_eq!(wall_line(None, b"msg", Some(42), |_| None), b"[42]: msg");
        assert_eq!(wall_line(Some(b"tag"), b"msg", None, comm), b"tag: msg");
        assert_eq!(wall_line(None, b"msg", None, comm), b"msg");
        assert_eq!(wall_line(Some(b"t\0x"), b"m\0y", None, comm), b"t: m");
    }

    #[test]
    fn command_names_are_escaped_as_cellescape_escapes_them() {
        assert_eq!(cellescape(b"bash", 128), b"bash");
        assert_eq!(cellescape(b"a\nb", 128), b"a\\nb");
        assert_eq!(cellescape(b"q\"'\\", 128), b"q\\\"\\'\\\\");
        assert_eq!(cellescape(b"\x01\x7f\xe9", 128), b"\\001\\177\\351");
        // Too long: an ellipsis, made room for by dropping whole escapes.
        assert_eq!(cellescape(b"abcdefghij", 8), b"abcd...");
        assert_eq!(cellescape(b"ab\ncd", 6), b"ab...");
    }

    #[test]
    fn a_line_is_a_path_when_its_components_begin_with_dev() {
        assert!(path_startswith(b"/dev/pts/0", b"/dev/"));
        assert!(path_startswith(b"//dev/./pts/0", b"/dev/"));
        assert!(path_startswith(b"/dev", b"/dev/"));
        assert!(!path_startswith(b"/devices/x", b"/dev/"));
        assert!(!path_startswith(b"pts/0", b"/dev/"));
        assert!(!path_startswith(b"dev/pts/0", b"/dev/"), "relative");
    }

    #[test]
    fn only_emerg_is_broadcast() {
        assert_eq!(log_pri(0), MAX_LEVEL_WALL);
        assert_eq!(
            log_pri(8),
            MAX_LEVEL_WALL,
            "facility bits are not the level"
        );
        assert!(log_pri(1) > MAX_LEVEL_WALL);
    }

    #[test]
    fn host_names_are_checked_as_systemd_checks_them() {
        assert!(hostname_is_valid(b"slate"));
        assert!(hostname_is_valid(b"a-b.example"));
        assert!(!hostname_is_valid(b""));
        assert!(!hostname_is_valid(b"a..b"));
        assert!(!hostname_is_valid(b"a b"));
    }
}
