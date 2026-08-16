//! Render tree — backend-agnostic drawing primitives.
//!
//! The layout engine produces a list of `RenderCommand`s that any
//! rendering backend (compositor, framebuffer, software rasterizer)
//! can consume. This decouples the widget library from any specific
//! graphics API.

use crate::color::Color;
use crate::style::{Border, CornerRadii, Shadow};

/// What a renderer does with text that does not fit its `max_width`.
///
/// There is no default, and that is the point. `max_width` on its own poses a
/// question — *and if it doesn't fit?* — that used to be answered by silence:
/// the compositor stopped before the first glyph that would cross the limit and
/// drew no mark. A label reading `Gateway 192.168.1.1 res` is then
/// indistinguishable from a complete one, which is how a truncated path, a
/// clipped host name or a spoofed peer name gets read as the whole value.
/// Making this a required field means the question cannot be left unanswered by
/// accident — see `design-decisions.md` §427.
///
/// The renderer, not the caller, places the mark: it is the party that knows
/// exactly where the glyphs ran out, and it is about to walk them anyway. A
/// caller who elides first pays for a second measurement of the same string by
/// a second implementation, and the two can disagree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextOverflow {
    /// Stop at the limit and draw nothing to say so.
    ///
    /// Correct when the cut is not information the reader needs: a decorative
    /// rule, a progress bar's caption that is duplicated elsewhere, or text
    /// whose containing box the reader can plainly see the text is filling.
    /// Also the only sensible value when `max_width` is `None`, where the
    /// choice is vacuous.
    Clip,

    /// End the visible text with `…`, cutting one glyph earlier to make room.
    ///
    /// The right answer for anything variable-length — a file name, an SSID, an
    /// error string, anything from the wire or from another process — because
    /// the reader's alternative is to mistake a fragment for the whole.
    Ellipsis,
}

/// A render command — one drawing primitive.
#[derive(Clone, Debug)]
pub enum RenderCommand {
    /// Fill a rectangle.
    FillRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        corner_radii: CornerRadii,
    },

    /// Draw a rectangle outline (border).
    StrokeRect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        line_width: f32,
        corner_radii: CornerRadii,
    },

    /// Draw text.
    Text {
        x: f32,
        y: f32,
        text: String,
        color: Color,
        font_size: f32,
        font_weight: FontWeightHint,
        max_width: Option<f32>,
        /// What to do with the part that does not fit `max_width`. Vacuous —
        /// and so [`TextOverflow::Clip`] by convention — when `max_width` is
        /// `None`, since nothing can fail to fit an unbounded width.
        overflow: TextOverflow,
    },

    /// Draw an image/bitmap.
    Image {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        /// Image data ID (reference to image in an asset store).
        image_id: u64,
    },

    /// Draw a line.
    Line {
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        color: Color,
        width: f32,
    },

    /// Set a clip rectangle (all subsequent commands clipped to this area).
    PushClip {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },

    /// Remove the most recent clip rectangle.
    PopClip,

    /// Apply a transform (translate).
    PushTranslate { dx: f32, dy: f32 },

    /// Remove the most recent transform.
    PopTranslate,

    /// Draw subsequent [`Text`](RenderCommand::Text) in `family`.
    ///
    /// Scoped rather than carried on each `Text` command for the same reason
    /// the clip and the translate are: a family is a property of a *region* of
    /// the tree — a terminal pane, a code block — not of each individual
    /// string in it, and the alternative would have put a field on the ~2500
    /// places in this tree that build a `Text` command so that a handful of
    /// them could set it to something other than the default.
    ///
    /// A backend that does not understand this command draws everything in the
    /// UI face, which is what it did before the command existed.
    PushFont { family: FontFamily },

    /// Return to the family in force before the matching
    /// [`PushFont`](RenderCommand::PushFont).
    PopFont,

    /// Draw a box shadow.
    BoxShadow {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        offset_x: f32,
        offset_y: f32,
        blur: f32,
        spread: f32,
        color: Color,
        corner_radii: CornerRadii,
    },
}

