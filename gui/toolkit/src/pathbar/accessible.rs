//! A path bar as tools see it ([`Accessible`]): the bar, its address -- the
//! path shown, or the one being typed -- and, showing the path, its crumbs,
//! each named by its folder; typing, the suggestions under it.
//!
//! Its boxes are in the bar's own space, as [`PathBar::handle_mouse_event`]
//! takes a click, and are where the bar was last laid out
//! ([`PathBar::render`], [`PathBar::lay_out`]) -- where a click is answered
//! from. A part is used as the user uses it: a crumb pressed is clicked; the
//! address focused is a press of Ctrl+L; its text set is the whole of it
//! selected and typed over; the address pressed is confirmed, as Enter
//! confirms it; a suggestion chosen is clicked -- scrolled into the list
//! first where it is out of it, as the arrow keys bring it. The events the
//! bar raises for it -- a request for suggestions while typing, a path to go
//! to -- are the tool's answer, every one, taken off the bar's queue for the
//! host to act on as it acts on a drained event.

use super::{DROPDOWN_ITEM_HEIGHT, Mode, PathBar, PathBarEvent};
use crate::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::row_strip::RowStrip;
use crate::text::scaled;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a path bar, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathBarPart {
    /// The bar.
    Bar,
    /// The address: the path shown, or being typed.
    Field,
    /// A crumb of the trail, by the segment it stands for: pressed, the bar
    /// goes to that folder.
    Crumb(usize),
    /// The suggestions under the address while one is being typed.
    Suggestions,
    /// A suggestion under the address while one is being typed, by its place
    /// among those offered.
    Completion(usize),
}

impl PathBarPart {
    /// What the part is, as a tool is told when refused.
    const fn role(self) -> Role {
        match self {
            Self::Bar => Role::Group,
            Self::Field => Role::TextField,
            Self::Crumb(_) => Role::Button,
            Self::Suggestions => Role::List,
            Self::Completion(_) => Role::ListItem,
        }
    }
}

/// A box given as `(x, y, width, height)`.
fn rect_of((x, y, w, h): (f32, f32, f32, f32)) -> Rect {
    Rect::new(x, y, w, h)
}

impl PathBar {
    /// The events raised since `queued` were waiting, taken off the queue.
    fn raised_since(&mut self, queued: usize) -> Option<Vec<PathBarEvent>> {
        let raised = self
            .pending_events
            .split_off(queued.min(self.pending_events.len()));
        (!raised.is_empty()).then_some(raised)
    }

    /// The bar's own key, as the user presses it, typing `text`.
    fn press_key(&mut self, key: Key, ctrl: bool, text: &str) {
        let modifiers = if ctrl {
            Modifiers::ctrl()
        } else {
            Modifiers::NONE
        };
        // Whether the bar took it is nothing a tool answers to.
        let _taken = self.handle_key_event(&KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        });
    }

    /// The bar's own click at `(x, y)`.
    fn click_at(&mut self, (x, y): (f32, f32)) {
        // Whether the bar took it is nothing a tool answers to.
        let _taken = self.handle_mouse_event(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
    }

    /// Where the `index`-th suggestion's row is while the list shows: on
    /// show, or -- scrolled past, or not reached -- above or below the list,
    /// where it would be drawn.
    fn completion_box(&self, index: usize) -> Option<Rect> {
        let (width, height) = self.last_size;
        let (_, top, list_w, _) = self.completions_rect(width, height)?;
        let first = self.completion_rows(top).top(0)?;
        let every = RowStrip::new(
            0.0,
            core::iter::repeat_n(scaled(DROPDOWN_ITEM_HEIGHT), self.completions.len()),
        );
        let scrolled = every.top(self.dropdown_scroll).unwrap_or_default();
        Some(Rect::new(
            0.0,
            first + every.top(index)? - scrolled,
            list_w,
            every.height(index)?,
        ))
    }
}

