//! Coefficient tokens: a transform block's quantised coefficients turned into
//! the tokens the bitstream codes them with, and counted.
//!
//! The decoder's `detokenize` read in reverse. Each token is coded against
//! the probabilities of its band and context; a token keeps *which*
//! probabilities those are, not their values, because the frame's compressed
//! header may still change them before the tokens are written -- libvpx keeps
//! a pointer into the frame context for the same reason.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/encoder/vp9_tokenize.c` and
//! `vp9_tokenize.h` (copyright the WebM project authors), used under libvpx's
//! BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

use crate::common::{
    COEF_BANDS, COEFF_CONTEXTS, PLANE_TYPES, REF_TYPES, TX_4X4, TX_32X32, TX_SIZES, TxSize,
    UNCONSTRAINED_NODES,
};
use crate::detokenize::Scan;
use crate::probs::{CoefCounts, EobBranchCounts};
use crate::tables;

/// The tokens, in libvpx's numbering: zero to four, six categories of larger
/// values with extra bits, and the end of a block.
pub(crate) const ZERO_TOKEN: u8 = 0;
pub(crate) const ONE_TOKEN: u8 = 1;
pub(crate) const TWO_TOKEN: u8 = 2;
pub(crate) const CATEGORY1_TOKEN: u8 = 5;
pub(crate) const CATEGORY6_TOKEN: u8 = 10;
pub(crate) const EOB_TOKEN: u8 = 11;
/// The end of a block's tokens: never coded, it tells the writer where one
/// block's tokens stop and the next block's start. libvpx's `EOSB_TOKEN`.
pub(crate) const EOSB_TOKEN: u8 = 127;
/// How many tokens are coded: libvpx's `ENTROPY_TOKENS`.
pub(crate) const ENTROPY_TOKENS: usize = 12;

/// Each token's energy class, the measure the contexts of the tokens after
/// it are made from: libvpx's `vp9_pt_energy_class`.
const ENERGY_CLASS: [u8; ENTROPY_TOKENS] = [0, 1, 2, 3, 3, 4, 4, 5, 5, 5, 5, 5];

/// The smallest value of each category, `CATEGORY1_TOKEN` on: libvpx's
/// `CAT1_MIN_VAL` to `CAT6_MIN_VAL`.
pub(crate) const CAT_MIN_VAL: [i32; 6] = [5, 7, 11, 19, 35, 67];

/// The extra-bit probabilities of categories 1 to 5: libvpx's
/// `vp9_cat1_prob` to `vp9_cat5_prob`. Category 6's depend on the bit depth
/// ([`cat6_probs`]).
pub(crate) const CAT_PROBS: [&[u8]; 5] = [
    &[159],
    &[165, 145],
    &[173, 148, 140],
    &[176, 155, 140, 135],
    &[180, 157, 141, 134, 130],
];

/// Category 6's extra-bit probabilities at `bit_depth`: 14 bits at 8, 16 at
/// 10 and 18 at 12, the tail of libvpx's `vp9_cat6_prob_high12`.
pub(crate) fn cat6_probs(bit_depth: u8) -> &'static [u8] {
    let skip = match bit_depth {
        12 => 0,
        10 => 2,
        _ => 4,
    };
    tables::CAT6_PROB_HIGH12.get(skip..).unwrap_or(&[])
}

/// Coefficient token counts of one transform size, by plane type, reference
/// type, band and context, every token counted apart: libvpx's
/// `vp9_coeff_count`. [`model_counts`] folds them into the decoder's model
/// counts for adaptation.
pub(crate) type CoefTokenCounts =
    [[[[[u32; ENTROPY_TOKENS]; COEFF_CONTEXTS]; COEF_BANDS]; REF_TYPES]; PLANE_TYPES];

/// Which probabilities a token is coded with: `coef_probs[tx][plane type]
/// [reference type][band][context]`, packed into 16 bits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProbIndex(u16);

