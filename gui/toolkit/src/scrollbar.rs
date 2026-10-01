//! Where a scrollbar's thumb sits, where a drag of it lands, and how the bar
//! is drawn.
//!
//! # Why the drawing is here too
//!
//! This module began as arithmetic alone: a scrollbar's look belonged to
//! whatever drew the list, and what the callers shared was the thumb's
//! geometry. A theme's widget style (`Palette::widget_style`'s `scrollbar`,
//! design-decisions 1435) changed that -- a theme that asks for thin bars, or
//! bars that stay out of the way until a hand goes to them, asks it of every
//! scrollbar, so every scrollbar has to be drawn by the one function that
//! keeps the promise: [`draw`].
//!
//! **The column never moves.** A bar is drawn *inside* its column -- the strip
//! at the view's side that takes a press on the bar, [`WIDTH`] wide in every
//! theme -- and never wider. The toolkit answers a click by laying a widget
//! out again with any palette to hand, on the rule that where things land
//! does not depend on colour, so a style that moved the column would put a
//! click somewhere other than the bar. What the style chooses is the drawing:
//! the full column or a thin bar at its outer edge, and a track always there
//! or a thin line that widens into the bar when the pointer comes to it.
//!
//! **Not every bar is a scrollbar.** The menus and the start menu draw a
//! four-pixel scroll *indicator* -- no track to press, no thumb to take hold
//! of, only a mark of where the list is -- and those stay as they are: a thin
//! mark already, with nothing for a style to widen or hide.
//!
//! # Why it exists at all
//!
//! Six places in this tree drew a scrollbar with their own copy of the same
//! formula. Five now call this module: `gui/toolkit`'s file dialog, `menu` and
//! `menubar`, the desktop shell's start menu, and `apps/dictionary`. Six copies
//! of one formula is six chances to write a different one — the same reasoning
//! that collapsed the glob matchers (design-decisions 555).
//!
//! **The sixth is `apps/spreadsheet`, and it is deliberately left alone.** Its
//! scrollbar is generic over the axis — one function draws both the vertical
//! and the horizontal bar from a `length` — where everything here is vertical,
//! reading `track.h` and `track.y` by name. Converting it means either
//! generalising this module to an axis-agnostic span or splitting that function
//! in two, and both are design decisions rather than mechanical substitutions.
//!
//! (A note on the count, because this module's own history has it wrong. The
//! doc first said six and named `gui/desktop/src/window_peek.rs` as one of
//! them; that file has no scrollbar. The grep behind the number had matched
//! `max_thumb_height` and `MIN_THUMBNAIL_WIDTH`, which size a window *preview
//! image*. It then said five, having dropped `window_peek` without looking for
//! what else the first grep had missed — which was `apps/spreadsheet`. Six was
//! right by accident and wrong in its membership, twice. Enumerate, then count.)
//!
//! **They had drifted, and on the part that shows.** Each had a different rule
//! for the smallest a thumb may get: 20 px in the dialog, 16 px in `menu` and
//! `menubar`, half a row in the shell, five per cent of the pane in
//! `dictionary`. That is why [`thumb_of`] takes the floor as an argument
//! instead of imposing [`MIN_THUMB`]: a menu is not a file dialog, and
//! collapsing five deliberate-looking choices into one would be a visual change
//! smuggled in under a refactor. What is shared is the arithmetic.
//!
//! The extraction started from the file dialog's copy, which was the most
//! carefully documented and the only one with tests for the end-of-list case;
//! its tests are what guard that this module says the same thing it did.

use crate::frame::Rect;
use crate::palette::{Palette, emphasized};
use crate::render::RenderCommand;
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::widget_style::ScrollbarVisibility;

/// Width of a scrollbar's column: the strip at a view's side that takes a
/// press on the bar, the same in every theme. The bar is drawn inside it, as
/// wide as the theme's style says and never wider ([`draw`]).
pub const WIDTH: f32 = 10.0;

