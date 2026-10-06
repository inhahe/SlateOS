//! A radio group as tools see it ([`Accessible`]): the group, named by its
//! host's label, and each option a radio button -- named by its label, the
//! chosen one chosen, the one the keyboard is on focused.
//!
//! A group keeps neither labels nor places -- its host draws each option
//! where it likes, with [`super::draw`], and hit-tests it with
//! [`super::hit`] -- so what implements [`Accessible`] is the group with its
//! options as the host lays them out, [`RadioAccess`]. Boxes are the host's.
//!
//! An option is used as the user uses it: pressed, it is the group's own
//! [`RadioGroup::click`] -- what the host calls when a click lands on it --
//! which chooses it, or, in a group that allows going back to no choice,
//! clears it when it was chosen; chosen, it is chosen unless it already is,
//! when nothing changes, as choosing is not clearing. A group its host has
//! disabled refuses (`Disabled`).

use super::{RadioEvent, RadioGroup};
use crate::frame::Rect;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a radio group, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioPart {
    /// The group.
    Group,
    /// An option, by its place.
    Option(usize),
}

/// A radio group with its options as its host lays them out: what serves a
/// tool.
pub struct RadioAccess<'a> {
    /// The group.
    pub group: &'a mut RadioGroup,
    /// What the group is called: the label its host shows over it.
    pub name: &'a str,
    /// Each option's label and where its host draws it, in order.
    pub options: &'a [(&'a str, Rect)],
    /// Whether it can be changed now -- `false` where its host draws it
    /// disabled.
    pub enabled: bool,
}

impl Accessible for RadioAccess<'_> {
    type Part = RadioPart;
    type Event = RadioEvent;

    fn automation(&self, _width: f32, _height: f32) -> Node<RadioPart> {
        let bounds = self
            .options
            .iter()
            .map(|(_, rect)| *rect)
            .reduce(|a, b| {
                let (left, top) = (a.x.min(b.x), a.y.min(b.y));
                Rect::new(
                    left,
                    top,
                    a.right().max(b.right()) - left,
                    a.bottom().max(b.bottom()) - top,
                )
            })
            .unwrap_or_default();
        let mut group = Node::new(RadioPart::Group, Role::Group, self.name, bounds);
        group.enabled = self.enabled;
        for (index, (label, rect)) in self.options.iter().enumerate().take(self.group.len()) {
            let mut option = Node::new(RadioPart::Option(index), Role::RadioButton, *label, *rect);
            option.value = Some(Value::Chosen(self.group.selected() == Some(index)));
            option.focused = self.group.focus() == index;
            option.enabled = self.enabled;
            option.focusable = self.enabled;
            group.children.push(option);
        }
        group
    }

    fn invoke(
        &mut self,
        part: &RadioPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<RadioEvent>, Refusal> {
        let RadioPart::Option(index) = *part else {
            return Err(Refusal::NotApplicable {
                role: Role::Group,
                action: action.name(),
            });
        };
        if index >= self.group.len() || index >= self.options.len() {
            return Err(Refusal::NoSuchWidget);
        }
        if !self.enabled {
            return Err(Refusal::Disabled);
        }
        match action {
            // Choosing what is chosen changes nothing: in a group that can
            // go back to no choice, the click would clear it.
            Action::Choose if self.group.selected() == Some(index) => Ok(None),
            Action::Choose | Action::Press => Ok(self.group.click(index)),
            _ => Err(Refusal::NotApplicable {
                role: Role::RadioButton,
                action: action.name(),
            }),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
