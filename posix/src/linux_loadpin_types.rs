//! `<linux/loadpin.h>` — LoadPin LSM constants.
//!
//! LoadPin is a Linux Security Module that ensures all kernel-loaded
//! files (modules, firmware, IMA policies, kexec images) originate
//! from the same trusted filesystem. The first kernel file load pins
//! a specific filesystem (identified by device/superblock), and all
//! subsequent loads must come from that same filesystem. This prevents
//! an attacker from loading malicious modules from a writable filesystem
//! even if they have root access.

// ---------------------------------------------------------------------------
// LoadPin file types (what's being loaded)
// ---------------------------------------------------------------------------

/// Kernel module (.ko file).
pub const LOADPIN_TYPE_MODULE: u32 = 0;
/// Firmware blob.
pub const LOADPIN_TYPE_FIRMWARE: u32 = 1;
/// IMA policy file.
pub const LOADPIN_TYPE_IMA_POLICY: u32 = 2;
/// kexec image (new kernel).
pub const LOADPIN_TYPE_KEXEC_IMAGE: u32 = 3;
/// kexec initramfs.
pub const LOADPIN_TYPE_KEXEC_INITRAMFS: u32 = 4;
/// Security policy (SELinux, AppArmor, etc.).
pub const LOADPIN_TYPE_SECURITY_POLICY: u32 = 5;

// ---------------------------------------------------------------------------
// LoadPin enforcement states
// ---------------------------------------------------------------------------

/// LoadPin is not enforcing (disabled or not configured).
pub const LOADPIN_STATE_DISABLED: u32 = 0;
/// LoadPin is enforcing (reject loads from wrong filesystem).
pub const LOADPIN_STATE_ENFORCING: u32 = 1;

// ---------------------------------------------------------------------------
// LoadPin pin status
// ---------------------------------------------------------------------------

/// No filesystem pinned yet (first load will pin).
pub const LOADPIN_NOT_PINNED: u32 = 0;
/// Filesystem is pinned (all loads must come from it).
pub const LOADPIN_PINNED: u32 = 1;

// ---------------------------------------------------------------------------
// LoadPin verification results
// ---------------------------------------------------------------------------

/// Load allowed (from pinned filesystem).
pub const LOADPIN_RESULT_ALLOW: i32 = 0;
/// Load denied (from wrong filesystem).
pub const LOADPIN_RESULT_DENY: i32 = -1;
/// Load from unknown filesystem (not yet pinned).
pub const LOADPIN_RESULT_UNKNOWN: i32 = -2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_types_distinct() {
        let types = [
            LOADPIN_TYPE_MODULE, LOADPIN_TYPE_FIRMWARE,
            LOADPIN_TYPE_IMA_POLICY, LOADPIN_TYPE_KEXEC_IMAGE,
            LOADPIN_TYPE_KEXEC_INITRAMFS, LOADPIN_TYPE_SECURITY_POLICY,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_states_distinct() {
        assert_ne!(LOADPIN_STATE_DISABLED, LOADPIN_STATE_ENFORCING);
    }

    #[test]
    fn test_pin_status_distinct() {
        assert_ne!(LOADPIN_NOT_PINNED, LOADPIN_PINNED);
    }

    #[test]
    fn test_results_distinct() {
        let results = [
            LOADPIN_RESULT_ALLOW, LOADPIN_RESULT_DENY,
            LOADPIN_RESULT_UNKNOWN,
        ];
        for i in 0..results.len() {
            for j in (i + 1)..results.len() {
                assert_ne!(results[i], results[j]);
            }
        }
    }
}
