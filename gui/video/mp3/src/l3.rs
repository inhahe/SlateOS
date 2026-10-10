//! Layer III: side information, scale factors, the Huffman-coded spectrum,
//! stereo processing, reordering, alias reduction and the IMDCT --
//! minimp3's `L3_*` functions, translated into Rust (minimp3, CC0:
//! `licenses/minimp3-LICENSE`).
//!
//! The floating-point operations are minimp3's scalar ones, in its order:
//! the decoder is held to minimp3 built without SIMD sample for sample, and
//! a reordered sum would round differently. Two places differ from minimp3,
//! for the reasons the crate's documentation gives: [`read_side_info`] (the
//! private bits) and [`Hdr::is_i_stereo`] (the intensity-stereo bit outside
//! joint stereo).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    reason = "minimp3's arithmetic on fields of a few bits and on indices bounded by its tables, translated as it is"
)]
#![allow(
    clippy::indexing_slicing,
    reason = "indices bounded by minimp3's tables: every scale-factor band table covers 576 lines and every Huffman lookup stays inside its table (tools/verify_tables.py)"
)]
#![allow(
    clippy::excessive_precision,
    clippy::unreadable_literal,
    reason = "minimp3's own constants, as written"
)]

use core::f32::consts::SQRT_2;

use crate::header::{Bits, Hdr};
use crate::tables::{
    G_AA, G_EXPFRAC, G_LINBITS, G_MDCT_WINDOW, G_MOD, G_PAN, G_POW43, G_PREAMP, G_SCF_LONG,
    G_SCF_MIXED, G_SCF_PARTITIONS, G_SCF_SHORT, G_SCFC_DECODE, G_TWID3, G_TWID9, TAB32, TAB33,
    TABINDEX, TABS,
};

pub(crate) const SHORT_BLOCK_TYPE: u8 = 2;
pub(crate) const STOP_BLOCK_TYPE: u8 = 3;
const BITS_DEQUANTIZER_OUT: i32 = -1;
const MAX_SCF: i32 = 255 + BITS_DEQUANTIZER_OUT * 4 - 210;
const MAX_SCFI: i32 = (MAX_SCF + 3) & !3;

/// `L3_gr_info_t`: one granule's side information for one channel.
#[derive(Clone, Copy)]
pub(crate) struct GrInfo {
    /// The scale-factor band widths, ending in 0: a row of `g_scf_long`,
    /// `g_scf_short` or `g_scf_mixed`.
    pub sfbtab: &'static [u8],
    pub part_23_length: u16,
    pub big_values: u16,
    pub scalefac_compress: u16,
    pub global_gain: u8,
    pub block_type: u8,
    pub mixed_block_flag: u8,
    pub n_long_sfb: u8,
    pub n_short_sfb: u8,
    pub table_select: [u8; 3],
    pub region_count: [u8; 3],
    pub subblock_gain: [u8; 3],
    pub preflag: u8,
    pub scalefac_scale: u8,
    pub count1_table: u8,
    pub scfsi: u8,
}

impl Default for GrInfo {
    fn default() -> Self {
        Self {
            sfbtab: long_row(0),
            part_23_length: 0,
            big_values: 0,
            scalefac_compress: 0,
            global_gain: 0,
            block_type: 0,
            mixed_block_flag: 0,
            n_long_sfb: 0,
            n_short_sfb: 0,
            table_select: [0; 3],
            region_count: [0; 3],
            subblock_gain: [0; 3],
            preflag: 0,
            scalefac_scale: 0,
            count1_table: 0,
            scfsi: 0,
        }
    }
}

fn long_row(sr_idx: usize) -> &'static [u8] {
    &G_SCF_LONG[sr_idx * 23..sr_idx * 23 + 23]
}

fn short_row(sr_idx: usize) -> &'static [u8] {
    &G_SCF_SHORT[sr_idx * 40..sr_idx * 40 + 40]
}

fn mixed_row(sr_idx: usize) -> &'static [u8] {
    &G_SCF_MIXED[sr_idx * 40..sr_idx * 40 + 40]
}

