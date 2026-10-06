//! `<printf.h>`: glibc's window onto its printf engine.
//!
//! [`parse_printf_format`] says how many arguments a format takes and the
//! type of each, the way glibc's own engine types them when it reads a
//! format by position: `%d` a `PA_INT`, `%lu` a `PA_INT | PA_FLAG_LONG`,
//! `%Lf` a `PA_DOUBLE | PA_FLAG_LONG_DOUBLE`, `%s` a `PA_STRING`, each `*`
//! a `PA_INT`. It is glibc's to the letter (`printf_parse_oracle.txt`, every
//! conversion under every length modifier), quirks included: `%lc` and
//! `%ls` are a `PA_CHAR` and a `PA_STRING`, as glibc's parser calls them --
//! only `%C` and `%S` are the wide types -- and `%qd` is a plain `PA_INT`
//! where `%lld` is a `long` ([`Length::Quad`]).
//!
//! The specification is read by the engine's own parser
//! ([`crate::printf::parse_spec`]), so the two cannot disagree about what a
//! format says.

use crate::printf::{Count, INT_MAX, Length, Star};
use crate::wchar::WcharT;

/// `int`.
pub const PA_INT: i32 = 0;
/// `int`, cast to `char`.
pub const PA_CHAR: i32 = 1;
/// A wide character, `wint_t`.
pub const PA_WCHAR: i32 = 2;
/// `const char *`, a NUL-terminated string.
pub const PA_STRING: i32 = 3;
/// `const wchar_t *`, a NUL-terminated wide string.
pub const PA_WSTRING: i32 = 4;
/// `void *`.
pub const PA_POINTER: i32 = 5;
/// `float`.
pub const PA_FLOAT: i32 = 6;
/// `double`.
pub const PA_DOUBLE: i32 = 7;
/// The first value after the built-in types.
pub const PA_LAST: i32 = 8;
/// The flag bits of a type.
pub const PA_FLAG_MASK: i32 = 0xff00;
/// `long long`.
pub const PA_FLAG_LONG_LONG: i32 = 1 << 8;
/// `long double`: the same bit as `long long`.
pub const PA_FLAG_LONG_DOUBLE: i32 = PA_FLAG_LONG_LONG;
/// `long`.
pub const PA_FLAG_LONG: i32 = 1 << 9;
/// `short`.
pub const PA_FLAG_SHORT: i32 = 1 << 10;
/// A pointer to the type.
pub const PA_FLAG_PTR: i32 = 1 << 11;

/// glibc's `struct printf_info`: a conversion specification as glibc's
/// parser leaves it. 20 bytes, measured against glibc 2.39's: the
/// precision (-1 for none), the width, the conversion character, then
/// thirteen one-bit fields packed into [`PrintfInfo::bits`] from its lowest
/// bit, the bits of user-registered modifiers, and the padding character.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintfInfo {
    /// The precision; -1 for none.
    pub prec: i32,
    /// The width.
    pub width: i32,
    /// The conversion character.
    pub spec: WcharT,
    /// glibc's bitfields, the `IS_*` and flag constants below.
    pub bits: u16,
    /// Bits for user-registered modifiers.
    pub user: u16,
    /// The padding character: `'0'` for the `0` flag without `-`, else `' '`.
    pub pad: WcharT,
}

impl PrintfInfo {
    /// `L` (and `ll`, `q`, `w64`): `long double`, or `long long`.
    pub const IS_LONG_DOUBLE: u16 = 1 << 0;
    /// `h`.
    pub const IS_SHORT: u16 = 1 << 1;
    /// `l` (and `ll`, `j`, `z`, `Z`, `t`, `w64`).
    pub const IS_LONG: u16 = 1 << 2;
    /// `#`.
    pub const ALT: u16 = 1 << 3;
    /// A space.
    pub const SPACE: u16 = 1 << 4;
    /// `-`.
    pub const LEFT: u16 = 1 << 5;
    /// `+`.
    pub const SHOWSIGN: u16 = 1 << 6;
    /// `'`.
    pub const GROUP: u16 = 1 << 7;
    /// For a handler's own use.
    pub const EXTRA: u16 = 1 << 8;
    /// `hh`.
    pub const IS_CHAR: u16 = 1 << 9;
    /// The output is a wide stream.
    pub const WIDE: u16 = 1 << 10;
    /// `I`.
    pub const I18N: u16 = 1 << 11;
    /// The floating argument is a `binary128`.
    pub const IS_BINARY128: u16 = 1 << 12;
}

