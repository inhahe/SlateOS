//! `<linux/timecounter.h>` — hardware time counter constants.
//!
//! Time counters are the abstraction layer between raw hardware
//! cycle counters (TSC, HPET, ACPI PM timer) and the kernel's
//! timekeeping. Each counter has a frequency, a bit width, and a
//! quality rating. The kernel selects the best available counter
//! as the system clocksource.

// ---------------------------------------------------------------------------
// Clocksource ratings (quality tiers)
// ---------------------------------------------------------------------------

/// Unusable (unstable or broken).
pub const CLOCKSOURCE_RATING_UNUSABLE: u32 = 0;
/// Very low quality (only use as last resort).
pub const CLOCKSOURCE_RATING_LOW: u32 = 100;
/// Acceptable (functional but not ideal).
pub const CLOCKSOURCE_RATING_ACCEPTABLE: u32 = 200;
/// Good (reliable, reasonable resolution).
pub const CLOCKSOURCE_RATING_GOOD: u32 = 300;
/// Perfect / ideal (TSC on modern x86, arch timer on ARM).
pub const CLOCKSOURCE_RATING_PERFECT: u32 = 400;

// ---------------------------------------------------------------------------
// Common clocksource masks (counter bit widths)
// ---------------------------------------------------------------------------

/// 24-bit counter mask (ACPI PM timer).
pub const CLOCKSOURCE_MASK_24: u64 = (1u64 << 24) - 1;
/// 32-bit counter mask (HPET, some arch timers).
pub const CLOCKSOURCE_MASK_32: u64 = (1u64 << 32) - 1;
/// 48-bit counter mask (some ARM arch timers).
pub const CLOCKSOURCE_MASK_48: u64 = (1u64 << 48) - 1;
/// 56-bit counter mask.
pub const CLOCKSOURCE_MASK_56: u64 = (1u64 << 56) - 1;
/// 64-bit counter mask (TSC, full-width).
pub const CLOCKSOURCE_MASK_64: u64 = u64::MAX;

// ---------------------------------------------------------------------------
// Clocksource flags
// ---------------------------------------------------------------------------

/// Clocksource is continuous (doesn't stop in idle).
pub const CLOCKSOURCE_FLAG_CONTINUOUS: u32 = 0x01;
/// Clocksource may be unstable (needs watchdog).
pub const CLOCKSOURCE_FLAG_UNSTABLE: u32 = 0x02;
/// Clocksource is suspend-aware.
pub const CLOCKSOURCE_FLAG_SUSPEND_NONSTOP: u32 = 0x04;
/// Clocksource is VDSO-capable.
pub const CLOCKSOURCE_FLAG_VDSO: u32 = 0x08;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ratings_ordered() {
        assert!(CLOCKSOURCE_RATING_UNUSABLE < CLOCKSOURCE_RATING_LOW);
        assert!(CLOCKSOURCE_RATING_LOW < CLOCKSOURCE_RATING_ACCEPTABLE);
        assert!(CLOCKSOURCE_RATING_ACCEPTABLE < CLOCKSOURCE_RATING_GOOD);
        assert!(CLOCKSOURCE_RATING_GOOD < CLOCKSOURCE_RATING_PERFECT);
    }

    #[test]
    fn test_masks_ordered() {
        assert!(CLOCKSOURCE_MASK_24 < CLOCKSOURCE_MASK_32);
        assert!(CLOCKSOURCE_MASK_32 < CLOCKSOURCE_MASK_48);
        assert!(CLOCKSOURCE_MASK_48 < CLOCKSOURCE_MASK_56);
        assert!(CLOCKSOURCE_MASK_56 < CLOCKSOURCE_MASK_64);
    }

    #[test]
    fn test_masks_correct() {
        assert_eq!(CLOCKSOURCE_MASK_24, 0x00FF_FFFF);
        assert_eq!(CLOCKSOURCE_MASK_32, 0xFFFF_FFFF);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CLOCKSOURCE_FLAG_CONTINUOUS, CLOCKSOURCE_FLAG_UNSTABLE,
            CLOCKSOURCE_FLAG_SUSPEND_NONSTOP, CLOCKSOURCE_FLAG_VDSO,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
