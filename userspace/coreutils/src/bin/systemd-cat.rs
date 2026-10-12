//! `systemd-cat`: systemd 255's (`src/journal/cat.c`), ported. It runs a
//! command with its standard output and standard error connected to the
//! journal -- or with no command, files its standard input there.
//!
//! # What upstream does, and what plays journald's part here
//!
//! Upstream asks journald for a stream (`sd_journal_stream_fd`: a socket
//! connected to `/run/systemd/journal/stdout`, a short header naming the
//! identifier, the priority and whether lines may carry a `<N>` priority of
//! their own), makes it the standard output and error, and *becomes* the
//! command (`execvp`) -- `/bin/cat` when none is named. journald reads the
//! stream, cuts it into lines and files each line as a record, under the
//! process that wrote it: the kernel attaches the writer's credentials to
//! what it reads (`SCM_CREDENTIALS`), so a line from a process the command
//! started is that process's, not the command's.
//!
//! SlateOS has no journald; its journal is the JSON-lines file `journalctl`
//! reads (`journalrec::MAIN_LOG_PATH`), appended to under its lock
//! (`journalio`). So this does what upstream does, and supplies the other end
//! itself: each stream is one end of a `socketpair`, and the other end goes
//! to a helper process that does what journald's `stdout_stream_process`
//! does with it -- the same cuts (a newline, a NUL, or 48 KiB), the same
//! trimming, the same `<N>`, the same attribution, and a record per line. The
//! helper is detached (its own session, its parent gone, its descriptors its
//! own), because journald is nobody's child either: a command that waits for
//! all its children, or a Ctrl-C at the terminal, must not reach it. Then
//! this process becomes the command, as upstream's does, so the command has
//! `systemd-cat`'s process id, its signals and its exit status, with nothing
//! in between.
//!
//! A record carries what journald would show of the line: `level` its
//! priority, `service` the identifier `-t` gave or else the writer's command
//! name (`journalctl` shows `_COMM` where there is no identifier), `pid` the
//! writer, and beside them journald's own fields, as `syslogd` writes them:
//! `_PID`, `_UID`, `_GID` and `_COMM` where they are known, `_TRANSPORT`
//! `stdout`, and `_LINE_BREAK` for a line that did not end with a newline.
//!
//! # Deliberately different
//!
//! - `--version` names SlateOS's coreutils, not systemd 255.
//! - Where the kernel cannot say who wrote a line, the line is the
//!   command's. journald does the same with a stream that carries no
//!   credentials; SlateOS's library carries none until its Unix-domain
//!   sockets are finished (`requests/b-ad-a-unix-socket-cannot-be-bound-to-a-
//!   path-so-nothing-can-receive-syslog.md`).
//! - journald names a writer by `/proc/PID/comm` as it reads the line, and
//!   when the writer has already gone it names nobody (`journalctl` shows
//!   `unknown`). This does the same, except for the command itself, which it
//!   names by the program it ran: `systemd-cat echo hi` is filed as `echo`
//!   however quickly `echo` finished.
//! - A message `systemd-cat` itself logs to "the journal" -- under
//!   `SYSTEMD_LOG_TARGET=journal`, or when its standard error is a journal
//!   stream -- is appended to the journal file, there being no journald to
//!   send it to.

use std::ffi::OsString;
use std::io;
use std::process::ExitCode;
use std::sync::OnceLock;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::os_bytes;
use coreutils::stdfd;

// Recorded before `main`: a standard descriptor the caller closed must stay
// closed, since upstream's stream lands on the lowest free number -- a closed
// standard input becomes the stream, and reads end at once.
coreutils::guard_std_fds!();

/// The parser. glibc's complaints name `argv[0]`, not this.
const SYSTEMD_CAT: Program = Program::new("systemd-cat", 1);

/// `options[]`, in its order.
const LONGS: &[(&str, Takes)] = &[
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
    ("identifier", Takes::Required),
    ("priority", Takes::Required),
    ("stderr-priority", Takes::Required),
    ("level-prefix", Takes::Required),
];

/// `LOG_ERR`.
const LOG_ERR: u8 = 3;
/// `LOG_WARNING`.
const LOG_WARNING: u8 = 4;
/// `LOG_NOTICE`.
const LOG_NOTICE: u8 = 5;
/// `LOG_INFO`: the default priority, and the default most verbose level the
/// log shows.
const LOG_INFO: u8 = 6;
/// `LOG_DEBUG`: the largest priority.
const LOG_DEBUG: u8 = 7;
/// `LOG_DAEMON`, already shifted: the facility the log stamps on its own
/// messages (`log_facility`).
const LOG_DAEMON: u8 = 3 << 3;
/// C's `LINE_MAX`: the log formats a message into a buffer this long, so
/// anything past 2047 bytes is cut off.
const LOG_LINE_MAX: usize = 2048;
/// systemd's `WHITESPACE`.
const WHITESPACE: &[u8] = b" \t\n\r";
/// systemd's `NEWLINE`: what the log splits a message into lines at.
const NEWLINE: &[u8] = b"\n\r";
/// `log_level_table`, by number.
const LEVELS: [&str; 8] = [
    "emerg", "alert", "crit", "err", "warning", "notice", "info", "debug",
];

const ANSI_HIGHLIGHT: &str = "\x1b[0;1;39m";
const ANSI_HIGHLIGHT_RED: &str = "\x1b[0;1;31m";
const ANSI_HIGHLIGHT_KHAKI3: &str = "\x1b[0;1;38;5;185m";
const ANSI_HIGHLIGHT_YELLOW4: &str = "\x1b[0;1;38;5;100m";
const ANSI_HIGHLIGHT_YELLOW_FALLBACK: &str = "\x1b[0;1;33m";
const ANSI_GREY: &str = "\x1b[0;38;5;245m";
const ANSI_BRIGHT_BLACK: &str = "\x1b[0;90m";
const ANSI_NORMAL: &str = "\x1b[0m";

// -------------------------------------------------------------- parsing ----

/// `parse_boolean`.
fn parse_boolean(v: &[u8]) -> Option<bool> {
    let is = |set: &[&str]| set.iter().any(|s| v.eq_ignore_ascii_case(s.as_bytes()));
    if is(&["1", "yes", "y", "true", "t", "on"]) {
        Some(true)
    } else if is(&["0", "no", "n", "false", "f", "off"]) {
        Some(false)
    } else {
        None
    }
}

/// C's `isspace` in the C locale, which `strtoul` skips.
fn c_isspace(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `safe_atou64`, and `safe_atou` with `max` set to `u32::MAX`: systemd's
/// `WHITESPACE` skipped, `0b` and `0o` taken as bases 2 and 8 (`mangle_base`),
/// then `strtoull (s, &end, base)` -- which skips its own white space, takes a
/// sign, and at base 0 reads `0x` as 16 and a leading `0` as 8 -- with
/// nothing allowed after the number, no value over `max`, and a minus sign
/// only on zero.
fn safe_atou_max(s: &[u8], max: u64) -> Option<u64> {
    let lead = s.iter().take_while(|c| WHITESPACE.contains(c)).count();
    let s = s.get(lead..).unwrap_or_default();
    let (mut base, s) = match s {
        [b'0', b'b' | b'B', rest @ ..] => (2u64, rest),
        [b'0', b'o' | b'O', rest @ ..] => (8u64, rest),
        _ => (0u64, s),
    };
    // `strtoull`'s own reading.
    let mut i = s.iter().take_while(|&&c| c_isspace(c)).count();
    let negative = s.get(i) == Some(&b'-');
    if matches!(s.get(i), Some(b'+' | b'-')) {
        i = i.saturating_add(1);
    }
    let hex = |c: Option<&u8>| c.is_some_and(u8::is_ascii_hexdigit);
    if base == 0 {
        if s.get(i) == Some(&b'0')
            && matches!(s.get(i.saturating_add(1)), Some(b'x' | b'X'))
            && hex(s.get(i.saturating_add(2)))
        {
            i = i.saturating_add(2);
            base = 16;
        } else if s.get(i) == Some(&b'0') {
            base = 8;
        } else {
            base = 10;
        }
    }
    let start = i;
    let mut value: u64 = 0;
    let mut overflow = false;
    while let Some(d) = s
        .get(i)
        .and_then(|&c| char::from(c).to_digit(36))
        .map(u64::from)
        .filter(|&d| d < base)
    {
        match value.checked_mul(base).and_then(|v| v.checked_add(d)) {
            Some(v) => value = v,
            None => overflow = true,
        }
        i = i.saturating_add(1);
    }
    // Nothing converted, or something after the number: `EINVAL`. Too big
    // for `unsigned long long`: `ERANGE`.
    if i == start || i != s.len() || overflow {
        return None;
    }
    if negative {
        // `l != 0 && s[0] == '-'`: the test is on the text after the white
        // space systemd skipped, so a minus after a vertical tab -- which only
        // `strtoull` skips -- wraps unseen, as it does upstream.
        if value != 0 && s.first() == Some(&b'-') {
            return None;
        }
        value = value.wrapping_neg();
    }
    (value <= max).then_some(value)
}

/// `log_level_from_string`: a level's name (exactly), or its number, 0 to 7.
fn log_level_from_string(s: &[u8]) -> Option<u8> {
    if let Some(i) = LEVELS.iter().position(|n| n.as_bytes() == s) {
        return u8::try_from(i).ok();
    }
    safe_atou_max(s, u64::from(u32::MAX))
        .filter(|&u| u <= u64::from(LOG_DEBUG))
        .and_then(|u| u8::try_from(u).ok())
}

/// `program_invocation_short_name`: `argv[0]` after its last `/`.
fn short_name(argv0: &[u8]) -> &[u8] {
    argv0.rsplit(|&c| c == b'/').next().unwrap_or_default()
}

// ------------------------------------------------------------- terminal ----

/// `ColorMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorMode {
    Off,
    On,
    Sixteen,
    TwoFiftySix,
    TwentyFourBit,
}

/// `on_tty`: standard output and standard error both terminals. Asked once,
/// as upstream caches it.
fn on_tty() -> bool {
    static ON_TTY: OnceLock<bool> = OnceLock::new();
    *ON_TTY.get_or_init(|| stdfd::is_tty(1) && stdfd::is_tty(2))
}

