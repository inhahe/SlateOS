//! gnulib's `parse-datetime`: the date language of `date -d`, `touch -d` and
//! `find -newermt`.
//!
//! ```text
//! 2021-06-15 12:00:00 +0530      @1623758400.5        next Tuesday
//! TZ="Asia/Tokyo" 9am tomorrow   17 Jun 1992          3 days ago 14:00
//! ```
//!
//! # A port, not a reimplementation
//!
//! The language has no specification beyond its implementation. The manual
//! describes the common forms; what an uncommon one means — `1 day 2 hours
//! ago`, `12:30 -5`, `Tue 2021-06-15`, `EST` in July, `2021-01-31 1 month` —
//! is whatever `parse-datetime.y` does with it, and a reimplementation that
//! agreed on every documented form would still disagree on those. `date.rs`
//! had such a reimplementation, grown form by form against a probe of GNU's
//! behaviour; every form it accepted was measured, and it still could not say
//! `12:30 -5` (GNU reads the `-5` as a zone), because the rule that decides
//! that is a shift/reduce conflict in a Bison grammar, not a rule anyone wrote
//! down.
//!
//! So this is upstream's parser, three ways:
//!
//! * [`tables`] is the LALR(1) automaton **Bison built** for coreutils 9.4,
//!   copied out of the release tarball's `lib/parse-datetime.c` by
//!   `gen_tables.py` — not rebuilt, because the conflicts and the default
//!   reductions are properties of those tables;
//! * [`grammar`] is `yacc.c`'s driver and the grammar's semantic actions,
//!   rule for rule;
//! * [`lex`] and this module are the hand-written C around them — the lexer
//!   and its word tables, and `parse_datetime_body`, which turns what the
//!   parser collected into an instant through `mktime`.
//!
//! `mktime` is the other half of the answer, and it is glibc's too:
//! [`localtime::Zone::mktime_internal`] is a port of the same `mktime.c`,
//! including its process-wide guess (see [`localtime::with_mktime_offset`]),
//! because the result for a skipped or repeated local time — and whether the
//! string is refused at all — is decided there.
//!
//! # Where C's integer rules leak through
//!
//! Upstream narrows `intmax_t` into `int` in a few assignments
//! (`tm.tm_min = pc.minutes`), and those narrowings are kept (as `as i32`,
//! which wraps as gcc does): `10:4294967326` really is 10:30 to GNU. Everything
//! upstream does with `ckd_*` is checked here too, and fails the same way.
//!
//! # `--debug`
//!
//! Every diagnostic upstream prints under `PARSE_DATETIME_DEBUG` is here,
//! formatted the same way, prefixed `date: ` as upstream hard-codes it. They
//! go to the writer the caller supplies, so `date --debug` can hand in stderr
//! and a test can hand in a buffer.

mod grammar;
mod lex;
mod tables;

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use localtime::{StructTm, Zone, with_mktime_offset};

use lex::{MER_24, MER_AM, MER_PM, c_isspace};

/// Nanoseconds per second.
const BILLION: i32 = 1_000_000_000;
/// Digits in a nanosecond count.
const LOG10_BILLION: i32 = 9;
/// `HOUR (1)`.
const HOUR: i64 = 60 * 60;
/// `tm_year` counts from 1900.
const TM_YEAR_BASE: i64 = 1900;

/// A `struct timespec`: seconds since the epoch, and nanoseconds, `0 <=
/// tv_nsec < 1_000_000_000` — positive even when `tv_sec` is negative.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Timespec {
    /// Seconds since 1970-01-01 00:00:00 UTC.
    pub tv_sec: i64,
    /// Nanoseconds past `tv_sec`.
    pub tv_nsec: i32,
}

impl Timespec {
    /// The current time, as upstream's `gettime` reads it.
    #[must_use]
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    /// A [`SystemTime`] as a `timespec`, saturating only past the ends of
    /// `i64` seconds, which no `SystemTime` on a 64-bit `time_t` reaches.
    #[must_use]
    pub fn from_system_time(t: SystemTime) -> Self {
        match t.duration_since(UNIX_EPOCH) {
            Ok(d) => Self {
                tv_sec: i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i32::try_from(d.subsec_nanos()).unwrap_or(0),
            },
            Err(e) => {
                // Before the epoch: -1.25s is { -2, 750_000_000 }. The whole
                // seconds are `0 - secs` exactly: 2^63 back is `i64::MIN`,
                // one past what negating an `i64` can reach.
                let d = e.duration();
                let whole = 0i64.checked_sub_unsigned(d.as_secs()).unwrap_or(i64::MIN);
                let nanos = i32::try_from(d.subsec_nanos()).unwrap_or(0);
                if nanos == 0 {
                    Self {
                        tv_sec: whole,
                        tv_nsec: 0,
                    }
                } else {
                    Self {
                        tv_sec: whole.saturating_sub(1),
                        tv_nsec: BILLION.saturating_sub(nanos),
                    }
                }
            }
        }
    }

    /// The instant as a [`SystemTime`], or `None` if the platform cannot
    /// represent it.
    #[must_use]
    pub fn to_system_time(self) -> Option<SystemTime> {
        let nanos = u32::try_from(self.tv_nsec).ok()?;
        // `unsigned_abs`, not negation: `i64::MIN` seconds is a real instant
        // (`@-9223372036854775808` parses), and has no positive `i64`.
        let whole = std::time::Duration::from_secs(self.tv_sec.unsigned_abs());
        let at = if self.tv_sec >= 0 {
            UNIX_EPOCH.checked_add(whole)?
        } else {
            UNIX_EPOCH.checked_sub(whole)?
        };
        at.checked_add(std::time::Duration::new(0, nanos))
    }
}

/// An integer and the number of digits it was written with: upstream's
/// `textint`. The digit count is part of the meaning — `2021/06/15` and
/// `06/15/21` are told apart by it, and so are `69` and `1969`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct TextInt {
    negative: bool,
    value: i64,
    digits: i64,
}

/// A relative displacement, field by field: upstream's `relative_time`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct RelativeTime {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minutes: i64,
    seconds: i64,
    ns: i32,
}

