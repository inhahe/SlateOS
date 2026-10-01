//! Time formatting, twice: glibc's `strftime` and gnulib's `nstrftime`.
//!
//! There is no one `strftime` to port, because the GNU programs this tree
//! reimplements do not share one:
//!
//! | caller | formats with |
//! |---|---|
//! | coreutils `date`, `ls`, `stat`, `du`, `pr`, `uptime`; diffutils | gnulib's `nstrftime` ([`nstrftime`]) |
//! | findutils `find -printf`, procps `ps`, GNU `tar`, `pinky`, bash's `printf %()T` | the C library's `strftime` ([`strftime`]) |
//!
//! and the two disagree in ways a user sees. gnulib pads a year to four
//! digits (`%Y` of year 21 is `0021`) where glibc prints `21`; gnulib has
//! `%N`, `%q`, `%:z` and the `+` flag, which glibc prints literally or
//! ignores; `%-5d` is `9` to gnulib and `    9` to glibc; `%10z` is `+0000`
//! padded to ten to one and `         +0000000000` to the other. Before this
//! module the tree had one formatter that was neither -- mostly gnulib's
//! behaviour, with glibc's here and there, and gnulib's `%3N` (the first three
//! digits: milliseconds) rendered as all nine.
//!
//! Both are ports of their sources for the C locale: glibc 2.39's
//! `time/strftime_l.c` and coreutils 9.4's `lib/nstrftime.c`, conversion for
//! conversion, keeping each one's flag, width, padding and sign rules. gnulib's
//! own copy hands the locale's words -- weekday and month names, `%c`, `%x`,
//! `%X`, `%r`, `%p`, and any `%E`/`%O` form -- to the C library's `strftime`
//! one conversion at a time and pads what comes back itself; so does this port,
//! through [`strftime`].
//!
//! # `%s` and `mktime`
//!
//! Both upstreams compute `%s` by calling `mktime` on the broken-down time,
//! which for a `Tm` produced by [`Zone::local`] is its `epoch` again. The call
//! has one side effect: it moves glibc's process-wide offset guess (see the
//! `mktime` module), which decides how a *later* `mktime` resolves a repeated
//! local hour. `date -f` formats with `%s` between parses, so
//! [`nstrftime_z`] takes the zone and makes that call; the other entry points
//! read `epoch`.

use crate::{MON_ABBR, MON_FULL, StructTm, Tm, WDAY_ABBR, WDAY_FULL, Zone};

/// `INT_MAX`, where upstream caps a width that overflows.
const INT_MAX: i64 = 2_147_483_647;

/// `TM_YEAR_BASE`.
const TM_YEAR_BASE: i64 = 1900;

/// The fields of `struct tm` the formatters read, widened to `i64` so that the
/// arithmetic upstream does in `int` cannot overflow here; for every `Tm`
/// [`Zone::local`] produces they hold the values the C fields would.
#[derive(Clone, Copy)]
struct Fields<'a> {
    sec: i64,
    min: i64,
    hour: i64,
    mday: i64,
    /// `tm_mon`: 0-11.
    mon: i64,
    /// `tm_year`: years since 1900.
    year: i64,
    wday: i64,
    yday: i64,
    isdst: i64,
    gmtoff: i64,
    zone: &'a [u8],
    ns: i64,
    epoch: i64,
}

impl<'a> Fields<'a> {
    fn of(tm: &'a Tm) -> Self {
        Fields {
            sec: i64::from(tm.second),
            min: i64::from(tm.minute),
            hour: i64::from(tm.hour),
            mday: i64::from(tm.day),
            mon: i64::from(tm.month).saturating_sub(1),
            year: tm.year.saturating_sub(TM_YEAR_BASE),
            wday: i64::from(tm.wday),
            yday: i64::from(tm.yday),
            isdst: i64::from(tm.is_dst),
            gmtoff: i64::from(tm.gmtoff),
            zone: tm.abbr.as_bytes(),
            ns: i64::from(tm.nanos),
            epoch: tm.epoch,
        }
    }

    /// `hour12`: 12, 1-11, 12, 1-11.
    fn hour12(&self) -> i64 {
        if self.hour > 12 {
            self.hour.saturating_sub(12)
        } else if self.hour == 0 {
            12
        } else {
            self.hour
        }
    }

    /// The C locale's abbreviated weekday, or `?` out of range.
    fn a_wkday(&self) -> &'static [u8] {
        name(&WDAY_ABBR, self.wday)
    }

    fn f_wkday(&self) -> &'static [u8] {
        name(&WDAY_FULL, self.wday)
    }

    fn a_month(&self) -> &'static [u8] {
        name(&MON_ABBR, self.mon)
    }

    fn f_month(&self) -> &'static [u8] {
        name(&MON_FULL, self.mon)
    }

    /// `AM` or `PM`, the C locale's `AM_STR`/`PM_STR`.
    fn ampm(&self) -> &'static [u8] {
        if self.hour > 11 { b"PM" } else { b"AM" }
    }
}

