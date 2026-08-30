//! `<linux/kfd_ioctl.h>` — AMD Kernel Fusion Driver constants.
//!
//! Constants for the AMD KFD (Kernel Fusion Driver) userspace
//! interface — used by ROCm to allocate GPU compute queues, shared
//! virtual memory, and HSA dispatch packets on AMD APUs/dGPUs.

// ---------------------------------------------------------------------------
// KFD interface version
// ---------------------------------------------------------------------------

/// Major version reported by KFD_IOC_GET_VERSION.
pub const KFD_IOCTL_MAJOR_VERSION: u32 = 1;
/// Minimum minor version the userspace library will accept.
pub const KFD_IOCTL_MINOR_VERSION: u32 = 14;

// ---------------------------------------------------------------------------
// Queue types (for KFD_IOC_CREATE_QUEUE)
// ---------------------------------------------------------------------------

/// Compute queue.
pub const KFD_IOC_QUEUE_TYPE_COMPUTE: u32 = 0;
/// SDMA (System DMA) queue.
pub const KFD_IOC_QUEUE_TYPE_SDMA: u32 = 1;
/// AQL (Architected Queueing Language) compute queue.
pub const KFD_IOC_QUEUE_TYPE_COMPUTE_AQL: u32 = 2;
/// AQL SDMA queue.
pub const KFD_IOC_QUEUE_TYPE_SDMA_XGMI: u32 = 3;

// ---------------------------------------------------------------------------
// Memory allocation flags (svm / ioctl_alloc_memory_of_gpu)
// ---------------------------------------------------------------------------

/// Allocate from VRAM.
pub const KFD_IOC_ALLOC_MEM_FLAGS_VRAM: u32 = 0x0000_0001;
/// Allocate from system memory (GTT).
pub const KFD_IOC_ALLOC_MEM_FLAGS_GTT: u32 = 0x0000_0002;
/// Allocate from userptr region.
pub const KFD_IOC_ALLOC_MEM_FLAGS_USERPTR: u32 = 0x0000_0004;
/// Allocate MMIO doorbell region.
pub const KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL: u32 = 0x0000_0008;
/// Allocate MMIO remap region.
pub const KFD_IOC_ALLOC_MEM_FLAGS_MMIO_REMAP: u32 = 0x0000_0010;
/// Writable mapping.
pub const KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE: u32 = 0x8000_0000;
/// Executable mapping.
pub const KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE: u32 = 0x4000_0000;
/// Public (shareable) mapping.
pub const KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC: u32 = 0x2000_0000;
/// No-substitute (must come from requested pool).
pub const KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE: u32 = 0x1000_0000;
/// AQL-queue memory.
pub const KFD_IOC_ALLOC_MEM_FLAGS_AQL_QUEUE_MEM: u32 = 0x0800_0000;
/// Coherent system memory.
pub const KFD_IOC_ALLOC_MEM_FLAGS_COHERENT: u32 = 0x0400_0000;
/// Uncached mapping.
pub const KFD_IOC_ALLOC_MEM_FLAGS_UNCACHED: u32 = 0x0200_0000;

// ---------------------------------------------------------------------------
// Process scheduling policies
// ---------------------------------------------------------------------------

/// Hardware-managed scheduler.
pub const KFD_IOC_SCHED_POLICY_HWS: u32 = 0;
/// HWS with over-subscription.
pub const KFD_IOC_SCHED_POLICY_HWS_OVER_SUBSCRIPTION: u32 = 1;
/// Software-managed scheduler.
pub const KFD_IOC_SCHED_POLICY_NO_HWS: u32 = 2;

// ---------------------------------------------------------------------------
// Cache types reported by topology queries
// ---------------------------------------------------------------------------

/// Data cache.
pub const KFD_IOC_CACHE_POLICY_COHERENT: u32 = 0;
/// Instruction cache.
pub const KFD_IOC_CACHE_POLICY_NONCOHERENT: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_components() {
        assert_eq!(KFD_IOCTL_MAJOR_VERSION, 1);
        assert!(KFD_IOCTL_MINOR_VERSION >= 1);
    }

    #[test]
    fn test_queue_types_distinct() {
        let qts = [
            KFD_IOC_QUEUE_TYPE_COMPUTE,
            KFD_IOC_QUEUE_TYPE_SDMA,
            KFD_IOC_QUEUE_TYPE_COMPUTE_AQL,
            KFD_IOC_QUEUE_TYPE_SDMA_XGMI,
        ];
        for i in 0..qts.len() {
            for j in (i + 1)..qts.len() {
                assert_ne!(qts[i], qts[j]);
            }
        }
    }

    #[test]
    fn test_alloc_flags_distinct_bits() {
        let flags = [
            KFD_IOC_ALLOC_MEM_FLAGS_VRAM,
            KFD_IOC_ALLOC_MEM_FLAGS_GTT,
            KFD_IOC_ALLOC_MEM_FLAGS_USERPTR,
            KFD_IOC_ALLOC_MEM_FLAGS_DOORBELL,
            KFD_IOC_ALLOC_MEM_FLAGS_MMIO_REMAP,
            KFD_IOC_ALLOC_MEM_FLAGS_WRITABLE,
            KFD_IOC_ALLOC_MEM_FLAGS_EXECUTABLE,
            KFD_IOC_ALLOC_MEM_FLAGS_PUBLIC,
            KFD_IOC_ALLOC_MEM_FLAGS_NO_SUBSTITUTE,
            KFD_IOC_ALLOC_MEM_FLAGS_AQL_QUEUE_MEM,
            KFD_IOC_ALLOC_MEM_FLAGS_COHERENT,
            KFD_IOC_ALLOC_MEM_FLAGS_UNCACHED,
        ];
        for &f in &flags {
            assert!(f.is_power_of_two(), "{f:#x} is not a single bit");
        }
        for i in 0..flags.len() {
            for j in (i + 1)..flags.len() {
                assert_ne!(flags[i], flags[j]);
            }
        }
    }

    #[test]
    fn test_sched_policies_distinct() {
        let polices = [
            KFD_IOC_SCHED_POLICY_HWS,
            KFD_IOC_SCHED_POLICY_HWS_OVER_SUBSCRIPTION,
            KFD_IOC_SCHED_POLICY_NO_HWS,
        ];
        for i in 0..polices.len() {
            for j in (i + 1)..polices.len() {
                assert_ne!(polices[i], polices[j]);
            }
        }
    }

    #[test]
    fn test_cache_policies_distinct() {
        assert_ne!(
            KFD_IOC_CACHE_POLICY_COHERENT,
            KFD_IOC_CACHE_POLICY_NONCOHERENT
        );
    }
}
