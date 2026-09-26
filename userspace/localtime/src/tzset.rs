//! glibc's reading of `TZ`: `tzset`, its POSIX-rule engine, and `tzfile`.
//!
//! Every program in this tree that prints a time reimplements one that runs on
//! glibc, and "the same instant and the same `TZ`, a different hour" is the
//! least forgivable way for a port to differ. So [`Zone`](crate::Zone) is
//! resolved the way glibc 2.39 resolves it, and this module is that
//! resolution, ported function by function from `time/tzset.c` and
//! `time/tzfile.c` rather than re-derived from POSIX -- because the answers a
//! user can see live in glibc's details, not in the standard:
//!
//! | glibc | here | what it decides |
//! |---|---|---|
//! | `tzset_internal` | [`resolve`] | unset is `/etc/localtime`, empty is `Universal`, a `:` is dropped, a file is tried before a rule |
//! | `__tzset_parse_tz` | [`parse_tz`] | a rule that parses only in part keeps the part |
//! | `parse_tzname`, `parse_offset`, `parse_rule` | the same names | `sscanf`'s `%hu`: white space, a sign, and wrapping at 65536 |
//! | `__tzfile_read` | [`Table::from_tzif`] | a file's tables, and the process-wide `rule_dstoff` |
//! | `__tzfile_default` | [`Table::defaulted`] | a DST name with no dates takes `posixrules`' history |
//! | `compute_change`, `__tz_compute` | the same names | the year is the instant's UTC year, and every year up to 1970 changes as 1970 does |
//! | `__tzfile_compute` | [`Table::state`] | before the first transition, the first standard type; past the last, the footer |
//!
//! # Why not `tzrules`' engine
//!
//! `tzrules` (the libc's, and `no_std`) computes each year's own transitions
//! and refuses a rule it cannot fully parse. Both are defensible and neither is
//! glibc: under `TZ='CET-1CEST,M3.5.0,M10.5.0/3'` glibc says no instant before
//! 1970 is summer time, and under `TZ=Foo/Bar` it says UTC *named* `Foo`. So
//! `tzrules` is used here only to decode TZif ([`TzFile`]'s raw accessors), and
//! the policy is this module's.
//!
//! # Process-wide state, as glibc has it
//!
//! glibc has one zone per process, read lazily at the first conversion and
//! read again when `TZ` changes -- which gnulib does twice for every `TZ="…"`
//! in a date string -- and on every `tzset ()` after a `posixrules` zone,
//! which every `mktime` makes. [`Zone`](crate::Zone) reproduces those reads
//! (see its docs), because two pieces of glibc's state change with them and
//! both are visible:
//!
//! * **`rule_dstoff`**, a static in `tzfile.c` that `__tzfile_read` sets only
//!   for a file with no transitions and `__tzfile_default` sets to the user's
//!   DST offset. It starts at 0, so in a fresh process `TZ=AAA3BBB` re-anchors
//!   `posixrules`' fall transitions by the whole DST offset and its spring ones
//!   by the difference of standard offsets -- New York's 06:00Z end of daylight
//!   time becomes 04:00Z, its 07:00Z start 09:00Z. Measured, not inferred; it
//!   is [`RULE_DSTOFF`] here.
//! * **`computed_for`**, the per-rule cache of `compute_change`. It begins at
//!   the `memset`'s 0 for a half that did not parse (and -1 for one that did),
//!   so glibc's *first* lookup of an instant in the year 0 uses that half's
//!   zeroed change time rather than computing one. Each [`Rules`] carries its
//!   own copy, since each is one `__tzset_parse_tz`.
//!
//! # What is not reproduced
//!
//! * **Leap seconds.** A `right/` zone's records are skipped, as `tzrules`
//!   skips them; glibc would shift the clock by the accumulated correction.
//! * **A footer with a DST name and no dates.** glibc would run
//!   `__tzfile_default` from inside a lookup, replacing the zone mid-call. `zic`
//!   never writes one; here it gets the US rules, as with no `posixrules`.
//! * **Undefined behaviour.** `M0.n.d` and `M13.n.d` survive a failed parse
//!   and make glibc read `__mon_yday` out of bounds; reads outside the table
//!   are 0 here, and only a rule already malformed can reach them.
//! * **Designations over [`TZ_NAME_CAP`](tzrules::TZ_NAME_CAP) bytes** are cut
//!   to that length when they reach a [`TzInfo`]; glibc has no limit.
//! * **`__libc_enable_secure`.** A name with a `..` component is never read as
//!   a file here (see [`crate::zoneinfo_path`]); glibc refuses one only in a
//!   setuid program. The value then falls through to the POSIX rule, exactly as
//!   a file that is not there does.

use std::path::Path;
use std::sync::atomic::{AtomicI32, AtomicI64, Ordering};

use tzrules::{TzFile, TzInfo, TzName};

/// `TZDEFRULES`: the file a rule with a DST name and no dates borrows them from.
pub(crate) const TZDEFRULES: &str = "posixrules";

/// `SECSPERDAY`, which glibc declares `(__time64_t) 86400`.
const SECSPERDAY: i64 = 86_400;