/// The bits glibc's parser sets in a [`PrintfInfo`] for a length modifier,
/// on x86-64 (`printf-parsemb.c`): `ll` is `is_long` and `is_long_double`,
/// `L` and `q` only `is_long_double`, and `j`, `z`, `Z`, `t` -- 64 bits
/// wide -- `is_long`. C23's `w8` and `wf8` are `is_char`, `w16` `is_short`,
/// `w32` nothing, and `w64` and the other fast widths, which are 64 bits
/// here, `is_long` and `is_long_double`, as `ll`.
pub(crate) const fn length_bits(length: Length) -> u16 {
    match length {
        Length::Char | Length::Bits(8) => PrintfInfo::IS_CHAR,
        Length::Short | Length::Bits(16) => PrintfInfo::IS_SHORT,
        Length::Long | Length::Word => PrintfInfo::IS_LONG,
        Length::LongLong | Length::Bits(64) => PrintfInfo::IS_LONG | PrintfInfo::IS_LONG_DOUBLE,
        Length::Quad | Length::LongDouble => PrintfInfo::IS_LONG_DOUBLE,
        Length::Int | Length::Bits(_) | Length::Invalid => 0,
    }
}

/// The type of the argument the built-in conversion `conv` takes, by the
/// length bits glibc's parser set ([`length_bits`]) -- glibc's own table,
/// `printf-parsemb.c` -- or `None` for one that takes none (`%m`, `%%`, a
/// conversion glibc does not know, a format that ends first).
pub(crate) const fn builtin_arg_type(conv: u8, bits: u16) -> Option<i32> {
    Some(match conv {
        b'i' | b'd' | b'u' | b'o' | b'X' | b'x' | b'B' | b'b' => {
            if bits & PrintfInfo::IS_LONG != 0 {
                PA_INT | PA_FLAG_LONG
            } else if bits & PrintfInfo::IS_SHORT != 0 {
                PA_INT | PA_FLAG_SHORT
            } else if bits & PrintfInfo::IS_CHAR != 0 {
                PA_CHAR
            } else {
                PA_INT
            }
        }
        b'e' | b'E' | b'f' | b'F' | b'g' | b'G' | b'a' | b'A' => {
            if bits & PrintfInfo::IS_LONG_DOUBLE != 0 {
                PA_DOUBLE | PA_FLAG_LONG_DOUBLE
            } else {
                PA_DOUBLE
            }
        }
        b'c' => PA_CHAR,
        b'C' => PA_WCHAR,
        b's' => PA_STRING,
        b'S' => PA_WSTRING,
        b'p' => PA_POINTER,
        b'n' => PA_INT | PA_FLAG_PTR,
        _ => return None,
    })
}

/// A `*`'s argument, as glibc's parser numbers it: `*m$`'s `m - 1`, or the
/// next unnumbered argument, `*posn`, taken. `m` past `INT_MAX` is no
/// position to glibc (its `read_int`'s -1), so the next argument is taken.
fn star_arg(count: Count, posn: &mut usize, max_ref: &mut usize) -> Option<usize> {
    match count {
        Count::Given(_) => None,
        Count::Star(Star::At(m)) if m <= INT_MAX => {
            *max_ref = (*max_ref).max(m);
            Some(m.saturating_sub(1))
        }
        Count::Star(_) => {
            let i = *posn;
            *posn = posn.saturating_add(1);
            Some(i)
        }
    }
}

