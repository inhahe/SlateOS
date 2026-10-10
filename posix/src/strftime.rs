//! `strftime`, `wcsftime` and their `_l` forms: glibc 2.39's
//! `__strftime_internal` (`time/strftime_l.c`), in the C locale.
//!
//! glibc compiles one source twice, once for bytes and once for `wchar_t`;
//! here one generic engine, [`ftime`], does the same, and [`Unit`] is what the
//! two widths vary -- the unit, its case mappings, and how `%Z`'s zone name,
//! which is multibyte for both, becomes units. The names and formats are the
//! C locale's `LC_TIME` items, read from [`crate::langinfo`] as glibc reads
//! them from the locale, so `%c` is `nl_langinfo(D_T_FMT)` by construction.
//!
//! What glibc's does, and so this one (every case below is replayed from
//! glibc's own answers, `strftime_oracle.txt`):
//!
//! - **Flags**: `_` pads a number with spaces, `-` does not pad it, `0` pads
//!   it with zeros -- the last of the three wins -- `^` upper-cases text, and
//!   `#` swaps its case: the names upper, `%p`, `%P` and `%Z` lower.
//! - **A field width**, in decimal and at most `INT_MAX`. A number pads to it
//!   as its flag says, zeros by default (spaces for `%e`, `%k` and `%l`), a
//!   minus sign ahead of zeros and behind spaces. Everything else pads on
//!   the left with spaces, or with zeros under `0`: names, `%Z`, `%n`, `%%`,
//!   a whole subformat like `%c`, an unknown conversion copied back -- and a
//!   number under `-`, which `-` keeps from padding with zeros but not from
//!   the width. `%z` pads its sign to the width and then its digits to the
//!   width again: `%6z` is `     +000000`.
//! - **`E` and `O`**: the C locale has no eras and no alternative digits, so
//!   a conversion that takes one writes what it writes without; one that
//!   does not take it is copied back.
//! - **An unknown conversion**, or one with a modifier it does not take, is
//!   copied back from its `%`; so is a `%` that ends the format. `%E%` is the
//!   exception: the copy starts at the nearest `%`, which is its second.
//! - **Numbers past their range** are written as they are, `-5` or `61`;
//!   a name past its range is `?`.
//! - **`%Z`** is `tm_zone`; when that is NULL or empty and `tm_isdst` is not
//!   negative, `tzname[tm_isdst]` after a `tzset`, or `?` past 1. `wcsftime`
//!   converts it as `mbsrtowcs` does and, unlike `strftime`, neither changes
//!   its case nor pads it under `-` (glibc's two copies differ there).
//! - **`%s`** is `mktime` of a copy of the `tm`: -1 when that fails.
//! - **Too small a buffer** answers 0, having written what fit before the
//!   field that did not -- a NUL among it, when a subformat wrote one. A
//!   NULL buffer is written nowhere and answers the length.
//!
//! What glibc's would do and this does not: a NULL format or `tm` answers 0
//! here, where glibc's reads through it and crashes; and a weekday or month
//! name past its range is `?` in `wcsftime` as in `strftime`, where glibc's
//! wide copy reads its narrow `"?"` as `wchar_t`s -- past the end of those
//! two bytes -- and in 2.39's build fails the call.

use crate::langinfo;
use crate::time::Tm;
use crate::wchar::{MbstateT, WcharT};

/// How a copy changes case: glibc's `to_lowcase`, which wins, and
/// `to_uppcase`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Case {
    Keep,
    Upper,
    Lower,
}

/// A unit of `strftime`'s output, a byte, or of `wcsftime`'s, a `wchar_t`.
pub(crate) trait Unit: Copy + PartialEq {
    /// The unit an ASCII byte is.
    fn ascii(b: u8) -> Self;
    /// The unit's value: a byte's, or a `wchar_t`'s bits, so that no unit but
    /// an ASCII one equals an ASCII byte.
    fn value(self) -> u32;
    /// `toupper` (in the C locale, ASCII's letters only) or `towupper`.
    fn upper(self) -> Self;
    /// `tolower` or `towlower`.
    fn lower(self) -> Self;
    /// The units of the NUL-terminated string at `p`, the NUL not counted.
    ///
    /// # Safety
    ///
    /// `p` is NUL-terminated.
    unsafe fn len_of(p: *const Self) -> usize;
    /// `%Z`: the NUL-terminated `zone` written at `out`'s position as this
    /// width writes it, padded as `spec` says; `false` when the pass must
    /// answer 0.
    ///
    /// # Safety
    ///
    /// `zone` is NUL-terminated.
    unsafe fn put_zone(out: &mut Out<Self>, zone: *const u8, spec: &Spec) -> bool;
}

impl Unit for u8 {
    fn ascii(b: u8) -> Self {
        b
    }

    fn value(self) -> u32 {
        u32::from(self)
    }

    fn upper(self) -> Self {
        // glibc's `toupper`, in C and C.UTF-8 alike: a byte past 0x7f is
        // no character, and only ASCII's letters have another case.
        self.to_ascii_uppercase()
    }

    fn lower(self) -> Self {
        self.to_ascii_lowercase()
    }

    unsafe fn len_of(p: *const Self) -> usize {
        // SAFETY: NUL-terminated, this function's contract.
        unsafe { crate::string::strlen(p) }
    }

    unsafe fn put_zone(out: &mut Out<Self>, zone: *const u8, spec: &Spec) -> bool {
        // SAFETY: NUL-terminated, this function's contract, so its `len`
        // bytes before the NUL are readable.
        let name = unsafe { core::slice::from_raw_parts(zone, Self::len_of(zone)) };
        out.cpy(name.len(), spec.width, spec.pad, spec.case(), |k| {
            name.get(k).copied().unwrap_or(0)
        })
    }
}

impl Unit for WcharT {
    fn ascii(b: u8) -> Self {
        Self::from(b)
    }

    fn value(self) -> u32 {
        self.cast_unsigned()
    }

    fn upper(self) -> Self {
        crate::wchar::towupper(self)
    }

    fn lower(self) -> Self {
        crate::wchar::towlower(self)
    }

    unsafe fn len_of(p: *const Self) -> usize {
        // SAFETY: NUL-terminated, this function's contract.
        unsafe { crate::wchar::wcslen(p) }
    }

