//! One spelling of "1.5 MiB", because the kernel had twenty-one.
//!
//! ## Why this exists
//!
//! Every module that reports a byte count to a human had grown its own
//! `format_size` / `format_bytes`, all private, all slightly different. They
//! disagreed on the unit names, on how many digits follow the point, on
//! whether there is a point at all, and — in six of them — on the arithmetic.
//!
//! That is not a tidiness complaint. Two of the copies were **wrong**, in the
//! same way, and one of them was found only because a self-test finally ran
//! against it at boot (`known-issues.md` →
//! `TD-A-FORMAT-SIZE-PRINTED-A-TWO-DIGIT-TENTHS`). The tenths digit was
//! computed as `remainder / (unit / 10)`, which is the obvious thing to write
//! and is off by one whole digit at the top of every unit: `1023 / 100` is
//! `10`, so a size just under 2 KiB printed as `1.10 KiB` — two digits after
//! the point, reading as *larger* than the `1.9 KiB` below it. The same
//! divisor, `107_374_183`-style, appeared in six files. Fixing it in one of
//! them would have left the other five to be rediscovered one at a time.
//!
//! Six others labelled 1024-based units `GB`/`MB`/`KB`, which overstates by
//! about 7% per unit and is the exact confusion the IEC prefixes exist to
//! prevent. The disk-cleanup tool and the disk-usage tool could report the
//! same directory with different numbers *and* different units.
//!
//! ## The arithmetic
//!
//! Integer throughout, and no `f64`. Several of the replaced copies did
//! `bytes as f64 / 1024.0` with `{:.1}`, which is both heavier than it looks
//! in a kernel and silently lossy above 2^53 — reachable on a `u64` byte
//! count. `remainder * 10 / unit` scales before dividing and so yields a digit
//! in `0..=9` by construction rather than by choosing the divisor carefully.
//!
//! ## Deliberately three functions, and no more
//!
//! [`iec`] is the prose form (`1.5 MiB`) used in reports and status lines, and
//! is what almost everything should call. [`compact`] is the `ls -h` column
//! form (`1.5M`) — no space, one letter, so it fits a fixed-width field, and
//! `ls -h`, `df` and `du` must agree with each other. [`iec2`] is [`iec`] with
//! two digits, for throughput reports where a tenth of a GiB is 107 MB of
//! slack.
//!
//! Those three differences are real. Every *other* difference between the
//! twenty-one copies was accident, and a fourth entry point should have to
//! argue for itself as hard as these did.

use alloc::format;
use alloc::string::String;

/// IEC binary unit names, ascending. Indexed by power of 1024.
const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

/// Split `bytes` into whole units, a fraction in `0..scale`, and the index
/// into [`UNITS`] of the unit chosen.
///
/// The unit is the largest one that leaves a whole part below 1024, so the
/// output is always one to four characters before the point. `scale` is the
/// power of ten the fraction is expressed in — `10` for one digit after the
/// point, `100` for two — and the returned fraction is in `0..scale` by
/// construction, which is the whole point of scaling before dividing.
fn split(bytes: u64, scale: u64) -> (u64, u64, usize) {
    let mut idx = 0usize;
    let mut unit = 1u64;
    while idx.saturating_add(1) < UNITS.len() && bytes / unit >= 1024 {
        unit = unit.saturating_mul(1024);
        idx = idx.saturating_add(1);
    }
    let whole = bytes / unit;
    let frac = if idx == 0 {
        0
    } else {
        // Widened because `remainder * 100` does not fit a `u64` at the top of
        // the table: `unit` reaches 1024^6 = 2^60 and the remainder can be
        // `unit - 1`, so the product needs 67 bits. `remainder * 10` does fit,
        // but branching on the scale to save one widened divide in a *string
        // formatter* would be trading the property that makes this function
        // correct — one path, no size-dependent cases — for nothing.
        let scaled = u128::from(bytes % unit).saturating_mul(u128::from(scale));
        (scaled / u128::from(unit)) as u64
    };
    (whole, frac, idx)
}

/// Format `bytes` as a human-readable IEC size: `512 B`, `1.5 KiB`, `2.0 GiB`.
///
/// Below 1 KiB the count is exact and is printed in bytes with no decimal
/// point — there is no `0.5 KiB`, because the byte count is not an
/// approximation at that scale and rounding it would lose information for no
/// gain in readability.
pub fn iec(bytes: u64) -> String {
    let (whole, tenths, idx) = split(bytes, 10);
    if idx == 0 {
        return format!("{} B", whole);
    }
    let unit = UNITS.get(idx).copied().unwrap_or("B");
    format!("{}.{} {}", whole, tenths, unit)
}

/// [`iec`] with two digits after the point: `512 B`, `1.50 KiB`, `2.00 GiB`.
///
/// For throughput and transfer reports, where one digit is too coarse to be
/// useful: at GiB scale a tenth is 107 MB, so `iperf`'s "1.2 GiB transferred"
/// spans a range wider than most of the transfers anyone measures. Everything
/// else — including every size a *file* is reported in — should use [`iec`];
/// two digits in a size column is noise, not precision.
pub fn iec2(bytes: u64) -> String {
    let (whole, hundredths, idx) = split(bytes, 100);
    if idx == 0 {
        return format!("{} B", whole);
    }
    let unit = UNITS.get(idx).copied().unwrap_or("B");
    format!("{}.{:02} {}", whole, hundredths, unit)
}

