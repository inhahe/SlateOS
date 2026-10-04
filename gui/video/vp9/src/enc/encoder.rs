//! The encoder: pictures in, compressed frames out.
//!
//! This is the skeleton libvpx's realtime encoder is being ported onto: a
//! frame goes through libvpx's steps -- its source copied in with the edges
//! repeated (`vp9_copy_and_extend_frame`), its probabilities reset
//! (`vp9_setup_past_independence`), its blocks encoded and reconstructed
//! (`encodeframe`), the reconstruction loop-filtered at the level libvpx's
//! realtime speeds pick from the quantiser (`vp9_pick_filter_level`), and
//! the whole written out (`bitstream`). Every frame is a key frame, coded at
//! a quantiser the caller fixes, with fixed block decisions; libvpx's rate
//! control, mode decisions and inter frames are what come next.
//!
//! What a decoder makes of each frame is exactly [`Encoder::reconstruction`]:
//! `tests/encoder.rs` decodes every frame it writes and compares.
//!
//! Translated in part into Rust from libvpx v1.17.0's
//! `vp9/encoder/vp9_encoder.c`, `vp9_picklpf.c`, `vp9_extend.c` and
//! `vp9/common/vp9_entropymode.c` (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

use std::sync::Arc;

use crate::Error;
use crate::common::ALLOW_32X32;
use crate::decoder::{Picture, PlaneView};
use crate::enc::bitstream::{
    self, CoefUpdates, FrameHeader, KeyFrame, LoopFilterState, UpdateSearch,
};
use crate::enc::encodeframe::{Decide, FixedDecisions, FrameEncoder};
use crate::enc::quantize::{Deltas, Quants};
use crate::enc::tokenize;
use crate::frame::{AnyFrame, Buffers, FrameBuf};
use crate::header::{self, Quantization, Segmentation};
use crate::loopfilter;
use crate::probs::{self, FrameContext};

/// How many probability contexts a stream may save.
const FRAME_CONTEXTS: usize = 4;

/// How a frame is coded, beyond what [`EncoderConfig`] says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrameOptions {
    /// Whether blocks choose their own transform size (`TX_MODE_SELECT`).
    pub tx_select: bool,
    /// libvpx's `frame_parallel_decoding_mode`: no adaptation of the
    /// probabilities from what a frame counted. libvpx's default for VP9.
    pub frame_parallel: bool,
}

impl Default for FrameOptions {
    fn default() -> Self {
        Self {
            tx_select: false,
            frame_parallel: true,
        }
    }
}

/// What to encode and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncoderConfig {
    /// The pictures' size in pixels, 1 to 65536 each way.
    pub width: u32,
    pub height: u32,
    /// The quantiser index every frame is coded at, 0 to 255: 0 is
    /// lossless, larger is smaller and blurrier.
    pub quantizer: u8,
}

/// A VP9 encoder: 8-bit 4:2:0 pictures in, one compressed frame each out.
///
/// ```
/// let (w, h) = (64u32, 48u32);
/// let y = vec![128u8; (w * h) as usize];
/// let uv = vec![128u8; ((w / 2) * (h / 2)) as usize];
/// let mut encoder = vp9::Encoder::new(vp9::EncoderConfig { width: w, height: h, quantizer: 60 })?;
/// let frame = encoder.encode([
///     vp9::PlaneView { data: &y, stride: w as usize, width: w as usize, height: h as usize },
///     vp9::PlaneView { data: &uv, stride: (w / 2) as usize, width: (w / 2) as usize, height: (h / 2) as usize },
///     vp9::PlaneView { data: &uv, stride: (w / 2) as usize, width: (w / 2) as usize, height: (h / 2) as usize },
/// ])?;
/// let mut decoder = vp9::Decoder::new();
/// assert!(decoder.decode(&frame)?.is_some());
/// # Ok::<(), vp9::Error>(())
/// ```
#[derive(Debug)]
pub struct Encoder {
    config: EncoderConfig,
    /// The probabilities each frame starts from, by context index.
    frame_contexts: [FrameContext; FRAME_CONTEXTS],
    lf: LoopFilterState,
    seg: Segmentation,
    quants: Quants,
    /// The last frame's reconstruction: what a decoder shows for it.
    last: Option<Arc<AnyFrame>>,
    /// The loop filter's buffers, kept from frame to frame.
    scratch: Buffers<u8>,
    threads: usize,
    /// What the last frame counted, folded as the decoder counts.
    #[cfg(test)]
    last_counts: crate::probs::Counts,
}

