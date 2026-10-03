//! VP9's vocabulary: block sizes, transform sizes, prediction modes, reference
//! frames, as the small integers libvpx numbers them.
//!
//! They stay integers, not Rust enums, because every one of them indexes a
//! table (`crate::tables`) laid out in libvpx's order, and a port that turned
//! each into an enum would turn each table lookup into a conversion that can
//! fail. What an enum would have guaranteed -- that a value is one of the
//! named ones -- is kept at the boundary instead: everything read from a
//! stream is decoded through a tree or bounded before it is used as an index.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/common/vp9_enums.h`,
//! `vp9_blockd.h`, `vp9_filter.h`, `vp9_entropy.h`, `vp9_entropymode.h`,
//! `vp9_entropymv.h` and `vp9_reconintra.c` (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

// --- Mode-info units -----------------------------------------------------------

/// log2 of the mode-info units per superblock side (64 = 8 << 3).
pub const MI_BLOCK_SIZE_LOG2: u32 = 3;
/// Mode-info units per superblock side.
pub const MI_BLOCK_SIZE: i32 = 8;
/// A position within a superblock, in mode-info units.
pub const MI_MASK: i32 = 7;

// --- Block sizes ---------------------------------------------------------------

/// A block size: `BLOCK_4X4` through `BLOCK_64X64`, or `BLOCK_INVALID`.
pub type BlockSize = u8;
pub const BLOCK_4X4: BlockSize = 0;
pub const BLOCK_4X8: BlockSize = 1;
pub const BLOCK_8X4: BlockSize = 2;
pub const BLOCK_8X8: BlockSize = 3;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_8X16: BlockSize = 4;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_16X8: BlockSize = 5;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_16X16: BlockSize = 6;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_16X32: BlockSize = 7;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_32X16: BlockSize = 8;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_32X32: BlockSize = 9;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_32X64: BlockSize = 10;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const BLOCK_64X32: BlockSize = 11;
pub const BLOCK_64X64: BlockSize = 12;
/// How many block sizes there are.
pub const BLOCK_SIZES: usize = 13;
/// No block size: what a subsampling or partition that does not exist gives.
pub const BLOCK_INVALID: BlockSize = 13;

// --- Partitions ----------------------------------------------------------------

/// How a square block divides.
pub type Partition = u8;
pub const PARTITION_NONE: Partition = 0;
pub const PARTITION_HORZ: Partition = 1;
pub const PARTITION_VERT: Partition = 2;
pub const PARTITION_SPLIT: Partition = 3;
/// How many partition types there are.
pub const PARTITION_TYPES: usize = 4;
/// Partition contexts: four for each of the four square sizes.
pub const PARTITION_CONTEXTS: usize = 16;
/// Partition contexts per block size.
pub const PARTITION_PLOFFSET: usize = 4;

// --- Transforms ----------------------------------------------------------------

/// A transform size: `TX_4X4` through `TX_32X32`.
pub type TxSize = u8;
pub const TX_4X4: TxSize = 0;
pub const TX_8X8: TxSize = 1;
pub const TX_16X16: TxSize = 2;
pub const TX_32X32: TxSize = 3;
/// How many transform sizes there are.
pub const TX_SIZES: usize = 4;

/// Which transform sizes a frame allows.
pub type TxMode = u8;
pub const ONLY_4X4: TxMode = 0;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const ALLOW_8X8: TxMode = 1;
#[allow(
    dead_code,
    reason = "named for libvpx's numbering; nothing refers to it by name"
)]
pub const ALLOW_16X16: TxMode = 2;
pub const ALLOW_32X32: TxMode = 3;
/// Each block says its own transform size.
pub const TX_MODE_SELECT: TxMode = 4;

/// Which one-dimensional transform runs in each direction.
pub type TxType = u8;
/// DCT both ways.
pub const DCT_DCT: TxType = 0;
/// ADST vertically, DCT horizontally.
pub const ADST_DCT: TxType = 1;
/// DCT vertically, ADST horizontally.
pub const DCT_ADST: TxType = 2;
/// ADST both ways.
pub const ADST_ADST: TxType = 3;

// --- Prediction modes ----------------------------------------------------------

/// A prediction mode: ten intra modes, then four inter.
pub type PredictionMode = u8;
pub const DC_PRED: PredictionMode = 0;
pub const V_PRED: PredictionMode = 1;
pub const H_PRED: PredictionMode = 2;
pub const D45_PRED: PredictionMode = 3;
pub const D135_PRED: PredictionMode = 4;
pub const D117_PRED: PredictionMode = 5;
pub const D153_PRED: PredictionMode = 6;
pub const D207_PRED: PredictionMode = 7;
pub const D63_PRED: PredictionMode = 8;
pub const TM_PRED: PredictionMode = 9;
pub const NEARESTMV: PredictionMode = 10;
pub const NEARMV: PredictionMode = 11;
pub const ZEROMV: PredictionMode = 12;
pub const NEWMV: PredictionMode = 13;
/// How many intra modes there are.
pub const INTRA_MODES: usize = 10;
/// How many inter modes there are.
pub const INTER_MODES: usize = 4;
/// How many modes there are in all.
pub const MB_MODE_COUNT: usize = 14;