/// `table[index]`, or `?` for an index out of range -- upstream's answer to a
/// `struct tm` that `strptime` filled only partly.
fn name(table: &[&'static [u8]], index: i64) -> &'static [u8] {
    usize::try_from(index)
        .ok()
        .and_then(|i| table.get(i).copied())
        .unwrap_or(b"?")
}

/// The byte at `i`, or NUL past the end: a format is a C string.
fn at(format: &[u8], i: usize) -> u8 {
    format.get(i).copied().unwrap_or(0)
}

/// Upper- or lower-case, as `memcpy_uppcase` and `memcpy_lowcase` do in the C
/// locale: ASCII letters only.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    Keep,
    Upper,
    Lower,
}

impl Case {
    /// Upstream's two booleans, `to_lowcase` winning.
    fn of(to_lowcase: bool, to_uppcase: bool) -> Self {
        if to_lowcase {
            Case::Lower
        } else if to_uppcase {
            Case::Upper
        } else {
            Case::Keep
        }
    }

    fn push(self, out: &mut Vec<u8>, bytes: &[u8]) {
        match self {
            Case::Keep => out.extend_from_slice(bytes),
            Case::Upper => out.extend(bytes.iter().map(u8::to_ascii_uppercase)),
            Case::Lower => out.extend(bytes.iter().map(u8::to_ascii_lowercase)),
        }
    }
}

fn fill(out: &mut Vec<u8>, count: i64, byte: u8) {
    let n = usize::try_from(count).unwrap_or(0);
    out.extend(std::iter::repeat_n(byte, n));
}

/// The decimal digits of `u`, most significant first.
fn digits_of(mut u: u128) -> Vec<u8> {
    let mut rev = Vec::new();
    loop {
        rev.push(b'0'.saturating_add(u8::try_from(u % 10).unwrap_or(0)));
        u /= 10;
        if u == 0 {
            break;
        }
    }
    rev.reverse();
    rev
}

/// `iso_week_days`: days from the first day of the first ISO week of the year
/// to day `yday`, a `wday` weekday; negative for a day in the previous ISO
/// year. Both upstreams share it.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "bounded: yday within a year either side, wday 0-6"
)]
fn iso_week_days(yday: i64, wday: i64) -> i64 {
    const ISO_WEEK_START_WDAY: i64 = 1;
    const ISO_WEEK1_WDAY: i64 = 4;
    const YDAY_MINIMUM: i64 = -366;
    let big_enough_multiple_of_7 = (-YDAY_MINIMUM / 7 + 2) * 7;
    yday - (yday - wday + ISO_WEEK1_WDAY + big_enough_multiple_of_7) % 7 + ISO_WEEK1_WDAY
        - ISO_WEEK_START_WDAY
}

/// `__isleap`, on a full year.
fn isleap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

// ---------------------------------------------------------------------------
// glibc
// ---------------------------------------------------------------------------

/// glibc's `strftime`, in the C locale: `tm` rendered by `fmt`.
///
/// This is the C library's function, for the programs that call it --
/// `find -printf`, `ps`, `tar`, `pinky`, the shell. coreutils' own programs
/// use [`nstrftime`], which differs; see the module docs.
#[must_use]
pub fn strftime(fmt: &[u8], tm: &Tm) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len().saturating_add(16));
    glibc(&mut out, fmt, &Fields::of(tm));
    out
}

/// glibc's `add`: `bytes` right-aligned in `width` -- zero-filled for the `0`
/// flag and space-filled otherwise, `-` included -- in `case`.
fn g_add(out: &mut Vec<u8>, bytes: &[u8], width: i64, pad: u8, case: Case) {
    let n = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    let delta = width.saturating_sub(n);
    if delta > 0 {
        fill(out, delta, if pad == b'0' { b'0' } else { b' ' });
    }
    case.push(out, bytes);
}

/// glibc's `do_number_sign_and_padding`: `body` (the digits of the magnitude)
/// with its sign, padded to `digits` by the pad flag, then through `g_add`.
fn g_sign_and_padding(
    out: &mut Vec<u8>,
    body: &[u8],
    negative: bool,
    digits: i64,
    mut width: i64,
    pad: u8,
    case: Case,
) {
    let mut s = Vec::with_capacity(body.len().saturating_add(1));
    if negative {
        s.push(b'-');
    }
    s.extend_from_slice(body);
    let mut start = 0usize;
    if pad != b'-' {
        let len = i64::try_from(s.len()).unwrap_or(i64::MAX);
        let padding = digits.saturating_sub(len);
        if padding > 0 {
            if pad == b'_' {
                fill(out, padding, b' ');
                width = if width > padding {
                    width.saturating_sub(padding)
                } else {
                    0
                };
            } else {
                if negative {
                    out.push(b'-');
                    start = 1;
                }
                fill(out, padding, b'0');
                width = 0;
            }
        }
    }
    g_add(out, s.get(start..).unwrap_or(&[]), width, pad, case);
}

/// `DO_NUMBER` and `DO_NUMBER_SPACEPAD`, from glibc: at least `d` digits, or
/// the width if that is more.
#[allow(clippy::too_many_arguments, reason = "upstream's macro, and its state")]
fn g_number(
    out: &mut Vec<u8>,
    d: i64,
    value: i64,
    width: i64,
    mut pad: u8,
    spacepad: bool,
    case: Case,
) {
    let digits = if d > width { d } else { width };
    if spacepad && pad != b'0' && pad != b'-' {
        pad = b'_';
    }
    // `%O`: the locale's alternative digits, which the C locale has none of.
    let body = digits_of(u128::from(value.unsigned_abs()));
    g_sign_and_padding(out, &body, value < 0, digits, width, pad, case);
}