/// One of the local zone's own abbreviations, which the lexer tries before
/// upstream's table.
struct LocalZone {
    name: Vec<u8>,
    /// The `tm_isdst` it implies, or -1 where standard and daylight time use
    /// the same abbreviation and so it implies neither.
    isdst: i64,
}

/// Everything the parser reads and writes: upstream's `parser_control`.
struct ParserControl<'a, 'd> {
    /// The string, from the first byte after any `TZ="…"` prefix.
    input: &'a [u8],
    /// Upstream's `pc->input`: how far the lexer has read.
    pos: usize,

    /// N, if this is the Nth Tuesday.
    day_ordinal: i64,
    /// Day of the week; Sunday is 0.
    day_number: i32,
    /// `tm_isdst` for the local zone.
    local_isdst: i32,
    /// Seconds east of UTC.
    time_zone: i32,
    /// `MER_AM`, `MER_PM` or `MER_24`.
    meridian: i64,

    year: TextInt,
    month: i64,
    day: i64,
    hour: i64,
    minutes: i64,
    seconds: Timespec,

    rel: RelativeTime,

    timespec_seen: bool,
    rels_seen: bool,
    dates_seen: i64,
    days_seen: i64,
    j_zones_seen: i64,
    local_zones_seen: i64,
    dsts_seen: i64,
    times_seen: i64,
    zones_seen: i64,
    year_seen: bool,

    /// Where `--debug` output goes; `None` is upstream's debugging off.
    debug: Option<&'d mut dyn Write>,
    debug_dates_seen: bool,
    debug_days_seen: bool,
    debug_local_zones_seen: bool,
    debug_times_seen: bool,
    debug_zones_seen: bool,
    debug_year_seen: bool,
    debug_ordinal_day_seen: bool,

    local_time_zone_table: Vec<LocalZone>,
}

impl ParserControl<'_, '_> {
    /// `debugging (pc)`.
    fn debugging(&self) -> bool {
        self.debug.is_some()
    }

    /// Write `bytes` to the debug stream, if there is one — `fprintf
    /// (stderr, …)`.
    fn dbg_raw(&mut self, bytes: &[u8]) {
        if let Some(out) = self.debug.as_mut() {
            // Upstream ignores a failed write to stderr, as every C program
            // that reports to it does; there is nowhere left to report it.
            let _ = out.write_all(bytes);
        }
    }

    /// `dbg_printf`: a line of debug output, prefixed `date: ` — which
    /// upstream hard-codes whichever program is running.
    fn dbg_printf(&mut self, msg: &[u8]) {
        self.dbg_raw(b"date: ");
        self.dbg_raw(msg);
    }

    /// `str_days`: "last wed", "this tues", "thu".
    fn str_days(&self) -> String {
        // Upstream's own spelling, "eight" included.
        const ORDINAL_VALUES: [&str; 14] = [
            "last",
            "this",
            "next/first",
            "(SECOND)",
            "third",
            "fourth",
            "fifth",
            "sixth",
            "seventh",
            "eight",
            "ninth",
            "tenth",
            "eleventh",
            "twelfth",
        ];
        const DAYS_VALUES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

        let mut buffer = String::new();
        if self.debug_ordinal_day_seen {
            let named = self
                .day_ordinal
                .checked_add(1)
                .and_then(|i| usize::try_from(i).ok())
                .and_then(|i| ORDINAL_VALUES.get(i));
            match named {
                Some(word) => buffer.push_str(word),
                None => buffer.push_str(&self.day_ordinal.to_string()),
            }
        }
        if let Some(day) = usize::try_from(self.day_number)
            .ok()
            .and_then(|i| DAYS_VALUES.get(i))
        {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(day);
        }
        buffer
    }

    /// `debug_print_current_time`: report the parts newly seen.
    fn debug_print_current_time(&mut self, item: &str) {
        if !self.debugging() {
            return;
        }
        let mut line = format!("parsed {item} part: ");
        let mut space = false;

        if self.dates_seen != 0 && !self.debug_dates_seen {
            line.push_str(&format!(
                "(Y-M-D) {:04}-{:02}-{:02}",
                self.year.value, self.month, self.day
            ));
            self.debug_dates_seen = true;
            space = true;
        }

        if self.year_seen != self.debug_year_seen {
            if space {
                line.push(' ');
            }
            line.push_str(&format!("year: {:04}", self.year.value));
            self.debug_year_seen = self.year_seen;
            space = true;
        }

        if self.times_seen != 0 && !self.debug_times_seen {
            if space {
                line.push(' ');
            }
            line.push_str(&format!(
                "{:02}:{:02}:{:02}",
                self.hour, self.minutes, self.seconds.tv_sec
            ));
            if self.seconds.tv_nsec != 0 {
                line.push_str(&format!(".{:09}", self.seconds.tv_nsec));
            }
            if self.meridian == MER_PM {
                line.push_str("pm");
            }
            self.debug_times_seen = true;
            space = true;
        }

        if self.days_seen != 0 && !self.debug_days_seen {
            if space {
                line.push(' ');
            }
            line.push_str(&format!(
                "{} (day ordinal={} number={})",
                self.str_days(),
                self.day_ordinal,
                self.day_number
            ));
            self.debug_days_seen = true;
            space = true;
        }

        // A local zone changes only the DST setting, not the offset.
        if self.local_zones_seen != 0 && !self.debug_local_zones_seen {
            if space {
                line.push(' ');
            }
            line.push_str(&format!(
                "isdst={}{}",
                self.local_isdst,
                if self.dsts_seen != 0 { " DST" } else { "" }
            ));
            self.debug_local_zones_seen = true;
            space = true;
        }

        if self.zones_seen != 0 && !self.debug_zones_seen {
            if space {
                line.push(' ');
            }
            line.push_str(&format!("UTC{}", time_zone_str(self.time_zone)));
            self.debug_zones_seen = true;
            space = true;
        }

        if self.timespec_seen {
            if space {
                line.push(' ');
            }
            line.push_str(&format!("number of seconds: {}", self.seconds.tv_sec));
        }

        line.push('\n');
        self.dbg_printf(line.as_bytes());
    }

