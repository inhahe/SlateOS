//! A tab bar as tools see it ([`Accessible`]): the bar, each tab -- named by
//! its label, chosen while its page shows, said to have unsaved changes --
//! and the close button of each tab that has one.
//!
//! Its boxes are in the bar's own space, as [`TabView::handle_click`] takes
//! a click: the bar's top-left corner is the origin. A tab is pressed as it
//! is clicked, at the middle of where it is drawn -- scrolled into the bar
//! first where it is out of it, as the user would scroll to it.

use super::{TabEvent, TabView, bar_height};
use crate::frame::Rect;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a tab bar, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabPart {
    /// The bar.
    Bar,
    /// A tab, by its id: chosen, its page shows.
    Tab(u64),
    /// A tab's close button, by the tab's id.
    Close(u64),
}

impl TabView {
    /// Scroll the bar, `width` wide, so the tab `id` is in it whole -- as
    /// little as that takes -- and say where it is drawn now, with its close
    /// button's place; `None` for no such tab.
    fn reveal(&mut self, id: u64, width: f32) -> Option<super::TabRect> {
        let place = self
            .tab_rects(0.0, 0.0)
            .into_iter()
            .find(|tab| tab.id == id)?;
        if place.rect.x < 0.0 {
            self.set_scroll_offset(self.scroll_offset() + place.rect.x);
        } else if place.rect.right() > width {
            self.set_scroll_offset(self.scroll_offset() + place.rect.right() - width);
        }
        self.tab_rects(0.0, 0.0)
            .into_iter()
            .find(|tab| tab.id == id)
    }
}

impl Accessible for TabView {
    type Part = TabPart;
    type Event = TabEvent;

    fn automation(&self, width: f32, _height: f32) -> Node<TabPart> {
        let mut bar = Node::new(
            TabPart::Bar,
            Role::TabList,
            "Tabs",
            Rect::new(0.0, 0.0, width, bar_height()),
        );
        for (tab, place) in self.tabs().iter().zip(self.tab_rects(0.0, 0.0)) {
            let mut node = Node::new(
                TabPart::Tab(tab.id),
                Role::Tab,
                tab.label.clone(),
                place.rect,
            );
            let chosen = self.active_id() == Some(tab.id);
            node.value = Some(Value::Chosen(chosen));
            node.focused = chosen;
            node.focusable = true;
            node.description = tab.dirty.then(|| "unsaved changes".to_owned());
            if let Some(close) = place.close {
                node.children.push(Node::new(
                    TabPart::Close(tab.id),
                    Role::Button,
                    "Close",
                    close,
                ));
            }
            bar.children.push(node);
        }
        bar
    }

    fn invoke(
        &mut self,
        part: &TabPart,
        action: Action,
        width: f32,
        _height: f32,
    ) -> Result<Option<TabEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (*part, &action) {
            (TabPart::Tab(id), Action::Press | Action::Choose | Action::Focus) => {
                let place = self.reveal(id, width).ok_or(Refusal::NoSuchWidget)?;
                // Its middle, or nearer its left edge where the close button
                // reaches the middle of a narrow tab: a press on the button
                // would close the tab rather than choose it.
                let (mut x, y) = place.rect.centre();
                if let Some(close) = place.close
                    && x >= close.x
                {
                    x = place.rect.x + (close.x - place.rect.x) / 2.0;
                }
                Ok(self.handle_click(x, y))
            }
            (TabPart::Close(id), Action::Press) => {
                let place = self.reveal(id, width).ok_or(Refusal::NoSuchWidget)?;
                let close = place.close.ok_or(Refusal::NoSuchWidget)?;
                let (x, y) = close.centre();
                Ok(self.handle_click(x, y))
            }
            (TabPart::Tab(_), _) => Err(not_for(Role::Tab)),
            (TabPart::Close(_), _) => Err(not_for(Role::Button)),
            (TabPart::Bar, _) => Err(not_for(Role::TabList)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
