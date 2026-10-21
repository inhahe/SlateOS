//! H.264 video -- AVC, MPEG-4 Part 10, the video of most MP4 files, of
//! phones, cameras and much of the web -- decoded as FFmpeg's decoder
//! decodes it: FFmpeg 8.1.3's `libavcodec` H.264 decoder translated into
//! Rust, module by module (the operator's choice, design-decisions/1376,
//! F-Q8), and held to FFmpeg picture for picture -- the conformance
//! streams' frame checksums FFmpeg's own tests keep, and FFmpeg's output on
//! files of every kind.
//!
//! # Where each part of FFmpeg lives
//!
//! - `get_bits.h`, `golomb.h` and `vlc.c` -- a NAL unit's bits, the
//!   Exp-Golomb codes most syntax is written in, and the variable-length
//!   codes of CAVLC -- are the `ffbits` crate, shared with the AAC decoder.
//! - `nal`: `h2645_parse.c` -- a packet split into NAL units, after start
//!   codes or after lengths, and unescaped.
//! - `ps`: `h264_ps.c` and `h2645_vui.c` -- sequence and picture parameter
//!   sets; `tables` the data tables they and the rest read.
//! - `sei`: `h264_sei.c` and `h2645_sei.c` -- supplemental enhancement
//!   information: recovery points, the x264 version, picture timing,
//!   captions, film grain, and what is about the picture rather than in it.
//! - `slice`: `h264_slice_header_parse` in `h264_slice.c`, with the
//!   reference counts and weights of `h264_parse.c` and the list
//!   modifications and marking of `h264_refs.c` -- a slice's header.
//! - `poc`: `ff_h264_init_poc` in `h264_parse.c` -- where each picture
//!   falls in display order.
//! - `picture`: `h264_picture.c` and the `H264Picture` of `h264dec.h` --
//!   the picture slots, their samples and per-macroblock tables, and what
//!   a decoded frame carries out.
//! - `pool`: `init_table_pools` and the RefStruct pools under it -- the
//!   per-macroblock tables reused as FFmpeg reuses them, contents and all,
//!   which a picture with lost macroblocks reads.
//! - `decoder`: the `H264Context` and `H264SliceContext` of `h264dec.h`,
//!   with the parts of `h264_slice.c` and `h264dec.c` that set a stream
//!   up -- its format, its tables, a picture slot for each picture.
//! - `refs`: `h264_refs.c` -- the reference lists each slice predicts
//!   from, their modification, and the marking of references after each
//!   picture.
//! - `field`: `h264_field_start`, `h264_select_output_frame` and
//!   `ff_h264_field_end` in `h264_slice.c` and `h264_picture.c` -- a
//!   picture begun (gaps in frame numbers, field pairs, recovery) and the
//!   order pictures are shown in.
//! - `queue`: `h264_slice_init`, `ff_h264_queue_decode_slice` and
//!   `ff_h264_execute_decode_slices` in `h264_slice.c` -- each slice made
//!   ready and decoded.
//! - `direct`: `h264_direct.c` -- direct prediction's scale factors, the
//!   map from the co-located picture's references, and a direct
//!   macroblock's vectors, spatial or temporal.
//! - `slicedata`: `decode_slice` and `loop_filter` in `h264_slice.c` -- a
//!   slice's macroblocks decoded in turn and filtered a row behind.
//! - `cavlc`: `h264_cavlc.c` -- a macroblock's syntax coded with CAVLC:
//!   its type, prediction modes, references, vectors and residual.
//! - `cabac`: `cabac.c` and `cabac_functions.h` -- the arithmetic decoder
//!   CABAC slices are read with, as FFmpeg's C build has it.
//! - `cabacmb`: `h264_cabac.c` -- a macroblock's syntax coded with CABAC,
//!   from its context states.
//! - `mbtype`: the macroblock and sub-macroblock types of `h264dec.h`,
//!   `h264data.c` and `mpegutils.h`, and `scan8`.
//! - `mvpred`: `h264_mvpred.h` -- what a macroblock's neighbours hold,
//!   gathered for it, and its vectors predicted from them.
//! - `scan`: `init_scan_tables` in `h264_slice.c` -- the zigzag and field
//!   scans, and their forms for lossless coding.
//! - `mb`: `h264_mb.c` and `h264_mb_template.c` -- a macroblock made
//!   samples: predicted, its residual added, its borders kept for the
//!   macroblocks below.
//! - `pred`: `h264pred.c` and its template -- intra prediction.
//! - `idct`: `h264idct_template.c` -- the inverse transforms.
//! - `mc`: `mc_dir_part`, `mc_part` and `hl_motion` in `h264_mb.c` --
//!   inter prediction, with `h264qpel`, `h264chroma` and the weighting of
//!   `h264dsp`.
//! - `loopfilter`: `h264_loopfilter.c` and the deblocking of
//!   `h264dsp_template.c`.
//! - `er`: `error_resilience.c` and `h264_er_decode_mb` -- the macroblocks
//!   of a frame that no slice decoded, or that a slice failed in, guessed
//!   from around them.
//! - `h274`: `h274.c` -- the film grain a film grain characteristics SEI
//!   describes, synthesized onto a finished picture as it is shown.
//! - `output`: `h264dec.c`'s packet loop -- extradata, NAL units handed to
//!   their parts, pictures finished and handed out, the last ones drained.
//! - `side_data`: what FFmpeg attaches to a frame from SEI -- stereo
//!   packing, rotation, captions, HDR metadata and the rest.

