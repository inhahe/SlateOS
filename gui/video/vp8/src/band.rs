//! A band: a thread's private copy of the macroblock row it is decoding,
//! with the rows above it that the row reads.
//!
//! Threads that decode rows of one frame at once (`threading`, `pipeline`)
//! cannot share the frame's planes -- this crate has no `unsafe` -- so each
//! decodes its row into a band of its own: per plane, [`APRON`] rows of the
//! row above (the loop filter's reach above a macroblock's top edge; the
//! last of them is what intra prediction reads), then the row's own rows,
//! at the frame's stride and with its border, so that prediction,
//! reconstruction and filtering index a band as they index the frame
//! ([`Band::target`]). What a band holds goes into the frame once final.

#![allow(
    clippy::indexing_slicing,
    reason = "band positions are rows and columns inside a band sized for a macroblock row of the frame, whose geometry is checked against the frame when it is taken from it"
)]
#![allow(
    clippy::arithmetic_side_effects,
    reason = "positions and sizes are bounded by the frame's (at most 16384 pixels a side, plus the border)"
)]

use crate::frame::{Frame, Target};

/// How many rows of the row above a row's filters reach: the normal loop
/// filter's `p3..p0` across a macroblock's top edge.
pub(crate) const APRON: usize = 4;

/// A macroblock's width in each plane: luma, then the two chroma planes.
const SIZES: [usize; 3] = [16, 8, 8];

/// One row of a macroblock in every plane, Y then U then V.
pub(crate) const EDGE: usize = 16 + 8 + 8;

/// A macroblock's last [`APRON`] rows in every plane, Y then U then V, each
/// row by row.
pub(crate) const TAIL: usize = APRON * EDGE;

/// One plane's layout, as the frame's.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Geometry {
    pub(crate) stride: usize,
    pub(crate) border: usize,
    /// The picture's width, rounded up to whole macroblocks.
    pub(crate) width: usize,
    /// A macroblock's width and height in this plane.
    pub(crate) size: usize,
}

impl Geometry {
    /// `frame`'s planes, if laid out as a band assumes -- `mb_rows` by
    /// `mb_cols` whole macroblocks, with borders wide enough for the pixels
    /// intra prediction reads left and right of them -- as `Frame::new`
    /// lays them out.
    pub(crate) fn of(frame: &Frame, mb_rows: usize, mb_cols: usize) -> Option<[Self; 3]> {
        let geometry: [Self; 3] = core::array::from_fn(|p| {
            let plane = &frame.planes[p];
            Self {
                stride: plane.stride,
                border: plane.border,
                width: plane.width,
                size: SIZES[p],
            }
        });
        let fits = frame.planes.iter().zip(&geometry).all(|(plane, g)| {
            g.width == mb_cols * g.size
                && plane.height == mb_rows * g.size
                && g.border >= APRON + 4
                && g.stride >= g.width + 2 * g.border
                && plane.data.len() == (plane.height + 2 * g.border) * g.stride
        });
        if !fits {
            debug_assert!(false, "a frame laid out unlike Frame::new's");
            return None;
        }
        Some(geometry)
    }
}

/// A thread's copy of the macroblock row it is decoding: per plane, the
/// last [`APRON`] rows of the row above and then the row's own, at the
/// frame's stride, with its border.
pub(crate) struct Band {
    planes: [Vec<u8>; 3],
    geometry: [Geometry; 3],
}

impl Band {
    pub(crate) fn new(geometry: [Geometry; 3]) -> Self {
        Self {
            planes: core::array::from_fn(|p| {
                let g = &geometry[p];
                vec![0; (APRON + g.size) * g.stride]
            }),
            geometry,
        }
    }

    /// The index in plane `p` of pixel `x` of the band's row `row`.
    fn at(&self, p: usize, row: usize, x: usize) -> usize {
        let g = &self.geometry[p];
        row * g.stride + g.border + x
    }