    /// glibc's wide `%Z`: `mbsrtowcs` straight into the buffer, at most what
    /// is left of it, then moved right to make room for the padding -- none
    /// under `-` -- and no case mapping at all.
    #[allow(clippy::arithmetic_side_effects)]
    unsafe fn put_zone(out: &mut Out<Self>, zone: *const u8, spec: &Spec) -> bool {
        let w = if spec.pad == b'-' {
            0
        } else {
            usize::try_from(spec.width).unwrap_or(0)
        };
        let room = out.room();
        let dst = out.tail();
        let mut src = zone;
        let mut state = MbstateT::new();
        // SAFETY: `src` is NUL-terminated, this function's contract; `dst`
        // is null, or the buffer from `i` on, `room` units, which is as many
        // as `mbsrtowcs` is told it may write.
        let len = unsafe { crate::wchar::mbsrtowcs(dst, &raw mut src, room, &raw mut state) };
        if len == usize::MAX {
            // An invalid sequence: `mbsrtowcs` has set `EILSEQ`.
            return false;
        }
        let incr = len.max(w);
        if incr >= room {
            crate::errno::set_errno(crate::errno::ERANGE);
            return false;
        }
        if !dst.is_null() && len < w {
            let delta = w - len;
            // SAFETY: `delta + len = incr < room`, so the moved name and the
            // padding before it are both inside the buffer; `copy` is
            // `memmove`.
            unsafe { core::ptr::copy(dst, dst.add(delta), len) };
            let fill = Self::from(if spec.pad == b'0' { b'0' } else { b' ' });
            for k in 0..delta {
                // SAFETY: `k < delta < room`, as above.
                unsafe { *dst.add(k) = fill };
            }
        }
        // `incr < room`, so `i` stays below `maxsize`.
        out.i += incr;
        true
    }
}

/// The format a pass reads: the caller's, or a subformat. Past its end it
/// reads as a NUL.
#[derive(Clone, Copy)]
pub(crate) enum Format<'a, U> {
    /// The caller's format, without its NUL.
    Caller(&'a [U]),
    /// A subformat -- the C locale's `D_T_FMT`, or `%D`'s `%m/%d/%y` -- in
    /// ASCII.
    Sub(&'static [u8]),
}

impl<U: Unit> Format<'_, U> {
    /// The unit at `idx`.
    fn at(self, idx: usize) -> U {
        match self {
            Self::Caller(s) => s.get(idx).copied().unwrap_or_else(|| U::ascii(0)),
            Self::Sub(s) => U::ascii(s.get(idx).copied().unwrap_or(0)),
        }
    }
}

/// Where a pass writes: glibc's `s`, `maxsize` and `i`, its `p` being
/// `s + i`.
///
/// Invariant: `i < maxsize`, or `i == 0 == maxsize` -- each step checks that
/// what it adds and the NUL still fit before it adds anything.
pub(crate) struct Out<U> {
    /// The buffer, or null when the pass only counts.
    s: *mut U,
    /// Units the buffer holds, the NUL among them.
    maxsize: usize,
    /// Units produced so far, and where the next one goes.
    i: usize,
}

impl<U: Unit> Out<U> {
    /// A pass writing `s`, or only counting when it is null.
    ///
    /// # Safety
    ///
    /// `s` is null, or valid for `maxsize` writes for as long as the pass
    /// lasts.
    pub(crate) unsafe fn new(s: *mut U, maxsize: usize) -> Self {
        Self { s, maxsize, i: 0 }
    }

    /// A pass that only counts.
    fn counting() -> Self {
        Self {
            s: core::ptr::null_mut(),
            maxsize: usize::MAX,
            i: 0,
        }
    }

    /// glibc's `maxsize - i`.
    fn room(&self) -> usize {
        self.maxsize.saturating_sub(self.i)
    }

    /// The buffer from `i` on -- `maxsize - i` units, none when `maxsize` is
    /// 0 -- or null when the pass only counts.
    fn tail(&self) -> *mut U {
        if self.s.is_null() || self.i > self.maxsize {
            core::ptr::null_mut()
        } else {
            // SAFETY: `i <= maxsize` (the invariant), so this is inside the
            // buffer or one past its end (`new`'s contract).
            unsafe { self.s.add(self.i) }
        }
    }

    /// Store `u` at `at`, unless the pass only counts.
    fn put(&mut self, at: usize, u: U) {
        if !self.s.is_null() && at < self.maxsize {
            // SAFETY: `at < maxsize`, inside the buffer (`new`'s contract).
            unsafe { *self.s.add(at) = u };
        }
    }

    /// `f` of the unit at `at`, stored back.
    fn map(&mut self, at: usize, f: impl Fn(U) -> U) {
        if !self.s.is_null() && at < self.maxsize {
            // SAFETY: `at < maxsize`, inside the buffer (`new`'s contract).
            unsafe {
                let p = self.s.add(at);
                *p = f(*p);
            }
        }
    }

    /// glibc's `add (n, f)`: room for `n` units padded on the left to
    /// `width` -- with zeros under the `0` flag, else spaces -- and the NUL;
    /// the padding written and `i` advanced past the whole field. Where the
    /// `n` units go, or `None` when the field does not fit and the pass
    /// answers 0.
    #[allow(clippy::arithmetic_side_effects)]
    fn add(&mut self, n: usize, width: i32, pad: u8) -> Option<usize> {
        let delta = usize::try_from(width).unwrap_or(0).saturating_sub(n);
        let incr = n.saturating_add(delta);
        if incr >= self.room() {
            return None;
        }
        let at = self.i;
        let fill = U::ascii(if pad == b'0' { b'0' } else { b' ' });
        for k in 0..delta {
            self.put(at + k, fill);
        }
        // `incr < maxsize - i`, so neither sum overflows and `i` stays
        // below `maxsize`.
        self.i = at + incr;
        Some(at + delta)
    }

    /// glibc's `cpy (n, s)`: [`Out::add`] with the units `src(0) ...
    /// src(n - 1)`, case-mapped; `false` when they do not fit.
    #[allow(clippy::arithmetic_side_effects)]
    fn cpy(&mut self, n: usize, width: i32, pad: u8, case: Case, src: impl Fn(usize) -> U) -> bool {
        let Some(at) = self.add(n, width, pad) else {
            return false;
        };
        for k in 0..n {
            let u = match case {
                Case::Keep => src(k),
                Case::Upper => src(k).upper(),
                Case::Lower => src(k).lower(),
            };
            // `add` found room for all `n` from `at`.
            self.put(at + k, u);
        }
        true
    }

