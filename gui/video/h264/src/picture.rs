//! Pictures: the frames pictures are decoded into, what FFmpeg keeps of
//! each (`H264Picture` in `h264dec.h`, `h264_picture.c`), and references to
//! them (`H264Ref`).
//!
//! FFmpeg keeps 36 pictures (`DPB`) and points at them: the one being
//! decoded, the reference lists, the pictures waiting to be shown. Here a
//! picture is its slot's index, the slots being the DPB's 36 and, after
//! them, the copy FFmpeg keeps to conceal a lost picture with
//! (`last_pic_for_ec`). FFmpeg's second view of the picture being decoded
//! (`cur_pic`) is not kept: it is read only for the samples and the tables,
//! which are the current slot's.
//!
//! A frame's samples are shared, as FFmpeg's reference-counted buffers are,
//! between the slot that decodes into it, the frames handed out, and a
//! frame-number gap's stand-in for a lost reference. Nothing writes a
//! frame anything else holds: a picture already handed out is not decoded
//! into again (`h264_field_start`), and a stand-in is never decoded at all.

#![allow(
    clippy::arithmetic_side_effects,
    reason = "plane sizes come from macroblock counts the SPS bounds; table offsets from mb_stride"
)]

use std::sync::Arc;

use crate::h274::GrainParams;
use crate::ps::Pps;
use crate::side_data::SideData;

/// The pictures FFmpeg keeps (`H264_MAX_PICTURE_COUNT`).
pub(crate) const MAX_PICTURE_COUNT: usize = 36;
/// The slot of the copy kept to conceal a lost picture with
/// (`last_pic_for_ec`).
pub(crate) const LAST_PIC_FOR_EC: usize = MAX_PICTURE_COUNT;
/// Every slot.
pub(crate) const SLOTS: usize = MAX_PICTURE_COUNT + 1;

/// `reference` of a picture held only to be shown (`DELAYED_PIC_REF`).
pub(crate) const DELAYED_PIC_REF: i32 = 1 << 2;

/// A picture after an IDR (`FRAME_RECOVERED_IDR`).
pub(crate) const FRAME_RECOVERED_IDR: i32 = 1 << 0;
/// A picture far enough past a recovery point (`FRAME_RECOVERED_SEI`).
pub(crate) const FRAME_RECOVERED_SEI: i32 = 1 << 1;
/// A random access point no SEI marked (`FRAME_RECOVERED_HEURISTIC`).
pub(crate) const FRAME_RECOVERED_HEURISTIC: i32 = 1 << 2;

/// A sample: `u8` at 8 bits, `u16` at 9 to 14.
pub trait Pixel: Copy + Default + PartialEq + Send + Sync + core::fmt::Debug + 'static {
    /// The sample as an int.
    fn int(self) -> i32;
    /// An int already in the stream's range, as a sample.
    fn from_int(v: i32) -> Self;
    /// The frame, if its samples are of this type.
    fn from_any(f: &AnyFrame) -> Option<&FrameBuf<Self>>;
    /// The same, to write.
    fn from_any_mut(f: &mut AnyFrame) -> Option<&mut FrameBuf<Self>>;
}

impl Pixel for u8 {
    #[inline(always)]
    fn int(self) -> i32 {
        i32::from(self)
    }
    #[inline(always)]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "callers clamp to 0..=255 first, as FFmpeg does before storing a uint8_t"
    )]
    fn from_int(v: i32) -> Self {
        v as u8
    }
    fn from_any(f: &AnyFrame) -> Option<&FrameBuf<Self>> {
        match f {
            AnyFrame::Eight(f) => Some(f),
            AnyFrame::High(_) => None,
        }
    }
    fn from_any_mut(f: &mut AnyFrame) -> Option<&mut FrameBuf<Self>> {
        match f {
            AnyFrame::Eight(f) => Some(f),
            AnyFrame::High(_) => None,
        }
    }
}

impl Pixel for u16 {
    #[inline(always)]
    fn int(self) -> i32 {
        i32::from(self)
    }
    #[inline(always)]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "callers clamp to the bit depth's range first"
    )]
    fn from_int(v: i32) -> Self {
        v as u16
    }
    fn from_any(f: &AnyFrame) -> Option<&FrameBuf<Self>> {
        match f {
            AnyFrame::High(f) => Some(f),
            AnyFrame::Eight(_) => None,
        }
    }
    fn from_any_mut(f: &mut AnyFrame) -> Option<&mut FrameBuf<Self>> {
        match f {
            AnyFrame::High(f) => Some(f),
            AnyFrame::Eight(_) => None,
        }
    }
}

