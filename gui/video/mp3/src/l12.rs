//! Layers I and II: bit allocation, scale factors and samples, dequantized
//! into the granule buffer -- minimp3's `L12_*` functions, translated into
//! Rust (minimp3, CC0: `licenses/minimp3-LICENSE`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "minimp3's arithmetic on fields of a few bits, translated as it is"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "band and sample indices bounded by the tables' sizes, as in minimp3"
)]

use crate::header::{Bits, Hdr};
use crate::tables::{G_BITALLOC_CODE_TAB, G_DEQ_L12};

/// `L12_scale_info`.
#[derive(Clone)]
pub(crate) struct ScaleInfo {
    pub scf: [f32; 3 * 64],
    pub total_bands: u8,
    pub stereo_bands: u8,
    pub bitalloc: [u8; 64],
    pub scfcod: [u8; 64],
}

impl Default for ScaleInfo {
    fn default() -> Self {
        Self {
            scf: [0.0; 192],
            total_bands: 0,
            stereo_bands: 0,
            bitalloc: [0; 64],
            scfcod: [0; 64],
        }
    }
}

/// `L12_subband_alloc_t`.
#[derive(Clone, Copy)]
struct Alloc {
    tab_offset: u8,
    code_tab_width: u8,
    band_count: u8,
}

const fn alloc(tab_offset: u8, code_tab_width: u8, band_count: u8) -> Alloc {
    Alloc {
        tab_offset,
        code_tab_width,
        band_count,
    }
}

static ALLOC_L1: [Alloc; 1] = [alloc(76, 4, 32)];
static ALLOC_L2M2: [Alloc; 3] = [alloc(60, 4, 4), alloc(44, 3, 7), alloc(44, 2, 19)];
static ALLOC_L2M1: [Alloc; 4] = [
    alloc(0, 4, 3),
    alloc(16, 4, 8),
    alloc(32, 3, 12),
    alloc(40, 2, 7),
];
static ALLOC_L2M1_LOWRATE: [Alloc; 2] = [alloc(44, 4, 2), alloc(44, 3, 10)];

/// `L12_subband_alloc_table`: the allocation table, and the bands.
fn subband_alloc_table(hdr: Hdr, sci: &mut ScaleInfo) -> &'static [Alloc] {
    let mode = hdr.stereo_mode();
    let stereo_bands = if mode == 3 {
        0
    } else if mode == 1 {
        (u32::from(hdr.stereo_mode_ext()) << 2) + 4
    } else {
        32
    };
    let (table, nbands): (&'static [Alloc], u32) = if hdr.is_layer_1() {
        (&ALLOC_L1, 32)
    } else if !hdr.test_mpeg1() {
        (&ALLOC_L2M2, 30)
    } else {
        let sample_rate_idx = hdr.sample_rate();
        let mut kbps = hdr.bitrate_kbps() >> u32::from(mode != 3);
        if kbps == 0 {
            // Free format.
            kbps = 192;
        }
        if kbps < 56 {
            (
                &ALLOC_L2M1_LOWRATE,
                if sample_rate_idx == 2 { 12 } else { 8 },
            )
        } else if kbps >= 96 && sample_rate_idx != 1 {
            (&ALLOC_L2M1, 30)
        } else {
            (&ALLOC_L2M1, 27)
        }
    };
    sci.total_bands = nbands as u8;
    sci.stereo_bands = stereo_bands.min(nbands) as u8;
    table
}

/// `L12_read_scalefactors`.
fn read_scalefactors(bs: &mut Bits<'_>, pba: &[u8], scfcod: &[u8], bands: usize, scf: &mut [f32]) {
    let mut out = 0usize;
    for i in 0..bands {
        let mut s = 0.0f32;
        let ba = i32::from(pba[i]);
        let mask = if ba != 0 {
            4 + ((19 >> scfcod[i]) & 3)
        } else {
            0
        };
        let mut m = 4;
        while m != 0 {
            if mask & m != 0 {
                let b = bs.get(6) as i32;
                s = G_DEQ_L12[(ba * 3 - 6 + b % 3) as usize] * ((1 << 21 >> (b / 3)) as f32);
            }
            scf[out] = s;
            out += 1;
            m >>= 1;
        }
    }
}

