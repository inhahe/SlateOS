//! Edge reservations: the strip along a screen edge that a panel keeps for
//! itself, and that tiling must therefore leave alone.
//!
//! A taskbar, a dock or a menu bar is an ordinary window sitting against one
//! edge of a monitor. Nothing about being a window stops another window being
//! tiled *underneath* it — and that is what the compositor did before this
//! module existed: a window snapped to the left half filled exactly half the
//! monitor including the strip the taskbar occupied, so its bottom rows, along
//! with whatever they held, were covered up.
//!
//! The fix is for the panel to *say* how much it needs. This module holds the
//! rules for that claim; the wire form is
//! [`RequestBody::ReserveEdge`](crate::control::RequestBody::ReserveEdge) and
//! the compositor is what enforces it.
//!
//! # Why this shape and not `_NET_WM_STRUT_PARTIAL`
//!
//! X11's strut hint encodes, for each of the four edges, both a thickness *and*
//! a start and end offset along that edge — twelve numbers — so that two panels
//! can share one edge side by side without each claiming the whole of it. It is
//! also the part of that protocol that implementations get wrong most reliably,
//! because the partial form interacts with multi-monitor layouts in ways the
//! specification never pinned down: an offset is measured along the whole
//! virtual desktop, not along the monitor, so a panel on the second monitor
//! must compute offsets that describe a span it does not occupy.
//!
//! Wayland's `zwlr_layer_shell` exclusive zone is one number — a thickness —
//! belonging to a surface that is already attached to a specific output. That
//! is the model here: a reservation is a thickness plus an edge, and *which
//! monitor* is answered by the window making the claim rather than by
//! arithmetic on the reservation. Two panels sharing one edge each reserve
//! their own thickness and the thicknesses add, which loses the ability to tile
//! a window *between* two half-width panels — a layout no shell here has, and
//! one that can be added later by giving the reservation a span, which is a
//! strictly larger protocol than this one rather than an incompatible one.

use crate::zones::WorkArea;

/// The edge of a monitor a panel can anchor to.
///
/// Four sides and no corners, unlike
/// [`ScreenEdge`](crate::zones::ScreenEdge), which has eight variants because
/// it answers a different question: *there* a corner is a distinct place a
/// pointer can be, whereas a strip of reserved pixels running along "the
/// top-left corner" is not a shape — it is either a horizontal band or a
/// vertical one, and a panel has to say which.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PanelEdge {
    /// A vertical strip down the left of the monitor.
    Left,
    /// A vertical strip down the right of the monitor.
    Right,
    /// A horizontal band across the top of the monitor.
    Top,
    /// A horizontal band across the bottom of the monitor — where a taskbar
    /// lives by default.
    Bottom,
}

impl PanelEdge {
    /// The wire byte for this edge.
    #[must_use]
    pub const fn as_byte(self) -> u8 {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Top => 2,
            Self::Bottom => 3,
        }
    }

    /// The edge a wire byte names, or `None` if it names nothing.
    #[must_use]
    pub const fn from_byte(b: u8) -> Option<Self> {
        Some(match b {
            0 => Self::Left,
            1 => Self::Right,
            2 => Self::Top,
            3 => Self::Bottom,
            _ => return None,
        })
    }

    /// Whether this edge eats into the monitor's width rather than its height.
    #[must_use]
    pub const fn is_horizontal_axis(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// The largest share of a monitor's extent that reservations along **one** edge
/// may take.
///
/// A third. The bound exists because a reservation is a client asking to shrink
/// everybody else's tiling, and a client that asked for the whole monitor would
/// otherwise leave a work area of zero — every tiled window collapsing to
/// nothing, with no visible cause and no window left to fix it from. Refusing
/// outright at some threshold would be the other option; clamping is chosen
/// because the failure it guards against is a panel that miscalculated its own
/// height (a font that measured larger than expected, a scale factor applied
/// twice), and a taskbar that is a third of the screen tall is recoverable
/// while a desktop with no work area is not.
///
/// A third **per edge**, so two opposing panels can take two thirds between
/// them and a third always survives. That composition is the reason the bound
/// is stated per edge rather than as a bound on the total: one rule, applied
/// four times, with the total following from it.
pub const MAX_RESERVED_FRACTION: f32 = 1.0 / 3.0;

/// The reserved strips along the four edges of one monitor.
///
/// Thicknesses in pixels, all zero for a monitor with no panels on it. Several
/// panels on the same edge add together — see the module docs for why sharing
/// an edge is addition here rather than the side-by-side spans X11 uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReservedEdges {
    /// Pixels reserved down the left.
    pub left: u32,
    /// Pixels reserved down the right.
    pub right: u32,
    /// Pixels reserved across the top.
    pub top: u32,
    /// Pixels reserved across the bottom.
    pub bottom: u32,
}

