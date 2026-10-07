//! Pictures: what an `<image>` shows, and what an `feImage` naming a picture
//! draws -- decoded from a `data:` URL, the one reference a drawing can make
//! without the renderer reaching outside it.
//!
//! # What is drawn
//!
//! - **Formats**: PNG, JPEG, GIF (its first frame), WebP, BMP, ICO and TIFF,
//!   chosen by what the bytes are rather than what the URL says
//!   (`imagecodec`), base64 or percent-encoded.
//! - **Placing**: `x`, `y`, `width` and `height` -- a size `auto` or not said
//!   being the picture's own, as SVG 2 has it -- with `preserveAspectRatio`
//!   fitting the picture into them, cut to them under `slice`.
//! - **Sampling**: between the picture's pixels, so a picture drawn larger or
//!   turned is smooth; nearest-pixel where `image-rendering` says
//!   `pixelated`, `crisp-edges` or `optimizeSpeed`.
//!
//! Not drawn: a picture named by a path or by any URL that is not `data:` --
//! a drawing is a file a user opened, and following its references would
//! read others they did not -- an SVG picture, and one past `imagecodec`'s
//! limits on size.

use std::collections::HashMap;

use super::XmlElement;
use crate::color::Color;

/// Every picture a document's `<image>`s and `feImage`s name, decoded once
/// each, however many name it.
#[derive(Default)]
pub(super) struct Pictures {
    /// The pictures, at the places [`Self::place`] gives.
    pub(super) decoded: Vec<Picture>,
    /// Each `href`, trimmed, and its picture's place -- `None` for one that
    /// names no picture this renderer draws.
    by_href: HashMap<String, Option<usize>>,
}

impl Pictures {
    /// The pictures the `<image>`s and `feImage`s under `root` name.
    pub(super) fn collect(root: &XmlElement) -> Self {
        let mut pictures = Self::default();
        pictures.gather(root);
        pictures
    }

    fn gather(&mut self, elem: &XmlElement) {
        if (elem.tag == "image" || elem.tag == "feImage")
            && let Some(href) = href(elem)
            && !href.starts_with('#')
            && !self.by_href.contains_key(href)
        {
            let place = Picture::decode(href).map(|picture| {
                self.decoded.push(picture);
                self.decoded.len().saturating_sub(1)
            });
            self.by_href.insert(href.to_owned(), place);
        }
        for child in &elem.children {
            self.gather(child);
        }
    }

    /// The place of the picture `href` names, if it names one that decoded.
    pub(super) fn place(&self, href: &str) -> Option<usize> {
        self.by_href.get(href.trim()).copied().flatten()
    }
}

/// What an element's `href` (SVG 2's, which wins) or `xlink:href` says,
/// trimmed.
pub(super) fn href(elem: &XmlElement) -> Option<&str> {
    elem.attr("href")
        .or_else(|| elem.attr("xlink:href"))
        .map(str::trim)
        .filter(|h| !h.is_empty())
}

/// A decoded picture: straight `[r, g, b, a]`, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Picture {
    pub(super) width: u32,
    pub(super) height: u32,
    pixels: Vec<u8>,
}

impl Picture {
    /// The picture a `data:` URL holds, if it holds one this renderer
    /// draws.
    pub(super) fn decode(href: &str) -> Option<Self> {
        let bytes = data_url(href)?;
        let image = imagecodec::decode(&bytes, imagecodec::Limits::default()).ok()?;
        let mut pixels = Vec::with_capacity(image.pixels.len().saturating_mul(4));
        for argb in &image.pixels {
            let [b, g, r, a] = argb.to_le_bytes();
            pixels.extend_from_slice(&[r, g, b, a]);
        }
        Some(Self {
            width: image.width,
            height: image.height,
            pixels,
        })
    }

    /// The pixel at `(x, y)`, held to the picture's edge.
    fn pixel(&self, x: i64, y: i64) -> [u8; 4] {
        let clamp = |v: i64, side: u32| {
            u64::try_from(v.clamp(0, i64::from(side).saturating_sub(1))).unwrap_or(0)
        };
        let at = clamp(y, self.height)
            .checked_mul(u64::from(self.width))
            .and_then(|row| row.checked_add(clamp(x, self.width)))
            .and_then(|i| i.checked_mul(4))
            .and_then(|i| usize::try_from(i).ok());
        at.and_then(|i| self.pixels.get(i..i.checked_add(4)?))
            .and_then(|p| <[u8; 4]>::try_from(p).ok())
            .unwrap_or([0; 4])
    }

