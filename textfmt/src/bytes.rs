//! Byte counts written the way a person reads them: `1.5 MiB`, `2.3 GB`.
//!
//! # Why this is a module and not four lines at the call site
//!
//! It was four lines at the call site, forty-seven times — forty-four byte
//! sizes and three throughput rates. An audit on 2026-08-21 read every one of
//! them and found that **twenty-nine divided by 1024 and labelled the result
//! `KB`, `MB`, `GB`** — which is not a style choice, it is a false statement.
//! `KB` means 1000 bytes. A 1 GiB file displayed by `apps/explorer` read
//! `1.00 GB`; it is 1.07 GB. A 4 TiB disk read `4096.00 GB`; it is 4.40 TB.
//!
//! The two halves of the answer — *what to divide by* and *what to call the
//! result* — were written as two independent statements, so they could
//! disagree, and in a majority of copies they did. Here they cannot: a base
//! and its unit names are one table, chosen together by [`Unit`], and no
//! caller can pair 1024 with `KB`.
//!
//! The disagreement was not merely theoretical. `gui/desktop` displayed the
//! same network byte counters in two places — the tray indicator (base 1000,
//! `MB`) and the network settings page (base 1024, `MB`) — so 1 500 000 bytes
//! of traffic read `1.5 MB` in the tray and `1.4 MB` in settings, in one
//! subsystem, on one screen.
//!
//! # The rounding lie
//!
//! Every one of the hand-written copies picked a unit from the raw byte count
//! and *then* rounded, which means the printed number could reach the next
//! unit while keeping the current unit's name. 1 048 575 bytes is under 1 MiB, so the
//! `KiB` branch was taken, and `1048575 / 1024 = 1023.999…` printed as
//! **`1024.0 KiB`** — a size no reader should ever be shown, because the
//! whole point of the unit is that there are only 1024 of them.
//!
//! [`iec`] and [`si`] round first and promote afterwards, so the printed
//! mantissa is always below the base.
//!
//! # Choosing between [`iec`] and [`si`]
//!
//! One rule, so that the next call site does not have to re-derive it:
//! **bytes *moved over a link* are decimal; bytes *occupying storage* are
//! binary.**
//!
//! | Quantity | Use | Because |
//! |---|---|---|
//! | file sizes, memory, anything the kernel counts | [`iec`] | allocation is in powers of two, and `design.txt` writes `KiB` throughout |
//! | disk and partition capacity as the vendor sells it | [`si`] | a "4 TB" drive is 4×10¹² bytes |
//! | an interface's `rx_bytes`/`tx_bytes`, a tunnel's transfer total | [`si`] | networking has always been decimal — and four different windows report these same counters |
//! | throughput | [`si_rate`] on a link, [`iec_rate`] on a disk | a rate should match the thing it is a rate of |
//!
//! The rule is worth stating because one formatter can straddle it:
//! `apps/remotedesktop` used a single function for a transferred *file's* size
//! and for the *session's* link counters, and those are genuinely different
//! answers — the file is the same file the explorer lists.
//!
//! When in doubt use [`iec`]: it is the house convention, and it is what the
//! twenty-nine broken copies were already *computing* before they mislabelled
//! it.

use alloc::format;
use alloc::string::String;
use core::num::NonZeroU128;

/// Which family of unit prefixes to scale into.
///
/// The base and the names travel together — that pairing is the invariant
/// this module exists to enforce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// Binary prefixes: 1024 bytes to the `KiB` (IEC 80000-13).
    Iec,
    /// Decimal prefixes: 1000 bytes to the `kB` (SI).
    Si,
}

