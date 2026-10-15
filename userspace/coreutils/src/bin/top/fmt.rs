//! "Basic Formatting support": every column value of a task row is one of
//! these -- a number, a string, a scaled amount of memory, a percentage or
//! a time -- justified to its width and followed by the column pad.
//!
//! Each keeps upstream's buffer sizes where a value could reach them (a
//! `make_str` of a long command line is cut at `SCREENMAX - 1` bytes), and
//! upstream's arithmetic: the memory and percentage figures are C `float`s,
//! divided by `1024.0` in `double` and stored back.

use crate::cfmt::{Arg, snprintf};
use crate::defs::{COLPLUSCH, EU_MAXPFLGS, SCREENMAX, SMLBUFSIZ};
use crate::utf8;

/// `TICS_AS_*`: the coarsest unit `scale_tics` may start from.
pub const TICS_AS_SECS: i32 = 0;
pub const TICS_AS_MINS: i32 = 1;
pub const TICS_AS_HOUR: i32 = 2;
pub const TICS_AS_DAY1: i32 = 3;
pub const TICS_AS_DAY2: i32 = 4;
pub const TICS_AS_WEEK: i32 = 5;
pub const TICS_AS_LAST: i32 = 6;

/// `Scaled_sfxtab`: the suffixes of `k`ibibytes through `e`xbibytes.
const SFX: [u8; 6] = *b"kmgtpe";

/// `AUTOX_NO`: a column that never widens itself.
pub const AUTOX_NO: i32 = EU_MAXPFLGS as i32;

/// `Autox_array` and `Autox_found`: the columns whose values were cut since
/// the last frame, which an auto-sized `Rc.fixed_widest` (-1) widens.
#[derive(Clone, Debug)]
pub struct Autox {
    pub array: [bool; EU_MAXPFLGS],
    pub found: bool,
}

impl Default for Autox {
    fn default() -> Self {
        Self {
            array: [false; EU_MAXPFLGS],
            found: false,
        }
    }
}

impl Autox {
    /// `AUTOX_COL (f)`.
    pub fn col(&mut self, f: i32) {
        if let Ok(i) = usize::try_from(f)
            && let Some(slot) = self.array.get_mut(i)
        {
            *slot = true;
            self.found = true;
        }
    }
}

/// What the scaling functions read of the rcfile and the system.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    /// `Rc.zero_suppress`.
    pub zero_suppress: bool,
    /// `Rc.tics_scaled`.
    pub tics_scaled: i32,
    /// `Hertz`.
    pub hertz: i64,
}

