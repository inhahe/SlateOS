//! The planes a frame is decoded into, laid out as libvpx lays them out.
//!
//! A frame is decoded at a whole number of macroblocks -- its width and
//! height rounded up to 16 -- inside a border of 32 pixels (16 for chroma)
//! on every side. The layout is libvpx's own, stride included, because the
//! decoder's behaviour depends on it: intra prediction reads the row above
//! and the column left of the picture, which libvpx fills with 127 and 129;
//! motion vectors reach up to 21 pixels outside the picture into a border
//! that holds copies of its edge pixels; and a few hostile streams make
//! libvpx predict from pixels a frame never wrote, which are then whatever
//! the buffer held before.
//!
//! Translated into Rust from libvpx v1.17.0's `vpx_scale/generic/yv12config.c`
//! (`vp8_yv12_realloc_frame_buffer`), `vp8/common/setupintrarecon.c` and
//! `setupintrarecon.h`, `vp8/common/extend.c` (`vp8_extend_mb_row`) and the
//! border extension of `vp8/decoder/decodeframe.c` (copyright the WebM project
//! authors), used under libvpx's BSD licence and patent grant
//! (`licenses/libvpx-LICENSE`, `licenses/libvpx-PATENTS`).

#![allow(
    clippy::indexing_slicing,
    reason = "every index is a position inside a plane allocated to hold the picture and its border, taken from the plane's own dimensions; positions a stream chooses (motion vectors) are checked where they are used, in `inter`"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "plane dimensions are at most 16384 plus the border, and their products fit a usize"
)]

/// libvpx's `VP8BORDERINPIXELS`: the luma border; chroma's is half.
pub(crate) const BORDER: usize = 32;

/// One plane: `height` rows of `width` pixels (the picture rounded up to
/// whole macroblocks) inside a border of `border` pixels, rows `stride`
/// apart.
#[derive(Clone, Debug)]
pub(crate) struct Plane {
    pub(crate) data: Vec<u8>,
    pub(crate) stride: usize,
    pub(crate) border: usize,
    pub(crate) width: usize,
    pub(crate) height: usize,
}

impl Plane {
    fn new(width: usize, height: usize, border: usize, stride: usize) -> Self {
        Self {
            data: vec![0; (height + 2 * border) * stride],
            stride,
            border,
            width,
            height,
        }
    }

    /// The index of pixel (0, 0): libvpx's `y_buffer` (or `u_buffer`,
    /// `v_buffer`) as an offset.
    #[inline]
    pub(crate) fn origin(&self) -> usize {
        self.border * self.stride + self.border
    }

    /// The index of the pixel `x` right and `y` down of (0, 0), which must
    /// be inside the plane or its border.
    #[inline]
    pub(crate) fn at(&self, x: usize, y: usize) -> usize {
        self.origin() + y * self.stride + x
    }

    /// Copy the left and right columns of `rows` rows from `y` out over the
    /// border: libvpx's `yv12_extend_frame_left_right_c`, for one plane.
    fn extend_left_right(&mut self, y: usize, rows: usize) {
        let (border, width) = (self.border, self.width);
        for row in y..y + rows {
            let start = self.at(0, row);
            let left = self.data[start];
            let right = self.data[start + width - 1];
            self.data[start - border..start].fill(left);
            self.data[start + width..start + width + border].fill(right);
        }
    }

    /// Copy the first row, border included, over the rows above it, and the
    /// last below it: libvpx's `yv12_extend_frame_top_c` and
    /// `yv12_extend_frame_bottom_c`, for one plane.
    fn extend_top_bottom(&mut self) {
        let (border, stride) = (self.border, self.stride);
        let first = self.at(0, 0) - border;
        let last = self.at(0, self.height - 1) - border;
        for i in 0..border {
            self.data
                .copy_within(first..first + stride, first - (border - i) * stride);
            self.data
                .copy_within(last..last + stride, last + (i + 1) * stride);
        }
    }
}

