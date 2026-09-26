//! Scaling a picture down to fit a box, shared by the decoders that scale by
//! averaging.
//!
//! A PNG streams its rows into a [`BoxFilter`] as they come out of the
//! decompressor, so a thumbnail never holds the picture at its own size; a GIF
//! composites its first frame and averages that. One averaging rule for both,
//! and one fitting rule ([`fit_within`]), so that the same picture scales to the
//! same pixels whichever file format it arrived in.

use alloc::vec;
use alloc::vec::Vec;

use crate::{Image, ImageError, ImageResult};

/// Where reconstructed pixels go.
pub(crate) trait Sink {
    /// The pixel at `(x, y)` of the picture is `argb`.
    fn put(&mut self, x: u32, y: u32, argb: u32);

    /// A row's worth at once: `argb[i]` is the pixel at
    /// `(x_start + i * x_step, y)` — a whole row of a plain picture, or one
    /// Adam7 pass's share of a row. The same as [`Sink::put`] for each
    /// pixel, which is what this does unless a sink can do better.
    fn put_row(&mut self, y: u32, x_start: u32, x_step: u32, argb: &[u32]) {
        let mut x = x_start;
        for &px in argb {
            self.put(x, y, px);
            x = x.saturating_add(x_step);
        }
    }
}

/// A picture no larger than a thumbnail, each cell the average of the source
/// pixels that fall in it.
///
/// Every source pixel is added to exactly one cell and each cell is divided
/// by its own count at the end. Counting per cell rather than assuming
/// `(w/dw) * (h/dh)` matters because the division is integer — with a
/// 100-wide source and a 30-wide destination the columns come out
/// 4,3,3,4,3..., and a fixed divisor would darken the wide ones.
///
/// Order does not matter to a sum, which is why interlaced files need no
/// special case: Adam7's passes arrive scattered across the picture, and each
/// pixel still lands in the one cell its position names.
///
/// **Colour is weighted by alpha.** A cell's alpha is the plain mean of its
/// pixels' alphas, but its colour is the mean of their colours each weighted
/// by how much of it shows: a fully transparent pixel's colour, which nobody
/// ever sees and is usually black, counts for nothing. Averaging the channels
/// separately, as this did until 2026-09-25, turned half a cell of red over
/// transparent black into dark red at half opacity, so every see-through edge
/// of a shrunken icon, PNG or GIF came out with a dark rim. For an opaque
/// picture the two rules are the same arithmetic and give the same bytes.
///
/// `gui/thumbs` averages again after a scaled decode (`Canvas::box_downscale`)
/// and must use the same rule, or a scaled decode and a decode-then-scale
/// disagree about the same picture:
/// `requests/f-c-average-translucent-pixels-by-their-alpha.md`.
pub(crate) struct BoxFilter {
    dest_w: u32,
    /// Which destination column each source column falls in.
    col_cell: Vec<u32>,
    /// Which destination row each source row falls in.
    row_cell: Vec<u32>,
    /// Per cell: the sum of alpha, then of red, green and blue each times its
    /// pixel's alpha. `u64` because a cell can cover the whole source, and 255
    /// squared times 24 million overflows a `u32` many times over.
    acc: Vec<[u64; 4]>,
    /// Per cell: how many source pixels went into it.
    counts: Vec<u64>,
}

impl BoxFilter {
    pub(crate) fn new(src_w: u32, src_h: u32, dest_w: u32, dest_h: u32) -> ImageResult<Self> {
        let cells = (dest_w as usize)
            .checked_mul(dest_h as usize)
            .ok_or(ImageError::Truncated)?;
        Ok(Self {
            dest_w,
            col_cell: cell_map(src_w, dest_w),
            row_cell: cell_map(src_h, dest_h),
            acc: vec![[0u64; 4]; cells],
            counts: vec![0u64; cells],
        })
    }