    /// `debug_print_relative_time`.
    fn debug_print_relative_time(&mut self, item: &str) {
        if !self.debugging() {
            return;
        }
        let mut line = format!("parsed {item} part: ");
        let rel = self.rel;
        if rel == RelativeTime::default() {
            // this/today/now
            line.push_str("today/this/now\n");
            self.dbg_printf(line.as_bytes());
            return;
        }
        let mut space = false;
        for (value, name) in [
            (rel.year, "year(s)"),
            (rel.month, "month(s)"),
            (rel.day, "day(s)"),
            (rel.hour, "hour(s)"),
            (rel.minutes, "minutes"),
            (rel.seconds, "seconds"),
            (i64::from(rel.ns), "nanoseconds"),
        ] {
            // print_rel_part
            if value != 0 {
                if space {
                    line.push(' ');
                }
                line.push_str(&format!("{value:+} {name}"));
                space = true;
            }
        }
        line.push('\n');
        self.dbg_printf(line.as_bytes());
    }
}

/// `digits_to_date_time`: a bare number — `YYYYMMDD`, `YYMMDD`, `HHMM`, `HH`,
/// or, after a date, a year.
fn digits_to_date_time(pc: &mut ParserControl<'_, '_>, text_int: TextInt) {
    if pc.dates_seen != 0
        && pc.year.digits == 0
        && !pc.rels_seen
        && (pc.times_seen != 0 || 2 < text_int.digits)
    {
        pc.year_seen = true;
        pc.year = text_int;
    } else if 4 < text_int.digits {
        pc.dates_seen = pc.dates_seen.saturating_add(1);
        pc.day = text_int.value % 100;
        pc.month = (text_int.value / 100) % 100;
        pc.year.value = text_int.value / 10000;
        pc.year.digits = text_int.digits.saturating_sub(4);
    } else {
        pc.times_seen = pc.times_seen.saturating_add(1);
        if text_int.digits <= 2 {
            pc.hour = text_int.value;
            pc.minutes = 0;
        } else {
            pc.hour = text_int.value / 100;
            pc.minutes = text_int.value % 100;
        }
        pc.seconds = Timespec::default();
        pc.meridian = MER_24;
    }
}

/// `apply_relative_time`: add `factor` (1 or -1) times `rel`, or fail on
/// overflow.
fn apply_relative_time(pc: &mut ParserControl<'_, '_>, rel: RelativeTime, factor: i64) -> bool {
    fn step(a: i64, b: i64, factor: i64) -> Option<i64> {
        if factor < 0 {
            a.checked_sub(b)
        } else {
            a.checked_add(b)
        }
    }
    let ns = if factor < 0 {
        pc.rel.ns.checked_sub(rel.ns)
    } else {
        pc.rel.ns.checked_add(rel.ns)
    };
    let sum = (|| {
        Some(RelativeTime {
            ns: ns?,
            seconds: step(pc.rel.seconds, rel.seconds, factor)?,
            minutes: step(pc.rel.minutes, rel.minutes, factor)?,
            hour: step(pc.rel.hour, rel.hour, factor)?,
            day: step(pc.rel.day, rel.day, factor)?,
            month: step(pc.rel.month, rel.month, factor)?,
            year: step(pc.rel.year, rel.year, factor)?,
        })
    })();
    match sum {
        Some(sum) => {
            pc.rel = sum;
            pc.rels_seen = true;
            true
        }
        None => false,
    }
}

/// `set_hhmmss`.
fn set_hhmmss(pc: &mut ParserControl<'_, '_>, hour: i64, minutes: i64, sec: i64, nsec: i32) {
    pc.hour = hour;
    pc.minutes = minutes;
    pc.seconds = Timespec {
        tv_sec: sec,
        tv_nsec: nsec,
    };
}

/// `time_zone_hhmm`: a zone written `±HH`, `±HHMM` or `±HH:MM`, into
/// `pc.time_zone`. With no minutes, one or two digits are hours and more are
/// `HHMM`. Anything beyond ±24 hours is refused, as POSIX's `TZ` bounds it.
fn time_zone_hhmm(pc: &mut ParserControl<'_, '_>, s: TextInt, mm: i64) -> bool {
    let mut value = s.value;
    if s.digits <= 2 && mm < 0 {
        value = value.saturating_mul(100);
    }
    let n_minutes = if mm < 0 {
        (value / 100)
            .checked_mul(60)
            .and_then(|h| h.checked_add(value % 100))
    } else {
        value.checked_mul(60).and_then(|m| {
            if s.negative {
                m.checked_sub(mm)
            } else {
                m.checked_add(mm)
            }
        })
    };
    match n_minutes {
        Some(n) if (-24 * 60..=24 * 60).contains(&n) => {
            // In range, so the product fits an `int`.
            pc.time_zone = i32::try_from(n.saturating_mul(60)).unwrap_or(0);
            true
        }
        _ => false,
    }
}

/// `to_hour`: a 12- or 24-hour clock reading as 0–23, or -1.
fn to_hour(hours: i64, meridian: i64) -> i32 {
    let h = match meridian {
        MER_AM => {
            if 0 < hours && hours < 12 {
                hours
            } else if hours == 12 {
                0
            } else {
                -1
            }
        }
        MER_PM => {
            if 0 < hours && hours < 12 {
                hours.saturating_add(12)
            } else if hours == 12 {
                12
            } else {
                -1
            }
        }
        _ => {
            if (0..24).contains(&hours) {
                hours
            } else {
                -1
            }
        }
    };
    i32::try_from(h).unwrap_or(-1)
}

/// `to_tm_year`: a written year as `tm_year`, with POSIX's two-digit window
/// (00–68 are 2000–2068, 69–99 are 1969–1999).
fn to_tm_year(pc: &mut ParserControl<'_, '_>, textyear: TextInt) -> Option<i32> {
    let mut year = textyear.value;
    if 0 <= year && textyear.digits == 2 {
        let adjusted = year.saturating_add(if year < 69 { 2000 } else { 1900 });
        if pc.debugging() {
            pc.dbg_printf(
                format!("warning: adjusting year value {year} to {adjusted}\n").as_bytes(),
            );
        }
        year = adjusted;
    }
    // A negative year counts back from 1900 the other way: upstream's
    // `-TM_YEAR_BASE - year`.
    let tm_year = if year < 0 {
        (-TM_YEAR_BASE).checked_sub(year)
    } else {
        year.checked_sub(TM_YEAR_BASE)
    }
    .and_then(|y| i32::try_from(y).ok());
    if tm_year.is_none() && pc.debugging() {
        pc.dbg_printf(format!("error: out-of-range year {year}\n").as_bytes());
    }
    tm_year
}

