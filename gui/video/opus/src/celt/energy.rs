//! A CELT frame's band energies (RFC 6716 §4.3.2): coarse energy in 6 dB
//! steps, predicted from the previous frame and the band below and coded with
//! a Laplace-like model; then fine energy, a few raw bits a band; then the
//! bits left over at the end, a last bit a band in order of priority.
//!
//! Energies are log2 amplitudes in Q`DB_SHIFT` (Q10), one per band per
//! channel, kept from frame to frame (`oldEBands`).
//!
//! Translated into Rust from libopus 1.5.2's `celt/laplace.c` (the decoder)
//! and `celt/quant_bands.c` (`unquant_*`, `FIXED_POINT`), copyright
//! Xiph.Org and the contributors named in its `COPYING`, used under
//! libopus's BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "band numbers run over start..end below NB_EBANDS, channels below 2, and LM below 4: the energy arrays hold 2 * NB_EBANDS entries and the models' rows 42"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "energies in Q10 within 16 bits, the Laplace model's frequencies below 2^15, and libopus's own sums on them, which these keep within 32 bits"
)]

use super::mode::NB_EBANDS;
use crate::entdec::Decoder;
use crate::fixed::{extract16, mult16_16, pshr32, qconst16, qconst32, shl16, shl32, shr32};

/// The mean energy of each band, in Q4 (`eMeans`).
pub(crate) const E_MEANS: [i32; 25] = [
    103, 100, 92, 85, 81, 77, 72, 70, 78, 75, 73, 71, 78, 74, 69, 72, 70, 74, 76, 71, 60, 60, 60,
    60, 60,
];

/// The inter-frame prediction coefficient by frame size (0.9 to 0.5), Q15.
const PRED_COEF: [i32; 4] = [29440, 26112, 21248, 16384];
/// The prediction's decay by frame size, Q15.
const BETA_COEF: [i32; 4] = [30147, 22282, 12124, 6554];
/// The intra-frame prediction's decay, Q15.
const BETA_INTRA: i32 = 4915;

/// The Laplace models' parameters (`e_prob_model`): for each frame size,
/// inter then intra, each band's probability of 0 and decay rate (Q8).
const E_PROB_MODEL: [[[u8; 42]; 2]; 4] = [
    [
        [
            72, 127, 65, 129, 66, 128, 65, 128, 64, 128, 62, 128, 64, 128, 64, 128, 92, 78, 92, 79,
            92, 78, 90, 79, 116, 41, 115, 40, 114, 40, 132, 26, 132, 26, 145, 17, 161, 12, 176, 10,
            177, 11,
        ],
        [
            24, 179, 48, 138, 54, 135, 54, 132, 53, 134, 56, 133, 55, 132, 55, 132, 61, 114, 70,
            96, 74, 88, 75, 88, 87, 74, 89, 66, 91, 67, 100, 59, 108, 50, 120, 40, 122, 37, 97, 43,
            78, 50,
        ],
    ],
    [
        [
            83, 78, 84, 81, 88, 75, 86, 74, 87, 71, 90, 73, 93, 74, 93, 74, 109, 40, 114, 36, 117,
            34, 117, 34, 143, 17, 145, 18, 146, 19, 162, 12, 165, 10, 178, 7, 189, 6, 190, 8, 177,
            9,
        ],
        [
            23, 178, 54, 115, 63, 102, 66, 98, 69, 99, 74, 89, 71, 91, 73, 91, 78, 89, 86, 80, 92,
            66, 93, 64, 102, 59, 103, 60, 104, 60, 117, 52, 123, 44, 138, 35, 133, 31, 97, 38, 77,
            45,
        ],
    ],
    [
        [
            61, 90, 93, 60, 105, 42, 107, 41, 110, 45, 116, 38, 113, 38, 112, 38, 124, 26, 132, 27,
            136, 19, 140, 20, 155, 14, 159, 16, 158, 18, 170, 13, 177, 10, 187, 8, 192, 6, 175, 9,
            159, 10,
        ],
        [
            21, 178, 59, 110, 71, 86, 75, 85, 84, 83, 91, 66, 88, 73, 87, 72, 92, 75, 98, 72, 105,
            58, 107, 54, 115, 52, 114, 55, 112, 56, 129, 51, 132, 40, 150, 33, 140, 29, 98, 35, 77,
            42,
        ],
    ],
    [
        [
            42, 121, 96, 66, 108, 43, 111, 40, 117, 44, 123, 32, 120, 36, 119, 33, 127, 33, 134,
            34, 139, 21, 147, 23, 152, 20, 158, 25, 154, 26, 166, 21, 173, 16, 184, 13, 184, 10,
            150, 13, 139, 15,
        ],
        [
            22, 178, 63, 114, 74, 82, 84, 83, 92, 82, 103, 62, 96, 72, 96, 67, 101, 73, 107, 72,
            113, 55, 118, 52, 125, 52, 118, 52, 117, 55, 135, 49, 137, 39, 157, 32, 145, 29, 97,
            33, 77, 40,
        ],
    ],
];

const SMALL_ENERGY_ICDF: [u8; 3] = [2, 1, 0];
const MAX_FINE_BITS: i32 = 8;
const DB_SHIFT: u32 = 10;

/// The smallest probability an energy delta has (out of 32768).
const LAPLACE_MINP: u32 = 1;
/// Deltas each side of zero guaranteed a representation.
const LAPLACE_NMIN: u32 = 16;