/// glibc's `__strftime_internal`, into `out`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "upstream's `int` arithmetic on `struct tm` fields, done on the `i64`s               of `Fields`, which every value a `Tm` can hold leaves far from overflow"
)]
#[allow(clippy::too_many_lines, reason = "one upstream function, ported whole")]
fn glibc(out: &mut Vec<u8>, format: &[u8], t: &Fields<'_>) {
    let hour12 = t.hour12();
    let mut f = 0usize;
    while at(format, f) != 0 {
        let c = at(format, f);
        if c != b'%' {
            out.push(c);
            f = f.saturating_add(1);
            continue;
        }

        let mut pad = 0u8;
        let mut width: i64 = -1;
        let mut to_lowcase = false;
        let mut to_uppcase = false;
        let mut change_case = false;

        // Flags.
        loop {
            f = f.saturating_add(1);
            match at(format, f) {
                b'_' | b'-' | b'0' => pad = at(format, f),
                b'^' => to_uppcase = true,
                b'#' => change_case = true,
                _ => break,
            }
        }

        // A width, a GNU extension.
        if at(format, f).is_ascii_digit() {
            width = 0;
            while at(format, f).is_ascii_digit() {
                let d = i64::from(at(format, f).wrapping_sub(b'0'));
                width = if width > INT_MAX / 10 || (width == INT_MAX / 10 && d > INT_MAX % 10) {
                    INT_MAX
                } else {
                    width.saturating_mul(10).saturating_add(d)
                };
                f = f.saturating_add(1);
            }
        }

        let modifier = match at(format, f) {
            b'E' | b'O' => {
                let m = at(format, f);
                f = f.saturating_add(1);
                m
            }
            _ => 0,
        };

        let format_char = at(format, f);
        // What to do; `None` is `bad_format`.
        let mut bad = false;
        match format_char {
            b'%' => {
                if modifier != 0 {
                    bad = true;
                } else {
                    g_add(out, b"%", width, pad, Case::Keep);
                }
            }
            b'a' | b'A' => {
                if modifier != 0 {
                    bad = true;
                } else {
                    if change_case {
                        to_uppcase = true;
                        to_lowcase = false;
                    }
                    let word = if format_char == b'a' {
                        t.a_wkday()
                    } else {
                        t.f_wkday()
                    };
                    g_add(out, word, width, pad, Case::of(to_lowcase, to_uppcase));
                }
            }
            b'b' | b'h' | b'B' => {
                if format_char == b'B' && modifier == b'E' {
                    bad = true;
                } else {
                    if change_case {
                        to_uppcase = true;
                        to_lowcase = false;
                    }
                    if modifier == b'E' {
                        bad = true;
                    } else {
                        // `%Ob`/`%OB`: the C locale's alternative month names
                        // are the ordinary ones.
                        let word = if format_char == b'B' {
                            t.f_month()
                        } else {
                            t.a_month()
                        };
                        g_add(out, word, width, pad, Case::of(to_lowcase, to_uppcase));
                    }
                }
            }
            b'c' | b'x' | b'X' | b'D' | b'F' | b'R' | b'r' | b'T' => {
                let sub: Option<&[u8]> = match format_char {
                    // The C locale has no era formats, so `%Ec` is `%c`.
                    b'c' if modifier != b'O' => Some(b"%a %b %e %H:%M:%S %Y"),
                    b'x' if modifier != b'O' => Some(b"%m/%d/%y"),
                    b'X' if modifier != b'O' => Some(b"%H:%M:%S"),
                    b'D' | b'F' if modifier == 0 => Some(if format_char == b'D' {
                        b"%m/%d/%y"
                    } else {
                        b"%Y-%m-%d"
                    }),
                    b'R' => Some(b"%H:%M"),
                    b'r' => Some(b"%I:%M:%S %p"),
                    b'T' => Some(b"%H:%M:%S"),
                    _ => None,
                };
                match sub {
                    Some(sub) => {
                        // `subformat`: render, pad the whole to the width, and
                        // upper-case it all for `^`.
                        let mut inner = Vec::new();
                        glibc(&mut inner, sub, t);
                        let mark = out.len();
                        g_add(out, &inner, width, pad, Case::Keep);
                        if to_uppcase {
                            if let Some(tail) = out.get_mut(mark..) {
                                tail.make_ascii_uppercase();
                            }
                        }
                    }
                    None => bad = true,
                }
            }
            b'C' => {
                // `%EC`: no era in the C locale.
                let year = t.year.saturating_add(TM_YEAR_BASE);
                let century = year / 100 - i64::from(year % 100 < 0);
                g_number(out, 1, century, width, pad, false, Case::Keep);
            }
            b'd' | b'e' | b'H' | b'I' | b'k' | b'l' | b'j' | b'M' | b'm' | b'S' | b'U' | b'W'
            | b'w' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    let (d, value, spacepad) = match format_char {
                        b'd' => (2, t.mday, false),
                        b'e' => (2, t.mday, true),
                        b'H' => (2, t.hour, false),
                        b'I' => (2, hour12, false),
                        b'k' => (2, t.hour, true),
                        b'l' => (2, hour12, true),
                        b'j' => (3, t.yday.saturating_add(1), false),
                        b'M' => (2, t.min, false),
                        b'm' => (2, t.mon.saturating_add(1), false),
                        b'S' => (2, t.sec, false),
                        b'U' => (2, (t.yday - t.wday + 7) / 7, false),
                        b'W' => (2, (t.yday - (t.wday - 1 + 7) % 7 + 7) / 7, false),
                        _ => (1, t.wday, false),
                    };
                    g_number(out, d, value, width, pad, spacepad, Case::Keep);
                }
            }
            b'n' => g_add(out, b"\n", width, pad, Case::Keep),
            b't' => g_add(out, b"\t", width, pad, Case::Keep),
            b'P' | b'p' => {
                if format_char == b'P' {
                    to_lowcase = true;
                }
                if change_case {
                    to_uppcase = false;
                    to_lowcase = true;
                }
                g_add(out, t.ampm(), width, pad, Case::of(to_lowcase, to_uppcase));
            }
            b's' => {
                let body = digits_of(u128::from(t.epoch.unsigned_abs()));
                g_sign_and_padding(out, &body, t.epoch < 0, 1, width, pad, Case::Keep);
            }
            b'u' => g_number(
                out,
                1,
                (t.wday - 1 + 7) % 7 + 1,
                width,
                pad,
                false,
                Case::Keep,
            ),
            b'V' | b'g' | b'G' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    let mut year = t.year.saturating_add(TM_YEAR_BASE);
                    let mut days = iso_week_days(t.yday, t.wday);
                    if days < 0 {
                        year = year.saturating_sub(1);
                        days = iso_week_days(t.yday + 365 + i64::from(isleap(year)), t.wday);
                    } else {
                        let d = iso_week_days(t.yday - (365 + i64::from(isleap(year))), t.wday);
                        if 0 <= d {
                            year = year.saturating_add(1);
                            days = d;
                        }
                    }
                    let (d, value) = match format_char {
                        b'g' => (2, (year % 100 + 100) % 100),
                        b'G' => (1, year),
                        _ => (2, days / 7 + 1),
                    };
                    g_number(out, d, value, width, pad, false, Case::Keep);
                }
            }
            b'Y' => {
                if modifier == b'O' {
                    bad = true;
                } else {
                    let year = t.year.saturating_add(TM_YEAR_BASE);
                    g_number(out, 1, year, width, pad, false, Case::Keep);
                }
            }
            b'y' => {
                let yy = (t.year % 100 + 100) % 100;
                g_number(out, 2, yy, width, pad, false, Case::Keep);
            }
            b'Z' => {
                if change_case {
                    to_uppcase = false;
                    to_lowcase = true;
                }
                g_add(out, t.zone, width, pad, Case::of(to_lowcase, to_uppcase));
            }
            b'z' => {
                if t.isdst >= 0 {
                    let mut diff = t.gmtoff;
                    if diff < 0 {
                        g_add(out, b"-", width, pad, Case::Keep);
                        diff = diff.saturating_neg();
                    } else {
                        g_add(out, b"+", width, pad, Case::Keep);
                    }
                    diff /= 60;
                    let value = (diff / 60) * 100 + diff % 60;
                    g_number(out, 4, value, width, pad, false, Case::Keep);
                }
            }
            0 => {
                // `%` at the end of the format: back up so the loop ends on
                // the NUL, and print what there was.
                f = f.saturating_sub(1);
                bad = true;
            }
            _ => bad = true,
        }

        if bad {
            // `bad_format`: everything from the nearest `%` at or before `f`.
            let mut start = f;
            while at(format, start) != b'%' && start > 0 {
                start = start.saturating_sub(1);
            }
            let text = format.get(start..=f).unwrap_or(&[]);
            g_add(out, text, width, pad, Case::of(to_lowcase, to_uppcase));
        }
        f = f.saturating_add(1);
    }
}