impl ProbIndex {
    #[allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "each field is masked to its width (2, 1, 1, 3 and 3 bits) before it is shifted into place"
    )]
    fn new(tx: usize, plane_type: usize, ref_type: usize, band: usize, ctx: usize) -> Self {
        Self(
            ((tx as u16 & 3) << 8)
                | ((plane_type as u16 & 1) << 7)
                | ((ref_type as u16 & 1) << 6)
                | ((band as u16 & 7) << 3)
                | (ctx as u16 & 7),
        )
    }

    /// The transform size, plane type, reference type, band and context.
    pub(crate) fn parts(self) -> [usize; 5] {
        let v = usize::from(self.0);
        [
            (v >> 8) & 3,
            (v >> 7) & 1,
            (v >> 6) & 1,
            (v >> 3) & 7,
            v & 7,
        ]
    }
}

/// One coded token: libvpx's `TOKENEXTRA`. `extra` is the category's extra
/// bits shifted up one, with the sign in the lowest bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TokenExtra {
    pub token: u8,
    pub extra: u32,
    pub probs: ProbIndex,
}

impl TokenExtra {
    /// The marker after a block's tokens.
    pub(crate) const END_OF_BLOCK_TOKENS: Self = Self {
        token: EOSB_TOKEN,
        extra: 0,
        probs: ProbIndex(0),
    };
}

/// The token coding `v` and its extra bits: libvpx's `vp9_get_token_extra`,
/// computed rather than looked up in `dct_cat_lt_10_value_tokens`, which is
/// this function tabulated.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::indexing_slicing,
    reason = "a quantised coefficient is far inside i32: its magnitude less a category's base, doubled, fits u32; cat starts at the table's last index and only falls"
)]
pub(crate) fn token_extra(v: i32) -> (u8, u32) {
    let sign = u32::from(v < 0);
    let mag = v.unsigned_abs();
    if mag <= 4 {
        // ZERO_TOKEN to FOUR_TOKEN: the value is the token. Zero has no sign.
        return (mag as u8, if mag == 0 { 0 } else { sign });
    }
    let mut cat = CAT_MIN_VAL.len() - 1;
    while cat > 0 && (mag as i32) < CAT_MIN_VAL[cat] {
        cat -= 1;
    }
    let base = CAT_MIN_VAL[cat] as u32;
    (CATEGORY1_TOKEN + cat as u8, ((mag - base) << 1) | sign)
}

/// A coefficient's context: the rounded mean of the energy classes of its
/// two neighbours already coded. libvpx's `get_coef_context`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "energy classes are at most 5 and positions below 1024"
)]
fn coef_context(neighbors: &[i16], token_cache: &[u8; 1024], c: usize) -> usize {
    let cache = |i: usize| {
        neighbors
            .get(i)
            .and_then(|&pos| token_cache.get(usize::try_from(pos).ok()?))
            .copied()
            .unwrap_or(0)
    };
    (1 + usize::from(cache(2 * c)) + usize::from(cache(2 * c + 1))) >> 1
}

/// Where one transform block's tokens and counts go.
pub(crate) struct TokenSink<'a> {
    pub tokens: &'a mut Vec<TokenExtra>,
    /// This transform size's token counts.
    pub counts: &'a mut CoefTokenCounts,
    /// This transform size's end-of-block decisions, which the decoder's
    /// adaptation reads directly.
    pub eob_branch: &'a mut EobBranchCounts,
    pub token_cache: &'a mut [u8; 1024],
}

