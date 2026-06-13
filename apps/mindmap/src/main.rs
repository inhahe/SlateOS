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
//! - Full undo/redo stack
//! - Text export (indented outline)
//! - Search with highlighting
//! - Keyboard shortcuts for all major actions
//! - Catppuccin Mocha theme
//!
//! Uses the guitk library for UI rendering.

#![allow(dead_code)]
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

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::{HashMap, VecDeque};

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const OVERLAY0: Color = Color::from_hex(0x6C7086);

/// Preset node colors the user can cycle through.
const NODE_COLORS: [Color; 8] = [BLUE, GREEN, RED, YELLOW, PEACH, TEAL, MAUVE, SURFACE0];

// ============================================================================
// Layout constants
// ============================================================================

/// Height of the top toolbar.
const TOOLBAR_HEIGHT: f32 = 40.0;
/// Height of the bottom status bar.
const STATUS_BAR_HEIGHT: f32 = 24.0;
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
}

impl MindMap {
    /// Create a new mind map with a single root node.
    pub fn new(name: String, id_gen: &mut IdGenerator) -> Self {
        let root_id = id_gen.next_id();
        let root = MindMapNode::new(root_id, "Central Idea".to_string(), None, BLUE, 0);
        let mut nodes = HashMap::new();
        nodes.insert(root_id, root);
        Self {
            name,
            nodes,
            root_id,
            id_gen: id_gen.clone(),
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
        if nodes.is_empty() {
            return;
        }
        let subtree_root_id = nodes[0].id;

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
                output.push_str(&node.text);
            } else {
                output.push_str("- ");
                output.push_str(&node.text);
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
        let subtree_h = heights[i];
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

    /// Undo stack.
    pub undo_stack: VecDeque<Action>,
    /// Redo stack.
    pub redo_stack: Vec<Action>,

    /// Search query.
    pub search_query: String,
    /// Node IDs matching the current search.
    pub search_results: Vec<NodeId>,
    /// Index into search_results for cycling.
    pub search_index: usize,

    /// Whether the sidebar is visible.
    pub show_sidebar: bool,
    /// Whether the search bar is visible.
    pub show_search: bool,

    /// Text input buffer for editing node text.
    pub edit_buffer: String,
    /// Whether we are in text editing mode.
    pub editing_node: Option<NodeId>,
}

impl Default for MindMapApp {
    fn default() -> Self {
        Self::new()
    }
}

impl MindMapApp {
    /// Create a new mind map application with default state.
    pub fn new() -> Self {
        let mut id_gen = IdGenerator::new();
        let map = MindMap::new("Mind Map 1".to_string(), &mut id_gen);
        let mut app = Self {
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
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            search_query: String::new(),
            search_results: Vec::new(),
            search_index: 0,
            show_sidebar: true,
            show_search: false,
            edit_buffer: String::new(),
            editing_node: None,
        };
        // Auto-layout the initial map
        let cx = app.win_width / 2.0;
        let cy = app.win_height / 2.0;
        auto_layout(&mut app.maps[0], cx, cy);
        app
    }

    // ========================================================================
    // Map accessors
    // ========================================================================

    /// Get the active mind map.
    pub fn active_map_ref(&self) -> &MindMap {
        &self.maps[self.active_map]
    }

    /// Get the active mind map mutably.
    pub fn active_map_mut(&mut self) -> &mut MindMap {
        &mut self.maps[self.active_map]
    }

    // ========================================================================
    // Map management
    // ========================================================================

    /// Add a new empty mind map.
    pub fn add_map(&mut self) {
        let name = format!("Mind Map {}", self.maps.len().saturating_add(1));
        let map = MindMap::new(name, &mut self.id_gen);
        self.maps.push(map);
        self.active_map = self.maps.len().saturating_sub(1);
        self.selected_node = None;
        let cx = self.win_width / 2.0;
        let cy = self.win_height / 2.0;
        auto_layout(self.active_map_mut(), cx, cy);
    }

    /// Switch to a different map by index.
    pub fn switch_map(&mut self, index: usize) {
        if index < self.maps.len() {
            self.active_map = index;
            self.selected_node = None;
            self.editing_node = None;
        }
    }

    /// Delete the active map (unless it's the last one).
    pub fn delete_active_map(&mut self) -> bool {
        if self.maps.len() <= 1 {
            return false;
        }
        self.maps.remove(self.active_map);
        if self.active_map >= self.maps.len() {
            self.active_map = self.maps.len().saturating_sub(1);
        }
        self.selected_node = None;
        self.editing_node = None;
        true
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

    /// Y origin of the canvas area.
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
        let color_index = (self.active_map_ref().depth(parent_id).saturating_add(1) as u8)
            % (NODE_COLORS.len() as u8);
        let color = NODE_COLORS[color_index as usize];

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
        let color = NODE_COLORS[color_index as usize];

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
            let new_index = (old_index.wrapping_add(1)) % (NODE_COLORS.len() as u8);
            let new_color = NODE_COLORS[new_index as usize];

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

    fn push_undo(&mut self, action: Action) {
        self.redo_stack.clear();
        if self.undo_stack.len() >= MAX_UNDO {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(action);
    }

    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop_back() {
            self.apply_reverse(&action);
            self.redo_stack.push(action);
        }
    }

    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop() {
            self.apply_forward(&action);
            self.undo_stack.push_back(action);
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
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
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
        self.search_index = (self.search_index.wrapping_add(1)) % self.search_results.len();
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
            && pos + 1 < parent.children.len()
        {
            self.selected_node = Some(parent.children[pos + 1]);
        }
    }

    /// Select the previous sibling.
    pub fn select_prev_sibling(&mut self) {
        if let Some(sel) = self.selected_node
            && let Some(node) = self.active_map_ref().node(sel)
            && let Some(pid) = node.parent
            && let Some(parent) = self.active_map_ref().node(pid)
            && let Some(pos) = parent.children.iter().position(|&c| c == sel)
            && pos > 0
        {
            self.selected_node = Some(parent.children[pos - 1]);
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
    // Rendering
    // ========================================================================

    /// Render the entire UI to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Full window background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.win_width,
            height: self.win_height,
            color: BASE,
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

        cmds
    }

    // ------ Toolbar ------

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.win_width,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Mind Map".to_string(),
            font_size: 15.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Toolbar buttons
        let buttons = [
            ("Add Child", 120.0),
            ("Add Sibling", 220.0),
            ("Delete", 330.0),
            ("Undo", 410.0),
            ("Redo", 475.0),
            ("Layout", 540.0),
            ("Zoom+", 615.0),
            ("Zoom-", 685.0),
        ];

        for (label, bx) in &buttons {
            cmds.push(RenderCommand::FillRect {
                x: *bx,
                y: 6.0,
                width: 70.0,
                height: 28.0,
                color: SURFACE0,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });
            cmds.push(RenderCommand::Text {
                x: *bx + 6.0,
                y: 14.0,
                text: label.to_string(),
                font_size: 11.0,
                color: TEXT,
                font_weight: FontWeightHint::Regular,
                max_width: Some(58.0),
            });
        }

        // Zoom indicator
        let zoom_pct = format!("{}%", (self.zoom * 100.0) as u32);
        cmds.push(RenderCommand::Text {
            x: 770.0,
            y: 14.0,
            text: zoom_pct,
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // ------ Map tabs ------

    fn render_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOOLBAR_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.win_width,
            height: TAB_HEIGHT,
            color: SURFACE0,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tx = 10.0;
        for (i, map) in self.maps.iter().enumerate() {
            let is_active = i == self.active_map;
            let tab_color = if is_active { BASE } else { SURFACE0 };
            let text_color = if is_active { TEXT } else { OVERLAY0 };
            let tab_w = 120.0f32;

            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 2.0,
                width: tab_w,
                height: TAB_HEIGHT - 2.0,
                color: tab_color,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });

            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 9.0,
                text: map.name.clone(),
                font_size: 11.0,
                color: text_color,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_w - 16.0),
            });

            tx += tab_w + 4.0;
        }

        // "+" button to add a new tab
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: y + 4.0,
            width: 24.0,
            height: 20.0,
            color: SURFACE1,
            corner_radii: CornerRadii::all(PANEL_CORNER),
        });
        cmds.push(RenderCommand::Text {
            x: tx + 7.0,
            y: y + 8.0,
            text: "+".to_string(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
    }

    // ------ Canvas background ------

    fn render_canvas_background(&self, cmds: &mut Vec<RenderCommand>) {
        cmds.push(RenderCommand::FillRect {
            x: self.canvas_x(),
            y: self.canvas_y(),
            width: self.canvas_width(),
            height: self.canvas_height(),
            color: BASE,
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

        let dot_color = Color::rgba(OVERLAY0.r, OVERLAY0.g, OVERLAY0.b, 40);
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
            let mid_x = (spx + scx) / 2.0;

            let is_search_match = self.search_results.contains(&node_id);
            let line_color = if is_search_match { YELLOW } else { node.color };
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
                color: if is_search_match { YELLOW } else { TEXT },
                line_width: 2.0,
                corner_radii: self.corner_radii_for_shape(node.shape, 11.0),
            });
        }

        // Node fill
        let fill_color = if is_editing { SURFACE1 } else { node.color };

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
            YELLOW
        } else if is_selected {
            TEXT
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
        });

        // Collapse indicator for nodes with children
        if !node.children.is_empty() {
            let indicator_x = sx + sw - COLLAPSE_SIZE - 2.0;
            let indicator_y = sy + sh / 2.0 - COLLAPSE_SIZE / 2.0;
            let indicator_text = if node.collapsed { "+" } else { "-" };

            cmds.push(RenderCommand::FillRect {
                x: indicator_x,
                y: indicator_y,
                width: COLLAPSE_SIZE,
                height: COLLAPSE_SIZE,
                color: SURFACE0,
                corner_radii: CornerRadii::all(2.0),
            });
            cmds.push(RenderCommand::Text {
                x: indicator_x + 2.0,
                y: indicator_y + 1.0,
                text: indicator_text.to_string(),
                font_size: 10.0,
                color: TEXT,
                font_weight: FontWeightHint::Bold,
                max_width: None,
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
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: y,
            x2: SIDEBAR_WIDTH,
            y2: y + h,
            color: SURFACE0,
            width: 1.0,
        });

        // Section: Node Properties
        let mut sy = y + 10.0;
        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: sy,
            text: "Node Properties".to_string(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        sy += 22.0;

        if let Some(sel_id) = self.selected_node {
            if let Some(node) = self.active_map_ref().node(sel_id) {
                // Text
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("Text: {}", truncate_str(&node.text, 18)),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(SIDEBAR_WIDTH - 20.0),
                });
                sy += 18.0;

                // Shape
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("Shape: {}", node.shape.label()),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                sy += 18.0;

                // Color swatch
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: "Color:".to_string(),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
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
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                sy += 18.0;

                // Depth
                let depth = self.active_map_ref().depth(sel_id);
                cmds.push(RenderCommand::Text {
                    x: x + 10.0,
                    y: sy,
                    text: format!("Depth: {depth}"),
                    font_size: 11.0,
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
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
                    color: SUBTEXT0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
                sy += 26.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: x + 10.0,
                y: sy,
                text: "No node selected".to_string(),
                font_size: 11.0,
                color: OVERLAY0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
            sy += 26.0;
        }

        // Section: Keyboard Shortcuts
        cmds.push(RenderCommand::Line {
            x1: x + 10.0,
            y1: sy,
            x2: x + SIDEBAR_WIDTH - 10.0,
            y2: sy,
            color: SURFACE0,
            width: 1.0,
        });
        sy += 10.0;

        cmds.push(RenderCommand::Text {
            x: x + 10.0,
            y: sy,
            text: "Shortcuts".to_string(),
            font_size: 13.0,
            color: TEXT,
            font_weight: FontWeightHint::Bold,
            max_width: None,
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
                color: BLUE,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });
            cmds.push(RenderCommand::Text {
                x: x + 70.0,
                y: sy,
                text: action.to_string(),
                font_size: 10.0,
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(SIDEBAR_WIDTH - 80.0),
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
            color: SURFACE0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Border
        cmds.push(RenderCommand::StrokeRect {
            x: bx,
            y: by,
            width: bar_width,
            height: bar_height,
            color: BLUE,
            line_width: 1.0,
            corner_radii: CornerRadii::all(6.0),
        });

        // Search icon text
        cmds.push(RenderCommand::Text {
            x: bx + 10.0,
            y: by + 10.0,
            text: "Search:".to_string(),
            font_size: 12.0,
            color: OVERLAY0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Query text
        let display = if self.search_query.is_empty() {
            "type to search...".to_string()
        } else {
            self.search_query.clone()
        };
        let query_color = if self.search_query.is_empty() {
            OVERLAY0
        } else {
            TEXT
        };

        cmds.push(RenderCommand::Text {
            x: bx + 70.0,
            y: by + 10.0,
            text: display,
            font_size: 12.0,
            color: query_color,
            font_weight: FontWeightHint::Regular,
            max_width: Some(bar_width - 110.0),
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
                color: SUBTEXT0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    // ------ Status bar ------

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.win_height - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.win_width,
            height: STATUS_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let map = self.active_map_ref();
        let status = format!(
            "Nodes: {} | Zoom: {}% | Pan: ({:.0}, {:.0}) | Map: {}",
            map.node_count(),
            (self.zoom * 100.0) as u32,
            self.pan_x,
            self.pan_y,
            map.name,
        );

        cmds.push(RenderCommand::Text {
            x: 10.0,
            y: y + 5.0,
            text: status,
            font_size: 11.0,
            color: SUBTEXT0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.win_width - 20.0),
        });

        // Selected node info on the right
        if let Some(sel_id) = self.selected_node
            && let Some(node) = map.node(sel_id)
        {
            let sel_info = format!(
                "Selected: \"{}\" (ID: {})",
                truncate_str(&node.text, 20),
                sel_id
            );
            cmds.push(RenderCommand::Text {
                x: self.win_width - 300.0,
                y: y + 5.0,
                text: sel_info,
                font_size: 11.0,
                color: BLUE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(290.0),
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

/// Truncate a string to a maximum number of characters, appending "..." if truncated.
fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let mut truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        truncated.push_str("...");
        truncated
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let _app = MindMapApp::new();
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

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
        let node = MindMapNode::new(1, "Root".to_string(), None, BLUE, 0);
        assert_eq!(node.id, 1);
        assert!(node.parent.is_none());
        assert_eq!(node.shape, NodeShape::Ellipse);
        assert_eq!(node.width, ROOT_NODE_W);
        assert_eq!(node.height, ROOT_NODE_H);
        assert!(!node.collapsed);
    }

    #[test]
    fn test_node_new_child() {
        let node = MindMapNode::new(2, "Child".to_string(), Some(1), GREEN, 1);
        assert_eq!(node.parent, Some(1));
        assert_eq!(node.shape, NodeShape::RoundedRect);
        assert_eq!(node.width, DEFAULT_NODE_W);
    }

    #[test]
    fn test_node_contains() {
        let mut node = MindMapNode::new(1, "Test".to_string(), None, BLUE, 0);
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
        let mut node = MindMapNode::new(1, "Test".to_string(), None, BLUE, 0);
        node.x = 50.0;
        node.y = 75.0;
        let (cx, cy) = node.center();
        assert!((cx - 50.0).abs() < 0.01);
        assert!((cy - 75.0).abs() < 0.01);
    }

    #[test]
    fn test_node_right_center() {
        let mut node = MindMapNode::new(1, "Test".to_string(), None, BLUE, 0);
        node.x = 100.0;
        node.y = 100.0;
        node.width = 140.0;
        let (rx, ry) = node.right_center();
        assert!((rx - 170.0).abs() < 0.01);
        assert!((ry - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_node_left_center() {
        let mut node = MindMapNode::new(1, "Test".to_string(), None, BLUE, 0);
        node.x = 100.0;
        node.y = 100.0;
        node.width = 140.0;
        let (lx, ly) = node.left_center();
        assert!((lx - 30.0).abs() < 0.01);
        assert!((ly - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_node_bounds() {
        let mut node = MindMapNode::new(1, "Test".to_string(), None, BLUE, 0);
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let child = map.add_child(root, "Child 1".to_string(), GREEN, 1);
        assert!(child.is_some());
        assert_eq!(map.node_count(), 2);
        let cid = child.unwrap();
        assert_eq!(map.node(cid).unwrap().parent, Some(root));
        assert_eq!(map.node(root).unwrap().children.len(), 1);
    }

    #[test]
    fn test_mind_map_add_child_invalid_parent() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let result = map.add_child(999, "Orphan".to_string(), GREEN, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_mind_map_add_sibling() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let c2 = map.add_sibling(c1, "C2".to_string(), GREEN, 1);
        assert!(c2.is_some());
        assert_eq!(map.node_count(), 3);
        let parent_children = &map.node(root).unwrap().children;
        assert_eq!(parent_children.len(), 2);
        assert_eq!(parent_children[0], c1);
        assert_eq!(parent_children[1], c2.unwrap());
    }

    #[test]
    fn test_mind_map_add_sibling_to_root_fails() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let result = map.add_sibling(map.root_id, "Sibling".to_string(), GREEN, 1);
        assert!(result.is_none());
    }

    #[test]
    fn test_mind_map_subtree_ids() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let c2 = map.add_child(root, "C2".to_string(), GREEN, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();
        let ids = map.subtree_ids(root);
        assert_eq!(ids.len(), 4);
        assert!(ids.contains(&root));
        assert!(ids.contains(&c1));
        assert!(ids.contains(&c2));
        assert!(ids.contains(&gc1));
    }

    #[test]
    fn test_mind_map_subtree_ids_leaf() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let ids = map.subtree_ids(c1);
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], c1);
    }

    #[test]
    fn test_mind_map_delete_subtree() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let _gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();

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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let result = map.change_color(root, RED, 2);
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let _gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();
        let visible = map.visible_node_ids();
        assert_eq!(visible.len(), 3);
    }

    #[test]
    fn test_mind_map_visible_nodes_collapsed() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let _gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();
        map.toggle_collapse(c1);
        let visible = map.visible_node_ids();
        assert_eq!(visible.len(), 2); // root + c1 (gc1 hidden)
    }

    #[test]
    fn test_mind_map_search_found() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "Alpha".to_string(), GREEN, 1);
        map.add_child(root, "Beta".to_string(), RED, 2);
        let results = map.search("alpha");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_mind_map_search_case_insensitive() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "MyNode".to_string(), GREEN, 1);
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "Child A".to_string(), GREEN, 1);
        map.add_child(root, "Child B".to_string(), RED, 2);
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        assert_eq!(map.depth(c1), 1);
    }

    #[test]
    fn test_mind_map_depth_grandchild() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();
        assert_eq!(map.depth(gc1), 2);
    }

    #[test]
    fn test_mind_map_descendant_count() {
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        map.add_child(c1, "GC1".to_string(), RED, 2);
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let c2 = map.add_child(root, "C2".to_string(), RED, 2).unwrap();
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        let c1 = map.add_child(root, "C1".to_string(), GREEN, 1).unwrap();
        let gc1 = map.add_child(c1, "GC1".to_string(), RED, 2).unwrap();

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
    fn test_app_delete_map() {
        let mut app = MindMapApp::new();
        app.add_map();
        assert_eq!(app.maps.len(), 2);
        assert!(app.delete_active_map());
        assert_eq!(app.maps.len(), 1);
    }

    #[test]
    fn test_app_delete_last_map_fails() {
        let mut app = MindMapApp::new();
        assert!(!app.delete_active_map());
        assert_eq!(app.maps.len(), 1);
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
        assert!(app.undo_stack.len() <= MAX_UNDO);
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
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = MindMapApp::new();
        let root = app.active_map_ref().root_id;
        app.selected_node = Some(root);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_search() {
        let mut app = MindMapApp::new();
        app.show_search = true;
        app.search_query = "test".to_string();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_sidebar_hidden() {
        let mut app = MindMapApp::new();
        app.show_sidebar = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_children() {
        let mut app = MindMapApp::new();
        app.add_child_to_selected("C1".to_string());
        app.add_child_to_selected("C2".to_string());
        let cmds = app.render();
        assert!(cmds.len() > 10); // should have many render commands
    }

    #[test]
    fn test_render_collapsed_hides_children() {
        let mut app = MindMapApp::new();
        let c1 = app.add_child_to_selected("C1".to_string()).unwrap();
        app.selected_node = Some(c1);
        app.add_child_to_selected("GC1".to_string());
        let cmds_expanded = app.render();

        app.selected_node = Some(c1);
        app.toggle_collapse_selected();
        let cmds_collapsed = app.render();

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

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("Hello", 10), "Hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        assert_eq!(truncate_str("Hello", 5), "Hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let result = truncate_str("Hello World", 8);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 8);
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
        let mut id_gen = IdGenerator::new();
        let mut map = MindMap::new("Test".to_string(), &mut id_gen);
        let root = map.root_id;
        map.add_child(root, "C1".to_string(), GREEN, 1);
        map.add_child(root, "C2".to_string(), RED, 2);
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
}
