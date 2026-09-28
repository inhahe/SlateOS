//! A drop-down list: a field showing one choice of several, which opens a
//! list to choose another.
//!
//! # Why the toolkit has one now
//!
//! Five programs drew their own -- the Settings application twice, the unit
//! converter, and inside the toolkit the menu bar and the path bar each had a
//! list that drops from a field. A drop-down is a control whose keys a user
//! learns once, and they were different in each. This is the one a program
//! can draw instead, like [`crate::button`], [`crate::checkbox`] and
//! [`crate::slider`].
//!
//! # How it looks
//!
//! The field is the Aero reference's `aero-srch-select`: 26 pixels tall, eight
//! pixels in from each side, 12.5-pixel text, three-pixel corners, a light
//! well with a thin edge -- here the input's well and edge every toolkit field
//! has (`crust`, `surface1`), so a drop-down and the text field beside it are
//! one family -- and a chevron at the right that says it opens. The list is
//! the toolkit's context menu ([`ContextMenu`]), as wide as the field or
//! wider, with the current choice ticked and highlighted, so it flips upwards
//! near the bottom of the screen and scrolls when it is long, as menus do.
//!
//! # The keyboard, as the desktop's own drop-downs have it
//!
//! Closed, the arrows *change the choice* without opening the list (as a
//! Windows drop-down list does), Home and End go to the first and last, and a
//! letter goes to the next choice starting with it. Alt+Down, F4, Space or
//! Enter open the list. Open, the arrows move the highlight, Enter or Space
//! chooses it and closes, Escape closes and changes nothing, and Tab closes
//! and is passed on so the focus moves.

use crate::color::Color;
use crate::disabled::DISABLED_OPACITY;
use crate::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::menu::{ContextMenu, MenuAction, MenuItem};
use crate::palette::Palette;
use crate::render::{FontWeightHint, RenderCommand, TextOverflow};
use crate::style::CornerRadii;
use crate::surface::CommandSink;

/// A drop-down's height where the caller has no layout of its own: the
/// reference's 26.
pub const HEIGHT: f32 = 26.0;
/// Room from each side of the field to its text: the reference's
/// `padding: 0 8px`.
pub const PADDING_H: f32 = 8.0;
/// The text's size: the reference's 12.5 pixels.
pub const FONT_SIZE: f32 = 12.5;
/// The field's corners: the reference's 3.
const RADIUS: f32 = 3.0;
/// The chevron's width; its height is half this.
const CHEVRON: f32 = 8.0;
/// From the text's end to the chevron.
const CHEVRON_GAP: f32 = 6.0;
/// How far the edge is tinted towards the accent under the pointer.
const HOVER_TINT: f32 = 0.5;

/// What a drop-down did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropdownEvent {
    /// This choice is the drop-down's value now.
    Selected(usize),
    /// The list opened: the host draws [`Dropdown::draw_list`] on top of
    /// everything from now until [`DropdownEvent::Closed`].
    Opened,
    /// The list closed without a new choice.
    Closed,
}

/// What is happening to the field now, which the host knows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The pointer is over it.
    pub hovered: bool,
    /// The keyboard is on it: a ring round the field in the accent.
    pub focused: bool,
    /// It cannot be changed now: drawn at the toolkit's disabled opacity.
    pub disabled: bool,
}

/// A drop-down list of `options`, at most one chosen.
pub struct Dropdown {
    options: Vec<String>,
    selected: Option<usize>,
    /// Drawn in the field when nothing is chosen.
    placeholder: String,
    /// The list, while it is open.
    list: ContextMenu,
}

impl Dropdown {
    /// A drop-down of `options` with `selected` chosen -- none, if it is
    /// `None` or out of range.
    #[must_use]
    pub fn new(options: Vec<String>, selected: Option<usize>) -> Self {
        let selected = selected.filter(|&i| i < options.len());
        Self {
            list: ContextMenu::new(Vec::new()),
            options,
            selected,
            placeholder: String::new(),
        }
    }

