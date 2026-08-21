//! gnulib's `human_readable` — the "1.5G" / "1.5 GB" / "1.4 GiB" formatter.
//!
//! Every utility that can print a size in a unit larger than a byte calls this
//! one function upstream: `df -h`, `du -h`, `ls -h`, `dd`'s transfer line, and
//! `--block-size=human-readable` everywhere it is accepted. It is here for the
//! same reason [`crate::xnum`] is: the rule is much larger than it looks, and a
//! partial reimplementation per caller is a set of utilities that round the
//! same number differently.
//!
//! How much larger it is than it looks:
//!
//! - **The value is a ratio, not a count.** The real signature takes
//!   `from_block_size` and `to_block_size`, and the answer is
//!   `n * from / to`. `df` uses it to restate 512-byte blocks in 1 KiB units;
//!   `dd` uses it to turn a byte count and an elapsed nanosecond count into a
//!   *rate*, with `from = 1e9` and `to = the elapsed time`. A helper that only
//!   takes a byte count cannot express either.
//! - **Rounding is done on the ratio, exactly, when it can be.** Upstream has
//!   two whole implementations: an integer one that tracks tenths and a
//!   three-valued "how far past the tenth are we" residue, and a floating one
//!   for when the ratio cannot be reduced. The integer path is not an
//!   optimisation — it is what makes `1024` render as `1.0 KiB` and not
//!   `1.0 KiB` by luck.
//! - **The choice between one decimal and none is a rule, not a format
//!   string.** A scaled value gets one decimal only while it is below ten;
//!   at ten and above the decimal is dropped, so the output is `9.8 MB` but
//!   `977 KiB`. See [`Opts::SUPPRESS_POINT_ZERO`] for the second rule layered
//!   on top of that one.
//! - **`k` is lowercase and every other prefix is not.** Only in base 1000,
//!   and only at exponent 1: `kB`, then `MB`, `GB`. In base 1024 it is `KiB`.
//!   This is SI's own inconsistency, faithfully reproduced.
//!
//! Ported from gnulib `lib/human.c` (the copy vendored into coreutils 9.4).
//! The one deliberate difference from upstream is the arithmetic type on the
//! floating path — see [`human_readable`].

/// The option bits, matching gnulib's `enum human_inexact_style` and the flag
/// bits beside it. The numeric values are upstream's, so a call site can be
/// compared against the C without translating.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Opts(u32);

impl Opts {
    /// Round an inexact value **up**. Mutually exclusive with the other two
    /// rounding styles; upstream packs all three into the same two bits.
    ///
    /// This is the default in the literal sense that it is zero, and it is what
    /// `ls -h`, `du -h` and `df -h` all use — deliberately, so that a file or a
    /// filesystem never renders as smaller than it really is.
    pub const CEILING: Self = Self(0);
    /// Round an inexact value to the nearest representable output, ties to
    /// even.
    ///
    /// Within coreutils this is `dd`'s style and, as far as the sweep in
    /// `tests/human_gnu.rs` could find, only `dd`'s — which is why `dd` is the
    /// only instrument that can measure this path. Measured on GNU 8.32, a
    /// 999499-byte file is `1.0M` to `ls -h` but `999 kB` to `dd`.
    pub const ROUND_TO_NEAREST: Self = Self(1);
    /// Round an inexact value **down**.
    pub const FLOOR: Self = Self(2);
    /// Insert the locale's thousands separators. In the C locale — the only
    /// locale this crate implements — the grouping string is empty and this is
    /// the identity, which is why it has no separate code path below.
    pub const GROUP_DIGITS: Self = Self(4);
    /// Print `1K` rather than `1.0K`: drop a decimal point followed by a zero.
    /// `df -h` and `du -h` pass this; `dd` does not, which is the whole reason
    /// `dd` says `1.0 kB` where `df` would say `1.0K` only when the tenth is
    /// nonzero.
    pub const SUPPRESS_POINT_ZERO: Self = Self(8);
    /// Divide by the base until the value is below it, and report the
    /// exponent as a letter. Without this the value is printed whole.
    pub const AUTOSCALE: Self = Self(16);
    /// Scale by 1024 rather than 1000, and spell the unit `KiB` rather
    /// than `kB`.
    pub const BASE_1024: Self = Self(32);
    /// Put a space between the number and the unit. Note that the space is
    /// emitted only when there *is* a unit — see [`Opts::B`], which is what
    /// makes a bare byte count still carry one.
    pub const SPACE_BEFORE_UNIT: Self = Self(64);
    /// Emit the unit at all. Without this the number is bare, whatever the
    /// exponent — so `ls -h`'s `1.5M` and `dd`'s `1.5 MB` differ in *two*
    /// options, not one.
    pub const SI: Self = Self(128);
    /// Append `B` (and, in base 1024, the `i` before it). Only meaningful
    /// together with [`Opts::SI`].
    pub const B: Self = Self(256);

