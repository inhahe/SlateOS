// Every index here is into a name no longer than its own `strlen` or a
// buffer the caller's size was checked against first, and every sum is of
// such lengths. Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! `<arpa/nameser.h>`'s utilities beside the message parser
//! ([`crate::resolv`]): domain names made canonical and compared
//! (`ns_makecanon`, `ns_samename`, `ns_samedomain`, `ns_subdomain`), a
//! message header's flags (`ns_msg_getflag`), TTLs as `1W2D3H4M5S` and
//! back (`ns_format_ttl`, `ns_parse_ttl`), and a DNSSEC date as seconds
//! (`ns_datetosecs`).
//!
//! BIND's, in glibc 2.39's libresolv (`resolv/ns_makecanon.c`,
//! `ns_samename.c`, `ns_samedomain.c`, `ns_parse.c`, `ns_ttl.c`,
//! `ns_date.c`), and here in libc.a, which a program's `-lresolv` finds as
//! it does the rest. Ported line for line where a line decides an answer,
//! and replayed against glibc's (`nsutil_oracle.txt`), BIND's odd answers
//! with the rest:
//!
//! - **Names** compare in ASCII case alone; a trailing dot is dropped
//!   unless a backslash escapes it -- counted in `ns_samedomain`, but in
//!   `ns_makecanon` only the one backslash before it, so `a\\\.` loses its
//!   escaped dot there. `ns_makecanon` wants room for the text as given
//!   and a dot, before it drops anything.
//! - **TTLs** are formatted a unit at a time, each refused when it does not
//!   fit after those before it are written, with -1 and `errno` untouched;
//!   more than one unit makes them lower case (`1d1h1m1s`, but `1D`). The
//!   weeks are a C `int`, and wrap as one: a TTL of 2^63-1 seconds is
//!   `-1144415625w...`. Parsed, a number is `unsigned long` and wraps too.
//! - **Dates** are `YYYYMMDDHHMMSS`, years 1990 to 9999, a day up to 31 in
//!   any month (February 30 is March 2), summed in 32 bits that wrap: the
//!   last second of 9999 is 4294197631.
//!
//! Where glibc would read or write through NULL, these refuse instead:
//! `EFAULT` where the call has an error channel, and otherwise the answer
//! for no match (0) or for a bad date (`*errp` = 1).

use crate::errno;
use crate::resolv::{_ns_flagdata, NS_MAXDNAME, NsMsg};

/// A NUL-terminated string's bytes, or `None` for NULL.
///
/// # Safety
///
/// `s` is NULL or a NUL-terminated string.
unsafe fn c_text<'a>(s: *const u8) -> Option<&'a [u8]> {
    if s.is_null() {
        return None;
    }
    // SAFETY: the caller's contract.
    Some(unsafe { core::slice::from_raw_parts(s, crate::string::strlen(s)) })
}

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

/// glibc's `__libc_ns_makecanon` into `dst`: the canonical name's length,
/// its dot counted, or `EMSGSIZE` when `dst` cannot hold `src`, a dot and a
/// NUL. Writes as glibc does -- the text and its NUL, a NUL over each dot
/// dropped, then the dot and a NUL.
fn makecanon(src: &[u8], dst: &mut [u8]) -> Result<usize, i32> {
    let n0 = src.len();
    if n0.saturating_add(2) > dst.len() {
        return Err(errno::EMSGSIZE);
    }
    dst[..n0].copy_from_slice(src);
    dst[n0] = 0;
    let mut n = n0;
    while n >= 1 && dst[n - 1] == b'.' {
        // Ends in `\.` but not `\\.`: the dot is escaped, and stays.
        if n >= 2 && dst[n - 2] == b'\\' && (n < 3 || dst[n - 3] != b'\\') {
            break;
        }
        n -= 1;
        dst[n] = 0;
    }
    dst[n] = b'.';
    dst[n + 1] = 0;
    Ok(n + 1)
}

/// Make a canonical copy of the name `src` in `dst`, `dstsize` bytes: its
/// trailing dots dropped but an escaped one, and one dot put back --
/// `foo`, `foo.` and `foo..` are all `foo.`, `foo\.` is `foo\..`.
///
/// 0, or -1 with `errno` `EMSGSIZE` when `dstsize` cannot hold `src` as
/// given, a dot and a NUL (`foo..` wants 7 bytes), `EFAULT` for a NULL
/// `src` or `dst`. Deprecated by glibc, as `posix/include`'s declaration
/// says.
///
/// # Safety
///
/// `src` is NULL or a NUL-terminated string; `dst` is NULL or holds
/// `dstsize` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_makecanon(src: *const u8, dst: *mut u8, dstsize: usize) -> i32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_text(src) }) else {
        errno::set_errno(errno::EFAULT);
        return -1;
    };
    if s.len().saturating_add(2) > dstsize {
        errno::set_errno(errno::EMSGSIZE);
        return -1;
    }
    if dst.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `dst` holds `dstsize` bytes, the caller's contract, and the
    // text and two more fit in them, checked above.
    let out = unsafe { core::slice::from_raw_parts_mut(dst, s.len() + 2) };
    match makecanon(s, out) {
        Ok(_) => 0,
        Err(e) => {
            errno::set_errno(e);
            -1
        }
    }
}

/// glibc's `__libc_ns_samename` on text: `Some(same)`, or `None` (with
/// `errno` `EMSGSIZE`) when either will not go canonical in `NS_MAXDNAME`
/// bytes.
pub(crate) fn samename(a: &[u8], b: &[u8]) -> Option<bool> {
    let mut ta = [0u8; NS_MAXDNAME];
    let mut tb = [0u8; NS_MAXDNAME];
    let (la, lb) = match (makecanon(a, &mut ta), makecanon(b, &mut tb)) {
        (Ok(la), Ok(lb)) => (la, lb),
        (Err(e), _) | (_, Err(e)) => {
            errno::set_errno(e);
            return None;
        }
    };
    Some(ta[..la].eq_ignore_ascii_case(&tb[..lb]))
}

/// Whether two domain names are the same, once canonical (see
/// [`ns_makecanon`]) and in ASCII case alone: 1 or 0, or -1 with `errno`
/// `EMSGSIZE` when either is too long to make canonical in `NS_MAXDNAME`
/// bytes (1023 bytes and a dot, and the NUL), or `EFAULT` for NULL.
/// Deprecated by glibc.
///
/// # Safety
///
/// `a` and `b` are NULL or NUL-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_samename(a: *const u8, b: *const u8) -> i32 {
    // SAFETY: the caller's contract.
    let (Some(a), Some(b)) = (unsafe { c_text(a) }, unsafe { c_text(b) }) else {
        errno::set_errno(errno::EFAULT);
        return -1;
    };
    samename(a, b).map_or(-1, i32::from)
}

/// The length of `s` without a trailing dot that no backslash escapes --
/// counting the backslashes before it, an even number being none.
fn without_trailing_dot(s: &[u8]) -> usize {
    let n = s.len();
    if n == 0 || s[n - 1] != b'.' {
        return n;
    }
    let slashes = s[..n - 1].iter().rev().take_while(|&&c| c == b'\\').count();
    if slashes % 2 == 1 { n } else { n - 1 }
}

/// glibc's `ns_samedomain` on text.
fn samedomain(a: &[u8], b: &[u8]) -> bool {
    let la = without_trailing_dot(a);
    let lb = without_trailing_dot(b);
    // The root holds every name; a longer name cannot be in a shorter one.
    if lb == 0 {
        return true;
    }
    if lb > la {
        return false;
    }
    if lb == la {
        return a[..lb].eq_ignore_ascii_case(&b[..lb]);
    }
    let diff = la - lb;
    // At least one byte of label and the dot before `b`'s part of `a`,
    // and that dot not escaped: "foobar.com" is not in "bar.com".
    if diff < 2 || a[diff - 1] != b'.' {
        return false;
    }
    let slashes = a[..diff - 1]
        .iter()
        .rev()
        .take_while(|&&c| c == b'\\')
        .count();
    if slashes % 2 == 1 {
        return false;
    }
    a[diff..diff + lb].eq_ignore_ascii_case(&b[..lb])
}