/// `L12_read_scale_info`.
pub(crate) fn read_scale_info(hdr: Hdr, bs: &mut Bits<'_>, sci: &mut ScaleInfo) {
    let table = subband_alloc_table(hdr, sci);
    let mut alloc = table.iter();
    let mut k = 0usize;
    let mut ba_bits = 0u32;
    let mut ba_code_tab = 0usize;
    for i in 0..usize::from(sci.total_bands) {
        if i == k
            && let Some(a) = alloc.next()
        {
            k += usize::from(a.band_count);
            ba_bits = u32::from(a.code_tab_width);
            ba_code_tab = usize::from(a.tab_offset);
        }
        let mut ba = G_BITALLOC_CODE_TAB[ba_code_tab + bs.get(ba_bits) as usize];
        sci.bitalloc[2 * i] = ba;
        if i < usize::from(sci.stereo_bands) {
            ba = G_BITALLOC_CODE_TAB[ba_code_tab + bs.get(ba_bits) as usize];
        }
        sci.bitalloc[2 * i + 1] = if sci.stereo_bands != 0 { ba } else { 0 };
    }
    for i in 0..2 * usize::from(sci.total_bands) {
        sci.scfcod[i] = if sci.bitalloc[i] != 0 {
            if hdr.is_layer_1() { 2 } else { bs.get(2) as u8 }
        } else {
            6
        };
    }
    let bands = usize::from(sci.total_bands) * 2;
    let (bitalloc, scfcod) = (sci.bitalloc, sci.scfcod);
    read_scalefactors(bs, &bitalloc, &scfcod, bands, &mut sci.scf);
    for i in usize::from(sci.stereo_bands)..usize::from(sci.total_bands) {
        sci.bitalloc[2 * i + 1] = 0;
    }
}

/// `L12_dequantize_granule`: `group_size` samples a band, four times, into
/// `grbuf` from `at`; returns the samples a band written.
pub(crate) fn dequantize_granule(
    grbuf: &mut [f32],
    at: usize,
    bs: &mut Bits<'_>,
    sci: &ScaleInfo,
    group_size: usize,
) -> usize {
    let mut choff = 576usize;
    for j in 0..4 {
        let mut dst = at + group_size * j;
        for i in 0..2 * usize::from(sci.total_bands) {
            let ba = u32::from(sci.bitalloc[i]);
            if ba != 0 {
                if ba < 17 {
                    let half = (1i32 << (ba - 1)) - 1;
                    for k in 0..group_size {
                        grbuf[dst + k] = (bs.get(ba) as i32 - half) as f32;
                    }
                } else {
                    let modulus = (2u32 << (ba - 17)) + 1;
                    let mut code = bs.get(modulus + 2 - (modulus >> 3));
                    for k in 0..group_size {
                        grbuf[dst + k] = ((code % modulus) as i32 - (modulus / 2) as i32) as f32;
                        code /= modulus;
                    }
                }
            }
            dst = dst.wrapping_add(choff);
            choff = 18usize.wrapping_sub(choff);
        }
    }
    group_size * 4
}

/// `L12_apply_scf_384`.
pub(crate) fn apply_scf_384(sci: &ScaleInfo, scf: &[f32], dst: &mut [f32]) {
    let stereo = usize::from(sci.stereo_bands) * 18;
    let total = usize::from(sci.total_bands);
    let n = (total - usize::from(sci.stereo_bands)) * 18;
    dst.copy_within(stereo..stereo + n, 576 + stereo);
    let mut d = 0usize;
    let mut s = 0usize;
    for _ in 0..total {
        for k in 0..12 {
            dst[d + k] *= scf[s];
            dst[d + k + 576] *= scf[s + 3];
        }
        d += 18;
        s += 6;
    }
}
