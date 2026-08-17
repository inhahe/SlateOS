//! Turning a string into the glyphs that get drawn, in the order and at the
//! spacing the font asks for.
//!
//! # Why a run, and not a loop over `chars()`
//!
//! Every text path in the OS used to be some variant of
//!
//! ```text
//! for ch in text.chars() { let g = font.glyph(ch); pen += g.advance; }
//! ```
//!
//! which quietly assumes three things that are all false of real fonts:
//!
//! 1. **One character makes one glyph.** `fi` is one glyph in most serif
//!    faces — the `f`'s hood and the `i`'s dot collide, so the designer drew
//!    a joined form and told the font to use it.
//! 2. **A glyph's width does not depend on its neighbours.** It does; that is
//!    what kerning is.
//! 3. **Every loop that walks the text will make the same assumptions.** They
//!    did not. Adding kerning to `measure` alone left the toolkit's caret
//!    placement and its ellipsis truncation summing unkerned advances, so a
//!    click landed one place and the caret another — on kerned text only, and
//!    by a few pixels, which is the kind of bug that gets reported as "the
//!    text feels wrong".
//!
//! A [`ShapedRun`] answers all three at once. It is produced once per string,
//! and *every* consumer — measuring, drawing, hit-testing, truncating — walks
//! the same list. They cannot disagree, because there is nothing left to
//! disagree about: the advances are already final.
//!
//! # Clusters
//!
//! [`ShapedGlyph::cluster`] is the byte offset in the source string of the
//! first character the glyph came from. It is what makes ligatures safe for
//! the rest of the system: a caret cannot land inside `fi`, because no
//! shaped glyph starts there, and a truncation cannot cut between the `f` and
//! the `i`, because they are one entry. Callers that need character indices
//! rather than byte offsets convert at the edge; byte offsets are what
//! actually slice a string.
//!
//! # Logical order and drawn order
//!
//! A run stores its glyphs in **logical** order — the order the characters
//! were typed in — and carries the permutation that puts them in the order
//! they are **drawn** in, which differs only where the text contains a
//! right-to-left stretch. Every query about the *text* (a cut, a width, a
//! cluster) walks the logical order; every query about the *screen* (a caret,
//! a hit test) walks the drawn one. Confusing the two is not a rounding error:
//! it puts the caret at the wrong end of a Hebrew word.
//!
//! Because the two orders differ, one byte offset can have two legitimate
//! caret positions — see [`Affinity`].

use alloc::vec::Vec;

use crate::bidi::Level;

/// How wide a tab is, in spaces.
///
/// A real tab is a *stop* — an absolute column the pen jumps to — which a
/// shaper cannot compute, because it does not know where in the line the run
/// starts. A fixed multiple of the space advance is the approximation every
/// simple text engine makes, and it is what the built-in face's measurement
/// already assumed; naming it here stops the two backends from each picking
/// their own number. A terminal, which does know its columns, must expand
/// tabs itself before shaping.
pub const TAB_WIDTH_IN_SPACES: f32 = 4.0;

/// Which glyph to draw, in the font that produced it.
///
/// Opaque, and deliberately: an outline face numbers its glyphs and the
/// built-in bitmap face is keyed by character, and a caller that could tell
/// the two apart would start branching on it — which is exactly what
/// [`SystemFont`](crate::system::SystemFont) exists to stop. A key is only
/// meaningful to the font that shaped it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlyphKey(u32);

impl GlyphKey {
    /// A glyph id in an outline face.
    pub(crate) fn outline(gid: u16) -> Self {
        Self(u32::from(gid))
    }

    /// A character in the built-in bitmap face, which has no glyph ids.
    pub(crate) fn bitmap(ch: char) -> Self {
        Self(ch as u32)
    }

    /// The outline glyph id this key holds.
    ///
    /// Zero (`.notdef`) if the value does not fit, which can only happen if a
    /// key from a bitmap font is handed to an outline font — a font mix-up,
    /// for which drawing the missing-glyph box is the right answer.
    pub(crate) fn gid(self) -> u16 {
        u16::try_from(self.0).unwrap_or(0)
    }

    /// The character this key holds, or the replacement character if the
    /// value is not a scalar value — again only reachable by mixing fonts.
    pub(crate) fn ch(self) -> char {
        char::from_u32(self.0).unwrap_or(char::REPLACEMENT_CHARACTER)
    }

    /// The raw value behind this key. **For diagnostics only.**
    ///
    /// It is a glyph id in an outline face and a character in the bitmap one,
    /// which is precisely the distinction this type exists to hide — so
    /// nothing that *draws* may call it. It exists for the tools that check
    /// this crate against an external shaper: they report glyph ids, and
    /// cannot be compared against without speaking in them.
    #[must_use]
    pub fn raw(self) -> u32 {
        self.0
    }
}

/// One glyph of a shaped run.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Which glyph to draw.
    pub key: GlyphKey,
    /// Byte offset in the shaped string of the first character this glyph
    /// came from.
    ///
    /// The mapping is many-to-many in both directions. Several *characters*
    /// share one cluster when they ligate: `ffi` is one glyph reporting the
    /// offset of the `f`. Several *glyphs* share one cluster when a character
    /// decomposes: GSUB LookupType 2 turns one glyph into a sequence, and
    /// every glyph of that sequence reports the offset of the character it
    /// came from.
    ///
    /// So a cluster is not an index and glyph *n* is not character *n*. What
    /// it is is a boundary: a caret, a cut and a hit test may only land where
    /// a cluster starts, because nowhere else in the run corresponds to a
    /// position in the string. The queries on [`ShapedRun`] all work in whole
    /// clusters for that reason. Clusters are non-decreasing along a run.
    pub cluster: usize,
    /// How far to move the pen after drawing this glyph, in pixels.
    ///
    /// The glyph's own advance **plus** any kerning against the glyph that
    /// follows it. Folding the kern into the preceding advance rather than
    /// exposing it separately is what makes the sum of a run equal to its
    /// width, and what stops a caller from drawing a run correctly while
    /// measuring it wrongly.
    pub advance: f32,
    /// How much of `advance` is the correction against the *following* glyph.
    /// Zero on the last glyph of a run, and on every glyph of an unkerned
    /// face.
    ///
    /// It has to be recoverable, because a run that gets cut short loses the
    /// glyph that correction was for. Slicing `"AVATAR"` after the `A` and
    /// drawing the `A` alone must not leave in the pull-in that only made
    /// sense with the `V` behind it — that is a real off-by-a-pixel: the
    /// truncated string measures *wider* than the budget it was cut to fit.
    pub kern_next: f32,
    /// Where to draw this glyph relative to the pen, in pixels, `y` upwards.
    ///
    /// Zero for everything except an attached combining mark, which is drawn
    /// on top of the glyph before it rather than after it. The offset does not
    /// move the pen — it is a displacement of the ink only, which is what lets
    /// a mark sit over a base that has already been advanced past.
    pub offset: (f32, f32),
}

/// Which of the two characters meeting at a byte offset a caret belongs to.
///
/// In left-to-right text this is a distinction without a difference: the end
/// of one character and the start of the next are the same point on the
/// screen. In bidirectional text they are not. In `hello שלום world` the
/// offset just after `hello` is both "after the `o`" — which is drawn at the
/// left, where `hello` ends — and "before the `ש`" — which is drawn at the
/// *right* end of the Hebrew word, because that word runs the other way. Both
/// are correct; which one the user means depends on which way they were
/// moving.
///
/// So a caret is a byte offset *and* one of these. Every mature text engine
/// models it: ICU calls it leading and trailing, DirectWrite calls it a
/// trailing hit, Chromium calls it affinity. Answering with one x and calling
/// it done just moves the wrongness somewhere less visible.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Affinity {
    /// The caret belongs to the character that **starts** at the offset — the
    /// one the next keystroke would push along. This is what a caret has when
    /// it arrived by moving backwards through the text, and what a click on
    /// the leading half of a character means.
    ///
    /// The default, because it is what a caller with no opinion wants: on
    /// left-to-right text the two agree, and at the very start of a run there
    /// is nothing upstream for the other to name.
    #[default]
    Downstream,
    /// The caret belongs to the character that **ends** at the offset — the
    /// one a backspace would delete. This is what a caret has after typing,
    /// and what a click on the trailing half of a character means.
    Upstream,
}

/// Where a click landed: a byte offset, and which side of it the caret is on.
///
/// The affinity is not decoration. Clicking the left edge of the `ש` in
/// `hello שלום world` and clicking the right edge of the `o` before it give
/// the *same* byte offset and different affinities, and a caret drawn without
/// the second one lands under whichever of the two words the implementation
/// happened to prefer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hit {
    /// The byte offset in the shaped string. Always a cluster boundary or the
    /// end of the string — the only two kinds of position that exist.
    pub offset: usize,
    /// Which of the two characters meeting at `offset` the caret belongs to.
    pub affinity: Affinity,
}