/// `on_dev_null`: standard output and standard error both `/dev/null`.
fn on_dev_null() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let same = |fd: i32, null: &std::fs::Metadata| {
            stdfd::metadata(fd).is_ok_and(|m| m.dev() == null.dev() && m.ino() == null.ino())
        };
        std::fs::metadata("/dev/null").is_ok_and(|null| same(1, &null) && same(2, &null))
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// `getenv_terminal_is_dumb`: no `$TERM`, or `dumb`.
fn getenv_terminal_is_dumb() -> bool {
    std::env::var_os("TERM").is_none_or(|t| t == "dumb")
}

/// `get_color_mode`, decided once.
fn color_mode() -> ColorMode {
    static MODE: OnceLock<ColorMode> = OnceLock::new();
    *MODE.get_or_init(|| {
        if let Some(e) = std::env::var_os("SYSTEMD_COLORS") {
            let e = os_bytes(&e);
            match &*e {
                b"16" => return ColorMode::Sixteen,
                b"256" => return ColorMode::TwoFiftySix,
                _ => {}
            }
            if let Some(b) = parse_boolean(&e) {
                return if b { ColorMode::On } else { ColorMode::Off };
            }
        }
        if std::env::var_os("NO_COLOR").is_some() {
            return ColorMode::Off;
        }
        // `terminal_is_dumb`; the PID 1 arm is no program's but init's.
        if (!on_tty() && !on_dev_null()) || getenv_terminal_is_dumb() {
            return ColorMode::Off;
        }
        match std::env::var_os("COLORTERM") {
            Some(c) if c == "truecolor" || c == "24bit" => ColorMode::TwentyFourBit,
            _ => ColorMode::TwoFiftySix,
        }
    })
}

/// `colors_enabled`.
fn colors_enabled() -> bool {
    color_mode() != ColorMode::Off
}

/// A `DEFINE_ANSI_FUNC` colour: the sequence when colours are on.
fn ansi(seq: &'static str) -> &'static str {
    if colors_enabled() { seq } else { "" }
}

/// A `DEFINE_ANSI_FUNC_256` colour: its 16-colour fallback in that mode.
fn ansi_256(seq: &'static str, fallback: &'static str) -> &'static str {
    match color_mode() {
        ColorMode::Off => "",
        ColorMode::Sixteen => fallback,
        _ => seq,
    }
}

/// `urlify_enabled`: `$SYSTEMD_URLIFY` as a boolean, else as colours are.
fn urlify_enabled() -> bool {
    static URLIFY: OnceLock<bool> = OnceLock::new();
    *URLIFY.get_or_init(|| {
        std::env::var_os("SYSTEMD_URLIFY")
            .and_then(|v| parse_boolean(&os_bytes(&v)))
            .unwrap_or_else(colors_enabled)
    })
}

/// `terminal_urlify_man ("systemd-cat", "1", ...)`.
fn man_link() -> String {
    let url = "man:systemd-cat(1)";
    let text = "systemd-cat(1) man page";
    if urlify_enabled() {
        format!("\x1b]8;;{url}\x07{text}\x1b]8;;\x07")
    } else {
        text.to_owned()
    }
}

// ------------------------------------------------------------------ log ----

/// `LogTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Console,
    ConsolePrefixed,
    Kmsg,
    Journal,
    JournalOrKmsg,
    Syslog,
    SyslogOrKmsg,
    Auto,
    Null,
}

impl Target {
    /// `log_target_from_string`.
    fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"console" => Self::Console,
            b"console-prefixed" => Self::ConsolePrefixed,
            b"kmsg" => Self::Kmsg,
            b"journal" => Self::Journal,
            b"journal-or-kmsg" => Self::JournalOrKmsg,
            b"syslog" => Self::Syslog,
            b"syslog-or-kmsg" => Self::SyslogOrKmsg,
            b"auto" => Self::Auto,
            b"null" => Self::Null,
            _ => return None,
        })
    }

    fn is(self, set: &[Self]) -> bool {
        set.contains(&self)
    }
}

/// Where in upstream's source a message is logged: what
/// `SYSTEMD_LOG_LOCATION` prints, and a journal record's `CODE_*`.
#[derive(Clone, Copy, Debug)]
struct Site {
    file: &'static str,
    line: u32,
    func: &'static str,
}

const CAT_C: &str = "src/journal/cat.c";
const LOG_C: &str = "src/basic/log.c";
const AT_PRIORITY: Site = Site {
    file: CAT_C,
    line: 102,
    func: "parse_argv",
};
const AT_STDERR_PRIORITY: Site = Site {
    file: CAT_C,
    line: 109,
    func: "parse_argv",
};
const AT_BOOLEAN: Site = Site {
    file: "src/shared/parse-argument.c",
    line: 21,
    func: "parse_boolean_argument",
};
const AT_STREAM: Site = Site {
    file: CAT_C,
    line: 141,
    func: "run",
};
#[cfg(unix)]
const AT_STDERR_STREAM: Site = Site {
    file: CAT_C,
    line: 146,
    func: "run",
};
#[cfg(unix)]
const AT_REARRANGE: Site = Site {
    file: CAT_C,
    line: 155,
    func: "run",
};
#[cfg(unix)]
const AT_FSTAT: Site = Site {
    file: CAT_C,
    line: 164,
    func: "run",
};
#[cfg(unix)]
const AT_EXECUTE: Site = Site {
    file: CAT_C,
    line: 182,
    func: "run",
};
const AT_ASSERT: Site = Site {
    file: CAT_C,
    line: 185,
    func: "main",
};

/// The syslog socket the log may write to, and how.
#[cfg(unix)]
enum SyslogSocket {
    Datagram(std::os::unix::net::UnixDatagram),
    Stream(std::os::unix::net::UnixStream),
}

/// systemd's `log.c`, for the few messages this program prints of its own:
/// where they go (`SYSTEMD_LOG_TARGET`), which are shown
/// (`SYSTEMD_LOG_LEVEL`), and how each line is dressed
/// (`SYSTEMD_LOG_COLOR`, `_LOCATION`, `_TIME`, `_TID`).
struct Log {
    target: Target,
    max_level: u8,
    /// `show_color`: unset until set, which `log_setup` does for a log on the
    /// console.
    show_color: Option<bool>,
    show_location: bool,
    show_time: bool,
    show_tid: bool,
    /// Standard error is the console (`console_fd`); closed when another
    /// target took over.
    console: bool,
    /// The journal could be written (`journal_fd`).
    journal: bool,
    kmsg: Option<std::fs::File>,
    #[cfg(unix)]
    syslog: Option<SyslogSocket>,
    /// `program_invocation_short_name`.
    name: Vec<u8>,
}

impl Log {
    /// The log before `log_setup`: what `main`'s own assertion writes to.
    fn initial(name: &[u8]) -> Self {
        Self {
            target: Target::Console,
            max_level: LOG_INFO,
            show_color: None,
            show_location: false,
            show_time: false,
            show_tid: false,
            console: true,
            journal: false,
            kmsg: None,
            #[cfg(unix)]
            syslog: None,
            name: name.to_vec(),
        }
    }

    /// `log_setup`.
    fn setup(name: &[u8]) -> Self {
        let mut log = Self::initial(name);
        log.target = Target::Auto;
        log.parse_environment();
        log.open();
        if log.on_console() && log.show_color.is_none() {
            log.show_color = Some(true);
        }
        log
    }

    /// `log_parse_environment_variables`, each in turn, so that a complaint
    /// about one is dressed by the ones before it.
    fn parse_environment(&mut self) {
        let var = |name: &str| std::env::var_os(name).map(|v| os_bytes(&v).into_owned());
        let complain = |log: &mut Self, line: u32, what: &str, value: &[u8]| {
            let mut m = format!("Failed to parse log {what} '").into_bytes();
            m.extend_from_slice(value);
            m.extend_from_slice(b"'. Ignoring.");
            log.log(
                LOG_WARNING,
                0,
                Site {
                    file: LOG_C,
                    line,
                    func: "log_parse_environment_variables",
                },
                &m,
            );
        };
        if let Some(e) = var("SYSTEMD_LOG_TARGET") {
            match Target::from_name(&e) {
                Some(t) => self.target = t,
                None => complain(self, 1308, "target", &e),
            }
        }
        if let Some(e) = var("SYSTEMD_LOG_LEVEL") {
            match log_level_from_string(&e) {
                Some(l) => self.max_level = l,
                None => complain(self, 1312, "level", &e),
            }
        }
        if let Some(e) = var("SYSTEMD_LOG_COLOR") {
            match parse_boolean(&e) {
                Some(b) => self.show_color = Some(b),
                None => complain(self, 1316, "color", &e),
            }
        }
        if let Some(e) = var("SYSTEMD_LOG_LOCATION") {
            match parse_boolean(&e) {
                Some(b) => self.show_location = b,
                None => complain(self, 1320, "location", &e),
            }
        }
        if let Some(e) = var("SYSTEMD_LOG_TIME") {
            match parse_boolean(&e) {
                Some(b) => self.show_time = b,
                None => complain(self, 1324, "time", &e),
            }
        }
        if let Some(e) = var("SYSTEMD_LOG_TID") {
            match parse_boolean(&e) {
                Some(b) => self.show_tid = b,
                None => complain(self, 1328, "tid", &e),
            }
        }
        if let Some(e) = var("SYSTEMD_LOG_RATELIMIT_KMSG")
            && parse_boolean(&e).is_none()
        {
            complain(self, 1332, "ratelimit kmsg boolean", &e);
        }
    }

    /// `log_open`.
    fn open(&mut self) {
        use Target::{Auto, Journal, JournalOrKmsg, Kmsg, Null, Syslog, SyslogOrKmsg};
        if self.target == Null {
            self.journal = false;
            self.close_syslog();
            self.console = false;
            return;
        }
        if stderr_is_journal()
            || self
                .target
                .is(&[Kmsg, Journal, JournalOrKmsg, Syslog, SyslogOrKmsg])
        {
            if self.target.is(&[Auto, JournalOrKmsg, Journal]) && journal_writable() {
                self.journal = true;
                self.close_syslog();
                self.console = false;
                return;
            }
            if self.target.is(&[SyslogOrKmsg, Syslog]) && self.open_syslog() {
                self.journal = false;
                self.console = false;
                return;
            }
            if self.target.is(&[Auto, JournalOrKmsg, SyslogOrKmsg, Kmsg]) && self.open_kmsg() {
                self.journal = false;
                self.close_syslog();
                self.console = false;
                return;
            }
        }
        self.journal = false;
        self.close_syslog();
        self.console = true;
    }