    /// The band as the frame's macroblock row `mb_row`, for reconstruction
    /// and filtering.
    #[allow(
        clippy::cast_possible_wrap,
        reason = "offsets within planes of at most 16384 pixels a side and their border"
    )]
    pub(crate) fn target(&mut self, mb_row: usize) -> Target<'_> {
        let g = self.geometry;
        let origins = core::array::from_fn(|p| {
            let band_start = (g[p].border + APRON * g[p].stride) as isize;
            band_start - (mb_row * g[p].size * g[p].stride) as isize
        });
        let [y, u, v] = &mut self.planes;
        Target::new(
            [y.as_mut_slice(), u.as_mut_slice(), v.as_mut_slice()],
            g.map(|g| g.stride),
            origins,
        )
    }

    /// Set what intra prediction reads around macroblock row `mb_row` that
    /// no macroblock writes: 129 left of each of its rows and of the row
    /// above, as libvpx's `setup_intra_recon_left`; and above the frame's
    /// first row, 127 from one pixel left of it to four right of its end, as
    /// `vp8_setup_intra_recon_top_line`.
    pub(crate) fn prepare(&mut self, mb_row: usize) {
        for (plane, g) in self.planes.iter_mut().zip(&self.geometry) {
            for row in APRON - 1..APRON + g.size {
                plane[row * g.stride + g.border - 1] = 129;
            }
            if mb_row == 0 {
                let at = (APRON - 1) * g.stride + g.border - 1;
                plane[at..at + g.width + 5].fill(127);
            }
        }
    }

    /// Macroblock `col`'s bottom row in each plane.
    pub(crate) fn edge(&self, col: usize) -> [u8; EDGE] {
        let mut out = [0; EDGE];
        let mut o = 0;
        for (p, g) in self.geometry.iter().enumerate() {
            let at = self.at(p, APRON + g.size - 1, col * g.size);
            out[o..o + g.size].copy_from_slice(&self.planes[p][at..at + g.size]);
            o += g.size;
        }
        out
    }

    /// Put macroblock `col` of the row above's bottom row above the band's
    /// own rows; after the row's `last`, its last pixel four times more,
    /// where the last macroblock of the band's row looks above and to its
    /// right: libvpx's `vp8_extend_mb_row`.
    pub(crate) fn put_edge(&mut self, col: usize, edge: &[u8; EDGE], last: bool) {
        let mut o = 0;
        for p in 0..3 {
            let g = self.geometry[p];
            let at = self.at(p, APRON - 1, col * g.size);
            let plane = &mut self.planes[p];
            plane[at..at + g.size].copy_from_slice(&edge[o..o + g.size]);
            if last {
                let end = at + g.size;
                let v = plane[end - 1];
                plane[end..end + 4].fill(v);
            }
            o += g.size;
        }
    }

    /// Macroblock `col`'s last [`APRON`] rows in each plane.
    pub(crate) fn tail(&self, col: usize) -> [u8; TAIL] {
        let mut out = [0; TAIL];
        let mut o = 0;
        for (p, g) in self.geometry.iter().enumerate() {
            for row in g.size..g.size + APRON {
                let at = self.at(p, row, col * g.size);
                out[o..o + g.size].copy_from_slice(&self.planes[p][at..at + g.size]);
                o += g.size;
            }
        }
        out
    }

    /// Put macroblock `col` of the row above's last rows above the band's
    /// own.
    pub(crate) fn put_tail(&mut self, col: usize, tail: &[u8; TAIL]) {
        let mut o = 0;
        for p in 0..3 {
            let g = self.geometry[p];
            for row in 0..APRON {
                let at = self.at(p, row, col * g.size);
                self.planes[p][at..at + g.size].copy_from_slice(&tail[o..o + g.size]);
                o += g.size;
            }
        }
    }

    /// Copy the band's rows that are final into its row's `pieces` of the
    /// frame, and out over the left and right borders, as libvpx's
    /// `yv12_extend_frame_left_right_c`: the rows above (but above the first
    /// row, which is border) and the row's own but its last four (which the
    /// row below's piece takes), or all of them in the `last` row.
    pub(crate) fn write_back(
        &self,
        mb_row: usize,
        last: bool,
        pieces: &mut [&mut [u8]; 3],
    ) -> Option<()> {
        for ((band, g), piece) in self
            .planes
            .iter()
            .zip(&self.geometry)
            .zip(pieces.iter_mut())
        {
            let first = if mb_row == 0 { APRON } else { 0 };
            let end = if last { APRON + g.size } else { g.size };
            // A piece starts four rows above its row; the first, at the
            // top of the plane's border.
            let offset = if mb_row == 0 { g.border - APRON } else { 0 };
            for row in first..end {
                let from = row * g.stride + g.border;
                let to = (row + offset) * g.stride;
                let out = piece.get_mut(to..to + 2 * g.border + g.width)?;
                let (left, rest) = out.split_at_mut(g.border);
                let (picture, right) = rest.split_at_mut(g.width);
                picture.copy_from_slice(band.get(from..from + g.width)?);
                left.fill(*picture.first()?);
                right.fill(*picture.last()?);
            }
        }
        Some(())
    }

    /// Put the row above's bottom row -- `above`, by plane, the picture's
    /// width of it, unfiltered -- above the band's own rows, its last pixel
    /// four times more after it, where the row's last macroblock looks
    /// above and to its right: libvpx's `vp8_extend_mb_row`.
    pub(crate) fn put_above(&mut self, above: &[Vec<u8>; 3]) {
        for (p, row) in above.iter().enumerate() {
            let g = self.geometry[p];
            let at = self.at(p, APRON - 1, 0);
            let plane = &mut self.planes[p];
            plane[at..at + g.width].copy_from_slice(&row[..g.width]);
            let last = plane[at + g.width - 1];
            plane[at + g.width..at + g.width + 4].fill(last);
        }
    }

    /// The band's own bottom row, the picture's width of it, by plane: what
    /// the row below predicts from.
    pub(crate) fn take_bottom(&self, out: &mut [Vec<u8>; 3]) {
        for (p, row) in out.iter_mut().enumerate() {
            let g = self.geometry[p];
            let at = self.at(p, APRON + g.size - 1, 0);
            row[..g.width].copy_from_slice(&self.planes[p][at..at + g.width]);
        }
    }

    /// Copy the band's own rows, the picture's width of each, into
    /// macroblock row `mb_row` of `frame`.
    pub(crate) fn copy_into(&self, frame: &mut Frame, mb_row: usize) -> Option<()> {
        for (p, plane) in frame.planes.iter_mut().enumerate() {
            let g = self.geometry[p];
            for row in 0..g.size {
                let from = self.at(p, APRON + row, 0);
                let to = plane.at(0, mb_row * g.size + row);
                plane
                    .data
                    .get_mut(to..to + g.width)?
                    .copy_from_slice(self.planes[p].get(from..from + g.width)?);
            }
        }
        Some(())
    }
}