/// One cluster as the screen sees it: where it is drawn, and the caret that
/// belongs at each of its two edges.
///
/// Built only by [`ShapedRun::visual_clusters`], which is the whole of what
/// visual-order caret motion needs — the sequence of clusters left to right,
/// and for each of them the byte position a caret arriving at either edge
/// should report. Keeping the two edges as [`Hit`]s rather than as a
/// direction flag plus a byte range is deliberate: the swap that
/// right-to-left text performs happens once, here, instead of at every place
/// that asks which edge it is looking at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VisualCluster {
    /// The leftmost drawn slot any of the cluster's glyphs occupies. Orders
    /// the clusters across the screen; not otherwise used.
    leftmost: usize,
    /// The caret at the cluster's left edge on the screen.
    left: Hit,
    /// The caret at its right edge.
    right: Hit,
}

impl VisualCluster {
    /// Is this cluster read right to left? Recovered from the edges rather
    /// than stored again: a left-to-right cluster's left edge is the boundary
    /// *before* its first character, which is the `Downstream` one, and a
    /// right-to-left cluster's left edge is the boundary after its last.
    const fn is_rtl(self) -> bool {
        matches!(self.left.affinity, Affinity::Upstream)
    }

    /// The source bytes the cluster covers, `start..next`, in logical terms —
    /// which is the `Downstream` edge's offset and the `Upstream` edge's,
    /// whichever way round they are drawn.
    const fn byte_range(self) -> (usize, usize) {
        if self.is_rtl() {
            (self.right.offset, self.left.offset)
        } else {
            (self.left.offset, self.right.offset)
        }
    }
}

/// The glyphs a string turns into, ready to draw.
///
/// The glyphs are in **logical** order — the order the characters were typed
/// in — and [`draw_order`](Self::draw_order) gives the order they are drawn
/// in, which differs only for text containing a right-to-left run. Keeping
/// logical order as the storage order is what lets every query below stay in
/// terms of the string: a cluster is a byte offset, clusters only ever
/// increase along the array, and a cut is a position in the text rather than a
/// position on the screen. The queries that *are* about the screen —
/// [`x_of`](Self::x_of) and [`offset_at`](Self::offset_at) — walk the drawn
/// order instead, and say so.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedRun {
    glyphs: Vec<ShapedGlyph>,
    /// Logical index of each glyph, in the order they are drawn (rule L2), or
    /// empty when that is the identity — which it is for all left-to-right
    /// text, and so for nearly every run this crate ever builds.
    visual: Vec<u32>,
    /// Bidi embedding level per glyph, or empty when every glyph is
    /// left-to-right.
    ///
    /// Kept even where `visual` is the identity, because the two answer
    /// different questions: reversing a *one-glyph* right-to-left run is the
    /// identity permutation, and that glyph still starts at its right edge. A
    /// caret that read the permutation alone would put itself at the wrong end
    /// of a lone Hebrew letter between two Latin words.
    levels: Vec<Level>,
}

impl ShapedRun {
    /// Build a run from its glyphs, drawn in the order they are given.
    /// Crate-internal: only a font knows how to produce a correct one.
    pub(crate) fn new(glyphs: Vec<ShapedGlyph>) -> Self {
        Self {
            glyphs,
            visual: Vec::new(),
            levels: Vec::new(),
        }
    }

    /// The same, for a run whose glyphs are drawn in some other order.
    ///
    /// `visual` is one logical index per glyph, in drawing order — rule L2's
    /// output. A `visual` that is the identity is dropped, so that the common
    /// case costs nothing to carry and [`is_reordered`](Self::is_reordered)
    /// answers what it says it does.
    ///
    /// `levels` is one bidi embedding level per glyph, in *logical* order. It
    /// is dropped when every level is even, and kept otherwise even if the
    /// permutation was the identity — see the field.
    pub(crate) fn reordered(glyphs: Vec<ShapedGlyph>, visual: Vec<u32>, levels: Vec<Level>) -> Self {
        let identity = visual
            .iter()
            .enumerate()
            .all(|(i, &v)| usize::try_from(v).is_ok_and(|v| v == i));
        let rtl = levels.iter().any(|l| !l.is_multiple_of(2));
        Self {
            glyphs,
            visual: if identity { Vec::new() } else { visual },
            levels: if rtl { levels } else { Vec::new() },
        }
    }

    /// The glyphs in **logical** order: glyph *n* comes from text no earlier
    /// than glyph *n − 1*.
    ///
    /// This is the order to walk for anything about the *text* — clusters,
    /// carets, cuts. To put ink on a screen use [`draw_order`](Self::draw_order)
    /// instead, which is the same glyphs rearranged by rule L2. The two agree
    /// unless the run contains right-to-left text.
    #[must_use]
    pub fn glyphs(&self) -> &[ShapedGlyph] {
        &self.glyphs
    }

