//! `<linux/landlock.h>` — Landlock unprivileged sandboxing.
//!
//! Landlock (Linux 5.13+) lets unprivileged processes restrict their
//! own filesystem (and, since 6.7, network) access. systemd, browsers,
//! and CI sandboxes call `landlock_create_ruleset`, attach rules with
//! `landlock_add_rule`, then `landlock_restrict_self`. The constants
//! below match `include/uapi/linux/landlock.h`.

// ---------------------------------------------------------------------------
// Syscall numbers on x86_64 (Linux 5.13 = first stable kernel with Landlock)
// ---------------------------------------------------------------------------

pub const NR_LANDLOCK_CREATE_RULESET: u32 = 444;
pub const NR_LANDLOCK_ADD_RULE: u32 = 445;
pub const NR_LANDLOCK_RESTRICT_SELF: u32 = 446;

// ---------------------------------------------------------------------------
// `landlock_create_ruleset` flags
// ---------------------------------------------------------------------------

/// Probe the kernel for the highest supported ABI version.
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// Rule types (`enum landlock_rule_type`)
// ---------------------------------------------------------------------------

pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Filesystem access bits (`struct landlock_ruleset_attr.handled_access_fs`)
// ---------------------------------------------------------------------------

pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Network access bits (Linux 6.7+)
// ---------------------------------------------------------------------------

pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// ABI versions reported by `LANDLOCK_CREATE_RULESET_VERSION`
// ---------------------------------------------------------------------------

pub const LANDLOCK_ABI_V1: u32 = 1;
pub const LANDLOCK_ABI_V2: u32 = 2;
pub const LANDLOCK_ABI_V3: u32 = 3;
pub const LANDLOCK_ABI_V4: u32 = 4;
pub const LANDLOCK_ABI_V5: u32 = 5;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syscall_numbers_x86_64() {
        assert_eq!(NR_LANDLOCK_CREATE_RULESET, 444);
        assert_eq!(NR_LANDLOCK_ADD_RULE, 445);
        assert_eq!(NR_LANDLOCK_RESTRICT_SELF, 446);
        // Three syscalls in dense numerical order.
        assert_eq!(NR_LANDLOCK_ADD_RULE - NR_LANDLOCK_CREATE_RULESET, 1);
        assert_eq!(NR_LANDLOCK_RESTRICT_SELF - NR_LANDLOCK_ADD_RULE, 1);
    }

    #[test]
    fn test_rule_types_distinct() {
        assert_eq!(LANDLOCK_RULE_PATH_BENEATH, 1);
        assert_eq!(LANDLOCK_RULE_NET_PORT, 2);
    }

    #[test]
    fn test_fs_access_bits_pow2() {
        let bits = [
            LANDLOCK_ACCESS_FS_EXECUTE,
            LANDLOCK_ACCESS_FS_WRITE_FILE,
            LANDLOCK_ACCESS_FS_READ_FILE,
            LANDLOCK_ACCESS_FS_READ_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_DIR,
            LANDLOCK_ACCESS_FS_REMOVE_FILE,
            LANDLOCK_ACCESS_FS_MAKE_CHAR,
            LANDLOCK_ACCESS_FS_MAKE_DIR,
            LANDLOCK_ACCESS_FS_MAKE_REG,
            LANDLOCK_ACCESS_FS_MAKE_SOCK,
            LANDLOCK_ACCESS_FS_MAKE_FIFO,
            LANDLOCK_ACCESS_FS_MAKE_BLOCK,
            LANDLOCK_ACCESS_FS_MAKE_SYM,
            LANDLOCK_ACCESS_FS_REFER,
            LANDLOCK_ACCESS_FS_TRUNCATE,
            LANDLOCK_ACCESS_FS_IOCTL_DEV,
        ];
        for b in bits {
            assert!(b.is_power_of_two());
        }
        // 16 single-bit flags OR together = 0xFFFF.
        let mut or = 0u64;
        for b in bits {
            or |= b;
        }
        assert_eq!(or, 0xFFFF);
    }

    #[test]
    fn test_net_access_bits_pow2() {
        for &b in &[LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP] {
            assert!(b.is_power_of_two());
        }
        assert_ne!(LANDLOCK_ACCESS_NET_BIND_TCP, LANDLOCK_ACCESS_NET_CONNECT_TCP);
    }

    #[test]
    fn test_abi_versions_dense() {
        let v = [
            LANDLOCK_ABI_V1,
            LANDLOCK_ABI_V2,
            LANDLOCK_ABI_V3,
            LANDLOCK_ABI_V4,
            LANDLOCK_ABI_V5,
        ];
        for (i, &x) in v.iter().enumerate() {
            assert_eq!(x as usize, i + 1);
        }
    }
}
