//! procps-ng 4.0.4's library: the pieces `uptime`, `w` and `ps` share.
//!
//! procps' programs are thin over `libproc2`. The line `uptime` prints is the
//! first line `w` prints, and both come from one function in
//! `library/uptime.c`; the process table `w` searches comes from
//! `library/readproc.c` by way of `library/pids.c`, which is where `ps` gets
//! its columns too. This module is those library functions, transcribed, so
//! that programs here which read the same file read it with one function --
//! as the programs there do. `w` and `ps` read `/proc/<pid>` through
//! [`readproc`] alone.
//!
//! | here | procps-ng 4.0.4 |
//! |---|---|
//! | [`scan_doubles`] | glibc's `fscanf` `%lf`, which is how the library reads `/proc/uptime` and `/proc/loadavg` |
//! | [`uptime_secs`], [`load_averages`] | `procps_uptime`, `procps_loadavg` |
//! | [`status_line`] | `procps_uptime_sprint` |
//! | [`count_users`] | `count_users` |
//! | [`hertz`] | `procps_hertz_get` |
//! | [`escape_str`], [`escape_command`] | `library/escape.c` |
//! | [`Task`], [`tasks`] | `procps_pids_reap(…, PIDS_FETCH_TASKS_ONLY)` for the items `w` asks for |
//! | [`readproc`] | `library/readproc.c`: `stat2proc`, `status2proc` and the rest, and the walks over `/proc` |
//! | [`scanf`] | glibc's `strtol`, `strtoul`, `atoi` and `sscanf`, as `readproc.c` applies them |
//! | [`devname`] | `library/devname.c`: a terminal's number to its name |
//! | [`pwcache`] | `library/pwcache.c`: user and group names by number |
//! | [`sysinfo`] | `procps_pid_length`, `btime` from `procps_stat_new`, `MemTotal`, `procps_uptime`, `lookup_wchan` |
//!
//! # Why `fscanf`, and not `str::parse`
//!
//! Because the two disagree about files that are not quite numbers, and the
//! status line is printed from whatever the scan made of them. `fscanf` takes
//! the longest prefix that could still become a number -- `1e` is consumed
//! whole and converts as `1` -- and leading whitespace, a sign, `inf` and
//! `nan` and hexadecimal all parse; `1.5x` is 1.5 followed by a failed second
//! conversion, where `parse` refuses the whole word. A load average that stops
//! after its first field still prints that field, with zeros after it.

use std::io;

use localtime::Tm;

pub mod devname;
pub mod pwcache;
pub mod readproc;
pub mod scanf;
pub mod sysinfo;
use procinfo::ProcFs;

use crate::extfloat::{self, ExtF80, Spec};
use crate::utmp::{Record, is_user_process};

/// `UPTIME_FILE`: seconds since boot, then seconds idle.
pub const UPTIME_FILE: &str = "/proc/uptime";

/// `LOADAVG_FILE`: the one-, five- and fifteen-minute load averages first.
pub const LOADAVG_FILE: &str = "/proc/loadavg";

/// `MAX_BUFSZ`: the size of `readproc`'s buffers for a command line and its
/// escaped form -- `1024*64*2`, one of which is kept for the NUL.
pub const MAX_BUFSZ: usize = 1024 * 64 * 2;

/// `sysconf`'s name for the clock tick rate: 2 on Linux and on SlateOS.
const SC_CLK_TCK: i32 = 2;

// ---------------------------------------------------------------------------
// Numbers from /proc
// ---------------------------------------------------------------------------

/// C's `isspace` for the bytes it can be asked about here: the six ASCII
/// spaces, in every locale this tree has.
fn c_isspace(b: u8) -> bool {
    matches!(b, b' ' | 0x09..=0x0d)
}