    /// The empty set, for building with `|`.
    pub const NONE: Self = Self(0);

    /// Whether every bit of `other` is set here.
    #[must_use]
    pub const fn has(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// The rounding style, isolated from the flag bits.
    ///
    /// Upstream masks with `human_round_to_nearest | human_floor |
    /// human_ceiling`, which is `3`, and compares the result against a whole
    /// style constant. `CEILING` being zero is why this cannot be a `has`
    /// test: every option set "has" it.
    const fn inexact_style(self) -> u32 {
        self.0 & 3
    }
}

impl std::ops::BitOr for Opts {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// The prefix letters, indexed by exponent. Index 0 is unused: exponent zero
/// means no prefix at all, not a prefix that happens to be empty.
///
/// `K` is uppercase here and lowercased at the call site for base 1000 only —
/// upstream does the same, because SI's kilo is `k` while IEC's kibi is `Ki`.
const POWER_LETTER: [u8; 11] = *b"\0KMGTPEZYRQ";

/// The largest exponent [`POWER_LETTER`] can name. `Q` (quetta / 2^100).
const EXPONENT_MAX: i32 = POWER_LETTER.len() as i32 - 1;

/// How far past `amt.tenths` the true value lies. Upstream calls this
/// `rounding` and gives it four values; naming them is the only change.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Residue {
    /// The value is exactly `amt.tenths`.
    Exact,
    /// Strictly between `amt.tenths` and `amt.tenths + 0.05`.
    BelowHalf,
    /// Exactly `amt.tenths + 0.05` — the tie.
    Half,
    /// Strictly between `amt.tenths + 0.05` and the next tenth.
    AboveHalf,
}

impl Residue {
    /// Upstream stores this as a small integer and does arithmetic on it
    /// (`rounding >> 1`, `r2 + rounding`), so the integer has to be reachable.
    const fn value(self) -> u32 {
        match self {
            Self::Exact => 0,
            Self::BelowHalf => 1,
            Self::Half => 2,
            Self::AboveHalf => 3,
        }
    }

