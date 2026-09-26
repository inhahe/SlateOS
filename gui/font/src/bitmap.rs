//! Colour bitmap glyphs: Google's `CBDT`/`CBLC` and Apple's `sbix`.
//!
//! Where `COLR` stores a recipe for each emoji ([`colr`](crate::colr)), these
//! store the picture: a PNG per glyph, drawn at one or a few sizes called
//! *strikes*. Noto Color Emoji's bitmap build has one strike, 109 pixels to
//! the em, and no outlines at all. [`render`] finds the glyph in the strike
//! nearest above the size asked for (the biggest there is, if none is above),
//! decodes its picture with `imagecodec`, and resamples it to the size: by
//! area when shrinking, which is every emoji in running text, and bilinearly
//! when enlarging. The result is a [`ColourImage`], so the glyph caches,
//! `draw_text` and the compositor draw it without knowing which kind of
//! colour glyph it was.
//!
//! # Placement
//!
//! A strike's metrics are in its own pixels. `CBDT`'s bearings put the
//! picture's top-left corner that far right of the pen and that far above the
//! baseline; `sbix`'s origin offset puts its *bottom*-left corner, as FreeType
//! reads it. Both scale by the ratio of the size asked for to the strike's,
//! and the picture is placed to the nearest whole pixel, as a mask is.
//!
//! # Hostile fonts
//!
//! Every offset is bounds-checked. A picture is decoded under limits that
//! refuse anything over [`MAX_STRIKE_PIXELS`] from its header, before a pixel
//! is allocated; the resampled picture is capped as a `COLR` one is
//! ([`MAX_COLOUR_PIXELS`]); an `sbix` `dupe` -- a glyph that says "draw that
//! one instead" -- is followed once; and no more than [`MAX_STRIKES`] strikes
//! are looked at.

use alloc::vec::Vec;

use crate::colr::{ColourImage, MAX_COLOUR_PIXELS, Rgba, pack, premultiply, unpack};
use crate::raster::mad;
use crate::sfnt::Face;

/// The largest picture a strike may hold for one glyph: 1024 by 1024, eight
/// times the side of the biggest emoji strike in use.
pub const MAX_STRIKE_PIXELS: u64 = 1 << 20;

/// How many strikes of a table are considered.
pub const MAX_STRIKES: usize = 64;

/// Glyph `gid` of `face` as a colour picture at `px_per_em`, from its `CBDT`
/// or `sbix` strikes. `None` if no strike has a picture for it, if the
/// picture cannot be read, or if it would be bigger than
/// [`MAX_COLOUR_PIXELS`].
#[must_use]
pub fn render(face: &Face, gid: u16, px_per_em: f32) -> Option<ColourImage> {
    if !px_per_em.is_finite() || px_per_em <= 0.0 {
        return None;
    }
    let tables = face.bitmap_tables();
    let found = tables
        .cbdt
        .and_then(|(cblc, cbdt)| cbdt_glyph(cblc, cbdt, gid, px_per_em))
        .or_else(|| {
            tables
                .sbix
                .and_then(|sbix| sbix_glyph(sbix, gid, face.num_glyphs(), px_per_em))
        })?;
    found.render(px_per_em)
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

fn u8_at(d: &[u8], at: usize) -> Option<u8> {
    d.get(at).copied()
}

fn i8_at(d: &[u8], at: usize) -> Option<i8> {
    d.get(at).map(|&b| i8::from_be_bytes([b]))
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn i16_at(d: &[u8], at: usize) -> Option<i16> {
    Some(i16::from_be_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn u32_at(d: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32::from_be_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
    .ok()
}

/// `base + k`, checked.
fn at(base: usize, k: usize) -> Option<usize> {
    base.checked_add(k)
}

/// `base + i * stride`, checked.
fn nth(base: usize, i: usize, stride: usize) -> Option<usize> {
    base.checked_add(i.checked_mul(stride)?)
}

/// Strike sizes in the order to try them for `px_per_em`: the smallest at or
/// above it first -- shrinking loses less than enlarging -- then the rest
/// from the biggest down.
fn by_preference<T>(strikes: &mut [(f32, T)], px_per_em: f32) {
    strikes.sort_by(|a, b| {
        let key = |ppem: f32| {
            (
                ppem < px_per_em,
                if ppem < px_per_em { -ppem } else { ppem },
            )
        };
        let ((a_below, a_size), (b_below, b_size)) = (key(a.0), key(b.0));
        a_below.cmp(&b_below).then(a_size.total_cmp(&b_size))
    });
}

/// A glyph's picture in a strike, not yet decoded.
struct Found<'a> {
    /// The encoded picture: PNG, and for `sbix` possibly JPEG or TIFF.
    data: &'a [u8],
    /// The strike's size in pixels per em, horizontally and vertically.
    ppem: (f32, f32),
    /// Where the picture sits, in the strike's pixels, y up.
    place: Place,
}

/// Where a strike puts a glyph's picture relative to the pen on the baseline.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    /// `CBDT`: the top-left corner, `bearing_y` above the baseline.
    TopLeft { bearing_x: f32, bearing_y: f32 },
    /// `sbix`: the bottom-left corner.
    BottomLeft { x: f32, y: f32 },
}

// ---------------------------------------------------------------------------
// CBDT / CBLC
// ---------------------------------------------------------------------------

/// Where one glyph's data is in `CBDT`, and the metrics the index gives it,
/// if the index gives any (index formats 2 and 5 do).
struct Located {
    offset: usize,
    len: usize,
    image_format: u16,
    index_metrics: Option<(f32, f32)>,
}

/// The picture of `gid` in the best of `CBLC`'s strikes for `px_per_em`.
fn cbdt_glyph<'a>(cblc: &[u8], cbdt: &'a [u8], gid: u16, px_per_em: f32) -> Option<Found<'a>> {
    let count = u32_at(cblc, 4)?;
    let mut strikes: Vec<(f32, usize)> = (0..count.min(MAX_STRIKES))
        .filter_map(|i| {
            let size = nth(8, i, 48)?;
            let (start, end) = (u16_at(cblc, at(size, 40)?)?, u16_at(cblc, at(size, 42)?)?);
            // Colour strikes are 32 bits a pixel; 1, 2, 4 and 8 are
            // monochrome and grey bitmaps, which an outline face has anyway.
            let colour = u8_at(cblc, at(size, 46)?)? == 32;
            let ppem_y = f32::from(u8_at(cblc, at(size, 45)?)?);
            (colour && ppem_y > 0.0 && (start..=end).contains(&gid)).then_some((ppem_y, size))
        })
        .collect();
    by_preference(&mut strikes, px_per_em);
    strikes.iter().find_map(|&(ppem_y, size)| {
        let ppem_x = f32::from(u8_at(cblc, at(size, 44)?)?);
        let located = locate(cblc, size, gid)?;
        cbdt_picture(
            cbdt,
            &located,
            (if ppem_x > 0.0 { ppem_x } else { ppem_y }, ppem_y),
        )
    })
}