/// Tokenize one transform block: libvpx's `tokenize_b`, less the contexts
/// it leaves behind, which the caller sets (the block has coefficients if
/// `eob` is above zero).
///
/// `qcoeff` holds the block's quantised coefficients in raster order; `eob`
/// is one past the last nonzero one in `scan` order, as the quantisers
/// return it; `ctx` is the block's context from its neighbours.
#[allow(
    clippy::too_many_arguments,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing,
    reason = "positions are below the transform's size (at most 1024) and counts wrap as libvpx's unsigned ones do; the plane and reference types are clamped to their arrays' lengths first"
)]
pub(crate) fn tokenize_b(
    qcoeff: &[i32],
    eob: usize,
    tx_size: TxSize,
    plane_type: usize,
    ref_type: usize,
    ctx: usize,
    scan: &Scan,
    sink: &mut TokenSink<'_>,
) {
    let tx = usize::from(tx_size.min(TX_32X32));
    let tx_eob = 16usize << (tx << 1);
    let band_translate: &[u8] = if tx_size == TX_4X4 {
        &tables::COEFBAND_TRANS_4X4
    } else {
        &tables::COEFBAND_TRANS_8X8PLUS
    };
    let band = |c: usize| usize::from(band_translate.get(c).copied().unwrap_or(5)).min(5);
    let plane_type = plane_type.min(PLANE_TYPES - 1);
    let ref_type = ref_type.min(REF_TYPES - 1);
    let counts = &mut sink.counts[plane_type][ref_type];
    let eob_branch = &mut sink.eob_branch[plane_type][ref_type];
    let coefficient = |c: usize| {
        scan.scan
            .get(c)
            .and_then(|&pos| qcoeff.get(usize::try_from(pos).ok()?))
            .copied()
            .unwrap_or(0)
    };
    let mut add = |tokens: &mut Vec<TokenExtra>, b: usize, pt: usize, token: u8, extra: u32| {
        tokens.push(TokenExtra {
            token,
            extra,
            probs: ProbIndex::new(tx, plane_type, ref_type, b, pt),
        });
        if let Some(n) = counts
            .get_mut(b)
            .and_then(|c| c.get_mut(pt))
            .and_then(|c| c.get_mut(usize::from(token)))
        {
            *n = n.wrapping_add(1);
        }
    };
    let cache_set = |cache: &mut [u8; 1024], c: usize, energy: u8| {
        if let Some(t) = scan
            .scan
            .get(c)
            .and_then(|&pos| cache.get_mut(usize::try_from(pos).ok()?))
        {
            *t = energy;
        }
    };

    let eob = eob.min(tx_eob);
    let mut pt = ctx.min(COEFF_CONTEXTS - 1);
    let mut c = 0usize;
    while c < eob {
        let mut v = coefficient(c);
        if let Some(n) = eob_branch.get_mut(band(c)).and_then(|e| e.get_mut(pt)) {
            *n = n.wrapping_add(1);
        }
        while v == 0 {
            add(sink.tokens, band(c), pt, ZERO_TOKEN, 0);
            cache_set(sink.token_cache, c, 0);
            c += 1;
            if c >= eob {
                // The quantisers' end of block is one past a nonzero
                // coefficient, so a run of zeros always ends inside it.
                debug_assert!(false, "a block's end of block follows a zero");
                return;
            }
            pt = coef_context(scan.neighbors, sink.token_cache, c);
            v = coefficient(c);
        }
        let (token, extra) = token_extra(v);
        add(sink.tokens, band(c), pt, token, extra);
        cache_set(
            sink.token_cache,
            c,
            ENERGY_CLASS.get(usize::from(token)).copied().unwrap_or(5),
        );
        c += 1;
        pt = coef_context(scan.neighbors, sink.token_cache, c);
    }
    if c < tx_eob {
        if let Some(n) = eob_branch.get_mut(band(c)).and_then(|e| e.get_mut(pt)) {
            *n = n.wrapping_add(1);
        }
        add(sink.tokens, band(c), pt, EOB_TOKEN, 0);
    }
}

