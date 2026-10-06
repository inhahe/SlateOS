//! A tree as tools see it ([`Accessible`]): the tree, and its rows nested as
//! the tree nests them -- each named by its label, described by its detail
//! or, greyed, by why, chosen while selected -- each holding its disclosure
//! arrow, where it has one, and its box, in a checkbox tree.
//!
//! A tool's actions need the source as well as the view -- opening a node
//! asks the source for its children, as the pointer's opening does -- so
//! what implements [`Accessible`] is the two together, [`TreeAccess`], made
//! for as long as a tool is served. Only the rows the tree shows are parts:
//! a node inside a closed one is reached by opening it.
//!
//! An action is the tree's own pointer event at the middle of where the part
//! is drawn -- read back from the frame the tree draws into, so it is where
//! the tree takes the press -- with a row scrolled out of the tree scrolled
//! into it first, as the user would. A row chosen is clicked; one pressed is
//! clicked twice, as a double click opens it.

use super::{TreeEvent, TreeHit, TreeSource, TreeView};
use crate::event::{MouseButton, MouseEvent, MouseEventKind};
use crate::frame::{Frame, Rect};
use crate::palette::Palette;
use crate::widget::automation::{Accessible, Action, Node, Refusal, Role, Value};

/// A part of a tree, as tools name it: by key path, as the tree names its
/// nodes, never by row -- a row number names another node once one opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreePart<K> {
    /// The tree.
    Tree,
    /// A node's row.
    Row(Vec<K>),
    /// A node's disclosure arrow: pressed, the node opens or closes.
    Disclosure(Vec<K>),
    /// A node's box, in a checkbox tree.
    Check(Vec<K>),
}

/// A tree view and the source of what it shows, together: what a tool sees
/// and acts on.
pub struct TreeAccess<'a, K, S> {
    /// The view: which nodes are open, chosen, ticked, and where it is.
    pub view: &'a mut TreeView<K>,
    /// What it shows.
    pub source: &'a S,
}

impl<K: Clone + Ord> TreeView<K> {
    /// The hit boxes of the tree as it is drawn now, from the frame it draws
    /// into: what the pointer's events are tested against.
    fn hit_boxes(&self) -> Frame<TreeHit<K>> {
        let mut frame = Frame::new(self.bounds.right(), self.bounds.bottom());
        // The boxes do not depend on colour, so any palette will do.
        self.draw(&Palette::for_mode(false), &mut frame, |hit| hit);
        frame
    }

    /// Where the row at `index` is, drawn or not: its own row while it is
    /// shown, a row's height above or below the tree's rows for each row it
    /// is scrolled out by.
    fn row_box(&self, index: usize, frame: &Frame<TreeHit<K>>) -> Option<Rect> {
        let row = self.rows.get(index)?;
        if let Some(rect) =
            frame.rect_of(|hit| matches!(hit, TreeHit::Row(path) if *path == row.path))
        {
            return Some(rect);
        }
        // Not drawn: placed from the first drawn row, as the tree would draw
        // it scrolled there.
        let first = self.rows.get(self.first_visible)?;
        let top = frame.rect_of(|hit| matches!(hit, TreeHit::Row(path) if *path == first.path))?;
        let rows_away = |n: usize| {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a count of rows, far below where f32 loses whole numbers"
            )]
            let n = n as f32;
            n * self.metrics.row_height
        };
        let y = if index >= self.first_visible {
            top.y + rows_away(index.saturating_sub(self.first_visible))
        } else {
            top.y - rows_away(self.first_visible.saturating_sub(index))
        };
        Some(Rect::new(top.x, y, top.w, top.h))
    }

    /// Scroll so the row at `index` is drawn, as little as that takes.
    fn reveal_row(&mut self, index: usize) {
        let shown = self.capacity().max(1);
        if index < self.first_visible {
            self.scroll_to(index);
        } else if index >= self.first_visible.saturating_add(shown) {
            self.scroll_to(index.saturating_add(1).saturating_sub(shown));
        }
    }

    /// The tree as tools see it: what [`TreeAccess`] shows, which needs the
    /// view alone -- for a component holding a tree, which shows it without
    /// a source at hand to make a [`TreeAccess`] of.
    pub(crate) fn tool_tree(&self) -> Node<TreePart<K>> {
        let frame = self.hit_boxes();
        let mut tree = Node::new(TreePart::Tree, Role::Tree, "Tree", self.bounds);
        tree.enabled = !self.state.is_disabled();
        tree.description = self.state.reason().map(str::to_owned);
        // The rows in order, nested by depth: the open nodes' rows above
        // their children's, each finished when the next row is no deeper.
        let mut open: Vec<Node<TreePart<K>>> = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            let Some(bounds) = self.row_box(index, &frame) else {
                continue;
            };
            let mut node = Node::new(
                TreePart::Row(row.path.clone()),
                Role::TreeItem,
                row.label.clone(),
                bounds,
            );
            let chosen = self.selected.as_deref() == Some(row.path.as_slice());
            node.value = Some(Value::Chosen(chosen));
            node.focused = chosen;
            node.focusable = true;
            node.enabled = !row.state.is_disabled();
            node.description = row
                .state
                .reason()
                .map(str::to_owned)
                .or_else(|| row.detail.clone());
            let cell = |hit: TreeHit<K>| {
                frame
                    .rect_of(|drawn| *drawn == hit)
                    .unwrap_or(Rect::new(bounds.x, bounds.y, 0.0, bounds.h))
            };
            if row.expandable {
                let arrow = TreePart::Disclosure(row.path.clone());
                let name = if row.expanded { "Collapse" } else { "Expand" };
                node.children.push(Node::new(
                    arrow,
                    Role::Button,
                    name,
                    cell(TreeHit::Disclosure(row.path.clone())),
                ));
            }
            if let Some(state) = self.check_state(&row.path) {
                let mut check = Node::new(
                    TreePart::Check(row.path.clone()),
                    Role::CheckBox,
                    row.label.clone(),
                    cell(TreeHit::Check(row.path.clone())),
                );
                check.value = Some(Value::Check(state));
                check.enabled = node.enabled;
                node.children.push(check);
            }
            // Close every open row this one is not inside.
            while let Some(parent) = open.last() {
                let TreePart::Row(parent_path) = &parent.id else {
                    break;
                };
                if row.path.starts_with(parent_path) && row.path.len() > parent_path.len() {
                    break;
                }
                let Some(done) = open.pop() else {
                    break;
                };
                match open.last_mut() {
                    Some(above) => above.children.push(done),
                    None => tree.children.push(done),
                }
            }
            open.push(node);
        }
        while let Some(done) = open.pop() {
            match open.last_mut() {
                Some(above) => above.children.push(done),
                None => tree.children.push(done),
            }
        }
        tree
    }
}

