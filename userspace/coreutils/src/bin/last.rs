//! `last`, `lastb` -- show a listing of last logged in users: util-linux
//! 2.39.3's, ported.
//!
//! ```text
//! last [options] [<username>...] [<tty>...]
//! ```
//!
//! A transcription of `login-utils/last.c`, sysvinit's `last` as util-linux
//! carries it. The program reads a wtmp file (`/var/log/wtmp`; btmp,
//! `/var/log/btmp`, when it is called `lastb`) *backwards*, and pairs each
//! login with the next record on its line -- a logout, a later login, or a
//! shutdown or reboot -- to say how long the session lasted. What upstream
//! does and this keeps:
//!
//! - **The backwards read** (`uread`): the file through a stdio stream, its
//!   length by `fseeko` and `ftello`, and 16 KiB chunks from the end, a
//!   record that straddles two chunks spliced from both. A file that is not
//!   a whole number of 384-byte records is read misaligned exactly as
//!   upstream's is, and the state is upstream's statics: what one `-f` file
//!   leaves, the next one starts from. A file that cannot be measured -- a
//!   pipe -- is `seek on FILE failed`; one that cannot be read -- a directory
//!   -- `cannot read FILE`; neither stops the run.
//! - **The pairing** (`process_wtmp_file`): the `~` records for shutdown,
//!   reboot and run-level changes, the compatibility guesses for old writers
//!   (`ut_type` from whether `ut_user` and `ut_line` are filled, `date` with
//!   `|` or `{` as a clock change), each login matched with the newest
//!   following record on its line and every such record dropped, and a login
//!   with none: still logged in, gone without a logout (a phantom: older than
//!   this boot, a user `getpwnam` does not know, or a process whose
//!   `/proc/PID/loginuid` -- else whose tty's owner -- is not that user), or
//!   cut short by a crash or a shutdown.
//! - **The line** (`list`): `%-8.*s %-12.12s %-16.*s ...` as upstream builds
//!   it in a 512-byte buffer, trailing blanks trimmed by its own function --
//!   which, given a line whose only non-blank is its first character, loses
//!   that character too -- and written through `fputs_careful`: a control
//!   character as `*` and its letter, a byte that is not a printable UTF-8
//!   character as `\ooo`.
//! - **Times**: `short` (`ctime`'s first 16 characters, logouts as `HH:MM`),
//!   `full`, `iso` (`2026-10-08T13:00:00+00:00`) and `notime`, in the local
//!   zone; durations `(D+HH:MM)` or ` (HH:MM)`, with upstream's arithmetic for
//!   a logout before its login.
//! - **`-s`, `-t`, `-p`** read by util-linux's `parse_timestamp`
//!   (`ultimeutils`), **`-d`** and **`-i`** by the C library's
//!   `getnameinfo`, so an address is written as the library writes it.
//! - **`-n` and `-NUMBER` count across every `-f` file**, as upstream's one
//!   counter does, and the two add up: `-n 5 -3` is 53.
//! - **`SIGINT`** ends the run with `last: Interrupted`, status 1, and what
//!   was buffered for standard output lost, as `_exit` loses it;
//!   **`SIGQUIT`** says the same and carries on.
//!
//! # Deliberate differences
//!
//! - `--version` names SlateOS coreutils, as every program here does.
//! - **Names and arguments echoed in a diagnostic** -- a `-f` file, a `-s`
//!   time, the program's own name -- have their unprintable bytes escaped
//!   (`quoting::escape_unprintable`), so none can forge a line of its own.
//!   Upstream prints the bytes. The listing itself is upstream's.
//! - A read cut short by the end of a file, which sets no `errno`, is
//!   reported as `cannot read FILE: Success`; upstream names whatever `errno`
//!   last held. Only a file whose size `fstat` overstates -- a sysfs
//!   attribute -- gets there.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io::SeekFrom;
use std::process::ExitCode;

use coreutils::getopt::{Opt, Program, Takes};
use coreutils::quote::{escape_unprintable, os_bytes, os_from_bytes};
use coreutils::stdfd;
use coreutils::stdio::StdioReader;

coreutils::guard_std_fds!();

/// The parser. Diagnostics name the program by `argv[0]` -- glibc's getopt
/// as given, util-linux's own by its last component -- not by this.
const LAST: Program = Program::new("last", 1);

/// Upstream's `getopt_long` string.
const SHORT_OPTIONS: &str = "hVf:n:RxadFit:p:s:0123456789w";

/// Upstream's `long_opts`, in its order.
const LONG_OPTIONS: &[(&str, Takes)] = &[
    ("limit", Takes::Required),
    ("help", Takes::Nothing),
    ("file", Takes::Required),
    ("nohostname", Takes::Nothing),
    ("version", Takes::Nothing),
    ("hostlast", Takes::Nothing),
    ("since", Takes::Required),
    ("until", Takes::Required),
    ("present", Takes::Required),
    ("system", Takes::Nothing),
    ("dns", Takes::Nothing),
    ("ip", Takes::Nothing),
    ("fulltimes", Takes::Nothing),
    ("fullnames", Takes::Nothing),
    ("time-format", Takes::Required),
];

/// Each long option's `val`, in [`LONG_OPTIONS`]' order; `--time-format`'s
/// is `OPT_TIME_FORMAT`.
const LONG_VALS: [i32; 15] = [
    b'n' as i32,
    b'h' as i32,
    b'f' as i32,
    b'R' as i32,
    b'V' as i32,
    b'a' as i32,
    b's' as i32,
    b't' as i32,
    b'p' as i32,
    b'x' as i32,
    b'd' as i32,
    b'i' as i32,
    b'F' as i32,
    b'w' as i32,
    OPT_TIME_FORMAT,
];

/// `OPT_TIME_FORMAT`, `CHAR_MAX + 1`.
const OPT_TIME_FORMAT: i32 = 128;

/// `excl[]`: `-F` and `--time-format`, which may not both be given.
const EXCL: [&[i32]; 1] = [&[b'F' as i32, OPT_TIME_FORMAT]];

/// `_PATH_WTMP` and `_PATH_BTMP`.
const PATH_WTMP: &[u8] = b"/var/log/wtmp";
const PATH_BTMP: &[u8] = b"/var/log/btmp";

/// `sizeof (struct utmpx)` on x86-64 glibc.
const UTSIZE: usize = utmpfile::RECORD_SIZE;
/// `UCHUNKSIZE`: how much `uread` reads at once.
const UCHUNKSIZE: usize = 16_384;
/// The two as `uread`'s signed arithmetic has them.
const UTSIZE_I64: i64 = 384;
const UCHUNKSIZE_I64: i64 = 16_384;
/// `LAST_LOGIN_LEN` and `LAST_DOMAIN_LEN`: the user and host widths.
const LAST_LOGIN_LEN: usize = 8;
const LAST_DOMAIN_LEN: usize = 16;
/// `LAST_TIMESTAMP_LEN`: the size of each time's buffer.
const LAST_TIMESTAMP_LEN: usize = 32;
/// `sizeof (final)`.
const FINAL_SIZE: usize = 512;
/// `sizeof (domain)`: what `getnameinfo` may write, and one more than the
/// most of `ut_host` that is kept.
const DOMAIN_SIZE: usize = 256;

