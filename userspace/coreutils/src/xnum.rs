//! The number on the command line: gnulib's `xstrtoumax` and `xdectoint`.
//!
//! ## Why this is shared
//!
//! Nearly every utility that takes a size or a count parses it through one
//! gnulib pair — `xstrtol.c` for the grammar, `xdectoint.c` for the range check
//! and the diagnostic — and the grammar is much larger than "a decimal
//! integer". It accepts leading whitespace and a leading `+`; it accepts a
//! multiplier suffix (`K`, `MiB`, `w`, `b`, …) when its caller passes a suffix
//! list, and rejects a trailing character otherwise; it reads a bare suffix
//! with no digits at all as the number **one**, so `head -c K` is a kibibyte;
//! and when a number is out of range it prints one of *two* different
//! `strerror` sentences, chosen by a heuristic that has nothing to do with
//! which limit was passed.
//!
//! None of that is guessable, and all of it is observable. `nl` and `head` each
//! grew their own partial copy before this module existed — `nl`'s is decimal
//! with no suffixes, `head`'s knows suffixes but not the two-sentence split —
//! and `fold` would have been the third. One copy, measured once, is the point.
//!
//! ## The two sentences
//!
//! A number outside the caller's `[min, max]` is reported with a `strerror`
//! tail, and the tail is **not** chosen by which end was violated:
//!
//! ```text
//! if (tnum < min || max < tnum)
//!   errno = tnum > INT_MAX / 2 ? EOVERFLOW : ERANGE;   /* signed: also < INT_MIN / 2 */
//! ```
//!
//! It is a heuristic for "did this look like a type overflow?", so it reads the
//! *value*, not the bound. `fold -w 0` is below the floor of 1 and says
//! `Numerical result out of range`; `fold -w 18446744073709551607` is above the
//! ceiling and says `Value too large for defined data type`; and a caller whose
//! floor was above `INT_MAX / 2` would get the second sentence for a value
//! below it, which reads wrong and is nevertheless what GNU prints.
//!
//! A number that does not parse at all gets **no** tail, and neither does one
//! whose suffix is invalid — even if the digits also overflowed, which gnulib
//! spells out (`LONGINT_INVALID_SUFFIX_CHAR_WITH_OVERFLOW` sets `errno = 0`,
//! "don't show ERANGE errors for invalid numbers"). Measured:
//! `fold -w 99999999999999999999999x` prints no tail, while
//! `fold -w 9999999999999999999999999999` prints the overflow one.

use crate::quote::{quote, quote_glibc};

/// gnulib's `strtol_error`.
///
/// Upstream is a bit set — `LONGINT_INVALID_SUFFIX_CHAR_WITH_OVERFLOW` is
/// literally `OVERFLOW | INVALID_SUFFIX_CHAR` — and it is spelled out as an
/// enum here because only these five combinations occur: `LONGINT_INVALID` is
/// returned alone, never or-ed with anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// A number, in range for the type.
    Ok,
    /// The digits (or the suffix multiplication) exceeded the type; the value
    /// returned alongside is saturated, as C's `strtoumax` leaves it.
    Overflow,
    /// A trailing character that is not in the caller's suffix list.
    InvalidSuffix,
    /// Both of the above. Distinguished because it suppresses the diagnostic's
    /// `strerror` tail where a plain [`Status::Overflow`] does not.
    InvalidSuffixWithOverflow,
    /// Not a number: no digits, or a leading `-` where unsigned was asked for.
    Invalid,
}

impl Status {
    fn of(overflow: bool, invalid_suffix: bool) -> Self {
        match (overflow, invalid_suffix) {
            (false, false) => Self::Ok,
            (true, false) => Self::Overflow,
            (false, true) => Self::InvalidSuffix,
            (true, true) => Self::InvalidSuffixWithOverflow,
        }
    }
}

/// `INT_MAX / 2`, the threshold in gnulib's "did this look like a type
/// overflow?" heuristic. `INT_MAX` is 32-bit there and here alike — it is C's
/// `int`, not a pointer-sized type, so this does not vary by host.
const INT_MAX_HALF: u64 = 2_147_483_647 / 2;
/// [`INT_MAX_HALF`] again, typed for the signed side.
const INT_MAX_HALF_SIGNED: i64 = 2_147_483_647 / 2;
/// `INT_MIN / 2`, the same heuristic on the negative side. C division truncates
/// toward zero, so this is -1073741824, not -1073741824.5 rounded down.
const INT_MIN_HALF: i64 = -2_147_483_648 / 2;