impl ReservedEdges {
    /// Nothing reserved anywhere.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            left: 0,
            right: 0,
            top: 0,
            bottom: 0,
        }
    }

    /// Add one panel's claim.
    ///
    /// Saturating, so a set of clients whose claims sum past `u32::MAX` gets a
    /// clamped work area rather than a wrapped one — which would read as *no*
    /// reservation and put the tiled window back under the panel, the exact
    /// bug this module exists to prevent, reachable by arithmetic.
    pub const fn add(&mut self, edge: PanelEdge, size: u32) {
        let slot = match edge {
            PanelEdge::Left => &mut self.left,
            PanelEdge::Right => &mut self.right,
            PanelEdge::Top => &mut self.top,
            PanelEdge::Bottom => &mut self.bottom,
        };
        *slot = slot.saturating_add(size);
    }

    /// Whether anything at all is reserved.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.left == 0 && self.right == 0 && self.top == 0 && self.bottom == 0
    }

    /// The work area left of `screen` once these strips are taken out of it.
    ///
    /// Each edge is clamped to [`MAX_RESERVED_FRACTION`] of the extent it eats
    /// into, so the result is never empty for a non-empty screen and never has
    /// a negative extent. A screen that is already empty stays empty rather
    /// than acquiring one: there is nothing to reserve out of nothing.
    #[must_use]
    pub fn apply(self, screen: WorkArea) -> WorkArea {
        let (dx0, dx1) = clamp_pair(self.left, self.right, screen.width);
        let (dy0, dy1) = clamp_pair(self.top, self.bottom, screen.height);
        WorkArea::new(
            screen.x + dx0,
            screen.y + dy0,
            (screen.width - dx0 - dx1).max(0.0),
            (screen.height - dy0 - dy1).max(0.0),
        )
    }
}

/// Clamp the two reservations along one axis to a third of `extent` each.
///
/// Kept as a free function taking the two opposing edges together rather than
/// clamping each edge on its own, because the clamp is against the *extent of
/// the axis* and doing it per edge in `apply` would repeat the same three lines
/// with the width and the height swapped — the shape in which a transposition
/// typo survives review.
#[expect(
    clippy::cast_precision_loss,
    reason = "reservations are screen-sized; f32 is exact well past any monitor"
)]
fn clamp_pair(near: u32, far: u32, extent: f32) -> (f32, f32) {
    if extent <= 0.0 {
        return (0.0, 0.0);
    }
    let cap = extent * MAX_RESERVED_FRACTION;
    ((near as f32).min(cap), (far as f32).min(cap))
}

#[cfg(test)]
mod tests {
    // Exact comparison is what these tests are for: a reservation is a whole
    // number of pixels subtracted from a screen dimension, so every expected
    // value here is exactly representable and an epsilon would only hide an
    // arithmetic slip. The one place a third of a screen is compared, it is
    // compared with a tolerance explicitly.
    #![allow(clippy::float_cmp)]

    use super::{MAX_RESERVED_FRACTION, PanelEdge, ReservedEdges};
    use crate::zones::WorkArea;

    const SCREEN: WorkArea = WorkArea::new(0.0, 0.0, 1920.0, 1080.0);

    #[test]
    fn reserving_nothing_leaves_the_screen_exactly_as_it_was() {
        assert!(ReservedEdges::none().is_empty());
        assert_eq!(
            ReservedEdges::none().apply(SCREEN),
            SCREEN,
            "an empty reservation set moved or resized the screen"
        );
    }