/// Where macroblocks are reconstructed and filtered: a frame's three planes
/// ([`Frame::target`]), or a band of rows of them a thread works on, each
/// with the index pixel (0, 0) has in its slice -- before the slice's start,
/// for a band below the top, so signed.
pub(crate) struct Target<'a> {
    pub(crate) planes: [&'a mut [u8]; 3],
    pub(crate) strides: [usize; 3],
    origins: [isize; 3],
}

impl<'a> Target<'a> {
    /// A view of `planes`, rows `strides` apart, in which pixel (0, 0) of
    /// each would be at `origins`.
    pub(crate) fn new(planes: [&'a mut [u8]; 3], strides: [usize; 3], origins: [isize; 3]) -> Self {
        Self {
            planes,
            strides,
            origins,
        }
    }

    /// The index of the pixel `x` right and `y` down of (0, 0) in plane `p`,
    /// which must be one the view holds.
    #[inline]
    #[allow(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "positions within planes of at most 16384 pixels a side and their border; a pixel the view holds is at a non-negative index"
    )]
    pub(crate) fn at(&self, p: usize, x: usize, y: usize) -> usize {
        (self.origins[p] + (y * self.strides[p] + x) as isize) as usize
    }
}

/// A decoded frame: libvpx's `YV12_BUFFER_CONFIG`, less its `corrupted`
/// flag, which the decoder keeps beside its buffers.
#[derive(Clone, Debug)]
pub(crate) struct Frame {
    /// Y, U and V.
    pub(crate) planes: [Plane; 3],
    /// The picture's size, which the planes round up to whole macroblocks.
    pub(crate) display_width: u32,
    pub(crate) display_height: u32,
}