/// `L3_read_side_info`: the granules' side information into `gr`; where
/// the frame's main data begins, back in the reservoir, or `None` where the
/// side information is impossible.
///
/// **Differs from minimp3:** the private bits are dropped before the scale
/// factor selection information is spread over the granules. minimp3 reads
/// them with it, and its shifting leaves them in the first granule's `scfsi`
/// -- the second channel's in stereo, the first in mono -- where a set bit
/// copies scale factors from a granule that does not exist (memory the frame
/// never wrote). The standard gives the first granule no `scfsi`, and every
/// encoder in common use writes the private bits as 0, where the two agree.
pub(crate) fn read_side_info(bs: &mut Bits<'_>, gr: &mut [GrInfo; 4], hdr: Hdr) -> Option<usize> {
    let mut scfsi: u32 = 0;
    let mut part_23_sum: usize = 0;
    let mut sr_idx = hdr.my_sample_rate() as usize;
    sr_idx -= usize::from(sr_idx != 0);
    let mut gr_count: u32 = if hdr.is_mono() { 1 } else { 2 };
    let main_data_begin;
    if hdr.test_mpeg1() {
        gr_count *= 2;
        main_data_begin = bs.get(9);
        // The private bits (5 in mono, 3 in stereo) above the scfsi bits.
        scfsi = bs.get(7 + gr_count) & ((1 << (2 * gr_count)) - 1);
    } else {
        main_data_begin = bs.get(8 + gr_count) >> gr_count;
    }

    for g in gr.iter_mut().take(gr_count as usize) {
        if hdr.is_mono() {
            scfsi <<= 4;
        }
        g.part_23_length = bs.get(12) as u16;
        part_23_sum += usize::from(g.part_23_length);
        g.big_values = bs.get(9) as u16;
        if g.big_values > 288 {
            return None;
        }
        g.global_gain = bs.get(8) as u8;
        g.scalefac_compress = bs.get(if hdr.test_mpeg1() { 4 } else { 9 }) as u16;
        g.sfbtab = long_row(sr_idx);
        g.n_long_sfb = 22;
        g.n_short_sfb = 0;
        let tables;
        if bs.get(1) != 0 {
            g.block_type = bs.get(2) as u8;
            if g.block_type == 0 {
                return None;
            }
            g.mixed_block_flag = bs.get(1) as u8;
            g.region_count[0] = 7;
            g.region_count[1] = 255;
            if g.block_type == SHORT_BLOCK_TYPE {
                scfsi &= 0x0F0F;
                if g.mixed_block_flag == 0 {
                    g.region_count[0] = 8;
                    g.sfbtab = short_row(sr_idx);
                    g.n_long_sfb = 0;
                    g.n_short_sfb = 39;
                } else {
                    g.sfbtab = mixed_row(sr_idx);
                    g.n_long_sfb = if hdr.test_mpeg1() { 8 } else { 6 };
                    g.n_short_sfb = 30;
                }
            }
            tables = bs.get(10) << 5;
            g.subblock_gain[0] = bs.get(3) as u8;
            g.subblock_gain[1] = bs.get(3) as u8;
            g.subblock_gain[2] = bs.get(3) as u8;
        } else {
            g.block_type = 0;
            g.mixed_block_flag = 0;
            tables = bs.get(15);
            g.region_count[0] = bs.get(4) as u8;
            g.region_count[1] = bs.get(3) as u8;
            g.region_count[2] = 255;
        }
        g.table_select[0] = (tables >> 10) as u8;
        g.table_select[1] = ((tables >> 5) & 31) as u8;
        g.table_select[2] = (tables & 31) as u8;
        g.preflag = if hdr.test_mpeg1() {
            bs.get(1) as u8
        } else {
            u8::from(g.scalefac_compress >= 500)
        };
        g.scalefac_scale = bs.get(1) as u8;
        g.count1_table = bs.get(1) as u8;
        g.scfsi = ((scfsi >> 12) & 15) as u8;
        scfsi <<= 4;
    }

    if part_23_sum + bs.pos > bs.limit + main_data_begin as usize * 8 {
        return None;
    }
    Some(main_data_begin as usize)
}

