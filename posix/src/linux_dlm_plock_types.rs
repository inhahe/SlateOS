//! `<linux/dlm_plock.h>` — DLM POSIX-lock userspace protocol constants.
//!
//! Constants exchanged between the kernel DLM (Distributed Lock
//! Manager) module and the userland `dlm_controld` daemon to mediate
//! POSIX file locks on cluster filesystems.

// ---------------------------------------------------------------------------
// dlm_plock_ops — operation codes for the misc-device protocol
// ---------------------------------------------------------------------------

/// Unspecified / unused.
pub const DLM_PLOCK_OP_LOCK: u32 = 1;
/// Unlock an existing POSIX range.
pub const DLM_PLOCK_OP_UNLOCK: u32 = 2;
/// Test whether a lock would block (F_GETLK).
pub const DLM_PLOCK_OP_GET: u32 = 3;
/// Cancel a pending blocking lock request.
pub const DLM_PLOCK_OP_CANCEL: u32 = 4;

// ---------------------------------------------------------------------------
// dlm_plock_flags — request flags
// ---------------------------------------------------------------------------

/// Request was made by a blocking caller (F_SETLKW).
pub const DLM_PLOCK_FL_CLOSE: u32 = 1;

// ---------------------------------------------------------------------------
// Protocol version of the dlm_plock_info structure
// ---------------------------------------------------------------------------

/// Protocol version number for the userspace/kernel dlm_plock interface.
pub const DLM_PLOCK_VERSION_MAJOR: u32 = 1;
/// Minor version.
pub const DLM_PLOCK_VERSION_MINOR: u32 = 2;
/// Patch version.
pub const DLM_PLOCK_VERSION_PATCH: u32 = 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ops_distinct() {
        let ops = [
            DLM_PLOCK_OP_LOCK,
            DLM_PLOCK_OP_UNLOCK,
            DLM_PLOCK_OP_GET,
            DLM_PLOCK_OP_CANCEL,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_flag_nonzero() {
        assert_ne!(DLM_PLOCK_FL_CLOSE, 0);
    }

    #[test]
    fn test_version_components() {
        assert_eq!(DLM_PLOCK_VERSION_MAJOR, 1);
        // Minor must be a small positive number — protect against an accidental
        // zeroing that would silently disable kernel-userspace compat checks.
        assert!(DLM_PLOCK_VERSION_MINOR >= 1);
    }
}
