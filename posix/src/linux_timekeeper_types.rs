//! `<linux/timekeeper_internal.h>` — timekeeper clock type constants.
//!
//! The timekeeper is the kernel's central timekeeping engine. It
//! maintains the relationship between the hardware counter (TSC,
//! HPET, etc.) and wall-clock / monotonic / boot time. It handles
//! NTP adjustments, leap seconds, and suspended-time accounting.

// ---------------------------------------------------------------------------
// Timekeeper clock bases (tk_base)
// ---------------------------------------------------------------------------

/// Monotonic clock base (never goes backward).
pub const TK_BASE_MONO: u32 = 0;
/// Raw monotonic (no NTP adjustment).
pub const TK_BASE_RAW: u32 = 1;
/// Boot time (monotonic + time spent suspended).
pub const TK_BASE_BOOT: u32 = 2;
/// TAI (International Atomic Time, monotonic + leap seconds).
pub const TK_BASE_TAI: u32 = 3;
/// Number of clock bases.
pub const TK_BASE_MAX: u32 = 4;

// ---------------------------------------------------------------------------
// Timekeeper update flags
// ---------------------------------------------------------------------------

/// Timekeeper has been updated (readers should re-read).
pub const TK_UPDATE_NORMAL: u32 = 0;
/// Timekeeper update includes clock change.
pub const TK_UPDATE_CLOCK_CHANGE: u32 = 1;
/// Timekeeper is in NTP error correction.
pub const TK_UPDATE_NTP_ERR: u32 = 2;

// ---------------------------------------------------------------------------
// VDSO clock modes
// ---------------------------------------------------------------------------

/// Clock is not usable from VDSO.
pub const VDSO_CLOCKMODE_NONE: u32 = 0;
/// Use TSC for VDSO (fastest path).
pub const VDSO_CLOCKMODE_TSC: u32 = 1;
/// Use pvclock for VDSO (paravirtualised).
pub const VDSO_CLOCKMODE_PVCLOCK: u32 = 2;
/// Use HVCLOCK for VDSO (Hyper-V).
pub const VDSO_CLOCKMODE_HVCLOCK: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bases_distinct() {
        let bases = [TK_BASE_MONO, TK_BASE_RAW, TK_BASE_BOOT, TK_BASE_TAI];
        for i in 0..bases.len() {
            for j in (i + 1)..bases.len() {
                assert_ne!(bases[i], bases[j]);
            }
        }
    }

    #[test]
    fn test_bases_sequential() {
        assert_eq!(TK_BASE_MONO, 0);
        assert_eq!(TK_BASE_RAW, 1);
        assert_eq!(TK_BASE_BOOT, 2);
        assert_eq!(TK_BASE_TAI, 3);
    }

    #[test]
    fn test_base_max() {
        assert_eq!(TK_BASE_MAX, TK_BASE_TAI + 1);
    }

    #[test]
    fn test_update_flags_distinct() {
        assert_ne!(TK_UPDATE_NORMAL, TK_UPDATE_CLOCK_CHANGE);
        assert_ne!(TK_UPDATE_CLOCK_CHANGE, TK_UPDATE_NTP_ERR);
    }

    #[test]
    fn test_vdso_modes_distinct() {
        let modes = [
            VDSO_CLOCKMODE_NONE,
            VDSO_CLOCKMODE_TSC,
            VDSO_CLOCKMODE_PVCLOCK,
            VDSO_CLOCKMODE_HVCLOCK,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }
}