/// One glibc `%lf` conversion at the start of `text`: the value, and how many
/// bytes the conversion consumed -- whitespace included -- or `None` for a
/// matching or input failure.
///
/// glibc's `vfscanf` does not hand `strtod` the input. It first collects the
/// longest run that could still be part of a number, one character of
/// look-ahead at a time and pushing back only the last, and converts *that*:
/// so `1e5` is consumed whole, but so is `1e` -- converting as `1`, with the
/// `e` gone from the stream -- and `0x` is a failure where `strtod` would
/// have read `0`. A conversion that collected only a sign, or only `0x`, has
/// failed; so has one whose collected text `strtod` cannot read at all (`.`).
fn scan_lf(text: &[u8]) -> Option<(f64, usize)> {
    let at = |k: usize| text.get(k).copied();
    // Every numeric conversion skips leading whitespace.
    let mut i = text.iter().take_while(|&&b| c_isspace(b)).count();
    let mut buf: Vec<u8> = Vec::new();

    // The character under consideration, already counted in `i`.
    let mut c = at(i)?;
    i = i.saturating_add(1);
    let mut got_sign = 0;
    if c == b'-' || c == b'+' {
        got_sign = 1;
        buf.push(c);
        c = at(i)?;
        i = i.saturating_add(1);
    }

    // `nan`, exactly three letters: glibc's scanf does not read `nan(…)`.
    if c.eq_ignore_ascii_case(&b'n') {
        buf.push(c);
        for want in *b"an" {
            let x = at(i)?;
            i = i.saturating_add(1);
            if !x.eq_ignore_ascii_case(&want) {
                return None;
            }
            buf.push(x);
        }
        return convert(&buf, i);
    }
    // `inf`, or all of `infinity` once its fourth letter is seen.
    if c.eq_ignore_ascii_case(&b'i') {
        buf.push(c);
        for want in *b"nf" {
            let x = at(i)?;
            i = i.saturating_add(1);
            if !x.eq_ignore_ascii_case(&want) {
                return None;
            }
            buf.push(x);
        }
        if at(i).is_some_and(|x| x.eq_ignore_ascii_case(&b'i')) {
            for want in *b"inity" {
                let x = at(i)?;
                i = i.saturating_add(1);
                if !x.eq_ignore_ascii_case(&want) {
                    return None;
                }
                buf.push(x);
            }
        }
        return convert(&buf, i);
    }

    let mut exp_char = b'e';
    let mut hexa = false;
    let mut got_digit = false;
    let mut got_dot = false;
    let mut got_e = false;
    // `None` is EOF, which ends the collection like any other non-member.
    let mut cur = Some(c);
    if c == b'0' {
        buf.push(c);
        cur = at(i);
        if let Some(x) = cur {
            i = i.saturating_add(1);
            if x.eq_ignore_ascii_case(&b'x') {
                buf.push(x);
                hexa = true;
                exp_char = b'p';
                cur = at(i);
                if cur.is_some() {
                    i = i.saturating_add(1);
                }
            } else {
                got_digit = true;
            }
        }
    }

    while let Some(ch) = cur {
        if ch.is_ascii_digit() || (!got_e && hexa && ch.is_ascii_hexdigit()) {
            buf.push(ch);
            got_digit = true;
        } else if got_e && buf.last() == Some(&exp_char) && (ch == b'-' || ch == b'+') {
            buf.push(ch);
        } else if got_digit && !got_e && ch.to_ascii_lowercase() == exp_char {
            buf.push(exp_char);
            got_e = true;
            got_dot = true;
        } else if !got_dot && ch == b'.' {
            buf.push(ch);
            got_dot = true;
        } else {
            // Pushed back: not part of the number, and not consumed.
            i = i.saturating_sub(1);
            break;
        }
        cur = at(i);
        if cur.is_some() {
            i = i.saturating_add(1);
        }
    }

    if buf.len() == got_sign || (hexa && buf.len() == got_sign.saturating_add(2)) {
        return None;
    }
    convert(&buf, i)
}

/// `strtod` over what [`scan_lf`] collected: a failure if it reads nothing.
fn convert(collected: &[u8], consumed: usize) -> Option<(f64, usize)> {
    let scanned = extfloat::strtod(collected);
    (scanned.consumed > 0).then_some((scanned.value, consumed))
}

/// glibc's `fscanf(fp, "%lf %lf …")` over `text`, for at most `want`
/// conversions: the values of the ones that succeeded, in order, up to the
/// first that did not.
#[must_use]
pub fn scan_doubles(text: &[u8], want: usize) -> Vec<f64> {
    let mut values = Vec::with_capacity(want);
    let mut at = 0usize;
    while values.len() < want {
        let Some(rest) = text.get(at..) else { break };
        let Some((value, used)) = scan_lf(rest) else {
            break;
        };
        values.push(value);
        at = at.saturating_add(used);
    }
    values
}

/// `procps_uptime`: the seconds since boot from `/proc/uptime`'s text, or
/// `None` -- upstream's `-ERANGE` -- unless *both* of its numbers are there.
#[must_use]
pub fn uptime_secs(text: &[u8]) -> Option<f64> {
    match scan_doubles(text, 2).as_slice() {
        [up, _idle] => Some(*up),
        _ => None,
    }
}