/// `L3_read_scalefactors`: a granule's scale factors into `scf` (and, for
/// intensity stereo's sake, into `ist_pos`, the largest value marked 255),
/// partition by partition; three zeros after the last.
fn read_scalefactors(
    scf: &mut [u8; 40],
    ist_pos: &mut [u8; 39],
    scf_size: [u8; 4],
    scf_count: &[u8],
    bitbuf: &mut Bits<'_>,
    mut scfsi: i32,
) {
    let mut at = 0usize;
    let mut i = 0usize;
    while i < 4 && scf_count[i] != 0 {
        let cnt = usize::from(scf_count[i]);
        if scfsi & 8 != 0 {
            scf[at..at + cnt].copy_from_slice(&ist_pos[at..at + cnt]);
        } else {
            let bits = u32::from(scf_size[i]);
            if bits == 0 {
                scf[at..at + cnt].fill(0);
                ist_pos[at..at + cnt].fill(0);
            } else {
                let max_scf: i32 = if scfsi < 0 { (1 << bits) - 1 } else { -1 };
                for k in 0..cnt {
                    let s = bitbuf.get(bits) as i32;
                    ist_pos[at + k] = if s == max_scf { u8::MAX } else { s as u8 };
                    scf[at + k] = s as u8;
                }
            }
        }
        at += cnt;
        i += 1;
        scfsi *= 2;
    }
    scf[at] = 0;
    scf[at + 1] = 0;
    scf[at + 2] = 0;
}

/// `L3_ldexp_q2`: `y` times 2 to the `-exp_q2 / 4`.
pub(crate) fn ldexp_q2(mut y: f32, mut exp_q2: i32) -> f32 {
    loop {
        let e = exp_q2.min(30 * 4);
        y *= G_EXPFRAC[(e & 3) as usize] * ((1i32 << 30 >> (e >> 2)) as f32);
        exp_q2 -= e;
        if exp_q2 <= 0 {
            return y;
        }
    }
}

/// `L3_decode_scalefactors`: a granule's scale factors, read and turned
/// into the gains its bands are multiplied by, into `scf`.
pub(crate) fn decode_scalefactors(
    hdr: Hdr,
    ist_pos: &mut [u8; 39],
    bs: &mut Bits<'_>,
    gr: &GrInfo,
    scf: &mut [f32; 40],
    ch: usize,
) {
    let row = usize::from(gr.n_short_sfb != 0) + usize::from(gr.n_long_sfb == 0);
    let mut partition = row * 28;
    let mut scf_size = [0u8; 4];
    let mut iscf = [0u8; 40];
    let scf_shift = u32::from(gr.scalefac_scale) + 1;
    let mut scfsi = i32::from(gr.scfsi);

    if hdr.test_mpeg1() {
        let part = G_SCFC_DECODE[usize::from(gr.scalefac_compress)];
        scf_size[0] = part >> 2;
        scf_size[1] = part >> 2;
        scf_size[2] = part & 3;
        scf_size[3] = part & 3;
    } else {
        let ist = usize::from(hdr.is_i_stereo() && ch != 0);
        let mut sfc = i32::from(gr.scalefac_compress >> ist);
        let mut k = ist * 3 * 4;
        while sfc >= 0 {
            let mut modprod = 1i32;
            for i in (0..4).rev() {
                scf_size[i] = (sfc / modprod % i32::from(G_MOD[k + i])) as u8;
                modprod *= i32::from(G_MOD[k + i]);
            }
            sfc -= modprod;
            k += 4;
        }
        partition += k;
        scfsi = -16;
    }
    read_scalefactors(
        &mut iscf,
        ist_pos,
        scf_size,
        &G_SCF_PARTITIONS[partition..],
        bs,
        scfsi,
    );

    if gr.n_short_sfb != 0 {
        let sh = 3 - scf_shift;
        let base = usize::from(gr.n_long_sfb);
        for i in (0..usize::from(gr.n_short_sfb)).step_by(3) {
            iscf[base + i] = iscf[base + i].wrapping_add(gr.subblock_gain[0] << sh);
            iscf[base + i + 1] = iscf[base + i + 1].wrapping_add(gr.subblock_gain[1] << sh);
            iscf[base + i + 2] = iscf[base + i + 2].wrapping_add(gr.subblock_gain[2] << sh);
        }
    } else if gr.preflag != 0 {
        for (i, preamp) in G_PREAMP.iter().enumerate() {
            iscf[11 + i] = iscf[11 + i].wrapping_add(*preamp);
        }
    }

    let gain_exp = i32::from(gr.global_gain) + BITS_DEQUANTIZER_OUT * 4
        - 210
        - if hdr.is_ms_stereo() { 2 } else { 0 };
    let gain = ldexp_q2((1i32 << (MAX_SCFI / 4)) as f32, MAX_SCFI - gain_exp);
    for i in 0..usize::from(gr.n_long_sfb + gr.n_short_sfb) {
        scf[i] = ldexp_q2(gain, i32::from(iscf[i]) << scf_shift);
    }
}

