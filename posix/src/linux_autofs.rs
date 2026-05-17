//! `<linux/auto_fs.h>` — autofs (automount) constants.
//!
//! autofs is the Linux kernel automounter infrastructure. It allows
//! filesystems to be mounted on demand when accessed and unmounted
//! after a period of inactivity. Used for NFS home directories,
//! removable media, and network shares via autofs daemon (automountd).

// ---------------------------------------------------------------------------
// autofs protocol version
// ---------------------------------------------------------------------------

/// autofs protocol major version.
pub const AUTOFS_PROTO_VERSION_MAJOR: u32 = 5;
/// autofs protocol minor version.
pub const AUTOFS_PROTO_VERSION_MINOR: u32 = 5;

// ---------------------------------------------------------------------------
// Packet types (kernel → daemon)
// ---------------------------------------------------------------------------

/// Missing (indirect mount request).
pub const AUTOFS_PTYPE_MISSING: u32 = 0;
/// Expire (inactivity timeout).
pub const AUTOFS_PTYPE_EXPIRE: u32 = 1;

// ---------------------------------------------------------------------------
// ioctl commands (as constant values)
// ---------------------------------------------------------------------------

/// Ready (mount succeeded).
pub const AUTOFS_IOC_READY: u32 = 0x9360;
/// Fail (mount failed).
pub const AUTOFS_IOC_FAIL: u32 = 0x9361;
/// Set timeout.
pub const AUTOFS_IOC_SETTIMEOUT: u32 = 0x9364;
/// Set pipe fd.
pub const AUTOFS_IOC_PROTOVER: u32 = 0x9363;
/// Ask daemon to expire.
pub const AUTOFS_IOC_EXPIRE: u32 = 0x9365;

// ---------------------------------------------------------------------------
// autofs mount types
// ---------------------------------------------------------------------------

/// Indirect mount (trigger on subdirectory access).
pub const AUTOFS_TYPE_INDIRECT: u32 = 1;
/// Direct mount (trigger on mount point itself).
pub const AUTOFS_TYPE_DIRECT: u32 = 2;
/// Offset mount (multi-mount entry).
pub const AUTOFS_TYPE_OFFSET: u32 = 4;

// ---------------------------------------------------------------------------
// Expiry flags
// ---------------------------------------------------------------------------

/// Expire immediately (don't wait for timeout).
pub const AUTOFS_EXP_IMMEDIATE: u32 = 1;
/// Force expire (even if busy).
pub const AUTOFS_EXP_FORCED: u32 = 2;

// ---------------------------------------------------------------------------
// State flags
// ---------------------------------------------------------------------------

/// Mount point is pending (waiting for daemon).
pub const AUTOFS_STATE_PENDING: u8 = 0;
/// Mount point is active.
pub const AUTOFS_STATE_ACTIVE: u8 = 1;
/// Mount point expiring.
pub const AUTOFS_STATE_EXPIRING: u8 = 2;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        assert_eq!(AUTOFS_PROTO_VERSION_MAJOR, 5);
        assert_eq!(AUTOFS_PROTO_VERSION_MINOR, 5);
    }

    #[test]
    fn test_packet_types_distinct() {
        assert_ne!(AUTOFS_PTYPE_MISSING, AUTOFS_PTYPE_EXPIRE);
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            AUTOFS_IOC_READY, AUTOFS_IOC_FAIL, AUTOFS_IOC_SETTIMEOUT,
            AUTOFS_IOC_PROTOVER, AUTOFS_IOC_EXPIRE,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }

    #[test]
    fn test_mount_types_distinct() {
        let types = [AUTOFS_TYPE_INDIRECT, AUTOFS_TYPE_DIRECT, AUTOFS_TYPE_OFFSET];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_expiry_flags_distinct() {
        assert_ne!(AUTOFS_EXP_IMMEDIATE, AUTOFS_EXP_FORCED);
    }

    #[test]
    fn test_states_distinct() {
        let states = [AUTOFS_STATE_PENDING, AUTOFS_STATE_ACTIVE, AUTOFS_STATE_EXPIRING];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }
}