    /// The glyphs in the order they are drawn, left to right.
    ///
    /// Each glyph's `advance` moves the pen and each `offset` displaces its
    /// ink, exactly as in a run that needed no reordering: the advances are
    /// computed for *this* order, so a caller draws a bidirectional run with
    /// the same three lines it draws an English one with.
    pub fn draw_order(&self) -> impl Iterator<Item = &ShapedGlyph> + '_ {
        let by_index = self
            .visual
            .iter()
            .filter_map(|&v| self.glyphs.get(usize::try_from(v).ok()?));
        // `chain` rather than a branch returning two types: the logical
        // iterator is empty exactly when `visual` is populated.
        let in_order: &[ShapedGlyph] = if self.visual.is_empty() {
            &self.glyphs
        } else {
            &[]
        };
        in_order.iter().chain(by_index)
    }

    /// Is the drawing order different from the logical order?
    ///
    /// True only for a run containing right-to-left text. A caller that wants
    /// to know whether its own cluster arithmetic is still geometrically
    /// meaningful — a caret, a selection highlight — asks this.
    #[must_use]
    pub fn is_reordered(&self) -> bool {
        !self.visual.is_empty()
    }

    /// Total width in pixels.
    #[must_use]
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// Whether the run drew nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// How many glyphs the run has, which is *not* the character count.
    #[must_use]
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// One past the last glyph sharing the cluster of the glyph at `i`.
    ///
    /// A cluster is the unit every query below works in, because it is the
    /// only unit that corresponds to a position in the source string. A
    /// decomposed character is several glyphs that must be counted, kept and
    /// dropped together — taking half of them would report a width for a
    /// string that cannot be sliced there.
    fn group_end(&self, i: usize) -> usize {
        let Some(cluster) = self.glyphs.get(i).map(|g| g.cluster) else {
            return i;
        };
        self.glyphs
            .iter()
            .enumerate()
            .skip(i)
            .find(|(_, g)| g.cluster != cluster)
            .map_or(self.glyphs.len(), |(j, _)| j)
    }

    /// The first glyph sharing the cluster of the glyph *before* `i`, i.e. the
    /// start of the last cluster of `self.glyphs[..i]`. The mirror of
    /// [`group_end`](Self::group_end), for the queries that walk backwards.
    fn group_start(&self, i: usize) -> usize {
        let Some(cluster) = i.checked_sub(1).and_then(|j| self.glyphs.get(j)).map(|g| g.cluster)
        else {
            return i;
        };
        self.glyphs
            .iter()
            .take(i)
            .rposition(|g| g.cluster != cluster)
            .map_or(0, |j| j.saturating_add(1))
    }

    /// Total advance of `self.glyphs[from..to]`.
    fn span_width(&self, from: usize, to: usize) -> f32 {
        self.glyphs
            .get(from..to)
            .map_or(0.0, |g| g.iter().map(|g| g.advance).sum())
    }

    /// Is the glyph at logical index `i` drawn right to left?
    fn is_rtl(&self, i: usize) -> bool {
        self.levels.get(i).is_some_and(|l| !l.is_multiple_of(2))
    }

    /// Where the drawn order puts the glyph at logical index `i`, counting
    /// from the left of the run. The identity when nothing was reordered.
    fn slot_of(&self, i: usize) -> usize {
        if self.visual.is_empty() {
            return i;
        }
        u32::try_from(i)
            .ok()
            .and_then(|i| self.visual.iter().position(|&v| v == i))
            .unwrap_or(i)
    }

    /// The left edge and the width of the cluster `glyphs[from..to]`, in
    /// pixels from the left of the run as drawn.
    ///
    /// A cluster occupies one contiguous box on the screen whatever the
    /// reordering did, because rule L2 reverses whole level runs and a cluster
    /// lies inside one: its glyphs may be drawn in the other order, but they
    /// are still drawn side by side. So the box starts at the leftmost slot
    /// any of them occupies and is as wide as their advances together.
    fn cluster_box(&self, from: usize, to: usize) -> (f32, f32) {
        let leftmost = (from..to).map(|i| self.slot_of(i)).min().unwrap_or(0);
        let left: f32 = self.draw_order().take(leftmost).map(|g| g.advance).sum();
        (left, self.span_width(from, to))
    }

    /// Where the cluster `glyphs[from..to]` *starts* as a reader reads it:
    /// its left edge when it is drawn left to right, its right edge when it is
    /// not. The caret before the cluster's first character goes here.
    fn leading_edge(&self, from: usize, to: usize) -> f32 {
        let (left, width) = self.cluster_box(from, to);
        if self.is_rtl(from) { left + width } else { left }
    }

    /// The mirror: where the cluster ends as a reader reads it. The caret
    /// after its last character goes here.
    fn trailing_edge(&self, from: usize, to: usize) -> f32 {
        let (left, width) = self.cluster_box(from, to);
        if self.is_rtl(from) { left } else { left + width }
    }

    /// The byte offset at which to cut the source string so that what remains
    /// is at most `max_width` wide.
    ///
    /// Cuts between glyphs, so a ligature is kept or dropped whole: half of
    /// an `fi` is not a character, and a string sliced there is not valid
    /// UTF-8 either.
    ///
    /// `end` is the source string's length, returned when everything fits —
    /// the run cannot know it, because the last glyph's cluster is where the
    /// last character *starts*.
    #[must_use]
    pub fn fit(&self, max_width: f32, end: usize) -> usize {
        let mut used = 0.0;
        // This one needs no cluster grouping, unlike its mirror: it returns
        // the failing glyph's *cluster*, and every glyph of a decomposed
        // character carries the same one, so stopping part-way through a
        // character still cuts before the whole of it.
        for glyph in &self.glyphs {
            // The caller slices the string here and draws the piece on its
            // own, so the width to test is the prefix's width *alone*: this
            // glyph's kern against the one about to be dropped goes with it.
            if used + glyph.advance - glyph.kern_next > max_width {
                return glyph.cluster;
            }
            used += glyph.advance;
        }
        end
    }

    /// The byte offset at which the longest *suffix* fitting `max_width`
    /// begins.
    ///
    /// The mirror of [`fit`](Self::fit), for the cases where the end of the
    /// string is the part worth keeping — a filesystem path, where the
    /// filename matters and the leading directories do not.
    ///
    /// No `kern_next` correction is needed in this direction: kerning is
    /// charged to the *preceding* glyph, so a suffix's leading glyph never
    /// carried one, and its trailing glyph is the run's own last, whose
    /// `kern_next` is already zero.
    #[must_use]
    pub fn fit_end(&self, max_width: f32, end: usize) -> usize {
        let mut used = 0.0;
        let mut start = end;
        let mut i = self.glyphs.len();
        // A whole cluster at a time, not a glyph at a time. Taking one glyph
        // of a decomposed character and stopping would return a byte offset
        // that re-shapes into *both* of its glyphs — a suffix measured at one
        // width and drawn at another, which is the divergence this type exists
        // to prevent.
        while i > 0 {
            let from = self.group_start(i);
            let width = self.span_width(from, i);
            if used + width > max_width {
                return start;
            }
            used += width;
            start = self.glyphs.get(from).map_or(start, |g| g.cluster);
            i = from;
        }
        0
    }

    /// Where a click `offset` pixels across the run puts the caret.
    ///
    /// This is what a click on a line of text means: the caret goes to the
    /// nearest boundary between characters, not to the character the click
    /// landed inside, so clicking the right half of an English letter puts the
    /// caret after it.
    ///
    /// Measured across the **screen**, in drawn order, which is the only thing
    /// a pixel coordinate can mean. In bidirectional text that is not the same
    /// walk as the logical one: clicking the left half of a Hebrew letter puts
    /// the caret *after* it, because a right-to-left letter is read from its
    /// right edge. The returned [`Affinity`] records which side of the
    /// boundary the click was on, which is what lets the caret be drawn back
    /// at the edge the user aimed at rather than at the other end of the word.
    ///
    /// `end` is the source string's length, for a click past the last glyph.
    #[must_use]
    pub fn offset_at(&self, offset: f32, end: usize) -> Hit {
        let downstream = |at: usize| Hit {
            offset: at,
            affinity: Affinity::Downstream,
        };
        let upstream = |at: usize| Hit {
            offset: at,
            affinity: Affinity::Upstream,
        };
        if self.glyphs.is_empty() {
            return downstream(end);
        }
        let mut i = 0usize;
        let mut leftmost: Option<(f32, Hit, Hit)> = None;
        let mut rightmost: Option<(f32, Hit, Hit)> = None;
        // Every cluster's box is tested rather than walked in order, because
        // the boxes are laid out in *drawn* order and this loop is in logical
        // order; the two agree only when nothing was reordered. They tile the
        // run with no gaps either way, so at most one box contains the click.
        while i < self.glyphs.len() {
            let to = self.group_end(i);
            let (left, width) = self.cluster_box(i, to);
            let start = self.glyphs.get(i).map_or(end, |g| g.cluster);
            let next = self.glyphs.get(to).map_or(end, |g| g.cluster);
            // Nearer the leading edge means the caret is *before* this
            // character; nearer the trailing edge means after it. Which of
            // those is the left half depends on the direction, and that is the
            // whole of the right-to-left case: the left half of a Hebrew
            // letter is its trailing half.
            let (in_left, in_right) = if self.is_rtl(i) {
                (upstream(next), downstream(start))
            } else {
                (downstream(start), upstream(next))
            };
            // The midpoint that matters is the *cluster's*, not a glyph's: a
            // character drawn as two glyphs has one boundary before it and one
            // after, and a click in its left half belongs to the first of
            // them. Splitting on each glyph's own midpoint would put the
            // tipping point three quarters of the way across the character.
            if offset >= left && offset < left + width {
                return if offset < left + width / 2.0 {
                    in_left
                } else {
                    in_right
                };
            }
            if leftmost.is_none_or(|(x, _, _)| left < x) {
                leftmost = Some((left, in_left, in_right));
            }
            if rightmost.is_none_or(|(x, _, _)| left > x) {
                rightmost = Some((left, in_left, in_right));
            }
            i = to;
        }
        // Off the end of the line on one side or the other. A click to the
        // left of everything is a click at the left edge of the leftmost
        // character, and the caret goes to whichever of its two boundaries is
        // drawn there — which for right-to-left text is the one *after* it.
        let past = if offset < 0.0 { leftmost } else { rightmost };
        past.map_or_else(
            || downstream(end),
            |(_, in_left, in_right)| if offset < 0.0 { in_left } else { in_right },
        )
    }

    /// Where to draw the caret for byte offset `at`, in pixels across the run.
    /// `end` is the source string's length.
    ///
    /// Measured across the **screen**, in drawn order — the inverse of
    /// [`offset_at`](Self::offset_at). For text that is all left to right this
    /// is simply the width of everything before `at`, and
    /// [`width_upto`](Self::width_upto) answers the same number; the two part
    /// company as soon as a right-to-left stretch is involved, and then this
    /// is the one that puts the caret where the user can see it.
    ///
    /// An offset inside a ligature reports the start of the ligature, since
    /// that is the only place a caret can honestly be drawn: the glyph has no
    /// interior boundary to point at.
    ///
    /// # Why the affinity
    ///
    /// At a direction boundary one byte offset has two positions on the
    /// screen, and neither is more correct than the other — see [`Affinity`].
    /// A caller with no opinion passes [`Affinity::Downstream`] and gets the
    /// leading edge of the character that starts at `at`; a caller that has
    /// just typed, or has just arrived from the left, passes
    /// [`Affinity::Upstream`] and gets the trailing edge of the character
    /// before it. On non-bidirectional text the two are the same point, so the
    /// choice costs nothing to get wrong there.
    #[must_use]
    pub fn x_of(&self, at: usize, end: usize, affinity: Affinity) -> f32 {
        let mut i = 0usize;
        let mut previous: Option<(usize, usize)> = None;
        // A *cluster* spans from its own byte offset up to the next cluster's,
        // so `at` is inside this one until the following cluster starts past
        // it. Two things go wrong if this walks glyphs instead: testing a
        // glyph's own cluster steps past a ligature and reports the caret
        // after it, and testing the next *glyph's* cluster counts the first
        // half of a decomposed character — `x_of(0)` on glyphs
        // `[A(0), B(0), C(1)]` answering `A`'s advance instead of zero.
        while i < self.glyphs.len() {
            let to = self.group_end(i);
            let start = self.glyphs.get(i).map_or(end, |g| g.cluster);
            let next = self.glyphs.get(to).map_or(end, |g| g.cluster);
            if at < next {
                // Inside a ligature, where the run offers no boundary to point
                // at, both affinities give the same answer — the only honest
                // one, the start of the glyph that swallowed the character.
                if at > start {
                    return self.leading_edge(i, to);
                }
                return match (affinity, previous) {
                    (Affinity::Upstream, Some((from, upto))) => self.trailing_edge(from, upto),
                    _ => self.leading_edge(i, to),
                };
            }
            previous = Some((i, to));
            i = to;
        }
        // Past the last character: the far end of the run as *read*, which for
        // a run that ends in right-to-left text is not its right edge.
        previous.map_or(0.0, |(from, upto)| self.trailing_edge(from, upto))
    }

    /// The caret one cluster to the **left on the screen** of `from`, or
    /// `None` when there is nothing further left in this run.
    ///
    /// `end` is the source string's length.
    ///
    /// This is what the left arrow key means. It is not "the previous byte
    /// offset", and on bidirectional text the two are different journeys: in
    /// `ab` + a right-to-left `HR` + `cd`, drawn `a b R H c d`, pressing left
    /// from the far right visits the offsets 5, 4, 2, 3, 1, 0 — the two in the
    /// middle *increase* as the caret moves left, because that stretch is read
    /// the other way. A cursor stepped by `offset - 1` walks 5, 4, 3, 2, 1, 0
    /// instead, which puts the caret at the far side of the right-to-left word
    /// on the first press and back again on the second: it jumps across the
    /// screen rather than stepping across it.
    ///
    /// See [`caret_right`](Self::caret_right) for the mirror, and
    /// [`Affinity`] for why the answer is a [`Hit`] and not an offset: the
    /// caret arriving from the right at a direction boundary and the caret
    /// arriving from the left name the same byte and two different pixels, and
    /// a caller that kept only the byte would draw the next press at the wrong
    /// end and then step the wrong way from it.
    #[must_use]
    pub fn caret_left(&self, from: Hit, end: usize) -> Option<Hit> {
        let clusters = self.visual_clusters(end);
        let slot = Self::slot_of_caret(&clusters, from)?;
        // Moving left crosses the cluster to the left of the caret, and lands
        // on that cluster's *left* edge — which is its leading edge if it is
        // read left to right and its trailing edge if it is not.
        clusters.get(slot.checked_sub(1)?).map(|c| c.left)
    }

    /// The caret one cluster to the **right on the screen** of `from`, or
    /// `None` when there is nothing further right in this run.
    ///
    /// `end` is the source string's length. The mirror of
    /// [`caret_left`](Self::caret_left); see it for why this is not
    /// `offset + 1`.
    #[must_use]
    pub fn caret_right(&self, from: Hit, end: usize) -> Option<Hit> {
        let clusters = self.visual_clusters(end);
        let slot = Self::slot_of_caret(&clusters, from)?;
        clusters.get(slot).map(|c| c.right)
    }

    /// The run's clusters in the order they are drawn, each carrying the caret
    /// that belongs at its left edge and the one that belongs at its right.
    ///
    /// The two are the cluster's leading and trailing boundaries, swapped when
    /// it is read right to left — the same mapping [`offset_at`](Self::offset_at)
    /// makes when it decides which half of a glyph a click landed in, and it
    /// has to be the same one or a click and an arrow key would disagree about
    /// where the caret is.
    fn visual_clusters(&self, end: usize) -> alloc::vec::Vec<VisualCluster> {
        // Logical index -> drawn slot, built once. `slot_of` answers the same
        // question by searching, which would make this walk quadratic in the
        // length of the line for no reason: the permutation is a permutation,
        // so inverting it costs one pass.
        let n = self.glyphs.len();
        let mut slot = alloc::vec![0usize; n];
        if self.visual.is_empty() {
            for (i, s) in slot.iter_mut().enumerate() {
                *s = i;
            }
        } else {
            for (s, &v) in self.visual.iter().enumerate() {
                if let Some(cell) = usize::try_from(v).ok().and_then(|v| slot.get_mut(v)) {
                    *cell = s;
                }
            }
        }
        let mut out = alloc::vec::Vec::new();
        let mut i = 0usize;
        while i < n {
            let to = self.group_end(i);
            let start = self.glyphs.get(i).map_or(end, |g| g.cluster);
            let next = self.glyphs.get(to).map_or(end, |g| g.cluster);
            let leading = Hit {
                offset: start,
                affinity: Affinity::Downstream,
            };
            let trailing = Hit {
                offset: next,
                affinity: Affinity::Upstream,
            };
            let rtl = self.is_rtl(i);
            // A cluster occupies consecutive slots whichever way it is drawn —
            // rule L2 reverses whole level runs and a cluster lies inside one —
            // so its leftmost slot orders it against every other cluster.
            let leftmost = (i..to).filter_map(|j| slot.get(j).copied()).min().unwrap_or(0);
            out.push(VisualCluster {
                leftmost,
                left: if rtl { trailing } else { leading },
                right: if rtl { leading } else { trailing },
            });
            i = to;
        }
        out.sort_by_key(|c| c.leftmost);
        out
    }

    /// Which gap between drawn clusters the caret `at` sits in: `0` is left of
    /// everything, `clusters.len()` is right of everything.
    ///
    /// A gap has two names whenever the clusters either side of it are read in
    /// different directions — the right edge of the one and the left edge of
    /// the other are the same pixels and different bytes — and the affinity is
    /// what picks between them. So the exact `(offset, affinity)` is tried
    /// first, and only then the offset alone.
    fn slot_of_caret(clusters: &[VisualCluster], at: Hit) -> Option<usize> {
        let exact = clusters
            .iter()
            .position(|c| c.left == at)
            .or_else(|| clusters.iter().position(|c| c.right == at).map(|k| k.saturating_add(1)));
        if let Some(slot) = exact {
            return Some(slot);
        }
        // The offset alone. This is the caller that has never had an affinity
        // to give — a cursor freshly built from a byte index — and the caller
        // sitting at the end of the text, where `Downstream` names a character
        // that does not exist and only the `Upstream` reading is real.
        let loose = clusters
            .iter()
            .position(|c| c.left.offset == at.offset)
            .or_else(|| {
                clusters
                    .iter()
                    .position(|c| c.right.offset == at.offset)
                    .map(|k| k.saturating_add(1))
            });
        if let Some(slot) = loose {
            return Some(slot);
        }
        // Not a boundary at all: an offset inside a ligature, which has no
        // interior the caret can occupy. Snap to where `x_of` would draw it —
        // the start of the glyph that swallowed the character — so that the
        // arrow key moves from the position the user can see rather than from
        // one nothing ever drew.
        clusters.iter().enumerate().find_map(|(k, c)| {
            let (start, next) = c.byte_range();
            (start < at.offset && at.offset < next)
                .then(|| if c.is_rtl() { k.saturating_add(1) } else { k })
        })
    }

    /// The boxes to paint to highlight the source bytes `from..to`, as
    /// `(left, width)` pairs in pixels across the run, ordered left to right
    /// and never overlapping. `end` is the source string's length.
    ///
    /// A selection is one *logical* range but it is not one region on the
    /// screen. Selecting `bH` of `abHRcd`, where `HR` is right to left, selects
    /// two characters that are drawn with a third between them: the highlight
    /// is two boxes with a gap, and no single rectangle can describe it. This
    /// is why a caller cannot get by with two [`x_of`](Self::x_of) calls and
    /// the distance between them — that number is the width of a region that
    /// includes characters the user did not select.
    ///
    /// A cluster is highlighted whole if the range touches it at all. Half a
    /// ligature is not a thing that can be painted: the glyph has no interior
    /// boundary, which is the same reason [`x_of`](Self::x_of) will not put a
    /// caret inside one.
    ///
    /// An empty range selects nothing and yields no boxes. That is a caret, and
    /// [`x_of`](Self::x_of) is what draws it.
    ///
    /// # Why a `Vec` and not an iterator
    ///
    /// Laziness would buy nothing. The walk is in drawn order, and whether the
    /// *leftmost* box is open cannot be known until every cluster's byte range
    /// has been tested — a cluster drawn first may be the last one logically.
    /// So the whole logical-to-selected map must exist before the first box can
    /// be emitted, and an iterator would only hide the allocation that builds
    /// it behind a borrow the caller has to keep alive.
    #[must_use]
    pub fn selection_rects(&self, from: usize, to: usize, end: usize) -> Vec<(f32, f32)> {
        let mut rects = Vec::new();
        if from >= to || self.glyphs.is_empty() {
            return rects;
        }
        // Which glyphs are selected, by *logical* index. Marked cluster by
        // cluster: a cluster is in the selection when its byte range overlaps
        // the range at all, which for a cluster spanning `start..next` is
        // `start < to && next > from`.
        let mut selected = alloc::vec![false; self.glyphs.len()];
        let mut i = 0usize;
        while i < self.glyphs.len() {
            let upto = self.group_end(i);
            let start = self.glyphs.get(i).map_or(end, |g| g.cluster);
            let next = self.glyphs.get(upto).map_or(end, |g| g.cluster);
            if start < to && next > from {
                for slot in selected.get_mut(i..upto).unwrap_or_default() {
                    *slot = true;
                }
            }
            i = upto;
        }
        // Now sweep the run as it is drawn, accumulating the pen position, and
        // close a box wherever the selection stops. Merging happens for free:
        // two selected clusters that are drawn side by side are consecutive
        // slots and never open a second box, whichever order they are read in.
        //
        // The sweep is in slot space rather than pixel space on purpose. The
        // alternative — collecting each cluster's box and coalescing those that
        // touch — has to compare two sums of the same advances made in
        // different orders, and floating-point addition is not associative, so
        // "these boxes touch" would need a tolerance. Adjacency of integers
        // needs none.
        let mut x = 0.0f32;
        let mut open: Option<(f32, f32)> = None;
        for (slot, glyph) in self.draw_order().enumerate() {
            let logical = if self.visual.is_empty() {
                slot
            } else {
                self.visual
                    .get(slot)
                    .and_then(|&v| usize::try_from(v).ok())
                    .unwrap_or(slot)
            };
            if selected.get(logical).copied().unwrap_or(false) {
                let box_ = open.get_or_insert((x, 0.0));
                box_.1 += glyph.advance;
            } else if let Some(done) = open.take() {
                rects.push(done);
            }
            x += glyph.advance;
        }
        if let Some(done) = open.take() {
            rects.push(done);
        }
        rects
    }

    /// The width of everything before byte offset `at`, in pixels. `end` is
    /// the source string's length.
    ///
    /// A **measurement**, not a caret: it sums advances in logical order, so
    /// it answers "how wide is the text up to here" and never goes backwards.
    /// That is what truncation and ellipsis placement want — they cut the
    /// string and measure the piece — and it is the wrong number to draw a
    /// caret at in bidirectional text, where the width of a prefix and the
    /// position of its end on the screen are different quantities. Use
    /// [`x_of`](Self::x_of) for a caret.
    #[must_use]
    pub fn width_upto(&self, at: usize, end: usize) -> f32 {
        let mut x = 0.0;
        let mut i = 0usize;
        while i < self.glyphs.len() {
            let to = self.group_end(i);
            let next = self.glyphs.get(to).map_or(end, |g| g.cluster);
            if next > at {
                return x;
            }
            x += self.span_width(i, to);
            i = to;
        }
        x
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::panic
)]
mod tests {
    use super::*;

