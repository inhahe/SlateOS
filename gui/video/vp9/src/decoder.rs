//! The decoder: a stream of packets in, the pictures it shows out.
//!
//! This is libvpx's frame loop -- `vp9_dx_iface.c`'s `decoder_decode`,
//! `vp9_decoder.c`'s `vp9_receive_compressed_data` and `swap_frame_buffers`,
//! and `vp9_decodeframe.c`'s `read_uncompressed_header` and `vp9_decode_frame`
//! -- with the state they keep between frames: the eight reference slots, the
//! four saved probability contexts, the loop filter's and segmentation's
//! persistent settings, and the previous frame's motion vectors and segment
//! map. Block decoding is `block.rs`'s; this decides which frames decode,
//! from what, and what each leaves behind.
//!
//! What it returns is libvpx's: at most one picture per packet, the last
//! frame the packet decodes if that frame is shown. A packet that fails to
//! decode returns its error and shows nothing, and frames after it are
//! refused until a key frame or intra-only frame resets the decoder, as
//! libvpx's `need_resync` does.
//!
//! Translated into Rust from libvpx v1.17.0's `vp9/vp9_dx_iface.c`,
//! `vp9/decoder/vp9_decoder.c`, `vp9/decoder/vp9_decodeframe.c` and
//! `vp9/common/vp9_entropymode.c` (copyright the WebM project authors), used
//! under libvpx's BSD licence and patent grant (`licenses/libvpx-LICENSE`,
//! `licenses/libvpx-PATENTS`).

#![allow(
    clippy::arithmetic_side_effects,
    reason = "arithmetic here is on frame dimensions (at most 65536), mode-info counts derived from them, and byte offsets within one packet"
)]

use std::sync::Arc;

use crate::Error;
use crate::bits::BitReader;
use crate::block::{self, FrameInfo, MvRef, RefInfo};
use crate::boolread::BoolReader;
use crate::common::{
    InterpFilter, MAX_REF_FRAMES, MAX_SEGMENTS, REF_FRAMES, REFS_PER_FRAME, RefFrame,
    ReferenceMode, SINGLE_REFERENCE, SWITCHABLE, TX_MODE_SELECT, TxMode,
};
use crate::frame::{AnyBuffers, AnyFrame, FrameBuf, Pixel};
use crate::header::{self, ColorConfig, LoopFilterParams, Quantization, Segmentation, StreamInfo};
use crate::inter::ScaleFactors;
use crate::loopfilter;
use crate::probs::{self, Counts, FrameContext};

/// libvpx's `KEY_FRAME`; an inter frame is 1.
const KEY_FRAME: u8 = 0;

/// How many probability contexts a stream may save.
const FRAME_CONTEXTS: usize = 4;

/// The largest picture this decoder accepts by default, in luma samples:
/// VP9's level 6.2 (8192 x 4352). libvpx accepts any size; a limit is what
/// keeps a hostile stream from asking for gigabytes. [`Decoder::with_max_pixels`]
/// changes it.
pub const DEFAULT_MAX_PIXELS: u64 = 8192 * 4352;

/// A picture the decoder showed.
#[derive(Clone, Debug)]
pub struct Picture {
    frame: Arc<AnyFrame>,
}

/// One plane of a [`Picture`]: `height` rows of `width` samples, `stride`
/// apart. Rows are longer than `width`; only the first `width` samples of
/// each are the picture.
#[derive(Clone, Copy, Debug)]
pub struct PlaneView<'a, P> {
    pub data: &'a [P],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

impl Picture {
    /// A picture of `frame`, which nothing will write again.
    pub(crate) fn from_frame(frame: Arc<AnyFrame>) -> Self {
        Self { frame }
    }