/// Find `gid` in the index subtables of the strike whose `BitmapSize` record
/// is at `size`.
fn locate(cblc: &[u8], size: usize, gid: u16) -> Option<Located> {
    let array = u32_at(cblc, size)?;
    let count = u32_at(cblc, at(size, 8)?)?;
    // The subtable records are sorted by first glyph; a scan is fine, there
    // are a handful.
    (0..count.min(1 << 16)).find_map(|k| {
        let record = nth(array, k, 8)?;
        let first = u16_at(cblc, record)?;
        let last = u16_at(cblc, at(record, 2)?)?;
        if !(first..=last).contains(&gid) {
            return None;
        }
        let sub = at(array, u32_at(cblc, at(record, 4)?)?)?;
        locate_in(cblc, sub, first, gid)
    })
}

/// Find `gid` in the index subtable at `sub`, which starts at glyph `first`.
fn locate_in(cblc: &[u8], sub: usize, first: u16, gid: u16) -> Option<Located> {
    let index_format = u16_at(cblc, sub)?;
    let image_format = u16_at(cblc, at(sub, 2)?)?;
    let image_data = u32_at(cblc, at(sub, 4)?)?;
    let i = usize::from(gid.checked_sub(first)?);
    // Big glyph metrics: height, width, then the horizontal bearings.
    let big = |m: usize| -> Option<(f32, f32)> {
        Some((
            f32::from(i8_at(cblc, at(m, 2)?)?),
            f32::from(i8_at(cblc, at(m, 3)?)?),
        ))
    };
    let span = |start: usize, end: usize| -> Option<(usize, usize)> {
        end.checked_sub(start)
            .filter(|&len| len > 0)
            .map(|len| (start, len))
    };
    let (start, len, index_metrics) = match index_format {
        // Offsets from the image data, one per glyph and one past the last.
        1 | 3 => {
            let (wide, stride) = if index_format == 1 {
                (true, 4)
            } else {
                (false, 2)
            };
            let offsets = at(sub, 8)?;
            let read = |j: usize| -> Option<usize> {
                let p = nth(offsets, j, stride)?;
                if wide {
                    u32_at(cblc, p)
                } else {
                    u16_at(cblc, p).map(usize::from)
                }
            };
            let (a, b) = span(read(i)?, read(i.checked_add(1)?)?)?;
            (a, b, None)
        }
        // Every glyph the same size, with the metrics here.
        2 => {
            let image_size = u32_at(cblc, at(sub, 8)?)?;
            (
                i.checked_mul(image_size)?,
                image_size,
                Some(big(at(sub, 12)?)?),
            )
        }
        // Sparse: (glyph, offset) pairs, one past the last.
        4 => {
            let count = u32_at(cblc, at(sub, 8)?)?;
            let pairs = at(sub, 12)?;
            let k = (0..count.min(1 << 16))
                .find(|&k| nth(pairs, k, 4).and_then(|p| u16_at(cblc, p)) == Some(gid))?;
            let offset = |k: usize| nth(pairs, k, 4).and_then(|p| u16_at(cblc, at(p, 2)?));
            let (a, b) = span(
                usize::from(offset(k)?),
                usize::from(offset(k.checked_add(1)?)?),
            )?;
            (a, b, None)
        }
        // Sparse and every glyph the same size.
        5 => {
            let image_size = u32_at(cblc, at(sub, 8)?)?;
            let metrics = big(at(sub, 12)?)?;
            let count = u32_at(cblc, at(sub, 20)?)?;
            let ids = at(sub, 24)?;
            let k = (0..count.min(1 << 16))
                .find(|&k| nth(ids, k, 2).and_then(|p| u16_at(cblc, p)) == Some(gid))?;
            (k.checked_mul(image_size)?, image_size, Some(metrics))
        }
        _ => return None,
    };
    Some(Located {
        offset: image_data.checked_add(start)?,
        len,
        image_format,
        index_metrics,
    })
}