    /// The ASCII `text`, as [`Out::cpy`].
    fn cpy_ascii(&mut self, text: &[u8], spec: &Spec) -> bool {
        self.cpy(text.len(), spec.width, spec.pad, spec.case(), |k| {
            U::ascii(text.get(k).copied().unwrap_or(0))
        })
    }

    /// One unit, padded to the width: glibc's `add (1, *p = c)`, which maps
    /// no case.
    fn add1(&mut self, c: U, width: i32, pad: u8) -> bool {
        match self.add(1, width, pad) {
            Some(at) => {
                self.put(at, c);
                true
            }
            None => false,
        }
    }
}

/// One conversion's flags, width and modifier.
pub(crate) struct Spec {
    /// `_`, `-`, `0`, or 0 for none.
    pad: u8,
    /// The field width, or -1 for none.
    width: i32,
    /// `E`, `O`, or 0 for none.
    modifier: u8,
    /// glibc's `to_uppcase`.
    upper: bool,
    /// glibc's `to_lowcase`.
    lower: bool,
    /// The `#` flag: glibc's `change_case`.
    change_case: bool,
}

impl Spec {
    fn case(&self) -> Case {
        if self.lower {
            Case::Lower
        } else if self.upper {
            Case::Upper
        } else {
            Case::Keep
        }
    }

    /// `#` on a name: upper case.
    fn names_upper(&mut self) {
        if self.change_case {
            self.upper = true;
            self.lower = false;
        }
    }

    /// `#` on `%p` or `%Z`: lower case.
    fn names_lower(&mut self) {
        if self.change_case {
            self.upper = false;
            self.lower = true;
        }
    }
}

/// What a conversion came to.
enum Did {
    /// Written.
    Done,
    /// Out of room: the pass answers 0.
    Full,
    /// Not a conversion it knows, or a modifier it does not take: copied
    /// back.
    Bad,
}

impl From<bool> for Did {
    fn from(fit: bool) -> Self {
        if fit { Self::Done } else { Self::Full }
    }
}

/// `u` as ASCII, or `None` -- for a byte past 0x7f, a `wchar_t` past 0x7f.
fn ascii_of<U: Unit>(u: U) -> Option<u8> {
    u8::try_from(u.value()).ok().filter(u8::is_ascii)
}

/// `u`'s value as a decimal digit: glibc's `ISDIGIT`, ASCII's ten only.
fn digit_of<U: Unit>(u: U) -> Option<i32> {
    let d = u.value().wrapping_sub(u32::from(b'0'));
    if d <= 9 { i32::try_from(d).ok() } else { None }
}

/// The C locale's `LC_TIME` string `item`.
fn item(item: i32) -> &'static [u8] {
    langinfo::c_locale_string(item)
}

/// The C locale's name `first + idx` -- `ABDAY_1 + tm_wday`, say -- or `?`
/// when `idx` is not below `count`: glibc's `a_wkday` and the like.
fn name(first: i32, idx: i32, count: i32) -> &'static [u8] {
    if (0..count).contains(&idx) {
        item(first.wrapping_add(idx))
    } else {
        b"?"
    }
}

/// `%Ec`, `%Ex` and `%EX`: the era's format when asked for and the locale
/// has one -- the C locale has none -- else the plain one.
fn era_or(e: bool, era: i32, plain: i32) -> &'static [u8] {
    let era = if e { item(era) } else { b"" };
    if era.is_empty() { item(plain) } else { era }
}

/// glibc's `__isleap`, on an `int` year: C's `%` truncates, as Rust's does.
fn is_leap(year: i32) -> bool {
    year.wrapping_rem(4) == 0 && (year.wrapping_rem(100) != 0 || year.wrapping_rem(400) == 0)
}

/// glibc's `iso_week_days`: the days from the first day of the first ISO
/// week of the year to year day `yday`, a `wday`; negative before it.
fn iso_week_days(yday: i32, wday: i32) -> i32 {
    // glibc's `(-YDAY_MINIMUM / 7 + 2) * 7`, YDAY_MINIMUM being -366.
    const BIG_ENOUGH_MULTIPLE_OF_7: i32 = (366 / 7 + 2) * 7;
    const ISO_WEEK1_WDAY: i32 = 4;
    const ISO_WEEK_START_WDAY: i32 = 1;
    let shifted = yday
        .wrapping_sub(wday)
        .wrapping_add(ISO_WEEK1_WDAY)
        .wrapping_add(BIG_ENOUGH_MULTIPLE_OF_7);
    yday.wrapping_sub(shifted.wrapping_rem(7))
        .wrapping_add(ISO_WEEK1_WDAY - ISO_WEEK_START_WDAY)
}

/// `n`'s decimal digits, at the end of `buf`; the slice they fill.
#[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
fn decimal(mut n: u64, buf: &mut [u8; 20]) -> &[u8] {
    let mut start = buf.len();
    loop {
        // A u64 has at most 20 digits, so `start` stays inside the buffer.
        start -= 1;
        if let Some(slot) = buf.get_mut(start) {
            // `% 10` is a digit, so the cast is exact.
            *slot = b'0' + (n % 10) as u8;
        }
        n /= 10;
        if n == 0 {
            break;
        }
    }
    buf.get(start..).unwrap_or_default()
}

