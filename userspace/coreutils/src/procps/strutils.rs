//! procps-ng 4.0.4's `local/strutils.c`: the two number readers its
//! programs check their arguments with -- `strtol_or_err` and
//! `strtod_nol_or_err` -- and the `errno` each refusal carries into the
//! diagnostic. `free` and `vmstat` share them, as upstream's do.
//!
//! One divergence, `free`'s number 3: an empty argument skips upstream's
//! conversion and is reported with whatever `errno` was left over, which is
//! not reproducible; here it is reported with none.

/// Why a number was refused, which decides the `: …` suffix on the diagnostic.
///
/// Upstream passes an `errno` to `error(3)`, which appends `strerror` of it.
/// The two values it can pass here are `ERANGE` and `EINVAL`; both are spelled
/// out as the literals glibc prints, because the program *chooses* them rather
/// than receiving them from the OS — there is no host error to translate, and
/// `coreutils::errmsg::strerror` maps `io::ErrorKind`s, of which neither has
/// one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumFault {
    /// `ERANGE` — `Numerical result out of range`.
    Range,
    /// No suffix at all: upstream reached `error(…, errno, …)` with `errno`
    /// still 0.
    NoConversion,
    /// `EINVAL` — `Invalid argument`.
    Invalid,
}

impl NumFault {
    /// The text `error(3)` appends, including its leading `: `.
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            NumFault::Range => ": Numerical result out of range",
            NumFault::NoConversion => "",
            NumFault::Invalid => ": Invalid argument",
        }
    }
}

/// `strtol_or_err` with base 10, returning the `long` upstream would.
///
/// Leading whitespace and a sign are accepted because that is `strtol`; a
/// trailing byte that is not a digit is not, because `strtol_or_err` insists on
/// `*end == '\0'`.
///
/// # Errors
///
/// The text is empty, is not wholly a number, or is out of range.
pub fn strtol(text: &[u8]) -> Result<i64, NumFault> {
    if text.is_empty() {
        // Upstream's `str != NULL && *str != '\0'` guard skips the conversion
        // entirely and reports with a stale `errno`; divergence 3.
        return Err(NumFault::NoConversion);
    }
    let mut i = 0usize;
    while matches!(text.get(i), Some(c) if c.is_ascii_whitespace()) {
        i = i.saturating_add(1);
    }
    let negative = match text.get(i) {
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
    let start = i;
    let mut value: i64 = 0;
    let mut overflow = false;
    while let Some(&c) = text.get(i) {
        if !c.is_ascii_digit() {
            break;
        }
        let digit = i64::from(c.saturating_sub(b'0'));
        // Accumulated with the sign already applied so that `-9223372036854775808`
        // is reachable, as it is for `strtol`.
        match value.checked_mul(10).and_then(|v| {
            v.checked_add(if negative {
                digit.saturating_neg()
            } else {
                digit
            })
        }) {
            Some(v) => value = v,
            // `strtol` keeps consuming digits after saturating, and so must
            // this, or `999999999999999999999x` would report the wrong fault.
            None => overflow = true,
        }
        i = i.saturating_add(1);
    }
    if i == start {
        // `str == end`: no digits at all.
        return Err(NumFault::NoConversion);
    }
    if i != text.len() {
        // `*end != '\0'`: trailing junk. Upstream reports with `errno` 0.
        return Err(NumFault::NoConversion);
    }
    if overflow {
        return Err(NumFault::Range);
    }
    Ok(value)
}

/// `strtod_nol_or_err` — procps' locale-independent decimal reader.
///
/// It is not `strtod`: there is no exponent, no hex form, no infinity and no
/// NaN, and the radix point may be `.` **or** `,` (the comment in `strutils.c`
/// notes that this is why the other cannot be a thousands separator). The
/// digits are accumulated by the same walk-forward-then-multiply-down loop
/// upstream uses, because its rounding is what ends up in the `float` the
/// caller compares against 1.
///
/// # Errors
///
/// The text is empty, or is not wholly a decimal number.
pub fn strtod_nol(text: &[u8]) -> Result<f64, NumFault> {
    if text.is_empty() {
        return Err(NumFault::NoConversion);
    }
    let mut cp = 0usize;
    while matches!(text.get(cp), Some(c) if c.is_ascii_whitespace()) {
        cp = cp.saturating_add(1);
    }
    let negative = match text.get(cp) {
        Some(b'-') => {
            cp = cp.saturating_add(1);
            true
        }
        Some(b'+') => {
            cp = cp.saturating_add(1);
            false
        }
        _ => false,
    };

    // Walk to the end of the integer part first so that `mult` starts at the
    // right power of ten and the digits can be consumed most-significant first.
    let mut num = 0.0f64;
    let mut mult = 0.1f64;
    let mut radix = cp;
    while matches!(text.get(radix), Some(c) if c.is_ascii_digit()) {
        radix = radix.saturating_add(1);
        mult *= 10.0;
    }
    while let Some(&c) = text.get(cp) {
        if !c.is_ascii_digit() {
            break;
        }
        num += f64::from(c.saturating_sub(b'0')) * mult;
        mult /= 10.0;
        cp = cp.saturating_add(1);
    }
    if cp == text.len() {
        return Ok(if negative { -num } else { num });
    }
    if !matches!(text.get(cp), Some(b'.' | b',')) {
        return Err(NumFault::Invalid);
    }
    cp = cp.saturating_add(1);
    mult = 0.1;
    while let Some(&c) = text.get(cp) {
        if !c.is_ascii_digit() {
            break;
        }
        num += f64::from(c.saturating_sub(b'0')) * mult;
        mult /= 10.0;
        cp = cp.saturating_add(1);
    }
    if cp == text.len() {
        return Ok(if negative { -num } else { num });
    }
    // Trailing junk after the fraction falls out of upstream's `if` block and
    // reaches `error(…, errno, …)` with `errno` still 0: no suffix.
    Err(NumFault::NoConversion)
}
