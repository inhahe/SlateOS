//! `mktime` and `localtime_r` as glibc computes them.
//!
//! This is a port of gnulib's `mktime.c` (`__mktime_internal`), which is the
//! file glibc itself builds its `mktime` from — the two share one source. It is
//! here because the question "which instant is this wall-clock time?" has no
//! single answer, and the program that asked it wants *glibc's* answer, not a
//! reasonable one:
//!
//! * **A time that a spring-forward skipped does not exist.** glibc returns an
//!   instant anyway — one that is the size of the gap away from the requested
//!   time, preferring the side whose `tm_isdst` differs from the one asked for
//!   — and the normalised fields it writes back are then *not* the fields that
//!   were requested. Callers depend on that: `posixtime` and `parse_datetime`
//!   both reject a date by comparing the fields they handed in with the fields
//!   they got back.
//! * **A time that a fall-back repeats exists twice**, and which of the two
//!   comes back depends on where the search *starts* — and glibc starts it from
//!   the offset the **previous** call found, kept in a process-wide static. So
//!   the answer for `01:30` on a November Sunday in New York depends on what
//!   the process converted before it. That is not a defect this port corrects;
//!   it is behaviour it reproduces ([`Zone::mktime`] keeps the same static),
//!   because a `date -f` that resolved the ambiguous hour differently from GNU
//!   on line 7 of a file would be wrong in the only sense that matters here.
//! * **An explicit `tm_isdst` that disagrees with the zone** is honoured by
//!   searching nearby instants for one with the requested flag and borrowing
//!   its offset — `2021-06-15 12:00` with `tm_isdst = 0` in New York is 13:00
//!   EDT. `parse_datetime` relies on this for input such as `EST` in July.
//!
//! [`Zone::epoch`] predates this and answers the first two questions its own
//! way; it is kept for the callers that want an instant that always exists.
//!
//! # `localtime_r` fails too
//!
//! `struct tm` holds the year as `int` years since 1900, so glibc's
//! `localtime_r` fails with `EOVERFLOW` for an instant whose local year does
//! not fit — about 68 billion seconds' worth of `i64` past either end. The
//! search above depends on that failure (it binary-searches for the last
//! instant that converts), so [`Zone::localtime_r`] reproduces it rather than
//! handing back a [`Tm`] with a year no `struct tm` could carry.

use std::sync::atomic::{AtomicI64, Ordering};

use tzrules::TzName;

use crate::{Tm, Zone};

/// C's `struct tm`, with C's field widths and C's meanings.
///
/// Distinct from [`Tm`], which is this crate's *rendering* type: a `Tm` is
/// always in range and knows its instant, while a `StructTm` is what a caller
/// hands to [`Zone::mktime`] — `tm_mday = 32` and `tm_mon = -3` included — and
/// what comes back from it. Keeping C's `int` widths is not nostalgia: where
/// upstream narrows a wider value into one of these fields, the narrowing is
/// part of the behaviour being ported.
///
/// As in C, [`Zone::mktime`] reads only the first six fields and `tm_isdst`;
/// the rest are outputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StructTm {
    /// Seconds, 0–60 on output (60 only for a leap second, which our zones
    /// never produce); any `int` on input.
    pub tm_sec: i32,
    /// Minutes, 0–59 on output.
    pub tm_min: i32,
    /// Hours, 0–23 on output.
    pub tm_hour: i32,
    /// Day of the month, 1–31 on output.
    pub tm_mday: i32,
    /// Months since January, 0–11 on output.
    pub tm_mon: i32,
    /// Years since 1900.
    pub tm_year: i32,
    /// Days since Sunday, 0–6. Output only.
    pub tm_wday: i32,
    /// Days since January 1st, 0–365. Output only.
    pub tm_yday: i32,
    /// Positive for daylight time, zero for standard time, negative for
    /// "unknown" — which only makes sense on input.
    pub tm_isdst: i32,
    /// Seconds east of UTC. Output only.
    pub tm_gmtoff: i64,
    /// The zone abbreviation. Output only.
    pub tm_zone: TzName,
}