// ---------------------------------------------------------------------------
// gnulib
// ---------------------------------------------------------------------------

/// gnulib's `nstrftime`, in the C locale: `tm` rendered by `fmt`.
///
/// This is what coreutils' `date`, `ls`, `stat`, `du`, `pr` and `uptime`, and
/// diffutils, format with. `%s` is `tm.epoch`; [`nstrftime_z`] computes it as
/// upstream does, through `mktime`.
#[must_use]
pub fn nstrftime(fmt: &[u8], tm: &Tm) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len().saturating_add(16));
    gnulib(&mut out, fmt, &Fields::of(tm), None, false, 0, -1);
    out
}

/// [`nstrftime`] with the zone `tm` was produced in, so that `%s` is
/// `mktime_z (tz, tm)` -- with the call's effect on glibc's process-wide
/// `mktime` offset guess, which `date -f` can observe (module docs).
#[must_use]
pub fn nstrftime_z(fmt: &[u8], tm: &Tm, zone: &Zone) -> Vec<u8> {
    let mut out = Vec::with_capacity(fmt.len().saturating_add(16));
    gnulib(&mut out, fmt, &Fields::of(tm), Some(zone), false, 0, -1);
    out
}

/// gnulib's `width_add`: `bytes` right-aligned in `width`, unless the pad flag
/// is `-` -- zero-filled for `0` and `+`, space-filled otherwise -- in `case`.
fn n_add(out: &mut Vec<u8>, bytes: &[u8], width: i64, pad: u8, case: Case) {
    let n = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    let w = if pad == b'-' || width < 0 { 0 } else { width };
    if n < w {
        fill(
            out,
            w.saturating_sub(n),
            if pad == b'0' || pad == b'+' {
                b'0'
            } else {
                b' '
            },
        );
    }
    case.push(out, bytes);
}

/// The kinds of number gnulib formats, by the label its macros jump to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Num {
    /// `DO_NUMBER`: a signed `int`.
    Plain,
    /// `DO_NUMBER_SPACEPAD`: the same, space-padded by default.
    SpacePad,
    /// `DO_SIGNED_NUMBER`: sign given separately, value as `unsigned`.
    Signed,
    /// `DO_YEARISH`: signed, and under `+` a `+` for a long year.
    Yearish,
    /// `DO_TZ_OFFSET`: always signed, with colons by `mask`.
    TzOffset(u32),
}

/// gnulib's `do_number` ... `do_number_sign_and_padding`, for a magnitude
/// `magnitude` whose sign is `negative`, of at least `digits` digits.
#[allow(
    clippy::too_many_arguments,
    reason = "upstream's labels, and their state"
)]
fn n_number(
    out: &mut Vec<u8>,
    kind: Num,
    digits: i64,
    negative: bool,
    magnitude: u128,
    width: i64,
    mut pad: u8,
    yr_spec: u8,
) {
    let mut always_output_a_sign = false;
    let mut colon_mask = 0u32;
    match kind {
        Num::SpacePad => {
            if pad == 0 {
                pad = b'_';
            }
        }
        Num::Yearish => {
            if pad == 0 {
                pad = yr_spec;
            }
            let limit: u128 = if digits == 2 { 99 } else { 9999 };
            always_output_a_sign = pad == b'+' && (limit < magnitude || digits < width);
        }
        Num::TzOffset(mask) => {
            always_output_a_sign = true;
            colon_mask = mask;
        }
        Num::Plain | Num::Signed => {}
    }

    // do_number_body: the digits, with a colon before each digit whose bit is
    // set in the mask, counting from the least significant.
    let mut rev = Vec::new();
    let mut u = magnitude;
    loop {
        if colon_mask & 1 != 0 {
            rev.push(b':');
        }
        colon_mask >>= 1;
        rev.push(b'0'.saturating_add(u8::try_from(u % 10).unwrap_or(0)));
        u /= 10;
        if u == 0 && colon_mask == 0 {
            break;
        }
    }
    rev.reverse();
    n_sign_and_padding(
        out,
        &rev,
        negative,
        always_output_a_sign,
        digits,
        width,
        pad,
    );
}

