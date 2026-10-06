//! A rich field as tools see it ([`Accessible`]): the field, a text area
//! holding its text, and -- where its host draws one -- its formatting
//! toolbar: each switch a check box, ticked where the field shows its
//! format, and the size and clearing buttons.
//!
//! A field keeps neither its box nor its keyboard -- its host draws it
//! where it likes and owns its focus and its events -- so what implements
//! [`Accessible`] is the field with what its host draws it as,
//! [`RichInputAccess`]. Boxes are the host's.
//!
//! # As its user would
//!
//! Its text set is a paste of plain text over all of it: one step of its
//! history, in the format typing takes there, and none where it already
//! says so. A toolbar button pressed is the toolbar's own click on it
//! ([`toolbar::apply`]): a switch flips over the selection -- or, with none,
//! for what is typed next -- and a size steps. A field its host has
//! disabled refuses. Its keyboard is its host's to give, as for a click, so
//! a tool's focus is the host's to answer.
//!
//! What it says back is what its keys say ([`KeyEdit`]): `Changed` where
//! the text or its formatting changed, `Handled` where only what typing
//! takes next did.

use super::RichInput;
use super::toolbar::{self, Tool};
use super::{Look, Toggle};
use crate::frame::Rect;
use crate::palette::Palette;
use crate::textinput::KeyEdit;
use crate::widget::CheckState;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a rich field, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichInputPart {
    /// The field and its toolbar together.
    Whole,
    /// The field.
    Field,
    /// Its toolbar, where its host draws one.
    Toolbar,
    /// A button of the toolbar.
    Tool(Tool),
}

/// A rich field with what its host draws it as: what serves a tool.
pub struct RichInputAccess<'a> {
    /// The field.
    pub input: &'a mut RichInput,
    /// What it is called: the label its host shows by it. With none, the
    /// field is called by what it shows while empty.
    pub name: &'a str,
    /// How its host draws it: where, whether it has the keyboard, and what
    /// it shows while empty.
    pub look: Look<'a>,
    /// Where its host draws its toolbar, if it draws one, and in what
    /// palette: the buttons are as wide as its button style makes them.
    pub toolbar: Option<((f32, f32), &'a Palette)>,
    /// The size its text is unless a format says otherwise: what the size
    /// buttons step from.
    pub base: f32,
    /// Whether it can be changed now -- `false` where its host draws it
    /// disabled.
    pub enabled: bool,
}

/// What a tool calls a toolbar button.
const fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Switch(Toggle::Bold) => "Bold",
        Tool::Switch(Toggle::Italic) => "Italic",
        Tool::Switch(Toggle::Underline) => "Underline",
        Tool::Switch(Toggle::Strike) => "Strikethrough",
        Tool::Smaller => "Smaller text",
        Tool::Larger => "Larger text",
        Tool::Clear => "Clear formatting",
    }
}

/// The smallest box holding all of `boxes`; `None` for none.
fn union(boxes: impl Iterator<Item = Rect>) -> Option<Rect> {
    boxes.reduce(|a, b| {
        let (left, top) = (a.x.min(b.x), a.y.min(b.y));
        Rect::new(
            left,
            top,
            a.right().max(b.right()) - left,
            a.bottom().max(b.bottom()) - top,
        )
    })
}

