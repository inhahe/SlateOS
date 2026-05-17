//! `<linux/clocksource.h>` — Clock source subsystem constants.
//!
//! A clocksource provides the kernel with a monotonically increasing
//! counter to measure time. The kernel selects the best available
//! clocksource based on rating (stability, resolution). Common sources:
//! TSC (fast but may be unstable), HPET (reliable but slow to read),
//! ACPI PM timer (very stable but low resolution). The selected source
//! drives timekeeping, CLOCK_MONOTONIC, and the vDSO clock.

// ---------------------------------------------------------------------------
// Clock source ratings (higher = preferred)
// ---------------------------------------------------------------------------

/// Perfect clocksource (e.g., hardware with certified stability).
pub const CLOCKSOURCE_RATING_PERFECT: u32 = 400;
/// Good clocksource (e.g., TSC when stable).
pub const CLOCKSOURCE_RATING_GOOD: u32 = 300;
/// Usable clocksource (e.g., HPET).
pub const CLOCKSOURCE_RATING_USABLE: u32 = 200;
/// Marginal clocksource (e.g., PIT).
pub const CLOCKSOURCE_RATING_MARGINAL: u32 = 100;
/// Dummy clocksource (jiffies-based fallback).
pub const CLOCKSOURCE_RATING_DUMMY: u32 = 0;

// ---------------------------------------------------------------------------
// Clock source flags
// ---------------------------------------------------------------------------

/// Clocksource can be used in vDSO (fast userspace reads).
pub const CLOCKSOURCE_VDSO_CAPABLE: u32 = 0x0001;
/// Clocksource is continuous (doesn't stop in suspend).
pub const CLOCKSOURCE_CONTINUOUS: u32 = 0x0002;
/// Clocksource has been verified stable.
pub const CLOCKSOURCE_STABLE: u32 = 0x0004;
/// Clocksource is currently selected as the system clock.
pub const CLOCKSOURCE_SELECTED: u32 = 0x0008;
/// Clocksource is being watched for instability.
pub const CLOCKSOURCE_WATCHDOG: u32 = 0x0010;
/// Clocksource has been marked unstable.
pub const CLOCKSOURCE_UNSTABLE: u32 = 0x0020;
/// Clocksource supports suspend/resume.
pub const CLOCKSOURCE_SUSPEND_OK: u32 = 0x0040;

// ---------------------------------------------------------------------------
// Clock source types (common hardware)
// ---------------------------------------------------------------------------

/// x86 Time Stamp Counter.
pub const CLOCKSOURCE_TYPE_TSC: u32 = 0;
/// High Precision Event Timer.
pub const CLOCKSOURCE_TYPE_HPET: u32 = 1;
/// ACPI Power Management Timer.
pub const CLOCKSOURCE_TYPE_ACPI_PM: u32 = 2;
/// Programmable Interval Timer (legacy).
pub const CLOCKSOURCE_TYPE_PIT: u32 = 3;
/// ARM architecture timer.
pub const CLOCKSOURCE_TYPE_ARCH_TIMER: u32 = 4;
/// Jiffies-based fallback.
pub const CLOCKSOURCE_TYPE_JIFFIES: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ratings_ordered() {
        assert!(CLOCKSOURCE_RATING_DUMMY < CLOCKSOURCE_RATING_MARGINAL);
        assert!(CLOCKSOURCE_RATING_MARGINAL < CLOCKSOURCE_RATING_USABLE);
        assert!(CLOCKSOURCE_RATING_USABLE < CLOCKSOURCE_RATING_GOOD);
        assert!(CLOCKSOURCE_RATING_GOOD < CLOCKSOURCE_RATING_PERFECT);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CLOCKSOURCE_VDSO_CAPABLE, CLOCKSOURCE_CONTINUOUS,
            CLOCKSOURCE_STABLE, CLOCKSOURCE_SELECTED,
            CLOCKSOURCE_WATCHDOG, CLOCKSOURCE_UNSTABLE,
            CLOCKSOURCE_SUSPEND_OK,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_types_distinct() {
        let types = [
            CLOCKSOURCE_TYPE_TSC, CLOCKSOURCE_TYPE_HPET,
            CLOCKSOURCE_TYPE_ACPI_PM, CLOCKSOURCE_TYPE_PIT,
            CLOCKSOURCE_TYPE_ARCH_TIMER, CLOCKSOURCE_TYPE_JIFFIES,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }
}