#![forbid(unsafe_code)]

mod cabac;
mod cabacmb;
mod cavlc;
mod decoder;
mod direct;
mod er;
mod field;
mod h274;
mod idct;
mod loopfilter;
mod mb;
mod mbtype;
mod mc;
mod mvpred;
mod nal;
mod output;
mod picture;
mod poc;
mod pool;
mod pred;
#[cfg(test)]
mod probe;
mod ps;
mod queue;
mod refs;
mod scan;
mod sei;
mod side_data;
mod slice;
mod slicedata;
mod tables;
#[cfg(test)]
mod writer;

pub use picture::{Pixel, PixelFormat};
pub use side_data::{MasteringDisplay, Rational, SideData, Stereo3d, Stereo3dType, Stereo3dView};

use decoder::{Context, SliceContext};
use nal::Splitter;
use output::{Output, Packet};
use picture::FrameBuf;

/// An H.264 decoder: a stream's packets in, in the order the file holds
/// them, its pictures out in the order they are shown.
///
/// The pictures are FFmpeg's: decoded as its decoder decodes them, held
/// back as it holds them to put them in display order, and handed out with
/// what its frames carry.
pub struct Decoder {
    h: Box<Context>,
    sl: Box<SliceContext>,
    pkt: Splitter,
}

impl Default for Decoder {
    fn default() -> Self {
        Self::new()
    }
}

impl Decoder {
    /// A decoder for a stream not yet seen.
    #[must_use]
    pub fn new() -> Self {
        Self {
            h: Box::default(),
            sl: Box::default(),
            pkt: Splitter::default(),
        }
    }

    /// The stream's configuration from its container: an MP4 or Matroska
    /// `avcC` record, or parameter sets as Annex B units (FFmpeg's
    /// `extradata`). A configuration that cannot be read is passed over,
    /// as FFmpeg passes it: the stream may carry its parameter sets
    /// itself.
    pub fn configure(&mut self, config: &[u8]) {
        // h264_decode_init: an error reading the extradata is only a
        // warning unless errors explode.
        let _ = self.h.decode_extradata(config);
        if let Some(sps) = self.h.ps.sps.clone() {
            if sps.bitstream_restriction_flag != 0
                && self.h.avctx.has_b_frames < sps.num_reorder_frames
            {
                self.h.avctx.has_b_frames = sps.num_reorder_frames;
            }
        }
        self.h.flush_change();
    }

