//! A menu bar as tools see it ([`Accessible`]): the bar, each menu's title
//! -- named by its label, chosen while its menu is open -- and the open
//! menu: its rows, each named by its label, described by its keys, ticked
//! where it is a check, greyed where it cannot be used, and the submenu a
//! row opened, held by that row.
//!
//! Its boxes are in the bar's own space, as
//! [`MenuBar::handle_mouse_event`] takes a click. A part is pressed as it is
//! clicked, at the middle of where it is drawn -- a row scrolled out of its
//! panel scrolled into it first, as the arrow keys bring it -- and the event
//! that click raises is the tool's answer, taken off the bar's queue so the
//! host acts on it once.

use super::{
    DropdownPanel, MenuBar, MenuBarEntry, MenuBarEvent, MenuItemId, OpenSubmenu, bar_height,
    children_of, resolve_submenu_entries, submenu_panel,
};
use crate::event::{MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::widget::CheckState;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a menu bar, as tools name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuBarPart {
    /// The bar.
    Bar,
    /// A menu's title on the bar, by its place: pressed, its menu opens, or
    /// closes.
    Title(usize),
    /// An open menu, by the path to it: its title's place, then the place of
    /// each row a submenu was opened from.
    Menu(Vec<usize>),
    /// A row that does something -- an action, a check -- by its id.
    Item(MenuItemId),
    /// A row a submenu opens from, by the path to it: its menu's path, then
    /// its own place.
    SubMenu(Vec<usize>),
}

/// An open panel: the path to it, the rows it shows, where it is.
struct Open {
    path: Vec<usize>,
    entries: Vec<MenuBarEntry>,
    panel: DropdownPanel,
}

/// A row's box in its panel, drawn or not: where the panel puts it.
fn row_box(open: &Open, index: usize) -> Option<Rect> {
    let strip = open.panel.strip(&open.entries);
    Some(Rect::new(
        open.panel.x,
        strip.top(index)?,
        open.panel.width,
        strip.height(index)?,
    ))
}

/// The node of the open panel at `depth` of `open` and, held by its rows, of
/// every panel open below it.
fn panel_node(open: &[Open], depth: usize, name: String) -> Option<Node<MenuBarPart>> {
    let here = open.get(depth)?;
    let mut menu = Node::new(
        MenuBarPart::Menu(here.path.clone()),
        Role::Menu,
        name,
        Rect::new(
            here.panel.x,
            here.panel.y,
            here.panel.width,
            here.panel.panel_height,
        ),
    );
    for (index, entry) in here.entries.iter().enumerate() {
        let Some(bounds) = row_box(here, index) else {
            continue;
        };
        let row = match entry {
            MenuBarEntry::Separator => continue,
            MenuBarEntry::Action {
                label,
                shortcut,
                enabled,
                id,
            } => {
                let mut row = Node::new(
                    MenuBarPart::Item(*id),
                    Role::MenuItem,
                    label.clone(),
                    bounds,
                );
                row.description.clone_from(shortcut);
                row.enabled = *enabled;
                row.focusable = *enabled;
                row
            }
            MenuBarEntry::Check { label, checked, id } => {
                let mut row = Node::new(
                    MenuBarPart::Item(*id),
                    Role::MenuItem,
                    label.clone(),
                    bounds,
                );
                row.value = Some(Value::Check(if *checked {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                }));
                row.focusable = true;
                row
            }
            MenuBarEntry::SubMenu { label, .. } => {
                let mut path = here.path.clone();
                path.push(index);
                let mut row = Node::new(
                    MenuBarPart::SubMenu(path.clone()),
                    Role::MenuItem,
                    label.clone(),
                    bounds,
                );
                row.focusable = true;
                let deeper = depth.saturating_add(1);
                if open.get(deeper).is_some_and(|below| below.path == path)
                    && let Some(below) = panel_node(open, deeper, label.clone())
                {
                    row.children.push(below);
                }
                row
            }
        };
        menu.children.push(row);
    }
    Some(menu)
}

impl MenuBar {
    /// The open panels, the menu first, then each submenu open below it.
    fn open_panels(&self) -> Vec<Open> {
        let Some(top) = self.open_index else {
            return Vec::new();
        };
        let entries = children_of(&self.items, top).to_vec();
        let mut path = vec![top];
        let mut out = vec![Open {
            path: path.clone(),
            entries: entries.clone(),
            panel: self.dropdown_panel(top),
        }];
        let mut parent = entries;
        let mut level = self.open_submenu.as_deref();
        while let Some(sub) = level {
            let entries = resolve_submenu_entries(&parent, sub);
            path.push(sub.parent_index);
            out.push(Open {
                path: path.clone(),
                panel: submenu_panel(&entries, sub, self.viewport),
                entries: entries.clone(),
            });
            parent = entries;
            level = sub.child.as_deref();
        }
        out
    }

    /// Where the title of menu `index` is on the bar.
    fn title_box(&self, index: usize) -> Option<Rect> {
        let (x, width, _) = self.label_metrics.get(index)?;
        Some(Rect::new(*x, 0.0, *width, bar_height()))
    }

    /// The open panel holding the row `wanted` names, and the row's place
    /// in it.
    fn find_row(&self, wanted: &MenuBarPart) -> Option<(usize, usize)> {
        self.open_panels()
            .iter()
            .enumerate()
            .find_map(|(depth, open)| {
                open.entries
                    .iter()
                    .enumerate()
                    .find(|(index, entry)| match (wanted, entry) {
                        (
                            MenuBarPart::Item(id),
                            MenuBarEntry::Action { id: row, .. }
                            | MenuBarEntry::Check { id: row, .. },
                        ) => row == id,
                        (MenuBarPart::SubMenu(path), MenuBarEntry::SubMenu { .. }) => path
                            .split_last()
                            .is_some_and(|(last, menu)| *last == *index && menu == open.path),
                        _ => false,
                    })
                    .map(|(index, _)| (depth, index))
            })
    }

    /// Scroll the panel at `depth` so its row `index` shows, as little as
    /// that takes, as the arrow keys do.
    fn reveal_row(&mut self, depth: usize, index: usize) {
        let open = self.open_panels();
        let Some(here) = open.get(depth) else {
            return;
        };
        let scroll = here.panel.scroll_showing(&here.entries, index);
        if depth == 0 {
            self.dropdown_scroll = scroll;
            return;
        }
        let mut level: Option<&mut OpenSubmenu> = self.open_submenu.as_deref_mut();
        for _ in 1..depth {
            level = level.and_then(|sub| sub.child.as_deref_mut());
        }
        if let Some(sub) = level {
            sub.scroll = scroll;
        }
    }

    /// The bar's own press at `(x, y)`, and the event it raised, taken off
    /// the queue for the tool.
    fn press_at(&mut self, (x, y): (f32, f32)) -> Option<MenuBarEvent> {
        let queued = self.events.len();
        let _handled = self.handle_mouse_event(
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            },
            self.viewport,
        );
        let mut raised = self.events.split_off(queued.min(self.events.len()));
        raised.pop()
    }
}

