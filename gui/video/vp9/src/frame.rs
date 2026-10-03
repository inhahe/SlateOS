//! Frame buffers: the planes a frame is decoded into and later predicted from.
//!
//! libvpx lays a frame out as three planes, each with a border around it for
//! motion compensation to read beyond the picture's edge. This decoder keeps
//! the planes and drops the border. Where libvpx would read the border, it
//! builds the edge-extended block itself, which libvpx's decoder already does
//! for every reference block that crosses the edge: `dec_build_inter_predictors`,
//! ported in `inter.rs`.
//!
//! What a plane keeps instead is room to the next superblock. Prediction and
//! reconstruction write whole blocks, and a block at the picture's right or
//! bottom edge may reach up to 24 luma pixels past it. libvpx's border
//! absorbs those writes; here, sizing every plane to whole 64-pixel
//! superblocks does. The pixels between the picture's edge and its size
//! rounded up to 8 do matter: intra prediction reads them, which is why each
//! plane records three sizes, not one.
//!
//! Translated in part from libvpx v1.17.0's `vpx_scale/generic/yv12config.c`
//! (copyright the WebM project authors), used under libvpx's BSD licence and
//! patent grant (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

use crate::Error;

/// A sample type: `u8` for 8-bit streams, `u16` for 10- and 12-bit ones.
pub trait Pixel: Copy + Default + PartialEq + Send + Sync + core::fmt::Debug + 'static {
    /// The sample as an int.
    fn int(self) -> i32;
    /// An int already clamped to the stream's range, as a sample.
    fn from_int(v: i32) -> Self;
    /// The frame, if its samples are of this type.
    fn from_any(f: &AnyFrame) -> Option<&FrameBuf<Self>>;
    /// The samples as bytes, if they are bytes (an 8-bit stream's): lets a
    /// hot loop take a path written for bytes. `None` for 16-bit samples.
    /// Which it is is settled when the code is compiled, not per call.
    fn bytes(s: &[Self]) -> Option<&[u8]>;
    /// `bytes`, for writing.
    fn bytes_mut(s: &mut [Self]) -> Option<&mut [u8]>;
}

impl Pixel for u8 {
    #[inline(always)]
    fn int(self) -> i32 {
        i32::from(self)
    }
    #[inline(always)]
    fn from_int(v: i32) -> Self {
        // Callers clamp to 0..=255 first; the cast keeps the low byte as
        // libvpx's stores to a `uint8_t` do.
        v as u8
    }
    fn from_any(f: &AnyFrame) -> Option<&FrameBuf<Self>> {
        match f {
            AnyFrame::Eight(f) => Some(f),
            AnyFrame::High(_) => None,
        }
    }
    #[inline(always)]
    fn bytes(s: &[Self]) -> Option<&[u8]> {
        Some(s)
    }
    #[inline(always)]
    fn bytes_mut(s: &mut [Self]) -> Option<&mut [u8]> {
        Some(s)
    }
}

impl Pixel for u16 {
    #[inline(always)]
    fn int(self) -> i32 {
        i32::from(self)
    }
    #[inline(always)]
    fn from_int(v: i32) -> Self {
        // Callers clamp to the bit depth's range first.
        v as u16
    }
    fn from_any(f: &AnyFrame) -> Option<&FrameBuf<Self>> {
        match f {
            AnyFrame::High(f) => Some(f),
            AnyFrame::Eight(_) => None,
        }
    }
    #[inline(always)]
    fn bytes(_: &[Self]) -> Option<&[u8]> {
        None
    }
    #[inline(always)]
    fn bytes_mut(_: &mut [Self]) -> Option<&mut [u8]> {
        None
    }
}

/// One plane of a frame.
#[derive(Clone, Debug)]
pub struct Plane<P> {
    /// The samples, `stride` per row, `alloc_height` rows.
    pub data: Vec<P>,
    /// Samples from one row to the next.
    pub stride: usize,
    /// Rows allocated: the picture's height rounded up to whole superblocks.
    pub alloc_height: usize,
    /// The plane's width rounded up to 8 luma pixels: libvpx's
    /// `y_width`/`uv_width`, the edge intra prediction stops at.
    pub width: usize,
    /// The same, down: libvpx's `y_height`/`uv_height`.
    pub height: usize,
    /// The picture's own width in this plane: libvpx's
    /// `y_crop_width`/`uv_crop_width`, the edge motion compensation extends.
    pub crop_width: usize,
    /// The same, down.
    pub crop_height: usize,
}

/// A decoded frame.
#[derive(Clone, Debug)]
pub struct FrameBuf<P> {
    /// Y, U, V.
    pub planes: [Plane<P>; 3],
    /// The picture's width and height in luma pixels.
    pub width: u32,
    pub height: u32,
    /// Whether chroma is halved horizontally and vertically.
    pub ss_x: u8,
    pub ss_y: u8,
    /// 8, 10 or 12.
    pub bit_depth: u8,
    /// libvpx's `vpx_color_space_t`, as the stream declared it.
    pub color_space: u8,
    /// Whether samples use the full range rather than the studio range.
    pub full_range: bool,
    /// The size the stream suggests the picture be shown at.
    pub render_width: u32,
    pub render_height: u32,
}

/// The largest frame dimension VP9 can code: sizes are 16-bit fields plus one.
pub const MAX_DIMENSION: u32 = 65536;