/// glibc's `do_number_sign_and_padding`: the digits `mag`, `negative`'s sign
/// before them, padded out to `digits` as `pad` says -- spaces before the
/// sign under `_`, zeros after it otherwise, nothing under `-` -- and then
/// the rest to `width`, by [`Out::add`]'s rules.
#[allow(clippy::arithmetic_side_effects)]
fn sign_and_padding<U: Unit>(
    out: &mut Out<U>,
    mag: &[u8],
    negative: bool,
    digits: i32,
    pad: u8,
    mut width: i32,
) -> bool {
    // At most 21 units: 20 digits and a sign.
    let len = i32::try_from(mag.len() + usize::from(negative)).unwrap_or(i32::MAX);
    let mut sign = negative;
    if pad != b'-' {
        let padding = digits.saturating_sub(len);
        if padding > 0 {
            // `padding > 0`, so the conversion is exact.
            let n = usize::try_from(padding).unwrap_or(0);
            if pad == b'_' {
                if n >= out.room() {
                    return false;
                }
                for _ in 0..n {
                    out.put(out.i, U::ascii(b' '));
                    // Below `maxsize`: `n < room`, checked above.
                    out.i += 1;
                }
                width = if width > padding { width - padding } else { 0 };
            } else {
                if usize::try_from(digits).unwrap_or(usize::MAX) >= out.room() {
                    return false;
                }
                if negative {
                    out.put(out.i, U::ascii(b'-'));
                    // The first of the `digits` units checked above.
                    out.i += 1;
                    sign = false;
                }
                for _ in 0..n {
                    out.put(out.i, U::ascii(b'0'));
                    // Still inside the `digits` checked above.
                    out.i += 1;
                }
                width = 0;
            }
        }
    }
    let lead = usize::from(sign);
    // The digits and a sign have no case, so `cpy`'s mapping is moot.
    out.cpy(mag.len() + lead, width, pad, Case::Keep, |k| {
        if k < lead {
            U::ascii(b'-')
        } else {
            U::ascii(mag.get(k - lead).copied().unwrap_or(b'0'))
        }
    })
}

/// glibc's `DO_NUMBER (d, v)` and, with `spacepad`, `DO_NUMBER_SPACEPAD`:
/// `value` in at least `d` digits, or the width's.
fn number<U: Unit>(out: &mut Out<U>, spec: &Spec, d: i32, value: i32, spacepad: bool) -> bool {
    let mut pad = spec.pad;
    if spacepad && pad != b'0' && pad != b'-' {
        pad = b'_';
    }
    let digits = d.max(spec.width);
    // `O` asks for the locale's alternative digits, glibc's `ALT_DIGITS`;
    // the C locale has none, and glibc's then writes the number as it is.
    let mut buf = [0u8; 20];
    let mag = decimal(u64::from(value.unsigned_abs()), &mut buf);
    sign_and_padding(out, mag, value < 0, digits, pad, spec.width)
}

/// A subformat -- `%c`'s `D_T_FMT`, `%D`'s `%m/%d/%y` -- as one field: its
/// length counted by a pass that writes nothing, the field padded to the
/// width, then written by a second pass, the NUL after it too; upper-cased
/// under `^`. glibc's `subformat:`.
///
/// # Safety
///
/// As [`ftime`].
#[allow(clippy::arithmetic_side_effects)]
unsafe fn subformat<U: Unit>(
    out: &mut Out<U>,
    spec: &Spec,
    sub: &'static [u8],
    tp: &Tm,
    tzset_called: &mut bool,
) -> bool {
    // SAFETY: `ftime`'s contract, for `tp`; a counting pass writes nothing.
    let len = unsafe { ftime(Out::<U>::counting(), Format::Sub(sub), tp, tzset_called) };
    let Some(at) = out.add(len, spec.width, spec.pad) else {
        return false;
    };
    if !out.s.is_null() {
        // SAFETY: `at < maxsize` -- `add` found room from it for `len` units
        // and the NUL -- so the rest of the buffer from `at`, `maxsize - at`
        // units, is valid for the second pass to write, which writes those
        // `len + 1`; `ftime`'s contract, for `tp`.
        unsafe {
            let inner = Out::new(out.s.add(at), out.maxsize - at);
            ftime(inner, Format::Sub(sub), tp, tzset_called);
        }
        if spec.upper {
            for k in at..at + len {
                out.map(k, U::upper);
            }
        }
    }
    true
}

/// `%Z`'s name, NUL-terminated: `tm_zone`, or when that is NULL or empty and
/// `tm_isdst` is not negative, `tzname[tm_isdst]` -- after a `tzset`, the
/// first time a call needs it -- or `?` past 1; else "".
///
/// # Safety
///
/// `tp.tm_zone` is null or NUL-terminated.
unsafe fn zone_name(tp: &Tm, tzset_called: &mut bool) -> *const u8 {
    let zone = tp.tm_zone;
    // SAFETY: null, or NUL-terminated and so readable at 0 (this function's
    // contract).
    if !zone.is_null() && unsafe { *zone } != 0 {
        return zone;
    }
    if tp.tm_isdst < 0 {
        return c"".as_ptr().cast();
    }
    if !*tzset_called {
        // This library's own `tzset`, as glibc's calls its own.
        crate::tz::tzset();
        *tzset_called = true;
    }
    let name = match tp.tm_isdst {
        0 => crate::time::tzname_entry(0),
        1 => crate::time::tzname_entry(1),
        _ => c"?".as_ptr().cast(),
    };
    if name.is_null() {
        c"".as_ptr().cast()
    } else {
        name
    }
}

