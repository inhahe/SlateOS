//! Which pixels a frame repaints.
//!
//! A partial recomposite is only as correct as its answer to one question:
//! *which pixels of the buffer I am about to draw into are wrong?* Two things
//! go into the answer, and the compositor got both of them wrong until
//! 2026-09-24:
//!
//! - **What changed in the scene this frame** — the damage, plus every
//!   blurred window the damage reaches, since a backdrop blur's output depends
//!   on the pixels behind it (see `Compositor::spread_through_blur`).
//! - **What the buffer is missing from earlier frames.** A buffer that last
//!   held the frame `n` presents ago is missing every change of the `n - 1`
//!   frames since. [`DamageHistory`] remembers those, and
//!   `RenderTarget::buffer_age` says how far back to look. The front/back pair
//!   this compositor used to have looked back zero frames where it needed one,
//!   so every change a frame made outside the next frame's damage reverted.
//!
//! [`Region`] is the answer's shape: a set of **pairwise-disjoint**
//! rectangles. Disjointness is load-bearing rather than tidy. The repaint walks
//! windows bottom to top and, for each, draws into every rectangle of the
//! region — so a pixel lying in two overlapping rectangles would have each
//! translucent layer blended onto it twice, and a shadow or a rounded corner
//! would darken wherever two damage rectangles met.

use std::collections::VecDeque;

use crate::Rect;

/// A set of screen pixels, held as pairwise-disjoint, non-empty rectangles.
///
/// Adding a rectangle adds only the part of it not already present, so the
/// disjointness holds by construction rather than by a pass that restores it.
/// Past [`MAX_RECTS`](Self::MAX_RECTS) pieces the region gives up precision
/// and becomes its own bounding box — always correct, since repainting a pixel
/// that did not need it costs time and never changes the result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    rects: Vec<Rect>,
}

impl Region {
    /// Most rectangles a region holds before collapsing to its bounding box.
    ///
    /// Each rectangle costs one replay of every window it meets, so a region
    /// of hundreds of slivers would spend more on replays than it saves in
    /// pixels. Thirty-two is far above what a frame's damage produces — a few
    /// windows and a cursor — and far below where the replays would dominate.
    pub const MAX_RECTS: usize = 32;

    /// The empty region.
    #[must_use]
    pub const fn new() -> Self {
        Self { rects: Vec::new() }
    }

    /// The region covering exactly `rect`.
    #[must_use]
    pub fn from_rect(rect: Rect) -> Self {
        let mut region = Self::new();
        region.add(rect);
        region
    }

    /// Add every pixel of `rect`.
    pub fn add(&mut self, rect: Rect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let mut pieces = vec![rect];
        for held in &self.rects {
            if pieces.is_empty() {
                return;
            }
            pieces = pieces
                .iter()
                .flat_map(|piece| piece.subtract(held))
                .collect();
        }
        self.rects.extend(pieces);
        if self.rects.len() > Self::MAX_RECTS
            && let Some(bounds) = self.bounding_box()
        {
            self.rects.clear();
            self.rects.push(bounds);
        }
    }

    /// Add every pixel of `other`.
    pub fn add_region(&mut self, other: &Region) {
        for &rect in &other.rects {
            self.add(rect);
        }
    }

    /// The rectangles, pairwise disjoint and each non-empty.
    #[must_use]
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Whether the region holds no pixels.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Whether any pixel of `rect` is in the region.
    #[must_use]
    pub fn intersects(&self, rect: &Rect) -> bool {
        self.rects.iter().any(|held| held.intersect(rect).is_some())
    }

    /// Whether every pixel of `rect` is in the region.
    ///
    /// Summing the overlaps is exact *because* the rectangles are disjoint: no
    /// pixel of `rect` can be counted twice.
    #[must_use]
    pub fn contains_rect(&self, rect: &Rect) -> bool {
        let wanted = area(rect);
        let covered = self
            .rects
            .iter()
            .filter_map(|held| held.intersect(rect))
            .fold(0u64, |sum, overlap| sum.saturating_add(area(&overlap)));
        covered >= wanted
    }

    /// How many pixels the region holds.
    #[must_use]
    pub fn area(&self) -> u64 {
        self.rects
            .iter()
            .fold(0u64, |sum, rect| sum.saturating_add(area(rect)))
    }