/// The transform an intra mode's residual uses: libvpx's
/// `intra_mode_to_tx_type_lookup`.
pub const INTRA_MODE_TO_TX_TYPE: [TxType; INTRA_MODES] = [
    DCT_DCT,   // DC
    ADST_DCT,  // V
    DCT_ADST,  // H
    DCT_DCT,   // D45
    ADST_ADST, // D135
    ADST_DCT,  // D117
    DCT_ADST,  // D153
    DCT_ADST,  // D207
    ADST_DCT,  // D63
    ADST_ADST, // TM
];

// --- Reference frames ----------------------------------------------------------

/// A reference frame: intra, or one of the three a frame may predict from;
/// `NO_REF_FRAME` for a block's absent second reference.
pub type RefFrame = i8;
pub const NO_REF_FRAME: RefFrame = -1;
pub const INTRA_FRAME: RefFrame = 0;
pub const LAST_FRAME: RefFrame = 1;
pub const GOLDEN_FRAME: RefFrame = 2;
pub const ALTREF_FRAME: RefFrame = 3;
/// Reference frames, intra included.
pub const MAX_REF_FRAMES: usize = 4;
/// How many references one frame may name.
pub const REFS_PER_FRAME: usize = 3;
/// How many reference slots the decoder keeps.
pub const REF_FRAMES: usize = 8;

/// How a frame's blocks choose their references.
pub type ReferenceMode = u8;
pub const SINGLE_REFERENCE: ReferenceMode = 0;
pub const COMPOUND_REFERENCE: ReferenceMode = 1;
pub const REFERENCE_MODE_SELECT: ReferenceMode = 2;

// --- Interpolation filters ------------------------------------------------------

/// An interpolation filter, or `SWITCHABLE` for one chosen per block.
pub type InterpFilter = u8;
pub const EIGHTTAP: InterpFilter = 0;
pub const EIGHTTAP_SMOOTH: InterpFilter = 1;
pub const EIGHTTAP_SHARP: InterpFilter = 2;
pub const BILINEAR: InterpFilter = 3;
pub const SWITCHABLE: InterpFilter = 4;
/// How many filters a block may choose between.
pub const SWITCHABLE_FILTERS: usize = 3;
/// Interpolation filter contexts.
pub const SWITCHABLE_FILTER_CONTEXTS: usize = 4;

// --- Contexts ------------------------------------------------------------------

/// Skip flag contexts.
pub const SKIP_CONTEXTS: usize = 3;
/// Inter mode contexts.
pub const INTER_MODE_CONTEXTS: usize = 7;
/// Intra-or-inter contexts.
pub const INTRA_INTER_CONTEXTS: usize = 4;
/// Compound-or-single contexts.
pub const COMP_INTER_CONTEXTS: usize = 5;
/// Reference frame contexts.
pub const REF_CONTEXTS: usize = 5;
/// Transform size contexts.
pub const TX_SIZE_CONTEXTS: usize = 2;
/// Luma intra mode probability sets, by block size group.
pub const BLOCK_SIZE_GROUPS: usize = 4;

// --- Coefficient tokens ----------------------------------------------------------

/// Plane types: luma, chroma.
pub const PLANE_TYPES: usize = 2;
/// Reference types: intra, inter.
pub const REF_TYPES: usize = 2;
/// Coefficient bands.
pub const COEF_BANDS: usize = 6;
/// Coefficient contexts per band (band 0 uses only three).
pub const COEFF_CONTEXTS: usize = 6;
/// Probabilities kept per coefficient context; the rest come from the model.
pub const UNCONSTRAINED_NODES: usize = 3;
/// The node whose probability selects the model's tail.
pub const PIVOT_NODE: usize = 2;
/// Contexts a band uses: three for band 0, six for the rest.
#[must_use]
pub const fn band_coeff_contexts(band: usize) -> usize {
    if band == 0 { 3 } else { COEFF_CONTEXTS }
}

// --- Segmentation ----------------------------------------------------------------

/// How many segments a frame may divide into.
pub const MAX_SEGMENTS: usize = 8;
/// Segment features: quantiser, loop filter, reference frame, skip.
pub const SEG_LVL_ALT_Q: usize = 0;
pub const SEG_LVL_ALT_LF: usize = 1;
pub const SEG_LVL_REF_FRAME: usize = 2;
pub const SEG_LVL_SKIP: usize = 3;
/// How many segment features there are.
pub const SEG_LVL_MAX: usize = 4;

// --- Motion vectors -----------------------------------------------------------