/// `__mon_yday`, flattened: days before each month of a normal year, then of
/// a leap year.
///
/// Flattened because glibc indexes it as `&__mon_yday[leap][m]` and reads
/// `[-1]` and `[0]` from there, so a malformed month walks into the other row
/// before it walks off the table -- `M13` of a normal year reads the leap
/// year's first entry. See [`mon_yday`].
const MON_YDAY: [u16; 26] = [
    0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365, //
    0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366,
];

/// glibc's `rule_dstoff` (see the module docs).
static RULE_DSTOFF: AtomicI64 = AtomicI64::new(0);

/// Reset [`RULE_DSTOFF`] to a fresh process's value, returning what it was.
/// For tests, which share one process and must not see each other's reads.
#[cfg(test)]
pub(crate) fn reset_rule_dstoff() -> i64 {
    RULE_DSTOFF.swap(0, Ordering::Relaxed)
}

/// The byte at `i`, or NUL past the end -- a C string's view of a slice.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// `s` up to its first NUL, which is where every C function here stops.
fn until_nul(s: &[u8]) -> &[u8] {
    s.iter()
        .position(|&b| b == 0)
        .and_then(|end| s.get(..end))
        .unwrap_or(s)
}

/// `isspace` in the C locale.
fn is_c_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

// ---------------------------------------------------------------------------
// sscanf, as far as glibc's TZ parser uses it
// ---------------------------------------------------------------------------

/// One directive of the two formats the parser gives `sscanf`:
/// `"%hu%n:%hu%n:%hu%n"` and `"M%hu.%hu.%hu%n"`.
#[derive(Clone, Copy)]
enum Directive {
    /// An ordinary character, which must match the input exactly.
    Lit(u8),
    /// `%hu`.
    Hu,
    /// `%n`: how many bytes have been consumed so far.
    N,
}

/// `"%hu%n:%hu%n:%hu%n"`: hours, minutes, seconds.
const HMS: [Directive; 8] = [
    Directive::Hu,
    Directive::N,
    Directive::Lit(b':'),
    Directive::Hu,
    Directive::N,
    Directive::Lit(b':'),
    Directive::Hu,
    Directive::N,
];

/// `"M%hu.%hu.%hu%n"`: month, week, day.
const MND: [Directive; 7] = [
    Directive::Lit(b'M'),
    Directive::Hu,
    Directive::Lit(b'.'),
    Directive::Hu,
    Directive::Lit(b'.'),
    Directive::Hu,
    Directive::N,
];

/// What one `sscanf` call did.
struct Scanned {
    /// Conversions made -- `sscanf`'s return value, except that an input
    /// failure before the first conversion is 0 here and `EOF` there; no
    /// caller distinguishes the two.
    count: usize,
    /// The converted values, in order; only the first `count` were assigned,
    /// and the caller's variable keeps its old value for the rest.
    values: [u16; 3],
    /// The last `%n` reached, if any.
    consumed: Option<usize>,
}

impl Scanned {
    /// Value `i` if it was assigned, else the variable's prior value `old`.
    fn value_or(&self, i: usize, old: u16) -> u16 {
        if i < self.count {
            self.values.get(i).copied().unwrap_or(old)
        } else {
            old
        }
    }
}

/// `sscanf (s, format, ...)` for the directives above.
///
/// glibc's rules, which are what make `TZ=EST+ 5` five hours west: `%hu` skips
/// white space, takes one sign, needs a digit, and stores `strtoul`'s result
/// truncated to 16 bits; `%n` skips nothing; an ordinary character must match
/// the next byte exactly. The first failure ends the scan.
fn sscanf(s: &[u8], format: &[Directive]) -> Scanned {
    let mut out = Scanned {
        count: 0,
        values: [0; 3],
        consumed: None,
    };
    let mut p = 0usize;
    for directive in format {
        match *directive {
            Directive::Lit(c) => {
                if at(s, p) != c {
                    break;
                }
                p = p.saturating_add(1);
            }
            Directive::N => out.consumed = Some(p),
            Directive::Hu => {
                let Some((value, next)) = scan_hu(s, p) else {
                    break;
                };
                if let Some(slot) = out.values.get_mut(out.count) {
                    *slot = value;
                }
                out.count = out.count.saturating_add(1);
                p = next;
            }
        }
    }
    out
}

/// One `%hu` starting at `p`: the stored value and where it stopped, or `None`
/// for an input or matching failure.
fn scan_hu(s: &[u8], mut p: usize) -> Option<(u16, usize)> {
    while is_c_space(at(s, p)) {
        p = p.saturating_add(1);
    }
    let sign = at(s, p);
    if sign == 0 {
        return None;
    }
    let negative = sign == b'-';
    if sign == b'-' || sign == b'+' {
        p = p.saturating_add(1);
    }
    let start = p;
    let mut magnitude: Option<u64> = Some(0);
    while at(s, p).is_ascii_digit() {
        let digit = u64::from(at(s, p).wrapping_sub(b'0'));
        magnitude = magnitude
            .and_then(|m| m.checked_mul(10))
            .and_then(|m| m.checked_add(digit));
        p = p.saturating_add(1);
    }
    if p == start {
        return None;
    }
    // `strtoul`: ULONG_MAX on overflow whatever the sign, else the magnitude,
    // negated modulo 2^64 under a minus. Then `(unsigned short)`.
    let value = match magnitude {
        None => u64::MAX,
        Some(m) if negative => m.wrapping_neg(),
        Some(m) => m,
    };
    let [lo, hi, ..] = value.to_le_bytes();
    Some((u16::from_le_bytes([lo, hi]), p))
}

