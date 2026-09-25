//! Bringing each component's plane up to the picture's resolution, the way
//! libjpeg does it.
//!
//! Most JPEGs keep their colour at half resolution -- across (4:2:2), or
//! across and down (4:2:0) -- because the eye resolves colour far more
//! coarsely than brightness. Restoring it is left to the decoder, and the
//! decoder everything is compared against, libjpeg-turbo, underneath every
//! mainstream browser, image library and desktop, uses libjpeg's "fancy"
//! triangle filter: each output sample is three quarters the input sample it
//! lies nearer and one quarter the next one out, in each direction the plane
//! was halved. Repeating each sample instead, as this crate once did, is a
//! different picture wherever colour changes sharply: the rim of a red disc on
//! a green field grows a staircase fringe, up to a hundred levels from what
//! every other decoder shows.
//!
//! Transcribed from libjpeg-turbo's `jdsample.c`, down to its rounding. The
//! two outputs of each input sample round with different biases -- `+1` and
//! `+2` across, `+8` and `+7` both ways -- so that neither of them is rounded
//! up consistently more than the other. At the edges the outermost sample
//! stands in for its missing neighbour, which is what libjpeg's special-cased
//! first and last columns and its duplicated context rows come to. And the
//! filter reads only the plane's real samples (libjpeg's `downsampled_width`
//! and `downsampled_height`), never the padding that fills out the last MCU,
//! which is decoded data from outside the picture.
//!
//! Which filter a plane gets is decided the way `jinit_upsampler` decides it,
//! so a plane libjpeg repeats is repeated here too: one only one or two
//! samples wide, which the across filter has no interior for; any ratio but
//! two; and every plane of an eighth-scale decode, which libjpeg leaves
//! unfiltered.

use alloc::vec;
use alloc::vec::Vec;

/// Where one component's plane lies: its padded size, and how much of it is
/// picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shape {
    /// Samples per row: every block the component has, whole, so a block is
    /// never clipped while it is written (libjpeg's `width_in_blocks *
    /// DCT_scaled_size`).
    pub(super) stride: usize,
    /// Rows, likewise: whole iMCU rows.
    pub(super) rows: usize,
    /// How many samples of each row are picture rather than padding:
    /// libjpeg's `downsampled_width`, and the filter's right edge.
    pub(super) width: usize,
    /// How many rows are picture: the filter's bottom edge.
    pub(super) height: usize,
    /// The plane's samples across an MCU against the picture's, as a ratio
    /// not necessarily in lowest terms: `(8, 16)` for colour halved across at
    /// full size. libjpeg's `h_in_group` and `h_out_group`, both multiplied by
    /// the picture's block size so that they stay whole.
    pub(super) across: (usize, usize),
    /// The same, down.
    pub(super) down: (usize, usize),
}


/// One component's reconstructed samples.
///
/// A plane grows as the decoder reconstructs it, a whole iMCU row at a time,
/// so a decode that is asked for only its first rows -- a TIFF strip whose
/// JPEG claims more rows than the strip has -- holds only those.
pub(super) struct Samples {
    pub(super) shape: Shape,
    /// Up to `stride * rows` samples, row by row.
    pub(super) data: Vec<u8>,
}

impl Samples {
    /// A plane of `shape` with no rows reconstructed yet.
    pub(super) const fn new(shape: Shape) -> Self {
        Self {
            shape,
            data: Vec::new(),
        }
    }

    /// Make room for the first `rows` rows, zero until written over.
    pub(super) fn grow_to(&mut self, rows: usize) {
        let len = rows.min(self.shape.rows).saturating_mul(self.shape.stride);
        if len > self.data.len() {
            self.data.resize(len, 0);
        }
    }

    /// The picture's samples in row `r`, without the padding: as many of them
    /// as the plane holds, which is all of them unless a truncated file cut it
    /// short, and none past the bottom.
    fn line(&self, r: usize) -> &[u8] {
        if r >= self.shape.rows {
            return &[];
        }
        let start = r.saturating_mul(self.shape.stride);
        let rest = self.data.get(start..).unwrap_or(&[]);
        let len = self.shape.width.min(self.shape.stride).min(rest.len());
        rest.get(..len).unwrap_or(&[])
    }

