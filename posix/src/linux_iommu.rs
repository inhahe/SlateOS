//! `<linux/iommu.h>` — IOMMU constants and types.
//!
//! The IOMMU (Input/Output Memory Management Unit) subsystem provides
//! DMA address translation and device isolation. Used for device
//! passthrough to VMs (VFIO), DMA remapping, and security isolation
//! of device memory accesses.

// ---------------------------------------------------------------------------
// IOMMU fault types
// ---------------------------------------------------------------------------

/// DMA unrecoverable fault.
pub const IOMMU_FAULT_DMA_UNRECOV: u32 = 1;
/// Page request.
pub const IOMMU_FAULT_PAGE_REQ: u32 = 2;

// ---------------------------------------------------------------------------
// IOMMU fault reason codes
// ---------------------------------------------------------------------------

/// Unknown fault.
pub const IOMMU_FAULT_REASON_UNKNOWN: u32 = 0;
/// PTE fetch fault.
pub const IOMMU_FAULT_REASON_PTE_FETCH: u32 = 1;
/// Permission violation.
pub const IOMMU_FAULT_REASON_PERMISSION: u32 = 2;
/// Access violation.
pub const IOMMU_FAULT_REASON_ACCESS: u32 = 3;

// ---------------------------------------------------------------------------
// IOMMU page response codes
// ---------------------------------------------------------------------------

/// Invalid response.
pub const IOMMU_PAGE_RESP_INVALID: u32 = 0;
/// Success.
pub const IOMMU_PAGE_RESP_SUCCESS: u32 = 1;
/// Failure.
pub const IOMMU_PAGE_RESP_FAILURE: u32 = 2;

// ---------------------------------------------------------------------------
// IOMMU protection flags (for iommu_map)
// ---------------------------------------------------------------------------

/// Readable mapping.
pub const IOMMU_READ: u32 = 1 << 0;
/// Writable mapping.
pub const IOMMU_WRITE: u32 = 1 << 1;
/// Cacheable mapping.
pub const IOMMU_CACHE: u32 = 1 << 2;
/// Non-coherent DMA.
pub const IOMMU_NOEXEC: u32 = 1 << 3;
/// MMIO mapping.
pub const IOMMU_MMIO: u32 = 1 << 4;
/// Privileged access.
pub const IOMMU_PRIV: u32 = 1 << 5;

// ---------------------------------------------------------------------------
// IOMMU domain types
// ---------------------------------------------------------------------------

/// Blocked domain (no access).
pub const IOMMU_DOMAIN_BLOCKED: u32 = 0;
/// Identity (pass-through) domain.
pub const IOMMU_DOMAIN_IDENTITY: u32 = 1;
/// Unmanaged domain (for VFIO).
pub const IOMMU_DOMAIN_UNMANAGED: u32 = 2;
/// DMA domain (default for drivers).
pub const IOMMU_DOMAIN_DMA: u32 = 3;
/// DMA-FQ domain (flush queue for batched unmaps).
pub const IOMMU_DOMAIN_DMA_FQ: u32 = 4;
/// SVA domain (shared virtual addressing).
pub const IOMMU_DOMAIN_SVA: u32 = 5;

// ---------------------------------------------------------------------------
// IOMMU capabilities
// ---------------------------------------------------------------------------

/// Supports cache coherency.
pub const IOMMU_CAP_CACHE_COHERENCY: u32 = 0;
/// Supports interrupt remapping.
pub const IOMMU_CAP_INTR_REMAP: u32 = 1;
/// No page request support needed.
pub const IOMMU_CAP_NOEXEC: u32 = 2;
/// Enforces dirty tracking.
pub const IOMMU_CAP_DIRTY_TRACKING: u32 = 3;

// ---------------------------------------------------------------------------
// IOMMU device feature flags
// ---------------------------------------------------------------------------

/// SVA (Shared Virtual Addressing) support.
pub const IOMMU_DEV_FEAT_SVA: u32 = 0;
/// IOPF (I/O Page Fault) support.
pub const IOMMU_DEV_FEAT_IOPF: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fault_types_distinct() {
        assert_ne!(IOMMU_FAULT_DMA_UNRECOV, IOMMU_FAULT_PAGE_REQ);
    }

    #[test]
    fn test_fault_reasons_distinct() {
        let reasons = [
            IOMMU_FAULT_REASON_UNKNOWN, IOMMU_FAULT_REASON_PTE_FETCH,
            IOMMU_FAULT_REASON_PERMISSION, IOMMU_FAULT_REASON_ACCESS,
        ];
        for i in 0..reasons.len() {
            for j in (i + 1)..reasons.len() {
                assert_ne!(reasons[i], reasons[j]);
            }
        }
    }

    #[test]
    fn test_page_resp_codes_distinct() {
        let codes = [
            IOMMU_PAGE_RESP_INVALID, IOMMU_PAGE_RESP_SUCCESS,
            IOMMU_PAGE_RESP_FAILURE,
        ];
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j]);
            }
        }
    }

    #[test]
    fn test_prot_flags_powers_of_two() {
        let flags = [
            IOMMU_READ, IOMMU_WRITE, IOMMU_CACHE,
            IOMMU_NOEXEC, IOMMU_MMIO, IOMMU_PRIV,
        ];
        for flag in &flags {
            assert!(flag.is_power_of_two(), "0x{:x}", flag);
        }
    }

    #[test]
    fn test_prot_flags_no_overlap() {
        let flags = [
            IOMMU_READ, IOMMU_WRITE, IOMMU_CACHE,
            IOMMU_NOEXEC, IOMMU_MMIO, IOMMU_PRIV,
        ];
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_domain_types_distinct() {
        let types = [
            IOMMU_DOMAIN_BLOCKED, IOMMU_DOMAIN_IDENTITY,
            IOMMU_DOMAIN_UNMANAGED, IOMMU_DOMAIN_DMA,
            IOMMU_DOMAIN_DMA_FQ, IOMMU_DOMAIN_SVA,
        ];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_capabilities_distinct() {
        let caps = [
            IOMMU_CAP_CACHE_COHERENCY, IOMMU_CAP_INTR_REMAP,
            IOMMU_CAP_NOEXEC, IOMMU_CAP_DIRTY_TRACKING,
        ];
        for i in 0..caps.len() {
            for j in (i + 1)..caps.len() {
                assert_ne!(caps[i], caps[j]);
            }
        }
    }

    #[test]
    fn test_dev_features_distinct() {
        assert_ne!(IOMMU_DEV_FEAT_SVA, IOMMU_DEV_FEAT_IOPF);
    }
}