/// `parse_printf_format(fmt, n, argtypes)`: how many arguments `fmt` takes,
/// and in `argtypes[0..n]` the type of each the format names -- a `PA_*`
/// value, with `PA_FLAG_*` bits -- as glibc's `parse_printf_format` says.
///
/// Each specification's arguments are typed in the order glibc types them:
/// a `*` width, then a `*` precision, each a `PA_INT`, then the argument
/// its conversion takes. An argument named twice keeps the type the later
/// specification gives it (`"%d %1$s"`: a `PA_STRING`), and one no
/// specification names is left as it was. Unnumbered specifications and
/// `*`s take arguments in order; the count is the larger of how many those
/// took and the highest position the format names. An entry past `n` is
/// counted and not written. A NULL `fmt` takes nothing (glibc's would
/// fault).
///
/// # Safety
///
/// `fmt` must be NULL or a NUL-terminated string, and `argtypes` writable
/// for `n` entries (or NULL with `n` 0).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub unsafe extern "C" fn parse_printf_format(
    fmt: *const u8,
    n: usize,
    argtypes: *mut i32,
) -> usize {
    if fmt.is_null() {
        return 0;
    }
    let put = |index: usize, ty: i32| {
        if index < n && !argtypes.is_null() {
            // SAFETY: the caller's contract: `argtypes` has `n` entries.
            unsafe { argtypes.add(index).write(ty) };
        }
    };
    // glibc's `nargs`: how many arguments the unnumbered specifications and
    // `*`s have taken.
    let mut posn = 0usize;
    let mut max_ref = 0usize;
    let mut fpos = 0usize;
    loop {
        // SAFETY: `fmt` is NUL-terminated; this walk stops at the NUL.
        let c = unsafe { *fmt.add(fpos) };
        if c == 0 {
            break;
        }
        fpos = fpos.wrapping_add(1);
        if c != b'%' {
            continue;
        }
        let raw = crate::printf::parse_spec(fmt, &mut fpos);
        let position = raw.position.filter(|&p| p <= INT_MAX);
        if let Some(p) = position {
            max_ref = max_ref.max(p);
        }
        let width_arg = star_arg(raw.width, &mut posn, &mut max_ref);
        let prec_arg = raw
            .precision
            .and_then(|count| star_arg(count, &mut posn, &mut max_ref));
        // SAFETY: `parse_spec` stopped at the conversion, or at the NUL.
        let conv = unsafe { *fmt.add(fpos) };
        let data = builtin_arg_type(conv, length_bits(raw.length));
        let data_arg = data.map(|_| match position {
            Some(p) => p.saturating_sub(1),
            None => {
                let i = posn;
                posn = posn.saturating_add(1);
                i
            }
        });
        if let Some(i) = width_arg {
            put(i, PA_INT);
        }
        if let Some(i) = prec_arg {
            put(i, PA_INT);
        }
        if let (Some(i), Some(ty)) = (data_arg, data) {
            put(i, ty);
        }
        if conv == 0 {
            break;
        }
        fpos = fpos.wrapping_add(1);
    }
    posn.max(max_ref)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// glibc's `struct printf_info`, as measured: 20 bytes, align 4, the
    /// bitfields at 12, `user` at 14, `pad` at 16.
    #[test]
    fn printf_info_is_glibcs() {
        assert_eq!(core::mem::size_of::<PrintfInfo>(), 20);
        assert_eq!(core::mem::align_of::<PrintfInfo>(), 4);
        assert_eq!(core::mem::offset_of!(PrintfInfo, spec), 8);
        assert_eq!(core::mem::offset_of!(PrintfInfo, bits), 12);
        assert_eq!(core::mem::offset_of!(PrintfInfo, user), 14);
        assert_eq!(core::mem::offset_of!(PrintfInfo, pad), 16);
    }

    /// Every line of `printf_parse_oracle.txt`
    /// (`posix/tools/oracle/printf_parse_harness.py`): glibc's
    /// `parse_printf_format` of each format, its return value and the 16
    /// entries of `argtypes` it was given, each -1 before the call.
    #[test]
    fn parse_printf_format_is_glibcs() {
        let oracle = include_str!("printf_parse_oracle.txt");
        let mut failures = std::vec::Vec::new();
        let mut compared = 0usize;
        for line in oracle.lines() {
            let (left, right) = line.split_once(" | ").expect("line");
            let (fmt_hex, n_text) = left.split_once(' ').expect("format and n");
            let mut fmt: std::vec::Vec<u8> = if fmt_hex == "-" {
                std::vec::Vec::new()
            } else {
                (0..fmt_hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&fmt_hex[i..i + 2], 16).expect("hex"))
                    .collect()
            };
            fmt.push(0);
            let n: usize = n_text.parse().expect("n");
            let (ret_text, types_text) = right.split_once(' ').expect("return and types");
            let want_ret: usize = ret_text.parse().expect("return");
            let want: std::vec::Vec<i32> = types_text
                .split(',')
                .map(|t| t.parse().expect("type"))
                .collect();
            let mut got = [-1i32; 16];
            // SAFETY: a NUL-terminated format, and 16 entries for `n <= 16`.
            let got_ret = unsafe { parse_printf_format(fmt.as_ptr(), n, got.as_mut_ptr()) };
            compared += 1;
            if got_ret != want_ret || got[..] != want[..] {
                failures.push(std::format!(
                    "{:?} n={n}: glibc {want_ret} {want:?}, here {got_ret} {got:?}",
                    std::string::String::from_utf8_lossy(&fmt[..fmt.len() - 1])
                ));
            }
        }
        assert_eq!(compared, 521, "the whole oracle");
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }

    /// A NULL format takes nothing, rather than faulting.
    #[test]
    fn a_null_format_takes_nothing() {
        let mut t = [-1i32; 4];
        // SAFETY: a NULL format is this function's to refuse.
        assert_eq!(
            unsafe { parse_printf_format(core::ptr::null(), 4, t.as_mut_ptr()) },
            0
        );
        assert_eq!(t, [-1; 4]);
    }
}