/// `L3_pow_43`: `x` to the power 4/3, for `x` past the table.
fn pow_43(mut x: i32) -> f32 {
    let mut mult = 256i32;
    if x < 129 {
        return G_POW43[(16 + x) as usize];
    }
    if x < 1024 {
        mult = 16;
        x <<= 3;
    }
    let sign = (2 * x) & 64;
    let frac = ((x & 63) - sign) as f32 / ((x & !63) + sign) as f32;
    G_POW43[(16 + ((x + sign) >> 6)) as usize]
        * (1.0 + frac * ((4.0f32 / 3.0) + frac * (2.0f32 / 9.0)))
        * mult as f32
}

/// minimp3's Huffman bit cache (`bs_cache`, `bs_sh`, `bs_next_ptr`): at
/// least 25 bits ahead after each [`Cache::check`]. Its reads run ahead of
/// what it uses, into the next granule's bits or past the main data; a byte
/// past the buffer reads as 0 (minimp3 reads its neighbouring fields).
struct Cache<'a> {
    buf: &'a [u8],
    next: usize,
    bits: u32,
    sh: i32,
}

impl<'a> Cache<'a> {
    fn new(buf: &'a [u8], pos: usize) -> Self {
        let mut c = Cache {
            buf,
            next: pos / 8,
            bits: 0,
            sh: (pos & 7) as i32 - 8,
        };
        c.bits = (((c.byte(c.next) * 256 + c.byte(c.next + 1)) * 256 + c.byte(c.next + 2)) * 256
            + c.byte(c.next + 3))
            << (pos & 7);
        c.next += 4;
        c
    }

    fn byte(&self, at: usize) -> u32 {
        u32::from(self.buf.get(at).copied().unwrap_or(0))
    }

    /// `PEEK_BITS`: the next `n` bits, 1 to 13.
    fn peek(&self, n: u32) -> u32 {
        self.bits.wrapping_shr(32 - n)
    }

    /// `FLUSH_BITS`.
    fn flush(&mut self, n: u32) {
        self.bits = self.bits.wrapping_shl(n);
        self.sh += n as i32;
    }

    /// `CHECK_BITS`: whole bytes in, until at least 25 bits are ahead.
    fn check(&mut self) {
        while self.sh >= 0 {
            self.bits |= self.byte(self.next).wrapping_shl(self.sh as u32);
            self.next += 1;
            self.sh -= 8;
        }
    }

    /// `BSPOS`: the position in bits.
    fn pos(&self) -> i64 {
        self.next as i64 * 8 - 24 + i64::from(self.sh)
    }

    /// Whether the next bit, a sign, is set.
    fn negative(&self) -> bool {
        (self.bits as i32) < 0
    }
}

/// `L3_huffman`'s state as it moves through a granule's bands: the bit
/// cache, the next band's width and gain, the gain of the band it is in,
/// and the next line to write.
struct Spectrum<'a> {
    c: Cache<'a>,
    sfb: &'static [u8],
    sfb_at: usize,
    scf: &'a [f32; 40],
    scf_at: usize,
    one: f32,
    d: usize,
}

