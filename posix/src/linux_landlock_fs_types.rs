//! `<linux/landlock.h>` — Landlock filesystem access-control rights.
//!
//! Landlock is an unprivileged sandboxing LSM. Userspace tools
//! (sandboxer, systemd's LandlockBPF, Tor's `landlock` lockdown,
//! Chromium's seccomp+landlock layer) build rulesets from the
//! `LANDLOCK_ACCESS_FS_*` bits below and apply them via
//! `landlock_create_ruleset`/`landlock_add_rule`/`landlock_restrict_self`.

// ---------------------------------------------------------------------------
// landlock_ruleset_attr handled-access bitmask version
// ---------------------------------------------------------------------------

/// First Landlock ABI (5.13) — exposes bits 0..=12.
pub const LANDLOCK_ABI_V1: u32 = 1;
/// Second ABI (5.19) — adds REFER.
pub const LANDLOCK_ABI_V2: u32 = 2;
/// Third ABI (6.2) — adds TRUNCATE.
pub const LANDLOCK_ABI_V3: u32 = 3;
/// Fourth ABI (6.7) — adds net rules (TCP bind/connect).
pub const LANDLOCK_ABI_V4: u32 = 4;
/// Fifth ABI (6.10) — adds IOCTL_DEV.
pub const LANDLOCK_ABI_V5: u32 = 5;

// ---------------------------------------------------------------------------
// landlock_create_ruleset flags
// ---------------------------------------------------------------------------

/// Query the current ABI version (size returned in result).
pub const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1 << 0;

// ---------------------------------------------------------------------------
// landlock_restrict_self flags
// ---------------------------------------------------------------------------

/// Suppress SIGSYS-style auditing of denied operations.
pub const LANDLOCK_RESTRICT_SELF_LOG_SAME_EXEC_OFF: u32 = 1 << 0;
/// Log enforcement of new-exec rules.
pub const LANDLOCK_RESTRICT_SELF_LOG_NEW_EXEC_ON: u32 = 1 << 1;
/// Suppress subdomain-allow logs.
pub const LANDLOCK_RESTRICT_SELF_LOG_SUBDOMAINS_OFF: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Rule types (landlock_add_rule)
// ---------------------------------------------------------------------------

/// Path-based filesystem access rule.
pub const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
/// Network (TCP) port rule.
pub const LANDLOCK_RULE_NET_PORT: u32 = 2;

// ---------------------------------------------------------------------------
// Filesystem access rights (handled_access_fs / path_beneath_attr.allowed)
// ---------------------------------------------------------------------------

/// Open a file for execute.
pub const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
/// Open a file for write.
pub const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
/// Open a file for read.
pub const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
/// List a directory's contents.
pub const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
/// Remove an empty directory.
pub const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
/// Unlink a file.
pub const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
/// Create a character device.
pub const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
/// Create a directory.
pub const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
/// Create a regular file.
pub const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
/// Create a UNIX-domain or named socket.
pub const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
/// Create a FIFO.
pub const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
/// Create a block device.
pub const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
/// Create a symbolic link.
pub const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
/// Rename / link / mount across boundaries (ABI v2).
pub const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
/// Truncate a file (ABI v3).
pub const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;
/// ioctl(2) on a device file (ABI v5).
pub const LANDLOCK_ACCESS_FS_IOCTL_DEV: u64 = 1 << 15;

// ---------------------------------------------------------------------------
// Network access rights (handled_access_net / net_port_attr.allowed)
// ---------------------------------------------------------------------------

/// Bind a TCP socket to a local port.
pub const LANDLOCK_ACCESS_NET_BIND_TCP: u64 = 1 << 0;
/// Connect a TCP socket to a remote port.
pub const LANDLOCK_ACCESS_NET_CONNECT_TCP: u64 = 1 << 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abi_versions_monotonic() {
        // ABI numbers must be strictly increasing — userspace compares
        // the kernel-reported number against expected feature bits.
        assert!(LANDLOCK_ABI_V1 < LANDLOCK_ABI_V2);
        assert!(LANDLOCK_ABI_V2 < LANDLOCK_ABI_V3);
        assert!(LANDLOCK_ABI_V3 < LANDLOCK_ABI_V4);
        assert!(LANDLOCK_ABI_V4 < LANDLOCK_ABI_V5);
        assert_eq!(LANDLOCK_ABI_V1, 1);
    }

    #[test]
    fn test_rule_types_distinct_nonzero() {
        assert_ne!(LANDLOCK_RULE_PATH_BENEATH, LANDLOCK_RULE_NET_PORT);
        assert!(LANDLOCK_RULE_PATH_BENEATH > 0);
        assert!(LANDLOCK_RULE_NET_PORT > 0);
    }

    #[test]
    fn test_fs_access_bits_distinct_pow2() {
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
        for &b in &bits {
            assert!(b.is_power_of_two());
        }
        for i in 0..bits.len() {
            for j in (i + 1)..bits.len() {
                assert_ne!(bits[i], bits[j]);
            }
        }
        // All FS bits fit in the low 32 of the u64 mask.
        for &b in &bits {
            assert!(b < (1u64 << 32));
        }
    }

    #[test]
    fn test_net_access_bits_distinct_pow2() {
        assert!(LANDLOCK_ACCESS_NET_BIND_TCP.is_power_of_two());
        assert!(LANDLOCK_ACCESS_NET_CONNECT_TCP.is_power_of_two());
        assert_ne!(
            LANDLOCK_ACCESS_NET_BIND_TCP,
            LANDLOCK_ACCESS_NET_CONNECT_TCP
        );
    }

    #[test]
    fn test_create_and_restrict_flags_pow2() {
        assert!(LANDLOCK_CREATE_RULESET_VERSION.is_power_of_two());
        let r = [
            LANDLOCK_RESTRICT_SELF_LOG_SAME_EXEC_OFF,
            LANDLOCK_RESTRICT_SELF_LOG_NEW_EXEC_ON,
            LANDLOCK_RESTRICT_SELF_LOG_SUBDOMAINS_OFF,
        ];
        for &b in &r {
            assert!(b.is_power_of_two());
        }
        for i in 0..r.len() {
            for j in (i + 1)..r.len() {
                assert_ne!(r[i], r[j]);
            }
        }
    }
}
