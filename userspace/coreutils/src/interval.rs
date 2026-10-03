//! A time interval as GNU's `sleep` and `timeout` read one: a `strtod` number
//! with at most one suffix -- `s` for seconds (the default), `m` minutes, `h`
//! hours, `d` days.
//!
//! The two utilities carry the same code upstream, copied rather than shared
//! (`sleep.c`'s `apply_suffix` and `timeout.c`'s `apply_time_suffix`, each
//! behind the same four-way test), so one reading serves both here and they
//! cannot drift apart:
//!
//! ```text
//! if (! (xstrtod (str, &ep, &d, cl_strtod) || errno == ERANGE)
//!     || ! (0 <= d)                  /* Nonnegative interval.  */
//!     || (*ep && *(ep + 1))          /* At most one character after it,  */
//!     || ! apply_suffix (&d, *ep))   /* ... and that a suffix.  */
//! ```
//!
//! # The number is read the way `strtod` reads one
//!
//! `cl_strtod` is the C-locale `strtod`, and [`crate::extfloat::strtod`] is
//! that function, measured against glibc's by `scripts/extfloat-diff.sh`. So
//! leading white space, a sign, a hexadecimal numeral and `inf` are all
//! accepted, as they are upstream: `sleep ' 1'`, `timeout 0x10 cmd` and
//! `timeout inf cmd` are measured to work.
//!
//! # A range error is not a refusal
//!
//! Upstream writes `xstrtod (…) || errno == ERANGE`, so a number too large for
//! a `double` is infinity and one too small is zero, and both are accepted:
//! `sleep 1e400` pauses forever, and `timeout 1e-400 cmd` sets no time limit
//! at all, a duration of 0 meaning none.

use crate::extfloat;

/// What one trailing suffix letter multiplies by: upstream's `apply_suffix`.
///
/// `None` is upstream's `multiplier == 0`, which is the *only* way a suffix is
/// refused -- so an unknown letter and a second letter after a good one are
/// both "invalid time interval", and neither message says anything about
/// suffixes.
fn multiplier(c: u8) -> Option<f64> {
    match c {
        b's' => Some(1.0),
        b'm' => Some(60.0),
        b'h' => Some(60.0 * 60.0),
        b'd' => Some(60.0 * 60.0 * 24.0),
        _ => None,
    }
}

/// `text` as a number of seconds: non-negative, never NaN, possibly infinite.
///
/// `None` is upstream's four-way test, collapsed: no number at all, a negative
/// or NaN one, more than one character after it, or a character after it that
/// is not a suffix. Each is the same "invalid time interval" upstream, so
/// there is nothing to tell them apart for.
#[must_use]
pub fn seconds(text: &[u8]) -> Option<f64> {
    let scanned = extfloat::strtod(text);
    if scanned.consumed == 0 {
        return None;
    }
    let value = scanned.value;
    // `0 <= d` upstream, which admits `-0.0` and refuses NaN. Two tests rather
    // than `!(value >= 0.0)`, because the negation of a partial order is the
    // shape that reads as a typo.
    if value.is_nan() || value < 0.0 {
        return None;
    }
    let factor = match text.get(scanned.consumed..) {
        None | Some([]) => 1.0,
        Some(&[c]) => multiplier(c)?,
        // Upstream's `*ep && *(ep+1)`: two or more characters left over is a
        // refusal before any suffix is looked at, so `1s2` never reaches
        // `apply_suffix`.
        Some(_) => return None,
    };
    Some(value * factor)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::seconds;

    fn secs(text: &str) -> Option<f64> {
        seconds(text.as_bytes())
    }

    #[test]
    fn a_number_is_seconds_and_a_suffix_multiplies() {
        assert_eq!(secs("5"), Some(5.0));
        assert_eq!(secs("0.25"), Some(0.25));
        assert_eq!(secs("2s"), Some(2.0));
        assert_eq!(secs("2m"), Some(120.0));
        assert_eq!(secs("2h"), Some(7200.0));
        assert_eq!(secs("1d"), Some(86_400.0));
    }

    /// `strtod`'s grammar, not `f64::from_str`'s.
    #[test]
    fn the_grammar_is_strtods() {
        assert_eq!(secs(" 1"), Some(1.0));
        assert_eq!(secs("0x10"), Some(16.0));
        assert_eq!(secs("0x1p-3"), Some(0.125));
        assert_eq!(secs("1e1"), Some(10.0));
        assert_eq!(secs("+3"), Some(3.0));
        assert!(secs("inf").unwrap().is_infinite());
        assert!(secs("INFINITY").unwrap().is_infinite());
    }

    /// Both ends of the `double` range are answers, not refusals.
    #[test]
    fn a_range_error_is_accepted_at_both_ends() {
        assert!(secs("1e400").unwrap().is_infinite());
        assert_eq!(secs("1e-400"), Some(0.0));
    }

    /// `-0` is not negative by upstream's `0 <= d`; `-1` and NaN are refused.
    #[test]
    fn a_negative_interval_is_refused_but_negative_zero_is_not() {
        assert_eq!(secs("-1"), None);
        assert_eq!(secs("nan"), None);
        assert_eq!(secs("-0"), Some(0.0));
    }

    /// Each measured with GNU 9.4 `timeout`, which says "invalid time
    /// interval" for every one.
    #[test]
    fn what_is_not_an_interval() {
        for text in ["", " ", "x", "1x", "1ss", "1s2", "1 ", "1e", "1e1x", "-"] {
            assert_eq!(secs(text), None, "{text:?}");
        }
    }

    /// Bytes past the number that are not UTF-8 are refused rather than
    /// misread.
    #[test]
    fn a_byte_that_is_not_utf8_is_not_a_suffix() {
        assert_eq!(seconds(b"1\xff"), None);
        assert_eq!(seconds(b"\xff"), None);
    }
}