/// gnulib's `do_number_sign_and_padding`.
fn n_sign_and_padding(
    out: &mut Vec<u8>,
    body: &[u8],
    negative: bool,
    always_output_a_sign: bool,
    digits: i64,
    mut width: i64,
    mut pad: u8,
) {
    if pad == 0 {
        pad = b'0';
    }
    if width < 0 {
        width = digits;
    }
    let sign_char = if negative {
        Some(b'-')
    } else if always_output_a_sign {
        Some(b'+')
    } else {
        None
    };
    let numlen = i64::try_from(body.len()).unwrap_or(i64::MAX);
    let shortage = width
        .saturating_sub(i64::from(sign_char.is_some()))
        .saturating_sub(numlen);
    let padding = if pad == b'-' || shortage <= 0 {
        0
    } else {
        shortage
    };
    if let Some(sign) = sign_char {
        if pad == b'_' {
            fill(out, padding, b' ');
            width = width.saturating_sub(padding);
        }
        out.push(sign);
        width = width.saturating_sub(1);
    }
    n_add(out, body, width, pad, Case::Keep);
}

/// `underlying_strftime`: what the C library's `strftime` makes of `%` plus
/// `modifier` plus `format_char` alone, which gnulib then pads and cases
/// itself.
fn underlying(t: &Fields<'_>, modifier: u8, format_char: u8) -> Vec<u8> {
    let mut ufmt = vec![b'%'];
    if modifier != 0 {
        ufmt.push(modifier);
    }
    ufmt.push(format_char);
    let mut out = Vec::new();
    glibc(&mut out, &ufmt, t);
    out
}