/// One plane of a frame: `height` rows of `width` samples, `stride` apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plane<P> {
    pub(crate) data: Vec<P>,
    pub(crate) stride: usize,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

/// A frame's samples: Y, Cb, Cr -- or G, B, R -- each at the coded size,
/// whole macroblocks, before any cropping. A monochrome stream's frame has
/// chroma planes too, which FFmpeg fills with the middle value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameBuf<P> {
    pub(crate) planes: [Plane<P>; 3],
}

/// A frame at either depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnyFrame {
    Eight(FrameBuf<u8>),
    High(FrameBuf<u16>),
}

impl AnyFrame {
    /// A frame of `width` by `height` luma samples, chroma `chroma_x_shift`
    /// and `chroma_y_shift` times halved, at `bit_depth` bits; every sample
    /// 0. `None` past what can be allocated.
    pub(crate) fn new(
        width: usize,
        height: usize,
        chroma_x_shift: u32,
        chroma_y_shift: u32,
        bit_depth: i32,
    ) -> Option<Self> {
        fn planes<P: Pixel>(w: usize, h: usize, cw: usize, ch: usize) -> Option<FrameBuf<P>> {
            let plane = |w: usize, h: usize| -> Option<Plane<P>> {
                let mut data = Vec::new();
                data.try_reserve_exact(w.checked_mul(h)?).ok()?;
                data.resize(w * h, P::default());
                Some(Plane {
                    data,
                    stride: w,
                    width: w,
                    height: h,
                })
            };
            Some(FrameBuf {
                planes: [plane(w, h)?, plane(cw, ch)?, plane(cw, ch)?],
            })
        }
        // AV_CEIL_RSHIFT: chroma of an odd size rounds up.
        let cw = (width + (1 << chroma_x_shift) - 1) >> chroma_x_shift;
        let ch = (height + (1 << chroma_y_shift) - 1) >> chroma_y_shift;
        if bit_depth > 8 {
            planes(width, height, cw, ch).map(Self::High)
        } else {
            planes(width, height, cw, ch).map(Self::Eight)
        }
    }

    /// Every sample of plane `p` set to `value[p]` (`color_frame`).
    pub(crate) fn fill(&mut self, value: [i32; 3]) {
        fn fill<P: Pixel>(f: &mut FrameBuf<P>, value: [i32; 3]) {
            for (plane, v) in f.planes.iter_mut().zip(value) {
                plane.data.fill(P::from_int(v));
            }
        }
        match self {
            Self::Eight(f) => fill(f, value),
            Self::High(f) => fill(f, value),
        }
    }

    /// Plane `p`'s row stride.
    pub(crate) fn stride(&self, p: usize) -> usize {
        match self {
            Self::Eight(f) => f.planes.get(p).map_or(0, |pl| pl.stride),
            Self::High(f) => f.planes.get(p).map_or(0, |pl| pl.stride),
        }
    }

    /// The luma plane's size.
    pub(crate) fn size(&self) -> (usize, usize) {
        match self {
            Self::Eight(f) => (f.planes[0].width, f.planes[0].height),
            Self::High(f) => (f.planes[0].width, f.planes[0].height),
        }
    }
}

/// A frame's sample layout, as FFmpeg names it (`AVPixelFormat`): the
/// chroma's size against the luma's, and the bits a sample.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PixelFormat {
    /// None yet (`AV_PIX_FMT_NONE`).
    #[default]
    None,
    /// Y, Cb and Cr, the chroma halved across and down: `yuv420p` at 8
    /// bits, `yuv420p10le` at 10 and so on. Monochrome is decoded as this,
    /// its chroma the middle value.
    Yuv420(u8),
    /// The chroma halved across only (`yuv422p`...).
    Yuv422(u8),
    /// The chroma whole (`yuv444p`...).
    Yuv444(u8),
    /// 8-bit 4:2:0 at full range: FFmpeg's deprecated `yuvj420p`, which it
    /// still chooses for such a stream.
    Yuvj420,
    /// `yuvj422p`.
    Yuvj422,
    /// `yuvj444p`.
    Yuvj444,
    /// G, B and R planes, in that order: 4:4:4 whose matrix is the
    /// identity (`gbrp`, `gbrp10le`...).
    Gbr(u8),
}

impl PixelFormat {
    /// The same without the full-range alias (`non_j_pixfmt`).
    pub(crate) fn non_j(self) -> Self {
        match self {
            Self::Yuvj420 => Self::Yuv420(8),
            Self::Yuvj422 => Self::Yuv422(8),
            Self::Yuvj444 => Self::Yuv444(8),
            f => f,
        }
    }
}

