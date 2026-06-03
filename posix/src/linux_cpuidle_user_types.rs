//! `<linux/cpuidle.h>` — userspace-visible cpuidle constants.
//!
//! cpuidle exposes per-CPU C-state selection to user space via
//! `/sys/devices/system/cpu/cpuN/cpuidle/` and the global governor
//! sysfs at `/sys/devices/system/cpu/cpuidle/`. These constants name
//! the well-known governors, flag bits returned in idle-state
//! descriptions, and known size limits used by the powertop, tuned,
//! and turbostat tools.

// ---------------------------------------------------------------------------
// Idle-state limits
// ---------------------------------------------------------------------------

/// Maximum number of idle states a driver can register per CPU
/// (`CPUIDLE_STATE_MAX`).
pub const CPUIDLE_STATE_MAX: u32 = 10;
/// Maximum length of an idle-state name (`CPUIDLE_NAME_LEN`).
pub const CPUIDLE_NAME_LEN: u32 = 16;
/// Maximum length of an idle-state description (`CPUIDLE_DESC_LEN`).
pub const CPUIDLE_DESC_LEN: u32 = 32;

// ---------------------------------------------------------------------------
// Idle-state flags (struct cpuidle_state.flags)
// ---------------------------------------------------------------------------

/// State entered with coupled CPUs.
pub const CPUIDLE_FLAG_COUPLED: u32 = 1 << 1;
/// Indicates a non-functional / polling state.
pub const CPUIDLE_FLAG_POLLING: u32 = 1 << 2;
/// Time-keeping is suspended in this state.
pub const CPUIDLE_FLAG_TIMER_STOP: u32 = 1 << 3;
/// State is currently disabled (no entry).
pub const CPUIDLE_FLAG_UNUSABLE: u32 = 1 << 4;
/// State is "off" (driver removed).
pub const CPUIDLE_FLAG_OFF: u32 = 1 << 5;
/// State requires RCU sleep accounting.
pub const CPUIDLE_FLAG_RCU_IDLE: u32 = 1 << 7;

// ---------------------------------------------------------------------------
// Built-in governor names
// ---------------------------------------------------------------------------

/// Default tickless governor.
pub const CPUIDLE_GOV_MENU: &str = "menu";
/// Ladder governor (legacy, used on tick-driven systems).
pub const CPUIDLE_GOV_LADDER: &str = "ladder";
/// "TEO" (Timer Event Oriented) governor.
pub const CPUIDLE_GOV_TEO: &str = "teo";
/// "haltpoll" governor (used in virtual machines).
pub const CPUIDLE_GOV_HALTPOLL: &str = "haltpoll";

// ---------------------------------------------------------------------------
// "current_driver" magic when no driver is registered
// ---------------------------------------------------------------------------

/// Sysfs returns this when cpuidle has no driver bound.
pub const CPUIDLE_DRIVER_NONE: &str = "none";

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_limits_sane() {
        // STATE_MAX must leave room for at least POLL + C1 + C2.
        assert!(CPUIDLE_STATE_MAX >= 3);
        assert!(CPUIDLE_NAME_LEN < CPUIDLE_DESC_LEN);
    }

    #[test]
    fn test_flag_bits_distinct_powers_of_two() {
        let flags = [
            CPUIDLE_FLAG_COUPLED,
            CPUIDLE_FLAG_POLLING,
            CPUIDLE_FLAG_TIMER_STOP,
            CPUIDLE_FLAG_UNUSABLE,
            CPUIDLE_FLAG_OFF,
            CPUIDLE_FLAG_RCU_IDLE,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_governor_names_distinct_lowercase() {
        let g = [
            CPUIDLE_GOV_MENU,
            CPUIDLE_GOV_LADDER,
            CPUIDLE_GOV_TEO,
            CPUIDLE_GOV_HALTPOLL,
            CPUIDLE_DRIVER_NONE,
        ];
        for i in 0..g.len() {
            for j in (i + 1)..g.len() {
                assert_ne!(g[i], g[j]);
            }
            // Sysfs governor strings are always lowercase ASCII.
            for b in g[i].bytes() {
                assert!(b.is_ascii_lowercase() || b == b'-' || b == b'_');
            }
            assert!((g[i].len() as u32) < CPUIDLE_NAME_LEN);
        }
    }
}
