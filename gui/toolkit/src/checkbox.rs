//! A checkbox: a small box with a mark in it, and a label beside it that is
//! part of the control.
//!
//! # Why the toolkit has one now
//!
//! The retained widget tree declares `WidgetKind::Checkbox`, and nothing uses
//! that tree (`known-issues.md`,
//! `TD-C-THE-RETAINED-WIDGET-TREE-HAS-NO-USER-AND-FIVE-OF-ITS-WIDGETS-DRAW-NOTHING`),
//! so a program that wanted a checkbox drew its own. This module is the one a
//! program can draw instead, in the default theme's look and the user's
//! colours, like [`crate::button`], [`crate::slider`] and [`crate::switch`].
//!
//! # Two kinds of three states
//!
//! `design.txt` asks for "tristate checkboxes -- good for yes/no/default,
//! 'default' is useful for cascading option overrides". There are two
//! different controls behind that word, and they move differently when
//! clicked, so the caller says which it has ([`Mode`]):
//!
//! - **[`Mode::TwoState`]**: the ordinary box, and the *summary* box -- a
//!   parent whose children disagree shows [`CheckState::Indeterminate`], a
//!   state the user cannot choose, only cause. A click sets it or clears it
//!   ([`CheckState::toggled`]), never into "partly".
//! - **[`Mode::ThreeState`]**: the *override* box, where the third state is
//!   "no opinion -- use the default" and the user chooses it like the other
//!   two. A click steps Unchecked, Indeterminate, Checked and round again
//!   ([`cycled`]) -- so from "default" the first click says *yes*, which is
//!   what a click on a checkbox means everywhere else. That is Qt's order; the
//!   Win32 one goes from "default" to *no* first.
//!
//! # How it looks
//!
//! The Aero reference (`Aero Desktop (offline).html`, `aero-srch-check`) sets
//! a check's label at 12 pixels, six pixels from the box, and draws its box in
//! a mid blue. Here the box is an input's well -- the palette's `crust` with
//! the `surface1` edge every text field in the toolkit has -- so a checkbox
//! and a field beside it are one family; the mark is the accent, held legible
//! on the well ([`legible_on`]); the edge takes a tint of the accent under the
//! pointer, as the reference's controls do on hover. A box is a small target,
//! so it is taken hold of by its [`crate::grab`] region and by its label.
//!
//! The box's corners are the theme's (`Palette::widget_style`'s `check`,
//! design-decisions 1435): 2 pixels in the built-in theme, up to a circle.
//! [`draw_box`] draws the box alone, for a control that is a check box in
//! another's room -- an on/off switch under a theme that draws switches as
//! boxes (`crate::switch`).

use crate::color::Color;
use crate::disabled::DISABLED_OPACITY;
use crate::event::{Key, KeyEvent};
use crate::frame::Rect;
use crate::grab;
use crate::palette::{Palette, legible_on};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::style::CornerRadii;
use crate::surface::CommandSink;

pub use crate::widget::CheckState;

/// The box's side.
pub const SIZE: f32 = 14.0;
/// The label's size: the reference's 12 pixels.
pub const FONT_SIZE: f32 = 12.0;
/// From the box to its label: the reference's `gap: 6px`.
pub const LABEL_GAP: f32 = 6.0;
/// A checkbox's height where the caller has no layout of its own: tall
/// enough for the label's line.
pub const HEIGHT: f32 = 20.0;
/// How far the edge is tinted towards the accent under the pointer.
const HOVER_TINT: f32 = 0.5;
/// The mark's stroke.
const MARK_WIDTH: f32 = 2.0;
/// The side of the square an indeterminate box shows.
const PARTLY: f32 = 6.0;
/// The space between the box and the keyboard ring round it.
const FOCUS_GAP: f32 = 1.0;

/// How a click moves a checkbox. See the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Set or cleared; "partly" is shown, never chosen.
    #[default]
    TwoState,
    /// Unchecked, partly (the default), checked, and round again.
    ThreeState,
}

