//! `<linux/auto_fs.h>` — Automount filesystem constants.
//!
//! autofs is the kernel-side component of the automount daemon (autofs/automount).
//! It creates mount points on demand when accessed, enabling transparent
//! mounting of NFS shares, USB drives, etc.

// ---------------------------------------------------------------------------
// autofs ioctl commands
// ---------------------------------------------------------------------------

/// autofs ioctl magic number.
pub const AUTOFS_IOC_MAGIC: u8 = 0x93;

/// Mark the autofs as ready.
pub const AUTOFS_IOC_READY: u64 = 0x00009360;
/// Report a mount failure.
pub const AUTOFS_IOC_FAIL: u64 = 0x00009361;
/// Set timeout.
pub const AUTOFS_IOC_SETTIMEOUT: u64 = 0xC0089364;
/// Set protocol version.
pub const AUTOFS_IOC_PROTOVER: u64 = 0x80049363;
/// Set protocol sub-version.
pub const AUTOFS_IOC_PROTOSUBVER: u64 = 0x80049367;
/// Expire entries.
pub const AUTOFS_IOC_EXPIRE: u64 = 0x81089365;
/// Ask daemon to umount.
pub const AUTOFS_IOC_ASKUMOUNT: u64 = 0x80049370;
/// Expire multi (v5).
pub const AUTOFS_IOC_EXPIRE_MULTI: u64 = 0x40049366;

// ---------------------------------------------------------------------------
// autofs protocol version
// ---------------------------------------------------------------------------

/// Minimum protocol version.
pub const AUTOFS_MIN_PROTO_VERSION: u32 = 3;
/// Maximum protocol version.
pub const AUTOFS_MAX_PROTO_VERSION: u32 = 5;
/// Protocol sub-version.
pub const AUTOFS_PROTO_SUBVERSION: u32 = 5;

// ---------------------------------------------------------------------------
// Packet types (delivered via pipe to daemon)
// ---------------------------------------------------------------------------

/// Missing entry (indirect mount).
pub const AUTOFS_PTYPE_MISSING: u32 = 0;
/// Expire entry.
pub const AUTOFS_PTYPE_EXPIRE: u32 = 1;
/// Missing (direct mount, v5).
pub const AUTOFS_PTYPE_MISSING_DIRECT: u32 = 3;
/// Expire (direct mount, v5).
pub const AUTOFS_PTYPE_EXPIRE_DIRECT: u32 = 4;
/// Missing (indirect, v5).
pub const AUTOFS_PTYPE_MISSING_INDIRECT: u32 = 5;
/// Expire (indirect, v5).
pub const AUTOFS_PTYPE_EXPIRE_INDIRECT: u32 = 6;

// ---------------------------------------------------------------------------
// Mount types
// ---------------------------------------------------------------------------

/// Indirect mount.
pub const AUTOFS_TYPE_INDIRECT: u32 = 1;
/// Direct mount.
pub const AUTOFS_TYPE_DIRECT: u32 = 2;
/// Offset mount.
pub const AUTOFS_TYPE_OFFSET: u32 = 4;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_version() {
        assert!(AUTOFS_MIN_PROTO_VERSION <= AUTOFS_MAX_PROTO_VERSION);
        assert_eq!(AUTOFS_MAX_PROTO_VERSION, 5);
    }

    #[test]
    fn test_packet_types_distinct() {
        let types = [
            AUTOFS_PTYPE_MISSING, AUTOFS_PTYPE_EXPIRE,
            AUTOFS_PTYPE_MISSING_DIRECT, AUTOFS_PTYPE_EXPIRE_DIRECT,
            AUTOFS_PTYPE_MISSING_INDIRECT, AUTOFS_PTYPE_EXPIRE_INDIRECT,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_mount_types_powers_of_two() {
        let types = [
            AUTOFS_TYPE_INDIRECT, AUTOFS_TYPE_DIRECT,
            AUTOFS_TYPE_OFFSET,
        ];
        for t in &types {
            assert!(t.is_power_of_two(), "type {t} not power of 2");
        }
    }

    #[test]
    fn test_ioctls_distinct() {
        let ioctls = [
            AUTOFS_IOC_READY, AUTOFS_IOC_FAIL,
            AUTOFS_IOC_SETTIMEOUT, AUTOFS_IOC_PROTOVER,
            AUTOFS_IOC_EXPIRE, AUTOFS_IOC_ASKUMOUNT,
        ];
        for i in 0..ioctls.len() {
            for j in (i + 1)..ioctls.len() {
                assert_ne!(ioctls[i], ioctls[j]);
            }
        }
    }
}