    /// The picture's width in luma samples.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.frame.width()
    }

    /// The picture's height in luma samples.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.frame.height()
    }

    /// 8, 10 or 12.
    #[must_use]
    pub fn bit_depth(&self) -> u8 {
        self.frame.bit_depth()
    }

    /// Whether chroma is halved horizontally and vertically.
    #[must_use]
    pub fn subsampling(&self) -> (u8, u8) {
        self.frame.subsampling()
    }

    /// The size the stream suggests the picture be displayed at.
    #[must_use]
    pub fn render_size(&self) -> (u32, u32) {
        match &*self.frame {
            AnyFrame::Eight(f) => (f.render_width, f.render_height),
            AnyFrame::High(f) => (f.render_width, f.render_height),
        }
    }

    /// libvpx's `vpx_color_space_t` and whether samples use the full range.
    #[must_use]
    pub fn color(&self) -> (u8, bool) {
        match &*self.frame {
            AnyFrame::Eight(f) => (f.color_space, f.full_range),
            AnyFrame::High(f) => (f.color_space, f.full_range),
        }
    }

    /// Plane `i` (0 Y, 1 U, 2 V) of an 8-bit picture.
    #[must_use]
    pub fn plane8(&self, i: usize) -> Option<PlaneView<'_, u8>> {
        match &*self.frame {
            AnyFrame::Eight(f) => f.planes.get(i).map(view),
            AnyFrame::High(_) => None,
        }
    }

    /// Plane `i` (0 Y, 1 U, 2 V) of a 10- or 12-bit picture.
    #[must_use]
    pub fn plane16(&self, i: usize) -> Option<PlaneView<'_, u16>> {
        match &*self.frame {
            AnyFrame::High(f) => f.planes.get(i).map(view),
            AnyFrame::Eight(_) => None,
        }
    }
}

fn view<P: Pixel>(p: &crate::frame::Plane<P>) -> PlaneView<'_, P> {
    PlaneView {
        data: &p.data,
        stride: p.stride,
        width: p.crop_width,
        height: p.crop_height,
    }
}

/// A reference the current frame predicts from: libvpx's `RefBuffer`.
#[derive(Clone, Debug)]
struct FrameRef {
    frame: Arc<AnyFrame>,
    sf: ScaleFactors,
}

/// The decoder. Feed it packets in stream order with [`Decoder::decode`].
#[derive(Debug)]
pub struct Decoder {
    /// Whether a packet has yet told the decoder the stream's size: libvpx's
    /// `ctx->si.h != 0`. Until one has, only key and intra-only frames are
    /// tried.
    si_known: bool,
    /// libvpx's `ctx->need_resync`: pictures are withheld until a key frame
    /// or intra-only frame decodes.
    iface_need_resync: bool,
    /// libvpx's `pbi->need_resync`: inter frames are refused.
    need_resync: bool,
    max_pixels: u64,
    /// How many threads a frame's tile columns may decode on.
    threads: usize,

    // --- VP9_COMMON: what persists from frame to frame -------------------------
    profile: u8,
    color: ColorConfig,
    width: u32,
    height: u32,
    last_width: u32,
    last_height: u32,
    render_width: u32,
    render_height: u32,
    frame_type: u8,
    last_frame_type: u8,
    intra_only: bool,
    last_intra_only: bool,
    show_frame: bool,
    last_show_frame: bool,
    show_existing_frame: bool,
    error_resilient_mode: bool,
    reset_frame_context: u8,
    refresh_frame_context: bool,
    frame_parallel_decoding_mode: bool,
    frame_context_idx: usize,
    allow_high_precision_mv: bool,
    interp_filter: InterpFilter,
    ref_frame_sign_bias: [bool; MAX_REF_FRAMES],
    refresh_frame_flags: u8,
    frame_refs: [Option<FrameRef>; REFS_PER_FRAME],
    ref_frame_map: [Option<Arc<AnyFrame>>; REF_FRAMES],
    lf: LoopFilterParams,
    seg: Segmentation,
    quant: Quantization,
    log2_tile_cols: u32,
    log2_tile_rows: u32,
    fc: FrameContext,
    frame_contexts: [FrameContext; FRAME_CONTEXTS],
    counts: Counts,
    /// The last frame's blocks, for tests that hold an encoder's to them.
    #[cfg(test)]
    last_mi: Option<block::MiGrid>,
    mi_cols: usize,
    mi_rows: usize,
    /// The motion vectors of the last frame decoded (not shown from a slot):
    /// libvpx's `prev_frame->mvs`.
    prev_mvs: Vec<MvRef>,
    /// Segment maps: libvpx's `seg_map_array`, current and last, swapped
    /// after each frame that has segmentation on.
    seg_maps: [Vec<u8>; 2],
    cur_seg_map: usize,
    /// Every frame this decoder made that may be decoded into again: one
    /// that nothing else holds -- no reference slot, no picture the caller
    /// kept -- is taken for the next frame of its size, rather than a new
    /// one allocated and cleared.
    pool: Vec<Arc<AnyFrame>>,
    /// Buffers the threads borrow, kept likewise.
    scratch: AnyBuffers,
}

/// How many frames nothing holds the pool keeps for later: a frame needs
/// one; a spare covers a packet that shows a frame the caller then keeps.
const POOL_SPARES: usize = 2;

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

