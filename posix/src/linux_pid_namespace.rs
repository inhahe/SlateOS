//! Linux PID namespace constants.
//!
//! PID namespaces isolate process ID number spaces. Processes
//! in a child PID namespace have PIDs that are independent of
//! the parent namespace. The init process (PID 1) in each
//! namespace acts as a reaper for orphaned processes.

// ---------------------------------------------------------------------------
// Clone flags
// ---------------------------------------------------------------------------

/// Create new PID namespace (for children).
pub const CLONE_NEWPID: u64 = 0x20000000;

// ---------------------------------------------------------------------------
// Namespace limits
// ---------------------------------------------------------------------------

/// Maximum PID namespace nesting depth.
pub const PID_NS_MAX_LEVEL: u32 = 32;

// ---------------------------------------------------------------------------
// Special PIDs
// ---------------------------------------------------------------------------

/// Init process PID within any PID namespace.
pub const PID_NS_INIT_PID: u32 = 1;

/// PID returned when process is not visible in caller's namespace.
pub const PID_NS_INVISIBLE: u32 = 0;

// ---------------------------------------------------------------------------
// /proc interface
// ---------------------------------------------------------------------------

/// proc filesystem mount point.
pub const PROC_MOUNT: &str = "/proc";
/// PID namespace info file.
pub const PROC_NS_PID: &str = "ns/pid";
/// PID namespace for children.
pub const PROC_NS_PID_FOR_CHILDREN: &str = "ns/pid_for_children";

// ---------------------------------------------------------------------------
// Signals in PID namespace
// ---------------------------------------------------------------------------

/// Signal that kills all processes in namespace when init dies.
/// (Kernel sends SIGKILL to all members when ns init exits.)
pub const PID_NS_INIT_DEATH_SIGNAL: u32 = 9; // SIGKILL

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clone_newpid() {
        assert_eq!(CLONE_NEWPID, 0x20000000);
        assert!((CLONE_NEWPID as u64).is_power_of_two());
    }

    #[test]
    fn test_clone_flags_no_overlap() {
        // CLONE_NEWPID and CLONE_NEWUSER from user_namespace should not collide
        assert_ne!(CLONE_NEWPID, 0x10000000u64); // CLONE_NEWUSER
    }

    #[test]
    fn test_max_level() {
        assert_eq!(PID_NS_MAX_LEVEL, 32);
        assert!(PID_NS_MAX_LEVEL > 0);
    }

    #[test]
    fn test_init_pid() {
        assert_eq!(PID_NS_INIT_PID, 1);
    }

    #[test]
    fn test_invisible_pid() {
        assert_eq!(PID_NS_INVISIBLE, 0);
        assert_ne!(PID_NS_INVISIBLE, PID_NS_INIT_PID);
    }

    #[test]
    fn test_proc_paths_distinct() {
        let paths = [PROC_MOUNT, PROC_NS_PID, PROC_NS_PID_FOR_CHILDREN];
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn test_init_death_signal() {
        assert_eq!(PID_NS_INIT_DEATH_SIGNAL, 9);
    }
}
