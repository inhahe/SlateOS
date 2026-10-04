//! libvpx's realtime decisions, as a [`Decide`]: the non-RD path
//! (`encode_nonrd_sb_row`) that its speeds 5 and up take.
//!
//! On a key frame each superblock is partitioned by variance
//! (`choose_partitioning`), the partition walked as `nonrd_use_partition`
//! walks it, and each block's mode picked by `vp9_pick_intra_mode` (the
//! speed-8 key frame's `nonrd_keyframe`). Inter frames come later.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_encodeframe.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

use crate::common::{
    BLOCK_4X4, BLOCK_8X8, BLOCK_64X64, BlockSize, INTRA_MODES, PARTITION_NONE, Partition,
};
use crate::enc::encodeframe::{BlockModes, Decide, FrameEncoder};
use crate::enc::partition::{self, SbPartition};
use crate::enc::rd::kf_y_mode_costs;
use crate::tables;

/// libvpx's realtime decisions for a key frame.
pub(crate) struct RtDecisions {
    /// The rate-distortion multiplier: libvpx's `RDMULT`.
    rdmult: i32,
    /// The variance partitioning's thresholds: libvpx's `vbp_thresholds`.
    thresholds: [i64; 4],
    y_mode_costs: Box<[[[i32; INTRA_MODES]; INTRA_MODES]; INTRA_MODES]>,
    /// The current superblock's partition.
    sb: Option<SbPartition>,
}

impl RtDecisions {
    /// The decisions for a key frame whose multiplier is `rdmult` and whose
    /// luma AC step is `y_ac_dequant`: libvpx's `vp9_initialize_rd_consts`
    /// and `vp9_set_variance_partition_thresholds`.
    pub(crate) fn key_frame(rdmult: i32, y_ac_dequant: i32) -> Self {
        Self {
            rdmult,
            thresholds: partition::key_frame_thresholds(y_ac_dequant),
            y_mode_costs: kf_y_mode_costs(),
            sb: None,
        }
    }
}

impl Decide for RtDecisions {
    fn partition(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> Partition {
        if bsize == BLOCK_64X64 {
            let frame = partition::Frame {
                luma: &f.src.planes[0],
                mi_rows: f.mi.mi_rows,
                mi_cols: f.mi.mi_cols,
            };
            self.sb = Some(partition::choose_key_frame_partitioning(
                &frame,
                mi_row,
                mi_col,
                self.thresholds,
            ));
        }
        // nonrd_use_partition: the size chosen where the block starts says
        // how the block is cut.
        let subsize = if bsize >= BLOCK_8X8 {
            self.sb.map_or(BLOCK_4X4, |sb| sb.size_at(mi_row, mi_col))
        } else {
            BLOCK_4X4
        };
        let bsl = usize::from(
            tables::B_WIDTH_LOG2
                .get(usize::from(bsize))
                .copied()
                .unwrap_or(0),
        );
        tables::PARTITION_LOOKUP
            .get(bsl)
            .and_then(|p| p.get(usize::from(subsize)))
            .copied()
            .unwrap_or(PARTITION_NONE)
    }

    fn modes(
        &mut self,
        f: &mut FrameEncoder<'_>,
        mi_row: usize,
        mi_col: usize,
        bsize: BlockSize,
    ) -> BlockModes {
        f.pick_intra_mode(mi_row, mi_col, bsize, self.rdmult, &self.y_mode_costs)
    }
}