impl Spectrum<'_> {
    /// The big values: pairs, region by region, each region's own table;
    /// what is left of the last band's pairs, as minimp3's `big_val_cnt`
    /// (0 or less).
    fn big_values(&mut self, dst: &mut [f32], gr_info: &GrInfo) -> i32 {
        let mut big_val_cnt = i32::from(gr_info.big_values);
        let mut ireg = 0usize;
        while big_val_cnt > 0 {
            let tab_num = usize::from(gr_info.table_select[ireg]);
            let mut sfb_cnt = i32::from(gr_info.region_count[ireg]);
            ireg += 1;
            let codebook = &TABS[TABINDEX[tab_num] as usize..];
            let linbits = u32::from(G_LINBITS[tab_num]);
            loop {
                let np = i32::from(self.sfb[self.sfb_at]) / 2;
                self.sfb_at += 1;
                let mut pairs_to_decode = big_val_cnt.min(np);
                self.one = self.scf[self.scf_at];
                self.scf_at += 1;
                loop {
                    self.pair(dst, codebook, linbits);
                    pairs_to_decode -= 1;
                    if pairs_to_decode == 0 {
                        break;
                    }
                }
                big_val_cnt -= np;
                if big_val_cnt <= 0 {
                    break;
                }
                sfb_cnt -= 1;
                if sfb_cnt < 0 {
                    break;
                }
            }
        }
        big_val_cnt
    }

    /// One pair of big values: a codeword, then each value's extra bits
    /// (`linbits` of them past 15) and sign.
    fn pair(&mut self, dst: &mut [f32], codebook: &[i16], linbits: u32) {
        let mut w = 5u32;
        let mut leaf = i32::from(codebook[self.c.peek(w) as usize]);
        while leaf < 0 {
            self.c.flush(w);
            w = (leaf & 7) as u32;
            leaf = i32::from(codebook[(self.c.peek(w) as i32 - (leaf >> 3)) as usize]);
        }
        self.c.flush((leaf >> 8) as u32);

        for _ in 0..2 {
            let mut lsb = leaf & 0x0F;
            if linbits != 0 && lsb == 15 {
                lsb += self.c.peek(linbits) as i32;
                self.c.flush(linbits);
                self.c.check();
                dst[self.d] = self.one * pow_43(lsb) * if self.c.negative() { -1.0 } else { 1.0 };
            } else {
                dst[self.d] =
                    G_POW43[(16 + lsb - 16 * (self.c.bits >> 31) as i32) as usize] * self.one;
            }
            self.c.flush(u32::from(lsb != 0));
            self.d += 1;
            leaf >>= 4;
        }
        self.c.check();
    }

    /// The count1 region: quadruples of -1, 0 and 1 until the granule's
    /// bits or its bands run out. `np` is minimp3's: one more than the pairs
    /// left in the band the big values ended in.
    fn count1(&mut self, dst: &mut [f32], gr_info: &GrInfo, mut np: i32, layer3gr_limit: usize) {
        let codebook: &[u8] = if gr_info.count1_table != 0 {
            &TAB33
        } else {
            &TAB32
        };
        loop {
            let mut leaf = i32::from(codebook[self.c.peek(4) as usize]);
            if leaf & 8 == 0 {
                let extra = (self.c.bits << 4) >> (32 - (leaf & 3) as u32);
                leaf = i32::from(codebook[(leaf >> 3) as usize + extra as usize]);
            }
            self.c.flush((leaf & 7) as u32);
            if self.c.pos() > layer3gr_limit as i64 {
                return;
            }
            for half in 0..2 {
                // RELOAD_SCALEFACTOR: on into the next band, if there is one.
                np -= 1;
                if np == 0 {
                    np = i32::from(self.sfb[self.sfb_at]) / 2;
                    self.sfb_at += 1;
                    if np == 0 {
                        return;
                    }
                    self.one = self.scf[self.scf_at];
                    self.scf_at += 1;
                }
                // DEQ_COUNT1, for the pair's two values.
                for v in 2 * half..2 * half + 2 {
                    if leaf & (128 >> v) != 0 {
                        dst[self.d + v] = if self.c.negative() {
                            -self.one
                        } else {
                            self.one
                        };
                        self.c.flush(1);
                    }
                }
            }
            self.c.check();
            self.d += 4;
        }
    }
}

/// `L3_huffman`: a granule's spectrum for one channel, the big values and
/// then the quadruples of the count1 region, each times its band's gain.
pub(crate) fn huffman(
    dst: &mut [f32],
    bs: &mut Bits<'_>,
    gr_info: &GrInfo,
    scf: &[f32; 40],
    layer3gr_limit: usize,
) {
    let mut s = Spectrum {
        c: Cache::new(bs.buf, bs.pos),
        sfb: gr_info.sfbtab,
        sfb_at: 0,
        scf,
        scf_at: 0,
        one: 0.0,
        d: 0,
    };
    let big_val_cnt = s.big_values(dst, gr_info);
    s.count1(dst, gr_info, 1 - big_val_cnt, layer3gr_limit);
    bs.pos = layer3gr_limit;
}

/// `L3_midside_stereo`: `n` lines from `left` of mid and side into left and
/// right (the right channel 576 lines on).
fn midside_stereo(grbuf: &mut [f32], left: usize, n: usize) {
    for i in left..left + n {
        let a = grbuf[i];
        let b = grbuf[i + 576];
        grbuf[i] = a + b;
        grbuf[i + 576] = a - b;
    }
}

/// `L3_intensity_stereo_band`.
fn intensity_stereo_band(grbuf: &mut [f32], left: usize, n: usize, kl: f32, kr: f32) {
    for i in left..left + n {
        grbuf[i + 576] = grbuf[i] * kr;
        grbuf[i] *= kl;
    }
}