/// How wide an overlaid bar's thumb is drawn while the pointer is elsewhere: a
/// line, still saying where the view is.
pub const IDLE_WIDTH: f32 = 3.0;

/// How round the thumb's ends are, at most: a hint of a corner on a bar that
/// fills its column, a pill on a thin one.
const THUMB_RADIUS: f32 = 3.0;

/// Shortest the thumb may get.
///
/// A thumb sized strictly in proportion to the visible fraction of a very long
/// listing shrinks to a couple of pixels, which is both invisible and too small
/// to grab. Every real scrollbar imposes a floor for the same reason; the cost
/// is that the thumb's *size* stops being a faithful proportion once the list
/// is long, which nobody reads it for, while its *position* stays exact.
pub const MIN_THUMB: f32 = 20.0;

/// Whether a list this long needs a scrollbar at all.
///
/// A caller that draws one unconditionally leaves a permanent grey stripe
/// beside a three-item list, and one that forgets to ask draws a full-height
/// thumb that cannot move — both of which read as "broken" rather than "empty".
#[must_use]
pub fn needed(total: usize, capacity: usize) -> bool {
    total > capacity
}

/// The thumb's rectangle inside `track`, for a list of `total` rows showing
/// `capacity` of them starting at row `first`.
///
/// Size shows how much of the listing is on screen; position shows where in it.
/// The two are computed separately because [`MIN_THUMB`] makes the size stop
/// being proportional for a long listing while the position must stay exact — a
/// thumb that reached the bottom of its track only when the last row was
/// reached, but sat at 90% when the listing was at its end, would be worse than
/// no thumb at all.
///
/// A `total` of zero yields a full-height thumb rather than a division by zero:
/// an empty list is entirely on screen.
#[must_use]
pub fn thumb(track: Rect, total: usize, capacity: usize, first: usize) -> Rect {
    if total == 0 {
        return track;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a row count large enough to lose f32 precision is 16M rows"
    )]
    let shown = capacity as f32 / total as f32;
    let hidden = total.saturating_sub(capacity);
    #[expect(
        clippy::cast_precision_loss,
        reason = "as above; the ratio is what matters and it is exact enough"
    )]
    let position = if hidden == 0 {
        0.0
    } else {
        first as f32 / hidden as f32
    };
    thumb_of(track, shown, position, MIN_THUMB)
}

/// The thumb's rectangle from two fractions, with an explicit size floor.
///
/// The shape underneath [`thumb`], for the callers that do not count rows: a
/// menu scrolls by pixels, and a pane by a fraction of its content. `shown` is
/// how much of the content is on screen and `position` how far through it the
/// view has travelled, both clamped to `0.0..=1.0`.
///
/// `min_thumb` is an argument rather than [`MIN_THUMB`] because the callers
/// disagree about it on purpose -- a menu's floor is smaller than a file
/// dialog's -- and imposing one value would be a visual change wearing a
/// refactor's clothes.
///
/// A non-finite fraction is read as zero. These come from a division whose
/// denominator a caller may not have checked, and a `NaN` reaching the
/// multiply would put the thumb at a position that compares false against
/// every bound.
#[must_use]
pub fn thumb_of(track: Rect, shown: f32, position: f32, min_thumb: f32) -> Rect {
    let shown = if shown.is_finite() {
        shown.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let position = if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let thumb_h = (track.h * shown).clamp(min_thumb.min(track.h), track.h);
    Rect::new(
        track.x,
        track.y + (track.h - thumb_h) * position,
        track.w,
        thumb_h,
    )
}

/// What is happening to a scrollbar now, which its owner knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BarState {
    /// The pointer is over the bar's column.
    pub hovered: bool,
    /// The thumb is held.
    pub dragging: bool,
}