    /// The smallest rectangle containing the whole region, if it has any
    /// pixels.
    #[must_use]
    pub fn bounding_box(&self) -> Option<Rect> {
        let (first, rest) = self.rects.split_first()?;
        Some(rest.iter().fold(*first, |bounds, rect| bounds.union(rect)))
    }
}

/// A rectangle's pixel count, which cannot overflow a `u64`.
fn area(rect: &Rect) -> u64 {
    u64::from(rect.width).saturating_mul(u64::from(rect.height))
}

/// What one frame changed on the screen.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// Everything: a full recomposite, or a frame after which nothing about
    /// the old pixels may be assumed.
    Everything,
    /// Only these pixels.
    Region(Region),
}

/// The changes of the last few frames, for a target whose next buffer is
/// older than the frame just presented.
///
/// Kept only as deep as a ring can be: a buffer of age `n` needs the `n - 1`
/// frames before this one, and [`Framebuffer::MAX_RING`](crate::Framebuffer::MAX_RING)
/// bounds `n`. Anything older is forgotten, and a question that reaches past
/// it is answered "repaint everything" rather than with a guess.
#[derive(Clone, Debug, Default)]
pub struct DamageHistory {
    /// Oldest first.
    frames: VecDeque<Change>,
}

impl DamageHistory {
    /// How many frames are remembered.
    pub const DEPTH: usize = crate::Framebuffer::MAX_RING.saturating_sub(1);

    /// No history: every question reaching back is answered with `None`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            frames: VecDeque::new(),
        }
    }

    /// Remember what the frame just composited changed.
    pub fn record(&mut self, change: Change) {
        if Self::DEPTH == 0 {
            return;
        }
        if self.frames.len() >= Self::DEPTH {
            self.frames.pop_front();
        }
        self.frames.push_back(change);
    }

    /// Everything the last `frames` frames changed, or `None` when the answer
    /// is "the whole screen" — one of them changed everything, or the question
    /// reaches further back than is remembered.
    ///
    /// `frames == 0` is the empty region: a buffer of age one is missing
    /// nothing.
    #[must_use]
    pub fn changed_in_last(&self, frames: usize) -> Option<Region> {
        if frames > self.frames.len() {
            return None;
        }
        let mut changed = Region::new();
        for change in self.frames.iter().rev().take(frames) {
            match change {
                Change::Everything => return None,
                Change::Region(region) => changed.add_region(region),
            }
        }
        Some(changed)
    }
}

// The five defensive lints the workspace turns on are for production code; a
// test that indexes a fixture it just built is asserting. CLAUDE.md's lint
// policy says as much.
#[cfg(test)]
#[allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]
mod tests {
    use super::*;

