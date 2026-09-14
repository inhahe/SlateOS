//! The local timezone, resolved once, and the calendar it implies.
//!
//! [`tzrules`] answers one question — *what offset is in force at this
//! instant?* — and stops there, deliberately: it is `no_std` and links into the
//! libc, so it can neither read a file nor allocate. Everything between that
//! answer and a printed timestamp was therefore left to the caller, and by the
//! time this crate was written three callers had each supplied it:
//!
//! | Caller | What it wrote privately |
//! |---|---|
//! | `userspace/oils` | `ShellZone`/`Zone`, `zoneinfo_path`, a `strftime` |
//! | `userspace/hwclock` | `TZ` parsing, no file support at all |
//! | `posix/src/tz.rs` | `resolve_tz_value`, for the libc's own `tzset` |
//!
//! That is the shape of every duplication this tree has found so far: not one
//! program doing it wrong, but several doing it *differently*, so that `date`,
//! `ls -l` and the shell's `\t` prompt can disagree about what time it is on
//! the same machine at the same instant. This crate is the one copy.
//!
//! # What "resolve `TZ`" actually means
//!
//! Four rules, all of which glibc applies and none of which is guessable:
//!
//! 1. **Unset or empty `TZ` is not UTC** — it is `/etc/localtime`, a TZif file.
//!    A program that answers UTC for an unset `TZ` prints the wrong hour on
//!    every desktop in the world, and prints it *silently*.
//! 2. **A leading `:` forces the file interpretation.** POSIX reserves that
//!    prefix for implementation-defined forms; every libc reads it as "the rest
//!    is a file name", and glibc still accepts `:EST5EDT`.
//! 3. **Otherwise a POSIX rule string is tried first, and a file only if that
//!    fails.** The order is observable, because `EST5EDT` is *both* a valid
//!    POSIX rule and a file in every zoneinfo tree — and the two do not agree,
//!    since the rule cannot know that the United States moved the start of
//!    daylight saving in 2007.
//! 4. **A zone name with a `..` component is refused.** `TZ` is inherited from
//!    whoever started the process, so without this check `TZ=../../etc/shadow`
//!    makes any program that prints a time open an arbitrary file and reveal,
//!    through whether the time changed, whether it parsed as TZif.
//!
//! # Why the calendar arithmetic lives here too
//!
//! Because [`Tm`] is what a `strftime` needs, and a `strftime` is what every
//! caller was really trying to write. Splitting the two would leave each caller
//! to turn "seconds east of Greenwich" into a day of the week — which is where
//! the off-by-one lives, since 1970-01-01 was a **Thursday** and the obvious
//! `days % 7` makes it a Sunday.

use std::path::{Path, PathBuf};

use tzrules::{Tz, TzFile, TzInfo, TzName};

/// Where a bare zone name is looked up when `TZDIR` says nothing.
pub const TZDIR_DEFAULT: &str = "/usr/share/zoneinfo";

/// The file an unset `TZ` means.
pub const LOCALTIME_PATH: &str = "/etc/localtime";

/// Largest zoneinfo file that will be read.
///
/// The biggest in tzdata is under 4 KiB, so this is generous. The cap is not an
/// optimisation: `TZ=/dev/zero` is a legal thing for a parent process to set,
/// and without a bound the first program to print a timestamp reads until it
/// runs out of memory.
const MAX_ZONEINFO_BYTES: usize = 64 * 1024;

/// The read limit: one byte past the cap, so an oversized file is refused
/// rather than truncated to a prefix that might still parse as TZif.
const ZONEINFO_READ_LIMIT: u64 = MAX_ZONEINFO_BYTES as u64 + 1;

/// An owned timezone: either a POSIX rule or the bytes of a zoneinfo file.
///
/// Owned rather than borrowed because a zoneinfo zone *is* the file's bytes —
/// [`TzFile`] reads the transition table out of them on every lookup rather
/// than copying it — so something has to hold them, and the something a
/// utility wants is a value it can keep for the length of a listing.
#[derive(Clone, Debug)]
pub struct Zone(Inner);

#[derive(Clone, Debug)]
enum Inner {
    /// A POSIX `TZ` rule string, or the UTC fallback.
    Posix(Tz),
    /// The bytes of a zoneinfo file, already known to parse as TZif.
    File(Vec<u8>),
}

impl Default for Zone {
    fn default() -> Self {
        Self::utc()
    }
}

impl Zone {
    /// The UTC zone, which is what every failure here falls back to.
    #[must_use]
    pub fn utc() -> Self {
        Self(Inner::Posix(Tz::UTC))
    }