impl Accessible for MenuBar {
    type Part = MenuBarPart;
    type Event = MenuBarEvent;

    fn automation(&self, width: f32, _height: f32) -> Node<MenuBarPart> {
        let mut bar = Node::new(
            MenuBarPart::Bar,
            Role::MenuBar,
            "Menu bar",
            Rect::new(0.0, 0.0, width, bar_height()),
        );
        let open = self.open_panels();
        for (index, (_, _, parsed)) in self.label_metrics.iter().enumerate() {
            let Some(bounds) = self.title_box(index) else {
                continue;
            };
            let mut title = Node::new(
                MenuBarPart::Title(index),
                Role::MenuItem,
                parsed.text.clone(),
                bounds,
            );
            let shown = self.open_index == Some(index);
            title.value = Some(Value::Chosen(shown));
            title.focusable = true;
            if shown && let Some(menu) = panel_node(&open, 0, parsed.text.clone()) {
                title.children.push(menu);
            }
            bar.children.push(title);
        }
        bar
    }

    fn invoke(
        &mut self,
        part: &MenuBarPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<MenuBarEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (part, &action) {
            (MenuBarPart::Title(index), Action::Press) => {
                let at = self.title_box(*index).ok_or(Refusal::NoSuchWidget)?;
                Ok(self.press_at(at.centre()))
            }
            (MenuBarPart::Item(_) | MenuBarPart::SubMenu(_), Action::Press | Action::Toggle) => {
                let (depth, index) = self.find_row(part).ok_or(Refusal::NoSuchWidget)?;
                let open = self.open_panels();
                let entry = open
                    .get(depth)
                    .and_then(|here| here.entries.get(index))
                    .ok_or(Refusal::NoSuchWidget)?;
                match (entry, &action) {
                    (MenuBarEntry::Action { enabled: false, .. }, _) => {
                        return Err(Refusal::Disabled);
                    }
                    (MenuBarEntry::Check { .. }, Action::Toggle | Action::Press)
                    | (MenuBarEntry::Action { .. } | MenuBarEntry::SubMenu { .. }, Action::Press) =>
                        {}
                    _ => return Err(not_for(Role::MenuItem)),
                }
                self.reveal_row(depth, index);
                let open = self.open_panels();
                let at = open
                    .get(depth)
                    .and_then(|here| row_box(here, index))
                    .ok_or(Refusal::NoSuchWidget)?;
                Ok(self.press_at(at.centre()))
            }
            (MenuBarPart::Title(_) | MenuBarPart::Item(_) | MenuBarPart::SubMenu(_), _) => {
                Err(not_for(Role::MenuItem))
            }
            (MenuBarPart::Menu(_), _) => Err(not_for(Role::Menu)),
            (MenuBarPart::Bar, _) => Err(not_for(Role::MenuBar)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
