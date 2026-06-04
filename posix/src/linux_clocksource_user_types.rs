//! `<linux/clocksource.h>` — clocksource flags and quality ratings.
//!
//! A clocksource is a free-running monotonic counter (TSC, HPET, ARM
//! arch timer) that the kernel polls to compute wall-clock and
//! monotonic time. Each registered clocksource has a quality rating
//! used to select the best one, plus flags describing its properties.

// ---------------------------------------------------------------------------
// Clocksource flags
// ---------------------------------------------------------------------------

/// Source is continuous (no wrap or jumps).
pub const CLOCK_SOURCE_IS_CONTINUOUS: u32 = 0x01;
/// Must be verified periodically against watchdog.
pub const CLOCK_SOURCE_MUST_VERIFY: u32 = 0x02;
/// Source is suitable for sub-jiffy reads (hot-path use).
pub const CLOCK_SOURCE_UNSTABLE: u32 = 0x04;
/// Suspended sources stay valid across suspend.
pub const CLOCK_SOURCE_SUSPEND_NONSTOP: u32 = 0x08;
/// Reserved for watchdog detection.
pub const CLOCK_SOURCE_RESELECT: u32 = 0x10;
/// Source is the watchdog itself (verifies others).
pub const CLOCK_SOURCE_VERIFY_PERCPU: u32 = 0x20;
/// Source is validated by a higher-rated clocksource.
pub const CLOCK_SOURCE_VALID_FOR_HRES: u32 = 0x40;

// ---------------------------------------------------------------------------
// Standard quality ratings
// ---------------------------------------------------------------------------

/// Default dummy clocksource (jiffies).
pub const CLOCKSOURCE_RATING_JIFFIES: u32 = 1;
/// Refinement of jiffies (jiffies + tick).
pub const CLOCKSOURCE_RATING_REFINED_JIFFIES: u32 = 2;
/// Old PIT (rarely the best choice).
pub const CLOCKSOURCE_RATING_PIT: u32 = 110;
/// HPET (high-precision event timer).
pub const CLOCKSOURCE_RATING_HPET: u32 = 250;
/// ACPI PM timer.
pub const CLOCKSOURCE_RATING_ACPI_PM: u32 = 200;
/// x86 TSC (stable, invariant TSC on modern CPUs).
pub const CLOCKSOURCE_RATING_TSC: u32 = 300;
/// ARM architected timer (CNTVCT_EL0).
pub const CLOCKSOURCE_RATING_ARM_ARCH: u32 = 450;

// ---------------------------------------------------------------------------
// Shift bounds used in mult/shift representation
// ---------------------------------------------------------------------------

/// Minimum shift used for clocksource mult/shift.
pub const CLOCKSOURCE_SHIFT_MIN: u32 = 0;
/// Maximum shift (limited by u64 arithmetic).
pub const CLOCKSOURCE_SHIFT_MAX: u32 = 32;

// ---------------------------------------------------------------------------
// Watchdog interval (nanoseconds)
// ---------------------------------------------------------------------------

/// Default watchdog polling interval (0.5 seconds).
pub const CLOCKSOURCE_WATCHDOG_INTERVAL_NS: u64 = 500_000_000;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flags_distinct_single_bit() {
        let f = [
            CLOCK_SOURCE_IS_CONTINUOUS,
            CLOCK_SOURCE_MUST_VERIFY,
            CLOCK_SOURCE_UNSTABLE,
            CLOCK_SOURCE_SUSPEND_NONSTOP,
            CLOCK_SOURCE_RESELECT,
            CLOCK_SOURCE_VERIFY_PERCPU,
            CLOCK_SOURCE_VALID_FOR_HRES,
        ];
        for (i, &x) in f.iter().enumerate() {
            assert!(x.is_power_of_two());
            for &y in &f[i + 1..] {
                assert_eq!(x & y, 0);
            }
        }
    }

    #[test]
    fn test_rating_jiffies_lowest() {
        // jiffies is the fallback — every other source must rank above it.
        assert!(CLOCKSOURCE_RATING_JIFFIES < CLOCKSOURCE_RATING_REFINED_JIFFIES);
        for r in [
            CLOCKSOURCE_RATING_PIT,
            CLOCKSOURCE_RATING_HPET,
            CLOCKSOURCE_RATING_ACPI_PM,
            CLOCKSOURCE_RATING_TSC,
            CLOCKSOURCE_RATING_ARM_ARCH,
        ] {
            assert!(r > CLOCKSOURCE_RATING_JIFFIES);
        }
    }

    #[test]
    fn test_rating_arm_arch_beats_tsc_beats_hpet() {
        // ARM architected timer is most stable; TSC second; HPET third.
        assert!(CLOCKSOURCE_RATING_HPET < CLOCKSOURCE_RATING_TSC);
        assert!(CLOCKSOURCE_RATING_TSC < CLOCKSOURCE_RATING_ARM_ARCH);
        // ACPI PM is between PIT and HPET.
        assert!(CLOCKSOURCE_RATING_PIT < CLOCKSOURCE_RATING_ACPI_PM);
        assert!(CLOCKSOURCE_RATING_ACPI_PM < CLOCKSOURCE_RATING_HPET);
    }

    #[test]
    fn test_shift_range_in_u64() {
        assert_eq!(CLOCKSOURCE_SHIFT_MIN, 0);
        assert_eq!(CLOCKSOURCE_SHIFT_MAX, 32);
        // shift ≤ 32 keeps (mult << shift) safe in u64 for sub-second deltas.
        assert!(CLOCKSOURCE_SHIFT_MAX <= 63);
    }

    #[test]
    fn test_watchdog_interval_is_half_second() {
        assert_eq!(CLOCKSOURCE_WATCHDOG_INTERVAL_NS, 500_000_000);
        // Half a second.
        assert_eq!(
            CLOCKSOURCE_WATCHDOG_INTERVAL_NS * 2,
            1_000_000_000,
        );
    }
}