impl Default for StructTm {
    /// Every field zero, as a zero-initialised `struct tm` is, except the zone
    /// name — which has no empty value — and `tm_isdst`, which is zero too.
    fn default() -> Self {
        Self {
            tm_sec: 0,
            tm_min: 0,
            tm_hour: 0,
            tm_mday: 0,
            tm_mon: 0,
            tm_year: 0,
            tm_wday: 0,
            tm_yday: 0,
            tm_isdst: 0,
            tm_gmtoff: 0,
            tm_zone: TzName::UTC,
        }
    }
}

impl StructTm {
    /// The `struct tm` that `localtime_r` would fill in for `tm`, or `None` if
    /// its year does not fit in `tm_year` — glibc's `EOVERFLOW`.
    #[must_use]
    pub fn from_tm(tm: &Tm) -> Option<Self> {
        let tm_year = i32::try_from(tm.year.checked_sub(TM_YEAR_BASE)?).ok()?;
        // The rest are bounded by construction (`Tm`'s documented ranges), so
        // these conversions cannot fail; `try_from` rather than `as` only so
        // that a future change to `Tm` cannot turn into silent wrapping.
        Some(Self {
            tm_sec: i32::try_from(tm.second).ok()?,
            tm_min: i32::try_from(tm.minute).ok()?,
            tm_hour: i32::try_from(tm.hour).ok()?,
            tm_mday: i32::try_from(tm.day).ok()?,
            tm_mon: i32::try_from(tm.month).ok()?.checked_sub(1)?,
            tm_year,
            tm_wday: i32::try_from(tm.wday).ok()?,
            tm_yday: i32::try_from(tm.yday).ok()?,
            tm_isdst: i32::from(tm.is_dst),
            tm_gmtoff: i64::from(tm.gmtoff),
            tm_zone: tm.abbr,
        })
    }
}

/// `TM_YEAR_BASE`: `tm_year` counts from 1900.
const TM_YEAR_BASE: i64 = 1900;

/// `EPOCH_YEAR`: the year `time_t` counts from.
const EPOCH_YEAR: i64 = 1970;

/// glibc's `localtime_offset`: the offset the last [`Zone::mktime`] found,
/// which the next one starts its search from.
///
/// Process-wide because glibc's is — see the module docs for why that is
/// observable. `Relaxed` is enough: this is a *guess*, and the only property
/// that matters is that a single-threaded program sees its own last store.
static LOCALTIME_OFFSET: AtomicI64 = AtomicI64::new(0);

/// Run `f` with glibc's process-wide `mktime` offset guess, and keep whatever
/// it leaves there.
///
/// For a caller that makes several [`Zone::mktime_internal`] calls as one C
/// function would make several `mktime` calls — `parse_datetime` makes up to
/// four — and wants them to see, and leave behind, exactly what glibc's would.
pub fn with_mktime_offset<R>(f: impl FnOnce(&mut i64) -> R) -> R {
    let mut offset = LOCALTIME_OFFSET.load(Ordering::Relaxed);
    let result = f(&mut offset);
    LOCALTIME_OFFSET.store(offset, Ordering::Relaxed);
    result
}

/// `__mon_yday`: days before each month, for normal and leap years.
const MON_YDAY: [[i64; 13]; 2] = [
    [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334, 365],
    [0, 31, 60, 91, 121, 152, 182, 213, 244, 274, 305, 335, 366],
];

/// Whether `year + 1900` is a leap year, for any `year` — including negative
/// ones, which is why this is not the textbook test on `year + 1900`.
fn leapyear(year: i64) -> bool {
    // `(-(TM_YEAR_BASE / 100)) & 3` is 1: two's complement, as in C.
    (year & 3) == 0 && (year % 100 != 0 || ((year / 100) & 3) == ((-(TM_YEAR_BASE / 100)) & 3))
}

