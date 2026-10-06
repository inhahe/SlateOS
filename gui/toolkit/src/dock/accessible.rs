//! A dock as tools see it ([`Accessible`]): each tab group a tab list --
//! each panel's tab named by what it says ([`title_of`]), the one in front
//! chosen, holding its close button -- the front panel's contents a group of
//! their own, for the application to fill; and each divider between them, a
//! separator saying where it stands across its split, from 0 to 1.
//!
//! A dock keeps no area of its own -- its host lays it out in the area it
//! gives it -- and its pointer state lives beside it ([`DockInput`]), so
//! what implements [`Accessible`] is the three together with the panel
//! kinds that name the tabs, [`DockAccess`]. Boxes are in the window's
//! space, where [`Dock::layout`] puts them.
//!
//! A part is used as the user uses it, through [`DockInput`]: a tab pressed
//! and let go of where it is comes to the front; a close button pressed and
//! let go of on it closes its panel; a divider set to a place is dragged
//! there from where it stands. A part the dock's own hit test does not find
//! under its middle is refused (`Hidden`). Moving a panel to another group
//! -- a drag a tool has no action for -- is the application's to offer
//! through [`Dock::apply`].

use super::{Dock, DockEvent, DockHit, DockInput, Layout, PanelId, PanelKind, title_of};
use crate::frame::Rect;
use crate::layout::Axis;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a dock, as tools name it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DockPart {
    /// The dock.
    Dock,
    /// A tab group, by its path from the root.
    Group(Vec<usize>),
    /// A panel's tab.
    Tab(PanelId),
    /// A panel's tab's close button.
    Close(PanelId),
    /// The front panel's contents: the application's.
    Content(PanelId),
    /// A divider: the split it divides, by its path, and which of its
    /// dividers.
    Divider(Vec<usize>, usize),
}

/// A dock, its pointer state, where its host lays it out and what names its
/// panels: what serves a tool.
pub struct DockAccess<'a> {
    /// The dock.
    pub dock: &'a mut Dock,
    /// Its pointer state, as the host feeds it.
    pub input: &'a mut DockInput,
    /// The area it is laid out in, in the window.
    pub area: Rect,
    /// The panel kinds, which name the tabs.
    pub kinds: &'a [PanelKind],
}

/// Where a divider stands across `area`, along the split's `axis`, from 0
/// to 1.
fn standing(axis: Axis, rect: Rect, area: Rect) -> f64 {
    let (at, from, span) = match axis {
        Axis::Horizontal => (rect.x + rect.w / 2.0, area.x, area.w),
        Axis::Vertical => (rect.y + rect.h / 2.0, area.y, area.h),
    };
    if span > 0.0 {
        f64::from(((at - from) / span).clamp(0.0, 1.0))
    } else {
        0.0
    }
}

impl DockAccess<'_> {
    /// Where the dock is now.
    fn layout(&self) -> Layout {
        self.dock.layout(self.area, self.kinds)
    }

    /// Where `part` is drawn, and what the dock's hit test calls its middle.
    fn place(&self, layout: &Layout, part: &DockPart) -> Option<(Rect, DockHit)> {
        match part {
            DockPart::Tab(panel) | DockPart::Close(panel) => {
                layout.groups.iter().find_map(|group| {
                    let (_, held) = self
                        .dock
                        .groups()
                        .into_iter()
                        .find(|(path, _)| *path == group.path)?;
                    let index = held.panels().iter().position(|p| p == panel)?;
                    let tab = group
                        .tabs
                        .iter()
                        .find(|tab| usize::try_from(tab.id).ok() == Some(index))?;
                    Some(if matches!(part, DockPart::Close(_)) {
                        (
                            tab.close?,
                            DockHit::Close {
                                panel: panel.clone(),
                            },
                        )
                    } else {
                        (
                            tab.rect,
                            DockHit::Tab {
                                panel: panel.clone(),
                            },
                        )
                    })
                })
            }
            DockPart::Divider(split, index) => layout
                .dividers
                .iter()
                .find(|d| d.split == *split && d.index == *index)
                .map(|d| {
                    (
                        d.rect,
                        DockHit::Divider {
                            split: split.clone(),
                            index: *index,
                        },
                    )
                }),
            _ => None,
        }
    }

    /// The dock's own press at `(x, y)` and its let go at `to`, dragged
    /// there in between when it is elsewhere: what it reported.
    fn press_at(&mut self, (x, y): (f32, f32), to: (f32, f32)) -> DockEvent {
        let layout = self.layout();
        let mut said = self.input.press(self.dock, &layout, x, y);
        if to != (x, y) {
            let dragged = self.input.drag_to(self.dock, &layout, to.0, to.1);
            if dragged != DockEvent::None {
                said = dragged;
            }
        }
        let layout = self.layout();
        let released = self.input.release(self.dock, &layout, to.0, to.1);
        if released == DockEvent::None {
            said
        } else {
            released
        }
    }

    /// Press `part` at its middle and let go at `to` (or where it was
    /// pressed), or why a press would not reach it.
    fn use_part(
        &mut self,
        part: &DockPart,
        to: Option<(f32, f32)>,
    ) -> Result<Option<DockEvent>, Refusal> {
        let layout = self.layout();
        let (rect, hit) = self.place(&layout, part).ok_or(Refusal::NoSuchWidget)?;
        let (x, y) = rect.centre();
        if self.dock.hit(&layout, x, y) != Some(hit) {
            return Err(Refusal::Hidden);
        }
        let said = self.press_at((x, y), to.unwrap_or((x, y)));
        Ok((said != DockEvent::None).then_some(said))
    }
}

