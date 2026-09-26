//! Slate OS Mind Map Application
//!
//! A full-featured mind mapping tool with:
//! - Central root node with branching children in a radial layout
//! - Node CRUD: add child, add sibling, edit text, delete subtree
//! - Node colors and shapes (rectangle, rounded rectangle, ellipse, diamond, pill)
//! - Parent-child connecting lines with curved bezier paths
//! - Auto-layout using radial tree algorithm
//! - Manual drag to reposition nodes
//! - Canvas pan and zoom (10% to 400%)
//! - Multiple maps with tab switching
//! - Collapse/expand subtrees
//! - Undo/redo, each map its own history
//! - Each map saved whole to a file of its own (YAML, `.mindmap`), and a
//!   question before a map with changes not saved is closed or lost with the
//!   window
//! - Text export and import (indented outline)
//! - Search with highlighting
//! - Keyboard shortcuts for all major actions
//! - Catppuccin Mocha theme
//!
//! Uses the guitk library for UI rendering.

#![allow(clippy::too_many_lines)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::module_name_repetitions)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::similar_names)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::fn_params_excessive_bools)]
#![allow(clippy::wildcard_imports)]

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

use oswindow::app::{self, App, Response};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;
use unsaved::{Choice, Question};

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

/// Preset node colors the user can cycle through.
/// The colours a node can be cycled through.
///
/// Content, not chrome: a node's colour is the user's choice, cycled with a
/// key and saved with the map, so it must not follow the desktop theme -- a
/// saved mind map would otherwise recolour itself when the theme changed.
/// Fixed values for the same reason `paint`'s swatches are fixed.
const NODE_COLORS: [Color; 8] = [
    Color::from_hex(0x89B4FA),
    Color::from_hex(0xA6E3A1),
    Color::from_hex(0xF38BA8),
    Color::from_hex(0xF9E2AF),
    Color::from_hex(0xFAB387),
    Color::from_hex(0x94E2D5),
    Color::from_hex(0xCBA6F7),
    Color::from_hex(0x313244),
];

// ============================================================================
// Layout constants
// ============================================================================

/// Height of the top toolbar.
const TOOLBAR_HEIGHT: f32 = 40.0;
/// Every key this program answers, and what it does.
///
/// Two dozen bindings and, until this list existed, no way to learn one but
/// reading the source. `B` is the worst of them: it is the only way to bring
/// the sidebar back, so a user who pressed it once had hidden a panel with no
/// way to find out how to return it -- a toolbar cannot advertise the key that
/// hides it.
///
/// **Each row is a key this program actually answers**, which is not a
/// property the list has on its own: `every_advertised_key_does_something`
/// walks it, reads each label with `guitk::shortcut` and presses every key it
/// names. `apps/rssreader` shipped an overlay of twenty-one shortcuts of which
/// about four worked.
const SHORTCUTS: &[(&str, &str)] = &[
    ("Up / Down", "Select the parent / the first child"),
    ("Left / Right", "Select the previous / next sibling"),
    ("Tab", "Add a child to this node"),
    ("Enter", "Add a sibling beside this node"),
    ("F2", "Rename this node"),
    ("Delete", "Delete this node and everything under it"),
    ("Space", "Collapse or expand this node"),
    ("C", "Next colour"),
    ("S", "Next shape"),
    ("L", "Lay the map out again"),
    ("B", "Show or hide the sidebar"),
    ("= / -", "Zoom in / out"),
    ("Ctrl+0", "Reset the view"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+F", "Find a node"),
    ("Ctrl+N", "A new map"),
    ("Ctrl+O", "Open a map or an outline"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as"),
    ("Ctrl+E", "Write this map as an outline"),
    ("Ctrl+W", "Close this map"),
    ("Ctrl+Tab / Ctrl+Shift+Tab", "Next / previous map"),
    ("F1 / ?", "This list"),
];

/// What a toolbar button does -- the same as its key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolbarAction {
    Open,
    Save,
    AddChild,
    AddSibling,
    Delete,
    Undo,
    Redo,
    Layout,
    ZoomIn,
    ZoomOut,
}

/// The toolbar's buttons, left to right.
///
/// They were drawn from a list of labels and fixed positions, and nothing
/// answered a click on any of them; three overlapped the next by five pixels.
/// One list now, which the drawing and the hit test both read, and each
/// button as wide as its label.
const TOOLBAR_BUTTONS: [(&str, ToolbarAction); 10] = [
    ("Open", ToolbarAction::Open),
    ("Save", ToolbarAction::Save),
    ("Add Child", ToolbarAction::AddChild),
    ("Add Sibling", ToolbarAction::AddSibling),
    ("Delete", ToolbarAction::Delete),
    ("Undo", ToolbarAction::Undo),
    ("Redo", ToolbarAction::Redo),
    ("Layout", ToolbarAction::Layout),
    ("Zoom+", ToolbarAction::ZoomIn),
    ("Zoom-", ToolbarAction::ZoomOut),
];
/// Where the first toolbar button starts: after the window's name.
const TOOLBAR_FIRST_X: f32 = 110.0;
/// The size a button's label is drawn at, which its width is measured at.
const TOOLBAR_LABEL_SIZE: f32 = 11.0;
/// Space between buttons.
const TOOLBAR_GAP: f32 = 6.0;

/// Every toolbar button's rectangle -- x, y, width, height -- in order.
fn toolbar_button_rects() -> Vec<(f32, f32, f32, f32)> {
    let mut x = TOOLBAR_FIRST_X;
    TOOLBAR_BUTTONS
        .iter()
        .map(|(label, _)| {
            let w = (text::measure(label, TOOLBAR_LABEL_SIZE, FontWeightHint::Regular) + 16.0)
                .max(44.0);
            let rect = (x, 6.0, w, 28.0);
            x += w + TOOLBAR_GAP;
            rect
        })
        .collect()
}

/// The toolbar button at a point, if there is one.
fn toolbar_action_at(x: f32, y: f32) -> Option<ToolbarAction> {
    toolbar_button_rects()
        .into_iter()
        .zip(TOOLBAR_BUTTONS)
        .find(|(rect, _)| inside(*rect, x, y))
        .map(|(_, (_, action))| action)
}

/// Whether `(x, y)` is inside `rect` (x, y, width, height).
fn inside((rx, ry, rw, rh): (f32, f32, f32, f32), x: f32, y: f32) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

/// Where the first tab starts, and the space between tabs.
const TAB_FIRST_X: f32 = 10.0;
const TAB_GAP: f32 = 4.0;
/// The widest a tab is drawn, and the narrowest: tabs narrow as maps are
/// added, so that more of them fit before any runs off the window.
const TAB_MAX_W: f32 = 140.0;
const TAB_MIN_W: f32 = 64.0;
/// The square a tab's close mark is drawn in, at its right end.
const TAB_CLOSE_W: f32 = 18.0;
/// The "+" button after the last tab.
const PLUS_W: f32 = 24.0;
/// Height of the bottom status bar.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Width the status bar's "Selected: … (ID: n)" line is drawn into.
///
/// Named because it is used twice and the two uses must agree: once to work
/// out how much room the node's name may have, and once as the `max_width` the
/// line is drawn with. A literal in both places is a pair that drifts.
const SEL_INFO_WIDTH: f32 = 290.0;
/// Size that line is drawn at — same reason.
const SEL_INFO_SIZE: f32 = 11.0;
/// Width of the sidebar panel.
const SIDEBAR_WIDTH: f32 = 200.0;
/// Height of a map tab.
const TAB_HEIGHT: f32 = 28.0;
/// Minimum zoom level (10%).
const MIN_ZOOM: f32 = 0.1;
/// Maximum zoom level (400%).
const MAX_ZOOM: f32 = 4.0;
/// Default node width.
const DEFAULT_NODE_W: f32 = 140.0;
/// Default node height.
const DEFAULT_NODE_H: f32 = 40.0;
/// Root node dimensions (slightly larger).
const ROOT_NODE_W: f32 = 180.0;
/// Root node height.
const ROOT_NODE_H: f32 = 50.0;
/// Maximum undo/redo steps.
const MAX_UNDO: usize = 200;
/// Horizontal spacing between parent and children in radial layout.
const RADIAL_H_GAP: f32 = 60.0;
/// Vertical spacing between sibling nodes.
const RADIAL_V_GAP: f32 = 20.0;
/// Font size for node text.
const NODE_FONT_SIZE: f32 = 13.0;
/// Font size for root node text.
const ROOT_FONT_SIZE: f32 = 16.0;
/// Corner radius for rounded rect nodes.
const NODE_CORNER_RADIUS: f32 = 8.0;
/// Corner radius for UI panels.
const PANEL_CORNER: f32 = 4.0;
/// Width of connecting lines between nodes.
const LINE_WIDTH: f32 = 2.0;
/// Collapse indicator size.
const COLLAPSE_SIZE: f32 = 12.0;

// ============================================================================
// Node ID type and generator
// ============================================================================

/// Stable identifier for mind map nodes. Stored in HashMap for O(1) lookup.
pub type NodeId = u32;

/// Monotonically increasing ID generator.
#[derive(Debug, Clone)]
pub struct IdGenerator {
    next: u32,
}

impl Default for IdGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl IdGenerator {
    pub const fn new() -> Self {
        Self { next: 1 }
    }

    pub fn next_id(&mut self) -> u32 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// ============================================================================
// Node shape
// ============================================================================

/// Visual shape of a mind map node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeShape {
    Rectangle,
    RoundedRect,
    Ellipse,
    Diamond,
    Pill,
}

impl NodeShape {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rect",
            Self::RoundedRect => "Rounded",
            Self::Ellipse => "Ellipse",
            Self::Diamond => "Diamond",
            Self::Pill => "Pill",
        }
    }

    pub fn all() -> &'static [NodeShape] {
        &[
            Self::Rectangle,
            Self::RoundedRect,
            Self::Ellipse,
            Self::Diamond,
            Self::Pill,
        ]
    }

    pub fn next(self) -> Self {
        match self {
            Self::Rectangle => Self::RoundedRect,
            Self::RoundedRect => Self::Ellipse,
            Self::Ellipse => Self::Diamond,
            Self::Diamond => Self::Pill,
            Self::Pill => Self::Rectangle,
        }
    }
}

// ============================================================================
// Mind map node
// ============================================================================

/// A single node in the mind map tree.
#[derive(Clone, Debug)]
pub struct MindMapNode {
    /// Unique stable identifier.
    pub id: NodeId,
    /// Text content of this node.
    pub text: String,
    /// Parent node ID (None for root).
    pub parent: Option<NodeId>,
    /// Ordered list of child node IDs.
    pub children: Vec<NodeId>,
    /// Fill color of the node.
    pub color: Color,
    /// Visual shape.
    pub shape: NodeShape,
    /// Position in canvas space (center x).
    pub x: f32,
    /// Position in canvas space (center y).
    pub y: f32,
    /// Width of this node.
    pub width: f32,
    /// Height of this node.
    pub height: f32,
    /// Whether the subtree rooted at this node is collapsed.
    pub collapsed: bool,
    /// Color index into `NODE_COLORS` for cycling.
    pub color_index: u8,
}

impl MindMapNode {
    /// Create a new node at a given position.
    pub fn new(
        id: NodeId,
        text: String,
        parent: Option<NodeId>,
        color: Color,
        color_index: u8,
    ) -> Self {
        let (w, h) = if parent.is_none() {
            (ROOT_NODE_W, ROOT_NODE_H)
        } else {
            (DEFAULT_NODE_W, DEFAULT_NODE_H)
        };
        Self {
            id,
            text,
            parent,
            children: Vec::new(),
            color,
            shape: if parent.is_none() {
                NodeShape::Ellipse
            } else {
                NodeShape::RoundedRect
            },
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            collapsed: false,
            color_index,
        }
    }

    /// Bounding rectangle: top-left corner, width, height.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        (
            self.x - self.width / 2.0,
            self.y - self.height / 2.0,
            self.width,
            self.height,
        )
    }

    /// Check if a point (in canvas space) is inside this node.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        let (bx, by, bw, bh) = self.bounds();
        px >= bx && px <= bx + bw && py >= by && py <= by + bh
    }

    /// Center point.
    pub fn center(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Right edge center (for connecting lines going right).
    pub fn right_center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y)
    }

    /// Left edge center (for connecting lines coming from left).
    pub fn left_center(&self) -> (f32, f32) {
        (self.x - self.width / 2.0, self.y)
    }
}

// ============================================================================
// Undo/redo action
// ============================================================================

/// Reversible actions for undo/redo.
#[derive(Clone, Debug)]
pub enum Action {
    /// A node was added with the given data.
    AddNode {
        node_id: NodeId,
        parent_id: Option<NodeId>,
        text: String,
        color: Color,
        color_index: u8,
        shape: NodeShape,
    },
    /// A subtree was deleted (stores all removed nodes).
    DeleteSubtree {
        nodes: Vec<MindMapNode>,
        parent_id: Option<NodeId>,
        /// Index in parent's children list where the subtree root was.
        child_index: usize,
    },
    /// Node text was edited.
    EditText {
        node_id: NodeId,
        old_text: String,
        new_text: String,
    },
    /// Node color was changed.
    ChangeColor {
        node_id: NodeId,
        old_color: Color,
        old_index: u8,
        new_color: Color,
        new_index: u8,
    },
    /// Node shape was changed.
    ChangeShape {
        node_id: NodeId,
        old_shape: NodeShape,
        new_shape: NodeShape,
    },
    /// Node was moved (dragged).
    MoveNode {
        node_id: NodeId,
        old_x: f32,
        old_y: f32,
        new_x: f32,
        new_y: f32,
    },
    /// Toggle collapsed state.
    ToggleCollapse { node_id: NodeId },
}

// ============================================================================
// Mind map (a single map with its own tree)
// ============================================================================

/// A single mind map containing a tree of nodes.
#[derive(Clone, Debug)]
pub struct MindMap {
    /// Name/title of this map.
    pub name: String,
    /// All nodes indexed by ID for O(1) lookup.
    pub nodes: HashMap<NodeId, MindMapNode>,
    /// The root node ID.
    pub root_id: NodeId,
    /// ID generator for this map.
    pub id_gen: IdGenerator,
    /// Which map this is, for as long as the window is open. A tab's index
    /// moves when a tab before it closes, and a question or a save waiting on
    /// the picker must still find the map it was about.
    pub id: u64,
    /// This map's history. It was the window's, replayed onto whichever map
    /// was showing -- and node numbers repeat from map to map, so undoing a
    /// change made on one map could delete a node of another.
    pub undo_stack: VecDeque<Action>,
    pub redo_stack: Vec<Action>,
    /// The map's own file, once it has one.
    pub document_path: Option<std::path::PathBuf>,
    /// Whether the map has changed since it was last saved or opened.
    pub dirty: bool,
}

impl MindMap {
    /// Create a new mind map with a single root node.
    pub fn new(name: String, id_gen: &mut IdGenerator) -> Self {
        let root_id = id_gen.next_id();
        let root = MindMapNode::new(root_id, "Central Idea".to_string(), None, NODE_COLORS[0], 0);
        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);
        Self {
            name,
            nodes,
            root_id,
            id_gen: id_gen.clone(),
            id: 0,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            document_path: None,
            dirty: false,
        }
    }

    /// Get a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&MindMapNode> {
        self.nodes.get(&id)
    }

    /// Get a mutable node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut MindMapNode> {
        self.nodes.get_mut(&id)
    }

    /// Number of nodes in this map.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Add a child node to a parent. Returns the new node's ID.
    pub fn add_child(
        &mut self,
        parent_id: NodeId,
        text: String,
        color: Color,
        color_index: u8,
    ) -> Option<NodeId> {
        if !self.nodes.contains_key(&parent_id) {
            return None;
        }
        let new_id = self.id_gen.next_id();
        let node = MindMapNode::new(new_id, text, Some(parent_id), color, color_index);
        self.nodes.insert(new_id, node);
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.push(new_id);
        }
        Some(new_id)
    }

    /// Re-add a child node reusing a previously-allocated ID and shape.
    ///
    /// Used by redo: re-running an `AddNode` action must restore the node under
    /// its *original* ID, not allocate a fresh one. If a new ID were generated,
    /// any later action that references this node as a parent (by its original
    /// ID) would fail to find it, silently dropping the redo. Returns `false`
    /// if the parent does not exist or the ID is already in use.
    pub fn add_child_with_id(
        &mut self,
        node_id: NodeId,
        parent_id: NodeId,
        text: String,
        color: Color,
        color_index: u8,
        shape: NodeShape,
    ) -> bool {
        if !self.nodes.contains_key(&parent_id) || self.nodes.contains_key(&node_id) {
            return false;
        }
        let mut node = MindMapNode::new(node_id, text, Some(parent_id), color, color_index);
        node.shape = shape;
        self.nodes.insert(node_id, node);
        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            parent.children.push(node_id);
        }
        true
    }

    /// Add a sibling node after the given node. Returns the new node's ID.
    pub fn add_sibling(
        &mut self,
        sibling_id: NodeId,
        text: String,
        color: Color,
        color_index: u8,
    ) -> Option<NodeId> {
        let parent_id = self.nodes.get(&sibling_id)?.parent?;
        let new_id = self.id_gen.next_id();
        let node = MindMapNode::new(new_id, text, Some(parent_id), color, color_index);
        self.nodes.insert(new_id, node);

        if let Some(parent) = self.nodes.get_mut(&parent_id) {
            // Insert after the sibling
            if let Some(pos) = parent.children.iter().position(|&c| c == sibling_id) {
                parent.children.insert(pos.saturating_add(1), new_id);
            } else {
                parent.children.push(new_id);
            }
        }
        Some(new_id)
    }

    /// Collect all node IDs in the subtree rooted at `root` (including root).
    pub fn subtree_ids(&self, root: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            result.push(id);
            if let Some(node) = self.nodes.get(&id) {
                for &child_id in node.children.iter().rev() {
                    stack.push(child_id);
                }
            }
        }
        result
    }

    /// Delete a subtree rooted at `node_id`. Returns all removed nodes in
    /// tree order, or None if the node doesn't exist or is the root.
    pub fn delete_subtree(
        &mut self,
        node_id: NodeId,
    ) -> Option<(Vec<MindMapNode>, Option<NodeId>, usize)> {
        if node_id == self.root_id {
            return None; // cannot delete root
        }
        let parent_id = self.nodes.get(&node_id)?.parent;

        // Find index in parent's children list
        let child_index = if let Some(pid) = parent_id {
            self.nodes
                .get(&pid)
                .and_then(|p| p.children.iter().position(|&c| c == node_id))
                .unwrap_or(0)
        } else {
            0
        };

        let ids = self.subtree_ids(node_id);
        let mut removed = Vec::new();
        for &id in &ids {
            if let Some(n) = self.nodes.remove(&id) {
                removed.push(n);
            }
        }

        // Remove from parent's children
        if let Some(pid) = parent_id
            && let Some(parent) = self.nodes.get_mut(&pid)
        {
            parent.children.retain(|&c| c != node_id);
        }

        Some((removed, parent_id, child_index))
    }

    /// Re-insert a previously deleted subtree.
    pub fn restore_subtree(
        &mut self,
        nodes: &[MindMapNode],
        parent_id: Option<NodeId>,
        child_index: usize,
    ) {
        // `first` rather than an emptiness test and then an index: one
        // expression, and the two cannot drift apart.
        let Some(subtree_root_id) = nodes.first().map(|n| n.id) else {
            return;
        };

        for node in nodes {
            self.nodes.insert(node.id, node.clone());
        }

        if let Some(pid) = parent_id
            && let Some(parent) = self.nodes.get_mut(&pid)
        {
            let idx = child_index.min(parent.children.len());
            parent.children.insert(idx, subtree_root_id);
        }
    }

    /// Edit the text of a node. Returns the old text.
    pub fn edit_text(&mut self, node_id: NodeId, new_text: String) -> Option<String> {
        let node = self.nodes.get_mut(&node_id)?;
        let old = core::mem::replace(&mut node.text, new_text);
        Some(old)
    }

    /// Change the color of a node. Returns the old color and index.
    pub fn change_color(
        &mut self,
        node_id: NodeId,
        color: Color,
        color_index: u8,
    ) -> Option<(Color, u8)> {
        let node = self.nodes.get_mut(&node_id)?;
        let old_color = node.color;
        let old_index = node.color_index;
        node.color = color;
        node.color_index = color_index;
        Some((old_color, old_index))
    }

    /// Change the shape of a node. Returns the old shape.
    pub fn change_shape(&mut self, node_id: NodeId, new_shape: NodeShape) -> Option<NodeShape> {
        let node = self.nodes.get_mut(&node_id)?;
        let old = node.shape;
        node.shape = new_shape;
        Some(old)
    }

    /// Toggle collapsed state of a node.
    pub fn toggle_collapse(&mut self, node_id: NodeId) -> bool {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.collapsed = !node.collapsed;
            node.collapsed
        } else {
            false
        }
    }

    /// Move a node to a new position. Returns old position.
    pub fn move_node(&mut self, node_id: NodeId, new_x: f32, new_y: f32) -> Option<(f32, f32)> {
        let node = self.nodes.get_mut(&node_id)?;
        let old = (node.x, node.y);
        node.x = new_x;
        node.y = new_y;
        Some(old)
    }

    /// Get all visible (non-collapsed) node IDs starting from root.
    pub fn visible_node_ids(&self) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut stack = vec![self.root_id];
        while let Some(id) = stack.pop() {
            result.push(id);
            if let Some(node) = self.nodes.get(&id)
                && !node.collapsed
            {
                for &child_id in node.children.iter().rev() {
                    stack.push(child_id);
                }
            }
        }
        result
    }

    /// Search for nodes containing the query (case-insensitive).
    pub fn search(&self, query: &str) -> Vec<NodeId> {
        if query.is_empty() {
            return Vec::new();
        }
        let lower = query.to_lowercase();
        self.nodes
            .iter()
            .filter(|(_, node)| node.text.to_lowercase().contains(&lower))
            .map(|(&id, _)| id)
            .collect()
    }

    /// Build a map from an indented outline, or `None` if there is nothing
    /// in it.
    ///
    /// The inverse of [`export_text`](Self::export_text) for the two things an
    /// outline actually carries: the words and the branches. It is NOT a full
    /// inverse and the caller says so -- colour, shape and collapse state are
    /// not in the file, and positions do not need to be, because `auto_layout`
    /// computes them.
    ///
    /// # How depth is read
    ///
    /// Two spaces per level, as `export_text` writes, and a leading `- ` on
    /// everything below the root. A line indented further than one level past
    /// its predecessor is clamped to one level deeper rather than rejected:
    /// hand-written outlines skip levels, and refusing the file would lose
    /// the whole map over a cosmetic slip. A line indented *less* closes as
    /// many levels as it needs to.
    pub fn from_outline(name: &str, text: &str, id_gen: &mut IdGenerator) -> Option<Self> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let first = lines.next()?;
        let mut map = Self::new(name.to_owned(), id_gen);
        let root_id = map.root_id;
        if let Some(root) = map.nodes.get_mut(&root_id) {
            root.text = outline_body(first).to_owned();
        }
        // `stack[d]` is the node that a line at depth `d + 1` hangs from.
        let mut stack = vec![root_id];
        for line in lines {
            let depth = outline_depth(line).max(1);
            // Clamped, not rejected: see the doc above.
            let depth = depth.min(stack.len());
            stack.truncate(depth);
            let parent = *stack.last()?;
            // The same cycle `MindMapApp::color_at` uses, reached directly:
            // that one is an associated function on the app and this is on the
            // map. `% len()` is why the index is always in range.
            // `checked_rem` and `get`, not `%` and `[]`: the defensive lints
            // are on in this crate and an empty palette would otherwise be a
            // panic in a file reader, which is the worst place for one.
            let slot = depth.checked_rem(NODE_COLORS.len()).unwrap_or(0);
            let colour = NODE_COLORS
                .get(slot)
                .copied()
                .unwrap_or(FALLBACK_NODE_COLOR);
            let colour_index = u8::try_from(slot).unwrap_or(0);
            let id = map.add_child(parent, outline_body(line).to_owned(), colour, colour_index)?;
            stack.push(id);
        }
        Some(map)
    }

    /// Export the map as an indented text outline.
    pub fn export_text(&self) -> String {
        let mut output = String::new();
        self.export_node_text(&mut output, self.root_id, 0);
        output
    }

    fn export_node_text(&self, output: &mut String, node_id: NodeId, depth: usize) {
        if let Some(node) = self.nodes.get(&node_id) {
            for _ in 0..depth {
                output.push_str("  ");
            }
            if depth == 0 {
                output.push_str(&outline_text(&node.text));
            } else {
                output.push_str("- ");
                output.push_str(&outline_text(&node.text));
            }
            output.push('\n');
            for &child_id in &node.children {
                self.export_node_text(output, child_id, depth.saturating_add(1));
            }
        }
    }

    /// Depth of a node from root.
    pub fn depth(&self, node_id: NodeId) -> u32 {
        let mut d = 0u32;
        let mut current = node_id;
        while let Some(node) = self.nodes.get(&current) {
            if let Some(pid) = node.parent {
                d = d.saturating_add(1);
                current = pid;
            } else {
                break;
            }
        }
        d
    }

    /// Count of all descendants (not including self).
    pub fn descendant_count(&self, node_id: NodeId) -> usize {
        self.subtree_ids(node_id).len().saturating_sub(1)
    }
}

