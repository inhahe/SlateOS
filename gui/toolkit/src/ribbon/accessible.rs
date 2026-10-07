//! A ribbon as tools see it ([`Accessible`]): its tabs, the front one
//! chosen; the front tab's groups, each holding its controls -- a button, a
//! toggle (a check, ticked while on), a split button with its arrow, a
//! drop-down (a combo box holding its choice), a gallery (a list of the
//! choices it shows, the chosen one chosen, and its arrow to all of them) --
//! each named by its command, described by why it cannot be used where it
//! cannot, or by its tooltip; a group folded into one button; the `»` of
//! the groups that do not fit; the Quick Access Toolbar; a panel the ribbon
//! has open over the page; a menu it has open, with its rows
//! ([`crate::menu::MenuPart`]); and, while it is open, the dialog for the
//! user's changes ([`Ribbon::open_customize`]): its list of every command,
//! its tree of tabs -- each with its box, ticked while the tab is shown --
//! their groups and their commands, and its buttons. A row of either list is
//! scrolled into it before it is clicked, as the user would; one chosen is
//! clicked, one pressed clicked twice -- a command added, on the left, or
//! taken out of its group, on the right.
//!
//! A ribbon keeps no place of its own -- its host lays it out each frame --
//! so what implements [`Accessible`] is the ribbon with where its host puts
//! it, [`RibbonAccess`], as a drop-down has [`crate::dropdown::DropdownAccess`].
//! Boxes are in the window's space, where [`Ribbon::layout`] puts them.
//!
//! A part is used as the user uses it: pressed at its middle, and let go of
//! there -- a face does its command on the release, as every button does; an
//! arrow opens its menu on the press -- through the ribbon's own
//! [`Ribbon::handle_mouse`]. A part a press would not reach is refused
//! (`Hidden`): one under an open menu, which takes every press; one outside
//! an open panel, where a press only closes the panel; one the ribbon's own
//! hit test does not find under its middle. A control whose command cannot
//! be used now is refused (`Disabled`). What the ribbon reports -- a
//! command, a toggle, a menu row, a choice, a tab brought to the front -- is
//! the tool's answer.

use super::customize::{self, Button, DialogSlots, Focus, TreeRow};
use super::{
    BodySlots, CommandId, Control, ControlSlot, Hit, Part, Ribbon, RibbonEvent, RibbonLayout,
};
use crate::event::{MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::menu::MenuPart;
use crate::widget::CheckState;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a ribbon, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RibbonPart {
    /// The ribbon.
    Ribbon,
    /// Its strip of tabs.
    Tabs,
    /// A tab, by its place in [`Ribbon::tabs`].
    Tab(usize),
    /// A group of the front tab, by its place in the tab.
    Group(usize),
    /// A group of the front tab folded into one button, by its place.
    Folded(usize),
    /// The `»` button holding the groups there is no room for.
    Overflow,
    /// A panel the ribbon has open over the page: a folded group whole, or
    /// a minimized ribbon's front tab.
    Panel,
    /// A control, by its command: a button's, a toggle's or a split
    /// button's face; a drop-down's box; a gallery.
    Command(CommandId),
    /// What opens a control's menu: a split button's arrow, a drop-down's,
    /// a gallery's.
    Arrow(CommandId),
    /// A gallery's choice shown in its group, by its place in the choices.
    Choice(CommandId, usize),
    /// The Quick Access Toolbar.
    Qat,
    /// A Quick Access Toolbar button, by its place on the toolbar.
    QatButton(usize),
    /// A part of the menu the ribbon has open.
    Menu(MenuPart),
    /// The dialog for the user's changes, while it is open
    /// ([`Ribbon::open_customize`]).
    Customize,
    /// Its list of every command, on the left.
    CustomizeCommands,
    /// A command in that list.
    CustomizeCommand(CommandId),
    /// Its tree of tabs, their groups and their commands, on the right.
    CustomizeTree,
    /// A tab's row in the tree, by its place in [`Ribbon::tabs`].
    CustomizeTab(usize),
    /// A tab's box in the tree, ticked while the tab is shown.
    CustomizeTabShown(usize),
    /// A group's row in the tree: its tab's place, and its own in the tab.
    CustomizeGroup(usize, usize),
    /// A command's row in a group in the tree: its tab's place, its group's,
    /// and the command.
    CustomizeEntry(usize, usize, CommandId),
    /// One of the dialog's buttons.
    CustomizeButton(Button),
}

/// A ribbon with where its host lays it out: what serves a tool.
pub struct RibbonAccess<'a> {
    /// The ribbon.
    pub ribbon: &'a mut Ribbon,
    /// Its top-left corner, in the window.
    pub x: f32,
    /// Its top-left corner, in the window.
    pub y: f32,
    /// Its width: the window's, as a rule.
    pub width: f32,
    /// The window's size, inside which its menus and panels open.
    pub viewport: (f32, f32),
}

