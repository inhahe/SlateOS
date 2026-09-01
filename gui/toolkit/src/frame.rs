//! Drawing and hit-testing as one walk: [`Rect`] and [`Frame`].
//!
//! An application window has to answer two questions about the same geometry:
//! *where do I paint this control* and *did that click land on it*. The obvious
//! structure answers them separately — a `Layout` pass computes the boxes, the
//! renderer draws from it, and a hit-test reads it back. That is fine when the
//! layout is a tree the engine owns. It is **not** what our applications look
//! like: they are one-to-five-thousand-line renderers built out of a running
//! `y` cursor, and a `Layout` for one of those is a second transcription of the
//! same arithmetic. Two transcriptions drift, and the one that is wrong is
//! always whichever one you are not currently reading. The symptom is a click
//! that lands on the row above the one it was aimed at, and it survives review
//! because both copies look right.
//!
//! [`Frame`] removes the second copy. The renderer *records* the box it painted
//! for every control, as it paints it:
//!
//! ```
//! use guitk::frame::{Frame, Rect};
//! use guitk::render::RenderCommand;
//! use guitk::color::Color;
//! use guitk::style::CornerRadii;
//!
//! #[derive(Clone, Copy, Debug, PartialEq, Eq)]
//! enum Target { Ok, Cancel }
//!
//! let mut frame: Frame<Target> = Frame::new(200.0, 100.0);
//! let button = Rect::new(10.0, 10.0, 80.0, 24.0);
//! frame.push(RenderCommand::FillRect {
//!     x: button.x, y: button.y, width: button.w, height: button.h,
//!     color: Color::rgb(0, 0, 0), corner_radii: CornerRadii::ZERO,
//! });
//! frame.hit(Target::Ok, button);
//!
//! assert_eq!(frame.hit_test(50.0, 20.0), Some(Target::Ok));
//! assert_eq!(frame.hit_test(50.0, 60.0), None, "bare background");
//! ```
//!
//! The hit-test *is* the renderer: run it, then read the boxes back. There is
//! no geometry anywhere else that could disagree.
//!
//! # Why it tracks clips and translations
//!
//! A recorded box is only clickable where it is actually *visible*, and both
//! of the things that decide that are render commands rather than arithmetic
//! the caller does:
//!
//! - **[`RenderCommand::PushClip`]** narrows the visible region. A list row
//!   scrolled half off the top of its pane is drawn half-height; recording the
//!   whole row would let the invisible half steal clicks from the toolbar
//!   painted above the pane. [`Frame::hit`] therefore intersects every rect
//!   with the clip in force and drops it entirely if it falls outside.
//! - **[`RenderCommand::PushTranslate`]** moves the origin. A scrolling pane
//!   that draws its rows at `y = 0, 24, 48…` inside a `PushTranslate` is
//!   painted somewhere else entirely, so a rect recorded verbatim would be
//!   clickable at coordinates nothing was drawn at.
//!
//! Both stacks mirror the compositor's own (`ClipStack`/`TranslateStack` in
//! `gui/compositor`): translations **accumulate additively**, a nested clip can
//! only shrink the visible region and so is intersected with its parent, and a
//! clip's own coordinates are subject to the translation in force when it is
//! pushed. Getting this wrong in the same direction as the compositor would at
//! least be consistent; getting it wrong in the other direction puts the click
//! target and the ink in different places.
//!
//! The consequence for callers is the useful part: **pass [`Frame::hit`] the
//! same coordinates you passed the draw command**, whatever clip or translation
//! is in force. `Frame` converts to window coordinates, which is the space
//! [`Frame::hit_test`] takes. There is no case where a renderer has to
//! compensate by hand.
//!
//! # History
//!
//! This started as a private `Frame` inside `apps/archivemanager`, copied into
//! `apps/netmanager`. The two copies had already diverged before this module
//! existed — only one of them tracked clips, so identical-looking code in the
//! other app left rows clickable after they scrolled out of sight — which is
//! the ordinary fate of a duplicated invariant and the reason this lives in the
//! toolkit now.

use crate::render::{RenderCommand, RenderTree};

/// An axis-aligned rectangle in window coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width. Never negative in a rectangle this module produced.
    pub w: f32,
    /// Height. Never negative in a rectangle this module produced.
    pub h: f32,
}

