//! `w` — show who is logged on and what they are doing.
//!
//! ```text
//! w [options] [user]
//! ```
//!
//! A transcription of procps-ng 4.0.4's `src/w.c`, as `./configure
//! --enable-w-from` builds it, with the library functions it calls in
//! [`coreutils::procps`]. Measured against that build, from the pristine
//! release, by `scripts/w-diff.sh` -- not against a distribution's `w`, which
//! links logind and (Ubuntu 24.04's) crashes in exactly the configuration this
//! port follows.
//!
//! # What it prints
//!
//! ```text
//!  20:14:05 up  3:01,  2 users,  load average: 0.08, 0.03, 0.01
//! USER     TTY      FROM             LOGIN@   IDLE   JCPU   PCPU WHAT
//! alice    pts/3    10.0.0.2         17:13    1:01m  0.04s  0.00s vim notes
//! ```
//!
//! The first line is `uptime`'s, from the same function. Then one row per
//! `USER_PROCESS` entry in `/var/run/utmp` with a name (or, given a USER,
//! whose name is that one) -- but only if the session's login process still
//! exists, and only if the name is a user in the password database: upstream
//! skips both without a word, a stale entry because its process is gone and an
//! unknown name because it cannot say whose processes to look for. `-u` drops
//! the second condition.
//!
//! * **IDLE** is the time since the terminal's device was last read
//!   (`stat`'s access time), or ` ?xdm? ` for a line beginning with `:`.
//! * **JCPU** is the CPU time of every process whose controlling terminal is
//!   the session's.
//! * **WHAT** is one process's command line, and **PCPU** its CPU time: the
//!   most recently started process on the terminal that is in its foreground
//!   process group and belongs to the user, or else the login process. (A
//!   third rule stands in the newest process on the terminal while WHAT is
//!   still `-`; it shows only when the login process's own command line is
//!   `-`, since the login process replaces it otherwise.)
//!
//! # `FROM` is on
//!
//! `w.c` compiles the `FROM` column in only under `W_SHOWFROM`, which
//! `--enable-w-from` defines, and `-f` *toggles* it. On here, as Debian and
//! Ubuntu configure it: a login's origin is the column most worth seeing on a
//! machine people log in to. So `w -f` hides it, and `w -i`, which asks for
//! the address in it, turns it on whatever came before.
//!
//! # Replaces `userspace/who`'s `w` personality
//!
//! `userspace/who` answered to `w` by reading its own `argv[0]`, and its rows
//! were a format of its own: a fixed 8/8/16/8/6/6/6 grid with `-` for an empty
//! host, `HH:MM` login times whatever their age, a header of
//! `  N users,` with two spaces and a plural `0 users`, and `up ?` for an
//! unreadable uptime. It matched a process to a session by reading
//! `/proc/<pid>/fd/0` -- so a process that had redirected its standard input
//! belonged to no session, and one reading a terminal it was not controlled by
//! belonged to that one -- and it took the WHAT column from the login process
//! alone, so every row of a busy machine said `-bash`. There were no options at
//! all: `w -h` printed the header it was asked not to. This is the one program
//! with the name now.
//!
//! # Deliberate differences from procps-ng 4.0.4
//!
//! 1. **`--version`** says `w from SlateOS coreutils 0.1.0`, in procps' shape.
//! 2. **A `/var/run/utmp` that exists and cannot be read is an error**:
//!    `w: /var/run/utmp: Permission denied`, status 1, before anything is
//!    printed. Upstream reads it through glibc's `getutxent`, which cannot
//!    report a failure, and prints a header over an empty table -- "nobody is
//!    logged in", which is not what happened. `users`, `who` and `pinky` make
//!    the same choice (see [`coreutils::utmp`]); a utmp that does not exist is
//!    still nobody.
//! 3. **utmp is read once**, and the header's count and the rows come from
//!    that one reading; upstream reads the file twice. They differ only if a
//!    login comes or goes in between.
//! 4. **`/proc` is read once**, for every row, where upstream reads the whole
//!    process table again for each session. The rows are then one photograph
//!    rather than several, and `w` on a machine with many sessions does not
//!    read `/proc` once per session.
//! 5. **An unreadable `/proc/loadavg` prints zeros.** Upstream prints whatever
//!    was on the stack: `procps_loadavg` returns before writing its results.
//!
//! # On SlateOS
//!
//! JCPU and the choice of WHAT need the kernel to say which terminal controls
//! each process and which process group is in its foreground -- fields 7 and
//! 8 of `/proc/<pid>/stat` -- and the device number of each terminal, from
//! `stat`. At the time of writing all three are constants there: every process
//! reports terminal 0 and no foreground group, and every device reports device
//! 0. So a session on the console, whose device also reads as 0, is credited
//! with every process on the machine for JCPU; one on a pty, which has no node
//! to `stat`, with none; and WHAT is the login process's command line either
//! way. See `requests/b-ad-proc-stat-reports-no-controlling-terminal.md`. The
//! port does not work around it: a guess at which processes belong to a
//! session is the bug this replaced, and the fields are the kernel's to fill.

// The host build stops at the `main` below that refuses to run.
#![cfg_attr(not(unix), allow(dead_code))]

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::procps::Task;
use coreutils::utmp::{Record, USER_PROCESS};
use localtime::Tm;
use std::ffi::OsString;
use std::io;

coreutils::guard_std_fds!();

/// procps exits 1 on a bad command line: `usage(stderr)`.
const W: Program = Program::new("w", 1);

/// procps-ng 4.0.4's `getopt_long` string, exactly.
const SHORT_OPTIONS: &str = "husfoVip";

/// procps-ng 4.0.4's `longopts[]`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("no-header", Takes::Nothing),
    ("no-current", Takes::Nothing),
    ("short", Takes::Nothing),
    ("from", Takes::Nothing),
    ("old-style", Takes::Nothing),
    ("ip-addr", Takes::Nothing),
    ("pids", Takes::Nothing),
    ("help", Takes::Nothing),
    ("version", Takes::Nothing),
];

/// glibc's `UT_NAMESIZE`: a utmp user name's field.
const UT_NAMESIZE: usize = 32;
/// glibc's `UT_HOSTSIZE`: a utmp host's field.
const UT_HOSTSIZE: usize = 256;
/// `MAX_CMD_WIDTH` and `MIN_CMD_WIDTH`: the WHAT column's bounds.
const MAX_CMD_WIDTH: i32 = 512;
const MIN_CMD_WIDTH: i32 = 7;
/// `MAX_CMD_WIDTH` as the byte count `strncpy` copies into WHAT's buffer.
const MAX_CMD_BYTES: usize = 512;

/// `AF_INET` and `AF_INET6`, as [`libcall::netdb`] names them.
const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;
/// `INET6_ADDRSTRLEN`: room for any IPv6 address and its NUL.
const INET6_ADDRSTRLEN: usize = 46;

/// `inet_ntop(af, src, buf, size)` into a buffer of `size` bytes: the text,
/// or `None` when the library refuses -- `ENOSPC` for a buffer too small.
type Ntop = dyn Fn(i32, &[u8], usize) -> Option<Vec<u8>>;

/// Upstream's `usage()`, byte for byte: on stdout for `--help`, on stderr
/// after every command-line error.
const HELP: &str = "
Usage:
 w [options] [user]

Options:
 -h, --no-header     do not print header
 -u, --no-current    ignore current process username
 -s, --short         short format
 -f, --from          show remote hostname field
 -o, --old-style     old style output
 -i, --ip-addr       display IP address instead of hostname (if possible)
 -p, --pids          show the PID(s) of processes in WHAT

     --help     display this help and exit
 -V, --version  output version information and exit