/// glibc's `__strftime_internal`: `format` for `tp`, into `out`. The units the
/// text has, the NUL not counted, or 0 when it and the NUL do not fit -- and
/// then the buffer holds what fit before the field that did not.
///
/// `tzset_called` is one call's: `%Z` calls `tzset` the first time it needs
/// `tzname`, and not again.
///
/// # Safety
///
/// `tp.tm_zone` is null or NUL-terminated.
#[allow(clippy::too_many_lines, clippy::arithmetic_side_effects)]
pub(crate) unsafe fn ftime<U: Unit>(
    mut out: Out<U>,
    format: Format<'_, U>,
    tp: &Tm,
    tzset_called: &mut bool,
) -> usize {
    let hour12 = match tp.tm_hour {
        h if h > 12 => h - 12,
        0 => 12,
        h => h,
    };

    // `f` is the index of the unit being read. It moves past a unit only
    // once that unit is known not to be the NUL, so it never passes the end.
    let mut f = 0usize;
    loop {
        let c = format.at(f);
        if c.value() == 0 {
            break;
        }
        if c.value() != u32::from(b'%') {
            if !out.add1(c, -1, 0) {
                return 0;
            }
            f += 1;
            continue;
        }

        let mut spec = Spec {
            pad: 0,
            width: -1,
            modifier: 0,
            upper: false,
            lower: false,
            change_case: false,
        };
        loop {
            f += 1;
            match ascii_of(format.at(f)) {
                Some(p @ (b'_' | b'-' | b'0')) => spec.pad = p,
                Some(b'^') => spec.upper = true,
                Some(b'#') => spec.change_case = true,
                _ => break,
            }
        }
        if let Some(mut d) = digit_of(format.at(f)) {
            spec.width = 0;
            loop {
                // glibc's guard: a width past an `int` is `INT_MAX`.
                spec.width = if spec.width > i32::MAX / 10
                    || (spec.width == i32::MAX / 10 && d > i32::MAX % 10)
                {
                    i32::MAX
                } else {
                    spec.width * 10 + d
                };
                f += 1;
                match digit_of(format.at(f)) {
                    Some(next) => d = next,
                    None => break,
                }
            }
        }
        if let Some(m @ (b'E' | b'O')) = ascii_of(format.at(f)) {
            spec.modifier = m;
            f += 1;
        }

        let e = spec.modifier == b'E';
        let o = spec.modifier == b'O';
        let any = spec.modifier != 0;
        let did = match ascii_of(format.at(f)) {
            Some(b'%') if !any => Did::from(out.add1(U::ascii(b'%'), spec.width, spec.pad)),
            Some(b'a') if !any => {
                spec.names_upper();
                Did::from(out.cpy_ascii(name(langinfo::ABDAY_1, tp.tm_wday, 7), &spec))
            }
            Some(b'A') if !any => {
                spec.names_upper();
                Did::from(out.cpy_ascii(name(langinfo::DAY_1, tp.tm_wday, 7), &spec))
            }
            Some(b'b' | b'h') => {
                // glibc maps the case before it refuses `E`, so `%#Eb`
                // copies back as `%#EB`.
                spec.names_upper();
                if e {
                    Did::Bad
                } else {
                    // `O` is the alternative names, `ABALTMON_1`; the C
                    // locale's are these.
                    Did::from(out.cpy_ascii(name(langinfo::ABMON_1, tp.tm_mon, 12), &spec))
                }
            }
            Some(b'B') if !e => {
                spec.names_upper();
                // `O`: `ALTMON_1`, these in the C locale.
                Did::from(out.cpy_ascii(name(langinfo::MON_1, tp.tm_mon, 12), &spec))
            }
            Some(b'c') if !o => {
                let sub = era_or(e, langinfo::ERA_D_T_FMT, langinfo::D_T_FMT);
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, sub, tp, tzset_called) })
            }
            Some(b'C') => {
                // `E` is the era's name; the C locale has no eras.
                let year = tp.tm_year.wrapping_add(1900);
                let century = year.wrapping_div(100) - i32::from(year.wrapping_rem(100) < 0);
                Did::from(number(&mut out, &spec, 1, century, false))
            }
            Some(b'x') if !o => {
                let sub = era_or(e, langinfo::ERA_D_FMT, langinfo::D_FMT);
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, sub, tp, tzset_called) })
            }
            Some(b'D') if !any => {
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, b"%m/%d/%y", tp, tzset_called) })
            }
            Some(b'd') if !e => Did::from(number(&mut out, &spec, 2, tp.tm_mday, false)),
            Some(b'e') if !e => Did::from(number(&mut out, &spec, 2, tp.tm_mday, true)),
            Some(b'F') if !any => {
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, b"%Y-%m-%d", tp, tzset_called) })
            }
            Some(b'H') if !e => Did::from(number(&mut out, &spec, 2, tp.tm_hour, false)),
            Some(b'I') if !e => Did::from(number(&mut out, &spec, 2, hour12, false)),
            Some(b'k') if !e => Did::from(number(&mut out, &spec, 2, tp.tm_hour, true)),
            Some(b'l') if !e => Did::from(number(&mut out, &spec, 2, hour12, true)),
            Some(b'j') if !e => Did::from(number(
                &mut out,
                &spec,
                3,
                tp.tm_yday.wrapping_add(1),
                false,
            )),
            Some(b'M') if !e => Did::from(number(&mut out, &spec, 2, tp.tm_min, false)),
            Some(b'm') if !e => {
                Did::from(number(&mut out, &spec, 2, tp.tm_mon.wrapping_add(1), false))
            }
            Some(b'n') => Did::from(out.add1(U::ascii(b'\n'), spec.width, spec.pad)),
            Some(c @ (b'P' | b'p')) => {
                if c == b'P' {
                    spec.lower = true;
                }
                spec.names_lower();
                let ampm = if tp.tm_hour > 11 {
                    langinfo::PM_STR
                } else {
                    langinfo::AM_STR
                };
                Did::from(out.cpy_ascii(item(ampm), &spec))
            }
            Some(b'R') => {
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, b"%H:%M", tp, tzset_called) })
            }
            Some(b'r') => {
                let ampm = item(langinfo::T_FMT_AMPM);
                let sub: &'static [u8] = if ampm.is_empty() {
                    b"%I:%M:%S %p"
                } else {
                    ampm
                };
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, sub, tp, tzset_called) })
            }
            Some(b'S') if !e => Did::from(number(&mut out, &spec, 2, tp.tm_sec, false)),
            Some(b's') => {
                // `mktime` of a copy: this library's own, which a program
                // defining its own `mktime` does not replace here.
                let mut ltm = *tp;
                let t = crate::time::mktime_ptr(&raw mut ltm);
                let mut buf = [0u8; 20];
                let mag = decimal(t.unsigned_abs(), &mut buf);
                Did::from(sign_and_padding(
                    &mut out,
                    mag,
                    t < 0,
                    1,
                    spec.pad,
                    spec.width,
                ))
            }
            Some(b'X') if !o => {
                let sub = era_or(e, langinfo::ERA_T_FMT, langinfo::T_FMT);
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, sub, tp, tzset_called) })
            }
            Some(b'T') => {
                // SAFETY: `ftime`'s contract.
                Did::from(unsafe { subformat(&mut out, &spec, b"%H:%M:%S", tp, tzset_called) })
            }
            Some(b't') => Did::from(out.add1(U::ascii(b'\t'), spec.width, spec.pad)),
            Some(b'u') => {
                let wday = tp
                    .tm_wday
                    .wrapping_sub(1)
                    .wrapping_add(7)
                    .wrapping_rem(7)
                    .wrapping_add(1);
                Did::from(number(&mut out, &spec, 1, wday, false))
            }
            Some(b'U') if !e => {
                let week = tp
                    .tm_yday
                    .wrapping_sub(tp.tm_wday)
                    .wrapping_add(7)
                    .wrapping_div(7);
                Did::from(number(&mut out, &spec, 2, week, false))
            }
            Some(c @ (b'V' | b'g' | b'G')) if !e => {
                let mut year = tp.tm_year.wrapping_add(1900);
                let mut days = iso_week_days(tp.tm_yday, tp.tm_wday);
                if days < 0 {
                    // The week is the previous year's last.
                    year = year.wrapping_sub(1);
                    let len = 365 + i32::from(is_leap(year));
                    days = iso_week_days(tp.tm_yday.wrapping_add(len), tp.tm_wday);
                } else {
                    let len = 365 + i32::from(is_leap(year));
                    let next = iso_week_days(tp.tm_yday.wrapping_sub(len), tp.tm_wday);
                    if next >= 0 {
                        // The week is the next year's first.
                        year = year.wrapping_add(1);
                        days = next;
                    }
                }
                let (d, value) = match c {
                    b'g' => (
                        2,
                        year.wrapping_rem(100).wrapping_add(100).wrapping_rem(100),
                    ),
                    b'G' => (1, year),
                    _ => (2, days.wrapping_div(7).wrapping_add(1)),
                };
                Did::from(number(&mut out, &spec, d, value, false))
            }
            Some(b'W') if !e => {
                let since_monday = tp.tm_wday.wrapping_sub(1).wrapping_add(7).wrapping_rem(7);
                let week = tp
                    .tm_yday
                    .wrapping_sub(since_monday)
                    .wrapping_add(7)
                    .wrapping_div(7);
                Did::from(number(&mut out, &spec, 2, week, false))
            }
            Some(b'w') if !e => Did::from(number(&mut out, &spec, 1, tp.tm_wday, false)),
            // `E` is the era's year; the C locale has no eras.
            Some(b'Y') if !o => Did::from(number(
                &mut out,
                &spec,
                1,
                tp.tm_year.wrapping_add(1900),
                false,
            )),
            Some(b'y') => {
                // `E` is the year in the era; the C locale has none.
                let yy = tp
                    .tm_year
                    .wrapping_rem(100)
                    .wrapping_add(100)
                    .wrapping_rem(100);
                Did::from(number(&mut out, &spec, 2, yy, false))
            }
            Some(b'Z') => {
                spec.names_lower();
                // SAFETY: `ftime`'s contract; `zone_name` answers a
                // NUL-terminated string.
                Did::from(unsafe { U::put_zone(&mut out, zone_name(tp, tzset_called), &spec) })
            }
            Some(b'z') => {
                if tp.tm_isdst < 0 {
                    // No zone known: nothing at all.
                    Did::Done
                } else {
                    // glibc's `int diff = tp->tm_gmtoff`: an offset past an
                    // `int` is cut to one, as there.
                    #[allow(clippy::cast_possible_truncation)]
                    let mut diff = tp.tm_gmtoff as i32;
                    let sign = if diff < 0 {
                        diff = diff.wrapping_neg();
                        b'-'
                    } else {
                        b'+'
                    };
                    if out.add1(U::ascii(sign), spec.width, spec.pad) {
                        let minutes = diff.wrapping_div(60);
                        let hhmm = minutes
                            .wrapping_div(60)
                            .wrapping_mul(100)
                            .wrapping_add(minutes.wrapping_rem(60));
                        Did::from(number(&mut out, &spec, 4, hhmm, false))
                    } else {
                        Did::Full
                    }
                }
            }
            Some(0) => {
                // A `%` and its flags ending the format: copied back from the
                // unit before the NUL, which the loop then reaches. `f` is
                // past the `%`, so this cannot go below it.
                f -= 1;
                Did::Bad
            }
            _ => Did::Bad,
        };

        match did {
            Did::Done => {}
            Did::Full => return 0,
            Did::Bad => {
                // Copied back from the nearest `%`: the conversion's own,
                // unless the conversion is itself a `%`, as in `%E%`.
                let mut start = f;
                while format.at(start).value() != u32::from(b'%') {
                    // The conversion's `%` is at or before `f`.
                    let Some(prev) = start.checked_sub(1) else {
                        break;
                    };
                    start = prev;
                }
                if !out.cpy(f + 1 - start, spec.width, spec.pad, spec.case(), |k| {
                    format.at(start + k)
                }) {
                    return 0;
                }
            }
        }
        f += 1;
    }

    if out.maxsize != 0 {
        out.put(out.i, U::ascii(0));
    }
    out.i
}

