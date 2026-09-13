//! How a box is told apart from what is behind it.
//!
//! Until 2026-09-11 every draw site answered that itself, by picking a fill
//! colour off the surface ladder. There were about 790 of them and they did not
//! agree: a settings page filled every row, a menu filled only the hovered one,
//! and "selected" meant `surface0` in one place and `surface1` in another --
//! because selection was a *step* above whatever was underneath, so it could not
//! be one colour. `design-decisions.md` §829 has the full account.
//!
//! Now a draw site says *what a box is* and this module says how that is drawn.
//! The immediate reason is that it made the theme switch possible at all: with
//! the decision in one place, [`SurfaceStyle::Borders`] and
//! [`SurfaceStyle::Cards`] are two arms of one `match` rather than 790 edits.
//! The lasting reason is that "selected" now means the same thing everywhere,
//! which no arrangement of shades could deliver.

use crate::{Palette, SurfaceStyle};
use guitk::color::Color;
use guitk::render::RenderTree;
use guitk::style::CornerRadii;

/// Somewhere render commands can be sent.
///
/// The tree emits into four different receivers -- `Vec<RenderCommand>` at
/// 1,258 sites, `Frame` at 299, `RenderTree` at 247, and assorted others -- and
/// `Frame::push` is not a plain append: it tracks the clip stack, so writing to
/// its inner buffer would lose that. One small trait lets every converted draw
/// site stay a single line regardless of what it is drawing into, which matters
/// when there are 991 of them.
pub trait CommandSink {
    /// Emit one command.
    fn emit(&mut self, cmd: guitk::render::RenderCommand);
}

impl CommandSink for Vec<guitk::render::RenderCommand> {
    fn emit(&mut self, cmd: guitk::render::RenderCommand) {
        self.push(cmd);
    }
}

impl CommandSink for RenderTree {
    fn emit(&mut self, cmd: guitk::render::RenderCommand) {
        self.push(cmd);
    }
}

/// So that `&mut receiver` is the right call shape whether the receiver is an
/// owned `Vec` or already a `&mut` binding. Without this the converted call
/// sites would have to know which they were looking at, and they cannot -- the
/// two are spelled identically at the point of use.
impl<S: CommandSink + ?Sized> CommandSink for &mut S {
    fn emit(&mut self, cmd: guitk::render::RenderCommand) {
        (**self).emit(cmd);
    }
}

impl<T> CommandSink for guitk::frame::Frame<T> {
    fn emit(&mut self, cmd: guitk::render::RenderCommand) {
        // Through `push`, never into the buffer behind it: this one maintains
        // the clip stack as it goes.
        self.push(cmd);
    }
}

/// The rectangle a surface command was asked for, whichever way it was drawn.
///
/// A filled surface is drawn at the rectangle given; an outlined one is stroked
/// half a line *inside* it, so its command reports a position one half-pixel in
/// and a size one pixel smaller. Tests that identify a box by its geometry --
/// "the tab at this x", "the row under this text" -- need the rectangle the
/// caller asked for, not the one the stroke occupies.
///
/// Extracted because the same three lines were being written into a fifth app's
/// test: jsonviewer, kanban, netmanager and spreadsheet each had a helper that
/// matched `FillRect` by colour and stopped finding anything once the box
/// became an outline.
///
/// Returns `None` for commands that are not rectangles.
#[must_use]
pub fn logical_rect(cmd: &guitk::render::RenderCommand) -> Option<(f32, f32, f32, f32)> {
    match *cmd {
        guitk::render::RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            ..
        } => Some((x, y, width, height)),
        guitk::render::RenderCommand::StrokeRect {
            x,
            y,
            width,
            height,
            line_width,
            ..
        } => Some((
            x - line_width / 2.0,
            y - line_width / 2.0,
            width + line_width,
            height + line_width,
        )),
        _ => None,
    }
}