/// Format `bytes` for a fixed-width column, `ls -h` style: `512`, `1.5K`,
/// `2.0G`.
///
/// One letter, no space, and a bare number below 1 KiB. This is a different
/// format on purpose and not a shorter spelling of [`iec`]: it has to fit a
/// size column, where `1.5 KiB` would not align with `512 B`.
pub fn compact(bytes: u64) -> String {
    let (whole, tenths, idx) = split(bytes, 10);
    if idx == 0 {
        return format!("{}", whole);
    }
    // The first byte of the IEC name is the conventional single-letter suffix
    // for every unit above bytes: KiB -> K, MiB -> M, and so on.
    let letter = UNITS
        .get(idx)
        .and_then(|u| u.as_bytes().first().copied())
        .unwrap_or(b'B') as char;
    format!("{}.{}{}", whole, tenths, letter)
}

/// Self-test for the byte formatters.
///
/// Pure functions with no state, so there is nothing to make pristine and no
/// `crate::fs::selftest` wrapper: every case here is a literal in and a
/// literal out.
pub fn self_test() {
    crate::serial_println!("[bytesize] Running self-tests...");

    // Below a KiB the count is exact and stays in bytes.
    assert_eq!(iec(0), "0 B");
    assert_eq!(iec(1), "1 B");
    assert_eq!(iec(512), "512 B");
    assert_eq!(iec(1023), "1023 B");

    assert_eq!(iec(1024), "1.0 KiB");
    assert_eq!(iec(1536), "1.5 KiB");

    // The case the replaced copies got wrong: at the top of a unit the tenths
    // digit must stay a single digit. `remainder / (unit / 10)` gives 10 here
    // and printed "1.10 KiB".
    assert_eq!(iec(2047), "1.9 KiB");
    assert_eq!(iec(1024 * 1024 - 1), "1023.9 KiB");
    assert_eq!(iec(1024 * 1024 * 1024 - 1), "1023.9 MiB");
    assert_eq!(iec(1024 * 1024 * 1024 * 1024 - 1), "1023.9 GiB");

    assert_eq!(iec(1024 * 1024), "1.0 MiB");
    assert_eq!(iec(1024 * 1024 * 1024), "1.0 GiB");
    assert_eq!(iec(1024 * 1024 * 1024 * 1024), "1.0 TiB");
    assert_eq!(iec(1024 * 1024 * 1024 * 1024 * 1024), "1.0 PiB");
    assert_eq!(iec(1024 * 1024 * 1024 * 1024 * 1024 * 1024), "1.0 EiB");

    // The largest `u64` must not run off the end of the unit table, which is
    // what the `idx + 1 < UNITS.len()` guard in `split` is for.
    assert_eq!(iec(u64::MAX), "15.9 EiB");

    // Truncation, not rounding: 1.99 KiB is "1.9 KiB", never "2.0 KiB". A
    // rounded size that reads as the next unit up is worse than a slightly
    // small one, especially for a disk-space report.
    assert_eq!(iec(2038), "1.9 KiB");

    crate::serial_println!("[bytesize]   1. iec: OK");

    // Two digits after the point. The leading zero matters: without `{:02}`,
    // 1025 bytes would print as "1.0 KiB" (fraction 0) and 1126 as "1.9 KiB"
    // (fraction 9) — the same two-digit-vs-one-digit ambiguity the tenths bug
    // produced, arrived at from the other direction.
    assert_eq!(iec2(0), "0 B");
    assert_eq!(iec2(1023), "1023 B");
    assert_eq!(iec2(1024), "1.00 KiB");
    assert_eq!(iec2(1025), "1.00 KiB");
    assert_eq!(iec2(1536), "1.50 KiB");
    assert_eq!(iec2(2047), "1.99 KiB");
    assert_eq!(iec2(1024 * 1024 - 1), "1023.99 KiB");
    assert_eq!(iec2(1024 * 1024), "1.00 MiB");
    // The case the `u64` arithmetic could not do: `(2^60 - 1) * 100` needs 67
    // bits, so `split` widens. A narrow multiply would wrap and print a
    // fraction unrelated to the size.
    assert_eq!(iec2(u64::MAX), "15.99 EiB");
    crate::serial_println!("[bytesize]   2. iec2: OK");

    assert_eq!(compact(0), "0");
    assert_eq!(compact(512), "512");
    assert_eq!(compact(1024), "1.0K");
    assert_eq!(compact(1536), "1.5K");
    assert_eq!(compact(1024 * 1024), "1.0M");
    assert_eq!(compact(1024 * 1024 * 1024), "1.0G");
    assert_eq!(compact(1024u64 * 1024 * 1024 * 1024), "1.0T");
    assert_eq!(compact(u64::MAX), "15.9E");
    crate::serial_println!("[bytesize]   3. compact: OK");

    crate::serial_println!("[bytesize] All 3 self-tests passed.");
}