/// Whether the name `a` is in the domain `b`, at or below it: 1 or 0.
/// `host.foobar.top` is in `foobar.top`, `top` and the root (`""` or
/// `.`), but not in `bar.top`; a trailing dot is ignored unless escaped,
/// case in ASCII. A NULL name is 0. Deprecated by glibc.
///
/// # Safety
///
/// `a` and `b` are NULL or NUL-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_samedomain(a: *const u8, b: *const u8) -> i32 {
    // SAFETY: the caller's contract.
    match (unsafe { c_text(a) }, unsafe { c_text(b) }) {
        (Some(a), Some(b)) => i32::from(samedomain(a, b)),
        _ => 0,
    }
}

/// Whether the name `a` is below the domain `b` -- in it, and not the same
/// name ([`ns_samename`]'s 1): 1 or 0. A name too long for `ns_samename`
/// is not the same, and so below `b` if [`ns_samedomain`] puts it there,
/// as glibc's `-1 != 1` has it. A NULL name is 0. Deprecated by glibc.
///
/// # Safety
///
/// `a` and `b` are NULL or NUL-terminated strings.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_subdomain(a: *const u8, b: *const u8) -> i32 {
    // SAFETY: the caller's contract.
    match (unsafe { c_text(a) }, unsafe { c_text(b) }) {
        (Some(a), Some(b)) => i32::from(samename(a, b) != Some(true) && samedomain(a, b)),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// The header's flags
// ---------------------------------------------------------------------------

/// One of a parsed message's header flags ([`crate::resolv::ns_initparse`]'s
/// handle), by `ns_flag`: `ns_f_qr` (0) ... `ns_f_rcode` (9), shifted down
/// -- the function `<arpa/nameser.h>`'s macro of the same name is, for a
/// program that takes its address. The six unused flags are 0, and so is
/// a flag outside the table's 16, where glibc's would read past it.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn ns_msg_getflag(handle: NsMsg, flag: i32) -> i32 {
    let Some(f) = usize::try_from(flag).ok().and_then(|i| _ns_flagdata.get(i)) else {
        return 0;
    };
    (i32::from(handle.flags) & f.mask) >> f.shift
}

// ---------------------------------------------------------------------------
// TTLs
// ---------------------------------------------------------------------------

/// glibc's `fmt1`: `t` and its unit at `*at` in `dst`, if what is left of
/// `dstlen` holds them and a NUL.
fn fmt1(t: i32, unit: u8, dst: *mut u8, at: &mut usize, left: &mut usize) -> Result<(), i32> {
    let mut tmp = [0u8; 12];
    let mut len = 0;
    let mut v = t.unsigned_abs();
    loop {
        tmp[len] = b'0' + (v % 10) as u8;
        len += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    if t < 0 {
        tmp[len] = b'-';
        len += 1;
    }
    tmp[..len].reverse();
    tmp[len] = unit;
    len += 1;
    if len + 1 > *left {
        return Err(-1);
    }
    if dst.is_null() {
        return Err(errno::EFAULT);
    }
    // SAFETY: `dst` holds `dstlen` bytes, `*at` of them used and `*left`
    // left, more than the unit and its NUL.
    unsafe {
        core::ptr::copy_nonoverlapping(tmp.as_ptr(), dst.add(*at), len);
        dst.add(*at + len).write(0);
    }
    *at += len;
    *left -= len;
    Ok(())
}

/// Format a TTL, `src` seconds, as BIND's `1W2D3H4M5S` in `dst` (`dstlen`
/// bytes): each unit that is not zero, seconds when all are, lower case
/// when there is more than one (`1d1h1m1s`, `1D`).
///
/// The length written, the NUL aside; or -1 when `dstlen` cannot hold the
/// next unit and its NUL, those before it written -- with `errno`
/// untouched, as glibc's is, or `EFAULT` for a NULL `dst` where glibc would
/// fault. The weeks are a C `int`, and wrap as glibc's do. Deprecated by
/// glibc.
///
/// # Safety
///
/// `dst` is NULL or holds `dstlen` bytes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_format_ttl(src: u64, dst: *mut u8, dstlen: usize) -> i32 {
    let mut src = src;
    let secs = (src % 60) as i32;
    src /= 60;
    let mins = (src % 60) as i32;
    src /= 60;
    let hours = (src % 24) as i32;
    src /= 24;
    let days = (src % 7) as i32;
    src /= 7;
    // `int weeks = src`: the low 32 bits, as a C conversion keeps them.
    let weeks = src as i32;
    let mut at = 0usize;
    let mut left = dstlen;
    let mut units = 0;
    let parts = [(weeks, b'W'), (days, b'D'), (hours, b'H'), (mins, b'M')];
    let all_zero = parts.iter().all(|&(v, _)| v == 0);
    for (v, unit) in parts {
        if v != 0 {
            if let Err(e) = fmt1(v, unit, dst, &mut at, &mut left) {
                if e != -1 {
                    errno::set_errno(e);
                }
                return -1;
            }
            units += 1;
        }
    }
    if secs != 0 || all_zero {
        if let Err(e) = fmt1(secs, b'S', dst, &mut at, &mut left) {
            if e != -1 {
                errno::set_errno(e);
            }
            return -1;
        }
        units += 1;
    }
    if units > 1 {
        // SAFETY: `at` bytes were written to `dst` above.
        let text = unsafe { core::slice::from_raw_parts_mut(dst, at) };
        text.make_ascii_lowercase();
    }
    i32::try_from(at).unwrap_or(i32::MAX)
}

/// Parse a TTL, BIND's `1W2D3H4M5S` -- units in either case, in any order,
/// each after a number, repeats adding up -- or a plain number of seconds,
/// into `*dst`.
///
/// 0, or -1 with `errno` `EINVAL`: a byte that is not printable ASCII, a
/// unit with no number, a letter that is no unit, a number after a unit
/// (`1H30`), nothing at all. Numbers are `unsigned long`, and wrap as
/// glibc's do. A NULL `src` is `EINVAL`, a NULL `dst` `EFAULT`. Deprecated
/// by glibc.
///
/// # Safety
///
/// `src` is NULL or a NUL-terminated string; `dst` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_parse_ttl(src: *const u8, dst: *mut u64) -> i32 {
    // SAFETY: the caller's contract.
    let Some(s) = (unsafe { c_text(src) }) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    let Some(ttl) = parse_ttl(s) else {
        errno::set_errno(errno::EINVAL);
        return -1;
    };
    if dst.is_null() {
        errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: `dst` is non-NULL and writable, the caller's contract.
    unsafe { dst.write(ttl) };
    0
}

/// glibc's `ns_parse_ttl` on text: the TTL, or `None` for `EINVAL`.
fn parse_ttl(s: &[u8]) -> Option<u64> {
    let mut ttl = 0u64;
    let mut tmp = 0u64;
    let mut digits = 0;
    let mut dirty = false;
    for &ch in s {
        if !(0x20..=0x7e).contains(&ch) {
            return None;
        }
        if ch.is_ascii_digit() {
            tmp = tmp.wrapping_mul(10).wrapping_add(u64::from(ch - b'0'));
            digits += 1;
            continue;
        }
        if digits == 0 {
            return None;
        }
        // Each unit is the next one's multiple, as the C falls through.
        let scale: u64 = match ch.to_ascii_uppercase() {
            b'W' => 7 * 24 * 60 * 60,
            b'D' => 24 * 60 * 60,
            b'H' => 60 * 60,
            b'M' => 60,
            b'S' => 1,
            _ => return None,
        };
        ttl = ttl.wrapping_add(tmp.wrapping_mul(scale));
        tmp = 0;
        digits = 0;
        dirty = true;
    }
    if digits > 0 {
        if dirty {
            return None;
        }
        ttl = ttl.wrapping_add(tmp);
    } else if !dirty {
        return None;
    }
    Some(ttl)
}

// ---------------------------------------------------------------------------
// Dates
// ---------------------------------------------------------------------------

/// glibc's `datepart`: `size` decimal digits, which must be between `min`
/// and `max`; `*err` set -- and never cleared -- if not.
fn datepart(s: &[u8], size: usize, min: i32, max: i32, err: &mut bool) -> i32 {
    let mut result: i32 = 0;
    for &c in &s[..size] {
        if !c.is_ascii_digit() {
            *err = true;
        }
        // A non-digit's value is never used: the error returns 0.
        result = result
            .wrapping_mul(10)
            .wrapping_add(i32::from(c) - i32::from(b'0'));
    }
    if result < min || result > max {
        *err = true;
    }
    result
}

