//! `<linux/rcupdate.h>` — Read-Copy-Update (RCU) constants.
//!
//! RCU is a synchronization mechanism optimized for read-heavy
//! workloads. Readers access shared data without locks (just
//! rcu_read_lock/rcu_read_unlock which disable preemption). Writers
//! make a copy, modify it, publish the new version atomically, then
//! wait for all existing readers to finish (grace period) before
//! freeing the old version. This gives readers zero-overhead access
//! while writers pay the cost of copying and waiting.

// ---------------------------------------------------------------------------
// RCU flavors
// ---------------------------------------------------------------------------

/// Classic RCU (preemptible, used by most code).
pub const RCU_FLAVOR_PREEMPT: u32 = 0;
/// RCU-bh (bottom-half, deprecated in favor of merged flavors).
pub const RCU_FLAVOR_BH: u32 = 1;
/// RCU-sched (non-preemptible critical sections).
pub const RCU_FLAVOR_SCHED: u32 = 2;
/// Expedited RCU (force fast grace period via IPI).
pub const RCU_FLAVOR_EXPEDITED: u32 = 3;

// ---------------------------------------------------------------------------
// RCU grace period states
// ---------------------------------------------------------------------------

/// Grace period not yet started.
pub const RCU_GP_IDLE: u32 = 0;
/// Grace period in progress (waiting for readers).
pub const RCU_GP_WAIT: u32 = 1;
/// Grace period completed (safe to free old data).
pub const RCU_GP_DONE: u32 = 2;
/// Grace period is being cleaned up.
pub const RCU_GP_CLEANUP: u32 = 3;

// ---------------------------------------------------------------------------
// RCU callback offloading
// ---------------------------------------------------------------------------

/// Callbacks processed on same CPU (default).
pub const RCU_NOCB_OFF: u32 = 0;
/// Callbacks offloaded to dedicated kthread (rcuo*).
pub const RCU_NOCB_ON: u32 = 1;

// ---------------------------------------------------------------------------
// SRCU (Sleepable RCU) constants
// ---------------------------------------------------------------------------

/// SRCU allows sleeping in read-side critical sections.
pub const SRCU_MAX_NODELAY_PHASE: u32 = 3;
/// SRCU expedited grace period timeout (ms).
pub const SRCU_EXP_TIMEOUT_MS: u32 = 20;

// ---------------------------------------------------------------------------
// RCU torture test constants
// ---------------------------------------------------------------------------

/// Number of RCU readers for torture testing.
pub const RCU_TORTURE_READERS: u32 = 16;
/// Duration of each torture test phase (seconds).
pub const RCU_TORTURE_PHASE_SEC: u32 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flavors_distinct() {
        let flavors = [
            RCU_FLAVOR_PREEMPT,
            RCU_FLAVOR_BH,
            RCU_FLAVOR_SCHED,
            RCU_FLAVOR_EXPEDITED,
        ];
        for i in 0..flavors.len() {
            for j in (i + 1)..flavors.len() {
                assert_ne!(flavors[i], flavors[j]);
            }
        }
    }

    #[test]
    fn test_gp_states_distinct() {
        let states = [RCU_GP_IDLE, RCU_GP_WAIT, RCU_GP_DONE, RCU_GP_CLEANUP];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_nocb_values() {
        assert_ne!(RCU_NOCB_OFF, RCU_NOCB_ON);
    }

    #[test]
    fn test_srcu_positive() {
        assert!(SRCU_EXP_TIMEOUT_MS > 0);
    }
}
