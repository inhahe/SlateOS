// Every index is into the caller's buffer below the `maxsize` it says it
// holds, or into the format before its NUL; every sum is of lengths bounded by
// those. Clippy cannot see the bounds.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
//! `<monetary.h>`: `strfmon` and `strfmon_l`, glibc 2.39's in the C (POSIX)
//! locale -- the one locale this library has -- to the byte: its answers to
//! 1,572 calls are replayed by the tests (posix/tools/oracle/strfmon_harness.py,
//! posix/src/strfmon_oracle.txt).
//!
//! A conversion is `%`, flags, a width, `#` and a left precision, `.` and a
//! right precision, `L`, and `n` or `i`:
//!
//! - **Flags.** `=f` fills the left precision with `f` (a space when not
//!   given); `(` puts a negative amount in parentheses, `+` gives it the
//!   locale's sign, `-` (the default, and only one of the two may be given);
//!   `-` justifies the field to the left; `^` (no grouping) and `!` (no
//!   currency symbol) change nothing here, the C locale grouping nothing and
//!   having no symbol.
//! - **Width.** The whole field is padded to it with spaces, on the left
//!   unless `-` was given.
//! - **Left precision.** The amount's integer part is padded to that many
//!   digits with the fill -- and a positive amount is given a space where a
//!   negative one has its `-` or `(`, so that both line up.
//! - **Right precision.** The digits after the point: 2 when not given (the C
//!   locale's `frac_digits` is `CHAR_MAX`, glibc's "none").
//! - **`L`.** The argument is a `long double`, not a `double`.
//!
//! The digits are `printf`'s `%f`, rounded as it rounds. An amount is negative
//! when it is below zero, so `-0.0` is not, and prints as `printf` prints it:
//! `-0.00`, its padding before the `-` unless the fill is `0`; infinities and
//! NaNs are `inf` and `nan`, padded with spaces whatever the fill. `%i` is
//! `%n`: the C locale's international symbol is empty too.
//!
//! Output that does not fit `maxsize` bytes with its NUL fails, -1 with
//! `E2BIG`, leaving what fitted written; a malformed conversion fails, -1 with
//! `EINVAL`, leaving written what came before it -- nothing past a failure is
//! read, as glibc's stops there too.
//!
//! Until 2026-09-30 this took one `double` rather than a variable argument
//! list -- a second conversion printed the first value again -- honoured no
//! width, left precision, `-` or `L`, and cut output that did not fit short,
//! counting it success: glibc's own test of CVE-2026-19499, `%100n` in 100
//! bytes, got 4 back where glibc answers -1 with `E2BIG`.

use crate::errno;
use crate::printf::{self, FloatArg, VaList};

#[cfg(target_os = "none")]
use crate::printf::va_trampoline;

#[cfg(target_os = "none")]
va_trampoline!("strfmon", "__slateos_vstrfmon", "24", "rcx");
#[cfg(target_os = "none")]
va_trampoline!("strfmon_l", "__slateos_vstrfmon_l", "32", "r8");

/// The caller's buffer as glibc's `__printf_buffer` keeps it: bytes that
/// would pass `max` are not written, and the call has failed.
struct Out {
    s: *mut u8,
    max: usize,
    n: usize,
    failed: bool,
}

impl Out {
    /// `bytes`, as many as fit.
    fn put(&mut self, bytes: &[u8]) {
        let k = bytes.len().min(self.max - self.n);
        // SAFETY: `k` bytes fit below `max`, which the caller's buffer holds.
        unsafe { core::ptr::copy_nonoverlapping(bytes.as_ptr(), self.s.add(self.n), k) };
        self.n += k;
        if k < bytes.len() {
            self.failed = true;
        }
    }

    /// `count` copies of `c`, as many as fit.
    fn pad(&mut self, c: u8, count: usize) {
        let k = count.min(self.max - self.n);
        // SAFETY: as `put`.
        unsafe { core::ptr::write_bytes(self.s.add(self.n), c, k) };
        self.n += k;
        if k < count {
            self.failed = true;
        }
    }
}