/// Whether the Gregorian year `y` has a February 29.
fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

/// glibc's `ns_datetosecs` on text: the seconds, or `None` for a date it
/// refuses.
fn datetosecs(s: &[u8]) -> Option<u32> {
    const SECS_PER_DAY: u32 = 24 * 60 * 60;
    const DAYS_PER_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if s.len() != 14 {
        return None;
    }
    let mut err = false;
    let year = datepart(&s[0..], 4, 1990, 9999, &mut err) - 1900;
    let mon = datepart(&s[4..], 2, 1, 12, &mut err) - 1;
    let mday = datepart(&s[6..], 2, 1, 31, &mut err);
    let hour = datepart(&s[8..], 2, 0, 23, &mut err);
    let min = datepart(&s[10..], 2, 0, 59, &mut err);
    let sec = datepart(&s[12..], 2, 0, 59, &mut err);
    if err {
        return None;
    }
    // The fields are in range here, so each converts; the sums are
    // glibc's `uint32_t`s, and wrap as they do.
    let u = |v: i32| v.cast_unsigned();
    let mut result = u(sec);
    result = result.wrapping_add(u(min) * 60);
    result = result.wrapping_add(u(hour) * 60 * 60);
    result = result.wrapping_add(u(mday - 1).wrapping_mul(SECS_PER_DAY));
    let mdays: u32 = DAYS_PER_MONTH[..mon as usize].iter().sum();
    result = result.wrapping_add(mdays.wrapping_mul(SECS_PER_DAY));
    if mon > 1 && is_leap(1900 + year) {
        result = result.wrapping_add(SECS_PER_DAY);
    }
    result = result.wrapping_add(u(year - 70).wrapping_mul(SECS_PER_DAY * 365));
    for y in 70..year {
        if is_leap(1900 + y) {
            result = result.wrapping_add(SECS_PER_DAY);
        }
    }
    Some(result)
}

/// A DNSSEC date, `YYYYMMDDHHMMSS` in UTC with every digit given, as
/// seconds since 1970: 0 with `*errp` 0, or 0 with `*errp` 1 for text that
/// is not 14 bytes, a byte that is not a digit, or a field out of its
/// range -- the year 1990 to 9999, a day 1 to 31 in any month. The sum is
/// 32-bit and wraps, as glibc's is. A NULL `cp` is a bad date; a NULL
/// `errp` is not written. Deprecated by glibc.
///
/// # Safety
///
/// `cp` is NULL or a NUL-terminated string; `errp` is NULL or writable.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_datetosecs(cp: *const u8, errp: *mut i32) -> u32 {
    // SAFETY: the caller's contract.
    let answer = unsafe { c_text(cp) }.and_then(datetosecs);
    if !errp.is_null() {
        // SAFETY: `errp` is non-NULL and writable, the caller's contract.
        unsafe { errp.write(i32::from(answer.is_none())) };
    }
    answer.unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Records as text: glibc 2.39's ns_print.c, with June 2026's fixes
// ---------------------------------------------------------------------------
//
// glibc's ns_print.c as its June 2026 fixes left it, which the oracle's
// glibc (Ubuntu's 2.39-0ubuntu8.9) carries: a class or type with no name
// prints as RFC 3597's CLASSn/TYPEn, A6 by name (bug 34289); CERT, TKEY,
// TSIG and OPT print as unknown records do, as hex, their printers gone
// (CVE-2026-5435); an unknown record has no comment and a format error's
// is " ; RR format error"; a LOC record must be 16 bytes and an A6's
// address must be all there (CVE-2026-6238); and an address that does not
// fit fails the call rather than printing nothing.

/// The comment after a record's data that does not read, printed as hex.
const FORMAT_ERROR: &[u8] = b" ; RR format error";

/// What is left of `ns_sprintrrf`'s buffer, as `ns_print.c`'s `char **buf,
/// size_t *buflen`: where the next byte goes, and how many remain -- a
/// copy of it saved and put back is ns_print.c's `save_buf`.
#[derive(Clone, Copy)]
struct Pr {
    buf: *mut u8,
    len: usize,
}

/// `ns_print.c`'s failures: `ENOSPC` (set where it happened, as glibc's)
/// or a record whose data does not read (`formerr`).
enum PrErr {
    /// -1 to the caller, `errno` as it was left.
    Fail,
    /// The data is printed as hex instead, this comment after its length:
    /// [`FORMAT_ERROR`], or none for a type printed no other way.
    Hexify(&'static [u8]),
}

impl Pr {
    /// `addlen`: `n` bytes, already written, are the buffer's.
    fn addlen(&mut self, n: usize) {
        let n = n.min(self.len);
        // SAFETY: `n` of the `len` bytes that remain.
        self.buf = unsafe { self.buf.add(n) };
        self.len -= n;
    }

    /// `addstr`: `s` and a NUL, or `ENOSPC` when they do not both fit.
    fn addstr(&mut self, s: &[u8]) -> Result<(), PrErr> {
        if s.len() >= self.len {
            crate::errno::set_errno(errno::ENOSPC);
            return Err(PrErr::Fail);
        }
        // SAFETY: `s` and a NUL fit in what remains.
        unsafe { core::ptr::copy_nonoverlapping(s.as_ptr(), self.buf, s.len()) };
        self.addlen(s.len());
        // SAFETY: a byte remains, checked above.
        unsafe { self.buf.write(0) };
        Ok(())
    }

    /// `addtab`: to the column `target`, with tabs if the text so far,
    /// `len` bytes, leaves room, else two spaces; whether it spaced.
    fn addtab(&mut self, len: usize, target: usize, spaced: bool) -> Result<bool, PrErr> {
        let save = *self;
        if spaced || len >= target - 1 {
            self.addstr(b"  ")?;
            return Ok(true);
        }
        for _ in 0..=((target - len - 1) / 8) {
            if self.addstr(b"\t").is_err() {
                *self = save;
                return Err(PrErr::Fail);
            }
        }
        Ok(false)
    }

    /// The NUL-terminated text now at the buffer's position.
    fn text(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: the position holds a NUL within what remains -- every
        // write here ends in one, as `inet_ntop`'s and `dn_expand`'s do.
        let n = unsafe { crate::string::strnlen(self.buf, self.len) };
        // SAFETY: as above.
        unsafe { core::slice::from_raw_parts(self.buf, n) }
    }
}

/// `prune_origin`: the bytes of `name` before `origin` begins in it --
/// label by label, an escaped dot no end -- or all of them when it does
/// not; the dot before `origin` is not counted.
fn prune_origin(name: &[u8], origin: Option<&[u8]>) -> usize {
    let mut i = 0;
    while i < name.len() {
        if let Some(o) = origin {
            if samename(&name[i..], o) == Some(true) {
                return i - usize::from(i > 0);
            }
        }
        while i < name.len() {
            if name[i] == b'\\' {
                i += 1;
                if i >= name.len() {
                    break;
                }
                i += 1;
            } else if name[i] == b'.' {
                i += 1;
                break;
            } else {
                i += 1;
            }
        }
    }
    i
}

/// Whether a name printed without its origin's part wants a dot after it:
/// no origin, or one not the root whose tail the name is not -- and the
/// name not ending in a dot already. glibc's test, `name[len]` being the
/// NUL when the origin was not found in it.
fn wants_dot(name: &[u8], len: usize, origin: Option<&[u8]>) -> bool {
    let origin_says = match origin {
        None | Some([]) => true,
        Some(o) => o[0] != b'.' && o.len() > 1 && len == name.len(),
    };
    origin_says && len > 0 && name[len - 1] != b'.'
}

/// The record's data being printed: the message it is in (for its names'
/// pointers), and where it is at in the data -- past its end when a name
/// in it ran on into the message after it, as `dn_expand` lets one.
struct Rd {
    msg: *const u8,
    msglen: usize,
    rdata: *const u8,
    at: usize,
    end: usize,
}

impl Rd {
    /// The bytes left: none once past the end.
    fn left(&self) -> usize {
        self.end.saturating_sub(self.at)
    }

    /// `(unsigned)(edata - rdata)`, the count glibc's hex form starts with:
    /// the bytes left, or -- a name having read past the end, which only an
    /// SOA's then prints -- the negative difference as C's cast shows it,
    /// modulo 2^32.
    #[allow(clippy::cast_possible_truncation)] // C's (unsigned) keeps the low 32 bits
    fn left_shown(&self) -> u32 {
        if self.at <= self.end {
            (self.end - self.at) as u32
        } else {
            0u32.wrapping_sub((self.at - self.end) as u32)
        }
    }