    /// The zone this process is running in, from `TZ`, `TZDIR` and
    /// `/etc/localtime`.
    ///
    /// This is the call a utility wants. It reads the environment exactly once,
    /// which matters for more than speed: `ls -l` renders a timestamp per file,
    /// and re-resolving per file would open `/etc/localtime` once per line.
    #[must_use]
    pub fn from_env() -> Self {
        let tz = std::env::var_os("TZ");
        let dir = std::env::var_os("TZDIR")
            .and_then(|d| d.into_string().ok())
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| TZDIR_DEFAULT.to_string());
        // Bytes, not a `String`: `TZ` is environment data, and environment data
        // is not required to be text. A non-UTF-8 `TZ` names no zone, but it
        // must reach the "names no zone" path rather than panic on the way.
        let raw = tz.as_ref().map(|v| os_bytes(v));
        Self::resolve(raw.as_deref(), &dir, Path::new(LOCALTIME_PATH))
    }

    /// Resolve an explicit `TZ` value against an explicit zoneinfo tree.
    ///
    /// Split out from [`Zone::from_env`] so it can be tested without a process
    /// environment, and so a caller that keeps its own variables — a shell —
    /// can pass its own rather than the ones it happened to inherit.
    ///
    /// `None`, or an empty value, means "the machine's zone": `localtime` is
    /// read. See the module docs for why that is not UTC.
    #[must_use]
    pub fn resolve(tz: Option<&[u8]>, dir: &str, localtime: &Path) -> Self {
        let Some(value) = tz.filter(|v| !v.is_empty()) else {
            return Self::from_file(localtime);
        };
        if let Some(name) = value.strip_prefix(b":") {
            return Self::from_name(name, dir);
        }
        match Tz::parse(value) {
            Some(tz) => Self(Inner::Posix(tz)),
            None => Self::from_name(value, dir),
        }
    }

    /// Load the zoneinfo file `name` names under `dir`, falling back to UTC.
    #[must_use]
    pub fn from_name(name: &[u8], dir: &str) -> Self {
        match zoneinfo_path(name, dir) {
            Some(path) => Self::from_file(&path),
            None => Self::utc(),
        }
    }

    /// Read and validate a zoneinfo file, falling back to UTC.
    ///
    /// The bytes are parsed *here* so that a file which is not TZif never
    /// becomes a zone. Every later lookup then has a file it already knows
    /// parses, and cannot silently answer UTC halfway down a listing.
    #[must_use]
    pub fn from_file(path: &Path) -> Self {
        let Ok(file) = std::fs::File::open(path) else {
            return Self::utc();
        };
        let mut bytes = Vec::new();
        if std::io::Read::read_to_end(
            &mut std::io::Read::take(file, ZONEINFO_READ_LIMIT),
            &mut bytes,
        )
        .is_err()
            || bytes.len() > MAX_ZONEINFO_BYTES
            || TzFile::parse(&bytes).is_none()
        {
            return Self::utc();
        }
        Self(Inner::File(bytes))
    }

    /// The zone state in force at UTC instant `t`.
    #[must_use]
    pub fn lookup(&self, t: i64) -> TzInfo {
        match &self.0 {
            Inner::Posix(tz) => tz.lookup(t),
            // `from_file` only builds this arm from bytes that parsed, so the
            // fallback is unreachable; it exists so that rendering a timestamp
            // cannot panic.
            Inner::File(bytes) => {
                TzFile::parse(bytes).map_or_else(|| Tz::UTC.lookup(t), |f| f.lookup(t))
            }
        }
    }

    /// Break UTC instant `t` (plus `nanos`) down into this zone's calendar.
    #[must_use]
    pub fn local(&self, t: i64, nanos: u32) -> Tm {
        Tm::from_utc(t, nanos, self.lookup(t))
    }
}

/// Build the path of the zoneinfo file `name` names, or `None` for a name that
/// must not be resolved.
///
/// See rule 4 in the module docs for why `..` is refused rather than resolved.
#[must_use]
pub fn zoneinfo_path(name: &[u8], dir: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains(&0) {
        return None;
    }
    // A `..` *inside* a component is not a traversal; only a whole component is.
    if name.split(|&b| b == b'/').any(|part| part == b"..") {
        return None;
    }
    let text = std::str::from_utf8(name).ok()?;
    if text.starts_with('/') {
        return Some(PathBuf::from(text));
    }
    Some(Path::new(dir).join(text))
}

/// Broken-down local time: what `struct tm` carries, plus what it does not.
///
/// The two extras are the reason this is not `libc::tm`. `epoch` is the
/// unshifted instant, which `%s` needs and which cannot be recovered from the
/// fields once the offset has been folded in; `nanos` is the sub-second part,
/// which `ls --time-style=full-iso` prints and `struct tm` has nowhere to put.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tm {
    /// The full proleptic-Gregorian year — 2026, not 126.
    pub year: i64,
    /// 1–12.
    pub month: u32,
    /// 1–31.
    pub day: u32,
    /// 0–23.
    pub hour: u32,
    /// 0–59.
    pub minute: u32,
    /// 0–60; 60 never occurs here, since our clock does not carry leap seconds.
    pub second: u32,
    /// Days since Sunday, 0–6.
    pub wday: u32,
    /// Days since January 1st, 0–365.
    pub yday: u32,
    /// The unshifted UTC instant, for `%s`.
    pub epoch: i64,
    /// Nanoseconds within the second, for `%N`.
    pub nanos: u32,
    /// Seconds east of Greenwich, for `%z`.
    pub gmtoff: i32,
    /// Whether daylight saving was in force.
    pub is_dst: bool,
    /// The zone abbreviation, for `%Z`.
    pub abbr: TzName,
}

impl Tm {
    /// Break a UTC instant down into the calendar `info` describes.
    #[must_use]
    pub fn from_utc(t: i64, nanos: u32, info: TzInfo) -> Self {
        let local = t.saturating_add(i64::from(info.gmtoff));
        // Euclidean, not truncating: an instant before 1970 has a negative
        // `local`, and `-1 / 86400` is 0 while the day it falls in is -1.
        let days = local.div_euclid(86_400);
        let secs = local.rem_euclid(86_400);
        let (year, month, day) = tzrules::civil_from_days(days);
        Self {
            year,
            month,
            day,
            hour: (secs / 3600) as u32,
            minute: ((secs / 60) % 60) as u32,
            second: (secs % 60) as u32,
            // 1970-01-01 was a **Thursday**, so day 0 is weekday 4. `rem_euclid`
            // again, for the same reason, and because a date before 1970 is not
            // exotic — `ls -l` on a restored archive prints them.
            wday: (days.saturating_add(4).rem_euclid(7)) as u32,
            yday: days.saturating_sub(tzrules::days_from_civil(year, 1, 1)) as u32,
            epoch: t,
            nanos,
            gmtoff: info.gmtoff,
            is_dst: info.is_dst,
            abbr: info.name,
        }
    }
}

