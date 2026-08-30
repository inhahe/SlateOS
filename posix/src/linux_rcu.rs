//! `<linux/rcupdate.h>` — Read-Copy-Update constants.
//!
//! RCU is a synchronization mechanism optimized for read-heavy
//! workloads. Readers run lock-free; updates create new versions
//! and reclaim old data after all readers have finished. Used
//! throughout the kernel for routing tables, module lists, etc.

// ---------------------------------------------------------------------------
// RCU flavor identifiers
// ---------------------------------------------------------------------------

/// Default RCU (preemptible or non-preemptible depending on config).
pub const RCU_FLAVOR_DEFAULT: u32 = 0;
/// RCU bottom-half (softirq context protection).
pub const RCU_FLAVOR_BH: u32 = 1;
/// RCU scheduler (non-preemptible, early-boot safe).
pub const RCU_FLAVOR_SCHED: u32 = 2;
/// SRCU (Sleepable RCU).
pub const RCU_FLAVOR_SRCU: u32 = 3;
/// Tasks RCU (voluntary context switch as quiescent state).
pub const RCU_FLAVOR_TASKS: u32 = 4;
/// Tasks-rude RCU.
pub const RCU_FLAVOR_TASKS_RUDE: u32 = 5;
/// Tasks-trace RCU (for BPF tracing).
pub const RCU_FLAVOR_TASKS_TRACE: u32 = 6;

// ---------------------------------------------------------------------------
// Grace period states
// ---------------------------------------------------------------------------

/// No grace period in progress.
pub const RCU_GP_IDLE: u32 = 0;
/// Grace period started, waiting for quiescent states.
pub const RCU_GP_WAIT_GPS: u32 = 1;
/// Force quiescent state scan.
pub const RCU_GP_WAIT_FQS: u32 = 2;
/// Grace period cleanup.
pub const RCU_GP_CLEANUP: u32 = 3;

// ---------------------------------------------------------------------------
// RCU callback offload modes
// ---------------------------------------------------------------------------

/// No offloading (callbacks in softirq).
pub const RCU_NOCB_OFF: u32 = 0;
/// Offload to kthread.
pub const RCU_NOCB_ON: u32 = 1;

// ---------------------------------------------------------------------------
// RCU boost priorities
// ---------------------------------------------------------------------------

/// Default RCU boost priority (RT priority 1).
pub const RCU_BOOST_PRIO: i32 = 1;
/// Maximum RCU kthread priority.
pub const RCU_KTHREAD_PRIO: i32 = 1;

// ---------------------------------------------------------------------------
// Jiffies-based parameters
// ---------------------------------------------------------------------------

/// Default grace period age before forcing (jiffies).
pub const RCU_JIFFIES_TILL_FORCE_QS: u32 = 3;
/// Default grace period age for expedited (jiffies).
pub const RCU_JIFFIES_EXPEDITED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flavors_distinct() {
        let flavors = [
            RCU_FLAVOR_DEFAULT,
            RCU_FLAVOR_BH,
            RCU_FLAVOR_SCHED,
            RCU_FLAVOR_SRCU,
            RCU_FLAVOR_TASKS,
            RCU_FLAVOR_TASKS_RUDE,
            RCU_FLAVOR_TASKS_TRACE,
        ];
        for i in 0..flavors.len() {
            for j in (i + 1)..flavors.len() {
                assert_ne!(flavors[i], flavors[j]);
            }
        }
    }

    #[test]
    fn test_gp_states_distinct() {
        let states = [
            RCU_GP_IDLE,
            RCU_GP_WAIT_GPS,
            RCU_GP_WAIT_FQS,
            RCU_GP_CLEANUP,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_nocb_modes() {
        assert_ne!(RCU_NOCB_OFF, RCU_NOCB_ON);
    }

    #[test]
    fn test_boost_prio() {
        assert_eq!(RCU_BOOST_PRIO, 1);
        assert!(RCU_BOOST_PRIO > 0);
    }

    #[test]
    fn test_jiffies_values() {
        assert!(RCU_JIFFIES_EXPEDITED <= RCU_JIFFIES_TILL_FORCE_QS);
    }
}
