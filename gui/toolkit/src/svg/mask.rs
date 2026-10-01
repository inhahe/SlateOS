//! Masks: what a `mask="url(#id)"` names, and how what it draws becomes
//! the share of each pixel the masked element keeps.
//!
//! # What is drawn
//!
//! - A `<mask>`'s content is drawn as any content is -- colours, gradients,
//!   strokes, `<use>`s, clips, opacity -- into a scratch surface over the
//!   region its rectangle covers, and each pixel's **luminance** times its
//!   alpha is the share the masked element keeps there (`mask-type:
//!   alpha`, its alpha alone). Then it is applied as a clip is: every
//!   pixel's coverage multiplied by it, masks within clips multiplying.
//! - **`maskUnits`**: the rectangle (`x`, `y`, `width`, `height`, by default
//!   -10%, -10%, 120% and 120%) in fractions of the element's box
//!   (`objectBoundingBox`, the default) or in user space; nothing outside it
//!   is kept. **`maskContentUnits`**: the content in user space (the
//!   default) or in fractions of the box.
//! - Luminance is the weighted sum of the red, green and blue the content is
//!   drawn in, by the coefficients CSS Masking gives (0.2125, 0.7154,
//!   0.0721), taken over the colours as they are -- as browsers and resvg
//!   take them, rather than linearising first.
//!
//! A `mask` that names no `<mask>` masks nothing, as CSS Masking says; one
//! measured against a box with no area keeps nothing. A mask whose content
//! uses masks is followed as deep as clip paths are, [`super::MAX_CLIP_DEPTH`].
//!
//! Not drawn: a `mask` on a `<mask>` itself.

use super::paint::Units;
use super::{SvgNode, XmlElement, property, viewport_length};

/// A `<mask>`, built: what it draws, and where.
#[derive(Clone, Debug)]
pub(super) struct MaskDef {
    /// What its rectangle is measured in.
    pub(super) units: Units,
    /// What its content is measured in.
    pub(super) content_units: Units,
    /// Its rectangle, `x, y, width, height`: fractions of the masked
    /// element's box in [`Units::ObjectBoundingBox`], user units otherwise.
    pub(super) rect: [f32; 4],
    /// Whether a pixel keeps its content's luminance times its alpha
    /// (`true`, the default) or its alpha alone.
    pub(super) luminance: bool,
    /// What it draws.
    pub(super) children: Vec<SvgNode>,
}

/// The parts of a `<mask>` that are not its content: its units, its
/// rectangle -- percentages of `viewport` where it is in user space -- and
/// its type.
pub(super) fn mask_frame(
    elem: &XmlElement,
    viewport: (f32, f32),
) -> (Units, Units, [f32; 4], bool) {
    let units_of = |name: &str, default: Units| match elem.attr(name).map(str::trim) {
        Some("objectBoundingBox") => Units::ObjectBoundingBox,
        Some("userSpaceOnUse") => Units::UserSpaceOnUse,
        _ => default,
    };
    let units = units_of("maskUnits", Units::ObjectBoundingBox);
    let content_units = units_of("maskContentUnits", Units::UserSpaceOnUse);
    let (view_w, view_h) = viewport;
    // A side measured in fractions of the box reads a percentage as one of 1;
    // one in user space, as one of the viewport's width or height.
    let side = |name: &str, extent: f32, default: f32| {
        let whole = match units {
            Units::ObjectBoundingBox => 1.0,
            Units::UserSpaceOnUse => extent,
        };
        elem.attr(name)
            .and_then(|value| viewport_length(value, whole))
            .unwrap_or(default * whole)
    };
    let rect = [
        side("x", view_w, -0.1),
        side("y", view_h, -0.1),
        side("width", view_w, 1.2),
        side("height", view_h, 1.2),
    ];
    let luminance = property(elem, "mask-type").is_none_or(|t| t.trim() != "alpha");
    (units, content_units, rect, luminance)
}

/// The share a mask keeps of a pixel its content is drawn in `[r, g, b, a]`
/// (straight alpha): luminance times alpha, or alpha alone.
pub(super) fn kept(pixel: [u8; 4], luminance: bool) -> u8 {
    let [r, g, b, a] = pixel;
    if !luminance {
        return a;
    }
    let lum = 0.2125 * f32::from(r) + 0.7154 * f32::from(g) + 0.0721 * f32::from(b);
    // In 0..=255: a weighted mean of bytes, times a share, rounded.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a mean of bytes times a share of one, held to 0..=255"
    )]
    let share = (lum * f32::from(a) / 255.0 + 0.5).clamp(0.0, 255.0) as u8;
    share
}

#[cfg(test)]
#[path = "mask_tests.rs"]
mod tests;