/// `ut_type` values, as `<utmpx.h>` numbers them, and `SHUTDOWN_TIME`.
const EMPTY: i16 = 0;
const RUN_LVL: i16 = 1;
const BOOT_TIME: i16 = 2;
const NEW_TIME: i16 = 3;
const OLD_TIME: i16 = 4;
const INIT_PROCESS: i16 = 5;
const LOGIN_PROCESS: i16 = 6;
const USER_PROCESS: i16 = 7;
const DEAD_PROCESS: i16 = 8;
/// glibc defines it under `_GNU_SOURCE`, which util-linux builds with, so
/// upstream ignores it with the others rather than calling it unrecognized.
const ACCOUNTING: i16 = 9;
const SHUTDOWN_TIME: i16 = 254;

/// Why a session ended -- `R_*`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Why {
    /// No logout record, a boot in between.
    Crash,
    /// Brought down in a decent way.
    Down,
    Normal,
    /// Still logged in.
    Now,
    Reboot,
    /// No logout record, and the session is stale.
    Phantom,
    /// `NEW_TIME` or `OLD_TIME`.
    TimeChange,
}

/// `LAST_TIMEFTM_*`, `HHMM` being the private one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TimeFmt {
    None,
    Ctime,
    Iso8601,
    Hhmm,
}

/// `struct last_timefmt`: how wide a login and a logout time print, and in
/// which format.
struct TimeFmtSpec {
    name: &'static str,
    in_len: usize,
    in_fmt: TimeFmt,
    out_len: usize,
    out_fmt: TimeFmt,
}

/// The public formats' places in [`TIMEFMTS`].
const FMT_NONE: usize = 0;
const FMT_SHORT: usize = 1;
const FMT_CTIME: usize = 2;

/// `timefmts[]`.
static TIMEFMTS: [TimeFmtSpec; 4] = [
    TimeFmtSpec {
        name: "notime",
        in_len: 0,
        in_fmt: TimeFmt::None,
        out_len: 0,
        out_fmt: TimeFmt::None,
    },
    TimeFmtSpec {
        name: "short",
        in_len: 16,
        in_fmt: TimeFmt::Ctime,
        out_len: 7,
        out_fmt: TimeFmt::Hhmm,
    },
    TimeFmtSpec {
        name: "full",
        in_len: 24,
        in_fmt: TimeFmt::Ctime,
        out_len: 26,
        out_fmt: TimeFmt::Ctime,
    },
    TimeFmtSpec {
        name: "iso",
        in_len: 25,
        in_fmt: TimeFmt::Iso8601,
        out_len: 27,
        out_fmt: TimeFmt::Iso8601,
    },
];

/// `struct last_control`.
#[allow(
    clippy::struct_excessive_bools,
    reason = "upstream's bit-fields, one flag each, kept as it has them"
)]
struct Ctl {
    lastb: bool,
    extended: bool,
    showhost: bool,
    altlist: bool,
    usedns: bool,
    useip: bool,
    name_len: usize,
    domain_len: usize,
    maxrecs: u32,
    /// The names and ttys asked for; `None` for all.
    show: Option<Vec<Vec<u8>>>,
    /// `boot_time.tv_sec`.
    boot_time: i64,
    since: i64,
    until: i64,
    present: i64,
    /// Which of [`TIMEFMTS`].
    time_fmt: usize,
}

/// One `struct utmpx`, as its 384 bytes.
#[derive(Clone)]
struct Ut([u8; UTSIZE]);

impl Ut {
    fn from_slice(b: &[u8]) -> Option<Ut> {
        let mut u = [0u8; UTSIZE];
        u.copy_from_slice(b.get(..UTSIZE)?);
        Some(Ut(u))
    }

    fn field(&self, at: usize, len: usize) -> &[u8] {
        self.0.get(at..at.saturating_add(len)).unwrap_or_default()
    }

    fn i32_at(&self, at: usize) -> i32 {
        let mut b = [0u8; 4];
        b.copy_from_slice(self.field(at, 4));
        i32::from_le_bytes(b)
    }

    fn ut_type(&self) -> i16 {
        let mut b = [0u8; 2];
        b.copy_from_slice(self.field(utmpfile::UT_TYPE_OFFSET, 2));
        i16::from_le_bytes(b)
    }

    fn set_type(&mut self, t: i16) {
        let at = utmpfile::UT_TYPE_OFFSET;
        if let Some(f) = self.0.get_mut(at..at.saturating_add(2)) {
            f.copy_from_slice(&t.to_le_bytes());
        }
    }

    fn pid(&self) -> i32 {
        self.i32_at(utmpfile::UT_PID_OFFSET)
    }

    fn line(&self) -> &[u8] {
        self.field(utmpfile::UT_LINE_OFFSET, utmpfile::UT_LINE_SIZE)
    }

    fn user(&self) -> &[u8] {
        self.field(utmpfile::UT_USER_OFFSET, utmpfile::UT_USER_SIZE)
    }

    fn host(&self) -> &[u8] {
        self.field(utmpfile::UT_HOST_OFFSET, utmpfile::UT_HOST_SIZE)
    }

    /// `ut_tv.tv_sec`: a 32-bit field, made a `time_t`.
    fn sec(&self) -> i64 {
        i64::from(self.i32_at(utmpfile::UT_TV_SEC_OFFSET))
    }

    /// `ut_addr_v6`, its bytes as they are stored -- network order.
    fn addr(&self) -> [u8; 16] {
        let mut a = [0u8; 16];
        a.copy_from_slice(self.field(utmpfile::UT_ADDR_OFFSET, 16));
        a
    }

    /// `strcpy (ut.ut_line, s)`, or `snprintf` into it: `s` and a NUL over
    /// the start of the field, the rest of the field as it was.
    fn set_line(&mut self, s: &[u8]) {
        let at = utmpfile::UT_LINE_OFFSET;
        if let Some(f) = self
            .0
            .get_mut(at..at.saturating_add(utmpfile::UT_LINE_SIZE))
        {
            let n = s.len().min(f.len().saturating_sub(1));
            if let (Some(dst), Some(src)) = (f.get_mut(..n), s.get(..n)) {
                dst.copy_from_slice(src);
            }
            if let Some(nul) = f.get_mut(n) {
                *nul = 0;
            }
        }
    }
}

