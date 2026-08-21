//! What one turn of the mouse wheel means.
//!
//! [`MouseEventKind::Scroll`] carries `dx`/`dy` in **wheel notches**: `1.0` per
//! detent of an ordinary wheel, positive away from the user, and a *fraction*
//! of one for a high-resolution wheel or a precision trackpad. That is what the
//! only producer in the tree emits — `wheel_delta()` in the compositor's
//! host-window input path returns `raw / WHEEL_DELTA`, where Windows' `raw` is
//! a signed multiple of 120.
//!
//! The declaration used to say "pixels", and the cost of that one wrong word is
//! the reason this module exists. Twelve independent consumers each invented an
//! interpretation:
//!
//! | What they wrote | One notch moved |
//! |---|---|
//! | `dy * 40.0` | 40 px |
//! | `dy * 20.0` | 20 px |
//! | `dy` | 1 px — visually nothing |
//! | `(dy / line_height * 3.0) as i64` | **0 lines — the wheel was dead** |
//! | `if dy > 0 { 3 } else { -3 }` | 3 rows |
//!
//! The fourth row is the instructive one. It is the only consumer that took the
//! doc comment at its word, and dividing a notch count by a line height gives
//! `1.0 / 21.0 * 3.0 = 0.14`, which truncates to zero: that editor's wheel did
//! nothing at all, at any speed. A unit that is documented wrong does not
//! produce a small error, it produces a silent one.
//!
//! # Rows, not pixels
//!
//! Most scrollable things here draw **whole rows**, and their offsets are row
//! indices (see [`scroll_window`]). So the useful conversion is notches → rows,
//! and it is [`Accumulator::rows`].
//!
//! It has to accumulate rather than round, which is the whole reason it is a
//! struct and not a function. A precision trackpad sends a stream of small
//! fractions — `0.2`, `0.2`, `0.2`… — and a converter that rounded each event
//! on its own would return zero every time and never scroll, which is exactly
//! the bug above in a different disguise. Keeping the remainder means the fifth
//! such event moves a row, and the device feels smooth instead of dead.
//!
//! ```
//! use guitk::wheel::Accumulator;
//!
//! let mut acc = Accumulator::default();
//! // An ordinary wheel: one notch is one full step.
//! assert_eq!(acc.rows(-1.0), 3, "one notch down scrolls three rows down");
//!
//! // A trackpad: fractions accumulate instead of vanishing.
//! let mut acc = Accumulator::default();
//! assert_eq!(acc.rows(-0.2), 0);
//! assert_eq!(acc.rows(-0.2), 1, "0.4 notches is 1.2 rows");
//! ```
//!
//! # When the offset is already continuous
//!
//! Some views hold an offset that is *not* a row index but is still continuous:
//! a fractional row count (a list that can sit half a row down), or a pixel
//! offset into a zoomed image or a canvas. These must **not** use an
//! accumulator — banking a fraction until it rounds is exactly the wrong thing
//! for an offset that could have shown the fraction. A fifth of a notch should
//! move a fifth of a notch, immediately.
//!
//! [`rows_f`] is the fractional-row conversion and [`pixels`] the pixel one;
//! `pixels(dy, row_h)` is just `rows_f(dy) * row_h`. Prefer whole rows wherever
//! the content really has rows and the renderer rounds to them anyway: a
//! continuous offset can only express positions that are then rounded away.
//!
//! [`MouseEventKind::Scroll`]: crate::event::MouseEventKind::Scroll
//! [`scroll_window`]: crate::scroll_window

/// Rows moved per notch of the wheel.
///
/// Three is the near-universal platform default (Windows' `SPI_GETWHEELSCROLLLINES`
/// ships as 3, GTK and Qt both use 3), and it is what the majority of this
/// tree's hand-rolled handlers had already picked independently.
pub const ROWS_PER_NOTCH: f32 = 3.0;

/// Turns a stream of wheel notches into a stream of whole-row movements,
/// without losing the fractions a high-resolution device sends.
///
/// One of these belongs to each independently-scrollable view, next to the
/// offset it drives — two views sharing an accumulator would steal each other's
/// remainders. It is `Default`-constructible and holds one `f32`, so it costs
/// nothing to give every view its own.
///
/// # Sign
///
/// `dy` is positive *away from the user*, and scrolling away moves the view
/// *up* — towards row 0. So [`rows`] returns a delta in the same direction as
/// the row index: negative for a scroll away from the user, ready to hand
/// straight to [`scroll_window::shift`].
///
/// [`rows`]: Accumulator::rows
/// [`scroll_window::shift`]: crate::scroll_window::shift
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Accumulator {
    /// Rows earned but not yet delivered, always in `(-1.0, 1.0)`.
    residue: f32,
}

