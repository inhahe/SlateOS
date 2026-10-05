//! SILK's codebooks built from its tables: the two NLSF codebooks (narrow-
//! and mediumband, and wideband) and the tables libopus gathers by pointer.
//!
//! Translated into Rust from libopus 1.5.2's `silk/tables_NLSF_CB_NB_MB.c`,
//! `silk/tables_NLSF_CB_WB.c`, `silk/tables_LTP.c` and `silk/tables_other.c`,
//! copyright Skype Limited, Xiph.Org and the contributors named in its
//! `COPYING`, used under libopus's BSD licence (`licenses/libopus-COPYING`).

use super::fix::fix_const;
use super::tables::{
    LBRR_FLAGS_2_ICDF, LBRR_FLAGS_3_ICDF, LTP_GAIN_ICDF_0, LTP_GAIN_ICDF_1, LTP_GAIN_ICDF_2,
    LTP_GAIN_VQ_0, LTP_GAIN_VQ_1, LTP_GAIN_VQ_2, NLSF_CB1_ICDF_NB_MB, NLSF_CB1_ICDF_WB,
    NLSF_CB1_NB_MB_Q8, NLSF_CB1_WB_Q8, NLSF_CB1_WB_WGHT_Q9, NLSF_CB1_WGHT_Q9, NLSF_CB2_ICDF_NB_MB,
    NLSF_CB2_ICDF_WB, NLSF_CB2_SELECT_NB_MB, NLSF_CB2_SELECT_WB, NLSF_DELTA_MIN_NB_MB_Q15,
    NLSF_DELTA_MIN_WB_Q15, NLSF_PRED_NB_MB_Q8, NLSF_PRED_WB_Q8,
};

/// An NLSF codebook: `silk_NLSF_CB_struct`, the decoder's fields.
#[derive(Debug)]
pub(crate) struct NlsfCodebook {
    /// First-stage vectors.
    pub n_vectors: usize,
    /// The LPC order: 10 or 16.
    pub order: usize,
    pub quant_step_size_q16: i32,
    /// The first stage's vectors (`n_vectors` x `order`), Q8.
    pub cb1_nlsf_q8: &'static [u8],
    /// Their weights, Q9.
    pub cb1_wght_q9: &'static [i16],
    /// The first stage's index, by signal type (unvoiced, then voiced).
    pub cb1_icdf: &'static [u8],
    /// The second stage's prediction coefficients, Q8.
    pub pred_q8: &'static [u8],
    /// Which entropy table and predictor each coefficient of each vector
    /// uses.
    pub ec_sel: &'static [u8],
    /// The second stage's entropy tables.
    pub ec_icdf: &'static [u8],
    /// The least spacing of the NLSFs, Q15 (`order + 1` values).
    pub delta_min_q15: &'static [i16],
}

/// `silk_NLSF_CB_NB_MB`: order 10.
pub(crate) static NLSF_CB_NB_MB: NlsfCodebook = NlsfCodebook {
    n_vectors: 32,
    order: 10,
    quant_step_size_q16: fix_const(0.18, 16),
    cb1_nlsf_q8: &NLSF_CB1_NB_MB_Q8,
    cb1_wght_q9: &NLSF_CB1_WGHT_Q9,
    cb1_icdf: &NLSF_CB1_ICDF_NB_MB,
    pred_q8: &NLSF_PRED_NB_MB_Q8,
    ec_sel: &NLSF_CB2_SELECT_NB_MB,
    ec_icdf: &NLSF_CB2_ICDF_NB_MB,
    delta_min_q15: &NLSF_DELTA_MIN_NB_MB_Q15,
};

/// `silk_NLSF_CB_WB`: order 16.
pub(crate) static NLSF_CB_WB: NlsfCodebook = NlsfCodebook {
    n_vectors: 32,
    order: 16,
    quant_step_size_q16: fix_const(0.15, 16),
    cb1_nlsf_q8: &NLSF_CB1_WB_Q8,
    cb1_wght_q9: &NLSF_CB1_WB_WGHT_Q9,
    cb1_icdf: &NLSF_CB1_ICDF_WB,
    pred_q8: &NLSF_PRED_WB_Q8,
    ec_sel: &NLSF_CB2_SELECT_WB,
    ec_icdf: &NLSF_CB2_ICDF_WB,
    delta_min_q15: &NLSF_DELTA_MIN_WB_Q15,
};

/// `silk_LTP_gain_iCDF_ptrs`: each LTP codebook's index table.
pub(crate) static LTP_GAIN_ICDF_PTRS: [&[u8]; 3] =
    [&LTP_GAIN_ICDF_0, &LTP_GAIN_ICDF_1, &LTP_GAIN_ICDF_2];

/// `silk_LTP_vq_ptrs_Q7`: each LTP codebook's filters, five taps each (Q7).
pub(crate) static LTP_VQ_PTRS_Q7: [&[[i8; 5]]; 3] =
    [&LTP_GAIN_VQ_0, &LTP_GAIN_VQ_1, &LTP_GAIN_VQ_2];

/// `silk_LBRR_flags_iCDF_ptr`: the redundancy flags of a 40 and a 60 ms
/// packet.
pub(crate) static LBRR_FLAGS_ICDF_PTR: [&[u8]; 2] = [&LBRR_FLAGS_2_ICDF, &LBRR_FLAGS_3_ICDF];

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "a test: a failure should be loud"
)]
mod tests {
    use super::*;

    #[test]
    fn the_step_sizes_are_libopuss() {
        // SILK_FIX_CONST(0.18, 16) and (0.15, 16).
        assert_eq!(NLSF_CB_NB_MB.quant_step_size_q16, 11796);
        assert_eq!(NLSF_CB_WB.quant_step_size_q16, 9830);
    }
}