/// The C string at the start of `s`: up to its first NUL, or all of it.
fn c_str(s: &[u8]) -> &[u8] {
    let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    s.get(..end).unwrap_or_default()
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

/// `uread`'s statics. As upstream's, they outlive one file: a second `-f`
/// file that cannot be measured is read from where the first left them.
struct Uread {
    buf: Vec<u8>,
    fpos: i64,
    bpos: i64,
}

impl Uread {
    fn new() -> Self {
        Uread {
            buf: vec![0; UCHUNKSIZE],
            fpos: 0,
            bpos: 0,
        }
    }

    /// `uread (fp, &ut, NULL, filename)`: the first record, read forwards --
    /// `None` unless all of it is there.
    fn first(fp: &mut StdioReader) -> Option<Ut> {
        let mut u = [0u8; UTSIZE];
        let (got, _) = fp.fread(&mut u);
        (got == UTSIZE).then_some(Ut(u))
    }

    /// `uread (fp, NULL, NULL, filename)`: the last chunk, which may be
    /// short, into the buffer. Its failures are warnings, and the caller
    /// carries on regardless, as upstream's ignores what this returns.
    fn init(&mut self, fp: &mut StdioReader, diag: &Diag, filename: &[u8]) {
        // `fseeko (fp, 0, SEEK_END)`, unchecked, then `ftello`, whose -1 for
        // a stream that cannot seek is kept.
        drop(fp.seek(SeekFrom::End(0)));
        self.fpos = fp
            .tell()
            .map_or(-1, |p| i64::try_from(p).unwrap_or(i64::MAX));
        if self.fpos == 0 {
            return;
        }
        // C's division, which truncates: -1 gives 0.
        let o = self
            .fpos
            .saturating_sub(1)
            .checked_div(UCHUNKSIZE_I64)
            .unwrap_or(0)
            .saturating_mul(UCHUNKSIZE_I64);
        if let Err(e) = fp.seek(SeekFrom::Start(u64::try_from(o).unwrap_or(0))) {
            diag.warn(&[b"seek on ", filename, b" failed"], Some(&e));
            return;
        }
        self.bpos = self.fpos.saturating_sub(o);
        let n = usize::try_from(self.bpos).unwrap_or(0);
        let (got, e) = match self.buf.get_mut(..n) {
            Some(dst) if n > 0 => fp.fread(dst),
            _ => (0, None),
        };
        if got != n || n == 0 {
            diag.warn(&[b"cannot read ", filename], Some(&read_error(e)));
            return;
        }
        self.fpos = o;
    }

    /// `uread (fp, &ut, &quit, filename)`: the record before the last one,
    /// from the buffer while it lasts, else spliced with the chunk before.
    fn next(&mut self, fp: &mut StdioReader, diag: &Diag, filename: &[u8]) -> Option<Ut> {
        self.bpos = self.bpos.saturating_sub(UTSIZE_I64);
        if self.bpos >= 0 {
            let at = usize::try_from(self.bpos).ok()?;
            return Ut::from_slice(self.buf.get(at..)?);
        }
        // "Oops we went "below" the buffer."
        self.fpos = self.fpos.saturating_sub(UCHUNKSIZE_I64);
        if self.fpos < 0 {
            return None;
        }
        // The record's tail is the buffer's head. More than a record below
        // it is a state only a file that could not be measured leaves, and
        // upstream's copy of a negative length there does not return.
        let short = usize::try_from(self.bpos.unsigned_abs()).ok()?;
        let keep = UTSIZE.checked_sub(short)?;
        let mut tmp = [0u8; UTSIZE];
        tmp.get_mut(short..)?.copy_from_slice(self.buf.get(..keep)?);
        if let Err(e) = fp.seek(SeekFrom::Start(u64::try_from(self.fpos).ok()?)) {
            diag.warn(&[b"seek on ", filename, b" failed"], Some(&e));
            return None;
        }
        let (got, e) = fp.fread(&mut self.buf);
        if got != UCHUNKSIZE {
            diag.warn(&[b"cannot read ", filename], Some(&read_error(e)));
            return None;
        }
        // Its head is the end of the chunk before.
        tmp.get_mut(..short)?
            .copy_from_slice(self.buf.get(UCHUNKSIZE.checked_sub(short)?..)?);
        self.bpos = self.bpos.saturating_add(UCHUNKSIZE_I64);
        Some(Ut(tmp))
    }
}

/// A short `fread`'s reason: its error, or for the end of the file -- which
/// sets no `errno` -- none at all.
fn read_error(e: Option<std::io::Error>) -> std::io::Error {
    e.unwrap_or_else(|| std::io::Error::from_raw_os_error(0))
}

/// `ctime`'s text, `Thu Oct  8 13:00:00 2026`, without its newline: glibc's
/// `"%.3s %.3s%3d %.2d:%.2d:%.2d %d\n"`.
fn ctime(zone: &localtime::Zone, t: i64) -> Vec<u8> {
    let tm = zone.localtime(t, 0);
    let wday = usize::try_from(tm.wday)
        .ok()
        .and_then(|w| localtime::WDAY_ABBR.get(w))
        .copied()
        .unwrap_or(b"???");
    let mon = usize::try_from(tm.month)
        .ok()
        .and_then(|m| m.checked_sub(1))
        .and_then(|m| localtime::MON_ABBR.get(m))
        .copied()
        .unwrap_or(b"???");
    let mut s = Vec::with_capacity(26);
    s.extend_from_slice(wday);
    s.push(b' ');
    s.extend_from_slice(mon);
    s.extend_from_slice(
        format!(
            "{:3} {:02}:{:02}:{:02} {}",
            tm.day, tm.hour, tm.minute, tm.second, tm.year
        )
        .as_bytes(),
    );
    s
}

/// `time_formatter`: one time in one format -- into a buffer of `room`
/// bytes, which none of them fills.
fn time_formatter(zone: &localtime::Zone, fmt: TimeFmt, when: i64, room: usize) -> Vec<u8> {
    let mut s = match fmt {
        TimeFmt::None => Vec::new(),
        TimeFmt::Hhmm => {
            let tm = zone.localtime(when, 0);
            format!("{:02}:{:02}", tm.hour, tm.minute).into_bytes()
        }
        // `ctime_r`, copied in and its newline trimmed.
        TimeFmt::Ctime => ctime(zone, when),
        // `strtime_iso (when, ISO_TIMESTAMP_T, ...)`: the zone as whole hours
        // and minutes, the minutes always positive -- so half an hour west is
        // `+00:30`, as util-linux has it.
        TimeFmt::Iso8601 => {
            let tm = zone.localtime(when, 0);
            let tmin = tm.gmtoff / 60;
            let zhour = tmin / 60;
            let zmin = (tmin % 60).abs();
            format!(
                "{:4}-{:02}-{:02}T{:02}:{:02}:{:02}{:+03}:{:02}",
                tm.year, tm.month, tm.day, tm.hour, tm.minute, tm.second, zhour, zmin
            )
            .into_bytes()
        }
    };
    s.truncate(room.saturating_sub(1));
    s
}

/// `%-W.Ps`: the C string `s`, cut to `prec` bytes and padded to `width`.
fn put_field(out: &mut Vec<u8>, s: &[u8], width: usize, prec: usize) {
    let s = c_str(s);
    let s = s.get(..s.len().min(prec)).unwrap_or(s);
    out.extend_from_slice(s);
    out.resize(
        out.len().saturating_add(width.saturating_sub(s.len())),
        b' ',
    );
}

/// `isspace` in the C locale -- and in a UTF-8 one, where no byte above 0x7f
/// is a character by itself.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | 0x09..=0x0d)
}

/// `trim_trailing_spaces`: the line's trailing white space replaced by one
/// newline. Upstream's loop stops *on* the first character without testing
/// whether to step past it, so a line whose only non-blank is its first
/// character loses that character as well.
fn trim_trailing_spaces(s: &mut Vec<u8>) {
    let mut p = s.len();
    while p > 0 {
        p = p.saturating_sub(1);
        if !s.get(p).copied().is_some_and(is_space) {
            break;
        }
    }
    if p > 0 {
        p = p.saturating_add(1);
    }
    s.truncate(p);
    s.push(b'\n');
}