/// The bytes C's `isspace` accepts in the `C` locale, which is what `strtoumax`
/// skips before the sign.
const SPACE: &[u8] = b" \t\n\x0b\x0c\r";

/// Which `strerror` tail a diagnostic carries, if any.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tail {
    /// `errno` was left at zero: the message ends after the quoted argument.
    None,
    /// `ERANGE`.
    Range,
    /// `EOVERFLOW`.
    Overflow,
}

impl Tail {
    fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Range => ": Numerical result out of range",
            Self::Overflow => ": Value too large for defined data type",
        }
    }
}

/// What C's `strtoumax` did to a decimal string.
struct Scan {
    /// The accumulated value, saturated at [`u64::MAX`] on overflow exactly as
    /// C's does.
    value: u64,
    /// Where `endptr` was left. Zero when no conversion happened, because C
    /// sets `endptr` to the *start* of the string in that case — not to the
    /// point the scan gave up, which matters: the suffix test that follows then
    /// looks at the first byte of the argument, whitespace included.
    end: usize,
    /// `errno == ERANGE`.
    overflow: bool,
    /// Whether any digits were consumed.
    converted: bool,
}

fn skip_space(text: &[u8]) -> usize {
    let mut at = 0usize;
    while text.get(at).is_some_and(|c| SPACE.contains(c)) {
        at = at.saturating_add(1);
    }
    at
}

/// The value of one digit in `base`, or `None` if it is not one.
///
/// Letters count from 10 in either case, as C's `strtoumax` reads them, and are
/// rejected by the `< base` test rather than by the character class — so `f` is
/// a digit in base 16 and a terminator in base 10.
fn digit_value(c: u8, base: u32) -> Option<u32> {
    let value = match c {
        b'0'..=b'9' => u32::from(c - b'0'),
        b'a'..=b'z' => u32::from(c - b'a') + 10,
        b'A'..=b'Z' => u32::from(c - b'A') + 10,
        _ => return None,
    };
    (value < base).then_some(value)
}

/// C's `strtoumax`, minus the sign handling its callers here never reach.
///
/// A negative sign is rejected by [`xstrtoumax`] *before* this runs — gnulib
/// does the same, because C would otherwise negate `-1` into `UINTMAX_MAX`
/// silently. So only `+` is accepted here.
///
/// # The base prefix is C's, and it is looser than it looks
///
/// Base 0 means "read the prefix": `0x`/`0X` is hexadecimal, a leading `0` is
/// octal, anything else decimal. Base 16 *also* accepts the `0x` prefix, and
/// no other base accepts any prefix. This matters to `od`, whose old-style
/// offset operand is octal by default and hexadecimal after `0x`, so that
/// `od f +010` skips eight bytes where `od -j 010 f` — same digits, same
/// base-0 grammar — also skips eight, and `od f +10.` skips none at all
/// because the `.` upstream's own `--help` documents is not in the suffix list.
///
/// The subtle part is a `0x` with nothing usable after it. C consumes the `0`
/// alone and leaves the pointer on the `x`, so `0xz` is the number zero with a
/// trailing `xz`, not a failed parse. A caller passing a suffix list therefore
/// sees an invalid suffix, which is a different diagnostic from an invalid
/// number, and that difference is observable.
fn strtoumax_base(text: &[u8], base: u32) -> Scan {
    let mut at = skip_space(text);
    if matches!(text.get(at), Some(b'+')) {
        at = at.saturating_add(1);
    }
    // The prefix, if this base takes one. `end_of_zero` remembers where a bare
    // `0` ended, for the `0x`-with-no-digits case below.
    let mut base = base;
    let mut end_of_zero = None;
    if (base == 0 || base == 16)
        && text.get(at) == Some(&b'0')
        && matches!(text.get(at.saturating_add(1)), Some(b'x' | b'X'))
    {
        end_of_zero = Some(at.saturating_add(1));
        at = at.saturating_add(2);
        base = 16;
    } else if base == 0 {
        base = if text.get(at) == Some(&b'0') { 8 } else { 10 };
    }

    let first_digit = at;
    let mut value = 0u64;
    let mut overflow = false;
    while let Some(digit) = text.get(at).and_then(|&c| digit_value(c, base)) {
        // C keeps consuming digits after ERANGE — it does not stop at the
        // first one that overflows — so the end pointer lands past the whole
        // run and a trailing character after a huge number is still seen.
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(digit)))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
        at = at.saturating_add(1);
    }
    if at == first_digit {
        // `0x` with no hexadecimal digit after it: the `0` converted, the `x`
        // did not.
        if let Some(end) = end_of_zero {
            return Scan {
                value: 0,
                end,
                overflow: false,
                converted: true,
            };
        }
        return Scan {
            value: 0,
            end: 0,
            overflow: false,
            converted: false,
        };
    }
    if overflow {
        value = u64::MAX;
    }
    Scan {
        value,
        end: at,
        overflow,
        converted: true,
    }
}

