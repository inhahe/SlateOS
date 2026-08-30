//! The sweep that proves a module does not draw things nobody can see.
//!
//! Sibling to [`crate::palette_check`], and born the same way: while converting
//! `power_settings` off its private colours, its charge bar turned out to push
//! the fill, then the full-width **opaque** track over it, then the same fill
//! again "on top (stacking order)". The first of those three draws could not be
//! seen by anyone — the track covers its rectangle exactly and is opaque — so a
//! third of the charge bar's commands only ever cost the compositor a blend.
//!
//! The reason this wants a test rather than an eye is that **a hidden draw is
//! hidden in a screenshot too**. Nothing about the rendered image says a
//! command was wasted; only the command list does. So the check runs on the
//! list.
//!
//! # The rule, and why it is deliberately narrow
//!
//! A fill is reported when a *later* fill contains it outright while being
//! opaque and **no more rounded at the corners**. Containment plus
//! equal-or-smaller radii is what makes coverage total: where the two outlines
//! touch they are the same curve, and everywhere else the earlier rectangle
//! lies in the later one's interior. A rounder cover would leave the earlier
//! fill's corners peeking out, so it is not reported.
//!
//! Partial overlap is left alone on purpose. Partial overlap is how every panel
//! on this desktop draws a border, a divider and a selection outline; a rule
//! that flagged it would flag the whole shell.
//!
//! The second, simpler way to draw nothing is an **empty rectangle** — a fill
//! of zero width or height. No later command covers it, so the containment rule
//! alone would never find it; it is checked separately. An empty battery used
//! to emit exactly that.

use guitk::render::RenderCommand;
use guitk::style::CornerRadii;

/// `(x, y, w, h, radii, opaque)` for a fill, or `None` for anything else.
///
/// Text and images are never treated as coverers: a glyph run's bounding box is
/// mostly not painted, and an image's alpha is not ours to assume.
fn fill(c: &RenderCommand) -> Option<(f32, f32, f32, f32, CornerRadii, bool)> {
    match c {
        RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color,
            corner_radii,
        } => Some((*x, *y, *width, *height, *corner_radii, color.a == 255)),
        _ => None,
    }
}

/// Assert no fill in `cmds` is erased by a later one, and none is empty.
///
/// `what` names the caller in the failure message; pass enough of the fixture's
/// state (`"power panel, charge=0%, tab=1, light"`) that a failure can be
/// reproduced without re-deriving it.
///
/// # Panics
///
/// Panics — that being the point — when a fill covers no pixels, or when a
/// later opaque fill covers one outright.
pub fn assert_nothing_is_drawn_and_never_seen(cmds: &[RenderCommand], what: &str) {
    for (i, under) in cmds.iter().enumerate() {
        let Some((ux, uy, uw, uh, ur, _)) = fill(under) else {
            continue;
        };
        assert!(
            uw > 0.0 && uh > 0.0,
            "{what}: command {i} is a {uw}x{uh} fill at {ux},{uy}, which covers \
             no pixels"
        );
        for over in cmds.iter().skip(i.saturating_add(1)) {
            let Some((ox, oy, ow, oh, or, opaque)) = fill(over) else {
                continue;
            };
            let contains =
                opaque && ox <= ux && oy <= uy && ox + ow >= ux + uw && oy + oh >= uy + uh;
            let no_rounder = or.top_left <= ur.top_left
                && or.top_right <= ur.top_right
                && or.bottom_right <= ur.bottom_right
                && or.bottom_left <= ur.bottom_left;
            assert!(
                !(contains && no_rounder),
                "{what}: command {i} ({uw}x{uh} at {ux},{uy}) is covered outright \
                 by a later opaque fill ({ow}x{oh} at {ox},{oy}) — it is drawn \
                 and never seen"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use guitk::color::Color;

    fn rect(x: f32, y: f32, w: f32, h: f32, a: u8, radius: f32) -> RenderCommand {
        RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: Color::rgba(200, 200, 200, a),
            corner_radii: CornerRadii::all(radius),
        }
    }

    fn rejects(cmds: &[RenderCommand]) -> bool {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            assert_nothing_is_drawn_and_never_seen(cmds, "probe");
        }))
        .is_err()
    }

    #[test]
    fn a_fill_buried_under_a_later_opaque_one_is_reported() {
        assert!(rejects(&[
            rect(10.0, 10.0, 20.0, 20.0, 255, 0.0),
            rect(0.0, 0.0, 100.0, 100.0, 255, 0.0),
        ]));
    }

    #[test]
    fn an_exactly_coincident_cover_is_reported() {
        assert!(rejects(&[
            rect(0.0, 0.0, 40.0, 6.0, 255, 3.0),
            rect(0.0, 0.0, 40.0, 6.0, 255, 3.0),
        ]));
    }

    #[test]
    fn an_empty_fill_is_reported() {
        assert!(rejects(&[rect(0.0, 0.0, 0.0, 6.0, 255, 0.0)]));
    }

    /// A translucent cover blends rather than erases, so the fill beneath it
    /// still contributes.
    #[test]
    fn a_translucent_cover_is_not_a_cover() {
        assert!(!rejects(&[
            rect(10.0, 10.0, 20.0, 20.0, 255, 0.0),
            rect(0.0, 0.0, 100.0, 100.0, 60, 0.0),
        ]));
    }

    /// Drawing order is the whole question: an opaque fill *under* a smaller
    /// one is a background, which is the commonest thing on the screen.
    #[test]
    fn a_cover_drawn_first_is_a_background() {
        assert!(!rejects(&[
            rect(0.0, 0.0, 100.0, 100.0, 255, 0.0),
            rect(10.0, 10.0, 20.0, 20.0, 255, 0.0),
        ]));
    }

    /// A rounder cover leaves the corners of what it contains peeking out.
    #[test]
    fn a_rounder_cover_is_not_a_cover() {
        assert!(!rejects(&[
            rect(0.0, 0.0, 40.0, 40.0, 255, 0.0),
            rect(0.0, 0.0, 40.0, 40.0, 255, 8.0),
        ]));
    }

    /// How every border, divider and outline on this desktop is drawn.
    #[test]
    fn a_partial_overlap_is_left_alone() {
        assert!(!rejects(&[
            rect(0.0, 0.0, 40.0, 40.0, 255, 0.0),
            rect(20.0, 20.0, 40.0, 40.0, 255, 0.0),
        ]));
    }
}