For more details see w(1).
";

/// What the switches asked for: upstream's locals of the same names.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Options {
    /// `-h` clears it: the status line and the column headings.
    header: bool,
    /// `-s` clears it: no LOGIN@, JCPU or PCPU.
    longform: bool,
    /// The FROM column. On by default (`W_SHOWFROM`); `-f` toggles it and
    /// `-i` sets it.
    from: bool,
    /// `-i`: the address in FROM, where there is one.
    ip_addresses: bool,
    /// `-p`: the login and WHAT process IDs before WHAT.
    pids: bool,
    /// `-u`: any user's processes may be WHAT, and a name need not be known.
    ignoreuser: bool,
    /// `-o`: the old interval formats.
    oldstyle: bool,
    /// The first operand: only sessions of this user. Any further operands
    /// are ignored, as upstream ignores them.
    user: Option<Vec<u8>>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            header: true,
            longform: true,
            from: true,
            ip_addresses: false,
            pids: false,
            ignoreuser: false,
            oldstyle: false,
            user: None,
        }
    }
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Request {
    Help,
    Version,
    Show(Options),
}

/// Upstream's `getopt_long` loop. `-V` and `--help` act where they are met,
/// as upstream's `exit` there does, so an error after them is never seen.
///
/// # Errors
///
/// getopt's sentence for an unknown, ambiguous or misused option -- which
/// upstream follows with the whole usage on stderr.
fn parse_args(args: &[OsString]) -> Result<Request, String> {
    let mut opts = Options::default();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    for item in W.parse(args, SHORT_OPTIONS, LONG_OPTIONS) {
        match item.map_err(|e| e.sentence)? {
            Opt::Short(b'h', _) | Opt::Long("no-header", _) => opts.header = false,
            Opt::Short(b's', _) | Opt::Long("short", _) => opts.longform = false,
            Opt::Short(b'f', _) | Opt::Long("from", _) => opts.from = !opts.from,
            Opt::Short(b'V', _) | Opt::Long("version", _) => return Ok(Request::Version),
            Opt::Short(b'u', _) | Opt::Long("no-current", _) => opts.ignoreuser = true,
            Opt::Short(b'o', _) | Opt::Long("old-style", _) => opts.oldstyle = true,
            Opt::Short(b'i', _) | Opt::Long("ip-addr", _) => {
                opts.ip_addresses = true;
                opts.from = true;
            }
            Opt::Short(b'p', _) | Opt::Long("pids", _) => opts.pids = true,
            Opt::Long("help", _) => return Ok(Request::Help),
            Opt::Operand(x) => operands.push(coreutils::quote::os_bytes(x).into_owned()),
            // Unreachable: every entry of both tables is matched above.
            Opt::Long(other, _) => return Err(format!("option '--{other}' is unhandled")),
            Opt::Short(other, _) => return Err(W.invalid_option(other).sentence),
        }
    }
    opts.user = operands.into_iter().next();
    Ok(Request::Show(opts))
}

/// glibc's `atoi`: `strtol` in base 10 -- leading C spaces, a sign, the
/// digits, saturating at `LONG_MAX`/`LONG_MIN` -- then C's conversion to
/// `int`, which keeps the low 32 bits. So `PROCPS_USERLEN=4294967304` is 8.
fn c_atoi(text: &[u8]) -> i32 {
    let mut rest = text;
    while let [b' ' | 0x09..=0x0d, tail @ ..] = rest {
        rest = tail;
    }
    let negative = rest.first() == Some(&b'-');
    if let [b'-' | b'+', tail @ ..] = rest {
        rest = tail;
    }
    // The magnitude, stopping at the first value past `LONG_MIN`'s.
    let limit: u64 = 1 << 63;
    let mut magnitude: u64 = 0;
    for &d in rest.iter().take_while(|b| b.is_ascii_digit()) {
        magnitude = magnitude
            .saturating_mul(10)
            .saturating_add(u64::from(d.saturating_sub(b'0')))
            .min(limit);
    }
    let long: i64 = if negative {
        // `-limit` is `LONG_MIN` exactly; anything larger saturates to it.
        0i64.checked_sub_unsigned(magnitude).unwrap_or(i64::MIN)
    } else {
        i64::try_from(magnitude).unwrap_or(i64::MAX)
    };
    let b = long.to_le_bytes();
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// The widths `main` settles before printing anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    /// The USER column: 8, or `PROCPS_USERLEN` if that is 8 to 32.
    userlen: usize,
    /// The FROM column: 16, or `PROCPS_FROMLEN` if that is 8 to 256.
    fromlen: usize,
    /// How much of WHAT fits, before `-p`'s IDs take their share.
    maxcmd: i32,
}

/// Upstream's two environment checks and its WHAT width, given the variables
/// and the terminal's width (`cols`, `None` when standard output has none or
/// says zero). Returns the warnings to print as well, in upstream's words --
/// each of which ends in a newline of its own, before `error()` adds one.
fn layout(
    opts: &Options,
    userlen_var: Option<&[u8]>,
    fromlen_var: Option<&[u8]>,
    cols: Option<u16>,
    columns_var: Option<&[u8]>,
) -> (Layout, Vec<String>) {
    let mut warnings = Vec::new();
    let mut userlen: i32 = 8;
    let mut fromlen: i32 = 16;
    if let Some(v) = userlen_var {
        userlen = c_atoi(v);
        if !(8..=32).contains(&userlen) {
            warnings.push(
                "User length environment PROCPS_USERLEN must be between 8 and 32, ignoring.\n"
                    .to_string(),
            );
            userlen = 8;
        }
    }
    if let Some(v) = fromlen_var {
        fromlen = c_atoi(v);
        if !(8..=256).contains(&fromlen) {
            warnings.push(
                "from length environment PROCPS_FROMLEN must be between 8 and 256, ignoring\n"
                    .to_string(),
            );
            fromlen = 16;
        }
    }
    let mut maxcmd = match (cols.filter(|&c| c > 0), columns_var) {
        (Some(c), _) => i32::from(c),
        (None, Some(v)) => c_atoi(v),
        (None, None) => MAX_CMD_WIDTH,
    };
    maxcmd = maxcmd.clamp(MIN_CMD_WIDTH, MAX_CMD_WIDTH);
    let from_width = if opts.from { fromlen } else { 0 };
    let times_width = if opts.longform { 20 } else { 0 };
    let taken = userlen
        .saturating_add(21)
        .saturating_add(from_width)
        .saturating_add(times_width);
    maxcmd = maxcmd
        .saturating_sub(taken)
        .clamp(MIN_CMD_WIDTH, MAX_CMD_WIDTH);
    let lay = Layout {
        userlen: usize::try_from(userlen).unwrap_or(8),
        fromlen: usize::try_from(fromlen).unwrap_or(16),
        maxcmd,
    };
    (lay, warnings)
}

/// The two heading lines, under the status line (`status`, empty when
/// `/proc/uptime` could not be read -- upstream then prints a blank line).
fn header(opts: &Options, lay: &Layout, status: &str) -> Vec<u8> {
    let mut h = format!(
        "{status}\n{:<width$} TTY      ",
        "USER",
        width = lay.userlen
    );
    if opts.from {
        h.push_str(&format!(
            "{:<width$}",
            "FROM",
            width = lay.fromlen.saturating_sub(1)
        ));
    }
    h.push_str(if opts.longform {
        "  LOGIN@   IDLE   JCPU   PCPU WHAT\n"
    } else {
        "   IDLE WHAT\n"
    });
    h.into_bytes()
}