/// `strtoul (s + p, &end, 10)` where `s[p]` is known to be a digit: the value
/// (saturating, as `strtoul` does) and where it stopped.
fn strtoul_digits(s: &[u8], mut p: usize) -> (u64, usize) {
    let mut value: u64 = 0;
    while at(s, p).is_ascii_digit() {
        let digit = u64::from(at(s, p).wrapping_sub(b'0'));
        value = value.saturating_mul(10).saturating_add(digit);
        p = p.saturating_add(1);
    }
    (value, p)
}

// ---------------------------------------------------------------------------
// tz_rule and the POSIX-string engine
// ---------------------------------------------------------------------------

/// `enum { J0, J1, M } type` of a `tz_rule`; the `memset`'s zero is `J0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Kind {
    /// `n`: day of the year, from 0, counting February 29th.
    #[default]
    J0,
    /// `Jn`: day of the year, from 1, never counting February 29th.
    J1,
    /// `Mm.n.d`: day `d` of week `n` of month `m`.
    M,
}

/// glibc's `tz_rule`: one half of a POSIX `TZ` string.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Rule {
    /// The abbreviation this half of the year goes by.
    name: Vec<u8>,
    kind: Kind,
    m: u16,
    n: u16,
    d: u16,
    /// The time of day of the change, in seconds; negative or past a day is
    /// allowed, and nothing clamps it.
    secs: i32,
    /// Seconds east of UT. `int` in glibc 2.39's `tz_rule`.
    offset: i32,
    /// Whether `parse_rule` ran to the end for this half, which leaves
    /// `computed_for` at -1 rather than the `memset`'s 0.
    parsed: bool,
}

/// `computed_for` and `change`, which glibc keeps in the `tz_rule` itself.
#[derive(Debug)]
struct Cache {
    computed_for: AtomicI32,
    change: AtomicI64,
}

impl Cache {
    /// The state `__tzset_parse_tz` leaves `rule` in.
    fn fresh(rule: &Rule) -> Self {
        Self {
            computed_for: AtomicI32::new(if rule.parsed { -1 } else { 0 }),
            change: AtomicI64::new(0),
        }
    }
}

impl Clone for Cache {
    fn clone(&self) -> Self {
        Self {
            computed_for: AtomicI32::new(self.computed_for.load(Ordering::Relaxed)),
            change: AtomicI64::new(self.change.load(Ordering::Relaxed)),
        }
    }
}

/// What one instant is in some zone: `tm_isdst`, `tm_gmtoff`, `tm_zone`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct State<'a> {
    pub(crate) gmtoff: i32,
    pub(crate) is_dst: bool,
    pub(crate) name: &'a [u8],
}

impl State<'_> {
    /// As a [`TzInfo`], which is what [`Tm`](crate::Tm) is built from. A name
    /// longer than [`tzrules::TZ_NAME_CAP`] is cut; see the module docs.
    pub(crate) fn info(&self) -> TzInfo {
        let keep = self.name.len().min(tzrules::TZ_NAME_CAP);
        let name = self
            .name
            .get(..keep)
            .and_then(TzName::new)
            .unwrap_or(TzName::UTC);
        TzInfo {
            gmtoff: self.gmtoff,
            is_dst: self.is_dst,
            name,
        }
    }
}

/// `tz_rules`: what `__tzset_parse_tz` makes of a POSIX `TZ` string, with
/// `compute_change`'s per-rule cache.
#[derive(Clone, Debug)]
pub(crate) struct Rules {
    rules: [Rule; 2],
    cache: [Cache; 2],
}

impl Rules {
    fn new(rules: [Rule; 2]) -> Self {
        let cache = [Cache::fresh(&rules[0]), Cache::fresh(&rules[1])];
        Self { rules, cache }
    }

    /// The zone `tzset_internal` falls back to when neither a file nor a rule
    /// is there to read: both halves UTC and named `UTC`, change times -1.
    pub(crate) fn utc() -> Self {
        let half = Rule {
            name: b"UTC".to_vec(),
            ..Rule::default()
        };
        let cache = || Cache {
            computed_for: AtomicI32::new(0),
            change: AtomicI64::new(-1),
        };
        Self {
            rules: [half.clone(), half],
            cache: [cache(), cache()],
        }
    }