    /// What the field says while nothing is chosen.
    #[must_use]
    pub fn with_placeholder(mut self, text: impl Into<String>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// The choices.
    #[must_use]
    pub fn options(&self) -> &[String] {
        &self.options
    }

    /// The chosen option, if any.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Choose `index` (or nothing) without reporting it: for a value that
    /// changed somewhere else. Out of range chooses nothing.
    pub fn set_selected(&mut self, index: Option<usize>) {
        self.selected = index.filter(|&i| i < self.options.len());
    }

    /// Whether the list is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.list.is_visible()
    }

    /// Open the list under `field`, on a screen `viewport` wide and tall: as
    /// wide as the field or wider, the current choice ticked, highlighted and
    /// scrolled into view.
    ///
    /// Placed about the *field*, not about a point. A menu flips about the
    /// point it is shown at, which for a list dropped from a field near the
    /// bottom of the screen means opening upwards *over the field* -- and near
    /// the right edge, leaping to the left of the field's left edge. So where
    /// the list does not fit below, it goes above the field's top; where it
    /// does not fit to the right, its right edge lines up with the field's.
    /// Only a list taller than the room on both sides still covers the field,
    /// because the screen has nowhere else to put it.
    pub fn open(&mut self, field: Rect, viewport: (f32, f32)) {
        let items = self
            .options
            .iter()
            .enumerate()
            .map(|(i, label)| MenuItem::Action {
                id: u64::try_from(i).unwrap_or(u64::MAX),
                label: label.clone(),
                shortcut: None,
                icon: None,
                enabled: true,
                checked: Some(self.selected == Some(i)),
            })
            .collect();
        self.list = ContextMenu::new(items);
        self.list.set_min_width(field.w);
        self.list.show(field.x, field.bottom(), viewport);
        if let Some(panel) = self.list.panel_rect() {
            let x = if field.x + panel.w > viewport.0 {
                (field.right() - panel.w).max(0.0)
            } else {
                field.x
            };
            let fits_below = field.bottom() + panel.h <= viewport.1;
            let y = if !fits_below && field.y >= panel.h {
                field.y - panel.h
            } else {
                field.bottom()
            };
            if (x, y) != (field.x, field.bottom()) {
                self.list.show(x, y, viewport);
            }
        }
        if let Some(i) = self.selected {
            self.list.highlight(i);
        }
    }

    /// Close the list without a choice.
    pub fn close(&mut self) {
        self.list.hide();
    }

    /// The list's commands while it is open, to be drawn on top of everything
    /// else in the window; nothing while it is closed.
    #[must_use]
    pub fn draw_list(&self, p: &Palette) -> Vec<RenderCommand> {
        if self.list.is_visible() {
            self.list.render(p)
        } else {
            Vec::new()
        }
    }

    /// Where option `index` is drawn in the open list.
    #[must_use]
    pub fn list_item_rect(&self, index: usize) -> Option<Rect> {
        self.list.item_rect(index)
    }

    /// The option the open list has highlighted.
    #[must_use]
    pub fn highlighted(&self) -> Option<usize> {
        self.list.highlighted()
    }

    /// The open list's panel.
    #[must_use]
    pub fn list_rect(&self) -> Option<Rect> {
        self.list.panel_rect()
    }

    /// Choose the list's item `id` and close.
    fn choose_item(&mut self, id: u64) -> Option<DropdownEvent> {
        let index = usize::try_from(id)
            .ok()
            .filter(|&i| i < self.options.len())?;
        self.list.hide();
        self.selected = Some(index);
        Some(DropdownEvent::Selected(index))
    }

    /// Act on a mouse event, with the field at `field` on a screen `viewport`
    /// big. Send every event: the open list needs the moves for its highlight
    /// and the wheel to scroll.
    pub fn handle_mouse(
        &mut self,
        field: Rect,
        event: &MouseEvent,
        viewport: (f32, f32),
    ) -> Option<DropdownEvent> {
        let (x, y) = (event.x, event.y);
        if !self.list.is_visible() {
            return match event.kind {
                MouseEventKind::Press(MouseButton::Left) if field.contains(x, y) => {
                    self.open(field, viewport);
                    Some(DropdownEvent::Opened)
                }
                _ => None,
            };
        }
        match event.kind {
            MouseEventKind::Move => {
                self.list.handle_mouse_move(x, y);
                None
            }
            MouseEventKind::Scroll { dy, .. } => {
                self.list.handle_scroll(x, y, dy);
                None
            }
            // The list first: it is drawn on top, and where it covers the
            // field (a list too tall for either side) a press there is aimed
            // at the item the user can see. Anywhere outside the list closes
            // it -- a second press on the field included, which is how a
            // drop-down folds away again.
            MouseEventKind::Press(MouseButton::Left) => match self.list.handle_click(x, y) {
                Some(id) => self.choose_item(id),
                // A press outside closed it; one on the list's padding left
                // it open.
                None if !self.list.is_visible() => Some(DropdownEvent::Closed),
                None => None,
            },
            _ => None,
        }
    }