/// Flatten node text onto the single line an outline entry occupies.
///
/// In an indented outline, structure *is* whitespace: the line break starts a
/// sibling and the leading spaces choose its depth. A node whose text contains
/// a newline therefore draws extra branches in the exported map that do not
/// exist in the real one -- and unlike the other exporters audited here, this
/// one has no matching importer to be fooled, so the victim is a human reading
/// the outline, or whatever other outliner they open it in.
///
/// Node text is a short label, so the format offers nothing to escape with and
/// nothing is lost by folding: control characters become single spaces, and
/// runs of resulting whitespace collapse so the label still reads as one
/// phrase rather than acquiring a gap where the break was.
fn outline_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c.is_control() || c == '\t' {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

/// What a node is coloured when the palette is somehow empty.
///
/// `NODE_COLORS` has eight entries and cannot be empty, so this is never
/// reached. It exists because the alternative is indexing, and a panic in a
/// file reader is the worst place for one.
const FALLBACK_NODE_COLOR: Color = Color::rgb(0x88, 0x88, 0x88);

/// A map name reduced to something that can be a filename.
///
/// Only the three characters a path cannot contain are replaced: the name is
/// the user's, and rewriting more of it than necessary means they cannot find
/// the file by the name they gave the map.
fn sanitise_map_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | '\0') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        String::from("map")
    } else {
        trimmed.to_owned()
    }
}

/// How many levels deep an outline line sits: two spaces per level.
fn outline_depth(line: &str) -> usize {
    // `saturating_sub` and `checked_div`: the trimmed string is never longer
    // than the original, so neither can bite -- but the defensive lints do not
    // know that, and proving it in a comment is cheaper than an allow.
    let spaces = line
        .len()
        .saturating_sub(line.trim_start_matches(' ').len());
    spaces.checked_div(2).unwrap_or(0)
}

/// The text of an outline line, without its indent or its bullet.
fn outline_body(line: &str) -> &str {
    let t = line.trim_start_matches(' ');
    t.strip_prefix("- ").unwrap_or(t).trim_end()
}

/// The first key of a mind map file, and the format it names.
const MINDMAP_FORMAT: i64 = 1;

/// The most of a file one open will read.
///
/// A mind map file larger than this is refused rather than read in part: a
/// map cut short reads as a smaller map with no sign anything was missing,
/// and saving it would write the loss over the whole file. An outline larger
/// than this is opened as far as its last whole line and said to be
/// incomplete. It becomes a new map, saved nowhere near the file it came
/// from, so part of one is worth having -- but said, because **a map short of
/// a branch looks like a map that never had one.**
const MAX_OPEN_BYTES: usize = 32 * 1024 * 1024;

/// A colour as written: `#RRGGBB`, or `#RRGGBBAA` when it is not opaque.
fn colour_hex(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", c.r, c.g, c.b, c.a)
    }
}

/// A colour read back from `#RRGGBB` or `#RRGGBBAA`.
fn parse_colour(text: &str) -> Option<Color> {
    let hex = text.trim().strip_prefix('#')?;
    if !hex.is_ascii() || !(hex.len() == 6 || hex.len() == 8) {
        return None;
    }
    let byte = |at: usize| {
        hex.get(at..at.saturating_add(2))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok())
    };
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, a))
}

/// The keys under `path` that are positions, in order.
fn positions(doc: &yamldoc::Document, path: &[&str]) -> Vec<String> {
    let mut keys: Vec<(u64, String)> = doc
        .keys(path)
        .into_iter()
        .filter_map(|k| k.parse::<u64>().ok().map(|n| (n, k)))
        .collect();
    keys.sort_unstable();
    keys.into_iter().map(|(_, k)| k).collect()
}

/// A number read back as the `f32` it was written from.
#[expect(
    clippy::cast_possible_truncation,
    reason = "every number here was written from an f32, so it comes back within range"
)]
fn read_f32(doc: &yamldoc::Document, path: &[&str]) -> Option<f32> {
    doc.get_f64(path)
        .filter(|v| v.is_finite())
        .map(|v| v as f32)
}

/// Whether two paths name one file: the same path, or two that resolve to the
/// same place. A path that cannot be resolved -- a file since deleted -- is
/// compared as written.
fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The map as a document: its name, then every node -- the root first, and
/// each node's children after it in their order -- with every property a
/// user can set: text, colour, shape, where it is, how big, and whether it is
/// folded.
///
/// Not the outline, which keeps the words and the tree and nothing else. YAML,
/// as the diagram and the whiteboard keep theirs, versioned under
/// `slateos-mindmap` so a later format is refused rather than half-read.
pub fn mindmap_document(map: &MindMap) -> yamldoc::Document {
    let mut doc = yamldoc::Document::new();
    doc.set_i64(&["slateos-mindmap"], MINDMAP_FORMAT);
    doc.set_str(&["name"], &map.name);
    // Depth first from the root, so a parent is always written before its
    // children and the children in their order.
    let mut order = Vec::with_capacity(map.nodes.len());
    let mut stack = vec![map.root_id];
    while let Some(id) = stack.pop() {
        let Some(node) = map.nodes.get(&id) else {
            continue;
        };
        order.push(node);
        stack.extend(node.children.iter().rev().copied());
    }
    for (i, node) in order.iter().enumerate() {
        let k = i.saturating_add(1).to_string();
        let at = |f: &'static str| ["nodes", k.as_str(), f];
        doc.set_i64(&at("id"), i64::from(node.id));
        if let Some(parent) = node.parent {
            doc.set_i64(&at("parent"), i64::from(parent));
        }
        doc.set_str(&at("text"), &node.text);
        doc.set_str(&at("colour"), &colour_hex(node.color));
        doc.set_i64(&at("colour-index"), i64::from(node.color_index));
        doc.set_str(&at("shape"), node.shape.label());
        doc.set_f64(&at("x"), f64::from(node.x));
        doc.set_f64(&at("y"), f64::from(node.y));
        doc.set_f64(&at("width"), f64::from(node.width));
        doc.set_f64(&at("height"), f64::from(node.height));
        doc.set_bool(&at("collapsed"), node.collapsed);
    }
    doc
}

/// A map read back from a document, or why it cannot be.
///
/// Read whole or not at all, for the finance ledger's reason
/// (design-decisions §1202): a map read in part and saved again would lose
/// what was not read. A later format, two nodes with one number, a node whose
/// parent is not written before it, a second root, or a node missing a
/// property this version writes is refused, and the refusal names the node.
fn mindmap_from_document(doc: &yamldoc::Document) -> Result<MindMap, String> {
    match doc.get_i64(&["slateos-mindmap"]) {
        Some(MINDMAP_FORMAT) => {}
        Some(v) if v > MINDMAP_FORMAT => {
            return Err(format!(
                "it is a later format ({v}) than this version reads ({MINDMAP_FORMAT})"
            ));
        }
        Some(v) => return Err(format!("its format ({v}) is not one this version reads")),
        None => return Err(String::from("it does not say which format it is")),
    }
    let name = doc.get_str(&["name"]).unwrap_or_default();
    let mut nodes: HashMap<NodeId, MindMapNode> = HashMap::new();
    let mut root: Option<NodeId> = None;
    for k in positions(doc, &["nodes"]) {
        let at = |f: &'static str| ["nodes", k.as_str(), f];
        let bad = |why: &str| format!("node {k}: {why}");
        // `NodeId::MAX` refused too: the next node added would be given the
        // same number, and take this one's place.
        let id = doc
            .get_i64(&at("id"))
            .and_then(|n| NodeId::try_from(n).ok())
            .filter(|&n| n < NodeId::MAX)
            .ok_or_else(|| bad("its number is missing or not one"))?;
        if nodes.contains_key(&id) {
            return Err(bad(&format!("another node has its number ({id})")));
        }
        let parent = match doc.get_i64(&at("parent")) {
            None if doc.contains(&at("parent")) => {
                return Err(bad("its parent's number is not one"));
            }
            // The first node is the root, and only the first: one written
            // later with no parent would be a second tree.
            None => {
                if root.is_some() {
                    return Err(bad("it is a second node with no parent"));
                }
                root = Some(id);
                None
            }
            Some(p) => {
                let p = NodeId::try_from(p).map_err(|_| bad("its parent's number is not one"))?;
                if !nodes.contains_key(&p) {
                    return Err(bad("its parent is not written before it"));
                }
                Some(p)
            }
        };
        let text = doc
            .get_str(&at("text"))
            .ok_or_else(|| bad("it has no text"))?;
        let shape_label = doc
            .get_str(&at("shape"))
            .ok_or_else(|| bad("it has no shape"))?;
        let shape = NodeShape::all()
            .iter()
            .copied()
            .find(|s| s.label() == shape_label)
            .ok_or_else(|| {
                bad(&format!(
                    "its shape ({shape_label}) is not one this version draws"
                ))
            })?;
        let color = doc
            .get_str(&at("colour"))
            .as_deref()
            .and_then(parse_colour)
            .ok_or_else(|| bad("its colour is missing or not one"))?;
        let color_index = doc
            .get_i64(&at("colour-index"))
            .and_then(|n| u8::try_from(n).ok())
            .filter(|&n| usize::from(n) < NODE_COLORS.len())
            .ok_or_else(|| bad("its colour number is missing or not one"))?;
        let number = |f: &'static str| {
            read_f32(doc, &at(f)).ok_or_else(|| bad(&format!("its {f} is missing or not a number")))
        };
        let size = |f: &'static str| {
            number(f).and_then(|v| {
                if v > 0.0 {
                    Ok(v)
                } else {
                    Err(bad(&format!("its {f} is not above nothing")))
                }
            })
        };
        let collapsed = doc
            .get_bool(&at("collapsed"))
            .ok_or_else(|| bad("whether it is folded is missing"))?;
        let mut node = MindMapNode::new(id, text, parent, color, color_index);
        node.shape = shape;
        node.x = number("x")?;
        node.y = number("y")?;
        node.width = size("width")?;
        node.height = size("height")?;
        node.collapsed = collapsed;
        if let Some(p) = parent
            && let Some(parent_node) = nodes.get_mut(&p)
        {
            parent_node.children.push(id);
        }
        nodes.insert(id, node);
    }
    let root_id = root.ok_or_else(|| String::from("it has no nodes"))?;
    let highest = nodes.keys().copied().max().unwrap_or(0);
    Ok(MindMap {
        name,
        nodes,
        root_id,
        // Past every number the file used, so a node added later cannot take
        // the number of one already there.
        id_gen: IdGenerator {
            next: highest.saturating_add(1),
        },
        id: 0,
        undo_stack: VecDeque::new(),
        redo_stack: Vec::new(),
        document_path: None,
        dirty: false,
    })
}

// ============================================================================
// Radial auto-layout
// ============================================================================

/// Perform radial auto-layout. The root is placed at the center and children
/// fan out to the right (and left for balance). Uses a recursive subtree
/// height measurement to avoid overlaps.
pub fn auto_layout(map: &mut MindMap, center_x: f32, center_y: f32) {
    let root_id = map.root_id;
    if let Some(root) = map.nodes.get_mut(&root_id) {
        root.x = center_x;
        root.y = center_y;
    }

    // Gather children of root
    let root_children: Vec<NodeId> = map
        .nodes
        .get(&root_id)
        .map(|n| n.children.clone())
        .unwrap_or_default();

    if root_children.is_empty() {
        return;
    }

    // Split children: odd-indexed go left, even-indexed go right
    let mut right_children = Vec::new();
    let mut left_children = Vec::new();
    for (i, &child_id) in root_children.iter().enumerate() {
        if i % 2 == 0 {
            right_children.push(child_id);
        } else {
            left_children.push(child_id);
        }
    }

    // Layout right side
    layout_branch(
        map,
        &right_children,
        center_x + ROOT_NODE_W / 2.0 + RADIAL_H_GAP,
        center_y,
        true,
    );
    // Layout left side
    layout_branch(
        map,
        &left_children,
        center_x - ROOT_NODE_W / 2.0 - RADIAL_H_GAP,
        center_y,
        false,
    );
}

/// Measure the total vertical height needed for a subtree.
fn measure_subtree_height(map: &MindMap, node_id: NodeId) -> f32 {
    let node = match map.nodes.get(&node_id) {
        Some(n) => n,
        None => return DEFAULT_NODE_H,
    };

    if node.children.is_empty() || node.collapsed {
        return node.height;
    }

    let mut total = 0.0f32;
    for (i, &child_id) in node.children.iter().enumerate() {
        if i > 0 {
            total += RADIAL_V_GAP;
        }
        total += measure_subtree_height(map, child_id);
    }
    total.max(node.height)
}

/// Layout a branch of children vertically centered around `center_y`.
fn layout_branch(
    map: &mut MindMap,
    children: &[NodeId],
    start_x: f32,
    center_y: f32,
    going_right: bool,
) {
    if children.is_empty() {
        return;
    }

    // Measure total height needed
    let mut total_height = 0.0f32;
    let heights: Vec<f32> = children
        .iter()
        .map(|&cid| measure_subtree_height(map, cid))
        .collect();
    for (i, h) in heights.iter().enumerate() {
        if i > 0 {
            total_height += RADIAL_V_GAP;
        }
        total_height += h;
    }

    let mut current_y = center_y - total_height / 2.0;

    for (i, &child_id) in children.iter().enumerate() {
        let subtree_h = heights.get(i).copied().unwrap_or(0.0);
        let node_y = current_y + subtree_h / 2.0;

        let node_w = map.nodes.get(&child_id).map_or(DEFAULT_NODE_W, |n| n.width);
        let node_x = if going_right {
            start_x + node_w / 2.0
        } else {
            start_x - node_w / 2.0
        };

        if let Some(node) = map.nodes.get_mut(&child_id) {
            node.x = node_x;
            node.y = node_y;
        }

        // Recursively layout grandchildren
        let grandchildren: Vec<NodeId> = map
            .nodes
            .get(&child_id)
            .map(|n| n.children.clone())
            .unwrap_or_default();
        let collapsed = map.nodes.get(&child_id).is_some_and(|n| n.collapsed);

        if !grandchildren.is_empty() && !collapsed {
            let next_x = if going_right {
                node_x + node_w / 2.0 + RADIAL_H_GAP
            } else {
                node_x - node_w / 2.0 - RADIAL_H_GAP
            };
            layout_branch(map, &grandchildren, next_x, node_y, going_right);
        }

        current_y += subtree_h + RADIAL_V_GAP;
    }
}

// ============================================================================
// Drag state
// ============================================================================

/// Tracks drag-in-progress state.
#[derive(Clone, Debug, PartialEq)]
pub enum DragState {
    None,
    /// Dragging a node.
    DraggingNode {
        node_id: NodeId,
        offset_x: f32,
        offset_y: f32,
        start_x: f32,
        start_y: f32,
    },
    /// Panning the canvas.
    Panning {
        start_pan_x: f32,
        start_pan_y: f32,
        start_mouse_x: f32,
        start_mouse_y: f32,
    },
}

// ============================================================================
// Main application state
// ============================================================================

/// The mind map application.
#[derive(Debug)]
pub struct MindMapApp {
    /// The open or save picker. Holds the dialog, the saving flag and the
    /// routing thirteen applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last open or save did, for the status line.
    pub last_file_action: Option<String>,
    /// Window dimensions.
    pub win_width: f32,
    pub win_height: f32,

    /// All mind maps.
    pub maps: Vec<MindMap>,
    /// Index of the currently active map.
    pub active_map: usize,

    /// Global ID generator shared across maps.
    pub id_gen: IdGenerator,

    /// Canvas pan offset.
    pub pan_x: f32,
    pub pan_y: f32,
    /// Zoom level (1.0 = 100%).
    pub zoom: f32,

    /// Currently selected node ID (in the active map).
    pub selected_node: Option<NodeId>,
    /// Current drag state.
    pub drag: DragState,

    /// What the picker is up for, while it is up.
    picker_for: PickerFor,
    /// The unsaved-changes question, while it is being asked, and what it is
    /// about.
    question: Option<Question<CloseScope>>,
    /// Set when the window may go: the question was answered and nothing
    /// unsaved is left.
    quit: bool,
    /// The id the next map is given (see [`MindMap::id`]).
    next_map_id: u64,

    /// Search query.
    pub search_query: String,
    /// Node IDs matching the current search.
    pub search_results: Vec<NodeId>,
    /// Index into search_results for cycling.
    pub search_index: usize,

    /// Whether the sidebar is visible.
    pub show_sidebar: bool,
    /// Whether the shortcut list is up.
    pub show_help: bool,
    /// Whether the search bar is visible.
    pub show_search: bool,

    /// Text input buffer for editing node text.
    pub edit_buffer: String,
    /// Whether we are in text editing mode.
    pub editing_node: Option<NodeId>,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

impl Default for MindMapApp {
    fn default() -> Self {
        Self::new()
    }
}

/// What the picker is up for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerFor {
    /// A file to open in a tab of its own.
    Open,
    /// Where to save map `id`, which then belongs to that file.
    Save(u64),
    /// Where to write the map showing as an outline: not a save, since an
    /// outline keeps the words and the branches and nothing else.
    Export,
    /// Where to save map `id` before its tab closes.
    SaveThenClose(u64),
    /// Where to save map `id` before the window goes on closing.
    SaveThenQuit(u64),
}

/// What the unsaved-changes question would close.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CloseScope {
    /// One map's tab, by [`MindMap::id`].
    Map(u64),
    /// The whole window.
    Window,
}