/// `L3_stereo_top_band`: the last band of each window where the right
/// channel is not silent, or -1.
fn stereo_top_band(right: &[f32], sfb: &[u8], nbands: usize, max_band: &mut [i32; 3]) {
    *max_band = [-1; 3];
    let mut r = 0usize;
    for (i, width) in sfb.iter().take(nbands).enumerate() {
        let width = usize::from(*width);
        let mut k = 0;
        while k < width {
            if right[r + k] != 0.0 || right[r + k + 1] != 0.0 {
                max_band[i % 3] = i as i32;
                break;
            }
            k += 2;
        }
        r += width;
    }
}

/// `L3_stereo_process`: intensity stereo above each window's top band,
/// mid/side (where the frame has it) below it.
fn stereo_process(
    grbuf: &mut [f32],
    ist_pos: &[u8; 39],
    sfb: &[u8],
    hdr: Hdr,
    max_band: &[i32; 3],
    mpeg2_sh: u32,
) {
    let max_pos: u32 = if hdr.test_mpeg1() { 7 } else { 64 };
    let mut left = 0usize;
    let mut i = 0usize;
    while sfb[i] != 0 {
        let width = usize::from(sfb[i]);
        let ipos = u32::from(ist_pos[i]);
        if i as i32 > max_band[i % 3] && ipos < max_pos {
            // minimp3's 1.41421356f: the same float.
            let s = if hdr.test_ms_stereo() { SQRT_2 } else { 1.0 };
            let (kl, kr);
            if hdr.test_mpeg1() {
                kl = G_PAN[2 * ipos as usize];
                kr = G_PAN[2 * ipos as usize + 1];
            } else {
                let r = ldexp_q2(1.0, ((ipos + 1) >> 1 << mpeg2_sh) as i32);
                if ipos & 1 != 0 {
                    kl = r;
                    kr = 1.0;
                } else {
                    kl = 1.0;
                    kr = r;
                }
            }
            intensity_stereo_band(grbuf, left, width, kl * s, kr * s);
        } else if hdr.test_ms_stereo() {
            midside_stereo(grbuf, left, width);
        }
        left += width;
        i += 1;
    }
}

/// `L3_intensity_stereo`: `gr` is the granule's two channels' side
/// information, `ist_pos` the second channel's positions.
fn intensity_stereo(grbuf: &mut [f32], ist_pos: &mut [u8; 39], gr: &[GrInfo], hdr: Hdr) {
    let g = &gr[0];
    let n_sfb = usize::from(g.n_long_sfb + g.n_short_sfb);
    let max_blocks = if g.n_short_sfb != 0 { 3 } else { 1 };
    let mut max_band = [-1i32; 3];
    stereo_top_band(&grbuf[576..], g.sfbtab, n_sfb, &mut max_band);
    if g.n_long_sfb != 0 {
        let top = max_band[0].max(max_band[1]).max(max_band[2]);
        max_band = [top; 3];
    }
    for (i, top) in max_band.iter().enumerate().take(max_blocks) {
        let default_pos = if hdr.test_mpeg1() { 3 } else { 0 };
        let itop = n_sfb - max_blocks + i;
        let prev = itop - max_blocks;
        ist_pos[itop] = if *top >= prev as i32 {
            default_pos
        } else {
            ist_pos[prev]
        };
    }
    stereo_process(
        grbuf,
        ist_pos,
        g.sfbtab,
        hdr,
        &max_band,
        u32::from(gr[1].scalefac_compress & 1),
    );
}

/// `L3_reorder`: a short block's lines from band-major to window-major,
/// through `scratch`.
fn reorder(grbuf: &mut [f32], scratch: &mut [f32], sfb: &[u8]) {
    let mut src = 0usize;
    let mut dst = 0usize;
    let mut s = 0usize;
    loop {
        let len = usize::from(sfb[s]);
        if len == 0 {
            break;
        }
        for _ in 0..len {
            scratch[dst] = grbuf[src];
            scratch[dst + 1] = grbuf[src + len];
            scratch[dst + 2] = grbuf[src + 2 * len];
            dst += 3;
            src += 1;
        }
        src += 2 * len;
        s += 3;
    }
    grbuf[..dst].copy_from_slice(&scratch[..dst]);
}

/// `L3_antialias`: the butterflies between `nbands` + 1 subbands.
fn antialias(grbuf: &mut [f32], mut nbands: i32) {
    let mut g = 0usize;
    while nbands > 0 {
        for i in 0..8 {
            let u = grbuf[g + 18 + i];
            let d = grbuf[g + 17 - i];
            grbuf[g + 18 + i] = u * G_AA[i] - d * G_AA[8 + i];
            grbuf[g + 17 - i] = u * G_AA[8 + i] + d * G_AA[i];
        }
        nbands -= 1;
        g += 18;
    }
}