impl Rect {
    /// A rectangle that contains nothing, used for a clip that clipped
    /// everything away.
    pub const EMPTY: Self = Self {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };

    /// A rectangle at `(x, y)` measuring `w` by `h`.
    #[must_use]
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// The x coordinate just past the right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.w
    }

    /// The y coordinate just past the bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.h
    }

    /// Whether this rectangle encloses no points at all.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Whether `(x, y)` is inside this rectangle.
    ///
    /// **Half-open on both axes**: the left and top edges belong to this
    /// rectangle, the right and bottom edges belong to whatever is next. Two
    /// rows that share a boundary pixel would otherwise both claim it, and
    /// which one won would depend on the order they happened to be recorded
    /// in — that is, on a detail of the renderer that nobody reading the click
    /// handler can see.
    #[must_use]
    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }

    /// This rectangle moved by `(dx, dy)`.
    #[must_use]
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// The overlap between two rectangles, or `None` if they do not overlap.
    ///
    /// Touching along an edge counts as *not* overlapping, to match
    /// [`contains`](Self::contains) being half-open: a zero-width overlap
    /// contains no points, and returning it would make [`Frame::hit`] record a
    /// target that can never be clicked.
    #[must_use]
    pub fn intersect(self, other: Self) -> Option<Self> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right <= x || bottom <= y {
            return None;
        }
        Some(Self::new(x, y, right - x, bottom - y))
    }

    /// The centre point, which is where a test should aim a click.
    ///
    /// Aiming at a corner is aiming at whichever neighbour rounds its way; the
    /// centre is the only point in a rectangle that is unambiguously its own.
    #[must_use]
    pub fn centre(self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// A frame being drawn: the commands to paint, and the clickable boxes that
/// painting them created.
///
/// `T` is the application's own "what can be clicked" type — normally a small
/// `Copy` enum naming each control, with row-bearing variants carrying a
/// **stable id rather than a row index**, since sorting a list or expanding a
/// tree node renumbers the rows under the pointer.
///
/// See the [module documentation](self) for why rendering and hit-testing are
/// the same walk.
#[derive(Clone, Debug)]
pub struct Frame<T> {
    /// The commands drawn so far. Private because pushing a clip or a
    /// translation straight onto it would desynchronise the stacks below from
    /// the commands they describe, and every rect recorded afterwards would be
    /// converted with the wrong offset.
    tree: RenderTree,
    /// Clickable boxes in paint order, in **window** coordinates, already
    /// trimmed to the clip that was in force. Later entries are painted on
    /// top, so [`Frame::hit_test`] reads this back to front.
    hits: Vec<(T, Rect)>,
    /// The clip stack, each entry already intersected with its parent.
    clips: Vec<Rect>,
    /// The individual translations pushed, kept so a pop can undo exactly the
    /// one it matches rather than recomputing a sum.
    translations: Vec<(f32, f32)>,
    /// The accumulated translation, maintained alongside `translations` so the
    /// common case — converting one rect — is an addition and not a fold.
    offset: (f32, f32),
    /// How many pops arrived with nothing to pop.
    ///
    /// An over-pop is as damaging as an unpopped push and was invisible to
    /// [`is_balanced`](Frame::is_balanced) until this was counted: `Vec::pop`
    /// on an empty stack returns `None` and does nothing, so a helper that
    /// popped a clip it had not pushed released *its caller's* clip, and the
    /// stack still ended empty. Found by a mutation sweep on `apps/pacman`,
    /// where deleting a `f.clip(...)` and leaving its `f.unclip()` behind let
    /// the rest of the frame escape the window's clip while the frame reported
    /// itself balanced.
    stray_pops: u32,
    /// The width this frame is being drawn at.
    pub width: f32,
    /// The height this frame is being drawn at.
    pub height: f32,
}

impl<T> Frame<T> {
    /// An empty frame for a window of `width` by `height`.
    ///
    /// The size is stored as given. Callers that have a minimum window size
    /// clamp before calling: what counts as too small is the application's
    /// policy, not this type's.
    #[must_use]
    pub fn new(width: f32, height: f32) -> Self {
        Self {
            tree: RenderTree::new(),
            hits: Vec::new(),
            clips: Vec::new(),
            translations: Vec::new(),
            offset: (0.0, 0.0),
            stray_pops: 0,
            width,
            height,
        }
    }

    /// Record a draw command, tracking clips and translations as they are
    /// pushed and popped.
    pub fn push(&mut self, command: RenderCommand) {
        match command {
            RenderCommand::PushClip {
                x,
                y,
                width,
                height,
            } => {
                // The clip's own coordinates are in the translated space, the
                // same as every other command's, so it has to be converted
                // before it can be intersected with a parent already in window
                // coordinates. The compositor does exactly this.
                let rect = Rect::new(x, y, width, height).translated(self.offset.0, self.offset.1);
                let effective = if rect.is_empty() {
                    // A degenerate clip is not an error — a pane can be
                    // squeezed to nothing by a resize — but it must clip
                    // everything, not wrap around to enormous.
                    Rect::EMPTY
                } else {
                    match self.clips.last() {
                        Some(outer) => outer.intersect(rect).unwrap_or(Rect::EMPTY),
                        None => rect,
                    }
                };
                self.clips.push(effective);
            }
            RenderCommand::PopClip => {
                if self.clips.pop().is_none() {
                    self.stray_pops = self.stray_pops.saturating_add(1);
                }
            }
            RenderCommand::PushTranslate { dx, dy } => {
                self.translations.push((dx, dy));
                self.offset.0 += dx;
                self.offset.1 += dy;
            }
            RenderCommand::PopTranslate => {
                if let Some((dx, dy)) = self.translations.pop() {
                    self.offset.0 -= dx;
                    self.offset.1 -= dy;
                } else {
                    self.stray_pops = self.stray_pops.saturating_add(1);
                }
            }
            _ => {}
        }
        self.tree.push(command);
    }

    /// Draw with a helper that writes into a `Vec<RenderCommand>`.
    ///
    /// Several drawing helpers in this crate — [`Table`](crate::table::Table)
    /// most of all — were written against a plain command list, because they
    /// predate `Frame` and because they have no use for hit testing. Their
    /// output still has to reach the frame through [`push`](Self::push), which
    /// is what keeps the clip and translation stacks honest; handing out a
    /// `&mut Vec` into the frame's own buffer would let a `PushClip` slip past
    /// that bookkeeping and silently mis-place every later hit box.
    ///
    /// So this stages the helper's commands in a scratch list and replays them
    /// through `push`. The closure's return value is passed back, so a helper
    /// that reports where it drew stays usable:
    ///
    /// ```ignore
    /// frame.draw_with(|cmds| table.header(cmds, y, colors::OVERLAY0, 12.0));
    /// ```
    pub fn draw_with<R>(&mut self, draw: impl FnOnce(&mut Vec<RenderCommand>) -> R) -> R {
        let mut staged = Vec::new();
        let result = draw(&mut staged);
        self.extend(staged);
        result
    }

    /// Push a clip rectangle, in the coordinate space currently being drawn in.
    pub fn clip(&mut self, rect: Rect) {
        self.push(RenderCommand::PushClip {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
        });
    }

    /// Pop the innermost clip rectangle.
    pub fn unclip(&mut self) {
        self.push(RenderCommand::PopClip);
    }

    /// Shift the origin by `(dx, dy)` for subsequent drawing.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.push(RenderCommand::PushTranslate { dx, dy });
    }

    /// Undo the innermost translation.
    pub fn untranslate(&mut self) {
        self.push(RenderCommand::PopTranslate);
    }

    /// Record that `target` occupies `rect`.
    ///
    /// `rect` is in the coordinate space currently being drawn in — pass the
    /// same numbers the draw command got. The rect is moved by the translation
    /// in force and trimmed to the clip in force, and is **dropped entirely**
    /// if nothing of it is visible: a control that was clipped away is not
    /// there to be clicked.
    pub fn hit(&mut self, target: T, rect: Rect) {
        let Some(visible) = self.visible_part(rect) else {
            return;
        };
        self.hits.push((target, visible));
    }

    /// How much of `rect` a viewer could actually see, or `None` for none of
    /// it.
    ///
    /// `rect` is in the coordinate space currently being drawn in, exactly as
    /// [`hit`](Self::hit) takes it, and the answer is in window coordinates.
    ///
    /// This is the rule [`hit`](Self::hit) applies to decide whether a control
    /// is there to be clicked, made available to the drawing pass so that ink
    /// and hit boxes can be governed by *one* rule rather than two that drift
    /// apart. A clip makes what is outside it invisible; it does not make it
    /// free. A list of six hundred rows scrolled to its end still builds six
    /// hundred rectangles, measures six hundred strings and hands the renderer
    /// six hundred commands to walk past — every frame — unless the pass that
    /// draws it asks first.
    #[must_use]
    pub fn visible_part(&self, rect: Rect) -> Option<Rect> {
        let moved = rect.translated(self.offset.0, self.offset.1);
        let visible = match self.clips.last() {
            Some(clip) => clip.intersect(moved)?,
            None => moved,
        };
        if visible.is_empty() {
            return None;
        }
        Some(visible)
    }

    /// Whether any part of `rect` would be visible if drawn now.
    ///
    /// See [`visible_part`](Self::visible_part), which this is the yes-or-no
    /// form of.
    #[must_use]
    pub fn is_visible(&self, rect: Rect) -> bool {
        self.visible_part(rect).is_some()
    }

    /// Forget every target recorded so far, keeping the drawing.
    ///
    /// This is what makes a modal dialog modal. A modal draws a scrim over the
    /// whole window and its controls on top; the window behind is still
    /// *painted*, so its commands must stay, but none of it can still be
    /// clicked. Without this the toolbar behind the dialog keeps working — the
    /// dialog only looks in front.
    ///
    /// Call it immediately before drawing the modal, so the modal's own targets
    /// are recorded after it and survive.
    pub fn discard_hits(&mut self) {
        self.hits.clear();
    }

    /// The topmost target at `(x, y)` in window coordinates, or `None` for
    /// bare background.
    ///
    /// Back to front, because later commands paint over earlier ones: a modal
    /// sheet covers the list behind it, so it must also intercept its clicks.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Option<T>
    where
        T: Clone,
    {
        self.hit_test_ref(x, y).cloned()
    }

    /// [`hit_test`](Self::hit_test) without cloning, for a target type that is
    /// expensive or impossible to clone.
    #[must_use]
    pub fn hit_test_ref(&self, x: f32, y: f32) -> Option<&T> {
        self.hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| target)
    }

    /// Every box recorded this frame, in paint order.
    ///
    /// This is how a test gets its geometry: find the control, click its
    /// [`centre`](Rect::centre). A test that recomputed the coordinate from
    /// layout constants instead would keep passing after the renderer moved
    /// the control, which is the one thing it exists to catch.
    #[must_use]
    pub fn hits(&self) -> &[(T, Rect)] {
        &self.hits
    }

    /// The box recorded for the topmost target satisfying `pred`, or `None`.
    ///
    /// **Back to front, exactly like [`hit_test`](Self::hit_test)**, and for
    /// the same reason: a control can be drawn more than once in a frame — a
    /// Connect button in the toolbar *and* on the panel it belongs to — and
    /// the copy painted last is the copy a click reaches. Searching front to
    /// back would hand a test the coordinates of the buried copy, so aiming a
    /// click at the returned rect would activate a *different* control than
    /// the one that was asked for, and the test would report on whichever one
    /// it happened to hit. Two answers to "where is this control" must not
    /// disagree, which is the whole premise of this module.
    #[must_use]
    pub fn rect_of(&self, pred: impl Fn(&T) -> bool) -> Option<Rect> {
        self.hits
            .iter()
            .rev()
            .find(|(target, _)| pred(target))
            .map(|(_, rect)| *rect)
    }

    /// Whether every clip and translation pushed has been popped, and no pop
    /// arrived with nothing to pop.
    ///
    /// An unbalanced frame is a bug that is invisible in the window it happens
    /// in — an unpopped clip silently clips the rest of the frame, and an
    /// unpopped translation silently shifts it — so it is worth an assertion
    /// rather than an eventual "why is the status bar missing".
    ///
    /// The stray-pop half matters just as much and used to be missed: this
    /// asked only whether the stacks ended empty, and a helper that popped a
    /// clip it never pushed leaves them empty while having released the clip
    /// its *caller* was relying on. Everything drawn after it escapes.
    #[must_use]
    pub fn is_balanced(&self) -> bool {
        self.clips.is_empty() && self.translations.is_empty() && self.stray_pops == 0
    }

    /// The commands drawn so far.
    #[must_use]
    pub fn commands(&self) -> &[RenderCommand] {
        &self.tree.commands
    }

    /// The finished render tree, consuming the frame.
    ///
    /// Debug-asserts that the frame is [balanced](Self::is_balanced).
    #[must_use]
    pub fn into_tree(self) -> RenderTree {
        debug_assert!(
            self.is_balanced(),
            "frame ended with {} clip(s) and {} translation(s) unpopped",
            self.clips.len(),
            self.translations.len()
        );
        self.tree
    }
}

