//! A drop-down as tools see it ([`Accessible`]): a combo box, named by the
//! label its host shows beside it, holding the chosen option's label (or,
//! where nothing is chosen, saying the placeholder) -- and, while its list is
//! open, the list: each option by its label, the chosen one chosen, the
//! highlighted one with the keyboard.
//!
//! A drop-down keeps no field of its own -- its host hands it one with every
//! event -- so what implements [`Accessible`] is the drop-down with where its
//! host draws it, [`DropdownAccess`], made for as long as a tool is served,
//! as [`crate::treeview::TreeAccess`] is for a tree. Boxes are in the
//! window's space, where the host draws the field and the list opens.
//!
//! A part is used as the user uses it: the combo box pressed is a click on
//! its field, which opens the list or folds it away; an option chosen is a
//! click on its row -- scrolled into the list first where it is out of it,
//! as the arrow keys bring it. Options are parts only while the list is
//! open, as they are only on screen then: one chosen with the list closed is
//! refused (`Hidden`), and the tool opens the list first, as the user would.
//! A drop-down its host has disabled refuses everything (`Disabled`).

use super::{Dropdown, DropdownEvent};
use crate::event::{MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::menu::MenuPart;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a drop-down, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropdownPart {
    /// The drop-down: its field.
    Field,
    /// Its list, while it is open.
    List,
    /// An option, by its place among them, while the list is open.
    Option(usize),
}

/// A drop-down with where its host draws it: what serves a tool.
pub struct DropdownAccess<'a> {
    /// The drop-down.
    pub dropdown: &'a mut Dropdown,
    /// What it is called: the label its host shows beside it.
    pub name: &'a str,
    /// Where its field is, in the window.
    pub field: Rect,
    /// The window's size, inside which the list opens.
    pub viewport: (f32, f32),
    /// Whether it can be changed now -- `false` where its host draws it
    /// disabled.
    pub enabled: bool,
}

/// The id of the row that stands for option `index` in the open list, as
/// [`Dropdown::open`] numbers them.
fn id_of(index: usize) -> u64 {
    u64::try_from(index).unwrap_or(u64::MAX)
}

impl DropdownAccess<'_> {
    /// The drop-down's own press at `(x, y)`, and what it did.
    fn press_at(&mut self, (x, y): (f32, f32)) -> Option<DropdownEvent> {
        self.dropdown.handle_mouse(
            self.field,
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            },
            self.viewport,
        )
    }
}

impl Accessible for DropdownAccess<'_> {
    type Part = DropdownPart;
    type Event = DropdownEvent;

    fn automation(&self, _width: f32, _height: f32) -> Node<DropdownPart> {
        let dropdown = &*self.dropdown;
        let mut combo = Node::new(DropdownPart::Field, Role::ComboBox, self.name, self.field);
        let chosen = dropdown
            .selected()
            .and_then(|index| dropdown.options().get(index));
        combo.value = chosen.map(|label| Value::Text(label.clone()));
        combo.description = chosen
            .is_none()
            .then(|| dropdown.placeholder.clone())
            .filter(|placeholder| !placeholder.is_empty());
        combo.enabled = self.enabled;
        combo.focusable = self.enabled;
        if dropdown.is_open() {
            // The rows' boxes as the list draws them, scrolled-out ones above
            // or below it: the list is the toolkit's context menu.
            let rows = dropdown.list.automation(self.viewport.0, self.viewport.1);
            let mut list = Node::new(
                DropdownPart::List,
                Role::List,
                self.name,
                dropdown.list_rect().unwrap_or_default(),
            );
            list.enabled = self.enabled;
            for (index, label) in dropdown.options().iter().enumerate() {
                let Some(row) = rows
                    .children
                    .iter()
                    .find(|row| row.id == MenuPart::Item(id_of(index)))
                else {
                    continue;
                };
                let mut option = Node::new(
                    DropdownPart::Option(index),
                    Role::ListItem,
                    label.clone(),
                    row.bounds,
                );
                option.value = Some(Value::Chosen(dropdown.selected() == Some(index)));
                option.focused = dropdown.highlighted() == Some(index);
                option.enabled = self.enabled;
                list.children.push(option);
            }
            combo.children.push(list);
        }
        combo
    }

    fn invoke(
        &mut self,
        part: &DropdownPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DropdownEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (*part, &action) {
            (DropdownPart::Option(index), _) if index >= self.dropdown.options().len() => {
                Err(Refusal::NoSuchWidget)
            }
            (DropdownPart::List | DropdownPart::Option(_), _) if !self.dropdown.is_open() => {
                Err(Refusal::Hidden)
            }
            _ if !self.enabled => Err(Refusal::Disabled),
            (DropdownPart::Field, Action::Press) => Ok(self.press_at(self.field.centre())),
            (DropdownPart::Option(index), Action::Choose | Action::Press) => {
                let at = self.dropdown.list.press_point(id_of(index))?;
                Ok(self.press_at(at))
            }
            (DropdownPart::Field, _) => Err(not_for(Role::ComboBox)),
            (DropdownPart::List, _) => Err(not_for(Role::List)),
            (DropdownPart::Option(_), _) => Err(not_for(Role::ListItem)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