/// Draw a scrollbar whose column is `track`, with its thumb at `thumb` (from
/// [`thumb`] or [`thumb_of`], in the same column), in the palette's colours
/// and its widget style's form.
///
/// The track is the palette's `surface0` and the thumb `surface2` -- "a
/// scrollbar thumb" is that role's documented job -- lit a step under the
/// pointer and while held, as every control here is. Drawn at the column's
/// outer (right) edge, as wide as the style says:
///
/// - **always**: the track and the thumb;
/// - **overlay**: no track, and the thumb [`IDLE_WIDTH`] wide until the
///   pointer is over the column or the thumb is held, when it is drawn full.
///
/// The caller records its own hit regions -- the whole column for the track,
/// the whole thumb rectangle for the thumb -- whatever is drawn: a thin bar is
/// taken hold of over its column, which is more than it looks and never less.
pub fn draw(sink: &mut impl CommandSink, p: &Palette, track: Rect, thumb: Rect, state: BarState) {
    let style = p.widget_style.scrollbar;
    let full = f32::from(style.width.pixels()).min(track.w.max(0.0));
    let lit = state.hovered || state.dragging;
    let (drawn, show_track) = match style.visibility {
        ScrollbarVisibility::Always => (full, true),
        ScrollbarVisibility::Overlay if lit => (full, false),
        ScrollbarVisibility::Overlay => (IDLE_WIDTH.min(full), false),
    };
    let x = track.right() - drawn;
    let ends = THUMB_RADIUS.min(drawn / 2.0);
    if show_track {
        sink.emit(RenderCommand::FillRect {
            x,
            y: track.y,
            width: drawn,
            height: track.h,
            color: p.surface0,
            corner_radii: CornerRadii::ZERO,
        });
    }
    sink.emit(RenderCommand::FillRect {
        x,
        y: thumb.y,
        width: drawn,
        height: thumb.h,
        color: if lit {
            emphasized(p.surface2)
        } else {
            p.surface2
        },
        corner_radii: CornerRadii::all(ends),
    });
}