/// Whether two `tm_isdst` values differ: one zero and the other positive.
/// A negative value ("unknown") differs from nothing.
fn isdst_differ(a: i32, b: i32) -> bool {
    ((a == 0) != (b == 0)) && 0 <= a && 0 <= b
}

/// `ydhms_diff`: `(YEAR1-YDAY1 HOUR1:MIN1:SEC1) - (YEAR0-YDAY0
/// HOUR0:MIN0:SEC0)` in seconds, assuming no clock was adjusted in between.
///
/// Years use `tm_year` numbering and need not be in range. The intermediate
/// leap-day counts are C `int`s upstream; every value that reaches them does so
/// from an `int` year divided by at least four, so they fit, and the
/// arithmetic here is the same computed wider.
#[allow(clippy::too_many_arguments, reason = "upstream's signature, kept")]
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded: see the doc comment"
)]
fn ydhms_diff(
    year1: i64,
    yday1: i64,
    hour1: i64,
    min1: i64,
    sec1: i64,
    year0: i64,
    yday0: i64,
    hour0: i64,
    min0: i64,
    sec0: i64,
) -> i64 {
    // `shr` is an arithmetic shift, which Rust's `>>` on a signed type is.
    let a4 = (year1 >> 2) + (TM_YEAR_BASE >> 2) - i64::from(year1 & 3 == 0);
    let b4 = (year0 >> 2) + (TM_YEAR_BASE >> 2) - i64::from(year0 & 3 == 0);
    let a100 = (a4 + i64::from(a4 < 0)) / 25 - i64::from(a4 < 0);
    let b100 = (b4 + i64::from(b4 < 0)) / 25 - i64::from(b4 < 0);
    let a400 = a100 >> 2;
    let b400 = b100 >> 2;
    let intervening_leap_days = (a4 - b4) - (a100 - b100) + (a400 - b400);

    let years = year1 - year0;
    let days = 365 * years + yday1 - yday0 + intervening_leap_days;
    let hours = 24 * days + hour1 - hour0;
    let minutes = 60 * hours + min1 - min0;
    60 * minutes + sec1 - sec0
}

/// `tm_diff`: the requested time minus `tp`, in seconds.
fn tm_diff(year: i64, yday: i64, hour: i64, min: i64, sec: i64, tp: &StructTm) -> i64 {
    ydhms_diff(
        year,
        yday,
        hour,
        min,
        sec,
        i64::from(tp.tm_year),
        i64::from(tp.tm_yday),
        i64::from(tp.tm_hour),
        i64::from(tp.tm_min),
        i64::from(tp.tm_sec),
    )
}

/// `long_int_avg`: the average of `a` and `b`, rounded toward +∞, without the
/// overflow that `(a + b) / 2` risks.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "each half is at most half the range"
)]
fn long_int_avg(a: i64, b: i64) -> i64 {
    (a >> 1) + (b >> 1) + ((a | b) & 1)
}

/// The earliest and latest instants whose local year could possibly fit in
/// `tm_year`, with two days of slack for the largest UTC offset a zone can
/// carry. Anything outside is refused before any calendar arithmetic runs —
/// both because glibc would refuse it and because the rules engine's
/// arithmetic is only sized for instants inside it.
fn convertible_bounds() -> (i64, i64) {
    const SLACK: i64 = 2 * 86_400;
    let first_year = i64::from(i32::MIN).saturating_add(TM_YEAR_BASE);
    let last_year = i64::from(i32::MAX).saturating_add(TM_YEAR_BASE);
    let lo = tzrules::days_from_civil(first_year, 1, 1)
        .saturating_mul(86_400)
        .saturating_sub(SLACK);
    let hi = tzrules::days_from_civil(last_year.saturating_add(1), 1, 1)
        .saturating_mul(86_400)
        .saturating_add(SLACK);
    (lo, hi)
}