/// `time_zone_str`: `+05`, `-03:30`, `+05:45:10`.
fn time_zone_str(time_zone: i32) -> String {
    let sign = if time_zone < 0 { '-' } else { '+' };
    let hour = (time_zone / 3600).unsigned_abs();
    let mut out = format!("{sign}{hour:02}");
    let offset_from_hour = (time_zone % 3600).unsigned_abs();
    if offset_from_hour != 0 {
        let mm = offset_from_hour / 60;
        let ss = offset_from_hour % 60;
        out.push_str(&format!(":{mm:02}"));
        if ss != 0 {
            out.push_str(&format!(":{ss:02}"));
        }
    }
    out
}

/// `tm_year_str`: a `tm_year` as a year, even where adding 1900 would
/// overflow an `int`.
fn tm_year_str(tm_year: i32) -> String {
    let century = (tm_year / 100).saturating_add(19).unsigned_abs();
    let yy = (tm_year % 100).unsigned_abs();
    if -1900 <= tm_year {
        format!("{century:02}{yy:02}")
    } else {
        format!("-{century:02}{yy:02}")
    }
}

/// `debug_strfdatetime`: `(Y-M-D) 2021-06-15 12:00:00`, plus ` TZ=+05:30`
/// when a zone was parsed. The fields are printed as they are, in range or
/// not, which is the point: this shows what was asked for.
fn debug_strfdatetime(tm: &StructTm, pc: Option<&ParserControl<'_, '_>>) -> String {
    // nstrftime's %Y is tm_year + 1900 at least four digits wide, sign
    // included, which is also what `{:04}` does with a negative number.
    let mut out = format!(
        "(Y-M-D) {:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        i64::from(tm.tm_year).saturating_add(TM_YEAR_BASE),
        i64::from(tm.tm_mon).saturating_add(1),
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    );
    if let Some(pc) = pc
        && pc.zones_seen != 0
    {
        // Upstream also adds an hour for a local zone in DST here, under a
        // condition that includes `!pc->zones_seen` and so never holds.
        out.push_str(&format!(" TZ={}", time_zone_str(pc.time_zone)));
    }
    out
}

/// `debug_strfdate`: `(Y-M-D) 2021-06-15`.
fn debug_strfdate(tm: &StructTm) -> String {
    format!(
        "(Y-M-D) {}-{:02}-{:02}",
        tm_year_str(tm.tm_year),
        i64::from(tm.tm_mon).saturating_add(1),
        tm.tm_mday
    )
}

/// `debug_strftime`: `12:00:00`.
fn debug_strftime(tm: &StructTm) -> String {
    format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec)
}

/// `mktime_ok`: `mktime` succeeded and left the fields as they were asked
/// for — so the time existed and every field was in range.
fn mktime_ok(tm0: &StructTm, tm1: &StructTm) -> bool {
    if tm1.tm_wday < 0 {
        return false;
    }
    tm0.tm_sec == tm1.tm_sec
        && tm0.tm_min == tm1.tm_min
        && tm0.tm_hour == tm1.tm_hour
        && tm0.tm_mday == tm1.tm_mday
        && tm0.tm_mon == tm1.tm_mon
        && tm0.tm_year == tm1.tm_year
}

/// `debug_mktime_not_ok`: explain a `mktime_ok` failure.
fn debug_mktime_not_ok(
    tm0: &StructTm,
    tm1: &StructTm,
    pc: &mut ParserControl<'_, '_>,
    time_zone_seen: bool,
) {
    if !pc.debugging() {
        return;
    }
    let eq_sec = tm0.tm_sec == tm1.tm_sec;
    let eq_min = tm0.tm_min == tm1.tm_min;
    let eq_hour = tm0.tm_hour == tm1.tm_hour;
    let eq_mday = tm0.tm_mday == tm1.tm_mday;
    let eq_month = tm0.tm_mon == tm1.tm_mon;
    let eq_year = tm0.tm_year == tm1.tm_year;
    let dst_shift = eq_sec && eq_min && !eq_hour && eq_mday && eq_month && eq_year;

    pc.dbg_printf(b"error: invalid date/time value:\n");
    let provided = debug_strfdatetime(tm0, Some(pc));
    pc.dbg_printf(format!("    user provided time: '{provided}'\n").as_bytes());
    let normalized = debug_strfdatetime(tm1, Some(pc));
    pc.dbg_printf(format!("       normalized time: '{normalized}'\n").as_bytes());
    // Aligned with the two lines above, and not translated upstream.
    let marks = format!(
        "                                 {:>4} {:>2} {:>2} {:>2} {:>2} {:>2}",
        if eq_year { "" } else { "----" },
        if eq_month { "" } else { "--" },
        if eq_mday { "" } else { "--" },
        if eq_hour { "" } else { "--" },
        if eq_min { "" } else { "--" },
        if eq_sec { "" } else { "--" },
    );
    pc.dbg_printf(format!("{}\n", marks.trim_end_matches(' ')).as_bytes());
    pc.dbg_printf(b"     possible reasons:\n");
    if dst_shift {
        pc.dbg_printf(b"       nonexistent due to daylight-saving time;\n");
    }
    if !eq_mday && !eq_month {
        pc.dbg_printf(b"       invalid day/month combination;\n");
    }
    pc.dbg_printf(b"       numeric values overflow;\n");
    pc.dbg_printf(if time_zone_seen {
        b"       incorrect timezone\n"
    } else {
        b"       missing timezone\n"
    });
}

/// `mktime_z`: `mktime` in `zone`, writing the result through `tm` only on
/// success — which is how upstream detects failure (`tm_wday` or `tm_yday`
/// left at -1).
///
/// As upstream's, it switches `TZ` to `zone` for the call unless that is the
/// process's own, `process` ([`Zone::switched`]) — and glibc's `mktime` starts
/// with a `tzset ()`. Both read zones, and a read can change how the next
/// `posixrules` zone is anchored, so both are reproduced.
fn mktime_z(zone: &Zone, process: &Zone, tm: &mut StructTm, offset: &mut i64) -> i64 {
    zone.switched(process, || {
        zone.tzset();
        let mut tm1 = StructTm { tm_yday: -1, ..*tm };
        match zone.mktime_internal(&mut tm1, offset) {
            Some(t) => {
                *tm = tm1;
                t
            }
            None => -1,
        }
    })
}