/// One conversion's specification.
struct Spec {
    fill: u8,
    /// `(`: a negative amount in parentheses.
    parens: bool,
    /// `-`: the field justified to the left.
    left: bool,
    width: usize,
    left_prec: Option<usize>,
    right_prec: Option<usize>,
    long: bool,
}

/// The digits at `*p`, read on as far as they go: `None` when there are
/// none; the value saturated when it overflows.
///
/// # Safety
///
/// `*p` points into a NUL-terminated string.
unsafe fn digits(p: &mut *const u8) -> Option<usize> {
    // SAFETY (the reads): the caller's contract; no read passes the NUL.
    unsafe {
        if !(**p).is_ascii_digit() {
            return None;
        }
        let mut v = 0usize;
        while (**p).is_ascii_digit() {
            v = v.saturating_mul(10).saturating_add(usize::from(**p - b'0'));
            *p = p.add(1);
        }
        Some(v)
    }
}

/// The conversion after the `%` at `*p`, its end left at `*p`; `Err` with
/// the `errno` for one that is malformed (`EINVAL`) or whose width passes
/// `LONG_MAX` (`E2BIG`), as glibc's.
///
/// # Safety
///
/// `*p` points into a NUL-terminated string.
unsafe fn spec(p: &mut *const u8) -> Result<Spec, i32> {
    // SAFETY (the whole body): the caller's contract; every read stops at the
    // NUL, which matches no case.
    unsafe {
        let mut s = Spec {
            fill: b' ',
            parens: false,
            left: false,
            width: 0,
            left_prec: None,
            right_prec: None,
            long: false,
        };
        let mut signed = false;
        loop {
            match **p {
                b'=' => {
                    *p = p.add(1);
                    if **p == 0 {
                        return Err(errno::EINVAL);
                    }
                    s.fill = **p;
                }
                b'+' | b'(' => {
                    // One of the two, once.
                    if signed {
                        return Err(errno::EINVAL);
                    }
                    signed = true;
                    s.parens = **p == b'(';
                }
                b'-' => s.left = true,
                b'^' | b'!' => {}
                _ => break,
            }
            *p = p.add(1);
        }
        if (**p).is_ascii_digit() {
            let mut w: i64 = 0;
            while (**p).is_ascii_digit() {
                w = w
                    .checked_mul(10)
                    .and_then(|w| w.checked_add(i64::from(**p - b'0')))
                    .ok_or(errno::E2BIG)?;
                *p = p.add(1);
            }
            s.width = usize::try_from(w).unwrap_or(usize::MAX);
        }
        if **p == b'#' {
            *p = p.add(1);
            s.left_prec = Some(digits(p).ok_or(errno::EINVAL)?);
        }
        if **p == b'.' {
            *p = p.add(1);
            s.right_prec = Some(digits(p).ok_or(errno::EINVAL)?);
        }
        if **p == b'L' {
            s.long = true;
            *p = p.add(1);
        }
        let c = **p;
        if c != 0 {
            *p = p.add(1);
        }
        if c == b'n' || c == b'i' {
            Ok(s)
        } else {
            Err(errno::EINVAL)
        }
    }
}

