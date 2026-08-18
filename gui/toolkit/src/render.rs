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

/// One coloured stretch of a [`RichText`](RenderCommand::RichText) string.
///
/// A span is a range of *bytes*, and deliberately not a range of glyphs: the
/// caller — a syntax highlighter, a diff view, a search-match highlighter —
/// knows where its runs begin and end in the text, and cannot know where they
/// begin and end in the glyphs, because that depends on the face, the size and
/// the shaping. The renderer performs that translation, since it is the party
/// doing the shaping.
///
/// Spans are cumulative: `end` is one past the last byte the colour covers, and
/// the span begins where the previous one ended (at 0 for the first). That
/// representation makes gaps and overlaps *unrepresentable* rather than merely
/// invalid — a start-and-end pair invites a caller to emit two spans that
/// disagree about a byte, and then the answer depends on which one the renderer
/// happens to consult first.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextSpan {
    /// Byte offset one past the last byte this colour applies to.
    ///
    /// `u32` rather than `usize` because this crosses to another process: the
    /// wire format needs a fixed width, and a 4 GiB line of text is not a case
    /// worth widening every span for.
    pub end: u32,
    /// What to draw those bytes in.
    pub color: Color,
}

impl TextSpan {
    /// The colour for a glyph whose source byte is `cluster`.
    ///
    /// Lives here, next to the representation, so that every backend resolves a
    /// span list the same way. Two backends that each write the obvious three
    /// lines will agree on the ordinary case and disagree on the boundary, and
    /// the boundary is every single colour change on the screen.
    ///
    /// A binary search rather than a forward walk because glyphs are *drawn* in
    /// visual order, where clusters do not ascend — a right-to-left word's
    /// glyphs run backwards through the string. The list is one entry per
    /// syntax token, so this is a handful of comparisons.
    ///
    /// `None` means no span covers the byte, rather than a fallback colour
    /// passed in and handed straight back: a backend holds its fallback in its
    /// own pixel format, and taking it here would oblige it to convert that
    /// colour into this one's form once per glyph only to convert it back.
    #[must_use]
    pub fn color_at(spans: &[Self], cluster: usize) -> Option<Color> {
        // The first span that has not already ended at `cluster`. Cumulative
        // spans mean that span also *starts* at or before `cluster`, so it is
        // the containing one; if there is none, the byte is past the last span.
        let i = spans.partition_point(|s| s.end as usize <= cluster);
        spans.get(i).copied().map(|s| s.color)
    }
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

    /// Draw one string in several colours, shaped as a single run.
    ///
    /// This is [`Text`](RenderCommand::Text) for text whose colour changes part
    /// way through — syntax highlighting, a diff, a highlighted search match —
    /// and it exists because the obvious alternative is wrong. Cutting the
    /// string at each colour change and drawing the pieces end to end assumes
    /// that screen order is byte order: it is, right up until the text contains
    /// a right-to-left run, at which point the pieces are laid out left to right
    /// while their glyphs belong interleaved. There is then no `x` at which a
    /// piece can be drawn correctly, because the piece is not contiguous on the
    /// screen. Colour has to be an attribute of a glyph, not of a substring to
    /// draw — which is what this command makes it.
    ///
    /// It is also *cheaper* than the decomposition it replaces, which is worth
    /// stating because the reverse is what one would guess. Shaping carries a
    /// fixed cost of a few microseconds on top of the per-character cost, and
    /// cutting a line into *n* pieces pays it *n* times: an 80-character line of
    /// 40 tokens measured 2.3x the cost of shaping it whole. See
    /// `known-issues.md` → `TD-EDITOR-IS-NOT-BIDIRECTIONAL`.
    ///
    /// Each glyph takes the colour of the span containing the byte it came
    /// from. Bytes past the last span — and the overflow mark, which belongs to
    /// no byte at all — take `color`.
    ///
    /// A backend that does not understand this command should draw the string
    /// in `color`, which is wrong only in its colours and never in its layout.
    RichText {
        x: f32,
        y: f32,
        text: String,
        /// Cumulative colour runs over `text`, in ascending order of `end`. May
        /// be empty, which is `Text` with extra steps.
        ///
        /// Offsets that are not on a character boundary, that do not ascend, or
        /// that run past the end of `text` are not an error the renderer
        /// reports: it resolves each glyph against whatever it is given and
        /// draws. A malformed list mis-colours; it never mis-positions, and it
        /// never fails to draw the text.
        spans: Vec<TextSpan>,
        /// The colour for bytes after the last span, and for the overflow mark.
        color: Color,
        font_size: f32,
        font_weight: FontWeightHint,
        max_width: Option<f32>,
        /// As [`Text`](RenderCommand::Text)'s: vacuous, and so
        /// [`TextOverflow::Clip`] by convention, when `max_width` is `None`.
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

    /// Draw one string whose colour changes part way through, shaped whole.
    ///
    /// The multi-coloured counterpart of [`RenderTree::text`], and unbounded for
    /// the same reason: use it where the text has nothing to its right, or clip
    /// the region yourself. See [`RenderCommand::RichText`] for why this is a
    /// primitive rather than a loop over [`RenderTree::text`] — briefly, the
    /// loop is the assumption that screen order is byte order, and it is both
    /// wrong under bidirectional text and slower on ordinary text.
    pub fn rich_text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        spans: Vec<TextSpan>,
        color: Color,
        font_size: f32,
    ) {
        self.push(RenderCommand::RichText {
            x,
            y,
            text: text.to_string(),
            spans,
            color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            // Vacuous: with no bound, nothing can fail to fit.
            overflow: TextOverflow::Clip,
        });
    }