    /// The standard half, for a caller that needs an answer where glibc has
    /// none (see [`Rules::state`]).
    pub(crate) fn standard(&self) -> State<'_> {
        let [std, _] = &self.rules;
        State {
            gmtoff: std.offset,
            is_dst: false,
            name: &std.name,
        }
    }

    /// `__tz_compute` after the `__offtime (timer, 0, tp)` that precedes it --
    /// `None` where that fails, because the instant's UTC year does not fit in
    /// `tm_year` and glibc's `localtime` returns `NULL`.
    pub(crate) fn state(&self, t: i64) -> Option<State<'_>> {
        self.state_with(t, &self.cache)
    }

    /// [`Rules::state`] with a cache fresh from the parse, which is what a
    /// zoneinfo file's footer gets: `__tzfile_compute` re-parses it on every
    /// lookup, and the parse's `memset` resets the cache each time.
    fn state_fresh(&self, t: i64) -> Option<State<'_>> {
        let cache = [Cache::fresh(&self.rules[0]), Cache::fresh(&self.rules[1])];
        self.state_with(t, &cache)
    }

    fn state_with(&self, t: i64, cache: &[Cache; 2]) -> Option<State<'_>> {
        let year = utc_year(t)?;
        let [std, dst] = &self.rules;
        let [std_cache, dst_cache] = cache;
        let c0 = compute_change(std, std_cache, year);
        let c1 = compute_change(dst, dst_cache, year);
        // "We have to distinguish between northern and southern hemisphere.
        // For the latter the daylight saving time ends in the next year."
        let is_dst = if c0 > c1 {
            t < c1 || t >= c0
        } else {
            t >= c0 && t < c1
        };
        let half = if is_dst { dst } else { std };
        Some(State {
            gmtoff: half.offset,
            is_dst,
            name: &half.name,
        })
    }
}

/// `1900 + tm->tm_year` after `__offtime (t, 0, tm)`: the UTC year of `t`, or
/// `None` where `tm_year` cannot hold it. The addition is C's `int`, which
/// wraps for the few years whose `tm_year` fits and whose year does not.
fn utc_year(t: i64) -> Option<i32> {
    let (year, _, _) = tzrules::civil_from_days(t.div_euclid(SECSPERDAY));
    let tm_year = i32::try_from(year.checked_sub(1900)?).ok()?;
    Some(1900i32.wrapping_add(tm_year))
}

/// `__isleap`.
fn is_leap(year: i32) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// `(&__mon_yday[leap][m])[k]`, reading across the two rows as C does and
/// yielding 0 off either end of the table (see the module docs).
fn mon_yday(leap: bool, m: u16, k: i64) -> i64 {
    let row = if leap { 13 } else { 0 };
    let index = i64::from(m).checked_add(row).and_then(|i| i.checked_add(k));
    index
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| MON_YDAY.get(i))
        .map_or(0, |&d| i64::from(d))
}

/// `compute_change`: when, in `year`, the change `rule` describes happens.
///
/// C's `int` arithmetic is kept, wrapping where C would overflow, because the
/// day count is computed in `int` before it is widened. And every year up to
/// 1970 starts from 1970-01-01 -- `t = 0` -- with only the weekday and the
/// leap day taken from the year itself.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "C's `int` arithmetic on bounded values: a month and weekday below               65536, and a year whose own additions are the explicitly wrapping ones"
)]
fn compute_change(rule: &Rule, cache: &Cache, year: i32) -> i64 {
    if year != -1 && cache.computed_for.load(Ordering::Relaxed) == year {
        return cache.change.load(Ordering::Relaxed);
    }

    let mut t: i64 = if year > 1970 {
        let y = year;
        let days = y
            .wrapping_sub(1970)
            .wrapping_mul(365)
            .wrapping_add(y.wrapping_sub(1) / 4 - 1970 / 4)
            .wrapping_sub(y.wrapping_sub(1) / 100 - 1970 / 100)
            .wrapping_add(y.wrapping_sub(1) / 400 - 1970 / 400);
        i64::from(days).wrapping_mul(SECSPERDAY)
    } else {
        0
    };

    let leap = is_leap(year);
    match rule.kind {
        Kind::J1 => {
            t = t.wrapping_add((i64::from(rule.d) - 1).wrapping_mul(SECSPERDAY));
            if rule.d >= 60 && leap {
                t = t.wrapping_add(SECSPERDAY);
            }
        }
        Kind::J0 => t = t.wrapping_add(i64::from(rule.d).wrapping_mul(SECSPERDAY)),
        Kind::M => {
            t = t.wrapping_add(mon_yday(leap, rule.m, -1).wrapping_mul(SECSPERDAY));
            // Zeller's congruence for the weekday of the month's first day.
            let m = i32::from(rule.m);
            let m1 = (m + 9) % 12 + 1;
            let yy0 = if rule.m <= 2 {
                year.wrapping_sub(1)
            } else {
                year
            };
            let yy1 = yy0 / 100;
            let yy2 = yy0 % 100;
            let mut dow = ((26 * m1 - 2) / 10 + 1)
                .wrapping_add(yy2)
                .wrapping_add(yy2 / 4)
                .wrapping_add(yy1 / 4)
                .wrapping_sub(yy1.wrapping_mul(2))
                % 7;
            if dow < 0 {
                dow += 7;
            }
            // The day of the month, from 0, of the first `d`-day; then step a
            // week at a time while the month still has one.
            let mut d = i32::from(rule.d) - dow;
            if d < 0 {
                d += 7;
            }
            let month_days = mon_yday(leap, rule.m, 0) - mon_yday(leap, rule.m, -1);
            let mut i: u32 = 1;
            while i < u32::from(rule.n) {
                if i64::from(d) + 7 >= month_days {
                    break;
                }
                d = d.wrapping_add(7);
                i = i.saturating_add(1);
            }
            t = t.wrapping_add(i64::from(d).wrapping_mul(SECSPERDAY));
        }
    }

    let change = t
        .wrapping_sub(i64::from(rule.offset))
        .wrapping_add(i64::from(rule.secs));
    cache.computed_for.store(year, Ordering::Relaxed);
    cache.change.store(change, Ordering::Relaxed);
    change
}