/// POSIX `basename` -- `<libgen.h>`'s, which `last.c` includes: the last
/// component, trailing slashes ignored; `/` for a path of slashes and `.` for
/// an empty one.
fn xpg_basename(path: &[u8]) -> &[u8] {
    if path.is_empty() {
        return b".";
    }
    let Some(last) = path.iter().rposition(|&c| c != b'/') else {
        return b"/";
    };
    let trimmed = path.get(..=last).unwrap_or(path);
    let start = trimmed
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(0, |i| i.saturating_add(1));
    trimmed.get(start..).unwrap_or_default()
}

/// How the run speaks on standard error: util-linux's `warn` and `warnx` --
/// standard output not flushed first -- under the program's short name.
struct Diag {
    /// `program_invocation_short_name`, escaped for printing.
    prog: Vec<u8>,
}

impl Diag {
    /// `PROG: PARTS[: REASON]`.
    fn warn(&self, parts: &[&[u8]], e: Option<&std::io::Error>) {
        let mut m = self.prog.clone();
        m.extend_from_slice(b": ");
        for part in parts {
            m.extend_from_slice(part);
        }
        if let Some(e) = e {
            m.extend_from_slice(b": ");
            m.extend_from_slice(coreutils::errmsg::strerror(e).as_bytes());
        }
        m.push(b'\n');
        ulclosestream::stderr_write(&m);
    }

    /// `errtryhelp (EXIT_FAILURE)`'s referral.
    fn try_help(&self) {
        let mut m = b"Try '".to_vec();
        m.extend_from_slice(&self.prog);
        m.extend_from_slice(b" --help' for more information.\n");
        ulclosestream::stderr_write(&m);
    }
}

/// Bytes from the user or the file system, made safe to print in a
/// diagnostic.
fn shown(s: &[u8]) -> Vec<u8> {
    escape_unprintable(s).into_bytes()
}

/// A run that has ended with this status, its diagnostic already out.
struct Die(u8);

/// The run: upstream's `last_control`, its file-scope statics, and the
/// standard output everything goes to.
struct Last<'o> {
    ctl: Ctl,
    diag: Diag,
    zone: localtime::Zone,
    /// `recsdone`: lines listed, across every file.
    recsdone: u32,
    /// `currentdate`: when this file's processing began.
    currentdate: i64,
    uread: Uread,
    /// The password database, read at the first `getpwnam`.
    passwd: Option<pwdb::Db>,
    out: &'o mut ulclosestream::Stdout,
}