/// What FFmpeg keeps on a picture's `AVFrame` besides the samples, as far
/// as the decoding of pictures reads and writes it. What is handed out
/// with each frame is the decoder's to fill in when it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrameProps {
    /// The layout the frame was allocated in (`format`).
    pub(crate) format: PixelFormat,
    /// `AV_FRAME_FLAG_KEY`: an IDR picture.
    pub(crate) key: bool,
    /// `AV_FRAME_FLAG_CORRUPT`: shown though not yet recovered.
    pub(crate) corrupt: bool,
    /// `pict_type`: the first slice's type.
    pub(crate) pict_type: i32,
    /// `crop_left`, `crop_right`, `crop_top` and `crop_bottom`.
    pub(crate) crop: [u32; 4],
    /// `AV_FRAME_FLAG_INTERLACED` and `AV_FRAME_FLAG_TOP_FIELD_FIRST`.
    pub(crate) interlaced: bool,
    pub(crate) top_field_first: bool,
    /// Extra field periods to show it for: 1 for a telecined frame's
    /// third field, 2 for frame doubling, 4 for tripling (`repeat_pict`).
    pub(crate) repeat_pict: i32,
    /// What the SEI said of it.
    pub(crate) side_data: Vec<SideData>,
    /// The `timecode` metadata entry: the last time code, as text.
    pub(crate) timecode: Option<String>,
    /// What the stream's properties were when the frame was allocated
    /// (`ff_decode_frame_props`): its aspect ratio, colour and chroma
    /// siting, as FFmpeg's codec context held them.
    pub(crate) sample_aspect_ratio: (i32, i32),
    pub(crate) color_range: i32,
    pub(crate) color_primaries: i32,
    pub(crate) color_trc: i32,
    pub(crate) colorspace: i32,
    pub(crate) chroma_location: i32,
    /// The time and duration of the packet whose slices began it.
    pub(crate) time: i64,
    pub(crate) duration: u64,
    /// That packet was marked to be discarded (`AV_FRAME_FLAG_DISCARD`):
    /// decoded for the pictures after it, not shown.
    pub(crate) discard: bool,
    /// `FF_DECODE_ERROR_*` bits: how decoding it went wrong.
    pub(crate) decode_error_flags: i32,
}

/// One of FFmpeg's pictures (`H264Picture`).
#[derive(Clone, Debug, Default)]
pub(crate) struct Picture {
    /// The samples (`f`); `None` when the slot is free (`!f->buf[0]`).
    pub(crate) f: Option<Arc<AnyFrame>>,
    /// Whether it is to be shown with film grain (`needs_fg`): begun while
    /// a film grain message was in force, and not refused when the grain
    /// was to be made.
    pub(crate) needs_fg: bool,
    /// The frame shown in its place when it is (`f_grain`): allocated with
    /// `f`, the grain made into it when the picture is finished.
    pub(crate) f_grain: Option<Arc<AnyFrame>>,
    /// The grain it is to be shown with: the film grain side data FFmpeg
    /// attaches to `f`, the first one attached when both fields bring one.
    pub(crate) film_grain: Option<Box<GrainParams>>,
    /// The frame's other properties.
    pub(crate) props: FrameProps,
    /// The quantizer of each macroblock (`qscale_table`), and the
    /// macroblock types (`mb_type`): both `mb_stride` wide, starting two
    /// rows and a macroblock into their allocation ([`TABLE_OFFSET`]).
    pub(crate) qscale_table: Vec<i8>,
    pub(crate) mb_type: Vec<u32>,
    /// Each list's motion vectors, 4 per macroblock across and down
    /// (`motion_val`), starting 4 into their allocation.
    pub(crate) motion_val: [Vec<[i16; 2]>; 2],
    /// Each list's reference index of each 8x8 block (`ref_index`).
    pub(crate) ref_index: [Vec<i8>; 2],
    /// The pool generation the four tables above came from
    /// ([`crate::pool::Tables::generation`]); 0 for none.
    pub(crate) tables_generation: u32,

