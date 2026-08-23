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
//! ## Deliberately two functions
//!
//! [`iec`] is the prose form (`1.5 MiB`) used in reports and status lines.
//! [`compact`] is the `ls -h` column form (`1.5M`) — no space, one letter, so
//! it fits a fixed-width field. That difference is real and the shell relies
//! on it; everything else was accident.

use alloc::format;
use alloc::string::String;

/// IEC binary unit names, ascending. Indexed by power of 1024.
const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];

/// Split `bytes` into whole units, a tenths digit in `0..=9`, and the index
/// into [`UNITS`] of the unit chosen.
///
/// The unit is the largest one that leaves a whole part below 1024, so the
/// output is always one to four characters before the point.
fn split(bytes: u64) -> (u64, u64, usize) {
    let mut idx = 0usize;
    let mut unit = 1u64;
    // `unit` tops out at 1024^6 = 2^60, so neither the multiply nor the
    // `* 10` below can overflow a u64 (2^60 * 10 < u64::MAX).
    while idx.saturating_add(1) < UNITS.len() && bytes / unit >= 1024 {
        unit = unit.saturating_mul(1024);
        idx = idx.saturating_add(1);
    }
    let whole = bytes / unit;
    let tenths = if idx == 0 {
        0
    } else {
        (bytes % unit).saturating_mul(10) / unit
    };
    (whole, tenths, idx)
}

/// Format `bytes` as a human-readable IEC size: `512 B`, `1.5 KiB`, `2.0 GiB`.
///
/// Below 1 KiB the count is exact and is printed in bytes with no decimal
/// point — there is no `0.5 KiB`, because the byte count is not an
/// approximation at that scale and rounding it would lose information for no
/// gain in readability.
pub fn iec(bytes: u64) -> String {
    let (whole, tenths, idx) = split(bytes);
    if idx == 0 {
        return format!("{} B", whole);
    }
    let unit = UNITS.get(idx).copied().unwrap_or("B");
    format!("{}.{} {}", whole, tenths, unit)
}

/// Format `bytes` for a fixed-width column, `ls -h` style: `512`, `1.5K`,
/// `2.0G`.
///
/// One letter, no space, and a bare number below 1 KiB. This is a different
/// format on purpose and not a shorter spelling of [`iec`]: it has to fit a
/// size column, where `1.5 KiB` would not align with `512 B`.
pub fn compact(bytes: u64) -> String {
    let (whole, tenths, idx) = split(bytes);
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

    assert_eq!(compact(0), "0");
    assert_eq!(compact(512), "512");
    assert_eq!(compact(1024), "1.0K");
    assert_eq!(compact(1536), "1.5K");
    assert_eq!(compact(1024 * 1024), "1.0M");
    assert_eq!(compact(1024 * 1024 * 1024), "1.0G");
    assert_eq!(compact(1024u64 * 1024 * 1024 * 1024), "1.0T");
    assert_eq!(compact(u64::MAX), "15.9E");
    crate::serial_println!("[bytesize]   2. compact: OK");

    crate::serial_println!("[bytesize] All 2 self-tests passed.");
}