impl RichInputAccess<'_> {
    /// The toolbar's buttons and their boxes, where its host draws one.
    fn tools(&self) -> Vec<(Tool, Rect)> {
        self.toolbar.map_or_else(Vec::new, |(origin, palette)| {
            toolbar::layout(palette, origin)
                .into_iter()
                .map(|(tool, (x, y, w, h))| (tool, Rect::new(x, y, w, h)))
                .collect()
        })
    }

    /// The field's box.
    fn field_box(&self) -> Rect {
        let (x, y, w, h) = self.look.rect;
        Rect::new(x, y, w, h)
    }

    /// The field's node.
    fn field_node(&self) -> Node<RichInputPart> {
        let name = if self.name.is_empty() {
            self.look.placeholder
        } else {
            self.name
        };
        let mut field = Node::new(RichInputPart::Field, Role::TextArea, name, self.field_box());
        field.value = Some(Value::Text(self.input.text().to_owned()));
        field.focused = self.look.focused;
        field.focusable = self.enabled;
        field.enabled = self.enabled;
        field
    }

    /// The toolbar's node, where its host draws one.
    fn toolbar_node(&self) -> Option<Node<RichInputPart>> {
        let tools = self.tools();
        let bounds = union(tools.iter().map(|(_, rect)| *rect))?;
        let mut bar = Node::new(RichInputPart::Toolbar, Role::Group, "Formatting", bounds);
        bar.enabled = self.enabled;
        for (tool, rect) in tools {
            let mut button = match tool {
                Tool::Switch(toggle) => {
                    let mut node = Node::new(
                        RichInputPart::Tool(tool),
                        Role::CheckBox,
                        tool_name(tool),
                        rect,
                    );
                    node.value = Some(Value::Check(if self.input.shown_has(toggle) {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    }));
                    node
                }
                Tool::Smaller | Tool::Larger | Tool::Clear => Node::new(
                    RichInputPart::Tool(tool),
                    Role::Button,
                    tool_name(tool),
                    rect,
                ),
            };
            button.enabled = self.enabled;
            bar.children.push(button);
        }
        Some(bar)
    }

    /// Make the text `text`, as a paste of plain text over all of it.
    fn set_text(&mut self, text: &str) -> Option<KeyEdit> {
        if self.input.text() == text {
            return None;
        }
        self.input.select_all();
        self.input.insert_str(text);
        Some(KeyEdit::Changed)
    }

    /// Press `tool`, as the toolbar's click on it does.
    fn press(&mut self, tool: Tool) -> KeyEdit {
        let before = self.input.doc.clone();
        toolbar::apply(self.input, tool, self.base);
        if self.input.doc == before {
            KeyEdit::Handled
        } else {
            KeyEdit::Changed
        }
    }
}

impl Accessible for RichInputAccess<'_> {
    type Part = RichInputPart;
    type Event = KeyEdit;

    fn automation(&self, _width: f32, _height: f32) -> Node<RichInputPart> {
        let field = self.field_node();
        let bar = self.toolbar_node();
        let bounds = union(
            bar.iter()
                .map(|bar| bar.bounds)
                .chain(core::iter::once(field.bounds)),
        )
        .unwrap_or(field.bounds);
        let mut whole = Node::new(RichInputPart::Whole, Role::Group, self.name, bounds);
        whole.enabled = self.enabled;
        whole.children.extend(bar);
        whole.children.push(field);
        whole
    }

    fn invoke(
        &mut self,
        part: &RichInputPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<KeyEdit>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        let exists = match part {
            RichInputPart::Whole | RichInputPart::Field => true,
            RichInputPart::Toolbar => self.toolbar.is_some(),
            RichInputPart::Tool(tool) => self.tools().iter().any(|(t, _)| t == tool),
        };
        if !exists {
            return Err(Refusal::NoSuchWidget);
        }
        let role = match part {
            RichInputPart::Whole | RichInputPart::Toolbar => Role::Group,
            RichInputPart::Field => Role::TextArea,
            RichInputPart::Tool(Tool::Switch(_)) => Role::CheckBox,
            RichInputPart::Tool(_) => Role::Button,
        };
        let applies = matches!(
            (part, &action),
            (RichInputPart::Field, Action::SetText(_))
                | (
                    RichInputPart::Tool(Tool::Switch(_)),
                    Action::Toggle | Action::Press
                )
                | (RichInputPart::Tool(_), Action::Press)
        );
        if !applies {
            return Err(not_for(role));
        }
        if !self.enabled {
            return Err(Refusal::Disabled);
        }
        Ok(match (part, &action) {
            (RichInputPart::Field, Action::SetText(text)) => self.set_text(text),
            (RichInputPart::Tool(tool), _) => Some(self.press(*tool)),
            _ => return Err(not_for(role)),
        })
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