/// `parse_tzname`: three or more letters, or `<` three or more of letters,
/// digits, `+` and `-`, then `>`.
fn parse_tzname(s: &[u8], pos: &mut usize, rule: &mut Rule) -> bool {
    let start = *pos;
    let mut p = start;
    while at(s, p).is_ascii_alphabetic() {
        p = p.saturating_add(1);
    }
    let (from, len) = if p.saturating_sub(start) >= 3 {
        (start, p.saturating_sub(start))
    } else {
        p = start;
        if at(s, p) != b'<' {
            return false;
        }
        p = p.saturating_add(1);
        let from = p;
        while matches!(at(s, p), b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'+' | b'-') {
            p = p.saturating_add(1);
        }
        let len = p.saturating_sub(from);
        if at(s, p) != b'>' || len < 3 {
            return false;
        }
        p = p.saturating_add(1);
        (from, len)
    };
    let Some(name) = s.get(from..from.saturating_add(len)) else {
        return false;
    };
    rule.name = name.to_vec();
    *pos = p;
    true
}

/// `compute_offset`: each field clamped, then summed.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "each field is clamped first, so the sum is at most 89999"
)]
fn compute_offset(ss: u16, mm: u16, hh: u16) -> i32 {
    let ss = i32::from(ss.min(59));
    let mm = i32::from(mm.min(59));
    let hh = i32::from(hh.min(24));
    ss + mm * 60 + hh * 3600
}

/// `parse_offset` for half `which` of `rules`.
///
/// The standard half needs a sign or a digit; the DST half may have neither,
/// and then is an hour ahead of standard time. The sign is POSIX's, inverted:
/// `EST5` is five hours *west*.
fn parse_offset(s: &[u8], pos: &mut usize, rules: &mut [Rule; 2], which: usize) -> bool {
    let mut p = *pos;
    let c = at(s, p);
    if which == 0 && (c == 0 || (c != b'+' && c != b'-' && !c.is_ascii_digit())) {
        return false;
    }
    let sign: i32 = if c == b'-' || c == b'+' {
        p = p.saturating_add(1);
        if c == b'-' { 1 } else { -1 }
    } else {
        -1
    };
    *pos = p;

    let scanned = sscanf(s.get(p..).unwrap_or(&[]), &HMS);
    let std_offset = rules[0].offset;
    let Some(rule) = rules.get_mut(which) else {
        return false;
    };
    if scanned.count > 0 {
        let hh = scanned.value_or(0, 0);
        let mm = scanned.value_or(1, 0);
        let ss = scanned.value_or(2, 0);
        rule.offset = sign.wrapping_mul(compute_offset(ss, mm, hh));
    } else if which == 0 {
        rule.offset = 0;
        return false;
    } else {
        rule.offset = std_offset.wrapping_add(3600);
    }
    *pos = p.saturating_add(scanned.consumed.unwrap_or(0));
    true
}

/// `parse_rule` for half `which`: a date (`Jn`, `n`, `Mm.n.d`, or nothing at
/// all for the US rules), then an optional `/time`.
///
/// On failure the fields already written stay written -- glibc parses straight
/// into the `tz_rule` -- and `computed_for` stays at the `memset`'s 0.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "`hh * 3600 + mm * 60 + ss` of three `u16`s is at most 239923635,               which `int` holds, as in glibc"
)]
fn parse_rule(s: &[u8], pos: &mut usize, rule: &mut Rule, which: usize) -> bool {
    let mut p = *pos;
    // "Ignore comma to support string following the incorrect specification
    // in early POSIX.1 printings."
    if at(s, p) == b',' {
        p = p.saturating_add(1);
    }

    let c = at(s, p);
    if c == b'J' || c.is_ascii_digit() {
        rule.kind = if c == b'J' { Kind::J1 } else { Kind::J0 };
        if rule.kind == Kind::J1 {
            p = p.saturating_add(1);
            if !at(s, p).is_ascii_digit() {
                return false;
            }
        }
        let (d, end) = strtoul_digits(s, p);
        if end == p || d > 365 {
            return false;
        }
        if rule.kind == Kind::J1 && d == 0 {
            return false;
        }
        rule.d = u16::try_from(d).unwrap_or(u16::MAX);
        p = end;
    } else if c == b'M' {
        rule.kind = Kind::M;
        let scanned = sscanf(s.get(p..).unwrap_or(&[]), &MND);
        rule.m = scanned.value_or(0, rule.m);
        rule.n = scanned.value_or(1, rule.n);
        rule.d = scanned.value_or(2, rule.d);
        if scanned.count != 3 || rule.m < 1 || rule.m > 12 || rule.n < 1 || rule.n > 5 || rule.d > 6
        {
            return false;
        }
        p = p.saturating_add(scanned.consumed.unwrap_or(0));
    } else if c == 0 {
        // The Energy Policy Act of 2005's dates: "M3.2.0,M11.1.0".
        rule.kind = Kind::M;
        (rule.m, rule.n, rule.d) = if which == 0 { (3, 2, 0) } else { (11, 1, 0) };
    } else {
        return false;
    }

    match at(s, p) {
        0 | b',' => rule.secs = 2 * 3600,
        b'/' => {
            p = p.saturating_add(1);
            if at(s, p) == 0 {
                return false;
            }
            let negative = at(s, p) == b'-';
            if negative {
                p = p.saturating_add(1);
            }
            // Unassigned fields keep these, so `/x` is 02:00:00.
            let scanned = sscanf(s.get(p..).unwrap_or(&[]), &HMS);
            let hh = i32::from(scanned.value_or(0, 2));
            let mm = i32::from(scanned.value_or(1, 0));
            let ss = i32::from(scanned.value_or(2, 0));
            p = p.saturating_add(scanned.consumed.unwrap_or(0));
            let secs = hh * 3600 + mm * 60 + ss;
            rule.secs = if negative { -secs } else { secs };
        }
        _ => return false,
    }

    rule.parsed = true;
    *pos = p;
    true
}

