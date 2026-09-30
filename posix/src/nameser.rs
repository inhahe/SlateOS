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
}
