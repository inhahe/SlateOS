//! `<uchar.h>` — the Unicode code-unit conversions: `mbrtoc8` and `c8rtomb`
//! (C23), `mbrtoc16` and `c16rtomb`, `mbrtoc32` and `c32rtomb` (C11).
//!
//! The multibyte encoding is UTF-8, as it is for `mbrtowc` and the rest of
//! [`crate::wchar`], which this shares its decoder ([`crate::wchar::decode`])
//! and its `mbstate_t` ([`MbstateT`]) with: strict UTF-8, each sequence
//! refused at the first byte no completion could make valid. (Until
//! 2026-09-29 these four refused every byte above 0x7F -- a C locale of
//! ASCII, where `mbrtowc` beside them read UTF-8. Which encoding the library
//! should *say* it has is open-questions D-Q7; that these must agree with
//! `mbrtowc` is true either way.)
//!
//! Replayed against glibc 2.39 in its C.UTF-8 locale
//! (`posix/tools/oracle/multibyte_harness.py`, `multibyte_oracle.txt`), with
//! these differences, each on purpose:
//!
//! - glibc's decoder refuses an overlong form or a surrogate at its last
//!   byte, and reads four-byte forms past U+10FFFF; this refuses both where
//!   the sequence first cannot be valid, as `mbrtowc` here does and as
//!   glibc's own `c8rtomb` does.
//! - glibc's `c32rtomb` writes code points past U+10FFFF as the old five-
//!   and six-byte forms; this refuses them, as UTF-8 has not had them since
//!   2003 (RFC 3629) and nothing here would read them back.
//! - glibc's `c16rtomb(NULL, ...)` after a lone high surrogate answers 1;
//!   this answers C's `c16rtomb(buf, u'\0', ps)`, an encoding error, as
//!   glibc's `c8rtomb` does for a sequence begun.
//! - glibc's `mbrtoc16(NULL, NULL, 0, ps)` with a low surrogate to hand out
//!   writes it through the NULL pointer and crashes; this hands it to no one.
//!
//! A NULL `ps` means the function's own internal state, the calling
//! thread's ([`crate::wchar::internal`]).

use crate::errno::{EILSEQ, set_errno};
use crate::wchar::{Decoded, ILSEQ, INCOMPLETE, decode, internal, state_for};

pub use crate::wchar::MbstateT;

/// `char8_t` — a UTF-8 code unit (C23).
pub type Char8T = u8;

/// `char16_t` — a UTF-16 code unit.
pub type Char16T = u16;

/// `char32_t` — a Unicode code point.
pub type Char32T = u32;

/// `(size_t)(-3)`: a code unit handed out from the state, no byte read.
const FROM_STATE: usize = usize::MAX.wrapping_sub(2);

/// `c`'s UTF-8 encoding into `out`, and its length; `None` for a surrogate
/// or a number past U+10FFFF -- `wcrtomb`'s encoder.
fn utf8(c: u32, out: &mut [u8; 4]) -> Option<usize> {
    match crate::wchar::utf8_encode(c, out) {
        0 => None,
        len => Some(len),
    }
}

/// Write `bytes` to the caller's `s`.
///
/// # Safety
///
/// `s` has room for `bytes`: `MB_CUR_MAX`, 4, always does.
unsafe fn put(s: *mut u8, bytes: &[u8]) {
    for (i, &b) in bytes.iter().enumerate() {
        // SAFETY: the caller's contract.
        unsafe { s.add(i).write(b) };
    }
}

/// -1, `errno` `EILSEQ`.
fn ilseq() -> usize {
    set_errno(EILSEQ);
    ILSEQ
}

/// The arguments a NULL `s` stands for: C says `mbrtoc*(NULL, s, n, ps)` with
/// a NULL `s` is `mbrtoc*(NULL, "", 1, ps)`.
fn or_empty<T>(out: *mut T, s: *const u8, n: usize) -> (*mut T, *const u8, usize) {
    if s.is_null() {
        (core::ptr::null_mut(), b"\0".as_ptr(), 1)
    } else {
        (out, s, n)
    }
}