/// Abbreviated weekday names, C locale, indexed by [`Tm::wday`].
pub const WDAY_ABBR: [&[u8]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
/// Full weekday names, C locale.
pub const WDAY_FULL: [&[u8]; 7] = [
    b"Sunday",
    b"Monday",
    b"Tuesday",
    b"Wednesday",
    b"Thursday",
    b"Friday",
    b"Saturday",
];
/// Abbreviated month names, C locale, indexed by `month - 1`.
pub const MON_ABBR: [&[u8]; 12] = [
    b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec",
];
/// Full month names, C locale.
pub const MON_FULL: [&[u8]; 12] = [
    b"January",
    b"February",
    b"March",
    b"April",
    b"May",
    b"June",
    b"July",
    b"August",
    b"September",
    b"October",
    b"November",
    b"December",
];

/// `strftime` over bytes, in the C locale.
///
/// Bytes rather than text throughout, because the *format* is user input —
/// `ls --time-style=+…` and `date +…` both hand it straight through from argv —
/// and argv is not required to be UTF-8. A format holding a stray `0x80` must
/// come back with that byte intact rather than as `U+FFFD`.
///
/// An unrecognised specifier is emitted **literally, `%` included**, which is
/// what glibc does and is not merely lenient: `%q` is not an error anyone
/// reports, so a renderer that swallowed it would silently delete two
/// characters of the user's format.
#[must_use]
#[allow(
    clippy::too_many_lines,
    reason = "one arm per specifier; a table would hide which is which"
)]
pub fn strftime(fmt: &[u8], tm: &Tm) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len().saturating_add(16));
    let mut it = fmt.iter().copied().peekable();
    while let Some(b) = it.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }
        // Flags and width first, then the conversion. A `%` followed only
        // by flags is a trailing `%`, same as a bare one.
        let fs = read_spec(&mut it);
        let Some(spec) = it.next() else {
            out.push(b'%');
            break;
        };
        // Rendered aside so the case flags and the string width can be applied
        // to the whole conversion rather than to whatever `out` already held.
        let mut piece: Vec<u8> = Vec::new();
        // `%P` is already the flipped spelling of `%p`, so glibc treats `#`
        // on it as a no-op -- measured, `%#P` is `am` and not `AM`. Every
        // other string field flips.
        let fs = if spec == b'P' {
            Spec { swap: false, ..fs }
        } else {
            fs
        };
        match spec {
            b'%' => piece.push(b'%'),
            b'n' => piece.push(b'\n'),
            b't' => piece.push(b'\t'),
            b'a' => piece.extend_from_slice(pick(&WDAY_ABBR, tm.wday as usize)),
            b'A' => piece.extend_from_slice(pick(&WDAY_FULL, tm.wday as usize)),
            b'b' | b'h' => piece.extend_from_slice(pick(&MON_ABBR, month_index(tm))),
            b'B' => piece.extend_from_slice(pick(&MON_FULL, month_index(tm))),
            b'c' => piece.extend_from_slice(&strftime(b"%a %b %e %H:%M:%S %Y", tm)),
            b'x' => piece.extend_from_slice(&strftime(b"%m/%d/%y", tm)),
            b'X' | b'T' => piece.extend_from_slice(&strftime(b"%H:%M:%S", tm)),
            b'r' => piece.extend_from_slice(&strftime(b"%I:%M:%S %p", tm)),
            b'R' => piece.extend_from_slice(&strftime(b"%H:%M", tm)),
            b'D' => piece.extend_from_slice(&strftime(b"%m/%d/%y", tm)),
            b'F' => piece.extend_from_slice(&strftime(b"%Y-%m-%d", tm)),
            b'C' => pad_with(
                &mut piece,
                tm.year.div_euclid(100),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'd' => pad_with(
                &mut piece,
                i64::from(tm.day),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'e' => pad_with(
                &mut piece,
                i64::from(tm.day),
                fs.num_width(2),
                fs.num_pad(b' '),
            ),
            b'H' => pad_with(
                &mut piece,
                i64::from(tm.hour),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'k' => pad_with(
                &mut piece,
                i64::from(tm.hour),
                fs.num_width(2),
                fs.num_pad(b' '),
            ),
            b'I' => pad_with(
                &mut piece,
                i64::from(hour12(tm)),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'l' => pad_with(
                &mut piece,
                i64::from(hour12(tm)),
                fs.num_width(2),
                fs.num_pad(b' '),
            ),
            b'j' => pad_with(
                &mut piece,
                i64::from(tm.yday).saturating_add(1),
                fs.num_width(3),
                fs.num_pad(b'0'),
            ),
            b'm' => pad_with(
                &mut piece,
                i64::from(tm.month),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'M' => pad_with(
                &mut piece,
                i64::from(tm.minute),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'S' => pad_with(
                &mut piece,
                i64::from(tm.second),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'N' => pad_with(
                &mut piece,
                i64::from(tm.nanos),
                fs.num_width(9),
                fs.num_pad(b'0'),
            ),
            b'p' => piece.extend_from_slice(if tm.hour < 12 { b"AM" } else { b"PM" }),
            b'P' => piece.extend_from_slice(if tm.hour < 12 { b"am" } else { b"pm" }),
            b's' => push_int(&mut piece, tm.epoch),
            b'u' => push_int(&mut piece, i64::from(iso_wday(tm))),
            b'w' => push_int(&mut piece, i64::from(tm.wday)),
            b'U' => pad_with(
                &mut piece,
                i64::from(week_of_year(tm, 0)),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'W' => pad_with(
                &mut piece,
                i64::from(week_of_year(tm, 1)),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'V' => pad_with(
                &mut piece,
                i64::from(iso_week(tm).1),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'G' => push_int(&mut piece, iso_week(tm).0),
            b'g' => pad_with(
                &mut piece,
                iso_week(tm).0.rem_euclid(100),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'y' => pad_with(
                &mut piece,
                tm.year.rem_euclid(100),
                fs.num_width(2),
                fs.num_pad(b'0'),
            ),
            b'Y' => push_int(&mut piece, tm.year),
            b'z' => push_offset(&mut piece, tm.gmtoff, 0),
            // `%:z`, `%::z` and `%:::z` -- GNU's colon forms, which are a
            // MODIFIER on `z` rather than specifiers of their own, so they are
            // read here rather than in the match arm above.
            //
            // They are not decoration: `date -Iseconds` and
            // `date --rfc-3339=seconds` are both defined in terms of `%:z`, so
            // without this they emit a literal `%:z` where the zone should be.
            // That is how this was found -- `date -Iseconds` printed
            // `2001-09-09T01:46:40%:z`.
            b':' => {
                let mut colons: u8 = 1;
                let mut next = it.next();
                while next == Some(b':') && colons < 3 {
                    colons = colons.saturating_add(1);
                    next = it.next();
                }
                if next == Some(b'z') {
                    push_offset(&mut piece, tm.gmtoff, colons);
                } else {
                    // Not a zone directive after all. Emit what was consumed,
                    // unchanged, the way an unknown specifier is emitted.
                    piece.push(b'%');
                    out.extend(std::iter::repeat_n(b':', usize::from(colons)));
                    if let Some(c) = next {
                        piece.push(c);
                    }
                }
            }
            b'Z' => piece.extend_from_slice(tm.abbr.as_bytes()),
            other => {
                piece.push(b'%');
                piece.push(other);
            }
        }
        finish_piece(&mut piece, fs);
        out.extend_from_slice(&piece);
    }
    out
}

/// Index a name table without panicking on a value that cannot occur.
fn pick(table: &[&'static [u8]], index: usize) -> &'static [u8] {
    table.get(index).copied().unwrap_or(b"?")
}

fn month_index(tm: &Tm) -> usize {
    (tm.month as usize).saturating_sub(1)
}

/// The ISO weekday: Monday is 1 and Sunday is 7, where [`Tm::wday`] has Sunday
/// at 0.
fn iso_wday(tm: &Tm) -> u32 {
    if tm.wday == 0 { 7 } else { tm.wday }
}

/// `%U` (`first = 0`, weeks start on Sunday) and `%W` (`first = 1`, Monday).
///
/// Week 1 is the one containing the first `first`-day of the year; the days
/// before it are week 0. That is glibc's rule and it is *not* the ISO one —
/// `%U`, `%W` and `%V` can all three differ on the same date, which is why
/// there are three of them.
fn week_of_year(tm: &Tm, first: u32) -> u32 {
    // How far into its week this day sits, counting from `first`. The `+ 7`
    // keeps the subtraction above zero for a Sunday under `%W`; it is written
    // saturating only because every arithmetic operator in this crate is.
    let offset = tm.wday.saturating_add(7).saturating_sub(first) % 7;
    tm.yday.saturating_add(7).saturating_sub(offset) / 7
}

/// `%G` and `%V`: the ISO 8601 week-based year and week number.
///
/// A week belongs to the year that holds its Thursday, so the first days of
/// January can be week 52 or 53 of the *previous* year and the last days of
/// December can be week 1 of the next.
fn iso_week(tm: &Tm) -> (i64, u32) {
    let yday1 = i64::from(tm.yday).saturating_add(1);
    let week = yday1
        .saturating_sub(i64::from(iso_wday(tm)))
        .saturating_add(10)
        .div_euclid(7);
    if week < 1 {
        let prev = tm.year.saturating_sub(1);
        return (prev, weeks_in_year(prev));
    }
    let week = u32::try_from(week).unwrap_or(1);
    if week > weeks_in_year(tm.year) {
        return (tm.year.saturating_add(1), 1);
    }
    (tm.year, week)
}

/// 52 or 53, by the ISO rule: a year is long when it starts on a Thursday, or
/// when it is a leap year starting on a Wednesday.
///
/// Expressed the usual way, over `p(y)` — the weekday of 31 December of year
/// `y` counted from Sunday — because the two conditions above then collapse to
/// `p(y) == 4 || p(y-1) == 3` and no leap-year test is needed at all.
fn weeks_in_year(year: i64) -> u32 {
    let p = |y: i64| {
        y.saturating_add(y.div_euclid(4))
            .saturating_sub(y.div_euclid(100))
            .saturating_add(y.div_euclid(400))
            .rem_euclid(7)
    };
    if p(year) == 4 || p(year.saturating_sub(1)) == 3 {
        53
    } else {
        52
    }
}

/// The 12-hour clock's reading, where midnight and noon are both 12.
fn hour12(tm: &Tm) -> u32 {
    match tm.hour % 12 {
        0 => 12,
        h => h,
    }
}

fn push_int(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(value.to_string().as_bytes());
}

/// `value` right-aligned in `width`, padded with `fill`.
fn pad_with(out: &mut Vec<u8>, value: i64, width: usize, fill: u8) {
    let text = value.to_string();
    for _ in text.len()..width {
        out.push(fill);
    }
    out.extend_from_slice(text.as_bytes());
}

/// Zero-padded, for the offset fields that always are.
fn pad_zero(out: &mut Vec<u8>, value: i64, width: usize) {
    pad_with(out, value, width, b'0');
}

/// The flags and field width GNU allows between `%` and the conversion.
///
/// Measured against coreutils 9.4 with `date -d @1000000000 +'[%X]'`, because
/// two of these rules are not what an implementation would naturally do:
///
/// | directive | output | rule |
/// |---|---|---|
/// | `%d` `%-d` `%_d` `%0d` | `09` `9` ` 9` `09` | `-` none, `_` space, `0` zero |
/// | `%e` `%0e` | ` 9` `09` | `0` overrides a space-padded field |
/// | `%a` `%^a` `%#a` | `Sun` `SUN` `SUN` | `^` upper, `#` swap case |
/// | `%S` `%5S` `%-5S` `%_5S` | `40` `00040` `40` `   40` | `-` discards the width too |
/// | `%5a` `%10B` | `  Sun` ` September` | a STRING field pads with space |
/// | `%0a` `%_a` | `Sun` `Sun` | a pad flag does nothing to a string |
///
/// **`%1d` is `9`, not `09`.** The width REPLACES the field's default width;
/// it is not a minimum applied to the default rendering. Rendering `%d` as
/// `09` and then padding to 1 returns `09`, which is why the width has to
/// reach the number formatter rather than being applied afterwards.
///
/// **The last flag wins**: `%-0d` is `09` and `%0-d` is `9`.
#[derive(Clone, Copy, Default)]
struct Spec {
    /// `Some(b'0')` or `Some(b' ')` from an explicit flag.
    pad: Option<u8>,
    /// `-`: no padding at all, and the width is discarded with it.
    nopad: bool,
    width: Option<usize>,
    upper: bool,
    swap: bool,
}

impl Spec {
    /// The width a numeric conversion should render at, given its default.
    fn num_width(self, default: usize) -> usize {
        if self.nopad {
            0
        } else {
            self.width.unwrap_or(default)
        }
    }

    /// The character a numeric conversion should pad with, given its default.
    fn num_pad(self, default: u8) -> u8 {
        self.pad.unwrap_or(default)
    }
}

/// Read the flags and width after a `%`, leaving the iterator on the
/// conversion character.
fn read_spec<I: Iterator<Item = u8>>(it: &mut core::iter::Peekable<I>) -> Spec {
    let mut spec = Spec::default();
    while let Some(&c) = it.peek() {
        match c {
            // Last of a kind wins, which is why each simply overwrites.
            b'-' => {
                spec.nopad = true;
                spec.pad = None;
            }
            b'_' => {
                spec.pad = Some(b' ');
                spec.nopad = false;
            }
            b'0' => {
                spec.pad = Some(b'0');
                spec.nopad = false;
            }
            b'^' => spec.upper = true,
            b'#' => spec.swap = true,
            _ => break,
        }
        it.next();
    }
    // `0` is a flag before it is a digit, so the width is whatever digits
    // remain -- `%05S` is flag `0` and width `5`, not width `05`.
    let mut width: Option<usize> = None;
    while let Some(&c) = it.peek() {
        if !c.is_ascii_digit() {
            break;
        }
        let d = usize::from(c.wrapping_sub(b'0'));
        width = Some(width.unwrap_or(0).saturating_mul(10).saturating_add(d));
        it.next();
    }
    spec.width = width;
    spec
}

/// Apply `^`/`#` and pad a STRING conversion out to the requested width.
///
/// Numeric conversions have already been rendered at their width, so the pad
/// below is a no-op for them -- which is why one function can serve both.
fn finish_piece(piece: &mut Vec<u8>, spec: Spec) {
    if spec.upper {
        piece.make_ascii_uppercase();
    } else if spec.swap {
        // `#` flips the case of the FIELD, not of each character, and the
        // direction comes from the text. Measured:
        //
        //     %a Sun -> %#a SUN          %p AM -> %#p am
        //     %B September -> SEPTEMBER  %Z UTC -> %#Z utc
        //
        // A per-character swap gives `sUN` for the first, which is what the
        // obvious reading of "opposite case" produces and is wrong.
        if piece.iter().any(u8::is_ascii_lowercase) {
            piece.make_ascii_uppercase();
        } else {
            piece.make_ascii_lowercase();
        }
    }
    if spec.nopad {
        return;
    }
    if let Some(w) = spec.width {
        if piece.len() < w {
            let mut padded = vec![b' '; w.saturating_sub(piece.len())];
            padded.extend_from_slice(piece);
            *piece = padded;
        }
    }
}

/// `%z`: `+hhmm`, with the sign taken from the offset and the magnitude from
/// its absolute value — `-0400`, not `-04-00`.
/// `colons` selects GNU's four spellings of the UTC offset, measured against
/// coreutils 9.4 at `+00:00`:
///
/// | directive | output |
/// |---|---|
/// | `%z`     | `+0000` |
/// | `%:z`    | `+00:00` |
/// | `%::z`   | `+00:00:00` |
/// | `%:::z`  | `+00` — the *minimal* form, which drops trailing zero fields |
fn push_offset(out: &mut Vec<u8>, gmtoff: i32, colons: u8) {
    let (sign, mag) = if gmtoff < 0 {
        (b'-', gmtoff.unsigned_abs())
    } else {
        (b'+', gmtoff.unsigned_abs())
    };
    let (hh, mm, ss) = (mag / 3600, (mag / 60) % 60, mag % 60);
    out.push(sign);
    pad_zero(out, i64::from(hh), 2);
    match colons {
        0 => pad_zero(out, i64::from(mm), 2),
        1 => {
            out.push(b':');
            pad_zero(out, i64::from(mm), 2);
        }
        2 => {
            out.push(b':');
            pad_zero(out, i64::from(mm), 2);
            out.push(b':');
            pad_zero(out, i64::from(ss), 2);
        }
        // `%:::z` prints only as much as it needs to.
        _ => {
            if mm != 0 || ss != 0 {
                out.push(b':');
                pad_zero(out, i64::from(mm), 2);
                if ss != 0 {
                    out.push(b':');
                    pad_zero(out, i64::from(ss), 2);
                }
            }
        }
    }
}

/// An `OsStr`'s bytes, on both the host and the target.
#[cfg(unix)]
fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    std::os::unix::ffi::OsStrExt::as_bytes(s).to_vec()
}

/// An `OsStr`'s bytes on Windows, where the only lossless route out of the
/// UTF-16-ish encoding is through `to_string_lossy`.
///
/// This is the host build only; a `TZ` that is not UTF-16-representable cannot
/// be set on Windows in the first place, so nothing is lost that could exist.
#[cfg(not(unix))]
fn os_bytes(s: &std::ffi::OsStr) -> Vec<u8> {
    s.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn utc_tm(t: i64) -> Tm {
        Zone::utc().local(t, 0)
    }

    fn fmt(f: &str, tm: &Tm) -> String {
        String::from_utf8(strftime(f.as_bytes(), tm)).unwrap()
    }

    /// The flags and widths GNU allows between `%` and the conversion.
    ///
    /// Every expectation here was run against coreutils 9.4 as
    /// `date -d @1000000000 +'[%X]'` with TZ=UTC, not recalled. Before this,
    /// `strftime` read exactly one byte after the `%`, so every one of these
    /// came out as the literal text of the directive.
    #[test]
    fn strftime_honours_the_pad_flags() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%d", &tm), "09");
        assert_eq!(fmt("%-d", &tm), "9", "`-` means no padding");
        assert_eq!(fmt("%_d", &tm), " 9", "`_` means space padding");
        assert_eq!(fmt("%0d", &tm), "09", "`0` means zero padding");
        // `0` overrides a field whose default pad is a space, and `_` a field
        // whose default is a zero -- the flag wins either way round.
        assert_eq!(fmt("%e", &tm), " 9");
        assert_eq!(fmt("%0e", &tm), "09");
        assert_eq!(fmt("%S", &tm), "40");
        assert_eq!(fmt("%_5S", &tm), "   40");
    }

    /// A width REPLACES the field's default width; it is not a minimum.
    ///
    /// This is the rule an implementation gets wrong by building the obvious
    /// way: render `%d` as `09` and then pad to the requested width, and `%1d`
    /// comes back `09`. GNU says `9`.
    #[test]
    fn strftime_width_replaces_the_default_width() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(
            fmt("%1d", &tm),
            "9",
            "a width of 1 is narrower than %d's default"
        );
        assert_eq!(fmt("%2d", &tm), "09");
        assert_eq!(fmt("%5S", &tm), "00040");
        assert_eq!(fmt("%3H", &tm), "001");
        assert_eq!(fmt("%12N", &tm), "000000000000");
        // `-` discards the width along with the padding.
        assert_eq!(fmt("%-5S", &tm), "40");
    }

    /// The LAST flag wins, so the two orderings differ.
    #[test]
    fn strftime_takes_the_last_flag() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%-0d", &tm), "09");
        assert_eq!(fmt("%0-d", &tm), "9");
    }

    /// A string field pads with SPACE whatever the pad flag says, and `^`/`#`
    /// change its case.
    #[test]
    fn strftime_cases_and_pads_a_string_field() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%a", &tm), "Sun");
        assert_eq!(fmt("%5a", &tm), "  Sun", "strings pad with space, not zero");
        assert_eq!(fmt("%10B", &tm), " September");
        assert_eq!(
            fmt("%0a", &tm),
            "Sun",
            "a pad flag does nothing to a string"
        );
        assert_eq!(fmt("%^a", &tm), "SUN");
        assert_eq!(fmt("%^B", &tm), "SEPTEMBER");
    }

    /// `#` flips the case of the FIELD, with the direction taken from the text.
    ///
    /// A per-character swap -- the obvious reading of "opposite case" -- gives
    /// `sUN` for the first of these, and is wrong. `%P` is exempt because it is
    /// already the flipped spelling of `%p`.
    #[test]
    fn strftime_hash_flips_the_whole_field() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%#a", &tm), "SUN", "not sUN");
        assert_eq!(fmt("%#B", &tm), "SEPTEMBER");
        assert_eq!(fmt("%#p", &tm), "am", "an upper-case field goes down");
        assert_eq!(fmt("%#Z", &tm), "utc");
        assert_eq!(fmt("%#P", &tm), "am", "%P is already the flipped spelling");
        assert_eq!(fmt("%^p", &tm), "AM");
    }

    /// An unrecognised specifier still comes back verbatim, flags and all --
    /// the behaviour the flag parsing must not have broken.
    #[test]
    fn strftime_still_passes_an_unknown_directive_through() {
        let tm = utc_tm(1_000_000_000);
        assert_eq!(fmt("%q", &tm), "%q");
        assert_eq!(fmt("%%", &tm), "%");
        assert_eq!(fmt("100%", &tm), "100%");
        assert_eq!(
            fmt("%Y-%m-%d", &tm),
            "2001-09-09",
            "plain formats unchanged"
        );
    }

    #[test]
    fn the_epoch_was_a_thursday() {
        // The single most-often-wrong constant in a hand-written calendar: the
        // obvious `days % 7` makes day zero a Sunday.
        let tm = utc_tm(0);
        assert_eq!(tm.wday, 4);
        assert_eq!(fmt("%a %Y-%m-%d %H:%M:%S", &tm), "Thu 1970-01-01 00:00:00");
    }

    #[test]
    fn a_moment_before_the_epoch_is_the_day_before() {
        // Truncating division would put this on 1970-01-01 at hour 0.
        let tm = utc_tm(-1);
        assert_eq!((tm.year, tm.month, tm.day), (1969, 12, 31));
        assert_eq!((tm.hour, tm.minute, tm.second), (23, 59, 59));
        assert_eq!(tm.wday, 3);
    }

    #[test]
    fn yday_counts_from_zero_and_j_counts_from_one() {
        let tm = utc_tm(0);
        assert_eq!(tm.yday, 0);
        assert_eq!(fmt("%j", &tm), "001");
        // 2024 is a leap year, so December 31st is day 365 (0-based).
        let dec31 = tzrules::days_from_civil(2024, 12, 31) * 86_400;
        assert_eq!(utc_tm(dec31).yday, 365);
        assert_eq!(fmt("%j", &utc_tm(dec31)), "366");
    }

    #[test]
    fn the_padding_characters_differ_between_d_and_e() {
        let tm = utc_tm(tzrules::days_from_civil(2019, 3, 4) * 86_400);
        assert_eq!(fmt("%d", &tm), "04");
        assert_eq!(fmt("%e", &tm), " 4");
        // This pair is exactly what `ls -l`'s two locale formats are built
        // from: `Mar  4  2019` needs `%e`, and `%d` would print `Mar 04`.
        assert_eq!(fmt("%b %e  %Y", &tm), "Mar  4  2019");
    }

    #[test]
    fn twelve_hour_midnight_and_noon_are_both_twelve() {
        let midnight = utc_tm(0);
        assert_eq!(fmt("%I %p", &midnight), "12 AM");
        assert_eq!(fmt("%l %p", &midnight), "12 AM");
        let noon = utc_tm(12 * 3600);
        assert_eq!(fmt("%I %p", &noon), "12 PM");
        let one_pm = utc_tm(13 * 3600);
        assert_eq!(fmt("%I %p", &one_pm), "01 PM");
        assert_eq!(fmt("%l %p", &one_pm), " 1 PM");
    }

    #[test]
    fn an_unknown_specifier_survives_whole() {
        // glibc emits it literally. A renderer that dropped it would silently
        // delete two characters of a format the user typed.
        let tm = utc_tm(0);
        assert_eq!(fmt("%q", &tm), "%q");
        assert_eq!(fmt("a%", &tm), "a%");
        assert_eq!(fmt("%%", &tm), "%");
    }

    #[test]
    fn n_is_a_newline_in_the_format_not_a_format_of_its_own() {
        // The distinction `ls --time-style=+%Y%n%H` turns on: `%n` reaches
        // `strftime` as two bytes and leaves as one newline.
        let tm = utc_tm(0);
        assert_eq!(fmt("%Y%n%H", &tm), "1970\n00");
    }

    #[test]
    fn the_three_week_numbers_are_three_different_questions() {
        // Every row measured from GNU `date -u -d DATE +'%U %W %V %G %g %u'`
        // under `LC_ALL=C TZ=UTC`. The dates are the ones where the three
        // definitions come apart: a year whose first days belong to the
        // previous ISO year, a year with 53 ISO weeks, and a Sunday — which is
        // week 0 for `%W` and week 1 for `%U` in the same breath.
        for &(y, m, d, u, w, v, g, g2, iso_dow) in &[
            (1970, 1, 1, "00", "00", "01", "1970", "70", "4"),
            (2000, 1, 1, "00", "00", "52", "1999", "99", "6"),
            (2015, 12, 28, "52", "52", "53", "2015", "15", "1"),
            (2016, 1, 3, "01", "00", "53", "2015", "15", "7"),
            (2019, 12, 30, "52", "52", "01", "2020", "20", "1"),
            (2020, 12, 31, "52", "52", "53", "2020", "20", "4"),
            (2021, 1, 1, "00", "00", "53", "2020", "20", "5"),
            (2021, 1, 3, "01", "00", "53", "2020", "20", "7"),
            (2024, 12, 29, "52", "52", "52", "2024", "24", "7"),
            (2026, 8, 23, "34", "33", "34", "2026", "26", "7"),
        ] {
            let tm = utc_tm(tzrules::days_from_civil(y, m, d) * 86_400);
            let got = fmt("%U %W %V %G %g %u", &tm);
            assert_eq!(
                got,
                format!("{u} {w} {v} {g} {g2} {iso_dow}"),
                "{y:04}-{m:02}-{d:02}"
            );
        }
    }

    #[test]
    fn a_long_iso_year_is_the_one_that_gains_a_week() {
        // 53 when the year starts on a Thursday (2015, 2020) or is a leap year
        // starting on a Wednesday (2020 again, 1992); 52 otherwise. The
        // constant this guards is the `p(y-1) == 3` half, which a naive
        // implementation drops because it never fires on a non-leap year.
        for &(y, n) in &[
            (1992, 53),
            (2015, 53),
            (2016, 52),
            (2020, 53),
            (2021, 52),
            (2024, 52),
            (2026, 53),
        ] {
            assert_eq!(weeks_in_year(y), n, "{y}");
        }
    }

    #[test]
    fn nanoseconds_are_nine_digits() {
        let tm = Zone::utc().local(0, 42);
        assert_eq!(fmt("%N", &tm), "000000042");
    }

    #[test]
    fn the_offset_sign_does_not_leak_into_the_minutes() {
        // `-0400`, never `-04-00`. The bug this guards is dividing a negative
        // offset before taking its magnitude.
        let info = TzInfo {
            gmtoff: -4 * 3600 - 30 * 60,
            is_dst: false,
            name: TzName::UTC,
        };
        let tm = Tm::from_utc(0, 0, info);
        assert_eq!(fmt("%z", &tm), "-0430");
        let east = Tm::from_utc(
            0,
            0,
            TzInfo {
                gmtoff: 5 * 3600 + 45 * 60,
                is_dst: false,
                name: TzName::UTC,
            },
        );
        assert_eq!(fmt("%z", &east), "+0545");
    }

    #[test]
    fn s_is_the_unshifted_instant() {
        // Not the local clock: the offset is folded into the fields and must
        // not be folded into `%s` as well.
        let info = TzInfo {
            gmtoff: -5 * 3600,
            is_dst: false,
            name: TzName::UTC,
        };
        let tm = Tm::from_utc(1_551_693_967, 0, info);
        assert_eq!(fmt("%s", &tm), "1551693967");
        assert_eq!(fmt("%Y-%m-%d %H:%M", &tm), "2019-03-04 05:06");
    }

    #[test]
    fn an_empty_or_absent_tz_is_the_machines_zone_not_utc() {
        // The rule that is wrong in the obvious implementation, and wrong
        // silently: an unset `TZ` means `/etc/localtime`, not Greenwich. Here
        // the file does not exist, so the fallback is UTC — what is being
        // asserted is that it was *looked for*.
        let missing = Path::new("/nonexistent-localtime-for-this-test");
        assert!(matches!(
            Zone::resolve(None, TZDIR_DEFAULT, missing).0,
            Inner::Posix(_)
        ));
        assert!(matches!(
            Zone::resolve(Some(b""), TZDIR_DEFAULT, missing).0,
            Inner::Posix(_)
        ));
    }

    #[test]
    fn a_posix_rule_beats_a_file_of_the_same_name() {
        // `EST5EDT` is both. glibc tries the rule first, and so do we — which
        // is observable, because the rule does not know about 2007.
        let zone = Zone::resolve(Some(b"EST5EDT"), TZDIR_DEFAULT, Path::new(LOCALTIME_PATH));
        assert!(matches!(zone.0, Inner::Posix(_)));
        // Midsummer: EDT, four hours west.
        let summer = zone.lookup(tzrules::days_from_civil(2020, 7, 1) * 86_400);
        assert_eq!(summer.gmtoff, -4 * 3600);
        assert!(summer.is_dst);
    }

    #[test]
    fn a_leading_colon_forces_the_file_reading() {
        // `:EST5EDT` is a *file* name even though the rest parses as a rule.
        // The file will not be there under this test's directory, so the
        // observable is that we did not end up with the rule's offsets.
        let zone = Zone::resolve(
            Some(b":EST5EDT"),
            "/nonexistent-zoneinfo-dir",
            Path::new(LOCALTIME_PATH),
        );
        assert_eq!(zone.lookup(0).gmtoff, 0);
    }

    #[test]
    fn a_dotdot_component_is_refused_but_a_dotdot_inside_a_name_is_not() {
        assert!(zoneinfo_path(b"../../etc/shadow", TZDIR_DEFAULT).is_none());
        assert!(zoneinfo_path(b"..", TZDIR_DEFAULT).is_none());
        assert!(zoneinfo_path(b"America/..", TZDIR_DEFAULT).is_none());
        // Not a traversal: `..` has to be a whole component.
        assert!(zoneinfo_path(b"a..b", TZDIR_DEFAULT).is_some());
        assert!(zoneinfo_path(b"", TZDIR_DEFAULT).is_none());
        assert!(zoneinfo_path(b"a\0b", TZDIR_DEFAULT).is_none());
    }

    #[test]
    fn an_absolute_zone_name_is_used_as_given() {
        // `TZ=/etc/localtime` is a thing people set, and joining it onto TZDIR
        // would look for `/usr/share/zoneinfo/etc/localtime`.
        assert_eq!(
            zoneinfo_path(b"/etc/localtime", TZDIR_DEFAULT).unwrap(),
            PathBuf::from("/etc/localtime")
        );
    }

    #[test]
    fn a_file_that_is_not_tzif_is_not_a_zone() {
        // Rejected at load, so a listing cannot switch to UTC halfway down.
        let zone = Zone::from_file(Path::new("Cargo.toml"));
        assert!(matches!(zone.0, Inner::Posix(_)));
        assert_eq!(zone.lookup(0).gmtoff, 0);
    }
}