/// `L3_dct3_9`.
fn dct3_9(y: &mut [f32; 9]) {
    let mut s0 = y[0];
    let mut s2 = y[2];
    let mut s4 = y[4];
    let mut s6 = y[6];
    let mut s8 = y[8];
    let mut t0 = s0 + s6 * 0.5;
    s0 -= s6;
    let mut t4 = (s4 + s2) * 0.93969262;
    let mut t2 = (s8 + s2) * 0.76604444;
    s6 = (s4 - s8) * 0.17364818;
    s4 += s8 - s2;

    s2 = s0 - s4 * 0.5;
    y[4] = s4 + s0;
    s8 = t0 - t2 + s6;
    s0 = t0 - t4 + t2;
    s4 = t0 + t4 - s6;

    let mut s1 = y[1];
    let mut s3 = y[3];
    let mut s5 = y[5];
    let mut s7 = y[7];

    s3 *= 0.86602540;
    t0 = (s5 + s1) * 0.98480775;
    t4 = (s5 - s7) * 0.34202014;
    t2 = (s1 + s7) * 0.64278761;
    s1 = (s1 - s5 - s7) * 0.86602540;

    s5 = t0 - s3 - t2;
    s7 = t4 - s3 - t0;
    s3 = t4 + s3 - t2;

    y[0] = s4 - s7;
    y[1] = s2 + s1;
    y[2] = s0 - s3;
    y[3] = s8 + s5;
    y[5] = s8 - s5;
    y[6] = s0 + s3;
    y[7] = s2 - s1;
    y[8] = s4 + s7;
}

/// `L3_imdct36`: `nbands` long blocks, overlapped with the last granule's.
fn imdct36(grbuf: &mut [f32], overlap: &mut [f32], window: &[f32], nbands: usize) {
    for j in 0..nbands {
        let g = &mut grbuf[j * 18..j * 18 + 18];
        let ov = &mut overlap[j * 9..j * 9 + 9];
        let mut co = [0.0f32; 9];
        let mut si = [0.0f32; 9];
        co[0] = -g[0];
        si[0] = g[17];
        for i in 0..4 {
            si[8 - 2 * i] = g[4 * i + 1] - g[4 * i + 2];
            co[1 + 2 * i] = g[4 * i + 1] + g[4 * i + 2];
            si[7 - 2 * i] = g[4 * i + 4] - g[4 * i + 3];
            co[2 + 2 * i] = -(g[4 * i + 3] + g[4 * i + 4]);
        }
        dct3_9(&mut co);
        dct3_9(&mut si);

        si[1] = -si[1];
        si[3] = -si[3];
        si[5] = -si[5];
        si[7] = -si[7];

        for i in 0..9 {
            let ovl = ov[i];
            let sum = co[i] * G_TWID9[9 + i] + si[i] * G_TWID9[i];
            ov[i] = co[i] * G_TWID9[i] - si[i] * G_TWID9[9 + i];
            g[i] = ovl * window[i] - sum * window[9 + i];
            g[17 - i] = ovl * window[9 + i] + sum * window[i];
        }
    }
}

/// `L3_idct3`.
fn idct3(x0: f32, x1: f32, x2: f32) -> [f32; 3] {
    let m1 = x1 * 0.86602540;
    let a1 = x0 - x2 * 0.5;
    [a1 + m1, x0 + x2, a1 - m1]
}

/// `L3_imdct12`: one short window from every third line of `x`, into
/// `dst`'s six, overlapped through `overlap`'s three.
fn imdct12(x: &[f32], dst: &mut [f32], overlap: &mut [f32]) {
    let co = idct3(-x[0], x[6] + x[3], x[12] + x[9]);
    let mut si = idct3(x[15], x[12] - x[9], x[6] - x[3]);
    si[1] = -si[1];

    for i in 0..3 {
        let ovl = overlap[i];
        let sum = co[i] * G_TWID3[3 + i] + si[i] * G_TWID3[i];
        overlap[i] = co[i] * G_TWID3[i] - si[i] * G_TWID3[3 + i];
        dst[i] = ovl * G_TWID3[2 - i] - sum * G_TWID3[5 - i];
        dst[5 - i] = ovl * G_TWID3[5 - i] + sum * G_TWID3[2 - i];
    }
}

