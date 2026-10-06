//! The code view as tools see it ([`Accessible`]): the code, a text area
//! holding all of it, and -- while it is open -- the find bar over it: what
//! to find and what to replace it with, each a field, its three switches,
//! each a check box, and what it says of the matches.
//!
//! # As its user would
//!
//! The code set is a paste over all of it: one edit, which Ctrl+Z takes
//! back, and none where it already says so. The code given the keyboard is
//! a click on it -- the keyboard leaves the find bar -- and a field given
//! it is a click in that field. What to find set is typed: the view moves
//! to the first match from where the caret was when the bar opened, as it
//! does while typing; what to replace it with set is typed too, and moves
//! nothing, as typing there does not. A switch pressed is clicked, and the
//! search runs again. The code scrolled is the scrollbar's thumb dragged:
//! down by whole lines, as far as the last screenful -- and, where long
//! lines are not wrapped, across, as far as the widest goes.
//!
//! The bar opens and closes by its keys alone (Ctrl+F, Ctrl+H, Escape),
//! and steps through its matches and replaces them by them too, so none of
//! that is a part: a tool has no more of it than a pointer does. While the
//! bar is closed its parts are none.
//!
//! # The keyboard
//!
//! Whether the view has it is its host's to say ([`CodeView::set_focused`]);
//! within it, the code or the bar has it, as the view says. A host that
//! keeps which of its parts has the keyboard moves it to the view for a
//! tool's focus, as it does for a click.

use super::findbar::{BarAction, Field, Switch};
use super::{CodeView, CodeViewEvent};
use crate::codeedit::CodeEditor;
use crate::frame::Rect;
use crate::widget::CheckState;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a code view, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodePart {
    /// The view: the code, and the find bar over it while it is open.
    View,
    /// The code.
    Code,
    /// The find bar, while it is open.
    FindBar,
    /// The find bar's field of what to find.
    Find,
    /// Its field of what to replace a match with, while that row shows.
    Replace,
    /// Its switch for matching case exactly.
    MatchCase,
    /// Its switch for matching whole words only.
    WholeWord,
    /// Its switch for reading what to find as a regular expression.
    Regex,
    /// What it says of the matches: how many, which one the selection is
    /// on, or why what to find is no pattern -- while it says anything.
    Matches,
}

impl CodePart {
    /// The switch it is, if it is one.
    const fn switch(self) -> Option<Switch> {
        match self {
            Self::MatchCase => Some(Switch::Case),
            Self::WholeWord => Some(Switch::Word),
            Self::Regex => Some(Switch::Regex),
            _ => None,
        }
    }

    /// Whether it is a part of the find bar.
    const fn in_bar(self) -> bool {
        !matches!(self, Self::View | Self::Code)
    }
}

/// A switch's part and name.
const fn switch_part(switch: Switch) -> (CodePart, &'static str) {
    match switch {
        Switch::Case => (CodePart::MatchCase, "Match case"),
        Switch::Word => (CodePart::WholeWord, "Whole word"),
        Switch::Regex => (CodePart::Regex, "Regular expression"),
    }
}

impl CodeView {
    /// The find bar's node, its parts under it.
    fn find_bar_node(&self) -> Node<CodePart> {
        let layout = self.find.layout(self.bounds);
        let strip = Rect::new(
            self.bounds.x,
            self.bounds.y,
            self.bounds.w,
            self.find.height(),
        );
        let mut bar = Node::new(CodePart::FindBar, Role::Group, "Find", strip);
        let typing_in =
            |field: Field| self.focused && self.find.focused && self.find.field == field;

        let mut find = Node::new(CodePart::Find, Role::TextField, "Find", layout.find);
        find.value = Some(Value::Text(self.find.find.text().to_owned()));
        find.focused = typing_in(Field::Find);
        find.focusable = true;
        bar.children.push(find);
        if let Some(rect) = layout.replace {
            let mut with = Node::new(CodePart::Replace, Role::TextField, "Replace with", rect);
            with.value = Some(Value::Text(self.find.with.text().to_owned()));
            with.focused = typing_in(Field::Replace);
            with.focusable = true;
            bar.children.push(with);
        }
        for (switch, rect) in layout.switches {
            let (part, name) = switch_part(switch);
            let mut node = Node::new(part, Role::CheckBox, name, rect);
            node.value = Some(Value::Check(if self.find.is_on(switch) {
                CheckState::Checked
            } else {
                CheckState::Unchecked
            }));
            bar.children.push(node);
        }
        if let Some((said, _)) = self.find.status(self.current_match()) {
            bar.children.push(Node::new(
                CodePart::Matches,
                Role::Label,
                said,
                layout.count,
            ));
        }
        bar
    }