impl<K: Clone + Ord, S: TreeSource<Key = K>> TreeAccess<'_, K, S> {
    /// Where a press lands on `hit` -- the middle of its box, as the tree
    /// is drawn after its row is scrolled into it -- or why there is
    /// nowhere: no such row, or a row its tree has no room to show.
    fn press_point(&mut self, hit: &TreeHit<K>) -> Result<(f32, f32), Refusal> {
        let (TreeHit::Row(path) | TreeHit::Disclosure(path) | TreeHit::Check(path)) = hit else {
            return Err(Refusal::NoSuchWidget);
        };
        let index = self.view.index_of(path).ok_or(Refusal::NoSuchWidget)?;
        self.view.reveal_row(index);
        let frame = self.view.hit_boxes();
        let rect = frame
            .hits()
            .iter()
            .rev()
            .find(|(drawn, _)| drawn == hit)
            .map(|(_, rect)| *rect)
            .ok_or(Refusal::NoSuchWidget)?;
        // The row's box holds its arrow's and its box's: aim past them, at
        // its last quarter, where only the row takes the press.
        let point = match hit {
            TreeHit::Row(_) => (rect.x + rect.w * 0.75, rect.y + rect.h / 2.0),
            _ => rect.centre(),
        };
        if frame.hit_test_ref(point.0, point.1) == Some(hit) {
            Ok(point)
        } else {
            Err(Refusal::Hidden)
        }
    }

    /// The tree's own event of `kind` at `(x, y)`.
    fn event(&mut self, kind: MouseEventKind, (x, y): (f32, f32)) -> Vec<TreeEvent<K>> {
        self.view
            .handle_mouse(&MouseEvent { x, y, kind }, self.source)
    }
}

/// Events for a tool: `None` for none.
fn said<K>(events: Vec<TreeEvent<K>>) -> Option<Vec<TreeEvent<K>>> {
    (!events.is_empty()).then_some(events)
}

impl<K, S> Accessible for TreeAccess<'_, K, S>
where
    K: Clone + Ord + std::fmt::Debug,
    S: TreeSource<Key = K>,
{
    type Part = TreePart<K>;
    type Event = Vec<TreeEvent<K>>;

    fn automation(&self, _width: f32, _height: f32) -> Node<TreePart<K>> {
        self.view.tool_tree()
    }

    fn invoke(
        &mut self,
        part: &TreePart<K>,
        action: Action,
        _width: f32,
        _height: f32,
    ) -> Result<Option<Vec<TreeEvent<K>>>, Refusal> {
        let not_for = |role: Role| Refusal::NotApplicable {
            role,
            action: action.name(),
        };
        let path = match part {
            TreePart::Tree => return Err(not_for(Role::Tree)),
            TreePart::Row(path) | TreePart::Disclosure(path) | TreePart::Check(path) => path,
        };
        let index = self.view.index_of(path).ok_or(Refusal::NoSuchWidget)?;
        let row_disabled = self
            .view
            .rows
            .get(index)
            .is_some_and(|row| row.state.is_disabled());
        let hit = match (part, &action) {
            (TreePart::Row(_), Action::Choose | Action::Focus | Action::Press) => {
                TreeHit::Row(path.clone())
            }
            (TreePart::Disclosure(_), Action::Press) => TreeHit::Disclosure(path.clone()),
            (TreePart::Check(_), Action::Toggle | Action::Press) => {
                if self.view.check_state(path).is_none() {
                    return Err(Refusal::NoSuchWidget);
                }
                TreeHit::Check(path.clone())
            }
            (TreePart::Row(_), _) => return Err(not_for(Role::TreeItem)),
            (TreePart::Disclosure(_), _) => return Err(not_for(Role::Button)),
            (TreePart::Check(_), _) => return Err(not_for(Role::CheckBox)),
            (TreePart::Tree, _) => return Err(not_for(Role::Tree)),
        };
        if self.view.state.is_disabled() || row_disabled {
            return Err(Refusal::Disabled);
        }
        let at = self.press_point(&hit)?;
        let mut events = self.event(MouseEventKind::Press(MouseButton::Left), at);
        events.extend(self.event(MouseEventKind::Release(MouseButton::Left), at));
        if matches!((part, &action), (TreePart::Row(_), Action::Press)) {
            // Opened, as a double click opens it.
            events.extend(self.event(MouseEventKind::DoubleClick(MouseButton::Left), at));
            events.extend(self.event(MouseEventKind::Release(MouseButton::Left), at));
        }
        Ok(said(events))
    }
}

#[cfg(test)]
#[path = "accessible_tests.rs"]
mod tests;