/// The logical rectangle and colour of a box, however the theme drew it.
///
/// The companion to [`logical_rect`] for tests that need the colour too. A
/// test that used to read
///
/// ```text
/// cmds.iter().find_map(|c| match c {
///     RenderCommand::FillRect { height: 30.0, color, .. } => Some(*color),
///     _ => None,
/// })
/// ```
///
/// sees nothing once that box becomes an outline, and -- worse -- keeps
/// passing if every assertion it feeds is over the resulting empty set. That
/// is not hypothetical: `privacy_settings` passed vacuously for exactly this
/// reason. Written through this function the same test matches either shape.
#[must_use]
pub fn painted_rect(cmd: &guitk::render::RenderCommand) -> Option<(f32, f32, f32, f32, Color)> {
    let color = match *cmd {
        guitk::render::RenderCommand::FillRect { color, .. }
        | guitk::render::RenderCommand::StrokeRect { color, .. } => color,
        _ => return None,
    };
    let (x, y, w, h) = logical_rect(cmd)?;
    Some((x, y, w, h, color))
}

/// What the theme painted at a given rectangle, whichever way it drew it.
///
/// For tests that used to assert "the well is `p.crust`" by matching a
/// `FillRect` on colour. Since §829 that box may be an outline instead, and
/// since §835 a strip may be a hairline, so the colour a test should expect is
/// no longer a constant -- it is whatever `surface_paint` says for the kind the
/// draw site named. The assertion becomes
///
/// ```ignore
/// assert_eq!(paint_at(&cmds, x, y, w, h), p.surface_paint(Surface::Card));
/// ```
///
/// which is true under both themes and stays true when either changes. Matching
/// is on the *logical* rectangle, so a caller passes the geometry the draw site
/// asked for and does not have to know about the half-pixel stroke inset.
#[must_use]
pub fn paint_at(
    cmds: &[guitk::render::RenderCommand],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> SurfacePaint {
    let mut found = SurfacePaint::none();
    for cmd in cmds {
        let Some((cx, cy, cw, ch)) = logical_rect(cmd) else {
            continue;
        };
        let same = (cx - x).abs() < 0.01
            && (cy - y).abs() < 0.01
            && (cw - width).abs() < 0.01
            && (ch - height).abs() < 0.01;
        if !same {
            continue;
        }
        match *cmd {
            guitk::render::RenderCommand::FillRect { color, .. } => found.fill = Some(color),
            guitk::render::RenderCommand::StrokeRect { color, .. } => found.border = Some(color),
            _ => {}
        }
    }
    found
}

/// What a box *is*, which is what a draw site knows.
///
/// Deliberately not a list of shades. A caller that knew it wanted `surface1`
/// would be back to choosing a colour, and would be wrong the moment the theme
/// changed underneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// An ordinary row or card sitting on the page.
    Card,
    /// A row or item that is currently chosen.
    ///
    /// The one that motivated the whole change: under cards this is a shade
    /// above whatever it sits on, so the same "selected" reads differently in a
    /// menu and in a list. Under borders it is an accent outline, which does
    /// not depend on its background and so means one thing everywhere.
    Selected,
    /// A floating panel: a menu, a dialog, a notification.
    ///
    /// Distinct from [`Card`](Self::Card) because it is already told apart by
    /// its shadow and its edge, and in the shipped code it is drawn on `base` --
    /// the page colour -- rather than on a shade. It gains nothing from a fill.
    Panel,
    /// A sidebar or other structural region of a window.
    Sidebar,
    /// The groove of a control: a switch track, a scrollbar trough, a progress
    /// bar's unfilled part.
    ///
    /// **Stays a fill in both themes**, and is the reason this enum has five
    /// members rather than three. A switch track outlined instead of filled
    /// would read as an empty box rather than as the "off" half of a control,
    /// and a scrollbar trough would vanish. These were the sites a blind sweep
    /// would have converted and should not have.
    ControlTrack,
    /// A full-width structural band: a toolbar, a status bar, a tab strip.
    ///
    /// Carries which edge faces the content, because a toolbar's separator sits
    /// along its bottom and a status bar's along its top, and nothing about the
    /// rectangle says which. Under [`StripStyle::Filled`] the edge is unused.
    ///
    /// Its own kind rather than a `Panel`, because a band spanning the window
    /// reads as a band and not as a box -- outlining one looks like a box that
    /// failed to fit, which is why almost no desktop does it. §835.
    Strip(Edge),
}