impl Last<'_> {
    /// `dns_lookup`: the address as `getnameinfo` gives it -- a name for
    /// `-d`, the numbers for `-i` -- or `None` where that fails.
    ///
    /// "IPv4 or IPv6? 1. If last 3 4bytes are 0, must be IPv4. 2. If IPv6 in
    /// IPv4, handle as IPv4. 3. Anything else is IPv6."
    fn dns_lookup(&self, a: [u8; 16]) -> Option<Vec<u8>> {
        let flags = if self.ctl.useip {
            libcall::netdb::NI_NUMERICHOST
        } else {
            0
        };
        let word = |k: usize| -> [u8; 4] {
            let mut w = [0u8; 4];
            let at = k.saturating_mul(4);
            if let Some(src) = a.get(at..at.saturating_add(4)) {
                w.copy_from_slice(src);
            }
            w
        };
        let mapped = word(0) == [0; 4] && word(1) == [0; 4] && word(2) == [0, 0, 0xff, 0xff];
        let text = if mapped || (word(1) == [0; 4] && word(2) == [0; 4] && word(3) == [0; 4]) {
            let v4 = if mapped { word(3) } else { word(0) };
            libcall::netdb::ipv4_name_info(v4, flags, DOMAIN_SIZE)
        } else {
            libcall::netdb::ipv6_name_info(a, flags, DOMAIN_SIZE)
        };
        text.ok().map(|t| t.as_bytes().to_vec())
    }

    /// `list`: one line, or nothing when the names asked for or `-p` leave
    /// it out. True once the `-n` limit is reached.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's list, kept in one piece so it can be read against it"
    )]
    fn list(&mut self, p: &Ut, logout_time: i64, what: Why) -> bool {
        // "uucp and ftp have special-type entries"
        let mut utline = c_str(p.line()).to_vec();
        if utline.starts_with(b"ftp") && utline.get(3).is_some_and(u8::is_ascii_digit) {
            utline.truncate(3);
        }
        if utline.starts_with(b"uucp") && utline.get(4).is_some_and(u8::is_ascii_digit) {
            utline.truncate(4);
        }

        // "Is this something we want to show?"
        if let Some(show) = &self.ctl.show {
            let wanted = show.iter().any(|walk| {
                strneq(p.user(), walk, utmpfile::UT_USER_SIZE)
                    || utline == *walk
                    || (utline.starts_with(b"tty") && utline.get(3..) == Some(walk.as_slice()))
            });
            if !wanted {
                return false;
            }
        }

        let fmt = TIMEFMTS
            .get(self.ctl.time_fmt)
            .unwrap_or(&TIMEFMTS[FMT_SHORT]);
        let utmp_time = p.sec();

        if self.ctl.present != 0 {
            if self.ctl.present < utmp_time {
                return false;
            }
            if 0 < logout_time && logout_time < self.ctl.present {
                return false;
            }
        }

        let logintime = time_formatter(&self.zone, fmt.in_fmt, utmp_time, LAST_TIMESTAMP_LEN);

        // "Under strange circumstances, secs < 0 can happen": C's division
        // and remainder, which truncate.
        let secs = logout_time.wrapping_sub(utmp_time);
        let mins = (secs / 60) % 60;
        let hours = (secs / 3600) % 24;
        let days = secs / 86_400;

        let mut logouttime = b"- ".to_vec();
        logouttime.extend(time_formatter(
            &self.zone,
            fmt.out_fmt,
            logout_time,
            LAST_TIMESTAMP_LEN.saturating_sub(2),
        ));
        let long_fmt = self.ctl.time_fmt > FMT_SHORT;
        let mut length: Vec<u8> = if logout_time == self.currentdate {
            if long_fmt {
                logouttime = b"  still running".to_vec();
                Vec::new()
            } else {
                logouttime = b"  still".to_vec();
                b"running".to_vec()
            }
        } else if days != 0 {
            // "hours and mins always shown as positive (w/o minus sign!)"
            format!("({days}+{:02}:{:02})", hours.abs(), mins.abs()).into_bytes()
        } else if hours != 0 {
            format!(" ({hours:02}:{:02})", mins.abs()).into_bytes()
        } else if secs >= 0 {
            format!(" ({hours:02}:{mins:02})").into_bytes()
        } else {
            format!(" (-00:{:02})", mins.abs()).into_bytes()
        };

        match what {
            Why::Crash => logouttime = b"- crash".to_vec(),
            Why::Down => logouttime = b"- down ".to_vec(),
            Why::Now => {
                if long_fmt {
                    logouttime = b"  still logged in".to_vec();
                    length.clear();
                } else {
                    logouttime = b"  still".to_vec();
                    length = b"logged in".to_vec();
                }
            }
            Why::Phantom => {
                if long_fmt {
                    logouttime = b"  gone - no logout".to_vec();
                    length.clear();
                } else if self.ctl.time_fmt == FMT_SHORT {
                    logouttime = b"   gone".to_vec();
                    length = b"- no logout".to_vec();
                } else {
                    logouttime.clear();
                    length = b"no logout".to_vec();
                }
            }
            Why::TimeChange => {
                logouttime.clear();
                length.clear();
            }
            Why::Normal | Why::Reboot => {}
        }

        // "Look up host with DNS if needed." `mem2strcpy` keeps at most 255
        // bytes of `ut_host`.
        let looked_up = if self.ctl.usedns || self.ctl.useip {
            self.dns_lookup(p.addr())
        } else {
            None
        };
        let domain = looked_up.unwrap_or_else(|| {
            c_str(
                p.host()
                    .get(..DOMAIN_SIZE.saturating_sub(1))
                    .unwrap_or_default(),
            )
            .to_vec()
        });

        // `snprintf (final, sizeof (final), ...)`, in whichever of its three
        // shapes `-R` and `-a` choose.
        let mut line = Vec::with_capacity(128);
        put_field(&mut line, p.user(), LAST_LOGIN_LEN, self.ctl.name_len);
        line.push(b' ');
        put_field(&mut line, &utline, 12, 12);
        line.push(b' ');
        if self.ctl.showhost && !self.ctl.altlist {
            put_field(&mut line, &domain, LAST_DOMAIN_LEN, self.ctl.domain_len);
            line.push(b' ');
        }
        put_field(&mut line, &logintime, fmt.in_len, fmt.in_len);
        line.push(b' ');
        put_field(&mut line, &logouttime, fmt.out_len, fmt.out_len);
        line.push(b' ');
        if self.ctl.showhost && self.ctl.altlist {
            put_field(&mut line, &length, 12, 12);
            line.push(b' ');
            line.extend_from_slice(c_str(&domain));
        } else {
            line.extend_from_slice(&length);
        }
        line.push(b'\n');
        let len = line.len();
        if len >= FINAL_SIZE {
            line.truncate(FINAL_SIZE.saturating_sub(1));
        }

        trim_trailing_spaces(&mut line);
        // "Print out "final" string safely."
        let mut shown = ulstrutils::fputs_careful(&line, b'*', false, 0);
        if len >= FINAL_SIZE {
            shown.push(b'\n');
        }
        self.out.write(&shown);

        self.recsdone = self.recsdone.wrapping_add(1);
        self.ctl.maxrecs != 0 && self.ctl.maxrecs <= self.recsdone
    }

    /// `is_phantom`: whether a login with no logout record is stale.
    fn is_phantom(&mut self, ut: &Ut) -> bool {
        if ut.sec() < self.ctl.boot_time {
            return true;
        }
        let user = c_str(ut.user()).to_vec();
        let Some(uid) = self
            .passwd
            .get_or_insert_with(pwdb::Db::load)
            .user_by_name(&user)
            .map(|u| u.uid)
        else {
            return true;
        };
        let pid = u32::from_ne_bytes(ut.pid().to_ne_bytes());
        let path = format!("/proc/{pid}/loginuid");
        if stdfd::readable(path.as_bytes()).is_ok() {
            let Ok(text) = std::fs::read(&path) else {
                return true;
            };
            // `fscanf (f, "%u", &loginuid) != 1`: `strtoul`'s reading, kept
            // as an `unsigned int`.
            let (v, used) = cstrtol::strtoul(&text, 10);
            if used == 0 {
                return true;
            }
            let loginuid = u32::try_from(v & 0xffff_ffff).unwrap_or(u32::MAX);
            uid != loginuid
        } else {
            let mut dev = b"/dev/".to_vec();
            dev.extend_from_slice(c_str(ut.line()));
            match std::fs::metadata(os_from_bytes(&dev)) {
                Ok(st) => uid != uid_of(&st),
                Err(_) => true,
            }
        }
    }

    /// `process_wtmp_file`.
    #[allow(
        clippy::too_many_lines,
        reason = "upstream's process_wtmp_file, kept in one piece so it can be read against it"
    )]
    fn process_wtmp_file(&mut self, filename: &[u8]) -> Result<(), Die> {
        let mut lastdown = wall_clock();
        let mut lastrch = lastdown;
        self.currentdate = lastdown;
        let mut lastboot: i64 = 0;
        let mut whydown = Why::Crash;
        let mut quit = false;
        let mut down = false;

        install_handlers();

        let shown_name = shown(filename);
        let file = std::fs::File::open(os_from_bytes(filename)).map_err(|e| {
            self.diag.warn(&[b"cannot open ", &shown_name], Some(&e));
            Die(1)
        })?;
        let mut fp = StdioReader::from_file(file);

        // "Read first structure to capture the time field."
        let begintime = if let Some(first) = Uread::first(&mut fp) {
            first.sec()
        } else {
            quit = true;
            match fp.file().map(std::fs::File::metadata) {
                Some(Ok(st)) => ctime_of(&st),
                Some(Err(e)) => {
                    self.diag
                        .warn(&[b"stat of ", &shown_name, b" failed"], Some(&e));
                    return Err(Die(1));
                }
                None => 0,
            }
        };

        self.uread.init(&mut fp, &self.diag, &shown_name);
        // `ulist`: the logouts waiting for their login, newest first.
        let mut ulist: VecDeque<Ut> = VecDeque::new();

        while !quit {
            let Some(mut ut) = self.uread.next(&mut fp, &self.diag, &shown_name) else {
                break;
            };
            let t = ut.sec();
            if self.ctl.since != 0 && t < self.ctl.since {
                continue;
            }
            if self.ctl.until != 0 && self.ctl.until < t {
                continue;
            }
            if self.ctl.lastb {
                quit = self.list(&ut, t, Why::Normal);
                continue;
            }

            // "Set ut_type to the correct type."
            if ut.line().first() == Some(&b'~') {
                if strneq(ut.user(), b"shutdown", 8) {
                    ut.set_type(SHUTDOWN_TIME);
                } else if strneq(ut.user(), b"reboot", 6) {
                    ut.set_type(BOOT_TIME);
                } else if strneq(ut.user(), b"runlevel", 8) {
                    ut.set_type(RUN_LVL);
                }
            } else {
                // "For stupid old applications that don't fill in ut_type
                // correctly."
                let user0 = ut.user().first().copied().unwrap_or(0);
                let line0 = ut.line().first().copied().unwrap_or(0);
                if ut.ut_type() != DEAD_PROCESS
                    && user0 != 0
                    && line0 != 0
                    && !strneq(ut.user(), b"LOGIN", 5)
                {
                    ut.set_type(USER_PROCESS);
                }
                // "Even worse, applications that write ghost entries."
                if user0 == 0 {
                    ut.set_type(DEAD_PROCESS);
                }
                // "Clock changes."
                if strneq(ut.user(), b"date", 4) {
                    if line0 == b'|' {
                        ut.set_type(OLD_TIME);
                    }
                    if line0 == b'{' {
                        ut.set_type(NEW_TIME);
                    }
                }
            }

            let mut store = false;
            match ut.ut_type() {
                SHUTDOWN_TIME => {
                    if self.ctl.extended {
                        ut.set_line(b"system down");
                        quit = self.list(&ut, lastboot, Why::Normal);
                    }
                    lastdown = t;
                    lastrch = t;
                    down = true;
                }
                OLD_TIME | NEW_TIME => {
                    if self.ctl.extended {
                        let what: &[u8] = if ut.ut_type() == NEW_TIME {
                            b"new time"
                        } else {
                            b"old time"
                        };
                        ut.set_line(what);
                        quit = self.list(&ut, lastdown, Why::TimeChange);
                    }
                }
                BOOT_TIME => {
                    ut.set_line(b"system boot");
                    quit = self.list(&ut, lastdown, Why::Reboot);
                    lastboot = t;
                    down = true;
                }
                RUN_LVL => {
                    let x = ut.pid().to_le_bytes()[0];
                    if self.ctl.extended {
                        // `snprintf ("(to lvl %c)", x)`: a NUL `x` ends the
                        // string there, as C's does.
                        let text: [&[u8]; 3] = [b"(to lvl ", &[x], b")"];
                        ut.set_line(&text.concat());
                        quit = self.list(&ut, lastrch, Why::Normal);
                    }
                    if x == b'0' || x == b'6' {
                        lastdown = t;
                        down = true;
                        ut.set_type(SHUTDOWN_TIME);
                    }
                    lastrch = t;
                }
                USER_PROCESS => {
                    // "This was a login - show the first matching logout
                    // record and delete all records with the same ut_line."
                    let mut matched = false;
                    let mut k = 0usize;
                    while let Some(p) = ulist.get(k) {
                        if strneq(p.line(), ut.line(), utmpfile::UT_LINE_SIZE) {
                            if !matched {
                                let logout = p.sec();
                                quit = self.list(&ut, logout, Why::Normal);
                                matched = true;
                            }
                            ulist.remove(k);
                        } else {
                            k = k.saturating_add(1);
                        }
                    }
                    // "Not found? Then crashed, down, still logged in, or
                    // missing logout record."
                    if !matched {
                        let why = if lastboot == 0 {
                            if self.is_phantom(&ut) {
                                Why::Phantom
                            } else {
                                Why::Now
                            }
                        } else {
                            whydown
                        };
                        quit = self.list(&ut, lastboot, why);
                    }
                    store = true;
                }
                DEAD_PROCESS => store = true,
                EMPTY | INIT_PROCESS | LOGIN_PROCESS | ACCOUNTING => {}
                other => {
                    self.diag
                        .warn(&[format!("unrecognized ut_type: {other}").as_bytes()], None);
                }
            }
            // "Just store the data if it is interesting enough."
            if store && ut.line().first().is_some_and(|&c| c != 0) {
                ulist.push_front(ut.clone());
            }

            // "If we saw a shutdown/reboot record we can remove the entire
            // current ulist."
            if down {
                lastboot = t;
                whydown = if ut.ut_type() == SHUTDOWN_TIME {
                    Why::Down
                } else {
                    Why::Crash
                };
                ulist.clear();
                down = false;
            }
        }

        if self.ctl.time_fmt != FMT_NONE {
            let fmt = TIMEFMTS
                .get(self.ctl.time_fmt)
                .unwrap_or(&TIMEFMTS[FMT_SHORT]);
            let mut text = b"\n".to_vec();
            text.extend_from_slice(xpg_basename(filename));
            text.extend_from_slice(b" begins ");
            text.extend(time_formatter(
                &self.zone,
                fmt.in_fmt,
                begintime,
                LAST_TIMESTAMP_LEN,
            ));
            text.push(b'\n');
            self.out.write(&text);
        }

        // `fclose`, whose failure upstream does not look at: the file was
        // only read.
        drop(fp.close());
        Ok(())
    }
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

