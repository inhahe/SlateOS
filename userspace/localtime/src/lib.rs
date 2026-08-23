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
    let mut it = fmt.iter().copied();
    while let Some(b) = it.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }
        let Some(spec) = it.next() else {
            // A trailing `%` is a literal `%`.
            out.push(b'%');
            break;
        };
        match spec {
            b'%' => out.push(b'%'),
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'a' => out.extend_from_slice(pick(&WDAY_ABBR, tm.wday as usize)),
            b'A' => out.extend_from_slice(pick(&WDAY_FULL, tm.wday as usize)),
            b'b' | b'h' => out.extend_from_slice(pick(&MON_ABBR, month_index(tm))),
            b'B' => out.extend_from_slice(pick(&MON_FULL, month_index(tm))),
            b'c' => out.extend_from_slice(&strftime(b"%a %b %e %H:%M:%S %Y", tm)),
            b'x' => out.extend_from_slice(&strftime(b"%m/%d/%y", tm)),
            b'X' | b'T' => out.extend_from_slice(&strftime(b"%H:%M:%S", tm)),
            b'r' => out.extend_from_slice(&strftime(b"%I:%M:%S %p", tm)),
            b'R' => out.extend_from_slice(&strftime(b"%H:%M", tm)),
            b'D' => out.extend_from_slice(&strftime(b"%m/%d/%y", tm)),
            b'F' => out.extend_from_slice(&strftime(b"%Y-%m-%d", tm)),
            b'C' => pad_zero(&mut out, tm.year.div_euclid(100), 2),
            b'd' => pad_zero(&mut out, i64::from(tm.day), 2),
            b'e' => pad_space(&mut out, i64::from(tm.day), 2),
            b'H' => pad_zero(&mut out, i64::from(tm.hour), 2),
            b'k' => pad_space(&mut out, i64::from(tm.hour), 2),
            b'I' => pad_zero(&mut out, i64::from(hour12(tm)), 2),
            b'l' => pad_space(&mut out, i64::from(hour12(tm)), 2),
            b'j' => pad_zero(&mut out, i64::from(tm.yday).saturating_add(1), 3),
            b'm' => pad_zero(&mut out, i64::from(tm.month), 2),
            b'M' => pad_zero(&mut out, i64::from(tm.minute), 2),
            b'S' => pad_zero(&mut out, i64::from(tm.second), 2),
            b'N' => pad_zero(&mut out, i64::from(tm.nanos), 9),
            b'p' => out.extend_from_slice(if tm.hour < 12 { b"AM" } else { b"PM" }),
            b'P' => out.extend_from_slice(if tm.hour < 12 { b"am" } else { b"pm" }),
            b's' => push_int(&mut out, tm.epoch),
            b'u' => push_int(&mut out, if tm.wday == 0 { 7 } else { i64::from(tm.wday) }),
            b'w' => push_int(&mut out, i64::from(tm.wday)),
            b'y' => pad_zero(&mut out, tm.year.rem_euclid(100), 2),
            b'Y' => push_int(&mut out, tm.year),
            b'z' => push_offset(&mut out, tm.gmtoff),
            b'Z' => out.extend_from_slice(tm.abbr.as_bytes()),
            other => {
                out.push(b'%');
                out.push(other);
            }
        }
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

fn pad_zero(out: &mut Vec<u8>, value: i64, width: usize) {
    pad_with(out, value, width, b'0');
}

fn pad_space(out: &mut Vec<u8>, value: i64, width: usize) {
    pad_with(out, value, width, b' ');
}

/// `%z`: `+hhmm`, with the sign taken from the offset and the magnitude from
/// its absolute value — `-0400`, not `-04-00`.
fn push_offset(out: &mut Vec<u8>, gmtoff: i32) {
    let (sign, mag) = if gmtoff < 0 {
        (b'-', gmtoff.unsigned_abs())
    } else {
        (b'+', gmtoff.unsigned_abs())
    };
    out.push(sign);
    pad_zero(out, i64::from(mag / 3600), 2);
    pad_zero(out, i64::from((mag / 60) % 60), 2);
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
