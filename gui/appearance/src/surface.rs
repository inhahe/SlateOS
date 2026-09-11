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

impl<T> CommandSink for guitk::frame::Frame<T> {
    fn emit(&mut self, cmd: guitk::render::RenderCommand) {
        // Through `push`, never into the buffer behind it: this one maintains
        // the clip stack as it goes.
        self.push(cmd);
    }
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
}

impl SurfacePaint {
    /// Neither filled nor outlined — it simply sits on the page.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            fill: None,
            border: None,
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
                fill: None,
                border: Some(self.border),
            },
            (SurfaceStyle::Borders, Surface::Selected) => SurfacePaint {
                fill: None,
                border: Some(self.accent),
            },
            // A panel already has a shadow and an edge; on the page, outlined.
            (SurfaceStyle::Borders, Surface::Panel) => SurfacePaint {
                fill: Some(self.base),
                border: Some(self.border),
            },

            // Cards: the shipped arrangement, kept as an optional theme.
            (SurfaceStyle::Cards, Surface::Card) => SurfacePaint {
                fill: Some(self.surface0),
                border: None,
            },
            (SurfaceStyle::Cards, Surface::Selected) => SurfacePaint {
                fill: Some(self.surface1),
                border: None,
            },
            (SurfaceStyle::Cards, Surface::Panel) => SurfacePaint {
                fill: Some(self.mantle),
                border: Some(self.surface1),
            },
            (SurfaceStyle::Cards, Surface::Sidebar) => SurfacePaint {
                fill: Some(self.crust),
                border: None,
            },

            // Identical in both themes. Written as one arm rather than two so
            // that it cannot drift apart later.
            (_, Surface::ControlTrack) => SurfacePaint {
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
        let paint = self.surface_paint(what);
        let radii = CornerRadii::all(radius);
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

    /// Borders is what a machine with no configuration gets.
    #[test]
    fn borders_is_the_default() {
        assert_eq!(SurfaceStyle::default(), SurfaceStyle::Borders);
    }
}