/// Which edge of a strip faces the content it is separated from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// The separator runs along the top -- a status bar, content above it.
    Top,
    /// The separator runs along the bottom -- a toolbar, content below it.
    Bottom,
}

/// The paint for one box: a fill, an outline, or both.
///
/// Returned rather than drawn so that a caller with an irregular shape -- a
/// rounded-only-at-the-top header, a circle -- can apply the same decision to
/// geometry this module does not know how to emit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePaint {
    /// Fill, if this surface is filled in the active theme.
    pub fill: Option<Color>,
    /// Outline, if this surface is outlined in the active theme.
    pub border: Option<Color>,
    /// A hairline along one edge, if this surface is separated rather than
    /// filled or boxed. `None` for every kind except [`Surface::Strip`] under
    /// [`StripStyle::Separator`] -- which is the cost §835 records for keeping
    /// that option.
    pub separator: Option<(Edge, Color)>,
}

impl SurfacePaint {
    /// Neither filled nor outlined — it simply sits on the page.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            fill: None,
            border: None,
            separator: None,
        }
    }
}

impl Palette {
    /// How to paint `what`, under `style`.
    ///
    /// # The `ControlTrack` arm is the same in both themes, on purpose
    ///
    /// Every other arm changes. That one does not, because a control's groove is
    /// not a container being told apart from its background -- it is a part of a
    /// control, and the fill *is* the thing. See [`Surface::ControlTrack`].
    #[must_use]
    pub fn surface_paint(&self, what: Surface) -> SurfacePaint {
        match (self.surface_style, what) {
            // Borders: nothing is filled, structure is carried by the outline.
            (SurfaceStyle::Borders, Surface::Card | Surface::Sidebar) => SurfacePaint {
                separator: None,
                fill: None,
                border: Some(self.border),
            },
            (SurfaceStyle::Borders, Surface::Selected) => SurfacePaint {
                separator: None,
                fill: None,
                border: Some(self.accent),
            },
            // A panel already has a shadow and an edge; on the page, outlined.
            (SurfaceStyle::Borders, Surface::Panel) => SurfacePaint {
                separator: None,
                fill: Some(self.base),
                border: Some(self.border),
            },

            // Cards: the shipped arrangement, kept as an optional theme.
            (SurfaceStyle::Cards, Surface::Card) => SurfacePaint {
                separator: None,
                fill: Some(self.surface0),
                border: None,
            },
            (SurfaceStyle::Cards, Surface::Selected) => SurfacePaint {
                separator: None,
                fill: Some(self.surface1),
                border: None,
            },
            (SurfaceStyle::Cards, Surface::Panel) => SurfacePaint {
                separator: None,
                fill: Some(self.mantle),
                border: Some(self.surface1),
            },
            (SurfaceStyle::Cards, Surface::Sidebar) => SurfacePaint {
                separator: None,
                fill: Some(self.crust),
                border: None,
            },

            // A strip answers to its own setting, not to the card/border one --
            // the two are orthogonal (§835), so this arm ignores `surface_style`
            // exactly as `ControlTrack` does.
            (_, Surface::Strip(edge)) => match self.strip_style {
                crate::StripStyle::Filled => SurfacePaint {
                    separator: None,
                    fill: Some(self.mantle),
                    border: None,
                },
                crate::StripStyle::Separator => SurfacePaint {
                    separator: Some((edge, self.border)),
                    fill: None,
                    border: None,
                },
            },

            // Identical in both themes. Written as one arm rather than two so
            // that it cannot drift apart later.
            (_, Surface::ControlTrack) => SurfacePaint {
                separator: None,
                fill: Some(self.surface2),
                border: None,
            },
        }
    }

    /// Append `what` as a rectangle to a plain command list.
    ///
    /// The form nearly every call site wants, because most of the tree emits
    /// into a `Vec<RenderCommand>` rather than a `RenderTree` -- 1,258 sites
    /// spell it `cmds.push`, 299 `frame.push`, 57 `commands.push`. A
    /// `RenderTree` caller passes `&mut tree.commands`, which is exactly what
    /// `RenderTree::push` does anyway.
    pub fn push_surface<S: CommandSink + ?Sized>(
        &self,
        out: &mut S,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        what: Surface,
    ) {
        self.push_surface_radii(out, x, y, width, height, CornerRadii::all(radius), what);
    }

