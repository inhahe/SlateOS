//! `<linux/virtio_balloon.h>` — Virtio memory balloon constants.
//!
//! The virtio balloon device allows the host to reclaim guest
//! memory dynamically. The guest inflates the balloon (returning
//! pages to the host) or deflates it (reclaiming pages). Used
//! for memory overcommit in hypervisors.

// ---------------------------------------------------------------------------
// Feature bits
// ---------------------------------------------------------------------------

/// Device tells guest actual memory usage.
pub const VIRTIO_BALLOON_F_MUST_TELL_HOST: u32 = 0;
/// Device supports memory statistics reporting.
pub const VIRTIO_BALLOON_F_STATS_VQ: u32 = 1;
/// Device supports deflate on OOM.
pub const VIRTIO_BALLOON_F_DEFLATE_ON_OOM: u32 = 2;
/// Device supports free page hinting.
pub const VIRTIO_BALLOON_F_FREE_PAGE_HINT: u32 = 3;
/// Device supports page poison tracking.
pub const VIRTIO_BALLOON_F_PAGE_POISON: u32 = 4;
/// Device supports reporting free pages.
pub const VIRTIO_BALLOON_F_REPORTING: u32 = 5;

// ---------------------------------------------------------------------------
// Statistics tags
// ---------------------------------------------------------------------------

/// Swap in (pages read from swap).
pub const VIRTIO_BALLOON_S_SWAP_IN: u16 = 0;
/// Swap out (pages written to swap).
pub const VIRTIO_BALLOON_S_SWAP_OUT: u16 = 1;
/// Major page faults.
pub const VIRTIO_BALLOON_S_MAJFLT: u16 = 2;
/// Minor page faults.
pub const VIRTIO_BALLOON_S_MINFLT: u16 = 3;
/// Total memory (bytes).
pub const VIRTIO_BALLOON_S_MEMFREE: u16 = 4;
/// Total memory available.
pub const VIRTIO_BALLOON_S_MEMTOT: u16 = 5;
/// Available memory (for allocation without swapping).
pub const VIRTIO_BALLOON_S_AVAIL: u16 = 6;
/// Disk cache size.
pub const VIRTIO_BALLOON_S_CACHES: u16 = 7;
/// Hugetlb allocations.
pub const VIRTIO_BALLOON_S_HTLB_PGALLOC: u16 = 8;
/// Hugetlb allocation failures.
pub const VIRTIO_BALLOON_S_HTLB_PGFAIL: u16 = 9;

/// Number of statistics tags.
pub const VIRTIO_BALLOON_S_NR: u16 = 10;

// ---------------------------------------------------------------------------
// Free page hint commands
// ---------------------------------------------------------------------------

/// Stop reporting free pages.
pub const VIRTIO_BALLOON_CMD_ID_STOP: u32 = 0;
/// Done reporting.
pub const VIRTIO_BALLOON_CMD_ID_DONE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_bits_distinct() {
        let features = [
            VIRTIO_BALLOON_F_MUST_TELL_HOST,
            VIRTIO_BALLOON_F_STATS_VQ,
            VIRTIO_BALLOON_F_DEFLATE_ON_OOM,
            VIRTIO_BALLOON_F_FREE_PAGE_HINT,
            VIRTIO_BALLOON_F_PAGE_POISON,
            VIRTIO_BALLOON_F_REPORTING,
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
            VIRTIO_BALLOON_S_SWAP_IN,
            VIRTIO_BALLOON_S_SWAP_OUT,
            VIRTIO_BALLOON_S_MAJFLT,
            VIRTIO_BALLOON_S_MINFLT,
            VIRTIO_BALLOON_S_MEMFREE,
            VIRTIO_BALLOON_S_MEMTOT,
            VIRTIO_BALLOON_S_AVAIL,
            VIRTIO_BALLOON_S_CACHES,
            VIRTIO_BALLOON_S_HTLB_PGALLOC,
            VIRTIO_BALLOON_S_HTLB_PGFAIL,
        ];
        for i in 0..tags.len() {
            for j in (i + 1)..tags.len() {
                assert_ne!(tags[i], tags[j]);
            }
        }
    }

    #[test]
    fn test_stat_tags_in_range() {
        let tags = [
            VIRTIO_BALLOON_S_SWAP_IN,
            VIRTIO_BALLOON_S_SWAP_OUT,
            VIRTIO_BALLOON_S_MAJFLT,
            VIRTIO_BALLOON_S_MINFLT,
            VIRTIO_BALLOON_S_MEMFREE,
            VIRTIO_BALLOON_S_MEMTOT,
            VIRTIO_BALLOON_S_AVAIL,
            VIRTIO_BALLOON_S_CACHES,
            VIRTIO_BALLOON_S_HTLB_PGALLOC,
            VIRTIO_BALLOON_S_HTLB_PGFAIL,
        ];
        for tag in &tags {
            assert!(*tag < VIRTIO_BALLOON_S_NR);
        }
    }

    #[test]
    fn test_cmd_ids_distinct() {
        assert_ne!(VIRTIO_BALLOON_CMD_ID_STOP, VIRTIO_BALLOON_CMD_ID_DONE);
    }
}
