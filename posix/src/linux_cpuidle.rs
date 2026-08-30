//! `<linux/cpuidle.h>` — CPU idle state constants.
//!
//! The cpuidle subsystem manages CPU idle states (C-states). When
//! a CPU has no work to do, the governor selects an appropriate
//! idle state that balances power savings against wake-up latency.

// ---------------------------------------------------------------------------
// C-state indices (generic, platform maps these)
// ---------------------------------------------------------------------------

/// C0: Active/running (not idle).
pub const CPUIDLE_STATE_C0: u8 = 0;
/// C1: Halt (clock gated, fast wake).
pub const CPUIDLE_STATE_C1: u8 = 1;
/// C2: Stop-clock (deeper, slower wake).
pub const CPUIDLE_STATE_C2: u8 = 2;
/// C3: Sleep (caches may be flushed).
pub const CPUIDLE_STATE_C3: u8 = 3;
/// C4: Deeper sleep (package C-state).
pub const CPUIDLE_STATE_C4: u8 = 4;
/// C5: Deepest sleep (may lose more state).
pub const CPUIDLE_STATE_C5: u8 = 5;

// ---------------------------------------------------------------------------
// cpuidle governor IDs
// ---------------------------------------------------------------------------

/// Menu governor (default on most systems).
pub const CPUIDLE_GOV_MENU: u8 = 0;
/// Ladder governor (simple step-up/down).
pub const CPUIDLE_GOV_LADDER: u8 = 1;
/// TEO (Timer Events Oriented) governor.
pub const CPUIDLE_GOV_TEO: u8 = 2;
/// Haltpoll governor (for VMs).
pub const CPUIDLE_GOV_HALTPOLL: u8 = 3;

// ---------------------------------------------------------------------------
// cpuidle flags
// ---------------------------------------------------------------------------

/// State is coupled (multiple CPUs enter together).
pub const CPUIDLE_FLAG_COUPLED: u32 = 1 << 0;
/// Timer stop (TSC may stop in this state).
pub const CPUIDLE_FLAG_TIMER_STOP: u32 = 1 << 1;
/// State not usable (disabled by user/firmware).
pub const CPUIDLE_FLAG_UNUSABLE: u32 = 1 << 2;
/// State is polling (busy-wait, not real idle).
pub const CPUIDLE_FLAG_POLLING: u32 = 1 << 3;
/// TLB not flushed on entry.
pub const CPUIDLE_FLAG_TLB_FLUSHED: u32 = 1 << 4;
/// RCU idle (CPUs in this state are in RCU extended QS).
pub const CPUIDLE_FLAG_RCU_IDLE: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum idle states per driver.
pub const CPUIDLE_STATE_MAX: u8 = 10;
/// Maximum name length.
pub const CPUIDLE_NAME_LEN: u8 = 16;
/// Maximum description length.
pub const CPUIDLE_DESC_LEN: u8 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_states_distinct() {
        let states = [
            CPUIDLE_STATE_C0,
            CPUIDLE_STATE_C1,
            CPUIDLE_STATE_C2,
            CPUIDLE_STATE_C3,
            CPUIDLE_STATE_C4,
            CPUIDLE_STATE_C5,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_governors_distinct() {
        let govs = [
            CPUIDLE_GOV_MENU,
            CPUIDLE_GOV_LADDER,
            CPUIDLE_GOV_TEO,
            CPUIDLE_GOV_HALTPOLL,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CPUIDLE_FLAG_COUPLED,
            CPUIDLE_FLAG_TIMER_STOP,
            CPUIDLE_FLAG_UNUSABLE,
            CPUIDLE_FLAG_POLLING,
            CPUIDLE_FLAG_TLB_FLUSHED,
            CPUIDLE_FLAG_RCU_IDLE,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_flags_power_of_two() {
        let flags = [
            CPUIDLE_FLAG_COUPLED,
            CPUIDLE_FLAG_TIMER_STOP,
            CPUIDLE_FLAG_UNUSABLE,
            CPUIDLE_FLAG_POLLING,
            CPUIDLE_FLAG_TLB_FLUSHED,
            CPUIDLE_FLAG_RCU_IDLE,
        ];
        for f in &flags {
            assert!(f.is_power_of_two());
        }
    }

    #[test]
    fn test_limits() {
        assert!(CPUIDLE_STATE_MAX >= 6);
    }
}