    /// `log_on_console`.
    fn on_console(&self) -> bool {
        if self.target.is(&[Target::Console, Target::ConsolePrefixed]) {
            return true;
        }
        !self.journal && self.kmsg.is_none() && !self.has_syslog()
    }

    #[cfg(unix)]
    fn has_syslog(&self) -> bool {
        self.syslog.is_some()
    }

    #[cfg(not(unix))]
    fn has_syslog(&self) -> bool {
        false
    }

    /// `log_open_syslog`: `/dev/log`, as a datagram socket or, failing that,
    /// a stream.
    #[cfg(unix)]
    fn open_syslog(&mut self) -> bool {
        use std::os::unix::net::{UnixDatagram, UnixStream};
        if self.syslog.is_some() {
            return true;
        }
        let datagram = UnixDatagram::unbound().and_then(|s| s.connect("/dev/log").map(|()| s));
        self.syslog = match datagram {
            Ok(s) => Some(SyslogSocket::Datagram(s)),
            Err(_) => UnixStream::connect("/dev/log")
                .ok()
                .map(SyslogSocket::Stream),
        };
        self.syslog.is_some()
    }

    #[cfg(not(unix))]
    fn open_syslog(&mut self) -> bool {
        false
    }

    #[cfg(unix)]
    fn close_syslog(&mut self) {
        self.syslog = None;
    }

    #[cfg(not(unix))]
    fn close_syslog(&mut self) {}

    /// `log_open_kmsg`.
    fn open_kmsg(&mut self) -> bool {
        if self.kmsg.is_none() {
            self.kmsg = std::fs::OpenOptions::new()
                .write(true)
                .open("/dev/kmsg")
                .and_then(stdfd::fd_safer)
                .ok();
        }
        self.kmsg.is_some()
    }

    /// `log_full_errno (level, errno, ...)`: shown if the level is, cut to
    /// what upstream's buffer holds, and sent where the log goes. `errno` is
    /// only the journal's `ERRNO` field -- the message already says it in
    /// words -- so a `SYNTHETIC_ERRNO`, which upstream keeps out of that
    /// field, is passed as 0.
    fn log(&mut self, level: u8, errno: i32, site: Site, message: &[u8]) {
        if level > self.max_level {
            return;
        }
        let cut = message.get(..message.len().min(LOG_LINE_MAX.saturating_sub(1)));
        self.dispatch(level | LOG_DAEMON, errno, site, cut.unwrap_or(message));
    }

    /// `log_error_errno` with `%m` as `errno`'s words.
    fn error(&mut self, errno: i32, site: Site, before: &str, after: &str) {
        let mut m = before.as_bytes().to_vec();
        m.extend_from_slice(strerror(errno).as_bytes());
        m.extend_from_slice(after.as_bytes());
        self.log(LOG_ERR, errno, site, &m);
    }

    /// `log_dispatch_internal`: each line of the message to the first place
    /// that takes it.
    fn dispatch(&mut self, level: u8, errno: i32, site: Site, message: &[u8]) {
        use Target::{Auto, Journal, JournalOrKmsg, Kmsg, Null, Syslog, SyslogOrKmsg};
        if self.target == Null {
            return;
        }
        let mut rest = message;
        loop {
            let skip = rest.iter().take_while(|c| NEWLINE.contains(c)).count();
            rest = rest.get(skip..).unwrap_or_default();
            if rest.is_empty() {
                break;
            }
            let end = rest
                .iter()
                .position(|c| NEWLINE.contains(c))
                .unwrap_or(rest.len());
            let line = rest.get(..end).unwrap_or_default();
            rest = rest
                .get(end.saturating_add(1).min(rest.len())..)
                .unwrap_or_default();

            let mut written = Sent::Nothing;
            if self.target.is(&[Auto, JournalOrKmsg, Journal]) {
                written = self.write_to_journal(level, errno, site, line);
                if written == Sent::Failed {
                    self.journal = false;
                }
            }
            if self.target.is(&[SyslogOrKmsg, Syslog]) {
                written = self.write_to_syslog(level, line);
                if written == Sent::Failed {
                    self.close_syslog();
                }
            }
            if written != Sent::Written
                && self.target.is(&[Auto, SyslogOrKmsg, JournalOrKmsg, Kmsg])
            {
                if written == Sent::Failed {
                    self.open_kmsg();
                }
                written = self.write_to_kmsg(level, line);
                if written == Sent::Failed {
                    self.kmsg = None;
                    self.console = true;
                }
            }
            if written != Sent::Written {
                self.write_to_console(level, site, line);
            }
        }
    }

    /// `write_to_console`.
    fn write_to_console(&self, level: u8, site: Site, line: &[u8]) {
        if !self.console {
            return;
        }
        let show_color = self.show_color == Some(true);
        let mut out: Vec<u8> = Vec::new();
        if self.target == Target::ConsolePrefixed {
            out.extend_from_slice(format!("<{level}>").as_bytes());
        }
        if self.show_time
            && let Some(stamp) = format_timestamp()
        {
            out.extend_from_slice(&stamp);
            out.push(b' ');
        }
        if self.show_tid {
            out.extend_from_slice(format!("({}) ", std::process::id()).as_bytes());
        }
        let (on, off) = if show_color {
            log_colors(level & 7)
        } else {
            ("", "")
        };
        if self.show_location {
            let (lon, loff) = if show_color {
                (
                    ansi_256(ANSI_HIGHLIGHT_YELLOW4, ANSI_HIGHLIGHT_YELLOW_FALLBACK),
                    ansi(ANSI_NORMAL),
                )
            } else {
                ("", "")
            };
            out.extend_from_slice(format!("{lon}{}:{}{loff}: ", site.file, site.line).as_bytes());
        }
        out.extend_from_slice(on.as_bytes());
        out.extend_from_slice(line);
        out.extend_from_slice(off.as_bytes());
        out.extend_from_slice(if stdfd::is_tty(2) { b"\r\n" } else { b"\n" });
        // Unchecked, as upstream's `writev` is by every caller here: a
        // message that cannot be delivered has nowhere else to go.
        let _ = stdfd::write_all(2, &out);
    }

    /// `write_to_kmsg`.
    fn write_to_kmsg(&mut self, level: u8, line: &[u8]) -> Sent {
        use std::io::Write;
        let Some(kmsg) = self.kmsg.as_mut() else {
            return Sent::Nothing;
        };
        let mut out = format!("<{level}>").into_bytes();
        out.extend_from_slice(&self.name);
        out.extend_from_slice(format!("[{}]: ", std::process::id()).as_bytes());
        out.extend_from_slice(line);
        out.push(b'\n');
        if kmsg.write_all(&out).is_ok() {
            Sent::Written
        } else {
            Sent::Failed
        }
    }

    /// `write_to_syslog`.
    #[cfg(unix)]
    fn write_to_syslog(&mut self, level: u8, line: &[u8]) -> Sent {
        use std::io::Write;
        let Some(socket) = self.syslog.as_mut() else {
            return Sent::Nothing;
        };
        let Some(stamp) = syslog_timestamp() else {
            return Sent::Failed;
        };
        let mut out = format!("<{level}>").into_bytes();
        out.extend_from_slice(&stamp);
        out.extend_from_slice(&self.name);
        out.extend_from_slice(format!("[{}]: ", std::process::id()).as_bytes());
        out.extend_from_slice(line);
        let sent = match socket {
            SyslogSocket::Datagram(s) => s.send(&out).map(|_| ()),
            SyslogSocket::Stream(s) => {
                // Over a stream, messages are told apart by a NUL.
                out.push(0);
                s.write_all(&out)
            }
        };
        if sent.is_ok() {
            Sent::Written
        } else {
            Sent::Failed
        }
    }

    #[cfg(not(unix))]
    fn write_to_syslog(&mut self, _level: u8, _line: &[u8]) -> Sent {
        Sent::Nothing
    }

    /// `write_to_journal`: a record of the fields journald's native protocol
    /// would carry, in the journal file this system's `journalctl` reads.
    fn write_to_journal(&self, level: u8, errno: i32, site: Site, line: &[u8]) -> Sent {
        if !self.journal {
            return Sent::Nothing;
        }
        let pid = std::process::id();
        let pid_text = pid.to_string();
        let line_text = site.line.to_string();
        let errno_text = errno.to_string();
        let facility_text = (level >> 3).to_string();
        let comm = read_comm(pid);
        let mut extra: Vec<(&str, &[u8])> = vec![
            ("facility", b"daemon"),
            ("SYSLOG_FACILITY", facility_text.as_bytes()),
            ("TID", pid_text.as_bytes()),
            ("CODE_FILE", site.file.as_bytes()),
            ("CODE_LINE", line_text.as_bytes()),
            ("CODE_FUNC", site.func.as_bytes()),
        ];
        if errno != 0 {
            extra.push(("ERRNO", errno_text.as_bytes()));
        }
        extra.push(("_PID", pid_text.as_bytes()));
        if let Some(c) = comm.as_deref() {
            extra.push(("_COMM", c));
        }
        extra.push(("_TRANSPORT", b"journal"));
        let record = journalrec::ByteRecord {
            ts: now_secs(),
            level: priority_name(level & 7),
            service: &self.name,
            msg: line,
            pid: Some(pid),
        };
        let mut bytes = record.to_json_line_with(&extra).into_bytes();
        bytes.push(b'\n');
        match journalio::append(std::path::Path::new(journalrec::MAIN_LOG_PATH), &bytes) {
            Ok(()) => Sent::Written,
            Err(_) => Sent::Failed,
        }
    }
}

/// What one of the log's writers did with a line: `k` in
/// `log_dispatch_internal`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sent {
    /// It has no sink open (`0`).
    Nothing,
    /// It wrote the line (`1`).
    Written,
    /// It tried and failed (`< 0`).
    Failed,
}

/// `get_log_colors (priority, &on, &off, NULL)`.
fn log_colors(priority: u8) -> (&'static str, &'static str) {
    if priority <= LOG_ERR {
        (ansi(ANSI_HIGHLIGHT_RED), ansi(ANSI_NORMAL))
    } else if priority <= LOG_WARNING {
        (
            ansi_256(ANSI_HIGHLIGHT_KHAKI3, ANSI_HIGHLIGHT_YELLOW_FALLBACK),
            ansi(ANSI_NORMAL),
        )
    } else if priority <= LOG_NOTICE {
        (ansi(ANSI_HIGHLIGHT), ansi(ANSI_NORMAL))
    } else if priority >= LOG_DEBUG {
        (ansi_256(ANSI_GREY, ANSI_BRIGHT_BLACK), ansi(ANSI_NORMAL))
    } else {
        ("", "")
    }
}

