//! `<linux/virtio_balloon.h>` — VirtIO memory balloon constants.
//!
//! The virtio-balloon device allows the host to dynamically reclaim
//! memory from a guest by inflating (requesting pages) or deflating
//! (returning pages) a memory balloon. This enables memory overcommit
//! and dynamic resource balancing across VMs.

// ---------------------------------------------------------------------------
// Balloon feature bits
// ---------------------------------------------------------------------------

/// Device tells guest actual memory available.
pub const VIRTIO_BALLOON_F_MUST_TELL_HOST: u64 = 1 << 0;
/// Device reports free page statistics.
pub const VIRTIO_BALLOON_F_STATS_VQ: u64 = 1 << 1;
/// Device supports deflate on OOM.
pub const VIRTIO_BALLOON_F_DEFLATE_ON_OOM: u64 = 1 << 2;
/// Device supports free page reporting.
pub const VIRTIO_BALLOON_F_FREE_PAGE_HINT: u64 = 1 << 3;
/// Device supports page poison tracking.
pub const VIRTIO_BALLOON_F_PAGE_POISON: u64 = 1 << 4;
/// Device supports page reporting.
pub const VIRTIO_BALLOON_F_REPORTING: u64 = 1 << 5;

// ---------------------------------------------------------------------------
// Balloon virtqueue indices
// ---------------------------------------------------------------------------

/// Inflate queue (guest → host: pages to reclaim).
pub const VIRTIO_BALLOON_VQ_INFLATE: u32 = 0;
/// Deflate queue (host → guest: return pages).
pub const VIRTIO_BALLOON_VQ_DEFLATE: u32 = 1;
/// Statistics queue (guest → host: memory stats).
pub const VIRTIO_BALLOON_VQ_STATS: u32 = 2;
/// Free page hint queue.
pub const VIRTIO_BALLOON_VQ_FREE_PAGE: u32 = 3;
/// Page reporting queue.
pub const VIRTIO_BALLOON_VQ_REPORTING: u32 = 4;

// ---------------------------------------------------------------------------
// Balloon statistics tags
// ---------------------------------------------------------------------------

/// Swap in (pages swapped in from disk).
pub const VIRTIO_BALLOON_S_SWAP_IN: u16 = 0;
/// Swap out (pages swapped out to disk).
pub const VIRTIO_BALLOON_S_SWAP_OUT: u16 = 1;
/// Major page faults.
pub const VIRTIO_BALLOON_S_MAJFLT: u16 = 2;
/// Minor page faults.
pub const VIRTIO_BALLOON_S_MINFLT: u16 = 3;
/// Total memory available to guest.
pub const VIRTIO_BALLOON_S_MEMFREE: u16 = 4;
/// Total memory.
pub const VIRTIO_BALLOON_S_MEMTOT: u16 = 5;
/// Available memory (free + reclaimable).
pub const VIRTIO_BALLOON_S_AVAIL: u16 = 6;
/// Caches (page cache size).
pub const VIRTIO_BALLOON_S_CACHES: u16 = 7;
/// HugePages total.
pub const VIRTIO_BALLOON_S_HTLB_PGALLOC: u16 = 8;
/// HugePages failed.
pub const VIRTIO_BALLOON_S_HTLB_PGFAIL: u16 = 9;
/// Number of stat tags.
pub const VIRTIO_BALLOON_S_NR: u16 = 10;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_no_overlap() {
        let feats = [
            VIRTIO_BALLOON_F_MUST_TELL_HOST,
            VIRTIO_BALLOON_F_STATS_VQ,
            VIRTIO_BALLOON_F_DEFLATE_ON_OOM,
            VIRTIO_BALLOON_F_FREE_PAGE_HINT,
            VIRTIO_BALLOON_F_PAGE_POISON,
            VIRTIO_BALLOON_F_REPORTING,
        ];
        for i in 0..feats.len() {
            assert!(feats[i].is_power_of_two());
            for j in (i + 1)..feats.len() {
                assert_eq!(feats[i] & feats[j], 0);
            }
        }
    }

    #[test]
    fn test_vq_indices_distinct() {
        let vqs = [
            VIRTIO_BALLOON_VQ_INFLATE,
            VIRTIO_BALLOON_VQ_DEFLATE,
            VIRTIO_BALLOON_VQ_STATS,
            VIRTIO_BALLOON_VQ_FREE_PAGE,
            VIRTIO_BALLOON_VQ_REPORTING,
        ];
        for i in 0..vqs.len() {
            for j in (i + 1)..vqs.len() {
                assert_ne!(vqs[i], vqs[j]);
            }
        }
    }

    #[test]
    fn test_stat_tags_sequential() {
        assert_eq!(VIRTIO_BALLOON_S_SWAP_IN, 0);
        assert_eq!(VIRTIO_BALLOON_S_NR, 10);
    }
}