/// One call of `strftime` or `wcsftime`: the format measured, the pass made.
///
/// # Safety
///
/// `s` is null or valid for `maxsize` units; `format` is NUL-terminated; `tm`
/// points to a `struct tm` whose `tm_zone` is null or NUL-terminated.
unsafe fn call<U: Unit>(s: *mut U, maxsize: usize, format: *const U, tm: *const Tm) -> usize {
    if format.is_null() || tm.is_null() {
        return 0;
    }
    let mut tzset_called = false;
    // SAFETY: this function's contract: `format` is NUL-terminated, so its
    // units before the NUL are readable; `s` is the caller's buffer; `tm`
    // is a `struct tm`.
    unsafe {
        let format = core::slice::from_raw_parts(format, U::len_of(format));
        ftime(
            Out::new(s, maxsize),
            Format::Caller(format),
            &*tm,
            &mut tzset_called,
        )
    }
}

/// Format `tm` as `format` says, into `s`: glibc's `strftime`, in the C
/// locale -- the conversions, flags, widths and modifiers in this module's
/// header. Answers the bytes written, the NUL not counted, or 0 when they
/// and the NUL do not fit in `maxsize`; a NULL `s` is written nowhere and
/// answers the length.
///
/// # Safety
///
/// `s` is null or valid for `maxsize` bytes; `format` is NUL-terminated;
/// `tm` points to a `struct tm` whose `tm_zone` is null or NUL-terminated.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strftime(
    s: *mut u8,
    maxsize: usize,
    format: *const u8,
    tm: *const Tm,
) -> usize {
    // SAFETY: this function's contract.
    unsafe { call(s, maxsize, format, tm) }
}