/// The picture and bearings of a located `CBDT` glyph.
fn cbdt_picture<'a>(cbdt: &'a [u8], located: &Located, ppem: (f32, f32)) -> Option<Found<'a>> {
    let glyph = cbdt.get(located.offset..located.offset.checked_add(located.len)?)?;
    // Small metrics are height, width, bearing x, bearing y, advance; big ones
    // the same with the vertical three after. Format 19 leaves them to the
    // index.
    let (metrics, data_at) = match located.image_format {
        17 => (Some((i8_at(glyph, 2)?, i8_at(glyph, 3)?)), 5),
        18 => (Some((i8_at(glyph, 2)?, i8_at(glyph, 3)?)), 8),
        19 => (None, 0),
        _ => return None,
    };
    let (bearing_x, bearing_y) = match metrics {
        Some((x, y)) => (f32::from(x), f32::from(y)),
        None => located.index_metrics?,
    };
    let len = u32_at(glyph, data_at)?;
    let start = at(data_at, 4)?;
    let data = glyph.get(start..start.checked_add(len)?)?;
    Some(Found {
        data,
        ppem,
        place: Place::TopLeft {
            bearing_x,
            bearing_y,
        },
    })
}

// ---------------------------------------------------------------------------
// sbix
// ---------------------------------------------------------------------------

/// The picture of `gid` in the best of `sbix`'s strikes for `px_per_em`.
fn sbix_glyph(sbix: &[u8], gid: u16, num_glyphs: u16, px_per_em: f32) -> Option<Found<'_>> {
    let count = u32_at(sbix, 4)?;
    let mut strikes: Vec<(f32, usize)> = (0..count.min(MAX_STRIKES))
        .filter_map(|i| {
            let strike = u32_at(sbix, nth(8, i, 4)?)?;
            let ppem = f32::from(u16_at(sbix, strike)?);
            (ppem > 0.0).then_some((ppem, strike))
        })
        .collect();
    by_preference(&mut strikes, px_per_em);
    strikes
        .iter()
        .find_map(|&(ppem, strike)| sbix_in_strike(sbix, strike, ppem, gid, num_glyphs, true))
}