/// Font weight hint for the renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontWeightHint {
    #[default]
    Regular,
    Bold,
    Light,
}

/// Which kind of face to draw text with.
///
/// The toolkit's mirror of [`osfont::system::Family`], kept separate for the
/// same reason [`FontWeightHint`] is kept separate from `osfont`'s `Weight`:
/// the render tree is the interface between an app and whatever draws it, and
/// it should not oblige every app to name the font crate.
///
/// [`Mono`](FontFamily::Mono) is a promise about the *metrics*, not about the
/// look: every glyph advances the same distance, so a caller may treat text as
/// a grid. A terminal is the case that needs it — with a proportional face,
/// column 40 of row 3 does not sit above column 40 of row 4.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontFamily {
    /// The system UI face. Proportional; what everything is drawn in unless
    /// it says otherwise.
    #[default]
    Ui,
    /// A fixed-pitch face.
    Mono,
}

/// Collected render output from a frame.
#[derive(Clone, Debug, Default)]
pub struct RenderTree {
    pub commands: Vec<RenderCommand>,
}

impl RenderTree {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn push(&mut self, cmd: RenderCommand) {
        self.commands.push(cmd);
    }

    pub fn fill_rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        self.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii: CornerRadii::ZERO,
        });
    }

    pub fn fill_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        radii: CornerRadii,
    ) {
        self.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii: radii,
        });
    }

    pub fn stroke_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
        line_width: f32,
    ) {
        self.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color,
            line_width,
            corner_radii: CornerRadii::ZERO,
        });
    }

    /// Outline a rounded rectangle.
    ///
    /// Takes the [`Border`] whole rather than a colour and a width side by
    /// side: a stroke's colour and thickness are one decision, and the pair is
    /// what the style system already hands around.
    pub fn stroke_rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        border: Border,
        radii: CornerRadii,
    ) {
        self.push(RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            color: border.color,
            line_width: border.width,
            corner_radii: radii,
        });
    }

    /// A drop shadow cast by the rectangle `(x, y, width, height)`.
    ///
    /// The shadow is emitted *before* the surface that casts it, since the
    /// command list is painted in order — a shadow pushed afterwards would be
    /// drawn on top of the thing it is supposed to sit behind.
    pub fn box_shadow(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        shadow: Shadow,
        radii: CornerRadii,
    ) {
        self.push(RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x: shadow.offset_x,
            offset_y: shadow.offset_y,
            blur: shadow.blur,
            spread: shadow.spread,
            color: shadow.color,
            corner_radii: radii,
        });
    }

    /// Draw text with no bound on how far right it may run.
    ///
    /// Correct only for text that has nothing to its right, or whose length is
    /// fixed and known to fit. For anything variable-length drawn into a column
    /// — a process name, a user name, a file path, anything from the wire or
    /// from another process — use [`RenderTree::text_in`]: this method cannot
    /// express a bound, so an over-long string is drawn straight over whatever
    /// is beside it.
    pub fn text(&mut self, x: f32, y: f32, text: &str, color: Color, font_size: f32) {
        self.push(RenderCommand::Text {
            x,
            y,
            text: text.to_string(),
            color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            // Vacuous: with no bound, nothing can fail to fit.
            overflow: TextOverflow::Clip,
        });
    }

    /// Draw text fitted to `width`, marking the cut with `…` if it did not fit.
    ///
    /// This is the one to reach for whenever the text is variable-length and
    /// something is drawn to the right of it. Two things happen, and both
    /// matter:
    ///
    /// - The string is elided to `width` **as measured at the size it will be
    ///   drawn at**, so it genuinely fits. A caller cannot get this right by
    ///   counting characters: a byte or `char` budget is not a width, and the
    ///   two only appear to agree on average-width ASCII.
    /// - The cut is *marked*. A silently clipped string is indistinguishable
    ///   from a short one, so the reader has no way to know they are looking at
    ///   a fragment — which is how a truncated path or a spoofed peer name gets
    ///   read as the whole thing.
    ///
    /// `max_width` is set as well, so the compositor's own clip is a backstop
    /// if the measured face and the drawn face ever disagree.
    pub fn text_in(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: &str,
        color: Color,
        font_size: f32,
    ) {
        self.text_in_weighted(x, y, width, text, color, font_size, FontWeightHint::Regular);
    }

    /// [`RenderTree::text_in`] for text drawn at a weight other than regular.
    ///
    /// Separate because the weight is not cosmetic here: bold glyphs are wider
    /// than regular ones at the same size, so measuring a bold string as
    /// regular under-measures it and lets a real overflow through.
    // The seven are the irreducible description of one piece of drawn text
    // (where, how wide, what, and in which face); bundling them into a struct
    // would only move the same seven fields to the call site, where they would
    // read worse than positional arguments matching the sibling primitives.
    #[allow(clippy::too_many_arguments)]
    pub fn text_in_weighted(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: &str,
        color: Color,
        font_size: f32,
        font_weight: FontWeightHint,
    ) {
        self.push(RenderCommand::Text {
            x,
            y,
            text: crate::text::elide(text, width, "…", font_size, font_weight),
            color,
            font_size,
            font_weight,
            max_width: Some(width),
            // Belt and braces, and deliberately not redundant. The string is
            // already elided above, so in the ordinary case the renderer finds
            // nothing to cut and this never fires. It fires exactly when the
            // measuring face and the drawing face disagree — and in that case
            // the old behaviour was to clip the difference away silently, which
            // is the failure this field exists to end.
            overflow: TextOverflow::Ellipsis,
        });
    }

    pub fn clip(&mut self, x: f32, y: f32, width: f32, height: f32) {
        self.push(RenderCommand::PushClip {
            x,
            y,
            width,
            height,
        });
    }

    pub fn unclip(&mut self) {
        self.push(RenderCommand::PopClip);
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.push(RenderCommand::PushTranslate { dx, dy });
    }

    pub fn untranslate(&mut self) {
        self.push(RenderCommand::PopTranslate);
    }

    /// Total number of draw commands.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Clear all commands (reuse allocation for next frame).
    pub fn clear(&mut self) {
        self.commands.clear();
    }
}