/// `strftime` in an explicit locale.
///
/// We have exactly one locale, so this is `strftime` and the handle is
/// ignored. That is a larger claim than it is for the character-class
/// wrappers and is worth stating: the month and day names `%A`/`%B` are what
/// a locale would change, and in the C locale they are English. Anything that
/// wanted translated names would need a real locale first, and would find
/// this function unchanged rather than silently wrong.
///
/// # Safety
///
/// As [`strftime`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn strftime_l(
    s: *mut u8,
    maxsize: usize,
    format: *const u8,
    tm: *const Tm,
    _loc: crate::locale::LocaleT,
) -> usize {
    // SAFETY: this function's contract.
    unsafe { call(s, maxsize, format, tm) }
}

/// `strftime` for wide characters: glibc's `wcsftime`, in the C locale.
/// The format is `wchar_t`s, any of them copied as they are; `maxsize` and
/// the answer count `wchar_t`s. `%Z`'s zone is converted as `mbsrtowcs`
/// converts it -- an invalid one answers 0, with `EILSEQ` -- and is neither
/// case-mapped nor padded under `-`, as glibc's is not.
///
/// # Safety
///
/// `s` is null or valid for `maxsize` `wchar_t`s; `format` is
/// NUL-terminated; `tm` as for [`strftime`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsftime(
    s: *mut WcharT,
    maxsize: usize,
    format: *const WcharT,
    tm: *const Tm,
) -> usize {
    // SAFETY: this function's contract.
    unsafe { call(s, maxsize, format, tm) }
}

