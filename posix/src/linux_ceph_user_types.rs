//! `<linux/ceph/ceph_fs.h>` — Ceph distributed filesystem user view.
//!
//! Ceph is a distributed object/file/block storage system. The kernel
//! ships a Ceph filesystem client and a RBD (Rados Block Device)
//! block driver. This module covers the protocol magic, the layered
//! object-namespace constants, and the file-layout limits used by
//! the userspace mount helper.

// ---------------------------------------------------------------------------
// Filesystem magic and signature
// ---------------------------------------------------------------------------

/// `CEPH_SUPER_MAGIC` returned by `statfs()`.
pub const CEPH_SUPER_MAGIC: u32 = 0x00c3_6400;

/// Ceph wire-protocol banner (Mon/OSD handshake).
pub const CEPH_BANNER: &str = "ceph v027";

/// Length of the wire banner.
pub const CEPH_BANNER_LEN: usize = 9;

// ---------------------------------------------------------------------------
// Object-name space sizes
// ---------------------------------------------------------------------------

/// Maximum object-name length (after layered hashing).
pub const CEPH_MAX_OID_NAME_LEN: usize = 64;

/// Maximum pool-name length.
pub const CEPH_MAX_POOL_NAME_LEN: usize = 64;

/// Maximum file-layout namespace string length.
pub const CEPH_MAX_NAMESPACE_LEN: usize = 64;

/// Maximum length of a Ceph snapshot name.
pub const CEPH_MAX_SNAP_NAME_LEN: usize = 64;

// ---------------------------------------------------------------------------
// File-layout defaults
// ---------------------------------------------------------------------------

/// Default object size (4 MiB).
pub const CEPH_DEFAULT_OBJECT_SIZE: u32 = 4 * 1024 * 1024;

/// Default stripe unit (4 MiB).
pub const CEPH_DEFAULT_STRIPE_UNIT: u32 = 4 * 1024 * 1024;

/// Default stripe count (1 — no striping unless reconfigured).
pub const CEPH_DEFAULT_STRIPE_COUNT: u32 = 1;

// ---------------------------------------------------------------------------
// Snapshot IDs (low values are reserved)
// ---------------------------------------------------------------------------

/// "no snap" sentinel.
pub const CEPH_NOSNAP: u64 = u64::MAX;

/// First valid snapshot ID.
pub const CEPH_SNAPDIR: u64 = u64::MAX - 1;

// ---------------------------------------------------------------------------
// Default ports
// ---------------------------------------------------------------------------

/// Ceph monitor (mon) default port.
pub const CEPH_MON_PORT: u16 = 6_789;

/// First OSD/MDS port (each increment is one daemon).
pub const CEPH_OSD_PORT_START: u16 = 6_800;

/// Last OSD/MDS port.
pub const CEPH_OSD_PORT_END: u16 = 7_300;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_magic_is_ceph_specific() {
        assert_eq!(CEPH_SUPER_MAGIC, 0x00c3_6400);
        // Magic is a 24-bit value with the high byte zero.
        assert_eq!(CEPH_SUPER_MAGIC >> 24, 0);
    }

    #[test]
    fn test_banner_length_consistent() {
        assert_eq!(CEPH_BANNER.len(), CEPH_BANNER_LEN);
        assert!(CEPH_BANNER.starts_with("ceph"));
    }

    #[test]
    fn test_name_length_uniform_64() {
        let lens = [
            CEPH_MAX_OID_NAME_LEN,
            CEPH_MAX_POOL_NAME_LEN,
            CEPH_MAX_NAMESPACE_LEN,
            CEPH_MAX_SNAP_NAME_LEN,
        ];
        for v in lens {
            assert_eq!(v, 64);
            assert!(v.is_power_of_two());
        }
    }

    #[test]
    fn test_default_layout_sane() {
        // 4 MiB objects, no striping by default.
        assert_eq!(CEPH_DEFAULT_OBJECT_SIZE, 4 * 1024 * 1024);
        assert_eq!(CEPH_DEFAULT_STRIPE_UNIT, CEPH_DEFAULT_OBJECT_SIZE);
        assert_eq!(CEPH_DEFAULT_STRIPE_COUNT, 1);
        // Layout invariant: stripe_unit divides object_size.
        assert_eq!(CEPH_DEFAULT_OBJECT_SIZE % CEPH_DEFAULT_STRIPE_UNIT, 0);
    }

    #[test]
    fn test_snapshot_sentinels() {
        // NOSNAP is all-ones, SNAPDIR is one below.
        assert_eq!(CEPH_NOSNAP, u64::MAX);
        assert_eq!(CEPH_SNAPDIR, u64::MAX - 1);
        assert!(CEPH_SNAPDIR < CEPH_NOSNAP);
    }

    #[test]
    fn test_port_ranges_well_known() {
        assert_eq!(CEPH_MON_PORT, 6_789);
        // OSDs live above the monitor port.
        assert!(CEPH_OSD_PORT_START > CEPH_MON_PORT);
        assert!(CEPH_OSD_PORT_END > CEPH_OSD_PORT_START);
        // 500 ports gives plenty of headroom (~250 OSDs).
        assert_eq!(CEPH_OSD_PORT_END - CEPH_OSD_PORT_START, 500);
    }
}