/// `snprintf (buf, size, ...)`'s text, and the length the whole would have
/// had, as an `int` for the comparisons upstream makes with it.
fn sn(size: usize, fmt: &[u8], args: &[Arg<'_>]) -> (Vec<u8>, i32) {
    let (text, full) = snprintf(size, fmt, args);
    (text, i32::try_from(full).unwrap_or(i32::MAX))
}

/// `justify_pad (str, width, justr)`: `str` cut to `width` bytes and padded
/// to them, right-justified with `justr`, then the column pad.
#[must_use]
pub fn justify_pad(s: &[u8], width: i32, justr: bool) -> Vec<u8> {
    let fmt: &[u8] = if justr { b"%*.*s%s" } else { b"%-*.*s%s" };
    snprintf(
        SCREENMAX,
        fmt,
        &[
            Arg::I(i64::from(width)),
            Arg::I(i64::from(width)),
            Arg::S(s),
            Arg::S(crate::defs::COLPADSTR),
        ],
    )
    .0
}

/// `make_chr (ch, width, justr)`.
#[must_use]
pub fn make_chr(ch: u8, width: i32, justr: bool) -> Vec<u8> {
    // `snprintf (buf, sizeof (buf), "%c", ch)`: a NUL character makes the
    // empty string.
    let one: &[u8] = if ch == 0 {
        b""
    } else {
        std::slice::from_ref(&ch)
    };
    justify_pad(one, width, justr)
}

/// The cut a too-long value gets: its `width`'th byte made `+` and the rest
/// gone -- in a buffer of `size` bytes that holds `buf`, NUL-terminated. A
/// width the buffer cannot hold is `size - 1`; where that puts the `+` past
/// the string's end the string is left as it was, its terminator unmoved.
fn plus_cut(buf: &mut Vec<u8>, width: &mut i32, size: usize) {
    let max = i32::try_from(size).unwrap_or(i32::MAX);
    if *width <= 0 || *width >= max {
        *width = max.saturating_sub(1);
    }
    let w = usize::try_from(*width).unwrap_or(0);
    let at = w.saturating_sub(1);
    match at.cmp(&buf.len()) {
        std::cmp::Ordering::Less => {
            if let Some(b) = buf.get_mut(at) {
                *b = COLPLUSCH;
            }
            buf.truncate(w);
        }
        std::cmp::Ordering::Equal => buf.push(COLPLUSCH),
        std::cmp::Ordering::Greater => {}
    }
}

/// `make_num (num, width, justr, col, noz)`: a number too wide for its
/// column ends in `+`, and its column is marked to widen.
#[must_use]
pub fn make_num(
    sc: &Scale,
    autox: &mut Autox,
    num: i64,
    width: i32,
    justr: bool,
    col: i32,
    noz: bool,
) -> Vec<u8> {
    let mut width = width;
    if noz && sc.zero_suppress && num == 0 {
        return justify_pad(b"", width, justr);
    }
    let (mut buf, len) = sn(SMLBUFSIZ, b"%ld", &[Arg::I(num)]);
    if width < len {
        plus_cut(&mut buf, &mut width, SMLBUFSIZ);
        autox.col(col);
    }
    justify_pad(&buf, width, justr)
}

/// `make_str (str, width, justr, col)`.
#[must_use]
pub fn make_str(autox: &mut Autox, s: &[u8], width: i32, justr: bool, col: i32) -> Vec<u8> {
    let mut width = width;
    let (mut buf, len) = sn(SCREENMAX, b"%s", &[Arg::S(s)]);
    if width < len {
        plus_cut(&mut buf, &mut width, SCREENMAX);
        autox.col(col);
    }
    justify_pad(&buf, width, justr)
}

/// `make_str_utf8 (str, width, justr, col)`: [`make_str`] in columns rather
/// than bytes.
#[must_use]
pub fn make_str_utf8(autox: &mut Autox, s: &[u8], width: i32, justr: bool, col: i32) -> Vec<u8> {
    let mut delta = utf8::delta(s);
    let (mut buf, len) = sn(SCREENMAX, b"%s", &[Arg::S(s)]);
    if width.saturating_add(delta) < len {
        buf = snprintf(
            SCREENMAX,
            b"%.*s%c",
            &[
                Arg::I(i64::from(utf8::embody(s, width.saturating_sub(1)))),
                Arg::S(s),
                Arg::C(COLPLUSCH),
            ],
        )
        .0;
        delta = utf8::delta(&buf);
        autox.col(col);
    }
    justify_pad(&buf, width.saturating_add(delta), justr)
}

/// `num /= 1024.0` for a C `float`: divided in `double`, stored back.
// The quotient of a `float` by a power of two is exact in `double` and fits
// a `float` again (short of the subnormals): the cast loses nothing a C
// assignment would keep.
#[allow(clippy::cast_possible_truncation)]
fn kib_down(num: f32) -> f32 {
    (f64::from(num) / 1024.0) as f32
}

/// `scale_mem (target, num, width, justr)`: `num` KiB in the first unit at
/// or above `target` that fits `width`, or `?`.
#[must_use]
pub fn scale_mem(sc: &Scale, target: i32, num: f32, width: i32, justr: bool) -> Vec<u8> {
    const FMTTAB: [&[u8]; 5] = [b"%.0f", b"%.1f%c", b"%.1f%c", b"%.1f%c", b"%.1f%c"];
    if sc.zero_suppress && 0.0 >= num {
        return justify_pad(b"", width, justr);
    }
    let mut num = num;
    for (i, (fmt, sfx)) in FMTTAB.iter().zip(SFX).enumerate() {
        let i = i32::try_from(i).unwrap_or(0);
        if i >= target {
            let (buf, len) = sn(SMLBUFSIZ, fmt, &[Arg::F(f64::from(num)), Arg::C(sfx)]);
            if width >= len {
                return justify_pad(&buf, width, justr);
            }
        }
        num = kib_down(num);
    }
    justify_pad(b"?", width, justr)
}

/// `scale_num (num, width, justr)`: a count, in thousands-of-1024 units if
/// it must be.
#[must_use]
pub fn scale_num(sc: &Scale, num: f32, width: i32, justr: bool) -> Vec<u8> {
    if sc.zero_suppress && 0.0 >= num {
        return justify_pad(b"", width, justr);
    }
    let (buf, len) = sn(SMLBUFSIZ, b"%.0f", &[Arg::F(f64::from(num))]);
    if width >= len {
        return justify_pad(&buf, width, justr);
    }
    let mut num = num;
    for sfx in SFX {
        num = kib_down(num);
        for fmt in [&b"%.1f%c"[..], b"%.0f%c"] {
            let (buf, len) = sn(SMLBUFSIZ, fmt, &[Arg::F(f64::from(num)), Arg::C(sfx)]);
            if width >= len {
                return justify_pad(&buf, width, justr);
            }
        }
    }
    justify_pad(b"?", width, justr)
}

/// `scale_pcnt (num, width, justr, xtra)`: a percentage at the most
/// precision `width` allows -- three or two places only with `xtra`.
#[must_use]
pub fn scale_pcnt(sc: &Scale, num: f32, width: i32, justr: bool, xtra: bool) -> Vec<u8> {
    if sc.zero_suppress && 0.0 >= num {
        return justify_pad(b"", width, justr);
    }
    let v = Arg::F(f64::from(num));
    if xtra {
        for fmt in [&b"%#.3f"[..], b"%#.2f"] {
            let (buf, len) = sn(SMLBUFSIZ, fmt, &[v]);
            if width >= len {
                return justify_pad(&buf, width, justr);
            }
        }
    }
    let (buf, len) = sn(SMLBUFSIZ, b"%#.1f", &[v]);
    if width >= len {
        return justify_pad(&buf, width, justr);
    }
    let (buf, len) = sn(SMLBUFSIZ, b"%*.0f", &[Arg::I(i64::from(width)), v]);
    if width >= len {
        return justify_pad(&buf, width, justr);
    }
    justify_pad(b"?", width, justr)
}

/// `scale_tics (tics, width, justr, target)`: clock ticks as a time, in the
/// finest form at or after `target` that fits `width` -- the forms
/// `uptime` uses, each tried only below its limit (six hours of minutes,
/// four days of hours, two weeks of days).
#[must_use]
pub fn scale_tics(sc: &Scale, tics: u64, width: i32, justr: bool, target: i32) -> Vec<u8> {
    const MM_LIMIT: u64 = 360;
    const HH_LIMIT: u64 = 96;
    const DD_LIMIT: u64 = 14;
    let hertz = u64::from_le_bytes(sc.hertz.to_le_bytes());
    let nt = tics.wrapping_mul(100).checked_div(hertz).unwrap_or(0);
    if sc.zero_suppress && nt == 0 {
        return justify_pad(b"", width, justr);
    }
    let cent = nt % 100;
    let secs = nt / 100;
    let mins = secs / 60;
    let hour = mins / 60;
    let days = hour / 24;
    let week = days / 7;

    let mut target = target;
    if sc.tics_scaled > target {
        target = sc.tics_scaled;
    }
    let u = Arg::U;
    let try_form = |fmt: &[u8], args: &[Arg<'_>]| -> Option<Vec<u8>> {
        let (buf, len) = sn(SMLBUFSIZ, fmt, args);
        (width >= len).then_some(buf)
    };
    // The cases fall through from `target` on; the day forms share one
    // guard, which the two-day case is entered beneath when it is the
    // target.
    let mut step = target;
    while (TICS_AS_SECS..=TICS_AS_LAST).contains(&step) {
        let got = match step {
            TICS_AS_SECS if mins < MM_LIMIT + 1 => {
                try_form(b"%lu:%02lu.%02lu", &[u(mins), u(secs % 60), u(cent)])
            }
            TICS_AS_MINS if mins < MM_LIMIT + 1 => try_form(b"%lu:%02lu", &[u(mins), u(secs % 60)]),
            TICS_AS_HOUR if hour < HH_LIMIT + 1 => try_form(b"%lu,%02lu", &[u(hour), u(mins % 60)]),
            TICS_AS_DAY1 => {
                if days < DD_LIMIT + 1 {
                    try_form(b"%lud+%luh", &[u(days), u(hour % 24)])
                } else {
                    // Past the guard: the two-day form inside it is skipped.
                    step = TICS_AS_WEEK;
                    continue;
                }
            }
            TICS_AS_DAY2 => try_form(b"%lud", &[u(days)]),
            TICS_AS_WEEK => try_form(b"%luw+%lud", &[u(week), u(days % 7)]),
            TICS_AS_LAST => try_form(b"%luw", &[u(week)]),
            _ => None,
        };
        if let Some(buf) = got {
            return justify_pad(&buf, width, justr);
        }
        step = step.saturating_add(1);
    }
    justify_pad(b"?", width, justr)
}

/// `hex_make (num, noz)`: `%08lx`, its zeros made dots with `noz`.
#[must_use]
pub fn hex_make(num: i64, noz: bool) -> Vec<u8> {
    let mut buf = snprintf(SMLBUFSIZ, b"%08lx", &[Arg::I(num)]).0;
    if noz {
        for b in &mut buf {
            if *b == b'0' {
                *b = b'.';
            }
        }
    }
    buf
}

/// `(float) strtol (s, &ep, 0)` as `mkfloat` takes a whole number: the
/// value, if all of `s` was the number and it is below `INT_MAX` as a
/// `float` compares it (2147483648).
// `(float)` of a `long`: C rounds to the nearest `float`, as `as` does.
#[allow(clippy::cast_precision_loss)]
fn whole(s: &[u8]) -> Option<f32> {
    let (v, used) = cstrtol::strtol(s, 0);
    let num = v as f32;
    (used != 0 && used == s.len() && num < 2_147_483_648.0).then_some(num)
}

/// `strtof` in a locale whose decimal point is `radix`: the value and how
/// many bytes it took.
pub fn strtof_in(s: &[u8], radix: u8) -> (f32, usize) {
    if radix == b'.' {
        let r = cprintf::extfloat::strtof(s);
        return (r.value, r.consumed);
    }
    // The locale's point read as C's, and C's as no part of a number.
    let mapped: Vec<u8> = s
        .iter()
        .map(|&b| match b {
            b'.' => 0x01,
            b if b == radix => b'.',
            b => b,
        })
        .collect();
    let r = cprintf::extfloat::strtof(&mapped);
    (r.value, r.consumed)
}

/// `mkfloat (str, &num, whole)`: `s` as a number, or `None` when it is not
/// one through to its end, or reaches `INT_MAX`. A fraction is read in the
/// locale's notation (`radix`), and failing that with `.` and `,` swapped
/// at the first byte the reading stopped at -- so both `1.5` and `1,5`
/// work in either.
#[must_use]
pub fn mkfloat(s: &[u8], whole_only: bool, radix: u8) -> Option<f32> {
    if whole_only {
        return whole(s);
    }
    // `snprintf (tmp, sizeof (tmp), "%s", str)`.
    let mut tmp = s.get(..s.len().min(SMLBUFSIZ - 1)).unwrap_or(s).to_vec();
    let (mut num, mut ep) = strtof_in(&tmp, radix);
    if ep < tmp.len() {
        if let Some(c) = tmp.get_mut(ep) {
            match *c {
                b'.' => *c = b',',
                b',' => *c = b'.',
                _ => {}
            }
        }
        (num, ep) = strtof_in(&tmp, radix);
    }
    (ep != 0 && ep == tmp.len() && num < 2_147_483_648.0).then_some(num)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const SC: Scale = Scale {
        zero_suppress: false,
        tics_scaled: 0,
        hertz: 100,
    };

    fn s(v: &[u8]) -> String {
        String::from_utf8_lossy(v).into_owned()
    }

    #[test]
    fn justify_cuts_and_pads_then_adds_the_column_pad() {
        assert_eq!(s(&justify_pad(b"abc", 5, true)), "  abc ");
        assert_eq!(s(&justify_pad(b"abc", 5, false)), "abc   ");
        assert_eq!(s(&justify_pad(b"abcdef", 3, true)), "abc ");
    }

    #[test]
    fn a_number_too_wide_ends_in_plus_and_widens_its_column() {
        let mut ax = Autox::default();
        assert_eq!(s(&make_num(&SC, &mut ax, 12345, 3, true, 4, false)), "12+ ");
        assert!(ax.found && ax.array[4]);
        let mut ax = Autox::default();
        assert_eq!(
            s(&make_num(&SC, &mut ax, 7, 3, true, AUTOX_NO, false)),
            "  7 "
        );
        assert!(!ax.found);
    }

    #[test]
    fn memory_scales_up_until_it_fits() {
        assert_eq!(s(&scale_mem(&SC, 0, 1234.0, 7, true)), "   1234 ");
        assert_eq!(s(&scale_mem(&SC, 0, 12_345_678.0, 6, true)), " 11.8g ");
        assert_eq!(s(&scale_mem(&SC, 1, 1024.0, 6, true)), "  1.0m ");
    }

    #[test]
    fn percentages_lose_precision_before_they_lose_digits() {
        assert_eq!(s(&scale_pcnt(&SC, 3.25, 5, true, false)), "  3.2 ");
        assert_eq!(s(&scale_pcnt(&SC, 1234.5, 5, true, false)), " 1234 ");
        assert_eq!(s(&scale_pcnt(&SC, 0.5, 6, true, true)), " 0.500 ");
    }

    #[test]
    fn times_take_the_finest_form_that_fits() {
        assert_eq!(
            s(&scale_tics(&SC, 12345, 9, true, TICS_AS_SECS)),
            "  2:03.45 "
        );
        assert_eq!(s(&scale_tics(&SC, 12345, 6, true, TICS_AS_SECS)), "  2:03 ");
        // 20 days of ticks: past the day forms' guard, so weeks.
        let t = 20 * 24 * 3600 * 100;
        assert_eq!(s(&scale_tics(&SC, t, 7, true, TICS_AS_DAY1)), "  2w+6d ");
        // Entered at the two-day form, the guard is not asked.
        assert_eq!(s(&scale_tics(&SC, t, 7, true, TICS_AS_DAY2)), "    20d ");
    }

    #[test]
    fn hex_suppresses_zeros_as_dots() {
        assert_eq!(s(&hex_make(0x4000_0100, true)), "4....1..");
        assert_eq!(s(&hex_make(0xab, false)), "000000ab");
    }

    #[test]
    fn mkfloat_takes_either_decimal_point() {
        assert_eq!(mkfloat(b"1.5", false, b'.'), Some(1.5));
        assert_eq!(mkfloat(b"1,5", false, b'.'), Some(1.5));
        assert_eq!(mkfloat(b"1,5", false, b','), Some(1.5));
        assert_eq!(mkfloat(b"1.5x", false, b'.'), None);
        assert_eq!(mkfloat(b"0x10", true, b'.'), Some(16.0));
        assert_eq!(mkfloat(b"12a", true, b'.'), None);
        assert_eq!(mkfloat(b"", true, b'.'), None);
    }
}
