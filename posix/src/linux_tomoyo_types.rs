//! `<linux/tomoyo.h>` — TOMOYO Linux path-based MAC constants.
//!
//! TOMOYO is a path-based mandatory access control system that focuses
//! on system behavior analysis and restriction. Unlike label-based
//! systems (SELinux, SMACK), TOMOYO works with actual pathnames,
//! making policies human-readable. It features a learning mode that
//! automatically generates policies from observed system behavior,
//! domain transitions on program execution, and fine-grained control
//! over file, network, mount, and environment operations.

// ---------------------------------------------------------------------------
// TOMOYO domain states
// ---------------------------------------------------------------------------

/// Domain is in enforcing mode (deny and log violations).
pub const TOMOYO_MODE_ENFORCE: u32 = 0;
/// Domain is in permissive mode (log but allow).
pub const TOMOYO_MODE_PERMISSIVE: u32 = 1;
/// Domain is in learning mode (auto-generate policy from behavior).
pub const TOMOYO_MODE_LEARNING: u32 = 2;
/// Domain is disabled (no checks).
pub const TOMOYO_MODE_DISABLED: u32 = 3;

// ---------------------------------------------------------------------------
// TOMOYO access types
// ---------------------------------------------------------------------------

/// File read access.
pub const TOMOYO_TYPE_READ: u32 = 0;
/// File write access.
pub const TOMOYO_TYPE_WRITE: u32 = 1;
/// File read+write access.
pub const TOMOYO_TYPE_READ_WRITE: u32 = 2;
/// File execute access.
pub const TOMOYO_TYPE_EXECUTE: u32 = 3;
/// File create.
pub const TOMOYO_TYPE_CREATE: u32 = 4;
/// File unlink.
pub const TOMOYO_TYPE_UNLINK: u32 = 5;
/// Directory mkdir.
pub const TOMOYO_TYPE_MKDIR: u32 = 6;
/// Directory rmdir.
pub const TOMOYO_TYPE_RMDIR: u32 = 7;
/// Create special node (mknode).
pub const TOMOYO_TYPE_MKNODE: u32 = 8;
/// Rename.
pub const TOMOYO_TYPE_RENAME: u32 = 9;
/// Hard link.
pub const TOMOYO_TYPE_LINK: u32 = 10;
/// Symbolic link.
pub const TOMOYO_TYPE_SYMLINK: u32 = 11;
/// Truncate.
pub const TOMOYO_TYPE_TRUNCATE: u32 = 12;
/// Change attributes (chown/chmod).
pub const TOMOYO_TYPE_CHATTR: u32 = 13;
/// Mount.
pub const TOMOYO_TYPE_MOUNT: u32 = 14;
/// Unmount.
pub const TOMOYO_TYPE_UMOUNT: u32 = 15;

// ---------------------------------------------------------------------------
// TOMOYO network operations
// ---------------------------------------------------------------------------

/// Network bind.
pub const TOMOYO_NETWORK_BIND: u32 = 0;
/// Network listen.
pub const TOMOYO_NETWORK_LISTEN: u32 = 1;
/// Network connect.
pub const TOMOYO_NETWORK_CONNECT: u32 = 2;
/// Network send.
pub const TOMOYO_NETWORK_SEND: u32 = 3;

// ---------------------------------------------------------------------------
// TOMOYO policy interfaces (/sys/kernel/security/tomoyo/)
// ---------------------------------------------------------------------------

/// Domain policy file.
pub const TOMOYO_IFACE_DOMAIN_POLICY: u32 = 0;
/// Exception policy file.
pub const TOMOYO_IFACE_EXCEPTION_POLICY: u32 = 1;
/// Process status.
pub const TOMOYO_IFACE_PROCESS_STATUS: u32 = 2;
/// Profile configuration (modes per domain).
pub const TOMOYO_IFACE_PROFILE: u32 = 3;
/// Manager (programs that can manage TOMOYO policy).
pub const TOMOYO_IFACE_MANAGER: u32 = 4;
/// Query (pending access requests in learning mode).
pub const TOMOYO_IFACE_QUERY: u32 = 5;
/// Audit log.
pub const TOMOYO_IFACE_AUDIT: u32 = 6;
/// Version string.
pub const TOMOYO_IFACE_VERSION: u32 = 7;
/// Statistics.
pub const TOMOYO_IFACE_STAT: u32 = 8;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            TOMOYO_MODE_ENFORCE,
            TOMOYO_MODE_PERMISSIVE,
            TOMOYO_MODE_LEARNING,
            TOMOYO_MODE_DISABLED,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_access_types_distinct() {
        let types = [
            TOMOYO_TYPE_READ,
            TOMOYO_TYPE_WRITE,
            TOMOYO_TYPE_READ_WRITE,
            TOMOYO_TYPE_EXECUTE,
            TOMOYO_TYPE_CREATE,
            TOMOYO_TYPE_UNLINK,
            TOMOYO_TYPE_MKDIR,
            TOMOYO_TYPE_RMDIR,
            TOMOYO_TYPE_MKNODE,
            TOMOYO_TYPE_RENAME,
            TOMOYO_TYPE_LINK,
            TOMOYO_TYPE_SYMLINK,
            TOMOYO_TYPE_TRUNCATE,
            TOMOYO_TYPE_CHATTR,
            TOMOYO_TYPE_MOUNT,
            TOMOYO_TYPE_UMOUNT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_network_ops_distinct() {
        let ops = [
            TOMOYO_NETWORK_BIND,
            TOMOYO_NETWORK_LISTEN,
            TOMOYO_NETWORK_CONNECT,
            TOMOYO_NETWORK_SEND,
        ];
        for i in 0..ops.len() {
            for j in (i + 1)..ops.len() {
                assert_ne!(ops[i], ops[j]);
            }
        }
    }

    #[test]
    fn test_interfaces_distinct() {
        let ifaces = [
            TOMOYO_IFACE_DOMAIN_POLICY,
            TOMOYO_IFACE_EXCEPTION_POLICY,
            TOMOYO_IFACE_PROCESS_STATUS,
            TOMOYO_IFACE_PROFILE,
            TOMOYO_IFACE_MANAGER,
            TOMOYO_IFACE_QUERY,
            TOMOYO_IFACE_AUDIT,
            TOMOYO_IFACE_VERSION,
            TOMOYO_IFACE_STAT,
        ];
        for i in 0..ifaces.len() {
            for j in (i + 1)..ifaces.len() {
                assert_ne!(ifaces[i], ifaces[j]);
            }
        }
    }
}