/// `value * factor`, saturating and recording the overflow the way gnulib's
/// `bkm_scale` does.
fn scale(value: u64, factor: u64, overflow: &mut bool) -> u64 {
    match value.checked_mul(factor) {
        Some(v) => v,
        None => {
            *overflow = true;
            u64::MAX
        }
    }
}

/// `value * base.pow(power)`, applied one factor at a time so that the overflow
/// flag is set by the same multiplication gnulib's loop would set it by.
fn scale_by_power(value: u64, base: u64, power: u32, overflow: &mut bool) -> u64 {
    let mut value = value;
    for _ in 0..power {
        value = scale(value, base, overflow);
    }
    value
}

/// The suffix letters that may be followed by a second suffix (`B`, `D` or
/// `iB`) when the caller's list contains `0`. `b`, `c` and `w` are deliberately
/// absent: upstream's first `switch` does not list them.
const SCALABLE: &[u8] = b"EGgkKMmPQRTtYZ";

/// gnulib's `xstrtoumax`: a decimal number with an optional multiplier suffix.
///
/// `valid_suffixes` is three-valued, and all three are used by real callers:
/// `None` means "accept any trailing text and ignore it" (gnulib's `NULL`),
/// `Some(b"")` means "accept no suffix at all" (what `fold -w` passes), and
/// `Some(b"bkKmMGTPEZYRQ0")` or similar names the letters allowed — where a `0`
/// in the list is not a letter but a flag, permitting the `B`/`iB` second
/// suffix that switches the base between 1000 and 1024.
///
/// The returned value is meaningful for every status except
/// [`Status::Invalid`], where it is zero.
#[must_use]
pub fn xstrtoumax(text: &[u8], valid_suffixes: Option<&[u8]>) -> (u64, Status) {
    xstrtoumax_base(text, 10, valid_suffixes)
}

