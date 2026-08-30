//! `<linux/nvme.h>` (namespace subset) — NVMe namespace constants.
//!
//! An NVMe namespace is a collection of logical blocks that can be
//! formatted, attached to controllers, and managed independently.
//! Each namespace has an ID (NSID), a format (LBA size, metadata),
//! and optional features like thin provisioning, data protection,
//! and multi-path access.

// ---------------------------------------------------------------------------
// Namespace IDs
// ---------------------------------------------------------------------------

/// Broadcast NSID (all namespaces).
pub const NVME_NSID_ALL: u32 = 0xFFFF_FFFF;
/// Minimum valid NSID.
pub const NVME_NSID_MIN: u32 = 1;

// ---------------------------------------------------------------------------
// Namespace features (NSFEAT field in Identify Namespace)
// ---------------------------------------------------------------------------

/// Namespace supports thin provisioning.
pub const NVME_NSFEAT_THIN_PROV: u32 = 1 << 0;
/// Namespace supports atomic write unit (NAWUN/NAWUPF).
pub const NVME_NSFEAT_ATOMICS: u32 = 1 << 1;
/// Namespace supports deallocated/unwritten logical block error.
pub const NVME_NSFEAT_DEALLOC: u32 = 1 << 2;
/// Namespace supports NGUID field.
pub const NVME_NSFEAT_GUID_REUSE: u32 = 1 << 3;
/// Namespace supports optimal I/O boundary.
pub const NVME_NSFEAT_IO_OPT: u32 = 1 << 4;

// ---------------------------------------------------------------------------
// LBA format data sizes (common, in bytes)
// ---------------------------------------------------------------------------

/// 512-byte LBA.
pub const NVME_LBA_SIZE_512: u32 = 512;
/// 4096-byte LBA (4K native).
pub const NVME_LBA_SIZE_4096: u32 = 4096;

// ---------------------------------------------------------------------------
// Data protection types (end-to-end protection)
// ---------------------------------------------------------------------------

/// No data protection.
pub const NVME_DPS_NONE: u32 = 0;
/// Type 1 protection (guard, app tag, ref tag).
pub const NVME_DPS_TYPE1: u32 = 1;
/// Type 2 protection (guard, app tag, ref tag).
pub const NVME_DPS_TYPE2: u32 = 2;
/// Type 3 protection (guard, app tag only).
pub const NVME_DPS_TYPE3: u32 = 3;
/// Protection info at first 8 bytes of metadata.
pub const NVME_DPS_FIRST_EIGHT: u32 = 1 << 3;

// ---------------------------------------------------------------------------
// Namespace management actions
// ---------------------------------------------------------------------------

/// Create namespace.
pub const NVME_NS_MGMT_CREATE: u32 = 0;
/// Delete namespace.
pub const NVME_NS_MGMT_DELETE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broadcast_nsid() {
        assert_eq!(NVME_NSID_ALL, u32::MAX);
    }

    #[test]
    fn test_nsid_min() {
        assert_eq!(NVME_NSID_MIN, 1);
    }

    #[test]
    fn test_nsfeat_flags_no_overlap() {
        let flags = [
            NVME_NSFEAT_THIN_PROV,
            NVME_NSFEAT_ATOMICS,
            NVME_NSFEAT_DEALLOC,
            NVME_NSFEAT_GUID_REUSE,
            NVME_NSFEAT_IO_OPT,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_lba_sizes() {
        assert!(NVME_LBA_SIZE_512.is_power_of_two());
        assert!(NVME_LBA_SIZE_4096.is_power_of_two());
        assert!(NVME_LBA_SIZE_512 < NVME_LBA_SIZE_4096);
    }

    #[test]
    fn test_dps_types_distinct() {
        let dps = [
            NVME_DPS_NONE,
            NVME_DPS_TYPE1,
            NVME_DPS_TYPE2,
            NVME_DPS_TYPE3,
        ];
        for i in 0..dps.len() {
            for j in (i + 1)..dps.len() {
                assert_ne!(dps[i], dps[j]);
            }
        }
    }

    #[test]
    fn test_ns_mgmt_actions() {
        assert_ne!(NVME_NS_MGMT_CREATE, NVME_NS_MGMT_DELETE);
    }
}