/// What a frame header left for the rest of the frame's decoding.
enum Header {
    /// Show the frame in a reference slot.
    ShowExisting(Arc<AnyFrame>),
    /// Decode a frame whose compressed header is this many bytes, after the
    /// uncompressed header's `offset` bytes.
    Frame {
        offset: usize,
        first_partition: usize,
    },
}

impl Decoder {
    /// What the last frame decoded counted, if it counted (it was neither
    /// error resilient nor frame-parallel): for tests that hold an encoder's
    /// counts to the decoder's.
    #[cfg(test)]
    pub(crate) fn last_counts(&self) -> &Counts {
        &self.counts
    }

    /// The last frame's blocks, as decoded.
    #[cfg(test)]
    pub(crate) fn last_mi(&self) -> Option<&block::MiGrid> {
        self.last_mi.as_ref()
    }

    /// A decoder at the start of a stream.
    #[must_use]
    pub fn new() -> Self {
        Self {
            si_known: false,
            iface_need_resync: true,
            need_resync: true,
            max_pixels: DEFAULT_MAX_PIXELS,
            threads: std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get),
            profile: 0,
            color: ColorConfig {
                bit_depth: 8,
                ..ColorConfig::default()
            },
            width: 0,
            height: 0,
            last_width: 0,
            last_height: 0,
            render_width: 0,
            render_height: 0,
            frame_type: KEY_FRAME,
            last_frame_type: KEY_FRAME,
            intra_only: false,
            last_intra_only: false,
            show_frame: false,
            last_show_frame: false,
            show_existing_frame: false,
            error_resilient_mode: false,
            reset_frame_context: 0,
            refresh_frame_context: false,
            frame_parallel_decoding_mode: false,
            frame_context_idx: 0,
            allow_high_precision_mv: false,
            interp_filter: 0,
            ref_frame_sign_bias: [false; 4],
            refresh_frame_flags: 0,
            frame_refs: [None, None, None],
            ref_frame_map: Default::default(),
            lf: LoopFilterParams::default(),
            seg: Segmentation::default(),
            quant: Quantization::default(),
            log2_tile_cols: 0,
            log2_tile_rows: 0,
            fc: FrameContext::uninitialized(),
            frame_contexts: core::array::from_fn(|_| FrameContext::uninitialized()),
            counts: Counts::default(),
            #[cfg(test)]
            last_mi: None,
            mi_cols: 0,
            mi_rows: 0,
            prev_mvs: Vec::new(),
            seg_maps: [Vec::new(), Vec::new()],
            cur_seg_map: 0,
            pool: Vec::new(),
            scratch: AnyBuffers::default(),
        }
    }

    /// A decoder that refuses pictures of more than `max_pixels` luma
    /// samples, rather than [`DEFAULT_MAX_PIXELS`].
    #[must_use]
    pub fn with_max_pixels(max_pixels: u64) -> Self {
        Self {
            max_pixels,
            ..Self::new()
        }
    }

    /// Decode on at most `threads` threads (at least one) from the next
    /// packet on. A new decoder uses as many as the machine has cores.
    ///
    /// A frame's tile columns are what decode in parallel, so a stream
    /// coded with one tile column decodes on one thread whatever this says;
    /// encoders give 1080p video four. The pictures are the same however
    /// many threads make them.
    pub fn set_threads(&mut self, threads: usize) {
        self.threads = threads.max(1);
    }

    /// How many threads the decoder may use.
    #[must_use]
    pub fn threads(&self) -> usize {
        self.threads
    }

    /// Decode one packet -- a frame, or a superframe of several -- and return
    /// the picture it shows, if any: libvpx's `decoder_decode` followed by
    /// `decoder_get_frame`.
    ///
    /// # Errors
    ///
    /// The packet's first frame that fails, as libvpx would report it. The
    /// decoder stays usable: later packets decode once a key frame or
    /// intra-only frame arrives.
    pub fn decode(&mut self, data: &[u8]) -> Result<Option<Picture>, Error> {
        if data.is_empty() {
            return Ok(None);
        }
        let sizes = header::parse_superframe_index(data)?;
        let mut shown = None;
        if sizes.is_empty() {
            // No index: frames back to back, any zero padding between them
            // skipped -- libvpx's "suboptimal termination".
            let mut start = 0usize;
            while let Some(rest) = data.get(start..).filter(|r| !r.is_empty()) {
                let (used, picture) = self.decode_one(rest)?;
                shown = picture;
                start = start.saturating_add(used.max(1));
                while data.get(start) == Some(&0) {
                    start = start.saturating_add(1);
                }
            }
        } else {
            let mut start = 0usize;
            for size in sizes {
                let end = start
                    .checked_add(size as usize)
                    .filter(|&e| e <= data.len())
                    .ok_or(Error::Corrupt(
                        "a superframe index names more data than its packet holds",
                    ))?;
                let frame = data.get(start..end).unwrap_or(&[]);
                let (_, picture) = self.decode_one(frame)?;
                shown = picture;
                start = end;
            }
        }
        Ok(shown)
    }

    /// One frame: libvpx's `decode_one` and `vp9_receive_compressed_data`.
    /// Returns where the frame's data ended and the picture it shows.
    fn decode_one(&mut self, data: &[u8]) -> Result<(usize, Option<Picture>), Error> {
        if !self.si_known {
            let si: StreamInfo = header::peek_stream_info(data)?;
            if si.height != 0 {
                self.si_known = true;
            }
            if !si.is_kf && !si.intra_only {
                return Err(Error::Corrupt("the stream does not start with a key frame"));
            }
        }
        match self.receive(data) {
            Ok((used, shown)) => {
                // libvpx's check_resync.
                if self.iface_need_resync
                    && !self.need_resync
                    && (self.intra_only || self.frame_type == KEY_FRAME)
                {
                    self.iface_need_resync = false;
                }
                let picture = if self.show_frame && !self.iface_need_resync {
                    shown.map(|frame| Picture { frame })
                } else {
                    None
                };
                Ok((used, picture))
            }
            Err(e) => {
                self.need_resync = true;
                self.iface_need_resync = true;
                Err(e)
            }
        }
    }

    /// libvpx's `vp9_receive_compressed_data` and `vp9_decode_frame`.
    fn receive(&mut self, data: &[u8]) -> Result<(usize, Option<Arc<AnyFrame>>), Error> {
        let mut rb = BitReader::new(data);
        let header = self.read_uncompressed_header(&mut rb)?;
        let (offset, first_partition) = match header {
            Header::ShowExisting(frame) => {
                // swap_frame_buffers with nothing refreshed; the picture is
                // the slot's. prev_frame and last_show_frame stay as they
                // were.
                self.last_width = self.width;
                self.last_height = self.height;
                let used = if self.profile <= 2 { 1 } else { 2 };
                return Ok((used, Some(frame)));
            }
            Header::Frame {
                offset,
                first_partition,
            } => (offset, first_partition),
        };
        let compressed = data
            .get(offset..)
            .and_then(|d| d.get(..first_partition))
            .filter(|_| first_partition != 0)
            .ok_or(Error::Corrupt("truncated packet or corrupt header length"))?;
        let tiles = data.get(offset + first_partition..).unwrap_or(&[]);

        let use_prev_frame_mvs = !self.error_resilient_mode
            && self.width == self.last_width
            && self.height == self.last_height
            && !self.last_intra_only
            && self.last_show_frame
            && self.last_frame_type != KEY_FRAME;

        self.fc = self
            .frame_contexts
            .get(self.frame_context_idx)
            .cloned()
            .unwrap_or_else(FrameContext::uninitialized);
        if !self.fc.initialized {
            return Err(Error::Corrupt("uninitialized entropy context"));
        }
        let (tx_mode, reference_mode, comp_fixed_ref, comp_var_ref) =
            self.read_compressed_header(compressed)?;

        let info = FrameInfo {
            mi_cols: self.mi_cols,
            mi_rows: self.mi_rows,
            bit_depth: self.color.bit_depth,
            ss_x: self.color.ss_x,
            ss_y: self.color.ss_y,
            intra_only: self.frame_is_intra_only(),
            lossless: self.quant.lossless(),
            tx_mode,
            interp_filter: self.interp_filter,
            allow_high_precision_mv: self.allow_high_precision_mv,
            reference_mode,
            comp_fixed_ref,
            comp_var_ref,
            ref_frame_sign_bias: self.ref_frame_sign_bias,
            log2_tile_cols: self.log2_tile_cols,
            log2_tile_rows: self.log2_tile_rows,
            seg: self.seg,
            dequant: self.dequant(),
            count: !self.frame_parallel_decoding_mode,
        };

        let mut frame_arc = self.take_frame()?;
        let frame = Arc::get_mut(&mut frame_arc)
            .ok_or(Error::Corrupt("a frame being decoded is held elsewhere"))?;
        let refs: Vec<Option<RefInfo<'_>>> = self
            .frame_refs
            .iter()
            .map(|r| {
                r.as_ref().map(|r| RefInfo {
                    frame: &r.frame,
                    sf: r.sf,
                })
            })
            .collect();
        let seg_maps = &mut self.seg_maps;
        let (cur_map, last_map) = {
            let [a, b] = seg_maps;
            if self.cur_seg_map == 0 {
                (a, &*b)
            } else {
                (b, &*a)
            }
        };
        let mut cur_mvs = vec![MvRef::default(); self.mi_rows * self.mi_cols];
        let mut counts = Counts::default();
        let decoded = block::decode_tiles(
            &info,
            &self.fc,
            &refs,
            if use_prev_frame_mvs {
                Some(self.prev_mvs.as_slice())
            } else {
                None
            },
            last_map,
            cur_map,
            &mut cur_mvs,
            &mut counts,
            tiles,
            frame,
            self.threads,
            &mut self.scratch,
        )?;

        #[cfg(test)]
        {
            self.last_mi = Some(decoded.mi.clone());
        }
        if self.lf.filter_level != 0 {
            loopfilter::filter_frame(
                frame,
                &decoded,
                &self.lf.levels(&self.seg),
                &self.lf.limits(),
                self.threads,
                &mut self.scratch,
            );
        }
        let used = offset + first_partition + decoded.end_of_data;

        let intra_only = self.frame_is_intra_only();
        if !self.error_resilient_mode && !self.frame_parallel_decoding_mode {
            self.counts = counts;
            let pre = self
                .frame_contexts
                .get(self.frame_context_idx)
                .cloned()
                .unwrap_or_else(FrameContext::defaults);
            probs::adapt_coef_probs(
                &mut self.fc,
                &pre,
                &self.counts,
                intra_only,
                self.last_frame_type == KEY_FRAME,
            );
            if !intra_only {
                probs::adapt_mode_probs(
                    &mut self.fc,
                    &pre,
                    &self.counts,
                    self.interp_filter == SWITCHABLE,
                    tx_mode == TX_MODE_SELECT,
                );
                probs::adapt_mv_probs(
                    &mut self.fc,
                    &pre,
                    &self.counts,
                    self.allow_high_precision_mv,
                );
            }
        }
        if self.refresh_frame_context
            && let Some(slot) = self.frame_contexts.get_mut(self.frame_context_idx)
        {
            *slot = self.fc.clone();
        }

        // swap_frame_buffers.
        let frame = frame_arc;
        self.pool.push(Arc::clone(&frame));
        for (i, slot) in self.ref_frame_map.iter_mut().enumerate() {
            if self.refresh_frame_flags & (1 << i) != 0 {
                *slot = Some(Arc::clone(&frame));
            }
        }
        self.frame_refs = [None, None, None];
        self.last_show_frame = self.show_frame;
        self.prev_mvs = cur_mvs;
        if self.seg.enabled {
            self.cur_seg_map ^= 1;
        }
        self.last_width = self.width;
        self.last_height = self.height;
        Ok((used, Some(frame)))
    }

    fn frame_is_intra_only(&self) -> bool {
        self.frame_type == KEY_FRAME || self.intra_only
    }

    /// The dequantisers of each segment: libvpx's
    /// `setup_segmentation_dequant`, as (Y DC, Y AC, UV DC, UV AC).
    fn dequant(&self) -> [[i16; 4]; MAX_SEGMENTS] {
        let bd = self.color.bit_depth;
        let q = &self.quant;
        let one = |qindex: i32| {
            [
                header::dc_quant(qindex, q.y_dc_delta_q, bd),
                header::ac_quant(qindex, 0, bd),
                header::dc_quant(qindex, q.uv_dc_delta_q, bd),
                header::ac_quant(qindex, q.uv_ac_delta_q, bd),
            ]
        };
        if self.seg.enabled {
            core::array::from_fn(|i| one(self.seg.qindex(i as u8, q.base_qindex)))
        } else {
            // Only the first is used.
            [one(q.base_qindex); MAX_SEGMENTS]
        }
    }

    /// A frame to decode into, of the current size and format: one from the
    /// pool that nothing else holds, or a new one. Frames of another size
    /// that nothing holds, and spares past [`POOL_SPARES`], are let go.
    fn take_frame(&mut self) -> Result<Arc<AnyFrame>, Error> {
        let c = self.color;
        let fits = |f: &AnyFrame| {
            let (w, h, ss, bd) = match f {
                AnyFrame::Eight(f) => (f.width, f.height, (f.ss_x, f.ss_y), f.bit_depth),
                AnyFrame::High(f) => (f.width, f.height, (f.ss_x, f.ss_y), f.bit_depth),
            };
            (w, h, ss, bd) == (self.width, self.height, (c.ss_x, c.ss_y), c.bit_depth)
        };
        let free = |f: &Arc<AnyFrame>| Arc::strong_count(f) == 1 && Arc::weak_count(f) == 0;
        let taken = self
            .pool
            .iter()
            .position(|f| free(f) && fits(f))
            .map(|i| self.pool.swap_remove(i));
        let mut spares = 0;
        self.pool.retain(|f| {
            if !free(f) {
                return true;
            }
            if !fits(f) || spares >= POOL_SPARES {
                return false;
            }
            spares += 1;
            true
        });
        let Some(mut frame) = taken else {
            return Ok(Arc::new(self.new_frame()?));
        };
        let f =
            Arc::get_mut(&mut frame).ok_or(Error::Corrupt("a pooled frame is held elsewhere"))?;
        let (rw, rh) = (self.render_width, self.render_height);
        match f {
            AnyFrame::Eight(f) => reuse(f, &c, rw, rh),
            AnyFrame::High(f) => reuse(f, &c, rw, rh),
        }
        Ok(frame)
    }

    /// A new frame to decode into, of the current size and format.
    fn new_frame(&self) -> Result<AnyFrame, Error> {
        let c = &self.color;
        let mut frame = if c.bit_depth == 8 {
            AnyFrame::Eight(FrameBuf::new(self.width, self.height, c.ss_x, c.ss_y, 8)?)
        } else {
            AnyFrame::High(FrameBuf::new(
                self.width,
                self.height,
                c.ss_x,
                c.ss_y,
                c.bit_depth,
            )?)
        };
        let (rw, rh) = (self.render_width, self.render_height);
        match &mut frame {
            AnyFrame::Eight(f) => set_meta(f, c, rw, rh),
            AnyFrame::High(f) => set_meta(f, c, rw, rh),
        }
        Ok(frame)
    }

    // --- The uncompressed header ----------------------------------------------------

    /// libvpx's `read_uncompressed_header`.
    fn read_uncompressed_header(&mut self, rb: &mut BitReader<'_>) -> Result<Header, Error> {
        self.last_frame_type = self.frame_type;
        self.last_intra_only = self.intra_only;

        if !header::frame_marker_ok(rb.literal(2)) {
            return Err(Error::Unsupported("invalid frame marker"));
        }
        self.profile = header::read_profile(rb);
        if self.profile >= header::MAX_PROFILES {
            return Err(Error::Unsupported("unsupported bitstream profile"));
        }

        self.show_existing_frame = rb.flag();
        if self.show_existing_frame {
            let slot = rb.literal(3) as usize;
            let frame = self
                .ref_frame_map
                .get(slot)
                .and_then(Clone::clone)
                .ok_or(Error::Unsupported("a slot holds no decoded frame to show"))?;
            self.refresh_frame_flags = 0;
            self.lf.filter_level = 0;
            self.show_frame = true;
            return Ok(Header::ShowExisting(frame));
        }

        self.frame_type = rb.bit() as u8;
        self.show_frame = rb.flag();
        self.error_resilient_mode = rb.flag();

        if self.frame_type == KEY_FRAME {
            if !header::read_sync_code(rb) {
                return Err(Error::Unsupported("invalid frame sync code"));
            }
            self.color = header::read_color_config(rb, self.profile)?;
            self.refresh_frame_flags = 0xff;
            self.frame_refs = [None, None, None];
            self.setup_frame_size(rb)?;
            if self.need_resync {
                self.ref_frame_map = Default::default();
                self.need_resync = false;
            }
        } else {
            self.intra_only = if self.show_frame { false } else { rb.flag() };
            self.reset_frame_context = if self.error_resilient_mode {
                0
            } else {
                rb.literal(2) as u8
            };
            if self.intra_only {
                if !header::read_sync_code(rb) {
                    return Err(Error::Unsupported("invalid frame sync code"));
                }
                if self.profile > 0 {
                    self.color = header::read_color_config(rb, self.profile)?;
                } else {
                    // Profile 0 intra-only frames are 8-bit 4:2:0 BT.601,
                    // normatively.
                    self.color = ColorConfig {
                        bit_depth: 8,
                        color_space: header::CS_BT_601,
                        full_range: false,
                        ss_x: 1,
                        ss_y: 1,
                    };
                }
                self.refresh_frame_flags = rb.literal(8) as u8;
                self.setup_frame_size(rb)?;
                if self.need_resync {
                    self.ref_frame_map = Default::default();
                    self.need_resync = false;
                }
            } else if !self.need_resync {
                self.refresh_frame_flags = rb.literal(8) as u8;
                for i in 0..REFS_PER_FRAME {
                    let slot = rb.literal(3) as usize;
                    let frame = self
                        .ref_frame_map
                        .get(slot)
                        .and_then(Clone::clone)
                        .ok_or(Error::Corrupt("invalid reference frame index"))?;
                    if let Some(r) = self.frame_refs.get_mut(i) {
                        *r = Some(FrameRef {
                            frame,
                            sf: ScaleFactors::invalid(),
                        });
                    }
                    if let Some(b) = self.ref_frame_sign_bias.get_mut(1 + i) {
                        *b = rb.flag();
                    }
                }
                self.setup_frame_size_with_refs(rb)?;
                self.allow_high_precision_mv = rb.flag();
                self.interp_filter = header::read_interp_filter(rb);
                let (w, h) = (self.width, self.height);
                for r in self.frame_refs.iter_mut().flatten() {
                    r.sf = ScaleFactors::new(r.frame.width(), r.frame.height(), w, h);
                }
            }
        }

        if self.need_resync {
            return Err(Error::Corrupt(
                "a key frame or intra-only frame is needed to reset the decoder",
            ));
        }

        if self.error_resilient_mode {
            self.refresh_frame_context = false;
            self.frame_parallel_decoding_mode = true;
        } else {
            self.refresh_frame_context = rb.flag();
            self.frame_parallel_decoding_mode = rb.flag();
        }
        // Overridden by setup_past_independence for intra-only and
        // error-resilient frames, which always use context 0.
        self.frame_context_idx = rb.literal(2) as usize;

        if self.frame_is_intra_only() || self.error_resilient_mode {
            self.setup_past_independence();
        }

        self.lf.read(rb);
        self.quant = Quantization::read(rb);
        self.seg.read(rb);
        let (cols, rows) = header::read_tile_info(rb, self.mi_cols as u32)?;
        self.log2_tile_cols = cols;
        self.log2_tile_rows = rows;
        let first_partition = rb.literal(16) as usize;
        if rb.overran() {
            return Err(Error::Corrupt("truncated packet"));
        }
        if first_partition == 0 {
            return Err(Error::Corrupt("invalid header size"));
        }
        Ok(Header::Frame {
            offset: rb.bytes_read(),
            first_partition,
        })
    }

    /// The frame size, then the render size: libvpx's `setup_frame_size`.
    fn setup_frame_size(&mut self, rb: &mut BitReader<'_>) -> Result<(), Error> {
        let (w, h) = header::read_frame_size(rb);
        self.resize_context_buffers(w, h)?;
        self.setup_render_size(rb);
        Ok(())
    }

    /// The frame size, from a reference or explicit, then the render size:
    /// libvpx's `setup_frame_size_with_refs`.
    fn setup_frame_size_with_refs(&mut self, rb: &mut BitReader<'_>) -> Result<(), Error> {
        let mut size = None;
        for r in &self.frame_refs {
            if rb.flag() {
                size = r.as_ref().map(|r| (r.frame.width(), r.frame.height()));
                break;
            }
        }
        let (w, h) = match size {
            Some(s) => s,
            None => header::read_frame_size(rb),
        };
        if w == 0 || h == 0 {
            return Err(Error::Corrupt("invalid frame size"));
        }
        let refs = self.frame_refs.iter().flatten();
        if !refs
            .clone()
            .any(|r| header::valid_ref_frame_size(r.frame.width(), r.frame.height(), w, h))
        {
            return Err(Error::Corrupt("no reference frame has a usable size"));
        }
        let c = &self.color;
        if refs.clone().any(|r| {
            r.frame.bit_depth() != c.bit_depth || r.frame.subsampling() != (c.ss_x, c.ss_y)
        }) {
            return Err(Error::Corrupt(
                "a reference frame has an incompatible colour format",
            ));
        }
        self.resize_context_buffers(w, h)?;
        self.setup_render_size(rb);
        Ok(())
    }

    /// libvpx's `setup_render_size`.
    fn setup_render_size(&mut self, rb: &mut BitReader<'_>) {
        (self.render_width, self.render_height) = (self.width, self.height);
        if rb.flag() {
            (self.render_width, self.render_height) = header::read_frame_size(rb);
        }
    }

    /// libvpx's `resize_context_buffers`: a new size clears the last
    /// segment map, as reallocating it does.
    fn resize_context_buffers(&mut self, w: u32, h: u32) -> Result<(), Error> {
        if u64::from(w) * u64::from(h) > self.max_pixels {
            return Err(Error::Unsupported(
                "the frame is larger than this decoder accepts",
            ));
        }
        if self.width != w || self.height != h {
            self.mi_cols = (w as usize).div_ceil(8);
            self.mi_rows = (h as usize).div_ceil(8);
            let cells = self.mi_cols * self.mi_rows;
            for map in &mut self.seg_maps {
                map.clear();
                map.resize(cells, 0);
            }
            self.width = w;
            self.height = h;
        }
        Ok(())
    }

    /// Forget everything earlier frames set up: libvpx's
    /// `vp9_setup_past_independence`.
    fn setup_past_independence(&mut self) {
        self.seg.clear_all_features();
        self.seg.abs_delta = false;
        for map in &mut self.seg_maps {
            map.fill(0);
        }
        self.lf.set_default_deltas();
        self.fc = FrameContext::defaults();
        if self.frame_type == KEY_FRAME
            || self.error_resilient_mode
            || self.reset_frame_context == 3
        {
            for c in &mut self.frame_contexts {
                *c = self.fc.clone();
            }
        } else if self.reset_frame_context == 2
            && let Some(c) = self.frame_contexts.get_mut(self.frame_context_idx)
        {
            *c = self.fc.clone();
        }
        self.ref_frame_sign_bias = [false; 4];
        self.frame_context_idx = 0;
    }

    // --- The compressed header -----------------------------------------------------

    /// libvpx's `read_compressed_header`: the frame's probability updates,
    /// its transform mode and its reference mode.
    fn read_compressed_header(
        &mut self,
        data: &[u8],
    ) -> Result<(TxMode, ReferenceMode, RefFrame, [RefFrame; 2]), Error> {
        let mut r = BoolReader::new(data)?;
        let fc = &mut self.fc;
        let tx_mode = if self.quant.lossless() {
            crate::common::ONLY_4X4
        } else {
            header::read_tx_mode(&mut r)
        };
        if tx_mode == TX_MODE_SELECT {
            header::read_tx_mode_probs(fc, &mut r);
        }
        header::read_coef_probs(fc, tx_mode, &mut r);
        header::read_skip_probs(fc, &mut r);

        let mut reference_mode = SINGLE_REFERENCE;
        let (mut fixed, mut var) = (0, [0, 0]);
        if !(self.frame_type == KEY_FRAME || self.intra_only) {
            header::read_inter_mode_probs(fc, &mut r);
            if self.interp_filter == SWITCHABLE {
                header::read_switchable_interp_probs(fc, &mut r);
            }
            header::read_intra_inter_probs(fc, &mut r);
            reference_mode = header::read_frame_reference_mode(&self.ref_frame_sign_bias, &mut r);
            if reference_mode != SINGLE_REFERENCE {
                (fixed, var) = header::compound_references(&self.ref_frame_sign_bias);
            }
            header::read_frame_reference_mode_probs(fc, reference_mode, &mut r);
            header::read_y_mode_probs(fc, &mut r);
            header::read_partition_probs(fc, &mut r);
            header::read_mv_probs(fc, self.allow_high_precision_mv, &mut r);
        }
        if r.has_error() {
            return Err(Error::Corrupt("the frame's compressed header is corrupt"));
        }
        Ok((tx_mode, reference_mode, fixed, var))
    }
}

/// Make a pooled frame ready to decode into. Its samples are what the last
/// frame decoded into it left: decoding writes every sample anything reads
/// before reading it, as libvpx's own reuse of its buffers depends on. Debug
/// builds fill it with garbage first, so the conformance tests would see a
/// sample read before it is written.
fn reuse<P: Pixel>(f: &mut FrameBuf<P>, c: &ColorConfig, rw: u32, rh: u32) {
    set_meta(f, c, rw, rh);
    if cfg!(debug_assertions) {
        let garbage = P::from_int(0x5a);
        for plane in &mut f.planes {
            plane.data.fill(garbage);
        }
    }
}

fn set_meta<P: Pixel>(f: &mut FrameBuf<P>, c: &ColorConfig, rw: u32, rh: u32) {
    f.color_space = c.color_space;
    f.full_range = c.full_range;
    f.render_width = rw;
    f.render_height = rh;
}