// ---------------------------------------------------------------------------
// mbrtoc32, c32rtomb
// ---------------------------------------------------------------------------

/// `mbrtoc32`: the next character of `s`, at most `n` bytes of it, into
/// `*pc32` -- `mbrtowc`'s answer, a `char32_t` being a code point as a
/// `wchar_t` is. 0 for the NUL, the bytes it took, `(size_t)-2` for a
/// character begun and not finished, `(size_t)-1` `EILSEQ` for no character.
///
/// # Safety
///
/// `pc32` NULL or writable; `s` NULL or `n` readable bytes; `ps` NULL or a
/// valid `mbstate_t`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mbrtoc32(
    pc32: *mut Char32T,
    s: *const u8,
    n: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::MBRTOC32) };
    let (pc32, s, n) = or_empty(pc32, s, n);
    // SAFETY: `s` has `n` readable bytes.
    match unsafe { decode(state, s, n) } {
        Decoded::Char { cp, took } => {
            if !pc32.is_null() {
                // SAFETY: the caller's writable `char32_t`.
                unsafe { pc32.write(cp) };
            }
            if cp == 0 { 0 } else { took }
        }
        Decoded::Incomplete => INCOMPLETE,
        Decoded::Invalid => ilseq(),
    }
}

/// `c32rtomb`: `c32` in UTF-8 into `s`, and how many bytes; `(size_t)-1`
/// `EILSEQ` for a surrogate or a number past U+10FFFF. A NULL `s` is
/// `c32rtomb(buf, U'\0', ps)`: the state initial again, and 1.
///
/// # Safety
///
/// `s` NULL or room for `MB_CUR_MAX` (4) bytes; `ps` NULL or a valid
/// `mbstate_t`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn c32rtomb(s: *mut u8, c32: Char32T, ps: *mut MbstateT) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::C32RTOMB) };
    if s.is_null() {
        state.reset();
        return 1;
    }
    let mut out = [0u8; 4];
    let Some(len) = utf8(c32, &mut out) else {
        return ilseq();
    };
    if c32 == 0 {
        state.reset();
    }
    // SAFETY: room for 4, by the contract.
    unsafe { put(s, out.get(..len).unwrap_or(&[])) };
    len
}

// ---------------------------------------------------------------------------
// mbrtoc16, c16rtomb
// ---------------------------------------------------------------------------

/// `mbrtoc16`: `mbrtoc32`'s, as UTF-16 code units. A character past U+FFFF
/// is two: its high surrogate now, with the bytes it took; its low one from
/// the next call, which reads nothing and returns `(size_t)-3`.
///
/// # Safety
///
/// As [`mbrtoc32`], `pc16` a `char16_t`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mbrtoc16(
    pc16: *mut Char16T,
    s: *const u8,
    n: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::MBRTOC16) };
    let (pc16, s, n) = or_empty(pc16, s, n);
    let store = |unit: u16| {
        if !pc16.is_null() {
            // SAFETY: the caller's writable `char16_t`.
            unsafe { pc16.write(unit) };
        }
    };
    if state.pending() == MbstateT::LOW_SURROGATE {
        let low = state.surrogate();
        state.reset();
        store(low);
        return FROM_STATE;
    }
    // SAFETY: `s` has `n` readable bytes.
    match unsafe { decode(state, s, n) } {
        Decoded::Char { cp, took } => {
            if let Some(v) = cp.checked_sub(0x1_0000) {
                // A code point is at most 0x10FFFF, so v < 2^20.
                store(0xD800 | (v >> 10) as u16);
                state.keep_surrogate(MbstateT::LOW_SURROGATE, 0xDC00 | (v & 0x3FF) as u16);
            } else {
                store(cp as u16);
            }
            if cp == 0 { 0 } else { took }
        }
        Decoded::Incomplete => INCOMPLETE,
        Decoded::Invalid => ilseq(),
    }
}