/// `print_time_ival7`: an interval in seven columns.
///
/// The two styles disagree about which unit gets the `m`: by default it marks
/// hours-and-minutes (`2:30m`), and minutes-and-seconds have none (`2:30 `);
/// `-o` swaps them, and shows nothing at all for a minute or less.
fn time_ival7(t: i64, centi: i32, oldstyle: bool) -> String {
    if t < 0 {
        // The clock went back.
        return "   ?   ".to_string();
    }
    let days = t / (24 * 60 * 60);
    let hours = t / (60 * 60);
    let hour_minutes = (t / 60) % 60;
    let minutes = t / 60;
    let seconds = t % 60;
    if t >= 48 * 60 * 60 {
        format!(" {days:2}days")
    } else if t >= 60 * 60 {
        if oldstyle {
            format!(" {hours:2}:{hour_minutes:02} ")
        } else {
            format!(" {hours:2}:{hour_minutes:02}m")
        }
    } else if t > 60 {
        if oldstyle {
            format!(" {minutes:2}:{seconds:02}m")
        } else {
            format!(" {minutes:2}:{seconds:02} ")
        }
    } else if oldstyle {
        "       ".to_string()
    } else {
        format!(" {t:2}.{centi:02}s")
    }
}

/// `print_time_ival7` of a tick count, as `showinfo` converts it: whole
/// seconds by integer division, and the hundredths as `(ticks % hz) * (100.0 /
/// hz)` in `double`, truncated.
// The remainder is below `hz`, a small positive number: exact as a `double`,
// and its hundredths are below 100.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
fn ticks_ival7(ticks: u64, hz: i64, oldstyle: bool) -> String {
    let hz = u64::try_from(hz).ok().filter(|&h| h > 0).unwrap_or(100);
    // `unsigned long long` handed to a `time_t`: C keeps the bits.
    let secs = i64::from_le_bytes(ticks.checked_div(hz).unwrap_or(0).to_le_bytes());
    let centi = (ticks.checked_rem(hz).unwrap_or(0) as f64 * (100.0 / hz as f64)) as i32;
    time_ival7(secs, centi, oldstyle)
}

/// `%b` and `%a` in the C locale, through the same `strftime` as `date`.
fn tm_name(fmt: &[u8], tm: &Tm) -> String {
    String::from_utf8(localtime::nstrftime(fmt, tm)).unwrap_or_default()
}

/// `print_logintime`: when the session began, in seven columns.
///
/// ` HH:MM  ` if it was less than twelve hours ago or earlier the same day of
/// the year; ` DddHH  ` -- weekday and hour -- within six days; ` DDMonYY`
/// before that. "The same day" is `tm_yday` alone, as upstream compares it,
/// so a login on this date a year ago counts as today once it is twelve
/// hours old.
fn logintime(now: i64, logt: i64, local: &dyn Fn(i64) -> Tm) -> String {
    let today = local(now).yday;
    let logtm = local(logt);
    let age = now.saturating_sub(logt);
    if age > 12 * 60 * 60 && logtm.yday != today {
        if age > 6 * 24 * 60 * 60 {
            format!(
                " {:02}{:>3}{:02}",
                logtm.day,
                tm_name(b"%b", &logtm),
                logtm.year.saturating_sub(1900) % 100
            )
        } else {
            format!(" {:>3}{:02}  ", tm_name(b"%a", &logtm), logtm.hour)
        }
    } else {
        format!(" {:02}:{:02}  ", logtm.hour, logtm.minute)
    }
}

/// C's `isprint` for a `char` in the C or a UTF-8 locale: printable ASCII.
/// A byte above `7F` is a negative `char`, and no table here calls it
/// printable.
fn c_isprint(b: u8) -> bool {
    (0x20..=0x7e).contains(&b)
}

/// `print_host`: the host, cut at `fromlen` and at the first unprintable byte
/// or space -- which shows as one `-` -- and padded to `fromlen`; a lone `-`
/// for an empty host.
fn print_host(host: &[u8], fromlen: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(fromlen);
    for &c in host.iter().take(UT_HOSTSIZE.min(fromlen)) {
        if c_isprint(c) && c != b' ' {
            out.push(c);
        } else {
            out.push(b'-');
            break;
        }
    }
    if out.is_empty() {
        out.push(b'-');
    }
    while out.len() < fromlen {
        out.push(b' ');
    }
    out
}

/// What `print_display_or_interface` prints from `host` after the address,
/// in `restlen` columns: the `:display` of an X login (`10.0.0.2:0`), or the
/// `%interface` of an IPv6 link address -- the host is IPv6, not a display,
/// once a second colon appears -- and then spaces to fill the field.
fn display_or_interface(host: &[u8], restlen: i64) -> Vec<u8> {
    let mut out = Vec::new();
    if restlen <= 0 {
        return out;
    }
    let mut restlen = restlen;
    // The field as C sees it: `UT_HOSTSIZE` bytes, NUL after the text.
    let at = |k: usize| host.get(k).copied().unwrap_or(0);
    let end = UT_HOSTSIZE;
    let field = i64::try_from(end).unwrap_or(i64::MAX);
    // Print from `from` while printable and not a space, at most
    // `field - from` bytes and `restlen` columns; a stop at anything but the
    // end of the text shows as `-`.
    let mut copy = |from: usize, restlen: &mut i64| {
        let mut len = field.saturating_sub(i64::try_from(from).unwrap_or(field));
        if len > *restlen {
            len = *restlen;
        }
        let mut k = from;
        while len > 0 && c_isprint(at(k)) && at(k) != b' ' {
            out.push(at(k));
            len = len.saturating_sub(1);
            *restlen = restlen.saturating_sub(1);
            k = k.saturating_add(1);
        }
        if len > 0 && at(k) != 0 {
            *restlen = restlen.saturating_sub(1);
            out.push(b'-');
        }
    };
    let mut disp = 0usize;
    while disp < end && at(disp) != b':' && c_isprint(at(disp)) {
        disp = disp.saturating_add(1);
    }
    if disp < end && at(disp) == b':' {
        let mut tmp = disp.saturating_add(1);
        while tmp < end && at(tmp) != b':' && c_isprint(at(tmp)) {
            tmp = tmp.saturating_add(1);
        }
        if tmp >= end || at(tmp) != b':' {
            copy(disp, &mut restlen);
        } else {
            while tmp < end && at(tmp) != b'%' && c_isprint(at(tmp)) {
                tmp = tmp.saturating_add(1);
            }
            if tmp < end && at(tmp) == b'%' {
                copy(tmp, &mut restlen);
            }
        }
    }
    while restlen > 0 {
        out.push(b' ');
        restlen = restlen.saturating_sub(1);
    }
    out
}