    /// Row `r` whole, padding included; empty past the bottom.
    fn padded_line(&self, r: usize) -> &[u8] {
        let start = r.saturating_mul(self.shape.stride);
        self.data
            .get(start..start.saturating_add(self.shape.stride))
            .unwrap_or(&[])
    }
}

/// Which of libjpeg's upsamplers a plane gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    /// Already at the picture's resolution (`fullsize_upsample`).
    Same,
    /// Halved across: the triangle filter along each row
    /// (`h2v1_fancy_upsample`).
    Across,
    /// Halved down: the filter between rows (`h1v2_fancy_upsample`).
    Down,
    /// Halved both ways: the filter in each direction, so each output is
    /// 9/16, 3/16, 3/16 and 1/16 of four inputs (`h2v2_fancy_upsample`).
    Both,
    /// Anything else: each sample repeated (`int_upsample`, and the plain
    /// `h2v1_upsample` and `h2v2_upsample`). A ratio that is not a whole
    /// number, which libjpeg refuses, takes the nearest sample.
    Repeat,
}

impl Filter {
    /// The choice `jinit_upsampler` makes for a plane of `shape`. `fancy` is
    /// whether the decode filters at all: libjpeg does not at eighth scale.
    fn choose(shape: &Shape, fancy: bool) -> Self {
        let (h_in, h_out) = shape.across;
        let (v_in, v_out) = shape.down;
        let halved_across = h_in.saturating_mul(2) == h_out;
        let halved_down = v_in.saturating_mul(2) == v_out;
        // The across filters special-case the first and the last sample of a
        // row, and need at least one more between them.
        let wide = shape.width > 2;
        if h_in == h_out && v_in == v_out {
            Self::Same
        } else if fancy && halved_across && v_in == v_out && wide {
            Self::Across
        } else if fancy && h_in == h_out && halved_down {
            Self::Down
        } else if fancy && halved_across && halved_down && wide {
            Self::Both
        } else {
            Self::Repeat
        }
    }
}

/// One plane's rows at the picture's resolution, handed out one at a time.
pub(super) struct Rows {
    filter: Filter,
    /// The picture's width: how many samples every row handed out has.
    width: usize,
    /// The row being handed out, for every filter but [`Filter::Same`] (and
    /// for that one too if its plane is somehow short).
    row: Vec<u8>,
    /// [`Filter::Both`]'s column sums, `3 * nearer + further`, one per input
    /// sample.
    sums: Vec<u16>,
    /// [`Filter::Repeat`]'s input column for each output column, worked out
    /// once rather than divided for per pixel.
    columns: Vec<usize>,
}

impl Rows {
    /// Rows `width` samples long from a plane of `shape`, filtered if `fancy`.
    pub(super) fn new(shape: &Shape, width: usize, fancy: bool) -> Self {
        let filter = Filter::choose(shape, fancy);
        let input = shape.width;
        let row_len = match filter {
            Filter::Across | Filter::Both => width.max(input.saturating_mul(2)),
            Filter::Same | Filter::Down | Filter::Repeat => width.max(input),
        };
        let (h_in, h_out) = shape.across;
        let columns = if filter == Filter::Repeat {
            (0..width)
                .map(|x| x.saturating_mul(h_in).checked_div(h_out).unwrap_or(0))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            filter,
            width,
            row: vec![0u8; row_len],
            sums: if filter == Filter::Both {
                vec![0u16; input]
            } else {
                Vec::new()
            },
            columns,
        }
    }

