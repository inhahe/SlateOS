//! `<linux/lsm.h>` — LSM enumeration syscalls.
//!
//! Linux 6.8 added three syscalls that let userspace ask "which LSMs
//! are active, what is my current label, and what would you like me
//! to set?" Cryptsetup, container runtimes (Podman, Docker), and the
//! AppArmor/SELinux userspace tools query this interface so they no
//! longer have to parse `/sys/kernel/security/lsm`.

// ---------------------------------------------------------------------------
// Syscall numbers on x86_64 (Linux 6.8)
// ---------------------------------------------------------------------------

pub const NR_LSM_GET_SELF_ATTR: u32 = 459;
pub const NR_LSM_SET_SELF_ATTR: u32 = 460;
pub const NR_LSM_LIST_MODULES: u32 = 461;

// ---------------------------------------------------------------------------
// LSM IDs (`include/uapi/linux/lsm.h`)
// ---------------------------------------------------------------------------

pub const LSM_ID_UNDEF: u32 = 0;
pub const LSM_ID_CAPABILITY: u32 = 100;
pub const LSM_ID_SELINUX: u32 = 101;
pub const LSM_ID_SMACK: u32 = 102;
pub const LSM_ID_TOMOYO: u32 = 103;
pub const LSM_ID_APPARMOR: u32 = 104;
pub const LSM_ID_YAMA: u32 = 105;
pub const LSM_ID_LOADPIN: u32 = 106;
pub const LSM_ID_SAFESETID: u32 = 107;
pub const LSM_ID_LOCKDOWN: u32 = 108;
pub const LSM_ID_BPF: u32 = 109;
pub const LSM_ID_LANDLOCK: u32 = 110;
pub const LSM_ID_IMA: u32 = 111;
pub const LSM_ID_EVM: u32 = 112;
pub const LSM_ID_IPE: u32 = 113;

// ---------------------------------------------------------------------------
// `lsm_ctx_attr` selectors for `lsm_*_self_attr`
// ---------------------------------------------------------------------------

pub const LSM_ATTR_UNDEF: u32 = 0;
pub const LSM_ATTR_CURRENT: u32 = 100;
pub const LSM_ATTR_EXEC: u32 = 101;
pub const LSM_ATTR_FSCREATE: u32 = 102;
pub const LSM_ATTR_KEYCREATE: u32 = 103;
pub const LSM_ATTR_PREV: u32 = 104;
pub const LSM_ATTR_SOCKCREATE: u32 = 105;

// ---------------------------------------------------------------------------
// Flags for `lsm_get_self_attr`
// ---------------------------------------------------------------------------

/// Caller wants only one (the single active) LSM's reply.
pub const LSM_FLAG_SINGLE: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_numbers_consecutive() {
        // Allocated as a contiguous block of 3 numbers in Linux 6.8.
        assert_eq!(NR_LSM_GET_SELF_ATTR, 459);
        assert_eq!(NR_LSM_SET_SELF_ATTR, 460);
        assert_eq!(NR_LSM_LIST_MODULES, 461);
    }

    #[test]
    fn test_lsm_ids_in_100_range() {
        let ids = [
            LSM_ID_CAPABILITY,
            LSM_ID_SELINUX,
            LSM_ID_SMACK,
            LSM_ID_TOMOYO,
            LSM_ID_APPARMOR,
            LSM_ID_YAMA,
            LSM_ID_LOADPIN,
            LSM_ID_SAFESETID,
            LSM_ID_LOCKDOWN,
            LSM_ID_BPF,
            LSM_ID_LANDLOCK,
            LSM_ID_IMA,
            LSM_ID_EVM,
            LSM_ID_IPE,
        ];
        // UNDEF is the sentinel, separate from the 100+ range.
        assert_eq!(LSM_ID_UNDEF, 0);
        // All real IDs land in the dense 100..113 range.
        for (i, &id) in ids.iter().enumerate() {
            assert_eq!(id as usize, 100 + i);
        }
    }

    #[test]
    fn test_attr_selectors_in_100_range() {
        let a = [
            LSM_ATTR_CURRENT,
            LSM_ATTR_EXEC,
            LSM_ATTR_FSCREATE,
            LSM_ATTR_KEYCREATE,
            LSM_ATTR_PREV,
            LSM_ATTR_SOCKCREATE,
        ];
        assert_eq!(LSM_ATTR_UNDEF, 0);
        for (i, &v) in a.iter().enumerate() {
            assert_eq!(v as usize, 100 + i);
        }
    }

    #[test]
    fn test_flag_single_is_bit0() {
        assert_eq!(LSM_FLAG_SINGLE, 1);
        assert!(LSM_FLAG_SINGLE.is_power_of_two());
    }
}