impl MindMapApp {
    /// Create a new mind map application with default state.
    pub fn new() -> Self {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Mind Map 1".to_string(), &mut id_gen);
        let mut app = Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::new(),
            last_file_action: None,
            win_width: 1280.0,
            win_height: 800.0,
            maps: vec![map],
            active_map: 0,
            id_gen,
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            selected_node: None,
            drag: DragState::None,
            picker_for: PickerFor::Open,
            question: None,
            quit: false,
            next_map_id: 2,
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: 0,
            show_sidebar: true,
            show_help: false,
            show_search: false,
            edit_buffer: String::new(),
            editing_node: None,
        };
        // Auto-layout the initial map
        let cx = app.win_width / 2.0;
        let cy = app.win_height / 2.0;
        if let Some(first) = app.maps.first_mut() {
            auto_layout(first, cx, cy);
            first.id = 1;
        }
        app
    }

    // ========================================================================
    // Map accessors
    // ========================================================================

    /// The palette entry a node at `depth` gets.
    ///
    /// The two callers each did this arithmetic themselves, in slightly
    /// different ways — one `saturating_add`, one `wrapping_add`, both casting
    /// through `u8` and indexing the palette. One function, one cast, and the
    /// index cannot be out of range because the modulo is against the palette's
    /// own length.
    fn color_index_for_depth(depth: u32) -> u8 {
        let n = u32::try_from(NODE_COLORS.len()).unwrap_or(1).max(1);
        // `checked_rem` although `n` is clamped to at least 1: the clamp and
        // the modulo are separate statements that an edit can separate further.
        u8::try_from(depth.saturating_add(1).checked_rem(n).unwrap_or(0)).unwrap_or(0)
    }

    /// The palette entry after `index`, wrapping round.
    fn next_color_index(index: u8) -> u8 {
        let n = u8::try_from(NODE_COLORS.len()).unwrap_or(1).max(1);
        index.wrapping_add(1).checked_rem(n).unwrap_or(0)
    }

    /// The colour at a palette index.
    ///
    /// Falls back to the first entry, which cannot happen for an index this
    /// module produced and is a visible wrong rather than a panic if it ever
    /// does.
    fn color_at(index: u8) -> Color {
        NODE_COLORS
            .get(index as usize)
            .copied()
            .unwrap_or(NODE_COLORS[0])
    }

    /// Get the active mind map.
    /// # Panics
    ///
    /// Never in practice: `active_map` is only ever set to an index that
    /// exists, `maps` is never left empty (`close_map` puts a fresh map in
    /// the place of the last one), and both are private to this type. The alternative — an
    /// `Option` return — would push that same guarantee onto every one of the
    /// hundred-odd call sites, each of which would have to invent a behaviour
    /// for a state that cannot arise.
    #[allow(
        clippy::indexing_slicing,
        reason = "the invariant is stated above and enforced by this module"
    )]
    pub fn active_map_ref(&self) -> &MindMap {
        &self.maps[self.active_map]
    }

    /// Get the active mind map mutably.
    /// # Panics
    ///
    /// Never, for the reason given on [`MindMapApp::active_map_ref`].
    #[allow(
        clippy::indexing_slicing,
        reason = "the invariant is stated on `active_map_ref`"
    )]
    pub fn active_map_mut(&mut self) -> &mut MindMap {
        &mut self.maps[self.active_map]
    }

    // ========================================================================
    // Map management
    // ========================================================================

    /// Add a new empty mind map, and show it.
    pub fn add_map(&mut self) {
        let name = self.fresh_name();
        let mut map = MindMap::new(name, &mut self.id_gen);
        auto_layout(&mut map, self.win_width / 2.0, self.win_height / 2.0);
        self.push_map(map);
    }

    /// "Mind Map N" for the lowest N no open map is called: a count of the
    /// maps would name a second map after one closed before it.
    fn fresh_name(&self) -> String {
        // Of the first `maps.len() + 1` names at least one is free.
        let tries = u64::try_from(self.maps.len())
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        (1..=tries)
            .map(|n| format!("Mind Map {n}"))
            .find(|name| !self.maps.iter().any(|m| &m.name == name))
            .unwrap_or_else(|| String::from("Mind Map"))
    }

    /// Give `map` an id, add it as a tab, and show it.
    fn push_map(&mut self, mut map: MindMap) {
        self.settle();
        map.id = self.next_map_id;
        self.next_map_id = self.next_map_id.saturating_add(1);
        self.maps.push(map);
        self.active_map = self.maps.len().saturating_sub(1);
        self.showing_another_map();
    }

    /// The map showing is now a different one. The selection, a name being
    /// typed, a drag and the search results all name nodes by number, and the
    /// numbers mean something else on another map -- so they are let go, and
    /// an open search is run again on the map now showing.
    fn showing_another_map(&mut self) {
        self.selected_node = None;
        self.editing_node = None;
        self.drag = DragState::None;
        if self.show_search {
            let query = self.search_query.clone();
            self.set_search_query(query);
        }
    }

    /// The tab the map with this id is in.
    fn index_of(&self, id: u64) -> Option<usize> {
        self.maps.iter().position(|m| m.id == id)
    }

    /// The map with this id, to change.
    fn map_mut(&mut self, id: u64) -> Option<&mut MindMap> {
        self.maps.iter_mut().find(|m| m.id == id)
    }

    /// Show the next map (`forward`) or the one before, round from the last
    /// to the first. Whether there was another map to show.
    fn step_map(&mut self, forward: bool) -> bool {
        let n = self.maps.len();
        if n < 2 {
            return false;
        }
        let next = if forward {
            self.active_map
                .saturating_add(1)
                .checked_rem(n)
                .unwrap_or(0)
        } else {
            self.active_map
                .checked_sub(1)
                .unwrap_or(n.saturating_sub(1))
        };
        self.switch_map(next);
        true
    }

    /// Close the map in tab `index`: at once when it has nothing unsaved,
    /// else after asking, with that map showing so it is clear which map the
    /// question is about.
    fn request_close_map(&mut self, index: usize) {
        if index == self.active_map {
            self.settle();
        }
        let Some(map) = self.maps.get(index) else {
            return;
        };
        let id = map.id;
        if map.dirty {
            let message = unsaved::message_for(&[&map.name]);
            self.switch_map(index);
            self.show_help = false;
            self.question = Some(Question::new(
                &message,
                "Save it before the map closes?",
                CloseScope::Map(id),
            ));
        } else {
            self.close_map(id);
        }
    }

    /// Close the map with this id, whatever it holds. The last map is not
    /// taken away -- a window with no map has nothing to draw or add to -- but
    /// replaced with a fresh one.
    fn close_map(&mut self, id: u64) {
        let Some(index) = self.index_of(id) else {
            return;
        };
        let was_showing = index == self.active_map;
        if was_showing {
            // Whatever was half done on it goes with it.
            self.editing_node = None;
            self.drag = DragState::None;
        }
        self.maps.remove(index);
        if self.maps.is_empty() {
            self.active_map = 0;
            self.add_map();
            return;
        }
        // The map showing stays showing; closing it shows its neighbour.
        if index < self.active_map || self.active_map >= self.maps.len() {
            self.active_map = self.active_map.saturating_sub(1);
        }
        if was_showing {
            self.showing_another_map();
        }
    }

    /// Switch to a different map by index. What is half done on the map
    /// showing -- a name being typed, a node being dragged -- is finished on
    /// that map first: the search, the selection and the drag all name nodes
    /// by number, and the numbers mean something else on the next map.
    pub fn switch_map(&mut self, index: usize) {
        if index < self.maps.len() && index != self.active_map {
            self.settle();
            self.active_map = index;
            self.showing_another_map();
        }
    }

    // ========================================================================
    // Coordinate transforms
    // ========================================================================

    /// Convert screen coordinates to canvas coordinates.
    pub fn screen_to_canvas(&self, sx: f32, sy: f32) -> (f32, f32) {
        let cx = (sx - self.canvas_x() - self.pan_x) / self.zoom;
        let cy = (sy - self.canvas_y() - self.pan_y) / self.zoom;
        (cx, cy)
    }

    /// Convert canvas coordinates to screen coordinates.
    pub fn canvas_to_screen(&self, cx: f32, cy: f32) -> (f32, f32) {
        let sx = cx * self.zoom + self.pan_x + self.canvas_x();
        let sy = cy * self.zoom + self.pan_y + self.canvas_y();
        (sx, sy)
    }

    /// X origin of the canvas area.
    fn canvas_x(&self) -> f32 {
        if self.show_sidebar {
            SIDEBAR_WIDTH
        } else {
            0.0
        }
    }

    /// Y origin of the canvas area: below the toolbar and the tabs.
    fn canvas_y(&self) -> f32 {
        TOOLBAR_HEIGHT + TAB_HEIGHT
    }

    /// Width of the canvas area.
    fn canvas_width(&self) -> f32 {
        let sidebar = if self.show_sidebar {
            SIDEBAR_WIDTH
        } else {
            0.0
        };
        (self.win_width - sidebar).max(1.0)
    }

    /// Height of the canvas area.
    fn canvas_height(&self) -> f32 {
        (self.win_height - TOOLBAR_HEIGHT - TAB_HEIGHT - STATUS_BAR_HEIGHT).max(1.0)
    }

    // ========================================================================
    // Zoom
    // ========================================================================

    pub fn zoom_in(&mut self) {
        self.set_zoom(self.zoom * 1.2);
    }

    pub fn zoom_out(&mut self) {
        self.set_zoom(self.zoom / 1.2);
    }

    pub fn set_zoom(&mut self, new_zoom: f32) {
        self.zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    pub fn reset_view(&mut self) {
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.zoom = 1.0;
    }

    // ========================================================================
    // Node operations (with undo support)
    // ========================================================================

    /// Add a child to the selected node (or root if nothing selected).
    pub fn add_child_to_selected(&mut self, text: String) -> Option<NodeId> {
        let parent_id = self.selected_node.unwrap_or(self.active_map_ref().root_id);
        let color_index = Self::color_index_for_depth(self.active_map_ref().depth(parent_id));
        let color = Self::color_at(color_index);

        let new_id =
            self.active_map_mut()
                .add_child(parent_id, text.clone(), color, color_index)?;

        self.push_undo(Action::AddNode {
            node_id: new_id,
            parent_id: Some(parent_id),
            text,
            color,
            color_index,
            shape: NodeShape::RoundedRect,
        });

        self.relayout();
        self.selected_node = Some(new_id);
        Some(new_id)
    }

    /// Add a sibling after the selected node.
    pub fn add_sibling_to_selected(&mut self, text: String) -> Option<NodeId> {
        let sel = self.selected_node?;
        // Cannot add sibling to root
        if sel == self.active_map_ref().root_id {
            return None;
        }

        let color_index = self
            .active_map_ref()
            .node(sel)
            .map(|n| n.color_index)
            .unwrap_or(0);
        let color = Self::color_at(color_index);

        let parent_id = self.active_map_ref().node(sel)?.parent;
        let new_id = self
            .active_map_mut()
            .add_sibling(sel, text.clone(), color, color_index)?;

        self.push_undo(Action::AddNode {
            node_id: new_id,
            parent_id,
            text,
            color,
            color_index,
            shape: NodeShape::RoundedRect,
        });

        self.relayout();
        self.selected_node = Some(new_id);
        Some(new_id)
    }

    /// Delete the selected node and its subtree.
    pub fn delete_selected(&mut self) -> bool {
        let sel = match self.selected_node {
            Some(id) => id,
            None => return false,
        };
        if sel == self.active_map_ref().root_id {
            return false; // can't delete root
        }

        if let Some((nodes, parent_id, child_index)) = self.active_map_mut().delete_subtree(sel) {
            self.push_undo(Action::DeleteSubtree {
                nodes,
                parent_id,
                child_index,
            });
            self.selected_node = parent_id;
            self.relayout();
            true
        } else {
            false
        }
    }

    /// Start editing the selected node's text.
    pub fn start_editing(&mut self) {
        if let Some(sel) = self.selected_node
            && let Some(node) = self.active_map_ref().node(sel)
        {
            self.edit_buffer = node.text.clone();
            self.editing_node = Some(sel);
        }
    }

    /// Finish editing and apply text change.
    pub fn finish_editing(&mut self) {
        if let Some(node_id) = self.editing_node.take() {
            let new_text = self.edit_buffer.clone();
            if let Some(old_text) = self.active_map_mut().edit_text(node_id, new_text.clone())
                && old_text != new_text
            {
                self.push_undo(Action::EditText {
                    node_id,
                    old_text,
                    new_text,
                });
            }
        }
    }

    /// Cancel editing without applying changes.
    pub fn cancel_editing(&mut self) {
        self.editing_node = None;
        self.edit_buffer.clear();
    }

    /// Cycle the selected node's color to the next preset.
    pub fn cycle_color(&mut self) {
        if let Some(sel) = self.selected_node {
            let old_index = self
                .active_map_ref()
                .node(sel)
                .map(|n| n.color_index)
                .unwrap_or(0);
            let new_index = Self::next_color_index(old_index);
            let new_color = Self::color_at(new_index);

            if let Some((old_color, oi)) = self
                .active_map_mut()
                .change_color(sel, new_color, new_index)
            {
                self.push_undo(Action::ChangeColor {
                    node_id: sel,
                    old_color,
                    old_index: oi,
                    new_color,
                    new_index,
                });
            }
        }
    }

    /// Cycle the selected node's shape.
    pub fn cycle_shape(&mut self) {
        if let Some(sel) = self.selected_node {
            let old_shape = self
                .active_map_ref()
                .node(sel)
                .map(|n| n.shape)
                .unwrap_or(NodeShape::RoundedRect);
            let new_shape = old_shape.next();

            if let Some(os) = self.active_map_mut().change_shape(sel, new_shape) {
                self.push_undo(Action::ChangeShape {
                    node_id: sel,
                    old_shape: os,
                    new_shape,
                });
            }
        }
    }

    /// Toggle collapse/expand on the selected node.
    pub fn toggle_collapse_selected(&mut self) {
        if let Some(sel) = self.selected_node {
            self.active_map_mut().toggle_collapse(sel);
            self.push_undo(Action::ToggleCollapse { node_id: sel });
            self.relayout();
        }
    }

    // ========================================================================
    // Undo / Redo
    // ========================================================================

    /// Record a change to the map showing, in that map's own history -- and
    /// mark the map changed. Every change a user makes is recorded here,
    /// which is what lets this be the one place that marks.
    fn push_undo(&mut self, action: Action) {
        let map = self.active_map_mut();
        map.dirty = true;
        map.redo_stack.clear();
        if map.undo_stack.len() >= MAX_UNDO {
            map.undo_stack.pop_front();
        }
        map.undo_stack.push_back(action);
    }

    /// Undo the last change to the map showing: that map's, from that map's
    /// own history, whatever was done to other maps in between.
    pub fn undo(&mut self) {
        if let Some(action) = self.active_map_mut().undo_stack.pop_back() {
            self.apply_reverse(&action);
            let map = self.active_map_mut();
            map.redo_stack.push(action);
            // Undoing past a save leaves a map its file does not hold.
            map.dirty = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some(action) = self.active_map_mut().redo_stack.pop() {
            self.apply_forward(&action);
            let map = self.active_map_mut();
            map.undo_stack.push_back(action);
            map.dirty = true;
        }
    }

    fn apply_reverse(&mut self, action: &Action) {
        match action {
            Action::AddNode { node_id, .. } => {
                // Reverse of add: delete
                self.active_map_mut().delete_subtree(*node_id);
                if self.selected_node == Some(*node_id) {
                    self.selected_node = None;
                }
                self.relayout();
            }
            Action::DeleteSubtree {
                nodes,
                parent_id,
                child_index,
            } => {
                // Reverse of delete: restore
                self.active_map_mut()
                    .restore_subtree(nodes, *parent_id, *child_index);
                self.relayout();
            }
            Action::EditText {
                node_id, old_text, ..
            } => {
                self.active_map_mut().edit_text(*node_id, old_text.clone());
            }
            Action::ChangeColor {
                node_id,
                old_color,
                old_index,
                ..
            } => {
                self.active_map_mut()
                    .change_color(*node_id, *old_color, *old_index);
            }
            Action::ChangeShape {
                node_id, old_shape, ..
            } => {
                self.active_map_mut().change_shape(*node_id, *old_shape);
            }
            Action::MoveNode {
                node_id,
                old_x,
                old_y,
                ..
            } => {
                self.active_map_mut().move_node(*node_id, *old_x, *old_y);
            }
            Action::ToggleCollapse { node_id } => {
                self.active_map_mut().toggle_collapse(*node_id);
                self.relayout();
            }
        }
    }

    fn apply_forward(&mut self, action: &Action) {
        match action {
            Action::AddNode {
                node_id,
                parent_id,
                text,
                color,
                color_index,
                shape,
            } => {
                // Restore under the original ID so later actions that reference
                // this node as a parent still resolve after a redo.
                if let Some(pid) = parent_id {
                    self.active_map_mut().add_child_with_id(
                        *node_id,
                        *pid,
                        text.clone(),
                        *color,
                        *color_index,
                        *shape,
                    );
                }
                self.relayout();
            }
            Action::DeleteSubtree { nodes, .. } => {
                if let Some(first) = nodes.first() {
                    self.active_map_mut().delete_subtree(first.id);
                    self.relayout();
                }
            }
            Action::EditText {
                node_id, new_text, ..
            } => {
                self.active_map_mut().edit_text(*node_id, new_text.clone());
            }
            Action::ChangeColor {
                node_id,
                new_color,
                new_index,
                ..
            } => {
                self.active_map_mut()
                    .change_color(*node_id, *new_color, *new_index);
            }
            Action::ChangeShape {
                node_id, new_shape, ..
            } => {
                self.active_map_mut().change_shape(*node_id, *new_shape);
            }
            Action::MoveNode {
                node_id,
                new_x,
                new_y,
                ..
            } => {
                self.active_map_mut().move_node(*node_id, *new_x, *new_y);
            }
            Action::ToggleCollapse { node_id } => {
                self.active_map_mut().toggle_collapse(*node_id);
                self.relayout();
            }
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.active_map_ref().undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.active_map_ref().redo_stack.is_empty()
    }

    // ========================================================================
    // Layout
    // ========================================================================

    /// Re-run auto-layout on the active map.
    pub fn relayout(&mut self) {
        let cx = self.win_width / 2.0;
        let cy = self.win_height / 2.0;
        auto_layout(self.active_map_mut(), cx, cy);
    }

    /// L, or the Layout button: lay the map out afresh -- a change to be
    /// saved when it moves anything, and not one when every node was already
    /// where the layout puts it.
    pub fn lay_out_again(&mut self) {
        let before: HashMap<NodeId, (f32, f32)> = self
            .active_map_ref()
            .nodes
            .iter()
            .map(|(id, n)| (*id, (n.x, n.y)))
            .collect();
        self.relayout();
        let moved = self
            .active_map_ref()
            .nodes
            .iter()
            .any(|(id, n)| before.get(id) != Some(&(n.x, n.y)));
        if moved {
            self.active_map_mut().dirty = true;
        }
    }

    /// Do what a toolbar button does.
    fn toolbar(&mut self, action: ToolbarAction) -> EventResult {
        match action {
            ToolbarAction::Open => {
                self.picker_for = PickerFor::Open;
                self.picker.open_to_read();
            }
            ToolbarAction::Save => self.save(),
            ToolbarAction::AddChild => {
                if self
                    .add_child_to_selected(String::from("New Node"))
                    .is_none()
                {
                    return EventResult::Ignored;
                }
            }
            ToolbarAction::AddSibling => {
                if self
                    .add_sibling_to_selected(String::from("New Node"))
                    .is_none()
                {
                    return EventResult::Ignored;
                }
            }
            ToolbarAction::Delete => {
                if !self.delete_selected() {
                    return EventResult::Ignored;
                }
            }
            ToolbarAction::Undo => {
                if !self.can_undo() {
                    return EventResult::Ignored;
                }
                self.undo();
            }
            ToolbarAction::Redo => {
                if !self.can_redo() {
                    return EventResult::Ignored;
                }
                self.redo();
            }
            ToolbarAction::Layout => self.lay_out_again(),
            ToolbarAction::ZoomIn => self.zoom_in(),
            ToolbarAction::ZoomOut => self.zoom_out(),
        }
        EventResult::Consumed
    }

    /// How wide each tab is drawn: as wide as it may be, narrower when that
    /// would push the "+" out of the window.
    fn tab_width(&self) -> f32 {
        let n = self.maps.len().max(1) as f32;
        let room = self.win_width - TAB_FIRST_X - PLUS_W - TAB_GAP - 10.0;
        (room / n - TAB_GAP).clamp(TAB_MIN_W, TAB_MAX_W)
    }

    /// Tab `i`'s rectangle: x, y, width, height. Tab `maps.len()` is where
    /// the "+" goes.
    fn tab_rect(&self, i: usize) -> (f32, f32, f32, f32) {
        let w = self.tab_width();
        (
            TAB_FIRST_X + i as f32 * (w + TAB_GAP),
            TOOLBAR_HEIGHT + 2.0,
            w,
            TAB_HEIGHT - 2.0,
        )
    }

    /// Tab `i`'s close mark.
    fn tab_close_rect(&self, i: usize) -> (f32, f32, f32, f32) {
        let (x, y, w, h) = self.tab_rect(i);
        (
            x + w - TAB_CLOSE_W - 2.0,
            y + (h - TAB_CLOSE_W) / 2.0,
            TAB_CLOSE_W,
            TAB_CLOSE_W,
        )
    }

    /// The "+" button, after the last tab.
    fn plus_rect(&self) -> (f32, f32, f32, f32) {
        let (x, _, _, _) = self.tab_rect(self.maps.len());
        (x, TOOLBAR_HEIGHT + 4.0, PLUS_W, 20.0)
    }

    /// A click on the toolbar or the tab strip: always on a button or on
    /// nothing, never on the map.
    fn click_above_canvas(&mut self, x: f32, y: f32) -> EventResult {
        if y < TOOLBAR_HEIGHT {
            return match toolbar_action_at(x, y) {
                Some(action) => self.toolbar(action),
                None => EventResult::Ignored,
            };
        }
        if inside(self.plus_rect(), x, y) {
            self.add_map();
            return EventResult::Consumed;
        }
        for i in 0..self.maps.len() {
            // The close mark first: it is inside the tab.
            if inside(self.tab_close_rect(i), x, y) {
                self.request_close_map(i);
                return EventResult::Consumed;
            }
            if inside(self.tab_rect(i), x, y) {
                if i == self.active_map {
                    return EventResult::Ignored;
                }
                self.switch_map(i);
                return EventResult::Consumed;
            }
        }
        EventResult::Ignored
    }

    // ========================================================================
    // Search
    // ========================================================================

    pub fn set_search_query(&mut self, query: String) {
        self.search_query = query;
        self.search_results = self.active_map_ref().search(&self.search_query);
        self.search_index = 0;
    }

    pub fn next_search_result(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        // `checked_rem` although the emptiness test is three lines up: the
        // guard and the modulo are separate statements that an edit can
        // separate further.
        self.search_index = self
            .search_index
            .wrapping_add(1)
            .checked_rem(self.search_results.len())
            .unwrap_or(0);
        self.selected_node = self.search_results.get(self.search_index).copied();
    }

    pub fn prev_search_result(&mut self) {
        if self.search_results.is_empty() {
            return;
        }
        if self.search_index == 0 {
            self.search_index = self.search_results.len().saturating_sub(1);
        } else {
            self.search_index = self.search_index.saturating_sub(1);
        }
        self.selected_node = self.search_results.get(self.search_index).copied();
    }

    /// Toggle search bar visibility.
    pub fn toggle_search(&mut self) {
        self.show_search = !self.show_search;
        if !self.show_search {
            self.search_query.clear();
            self.search_results.clear();
            self.search_index = 0;
        }
    }

    // ========================================================================
    // Hit testing
    // ========================================================================

    /// Find which node is at a given canvas-space point.
    pub fn hit_test_canvas(&self, cx: f32, cy: f32) -> Option<NodeId> {
        let visible = self.active_map_ref().visible_node_ids();
        // Test in reverse order so topmost (last rendered) nodes are hit first
        for &id in visible.iter().rev() {
            if let Some(node) = self.active_map_ref().node(id)
                && node.contains(cx, cy)
            {
                return Some(id);
            }
        }
        None
    }

    /// Find which node is at a screen-space point.
    pub fn hit_test_screen(&self, sx: f32, sy: f32) -> Option<NodeId> {
        let (cx, cy) = self.screen_to_canvas(sx, sy);
        self.hit_test_canvas(cx, cy)
    }

    // ========================================================================
    // Drag operations
    // ========================================================================

    /// Begin dragging a node.
    pub fn start_node_drag(&mut self, node_id: NodeId, mouse_x: f32, mouse_y: f32) {
        if let Some(node) = self.active_map_ref().node(node_id) {
            self.drag = DragState::DraggingNode {
                node_id,
                offset_x: mouse_x - node.x,
                offset_y: mouse_y - node.y,
                start_x: node.x,
                start_y: node.y,
            };
        }
    }

    /// Begin canvas panning.
    pub fn start_pan(&mut self, mouse_x: f32, mouse_y: f32) {
        self.drag = DragState::Panning {
            start_pan_x: self.pan_x,
            start_pan_y: self.pan_y,
            start_mouse_x: mouse_x,
            start_mouse_y: mouse_y,
        };
    }

    /// Update drag position.
    pub fn update_drag(&mut self, mouse_x: f32, mouse_y: f32) {
        match self.drag.clone() {
            DragState::DraggingNode {
                node_id,
                offset_x,
                offset_y,
                ..
            } => {
                let new_x = mouse_x - offset_x;
                let new_y = mouse_y - offset_y;
                self.active_map_mut().move_node(node_id, new_x, new_y);
            }
            DragState::Panning {
                start_pan_x,
                start_pan_y,
                start_mouse_x,
                start_mouse_y,
            } => {
                self.pan_x = start_pan_x + (mouse_x - start_mouse_x);
                self.pan_y = start_pan_y + (mouse_y - start_mouse_y);
            }
            DragState::None => {}
        }
    }

    /// End drag operation.
    pub fn end_drag(&mut self) {
        if let DragState::DraggingNode {
            node_id,
            start_x,
            start_y,
            ..
        } = self.drag
            && let Some(node) = self.active_map_ref().node(node_id)
        {
            let (new_x, new_y) = (node.x, node.y);
            if (new_x - start_x).abs() > 0.1 || (new_y - start_y).abs() > 0.1 {
                self.push_undo(Action::MoveNode {
                    node_id,
                    old_x: start_x,
                    old_y: start_y,
                    new_x,
                    new_y,
                });
            }
        }
        self.drag = DragState::None;
    }

    // ========================================================================
    // Selection navigation
    // ========================================================================

    /// Select the parent of the currently selected node.
    pub fn select_parent(&mut self) {
        if let Some(sel) = self.selected_node
            && let Some(node) = self.active_map_ref().node(sel)
            && let Some(pid) = node.parent
        {
            self.selected_node = Some(pid);
        }
    }

    /// Select the first child of the currently selected node.
    pub fn select_first_child(&mut self) {
        if let Some(sel) = self.selected_node
            && let Some(node) = self.active_map_ref().node(sel)
            && let Some(&first) = node.children.first()
            && !node.collapsed
        {
            self.selected_node = Some(first);
        }
    }

    /// Select the next sibling.
    pub fn select_next_sibling(&mut self) {
        if let Some(sel) = self.selected_node
            && let Some(node) = self.active_map_ref().node(sel)
            && let Some(pid) = node.parent
            && let Some(parent) = self.active_map_ref().node(pid)
            && let Some(pos) = parent.children.iter().position(|&c| c == sel)
            && let Some(next) = pos.checked_add(1).and_then(|n| parent.children.get(n))
        {
            self.selected_node = Some(*next);
        }
    }

    /// Select the previous sibling.
    pub fn select_prev_sibling(&mut self) {
        if let Some(sel) = self.selected_node
            && let Some(node) = self.active_map_ref().node(sel)
            && let Some(pid) = node.parent
            && let Some(parent) = self.active_map_ref().node(pid)
            && let Some(pos) = parent.children.iter().position(|&c| c == sel)
            && let Some(prev) = pos.checked_sub(1).and_then(|n| parent.children.get(n))
        {
            self.selected_node = Some(*prev);
        }
    }

    // ========================================================================
    // Export
    // ========================================================================

    /// Export the active map as indented text.
    pub fn export_text(&self) -> String {
        self.active_map_ref().export_text()
    }

    // ========================================================================
    // Files
    // ========================================================================

    /// Write the map showing to `path` as an indented outline -- an export,
    /// not a save: the map keeps its own file and its unsaved mark.
    ///
    /// **Says what the format does not carry**, and there are two different
    /// losses. Colours, shapes, positions and which branches are folded are
    /// not in an outline at all. And a node whose text contains a line break
    /// is written with that break turned into a space, because the format puts
    /// one node on one line -- a silent flattening looks exactly like a node
    /// that was typed on one line.
    pub fn write_outline(&mut self, path: &Path) -> String {
        let map = self.active_map_ref();
        let text = map.export_text();
        let flattened = map
            .nodes
            .values()
            .filter(|n| n.text.chars().any(|c| c.is_control() || c == '\t'))
            .count();
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => {
                let note = if flattened > 0 {
                    format!(
                        "; {flattened} node(s) had line breaks, which an outline stores as spaces"
                    )
                } else {
                    String::new()
                };
                format!(
                    "Wrote {} node(s) to {}{note}. An outline keeps the words and the branches, not the colours, shapes or folds -- Ctrl+S saves those.",
                    map.nodes.len(),
                    path.display()
                )
            }
            Err(err) => format!("Could not write {}: {err}", path.display()),
        }
    }

    /// Open `path` in a tab of its own: a mind map file whole, or an
    /// outline's words and branches as a new map. What to say.
    ///
    /// Which of the two is decided by what the file holds, not by its name:
    /// a name is a claim by whoever gave it, the content is the fact. A mind
    /// map file names its format in its first key; an outline is lines of
    /// text.
    pub fn open_file(&mut self, path: &Path) -> String {
        self.open_file_within(path, MAX_OPEN_BYTES)
    }

    /// [`open_file`](Self::open_file), reading at most `max` bytes.
    fn open_file_within(&mut self, path: &Path, max: usize) -> String {
        // One file, one tab: two tabs on one file would each save over the
        // other's changes.
        if let Some(index) = self.maps.iter().position(|m| {
            m.document_path
                .as_deref()
                .is_some_and(|p| same_file(p, path))
        }) {
            self.switch_map(index);
            return format!("{} is open already -- this is it", path.display());
        }
        let read = match safeio::read_to_string_capped(path, max) {
            Ok(read) => read,
            Err(err) => return format!("Could not open {}: {err}", path.display()),
        };
        let doc = yamldoc::Document::parse(&read.text);
        if !doc.contains(&["slateos-mindmap"]) {
            return self.import_outline(path, read.to_last_line(), max);
        }
        if read.truncated {
            return format!(
                "Could not open {}: at {} bytes it is larger than the {max} this reads",
                path.display(),
                read.whole
            );
        }
        match mindmap_from_document(&doc) {
            Ok(mut map) => {
                if map.name.trim().is_empty() {
                    map.name = path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .filter(|s| !s.is_empty())
                        .map_or_else(|| self.fresh_name(), str::to_owned);
                }
                map.document_path = Some(path.to_path_buf());
                self.push_map(map);
                format!("Opened {}", path.display())
            }
            Err(why) => format!("Could not open {}: {why}", path.display()),
        }
    }

    /// An outline read from `path`, opened as a new map with no file of its
    /// own -- saving it asks where, so the outline is never written over with
    /// a different format. What to say.
    ///
    /// The words and the branches come back; the nodes are laid out afresh by
    /// `auto_layout`. What an outline cannot return is stated rather than left
    /// to be noticed: colour, shape and collapse state are not in the file.
    fn import_outline(&mut self, path: &Path, read: safeio::CappedRead, max: usize) -> String {
        let note = read.note(max);
        let title = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.is_empty())
            .map_or_else(|| String::from("Imported map"), str::to_owned);
        let Some(mut map) = MindMap::from_outline(&title, &read.text, &mut self.id_gen) else {
            return format!(
                "{note}{} has no outline in it -- every line was blank",
                path.display()
            );
        };
        let count = map.nodes.len();
        auto_layout(&mut map, self.win_width / 2.0, self.win_height / 2.0);
        self.push_map(map);
        format!(
            "{note}Opened {count} node(s) from {} as a new map. Colours, shapes and folded branches are not in an outline.",
            path.display()
        )
    }

    /// Write map `id` to `path`, which becomes its file. What to say: `Ok`
    /// once it is written, `Err` if it is not.
    fn write_map(&mut self, id: u64, path: &Path) -> Result<String, String> {
        let Some(map) = self.map_mut(id) else {
            return Err(String::from("Not saved: that map is closed"));
        };
        match safeio::write_str_atomically(path, &mindmap_document(map).to_text()) {
            Ok(()) => {
                map.document_path = Some(path.to_path_buf());
                map.dirty = false;
                Ok(format!("Saved {}", path.display()))
            }
            Err(err) => Err(format!("Could not save {}: {err}", path.display())),
        }
    }

    /// Save map `id` to a file the picker chose, the map taking the file's
    /// name as its own -- the name on its tab is the name it is found by.
    fn save_map_as(&mut self, id: u64, path: &Path) -> Result<String, String> {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned);
        let Some(map) = self.map_mut(id) else {
            return Err(String::from("Not saved: that map is closed"));
        };
        let old_name = map.name.clone();
        if let Some(stem) = stem {
            map.name = stem;
        }
        let result = self.write_map(id, path);
        if result.is_err()
            && let Some(map) = self.map_mut(id)
        {
            // Not saved, so not renamed either.
            map.name = old_name;
        }
        result
    }

    /// Ctrl+S: the map showing, over its own file, or wherever the picker
    /// says when it has none.
    pub fn save(&mut self) {
        self.settle();
        let map = self.active_map_ref();
        let id = map.id;
        match map.document_path.clone() {
            Some(path) => {
                let (Ok(said) | Err(said)) = self.write_map(id, &path);
                self.last_file_action = Some(said);
            }
            None => self.ask_where_to_save(PickerFor::Save(id)),
        }
    }

    /// Put the save picker up for `purpose`, beside the map's own file when
    /// it has one, offering its name.
    fn ask_where_to_save(&mut self, purpose: PickerFor) {
        let (id, extension) = match purpose {
            PickerFor::Save(id) | PickerFor::SaveThenClose(id) | PickerFor::SaveThenQuit(id) => {
                (id, ".mindmap")
            }
            PickerFor::Export => (self.active_map_ref().id, ".outline"),
            PickerFor::Open => return,
        };
        let Some(map) = self.maps.iter().find(|m| m.id == id) else {
            return;
        };
        let start = map
            .document_path
            .as_deref()
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(FilePicker::default_start, Path::to_path_buf);
        // The file's own name when it has one -- as bytes, since a name that
        // is not text is still the file's name -- else the map's.
        let mut name = map
            .document_path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(
                || std::ffi::OsString::from(sanitise_map_name(&map.name)),
                std::ffi::OsString::from,
            );
        name.push(extension);
        self.picker_for = purpose;
        self.picker.put_up(
            guitk::dialog::FileDialog::save()
                .with_initial_path(start)
                .with_filename(name),
            true,
        );
    }

    /// The picker chose `path`: do what it was put up for.
    fn picked(&mut self, path: &Path) {
        let said = match self.picker_for {
            PickerFor::Open => self.open_file(path),
            PickerFor::Export => self.write_outline(path),
            PickerFor::Save(id) => {
                let (Ok(said) | Err(said)) = self.save_map_as(id, path);
                said
            }
            PickerFor::SaveThenClose(id) => match self.save_map_as(id, path) {
                Ok(said) => {
                    self.close_map(id);
                    said
                }
                Err(said) => said,
            },
            PickerFor::SaveThenQuit(id) => match self.save_map_as(id, path) {
                Ok(said) => {
                    self.last_file_action = Some(said);
                    self.continue_quitting();
                    return;
                }
                Err(said) => format!("{said} -- so the window stays open"),
            },
        };
        self.last_file_action = Some(said);
    }

    /// What is half done becomes part of the map before the map is saved,
    /// asked about or left: text being typed into a node, a node being
    /// dragged.
    fn settle(&mut self) {
        if self.editing_node.is_some() {
            self.finish_editing();
        }
        if matches!(self.drag, DragState::DraggingNode { .. }) {
            self.end_drag();
        }
    }

    /// The window has been asked to close. Whether it may go now; if not, the
    /// question is up.
    fn request_quit(&mut self) -> bool {
        self.settle();
        let names: Vec<&str> = self
            .maps
            .iter()
            .filter(|m| m.dirty)
            .map(|m| m.name.as_str())
            .collect();
        if names.is_empty() {
            self.quit = true;
            return true;
        }
        let message = unsaved::message_for(&names);
        // The question replaces whatever was up: a picker left open would
        // take the keys the question needs, and be drawn over it.
        self.picker.close();
        self.show_help = false;
        self.question = Some(Question::new(
            &message,
            "Save them before closing?",
            CloseScope::Window,
        ));
        false
    }

    /// Answer the close question put before `scope`. Save goes on only if the
    /// save worked: a map that could not be written is still the only copy.
    fn answer_close(&mut self, scope: CloseScope, choice: Choice) {
        match (scope, choice) {
            (_, Choice::Cancel) => {}
            (CloseScope::Map(id), Choice::Discard) => self.close_map(id),
            (CloseScope::Map(id), Choice::Save) => {
                let own = self
                    .maps
                    .iter()
                    .find(|m| m.id == id)
                    .and_then(|m| m.document_path.clone());
                match own {
                    Some(path) => {
                        let said = match self.write_map(id, &path) {
                            Ok(said) => {
                                self.close_map(id);
                                said
                            }
                            Err(said) => format!("{said} -- so the map stays open"),
                        };
                        self.last_file_action = Some(said);
                    }
                    None => self.ask_where_to_save(PickerFor::SaveThenClose(id)),
                }
            }
            (CloseScope::Window, Choice::Discard) => self.quit = true,
            (CloseScope::Window, Choice::Save) => self.continue_quitting(),
        }
    }

    /// Carry on closing the window: save every map that has a file of its
    /// own over it, ask where to put the first that has none, and quit once
    /// nothing is left unsaved. A failed save stops it, with the window open
    /// and the failure on the status line.
    fn continue_quitting(&mut self) {
        let own_files: Vec<(u64, PathBuf)> = self
            .maps
            .iter()
            .filter(|m| m.dirty)
            .filter_map(|m| m.document_path.clone().map(|p| (m.id, p)))
            .collect();
        for (id, path) in own_files {
            if let Err(said) = self.write_map(id, &path) {
                self.last_file_action = Some(format!("{said} -- so the window stays open"));
                return;
            }
        }
        match self.maps.iter().find(|m| m.dirty).map(|m| m.id) {
            Some(id) => {
                if let Some(index) = self.index_of(id) {
                    self.switch_map(index);
                }
                self.ask_where_to_save(PickerFor::SaveThenQuit(id));
            }
            None => self.quit = true,
        }
    }

    // ========================================================================
    // Events
    // ========================================================================

    /// Route a compositor event into the app.
    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The unsaved-changes question has every key and click while it is
        // up: a key that reached the map under it would be a change made
        // while being asked whether to keep the changes. Each one is a
        // redraw, since focus and hover move inside it.
        if let Some(question) = self.question.as_mut()
            && matches!(event, Event::Key(_) | Event::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                let scope = question.pending();
                self.question = None;
                self.answer_close(scope, choice);
            }
            return EventResult::Consumed;
        }
        // The picker takes input first while it is up, or a filename is typed
        // into the node being edited behind it.
        match self.picker.handle(event, self.win_width, self.win_height) {
            Picked::Chose(path) => {
                self.picked(&path);
                return EventResult::Consumed;
            }
            // Cancelled grouped with Handled: this caller keeps no dialog
            // state of its own that could go stale.
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse_ev) => self.handle_mouse(mouse_ev),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.win_width = *width as f32;
                    self.win_height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a mouse event.
    ///
    /// Screen coordinates arrive here and canvas coordinates leave: the hit
    /// test and the drag routines work in canvas space, and mixing the two is
    /// how a node jumps across the map the moment it is grabbed at any zoom
    /// other than 1.0.
    pub fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        if matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            if ev.y < self.canvas_y() {
                return self.click_above_canvas(ev.x, ev.y);
            }
            // The sidebar and the status line hold nothing to click. A press
            // there was taken as one on empty canvas: it dropped the
            // selection and started a pan.
            if ev.x < self.canvas_x() || ev.y >= self.canvas_y() + self.canvas_height() {
                return EventResult::Ignored;
            }
        }
        let (cx, cy) = self.screen_to_canvas(ev.x, ev.y);
        match ev.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some(node_id) = self.hit_test_canvas(cx, cy) {
                    self.selected_node = Some(node_id);
                    self.start_node_drag(node_id, cx, cy);
                } else {
                    // Empty canvas: a drag is a pan, and the click clears the
                    // selection, which is what makes "click away to deselect"
                    // work.
                    self.selected_node = None;
                    self.start_pan(cx, cy);
                }
                EventResult::Consumed
            }
            MouseEventKind::Move => {
                if matches!(self.drag, DragState::None) {
                    // Not dragging: a bare pointer move changes nothing, and
                    // answering `Consumed` would redraw the map on every pixel
                    // the mouse crosses.
                    return EventResult::Ignored;
                }
                self.update_drag(cx, cy);
                EventResult::Consumed
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if matches!(self.drag, DragState::None) {
                    return EventResult::Ignored;
                }
                self.end_drag();
                EventResult::Consumed
            }
            MouseEventKind::Scroll { dy, .. } => {
                if dy > 0.0 {
                    self.zoom_in();
                } else if dy < 0.0 {
                    self.zoom_out();
                } else {
                    return EventResult::Ignored;
                }
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// While a node's text is being edited every printable key is text, which
    /// is why the editing branch comes first: otherwise typing "d" into a node
    /// name would delete the node.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        if self.editing_node.is_some() {
            return self.handle_key_editing(key);
        }
        if self.show_search {
            return self.handle_key_search(key);
        }
        let ctrl = key.modifiers.ctrl;
        match key.key {
            // `show_sidebar` gates the sidebar's draw and had no writer, so
            // the panel could never be closed. Safe as a plain key: the
            // editing and search modes above return before reaching here, so
            // this cannot swallow a letter meant for a node's text.
            Key::B if !ctrl => {
                self.show_sidebar = !self.show_sidebar;
                EventResult::Consumed
            }
            // `?`, which is Shift and the slash key. Escape closes it, because
            // that is what Escape means over anything laid on top.
            // The shortcut list. `F1` raises it in every app in this tree,
            // including `apps/spreadsheet`, where `?` is a character the
            // program has to be able to type into a cell -- so somebody who
            // has learned one key is never stuck. `?` as well, wherever the
            // program is not obliged to type one.
            Key::F1 => {
                self.show_help = !self.show_help;
                EventResult::Consumed
            }
            Key::Slash if key.modifiers.shift => {
                self.show_help = !self.show_help;
                EventResult::Consumed
            }
            Key::Escape if self.show_help => {
                self.show_help = false;
                EventResult::Consumed
            }
            // Files. Save As before Save: the one with Shift is the more
            // particular, and would never be reached after.
            Key::S if ctrl && key.modifiers.shift => {
                self.settle();
                let id = self.active_map_ref().id;
                self.ask_where_to_save(PickerFor::Save(id));
                EventResult::Consumed
            }
            Key::S if ctrl => {
                self.save();
                EventResult::Consumed
            }
            Key::O if ctrl => {
                self.picker_for = PickerFor::Open;
                self.picker.open_to_read();
                EventResult::Consumed
            }
            Key::E if ctrl => {
                self.settle();
                self.ask_where_to_save(PickerFor::Export);
                EventResult::Consumed
            }
            // Maps.
            Key::N if ctrl => {
                self.add_map();
                EventResult::Consumed
            }
            Key::W if ctrl => {
                self.request_close_map(self.active_map);
                EventResult::Consumed
            }
            // Before plain Tab, which adds a child.
            Key::Tab if ctrl => {
                if self.step_map(!key.modifiers.shift) {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // Structure.
            Key::Tab => {
                self.add_child_to_selected(String::from("New Node"));
                EventResult::Consumed
            }
            Key::Enter => {
                self.add_sibling_to_selected(String::from("New Node"));
                EventResult::Consumed
            }
            Key::Delete => {
                if self.delete_selected() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Key::F2 => {
                self.start_editing();
                if self.editing_node.is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            // Selection, along the tree rather than the screen: a mind map's
            // "up" is its parent, which is what makes arrow navigation useful
            // on a radial layout where screen-up is meaningless.
            Key::Up => self.reselect(Self::select_parent),
            Key::Down => self.reselect(Self::select_first_child),
            Key::Right => self.reselect(Self::select_next_sibling),
            Key::Left => self.reselect(Self::select_prev_sibling),
            // View.
            Key::Equals => {
                self.zoom_in();
                EventResult::Consumed
            }
            Key::Minus => {
                self.zoom_out();
                EventResult::Consumed
            }
            Key::Num0 if ctrl => {
                self.reset_view();
                EventResult::Consumed
            }
            // Undo and redo, which the app already tracks and had no way to
            // reach.
            Key::Z if ctrl => {
                if self.can_undo() {
                    self.undo();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Key::Y if ctrl => {
                if self.can_redo() {
                    self.redo();
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Key::F if ctrl => {
                self.toggle_search();
                EventResult::Consumed
            }
            Key::C if !ctrl => {
                self.cycle_color();
                EventResult::Consumed
            }
            Key::S => {
                self.cycle_shape();
                EventResult::Consumed
            }
            Key::Space => {
                self.toggle_collapse_selected();
                EventResult::Consumed
            }
            Key::L if !ctrl => {
                self.lay_out_again();
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Run a selection move and report whether the selection actually moved.
    ///
    /// The `select_*` methods return nothing and do nothing at the edges of the
    /// tree, so this is what keeps "Up at the root" from redrawing.
    fn reselect(&mut self, mv: fn(&mut Self)) -> EventResult {
        let before = self.selected_node;
        mv(self);
        if self.selected_node == before {
            EventResult::Ignored
        } else {
            EventResult::Consumed
        }
    }

    /// Keys while a node's text is being edited.
    fn handle_key_editing(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Enter => {
                self.finish_editing();
                EventResult::Consumed
            }
            Key::Escape => {
                self.cancel_editing();
                EventResult::Consumed
            }
            Key::Backspace => {
                if self.edit_buffer.pop().is_none() {
                    return EventResult::Ignored;
                }
                EventResult::Consumed
            }
            _ => {
                if key.text.is_empty() || key.modifiers.ctrl {
                    return EventResult::Ignored;
                }
                self.edit_buffer.push_str(&key.text);
                EventResult::Consumed
            }
        }
    }

    /// Keys while the search box is open.
    fn handle_key_search(&mut self, key: &KeyEvent) -> EventResult {
        match key.key {
            Key::Escape => {
                self.toggle_search();
                EventResult::Consumed
            }
            Key::Enter => {
                if key.modifiers.shift {
                    self.prev_search_result();
                } else {
                    self.next_search_result();
                }
                EventResult::Consumed
            }
            Key::Backspace => {
                let mut q = self.search_query.clone();
                if q.pop().is_none() {
                    return EventResult::Ignored;
                }
                self.set_search_query(q);
                EventResult::Consumed
            }
            _ => {
                if key.text.is_empty() || key.modifiers.ctrl {
                    return EventResult::Ignored;
                }
                let mut q = self.search_query.clone();
                q.push_str(&key.text);
                self.set_search_query(q);
                EventResult::Consumed
            }
        }
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    ///
    /// Renders the entire UI to a list of render commands.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Full window background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.win_width,
            height: self.win_height,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut cmds);
        self.render_tabs(&mut cmds);
        self.render_canvas_background(&mut cmds);
        self.render_connections(&mut cmds);
        self.render_nodes(&mut cmds);
        if self.show_sidebar {
            self.render_sidebar(&mut cmds);
        }
        if self.show_search {
            self.render_search_bar(&mut cmds);
        }
        self.render_status_bar(&mut cmds);

        // Last, so it is above everything. Forgetting this is how a picker
        // ends up open and invisible, taking every keystroke with nothing on
        // screen to say why -- apps/flashcards shipped exactly that this
        // afternoon and its own test caught it.
        cmds.extend(
            self.picker
                .render(&self.palette, self.win_width, self.win_height),
        );

        // And the shortcut list over even that, because it is the one thing a
        // reader asked for explicitly.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut cmds,
                &self.palette,
                (self.win_width, self.win_height),
                TOOLBAR_HEIGHT,
                SHORTCUTS,
                "F1 or ? closes this",
            );
        }

        cmds
    }

    // ------ Toolbar ------

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.win_width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Mind Map".to_string(),
            font_size: 15.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Toolbar buttons, where a click finds them.
        let rects = toolbar_button_rects();
        for ((label, _), &(bx, by, bw, bh)) in TOOLBAR_BUTTONS.iter().zip(&rects) {
            self.palette
                .push_surface(cmds, bx, by, bw, bh, PANEL_CORNER, Surface::Card);
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 14.0,
                text: (*label).to_string(),
                font_size: TOOLBAR_LABEL_SIZE,
                color: self.palette.text,
                font_weight: FontWeightHint::Regular,
                max_width: Some((bw - 12.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Zoom indicator, after the last button.
        let after = rects.last().map_or(TOOLBAR_FIRST_X, |&(x, _, w, _)| x + w);
        let zoom_pct = format!("{}%", (self.zoom * 100.0) as u32);
        cmds.push(RenderCommand::Text {
            x: after + 14.0,
            y: 14.0,
            text: zoom_pct,
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // ------ Map tabs ------

    fn render_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT;
        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.win_width,
            TAB_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        for (i, map) in self.maps.iter().enumerate() {
            let is_active = i == self.active_map;
            let tab_color = if is_active {
                self.palette.base
            } else {
                self.palette.surface0
            };
            let text_color = if is_active {
                self.palette.text
            } else {
                self.palette.overlay0
            };
            let (tx, ty, tw, th) = self.tab_rect(i);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: ty,
                width: tw,
                height: th,
                color: tab_color,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });
            // `*` for a map with changes not saved, as the title marks it.
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 9.0,
                text: if map.dirty {
                    format!("*{}", map.name)
                } else {
                    map.name.clone()
                },
                font_size: 11.0,
                color: text_color,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some((tw - 12.0 - TAB_CLOSE_W).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
            let (cx, cy, cw, _) = self.tab_close_rect(i);
            cmds.push(RenderCommand::Text {
                x: cx + 5.0,
                y: cy + 2.0,
                text: String::from("\u{d7}"),
                font_size: 12.0,
                color: text_color,
                font_weight: FontWeightHint::Regular,
                max_width: Some(cw),
                overflow: TextOverflow::Clip,
            });
        }

        // "+" button to add a new map.
        let (px, py, pw, ph) = self.plus_rect();
        self.palette
            .push_surface(cmds, px, py, pw, ph, PANEL_CORNER, Surface::Card);
        cmds.push(RenderCommand::Text {
            x: px + 7.0,
            y: py + 4.0,
            text: "+".to_string(),
            font_size: 13.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // ------ Canvas background ------

    fn render_canvas_background(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: self.canvas_x(),
            y: self.canvas_y(),
            width: self.canvas_width(),
            height: self.canvas_height(),
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });

        // Draw subtle grid dots
        self.render_grid_dots(cmds);
    }

    fn render_grid_dots(&self, cmds: &mut Vec<RenderCommand>) {
        let grid_step = 40.0 * self.zoom;
        if grid_step < 8.0 {
            return; // too zoomed out, skip grid
        }

        let cx0 = self.canvas_x();
        let cy0 = self.canvas_y();
        let cw = self.canvas_width();
        let ch = self.canvas_height();

        let offset_x = self.pan_x % grid_step;
        let offset_y = self.pan_y % grid_step;

        let dot_color = Color::rgba(
            self.palette.overlay0.r,
            self.palette.overlay0.g,
            self.palette.overlay0.b,
            40,
        );
        let dot_size = 2.0f32;

        let mut gx = offset_x;
        while gx < cw {
            let mut gy = offset_y;
            while gy < ch {
                cmds.push(RenderCommand::FillRect {
                    x: cx0 + gx,
                    y: cy0 + gy,
                    width: dot_size,
                    height: dot_size,
                    color: dot_color,
                    corner_radii: CornerRadii::ZERO,
                });
                gy += grid_step;
            }
            gx += grid_step;
        }
    }

    // ------ Connection lines between nodes ------

    fn render_connections(&self, cmds: &mut Vec<RenderCommand>) {
        let map = self.active_map_ref();
        let visible = map.visible_node_ids();
        let visible_set: std::collections::HashSet<NodeId> = visible.iter().copied().collect();

        for &node_id in &visible {
            let node = match map.node(node_id) {
                Some(n) => n,
                None => continue,
            };
            let parent_id = match node.parent {
                Some(pid) => pid,
                None => continue,
            };
            // Only draw connection if parent is also visible
            if !visible_set.contains(&parent_id) {
                continue;
            }
            let parent = match map.node(parent_id) {
                Some(p) => p,
                None => continue,
            };

            // Determine if child is to the right or left of parent
            let child_on_right = node.x > parent.x;

            let (px, py) = if child_on_right {
                parent.right_center()
            } else {
                parent.left_center()
            };
            let (cx, cy) = if child_on_right {
                node.left_center()
            } else {
                node.right_center()
            };

            let (spx, spy) = self.canvas_to_screen(px, py);
            let (scx, scy) = self.canvas_to_screen(cx, cy);

            // Draw a bezier-like curve approximated by a straight line
            // For visual appeal, we use three line segments to approximate a curve
            let mid_x = f32::midpoint(spx, scx);

            let is_search_match = self.search_results.contains(&node_id);
            let line_color = if is_search_match {
                self.palette.yellow
            } else {
                node.color
            };
            let alpha_color = Color::rgba(line_color.r, line_color.g, line_color.b, 140);

            // Segment 1: parent to control point 1
            cmds.push(RenderCommand::Line {
                x1: spx,
                y1: spy,
                x2: mid_x,
                y2: spy,
                color: alpha_color,
                width: LINE_WIDTH * self.zoom,
            });
            // Segment 2: control point 1 to control point 2
            cmds.push(RenderCommand::Line {
                x1: mid_x,
                y1: spy,
                x2: mid_x,
                y2: scy,
                color: alpha_color,
                width: LINE_WIDTH * self.zoom,
            });
            // Segment 3: control point 2 to child
            cmds.push(RenderCommand::Line {
                x1: mid_x,
                y1: scy,
                x2: scx,
                y2: scy,
                color: alpha_color,
                width: LINE_WIDTH * self.zoom,
            });
        }
    }

    // ------ Nodes ------

    fn render_nodes(&self, cmds: &mut Vec<RenderCommand>) {
        let map = self.active_map_ref();
        let visible = map.visible_node_ids();

        for &node_id in &visible {
            let node = match map.node(node_id) {
                Some(n) => n,
                None => continue,
            };
            self.render_single_node(cmds, node, node_id);
        }
    }

    fn render_single_node(
        &self,
        cmds: &mut Vec<RenderCommand>,
        node: &MindMapNode,
        node_id: NodeId,
    ) {
        let (bx, by, bw, bh) = node.bounds();
        let (sx, sy) = self.canvas_to_screen(bx, by);
        let sw = bw * self.zoom;
        let sh = bh * self.zoom;

        let is_selected = self.selected_node == Some(node_id);
        let is_search_match = self.search_results.contains(&node_id);
        let is_editing = self.editing_node == Some(node_id);

        // Selection highlight (drawn behind the node)
        if is_selected {
            cmds.push(RenderCommand::StrokeRect {
                x: sx - 3.0,
                y: sy - 3.0,
                width: sw + 6.0,
                height: sh + 6.0,
                color: if is_search_match {
                    self.palette.yellow
                } else {
                    self.palette.text
                },
                line_width: 2.0,
                corner_radii: self.corner_radii_for_shape(node.shape, 11.0),
            });
        }

        // Node fill
        let fill_color = if is_editing {
            self.palette.surface1
        } else {
            node.color
        };

        let cr = self.corner_radii_for_shape(node.shape, NODE_CORNER_RADIUS);
        cmds.push(RenderCommand::FillRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
            color: fill_color,
            corner_radii: cr,
        });

        // Node border
        let border_color = if is_search_match {
            self.palette.yellow
        } else if is_selected {
            self.palette.text
        } else {
            Color::rgba(node.color.r, node.color.g, node.color.b, 180)
        };
        cmds.push(RenderCommand::StrokeRect {
            x: sx,
            y: sy,
            width: sw,
            height: sh,
            color: border_color,
            line_width: if is_selected { 2.0 } else { 1.0 },
            corner_radii: cr,
        });

        // Node text
        let is_root = node.parent.is_none();
        let font_size = if is_root {
            ROOT_FONT_SIZE
        } else {
            NODE_FONT_SIZE
        };
        let display_text = if is_editing {
            format!("{}|", self.edit_buffer)
        } else {
            node.text.clone()
        };

        // Center the text approximately
        let text_x = sx + 8.0;
        let text_y = sy + sh / 2.0 - font_size / 2.0;

        // Determine text color (dark on light backgrounds, light on dark)
        let text_color = node_text_color(fill_color);

        cmds.push(RenderCommand::Text {
            x: text_x,
            y: text_y,
            text: display_text,
            font_size: font_size * self.zoom,
            color: text_color,
            font_weight: if is_root {
                FontWeightHint::Bold
            } else {
                FontWeightHint::Regular
            },
            max_width: Some(sw - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Collapse indicator for nodes with children
        if !node.children.is_empty() {
            let indicator_x = sx + sw - COLLAPSE_SIZE - 2.0;
            let indicator_y = sy + sh / 2.0 - COLLAPSE_SIZE / 2.0;
            let indicator_text = if node.collapsed { "+" } else { "-" };

            self.palette.push_surface(
                cmds,
                indicator_x,
                indicator_y,
                COLLAPSE_SIZE,
                COLLAPSE_SIZE,
                2.0,
                Surface::Card,
            );
            cmds.push(RenderCommand::Text {
                x: indicator_x + 2.0,
                y: indicator_y + 1.0,
                text: indicator_text.to_string(),
                font_size: 10.0,
                color: self.palette.text,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    fn corner_radii_for_shape(&self, shape: NodeShape, base: f32) -> CornerRadii {
        match shape {
            NodeShape::Rectangle | NodeShape::Diamond => CornerRadii::ZERO,
            NodeShape::RoundedRect => CornerRadii::all(base),
            NodeShape::Ellipse => CornerRadii::all(base * 2.0),
            NodeShape::Pill => CornerRadii::all(base * 3.0),
        }
    }

    // ------ Sidebar ------

    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let x = 0.0;
        let y = self.canvas_y();
        let h = self.canvas_height();

        // Background
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: SIDEBAR_WIDTH,
            height: h,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: y,
            x2: SIDEBAR_WIDTH,
            y2: y + h,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Section: Node Properties
        let mut sy = y + 10.0;
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: sy,
            text: "Node Properties".to_string(),
            font_size: 13.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        sy += 22.0;

        if let Some(sel_id) = self.selected_node {
            if let Some(node) = self.active_map_ref().node(sel_id) {
                // Text
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    // Nothing follows the node's text on this row, so the
                    // renderer's own elision is the whole answer. It used to be
                    // cut to 18 characters first — a budget with no relation to
                    // the `SIDEBAR_WIDTH - 20` two lines below it.
                    text: format!("Text: {}", node.text),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - 20.0),
                    overflow: TextOverflow::Ellipsis,
                });
                sy += 18.0;

                // Shape
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("Shape: {}", node.shape.label()),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                sy += 18.0;

                // Color swatch
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: "Color:".to_string(),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                cmds.push(RenderCommand::FillRect {
                    x: x + 55.0,
                    y: sy - 1.0,
                    width: 16.0,
                    height: 14.0,
                    color: node.color,
                    corner_radii: CornerRadii::all(2.0),
                });
                sy += 18.0;

                // Children count
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("Children: {}", node.children.len()),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                sy += 18.0;

                // Depth
                let depth = self.active_map_ref().depth(sel_id);
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("Depth: {depth}"),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                sy += 18.0;

                // Collapsed state
                let state_text = if node.collapsed {
                    "Collapsed"
                } else {
                    "Expanded"
                };
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("State: {state_text}"),
                    font_size: 11.0,
                    color: self.palette.subtext0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                sy += 26.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: sy,
                text: "No node selected".to_string(),
                font_size: 11.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            sy += 26.0;
        }

        // Section: Keyboard Shortcuts
        cmds.push(RenderCommand::Line {
            x1: x + 10.0,
            y1: sy,
            x2: x + SIDEBAR_WIDTH - 10.0,
            y2: sy,
            color: self.palette.surface0,
            width: 1.0,
        });
        sy += 10.0;

        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: sy,
            text: "Shortcuts".to_string(),
            font_size: 13.0,
            color: self.palette.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        sy += 20.0;

        let shortcuts = [
            ("Tab", "Add child"),
            ("Enter", "Add sibling"),
            ("F2 / E", "Edit node"),
            ("Del", "Delete subtree"),
            ("C", "Cycle color"),
            ("S", "Cycle shape"),
            ("Space", "Collapse/Expand"),
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y", "Redo"),
            ("Ctrl+F", "Search"),
            ("+/-", "Zoom in/out"),
            ("Arrows", "Navigate"),
        ];

        for (key, action) in &shortcuts {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: sy,
                text: key.to_string(),
                font_size: 10.0,
                color: self.palette.ink(self.palette.blue),
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cmds.push(RenderCommand::Text {
                x: x + 70.0,
                y: sy,
                text: action.to_string(),
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 80.0),
                overflow: TextOverflow::Ellipsis,
            });
            sy += 16.0;
        }
    }

    // ------ Search bar ------

    fn render_search_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let bar_width = 300.0f32;
        let bar_height = 36.0f32;
        let bx = self.canvas_x() + (self.canvas_width() - bar_width) / 2.0;
        let by = self.canvas_y() + 8.0;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: bx,
            y: by,
            width: bar_width,
            height: bar_height,
            color: self.palette.surface0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: bx,
            y: by,
            width: bar_width,
            height: bar_height,
            color: self.palette.blue,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Search icon text
        cmds.push(RenderCommand::Text {
            x: bx + 10.0,
            y: by + 10.0,
            text: "Search:".to_string(),
            font_size: 12.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Query text
        let display = if self.search_query.is_empty() {
            "type to search...".to_string()
        } else {
            self.search_query.clone()
        };
        let query_color = if self.search_query.is_empty() {
            self.palette.overlay0
        } else {
            self.palette.text
        };

        cmds.push(RenderCommand::Text {
            x: bx + 70.0,
            y: by + 10.0,
            text: display,
            font_size: 12.0,
            color: query_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_width - 110.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Result count
        if !self.search_query.is_empty() {
            let count_text = if self.search_results.is_empty() {
                "0 results".to_string()
            } else {
                format!(
                    "{}/{}",
                    self.search_index.saturating_add(1),
                    self.search_results.len()
                )
            };
            cmds.push(RenderCommand::Text {
                x: bx + bar_width - 60.0,
                y: by + 10.0,
                text: count_text,
                font_size: 10.0,
                color: self.palette.subtext0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }
    }

    // ------ Status bar ------

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.win_height - STATUS_BAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.win_width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        let map = self.active_map_ref();
        // What the last open or save did comes first: a failed save is the
        // one thing on this line that must not be the part cut off. It was
        // kept and never drawn -- every open and save said nothing.
        let counts = format!(
            "Nodes: {} | Zoom: {}%",
            map.node_count(),
            (self.zoom * 100.0) as u32,
        );
        let status = match &self.last_file_action {
            Some(said) => format!("{said} | {counts}"),
            None => counts,
        };
        // Stopped short of the selected node's line on the right, which it
        // ran under.
        let selected = self.selected_node.is_some_and(|id| map.node(id).is_some());
        let room = if selected {
            self.win_width - 20.0 - SEL_INFO_WIDTH - 20.0
        } else {
            self.win_width - 20.0
        };
        cmds.push(RenderCommand::Text {
            x: 10.0,
            y: y + 5.0,
            text: status,
            font_size: 11.0,
            color: self.palette.subtext0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(room.max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // Selected node info on the right
        if let Some(sel_id) = self.selected_node
            && let Some(node) = map.node(sel_id)
        {
            // The node's text is elided here rather than by the renderer,
            // because it is *not* at the end of the line: the id follows it,
            // and the id is what a reader needs when the name is too long to
            // show whole. Eliding the line as a whole would drop the id and
            // keep the name, which is backwards. So the room is worked out by
            // measuring the parts that must survive and giving the name what is
            // left — measured, not counted: this was a flat 20 characters,
            // which is a different width in every name, compared against
            // `s.len()`, a count of *bytes*.
            let prefix = "Selected: \"";
            let suffix = format!("\" (ID: {sel_id})");
            let room = (SEL_INFO_WIDTH
                - text::measure(prefix, SEL_INFO_SIZE, FontWeightHint::Regular)
                - text::measure(&suffix, SEL_INFO_SIZE, FontWeightHint::Regular))
            .max(0.0);
            let name = text::elide(
                &node.text,
                room,
                "\u{2026}",
                SEL_INFO_SIZE,
                FontWeightHint::Regular,
            );
            let sel_info = format!("{prefix}{name}{suffix}");
            cmds.push(RenderCommand::Text {
                x: self.win_width - 300.0,
                y: y + 5.0,
                text: sel_info,
                font_size: SEL_INFO_SIZE,
                color: self.palette.ink(self.palette.blue),
                font_weight: FontWeightHint::Regular,
                max_width: Some(SEL_INFO_WIDTH),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Helper: determine text color based on background brightness
// ============================================================================

/// Choose a readable text color for a node based on its background color.
fn node_text_color(bg: Color) -> Color {
    // Simple luminance approximation
    let luminance = (bg.r as f32 * 0.299) + (bg.g as f32 * 0.587) + (bg.b as f32 * 0.114);
    if luminance > 140.0 {
        Color::from_hex(0x1E1E2E) // dark text on light background
    } else {
        Color::from_hex(0xCDD6F4) // light text on dark background
    }
}

// A `truncate_str(s, max_chars)` used to live here. It is gone: one of its two
// callers did not need it at all (the renderer elides the end of a line), and
// the other needed a *width*, which a character count cannot express. It also
// compared `s.len()` — bytes — against a character budget, so it cut accented
// text early and CJK text several characters early.

// ============================================================================
// Entry point
// ============================================================================

impl App for MindMapApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    /// The map showing, marked `*` while it has changes not saved.
    fn title(&self) -> String {
        let map = self.active_map_ref();
        format!(
            "{}{} — Mind Map",
            if map.dirty { "*" } else { "" },
            map.name
        )
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.win_width as u32, self.win_height as u32)
        }
    }

    /// No clock.
    ///
    /// Nothing here advances on its own: a node moves when it is dragged, the
    /// view pans when it is panned, and the layout is recomputed on demand.
    /// There is no animation and no data that ages, so a tick would wake the
    /// machine to redraw an identical frame. This is `known-issues.md` lesson
    /// 47's question asked and answered the other way.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    /// Closing over maps with changes not saved asks first, and the window
    /// waits for the answer: `KeepOpen` declines the close and draws the
    /// question.
    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return if self.request_quit() {
                Response::Exit
            } else {
                Response::KeepOpen
            };
        }
        let result = self.handle_event(event);
        if self.quit {
            // The question was answered, or the last save it asked for was
            // made: nothing is left unsaved.
            return Response::Exit;
        }
        match result {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.win_width = width;
        self.win_height = height;
        let mut tree = RenderTree {
            commands: self.render_commands(),
        };
        // Over everything, the picker included: they are never up together.
        let palette = self.palette;
        if let Some(question) = self.question.as_mut() {
            question.render(&palette, width, height, &mut tree);
        }
        tree
    }
}

fn main() -> ExitCode {
    let mut map = MindMapApp::new();
    app::launch("mindmap", &mut map)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
// A test that reaches into a structure it just built and finds it missing has
// found a bug, and panicking is how it reports one. The defensive lints are
// about untrusted input reaching production code; there is none here, and
// leaving them on buried real warnings under ninety-odd of these.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    // A test comparing a coordinate it computed against one it wrote down is
    // asking whether the arithmetic produced exactly that value, which is the
    // question, not an accident.
    clippy::float_cmp,
    // `sort_unstable` is faster and a test that sorts a handful of ids to
    // compare them does not care.
    clippy::stable_sort_primitive
)]
mod tests {

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no event handling at all until it was wired to the
    // compositor: every one of its mutators was reachable only by a caller
    // invoking it directly.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;
    use guitk::shortcut::keystrokes;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// A list on screen and the handler behind it are two copies of one fact,
    /// and they drift: `apps/rssreader` shipped an overlay of twenty-one
    /// shortcuts of which about four worked. The label is read by
    /// `guitk::shortcut` rather than matched against a table written beside it
    /// here, because that table would be a third copy drifting from both.
    ///
    /// The property is "some reachable state answers this key", not "this key
    /// is taken right now". `Up` at the root, `Delete` with nothing selected
    /// and `Ctrl+Y` with nothing undone all decline on purpose, and declining
    /// from its own arm is answering.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let event = Event::Key(stroke.clone());
                let answered = states()
                    .iter_mut()
                    .any(|app| app.handle_event(&event) == EventResult::Consumed);
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and no map answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Maps chosen so that between them every advertised key has work to do.
    fn states() -> Vec<MindMapApp> {
        // Untouched.
        let fresh = MindMapApp::new();

        // A node with a child under it, which is the one state `Down` has
        // anywhere to descend to. Two `Tab`s and an `Up`, because adding a
        // child selects the child -- a new map is a root on its own, and the
        // selection is never above a node that has one.
        let mut budded = MindMapApp::new();
        for k in [Key::Tab, Key::Tab, Key::Up] {
            budded.handle_event(&press(k));
        }

        // Two siblings, standing on the *second*: a sibling behind it for
        // `Left`, a parent above it for `Up`, and a node under the cursor that
        // may be deleted or renamed.
        let mut second = MindMapApp::new();
        for k in [Key::Tab, Key::Enter] {
            second.handle_event(&press(k));
        }

        // ...and standing on the first, because `Right` needs one ahead of it
        // and `Left` needs one behind, and no single selection has both.
        let mut first = MindMapApp::new();
        for k in [Key::Tab, Key::Enter, Key::Left] {
            first.handle_event(&press(k));
        }

        // Something done and then undone, so redo has work.
        let mut undone = MindMapApp::new();
        undone.handle_event(&press(Key::Tab));
        undone.handle_event(&press_ctrl(Key::Z));

        // Two maps, so there is another to go to.
        let mut two = MindMapApp::new();
        two.add_map();

        vec![fresh, budded, second, first, undone, two]
    }

    /// **The shortcut list reaches the window.**
    ///
    /// `every_advertised_key_does_something` reads the list and the handler;
    /// this reads the list and the *screen*. They are different questions, and
    /// `apps/netscan`'s `wol_note` is why both get asked: it was written by the
    /// model and drawn by nothing for three commits, and every model-level test
    /// passed throughout. A help overlay that never draws is the same defect
    /// with the same green suite.
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = MindMapApp::new();
        let quiet = drawn_text(&app);
        assert!(
            !quiet.contains("? closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press_shift(Key::Slash));
        let shown = drawn_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(keys), "{keys:?} never reached the window");
            assert!(shown.contains(what), "{what:?} never reached the window");
        }

        app.handle_event(&press(Key::Escape));
        assert!(
            !drawn_text(&app).contains("? closes this"),
            "Escape did not close it"
        );
    }

    /// Every string the window is drawing, joined.
    fn drawn_text(app: &MindMapApp) -> String {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// A key with Shift held, which is how `?` is typed.
    fn press_shift(k: Key) -> Event {
        let mut modifiers = Modifiers::NONE;
        modifiers.shift = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent {
            key: Key::Unknown(0),
            pressed: true,
            modifiers: Modifiers::NONE,
            text: c.to_string(),
        })
    }

    fn mouse(kind: MouseEventKind, x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    /// A key with Ctrl held, for the picker tests.
    fn ctrl_press(k: Key) -> Event {
        let mut modifiers = Modifiers::NONE;
        modifiers.ctrl = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// An open picker takes the keyboard, and the window behind it does not.
    ///
    /// The open test asserts that the KEY HANDLER opened the dialog, which
    /// holds whether or not the picker is ever handed another event; the
    /// writer test calls the writer with a path directly and never touches the
    /// dialog. This is the half routing actually decides -- with a dialog up,
    /// a keystroke belongs to the dialog.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_map() {
        let mut app = MindMapApp::new();
        // Down selects the first CHILD, and needs both a selection to start
        // from and a child to move to. A new map has neither: `selected_node`
        // is `None`, so `select_first_child` returns immediately. Two earlier
        // versions of this test asserted `None == None` and passed with the
        // dialog doing nothing -- the scanner caught both, by cutting the
        // routing and staying green.
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let child = app.add_child_to_selected(String::from("a child"));
        // **Back to the root.** `add_child_to_selected` selects the node it
        // creates, so without this the selection is already the leaf and Down
        // has nowhere further to go -- which is what the two previous versions
        // of this fixture actually tested, and why the scanner kept saying the
        // routing was unpinned while the test passed.
        app.selected_node = Some(root);
        assert!(
            child.is_some(),
            "control: the fixture needs somewhere to move to"
        );
        let before = app.selected_node;
        assert!(
            before.is_some(),
            "control: something must be selected to move from"
        );

        app.handle_event(&ctrl_press(Key::O));
        assert!(app.picker.is_open(), "control: the picker must be up");

        app.handle_event(&press(Key::Down));
        assert_eq!(
            app.selected_node, before,
            "Down at the open dialog moved the selection behind it"
        );
    }

    #[test]
    fn a_click_selects_the_node_under_it_at_a_zoom_that_is_not_one() {
        // The hit test and the drag routines work in canvas space and the mouse
        // arrives in screen space. At zoom 1.0 with no pan the two are the
        // same, so a test that only checks the default view proves nothing
        // about the conversion -- the transform has to be exercised away from
        // its identity.
        let mut app = MindMapApp::new();
        app.set_zoom(2.0);
        // Panned so the root, twice as far from the corner at this zoom, is
        // still inside the window: a press outside the canvas is not one on
        // the map.
        app.pan_x = -603.0;
        app.pan_y = -419.0;
        let root = app.active_map_ref().root_id;
        let (nx, ny) = {
            let n = app.active_map_ref().node(root).expect("the root exists");
            (n.x, n.y)
        };
        let (sx, sy) = app.canvas_to_screen(nx, ny);
        assert_eq!(
            app.handle_event(&mouse(MouseEventKind::Press(MouseButton::Left), sx, sy)),
            EventResult::Consumed
        );
        assert_eq!(app.selected_node, Some(root), "the click missed the root");
    }

    #[test]
    fn dragging_a_node_moves_it_by_the_distance_dragged_in_canvas_units() {
        // At zoom 2.0 a 100-pixel drag is a 50-unit move. Getting this wrong is
        // invisible at the default zoom and obvious at any other.
        let mut app = MindMapApp::new();
        app.set_zoom(2.0);
        // Panned to keep the root inside the window at this zoom.
        app.pan_x = -640.0;
        app.pan_y = -400.0;
        let root = app.active_map_ref().root_id;
        let (nx, ny) = {
            let n = app.active_map_ref().node(root).expect("the root exists");
            (n.x, n.y)
        };
        let (sx, sy) = app.canvas_to_screen(nx, ny);
        assert!(
            sx < app.win_width && sy < app.canvas_y() + app.canvas_height(),
            "control: the root is off the canvas at ({sx}, {sy})"
        );
        app.handle_event(&mouse(MouseEventKind::Press(MouseButton::Left), sx, sy));
        app.handle_event(&mouse(MouseEventKind::Move, sx + 100.0, sy));
        let moved = app.active_map_ref().node(root).expect("still there").x;
        assert!(
            (moved - (nx + 50.0)).abs() < 0.01,
            "expected a 50-unit move at zoom 2, got {}",
            moved - nx
        );
        app.handle_event(&mouse(
            MouseEventKind::Release(MouseButton::Left),
            sx + 100.0,
            sy,
        ));
        assert!(app.can_undo(), "a completed drag should be undoable");
    }

    #[test]
    fn a_pointer_move_with_no_button_down_is_not_a_redraw() {
        // Otherwise the whole map redraws on every pixel the pointer crosses.
        let mut app = MindMapApp::new();
        assert_eq!(
            app.handle_event(&mouse(MouseEventKind::Move, 10.0, 10.0)),
            EventResult::Ignored
        );
    }

    #[test]
    fn clicking_empty_canvas_clears_the_selection_and_pans() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        // The canvas's top left corner, far from the one node, which is laid
        // out in the middle.
        let (x, y) = (app.canvas_x() + 10.0, app.canvas_y() + 10.0);
        assert_eq!(app.hit_test_screen(x, y), None, "control: a node is there");
        app.handle_event(&mouse(MouseEventKind::Press(MouseButton::Left), x, y));
        assert_eq!(app.selected_node, None, "clicking away should deselect");
        let before = app.pan_x;
        app.handle_event(&mouse(MouseEventKind::Move, x + 100.0, y));
        assert!(
            (app.pan_x - before).abs() > 1.0,
            "the canvas should have panned"
        );
    }

    #[test]
    fn the_wheel_zooms_and_a_zero_delta_does_nothing() {
        let mut app = MindMapApp::new();
        let start = app.zoom;
        app.handle_event(&mouse(
            MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
            0.0,
            0.0,
        ));
        assert!(app.zoom > start, "wheel away from the user should zoom in");
        app.handle_event(&mouse(
            MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            0.0,
            0.0,
        ));
        assert!((app.zoom - start).abs() < 0.001, "and back out again");
        assert_eq!(
            app.handle_event(&mouse(
                MouseEventKind::Scroll { dx: 0.0, dy: 0.0 },
                0.0,
                0.0
            )),
            EventResult::Ignored
        );
    }

    #[test]
    fn tab_adds_a_child_and_enter_adds_a_sibling() {
        // Counting nodes is not enough: a child and a sibling both add one, so
        // a test that only counts cannot tell the two keys apart at all. What
        // distinguishes them is the parent of what they made.
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);

        assert_eq!(app.handle_event(&press(Key::Tab)), EventResult::Consumed);
        let child = app.selected_node.expect("the new node is selected");
        assert_ne!(child, root);
        assert_eq!(
            app.active_map_ref().node(child).expect("it exists").parent,
            Some(root),
            "Tab should have made a child of the selection"
        );

        assert_eq!(app.handle_event(&press(Key::Enter)), EventResult::Consumed);
        let sibling = app.selected_node.expect("the new node is selected");
        assert_ne!(sibling, child);
        assert_eq!(
            app.active_map_ref()
                .node(sibling)
                .expect("it exists")
                .parent,
            Some(root),
            "Enter should have made a sibling of the selection, not a child of it"
        );
        assert_eq!(
            app.active_map_ref()
                .node(child)
                .expect("it exists")
                .children
                .len(),
            0,
            "the sibling was attached under the previous node"
        );
    }

    #[test]
    fn the_arrows_walk_the_tree_and_stop_at_its_edges() {
        // "Up" is the parent, not the node above on screen: a radial layout has
        // no meaningful screen-up.
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        assert_eq!(
            app.handle_event(&press(Key::Up)),
            EventResult::Ignored,
            "the root has no parent"
        );
        app.handle_event(&press(Key::Tab));
        let child = app.selected_node.expect("the new child is selected");
        assert_ne!(child, root);
        assert_eq!(app.handle_event(&press(Key::Up)), EventResult::Consumed);
        assert_eq!(app.selected_node, Some(root));
        assert_eq!(app.handle_event(&press(Key::Down)), EventResult::Consumed);
        assert_eq!(app.selected_node, Some(child));
    }

    #[test]
    fn typing_into_a_node_does_not_run_the_shortcuts_those_letters_name() {
        // `c` cycles the colour and `s` the shape outside the editor. Inside
        // it they are text, and a node named "cs" must not have been recoloured
        // and reshaped on the way in.
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let (colour, shape) = {
            let n = app.active_map_ref().node(root).expect("the root exists");
            (n.color, n.shape)
        };
        app.handle_event(&press(Key::F2));
        assert!(app.editing_node.is_some(), "F2 should start editing");
        // Editing starts from the existing text, so the typed characters are
        // appended to it rather than replacing it.
        let seeded = app.edit_buffer.clone();
        app.handle_event(&typed('c'));
        app.handle_event(&typed('s'));
        assert_eq!(app.edit_buffer, format!("{seeded}cs"));
        let n = app.active_map_ref().node(root).expect("the root exists");
        assert_eq!(n.color, colour, "the colour changed while typing");
        assert_eq!(n.shape, shape, "the shape changed while typing");
        app.handle_event(&press(Key::Enter));
        assert!(app.editing_node.is_none(), "Enter should finish editing");
    }

    #[test]
    fn escape_abandons_an_edit_and_leaves_the_text_alone() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let before = app
            .active_map_ref()
            .node(root)
            .expect("the root exists")
            .text
            .clone();
        app.handle_event(&press(Key::F2));
        app.handle_event(&typed('x'));
        app.handle_event(&press(Key::Escape));
        assert!(app.editing_node.is_none());
        assert_eq!(
            app.active_map_ref().node(root).expect("still there").text,
            before
        );
    }

    #[test]
    fn ctrl_z_reaches_the_undo_stack_the_app_was_already_keeping() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let n = app.active_map_ref().nodes.len();
        app.handle_event(&press(Key::Tab));
        assert_eq!(app.active_map_ref().nodes.len(), n + 1);
        assert!(app.can_undo());
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Consumed);
        assert_eq!(app.active_map_ref().nodes.len(), n, "undo did not undo");
        assert_eq!(app.handle_event(&press_ctrl(Key::Y)), EventResult::Consumed);
        assert_eq!(app.active_map_ref().nodes.len(), n + 1, "redo did not redo");
    }

    #[test]
    fn undo_with_nothing_to_undo_is_not_a_redraw() {
        let mut app = MindMapApp::new();
        assert!(!app.can_undo());
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Ignored);
    }

    #[test]
    fn typing_in_the_search_box_searches_rather_than_editing_a_node() {
        let mut app = MindMapApp::new();
        assert_eq!(app.handle_event(&press_ctrl(Key::F)), EventResult::Consumed);
        assert!(app.show_search);
        app.handle_event(&typed('a'));
        assert_eq!(app.search_query, "a");
        assert!(
            app.editing_node.is_none(),
            "search must not open the editor"
        );
        app.handle_event(&press(Key::Backspace));
        assert_eq!(app.search_query, "");
        app.handle_event(&press(Key::Escape));
        assert!(!app.show_search);
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        let mut app = MindMapApp::new();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = MindMapApp::new();
        let n = app.active_map_ref().nodes.len();
        let release = Event::Key(KeyEvent {
            key: Key::Tab,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.active_map_ref().nodes.len(), n);
    }

    #[test]
    fn a_resize_is_taken_but_is_not_itself_a_redraw() {
        let mut app = MindMapApp::new();
        assert_eq!(
            app.handle_event(&Event::Resize {
                width: 1280,
                height: 1024
            }),
            EventResult::Ignored
        );
        assert!((app.win_width - 1280.0).abs() < f32::EPSILON);
        assert!((app.win_height - 1024.0).abs() < f32::EPSILON);
    }
    use super::*;
    /// The picker is not merely open: it is DRAWN.
    ///
    /// `is_open()` returning true is not the same claim, and assuming it was
    /// is how `apps/flashcards` shipped a dialog that took every keystroke and
    /// painted nothing. Deleting the `picker.render` line in the renderer
    /// leaves `is_open()` true and every other test green; this is the one
    /// that notices.
    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        let mut app = MindMapApp::new();
        let before = app.render_commands().len();
        app.picker.open_to_read();
        assert!(app.picker.is_open(), "no picker came up");
        let after = app.render_commands().len();
        let own = app
            .picker
            .render(&app.palette, app.win_width, app.win_height)
            .len();
        assert!(
            own > 0,
            "the picker itself draws nothing, so this proves nothing"
        );
        assert!(
            after >= before + own,
            "the frame does not contain the picker's own {own} command(s) ({before} before, {after} after) -- something else grew instead"
        );
    }

    fn mm_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("slateos-mindmap-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// The words and the branches survive a write and a read.
    ///
    /// `export_text` was written, tested and unreachable, and there was no
    /// reader at all. An outline is not a full inverse -- colour, shape and
    /// collapse state are not in it -- but it carries the two things a mind
    /// map is actually made of, and `auto_layout` puts the nodes back where
    /// they belong, which is why losing their positions costs nothing.
    #[test]
    fn the_words_and_the_branches_survive_a_write_and_a_read() {
        let path = mm_dir().join("tree.outline");
        let _ = std::fs::remove_file(&path);

        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.active_map_mut()
            .nodes
            .get_mut(&root)
            .expect("root")
            .text = String::from("Topic");
        let child = app
            .active_map_mut()
            .add_child(root, String::from("Branch"), NODE_COLORS[1], 1)
            .expect("a child");
        app.active_map_mut()
            .add_child(child, String::from("Twig"), NODE_COLORS[2], 2)
            .expect("a grandchild");

        let said = app.write_outline(&path);
        assert!(said.starts_with("Wrote 3 node(s)"), "said: {said}");

        let before = app.maps.len();
        let said = app.open_file(&path);
        assert!(said.starts_with("Opened 3 node(s)"), "said: {said}");
        assert_eq!(app.maps.len(), before + 1, "no map was opened");

        let opened = app.active_map_ref();
        let texts: Vec<&str> = opened.nodes.values().map(|n| n.text.as_str()).collect();
        for want in ["Topic", "Branch", "Twig"] {
            assert!(texts.contains(&want), "lost {want:?}: {texts:?}");
        }
        // The shape, not just the words: Twig hangs off Branch, not off Topic.
        let branch = opened
            .nodes
            .values()
            .find(|n| n.text == "Branch")
            .expect("Branch");
        let twig = opened
            .nodes
            .values()
            .find(|n| n.text == "Twig")
            .expect("Twig");
        assert_eq!(twig.parent, Some(branch.id), "the tree was flattened");

        let _ = std::fs::remove_file(&path);
    }

    /// The import says what an outline does not carry.
    ///
    /// Without it the map comes back in default colours with every branch
    /// unfolded, and that looks like the map -- the user has no way to tell a
    /// format that dropped their choices from a map that never had any.
    #[test]
    fn the_import_says_what_an_outline_leaves_behind() {
        let path = mm_dir().join("plain.outline");
        std::fs::write(&path, "Topic\n  - Branch\n").expect("write outline");

        let mut app = MindMapApp::new();
        let said = app.open_file(&path);
        assert!(
            said.contains("Colours, shapes and folded branches are not in an outline"),
            "said: {said}"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// A node holding a line break is written flat, and the write says so.
    ///
    /// `outline_text` has always replaced control characters with a space --
    /// one node, one line -- and never reported it. A silent flattening looks
    /// exactly like a node that was typed on one line.
    #[test]
    fn a_node_with_a_line_break_is_flattened_and_the_write_says_so() {
        let path = mm_dir().join("flat.outline");
        let _ = std::fs::remove_file(&path);

        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.active_map_mut()
            .nodes
            .get_mut(&root)
            .expect("root")
            .text = String::from("two\nlines");

        let said = app.write_outline(&path);
        assert!(
            said.contains("line breaks, which an outline stores as spaces"),
            "the flattening went unreported: {said}"
        );
        let body = std::fs::read_to_string(&path).expect("written");
        assert!(body.contains("two lines"), "not flattened: {body:?}");

        let _ = std::fs::remove_file(&path);
    }

    /// An outline that skips a level is clamped, not rejected.
    ///
    /// Hand-written outlines indent by four spaces, or by eight. Refusing the
    /// file would lose the whole map over a cosmetic slip, so a line deeper
    /// than one level past its predecessor hangs off that predecessor.
    #[test]
    fn an_outline_that_skips_a_level_is_clamped_rather_than_refused() {
        // `gen` is a reserved keyword in edition 2024.
        let mut ids = IdGenerator::new();
        let map = MindMap::from_outline("M", "Root\n      - Deep\n", &mut ids)
            .expect("an outline with a line in it");
        assert_eq!(map.nodes.len(), 2, "the skipped level lost a node");
        let deep = map.nodes.values().find(|n| n.text == "Deep").expect("Deep");
        assert_eq!(
            deep.parent,
            Some(map.root_id),
            "it did not hang off the root"
        );
    }

    /// A file with nothing in it is reported, not opened as an empty map.
    #[test]
    fn an_outline_of_blank_lines_opens_nothing() {
        let path = mm_dir().join("blank.outline");
        std::fs::write(&path, "\n   \n\n").expect("write blanks");

        let mut app = MindMapApp::new();
        let before = app.maps.len();
        let said = app.open_file(&path);
        assert!(said.contains("every line was blank"), "said: {said}");
        assert_eq!(app.maps.len(), before, "an empty map was opened anyway");

        let _ = std::fs::remove_file(&path);
    }

    // ---- Outline export ----

    #[test]
    fn node_text_cannot_add_branches_to_the_outline() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Map".to_string(), &mut id_gen);
        let root_id = map.root_id;
        if let Some(root) = map.nodes.get_mut(&root_id) {
            root.text = "Root\n- forged sibling\n  - forged child".to_string();
        }
        map.add_child(
            root_id,
            "Child\n- another forged".to_string(),
            NODE_COLORS[0],
            0,
        );
        let text = map.export_text();
        // Two real nodes, so two lines -- counting lines rather than searching
        // for the payload, since a correctly folded label still contains it.
        assert_eq!(
            text.lines().count(),
            2,
            "node text drew branches that do not exist: {text:?}"
        );
    }

    #[test]
    fn folding_a_label_keeps_it_readable() {
        assert_eq!(outline_text("one\ntwo"), "one two");
        assert_eq!(outline_text("a\r\n\nb"), "a b");
        assert_eq!(outline_text("\n\nleading"), "leading");
        assert_eq!(outline_text("trailing\n\n"), "trailing");
        assert_eq!(outline_text("plain label"), "plain label");
    }

    // ---- ID Generator ----

    #[test]
    fn test_id_gen_starts_at_one() {
        let mut id_gen = IdGenerator::new();
        assert_eq!(id_gen.next_id(), 1);
    }

    #[test]
    fn test_id_gen_increments() {
        let mut id_gen = IdGenerator::new();
        let a = id_gen.next_id();
        let b = id_gen.next_id();
        assert_eq!(b, a + 1);
    }

    #[test]
    fn test_id_gen_unique() {
        let mut id_gen = IdGenerator::new();
        let mut ids = Vec::new();
        for _ in 0..100 {
            ids.push(id_gen.next_id());
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 100);
    }

    // ---- NodeShape ----

    #[test]
    fn test_node_shape_all_count() {
        assert_eq!(NodeShape::all().len(), 5);
    }

    #[test]
    fn test_node_shape_labels_not_empty() {
        for shape in NodeShape::all() {
            assert!(!shape.label().is_empty());
        }
    }

    #[test]
    fn test_node_shape_next_cycles() {
        let start = NodeShape::Rectangle;
        let mut current = start;
        for _ in 0..5 {
            current = current.next();
        }
        assert_eq!(current, start);
    }

    #[test]
    fn test_node_shape_next_not_identity() {
        assert_ne!(NodeShape::Rectangle.next(), NodeShape::Rectangle);
    }

    // ---- MindMapNode ----

    #[test]
    fn test_node_new_root() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let node = MindMapNode::new(1, "Root".to_string(), None, pal.blue, 0);
        assert_eq!(node.id, 1);
        assert!(node.parent.is_none());
        assert_eq!(node.shape, NodeShape::Ellipse);
        assert_eq!(node.width, ROOT_NODE_W);
        assert_eq!(node.height, ROOT_NODE_H);
        assert!(!node.collapsed);
    }

    #[test]
    fn test_node_new_child() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let node = MindMapNode::new(2, "Child".to_string(), Some(1), pal.green, 1);
        assert_eq!(node.parent, Some(1));
        assert_eq!(node.shape, NodeShape::RoundedRect);
        assert_eq!(node.width, DEFAULT_NODE_W);
    }

    #[test]
    fn test_node_contains() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut node = MindMapNode::new(1, "Test".to_string(), None, pal.blue, 0);
        node.x = 100.0;
        node.y = 100.0;
        node.width = 140.0;
        node.height = 40.0;
        // Center is (100, 100), so bounds are (30, 80, 140, 40)
        assert!(node.contains(100.0, 100.0)); // center
        assert!(node.contains(31.0, 81.0)); // near top-left
        assert!(!node.contains(0.0, 0.0)); // far away
    }

    #[test]
    fn test_node_center() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut node = MindMapNode::new(1, "Test".to_string(), None, pal.blue, 0);
        node.x = 50.0;
        node.y = 75.0;
        let (cx, cy) = node.center();
        assert!((cx - 50.0).abs() < 0.01);
        assert!((cy - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_node_right_center() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut node = MindMapNode::new(1, "Test".to_string(), None, pal.blue, 0);
        node.x = 100.0;
        node.y = 100.0;
        node.width = 140.0;
        let (rx, ry) = node.right_center();
        assert!((rx - 170.0).abs() < 0.01);
        assert!((ry - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_node_left_center() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut node = MindMapNode::new(1, "Test".to_string(), None, pal.blue, 0);
        node.x = 100.0;
        node.y = 100.0;
        node.width = 140.0;
        let (lx, ly) = node.left_center();
        assert!((lx - 30.0).abs() < 0.01);
        assert!((ly - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_node_bounds() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut node = MindMapNode::new(1, "Test".to_string(), None, pal.blue, 0);
        node.x = 200.0;
        node.y = 150.0;
        node.width = 100.0;
        node.height = 40.0;
        let (bx, by, bw, bh) = node.bounds();
        assert!((bx - 150.0).abs() < 0.01);
        assert!((by - 130.0).abs() < 0.01);
        assert!((bw - 100.0).abs() < 0.01);
        assert!((bh - 40.0).abs() < 0.01);
    }

    // ---- MindMap ----

    #[test]
    fn test_mind_map_new_has_root() {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Test".to_string(), &mut id_gen);
        assert_eq!(map.node_count(), 1);
        assert!(map.node(map.root_id).is_some());
        assert_eq!(map.node(map.root_id).unwrap().text, "Central Idea");
    }

    #[test]
    fn test_mind_map_add_child() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let child = map.add_child(root, "Child 1".to_string(), pal.green, 1);
        assert!(child.is_some());
        assert_eq!(map.node_count(), 2);
        let cid = child.unwrap();
        assert_eq!(map.node(cid).unwrap().parent, Some(root));
        assert_eq!(map.node(root).unwrap().children.len(), 1);
    }

    #[test]
    fn test_mind_map_add_child_invalid_parent() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let result = map.add_child(999, "Orphan".to_string(), pal.green, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_mind_map_add_sibling() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let c2 = map.add_sibling(c1, "C2".to_string(), pal.green, 1);
        assert!(c2.is_some());
        assert_eq!(map.node_count(), 3);
        let parent_children = &map.node(root).unwrap().children;
        assert_eq!(parent_children.len(), 2);
        assert_eq!(parent_children[0], c1);
        assert_eq!(parent_children[1], c2.unwrap());
    }

    #[test]
    fn test_mind_map_add_sibling_to_root_fails() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let result = map.add_sibling(map.root_id, "Sibling".to_string(), pal.green, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_mind_map_subtree_ids() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let c2 = map.add_child(root, "C2".to_string(), pal.green, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();
        let ids = map.subtree_ids(root);
        assert_eq!(ids.len(), 4);
        assert!(ids.contains(&root));
        assert!(ids.contains(&c1));
        assert!(ids.contains(&c2));
        assert!(ids.contains(&gc1));
    }

    #[test]
    fn test_mind_map_subtree_ids_leaf() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let ids = map.subtree_ids(c1);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], c1);
    }

    #[test]
    fn test_mind_map_delete_subtree() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let _gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();
        let result = map.delete_subtree(c1);
        assert!(result.is_some());
        let (removed, parent_id, _) = result.unwrap();
        assert_eq!(removed.len(), 2); // c1 + gc1
        assert_eq!(parent_id, Some(root));
        assert_eq!(map.node_count(), 1); // only root remains
        assert!(map.node(root).unwrap().children.is_empty());
    }

    #[test]
    fn test_mind_map_delete_root_fails() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let result = map.delete_subtree(map.root_id);
        assert!(result.is_none());
    }

    #[test]
    fn test_mind_map_restore_subtree() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();

        let (removed, parent_id, child_index) = map.delete_subtree(c1).unwrap();
        assert_eq!(map.node_count(), 1);

        map.restore_subtree(&removed, parent_id, child_index);
        assert_eq!(map.node_count(), 3);
        assert!(map.node(c1).is_some());
        assert!(map.node(gc1).is_some());
        assert!(map.node(root).unwrap().children.contains(&c1));
    }

    #[test]
    fn test_mind_map_edit_text() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let old = map.edit_text(root, "New Root".to_string());
        assert_eq!(old, Some("Central Idea".to_string()));
        assert_eq!(map.node(root).unwrap().text, "New Root");
    }

    #[test]
    fn test_mind_map_edit_text_invalid_id() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let result = map.edit_text(999, "Nope".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_mind_map_change_color() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let result = map.change_color(root, pal.red, 2);
        assert!(result.is_some());
        assert_eq!(map.node(root).unwrap().color_index, 2);
    }

    #[test]
    fn test_mind_map_change_shape() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let old = map.change_shape(root, NodeShape::Diamond);
        assert_eq!(old, Some(NodeShape::Ellipse));
        assert_eq!(map.node(root).unwrap().shape, NodeShape::Diamond);
    }

    #[test]
    fn test_mind_map_toggle_collapse() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        assert!(!map.node(root).unwrap().collapsed);
        map.toggle_collapse(root);
        assert!(map.node(root).unwrap().collapsed);
        map.toggle_collapse(root);
        assert!(!map.node(root).unwrap().collapsed);
    }

    #[test]
    fn test_mind_map_move_node() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let old = map.move_node(root, 500.0, 300.0);
        assert!(old.is_some());
        assert!((map.node(root).unwrap().x - 500.0).abs() < 0.01);
        assert!((map.node(root).unwrap().y - 300.0).abs() < 0.01);
    }

    #[test]
    fn test_mind_map_visible_nodes_no_collapse() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let _gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();
        let visible = map.visible_node_ids();
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn test_mind_map_visible_nodes_collapsed() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let _gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();
        map.toggle_collapse(c1);
        let visible = map.visible_node_ids();
        assert_eq!(visible.len(), 2); // root + c1 (gc1 hidden)
    }

    #[test]
    fn test_mind_map_search_found() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "Alpha".to_string(), pal.green, 1);
        map.add_child(root, "Beta".to_string(), pal.red, 2);
        let results = map.search("alpha");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_mind_map_search_case_insensitive() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "MyNode".to_string(), pal.green, 1);
        let results = map.search("MYNODE");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_mind_map_search_empty_query() {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Test".to_string(), &mut id_gen);
        let results = map.search("");
        assert!(results.is_empty());
    }

    #[test]
    fn test_mind_map_search_no_match() {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Test".to_string(), &mut id_gen);
        let results = map.search("zzzzz");
        assert!(results.is_empty());
    }

    #[test]
    fn test_mind_map_export_text_root_only() {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Test".to_string(), &mut id_gen);
        let text = map.export_text();
        assert!(text.contains("Central Idea"));
        assert!(!text.contains("- "));
    }

    #[test]
    fn test_mind_map_export_text_with_children() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "Child A".to_string(), pal.green, 1);
        map.add_child(root, "Child B".to_string(), pal.red, 2);
        let text = map.export_text();
        assert!(text.contains("Central Idea\n"));
        assert!(text.contains("  - Child A\n"));
        assert!(text.contains("  - Child B\n"));
    }

    #[test]
    fn test_mind_map_depth_root() {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Test".to_string(), &mut id_gen);
        assert_eq!(map.depth(map.root_id), 0);
    }

    #[test]
    fn test_mind_map_depth_child() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        assert_eq!(map.depth(c1), 1);
    }

    #[test]
    fn test_mind_map_depth_grandchild() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();
        assert_eq!(map.depth(gc1), 2);
    }

    #[test]
    fn test_mind_map_descendant_count() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        map.add_child(c1, "GC1".to_string(), pal.red, 2);
        assert_eq!(map.descendant_count(root), 2);
        assert_eq!(map.descendant_count(c1), 1);
    }

    // ---- Auto-layout ----

    #[test]
    fn test_auto_layout_root_centered() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        auto_layout(&mut map, 500.0, 400.0);
        let root = map.node(map.root_id).unwrap();
        assert!((root.x - 500.0).abs() < 0.01);
        assert!((root.y - 400.0).abs() < 0.01);
    }

    #[test]
    fn test_auto_layout_children_positioned() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let c2 = map.add_child(root, "C2".to_string(), pal.red, 2).unwrap();
        auto_layout(&mut map, 500.0, 400.0);

        // c1 (even index) goes right, c2 (odd index) goes left
        let n1 = map.node(c1).unwrap();
        let n2 = map.node(c2).unwrap();
        assert!(n1.x > 500.0, "first child should be to the right");
        assert!(n2.x < 500.0, "second child should be to the left");
    }

    #[test]
    fn test_auto_layout_no_children() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        auto_layout(&mut map, 300.0, 200.0);
        // Should not panic with root only
        assert_eq!(map.node_count(), 1);
    }

    #[test]
    fn test_auto_layout_collapsed_subtree_not_laid_out() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), pal.green, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), pal.red, 2).unwrap();

        // Collapse c1
        map.toggle_collapse(c1);
        auto_layout(&mut map, 500.0, 400.0);

        // gc1 should still have its old position (layout skips it)
        let gc = map.node(gc1).unwrap();
        // Since it was just created, its position is 0,0 and layout shouldn't touch it
        // (because c1 is collapsed)
        assert!((gc.x).abs() < 0.01);
    }

    // ---- MindMapApp ----

    #[test]
    fn test_app_new_defaults() {
        let app = MindMapApp::new();
        assert_eq!(app.win_width, 1280.0);
        assert_eq!(app.win_height, 800.0);
        assert_eq!(app.maps.len(), 1);
        assert_eq!(app.active_map, 0);
        assert_eq!(app.zoom, 1.0);
        assert_eq!(app.pan_x, 0.0);
        assert_eq!(app.pan_y, 0.0);
        assert!(app.selected_node.is_none());
        assert!(app.show_sidebar);
        assert!(!app.show_search);
    }

    #[test]
    fn test_app_add_map() {
        let mut app = MindMapApp::new();
        app.add_map();
        assert_eq!(app.maps.len(), 2);
        assert_eq!(app.active_map, 1);
    }

    #[test]
    fn test_app_switch_map() {
        let mut app = MindMapApp::new();
        app.add_map();
        app.switch_map(0);
        assert_eq!(app.active_map, 0);
        app.switch_map(1);
        assert_eq!(app.active_map, 1);
    }

    #[test]
    fn test_app_switch_map_invalid() {
        let mut app = MindMapApp::new();
        app.switch_map(99);
        assert_eq!(app.active_map, 0);
    }

    #[test]
    fn a_map_with_nothing_unsaved_closes_at_once() {
        let mut app = MindMapApp::new();
        app.add_map();
        assert_eq!(app.maps.len(), 2);
        assert_eq!(app.handle_event(&press_ctrl(Key::W)), EventResult::Consumed);
        assert!(
            app.question.is_none(),
            "asked about a map with nothing unsaved"
        );
        assert_eq!(app.maps.len(), 1);
        assert_eq!(app.active_map, 0);
    }

    #[test]
    fn closing_the_last_map_leaves_a_fresh_one() {
        let mut app = MindMapApp::new();
        let id = app.active_map_ref().id;
        app.handle_event(&press_ctrl(Key::W));
        assert_eq!(app.maps.len(), 1, "the window was left with no map");
        let fresh = app.active_map_ref();
        assert_ne!(fresh.id, id, "the closed map is still there");
        assert_eq!(fresh.node_count(), 1);
        assert!(!fresh.dirty);
        assert_eq!(fresh.name, "Mind Map 1");
    }

    #[test]
    fn a_new_map_is_not_named_after_one_still_open() {
        let mut app = MindMapApp::new();
        app.add_map();
        app.add_map();
        // Mind Map 1, 2, 3; close 1, and the next new map is 1 again -- not a
        // second 3, which a count of the maps would give.
        app.close_map(app.maps[0].id);
        app.add_map();
        let mut names: Vec<&str> = app.maps.iter().map(|m| m.name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, ["Mind Map 1", "Mind Map 2", "Mind Map 3"]);
    }

    #[test]
    fn test_app_zoom_in() {
        let mut app = MindMapApp::new();
        app.zoom_in();
        assert!(app.zoom > 1.0);
    }

    #[test]
    fn test_app_zoom_out() {
        let mut app = MindMapApp::new();
        app.zoom_out();
        assert!(app.zoom < 1.0);
    }

    #[test]
    fn test_app_zoom_clamp_min() {
        let mut app = MindMapApp::new();
        app.set_zoom(0.001);
        assert!((app.zoom - MIN_ZOOM).abs() < 0.001);
    }

    #[test]
    fn test_app_zoom_clamp_max() {
        let mut app = MindMapApp::new();
        app.set_zoom(999.0);
        assert!((app.zoom - MAX_ZOOM).abs() < 0.001);
    }

    #[test]
    fn test_app_reset_view() {
        let mut app = MindMapApp::new();
        app.set_zoom(2.5);
        app.pan_x = 100.0;
        app.pan_y = -50.0;
        app.reset_view();
        assert_eq!(app.zoom, 1.0);
        assert_eq!(app.pan_x, 0.0);
        assert_eq!(app.pan_y, 0.0);
    }

    #[test]
    fn test_app_add_child_to_root() {
        let mut app = MindMapApp::new();
        let result = app.add_child_to_selected("New Child".to_string());
        assert!(result.is_some());
        assert_eq!(app.active_map_ref().node_count(), 2);
        assert_eq!(app.selected_node, result);
    }

    #[test]
    fn test_app_add_child_to_selected() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        let gc1 = app.add_child_to_selected("GC1".to_string());
        assert!(gc1.is_some());
        assert_eq!(app.active_map_ref().node_count(), 3);
    }

    #[test]
    fn test_app_add_sibling() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        let c2 = app.add_sibling_to_selected("C2".to_string());
        assert!(c2.is_some());
        assert_eq!(app.active_map_ref().node_count(), 3);
    }

    #[test]
    fn test_app_add_sibling_to_root_fails() {
        let mut app = MindMapApp::new();
        app.selected_node = Some(app.active_map_ref().root_id);
        let result = app.add_sibling_to_selected("Sibling".to_string());
        assert!(result.is_none());
    }

    #[test]
    fn test_app_delete_selected() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        assert!(app.delete_selected());
        assert_eq!(app.active_map_ref().node_count(), 1);
    }

    #[test]
    fn test_app_delete_root_fails() {
        let mut app = MindMapApp::new();
        app.selected_node = Some(app.active_map_ref().root_id);
        assert!(!app.delete_selected());
    }

    #[test]
    fn test_app_delete_nothing_selected() {
        let mut app = MindMapApp::new();
        assert!(!app.delete_selected());
    }

    #[test]
    fn test_app_edit_text() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        app.start_editing();
        assert_eq!(app.editing_node, Some(root));
        assert_eq!(app.edit_buffer, "Central Idea");
        app.edit_buffer = "New Idea".to_string();
        app.finish_editing();
        assert!(app.editing_node.is_none());
        assert_eq!(app.active_map_ref().node(root).unwrap().text, "New Idea");
    }

    #[test]
    fn test_app_cancel_editing() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        app.start_editing();
        app.edit_buffer = "Changed".to_string();
        app.cancel_editing();
        assert!(app.editing_node.is_none());
        assert_eq!(
            app.active_map_ref().node(root).unwrap().text,
            "Central Idea"
        );
    }

    #[test]
    fn test_app_cycle_color() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let old_index = app.active_map_ref().node(root).unwrap().color_index;
        app.cycle_color();
        let new_index = app.active_map_ref().node(root).unwrap().color_index;
        assert_ne!(old_index, new_index);
    }

    #[test]
    fn test_app_cycle_shape() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let old_shape = app.active_map_ref().node(root).unwrap().shape;
        app.cycle_shape();
        let new_shape = app.active_map_ref().node(root).unwrap().shape;
        assert_ne!(old_shape, new_shape);
    }

    #[test]
    fn test_app_toggle_collapse() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        assert!(!app.active_map_ref().node(root).unwrap().collapsed);
        app.toggle_collapse_selected();
        assert!(app.active_map_ref().node(root).unwrap().collapsed);
    }

    // ---- Undo / Redo ----

    #[test]
    fn test_app_undo_add_child() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("Test Child".to_string());
        assert_eq!(app.active_map_ref().node_count(), 2);
        app.undo();
        assert_eq!(app.active_map_ref().node_count(), 1);
    }

    #[test]
    fn test_app_redo_add_child() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("Test Child".to_string());
        app.undo();
        assert_eq!(app.active_map_ref().node_count(), 1);
        app.redo();
        assert_eq!(app.active_map_ref().node_count(), 2);
    }

    #[test]
    fn test_app_undo_delete() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        app.delete_selected();
        assert_eq!(app.active_map_ref().node_count(), 1);
        app.undo();
        assert_eq!(app.active_map_ref().node_count(), 2);
    }

    #[test]
    fn test_app_undo_edit_text() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        app.start_editing();
        app.edit_buffer = "Changed".to_string();
        app.finish_editing();
        assert_eq!(app.active_map_ref().node(root).unwrap().text, "Changed");
        app.undo();
        assert_eq!(
            app.active_map_ref().node(root).unwrap().text,
            "Central Idea"
        );
    }

    #[test]
    fn test_app_undo_color_change() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let old_idx = app.active_map_ref().node(root).unwrap().color_index;
        app.cycle_color();
        assert_ne!(
            app.active_map_ref().node(root).unwrap().color_index,
            old_idx
        );
        app.undo();
        assert_eq!(
            app.active_map_ref().node(root).unwrap().color_index,
            old_idx
        );
    }

    #[test]
    fn test_app_undo_shape_change() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let old_shape = app.active_map_ref().node(root).unwrap().shape;
        app.cycle_shape();
        app.undo();
        assert_eq!(app.active_map_ref().node(root).unwrap().shape, old_shape);
    }

    #[test]
    fn test_app_can_undo_redo() {
        let mut app = MindMapApp::new();
        assert!(!app.can_undo());
        assert!(!app.can_redo());
        app.add_child_to_selected("C1".to_string());
        assert!(app.can_undo());
        assert!(!app.can_redo());
        app.undo();
        assert!(!app.can_undo());
        assert!(app.can_redo());
    }

    #[test]
    fn test_app_redo_cleared_on_new_action() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("C1".to_string());
        app.undo();
        assert!(app.can_redo());
        app.add_child_to_selected("C2".to_string());
        assert!(!app.can_redo());
    }

    #[test]
    fn test_app_undo_stack_limit() {
        let mut app = MindMapApp::new();
        for i in 0..MAX_UNDO + 50 {
            app.add_child_to_selected(format!("Node {i}"));
        }
        assert!(app.active_map_ref().undo_stack.len() <= MAX_UNDO);
    }

    // ---- Search ----

    #[test]
    fn test_app_search() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("Alpha".to_string());
        app.add_child_to_selected("Beta".to_string());
        app.set_search_query("alpha".to_string());
        assert_eq!(app.search_results.len(), 1);
    }

    #[test]
    fn test_app_search_next_prev() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("Alpha".to_string());
        app.selected_node = None;
        app.add_child_to_selected("Alpha Two".to_string());
        app.set_search_query("Alpha".to_string());
        assert_eq!(app.search_results.len(), 2);

        app.next_search_result();
        assert_eq!(app.search_index, 1);
        app.next_search_result();
        assert_eq!(app.search_index, 0); // wraps around

        app.prev_search_result();
        assert_eq!(app.search_index, 1);
    }

    #[test]
    fn test_app_toggle_search() {
        let mut app = MindMapApp::new();
        assert!(!app.show_search);
        app.toggle_search();
        assert!(app.show_search);
        app.search_query = "test".to_string();
        app.toggle_search();
        assert!(!app.show_search);
        assert!(app.search_query.is_empty());
    }

    // ---- Coordinate transforms ----

    #[test]
    fn test_screen_to_canvas_identity() {
        let app = MindMapApp::new();
        let cx0 = app.canvas_x();
        let cy0 = app.canvas_y();
        let (cx, cy) = app.screen_to_canvas(cx0, cy0);
        assert!((cx).abs() < 0.01);
        assert!((cy).abs() < 0.01);
    }

    #[test]
    fn test_canvas_to_screen_roundtrip() {
        let app = MindMapApp::new();
        let (sx, sy) = app.canvas_to_screen(100.0, 200.0);
        let (cx, cy) = app.screen_to_canvas(sx, sy);
        assert!((cx - 100.0).abs() < 0.01);
        assert!((cy - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_canvas_to_screen_with_zoom() {
        let mut app = MindMapApp::new();
        app.set_zoom(2.0);
        let (sx, sy) = app.canvas_to_screen(100.0, 200.0);
        let (cx, cy) = app.screen_to_canvas(sx, sy);
        assert!((cx - 100.0).abs() < 0.01);
        assert!((cy - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_canvas_to_screen_with_pan() {
        let mut app = MindMapApp::new();
        app.pan_x = 50.0;
        app.pan_y = -30.0;
        let (sx, sy) = app.canvas_to_screen(100.0, 200.0);
        let (cx, cy) = app.screen_to_canvas(sx, sy);
        assert!((cx - 100.0).abs() < 0.01);
        assert!((cy - 200.0).abs() < 0.01);
    }

    // ---- Hit testing ----

    #[test]
    fn test_hit_test_root() {
        let app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        let root_node = app.active_map_ref().node(root).unwrap();
        let result = app.hit_test_canvas(root_node.x, root_node.y);
        assert_eq!(result, Some(root));
    }

    #[test]
    fn test_hit_test_miss() {
        let app = MindMapApp::new();
        let result = app.hit_test_canvas(-9999.0, -9999.0);
        assert!(result.is_none());
    }

    // ---- Drag operations ----

    #[test]
    fn test_start_node_drag() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.start_node_drag(root, 500.0, 400.0);
        match &app.drag {
            DragState::DraggingNode { node_id, .. } => assert_eq!(*node_id, root),
            _ => panic!("Expected DraggingNode"),
        }
    }

    #[test]
    fn test_start_pan() {
        let mut app = MindMapApp::new();
        app.start_pan(100.0, 200.0);
        match &app.drag {
            DragState::Panning {
                start_mouse_x,
                start_mouse_y,
                ..
            } => {
                assert!((start_mouse_x - 100.0).abs() < 0.01);
                assert!((start_mouse_y - 200.0).abs() < 0.01);
            }
            _ => panic!("Expected Panning"),
        }
    }

    #[test]
    fn test_end_drag() {
        let mut app = MindMapApp::new();
        app.start_pan(100.0, 200.0);
        app.end_drag();
        assert_eq!(app.drag, DragState::None);
    }

    // ---- Selection navigation ----

    #[test]
    fn test_select_parent() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        app.select_parent();
        assert_eq!(app.selected_node, Some(root));
    }

    #[test]
    fn test_select_first_child() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(root);
        app.select_first_child();
        assert_eq!(app.selected_node, Some(c1));
    }

    #[test]
    fn test_select_next_sibling() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = None;
        let c2 = app.add_child_to_selected("C2".to_string()).unwrap();
        app.selected_node = Some(c1);
        app.select_next_sibling();
        assert_eq!(app.selected_node, Some(c2));
    }

    #[test]
    fn test_select_prev_sibling() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = None;
        let c2 = app.add_child_to_selected("C2".to_string()).unwrap();
        app.selected_node = Some(c2);
        app.select_prev_sibling();
        assert_eq!(app.selected_node, Some(c1));
    }

    #[test]
    fn test_select_parent_on_root_does_nothing() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        app.select_parent();
        assert_eq!(app.selected_node, Some(root));
    }

    // ---- Export ----

    #[test]
    fn test_export_text() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("Branch A".to_string());
        let text = app.export_text();
        assert!(text.contains("Central Idea"));
        assert!(text.contains("Branch A"));
    }

    // ---- Rendering ----

    #[test]
    fn test_render_produces_commands() {
        let app = MindMapApp::new();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_search() {
        let mut app = MindMapApp::new();
        app.show_search = true;
        app.search_query = "test".to_string();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_sidebar_hidden() {
        let mut app = MindMapApp::new();
        app.show_sidebar = false;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_children() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("C1".to_string());
        app.add_child_to_selected("C2".to_string());
        let cmds = app.render_commands();
        assert!(cmds.len() > 10); // should have many render commands
    }

    #[test]
    fn test_render_collapsed_hides_children() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        app.add_child_to_selected("GC1".to_string());
        let cmds_expanded = app.render_commands();

        app.selected_node = Some(c1);
        app.toggle_collapse_selected();
        let cmds_collapsed = app.render_commands();

        // Collapsed should have fewer render commands (no grandchild rendered)
        assert!(cmds_collapsed.len() < cmds_expanded.len());
    }

    // ---- Helper functions ----

    #[test]
    fn test_node_text_color_dark_bg() {
        let color = node_text_color(Color::from_hex(0x1E1E2E));
        // Dark background should get light text
        assert!(color.r > 150);
    }

    #[test]
    fn test_node_text_color_light_bg() {
        let color = node_text_color(Color::from_hex(0xCDD6F4));
        // Light background should get dark text
        assert!(color.r < 100);
    }

    /// The status bar shortens a long node name so that the id *after* it
    /// survives — that is the whole reason it elides here instead of leaving it
    /// to the renderer — and the result fits the width it is drawn into.
    #[test]
    fn a_long_node_name_is_shortened_around_the_id_that_follows_it() {
        let mut app = MindMapApp::new();
        let id = app.active_map_ref().root_id;
        if let Some(node) = app.active_map_mut().node_mut(id) {
            node.text = "A node whose name is far too long for the status bar, Café 日本語 and all"
                .to_string();
        }
        app.selected_node = Some(id);

        let mut cmds = Vec::new();
        app.render_status_bar(&mut cmds);
        let line = cmds
            .into_iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Text { text, .. } if text.starts_with("Selected: ") => Some(text),
                _ => None,
            })
            .expect("the status bar names the selection");

        assert!(
            line.ends_with(&format!("(ID: {id})")),
            "the id survives the shortening: {line}"
        );
        assert!(line.contains('\u{2026}'), "and the cut is marked: {line}");
        assert!(
            text::measure(&line, SEL_INFO_SIZE, FontWeightHint::Regular) <= SEL_INFO_WIDTH + 0.5,
            "and the whole line fits the width it is drawn into: {line}"
        );
    }

    #[test]
    fn test_corner_radii_rectangle() {
        let app = MindMapApp::new();
        let cr = app.corner_radii_for_shape(NodeShape::Rectangle, 8.0);
        // CornerRadii::ZERO
        let zero = CornerRadii::ZERO;
        assert_eq!(cr, zero);
    }

    #[test]
    fn test_corner_radii_rounded() {
        let app = MindMapApp::new();
        let cr = app.corner_radii_for_shape(NodeShape::RoundedRect, 8.0);
        let expected = CornerRadii::all(8.0);
        assert_eq!(cr, expected);
    }

    #[test]
    fn test_corner_radii_ellipse() {
        let app = MindMapApp::new();
        let cr = app.corner_radii_for_shape(NodeShape::Ellipse, 8.0);
        let expected = CornerRadii::all(16.0);
        assert_eq!(cr, expected);
    }

    /// `B` closes the sidebar.
    ///
    /// `show_sidebar` gates the sidebar's draw and had no writer, so the panel
    /// could never be closed.
    #[test]
    fn b_closes_the_sidebar() {
        let mut app = MindMapApp::new();
        assert!(app.show_sidebar, "control: the sidebar starts open");

        app.handle_key(&KeyEvent {
            key: Key::B,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::from("b"),
        });

        assert!(!app.show_sidebar, "B did not close the sidebar");
    }

    #[test]
    fn test_corner_radii_pill() {
        let app = MindMapApp::new();
        let cr = app.corner_radii_for_shape(NodeShape::Pill, 8.0);
        let expected = CornerRadii::all(24.0);
        assert_eq!(cr, expected);
    }

    // ---- Canvas area ----

    #[test]
    fn test_canvas_dimensions() {
        let app = MindMapApp::new();
        let w = app.canvas_width();
        let h = app.canvas_height();
        assert!(w > 0.0);
        assert!(h > 0.0);
        assert!(w < app.win_width);
        assert!(h < app.win_height);
    }

    #[test]
    fn test_canvas_dimensions_no_sidebar() {
        let mut app = MindMapApp::new();
        let w_with = app.canvas_width();
        app.show_sidebar = false;
        let w_without = app.canvas_width();
        assert!(w_without > w_with);
    }

    // ---- Measure subtree height ----

    #[test]
    fn test_measure_subtree_leaf() {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Test".to_string(), &mut id_gen);
        let h = measure_subtree_height(&map, map.root_id);
        assert!((h - ROOT_NODE_H).abs() < 0.01);
    }

    #[test]
    fn test_measure_subtree_with_children() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "C1".to_string(), pal.green, 1);
        map.add_child(root, "C2".to_string(), pal.red, 2);
        let h = measure_subtree_height(&map, root);
        // Should be at least 2 children heights + gap
        assert!(h >= DEFAULT_NODE_H * 2.0 + RADIAL_V_GAP);
    }

    // ---- Multiple undo/redo ----

    #[test]
    fn test_multiple_undo_redo() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("A".to_string());
        app.add_child_to_selected("B".to_string());
        assert_eq!(app.active_map_ref().node_count(), 3);

        app.undo();
        assert_eq!(app.active_map_ref().node_count(), 2);
        app.undo();
        assert_eq!(app.active_map_ref().node_count(), 1);

        app.redo();
        assert_eq!(app.active_map_ref().node_count(), 2);
        app.redo();
        assert_eq!(app.active_map_ref().node_count(), 3);
    }

    #[test]
    fn test_undo_toggle_collapse() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        assert!(!app.active_map_ref().node(root).unwrap().collapsed);
        app.toggle_collapse_selected();
        assert!(app.active_map_ref().node(root).unwrap().collapsed);
        app.undo();
        assert!(!app.active_map_ref().node(root).unwrap().collapsed);
    }

    // ---- Drag with undo ----

    #[test]
    fn test_drag_node_creates_undo() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        let orig_x = app.active_map_ref().node(root).unwrap().x;
        let orig_y = app.active_map_ref().node(root).unwrap().y;

        app.start_node_drag(root, orig_x, orig_y);
        // Simulate moving to (orig + 100, orig + 50)
        app.active_map_mut()
            .move_node(root, orig_x + 100.0, orig_y + 50.0);
        app.end_drag();

        assert!(app.can_undo());
    }

    // -- Following the user's theme -------------------------------------------

    /// The window draws in the user's colours rather than in constants of its
    /// own.
    ///
    /// Asserted on the rectangles emitted, not on the `palette` field: a field
    /// that was assigned proves nothing a user would see.
    #[test]
    fn the_window_draws_in_the_theme_it_is_given() {
        fn theme(
            mode: appearance::ThemeMode,
            contrast: Option<appearance::HighContrastScheme>,
        ) -> Palette {
            Palette::from_settings(&appearance::AppearanceSettings {
                theme_mode: mode,
                high_contrast: contrast,
                ..appearance::AppearanceSettings::default()
            })
        }

        fn fills(app: &mut MindMapApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = MindMapApp::new();

        app.theme_changed(&theme(appearance::ThemeMode::Dark, None));
        let dark = fills(&mut app);
        assert!(!dark.is_empty(), "the window drew no filled rectangles");

        app.theme_changed(&theme(appearance::ThemeMode::Light, None));
        let light = fills(&mut app);
        assert_eq!(dark.len(), light.len(), "the theme changed the layout");
        assert_ne!(
            dark, light,
            "the window drew identically on the dark and light themes, so it \
             is still painting from constants"
        );

        // High contrast is the case a hardcoded palette fails silently: the
        // user asks for maximum legibility and this window alone ignores them.
        app.theme_changed(&theme(
            appearance::ThemeMode::Dark,
            Some(appearance::HighContrastScheme::WhiteOnBlack),
        ));
        assert_ne!(
            dark,
            fills(&mut app),
            "high contrast reached every other surface but not this window"
        );
    }

    // ------------------------------------------------------------------
    // A map's own file, and the question before one is lost
    // ------------------------------------------------------------------

    /// A directory of this test's own, empty.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("slateos-mindmap-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// A key with Ctrl and Shift held.
    fn press_ctrl_shift(k: Key) -> Event {
        let mut modifiers = Modifiers::ctrl();
        modifiers.shift = true;
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    /// A left click at a point.
    fn click(app: &mut MindMapApp, (x, y): (f32, f32)) -> EventResult {
        app.handle_event(&mouse(MouseEventKind::Press(MouseButton::Left), x, y))
    }

    /// The middle of a rectangle.
    fn centre((x, y, w, h): (f32, f32, f32, f32)) -> (f32, f32) {
        (x + w / 2.0, y + h / 2.0)
    }

    /// A left click in the middle of whatever `rect` finds on the app.
    fn click_on(
        app: &mut MindMapApp,
        rect: impl Fn(&MindMapApp) -> (f32, f32, f32, f32),
    ) -> EventResult {
        let at = centre(rect(app));
        click(app, at)
    }

    /// Something of every kind a user can set: text with a line break, a
    /// colon and edge spaces; a colour and a shape cycled; a fold; a node
    /// dragged where no layout would put it.
    fn rich_map(app: &mut MindMapApp) {
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let a = app
            .add_child_to_selected(String::from("Plans: 2026"))
            .expect("a");
        app.selected_node = Some(root);
        let b = app
            .add_child_to_selected(String::from("two\nlines"))
            .expect("b");
        app.selected_node = Some(a);
        let c = app
            .add_child_to_selected(String::from("  spaced  "))
            .expect("c");
        app.selected_node = Some(b);
        app.cycle_color();
        app.cycle_shape();
        app.cycle_shape();
        app.selected_node = Some(a);
        app.toggle_collapse_selected();
        app.start_node_drag(c, 0.0, 0.0);
        app.update_drag(123.5, -45.25);
        app.end_drag();
    }

    /// Everything about a map a save must keep, walked from the root so the
    /// order does not depend on how the nodes are stored.
    fn describe(map: &MindMap) -> String {
        let mut out = format!("{:?}\n", map.name);
        let mut stack = vec![(map.root_id, 0_usize)];
        while let Some((id, depth)) = stack.pop() {
            let n = map.node(id).expect("a node its parent names");
            out.push_str(&format!(
                "{}{} {:?} parent={:?} {} {} {:?} {} {} {} {} folded={}\n",
                "  ".repeat(depth),
                n.id,
                n.text,
                n.parent,
                colour_hex(n.color),
                n.color_index,
                n.shape,
                n.x,
                n.y,
                n.width,
                n.height,
                n.collapsed
            ));
            for child in n.children.iter().rev() {
                stack.push((*child, depth + 1));
            }
        }
        out
    }

    /// **A map saved and opened again is the same map** -- every node, where
    /// it hangs, in its order, with every property a user can set.
    #[test]
    fn a_map_saved_and_opened_again_is_the_same_map() {
        let dir = scratch("roundtrip");
        let path = dir.join("plans.mindmap");
        let mut app = MindMapApp::new();
        rich_map(&mut app);
        assert!(app.active_map_ref().dirty, "control: the map was changed");
        let id = app.active_map_ref().id;
        let said = app.save_map_as(id, &path).expect("saved");
        assert!(said.starts_with("Saved"), "{said}");
        assert!(!app.active_map_ref().dirty, "a save left the map marked");
        assert_eq!(
            app.active_map_ref().name,
            "plans",
            "the map did not take its file's name"
        );
        let saved = describe(app.active_map_ref());
        assert!(saved.contains("folded=true"), "control: {saved}");

        let mut other = MindMapApp::new();
        let said = other.open_file(&path);
        assert_eq!(said, format!("Opened {}", path.display()));
        assert_eq!(
            other.maps.len(),
            2,
            "the file did not open in a tab of its own"
        );
        let opened = other.active_map_ref();
        assert_eq!(describe(opened), saved);
        assert!(!opened.dirty, "a map just opened has nothing unsaved");
        assert_eq!(opened.document_path.as_deref(), Some(path.as_path()));
        assert_eq!(other.title(), "plans — Mind Map");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A node added to an opened map takes a number the file did not use.
    #[test]
    fn a_node_added_after_opening_takes_a_number_of_its_own() {
        let dir = scratch("numbers");
        let path = dir.join("n.mindmap");
        let mut app = MindMapApp::new();
        rich_map(&mut app);
        let id = app.active_map_ref().id;
        app.save_map_as(id, &path).expect("saved");
        let mut other = MindMapApp::new();
        other.open_file(&path);
        let before = other.active_map_ref().node_count();
        let root = other.active_map_ref().root_id;
        other.selected_node = Some(root);
        other
            .add_child_to_selected(String::from("new"))
            .expect("added");
        assert_eq!(
            other.active_map_ref().node_count(),
            before + 1,
            "the new node took the place of one read from the file"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Ctrl+S over a map with a file writes it there, without asking; a map
    /// with none asks where; Ctrl+Shift+S always asks.
    #[test]
    fn ctrl_s_saves_over_the_maps_own_file_and_asks_only_when_it_has_none() {
        let dir = scratch("ctrl-s");
        let path = dir.join("own.mindmap");
        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press_ctrl(Key::S));
        assert!(app.picker.is_open(), "a map with no file did not ask where");
        assert_eq!(app.picker_for, PickerFor::Save(app.active_map_ref().id));
        app.picker.close();
        app.picked(&path);
        assert!(!app.active_map_ref().dirty);
        assert_eq!(
            app.last_file_action.as_deref(),
            Some(format!("Saved {}", path.display()).as_str())
        );

        app.handle_event(&press(Key::Tab));
        assert!(app.active_map_ref().dirty);
        app.handle_event(&press_ctrl(Key::S));
        assert!(!app.picker.is_open(), "asked where, for a map with a file");
        assert!(!app.active_map_ref().dirty, "not saved over its own file");
        let on_disk = std::fs::read_to_string(&path).expect("saved");
        assert_eq!(on_disk, mindmap_document(app.active_map_ref()).to_text());

        app.handle_event(&press_ctrl_shift(Key::S));
        assert!(app.picker.is_open(), "Ctrl+Shift+S did not ask where");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Every change marks the map, and the title and its tab say so; opening
    /// or adding a map, moving round the maps and selecting do not.
    #[test]
    fn a_change_marks_the_map_and_the_title_and_tab_say_so() {
        let mut app = MindMapApp::new();
        assert!(!app.active_map_ref().dirty, "a new map has nothing unsaved");
        assert!(!app.title().starts_with('*'));
        app.handle_event(&press(Key::Tab));
        assert!(app.active_map_ref().dirty);
        assert!(app.title().starts_with('*'), "{}", app.title());
        let name = app.active_map_ref().name.clone();
        assert!(
            drawn_text(&app).contains(&format!("*{name}")),
            "the tab does not say its map is unsaved"
        );
        // A second map, fresh; moving to it and back changes neither.
        app.add_map();
        assert!(!app.active_map_ref().dirty);
        app.handle_event(&press_ctrl(Key::Tab));
        app.handle_event(&press_ctrl(Key::Tab));
        app.handle_event(&press(Key::Up));
        assert!(!app.active_map_ref().dirty, "looking at a map changed it");
        // Renaming a node to what it already says is not a change.
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        app.handle_event(&press(Key::F2));
        app.handle_event(&press(Key::Enter));
        assert!(
            !app.active_map_ref().dirty,
            "an unchanged name marked the map"
        );
        // Undo is a change too: undoing past a save leaves a map its file
        // does not hold.
        app.handle_event(&press(Key::Tab));
        app.active_map_mut().dirty = false;
        app.handle_event(&press_ctrl(Key::Z));
        assert!(app.active_map_ref().dirty, "an undo did not mark the map");
    }

    /// L marks the map only when it moves something.
    #[test]
    fn laying_the_map_out_again_marks_it_only_when_it_moves_a_node() {
        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::L));
        assert!(
            !app.active_map_ref().dirty,
            "a layout that moved nothing marked the map"
        );
        let root = app.active_map_ref().root_id;
        app.active_map_mut().move_node(root, 5.0, 5.0);
        app.handle_event(&press(Key::L));
        assert!(
            app.active_map_ref().dirty,
            "a layout that moved a node did not mark it"
        );
    }

    /// **Each map keeps its own history.** The window's single history was
    /// replayed onto whichever map was showing, and node numbers repeat from
    /// map to map -- so Ctrl+Z on one map acted on another's change.
    #[test]
    fn undo_on_one_map_never_touches_another() {
        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::Tab));
        let first = describe(app.active_map_ref());
        app.add_map();
        assert!(!app.can_undo(), "a new map has the last one's history");
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Tab));
        let second_before_undo = app.active_map_ref().node_count();

        app.switch_map(0);
        assert_eq!(describe(app.active_map_ref()), first);
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Consumed);
        assert_eq!(
            app.active_map_ref().node_count(),
            1,
            "the first map's change was not undone"
        );
        assert!(!app.can_undo(), "the first map had one change");

        app.switch_map(1);
        assert_eq!(
            app.active_map_ref().node_count(),
            second_before_undo,
            "an undo on the first map reached the second"
        );
        app.handle_event(&press_ctrl(Key::Z));
        assert_eq!(app.active_map_ref().node_count(), second_before_undo - 1);
        app.handle_event(&press_ctrl(Key::Y));
        assert_eq!(app.active_map_ref().node_count(), second_before_undo);
    }

    /// Closing a map with changes asks, and each answer does what it says.
    #[test]
    fn closing_a_changed_map_asks_and_each_answer_does_what_it_says() {
        let dir = scratch("close-map");
        let path = dir.join("kept.mindmap");
        let mut app = MindMapApp::new();
        app.add_map();
        app.handle_event(&press(Key::Tab));
        let id = app.active_map_ref().id;

        // Cancel: still open, still changed.
        app.handle_event(&press_ctrl(Key::W));
        assert_eq!(
            app.question.as_ref().map(Question::pending),
            Some(CloseScope::Map(id))
        );
        // A key under the question goes to the question, not the map.
        let n = app.active_map_ref().node_count();
        app.handle_event(&press(Key::Tab));
        assert_eq!(
            app.active_map_ref().node_count(),
            n,
            "a key reached the map under the question"
        );
        app.handle_event(&press(Key::Escape));
        assert!(app.question.is_none());
        assert_eq!(app.maps.len(), 2);
        assert!(app.active_map_ref().dirty);

        // Save, with no file yet: asks where, then closes once written.
        app.handle_event(&press_ctrl(Key::W));
        app.handle_event(&press(Key::S));
        assert!(app.picker.is_open(), "Save did not ask where");
        assert_eq!(app.picker_for, PickerFor::SaveThenClose(id));
        app.picker.close();
        app.picked(&path);
        assert!(app.index_of(id).is_none(), "saved, and not closed");
        assert!(path.exists(), "closed, and not saved");

        // Don't save: gone, and nothing written.
        app.handle_event(&press(Key::Tab));
        let other = app.active_map_ref().id;
        app.handle_event(&press_ctrl(Key::W));
        app.handle_event(&press(Key::D));
        assert!(app.index_of(other).is_none(), "Don't save did not close it");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A map whose save fails is not closed: it is still the only copy.
    #[test]
    fn a_map_whose_save_fails_stays_open() {
        let dir = scratch("save-fails");
        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::Tab));
        let id = app.active_map_ref().id;
        // A file inside a directory that does not exist cannot be written.
        app.active_map_mut().document_path = Some(dir.join("missing").join("m.mindmap"));
        app.handle_event(&press_ctrl(Key::W));
        app.handle_event(&press(Key::S));
        assert!(
            app.index_of(id).is_some(),
            "closed although the save failed"
        );
        assert!(app.active_map_ref().dirty);
        let said = app.last_file_action.clone().unwrap_or_default();
        assert!(said.starts_with("Could not save"), "{said}");
        assert!(said.ends_with("so the map stays open"), "{said}");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// The window closes at once with nothing unsaved; with maps unsaved it
    /// asks about all of them, saves those with files, asks where for the
    /// rest, and goes once nothing is left.
    #[test]
    fn closing_the_window_asks_about_every_unsaved_map() {
        let dir = scratch("close-window");
        let own = dir.join("own.mindmap");
        let new = dir.join("new.mindmap");

        let mut clean = MindMapApp::new();
        assert_eq!(clean.on_event(&Event::CloseRequested), Response::Exit);

        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::Tab));
        let first = app.active_map_ref().id;
        app.save_map_as(first, &own).expect("saved");
        app.handle_event(&press(Key::Tab));
        app.add_map();
        app.handle_event(&press(Key::Tab));
        let second = app.active_map_ref().id;
        app.add_map(); // unchanged: not asked about

        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        let asked = app
            .question
            .as_ref()
            .map(|q| q.message().to_owned())
            .unwrap_or_default();
        assert!(asked.starts_with("2 documents"), "{asked}");
        assert!(
            asked.contains("own") && asked.contains("Mind Map 1"),
            "{asked}"
        );
        let drawn: String = app
            .render(1280.0, 800.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            drawn.contains("Unsaved changes"),
            "the question is not drawn: {drawn}"
        );

        // Save: the one with a file is written there; the other asks where.
        assert_eq!(app.on_event(&press(Key::S)), Response::Redraw);
        assert!(!app.maps[app.index_of(first).expect("first")].dirty);
        assert!(app.picker.is_open());
        assert_eq!(app.picker_for, PickerFor::SaveThenQuit(second));
        assert_eq!(
            app.active_map_ref().id,
            second,
            "not showing the map it asks about"
        );
        app.picker.close();
        app.picked(&new);
        assert!(app.quit, "everything is saved and the window did not go");
        assert!(new.exists());
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A failed save while the window closes keeps the window open, and says
    /// so.
    #[test]
    fn a_failed_save_keeps_the_window_open() {
        let dir = scratch("quit-fails");
        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::Tab));
        app.active_map_mut().document_path = Some(dir.join("missing").join("m.mindmap"));
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(app.on_event(&press(Key::S)), Response::Redraw);
        assert!(!app.quit);
        assert!(
            !app.picker.is_open(),
            "went on to ask where to save after a save failed"
        );
        let said = app.last_file_action.clone().unwrap_or_default();
        assert!(said.ends_with("so the window stays open"), "{said}");
        // Don't save lets it go.
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(app.on_event(&press(Key::D)), Response::Exit);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A name half typed is part of the map when the window is asked to
    /// close: it is kept, and asked about.
    #[test]
    fn a_name_being_typed_is_asked_about_on_close() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        app.handle_event(&press(Key::F2));
        app.handle_event(&typed('!'));
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert!(app.active_map_ref().dirty);
        assert!(
            app.active_map_ref()
                .node(root)
                .expect("root")
                .text
                .ends_with('!')
        );
    }

    /// One file, one tab: opening a file that is open already shows it.
    #[test]
    fn opening_a_file_that_is_open_already_shows_its_tab() {
        let dir = scratch("open-twice");
        let path = dir.join("once.mindmap");
        let mut app = MindMapApp::new();
        let id = app.active_map_ref().id;
        app.save_map_as(id, &path).expect("saved");
        app.add_map();
        let said = app.open_file(&path);
        assert!(said.contains("open already"), "{said}");
        assert_eq!(app.maps.len(), 2, "a second tab on one file");
        assert_eq!(app.active_map_ref().id, id);
        // The same file by another spelling of its path.
        std::fs::create_dir_all(dir.join("sub")).expect("sub");
        app.switch_map(1);
        let said = app.open_file(&dir.join("sub").join("..").join("once.mindmap"));
        assert!(said.contains("open already"), "{said}");
        assert_eq!(
            app.maps.len(),
            2,
            "a second tab on one file, named another way"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Closing a map that is not showing leaves the one showing, selection
    /// and all; closing the one showing shows its neighbour.
    #[test]
    fn closing_another_map_keeps_this_one_showing() {
        let mut app = MindMapApp::new();
        app.add_map();
        app.add_map();
        app.switch_map(1);
        let showing = app.active_map_ref().id;
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let first = app.maps[0].id;
        app.close_map(first);
        assert_eq!(
            app.active_map_ref().id,
            showing,
            "closing another map changed the one showing"
        );
        assert_eq!(
            app.selected_node,
            Some(root),
            "closing another map dropped the selection"
        );
        let last = app.maps[1].id;
        app.close_map(showing);
        assert_eq!(app.active_map_ref().id, last);
        assert!(app.selected_node.is_none());
    }

    /// A file is read as what it holds, whatever it is called.
    #[test]
    fn a_file_is_opened_as_what_it_holds_not_what_it_is_called() {
        let dir = scratch("by-content");
        let outline = dir.join("looks-like-a-map.mindmap");
        std::fs::write(&outline, "Topic\n  - Branch\n").expect("write");
        let map = dir.join("looks-like-an-outline.outline");
        let mut app = MindMapApp::new();
        rich_map(&mut app);
        let id = app.active_map_ref().id;
        app.save_map_as(id, &map).expect("saved");

        let mut other = MindMapApp::new();
        let said = other.open_file(&outline);
        assert!(said.starts_with("Opened 2 node(s)"), "{said}");
        assert!(
            other.active_map_ref().document_path.is_none(),
            "an imported outline took the outline as its file -- a save would write a map over it"
        );
        assert!(!other.active_map_ref().dirty);
        let said = other.open_file(&map);
        assert_eq!(said, format!("Opened {}", map.display()));
        assert_eq!(other.active_map_ref().node_count(), 4);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A map file is read whole or not at all, and the refusal names what is
    /// wrong.
    #[test]
    fn a_map_file_that_cannot_be_read_whole_is_refused_and_says_why() {
        let dir = scratch("refused");
        let mut app = MindMapApp::new();
        rich_map(&mut app);
        let good = mindmap_document(app.active_map_ref()).to_text();
        let cases: [(&str, String, &str); 9] = [
            (
                "colour-number",
                good.replacen("colour-index: 0", "colour-index: 9", 1),
                "colour number is missing or not one",
            ),
            (
                "later",
                good.replacen("slateos-mindmap: 1", "slateos-mindmap: 2", 1),
                "later format (2)",
            ),
            (
                "orphan",
                good.replacen("parent: 1", "parent: 99", 1),
                "not written before it",
            ),
            (
                "two-roots",
                good.replacen("    parent: 1\n", "", 1),
                "second node with no parent",
            ),
            (
                "twins",
                good.replacen("id: 3", "id: 2", 1),
                "another node has its number",
            ),
            (
                "shape",
                good.replacen("shape: Rounded", "shape: Star", 1),
                "(Star) is not one",
            ),
            (
                "colour",
                good.replacen("colour: \"#", "colour: \"#Z", 1),
                "colour is missing or not one",
            ),
            (
                "width",
                good.replacen("width: 140", "width: 0", 1),
                "width is not above nothing",
            ),
            (
                "folded",
                good.replacen("collapsed: false", "collapsed: maybe", 1),
                "folded is missing",
            ),
        ];
        for (name, text, why) in cases {
            assert_ne!(text, good, "control: case {name} changed nothing");
            let path = dir.join(format!("{name}.mindmap"));
            std::fs::write(&path, &text).expect("write");
            let mut other = MindMapApp::new();
            let said = other.open_file(&path);
            assert!(said.starts_with("Could not open"), "{name}: {said}");
            assert!(said.contains(why), "{name}: {said}");
            assert_eq!(other.maps.len(), 1, "{name}: a map was opened anyway");
        }
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A map file larger than one open reads is refused, not read in part.
    #[test]
    fn a_map_file_larger_than_an_open_reads_is_refused() {
        let dir = scratch("large");
        let path = dir.join("big.mindmap");
        let mut app = MindMapApp::new();
        rich_map(&mut app);
        let id = app.active_map_ref().id;
        app.save_map_as(id, &path).expect("saved");
        let len = std::fs::metadata(&path).expect("saved").len();
        let mut other = MindMapApp::new();
        let said = other.open_file_within(&path, usize::try_from(len).expect("small") - 1);
        assert!(said.contains("larger than"), "{said}");
        assert_eq!(other.maps.len(), 1, "a map cut short was opened");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// An outline larger than one open reads is opened as far as its last
    /// whole line, and said to be incomplete.
    #[test]
    fn an_outline_cut_short_ends_at_a_whole_line_and_says_so() {
        let dir = scratch("cut-outline");
        let path = dir.join("long.outline");
        std::fs::write(&path, "Topic\n  - Alpha\n  - Bravo\n").expect("write");
        let mut app = MindMapApp::new();
        // Cut inside "Bravo".
        let said = app.open_file_within(&path, "Topic\n  - Alpha\n  - Br".len());
        assert!(said.starts_with("INCOMPLETE"), "{said}");
        let texts: Vec<&str> = app
            .active_map_ref()
            .nodes
            .values()
            .map(|n| n.text.as_str())
            .collect();
        assert!(
            !texts.contains(&"Br"),
            "a line cut in half became a node: {texts:?}"
        );
        assert_eq!(app.active_map_ref().node_count(), 2);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Ctrl+E writes the outline and leaves the map's own file and mark alone.
    #[test]
    fn exporting_an_outline_is_not_a_save() {
        let dir = scratch("export");
        let path = dir.join("words.outline");
        let mut app = MindMapApp::new();
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press_ctrl(Key::E));
        assert!(app.picker.is_open());
        assert_eq!(app.picker_for, PickerFor::Export);
        app.picker.close();
        app.picked(&path);
        let said = app.last_file_action.clone().unwrap_or_default();
        assert!(said.starts_with("Wrote 2 node(s)"), "{said}");
        assert!(said.contains("Ctrl+S saves those"), "{said}");
        assert!(
            app.active_map_ref().dirty,
            "an export cleared the unsaved mark"
        );
        assert!(
            app.active_map_ref().document_path.is_none(),
            "an export became the map's file"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// The status line says what the last open or save did. It was kept and
    /// never drawn.
    #[test]
    fn the_status_line_says_what_the_last_save_did() {
        let mut app = MindMapApp::new();
        app.last_file_action = Some(String::from("Could not save /x: no room"));
        assert!(drawn_text(&app).contains("Could not save /x: no room"));
    }

    /// The tabs answer clicks: a tab shows its map, its mark closes it, and
    /// "+" adds one.
    #[test]
    fn the_tabs_answer_clicks() {
        let mut app = MindMapApp::new();
        assert_eq!(click_on(&mut app, |a| a.plus_rect()), EventResult::Consumed);
        assert_eq!(app.maps.len(), 2);
        assert_eq!(app.active_map, 1);
        assert_eq!(click_on(&mut app, |a| a.tab_rect(0)), EventResult::Consumed);
        assert_eq!(app.active_map, 0);
        assert_eq!(
            click_on(&mut app, |a| a.tab_rect(0)),
            EventResult::Ignored,
            "a click on the tab showing is not a change"
        );
        // A changed map's mark asks first, about that map.
        app.handle_event(&press(Key::Tab));
        let id = app.active_map_ref().id;
        app.switch_map(1);
        click_on(&mut app, |a| a.tab_close_rect(0));
        assert_eq!(
            app.question.as_ref().map(Question::pending),
            Some(CloseScope::Map(id))
        );
        assert_eq!(app.active_map_ref().id, id, "asked without showing the map");
        app.handle_event(&press(Key::D));
        assert_eq!(app.maps.len(), 1);
        // A click on the tabs is never a click on the map.
        let selected = app.selected_node;
        let (x, _, w, _) = app.plus_rect();
        click(&mut app, (x + w + 200.0, TOOLBAR_HEIGHT + 10.0));
        assert_eq!(app.selected_node, selected);
        assert!(
            matches!(app.drag, DragState::None),
            "a click on the tab strip started a pan"
        );
    }

    /// Every toolbar button does what its key does. They were drawn and
    /// answered nothing.
    #[test]
    fn the_toolbar_buttons_answer_clicks() {
        let rects = toolbar_button_rects();
        let at = |action: ToolbarAction| {
            let i = TOOLBAR_BUTTONS
                .iter()
                .position(|(_, a)| *a == action)
                .expect("a button");
            centre(rects[i])
        };
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        assert_eq!(
            click(&mut app, at(ToolbarAction::AddChild)),
            EventResult::Consumed
        );
        assert_eq!(app.active_map_ref().node_count(), 2);
        assert_eq!(
            click(&mut app, at(ToolbarAction::AddSibling)),
            EventResult::Consumed
        );
        assert_eq!(app.active_map_ref().node_count(), 3);
        assert_eq!(
            click(&mut app, at(ToolbarAction::Delete)),
            EventResult::Consumed
        );
        assert_eq!(app.active_map_ref().node_count(), 2);
        assert_eq!(
            click(&mut app, at(ToolbarAction::Undo)),
            EventResult::Consumed
        );
        assert_eq!(app.active_map_ref().node_count(), 3);
        assert_eq!(
            click(&mut app, at(ToolbarAction::Redo)),
            EventResult::Consumed
        );
        assert_eq!(app.active_map_ref().node_count(), 2);
        let zoom = app.zoom;
        click(&mut app, at(ToolbarAction::ZoomIn));
        assert!(app.zoom > zoom);
        click(&mut app, at(ToolbarAction::ZoomOut));
        assert!((app.zoom - zoom).abs() < 1e-4);
        let root = app.active_map_ref().root_id;
        app.active_map_mut().move_node(root, 1.0, 1.0);
        click(&mut app, at(ToolbarAction::Layout));
        assert_ne!(app.active_map_ref().node(root).map(|n| n.x), Some(1.0));
        assert_eq!(
            click(&mut app, at(ToolbarAction::Save)),
            EventResult::Consumed
        );
        assert_eq!(app.picker_for, PickerFor::Save(app.active_map_ref().id));
        app.picker.close();
        assert_eq!(
            click(&mut app, at(ToolbarAction::Open)),
            EventResult::Consumed
        );
        assert!(app.picker.is_open());
        assert_eq!(app.picker_for, PickerFor::Open);
        // No two buttons share a pixel.
        for pair in rects.windows(2) {
            assert!(
                pair[0].0 + pair[0].2 <= pair[1].0,
                "buttons overlap: {pair:?}"
            );
        }
    }

    /// Ctrl+Tab goes round the maps, Ctrl+Shift+Tab back.
    #[test]
    fn ctrl_tab_goes_round_the_maps() {
        let mut app = MindMapApp::new();
        assert_eq!(
            app.handle_event(&press_ctrl(Key::Tab)),
            EventResult::Ignored
        );
        assert_eq!(
            app.active_map_ref().node_count(),
            1,
            "Ctrl+Tab added a child"
        );
        app.add_map();
        app.add_map();
        assert_eq!(app.active_map, 2);
        app.handle_event(&press_ctrl(Key::Tab));
        assert_eq!(app.active_map, 0);
        app.handle_event(&press_ctrl_shift(Key::Tab));
        assert_eq!(app.active_map, 2);
        app.handle_event(&press_ctrl_shift(Key::Tab));
        assert_eq!(app.active_map, 1);
        app.handle_event(&press_ctrl(Key::N));
        assert_eq!(app.maps.len(), 4);
        assert_eq!(app.active_map, 3);
    }

    /// A press on the sidebar or the status line is not one on empty canvas.
    #[test]
    fn a_press_beside_the_canvas_keeps_the_selection() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        assert_eq!(click(&mut app, (20.0, 300.0)), EventResult::Ignored);
        let status_line = app.win_height - 5.0;
        assert_eq!(click(&mut app, (600.0, status_line)), EventResult::Ignored);
        assert_eq!(app.selected_node, Some(root));
        assert!(matches!(app.drag, DragState::None));
    }
}