/// The amount `arg` as `spec` asks, into `out`: glibc's sign and field in the
/// C locale -- its alignment space, its `-` or `(`, `printf`'s digits padded
/// to the left precision, the `)`, and the width's spaces. `Err(ENOMEM)` when
/// a `long double`'s digits cannot get the memory they need.
fn amount(out: &mut Out, spec: &Spec, arg: FloatArg) -> Result<(), i32> {
    let negative = arg.below_zero();
    let magnitude = if negative { arg.negated() } else { arg };
    let right_prec = spec.right_prec.unwrap_or(2);
    let start = out.n;
    // A positive amount's space for the sign it lacks, when the left
    // precision lines the two up.
    if spec.left_prec.is_some() && !negative {
        out.pad(b' ', 1);
    }
    if negative {
        out.put(if spec.parens { b"(" } else { b"-" });
    }
    // `printf("%*.*f")` of the magnitude, `*` the left precision (0 when not
    // given) and the point and its digits, padded with the fill -- which a
    // finite amount always fills already, and `inf` or `nan` may not.
    let fraction = if right_prec > 0 {
        right_prec.saturating_add(1)
    } else {
        0
    };
    let width = spec.left_prec.unwrap_or(0).saturating_add(fraction);
    printf::with_fixed_text(magnitude, right_prec, |t| {
        let len = t.len() + usize::from(t.negative);
        let pad = width.saturating_sub(len);
        if t.finite && spec.fill == b'0' {
            if t.negative {
                out.put(b"-");
            }
            out.pad(b'0', pad);
        } else {
            out.pad(if t.finite { spec.fill } else { b' ' }, pad);
            if t.negative {
                out.put(b"-");
            }
        }
        out.put(t.head);
        out.pad(b'0', t.zeros);
        out.put(t.tail);
    })
    .ok_or(errno::ENOMEM)?;
    if out.failed {
        return Ok(());
    }
    if negative && spec.parens {
        out.put(b")");
    }
    // The width: spaces after the field, then -- for one justified to the
    // right, and written whole -- moved before it.
    let written = out.n - start;
    if written < spec.width {
        let pad = spec.width - written;
        out.pad(b' ', pad);
        if out.failed || spec.left {
            return Ok(());
        }
        // SAFETY: `start..out.n` is what this call wrote, below `max`.
        let field = unsafe { core::slice::from_raw_parts_mut(out.s.add(start), out.n - start) };
        field.rotate_right(pad);
    }
    Ok(())
}

/// `strfmon` with its arguments in `ap`.
///
/// # Safety
///
/// `s` holds `maxsize` bytes; `format` is a NUL-terminated string; `ap`
/// holds the arguments its conversions read.
unsafe fn vstrfmon(s: *mut u8, maxsize: usize, format: *const u8, ap: *mut VaList) -> isize {
    if s.is_null() || format.is_null() || ap.is_null() {
        errno::set_errno(errno::EINVAL);
        return -1;
    }
    let mut out = Out {
        s,
        max: maxsize,
        n: 0,
        failed: false,
    };
    let mut p = format;
    // SAFETY (the whole loop): the caller's contract; `p` never passes the
    // format's NUL, and `ap` yields what the conversions read.
    unsafe {
        let ap = &mut *ap;
        while *p != 0 && !out.failed {
            if *p != b'%' {
                out.put(&[*p]);
                p = p.add(1);
                continue;
            }
            if *p.add(1) == b'%' {
                out.put(b"%");
                p = p.add(2);
                continue;
            }
            p = p.add(1);
            let spec = match spec(&mut p) {
                Ok(spec) => spec,
                Err(e) => {
                    errno::set_errno(e);
                    return -1;
                }
            };
            let arg = if spec.long {
                FloatArg::Long(printf::va_arg_long_double(ap))
            } else {
                FloatArg::Double(f64::from_bits(printf::va_arg_double(ap)))
            };
            if let Err(e) = amount(&mut out, &spec, arg) {
                errno::set_errno(e);
                return -1;
            }
        }
    }
    out.put(&[0]);
    if out.failed {
        errno::set_errno(errno::E2BIG);
        return -1;
    }
    isize::try_from(out.n - 1).unwrap_or(isize::MAX)
}

/// `strfmon`'s body, which the variadic `strfmon` hands its argument list:
/// the amounts `format` asks for, written into `s` (`maxsize` bytes, the NUL
/// included) -- see the module documentation. The bytes written less the
/// NUL, or -1 with `E2BIG` or `EINVAL`.
///
/// # Safety
///
/// `s` holds `maxsize` bytes; `format` is a NUL-terminated string; `ap` is a
/// `va_list` holding the arguments its conversions read.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __slateos_vstrfmon(
    s: *mut u8,
    maxsize: usize,
    format: *const u8,
    ap: *mut VaList,
) -> isize {
    // SAFETY: the caller's contract.
    unsafe { vstrfmon(s, maxsize, format, ap) }
}