    /// Make the code `text`, as a paste over all of it: one edit, and none
    /// where it already is.
    fn set_code(&mut self, text: &str) -> Option<CodeViewEvent> {
        if self.editor.buffer().text() == text {
            return None;
        }
        let event = self.changed(|editor: &mut CodeEditor| {
            editor.select_all();
            if text.is_empty() {
                editor.delete_selected();
            } else {
                editor.paste(text);
            }
        });
        Some(event)
    }

    /// Scroll the code as the scrollbar's thumb drags it -- down to the line
    /// `y` falls on, as far as the last screenful -- and, where long lines
    /// are not wrapped, across to `x`, as far as the widest goes.
    fn scroll_code_to(&mut self, x: f32, y: f32) -> Result<Option<CodeViewEvent>, Refusal> {
        if !x.is_finite() || !y.is_finite() {
            return Err(Refusal::NotANumber);
        }
        let lines = self.editor.buffer().line_count();
        let last = lines.saturating_sub(self.visible_rows());
        self.top_line = ((y.max(0.0) / self.line_height()) as usize).min(last);
        self.top_row = 0;
        if !self.options.wrap {
            let widest = (0..lines)
                .flat_map(|line| self.rows_of(line))
                .map(|row| self.x_in_row(&row, row.range.end))
                .fold(0.0_f32, f32::max);
            self.scroll_x = x.clamp(0.0, (widest - self.text_rect().w).max(0.0));
        }
        Ok(Some(CodeViewEvent::Moved))
    }

    /// Give the keyboard to the find bar's `field`, as a click in it does.
    fn type_in(&mut self, field: Field) -> CodeViewEvent {
        self.find.focused = true;
        self.find.field = field;
        CodeViewEvent::Moved
    }
}

impl Accessible for CodeView {
    type Part = CodePart;
    type Event = CodeViewEvent;

    fn automation(&self, _width: f32, _height: f32) -> Node<CodePart> {
        let mut root = Node::new(CodePart::View, Role::Group, "Code editor", self.bounds);
        if self.find.open {
            root.children.push(self.find_bar_node());
        }
        let mut code = Node::new(
            CodePart::Code,
            Role::TextArea,
            "Code",
            Rect::new(
                self.bounds.x,
                self.rows_top(),
                self.bounds.w,
                self.rows_height(),
            ),
        );
        code.value = Some(Value::Text(self.editor.buffer().text()));
        code.focused = self.focused && !self.find.focused;
        code.focusable = true;
        root.children.push(code);
        root
    }

    fn invoke(
        &mut self,
        part: &CodePart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<CodeViewEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        // The bar's parts are none while it is closed, and the replace
        // field none while its row is hidden.
        let gone = (part.in_bar() && !self.find.open)
            || (*part == CodePart::Replace && !self.find.replace)
            || (*part == CodePart::Matches && self.find.status(self.current_match()).is_none());
        if gone {
            return Err(Refusal::NoSuchWidget);
        }
        match (*part, &action) {
            (
                CodePart::MatchCase | CodePart::WholeWord | CodePart::Regex,
                Action::Toggle | Action::Press,
            ) => {
                if let Some(switch) = part.switch() {
                    self.find.flip(switch);
                }
                Ok(Some(self.act_on_bar(BarAction::Search)))
            }
            (CodePart::MatchCase | CodePart::WholeWord | CodePart::Regex, _) => {
                Err(not_for(Role::CheckBox))
            }
            (CodePart::Code, Action::SetText(text)) => Ok(self.set_code(text)),
            (CodePart::Code, Action::Focus) => {
                self.find.focused = false;
                Ok(Some(CodeViewEvent::Moved))
            }
            (CodePart::Code, Action::ScrollTo { x, y }) => self.scroll_code_to(*x, *y),
            (CodePart::Code, _) => Err(not_for(Role::TextArea)),
            (CodePart::Find, Action::SetText(text)) => {
                if self.find.find.text() == text {
                    return Ok(None);
                }
                self.find.find.set_text(text);
                Ok(Some(self.act_on_bar(BarAction::Search)))
            }
            (CodePart::Replace, Action::SetText(text)) => {
                if self.find.with.text() == text {
                    return Ok(None);
                }
                // A change to the replacement is not a new search.
                self.find.with.set_text(text);
                Ok(Some(CodeViewEvent::Moved))
            }
            (CodePart::Find, Action::Focus) => Ok(Some(self.type_in(Field::Find))),
            (CodePart::Replace, Action::Focus) => Ok(Some(self.type_in(Field::Replace))),
            (CodePart::Find | CodePart::Replace, _) => Err(not_for(Role::TextField)),
            (CodePart::Matches, _) => Err(not_for(Role::Label)),
            (CodePart::View | CodePart::FindBar, _) => Err(not_for(Role::Group)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