/// `print_from`: the FROM column for `rec`, `fromlen` wide.
///
/// With `-i`, the address in the record when there is one: an IPv4-mapped
/// IPv6 address as IPv4, an IPv6 one cut to the column, an IPv4 one only if
/// it fits whole -- then whatever display or interface the host names. With no
/// address, or without `-i`, the host. `ntop` is `inet_ntop` into a buffer of
/// the size given.
fn from_field(rec: &Record, ip_addresses: bool, fromlen: usize, ntop: &Ntop) -> Vec<u8> {
    if !ip_addresses {
        return print_host(&rec.host, fromlen);
    }
    // `ut_addr_v6` as it lies in the record: four words in network order.
    let mut addr = [0u8; 16];
    for (slot, word) in addr.as_chunks_mut::<4>().0.iter_mut().zip(rec.addr_v6) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    // `IN6_IS_ADDR_V4MAPPED`, mapped back to the IPv4 address.
    if addr[..10] == [0; 10] && addr[10..12] == [0xff, 0xff] {
        let v4 = [addr[12], addr[13], addr[14], addr[15]];
        addr = [0; 16];
        addr[..4].copy_from_slice(&v4);
    }
    // The address as text, or `None` -- there is no address, or the library
    // would not write it -- in which case upstream shows the host instead
    // (`strcpy (buf, "")`, then `print_host`). For IPv4 the refusal is the
    // expected one: `inet_ntop` answers `ENOSPC` for an address longer than
    // the column, and the column then shows the host.
    let text = if addr[4..].iter().any(|&b| b != 0) {
        ntop(AF_INET6, &addr, INET6_ADDRSTRLEN).map(|mut t| {
            // `strncpy (buf, buf_ipv6, fromlen)`: cut to the column.
            t.truncate(fromlen);
            t
        })
    } else if addr[..4] != [0; 4] {
        ntop(AF_INET, &addr[..4], fromlen.saturating_add(1))
    } else {
        None
    };
    match text.filter(|t| !t.is_empty()) {
        Some(mut out) => {
            let rest = i64::try_from(fromlen.saturating_sub(out.len())).unwrap_or(0);
            out.extend_from_slice(&display_or_interface(&rec.host, rest));
            out
        }
        None => print_host(&rec.host, fromlen),
    }
}

/// `showinfo`'s cleanup of `ut_line`: the name up to its first byte that is
/// not a letter, a digit or `/`.
fn clean_line(line: &[u8]) -> Vec<u8> {
    line.iter()
        .take_while(|&&c| c.is_ascii_alphanumeric() || c == b'/')
        .copied()
        .collect()
}

/// What `w` asks of a file: the access time, and the device it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Node {
    atime: i64,
    rdev: u64,
    is_char: bool,
}

/// C's conversion of a `dev_t` to `int`: the low 32 bits.
fn dev_int(rdev: u64) -> i32 {
    let b = rdev.to_le_bytes();
    i32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

/// `get_tty_device`: the device number of the terminal `name`, or -1.
///
/// An absolute name is taken as it is, whatever it is; otherwise the first of
/// `/dev/NAME`, `/dev/ttyNAME` and `/dev/pts/NAME` that is a character device
/// -- each spelled into upstream's 32-byte buffer, so a long name is cut.
fn tty_device(name: &[u8], stat: &dyn Fn(&[u8]) -> Option<Node>) -> i32 {
    if name.first() == Some(&b'/') {
        if let Some(node) = stat(name) {
            return dev_int(node.rdev);
        }
    }
    for prefix in [&b"/dev/"[..], b"/dev/tty", b"/dev/pts/"] {
        let mut path = prefix.to_vec();
        path.extend_from_slice(name);
        path.truncate(31);
        if let Some(node) = stat(&path).filter(|n| n.is_char) {
            return dev_int(node.rdev);
        }
    }
    -1
}

/// What `find_best_proc` found for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Best {
    /// Whether the login process exists; upstream prints no row if not.
    found: bool,
    /// JCPU, in ticks.
    jcpu: u64,
    /// PCPU, in ticks: WHAT's.
    pcpu: u64,
    /// WHAT: at most 512 bytes of a command line, `-` for none.
    cmdline: Vec<u8>,
    /// WHAT's process ID, for `-p`; -1 for none.
    pid: i32,
}

/// `find_best_proc`, given the process table: the login process `ut_pid`,
/// the session's terminal `line`, and the user's ID (`None` under `-u`).
///
/// In table order: the login process seeds the answer; each process on the
/// terminal adds to JCPU, and the newest of them so far replaces a `-`; and
/// one in the terminal's foreground group, belonging to the user, that started
/// later than the answer so far, replaces it.
fn find_best_proc(tasks: &[Task], ut_pid: i32, line: i32, uid: Option<u32>) -> Best {
    let mut best = Best {
        found: false,
        jcpu: 0,
        pcpu: 0,
        cmdline: b"-".to_vec(),
        pid: -1,
    };
    let take = |best: &mut Best, t: &Task| {
        best.cmdline = t.cmdline.iter().take(MAX_CMD_BYTES).copied().collect();
        best.pid = t.pid;
        best.pcpu = t.tics_all;
    };
    let mut best_time: u64 = 0;
    let mut secondbest_time: u64 = 0;
    for t in tasks {
        if t.pid == ut_pid {
            best.found = true;
            if best_time == 0 {
                best_time = t.start;
                take(&mut best, t);
            }
        }
        if t.tty != line {
            continue;
        }
        best.jcpu = best.jcpu.wrapping_add(t.tics_all);
        if !(secondbest_time != 0 && t.start <= secondbest_time) {
            secondbest_time = t.start;
            if best.cmdline == b"-" {
                take(&mut best, t);
            }
        }
        if uid.is_some_and(|u| u != t.euid && u != t.ruid)
            || t.pgrp != t.tpgid
            || t.start <= best_time
        {
            continue;
        }
        best_time = t.start;
        take(&mut best, t);
    }
    best
}

/// `/proc`, read the first time a row needs it and kept for the rest.
struct TaskTable<'a> {
    read: Option<Vec<Task>>,
    load: &'a dyn Fn() -> io::Result<Vec<Task>>,
}

impl TaskTable<'_> {
    /// The table.
    ///
    /// # Errors
    ///
    /// `/proc` could not be listed.
    fn get(&mut self) -> io::Result<&[Task]> {
        if self.read.is_none() {
            self.read = Some((self.load)()?);
        }
        Ok(self.read.as_deref().unwrap_or_default())
    }
}

/// Everything about the machine a row reads, so that a row can be printed
/// from a made-up one.
struct World<'a> {
    /// `time(NULL)`.
    now: i64,
    /// `localtime`.
    local: &'a dyn Fn(i64) -> Tm,
    /// `stat`, following links.
    stat: &'a dyn Fn(&[u8]) -> Option<Node>,
    /// `getpwnam(name)->pw_uid`.
    uid_of: &'a dyn Fn(&[u8]) -> Option<u32>,
    /// `inet_ntop` into a buffer of the size given.
    ntop: &'a Ntop,
    /// `procps_hertz_get`.
    hertz: i64,
}

