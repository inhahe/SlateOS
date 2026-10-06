//! The dialogs as tools see them ([`Accessible`]): a message box
//! ([`AlertDialog`]) -- named by its title, described by its message and the
//! quieter line under it, each button by its label, the one with the
//! keyboard focused, a destructive one said to be; an input dialog
//! ([`InputDialog`]) -- its field, named by the prompt, whose text a
//! password field never shows tools, and OK and Cancel; a progress dialog
//! ([`ProgressDialog`]) -- its bar, how far the work has gone where it is
//! known, and Cancel where there is one; and a floating dialog
//! ([`NonModalDialog`]) -- its close button, what it holds being its host's
//! to show.
//!
//! Boxes are where a dialog last drew itself, in the space of what it was
//! drawn over, which is where a click is answered from: before its first
//! frame a dialog is on no screen, and has no boxes and is not shown. A part
//! is used as the user uses it -- a button clicked at its middle; the
//! keyboard brought to a part with Tab; a field's text set by clicking into
//! it, selecting it all and typing over it, and confirmed with Enter. What a
//! dialog answers ([`DialogResult`]) is the tool's answer, and stays the
//! dialog's for its host to read, as after a click. A dialog not up --
//! never shown, or answered and on its way out -- refuses (`Hidden`).

use super::{
    AlertDialog, ButtonRole, DialogResult, InputDialog, InputFocus, ModalOverlay, NonModalDialog,
    ProgressDialog, ProgressMode,
};
use crate::event::{Event, Key, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a dialog, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalPart {
    /// The dialog.
    Dialog,
    /// A button of its row, by its place, left to right: a message box's
    /// buttons; an input dialog's OK and Cancel; a progress dialog's Cancel.
    Button(usize),
    /// An input dialog's field.
    Field,
    /// A progress dialog's bar.
    Progress,
    /// A floating dialog's close button.
    Close,
}

impl ModalPart {
    /// What the part is, as a tool is told when refused.
    const fn role(self) -> Role {
        match self {
            Self::Dialog => Role::Dialog,
            Self::Button(_) | Self::Close => Role::Button,
            Self::Field => Role::TextField,
            Self::Progress => Role::ProgressBar,
        }
    }

    /// `action` refused this part, as not done to what it is.
    fn refuses(self, action: &Action) -> Refusal {
        Refusal::NotApplicable {
            role: self.role(),
            action: action.name(),
        }
    }
}

/// A box given as `(x, y, width, height)`.
fn rect_of((x, y, w, h): (f32, f32, f32, f32)) -> Rect {
    Rect::new(x, y, w, h)
}

/// A press of the left button at `(x, y)`.
fn click((x, y): (f32, f32)) -> Event {
    Event::Mouse(MouseEvent {
        x,
        y,
        kind: MouseEventKind::Press(MouseButton::Left),
    })
}

/// `key` pressed with `modifiers` held, typing `text`.
fn key(key: Key, modifiers: Modifiers, text: &str) -> Event {
    Event::Key(KeyEvent {
        key,
        pressed: true,
        modifiers,
        text: text.to_owned(),
    })
}

impl ModalOverlay {
    /// Whether its dialog is up: shown, and not on its way out -- answered,
    /// cancelled, or put away by its host.
    fn up(&self) -> bool {
        self.active && self.target_opacity > 0.0
    }
}

/// A dialog's node: named by its title, described by what it says, at its
/// box as last drawn, shown while it is up and has been drawn.
fn dialog_node(title: &str, said: String, overlay: &ModalOverlay) -> Node<ModalPart> {
    let drawn = overlay.content_rect.map(rect_of);
    let mut node = Node::new(
        ModalPart::Dialog,
        Role::Dialog,
        title,
        drawn.unwrap_or_default(),
    );
    node.description = Some(said);
    node.shown = overlay.up() && drawn.is_some();
    node
}

/// A part's node, inside `dialog`: shown where the dialog is.
fn part_node(
    dialog: &Node<ModalPart>,
    part: ModalPart,
    name: &str,
    drawn: Option<(f32, f32, f32, f32)>,
) -> Node<ModalPart> {
    let mut node = Node::new(
        part,
        part.role(),
        name,
        drawn.map(rect_of).unwrap_or_default(),
    );
    node.shown = dialog.shown;
    node
}

/// What a dialog says: `first`, and under it `then` where there is one.
fn lines(first: &str, then: Option<&str>) -> String {
    match then {
        Some(then) => format!("{first}\n{then}"),
        None => first.to_owned(),
    }
}

impl Accessible for AlertDialog {
    type Part = ModalPart;
    type Event = DialogResult;

    fn automation(&self, _width: f32, _height: f32) -> Node<ModalPart> {
        let mut dialog = dialog_node(
            &self.title,
            lines(&self.message, self.detail.as_deref()),
            &self.overlay,
        );
        for (index, button) in self.buttons.iter().enumerate() {
            let mut node = part_node(
                &dialog,
                ModalPart::Button(index),
                button.label(),
                self.button_rect(index),
            );
            node.focused = index == self.focused_button;
            node.focusable = true;
            node.description =
                (button.role() == ButtonRole::Destructive).then(|| "destructive".to_owned());
            dialog.children.push(node);
        }
        dialog
    }

    fn invoke(
        &mut self,
        part: &ModalPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DialogResult>, Refusal> {
        let ModalPart::Button(index) = *part else {
            return Err(match part {
                ModalPart::Dialog => part.refuses(&action),
                _ => Refusal::NoSuchWidget,
            });
        };
        if index >= self.buttons.len() {
            return Err(Refusal::NoSuchWidget);
        }
        if !self.overlay.up() {
            return Err(Refusal::Hidden);
        }
        match action {
            Action::Press => {
                let at = self.button_rect(index).ok_or(Refusal::Hidden)?;
                // Whether it took the click is what its answer says.
                let _taken = self.handle_event(&click(rect_of(at).centre()));
                Ok(self.result.clone())
            }
            Action::Focus => {
                // Tab steps the keyboard along the row, round to it.
                for _ in 0..self.buttons.len() {
                    if self.focused_button == index {
                        break;
                    }
                    let _taken = self.handle_event(&key(Key::Tab, Modifiers::NONE, ""));
                }
                Ok(None)
            }
            _ => Err(part.refuses(&action)),
        }
    }
}

impl InputDialog {
    /// Where `part` was last drawn, if it is one of the dialog's and the
    /// dialog has been drawn.
    fn drawn(&self, part: ModalPart) -> Option<(f32, f32, f32, f32)> {
        let placed = self.placement.as_ref()?;
        match part {
            ModalPart::Field => Some(placed.field),
            ModalPart::Button(0) => Some(placed.ok),
            ModalPart::Button(1) => Some(placed.cancel),
            _ => None,
        }
    }

    /// What has the keyboard when `part` has it.
    const fn focus_of(part: ModalPart) -> Option<InputFocus> {
        match part {
            ModalPart::Field => Some(InputFocus::TextField),
            ModalPart::Button(0) => Some(InputFocus::OkButton),
            ModalPart::Button(1) => Some(InputFocus::CancelButton),
            _ => None,
        }
    }

    /// The keyboard brought to `wanted` with Tab, as the user steps it
    /// round the field and the two buttons.
    fn tab_to(&mut self, wanted: InputFocus) {
        for _ in 0..3 {
            if self.focused_element == wanted {
                return;
            }
            let _taken = self.handle_event(&key(Key::Tab, Modifiers::NONE, ""));
        }
    }

    /// The field clicked into, unless it has the keyboard already.
    fn click_field(&mut self, field: (f32, f32, f32, f32)) {
        if self.focused_element != InputFocus::TextField {
            let _taken = self.handle_event(&click(rect_of(field).centre()));
        }
    }
}

impl Accessible for InputDialog {
    type Part = ModalPart;
    type Event = DialogResult;

    fn automation(&self, _width: f32, _height: f32) -> Node<ModalPart> {
        let mut dialog = dialog_node(&self.title, self.message.clone(), &self.overlay);
        let mut field = part_node(
            &dialog,
            ModalPart::Field,
            &self.message,
            self.drawn(ModalPart::Field),
        );
        // A password is never shown to tools (`widget::automation`).
        field.value = (!self.password_mode).then(|| Value::Text(self.input_text.clone()));
        // What is wrong with it, where something is; else the hint it shows
        // while empty.
        field.description = self
            .validation_error
            .clone()
            .or_else(|| (!self.placeholder.is_empty()).then(|| self.placeholder.clone()));
        field.focused = self.focused_element == InputFocus::TextField;
        field.focusable = true;
        dialog.children.push(field);
        for (index, label) in ["OK", "Cancel"].into_iter().enumerate() {
            let part = ModalPart::Button(index);
            let mut button = part_node(&dialog, part, label, self.drawn(part));
            button.focused = Self::focus_of(part) == Some(self.focused_element);
            button.focusable = true;
            dialog.children.push(button);
        }
        dialog
    }

    fn invoke(
        &mut self,
        part: &ModalPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DialogResult>, Refusal> {
        let Some(focus) = Self::focus_of(*part) else {
            return Err(match part {
                ModalPart::Dialog => part.refuses(&action),
                _ => Refusal::NoSuchWidget,
            });
        };
        if !self.overlay.up() {
            return Err(Refusal::Hidden);
        }
        let at = self.drawn(*part).ok_or(Refusal::Hidden)?;
        match (*part, action) {
            (_, Action::Focus) => {
                self.tab_to(focus);
                Ok(None)
            }
            (ModalPart::Field, Action::SetText(text)) => {
                self.click_field(at);
                let _home = self.handle_event(&key(Key::Home, Modifiers::NONE, ""));
                let _all = self.handle_event(&key(Key::End, Modifiers::shift(), ""));
                let typed = if text.is_empty() {
                    key(Key::Backspace, Modifiers::NONE, "")
                } else {
                    // The text as one keystroke, as an input method commits
                    // what it composed: a key standing for no key of its own.
                    key(Key::Unknown(0), Modifiers::NONE, &text)
                };
                let _taken = self.handle_event(&typed);
                Ok(None)
            }
            (ModalPart::Field, Action::Press) => {
                // Enter in the field: with the keyboard on Cancel, Enter
                // would press that instead.
                self.click_field(at);
                let _taken = self.handle_event(&key(Key::Enter, Modifiers::NONE, ""));
                Ok(self.result.clone())
            }
            (ModalPart::Button(_), Action::Press) => {
                let _taken = self.handle_event(&click(rect_of(at).centre()));
                Ok(self.result.clone())
            }
            (part, action) => Err(part.refuses(&action)),
        }
    }
}

impl Accessible for ProgressDialog {
    type Part = ModalPart;
    type Event = DialogResult;

    fn automation(&self, _width: f32, _height: f32) -> Node<ModalPart> {
        let detail = self.detail_text.as_deref().filter(|_| self.show_detail);
        let mut dialog = dialog_node(&self.title, lines(&self.status_text, detail), &self.overlay);
        let mut bar = part_node(
            &dialog,
            ModalPart::Progress,
            &self.status_text,
            self.bar_rect,
        );
        // How far, where it is known: a bar that only says work is going on
        // has no value.
        bar.value = match self.progress {
            ProgressMode::Determinate(value) => Some(Value::Progress { value, max: 1.0 }),
            ProgressMode::Indeterminate => None,
        };
        dialog.children.push(bar);
        if self.cancelable {
            dialog.children.push(part_node(
                &dialog,
                ModalPart::Button(0),
                "Cancel",
                self.cancel_rect,
            ));
        }
        dialog
    }

    fn invoke(
        &mut self,
        part: &ModalPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DialogResult>, Refusal> {
        match (*part, action) {
            (ModalPart::Button(0), Action::Press) if self.cancelable => {
                if !self.overlay.up() {
                    return Err(Refusal::Hidden);
                }
                let at = self.cancel_rect.ok_or(Refusal::Hidden)?;
                let _taken = self.handle_event(&click(rect_of(at).centre()));
                Ok(self.cancelled.then_some(DialogResult::Cancel))
            }
            (ModalPart::Button(0), action) if self.cancelable => Err(part.refuses(&action)),
            (ModalPart::Dialog | ModalPart::Progress, action) => Err(part.refuses(&action)),
            _ => Err(Refusal::NoSuchWidget),
        }
    }
}

impl Accessible for NonModalDialog {
    type Part = ModalPart;
    type Event = DialogResult;

    fn automation(&self, _width: f32, _height: f32) -> Node<ModalPart> {
        let mut dialog = Node::new(
            ModalPart::Dialog,
            Role::Dialog,
            self.title.clone(),
            Rect::new(self.x, self.y, self.width, self.height),
        );
        dialog.shown = self.visible;
        let close = part_node(&dialog, ModalPart::Close, "Close", Some(self.close_rect()));
        dialog.children.push(close);
        dialog
    }

    fn invoke(
        &mut self,
        part: &ModalPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DialogResult>, Refusal> {
        match (*part, action) {
            (ModalPart::Close, Action::Press) => {
                if !self.visible {
                    return Err(Refusal::Hidden);
                }
                // Closed, it is put away; there is no answer to give.
                let _taken = self.handle_event(&click(rect_of(self.close_rect()).centre()));
                Ok(None)
            }
            (ModalPart::Close | ModalPart::Dialog, action) => Err(part.refuses(&action)),
            _ => Err(Refusal::NoSuchWidget),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
