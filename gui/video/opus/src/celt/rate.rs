//! CELT's bit allocation (RFC 6716 §4.3.3): how a frame's bits are shared
//! among its bands -- interpolated between rows of the allocation table, tilted
//! by the trim, boosted by the dynamic allocation, bands skipped from the top
//! as the bitstream says -- and split in each band between fine energy and
//! the PVQ shape. The decoder's half of `clt_compute_allocation`.
//!
//! Translated into Rust from libopus 1.5.2's `celt/rate.c`, copyright
//! Xiph.Org and the contributors named in its `COPYING`, used under libopus's
//! BSD licence (`licenses/libopus-COPYING`).

#![allow(
    clippy::indexing_slicing,
    reason = "band numbers run over start..end within NB_EBANDS, every per-band slice the caller passes holds NB_EBANDS entries, and the allocation table's rows are indexed by the bisection's bounds (0..NB_ALLOC_VECTORS)"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "bit counts in eighths of a frame's at most 10,200 bits, band widths below 100 and shifts below 8: libopus's int arithmetic, which these keep far from overflow"
)]

use super::mode::{NB_ALLOC_VECTORS, NB_EBANDS, alloc_vector, ebands, log_n};
use crate::entdec::{BITRES, Decoder};

const ALLOC_STEPS: i32 = 6;
const MAX_FINE_BITS: i32 = 8;
const FINE_OFFSET: i32 = 21;
const BITRES_I: i32 = BITRES as i32;

/// `LOG2_FRAC_TABLE`: the cost of coding the intensity band, by how many
/// bands it may be.
const LOG2_FRAC_TABLE: [i32; 24] = [
    0, 8, 13, 16, 19, 21, 23, 24, 26, 27, 28, 29, 30, 31, 32, 32, 33, 34, 34, 35, 36, 36, 37, 37,
];

/// What the allocation decided beyond each band's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Allocation {
    /// Bands coded; those above are skipped.
    pub coded_bands: usize,
    /// The first band coded as intensity stereo.
    pub intensity: i32,
    pub dual_stereo: bool,
    /// Bits over the bands' caps, for `quant_all_bands` to rebalance.
    pub balance: i32,
}

/// `celt_udiv` of a signed numerator: C converts it to unsigned first, and
/// the quotient back.
fn udiv(n: i32, d: i32) -> i32 {
    (n as u32).checked_div(d as u32).unwrap_or(0) as i32
}

/// `clt_compute_allocation`, decoding: each band's PVQ bits into `pulses`,
/// fine energy bits into `ebits` and the fine-energy priority into
/// `fine_priority` (all in eighths of a bit, but `ebits` in whole bits).
pub(crate) fn compute_allocation(
    start: usize,
    end: usize,
    offsets: &[i32],
    cap: &[i32],
    alloc_trim: i32,
    total: i32,
    pulses: &mut [i32],
    ebits: &mut [i32],
    fine_priority: &mut [i32],
    c: i32,
    lm: i32,
    ec: &mut Decoder<'_>,
) -> Allocation {
    let mut total = total.max(0);
    let len = NB_EBANDS;
    let mut skip_start = start;
    // A bit to signal the end of the bands skipped by hand.
    let skip_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
    total -= skip_rsv;
    // Bits for the intensity and dual stereo parameters.
    let (mut intensity_rsv, mut dual_stereo_rsv) = (0, 0);
    if c == 2 {
        intensity_rsv = LOG2_FRAC_TABLE[end - start];
        if intensity_rsv > total {
            intensity_rsv = 0;
        } else {
            total -= intensity_rsv;
            dual_stereo_rsv = if total >= 1 << BITRES { 1 << BITRES } else { 0 };
            total -= dual_stereo_rsv;
        }
    }
    let mut bits1 = [0i32; NB_EBANDS];
    let mut bits2 = [0i32; NB_EBANDS];
    let mut thresh = [0i32; NB_EBANDS];
    let mut trim_offset = [0i32; NB_EBANDS];
    for j in start..end {
        let width = ebands(j + 1) - ebands(j);
        // Below this, no PVQ bits are allocated for sure.
        thresh[j] = (c << BITRES).max(((3 * width) << lm << BITRES) >> 4);
        // The allocation curve's tilt.
        trim_offset[j] = (c
            * width
            * (alloc_trim - 5 - lm)
            * (end as i32 - j as i32 - 1)
            * (1 << (lm + BITRES_I)))
            >> 6;
        // Less resolution to single-coefficient bands.
        if width << lm == 1 {
            trim_offset[j] -= c << BITRES;
        }
    }
    let (mut lo, mut hi) = (1i32, NB_ALLOC_VECTORS as i32 - 1);
    loop {
        let mut done = false;
        let mut psum = 0;
        let mid = (lo + hi) >> 1;
        for j in (start..end).rev() {
            let n = ebands(j + 1) - ebands(j);
            let mut bitsj = ((c * n * alloc_vector(mid as usize * len + j)) << lm) >> 2;
            if bitsj > 0 {
                bitsj = (bitsj + trim_offset[j]).max(0);
            }
            bitsj += offsets[j];
            if bitsj >= thresh[j] || done {
                done = true;
                // Not more than can be used.
                psum += bitsj.min(cap[j]);
            } else if bitsj >= c << BITRES {
                psum += c << BITRES;
            }
        }
        if psum > total {
            hi = mid - 1;
        } else {
            lo = mid + 1;
        }
        if lo > hi {
            break;
        }
    }
    hi = lo;
    lo -= 1;
    for j in start..end {
        let n = ebands(j + 1) - ebands(j);
        let mut bits1j = ((c * n * alloc_vector(lo as usize * len + j)) << lm) >> 2;
        let mut bits2j = if hi as usize >= NB_ALLOC_VECTORS {
            cap[j]
        } else {
            ((c * n * alloc_vector(hi as usize * len + j)) << lm) >> 2
        };
        if bits1j > 0 {
            bits1j = (bits1j + trim_offset[j]).max(0);
        }
        if bits2j > 0 {
            bits2j = (bits2j + trim_offset[j]).max(0);
        }
        if lo > 0 {
            bits1j += offsets[j];
        }
        bits2j += offsets[j];
        if offsets[j] > 0 {
            skip_start = j;
        }
        bits2j = (bits2j - bits1j).max(0);
        bits1[j] = bits1j;
        bits2[j] = bits2j;
    }
    interp_bits2pulses(
        start,
        end,
        skip_start,
        &bits1,
        &bits2,
        &thresh,
        cap,
        total,
        skip_rsv,
        intensity_rsv,
        dual_stereo_rsv,
        pulses,
        ebits,
        fine_priority,
        c,
        lm,
        ec,
    )
}