/// The zone upstream falls back to when `mktime` fails for a string that
/// named its own offset: `TZ="XXX<offset>"`, a fixed offset.
///
/// Built from the same text upstream builds, so that glibc's reading of it —
/// the sign inverted, as POSIX `TZ` inverts it, and the hour clamped to 24 —
/// is the one used. (Which fixed offset it is does not change the answer,
/// which is corrected by the zone's own `tm_gmtoff` afterwards; it changes
/// only whether `mktime` can succeed at the edges of the representable
/// range.)
fn repair_zone(time_zone: i32) -> Zone {
    let sign = if time_zone < 0 { '-' } else { '+' };
    let hours = (time_zone / 3600).unsigned_abs().min(24);
    let rest = (time_zone % 3600).unsigned_abs();
    let (mm, ss) = (rest / 60, rest % 60);
    let mut rule = format!("XXX{sign}{hours:02}");
    if rest != 0 {
        rule.push_str(&format!(":{mm:02}"));
        if ss != 0 {
            rule.push_str(&format!(":{ss:02}"));
        }
    }
    Zone::from_tz(Some(rule.as_bytes()))
}

/// Upstream's `TZ="…"` prefix: if `p` starts with one, the zone it names, the
/// value as written (unescaped), and the rest of the string.
///
/// `\\` and `\"` are the only escapes; any other backslash, or no closing
/// quote, means there is no prefix and the string is parsed as it stands —
/// where `TZ` is then an unknown word.
fn tz_prefix(p: &[u8]) -> Option<(Zone, Vec<u8>, &[u8])> {
    let base = p.strip_prefix(b"TZ=\"")?;
    let mut value = Vec::new();
    let mut i = 0usize;
    loop {
        match *base.get(i)? {
            b'\\' => {
                i = i.saturating_add(1);
                let escaped = *base.get(i)?;
                if !(escaped == b'\\' || escaped == b'"') {
                    return None;
                }
                value.push(escaped);
            }
            b'"' => break,
            c => value.push(c),
        }
        i = i.saturating_add(1);
    }
    let zone = Zone::from_tz(Some(&value));
    let mut rest = base.get(i.saturating_add(1)..).unwrap_or(&[]);
    while let Some((&c, tail)) = rest.split_first() {
        if !c_isspace(c) {
            break;
        }
        rest = tail;
    }
    Some((zone, value, rest))
}

/// `localtime_rz`: `localtime_r` in `zone`, with `TZ` switched to it and back
/// unless it is the process's own, `process` (see [`mktime_z`]).
fn localtime_rz(zone: &Zone, process: &Zone, t: i64) -> Option<StructTm> {
    zone.switched(process, || zone.localtime_r(t))
}