/// How a control is pressed by a tool, from what kind it is.
fn role_of(control: &Control) -> Role {
    match control {
        Control::Button { .. } | Control::Split { .. } => Role::Button,
        Control::Toggle { .. } => Role::CheckBox,
        Control::Dropdown { .. } => Role::ComboBox,
        Control::Gallery { .. } => Role::List,
    }
}

impl RibbonAccess<'_> {
    /// Where the ribbon is now.
    fn layout(&self) -> RibbonLayout {
        self.ribbon
            .layout(self.x, self.y, self.width, self.viewport)
    }

    /// The ribbon's own press of the left button at `(x, y)`, and its let
    /// go there when `release`: what it reported, the release's first.
    fn press_at(&mut self, (x, y): (f32, f32), release: bool) -> Option<RibbonEvent> {
        let layout = self.layout();
        let pressed = self.ribbon.handle_mouse(
            &layout,
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::Press(MouseButton::Left),
            },
        );
        let released = if release {
            let layout = self.layout();
            self.ribbon.handle_mouse(
                &layout,
                &MouseEvent {
                    x,
                    y,
                    kind: MouseEventKind::Release(MouseButton::Left),
                },
            )
        } else {
            RibbonEvent::Handled
        };
        [released, pressed]
            .into_iter()
            .find(|said| !matches!(said, RibbonEvent::Handled | RibbonEvent::Ignored))
    }

    /// Where the ribbon draws `part`, and what its hit test calls the point
    /// in its middle, while it is on show.
    fn place(&self, layout: &RibbonLayout, part: RibbonPart) -> Option<(Rect, Hit)> {
        let bodies = layout.body.iter().chain(layout.panel.iter());
        match part {
            RibbonPart::Tab(tab) => layout
                .tabs
                .iter()
                .find(|slot| slot.tab == tab)
                .map(|slot| (slot.rect, Hit::Tab(tab))),
            RibbonPart::Folded(group) => bodies
                .flat_map(|body| &body.groups)
                .find(|slot| slot.group == group)
                .and_then(|slot| slot.button)
                .map(|rect| (rect, Hit::Folded(group))),
            RibbonPart::Overflow => layout
                .body
                .as_ref()
                .and_then(|body| body.overflow.as_ref())
                .map(|overflow| (overflow.rect, Hit::Overflow)),
            RibbonPart::QatButton(index) => layout
                .qat
                .as_ref()
                .and_then(|qat| qat.buttons.iter().find(|(at, _)| *at == index))
                .map(|(_, rect)| (*rect, Hit::Qat(index))),
            RibbonPart::Command(id) | RibbonPart::Arrow(id) | RibbonPart::Choice(id, _) => {
                let front = self.ribbon.front()?;
                let tab = self.ribbon.tabs().get(front)?;
                bodies.flat_map(|body| &body.groups).find_map(|slot| {
                    let group = tab.groups.get(slot.group)?;
                    slot.controls.iter().find_map(|at| {
                        if group.controls.get(at.control)?.command().id != id {
                            return None;
                        }
                        let (rect, part) = match part {
                            RibbonPart::Arrow(_) => (at.arrow?, Part::Arrow),
                            RibbonPart::Choice(_, choice) => (
                                at.choices
                                    .iter()
                                    .find(|(shown, _)| *shown == choice)
                                    .map(|(_, rect)| *rect)?,
                                Part::Choice(choice),
                            ),
                            _ => (at.face, Part::Face),
                        };
                        Some((
                            rect,
                            Hit::Control {
                                group: slot.group,
                                control: at.control,
                                part,
                            },
                        ))
                    })
                })
            }
            _ => None,
        }
    }

    /// Press `part` as the user does -- at its middle, let go of there when
    /// `release` -- or why a press would not reach it.
    fn press_part(
        &mut self,
        part: RibbonPart,
        release: bool,
    ) -> Result<Option<RibbonEvent>, Refusal> {
        let layout = self.layout();
        let (rect, hit) = self.place(&layout, part).ok_or(Refusal::NoSuchWidget)?;
        let (x, y) = rect.centre();
        // An open menu or dialog takes every press; outside an open panel a
        // press only closes the panel (a tab excepted); and the ribbon's own
        // hit test must find the part where it is drawn.
        let outside_panel = self.ribbon.panel_open()
            && !layout
                .panel
                .as_ref()
                .is_some_and(|panel| panel.rect.contains(x, y))
            && !matches!(hit, Hit::Tab(_));
        if self.ribbon.menu_open()
            || layout.dialog.is_some()
            || outside_panel
            || self.ribbon.hit(&layout, x, y) != Some(hit)
        {
            return Err(Refusal::Hidden);
        }
        Ok(self.press_at((x, y), release))
    }

    /// The node of `control`, laid out at `at`.
    fn control_node(control: &Control, at: &ControlSlot) -> Node<RibbonPart> {
        let command = control.command();
        let mut node = Node::new(
            RibbonPart::Command(command.id),
            role_of(control),
            command.label.clone(),
            match control {
                Control::Dropdown { .. } | Control::Gallery { .. } => at.rect,
                _ => at.face,
            },
        );
        node.enabled = command.enabled;
        node.focusable = command.enabled;
        node.description = command
            .disabled_reason
            .clone()
            .filter(|_| !command.enabled)
            .or_else(|| command.tooltip.clone());
        match control {
            Control::Toggle { on, .. } => {
                node.value = Some(Value::Check(if *on {
                    CheckState::Checked
                } else {
                    CheckState::Unchecked
                }));
            }
            Control::Dropdown {
                choices, selected, ..
            } => {
                node.value = selected
                    .and_then(|chosen| choices.get(chosen))
                    .map(|choice| Value::Text(choice.label.clone()));
            }
            Control::Gallery {
                choices, selected, ..
            } => {
                for (index, rect) in &at.choices {
                    let Some(choice) = choices.get(*index) else {
                        continue;
                    };
                    let mut item = Node::new(
                        RibbonPart::Choice(command.id, *index),
                        Role::ListItem,
                        choice.label.clone(),
                        *rect,
                    );
                    item.value = Some(Value::Chosen(*selected == Some(*index)));
                    item.enabled = command.enabled;
                    node.children.push(item);
                }
            }
            Control::Button { .. } | Control::Split { .. } => {}
        }
        if let Some(arrow) = at.arrow {
            let mut opens = Node::new(
                RibbonPart::Arrow(command.id),
                Role::Button,
                format!("{} options", command.label),
                arrow,
            );
            opens.enabled = command.enabled;
            node.children.push(opens);
        }
        node
    }

    /// The nodes of the groups `body` shows of the front tab.
    fn group_nodes(&self, body: &BodySlots) -> Vec<Node<RibbonPart>> {
        let Some(tab) = self.ribbon.front().and_then(|i| self.ribbon.tabs().get(i)) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for slot in &body.groups {
            let Some(group) = tab.groups.get(slot.group) else {
                continue;
            };
            if let Some(button) = slot.button {
                let mut folded = Node::new(
                    RibbonPart::Folded(slot.group),
                    Role::Button,
                    group.label.clone(),
                    button,
                );
                folded.description = Some("folded".to_owned());
                out.push(folded);
                continue;
            }
            let mut node = Node::new(
                RibbonPart::Group(slot.group),
                Role::Group,
                group.label.clone(),
                slot.rect,
            );
            for at in &slot.controls {
                if let Some(control) = group.controls.get(at.control) {
                    node.children.push(Self::control_node(control, at));
                }
            }
            out.push(node);
        }
        if let Some(overflow) = &body.overflow {
            out.push(Node::new(
                RibbonPart::Overflow,
                Role::Button,
                "More groups",
                overflow.rect,
            ));
        }
        out
    }

    /// Where row `index` of one of the dialog's lists, `list`, is while the
    /// list shows from row `top`: its row, or above or below the list for
    /// one scrolled out of it -- where the user would find it, scrolling.
    fn list_row(list: Rect, index: usize, top: usize) -> Rect {
        let rows = |n: usize| f32::from(u16::try_from(n).unwrap_or(u16::MAX)) * customize::ROW;
        let y = if index >= top {
            list.y + rows(index.saturating_sub(top))
        } else {
            list.y - rows(top.saturating_sub(index))
        };
        Rect::new(list.x, y, list.w, customize::ROW)
    }

    /// The part a row of the dialog's tree is, named by places rather than
    /// ids, as tools name parts.
    fn part_of_row(&self, row: &TreeRow) -> Option<RibbonPart> {
        let tabs = self.ribbon.tabs();
        let tab_at = |id: &str| tabs.iter().position(|tab| tab.id == id);
        let group_at = |tab: usize, id: &str| {
            tabs.get(tab)?
                .groups
                .iter()
                .position(|group| group.id == id)
        };
        Some(match row {
            TreeRow::Tab(tab) => RibbonPart::CustomizeTab(tab_at(tab)?),
            TreeRow::Group(tab, group) => {
                let tab = tab_at(tab)?;
                RibbonPart::CustomizeGroup(tab, group_at(tab, group)?)
            }
            TreeRow::Command(tab, group, id) => {
                let tab = tab_at(tab)?;
                RibbonPart::CustomizeEntry(tab, group_at(tab, group)?, *id)
            }
        })
    }

    /// The row of the dialog's tree `part` is: the inverse of
    /// [`part_of_row`](Self::part_of_row). A tab's box is its tab's row.
    fn row_of(&self, part: RibbonPart) -> Option<TreeRow> {
        let tabs = self.ribbon.tabs();
        match part {
            RibbonPart::CustomizeTab(tab) | RibbonPart::CustomizeTabShown(tab) => {
                Some(TreeRow::Tab(tabs.get(tab)?.id.clone()))
            }
            RibbonPart::CustomizeGroup(tab, group) => {
                let tab = tabs.get(tab)?;
                Some(TreeRow::Group(
                    tab.id.clone(),
                    tab.groups.get(group)?.id.clone(),
                ))
            }
            // Whether the group still holds the command is the tree's to
            // say: a row it does not list is no part.
            RibbonPart::CustomizeEntry(tab, group, id) => {
                let tab = tabs.get(tab)?;
                let group = tab.groups.get(group)?;
                Some(TreeRow::Command(tab.id.clone(), group.id.clone(), id))
            }
            _ => None,
        }
    }

    /// The dialog for the user's changes, while it is open: its list of
    /// commands, every one, the chosen one chosen; its tree, the rows
    /// nested as the list indents them -- a tab's groups under it, its box
    /// ticked while it is shown; a group's commands under it -- the chosen
    /// row chosen; and its buttons, one that cannot act now disabled. The
    /// row the keys move in has the keyboard.
    fn customize_node(&self, slots: &DialogSlots) -> Option<Node<RibbonPart>> {
        let dialog = self.ribbon.customize.as_ref()?;
        let (left_top, right_top) = dialog.tops();
        let mut node = Node::new(
            RibbonPart::Customize,
            Role::Dialog,
            "Customize the ribbon",
            slots.rect,
        );

        let mut commands = Node::new(
            RibbonPart::CustomizeCommands,
            Role::List,
            "Commands",
            slots.commands,
        );
        for (index, command) in self.ribbon.catalog().iter().enumerate() {
            let mut item = Node::new(
                RibbonPart::CustomizeCommand(command.id),
                Role::ListItem,
                command.label.clone(),
                Self::list_row(slots.commands, index, left_top),
            );
            let chosen = dialog.command == Some(command.id);
            item.value = Some(Value::Chosen(chosen));
            item.focused = chosen && dialog.focus == Focus::Commands;
            item.focusable = true;
            commands.children.push(item);
        }
        node.children.push(commands);

        let mut tabs: Vec<Node<RibbonPart>> = Vec::new();
        for (index, (row, label, hidden)) in customize::tree(self.ribbon).into_iter().enumerate() {
            let Some(part) = self.part_of_row(&row) else {
                continue;
            };
            let bounds = Self::list_row(slots.tree, index, right_top);
            let chosen = dialog.chosen.as_ref() == Some(&row);
            let mut item = Node::new(part, Role::TreeItem, label, bounds);
            item.value = Some(Value::Chosen(chosen));
            item.focused = chosen && dialog.focus == Focus::Tabs;
            item.focusable = true;
            match (part, row) {
                (RibbonPart::CustomizeTab(tab), TreeRow::Tab(_)) => {
                    let mut shown = Node::new(
                        RibbonPart::CustomizeTabShown(tab),
                        Role::CheckBox,
                        "Shown",
                        customize::tab_check(bounds, 0.0),
                    );
                    shown.value = Some(Value::Check(if hidden {
                        CheckState::Unchecked
                    } else {
                        CheckState::Checked
                    }));
                    item.children.push(shown);
                    tabs.push(item);
                }
                (_, TreeRow::Group(..)) => {
                    if let Some(tab) = tabs.last_mut() {
                        tab.children.push(item);
                    }
                }
                (_, TreeRow::Command(..)) => {
                    let group = tabs.last_mut().and_then(|tab| {
                        tab.children
                            .iter_mut()
                            .rev()
                            .find(|child| matches!(child.id, RibbonPart::CustomizeGroup(..)))
                    });
                    if let Some(group) = group {
                        group.children.push(item);
                    }
                }
                _ => {}
            }
        }
        let mut tree = Node::new(RibbonPart::CustomizeTree, Role::Tree, "Tabs", slots.tree);
        tree.children = tabs;
        node.children.push(tree);

        for (button, rect) in &slots.buttons {
            let mut node_of = Node::new(
                RibbonPart::CustomizeButton(*button),
                Role::Button,
                button.label(),
                *rect,
            );
            node_of.enabled = customize::enabled(self.ribbon, dialog, *button);
            node.children.push(node_of);
        }
        Some(node)
    }

    /// Scroll the dialog's list -- the commands', where `commands`, else the
    /// tree -- so its row `index` shows, as the user would before clicking
    /// it, and answer where the row is now. A list too short to show a row
    /// shows none of them (`Hidden`).
    fn reveal_row(&mut self, commands: bool, index: usize) -> Result<Rect, Refusal> {
        let layout = self.layout();
        let slots = layout.dialog.as_ref().ok_or(Refusal::NoSuchWidget)?;
        let list = if commands { slots.commands } else { slots.tree };
        let shown = customize::rows_in(list.h);
        if shown == 0 {
            return Err(Refusal::Hidden);
        }
        self.ribbon.reveal_customize_row(commands, index, shown);
        let dialog = self
            .ribbon
            .customize
            .as_ref()
            .ok_or(Refusal::NoSuchWidget)?;
        let (left_top, right_top) = dialog.tops();
        Ok(Self::list_row(
            list,
            index,
            if commands { left_top } else { right_top },
        ))
    }

    /// The second press of a double click at `(x, y)`, and its let go:
    /// what the ribbon reported.
    fn double_click_at(&mut self, (x, y): (f32, f32)) -> Option<RibbonEvent> {
        let layout = self.layout();
        let doubled = self.ribbon.handle_mouse(
            &layout,
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::DoubleClick(MouseButton::Left),
            },
        );
        let layout = self.layout();
        let released = self.ribbon.handle_mouse(
            &layout,
            &MouseEvent {
                x,
                y,
                kind: MouseEventKind::Release(MouseButton::Left),
            },
        );
        [doubled, released]
            .into_iter()
            .find(|said| !matches!(said, RibbonEvent::Handled | RibbonEvent::Ignored))
    }

    /// Do `action` to `part` of the open dialog, as the user would: a row
    /// chosen with a click on it, scrolled into its list first if it was out
    /// of it; one pressed clicked twice, as a double click adds a command,
    /// on the left, or takes it out of its group, on the right; a tab's box
    /// toggled with a click on it; a button pressed as clicked, refused while
    /// it cannot act.
    fn invoke_customize(
        &mut self,
        part: RibbonPart,
        action: &Action,
    ) -> Result<Option<RibbonEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        let dialog = self
            .ribbon
            .customize
            .as_ref()
            .ok_or(Refusal::NoSuchWidget)?;
        match (part, action) {
            (RibbonPart::CustomizeButton(button), Action::Press) => {
                if !customize::enabled(self.ribbon, dialog, button) {
                    return Err(Refusal::Disabled);
                }
                let layout = self.layout();
                let slots = layout.dialog.as_ref().ok_or(Refusal::NoSuchWidget)?;
                let (_, rect) = slots
                    .buttons
                    .iter()
                    .find(|(shown, _)| *shown == button)
                    .ok_or(Refusal::NoSuchWidget)?;
                Ok(self.press_at(rect.centre(), true))
            }
            (RibbonPart::CustomizeButton(_), _) => Err(not_for(Role::Button)),
            (RibbonPart::CustomizeCommand(id), Action::Choose | Action::Focus | Action::Press) => {
                let index = self
                    .ribbon
                    .catalog()
                    .iter()
                    .position(|command| command.id == id)
                    .ok_or(Refusal::NoSuchWidget)?;
                let at = self.reveal_row(true, index)?.centre();
                let chosen = self.press_at(at, true);
                if *action == Action::Press {
                    return Ok(self.double_click_at(at).or(chosen));
                }
                Ok(chosen)
            }
            (RibbonPart::CustomizeCommand(_), _) => Err(not_for(Role::ListItem)),
            (
                RibbonPart::CustomizeTab(_)
                | RibbonPart::CustomizeGroup(..)
                | RibbonPart::CustomizeEntry(..),
                Action::Choose | Action::Focus,
            )
            | (RibbonPart::CustomizeEntry(..), Action::Press) => {
                let row = self.row_of(part).ok_or(Refusal::NoSuchWidget)?;
                let index = customize::tree(self.ribbon)
                    .iter()
                    .position(|(shown, _, _)| *shown == row)
                    .ok_or(Refusal::NoSuchWidget)?;
                let at = self.reveal_row(false, index)?.centre();
                let chosen = self.press_at(at, true);
                if *action == Action::Press {
                    return Ok(self.double_click_at(at).or(chosen));
                }
                Ok(chosen)
            }
            (RibbonPart::CustomizeTab(_), Action::Toggle)
            | (RibbonPart::CustomizeTabShown(_), Action::Toggle | Action::Press) => {
                let row = self.row_of(part).ok_or(Refusal::NoSuchWidget)?;
                let index = customize::tree(self.ribbon)
                    .iter()
                    .position(|(shown, _, _)| *shown == row)
                    .ok_or(Refusal::NoSuchWidget)?;
                let at = customize::tab_check(self.reveal_row(false, index)?, 0.0).centre();
                Ok(self.press_at(at, true))
            }
            (RibbonPart::CustomizeTabShown(_), _) => Err(not_for(Role::CheckBox)),
            (
                RibbonPart::CustomizeTab(_)
                | RibbonPart::CustomizeGroup(..)
                | RibbonPart::CustomizeEntry(..),
                _,
            ) => Err(not_for(Role::TreeItem)),
            (RibbonPart::CustomizeCommands, _) => Err(not_for(Role::List)),
            (RibbonPart::CustomizeTree, _) => Err(not_for(Role::Tree)),
            _ => Err(not_for(Role::Dialog)),
        }
    }
}

