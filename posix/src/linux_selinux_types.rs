//! `<linux/selinux.h>` — SELinux (Security-Enhanced Linux) constants.
//!
//! SELinux implements Mandatory Access Control (MAC) using security
//! labels (contexts) attached to every process, file, socket, and IPC
//! object. Access decisions are made by comparing the source context
//! (subject) against the target context (object) using a loaded policy.
//! Unlike DAC (traditional Unix permissions), SELinux can restrict
//! even root if the policy doesn't grant the required permission.

// ---------------------------------------------------------------------------
// SELinux modes
// ---------------------------------------------------------------------------

/// Disabled (SELinux not active).
pub const SELINUX_MODE_DISABLED: u32 = 0;
/// Permissive (log denials but don't enforce).
pub const SELINUX_MODE_PERMISSIVE: u32 = 1;
/// Enforcing (deny and log unauthorized access).
pub const SELINUX_MODE_ENFORCING: u32 = 2;

// ---------------------------------------------------------------------------
// SELinux security class categories
// ---------------------------------------------------------------------------

/// File security class.
pub const SELINUX_CLASS_FILE: u32 = 0;
/// Directory security class.
pub const SELINUX_CLASS_DIR: u32 = 1;
/// Process security class.
pub const SELINUX_CLASS_PROCESS: u32 = 2;
/// Socket security class.
pub const SELINUX_CLASS_SOCKET: u32 = 3;
/// TCP socket security class.
pub const SELINUX_CLASS_TCP_SOCKET: u32 = 4;
/// UDP socket security class.
pub const SELINUX_CLASS_UDP_SOCKET: u32 = 5;
/// Network node security class.
pub const SELINUX_CLASS_NODE: u32 = 6;
/// Network interface security class.
pub const SELINUX_CLASS_NETIF: u32 = 7;
/// IPC shared memory security class.
pub const SELINUX_CLASS_SHM: u32 = 8;
/// IPC message queue security class.
pub const SELINUX_CLASS_MSGQ: u32 = 9;
/// IPC semaphore security class.
pub const SELINUX_CLASS_SEM: u32 = 10;

// ---------------------------------------------------------------------------
// SELinux access vector bits (common file permissions)
// ---------------------------------------------------------------------------

/// Read permission.
pub const SELINUX_AV_READ: u32 = 0x01;
/// Write permission.
pub const SELINUX_AV_WRITE: u32 = 0x02;
/// Execute permission.
pub const SELINUX_AV_EXECUTE: u32 = 0x04;
/// Create permission.
pub const SELINUX_AV_CREATE: u32 = 0x08;
/// Unlink/delete permission.
pub const SELINUX_AV_UNLINK: u32 = 0x10;
/// Get attributes permission.
pub const SELINUX_AV_GETATTR: u32 = 0x20;
/// Set attributes permission.
pub const SELINUX_AV_SETATTR: u32 = 0x40;
/// Append permission.
pub const SELINUX_AV_APPEND: u32 = 0x80;

// ---------------------------------------------------------------------------
// SELinux AVC (Access Vector Cache) stats
// ---------------------------------------------------------------------------

/// AVC cache hit.
pub const SELINUX_AVC_HIT: u32 = 0;
/// AVC cache miss (policy lookup needed).
pub const SELINUX_AVC_MISS: u32 = 1;
/// AVC entry added.
pub const SELINUX_AVC_ADDED: u32 = 2;
/// AVC entry evicted.
pub const SELINUX_AVC_EVICTED: u32 = 3;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modes_distinct() {
        let modes = [
            SELINUX_MODE_DISABLED,
            SELINUX_MODE_PERMISSIVE,
            SELINUX_MODE_ENFORCING,
        ];
        for i in 0..modes.len() {
            for j in (i + 1)..modes.len() {
                assert_ne!(modes[i], modes[j]);
            }
        }
    }

    #[test]
    fn test_classes_distinct() {
        let classes = [
            SELINUX_CLASS_FILE,
            SELINUX_CLASS_DIR,
            SELINUX_CLASS_PROCESS,
            SELINUX_CLASS_SOCKET,
            SELINUX_CLASS_TCP_SOCKET,
            SELINUX_CLASS_UDP_SOCKET,
            SELINUX_CLASS_NODE,
            SELINUX_CLASS_NETIF,
            SELINUX_CLASS_SHM,
            SELINUX_CLASS_MSGQ,
            SELINUX_CLASS_SEM,
        ];
        for i in 0..classes.len() {
            for j in (i + 1)..classes.len() {
                assert_ne!(classes[i], classes[j]);
            }
        }
    }

    #[test]
    fn test_av_bits_no_overlap() {
        let bits = [
            SELINUX_AV_READ,
            SELINUX_AV_WRITE,
            SELINUX_AV_EXECUTE,
            SELINUX_AV_CREATE,
            SELINUX_AV_UNLINK,
            SELINUX_AV_GETATTR,
            SELINUX_AV_SETATTR,
            SELINUX_AV_APPEND,
        ];
        for i in 0..bits.len() {
            assert!(bits[i].is_power_of_two());
            for j in (i + 1)..bits.len() {
                assert_eq!(bits[i] & bits[j], 0);
            }
        }
    }

    #[test]
    fn test_avc_stats_distinct() {
        let stats = [
            SELINUX_AVC_HIT,
            SELINUX_AVC_MISS,
            SELINUX_AVC_ADDED,
            SELINUX_AVC_EVICTED,
        ];
        for i in 0..stats.len() {
            for j in (i + 1)..stats.len() {
                assert_ne!(stats[i], stats[j]);
            }
        }
    }
}