/// [`xstrtoumax`] in a base other than ten — gnulib's third argument, which
/// most callers pass as `10` and `od` passes as `0`, `8` or `16`.
///
/// A base of zero reads C's prefix: see [`strtoumax_base`].
#[must_use]
pub fn xstrtoumax_base(text: &[u8], base: u32, valid_suffixes: Option<&[u8]>) -> (u64, Status) {
    // gnulib refuses a leading `-` itself rather than letting C's strtoumax
    // wrap it around into a huge positive. This is why `fold -w -3` says
    // "invalid number of columns" and not "out of range".
    if matches!(text.get(skip_space(text)), Some(b'-')) {
        return (0, Status::Invalid);
    }

    let scan = strtoumax_base(text, base);
    let mut value = scan.value;
    let mut overflow = scan.overflow;
    let mut at = scan.end;

    if !scan.converted {
        // "If there is no number but there is a valid suffix, assume the
        // number is 1." So `head -c K` reads a kibibyte, and `head -c x` does
        // not read anything.
        let bare_suffix = valid_suffixes
            .is_some_and(|sfx| text.first().is_some_and(|c| *c != 0 && sfx.contains(c)));
        if !bare_suffix {
            return (0, Status::Invalid);
        }
        value = 1;
    }

    let Some(suffixes) = valid_suffixes else {
        // A null suffix list means "allow any suffix", and upstream returns
        // before even looking at the trailing bytes.
        return (value, Status::of(overflow, false));
    };

    let Some(&c) = text.get(at).filter(|c| **c != 0) else {
        return (value, Status::of(overflow, false));
    };
    if !suffixes.contains(&c) {
        return (value, Status::of(overflow, true));
    }

    let mut base = 1024u64;
    let mut consumed = 1usize;
    if SCALABLE.contains(&c) && suffixes.contains(&b'0') {
        match text.get(at.saturating_add(1)) {
            Some(b'i') if matches!(text.get(at.saturating_add(2)), Some(b'B')) => {
                consumed = consumed.saturating_add(2);
            }
            // `D` is obsolescent but still accepted, and both spell 1000.
            Some(b'B' | b'D') => {
                base = 1000;
                consumed = consumed.saturating_add(1);
            }
            _ => {}
        }
    }

    value = match c {
        b'b' => scale(value, 512, &mut overflow),
        // Distinct from the `B` *second* suffix above: as a first suffix it is
        // the obsolescent "blocks of 1024", as in `tar -L 1000B`.
        b'B' => scale(value, 1024, &mut overflow),
        b'c' => value,
        b'w' => scale(value, 2, &mut overflow),
        b'k' | b'K' => scale_by_power(value, base, 1, &mut overflow),
        b'M' | b'm' => scale_by_power(value, base, 2, &mut overflow),
        b'G' | b'g' => scale_by_power(value, base, 3, &mut overflow),
        b'T' | b't' => scale_by_power(value, base, 4, &mut overflow),
        b'P' => scale_by_power(value, base, 5, &mut overflow),
        b'E' => scale_by_power(value, base, 6, &mut overflow),
        b'Z' => scale_by_power(value, base, 7, &mut overflow),
        b'Y' => scale_by_power(value, base, 8, &mut overflow),
        b'R' => scale_by_power(value, base, 9, &mut overflow),
        b'Q' => scale_by_power(value, base, 10, &mut overflow),
        // Reachable only when the caller's list holds a character the scaling
        // switch does not name — the `0` flag itself, most plausibly.
        _ => return (value, Status::of(overflow, true)),
    };

    at = at.saturating_add(consumed);
    let trailing = text.get(at).is_some_and(|c| *c != 0);
    (value, Status::of(overflow, trailing))
}

/// gnulib's `xdectoumax`: [`xstrtoumax`] plus a range check, returning the
/// diagnostic rather than exiting.
///
/// `what` is the caller's phrase — `"invalid number of columns"` for `fold` —
/// and the returned `Err` is the whole message body after the program name,
/// including the quoted argument and the `strerror` tail when there is one:
///
/// ```text
/// invalid number of columns: ‘0’: Numerical result out of range
/// ```
///
/// Upstream exits from inside this function, which is observable: a bad number
/// preempts every option after it, so `fold -w 0 -Z` never mentions `-Z`.
/// Callers that parse options in a loop must therefore report this error where
/// they find it, not collect it for later.
///
/// # Errors
///
/// Returns the diagnostic body when the argument is not a number, carries a
/// suffix the caller did not allow, or falls outside `min..=max`.
pub fn xdectoumax(
    text: &[u8],
    min: u64,
    max: u64,
    valid_suffixes: Option<&[u8]>,
    what: &str,
) -> Result<u64, String> {
    let (value, status) = xstrtoumax(text, valid_suffixes);
    let tail = match status {
        Status::Ok => {
            if value >= min && value <= max {
                return Ok(value);
            }
            if value > INT_MAX_HALF {
                Tail::Overflow
            } else {
                Tail::Range
            }
        }
        Status::Overflow => Tail::Overflow,
        // Both of these leave errno alone (or clear it), so the sentence stops
        // after the argument.
        Status::InvalidSuffix | Status::InvalidSuffixWithOverflow | Status::Invalid => Tail::None,
    };
    Err(format!("{what}: {}{}", quote(text), tail.text()))
}

