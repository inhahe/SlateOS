//! A field's well: the sunken box a text field, a drop-down or an address bar
//! is drawn in -- its fill, its edge, and the mark that says it has the
//! keyboard -- drawn once, in the theme's shape.
//!
//! # Why one drawer
//!
//! Every field in the toolkit drew its own box, and they had drifted: the
//! drop-down's was the palette's `crust` with a `surface1` edge and a ring
//! round it when focused; the input dialog's was `surface0` with a 1.5-pixel
//! edge that turned *blue* -- not the accent -- when focused, and hid its red
//! "this is wrong" edge whenever the field had the keyboard, which is exactly
//! when someone is fixing it; the address bar's had no focus mark at all. A
//! theme's widget style (`Palette::widget_style`, design-decisions 1435) is a
//! promise about every field, so every field has to be drawn by the one
//! function that keeps it: [`draw`].
//!
//! # What the style decides
//!
//! The corners (`field.radius`); whether the edge goes all round the well or
//! only along its bottom (`field.border`); and how the keyboard is shown
//! (`field.focus`) -- a ring outside the field, the built-in glow (the edge in
//! the accent with a soft halo of it, after the reference's
//! `.aero-srch-in:focus`), or a bar of the accent along the bottom.
//!
//! # What it does not
//!
//! The colours are the palette's: the well is `crust`, the edge `surface1`,
//! warmed towards the accent under the pointer; the focus mark is the accent
//! and at least the user's focus width. A field whose content is wrong has a
//! red edge, whatever else is true of it -- a style may choose how focus
//! looks, not whether an error shows -- and while it has the keyboard its
//! focus mark is red too: one signal, rather than a red edge inside a ring of
//! the accent, which reads as two controls or as a field that is both fine and
//! wrong.

use crate::color::Color;
use crate::disabled::DISABLED_OPACITY;
use crate::frame::Rect;
use crate::palette::Palette;
use crate::render::RenderCommand;
use crate::style::CornerRadii;
use crate::surface::CommandSink;
use crate::theme::with_alpha;
use crate::widget_style::{FieldBorder, FieldStyle, FocusMark};

/// How far the edge is warmed towards the accent under the pointer.
pub const HOVER_TINT: f32 = 0.5;

/// How opaque the glow's halo is, of 255: the reference's `rgba(120, 185,
/// 235, 0.35)`, a shade firmer (0.45), since the halo is half of what says
/// "this has the keyboard" and the reference's is only ever on white.
const GLOW_ALPHA: u8 = 115;

/// The thinnest an underline focus bar is drawn, whatever width the caller
/// passes: a bar is a narrower mark than a ring, so it gets a floor.
const MIN_UNDERLINE: f32 = 2.0;

/// What is happening to a field now, which its owner knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The pointer is over it -- or, for a drop-down, its list is open.
    pub hovered: bool,
    /// It has the keyboard.
    pub focused: bool,
    /// It cannot be used now: drawn at the toolkit's disabled opacity, with
    /// no focus mark.
    pub disabled: bool,
    /// What is in it is wrong -- a path that does not exist, a name that is
    /// not allowed: the edge is red.
    pub invalid: bool,
}

/// The two colours of a field's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    /// The well: what the text is written on.
    pub well: Color,
    /// The edge.
    pub edge: Color,
}

/// The colours of a field in `state`.
///
/// The edge is red for a field whose content is wrong, whatever else is
/// true; the accent for a focused field under the glow; warmed towards the
/// accent under the pointer; `surface1` otherwise.
#[must_use]
pub fn paint(p: &Palette, state: State) -> Paint {
    let live = !state.disabled;
    let edge = if state.invalid {
        p.red
    } else if live && state.focused && p.widget_style.field.focus == FocusMark::Glow {
        p.accent
    } else if live && state.hovered {
        p.surface1.lerp(p.accent, HOVER_TINT)
    } else {
        p.surface1
    };
    let fade = |c: Color| if state.disabled { faded(c) } else { c };
    Paint {
        well: fade(p.crust),
        edge: fade(edge),
    }
}

/// The corner radius a field `h` tall is drawn with in `style`: the style's,
/// never more than half the height.
#[must_use]
pub fn radius(style: &FieldStyle, h: f32) -> f32 {
    f32::from(style.radius).min((h / 2.0).max(0.0))
}