/// A zone as glibc holds one: POSIX rules, or a zoneinfo file's tables.
#[derive(Clone, Debug)]
pub(crate) enum Engine {
    /// `tz_rules`, with `__use_tzfile` clear.
    Rules(Rules),
    /// A file's tables, with `__use_tzfile` set.
    Table(Table),
}

impl Engine {
    /// What instant `t` is in this zone -- `None` only where glibc's
    /// `localtime` fails for a POSIX-rule zone (see [`Rules::state`]).
    pub(crate) fn state(&self, t: i64) -> Option<State<'_>> {
        match self {
            Self::Rules(rules) => rules.state(t),
            Self::Table(table) => Some(table.state(t)),
        }
    }

    /// Whether glibc would read this zone again at every `tzset ()`: one built
    /// by `__tzfile_default`, after which `old_tz` is left `NULL`, so the next
    /// `TZ` never compares equal to it.
    pub(crate) fn stale(&self) -> bool {
        matches!(self, Self::Table(table) if table.defaulted)
    }

    /// [`Engine::state`], answering the standard half where glibc has no
    /// answer at all, for callers that must print something.
    pub(crate) fn state_or_standard(&self, t: i64) -> State<'_> {
        match self {
            Self::Rules(rules) => rules.state(t).unwrap_or_else(|| rules.standard()),
            Self::Table(table) => table.state(t),
        }
    }
}

/// `__tzset_parse_tz`: what glibc makes of a `TZ` value that names no file.
///
/// `default` is `__tzfile_default`: given the two halves the string named, the
/// zone `posixrules` makes of them, if it can. It is asked only for a string
/// with a DST name and nothing after it but an offset and at most a lone
/// comma; if it answers `None`, the string's own (absent) dates are parsed,
/// which gives the US rules.
pub(crate) fn parse_tz(
    tz: &[u8],
    default: &mut dyn FnMut(&Rule, &Rule) -> Option<Table>,
) -> Engine {
    let s = until_nul(tz);
    let mut rules = [Rule::default(), Rule::default()];
    let mut p = 0usize;
    let [std, _] = &mut rules;
    if parse_tzname(s, &mut p, std) && parse_offset(s, &mut p, &mut rules, 0) {
        if at(s, p) == 0 {
            // "There is no DST."
            rules[1].name = rules[0].name.clone();
            rules[1].offset = rules[0].offset;
        } else {
            let [_, dst] = &mut rules;
            if parse_tzname(s, &mut p, dst) {
                parse_offset(s, &mut p, &mut rules, 1);
                if at(s, p) == 0 || (at(s, p) == b',' && at(s, p.saturating_add(1)) == 0) {
                    let [std, dst] = &rules;
                    if let Some(table) = default(std, dst) {
                        return Engine::Table(table);
                    }
                }
            }
            let [std, dst] = &mut rules;
            if parse_rule(s, &mut p, std, 0) {
                parse_rule(s, &mut p, dst, 1);
            }
        }
    }
    Engine::Rules(Rules::new(rules))
}

// ---------------------------------------------------------------------------
// tzfile
// ---------------------------------------------------------------------------

/// One `struct ttinfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ttinfo {
    offset: i32,
    is_dst: bool,
    name: Vec<u8>,
    is_std: bool,
    is_ut: bool,
}

/// A zoneinfo file's tables as `__tzfile_read` leaves them.
#[derive(Clone, Debug)]
pub(crate) struct Table {
    transitions: Vec<i64>,
    type_idxs: Vec<usize>,
    types: Vec<Ttinfo>,
    /// `tzspec`, when non-empty, already parsed -- see [`Rules::state_fresh`]
    /// for why parsing once is the same as glibc's parsing per lookup.
    spec: Option<Rules>,
    /// Built by `__tzfile_default` (see [`Engine::stale`]).
    defaulted: bool,
}