impl Unit {
    /// The scaling base, which is never zero or one.
    fn base(self) -> NonZeroU128 {
        match self {
            // SAFETY-free: both literals are non-zero constants, and
            // `NonZeroU128::new(..).unwrap_or(MIN)` cannot be reached. A `MIN`
            // fallback would silently mean "base 1", i.e. an infinite promotion
            // loop, so it is worth not spelling it that way.
            Self::Iec => match NonZeroU128::new(1024) {
                Some(b) => b,
                None => NonZeroU128::MIN,
            },
            Self::Si => match NonZeroU128::new(1000) {
                Some(b) => b,
                None => NonZeroU128::MIN,
            },
        }
    }

    /// Unit names, smallest first. Index 0 is unscaled bytes.
    ///
    /// Both lists run to the exabyte, which is past `u64::MAX` (16 EiB), so
    /// no input can exhaust them and no value is ever shown in a unit far
    /// below its magnitude. Several of the hand-written copies stopped at
    /// `GB`, which is how a 4 TiB disk came to be described in gigabytes.
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::Iec => &["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"],
            // Lowercase `k` is the SI prefix for kilo; `K` is kelvin. The
            // uppercase spelling in the old copies was one more symptom of
            // units picked by eye rather than from a table.
            Self::Si => &["B", "kB", "MB", "GB", "TB", "PB", "EB"],
        }
    }
}

/// `bytes` in binary units — `1.0 KiB` is 1024 bytes.
///
/// Exact below 1024, and one decimal place above it.
///
/// ```
/// use textfmt::bytes::iec;
/// assert_eq!(iec(0), "0 B");
/// assert_eq!(iec(1023), "1023 B");
/// assert_eq!(iec(1024), "1.0 KiB");
/// assert_eq!(iec(1536), "1.5 KiB");
/// // Rounds to the next unit rather than printing "1024.0 KiB".
/// assert_eq!(iec(1_048_575), "1.0 MiB");
/// assert_eq!(iec(u64::MAX), "16.0 EiB");
/// ```
#[must_use]
pub fn iec(bytes: u64) -> String {
    scale(bytes, Unit::Iec)
}

/// `bytes` in decimal units — `1.0 kB` is 1000 bytes.
///
/// ```
/// use textfmt::bytes::si;
/// assert_eq!(si(999), "999 B");
/// assert_eq!(si(1000), "1.0 kB");
/// assert_eq!(si(1_500_000), "1.5 MB");
/// // The same count that `iec` calls 1.0 GiB.
/// assert_eq!(si(1_073_741_824), "1.1 GB");
/// ```
#[must_use]
pub fn si(bytes: u64) -> String {
    scale(bytes, Unit::Si)
}

/// `bytes` in whichever family `unit` names, for callers that choose at run
/// time (a settings toggle, say) rather than at the call site.
#[must_use]
pub fn scale(bytes: u64, unit: Unit) -> String {
    let base = unit.base();
    let names = unit.names();
    let value = u128::from(bytes);

    // Largest unit whose threshold `bytes` reaches. All arithmetic is in
    // `u128` so that the top of the `u64` range cannot overflow the scaled
    // numerator below; `saturating_mul` is belt-and-braces for the lint.
    let mut index = 0usize;
    let mut divisor = NonZeroU128::MIN;
    while index.saturating_add(1) < names.len() {
        let next = divisor.saturating_mul(base);
        if value < next.get() {
            break;
        }
        divisor = next;
        index = index.saturating_add(1);
    }

    if index == 0 {
        // Below one unit there is nothing to round, and a fraction of a byte
        // is not a thing, so print the count exactly.
        return format!("{bytes} {}", names.first().copied().unwrap_or("B"));
    }

    // Tenths of a unit, rounded half-up. Rounding *before* choosing the final
    // name is the whole point: `tenths` may land exactly on the base, and the
    // promotion below is what stops that being printed as `1024.0 KiB`.
    let mut tenths = round_tenths(value, divisor);
    if tenths >= base.get().saturating_mul(10) && index.saturating_add(1) < names.len() {
        divisor = divisor.saturating_mul(base);
        index = index.saturating_add(1);
        tenths = round_tenths(value, divisor);
    }

    let whole = tenths / 10;
    let frac = tenths % 10;
    format!("{whole}.{frac} {}", names.get(index).copied().unwrap_or("B"))
}