/// The well's corners at `scale`: all round for an edge all round, the top
/// pair only for an edge along the bottom -- whose line runs straight to both
/// ends.
fn well_corners(style: &FieldStyle, h: f32, scale: f32) -> CornerRadii {
    let r = (f32::from(style.radius) * scale).min((h / 2.0).max(0.0));
    match style.border {
        FieldBorder::Box => CornerRadii::all(r),
        FieldBorder::Underline => CornerRadii::top(r),
    }
}

/// `corners` grown by `by`, for a mark drawn `by` outside them: a rounded
/// corner grows, a square one stays square.
fn grown(corners: CornerRadii, by: f32) -> CornerRadii {
    let grow = |r: f32| if r > 0.0 { r + by } else { 0.0 };
    CornerRadii {
        top_left: grow(corners.top_left),
        top_right: grow(corners.top_right),
        bottom_right: grow(corners.bottom_right),
        bottom_left: grow(corners.bottom_left),
    }
}

/// Draw a field's box at `rect`: the well, its edge and -- when it has the
/// keyboard -- the focus mark, in the palette's colours and its widget
/// style's shape. The caller draws what is in the field afterwards.
///
/// `focus_ring` is the user's focus width (`AppearanceSettings::
/// focus_ring_width`); a caller that has none passes
/// [`crate::style::FOCUS_RING_WIDTH`].
pub fn draw(sink: &mut impl CommandSink, p: &Palette, rect: Rect, state: State, focus_ring: f32) {
    draw_at_scale(sink, p, rect, state, focus_ring, 1.0);
}