impl Accessible for DockAccess<'_> {
    type Part = DockPart;
    type Event = DockEvent;

    fn automation(&self, _width: f32, _height: f32) -> Node<DockPart> {
        let layout = self.layout();
        let mut root = Node::new(DockPart::Dock, Role::Group, "Dock", layout.area);
        let groups = self.dock.groups();
        for group in &layout.groups {
            let Some((_, held)) = groups.iter().find(|(path, _)| *path == group.path) else {
                continue;
            };
            let mut node = Node::new(
                DockPart::Group(group.path.clone()),
                Role::TabList,
                "Panels",
                group.rect,
            );
            let front = held.active();
            for tab in &group.tabs {
                let Some(panel) = usize::try_from(tab.id)
                    .ok()
                    .and_then(|index| held.panels().get(index))
                else {
                    continue;
                };
                let title = title_of(panel, self.kinds);
                let mut item = Node::new(
                    DockPart::Tab(panel.clone()),
                    Role::Tab,
                    title.clone(),
                    tab.rect,
                );
                item.value = Some(Value::Chosen(front == Some(panel)));
                item.focusable = true;
                // A tab laid out past the end of its bar is not drawn there,
                // and the dock's hit test does not find it: not shown.
                item.shown = tab.rect.x < group.strip.right();
                if let Some(close) = tab.close {
                    let mut button = Node::new(
                        DockPart::Close(panel.clone()),
                        Role::Button,
                        format!("Close {title}"),
                        close,
                    );
                    button.shown = close.x < group.strip.right();
                    item.children.push(button);
                }
                node.children.push(item);
            }
            if let Some(panel) = front {
                node.children.push(Node::new(
                    DockPart::Content(panel.clone()),
                    Role::Group,
                    title_of(panel, self.kinds),
                    group.content,
                ));
            }
            root.children.push(node);
        }
        for divider in &layout.dividers {
            let mut node = Node::new(
                DockPart::Divider(divider.split.clone(), divider.index),
                Role::Separator,
                "Divider",
                divider.rect,
            );
            node.value = Some(Value::Range {
                value: standing(divider.axis, divider.rect, divider.area),
                min: 0.0,
                max: 1.0,
            });
            node.focusable = true;
            root.children.push(node);
        }
        root
    }

    fn invoke(
        &mut self,
        part: &DockPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<DockEvent>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        match (part, &action) {
            (DockPart::Tab(_), Action::Press | Action::Choose)
            | (DockPart::Close(_), Action::Press) => self.use_part(part, None),
            (DockPart::Divider(split, index), Action::SetValue(value)) => {
                if !value.is_finite() {
                    return Err(Refusal::NotANumber);
                }
                let layout = self.layout();
                let divider = layout
                    .dividers
                    .iter()
                    .find(|d| d.split == *split && d.index == *index)
                    .ok_or(Refusal::NoSuchWidget)?;
                // Dragged along its split's axis to the place asked for, as
                // the pointer drags it -- the dock keeps it where a child
                // still has room, as it does a drag's.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a fraction from 0 to 1, clamped first"
                )]
                let share = value.clamp(0.0, 1.0) as f32;
                let (cx, cy) = divider.rect.centre();
                let to = match divider.axis {
                    Axis::Horizontal => (divider.area.x + share * divider.area.w, cy),
                    Axis::Vertical => (cx, divider.area.y + share * divider.area.h),
                };
                self.use_part(part, Some(to))
            }
            (DockPart::Tab(_), _) => Err(not_for(Role::Tab)),
            (DockPart::Close(_), _) => Err(not_for(Role::Button)),
            (DockPart::Divider(..), _) => Err(not_for(Role::Separator)),
            (DockPart::Group(_), _) => Err(not_for(Role::TabList)),
            (DockPart::Dock | DockPart::Content(_), _) => Err(not_for(Role::Group)),
        }
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
