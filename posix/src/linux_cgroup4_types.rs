//! `<linux/cgroup.h>` — Additional cgroup constants (batch 4).
//!
//! Supplementary cgroup constants covering cgroup2 thread modes,
//! pressure stall information, and cgroup freezer states.

// ---------------------------------------------------------------------------
// Cgroup2 thread modes
// ---------------------------------------------------------------------------

/// Domain cgroup (default).
pub const CGROUP_DOMAIN: u32 = 0;
/// Threaded cgroup.
pub const CGROUP_THREADED: u32 = 1;
/// Domain-threaded (transition state).
pub const CGROUP_DOMAIN_THREADED: u32 = 2;
/// Domain-invalid (internal).
pub const CGROUP_DOMAIN_INVALID: u32 = 3;

// ---------------------------------------------------------------------------
// PSI (Pressure Stall Information) states
// ---------------------------------------------------------------------------

/// Some tasks stalled.
pub const PSI_SOME: u32 = 0;
/// All tasks stalled (full).
pub const PSI_FULL: u32 = 1;

/// CPU resource.
pub const PSI_CPU: u32 = 0;
/// Memory resource.
pub const PSI_MEM: u32 = 1;
/// I/O resource.
pub const PSI_IO: u32 = 2;
/// IRQ resource.
pub const PSI_IRQ: u32 = 3;

/// PSI avg10 window (10 seconds).
pub const PSI_AVG10: u32 = 10;
/// PSI avg60 window (60 seconds).
pub const PSI_AVG60: u32 = 60;
/// PSI avg300 window (300 seconds).
pub const PSI_AVG300: u32 = 300;

// ---------------------------------------------------------------------------
// Cgroup freezer states
// ---------------------------------------------------------------------------

/// Cgroup is thawed (running).
pub const CGROUP_THAWED: u32 = 0;
/// Cgroup is frozen.
pub const CGROUP_FROZEN: u32 = 1;
/// Cgroup is freezing (transitioning).
pub const CGROUP_FREEZING: u32 = 2;

// ---------------------------------------------------------------------------
// Cgroup kill flags
// ---------------------------------------------------------------------------

/// Kill all processes in cgroup.
pub const CGROUP_KILL_ALL: u32 = 1 << 0;
/// Kill recursively into child cgroups.
pub const CGROUP_KILL_RECURSIVE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_modes_distinct() {
        let modes = [
            CGROUP_DOMAIN,
            CGROUP_THREADED,
            CGROUP_DOMAIN_THREADED,
            CGROUP_DOMAIN_INVALID,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_psi_states_distinct() {
        assert_ne!(PSI_SOME, PSI_FULL);
    }

    #[test]
    fn test_psi_resources_distinct() {
        let res = [PSI_CPU, PSI_MEM, PSI_IO, PSI_IRQ];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_psi_windows_distinct() {
        let wins = [PSI_AVG10, PSI_AVG60, PSI_AVG300];
        for i in 0..wins.len() {
            for j in (i + 1)..wins.len() {
                assert_ne!(wins[i], wins[j]);
            }
        }
    }

    #[test]
    fn test_freezer_states_distinct() {
        let states = [CGROUP_THAWED, CGROUP_FROZEN, CGROUP_FREEZING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_kill_flags_power_of_two() {
        assert!(CGROUP_KILL_ALL.is_power_of_two());
        assert!(CGROUP_KILL_RECURSIVE.is_power_of_two());
    }

    #[test]
    fn test_kill_flags_no_overlap() {
        assert_eq!(CGROUP_KILL_ALL & CGROUP_KILL_RECURSIVE, 0);
    }
}
