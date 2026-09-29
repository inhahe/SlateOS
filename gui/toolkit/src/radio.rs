//! Radio buttons: a group of choices of which at most one is chosen.
//!
//! # A group, not a button
//!
//! A radio button means nothing alone -- choosing one un-chooses the others --
//! so the state here is the *group's*: which option is chosen ([`RadioGroup`]),
//! and which the keyboard is on. Each option is drawn by [`draw`] with a label
//! beside a round well, as the checkbox is drawn with a square one
//! ([`crate::checkbox`], whose look this shares: the input's well and edge,
//! the accent's dot, the label a few pixels off).
//!
//! # Going back to no choice at all
//!
//! `design.txt` asks: "allow a way for the user to go back to having no radio
//! button selected? can't think of a good way to do that, other than clicking
//! again on the currently selected one, which isn't a very good way." Clicking
//! the chosen option again is the way here, but only in a group that says so
//! ([`RadioGroup::deselectable`]). In most groups -- a theme's light or dark,
//! a paper size -- *no choice* is not an answer, and a click that silently
//! removed the choice would leave a setting with no value; there the click
//! does nothing, as everywhere else. A group whose "none" means something (a
//! filter, "any") opts in, and then Space on the chosen option clears it too,
//! so the keyboard can do what the pointer can. `design-decisions.md` §1432.
//!
//! # The keyboard, as every desktop's radio group has it
//!
//! Up and Left choose the previous option, Down and Right the next, wrapping
//! at the ends -- in a radio group the arrows *choose*, they do not only move
//! (the WAI-ARIA radio group pattern, which is what the other toolkits do).
//! Home and End choose the first and last. Space chooses the option the
//! keyboard is on. Keys with Ctrl, Alt or Super are the program's.

use crate::checkbox::{FONT_SIZE, LABEL_GAP, SIZE};
use crate::color::Color;
use crate::disabled::DISABLED_OPACITY;
use crate::event::{Key, KeyEvent};
use crate::frame::Rect;
use crate::palette::{Palette, legible_on};
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::step;
use crate::style::CornerRadii;
use crate::surface::CommandSink;

/// The dot's diameter in a chosen option.
const DOT: f32 = 6.0;
/// How far the edge is tinted towards the accent under the pointer.
const HOVER_TINT: f32 = 0.5;
/// The space between the circle and the keyboard ring round it.
const FOCUS_GAP: f32 = 1.0;

/// What became of a group's choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioEvent {
    /// This option is chosen now.
    Selected(usize),
    /// Nothing is chosen now -- only in a group that allows it.
    Cleared,
}

/// A group of `len` options, at most one chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RadioGroup {
    len: usize,
    selected: Option<usize>,
    /// The option the keyboard is on. Follows the choice, and stays where it
    /// was when the choice is cleared, so the arrows go on from there.
    focus: usize,
    deselectable: bool,
}

impl RadioGroup {
    /// A group of `len` options with `selected` chosen -- or none, if it is
    /// `None` or out of range.
    #[must_use]
    pub fn new(len: usize, selected: Option<usize>) -> Self {
        let selected = selected.filter(|&i| i < len);
        Self {
            len,
            selected,
            focus: selected.unwrap_or(0),
            deselectable: false,
        }
    }

    /// Whether a click on the chosen option clears the choice. Off by
    /// default: see the module docs.
    #[must_use]
    pub const fn deselectable(mut self, yes: bool) -> Self {
        self.deselectable = yes;
        self
    }

    /// How many options there are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the group has no options.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The chosen option, if any.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The option the keyboard is on.
    #[must_use]
    pub const fn focus(&self) -> usize {
        self.focus
    }

    /// Choose `index` (or nothing, with `None`) without reporting it: for a
    /// choice made somewhere else. An index out of range chooses nothing.
    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index.filter(|&i| i < self.len);
        if let Some(i) = self.selected {
            self.focus = i;
        }
    }

    /// A click on option `index`.
    ///
    /// Chooses it; or, in a group that allows it, clears the choice if it was
    /// already chosen. `None` when nothing changed -- a click on the chosen
    /// option of an ordinary group, or on an option that does not exist.
    pub fn click(&mut self, index: usize) -> Option<RadioEvent> {
        if index >= self.len {
            return None;
        }
        self.focus = index;
        self.choose_or_clear(index)
    }

    /// Choose `index`, or clear it if it is chosen and the group allows that.
    fn choose_or_clear(&mut self, index: usize) -> Option<RadioEvent> {
        if self.selected == Some(index) {
            if self.deselectable {
                self.selected = None;
                return Some(RadioEvent::Cleared);
            }
            return None;
        }
        self.selected = Some(index);
        Some(RadioEvent::Selected(index))
    }

    /// Choose `index` if it is not already chosen.
    fn choose(&mut self, index: usize) -> Option<RadioEvent> {
        self.focus = index;
        if self.selected == Some(index) {
            return None;
        }
        self.selected = Some(index);
        Some(RadioEvent::Selected(index))
    }

    /// Act on a key while the group has the keyboard. Returns what happened
    /// to the choice, and whether the key was the group's at all, as
    /// `(event, taken)`.
    pub fn handle_key(&mut self, key: &KeyEvent) -> (Option<RadioEvent>, bool) {
        let m = key.modifiers;
        if !key.pressed || m.ctrl || m.alt || m.super_key || self.len == 0 {
            return (None, false);
        }
        let from = self.selected.unwrap_or(self.focus);
        let event = match key.key {
            Key::Up | Key::Left => self.choose(step::wrapping_before(self.len, from)),
            Key::Down | Key::Right => self.choose(step::wrapping_after(self.len, from)),
            Key::Home => self.choose(0),
            Key::End => self.choose(self.len.saturating_sub(1)),
            Key::Space => {
                let at = self.focus;
                self.choose_or_clear(at)
            }
            _ => return (None, false),
        };
        (event, true)
    }
}