/// gnulib's `__strftime_internal`, into `out`.
///
/// `upcase` upper-cases every conversion (a `%^` sub-format); `yr_spec` is the
/// pad an enclosing `%F` gives its year; `width` is the width the first
/// directive inherits from one.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "upstream's `int` arithmetic on `struct tm` fields, done on the `i64`s               of `Fields`, which every value a `Tm` can hold leaves far from overflow"
)]
#[allow(clippy::too_many_lines, reason = "one upstream function, ported whole")]
#[allow(clippy::too_many_arguments, reason = "upstream's signature")]
fn gnulib(
    out: &mut Vec<u8>,
    format: &[u8],
    t: &Fields<'_>,
    zone: Option<&Zone>,
    upcase: bool,
    yr_spec: u8,
    mut width: i64,
) {
    let hour12 = t.hour12();
    let mut f = 0usize;
    while at(format, f) != 0 {
        let c = at(format, f);
        let mut pad = 0u8;
        let mut to_lowcase = false;
        let mut to_uppcase = upcase;
        if c != b'%' {
            // `add1`, which the first iteration of a sub-format pads to the
            // inherited width.
            n_add(out, &[c], width, pad, Case::Keep);
            f = f.saturating_add(1);
            width = -1;
            continue;
        }
        let percent = f;
        let mut change_case = false;

        // Flags.
        loop {
            f = f.saturating_add(1);
            match at(format, f) {
                b'_' | b'-' | b'+' | b'0' => pad = at(format, f),
                b'^' => to_uppcase = true,
                b'#' => change_case = true,
                _ => break,
            }
        }

        if at(format, f).is_ascii_digit() {
            width = 0;
            while at(format, f).is_ascii_digit() {
                let d = i64::from(at(format, f).wrapping_sub(b'0'));
                width = width
                    .checked_mul(10)
                    .and_then(|w| w.checked_add(d))
                    .filter(|&w| w <= INT_MAX)
                    .unwrap_or(INT_MAX);
                f = f.saturating_add(1);
            }
        }

        let modifier = match at(format, f) {
            b'E' | b'O' => {
                let m = at(format, f);
                f = f.saturating_add(1);
                m
            }
            _ => 0,
        };

        let mut format_char = at(format, f);
        let mut bad = false;
        // A number to format: (kind, digits, negative, magnitude).
        let mut number: Option<(Num, i64, bool, u128)> = None;
        // A C-library conversion to pad and case.
        let mut via_libc = false;

        /// A signed `int` value, as `DO_NUMBER` takes it.
        fn plain(v: i64) -> (bool, u128) {
            (v < 0, u128::from(v.unsigned_abs()))
        }

        match format_char {
            b'%' => {
                if f.saturating_sub(1) != percent {
                    // `bad_percent`: `%` after flags, width or a modifier.
                    f = f.saturating_sub(1);
                    bad = true;
                } else {
                    n_add(out, b"%", width, pad, Case::Keep);
                }
            }
            b'a' | b'A' => {
                if modifier != 0 {
                    bad = true;
                } else {
                    if change_case {
                        to_uppcase = true;
                        to_lowcase = false;
                    }
                    via_libc = true;
                }
            }
            b'b' | b'h' => {
                if change_case {
                    to_uppcase = true;
                    to_lowcase = false;
                }
                if modifier == b'E' {
                    bad = true;
                } else {
                    via_libc = true;
                }
            }
            b'B' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    if change_case {
                        to_uppcase = true;
                        to_lowcase = false;
                    }
                    via_libc = true;
                }
            }
            b'c' | b'x' | b'X' => {
                if modifier == b'O' {
                    bad = true;
                } else {
                    via_libc = true;
                }
            }
            b'C' => {
                if modifier == b'E' {
                    via_libc = true;
                } else {
                    let negative_year = t.year < -TM_YEAR_BASE;
                    let zero_thru_1899 = !negative_year && t.year < 0;
                    let century =
                        (t.year - 99 * i64::from(zero_thru_1899)) / 100 + TM_YEAR_BASE / 100;
                    // `DO_YEARISH (2, negative_year, century)`: `century` goes
                    // in as `unsigned`, so a negative one is its magnitude.
                    number = Some((
                        Num::Yearish,
                        2,
                        negative_year,
                        u128::from(century.unsigned_abs()),
                    ));
                }
            }
            b'D' => {
                if modifier != 0 {
                    bad = true;
                } else {
                    n_sub(out, b"%m/%d/%y", t, zone, to_uppcase, pad, -1, width);
                }
            }
            b'd' | b'e' | b'H' | b'I' | b'k' | b'l' | b'M' | b'S' | b'U' | b'W' | b'w' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    let (kind, d, v) = match format_char {
                        b'd' => (Num::Plain, 2, t.mday),
                        b'e' => (Num::SpacePad, 2, t.mday),
                        b'H' => (Num::Plain, 2, t.hour),
                        b'I' => (Num::Plain, 2, hour12),
                        b'k' => (Num::SpacePad, 2, t.hour),
                        b'l' => (Num::SpacePad, 2, hour12),
                        b'M' => (Num::Plain, 2, t.min),
                        b'S' => (Num::Plain, 2, t.sec),
                        b'U' => (Num::Plain, 2, (t.yday - t.wday + 7) / 7),
                        b'W' => (Num::Plain, 2, (t.yday - (t.wday - 1 + 7) % 7 + 7) / 7),
                        _ => (Num::Plain, 1, t.wday),
                    };
                    let (neg, mag) = plain(v);
                    number = Some((kind, d, neg, mag));
                }
            }
            b'F' => {
                if modifier != 0 {
                    bad = true;
                } else {
                    // `%+4Y-%m-%d` unless a pad or width was given; then the
                    // year gets what is left of the width after `-mm-dd`.
                    let subwidth = if pad == 0 && width < 0 {
                        pad = b'+';
                        4
                    } else {
                        width.saturating_sub(6).max(0)
                    };
                    n_sub(out, b"%Y-%m-%d", t, zone, to_uppcase, pad, subwidth, width);
                }
            }
            b'j' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    // `DO_SIGNED_NUMBER (3, tm_yday < -1, tm_yday + 1U)`.
                    let v = t.yday.saturating_add(1);
                    number = Some((Num::Signed, 3, t.yday < -1, u128::from(v.unsigned_abs())));
                }
            }
            b'm' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    let v = t.mon.saturating_add(1);
                    number = Some((Num::Signed, 2, t.mon < -1, u128::from(v.unsigned_abs())));
                }
            }
            b'N' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    let mut n = t.ns;
                    let ns_digits: i64 = 9;
                    if width <= 0 {
                        width = ns_digits;
                    }
                    let mut ndigs = ns_digits;
                    while width < ndigs || (1 < ndigs && n % 10 == 0) {
                        ndigs = ndigs.saturating_sub(1);
                        n /= 10;
                    }
                    let mut buf = vec![b'0'; usize::try_from(ndigs).unwrap_or(0)];
                    for slot in buf.iter_mut().rev() {
                        *slot = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
                        n /= 10;
                    }
                    if pad == 0 {
                        pad = b'0';
                    }
                    // `width_cpy (0, …)`, then the padding *after* it.
                    Case::Keep.push(out, &buf);
                    let rest = width.saturating_sub(ndigs);
                    if pad != b'-' && rest > 0 {
                        fill(
                            out,
                            rest,
                            if pad == b'0' || pad == b'+' {
                                b'0'
                            } else {
                                b' '
                            },
                        );
                    }
                }
            }
            b'n' => n_add(out, b"\n", width, pad, Case::Keep),
            b't' => n_add(out, b"\t", width, pad, Case::Keep),
            b'P' | b'p' => {
                if format_char == b'P' {
                    to_lowcase = true;
                    format_char = b'p';
                }
                if change_case {
                    to_uppcase = false;
                    to_lowcase = true;
                }
                via_libc = true;
            }
            b'q' => {
                // `DO_SIGNED_NUMBER (1, false, ((tm_mon * 11) >> 5) + 1)`.
                let v = ((t.mon * 11) >> 5) + 1;
                number = Some((Num::Signed, 1, false, u128::from(v.unsigned_abs())));
            }
            b'R' => n_sub(out, b"%H:%M", t, zone, to_uppcase, pad, -1, width),
            b'r' => via_libc = true,
            b's' => {
                let secs = match zone {
                    Some(z) => mktime_side_effect(z, t),
                    None => t.epoch,
                };
                let body = digits_of(u128::from(secs.unsigned_abs()));
                n_sign_and_padding(out, &body, secs < 0, false, 1, width, pad);
            }
            b'T' => n_sub(out, b"%H:%M:%S", t, zone, to_uppcase, pad, -1, width),
            b'u' => {
                let (neg, mag) = plain((t.wday - 1 + 7) % 7 + 1);
                number = Some((Num::Plain, 1, neg, mag));
            }
            b'V' | b'g' | b'G' => {
                if modifier == b'E' {
                    bad = true;
                } else {
                    // Upstream keeps the year near 2000 so its `int` cannot
                    // overflow; only its leap-ness is used.
                    let year = t.year
                        + if t.year < 0 {
                            1900 % 400
                        } else {
                            1900 % 400 - 400
                        };
                    let mut year_adjust = 0i64;
                    let mut days = iso_week_days(t.yday, t.wday);
                    if days < 0 {
                        year_adjust = -1;
                        days = iso_week_days(t.yday + (365 + i64::from(isleap(year - 1))), t.wday);
                    } else {
                        let d = iso_week_days(t.yday - (365 + i64::from(isleap(year))), t.wday);
                        if 0 <= d {
                            year_adjust = 1;
                            days = d;
                        }
                    }
                    match format_char {
                        b'g' => {
                            let yy = (t.year % 100 + year_adjust) % 100;
                            let v = if 0 <= yy {
                                yy
                            } else if t.year < -TM_YEAR_BASE - year_adjust {
                                -yy
                            } else {
                                yy + 100
                            };
                            number = Some((Num::Yearish, 2, false, u128::from(v.unsigned_abs())));
                        }
                        b'G' => {
                            let full = t.year + TM_YEAR_BASE + year_adjust;
                            number = Some((
                                Num::Yearish,
                                4,
                                t.year < -TM_YEAR_BASE - year_adjust,
                                u128::from(full.unsigned_abs()),
                            ));
                        }
                        _ => {
                            let (neg, mag) = plain(days / 7 + 1);
                            number = Some((Num::Plain, 2, neg, mag));
                        }
                    }
                }
            }
            b'Y' => {
                if modifier == b'E' {
                    via_libc = true;
                } else if modifier == b'O' {
                    bad = true;
                } else {
                    let full = t.year + TM_YEAR_BASE;
                    number = Some((
                        Num::Yearish,
                        4,
                        t.year < -TM_YEAR_BASE,
                        u128::from(full.unsigned_abs()),
                    ));
                }
            }
            b'y' => {
                if modifier == b'E' {
                    via_libc = true;
                } else {
                    let mut yy = t.year % 100;
                    if yy < 0 {
                        yy = if t.year < -TM_YEAR_BASE {
                            -yy
                        } else {
                            yy + 100
                        };
                    }
                    number = Some((Num::Yearish, 2, false, u128::from(yy.unsigned_abs())));
                }
            }
            b'Z' => {
                if change_case {
                    to_uppcase = false;
                    to_lowcase = true;
                }
                n_add(out, t.zone, width, pad, Case::of(to_lowcase, to_uppcase));
            }
            b':' | b'z' => {
                let mut colons = 0usize;
                if format_char == b':' {
                    colons = 1;
                    while at(format, f.saturating_add(colons)) == b':' {
                        colons = colons.saturating_add(1);
                    }
                    if at(format, f.saturating_add(colons)) != b'z' {
                        bad = true;
                    } else {
                        f = f.saturating_add(colons);
                    }
                }
                if !bad && t.isdst >= 0 {
                    let diff = t.gmtoff;
                    let negative = diff < 0 || (diff == 0 && t.zone.first() == Some(&b'-'));
                    let hour_diff = (diff / 60 / 60).unsigned_abs();
                    let min_diff = (diff / 60 % 60).unsigned_abs();
                    let sec_diff = (diff % 60).unsigned_abs();
                    let hh_mm = u128::from(hour_diff * 100 + min_diff);
                    let hh_mm_ss = u128::from(hour_diff * 10000 + min_diff * 100 + sec_diff);
                    let spec = match colons {
                        0 => Some((5, 0, hh_mm)),
                        1 => Some((6, 0o4, hh_mm)),
                        2 => Some((9, 0o24, hh_mm_ss)),
                        3 => Some(if sec_diff != 0 {
                            (9, 0o24, hh_mm_ss)
                        } else if min_diff != 0 {
                            (6, 0o4, hh_mm)
                        } else {
                            (3, 0, u128::from(hour_diff))
                        }),
                        _ => None,
                    };
                    match spec {
                        Some((d, mask, mag)) => {
                            number = Some((Num::TzOffset(mask), d, negative, mag));
                        }
                        None => bad = true,
                    }
                }
            }
            0 => {
                // `%` at the end of the format.
                f = f.saturating_sub(1);
                bad = true;
            }
            _ => bad = true,
        }

        if via_libc {
            let text = underlying(t, modifier, format_char);
            n_add(out, &text, width, pad, Case::of(to_lowcase, to_uppcase));
        }
        if let Some((kind, digits, negative, magnitude)) = number {
            if modifier == b'O' && !negative {
                // `%O`: the C library's own rendering, padded by gnulib.
                let text = underlying(t, modifier, format_char);
                n_add(out, &text, width, pad, Case::of(to_lowcase, to_uppcase));
            } else {
                n_number(out, kind, digits, negative, magnitude, width, pad, yr_spec);
            }
        }
        if bad {
            // `bad_format`: the directive as written, from its `%`.
            let text = format.get(percent..=f).unwrap_or(&[]);
            n_add(out, text, width, pad, Case::of(to_lowcase, to_uppcase));
        }
        f = f.saturating_add(1);
        width = -1;
    }
}