    const fn is_exact(self) -> bool {
        matches!(self, Self::Exact)
    }
}

/// `n * from_block_size / to_block_size`, rendered the way gnulib renders it.
///
/// `from_block_size` is the unit `n` is counted in and `to_block_size` the unit
/// to report in; both are `1` when `n` is simply a byte count. A
/// `to_block_size` of zero is upstream's "fall back on floating point" case and
/// is handled the same way rather than rejected, because upstream's callers can
/// reach it (`dd`'s rate, on a transfer that took no measurable time, is
/// guarded at the call site rather than here).
///
/// # Difference from upstream
///
/// The floating path computes in `f64` where gnulib uses `long double`. On x86
/// that is 53 significand bits against 64. It cannot change the output of any
/// call that reaches the *integer* path — that path does no floating arithmetic
/// at all — and on the floating path it can only move the last displayed digit,
/// which is the first decimal of a value below ten or the last digit of an
/// integer below 1000. Reproducing `long double` would mean either an x87
/// dependency in code that also compiles for the target, or software extended
/// precision for one rounding step; [`crate::extfloat`] exists for the case
/// where the *format* needs 80-bit fidelity (`printf %La`), not the arithmetic.
#[must_use]
pub fn human_readable(n: u64, opts: Opts, from_block_size: u64, to_block_size: u64) -> String {
    let base: u64 = if opts.has(Opts::BASE_1024) { 1024 } else { 1000 };

    // The two exact cases, in upstream's order. Anything else falls through to
    // the floating path below.
    let exact = if to_block_size <= from_block_size && to_block_size != 0 {
        if from_block_size.is_multiple_of(to_block_size) {
            let multiplier = from_block_size / to_block_size;
            n.checked_mul(multiplier)
                .map(|amt| (amt, 0u32, Residue::Exact))
        } else {
            None
        }
    } else if from_block_size != 0 && to_block_size.is_multiple_of(from_block_size) {
        let divisor = to_block_size / from_block_size;
        // `(n % divisor) * 10` cannot overflow for any divisor a caller can
        // reach here, but saturating it costs nothing and keeps the arithmetic
        // lint quiet without an allow.
        let r10 = (n % divisor).saturating_mul(10);
        let r2 = (r10 % divisor).saturating_mul(2);
        let residue = if r2 < divisor {
            if r2 > 0 { Residue::BelowHalf } else { Residue::Exact }
        } else if divisor < r2 {
            Residue::AboveHalf
        } else {
            Residue::Half
        };
        Some((n / divisor, u32::try_from(r10 / divisor).unwrap_or(0), residue))
    } else {
        None
    };

    let (digits, exponent) = match exact {
        Some((amt, tenths, residue)) => integer_path(amt, tenths, residue, opts, base),
        None => floating_path(n, opts, base, from_block_size, to_block_size),
    };

    let mut out = digits;
    if opts.has(Opts::SI) {
        // Upstream recomputes the exponent here when autoscale was off, so
        // that `--block-size=1M` still prints an `M`. `exponent` is `None` in
        // exactly that case.
        let exponent = exponent.unwrap_or_else(|| {
            let mut e = 0;
            let mut power: u64 = 1;
            while power < to_block_size {
                power = power.saturating_mul(base);
                e += 1;
                if e == EXPONENT_MAX {
                    break;
                }
            }
            e
        });

        if (exponent != 0 || opts.has(Opts::B)) && opts.has(Opts::SPACE_BEFORE_UNIT) {
            out.push(' ');
        }
        if exponent != 0 {
            let letter = if !opts.has(Opts::BASE_1024) && exponent == 1 {
                b'k'
            } else {
                POWER_LETTER
                    .get(usize::try_from(exponent).unwrap_or(0))
                    .copied()
                    .unwrap_or(b'\0')
            };
            out.push(char::from(letter));
        }
        if opts.has(Opts::B) {
            if opts.has(Opts::BASE_1024) && exponent != 0 {
                out.push('i');
            }
            out.push('B');
        }
    }
    out
}

/// The exact path: scale by repeated integer division, carrying a tenth and a
/// residue so the final rounding sees the whole discarded remainder.
///
/// Returns the digits and, when autoscaling, the exponent they were scaled by.
/// `None` for the exponent means "not autoscaled", which the caller
/// distinguishes because it then derives one from `to_block_size` instead.
fn integer_path(
    mut amt: u64,
    mut tenths: u32,
    mut residue: Residue,
    opts: Opts,
    base: u64,
) -> (String, Option<i32>) {
    let style = opts.inexact_style();
    let round_to_nearest = style == Opts::ROUND_TO_NEAREST.0;
    let ceiling = style == Opts::CEILING.0 && !round_to_nearest && style != Opts::FLOOR.0;
    let mut exponent: Option<i32> = None;
    // The fractional digits, if any, as they will appear *after* the integer
    // part. Built here because upstream writes them into the buffer backwards
    // before the integer digits are known.
    let mut fraction = String::new();

    if opts.has(Opts::AUTOSCALE) {
        let mut e = 0i32;
        if base <= amt {
            while {
                let r10 = (amt % base)
                    .saturating_mul(10)
                    .saturating_add(u64::from(tenths));
                let r2 = (r10 % base)
                    .saturating_mul(2)
                    .saturating_add(u64::from(residue.value() >> 1));
                amt /= base;
                tenths = u32::try_from(r10 / base).unwrap_or(0);
                let carried = r2.saturating_add(u64::from(residue.value()));
                residue = if r2 < base {
                    if carried != 0 { Residue::BelowHalf } else { Residue::Exact }
                } else if base < carried {
                    Residue::AboveHalf
                } else {
                    Residue::Half
                };
                e += 1;
                base <= amt && e < EXPONENT_MAX
            } {}

            if amt < 10 {
                // Round the tenth before deciding whether to print it: a value
                // that rounds up from .95 to 1.0 must widen the integer part,
                // not print `0.10`.
                let bump = if round_to_nearest {
                    residue.value() + (tenths & 1) > 2
                } else {
                    ceiling && !residue.is_exact()
                };
                if bump {
                    tenths += 1;
                    residue = Residue::Exact;
                    if tenths == 10 {
                        amt += 1;
                        tenths = 0;
                    }
                }

                if amt < 10 && (tenths != 0 || !opts.has(Opts::SUPPRESS_POINT_ZERO)) {
                    fraction.push('.');
                    fraction.push(char::from(b'0' + u8::try_from(tenths).unwrap_or(0)));
                    tenths = 0;
                    residue = Residue::Exact;
                }
            }
        }
        exponent = Some(e);
    }

    // The final rounding of the integer part, which fires when the tenths
    // were *not* printed — either because the value was ten or larger, or
    // because autoscale was off entirely.
    let bump = if round_to_nearest {
        // Upstream is `5 < tenths + (0 < rounding + (amt & 1))` (human.c:327).
        // The threshold is `0 <`, not the `2 <` that appears twelve lines
        // earlier at human.c:301 — the two rounding decisions in this function
        // look alike and are not the same test, and reading the second as a
        // copy of the first moves the SI carry by 450 bytes (999500 renders as
        // `999 kB` instead of the measured `1.0 MB`).
        //
        // What it means: a half or more rounds up unless the value is an exact
        // tie *and* `amt` is even. That is round-half-to-even, with the parity
        // consulted only to break a tie that `rounding == 0` certifies is exact.
        let up = residue.value() + u32::try_from(amt & 1).unwrap_or(0) > 0;
        u64::from(tenths) + u64::from(up) > 5
    } else {
        ceiling && (tenths != 0 || !residue.is_exact())
    };
    if bump {
        amt = amt.saturating_add(1);
        if opts.has(Opts::AUTOSCALE)
            && amt == base
            && exponent.is_some_and(|e| e < EXPONENT_MAX)
        {
            exponent = exponent.map(|e| e + 1);
            if !opts.has(Opts::SUPPRESS_POINT_ZERO) {
                fraction = ".0".to_string();
            }
            amt = 1;
        }
    }

    (format!("{amt}{fraction}"), exponent)
}

/// The inexact path: upstream's `FIXME: This can yield answers that are
/// slightly off` comment applies here too, and for the same reason.
fn floating_path(
    n: u64,
    opts: Opts,
    base: u64,
    from_block_size: u64,
    to_block_size: u64,
) -> (String, Option<i32>) {
    let style = opts.inexact_style();
    #[allow(clippy::cast_precision_loss)]
    let mut damt = (n as f64) * ((from_block_size as f64) / (to_block_size as f64));

    if !opts.has(Opts::AUTOSCALE) {
        return (format_fixed(adjust(style, damt), 0), None);
    }

    let mut e = 0i32;
    let mut divisor = 1f64;
    #[allow(clippy::cast_precision_loss)]
    let basef = base as f64;
    loop {
        divisor *= basef;
        e += 1;
        if !(divisor * basef <= damt && e < EXPONENT_MAX) {
            break;
        }
    }
    // Upstream enters the loop unconditionally, so a value below the base
    // still divides once and comes back with exponent 1 — no: it divides and
    // then, because `e * base <= damt` failed on the first test, stops with
    // exponent 1 having divided by `base`. The condition is a do-while, so the
    // first division always happens; a value below `base` therefore renders as
    // a fraction of one `base` unit. That is upstream's behaviour and not a
    // transcription slip: reaching this path at all requires a ratio, and a
    // ratio below one unit has no whole-unit spelling.
    damt /= divisor;

    let mut text = format_fixed(adjust(style, damt), 1);
    // "1 + <point and digit> + <1 unless base 1024> < len" is upstream's test
    // for "the integer part got wide enough that a decimal is noise". The
    // base-1024 term is there because a 1024-scaled value can reach four
    // integer digits where a 1000-scaled one cannot.
    let extra = usize::from(!opts.has(Opts::BASE_1024));
    if 1 + 2 + extra < text.len()
        || (opts.has(Opts::SUPPRESS_POINT_ZERO) && text.ends_with('0'))
    {
        text = format_fixed(adjust(style, damt * 10.0) / 10.0, 0);
    }
    (text, Some(e))
}

/// gnulib's `adjust_value`: for a non-nearest style, snap to an integer before
/// the decimal formatter sees the value.
fn adjust(style: u32, value: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let limit = u64::MAX as f64;
    if style != Opts::ROUND_TO_NEAREST.0 && value < limit {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let u = value as u64;
        #[allow(clippy::cast_precision_loss)]
        let uf = u as f64;
        return uf + f64::from(u8::from(style == Opts::CEILING.0 && uf != value));
    }
    value
}

/// C's `sprintf("%.<prec>f")`, which rounds ties to even — the same rule Rust's
/// `{:.*}` uses, so this is a thin wrapper and not a reimplementation.
fn format_fixed(value: f64, precision: usize) -> String {
    format!("{value:.precision$}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    /// `dd`'s option set: an SI byte count with a space before the unit.
    fn dd_si() -> Opts {
        Opts::AUTOSCALE | Opts::ROUND_TO_NEAREST | Opts::SPACE_BEFORE_UNIT | Opts::SI | Opts::B
    }

    fn dd_iec() -> Opts {
        dd_si() | Opts::BASE_1024
    }

    fn si(n: u64) -> String {
        human_readable(n, dd_si(), 1, 1)
    }

    fn iec(n: u64) -> String {
        human_readable(n, dd_iec(), 1, 1)
    }

    /// Measured from GNU coreutils 9.4's own `dd` output on the dev machine —
    /// these are the exact parentheticals it printed for these byte counts.
    #[test]
    fn dd_byte_counts_match_gnu() {
        assert_eq!(si(1), "1 B");
        assert_eq!(si(512), "512 B");
        assert_eq!(si(1000), "1.0 kB");
        assert_eq!(si(1024), "1.0 kB");
        assert_eq!(si(999_999), "1.0 MB");
        assert_eq!(si(1_048_576), "1.0 MB");
        assert_eq!(si(1_500_000), "1.5 MB");

        assert_eq!(iec(1), "1 B");
        assert_eq!(iec(1000), "1000 B");
        assert_eq!(iec(1024), "1.0 KiB");
        assert_eq!(iec(999_999), "977 KiB");
        assert_eq!(iec(1_048_576), "1.0 MiB");
        assert_eq!(iec(1_500_000), "1.4 MiB");
    }

    /// The decimal disappears at ten, not at some byte threshold. This is the
    /// rule a naive `{:.1}` formatter gets wrong, and it is why `977 KiB` above
    /// has no decimal while `1.4 MiB` does.
    #[test]
    fn one_decimal_only_below_ten() {
        assert_eq!(iec(9 * 1024), "9.0 KiB");
        assert_eq!(iec(10 * 1024), "10 KiB");
        assert_eq!(iec(1023 * 1024), "1023 KiB");
    }

    /// Rounding up out of the top of a scale must widen the exponent rather
    /// than print the base itself.
    ///
    /// The boundary below is measured, not derived: GNU `dd` 8.32 on the
    /// development host reports `999499 bytes (999 kB, 976 KiB)` and
    /// `999500 bytes (1.0 MB, 976 KiB)`. One byte of input decides between a
    /// three-digit `kB` and a one-decimal `MB`, and the carry has to travel
    /// out of the mantissa and into the exponent to do it.
    ///
    /// Note that `ls` cannot be used to measure this: `ls -h`/`ls --si` round
    /// *up* (`human_ceiling`, so a file never looks smaller than it is) and
    /// report all of these as `1.0M`. Only the round-to-nearest callers —
    /// `dd`, `df`, `du` — exercise the boundary at all.
    #[test]
    fn rounding_up_past_the_base_advances_the_exponent() {
        // 1023.99 KiB rounds to 1.0 MiB, not to 1024.0 KiB.
        assert_eq!(iec(1024 * 1024 - 1), "1.0 MiB");

        // The SI carry, either side of the measured boundary.
        assert_eq!(si(999_499), "999 kB");
        assert_eq!(si(999_500), "1.0 MB");
        assert_eq!(si(999_949), "1.0 MB");
        assert_eq!(si(999_999), "1.0 MB");

        // The same run measured the IEC column, whose boundary sits elsewhere
        // (976.5 KiB is 999936 bytes), so the two scales disagree about which
        // side of a round number a given count falls on.
        assert_eq!(iec(999_499), "976 KiB");
        assert_eq!(iec(999_550), "976 KiB");
        assert_eq!(iec(999_949), "977 KiB");
    }

    /// `SUPPRESS_POINT_ZERO` is what separates `df -h`'s `16G` from `dd`'s
    /// `16 GB` — the exponent and the value are identical.
    #[test]
    fn suppress_point_zero_drops_a_trailing_zero_decimal() {
        let df = Opts::AUTOSCALE
            | Opts::ROUND_TO_NEAREST
            | Opts::BASE_1024
            | Opts::SUPPRESS_POINT_ZERO
            | Opts::SI;
        assert_eq!(human_readable(1024, df, 1, 1), "1K");
        assert_eq!(human_readable(1536, df, 1, 1), "1.5K");
        assert_eq!(human_readable(16 * 1024 * 1024 * 1024, df, 1, 1), "16G");
        // Without the option the same numbers keep the zero.
        let keep = df.0 & !Opts::SUPPRESS_POINT_ZERO.0;
        assert_eq!(human_readable(1024, Opts(keep), 1, 1), "1.0K");
    }

    /// A zero exponent still gets its space when `B` is asked for, and does not
    /// when it is not. This is the whole of `abbreviation_lacks_prefix`'s input
    /// in `dd`.
    #[test]
    fn the_space_belongs_to_the_unit_not_the_number() {
        assert_eq!(si(0), "0 B");
        let no_b = Opts::AUTOSCALE | Opts::ROUND_TO_NEAREST | Opts::SPACE_BEFORE_UNIT | Opts::SI;
        assert_eq!(human_readable(0, no_b, 1, 1), "0");
        assert_eq!(human_readable(2048, no_b | Opts::BASE_1024, 1, 1), "2.0 K");
    }

    /// The block-size arguments are the reason this is not a byte-count
    /// helper: `df` restates 512-byte blocks in kibibytes with them.
    #[test]
    fn block_sizes_restate_the_unit() {
        let plain = Opts::ROUND_TO_NEAREST;
        // 2048 512-byte blocks is 1024 KiB.
        assert_eq!(human_readable(2048, plain, 512, 1024), "1024");
        // Rounding to nearest, not truncation: 3 blocks of 512 in 1024 units
        // is 1.5, which renders as 2.
        assert_eq!(human_readable(3, plain, 512, 1024), "2");
        assert_eq!(human_readable(1, plain, 512, 1024), "0");
    }

    /// Without `SI` there is no unit at all, whatever the exponent — the
    /// difference between `ls -h`'s bare `1.5M` and this crate printing
    /// `1.5 MB` is two option bits, and forgetting either produces the other
    /// utility's output.
    #[test]
    fn si_gates_the_whole_suffix() {
        let bare = Opts::AUTOSCALE | Opts::ROUND_TO_NEAREST | Opts::BASE_1024;
        assert_eq!(human_readable(1_572_864, bare, 1, 1), "1.5");
        assert_eq!(human_readable(1_572_864, bare | Opts::SI, 1, 1), "1.5M");
    }
}