/// `procps_loadavg`'s three figures from `/proc/loadavg`'s text.
///
/// Upstream starts all three at zero and lets `fscanf` overwrite as many as it
/// can parse, then hands back all three whatever the count -- so a file that
/// stops after one number prints that number and two zeros, and one that is
/// not numbers at all prints three zeros. (A file that cannot be *opened*
/// leaves upstream's caller printing uninitialised stack; that case is the
/// caller's to decide, and both callers here print zeros.)
#[must_use]
pub fn load_averages(text: &[u8]) -> (f64, f64, f64) {
    let values = scan_doubles(text, 3);
    let nth = |n: usize| values.get(n).copied().unwrap_or(0.0);
    (nth(0), nth(1), nth(2))
}

/// C's `(int) x` for a double, as x86-64 performs it: truncation toward zero
/// in range, and for NaN or a value outside `int`'s range, `cvttsd2si`'s
/// "integer indefinite", `INT_MIN`. The conversion is undefined in C there;
/// this is what the reference binary, compiled for the one architecture this
/// tree targets, does with a `/proc/uptime` of a hundred years.
// The `as` is reached only in range, checked first: there it truncates toward
// zero, which is C's conversion, and it cannot saturate.
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn c_int(x: f64) -> i32 {
    if x.is_nan() || x >= 2_147_483_648.0 || x <= -2_147_483_649.0 {
        i32::MIN
    } else {
        x as i32
    }
}

/// `%.2f`, glibc's: rounded from the exact binary value, `nan`/`-nan`/`inf`
/// spelled as C spells them.
fn fixed2(x: f64) -> String {
    extfloat::render(&Spec::fixed(2), ExtF80::from_f64(x))
}

/// `procps_uptime_sprint`, from what it reads: the wall clock `now`, already
/// in local time; the seconds since boot; the session count; and the three
/// load averages.
///
/// ```text
///  20:48:41 up  1:21,  1 user,  load average: 0.10, 0.10, 0.09
///  09:02:30 up 3 days, 0 min, 12 users,  load average: 0.00, 0.01, 0.05
/// ```
///
/// Every width is upstream's format string's. Under an hour the uptime is
/// `N min` with no clock at all; at an hour or more the hour is padded with a
/// space, never a zero; the session count is `%2d` and singular at zero; and
/// the arithmetic is `int`'s, on `(int) uptime_secs` -- which is why an
/// uptime past `INT_MAX` seconds prints as a negative one (see [`c_int`]).
#[must_use]
pub fn status_line(now: &Tm, uptime_secs: f64, users: i32, load: (f64, f64, f64)) -> String {
    let up = c_int(uptime_secs);
    // `int` division and remainder: both truncate toward zero, as Rust's do,
    // and neither can overflow with a positive constant divisor.
    let updays = up / (60 * 60 * 24);
    let uphours = (up / (60 * 60)) % 24;
    let upminutes = (up / 60) % 60;

    let mut line = format!(" {:02}:{:02}:{:02} up ", now.hour, now.minute, now.second);
    if updays != 0 {
        let unit = if updays > 1 { "days" } else { "day" };
        line.push_str(&format!("{updays} {unit}, "));
    }
    if uphours != 0 {
        line.push_str(&format!("{uphours:2}:{upminutes:02}, "));
    } else {
        line.push_str(&format!("{upminutes} min, "));
    }
    if users < 0 {
        line.push_str(" ? ");
    } else {
        line.push_str(&format!("{users:2} "));
    }
    let noun = if users > 1 { "users" } else { "user" };
    line.push_str(&format!(
        "{noun},  load average: {}, {}, {}",
        fixed2(load.0),
        fixed2(load.1),
        fixed2(load.2)
    ));
    line
}

/// `count_users`: the `USER_PROCESS` entries with a name -- every one, live
/// or stale, since upstream does not ask whether the process still exists.
#[must_use]
pub fn count_users(records: &[Record]) -> i32 {
    let n = records.iter().filter(|r| is_user_process(r)).count();
    i32::try_from(n).unwrap_or(i32::MAX)
}

/// `procps_hertz_get`: clock ticks per second -- `sysconf(_SC_CLK_TCK)`, or
/// 100 when that has no positive answer.
#[must_use]
pub fn hertz() -> i64 {
    let hz = libcall::conf::sysconf(SC_CLK_TCK);
    if hz > 0 { hz } else { 100 }
}