/// `v` rounded up to a multiple of `1 << log2`.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "dimensions are at most 65536, so rounding up cannot overflow"
)]
const fn align_up(v: usize, log2: u32) -> usize {
    let mask = (1usize << log2) - 1;
    (v + mask) & !mask
}

impl<P: Pixel> FrameBuf<P> {
    /// A frame of `width` by `height` with the given chroma subsampling, its
    /// samples zero.
    ///
    /// # Errors
    ///
    /// [`Error::Unsupported`] for a zero or oversized dimension, or when the
    /// buffer cannot be allocated.
    #[allow(
        clippy::arithmetic_side_effects,
        reason = "dimensions are checked to be at most 65536 first"
    )]
    pub fn new(width: u32, height: u32, ss_x: u8, ss_y: u8, bit_depth: u8) -> Result<Self, Error> {
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
            return Err(Error::Unsupported("a frame dimension is out of range"));
        }
        let (w, h) = (width as usize, height as usize);
        let (sx, sy) = (u32::from(ss_x & 1), u32::from(ss_y & 1));
        // libvpx: aligned_width = (width + 7) & ~7, the planes' "width".
        let (aligned_w, aligned_h) = (align_up(w, 3), align_up(h, 3));
        // Whole superblocks: where the largest block at the edge can reach.
        let (alloc_w, alloc_h) = (align_up(w, 6), align_up(h, 6));
        let luma = Self::plane(alloc_w, alloc_h, aligned_w, aligned_h, w, h)?;
        let chroma = || {
            Self::plane(
                alloc_w >> sx,
                alloc_h >> sy,
                aligned_w >> sx,
                aligned_h >> sy,
                (w + sx as usize) >> sx,
                (h + sy as usize) >> sy,
            )
        };
        Ok(Self {
            planes: [luma, chroma()?, chroma()?],
            width,
            height,
            ss_x: ss_x & 1,
            ss_y: ss_y & 1,
            bit_depth,
            color_space: 0,
            full_range: false,
            render_width: width,
            render_height: height,
        })
    }

    fn plane(
        alloc_w: usize,
        alloc_h: usize,
        width: usize,
        height: usize,
        crop_width: usize,
        crop_height: usize,
    ) -> Result<Plane<P>, Error> {
        let len = alloc_w
            .checked_mul(alloc_h)
            .ok_or(Error::Unsupported("a frame is too large to allocate"))?;
        let mut data = Vec::new();
        data.try_reserve_exact(len)
            .map_err(|_| Error::Unsupported("a frame is too large to allocate"))?;
        data.resize(len, P::default());
        Ok(Plane {
            data,
            stride: alloc_w,
            alloc_height: alloc_h,
            width,
            height,
            crop_width,
            crop_height,
        })
    }
}

/// A frame of either sample type, as the reference slots hold them.
#[derive(Clone, Debug)]
pub enum AnyFrame {
    /// An 8-bit frame.
    Eight(FrameBuf<u8>),
    /// A 10- or 12-bit frame.
    High(FrameBuf<u16>),
}

impl AnyFrame {
    /// The picture's width in luma pixels.
    #[must_use]
    pub fn width(&self) -> u32 {
        match self {
            Self::Eight(f) => f.width,
            Self::High(f) => f.width,
        }
    }

    /// The picture's height in luma pixels.
    #[must_use]
    pub fn height(&self) -> u32 {
        match self {
            Self::Eight(f) => f.height,
            Self::High(f) => f.height,
        }
    }

    /// 8, 10 or 12.
    #[must_use]
    pub fn bit_depth(&self) -> u8 {
        match self {
            Self::Eight(f) => f.bit_depth,
            Self::High(f) => f.bit_depth,
        }
    }

    /// Horizontal and vertical chroma subsampling.
    #[must_use]
    pub fn subsampling(&self) -> (u8, u8) {
        match self {
            Self::Eight(f) => (f.ss_x, f.ss_y),
            Self::High(f) => (f.ss_x, f.ss_y),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    #[test]
    fn a_plane_records_libvpx_sizes_and_superblock_room() {
        let f = FrameBuf::<u8>::new(352, 288, 1, 1, 8).unwrap();
        let y = &f.planes[0];
        assert_eq!((y.width, y.height), (352, 288));
        assert_eq!((y.crop_width, y.crop_height), (352, 288));
        assert_eq!((y.stride, y.alloc_height), (384, 320), "whole superblocks");
        let u = &f.planes[1];
        assert_eq!((u.width, u.height), (176, 144));
        assert_eq!((u.stride, u.alloc_height), (192, 160));
    }

    #[test]
    fn odd_sizes_round_as_libvpx_does() {
        let f = FrameBuf::<u16>::new(35, 9, 1, 1, 10).unwrap();
        let y = &f.planes[0];
        assert_eq!((y.width, y.height), (40, 16), "aligned to 8");
        assert_eq!((y.crop_width, y.crop_height), (35, 9));
        let u = &f.planes[1];
        assert_eq!((u.width, u.height), (20, 8));
        assert_eq!(
            (u.crop_width, u.crop_height),
            (18, 5),
            "(35 + 1) >> 1, (9 + 1) >> 1"
        );
    }

    #[test]
    fn a_frame_too_large_or_empty_is_refused() {
        assert!(FrameBuf::<u8>::new(0, 16, 1, 1, 8).is_err());
        assert!(FrameBuf::<u8>::new(16, MAX_DIMENSION + 1, 1, 1, 8).is_err());
    }
}