/// `stderr_is_journal`: `$JOURNAL_STREAM` names standard error's device and
/// inode.
fn stderr_is_journal() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let Some(e) = std::env::var_os("JOURNAL_STREAM") else {
            return false;
        };
        let e = os_bytes(&e);
        let Some(colon) = e.iter().position(|&c| c == b':') else {
            return false;
        };
        let (dev, ino) = (
            e.get(..colon).unwrap_or_default(),
            e.get(colon.saturating_add(1)..).unwrap_or_default(),
        );
        let (Some(dev), Some(ino)) = (safe_atou_max(dev, u64::MAX), safe_atou_max(ino, u64::MAX))
        else {
            return false;
        };
        stdfd::metadata(2).is_ok_and(|m| m.dev() == dev && m.ino() == ino)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Whether the journal file can be appended to: this system's answer to
/// "is journald there".
fn journal_writable() -> bool {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(journalrec::MAIN_LOG_PATH)
        .is_ok()
}

/// `format_timestamp (now)`: `Thu 2026-10-08 23:39:56 EDT`.
fn format_timestamp() -> Option<Vec<u8>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let secs = i64::try_from(now.as_secs()).ok()?;
    let tm = localtime::Zone::from_env().localtime(secs, 0);
    Some(localtime::strftime(b"%a %Y-%m-%d %H:%M:%S %Z", &tm))
}

/// `write_to_syslog`'s `strftime (..., "%h %e %T ", ...)`.
#[cfg(unix)]
fn syslog_timestamp() -> Option<Vec<u8>> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let secs = i64::try_from(now.as_secs()).ok()?;
    let tm = localtime::Zone::from_env().localtime(secs, 0);
    Some(localtime::strftime(b"%h %e %T ", &tm))
}

/// `%m`: `strerror (errno)`.
fn strerror(errno: i32) -> String {
    coreutils::errmsg::strerror(&io::Error::from_raw_os_error(errno))
}

/// The `errno` an error carries, or `EIO` for one that carries none.
#[cfg_attr(not(unix), allow(dead_code))]
fn errno_of(e: &io::Error) -> i32 {
    e.raw_os_error().unwrap_or(5)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The journal's spelling of a priority.
fn priority_name(priority: u8) -> &'static str {
    journalrec::PRIORITY_NAMES
        .get(usize::from(priority))
        .copied()
        .unwrap_or("info")
}

/// `/proc/PID/comm`, escaped as journald's `pid_get_comm` escapes it, while
/// the process is there.
fn read_comm(pid: u32) -> Option<Vec<u8>> {
    journalfwd::pid_get_comm(pid)
}

// ------------------------------------------------------------ arguments ----

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
struct Args {
    identifier: Option<Vec<u8>>,
    priority: u8,
    stderr_priority: Option<u8>,
    level_prefix: bool,
    /// The command and its arguments; empty for `/bin/cat`.
    command: Vec<OsString>,
}

/// `help`.
fn help(name: &[u8]) {
    let link = man_link();
    let (on, off) = (ansi(ANSI_HIGHLIGHT), ansi(ANSI_NORMAL));
    let mut m = name.to_vec();
    m.extend_from_slice(
        format!(
            " [OPTIONS...] COMMAND ...\n\
             \n{on}Execute process with stdout/stderr connected to the journal.{off}\n\n\
             \x20 -h --help                      Show this help\n\
             \x20    --version                   Show package version\n\
             \x20 -t --identifier=STRING         Set syslog identifier\n\
             \x20 -p --priority=PRIORITY         Set priority value (0..7)\n\
             \x20    --stderr-priority=PRIORITY  Set priority value (0..7) used for stderr\n\
             \x20    --level-prefix=BOOL         Control whether level prefix shall be parsed\n\
             \nSee the {link} for details.\n"
        )
        .as_bytes(),
    );
    // Unchecked, as upstream's `printf` is: its status is 0 whatever became
    // of the text.
    let _ = stdfd::write_all(1, &m);
}

/// `parse_argv`: the options, or the status to exit with.
fn parse_argv(argv: &[OsString], log: &mut Log) -> Result<Args, u8> {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let name = short_name(&argv0).to_vec();
    let mut args = Args {
        identifier: None,
        priority: LOG_INFO,
        stderr_priority: None,
        level_prefix: true,
        command: Vec::new(),
    };
    let value = |v: Option<OsString>| v.map(|v| os_bytes(&v).into_owned()).unwrap_or_default();
    let words = argv.get(1..).unwrap_or_default();
    for item in SYSTEMD_CAT.parse(words, "+ht:p:", LONGS) {
        match item {
            Err(e) => {
                // glibc's own message, under `argv[0]` as given; systemd adds
                // nothing to it.
                let mut m = argv0.clone();
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                stdfd::diag_bytes(&m);
                return Err(1);
            }
            Ok(Opt::Short(b'h', _) | Opt::Long("help", _)) => {
                help(&name);
                return Err(0);
            }
            Ok(Opt::Long("version", _)) => {
                // Unchecked, as `help` is.
                let _ = stdfd::write_all(1, b"systemd-cat (SlateOS coreutils) 0.1.0\n");
                return Err(0);
            }
            Ok(Opt::Short(b't', v) | Opt::Long("identifier", v)) => {
                let v = value(v);
                args.identifier = (!v.is_empty()).then_some(v);
            }
            Ok(Opt::Short(b'p', v) | Opt::Long("priority", v)) => {
                let Some(p) = log_level_from_string(&value(v)) else {
                    // `SYNTHETIC_ERRNO (EINVAL)`: no `ERRNO` field.
                    log.log(LOG_ERR, 0, AT_PRIORITY, b"Failed to parse priority value.");
                    return Err(1);
                };
                args.priority = p;
            }
            Ok(Opt::Long("stderr-priority", v)) => {
                let Some(p) = log_level_from_string(&value(v)) else {
                    log.log(
                        LOG_ERR,
                        // `SYNTHETIC_ERRNO (EINVAL)`, as above.
                        0,
                        AT_STDERR_PRIORITY,
                        b"Failed to parse stderr priority value.",
                    );
                    return Err(1);
                };
                args.stderr_priority = Some(p);
            }
            Ok(Opt::Long("level-prefix", v)) => {
                let v = value(v);
                let Some(b) = parse_boolean(&v) else {
                    let mut m = b"Failed to parse boolean argument to --level-prefix=: ".to_vec();
                    m.extend_from_slice(&v);
                    m.push(b'.');
                    log.log(LOG_ERR, 22, AT_BOOLEAN, &m);
                    return Err(1);
                };
                args.level_prefix = b;
            }
            Ok(Opt::Operand(o)) => args.command.push(o.clone()),
            // Every option in the table is matched above.
            Ok(Opt::Short(..) | Opt::Long(..)) => return Err(1),
        }
    }
    Ok(args)
}

// --------------------------------------------------------------- stream ----
mod journald {
    //! journald's half of a stream (`journald-stream.c`): the header that
    //! opens it, where its lines end, what each is filed as, and the record
    //! made of it. Off Unix nothing reads a stream, since nothing can become
    //! the command that writes one.
    #![cfg_attr(not(unix), allow(dead_code))]

    use super::{
        LOG_INFO, WHITESPACE, now_secs, parse_boolean, priority_name, safe_atou_max, short_name,
    };

    /// journald's `LineMax=` default: a longer line is cut there.
    pub const LINE_MAX: usize = 48 * 1024;
    /// `STDOUT_STREAM_SETUP_PROTOCOL_LINE_MAX`, `UNIT_NAME_MAX - 1`: the
    /// longest line of a stream's header. An identifier this long or longer
    /// leaves its line unfinished there, and journald refuses the stream.
    const SETUP_LINE_MAX: usize = 255;
    /// `TASK_COMM_LEN - 1`: how much of a program's name the kernel keeps.
    const COMM_MAX: usize = 15;
    /// `LOG_FACMASK`.
    const LOG_FACMASK: u32 = 0x3f8;
    /// The largest priority a header may give, facility included
    /// (`syslog_parse_priority_and_facility`).
    const PRIORITY_MAX: u64 = 999;

    /// The name the kernel gives a program run from `path`: its last
    /// component, cut to [`COMM_MAX`] bytes.
    pub fn comm_of(path: &[u8]) -> Vec<u8> {
        let base = short_name(path);
        base.get(..base.len().min(COMM_MAX))
            .unwrap_or(base)
            .to_vec()
    }

    /// `sd_journal_stream_fd`'s header, a line each: the identifier, an empty
    /// unit name, the priority, whether a line may carry its own, and three
    /// forwarding switches, off.
    pub fn header(identifier: Option<&[u8]>, priority: u8, level_prefix: bool) -> Vec<u8> {
        let mut h = identifier.unwrap_or_default().to_vec();
        h.extend_from_slice(b"\n\n");
        h.push(b'0'.wrapping_add(priority));
        h.push(b'\n');
        h.push(if level_prefix { b'1' } else { b'0' });
        h.extend_from_slice(b"\n0\n0\n0\n");
        h
    }