/// `get_boot_time`'s seconds: now less `CLOCK_BOOTTIME`, `timersub`'s way.
/// `None` where the clock cannot be read, which leaves upstream's
/// `boot_time` as the file before left it.
fn boot_time() -> Option<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    let (up_sec, up_nsec) = libcall::clock::since_boot().ok()?;
    let now_us = i128::from(now.as_secs())
        .saturating_mul(1_000_000)
        .saturating_add(i128::from(now.subsec_micros()));
    let up_us = i128::from(up_sec)
        .saturating_mul(1_000_000)
        .saturating_add(i128::from(up_nsec / 1000));
    i64::try_from(now_us.saturating_sub(up_us).div_euclid(1_000_000)).ok()
}

/// `st.st_uid`.
#[cfg(unix)]
fn uid_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt;
    meta.uid()
}

/// `st.st_uid`, which a host build has no notion of: root's.
#[cfg(not(unix))]
fn uid_of(_meta: &std::fs::Metadata) -> u32 {
    0
}

/// `st.st_ctime`.
#[cfg(unix)]
fn ctime_of(meta: &std::fs::Metadata) -> i64 {
    use std::os::unix::fs::MetadataExt;
    meta.ctime()
}

/// `st.st_ctime`, which a host build stands in for with the modification
/// time.
#[cfg(not(unix))]
fn ctime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

/// `program_invocation_short_name`, escaped, for the signal handlers. Set
/// once, before either is installed.
static PROGRAM_NAME: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();

/// `signal (SIGINT, int_handler)` and `signal (SIGQUIT, quit_handler)`, as
/// each file's processing begins. glibc's `signal` restarts what a handler
/// interrupts. Should either fail, the default stays, which is no worse.
fn install_handlers() {
    use libcall::signal::{SIGINT, SIGQUIT, set_handler};
    // Failure leaves the default disposition, as above: nothing to report.
    let _ = set_handler(SIGINT, int_handler, true);
    let _ = set_handler(SIGQUIT, quit_handler, true);
}

/// `ul_sig_warn ("Interrupted")`: `NAME: Interrupted`, by `write(2)`.
fn sig_warn() {
    let name = PROGRAM_NAME.get().map_or(&b"last"[..], Vec::as_slice);
    let mut m = Vec::with_capacity(name.len().saturating_add(14));
    m.extend_from_slice(name);
    m.extend_from_slice(b": Interrupted\n");
    stdfd::diag_bytes_in_handler(&m);
}

/// `int_handler`: `ul_sig_err (EXIT_FAILURE, "Interrupted")` -- the message,
/// then `_exit`, standard output unflushed.
extern "C" fn int_handler(_sig: i32) {
    sig_warn();
    #[cfg(unix)]
    libcall::process::exit_immediately(1);
}

/// `quit_handler`: the message, and on.
extern "C" fn quit_handler(_sig: i32) {
    sig_warn();
}

