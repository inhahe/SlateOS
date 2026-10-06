//! A context menu as tools see it ([`Accessible`]): the menu and its rows --
//! each named by its label, ticked where it is a check, described by its
//! keys (its shortcut) or, where it is greyed, by why -- and, where one is
//! open, the submenu a row opened, held by that row.
//!
//! A row pressed is clicked at the middle of where it is drawn -- scrolled
//! into the panel first where it is out of it, as the arrow keys bring it --
//! so the menu does what a click does: an action chosen and the menu closed,
//! or a submenu opened. A host that routes clicks to its menus through its
//! own handling, to act on the row chosen -- the desktop shell does -- asks
//! [`ContextMenu::press_point`] where to click instead, and clicks there.

use super::{ContextMenu, MenuAction, MenuItem, MenuItemId};
use crate::frame::Rect;
use crate::widget::CheckState;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a context menu, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuPart {
    /// The menu.
    Menu,
    /// The submenu opened from the row with this id.
    Submenu(MenuItemId),
    /// A row -- an action, or one a submenu opens from -- by its id.
    Item(MenuItemId),
}

impl MenuItem {
    /// Its id, and whether it can be chosen: `None` for a separator.
    const fn id_and_enabled(&self) -> Option<(MenuItemId, bool)> {
        match self {
            Self::Action { id, enabled, .. } | Self::Submenu { id, enabled, .. } => {
                Some((*id, *enabled))
            }
            Self::Separator => None,
        }
    }
}

impl ContextMenu {
    /// The menu as tools see it -- the whole menu, or the submenu opened from
    /// a row, named by the row.
    fn menu_node(&self, part: MenuPart, name: &str) -> Node<MenuPart> {
        let panel = self
            .panel_rect()
            .unwrap_or(Rect::new(self.x, self.y, 0.0, 0.0));
        let mut menu = Node::new(part, Role::Menu, name, panel);
        menu.shown = self.visible;
        let strip = self.strip();
        for (index, item) in self.items.iter().enumerate() {
            let (Some((id, enabled)), Some(top), Some(height)) =
                (item.id_and_enabled(), strip.top(index), strip.height(index))
            else {
                continue;
            };
            // A row scrolled out of the panel has its box above or below it,
            // where it would be drawn.
            let bounds = Rect::new(self.x, top, self.width, height);
            let (MenuItem::Action { label, .. } | MenuItem::Submenu { label, .. }) = item else {
                continue;
            };
            let mut row = Node::new(MenuPart::Item(id), Role::MenuItem, label.clone(), bounds);
            row.enabled = enabled;
            row.focused = self.hover_index == Some(index);
            row.focusable = enabled;
            row.description = self.reason(id).map(str::to_owned);
            match item {
                MenuItem::Action {
                    shortcut, checked, ..
                } => {
                    row.value = checked.map(|on| {
                        Value::Check(if on {
                            CheckState::Checked
                        } else {
                            CheckState::Unchecked
                        })
                    });
                    if row.description.is_none() {
                        row.description.clone_from(shortcut);
                    }
                }
                MenuItem::Submenu { .. } => {
                    if let Some((open, submenu)) = &self.open_submenu
                        && *open == index
                    {
                        row.children
                            .push(submenu.menu_node(MenuPart::Submenu(id), label));
                    }
                }
                MenuItem::Separator => {}
            }
            menu.children.push(row);
        }
        menu
    }

    /// The menu -- this one, or a submenu open below it -- holding the row
    /// `id`, and the row's place in it.
    fn level_of(&mut self, id: MenuItemId) -> Option<(&mut Self, usize)> {
        let own = self
            .items
            .iter()
            .position(|item| item.id_and_enabled().is_some_and(|(row, _)| row == id));
        if let Some(index) = own {
            return Some((self, index));
        }
        let (_, submenu) = self.open_submenu.as_mut()?;
        submenu.level_of(id)
    }

    /// Where a click lands on the row `id`: the middle of where it is drawn,
    /// scrolled into its panel first if it was out of it -- for a host that
    /// routes clicks to the menu itself, and for [`Accessible::invoke`].
    ///
    /// # Errors
    ///
    /// [`Refusal::NoSuchWidget`] for a row the menu does not show -- not in
    /// it, or in a submenu that is not open -- or for a menu not shown;
    /// [`Refusal::Disabled`] for a greyed row; [`Refusal::Hidden`] for a row
    /// a panel too short to hold it cannot show.
    pub fn press_point(&mut self, id: MenuItemId) -> Result<(f32, f32), Refusal> {
        if !self.visible {
            return Err(Refusal::NoSuchWidget);
        }
        let (menu, index) = self.level_of(id).ok_or(Refusal::NoSuchWidget)?;
        let enabled = menu
            .items
            .get(index)
            .and_then(MenuItem::id_and_enabled)
            .is_some_and(|(_, enabled)| enabled);
        if !enabled {
            return Err(Refusal::Disabled);
        }
        menu.scroll_index_into_view(index);
        let rect = menu.item_rect(index).ok_or(Refusal::Hidden)?;
        Ok(rect.centre())
    }

    /// Whether the row `id` is one a menu shows ticked or not: a check.
    fn is_check(&self, id: MenuItemId) -> bool {
        self.items.iter().any(
            |item| matches!(item, MenuItem::Action { id: row, checked: Some(_), .. } if *row == id),
        ) || self
            .open_submenu
            .as_ref()
            .is_some_and(|(_, submenu)| submenu.is_check(id))
    }
}

impl Accessible for ContextMenu {
    type Part = MenuPart;
    type Event = MenuAction;

    fn automation(&self, _width: f32, _height: f32) -> Node<MenuPart> {
        self.menu_node(MenuPart::Menu, "")
    }

    fn invoke(
        &mut self,
        part: &MenuPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<MenuAction>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (*part, &action) {
            (MenuPart::Item(id), Action::Press) => {
                let (x, y) = self.press_point(id)?;
                Ok(self.handle_click(x, y).map(MenuAction::Selected))
            }
            (MenuPart::Item(id), Action::Toggle) if self.is_check(id) => {
                let (x, y) = self.press_point(id)?;
                Ok(self.handle_click(x, y).map(MenuAction::Selected))
            }
            (MenuPart::Item(_), _) => Err(not_for(Role::MenuItem)),
            (MenuPart::Menu | MenuPart::Submenu(_), _) => Err(not_for(Role::Menu)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