    /// A run of `n` glyphs, each 10 px wide, one byte per character.
    fn run(n: usize) -> ShapedRun {
        ShapedRun::new(
            (0..n)
                .map(|i| ShapedGlyph {
                    key: GlyphKey::bitmap('x'),
                    cluster: i,
                    advance: 10.0,
                    kern_next: 0.0,
                    offset: (0.0, 0.0),
                })
                .collect(),
        )
    }

    #[test]
    fn width_is_the_sum_of_the_advances() {
        assert!((run(4).width() - 40.0).abs() < f32::EPSILON);
        assert!(run(0).width().abs() < f32::EPSILON);
        assert!(run(0).is_empty());
    }

    #[test]
    fn fit_cuts_before_the_glyph_that_does_not_fit() {
        let r = run(5);
        assert_eq!(r.fit(25.0, 5), 2);
        // Exactly enough room is enough room.
        assert_eq!(r.fit(30.0, 5), 3);
        assert_eq!(r.fit(1000.0, 5), 5);
        assert_eq!(r.fit(0.0, 5), 0);
    }

    /// A prefix is drawn on its own, so the kern that pulled its last glyph
    /// towards a glyph that is about to be dropped must not be counted.
    ///
    /// Concretely, the bug this pins: `"AVATAR"` at a 10 px budget. The `A`'s
    /// advance is 10.32, but in the run it carries a -0.4 kern against the
    /// `V`, so a naive fit sees 9.9, keeps the `A`, and hands back a string
    /// that measures 10.32 — wider than the budget it was cut to fit.
    #[test]
    fn fit_drops_the_kern_to_the_glyph_it_drops() {
        let r = ShapedRun::new(alloc::vec![
            // "A" — 10.32 wide alone, 9.92 in the run.
            ShapedGlyph {
                key: GlyphKey::bitmap('A'),
                cluster: 0,
                advance: 9.92,
                kern_next: -0.4,
                offset: (0.0, 0.0),
            },
            ShapedGlyph {
                key: GlyphKey::bitmap('V'),
                cluster: 1,
                advance: 10.0,
                kern_next: 0.0,
                offset: (0.0, 0.0),
            },
        ]);
        // The whole run is still 19.92 wide as drawn.
        assert!((r.width() - 19.92).abs() < 0.001);
        // But "A" alone does not fit in 10.
        assert_eq!(r.fit(10.0, 2), 0);
        assert_eq!(r.fit(10.4, 2), 1);
        assert_eq!(r.fit(19.92, 2), 2);
    }