    /// Why journald ended a line: `LineBreak`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum LineBreak {
        Newline,
        Nul,
        LineMax,
        Eof,
        PidChange,
    }

    impl LineBreak {
        /// `line_break_field_table`: the `_LINE_BREAK` a record carries, if
        /// any.
        pub fn field(self) -> Option<&'static [u8]> {
            match self {
                Self::Newline => None,
                Self::Nul => Some(b"nul"),
                Self::LineMax => Some(b"line-max"),
                Self::Eof => Some(b"eof"),
                Self::PidChange => Some(b"pid-change"),
            }
        }
    }

    /// Where in its protocol a stream is: `StdoutStreamState`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum State {
        Identifier,
        UnitId,
        Priority,
        LevelPrefix,
        ForwardToSyslog,
        ForwardToKmsg,
        ForwardToConsole,
        Running,
    }

    /// journald gave up on the stream -- a header it could not read -- and
    /// closed it (`stdout_stream_destroy`). The writer's next write fails.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Refused;

    /// A stream as journald holds one: how far into its header it is, and
    /// what the header said.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct Stream {
        state: State,
        /// `SYSLOG_IDENTIFIER`.
        identifier: Option<Vec<u8>>,
        /// The lines' priority, facility included: 0 to 999.
        priority: u32,
        /// Whether a line's own `<N>` is read.
        level_prefix: bool,
        /// The header's three forwarding switches -- syslog, kmsg, console.
        /// Read, because the protocol has them, and not acted on yet:
        /// `known-issues/TD-B-A-STREAMS-FORWARDING-SWITCHES-ARE-READ-AND-IGNORED.md`. (The
        /// broadcast of an `emerg` line needs no switch, and is done.)
        forward: [bool; 3],
    }

    impl Default for Stream {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Stream {
        /// A stream just connected, its header still to come.
        pub fn new() -> Self {
            Self {
                state: State::Identifier,
                identifier: None,
                priority: u32::from(LOG_INFO),
                level_prefix: true,
                forward: [false; 3],
            }
        }

        /// `stdout_stream_line_max`.
        fn line_max(&self) -> usize {
            if self.state == State::Running {
                LINE_MAX
            } else {
                SETUP_LINE_MAX
            }
        }

        /// `stdout_stream_scan`: each line in `data` -- ended by a NUL, a
        /// newline, or the most a line may hold -- taken as the header's next
        /// line or handed to `file` with what ended it; with `force`, what is
        /// left is one more line, ended so. Returns how many bytes were used
        /// up -- the rest waits for more -- or [`Refused`].
        pub fn scan(
            &mut self,
            data: &[u8],
            force: Option<LineBreak>,
            file: &mut dyn FnMut(&Self, &[u8], LineBreak),
        ) -> Result<usize, Refused> {
            let mut consumed = 0usize;
            loop {
                // Asked each time round: the header's last line raises it.
                let line_max = self.line_max();
                let rest = data.get(consumed..).unwrap_or_default();
                let window = rest.get(..rest.len().min(line_max)).unwrap_or_default();
                let newline = window.iter().position(|&c| c == b'\n');
                let nul = window
                    .get(..newline.unwrap_or(window.len()))
                    .unwrap_or_default()
                    .iter()
                    .position(|&c| c == 0);
                let (found, skip, why) = match (nul, newline) {
                    (Some(z), _) => (z, z.saturating_add(1), LineBreak::Nul),
                    (None, Some(n)) => (n, n.saturating_add(1), LineBreak::Newline),
                    (None, None) if rest.len() >= line_max => {
                        (line_max, line_max, LineBreak::LineMax)
                    }
                    (None, None) => break,
                };
                self.line(rest.get(..found).unwrap_or_default(), why, file)?;
                consumed = consumed.saturating_add(skip);
            }
            if let Some(why) = force {
                let rest = data.get(consumed..).unwrap_or_default();
                if !rest.is_empty() {
                    self.line(rest, why, file)?;
                    consumed = data.len();
                }
            }
            Ok(consumed)
        }

        /// `stdout_stream_line`: the header, a line at a time, then the lines
        /// to file. A header line must end with a newline and say what its
        /// place in the header calls for.
        fn line(
            &mut self,
            line: &[u8],
            why: LineBreak,
            file: &mut dyn FnMut(&Self, &[u8], LineBreak),
        ) -> Result<(), Refused> {
            if self.state == State::Running {
                file(self, line, why);
                return Ok(());
            }
            if why != LineBreak::Newline {
                return Err(Refused);
            }
            let p = strip(line);
            let boolean = |p: &[u8]| parse_boolean(p).ok_or(Refused);
            self.state = match self.state {
                State::Identifier => {
                    self.identifier = (!p.is_empty()).then(|| p.to_vec());
                    State::UnitId
                }
                // A unit's name, honoured from root alone, and named in no
                // record here.
                State::UnitId => State::Priority,
                State::Priority => {
                    self.priority = safe_atou_max(p, PRIORITY_MAX)
                        .and_then(|v| u32::try_from(v).ok())
                        .ok_or(Refused)?;
                    State::LevelPrefix
                }
                State::LevelPrefix => {
                    self.level_prefix = boolean(p)?;
                    State::ForwardToSyslog
                }
                State::ForwardToSyslog => {
                    self.forward[0] = boolean(p)?;
                    State::ForwardToKmsg
                }
                State::ForwardToKmsg => {
                    self.forward[1] = boolean(p)?;
                    State::ForwardToConsole
                }
                State::ForwardToConsole => {
                    self.forward[2] = boolean(p)?;
                    State::Running
                }
                State::Running => State::Running,
            };
            Ok(())
        }
    }

    /// `strstrip`: white space off both ends.
    fn strip(line: &[u8]) -> &[u8] {
        let start = line
            .iter()
            .position(|c| !WHITESPACE.contains(c))
            .unwrap_or(line.len());
        let end = line
            .iter()
            .rposition(|c| !WHITESPACE.contains(c))
            .map_or(start, |e| e.saturating_add(1));
        line.get(start..end).unwrap_or_default()
    }

    /// What a line is filed as: `stdout_stream_line`'s `strstrip` -- which cuts
    /// trailing white space in place unless the line is nothing else, while the
    /// line is still logged from its start -- then, with `level_prefix`,
    /// `syslog_parse_priority`'s `<N>`, which replaces the level and keeps the
    /// facility. `None` for a line left empty, which journald does not file.
    pub fn message(line: &[u8], level_prefix: bool, priority: u32) -> Option<(u32, &[u8])> {
        let mut line = line;
        if let Some(last) = line.iter().rposition(|c| !WHITESPACE.contains(c)) {
            line = line.get(..last.saturating_add(1)).unwrap_or_default();
        }
        let mut priority = priority;
        if level_prefix
            && line.first() == Some(&b'<')
            && let Some(k) = line.iter().position(|&c| c == b'>')
            && (2..=4).contains(&k)
        {
            // `undecchar` each of one to three digits; with no facility, only
            // zeros before a last digit of at most 7.
            let digits = line.get(1..k).unwrap_or_default();
            let (head, last) = digits.split_at(digits.len().saturating_sub(1));
            if digits.iter().all(u8::is_ascii_digit)
                && head.iter().all(|&d| d == b'0')
                && let Some(&c) = last.first()
                && c <= b'7'
            {
                priority = (priority & LOG_FACMASK) | u32::from(c.wrapping_sub(b'0'));
                line = line.get(k.saturating_add(1)..).unwrap_or_default();
            }
        }
        (!line.is_empty()).then_some((priority, line))
    }

    /// Who wrote a line, as far as the kernel said.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Writer {
        pub pid: i32,
        pub uid: Option<u32>,
        pub gid: Option<u32>,
    }

    /// journald's forwarding of one line, done before the line is filed, as
    /// `stdout_stream_log` does it: a line at `emerg` broadcast to every
    /// logged-in user's terminal (`ForwardToWall`, on by default), as
    /// `IDENTIFIER[PID]: MESSAGE`. A line journald would not file is not
    /// forwarded either.
    pub fn forward(stream: &Stream, writer: &Writer, line: &[u8]) {
        if let Some((priority, text)) = message(line, stream.level_prefix, stream.priority) {
            journalfwd::forward_wall(
                priority,
                stream.identifier.as_deref(),
                text,
                u32::try_from(writer.pid).ok(),
            );
        }
    }

    /// The record journald would make of one line, as a journal line with its
    /// newline; `None` for a line it would not file. `name` gives a writer's
    /// command name, asked only for a line that is filed -- journald reads a
    /// writer's details when it first files one of its lines, not before.
    pub fn record(
        stream: &Stream,
        writer: &Writer,
        name: &mut dyn FnMut(i32) -> Option<Vec<u8>>,
        line: &[u8],
        why: LineBreak,
    ) -> Option<Vec<u8>> {
        let (priority, text) = message(line, stream.level_prefix, stream.priority)?;
        let comm = name(writer.pid);
        let facility = (priority & LOG_FACMASK) >> 3;
        let facility_text = facility.to_string();
        let pid_text = writer.pid.to_string();
        let uid_text = writer.uid.map(|u| u.to_string());
        let gid_text = writer.gid.map(|g| g.to_string());
        let mut extra: Vec<(&str, &[u8])> = Vec::new();
        // journald files a facility only when the priority has one.
        if facility != 0 {
            if let Some(name) = journalrec::facility_name(facility) {
                extra.push(("facility", name.as_bytes()));
            }
            extra.push(("SYSLOG_FACILITY", facility_text.as_bytes()));
        }
        extra.push(("_PID", pid_text.as_bytes()));
        if let Some(u) = uid_text.as_deref() {
            extra.push(("_UID", u.as_bytes()));
        }
        if let Some(g) = gid_text.as_deref() {
            extra.push(("_GID", g.as_bytes()));
        }
        if let Some(c) = comm.as_deref() {
            extra.push(("_COMM", c));
        }
        extra.push(("_TRANSPORT", b"stdout"));
        if let Some(field) = why.field() {
            extra.push(("_LINE_BREAK", field));
        }
        let level = u8::try_from(priority & 7).map_or("info", priority_name);
        let record = journalrec::ByteRecord {
            ts: now_secs(),
            level,
            service: stream
                .identifier
                .as_deref()
                .or(comm.as_deref())
                .unwrap_or_default(),
            msg: text,
            pid: u32::try_from(writer.pid).ok(),
        };
        let mut bytes = record.to_json_line_with(&extra).into_bytes();
        bytes.push(b'\n');
        Some(bytes)
    }

    #[cfg(test)]
    #[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
    mod tests {
        use super::*;

        /// The lines filed from `data`, and what the scan said.
        type Filed = (Vec<(Vec<u8>, LineBreak)>, Result<usize, Refused>);

        fn feed(stream: &mut Stream, data: &[u8], force: Option<LineBreak>) -> Filed {
            let mut got = Vec::new();
            let r = stream.scan(data, force, &mut |_, l, why| got.push((l.to_vec(), why)));
            (got, r)
        }

        /// A stream past its header, as `systemd-cat` opens one.
        fn running(identifier: Option<&[u8]>, priority: u8, level_prefix: bool) -> Stream {
            let mut s = Stream::new();
            let h = header(identifier, priority, level_prefix);
            let (filed, used) = feed(&mut s, &h, None);
            assert!(filed.is_empty(), "a header line was filed: {filed:?}");
            assert_eq!(used, Ok(h.len()));
            assert_eq!(s.state, State::Running);
            s
        }

        #[test]
        fn the_header_is_sd_journal_stream_fds() {
            assert_eq!(header(Some(b"T"), 6, true), b"T\n\n6\n1\n0\n0\n0\n");
            assert_eq!(header(None, 3, false), b"\n\n3\n0\n0\n0\n0\n");
        }

        #[test]
        fn the_header_sets_what_lines_are_filed_under() {
            let s = running(Some(b"tag"), 3, false);
            assert_eq!(s.identifier.as_deref(), Some(&b"tag"[..]));
            assert_eq!(s.priority, 3);
            assert!(!s.level_prefix);
        }

        #[test]
        fn an_identifier_is_stripped_as_journald_strips_it() {
            // Measured against systemd 255's journald.
            let s = running(Some(b"  spaced  "), 6, true);
            assert_eq!(s.identifier.as_deref(), Some(&b"spaced"[..]));
            let s = running(Some(b"   "), 6, true);
            assert_eq!(s.identifier, None);
            let s = running(Some(b"tab\there"), 6, true);
            assert_eq!(s.identifier.as_deref(), Some(&b"tab\there"[..]));
        }

        #[test]
        fn a_header_journald_cannot_read_refuses_the_stream() {
            // A newline in the identifier pushes an empty line into the
            // priority's place.
            let mut s = Stream::new();
            assert_eq!(
                feed(&mut s, &header(Some(b"a\nb"), 6, true), None).1,
                Err(Refused)
            );
            // The longest identifier is 254 bytes; at 255 the line runs out
            // before its newline.
            let ok = vec![b'i'; 254];
            running(Some(&ok), 6, true);
            let long = vec![b'j'; 255];
            let mut s = Stream::new();
            assert_eq!(
                feed(&mut s, &header(Some(&long), 6, true), None).1,
                Err(Refused)
            );
            // A header cut short by the end of the stream, or by a NUL.
            let mut s = Stream::new();
            assert_eq!(
                feed(&mut s, b"T\n\n6", Some(LineBreak::Eof)).1,
                Err(Refused)
            );
            let mut s = Stream::new();
            assert_eq!(feed(&mut s, b"T\0", None).1, Err(Refused));
            // A priority past 999, a level prefix that is no boolean.
            let mut s = Stream::new();
            assert_eq!(feed(&mut s, b"T\n\n1000\n", None).1, Err(Refused));
            let mut s = Stream::new();
            assert_eq!(feed(&mut s, b"T\n\n6\nmaybe\n", None).1, Err(Refused));
        }

        #[test]
        fn a_header_written_in_the_identifier_is_obeyed() {
            // Measured: `-t $'x\n\n27\n0\n0\n0\n0'` files at priority 27
            // (daemon.err) with level prefixes off, and the rest of the real
            // header as lines.
            let mut s = Stream::new();
            let (filed, used) = feed(&mut s, &header(Some(b"x\n\n27\n0\n0\n0\n0"), 6, true), None);
            assert!(used.is_ok());
            assert_eq!(s.identifier.as_deref(), Some(&b"x"[..]));
            assert_eq!(s.priority, 27);
            assert!(!s.level_prefix);
            // The real identifier line's end is an empty line, which journald
            // files nothing for; then the real header's own lines.
            let lines: Vec<&[u8]> = filed.iter().map(|(l, _)| l.as_slice()).collect();
            assert_eq!(lines, [&b""[..], b"6", b"1", b"0", b"0", b"0"]);
            let w = Writer {
                pid: 9,
                uid: None,
                gid: None,
            };
            assert!(record(&s, &w, &mut |_| None, b"", LineBreak::Newline).is_none());
            let rec = record(
                &s,
                &Writer {
                    pid: 9,
                    uid: None,
                    gid: None,
                },
                &mut |_| None,
                b"line",
                LineBreak::Newline,
            )
            .unwrap();
            let text = String::from_utf8(rec).unwrap();
            assert!(text.contains(r#""level":"err""#), "{text}");
            assert!(text.contains(r#""facility":"daemon""#), "{text}");
            assert!(text.contains(r#""SYSLOG_FACILITY":"3""#), "{text}");
        }

        #[test]
        fn lines_are_trimmed_and_prefixed_as_journald_files_them() {
            // Each measured against systemd 255's journald.
            let m = |s: &[u8]| message(s, true, 6).map(|(p, t)| (p, t.to_vec()));
            assert_eq!(m(b"tr  "), Some((6, b"tr".to_vec())));
            assert_eq!(m(b"   "), Some((6, b"   ".to_vec())));
            assert_eq!(m(b"\t"), Some((6, b"\t".to_vec())));
            assert_eq!(m(b"x\r\r"), Some((6, b"x".to_vec())));
            assert_eq!(m(b" end"), Some((6, b" end".to_vec())));
            assert_eq!(m(b"a\rb"), Some((6, b"a\rb".to_vec())));
            assert_eq!(m(b""), None);
            assert_eq!(m(b"<7>p7"), Some((7, b"p7".to_vec())));
            assert_eq!(m(b"<07>p"), Some((7, b"p".to_vec())));
            assert_eq!(m(b"<007>p"), Some((7, b"p".to_vec())));
            assert_eq!(m(b"<8>p8"), Some((6, b"<8>p8".to_vec())));
            assert_eq!(m(b"<12>p"), Some((6, b"<12>p".to_vec())));
            assert_eq!(m(b"<1>"), None);
            assert_eq!(m(b"<1>   "), None);
            assert_eq!(m(b"<3>\r"), None);
            assert_eq!(m(b"<3>  sp"), Some((3, b"  sp".to_vec())));
            assert_eq!(m(b"< 3>x"), Some((6, b"< 3>x".to_vec())));
            assert_eq!(m(b"<3x>y"), Some((6, b"<3x>y".to_vec())));
            assert_eq!(m(b"<>e"), Some((6, b"<>e".to_vec())));
            assert_eq!(m(b"<3"), Some((6, b"<3".to_vec())));
            assert_eq!(m(b"<0007>x"), Some((6, b"<0007>x".to_vec())));
            assert_eq!(message(b"<2>x", false, 6).map(|(p, _)| p), Some(6));
            // A prefix replaces the level and keeps the header's facility.
            assert_eq!(message(b"<5>x", true, 27).map(|(p, _)| p), Some(29));
        }

        #[test]
        fn streams_are_cut_at_nuls_newlines_and_the_line_maximum() {
            let mut s = running(Some(b"T"), 6, true);
            let (got, used) = feed(&mut s, b"a\nb\0c\npartial", None);
            assert_eq!(
                got,
                vec![
                    (b"a".to_vec(), LineBreak::Newline),
                    (b"b".to_vec(), LineBreak::Nul),
                    (b"c".to_vec(), LineBreak::Newline),
                ]
            );
            assert_eq!(used, Ok(6));
            let (got, used) = feed(&mut s, b"partial", Some(LineBreak::Eof));
            assert_eq!(got, vec![(b"partial".to_vec(), LineBreak::Eof)]);
            assert_eq!(used, Ok(7));
            // Nothing left over is no line, forced or not.
            assert_eq!(feed(&mut s, b"x\n", Some(LineBreak::Eof)).0.len(), 1);
            // A full buffer with no end in it is a line of its own.
            let big = vec![b'L'; LINE_MAX];
            let (got, used) = feed(&mut s, &big, None);
            assert_eq!(got.len(), 1);
            assert_eq!(got[0].0.len(), LINE_MAX);
            assert_eq!(got[0].1, LineBreak::LineMax);
            assert_eq!(used, Ok(LINE_MAX));
        }

        #[test]
        fn a_record_carries_journalds_fields() {
            let s = running(Some(b"tag"), 6, true);
            let writer = Writer {
                pid: 42,
                uid: Some(1000),
                gid: Some(100),
            };
            let r = record(
                &s,
                &writer,
                &mut |_| Some(b"sh".to_vec()),
                b"<3>boom  ",
                LineBreak::Eof,
            )
            .unwrap();
            let text = String::from_utf8(r).unwrap();
            assert!(text.ends_with('\n'));
            for part in [
                r#""level":"err""#,
                r#""service":"tag""#,
                r#""msg":"boom""#,
                r#""pid":42"#,
                r#""_PID":"42""#,
                r#""_UID":"1000""#,
                r#""_GID":"100""#,
                r#""_COMM":"sh""#,
                r#""_TRANSPORT":"stdout""#,
                r#""_LINE_BREAK":"eof""#,
            ] {
                assert!(text.contains(part), "{part} in {text}");
            }
            assert!(!text.contains("FACILITY"), "{text}");
            // No identifier: the writer's name.
            let bare = running(None, 6, true);
            let r = record(
                &bare,
                &writer,
                &mut |_| Some(b"echo".to_vec()),
                b"hi",
                LineBreak::Newline,
            )
            .unwrap();
            let r = String::from_utf8(r).unwrap();
            assert!(r.contains(r#""service":"echo""#), "{r}");
            assert!(!r.contains("_LINE_BREAK"), "{r}");
            // Nothing to file: no record, and nobody's name asked for.
            let asked = record(
                &bare,
                &writer,
                &mut |_| panic!("name asked"),
                b"<5>",
                LineBreak::Newline,
            );
            assert!(asked.is_none());
        }

        #[test]
        fn a_line_that_is_not_text_is_kept_as_bytes() {
            let s = running(None, 6, true);
            let writer = Writer {
                pid: 1,
                uid: None,
                gid: None,
            };
            let r = record(&s, &writer, &mut |_| None, b"a\xffb", LineBreak::Newline).unwrap();
            let r = String::from_utf8(r).unwrap();
            assert!(r.contains(r#""msg":[97,255,98]"#), "{r}");
            assert!(!r.contains("_UID"), "{r}");
        }
    }
}

// ------------------------------------------------------------- plumbing ----

#[cfg(unix)]
mod plumbing {
    //! The streams, the helpers that read them, and the `exec`.

    use super::journald::{
        LINE_MAX, LineBreak, Refused, Stream, Writer, comm_of, forward, header, record,
    };
    use super::{
        AT_EXECUTE, AT_FSTAT, AT_REARRANGE, AT_STDERR_STREAM, AT_STREAM, Args, Log, errno_of,
        read_comm,
    };
    use coreutils::quote::os_bytes;
    use coreutils::stdfd;
    use libcall::process::{Forked, exit_immediately};
    use std::collections::HashMap;
    use std::ffi::CString;
    use std::io::{self, Write};
    use std::os::fd::{AsRawFd, IntoRawFd, OwnedFd, RawFd};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    /// `EINTR`.
    const EINTR: i32 = 4;
    /// `EINVAL`.
    const EINVAL: i32 = 22;
    /// `EPIPE`.
    const EPIPE: i32 = 32;
    /// `SIGPIPE`.
    const SIGPIPE: i32 = 13;

    /// `run`, from the journal streams on.
    pub fn run(args: &Args, log: &mut Log) -> u8 {
        // `sd_journal_stream_fd` fails when journald is not there to take the
        // stream. Here that is the journal file, which the helpers write: one
        // this process cannot append to, it says so about now, as upstream
        // does about a journald it cannot reach.
        if let Err(e) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(journalrec::MAIN_LOG_PATH)
        {
            log.error(errno_of(&e), AT_STREAM, "Failed to create stream fd: ", "");
            return 1;
        }
        let me = i32::try_from(std::process::id()).unwrap_or(0);
        // The command's own name, for a line it wrote after it had gone: what
        // the kernel will call it once this process has become it.
        let command_comm = args
            .command
            .first()
            .map_or_else(|| b"cat".to_vec(), |c| comm_of(&os_bytes(c)));
        let identifier = args.identifier.as_deref();

        let opened = open_stream(
            identifier,
            args.priority,
            args.level_prefix,
            me,
            &command_comm,
            &[],
        );
        let out = match opened {
            Ok(fd) => fd,
            Err(e) => return stream_failed(&e, log, AT_STREAM),
        };
        let err = match args.stderr_priority {
            Some(p) if p != args.priority => {
                let others = [out.as_raw_fd()];
                match open_stream(identifier, p, args.level_prefix, me, &command_comm, &others) {
                    Ok(fd) => Some(fd),
                    Err(e) => return stream_failed(&e, log, AT_STDERR_STREAM),
                }
            }
            _ => None,
        };

        // `fcntl (STDERR_FILENO, F_DUPFD_CLOEXEC, 3)`, kept to say why the
        // `exec` failed where the caller will see it.
        let saved_stderr = libcall::fd::dup_above(2, 3).ok();

        let out_fd = out.as_raw_fd();
        let err_fd = err.as_ref().map_or(out_fd, AsRawFd::as_raw_fd);
        let arranged = rearrange_stdio(out_fd, err_fd);
        // `rearrange_stdio` "invalidates" what it was given: a stream above
        // the standard three is closed, one that landed on 0, 1 or 2 -- the
        // caller had closed that one -- now is that descriptor.
        for fd in [Some(out), err].into_iter().flatten() {
            if fd.as_raw_fd() <= 2 {
                let _ = fd.into_raw_fd();
            }
        }
        if let Err(errno) = arranged {
            log.error(
                errno,
                AT_REARRANGE,
                "Failed to rearrange stdout/stderr: ",
                "",
            );
            return 1;
        }

        let argv: Vec<Vec<u8>> = if args.command.is_empty() {
            vec![b"/bin/cat".to_vec()]
        } else {
            // `JOURNAL_STREAM`: the stream's device and inode, by which the
            // command can tell that its standard error is the journal.
            let meta = match stdfd::metadata(2) {
                Ok(m) => m,
                Err(e) => {
                    log.error(
                        errno_of(&e),
                        AT_FSTAT,
                        "Failed to fstat(/proc/self/fd/2): ",
                        "",
                    );
                    return 1;
                }
            };
            {
                use std::os::unix::fs::MetadataExt;
                let value = format!("{}:{}", meta.dev(), meta.ino());
                // SAFETY: this process has one thread -- the helpers are
                // processes of their own -- so nothing reads the environment
                // while it changes.
                unsafe { std::env::set_var("JOURNAL_STREAM", value) };
            }
            args.command
                .iter()
                .map(|a| os_bytes(a).into_owned())
                .collect()
        };

        // The command gets `SIGPIPE` as this process was given it, not as
        // the Rust runtime left it.
        if !stdfd::sigpipe_ignored_at_startup() {
            let _ = libcall::signal::set_default(SIGPIPE);
        }
        let errno = exec(&argv);

        // `dup3 (saved_stderr, STDERR_FILENO, 0)`: the message goes where
        // the caller's standard error was -- or, if there was none, into the
        // stream, as upstream's does.
        if let Some(saved) = saved_stderr {
            let _ = libcall::fd::dup2(saved, 2);
        }
        log.error(errno, AT_EXECUTE, "Failed to execute process: ", "");
        1
    }

    /// `log_error_errno (r, "Failed to create stream fd: %m")` -- unless the
    /// failure was the header's write finding the stream already refused,
    /// which upstream, with `SIGPIPE` at its default, does not live to report.
    fn stream_failed(e: &io::Error, log: &mut Log, site: super::Site) -> u8 {
        if e.raw_os_error() == Some(EPIPE) && !stdfd::sigpipe_ignored_at_startup() {
            let _ = libcall::signal::set_default(SIGPIPE);
            let _ = libcall::signal::raise(SIGPIPE);
        }
        log.error(errno_of(e), site, "Failed to create stream fd: ", "");
        1
    }

    /// `execvp (argv[0], argv)`; returns only on failure, with the `errno`.
    fn exec(argv: &[Vec<u8>]) -> i32 {
        let Ok(owned) = argv
            .iter()
            .map(|a| CString::new(a.clone()))
            .collect::<Result<Vec<CString>, _>>()
        else {
            // A NUL inside an argument: no `argv` word can hold one.
            return EINVAL;
        };
        let refs: Vec<&std::ffi::CStr> = owned.iter().map(CString::as_c_str).collect();
        let mut slots = vec![std::ptr::null::<u8>(); refs.len().saturating_add(1)];
        libcall::process::execvp(&refs, &mut slots)
    }

    /// `rearrange_stdio (STDIN_FILENO, out, err)`: standard input left where
    /// it is but kept across the `exec`, `out` and `err` put on 1 and 2. A
    /// stream that is itself on 0, 1 or 2 but not its own place is copied
    /// out of the way first, so that no `dup2` closes one still needed.
    fn rearrange_stdio(out: RawFd, err: RawFd) -> Result<(), i32> {
        let mut fds = [0, out, err];
        let mut copies: Vec<RawFd> = Vec::new();
        let mut result = Ok(());
        for (i, fd) in (0..).zip(fds.iter_mut()) {
            if *fd != i && *fd < 3 {
                match libcall::fd::dup_above(*fd, 3) {
                    Ok(copy) => {
                        copies.push(copy);
                        *fd = copy;
                    }
                    Err(e) => {
                        result = Err(e);
                        break;
                    }
                }
            }
        }
        if result.is_ok() {
            for (i, &fd) in (0..).zip(fds.iter()) {
                let step = if fd == i {
                    libcall::fd::set_close_on_exec(i, false)
                } else {
                    libcall::fd::dup2(fd, i)
                };
                if let Err(e) = step {
                    result = Err(e);
                    break;
                }
            }
        }
        for copy in copies {
            // The copies were this function's own, made close-on-exec; a close
            // that fails leaves nothing to undo.
            let _ = libcall::fd::close(copy);
        }
        result
    }

    /// `sd_journal_stream_fd`: a stream whose other end a helper reads and
    /// files, its header written, ready to be some process's standard
    /// output. `others` are the streams made before this one, which the
    /// helper must not hold open.
    fn open_stream(
        identifier: Option<&[u8]>,
        priority: u8,
        level_prefix: bool,
        me: i32,
        command_comm: &[u8],
        others: &[RawFd],
    ) -> io::Result<OwnedFd> {
        let (client, server) = UnixStream::pair()?;
        // journald's end: who wrote each piece, asked before anything is
        // written, and who made the connection. Without the first, every
        // line is the connection's maker's -- the command's -- as it is with
        // journald on a stream that carries no credentials.
        let credentials = libcall::socket::pass_credentials(server.as_raw_fd()).is_ok();
        let maker = match libcall::socket::peer_credentials(server.as_raw_fd()) {
            Ok(c) => Writer {
                pid: c.pid,
                uid: Some(c.uid),
                gid: Some(c.gid),
            },
            Err(_) => Writer {
                pid: me,
                uid: None,
                gid: None,
            },
        };
        let client = OwnedFd::from(client);
        let mut close: Vec<RawFd> = others.to_vec();
        close.push(client.as_raw_fd());
        // The helper first: it is reading by the time the header is written,
        // as journald is, so a header too long for the socket to hold is
        // refused rather than left blocking.
        spawn_helper(&server, &close, credentials, maker, me, command_comm)?;
        drop(server);
        let mut client = UnixStream::from(client);
        // `shutdown (fd, SHUT_RD)`: nothing comes back on a journal stream.
        // Unchecked: a library that cannot shut half a socket leaves it
        // whole, and nothing ever writes to this end anyway.
        let _ = client.shutdown(std::net::Shutdown::Read);
        client.write_all(&header(identifier, priority, level_prefix))?;
        Ok(OwnedFd::from(client))
    }

    /// Start the helper for `server`: two forks, so that it is nobody's child
    /// but init's; this process waits only for the one in between.
    fn spawn_helper(
        server: &UnixStream,
        close: &[RawFd],
        credentials: bool,
        maker: Writer,
        me: i32,
        command_comm: &[u8],
    ) -> io::Result<()> {
        // SAFETY: this process has one thread -- nothing here starts another
        // -- so the copy `fork` makes may do anything the original could,
        // allocation included.
        match unsafe { libcall::process::fork() } {
            Err(e) => Err(io::Error::from_raw_os_error(e)),
            Ok(Forked::Child) => {
                // SAFETY: as above; this copy has one thread too.
                match unsafe { libcall::process::fork() } {
                    Ok(Forked::Child) => {
                        helper(
                            server.as_raw_fd(),
                            close,
                            credentials,
                            maker,
                            me,
                            command_comm,
                        );
                        exit_immediately(0)
                    }
                    Ok(Forked::Parent(_)) => exit_immediately(0),
                    // The reason, as the status: `errno`s fit.
                    Err(e) => exit_immediately(e),
                }
            }
            Ok(Forked::Parent(middle)) => {
                let status = loop {
                    match libcall::process::wait(middle) {
                        Ok(s) => break s,
                        Err(EINTR) => {}
                        Err(e) => return Err(io::Error::from_raw_os_error(e)),
                    }
                };
                match status.exit_code() {
                    Some(0) => Ok(()),
                    Some(e) => Err(io::Error::from_raw_os_error(e)),
                    None => Err(io::Error::from_raw_os_error(EINTR)),
                }
            }
        }
    }

    /// The helper: journald's half of the stream. Stands apart first -- its
    /// own session, so that neither the terminal's signals nor its hangup
    /// reach it; its standard descriptors on `/dev/null` and every other but
    /// the stream closed, so that nothing the caller waits on to be closed
    /// waits on it; `/` as its directory, so that it keeps no file system
    /// busy -- then reads to the stream's end.
    fn helper(
        fd: RawFd,
        close: &[RawFd],
        credentials: bool,
        maker: Writer,
        me: i32,
        command_comm: &[u8],
    ) {
        // Each unchecked: a helper that could not stand apart still files
        // every line, which is what it is for.
        let _ = libcall::process::start_session();
        let _ = std::env::set_current_dir("/");
        detach(fd, close);
        let mut names = Names {
            me,
            command: command_comm.to_vec(),
            seen: HashMap::new(),
        };
        pump(fd, credentials, maker, &mut names);
    }

    /// Standard descriptors onto `/dev/null`; every other but `keep` closed.
    fn detach(keep: RawFd, known: &[RawFd]) {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/null")
        {
            Ok(null) => {
                let n = null.as_raw_fd();
                for std_fd in 0..3 {
                    if n != std_fd {
                        let _ = libcall::fd::dup2(n, std_fd);
                    }
                }
                if n <= 2 {
                    // It landed on a standard number, which it now is.
                    let _ = null.into_raw_fd();
                }
            }
            Err(_) => {
                for std_fd in 0..3 {
                    let _ = libcall::fd::close(std_fd);
                }
            }
        }
        for &fd in known {
            if fd != keep && fd > 2 {
                let _ = libcall::fd::close(fd);
            }
        }
        // And whatever else was inherited, where the system will list it.
        let listed: Vec<RawFd> = std::fs::read_dir("/proc/self/fd")
            .map(|dir| {
                dir.filter_map(|e| e.ok()?.file_name().to_str()?.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        for fd in listed {
            if fd != keep && fd > 2 {
                // The listing's own descriptor is among them, closed already.
                let _ = libcall::fd::close(fd);
            }
        }
    }

    /// Writers' command names, as journald's client contexts keep them: read
    /// when a writer's first line is filed, read again when a line comes more
    /// than a second after the last reading (`REFRESH_USEC`), and kept when
    /// the process has gone by then. The command itself, gone before its
    /// first line was filed, is named by what it ran.
    struct Names {
        me: i32,
        command: Vec<u8>,
        seen: HashMap<i32, (Option<Vec<u8>>, Instant)>,
    }

    impl Names {
        fn get(&mut self, pid: i32) -> Option<Vec<u8>> {
            let now = Instant::now();
            let kept = match self.seen.get(&pid) {
                Some((name, at)) if now.duration_since(*at) < Duration::from_secs(1) => {
                    return name.clone();
                }
                Some((name, _)) => name.clone(),
                None => None,
            };
            let name = u32::try_from(pid)
                .ok()
                .and_then(read_comm)
                .or(kept)
                .or_else(|| (pid == self.me).then(|| self.command.clone()));
            self.seen.insert(pid, (name.clone(), now));
            name
        }
    }

    /// `stdout_stream_process`, to the stream's end: read what is there --
    /// never more than a line's worth -- and file the lines in it; when the
    /// kernel names a writer other than the last, file what the last one
    /// left unfinished as its own (`_LINE_BREAK=pid-change`) first.
    fn pump(fd: RawFd, credentials: bool, maker: Writer, names: &mut Names) {
        let mut stream = Stream::new();
        let mut buf = vec![0u8; LINE_MAX];
        let mut len = 0usize;
        let mut writer = maker;
        loop {
            let room = buf.get_mut(len..).unwrap_or_default();
            let got = if credentials {
                libcall::socket::receive(fd, room).map(|r| (r.len, r.sender))
            } else {
                stdfd::read(fd, room)
                    .map(|n| (n, None))
                    .map_err(|e| errno_of(&e))
            };
            let (n, sender) = match got {
                Ok(x) => x,
                Err(EINTR) => continue,
                // journald gives up on a stream it cannot read, without
                // filing what it held.
                Err(_) => return,
            };
            let mut records = Vec::new();
            if n == 0 {
                let held = buf.get(..len).unwrap_or_default();
                // Refused or not, the stream ends here.
                let _ = file(
                    &mut stream,
                    &writer,
                    names,
                    held,
                    Some(LineBreak::Eof),
                    &mut records,
                );
                let _ = append(&records);
                return;
            }
            let end = len.saturating_add(n);
            let start = match sender {
                Some(s) if s.pid != writer.pid => {
                    let held = buf.get(..len).unwrap_or_default();
                    let flushed = file(
                        &mut stream,
                        &writer,
                        names,
                        held,
                        Some(LineBreak::PidChange),
                        &mut records,
                    );
                    if flushed.is_err() {
                        let _ = append(&records);
                        return;
                    }
                    len
                }
                _ => 0,
            };
            if let Some(s) = sender {
                writer = Writer {
                    pid: s.pid,
                    uid: Some(s.uid),
                    gid: Some(s.gid),
                };
            }
            let data = buf.get(start..end).unwrap_or_default();
            let used = file(&mut stream, &writer, names, data, None, &mut records);
            // A journal that cannot be written ends the stream, as a journald
            // that goes away does: the writer's next write fails. So does a
            // header journald would refuse.
            let Ok(used) = used else {
                let _ = append(&records);
                return;
            };
            if append(&records).is_err() {
                return;
            }
            let from = start.saturating_add(used);
            buf.copy_within(from..end, 0);
            len = end.saturating_sub(from);
        }
    }

    /// The records for the lines `stream` finds in `data`, appended to
    /// `out`; how much of `data` they used.
    fn file(
        stream: &mut Stream,
        writer: &Writer,
        names: &mut Names,
        data: &[u8],
        force: Option<LineBreak>,
        out: &mut Vec<u8>,
    ) -> Result<usize, Refused> {
        stream.scan(data, force, &mut |s, line, why| {
            forward(s, writer, line);
            if let Some(r) = record(s, writer, &mut |pid| names.get(pid), line, why) {
                out.extend_from_slice(&r);
            }
        })
    }

    /// The records, appended together under the journal's lock.
    fn append(records: &[u8]) -> io::Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        journalio::append(std::path::Path::new(journalrec::MAIN_LOG_PATH), records)
    }
}

/// `run`, off Unix: there is no `exec` to become the command with.
#[cfg(not(unix))]
mod plumbing {
    use super::{AT_STREAM, Args, Log};

    /// `ENOSYS`.
    const ENOSYS: i32 = 38;

    pub fn run(_args: &Args, log: &mut Log) -> u8 {
        log.error(ENOSYS, AT_STREAM, "Failed to create stream fd: ", "");
        1
    }
}

// ----------------------------------------------------------------- main ----

/// `main`, as `DEFINE_MAIN_FUNCTION` writes it.
fn run(argv: &[OsString]) -> u8 {
    let argv0 = argv
        .first()
        .map(|a| os_bytes(a).into_owned())
        .unwrap_or_default();
    let name = short_name(&argv0).to_vec();
    if argv0.is_empty() {
        // `assert_se (argc > 0 && !isempty (argv[0]))`.
        let mut log = Log::initial(&name);
        log.log(
            2,
            0,
            AT_ASSERT,
            b"Assertion 'argc > 0 && !isempty(argv[0])' failed at src/journal/cat.c:185, function main(). Aborting.",
        );
        std::process::abort();
    }
    let mut log = Log::setup(&name);
    let args = match parse_argv(argv, &mut log) {
        Ok(a) => a,
        Err(status) => return status,
    };
    plumbing::run(&args, &mut log)
}

fn main() -> ExitCode {
    stdfd::restore();
    let argv: Vec<OsString> = std::env::args_os().collect();
    ExitCode::from(run(&argv))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::journald::comm_of;
    use super::*;

    #[test]
    fn priorities_parse_as_log_level_from_string_does() {
        // Measured against systemd 255's `systemd-cat -p`, and read from
        // `safe_atou_full`.
        for (s, want) in [
            ("info", Some(6)),
            ("debug", Some(7)),
            ("3", Some(3)),
            (" 3", Some(3)),
            ("\t 3", Some(3)),
            ("007", Some(7)),
            ("0x3", Some(3)),
            ("0X7", Some(7)),
            ("0b11", Some(3)),
            ("0o7", Some(7)),
            ("0b", None),
            ("0x", None),
            ("3x", None),
            ("8", None),
            ("", None),
            ("error", None),
            ("warn", None),
            ("INFO", None),
            ("-0", Some(0)),
            ("-3", None),
            ("+3", Some(3)),
            ("3 ", None),
            ("08", None),
            ("99999999999999999999", None),
            ("\x0b3", Some(3)),
        ] {
            assert_eq!(log_level_from_string(s.as_bytes()), want, "{s:?}");
        }
    }

    #[test]
    fn booleans_parse_as_parse_boolean_does() {
        for (s, want) in [
            ("yes", Some(true)),
            ("ON", Some(true)),
            ("t", Some(true)),
            ("1", Some(true)),
            ("off", Some(false)),
            ("0", Some(false)),
            ("N", Some(false)),
            ("maybe", None),
            ("", None),
            ("on ", None),
        ] {
            assert_eq!(parse_boolean(s.as_bytes()), want, "{s:?}");
        }
    }

    #[test]
    fn a_command_is_known_by_its_last_component_cut_as_the_kernel_cuts_it() {
        assert_eq!(comm_of(b"/usr/bin/sh"), b"sh");
        assert_eq!(comm_of(b"python3.12-config"), b"python3.12-conf");
        assert_eq!(short_name(b"/usr/bin/systemd-cat"), b"systemd-cat");
        assert_eq!(short_name(b"systemd-cat"), b"systemd-cat");
    }

    #[test]
    fn targets_are_named_as_log_target_table_names_them() {
        assert_eq!(
            Target::from_name(b"console-prefixed"),
            Some(Target::ConsolePrefixed)
        );
        assert_eq!(Target::from_name(b"null"), Some(Target::Null));
        assert_eq!(Target::from_name(b"Console"), None);
        assert_eq!(Target::from_name(b""), None);
    }

    #[test]
    fn the_log_cuts_a_message_where_upstreams_buffer_ends() {
        let mut log = Log::initial(b"systemd-cat");
        log.target = Target::Null;
        // Nothing is written under the null target; the cut is what matters,
        // and `log` must not panic on a message far longer than the buffer.
        log.log(LOG_ERR, 0, AT_PRIORITY, &vec![b'x'; 10_000]);
        log.max_level = 2;
        log.log(LOG_ERR, 0, AT_PRIORITY, b"hidden");
    }
}