/// `showinfo`: the row for `rec`, `None` when upstream prints none -- the
/// user is unknown (without `-u`) or the login process has gone.
///
/// # Errors
///
/// `/proc` could not be listed, which upstream reports and exits on.
fn showinfo(
    rec: &Record,
    opts: &Options,
    lay: &Layout,
    world: &World<'_>,
    table: &mut TaskTable<'_>,
) -> io::Result<Option<Vec<u8>>> {
    let line_name = clean_line(&rec.tty);
    let mut tty = b"/dev/".to_vec();
    tty.extend_from_slice(&line_name);

    let uid = if opts.ignoreuser {
        None
    } else {
        match (world.uid_of)(&rec.user) {
            Some(uid) => Some(uid),
            None => return Ok(None),
        }
    };
    let line = tty_device(&line_name, world.stat);
    let best = find_best_proc(table.get()?, rec.pid, line, uid);
    if !best.found {
        return Ok(None);
    }

    let mut row = Vec::new();
    let name = rec
        .user
        .get(..lay.userlen.min(rec.user.len()))
        .unwrap_or_default();
    row.extend_from_slice(name);
    row.resize(
        row.len()
            .saturating_add(lay.userlen.saturating_add(1).saturating_sub(name.len())),
        b' ',
    );
    let shown = line_name.get(..8.min(line_name.len())).unwrap_or_default();
    row.extend_from_slice(shown);
    row.resize(
        row.len().saturating_add(9usize.saturating_sub(shown.len())),
        b' ',
    );
    if opts.from {
        row.extend_from_slice(&from_field(rec, opts.ip_addresses, lay.fromlen, world.ntop));
    }
    if opts.longform {
        row.extend_from_slice(logintime(world.now, rec.tv_sec, world.local).as_bytes());
    }
    let idle = if rec.tty.first() == Some(&b':') {
        // Idle is unknown for an xdm login.
        " ?xdm? ".to_string()
    } else {
        let atime = (world.stat)(&tty).map_or(world.now, |n| n.atime);
        time_ival7(world.now.saturating_sub(atime), 0, opts.oldstyle)
    };
    row.extend_from_slice(idle.as_bytes());
    if opts.longform {
        row.extend_from_slice(ticks_ival7(best.jcpu, world.hertz, opts.oldstyle).as_bytes());
        if best.pcpu > 0 {
            row.extend_from_slice(ticks_ival7(best.pcpu, world.hertz, opts.oldstyle).as_bytes());
        } else {
            row.extend_from_slice(b"   ?   ");
        }
    }
    let mut maxcmd = lay.maxcmd;
    if opts.pids {
        let ids = format!(" {}/{}", rec.pid, best.pid);
        let len = i32::try_from(ids.len()).unwrap_or(i32::MAX);
        if len > maxcmd {
            maxcmd = 0;
        } else if len > 0 {
            maxcmd = maxcmd.saturating_sub(len);
        }
        row.extend_from_slice(ids.as_bytes());
    }
    row.push(b' ');
    let width = usize::try_from(maxcmd).unwrap_or(0);
    row.extend_from_slice(
        best.cmdline
            .get(..width.min(best.cmdline.len()))
            .unwrap_or_default(),
    );
    row.push(b'\n');
    Ok(Some(row))
}

/// The sessions upstream's loop hands to `showinfo`: `USER_PROCESS` entries,
/// either those whose name is `user` -- compared as `strncmp` over the
/// 32-byte field -- or, with no user given, every one with a name.
fn sessions<'r>(records: &'r [Record], user: Option<&[u8]>) -> impl Iterator<Item = &'r Record> {
    let user = user.map(|u| {
        u.get(..UT_NAMESIZE.min(u.len()))
            .unwrap_or_default()
            .to_vec()
    });
    records.iter().filter(move |r| {
        r.record_type == USER_PROCESS
            && match &user {
                Some(u) => r.user == *u,
                None => !r.user.is_empty(),
            }
    })
}

#[cfg(unix)]
mod imp {
    use super::{
        HELP, Layout, Node, Options, Request, TaskTable, UT_NAMESIZE, World, header, layout,
        parse_args, sessions, showinfo,
    };
    use coreutils::diag;
    use coreutils::errmsg::strerror;
    use coreutils::procps;
    use coreutils::quote::{os_bytes, os_from_bytes, quotef};
    use coreutils::stdfd::{self, Stream};
    use coreutils::utmp::{self, UTMP_FILE};
    use std::ffi::OsString;
    use std::io::Write;
    use std::os::unix::fs::{FileTypeExt, MetadataExt};
    use std::process::ExitCode;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A variable of the environment, as bytes; set-but-empty is still set.
    fn var(name: &str) -> Option<Vec<u8>> {
        std::env::var_os(name).map(|v| os_bytes(&v).into_owned())
    }

    fn stat(path: &[u8]) -> Option<Node> {
        let m = std::fs::metadata(os_from_bytes(path)).ok()?;
        Some(Node {
            atime: m.atime(),
            rdev: m.rdev(),
            is_char: m.file_type().is_char_device(),
        })
    }

    fn ntop(af: i32, src: &[u8], size: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; size];
        let len = libcall::netdb::inet_ntop(af, src, &mut buf).ok()?;
        buf.truncate(len);
        Some(buf)
    }

    /// Everything after the options: the warnings, utmp, the header, the rows.
    fn show(opts: &Options, out: &mut Stream) -> u8 {
        let cols = libcall::pty::window_size(1).ok().map(|w| w.cols);
        let (lay, warnings): (Layout, _) = layout(
            opts,
            var("PROCPS_USERLEN").as_deref(),
            var("PROCPS_FROMLEN").as_deref(),
            cols,
            var("COLUMNS").as_deref(),
        );
        for w in warnings {
            diag!("w: {w}");
        }

        let records = match utmp::read_bytes(UTMP_FILE.as_bytes(), false) {
            Ok(data) => utmpfile::parse(&data),
            Err(e) => {
                diag!("w: {}: {}", quotef(UTMP_FILE.as_bytes()), strerror(&e));
                return 1;
            }
        };

        let zone = localtime::Zone::from_env();
        let local = |t: i64| zone.local(t, 0);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok())
            .unwrap_or(0);

        // Each write is deliberately unread: a failed write is `Stream`'s to
        // remember and `close_stdout`'s to report, once.
        if opts.header {
            let status = std::fs::read(procps::UPTIME_FILE)
                .ok()
                .and_then(|text| procps::uptime_secs(&text))
                .map_or_else(String::new, |up| {
                    let load = std::fs::read(procps::LOADAVG_FILE)
                        .map_or((0.0, 0.0, 0.0), |text| procps::load_averages(&text));
                    procps::status_line(&local(now), up, procps::count_users(&records), load)
                });
            let _ = out.write_all(&header(opts, &lay, &status));
        }

        let utf8 = coreutils::locale::ctype_is_utf8();
        let load = || procps::tasks(&procinfo::ProcFs::new(), utf8);
        let mut table = TaskTable {
            read: None,
            load: &load,
        };
        let db = pwdb::Db::load();
        let uid_of = |name: &[u8]| {
            let name = name.get(..UT_NAMESIZE.min(name.len())).unwrap_or_default();
            db.user_by_name(name).map(|u| u.uid)
        };
        let world = World {
            now,
            local: &local,
            stat: &stat,
            uid_of: &uid_of,
            ntop: &ntop,
            hertz: procps::hertz(),
        };
        for rec in sessions(&records, opts.user.as_deref()) {
            match showinfo(rec, opts, &lay, &world, &mut table) {
                Ok(Some(row)) => {
                    let _ = out.write_all(&row);
                }
                Ok(None) => {}
                Err(_) => {
                    diag!("w: Unable to load process information");
                    return 1;
                }
            }
        }
        0
    }

    pub fn main() -> ExitCode {
        stdfd::restore();
        let args: Vec<OsString> = std::env::args_os().skip(1).collect();
        let mut out = Stream::stdout();
        let status = match parse_args(&args) {
            Ok(Request::Help) => {
                let _ = out.write_all(HELP.as_bytes());
                0
            }
            Ok(Request::Version) => {
                let _ = out.write_all(b"w from SlateOS coreutils 0.1.0\n");
                0
            }
            Ok(Request::Show(opts)) => show(&opts, &mut out),
            Err(sentence) => {
                let mut err = Stream::stderr();
                let _ = err.write_all(format!("w: {sentence}\n").as_bytes());
                let _ = err.write_all(HELP.as_bytes());
                1
            }
        };
        stdfd::close_stdout("w", out, ExitCode::from(status))
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    coreutils::stdfd::close_stderr(imp::main(), 1)
}