/// `c16rtomb`: the character `c16` ends, in UTF-8 into `s`, and how many
/// bytes. A high surrogate begins a character and writes nothing (0); the
/// low one after it ends it (4). A low surrogate with no high one before it,
/// or anything but a low one after a high one, is `(size_t)-1` `EILSEQ`. A
/// NULL `s` is `c16rtomb(buf, u'\0', ps)`.
///
/// # Safety
///
/// As [`c32rtomb`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn c16rtomb(s: *mut u8, c16: Char16T, ps: *mut MbstateT) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::C16RTOMB) };
    let mut discard = [0u8; 4];
    let (s, c16) = if s.is_null() {
        (discard.as_mut_ptr(), 0)
    } else {
        (s, c16)
    };
    let c = if state.pending() == MbstateT::HIGH_SURROGATE {
        let high = state.surrogate();
        state.reset();
        if !(0xDC00..=0xDFFF).contains(&c16) {
            return ilseq();
        }
        // Twenty bits from the pair, over U+10000: at most U+10FFFF.
        (((u32::from(high) & 0x3FF) << 10) | (u32::from(c16) & 0x3FF)).wrapping_add(0x1_0000)
    } else {
        match c16 {
            0xD800..=0xDBFF => {
                state.keep_surrogate(MbstateT::HIGH_SURROGATE, c16);
                return 0;
            }
            0xDC00..=0xDFFF => {
                state.reset();
                return ilseq();
            }
            _ => u32::from(c16),
        }
    };
    let mut out = [0u8; 4];
    let Some(len) = utf8(c, &mut out) else {
        return ilseq();
    };
    // SAFETY: room for 4, by the contract (or `discard`).
    unsafe { put(s, out.get(..len).unwrap_or(&[])) };
    len
}

// ---------------------------------------------------------------------------
// mbrtoc8, c8rtomb (C23)
// ---------------------------------------------------------------------------

/// `mbrtoc8` (C23): `mbrtoc32`'s, as UTF-8 code units. A character of `k`
/// units gives its first now, with the bytes it took, and the rest from the
/// next `k - 1` calls, which read nothing and return `(size_t)-3`.
///
/// # Safety
///
/// As [`mbrtoc32`], `pc8` a `char8_t`.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn mbrtoc8(
    pc8: *mut Char8T,
    s: *const u8,
    n: usize,
    ps: *mut MbstateT,
) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::MBRTOC8) };
    let (pc8, s, n) = or_empty(pc8, s, n);
    let store = |unit: u8| {
        if !pc8.is_null() {
            // SAFETY: the caller's writable `char8_t`.
            unsafe { pc8.write(unit) };
        }
    };
    if state.pending() == MbstateT::UTF8_UNITS {
        store(state.next_unit());
        return FROM_STATE;
    }
    // SAFETY: `s` has `n` readable bytes.
    match unsafe { decode(state, s, n) } {
        Decoded::Char { cp, took } => {
            let mut out = [0u8; 4];
            // What decode gives is a scalar value, which always encodes.
            let len = utf8(cp, &mut out).unwrap_or(1);
            let [first, rest @ ..] = out;
            store(first);
            if len > 1 {
                state.keep_units(rest.get(..len.wrapping_sub(1)).unwrap_or(&[]));
            }
            if cp == 0 { 0 } else { took }
        }
        Decoded::Incomplete => INCOMPLETE,
        Decoded::Invalid => ilseq(),
    }
}