    /// Each cell's average, as `0xAARRGGBB`.
    pub(crate) fn finish(self) -> Vec<u32> {
        self.acc
            .iter()
            .zip(self.counts.iter())
            .map(|(cell, &n)| {
                let [alpha, red, green, blue] = *cell;
                // `checked_div` for the empty cell, which cannot happen with a
                // destination no larger than its source -- every cell receives
                // at least one pixel -- and would otherwise be a panic in a
                // decoder that reads files it did not write; and for a cell
                // nothing in which shows, whose colour is then no colour at
                // all. Both come out as a clear pixel.
                let byte = |sum: u64, over: u64| {
                    u32::try_from(sum.checked_div(over).unwrap_or(0))
                        .unwrap_or(255)
                        .min(255)
                };
                (byte(alpha, n) << 24)
                    | (byte(red, alpha) << 16)
                    | (byte(green, alpha) << 8)
                    | byte(blue, alpha)
            })
            .collect()
    }
}

impl Sink for BoxFilter {
    fn put(&mut self, x: u32, y: u32, argb: u32) {
        let (Some(&dx), Some(&dy)) = (self.col_cell.get(x as usize), self.row_cell.get(y as usize))
        else {
            return;
        };
        let at = (dy as usize)
            .checked_mul(self.dest_w as usize)
            .and_then(|row| row.checked_add(dx as usize));
        let Some(at) = at else {
            return;
        };
        if let (Some(cell), Some(n)) = (self.acc.get_mut(at), self.counts.get_mut(at)) {
            // Saturating throughout. The sums are bounded by 255 times the
            // pixel cap and cannot reach a u64 in practice, but a codec is
            // exactly the place where "in practice" is decided by whoever
            // supplies the file.
            let alpha = u64::from((argb >> 24) & 0xFF);
            let weighed = |shift: u32| u64::from((argb >> shift) & 0xFF).saturating_mul(alpha);
            cell[0] = cell[0].saturating_add(alpha);
            cell[1] = cell[1].saturating_add(weighed(16));
            cell[2] = cell[2].saturating_add(weighed(8));
            cell[3] = cell[3].saturating_add(weighed(0));
            *n = n.saturating_add(1);
        }
    }
}

/// For each of `src` source positions, the destination cell of `dest` it falls
/// in: `i * dest / src`, clamped so the last position cannot land one past the
/// end when the division is exact.
///
/// A table rather than a division per pixel, because `put` runs once for every
/// pixel of a picture that may have tens of millions of them.
fn cell_map(src: u32, dest: u32) -> Vec<u32> {
    (0..src)
        .map(|i| {
            let cell = u64::from(i)
                .saturating_mul(u64::from(dest))
                .checked_div(u64::from(src.max(1)))
                .unwrap_or(0)
                .min(u64::from(dest.saturating_sub(1)));
            u32::try_from(cell).unwrap_or(0)
        })
        .collect()
}

/// The largest `w x h` that fits in `max_w x max_h` with the aspect ratio of
/// `w x h`, never zero in either dimension.
///
/// A picture is never scaled *up*: a 16x16 icon asked to fit 128x128 stays
/// 16x16, because inventing pixels is not what a thumbnailer is for.
pub(crate) fn fit_within(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w <= max_w && h <= max_h {
        return (w.max(1), h.max(1));
    }
    // Saturating rather than plain: the product of two `u32`s fits a `u64`
    // with a hair to spare, but writing the proof in a comment and the
    // multiplication as if it needed none is how the next edit loses it.
    let by_w = u64::from(max_w).saturating_mul(u64::from(h));
    let by_h = u64::from(max_h).saturating_mul(u64::from(w));
    // Whichever bound binds first: comparing the cross-products avoids
    // floating point and its rounding.
    if by_w <= by_h {
        let dh = by_w.checked_div(u64::from(w.max(1))).unwrap_or(1).max(1);
        (max_w.max(1), u32::try_from(dh).unwrap_or(1))
    } else {
        let dw = by_h.checked_div(u64::from(h.max(1))).unwrap_or(1).max(1);
        (u32::try_from(dw).unwrap_or(1), max_h.max(1))
    }
}