    /// A new configuration the container gives with the next packet -- an
    /// MP4 whose sample description changes partway, say (FFmpeg's
    /// `AV_PKT_DATA_NEW_EXTRADATA`): its parameter sets taken up, as
    /// `h264_decode_frame` takes them up before the packet's own units. Call
    /// it just before [`decode`](Self::decode)s that packet. Unlike
    /// [`configure`](Self::configure), nothing is flushed; one that cannot
    /// be read is passed over, as FFmpeg passes it over.
    pub fn new_extradata(&mut self, config: &[u8]) {
        // h264_decode_frame does not look at how reading it went.
        let _ = self.h.decode_extradata(config);
    }

    /// One packet decoded: an access unit -- an MP4 sample, a Matroska
    /// block -- or, for a raw stream, whatever it was split into, with the
    /// time and duration its pictures are to carry. Returns the picture it
    /// lets out, if any: the next in display order, once as many are held
    /// as the stream reorders by.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidData`] for a packet FFmpeg refuses, or one with no
    /// picture in it (FFmpeg's "no frame!"); [`Error::TooLarge`] where that
    /// is for a picture larger than [`set_max_pixels`](Self::set_max_pixels)
    /// allows, or a packet larger than FFmpeg's sizes hold. The decoder goes
    /// on with the next packet either way, as FFmpeg's does.
    pub fn decode(
        &mut self,
        data: &[u8],
        time: i64,
        duration: u64,
    ) -> Result<Option<Picture>, Error> {
        self.decode_packet(data, time, duration, false)
    }

    /// [`decode`](Self::decode) for a packet decoded only for those after
    /// it -- one an MP4 edit list leaves out (FFmpeg's
    /// `AV_PKT_FLAG_DISCARD`): the picture it begins is dropped when its
    /// turn to be shown comes, whichever packet's decoding that is.
    ///
    /// # Errors
    ///
    /// As [`decode`](Self::decode).
    pub fn decode_discarded(
        &mut self,
        data: &[u8],
        time: i64,
        duration: u64,
    ) -> Result<Option<Picture>, Error> {
        self.decode_packet(data, time, duration, true)
    }

    fn decode_packet(
        &mut self,
        data: &[u8],
        time: i64,
        duration: u64,
        discard: bool,
    ) -> Result<Option<Picture>, Error> {
        if data.is_empty() {
            return Ok(None);
        }
        let packet = Packet {
            data,
            time,
            duration,
            discard,
        };
        self.h.refusal = None;
        match self.h.decode_frame(&mut self.sl, &mut self.pkt, &packet) {
            // decode.c drops a frame marked to be discarded as it leaves
            // the decoder.
            Ok(o) => Ok(o.filter(|o| !o.props.discard).map(Picture::new)),
            Err(_) => Err(self.h.refusal.take().unwrap_or(Error::InvalidData)),
        }
    }