/// `c8rtomb` (C23): the character the code unit `c8` ends, in UTF-8 into
/// `s`, and how many bytes; a unit that does not end one is kept in the state
/// and writes nothing (0). A unit that cannot come where it does -- a
/// continuation without a lead, a lead a sequence cannot start with, a
/// second unit that makes an overlong form, a surrogate or a number past
/// U+10FFFF -- is `(size_t)-1` `EILSEQ`. A NULL `s` is
/// `c8rtomb(buf, u8'\0', ps)`: 1, or an error in the middle of a character.
///
/// # Safety
///
/// As [`c32rtomb`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn c8rtomb(s: *mut u8, c8: Char8T, ps: *mut MbstateT) -> usize {
    // SAFETY: the caller's state, or this thread's own for this function.
    let state = unsafe { &mut *state_for(ps, internal::C8RTOMB) };
    let mut discard = [0u8; 4];
    let (s, c8) = if s.is_null() {
        (discard.as_mut_ptr(), 0)
    } else {
        (s, c8)
    };
    // The same byte-by-byte decoding as `mbrtowc`'s, one unit at a time.
    // SAFETY: one readable byte.
    match unsafe { decode(state, &raw const c8, 1) } {
        Decoded::Char { cp, .. } => {
            let mut out = [0u8; 4];
            let len = utf8(cp, &mut out).unwrap_or(1);
            // SAFETY: room for 4, by the contract (or `discard`).
            unsafe { put(s, out.get(..len).unwrap_or(&[])) };
            len
        }
        Decoded::Incomplete => 0,
        Decoded::Invalid => ilseq(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr::{null, null_mut};

    /// glibc 2.39's answers (`posix/tools/oracle/multibyte_harness.py`).
    const ORACLE: &str = include_str!("multibyte_oracle.txt");

    fn unhex(h: &str) -> Vec<u8> {
        (0..h.len() / 2)
            .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap())
            .collect()
    }

    /// Whether `bytes` is a prefix of a well-formed UTF-8 sequence, by
    /// Unicode's table 3-7 written out afresh, and how long its sequence is.
    fn well_formed_prefix(bytes: &[u8]) -> Option<usize> {
        let &lead = bytes.first()?;
        let (len, second) = match lead {
            0x00..=0x7F => (1, 0x80..=0xBF),
            0xC2..=0xDF => (2, 0x80..=0xBF),
            0xE0 => (3, 0xA0..=0xBF),
            0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80..=0xBF),
            0xED => (3, 0x80..=0x9F),
            0xF0 => (4, 0x90..=0xBF),
            0xF1..=0xF3 => (4, 0x80..=0xBF),
            0xF4 => (4, 0x80..=0x8F),
            _ => return None,
        };
        (bytes.len() <= len
            && bytes.get(1).is_none_or(|b| second.contains(b))
            && bytes.iter().skip(2).all(|b| (0x80..=0xBF).contains(b)))
        .then_some(len)
    }

    /// What C's rules and strict UTF-8 say `func` gives for `input`, fed a
    /// byte a call (`one`) or all that is left: the oracle's steps, worked
    /// out from [`well_formed_prefix`] and Rust's own encoders -- nothing of
    /// `decode`'s -- so that the replay holds the library to the rules, and
    /// the rules to glibc's answers wherever glibc is strict.
    fn model(func: &str, input: &[u8], one: bool) -> Vec<String> {
        let mut out = Vec::new();
        let mut at = 0;
        let mut begun: Vec<u8> = Vec::new();
        let mut queue: std::collections::VecDeque<String> = Default::default();
        loop {
            if let Some(unit) = queue.pop_front() {
                out.push(format!("-3:{unit}"));
                continue;
            }
            let left = input.len() - at;
            let n = if one { left.min(1) } else { left };
            let mut took = 0;
            let done = loop {
                if took == n {
                    break None;
                }
                begun.push(input[at + took]);
                took += 1;
                match well_formed_prefix(&begun) {
                    None => break Some(Err(())),
                    Some(len) if len == begun.len() => break Some(Ok(())),
                    Some(_) => {}
                }
            };
            at += took;
            match done {
                None => {
                    out.push("-2:-".to_string());
                    if at >= input.len() {
                        return out;
                    }
                }
                Some(Err(())) => {
                    out.push("-1!EILSEQ".to_string());
                    return out;
                }
                Some(Ok(())) => {
                    let c = core::str::from_utf8(&begun)
                        .unwrap()
                        .chars()
                        .next()
                        .unwrap();
                    begun.clear();
                    let units: Vec<String> = match func {
                        "mbrtoc8" => {
                            let mut b = [0; 4];
                            c.encode_utf8(&mut b)
                                .bytes()
                                .map(|u| format!("{u:02x}"))
                                .collect()
                        }
                        "mbrtoc16" => {
                            let mut b = [0; 2];
                            c.encode_utf16(&mut b)
                                .iter()
                                .map(|u| format!("{u:04x}"))
                                .collect()
                        }
                        _ => vec![format!("{:08x}", u32::from(c))],
                    };
                    let r = if c == '\0' { 0 } else { took };
                    out.push(format!("{r}:{}", units[0]));
                    if c == '\0' {
                        return out;
                    }
                    queue.extend(units.into_iter().skip(1));
                }
            }
        }
    }

    /// A step as the oracle writes it.
    fn step(r: usize, stored: Option<String>) -> String {
        match (r as isize, stored) {
            (-1, _) => "-1!EILSEQ".to_string(),
            (-2, _) => "-2:-".to_string(),
            (r, Some(s)) => format!("{r}:{s}"),
            (r, None) => format!("{r}:-"),
        }
    }

    /// Run one of the three `mbrto*` over `input`, the oracle's way.
    fn run_mbrto(func: &str, input: &[u8], one: bool) -> Vec<String> {
        let mut st = MbstateT::new();
        let mut at = 0;
        let mut out = Vec::new();
        for _ in 0..64 {
            let left = input.len() - at;
            let n = if one { left.min(1) } else { left };
            let p = input[at..].as_ptr();
            let (r, unit) = unsafe {
                match func {
                    "mbrtoc8" => {
                        let mut u: u8 = 0x55;
                        let r = mbrtoc8(&raw mut u, p, n, &raw mut st);
                        (r, format!("{u:02x}"))
                    }
                    "mbrtoc16" => {
                        let mut u: u16 = 0x5555;
                        let r = mbrtoc16(&raw mut u, p, n, &raw mut st);
                        (r, format!("{u:04x}"))
                    }
                    _ => {
                        let mut u: u32 = 0x5555;
                        let r = mbrtoc32(&raw mut u, p, n, &raw mut st);
                        (r, format!("{u:08x}"))
                    }
                }
            };
            match r as isize {
                -1 => {
                    out.push(step(r, None));
                    break;
                }
                -2 => {
                    out.push(step(r, None));
                    at += n;
                    if at >= input.len() {
                        break;
                    }
                    continue;
                }
                _ => out.push(step(r, Some(unit))),
            }
            if r == 0 {
                break;
            }
            if r != FROM_STATE {
                at += r;
            }
        }
        out
    }

    /// Every `mbrto*` line of the oracle: the library gives what the rules
    /// give ([`model`]), and the rules give glibc's answer but where glibc's
    /// decoder is laxer -- a -2 where the character can no longer be valid,
    /// or a four-byte form past U+10FFFF read as a character -- and there the
    /// rules' -1 comes where glibc still waits or reads on.
    #[test]
    fn mbrto_is_the_rules_and_the_rules_are_glibcs() {
        let (mut replayed, mut laxer) = (0, 0);
        for line in ORACLE.lines() {
            let Some((head, steps)) = line.split_once(" = ") else {
                continue;
            };
            let f: Vec<&str> = head.split(' ').collect();
            let [func @ ("mbrtoc8" | "mbrtoc16" | "mbrtoc32"), input, feed] = f[..] else {
                continue;
            };
            let input = unhex(input);
            let one = feed == "one";
            let rules = model(func, &input, one);
            assert_eq!(run_mbrto(func, &input, one), rules, "{line}");
            let glibc: Vec<String> = steps.split(' ').map(str::to_string).collect();
            if rules != glibc {
                laxer += 1;
                // The rules end in -1, where glibc's steps agree up to it.
                let last = rules.len() - 1;
                assert_eq!(rules[last], "-1!EILSEQ", "{line}");
                assert_eq!(rules[..last], glibc[..last.min(glibc.len())], "{line}");
                let past_unicode = input.windows(2).any(|w| w[0] == 0xF4 && w[1] >= 0x90);
                assert!(
                    glibc.get(last).is_some_and(|s| s == "-2:-") || past_unicode,
                    "{line}: glibc differs other than by waiting"
                );
            }
            replayed += 1;
        }
        assert_eq!(replayed, 150, "every mbrto* line");
        assert!(
            (10..40).contains(&laxer),
            "{laxer} lines where glibc is laxer"
        );
    }

    /// The strict decoder's timing, exactly: -2 for each byte that can still
    /// begin a character, -1 at the first that cannot.
    #[test]
    fn a_sequence_is_refused_where_it_becomes_impossible() {
        for (input, steps) in [
            (&b"\xE0\x80\xAF"[..], 1), // overlong: refused at its second byte
            (b"\xED\xA0\x80", 1),      // a surrogate: likewise
            (b"\xF4\x90\x80\x80", 1),  // past U+10FFFF: likewise
            (b"\xF0\x80\x82\xAF", 1),
            (b"\xC0\xAF", 0), // never a lead
            (b"\xF8\x80", 0),
            (b"\xC3\x28", 1), // no continuation
            (b"\xE2\x82\x28", 2),
        ] {
            let mut st = MbstateT::new();
            for i in 0..steps {
                let r = unsafe { mbrtoc32(null_mut(), input[i..].as_ptr(), 1, &raw mut st) };
                assert_eq!(r, INCOMPLETE, "{input:02x?} byte {i}");
            }
            let r = unsafe { mbrtoc32(null_mut(), input[steps..].as_ptr(), 1, &raw mut st) };
            assert_eq!(r, ILSEQ, "{input:02x?}");
            assert!(st.is_initial(), "the state is initial again after an error");
        }
    }

    fn stored(buf: &[u8], r: usize) -> String {
        if r == 0 || r == ILSEQ {
            "-".to_string()
        } else {
            buf[..r]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<Vec<_>>()
                .concat()
        }
    }

    /// Every `c8rtomb`, `c16rtomb` and `c32rtomb` line, but `c32rtomb` past
    /// U+10FFFF and `c16rtomb`'s none (glibc's c16 ones are all strict).
    #[test]
    fn c_rtomb_is_glibcs() {
        let mut replayed = 0;
        for line in ORACLE.lines() {
            let Some((head, steps)) = line.split_once(" = ") else {
                continue;
            };
            let f: Vec<&str> = head.split(' ').collect();
            let [func @ ("c8rtomb" | "c16rtomb" | "c32rtomb"), units, "all"] = f[..] else {
                continue;
            };
            let mut st = MbstateT::new();
            let mut got = Vec::new();
            let width = match func {
                "c8rtomb" => 2,
                "c16rtomb" => 4,
                _ => 8,
            };
            let list: Vec<u32> = (0..units.len() / width)
                .map(|i| u32::from_str_radix(&units[i * width..(i + 1) * width], 16).unwrap())
                .collect();
            let feed: Vec<u32> = if func == "c32rtomb" {
                list.clone()
            } else {
                list.iter().copied().chain([0]).collect()
            };
            for &u in &feed {
                let mut buf = [0u8; 8];
                let r = unsafe {
                    match func {
                        "c8rtomb" => c8rtomb(buf.as_mut_ptr(), u as u8, &raw mut st),
                        "c16rtomb" => c16rtomb(buf.as_mut_ptr(), u as u16, &raw mut st),
                        _ => c32rtomb(buf.as_mut_ptr(), u, &raw mut st),
                    }
                };
                let s = stored(&buf, r);
                got.push(if r == ILSEQ {
                    "-1:-!EILSEQ".to_string()
                } else {
                    format!("{}:{s}", r as isize)
                });
                if r == ILSEQ {
                    break;
                }
            }
            let want: Vec<String> = steps.split(' ').map(str::to_string).collect();
            if func == "c32rtomb" && list[0] > 0x10_FFFF && list[0] < 0x8000_0000 {
                // glibc writes the old five- and six-byte forms; not here.
                assert_eq!(got, ["-1:-!EILSEQ"], "{line}");
            } else {
                assert_eq!(got, want, "{line}");
            }
            replayed += 1;
        }
        assert_eq!(replayed, 44, "every c*rtomb line");
    }

    /// A NULL `s`: the same answers as glibc's but for the two above.
    #[test]
    fn a_null_s_is_cs() {
        let find = |name: &str| {
            ORACLE
                .lines()
                .find_map(|l| l.strip_prefix(name)?.strip_prefix(" = "))
                .unwrap_or_else(|| panic!("{name}"))
                .to_string()
        };
        unsafe {
            let mut st = MbstateT::new();
            let mut u: u8 = 0x55;
            let a = mbrtoc8(null_mut(), null(), 0, &raw mut st) as isize;
            let b = mbrtoc8(&raw mut u, b"a".as_ptr(), 1, &raw mut st) as isize;
            assert_eq!(format!("{a} {b}"), find("mbrtoc8-null-initial"));

            let mut st = MbstateT::new();
            let a = mbrtoc8(&raw mut u, b"\xF0\x9F\x98\x80".as_ptr(), 4, &raw mut st) as isize;
            let b = mbrtoc8(null_mut(), null(), 0, &raw mut st) as isize;
            assert_eq!(format!("{a} {b}"), find("mbrtoc8-null-pending"));

            let mut st = MbstateT::new();
            let mut buf = [0u8; 8];
            let a = c8rtomb(null_mut(), 0x41, &raw mut st) as isize;
            let b = c8rtomb(buf.as_mut_ptr(), 0x41, &raw mut st) as isize;
            assert_eq!(format!("{a} {b}"), find("c8rtomb-null-initial"));

            let mut st = MbstateT::new();
            let a = c8rtomb(buf.as_mut_ptr(), 0xC3, &raw mut st) as isize;
            let b = c8rtomb(null_mut(), 0x41, &raw mut st) as isize;
            assert_eq!(format!("{a} {b}"), find("c8rtomb-null-partial"));

            // glibc crashes here; the low surrogate goes to no one.
            let mut st = MbstateT::new();
            let mut w: u16 = 0x5555;
            let a = mbrtoc16(&raw mut w, b"\xF0\x9F\x98\x80".as_ptr(), 4, &raw mut st);
            let b = mbrtoc16(null_mut(), null(), 0, &raw mut st);
            assert_eq!((a, w, b), (4, 0xD83D, FROM_STATE));
            assert!(st.is_initial());
            assert_eq!(find("probe-4"), "crash");

            // glibc answers 1; C's c16rtomb(buf, u'\0', ps) is an error.
            let mut st = MbstateT::new();
            let a = c16rtomb(buf.as_mut_ptr(), 0xD83D, &raw mut st);
            let b = c16rtomb(null_mut(), 0, &raw mut st);
            assert_eq!((a, b), (0, ILSEQ));
            assert_eq!(find("c16rtomb-null-partial"), "0 1");
            assert!(st.is_initial());
        }
    }

    /// `mbsinit` sees a unit waiting to be handed out, or a surrogate
    /// waiting for its pair, as a state that is not initial.
    #[test]
    fn a_state_with_something_to_hand_out_is_not_initial() {
        unsafe {
            let mut st = MbstateT::new();
            let mut u = 0u8;
            mbrtoc8(&raw mut u, "é".as_ptr(), 2, &raw mut st);
            assert_eq!(crate::wchar::mbsinit(&raw const st), 0);
            // The unit kept comes from the state, however little is given.
            assert_eq!(
                mbrtoc8(&raw mut u, b"".as_ptr(), 0, &raw mut st),
                FROM_STATE
            );
            assert_eq!(u, 0xA9);
            assert_eq!(crate::wchar::mbsinit(&raw const st), 1);
            // With a NULL `s` it goes to no one: C's mbrtoc8(NULL, "", 1, ps).
            mbrtoc8(&raw mut u, "é".as_ptr(), 2, &raw mut st);
            u = 0;
            assert_eq!(mbrtoc8(&raw mut u, null(), 0, &raw mut st), FROM_STATE);
            assert_eq!(u, 0);
            assert_eq!(crate::wchar::mbsinit(&raw const st), 1);
            let mut buf = [0u8; 4];
            c16rtomb(buf.as_mut_ptr(), 0xD800, &raw mut st);
            assert_eq!(crate::wchar::mbsinit(&raw const st), 0);
        }
    }

    /// With a NULL `ps` each function has its own state, and each thread its
    /// own: a character half-read by one is not another's.
    #[test]
    fn a_null_ps_is_the_functions_own_and_the_threads_own() {
        unsafe {
            let mut u = 0u32;
            // mbrtoc32 begins a character; mbrtowc and mbrlen do not see it.
            assert_eq!(
                mbrtoc32(&raw mut u, b"\xC3".as_ptr(), 1, null_mut()),
                INCOMPLETE
            );
            let mut w: i32 = 0;
            assert_eq!(
                crate::wchar::mbrtowc(&raw mut w, b"a".as_ptr(), 1, null_mut()),
                1
            );
            assert_eq!(crate::wchar::mbrlen(b"a".as_ptr(), 1, null_mut()), 1);
            // Another thread's mbrtoc32 does not see it either.
            let other = std::thread::spawn(|| {
                let mut u = 0u32;
                let r = mbrtoc32(&raw mut u, b"b".as_ptr(), 1, null_mut());
                (r, u)
            })
            .join()
            .unwrap();
            assert_eq!(other, (1, 0x62));
            // And this thread's finishes it.
            assert_eq!(mbrtoc32(&raw mut u, b"\xA9".as_ptr(), 1, null_mut()), 1);
            assert_eq!(u, 0xE9);
        }
    }

    #[test]
    fn the_types_are_cs() {
        assert_eq!(core::mem::size_of::<Char8T>(), 1);
        assert_eq!(core::mem::size_of::<Char16T>(), 2);
        assert_eq!(core::mem::size_of::<Char32T>(), 4);
        let line = ORACLE
            .lines()
            .find_map(|l| l.strip_prefix("sizeof mbstate_t = "))
            .unwrap();
        assert_eq!(core::mem::size_of::<MbstateT>().to_string(), line);
        assert!(MbstateT::new().is_initial());
    }

    /// Round trips: every scalar value's UTF-8 through `c32rtomb` and back
    /// through `mbrtoc32`, and through UTF-16 and UTF-8 units.
    #[test]
    fn every_scalar_value_round_trips() {
        for c in (0..=0x10_FFFFu32)
            .filter(|c| !(0xD800..=0xDFFF).contains(c))
            .step_by(7)
        {
            unsafe {
                let mut buf = [0u8; 4];
                let n = c32rtomb(buf.as_mut_ptr(), c, null_mut());
                assert_eq!(
                    &buf[..n],
                    char::from_u32(c)
                        .unwrap()
                        .encode_utf8(&mut [0; 4])
                        .as_bytes()
                );
                let mut st = MbstateT::new();
                let mut back = 0u32;
                let r = mbrtoc32(&raw mut back, buf.as_ptr(), n, &raw mut st);
                assert_eq!((r, back), (if c == 0 { 0 } else { n }, c));
                // UTF-16 units and back.
                let mut units = Vec::new();
                let mut st = MbstateT::new();
                let mut u = 0u16;
                let r = mbrtoc16(&raw mut u, buf.as_ptr(), n, &raw mut st);
                assert_eq!(r, if c == 0 { 0 } else { n });
                units.push(u);
                if c > 0xFFFF {
                    assert_eq!(
                        mbrtoc16(&raw mut u, buf.as_ptr(), 0, &raw mut st),
                        FROM_STATE
                    );
                    units.push(u);
                }
                let mut want = [0u16; 2];
                assert_eq!(units, char::from_u32(c).unwrap().encode_utf16(&mut want));
                let mut st = MbstateT::new();
                let mut out = Vec::new();
                for &unit in &units {
                    let mut b = [0u8; 4];
                    let r = c16rtomb(b.as_mut_ptr(), unit, &raw mut st);
                    out.extend_from_slice(&b[..if r == ILSEQ { 0 } else { r }]);
                }
                assert_eq!(out, &buf[..n]);
                // UTF-8 units, one at a time, both ways.
                let mut st = MbstateT::new();
                let mut out = Vec::new();
                for &unit in &buf[..n] {
                    let mut b = [0u8; 4];
                    let r = c8rtomb(b.as_mut_ptr(), unit, &raw mut st);
                    out.extend_from_slice(&b[..r]);
                }
                assert_eq!(out, &buf[..n]);
            }
        }
    }
}