/// `ec_laplace_get_freq1`.
fn laplace_freq1(fs0: u32, decay: i32) -> u32 {
    let ft = (32768 - LAPLACE_MINP * (2 * LAPLACE_NMIN)).wrapping_sub(fs0);
    ft.wrapping_mul((16384 - decay) as u32) >> 15
}

/// `ec_laplace_decode`: an energy delta, its probability of 0 `fs` and its
/// decay `decay` (both Q15).
pub(crate) fn laplace_decode(dec: &mut Decoder<'_>, fs: u32, decay: i32) -> i32 {
    let mut val = 0i32;
    let mut fs = fs;
    let fm = dec.decode_bin(15);
    let mut fl = 0u32;
    if fm >= fs {
        val += 1;
        fl = fs;
        fs = laplace_freq1(fs, decay) + LAPLACE_MINP;
        // The decaying part of the distribution.
        while fs > LAPLACE_MINP && fm >= fl + 2 * fs {
            fs *= 2;
            fl += fs;
            fs = (fs - 2 * LAPLACE_MINP).wrapping_mul(decay as u32) >> 15;
            fs += LAPLACE_MINP;
            val += 1;
        }
        // Beyond it, every value has probability LAPLACE_MINP.
        if fs <= LAPLACE_MINP {
            let di = (fm - fl) >> 1;
            val += di as i32;
            fl += 2 * di * LAPLACE_MINP;
        }
        if fm < fl + fs {
            val = -val;
        } else {
            fl += fs;
        }
    }
    dec.update(fl, (fl + fs).min(32768), 32768);
    val
}

/// `unquant_coarse_energy`: each band's coarse energy into `old`.
pub(crate) fn unquant_coarse(
    start: usize,
    end: usize,
    old: &mut [i32],
    intra: bool,
    dec: &mut Decoder<'_>,
    c: usize,
    lm: usize,
) {
    let prob_model = &E_PROB_MODEL[lm][usize::from(intra)];
    let mut prev = [0i32; 2];
    let (coef, beta) = if intra {
        (0, BETA_INTRA)
    } else {
        (PRED_COEF[lm], BETA_COEF[lm])
    };
    let budget = dec.storage as i32 * 8;
    for i in start..end {
        for ch in 0..c {
            let tell = dec.tell();
            let qi = if budget - tell >= 15 {
                let pi = 2 * i.min(20);
                laplace_decode(
                    dec,
                    u32::from(prob_model[pi]) << 7,
                    i32::from(prob_model[pi + 1]) << 6,
                )
            } else if budget - tell >= 2 {
                let qi = dec.icdf(&SMALL_ENERGY_ICDF, 2) as i32;
                (qi >> 1) ^ -(qi & 1)
            } else if budget - tell >= 1 {
                -i32::from(dec.bit_logp(1))
            } else {
                -1
            };
            let q = shl32(qi, DB_SHIFT);
            let at = i + ch * NB_EBANDS;
            old[at] = old[at].max(-qconst16(9.0, DB_SHIFT));
            let mut tmp = pshr32(mult16_16(coef, old[at]), 8)
                .wrapping_add(prev[ch])
                .wrapping_add(shl32(q, 7));
            tmp = tmp.max(-qconst32(28.0, DB_SHIFT + 7));
            old[at] = extract16(pshr32(tmp, 7));
            prev[ch] = prev[ch]
                .wrapping_add(shl32(q, 7))
                .wrapping_sub(mult16_16(beta, pshr32(q, 8)));
        }
    }
}

/// `unquant_fine_energy`: `fine_quant[i]` raw bits of each band's finer
/// energy.
pub(crate) fn unquant_fine(
    start: usize,
    end: usize,
    old: &mut [i32],
    fine_quant: &[i32],
    dec: &mut Decoder<'_>,
    c: usize,
) {
    for i in start..end {
        if fine_quant[i] <= 0 {
            continue;
        }
        let fq = fine_quant[i] as u32;
        for ch in 0..c {
            let q2 = dec.bits(fq) as i32;
            let offset = extract16(
                shr32(shl32(q2, DB_SHIFT) + qconst16(0.5, DB_SHIFT), fq) - qconst16(0.5, DB_SHIFT),
            );
            let at = i + ch * NB_EBANDS;
            old[at] = extract16(old[at] + offset);
        }
    }
}

/// `unquant_energy_finalise`: the bits left at the end of the frame, one a
/// band and channel, those of priority 0 first.
pub(crate) fn unquant_finalise(
    start: usize,
    end: usize,
    old: &mut [i32],
    fine_quant: &[i32],
    fine_priority: &[i32],
    mut bits_left: i32,
    dec: &mut Decoder<'_>,
    c: usize,
) {
    for prio in 0..2 {
        let mut i = start;
        while i < end && bits_left >= c as i32 {
            if fine_quant[i] >= MAX_FINE_BITS || fine_priority[i] != prio {
                i += 1;
                continue;
            }
            for ch in 0..c {
                let q2 = dec.bits(1) as i32;
                let offset = extract16(
                    (shl16(q2, DB_SHIFT) - qconst16(0.5, DB_SHIFT)) >> (fine_quant[i] + 1),
                );
                let at = i + ch * NB_EBANDS;
                old[at] = extract16(old[at] + offset);
                bits_left -= 1;
            }
            i += 1;
        }
    }
}