impl Encoder {
    /// An encoder for pictures of `config`'s size.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if a dimension is 0 or above 65536.
    pub fn new(config: EncoderConfig) -> Result<Self, Error> {
        let max = crate::frame::MAX_DIMENSION;
        if config.width == 0 || config.height == 0 || config.width > max || config.height > max {
            return Err(Error::Unsupported("a frame dimension is out of range"));
        }
        Ok(Self {
            config,
            frame_contexts: core::array::from_fn(|_| FrameContext::defaults()),
            lf: LoopFilterState::default(),
            seg: Segmentation::default(),
            quants: Quants::new(Deltas::default(), 0),
            last: None,
            scratch: Buffers::default(),
            threads: std::thread::available_parallelism().map_or(1, usize::from),
            #[cfg(test)]
            last_counts: crate::probs::Counts::default(),
        })
    }

    /// What a decoder shows for the last frame encoded, if any.
    #[must_use]
    pub fn reconstruction(&self) -> Option<Picture> {
        self.last.clone().map(Picture::from_frame)
    }

    /// Encode one picture: its Y, U and V planes, chroma at half the width
    /// and height (rounded up). Returns the compressed frame, which a
    /// [`crate::Decoder`] decodes to [`Encoder::reconstruction`].
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] if a plane is not the configured size, or its
    /// data is too short for its stride.
    pub fn encode(&mut self, planes: [PlaneView<'_, u8>; 3]) -> Result<Vec<u8>, Error> {
        self.encode_frame(planes, &mut FixedDecisions, FrameOptions::default())
    }

    /// [`Encoder::encode`] with the block decisions `d` makes, coded as
    /// `opts` says.
    pub(crate) fn encode_frame(
        &mut self,
        planes: [PlaneView<'_, u8>; 3],
        d: &mut dyn Decide,
        opts: FrameOptions,
    ) -> Result<Vec<u8>, Error> {
        let EncoderConfig {
            width,
            height,
            quantizer,
        } = self.config;
        let src = copy_and_extend(&planes, width, height)?;
        let mut recon = FrameBuf::<u8>::new(width, height, 1, 1, 8)?;

        // A key frame forgets everything earlier frames set up.
        self.setup_past_independence();
        let quant = Quantization {
            base_qindex: i32::from(quantizer),
            ..Quantization::default()
        };
        let mi_cols = (width as usize).div_ceil(8);
        let (min_log2_cols, _) = header::tile_n_bits(u32::try_from(mi_cols).unwrap_or(u32::MAX));
        let h = FrameHeader {
            profile: 0,
            bit_depth: 8,
            color_space: 0,
            full_range: false,
            ss_x: 1,
            ss_y: 1,
            width,
            height,
            render_width: width,
            render_height: height,
            show_frame: true,
            error_resilient_mode: false,
            refresh_frame_context: true,
            frame_parallel_decoding_mode: opts.frame_parallel,
            frame_context_idx: 0,
            quant,
            log2_tile_cols: min_log2_cols,
            log2_tile_rows: 0,
            tx_mode: if quant.lossless() {
                crate::common::ONLY_4X4
            } else if opts.tx_select {
                crate::common::TX_MODE_SELECT
            } else {
                ALLOW_32X32
            },
        };

        let mut fc = FrameContext::defaults();
        let mut fe = FrameEncoder::new(&h, &self.seg, &self.quants, &src, &mut recon);
        fe.encode_tiles(d);
        let (mi, mut counts, tile_tokens) = fe.finish();

        self.lf.params.filter_level = pick_filter_level_from_q(quant.base_qindex, true);
        self.lf.params.sharpness_level = 0;
        if self.lf.params.filter_level != 0 {
            loopfilter::filter_frame_t(
                &mut recon,
                &mi,
                &self.lf.params.levels(&self.seg),
                &self.lf.params.limits(),
                self.threads,
                &mut self.scratch,
            );
        }

        let out = bitstream::pack_key_frame(KeyFrame {
            header: h,
            lf: &mut self.lf,
            seg: &self.seg,
            fc: &mut fc,
            counts: &counts,
            // libvpx's realtime speeds on a key frame.
            search: UpdateSearch {
                coef_updates: CoefUpdates::TwoLoop,
                coef_step: 4,
                tx_8x8_only: false,
            },
            mi: &mi,
            tile_tokens: &tile_tokens,
        });

        // What the frame leaves for the next: adapted probabilities (unless
        // frame-parallel), the saved context, the one-shot update flags
        // cleared.
        tokenize::model_counts(&counts.coef, &mut counts.counts.coef);
        if !h.error_resilient_mode && !h.frame_parallel_decoding_mode {
            let pre = self
                .frame_contexts
                .get(usize::from(h.frame_context_idx))
                .cloned()
                .unwrap_or_else(FrameContext::defaults);
            probs::adapt_coef_probs(&mut fc, &pre, &counts.counts, true, false);
        }
        if h.refresh_frame_context
            && let Some(slot) = self
                .frame_contexts
                .get_mut(usize::from(h.frame_context_idx))
        {
            *slot = fc;
        }
        #[cfg(test)]
        {
            self.last_counts = counts.counts.clone();
        }
        self.lf.params.mode_ref_delta_update = false;
        self.seg.update_map = false;
        self.seg.update_data = false;
        self.last = Some(Arc::new(AnyFrame::Eight(recon)));
        Ok(out)
    }

    /// libvpx's `vp9_setup_past_independence`, as a key frame runs it: the
    /// segmentation's features and the loop filter's deltas back to their
    /// defaults, every saved probability context to the defaults.
    fn setup_past_independence(&mut self) {
        self.seg.clear_all_features();
        self.seg.abs_delta = false;
        self.lf.last_ref_deltas = [0; 4];
        self.lf.last_mode_deltas = [0; 2];
        self.lf.params.set_default_deltas();
        for c in &mut self.frame_contexts {
            *c = FrameContext::defaults();
        }
    }
}

/// The loop filter level libvpx's realtime speeds use: a line fitted to the
/// levels its search finds, from the quantiser's AC step, four lower on key
/// frames. libvpx's `vp9_pick_filter_level` with `LPF_PICK_FROM_Q`, 8-bit,
/// one pass.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the AC step is at most 1828, so the fit is far inside i32, and it is clamped to 0..=63 before narrowing"
)]
pub(crate) fn pick_filter_level_from_q(base_qindex: i32, key_frame: bool) -> u8 {
    let q = i32::from(header::ac_quant(base_qindex, 0, 8));
    let mut guess = (q * 20723 + 1_015_158 + (1 << 17)) >> 18;
    if key_frame {
        guess -= 4;
    }
    guess.clamp(0, header::MAX_LOOP_FILTER) as u8
}