    /// How many pictures to hold back to put them in display order, from
    /// the start (FFmpeg's `has_b_frames` as its caller sets it, from the
    /// container's `video_delay` or from probing the stream), at most 16 --
    /// as many as the decoder can hold. A stream whose parameter sets do not
    /// say how far it reorders otherwise shows its first pictures in
    /// decoding order until the decoder sees it needs the delay, and drops
    /// what it can then no longer show in order. The decoder raises it as
    /// the stream asks; it is never lowered.
    pub fn set_reorder_delay(&mut self, frames: u32) {
        // FFmpeg's demuxers estimate no more, and its decoder asserts that
        // no more pictures than this wait to be shown.
        #[allow(
            clippy::cast_possible_wrap,
            clippy::cast_possible_truncation,
            reason = "at most 16"
        )]
        let frames = frames.min(decoder::MAX_DPB_FRAMES as u32) as i32;
        self.h.avctx.has_b_frames = frames;
    }

    /// Refuse pictures of more than `max_pixels` luma samples, as coded
    /// (before cropping): the parameter sets that ask for them are refused
    /// before anything is allocated for the size, and their packets fail
    /// with [`Error::TooLarge`] (FFmpeg's `max_pixels`, which it checks as
    /// each frame's buffers are got). A new decoder takes any size FFmpeg's
    /// arithmetic can hold. Set it before the first packet.
    pub fn set_max_pixels(&mut self, max_pixels: u64) {
        self.h.max_pixels = max_pixels;
    }

    /// The picture size the caller has for the stream -- its container's,
    /// or what probing the stream found -- before the first packet
    /// (FFmpeg's `width` and `height` as the decoder is opened with them:
    /// `h264_decode_init`'s `width_from_caller`). Where the parameter sets
    /// crop nothing off the top or left, and this size is no larger than
    /// theirs and rounds up to the same macroblocks, pictures are cropped
    /// to it: a stream coded 1920x1088 in an MP4 that says 1920x1080 is
    /// shown 1080 high. Once the stream's size stops fitting it, it is
    /// forgotten for good, as FFmpeg forgets it.
    pub fn set_container_size(&mut self, width: u32, height: u32) {
        self.h.width_from_caller = i32::try_from(width).unwrap_or(0);
        self.h.height_from_caller = i32::try_from(height).unwrap_or(0);
    }

    /// At the end of the stream, the next picture still held back: call
    /// until it gives none. Pictures of packets marked to be discarded are
    /// passed over.
    pub fn drain(&mut self) -> Option<Picture> {
        loop {
            let out = self.h.send_next_delayed_frame()?;
            if !out.props.discard {
                return Some(Picture::new(out));
            }
        }
    }

    /// Every picture forgotten, as after a seek (`h264_decode_flush`):
    /// decoding starts again at the next IDR or recovery point.
    pub fn flush(&mut self) {
        self.h.flush();
    }
}

/// A picture the decoder showed: its samples, cropped as the stream says,
/// and what came with them.
#[derive(Clone, Debug)]
pub struct Picture {
    out: Output,
}

/// One plane of a [`Picture`]: `height` rows of `width` samples, `stride`
/// apart, starting at `data[0]`. Rows are longer than `width`.
#[derive(Clone, Copy, Debug)]
pub struct PlaneView<'a, P> {
    pub data: &'a [P],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

impl Picture {
    fn new(out: Output) -> Self {
        Self { out }
    }