    /// The data from here to its end.
    fn rest(&self) -> &[u8] {
        // SAFETY: `rdata` holds `end` bytes, the caller's contract; the
        // slice is of those that are left.
        unsafe { core::slice::from_raw_parts(self.rdata.add(self.at.min(self.end)), self.left()) }
    }

    /// `n` bytes, or a format error where glibc would read past the data.
    fn take(&mut self, n: usize) -> Result<&[u8], PrErr> {
        if n > self.left() {
            return Err(PrErr::Hexify(FORMAT_ERROR));
        }
        // SAFETY: as `rest`; `n` of the bytes that remain.
        let s = unsafe { core::slice::from_raw_parts(self.rdata.add(self.at), n) };
        self.at += n;
        Ok(s)
    }

    fn u8(&mut self) -> Result<u8, PrErr> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u32, PrErr> {
        let b = self.take(2)?;
        Ok(u32::from(u16::from_be_bytes([b[0], b[1]])))
    }

    fn u32(&mut self) -> Result<u32, PrErr> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// `charstr`: a `<character-string>` at the data's position, quoted, with
/// `"`, `\`, a newline -- and, strchr's way, a NUL -- escaped: the bytes it
/// took (0 for none that reads: a format error to the callers).
fn charstr(rd: &mut Rd, out: &mut Pr) -> Result<usize, PrErr> {
    let save = *out;
    let r = (|| {
        out.addstr(b"\"")?;
        let s = rd.rest();
        let mut took = 0;
        if let Some(&n) = s.first() {
            let n = usize::from(n);
            // The length byte and its `n` bytes are all in the data.
            if n < s.len() {
                for &c in &s[1..=n] {
                    if matches!(c, b'\n' | b'"' | b'\\' | 0) {
                        out.addstr(b"\\")?;
                    }
                    out.addstr(&[c])?;
                }
                took = 1 + n;
            }
        }
        out.addstr(b"\"")?;
        Ok(took)
    })();
    match r {
        Ok(took) => {
            rd.at += took;
            Ok(took)
        }
        Err(e) => {
            *out = save;
            crate::errno::set_errno(errno::ENOSPC);
            Err(e)
        }
    }
}

/// `addname`: the compressed name at the data's position, expanded into the
/// buffer by `dn_expand`, less its origin (`@` for the origin itself), a dot
/// after an absolute one: its length.
fn addname(rd: &mut Rd, origin: Option<&[u8]>, out: &mut Pr) -> Result<usize, PrErr> {
    let save = *out;
    let fail = |out: &mut Pr| {
        crate::errno::set_errno(errno::ENOSPC);
        *out = save;
        Err(PrErr::Fail)
    };
    // SAFETY: the position is within the data, or past it only as far as
    // a name before it ran on through the message -- `dn_expand`'s reach,
    // which ends at the message's end.
    let at = unsafe { rd.rdata.add(rd.at) };
    // SAFETY: as above: the message's end.
    let eom = unsafe { rd.msg.add(rd.msglen) };
    let size = i32::try_from(out.len).unwrap_or(i32::MAX);
    let n = crate::resolv::dn_expand(rd.msg, eom, at, out.buf, size);
    let Ok(n) = usize::try_from(n) else {
        return fail(out);
    };
    let name = NameCopy::of(out.text());
    let mut newlen = prune_origin(name.as_slice(), origin);
    let put = |out: &mut Pr, at: usize, c: u8| {
        // SAFETY: `at + 2 <= len`, checked by the callers.
        unsafe {
            out.buf.add(at).write(c);
            out.buf.add(at + 1).write(0);
        }
    };
    if name.is_empty() || (newlen > 0 && wants_dot(name.as_slice(), newlen, origin)) {
        if newlen + 2 > out.len {
            return fail(out);
        }
        put(out, newlen, b'.');
        newlen += 1;
    } else if newlen == 0 {
        if newlen + 2 > out.len {
            return fail(out);
        }
        put(out, newlen, b'@');
        newlen += 1;
    }
    rd.at += n;
    out.addlen(newlen);
    // SAFETY: a byte remains: the name and its NUL fit.
    unsafe { out.buf.write(0) };
    Ok(newlen)
}

/// A NUL-terminated text copied out of the buffer, for reading while the
/// buffer is written: at most `NS_MAXDNAME` bytes, as a name is.
struct NameCopy {
    b: [u8; NS_MAXDNAME],
    n: usize,
}

impl NameCopy {
    fn of(s: &[u8]) -> Self {
        let mut c = Self {
            b: [0; NS_MAXDNAME],
            n: s.len().min(NS_MAXDNAME),
        };
        c.b[..c.n].copy_from_slice(&s[..c.n]);
        c
    }

    fn as_slice(&self) -> &[u8] {
        &self.b[..self.n]
    }

    fn is_empty(&self) -> bool {
        self.n == 0
    }
}

/// `sprintf`'s decimal, as text.
fn dec(v: i64) -> ([u8; 24], usize) {
    let mut b = [0u8; 24];
    let mut k = b.len();
    let mut m = v.unsigned_abs();
    loop {
        k -= 1;
        b[k] = b'0' + (m % 10) as u8;
        m /= 10;
        if m == 0 {
            break;
        }
    }
    if v < 0 {
        k -= 1;
        b[k] = b'-';
    }
    let n = b.len() - k;
    b.copy_within(k.., 0);
    (b, n)
}

/// Text built for one `addstr`, as ns_print.c's `tmp`.
struct Tmp {
    b: [u8; 128],
    n: usize,
}

impl Tmp {
    const fn new() -> Self {
        Self { b: [0; 128], n: 0 }
    }

    fn s(&mut self, s: &[u8]) -> &mut Self {
        let k = s.len().min(self.b.len() - self.n);
        self.b[self.n..self.n + k].copy_from_slice(&s[..k]);
        self.n += k;
        self
    }

    fn d(&mut self, v: i64) -> &mut Self {
        let (b, n) = dec(v);
        self.s(&b[..n])
    }