/// Fold token counts into the decoder's model counts -- zero, one, two or
/// more, end of block -- for adaptation: libvpx's `full_to_model_counts`.
#[allow(
    clippy::indexing_slicing,
    reason = "every index is a loop position over arrays of the same fixed dimensions"
)]
pub(crate) fn model_counts(full: &[CoefTokenCounts; TX_SIZES], model: &mut [CoefCounts; TX_SIZES]) {
    for (f_tx, m_tx) in full.iter().zip(model.iter_mut()) {
        for (f_pl, m_pl) in f_tx.iter().zip(m_tx.iter_mut()) {
            for (f_ref, m_ref) in f_pl.iter().zip(m_pl.iter_mut()) {
                for (f_band, m_band) in f_ref.iter().zip(m_ref.iter_mut()) {
                    for (f, m) in f_band.iter().zip(m_band.iter_mut()) {
                        let two_or_more = f[usize::from(TWO_TOKEN)..usize::from(EOB_TOKEN)]
                            .iter()
                            .fold(0u32, |a, &n| a.wrapping_add(n));
                        *m = [
                            f[usize::from(ZERO_TOKEN)],
                            f[usize::from(ONE_TOKEN)],
                            two_or_more,
                            f[usize::from(EOB_TOKEN)],
                        ];
                        debug_assert_eq!(m.len(), UNCONSTRAINED_NODES + 1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::common::{DCT_DCT, TX_8X8};
    use crate::detokenize::scan_for;

    /// libvpx's `dct_cat_lt_10_value_tokens`, from -66 to 66, the table
    /// [`token_extra`] computes.
    const LIBVPX_TOKENS: [(u8, u32); 133] = [
        (9, 63),
        (9, 61),
        (9, 59),
        (9, 57),
        (9, 55),
        (9, 53),
        (9, 51),
        (9, 49),
        (9, 47),
        (9, 45),
        (9, 43),
        (9, 41),
        (9, 39),
        (9, 37),
        (9, 35),
        (9, 33),
        (9, 31),
        (9, 29),
        (9, 27),
        (9, 25),
        (9, 23),
        (9, 21),
        (9, 19),
        (9, 17),
        (9, 15),
        (9, 13),
        (9, 11),
        (9, 9),
        (9, 7),
        (9, 5),
        (9, 3),
        (9, 1),
        (8, 31),
        (8, 29),
        (8, 27),
        (8, 25),
        (8, 23),
        (8, 21),
        (8, 19),
        (8, 17),
        (8, 15),
        (8, 13),
        (8, 11),
        (8, 9),
        (8, 7),
        (8, 5),
        (8, 3),
        (8, 1),
        (7, 15),
        (7, 13),
        (7, 11),
        (7, 9),
        (7, 7),
        (7, 5),
        (7, 3),
        (7, 1),
        (6, 7),
        (6, 5),
        (6, 3),
        (6, 1),
        (5, 3),
        (5, 1),
        (4, 1),
        (3, 1),
        (2, 1),
        (1, 1),
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 0),
        (5, 2),
        (6, 0),
        (6, 2),
        (6, 4),
        (6, 6),
        (7, 0),
        (7, 2),
        (7, 4),
        (7, 6),
        (7, 8),
        (7, 10),
        (7, 12),
        (7, 14),
        (8, 0),
        (8, 2),
        (8, 4),
        (8, 6),
        (8, 8),
        (8, 10),
        (8, 12),
        (8, 14),
        (8, 16),
        (8, 18),
        (8, 20),
        (8, 22),
        (8, 24),
        (8, 26),
        (8, 28),
        (8, 30),
        (9, 0),
        (9, 2),
        (9, 4),
        (9, 6),
        (9, 8),
        (9, 10),
        (9, 12),
        (9, 14),
        (9, 16),
        (9, 18),
        (9, 20),
        (9, 22),
        (9, 24),
        (9, 26),
        (9, 28),
        (9, 30),
        (9, 32),
        (9, 34),
        (9, 36),
        (9, 38),
        (9, 40),
        (9, 42),
        (9, 44),
        (9, 46),
        (9, 48),
        (9, 50),
        (9, 52),
        (9, 54),
        (9, 56),
        (9, 58),
        (9, 60),
        (9, 62),
    ];

    #[test]
    fn small_values_tokenize_as_libvpx_tabulates_them() {
        for (i, &want) in LIBVPX_TOKENS.iter().enumerate() {
            let v = i as i32 - 66;
            assert_eq!(token_extra(v), want, "value {v}");
        }
    }

    #[test]
    fn category_six_carries_the_rest_in_its_extra_bits() {
        // libvpx: 2v - 2 * CAT6_MIN_VAL, or -2v - 2 * CAT6_MIN_VAL + 1.
        for v in [67, 68, 100, 1000, 16_449] {
            assert_eq!(token_extra(v), (CATEGORY6_TOKEN, (2 * v - 134) as u32));
            assert_eq!(token_extra(-v), (CATEGORY6_TOKEN, (2 * v - 133) as u32));
        }
    }

    /// Tokenize a block with the encoder, decode the tokens' values back:
    /// the coefficients come back, the counts add up.
    #[test]
    fn a_block_tokenizes_to_its_values_and_counts() {
        let scan = scan_for(TX_8X8, DCT_DCT);
        let mut q = [0i32; 64];
        // Coefficients in scan order: a run of zeros, every category, a sign.
        let values = [
            (0, 70),
            (1, -1),
            (3, 2),
            (4, 5),
            (5, -7),
            (9, 11),
            (12, 19),
            (20, -35),
            (30, 3),
        ];
        for &(c, v) in &values {
            q[scan.scan[c] as usize] = v;
        }
        let eob = 31;
        let mut tokens = Vec::new();
        let mut counts = Box::new([[[[[0u32; ENTROPY_TOKENS]; 6]; 6]; 2]; 2]);
        let mut eob_branch = [[[[0u32; 6]; 6]; 2]; 2];
        let mut token_cache = [0u8; 1024];
        let mut sink = TokenSink {
            tokens: &mut tokens,
            counts: &mut counts,
            eob_branch: &mut eob_branch,
            token_cache: &mut token_cache,
        };
        tokenize_b(&q, eob, TX_8X8, 0, 0, 0, &scan, &mut sink);

        // Rebuild the values from the tokens.
        let mut c = 0;
        let mut rebuilt = [0i32; 64];
        for t in &tokens {
            if t.token == EOB_TOKEN {
                break;
            }
            let mag = match t.token {
                0..=4 => i32::from(t.token),
                cat => CAT_MIN_VAL[usize::from(cat - CATEGORY1_TOKEN)] + (t.extra >> 1) as i32,
            };
            let v = if t.extra & 1 == 1 { -mag } else { mag };
            rebuilt[scan.scan[c] as usize] = v;
            c += 1;
        }
        assert_eq!(rebuilt, q);
        assert_eq!(c, eob);
        assert_eq!(tokens.last().unwrap().token, EOB_TOKEN);
        // One token per coefficient up to the end of block, then the end.
        assert_eq!(tokens.len(), eob + 1);
        let counted: u32 = counts.iter().flatten().flatten().flatten().flatten().sum();
        assert_eq!(counted as usize, tokens.len());
        // An end-of-block decision before each run of zeros and nonzero
        // value -- nine of them -- and the end itself.
        let decisions: u32 = eob_branch.iter().flatten().flatten().flatten().sum();
        assert_eq!(decisions, values.len() as u32 + 1);
        // The first token is coded in context 0 of band 0; each token's
        // band follows its position.
        assert_eq!(tokens[0].probs.parts(), [1, 0, 0, 0, 0]);
        for (i, t) in tokens.iter().enumerate() {
            let band = tables::COEFBAND_TRANS_8X8PLUS[i];
            assert_eq!(t.probs.parts()[3], usize::from(band), "token {i}");
        }
    }

    #[test]
    fn a_full_block_has_no_end_token() {
        let scan = scan_for(TX_4X4, DCT_DCT);
        let q = [1i32; 16];
        let mut tokens = Vec::new();
        let mut counts = Box::new([[[[[0u32; ENTROPY_TOKENS]; 6]; 6]; 2]; 2]);
        let mut eob_branch = [[[[0u32; 6]; 6]; 2]; 2];
        let mut token_cache = [0u8; 1024];
        let mut sink = TokenSink {
            tokens: &mut tokens,
            counts: &mut counts,
            eob_branch: &mut eob_branch,
            token_cache: &mut token_cache,
        };
        tokenize_b(&q, 16, TX_4X4, 1, 1, 2, &scan, &mut sink);
        assert_eq!(tokens.len(), 16);
        assert!(tokens.iter().all(|t| t.token == ONE_TOKEN));
        assert_eq!(tokens[0].probs.parts(), [0, 1, 1, 0, 2]);
    }

    #[test]
    fn model_counts_fold_two_and_up_together() {
        let mut full: Box<[CoefTokenCounts; TX_SIZES]> =
            vec![[[[[[0u32; ENTROPY_TOKENS]; 6]; 6]; 2]; 2]; TX_SIZES]
                .into_boxed_slice()
                .try_into()
                .unwrap();
        full[2][1][0][3][4] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];
        let mut model = Box::new([[[[[[0u32; 4]; 6]; 6]; 2]; 2]; 4]);
        model_counts(&full, &mut model);
        assert_eq!(
            model[2][1][0][3][4],
            [1, 2, 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 + 11, 12]
        );
        assert_eq!(model[0][0][0][0][0], [0; 4]);
    }

    #[test]
    fn category_six_probabilities_by_bit_depth() {
        assert_eq!(cat6_probs(8).len(), 14);
        assert_eq!(cat6_probs(10).len(), 16);
        assert_eq!(cat6_probs(12).len(), 18);
        // libvpx's vp9_cat6_prob, the 8-bit table.
        assert_eq!(
            cat6_probs(8),
            &[
                254, 254, 254, 252, 249, 243, 230, 196, 177, 153, 140, 133, 130, 129
            ]
        );
    }
}