/// `image` averaged down to fit `max_w` x `max_h`, or `image` itself if it
/// already fits.
///
/// For a decoder that has to reconstruct the whole picture anyway -- a GIF
/// frame is composited onto its canvas before it is a picture at all -- and so
/// has nothing to gain from streaming into the filter.
///
/// # Errors
///
/// Only [`ImageError::Truncated`] for a size that cannot be represented.
pub(crate) fn shrink_to_fit(image: Image, max_w: u32, max_h: u32) -> ImageResult<Image> {
    let (dw, dh) = fit_within(image.width, image.height, max_w, max_h);
    if (dw, dh) == (image.width, image.height) {
        return Ok(image);
    }
    let mut sink = BoxFilter::new(image.width, image.height, dw, dh)?;
    let width = image.width as usize;
    for (y, row) in image.pixels.chunks(width.max(1)).enumerate() {
        let y = u32::try_from(y).map_err(|_| ImageError::Truncated)?;
        for (x, &argb) in row.iter().enumerate() {
            let x = u32::try_from(x).map_err(|_| ImageError::Truncated)?;
            sink.put(x, y, argb);
        }
    }
    Ok(Image {
        width: dw,
        height: dh,
        pixels: sink.finish(),
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use alloc::vec;
    use alloc::vec::Vec;

    use super::*;

    fn shrink(width: u32, pixels: Vec<u32>, max: u32) -> Image {
        let height = u32::try_from(pixels.len()).unwrap() / width;
        shrink_to_fit(
            Image {
                width,
                height,
                pixels,
            },
            max,
            max,
        )
        .unwrap()
    }

    #[test]
    fn an_edge_over_transparency_keeps_its_colour_and_loses_only_opacity() {
        // Opaque red beside transparent black, averaged into one pixel: red at
        // half opacity -- not the dark red at half opacity that averaging the
        // channels separately makes, which is a dark rim round every icon.
        let out = shrink(2, vec![0xFFFF_0000, 0x0000_0000], 1);
        assert_eq!(out.pixels, vec![0x7FFF_0000]);
        // And a cell half of which is any colour at all, transparently.
        let out = shrink(2, vec![0xFF20_40C0, 0x00FF_FFFF], 1);
        assert_eq!(out.pixels, vec![0x7F20_40C0]);
    }

    #[test]
    fn a_cell_nothing_in_which_shows_is_clear() {
        let out = shrink(2, vec![0x00FF_0000, 0x0000_FF00], 1);
        assert_eq!(out.pixels, vec![0]);
    }

    #[test]
    fn opaque_pictures_average_exactly_as_the_channels_do() {
        // Weighting by an alpha of 255 everywhere is the same arithmetic as not
        // weighting: an opaque picture's thumbnail is unchanged by the rule.
        let mut seed = 0x2545_F491u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let pixels: Vec<u32> = (0..97 * 61)
            .map(|_| 0xFF00_0000 | (next() & 0x00FF_FFFF))
            .collect();
        let out = shrink(97, pixels.clone(), 20);
        let (dw, dh) = (out.width as usize, out.height as usize);
        let cells_x = cell_map(97, out.width);
        let cells_y = cell_map(61, out.height);
        let mut sums = vec![[0u64; 4]; dw * dh];
        for (i, &p) in pixels.iter().enumerate() {
            let (x, y) = (i % 97, i / 97);
            let cell = &mut sums[cells_y[y] as usize * dw + cells_x[x] as usize];
            cell[0] += 1;
            for (k, shift) in [16u32, 8, 0].into_iter().enumerate() {
                cell[k + 1] += u64::from((p >> shift) & 0xFF);
            }
        }
        for (got, cell) in out.pixels.iter().zip(&sums) {
            let n = cell[0];
            let want = 0xFF00_0000
                | ((cell[1] / n) as u32) << 16
                | ((cell[2] / n) as u32) << 8
                | (cell[3] / n) as u32;
            assert_eq!(*got, want);
        }
    }
}