/// The signed twin of [`xdectoumax`], for the callers whose floor is negative
/// or whose values can be — `nl -v -1`, `tail -n -5`.
///
/// The grammar differs in exactly one place: a leading `-` is a sign here
/// rather than an immediate refusal. The two-sentence split gains its negative
/// half, `tnum < INT_MIN / 2`, so a large *negative* value out of range also
/// reads `Value too large for defined data type`.
///
/// # Errors
///
/// As [`xdectoumax`].
pub fn xdectoimax(
    text: &[u8],
    min: i64,
    max: i64,
    valid_suffixes: Option<&[u8]>,
    what: &str,
) -> Result<i64, String> {
    let (value, status) = xstrtoimax(text, valid_suffixes);
    let tail = match status {
        Status::Ok => {
            if value >= min && value <= max {
                return Ok(value);
            }
            // Upstream writes this as
            // `(tnum < INT_MIN / 2 || INT_MAX / 2 < tnum) ? EOVERFLOW : ERANGE`;
            // the range form says the same thing and is what clippy asks for.
            if (INT_MIN_HALF..=INT_MAX_HALF_SIGNED).contains(&value) {
                Tail::Range
            } else {
                Tail::Overflow
            }
        }
        Status::Overflow => Tail::Overflow,
        Status::InvalidSuffix | Status::InvalidSuffixWithOverflow | Status::Invalid => Tail::None,
    };
    Err(format!("{what}: {}{}", quote(text), tail.text()))
}

/// gnulib's `xstrtoimax`: [`xstrtoumax`]'s grammar with a sign.
///
/// The value saturates at [`i64::MIN`] / [`i64::MAX`], as C's `strtoimax` does,
/// and the suffix multipliers apply to the magnitude.
#[must_use]
pub fn xstrtoimax(text: &[u8], valid_suffixes: Option<&[u8]>) -> (i64, Status) {
    xstrtoimax_base(text, 10, valid_suffixes)
}

/// [`xstrtoimax`] in a base other than ten — gnulib's third argument, which
/// `du -t` passes as `0`.
///
/// It is not decoration there: measured against GNU du 9.4, `du -t 0x10`
/// succeeds and thresholds at sixteen, where base ten would have refused it as
/// `invalid suffix in -t argument '0x10'`. See [`strtoumax_base`] for what a
/// base of zero reads.
#[must_use]
pub fn xstrtoimax_base(text: &[u8], base: u32, valid_suffixes: Option<&[u8]>) -> (i64, Status) {
    let lead = skip_space(text);
    let negative = matches!(text.get(lead), Some(b'-'));
    if !negative {
        let (value, status) = xstrtoumax_base(text, base, valid_suffixes);
        let Ok(signed) = i64::try_from(value) else {
            return (i64::MAX, promote_overflow(status));
        };
        return (signed, status);
    }

    // Re-parse the magnitude with the sign removed. Splicing rather than
    // teaching `xstrtoumax` about signs keeps the unsigned path — the one the
    // size-taking utilities use — with no branch it can get wrong.
    let mut magnitude = Vec::with_capacity(text.len());
    magnitude.extend_from_slice(text.get(lead.saturating_add(1)..).unwrap_or_default());
    let (value, status) = xstrtoumax_base(&magnitude, base, valid_suffixes);
    if status == Status::Invalid {
        return (0, Status::Invalid);
    }
    // i64::MIN's magnitude is one past i64::MAX's, so the negative range is
    // wider by one and the conversion has to go through the unsigned side.
    let limit = i64::MIN.unsigned_abs();
    if value > limit {
        return (i64::MIN, promote_overflow(status));
    }
    let signed = i64::try_from(value)
        .map(i64::saturating_neg)
        .unwrap_or(i64::MIN);
    (signed, status)
}