/// `bytes_per_sec` in binary units, suffixed `/s` — `1.0 KiB/s` is 1024 B/s.
///
/// A throughput figure is the same defect waiting to happen as a size: the
/// three rate formatters this replaced all divided by 1024 and then wrote
/// `KB/s`. Going through the same table means a rate can never disagree with
/// the size it is a rate of.
///
/// ```
/// use textfmt::bytes::iec_rate;
/// assert_eq!(iec_rate(512), "512 B/s");
/// assert_eq!(iec_rate(2560), "2.5 KiB/s");
/// ```
#[must_use]
pub fn iec_rate(bytes_per_sec: u64) -> String {
    rate(bytes_per_sec, Unit::Iec)
}

/// `bytes_per_sec` in decimal units, suffixed `/s` — `1.0 kB/s` is 1000 B/s.
///
/// This is the one to reach for on a network link; see design-decisions.md
/// §489 for why link counters are decimal and stored bytes are binary.
///
/// ```
/// use textfmt::bytes::si_rate;
/// assert_eq!(si_rate(1_500_000), "1.5 MB/s");
/// ```
#[must_use]
pub fn si_rate(bytes_per_sec: u64) -> String {
    rate(bytes_per_sec, Unit::Si)
}

/// `bytes_per_sec` in whichever family `unit` names, suffixed `/s`.
#[must_use]
pub fn rate(bytes_per_sec: u64, unit: Unit) -> String {
    let mut text = scale(bytes_per_sec, unit);
    text.push_str("/s");
    text
}

/// `value / divisor`, in tenths, rounded half-up.
fn round_tenths(value: u128, divisor: NonZeroU128) -> u128 {
    let numerator = value.saturating_mul(10).saturating_add(divisor.get() / 2);
    numerator / divisor
}

#[cfg(test)]
// `expect` rather than the `allow` the sibling modules use: it reports itself
// as unfulfilled if the code stops needing it, so the permission cannot outlive
// its reason.
#[expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test code: panicking on bad data is the assertion"
)]
mod tests {
    use super::{Unit, iec, scale, si};
    use alloc::format;
    use alloc::string::String;

    #[test]
    fn bytes_below_the_first_prefix_are_printed_exactly() {
        for n in [0u64, 1, 2, 512, 999, 1000, 1023] {
            assert_eq!(iec(n), format!("{n} B"), "iec({n})");
        }
        for n in [0u64, 1, 2, 512, 999] {
            assert_eq!(si(n), format!("{n} B"), "si({n})");
        }
        // The one place the two families disagree about whether to scale.
        assert_eq!(iec(1000), "1000 B");
        assert_eq!(si(1000), "1.0 kB");
        assert_eq!(iec(1024), "1.0 KiB");
        assert_eq!(si(1024), "1.0 kB");
    }

    #[test]
    fn the_mantissa_is_always_below_the_base() {
        // This is the property the hand-written copies broke: they chose the
        // unit from the unrounded count, so the rounded mantissa could reach
        // the base and still wear the smaller unit's name.
        for unit in [Unit::Iec, Unit::Si] {
            let base = unit.base().get();
            let mut divisor: u128 = 1;
            // One byte below each unit boundary, where the old code printed
            // e.g. "1024.0 KiB" / "1000.0 kB".
            for _ in 0..6u32 {
                divisor *= base;
                let Ok(just_under) = u64::try_from(divisor - 1) else {
                    break;
                };
                let s = scale(just_under, unit);
                let mantissa: f64 = s
                    .split(' ')
                    .next()
                    .unwrap()
                    .parse()
                    .expect("mantissa parses");
                #[expect(clippy::cast_precision_loss, reason = "base is 1000 or 1024")]
                let base_f = base as f64;
                assert!(
                    mantissa < base_f,
                    "scale({just_under}, {unit:?}) = {s}: mantissa reached the base"
                );
            }
        }
    }