    fn get(&self) -> &[u8] {
        &self.b[..self.n]
    }
}

/// `addsym`: a space and the name `table` gives `number`, or -- it having
/// none -- a space, `prefix` and the number, RFC 3597's form (`TYPE249`).
fn addsym<const N: usize>(
    table: &'static crate::res_debug::SymTable<N>,
    number: i32,
    prefix: &[u8],
    out: &mut Pr,
) -> Result<(), PrErr> {
    if let Some(name) = crate::res_debug::sym_name(table, number) {
        // Two writes, as glibc's: the space may fit where the name does not.
        out.addstr(b" ")?;
        return out.addstr(name);
    }
    out.addstr(Tmp::new().s(b" ").s(prefix).d(i64::from(number)).get())
}

/// `inet_ntop` into the buffer, the text's length added; when it does not
/// fit, the call fails, `inet_ntop`'s `ENOSPC` its `errno`.
fn ntop_into(af: i32, src: *const u8, out: &mut Pr) -> Result<(), PrErr> {
    let size = u32::try_from(out.len).unwrap_or(u32::MAX);
    if crate::inet::inet_ntop(af, src, out.buf, size).is_null() {
        return Err(PrErr::Fail);
    }
    let n = out.text().len();
    out.addlen(n);
    Ok(())
}

/// The type numbers ns_sprintrrf prints by their own rules.
mod t {
    pub(super) const A: i32 = 1;
    pub(super) const NS: i32 = 2;
    pub(super) const CNAME: i32 = 5;
    pub(super) const SOA: i32 = 6;
    pub(super) const MB: i32 = 7;
    pub(super) const MG: i32 = 8;
    pub(super) const MR: i32 = 9;
    pub(super) const WKS: i32 = 11;
    pub(super) const PTR: i32 = 12;
    pub(super) const HINFO: i32 = 13;
    pub(super) const MINFO: i32 = 14;
    pub(super) const MX: i32 = 15;
    pub(super) const TXT: i32 = 16;
    pub(super) const RP: i32 = 17;
    pub(super) const AFSDB: i32 = 18;
    pub(super) const X25: i32 = 19;
    pub(super) const ISDN: i32 = 20;
    pub(super) const RT: i32 = 21;
    pub(super) const NSAP: i32 = 22;
    pub(super) const PX: i32 = 26;
    pub(super) const AAAA: i32 = 28;
    pub(super) const LOC: i32 = 29;
    pub(super) const SRV: i32 = 33;
    pub(super) const NAPTR: i32 = 35;
    pub(super) const A6: i32 = 38;
    pub(super) const DNAME: i32 = 39;
}

/// The record's data, printed by its type (the switch of glibc's
/// `ns_sprintrrf`); `spaced` as the owner and TTL left it.
#[allow(clippy::too_many_lines)] // one arm a type, as glibc's switch
fn rdata_text(
    type_: i32,
    rd: &mut Rd,
    origin: Option<&[u8]>,
    out: &mut Pr,
    spaced: &mut bool,
) -> Result<(), PrErr> {
    let formerr = || Err(PrErr::Hexify(FORMAT_ERROR));
    match type_ {
        t::A => {
            if rd.left() != 4 {
                return formerr();
            }
            // SAFETY: four bytes of data remain.
            ntop_into(crate::socket::AF_INET, unsafe { rd.rdata.add(rd.at) }, out)?;
        }
        t::CNAME | t::MB | t::MG | t::MR | t::NS | t::PTR | t::DNAME => {
            addname(rd, origin, out)?;
        }
        t::HINFO | t::ISDN => {
            if charstr(rd, out)? == 0 {
                return formerr();
            }
            out.addstr(b" ")?;
            if type_ == t::ISDN && rd.left() == 0 {
                return Ok(());
            }
            if charstr(rd, out)? == 0 {
                return formerr();
            }
        }
        t::SOA => {
            addname(rd, origin, out)?;
            out.addstr(b" ")?;
            addname(rd, origin, out)?;
            out.addstr(b" (\n")?;
            *spaced = false;
            if rd.left() != 20 {
                return formerr();
            }
            let serial = rd.u32()?;
            out.addstr(b"\t\t\t\t\t")?;
            let mut tmp = Tmp::new();
            tmp.d(i64::from(serial));
            out.addstr(tmp.get())?;
            *spaced = out.addtab(tmp.get().len(), 16, *spaced)?;
            out.addstr(b"; serial\n")?;
            *spaced = false;
            for (i, what) in [
                &b"; refresh\n"[..],
                b"; retry\n",
                b"; expiry\n",
                b"; minimum\n",
            ]
            .into_iter()
            .enumerate()
            {
                let v = rd.u32()?;
                out.addstr(b"\t\t\t\t\t")?;
                // SAFETY: the buffer holds `out.len` bytes from its position.
                let len = unsafe { ns_format_ttl(u64::from(v), out.buf, out.len) };
                let Ok(len) = usize::try_from(len) else {
                    return Err(PrErr::Fail);
                };
                out.addlen(len);
                if i == 3 {
                    out.addstr(b" )")?;
                }
                *spaced = out.addtab(len, 16, *spaced)?;
                out.addstr(what)?;
                if i < 3 {
                    *spaced = false;
                }
            }
        }
        t::MX | t::AFSDB | t::RT | t::PX => {
            if rd.left() < 2 {
                return formerr();
            }
            let pref = rd.u16()?;
            out.addstr(Tmp::new().d(i64::from(pref)).s(b" ").get())?;
            addname(rd, origin, out)?;
            if type_ == t::PX {
                out.addstr(b" ")?;
                addname(rd, origin, out)?;
            }
        }
        t::X25 => {
            if charstr(rd, out)? == 0 {
                return formerr();
            }
        }
        t::TXT => {
            while rd.left() > 0 {
                if charstr(rd, out)? == 0 {
                    return formerr();
                }
                if rd.left() > 0 {
                    out.addstr(b" ")?;
                }
            }
        }
        t::NSAP => {
            let mut t = [0u8; crate::inet::NSAP_NTOA_MAX];
            let n = i32::try_from(rd.left()).unwrap_or(i32::MAX);
            // SAFETY: the data holds `n` bytes; `t` the longest text.
            unsafe { crate::inet::inet_nsap_ntoa(n, rd.rdata.add(rd.at), t.as_mut_ptr()) };
            let len = t.iter().position(|&c| c == 0).unwrap_or(t.len());
            out.addstr(&t[..len])?;
        }
        t::AAAA => {
            if rd.left() != 16 {
                return formerr();
            }
            // SAFETY: sixteen bytes of data remain.
            ntop_into(crate::socket::AF_INET6, unsafe { rd.rdata.add(rd.at) }, out)?;
        }
        t::LOC => {
            if rd.left() != 16 {
                return formerr();
            }
            let b = rd.take(16)?;
            let mut t = [0u8; crate::res_debug::LOC_NTOA_MAX];
            // SAFETY: `b` holds 16 bytes; `t` the longest text.
            unsafe { crate::res_debug::__loc_ntoa(b.as_ptr(), t.as_mut_ptr()) };
            let len = t.iter().position(|&c| c == 0).unwrap_or(t.len());
            out.addstr(&t[..len])?;
        }
        t::NAPTR => {
            if rd.left() < 4 {
                return formerr();
            }
            let order = rd.u16()?;
            let pref = rd.u16()?;
            out.addstr(
                Tmp::new()
                    .d(i64::from(order))
                    .s(b" ")
                    .d(i64::from(pref))
                    .s(b" ")
                    .get(),
            )?;
            for _ in 0..3 {
                if charstr(rd, out)? == 0 {
                    return formerr();
                }
                out.addstr(b" ")?;
            }
            addname(rd, origin, out)?;
        }
        t::SRV => {
            if rd.left() < 6 {
                return formerr();
            }
            let (p, w, port) = (rd.u16()?, rd.u16()?, rd.u16()?);
            out.addstr(
                Tmp::new()
                    .d(i64::from(p))
                    .s(b" ")
                    .d(i64::from(w))
                    .s(b" ")
                    .d(i64::from(port))
                    .s(b" ")
                    .get(),
            )?;
            addname(rd, origin, out)?;
        }
        t::MINFO | t::RP => {
            addname(rd, origin, out)?;
            out.addstr(b" ")?;
            addname(rd, origin, out)?;
        }
        t::WKS => {
            if rd.left() < 5 {
                return formerr();
            }
            // SAFETY: four bytes of data remain.
            ntop_into(crate::socket::AF_INET, unsafe { rd.rdata.add(rd.at) }, out)?;
            rd.at += 4;
            let proto = rd.u8()?;
            out.addstr(Tmp::new().s(b" ").d(i64::from(proto)).s(b" ( ").get())?;
            let mut n: i64 = 0;
            let mut lcnt = 0;
            while rd.left() > 0 {
                let mut c = u32::from(rd.u8()?);
                loop {
                    if c & 0o200 != 0 {
                        if lcnt == 0 {
                            out.addstr(b"\n\t\t\t\t")?;
                            lcnt = 10;
                            *spaced = false;
                        }
                        out.addstr(Tmp::new().d(n).s(b" ").get())?;
                        lcnt -= 1;
                    }
                    c <<= 1;
                    n += 1;
                    // glibc's `while (++n & 07)`: a byte's eight bits done.
                    if n % 8 == 0 {
                        break;
                    }
                }
            }
            out.addstr(b")")?;
        }
        t::A6 => {
            // The prefix length, printed -- and refused past 128 -- before
            // it is stepped over, so the hex of a refused one starts with it.
            let Some(&pbit) = rd.rest().first() else {
                return formerr();
            };
            out.addstr(Tmp::new().d(i64::from(pbit)).s(b" ").get())?;
            if pbit > 128 {
                return formerr();
            }
            rd.at += 1;
            let pbyte = usize::from(pbit & !7) / 8;
            if pbit < 128 {
                // The address's last 16 - pbyte bytes, all of them there.
                let bytelen = 16 - pbyte;
                if rd.left() < bytelen {
                    return formerr();
                }
                let suffix = rd.take(bytelen)?;
                let mut a = [0u8; 16];
                a[pbyte..].copy_from_slice(suffix);
                ntop_into(crate::socket::AF_INET6, a.as_ptr(), out)?;
            }
            if pbit == 0 {
                return Ok(());
            }
            if rd.left() == 0 {
                return formerr();
            }
            out.addstr(b" ")?;
            addname(rd, origin, out)?;
        }
        _ => return Err(PrErr::Hexify(b"")),
    }
    Ok(())
}

/// The data that did not print, as hex and its printable bytes, `comment`
/// after its length (`hexify:` in glibc's `ns_sprintrrf`): RFC 3597's `\#`
/// form, with glibc's rows -- each starting a line, so none is spaced.
fn hexify(comment: &[u8], rd: &Rd, rdlen: usize, out: &mut Pr) -> Result<(), PrErr> {
    let mut tmp = Tmp::new();
    tmp.s(b"\\# ").d(i64::from(rd.left_shown()));
    tmp.s(if rdlen != 0 { b" (" } else { b"" }).s(comment);
    out.addstr(tmp.get())?;
    let data = rd.rest();
    for row in data.chunks(16) {
        let mut tmp = Tmp::new();
        tmp.s(b"\n\t");
        for &b in row {
            let hex = b"0123456789abcdef";
            tmp.s(&[hex[usize::from(b >> 4)], hex[usize::from(b & 0xf)], b' ']);
        }
        out.addstr(tmp.get())?;
        if row.len() < 16 {
            out.addstr(b")")?;
            out.addtab(tmp.get().len() + 1, 48, false)?;
        }
        let mut tmp = Tmp::new();
        tmp.s(b"; ");
        for &b in row {
            tmp.s(&[if (0x20..=0x7e).contains(&b) { b } else { b'.' }]);
        }
        out.addstr(tmp.get())?;
    }
    Ok(())
}

/// Print one record in zone-file form into `buf` (`buflen` bytes): the
/// owner (a tab stop, or blank when it is `name_ctx`'s, `@` for `origin`
/// itself, relative under it), the TTL as `ns_format_ttl` writes it, the
/// class and type (`CLASSn` and `TYPEn` for ones with no name, RFC 3597's
/// form), and the data by its type -- names relative to `origin`, strings
/// quoted, and as hex in RFC 3597's `\#` form a type it has no printer for
/// (CERT, TKEY, TSIG and OPT among them) or data that does not read, the
/// latter marked `; RR format error`. The names in the data are expanded
/// against the message `[msg, msg + msglen)`. glibc's as its June 2026
/// fixes left it (CVE-2026-5435, CVE-2026-6238, bug 34289).
///
/// The characters written, or -1: `ENOSPC` for a buffer too small at any
/// step, with what came before it written, as glibc's; a TTL that does
/// not fit returns -1 with `errno` as it was, as glibc's does.
/// Deprecated by glibc.
///
/// # Safety
///
/// `msg` holds `msglen` bytes and `rdata` `rdlen` of them; `name`,
/// `name_ctx` and `origin` are NULL or NUL-terminated strings; `buf` holds
/// `buflen` bytes.
#[allow(clippy::too_many_arguments)] // glibc's signature
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_sprintrrf(
    msg: *const u8,
    msglen: usize,
    name: *const u8,
    class: i32,
    type_: i32,
    ttl: u64,
    rdata: *const u8,
    rdlen: usize,
    name_ctx: *const u8,
    origin: *const u8,
    buf: *mut u8,
    buflen: usize,
) -> i32 {
    if buf.is_null() || (rdata.is_null() && rdlen > 0) {
        crate::errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: the caller's contract.
    let (name, ctx, origin) = unsafe {
        (
            c_text(name).unwrap_or(b""),
            c_text(name_ctx),
            c_text(origin),
        )
    };
    let mut out = Pr { buf, len: buflen };
    let mut rd = Rd {
        msg,
        msglen,
        rdata,
        at: 0,
        end: rdlen,
    };
    let r = (|| -> Result<(), PrErr> {
        let mut spaced = false;
        // The owner.
        if ctx.is_some_and(|c| samename(c, name) == Some(true)) {
            out.addstr(b"\t\t\t")?;
        } else {
            let mut len = prune_origin(name, origin);
            if !name.is_empty() && len == 0 {
                out.addstr(b"@\t\t\t")?;
            } else {
                if !name.is_empty() {
                    out.addstr(&name[..len])?;
                }
                if name.is_empty() || wants_dot(name, len, origin) {
                    out.addstr(b".")?;
                    len += 1;
                }
                spaced = out.addtab(len, 24, spaced)?;
            }
        }
        // The TTL, class and type: A6 by name, though the type table has
        // none for it -- glibc's cannot grow, its size being ABI.
        let start = out.buf as usize;
        // SAFETY: the buffer holds `out.len` bytes from its position.
        let x = unsafe { ns_format_ttl(ttl, out.buf, out.len) };
        let Ok(x) = usize::try_from(x) else {
            return Err(PrErr::Fail);
        };
        out.addlen(x);
        addsym(&crate::res_debug::__p_class_syms, class, b"CLASS", &mut out)?;
        if type_ == t::A6 {
            out.addstr(b" A6")?;
        } else {
            addsym(&crate::res_debug::__p_type_syms, type_, b"TYPE", &mut out)?;
        }
        spaced = out.addtab(out.buf as usize - start, 16, spaced)?;
        match rdata_text(type_, &mut rd, origin, &mut out, &mut spaced) {
            Err(PrErr::Hexify(comment)) => hexify(comment, &rd, rdlen, &mut out),
            other => other,
        }
    })();
    match r {
        Ok(()) => i32::try_from(out.buf as usize - buf as usize).unwrap_or(i32::MAX),
        Err(_) => -1,
    }
}

/// Print a record parsed from a message ([`crate::resolv::ns_parserr`]'s)
/// with [`ns_sprintrrf`], its names expanded against the message.
/// Deprecated by glibc.
///
/// # Safety
///
/// `handle` and `rr` are NULL or `ns_initparse`'s and `ns_parserr`'s; as
/// [`ns_sprintrrf`] for the rest.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn ns_sprintrr(
    handle: *const NsMsg,
    rr: *const crate::resolv::NsRr,
    name_ctx: *const u8,
    origin: *const u8,
    buf: *mut u8,
    buflen: usize,
) -> i32 {
    if handle.is_null() || rr.is_null() {
        crate::errno::set_errno(errno::EFAULT);
        return -1;
    }
    // SAFETY: the caller's contract.
    let (h, r) = unsafe { (&*handle, &*rr) };
    let msglen = (h.eom as usize).saturating_sub(h.msg as usize);
    // SAFETY: as above; the record's fields are ns_parserr's.
    unsafe {
        ns_sprintrrf(
            h.msg,
            msglen,
            r.name.as_ptr(),
            i32::from(r.rr_class),
            i32::from(r.type_),
            u64::from(r.ttl),
            r.rdata,
            usize::from(r.rdlength),
            name_ctx,
            origin,
            buf,
            buflen,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers, one line a call
    /// (`posix/tools/oracle/nsutil_harness.py`, which says the forms).
    const ORACLE: &str = include_str!("nsutil_oracle.txt");

    /// A text as the harness writes it: `\xHH` for a byte, `\x` for none,
    /// `x*N` for N `x`s.
    fn untoken(t: &str) -> Vec<u8> {
        if t == "\\x" {
            return Vec::new();
        }
        if let Some(n) = t.strip_prefix("x*") {
            return std::vec![b'x'; n.parse().unwrap()];
        }
        let b = t.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'\\' && b.get(i + 1) == Some(&b'x') {
                out.push(u8::from_str_radix(&t[i + 2..i + 4], 16).unwrap());
                i += 4;
            } else {
                out.push(b[i]);
                i += 1;
            }
        }
        out
    }

    /// Text as the harness writes it.
    fn token(b: &[u8]) -> String {
        use core::fmt::Write;
        if b.is_empty() {
            return "\\x".into();
        }
        if b.len() > 40 && b.iter().all(|&c| c == b'x') {
            return format!("x*{}", b.len());
        }
        b.iter().fold(String::new(), |mut s, &c| {
            if (0x21..=0x7e).contains(&c) && c != b'\\' {
                s.push(c as char);
            } else {
                // Writing into a String cannot fail.
                let _ = write!(s, "\\x{c:02x}");
            }
            s
        })
    }

    fn hex(b: &[u8]) -> String {
        use core::fmt::Write;
        b.iter().fold(String::new(), |mut s, x| {
            // Writing into a String cannot fail.
            let _ = write!(s, "{x:02x}");
            s
        })
    }

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap())
            .collect()
    }