/// MV joint types: which components are nonzero.
pub const MV_JOINTS: usize = 4;
/// MV magnitude classes.
pub const MV_CLASSES: usize = 11;
/// Integer bits of class 0.
pub const CLASS0_BITS: usize = 1;
/// Class 0's integer values.
pub const CLASS0_SIZE: usize = 2;
/// Offset bits of the largest class.
pub const MV_OFFSET_BITS: usize = MV_CLASSES + CLASS0_BITS - 2;
/// Fractional positions.
pub const MV_FP_SIZE: usize = 4;
/// The largest MV component a stream may produce, exclusive, in 1/8 pixels:
/// libvpx's `MV_UPP`.
pub const MV_UPP: i32 = (1 << 14) - 1;
/// The smallest, exclusive: libvpx's `MV_LOW`.
pub const MV_LOW: i32 = -(1 << 14);

/// A motion vector in eighth pixels, row then column, each 16 bits as
/// libvpx keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mv {
    pub row: i16,
    pub col: i16,
}

impl Mv {
    /// The zero vector.
    pub const ZERO: Self = Self { row: 0, col: 0 };

    /// libvpx's `0x80008000`: a value no decoded vector has, used where a
    /// slot must hold something.
    pub const INVALID: Self = Self {
        row: i16::MIN,
        col: i16::MIN,
    };

    /// Both components negated, wrapping as libvpx's `*= -1` on its 16-bit
    /// fields does.
    #[must_use]
    pub const fn negated(self) -> Self {
        Self {
            row: self.row.wrapping_neg(),
            col: self.col.wrapping_neg(),
        }
    }
}

// --- Trees ----------------------------------------------------------------------

/// The intra mode tree: libvpx's `vp9_intra_mode_tree`.
pub const INTRA_MODE_TREE: [i8; 18] = [
    -(DC_PRED as i8),
    2,
    -(TM_PRED as i8),
    4,
    -(V_PRED as i8),
    6,
    8,
    12,
    -(H_PRED as i8),
    10,
    -(D135_PRED as i8),
    -(D117_PRED as i8),
    -(D45_PRED as i8),
    14,
    -(D63_PRED as i8),
    16,
    -(D153_PRED as i8),
    -(D207_PRED as i8),
];

/// The inter mode tree, symbols offset from `NEARESTMV`: libvpx's
/// `vp9_inter_mode_tree`.
pub const INTER_MODE_TREE: [i8; 6] = [-2, 2, 0, 4, -1, -3];

/// The partition tree: libvpx's `vp9_partition_tree`.
pub const PARTITION_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];

/// The interpolation filter tree: libvpx's `vp9_switchable_interp_tree`.
pub const SWITCHABLE_INTERP_TREE: [i8; 4] = [0, 2, -1, -2];

/// The segment id tree: libvpx's `vp9_segment_tree`.
pub const SEGMENT_TREE: [i8; 14] = [2, 4, 6, 8, 10, 12, 0, -1, -2, -3, -4, -5, -6, -7];

/// The MV joint tree: libvpx's `vp9_mv_joint_tree`.
pub const MV_JOINT_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];

/// The MV class tree: libvpx's `vp9_mv_class_tree`.
pub const MV_CLASS_TREE: [i8; 20] = [
    0, 2, -1, 4, 6, 8, -2, -3, 10, 12, -4, -5, -6, 14, 16, 18, -7, -8, -9, -10,
];

/// The class 0 tree: libvpx's `vp9_mv_class0_tree`.
pub const MV_CLASS0_TREE: [i8; 2] = [0, -1];

/// The fractional MV tree: libvpx's `vp9_mv_fp_tree`.
pub const MV_FP_TREE: [i8; 6] = [0, 2, -1, 4, -2, -3];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inter_mode_tree_names_modes_from_nearestmv() {
        // libvpx: { -INTER_OFFSET(ZEROMV), 2, -INTER_OFFSET(NEARESTMV), 4,
        //           -INTER_OFFSET(NEARMV), -INTER_OFFSET(NEWMV) }
        let offset = |m: PredictionMode| -((m - NEARESTMV) as i8);
        assert_eq!(
            INTER_MODE_TREE,
            [
                offset(ZEROMV),
                2,
                offset(NEARESTMV),
                4,
                offset(NEARMV),
                offset(NEWMV)
            ]
        );
    }

    #[test]
    fn every_tree_reaches_each_of_its_symbols_once() {
        fn leaves(tree: &[i8]) -> Vec<i8> {
            let mut out: Vec<i8> = tree.iter().copied().filter(|&n| n <= 0).collect();
            out.sort_unstable();
            out
        }
        let expect = |n: i8| (0..n).map(|v| -v).rev().collect::<Vec<i8>>();
        assert_eq!(leaves(&INTRA_MODE_TREE), expect(10));
        assert_eq!(leaves(&INTER_MODE_TREE), expect(4));
        assert_eq!(leaves(&PARTITION_TREE), expect(4));
        assert_eq!(leaves(&SWITCHABLE_INTERP_TREE), expect(3));
        assert_eq!(leaves(&SEGMENT_TREE), expect(8));
        assert_eq!(leaves(&MV_JOINT_TREE), expect(4));
        assert_eq!(leaves(&MV_CLASS_TREE), expect(11));
        assert_eq!(leaves(&MV_CLASS0_TREE), expect(2));
        assert_eq!(leaves(&MV_FP_TREE), expect(4));
    }
}