/// A tree is a sink for commands, the same as the bare `Vec<RenderCommand>`
/// the other half of the app tree builds.
///
/// Drawing helpers that emit several commands at once — `text::Paragraph`, for
/// one — take `&mut impl Extend<RenderCommand>` so that they work with either
/// shape. Without this a caller holding a tree would have to reach past it into
/// `commands`, which is exactly the kind of detail a helper should not make
/// its callers know about.
impl Extend<RenderCommand> for RenderTree {
    fn extend<T: IntoIterator<Item = RenderCommand>>(&mut self, iter: T) {
        self.commands.extend(iter);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn drawn(tree: &RenderTree) -> (&str, Option<f32>) {
        match tree.commands.first().expect("one command") {
            RenderCommand::Text {
                text, max_width, ..
            } => (text.as_str(), *max_width),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// The point of the primitive: what it emits genuinely fits the width it
    /// was given, measured at the size and weight it will be drawn at.
    #[test]
    fn fitted_text_measures_within_its_width() {
        for width in [20.0_f32, 60.0, 140.0, 400.0] {
            let mut tree = RenderTree::new();
            tree.text_in(0.0, 0.0, width, &"W".repeat(300), Color::rgb(0, 0, 0), 11.0);
            let (text, max_width) = drawn(&tree);
            assert_eq!(max_width, Some(width));
            let measured = crate::text::measure(text, 11.0, FontWeightHint::Regular);
            assert!(
                measured <= width + 0.5,
                "fitted text measures {measured} in a width of {width}",
            );
        }
    }

    /// A silent clip is the defect this exists to prevent, so the marker is
    /// part of the contract, not a nicety.
    #[test]
    fn text_that_did_not_fit_is_marked() {
        let mut tree = RenderTree::new();
        tree.text_in(0.0, 0.0, 60.0, &"W".repeat(300), Color::rgb(0, 0, 0), 11.0);
        assert!(drawn(&tree).0.ends_with('…'));
    }

    /// Text that fits is passed through untouched — eliding is for text that
    /// genuinely overflows, not a blanket shortening.
    #[test]
    fn text_that_fits_is_left_verbatim() {
        let mut tree = RenderTree::new();
        tree.text_in(0.0, 0.0, 400.0, "init", Color::rgb(0, 0, 0), 11.0);
        assert_eq!(drawn(&tree).0, "init");
    }

    /// Bold glyphs are wider than regular ones at the same size, so a bold
    /// string measured as regular under-measures and overflows anyway. This is
    /// why the weight-taking variant exists.
    #[test]
    fn a_bold_string_is_fitted_at_its_own_weight() {
        let sample = "The quick brown fox jumps over the lazy dog";
        let width = 120.0_f32;
        let mut tree = RenderTree::new();
        tree.text_in_weighted(
            0.0,
            0.0,
            width,
            sample,
            Color::rgb(0, 0, 0),
            13.0,
            FontWeightHint::Bold,
        );
        let (text, _) = drawn(&tree);
        let measured = crate::text::measure(text, 13.0, FontWeightHint::Bold);
        assert!(
            measured <= width + 0.5,
            "bold text measures {measured} in a width of {width}",
        );
    }

    /// A width too small even for the ellipsis must produce nothing rather than
    /// an ellipsis that itself overflows.
    #[test]
    fn an_impossible_width_draws_nothing() {
        let mut tree = RenderTree::new();
        tree.text_in(0.0, 0.0, 0.0, "anything at all", Color::rgb(0, 0, 0), 11.0);
        assert_eq!(drawn(&tree).0, "");
    }

    // ---- overflow policy (design-decisions.md §427) ------------------------

    fn overflow_of(tree: &RenderTree) -> TextOverflow {
        match tree.commands.first().expect("one command") {
            RenderCommand::Text { overflow, .. } => *overflow,
            other => panic!("expected Text, got {other:?}"),
        }
    }

    /// `text` sets no bound, so nothing can fail to fit and the policy is
    /// vacuous. `Clip` is the honest spelling of "this question does not
    /// arise"; `Ellipsis` here would suggest a cut that cannot happen.
    #[test]
    fn unbounded_text_declares_the_vacuous_policy() {
        let mut tree = RenderTree::new();
        tree.text(0.0, 0.0, "unbounded", Color::rgb(0, 0, 0), 11.0);
        assert_eq!(overflow_of(&tree), TextOverflow::Clip);
    }

    /// `text_in` already elides against its *measuring* face, so the field
    /// looks redundant. It is not: it fires exactly when the compositor's
    /// drawing face disagrees with the measurement, and before §427 that
    /// disagreement was resolved by silently cutting the difference away.
    #[test]
    fn fitted_text_asks_for_the_mark_even_though_it_already_elided() {
        let mut tree = RenderTree::new();
        tree.text_in(0.0, 0.0, 60.0, &"W".repeat(300), Color::rgb(0, 0, 0), 11.0);
        assert_eq!(overflow_of(&tree), TextOverflow::Ellipsis);
    }

    /// The policy is a property of the call site, not of the particular string:
    /// a bounded site that happens to be given something short this frame is
    /// still a bounded site next frame.
    #[test]
    fn a_bounded_site_asks_for_the_mark_even_when_the_text_fits() {
        let mut tree = RenderTree::new();
        tree.text_in(0.0, 0.0, 400.0, "init", Color::rgb(0, 0, 0), 11.0);
        assert_eq!(drawn(&tree).0, "init");
        assert_eq!(overflow_of(&tree), TextOverflow::Ellipsis);
    }

    #[test]
    fn the_weight_taking_variant_asks_for_the_mark_too() {
        let mut tree = RenderTree::new();
        tree.text_in_weighted(
            0.0,
            0.0,
            120.0,
            "The quick brown fox",
            Color::rgb(0, 0, 0),
            13.0,
            FontWeightHint::Bold,
        );
        assert_eq!(overflow_of(&tree), TextOverflow::Ellipsis);
    }
}
