//! `<linux/seccomp.h>` — Seccomp return action constants.
//!
//! When a seccomp BPF filter evaluates a syscall, it returns a
//! 32-bit value. The upper 16 bits specify the action to take;
//! the lower 16 bits carry action-specific data (e.g., errno value
//! for SECCOMP_RET_ERRNO).

// ---------------------------------------------------------------------------
// Seccomp return action values (upper 16 bits)
// ---------------------------------------------------------------------------

/// Kill the offending thread immediately.
pub const SECCOMP_RET_KILL_THREAD: u32 = 0x0000_0000;
/// Kill the entire process.
pub const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
/// Send SIGSYS to the thread.
pub const SECCOMP_RET_TRAP: u32 = 0x0003_0000;
/// Return errno to the syscall caller.
pub const SECCOMP_RET_ERRNO: u32 = 0x0005_0000;
/// Forward to userspace notification listener.
pub const SECCOMP_RET_USER_NOTIF: u32 = 0x7FC0_0000;
/// Pass to ptrace tracer.
pub const SECCOMP_RET_TRACE: u32 = 0x7FF0_0000;
/// Log the action and allow.
pub const SECCOMP_RET_LOG: u32 = 0x7FFC_0000;
/// Allow the syscall.
pub const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

// ---------------------------------------------------------------------------
// Return value masks
// ---------------------------------------------------------------------------

/// Mask for the action field (upper 16 bits).
pub const SECCOMP_RET_ACTION_FULL: u32 = 0xFFFF_0000;
/// Mask for the data field (lower 16 bits).
pub const SECCOMP_RET_DATA: u32 = 0x0000_FFFF;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_actions_distinct() {
        let actions = [
            SECCOMP_RET_KILL_THREAD, SECCOMP_RET_KILL_PROCESS,
            SECCOMP_RET_TRAP, SECCOMP_RET_ERRNO,
            SECCOMP_RET_USER_NOTIF, SECCOMP_RET_TRACE,
            SECCOMP_RET_LOG, SECCOMP_RET_ALLOW,
        ];
        for i in 0..actions.len() {
            for j in (i + 1)..actions.len() {
                assert_ne!(actions[i], actions[j]);
            }
        }
    }

    #[test]
    fn test_kill_thread_is_zero() {
        assert_eq!(SECCOMP_RET_KILL_THREAD, 0);
    }

    #[test]
    fn test_masks_complement() {
        assert_eq!(SECCOMP_RET_ACTION_FULL | SECCOMP_RET_DATA, u32::MAX);
        assert_eq!(SECCOMP_RET_ACTION_FULL & SECCOMP_RET_DATA, 0);
    }

    #[test]
    fn test_actions_aligned_to_data_boundary() {
        // All actions should have zero in the lower 16 bits
        let actions = [
            SECCOMP_RET_KILL_THREAD, SECCOMP_RET_KILL_PROCESS,
            SECCOMP_RET_TRAP, SECCOMP_RET_ERRNO,
            SECCOMP_RET_USER_NOTIF, SECCOMP_RET_TRACE,
            SECCOMP_RET_LOG, SECCOMP_RET_ALLOW,
        ];
        for a in &actions {
            assert_eq!(a & SECCOMP_RET_DATA, 0);
        }
    }

    #[test]
    fn test_action_ordering() {
        // Actions are ordered by severity (lower = more severe)
        assert!(SECCOMP_RET_KILL_THREAD < SECCOMP_RET_TRAP);
        assert!(SECCOMP_RET_TRAP < SECCOMP_RET_ERRNO);
        assert!(SECCOMP_RET_ERRNO < SECCOMP_RET_ALLOW);
    }
}