impl Accumulator {
    /// The whole rows this event moves the view by, carrying the remainder.
    ///
    /// Positive result means "towards the end of the list", matching the
    /// direction of a row index; see the sign note on [`Accumulator`].
    ///
    /// A non-finite `dy` contributes nothing and leaves the remainder alone.
    /// Input events come from outside this process, and a `NaN` that reached
    /// the residue would poison every later event through it — the view would
    /// stop scrolling permanently, with no way back short of restarting the
    /// app.
    pub fn rows(&mut self, dy: f32) -> isize {
        self.rows_at(dy, ROWS_PER_NOTCH)
    }

    /// [`rows`] with an explicit rows-per-notch, for a view that genuinely
    /// needs a different step — a list of very tall rows, say.
    ///
    /// [`rows`]: Accumulator::rows
    pub fn rows_at(&mut self, dy: f32, rows_per_notch: f32) -> isize {
        if !dy.is_finite() || !rows_per_notch.is_finite() {
            return 0;
        }
        // Negated because `dy` is positive away from the user, which scrolls
        // the view towards row 0.
        let wanted = self.residue - dy * rows_per_notch;
        if !wanted.is_finite() {
            // A large-enough `dy` can overflow the sum to infinity even though
            // both inputs were finite. Drop the event rather than store the
            // infinity, for the same reason as the NaN check above.
            return 0;
        }
        let whole = wanted.trunc();
        self.residue = wanted - whole;
        // `whole` is a truncated f32 and fits comfortably; the saturating cast
        // covers the pathological `dy` that makes it enormous.
        #[allow(clippy::cast_possible_truncation)]
        {
            whole as isize
        }
    }

    /// Forget the outstanding fraction.
    ///
    /// Worth calling when the view the accumulator drives is replaced —
    /// switching tabs, or opening a different document — so that a fraction
    /// earned in the old view cannot deliver a row in the new one.
    pub fn reset(&mut self) {
        self.residue = 0.0;
    }
}

/// Notches to **fractional rows**, for a view whose offset is a row count that
/// need not be whole.
///
/// This is [`Accumulator::rows`] without the accumulator, and choosing between
/// them is a question about the *offset*, not about the device. An accumulator
/// exists to stop a trackpad's `0.2`-notch events rounding to zero forever — a
/// hazard that only arises because the offset it drives is an integer and
/// cannot hold the fraction. An offset that is already an `f32` can, so banking
/// the fraction would be strictly worse: it would sit on movement the view was
/// perfectly able to show, and make a precision device feel stepped when the
/// whole point of one is that it is not.
///
/// Sign matches [`Accumulator::rows`]: positive means "towards the end of the
/// list", i.e. a larger offset. Non-finite input gives `0.0`.
///
/// ```
/// use guitk::wheel::rows_f;
/// assert_eq!(rows_f(-1.0), 3.0, "one notch down is three rows");
/// assert_eq!(rows_f(-0.2), 0.6, "and a fifth of a notch really moves");
/// assert_eq!(rows_f(f32::NAN), 0.0);
/// ```
#[must_use]
pub fn rows_f(dy: f32) -> f32 {
    if !dy.is_finite() {
        return 0.0;
    }
    // Negated for the same reason as `Accumulator::rows_at`: `dy` is positive
    // away from the user, which scrolls the view towards row 0.
    let out = -dy * ROWS_PER_NOTCH;
    // A finite `dy` near `f32::MAX` still overflows the product to infinity,
    // and an infinity added to a stored offset freezes the view for good.
    if out.is_finite() { out } else { 0.0 }
}