/// The host build exists only so `cargo test` runs on the developer machine,
/// which has no utmp and no `/proc`.
#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    coreutils::diag!("w: unix-only utility; not supported on this platform");
    std::process::ExitCode::from(1)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use coreutils::utmp::{DEAD_PROCESS, LOGIN_PROCESS};
    use localtime::Zone;

    fn argv(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    fn opts(args: &[&str]) -> Options {
        match parse_args(&argv(args)).unwrap() {
            Request::Show(o) => o,
            other => panic!("{args:?} gave {other:?}"),
        }
    }

    #[test]
    fn the_switches() {
        let d = opts(&[]);
        assert!(d.header && d.longform && d.from && !d.ip_addresses && !d.pids);
        assert!(!opts(&["-h"]).header);
        assert!(!opts(&["--short"]).longform);
        // `-f` toggles; `-i` sets FROM whatever `-f` did.
        assert!(!opts(&["-f"]).from);
        assert!(opts(&["-ff"]).from);
        let i = opts(&["-f", "-i"]);
        assert!(i.from && i.ip_addresses);
        assert!(!opts(&["-i", "-f"]).from);
        assert!(opts(&["-u"]).ignoreuser);
        assert!(opts(&["--old"]).oldstyle);
        assert!(opts(&["--pid"]).pids);
    }

    #[test]
    fn the_first_operand_is_the_user_and_the_rest_are_ignored() {
        assert_eq!(opts(&["alice", "bob"]).user.as_deref(), Some(&b"alice"[..]));
        assert_eq!(opts(&["--", "-h"]).user.as_deref(), Some(&b"-h"[..]));
        // Permuted, as GNU's getopt does.
        let o = opts(&["alice", "-s"]);
        assert!(!o.longform);
        assert_eq!(o.user.as_deref(), Some(&b"alice"[..]));
    }

    #[test]
    fn version_and_help_act_where_they_are_met() {
        assert_eq!(parse_args(&argv(&["-V", "-x"])).unwrap(), Request::Version);
        assert_eq!(parse_args(&argv(&["--help", "-x"])).unwrap(), Request::Help);
        assert_eq!(
            parse_args(&argv(&["-x", "--help"])).unwrap_err(),
            "invalid option -- 'x'"
        );
        assert_eq!(
            parse_args(&argv(&["--no"])).unwrap_err(),
            "option '--no' is ambiguous; possibilities: '--no-header' '--no-current'"
        );
        assert_eq!(
            parse_args(&argv(&["--pids=1"])).unwrap_err(),
            "option '--pids' doesn't allow an argument"
        );
    }

    #[test]
    fn the_help_is_upstreams() {
        assert!(HELP.starts_with("\nUsage:\n w [options] [user]\n\nOptions:\n"));
        assert!(HELP.ends_with("\nFor more details see w(1).\n"));
        // Measured: `w --help | wc -c` of procps-ng 4.0.4 is 508.
        assert_eq!(HELP.len(), 508);
    }

    #[test]
    fn atoi_is_strtol_then_int() {
        assert_eq!(c_atoi(b"12"), 12);
        assert_eq!(c_atoi(b"  +9x"), 9);
        assert_eq!(c_atoi(b"-3"), -3);
        assert_eq!(c_atoi(b""), 0);
        assert_eq!(c_atoi(b"abc"), 0);
        assert_eq!(c_atoi(b"4294967304"), 8);
        assert_eq!(c_atoi(b"99999999999999999999"), -1);
        assert_eq!(c_atoi(b"-99999999999999999999"), 0);
        assert_eq!(c_atoi(b"-9223372036854775808"), 0);
    }

    #[test]
    fn the_layout() {
        let d = Options::default();
        // 512 less 21 + 8 + 16 + 20.
        let (lay, w) = layout(&d, None, None, None, None);
        assert_eq!(
            lay,
            Layout {
                userlen: 8,
                fromlen: 16,
                maxcmd: 447
            }
        );
        assert!(w.is_empty());
        // The terminal first, then COLUMNS; zero columns is no answer.
        assert_eq!(layout(&d, None, None, Some(80), Some(b"200")).0.maxcmd, 15);
        assert_eq!(layout(&d, None, None, Some(0), Some(b"200")).0.maxcmd, 135);
        assert_eq!(layout(&d, None, None, None, Some(b"")).0.maxcmd, 7);
        let short = Options {
            longform: false,
            from: false,
            ..Options::default()
        };
        assert_eq!(layout(&short, None, None, Some(80), None).0.maxcmd, 51);
        // The variables, and their refusals.
        let (lay, w) = layout(&d, Some(b"20"), Some(b"30"), Some(200), None);
        assert_eq!((lay.userlen, lay.fromlen, lay.maxcmd), (20, 30, 109));
        assert!(w.is_empty());
        let (lay, w) = layout(&d, Some(b"7"), Some(b"257"), None, None);
        assert_eq!((lay.userlen, lay.fromlen), (8, 16));
        assert_eq!(
            w,
            [
                "User length environment PROCPS_USERLEN must be between 8 and 32, ignoring.\n",
                "from length environment PROCPS_FROMLEN must be between 8 and 256, ignoring\n",
            ]
        );
    }

    #[test]
    fn the_header() {
        let d = Options::default();
        let (lay, _) = layout(&d, None, None, None, None);
        assert_eq!(
            String::from_utf8(header(&d, &lay, " 10:00:00 up 1 min")).unwrap(),
            " 10:00:00 up 1 min\nUSER     TTY      FROM             LOGIN@   IDLE   JCPU   PCPU WHAT\n"
        );
        let s = Options {
            longform: false,
            from: false,
            ..Options::default()
        };
        assert_eq!(
            String::from_utf8(header(&s, &lay, "")).unwrap(),
            "\nUSER     TTY         IDLE WHAT\n"
        );
    }

    #[test]
    fn intervals_in_both_styles() {
        let new = |t| time_ival7(t, 0, false);
        let old = |t| time_ival7(t, 0, true);
        assert_eq!(new(-1), "   ?   ");
        assert_eq!(new(5), "  5.00s");
        assert_eq!(new(60), " 60.00s");
        assert_eq!(new(61), "  1:01 ");
        assert_eq!(new(3599), " 59:59 ");
        assert_eq!(new(3600), "  1:00m");
        assert_eq!(new(48 * 3600 - 1), " 47:59m");
        assert_eq!(new(48 * 3600), "  2days");
        assert_eq!(new(400 * 86400), " 400days");
        assert_eq!(old(60), "       ");
        assert_eq!(old(61), "  1:01m");
        assert_eq!(old(3600), "  1:00 ");
        assert_eq!(old(48 * 3600), "  2days");
        assert_eq!(time_ival7(3, 7, false), "  3.07s");
    }

    #[test]
    fn ticks_become_seconds_and_hundredths() {
        assert_eq!(ticks_ival7(0, 100, false), "  0.00s");
        assert_eq!(ticks_ival7(1234, 100, false), " 12.34s");
        assert_eq!(ticks_ival7(250 * 61 + 3, 250, false), "  1:01 ");
        // 3 ticks of 250 are 1.2 hundredths: truncated.
        assert_eq!(ticks_ival7(3, 250, false), "  0.01s");
    }

    fn utc(t: i64) -> Tm {
        Zone::utc().local(t, 0)
    }

    #[test]
    fn login_times_by_age() {
        // 2023-11-14 22:13:20 UTC, a Tuesday.
        let now = 1_700_000_000;
        let at = |logt| logintime(now, logt, &utc);
        assert_eq!(at(now - 60), " 22:12  ");
        // Earlier the same day: a clock even when more than twelve hours ago.
        assert_eq!(at(now - 13 * 3600), " 09:13  ");
        // Yesterday, but less than twelve hours ago: still a clock. At 05:00
        // on the 14th, 23:00 on the 13th was six hours ago.
        let early = 1_699_920_000 + 5 * 3600;
        assert_eq!(logintime(early, 1_699_920_000 - 3600, &utc), " 23:00  ");
        // Yesterday and more than twelve hours ago: weekday and hour.
        assert_eq!(
            logintime(early, 1_699_920_000 - 20 * 3600, &utc),
            " Mon04  "
        );
        // Within six days: weekday and hour.
        assert_eq!(at(now - 2 * 86400), " Sun22  ");
        // Before that: day, month, year.
        assert_eq!(at(now - 30 * 86400), " 15Oct23");
        assert_eq!(at(0), " 01Jan70");
    }

    #[test]
    fn the_host_column() {
        let show = |h: &[u8], n| String::from_utf8(print_host(h, n)).unwrap();
        assert_eq!(show(b"10.0.0.2", 16), "10.0.0.2        ");
        assert_eq!(show(b"", 16), "-               ");
        assert_eq!(show(b"bad host", 16), "bad-            ");
        assert_eq!(show(b"tab\there", 8), "tab-    ");
        assert_eq!(show(b"caf\xc3\xa9", 8), "caf-    ");
        assert_eq!(show(b"a-very-long-host-name", 8), "a-very-l");
    }

    #[test]
    fn displays_and_interfaces_after_an_address() {
        let show = |h: &[u8], n| String::from_utf8(display_or_interface(h, n)).unwrap();
        assert_eq!(show(b"10.0.0.2:0", 8), ":0      ");
        assert_eq!(show(b"host:0.0", 4), ":0.0");
        assert_eq!(show(b"host:12345", 3), ":12");
        assert_eq!(show(b"host:0 x", 8), ":0-     ");
        assert_eq!(show(b"plainhost", 5), "     ");
        assert_eq!(show(b"fe80::1%eth0", 8), "%eth0   ");
        assert_eq!(show(b"fe80::1", 4), "    ");
        assert_eq!(show(b"anything", 0), "");
        assert_eq!(show(b"anything", -3), "");
    }

    fn rec(user: &[u8], tty: &[u8], host: &[u8], pid: i32) -> Record {
        Record {
            record_type: USER_PROCESS,
            user: user.to_vec(),
            tty: tty.to_vec(),
            host: host.to_vec(),
            id: Vec::new(),
            pid,
            login_time: 0,
            tv_sec: 1_700_000_000 - 60,
            login_usec: 0,
            exit_status: 0,
            session: 0,
            addr_v6: [0; 4],
        }
    }

    /// `inet_ntop`'s IPv4 half, enough for the tests: the library's own is
    /// what runs.
    fn fake_ntop(af: i32, src: &[u8], size: usize) -> Option<Vec<u8>> {
        let text = match af {
            AF_INET => format!("{}.{}.{}.{}", src[0], src[1], src[2], src[3]).into_bytes(),
            _ => b"2001:db8::1".to_vec(),
        };
        (text.len() < size).then_some(text)
    }

    #[test]
    fn from_with_and_without_addresses() {
        let mut r = rec(b"alice", b"pts/3", b"example.org:0", 1);
        let show = |r: &Record, ip, n| String::from_utf8(from_field(r, ip, n, &fake_ntop)).unwrap();
        assert_eq!(show(&r, false, 16), "example.org:0   ");
        // No address: the host, even with -i.
        assert_eq!(show(&r, true, 16), "example.org:0   ");
        r.addr_v6 = [u32::from_le_bytes([10, 0, 0, 2]), 0, 0, 0];
        assert_eq!(show(&r, true, 16), "10.0.0.2:0      ");
        // An IPv4 address that does not fit whole gives way to the host.
        r.addr_v6 = [u32::from_le_bytes([192, 168, 100, 200]), 0, 0, 0];
        assert_eq!(show(&r, true, 8), "example.");
        // IPv4-mapped IPv6 is IPv4.
        r.addr_v6 = [
            0,
            0,
            u32::from_le_bytes([0, 0, 0xff, 0xff]),
            u32::from_le_bytes([10, 0, 0, 3]),
        ];
        assert_eq!(show(&r, true, 16), "10.0.0.3:0      ");
        // IPv6 is cut to the column.
        r.addr_v6 = [1, 0, 0, 1];
        assert_eq!(show(&r, true, 8), "2001:db8");
    }

    #[test]
    fn the_line_is_cleaned() {
        assert_eq!(clean_line(b"pts/3"), b"pts/3");
        assert_eq!(clean_line(b"tty-1"), b"tty");
        assert_eq!(clean_line(b":0"), b"");
        assert_eq!(clean_line(b"/dev/pts/1"), b"/dev/pts/1");
        assert_eq!(clean_line(b"t\xe9ty"), b"t");
    }

    #[test]
    fn the_terminal_device() {
        let stat = |p: &[u8]| -> Option<Node> {
            let char_dev = |rdev| {
                Some(Node {
                    atime: 0,
                    rdev,
                    is_char: true,
                })
            };
            match p {
                b"/dev/pts/3" => char_dev(34819),
                b"/dev/tty1" => char_dev(1025),
                b"/dev/" => Some(Node {
                    atime: 0,
                    rdev: 0,
                    is_char: false,
                }),
                b"/dev/tty" => char_dev(1280),
                b"/elsewhere" => Some(Node {
                    atime: 0,
                    rdev: 77,
                    is_char: false,
                }),
                _ => None,
            }
        };
        assert_eq!(tty_device(b"pts/3", &stat), 34819);
        assert_eq!(tty_device(b"3", &stat), 34819);
        assert_eq!(tty_device(b"1", &stat), 1025);
        assert_eq!(tty_device(b"/elsewhere", &stat), 77);
        // `/dev/` is a directory; `/dev/tty` is next.
        assert_eq!(tty_device(b"", &stat), 1280);
        assert_eq!(tty_device(b"nosuch", &stat), -1);
        // The 32-byte buffer cuts a long name before it is looked up.
        let seen = std::cell::RefCell::new(Vec::new());
        let record = |p: &[u8]| -> Option<Node> {
            seen.borrow_mut().push(p.to_vec());
            None
        };
        tty_device(b"0123456789012345678901234567890", &record);
        assert!(seen.borrow().iter().all(|p| p.len() <= 31));
    }

    fn task(pid: i32, start: u64, tty: i32, pgrp: i32, tpgid: i32, cmd: &[u8]) -> Task {
        Task {
            pid,
            start,
            euid: 1000,
            ruid: 1000,
            tpgid,
            pgrp,
            tty,
            tics_all: u64::try_from(pid).unwrap(),
            cmdline: cmd.to_vec(),
        }
    }

    #[test]
    fn the_best_process_is_the_newest_foreground_one_of_the_user() {
        let tasks = [
            task(10, 100, 7, 10, 30, b"-bash"),
            task(20, 200, 7, 20, 30, b"make"),
            task(30, 300, 7, 30, 30, b"vim notes"),
            task(40, 400, 9, 40, 40, b"elsewhere"),
        ];
        let best = find_best_proc(&tasks, 10, 7, Some(1000));
        assert!(best.found);
        assert_eq!(best.cmdline, b"vim notes");
        assert_eq!(best.pid, 30);
        assert_eq!(best.pcpu, 30);
        assert_eq!(best.jcpu, 10 + 20 + 30);
        // Someone else's process is not WHAT unless -u.
        let mut theirs = tasks.clone();
        theirs[2].euid = 0;
        theirs[2].ruid = 0;
        assert_eq!(find_best_proc(&theirs, 10, 7, Some(1000)).cmdline, b"-bash");
        assert_eq!(find_best_proc(&theirs, 10, 7, None).cmdline, b"vim notes");
        // The real ID is enough.
        theirs[2].ruid = 1000;
        assert_eq!(
            find_best_proc(&theirs, 10, 7, Some(1000)).cmdline,
            b"vim notes"
        );
    }

    #[test]
    fn no_login_process_is_no_row() {
        let tasks = [task(20, 200, 7, 20, 99, b"make")];
        assert!(!find_best_proc(&tasks, 10, 7, Some(1000)).found);
    }

    /// The fallback to the terminal's newest process: it can stand in only
    /// while WHAT reads `-`, which the login process's own command line
    /// replaces as soon as it is seen -- unless that command line *is* `-`.
    #[test]
    fn the_terminal_stands_in_only_for_a_dash() {
        // Background processes only, and a login process named normally:
        // the login process is WHAT, and the terminal adds only to JCPU.
        let tasks = [
            task(10, 100, 5, 10, 99, b"login"),
            task(20, 200, 7, 20, 99, b"make"),
            task(21, 150, 7, 21, 99, b"older"),
        ];
        let best = find_best_proc(&tasks, 10, 7, Some(1000));
        assert_eq!(best.cmdline, b"login");
        assert_eq!(best.jcpu, 20 + 21);
        // Seen before the login process, the terminal's newest is replaced by it.
        let tasks = [
            task(5, 50, 7, 5, 99, b"early"),
            task(10, 100, 5, 10, 99, b"login"),
        ];
        assert_eq!(find_best_proc(&tasks, 10, 7, Some(1000)).cmdline, b"login");
        // A login process whose command line is `-` leaves the dash in place,
        // and the newest process on the terminal takes it.
        let tasks = [
            task(10, 100, 5, 10, 99, b"-"),
            task(20, 200, 7, 20, 99, b"make"),
            task(21, 150, 7, 21, 99, b"older"),
        ];
        let best = find_best_proc(&tasks, 10, 7, Some(1000));
        assert_eq!((best.cmdline.as_slice(), best.pid), (&b"make"[..], 20));
        // A login process that began at tick zero seeds WHAT but not the
        // time, so any foreground process of the user beats it.
        let tasks = [
            task(10, 0, 5, 10, 99, b"init"),
            task(20, 1, 7, 20, 20, b"sh"),
        ];
        assert_eq!(find_best_proc(&tasks, 10, 7, Some(1000)).cmdline, b"sh");
    }

    #[test]
    fn what_is_cut_at_512_bytes() {
        let long = vec![b'x'; 600];
        let best = find_best_proc(&[task(10, 100, 7, 10, 10, &long)], 10, 7, None);
        assert_eq!(best.cmdline.len(), 512);
    }

    #[test]
    fn the_sessions_shown() {
        let mut dead = rec(b"carol", b"pts/9", b"", 3);
        dead.record_type = DEAD_PROCESS;
        let mut login = rec(b"LOGIN", b"tty2", b"", 4);
        login.record_type = LOGIN_PROCESS;
        let records = [
            rec(b"alice", b"pts/1", b"", 1),
            rec(b"", b"pts/2", b"", 2),
            dead,
            login,
            rec(b"bob", b"pts/4", b"", 5),
            rec(b"alice", b"pts/5", b"", 6),
        ];
        let pids =
            |user: Option<&[u8]>| sessions(&records, user).map(|r| r.pid).collect::<Vec<_>>();
        assert_eq!(pids(None), [1, 5, 6]);
        assert_eq!(pids(Some(b"alice")), [1, 6]);
        // An empty USER matches the nameless entry, as `strncmp` does.
        assert_eq!(pids(Some(b"")), [2]);
        assert_eq!(pids(Some(b"al")), Vec::<i32>::new());
    }

    #[test]
    fn a_name_is_compared_over_its_32_byte_field() {
        let long = [b'n'; 32];
        let records = [rec(&long, b"pts/1", b"", 1)];
        let mut asked = long.to_vec();
        asked.extend_from_slice(b"-and-more");
        assert_eq!(sessions(&records, Some(&asked)).count(), 1);
    }

    /// A whole row, from a made-up machine.
    #[test]
    fn a_row() {
        let now = 1_700_000_000;
        let stat = |p: &[u8]| -> Option<Node> {
            (p == b"/dev/pts/3").then_some(Node {
                atime: now - 3700,
                rdev: 34819,
                is_char: true,
            })
        };
        let uid_of = |n: &[u8]| (n == b"alice").then_some(1000);
        let world = World {
            now,
            local: &utc,
            stat: &stat,
            uid_of: &uid_of,
            ntop: &fake_ntop,
            hertz: 100,
        };
        let load = || {
            Ok(vec![
                task(10, 100, 34819, 10, 30, b"-bash"),
                task(30, 300, 34819, 30, 30, b"vim notes"),
            ])
        };
        let mut table = TaskTable {
            read: None,
            load: &load,
        };
        let d = Options::default();
        let (lay, _) = layout(&d, None, None, Some(80), None);
        let row = |r: &Record, o: &Options, t: &mut TaskTable<'_>| {
            showinfo(r, o, &lay, &world, t)
                .unwrap()
                .map(|b| String::from_utf8(b).unwrap())
        };
        let alice = rec(b"alice", b"pts/3", b"10.0.0.2", 10);
        assert_eq!(
            row(&alice, &d, &mut table).as_deref(),
            Some("alice    pts/3    10.0.0.2         22:12    1:01m  0.40s  0.30s vim notes\n")
        );
        // -p takes its share of WHAT; 80 columns leave 15, the IDs 6.
        let p = Options {
            pids: true,
            ..Options::default()
        };
        assert_eq!(
            row(&alice, &p, &mut table).as_deref(),
            Some(
                "alice    pts/3    10.0.0.2         22:12    1:01m  0.40s  0.30s 10/30 vim notes\n"
            )
        );
        // An unknown user has no row, unless -u.
        let mallory = rec(b"mallory", b"pts/3", b"", 10);
        assert_eq!(row(&mallory, &d, &mut table), None);
        let u = Options {
            ignoreuser: true,
            ..Options::default()
        };
        assert!(row(&mallory, &u, &mut table).is_some());
        // A session whose login process has gone has none either.
        let stale = rec(b"alice", b"pts/3", b"", 99);
        assert_eq!(row(&stale, &d, &mut table), None);
        // An xdm login's idle time is unknown.
        let xdm = rec(b"alice", b":0", b"", 10);
        let line = row(&xdm, &d, &mut table).unwrap();
        assert!(line.contains(" ?xdm? "), "{line}");
    }

    #[test]
    fn a_table_that_cannot_be_read_is_an_error_only_when_a_row_needs_it() {
        let failed = || Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let mut table = TaskTable {
            read: None,
            load: &failed,
        };
        let stat = |_: &[u8]| None;
        let nobody = |_: &[u8]| None;
        let world = World {
            now: 0,
            local: &utc,
            stat: &stat,
            uid_of: &nobody,
            ntop: &fake_ntop,
            hertz: 100,
        };
        let d = Options::default();
        let (lay, _) = layout(&d, None, None, None, None);
        // An unknown user is decided before the table is read.
        assert_eq!(
            showinfo(&rec(b"x", b"pts/1", b"", 1), &d, &lay, &world, &mut table).unwrap(),
            None
        );
        let u = Options {
            ignoreuser: true,
            ..Options::default()
        };
        assert!(showinfo(&rec(b"x", b"pts/1", b"", 1), &u, &lay, &world, &mut table).is_err());
    }
}