    /// Every pixel the region holds, as a set, by brute force.
    fn pixels(region: &Region) -> std::collections::BTreeSet<(i32, i32)> {
        let mut out = std::collections::BTreeSet::new();
        for rect in region.rects() {
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    out.insert((x, y));
                }
            }
        }
        out
    }

    /// The pixels of a list of possibly-overlapping rectangles.
    fn union_pixels(rects: &[Rect]) -> std::collections::BTreeSet<(i32, i32)> {
        let mut out = std::collections::BTreeSet::new();
        for rect in rects {
            for y in rect.y..rect.bottom() {
                for x in rect.x..rect.right() {
                    out.insert((x, y));
                }
            }
        }
        out
    }

    fn assert_disjoint(region: &Region) {
        let rects = region.rects();
        for (i, a) in rects.iter().enumerate() {
            assert!(a.width > 0 && a.height > 0, "empty piece {a:?}");
            for b in rects.iter().skip(i + 1) {
                assert!(
                    a.intersect(b).is_none(),
                    "{a:?} and {b:?} overlap: a pixel there would be painted twice"
                );
            }
        }
    }

    /// The property the repaint depends on, checked against brute force over
    /// a spread of overlapping, nested, touching and repeated rectangles.
    #[test]
    fn a_region_holds_exactly_the_pixels_added_and_no_pixel_twice() {
        let cases: &[&[Rect]] = &[
            &[Rect::new(0, 0, 10, 10)],
            &[Rect::new(0, 0, 10, 10), Rect::new(5, 5, 10, 10)],
            &[Rect::new(0, 0, 10, 10), Rect::new(2, 2, 3, 3)],
            &[Rect::new(2, 2, 3, 3), Rect::new(0, 0, 10, 10)],
            &[Rect::new(0, 0, 10, 10), Rect::new(10, 0, 10, 10)],
            &[Rect::new(0, 0, 10, 10), Rect::new(0, 0, 10, 10)],
            &[
                Rect::new(0, 0, 4, 20),
                Rect::new(0, 0, 20, 4),
                Rect::new(8, 8, 8, 8),
                Rect::new(-3, 6, 30, 2),
            ],
        ];
        for rects in cases {
            let mut region = Region::new();
            for &rect in *rects {
                region.add(rect);
            }
            assert_disjoint(&region);
            assert_eq!(pixels(&region), union_pixels(rects), "for {rects:?}");
            assert_eq!(region.area(), union_pixels(rects).len() as u64);
        }
    }

    #[test]
    fn an_empty_rectangle_adds_nothing() {
        let mut region = Region::new();
        region.add(Rect::new(5, 5, 0, 10));
        region.add(Rect::new(5, 5, 10, 0));
        assert!(region.is_empty());
        assert_eq!(region.bounding_box(), None);
    }

    /// Past the cap the region becomes its bounding box: more pixels, never
    /// fewer. Fewer would be a missed repaint; more is only wasted work.
    #[test]
    fn too_many_pieces_collapse_to_a_box_that_still_holds_every_pixel() {
        let mut region = Region::new();
        let mut added = Vec::new();
        for i in 0..(Region::MAX_RECTS as i32 + 8) {
            let rect = Rect::new(i * 3, (i % 5) * 7, 2, 2);
            added.push(rect);
            region.add(rect);
        }
        assert!(region.rects().len() <= Region::MAX_RECTS);
        assert_disjoint(&region);
        let held = pixels(&region);
        for p in union_pixels(&added) {
            assert!(held.contains(&p), "{p:?} was added and is no longer held");
        }
    }

    #[test]
    fn containment_and_intersection_answer_for_the_whole_region() {
        let mut region = Region::new();
        region.add(Rect::new(0, 0, 10, 10));
        region.add(Rect::new(10, 0, 10, 10));
        // Straddles the seam between two pieces: contained by neither alone,
        // contained by the region.
        assert!(region.contains_rect(&Rect::new(5, 2, 10, 5)));
        assert!(!region.contains_rect(&Rect::new(15, 5, 10, 10)));
        assert!(region.intersects(&Rect::new(15, 5, 10, 10)));
        assert!(
            !region.intersects(&Rect::new(20, 0, 5, 5)),
            "touching is not meeting"
        );
        assert_eq!(region.bounding_box(), Some(Rect::new(0, 0, 20, 10)));
    }

    #[test]
    fn history_answers_for_the_frames_it_remembers_and_no_further() {
        let mut history = DamageHistory::new();
        // Nothing remembered: a buffer of age one needs nothing, anything
        // older needs everything.
        assert_eq!(history.changed_in_last(0), Some(Region::new()));
        assert_eq!(history.changed_in_last(1), None);

        history.record(Change::Region(Region::from_rect(Rect::new(0, 0, 4, 4))));
        history.record(Change::Region(Region::from_rect(Rect::new(10, 0, 4, 4))));
        let last = history.changed_in_last(1).expect("one frame back");
        assert_eq!(last.rects(), &[Rect::new(10, 0, 4, 4)]);
        let both = history.changed_in_last(2).expect("two frames back");
        assert_eq!(both.area(), 32);
        assert_eq!(history.changed_in_last(3), None, "only two frames happened");
    }

    #[test]
    fn a_frame_that_changed_everything_poisons_every_answer_reaching_it() {
        let mut history = DamageHistory::new();
        history.record(Change::Everything);
        history.record(Change::Region(Region::from_rect(Rect::new(0, 0, 4, 4))));
        assert!(
            history.changed_in_last(1).is_some(),
            "the full frame is two back"
        );
        assert_eq!(history.changed_in_last(2), None);
    }

    #[test]
    fn history_forgets_what_no_ring_could_ask_about() {
        let mut history = DamageHistory::new();
        history.record(Change::Everything);
        for i in 0..DamageHistory::DEPTH {
            history.record(Change::Region(Region::from_rect(Rect::new(
                i as i32 * 10,
                0,
                2,
                2,
            ))));
        }
        // The full frame has aged out, so the deepest question is answerable.
        assert!(history.changed_in_last(DamageHistory::DEPTH).is_some());
        assert_eq!(history.changed_in_last(DamageHistory::DEPTH + 1), None);
    }
}