/// Where a click takes a box in `mode`.
#[must_use]
pub const fn next(check: CheckState, mode: Mode) -> CheckState {
    match mode {
        Mode::TwoState => check.toggled(),
        Mode::ThreeState => cycled(check),
    }
}

/// The three-state order: Unchecked, Indeterminate, Checked, Unchecked.
#[must_use]
pub const fn cycled(check: CheckState) -> CheckState {
    match check {
        CheckState::Unchecked => CheckState::Indeterminate,
        CheckState::Indeterminate => CheckState::Checked,
        CheckState::Checked => CheckState::Unchecked,
    }
}

/// What is happening to a checkbox now, which the host knows and the box
/// does not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The pointer is over it.
    pub hovered: bool,
    /// The keyboard is on it: a ring round the box in the accent.
    pub focused: bool,
    /// It cannot be changed now: drawn at the toolkit's disabled opacity.
    pub disabled: bool,
}

/// The width a checkbox with `label` needs: the box, the gap and the label.
#[must_use]
pub fn width(label: &str) -> f32 {
    if label.is_empty() {
        return SIZE;
    }
    SIZE + LABEL_GAP + crate::text::measure(label, FONT_SIZE, FontWeightHint::Regular)
}

/// Where the box is for a checkbox whose row starts at `(x, y)` and is `h`
/// tall: at the left, centred down the row.
#[must_use]
pub fn box_rect(x: f32, y: f32, h: f32) -> Rect {
    Rect::new(x, y + (h - SIZE) / 2.0, SIZE, SIZE)
}

/// The region a press takes hold of the checkbox in: the box, grown as
/// [`grab::handle`] grows a handle, and the label beside it -- a click on the
/// words "Show hidden files" ticks the box, as it does everywhere.
#[must_use]
pub fn hit(x: f32, y: f32, h: f32, label: &str) -> Rect {
    let region = grab::handle(box_rect(x, y, h));
    let right = (x + width(label)).max(region.right());
    let top = region.y.min(y);
    let bottom = region.bottom().max(y + h);
    Rect::new(region.x, top, right - region.x, bottom - top)
}

/// Whether `key` flips a checkbox that has the keyboard: Space, pressed, with
/// no Ctrl, Alt or Super. (Not Enter: in a dialog Enter presses the default
/// button, and a checkbox that took it would stop the dialog closing.)
#[must_use]
pub fn toggles(key: &KeyEvent) -> bool {
    let m = key.modifiers;
    key.pressed && !(m.ctrl || m.alt || m.super_key) && key.key == Key::Space
}

/// The colours a box is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    /// The inside of the box: an input's well.
    pub well: Color,
    /// The box's edge.
    pub edge: Color,
    /// The mark -- the tick, or the square of a box that is partly set.
    pub mark: Color,
    /// The label.
    pub label: Color,
}

/// The colours of a checkbox in `state`.
#[must_use]
pub fn paint(p: &Palette, state: State) -> Paint {
    let edge = if state.hovered && !state.disabled {
        p.surface1.lerp(p.accent, HOVER_TINT)
    } else {
        p.surface1
    };
    Paint {
        well: p.crust,
        edge,
        mark: legible_on(p.accent, p.crust),
        label: p.text,
    }
}

/// The corner radius a box `side` pixels across is drawn with in the theme:
/// the widget style's, never more than half the side -- a circle.
#[must_use]
pub fn radius(p: &Palette, side: f32) -> f32 {
    f32::from(p.widget_style.check.radius).min((side / 2.0).max(0.0))
}