    /// Output row `y` of `samples`, exactly `width` samples; zeros where the
    /// plane has nothing, which only a plane not yet reconstructed that far
    /// can.
    pub(super) fn row<'s>(&'s mut self, samples: &'s Samples, y: usize) -> &'s [u8] {
        let shape = &samples.shape;
        // The two input rows a halved-down plane filters between: the one
        // output row `y` lies nearer, and the one beyond it -- above for the
        // upper of the pair, below for the lower -- with the outermost row
        // standing in past either edge, as libjpeg's context rows do.
        let last = shape.height.saturating_sub(1);
        let near = (y / 2).min(last);
        let (far, above) = if y.is_multiple_of(2) {
            (near.saturating_sub(1), true)
        } else {
            (near.saturating_add(1).min(last), false)
        };
        match self.filter {
            Filter::Same => {
                let line = samples.line(y);
                if line.len() >= self.width {
                    return line.get(..self.width).unwrap_or(line);
                }
                // A plane shorter than the picture: what there is, then zeros.
                fill(&mut self.row, line);
            }
            Filter::Across => across(samples.line(y), &mut self.row),
            Filter::Down => down(
                samples.line(near),
                samples.line(far),
                if above { 1 } else { 2 },
                &mut self.row,
            ),
            Filter::Both => {
                for ((sum, &n), &f) in self
                    .sums
                    .iter_mut()
                    .zip(samples.line(near))
                    .zip(samples.line(far))
                {
                    *sum = column_sum(n, f);
                }
                both(&self.sums, &mut self.row);
            }
            Filter::Repeat => {
                let (v_in, v_out) = shape.down;
                let line =
                    samples.padded_line(y.saturating_mul(v_in).checked_div(v_out).unwrap_or(0));
                for (slot, &column) in self.row.iter_mut().zip(&self.columns) {
                    *slot = line.get(column).copied().unwrap_or(0);
                }
            }
        }
        self.row.get(..self.width).unwrap_or(&self.row)
    }
}

/// `row` as `from`, then zeros.
fn fill(row: &mut [u8], from: &[u8]) {
    let mut from = from.iter();
    for slot in row {
        *slot = from.next().copied().unwrap_or(0);
    }
}

/// `3 * nearer + further`: one column of the halved-both-ways filter's
/// vertical pass, at most 1020.
#[allow(clippy::arithmetic_side_effects, reason = "at most 4 * 255")]
fn column_sum(nearer: u8, further: u8) -> u16 {
    3 * u16::from(nearer) + u16::from(further)
}

/// `f(before, here, after)` for each input sample, into that sample's pair of
/// outputs, with the outermost sample standing in for the neighbour past each
/// end: the edge rule of both across filters.
///
/// The first and last samples are done on their own and the rest as three
/// shifted slices zipped together, which compiles to a straight loop. An
/// iterator that asked at every sample whether it was at an edge cost a 4:2:2
/// decode a tenth of its time.
#[inline]
fn in_pairs<T: Copy>(input: &[T], out: &mut [u8], f: impl Fn(T, T, T) -> [u8; 2]) {
    let (Some(&first), Some(&last)) = (input.first(), input.last()) else {
        return;
    };
    let Some((head, rest)) = out.as_chunks_mut::<2>().0.split_first_mut() else {
        return;
    };
    *head = f(first, first, input.get(1).copied().unwrap_or(first));
    // One sample has no other; its pair is done.
    let Some(&before_last) = input.len().checked_sub(2).and_then(|at| input.get(at)) else {
        return;
    };
    let interior = input.len().saturating_sub(2);
    let (middle, tail) = match rest.split_at_mut_checked(interior) {
        Some(split) => split,
        // Room for fewer pairs than there are samples: fill what there is.
        None => (rest, <&mut [[u8; 2]]>::default()),
    };
    let triples = input
        .iter()
        .zip(input.get(1..).unwrap_or(&[]))
        .zip(input.get(2..).unwrap_or(&[]));
    for (slot, ((&before, &here), &after)) in middle.iter_mut().zip(triples) {
        *slot = f(before, here, after);
    }
    if let Some(slot) = tail.first_mut() {
        *slot = f(before_last, last, last);
    }
}

/// `h2v1_fancy_upsample`: two outputs per input, each three quarters the
/// input and a quarter its neighbour on that side, the left one rounded with
/// `+1` and the right with `+2`.
// Each value is at most 4 * 255 + 2 before the shift, so the arithmetic cannot
// overflow a u16, and after the shift it fits the byte it is cast to.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "at most (4 * 255 + 2) >> 2"
)]
fn across(input: &[u8], out: &mut [u8]) {
    in_pairs(input, out, |before, here, after| {
        let near = 3 * u16::from(here);
        [
            ((near + u16::from(before) + 1) >> 2) as u8,
            ((near + u16::from(after) + 2) >> 2) as u8,
        ]
    });
}