/// gnulib's `subformat_width`: `sub` rendered with `upcase`, `pad` as its
/// year's pad and `subwidth` as its first directive's width, and the whole
/// then padded to the outer `width` by `pad`.
#[allow(clippy::too_many_arguments, reason = "upstream's call, and its state")]
fn n_sub(
    out: &mut Vec<u8>,
    sub: &[u8],
    t: &Fields<'_>,
    zone: Option<&Zone>,
    upcase: bool,
    pad: u8,
    subwidth: i64,
    width: i64,
) {
    let mut inner = Vec::new();
    gnulib(&mut inner, sub, t, zone, upcase, pad, subwidth);
    n_add(out, &inner, width, pad, Case::Keep);
}

/// `%s` as upstream computes it: `mktime_z (tz, &ltm)` on the broken-down
/// time, which for a `Tm` from `zone` is its epoch -- called for its effect on
/// the process-wide offset guess.
fn mktime_side_effect(zone: &Zone, t: &Fields<'_>) -> i64 {
    let fits = |v: i64| i32::try_from(v).ok();
    let tm = (|| {
        Some(StructTm {
            tm_sec: fits(t.sec)?,
            tm_min: fits(t.min)?,
            tm_hour: fits(t.hour)?,
            tm_mday: fits(t.mday)?,
            tm_mon: fits(t.mon)?,
            tm_year: fits(t.year)?,
            tm_isdst: fits(t.isdst)?,
            ..StructTm::default()
        })
    })();
    match tm {
        Some(mut tm) => zone.mktime(&mut tm).unwrap_or(t.epoch),
        None => t.epoch,
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

    /// 2001-09-09 01:46:40 UTC, a Sunday, with `ns` nanoseconds.
    fn sunday(ns: u32) -> Tm {
        Zone::utc().local(1_000_000_000, ns)
    }

    fn glibc_fmt(f: &str, tm: &Tm) -> String {
        String::from_utf8(strftime(f.as_bytes(), tm)).unwrap()
    }

    fn gnulib_fmt(f: &str, tm: &Tm) -> String {
        String::from_utf8(nstrftime(f.as_bytes(), tm)).unwrap()
    }

    /// Measured from glibc 2.39 (`time.strftime`, which reaches the same
    /// code for everything but `%Z`'s case flags).
    #[test]
    fn glibc_pads_even_under_the_dash_flag() {
        let tm = sunday(0);
        assert_eq!(glibc_fmt("%-5d", &tm), "    9");
        assert_eq!(glibc_fmt("%_5d", &tm), "    9");
        assert_eq!(glibc_fmt("%05d", &tm), "00009");
        assert_eq!(glibc_fmt("%5d", &tm), "00009");
        assert_eq!(glibc_fmt("%-d", &tm), "9");
        assert_eq!(glibc_fmt("%-10a", &tm), "       Sun");
        assert_eq!(glibc_fmt("%010a", &tm), "0000000Sun");
        assert_eq!(glibc_fmt("%-10Y", &tm), "      2001");
        assert_eq!(glibc_fmt("%010Y", &tm), "0000002001");
    }

    #[test]
    fn glibc_pads_the_zone_sign_and_the_number_separately() {
        let tm = sunday(0);
        assert_eq!(glibc_fmt("%10z", &tm), "         +0000000000");
        assert_eq!(glibc_fmt("%z", &tm), "+0000");
    }

    #[test]
    fn glibc_does_not_know_gnulibs_extensions() {
        let tm = sunday(0);
        assert_eq!(glibc_fmt("%N", &tm), "%N");
        assert_eq!(glibc_fmt("%q", &tm), "%q");
        assert_eq!(glibc_fmt("%:z", &tm), "%:z");
        assert_eq!(glibc_fmt("%+Y", &tm), "%+Y");
    }

    #[test]
    fn glibc_percent_takes_a_width_and_refuses_a_modifier() {
        let tm = sunday(0);
        assert_eq!(glibc_fmt("%5%", &tm), "    %");
        // `%E%` is a bad format, printed from the nearest `%` back -- which
        // is the second one.
        assert_eq!(glibc_fmt("%E%", &tm), "%");
    }

    #[test]
    fn glibc_years_are_not_padded() {
        let year21 = Zone::utc().local(-61_490_188_800, 0);
        assert_eq!(glibc_fmt("%Y", &year21), "21");
        assert_eq!(glibc_fmt("%C", &year21), "0");
        assert_eq!(glibc_fmt("%F", &year21), "21-06-15");
        assert_eq!(glibc_fmt("%c", &year21), "Tue Jun 15 00:00:00 21");
    }

    /// Measured from coreutils 9.4's `date`, which formats with `nstrftime`.
    #[test]
    fn gnulib_pads_years_to_four_and_signs_a_long_one_in_f() {
        let year21 = Zone::utc().local(-61_490_188_800, 0);
        assert_eq!(gnulib_fmt("%Y", &year21), "0021");
        assert_eq!(gnulib_fmt("%F", &year21), "0021-06-15");
        assert_eq!(gnulib_fmt("%G", &year21), "0021");
        assert_eq!(gnulib_fmt("%C", &year21), "00");
        assert_eq!(gnulib_fmt("%y", &year21), "21");
        let year10000 = Zone::utc().local(253_402_300_800, 0);
        assert_eq!(gnulib_fmt("%Y", &year10000), "10000");
        assert_eq!(gnulib_fmt("%F", &year10000), "+10000-01-01");
        assert_eq!(gnulib_fmt("%C", &year10000), "100");
    }

    #[test]
    fn gnulib_hands_the_locales_words_to_the_c_library() {
        let year21 = Zone::utc().local(-61_490_188_800, 0);
        // `%c` is glibc's, unpadded year and all.
        assert_eq!(gnulib_fmt("%c", &year21), "Tue Jun 15 00:00:00 21");
        assert_eq!(gnulib_fmt("%^c", &year21), "TUE JUN 15 00:00:00 21");
        // `%Od` is glibc's `09`, then padded by gnulib -- with spaces.
        assert_eq!(gnulib_fmt("%5Od", &year21), "   15");
    }

    #[test]
    fn gnulib_n_takes_its_width_as_a_precision() {
        let tm = sunday(500_000_000);
        assert_eq!(gnulib_fmt("%N", &tm), "500000000");
        assert_eq!(gnulib_fmt("%3N", &tm), "500");
        assert_eq!(gnulib_fmt("%_3N", &tm), "5  ");
        assert_eq!(gnulib_fmt("%_N", &tm), "5        ");
        assert_eq!(gnulib_fmt("%12N", &tm), "500000000000");
        assert_eq!(gnulib_fmt("%1N", &sunday(123_456_789)), "1");
    }

    #[test]
    fn gnulib_colon_zones() {
        let info = tzrules::TzInfo {
            gmtoff: 5 * 3600 + 30 * 60,
            is_dst: false,
            name: tzrules::TzName::UTC,
        };
        let tm = Tm::from_utc(0, 0, info);
        assert_eq!(gnulib_fmt("%z", &tm), "+0530");
        assert_eq!(gnulib_fmt("%:z", &tm), "+05:30");
        assert_eq!(gnulib_fmt("%::z", &tm), "+05:30:00");
        assert_eq!(gnulib_fmt("%:::z", &tm), "+05:30");
        assert_eq!(gnulib_fmt("%::::z", &tm), "%::::z");
        assert_eq!(gnulib_fmt("%:x", &tm), "%:x");
    }

    #[test]
    fn gnulib_percent_after_a_flag_is_printed_and_restarts() {
        let tm = sunday(0);
        // `bad_percent` backs up one and copies `%5` through `cpy`, which pads
        // to the width like any other output; the second `%` then starts `%d`.
        assert_eq!(gnulib_fmt("%5%d", &tm), "   %509");
        assert_eq!(gnulib_fmt("abc%", &tm), "abc%");
        assert_eq!(gnulib_fmt("%%", &tm), "%");
    }

    #[test]
    fn s_moves_the_offset_guess_as_mktime_does() {
        let tm = sunday(0);
        assert_eq!(
            String::from_utf8(nstrftime_z(b"%s", &tm, &Zone::utc())).unwrap(),
            "1000000000"
        );
    }
}