/// Draw a checkbox whose row starts at `(x, y)` and is `h` tall, with `label`
/// beside the box.
///
/// `focus_ring` is the ring's width when the box has the keyboard; a host that
/// scales its lines for accessibility passes the scaled width.
#[allow(
    clippy::too_many_arguments,
    reason = "where, what it says, what it shows, and the three things the host knows"
)]
pub fn draw(
    sink: &mut impl CommandSink,
    p: &Palette,
    (x, y, h): (f32, f32, f32),
    label: &str,
    check: CheckState,
    state: State,
    focus_ring: f32,
) {
    let colours = paint(p, state);
    let b = box_rect(x, y, h);
    draw_box(sink, p, b, check, state, focus_ring, colours.mark);
    if !label.is_empty() {
        sink.emit(RenderCommand::Text {
            x: b.right() + LABEL_GAP,
            y: y + (h - FONT_SIZE) / 2.0,
            text: label.to_string(),
            color: fade(colours.label, state),
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

/// `c` as a control in `state` shows it: at the toolkit's disabled opacity
/// when it cannot be used.
fn fade(c: Color, state: State) -> Color {
    if state.disabled {
        let a = (f32::from(c.a) * DISABLED_OPACITY).round();
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "rounded, and between 0 and 255 because both factors are"
        )]
        let a = a as u8;
        Color::rgba(c.r, c.g, c.b, a)
    } else {
        c
    }
}

/// Draw a check box's box alone at `b`: the well, its edge in `state`, the
/// mark for `check` in `mark`, and the keyboard's ring -- with the theme's
/// corners. For a control that is a check box without a label of its own,
/// in a room another control would have had: `mark` is the caller's colour
/// for "set", held legible on the well here, so a switch whose "on" means
/// *safe* keeps its green as a box.
pub fn draw_box(
    sink: &mut impl CommandSink,
    p: &Palette,
    b: Rect,
    check: CheckState,
    state: State,
    focus_ring: f32,
    mark: Color,
) {
    let colours = paint(p, state);
    let mark = legible_on(mark, colours.well);
    let r = radius(p, b.w.min(b.h));
    sink.emit(RenderCommand::FillRect {
        x: b.x,
        y: b.y,
        width: b.w,
        height: b.h,
        color: fade(colours.well, state),
        corner_radii: CornerRadii::all(r),
    });
    sink.emit(RenderCommand::StrokeRect {
        x: b.x,
        y: b.y,
        width: b.w,
        height: b.h,
        color: fade(colours.edge, state),
        line_width: 1.0,
        corner_radii: CornerRadii::all(r),
    });
    match check {
        CheckState::Unchecked => {}
        CheckState::Checked => {
            // A tick: down to a point left of centre, then up to the right --
            // proportions of the box, so it scales with SIZE.
            let (x1, y1) = (b.x + b.w * 0.22, b.y + b.h * 0.52);
            let (x2, y2) = (b.x + b.w * 0.42, b.y + b.h * 0.74);
            let (x3, y3) = (b.x + b.w * 0.78, b.y + b.h * 0.28);
            for (xa, ya, xb, yb) in [(x1, y1, x2, y2), (x2, y2, x3, y3)] {
                sink.emit(RenderCommand::Line {
                    x1: xa,
                    y1: ya,
                    x2: xb,
                    y2: yb,
                    color: fade(mark, state),
                    width: MARK_WIDTH,
                });
            }
        }
        CheckState::Indeterminate => {
            sink.emit(RenderCommand::FillRect {
                x: b.x + (b.w - PARTLY) / 2.0,
                y: b.y + (b.h - PARTLY) / 2.0,
                width: PARTLY,
                height: PARTLY,
                color: fade(mark, state),
                corner_radii: CornerRadii::all(1.0),
            });
        }
    }
    if state.focused && !state.disabled && focus_ring > 0.0 && focus_ring.is_finite() {
        let reach = FOCUS_GAP + focus_ring;
        sink.emit(RenderCommand::StrokeRect {
            x: b.x - reach,
            y: b.y - reach,
            width: b.w + reach * 2.0,
            height: b.h + reach * 2.0,
            color: p.accent,
            line_width: focus_ring,
            corner_radii: CornerRadii::all(r + reach),
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::event::Modifiers;
    use crate::palette::TEXT_CONTRAST_FLOOR;
    use crate::theme::contrast_ratio;

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn drawn(check: CheckState, state: State) -> Vec<RenderCommand> {
        let p = Palette::for_mode(false);
        let mut cmds = Vec::new();
        draw(
            &mut cmds,
            &p,
            (10.0, 20.0, HEIGHT),
            "Show hidden files",
            check,
            state,
            2.0,
        );
        cmds
    }

    /// A two-state box is set or cleared by a click, and a box that was
    /// partly set is set -- "partly" is never chosen.
    #[test]
    fn a_two_state_box_is_set_or_cleared() {
        use CheckState::{Checked, Indeterminate, Unchecked};
        assert_eq!(next(Unchecked, Mode::TwoState), Checked);
        assert_eq!(next(Checked, Mode::TwoState), Unchecked);
        assert_eq!(next(Indeterminate, Mode::TwoState), Checked);
    }

    /// A three-state box steps round all three, and from "default" the first
    /// click says yes.
    #[test]
    fn a_three_state_box_steps_round_all_three_and_default_goes_to_yes() {
        use CheckState::{Checked, Indeterminate, Unchecked};
        assert_eq!(next(Indeterminate, Mode::ThreeState), Checked);
        assert_eq!(next(Checked, Mode::ThreeState), Unchecked);
        assert_eq!(next(Unchecked, Mode::ThreeState), Indeterminate);
    }

    /// **The box's corners are the theme's**: 2 pixels in the built-in theme,
    /// a circle at the most, and the focus ring round with them.
    #[test]
    fn the_boxs_corners_are_the_themes() {
        let corners = |radius: u8| -> (f32, f32) {
            let mut p = Palette::for_mode(false);
            p.widget_style.check.radius = radius;
            let mut cmds = Vec::new();
            let focused = State {
                focused: true,
                ..State::default()
            };
            draw(&mut cmds, &p, (0.0, 0.0, HEIGHT), "", CheckState::Checked, focused, 2.0);
            let well = cmds.iter().find_map(|c| match c {
                RenderCommand::FillRect { corner_radii, .. } => Some(corner_radii.top_left),
                _ => None,
            });
            let ring = cmds.iter().find_map(|c| match c {
                RenderCommand::StrokeRect {
                    color,
                    corner_radii,
                    ..
                } if *color == p.accent => Some(corner_radii.top_left),
                _ => None,
            });
            (well.expect("no well"), ring.expect("no ring"))
        };
        assert_eq!(corners(2), (2.0, 5.0), "the built-in box");
        assert_eq!(corners(7), (7.0, 10.0), "a circle");
        assert_eq!(corners(40), (7.0, 10.0), "never rounder than a circle");
        assert_eq!(Palette::for_mode(false).widget_style.check.radius, 2);
    }

    /// The box is an input's well with an input's edge; checked draws a tick
    /// of two strokes; partly draws a square; unchecked draws neither.
    #[test]
    fn each_state_draws_its_own_mark() {
        let p = Palette::for_mode(false);
        let none = drawn(CheckState::Unchecked, State::default());
        assert!(matches!(none[0], RenderCommand::FillRect { color, .. } if color == p.crust));
        assert!(matches!(none[1], RenderCommand::StrokeRect { color, .. } if color == p.surface1));
        assert!(!none.iter().any(|c| matches!(c, RenderCommand::Line { .. })));

        let ticked = drawn(CheckState::Checked, State::default());
        let strokes = ticked
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        assert_eq!(strokes, 2, "a tick is two strokes: {ticked:?}");

        let partly = drawn(CheckState::Indeterminate, State::default());
        assert!(
            !partly
                .iter()
                .any(|c| matches!(c, RenderCommand::Line { .. }))
        );
        assert!(
            partly.iter().any(|c| matches!(c, RenderCommand::FillRect { width, height, .. }
                if (*width - PARTLY).abs() < f32::EPSILON && (*height - PARTLY).abs() < f32::EPSILON)),
            "a partly-set box shows its square: {partly:?}"
        );
    }

    /// The mark reads on the well in both modes, for every hue the accent can
    /// be -- including a custom one too pale to read unaided.
    #[test]
    fn the_mark_reads_on_the_well_whatever_the_accent() {
        for light in [false, true] {
            let mut p = Palette::for_mode(light);
            for accent in [
                p.blue,
                p.green,
                p.red,
                p.yellow,
                p.peach,
                p.lavender,
                p.mauve,
                p.sapphire,
                p.teal,
                p.sky,
                p.pink,
                p.rosewater,
                p.flamingo,
                p.maroon,
                Color::rgb(0xFF, 0xFF, 0xE0),
                Color::rgb(0x10, 0x10, 0x20),
            ] {
                p.accent = accent;
                let mark = paint(&p, State::default()).mark;
                let c = contrast_ratio(mark, p.crust);
                assert!(
                    c >= TEXT_CONTRAST_FLOOR,
                    "{accent:?} in light={light}: {c:.2}:1"
                );
            }
        }
    }

    /// Under the pointer the edge takes a tint of the accent; focused, a ring
    /// in the accent goes round the box; disabled, everything is dimmed and
    /// there is no ring.
    #[test]
    fn hover_tints_focus_rings_and_disabled_dims() {
        let p = Palette::for_mode(false);
        let lit = drawn(
            CheckState::Checked,
            State {
                hovered: true,
                ..State::default()
            },
        );
        assert!(
            matches!(lit[1], RenderCommand::StrokeRect { color, .. } if color != p.surface1),
            "hovered edge: {lit:?}"
        );
        let ringed = drawn(
            CheckState::Checked,
            State {
                focused: true,
                ..State::default()
            },
        );
        let rings = ringed
            .iter()
            .filter(|c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == p.accent))
            .count();
        assert_eq!(rings, 1, "{ringed:?}");
        let dim = drawn(
            CheckState::Checked,
            State {
                disabled: true,
                focused: true,
                hovered: true,
            },
        );
        assert!(
            !dim.iter().any(
                |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == p.accent)
            )
        );
        for c in &dim {
            let a = match c {
                RenderCommand::FillRect { color, .. }
                | RenderCommand::StrokeRect { color, .. }
                | RenderCommand::Line { color, .. }
                | RenderCommand::Text { color, .. } => color.a,
                other => panic!("unexpected {other:?}"),
            };
            assert!(a < u8::MAX, "not dimmed: {c:?}");
        }
    }

    /// The label is part of the control: the region covers it, and the box's
    /// own region is at least a 24-pixel target.
    #[test]
    fn the_label_and_a_generous_box_are_the_target() {
        let label = "Show hidden files";
        let r = hit(10.0, 20.0, HEIGHT, label);
        let b = box_rect(10.0, 20.0, HEIGHT);
        assert!(
            r.contains(10.0 + width(label) - 1.0, 30.0),
            "the label's end: {r:?}"
        );
        assert!(
            r.contains(b.x - 4.0, b.y + b.h / 2.0),
            "just left of the box: {r:?}"
        );
        assert!(r.h >= grab::MIN_TARGET, "{r:?}");
        assert!(
            !r.contains(10.0 + width(label) + 20.0, 30.0),
            "well past the label: {r:?}"
        );
    }

    /// Space flips a focused box; Enter does not (it presses the dialog's
    /// default button); shortcuts and releases do not.
    #[test]
    fn space_flips_it_and_enter_does_not() {
        assert!(toggles(&key(Key::Space)));
        assert!(!toggles(&key(Key::Enter)));
        let mut released = key(Key::Space);
        released.pressed = false;
        assert!(!toggles(&released));
        let mut ctrl = key(Key::Space);
        ctrl.modifiers.ctrl = true;
        assert!(!toggles(&ctrl));
    }
}
