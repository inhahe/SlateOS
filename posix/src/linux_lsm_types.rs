//! `<linux/lsm.h>` — Linux Security Modules (LSM) framework constants.
//!
//! LSM provides hook-based security infrastructure allowing multiple
//! security modules to coexist. Each module registers callbacks on
//! security-relevant operations (file open, socket connect, task
//! signal, etc.). The framework supports stacking: multiple LSMs
//! can be active simultaneously (BPF + AppArmor + Yama, etc.).
//! The `lsm` syscall (added in 6.x) allows userspace to query which
//! LSMs are active and retrieve security attributes.

// ---------------------------------------------------------------------------
// LSM IDs (each module has a unique ID)
// ---------------------------------------------------------------------------

/// Undefined/no LSM.
pub const LSM_ID_UNDEF: u32 = 0;
/// SELinux.
pub const LSM_ID_SELINUX: u32 = 109;
/// SMACK.
pub const LSM_ID_SMACK: u32 = 110;
/// TOMOYO.
pub const LSM_ID_TOMOYO: u32 = 111;
/// AppArmor.
pub const LSM_ID_APPARMOR: u32 = 112;
/// Yama (ptrace restrictions).
pub const LSM_ID_YAMA: u32 = 113;
/// LoadPin (pin kernel module sources).
pub const LSM_ID_LOADPIN: u32 = 114;
/// SafeSetID (UID/GID transition policy).
pub const LSM_ID_SAFESETID: u32 = 115;
/// Lockdown (restrict kernel features).
pub const LSM_ID_LOCKDOWN: u32 = 116;
/// BPF LSM (eBPF-based security).
pub const LSM_ID_BPF: u32 = 117;
/// Landlock (unprivileged sandboxing).
pub const LSM_ID_LANDLOCK: u32 = 118;
/// IMA (Integrity Measurement Architecture).
pub const LSM_ID_IMA: u32 = 119;
/// EVM (Extended Verification Module).
pub const LSM_ID_EVM: u32 = 120;
/// IPE (Integrity Policy Enforcement).
pub const LSM_ID_IPE: u32 = 121;

// ---------------------------------------------------------------------------
// LSM attribute types (for lsm_get_self_attr / lsm_set_self_attr syscalls)
// ---------------------------------------------------------------------------

/// Current security context.
pub const LSM_ATTR_CURRENT: u32 = 100;
/// Exec security context (applied on next exec).
pub const LSM_ATTR_EXEC: u32 = 101;
/// Fscreate security context (applied to new files).
pub const LSM_ATTR_FSCREATE: u32 = 102;
/// Keycreate security context (applied to new keys).
pub const LSM_ATTR_KEYCREATE: u32 = 103;
/// Previous security context (before last transition).
pub const LSM_ATTR_PREV: u32 = 104;
/// Sockcreate security context (applied to new sockets).
pub const LSM_ATTR_SOCKCREATE: u32 = 105;

// ---------------------------------------------------------------------------
// LSM hook categories (for understanding stacking)
// ---------------------------------------------------------------------------

/// File operations hooks.
pub const LSM_HOOK_FILE: u32 = 0;
/// Task/process hooks.
pub const LSM_HOOK_TASK: u32 = 1;
/// IPC hooks.
pub const LSM_HOOK_IPC: u32 = 2;
/// Network hooks.
pub const LSM_HOOK_NET: u32 = 3;
/// Inode hooks.
pub const LSM_HOOK_INODE: u32 = 4;
/// Superblock hooks.
pub const LSM_HOOK_SUPERBLOCK: u32 = 5;

// ---------------------------------------------------------------------------
// LSM flags
// ---------------------------------------------------------------------------

/// LSM is enabled.
pub const LSM_FLAG_ENABLED: u32 = 1 << 0;
/// LSM is exclusive (cannot stack with others of same type).
pub const LSM_FLAG_EXCLUSIVE: u32 = 1 << 1;
/// LSM handles legacy "major" interfaces (/proc/self/attr/).
pub const LSM_FLAG_LEGACY_MAJOR: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsm_ids_distinct() {
        let ids = [
            LSM_ID_UNDEF, LSM_ID_SELINUX, LSM_ID_SMACK,
            LSM_ID_TOMOYO, LSM_ID_APPARMOR, LSM_ID_YAMA,
            LSM_ID_LOADPIN, LSM_ID_SAFESETID, LSM_ID_LOCKDOWN,
            LSM_ID_BPF, LSM_ID_LANDLOCK, LSM_ID_IMA,
            LSM_ID_EVM, LSM_ID_IPE,
        ];
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                assert_ne!(ids[i], ids[j]);
            }
        }
    }

    #[test]
    fn test_attrs_distinct() {
        let attrs = [
            LSM_ATTR_CURRENT, LSM_ATTR_EXEC, LSM_ATTR_FSCREATE,
            LSM_ATTR_KEYCREATE, LSM_ATTR_PREV, LSM_ATTR_SOCKCREATE,
        ];
        for i in 0..attrs.len() {
            for j in (i + 1)..attrs.len() {
                assert_ne!(attrs[i], attrs[j]);
            }
        }
    }

    #[test]
    fn test_hook_categories_distinct() {
        let cats = [
            LSM_HOOK_FILE, LSM_HOOK_TASK, LSM_HOOK_IPC,
            LSM_HOOK_NET, LSM_HOOK_INODE, LSM_HOOK_SUPERBLOCK,
        ];
        for i in 0..cats.len() {
            for j in (i + 1)..cats.len() {
                assert_ne!(cats[i], cats[j]);
            }
        }
    }

    #[test]
    fn test_flags_no_overlap() {
        let flags = [
            LSM_FLAG_ENABLED, LSM_FLAG_EXCLUSIVE, LSM_FLAG_LEGACY_MAJOR,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }
}