    #[test]
    fn a_taskbar_at_the_bottom_shortens_the_area_without_moving_its_top() {
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Bottom, 40);
        let area = r.apply(SCREEN);
        assert_eq!(
            (area.x, area.y),
            (SCREEN.x, SCREEN.y),
            "a bottom reservation moved the top-left corner"
        );
        assert_eq!(area.width, SCREEN.width, "a bottom reservation lost width");
        assert_eq!(
            area.height,
            SCREEN.height - 40.0,
            "the bottom strip was not the height that was asked for"
        );
        assert_eq!(
            area.bottom(),
            SCREEN.bottom() - 40.0,
            "the work area still reaches under the taskbar"
        );
    }

    #[test]
    fn a_panel_at_the_top_or_the_left_moves_the_origin_and_the_far_edge_stays() {
        for (edge, expect_origin) in [
            (PanelEdge::Top, (0.0, 50.0)),
            (PanelEdge::Left, (50.0, 0.0)),
        ] {
            let mut r = ReservedEdges::none();
            r.add(edge, 50);
            let area = r.apply(SCREEN);
            assert_eq!(
                (area.x, area.y),
                expect_origin,
                "{edge:?} did not push the origin out of its strip"
            );
            assert_eq!(
                (area.right(), area.bottom()),
                (SCREEN.right(), SCREEN.bottom()),
                "{edge:?} moved the far edge, which it does not touch"
            );
        }
    }

    #[test]
    fn a_panel_on_each_edge_takes_from_each_edge_independently() {
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Left, 10);
        r.add(PanelEdge::Right, 20);
        r.add(PanelEdge::Top, 30);
        r.add(PanelEdge::Bottom, 40);
        let area = r.apply(SCREEN);
        assert_eq!(
            (area.x, area.y, area.width, area.height),
            (10.0, 30.0, SCREEN.width - 30.0, SCREEN.height - 70.0),
            "four reservations did not each come out of their own edge"
        );
    }

    #[test]
    fn two_panels_on_one_edge_add_up() {
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Bottom, 40);
        r.add(PanelEdge::Bottom, 24);
        assert_eq!(r.bottom, 64, "a second bottom panel replaced the first");
        assert_eq!(
            r.apply(SCREEN).height,
            SCREEN.height - 64.0,
            "the two bottom panels did not both come out of the screen"
        );
    }

    #[test]
    fn a_greedy_panel_is_clamped_to_a_third_rather_than_erasing_the_desktop() {
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Bottom, 100_000);
        let area = r.apply(SCREEN);
        let cap = SCREEN.height * MAX_RESERVED_FRACTION;
        assert_eq!(
            area.height,
            SCREEN.height - cap,
            "a client asking for the whole screen was not clamped to a third"
        );
        assert!(
            area.height > 0.0,
            "the clamp still left no work area at all"
        );
    }

    #[test]
    fn two_greedy_panels_facing_each_other_still_leave_a_third() {
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Top, u32::MAX);
        r.add(PanelEdge::Bottom, u32::MAX);
        let area = r.apply(SCREEN);
        assert!(
            (area.height - SCREEN.height * MAX_RESERVED_FRACTION).abs() < 0.01,
            "two opposing clamped panels left {} of {}, not a third",
            area.height,
            SCREEN.height
        );
        assert!(
            area.y > 0.0 && area.bottom() < SCREEN.bottom(),
            "the surviving third is not between the two panels"
        );
    }

    #[test]
    fn a_sum_that_overflows_clamps_rather_than_wrapping_back_to_nothing() {
        // Two claims that would wrap a u32 add: without the saturating add the
        // total reads as a small number and the taskbar's strip comes back.
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Bottom, u32::MAX);
        r.add(PanelEdge::Bottom, 100);
        assert_eq!(r.bottom, u32::MAX, "the reservation sum wrapped");
        assert!(
            r.apply(SCREEN).height < SCREEN.height,
            "a wrapped sum gave the screen back"
        );
    }

    #[test]
    fn a_reservation_out_of_an_empty_screen_stays_empty_and_does_not_go_negative() {
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Bottom, 40);
        r.add(PanelEdge::Left, 40);
        let area = r.apply(WorkArea::new(7.0, 9.0, 0.0, 0.0));
        assert_eq!(
            (area.x, area.y, area.width, area.height),
            (7.0, 9.0, 0.0, 0.0),
            "reserving out of an empty screen produced {area:?}"
        );

        // And an extent that is not merely empty but *negative*, which is what
        // the guard in `clamp_pair` is actually there for: a third of a
        // negative extent is a negative cap, and `min` against it returns the
        // cap — a negative inset, which does not shrink the area but widens
        // it, off the monitor, in the direction the panel is on. `WorkArea` is
        // a public type with a public constructor and does not refuse one, so
        // this is reachable from outside this crate even though the compositor
        // itself derives every area from a `Rect` with unsigned extents.
        let backwards = r.apply(WorkArea::new(7.0, 9.0, -100.0, -100.0));
        assert_eq!(
            (backwards.x, backwards.y),
            (7.0, 9.0),
            "a backwards screen moved the origin: {backwards:?}"
        );
        assert!(
            backwards.width <= 0.0 && backwards.height <= 0.0,
            "a backwards screen was widened rather than left alone: {backwards:?}"
        );
    }

    #[test]
    fn a_reservation_off_the_origin_stays_on_its_own_monitor() {
        // The second monitor of a two-monitor desktop: its work area must not
        // acquire the first monitor's origin.
        let screen = WorkArea::new(1920.0, 0.0, 1024.0, 768.0);
        let mut r = ReservedEdges::none();
        r.add(PanelEdge::Bottom, 40);
        let area = r.apply(screen);
        assert_eq!(area.x, 1920.0, "the work area slid onto the other monitor");
        assert_eq!(area.width, 1024.0, "the work area changed monitor width");
        assert_eq!(area.height, 728.0, "the bottom strip was not subtracted");
    }

    #[test]
    fn every_edge_survives_the_round_trip_through_its_wire_byte() {
        for edge in [
            PanelEdge::Left,
            PanelEdge::Right,
            PanelEdge::Top,
            PanelEdge::Bottom,
        ] {
            assert_eq!(
                PanelEdge::from_byte(edge.as_byte()),
                Some(edge),
                "{edge:?} did not survive its own wire byte"
            );
        }
        assert_eq!(
            PanelEdge::from_byte(4),
            None,
            "a byte naming no edge decoded to one anyway"
        );
    }

    #[test]
    fn the_two_axes_are_told_apart() {
        assert!(PanelEdge::Left.is_horizontal_axis());
        assert!(PanelEdge::Right.is_horizontal_axis());
        assert!(!PanelEdge::Top.is_horizontal_axis());
        assert!(!PanelEdge::Bottom.is_horizontal_axis());
    }
}