/// `parse_datetime_body`: the whole of upstream's parse, with `mktime`'s
/// offset guess passed in.
#[allow(clippy::too_many_lines, reason = "one upstream function, ported whole")]
#[allow(
    clippy::cast_possible_truncation,
    reason = "upstream's intmax_t-to-int narrowings, which wrap as gcc's do"
)]
fn parse_datetime_body(
    input: &[u8],
    now: Option<Timespec>,
    debug: Option<&mut dyn Write>,
    tzdefault: &Zone,
    tzstring: Option<&[u8]>,
    offset: &mut i64,
) -> Option<Timespec> {
    // A C string ends at its first NUL; `date -f` can hand in a line with one.
    let input = input.split(|&b| b == 0).next().unwrap_or(&[]);
    let now = now.unwrap_or_else(Timespec::now);
    let mut start = now.tv_sec;
    let start_ns = now.tv_nsec;

    let mut p = input;
    while let Some((&c, tail)) = p.split_first() {
        if !c_isspace(c) {
            break;
        }
        p = tail;
    }

    // A `TZ="…"` prefix replaces the default zone for this string.
    let owned;
    let (tz, tzstring, from_string) = match tz_prefix(p) {
        Some((zone, value, rest)) => {
            p = rest;
            owned = (zone, value);
            (&owned.0, Some(owned.1.as_slice()), true)
        }
        None => (tzdefault, tzstring, false),
    };

    let tmp = localtime_rz(tz, tzdefault, now.tv_sec)?;

    // The empty string is "0" — without this it would be refused when parsed
    // during a DST transition.
    if p.is_empty() {
        p = b"0";
    }

    let mut pc = ParserControl {
        input: p,
        pos: 0,
        day_ordinal: 0,
        day_number: 0,
        local_isdst: 0,
        time_zone: 0,
        meridian: MER_24,
        year: TextInt::default(),
        month: i64::from(tmp.tm_mon).saturating_add(1),
        day: i64::from(tmp.tm_mday),
        hour: i64::from(tmp.tm_hour),
        minutes: i64::from(tmp.tm_min),
        seconds: Timespec {
            tv_sec: i64::from(tmp.tm_sec),
            tv_nsec: start_ns,
        },
        rel: RelativeTime::default(),
        timespec_seen: false,
        rels_seen: false,
        dates_seen: 0,
        days_seen: 0,
        j_zones_seen: 0,
        local_zones_seen: 0,
        dsts_seen: 0,
        times_seen: 0,
        zones_seen: 0,
        year_seen: false,
        debug,
        debug_dates_seen: false,
        debug_days_seen: false,
        debug_local_zones_seen: false,
        debug_times_seen: false,
        debug_zones_seen: false,
        debug_year_seen: false,
        debug_ordinal_day_seen: false,
        local_time_zone_table: Vec::with_capacity(2),
    };
    // Cannot saturate: tm_year is an int.
    pc.year.value = i64::from(tmp.tm_year).saturating_add(TM_YEAR_BASE);

    let mut tm = StructTm {
        tm_isdst: tmp.tm_isdst,
        ..StructTm::default()
    };

    // The local zone's abbreviation now, and — probing the next three
    // quarters — the one it uses on the other side of DST, if any.
    pc.local_time_zone_table.push(LocalZone {
        name: tmp.tm_zone.as_bytes().to_vec(),
        isdst: i64::from(tmp.tm_isdst),
    });
    for quarter in 1..=3i64 {
        let Some(probe) = start.checked_add(quarter.saturating_mul(90 * 24 * 60 * 60)) else {
            break;
        };
        if let Some(probe_tm) = localtime_rz(tz, tzdefault, probe)
            && probe_tm.tm_isdst != tmp.tm_isdst
        {
            pc.local_time_zone_table.push(LocalZone {
                name: probe_tm.tm_zone.as_bytes().to_vec(),
                isdst: i64::from(probe_tm.tm_isdst),
            });
            break;
        }
    }
    // A zone that uses one abbreviation for both says nothing about DST.
    if let [first, second] = pc.local_time_zone_table.as_mut_slice()
        && first.name == second.name
    {
        first.isdst = -1;
        pc.local_time_zone_table.truncate(1);
    }

    if grammar::yyparse(&mut pc) != grammar::Outcome::Accept {
        if pc.debugging() {
            let rest = pc.input.get(pc.pos..).unwrap_or(&[]);
            if rest.is_empty() {
                pc.dbg_printf(b"error: parsing failed\n");
            } else {
                let mut msg = b"error: parsing failed, stopped at '".to_vec();
                msg.extend_from_slice(rest);
                msg.extend_from_slice(b"'\n");
                pc.dbg_printf(&msg);
            }
        }
        return None;
    }

    // Which zone the input is read in.
    if pc.debugging() {
        let mut line = b"input timezone: ".to_vec();
        if pc.timespec_seen {
            line.extend_from_slice(b"'@timespec' - always UTC");
        } else if pc.zones_seen != 0 {
            line.extend_from_slice(b"parsed date/time string");
        } else if let Some(tzs) = tzstring {
            if from_string {
                line.extend_from_slice(b"TZ=\"");
                line.extend_from_slice(tzs);
                line.extend_from_slice(b"\" in date string");
            } else if tzs == b"UTC0" {
                // `date -u` sets TZ="UTC0".
                line.extend_from_slice(b"TZ=\"UTC0\" environment value or -u");
            } else {
                line.extend_from_slice(b"TZ=\"");
                line.extend_from_slice(tzs);
                line.extend_from_slice(b"\" environment value");
            }
        } else {
            line.extend_from_slice(b"system default");
        }
        // A local zone only changes DST, relative to the default zone.
        if pc.local_zones_seen != 0 && pc.zones_seen == 0 && 0 < pc.local_isdst {
            line.extend_from_slice(b", dst");
        }
        if pc.zones_seen != 0 {
            line.extend_from_slice(format!(" ({})", time_zone_str(pc.time_zone)).as_bytes());
        }
        line.push(b'\n');
        pc.dbg_printf(&line);
    }

    let result = if pc.timespec_seen {
        pc.seconds
    } else {
        let zone_parts = pc
            .j_zones_seen
            .saturating_add(pc.local_zones_seen)
            .saturating_add(pc.zones_seen);
        if 1 < (pc.times_seen | pc.dates_seen | pc.days_seen | pc.dsts_seen | zone_parts) {
            if pc.debugging() {
                if pc.times_seen > 1 {
                    pc.dbg_printf(b"error: seen multiple time parts\n");
                }
                if pc.dates_seen > 1 {
                    pc.dbg_printf(b"error: seen multiple date parts\n");
                }
                if pc.days_seen > 1 {
                    pc.dbg_printf(b"error: seen multiple days parts\n");
                }
                if pc.dsts_seen > 1 {
                    pc.dbg_printf(b"error: seen multiple daylight-saving parts\n");
                }
                if zone_parts > 1 {
                    pc.dbg_printf(b"error: seen multiple time-zone parts\n");
                }
            }
            return None;
        }

        let year = pc.year;
        let fields = to_tm_year(&mut pc, year).and_then(|tm_year| {
            let mon = i32::try_from(pc.month.checked_sub(1)?).ok()?;
            let mday = i32::try_from(pc.day).ok()?;
            Some((tm_year, mon, mday))
        });
        let Some((tm_year, tm_mon, tm_mday)) = fields else {
            if pc.debugging() {
                pc.dbg_printf(b"error: year, month, or day overflow\n");
            }
            return None;
        };
        tm.tm_year = tm_year;
        tm.tm_mon = tm_mon;
        tm.tm_mday = tm_mday;

        if pc.times_seen != 0 || (pc.rels_seen && pc.dates_seen == 0 && pc.days_seen == 0) {
            tm.tm_hour = to_hour(pc.hour, pc.meridian);
            if tm.tm_hour < 0 {
                if pc.debugging() {
                    let mrd = match pc.meridian {
                        MER_AM => "am",
                        MER_PM => "pm",
                        _ => "",
                    };
                    pc.dbg_printf(format!("error: invalid hour {}{mrd}\n", pc.hour).as_bytes());
                }
                return None;
            }
            // Upstream assigns these `intmax_t`/`time_t` values to `int`
            // fields; gcc keeps the low 32 bits.
            tm.tm_min = pc.minutes as i32;
            tm.tm_sec = pc.seconds.tv_sec as i32;
            if pc.debugging() {
                let what = if pc.times_seen != 0 {
                    "using specified time as starting value"
                } else {
                    "using current time as starting value"
                };
                pc.dbg_printf(format!("{what}: '{}'\n", debug_strftime(&tm)).as_bytes());
            }
        } else {
            tm.tm_hour = 0;
            tm.tm_min = 0;
            tm.tm_sec = 0;
            pc.seconds.tv_nsec = 0;
            if pc.debugging() {
                pc.dbg_printf(b"warning: using midnight as starting time: 00:00:00\n");
            }
        }

        // Let mktime decide tm_isdst for an absolute time...
        if (pc.dates_seen | pc.days_seen | pc.times_seen) != 0 {
            tm.tm_isdst = -1;
        }
        // ...unless the input named local standard or daylight time.
        if pc.local_zones_seen != 0 {
            tm.tm_isdst = pc.local_isdst;
        }

        let tm0 = tm;
        tm.tm_wday = -1;

        start = mktime_z(tz, tzdefault, &mut tm, offset);

        if !mktime_ok(&tm0, &tm) {
            let time_zone_seen = pc.zones_seen != 0;
            let mut repaired = false;
            if time_zone_seen {
                // Guard against a false failure near the edges of time_t when
                // the string is in another zone: retry in a fixed zone at the
                // string's own offset. (See `repair_zone`.)
                let tz2 = repair_zone(pc.time_zone);
                tm = StructTm { tm_wday: -1, ..tm0 };
                start = mktime_z(&tz2, tzdefault, &mut tm, offset);
                repaired = mktime_ok(&tm0, &tm);
            }
            if !repaired {
                debug_mktime_not_ok(&tm0, &tm, &mut pc, time_zone_seen);
                return None;
            }
        }

        if pc.days_seen != 0 && pc.dates_seen == 0 {
            // A weekday: move forward to it, by whole weeks for an ordinal.
            tm.tm_yday = -1;
            let day_ordinal = pc
                .day_ordinal
                .saturating_sub(i64::from(0 < pc.day_ordinal && tm.tm_wday != pc.day_number));
            // Both are 0-6, so this is 1-13 before the remainder.
            let to_weekday = i64::from(pc.day_number)
                .saturating_sub(i64::from(tm.tm_wday))
                .saturating_add(7)
                .rem_euclid(7);
            let moved = day_ordinal
                .checked_mul(7)
                .and_then(|weeks| to_weekday.checked_add(weeks))
                .and_then(|dayincr| dayincr.checked_add(i64::from(tm.tm_mday)))
                .and_then(|mday| i32::try_from(mday).ok());
            if let Some(mday) = moved {
                tm.tm_mday = mday;
                tm.tm_isdst = -1;
                start = mktime_z(tz, tzdefault, &mut tm, offset);
            }
            if tm.tm_yday < 0 {
                if pc.debugging() {
                    let msg = format!(
                        "error: day '{}' (day ordinal={} number={}) resulted in an invalid date: '{}'\n",
                        pc.str_days(),
                        pc.day_ordinal,
                        pc.day_number,
                        debug_strfdatetime(&tm, Some(&pc))
                    );
                    pc.dbg_printf(msg.as_bytes());
                }
                return None;
            }
            if pc.debugging() {
                let msg = format!(
                    "new start date: '{}' is '{}'\n",
                    pc.str_days(),
                    debug_strfdatetime(&tm, Some(&pc))
                );
                pc.dbg_printf(msg.as_bytes());
            }
        }

        if pc.debugging() {
            if pc.dates_seen == 0 && pc.days_seen == 0 {
                let msg = format!(
                    "using current date as starting value: '{}'\n",
                    debug_strfdate(&tm)
                );
                pc.dbg_printf(msg.as_bytes());
            }
            if pc.days_seen != 0 && pc.dates_seen != 0 {
                let msg = format!(
                    "warning: day ({}) ignored when explicit dates are given\n",
                    pc.str_days()
                );
                pc.dbg_printf(msg.as_bytes());
            }
            let msg = format!(
                "starting date/time: '{}'\n",
                debug_strfdatetime(&tm, Some(&pc))
            );
            pc.dbg_printf(msg.as_bytes());
        }

        // Add the relative date.
        if (pc.rel.year | pc.rel.month | pc.rel.day) != 0 {
            if pc.debugging() {
                if (pc.rel.year != 0 || pc.rel.month != 0) && tm.tm_mday != 15 {
                    pc.dbg_printf(
                        b"warning: when adding relative months/years, it is recommended to specify the 15th of the months\n",
                    );
                }
                if pc.rel.day != 0 && tm.tm_hour != 12 {
                    pc.dbg_printf(
                        b"warning: when adding relative days, it is recommended to specify noon\n",
                    );
                }
            }

            let shifted = (|| {
                let year = i32::try_from(i64::from(tm.tm_year).checked_add(pc.rel.year)?).ok()?;
                let month = i32::try_from(i64::from(tm.tm_mon).checked_add(pc.rel.month)?).ok()?;
                let day = i32::try_from(i64::from(tm.tm_mday).checked_add(pc.rel.day)?).ok()?;
                Some((year, month, day))
            })();
            let Some((year, month, day)) = shifted else {
                if pc.debugging() {
                    // Upstream prints its own `__FILE__:__LINE__` here.
                    pc.dbg_printf(b"error: parse-datetime.y:2144\n");
                }
                return None;
            };
            tm.tm_year = year;
            tm.tm_mon = month;
            tm.tm_mday = day;
            tm.tm_hour = tm0.tm_hour;
            tm.tm_min = tm0.tm_min;
            tm.tm_sec = tm0.tm_sec;
            tm.tm_isdst = tm0.tm_isdst;
            tm.tm_wday = -1;
            start = mktime_z(tz, tzdefault, &mut tm, offset);
            if tm.tm_wday < 0 {
                if pc.debugging() {
                    let msg = format!(
                        "error: adding relative date resulted in an invalid date: '{}'\n",
                        debug_strfdatetime(&tm, Some(&pc))
                    );
                    pc.dbg_printf(msg.as_bytes());
                }
                return None;
            }

            if pc.debugging() {
                let msg = format!(
                    "after date adjustment ({:+} years, {:+} months, {:+} days),\n",
                    pc.rel.year, pc.rel.month, pc.rel.day
                );
                pc.dbg_printf(msg.as_bytes());
                let msg = format!(
                    "    new date/time = '{}'\n",
                    debug_strfdatetime(&tm, Some(&pc))
                );
                pc.dbg_printf(msg.as_bytes());

                // Crossing a DST change by a day adjustment (bug#8357).
                if tm0.tm_isdst != -1 && tm.tm_isdst != tm0.tm_isdst {
                    pc.dbg_printf(b"warning: daylight saving time changed after date adjustment\n");
                }

                // The day was not asked to change but did (May 31 + 1 month),
                // or the month was not asked to change but did (Feb 29 + 1
                // year).
                if pc.rel.day == 0
                    && (tm.tm_mday != day || (pc.rel.month == 0 && tm.tm_mon != month))
                {
                    pc.dbg_printf(b"warning: month/year adjustment resulted in shifted dates:\n");
                    let msg = format!(
                        "     adjusted Y M D: {} {:02} {:02}\n",
                        tm_year_str(year),
                        i64::from(month).saturating_add(1),
                        day
                    );
                    pc.dbg_printf(msg.as_bytes());
                    let msg = format!(
                        "   normalized Y M D: {} {:02} {:02}\n",
                        tm_year_str(tm.tm_year),
                        i64::from(tm.tm_mon).saturating_add(1),
                        tm.tm_mday
                    );
                    pc.dbg_printf(msg.as_bytes());
                }
            }
        }

        // The zone correction: the only output of this block is `start`, so
        // it follows every block that sets it.
        if pc.zones_seen != 0 {
            let corrected = i64::from(pc.time_zone)
                .checked_sub(tm.tm_gmtoff)
                .and_then(|delta| start.checked_sub(delta));
            let Some(t1) = corrected else {
                if pc.debugging() {
                    let msg = format!("error: timezone {} caused time_t overflow\n", pc.time_zone);
                    pc.dbg_printf(msg.as_bytes());
                }
                return None;
            };
            start = t1;
        }

        if pc.debugging() {
            let msg = format!(
                "'{}' = {} epoch-seconds\n",
                debug_strfdatetime(&tm, Some(&pc)),
                start
            );
            pc.dbg_printf(msg.as_bytes());
        }

        // Add the relative time. Leap seconds are ignored: "+ 10 minutes" is
        // 600 seconds, because the zone must be applied before relative
        // times and a second mktime would lose it.
        let orig_ns = i64::from(pc.seconds.tv_nsec);
        let sum_ns = orig_ns.saturating_add(i64::from(pc.rel.ns));
        let billion = i64::from(BILLION);
        let normalized_ns = sum_ns.rem_euclid(billion);
        // Exact: `normalized_ns` is `sum_ns`'s remainder.
        let d4 = sum_ns
            .saturating_sub(normalized_ns)
            .checked_div(billion)
            .unwrap_or(0);
        let t4 = pc
            .rel
            .hour
            .checked_mul(60 * 60)
            .and_then(|d1| start.checked_add(d1))
            .and_then(|t1| {
                pc.rel
                    .minutes
                    .checked_mul(60)
                    .and_then(|d2| t1.checked_add(d2))
            })
            .and_then(|t2| t2.checked_add(pc.rel.seconds))
            .and_then(|t3| t3.checked_add(d4));
        let Some(t4) = t4 else {
            if pc.debugging() {
                pc.dbg_printf(b"error: adding relative time caused an overflow\n");
            }
            return None;
        };
        let result = Timespec {
            tv_sec: t4,
            tv_nsec: i32::try_from(normalized_ns).unwrap_or(0),
        };

        if pc.debugging()
            && (pc.rel.hour | pc.rel.minutes | pc.rel.seconds | i64::from(pc.rel.ns)) != 0
        {
            let msg = format!(
                "after time adjustment ({:+} hours, {:+} minutes, {:+} seconds, {:+} ns),\n",
                pc.rel.hour, pc.rel.minutes, pc.rel.seconds, pc.rel.ns
            );
            pc.dbg_printf(msg.as_bytes());
            pc.dbg_printf(format!("    new time = {t4} epoch-seconds\n").as_bytes());

            // Crossing a DST change by a time adjustment (bug#8357).
            if tm.tm_isdst != -1
                && let Some(lmt) = localtime_rz(tz, tzdefault, t4)
                && tm.tm_isdst != lmt.tm_isdst
            {
                pc.dbg_printf(b"warning: daylight saving time changed after time adjustment\n");
            }
        }
        result
    };

    if pc.debugging() {
        match tzstring {
            None => pc.dbg_printf(b"timezone: system default\n"),
            // `date -u` simply sets TZ="UTC0".
            Some(b"UTC0") => pc.dbg_printf(b"timezone: Universal Time\n"),
            Some(tzs) => {
                let mut msg = b"timezone: TZ=\"".to_vec();
                msg.extend_from_slice(tzs);
                msg.extend_from_slice(b"\" environment value\n");
                pc.dbg_printf(&msg);
            }
        }
        let msg = format!(
            "final: {}.{:09} (epoch-seconds)\n",
            result.tv_sec, result.tv_nsec
        );
        pc.dbg_printf(msg.as_bytes());
        if let Some(gmt) = Zone::utc().localtime_r(result.tv_sec) {
            let msg = format!("final: {} (UTC)\n", debug_strfdatetime(&gmt, None));
            pc.dbg_printf(msg.as_bytes());
        }
        if let Some(lmt) = localtime_rz(tz, tzdefault, result.tv_sec) {
            // `time_zone_str` takes an int; tm_gmtoff always fits one.
            let utcoff = i32::try_from(lmt.tm_gmtoff).unwrap_or(0);
            let msg = format!(
                "final: {} (UTC{})\n",
                debug_strfdatetime(&lmt, None),
                time_zone_str(utcoff)
            );
            pc.dbg_printf(msg.as_bytes());
        }
    }

    Some(result)
}