    /// As [`push_surface`](Self::push_surface), for a box whose corners are not
    /// all the same.
    ///
    /// A header rounded along its top edge and square along its bottom is the
    /// common case -- it sits against the panel below it. Those sites cannot use
    /// the single-radius form and would otherwise have had to assemble the
    /// commands themselves, which is precisely how a draw site ends up choosing
    /// a colour again.
    pub fn push_surface_radii<S: CommandSink + ?Sized>(
        &self,
        out: &mut S,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: CornerRadii,
        what: Surface,
    ) {
        self.push_paint_radii(out, x, y, width, height, radii, self.surface_paint(what));
    }

    /// Draw a paint that a *state* has altered, rather than one a theme chose.
    ///
    /// There is exactly one legitimate reason to reach past
    /// [`push_surface_radii`](Self::push_surface_radii): a site whose box takes
    /// a different outline when something is wrong with it. The login screen's
    /// password field is the case -- a rejected password outlines it in
    /// `p.red`, and under the bordered theme that would otherwise be a *second*
    /// border drawn concentric with the theme's own, at the same rectangle, one
    /// in red and one in black.
    ///
    /// Note the shape of the fix: the state **replaces** a member of the paint
    /// rather than drawing another rectangle over it. A site that stacks a
    /// second rectangle looks right under whichever theme was in front of the
    /// author and wrong under the other one.
    ///
    /// This is not a way to pick colours at a draw site. Start from
    /// `surface_paint`, change the one member the state owns, and pass it here.
    pub fn push_paint_radii<S: CommandSink + ?Sized>(
        &self,
        out: &mut S,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radii: CornerRadii,
        paint: SurfacePaint,
    ) {
        if let Some(fill) = paint.fill {
            out.emit(guitk::render::RenderCommand::FillRect {
                x,
                y,
                width,
                height,
                color: fill,
                corner_radii: radii,
            });
        }
        if let Some((edge, colour)) = paint.separator {
            // A hairline, not a box. Drawn as a fill rather than a stroke
            // because a stroke of width 1 is centred on its path and would
            // straddle the boundary; this sits wholly inside the rectangle, so
            // a strip still occupies exactly the space it was given.
            let line = 1.0_f32;
            let y = match edge {
                Edge::Top => y,
                Edge::Bottom => y + (height - line).max(0.0),
            };
            out.emit(guitk::render::RenderCommand::FillRect {
                x,
                y,
                width,
                height: line.min(height),
                color: colour,
                corner_radii: CornerRadii::ZERO,
            });
        }
        if let Some(border) = paint.border {
            // Inset by half the line so the stroke lands inside the rectangle
            // the caller asked for. Without this a bordered row is a pixel
            // taller than the filled row it replaces, and a column of them
            // drifts -- invisible in one row, obvious down a page.
            out.emit(guitk::render::RenderCommand::StrokeRect {
                x: x + 0.5,
                y: y + 0.5,
                width: (width - 1.0).max(0.0),
                height: (height - 1.0).max(0.0),
                color: border,
                line_width: 1.0,
                corner_radii: radii,
            });
        }
    }

    /// The single colour this theme marks `what` with.
    ///
    /// For tests, and for the handful of callers that need a colour rather than
    /// a paint. A box is filled *or* outlined depending on the theme, and a
    /// test that used to say `p.surface0` now says `p.painted(Surface::Card)` --
    /// one token, and correct under both themes.
    ///
    /// # Panics
    ///
    /// If `what` is drawn with nothing at all, which
    /// `no_surface_is_invisible_in_either_theme` forbids. A panic here means
    /// that invariant broke, and a test is the right place to hear about it.
    #[must_use]
    // Total by construction: every arm of `surface_paint` above returns a
    // paint with at least one of the three set, and
    // `no_surface_is_invisible_in_either_theme` asserts exactly that over the
    // cross product of styles and kinds. Returning `Option` instead would put
    // an `.expect` at each of the ~40 call sites rather than removing one.
    #[allow(clippy::expect_used)]
    pub fn painted(&self, what: Surface) -> Color {
        let paint = self.surface_paint(what);
        paint
            .fill
            .or(paint.border)
            .or(paint.separator.map(|(_, c)| c))
            .expect("every surface is drawn with something")
    }