/// The first visible row a thumb drag lands on.
///
/// `grab` is how far below the thumb's own top the pointer took hold, which is
/// what stops the thumb jumping under the pointer on the first move: the thumb
/// follows the grab point, not the pointer.
///
/// Returns `None` when there is nothing to scroll — an empty track, a thumb as
/// tall as its track, or a list that fits — so a caller does not move the view
/// to row zero on a drag that meant nothing.
#[must_use]
pub fn first_from_drag(
    track: Rect,
    thumb_h: f32,
    grab: f32,
    pointer_y: f32,
    total: usize,
    capacity: usize,
) -> Option<usize> {
    let span = track.h - thumb_h;
    let hidden = total.saturating_sub(capacity);
    if span <= 0.0 || hidden == 0 {
        return None;
    }
    let fraction = ((pointer_y - grab - track.y) / span).clamp(0.0, 1.0);
    #[expect(
        clippy::cast_precision_loss,
        reason = "the hidden-row count is far inside f32's exact range"
    )]
    let rows = fraction * hidden as f32;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded, and bounded by `hidden` because `fraction` is clamped"
    )]
    let first = rows.round() as usize;
    Some(first.min(hidden))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    use crate::widget_style::{ScrollbarStyle, ScrollbarWidth};

    const TRACK: Rect = Rect {
        x: 100.0,
        y: 0.0,
        w: WIDTH,
        h: 200.0,
    };

    /// The fills `draw` makes: `(x, width, colour)` each.
    fn fills(p: &Palette, state: BarState) -> Vec<(f32, f32, crate::color::Color)> {
        let thumb = thumb(TRACK, 100, 10, 30);
        let mut cmds: Vec<RenderCommand> = Vec::new();
        draw(&mut cmds, p, TRACK, thumb, state);
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect {
                    x, width, color, ..
                } => Some((*x, *width, *color)),
                _ => None,
            })
            .collect()
    }

    fn styled(width: ScrollbarWidth, visibility: ScrollbarVisibility) -> Palette {
        let mut p = Palette::for_mode(false);
        p.widget_style.scrollbar = ScrollbarStyle { width, visibility };
        p
    }

    /// **The built-in bar is the one the toolkit always drew**: a `surface0`
    /// track the width of the column, a `surface2` thumb in it.
    #[test]
    fn the_built_in_bar_fills_its_column() {
        let p = Palette::for_mode(false);
        assert_eq!(
            fills(&p, BarState::default()),
            [(TRACK.x, WIDTH, p.surface0), (TRACK.x, WIDTH, p.surface2)]
        );
    }

    /// **A thin bar is drawn at the column's outer edge**, the column itself
    /// unchanged -- so the press still lands over all of it.
    #[test]
    fn a_thin_bar_sits_at_the_outer_edge_of_its_column() {
        let p = styled(ScrollbarWidth::Thin, ScrollbarVisibility::Always);
        let x = TRACK.right() - 6.0;
        assert_eq!(
            fills(&p, BarState::default()),
            [(x, 6.0, p.surface0), (x, 6.0, p.surface2)]
        );
    }

    /// **An overlaid bar is a line until a hand comes to it**: no track, the
    /// thumb [`IDLE_WIDTH`] wide at the edge, and its full width, lit, while
    /// the pointer is over the column or the thumb is held.
    #[test]
    fn an_overlaid_bar_is_a_line_until_the_pointer_comes() {
        let p = styled(ScrollbarWidth::Normal, ScrollbarVisibility::Overlay);
        assert_eq!(
            fills(&p, BarState::default()),
            [(TRACK.right() - IDLE_WIDTH, IDLE_WIDTH, p.surface2)]
        );
        for state in [
            BarState {
                hovered: true,
                dragging: false,
            },
            BarState {
                hovered: false,
                dragging: true,
            },
        ] {
            assert_eq!(
                fills(&p, state),
                [(TRACK.x, WIDTH, emphasized(p.surface2))],
                "{state:?}"
            );
        }
    }

    /// **The pointer lights the thumb** in the ordinary bar too, as it lights
    /// every control.
    #[test]
    fn the_pointer_lights_the_thumb() {
        let p = Palette::for_mode(false);
        let lit = fills(
            &p,
            BarState {
                hovered: true,
                dragging: false,
            },
        );
        assert_eq!(lit[1].2, emphasized(p.surface2));
        assert_eq!(lit[0].2, p.surface0, "the track stays");
    }

    /// **No bar is drawn wider than its column**, whatever the column is.
    #[test]
    fn no_bar_is_wider_than_its_column() {
        let p = Palette::for_mode(false);
        let narrow = Rect::new(0.0, 0.0, 4.0, 100.0);
        let mut cmds: Vec<RenderCommand> = Vec::new();
        draw(&mut cmds, &p, narrow, narrow, BarState::default());
        for c in &cmds {
            if let RenderCommand::FillRect { x, width, .. } = c {
                assert!(*x >= 0.0 && *width <= 4.0, "{c:?}");
            }
        }
    }

    #[test]
    fn a_list_that_fits_needs_no_scrollbar() {
        assert!(!needed(10, 10));
        assert!(!needed(3, 10));
        assert!(needed(11, 10));
    }

    #[test]
    fn the_thumb_fills_the_track_when_everything_is_on_screen() {
        let t = thumb(TRACK, 10, 10, 0);
        assert_eq!(t.h, TRACK.h);
        assert_eq!(t.y, TRACK.y);
    }

    #[test]
    fn an_empty_list_does_not_divide_by_zero() {
        let t = thumb(TRACK, 0, 10, 0);
        assert_eq!(t.h, TRACK.h);
    }

    #[test]
    fn the_thumb_sits_at_the_end_when_the_list_is_at_its_end() {
        // The property the size floor puts at risk. A very long list gives a
        // thumb pinned to `MIN_THUMB`, so its *size* is no longer a faithful
        // proportion -- but its *position* must still reach the bottom of the
        // track exactly when the last row is showing, or the bar lies about
        // where you are.
        let total = 10_000;
        let capacity = 20;
        let t = thumb(TRACK, total, capacity, total - capacity);
        assert_eq!(
            t.h, MIN_THUMB,
            "a huge list should pin the thumb to the floor"
        );
        assert!(
            (t.y + t.h - (TRACK.y + TRACK.h)).abs() < 0.01,
            "the thumb stopped at {} instead of the track's end {}",
            t.y + t.h,
            TRACK.y + TRACK.h
        );
    }

    #[test]
    fn the_floor_is_the_callers_and_not_this_modules() {
        // Five callers disagree about the smallest a thumb may be -- 20 px in
        // the file dialog, 16 in the menus, half a row in the shell, three bar
        // widths in the dictionary -- and each is a deliberate choice about
        // its own surface. Imposing one would be a visual change hidden in a
        // refactor, so the floor is an argument.
        let tiny = thumb_of(TRACK, 0.001, 0.0, 4.0);
        assert_eq!(tiny.h, 4.0, "the caller's floor was not honoured");
        let bigger = thumb_of(TRACK, 0.001, 0.0, 40.0);
        assert_eq!(bigger.h, 40.0);
    }

    #[test]
    fn a_floor_taller_than_the_track_does_not_overflow_it() {
        let t = thumb_of(TRACK, 0.01, 1.0, TRACK.h * 4.0);
        assert_eq!(t.h, TRACK.h);
        assert_eq!(t.y, TRACK.y, "a full-height thumb has nowhere to travel");
    }

    #[test]
    fn a_non_finite_fraction_does_not_put_the_thumb_nowhere() {
        // Both fractions come from a division whose denominator the caller may
        // not have checked -- content height, a row count, a max-scroll of
        // zero. A NaN reaching the multiply compares false against every bound,
        // so the thumb would be drawn at a position nothing could clamp.
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let t = thumb_of(TRACK, bad, 0.5, MIN_THUMB);
            assert!(
                t.h.is_finite() && t.y.is_finite(),
                "shown={bad} produced {t:?}"
            );
            let t = thumb_of(TRACK, 0.5, bad, MIN_THUMB);
            assert!(
                t.h.is_finite() && t.y.is_finite(),
                "position={bad} produced {t:?}"
            );
            assert!(t.y >= TRACK.y && t.y + t.h <= TRACK.y + TRACK.h + 0.01);
        }
    }

    #[test]
    fn the_thumb_never_leaves_its_track() {
        for first in [0, 1, 50, 499, 500, 10_000] {
            let t = thumb(TRACK, 500, 20, first);
            assert!(t.y >= TRACK.y, "thumb above the track at first={first}");
            assert!(
                t.y + t.h <= TRACK.y + TRACK.h + 0.01,
                "thumb below the track at first={first}"
            );
        }
    }

    #[test]
    fn dragging_to_the_bottom_shows_the_last_page() {
        let t = thumb(TRACK, 100, 10, 0);
        let first = first_from_drag(TRACK, t.h, 0.0, TRACK.y + TRACK.h, 100, 10);
        assert_eq!(
            first,
            Some(90),
            "a drag to the bottom should show the last page"
        );
    }

    #[test]
    fn dragging_above_the_track_shows_the_first_page() {
        let t = thumb(TRACK, 100, 10, 50);
        let first = first_from_drag(TRACK, t.h, 0.0, TRACK.y - 500.0, 100, 10);
        assert_eq!(first, Some(0));
    }

    #[test]
    fn the_grab_offset_keeps_the_thumb_under_the_pointer() {
        // Grabbing the thumb halfway down and moving nowhere must not move the
        // view: without the offset the thumb's *top* would jump to the pointer.
        let t = thumb(TRACK, 100, 10, 40);
        let grab = t.h / 2.0;
        let first = first_from_drag(TRACK, t.h, grab, t.y + grab, 100, 10);
        assert_eq!(first, Some(40), "a drag that moved nowhere moved the view");
    }

    #[test]
    fn a_drag_with_nothing_to_scroll_reports_nothing() {
        assert_eq!(first_from_drag(TRACK, TRACK.h, 0.0, 50.0, 10, 10), None);
        assert_eq!(first_from_drag(TRACK, 20.0, 0.0, 50.0, 5, 10), None);
    }
}