/// `usage`, to standard output.
fn usage(prog: &[u8], lastb: bool) -> Vec<u8> {
    let mut s = b"\nUsage:\n ".to_vec();
    s.extend_from_slice(prog);
    s.extend_from_slice(b" [options] [<username>...] [<tty>...]\n");
    s.extend_from_slice(b"\nShow a listing of last logged in users.\n");
    s.extend_from_slice(b"\nOptions:\n");
    s.extend_from_slice(b" -<number>            how many lines to show\n");
    s.extend_from_slice(b" -a, --hostlast       display hostnames in the last column\n");
    s.extend_from_slice(b" -d, --dns            translate the IP number back into a hostname\n");
    s.extend_from_slice(b" -f, --file <file>    use a specific file instead of ");
    s.extend_from_slice(if lastb { PATH_BTMP } else { PATH_WTMP });
    s.push(b'\n');
    s.extend_from_slice(b" -F, --fulltimes      print full login and logout times and dates\n");
    s.extend_from_slice(b" -i, --ip             display IP numbers in numbers-and-dots notation\n");
    s.extend_from_slice(b" -n, --limit <number> how many lines to show\n");
    s.extend_from_slice(b" -R, --nohostname     don't display the hostname field\n");
    s.extend_from_slice(b" -s, --since <time>   display the lines since the specified time\n");
    s.extend_from_slice(b" -t, --until <time>   display the lines until the specified time\n");
    s.extend_from_slice(b" -p, --present <time> display who were present at the specified time\n");
    s.extend_from_slice(b" -w, --fullnames      display full user and domain names\n");
    s.extend_from_slice(
        b" -x, --system         display system shutdown entries and run level changes\n",
    );
    s.extend_from_slice(
        b"     --time-format <format>  show timestamps in the specified <format>:\n",
    );
    s.extend_from_slice(b"                               notime|short|full|iso\n");
    s.push(b'\n');
    s.extend_from_slice(format!("{:<22}{}\n", " -h, --help", "display this help").as_bytes());
    s.extend_from_slice(format!("{:<22}{}\n", " -V, --version", "display version").as_bytes());
    s.extend_from_slice(b"\nFor more details see last(1).\n");
    s
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
    // `program_invocation_short_name`: what follows the last slash.
    let short = argv0
        .iter()
        .rposition(|&c| c == b'/')
        .map_or(argv0.as_slice(), |i| {
            argv0.get(i.saturating_add(1)..).unwrap_or_default()
        })
        .to_vec();
    // Before any handler can be installed; a second `set` cannot happen.
    drop(PROGRAM_NAME.set(shown(&short)));
    let mut out = ulclosestream::Stdout::new(1);
    let status = match run(argv.get(1..).unwrap_or_default(), &argv0, &short, &mut out) {
        Ok(code) | Err(Die(code)) => code,
    };
    ExitCode::from(out.close(status, &short))
}

/// The long option whose `val` is `c` -- `option_to_longopt`.
fn option_to_longopt(c: i32) -> Option<&'static str> {
    LONG_VALS
        .iter()
        .position(|&v| v == c)
        .and_then(|i| LONG_OPTIONS.get(i))
        .map(|&(name, _)| name)
}

/// The `val` upstream's `getopt_long` returns for `opt`.
fn option_value(opt: &Opt<'_>) -> Option<i32> {
    match opt {
        Opt::Short(c, _) => Some(i32::from(*c)),
        Opt::Long(name, _) => LONG_OPTIONS
            .iter()
            .position(|&(n, _)| n == *name)
            .and_then(|i| LONG_VALS.get(i))
            .copied(),
        Opt::Operand(_) => None,
    }
}

/// `parse_timestamp (arg, &p)`, as `(time_t) (p / 1000000)`, or `invalid
/// time value`.
fn timestamp(diag: &Diag, zone: &localtime::Zone, arg: &[u8]) -> Result<i64, Die> {
    match ultimeutils::parse_timestamp(zone, wall_clock(), arg) {
        Some(p) => Ok(i64::try_from(p / 1_000_000).unwrap_or(i64::MAX)),
        None => {
            diag.warn(&[b"invalid time value \"", &shown(arg), b"\""], None);
            Err(Die(1))
        }
    }
}