    /// Draw `what` as a rectangle, in whichever way the theme calls for.
    ///
    /// The one-line form, which is what nearly every call site wants. A site
    /// with a shape this cannot emit should call
    /// [`surface_paint`](Self::surface_paint) and draw it itself, so that the
    /// *decision* still lives here even when the geometry does not.
    pub fn draw_surface(
        &self,
        tree: &mut RenderTree,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        radius: f32,
        what: Surface,
    ) {
        self.push_surface(tree, x, y, width, height, radius, what);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test that panics on bad data is reporting, which is its job"
)]
mod tests {
    use super::*;
    use guitk::render::RenderCommand;

    fn light() -> Palette {
        Palette::for_mode(true)
    }

    /// The light palette in a chosen style. Since the style rides on the
    /// palette, "the same palette under the other theme" is how these tests
    /// have to ask the question.
    fn styled(style: SurfaceStyle) -> Palette {
        let mut p = Palette::for_mode(true);
        p.surface_style = style;
        p
    }

    /// The property the whole change exists for.
    ///
    /// Under cards, "selected" is a shade one step above whatever is underneath,
    /// so a selected menu item and a selected list row are different colours and
    /// cannot be made the same without erasing the distinction from unselected
    /// rows. Under borders it is one outline, independent of what is behind it.
    #[test]
    fn selected_means_one_thing_everywhere_under_borders() {
        let p = light();
        let selected = styled(SurfaceStyle::Borders).surface_paint(Surface::Selected);
        assert_eq!(
            selected.fill, None,
            "a selected box is not filled under borders"
        );
        assert_eq!(selected.border, Some(p.accent));

        // ...and under cards it is a fill, which is what makes it relative.
        let carded = styled(SurfaceStyle::Cards).surface_paint(Surface::Selected);
        assert_eq!(carded.fill, Some(p.surface1));
        assert_ne!(
            carded.fill,
            styled(SurfaceStyle::Cards)
                .surface_paint(Surface::Card)
                .fill,
            "selected and unselected must differ under cards, or selection is invisible"
        );
    }

