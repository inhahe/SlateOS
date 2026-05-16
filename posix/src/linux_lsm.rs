//! `<linux/lsm.h>` — Linux Security Module framework constants.
//!
//! The LSM framework provides a pluggable interface for mandatory
//! access control. Multiple LSMs can be stacked (major + minor).
//! Each LSM registers hooks that are called at security decision
//! points throughout the kernel.

// ---------------------------------------------------------------------------
// LSM IDs
// ---------------------------------------------------------------------------

/// No LSM / unset.
pub const LSM_ID_UNDEF: u64 = 0;
/// SELinux.
pub const LSM_ID_SELINUX: u64 = 1;
/// Smack.
pub const LSM_ID_SMACK: u64 = 2;
/// TOMOYO.
pub const LSM_ID_TOMOYO: u64 = 3;
/// AppArmor.
pub const LSM_ID_APPARMOR: u64 = 4;
/// Yama (ptrace restrictions).
pub const LSM_ID_YAMA: u64 = 5;
/// LoadPin (kernel module origin).
pub const LSM_ID_LOADPIN: u64 = 6;
/// SafeSetID (setuid restrictions).
pub const LSM_ID_SAFESETID: u64 = 7;
/// Lockdown.
pub const LSM_ID_LOCKDOWN: u64 = 8;
/// BPF LSM.
pub const LSM_ID_BPF: u64 = 9;
/// Landlock.
pub const LSM_ID_LANDLOCK: u64 = 10;
/// Integrity Policy Enforcement.
pub const LSM_ID_IPE: u64 = 11;

// ---------------------------------------------------------------------------
// LSM attribute types (for lsm_get_self_attr / lsm_set_self_attr)
// ---------------------------------------------------------------------------

/// Current security context.
pub const LSM_ATTR_CURRENT: u32 = 100;
/// Exec security context (set on next exec).
pub const LSM_ATTR_EXEC: u32 = 101;
/// Filesystem creation context.
pub const LSM_ATTR_FSCREATE: u32 = 102;
/// Key creation context.
pub const LSM_ATTR_KEYCREATE: u32 = 103;
/// Socket creation context.
pub const LSM_ATTR_SOCKCREATE: u32 = 104;
/// Previous security context (after transition).
pub const LSM_ATTR_PREV: u32 = 105;

// ---------------------------------------------------------------------------
// LSM flags
// ---------------------------------------------------------------------------

/// LSM is enabled.
pub const LSM_FLAG_ENABLED: u32 = 1 << 0;
/// LSM is a major (exclusive) LSM.
pub const LSM_FLAG_EXCLUSIVE: u32 = 1 << 1;

// ---------------------------------------------------------------------------
// Lockdown levels
// ---------------------------------------------------------------------------

/// No lockdown.
pub const LOCKDOWN_NONE: u32 = 0;
/// Integrity lockdown (prevent unsigned modules, etc.).
pub const LOCKDOWN_INTEGRITY: u32 = 1;
/// Confidentiality lockdown (prevent reading kernel memory).
pub const LOCKDOWN_CONFIDENTIALITY: u32 = 2;

// ---------------------------------------------------------------------------
// lsm_list_modules flags
// ---------------------------------------------------------------------------

/// Return all modules (including disabled).
pub const LSM_LIST_ALL: u32 = 0;
/// Return only enabled modules.
pub const LSM_LIST_ENABLED: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsm_ids_distinct() {
        let ids = [
            LSM_ID_UNDEF, LSM_ID_SELINUX, LSM_ID_SMACK, LSM_ID_TOMOYO,
            LSM_ID_APPARMOR, LSM_ID_YAMA, LSM_ID_LOADPIN, LSM_ID_SAFESETID,
            LSM_ID_LOCKDOWN, LSM_ID_BPF, LSM_ID_LANDLOCK, LSM_ID_IPE,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_attr_types_distinct() {
        let attrs = [
            LSM_ATTR_CURRENT, LSM_ATTR_EXEC, LSM_ATTR_FSCREATE,
            LSM_ATTR_KEYCREATE, LSM_ATTR_SOCKCREATE, LSM_ATTR_PREV,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_flags_powers_of_two() {
        let flags = [LSM_FLAG_ENABLED, LSM_FLAG_EXCLUSIVE];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        assert_eq!(LSM_FLAG_ENABLED & LSM_FLAG_EXCLUSIVE, 0);
    }

    #[test]
    fn test_lockdown_levels_distinct() {
        let levels = [LOCKDOWN_NONE, LOCKDOWN_INTEGRITY, LOCKDOWN_CONFIDENTIALITY];
        for i in 0..levels.len() {
            for j in (i + 1)..levels.len() {
                assert_ne!(levels[i], levels[j]);
            }
        }
    }

    #[test]
    fn test_lockdown_ordering() {
        assert!(LOCKDOWN_NONE < LOCKDOWN_INTEGRITY);
        assert!(LOCKDOWN_INTEGRITY < LOCKDOWN_CONFIDENTIALITY);
    }

    #[test]
    fn test_list_flags_distinct() {
        assert_ne!(LSM_LIST_ALL, LSM_LIST_ENABLED);
    }
}
