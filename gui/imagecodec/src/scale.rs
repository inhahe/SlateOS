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
/// Alpha is averaged with the colour rather than premultiplied first. That is
/// the same thing `box_filter_downscale` in the thumbnailer does, and matching
/// it is deliberate: two averaging rules would make a scaled decode and a
/// decode-then-scale disagree about the same picture.
pub(crate) struct BoxFilter {
    dest_w: u32,
    /// Which destination column each source column falls in.
    col_cell: Vec<u32>,
    /// Which destination row each source row falls in.
    row_cell: Vec<u32>,
    /// Per cell: the sums of alpha, red, green and blue. `u64` because a cell
    /// can cover the whole source, and 255 times 24 million overflows a `u32`.
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
                // `checked_div` for the empty cell, which cannot happen with a
                // destination no larger than its source — every cell receives
                // at least one pixel — and would otherwise be a panic in a
                // decoder that reads files it did not write. It comes out as a
                // clear pixel.
                let mean = |v: u64| {
                    u32::try_from(v.checked_div(n).unwrap_or(0))
                        .unwrap_or(255)
                        .min(255)
                };
                (mean(cell[0]) << 24) | (mean(cell[1]) << 16) | (mean(cell[2]) << 8) | mean(cell[3])
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
            cell[0] = cell[0].saturating_add(u64::from((argb >> 24) & 0xFF));
            cell[1] = cell[1].saturating_add(u64::from((argb >> 16) & 0xFF));
            cell[2] = cell[2].saturating_add(u64::from((argb >> 8) & 0xFF));
            cell[3] = cell[3].saturating_add(u64::from(argb & 0xFF));
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
