//! `<linux/balloon_compaction.h>` — Memory balloon constants.
//!
//! Memory ballooning is a virtualization technique where the guest OS
//! "inflates" a balloon by allocating pages and reporting them to the
//! hypervisor, which reclaims the underlying physical memory for other
//! VMs. Deflating releases pages back to the guest. This allows dynamic
//! memory sharing between VMs without hardware-level memory hotplug.
//! Balloon drivers exist for virtio (KVM/QEMU), Hyper-V, VMware, and Xen.

// ---------------------------------------------------------------------------
// Balloon states
// ---------------------------------------------------------------------------

/// Balloon is inactive (no inflation).
pub const BALLOON_STATE_INACTIVE: u32 = 0;
/// Balloon is inflating (allocating pages to give to hypervisor).
pub const BALLOON_STATE_INFLATING: u32 = 1;
/// Balloon is deflating (reclaiming pages from hypervisor).
pub const BALLOON_STATE_DEFLATING: u32 = 2;
/// Balloon is at target size (stable).
pub const BALLOON_STATE_STABLE: u32 = 3;

// ---------------------------------------------------------------------------
// Balloon page flags
// ---------------------------------------------------------------------------

/// Page is in the balloon (not available to guest).
pub const BALLOON_PAGE_ENQUEUED: u32 = 0x01;
/// Page is being migrated (compaction support).
pub const BALLOON_PAGE_MIGRATING: u32 = 0x02;
/// Page is isolated for migration.
pub const BALLOON_PAGE_ISOLATED: u32 = 0x04;

// ---------------------------------------------------------------------------
// Balloon compaction
// ---------------------------------------------------------------------------

/// Balloon supports compaction (page migration).
pub const BALLOON_COMPACTION_SUPPORTED: u32 = 1;
/// Balloon does not support compaction.
pub const BALLOON_COMPACTION_UNSUPPORTED: u32 = 0;

// ---------------------------------------------------------------------------
// Virtio balloon feature bits
// ---------------------------------------------------------------------------

/// Must-tell-host feature (report all inflated pages).
pub const VIRTIO_BALLOON_F_MUST_TELL_HOST: u32 = 0;
/// Stats VQ feature (host can request memory stats).
pub const VIRTIO_BALLOON_F_STATS_VQ: u32 = 1;
/// Deflate-on-OOM feature (auto-deflate on memory pressure).
pub const VIRTIO_BALLOON_F_DEFLATE_ON_OOM: u32 = 2;
/// Free page reporting feature (proactively report free pages).
pub const VIRTIO_BALLOON_F_FREE_PAGE_HINT: u32 = 3;
/// Page poison feature (poisoned pages can be skipped).
pub const VIRTIO_BALLOON_F_PAGE_POISON: u32 = 4;
/// Reporting VQ feature (free page reporting via dedicated VQ).
pub const VIRTIO_BALLOON_F_REPORTING: u32 = 5;

// ---------------------------------------------------------------------------
// Virtio balloon stat tags
// ---------------------------------------------------------------------------

/// Swap in (pages read from swap).
pub const VIRTIO_BALLOON_S_SWAP_IN: u32 = 0;
/// Swap out (pages written to swap).
pub const VIRTIO_BALLOON_S_SWAP_OUT: u32 = 1;
/// Major page faults.
pub const VIRTIO_BALLOON_S_MAJFLT: u32 = 2;
/// Minor page faults.
pub const VIRTIO_BALLOON_S_MINFLT: u32 = 3;
/// Total memory (in bytes).
pub const VIRTIO_BALLOON_S_MEMTOT: u32 = 4;
/// Free memory (in bytes).
pub const VIRTIO_BALLOON_S_MEMFREE: u32 = 5;
/// Available memory (including reclaimable caches).
pub const VIRTIO_BALLOON_S_AVAIL: u32 = 6;
/// Disk caches.
pub const VIRTIO_BALLOON_S_CACHES: u32 = 7;
/// Hugetlb allocations.
pub const VIRTIO_BALLOON_S_HTLB_PGALLOC: u32 = 8;
/// Hugetlb allocation failures.
pub const VIRTIO_BALLOON_S_HTLB_PGFAIL: u32 = 9;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_states_distinct() {
        let states = [
            BALLOON_STATE_INACTIVE, BALLOON_STATE_INFLATING,
            BALLOON_STATE_DEFLATING, BALLOON_STATE_STABLE,
        ];
        for i in 0..states.len() {
            for j in (i + 1)..states.len() {
                assert_ne!(states[i], states[j]);
            }
        }
    }

    #[test]
    fn test_page_flags_no_overlap() {
        let flags = [
            BALLOON_PAGE_ENQUEUED, BALLOON_PAGE_MIGRATING,
            BALLOON_PAGE_ISOLATED,
        ];
        for i in 0..flags.len() {
            assert!(flags[i].is_power_of_two());
            for j in (i + 1)..flags.len() {
                assert_eq!(flags[i] & flags[j], 0);
            }
        }
    }

    #[test]
    fn test_feature_bits_distinct() {
        let features = [
            VIRTIO_BALLOON_F_MUST_TELL_HOST, VIRTIO_BALLOON_F_STATS_VQ,
            VIRTIO_BALLOON_F_DEFLATE_ON_OOM, VIRTIO_BALLOON_F_FREE_PAGE_HINT,
            VIRTIO_BALLOON_F_PAGE_POISON, VIRTIO_BALLOON_F_REPORTING,
        ];
        for i in 0..features.len() {
            for j in (i + 1)..features.len() {
                assert_ne!(features[i], features[j]);
            }
        }
    }

    #[test]
    fn test_stat_tags_distinct() {
        let tags = [
            VIRTIO_BALLOON_S_SWAP_IN, VIRTIO_BALLOON_S_SWAP_OUT,
            VIRTIO_BALLOON_S_MAJFLT, VIRTIO_BALLOON_S_MINFLT,
            VIRTIO_BALLOON_S_MEMTOT, VIRTIO_BALLOON_S_MEMFREE,
            VIRTIO_BALLOON_S_AVAIL, VIRTIO_BALLOON_S_CACHES,
            VIRTIO_BALLOON_S_HTLB_PGALLOC, VIRTIO_BALLOON_S_HTLB_PGFAIL,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }
}
