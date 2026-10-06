//! The Run box as tools see it ([`Accessible`]): the box, named by its title
//! and described by what it says it is for; its command line, named by its
//! label and holding the line -- described by why the line did not run,
//! where it did not; while the line has them, the suggestions under it, the
//! one Tab takes chosen; OK, Cancel, Browse... and the frame's close button.
//!
//! Boxes are on the screen, where the box draws its parts and takes its
//! presses. A part is used as the user uses it: a button or a suggestion
//! clicked at its middle; the line's text set by selecting it all (Ctrl+A)
//! and typing over it, which brings the suggestions typing brings; the line
//! pressed as Enter runs it. What the box says it was asked -- to run a
//! line, to browse, to go away -- is the tool's answer, taken off its queue
//! for its host, the shell, to act on as it acts after a click.

use super::{ButtonId, INSTRUCTION, RunDialog, RunDialogEvent, TITLE};
use guitk::event::{Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of the Run box, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunPart {
    /// The box.
    Box,
    /// The command line.
    Field,
    /// The suggestions under the line, while it has them.
    Suggestions,
    /// A suggestion, by its place among them.
    Suggestion(usize),
    /// OK: run, or open, what the line says.
    Ok,
    /// Cancel.
    Cancel,
    /// Browse...: a file chooser, to fill the line from.
    Browse,
    /// The frame's close button.
    Close,
}

impl RunPart {
    /// What the part is, as a tool is told when refused.
    const fn role(self) -> Role {
        match self {
            Self::Box => Role::Dialog,
            Self::Field => Role::TextField,
            Self::Suggestions => Role::List,
            Self::Suggestion(_) => Role::ListItem,
            Self::Ok | Self::Cancel | Self::Browse | Self::Close => Role::Button,
        }
    }
}

impl RunDialog {
    /// Where `part` is on the screen: where the box draws it and takes a
    /// press on it.
    fn part_rect(&self, part: RunPart) -> Option<Rect> {
        let layout = self.layout();
        match part {
            RunPart::Box => Some(layout.outer),
            RunPart::Field => Some(self.field_rect()),
            RunPart::Suggestions => self.suggestion_rows().map(|(list, _)| list),
            RunPart::Suggestion(index) => {
                let (list, rows) = self.suggestion_rows()?;
                Some(Rect::new(
                    list.x,
                    rows.top(index)?,
                    list.w,
                    rows.height(index)?,
                ))
            }
            RunPart::Ok => Some(Self::button_rect(layout.content, ButtonId::Ok)),
            RunPart::Cancel => Some(Self::button_rect(layout.content, ButtonId::Cancel)),
            RunPart::Browse => Some(Self::button_rect(layout.content, ButtonId::Browse)),
            RunPart::Close => layout.close(),
        }
    }

    /// The box's own key, as the user presses it, typing `text`.
    fn press_key(&mut self, key: Key, modifiers: Modifiers, text: &str) {
        // Whether the box took it is nothing a tool answers to.
        let _taken = self.handle_key_event(&KeyEvent {
            key,
            pressed: true,
            modifiers,
            text: text.to_owned(),
        });
    }

    /// The box's own click at the middle of `rect`.
    fn click(&mut self, rect: Rect) {
        let (x, y) = rect.centre();
        // Whether the box took it is nothing a tool answers to.
        let _taken = self.handle_mouse_event(&MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        });
    }
}

impl Accessible for RunDialog {
    type Part = RunPart;
    type Event = Vec<RunDialogEvent>;

    fn automation(&self, _width: f32, _height: f32) -> Node<RunPart> {
        let at = |part| self.part_rect(part).unwrap_or_default();
        let mut dialog = Node::new(RunPart::Box, Role::Dialog, TITLE, at(RunPart::Box));
        dialog.description = Some(INSTRUCTION.to_owned());
        dialog.shown = self.visible;
        let mut field = Node::new(RunPart::Field, Role::TextField, "Open:", at(RunPart::Field));
        field.value = Some(Value::Text(self.input.text().to_owned()));
        field.description.clone_from(&self.error_message);
        // The line has the keyboard whenever the box is up.
        field.focused = self.visible;
        field.focusable = true;
        dialog.children.push(field);
        if let Some((list, rows)) = self.suggestion_rows() {
            let mut suggestions = Node::new(RunPart::Suggestions, Role::List, "Suggestions", list);
            for (index, suggestion) in self.suggestions.iter().enumerate() {
                let (Some(top), Some(height)) = (rows.top(index), rows.height(index)) else {
                    continue;
                };
                let mut row = Node::new(
                    RunPart::Suggestion(index),
                    Role::ListItem,
                    suggestion.text.clone(),
                    Rect::new(list.x, top, list.w, height),
                );
                row.value = Some(Value::Chosen(self.suggestion_index == Some(index)));
                suggestions.children.push(row);
            }
            dialog.children.push(suggestions);
        }
        for (part, name) in [
            (RunPart::Ok, "OK"),
            (RunPart::Cancel, "Cancel"),
            (RunPart::Browse, "Browse..."),
            (RunPart::Close, "Close"),
        ] {
            if let Some(bounds) = self.part_rect(part) {
                dialog
                    .children
                    .push(Node::new(part, Role::Button, name, bounds));
            }
        }
        for child in &mut dialog.children {
            child.shown = self.visible;
            for row in &mut child.children {
                row.shown = self.visible;
            }
        }
        dialog
    }

    fn invoke(
        &mut self,
        part: &RunPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<Vec<RunDialogEvent>>, Refusal> {
        if !self.visible {
            return Err(Refusal::Hidden);
        }
        let queued = self.events.len();
        match (*part, action) {
            (RunPart::Field, Action::SetText(text)) => {
                self.press_key(Key::A, Modifiers::ctrl(), "");
                if text.is_empty() {
                    self.press_key(Key::Backspace, Modifiers::NONE, "");
                } else {
                    // The text as one keystroke, as an input method commits
                    // what it composed: a key standing for no key of its own.
                    self.press_key(Key::Unknown(0), Modifiers::NONE, &text);
                }
            }
            (RunPart::Field, Action::Press) => self.press_key(Key::Enter, Modifiers::NONE, ""),
            // The line has the keyboard whenever the box is up.
            (RunPart::Field, Action::Focus) => {}
            (RunPart::Suggestion(_), Action::Choose | Action::Press)
            | (RunPart::Ok | RunPart::Cancel | RunPart::Browse | RunPart::Close, Action::Press) => {
                let at = self.part_rect(*part).ok_or(Refusal::NoSuchWidget)?;
                self.click(at);
            }
            (part, action) => {
                return Err(Refusal::NotApplicable {
                    role: part.role(),
                    action: action.name(),
                });
            }
        }
        let raised = self.events.split_off(queued.min(self.events.len()));
        Ok((!raised.is_empty()).then_some(raised))
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