/// The picture of `gid` in the `sbix` strike at `strike`, following one
/// `dupe` if `follow` is set.
fn sbix_in_strike(
    sbix: &[u8],
    strike: usize,
    ppem: f32,
    gid: u16,
    num_glyphs: u16,
    follow: bool,
) -> Option<Found<'_>> {
    if gid >= num_glyphs {
        return None;
    }
    let offsets = at(strike, 4)?;
    let start = u32_at(sbix, nth(offsets, usize::from(gid), 4)?)?;
    let end = u32_at(sbix, nth(offsets, usize::from(gid).checked_add(1)?, 4)?)?;
    // Eight bytes of header before any picture; less is no glyph at all.
    if end < start.checked_add(8)? {
        return None;
    }
    let (start, end) = (at(strike, start)?, at(strike, end)?);
    let x = f32::from(i16_at(sbix, start)?);
    let y = f32::from(i16_at(sbix, at(start, 2)?)?);
    let tag = sbix.get(at(start, 4)?..at(start, 8)?)?;
    let data = sbix.get(at(start, 8)?..end)?;
    match tag {
        b"dupe" if follow => {
            let other = u16_at(data, 0)?;
            sbix_in_strike(sbix, strike, ppem, other, num_glyphs, false)
        }
        b"png " | b"jpg " | b"tiff" => Some(Found {
            data,
            ppem: (ppem, ppem),
            place: Place::BottomLeft { x, y },
        }),
        // `mask`, `pdf `, and a `dupe` of a `dupe`.
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Drawing
// ---------------------------------------------------------------------------

impl Found<'_> {
    /// Decode the picture and fit it to `px_per_em`.
    fn render(&self, px_per_em: f32) -> Option<ColourImage> {
        let limits = imagecodec::Limits {
            max_pixels: MAX_STRIKE_PIXELS,
            max_decompressed_bytes: usize::try_from(MAX_STRIKE_PIXELS.saturating_mul(16)).ok()?,
        };
        let image = imagecodec::decode(self.data, limits).ok()?;
        let (sw, sh) = (
            usize::try_from(image.width).ok()?,
            usize::try_from(image.height).ok()?,
        );
        #[allow(
            clippy::cast_precision_loss,
            reason = "a strike's picture is at most 2^20 pixels"
        )]
        let (w, h) = (sw as f32, sh as f32);
        let (scale_x, scale_y) = (px_per_em / self.ppem.0, px_per_em / self.ppem.1);
        // The top-left corner, in pixels at the size asked for, y down.
        let (left, top) = match self.place {
            Place::TopLeft {
                bearing_x,
                bearing_y,
            } => (bearing_x * scale_x, -bearing_y * scale_y),
            Place::BottomLeft { x, y } => (x * scale_x, -(y + h) * scale_y),
        };
        let (dw, dh) = (
            (w * scale_x).round().max(1.0),
            (h * scale_y).round().max(1.0),
        );
        #[allow(clippy::cast_precision_loss, reason = "a power of two, exact")]
        let fits = dw * dh <= MAX_COLOUR_PIXELS as f32 && left.abs() <= 1.0e8 && top.abs() <= 1.0e8;
        if !fits {
            return None;
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "whole numbers from 1 to MAX_COLOUR_PIXELS, checked above"
        )]
        let (dw, dh) = (dw as usize, dh as usize);
        let source: Vec<Rgba> = image
            .pixels
            .iter()
            .map(|&p| premultiply(unpack(p)))
            .collect();
        let pixels = resample(&source, (sw, sh), (dw, dh));
        #[allow(
            clippy::cast_possible_truncation,
            reason = "sides at most MAX_COLOUR_PIXELS, offsets at most 1e8"
        )]
        Some(ColourImage {
            width: dw as u32,
            height: dh as u32,
            left: left.round() as i32,
            top: top.round() as i32,
            pixels: pixels.into_iter().map(pack).collect(),
            uses_foreground: false,
        })
    }
}

/// For each of `dst` output positions along one axis of `src` input pixels,
/// the first input pixel it draws from and the weights of it and those after.
///
/// Shrinking, an output pixel is the area average of the input it covers,
/// partial pixels at either end weighted by how much of them it covers.
/// Enlarging, it is the linear blend of the two input pixels whose centres
/// are either side of its own, clamped at the edges.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "sizes below 2^20, and every float cast is of a value clamped to the axis first"
)]
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    if src == 0 || dst == 0 {
        return Vec::new();
    }
    let ratio = src as f32 / dst as f32;
    let last = src.saturating_sub(1) as f32;
    (0..dst)
        .map(|i| {
            if ratio > 1.0 {
                let (a, b) = (i as f32 * ratio, (i as f32 + 1.0) * ratio);
                let first = a.floor().clamp(0.0, last) as usize;
                let end = (b.ceil() as usize).clamp(first.saturating_add(1), src);
                let mut ws: Vec<f32> = (first..end)
                    .map(|j| {
                        let j = j as f32;
                        (b.min(j + 1.0) - a.max(j)).max(0.0)
                    })
                    .collect();
                let total: f32 = ws.iter().sum();
                if total > 0.0 {
                    for w in &mut ws {
                        *w /= total;
                    }
                }
                (first, ws)
            } else {
                let c = ((i as f32 + 0.5) * ratio - 0.5).clamp(0.0, last);
                let j0 = c.floor();
                let f = c - j0;
                let j0 = j0 as usize;
                if f > 0.0 && j0.checked_add(1).is_some_and(|next| next < src) {
                    (j0, alloc::vec![1.0 - f, f])
                } else {
                    (j0, alloc::vec![1.0])
                }
            }
        })
        .collect()
}