    fn cstring(b: &[u8]) -> Vec<u8> {
        let mut v = b.to_vec();
        v.push(0);
        v
    }

    fn errno_name(e: i32) -> &'static str {
        match e {
            0 => "0",
            errno::EMSGSIZE => "EMSGSIZE",
            errno::EINVAL => "EINVAL",
            errno::ENOSPC => "ENOSPC",
            _ => "other",
        }
    }

    fn a_message(flags: u16) -> NsMsg {
        NsMsg {
            msg: core::ptr::null(),
            eom: core::ptr::null(),
            id: 0,
            flags,
            counts: [0; 4],
            sections: [core::ptr::null(); 4],
            sect: 0,
            rrnum: 0,
            msg_ptr: core::ptr::null(),
        }
    }

    /// What the harness prints after ` = ` for `call`, from this crate's
    /// functions.
    fn ours(call: &str) -> String {
        let f: Vec<&str> = call.split(' ').collect();
        errno::set_errno(0);
        match f[0] {
            "K" => {
                let s = cstring(&untoken(f[1]));
                let mut out = std::vec![0xaau8; 4096];
                // SAFETY: `s` is NUL-terminated; `out` holds every size asked.
                let r =
                    unsafe { ns_makecanon(s.as_ptr(), out.as_mut_ptr(), f[2].parse().unwrap()) };
                let e = if r < 0 { errno::get_errno() } else { 0 };
                let text = if r == 0 {
                    let n = out.iter().position(|&c| c == 0).unwrap();
                    token(&out[..n])
                } else {
                    "-".into()
                };
                format!("{r} {} {text}", errno_name(e))
            }
            "M" | "D" | "U" => {
                let a = cstring(&untoken(f[1]));
                let b = cstring(&untoken(f[2]));
                // SAFETY: both NUL-terminated.
                let r = unsafe {
                    match f[0] {
                        "M" => ns_samename(a.as_ptr(), b.as_ptr()),
                        "D" => ns_samedomain(a.as_ptr(), b.as_ptr()),
                        _ => ns_subdomain(a.as_ptr(), b.as_ptr()),
                    }
                };
                format!("{r}")
            }
            "G" => {
                let flags = u16::from_str_radix(f[1], 16).unwrap();
                format!(
                    "{}",
                    ns_msg_getflag(a_message(flags), f[2].parse().unwrap())
                )
            }
            "L" => {
                let wire = unhex(f[1]);
                let mut out = [0xaau8; 32];
                // SAFETY: `wire` is a name to its root label or a pointer
                // that stops it; `out` holds every size asked.
                let r = unsafe {
                    crate::resolv::ns_name_ntol(
                        wire.as_ptr(),
                        out.as_mut_ptr(),
                        f[2].parse().unwrap(),
                    )
                };
                let e = if r < 0 { errno::get_errno() } else { 0 };
                format!("{r} {} {}", errno_name(e), hex(&out))
            }
            "R" => {
                let msg = [0u8; 64];
                let mut table: Vec<*const u8> = f[1]
                    .split(',')
                    .map(|e| {
                        if e == "-" {
                            core::ptr::null()
                        } else {
                            msg[e.parse::<usize>().unwrap()..].as_ptr()
                        }
                    })
                    .collect();
                let src = msg[f[2].parse::<usize>().unwrap()..].as_ptr();
                let last: usize = f[3].parse().unwrap();
                let base = table.as_mut_ptr();
                // SAFETY: `base..base+last` is within the table.
                unsafe { crate::resolv::ns_name_rollback(src, base, base.add(last)) };
                table
                    .iter()
                    .map(|&p| {
                        if p.is_null() {
                            "-".into()
                        } else {
                            format!("{}", p as usize - msg.as_ptr() as usize)
                        }
                    })
                    .collect::<Vec<String>>()
                    .join(",")
            }
            "F" => {
                let mut out = [0xaau8; 24];
                // SAFETY: `out` holds every size asked.
                let r = unsafe {
                    ns_format_ttl(
                        f[1].parse().unwrap(),
                        out.as_mut_ptr(),
                        f[2].parse().unwrap(),
                    )
                };
                let e = if r < 0 { errno::get_errno() } else { 0 };
                format!("{r} {} {}", errno_name(e), hex(&out))
            }
            "P" => {
                let s = cstring(&untoken(f[1]));
                let mut v = 0xaaaa_aaaa_aaaa_aaaau64;
                // SAFETY: `s` is NUL-terminated; `v` is writable.
                let r = unsafe { ns_parse_ttl(s.as_ptr(), &mut v) };
                let e = if r < 0 { errno::get_errno() } else { 0 };
                format!("{r} {} {v}", errno_name(e))
            }
            "S" => {
                let s = cstring(&untoken(f[1]));
                let mut err = 7;
                // SAFETY: `s` is NUL-terminated; `err` is writable.
                let v = unsafe { ns_datetosecs(s.as_ptr(), &mut err) };
                format!("{v} {err}")
            }
            other => panic!("unknown line kind {other}"),
        }
    }