/// gnulib's `xstrtol_fatal`: the diagnostic an option-taking caller prints when
/// [`xstrtoumax`] refused its argument.
///
/// This is a *different* sentence shape from [`xdectoumax`]'s. That one names
/// the quantity (`invalid number of columns: ‘0’`); this one names the **option
/// that carried it**, which is why upstream threads `long_options` and the short
/// option character all the way down into it:
///
/// ```text
/// invalid -N argument 'abc'
/// invalid suffix in --read-bytes argument '4q'
/// -w argument '99999999999999999999' too large
/// ```
///
/// **The marks are straight, and stay straight in every locale.** That is worth
/// stating because it is not what the file this lives beside would predict:
/// `xstrtol-error.c` is gnulib's, and gnulib is where [`quote`] comes from, but
/// this particular message spells its `'%s'` into the format string the way
/// glibc's `getopt_long` does rather than calling `quote()`. Measured against
/// GNU od 9.4 under `LC_ALL=C.UTF-8`:
///
/// ```text
/// $ od -j x  f   ->  od: invalid -j argument 'x'          <- straight, here
/// $ head -n x f  ->  head: invalid number of lines: ‘x’   <- curly, xdectoumax
/// ```
///
/// So the sibling sentence shape and this one disagree about their marks, and
/// which one a utility prints is a fact about upstream's source rather than
/// about the message. `sort -S x` prints this shape too, and is straight.
///
/// `option` is that name already rendered with its hyphens — `"-N"` or
/// `"--read-bytes"` — rather than upstream's `(opt_idx, c, long_options)` triple,
/// which exists only because C cannot return two strings from a `switch`. Which
/// spelling appears is observable: it is whichever form the user actually typed,
/// so a caller that resolved a long option must pass the long name.
///
/// Returns `None` for [`Status::Ok`], where upstream calls `abort` — there is no
/// message for a number that converted, and a caller reaching here with one has
/// a bug rather than a diagnostic to print.
#[must_use]
pub fn strtol_fatal(status: Status, option: &str, arg: &[u8]) -> Option<String> {
    // No `strerror` tail on any of the three: upstream passes 0 for errno.
    //
    // `quote_glibc`, not `quote` — see the straight-marks note above. Upstream
    // interpolates the raw bytes between two literal apostrophes and escapes
    // nothing at all (`od -j $'\xff'` prints the 0xFF byte itself); we escape,
    // for the same reason `quote_glibc` escapes everywhere else, and the two
    // agree byte for byte on every argument that is not an attack.
    match status {
        Status::Ok => None,
        Status::Invalid => Some(format!("invalid {option} argument {}", quote_glibc(arg))),
        Status::InvalidSuffix | Status::InvalidSuffixWithOverflow => Some(format!(
            "invalid suffix in {option} argument {}",
            quote_glibc(arg)
        )),
        Status::Overflow => Some(format!("{option} argument {} too large", quote_glibc(arg))),
    }
}

/// [`xstrtoumax_base`] and [`strtol_fatal`] as the one step every caller of the
/// pair actually wants.
///
/// Upstream writes it out longhand at each site — `xstrtoumax (…); if (s_err !=
/// LONGINT_OK) xstrtol_fatal (…)` — five times in `od` alone. Folding it here
/// keeps the base and the suffix list next to the option they belong to and
/// leaves no site free to forget the check.
///
/// # Errors
///
/// The diagnostic body, ready to print after the program name.
pub fn xstrtoumax_option(
    text: &[u8],
    base: u32,
    valid_suffixes: Option<&[u8]>,
    option: &str,
) -> Result<u64, String> {
    let (value, status) = xstrtoumax_base(text, base, valid_suffixes);
    match strtol_fatal(status, option, text) {
        Some(message) => Err(message),
        None => Ok(value),
    }
}

/// Fold an overflow that only the signed range noticed into an existing status.
fn promote_overflow(status: Status) -> Status {
    match status {
        Status::Ok | Status::Overflow => Status::Overflow,
        Status::InvalidSuffix | Status::InvalidSuffixWithOverflow => {
            Status::InvalidSuffixWithOverflow
        }
        Status::Invalid => Status::Invalid,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::expect_used)]
mod tests {
    use super::*;

    const NONE: Option<&[u8]> = Some(b"");
    const SIZES: Option<&[u8]> = Some(b"bkKmMGTPEZYRQ0");

    fn columns(text: &str) -> Result<u64, String> {
        // fold's own call: `xdectoumax (optarg, 1, SIZE_MAX - TAB_WIDTH - 1,
        // "", _("invalid number of columns"), 0)`.
        xdectoumax(
            text.as_bytes(),
            1,
            u64::MAX.saturating_sub(9),
            NONE,
            "invalid number of columns",
        )
    }

    #[test]
    fn a_plain_decimal_is_the_value() {
        assert_eq!(columns("80"), Ok(80));
        assert_eq!(columns("1"), Ok(1));
        assert_eq!(columns("007"), Ok(7));
    }

    #[test]
    fn leading_space_and_plus_are_accepted_trailing_space_is_not() {
        assert_eq!(columns(" 5"), Ok(5));
        assert_eq!(columns("+5"), Ok(5));
        assert_eq!(
            columns("5 "),
            Err("invalid number of columns: ‘5 ’".to_string())
        );
    }

