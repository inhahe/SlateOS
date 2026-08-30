//! `<linux/input-event-codes.h>` (REP subset) — auto-repeat parameter codes.
//!
//! Auto-repeat events control keyboard repeat behaviour: how long
//! to wait before repeating starts (delay) and how fast keys repeat
//! once started (period). These parameters are set per-device via
//! the `EVIOCSREP` ioctl and reported via `EV_REP` events.

// ---------------------------------------------------------------------------
// Auto-repeat parameter codes
// ---------------------------------------------------------------------------

/// Delay before repeat starts (milliseconds).
pub const REP_DELAY: u16 = 0x00;
/// Period between repeats (milliseconds).
pub const REP_PERIOD: u16 = 0x01;

// ---------------------------------------------------------------------------
// Typical default values (informational)
// ---------------------------------------------------------------------------

/// Typical default repeat delay (250 ms).
pub const REP_DEFAULT_DELAY: u32 = 250;
/// Typical default repeat period (33 ms ≈ 30 repeats/sec).
pub const REP_DEFAULT_PERIOD: u32 = 33;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum repeat code.
pub const REP_MAX: u16 = 0x01;
/// Number of repeat codes (REP_MAX + 1).
pub const REP_CNT: u16 = 0x02;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rep_codes_distinct() {
        assert_ne!(REP_DELAY, REP_PERIOD);
    }

    #[test]
    fn test_rep_values() {
        assert_eq!(REP_DELAY, 0);
        assert_eq!(REP_PERIOD, 1);
    }

    #[test]
    fn test_defaults_reasonable() {
        // Delay should be > period
        assert!(REP_DEFAULT_DELAY > REP_DEFAULT_PERIOD);
        // Both should be positive
        assert!(REP_DEFAULT_DELAY > 0);
        assert!(REP_DEFAULT_PERIOD > 0);
        // Delay < 2 seconds
        assert!(REP_DEFAULT_DELAY < 2000);
        // Period < 500ms (> 2 repeats/sec)
        assert!(REP_DEFAULT_PERIOD < 500);
    }

    #[test]
    fn test_all_within_max() {
        assert!(REP_DELAY <= REP_MAX);
        assert!(REP_PERIOD <= REP_MAX);
    }

    #[test]
    fn test_rep_cnt() {
        assert_eq!(REP_CNT, REP_MAX + 1);
    }
}