/// `parse_datetime2`: `input` as an instant, relative to `now` (the current
/// time if `None`), in `tzdefault` unless the string names a zone of its own.
///
/// `tzstring` is the `TZ` value `tzdefault` was built from; it is used only
/// in `--debug` output. `debug`, when given, receives that output — upstream's
/// `PARSE_DATETIME_DEBUG` flag.
///
/// `None` means the string is not a date: upstream's `false`.
#[must_use]
pub fn parse_datetime2(
    input: &[u8],
    now: Option<Timespec>,
    debug: Option<&mut dyn Write>,
    tzdefault: &Zone,
    tzstring: Option<&[u8]>,
) -> Option<Timespec> {
    with_mktime_offset(|offset| parse_datetime_body(input, now, debug, tzdefault, tzstring, offset))
}

/// `parse_datetime`: [`parse_datetime2`] with debugging off and the zone
/// from `TZ`.
#[must_use]
pub fn parse_datetime(input: &[u8], now: Option<Timespec>) -> Option<Timespec> {
    let tzstring = std::env::var_os("TZ").map(|v| crate::quote::os_bytes(&v).into_owned());
    let tz = Zone::from_tz(tzstring.as_deref());
    parse_datetime2(input, now, None, &tz, tzstring.as_deref())
}

#[cfg(test)]
mod tests;