impl Accessible for RibbonAccess<'_> {
    type Part = RibbonPart;
    type Event = RibbonEvent;

    fn automation(&self, _width: f32, _height: f32) -> Node<RibbonPart> {
        let layout = self.layout();
        let mut root = Node::new(RibbonPart::Ribbon, Role::Group, "Ribbon", layout.rect);
        let front = self.ribbon.front();
        let mut strip = Node::new(RibbonPart::Tabs, Role::TabList, "Tabs", layout.strip);
        for slot in &layout.tabs {
            let Some(tab) = self.ribbon.tabs().get(slot.tab) else {
                continue;
            };
            let mut node = Node::new(
                RibbonPart::Tab(slot.tab),
                Role::Tab,
                tab.label.clone(),
                slot.rect,
            );
            node.value = Some(Value::Chosen(front == Some(slot.tab)));
            node.focusable = true;
            strip.children.push(node);
        }
        root.children.push(strip);
        if let Some(qat) = &layout.qat {
            let mut bar = Node::new(
                RibbonPart::Qat,
                Role::Group,
                "Quick Access Toolbar",
                qat.rect,
            );
            for (index, rect) in &qat.buttons {
                let Some(control) = self
                    .ribbon
                    .qat()
                    .get(*index)
                    .and_then(|id| self.ribbon.control(*id))
                else {
                    continue;
                };
                let command = control.command();
                let mut button = Node::new(
                    RibbonPart::QatButton(*index),
                    role_of(control),
                    command.label.clone(),
                    *rect,
                );
                button.enabled = command.enabled;
                if let Control::Toggle { on, .. } = control {
                    button.value = Some(Value::Check(if *on {
                        CheckState::Checked
                    } else {
                        CheckState::Unchecked
                    }));
                }
                bar.children.push(button);
            }
            root.children.push(bar);
        }
        if let Some(body) = &layout.body {
            root.children.extend(self.group_nodes(body));
        }
        if let Some(panel) = &layout.panel {
            let mut node = Node::new(RibbonPart::Panel, Role::Group, "Panel", panel.rect);
            node.children = self.group_nodes(panel);
            root.children.push(node);
        }
        if let Some(open) = &self.ribbon.menu {
            root.children.push(
                open.menu
                    .automation(self.viewport.0, self.viewport.1)
                    .map(&RibbonPart::Menu),
            );
        }
        // Last, as it is drawn over everything.
        if let Some(dialog) = layout
            .dialog
            .as_ref()
            .and_then(|slots| self.customize_node(slots))
        {
            root.children.push(dialog);
        }
        root
    }

    fn invoke(
        &mut self,
        part: &RibbonPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<RibbonEvent>, Refusal> {
        if matches!(
            *part,
            RibbonPart::Customize
                | RibbonPart::CustomizeCommands
                | RibbonPart::CustomizeCommand(_)
                | RibbonPart::CustomizeTree
                | RibbonPart::CustomizeTab(_)
                | RibbonPart::CustomizeTabShown(_)
                | RibbonPart::CustomizeGroup(..)
                | RibbonPart::CustomizeEntry(..)
                | RibbonPart::CustomizeButton(_)
        ) {
            return self.invoke_customize(*part, &action);
        }
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        let control = match *part {
            RibbonPart::Command(id) | RibbonPart::Arrow(id) | RibbonPart::Choice(id, _) => Some(
                self.ribbon
                    .control(id)
                    .ok_or(Refusal::NoSuchWidget)?
                    .clone(),
            ),
            _ => None,
        };
        if control.as_ref().is_some_and(|c| !c.command().enabled) {
            return Err(Refusal::Disabled);
        }
        match (*part, &action, &control) {
            (RibbonPart::Tab(_), Action::Press | Action::Choose, _) => self.press_part(*part, true),
            (
                RibbonPart::Command(_),
                Action::Press,
                Some(Control::Button { .. } | Control::Split { .. }),
            )
            | (
                RibbonPart::Command(_),
                Action::Press | Action::Toggle,
                Some(Control::Toggle { .. }),
            )
            | (RibbonPart::Choice(..), Action::Press | Action::Choose, _)
            | (RibbonPart::QatButton(_), Action::Press, _) => self.press_part(*part, true),
            // A drop-down opens its list from its arrow, as a press on the
            // box does.
            (RibbonPart::Command(id), Action::Press, Some(Control::Dropdown { .. })) => {
                self.press_part(RibbonPart::Arrow(id), false)
            }
            (
                RibbonPart::Arrow(_) | RibbonPart::Folded(_) | RibbonPart::Overflow,
                Action::Press,
                _,
            ) => self.press_part(*part, false),
            (RibbonPart::Menu(MenuPart::Item(id)), Action::Press | Action::Toggle, _) => {
                let open = self.ribbon.menu.as_mut().ok_or(Refusal::NoSuchWidget)?;
                let at = open.menu.press_point(id)?;
                Ok(self.press_at(at, false))
            }
            (RibbonPart::Command(_), _, Some(control)) => Err(not_for(role_of(control))),
            (RibbonPart::Tab(_), _, _) => Err(not_for(Role::Tab)),
            (RibbonPart::Choice(..), _, _) => Err(not_for(Role::ListItem)),
            (RibbonPart::Menu(MenuPart::Item(_)), _, _) => Err(not_for(Role::MenuItem)),
            (RibbonPart::Menu(_), _, _) => Err(not_for(Role::Menu)),
            (RibbonPart::Tabs, _, _) => Err(not_for(Role::TabList)),
            (
                RibbonPart::Arrow(_)
                | RibbonPart::Folded(_)
                | RibbonPart::Overflow
                | RibbonPart::QatButton(_),
                _,
                _,
            ) => Err(not_for(Role::Button)),
            _ => Err(not_for(Role::Group)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
