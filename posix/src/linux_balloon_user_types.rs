//! `<linux/virtio_balloon.h>` — virtio memory-balloon device constants.
//!
//! The virtio balloon lets a hypervisor reclaim guest memory by
//! "inflating" a balloon driver that pins pages from the guest.
//! Constants here mirror the virtio 1.x spec for device ID 5.

// ---------------------------------------------------------------------------
// Device identity
// ---------------------------------------------------------------------------

/// Virtio device-ID for memory balloon (per virtio 1.x).
pub const VIRTIO_ID_BALLOON: u32 = 5;

// ---------------------------------------------------------------------------
// Virtqueue indices
// ---------------------------------------------------------------------------

pub const VIRTIO_BALLOON_VQ_INFLATE: u32 = 0;
pub const VIRTIO_BALLOON_VQ_DEFLATE: u32 = 1;
pub const VIRTIO_BALLOON_VQ_STATS: u32 = 2;
pub const VIRTIO_BALLOON_VQ_FREE_PAGE: u32 = 3;
pub const VIRTIO_BALLOON_VQ_REPORTING: u32 = 4;

// ---------------------------------------------------------------------------
// Feature-negotiation bits (`u64` device features)
// ---------------------------------------------------------------------------

pub const VIRTIO_BALLOON_F_MUST_TELL_HOST: u32 = 0;
pub const VIRTIO_BALLOON_F_STATS_VQ: u32 = 1;
pub const VIRTIO_BALLOON_F_DEFLATE_ON_OOM: u32 = 2;
pub const VIRTIO_BALLOON_F_FREE_PAGE_HINT: u32 = 3;
pub const VIRTIO_BALLOON_F_PAGE_POISON: u32 = 4;
pub const VIRTIO_BALLOON_F_REPORTING: u32 = 5;

// ---------------------------------------------------------------------------
// Page-frame number shift (balloon reports in 4 KiB units regardless of
// guest page size)
// ---------------------------------------------------------------------------

pub const VIRTIO_BALLOON_PFN_SHIFT: u32 = 12;

// ---------------------------------------------------------------------------
// Stats vq tag identifiers (`virtio_balloon_stat.tag`)
// ---------------------------------------------------------------------------

pub const VIRTIO_BALLOON_S_SWAP_IN: u32 = 0;
pub const VIRTIO_BALLOON_S_SWAP_OUT: u32 = 1;
pub const VIRTIO_BALLOON_S_MAJFLT: u32 = 2;
pub const VIRTIO_BALLOON_S_MINFLT: u32 = 3;
pub const VIRTIO_BALLOON_S_MEMFREE: u32 = 4;
pub const VIRTIO_BALLOON_S_MEMTOT: u32 = 5;
pub const VIRTIO_BALLOON_S_AVAIL: u32 = 6;
pub const VIRTIO_BALLOON_S_CACHES: u32 = 7;
pub const VIRTIO_BALLOON_S_HTLB_PGALLOC: u32 = 8;
pub const VIRTIO_BALLOON_S_HTLB_PGFAIL: u32 = 9;
pub const VIRTIO_BALLOON_S_OOM_KILL: u32 = 10;
pub const VIRTIO_BALLOON_S_ALLOC_STALL: u32 = 11;
pub const VIRTIO_BALLOON_S_ASYNC_SCAN: u32 = 12;
pub const VIRTIO_BALLOON_S_DIRECT_SCAN: u32 = 13;
pub const VIRTIO_BALLOON_S_ASYNC_RECLAIM: u32 = 14;
pub const VIRTIO_BALLOON_S_DIRECT_RECLAIM: u32 = 15;
pub const VIRTIO_BALLOON_S_NR: u32 = 16;

// ---------------------------------------------------------------------------
// Free-page-hint command status (driver writes, device reads)
// ---------------------------------------------------------------------------

pub const VIRTIO_BALLOON_CMD_ID_STOP: u32 = 0;
pub const VIRTIO_BALLOON_CMD_ID_DONE: u32 = 1;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_is_five() {
        // Virtio spec §5.5: balloon is device 5.
        assert_eq!(VIRTIO_ID_BALLOON, 5);
    }

    #[test]
    fn test_vq_indices_dense_0_to_4() {
        let v = [
            VIRTIO_BALLOON_VQ_INFLATE,
            VIRTIO_BALLOON_VQ_DEFLATE,
            VIRTIO_BALLOON_VQ_STATS,
            VIRTIO_BALLOON_VQ_FREE_PAGE,
            VIRTIO_BALLOON_VQ_REPORTING,
        ];
        for (i, &v) in v.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // Inflate=0 / Deflate=1 form a complementary pair.
        assert_eq!(VIRTIO_BALLOON_VQ_DEFLATE - VIRTIO_BALLOON_VQ_INFLATE, 1);
    }

    #[test]
    fn test_features_dense_0_to_5() {
        let f = [
            VIRTIO_BALLOON_F_MUST_TELL_HOST,
            VIRTIO_BALLOON_F_STATS_VQ,
            VIRTIO_BALLOON_F_DEFLATE_ON_OOM,
            VIRTIO_BALLOON_F_FREE_PAGE_HINT,
            VIRTIO_BALLOON_F_PAGE_POISON,
            VIRTIO_BALLOON_F_REPORTING,
        ];
        for (i, &v) in f.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn test_pfn_shift_is_twelve() {
        // Balloon page size is fixed at 4 KiB regardless of guest page
        // size, so the shift is always 12.
        assert_eq!(VIRTIO_BALLOON_PFN_SHIFT, 12);
        assert_eq!(1u32 << VIRTIO_BALLOON_PFN_SHIFT, 4096);
    }

    #[test]
    fn test_stat_tags_dense_0_to_15_with_count_at_16() {
        let s = [
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
            VIRTIO_BALLOON_S_OOM_KILL,
            VIRTIO_BALLOON_S_ALLOC_STALL,
            VIRTIO_BALLOON_S_ASYNC_SCAN,
            VIRTIO_BALLOON_S_DIRECT_SCAN,
            VIRTIO_BALLOON_S_ASYNC_RECLAIM,
            VIRTIO_BALLOON_S_DIRECT_RECLAIM,
        ];
        for (i, &v) in s.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
        // _S_NR is the cardinality marker (one past the last tag).
        assert_eq!(VIRTIO_BALLOON_S_NR, s.len() as u32);
    }

    #[test]
    fn test_cmd_status_pair() {
        assert_eq!(VIRTIO_BALLOON_CMD_ID_STOP, 0);
        assert_eq!(VIRTIO_BALLOON_CMD_ID_DONE, 1);
        assert_eq!(
            VIRTIO_BALLOON_CMD_ID_DONE - VIRTIO_BALLOON_CMD_ID_STOP,
            1
        );
    }
}