impl Table {
    /// `__tzfile_read`: the tables of a TZif file, or `None` if it is not one.
    ///
    /// Like glibc, reading a file with no transitions sets `rule_dstoff` to its
    /// first type's offset; reading any other leaves it alone.
    pub(crate) fn from_tzif(bytes: &[u8]) -> Option<Self> {
        let file = TzFile::parse(bytes)?;
        let count = file.transition_count();
        let mut transitions = Vec::with_capacity(count);
        let mut type_idxs = Vec::with_capacity(count);
        for i in 0..count {
            let (t, ty) = file.transition(i)?;
            transitions.push(t);
            type_idxs.push(ty);
        }
        let mut types = Vec::with_capacity(file.type_count());
        for ty in 0..file.type_count() {
            let lt = file.local_type(ty)?;
            types.push(Ttinfo {
                offset: lt.utoff,
                is_dst: lt.is_dst,
                name: lt.name.as_bytes().to_vec(),
                is_std: lt.is_std,
                is_ut: lt.is_ut,
            });
        }
        // "Don't use an empty TZ string." A footer that wants `posixrules`
        // gets the US rules instead; see the module docs.
        let spec = file.footer().filter(|f| !f.is_empty()).and_then(|f| {
            match parse_tz(f, &mut |_, _| None) {
                Engine::Rules(rules) => Some(rules),
                Engine::Table(_) => None,
            }
        });
        if count == 0
            && let Some(first) = types.first()
        {
            RULE_DSTOFF.store(i64::from(first.offset), Ordering::Relaxed);
        }
        Some(Self {
            transitions,
            type_idxs,
            types,
            spec,
            defaulted: false,
        })
    }

    /// `rule_stdoff` as `__tzfile_read` computes it: the offset of the last
    /// transition into standard time, 0 if every transition is into DST, or
    /// the first type's for a file with none.
    fn rule_stdoff(&self) -> i64 {
        if self.transitions.is_empty() {
            return self.types.first().map_or(0, |t| i64::from(t.offset));
        }
        self.type_idxs
            .iter()
            .rev()
            .filter_map(|&ty| self.types.get(ty))
            .find(|t| !t.is_dst)
            .map_or(0, |t| i64::from(t.offset))
    }

    /// `__tzfile_default`: `posixrules`' history under the user's names and
    /// offsets, or `None` if that file cannot stand in (fewer than two types).
    ///
    /// Every transition is re-anchored from the file's offsets to the user's:
    /// by the standard offsets, or by the DST ones when the clock was on DST
    /// and the transition is in wall-clock time -- where "the file's DST
    /// offset" is `rule_dstoff`, the process-wide value that this function
    /// then sets to the user's. See the module docs for what that does to the
    /// first use in a process.
    pub(crate) fn defaulted(bytes: &[u8], std: &Rule, dst: &Rule) -> Option<Self> {
        let mut table = Self::from_tzif(bytes)?;
        if table.types.len() < 2 {
            return None;
        }
        let rule_stdoff = table.rule_stdoff();
        let rule_dstoff = RULE_DSTOFF.load(Ordering::Relaxed);
        let (stdoff, dstoff) = (i64::from(std.offset), i64::from(dst.offset));

        let mut isdst = false;
        for (t, ty) in table.transitions.iter_mut().zip(table.type_idxs.iter_mut()) {
            let Some(trans_type) = table.types.get(*ty) else {
                continue;
            };
            if trans_type.is_ut {
                // "The transition time is in GMT. No correction to apply."
            } else if isdst && !trans_type.is_std {
                *t = t.wrapping_add(dstoff.wrapping_sub(rule_dstoff));
            } else {
                *t = t.wrapping_add(stdoff.wrapping_sub(rule_stdoff));
            }
            isdst = trans_type.is_dst;
            *ty = usize::from(trans_type.is_dst);
        }
        RULE_DSTOFF.store(dstoff, Ordering::Relaxed);

        // Types 0 and 1 become the user's; their indicators are the file's,
        // and nothing reads them again.
        let indicators = |i: usize| {
            table
                .types
                .get(i)
                .map_or((false, false), |t| (t.is_std, t.is_ut))
        };
        let (std_is_std, std_is_ut) = indicators(0);
        let (dst_is_std, dst_is_ut) = indicators(1);
        table.types = vec![
            Ttinfo {
                offset: std.offset,
                is_dst: false,
                name: std.name.clone(),
                is_std: std_is_std,
                is_ut: std_is_ut,
            },
            Ttinfo {
                offset: dst.offset,
                is_dst: true,
                name: dst.name.clone(),
                is_std: dst_is_std,
                is_ut: dst_is_ut,
            },
        ];
        table.defaulted = true;
        Some(table)
    }