    /// The top and bottom field's order counts, and the lower of them.
    pub(crate) field_poc: [i32; 2],
    pub(crate) poc: i32,
    /// `frame_num` as the slice header gave it.
    pub(crate) frame_num: i32,
    /// Whether a memory management reset came with it: output must not
    /// mix pictures from before it with those after.
    pub(crate) mmco_reset: i32,
    /// Its picture number, unwrapped for a short-term reference, or its
    /// long-term index.
    pub(crate) pic_id: i32,
    /// Whether it is a long-term reference.
    pub(crate) long_ref: i32,
    /// The order counts of what it was predicted from, for direct
    /// prediction: `[field][list][ref]`.
    pub(crate) ref_poc: [[[i32; 32]; 2]; 2],
    pub(crate) ref_count: [[i32; 2]; 2],
    pub(crate) mbaff: i32,
    /// Whether it was coded as two fields.
    pub(crate) field_picture: i32,
    /// Which of its fields are references (`PICT_*` bits), or
    /// [`DELAYED_PIC_REF`] when held only to be shown.
    pub(crate) reference: i32,
    /// `FRAME_RECOVERED_*` bits: why it can be shown.
    pub(crate) recovered: i32,
    /// A stand-in for a frame-number gap the stream does not allow.
    pub(crate) invalid_gap: i32,
    pub(crate) sei_recovery_frame_cnt: i32,
    /// The PPS it was decoded with.
    pub(crate) pps: Option<Arc<Pps>>,
    pub(crate) mb_width: i32,
    pub(crate) mb_height: i32,
    pub(crate) mb_stride: i32,
    /// A stand-in left gray for a lost reference.
    pub(crate) gray: i32,
}

/// Where a picture's tables start in their allocations: two rows and a
/// macroblock in, so that the rows above the picture can be read
/// (`2 * mb_stride + 1`).
pub(crate) fn table_offset(mb_stride: i32) -> usize {
    usize::try_from(2 * mb_stride + 1).unwrap_or(0)
}

impl Picture {
    /// Whether the slot holds a picture (`f->buf[0]`).
    pub(crate) fn in_use(&self) -> bool {
        self.f.is_some()
    }

    /// Release everything the picture holds and forget all it was
    /// (`ff_h264_unref_picture`): every field after the frame's own goes
    /// back to its starting value, as FFmpeg zeroes them. Its tables are
    /// handed back, for the caller to return to their pool
    /// ([`crate::decoder::Context::unref_pic`]); `None` when the slot held
    /// nothing, tables included.
    #[must_use]
    pub(crate) fn unref(&mut self) -> Option<crate::pool::Tables> {
        self.f.as_ref()?;
        let tables = self.take_tables();
        *self = Self::default();
        Some(tables)
    }

    /// The picture's tables, moved out.
    pub(crate) fn take_tables(&mut self) -> crate::pool::Tables {
        crate::pool::Tables {
            qscale_table: std::mem::take(&mut self.qscale_table),
            mb_type: std::mem::take(&mut self.mb_type),
            motion_val: std::mem::take(&mut self.motion_val),
            ref_index: std::mem::take(&mut self.ref_index),
            generation: std::mem::take(&mut self.tables_generation),
        }
    }

    /// Tables moved in.
    pub(crate) fn put_tables(&mut self, t: crate::pool::Tables) {
        self.qscale_table = t.qscale_table;
        self.mb_type = t.mb_type;
        self.motion_val = t.motion_val;
        self.ref_index = t.ref_index;
        self.tables_generation = t.generation;
    }

    /// The picture without its tables: what a second holder copies, the
    /// tables being held, not copied ([`crate::pool`]).
    pub(crate) fn without_tables(&mut self) -> Self {
        let tables = self.take_tables();
        let copy = self.clone();
        self.put_tables(tables);
        copy
    }

    /// A second holder of `src`'s samples and properties
    /// (`ff_h264_ref_picture`, into a slot that holds nothing): all but the
    /// picture number, which FFmpeg does not copy. `src` is a picture
    /// [`without_tables`](Self::without_tables).
    pub(crate) fn ref_from(&mut self, src: &Self) {
        let pic_id = self.pic_id;
        *self = src.clone();
        self.pic_id = pic_id;
    }

    /// Whether `other`'s frame has the size and layout this one's has:
    /// what FFmpeg compares width, height and format for. A picture
    /// without a frame matches nothing.
    pub(crate) fn same_shape(&self, other: &Self) -> bool {
        match (&self.f, &other.f) {
            (Some(a), Some(b)) => a.size() == b.size() && self.props.format == other.props.format,
            _ => false,
        }
    }
}

/// A reference picture as a slice uses it (`H264Ref`): a frame, or one
/// field of one, and the numbers it goes by in the reference lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Ref {
    /// The picture's slot (`parent`); `None` for an empty entry.
    pub(crate) parent: Option<usize>,
    /// Where each plane starts in the frame, in samples, and its row
    /// stride: a bottom field starts a row down, a field strides two rows
    /// (`data`, `linesize`).
    pub(crate) offset: [usize; 3],
    pub(crate) stride: [usize; 3],
    /// Which fields it is: `PICT_TOP_FIELD`, `PICT_BOTTOM_FIELD` or both.
    pub(crate) reference: i32,
    pub(crate) poc: i32,
    pub(crate) pic_id: i32,
}

#[cfg(test)]
mod tests;
