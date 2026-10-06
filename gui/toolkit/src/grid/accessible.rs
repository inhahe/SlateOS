//! An icon grid as tools see it ([`Accessible`]): a list of its items, each
//! named by its label, the chosen ones chosen and the one with the keyboard
//! focused -- every item, one scrolled out of sight with its box above or
//! below the grid, where it would be drawn.
//!
//! Boxes are in the grid's own space, as [`GridView::handle_event`] takes a
//! pointer event. An item is used as the user uses it: scrolled into view as
//! the arrow keys bring it, then chosen as a click chooses it, or opened as a
//! double click opens it. The events the grid raises for it -- a selection
//! changed, an item to open -- are the tool's answer, taken off its queue for
//! the host to act on as it acts on a drained event.

use super::{GridEvent, GridView};
use crate::event::{Event, MouseButton, MouseEvent, MouseEventKind};
use crate::frame::Rect;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of an icon grid, as tools name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridPart {
    /// The grid.
    Grid,
    /// An item, by its id ([`super::GridItem::id`]).
    Item(u64),
}

impl GridView {
    /// Where item `index` is drawn in the grid's own space, at its scroll --
    /// above or below the grid for one scrolled out of sight.
    fn item_box(&self, index: usize) -> Option<Rect> {
        if index >= self.items.len() {
            return None;
        }
        let layout = self.layout();
        let (x, y) = layout.cell_origin(
            index,
            self.config.padding,
            self.config.gap_x,
            self.config.gap_y,
        );
        Some(Rect::new(
            x,
            y - self.scroll_y,
            layout.cell_width,
            layout.cell_height,
        ))
    }

    /// The grid's own pointer event of `kind` at the middle of `rect`.
    fn point(&mut self, rect: Rect, kind: MouseEventKind) {
        let (x, y) = rect.centre();
        // Whether the grid took it is nothing a tool answers to.
        let _taken = self.handle_event(&Event::Mouse(MouseEvent { x, y, kind }));
    }
}

impl Accessible for GridView {
    type Part = GridPart;
    type Event = Vec<GridEvent>;

    fn automation(&self, _width: f32, _height: f32) -> Node<GridPart> {
        let mut grid = Node::new(
            GridPart::Grid,
            Role::List,
            "Items",
            Rect::new(0.0, 0.0, self.container_width, self.container_height),
        );
        for (index, item) in self.items.iter().enumerate() {
            let Some(bounds) = self.item_box(index) else {
                continue;
            };
            let mut node = Node::new(
                GridPart::Item(item.id),
                Role::ListItem,
                item.label.clone(),
                bounds,
            );
            node.value = Some(Value::Chosen(self.selection.is_selected(index)));
            node.focused = self.selection.focused() == Some(index);
            node.focusable = true;
            grid.children.push(node);
        }
        grid
    }

    fn invoke(
        &mut self,
        part: &GridPart,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<Vec<GridEvent>>, Refusal> {
        let GridPart::Item(id) = *part else {
            return Err(Refusal::NotApplicable {
                role: Role::List,
                action: action.name(),
            });
        };
        let index = self
            .items
            .iter()
            .position(|item| item.id == id)
            .ok_or(Refusal::NoSuchWidget)?;
        if !matches!(action, Action::Choose | Action::Press) {
            return Err(Refusal::NotApplicable {
                role: Role::ListItem,
                action: action.name(),
            });
        }
        let queued = self.pending_events.len();
        // Into view as the arrow keys bring it, the scroll finished as the
        // eye sees it finish before the click.
        self.scroll_to_item(index);
        let target = self.scroll_target_y;
        self.set_scroll_y(target);
        let at = self.item_box(index).ok_or(Refusal::NoSuchWidget)?;
        if at.h <= 0.0 || at.bottom() <= 0.0 || at.y >= self.container_height {
            // A grid too short to show the row: nothing to click.
            return Err(Refusal::Hidden);
        }
        self.point(at, MouseEventKind::Press(MouseButton::Left));
        self.point(at, MouseEventKind::Release(MouseButton::Left));
        if matches!(action, Action::Press) {
            self.point(at, MouseEventKind::DoubleClick(MouseButton::Left));
        }
        let raised = self
            .pending_events
            .split_off(queued.min(self.pending_events.len()));
        Ok((!raised.is_empty()).then_some(raised))
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