    /// [`RenderTree::rich_text`] bounded to `width`, cutting without a mark.
    ///
    /// The unmarked cut is deliberate and is the one case §427 exempts: this is
    /// for a *viewport* onto text that visibly continues — a code editor's line,
    /// a log pane — where the reader can see the text filling the pane and has a
    /// scrollbar to reach the rest. An ellipsis there would claim the line ends
    /// at the window edge, which is the opposite of true. Use it only where that
    /// holds; for a rich-text *label* in a column, the cut is real information
    /// and wants a mark.
    ///
    /// The bound is not only about honesty: without it the renderer shapes and
    /// blits every glyph of a line that may be thousands of columns wide, of
    /// which a screenful is visible. `max_width` is what lets it stop.
    // The eight are [`RenderTree::text_in`]'s seven plus the spans, and the same
    // argument applies: they are the irreducible description of one piece of
    // drawn text, and a struct would only move them to the call site, where they
    // would read worse than positional arguments matching the sibling
    // primitives.
    #[allow(clippy::too_many_arguments)]
    pub fn rich_text_clipped(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        text: &str,
        spans: Vec<TextSpan>,
        color: Color,
        font_size: f32,
    ) {
        self.push(RenderCommand::RichText {
            x,
            y,
            text: text.to_string(),
            spans,
            color,
            font_size,
            font_weight: FontWeightHint::Regular,
            max_width: Some(width),
            overflow: TextOverflow::Clip,
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

    // ---- rich text ---------------------------------------------------------

    const RED: Color = Color::rgb(255, 0, 0);
    const GREEN: Color = Color::rgb(0, 255, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    fn spans(ends: &[(u32, Color)]) -> Vec<TextSpan> {
        ends.iter()
            .map(|&(end, color)| TextSpan { end, color })
            .collect()
    }

    /// The basic contract: a byte inside a span takes that span's colour, and
    /// the span it is inside is the first one that has not already ended.
    #[test]
    fn a_byte_takes_the_colour_of_the_span_containing_it() {
        let s = spans(&[(3, RED), (6, GREEN)]);
        for (byte, want) in [(0, RED), (1, RED), (2, RED), (3, GREEN), (5, GREEN)] {
            assert_eq!(
                TextSpan::color_at(&s, byte),
                Some(want),
                "byte {byte} resolved wrongly",
            );
        }
    }

    /// `end` is exclusive, so the boundary byte belongs to the *next* span. This
    /// is the off-by-one that decides the colour of every character adjacent to
    /// a colour change on the screen — which is to say, all of the interesting
    /// ones.
    #[test]
    fn a_span_end_is_exclusive() {
        let s = spans(&[(1, RED), (2, GREEN)]);
        assert_eq!(TextSpan::color_at(&s, 0), Some(RED));
        assert_eq!(TextSpan::color_at(&s, 1), Some(GREEN));
    }

    /// Past the last span there is no answer to give, and the *backend's* own
    /// fallback is the right one — see the method's doc for why it is not passed
    /// in and handed back.
    #[test]
    fn a_byte_past_every_span_has_no_colour() {
        assert_eq!(TextSpan::color_at(&spans(&[(3, RED)]), 3), None);
        assert_eq!(TextSpan::color_at(&spans(&[(3, RED)]), 900), None);
        assert_eq!(TextSpan::color_at(&[], 0), None);
    }

    /// An empty span — one whose `end` equals the previous span's — covers no
    /// byte and must never win. A tokenizer emitting a zero-length token, or a
    /// scroll position that clamps two token starts to the same offset, produces
    /// exactly this.
    #[test]
    fn an_empty_span_colours_nothing() {
        let s = spans(&[(2, RED), (2, GREEN), (4, BLUE)]);
        assert_eq!(TextSpan::color_at(&s, 0), Some(RED));
        assert_eq!(TextSpan::color_at(&s, 1), Some(RED));
        assert_eq!(TextSpan::color_at(&s, 2), Some(BLUE));
        assert_eq!(TextSpan::color_at(&s, 3), Some(BLUE));
    }

    /// The lookup is a binary search, so it must not assume it is walked in
    /// ascending order — glyphs are *drawn* in visual order, where a
    /// right-to-left word's clusters run backwards. Querying the same list
    /// backwards must give the same answers.
    #[test]
    fn resolution_does_not_depend_on_query_order() {
        let s = spans(&[(2, RED), (5, GREEN), (9, BLUE)]);
        let forward: Vec<_> = (0..9).map(|b| TextSpan::color_at(&s, b)).collect();
        let mut backward: Vec<_> = (0..9).rev().map(|b| TextSpan::color_at(&s, b)).collect();
        backward.reverse();
        assert_eq!(forward, backward);
    }

    fn rich(tree: &RenderTree) -> (&str, &[TextSpan], Option<f32>, TextOverflow) {
        match tree.commands.first().expect("one command") {
            RenderCommand::RichText {
                text,
                spans,
                max_width,
                overflow,
                ..
            } => (text.as_str(), spans.as_slice(), *max_width, *overflow),
            other => panic!("expected RichText, got {other:?}"),
        }
    }

    /// One command, not one per colour: the whole point is that the string is
    /// shaped once. A helper that quietly emitted a command per span would pass
    /// every colour assertion and reintroduce the bug.
    #[test]
    fn rich_text_emits_a_single_command() {
        let mut tree = RenderTree::new();
        tree.rich_text(
            0.0,
            0.0,
            "let x = 1;",
            spans(&[(3, RED), (5, GREEN), (10, BLUE)]),
            Color::rgb(0, 0, 0),
            11.0,
        );
        assert_eq!(tree.len(), 1);
        let (text, s, max_width, overflow) = rich(&tree);
        assert_eq!(text, "let x = 1;");
        assert_eq!(s.len(), 3);
        // Unbounded, so the overflow question is vacuous — as for `text`.
        assert_eq!(max_width, None);
        assert_eq!(overflow, TextOverflow::Clip);
    }

    /// The viewport variant bounds the run and says so, and does *not* ask for a
    /// mark: the text visibly continues past the pane edge, and an ellipsis
    /// there would claim it ends.
    #[test]
    fn clipped_rich_text_is_bounded_and_unmarked() {
        let mut tree = RenderTree::new();
        tree.rich_text_clipped(
            0.0,
            0.0,
            120.0,
            "let x = 1;",
            spans(&[(3, RED)]),
            Color::rgb(0, 0, 0),
            11.0,
        );
        let (text, _, max_width, overflow) = rich(&tree);
        // Not elided by the helper: the renderer cuts at the pixel, so the
        // string arrives whole and the cut lands where the glyphs actually run
        // out rather than where a second measurement guessed they would.
        assert_eq!(text, "let x = 1;");
        assert_eq!(max_width, Some(120.0));
        assert_eq!(overflow, TextOverflow::Clip);
    }

    /// An empty span list is `Text` with extra steps, and must behave like it —
    /// every byte falls through to the fallback colour.
    #[test]
    fn rich_text_with_no_spans_is_uniformly_the_fallback() {
        let mut tree = RenderTree::new();
        tree.rich_text(0.0, 0.0, "plain", Vec::new(), RED, 11.0);
        let (text, s, _, _) = rich(&tree);
        assert_eq!(text, "plain");
        assert!(s.is_empty());
        assert!((0..5).all(|b| TextSpan::color_at(s, b).is_none()));
    }
}