/// `interp_bits2pulses`, decoding.
fn interp_bits2pulses(
    start: usize,
    end: usize,
    skip_start: usize,
    bits1: &[i32],
    bits2: &[i32],
    thresh: &[i32],
    cap: &[i32],
    mut total: i32,
    skip_rsv: i32,
    mut intensity_rsv: i32,
    mut dual_stereo_rsv: i32,
    bits: &mut [i32],
    ebits: &mut [i32],
    fine_priority: &mut [i32],
    c: i32,
    lm: i32,
    ec: &mut Decoder<'_>,
) -> Allocation {
    let alloc_floor = c << BITRES;
    let stereo = i32::from(c > 1);
    let log_m = lm << BITRES;
    let (mut lo, mut hi) = (0i32, 1i32 << ALLOC_STEPS);
    for _ in 0..ALLOC_STEPS {
        let mid = (lo + hi) >> 1;
        let mut psum = 0;
        let mut done = false;
        for j in (start..end).rev() {
            let tmp = bits1[j] + ((mid * bits2[j]) >> ALLOC_STEPS);
            if tmp >= thresh[j] || done {
                done = true;
                psum += tmp.min(cap[j]);
            } else if tmp >= alloc_floor {
                psum += alloc_floor;
            }
        }
        if psum > total {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    let mut psum = 0;
    let mut done = false;
    for j in (start..end).rev() {
        let mut tmp = bits1[j] + ((lo * bits2[j]) >> ALLOC_STEPS);
        if tmp < thresh[j] && !done {
            tmp = if tmp >= alloc_floor { alloc_floor } else { 0 };
        } else {
            done = true;
        }
        tmp = tmp.min(cap[j]);
        bits[j] = tmp;
        psum += tmp;
    }

    // Which bands to skip, from the top down.
    let mut coded_bands = end;
    loop {
        let j = coded_bands - 1;
        // Never the first band, nor one boosted by the dynamic allocation.
        if j <= skip_start {
            total += skip_rsv;
            break;
        }
        // The bits left over that this band would gain.
        let mut left = total - psum;
        let span = ebands(coded_bands) - ebands(start);
        let percoeff = udiv(left, span);
        left -= span * percoeff;
        let rem = (left - (ebands(j) - ebands(start))).max(0);
        let band_width = ebands(coded_bands) - ebands(j);
        let mut band_bits = bits[j] + percoeff * band_width + rem;
        // A skip decision is coded only above the band's threshold;
        // otherwise the band is skipped without one.
        if band_bits >= thresh[j].max(alloc_floor + (1 << BITRES)) {
            if ec.bit_logp(1) {
                break;
            }
            // A bit was spent skipping this band.
            psum += 1 << BITRES;
            band_bits -= 1 << BITRES;
        }
        // Take back the band's bits.
        psum -= bits[j] + intensity_rsv;
        if intensity_rsv > 0 {
            intensity_rsv = LOG2_FRAC_TABLE[j - start];
        }
        psum += intensity_rsv;
        if band_bits >= alloc_floor {
            // Enough for a fine energy bit a channel.
            psum += alloc_floor;
            bits[j] = alloc_floor;
        } else {
            bits[j] = 0;
        }
        coded_bands -= 1;
    }

    // The intensity and dual stereo parameters.
    let intensity = if intensity_rsv > 0 {
        start as i32 + ec.uint((coded_bands + 1 - start) as u32) as i32
    } else {
        0
    };
    if intensity <= start as i32 {
        total += dual_stereo_rsv;
        dual_stereo_rsv = 0;
    }
    let dual_stereo = if dual_stereo_rsv > 0 {
        ec.bit_logp(1)
    } else {
        false
    };

    // The bits that remain.
    let mut left = total - psum;
    let span = ebands(coded_bands) - ebands(start);
    let percoeff = udiv(left, span);
    left -= span * percoeff;
    for j in start..coded_bands {
        bits[j] += percoeff * (ebands(j + 1) - ebands(j));
    }
    for j in start..coded_bands {
        let tmp = left.min(ebands(j + 1) - ebands(j));
        bits[j] += tmp;
        left -= tmp;
    }

    let mut balance = 0;
    let mut j = start;
    while j < coded_bands {
        let n0 = ebands(j + 1) - ebands(j);
        let n = n0 << lm;
        let bit = bits[j] + balance;
        let mut excess;
        if n > 1 {
            excess = (bit - cap[j]).max(0);
            bits[j] = bit - excess;
            // The extra degree of freedom of stereo.
            let den = c * n + i32::from(c == 2 && n > 2 && !dual_stereo && (j as i32) < intensity);
            let nclogn = den * (log_n(j) + log_m);
            // The fine bits' offset: log2(N)/2 + FINE_OFFSET from their fair
            // share.
            let mut offset = (nclogn >> 1) - den * FINE_OFFSET;
            // N=2 is the one point off the curve.
            if n == 2 {
                offset += den << BITRES >> 2;
            }
            // The offset for the second and third fine bits.
            if bits[j] + offset < (den * 2) << BITRES {
                offset += nclogn >> 2;
            } else if bits[j] + offset < (den * 3) << BITRES {
                offset += nclogn >> 3;
            }
            // Divided, rounding.
            ebits[j] = (bits[j] + offset + (den << (BITRES - 1))).max(0);
            ebits[j] = udiv(ebits[j], den) >> BITRES;
            // Not more than the band has.
            if c * ebits[j] > bits[j] >> BITRES {
                ebits[j] = bits[j] >> stereo >> BITRES;
            }
            // More is useless: about as far as PVQ goes.
            ebits[j] = ebits[j].min(MAX_FINE_BITS);
            // Rounded down or capped: a candidate for the final fine pass.
            fine_priority[j] = i32::from(ebits[j] * (den << BITRES) >= bits[j] + offset);
            // The rest are PVQ's.
            bits[j] -= (c * ebits[j]) << BITRES;
        } else {
            // One coefficient: all to fine energy but a sign bit.
            excess = (bit - (c << BITRES)).max(0);
            bits[j] = bit - excess;
            ebits[j] = 0;
            fine_priority[j] = 1;
        }
        // Fine energy cannot use quant_all_bands' rebalancing: done here.
        if excess > 0 {
            let extra_fine = (excess >> (stereo + BITRES_I)).min(MAX_FINE_BITS - ebits[j]);
            ebits[j] += extra_fine;
            let extra_bits = (extra_fine * c) << BITRES;
            fine_priority[j] = i32::from(extra_bits >= excess - balance);
            excess -= extra_bits;
        }
        balance = excess;
        j += 1;
    }

    // Skipped bands spend all their bits on fine energy.
    while j < end {
        ebits[j] = bits[j] >> stereo >> BITRES;
        bits[j] = 0;
        fine_priority[j] = i32::from(ebits[j] < 1);
        j += 1;
    }
    Allocation {
        coded_bands,
        intensity,
        dual_stereo,
        balance,
    }
}
