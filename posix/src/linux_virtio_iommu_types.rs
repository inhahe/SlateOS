//! `<linux/virtio_iommu.h>` — virtio-iommu device constants.
//!
//! virtio-iommu is the standard paravirtualised IOMMU exposed to
//! guest VMs by QEMU, crosvm and cloud-hypervisor. Userspace
//! debuggers and VFIO-aware tools consume these feature, request,
//! and probe identifiers.

// ---------------------------------------------------------------------------
// Device feature bits (offsets within virtio config space feature bits)
// ---------------------------------------------------------------------------

/// Device supports range-based input-address invalidation.
pub const VIRTIO_IOMMU_F_INPUT_RANGE: u32 = 0;
/// Device exposes per-domain-range hints.
pub const VIRTIO_IOMMU_F_DOMAIN_RANGE: u32 = 1;
/// Device requires the driver to bypass IDs in a fixed range.
pub const VIRTIO_IOMMU_F_MAP_UNMAP: u32 = 2;
/// Device supports PROBE requests for endpoint properties.
pub const VIRTIO_IOMMU_F_BYPASS: u32 = 3;
/// Probe interface is supported.
pub const VIRTIO_IOMMU_F_PROBE: u32 = 4;
/// MMIO bypass for non-iommu endpoints.
pub const VIRTIO_IOMMU_F_MMIO: u32 = 5;
/// Bypass-config feature.
pub const VIRTIO_IOMMU_F_BYPASS_CONFIG: u32 = 6;

// ---------------------------------------------------------------------------
// Request types (struct virtio_iommu_req_head.type)
// ---------------------------------------------------------------------------

/// Attach endpoint to a domain.
pub const VIRTIO_IOMMU_T_ATTACH: u32 = 0x01;
/// Detach endpoint from its domain.
pub const VIRTIO_IOMMU_T_DETACH: u32 = 0x02;
/// Create an IOVA->phys mapping.
pub const VIRTIO_IOMMU_T_MAP: u32 = 0x03;
/// Remove an IOVA->phys mapping.
pub const VIRTIO_IOMMU_T_UNMAP: u32 = 0x04;
/// Probe endpoint for capabilities.
pub const VIRTIO_IOMMU_T_PROBE: u32 = 0x05;

// ---------------------------------------------------------------------------
// Request status codes (struct virtio_iommu_req_tail.status)
// ---------------------------------------------------------------------------

/// Successful completion.
pub const VIRTIO_IOMMU_S_OK: u32 = 0;
/// Unrecognised request type.
pub const VIRTIO_IOMMU_S_IOERR: u32 = 1;
/// Request not supported.
pub const VIRTIO_IOMMU_S_UNSUPP: u32 = 2;
/// Device error.
pub const VIRTIO_IOMMU_S_DEVERR: u32 = 3;
/// Request invalid.
pub const VIRTIO_IOMMU_S_INVAL: u32 = 4;
/// Operation out of range.
pub const VIRTIO_IOMMU_S_RANGE: u32 = 5;
/// Endpoint not found.
pub const VIRTIO_IOMMU_S_NOENT: u32 = 6;
/// Endpoint busy.
pub const VIRTIO_IOMMU_S_FAULT: u32 = 7;
/// No memory.
pub const VIRTIO_IOMMU_S_NOMEM: u32 = 8;

// ---------------------------------------------------------------------------
// Map flags (struct virtio_iommu_req_map.flags)
// ---------------------------------------------------------------------------

/// Mapping is readable.
pub const VIRTIO_IOMMU_MAP_F_READ: u32 = 1 << 0;
/// Mapping is writable.
pub const VIRTIO_IOMMU_MAP_F_WRITE: u32 = 1 << 1;
/// Mapping is executable.
pub const VIRTIO_IOMMU_MAP_F_MMIO: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// Probe property type codes
// ---------------------------------------------------------------------------

/// End-of-probe-properties marker.
pub const VIRTIO_IOMMU_PROBE_T_NONE: u32 = 0;
/// Reserved-memory regions.
pub const VIRTIO_IOMMU_PROBE_T_RESV_MEM: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_bits_distinct() {
        let feats = [
            VIRTIO_IOMMU_F_INPUT_RANGE,
            VIRTIO_IOMMU_F_DOMAIN_RANGE,
            VIRTIO_IOMMU_F_MAP_UNMAP,
            VIRTIO_IOMMU_F_BYPASS,
            VIRTIO_IOMMU_F_PROBE,
            VIRTIO_IOMMU_F_MMIO,
            VIRTIO_IOMMU_F_BYPASS_CONFIG,
        ];
        for i in 0..feats.len() {
            for j in (i + 1)..feats.len() {
                assert_ne!(feats[i], feats[j]);
            }
        }
    }

    #[test]
    fn test_req_types_distinct() {
        let reqs = [
            VIRTIO_IOMMU_T_ATTACH,
            VIRTIO_IOMMU_T_DETACH,
            VIRTIO_IOMMU_T_MAP,
            VIRTIO_IOMMU_T_UNMAP,
            VIRTIO_IOMMU_T_PROBE,
        ];
        for i in 0..reqs.len() {
            for j in (i + 1)..reqs.len() {
                assert_ne!(reqs[i], reqs[j]);
            }
        }
    }

    #[test]
    fn test_status_codes_distinct_and_ok_is_zero() {
        assert_eq!(VIRTIO_IOMMU_S_OK, 0);
        let codes = [
            VIRTIO_IOMMU_S_OK,
            VIRTIO_IOMMU_S_IOERR,
            VIRTIO_IOMMU_S_UNSUPP,
            VIRTIO_IOMMU_S_DEVERR,
            VIRTIO_IOMMU_S_INVAL,
            VIRTIO_IOMMU_S_RANGE,
            VIRTIO_IOMMU_S_NOENT,
            VIRTIO_IOMMU_S_FAULT,
            VIRTIO_IOMMU_S_NOMEM,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_map_flags_distinct_powers_of_two() {
        let flags = [
            VIRTIO_IOMMU_MAP_F_READ,
            VIRTIO_IOMMU_MAP_F_WRITE,
            VIRTIO_IOMMU_MAP_F_MMIO,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two());
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_probe_types_distinct() {
        assert_eq!(VIRTIO_IOMMU_PROBE_T_NONE, 0);
        assert_ne!(VIRTIO_IOMMU_PROBE_T_NONE, VIRTIO_IOMMU_PROBE_T_RESV_MEM);
    }
}