/// Upstream's `main`, after `setlocale`.
#[allow(
    clippy::too_many_lines,
    reason = "upstream's main, kept in one piece so it can be read against it"
)]
fn run(
    argv: &[OsString],
    argv0: &[u8],
    short: &[u8],
    out: &mut ulclosestream::Stdout,
) -> Result<u8, Die> {
    let diag = Diag { prog: shown(short) };
    let zone = localtime::Zone::from_env();
    let mut ctl = Ctl {
        lastb: short == b"lastb",
        extended: false,
        showhost: true,
        altlist: false,
        usedns: false,
        useip: false,
        name_len: LAST_LOGIN_LEN,
        domain_len: LAST_DOMAIN_LEN,
        maxrecs: 0,
        show: None,
        boot_time: 0,
        since: 0,
        until: 0,
        present: 0,
        time_fmt: FMT_SHORT,
    };
    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut operands: Vec<Vec<u8>> = Vec::new();
    let mut excl_st = [0i32; EXCL.len()];
    let arg = |v: Option<OsString>| v.as_deref().map(os_bytes).unwrap_or_default().into_owned();

    for item in LAST.parse(argv, SHORT_OPTIONS, LONG_OPTIONS) {
        let opt = match item {
            Ok(o) => o,
            Err(e) => {
                // glibc's getopt names the program by `argv[0]` as given.
                let mut m = shown(argv0);
                m.extend_from_slice(b": ");
                m.extend_from_slice(e.sentence.as_bytes());
                m.push(b'\n');
                ulclosestream::stderr_write(&m);
                diag.try_help();
                return Err(Die(1));
            }
        };
        if let Some(c) = option_value(&opt)
            && let Some(msg) =
                ulstrutils::err_exclusive_options(c, &EXCL, &mut excl_st, option_to_longopt, short)
        {
            ulclosestream::stderr_write(msg.as_bytes());
            return Err(Die(1));
        }
        match opt {
            Opt::Short(b'h', _) | Opt::Long("help", _) => {
                out.write(&usage(&diag.prog, ctl.lastb));
                return Ok(0);
            }
            Opt::Short(b'V', _) | Opt::Long("version", _) => {
                let mut v = diag.prog.clone();
                v.extend_from_slice(b" from SlateOS coreutils 0.1.0\n");
                out.write(&v);
                return Ok(0);
            }
            Opt::Short(b'R', _) | Opt::Long("nohostname", _) => ctl.showhost = false,
            Opt::Short(b'x', _) | Opt::Long("system", _) => ctl.extended = true,
            Opt::Short(b'n', v) | Opt::Long("limit", v) => {
                let text = v.unwrap_or_default();
                let n = ulstrutils::ul_strtos32(&os_bytes(&text), 10).map_err(|e| {
                    let msg = ulstrutils::num_error_message("failed to parse number", &text, e);
                    diag.warn(&[msg.as_bytes()], None);
                    Die(1)
                })?;
                ctl.maxrecs = u32::from_ne_bytes(n.to_ne_bytes());
            }
            Opt::Short(b'f', v) | Opt::Long("file", v) => files.push(arg(v)),
            Opt::Short(b'd', _) | Opt::Long("dns", _) => ctl.usedns = true,
            Opt::Short(b'i', _) | Opt::Long("ip", _) => ctl.useip = true,
            Opt::Short(b'a', _) | Opt::Long("hostlast", _) => ctl.altlist = true,
            Opt::Short(b'F', _) | Opt::Long("fulltimes", _) => ctl.time_fmt = FMT_CTIME,
            Opt::Short(b'p', v) | Opt::Long("present", v) => {
                ctl.present = timestamp(&diag, &zone, &arg(v))?;
            }
            Opt::Short(b's', v) | Opt::Long("since", v) => {
                ctl.since = timestamp(&diag, &zone, &arg(v))?;
            }
            Opt::Short(b't', v) | Opt::Long("until", v) => {
                ctl.until = timestamp(&diag, &zone, &arg(v))?;
            }
            Opt::Short(b'w', _) | Opt::Long("fullnames", _) => {
                ctl.name_len = ctl.name_len.max(utmpfile::UT_USER_SIZE);
                ctl.domain_len = ctl.domain_len.max(utmpfile::UT_HOST_SIZE);
            }
            Opt::Short(d @ b'0'..=b'9', _) => {
                ctl.maxrecs = ctl
                    .maxrecs
                    .wrapping_mul(10)
                    .wrapping_add(u32::from(d.wrapping_sub(b'0')));
            }
            Opt::Long("time-format", v) => {
                let name = arg(v);
                match TIMEFMTS
                    .iter()
                    .position(|f| f.name.as_bytes() == name.as_slice())
                {
                    Some(k) => ctl.time_fmt = k,
                    None => {
                        diag.warn(&[b"unknown time format: ", &shown(&name)], None);
                        return Err(Die(1));
                    }
                }
            }
            Opt::Operand(v) => operands.push(os_bytes(v).into_owned()),
            Opt::Short(..) | Opt::Long(..) => {
                diag.try_help();
                return Err(Die(1));
            }
        }
    }

    if !operands.is_empty() {
        ctl.show = Some(operands);
    }
    if files.is_empty() {
        files.push(if ctl.lastb { PATH_BTMP } else { PATH_WTMP }.to_vec());
    }

    let mut last = Last {
        ctl,
        diag,
        zone,
        recsdone: 0,
        currentdate: 0,
        uread: Uread::new(),
        passwd: None,
        out,
    };
    for file in &files {
        if let Some(t) = boot_time() {
            last.ctl.boot_time = t;
        }
        last.process_wtmp_file(file)?;
    }
    Ok(0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn record(ty: i16, pid: i32, line: &[u8], user: &[u8], sec: i32) -> Vec<u8> {
        let mut r = vec![0u8; UTSIZE];
        r[0..2].copy_from_slice(&ty.to_le_bytes());
        r[4..8].copy_from_slice(&pid.to_le_bytes());
        r[8..8 + line.len()].copy_from_slice(line);
        r[44..44 + user.len()].copy_from_slice(user);
        r[340..344].copy_from_slice(&sec.to_le_bytes());
        r
    }

    /// A stream over `data` in a file of its own, and the file to remove.
    fn reader(data: &[u8], tag: &str) -> (StdioReader, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("last-test-{}-{tag}", std::process::id()));
        std::fs::write(&path, data).unwrap();
        let fp = StdioReader::from_file(std::fs::File::open(&path).unwrap());
        (fp, path)
    }

    fn quiet() -> Diag {
        Diag {
            prog: b"last".to_vec(),
        }
    }

    #[test]
    fn records_come_back_newest_first_across_chunk_boundaries() {
        // 50 records: 19200 bytes, so the first chunk read is the short one
        // at the end (2816 bytes: seven records and part of an eighth).
        let mut data = Vec::new();
        for k in 0..50 {
            data.extend(record(USER_PROCESS, k, b"pts/0", b"u", k));
        }
        let (mut fp, path) = reader(&data, "chunks");
        let mut u = Uread::new();
        assert_eq!(Uread::first(&mut fp).unwrap().pid(), 0);
        u.init(&mut fp, &quiet(), b"f");
        let mut seen = Vec::new();
        while let Some(r) = u.next(&mut fp, &quiet(), b"f") {
            seen.push(r.pid());
        }
        std::fs::remove_file(path).unwrap();
        assert_eq!(seen, (0..50).rev().collect::<Vec<_>>());
    }

    #[test]
    fn a_file_that_is_not_whole_records_is_read_misaligned() {
        let mut data = record(USER_PROCESS, 1, b"a", b"u", 1);
        data.extend_from_slice(&[0xee; 10]);
        let (mut fp, path) = reader(&data, "misaligned");
        let mut u = Uread::new();
        u.init(&mut fp, &quiet(), b"f");
        // The last 384 bytes: 374 of the record and the ten extra.
        let r = u.next(&mut fp, &quiet(), b"f").unwrap();
        assert_eq!(&r.0[UTSIZE - 10..], &[0xee; 10]);
        // Then nothing: the ten bytes left are not a record.
        assert!(u.next(&mut fp, &quiet(), b"f").is_none());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn trailing_blanks_go_and_so_may_the_first_character() {
        let mut s = b"root     pts/0   \t  \n".to_vec();
        trim_trailing_spaces(&mut s);
        assert_eq!(s, b"root     pts/0\n");
        // Upstream's loop stops on the first character without testing it:
        // a line of one character and blanks loses the character.
        let mut s = b"a    \n".to_vec();
        trim_trailing_spaces(&mut s);
        assert_eq!(s, b"\n");
        let mut s = b"    ".to_vec();
        trim_trailing_spaces(&mut s);
        assert_eq!(s, b"\n");
        let mut s = b"ab \n".to_vec();
        trim_trailing_spaces(&mut s);
        assert_eq!(s, b"ab\n");
    }

    #[test]
    fn basename_is_posixs() {
        assert_eq!(xpg_basename(b"/var/log/wtmp"), b"wtmp");
        assert_eq!(xpg_basename(b"dir/"), b"dir");
        assert_eq!(xpg_basename(b"a//b//"), b"b");
        assert_eq!(xpg_basename(b"///"), b"/");
        assert_eq!(xpg_basename(b"//"), b"/");
        assert_eq!(xpg_basename(b""), b".");
        assert_eq!(xpg_basename(b"wtmp"), b"wtmp");
    }

    #[test]
    fn times_in_each_format() {
        let utc = localtime::Zone::utc();
        let t = 1_700_000_000; // 2023-11-14 22:13:20 UTC, a Tuesday.
        assert_eq!(ctime(&utc, t), b"Tue Nov 14 22:13:20 2023");
        assert_eq!(time_formatter(&utc, TimeFmt::Hhmm, t, 32), b"22:13");
        assert_eq!(
            time_formatter(&utc, TimeFmt::Iso8601, t, 32),
            b"2023-11-14T22:13:20+00:00"
        );
        assert_eq!(ctime(&utc, 0), b"Thu Jan  1 00:00:00 1970");
        assert_eq!(time_formatter(&utc, TimeFmt::None, t, 32), b"");
    }

    #[test]
    fn a_field_is_cut_then_padded() {
        let mut out = Vec::new();
        put_field(&mut out, b"abcdefghij\0zz", 8, 8);
        assert_eq!(out, b"abcdefgh");
        out.clear();
        put_field(&mut out, b"ab", 4, 8);
        assert_eq!(out, b"ab  ");
        out.clear();
        put_field(&mut out, b"anything", 0, 0);
        assert_eq!(out, b"");
    }

    #[test]
    fn options_have_upstreams_values() {
        let long = |name| option_value(&Opt::Long(name, None));
        assert_eq!(long("fulltimes"), Some(i32::from(b'F')));
        assert_eq!(long("time-format"), Some(OPT_TIME_FORMAT));
        assert_eq!(long("limit"), Some(i32::from(b'n')));
        assert_eq!(option_to_longopt(OPT_TIME_FORMAT), Some("time-format"));
        assert_eq!(option_to_longopt(i32::from(b'F')), Some("fulltimes"));
        let mut st = [0; 1];
        let f = i32::from(b'F');
        assert!(
            ulstrutils::err_exclusive_options(f, &EXCL, &mut st, option_to_longopt, b"last")
                .is_none()
        );
        assert_eq!(
            ulstrutils::err_exclusive_options(
                OPT_TIME_FORMAT,
                &EXCL,
                &mut st,
                option_to_longopt,
                b"last"
            )
            .as_deref(),
            Some("last: mutually exclusive arguments: --fulltimes --time-format\n")
        );
    }
}
