//! WebP's `ALPH` chunk: the alpha plane of a lossy picture (RFC 9649 §2.7.1.2
//! of the container, "Alpha").
//!
//! VP8 has no alpha, so an extended WebP carries it beside the frame: one byte
//! of header, then a plane of one byte per pixel, stored raw or as a lossless
//! image stream whose green channel is the alpha. Either way the plane may
//! first have been through a predictive filter -- each value stored as its
//! difference from its left, upper, or gradient-predicted neighbour -- which
//! decoding undoes row by row.
//!
//! Decoded as libwebp decodes it (`src/dec/alpha_dec.c`,
//! `src/dsp/filters.c`), with its refusals: an unknown compression method, an
//! unknown preprocessing, or reserved bits set. The "level reduction"
//! preprocessing is only a note that the encoder quantised the plane; libwebp
//! does nothing with it unless asked to dither, which its default -- and every
//! browser's -- does not.

use alloc::vec::Vec;

use crate::{ImageError, ImageResult, Limits};

/// The predictive filters (RFC 9649, "Filtering method").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filter {
    None,
    Horizontal,
    Vertical,
    Gradient,
}

/// Decode an `ALPH` chunk's plane for a `width` x `height` picture.
///
/// # Errors
///
/// [`ImageError::Truncated`] for a plane shorter than the picture;
/// [`ImageError::Malformed`] for a header libwebp refuses; and whatever the
/// lossless decoder reports for a compressed plane.
pub(super) fn decode(
    chunk: &[u8],
    width: u32,
    height: u32,
    limits: Limits,
) -> ImageResult<Vec<u8>> {
    let (&header, data) = chunk.split_first().ok_or(ImageError::Truncated)?;
    if data.is_empty() {
        return Err(ImageError::Truncated);
    }
    let method = header & 0x03;
    let filter = match (header >> 2) & 0x03 {
        0 => Filter::None,
        1 => Filter::Horizontal,
        2 => Filter::Vertical,
        _ => Filter::Gradient,
    };
    let preprocessing = (header >> 4) & 0x03;
    let reserved = header >> 6;
    if method > 1 {
        return Err(ImageError::Malformed(
            "an alpha plane of an unknown compression method",
        ));
    }
    if preprocessing > 1 {
        return Err(ImageError::Malformed(
            "an alpha plane of an unknown preprocessing",
        ));
    }
    if reserved != 0 {
        return Err(ImageError::Malformed(
            "an alpha plane with its reserved bits set",
        ));
    }
    let (w, h) = (width as usize, height as usize);
    let count = w.saturating_mul(h);
    let mut plane: Vec<u8> = if method == 0 {
        data.get(..count).ok_or(ImageError::Truncated)?.to_vec()
    } else {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the green channel of 0xAARRGGBB, shifted down and masked by the cast"
        )]
        let green = |pixel: u32| (pixel >> 8) as u8;
        super::lossless::decode_alpha(data, width, height, limits)?
            .into_iter()
            .map(green)
            .collect()
    };
    unfilter(filter, &mut plane, w);
    Ok(plane)
}