/// `source`, `(width, height)` premultiplied pixels, resampled to `to`:
/// columns first, then rows.
fn resample(source: &[Rgba], (sw, sh): (usize, usize), (dw, dh): (usize, usize)) -> Vec<Rgba> {
    let columns = weights(sw, dw);
    let rows = weights(sh, dh);
    let blend = |pixels: &mut dyn Iterator<Item = (Rgba, f32)>| {
        let mut out = [0.0f32; 4];
        for (p, w) in pixels {
            for (o, v) in out.iter_mut().zip(p) {
                *o = mad(v, w, *o);
            }
        }
        out
    };
    // Every source row, at the new width.
    let wide: Vec<Rgba> = source
        .chunks_exact(sw.max(1))
        .flat_map(|row| {
            columns.iter().map(move |(first, ws)| {
                blend(
                    &mut ws
                        .iter()
                        .enumerate()
                        .filter_map(|(k, &w)| Some((*row.get(first.checked_add(k)?)?, w))),
                )
            })
        })
        .collect();
    // Then every output row from those.
    let mut out = Vec::with_capacity(dw.saturating_mul(dh));
    for (first, ws) in &rows {
        for x in 0..dw {
            out.push(blend(&mut ws.iter().enumerate().filter_map(|(k, &w)| {
                let y = first.checked_add(k)?;
                Some((*wide.get(y.checked_mul(dw)?.checked_add(x)?)?, w))
            })));
        }
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::float_cmp,
    reason = "test fixture builder"
)]
mod tests {
    use super::*;
    use crate::sfnt::tests::{build_test_font_with, build_test_font_without};
    use alloc::vec;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];

    /// A `side` x `side` PNG of one colour.
    fn solid(side: u32, colour: [u8; 4]) -> Vec<u8> {
        imagecodec::testing::png_rgba(side, side, |_, _| colour)
    }

    /// `0xAARRGGBB` of an opaque RGBA colour.
    fn argb([r, g, b, a]: [u8; 4]) -> u32 {
        u32::from_be_bytes([a, r, g, b])
    }

    struct Glyph {
        gid: u16,
        png: Vec<u8>,
        /// Bearing x and bearing y, in the strike's pixels.
        bearing: (i8, i8),
        side: u8,
    }

    struct Strike {
        ppem: u8,
        index_format: u16,
        image_format: u16,
        glyphs: Vec<Glyph>,
    }

    /// `CBLC` and `CBDT` for `strikes`, each strike one index subtable of its
    /// own formats. Formats 1 to 3 need their glyphs consecutive.
    fn cbdt_tables(strikes: &[Strike]) -> (Vec<u8>, Vec<u8>) {
        let mut cbdt = vec![0, 3, 0, 0];
        let mut cblc = vec![0, 3, 0, 0];
        cblc.extend_from_slice(&(strikes.len() as u32).to_be_bytes());
        let sizes = cblc.len();
        cblc.resize(sizes + 48 * strikes.len(), 0);
        for (n, strike) in strikes.iter().enumerate() {
            let glyph_record = |g: &Glyph| {
                let mut r = Vec::new();
                let (bx, by) = (g.bearing.0 as u8, g.bearing.1 as u8);
                match strike.image_format {
                    17 => r.extend_from_slice(&[g.side, g.side, bx, by, g.side]),
                    18 => r.extend_from_slice(&[g.side, g.side, bx, by, g.side, 0, 0, g.side]),
                    _ => {}
                }
                r.extend_from_slice(&(g.png.len() as u32).to_be_bytes());
                r.extend_from_slice(&g.png);
                r
            };
            let records: Vec<Vec<u8>> = strike.glyphs.iter().map(glyph_record).collect();
            // Formats 2 and 5 give every glyph the same size of record.
            let uniform = records.iter().map(Vec::len).max().unwrap_or(0);
            let image_data = cbdt.len();
            let mut offsets = vec![0usize];
            for r in &records {
                cbdt.extend_from_slice(r);
                if matches!(strike.index_format, 2 | 5) {
                    cbdt.resize(cbdt.len() + uniform - r.len(), 0);
                }
                offsets.push(cbdt.len() - image_data);
            }
            let first = strike.glyphs.first().unwrap();
            let big = [
                first.side,
                first.side,
                first.bearing.0 as u8,
                first.bearing.1 as u8,
                first.side,
                0,
                0,
                first.side,
            ];
            let mut sub = Vec::new();
            sub.extend_from_slice(&strike.index_format.to_be_bytes());
            sub.extend_from_slice(&strike.image_format.to_be_bytes());
            sub.extend_from_slice(&(image_data as u32).to_be_bytes());
            match strike.index_format {
                1 => offsets
                    .iter()
                    .for_each(|&o| sub.extend_from_slice(&(o as u32).to_be_bytes())),
                2 => {
                    sub.extend_from_slice(&(uniform as u32).to_be_bytes());
                    sub.extend_from_slice(&big);
                }
                3 => offsets
                    .iter()
                    .for_each(|&o| sub.extend_from_slice(&(o as u16).to_be_bytes())),
                4 => {
                    sub.extend_from_slice(&(strike.glyphs.len() as u32).to_be_bytes());
                    for (k, &o) in offsets.iter().enumerate() {
                        let gid = strike.glyphs.get(k).map_or(0, |g| g.gid);
                        sub.extend_from_slice(&gid.to_be_bytes());
                        sub.extend_from_slice(&(o as u16).to_be_bytes());
                    }
                }
                _ => {
                    sub.extend_from_slice(&(uniform as u32).to_be_bytes());
                    sub.extend_from_slice(&big);
                    sub.extend_from_slice(&(strike.glyphs.len() as u32).to_be_bytes());
                    for g in &strike.glyphs {
                        sub.extend_from_slice(&g.gid.to_be_bytes());
                    }
                }
            }
            let (lo, hi) = (first.gid, strike.glyphs.last().unwrap().gid);
            let array = cblc.len();
            cblc.extend_from_slice(&lo.to_be_bytes());
            cblc.extend_from_slice(&hi.to_be_bytes());
            cblc.extend_from_slice(&8u32.to_be_bytes());
            cblc.extend_from_slice(&sub);
            let size = sizes + 48 * n;
            cblc[size..size + 4].copy_from_slice(&(array as u32).to_be_bytes());
            cblc[size + 4..size + 8].copy_from_slice(&((8 + sub.len()) as u32).to_be_bytes());
            cblc[size + 8..size + 12].copy_from_slice(&1u32.to_be_bytes());
            cblc[size + 40..size + 42].copy_from_slice(&lo.to_be_bytes());
            cblc[size + 42..size + 44].copy_from_slice(&hi.to_be_bytes());
            cblc[size + 44] = strike.ppem;
            cblc[size + 45] = strike.ppem;
            cblc[size + 46] = 32;
            cblc[size + 47] = 1;
        }
        (cblc, cbdt)
    }

    fn cbdt_face(strikes: &[Strike]) -> Face {
        let (cblc, cbdt) = cbdt_tables(strikes);
        Face::parse(build_test_font_with(vec![
            (*b"CBLC", cblc),
            (*b"CBDT", cbdt),
        ]))
        .unwrap()
    }

    fn one_strike(index_format: u16, image_format: u16) -> Strike {
        Strike {
            ppem: 20,
            index_format,
            image_format,
            glyphs: vec![
                Glyph {
                    gid: 1,
                    png: solid(20, RED),
                    bearing: (2, 16),
                    side: 20,
                },
                Glyph {
                    gid: 2,
                    png: solid(20, BLUE),
                    bearing: (2, 16),
                    side: 20,
                },
            ],
        }
    }

    fn place(img: &ColourImage) -> (i32, i32, u32, u32) {
        (img.left, img.top, img.width, img.height)
    }

    #[test]
    fn a_cbdt_glyph_is_its_picture_scaled_to_the_size_and_placed_by_its_bearings() {
        let face = cbdt_face(&[one_strike(1, 17)]);
        assert!(face.has_bitmap_glyphs() && face.has_colour_glyphs());
        // At the strike's own size: the picture, 2 px right of the pen and
        // its top 16 above the baseline.
        let img = render(&face, 1, 20.0).unwrap();
        assert_eq!(place(&img), (2, -16, 20, 20));
        assert!(img.pixels.iter().all(|&p| p == argb(RED)));
        assert!(!img.uses_foreground);
        // Half the size, and twice: everything scales.
        let img = render(&face, 1, 10.0).unwrap();
        assert_eq!(place(&img), (1, -8, 10, 10));
        assert!(img.pixels.iter().all(|&p| p == argb(RED)));
        let img = render(&face, 2, 40.0).unwrap();
        assert_eq!(place(&img), (4, -32, 40, 40));
        assert!(img.pixels.iter().all(|&p| p == argb(BLUE)));
        // A glyph the strike does not hold.
        assert!(render(&face, 3, 20.0).is_none());
        assert!(render(&face, 0, 20.0).is_none());
    }

    #[test]
    fn every_index_format_and_image_format_finds_the_right_picture() {
        for (index_format, image_format) in [
            (1, 17),
            (1, 18),
            (2, 19),
            (3, 17),
            (4, 18),
            (5, 19),
            (4, 17),
        ] {
            let face = cbdt_face(&[one_strike(index_format, image_format)]);
            for (gid, colour) in [(1, RED), (2, BLUE)] {
                let img = render(&face, gid, 20.0).unwrap_or_else(|| {
                    panic!("index {index_format}, image {image_format}: glyph {gid}")
                });
                assert_eq!(
                    place(&img),
                    (2, -16, 20, 20),
                    "index {index_format}, image {image_format}"
                );
                assert!(img.pixels.iter().all(|&p| p == argb(colour)));
            }
            assert!(render(&face, 3, 20.0).is_none());
        }
        // Image formats with no colour, and index formats that do not exist.
        for (index_format, image_format) in [(1, 1), (1, 5), (1, 9), (6, 17)] {
            let face = cbdt_face(&[one_strike(index_format, image_format)]);
            assert!(
                render(&face, 1, 20.0).is_none(),
                "index {index_format}, image {image_format}"
            );
        }
    }

    #[test]
    fn the_strike_at_or_above_the_size_is_preferred_then_the_biggest() {
        let strike = |ppem: u8, colour| Strike {
            ppem,
            index_format: 1,
            image_format: 17,
            glyphs: vec![Glyph {
                gid: 1,
                png: solid(u32::from(ppem), colour),
                bearing: (0, 0),
                side: ppem,
            }],
        };
        let face = cbdt_face(&[strike(10, GREEN), strike(40, RED), strike(80, BLUE)]);
        let colour = |px: f32| render(&face, 1, px).unwrap().pixels[0];
        assert_eq!(colour(8.0), argb(GREEN));
        assert_eq!(colour(10.0), argb(GREEN));
        assert_eq!(colour(20.0), argb(RED));
        assert_eq!(colour(41.0), argb(BLUE));
        assert_eq!(colour(200.0), argb(BLUE));
        // Whichever strike, the picture is the size asked for.
        let img = render(&face, 1, 20.0).unwrap();
        assert_eq!((img.width, img.height), (20, 20));
    }

    #[test]
    fn a_face_of_pictures_alone_parses_and_draws_them() {
        let (cblc, cbdt) = cbdt_tables(&[one_strike(1, 17)]);
        let face = Face::parse(build_test_font_without(
            &[*b"glyf", *b"loca"],
            vec![(*b"CBLC", cblc), (*b"CBDT", cbdt)],
        ))
        .unwrap();
        assert!(face.outline(1).unwrap().is_empty());
        assert!(face.outline(9).is_err());
        assert_eq!(render(&face, 1, 20.0).unwrap().pixels[0], argb(RED));
        // Without the pictures it is refused, as before.
        assert!(Face::parse(build_test_font_without(&[*b"glyf", *b"loca"], vec![])).is_err());
    }

    #[test]
    fn a_scaled_font_draws_pictures_through_its_colour_cache() {
        let face = alloc::sync::Arc::new(cbdt_face(&[one_strike(1, 17)]));
        let mut font = crate::scaled::ScaledFont::shared(face, 10.0).unwrap();
        let img = font.colour_glyph(1, 0xFF00_0000).cloned().unwrap();
        assert_eq!(place(&img), (1, -8, 10, 10));
        assert!(img.pixels.iter().all(|&p| p == argb(RED)));
        // A glyph with no picture falls to its outline.
        assert!(font.colour_glyph(3, 0xFF00_0000).is_none());
        // A face with both a recipe and a picture for a glyph uses the recipe.
        let (cblc, cbdt) = cbdt_tables(&[one_strike(1, 17)]);
        let both = Face::parse(build_test_font_with(vec![
            (*b"CBLC", cblc),
            (*b"CBDT", cbdt),
            (
                *b"COLR",
                crate::colr::tests::colr_v0(&[(1, 0, 1)], &[(1, 0)]),
            ),
            (*b"CPAL", crate::colr::tests::cpal(&[0xFF00_FF00])),
        ]))
        .unwrap();
        let mut font =
            crate::scaled::ScaledFont::shared(alloc::sync::Arc::new(both), 100.0).unwrap();
        // The recipe paints the fixture's square green; the picture is red.
        let img = font.colour_glyph(1, 0xFF00_0000).cloned().unwrap();
        assert!(img.pixels.iter().all(|&p| p == 0xFF00_FF00));
        // And a glyph only the pictures have comes from them.
        assert!(
            font.colour_glyph(2, 0xFF00_0000)
                .is_some_and(|i| i.pixels[0] == argb(BLUE))
        );
    }

    /// An `sbix` glyph for [`sbix_table`]: `(gid, origin x, origin y, tag,
    /// data)`.
    type SbixGlyph = (u16, i16, i16, [u8; 4], Vec<u8>);

    /// An `sbix` of `strikes`, each `(ppem, glyphs)`, a glyph
    /// `(gid, origin x, origin y, tag, data)`, for a face of `num_glyphs`.
    fn sbix_table(num_glyphs: u16, strikes: &[(u16, Vec<SbixGlyph>)]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&(strikes.len() as u32).to_be_bytes());
        let offsets_at = out.len();
        out.resize(offsets_at + 4 * strikes.len(), 0);
        for (n, (ppem, glyphs)) in strikes.iter().enumerate() {
            let strike = out.len();
            out[offsets_at + 4 * n..offsets_at + 4 * n + 4]
                .copy_from_slice(&(strike as u32).to_be_bytes());
            out.extend_from_slice(&ppem.to_be_bytes());
            out.extend_from_slice(&72u16.to_be_bytes());
            let table = out.len();
            out.resize(table + 4 * (usize::from(num_glyphs) + 1), 0);
            let mut data = Vec::new();
            let base = 4 + 4 * (usize::from(num_glyphs) + 1);
            for gid in 0..=num_glyphs {
                let at = base + data.len();
                out[table + 4 * usize::from(gid)..table + 4 * usize::from(gid) + 4]
                    .copy_from_slice(&(at as u32).to_be_bytes());
                if let Some((_, x, y, tag, bytes)) = glyphs.iter().find(|g| g.0 == gid) {
                    data.extend_from_slice(&x.to_be_bytes());
                    data.extend_from_slice(&y.to_be_bytes());
                    data.extend_from_slice(tag);
                    data.extend_from_slice(bytes);
                }
            }
            out.extend_from_slice(&data);
        }
        out
    }

    #[test]
    fn an_sbix_glyph_is_placed_by_its_bottom_left_corner_and_a_dupe_is_followed_once() {
        let face = Face::parse(build_test_font_with(vec![(
            *b"sbix",
            sbix_table(
                4,
                &[(
                    20,
                    vec![
                        (1, 1, -2, *b"png ", solid(20, RED)),
                        (2, 0, 0, *b"dupe", 1u16.to_be_bytes().to_vec()),
                        (3, 0, 0, *b"dupe", 2u16.to_be_bytes().to_vec()),
                    ],
                )],
            ),
        )]))
        .unwrap();
        assert!(face.has_bitmap_glyphs());
        // The bottom edge 2 below the baseline: the top 18 above it.
        let img = render(&face, 1, 20.0).unwrap();
        assert_eq!(place(&img), (1, -18, 20, 20));
        assert!(img.pixels.iter().all(|&p| p == argb(RED)));
        assert_eq!(render(&face, 2, 20.0), Some(img));
        // A dupe of a dupe is not followed; an empty record is no glyph.
        assert!(render(&face, 3, 20.0).is_none());
        assert!(render(&face, 0, 20.0).is_none());
        assert_eq!(place(&render(&face, 1, 10.0).unwrap()), (1, -9, 10, 10));
    }

    #[test]
    fn resampling_averages_by_area_and_blends_between_centres() {
        let (a, b) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        // Halving: pairs averaged.
        assert_eq!(resample(&[a, a, b, b], (4, 1), (2, 1)), vec![a, b]);
        // A third: a partial pixel weighted by its share.
        let third = resample(&[a, a, b], (3, 1), (2, 1));
        assert!((third[0][0] - 1.0).abs() < 1e-6);
        assert!((third[1][0] - 1.0 / 3.0).abs() < 1e-6 && (third[1][2] - 2.0 / 3.0).abs() < 1e-6);
        // Doubling: the two ends clamp, the middles blend a quarter and three
        // quarters.
        let up = resample(&[a, b], (2, 1), (4, 1));
        assert_eq!(up[0], a);
        assert_eq!(up[3], b);
        assert!((up[1][0] - 0.75).abs() < 1e-6 && (up[2][0] - 0.25).abs() < 1e-6);
        // The same size is the same picture; and down both axes at once.
        assert_eq!(resample(&[a, b, b, a], (2, 2), (2, 2)), vec![a, b, b, a]);
        let one = resample(&[a, b, b, a], (2, 2), (1, 1));
        assert!((one[0][0] - 0.5).abs() < 1e-6 && (one[0][2] - 0.5).abs() < 1e-6);
        // Premultiplied: a transparent pixel beside an opaque one averages to
        // the opaque one's colour at half alpha.
        let half = resample(&[[0.0; 4], a], (2, 1), (1, 1));
        assert_eq!(half, vec![[0.5, 0.0, 0.0, 0.5]]);
    }

    #[test]
    fn a_mutated_table_draws_something_or_nothing_but_never_panics() {
        let strikes = [one_strike(1, 17), one_strike(4, 18), one_strike(5, 19)];
        let (cblc, cbdt) = cbdt_tables(&strikes);
        let sbix = sbix_table(
            4,
            &[(
                20,
                vec![
                    (1, 1, -2, *b"png ", solid(4, RED)),
                    (2, 0, 0, *b"dupe", vec![0, 1]),
                ],
            )],
        );
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for round in 0..600 {
            let (mut cblc, mut cbdt, mut sbix) = (cblc.clone(), cbdt.clone(), sbix.clone());
            let target: &mut Vec<u8> = match round % 3 {
                0 => &mut cblc,
                1 => &mut cbdt,
                _ => &mut sbix,
            };
            for _ in 0..=(next() % 3) {
                let i = (next() % target.len() as u64) as usize;
                target[i] = next() as u8;
            }
            let Ok(face) = Face::parse(build_test_font_with(vec![
                (*b"CBLC", cblc),
                (*b"CBDT", cbdt),
                (*b"sbix", sbix),
            ])) else {
                continue;
            };
            for gid in 0..=4 {
                for px in [7.0, 20.0, 33.0] {
                    let _ = render(&face, gid, px);
                }
            }
        }
    }
}