/// `h1v2_fancy_upsample`: one output per input, three quarters the nearer
/// row and a quarter the further, rounded with `+1` when the further row is
/// above and `+2` when it is below.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "at most (4 * 255 + 2) >> 2"
)]
fn down(nearer: &[u8], further: &[u8], bias: u16, out: &mut [u8]) {
    for ((slot, &n), &f) in out.iter_mut().zip(nearer).zip(further) {
        *slot = ((3 * u16::from(n) + u16::from(f) + bias) >> 2) as u8;
    }
}

/// `h2v2_fancy_upsample`'s horizontal pass over the column sums: two outputs
/// per input column, the left rounded with `+8` and the right with `+7`.
#[allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "at most (16 * 255 + 8) >> 4"
)]
fn both(sums: &[u16], out: &mut [u8]) {
    in_pairs(sums, out, |before, here, after| {
        let near = 3 * here;
        [
            ((near + before + 8) >> 4) as u8,
            ((near + after + 7) >> 4) as u8,
        ]
    });
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    /// A plane of the given ratios holding `picture` (row by row, `width`
    /// samples a row), padded out by `pad` samples of `fill` on the right and
    /// `pad` rows of it below.
    fn plane(
        picture: &[u8],
        (width, height): (usize, usize),
        (across, down): ((usize, usize), (usize, usize)),
        pad: usize,
        filler: u8,
    ) -> Samples {
        let shape = Shape {
            stride: width + pad,
            rows: height + pad,
            width,
            height,
            across,
            down,
        };
        let mut data = vec![filler; shape.stride * shape.rows];
        for (r, line) in picture.chunks(width).enumerate() {
            data[r * shape.stride..r * shape.stride + width].copy_from_slice(line);
        }
        Samples { shape, data }
    }

    /// Every output row of `samples` at `out_w` x `out_h`.
    fn upsample(samples: &Samples, (out_w, out_h): (usize, usize), fancy: bool) -> Vec<Vec<u8>> {
        let mut rows = Rows::new(&samples.shape, out_w, fancy);
        (0..out_h).map(|y| rows.row(samples, y).to_vec()).collect()
    }

    /// libjpeg-turbo's `h2v1_fancy_upsample`, `h1v2_fancy_upsample` and
    /// `h2v2_fancy_upsample`, transcribed as they are written -- a pointer
    /// walk with the first and last columns special-cased, and context rows
    /// from pointer lists that repeat the edge rows -- rather than as the
    /// edge-repeating neighbour sums the module uses, so the two agreeing is
    /// evidence rather than one formula checked against itself.
    mod libjpeg {
        use alloc::vec;
        use alloc::vec::Vec;

        pub fn h2v1(input: &[u8]) -> Vec<u8> {
            let w = input.len();
            let mut out = Vec::with_capacity(2 * w);
            let mut p = 0usize;
            let mut invalue = i32::from(input[p]);
            p += 1;
            out.push(invalue as u8);
            out.push(((invalue * 3 + i32::from(input[p]) + 2) >> 2) as u8);
            for _ in 0..w - 2 {
                invalue = i32::from(input[p]) * 3;
                p += 1;
                out.push(((invalue + i32::from(input[p - 2]) + 1) >> 2) as u8);
                out.push(((invalue + i32::from(input[p]) + 2) >> 2) as u8);
            }
            invalue = i32::from(input[p]);
            out.push(((invalue * 3 + i32::from(input[p - 1]) + 1) >> 2) as u8);
            out.push(invalue as u8);
            out
        }

        /// The row list libjpeg's main controller hands the upsampler: the
        /// real rows with the first repeated above and the last below.
        fn context(rows: &[Vec<u8>]) -> Vec<&[u8]> {
            let mut list = vec![rows[0].as_slice()];
            list.extend(rows.iter().map(Vec::as_slice));
            list.push(rows[rows.len() - 1].as_slice());
            list
        }

        pub fn h1v2(rows: &[Vec<u8>]) -> Vec<Vec<u8>> {
            let list = context(rows);
            let mut out = Vec::new();
            for inrow in 1..=rows.len() {
                for v in 0..2 {
                    let inptr0 = list[inrow];
                    let (inptr1, bias) = if v == 0 {
                        (list[inrow - 1], 1)
                    } else {
                        (list[inrow + 1], 2)
                    };
                    out.push(
                        inptr0
                            .iter()
                            .zip(inptr1)
                            .map(|(&a, &b)| ((i32::from(a) * 3 + i32::from(b) + bias) >> 2) as u8)
                            .collect(),
                    );
                }
            }
            out
        }

        pub fn h2v2(rows: &[Vec<u8>]) -> Vec<Vec<u8>> {
            let list = context(rows);
            let w = rows[0].len();
            let mut out = Vec::new();
            for inrow in 1..=rows.len() {
                for v in 0..2 {
                    let inptr0 = list[inrow];
                    let inptr1 = if v == 0 {
                        list[inrow - 1]
                    } else {
                        list[inrow + 1]
                    };
                    let mut row = Vec::with_capacity(2 * w);
                    let (mut p0, mut p1) = (0usize, 0usize);
                    let mut thiscolsum = i32::from(inptr0[p0]) * 3 + i32::from(inptr1[p1]);
                    p0 += 1;
                    p1 += 1;
                    let mut nextcolsum = i32::from(inptr0[p0]) * 3 + i32::from(inptr1[p1]);
                    p0 += 1;
                    p1 += 1;
                    row.push(((thiscolsum * 4 + 8) >> 4) as u8);
                    row.push(((thiscolsum * 3 + nextcolsum + 7) >> 4) as u8);
                    let mut lastcolsum = thiscolsum;
                    thiscolsum = nextcolsum;
                    for _ in 0..w - 2 {
                        nextcolsum = i32::from(inptr0[p0]) * 3 + i32::from(inptr1[p1]);
                        p0 += 1;
                        p1 += 1;
                        row.push(((thiscolsum * 3 + lastcolsum + 8) >> 4) as u8);
                        row.push(((thiscolsum * 3 + nextcolsum + 7) >> 4) as u8);
                        lastcolsum = thiscolsum;
                        thiscolsum = nextcolsum;
                    }
                    row.push(((thiscolsum * 3 + lastcolsum + 8) >> 4) as u8);
                    row.push(((thiscolsum * 4 + 7) >> 4) as u8);
                    out.push(row);
                }
            }
            out
        }
    }

    /// A xorshift generator: the same planes every run, without a dependency.
    struct Rng(u64);
    impl Rng {
        fn byte(&mut self) -> u8 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 24) as u8
        }
    }

    fn random(rng: &mut Rng, width: usize, height: usize) -> Vec<Vec<u8>> {
        (0..height)
            .map(|_| (0..width).map(|_| rng.byte()).collect())
            .collect()
    }

    const ACROSS: ((usize, usize), (usize, usize)) = ((1, 2), (1, 1));
    const DOWN: ((usize, usize), (usize, usize)) = ((1, 1), (1, 2));
    const BOTH: ((usize, usize), (usize, usize)) = ((1, 2), (1, 2));

    #[test]
    fn every_filter_is_libjpegs_on_every_size_of_plane() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for width in 3..=19 {
            for height in 1..=7 {
                let rows = random(&mut rng, width, height);
                let flat: Vec<u8> = rows.concat();
                // Garbage in the padding: the filter must never read it.
                let garbage = rng.byte();

                let got = upsample(
                    &plane(&flat, (width, height), ACROSS, 3, garbage),
                    (2 * width, height),
                    true,
                );
                let want: Vec<Vec<u8>> = rows.iter().map(|r| libjpeg::h2v1(r)).collect();
                assert_eq!(got, want, "across, {width}x{height}");

                let got = upsample(
                    &plane(&flat, (width, height), DOWN, 3, garbage),
                    (width, 2 * height),
                    true,
                );
                assert_eq!(got, libjpeg::h1v2(&rows), "down, {width}x{height}");

                let got = upsample(
                    &plane(&flat, (width, height), BOTH, 3, garbage),
                    (2 * width, 2 * height),
                    true,
                );
                assert_eq!(got, libjpeg::h2v2(&rows), "both, {width}x{height}");
            }
        }
    }

    #[test]
    fn an_odd_sized_picture_takes_the_first_of_the_last_pair() {
        // A 4:2:0 picture 7 wide and 5 high has a 4x3 colour plane, whose
        // filtered 8x6 rows are cut to 7x5: the filter runs over the whole
        // plane and the picture keeps what lies over it.
        let mut rng = Rng(7);
        let rows = random(&mut rng, 4, 3);
        let got = upsample(&plane(&rows.concat(), (4, 3), BOTH, 5, 0xAA), (7, 5), true);
        let want: Vec<Vec<u8>> = libjpeg::h2v2(&rows)
            .into_iter()
            .take(5)
            .map(|r| r[..7].to_vec())
            .collect();
        assert_eq!(got, want);
    }

    #[test]
    fn a_flat_plane_stays_flat_under_every_filter() {
        // The alternating biases exist so that rounding does not drift a flat
        // colour up or down; any bias wrong by one shows here as a level.
        for value in [0u8, 1, 2, 3, 127, 128, 254, 255] {
            for ratios in [ACROSS, DOWN, BOTH] {
                let picture = vec![value; 5 * 4];
                let out = (
                    if ratios.0 == (1, 2) { 10 } else { 5 },
                    if ratios.1 == (1, 2) { 8 } else { 4 },
                );
                let rows = upsample(&plane(&picture, (5, 4), ratios, 2, 0x55), out, true);
                assert_eq!(rows.len(), out.1);
                for row in &rows {
                    assert_eq!(row.len(), out.0);
                    assert!(
                        row.iter().all(|&s| s == value),
                        "{ratios:?} at {value}: {row:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_filters_weigh_the_nearer_sample_three_to_one() {
        // By hand, from the weights: across, [0, 100, 200] becomes
        //   0, (0*3 + 100 + 2) >> 2 = 25, (300 + 0 + 1) >> 2 = 75,
        //   (300 + 200 + 2) >> 2 = 125, (600 + 100 + 1) >> 2 = 175, 200.
        let got = upsample(&plane(&[0, 100, 200], (3, 1), ACROSS, 0, 0), (6, 1), true);
        assert_eq!(got, vec![vec![0, 25, 75, 125, 175, 200]]);
        // Down, rows [0] and [100]: the upper output of each is pulled toward
        // the row above and the lower toward the row below.
        //   row 0: 0 above 0 -> 0; 0 then 100 below -> (0 + 100 + 2) >> 2 = 25
        //   row 1: 100 then 0 above -> (300 + 0 + 1) >> 2 = 75; 100 below 100 -> 100
        let got = upsample(&plane(&[0, 100], (1, 2), DOWN, 0, 0), (1, 4), true);
        assert_eq!(got, vec![vec![0], vec![25], vec![75], vec![100]]);
    }

    #[test]
    fn a_plane_too_narrow_to_filter_is_repeated_as_libjpeg_does() {
        // One or two samples across leave the across filters no interior, and
        // libjpeg repeats them instead (`downsampled_width > 2`).
        for ratios in [ACROSS, BOTH] {
            for width in [1usize, 2] {
                let picture: Vec<u8> = (0..width * 2).map(|i| (i * 60) as u8).collect();
                let samples = plane(&picture, (width, 2), ratios, 1, 0xEE);
                assert_eq!(Filter::choose(&samples.shape, true), Filter::Repeat);
                let rows = upsample(
                    &samples,
                    (2 * width, if ratios == BOTH { 4 } else { 2 }),
                    true,
                );
                for (y, row) in rows.iter().enumerate() {
                    let source = if ratios == BOTH { y / 2 } else { y };
                    for (x, &s) in row.iter().enumerate() {
                        assert_eq!(
                            s,
                            picture[source * width + x / 2],
                            "{ratios:?} {width}: ({x}, {y})"
                        );
                    }
                }
            }
        }
        // Down has no across filter, so no minimum width.
        let samples = plane(&[10, 20], (1, 2), DOWN, 0, 0);
        assert_eq!(Filter::choose(&samples.shape, true), Filter::Down);
    }

    #[test]
    fn each_layout_gets_the_filter_libjpeg_gives_it() {
        let shape = |across, down| Shape {
            stride: 16,
            rows: 16,
            width: 8,
            height: 8,
            across,
            down,
        };
        let cases = [
            ((1, 1), (1, 1), Filter::Same),   // 4:4:4, or luma anywhere
            ((2, 2), (1, 1), Filter::Same),   // luma of 4:2:2
            ((1, 2), (1, 1), Filter::Across), // chroma of 4:2:2
            ((1, 1), (1, 2), Filter::Down),   // chroma of 4:4:0
            ((1, 2), (1, 2), Filter::Both),   // chroma of 4:2:0
            ((2, 4), (1, 1), Filter::Across), // a 2:1 ratio in any factors
            ((1, 4), (1, 1), Filter::Repeat), // chroma of 4:1:1
            ((1, 2), (1, 4), Filter::Repeat), // halved across, quartered down
            ((2, 3), (1, 1), Filter::Repeat), // not a whole ratio at all
        ];
        for (across, down, want) in cases {
            assert_eq!(
                Filter::choose(&shape(across, down), true),
                want,
                "{across:?} {down:?}"
            );
        }
        // At eighth scale libjpeg filters nothing.
        for (across, down, _) in cases {
            let got = Filter::choose(&shape(across, down), false);
            let want = if across.0 == across.1 && down.0 == down.1 {
                Filter::Same
            } else {
                Filter::Repeat
            };
            assert_eq!(got, want, "{across:?} {down:?} unfiltered");
        }
    }

    #[test]
    fn the_pair_walk_repeats_the_edge_samples_at_every_length() {
        // `in_pairs` does the first and last samples on their own; one and two
        // samples are where those cases meet, and a row with room for fewer
        // pairs than there are samples is filled as far as it goes. The filter
        // choice never sends it fewer than three, which is why this asks it
        // directly.
        let gather = |input: &[u8], room: usize| {
            let mut out = vec![0u8; room * 2];
            in_pairs(input, &mut out, |before, here, after| {
                [before + here, here + after]
            });
            out
        };
        assert_eq!(gather(&[5], 1), vec![10, 10]);
        assert_eq!(gather(&[5, 7], 2), vec![10, 12, 12, 14]);
        assert_eq!(gather(&[5, 7, 9], 3), vec![10, 12, 12, 16, 16, 18]);
        assert_eq!(
            gather(&[5, 7, 9, 11], 4),
            vec![10, 12, 12, 16, 16, 20, 20, 22]
        );
        assert_eq!(gather(&[5, 7, 9], 2), vec![10, 12, 12, 16]);
        assert_eq!(gather(&[], 2), vec![0, 0, 0, 0]);
        assert_eq!(gather(&[5, 7, 9], 0), Vec::<u8>::new());
    }

    #[test]
    fn a_repeated_plane_is_each_sample_repeated() {
        // 4:1:1: every chroma sample stands for four luma columns.
        let picture: Vec<u8> = (0..3u8).map(|i| i * 50).collect();
        let samples = plane(&picture, (3, 1), ((1, 4), (1, 1)), 1, 0xEE);
        let rows = upsample(&samples, (11, 1), true);
        assert_eq!(rows, vec![vec![0, 0, 0, 0, 50, 50, 50, 50, 100, 100, 100]]);
    }

    #[test]
    fn a_plane_shorter_than_its_shape_gives_what_it_has_then_zeros() {
        // Not something a decode hands out -- a plane is grown to cover every
        // row before any output row that reads it -- but the rows are read
        // through `get` all the same, and a plane that fell short must cost a
        // picture its missing samples, not panic.
        let samples = Samples {
            shape: Shape {
                stride: 4,
                rows: 2,
                width: 4,
                height: 2,
                across: (1, 1),
                down: (1, 1),
            },
            data: vec![9, 9],
        };
        let rows = upsample(&samples, (4, 3), true);
        assert_eq!(
            rows,
            vec![vec![9, 9, 0, 0], vec![0, 0, 0, 0], vec![0, 0, 0, 0]]
        );
    }

    #[test]
    fn a_plane_grows_by_whole_rows_up_to_its_shape() {
        let mut samples = Samples::new(Shape {
            stride: 3,
            rows: 4,
            width: 3,
            height: 4,
            across: (1, 1),
            down: (1, 1),
        });
        assert!(samples.data.is_empty());
        samples.grow_to(2);
        assert_eq!(samples.data.len(), 6);
        samples.grow_to(1);
        assert_eq!(samples.data.len(), 6);
        samples.grow_to(9);
        assert_eq!(samples.data.len(), 12);
    }
}