impl Zone {
    /// `localtime_r`: the `struct tm` for UTC instant `t` in this zone, or
    /// `None` where glibc fails with `EOVERFLOW` — when the local year does
    /// not fit in `tm_year`.
    #[must_use]
    pub fn localtime_r(&self, t: i64) -> Option<StructTm> {
        let (lo, hi) = convertible_bounds();
        if !(lo..=hi).contains(&t) {
            return None;
        }
        StructTm::from_tm(&self.local(t, 0))
    }

    /// `ranged_convert`: convert `*t`, or if that overflows, the nearest
    /// instant between it and zero that does not — updating `*t` to match.
    fn ranged_convert(&self, t: &mut i64) -> Option<StructTm> {
        if let Some(tm) = self.localtime_r(*t) {
            return Some(tm);
        }
        // BAD is a known out-of-range value and OK a known in-range one;
        // narrow the gap until they are adjacent.
        let mut bad = *t;
        let mut ok = 0i64;
        let mut oktm: Option<StructTm> = None;
        loop {
            let mid = long_int_avg(ok, bad);
            if mid == ok || mid == bad {
                break;
            }
            match self.localtime_r(mid) {
                Some(tm) => {
                    ok = mid;
                    oktm = Some(tm);
                }
                None => bad = mid,
            }
        }
        // Upstream tests `oktm.tm_sec < 0`, its "never converted" sentinel.
        let tm = oktm?;
        *t = ok;
        Some(tm)
    }

    /// `mktime`: the instant `tm` names in this zone, writing the normalised
    /// fields back through `tm` — or `None`, leaving `tm` untouched, where
    /// glibc returns -1 with `EOVERFLOW`.
    ///
    /// The search starts from the offset the previous call found, process-wide,
    /// as glibc's does; see the module docs for when that is visible.
    pub fn mktime(&self, tm: &mut StructTm) -> Option<i64> {
        with_mktime_offset(|offset| self.mktime_internal(tm, offset))
    }

