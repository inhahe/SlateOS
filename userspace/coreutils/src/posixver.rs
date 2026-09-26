//! gnulib's `posix2_version`: which edition of POSIX the user asked to conform
//! to.
//!
//! POSIX 1003.1-2001 withdrew a family of obsolete spellings -- `sort +POS`,
//! `uniq +N`, `tail +N` -- and POSIX 2008 let them back in, so GNU honours
//! whichever edition `_POSIX2_VERSION` names. The variable is read the way
//! gnulib reads it, which is looser than it looks and was measured rather than
//! recalled: `strtol` in base 10, leading white space skipped and a sign
//! allowed, but **any** trailing byte makes the whole variable fall back to the
//! default. So `_POSIX2_VERSION=' 200112'` is 200112 and `'200112x'` is the
//! default, 200809.
//!
//! One copy, because the utilities that ask are several and the parse is easy
//! to get subtly wrong -- `u8::is_ascii_whitespace`, the obvious tool for the
//! leading white space, does not include the vertical tab that C's `isspace`
//! does.

use crate::quote::os_bytes;
use std::ffi::OsStr;

/// What an unset, empty or unparsable `_POSIX2_VERSION` means: glibc's own
/// `_POSIX2_VERSION`, POSIX.1-2008.
pub const DEFAULT: i32 = 200_809;

/// `posix2_version()` for a given value of the variable, `None` being unset.
///
/// A value outside `int` is clamped into it, as upstream clamps the `long`
/// that `strtol` returns.
#[must_use]
pub fn posix2_version_from(value: Option<&OsStr>) -> i32 {
    value
        .map(os_bytes)
        .filter(|bytes| !bytes.is_empty())
        .and_then(|bytes| strtol(&bytes))
        .map_or(DEFAULT, |v| {
            i32::try_from(v).unwrap_or(if v < 0 { i32::MIN } else { i32::MAX })
        })
}

/// `posix2_version()`: the process environment's answer.
#[must_use]
pub fn posix2_version() -> i32 {
    posix2_version_from(std::env::var_os("_POSIX2_VERSION").as_deref())
}

/// Whether `version` is POSIX 1003.1-2001's edition, the half-open window
/// \[200112, 200809) in which the obsolete `+N` spellings were withdrawn.
///
/// Upstream spells the same window three ways -- `uniq`'s `strict_posix2`,
/// `sort`'s `! traditional_usage`, `tail`'s `! (obsolete_usage ||
/// traditional_usage)` -- and all three are this.
#[must_use]
pub fn withdraws_obsolete_forms(version: i32) -> bool {
    (200_112..DEFAULT).contains(&version)
}

/// C's `isspace` in the C locale: `u8::is_ascii_whitespace` plus the vertical
/// tab, which Rust's definition leaves out.
fn c_isspace(b: u8) -> bool {
    b.is_ascii_whitespace() || b == 0x0b
}

/// `strtol` in base 10 over a whole byte string, or `None` if anything is
/// left over or there are no digits. Saturates rather than wrapping, matching
/// `strtol`'s `ERANGE` behaviour of returning `LONG_MAX`/`LONG_MIN` with the
/// tail consumed.
fn strtol(bytes: &[u8]) -> Option<i64> {
    let mut i = 0usize;
    while bytes.get(i).copied().is_some_and(c_isspace) {
        i = i.saturating_add(1);
    }
    let negative = match bytes.get(i) {
        Some(b'-') => {
            i = i.saturating_add(1);
            true
        }
        Some(b'+') => {
            i = i.saturating_add(1);
            false
        }
        _ => false,
    };
    let digits = bytes.get(i..)?;
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let mut value: i64 = 0;
    for &d in digits {
        value = value
            .saturating_mul(10)
            .saturating_add(i64::from(d.wrapping_sub(b'0')));
    }
    Some(if negative {
        value.saturating_neg()
    } else {
        value
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::{DEFAULT, posix2_version_from, withdraws_obsolete_forms};
    use std::ffi::OsStr;

    fn version(v: &str) -> i32 {
        posix2_version_from(Some(OsStr::new(v)))
    }

    #[test]
    fn unset_empty_and_unparsable_all_mean_the_default() {
        assert_eq!(posix2_version_from(None), DEFAULT);
        assert_eq!(version(""), DEFAULT);
        assert_eq!(version("200112x"), DEFAULT);
        assert_eq!(version("x"), DEFAULT);
        assert_eq!(version("   "), DEFAULT);
        assert_eq!(version("+"), DEFAULT);
    }

    #[test]
    fn strtol_skips_leading_space_and_takes_a_sign() {
        assert_eq!(version(" 200112"), 200_112);
        // The vertical tab is C white space, and not Rust's.
        assert_eq!(version("\u{b}200112"), 200_112);
        assert_eq!(version("+200112"), 200_112);
        assert_eq!(version("-5"), -5);
        // A trailing space is a trailing byte.
        assert_eq!(version("200112 "), DEFAULT);
    }

    #[test]
    fn out_of_range_values_clamp_into_int() {
        assert_eq!(version("99999999999"), i32::MAX);
        assert_eq!(version("-99999999999"), i32::MIN);
    }

    #[test]
    fn the_2001_window_is_half_open() {
        assert!(!withdraws_obsolete_forms(200_111));
        assert!(withdraws_obsolete_forms(200_112));
        assert!(withdraws_obsolete_forms(200_808));
        assert!(!withdraws_obsolete_forms(200_809));
        assert!(!withdraws_obsolete_forms(DEFAULT));
    }
}
