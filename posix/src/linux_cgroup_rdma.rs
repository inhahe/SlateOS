//! `<linux/cgroup_rdma.h>` — cgroup RDMA controller constants.
//!
//! The RDMA cgroup controller limits RDMA/InfiniBand resource
//! usage per cgroup: maximum number of protection domains, memory
//! regions, completion queues, queue pairs, etc. Works with both
//! InfiniBand verbs and iWARP (RDMA over Ethernet).

// ---------------------------------------------------------------------------
// RDMA resource types
// ---------------------------------------------------------------------------

/// HCA handle count.
pub const RDMACG_RESOURCE_HCA_HANDLE: u32 = 0;
/// HCA object count.
pub const RDMACG_RESOURCE_HCA_OBJECT: u32 = 1;
/// Maximum resource type count.
pub const RDMACG_RESOURCE_MAX: u32 = 2;

// ---------------------------------------------------------------------------
// Specific RDMA resources (for per-device limits)
// ---------------------------------------------------------------------------

/// Protection domains.
pub const RDMA_RESOURCE_PD: u32 = 0;
/// Address handles.
pub const RDMA_RESOURCE_AH: u32 = 1;
/// Completion queues.
pub const RDMA_RESOURCE_CQ: u32 = 2;
/// Memory regions.
pub const RDMA_RESOURCE_MR: u32 = 3;
/// Queue pairs.
pub const RDMA_RESOURCE_QP: u32 = 4;
/// Shared receive queues.
pub const RDMA_RESOURCE_SRQ: u32 = 5;
/// Memory windows.
pub const RDMA_RESOURCE_MW: u32 = 6;
/// Number of RDMA resource types.
pub const RDMA_RESOURCE_MAX: u32 = 7;

// ---------------------------------------------------------------------------
// Resource names (for sysfs/cgroup files)
// ---------------------------------------------------------------------------

/// PD resource name.
pub const RDMA_RESOURCE_PD_NAME: &str = "pd";
/// AH resource name.
pub const RDMA_RESOURCE_AH_NAME: &str = "ah";
/// CQ resource name.
pub const RDMA_RESOURCE_CQ_NAME: &str = "cq";
/// MR resource name.
pub const RDMA_RESOURCE_MR_NAME: &str = "mr";
/// QP resource name.
pub const RDMA_RESOURCE_QP_NAME: &str = "qp";
/// SRQ resource name.
pub const RDMA_RESOURCE_SRQ_NAME: &str = "srq";
/// MW resource name.
pub const RDMA_RESOURCE_MW_NAME: &str = "mw";

// ---------------------------------------------------------------------------
// Limit sentinel
// ---------------------------------------------------------------------------

/// Unlimited resource.
pub const RDMA_RESOURCE_UNLIMITED: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resource_types_distinct() {
        let types = [RDMACG_RESOURCE_HCA_HANDLE, RDMACG_RESOURCE_HCA_OBJECT];
        for i in 0..types.len() {
            for j in (i + 1)..types.len() {
                assert_ne!(types[i], types[j]);
            }
        }
    }

    #[test]
    fn test_rdma_resources_distinct() {
        let res = [
            RDMA_RESOURCE_PD, RDMA_RESOURCE_AH, RDMA_RESOURCE_CQ,
            RDMA_RESOURCE_MR, RDMA_RESOURCE_QP, RDMA_RESOURCE_SRQ,
            RDMA_RESOURCE_MW,
        ];
        for i in 0..res.len() {
            for j in (i + 1)..res.len() {
                assert_ne!(res[i], res[j]);
            }
        }
    }

    #[test]
    fn test_resources_below_max() {
        let res = [
            RDMA_RESOURCE_PD, RDMA_RESOURCE_AH, RDMA_RESOURCE_CQ,
            RDMA_RESOURCE_MR, RDMA_RESOURCE_QP, RDMA_RESOURCE_SRQ,
            RDMA_RESOURCE_MW,
        ];
        for r in &res {
            assert!(*r < RDMA_RESOURCE_MAX);
        }
    }

    #[test]
    fn test_resource_names_distinct() {
        let names = [
            RDMA_RESOURCE_PD_NAME, RDMA_RESOURCE_AH_NAME,
            RDMA_RESOURCE_CQ_NAME, RDMA_RESOURCE_MR_NAME,
            RDMA_RESOURCE_QP_NAME, RDMA_RESOURCE_SRQ_NAME,
            RDMA_RESOURCE_MW_NAME,
        ];
        for i in 0..names.len() {
            for j in (i + 1)..names.len() {
                assert_ne!(names[i], names[j]);
            }
        }
    }

    #[test]
    fn test_unlimited() {
        assert_eq!(RDMA_RESOURCE_UNLIMITED, u32::MAX);
    }
}