/// `strfmon_l`'s body: [`__slateos_vstrfmon`], any locale being the C
/// locale's monetary conventions -- the only ones this library has.
///
/// # Safety
///
/// As [`__slateos_vstrfmon`].
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn __slateos_vstrfmon_l(
    s: *mut u8,
    maxsize: usize,
    _locale: *mut u8,
    format: *const u8,
    ap: *mut VaList,
) -> isize {
    // SAFETY: the caller's contract.
    unsafe { vstrfmon(s, maxsize, format, ap) }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::x87::LongDouble;
    use std::format;
    use std::string::String;
    use std::vec::Vec;

    /// glibc 2.39's answers in the C locale, one line a call
    /// (`strfmon_harness.py`, which says the forms).
    const ORACLE: &str = include_str!("strfmon_oracle.txt");

    /// The stack's argument area, as aligned as a `long double` needs.
    #[repr(C, align(16))]
    struct Area([u8; 512]);

    /// `f` with a `va_list` holding `doubles` in the SSE save area (and past
    /// its eight, on the stack) and `longs` on the stack, 16 bytes each.
    fn with_valist<R>(
        doubles: &[f64],
        longs: &[LongDouble],
        f: impl FnOnce(*mut VaList) -> R,
    ) -> R {
        let mut reg = [0u8; 176];
        let mut stack = Area([0; 512]);
        let mut at = 0usize;
        for (i, v) in doubles.iter().enumerate() {
            if i < 8 {
                reg[48 + 16 * i..56 + 16 * i].copy_from_slice(&v.to_bits().to_le_bytes());
            } else {
                stack.0[at..at + 8].copy_from_slice(&v.to_bits().to_le_bytes());
                at += 8;
            }
        }
        for l in longs {
            at = (at + 15) & !15;
            stack.0[at..at + 8].copy_from_slice(&l.significand.to_le_bytes());
            stack.0[at + 8..at + 10].copy_from_slice(&l.sign_exp.to_le_bytes());
            at += 16;
        }
        let mut va = VaList {
            gp_offset: 48,
            fp_offset: 48,
            overflow_arg_area: stack.0.as_mut_ptr(),
            reg_save_area: reg.as_mut_ptr(),
        };
        f(&raw mut va)
    }

    /// The harness's value, as its C source writes it: a `double` literal,
    /// an integer, or one of `<math.h>`'s names.
    fn double(v: &str) -> f64 {
        match v {
            "INFINITY" => f64::INFINITY,
            "-INFINITY" => f64::NEG_INFINITY,
            "NAN" => f64::NAN,
            "-NAN" => -f64::NAN,
            _ => v.parse().unwrap(),
        }
    }

    /// `(long double) v`: exact for an integer literal, as C converts one,
    /// else the `double` literal widened.
    fn long_double(v: &str) -> LongDouble {
        if let Ok(n) = v.parse::<i64>() {
            if n == 0 {
                return LongDouble::from_bits(0, 0);
            }
            let m = n.unsigned_abs();
            let top = m.ilog2();
            let exp = u16::try_from(16383 + top).unwrap() | if n < 0 { 0x8000 } else { 0 };
            return LongDouble::from_bits(exp, m << (63 - top));
        }
        crate::x87::from_f64(double(v))
    }

    fn errno_name(e: i32) -> &'static str {
        match e {
            0 => "0",
            errno::E2BIG => "E2BIG",
            errno::EINVAL => "EINVAL",
            _ => "other",
        }
    }

    /// A text as the harness writes it: `\xHH` for a byte outside `!`..`~`
    /// and for `\`, `\x` for none.
    fn token(b: &[u8]) -> String {
        use core::fmt::Write;
        if b.is_empty() {
            return "\\x".into();
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

    fn untoken(t: &str) -> Vec<u8> {
        if t == "\\x" {
            return Vec::new();
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

    /// What the harness prints after ` = ` for `call`.
    fn ours(call: &str) -> String {
        let f: Vec<&str> = call.split(' ').collect();
        let mut fmt = untoken(f[0]);
        fmt.push(0);
        let size: usize = f[1].parse().unwrap();
        let values = &f[3..];
        let (doubles, longs): (Vec<f64>, Vec<LongDouble>) = if f[2] == "L" {
            (Vec::new(), values.iter().map(|v| long_double(v)).collect())
        } else {
            (values.iter().map(|v| double(v)).collect(), Vec::new())
        };
        let mut buf = [0u8; 512];
        errno::set_errno(0);
        // SAFETY: `buf` holds more than `size` bytes; the format is
        // NUL-terminated; the list holds what it reads.
        let r = with_valist(&doubles, &longs, |ap| unsafe {
            __slateos_vstrfmon(buf.as_mut_ptr(), size, fmt.as_ptr(), ap)
        });
        let e = errno::get_errno();
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        format!("{r} {} {}", errno_name(e), token(&buf[..n]))
    }

    #[test]
    fn every_call_is_glibcs() {
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
        assert!(n > 1500, "the oracle has {n} lines");
        assert!(
            wrong.is_empty(),
            "{} of {n}:\n{}",
            wrong.len(),
            wrong
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    /// glibc's own test of CVE-2026-19499 (stdlib/tst-strfmon-bug34510.c): a
    /// 100-wide field in 100 bytes leaves no room for the NUL.
    #[test]
    fn a_field_as_wide_as_the_buffer_is_e2big() {
        let mut buf = [0u8; 100];
        errno::set_errno(0);
        // SAFETY: as above.
        let r = with_valist(&[1.23], &[], |ap| unsafe {
            __slateos_vstrfmon(buf.as_mut_ptr(), buf.len(), c"%100n".as_ptr().cast(), ap)
        });
        assert_eq!(r, -1);
        assert_eq!(errno::get_errno(), errno::E2BIG);
    }

    #[test]
    fn a_null_buffer_or_format_is_einval() {
        let mut buf = [0u8; 8];
        errno::set_errno(0);
        // SAFETY: NULL is refused before anything is read.
        let r = with_valist(&[1.0], &[], |ap| unsafe {
            __slateos_vstrfmon(core::ptr::null_mut(), 8, c"%n".as_ptr().cast(), ap)
        });
        assert_eq!((r, errno::get_errno()), (-1, errno::EINVAL));
        // SAFETY: as above.
        let r = with_valist(&[1.0], &[], |ap| unsafe {
            __slateos_vstrfmon(buf.as_mut_ptr(), 8, core::ptr::null(), ap)
        });
        assert_eq!(r, -1);
        // SAFETY: as above.
        let r = with_valist(&[1.0], &[], |ap| unsafe {
            __slateos_vstrfmon_l(
                buf.as_mut_ptr(),
                8,
                core::ptr::null_mut(),
                c"%n".as_ptr().cast(),
                ap,
            )
        });
        assert_eq!((r, &buf[..5]), (4, &b"1.00\0"[..]));
    }

    /// A width past `LONG_MAX` is `E2BIG`, as glibc's parse makes it; one
    /// that merely does not fit stops at the buffer's end, without writing
    /// or looping over the rest.
    #[test]
    fn huge_widths_fail_without_writing_them() {
        let mut buf = [0u8; 16];
        errno::set_errno(0);
        // SAFETY: as above.
        let r = with_valist(&[1.0], &[], |ap| unsafe {
            __slateos_vstrfmon(
                buf.as_mut_ptr(),
                16,
                c"%99999999999999999999n".as_ptr().cast(),
                ap,
            )
        });
        assert_eq!((r, errno::get_errno()), (-1, errno::E2BIG));
        errno::set_errno(0);
        // SAFETY: as above.
        let r = with_valist(&[1.0], &[], |ap| unsafe {
            __slateos_vstrfmon(buf.as_mut_ptr(), 16, c"%#999999999999n".as_ptr().cast(), ap)
        });
        assert_eq!((r, errno::get_errno()), (-1, errno::E2BIG));
        assert!(buf.iter().all(|&c| c == b' '), "{buf:?}");
    }
}