/// [`draw`], for a caller that draws in physical pixels at the user's display
/// scaling -- the desktop shell's chrome: the corners and the edge are the
/// style's lengths times `scale`. `rect` and `focus_ring` are already
/// physical. A `scale` that is not a positive number is read as one.
pub fn draw_at_scale(
    sink: &mut impl CommandSink,
    p: &Palette,
    rect: Rect,
    state: State,
    focus_ring: f32,
    scale: f32,
) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let style = &p.widget_style.field;
    let colours = paint(p, state);
    let corners = well_corners(style, rect.h, scale);
    let line = scale;
    sink.emit(RenderCommand::FillRect {
        x: rect.x,
        y: rect.y,
        width: rect.w,
        height: rect.h,
        color: colours.well,
        corner_radii: corners,
    });
    match style.border {
        FieldBorder::Box => sink.emit(RenderCommand::StrokeRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: colours.edge,
            line_width: line,
            corner_radii: corners,
        }),
        FieldBorder::Underline => sink.emit(RenderCommand::FillRect {
            x: rect.x,
            y: rect.bottom() - line,
            width: rect.w,
            height: line,
            color: colours.edge,
            corner_radii: CornerRadii::ZERO,
        }),
    }
    if !state.focused || state.disabled || !(focus_ring > 0.0 && focus_ring.is_finite()) {
        return;
    }
    // The mark says where the typing goes; on a wrong field it says what is
    // wrong with it as well.
    let mark = if state.invalid { p.red } else { p.accent };
    match style.focus {
        FocusMark::Ring => sink.emit(RenderCommand::StrokeRect {
            // Outside the field, so a thicker ring is still a ring round it
            // rather than a band across what is typed in it.
            x: rect.x - focus_ring,
            y: rect.y - focus_ring,
            width: rect.w + focus_ring * 2.0,
            height: rect.h + focus_ring * 2.0,
            color: mark,
            line_width: focus_ring,
            corner_radii: grown(corners, focus_ring),
        }),
        FocusMark::Glow => {
            // The edge is already the mark's colour (`paint`); the halo
            // round it.
            let halo = with_alpha(mark, GLOW_ALPHA);
            sink.emit(RenderCommand::StrokeRect {
                x: rect.x - focus_ring,
                y: rect.y - focus_ring,
                width: rect.w + focus_ring * 2.0,
                height: rect.h + focus_ring * 2.0,
                color: halo,
                line_width: focus_ring,
                corner_radii: grown(corners, focus_ring),
            });
        }
        FocusMark::Underline => {
            // Along the bottom, over the edge. Inside the corners of a box,
            // so it does not stick out past a rounded one.
            let thick = focus_ring.max(MIN_UNDERLINE * scale);
            let inset = match style.border {
                FieldBorder::Box => corners.bottom_left,
                FieldBorder::Underline => 0.0,
            };
            sink.emit(RenderCommand::FillRect {
                x: rect.x + inset,
                y: rect.bottom() - thick,
                width: (rect.w - inset * 2.0).max(0.0),
                height: thick,
                color: mark,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }
}

/// `c` at the toolkit's disabled opacity.
fn faded(c: Color) -> Color {
    let a = (f32::from(c.a) * DISABLED_OPACITY).round();
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "rounded, and between 0 and 255 because both factors are"
    )]
    let a = a as u8;
    Color::rgba(c.r, c.g, c.b, a)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::panic,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::float_cmp
    )]

    use super::*;
    use crate::widget_style::WidgetStyle;

    const RECT: Rect = Rect {
        x: 10.0,
        y: 20.0,
        w: 200.0,
        h: 26.0,
    };

    fn palette(field: FieldStyle) -> Palette {
        let mut p = Palette::for_mode(false);
        p.widget_style.field = field;
        p
    }

    fn drawn(p: &Palette, state: State) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();
        draw(&mut cmds, p, RECT, state, 2.0);
        cmds
    }

    const FOCUSED: State = State {
        hovered: false,
        focused: true,
        disabled: false,
        invalid: false,
    };

    /// **The built-in field is the reference's**: a `crust` well with 3-pixel
    /// corners and a line all round, and, with the keyboard, the edge in the
    /// accent with a halo of it outside.
    #[test]
    fn the_built_in_field_is_the_references() {
        let p = Palette::for_mode(false);
        assert_eq!(p.widget_style.field, WidgetStyle::AERO.field);
        let rest = drawn(&p, State::default());
        assert!(matches!(
            rest.as_slice(),
            [
                RenderCommand::FillRect { color, corner_radii, .. },
                RenderCommand::StrokeRect { color: edge, line_width, .. },
            ] if *color == p.crust && corner_radii.top_left == 3.0
                && *edge == p.surface1 && *line_width == 1.0
        ));
        let focused = drawn(&p, FOCUSED);
        assert!(matches!(
            focused.as_slice(),
            [
                RenderCommand::FillRect { .. },
                RenderCommand::StrokeRect { color: edge, .. },
                RenderCommand::StrokeRect { color: halo, x, line_width, .. },
            ] if *edge == p.accent && halo.r == p.accent.r && halo.a < 255
                && *x == RECT.x - 2.0 && *line_width == 2.0
        ));
    }

    /// **A ring is the accent, outside the field, as wide as asked** -- and
    /// the edge is not the accent then: the ring says it.
    #[test]
    fn a_ring_is_outside_the_field() {
        let p = palette(FieldStyle {
            focus: FocusMark::Ring,
            ..WidgetStyle::AERO.field
        });
        let cmds = drawn(&p, FOCUSED);
        let ring = cmds
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::StrokeRect {
                    x,
                    color,
                    line_width,
                    corner_radii,
                    ..
                } if *color == p.accent => Some((*x, *line_width, corner_radii.top_left)),
                _ => None,
            })
            .expect("no ring");
        assert_eq!(ring, (RECT.x - 2.0, 2.0, 5.0));
        assert_eq!(paint(&p, FOCUSED).edge, p.surface1);
    }

    /// **An underline is a bar of the accent along the bottom**, never
    /// thinner than two pixels, inside a box's rounded corners.
    #[test]
    fn an_underline_is_a_bar_along_the_bottom() {
        let p = palette(FieldStyle {
            focus: FocusMark::Underline,
            ..WidgetStyle::AERO.field
        });
        let mut cmds = Vec::new();
        draw(&mut cmds, &p, RECT, FOCUSED, 1.0);
        let bar = cmds
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } if *color == p.accent => Some((*x, *y, *width, *height)),
                _ => None,
            })
            .expect("no bar");
        assert_eq!(bar, (RECT.x + 3.0, RECT.bottom() - 2.0, RECT.w - 6.0, 2.0));
    }

    /// **An edge along the bottom only** squares the well's lower corners and
    /// draws a line the full width under it -- and nothing round the sides.
    #[test]
    fn an_underline_edge_is_a_line_under_a_square_bottomed_well() {
        let p = palette(FieldStyle {
            border: FieldBorder::Underline,
            ..WidgetStyle::AERO.field
        });
        let cmds = drawn(&p, State::default());
        assert!(matches!(
            cmds.as_slice(),
            [
                RenderCommand::FillRect { corner_radii: well, .. },
                RenderCommand::FillRect { y, width, height, color, .. },
            ] if well.top_left == 3.0 && well.bottom_left == 0.0
                && *y == RECT.bottom() - 1.0 && *width == RECT.w && *height == 1.0
                && *color == p.surface1
        ));
    }

    /// **A wrong field's edge is red whatever else is true** -- focused under
    /// the glow, hovered, anything. The input dialog's used to turn blue when
    /// focused and hide its error just when someone was fixing it.
    #[test]
    fn a_wrong_fields_edge_is_red_whatever_else_is_true() {
        let p = Palette::for_mode(false);
        for hovered in [false, true] {
            for focused in [false, true] {
                let state = State {
                    hovered,
                    focused,
                    disabled: false,
                    invalid: true,
                };
                assert_eq!(paint(&p, state).edge, p.red, "{state:?}");
            }
        }
    }

    /// **A wrong field with the keyboard says so with its focus mark too**:
    /// red, in each of the three marks -- not a red edge inside a ring of the
    /// accent, which reads as a field both fine and wrong.
    #[test]
    fn a_wrong_fields_focus_mark_is_red() {
        for focus in [FocusMark::Ring, FocusMark::Glow, FocusMark::Underline] {
            let p = palette(FieldStyle {
                focus,
                ..WidgetStyle::AERO.field
            });
            let wrong = State {
                invalid: true,
                ..FOCUSED
            };
            let cmds = drawn(&p, wrong);
            let hue = |c: &Color| (c.r, c.g, c.b);
            let marks: Vec<Color> = cmds
                .iter()
                .skip(2)
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { color, .. }
                    | RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect();
            assert!(!marks.is_empty(), "{focus:?}: no focus mark");
            assert!(
                marks.iter().all(|c| hue(c) == hue(&p.red)),
                "{focus:?}: {marks:?}"
            );
        }
    }

    /// **The pointer warms the edge; a disabled field shows nothing moving**:
    /// no warmth, no focus mark, and its box at the disabled opacity.
    #[test]
    fn hover_warms_and_disabled_quiets() {
        let p = Palette::for_mode(false);
        let hovered = State {
            hovered: true,
            ..State::default()
        };
        assert_eq!(
            paint(&p, hovered).edge,
            p.surface1.lerp(p.accent, HOVER_TINT)
        );
        let off = State {
            hovered: true,
            focused: true,
            disabled: true,
            invalid: false,
        };
        let c = paint(&p, off);
        assert!(c.well.a < 255 && c.edge.a < 255);
        assert_eq!(Color::rgb(c.edge.r, c.edge.g, c.edge.b), p.surface1);
        assert_eq!(drawn(&p, off).len(), 2, "a disabled field has a focus mark");
    }

    /// **No width, no mark** -- a caller that scales its lines to nothing, or
    /// hands in a NaN, gets no zero-width ring.
    #[test]
    fn a_focus_width_of_nothing_draws_no_mark() {
        let p = Palette::for_mode(false);
        for width in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let mut cmds = Vec::new();
            draw(&mut cmds, &p, RECT, FOCUSED, width);
            assert_eq!(cmds.len(), 2, "width {width}");
        }
    }

    /// **At a display scaling, the corners and the edge are scaled** -- and a
    /// nonsense scale is one.
    #[test]
    fn a_scaled_field_scales_its_corners_and_edge() {
        let p = Palette::for_mode(false);
        let big = Rect::new(0.0, 0.0, 400.0, 52.0);
        let mut cmds = Vec::new();
        draw_at_scale(&mut cmds, &p, big, State::default(), 4.0, 2.0);
        assert!(matches!(
            cmds.as_slice(),
            [
                RenderCommand::FillRect { corner_radii, .. },
                RenderCommand::StrokeRect { line_width, .. },
            ] if corner_radii.top_left == 6.0 && *line_width == 2.0
        ));
        for bad in [0.0, -2.0, f32::NAN] {
            let mut odd = Vec::new();
            draw_at_scale(&mut odd, &p, RECT, State::default(), 2.0, bad);
            let mut plain = Vec::new();
            draw(&mut plain, &p, RECT, State::default(), 2.0);
            assert_eq!(odd, plain, "scale {bad}");
        }
    }

    /// **The corners are the style's, never rounder than a pill.**
    #[test]
    fn the_corners_are_the_styles_up_to_a_pill() {
        let style = FieldStyle {
            radius: FieldStyle::MAX_RADIUS,
            ..WidgetStyle::AERO.field
        };
        assert_eq!(radius(&style, 26.0), 13.0);
        assert_eq!(radius(&style, 20.0), 10.0);
        assert_eq!(radius(&style, -4.0), 0.0);
        assert_eq!(radius(&WidgetStyle::AERO.field, 26.0), 3.0);
    }
}