// ---------------------------------------------------------------------------
// library/escape.c
// ---------------------------------------------------------------------------

/// `UTF_tab`: how long a UTF-8 sequence starting with `b` is, or `None` for a
/// byte that cannot start one -- a continuation byte, the overlong `C0` and
/// `C1`, and everything past `F4`.
fn utf8_len(b: u8) -> Option<usize> {
    match b {
        0x00..=0x7f => Some(1),
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

/// `esc_ctl`, in a UTF-8 locale: each byte that does not begin a well-formed
/// sequence becomes `?` -- a bad lead byte, a sequence cut short by the end,
/// a continuation byte outside `80`-`BF` -- as does a C1 control (`C2 80` to
/// `C2 9F`) and an ASCII control; a good sequence is left alone. Only the
/// *first* byte of a bad sequence is replaced, so the bytes after it are
/// judged again as leads, and fail as leads.
fn esc_ctl(s: &mut [u8]) {
    let len = s.len();
    let mut i = 0usize;
    while i < len {
        let lead = s.get(i).copied().unwrap_or(0);
        let n = utf8_len(lead);
        let whole = n.filter(|&n| i.saturating_add(n) <= len);
        let c1 = lead == 0xc2
            && s.get(i.saturating_add(1))
                .is_some_and(|b| (0x80..=0x9f).contains(b));
        let good = whole.filter(|&n| {
            !c1 && (1..n).all(|x| {
                s.get(i.saturating_add(x))
                    .is_some_and(|b| (0x80..=0xbf).contains(b))
            })
        });
        match good {
            Some(n) => {
                if (lead < 0x20 || lead == 0x7f)
                    && let Some(b) = s.get_mut(i)
                {
                    *b = b'?';
                }
                i = i.saturating_add(n);
            }
            None => {
                if let Some(b) = s.get_mut(i) {
                    *b = b'?';
                }
                i = i.saturating_add(1);
            }
        }
    }
}

/// `esc_all`, anywhere but a UTF-8 locale: `ESC_tab`'s three classes -- a
/// control or DEL becomes `.`, a byte above `7F` becomes `?`, and printable
/// ASCII stays.
fn esc_all(s: &mut [u8]) {
    for b in s {
        match *b {
            0x20..=0x7e => {}
            0x80..=0xff => *b = b'?',
            _ => *b = b'.',
        }
    }
}

/// procps' `escape_str`: `src` up to its first NUL and at most `bufsize - 1`
/// bytes of it -- the `snprintf` into a `bufsize`-byte buffer -- made safe to
/// print: `esc_ctl`'s rules in a UTF-8 locale (`utf8`), `esc_all`'s in
/// any other.
#[must_use]
pub fn escape_str(src: &[u8], bufsize: usize, utf8: bool) -> Vec<u8> {
    let Some(room) = bufsize.checked_sub(1) else {
        return Vec::new();
    };
    let end = src.iter().position(|&b| b == 0).unwrap_or(src.len());
    let mut out = src.get(..end.min(room)).unwrap_or_default().to_vec();
    if utf8 {
        esc_ctl(&mut out);
    } else {
        esc_all(&mut out);
    }
    out
}

/// procps' `escape_command` with `ESC_BRACKETS | ESC_DEFUNCT` -- how
/// `readproc` names a process that has no command line: `[name]`, and
/// `[name] <defunct>` for a zombie (`state` `Z`). `cmd` is the name as
/// `readproc` holds it, already through [`escape_str`]; `bytes` is the room
/// for the whole result and its NUL, and with less than a byte to spare for
/// the name there is no result at all.
#[must_use]
pub fn escape_command(cmd: &[u8], state: u8, bytes: usize, utf8: bool) -> Vec<u8> {
    let defunct = state == b'Z';
    let overhead: usize = if defunct { 12 } else { 2 };
    if overhead.saturating_add(1) >= bytes {
        return Vec::new();
    }
    let mut out = vec![b'['];
    out.extend_from_slice(&escape_str(cmd, bytes.saturating_sub(overhead), utf8));
    out.push(b']');
    if defunct {
        out.extend_from_slice(b" <defunct>");
    }
    out
}

/// `read_unvectored`'s rewrite of `/proc/<pid>/cmdline`, given the bytes read
/// from it -- of which upstream reads at most `MAX_BUFSZ - 1`.
///
/// The NULs separating the arguments, and any newline in one, become spaces;
/// NULs at the very end are dropped first, so the last argument gets no
/// trailing space; and a space that is the last byte *read* is cut, which is
/// different from the last byte kept: `a b \0` keeps its space. `None` when
/// nothing was read, which is when upstream names the process by
/// [`escape_command`] instead; a command line of NULs alone reads as
/// `Some(empty)`.
#[must_use]
pub fn unvectored(raw: &[u8]) -> Option<Vec<u8>> {
    let n = raw.len().min(MAX_BUFSZ.saturating_sub(1));
    let mut dst = raw.get(..n)?.to_vec();
    let ends_in_space = matches!(dst.last()?, b' ' | b'\n');
    let keep = dst
        .iter()
        .rposition(|&b| b != 0)
        .map_or(0, |at| at.saturating_add(1));
    for b in dst.iter_mut().take(keep) {
        if *b == b'\n' || *b == 0 {
            *b = b' ';
        }
    }
    // `dst[n-1]`: the last byte read, now a space if it was one or was a
    // newline. When it was a NUL, `keep < n` and there is nothing to cut.
    if keep == n && ends_in_space {
        dst.truncate(n.saturating_sub(1));
    } else {
        dst.truncate(keep);
    }
    Some(dst)
}

/// `fill_cmdline_cvt`: the `CMDLINE` item, from what `/proc/<pid>/cmdline`
/// held (`raw`; `None` when it could not be read, which is treated as an
/// empty file is, as upstream's `read_unvectored` returns 0 for both) and,
/// for a process with no command line, its name and state.
#[must_use]
pub fn cmdline_cvt(raw: Option<&[u8]>, cmd: &[u8], state: u8, utf8: bool) -> Vec<u8> {
    let text = match raw.and_then(unvectored) {
        Some(line) => escape_str(&line, MAX_BUFSZ, utf8),
        None => escape_command(cmd, state, MAX_BUFSZ, utf8),
    };
    if text.is_empty() { b"?".to_vec() } else { text }
}

// ---------------------------------------------------------------------------
// readproc, for w
// ---------------------------------------------------------------------------

/// One process as `readproc` describes it to `w`: the ten items `w.c` asks
/// `procps_pids_new` for, with the two that are one number for a process
/// (`PIDS_ID_PID`, `PIDS_ID_TGID`) given once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// `PIDS_ID_TGID`: the number the `/proc` directory is named by.
    pub pid: i32,
    /// `PIDS_TICS_BEGAN`: when it started, in clock ticks after boot.
    pub start: u64,
    /// `PIDS_ID_EUID`: the `Uid:` line's second column -- or, if `status`
    /// could not be read, the owner of the `/proc` directory.
    pub euid: u32,
    /// `PIDS_ID_RUID`: the `Uid:` line's first column, zero without one.
    pub ruid: u32,
    /// `PIDS_ID_TPGID`: the foreground process group of its terminal.
    pub tpgid: i32,
    /// `PIDS_ID_PGRP`.
    pub pgrp: i32,
    /// `PIDS_TTY`: its controlling terminal's device number, zero for none.
    pub tty: i32,
    /// `PIDS_TICS_ALL`: user and system time together, in ticks.
    pub tics_all: u64,
    /// `PIDS_CMDLINE`, through [`cmdline_cvt`].
    pub cmdline: Vec<u8>,
}

/// `procps_pids_reap(info, PIDS_FETCH_TASKS_ONLY)` for `w`'s items: every
/// process under `procfs`, in the order the directory lists them -- which on
/// Linux is process-ID order, and so the order upstream sees them -- read by
/// [`readproc`] as upstream's library reads them: `stat`, then `status`
/// (whose `Tgid:`, `Pid:` and `Uid:` lines win), then `cmdline`. A process
/// that exits while being read, or whose `stat` cannot be read, is left out.
///
/// # Errors
///
/// `/proc` itself could not be listed: upstream's `Unable to load process
/// information`.
pub fn tasks(procfs: &ProcFs, utf8: bool) -> io::Result<Vec<Task>> {
    let fill = readproc::Fill {
        stat: true,
        status: true,
        cmdline: true,
        ..readproc::Fill::default()
    };
    // `w` asks for no names, so no password database is read.
    let pw = pwcache::Pwcache::with_db(pwdb::Db::default());
    let mut reader = readproc::Reader::new(procfs.root().to_path_buf(), fill, utf8, pw);
    Ok(reader
        .reap()?
        .into_iter()
        .map(|p| Task {
            pid: p.tgid,
            start: p.start_time,
            euid: p.euid,
            ruid: p.ruid,
            tpgid: p.tpgid,
            pgrp: p.pgrp,
            tty: p.tty,
            tics_all: p.utime.wrapping_add(p.stime),
            // Always read, so always there; `?` is what upstream shows for
            // a command line that escapes to nothing.
            cmdline: p.cmdline.unwrap_or_else(|| b"?".to_vec()),
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::utmp::{DEAD_PROCESS, LOGIN_PROCESS, USER_PROCESS};

    #[test]
    fn scanf_reads_what_strtod_would_and_stops_where_it_stops() {
        assert_eq!(
            scan_doubles(b"16000.00 100000.00\n", 2),
            [16000.0, 100_000.0]
        );
        assert_eq!(scan_doubles(b"  \t1.5\n\n2.5", 2), [1.5, 2.5]);
        assert_eq!(scan_doubles(b"-3 +4", 2), [-3.0, 4.0]);
        assert_eq!(scan_doubles(b"1e3 2E-1", 2), [1000.0, 0.2]);
        assert_eq!(scan_doubles(b"0x10 0x1p4", 2), [16.0, 16.0]);
        assert_eq!(scan_doubles(b".5 5.", 2), [0.5, 5.0]);
        // Junk ends the scan, after what parsed.
        assert_eq!(scan_doubles(b"1.5 abc 2", 3), [1.5]);
        assert_eq!(scan_doubles(b"16000.00x 5", 2), [16000.0]);
        assert!(scan_doubles(b"", 2).is_empty());
        assert!(scan_doubles(b"   ", 2).is_empty());
        assert!(scan_doubles(b"abc", 2).is_empty());
    }

    /// glibc collects before it converts, which is where `fscanf` and `strtod`
    /// part company.
    #[test]
    fn scanf_consumes_a_dangling_exponent_and_refuses_a_bare_prefix() {
        // `1e` is consumed whole and is 1; the 5 after the space is the
        // second number. `strtod` would leave the `e` and fail on it.
        assert_eq!(scan_doubles(b"1e 5", 2), [1.0, 5.0]);
        assert_eq!(scan_doubles(b"1e+ 5", 2), [1.0, 5.0]);
        // `0x` with no digit, a lone sign, a lone point: failures.
        assert!(scan_doubles(b"0x 5", 2).is_empty());
        assert!(scan_doubles(b"- 5", 2).is_empty());
        assert!(scan_doubles(b"-", 2).is_empty());
        assert!(scan_doubles(b". 5", 2).is_empty());
        // A second point ends the number; it does not end the scan.
        assert_eq!(scan_doubles(b"1.2.3", 2), [1.2, 0.3]);
        // `0` then something that is not `x` is a digit already read.
        assert_eq!(scan_doubles(b"0 7", 2), [0.0, 7.0]);
        assert_eq!(scan_doubles(b"0", 2), [0.0]);
    }

    #[test]
    fn scanf_reads_the_named_values() {
        let v = scan_doubles(b"inf -INFINITY nan NaN", 4);
        assert_eq!(v.len(), 4);
        assert!(v[0].is_infinite() && v[0] > 0.0);
        assert!(v[1].is_infinite() && v[1] < 0.0);
        assert!(v[2].is_nan() && v[3].is_nan());
        // `infin` then anything but `ity` fails; `inf` then anything else is
        // `inf` with the rest left for the next conversion.
        assert!(scan_doubles(b"infin", 1).is_empty());
        assert_eq!(scan_doubles(b"inf5", 2).len(), 2);
        assert!(scan_doubles(b"na", 1).is_empty());
    }

    #[test]
    fn uptime_needs_both_numbers() {
        assert_eq!(uptime_secs(b"16000.00 100000.00\n"), Some(16000.0));
        assert_eq!(uptime_secs(b"16000.00\n"), None);
        assert_eq!(uptime_secs(b""), None);
        assert_eq!(uptime_secs(b"garbage"), None);
    }

    #[test]
    fn load_averages_keep_what_parsed_and_zero_the_rest() {
        assert_eq!(
            load_averages(b"0.07 1.05 12.34 2/297 51893\n"),
            (0.07, 1.05, 12.34)
        );
        assert_eq!(load_averages(b"1.50 x"), (1.5, 0.0, 0.0));
        assert_eq!(load_averages(b"a line of prose"), (0.0, 0.0, 0.0));
        assert_eq!(load_averages(b""), (0.0, 0.0, 0.0));
    }

    #[test]
    fn c_int_truncates_and_overflows_to_int_min() {
        assert_eq!(c_int(16000.99), 16000);
        assert_eq!(c_int(-5.5), -5);
        assert_eq!(c_int(2_147_483_647.9), i32::MAX);
        assert_eq!(c_int(2_147_483_648.0), i32::MIN);
        assert_eq!(c_int(-2_147_483_648.9), i32::MIN);
        assert_eq!(c_int(-2_147_483_649.0), i32::MIN);
        assert_eq!(c_int(f64::NAN), i32::MIN);
        assert_eq!(c_int(f64::INFINITY), i32::MIN);
    }

    fn clock(hour: u32, minute: u32, second: u32) -> Tm {
        let mut tm = localtime::Zone::utc().local(0, 0);
        tm.hour = hour;
        tm.minute = minute;
        tm.second = second;
        tm
    }

    #[test]
    fn the_status_line_is_upstreams() {
        let now = clock(20, 48, 41);
        let load = (0.10, 0.10, 0.09);
        assert_eq!(
            status_line(&now, 4860.0, 1, load),
            " 20:48:41 up  1:21,  1 user,  load average: 0.10, 0.10, 0.09"
        );
        assert_eq!(
            status_line(&now, 59.0, 0, load),
            " 20:48:41 up 0 min,  0 user,  load average: 0.10, 0.10, 0.09"
        );
        assert_eq!(
            status_line(&now, 3599.0, 2, load),
            " 20:48:41 up 59 min,  2 users,  load average: 0.10, 0.10, 0.09"
        );
        assert_eq!(
            status_line(&now, 86400.0, 12, load),
            " 20:48:41 up 1 day, 0 min, 12 users,  load average: 0.10, 0.10, 0.09"
        );
        assert_eq!(
            status_line(&now, 2.0 * 86400.0 + 3600.0, 1, load),
            " 20:48:41 up 2 days,  1:00,  1 user,  load average: 0.10, 0.10, 0.09"
        );
        // Past INT_MAX seconds the arithmetic is on INT_MIN, as upstream's is.
        assert_eq!(
            status_line(&now, 3.0e9, 1, load),
            " 20:48:41 up -24855 day, -3:-14,  1 user,  load average: 0.10, 0.10, 0.09"
        );
        // A count upstream could not take is ` ? `.
        assert!(status_line(&now, 60.0, -1, load).contains("up 1 min,  ? user,"));
    }

    #[test]
    fn load_averages_print_as_c_prints_them() {
        let now = clock(0, 0, 0);
        let line = status_line(&now, 0.0, 0, (f64::NAN, -f64::NAN, f64::INFINITY));
        assert!(line.ends_with("load average: nan, -nan, inf"), "{line}");
        let line = status_line(&now, 0.0, 0, (0.125, 0.375, 2.675));
        // Exact ties go to even; 2.675 is below its decimal spelling.
        assert!(line.ends_with("load average: 0.12, 0.38, 2.67"), "{line}");
    }

    fn rec(record_type: i32, user: &[u8]) -> Record {
        Record {
            record_type,
            user: user.to_vec(),
            tty: b"pts/0".to_vec(),
            host: Vec::new(),
            id: Vec::new(),
            pid: 1,
            login_time: 0,
            tv_sec: 0,
            login_usec: 0,
            exit_status: 0,
            session: 0,
            addr_v6: [0; 4],
        }
    }

    #[test]
    fn users_are_named_user_processes() {
        let records = [
            rec(USER_PROCESS, b"a"),
            rec(USER_PROCESS, b""),
            rec(LOGIN_PROCESS, b"LOGIN"),
            rec(DEAD_PROCESS, b"b"),
            rec(USER_PROCESS, b"a"),
        ];
        assert_eq!(count_users(&records), 2);
    }

    #[test]
    fn escape_str_in_utf8() {
        assert_eq!(escape_str(b"plain text", 100, true), b"plain text");
        assert_eq!(escape_str(b"caf\xc3\xa9", 100, true), b"caf\xc3\xa9");
        assert_eq!(escape_str(b"tab\there\x7f", 100, true), b"tab?here?");
        // A lone continuation byte, a truncated sequence, overlong leads.
        assert_eq!(escape_str(b"a\x80b", 100, true), b"a?b");
        assert_eq!(escape_str(b"a\xc3", 100, true), b"a?");
        assert_eq!(escape_str(b"\xc0\xaf", 100, true), b"??");
        assert_eq!(escape_str(b"\xf5\x80", 100, true), b"??");
        // A bad continuation replaces only the lead; the rest is judged again.
        assert_eq!(escape_str(b"\xe2\x28\xa1", 100, true), b"?(?");
        // C1 controls, `C2 80` to `C2 9F`, are controls -- and only the lead
        // is replaced, so the `85` after it fails as a lead in turn; `C2 A0`
        // is not a control.
        assert_eq!(escape_str(b"\xc2\x85x", 100, true), b"??x");
        assert_eq!(escape_str(b"\xc2\xa0", 100, true), b"\xc2\xa0");
        // Up to the NUL, and at most `bufsize - 1` bytes.
        assert_eq!(escape_str(b"ab\0cd", 100, true), b"ab");
        assert_eq!(escape_str(b"abcdef", 4, true), b"abc");
        assert_eq!(escape_str(b"abcdef", 1, true), b"");
        assert_eq!(escape_str(b"abcdef", 0, true), b"");
        // A cut through a sequence leaves a truncated one, which is a `?`.
        assert_eq!(escape_str(b"ab\xc3\xa9", 4, true), b"ab?");
    }

    #[test]
    fn escape_str_elsewhere() {
        assert_eq!(escape_str(b"tab\there\x7f", 100, false), b"tab.here.");
        assert_eq!(escape_str(b"caf\xc3\xa9", 100, false), b"caf??");
        assert_eq!(escape_str(b"\x01~ ", 100, false), b".~ ");
    }

    #[test]
    fn escape_command_brackets_and_marks_a_zombie() {
        assert_eq!(
            escape_command(b"kworker/0:1", b'S', MAX_BUFSZ, true),
            b"[kworker/0:1]"
        );
        assert_eq!(
            escape_command(b"sh", b'Z', MAX_BUFSZ, true),
            b"[sh] <defunct>"
        );
        assert_eq!(escape_command(b"", b'R', MAX_BUFSZ, true), b"[]");
        // No room for a byte of the name: nothing at all.
        assert_eq!(escape_command(b"sh", b'S', 3, true), b"");
        assert_eq!(escape_command(b"sh", b'S', 4, true), b"[s]");
        assert_eq!(escape_command(b"sh", b'Z', 13, true), b"");
    }

    #[test]
    fn unvectored_joins_the_arguments() {
        assert_eq!(unvectored(b"sleep\x00600\x00"), Some(b"sleep 600".to_vec()));
        assert_eq!(unvectored(b"a\nb\x00c\x00\x00"), Some(b"a b c".to_vec()));
        // The last byte read is cut if it is a space or a newline...
        assert_eq!(unvectored(b"a b "), Some(b"a b".to_vec()));
        assert_eq!(unvectored(b"a b\n"), Some(b"a b".to_vec()));
        // ...but not a space before the trailing NUL.
        assert_eq!(unvectored(b"a b \x00"), Some(b"a b ".to_vec()));
        // NULs alone are something read, and nothing to show.
        assert_eq!(unvectored(b"\x00\x00"), Some(Vec::new()));
        assert_eq!(unvectored(b""), None);
    }

    #[test]
    fn the_cmdline_item_falls_back_to_the_name_and_then_to_a_question_mark() {
        assert_eq!(
            cmdline_cvt(Some(b"sleep\x00600\x00"), b"sleep", b'S', true),
            b"sleep 600"
        );
        assert_eq!(cmdline_cvt(None, b"kthreadd", b'S', true), b"[kthreadd]");
        assert_eq!(
            cmdline_cvt(Some(b""), b"kthreadd", b'S', true),
            b"[kthreadd]"
        );
        assert_eq!(cmdline_cvt(None, b"sh", b'Z', true), b"[sh] <defunct>");
        assert_eq!(cmdline_cvt(Some(b"\x00"), b"sh", b'S', true), b"?");
        assert_eq!(cmdline_cvt(Some(b"\x1b[2J\x00"), b"x", b'S', true), b"?[2J");
    }

    #[test]
    fn the_tick_rate_is_positive() {
        assert!(hertz() > 0);
    }
}