    /// `__mktime_internal`, with the offset guess passed explicitly.
    ///
    /// [`Zone::mktime`] is this with glibc's process-wide guess. A caller that
    /// must not observe what an earlier conversion found — a test, above all —
    /// passes its own; `0` reproduces the first `mktime` of a fresh process.
    #[allow(clippy::too_many_lines, reason = "one upstream function, ported whole")]
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "bounded as upstream's `long int` arithmetic is; see ydhms_diff"
    )]
    pub fn mktime_internal(&self, tp: &mut StructTm, offset: &mut i64) -> Option<i64> {
        // The maximum number of probes (calls to the conversion) should be
        // enough to handle any combination of zone rule changes, solar time,
        // leap seconds, and oscillation around a spring-forward gap.
        let mut remaining_probes = 6;

        let mut sec = i64::from(tp.tm_sec);
        let min = i64::from(tp.tm_min);
        let hour = i64::from(tp.tm_hour);
        let mday = i64::from(tp.tm_mday);
        let mon = i64::from(tp.tm_mon);
        let year_requested = i64::from(tp.tm_year);
        let isdst = tp.tm_isdst;

        // 1 if the previous probe was DST.
        let mut dst2 = false;

        // Bring the month into range, carrying into the year.
        let mon_remainder = mon % 12;
        let negative_mon_remainder = i64::from(mon_remainder < 0);
        let mon_years = mon / 12 - negative_mon_remainder;
        let year = year_requested + mon_years;

        // Day of the year from year, month and day of the month; need not be
        // in range.
        let month_index = usize::try_from(mon_remainder + 12 * negative_mon_remainder).ok()?;
        let mon_yday = MON_YDAY
            .get(usize::from(leapyear(year)))?
            .get(month_index)?
            - 1;
        let yday = mon_yday + mday;

        let off = *offset;
        let sec_requested = sec;

        // Out-of-range seconds are handled specially, since the difference
        // arithmetic assumes every minute has sixty of them.
        sec = sec.clamp(0, 59);

        // `ckd_sub (&negative_offset_guess, 0, off)` into an `int`: the wrapped
        // result is what upstream stores, overflow or not.
        let negative_offset_guess = i64::from(0i64.wrapping_sub(off) as i32);

        // Invert the conversion by probing, starting from the last offset.
        let t0 = ydhms_diff(
            year,
            yday,
            hour,
            min,
            sec,
            EPOCH_YEAR - TM_YEAR_BASE,
            0,
            0,
            0,
            negative_offset_guess,
        );
        let mut t = t0;
        let mut t1 = t0;
        let mut t2 = t0;

        let mut tm;
        let mut found = false;
        loop {
            tm = self.ranged_convert(&mut t)?;
            let dt = tm_diff(year, yday, hour, min, sec, &tm);
            if dt == 0 {
                break;
            }

            if t == t1
                && t != t2
                && (tm.tm_isdst < 0
                    || (if isdst < 0 {
                        dst2 <= (tm.tm_isdst != 0)
                    } else {
                        (isdst != 0) != (tm.tm_isdst != 0)
                    }))
            {
                // Oscillating between two values: the requested time is in a
                // spring-forward gap of size DT. Return a time DT away from
                // it, preferring the side whose tm_isdst differs from the one
                // requested (or, with none requested, the DST side).
                found = true;
                break;
            }

            remaining_probes -= 1;
            if remaining_probes == 0 {
                return None;
            }

            t1 = t2;
            t2 = t;
            t += dt;
            dst2 = tm.tm_isdst != 0;
        }

        // A match: check that tm_isdst has the requested value, if any.
        if !found && isdst_differ(isdst, tm.tm_isdst) {
            // It does not. Probe the adjacent instants in both directions for
            // one with the requested flag, and use its UTC offset; if none is
            // found within a reasonable bound, assume a one-hour difference.

            // +1 if standard time was wanted but DST was found, -1 if the
            // reverse.
            let dst_difference = i64::from(isdst == 0) - i64::from(tm.tm_isdst == 0);

            // The shortest DST period in tzdata2003a is 601200 seconds, and
            // the shortest non-DST period between two DST periods 694800;
            // probing at the smaller cannot step over either.
            let stride: i64 = 601_200;

            // The longest period of DST (or not) whose difference from the
            // adjacent period is not one hour, as of TZDB 2021e.
            let duration_max: i64 = 457_243_209;

            // Both directions are searched, so half the duration suffices;
            // the extra stride avoids an off-by-one at the end.
            let delta_bound = duration_max / 2 + stride;

            let mut delta = stride;
            let mut probed = None;
            'search: while delta < delta_bound {
                for direction in [-1i64, 1] {
                    let Some(mut ot) = t.checked_add(delta * direction) else {
                        continue;
                    };
                    let otm = self.ranged_convert(&mut ot)?;
                    if !isdst_differ(isdst, otm.tm_isdst) {
                        // The desired tm_isdst: extrapolate back to the
                        // desired time.
                        let gt = ot + tm_diff(year, yday, hour, min, sec, &otm);
                        if let Some(gtm) = self.localtime_r(gt) {
                            probed = Some((gt, gtm));
                            break 'search;
                        }
                    }
                }
                delta += stride;
            }

            if let Some((gt, gtm)) = probed {
                t = gt;
                tm = gtm;
            } else {
                // No unusual DST offset nearby: assume a one-hour difference.
                t += 60 * 60 * dst_difference;
                tm = self.localtime_r(t)?;
            }
        }

        // offset_found:
        // Remember T - T0 - NEGATIVE_OFFSET_GUESS for the next call. Only a
        // heuristic, so upstream lets it wrap; so does this.
        *offset = t.wrapping_sub(t0).wrapping_sub(negative_offset_guess);

        if sec_requested != i64::from(tm.tm_sec) {
            // Adjust for the seconds requested rather than the clamped value,
            // and repair a false match caused by a leap second.
            let sec_adjustment = i64::from(sec == 0 && tm.tm_sec == 60) - sec + sec_requested;
            t = t.checked_add(sec_adjustment)?;
            tm = self.localtime_r(t)?;
        }

        *tp = tm;
        Some(t)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use std::path::Path;

    /// New York's rules as a POSIX string, so these tests need no zoneinfo
    /// tree: EST, and EDT from the second Sunday in March to the first in
    /// November.
    fn new_york() -> Zone {
        Zone::resolve(
            Some(b"EST5EDT,M3.2.0,M11.1.0"),
            "/nonexistent",
            Path::new("/nonexistent"),
        )
    }

    fn request(
        year: i32,
        mon: i32,
        mday: i32,
        hour: i32,
        min: i32,
        sec: i32,
        isdst: i32,
    ) -> StructTm {
        StructTm {
            tm_sec: sec,
            tm_min: min,
            tm_hour: hour,
            tm_mday: mday,
            tm_mon: mon - 1,
            tm_year: year - 1900,
            tm_isdst: isdst,
            ..StructTm::default()
        }
    }

    fn fields(tm: &StructTm) -> (i32, i32, i32, i32, i32, i32) {
        (
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday,
            tm.tm_hour,
            tm.tm_min,
            tm.tm_sec,
        )
    }

    #[test]
    fn an_ordinary_time_round_trips_and_is_normalised() {
        let utc = Zone::utc();
        let mut tm = request(2021, 3, 32, 25, 61, 61, -1);
        let t = utc.mktime_internal(&mut tm, &mut 0).unwrap();
        // March 32nd 25:61:61 is April 2nd 02:02:01.
        assert_eq!(fields(&tm), (2021, 4, 2, 2, 2, 1));
        assert_eq!(t, 1_617_328_921);
        assert_eq!(tm.tm_wday, 5);
        assert_eq!(tm.tm_yday, 91);
    }

    #[test]
    fn a_negative_month_borrows_from_the_year() {
        let utc = Zone::utc();
        let mut tm = request(2021, -1, 15, 0, 0, 0, -1);
        utc.mktime_internal(&mut tm, &mut 0).unwrap();
        // Month -1 (tm_mon -2) of 2021 is November 2020.
        assert_eq!(fields(&tm), (2020, 11, 15, 0, 0, 0));
    }

    #[test]
    fn a_skipped_time_lands_the_gap_away_and_says_so_in_its_fields() {
        // 2021-03-14 02:30 does not exist in New York.
        let ny = new_york();
        let mut tm = request(2021, 3, 14, 2, 30, 0, -1);
        let t = ny.mktime_internal(&mut tm, &mut 0).unwrap();
        // glibc answers 03:30 EDT -- 07:30 UTC -- and the hour it writes back
        // is the evidence a caller uses to reject the input.
        assert_eq!(t, 1_615_707_000);
        assert_eq!(fields(&tm), (2021, 3, 14, 3, 30, 0));
        assert_eq!(tm.tm_isdst, 1);
    }

    #[test]
    fn a_repeated_time_depends_on_where_the_search_starts() {
        // 2021-11-07 01:30 happens twice in New York.
        let ny = new_york();
        // A fresh process starts from a UTC guess and finds the first (EDT).
        let mut tm = request(2021, 11, 7, 1, 30, 0, -1);
        let mut offset = 0;
        let first = ny.mktime_internal(&mut tm, &mut offset).unwrap();
        assert_eq!(first, 1_636_263_000);
        assert_eq!(tm.tm_isdst, 1);
        // Starting from the EST offset finds the second.
        let mut tm = request(2021, 11, 7, 1, 30, 0, -1);
        let mut offset = 5 * 3600;
        let second = ny.mktime_internal(&mut tm, &mut offset).unwrap();
        assert_eq!(second, first + 3600);
        assert_eq!(tm.tm_isdst, 0);
    }

    #[test]
    fn an_explicit_isdst_that_disagrees_borrows_the_other_offset() {
        let ny = new_york();
        // Noon in June "in standard time" is 17:00 UTC, i.e. 13:00 EDT.
        let mut tm = request(2021, 6, 15, 12, 0, 0, 0);
        let t = ny.mktime_internal(&mut tm, &mut 0).unwrap();
        assert_eq!(t, 1_623_776_400);
        assert_eq!(fields(&tm), (2021, 6, 15, 13, 0, 0));
        assert_eq!(tm.tm_isdst, 1);
        // And noon in January "in daylight time" is 16:00 UTC, 11:00 EST.
        let mut tm = request(2021, 1, 15, 12, 0, 0, 1);
        let t = ny.mktime_internal(&mut tm, &mut 0).unwrap();
        assert_eq!(t, 1_610_726_400);
        assert_eq!(fields(&tm), (2021, 1, 15, 11, 0, 0));
    }

    #[test]
    fn the_offset_found_is_remembered_for_the_next_call() {
        let ny = new_york();
        let mut offset = 0;
        let mut tm = request(2021, 1, 15, 12, 0, 0, -1);
        ny.mktime_internal(&mut tm, &mut offset).unwrap();
        // EST is five hours west: the guess becomes +5h.
        assert_eq!(offset, 5 * 3600);
    }

    #[test]
    fn a_leap_second_is_the_second_after_59() {
        let utc = Zone::utc();
        let mut tm = request(2016, 12, 31, 23, 59, 60, -1);
        let t = utc.mktime_internal(&mut tm, &mut 0).unwrap();
        assert_eq!(t, 1_483_228_800);
        assert_eq!(fields(&tm), (2017, 1, 1, 0, 0, 0));
    }

    #[test]
    fn a_year_that_does_not_fit_tm_year_is_refused_and_tm_is_untouched() {
        let utc = Zone::utc();
        // December of the last representable year, plus a month.
        let mut tm = request(0, 13, 1, 0, 0, 0, -1);
        tm.tm_year = i32::MAX;
        let before = tm;
        assert_eq!(utc.mktime_internal(&mut tm, &mut 0), None);
        assert_eq!(tm, before);
        // The last representable second itself converts.
        let mut tm = request(0, 12, 31, 23, 59, 59, -1);
        tm.tm_year = i32::MAX;
        let t = utc.mktime_internal(&mut tm, &mut 0).unwrap();
        assert_eq!(utc.localtime_r(t).unwrap().tm_year, i32::MAX);
        assert_eq!(utc.localtime_r(t + 1), None);
    }

    #[test]
    fn localtime_r_refuses_what_struct_tm_cannot_hold() {
        let utc = Zone::utc();
        assert_eq!(utc.localtime_r(i64::MAX), None);
        assert_eq!(utc.localtime_r(i64::MIN), None);
        let tm = utc.localtime_r(0).unwrap();
        assert_eq!(fields(&tm), (1970, 1, 1, 0, 0, 0));
        assert_eq!(tm.tm_wday, 4);
    }

    #[test]
    fn leapyear_works_for_negative_tm_years() {
        // tm_year 100 is 2000, a leap year; 0 is 1900, not one; -1900 is year
        // 0, which is.
        assert!(leapyear(100));
        assert!(!leapyear(0));
        assert!(leapyear(-1900));
        assert!(!leapyear(-1800));
        assert!(leapyear(-1896));
    }

    #[test]
    fn isdst_differ_ignores_unknown() {
        assert!(isdst_differ(0, 1));
        assert!(isdst_differ(1, 0));
        assert!(!isdst_differ(1, 2));
        assert!(!isdst_differ(-1, 0));
        assert!(!isdst_differ(0, -1));
    }
}