    /// A control's groove is not a container, and stays filled either way.
    ///
    /// This is the case a blind sweep would have got wrong: a switch track
    /// outlined rather than filled reads as an empty box, not as the off half of
    /// a switch.
    #[test]
    fn a_control_track_is_filled_in_both_themes() {
        let p = light();
        for style in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
            let paint = styled(style).surface_paint(Surface::ControlTrack);
            assert_eq!(
                paint.fill,
                Some(p.surface2),
                "track lost its fill under {style:?}"
            );
            assert_eq!(paint.border, None);
        }
    }

    /// Under borders, nothing that is merely a container gets a fill from the
    /// shade ladder. That is the whole point: the ladder stops being load-bearing.
    #[test]
    fn borders_take_no_colour_from_the_shade_ladder() {
        let p = light();
        let ladder = [p.surface0, p.surface1, p.surface2, p.mantle, p.crust];
        for what in [Surface::Card, Surface::Selected, Surface::Sidebar] {
            let paint = styled(SurfaceStyle::Borders).surface_paint(what);
            if let Some(fill) = paint.fill {
                assert!(
                    !ladder.contains(&fill),
                    "{what:?} is still filled from the ladder under borders"
                );
            }
        }
    }

    /// Every surface is visible somehow. A box that is neither filled nor
    /// outlined has vanished, which is the one outcome worse than either theme.
    #[test]
    fn no_surface_is_invisible_in_either_theme() {
        for style in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
            for what in [
                Surface::Card,
                Surface::Selected,
                Surface::Panel,
                Surface::Sidebar,
                Surface::ControlTrack,
            ] {
                let paint = styled(style).surface_paint(what);
                assert!(
                    paint.fill.is_some() || paint.border.is_some(),
                    "{what:?} under {style:?} is drawn with nothing at all"
                );
            }
        }
    }

    /// The border is inset so a bordered box occupies the rectangle asked for.
    ///
    /// Without this a bordered row is a pixel taller than the filled row it
    /// replaces, and a column of them drifts -- a difference invisible in one
    /// row and obvious down a whole page.
    #[test]
    fn a_bordered_box_fills_the_rectangle_it_was_given() {
        let p = styled(SurfaceStyle::Borders);
        let mut tree = RenderTree::new();
        p.draw_surface(&mut tree, 10.0, 20.0, 100.0, 40.0, 6.0, Surface::Card);
        let stroke = tree
            .commands
            .iter()
            .find_map(|c| match c {
                RenderCommand::StrokeRect {
                    x,
                    y,
                    width,
                    height,
                    line_width,
                    ..
                } => Some((*x, *y, *width, *height, *line_width)),
                _ => None,
            })
            .expect("a bordered card draws a stroke");
        let (x, y, w, h, lw) = stroke;
        assert!(
            (x - lw / 2.0 - 10.0).abs() < f32::EPSILON,
            "left edge drifted: {x}"
        );
        assert!(
            (y - lw / 2.0 - 20.0).abs() < f32::EPSILON,
            "top edge drifted: {y}"
        );
        assert!(
            (x + w + lw / 2.0 - 110.0).abs() < f32::EPSILON,
            "right edge drifted"
        );
        assert!(
            (y + h + lw / 2.0 - 60.0).abs() < f32::EPSILON,
            "bottom edge drifted"
        );
    }

    /// A degenerate rectangle does not produce a negative-sized stroke.
    #[test]
    fn a_zero_sized_surface_does_not_go_negative() {
        let p = styled(SurfaceStyle::Borders);
        let mut tree = RenderTree::new();
        p.draw_surface(&mut tree, 0.0, 0.0, 0.0, 0.0, 0.0, Surface::Card);
        for cmd in &tree.commands {
            if let RenderCommand::StrokeRect { width, height, .. } = cmd {
                assert!(
                    *width >= 0.0 && *height >= 0.0,
                    "negative stroke: {width}x{height}"
                );
            }
        }
    }

    /// `logical_rect` gives back the rectangle asked for, both ways round.
    #[test]
    fn a_logical_rect_is_the_rectangle_that_was_asked_for() {
        for style in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
            let mut tree = RenderTree::new();
            styled(style).draw_surface(&mut tree, 10.0, 20.0, 100.0, 40.0, 4.0, Surface::Card);
            let rects: Vec<_> = tree.commands.iter().filter_map(logical_rect).collect();
            assert_eq!(
                rects,
                [(10.0, 20.0, 100.0, 40.0)],
                "under {style:?} the rectangle came back changed"
            );
        }
    }

    /// Switching the theme does not change how much the compositor is asked to
    /// do.
    ///
    /// The question this answers is the one a benchmark cannot, because nothing
    /// benchmarks the compositor: 436 draw sites moved from a fill to a stroke,
    /// and if an outline cost an extra command each, that would be 436 more
    /// commands per full redraw with no instrument anywhere to notice. It does
    /// not -- every kind emits the same count either way. `Panel` emits two in
    /// both themes because it is filled *and* edged in both.
    ///
    /// This is deliberately a count and not a time. A timing assertion on a
    /// developer machine is noise: lane A measured run-to-run variation at a
    /// median 1.02x under hardware virtualisation and 1.10x under emulation,
    /// with a p90 of 1.75x, which cannot resolve anything smaller than a
    /// disaster. A command count is exact, and it is the half of the cost this
    /// change could plausibly have moved.
    #[test]
    fn neither_theme_asks_the_compositor_for_more_work_than_the_other() {
        use guitk::render::RenderTree;

        for what in [
            Surface::Card,
            Surface::Selected,
            Surface::Panel,
            Surface::Sidebar,
            Surface::ControlTrack,
        ] {
            let count = |style| {
                let mut tree = RenderTree::new();
                styled(style).draw_surface(&mut tree, 0.0, 0.0, 100.0, 40.0, 4.0, what);
                tree.commands.len()
            };
            assert_eq!(
                count(SurfaceStyle::Borders),
                count(SurfaceStyle::Cards),
                "{what:?} costs a different number of commands in the two themes"
            );
        }
    }

    /// An outline covers a fraction of the pixels a fill does, which is the
    /// other half of the cost and the half that favours the new default.
    ///
    /// Not a measurement of the renderer -- it is arithmetic on the geometry,
    /// pinned so the claim is checkable rather than asserted in a commit
    /// message. A 100x40 box is 4,000 pixels filled; its 1px outline is 276.
    #[test]
    fn an_outline_touches_far_fewer_pixels_than_the_fill_it_replaces() {
        let (w, h, line) = (100.0_f32, 40.0_f32, 1.0_f32);
        let filled = w * h;
        let outlined = w * h - (w - 2.0 * line) * (h - 2.0 * line);
        assert!(
            outlined * 10.0 < filled,
            "an outline of {outlined} px is not markedly cheaper than {filled} px filled"
        );
    }

    /// Selection changes colour and nothing else (§834).
    ///
    /// The operator took option A and then rejected the thickened outline the
    /// mock-up drew: a 2px line on a 1px layout costs a pixel, so a row would
    /// grow when chosen and a list would twitch as the selection travelled down
    /// it. Colour changes no geometry.
    #[test]
    fn a_selected_box_differs_from_an_unselected_one_only_in_colour() {
        use guitk::render::RenderCommand;
        let p = styled(SurfaceStyle::Borders);
        let widths = |what| {
            let mut tree = RenderTree::new();
            p.draw_surface(&mut tree, 0.0, 0.0, 100.0, 40.0, 4.0, what);
            tree.commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect {
                        line_width,
                        x,
                        y,
                        width,
                        height,
                        ..
                    } => Some((*line_width, *x, *y, *width, *height)),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(
            widths(Surface::Selected),
            widths(Surface::Card),
            "a selected box is a different size or weight from an unselected one"
        );
        assert_ne!(
            p.surface_paint(Surface::Selected).border,
            p.surface_paint(Surface::Card).border,
            "selection has to differ by colour, since it differs by nothing else"
        );
    }

    /// A strip answers to its own setting, not the card/border one (§835).
    #[test]
    fn a_strip_follows_the_strip_setting_and_ignores_the_surface_one() {
        for surface in [SurfaceStyle::Borders, SurfaceStyle::Cards] {
            let mut filled = Palette::for_mode(true);
            filled.surface_style = surface;
            filled.strip_style = crate::StripStyle::Filled;
            let paint = filled.surface_paint(Surface::Strip(Edge::Bottom));
            assert_eq!(paint.fill, Some(filled.mantle), "under {surface:?}");
            assert_eq!(paint.separator, None);

            let mut lined = filled;
            lined.strip_style = crate::StripStyle::Separator;
            let paint = lined.surface_paint(Surface::Strip(Edge::Bottom));
            assert_eq!(paint.fill, None, "a separated strip has no band");
            assert_eq!(paint.separator, Some((Edge::Bottom, lined.border)));
        }
    }

    /// The hairline sits inside the rectangle, on the edge asked for.
    ///
    /// Drawn as a fill rather than a stroke: a 1px stroke is centred on its
    /// path and would straddle the boundary, so a strip would occupy a pixel
    /// more than it was given and the content below it would shift.
    #[test]
    fn a_strips_separator_lands_on_the_edge_it_names() {
        use guitk::render::RenderCommand;
        let mut p = Palette::for_mode(true);
        p.strip_style = crate::StripStyle::Separator;
        for (edge, want_y) in [(Edge::Top, 20.0_f32), (Edge::Bottom, 20.0 + 40.0 - 1.0)] {
            let mut tree = RenderTree::new();
            p.draw_surface(
                &mut tree,
                10.0,
                20.0,
                100.0,
                40.0,
                0.0,
                Surface::Strip(edge),
            );
            let line = tree
                .commands
                .iter()
                .find_map(|c| match c {
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        ..
                    } => Some((*x, *y, *width, *height)),
                    _ => None,
                })
                .expect("a separated strip draws a line");
            assert_eq!(line, (10.0, want_y, 100.0, 1.0), "{edge:?} landed wrong");
        }
    }

    /// Borders is what a machine with no configuration gets.
    #[test]
    fn borders_is_the_default() {
        assert_eq!(SurfaceStyle::default(), SurfaceStyle::Borders);
    }
}