/// `L3_imdct_short`: `nbands` short blocks.
fn imdct_short(grbuf: &mut [f32], overlap: &mut [f32], nbands: usize) {
    for j in 0..nbands {
        let g = &mut grbuf[j * 18..j * 18 + 18];
        let ov = &mut overlap[j * 9..j * 9 + 9];
        let mut tmp = [0.0f32; 18];
        tmp.copy_from_slice(g);
        g[..6].copy_from_slice(&ov[..6]);
        let (head, tail) = ov.split_at_mut(6);
        imdct12(&tmp, &mut g[6..12], tail);
        imdct12(&tmp[1..], &mut g[12..18], tail);
        imdct12(&tmp[2..], head, tail);
    }
}

/// `L3_change_sign`: every other line of every other subband negated.
fn change_sign(grbuf: &mut [f32]) {
    let mut g = 18usize;
    for _ in (0..32).step_by(2) {
        for i in (1..18).step_by(2) {
            grbuf[g + i] = -grbuf[g + i];
        }
        g += 36;
    }
}

/// `L3_imdct_gr`: a granule's 32 subbands, `n_long_bands` of them long
/// blocks whatever the block type.
fn imdct_gr(grbuf: &mut [f32], overlap: &mut [f32], block_type: u8, n_long_bands: usize) {
    let mut g = 0usize;
    let mut o = 0usize;
    if n_long_bands != 0 {
        imdct36(grbuf, overlap, &G_MDCT_WINDOW[..18], n_long_bands);
        g = 18 * n_long_bands;
        o = 9 * n_long_bands;
    }
    if block_type == SHORT_BLOCK_TYPE {
        imdct_short(&mut grbuf[g..], &mut overlap[o..], 32 - n_long_bands);
    } else {
        let w = usize::from(block_type == STOP_BLOCK_TYPE) * 18;
        imdct36(
            &mut grbuf[g..],
            &mut overlap[o..],
            &G_MDCT_WINDOW[w..w + 18],
            32 - n_long_bands,
        );
    }
}

/// The scratch memory of one granule's decoding: minimp3's
/// `mp3dec_scratch_t` without its bit reader and main data.
pub(crate) struct Granule<'a> {
    pub gr_info: &'a [GrInfo],
    pub grbuf: &'a mut [f32; 1152],
    pub scf: &'a mut [f32; 40],
    pub syn: &'a mut [f32],
    pub ist_pos: &'a mut [[u8; 39]; 2],
}

/// `L3_decode`: one granule of `nch` channels from the main data in `bs`,
/// into `grbuf` as 18 time slots of 32 subbands a channel.
pub(crate) fn decode(
    hdr: Hdr,
    mdct_overlap: &mut [[f32; 288]; 2],
    bs: &mut Bits<'_>,
    s: &mut Granule<'_>,
    nch: usize,
) {
    for ch in 0..nch {
        let gr = &s.gr_info[ch];
        let layer3gr_limit = bs.pos + usize::from(gr.part_23_length);
        decode_scalefactors(hdr, &mut s.ist_pos[ch], bs, gr, s.scf, ch);
        huffman(
            &mut s.grbuf[ch * 576..ch * 576 + 576],
            bs,
            gr,
            s.scf,
            layer3gr_limit,
        );
    }

    if hdr.is_i_stereo() {
        intensity_stereo(&mut s.grbuf[..], &mut s.ist_pos[1], s.gr_info, hdr);
    } else if hdr.is_ms_stereo() {
        midside_stereo(&mut s.grbuf[..], 0, 576);
    }

    for (ch, overlap) in mdct_overlap.iter_mut().enumerate().take(nch) {
        let gr = &s.gr_info[ch];
        let mut aa_bands = 31i32;
        let n_long_bands = (if gr.mixed_block_flag != 0 { 2usize } else { 0 })
            << u32::from(hdr.my_sample_rate() == 2);
        let grbuf = &mut s.grbuf[ch * 576..ch * 576 + 576];
        if gr.n_short_sfb != 0 {
            aa_bands = n_long_bands as i32 - 1;
            reorder(
                &mut grbuf[n_long_bands * 18..],
                s.syn,
                &gr.sfbtab[usize::from(gr.n_long_sfb)..],
            );
        }
        antialias(grbuf, aa_bands);
        imdct_gr(grbuf, overlap, gr.block_type, n_long_bands);
        change_sign(grbuf);
    }
}
