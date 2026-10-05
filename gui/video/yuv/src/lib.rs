//! Pictures in YUV -- the form video codecs and AVIF store them in --
//! turned into `0xAARRGGBB` pixels as libavif turns them, and planes of
//! samples scaled to another size as libyuv scales them.
//!
//! - [`reformat`]: a picture to pixels, libavif's `avifImageYUVToRGB`: which
//!   arithmetic each colour description gets -- libyuv's fixed point for the
//!   matrices libyuv has constants for, libavif's own floating point for the
//!   rest (SMPTE 240M, FCC, YCgCo, the identity matrix) -- with alpha, and
//!   premultiplied alpha undone. What a caller with a decoded picture calls.
//! - [`convert`]: libyuv's conversions beneath it, as libyuv's x86 code
//!   computes them (its C copies the x86 formulation): BT.601, BT.709 and
//!   BT.2020 at limited and full range ([`convert::I601`] and its siblings),
//!   8-, 10- and 12-bit samples, 4:4:4, 4:2:2 and 4:2:0 with libyuv's chroma
//!   upsampling, grey, alpha carried through, and `ARGBUnattenuate`.
//! - [`scale`]: `ScalePlane` and `ScalePlane_12` with `kFilterBox`, every
//!   method they pick by the sizes, as libyuv's C computes them.
//!
//! libyuv's revision is 1924 (`644251f252a84bf8ce91ff0aca86a9b16b069ab8`),
//! the one libavif 1.4.2 pins; libavif's is 1.3.0. Where libyuv's C and its
//! x86 SIMD compute different pixels, this is the C, libyuv's reference
//! (design-decisions §1344). Its users: AVIF pictures (`gui/imagecodec`),
//! and video frames (`gui/video/codec`), which therefore have the colours an
//! AVIF still of the same picture would.
//!
//! Portions of this crate are copyright 2011, 2013 and 2015 The LibYuv
//! Project Authors, from libyuv, and used under its BSD licence and patent
//! grant: `licenses/libyuv-LICENSE`, `licenses/libyuv-PATENTS`. Portions
//! ([`reformat`]) are copyright 2019-2020 Joe Drago, from libavif, used
//! under its BSD-2-Clause licence: `gui/imagecodec/licenses/libavif-LICENSE.txt`.

#![no_std]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;

pub mod convert;
pub mod reformat;
pub mod scale;

/// A sample of 8 or 16 bits.
pub trait Sample: Copy + Default + Into<u32> + 'static {}

impl Sample for u8 {}
impl Sample for u16 {}

/// One plane of samples to read: `height` rows of `width`, each `stride`
/// samples after the last -- a decoder's picture as it hands it out, without
/// a copy.
#[derive(Debug)]
pub struct Plane<'a, T> {
    pub samples: &'a [T],
    pub stride: usize,
    pub width: usize,
    pub height: usize,
}

// A view copies whatever its samples are, so not derived (which would ask
// `T: Copy`).
impl<T> Clone for Plane<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Plane<'_, T> {}

impl<'a, T> Plane<'a, T> {
    /// Row `y`: empty past the last, or where the samples run out.
    pub fn row(&self, y: usize) -> &'a [T] {
        if y >= self.height {
            return &[];
        }
        let start = y.saturating_mul(self.stride);
        self.samples
            .get(start..start.saturating_add(self.width))
            .unwrap_or_default()
    }
}

/// One plane of samples, owned and packed: `height` rows of `width`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlaneBuf<T> {
    pub width: usize,
    pub height: usize,
    pub samples: Vec<T>,
}

impl<T> PlaneBuf<T> {
    /// The plane to read.
    pub fn view(&self) -> Plane<'_, T> {
        Plane {
            samples: &self.samples,
            stride: self.width,
            width: self.width,
            height: self.height,
        }
    }

    /// Row `y`: empty past the last.
    pub fn row(&self, y: usize) -> &[T] {
        let start = y.saturating_mul(self.width);
        self.samples
            .get(start..start.saturating_add(self.width))
            .unwrap_or_default()
    }

    /// Row `y`, to write: empty past the last.
    pub fn row_mut(&mut self, y: usize) -> &mut [T] {
        let start = y.saturating_mul(self.width);
        let end = start.saturating_add(self.width);
        self.samples.get_mut(start..end).unwrap_or_default()
    }
}

impl<T: Copy + Default> PlaneBuf<T> {
    /// A `width` x `height` plane of zeros; `None` if its size overflows.
    pub fn new(width: usize, height: usize) -> Option<Self> {
        let len = width.checked_mul(height)?;
        Some(Self {
            width,
            height,
            samples: vec![T::default(); len],
        })
    }

    /// A copy of the `width` x `height` rectangle at (`x`, `y`), cut to the
    /// plane.
    pub fn crop(&self, x: usize, y: usize, width: usize, height: usize) -> Self {
        let mut samples = Vec::with_capacity(width.saturating_mul(height));
        for row in y..y.saturating_add(height) {
            samples.extend(self.row(row).iter().skip(x).take(width));
        }
        Self {
            width,
            height,
            samples,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "a test: a failure should be loud")]

    use super::*;

    #[test]
    fn a_plane_with_a_stride_reads_only_its_width() {
        let samples = [1u8, 2, 9, 3, 4, 9, 5, 6, 9];
        let p = Plane {
            samples: &samples,
            stride: 3,
            width: 2,
            height: 3,
        };
        assert_eq!(p.row(1), [3, 4]);
        assert!(p.row(3).is_empty(), "past the last row");
    }

    #[test]
    fn an_owned_plane_crops_and_views() {
        let mut b = PlaneBuf::<u8>::new(3, 2).unwrap();
        b.row_mut(1).copy_from_slice(&[7, 8, 9]);
        assert_eq!(b.crop(1, 1, 2, 1).samples, [8, 9]);
        assert_eq!(b.view().row(1), [7, 8, 9]);
        assert!(PlaneBuf::<u8>::new(usize::MAX, 2).is_none());
    }
}