    #[test]
    fn fit_end_keeps_the_tail() {
        let r = run(5);
        assert_eq!(r.fit_end(25.0, 5), 3);
        assert_eq!(r.fit_end(30.0, 5), 2);
        assert_eq!(r.fit_end(1000.0, 5), 0);
        assert_eq!(r.fit_end(0.0, 5), 5);
    }

    #[test]
    fn a_click_goes_to_the_nearest_gap() {
        let r = run(3);
        // Left half of the first glyph.
        assert_eq!(r.offset_at(4.0, 3).offset, 0);
        // Right half of the first glyph: after it.
        assert_eq!(r.offset_at(6.0, 3).offset, 1);
        assert_eq!(r.offset_at(16.0, 3).offset, 2);
        // Past the end.
        assert_eq!(r.offset_at(1000.0, 3).offset, 3);
        assert_eq!(r.offset_at(-5.0, 3).offset, 0);
    }

    /// On left-to-right text the affinity picks between two names for the same
    /// point, so it cannot change an answer. Every caret query below therefore
    /// passes `Downstream` and means nothing by it.
    #[test]
    fn x_of_walks_to_the_cluster() {
        let r = run(4);
        assert!(r.x_of(0, 4, Affinity::Downstream).abs() < f32::EPSILON);
        assert!((r.x_of(2, 4, Affinity::Downstream) - 20.0).abs() < f32::EPSILON);
        assert!((r.x_of(4, 4, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);
        assert!((r.x_of(99, 4, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);
        // And the two affinities agree everywhere, there being no direction
        // boundary anywhere in the run for them to disagree at.
        for at in 0..=5 {
            let (down, up) = (
                r.x_of(at, 4, Affinity::Downstream),
                r.x_of(at, 4, Affinity::Upstream),
            );
            assert!((down - up).abs() < f32::EPSILON, "x_of({at}) split");
        }
    }

    /// `width_upto` is the old `x_of`: a prefix width, not a caret.
    #[test]
    fn width_upto_measures_the_prefix() {
        let r = run(4);
        assert!(r.width_upto(0, 4).abs() < f32::EPSILON);
        assert!((r.width_upto(2, 4) - 20.0).abs() < f32::EPSILON);
        assert!((r.width_upto(4, 4) - 40.0).abs() < f32::EPSILON);
        assert!((r.width_upto(99, 4) - 40.0).abs() < f32::EPSILON);
    }

    /// The three queries above have to agree with each other on a run whose
    /// glyphs span several characters, which is the whole point of clusters:
    /// no offset any of them returns may fall inside a ligature.
    #[test]
    fn a_ligature_is_never_split() {
        // "office": o, ffi (one glyph, three chars), c, e.
        let r = ShapedRun::new(alloc::vec![
            ShapedGlyph { key: GlyphKey::bitmap('o'), cluster: 0, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('\u{FB03}'), cluster: 1, advance: 20.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('c'), cluster: 4, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('e'), cluster: 5, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
        ]);
        assert!((r.width() - 50.0).abs() < f32::EPSILON);

        let inside = [2, 3];
        for width in 0..60 {
            let cut = r.fit(f32::from(u16::try_from(width).unwrap()), 6);
            assert!(!inside.contains(&cut), "fit({width}) cut inside the ligature");
        }
        for offset in 0..60 {
            let at = r.offset_at(f32::from(u16::try_from(offset).unwrap()), 6).offset;
            assert!(
                !inside.contains(&at),
                "offset_at({offset}) landed inside the ligature"
            );
        }
        // A caret asked for inside the ligature is drawn at its start.
        assert!((r.x_of(1, 6, Affinity::Downstream) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(2, 6, Affinity::Downstream) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(3, 6, Affinity::Downstream) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(4, 6, Affinity::Downstream) - 30.0).abs() < f32::EPSILON);
        assert!((r.x_of(6, 6, Affinity::Downstream) - 50.0).abs() < f32::EPSILON);
    }

    /// The other direction of the same relation: one *character* drawn as
    /// several glyphs, which GSUB LookupType 2 produces.
    ///
    /// Every query has to treat those glyphs as one unit. The failure mode is
    /// not a crash but an off-by-a-glyph: a caret drawn between the two halves
    /// of a character, or a suffix measured at a width the string it names
    /// does not have.
    #[test]
    fn a_decomposed_character_is_never_split() {
        // "aXb", where X is one character drawn as two 10 px glyphs — say a
        // precomposed vowel the font renders as a base plus a sign.
        let r = ShapedRun::new(alloc::vec![
            ShapedGlyph { key: GlyphKey::bitmap('a'), cluster: 0, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('X'), cluster: 1, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('\u{0301}'), cluster: 1, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('b'), cluster: 2, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
        ]);
        assert!((r.width() - 40.0).abs() < f32::EPSILON);

        // The caret before X is before *both* its glyphs. This is the case the
        // old glyph-at-a-time walk got wrong: it counted the first glyph and
        // answered 20.
        assert!((r.x_of(0, 3, Affinity::Downstream)).abs() < f32::EPSILON);
        assert!((r.x_of(1, 3, Affinity::Downstream) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(2, 3, Affinity::Downstream) - 30.0).abs() < f32::EPSILON);
        assert!((r.x_of(3, 3, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);

        // A suffix keeps whole characters: 25 px is room for `b` and no more,
        // because X costs 20 and half an X is not a byte offset.
        assert_eq!(r.fit_end(25.0, 3), 2);
        assert_eq!(r.fit_end(30.0, 3), 1);
        assert_eq!(r.fit_end(35.0, 3), 1);
        assert_eq!(r.fit_end(40.0, 3), 0);

        // And a prefix does too.
        assert_eq!(r.fit(15.0, 3), 1);
        assert_eq!(r.fit(25.0, 3), 1);
        assert_eq!(r.fit(30.0, 3), 2);

        // The click tips at X's own midpoint — 20 px in — not at the midpoint
        // of its first glyph.
        assert_eq!(r.offset_at(15.0, 3).offset, 1);
        assert_eq!(r.offset_at(19.0, 3).offset, 1);
        assert_eq!(r.offset_at(21.0, 3).offset, 2);

        // No query may ever name a position inside X. There is only one: X
        // starts at byte 1 and ends at byte 2, and it is one character, so
        // there is no interior byte to name — the check is that every answer
        // is a real cluster boundary.
        let boundaries = [0usize, 1, 2, 3];
        for px in 0..50 {
            let px = f32::from(u16::try_from(px).unwrap());
            assert!(boundaries.contains(&r.fit(px, 3)), "fit({px})");
            assert!(boundaries.contains(&r.fit_end(px, 3)), "fit_end({px})");
            assert!(
                boundaries.contains(&r.offset_at(px, 3).offset),
                "offset_at({px})"
            );
        }
    }

    /// Whatever `fit_end` hands back must name a suffix that really does fit,
    /// which is the property a glyph-at-a-time walk breaks on a decomposed
    /// character: it counts one glyph's advance and returns an offset whose
    /// string draws with two.
    #[test]
    fn a_suffix_is_never_wider_than_the_budget_it_was_cut_to() {
        let r = ShapedRun::new(alloc::vec![
            ShapedGlyph { key: GlyphKey::bitmap('a'), cluster: 0, advance: 7.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('X'), cluster: 1, advance: 11.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('\u{0301}'), cluster: 1, advance: 5.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('b'), cluster: 2, advance: 9.0, kern_next: 0.0, offset: (0.0, 0.0) },
        ]);
        for px in 0..40 {
            let budget = f32::from(u16::try_from(px).unwrap());
            let start = r.fit_end(budget, 3);
            // Width of the suffix beginning at `start`, summed from the run
            // itself so the two cannot drift apart.
            let width: f32 = r
                .glyphs()
                .iter()
                .filter(|g| g.cluster >= start)
                .map(|g| g.advance)
                .sum();
            assert!(
                width <= budget || start == 3,
                "fit_end({budget}) returned {start}, a suffix {width} px wide"
            );
        }
    }

    /// A run whose glyph keys spell `text`, so a drawing order is readable.
    fn spelled(text: &str) -> Vec<ShapedGlyph> {
        text.chars()
            .enumerate()
            .map(|(i, ch)| ShapedGlyph {
                key: GlyphKey::bitmap(ch),
                cluster: i,
                advance: 10.0,
                kern_next: 0.0,
                offset: (0.0, 0.0),
            })
            .collect()
    }

    fn drawn(run: &ShapedRun) -> String {
        run.draw_order().map(|g| g.key.ch()).collect()
    }

    #[test]
    fn a_permutation_that_changes_nothing_is_not_stored() {
        let run = ShapedRun::reordered(spelled("abc"), vec![0, 1, 2], vec![0, 0, 0]);
        assert!(!run.is_reordered());
        // And the walk still works: it falls through to the glyph slice.
        assert_eq!(drawn(&run), "abc");
    }

    #[test]
    fn draw_order_follows_the_permutation_and_glyphs_do_not() {
        let run = ShapedRun::reordered(spelled("abc"), vec![2, 1, 0], vec![1, 1, 1]);
        assert!(run.is_reordered());
        assert_eq!(drawn(&run), "cba");
        // The point of keeping both: clusters stay sorted for the queries.
        let logical: String = run.glyphs().iter().map(|g| g.key.ch()).collect();
        assert_eq!(logical, "abc");
        assert!(run.glyphs().windows(2).all(|w| w[0].cluster <= w[1].cluster));
    }

    #[test]
    fn a_partial_permutation_drops_glyphs_rather_than_panicking() {
        // Nothing should build one, but `visual` is a plain vector and a
        // reordering bug that produced an out-of-range index must not take
        // the process with it.
        let run = ShapedRun::reordered(spelled("abc"), vec![1, 9], vec![1, 1, 1]);
        assert_eq!(drawn(&run), "b");
    }

    /// `ab<HE><BR>cd`, five one-byte "characters" of which two are Hebrew, so
    /// the drawn order is `ab<BR><HE>cd` and the Hebrew pair occupies the box
    /// from 20 px to 40 px with its *first* character on the right.
    fn bidi_run() -> ShapedRun {
        // Glyph keys spell the logical string; levels mark the middle pair.
        ShapedRun::reordered(
            spelled("abHRcd"),
            // Logical 0,1 then the pair reversed, then 4,5.
            vec![0, 1, 3, 2, 4, 5],
            vec![0, 0, 1, 1, 0, 0],
        )
    }

    #[test]
    fn a_right_to_left_caret_is_at_the_right_edge_of_its_character() {
        let r = bidi_run();
        assert_eq!(drawn(&r), "abRHcd");
        // The Latin half behaves exactly as before.
        assert!((r.x_of(0, 6, Affinity::Downstream)).abs() < f32::EPSILON);
        assert!((r.x_of(1, 6, Affinity::Downstream) - 10.0).abs() < f32::EPSILON);
        // Byte 2 starts the Hebrew word, which is drawn from 20 to 40 with its
        // first letter on the right — so the caret before it is at 40, not 20.
        // This is the whole bug: the logical prefix width says 20.
        assert!((r.x_of(2, 6, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);
        assert!((r.width_upto(2, 6) - 20.0).abs() < f32::EPSILON);
        // Between the two Hebrew letters: the second is drawn at 20..30, and
        // it starts at *its* right edge, 30.
        assert!((r.x_of(3, 6, Affinity::Downstream) - 30.0).abs() < f32::EPSILON);
        // Byte 4 is the `c`, back in Latin at 40.
        assert!((r.x_of(4, 6, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);
        assert!((r.x_of(6, 6, Affinity::Downstream) - 60.0).abs() < f32::EPSILON);
    }

    /// The two positions of one offset. At byte 2 the caret can be after the
    /// `b` — 20 px, where the Latin run ends — or before the first Hebrew
    /// letter — 40 px, where the Hebrew run starts. Both are the same place in
    /// the string and different places on the screen.
    #[test]
    fn the_affinity_picks_between_the_two_sides_of_a_direction_boundary() {
        let r = bidi_run();
        assert!((r.x_of(2, 6, Affinity::Upstream) - 20.0).abs() < f32::EPSILON);
        assert!((r.x_of(2, 6, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);
        // And at the far boundary, mirrored: byte 4 is the trailing edge of the
        // Hebrew word (20 px, its left edge) or the leading edge of `c` (40).
        assert!((r.x_of(4, 6, Affinity::Upstream) - 20.0).abs() < f32::EPSILON);
        assert!((r.x_of(4, 6, Affinity::Downstream) - 40.0).abs() < f32::EPSILON);
    }

    /// The inverse: clicking the *left* half of a right-to-left character puts
    /// the caret after it, because that is the half it is read towards.
    #[test]
    fn a_click_on_a_right_to_left_character_reads_the_other_way() {
        let r = bidi_run();
        // 22 px — the left half of the box 20..30, which is drawn with the
        // *second* Hebrew letter (logical byte 3). Its trailing side is the
        // left, so the caret goes to byte 4, arrived at from upstream.
        let hit = r.offset_at(22.0, 6);
        assert_eq!(hit.offset, 4);
        assert_eq!(hit.affinity, Affinity::Upstream);
        // 28 px — the right half of the same box, so before that letter.
        let hit = r.offset_at(28.0, 6);
        assert_eq!(hit.offset, 3);
        assert_eq!(hit.affinity, Affinity::Downstream);
        // Latin, unchanged: the right half of `a` is after it.
        let hit = r.offset_at(6.0, 6);
        assert_eq!(hit.offset, 1);
        assert_eq!(hit.affinity, Affinity::Upstream);
    }

    /// Whatever a click returns, drawing a caret there has to put it back
    /// where the click was — within half a character, since a click is
    /// snapped to the nearer boundary.
    #[test]
    fn a_click_and_the_caret_it_produces_agree() {
        let r = bidi_run();
        for px in 0..60 {
            let px = f32::from(u16::try_from(px).unwrap());
            let hit = r.offset_at(px, 6);
            let back = r.x_of(hit.offset, 6, hit.affinity);
            assert!(
                (back - px).abs() <= 5.0,
                "click at {px} became offset {} ({:?}), drawn back at {back}",
                hit.offset,
                hit.affinity
            );
        }
    }

    /// A single right-to-left glyph between two Latin words: the permutation
    /// is the identity, so a caret reading it alone would face the wrong way.
    /// This is why the levels are stored separately.
    #[test]
    fn one_right_to_left_glyph_still_starts_at_its_right_edge() {
        let r = ShapedRun::reordered(spelled("aHb"), vec![0, 1, 2], vec![0, 1, 0]);
        assert!(!r.is_reordered());
        assert_eq!(drawn(&r), "aHb");
        // The letter is drawn at 10..20 and read from 20 leftwards.
        assert!((r.x_of(1, 3, Affinity::Downstream) - 20.0).abs() < f32::EPSILON);
        assert!((r.x_of(2, 3, Affinity::Downstream) - 20.0).abs() < f32::EPSILON);
        assert!((r.x_of(2, 3, Affinity::Upstream) - 10.0).abs() < f32::EPSILON);
    }

    /// Walk a caret to one side until it runs out, collecting where it stops.
    fn walk(r: &ShapedRun, from: Hit, end: usize, right: bool) -> Vec<(usize, Affinity, f32)> {
        let mut out = Vec::new();
        let mut at = from;
        // Bounded rather than `loop`: a bug that made motion cyclic would
        // otherwise hang the test suite instead of failing it.
        for _ in 0..64 {
            let Some(next) = (if right {
                r.caret_right(at, end)
            } else {
                r.caret_left(at, end)
            }) else {
                return out;
            };
            out.push((next.offset, next.affinity, r.x_of(next.offset, end, next.affinity)));
            at = next;
        }
        panic!("caret motion did not terminate: {out:?}");
    }

    /// Every stop was drawn where it was supposed to be. Separate from the
    /// offsets so that a failure says which of the two went wrong: a walk with
    /// the right bytes and the wrong pixels is a different bug from a walk
    /// with the wrong bytes.
    fn assert_xs(stops: &[(usize, Affinity, f32)], want: &[f32]) {
        let got: Vec<f32> = stops.iter().map(|s| s.2).collect();
        let same = got.len() == want.len()
            && got.iter().zip(want).all(|(g, w)| (g - w).abs() < 0.001);
        assert!(same, "drawn at {got:?}, want {want:?} (stops {stops:?})");
    }

    /// The bug this whole pair of methods exists for. `ab` + a right-to-left
    /// `HR` + `cd` is drawn `abRHcd`; pressing the right arrow from the left
    /// edge must walk the caret across the screen in even 10 px steps, and the
    /// byte offsets it passes through are *not* sorted — 0, 1, 2, 3, 2, 5, 6.
    /// A cursor stepped by `offset + 1` would visit 1, 2, 3, 4, 5, 6 and draw
    /// itself at 10, 20, 30, 20, 40, 50: two of those go backwards.
    #[test]
    fn the_right_arrow_steps_right_across_the_screen() {
        let r = bidi_run();
        let start = Hit { offset: 0, affinity: Affinity::Downstream };
        let stops = walk(&r, start, 6, true);
        let offsets: Vec<usize> = stops.iter().map(|s| s.0).collect();
        assert_eq!(offsets, alloc::vec![1, 2, 3, 2, 5, 6], "{stops:?}");
        assert_xs(&stops, &[10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
    }

    /// The mirror. The offsets are *not* the rightward walk reversed, and that
    /// is not a bug: at a direction boundary two byte offsets share one pixel,
    /// and each direction names the boundary by the character it has just
    /// crossed. What must match — and does — is the sequence of positions.
    #[test]
    fn the_left_arrow_steps_left_across_the_screen() {
        let r = bidi_run();
        let start = Hit { offset: 6, affinity: Affinity::Upstream };
        let stops = walk(&r, start, 6, false);
        let offsets: Vec<usize> = stops.iter().map(|s| s.0).collect();
        assert_eq!(offsets, alloc::vec![5, 4, 3, 4, 1, 0], "{stops:?}");
        assert_xs(&stops, &[50.0, 40.0, 30.0, 20.0, 10.0, 0.0]);
    }

    /// Right then left returns the caret to the pixel it started at, every
    /// time. The *offset* may come back as the other of the boundary's two
    /// names — that is inherent, since the two are one place on the screen —
    /// so the invariant is stated in pixels, which is where the user is
    /// looking.
    #[test]
    fn a_step_and_its_reverse_return_to_the_same_place() {
        let r = bidi_run();
        let mut at = Hit { offset: 0, affinity: Affinity::Downstream };
        while let Some(next) = r.caret_right(at, 6) {
            let there = r.x_of(next.offset, 6, next.affinity);
            let back = r.caret_left(next, 6).expect("something was crossed to get here");
            let here = r.x_of(at.offset, 6, at.affinity);
            let returned = r.x_of(back.offset, 6, back.affinity);
            assert!(
                (returned - here).abs() < 0.001,
                "right to {there} then left landed at {returned}, want {here}"
            );
            at = next;
        }
    }

    /// The run's two ends are the two ends. Nothing to the left of the left
    /// edge, nothing to the right of the right one — and `None` rather than a
    /// caret that stays put, so a widget can hand the keystroke to whatever
    /// owns the line above or below instead of swallowing it.
    #[test]
    fn motion_stops_at_the_edges_and_says_so() {
        let r = bidi_run();
        let left_edge = Hit { offset: 0, affinity: Affinity::Downstream };
        assert_eq!(r.caret_left(left_edge, 6), None);
        let right_edge = Hit { offset: 6, affinity: Affinity::Upstream };
        assert_eq!(r.caret_right(right_edge, 6), None);
        // An empty run has no clusters, so neither direction goes anywhere.
        let empty = ShapedRun::new(alloc::vec![]);
        assert_eq!(empty.caret_right(left_edge, 0), None);
        assert_eq!(empty.caret_left(left_edge, 0), None);
    }

    /// A cursor that has never carried an affinity — one built from a byte
    /// index, which is every cursor in the toolkit until it is told otherwise
    /// — still moves. At the end of the text `Downstream` names a character
    /// that does not exist, so only the `Upstream` reading is real, and the
    /// query has to fall back to it rather than refusing.
    #[test]
    fn a_cursor_with_only_a_byte_offset_still_moves() {
        let r = bidi_run();
        let end = Hit { offset: 6, affinity: Affinity::Downstream };
        assert_eq!(r.caret_left(end, 6), Some(Hit { offset: 5, affinity: Affinity::Downstream }));
        assert_eq!(r.caret_right(end, 6), None);
    }

    /// On text that runs one way, this is simply "the next character" — the
    /// property that keeps the arrow keys unsurprising for the overwhelming
    /// majority of text, and the one a bidi-aware implementation is most
    /// likely to break.
    #[test]
    fn motion_on_one_direction_text_is_the_next_character() {
        let r = run(4);
        let mut at = Hit { offset: 0, affinity: Affinity::Downstream };
        for want in 1..=4 {
            at = r.caret_right(at, 4).expect("four characters, four steps");
            assert_eq!(at.offset, want);
        }
        assert_eq!(r.caret_right(at, 4), None);
        for want in (0..4).rev() {
            at = r.caret_left(at, 4).expect("and four steps back");
            assert_eq!(at.offset, want);
        }
        assert_eq!(r.caret_left(at, 4), None);
    }

    /// A ligature has no interior a caret can occupy, so motion crosses it
    /// whole — and a cursor that somehow *is* inside one steps out of it
    /// rather than refusing to move. `x_of` already draws such a cursor at
    /// the ligature's start; motion has to begin from the same place, or the
    /// caret would jump on the first keypress.
    #[test]
    fn a_ligature_is_crossed_whole() {
        // "office": o, ffi (one glyph, three chars), c, e.
        let r = ShapedRun::new(alloc::vec![
            ShapedGlyph { key: GlyphKey::bitmap('o'), cluster: 0, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('\u{FB03}'), cluster: 1, advance: 20.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('c'), cluster: 4, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('e'), cluster: 5, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
        ]);
        let at = Hit { offset: 1, affinity: Affinity::Downstream };
        // One press crosses all three characters of the `ffi`.
        assert_eq!(r.caret_right(at, 6).map(|h| h.offset), Some(4));
        // From byte 2 — inside the ligature — the next stop is the same one,
        // because that cursor is drawn at byte 1 and moving is relative to
        // where it is drawn.
        let inside = Hit { offset: 2, affinity: Affinity::Downstream };
        assert_eq!(r.caret_right(inside, 6).map(|h| h.offset), Some(4));
        assert_eq!(r.caret_left(inside, 6).map(|h| h.offset), Some(0));
    }

    /// A lone right-to-left letter between two Latin words: the permutation is
    /// the identity, so an implementation that read the permutation and not the
    /// levels would see nothing to do here and step straight through. The
    /// caret still has to face the other way while it crosses the letter.
    #[test]
    fn a_lone_right_to_left_letter_is_still_crossed_backwards() {
        let r = ShapedRun::reordered(spelled("aHb"), vec![0, 1, 2], vec![0, 1, 0]);
        let stops = walk(&r, Hit { offset: 0, affinity: Affinity::Downstream }, 3, true);
        let offsets: Vec<usize> = stops.iter().map(|s| s.0).collect();
        // Crossing `H` rightwards lands *before* it — byte 1 — because that is
        // the edge of it that faces right.
        assert_eq!(offsets, alloc::vec![1, 1, 3], "{stops:?}");
        assert_xs(&stops, &[10.0, 20.0, 30.0]);
    }

    /// `(left, width)` pairs compared at a tolerance. Spelled as an assertion
    /// rather than a conversion so a failure prints both lists of boxes: the
    /// interesting failures here are off-by-one-box, and a boolean would not
    /// say which box.
    fn assert_boxes(rects: &[(f32, f32)], want: &[(f32, f32)]) {
        let same = rects.len() == want.len()
            && rects
                .iter()
                .zip(want)
                .all(|(g, w)| (g.0 - w.0).abs() < 0.001 && (g.1 - w.1).abs() < 0.001);
        assert!(same, "got {rects:?}, want {want:?}");
    }

    #[test]
    fn a_selection_of_left_to_right_text_is_one_box() {
        let r = run(4);
        assert_boxes(&r.selection_rects(1, 3, 4), &[(10.0, 20.0)]);
        assert_boxes(&r.selection_rects(0, 4, 4), &[(0.0, 40.0)]);
        // An empty range is a caret, not a selection.
        assert!(r.selection_rects(2, 2, 4).is_empty());
        assert!(r.selection_rects(3, 1, 4).is_empty());
        assert!(run(0).selection_rects(0, 4, 4).is_empty());
    }

    /// The reason this method exists: a selection that is one range in the
    /// text is two rectangles with a gap on the screen, and the gap holds a
    /// character the user did not select.
    #[test]
    fn a_selection_across_a_direction_boundary_is_two_boxes() {
        let r = bidi_run();
        // Drawn `abRHcd`. Selecting `bH` — logically adjacent — highlights the
        // `b` at 10..20 and the `H` at 30..40, leaving the `R` at 20..30 out.
        assert_boxes(&r.selection_rects(1, 3, 6), &[(10.0, 10.0), (30.0, 10.0)]);
        // Which is exactly what two carets cannot describe: the caret at byte 1
        // is at 10 and the one at byte 3 is at 30, and the 20 px between them
        // is not the 20 px that is selected.
        assert!((r.x_of(1, 6, Affinity::Downstream) - 10.0).abs() < f32::EPSILON);
        assert!((r.x_of(3, 6, Affinity::Downstream) - 30.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_selection_of_a_whole_right_to_left_word_is_one_box_again() {
        let r = bidi_run();
        // `HR` is drawn as `RH` from 20 to 40 — reversed, but contiguous, so
        // the two clusters merge into a single box rather than two touching
        // ones. The merge is what the slot-space sweep buys.
        assert_boxes(&r.selection_rects(2, 4, 6), &[(20.0, 20.0)]);
        // The Latin either side, and the whole line.
        assert_boxes(&r.selection_rects(0, 2, 6), &[(0.0, 20.0)]);
        assert_boxes(&r.selection_rects(4, 6, 6), &[(40.0, 20.0)]);
        assert_boxes(&r.selection_rects(0, 6, 6), &[(0.0, 60.0)]);
        // A range that reaches past the end still stops at the run.
        assert_boxes(&r.selection_rects(4, 99, 6), &[(40.0, 20.0)]);
    }

    /// Every box a selection paints must lie inside the run, the boxes must be
    /// ordered and disjoint, and their widths must add up to the width of the
    /// characters selected — which for a contiguous logical range is
    /// `width_upto(to) - width_upto(from)`, the one thing the prefix width is
    /// still good for.
    #[test]
    fn selection_boxes_are_ordered_disjoint_and_add_up() {
        let r = bidi_run();
        for from in 0..=6 {
            for to in from..=6 {
                let rects = r.selection_rects(from, to, 6);
                let mut edge = 0.0f32;
                let mut total = 0.0f32;
                for &(left, width) in &rects {
                    assert!(width > 0.0, "empty box in {from}..{to}");
                    assert!(left >= edge, "boxes out of order in {from}..{to}");
                    assert!(left + width <= r.width() + 0.001, "box past the run");
                    edge = left + width;
                    total += width;
                }
                let want = r.width_upto(to, 6) - r.width_upto(from, 6);
                assert!(
                    (total - want).abs() < 0.001,
                    "{from}..{to}: painted {total} for {want} of text"
                );
            }
        }
    }

    /// A ligature has no interior boundary, so a range that touches it paints
    /// the whole glyph — the same rule that stops a caret being drawn inside
    /// one.
    #[test]
    fn a_selection_paints_a_ligature_whole() {
        // "office": o, ffi (one glyph, three chars), c, e.
        let r = ShapedRun::new(alloc::vec![
            ShapedGlyph { key: GlyphKey::bitmap('o'), cluster: 0, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('\u{FB03}'), cluster: 1, advance: 20.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('c'), cluster: 4, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
            ShapedGlyph { key: GlyphKey::bitmap('e'), cluster: 5, advance: 10.0, kern_next: 0.0, offset: (0.0, 0.0) },
        ]);
        // Selecting just the `f` of `ffi` paints all 20 px of the ligature.
        assert_boxes(&r.selection_rects(1, 2, 6), &[(10.0, 20.0)]);
        assert_boxes(&r.selection_rects(2, 3, 6), &[(10.0, 20.0)]);
        // And it does not bleed into the neighbours.
        assert_boxes(&r.selection_rects(0, 1, 6), &[(0.0, 10.0)]);
        assert_boxes(&r.selection_rects(4, 5, 6), &[(30.0, 10.0)]);
    }
}