/// Undo `filter` over `plane`, rows `width` long, in place.
///
/// Every filter predicts the first row as horizontal does, from the left with
/// zero before the first sample; later rows predict their first sample from
/// the one above it.
#[allow(
    clippy::arithmetic_side_effects,
    reason = "the gradient is a sum of three bytes in sixteen bits"
)]
fn unfilter(filter: Filter, plane: &mut [u8], width: usize) {
    if filter == Filter::None || width == 0 {
        return;
    }
    let mut rows = plane.chunks_exact_mut(width);
    let Some(first) = rows.next() else {
        return;
    };
    let mut left = 0u8;
    for sample in first.iter_mut() {
        *sample = sample.wrapping_add(left);
        left = *sample;
    }
    let mut above: &mut [u8] = first;
    for row in rows {
        match filter {
            Filter::Horizontal => {
                let mut left = above.first().copied().unwrap_or(0);
                for sample in row.iter_mut() {
                    *sample = sample.wrapping_add(left);
                    left = *sample;
                }
            }
            Filter::Vertical => {
                for (sample, &up) in row.iter_mut().zip(above.iter()) {
                    *sample = sample.wrapping_add(up);
                }
            }
            Filter::Gradient | Filter::None => {
                // The first sample: left, above and above-left are all the
                // sample above, so the gradient predicts exactly it.
                let start = above.first().copied().unwrap_or(0);
                let (mut left, mut above_left) = (start, start);
                for (sample, &up) in row.iter_mut().zip(above.iter()) {
                    let predicted =
                        (i16::from(left) + i16::from(up) - i16::from(above_left)).clamp(0, 255);
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "clamped to 0..=255"
                    )]
                    let predicted = predicted as u8;
                    *sample = sample.wrapping_add(predicted);
                    left = *sample;
                    above_left = up;
                }
            }
        }
        above = row;
    }
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

    /// The forward filters, from RFC 9649's description, to round-trip.
    fn filtered(filter: Filter, plane: &[u8], width: usize) -> Vec<u8> {
        let height = plane.len() / width;
        let at = |x: usize, y: usize| plane[y * width + x];
        let mut out = vec![0u8; plane.len()];
        for y in 0..height {
            for x in 0..width {
                let predicted = match (filter, x, y) {
                    (Filter::None, ..) => 0,
                    (_, 0, 0) => 0,
                    (_, x, 0) => at(x - 1, 0),
                    (_, 0, y) => at(0, y - 1),
                    (Filter::Horizontal, x, y) => at(x - 1, y),
                    (Filter::Vertical, x, y) => at(x, y - 1),
                    (Filter::Gradient, x, y) => {
                        let g = i16::from(at(x - 1, y)) + i16::from(at(x, y - 1))
                            - i16::from(at(x - 1, y - 1));
                        g.clamp(0, 255) as u8
                    }
                };
                out[y * width + x] = at(x, y).wrapping_sub(predicted);
            }
        }
        out
    }

    #[test]
    fn every_filter_round_trips() {
        let (width, height) = (13usize, 7usize);
        let plane: Vec<u8> = (0..width * height)
            .map(|i| ((i * 37) ^ (i / width * 91)) as u8)
            .collect();
        for filter in [
            Filter::None,
            Filter::Horizontal,
            Filter::Vertical,
            Filter::Gradient,
        ] {
            let mut stored = filtered(filter, &plane, width);
            unfilter(filter, &mut stored, width);
            assert_eq!(stored, plane, "{filter:?}");
        }
    }

    #[test]
    fn a_raw_plane_is_read_and_unfiltered() {
        // 3x2, horizontal: differences from the left, the first of each row
        // from the one above.
        let chunk = [0b0000_0100, 10, 5, 5, 1, 2, 2];
        assert_eq!(
            decode(&chunk, 3, 2, Limits::default()).unwrap(),
            [10, 15, 20, 11, 13, 15]
        );
    }

    #[test]
    fn headers_libwebp_refuses_are_refused() {
        let plane = [0u8; 7];
        let with = |header: u8| {
            let mut chunk = vec![header];
            chunk.extend_from_slice(&plane);
            decode(&chunk, 3, 2, Limits::default())
        };
        assert!(with(0).is_ok());
        assert!(matches!(with(2), Err(ImageError::Malformed(_))), "method 2");
        assert!(
            matches!(with(0b0010_0000), Err(ImageError::Malformed(_))),
            "preprocessing 2"
        );
        assert!(
            matches!(with(0b0100_0000), Err(ImageError::Malformed(_))),
            "reserved"
        );
        // Level reduction is accepted, and changes nothing.
        assert_eq!(with(0b0001_0000).unwrap(), vec![0; 6]);
        assert_eq!(
            decode(&[0, 1, 2], 3, 2, Limits::default()),
            Err(ImageError::Truncated)
        );
        assert_eq!(
            decode(&[0], 3, 2, Limits::default()),
            Err(ImageError::Truncated)
        );
    }
}