    /// `__tzfile_compute`, for `localtime`.
    pub(crate) fn state(&self, t: i64) -> State<'_> {
        let first = self.transitions.first().copied();
        let last = self.transitions.last().copied();
        let ty = match (first, last) {
            (Some(first), Some(last)) if t >= first => {
                if t >= last {
                    if let Some(spec) = &self.spec
                        && let Some(state) = spec.state_fresh(t)
                    {
                        return state;
                    }
                    // `use_last`.
                    self.type_idxs.last().copied()
                } else {
                    let i = self.search(t);
                    i.checked_sub(1)
                        .and_then(|j| self.type_idxs.get(j).copied())
                }
            }
            // Before the first transition, or there are none: "the first
            // non-DST type (or the first if they're all DST types)" -- which
            // for a file with no transitions means its footer is never read.
            _ => Some(self.types.iter().position(|x| !x.is_dst).unwrap_or(0)),
        };
        match ty.and_then(|ty| self.types.get(ty)) {
            Some(info) => State {
                gmtoff: info.offset,
                is_dst: info.is_dst,
                name: &info.name,
            },
            // Unreachable: every index was checked when the file was parsed.
            None => State {
                gmtoff: 0,
                is_dst: false,
                name: b"UTC",
            },
        }
    }

    /// The index `i` with `transitions[i - 1] <= t < transitions[i]`, found by
    /// `__tzfile_compute`'s own search: a guess from "DST changes twice a
    /// year", a linear walk if the guess is within ten, a binary search
    /// otherwise. Ported as written so that it gives glibc's answer on any
    /// table, including one `__tzfile_default` left out of order.
    ///
    /// Requires `transitions[0] <= t < transitions[len - 1]`.
    fn search(&self, t: i64) -> usize {
        let tr = &self.transitions;
        let n = tr.len();
        let get = |i: usize| tr.get(i).copied().unwrap_or(i64::MAX);
        let mut lo = 0usize;
        let mut hi = n.saturating_sub(1);

        let last = get(n.saturating_sub(1));
        let guess = last.wrapping_sub(t) / 15_778_476;
        // `size_t` of a possibly negative `__time64_t`: a huge value, which
        // then fails `i < num_transitions` as it does in C.
        let guess = usize::try_from(guess).unwrap_or(usize::MAX);
        if guess < n {
            let mut i = n.saturating_sub(1).saturating_sub(guess);
            if t < get(i) {
                if i < 10 || t >= get(i.saturating_sub(10)) {
                    while i > 0 && t < get(i.saturating_sub(1)) {
                        i = i.saturating_sub(1);
                    }
                    return i;
                }
                hi = i.saturating_sub(10);
            } else {
                if i.saturating_add(10) >= n || t < get(i.saturating_add(10)) {
                    while i < n && t >= get(i) {
                        i = i.saturating_add(1);
                    }
                    return i;
                }
                lo = i.saturating_add(10);
            }
        }

        while lo.saturating_add(1) < hi {
            let i = lo.saturating_add(hi) / 2;
            if t < get(i) {
                hi = i;
            } else {
                lo = i;
            }
        }
        hi
    }
}

// ---------------------------------------------------------------------------
// tzset_internal
// ---------------------------------------------------------------------------

/// Largest zoneinfo file that will be read.
///
/// The biggest in tzdata is under 4 KiB. The cap is not an optimisation:
/// `TZ=/dev/zero` is a legal thing for a parent process to set, and without a
/// bound the first program to print a timestamp reads until it runs out of
/// memory. glibc has no cap; a file this large is not a zone.
const MAX_ZONEINFO_BYTES: usize = 64 * 1024;

/// Read `path` whole, or `None` if it cannot be read or is over the cap.
pub(crate) fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    // One byte past the cap, so an oversized file is refused rather than
    // truncated to a prefix that might still parse.
    let limit = u64::try_from(MAX_ZONEINFO_BYTES).ok()?.saturating_add(1);
    std::io::Read::read_to_end(&mut std::io::Read::take(file, limit), &mut bytes).ok()?;
    (bytes.len() <= MAX_ZONEINFO_BYTES).then_some(bytes)
}

/// `__tzfile_read (name, 0, NULL)`: the zone file `name` names, if there is
/// one -- absolute, or under `dir`.
fn read_tzfile(name: &[u8], dir: &str) -> Option<Table> {
    // "User specified the empty string; use UTC with no leap seconds."
    if name.is_empty() {
        return None;
    }
    let path = crate::zoneinfo_path(name, dir)?;
    Table::from_tzif(&read_capped(&path)?)
}

/// `tzset_internal`: the zone a process runs in whose `TZ` is `tz`.
///
/// * unset: the file `localtime` (glibc's `TZDEFAULT`); if it cannot be read,
///   UTC named `UTC`.
/// * empty: the name `Universal`, then as any other value.
/// * a leading `:` is dropped -- and means nothing more than that.
/// * a zoneinfo file of that name, absolute or under `dir`, if there is one;
/// * else, if the name is now empty or is `localtime`'s own path, UTC named
///   `UTC`;
/// * else the POSIX rule, with `dir/posixrules` for a rule with no dates.
pub(crate) fn resolve(tz: Option<&[u8]>, dir: &str, localtime: &Path) -> Engine {
    let tz = tz.map(until_nul);
    let tz: Option<&[u8]> = match tz {
        Some([]) => Some(b"Universal"),
        other => other,
    };
    let tz = tz.map(|v| v.strip_prefix(b":").unwrap_or(v));
    let default_name = path_bytes(localtime);
    let name: &[u8] = tz.unwrap_or(&default_name);

    if let Some(table) = read_tzfile(name, dir) {
        return Engine::Table(table);
    }
    if name.is_empty() || name == default_name.as_slice() {
        return Engine::Rules(Rules::utc());
    }
    parse_tz(name, &mut |std, dst| {
        let bytes = read_capped(&Path::new(dir).join(TZDEFRULES))?;
        Table::defaulted(&bytes, std, dst)
    })
}

/// A path's bytes, for comparing with `TZ` as glibc compares two C strings.
#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    std::os::unix::ffi::OsStrExt::as_bytes(path.as_os_str()).to_vec()
}

/// A path's bytes on the Windows host build, where a path that is not valid
/// Unicode cannot have been written in a test.
#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().into_owned().into_bytes()
}

#[cfg(test)]
#[path = "tzset_tests.rs"]
mod tests;
