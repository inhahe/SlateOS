//! `<linux/cpuidle.h>` — CPU idle state (C-state) constants.
//!
//! When a CPU has no work to do, it enters progressively deeper
//! idle states (C-states) to save power. Deeper states save more
//! power but have higher exit latency (time to wake back up).
//! The cpuidle governor (menu or TEO) predicts how long the CPU
//! will be idle and selects the deepest state whose exit latency
//! is acceptable for the expected idle duration.

// ---------------------------------------------------------------------------
// C-state indices (ACPI standard)
// ---------------------------------------------------------------------------

/// C0: active (CPU running, not idle).
pub const CPUIDLE_STATE_C0: u32 = 0;
/// C1: halt (clock gated, fast wake, ~1us exit).
pub const CPUIDLE_STATE_C1: u32 = 1;
/// C1E: enhanced halt (lower power, ~10us exit).
pub const CPUIDLE_STATE_C1E: u32 = 2;
/// C3: sleep (caches flushed, ~100us exit).
pub const CPUIDLE_STATE_C3: u32 = 3;
/// C6: deep sleep (voltage reduced, ~200us exit).
pub const CPUIDLE_STATE_C6: u32 = 4;
/// C7+: deepest sleep (package-level power off, ~500us+ exit).
pub const CPUIDLE_STATE_C7: u32 = 5;

// ---------------------------------------------------------------------------
// cpuidle governor types
// ---------------------------------------------------------------------------

/// Menu governor (prediction based on recent history + timers).
pub const CPUIDLE_GOV_MENU: u32 = 0;
/// TEO governor (Timer Events Oriented, timer-based prediction).
pub const CPUIDLE_GOV_TEO: u32 = 1;
/// Ladder governor (simple, step up/down based on last residency).
pub const CPUIDLE_GOV_LADDER: u32 = 2;
/// Haltpoll governor (poll before entering deep state, for VMs).
pub const CPUIDLE_GOV_HALTPOLL: u32 = 3;

// ---------------------------------------------------------------------------
// cpuidle limits
// ---------------------------------------------------------------------------

/// Maximum number of C-states per CPU.
pub const CPUIDLE_STATE_MAX: u32 = 10;
/// Maximum state name length.
pub const CPUIDLE_NAME_LEN: u32 = 16;

// ---------------------------------------------------------------------------
// cpuidle flags
// ---------------------------------------------------------------------------

/// State is coupled (multiple CPUs must enter together).
pub const CPUIDLE_FLAG_COUPLED: u32 = 0x01;
/// State has timer stop (timers don't fire in this state).
pub const CPUIDLE_FLAG_TIMER_STOP: u32 = 0x02;
/// State requires polling (can't use hardware C-state entry).
pub const CPUIDLE_FLAG_POLLING: u32 = 0x04;
/// State is a shallow state (use for latency-sensitive workloads).
pub const CPUIDLE_FLAG_SHALLOW: u32 = 0x08;
/// State flushes TLB on exit.
pub const CPUIDLE_FLAG_TLB_FLUSHED: u32 = 0x10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cstates_distinct() {
        let states = [
            CPUIDLE_STATE_C0,
            CPUIDLE_STATE_C1,
            CPUIDLE_STATE_C1E,
            CPUIDLE_STATE_C3,
            CPUIDLE_STATE_C6,
            CPUIDLE_STATE_C7,
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
            CPUIDLE_GOV_TEO,
            CPUIDLE_GOV_LADDER,
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
            CPUIDLE_FLAG_POLLING,
            CPUIDLE_FLAG_SHALLOW,
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
    fn test_limits() {
        assert!(CPUIDLE_STATE_MAX > 0);
        assert!(CPUIDLE_NAME_LEN > 0);
    }
}