/// What is happening to one option now, which the host knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The pointer is over it.
    pub hovered: bool,
    /// The keyboard is on it: a ring round its circle in the accent.
    pub focused: bool,
    /// It cannot be chosen now: drawn at the toolkit's disabled opacity.
    pub disabled: bool,
}

/// The width an option with `label` needs: the circle, the gap and the label.
#[must_use]
pub fn width(label: &str) -> f32 {
    crate::checkbox::width(label)
}

/// Where the circle is for an option whose row starts at `(x, y)` and is `h`
/// tall.
#[must_use]
pub fn circle_rect(x: f32, y: f32, h: f32) -> Rect {
    crate::checkbox::box_rect(x, y, h)
}

/// The region a press chooses the option in: the circle grown as
/// [`crate::grab::handle`] grows a handle, and the label beside it.
#[must_use]
pub fn hit(x: f32, y: f32, h: f32, label: &str) -> Rect {
    crate::checkbox::hit(x, y, h, label)
}

/// Draw one option whose row starts at `(x, y)` and is `h` tall.
#[allow(
    clippy::too_many_arguments,
    reason = "where, what it says, whether it is chosen, and the three things the host knows"
)]
pub fn draw(
    sink: &mut impl CommandSink,
    p: &Palette,
    (x, y, h): (f32, f32, f32),
    label: &str,
    chosen: bool,
    state: State,
    focus_ring: f32,
) {
    let fade = |c: Color| {
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
    };
    let edge = if state.hovered && !state.disabled {
        p.surface1.lerp(p.accent, HOVER_TINT)
    } else {
        p.surface1
    };
    let c = circle_rect(x, y, h);
    let round = CornerRadii::all(SIZE / 2.0);
    sink.emit(RenderCommand::FillRect {
        x: c.x,
        y: c.y,
        width: c.w,
        height: c.h,
        color: fade(p.crust),
        corner_radii: round,
    });
    sink.emit(RenderCommand::StrokeRect {
        x: c.x,
        y: c.y,
        width: c.w,
        height: c.h,
        color: fade(edge),
        line_width: 1.0,
        corner_radii: round,
    });
    if chosen {
        sink.emit(RenderCommand::FillRect {
            x: c.x + (c.w - DOT) / 2.0,
            y: c.y + (c.h - DOT) / 2.0,
            width: DOT,
            height: DOT,
            color: fade(legible_on(p.accent, p.crust)),
            corner_radii: CornerRadii::all(DOT / 2.0),
        });
    }
    if state.focused && !state.disabled && focus_ring > 0.0 && focus_ring.is_finite() {
        let reach = FOCUS_GAP + focus_ring;
        let d = SIZE + reach * 2.0;
        sink.emit(RenderCommand::StrokeRect {
            x: c.x - reach,
            y: c.y - reach,
            width: d,
            height: d,
            color: p.accent,
            line_width: focus_ring,
            corner_radii: CornerRadii::all(d / 2.0),
        });
    }
    if !label.is_empty() {
        sink.emit(RenderCommand::Text {
            x: c.right() + LABEL_GAP,
            y: y + (h - FONT_SIZE) / 2.0,
            text: label.to_string(),
            color: fade(p.text),
            font_size: FONT_SIZE,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::checkbox::HEIGHT;
    use crate::event::Modifiers;

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    /// A click chooses; a click on the chosen option of an ordinary group does
    /// nothing; a click past the end does nothing.
    #[test]
    fn a_click_chooses_and_the_chosen_one_stays_chosen() {
        let mut g = RadioGroup::new(3, Some(0));
        assert_eq!(g.click(2), Some(RadioEvent::Selected(2)));
        assert_eq!(g.selected(), Some(2));
        assert_eq!(g.click(2), None, "an ordinary group keeps its choice");
        assert_eq!(g.selected(), Some(2));
        assert_eq!(g.click(3), None);
        assert_eq!(g.selected(), Some(2));
    }

    /// In a group that allows it, a click on the chosen option clears the
    /// choice, and Space on it does too.
    #[test]
    fn a_deselectable_group_can_go_back_to_no_choice() {
        let mut g = RadioGroup::new(3, Some(1)).deselectable(true);
        assert_eq!(g.click(1), Some(RadioEvent::Cleared));
        assert_eq!(g.selected(), None);
        assert_eq!(g.click(1), Some(RadioEvent::Selected(1)));
        let (event, taken) = g.handle_key(&key(Key::Space));
        assert_eq!((event, taken), (Some(RadioEvent::Cleared), true));
        assert_eq!(g.selected(), None);
        assert_eq!(g.focus(), 1, "the keyboard stays where it was");
    }

    /// The arrows choose, wrapping at the ends; Home and End choose the first
    /// and last; Space chooses the option the keyboard is on.
    #[test]
    fn the_arrows_choose_and_wrap() {
        let mut g = RadioGroup::new(3, Some(0));
        assert_eq!(
            g.handle_key(&key(Key::Down)),
            (Some(RadioEvent::Selected(1)), true)
        );
        assert_eq!(
            g.handle_key(&key(Key::Right)),
            (Some(RadioEvent::Selected(2)), true)
        );
        assert_eq!(
            g.handle_key(&key(Key::Down)),
            (Some(RadioEvent::Selected(0)), true),
            "wraps"
        );
        assert_eq!(
            g.handle_key(&key(Key::Up)),
            (Some(RadioEvent::Selected(2)), true),
            "wraps back"
        );
        assert_eq!(
            g.handle_key(&key(Key::Home)),
            (Some(RadioEvent::Selected(0)), true)
        );
        assert_eq!(
            g.handle_key(&key(Key::End)),
            (Some(RadioEvent::Selected(2)), true)
        );
        assert_eq!(g.handle_key(&key(Key::End)), (None, true), "already chosen");

        let mut empty = RadioGroup::new(3, None);
        assert_eq!(
            empty.handle_key(&key(Key::Space)),
            (Some(RadioEvent::Selected(0)), true)
        );
    }

    /// Keys a group has no use for, shortcuts and releases are passed on.
    #[test]
    fn other_keys_are_passed_on() {
        let mut g = RadioGroup::new(3, Some(1));
        assert_eq!(g.handle_key(&key(Key::Tab)), (None, false));
        let mut ctrl = key(Key::Down);
        ctrl.modifiers.ctrl = true;
        assert_eq!(g.handle_key(&ctrl), (None, false));
        let mut released = key(Key::Down);
        released.pressed = false;
        assert_eq!(g.handle_key(&released), (None, false));
        assert_eq!(g.selected(), Some(1));
        assert_eq!(
            RadioGroup::new(0, None).handle_key(&key(Key::Down)),
            (None, false)
        );
    }

    /// An out-of-range starting choice is no choice.
    #[test]
    fn an_out_of_range_choice_is_none() {
        assert_eq!(RadioGroup::new(2, Some(5)).selected(), None);
        let mut g = RadioGroup::new(2, None);
        g.set_selected(Some(9));
        assert_eq!(g.selected(), None);
        g.set_selected(Some(1));
        assert_eq!((g.selected(), g.focus()), (Some(1), 1));
    }

    /// A chosen option shows a dot in its well; an unchosen one does not; the
    /// focus ring is round and in the accent; disabled dims and drops it.
    #[test]
    fn the_dot_the_ring_and_the_dimming() {
        let p = Palette::for_mode(false);
        let draw_one = |chosen: bool, state: State| {
            let mut cmds = Vec::new();
            draw(
                &mut cmds,
                &p,
                (0.0, 0.0, HEIGHT),
                "Any size",
                chosen,
                state,
                2.0,
            );
            cmds
        };
        let off = draw_one(false, State::default());
        let on = draw_one(true, State::default());
        assert_eq!(on.len(), off.len() + 1, "the dot: {on:?}");
        assert!(
            matches!(off[0], RenderCommand::FillRect { color, corner_radii, .. }
            if color == p.crust && (corner_radii.top_left - SIZE / 2.0).abs() < f32::EPSILON)
        );

        let ringed = draw_one(
            true,
            State {
                focused: true,
                ..State::default()
            },
        );
        assert!(
            ringed.iter().any(
                |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == p.accent)
            )
        );
        let dim = draw_one(
            true,
            State {
                focused: true,
                disabled: true,
                hovered: false,
            },
        );
        assert!(
            !dim.iter().any(
                |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == p.accent)
            )
        );
        assert!(dim.iter().all(|c| match c {
            RenderCommand::FillRect { color, .. }
            | RenderCommand::StrokeRect { color, .. }
            | RenderCommand::Text { color, .. } => color.a < u8::MAX,
            _ => true,
        }));
    }
}
