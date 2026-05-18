//! `<linux/cpuidle.h>` — CPU idle state constants.
//!
//! The cpuidle framework manages processor low-power states (C-states).
//! When a CPU has no work, the governor selects an appropriate idle
//! state based on predicted idle duration, latency requirements, and
//! power savings. Deeper states save more power but take longer to exit.

// ---------------------------------------------------------------------------
// C-state indices (common x86 idle states)
// ---------------------------------------------------------------------------

/// C0: Active (CPU executing instructions).
pub const CPUIDLE_STATE_C0: u32 = 0;
/// C1: Halt (clock gated, instant wake).
pub const CPUIDLE_STATE_C1: u32 = 1;
/// C1E: Enhanced halt (lower voltage).
pub const CPUIDLE_STATE_C1E: u32 = 2;
/// C2: Stop-clock (bus interface quiesced).
pub const CPUIDLE_STATE_C2: u32 = 3;
/// C3: Sleep (L1/L2 cache may be flushed).
pub const CPUIDLE_STATE_C3: u32 = 4;
/// C6: Deep power down (voltage reduced to retention).
pub const CPUIDLE_STATE_C6: u32 = 5;

// ---------------------------------------------------------------------------
// cpuidle driver flags
// ---------------------------------------------------------------------------

/// Driver supports coupled idle (multi-core coordination).
pub const CPUIDLE_FLAG_COUPLED: u32 = 1 << 0;
/// State requires timer broadcast (local APIC stops).
pub const CPUIDLE_FLAG_TIMER_STOP: u32 = 1 << 1;
/// State is polling (not a real idle state).
pub const CPUIDLE_FLAG_POLLING: u32 = 1 << 2;
/// State is an RCU idle state.
pub const CPUIDLE_FLAG_RCU_IDLE: u32 = 1 << 3;
/// State is for TLB flushing on idle.
pub const CPUIDLE_FLAG_TLB_FLUSHED: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// cpuidle governor policy
// ---------------------------------------------------------------------------

/// Menu governor (default, prediction-based).
pub const CPUIDLE_GOV_MENU: u32 = 0;
/// Ladder governor (step-based, for tickful systems).
pub const CPUIDLE_GOV_LADDER: u32 = 1;
/// TEO governor (Timer Events Oriented).
pub const CPUIDLE_GOV_TEO: u32 = 2;
/// Haltpoll governor (poll before halt).
pub const CPUIDLE_GOV_HALTPOLL: u32 = 3;

// ---------------------------------------------------------------------------
// Limits
// ---------------------------------------------------------------------------

/// Maximum number of idle states per driver.
pub const CPUIDLE_STATE_MAX: u32 = 10;
/// Maximum idle state name length.
pub const CPUIDLE_NAME_LEN: u32 = 16;
/// Maximum idle state description length.
pub const CPUIDLE_DESC_LEN: u32 = 32;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cstates_sequential() {
        assert_eq!(CPUIDLE_STATE_C0, 0);
        assert_eq!(CPUIDLE_STATE_C1, 1);
        assert_eq!(CPUIDLE_STATE_C1E, 2);
        assert_eq!(CPUIDLE_STATE_C2, 3);
        assert_eq!(CPUIDLE_STATE_C3, 4);
        assert_eq!(CPUIDLE_STATE_C6, 5);
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            CPUIDLE_FLAG_COUPLED, CPUIDLE_FLAG_TIMER_STOP,
            CPUIDLE_FLAG_POLLING, CPUIDLE_FLAG_RCU_IDLE,
            CPUIDLE_FLAG_TLB_FLUSHED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_governors_distinct() {
        let govs = [
            CPUIDLE_GOV_MENU, CPUIDLE_GOV_LADDER,
            CPUIDLE_GOV_TEO, CPUIDLE_GOV_HALTPOLL,
        ];
        for i in 0..govs.len() {
            for j in (i + 1)..govs.len() {
                assert_ne!(govs[i], govs[j]);
            }
        }
    }

    #[test]
    fn test_limits() {
        assert!(CPUIDLE_STATE_MAX >= 6);
        assert!(CPUIDLE_NAME_LEN > 0);
        assert!(CPUIDLE_DESC_LEN > CPUIDLE_NAME_LEN);
    }
}