impl Accessible for PathBar {
    type Part = PathBarPart;
    type Event = Vec<PathBarEvent>;

    fn automation(&self, width: f32, height: f32) -> Node<PathBarPart> {
        let bounds = Rect::new(0.0, 0.0, width, height);
        let mut bar = Node::new(PathBarPart::Bar, Role::Group, "Address bar", bounds);
        let editing = self.mode == Mode::Edit;
        let mut field = Node::new(PathBarPart::Field, Role::TextField, "Address", bounds);
        field.value = Some(Value::Text(if editing {
            self.edit_text.clone()
        } else {
            pathcodec::display_path(&self.path)
        }));
        field.focused = editing;
        field.focusable = true;
        bar.children.push(field);
        if editing {
            if let Some(list) = self.completions_rect(self.last_size.0, self.last_size.1) {
                let mut suggestions = Node::new(
                    PathBarPart::Suggestions,
                    Role::List,
                    "Suggestions",
                    rect_of(list),
                );
                for (index, item) in self.completions.iter().enumerate() {
                    let Some(row) = self.completion_box(index) else {
                        continue;
                    };
                    let mut node = Node::new(
                        PathBarPart::Completion(index),
                        Role::ListItem,
                        item.name.clone(),
                        row,
                    );
                    node.description = item.is_directory.then(|| "folder".to_owned());
                    node.value = Some(Value::Chosen(self.completion_index == Some(index)));
                    suggestions.children.push(node);
                }
                bar.children.push(suggestions);
            }
        } else {
            for hit in &self.crumb_hits {
                let Some(segment) = self.segments.get(hit.segment) else {
                    continue;
                };
                bar.children.push(Node::new(
                    PathBarPart::Crumb(hit.segment),
                    Role::Button,
                    segment.label.clone(),
                    rect_of(hit.rect),
                ));
            }
        }
        bar
    }

    fn invoke(
        &mut self,
        part: &PathBarPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<Vec<PathBarEvent>>, Refusal> {
        let queued = self.pending_events.len();
        match (*part, action) {
            (PathBarPart::Crumb(segment), Action::Press) => {
                // Typing, the bar shows the text and not the trail.
                if self.mode == Mode::Edit {
                    return Err(Refusal::Hidden);
                }
                let rect = self
                    .crumb_hits
                    .iter()
                    .find(|hit| hit.segment == segment)
                    .map(|hit| rect_of(hit.rect))
                    .ok_or(Refusal::NoSuchWidget)?;
                self.click_at(rect.centre());
            }
            (PathBarPart::Field, Action::Focus) => self.press_key(Key::L, true, ""),
            (PathBarPart::Field, Action::SetText(text)) => {
                self.press_key(Key::L, true, "");
                self.press_key(Key::A, true, "");
                if text.is_empty() {
                    self.press_key(Key::Backspace, false, "");
                } else {
                    // The text as one keystroke, as an input method commits
                    // what it composed: a key standing for no key of its own.
                    self.press_key(Key::Unknown(0), false, &text);
                }
            }
            (PathBarPart::Field, Action::Press) => {
                // Nothing typed, there is nothing to confirm.
                if self.mode != Mode::Edit {
                    return Err(Refusal::NotApplicable {
                        role: Role::TextField,
                        action: "press",
                    });
                }
                self.press_key(Key::Enter, false, "");
            }
            (PathBarPart::Completion(index), Action::Choose | Action::Press) => {
                if self.completion_box(index).is_none() {
                    return Err(Refusal::NoSuchWidget);
                }
                self.scroll_completion_into_view(index);
                let row = self.completion_box(index).ok_or(Refusal::NoSuchWidget)?;
                self.click_at(row.centre());
            }
            (part, action) => {
                return Err(Refusal::NotApplicable {
                    role: part.role(),
                    action: action.name(),
                });
            }
        }
        Ok(self.raised_since(queued))
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