/// Lets a frame stand in for a `RenderTree` wherever a drawing helper takes
/// `&mut impl Extend<RenderCommand>` — [`crate::text::Paragraph::draw`] is the
/// one that forced this.
///
/// It routes each command through [`Frame::push`] rather than extending the
/// inner tree directly, because a helper that emits a `PushClip` must still
/// move the frame's clip stack. Extending the tree behind the frame's back
/// would leave the two disagreeing about what is clipped, which is exactly the
/// class of bug this type exists to prevent.
impl<T> Extend<RenderCommand> for Frame<T> {
    fn extend<I: IntoIterator<Item = RenderCommand>>(&mut self, iter: I) {
        for command in iter {
            self.push(command);
        }
    }
}

#[cfg(test)]
mod tests {
    // Panicking on bad data is the point of a test: an `expect` that fires is a
    // failure report, and an index out of range is the assertion.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::float_cmp
    )]

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum T {
        A,
        B,
        C,
    }

    // ---- Rect --------------------------------------------------------------

    #[test]
    fn contains_is_half_open_so_neighbours_cannot_both_claim_a_pixel() {
        let top = Rect::new(0.0, 0.0, 10.0, 10.0);
        let bottom = Rect::new(0.0, 10.0, 10.0, 10.0);
        assert!(
            top.contains(0.0, 0.0),
            "top-left corner belongs to the rect"
        );
        assert!(!top.contains(0.0, 10.0), "bottom edge belongs to the next");
        assert!(bottom.contains(0.0, 10.0));
        assert!(!top.contains(10.0, 0.0), "right edge belongs to the next");
    }

    #[test]
    fn rectangles_that_only_touch_do_not_intersect() {
        let left = Rect::new(0.0, 0.0, 10.0, 10.0);
        let right = Rect::new(10.0, 0.0, 10.0, 10.0);
        assert_eq!(left.intersect(right), None, "a zero-width overlap is none");

        let overlapping = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(
            left.intersect(overlapping),
            Some(Rect::new(5.0, 5.0, 5.0, 5.0))
        );
    }

    #[test]
    fn intersect_is_symmetric() {
        let a = Rect::new(2.0, 3.0, 20.0, 7.0);
        let b = Rect::new(5.0, 1.0, 9.0, 30.0);
        assert_eq!(a.intersect(b), b.intersect(a));
    }

    #[test]
    fn centre_is_inside_the_rect_it_came_from() {
        let rect = Rect::new(3.0, 7.0, 11.0, 5.0);
        let (x, y) = rect.centre();
        assert!(rect.contains(x, y));
    }

    // ---- Frame: the basics -------------------------------------------------

    #[test]
    fn the_topmost_recorded_target_wins() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.hit(T::A, Rect::new(0.0, 0.0, 50.0, 50.0));
        frame.hit(T::B, Rect::new(0.0, 0.0, 50.0, 50.0));
        assert_eq!(
            frame.hit_test(10.0, 10.0),
            Some(T::B),
            "later is painted on top, so later intercepts"
        );
    }

    #[test]
    fn bare_background_hits_nothing() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.hit(T::A, Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(frame.hit_test(50.0, 50.0), None);
    }

    #[test]
    fn a_zero_sized_target_is_not_recorded() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.hit(T::A, Rect::new(10.0, 10.0, 0.0, 20.0));
        assert!(
            frame.hits().is_empty(),
            "a control with no area cannot be clicked, so recording it only \
             risks it shadowing something that can"
        );
    }

    // ---- Frame: clipping ---------------------------------------------------

    #[test]
    fn a_target_is_trimmed_to_the_clip_in_force() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.clip(Rect::new(0.0, 20.0, 100.0, 60.0));
        // A row straddling the top of the pane, as a half-scrolled row does.
        frame.hit(T::A, Rect::new(0.0, 10.0, 100.0, 20.0));
        frame.unclip();

        assert_eq!(
            frame.hit_test(50.0, 15.0),
            None,
            "the half above the pane was never drawn, so it is not clickable"
        );
        assert_eq!(
            frame.hit_test(50.0, 25.0),
            Some(T::A),
            "the visible half is"
        );
        assert_eq!(frame.hits()[0].1, Rect::new(0.0, 20.0, 100.0, 10.0));
    }

    #[test]
    fn a_target_entirely_outside_the_clip_is_dropped() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.clip(Rect::new(0.0, 50.0, 100.0, 50.0));
        frame.hit(T::A, Rect::new(0.0, 0.0, 100.0, 20.0));
        frame.unclip();
        assert!(
            frame.hits().is_empty(),
            "a row scrolled fully out of sight must not keep stealing clicks \
             from whatever is painted where it used to be"
        );
    }

    #[test]
    fn a_nested_clip_can_only_shrink_the_visible_region() {
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.clip(Rect::new(0.0, 0.0, 100.0, 100.0));
        // An inner clip that asks for more than the outer one allows.
        frame.clip(Rect::new(0.0, 0.0, 200.0, 200.0));
        frame.hit(T::A, Rect::new(0.0, 0.0, 200.0, 200.0));
        frame.unclip();
        frame.unclip();
        assert_eq!(frame.hits()[0].1, Rect::new(0.0, 0.0, 100.0, 100.0));
        assert_eq!(frame.hit_test(150.0, 50.0), None);
    }

    #[test]
    fn popping_a_clip_restores_the_one_outside_it() {
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.clip(Rect::new(0.0, 0.0, 150.0, 150.0));
        frame.clip(Rect::new(0.0, 0.0, 50.0, 50.0));
        frame.unclip();
        frame.hit(T::A, Rect::new(0.0, 0.0, 200.0, 200.0));
        frame.unclip();
        assert_eq!(
            frame.hits()[0].1,
            Rect::new(0.0, 0.0, 150.0, 150.0),
            "the inner clip should not outlive its pop"
        );
    }

    #[test]
    fn a_degenerate_clip_clips_everything_away() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        // A pane squeezed to nothing by a resize.
        frame.clip(Rect::new(10.0, 10.0, 0.0, 40.0));
        frame.hit(T::A, Rect::new(10.0, 10.0, 40.0, 40.0));
        frame.unclip();
        assert!(frame.hits().is_empty());
    }

    // ---- Frame: translation ------------------------------------------------

    #[test]
    fn a_target_is_recorded_where_the_translation_actually_drew_it() {
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.translate(30.0, 40.0);
        // The renderer's own coordinates, as it would pass them to a draw call.
        frame.hit(T::A, Rect::new(0.0, 0.0, 20.0, 20.0));
        frame.untranslate();

        assert_eq!(frame.hits()[0].1, Rect::new(30.0, 40.0, 20.0, 20.0));
        assert_eq!(frame.hit_test(35.0, 45.0), Some(T::A));
        assert_eq!(
            frame.hit_test(5.0, 5.0),
            None,
            "nothing was painted at the untranslated origin"
        );
    }

    #[test]
    fn translations_accumulate_and_unwind_one_at_a_time() {
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.translate(10.0, 10.0);
        frame.translate(5.0, 0.0);
        frame.hit(T::A, Rect::new(0.0, 0.0, 10.0, 10.0));
        frame.untranslate();
        frame.hit(T::B, Rect::new(0.0, 0.0, 10.0, 10.0));
        frame.untranslate();
        frame.hit(T::C, Rect::new(0.0, 0.0, 10.0, 10.0));

        assert_eq!(frame.hits()[0].1, Rect::new(15.0, 10.0, 10.0, 10.0));
        assert_eq!(frame.hits()[1].1, Rect::new(10.0, 10.0, 10.0, 10.0));
        assert_eq!(frame.hits()[2].1, Rect::new(0.0, 0.0, 10.0, 10.0));
    }

    #[test]
    fn a_clip_pushed_under_a_translation_moves_with_it() {
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.translate(0.0, 100.0);
        // In the translated space this pane sits at y=0; on screen it is at 100.
        frame.clip(Rect::new(0.0, 0.0, 200.0, 50.0));
        frame.hit(T::A, Rect::new(0.0, 0.0, 200.0, 50.0));
        frame.unclip();
        frame.untranslate();

        assert_eq!(
            frame.hits()[0].1,
            Rect::new(0.0, 100.0, 200.0, 50.0),
            "clipping must not undo the translation the compositor applies too"
        );
        assert_eq!(frame.hit_test(10.0, 120.0), Some(T::A));
    }

    // ---- Frame: bookkeeping ------------------------------------------------

    #[test]
    fn commands_reach_the_tree_including_the_intercepted_ones() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.clip(Rect::new(0.0, 0.0, 10.0, 10.0));
        frame.translate(1.0, 1.0);
        frame.untranslate();
        frame.unclip();
        assert_eq!(
            frame.commands().len(),
            4,
            "intercepting a clip must not swallow it: the compositor needs it too"
        );
        assert!(frame.is_balanced());
        let tree = frame.into_tree();
        assert_eq!(tree.commands.len(), 4);
    }

    #[test]
    fn discarding_hits_keeps_the_drawing_and_drops_the_clicks() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            color: crate::Color::WHITE,
            corner_radii: crate::style::CornerRadii::ZERO,
        });
        frame.hit(T::A, Rect::new(0.0, 0.0, 50.0, 50.0));
        // A modal goes up: the window behind is still painted, none of it is
        // still clickable.
        frame.discard_hits();
        frame.hit(T::B, Rect::new(20.0, 20.0, 20.0, 20.0));

        assert_eq!(frame.commands().len(), 1, "the drawing must survive");
        assert_eq!(frame.hit_test(5.0, 5.0), None, "covered by the modal");
        assert_eq!(frame.hit_test(25.0, 25.0), Some(T::B));
    }

    #[test]
    fn extending_a_frame_moves_its_stacks_like_pushing_would() {
        // A drawing helper handed `&mut frame` emits its commands through
        // `Extend`. If that bypassed `push`, a helper that clipped would leave
        // the frame's stack untouched and every later `hit` would be recorded
        // unclipped — the netmanager bug, reintroduced through a side door.
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.extend([
            RenderCommand::PushTranslate { dx: 0.0, dy: 100.0 },
            RenderCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 20.0,
            },
        ]);
        frame.hit(T::A, Rect::new(0.0, 0.0, 200.0, 50.0));
        frame.extend([RenderCommand::PopClip, RenderCommand::PopTranslate]);

        assert!(frame.is_balanced());
        assert_eq!(
            frame.hits()[0].1,
            Rect::new(0.0, 100.0, 200.0, 20.0),
            "the clip and the translation both came in through `extend`"
        );
    }

    #[test]
    fn draw_with_replays_a_helpers_clip_through_the_frames_stacks() {
        // `draw_with` exists so a helper written against a plain
        // `Vec<RenderCommand>` — `Table`, above all — can still reach the frame
        // without handing out a `&mut Vec` into its buffer. Staging is only
        // worth the copy if the staged commands go back in through `push`: a
        // `PushClip` the helper emitted has to move the frame's clip stack, or
        // a hit recorded afterwards is recorded unclipped.
        let mut frame: Frame<T> = Frame::new(200.0, 200.0);
        frame.translate(0.0, 100.0);
        let reported = frame.draw_with(|cmds| {
            cmds.push(RenderCommand::PushClip {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 20.0,
            });
            "header"
        });
        assert_eq!(reported, "header", "the closure's value is passed back");

        frame.hit(T::A, Rect::new(0.0, 0.0, 200.0, 50.0));
        assert_eq!(
            frame.hits()[0].1,
            Rect::new(0.0, 100.0, 200.0, 20.0),
            "the staged clip trimmed the hit, and the translation moved it"
        );
        assert!(!frame.is_balanced(), "the staged clip is still open");

        frame.draw_with(|cmds| cmds.push(RenderCommand::PopClip));
        frame.untranslate();
        assert!(frame.is_balanced(), "a staged pop closes it again");

        // A hit outside the (now popped) clip is no longer trimmed.
        frame.hit(T::B, Rect::new(0.0, 0.0, 200.0, 50.0));
        assert_eq!(frame.hits()[1].1, Rect::new(0.0, 0.0, 200.0, 50.0));
    }

    #[test]
    fn an_unpopped_clip_or_translation_is_not_balanced() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.clip(Rect::new(0.0, 0.0, 10.0, 10.0));
        assert!(!frame.is_balanced());
        frame.unclip();
        assert!(frame.is_balanced());
        frame.translate(1.0, 1.0);
        assert!(!frame.is_balanced());
    }

    #[test]
    fn a_pop_with_nothing_to_pop_is_not_balanced() {
        // The damaging case: a helper pops a clip it never pushed, which
        // releases the clip its caller was relying on. The stacks still end
        // empty, so `is_balanced` used to call this frame fine.
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.unclip();
        assert!(!frame.is_balanced(), "a stray unclip is not balance");

        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.untranslate();
        assert!(!frame.is_balanced(), "a stray untranslate is not balance");
    }

    #[test]
    fn an_over_popped_clip_stops_trimming_the_callers_hits() {
        // Why the stray pop matters rather than merely being untidy.
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.clip(Rect::new(0.0, 0.0, 10.0, 10.0));
        frame.unclip();
        frame.unclip();
        frame.hit(T::A, Rect::new(0.0, 0.0, 200.0, 50.0));
        assert_eq!(
            frame.hits()[0].1,
            Rect::new(0.0, 0.0, 200.0, 50.0),
            "the box escaped the clip, which is the fault to report"
        );
        assert!(!frame.is_balanced());
    }

    #[test]
    fn rect_of_finds_the_first_matching_target() {
        let mut frame: Frame<T> = Frame::new(100.0, 100.0);
        frame.hit(T::A, Rect::new(0.0, 0.0, 10.0, 10.0));
        frame.hit(T::B, Rect::new(0.0, 20.0, 10.0, 10.0));
        assert_eq!(
            frame.rect_of(|t| *t == T::B),
            Some(Rect::new(0.0, 20.0, 10.0, 10.0))
        );
        assert_eq!(frame.rect_of(|t| *t == T::C), None);
    }

    #[test]
    fn hit_test_ref_works_for_a_target_that_is_not_copy() {
        let mut frame: Frame<String> = Frame::new(100.0, 100.0);
        frame.hit(String::from("row-7"), Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(
            frame.hit_test_ref(5.0, 5.0).map(String::as_str),
            Some("row-7")
        );
    }

    #[test]
    fn what_is_visible_is_what_a_hit_box_would_be_trimmed_to() {
        // The two must not drift: `visible_part` exists so a drawing pass can
        // skip ink for the same reason `hit` drops a control, and a rule kept
        // in two places is a rule that will one day disagree with itself.
        let mut frame: Frame<&str> = Frame::new(100.0, 100.0);
        frame.clip(Rect::new(0.0, 20.0, 100.0, 40.0));
        for row in [
            Rect::new(0.0, -30.0, 100.0, 20.0),
            Rect::new(0.0, 10.0, 100.0, 20.0),
            Rect::new(0.0, 30.0, 100.0, 20.0),
            Rect::new(0.0, 55.0, 100.0, 20.0),
            Rect::new(0.0, 80.0, 100.0, 20.0),
        ] {
            let expected = frame.visible_part(row);
            frame.hit("row", row);
            let recorded = frame.hits().last().filter(|_| expected.is_some());
            assert_eq!(
                expected,
                recorded.map(|(_, r)| *r),
                "{row:?} was called {expected:?} visible but recorded as {recorded:?}"
            );
            frame.discard_hits();
        }
        frame.unclip();
    }

    #[test]
    fn nothing_is_hidden_when_nothing_is_clipped() {
        // A rectangle outside the window is still "visible" to a frame with
        // no clip: the frame's own size is not a clip, and pretending it were
        // would make this disagree with `hit`, which records such a box.
        let frame: Frame<&str> = Frame::new(100.0, 100.0);
        assert!(frame.is_visible(Rect::new(500.0, 500.0, 10.0, 10.0)));
        assert!(!frame.is_visible(Rect::new(0.0, 0.0, 0.0, 10.0)));
    }
}