    #[test]
    fn the_nameser_utilities_answer_as_glibcs() {
        let mut wrong = Vec::new();
        let mut n = 0;
        for line in ORACLE
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (call, want) = line.split_once(" = ").unwrap();
            n += 1;
            let got = ours(call);
            if got != want {
                wrong.push(format!("{call}\n  glibc: {want}\n  ours:  {got}"));
            }
        }
        assert!(n > 600, "the oracle has {n} lines");
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// Where glibc would read or write through NULL, these refuse.
    #[test]
    fn null_is_refused_where_glibc_would_fault() {
        let mut buf = [0u8; 16];
        // SAFETY: NULL is refused before it is read.
        assert_eq!(
            unsafe { ns_makecanon(core::ptr::null(), buf.as_mut_ptr(), 16) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // SAFETY: NUL-terminated text; the NULL destination is refused.
        assert_eq!(
            unsafe { ns_makecanon(c"a".as_ptr().cast(), core::ptr::null_mut(), 16) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        // SAFETY: as above.
        assert_eq!(
            unsafe { ns_samename(core::ptr::null(), c"a".as_ptr().cast()) },
            -1
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { ns_samedomain(c"a".as_ptr().cast(), core::ptr::null()) },
            0
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { ns_subdomain(core::ptr::null(), core::ptr::null()) },
            0
        );
        // SAFETY: as above.
        assert_eq!(
            unsafe { ns_format_ttl(90061, core::ptr::null_mut(), 24) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let mut v = 0u64;
        // SAFETY: as above.
        assert_eq!(unsafe { ns_parse_ttl(core::ptr::null(), &mut v) }, -1);
        assert_eq!(errno::get_errno(), errno::EINVAL);
        // SAFETY: as above.
        assert_eq!(
            unsafe { ns_parse_ttl(c"1h".as_ptr().cast(), core::ptr::null_mut()) },
            -1
        );
        assert_eq!(errno::get_errno(), errno::EFAULT);
        let mut err = 0;
        // SAFETY: as above.
        assert_eq!(unsafe { ns_datetosecs(core::ptr::null(), &mut err) }, 0);
        assert_eq!(err, 1);
        // SAFETY: a NULL `errp` is not written.
        assert_eq!(
            unsafe { ns_datetosecs(c"19900101000000".as_ptr().cast(), core::ptr::null_mut()) },
            631_152_000
        );
        assert_eq!(ns_msg_getflag(a_message(0xffff), 16), 0);
        assert_eq!(ns_msg_getflag(a_message(0xffff), -1), 0);
    }

    /// glibc 2.39's `ns_sprintrrf` and `ns_sprintrr`, one line a call
    /// (`posix/tools/oracle/nsprint_harness.py`, which says the forms).
    const PRINT_ORACLE: &str = include_str!("nsprint_oracle.txt");

    /// An optional text: `-` for NULL.
    fn opt(t: &str) -> Option<Vec<u8>> {
        (t != "-").then(|| cstring(&untoken(t)))
    }

    /// The text's pointer, NULL for none.
    fn ptr(v: Option<&[u8]>) -> *const u8 {
        v.map_or(core::ptr::null(), <[u8]>::as_ptr)
    }

    /// What the harness prints after ` = ` for `call`.
    fn print_ours(call: &str) -> String {
        let f: Vec<&str> = call.split(' ').collect();
        let mut buf = std::vec![0u8; 4096];
        errno::set_errno(0);
        let r = match f[0] {
            "F" => {
                // `<data>+<more>`: the message is both, the data the first.
                let (data, more) = f[5].split_once('+').unwrap_or((f[5], ""));
                let mut rdata = if data == "-" {
                    std::vec![b'x']
                } else {
                    unhex(data)
                };
                let rdlen = if data == "-" { 0 } else { rdata.len() };
                rdata.extend(unhex(more));
                let owner = cstring(&untoken(f[4]));
                let (ctx, origin) = (opt(f[6]), opt(f[7]));
                // SAFETY: each buffer holds what the call is told it does.
                unsafe {
                    ns_sprintrrf(
                        rdata.as_ptr(),
                        if more.is_empty() { rdlen } else { rdata.len() },
                        owner.as_ptr(),
                        f[2].parse().unwrap(),
                        f[1].parse().unwrap(),
                        f[3].parse().unwrap(),
                        rdata.as_ptr(),
                        rdlen,
                        ptr(ctx.as_deref()),
                        ptr(origin.as_deref()),
                        buf.as_mut_ptr(),
                        f[8].parse().unwrap(),
                    )
                }
            }
            "R" => {
                let m = unhex(f[1]);
                let (ctx, origin) = (opt(f[4]), opt(f[5]));
                let mut h = a_message(0);
                // SAFETY: an all-zero record is a valid one to fill.
                let mut rr: crate::resolv::NsRr = unsafe { core::mem::zeroed() };
                // SAFETY: `m` is the message; `h` and `rr` are writable.
                unsafe {
                    assert_eq!(
                        crate::resolv::ns_initparse(
                            m.as_ptr(),
                            i32::try_from(m.len()).unwrap(),
                            &mut h
                        ),
                        0
                    );
                    assert_eq!(
                        crate::resolv::ns_parserr(
                            &mut h,
                            f[2].parse().unwrap(),
                            f[3].parse().unwrap(),
                            &mut rr
                        ),
                        0
                    );
                }
                errno::set_errno(0);
                // SAFETY: as above; `buf` holds the size asked.
                unsafe {
                    ns_sprintrr(
                        &h,
                        &rr,
                        ptr(ctx.as_deref()),
                        ptr(origin.as_deref()),
                        buf.as_mut_ptr(),
                        f[6].parse().unwrap(),
                    )
                }
            }
            other => panic!("unknown line kind {other}"),
        };
        let e = errno::get_errno();
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        format!("{r} {} {}", errno_name(e), token(&buf[..n]))
    }

    #[test]
    fn records_print_as_glibcs() {
        let mut wrong = Vec::new();
        let mut n = 0;
        let lines = PRINT_ORACLE
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'));
        for line in lines.filter(|l| !l.starts_with("S ")) {
            let (call, want) = line.split_once(" = ").unwrap();
            n += 1;
            let got = print_ours(call);
            if got != want {
                wrong.push(format!("{call}\n  glibc: {want}\n  ours:  {got}"));
            }
        }
        assert!(n > 150, "the oracle has {n} lines");
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// The packet glibc's own test of its June 2026 fixes
    /// (`resolv/tst-ns_sprintrr.c`) prints a record from: a response to
    /// www.example.org/IN/ANY, the record its one answer, the answer's
    /// owner a pointer to the question's name.
    fn sweep_packet(type_: u16, rdata: &[u8]) -> Vec<u8> {
        let mut p = b"AA\x81\x80\x00\x01\x00\x01\x00\x00\x00\x00\x03www\x07example\x03org\x00\x00\xff\x00\x01\xc0\x0c".to_vec();
        p.extend_from_slice(&type_.to_be_bytes());
        p.extend_from_slice(&1u16.to_be_bytes());
        p.extend_from_slice(&86400u32.to_be_bytes());
        p.extend_from_slice(&u16::try_from(rdata.len()).unwrap().to_be_bytes());
        p.extend_from_slice(rdata);
        p
    }

    /// `ns_sprintrr` on the packet's answer into `size` of 4096 zeroed
    /// bytes: its answer, `errno` and the text, or `None` when the packet
    /// does not parse.
    fn sweep_answer(p: &[u8], size: usize) -> Option<(i32, i32, Vec<u8>)> {
        let mut h = a_message(0);
        // SAFETY: an all-zero record is a valid one to fill.
        let mut rr: crate::resolv::NsRr = unsafe { core::mem::zeroed() };
        // SAFETY: `p` is the packet; `h` and `rr` are writable.
        unsafe {
            if crate::resolv::ns_initparse(p.as_ptr(), i32::try_from(p.len()).unwrap(), &mut h) != 0
                || crate::resolv::ns_parserr(&mut h, 1, 0, &mut rr) != 0
            {
                return None;
            }
        }
        let mut buf = std::vec![0u8; 4096];
        errno::set_errno(0);
        // SAFETY: as above; `buf` holds more than `size` bytes.
        let r = unsafe {
            ns_sprintrr(
                &h,
                &rr,
                core::ptr::null(),
                core::ptr::null(),
                buf.as_mut_ptr(),
                size,
            )
        };
        let e = errno::get_errno();
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        buf.truncate(n);
        Some((r, e, buf))
    }

    /// One of a sweep's answers as the harness writes it: `rc/errno/k`, `k`
    /// the bytes the text shares with the whole text, `/` and the rest of
    /// the text after them when there is any.
    fn sweep_entry(answer: Option<(i32, i32, Vec<u8>)>, full: &[u8]) -> String {
        let Some((r, e, got)) = answer else {
            return "parse-failed".into();
        };
        let k = full.iter().zip(&got).take_while(|(a, b)| a == b).count();
        let mut s = format!("{r}/{}/{k}", errno_name(e));
        if k < got.len() {
            s.push('/');
            s.push_str(&token(&got[k..]));
        }
        s
    }

    /// What the harness prints after ` = ` for an `S` line: the whole
    /// text, each size's answer, each truncation's.
    fn sweep_ours(type_: u16, rdata: &[u8]) -> String {
        let p = sweep_packet(type_, rdata);
        let Some((r, e, full)) = sweep_answer(&p, 4096) else {
            return "parse-failed".into();
        };
        let mut s = format!("{r} {} {} |", errno_name(e), token(&full));
        for size in 1..=full.len() + 16 {
            s.push(' ');
            s.push_str(&sweep_entry(sweep_answer(&p, size), &full));
        }
        s.push_str(" |");
        for k in 0..=rdata.len() {
            // The message ends where the data, cut to `k` bytes, does.
            let mut t = p[..p.len() - rdata.len() + k].to_vec();
            let at = t.len() - k - 2;
            t[at..at + 2].copy_from_slice(&u16::try_from(k).unwrap().to_be_bytes());
            s.push(' ');
            s.push_str(&sweep_entry(sweep_answer(&t, 4096), &full));
        }
        s
    }

    /// glibc's answers for its own test's records, and a few more, at every
    /// buffer size from 1 past the text's length and with the data cut to
    /// every length: where each piece stops fitting, what a failing call
    /// leaves in the buffer, and how each cut record reads.
    #[test]
    fn records_print_as_glibcs_at_every_size_and_cut() {
        let mut wrong = Vec::new();
        let mut n = 0;
        for line in PRINT_ORACLE.lines().filter(|l| l.starts_with("S ")) {
            let (call, want) = line.split_once(" = ").unwrap();
            let f: Vec<&str> = call.split(' ').collect();
            let rdata = if f[2] == "-" { Vec::new() } else { unhex(f[2]) };
            n += 1;
            let got = sweep_ours(f[1].parse().unwrap(), &rdata);
            if got != want {
                // The answers that differ, by their place in the line.
                let (w, g): (Vec<&str>, Vec<&str>) =
                    (want.split(' ').collect(), got.split(' ').collect());
                let diffs: Vec<String> = (0..w.len().max(g.len()))
                    .filter(|&i| w.get(i) != g.get(i))
                    .take(6)
                    .map(|i| {
                        format!(
                            "  [{i}] glibc: {:?}\n  [{i}] ours:  {:?}",
                            w.get(i),
                            g.get(i)
                        )
                    })
                    .collect();
                wrong.push(format!("{call}\n{}", diffs.join("\n")));
            }
        }
        assert!(n >= 30, "the oracle has {n} sweeps");
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }
}