    /// Act on a key while the drop-down has the keyboard. Returns what
    /// happened, and whether the key was the drop-down's at all, as
    /// `(event, taken)`.
    pub fn handle_key(
        &mut self,
        field: Rect,
        key: &KeyEvent,
        viewport: (f32, f32),
    ) -> (Option<DropdownEvent>, bool) {
        if !key.pressed {
            return (None, false);
        }
        let m = key.modifiers;
        if self.list.is_visible() {
            return match key.key {
                Key::Tab => {
                    // Closed, and passed on so the focus moves.
                    self.list.hide();
                    (Some(DropdownEvent::Closed), false)
                }
                Key::F4 => {
                    self.list.hide();
                    (Some(DropdownEvent::Closed), true)
                }
                Key::Up if m.alt => {
                    self.list.hide();
                    (Some(DropdownEvent::Closed), true)
                }
                Key::Space => match self.list.highlighted() {
                    Some(i) => (self.choose_item(u64::try_from(i).unwrap_or(u64::MAX)), true),
                    None => (None, true),
                },
                _ => match self.list.handle_key(key) {
                    Some(MenuAction::Selected(id)) => (self.choose_item(id), true),
                    Some(MenuAction::Closed) => (Some(DropdownEvent::Closed), true),
                    _ => (None, true),
                },
            };
        }
        if m.ctrl || m.super_key {
            return (None, false);
        }
        let last = self.options.len().checked_sub(1);
        match key.key {
            Key::Down if m.alt => {
                self.open(field, viewport);
                (Some(DropdownEvent::Opened), true)
            }
            Key::F4 | Key::Space | Key::Enter if !m.alt => {
                self.open(field, viewport);
                (Some(DropdownEvent::Opened), true)
            }
            _ if m.alt => (None, false),
            Key::Down => (
                self.select_now(self.selected.map_or(Some(0), |i| {
                    Some(i.saturating_add(1).min(last.unwrap_or(0)))
                })),
                true,
            ),
            Key::Up => (
                self.select_now(self.selected.map_or(Some(0), |i| Some(i.saturating_sub(1)))),
                true,
            ),
            Key::Home => (self.select_now(Some(0)), true),
            Key::End => (self.select_now(last), true),
            _ => match key.single_char() {
                Some(c) if !c.is_control() && !c.is_whitespace() => {
                    (self.select_now(self.next_starting_with(c)), true)
                }
                _ => (None, false),
            },
        }
    }

    /// Choose `index` now, if it exists and is not already chosen.
    fn select_now(&mut self, index: Option<usize>) -> Option<DropdownEvent> {
        let index = index.filter(|&i| i < self.options.len())?;
        if self.selected == Some(index) {
            return None;
        }
        self.selected = Some(index);
        Some(DropdownEvent::Selected(index))
    }

    /// The next option after the current one whose label starts with `c`
    /// (ignoring case), wrapping round -- so pressing a letter again steps
    /// through the choices that start with it.
    fn next_starting_with(&self, c: char) -> Option<usize> {
        let len = self.options.len();
        if len == 0 {
            return None;
        }
        let start = self
            .selected
            .map_or(0, |i| crate::step::wrapping_after(len, i));
        let lower: String = c.to_lowercase().collect();
        crate::step::indices(len, start, true).find(|&i| {
            self.options
                .get(i)
                .is_some_and(|o| o.to_lowercase().starts_with(&lower))
        })
    }