/// The source picture in a frame buffer, its edges repeated out to whole
/// superblocks: libvpx's `vp9_copy_and_extend_frame`, which the encoder's
/// transforms read past the picture's edge through.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "plane sizes are at most 65536 and are checked against the data before any row is read"
)]
fn copy_and_extend(
    planes: &[PlaneView<'_, u8>; 3],
    width: u32,
    height: u32,
) -> Result<FrameBuf<u8>, Error> {
    let mut f = FrameBuf::<u8>::new(width, height, 1, 1, 8)?;
    for (dst, src) in f.planes.iter_mut().zip(planes) {
        let (w, h) = (dst.crop_width, dst.crop_height);
        if src.width != w || src.height != h || src.stride < w {
            return Err(Error::Unsupported("a plane is not the configured size"));
        }
        let need = (h - 1) * src.stride + w;
        if src.data.len() < need {
            return Err(Error::Unsupported(
                "a plane's data is too short for its stride",
            ));
        }
        let stride = dst.stride;
        for y in 0..dst.alloc_height {
            let row = src.data.get(y.min(h - 1) * src.stride..).unwrap_or(&[]);
            let out = dst
                .data
                .get_mut(y * stride..(y + 1) * stride)
                .unwrap_or(&mut []);
            let (inside, past) = out.split_at_mut(w.min(stride));
            for (d, &s) in inside.iter_mut().zip(row) {
                *d = s;
            }
            let last = inside.last().copied().unwrap_or(0);
            past.fill(last);
        }
    }
    Ok(f)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "a test: a failure should be loud"
    )]

    use super::*;
    use crate::common::{BlockSize, Partition};
    use crate::enc::encodeframe::BlockModes;

    struct Lcg(u32);

    impl Lcg {
        fn next(&mut self) -> u32 {
            self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            self.0 >> 8
        }
    }

    /// Decisions drawn at random: every partition, every intra mode, every
    /// transform size, wherever the stream allows them.
    struct RandomDecisions(Lcg);

    impl Decide for RandomDecisions {
        fn partition(
            &mut self,
            _f: &FrameEncoder<'_>,
            _r: usize,
            _c: usize,
            _b: BlockSize,
        ) -> Partition {
            (self.0.next() % 4) as Partition
        }

        fn modes(
            &mut self,
            _f: &FrameEncoder<'_>,
            _r: usize,
            _c: usize,
            bsize: BlockSize,
        ) -> BlockModes {
            let mut m = || (self.0.next() % 10) as u8;
            let modes = BlockModes {
                mode: m(),
                sub_modes: [m(), m(), m(), m()],
                uv_mode: m(),
                tx_size: (self.0.next() % 4) as u8,
            };
            debug_assert!(bsize <= crate::common::BLOCK_64X64);
            modes
        }
    }

    /// A picture with something of everything in it, in six regions: flat
    /// (which codes to nothing and is skipped), smooth gradients, edges,
    /// noise, and all three together.
    fn picture(w: usize, h: usize, seed: u32) -> [Vec<u8>; 3] {
        let mut rng = Lcg(seed);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let mut plane = |pw: usize, ph: usize| {
            let mut v = vec![0u8; pw * ph];
            for y in 0..ph {
                for x in 0..pw {
                    let gradient = (x * 255 / pw.max(1) + y * 97 / ph.max(1)) as u32;
                    let edge = if (x / 7 + y / 5) % 3 == 0 { 60 } else { 0 };
                    let noise = rng.next() % 24;
                    let value = match (x * 3 / pw.max(1), y * 2 / ph.max(1)) {
                        (0, 0) => gradient + edge + noise,
                        (1, 0) => 77,
                        (2, 0) => gradient,
                        (0, _) => 200,
                        (1, _) => 128 + noise,
                        _ => 90 + edge,
                    };
                    v[y * pw + x] = (value % 256) as u8;
                }
            }
            v
        };
        [plane(w, h), plane(cw, ch), plane(cw, ch)]
    }

    fn views(p: &[Vec<u8>; 3], w: usize, h: usize) -> [PlaneView<'_, u8>; 3] {
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        [
            PlaneView {
                data: &p[0],
                stride: w,
                width: w,
                height: h,
            },
            PlaneView {
                data: &p[1],
                stride: cw,
                width: cw,
                height: ch,
            },
            PlaneView {
                data: &p[2],
                stride: cw,
                width: cw,
                height: ch,
            },
        ]
    }

    /// Every plane of two pictures, sample for sample.
    fn same(a: &Picture, b: &Picture) -> bool {
        (0..3).all(|i| {
            let (pa, pb) = (a.plane8(i).unwrap(), b.plane8(i).unwrap());
            pa.width == pb.width
                && pa.height == pb.height
                && (0..pa.height).all(|y| {
                    pa.data[y * pa.stride..][..pa.width] == pb.data[y * pb.stride..][..pb.width]
                })
        })
    }

    /// What the decoder makes of each frame is what the encoder
    /// reconstructed: at every size, quantiser and transform mode, with
    /// partitions, modes and transform sizes drawn at random.
    #[test]
    fn the_decoder_sees_what_the_encoder_reconstructed() {
        let sizes = [
            (64, 64),
            (100, 60),
            (17, 9),
            (8, 8),
            (1, 1),
            (130, 70),
            (72, 200),
        ];
        let mut seed = 1;
        for &(w, h) in &sizes {
            for q in [0u8, 1, 40, 120, 255] {
                for tx_select in [false, true] {
                    seed += 1;
                    // Frame-parallel off on every other frame, so the
                    // decoder counts what it reads.
                    let frame_parallel = seed % 2 == 0;
                    let pic = picture(w, h, seed);
                    let mut enc = Encoder::new(EncoderConfig {
                        width: w as u32,
                        height: h as u32,
                        quantizer: q,
                    })
                    .unwrap();
                    let mut d = RandomDecisions(Lcg(seed * 7919));
                    let opts = FrameOptions {
                        tx_select,
                        frame_parallel,
                    };
                    let frame = enc.encode_frame(views(&pic, w, h), &mut d, opts).unwrap();
                    let mut dec = crate::Decoder::new();
                    let shown = dec
                        .decode(&frame)
                        .unwrap_or_else(|e| panic!("{w}x{h} q{q} select {tx_select}: {e}"))
                        .unwrap();
                    let recon = enc.reconstruction().unwrap();
                    assert!(same(&shown, &recon), "{w}x{h} q{q} select {tx_select}");
                    if !frame_parallel {
                        // What the encoder counted is what the decoder
                        // counted, wherever a key frame counts.
                        let (e, dc) = (&enc.last_counts, dec.last_counts());
                        let at = format!("{w}x{h} q{q} select {tx_select}");
                        assert_eq!(e.coef, dc.coef, "coefficients, {at}");
                        assert_eq!(e.eob_branch, dc.eob_branch, "end of block, {at}");
                        assert_eq!(e.skip, dc.skip, "skip, {at}");
                        assert_eq!(e.tx, dc.tx, "transform size, {at}");
                    }
                    if q == 0 {
                        // Lossless: the source itself.
                        let y = recon.plane8(0).unwrap();
                        for row in 0..h {
                            assert_eq!(y.data[row * y.stride..][..w], pic[0][row * w..][..w]);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_plane_of_the_wrong_size_is_refused() {
        let pic = picture(32, 32, 5);
        let mut enc = Encoder::new(EncoderConfig {
            width: 32,
            height: 30,
            quantizer: 10,
        })
        .unwrap();
        assert!(enc.encode(views(&pic, 32, 32)).is_err());
        assert!(
            Encoder::new(EncoderConfig {
                width: 0,
                height: 30,
                quantizer: 10
            })
            .is_err()
        );
    }

    /// libvpx's fit at its ends: q index 0 (AC step 4) and 255 (1828).
    #[test]
    fn the_filter_level_follows_libvpxs_fit() {
        // (4 * 20723 + 1015158 + 131072) >> 18 = 4.
        assert_eq!(pick_filter_level_from_q(0, false), 4);
        assert_eq!(pick_filter_level_from_q(0, true), 0);
        // (1828 * 20723 + 1015158 + 131072) >> 18 = 148, clamped.
        assert_eq!(pick_filter_level_from_q(255, false), 63);
    }
}