    /// The picture's size after cropping, in luma samples.
    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        let (w, h) = self.out.frame.size();
        let [l, r, t, b] = self.out.props.crop;
        #[allow(clippy::cast_possible_truncation, reason = "coded sizes, under 2^16")]
        let (w, h) = (w as u32, h as u32);
        (
            w.saturating_sub(l.saturating_add(r)),
            h.saturating_sub(t.saturating_add(b)),
        )
    }

    /// How its samples are laid out.
    #[must_use]
    pub fn format(&self) -> PixelFormat {
        self.out.props.format
    }

    /// Plane `i` (0 Y, 1 Cb, 2 Cr; for GBR, 0 G, 1 B, 2 R) of an 8-bit
    /// picture, cropped.
    #[must_use]
    pub fn plane8(&self, i: usize) -> Option<PlaneView<'_, u8>> {
        u8::from_any(&self.out.frame).and_then(|f| self.view(f, i))
    }

    /// Plane `i` of a 9- to 14-bit picture, cropped.
    #[must_use]
    pub fn plane16(&self, i: usize) -> Option<PlaneView<'_, u16>> {
        u16::from_any(&self.out.frame).and_then(|f| self.view(f, i))
    }

    fn view<'a, P: Pixel>(&self, f: &'a FrameBuf<P>, i: usize) -> Option<PlaneView<'a, P>> {
        let plane = f.planes.get(i)?;
        let [l, r, t, b] = self.out.props.crop.map(|c| c as usize);
        // A chroma plane's crop is the luma's, scaled as the plane is.
        let (luma_w, luma_h) = (f.planes[0].width, f.planes[0].height);
        let (sx, sy) = (
            usize::from(plane.width < luma_w),
            usize::from(plane.height < luma_h),
        );
        let (l, r, t, b) = if i == 0 {
            (l, r, t, b)
        } else {
            (l >> sx, r >> sx, t >> sy, b >> sy)
        };
        let width = plane.width.checked_sub(l.checked_add(r)?)?;
        let height = plane.height.checked_sub(t.checked_add(b)?)?;
        let start = t.checked_mul(plane.stride)?.checked_add(l)?;
        Some(PlaneView {
            data: plane.data.get(start..)?,
            stride: plane.stride,
            width,
            height,
        })
    }

    /// The time and duration of the packet that began it.
    #[must_use]
    pub fn time(&self) -> (i64, u64) {
        (self.out.props.time, self.out.props.duration)
    }

    /// Whether it is a key frame: an IDR picture, or one at a recovery
    /// point the stream marks as whole.
    #[must_use]
    pub fn is_key_frame(&self) -> bool {
        self.out.props.key
    }

    /// Whether it was coded as fields, or is to be shown as them, and
    /// which comes first (`AV_FRAME_FLAG_INTERLACED`,
    /// `AV_FRAME_FLAG_TOP_FIELD_FIRST`).
    #[must_use]
    pub fn interlacing(&self) -> (bool, bool) {
        (self.out.props.interlaced, self.out.props.top_field_first)
    }

    /// Extra field periods to show it for (`repeat_pict`): 1 for a
    /// telecined frame's third field, 2 for a frame shown twice, 4 three
    /// times.
    #[must_use]
    pub fn repeat(&self) -> i32 {
        self.out.props.repeat_pict
    }

    /// The width a sample is shown at against its height
    /// (`sample_aspect_ratio`): `(0, 1)` when the stream does not say.
    #[must_use]
    pub fn sample_aspect_ratio(&self) -> Rational {
        self.out.props.sample_aspect_ratio
    }

    /// H.273's colour codes -- primaries, transfer characteristics, matrix
    /// coefficients -- and whether samples use the full range, as the
    /// stream says them (2, "unspecified", where it does not; `None` for
    /// the range).
    #[must_use]
    pub fn colour(&self) -> (i32, i32, i32, Option<bool>) {
        let p = &self.out.props;
        let range = match p.color_range {
            1 => Some(false),
            2 => Some(true),
            _ => None,
        };
        (p.color_primaries, p.color_trc, p.colorspace, range)
    }

    /// Where chroma samples sit against luma's (`AVChromaLocation`): 0
    /// unspecified, 1 left, 2 center, then H.264's other sitings.
    #[must_use]
    pub fn chroma_location(&self) -> i32 {
        self.out.props.chroma_location
    }

    /// What came with it: captions, stereo packing, orientation, HDR
    /// metadata, time codes.
    #[must_use]
    pub fn side_data(&self) -> &[SideData] {
        &self.out.props.side_data
    }

    /// FFmpeg's `stereo_mode` and `timecode` metadata, where there is any.
    #[must_use]
    pub fn metadata(&self) -> (Option<&'static str>, Option<&str>) {
        (self.out.stereo_mode, self.out.props.timecode.as_deref())
    }

    /// Whether it is shown though decoding has not yet recovered
    /// (`AV_FRAME_FLAG_CORRUPT`), and how decoding it went wrong
    /// (`decode_error_flags`).
    #[must_use]
    pub fn damage(&self) -> (bool, i32) {
        (self.out.props.corrupt, self.out.props.decode_error_flags)
    }
}

/// Why a packet or a stream could not be decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The data is not H.264 where FFmpeg needs it to be
    /// (`AVERROR_INVALIDDATA`).
    InvalidData,
    /// A picture larger than [`Decoder::set_max_pixels`] allows, or a size
    /// past what FFmpeg's arithmetic can hold (`AVERROR(ERANGE)`,
    /// `AVERROR(ENOMEM)` for an allocation that large).
    TooLarge,
    /// Valid H.264 that FFmpeg does not decode either -- slice groups, an
    /// 11- or 13-bit depth (`AVERROR_PATCHWELCOME`): slices whose picture
    /// parameter set was refused for one.
    Unsupported,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::InvalidData => "invalid H.264 data",
            Self::TooLarge => "an H.264 size too large to decode",
            Self::Unsupported => "H.264 that FFmpeg does not decode either",
        })
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests;