    #[test]
    fn below_the_floor_is_out_of_range_and_above_the_ceiling_is_too_large() {
        // The split is by the *value* against INT_MAX/2, not by which bound
        // was violated. Measured against GNU fold 9.4.
        assert_eq!(
            columns("0"),
            Err("invalid number of columns: ‘0’: Numerical result out of range".to_string())
        );
        assert_eq!(
            columns("18446744073709551607"),
            Err("invalid number of columns: ‘18446744073709551607’: \
                 Value too large for defined data type"
                .to_string())
        );
        // One below that is the largest value fold accepts: SIZE_MAX - 9.
        assert_eq!(columns("18446744073709551606"), Ok(u64::MAX - 9));
    }

    #[test]
    fn a_number_beyond_the_type_is_too_large_not_merely_out_of_range() {
        assert_eq!(
            columns("9999999999999999999999999999"),
            Err(
                "invalid number of columns: ‘9999999999999999999999999999’: \
                 Value too large for defined data type"
                    .to_string()
            )
        );
    }

    #[test]
    fn nonsense_gets_no_strerror_tail() {
        for text in ["bogus", "", "  ", "+", "-", "0x10", "1_000", "1,2"] {
            assert_eq!(
                columns(text),
                Err(format!("invalid number of columns: ‘{text}’")),
                "for {text:?}"
            );
        }
    }

    #[test]
    fn a_negative_is_invalid_rather_than_out_of_range_when_unsigned() {
        // Because gnulib refuses the sign before C's strtoumax can wrap it.
        assert_eq!(
            columns("-3"),
            Err("invalid number of columns: ‘-3’".to_string())
        );
    }

    #[test]
    fn an_invalid_suffix_suppresses_the_overflow_tail() {
        // LONGINT_INVALID_SUFFIX_CHAR_WITH_OVERFLOW clears errno on purpose:
        // "don't show ERANGE errors for invalid numbers".
        assert_eq!(
            columns("99999999999999999999999x"),
            Err("invalid number of columns: ‘99999999999999999999999x’".to_string())
        );
        // …while the same overflow with no trailing character keeps it.
        assert_eq!(
            columns("99999999999999999999999"),
            Err("invalid number of columns: ‘99999999999999999999999’: \
                 Value too large for defined data type"
                .to_string())
        );
    }

    #[test]
    fn a_suffix_list_of_none_rejects_every_suffix() {
        assert_eq!(
            columns("1K"),
            Err("invalid number of columns: ‘1K’".to_string())
        );
    }

    #[test]
    fn the_size_suffixes_are_powers_of_1024_by_default() {
        assert_eq!(xstrtoumax(b"1K", SIZES), (1024, Status::Ok));
        assert_eq!(xstrtoumax(b"1k", SIZES), (1024, Status::Ok));
        assert_eq!(xstrtoumax(b"2M", SIZES), (2 * 1024 * 1024, Status::Ok));
        assert_eq!(xstrtoumax(b"1G", SIZES), (1024 * 1024 * 1024, Status::Ok));
        assert_eq!(xstrtoumax(b"1b", SIZES), (512, Status::Ok));
    }

    #[test]
    fn a_b_second_suffix_switches_the_base_to_1000_and_ib_keeps_1024() {
        assert_eq!(xstrtoumax(b"1KB", SIZES), (1000, Status::Ok));
        assert_eq!(xstrtoumax(b"1KiB", SIZES), (1024, Status::Ok));
        assert_eq!(xstrtoumax(b"1MB", SIZES), (1_000_000, Status::Ok));
        assert_eq!(xstrtoumax(b"1MiB", SIZES), (1024 * 1024, Status::Ok));
        // `D` is the obsolescent spelling of `B`.
        assert_eq!(xstrtoumax(b"1KD", SIZES), (1000, Status::Ok));
    }

    #[test]
    fn a_bare_suffix_means_one_of_it() {
        // "If there is no number but there is a valid suffix, assume the
        // number is 1."
        assert_eq!(xstrtoumax(b"K", SIZES), (1024, Status::Ok));
        assert_eq!(xstrtoumax(b"MiB", SIZES), (1024 * 1024, Status::Ok));
        // …but only when the *first* byte is the suffix, because C left the
        // end pointer at the start of the string.
        assert_eq!(xstrtoumax(b" K", SIZES).1, Status::Invalid);
    }