    /// Draw the field at `field`: the chosen option's label (or the
    /// placeholder, dimmer), and the chevron.
    pub fn draw(
        &self,
        sink: &mut impl CommandSink,
        p: &Palette,
        field: Rect,
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
        let edge = if (state.hovered || self.list.is_visible()) && !state.disabled {
            p.surface1.lerp(p.accent, HOVER_TINT)
        } else {
            p.surface1
        };
        sink.emit(RenderCommand::FillRect {
            x: field.x,
            y: field.y,
            width: field.w,
            height: field.h,
            color: fade(p.crust),
            corner_radii: CornerRadii::all(RADIUS),
        });
        sink.emit(RenderCommand::StrokeRect {
            x: field.x,
            y: field.y,
            width: field.w,
            height: field.h,
            color: fade(edge),
            line_width: 1.0,
            corner_radii: CornerRadii::all(RADIUS),
        });
        let (text, ink) = match self.selected.and_then(|i| self.options.get(i)) {
            Some(label) => (label.as_str(), p.text),
            None => (self.placeholder.as_str(), p.subtext0),
        };
        if !text.is_empty() {
            sink.emit(RenderCommand::Text {
                x: field.x + PADDING_H,
                y: field.y + (field.h - FONT_SIZE) / 2.0,
                text: text.to_string(),
                color: fade(ink),
                font_size: FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some((field.w - PADDING_H * 2.0 - CHEVRON - CHEVRON_GAP).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
        // The chevron: a V at the right, pointing the way the list opens.
        let cx = field.right() - PADDING_H - CHEVRON / 2.0;
        let cy = field.y + field.h / 2.0;
        let half = CHEVRON / 2.0;
        for (x1, y1, x2, y2) in [
            (cx - half, cy - half / 2.0, cx, cy + half / 2.0),
            (cx, cy + half / 2.0, cx + half, cy - half / 2.0),
        ] {
            sink.emit(RenderCommand::Line {
                x1,
                y1,
                x2,
                y2,
                color: fade(p.subtext0),
                width: 1.5,
            });
        }
        if state.focused && !state.disabled && focus_ring > 0.0 && focus_ring.is_finite() {
            sink.emit(RenderCommand::StrokeRect {
                x: field.x - focus_ring,
                y: field.y - focus_ring,
                width: field.w + focus_ring * 2.0,
                height: field.h + focus_ring * 2.0,
                color: p.accent,
                line_width: focus_ring,
                corner_radii: CornerRadii::all(RADIUS + focus_ring),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::expect_used, clippy::indexing_slicing)]

    use super::*;
    use crate::event::Modifiers;

    const VIEW: (f32, f32) = (1024.0, 768.0);

    fn field() -> Rect {
        Rect::new(100.0, 100.0, 220.0, HEIGHT)
    }

    fn sizes() -> Dropdown {
        Dropdown::new(
            ["Letter", "Legal", "A4", "A5", "Tabloid"]
                .map(String::from)
                .to_vec(),
            Some(2),
        )
    }

    /// In the bottom-right corner there is room neither below nor to the
    /// right: the list opens above the field, its right edge on the field's,
    /// and covers none of it -- a menu flipped about a point would have opened
    /// over the field and to the left of it.
    #[test]
    fn in_a_corner_the_list_opens_above_and_right_aligned_without_covering_the_field() {
        let mut d = sizes();
        let f = Rect::new(VIEW.0 - 120.0, VIEW.1 - 40.0, 110.0, HEIGHT);
        assert_eq!(
            d.handle_mouse(f, &press(f.x + 10.0, f.y + 10.0), VIEW),
            Some(DropdownEvent::Opened)
        );
        let list = d.list_rect().expect("open");
        assert!(
            list.bottom() <= f.y + 0.01,
            "above the field: {list:?} vs {f:?}"
        );
        assert!(
            (list.right() - f.right()).abs() < 0.01,
            "right edges aligned: {list:?} vs {f:?}"
        );
        assert!(list.intersect(f).is_none(), "the list covers the field");

        // And a second press on the field -- outside the list now -- folds it.
        assert_eq!(
            d.handle_mouse(f, &press(f.x + 10.0, f.y + 10.0), VIEW),
            Some(DropdownEvent::Closed)
        );
        assert!(!d.is_open());
    }

    fn key(k: Key) -> KeyEvent {
        KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        }
    }

    fn typed(c: char) -> KeyEvent {
        KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        }
    }

    fn press(x: f32, y: f32) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        }
    }

    /// A press on the field opens the list below it, at least as wide as the
    /// field, with the current choice highlighted.
    #[test]
    fn a_press_opens_the_list_under_the_field_on_the_current_choice() {
        let mut d = sizes();
        let f = field();
        assert_eq!(
            d.handle_mouse(f, &press(150.0, 110.0), VIEW),
            Some(DropdownEvent::Opened)
        );
        assert!(d.is_open());
        let list = d.list_rect().expect("open");
        assert!(list.y >= f.bottom() - 0.01, "below the field: {list:?}");
        assert!(list.w >= f.w, "at least as wide as the field: {list:?}");
        assert_eq!(d.highlighted(), Some(2));
    }

    /// Clicking an option chooses it and closes; clicking outside closes and
    /// changes nothing; a second press on the field folds the list away.
    #[test]
    fn the_mouse_chooses_closes_and_folds() {
        let mut d = sizes();
        let f = field();
        d.handle_mouse(f, &press(150.0, 110.0), VIEW);
        let (x, y) = d.list_item_rect(4).expect("Tabloid is shown").centre();
        assert_eq!(
            d.handle_mouse(f, &press(x, y), VIEW),
            Some(DropdownEvent::Selected(4))
        );
        assert!(!d.is_open());
        assert_eq!(d.selected(), Some(4));

        d.handle_mouse(f, &press(150.0, 110.0), VIEW);
        assert_eq!(
            d.handle_mouse(f, &press(900.0, 700.0), VIEW),
            Some(DropdownEvent::Closed)
        );
        assert_eq!(d.selected(), Some(4));

        d.handle_mouse(f, &press(150.0, 110.0), VIEW);
        assert_eq!(
            d.handle_mouse(f, &press(150.0, 110.0), VIEW),
            Some(DropdownEvent::Closed)
        );
        assert!(!d.is_open());
    }

    /// Closed, the arrows change the choice without opening, stopping at the
    /// ends; Home and End go to the ends; a letter goes to the next choice
    /// starting with it, and again to the one after.
    #[test]
    fn closed_the_keys_change_the_choice_in_place() {
        let mut d = sizes();
        let f = field();
        assert_eq!(
            d.handle_key(f, &key(Key::Down), VIEW),
            (Some(DropdownEvent::Selected(3)), true)
        );
        assert!(!d.is_open());
        assert_eq!(
            d.handle_key(f, &key(Key::Up), VIEW),
            (Some(DropdownEvent::Selected(2)), true)
        );
        assert_eq!(
            d.handle_key(f, &key(Key::End), VIEW),
            (Some(DropdownEvent::Selected(4)), true)
        );
        assert_eq!(
            d.handle_key(f, &key(Key::Down), VIEW),
            (None, true),
            "stops at the end"
        );
        assert_eq!(
            d.handle_key(f, &key(Key::Home), VIEW),
            (Some(DropdownEvent::Selected(0)), true)
        );
        assert_eq!(
            d.handle_key(f, &key(Key::Up), VIEW),
            (None, true),
            "stops at the start"
        );
        // "L" from Letter goes on to Legal, then round to Letter.
        assert_eq!(
            d.handle_key(f, &typed('l'), VIEW),
            (Some(DropdownEvent::Selected(1)), true)
        );
        assert_eq!(
            d.handle_key(f, &typed('L'), VIEW),
            (Some(DropdownEvent::Selected(0)), true)
        );
        assert_eq!(
            d.handle_key(f, &typed('a'), VIEW),
            (Some(DropdownEvent::Selected(2)), true)
        );
    }

    /// Alt+Down, F4, Space and Enter open the list; Tab and shortcuts are
    /// passed on.
    #[test]
    fn the_opening_keys_open_it() {
        let f = field();
        for k in [Key::F4, Key::Space, Key::Enter] {
            let mut d = sizes();
            assert_eq!(
                d.handle_key(f, &key(k), VIEW),
                (Some(DropdownEvent::Opened), true),
                "{k:?}"
            );
            assert!(d.is_open());
        }
        let mut d = sizes();
        let mut alt_down = key(Key::Down);
        alt_down.modifiers.alt = true;
        assert_eq!(
            d.handle_key(f, &alt_down, VIEW),
            (Some(DropdownEvent::Opened), true)
        );
        let mut d = sizes();
        assert_eq!(d.handle_key(f, &key(Key::Tab), VIEW), (None, false));
        let mut ctrl = key(Key::Down);
        ctrl.modifiers.ctrl = true;
        assert_eq!(d.handle_key(f, &ctrl, VIEW), (None, false));
        assert_eq!(d.selected(), Some(2));
    }

    /// Open, the arrows move the highlight, Enter chooses it and closes,
    /// Escape closes and changes nothing, Tab closes and is passed on.
    #[test]
    fn open_the_keys_move_the_highlight_and_choose() {
        let f = field();
        let mut d = sizes();
        d.handle_key(f, &key(Key::Enter), VIEW);
        d.handle_key(f, &key(Key::Down), VIEW);
        assert_eq!(d.highlighted(), Some(3));
        assert_eq!(
            d.selected(),
            Some(2),
            "moving the highlight does not choose"
        );
        assert_eq!(
            d.handle_key(f, &key(Key::Enter), VIEW),
            (Some(DropdownEvent::Selected(3)), true)
        );
        assert!(!d.is_open());

        d.handle_key(f, &key(Key::Enter), VIEW);
        d.handle_key(f, &key(Key::Down), VIEW);
        assert_eq!(
            d.handle_key(f, &key(Key::Escape), VIEW),
            (Some(DropdownEvent::Closed), true)
        );
        assert_eq!(d.selected(), Some(3));

        d.handle_key(f, &key(Key::Enter), VIEW);
        assert_eq!(
            d.handle_key(f, &key(Key::Tab), VIEW),
            (Some(DropdownEvent::Closed), false)
        );
        assert!(!d.is_open());

        d.handle_key(f, &key(Key::Enter), VIEW);
        d.handle_key(f, &key(Key::Up), VIEW);
        assert_eq!(
            d.handle_key(f, &key(Key::Space), VIEW),
            (Some(DropdownEvent::Selected(2)), true)
        );
    }

    /// The field shows the choice, or the placeholder in the dimmer ink; the
    /// chevron is two strokes; focus rings it; disabled dims it all.
    #[test]
    fn the_field_draws_the_choice_or_the_placeholder() {
        let p = Palette::for_mode(false);
        let text_of = |d: &Dropdown| -> Option<(String, Color)> {
            let mut cmds = Vec::new();
            d.draw(&mut cmds, &p, field(), State::default(), 2.0);
            cmds.iter().find_map(|c| match c {
                RenderCommand::Text { text, color, .. } => Some((text.clone(), *color)),
                _ => None,
            })
        };
        assert_eq!(text_of(&sizes()), Some(("A4".to_string(), p.text)));
        let empty = Dropdown::new(vec!["x".into()], None).with_placeholder("Choose a size");
        assert_eq!(
            text_of(&empty),
            Some(("Choose a size".to_string(), p.subtext0))
        );

        let mut cmds = Vec::new();
        sizes().draw(
            &mut cmds,
            &p,
            field(),
            State {
                focused: true,
                ..State::default()
            },
            2.0,
        );
        let strokes = cmds
            .iter()
            .filter(|c| matches!(c, RenderCommand::Line { .. }))
            .count();
        assert_eq!(strokes, 2, "the chevron");
        assert!(
            matches!(cmds.last(), Some(RenderCommand::StrokeRect { color, .. }) if *color == p.accent)
        );

        let mut dim = Vec::new();
        sizes().draw(
            &mut dim,
            &p,
            field(),
            State {
                disabled: true,
                focused: true,
                hovered: false,
            },
            2.0,
        );
        assert!(
            !dim.iter().any(
                |c| matches!(c, RenderCommand::StrokeRect { color, .. } if *color == p.accent)
            )
        );
    }

    /// Out-of-range choices are no choice, and an empty drop-down takes no
    /// keys but the opening ones.
    #[test]
    fn nonsense_is_no_choice() {
        assert_eq!(Dropdown::new(vec!["a".into()], Some(3)).selected(), None);
        let mut d = Dropdown::new(Vec::new(), None);
        assert_eq!(d.handle_key(field(), &key(Key::Down), VIEW), (None, true));
        assert_eq!(d.handle_key(field(), &typed('x'), VIEW), (None, true));
        d.set_selected(Some(0));
        assert_eq!(d.selected(), None);
    }
}