impl Frame {
    /// A frame for a picture of `width` by `height` pixels, zeroed as libvpx
    /// zeroes a new buffer: `vp8_yv12_alloc_frame_buffer` after
    /// `vp8_alloc_frame_buffers` has rounded the size up to whole
    /// macroblocks.
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let aligned_width = (width as usize + 15) & !15;
        let aligned_height = (height as usize + 15) & !15;
        let y_stride = (aligned_width + 2 * BORDER + 31) & !31;
        let uv_stride = y_stride >> 1;
        Self {
            planes: [
                Plane::new(aligned_width, aligned_height, BORDER, y_stride),
                Plane::new(
                    aligned_width >> 1,
                    aligned_height >> 1,
                    BORDER / 2,
                    uv_stride,
                ),
                Plane::new(
                    aligned_width >> 1,
                    aligned_height >> 1,
                    BORDER / 2,
                    uv_stride,
                ),
            ],
            display_width: width,
            display_height: height,
        }
    }

    /// The whole frame as a [`Target`].
    #[allow(
        clippy::cast_possible_wrap,
        reason = "an origin is a border's rows and columns into a plane of at most 16384 pixels a side"
    )]
    pub(crate) fn target(&mut self) -> Target<'_> {
        let [y, u, v] = &mut self.planes;
        let origins = [
            y.origin() as isize,
            u.origin() as isize,
            v.origin() as isize,
        ];
        let strides = [y.stride, u.stride, v.stride];
        Target::new([&mut y.data, &mut u.data, &mut v.data], strides, origins)
    }

    /// Prepare the row above the picture for intra prediction: 127 from the
    /// pixel left of the first column to four right of the last, as libvpx's
    /// `vp8_setup_intra_recon_top_line` does.
    pub(crate) fn setup_intra_top_line(&mut self) {
        for plane in &mut self.planes {
            let start = plane.origin() - plane.stride - 1;
            let width = plane.width;
            plane.data[start..start + width + 5].fill(127);
        }
    }

    /// Prepare the column left of macroblock row `mb_row` for intra
    /// prediction: 129, as libvpx's `setup_intra_recon_left` does.
    pub(crate) fn setup_intra_left(&mut self, mb_row: usize) {
        for (i, plane) in self.planes.iter_mut().enumerate() {
            let size = if i == 0 { 16 } else { 8 };
            for row in 0..size {
                let at = plane.at(0, mb_row * size + row) - 1;
                plane.data[at] = 129;
            }
        }
    }

    /// Copy the last pixel of rows 14 and 15 of macroblock row `mb_row` (6
    /// and 7 for chroma) four pixels out to the right, where the row below's
    /// last macroblock looks for the pixels above and to its right: libvpx's
    /// `vp8_extend_mb_row`, called with the end of the row.
    pub(crate) fn extend_mb_row(&mut self, mb_row: usize) {
        for (i, plane) in self.planes.iter_mut().enumerate() {
            let size = if i == 0 { 16 } else { 8 };
            for row in size - 2..size {
                let at = plane.at(plane.width, mb_row * size + row);
                let last = plane.data[at - 1];
                plane.data[at..at + 4].fill(last);
            }
        }
    }

    /// Copy the edge pixels of macroblock row `mb_row` out over the left and
    /// right borders: libvpx's `yv12_extend_frame_left_right_c`.
    pub(crate) fn extend_row_left_right(&mut self, mb_row: usize) {
        for (i, plane) in self.planes.iter_mut().enumerate() {
            let size = if i == 0 { 16 } else { 8 };
            plane.extend_left_right(mb_row * size, size);
        }
    }

    /// Copy the first and last rows, borders included, out over the top and
    /// bottom borders: libvpx's `yv12_extend_frame_top_c` and
    /// `yv12_extend_frame_bottom_c`.
    pub(crate) fn extend_top_bottom(&mut self) {
        for plane in &mut self.planes {
            plane.extend_top_bottom();
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::cast_possible_truncation,
        reason = "a test pattern: pixel values wrap at 256 on purpose"
    )]

    use super::*;

    #[test]
    fn a_frame_is_laid_out_as_libvpx_lays_it_out() {
        // 175x143: rounded up to 176x144; the luma stride is 176 + 64
        // rounded up to 32, the chroma stride half of it.
        let f = Frame::new(175, 143);
        let [y, u, v] = &f.planes;
        assert_eq!((y.width, y.height, y.stride, y.border), (176, 144, 256, 32));
        assert_eq!((u.width, u.height, u.stride, u.border), (88, 72, 128, 16));
        assert_eq!((v.width, v.height, v.stride, v.border), (88, 72, 128, 16));
        assert_eq!(y.data.len(), (144 + 64) * 256);
        assert_eq!(u.data.len(), (72 + 32) * 128);
        assert_eq!(y.origin(), 32 * 256 + 32);
        assert_eq!(u.origin(), 16 * 128 + 16);
        assert_eq!((f.display_width, f.display_height), (175, 143));
    }

    #[test]
    fn the_intra_edges_are_127_above_and_129_left() {
        let mut f = Frame::new(32, 32);
        f.setup_intra_top_line();
        f.setup_intra_left(1);
        let y = &f.planes[0];
        let above = y.origin() - y.stride;
        assert!(y.data[above - 1..above + 32 + 4].iter().all(|&p| p == 127));
        assert_eq!(
            y.data[above + 32 + 4],
            0,
            "five past the width is left alone"
        );
        for row in 16..32 {
            assert_eq!(y.data[y.at(0, row) - 1], 129);
        }
        assert_eq!(y.data[y.at(0, 15) - 1], 0, "only the row asked for");
    }

    #[test]
    fn the_border_copies_the_edges() {
        let mut f = Frame::new(16, 16);
        for (i, plane) in f.planes.iter_mut().enumerate() {
            for row in 0..plane.height {
                for col in 0..plane.width {
                    let at = plane.at(col, row);
                    plane.data[at] = (i * 64 + row * 3 + col) as u8;
                }
            }
        }
        f.extend_row_left_right(0);
        f.extend_top_bottom();
        let y = &f.planes[0];
        // The top-left corner of the border is the first pixel.
        assert_eq!(y.data[0], y.data[y.origin()]);
        // The bottom-right corner is the last.
        let last = y.at(15, 15);
        assert_eq!(
            y.data[(16 + 64) * y.stride - y.stride + 16 + 32 + 31],
            y.data[last]
        );
        // A row's right border repeats its last pixel.
        assert_eq!(y.data[y.at(16 + 31, 7)], y.data[y.at(15, 7)]);
        let u = &f.planes[1];
        assert_eq!(u.data[u.at(0, 0) - 16], u.data[u.at(0, 0)]);
    }
}