    #[test]
    fn a_suffix_that_overflows_saturates_and_says_so() {
        let (value, status) = xstrtoumax(b"1000Q", SIZES);
        assert_eq!(status, Status::Overflow);
        assert_eq!(value, u64::MAX);
    }

    #[test]
    fn text_after_a_valid_suffix_is_still_invalid() {
        assert_eq!(xstrtoumax(b"1Kx", SIZES), (1024, Status::InvalidSuffix));
        assert_eq!(xstrtoumax(b"1KiBx", SIZES), (1024, Status::InvalidSuffix));
    }

    #[test]
    fn a_null_suffix_list_ignores_trailing_text_entirely() {
        assert_eq!(xstrtoumax(b"12abc", None), (12, Status::Ok));
    }

    #[test]
    fn the_signed_side_takes_a_sign_and_keeps_the_two_sentences() {
        let what = "invalid line number field width";
        assert_eq!(xdectoimax(b"-1", -9, 9, NONE, what), Ok(-1));
        assert_eq!(
            xdectoimax(b"0", 1, 2_147_483_647, NONE, what),
            Err(format!("{what}: ‘0’: Numerical result out of range"))
        );
        assert_eq!(
            xdectoimax(b"2147483648", 1, 2_147_483_647, NONE, what),
            Err(format!(
                "{what}: ‘2147483648’: Value too large for defined data type"
            ))
        );
        assert_eq!(
            xdectoimax(b"99999999999999999999", 1, 2_147_483_647, NONE, what),
            Err(format!(
                "{what}: ‘99999999999999999999’: Value too large for defined data type"
            ))
        );
        assert_eq!(
            xdectoimax(b"abc", 1, 9, NONE, what),
            Err(format!("{what}: ‘abc’"))
        );
    }

    #[test]
    fn the_signed_side_reaches_both_ends_of_the_type() {
        assert_eq!(
            xstrtoimax(b"9223372036854775807", NONE),
            (i64::MAX, Status::Ok)
        );
        assert_eq!(
            xstrtoimax(b"-9223372036854775808", NONE),
            (i64::MIN, Status::Ok)
        );
        assert_eq!(
            xstrtoimax(b"9223372036854775808", NONE),
            (i64::MAX, Status::Overflow)
        );
        assert_eq!(
            xstrtoimax(b"-9223372036854775809", NONE),
            (i64::MIN, Status::Overflow)
        );
    }

    /// The base parameter reaches the signed side too, which is what `du -t`
    /// needs: measured against GNU du 9.4, `du -t 0x10` thresholds at sixteen
    /// and `du -t 010` at eight, so a base-ten reading of either would be a
    /// different threshold rather than a different diagnostic.
    #[test]
    fn the_signed_side_reads_a_base_prefix_when_asked() {
        const SIZES: Option<&[u8]> = Some(b"kKmMGTPEZYRQ0");
        assert_eq!(xstrtoimax_base(b"0x10", 0, SIZES), (16, Status::Ok));
        assert_eq!(xstrtoimax_base(b"010", 0, SIZES), (8, Status::Ok));
        assert_eq!(xstrtoimax_base(b"-0x10", 0, SIZES), (-16, Status::Ok));
        // Base ten is the other reading of the same three arguments, and it
        // refuses rather than answering sixteen.
        assert_eq!(xstrtoimax_base(b"0x10", 10, SIZES).1, Status::InvalidSuffix);
        // The suffix still applies on top of the prefix.
        assert_eq!(xstrtoimax_base(b"0x10K", 0, SIZES), (16384, Status::Ok));
    }

    #[test]
    fn a_lone_sign_is_not_a_number_on_either_side() {
        assert_eq!(xstrtoimax(b"-", NONE).1, Status::Invalid);
        assert_eq!(xstrtoimax(b"+", NONE).1, Status::Invalid);
        assert_eq!(xstrtoumax(b"-", NONE).1, Status::Invalid);
    }

    #[test]
    fn the_quoted_argument_survives_bytes_that_are_not_text() {
        // The argument is echoed through `quote`, which is why this takes
        // bytes rather than a string: a width argument can be any bytes at all.
        let message = xdectoumax(b"\xff\xfe", 1, 9, NONE, "invalid number of columns")
            .expect_err("not a number");
        assert!(
            message.starts_with("invalid number of columns: "),
            "{message}"
        );
    }
}