/// `wcsftime` in an explicit locale: the one locale's names and formats,
/// as for [`strftime_l`].
///
/// # Safety
///
/// As [`wcsftime`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn wcsftime_l(
    s: *mut WcharT,
    maxsize: usize,
    format: *const WcharT,
    tm: *const Tm,
    _loc: crate::locale::LocaleT,
) -> usize {
    // SAFETY: this function's contract.
    unsafe { call(s, maxsize, format, tm) }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `TZ` installed for one test, and the previous one put back after it.
    /// The environment and the zone are per test thread on the host.
    struct Zone {
        saved: Option<std::vec::Vec<u8>>,
    }

    impl Zone {
        fn set(tz: &str) -> Self {
            let saved = crate::environ::getenv_bytes(b"TZ").map(<[u8]>::to_vec);
            Self::put(tz.as_bytes());
            Self { saved }
        }

        fn put(tz: &[u8]) {
            let mut value = tz.to_vec();
            value.push(0);
            // SAFETY: both strings are NUL-terminated and outlive the call.
            let rc = unsafe { crate::environ::setenv(c"TZ".as_ptr().cast(), value.as_ptr(), 1) };
            assert_eq!(rc, 0, "setenv(TZ)");
            crate::tz::tzset();
        }
    }

    impl Drop for Zone {
        fn drop(&mut self) {
            match self.saved.take() {
                Some(previous) => Self::put(&previous),
                None => {
                    // SAFETY: a NUL-terminated literal.
                    let rc = unsafe { crate::environ::unsetenv(c"TZ".as_ptr().cast()) };
                    assert_eq!(rc, 0, "unsetenv(TZ)");
                    crate::tz::tzset();
                }
            }
        }
    }

    fn unhex(h: &str) -> std::vec::Vec<u8> {
        (0..h.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&h[i..i + 2], 16).expect("hex"))
            .collect()
    }

    fn hex(bytes: &[u8]) -> std::string::String {
        use core::fmt::Write;
        if bytes.is_empty() {
            return "-".into();
        }
        bytes.iter().fold(std::string::String::new(), |mut s, b| {
            // Writing into a `String` cannot fail.
            let _ = write!(s, "{b:02x}");
            s
        })
    }

    fn hex_units(units: &[WcharT]) -> std::string::String {
        if units.is_empty() {
            return "-".into();
        }
        let parts: std::vec::Vec<std::string::String> = units
            .iter()
            .map(|u| std::format!("{:08x}", u.cast_unsigned()))
            .collect();
        parts.join(".")
    }

    /// glibc's `strftime` and `wcsftime`, replayed:
    /// `posix/tools/oracle/strftime_harness.py`'s calls -- every conversion
    /// under the flags, widths and modifiers, the ISO week-based year across
    /// the turn of 22 years, years from `INT_MIN` to `INT_MAX`, `%z` and `%Z`
    /// from every kind of zone field, `%s` in two zones, formats cut short,
    /// and buffers from none to more than enough. Each call's answer, its
    /// `errno`, and every unit it wrote, a partial write included, must be
    /// glibc's.
    ///
    /// One answer of glibc's is not taken: a weekday or month name past its
    /// range in `wcsftime`. glibc's wide copy writes the narrow literal `"?"`
    /// read as `wchar_t`s -- its `a_wkday` and the like are `"?"` cast to a
    /// wide pointer -- so it measures and copies whatever follows the two
    /// bytes in its `.rodata`, and in 2.39's build the call fails, answering
    /// 0. This one writes `?`, as glibc's `strftime` does and as its wide
    /// copy plainly means to; for those calls the answer expected is the
    /// narrow twin's, widened (the harness writes each wide call just after
    /// its narrow one). There are exactly 73 of them.
    #[test]
    fn strftime_is_glibcs() {
        let oracle = include_str!("strftime_oracle.txt");
        let mut zone: Option<(&str, Zone)> = None;
        let mut failures = std::vec::Vec::new();
        let mut compared = 0usize;
        let mut deviations = 0usize;
        let mut last_narrow: Option<(std::string::String, std::string::String)> = None;
        for line in oracle.lines() {
            let (left, right) = line.split_once(" | ").expect("line");
            let l: std::vec::Vec<&str> = left.split(' ').collect();
            let mut r: std::vec::Vec<std::string::String> =
                right.split(' ').map(std::string::String::from).collect();
            if l[0] == "N" {
                last_narrow = Some((l[1..15].join(" "), right.to_string()));
            } else if !(0..=6).contains(&l[8].parse::<i32>().expect("wday"))
                || !(0..=11).contains(&l[6].parse::<i32>().expect("mon"))
            {
                let names_one = l[15] != "~"
                    && l[15]
                        .split('.')
                        .any(|u| matches!(u32::from_str_radix(u, 16), Ok(c) if "aAbBhc".contains(char::from_u32(c).unwrap_or('\0'))));
                if let (true, Some((inputs, answer))) = (names_one, last_narrow.as_ref())
                    && *inputs == l[1..15].join(" ")
                {
                    let mut widened: std::vec::Vec<std::string::String> =
                        answer.split(' ').map(std::string::String::from).collect();
                    if widened[2] != "-" {
                        widened[2] = unhex(&widened[2])
                            .iter()
                            .map(|b| std::format!("{b:08x}"))
                            .collect::<std::vec::Vec<_>>()
                            .join(".");
                    }
                    if widened != r {
                        deviations += 1;
                        r = widened;
                    }
                }
            }
            let tz = l[1];
            if zone.as_ref().map(|z| z.0) != Some(tz) {
                // The old guard's drop puts back the zone before it, and the
                // new one's set saves that.
                drop(zone.take());
                zone = Some((tz, Zone::set(tz)));
            }
            let f: std::vec::Vec<i32> =
                l[2..11].iter().map(|v| v.parse().expect("field")).collect();
            let zone_bytes: Option<std::vec::Vec<u8>> = match l[12] {
                "-" => None,
                "~" => Some(std::vec![0]),
                h => {
                    let mut z = unhex(h);
                    z.push(0);
                    Some(z)
                }
            };
            let tm = Tm {
                tm_sec: f[0],
                tm_min: f[1],
                tm_hour: f[2],
                tm_mday: f[3],
                tm_mon: f[4],
                tm_year: f[5],
                tm_wday: f[6],
                tm_yday: f[7],
                tm_isdst: f[8],
                tm_gmtoff: l[11].parse().expect("gmtoff"),
                tm_zone: zone_bytes
                    .as_ref()
                    .map_or(core::ptr::null(), |z| z.as_ptr()),
            };
            let maxsize: usize = l[13].parse().expect("maxsize");
            let null = l[14] == "1";
            crate::errno::set_errno(0);
            let (ret, written) = if l[0] == "W" {
                let mut fmt: std::vec::Vec<WcharT> = if l[15] == "~" {
                    std::vec::Vec::new()
                } else {
                    l[15]
                        .split('.')
                        .map(|u| u32::from_str_radix(u, 16).expect("unit").cast_signed())
                        .collect()
                };
                fmt.push(0);
                let sentinel = 0xaaaa_aaaa_u32.cast_signed();
                let mut buf = std::vec![sentinel; maxsize + 16];
                let s = if null {
                    core::ptr::null_mut()
                } else {
                    buf.as_mut_ptr()
                };
                // SAFETY: `s` is null or `maxsize + 16` units; the format is
                // NUL-terminated; `tm_zone` is null or NUL-terminated.
                let ret = unsafe { wcsftime(s, maxsize, fmt.as_ptr(), &raw const tm) };
                let touched = buf
                    .iter()
                    .rposition(|&u| u != sentinel)
                    .map_or(0, |i| i + 1);
                (ret, hex_units(&buf[..touched]))
            } else {
                let mut fmt = if l[15] == "~" {
                    std::vec::Vec::new()
                } else {
                    unhex(l[15])
                };
                fmt.push(0);
                let mut buf = std::vec![0xaa_u8; maxsize + 16];
                let s = if null {
                    core::ptr::null_mut()
                } else {
                    buf.as_mut_ptr()
                };
                // SAFETY: as above, in bytes.
                let ret = unsafe { strftime(s, maxsize, fmt.as_ptr(), &raw const tm) };
                let touched = buf.iter().rposition(|&b| b != 0xaa).map_or(0, |i| i + 1);
                (ret, hex(&buf[..touched]))
            };
            let errno = crate::errno::get_errno();
            let got = std::format!("{ret} {errno} {written}");
            let want = std::format!("{} {} {}", r[0], r[1], r[2]);
            compared += 1;
            if got != want {
                failures.push(std::format!("{left}\n    glibc: {want}\n    ours:  {got}"));
            }
        }
        drop(zone);
        assert!(compared > 12_000, "the oracle has {compared} lines");
        assert!(
            failures.is_empty(),
            "{} of {compared} calls differ from glibc's:\n{}",
            failures.len(),
            failures
                .iter()
                .take(40)
                .cloned()
                .collect::<std::vec::Vec<_>>()
                .join("\n")
        );
        assert_eq!(
            deviations, 73,
            "the wide names past their range, which glibc's misreads"
        );
    }

    /// A NULL format or `tm` answers 0 rather than crashing, as glibc's
    /// would.
    #[test]
    fn a_null_format_or_tm_answers_nothing() {
        let tm = Tm::ZERO;
        let mut buf = [0x55u8; 8];
        // SAFETY: the buffer is 8 bytes; the NULLs are what is tested.
        unsafe {
            assert_eq!(
                strftime(buf.as_mut_ptr(), 8, core::ptr::null(), &raw const tm),
                0
            );
            assert_eq!(
                strftime(
                    buf.as_mut_ptr(),
                    8,
                    c"%Y".as_ptr().cast(),
                    core::ptr::null()
                ),
                0
            );
        }
        assert_eq!(buf, [0x55; 8]);
    }

    /// `strftime_l` and `wcsftime_l` are the C locale's whatever the handle.
    #[test]
    fn the_l_forms_are_the_c_locales() {
        let mut tm = Tm::ZERO;
        tm.tm_wday = 3;
        tm.tm_mon = 8;
        let mut buf = [0u8; 32];
        // SAFETY: the buffer is 32 bytes; the format is NUL-terminated.
        let n = unsafe {
            strftime_l(
                buf.as_mut_ptr(),
                32,
                c"%A %B".as_ptr().cast(),
                &raw const tm,
                0,
            )
        };
        assert_eq!(&buf[..n], b"Wednesday September");
        let fmt: std::vec::Vec<WcharT> = "%a %b".chars().map(|c| c as WcharT).chain([0]).collect();
        let mut wbuf = [0 as WcharT; 32];
        // SAFETY: as above, in `wchar_t`s.
        let n = unsafe { wcsftime_l(wbuf.as_mut_ptr(), 32, fmt.as_ptr(), &raw const tm, 0) };
        let text: std::string::String = wbuf[..n]
            .iter()
            .map(|&u| char::from_u32(u.cast_unsigned()).expect("char"))
            .collect();
        assert_eq!(text, "Wed Sep");
    }
}