    #[test]
    fn one_below_a_boundary_promotes_instead_of_printing_a_full_base() {
        // The concrete regression, spelled out. Every one of these printed
        // "<base>.0 <smaller unit>" before this module existed.
        assert_eq!(iec(1_048_575), "1.0 MiB");
        assert_eq!(iec(1_073_741_823), "1.0 GiB");
        assert_eq!(iec(1_099_511_627_775), "1.0 TiB");
        assert_eq!(si(999_999), "1.0 MB");
        assert_eq!(si(999_999_999), "1.0 GB");
    }

    #[test]
    fn the_divisor_and_the_name_always_agree() {
        // The defect that motivated the module: 1024-based arithmetic wearing
        // SI names. Reconstruct the byte count from what was printed and
        // require it back within half a unit of the input.
        for unit in [Unit::Iec, Unit::Si] {
            let base = unit.base().get();
            for &n in &[
                1024u64,
                1000,
                1_048_576,
                1_000_000,
                1_500_000,
                1_073_741_824,
                4_398_046_511_104,
                u64::MAX,
            ] {
                let s = scale(n, unit);
                let (num, name) = s.split_once(' ').unwrap();
                let idx = unit
                    .names()
                    .iter()
                    .position(|u| *u == name)
                    .expect("printed name is one of this unit family's own");
                let mut divisor: u128 = 1;
                for _ in 0..idx {
                    divisor *= base;
                }
                let mantissa: f64 = num.parse().unwrap();
                #[expect(clippy::cast_precision_loss, reason = "display-scale check")]
                let reconstructed = mantissa * divisor as f64;
                #[expect(clippy::cast_precision_loss, reason = "display-scale check")]
                let actual = n as f64;
                // One decimal place, so half a tenth of a unit of slack.
                #[expect(clippy::cast_precision_loss, reason = "display-scale check")]
                let tolerance = (divisor as f64) / 20.0;
                assert!(
                    (reconstructed - actual).abs() <= tolerance,
                    "scale({n}, {unit:?}) = {s} reconstructs to {reconstructed}, not {actual}"
                );
            }
        }
    }

    #[test]
    fn nothing_saturates_at_the_top_of_the_range() {
        // Several of the old copies stopped at GB or TB, so a large disk was
        // described in thousands of gigabytes.
        assert_eq!(iec(u64::MAX), "16.0 EiB");
        assert_eq!(si(u64::MAX), "18.4 EB");
        assert_eq!(iec(4 * 1024 * 1024 * 1024 * 1024), "4.0 TiB");
        assert_eq!(si(4_000_000_000_000), "4.0 TB");
        // The vendor's "4 TB" disk, in both families.
        assert_eq!(iec(4_000_000_000_000), "3.6 TiB");
    }

    #[test]
    fn every_output_parses_back_into_a_number_and_a_known_unit() {
        for unit in [Unit::Iec, Unit::Si] {
            let names = unit.names();
            let mut n: u64 = 0;
            loop {
                let s = scale(n, unit);
                let (num, name) = s.split_once(' ').expect("one space, always");
                assert!(names.contains(&name), "{s}: unknown unit");
                assert!(
                    num.parse::<f64>().is_ok(),
                    "{s}: mantissa is not a number"
                );
                // Exactly one decimal place above the first prefix, none below.
                let expected_dot = usize::from(name != "B");
                assert_eq!(
                    num.matches('.').count(),
                    expected_dot,
                    "{s}: wrong decimal shape"
                );
                let Some(next) = n.checked_mul(3).and_then(|v| v.checked_add(7)) else {
                    break;
                };
                n = next;
            }
            // And the very top.
            let s: String = scale(u64::MAX, unit);
            assert!(names.iter().any(|u| s.ends_with(u)));
        }
    }
}