    /// The colour at `(u, v)` in the picture's pixels -- their centres at
    /// half-way points -- held to its edge: between the four nearest where
    /// `smooth`, mixed with their alphas so a transparent pixel's colour
    /// does not bleed; the nearest one where not.
    pub(super) fn color_at(&self, u: f32, v: f32, smooth: bool) -> Color {
        if self.width == 0 || self.height == 0 || !(u.is_finite() && v.is_finite()) {
            return Color::rgba(0, 0, 0, 0);
        }
        if !smooth {
            let [r, g, b, a] = self.pixel(whole(u.floor()), whole(v.floor()));
            return Color::rgba(r, g, b, a);
        }
        let (x, y) = (u - 0.5, v - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (tx, ty) = (x - x0, y - y0);
        let (x0, y0) = (whole(x0), whole(y0));
        let mut sum = [0.0f32; 4];
        for (dx, dy, weight) in [
            (0, 0, (1.0 - tx) * (1.0 - ty)),
            (1, 0, tx * (1.0 - ty)),
            (0, 1, (1.0 - tx) * ty),
            (1, 1, tx * ty),
        ] {
            let [r, g, b, a] = self.pixel(x0.saturating_add(dx), y0.saturating_add(dy));
            let alpha = f32::from(a) / 255.0 * weight;
            sum[0] += f32::from(r) * alpha;
            sum[1] += f32::from(g) * alpha;
            sum[2] += f32::from(b) * alpha;
            sum[3] += alpha;
        }
        let [r, g, b, a] = sum;
        if a <= 0.0 {
            return Color::rgba(0, 0, 0, 0);
        }
        Color::rgba(byte(r / a), byte(g / a), byte(b / a), byte(a * 255.0))
    }
}

/// `v`, a whole number, as an `i64` held far inside its range.
fn whole(v: f32) -> i64 {
    // Clamped to +-2^40 first, so the cast is exact.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "clamped to +-2^40 first, and whole"
    )]
    let n = v.clamp(-1.0e12, 1.0e12) as i64;
    n
}

/// A colour channel, 0 to 255, rounded and held to a byte.
fn byte(v: f32) -> u8 {
    // Held to 0..=255 and rounded first.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "held to 0..=255 and rounded first"
    )]
    let b = (v + 0.5).clamp(0.0, 255.0) as u8;
    b
}

/// The bytes a `data:` URL carries -- RFC 2397: `data:[<type>][;base64],<data>`
/// -- or `None` for any other URL, a malformed one, or an SVG picture, which
/// this renderer does not draw inside another.
pub(super) fn data_url(href: &str) -> Option<Vec<u8>> {
    let href = href.trim();
    let scheme = href.get(..5)?;
    if !scheme.eq_ignore_ascii_case("data:") {
        return None;
    }
    let rest = href.get(5..)?;
    let (header, data) = rest.split_once(',')?;
    let mut params = header.split(';').map(str::trim);
    let media = params.next().unwrap_or("");
    if media.eq_ignore_ascii_case("image/svg+xml") {
        return None;
    }
    let base64_encoded = params.any(|p| p.eq_ignore_ascii_case("base64"));
    if base64_encoded {
        base64(&percent(data))
    } else {
        Some(percent(data))
    }
}

/// `text` with its `%XX` escapes decoded; anything else as its UTF-8 bytes.
fn percent(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while let Some(&b) = bytes.get(i) {
        let escaped = (b == b'%')
            .then(|| {
                let hi = hex(*bytes.get(i.checked_add(1)?)?)?;
                let lo = hex(*bytes.get(i.checked_add(2)?)?)?;
                Some(hi.wrapping_shl(4) | lo)
            })
            .flatten();
        match escaped {
            Some(decoded) => {
                out.push(decoded);
                i = i.saturating_add(3);
            }
            None => {
                out.push(b);
                i = i.saturating_add(1);
            }
        }
    }
    out
}

/// A hexadecimal digit's value.
fn hex(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(digit.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(digit.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// Base64 (RFC 4648's alphabet) decoded; spaces and line breaks -- which a
/// drawing's long URL is full of -- skipped, the `=` padding optional.
/// `None` where anything else is not base64, or the length cannot be.
pub(super) fn base64(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    let mut buffer = 0u32;
    let mut bits = 0u32;
    let mut ended = false;
    for &c in text {
        let value = match c {
            b'A'..=b'Z' => c.wrapping_sub(b'A'),
            b'a'..=b'z' => c.wrapping_sub(b'a').wrapping_add(26),
            b'0'..=b'9' => c.wrapping_sub(b'0').wrapping_add(52),
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                ended = true;
                continue;
            }
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => continue,
            _ => return None,
        };
        if ended {
            // Data after padding is not base64.
            return None;
        }
        // Bits above the twenty-four held fall off the top; only the last
        // few are ever read.
        buffer = buffer.wrapping_shl(6) | u32::from(value);
        bits = bits.saturating_add(6);
        if bits >= 8 {
            bits = bits.saturating_sub(8);
            // The top eight of the bits held.
            let [low, ..] = buffer.wrapping_shr(bits).to_le_bytes();
            out.push(low);
        }
    }
    // Six bits left over can make no byte: a length no encoder writes.
    (bits < 6).then_some(out)
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;