/// Notches to pixels, for a view that really is continuous.
///
/// `row_h` is what a "row" would be in this view — a line height for text, a
/// sensible step for a canvas. A notch moves [`ROWS_PER_NOTCH`] of them, so the
/// wheel travels the same distance here as it does over a list. It is exactly
/// [`rows_f`] scaled by `row_h`, and is written that way so the two conversions
/// cannot drift apart.
///
/// Sign matches [`Accumulator::rows`]: positive result means "towards the end",
/// i.e. a larger pixel offset.
///
/// Non-finite input gives `0.0`, so a bad event cannot poison a stored offset.
///
/// ```
/// use guitk::wheel::pixels;
/// assert_eq!(pixels(-1.0, 20.0), 60.0, "one notch down is three 20px rows");
/// assert_eq!(pixels(1.0, 20.0), -60.0);
/// assert_eq!(pixels(f32::NAN, 20.0), 0.0);
/// ```
#[must_use]
pub fn pixels(dy: f32, row_h: f32) -> f32 {
    if !dy.is_finite() || !row_h.is_finite() {
        return 0.0;
    }
    let out = rows_f(dy) * row_h;
    if out.is_finite() { out } else { 0.0 }
}

/// Notches to pixels for the **horizontal** wheel — which is *not* [`pixels`]
/// with `dx` substituted for `dy`, because the two axes disagree about sign.
///
/// `WM_MOUSEWHEEL`'s delta is positive **away from the user**, and scrolling
/// away moves the view *towards the start*, so the vertical converter negates.
/// `WM_MOUSEHWHEEL`'s delta is positive **to the right**, and tilting right
/// moves the view *towards the right* — towards the end. Same producer
/// (`wheel_delta`), same units, opposite relationship to the offset it drives.
/// So this one does not negate, and a caller who reached for [`pixels`] because
/// the name looked right would scroll every horizontal wheel backwards.
///
/// That is not a hypothetical. When this was written no consumer in the tree
/// read `dx` at all — twelve handlers, every one of them dropping it — so the
/// first one to try had nothing to copy from and a plausible-looking wrong
/// answer sitting next to the right one. Hence a separate function with the
/// asymmetry in its name and its doc, rather than a comment at each call site.
///
/// `col_w` is what a "column" is in this view, exactly as `row_h` is a row in
/// [`pixels`]: a notch moves [`ROWS_PER_NOTCH`] of them, so a horizontal notch
/// travels as far across as a vertical one travels down.
///
/// Non-finite input gives `0.0`, for the same reason as [`pixels`].
///
/// ```
/// use guitk::wheel::{pixels, pixels_x};
/// assert_eq!(pixels_x(1.0, 20.0), 60.0, "tilt right: towards the end");
/// assert_eq!(pixels_x(-1.0, 20.0), -60.0);
/// assert_eq!(pixels_x(1.0, 20.0), -pixels(1.0, 20.0), "the axes disagree");
/// assert_eq!(pixels_x(f32::NAN, 20.0), 0.0);
/// ```
#[must_use]
pub fn pixels_x(dx: f32, col_w: f32) -> f32 {
    if !dx.is_finite() || !col_w.is_finite() {
        return 0.0;
    }
    let out = dx * ROWS_PER_NOTCH * col_w;
    if out.is_finite() { out } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::{Accumulator, ROWS_PER_NOTCH, pixels, pixels_x, rows_f};

    /// The ordinary case: a detent is a full three-row step, both ways.
    #[test]
    fn one_notch_is_three_rows() {
        let mut acc = Accumulator::default();
        assert_eq!(
            acc.rows(-1.0),
            3,
            "away from the user scrolls down the list"
        );
        assert_eq!(acc.rows(1.0), -3, "towards the user scrolls back up");
    }

    /// The bug this module exists to prevent: a device that sends fractions
    /// must still scroll. Rounding each event alone would return 0 forever.
    #[test]
    fn a_trackpads_fractions_add_up_instead_of_vanishing() {
        let mut acc = Accumulator::default();
        let mut total = 0isize;
        for _ in 0..10 {
            total += acc.rows(-0.1);
        }
        assert_eq!(
            total, 3,
            "ten tenths of a notch is one notch, which is three rows"
        );
    }

    /// No event may be silently dropped: over a long stream the delivered rows
    /// must track the notches turned, never drifting away from them.
    #[test]
    fn the_delivered_rows_track_the_notches_turned() {
        let mut acc = Accumulator::default();
        let mut delivered = 0isize;
        let events = 500;
        let per_event = -0.37_f32;
        for _ in 0..events {
            delivered += acc.rows(per_event);
        }
        let expected = -per_event * ROWS_PER_NOTCH * events as f32;
        let drift = (delivered as f32 - expected).abs();
        assert!(
            drift <= 1.0,
            "delivered {delivered} rows for {expected} turned; drift {drift} must stay under one row"
        );
    }

    /// Reversing direction must not need the residue paid back first: half a
    /// notch each way is a round trip, not a dead zone.
    #[test]
    fn reversing_direction_cancels_rather_than_accumulating() {
        let mut acc = Accumulator::default();
        let down = acc.rows(-0.5);
        let up = acc.rows(0.5);
        assert_eq!(down + up, 0, "half a notch each way returns to the start");
    }

    /// A fraction earned in one view must not deliver a row in the next.
    #[test]
    fn reset_forgets_the_outstanding_fraction() {
        let mut acc = Accumulator::default();
        assert_eq!(acc.rows(-0.2), 0, "not yet a whole row");
        acc.reset();
        assert_eq!(acc.rows(-0.2), 0, "the earlier fraction is gone");
    }

    /// Input events come from outside the process. A NaN that reached the
    /// residue would stop the view scrolling for the life of the app.
    #[test]
    fn a_nonfinite_event_cannot_poison_later_ones() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let mut acc = Accumulator::default();
            assert_eq!(acc.rows(bad), 0, "{bad} must not move the view");
            assert_eq!(
                acc.rows(-1.0),
                3,
                "and must leave the next ordinary notch working"
            );
        }
    }

    /// Zero is not a scroll. A wheel that reports no movement must not move
    /// the view, and must not disturb a pending fraction either.
    #[test]
    fn a_zero_delta_moves_nothing_and_keeps_the_fraction() {
        let mut acc = Accumulator::default();
        // 0.2 notches is 0.6 rows — a real fraction of a row, unlike 0.5
        // notches, which is already 1.5 rows and delivers one immediately.
        assert_eq!(acc.rows(-0.2), 0);
        assert_eq!(acc.rows(0.0), 0);
        assert_eq!(acc.rows(-0.2), 1, "the pending 0.6 of a row survived");
    }

    /// `rows_at` is `rows` with the constant spelled out.
    #[test]
    fn rows_at_with_the_default_step_matches_rows() {
        let mut a = Accumulator::default();
        let mut b = Accumulator::default();
        for dy in [-0.3, -0.3, 1.4, -2.0] {
            assert_eq!(a.rows(dy), b.rows_at(dy, ROWS_PER_NOTCH));
        }
    }

    /// A view with its own step size gets it honoured.
    #[test]
    fn rows_at_honours_a_custom_step() {
        let mut acc = Accumulator::default();
        assert_eq!(acc.rows_at(-1.0, 1.0), 1, "one row per notch");
        let mut acc = Accumulator::default();
        assert_eq!(acc.rows_at(-1.0, 10.0), 10);
    }

    /// The pixel path moves the same distance the row path would.
    // Exact comparison is the point: both sides are 3 * 18.0 = 54.0, which is
    // exactly representable, and an epsilon here would hide the off-by-a-factor
    // mistakes this test exists to catch.
    #[allow(clippy::float_cmp)]
    #[test]
    fn pixels_and_rows_agree_about_how_far_a_notch_goes() {
        let row_h = 18.0;
        let mut acc = Accumulator::default();
        let rows = acc.rows(-1.0);
        assert_eq!(pixels(-1.0, row_h), rows as f32 * row_h);
    }

    /// Same non-finite guard as the accumulator, for the same reason.
    // Exact comparison against a literal zero, which `pixels` returns by an
    // early `return 0.0` rather than by arithmetic — there is no rounding to
    // tolerate, and "near zero" would pass even if the guard were removed.
    #[allow(clippy::float_cmp)]
    #[test]
    fn pixels_rejects_nonfinite_input() {
        assert_eq!(pixels(f32::NAN, 20.0), 0.0);
        assert_eq!(pixels(1.0, f32::NAN), 0.0);
        assert_eq!(pixels(f32::MAX, f32::MAX), 0.0);
    }

    /// The point of the continuous converter, and the one thing an accumulator
    /// cannot do: a fifth of a notch moves a fifth of a notch *now*, rather
    /// than four events later.
    #[allow(clippy::float_cmp)]
    #[test]
    fn a_fraction_of_a_notch_moves_a_fraction_of_a_row() {
        assert_eq!(rows_f(-0.2), 0.6);
        let mut acc = Accumulator::default();
        assert_eq!(acc.rows(-0.2), 0, "the integer offset has to wait");
    }

    /// The two continuous converters must agree, which they do by construction
    /// — `pixels` is written in terms of `rows_f`. This pins that it stays so.
    // Exact: 3 * 0.25 * 16.0 = 12.0, representable to the bit.
    #[allow(clippy::float_cmp)]
    #[test]
    fn pixels_is_rows_f_scaled_by_the_row_height() {
        for dy in [-1.0, 1.0, -0.25, 2.5, 0.0] {
            assert_eq!(pixels(dy, 16.0), rows_f(dy) * 16.0, "dy = {dy}");
        }
    }

    /// `rows_f` and the accumulator must travel the same distance over a whole
    /// number of notches; they differ in *when* the movement lands, not in how
    /// far it goes.
    #[allow(clippy::float_cmp)]
    #[test]
    fn rows_f_and_the_accumulator_agree_over_whole_notches() {
        let mut acc = Accumulator::default();
        let mut whole = 0isize;
        let mut continuous = 0.0_f32;
        for dy in [-1.0, -2.0, 1.0, -3.0] {
            whole += acc.rows(dy);
            continuous += rows_f(dy);
        }
        assert_eq!(continuous, whole as f32);
    }

    /// Same non-finite guard as the rest of the module. The overflow case is
    /// the one worth naming: `dy` is finite but `dy * 3.0` is not.
    #[allow(clippy::float_cmp)]
    #[test]
    fn rows_f_rejects_nonfinite_input_and_overflow() {
        assert_eq!(rows_f(f32::NAN), 0.0);
        assert_eq!(rows_f(f32::INFINITY), 0.0);
        assert_eq!(rows_f(f32::NEG_INFINITY), 0.0);
        assert_eq!(rows_f(f32::MAX), 0.0, "finite input, infinite product");
    }

    /// The whole reason `pixels_x` exists. `WM_MOUSEWHEEL` is positive away
    /// from the user (view goes *up*, offset shrinks); `WM_MOUSEHWHEEL` is
    /// positive to the right (view goes *right*, offset grows). Both arrive
    /// through the same `wheel_delta`, so the sign has to be undone on exactly
    /// one axis — and this asserts it is undone on exactly one.
    // Exact: both sides are 3 * 25.0 = 75.0, representable to the bit. An
    // epsilon would let a sign error through if it were also a rounding error,
    // which is precisely the bug being pinned.
    #[allow(clippy::float_cmp)]
    #[test]
    fn the_two_axes_take_a_positive_delta_in_opposite_directions() {
        assert_eq!(pixels(1.0, 25.0), -75.0, "wheel away: towards the start");
        assert_eq!(pixels_x(1.0, 25.0), 75.0, "tilt right: towards the end");
        assert_eq!(pixels_x(1.0, 25.0), -pixels(1.0, 25.0));
        assert_eq!(pixels_x(-1.0, 25.0), -pixels(-1.0, 25.0));
    }

    /// A horizontal notch crosses as many columns as a vertical one crosses
    /// rows — the two axes differ in sign, not in step.
    #[allow(clippy::float_cmp)]
    #[test]
    fn a_horizontal_notch_is_the_same_distance_as_a_vertical_one() {
        let unit = 100.0;
        assert_eq!(pixels_x(1.0, unit).abs(), pixels(1.0, unit).abs());
        assert_eq!(pixels_x(1.0, unit).abs(), ROWS_PER_NOTCH * unit);
    }

    /// Same non-finite guard as [`pixels`], for the same reason: a `NaN` that
    /// reached a stored offset would freeze the view for good.
    #[allow(clippy::float_cmp)]
    #[test]
    fn pixels_x_rejects_nonfinite_input() {
        assert_eq!(pixels_x(f32::NAN, 20.0), 0.0);
        assert_eq!(pixels_x(1.0, f32::NAN), 0.0);
        assert_eq!(pixels_x(f32::INFINITY, 20.0), 0.0);
        assert_eq!(pixels_x(f32::MAX, f32::MAX), 0.0);
    }
}
