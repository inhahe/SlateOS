//! `Slate OS` Diagram Editor
//!
//! A full-featured diagram and flowchart editor with:
//! - Node types: rectangle, rounded rectangle, diamond (decision), circle,
//!   ellipse, parallelogram, hexagon, triangle, cylinder (database), cloud
//! - Connection/edge types: straight line, curved bezier, orthogonal (right-angle),
//!   arrow with/without head
//! - Node properties: label text, fill color, border color, border width, font size
//! - Edge properties: label, color, line style (solid/dashed/dotted), arrow head style
//! - Snap-to-grid with configurable grid size
//! - Alignment tools: align left/right/center/top/bottom/middle, distribute evenly
//! - Grouping: select multiple nodes, group/ungroup, move group as unit
//! - Layers with visibility toggle and reordering
//! - Zoom/pan from 25% to 400%
//! - Templates: blank, flowchart, org chart, UML class diagram, network diagram,
//!   mind map, ER diagram
//! - Export to SVG text and JSON serialization
//! - Undo/redo stack
//! - Copy/paste/duplicate
//! - Multi-select with selection rectangle
//! - Canvas with infinite scroll
//! - Auto-layout suggestions (basic top-down or left-right)
//! - Multi-panel UI: shape palette sidebar, canvas area, properties panel
//!
//! Uses the guitk library for UI rendering.

// Lint policy is inherited from the workspace (`[lints] workspace = true`):
// `clippy::all` denied, `clippy::pedantic` at warn, with the curated allow
// list documented in the root Cargo.toml (keeps the discipline centralised).
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

use guitk::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

const BASE: Color = Color::from_hex(0x1E1E2E);
const MANTLE: Color = Color::from_hex(0x181825);
const CRUST: Color = Color::from_hex(0x11111B);
const SURFACE0: Color = Color::from_hex(0x313244);
const SURFACE1: Color = Color::from_hex(0x45475A);
const SURFACE2: Color = Color::from_hex(0x585B70);
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const BLUE: Color = Color::from_hex(0x89B4FA);
const GREEN: Color = Color::from_hex(0xA6E3A1);
const RED: Color = Color::from_hex(0xF38BA8);
const YELLOW: Color = Color::from_hex(0xF9E2AF);
const PEACH: Color = Color::from_hex(0xFAB387);
const LAVENDER: Color = Color::from_hex(0xB4BEFE);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const MAUVE: Color = Color::from_hex(0xCBA6F7);
const SKY: Color = Color::from_hex(0x89DCEB);
const PINK: Color = Color::from_hex(0xF5C2E7);

// ============================================================================
// Layout constants
// ============================================================================

/// Width of the shape palette sidebar on the left.
const PALETTE_WIDTH: f32 = 200.0;
/// Width of the properties panel on the right.
const PROPERTIES_WIDTH: f32 = 240.0;
/// Height of the top toolbar.
const TOOLBAR_HEIGHT: f32 = 40.0;
/// Height of the bottom status bar.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Default grid size in pixels.
const DEFAULT_GRID_SIZE: f32 = 20.0;
/// Minimum zoom level (25%).
const MIN_ZOOM: f32 = 0.25;
/// Maximum zoom level (400%).
const MAX_ZOOM: f32 = 4.0;
/// Default zoom level (100%).
const DEFAULT_ZOOM: f32 = 1.0;
/// Maximum undo/redo steps.
const MAX_UNDO: usize = 100;
/// Default node width.
const DEFAULT_NODE_W: f32 = 140.0;
/// Default node height.
const DEFAULT_NODE_H: f32 = 60.0;
/// Corner radius for UI panels.
const PANEL_CORNER: f32 = 4.0;
/// Height of each shape button in the palette.
const SHAPE_BTN_H: f32 = 32.0;
/// Height of each layer row.
const LAYER_ROW_H: f32 = 28.0;

// ============================================================================
// Unique ID generation
// ============================================================================

/// Unique identifier for nodes and edges.
pub type NodeId = u64;
/// Unique identifier for edges.
pub type EdgeId = u64;
/// Group identifier.
pub type GroupId = u64;
/// Layer identifier.
pub type LayerId = u64;

/// Monotonically increasing ID generator.
#[derive(Debug)]
pub struct IdGen {
    next: u64,
}

impl IdGen {
    const fn new(start: u64) -> Self {
        Self { next: start }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next;
        self.next = self.next.saturating_add(1);
        id
    }
}

// ============================================================================
// Node shape types
// ============================================================================

/// Shape types available for diagram nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeShape {
    /// Standard rectangle.
    Rectangle,
    /// Rectangle with rounded corners.
    RoundedRectangle,
    /// Diamond shape for decisions.
    Diamond,
    /// Circle (width == height).
    Circle,
    /// Ellipse (width != height allowed).
    Ellipse,
    /// Parallelogram for I/O operations.
    Parallelogram,
    /// Regular hexagon.
    Hexagon,
    /// Triangle (pointing up).
    Triangle,
    /// Cylinder for databases.
    Cylinder,
    /// Cloud shape.
    Cloud,
}

impl NodeShape {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::RoundedRectangle => "Rounded Rect",
            Self::Diamond => "Diamond",
            Self::Circle => "Circle",
            Self::Ellipse => "Ellipse",
            Self::Parallelogram => "Parallelogram",
            Self::Hexagon => "Hexagon",
            Self::Triangle => "Triangle",
            Self::Cylinder => "Cylinder",
            Self::Cloud => "Cloud",
        }
    }

    /// All shapes in display order.
    pub fn all() -> &'static [Self] {
        &[
            Self::Rectangle,
            Self::RoundedRectangle,
            Self::Diamond,
            Self::Circle,
            Self::Ellipse,
            Self::Parallelogram,
            Self::Hexagon,
            Self::Triangle,
            Self::Cylinder,
            Self::Cloud,
        ]
    }

    /// Accent color for the shape button in the palette.
    pub fn accent_color(self) -> Color {
        match self {
            Self::Rectangle => BLUE,
            Self::RoundedRectangle => TEAL,
            Self::Diamond => YELLOW,
            Self::Circle => GREEN,
            Self::Ellipse => LAVENDER,
            Self::Parallelogram => PEACH,
            Self::Hexagon => MAUVE,
            Self::Triangle => RED,
            Self::Cylinder => SKY,
            Self::Cloud => PINK,
        }
    }
}

// ============================================================================
// Edge / connection types
// ============================================================================

/// How the edge line is routed between nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeKind {
    /// Straight line connecting two endpoints.
    Straight,
    /// Cubic bezier curve.
    Bezier,
    /// Orthogonal (right-angle) routing.
    Orthogonal,
}

impl EdgeKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Straight => "Straight",
            Self::Bezier => "Bezier",
            Self::Orthogonal => "Orthogonal",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Straight, Self::Bezier, Self::Orthogonal]
    }
}

/// Arrow head style on an edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowHead {
    /// No arrow head.
    None,
    /// Filled triangle arrow.
    Filled,
    /// Open (outline only) triangle arrow.
    Open,
    /// Diamond head.
    Diamond,
}

impl ArrowHead {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Filled => "Filled",
            Self::Open => "Open",
            Self::Diamond => "Diamond",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::None, Self::Filled, Self::Open, Self::Diamond]
    }
}

/// Line dash style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    /// Solid line.
    Solid,
    /// Dashed line.
    Dashed,
    /// Dotted line.
    Dotted,
}

impl LineStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
        }
    }

    pub fn all() -> &'static [Self] {
        &[Self::Solid, Self::Dashed, Self::Dotted]
    }
}

// ============================================================================
// Diagram node
// ============================================================================

/// A single diagram node on the canvas.
#[derive(Clone, Debug)]
pub struct DiagramNode {
    pub id: NodeId,
    pub shape: NodeShape,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub label: String,
    pub fill_color: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub font_size: f32,
    pub layer_id: LayerId,
    pub group_id: Option<GroupId>,
}

impl DiagramNode {
    /// Create a new node with default styling at the given position.
    pub fn new(id: NodeId, shape: NodeShape, x: f32, y: f32, layer_id: LayerId) -> Self {
        let (w, h) = match shape {
            NodeShape::Circle => (80.0, 80.0),
            NodeShape::Cylinder => (100.0, 80.0),
            _ => (DEFAULT_NODE_W, DEFAULT_NODE_H),
        };
        Self {
            id,
            shape,
            x,
            y,
            width: w,
            height: h,
            label: String::new(),
            fill_color: SURFACE0,
            border_color: BLUE,
            border_width: 2.0,
            font_size: 14.0,
            layer_id,
            group_id: None,
        }
    }

    /// Center point of the node.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    /// Tests whether a point (px, py) is inside this node's bounding box.
    pub fn hit_test(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }

    /// Connection point on the boundary closest to an external point.
    pub fn connection_point(&self, target_x: f32, target_y: f32) -> (f32, f32) {
        let (cx, cy) = self.center();
        let dx = target_x - cx;
        let dy = target_y - cy;
        let hw = self.width / 2.0;
        let hh = self.height / 2.0;

        if dx.abs() < 0.001 && dy.abs() < 0.001 {
            return (cx, cy - hh); // default: top
        }

        // Find edge intersection via scaling
        let scale_x = if dx.abs() > 0.001 {
            hw / dx.abs()
        } else {
            f32::MAX
        };
        let scale_y = if dy.abs() > 0.001 {
            hh / dy.abs()
        } else {
            f32::MAX
        };
        let scale = scale_x.min(scale_y);

        (cx + dx * scale, cy + dy * scale)
    }
}

// ============================================================================
// Diagram edge
// ============================================================================

/// A connection between two nodes.
#[derive(Clone, Debug)]
pub struct DiagramEdge {
    pub id: EdgeId,
    pub from_node: NodeId,
    pub to_node: NodeId,
    pub kind: EdgeKind,
    pub label: String,
    pub color: Color,
    pub line_style: LineStyle,
    pub line_width: f32,
    pub start_arrow: ArrowHead,
    pub end_arrow: ArrowHead,
    pub layer_id: LayerId,
}

impl DiagramEdge {
    /// Create a new edge between two nodes with default styling.
    pub fn new(id: EdgeId, from: NodeId, to: NodeId, layer_id: LayerId) -> Self {
        Self {
            id,
            from_node: from,
            to_node: to,
            kind: EdgeKind::Straight,
            label: String::new(),
            color: TEXT,
            line_style: LineStyle::Solid,
            line_width: 2.0,
            start_arrow: ArrowHead::None,
            end_arrow: ArrowHead::Filled,
            layer_id,
        }
    }
}

// ============================================================================
// Layers
// ============================================================================

/// A diagram layer for organizing elements.
#[derive(Clone, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub order: usize,
}

impl Layer {
    pub fn new(id: LayerId, name: String, order: usize) -> Self {
        Self {
            id,
            name,
            visible: true,
            order,
        }
    }
}

// ============================================================================
// Groups
// ============================================================================

/// A named group of nodes that move together.
#[derive(Clone, Debug)]
pub struct Group {
    pub id: GroupId,
    pub name: String,
    pub member_ids: Vec<NodeId>,
}

impl Group {
    pub fn new(id: GroupId, members: Vec<NodeId>) -> Self {
        Self {
            id,
            name: format!("Group {id}"),
            member_ids: members,
        }
    }
}

// ============================================================================
// Diagram template
// ============================================================================

/// Predefined diagram templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagramTemplate {
    /// Empty canvas.
    Blank,
    /// Flowchart with start/end, process, decision nodes.
    Flowchart,
    /// Organizational chart.
    OrgChart,
    /// UML class diagram.
    UmlClass,
    /// Network topology diagram.
    NetworkDiagram,
    /// Mind map with central topic and branches.
    MindMap,
    /// Entity-relationship diagram.
    ErDiagram,
}

impl DiagramTemplate {
    pub fn label(self) -> &'static str {
        match self {
            Self::Blank => "Blank",
            Self::Flowchart => "Flowchart",
            Self::OrgChart => "Org Chart",
            Self::UmlClass => "UML Class",
            Self::NetworkDiagram => "Network",
            Self::MindMap => "Mind Map",
            Self::ErDiagram => "ER Diagram",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Blank,
            Self::Flowchart,
            Self::OrgChart,
            Self::UmlClass,
            Self::NetworkDiagram,
            Self::MindMap,
            Self::ErDiagram,
        ]
    }
}

// ============================================================================
// Auto-layout direction
// ============================================================================

/// Direction for automatic layout arrangement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutDirection {
    /// Top to bottom.
    TopDown,
    /// Left to right.
    LeftRight,
}

impl LayoutDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::TopDown => "Top-Down",
            Self::LeftRight => "Left-Right",
        }
    }
}

// ============================================================================
// Alignment operations
// ============================================================================

/// Alignment operations for selected nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignOp {
    Left,
    Right,
    CenterH,
    Top,
    Bottom,
    CenterV,
    DistributeH,
    DistributeV,
}

impl AlignOp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Left => "Align Left",
            Self::Right => "Align Right",
            Self::CenterH => "Center H",
            Self::Top => "Align Top",
            Self::Bottom => "Align Bottom",
            Self::CenterV => "Center V",
            Self::DistributeH => "Distribute H",
            Self::DistributeV => "Distribute V",
        }
    }
}

// ============================================================================
// Interaction mode
// ============================================================================

/// Current interaction mode / active tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionMode {
    /// Default: select and move nodes.
    Select,
    /// Drawing a selection rectangle.
    RectSelect,
    /// Creating a new node of a specific shape.
    AddNode(NodeShape),
    /// Creating a new edge (pick source, then target).
    AddEdge,
    /// Panning the canvas.
    Pan,
}

// ============================================================================
// Selection state
// ============================================================================

/// What the user currently has selected.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    /// Selected node IDs.
    pub nodes: Vec<NodeId>,
    /// Selected edge IDs.
    pub edges: Vec<EdgeId>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty() && self.edges.is_empty()
    }

    pub fn has_node(&self, id: NodeId) -> bool {
        self.nodes.contains(&id)
    }

    pub fn has_edge(&self, id: EdgeId) -> bool {
        self.edges.contains(&id)
    }

    pub fn toggle_node(&mut self, id: NodeId) {
        if let Some(pos) = self.nodes.iter().position(|n| *n == id) {
            self.nodes.remove(pos);
        } else {
            self.nodes.push(id);
        }
    }

    pub fn select_single_node(&mut self, id: NodeId) {
        self.clear();
        self.nodes.push(id);
    }

    pub fn select_single_edge(&mut self, id: EdgeId) {
        self.clear();
        self.edges.push(id);
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }
}

// ============================================================================
// Undo / Redo
// ============================================================================

/// A snapshot of the diagram state for undo/redo.
#[derive(Clone, Debug)]
struct DiagramSnapshot {
    nodes: Vec<DiagramNode>,
    edges: Vec<DiagramEdge>,
    layers: Vec<Layer>,
    groups: Vec<Group>,
}

/// Undo/redo manager with a fixed-size stack.
#[derive(Debug)]
struct UndoManager {
    undo_stack: VecDeque<DiagramSnapshot>,
    redo_stack: Vec<DiagramSnapshot>,
    max_steps: usize,
}

impl UndoManager {
    fn new(max_steps: usize) -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(max_steps),
            redo_stack: Vec::new(),
            max_steps,
        }
    }

    /// Save a snapshot before making a change.
    fn save(&mut self, snapshot: DiagramSnapshot) {
        if self.undo_stack.len() >= self.max_steps {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(snapshot);
        self.redo_stack.clear();
    }

    /// Undo: pop from undo, push current to redo, return the old state.
    fn undo(&mut self, current: DiagramSnapshot) -> Option<DiagramSnapshot> {
        let prev = self.undo_stack.pop_back()?;
        self.redo_stack.push(current);
        Some(prev)
    }

    /// Redo: pop from redo, push current to undo, return the newer state.
    fn redo(&mut self, current: DiagramSnapshot) -> Option<DiagramSnapshot> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push_back(current);
        Some(next)
    }

    #[cfg(test)]
    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[cfg(test)]
    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

// ============================================================================
// Clipboard
// ============================================================================

/// Clipboard state for copy/paste of nodes.
#[derive(Clone, Debug, Default)]
struct Clipboard {
    nodes: Vec<DiagramNode>,
    edges: Vec<DiagramEdge>,
}

impl Clipboard {
    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

// ============================================================================
// Main application state
// ============================================================================

/// The diagram editor application.
#[derive(Debug)]
pub struct DiagramApp {
    /// Window width.
    pub window_w: f32,
    /// Window height.
    pub window_h: f32,
    /// All nodes in the diagram.
    pub nodes: Vec<DiagramNode>,
    /// All edges in the diagram.
    pub edges: Vec<DiagramEdge>,
    /// Layers.
    pub layers: Vec<Layer>,
    /// Groups.
    pub groups: Vec<Group>,
    /// Current selection.
    pub selection: Selection,
    /// Current interaction mode.
    pub mode: InteractionMode,
    /// Whether to snap to grid.
    pub snap_to_grid: bool,
    /// Grid spacing in canvas units.
    pub grid_size: f32,
    /// Whether to show the grid overlay.
    pub show_grid: bool,
    /// Zoom factor (1.0 = 100%).
    pub zoom: f32,
    /// Pan offset X (canvas scroll position).
    pub pan_x: f32,
    /// Pan offset Y.
    pub pan_y: f32,
    /// Active layer ID for new elements.
    pub active_layer_id: LayerId,
    /// ID generator.
    id_gen: IdGen,
    /// Undo/redo manager.
    undo: UndoManager,
    /// Clipboard.
    clipboard: Clipboard,
    /// Whether to show the properties panel.
    pub show_properties: bool,
    /// Currently active template.
    pub current_template: DiagramTemplate,
    /// Selection rectangle start (screen coords, if dragging).
    pub rect_select_start: Option<(f32, f32)>,
    /// Selection rectangle end (screen coords).
    pub rect_select_end: Option<(f32, f32)>,
    /// Edge drawing: source node for a new edge.
    pub edge_source: Option<NodeId>,
    /// Whether app wants to quit.
    pub should_quit: bool,
}

impl DiagramApp {
    // ========================================================================
    // Construction
    // ========================================================================

    /// Create a new diagram editor with default (blank) template.
    pub fn new(window_w: f32, window_h: f32) -> Self {
        let mut id_gen = IdGen::new(1);
        let default_layer_id = id_gen.next_id();
        let layers = vec![Layer::new(default_layer_id, String::from("Layer 1"), 0)];

        Self {
            window_w,
            window_h,
            nodes: Vec::new(),
            edges: Vec::new(),
            layers,
            groups: Vec::new(),
            selection: Selection::default(),
            mode: InteractionMode::Select,
            snap_to_grid: true,
            grid_size: DEFAULT_GRID_SIZE,
            show_grid: true,
            zoom: DEFAULT_ZOOM,
            pan_x: 0.0,
            pan_y: 0.0,
            active_layer_id: default_layer_id,
            id_gen,
            undo: UndoManager::new(MAX_UNDO),
            clipboard: Clipboard::default(),
            show_properties: true,
            current_template: DiagramTemplate::Blank,
            rect_select_start: None,
            rect_select_end: None,
            edge_source: None,
            should_quit: false,
        }
    }

    // ========================================================================
    // Snapshot for undo
    // ========================================================================

    fn snapshot(&self) -> DiagramSnapshot {
        DiagramSnapshot {
            nodes: self.nodes.clone(),
            edges: self.edges.clone(),
            layers: self.layers.clone(),
            groups: self.groups.clone(),
        }
    }

    fn restore_snapshot(&mut self, snap: DiagramSnapshot) {
        self.nodes = snap.nodes;
        self.edges = snap.edges;
        self.layers = snap.layers;
        self.groups = snap.groups;
    }

    fn save_undo(&mut self) {
        let snap = self.snapshot();
        self.undo.save(snap);
    }

    /// Undo the last change.
    pub fn undo(&mut self) {
        let current = self.snapshot();
        if let Some(prev) = self.undo.undo(current) {
            self.restore_snapshot(prev);
        }
    }

    /// Redo a previously undone change.
    pub fn redo(&mut self) {
        let current = self.snapshot();
        if let Some(next) = self.undo.redo(current) {
            self.restore_snapshot(next);
        }
    }

    // ========================================================================
    // Node operations
    // ========================================================================

    /// Add a new node at the given canvas position.
    pub fn add_node(&mut self, shape: NodeShape, x: f32, y: f32) -> NodeId {
        self.save_undo();
        let id = self.id_gen.next_id();
        let sx = if self.snap_to_grid { self.snap(x) } else { x };
        let sy = if self.snap_to_grid { self.snap(y) } else { y };
        let node = DiagramNode::new(id, shape, sx, sy, self.active_layer_id);
        self.nodes.push(node);
        id
    }

    /// Remove a node and all connected edges.
    pub fn remove_node(&mut self, id: NodeId) {
        self.save_undo();
        self.edges.retain(|e| e.from_node != id && e.to_node != id);
        self.nodes.retain(|n| n.id != id);
        self.selection.nodes.retain(|nid| *nid != id);
    }

    /// Find a node by ID.
    pub fn find_node(&self, id: NodeId) -> Option<&DiagramNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Find a mutable node by ID.
    pub fn find_node_mut(&mut self, id: NodeId) -> Option<&mut DiagramNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Move a node by a delta offset.
    pub fn move_node(&mut self, id: NodeId, dx: f32, dy: f32) {
        let snap = self.snap_to_grid;
        let grid = self.grid_size;
        if let Some(node) = self.find_node_mut(id) {
            node.x += dx;
            node.y += dy;
            if snap && grid > 0.0 {
                node.x = (node.x / grid).round() * grid;
                node.y = (node.y / grid).round() * grid;
            }
        }
    }

    /// Move all nodes in a group by a delta offset.
    pub fn move_group(&mut self, group_id: GroupId, dx: f32, dy: f32) {
        let member_ids: Vec<NodeId> = self
            .groups
            .iter()
            .find(|g| g.id == group_id)
            .map_or_else(Vec::new, |g| g.member_ids.clone());

        for nid in member_ids {
            self.move_node(nid, dx, dy);
        }
    }

    /// Set the label text for a node.
    pub fn set_node_label(&mut self, id: NodeId, label: String) {
        self.save_undo();
        if let Some(node) = self.find_node_mut(id) {
            node.label = label;
        }
    }

    /// Set the fill color for a node.
    pub fn set_node_fill(&mut self, id: NodeId, color: Color) {
        self.save_undo();
        if let Some(node) = self.find_node_mut(id) {
            node.fill_color = color;
        }
    }

    /// Set the border color for a node.
    pub fn set_node_border_color(&mut self, id: NodeId, color: Color) {
        self.save_undo();
        if let Some(node) = self.find_node_mut(id) {
            node.border_color = color;
        }
    }

    /// Set the border width for a node.
    pub fn set_node_border_width(&mut self, id: NodeId, width: f32) {
        self.save_undo();
        if let Some(node) = self.find_node_mut(id) {
            node.border_width = width.max(0.0);
        }
    }

    /// Set the font size for a node's label.
    pub fn set_node_font_size(&mut self, id: NodeId, size: f32) {
        self.save_undo();
        if let Some(node) = self.find_node_mut(id) {
            node.font_size = size.clamp(8.0, 72.0);
        }
    }

    /// Resize a node.
    pub fn resize_node(&mut self, id: NodeId, width: f32, height: f32) {
        self.save_undo();
        if let Some(node) = self.find_node_mut(id) {
            node.width = width.max(20.0);
            node.height = height.max(20.0);
        }
    }

    /// Hit-test: find the topmost node at the given canvas point.
    pub fn node_at(&self, cx: f32, cy: f32) -> Option<NodeId> {
        // Iterate in reverse so top-rendered nodes are checked first.
        for node in self.nodes.iter().rev() {
            if self.is_layer_visible(node.layer_id) && node.hit_test(cx, cy) {
                return Some(node.id);
            }
        }
        None
    }

    // ========================================================================
    // Edge operations
    // ========================================================================

    /// Add a new edge between two nodes.
    pub fn add_edge(&mut self, from: NodeId, to: NodeId) -> EdgeId {
        self.save_undo();
        let id = self.id_gen.next_id();
        let edge = DiagramEdge::new(id, from, to, self.active_layer_id);
        self.edges.push(edge);
        id
    }

    /// Remove an edge.
    pub fn remove_edge(&mut self, id: EdgeId) {
        self.save_undo();
        self.edges.retain(|e| e.id != id);
        self.selection.edges.retain(|eid| *eid != id);
    }

    /// Find an edge by ID.
    pub fn find_edge(&self, id: EdgeId) -> Option<&DiagramEdge> {
        self.edges.iter().find(|e| e.id == id)
    }

    /// Find a mutable edge by ID.
    pub fn find_edge_mut(&mut self, id: EdgeId) -> Option<&mut DiagramEdge> {
        self.edges.iter_mut().find(|e| e.id == id)
    }

    /// Set edge kind.
    pub fn set_edge_kind(&mut self, id: EdgeId, kind: EdgeKind) {
        self.save_undo();
        if let Some(edge) = self.find_edge_mut(id) {
            edge.kind = kind;
        }
    }

    /// Set edge color.
    pub fn set_edge_color(&mut self, id: EdgeId, color: Color) {
        self.save_undo();
        if let Some(edge) = self.find_edge_mut(id) {
            edge.color = color;
        }
    }

    /// Set edge line style.
    pub fn set_edge_line_style(&mut self, id: EdgeId, style: LineStyle) {
        self.save_undo();
        if let Some(edge) = self.find_edge_mut(id) {
            edge.line_style = style;
        }
    }

    /// Set edge arrow heads.
    pub fn set_edge_arrows(&mut self, id: EdgeId, start: ArrowHead, end: ArrowHead) {
        self.save_undo();
        if let Some(edge) = self.find_edge_mut(id) {
            edge.start_arrow = start;
            edge.end_arrow = end;
        }
    }

    /// Set edge label.
    pub fn set_edge_label(&mut self, id: EdgeId, label: String) {
        self.save_undo();
        if let Some(edge) = self.find_edge_mut(id) {
            edge.label = label;
        }
    }

    // ========================================================================
    // Layer operations
    // ========================================================================

    /// Add a new layer. Returns the layer ID.
    pub fn add_layer(&mut self, name: String) -> LayerId {
        let id = self.id_gen.next_id();
        let order = self.layers.len();
        self.layers.push(Layer::new(id, name, order));
        id
    }

    /// Remove a layer and all elements on it.
    pub fn remove_layer(&mut self, id: LayerId) {
        // Must have at least one layer.
        if self.layers.len() <= 1 {
            return;
        }
        self.save_undo();
        self.nodes.retain(|n| n.layer_id != id);
        self.edges.retain(|e| e.layer_id != id);
        self.layers.retain(|l| l.id != id);
        // Fix active layer if we removed it.
        if self.active_layer_id == id {
            self.active_layer_id = self.layers.first().map_or(0, |l| l.id);
        }
    }

    /// Toggle layer visibility.
    pub fn toggle_layer_visibility(&mut self, id: LayerId) {
        if let Some(layer) = self.layers.iter_mut().find(|l| l.id == id) {
            layer.visible = !layer.visible;
        }
    }

    /// Check whether a layer is visible.
    pub fn is_layer_visible(&self, id: LayerId) -> bool {
        self.layers
            .iter()
            .find(|l| l.id == id)
            .is_some_and(|l| l.visible)
    }

    /// Move a layer up in the ordering.
    pub fn move_layer_up(&mut self, id: LayerId) {
        if let Some(idx) = self.layers.iter().position(|l| l.id == id)
            && idx > 0
        {
            self.layers.swap(idx, idx.saturating_sub(1));
            self.reindex_layers();
        }
    }

    /// Move a layer down in the ordering.
    pub fn move_layer_down(&mut self, id: LayerId) {
        if let Some(idx) = self.layers.iter().position(|l| l.id == id)
            && idx.saturating_add(1) < self.layers.len()
        {
            self.layers.swap(idx, idx.saturating_add(1));
            self.reindex_layers();
        }
    }

    fn reindex_layers(&mut self) {
        for (i, layer) in self.layers.iter_mut().enumerate() {
            layer.order = i;
        }
    }

    // ========================================================================
    // Group operations
    // ========================================================================

    /// Group the currently selected nodes. Returns the group ID or None.
    pub fn group_selection(&mut self) -> Option<GroupId> {
        if self.selection.nodes.len() < 2 {
            return None;
        }
        self.save_undo();
        let id = self.id_gen.next_id();
        let members = self.selection.nodes.clone();
        // Assign group_id to each member node.
        for nid in &members {
            if let Some(node) = self.find_node_mut(*nid) {
                node.group_id = Some(id);
            }
        }
        self.groups.push(Group::new(id, members));
        Some(id)
    }

    /// Ungroup: dissolve the group that contains the given node.
    pub fn ungroup(&mut self, node_id: NodeId) {
        let group_id = self.find_node(node_id).and_then(|n| n.group_id);
        if let Some(gid) = group_id {
            self.save_undo();
            for node in &mut self.nodes {
                if node.group_id == Some(gid) {
                    node.group_id = None;
                }
            }
            self.groups.retain(|g| g.id != gid);
        }
    }

    // ========================================================================
    // Alignment
    // ========================================================================

    /// Apply an alignment operation to the currently selected nodes.
    pub fn align_selection(&mut self, op: AlignOp) {
        if self.selection.nodes.len() < 2 {
            return;
        }
        self.save_undo();

        // Collect positions.
        let positions: Vec<(NodeId, f32, f32, f32, f32)> = self
            .selection
            .nodes
            .iter()
            .filter_map(|nid| {
                self.find_node(*nid)
                    .map(|n| (n.id, n.x, n.y, n.width, n.height))
            })
            .collect();

        if positions.is_empty() {
            return;
        }

        match op {
            AlignOp::Left => {
                let min_x = positions.iter().map(|p| p.1).fold(f32::MAX, f32::min);
                for (nid, _, _, _, _) in &positions {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.x = min_x;
                    }
                }
            }
            AlignOp::Right => {
                let max_right = positions.iter().map(|p| p.1 + p.3).fold(f32::MIN, f32::max);
                for (nid, _, _, w, _) in &positions {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.x = max_right - w;
                    }
                }
            }
            AlignOp::CenterH => {
                let avg_cx: f32 =
                    positions.iter().map(|p| p.1 + p.3 / 2.0).sum::<f32>() / positions.len() as f32;
                for (nid, _, _, w, _) in &positions {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.x = avg_cx - w / 2.0;
                    }
                }
            }
            AlignOp::Top => {
                let min_y = positions.iter().map(|p| p.2).fold(f32::MAX, f32::min);
                for (nid, _, _, _, _) in &positions {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.y = min_y;
                    }
                }
            }
            AlignOp::Bottom => {
                let max_bottom = positions.iter().map(|p| p.2 + p.4).fold(f32::MIN, f32::max);
                for (nid, _, _, _, h) in &positions {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.y = max_bottom - h;
                    }
                }
            }
            AlignOp::CenterV => {
                let avg_cy: f32 =
                    positions.iter().map(|p| p.2 + p.4 / 2.0).sum::<f32>() / positions.len() as f32;
                for (nid, _, _, _, h) in &positions {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.y = avg_cy - h / 2.0;
                    }
                }
            }
            AlignOp::DistributeH => {
                if positions.len() < 3 {
                    return;
                }
                let mut sorted: Vec<_> = positions.clone();
                sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                let total_w: f32 = sorted.iter().map(|p| p.3).sum();
                let first_x = sorted.first().map_or(0.0, |p| p.1);
                let last_end = sorted.last().map_or(0.0, |p| p.1 + p.3);
                let spacing =
                    (last_end - first_x - total_w) / (sorted.len().saturating_sub(1)) as f32;
                let mut current_x = first_x;
                for (nid, _, _, w, _) in &sorted {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.x = current_x;
                    }
                    current_x += w + spacing;
                }
            }
            AlignOp::DistributeV => {
                if positions.len() < 3 {
                    return;
                }
                let mut sorted: Vec<_> = positions.clone();
                sorted.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
                let total_h: f32 = sorted.iter().map(|p| p.4).sum();
                let first_y = sorted.first().map_or(0.0, |p| p.2);
                let last_end = sorted.last().map_or(0.0, |p| p.2 + p.4);
                let spacing =
                    (last_end - first_y - total_h) / (sorted.len().saturating_sub(1)) as f32;
                let mut current_y = first_y;
                for (nid, _, _, _, h) in &sorted {
                    if let Some(n) = self.find_node_mut(*nid) {
                        n.y = current_y;
                    }
                    current_y += h + spacing;
                }
            }
        }
    }

    // ========================================================================
    // Zoom / Pan
    // ========================================================================

    /// Set zoom level, clamped to valid range.
    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Zoom in one step.
    pub fn zoom_in(&mut self) {
        let steps = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
        for &s in &steps {
            if s > self.zoom + 0.01 {
                self.zoom = s;
                return;
            }
        }
    }

    /// Zoom out one step.
    pub fn zoom_out(&mut self) {
        let steps = [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];
        for &s in steps.iter().rev() {
            if s < self.zoom - 0.01 {
                self.zoom = s;
                return;
            }
        }
    }

    /// Returns zoom as a percentage string, e.g. "100%".
    pub fn zoom_percent_str(&self) -> String {
        format!("{}%", (self.zoom * 100.0) as u32)
    }

    /// Pan by a delta in screen pixels.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        self.pan_x += dx;
        self.pan_y += dy;
    }

    /// Reset pan to origin.
    pub fn reset_pan(&mut self) {
        self.pan_x = 0.0;
        self.pan_y = 0.0;
    }

    // ========================================================================
    // Grid snapping
    // ========================================================================

    /// Snap a value to the nearest grid line.
    pub fn snap(&self, value: f32) -> f32 {
        if self.grid_size <= 0.0 {
            return value;
        }
        (value / self.grid_size).round() * self.grid_size
    }

    /// Set the grid size.
    pub fn set_grid_size(&mut self, size: f32) {
        self.grid_size = size.clamp(5.0, 100.0);
    }

    // ========================================================================
    // Copy / Paste / Duplicate
    // ========================================================================

    /// Copy selected nodes and their interconnecting edges to clipboard.
    pub fn copy_selection(&mut self) {
        let node_ids: Vec<NodeId> = self.selection.nodes.clone();
        let nodes: Vec<DiagramNode> = self
            .nodes
            .iter()
            .filter(|n| node_ids.contains(&n.id))
            .cloned()
            .collect();
        let edges: Vec<DiagramEdge> = self
            .edges
            .iter()
            .filter(|e| node_ids.contains(&e.from_node) && node_ids.contains(&e.to_node))
            .cloned()
            .collect();
        self.clipboard = Clipboard { nodes, edges };
    }

    /// Paste clipboard contents at an offset.
    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        self.save_undo();

        let offset = 20.0;
        let mut id_map: Vec<(NodeId, NodeId)> = Vec::new();

        for old_node in &self.clipboard.nodes {
            let new_id = self.id_gen.next_id();
            id_map.push((old_node.id, new_id));
            let mut new_node = old_node.clone();
            new_node.id = new_id;
            new_node.x += offset;
            new_node.y += offset;
            new_node.group_id = None;
            self.nodes.push(new_node);
        }

        for old_edge in &self.clipboard.edges {
            let new_from = id_map
                .iter()
                .find(|m| m.0 == old_edge.from_node)
                .map(|m| m.1);
            let new_to = id_map.iter().find(|m| m.0 == old_edge.to_node).map(|m| m.1);
            if let (Some(from), Some(to)) = (new_from, new_to) {
                let new_id = self.id_gen.next_id();
                let mut new_edge = old_edge.clone();
                new_edge.id = new_id;
                new_edge.from_node = from;
                new_edge.to_node = to;
                self.edges.push(new_edge);
            }
        }

        // Select the newly pasted nodes.
        self.selection.clear();
        for (_, new_id) in &id_map {
            self.selection.nodes.push(*new_id);
        }
    }

    /// Duplicate the current selection in-place with an offset.
    pub fn duplicate_selection(&mut self) {
        self.copy_selection();
        self.paste();
    }

    // ========================================================================
    // Delete selection
    // ========================================================================

    /// Delete all currently selected nodes and edges.
    pub fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.save_undo();

        let node_ids = self.selection.nodes.clone();
        let edge_ids = self.selection.edges.clone();

        for nid in &node_ids {
            self.edges
                .retain(|e| e.from_node != *nid && e.to_node != *nid);
            self.nodes.retain(|n| n.id != *nid);
        }
        for eid in &edge_ids {
            self.edges.retain(|e| e.id != *eid);
        }

        self.selection.clear();
    }

    // ========================================================================
    // Templates
    // ========================================================================

    /// Load a diagram template, replacing all current content.
    pub fn load_template(&mut self, template: DiagramTemplate) {
        self.save_undo();
        self.nodes.clear();
        self.edges.clear();
        self.groups.clear();
        self.selection.clear();
        self.current_template = template;

        match template {
            DiagramTemplate::Blank => {}
            DiagramTemplate::Flowchart => self.create_flowchart_template(),
            DiagramTemplate::OrgChart => self.create_org_chart_template(),
            DiagramTemplate::UmlClass => self.create_uml_class_template(),
            DiagramTemplate::NetworkDiagram => self.create_network_template(),
            DiagramTemplate::MindMap => self.create_mind_map_template(),
            DiagramTemplate::ErDiagram => self.create_er_diagram_template(),
        }
    }

    fn create_flowchart_template(&mut self) {
        let lid = self.active_layer_id;
        let start = self.add_template_node(NodeShape::RoundedRectangle, 200.0, 40.0, lid, "Start");
        let proc1 = self.add_template_node(NodeShape::Rectangle, 200.0, 140.0, lid, "Process");
        let dec = self.add_template_node(NodeShape::Diamond, 200.0, 260.0, lid, "Decision?");
        let proc2 = self.add_template_node(NodeShape::Rectangle, 60.0, 380.0, lid, "Action A");
        let proc3 = self.add_template_node(NodeShape::Rectangle, 340.0, 380.0, lid, "Action B");
        let end = self.add_template_node(NodeShape::RoundedRectangle, 200.0, 500.0, lid, "End");

        self.add_edge(start, proc1);
        self.add_edge(proc1, dec);
        self.add_edge(dec, proc2);
        self.add_edge(dec, proc3);
        self.add_edge(proc2, end);
        self.add_edge(proc3, end);
    }

    fn create_org_chart_template(&mut self) {
        let lid = self.active_layer_id;
        let ceo = self.add_template_node(NodeShape::RoundedRectangle, 300.0, 40.0, lid, "CEO");
        let vp1 = self.add_template_node(NodeShape::Rectangle, 100.0, 160.0, lid, "VP Eng");
        let vp2 = self.add_template_node(NodeShape::Rectangle, 300.0, 160.0, lid, "VP Sales");
        let vp3 = self.add_template_node(NodeShape::Rectangle, 500.0, 160.0, lid, "VP Ops");
        let m1 = self.add_template_node(NodeShape::Rectangle, 40.0, 280.0, lid, "Team Lead A");
        let m2 = self.add_template_node(NodeShape::Rectangle, 180.0, 280.0, lid, "Team Lead B");

        self.add_edge(ceo, vp1);
        self.add_edge(ceo, vp2);
        self.add_edge(ceo, vp3);
        self.add_edge(vp1, m1);
        self.add_edge(vp1, m2);
    }

    fn create_uml_class_template(&mut self) {
        let lid = self.active_layer_id;
        let c1 = self.add_template_node(NodeShape::Rectangle, 100.0, 60.0, lid, "BaseClass");
        let c2 = self.add_template_node(NodeShape::Rectangle, 60.0, 220.0, lid, "ChildA");
        let c3 = self.add_template_node(NodeShape::Rectangle, 280.0, 220.0, lid, "ChildB");
        let c4 = self.add_template_node(NodeShape::Rectangle, 400.0, 60.0, lid, "Interface");

        self.add_edge(c2, c1);
        self.add_edge(c3, c1);
        self.add_edge(c3, c4);
    }

    fn create_network_template(&mut self) {
        let lid = self.active_layer_id;
        let router = self.add_template_node(NodeShape::Hexagon, 250.0, 40.0, lid, "Router");
        let sw1 = self.add_template_node(NodeShape::Rectangle, 100.0, 180.0, lid, "Switch A");
        let sw2 = self.add_template_node(NodeShape::Rectangle, 400.0, 180.0, lid, "Switch B");
        let srv1 = self.add_template_node(NodeShape::Cylinder, 40.0, 320.0, lid, "Server 1");
        let srv2 = self.add_template_node(NodeShape::Cylinder, 180.0, 320.0, lid, "Server 2");
        let db = self.add_template_node(NodeShape::Cylinder, 400.0, 320.0, lid, "Database");

        self.add_edge(router, sw1);
        self.add_edge(router, sw2);
        self.add_edge(sw1, srv1);
        self.add_edge(sw1, srv2);
        self.add_edge(sw2, db);
    }

    fn create_mind_map_template(&mut self) {
        let lid = self.active_layer_id;
        let center = self.add_template_node(NodeShape::Ellipse, 250.0, 200.0, lid, "Main Topic");
        let b1 = self.add_template_node(NodeShape::RoundedRectangle, 50.0, 60.0, lid, "Branch 1");
        let b2 = self.add_template_node(NodeShape::RoundedRectangle, 450.0, 60.0, lid, "Branch 2");
        let b3 = self.add_template_node(NodeShape::RoundedRectangle, 50.0, 340.0, lid, "Branch 3");
        let b4 = self.add_template_node(NodeShape::RoundedRectangle, 450.0, 340.0, lid, "Branch 4");

        self.add_edge(center, b1);
        self.add_edge(center, b2);
        self.add_edge(center, b3);
        self.add_edge(center, b4);
    }

    fn create_er_diagram_template(&mut self) {
        let lid = self.active_layer_id;
        let e1 = self.add_template_node(NodeShape::Rectangle, 60.0, 100.0, lid, "Customer");
        let e2 = self.add_template_node(NodeShape::Rectangle, 300.0, 100.0, lid, "Order");
        let e3 = self.add_template_node(NodeShape::Rectangle, 540.0, 100.0, lid, "Product");
        let r1 = self.add_template_node(NodeShape::Diamond, 180.0, 260.0, lid, "places");
        let r2 = self.add_template_node(NodeShape::Diamond, 420.0, 260.0, lid, "contains");

        self.add_edge(e1, r1);
        self.add_edge(r1, e2);
        self.add_edge(e2, r2);
        self.add_edge(r2, e3);
    }

    /// Helper for templates: add a node with a preset label.
    fn add_template_node(
        &mut self,
        shape: NodeShape,
        x: f32,
        y: f32,
        layer_id: LayerId,
        label: &str,
    ) -> NodeId {
        let id = self.id_gen.next_id();
        let mut node = DiagramNode::new(id, shape, x, y, layer_id);
        node.label = String::from(label);
        self.nodes.push(node);
        id
    }

    // ========================================================================
    // Auto-layout
    // ========================================================================

    /// Apply a basic automatic layout to all nodes.
    pub fn auto_layout(&mut self, direction: LayoutDirection) {
        if self.nodes.is_empty() {
            return;
        }
        self.save_undo();

        let spacing_h = 60.0;
        let spacing_v = 80.0;
        let start_x = 40.0;
        let start_y = 40.0;

        // Simple: lay out in a grid based on direction.
        let cols = (self.nodes.len() as f32).sqrt().ceil() as usize;

        for (i, node) in self.nodes.iter_mut().enumerate() {
            let cmax = cols.max(1);
            let row = i.checked_div(cmax).unwrap_or(0);
            let col = i.checked_rem(cmax).unwrap_or(0);
            match direction {
                LayoutDirection::TopDown => {
                    node.x = start_x + col as f32 * (DEFAULT_NODE_W + spacing_h);
                    node.y = start_y + row as f32 * (DEFAULT_NODE_H + spacing_v);
                }
                LayoutDirection::LeftRight => {
                    node.x = start_x + row as f32 * (DEFAULT_NODE_W + spacing_h);
                    node.y = start_y + col as f32 * (DEFAULT_NODE_H + spacing_v);
                }
            }
        }
    }

    // ========================================================================
    // Export: SVG
    // ========================================================================

    /// Export the diagram to an SVG string.
    pub fn export_svg(&self) -> String {
        let mut svg = String::with_capacity(4096);

        // Compute bounding box.
        let (min_x, min_y, max_x, max_y) = self.bounding_box();
        let margin = 20.0;
        let w = (max_x - min_x) + margin * 2.0;
        let h = (max_y - min_y) + margin * 2.0;
        let off_x = -min_x + margin;
        let off_y = -min_y + margin;

        svg.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">\n"
        ));
        svg.push_str(&format!("<g transform=\"translate({off_x},{off_y})\">\n"));

        // Edges first (below nodes).
        for edge in &self.edges {
            if !self.is_layer_visible(edge.layer_id) {
                continue;
            }
            let from = self.find_node(edge.from_node);
            let to = self.find_node(edge.to_node);
            if let (Some(f), Some(t)) = (from, to) {
                let (fx, fy) = f.connection_point(t.center().0, t.center().1);
                let (tx, ty) = t.connection_point(f.center().0, f.center().1);
                let stroke = color_to_svg_hex(edge.color);
                let dash = match edge.line_style {
                    LineStyle::Solid => String::new(),
                    LineStyle::Dashed => String::from(" stroke-dasharray=\"8,4\""),
                    LineStyle::Dotted => String::from(" stroke-dasharray=\"2,4\""),
                };
                svg.push_str(&format!(
                    "  <line x1=\"{fx}\" y1=\"{fy}\" x2=\"{tx}\" y2=\"{ty}\" stroke=\"{stroke}\" stroke-width=\"{}\"{dash}/>\n",
                    edge.line_width
                ));
                if !edge.label.is_empty() {
                    let mx = f32::midpoint(fx, tx);
                    let my = f32::midpoint(fy, ty);
                    svg.push_str(&format!(
                        "  <text x=\"{mx}\" y=\"{}\" text-anchor=\"middle\" fill=\"{stroke}\" font-size=\"12\">{}</text>\n",
                        my - 6.0,
                        escape_xml(&edge.label)
                    ));
                }
            }
        }

        // Nodes.
        for node in &self.nodes {
            if !self.is_layer_visible(node.layer_id) {
                continue;
            }
            let fill = color_to_svg_hex(node.fill_color);
            let stroke = color_to_svg_hex(node.border_color);
            let bw = node.border_width;
            let (cx, cy) = node.center();

            match node.shape {
                NodeShape::Rectangle => {
                    svg.push_str(&format!(
                        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.x, node.y, node.width, node.height
                    ));
                }
                NodeShape::RoundedRectangle => {
                    svg.push_str(&format!(
                        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"8\" ry=\"8\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.x, node.y, node.width, node.height
                    ));
                }
                NodeShape::Diamond => {
                    let (mx, my) = (cx, cy);
                    let hw = node.width / 2.0;
                    let hh = node.height / 2.0;
                    svg.push_str(&format!(
                        "  <polygon points=\"{mx},{} {},{my} {mx},{} {},{my}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.y, node.x + node.width, node.y + node.height, node.x
                    ));
                    let _ = (hw, hh); // used implicitly via cx/cy
                }
                NodeShape::Circle | NodeShape::Ellipse => {
                    let rx = node.width / 2.0;
                    let ry = node.height / 2.0;
                    svg.push_str(&format!(
                        "  <ellipse cx=\"{cx}\" cy=\"{cy}\" rx=\"{rx}\" ry=\"{ry}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                    ));
                }
                NodeShape::Parallelogram => {
                    let skew = node.width * 0.2;
                    svg.push_str(&format!(
                        "  <polygon points=\"{},{} {},{} {},{} {},{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.x + skew, node.y,
                        node.x + node.width, node.y,
                        node.x + node.width - skew, node.y + node.height,
                        node.x, node.y + node.height
                    ));
                }
                NodeShape::Hexagon => {
                    let qw = node.width * 0.25;
                    let my = cy;
                    svg.push_str(&format!(
                        "  <polygon points=\"{},{my} {},{} {},{} {},{my} {},{} {},{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.x, node.x + qw, node.y,
                        node.x + node.width - qw, node.y,
                        node.x + node.width,
                        node.x + node.width - qw, node.y + node.height,
                        node.x + qw, node.y + node.height
                    ));
                }
                NodeShape::Triangle => {
                    svg.push_str(&format!(
                        "  <polygon points=\"{cx},{} {},{} {},{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.y, node.x, node.y + node.height,
                        node.x + node.width, node.y + node.height
                    ));
                }
                NodeShape::Cylinder => {
                    let ry = node.height * 0.12;
                    let body_y = node.y + ry;
                    let body_h = node.height - ry * 2.0;
                    svg.push_str(&format!(
                        "  <rect x=\"{}\" y=\"{body_y}\" width=\"{}\" height=\"{body_h}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.x, node.width
                    ));
                    svg.push_str(&format!(
                        "  <ellipse cx=\"{cx}\" cy=\"{body_y}\" rx=\"{}\" ry=\"{ry}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        node.width / 2.0
                    ));
                    svg.push_str(&format!(
                        "  <ellipse cx=\"{cx}\" cy=\"{}\" rx=\"{}\" ry=\"{ry}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        body_y + body_h, node.width / 2.0
                    ));
                }
                NodeShape::Cloud => {
                    // Simplified cloud as overlapping ellipses.
                    let w3 = node.width / 3.0;
                    let h2 = node.height / 2.0;
                    svg.push_str(&format!(
                        "  <ellipse cx=\"{cx}\" cy=\"{}\" rx=\"{w3}\" ry=\"{h2}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        cy - node.height * 0.1
                    ));
                    svg.push_str(&format!(
                        "  <ellipse cx=\"{}\" cy=\"{cy}\" rx=\"{}\" ry=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        cx - w3 * 0.6, w3 * 0.8, h2 * 0.8
                    ));
                    svg.push_str(&format!(
                        "  <ellipse cx=\"{}\" cy=\"{cy}\" rx=\"{}\" ry=\"{}\" fill=\"{fill}\" stroke=\"{stroke}\" stroke-width=\"{bw}\"/>\n",
                        cx + w3 * 0.6, w3 * 0.8, h2 * 0.8
                    ));
                }
            }

            // Label text
            if !node.label.is_empty() {
                let text_color = color_to_svg_hex(TEXT);
                svg.push_str(&format!(
                    "  <text x=\"{cx}\" y=\"{}\" text-anchor=\"middle\" fill=\"{text_color}\" font-size=\"{}\">{}</text>\n",
                    cy + node.font_size / 3.0,
                    node.font_size,
                    escape_xml(&node.label)
                ));
            }
        }

        svg.push_str("</g>\n</svg>\n");
        svg
    }

    // ========================================================================
    // Export: JSON
    // ========================================================================

    /// Export the diagram to a simple JSON string.
    pub fn export_json(&self) -> String {
        let mut out = String::with_capacity(2048);
        out.push_str("{\n  \"nodes\": [\n");
        for (i, node) in self.nodes.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"id\":{},\"shape\":\"{}\",\"x\":{},\"y\":{},\"w\":{},\"h\":{},\"label\":\"{}\"}}",
                node.id,
                node.shape.label(),
                node.x, node.y, node.width, node.height,
                escape_json(&node.label)
            ));
            if i.saturating_add(1) < self.nodes.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ],\n  \"edges\": [\n");
        for (i, edge) in self.edges.iter().enumerate() {
            out.push_str(&format!(
                "    {{\"id\":{},\"from\":{},\"to\":{},\"kind\":\"{}\",\"label\":\"{}\"}}",
                edge.id,
                edge.from_node,
                edge.to_node,
                edge.kind.label(),
                escape_json(&edge.label)
            ));
            if i.saturating_add(1) < self.edges.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str("  ]\n}\n");
        out
    }

    // ========================================================================
    // Bounding box
    // ========================================================================

    /// Compute the bounding box of all nodes.
    pub fn bounding_box(&self) -> (f32, f32, f32, f32) {
        if self.nodes.is_empty() {
            return (0.0, 0.0, 100.0, 100.0);
        }
        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;
        for node in &self.nodes {
            if node.x < min_x {
                min_x = node.x;
            }
            if node.y < min_y {
                min_y = node.y;
            }
            let r = node.x + node.width;
            let b = node.y + node.height;
            if r > max_x {
                max_x = r;
            }
            if b > max_y {
                max_y = b;
            }
        }
        (min_x, min_y, max_x, max_y)
    }

    // ========================================================================
    // Screen <-> canvas coordinate conversion
    // ========================================================================

    /// Convert screen coordinates to canvas coordinates.
    pub fn screen_to_canvas(&self, sx: f32, sy: f32) -> (f32, f32) {
        let canvas_x = (sx - PALETTE_WIDTH - self.pan_x) / self.zoom;
        let canvas_y = (sy - TOOLBAR_HEIGHT - self.pan_y) / self.zoom;
        (canvas_x, canvas_y)
    }

    /// Convert canvas coordinates to screen coordinates.
    pub fn canvas_to_screen(&self, cx: f32, cy: f32) -> (f32, f32) {
        let sx = cx * self.zoom + self.pan_x + PALETTE_WIDTH;
        let sy = cy * self.zoom + self.pan_y + TOOLBAR_HEIGHT;
        (sx, sy)
    }

    // ========================================================================
    // Rendering: full frame
    // ========================================================================

    /// Render the entire application UI and return draw commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds: Vec<RenderCommand> = Vec::with_capacity(512);

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_w,
            height: self.window_h,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_toolbar(&mut cmds);
        self.render_palette(&mut cmds);
        self.render_canvas(&mut cmds);
        if self.show_properties {
            self.render_properties_panel(&mut cmds);
        }
        self.render_status_bar(&mut cmds);

        cmds
    }

    // ========================================================================
    // Rendering: toolbar
    // ========================================================================

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        // Toolbar background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_w,
            height: TOOLBAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_w,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });

        // Tool buttons.
        let buttons = [
            ("Select", self.mode == InteractionMode::Select),
            ("Edge", self.mode == InteractionMode::AddEdge),
            ("Pan", self.mode == InteractionMode::Pan),
        ];

        let mut bx = 8.0;
        for (label, active) in &buttons {
            let bg = if *active { BLUE } else { SURFACE0 };
            let fg = if *active { CRUST } else { TEXT };
            cmds.push(RenderCommand::FillRect {
                x: bx,
                y: 6.0,
                width: 60.0,
                height: 28.0,
                color: bg,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });
            cmds.push(RenderCommand::Text {
                x: bx + 8.0,
                y: 14.0,
                text: String::from(*label),
                color: fg,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(52.0),
            });
            bx += 68.0;
        }

        // Zoom controls.
        let zoom_text = self.zoom_percent_str();
        cmds.push(RenderCommand::Text {
            x: bx + 20.0,
            y: 14.0,
            text: zoom_text,
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
        });

        // Grid toggle.
        let grid_label = if self.show_grid {
            "Grid: On"
        } else {
            "Grid: Off"
        };
        cmds.push(RenderCommand::Text {
            x: bx + 100.0,
            y: 14.0,
            text: String::from(grid_label),
            color: if self.show_grid { GREEN } else { OVERLAY0 },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });

        // Snap toggle.
        let snap_label = if self.snap_to_grid {
            "Snap: On"
        } else {
            "Snap: Off"
        };
        cmds.push(RenderCommand::Text {
            x: bx + 190.0,
            y: 14.0,
            text: String::from(snap_label),
            color: if self.snap_to_grid { GREEN } else { OVERLAY0 },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });

        // Undo/Redo indicators.
        let undo_text = format!(
            "Undo:{} Redo:{}",
            self.undo.undo_count(),
            self.undo.redo_count()
        );
        cmds.push(RenderCommand::Text {
            x: self.window_w - 160.0,
            y: 14.0,
            text: undo_text,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
        });
    }

    // ========================================================================
    // Rendering: shape palette sidebar
    // ========================================================================

    fn render_palette(&self, cmds: &mut Vec<RenderCommand>) {
        let pal_y = TOOLBAR_HEIGHT;
        let pal_h = self.window_h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: pal_y,
            width: PALETTE_WIDTH,
            height: pal_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: PALETTE_WIDTH,
            y1: pal_y,
            x2: PALETTE_WIDTH,
            y2: pal_y + pal_h,
            color: SURFACE0,
            width: 1.0,
        });

        // Section header: Shapes.
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: pal_y + 16.0,
            text: String::from("Shapes"),
            color: TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PALETTE_WIDTH - 24.0),
        });

        // Shape buttons.
        let mut by = pal_y + 36.0;
        for shape in NodeShape::all() {
            let is_active = matches!(self.mode, InteractionMode::AddNode(s) if s == *shape);
            let bg = if is_active {
                shape.accent_color()
            } else {
                SURFACE0
            };
            let fg = if is_active { CRUST } else { TEXT };

            cmds.push(RenderCommand::FillRect {
                x: 8.0,
                y: by,
                width: PALETTE_WIDTH - 16.0,
                height: SHAPE_BTN_H,
                color: bg,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });

            // Mini shape icon (a small preview colored square).
            cmds.push(RenderCommand::FillRect {
                x: 14.0,
                y: by + 6.0,
                width: 20.0,
                height: 20.0,
                color: shape.accent_color(),
                corner_radii: if matches!(shape, NodeShape::Circle | NodeShape::Ellipse) {
                    CornerRadii::all(10.0)
                } else if matches!(shape, NodeShape::RoundedRectangle) {
                    CornerRadii::all(4.0)
                } else {
                    CornerRadii::ZERO
                },
            });

            cmds.push(RenderCommand::Text {
                x: 40.0,
                y: by + 10.0,
                text: String::from(shape.label()),
                color: fg,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PALETTE_WIDTH - 56.0),
            });

            by += SHAPE_BTN_H + 4.0;
        }

        // Section header: Layers.
        by += 12.0;
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: by,
            text: String::from("Layers"),
            color: TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PALETTE_WIDTH - 24.0),
        });
        by += 20.0;

        for layer in &self.layers {
            let is_active = layer.id == self.active_layer_id;
            let bg = if is_active { SURFACE1 } else { SURFACE0 };

            cmds.push(RenderCommand::FillRect {
                x: 8.0,
                y: by,
                width: PALETTE_WIDTH - 16.0,
                height: LAYER_ROW_H,
                color: bg,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });

            // Visibility indicator.
            let vis_color = if layer.visible { GREEN } else { OVERLAY0 };
            cmds.push(RenderCommand::FillRect {
                x: 14.0,
                y: by + 8.0,
                width: 12.0,
                height: 12.0,
                color: vis_color,
                corner_radii: CornerRadii::all(2.0),
            });

            cmds.push(RenderCommand::Text {
                x: 32.0,
                y: by + 9.0,
                text: layer.name.clone(),
                color: if layer.visible { TEXT } else { OVERLAY0 },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PALETTE_WIDTH - 48.0),
            });

            by += LAYER_ROW_H + 2.0;
        }

        // Section header: Templates.
        by += 12.0;
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: by,
            text: String::from("Templates"),
            color: TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PALETTE_WIDTH - 24.0),
        });
        by += 20.0;

        for tmpl in DiagramTemplate::all() {
            let is_active = self.current_template == *tmpl;
            let fg = if is_active { BLUE } else { SUBTEXT0 };

            cmds.push(RenderCommand::Text {
                x: 14.0,
                y: by,
                text: String::from(tmpl.label()),
                color: fg,
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(PALETTE_WIDTH - 28.0),
            });

            by += 20.0;
        }
    }

    // ========================================================================
    // Rendering: canvas area
    // ========================================================================

    fn render_canvas(&self, cmds: &mut Vec<RenderCommand>) {
        let canvas_x = PALETTE_WIDTH;
        let canvas_y = TOOLBAR_HEIGHT;
        let props_w = if self.show_properties {
            PROPERTIES_WIDTH
        } else {
            0.0
        };
        let canvas_w = self.window_w - PALETTE_WIDTH - props_w;
        let canvas_h = self.window_h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Canvas background.
        cmds.push(RenderCommand::FillRect {
            x: canvas_x,
            y: canvas_y,
            width: canvas_w,
            height: canvas_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip to canvas area.
        cmds.push(RenderCommand::PushClip {
            x: canvas_x,
            y: canvas_y,
            width: canvas_w,
            height: canvas_h,
        });

        // Apply pan and zoom transforms.
        cmds.push(RenderCommand::PushTranslate {
            dx: canvas_x + self.pan_x,
            dy: canvas_y + self.pan_y,
        });

        // Grid.
        if self.show_grid {
            self.render_grid(cmds, canvas_w, canvas_h);
        }

        // Edges (render below nodes).
        for edge in &self.edges {
            if !self.is_layer_visible(edge.layer_id) {
                continue;
            }
            self.render_edge(cmds, edge);
        }

        // Nodes.
        for node in &self.nodes {
            if !self.is_layer_visible(node.layer_id) {
                continue;
            }
            self.render_node(cmds, node);
        }

        // Selection rectangle overlay.
        if let (Some(start), Some(end)) = (self.rect_select_start, self.rect_select_end) {
            let rx = start.0.min(end.0);
            let ry = start.1.min(end.1);
            let rw = (end.0 - start.0).abs();
            let rh = (end.1 - start.1).abs();
            cmds.push(RenderCommand::FillRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: Color::rgba(137, 180, 250, 40),
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::StrokeRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        cmds.push(RenderCommand::PopTranslate);
        cmds.push(RenderCommand::PopClip);
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>, view_w: f32, view_h: f32) {
        let grid = self.grid_size * self.zoom;
        if grid < 4.0 {
            return; // Too dense to render.
        }

        let grid_color = SURFACE0;
        let start_x = ((-self.pan_x) / grid).floor() * grid;
        let start_y = ((-self.pan_y) / grid).floor() * grid;
        let end_x = start_x + view_w / self.zoom + grid * 2.0;
        let end_y = start_y + view_h / self.zoom + grid * 2.0;

        let mut gx = start_x;
        while gx <= end_x {
            cmds.push(RenderCommand::Line {
                x1: gx * self.zoom,
                y1: start_y * self.zoom,
                x2: gx * self.zoom,
                y2: end_y * self.zoom,
                color: grid_color,
                width: 0.5,
            });
            gx += self.grid_size;
        }
        let mut gy = start_y;
        while gy <= end_y {
            cmds.push(RenderCommand::Line {
                x1: start_x * self.zoom,
                y1: gy * self.zoom,
                x2: end_x * self.zoom,
                y2: gy * self.zoom,
                color: grid_color,
                width: 0.5,
            });
            gy += self.grid_size;
        }
    }

    // ========================================================================
    // Rendering: individual node
    // ========================================================================

    fn render_node(&self, cmds: &mut Vec<RenderCommand>, node: &DiagramNode) {
        let z = self.zoom;
        let x = node.x * z;
        let y = node.y * z;
        let w = node.width * z;
        let h = node.height * z;
        let selected = self.selection.has_node(node.id);

        // Shape fill.
        match node.shape {
            NodeShape::Rectangle => {
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii::ZERO,
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            NodeShape::RoundedRectangle => {
                let r = 8.0 * z;
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(r),
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii::all(r),
                });
            }
            NodeShape::Diamond => {
                // Approximate diamond with a rotated rect using 4 lines.
                let cx = x + w / 2.0;
                let cy = y + h / 2.0;
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii::ZERO,
                });
                // Overlay lines to hint diamond shape.
                let lw = node.border_width;
                cmds.push(RenderCommand::Line {
                    x1: cx,
                    y1: y,
                    x2: x + w,
                    y2: cy,
                    color: node.border_color,
                    width: lw,
                });
                cmds.push(RenderCommand::Line {
                    x1: x + w,
                    y1: cy,
                    x2: cx,
                    y2: y + h,
                    color: node.border_color,
                    width: lw,
                });
                cmds.push(RenderCommand::Line {
                    x1: cx,
                    y1: y + h,
                    x2: x,
                    y2: cy,
                    color: node.border_color,
                    width: lw,
                });
                cmds.push(RenderCommand::Line {
                    x1: x,
                    y1: cy,
                    x2: cx,
                    y2: y,
                    color: node.border_color,
                    width: lw,
                });
            }
            NodeShape::Circle | NodeShape::Ellipse => {
                // Approximate with a rounded rectangle at maximum radii.
                let rx = w / 2.0;
                let ry = h / 2.0;
                let r = rx.min(ry);
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(r),
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii::all(r),
                });
            }
            NodeShape::Parallelogram => {
                // Approximate with a slightly skewed rectangle.
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii {
                        top_left: 0.0,
                        top_right: w * 0.15,
                        bottom_right: 0.0,
                        bottom_left: w * 0.15,
                    },
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii {
                        top_left: 0.0,
                        top_right: w * 0.15,
                        bottom_right: 0.0,
                        bottom_left: w * 0.15,
                    },
                });
            }
            NodeShape::Hexagon => {
                // Approximate with rounded rect.
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(h * 0.3),
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii::all(h * 0.3),
                });
            }
            NodeShape::Triangle => {
                // Triangle via 3 lines on a filled background.
                let cx = x + w / 2.0;
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.fill_color,
                    corner_radii: CornerRadii::ZERO,
                });
                let lw = node.border_width;
                cmds.push(RenderCommand::Line {
                    x1: cx,
                    y1: y,
                    x2: x,
                    y2: y + h,
                    color: node.border_color,
                    width: lw,
                });
                cmds.push(RenderCommand::Line {
                    x1: x,
                    y1: y + h,
                    x2: x + w,
                    y2: y + h,
                    color: node.border_color,
                    width: lw,
                });
                cmds.push(RenderCommand::Line {
                    x1: x + w,
                    y1: y + h,
                    x2: cx,
                    y2: y,
                    color: node.border_color,
                    width: lw,
                });
            }
            NodeShape::Cylinder => {
                // Cylinder: rect body + top/bottom ellipses.
                let cap_h = h * 0.12;
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: y + cap_h,
                    width: w,
                    height: h - cap_h * 2.0,
                    color: node.fill_color,
                    corner_radii: CornerRadii::ZERO,
                });
                // Top cap.
                cmds.push(RenderCommand::FillRect {
                    x,
                    y,
                    width: w,
                    height: cap_h * 2.0,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(w / 2.0),
                });
                // Bottom cap.
                cmds.push(RenderCommand::FillRect {
                    x,
                    y: y + h - cap_h * 2.0,
                    width: w,
                    height: cap_h * 2.0,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(w / 2.0),
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii {
                        top_left: w * 0.3,
                        top_right: w * 0.3,
                        bottom_right: w * 0.3,
                        bottom_left: w * 0.3,
                    },
                });
            }
            NodeShape::Cloud => {
                // Cloud: overlapping rounded rects.
                let r = w.min(h) / 3.0;
                cmds.push(RenderCommand::FillRect {
                    x: x + w * 0.1,
                    y: y + h * 0.15,
                    width: w * 0.8,
                    height: h * 0.7,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(r),
                });
                cmds.push(RenderCommand::FillRect {
                    x: x + w * 0.02,
                    y: y + h * 0.3,
                    width: w * 0.5,
                    height: h * 0.5,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(r * 0.8),
                });
                cmds.push(RenderCommand::FillRect {
                    x: x + w * 0.48,
                    y: y + h * 0.3,
                    width: w * 0.5,
                    height: h * 0.5,
                    color: node.fill_color,
                    corner_radii: CornerRadii::all(r * 0.8),
                });
                cmds.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: w,
                    height: h,
                    color: node.border_color,
                    line_width: node.border_width,
                    corner_radii: CornerRadii::all(r * 0.5),
                });
            }
        }

        // Label text.
        if !node.label.is_empty() {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let fs = node.font_size * z;
            cmds.push(RenderCommand::Text {
                x: cx - w * 0.4,
                y: cy - fs / 2.0,
                text: node.label.clone(),
                color: TEXT,
                font_size: fs,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w * 0.8),
            });
        }

        // Selection highlight.
        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 3.0,
                y: y - 3.0,
                width: w + 6.0,
                height: h + 6.0,
                color: BLUE,
                line_width: 2.0,
                corner_radii: CornerRadii::all(2.0),
            });
        }
    }

    // ========================================================================
    // Rendering: individual edge
    // ========================================================================

    fn render_edge(&self, cmds: &mut Vec<RenderCommand>, edge: &DiagramEdge) {
        let from = self.find_node(edge.from_node);
        let to = self.find_node(edge.to_node);
        let (from_node, to_node) = match (from, to) {
            (Some(f), Some(t)) => (f, t),
            _ => return,
        };

        let z = self.zoom;
        let (fc, tc) = (from_node.center(), to_node.center());
        let (fx, fy) = from_node.connection_point(tc.0, tc.1);
        let (tx, ty) = to_node.connection_point(fc.0, fc.1);

        let sx1 = fx * z;
        let sy1 = fy * z;
        let sx2 = tx * z;
        let sy2 = ty * z;

        let selected = self.selection.has_edge(edge.id);
        let color = if selected { BLUE } else { edge.color };

        match edge.kind {
            EdgeKind::Straight | EdgeKind::Bezier => {
                cmds.push(RenderCommand::Line {
                    x1: sx1,
                    y1: sy1,
                    x2: sx2,
                    y2: sy2,
                    color,
                    width: edge.line_width,
                });
            }
            EdgeKind::Orthogonal => {
                // Right-angle routing: go horizontal first, then vertical.
                let mid_x = f32::midpoint(sx1, sx2);
                cmds.push(RenderCommand::Line {
                    x1: sx1,
                    y1: sy1,
                    x2: mid_x,
                    y2: sy1,
                    color,
                    width: edge.line_width,
                });
                cmds.push(RenderCommand::Line {
                    x1: mid_x,
                    y1: sy1,
                    x2: mid_x,
                    y2: sy2,
                    color,
                    width: edge.line_width,
                });
                cmds.push(RenderCommand::Line {
                    x1: mid_x,
                    y1: sy2,
                    x2: sx2,
                    y2: sy2,
                    color,
                    width: edge.line_width,
                });
            }
        }

        // Arrow head at destination (simple triangle lines).
        if edge.end_arrow != ArrowHead::None {
            self.render_arrow_head(cmds, sx2, sy2, sx1, sy1, color, edge.line_width);
        }
        // Arrow head at source (reverse direction).
        if edge.start_arrow != ArrowHead::None {
            self.render_arrow_head(cmds, sx1, sy1, sx2, sy2, color, edge.line_width);
        }

        // Edge label at midpoint.
        if !edge.label.is_empty() {
            let mx = f32::midpoint(sx1, sx2);
            let my = f32::midpoint(sy1, sy2);
            cmds.push(RenderCommand::Text {
                x: mx,
                y: my - 10.0,
                text: edge.label.clone(),
                color: SUBTEXT0,
                font_size: 11.0 * z,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0 * z),
            });
        }
    }

    fn render_arrow_head(
        &self,
        cmds: &mut Vec<RenderCommand>,
        tip_x: f32,
        tip_y: f32,
        from_x: f32,
        from_y: f32,
        color: Color,
        line_width: f32,
    ) {
        let dx = tip_x - from_x;
        let dy = tip_y - from_y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 0.01 {
            return;
        }
        let ux = dx / len;
        let uy = dy / len;
        let arrow_len = 12.0;
        let arrow_half_w = 5.0;

        let base_x = tip_x - ux * arrow_len;
        let base_y = tip_y - uy * arrow_len;
        let perp_x = -uy * arrow_half_w;
        let perp_y = ux * arrow_half_w;

        cmds.push(RenderCommand::Line {
            x1: tip_x,
            y1: tip_y,
            x2: base_x + perp_x,
            y2: base_y + perp_y,
            color,
            width: line_width,
        });
        cmds.push(RenderCommand::Line {
            x1: tip_x,
            y1: tip_y,
            x2: base_x - perp_x,
            y2: base_y - perp_y,
            color,
            width: line_width,
        });
    }

    // ========================================================================
    // Rendering: properties panel
    // ========================================================================

    fn render_properties_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let px = self.window_w - PROPERTIES_WIDTH;
        let py = TOOLBAR_HEIGHT;
        let ph = self.window_h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: PROPERTIES_WIDTH,
            height: ph,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: px,
            y1: py,
            x2: px,
            y2: py + ph,
            color: SURFACE0,
            width: 1.0,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: px + 12.0,
            y: py + 16.0,
            text: String::from("Properties"),
            color: TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PROPERTIES_WIDTH - 24.0),
        });

        let mut row_y = py + 40.0;

        // Show properties for single selected node.
        if self.selection.nodes.len() == 1 {
            let nid = self.selection.nodes.first().copied().unwrap_or(0);
            if let Some(node) = self.find_node(nid) {
                self.render_property_row(cmds, px, &mut row_y, "Shape", node.shape.label());
                self.render_property_row(cmds, px, &mut row_y, "Label", &node.label);
                self.render_property_row(cmds, px, &mut row_y, "X", &format!("{:.0}", node.x));
                self.render_property_row(cmds, px, &mut row_y, "Y", &format!("{:.0}", node.y));
                self.render_property_row(
                    cmds,
                    px,
                    &mut row_y,
                    "Width",
                    &format!("{:.0}", node.width),
                );
                self.render_property_row(
                    cmds,
                    px,
                    &mut row_y,
                    "Height",
                    &format!("{:.0}", node.height),
                );
                self.render_property_row(
                    cmds,
                    px,
                    &mut row_y,
                    "Border W",
                    &format!("{:.1}", node.border_width),
                );
                self.render_property_row(
                    cmds,
                    px,
                    &mut row_y,
                    "Font Size",
                    &format!("{:.0}", node.font_size),
                );

                // Fill color swatch.
                self.render_color_swatch(cmds, px + 12.0, row_y, "Fill", node.fill_color);
                row_y += 24.0;
                self.render_color_swatch(cmds, px + 12.0, row_y, "Border", node.border_color);
                row_y += 24.0;

                // Group info.
                if let Some(gid) = node.group_id {
                    self.render_property_row(cmds, px, &mut row_y, "Group", &format!("{gid}"));
                }
            }
        } else if self.selection.edges.len() == 1 {
            let eid = self.selection.edges.first().copied().unwrap_or(0);
            if let Some(edge) = self.find_edge(eid) {
                self.render_property_row(cmds, px, &mut row_y, "Kind", edge.kind.label());
                self.render_property_row(cmds, px, &mut row_y, "Label", &edge.label);
                self.render_property_row(cmds, px, &mut row_y, "Style", edge.line_style.label());
                self.render_property_row(
                    cmds,
                    px,
                    &mut row_y,
                    "Width",
                    &format!("{:.1}", edge.line_width),
                );
                self.render_property_row(
                    cmds,
                    px,
                    &mut row_y,
                    "Start Arr",
                    edge.start_arrow.label(),
                );
                self.render_property_row(cmds, px, &mut row_y, "End Arr", edge.end_arrow.label());
                self.render_color_swatch(cmds, px + 12.0, row_y, "Color", edge.color);
                let _ = row_y; // future expansion point
            }
        } else if self.selection.node_count() > 1 {
            self.render_property_row(
                cmds,
                px,
                &mut row_y,
                "Selected",
                &format!("{} nodes", self.selection.node_count()),
            );

            // Alignment buttons.
            row_y += 12.0;
            cmds.push(RenderCommand::Text {
                x: px + 12.0,
                y: row_y,
                text: String::from("Alignment"),
                color: TEXT,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(PROPERTIES_WIDTH - 24.0),
            });
            row_y += 20.0;

            let ops = [
                AlignOp::Left,
                AlignOp::CenterH,
                AlignOp::Right,
                AlignOp::Top,
                AlignOp::CenterV,
                AlignOp::Bottom,
                AlignOp::DistributeH,
                AlignOp::DistributeV,
            ];
            for op in &ops {
                cmds.push(RenderCommand::FillRect {
                    x: px + 12.0,
                    y: row_y,
                    width: PROPERTIES_WIDTH - 24.0,
                    height: 22.0,
                    color: SURFACE0,
                    corner_radii: CornerRadii::all(3.0),
                });
                cmds.push(RenderCommand::Text {
                    x: px + 18.0,
                    y: row_y + 5.0,
                    text: String::from(op.label()),
                    color: TEXT,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(PROPERTIES_WIDTH - 36.0),
                });
                row_y += 26.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: px + 12.0,
                y: row_y,
                text: String::from("No selection"),
                color: OVERLAY0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PROPERTIES_WIDTH - 24.0),
            });
        }
    }

    fn render_property_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        panel_x: f32,
        row_y: &mut f32,
        label: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x: panel_x + 12.0,
            y: *row_y,
            text: String::from(label),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
        cmds.push(RenderCommand::Text {
            x: panel_x + 100.0,
            y: *row_y,
            text: String::from(value),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(PROPERTIES_WIDTH - 112.0),
        });
        *row_y += 20.0;
    }

    fn render_color_swatch(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        label: &str,
        color: Color,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y: y + 2.0,
            text: String::from(label),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
        });
        cmds.push(RenderCommand::FillRect {
            x: x + 88.0,
            y,
            width: 18.0,
            height: 18.0,
            color,
            corner_radii: CornerRadii::all(3.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: x + 88.0,
            y,
            width: 18.0,
            height: 18.0,
            color: SURFACE2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
    }

    // ========================================================================
    // Rendering: status bar
    // ========================================================================

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let sy = self.window_h - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: sy,
            width: self.window_w,
            height: STATUS_BAR_HEIGHT,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: sy,
            x2: self.window_w,
            y2: sy,
            color: SURFACE0,
            width: 1.0,
        });

        // Node / edge counts.
        let info = format!(
            "Nodes: {}  Edges: {}  Layers: {}  Zoom: {}",
            self.nodes.len(),
            self.edges.len(),
            self.layers.len(),
            self.zoom_percent_str(),
        );
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: sy + 6.0,
            text: info,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(self.window_w - 24.0),
        });

        // Mode indicator.
        let mode_str = match self.mode {
            InteractionMode::Select => "Mode: Select",
            InteractionMode::RectSelect => "Mode: Rect Select",
            InteractionMode::AddNode(shape) => {
                // Use a static label lookup to avoid returning a temp borrow.
                match shape {
                    NodeShape::Rectangle => "Mode: Add Rectangle",
                    NodeShape::RoundedRectangle => "Mode: Add Rounded Rect",
                    NodeShape::Diamond => "Mode: Add Diamond",
                    NodeShape::Circle => "Mode: Add Circle",
                    NodeShape::Ellipse => "Mode: Add Ellipse",
                    NodeShape::Parallelogram => "Mode: Add Parallelogram",
                    NodeShape::Hexagon => "Mode: Add Hexagon",
                    NodeShape::Triangle => "Mode: Add Triangle",
                    NodeShape::Cylinder => "Mode: Add Cylinder",
                    NodeShape::Cloud => "Mode: Add Cloud",
                }
            }
            InteractionMode::AddEdge => "Mode: Add Edge",
            InteractionMode::Pan => "Mode: Pan",
        };
        cmds.push(RenderCommand::Text {
            x: self.window_w - 200.0,
            y: sy + 6.0,
            text: String::from(mode_str),
            color: LAVENDER,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
        });
    }

    // ========================================================================
    // Keyboard shortcut reference
    // ========================================================================

    /// Return a list of keyboard shortcuts.
    pub fn shortcuts_list() -> &'static [(&'static str, &'static str)] {
        &[
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Y", "Redo"),
            ("Ctrl+C", "Copy"),
            ("Ctrl+V", "Paste"),
            ("Ctrl+D", "Duplicate"),
            ("Delete", "Delete selected"),
            ("Ctrl+G", "Group"),
            ("Ctrl+Shift+G", "Ungroup"),
            ("Ctrl+A", "Select all"),
            ("+/-", "Zoom in/out"),
            ("Ctrl+0", "Reset zoom"),
            ("G", "Toggle grid"),
            ("S", "Toggle snap"),
            ("Escape", "Deselect / cancel"),
        ]
    }
}

// ============================================================================
// Utility functions
// ============================================================================

/// Convert a Color to an SVG hex string like "#RRGGBB".
fn color_to_svg_hex(c: Color) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

/// Escape XML special characters.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Escape JSON special characters in a string value.
fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = DiagramApp::new(1280.0, 800.0);

    // Load a sample template.
    app.load_template(DiagramTemplate::Flowchart);

    // Render one frame to verify everything works.
    let cmds = app.render();
    let _ = cmds.len();

    // Test zoom.
    app.zoom_in();
    let _ = app.zoom_percent_str();

    // Test export.
    let _svg = app.export_svg();
    let _json = app.export_json();

    // Test shortcuts reference.
    let _ = DiagramApp::shortcuts_list();

    // In a real OS environment, we would enter the event loop here:
    // loop {
    //     let event = wait_for_event();
    //     match event { ... }
    //     let cmds = app.render();
    //     submit_render_commands(cmds);
    // }

    let _ = app.should_quit;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    // ---- IdGen tests -------------------------------------------------------

    #[test]
    fn test_id_gen_monotonic() {
        let mut g = IdGen::new(1);
        assert_eq!(g.next_id(), 1);
        assert_eq!(g.next_id(), 2);
        assert_eq!(g.next_id(), 3);
    }

    #[test]
    fn test_id_gen_saturating() {
        let mut g = IdGen::new(u64::MAX);
        assert_eq!(g.next_id(), u64::MAX);
        assert_eq!(g.next_id(), u64::MAX);
    }

    // ---- NodeShape tests ---------------------------------------------------

    #[test]
    fn test_node_shape_labels() {
        assert_eq!(NodeShape::Rectangle.label(), "Rectangle");
        assert_eq!(NodeShape::RoundedRectangle.label(), "Rounded Rect");
        assert_eq!(NodeShape::Diamond.label(), "Diamond");
        assert_eq!(NodeShape::Circle.label(), "Circle");
        assert_eq!(NodeShape::Ellipse.label(), "Ellipse");
        assert_eq!(NodeShape::Parallelogram.label(), "Parallelogram");
        assert_eq!(NodeShape::Hexagon.label(), "Hexagon");
        assert_eq!(NodeShape::Triangle.label(), "Triangle");
        assert_eq!(NodeShape::Cylinder.label(), "Cylinder");
        assert_eq!(NodeShape::Cloud.label(), "Cloud");
    }

    #[test]
    fn test_node_shape_all() {
        let all = NodeShape::all();
        assert_eq!(all.len(), 10);
        assert_eq!(all[0], NodeShape::Rectangle);
        assert_eq!(all[9], NodeShape::Cloud);
    }

    #[test]
    fn test_node_shape_accent_colors_unique() {
        let all = NodeShape::all();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a.accent_color(), b.accent_color());
                }
            }
        }
    }

    // ---- EdgeKind tests ----------------------------------------------------

    #[test]
    fn test_edge_kind_labels() {
        assert_eq!(EdgeKind::Straight.label(), "Straight");
        assert_eq!(EdgeKind::Bezier.label(), "Bezier");
        assert_eq!(EdgeKind::Orthogonal.label(), "Orthogonal");
    }

    #[test]
    fn test_edge_kind_all() {
        assert_eq!(EdgeKind::all().len(), 3);
    }

    // ---- ArrowHead tests ---------------------------------------------------

    #[test]
    fn test_arrow_head_labels() {
        assert_eq!(ArrowHead::None.label(), "None");
        assert_eq!(ArrowHead::Filled.label(), "Filled");
        assert_eq!(ArrowHead::Open.label(), "Open");
        assert_eq!(ArrowHead::Diamond.label(), "Diamond");
    }

    #[test]
    fn test_arrow_head_all() {
        assert_eq!(ArrowHead::all().len(), 4);
    }

    // ---- LineStyle tests ---------------------------------------------------

    #[test]
    fn test_line_style_labels() {
        assert_eq!(LineStyle::Solid.label(), "Solid");
        assert_eq!(LineStyle::Dashed.label(), "Dashed");
        assert_eq!(LineStyle::Dotted.label(), "Dotted");
    }

    #[test]
    fn test_line_style_all() {
        assert_eq!(LineStyle::all().len(), 3);
    }

    // ---- DiagramTemplate tests ---------------------------------------------

    #[test]
    fn test_template_labels() {
        assert_eq!(DiagramTemplate::Blank.label(), "Blank");
        assert_eq!(DiagramTemplate::Flowchart.label(), "Flowchart");
        assert_eq!(DiagramTemplate::OrgChart.label(), "Org Chart");
        assert_eq!(DiagramTemplate::UmlClass.label(), "UML Class");
        assert_eq!(DiagramTemplate::NetworkDiagram.label(), "Network");
        assert_eq!(DiagramTemplate::MindMap.label(), "Mind Map");
        assert_eq!(DiagramTemplate::ErDiagram.label(), "ER Diagram");
    }

    #[test]
    fn test_template_all() {
        assert_eq!(DiagramTemplate::all().len(), 7);
    }

    // ---- LayoutDirection tests ---------------------------------------------

    #[test]
    fn test_layout_direction_labels() {
        assert_eq!(LayoutDirection::TopDown.label(), "Top-Down");
        assert_eq!(LayoutDirection::LeftRight.label(), "Left-Right");
    }

    // ---- AlignOp tests -----------------------------------------------------

    #[test]
    fn test_align_op_labels() {
        assert_eq!(AlignOp::Left.label(), "Align Left");
        assert_eq!(AlignOp::Right.label(), "Align Right");
        assert_eq!(AlignOp::CenterH.label(), "Center H");
        assert_eq!(AlignOp::Top.label(), "Align Top");
        assert_eq!(AlignOp::Bottom.label(), "Align Bottom");
        assert_eq!(AlignOp::CenterV.label(), "Center V");
        assert_eq!(AlignOp::DistributeH.label(), "Distribute H");
        assert_eq!(AlignOp::DistributeV.label(), "Distribute V");
    }

    // ---- DiagramNode tests -------------------------------------------------

    #[test]
    fn test_node_new_defaults() {
        let node = DiagramNode::new(1, NodeShape::Rectangle, 10.0, 20.0, 100);
        assert_eq!(node.id, 1);
        assert_eq!(node.shape, NodeShape::Rectangle);
        assert_eq!(node.x, 10.0);
        assert_eq!(node.y, 20.0);
        assert_eq!(node.width, DEFAULT_NODE_W);
        assert_eq!(node.height, DEFAULT_NODE_H);
        assert!(node.label.is_empty());
        assert_eq!(node.layer_id, 100);
        assert!(node.group_id.is_none());
    }

    #[test]
    fn test_circle_node_equal_dimensions() {
        let node = DiagramNode::new(2, NodeShape::Circle, 0.0, 0.0, 1);
        assert_eq!(node.width, 80.0);
        assert_eq!(node.height, 80.0);
    }

    #[test]
    fn test_node_center() {
        let node = DiagramNode::new(1, NodeShape::Rectangle, 100.0, 200.0, 1);
        let (cx, cy) = node.center();
        assert!((cx - (100.0 + DEFAULT_NODE_W / 2.0)).abs() < 0.01);
        assert!((cy - (200.0 + DEFAULT_NODE_H / 2.0)).abs() < 0.01);
    }

    #[test]
    fn test_node_hit_test() {
        let node = DiagramNode::new(1, NodeShape::Rectangle, 50.0, 50.0, 1);
        assert!(node.hit_test(60.0, 60.0));
        assert!(node.hit_test(50.0, 50.0));
        assert!(node.hit_test(50.0 + node.width, 50.0 + node.height));
        assert!(!node.hit_test(49.0, 60.0));
        assert!(!node.hit_test(60.0, 49.0));
    }

    #[test]
    fn test_node_connection_point_right() {
        let node = DiagramNode::new(1, NodeShape::Rectangle, 0.0, 0.0, 1);
        let (cx, cy) = node.center();
        let (px, py) = node.connection_point(cx + 1000.0, cy);
        assert!((px - node.width).abs() < 0.1);
        assert!((py - cy).abs() < 0.1);
    }

    // ---- DiagramEdge tests -------------------------------------------------

    #[test]
    fn test_edge_new_defaults() {
        let edge = DiagramEdge::new(10, 1, 2, 100);
        assert_eq!(edge.id, 10);
        assert_eq!(edge.from_node, 1);
        assert_eq!(edge.to_node, 2);
        assert_eq!(edge.kind, EdgeKind::Straight);
        assert_eq!(edge.end_arrow, ArrowHead::Filled);
        assert_eq!(edge.start_arrow, ArrowHead::None);
        assert!(edge.label.is_empty());
    }

    // ---- Layer tests -------------------------------------------------------

    #[test]
    fn test_layer_new() {
        let layer = Layer::new(5, String::from("bg"), 0);
        assert_eq!(layer.id, 5);
        assert_eq!(layer.name, "bg");
        assert!(layer.visible);
        assert_eq!(layer.order, 0);
    }

    // ---- Group tests -------------------------------------------------------

    #[test]
    fn test_group_new() {
        let g = Group::new(42, vec![1, 2, 3]);
        assert_eq!(g.id, 42);
        assert_eq!(g.member_ids, vec![1, 2, 3]);
        assert!(g.name.contains("42"));
    }

    // ---- Selection tests ---------------------------------------------------

    #[test]
    fn test_selection_empty() {
        let s = Selection::default();
        assert!(s.is_empty());
        assert!(!s.has_node(1));
        assert_eq!(s.node_count(), 0);
    }

    #[test]
    fn test_selection_toggle() {
        let mut s = Selection::default();
        s.toggle_node(5);
        assert!(s.has_node(5));
        assert_eq!(s.node_count(), 1);
        s.toggle_node(5);
        assert!(!s.has_node(5));
        assert_eq!(s.node_count(), 0);
    }

    #[test]
    fn test_selection_single_node() {
        let mut s = Selection::default();
        s.toggle_node(1);
        s.toggle_node(2);
        assert_eq!(s.node_count(), 2);
        s.select_single_node(3);
        assert_eq!(s.node_count(), 1);
        assert!(s.has_node(3));
    }

    #[test]
    fn test_selection_single_edge() {
        let mut s = Selection::default();
        s.toggle_node(1);
        s.select_single_edge(10);
        assert!(s.nodes.is_empty());
        assert!(s.has_edge(10));
    }

    // ---- UndoManager tests -------------------------------------------------

    #[test]
    fn test_undo_manager_basic() {
        let mut mgr = UndoManager::new(10);
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());

        let snap1 = DiagramSnapshot {
            nodes: vec![],
            edges: vec![],
            layers: vec![],
            groups: vec![],
        };
        mgr.save(snap1);
        assert!(mgr.can_undo());
    }

    #[test]
    fn test_undo_redo_cycle() {
        let mut mgr = UndoManager::new(10);
        let empty = DiagramSnapshot {
            nodes: vec![],
            edges: vec![],
            layers: vec![],
            groups: vec![],
        };
        mgr.save(empty.clone());
        mgr.save(empty.clone());
        assert_eq!(mgr.undo_count(), 2);

        let current = empty.clone();
        let _prev = mgr.undo(current);
        assert_eq!(mgr.undo_count(), 1);
        assert!(mgr.can_redo());

        let current2 = empty.clone();
        let _next = mgr.redo(current2);
        assert!(!mgr.can_redo());
    }

    #[test]
    fn test_undo_max_steps() {
        let mut mgr = UndoManager::new(3);
        let snap = DiagramSnapshot {
            nodes: vec![],
            edges: vec![],
            layers: vec![],
            groups: vec![],
        };
        for _ in 0..10 {
            mgr.save(snap.clone());
        }
        assert_eq!(mgr.undo_count(), 3);
    }

    // ---- Clipboard tests ---------------------------------------------------

    #[test]
    fn test_clipboard_empty() {
        let cb = Clipboard::default();
        assert!(cb.is_empty());
    }

    // ---- DiagramApp construction -------------------------------------------

    #[test]
    fn test_app_new() {
        let app = DiagramApp::new(1024.0, 768.0);
        assert_eq!(app.window_w, 1024.0);
        assert_eq!(app.window_h, 768.0);
        assert!(app.nodes.is_empty());
        assert!(app.edges.is_empty());
        assert_eq!(app.layers.len(), 1);
        assert_eq!(app.zoom, 1.0);
        assert!(app.snap_to_grid);
        assert!(app.show_grid);
    }

    // ---- Node add / remove -------------------------------------------------

    #[test]
    fn test_add_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 100.0, 200.0);
        assert_eq!(app.nodes.len(), 1);
        assert!(app.find_node(id).is_some());
    }

    #[test]
    fn test_remove_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Circle, 50.0, 50.0);
        app.remove_node(id);
        assert!(app.nodes.is_empty());
    }

    #[test]
    fn test_remove_node_removes_connected_edges() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        app.add_edge(n1, n2);
        assert_eq!(app.edges.len(), 1);
        app.remove_node(n1);
        assert!(app.edges.is_empty());
    }

    // ---- Move / snap -------------------------------------------------------

    #[test]
    fn test_snap_to_grid() {
        let app = DiagramApp::new(800.0, 600.0);
        assert_eq!(app.snap(23.0), 20.0);
        assert_eq!(app.snap(30.0), 40.0);
        assert_eq!(app.snap(10.0), 20.0);
    }

    #[test]
    fn test_move_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.snap_to_grid = false;
        let id = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.move_node(id, 10.0, -5.0);
        let node = app.find_node(id).unwrap();
        assert!((node.x - 110.0).abs() < 0.01);
        assert!((node.y - 95.0).abs() < 0.01);
    }

    // ---- Edge operations ---------------------------------------------------

    #[test]
    fn test_add_edge() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        let eid = app.add_edge(n1, n2);
        assert_eq!(app.edges.len(), 1);
        let edge = app.find_edge(eid).unwrap();
        assert_eq!(edge.from_node, n1);
        assert_eq!(edge.to_node, n2);
    }

    #[test]
    fn test_remove_edge() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        let eid = app.add_edge(n1, n2);
        app.remove_edge(eid);
        assert!(app.edges.is_empty());
    }

    #[test]
    fn test_set_edge_kind() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        let eid = app.add_edge(n1, n2);
        app.set_edge_kind(eid, EdgeKind::Orthogonal);
        assert_eq!(app.find_edge(eid).unwrap().kind, EdgeKind::Orthogonal);
    }

    // ---- Layer operations --------------------------------------------------

    #[test]
    fn test_add_layer() {
        let mut app = DiagramApp::new(800.0, 600.0);
        assert_eq!(app.layers.len(), 1);
        app.add_layer(String::from("Layer 2"));
        assert_eq!(app.layers.len(), 2);
    }

    #[test]
    fn test_remove_layer_keeps_minimum_one() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let only_id = app.layers[0].id;
        app.remove_layer(only_id);
        assert_eq!(app.layers.len(), 1); // Must keep at least one.
    }

    #[test]
    fn test_toggle_layer_visibility() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let lid = app.layers[0].id;
        assert!(app.is_layer_visible(lid));
        app.toggle_layer_visibility(lid);
        assert!(!app.is_layer_visible(lid));
        app.toggle_layer_visibility(lid);
        assert!(app.is_layer_visible(lid));
    }

    // ---- Grouping ----------------------------------------------------------

    #[test]
    fn test_group_requires_two_nodes() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.selection.select_single_node(n1);
        assert!(app.group_selection().is_none());
    }

    #[test]
    fn test_group_and_ungroup() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        app.selection.nodes = vec![n1, n2];
        let gid = app.group_selection().unwrap();
        assert_eq!(app.groups.len(), 1);
        assert!(app.find_node(n1).unwrap().group_id.is_some());

        app.ungroup(n1);
        assert!(app.groups.is_empty());
        assert!(app.find_node(n1).unwrap().group_id.is_none());
        let _ = gid;
    }

    // ---- Zoom / Pan --------------------------------------------------------

    #[test]
    fn test_zoom_in_out() {
        let mut app = DiagramApp::new(800.0, 600.0);
        assert_eq!(app.zoom, 1.0);
        app.zoom_in();
        assert!(app.zoom > 1.0);
        app.zoom_out();
        app.zoom_out();
        assert!(app.zoom < 1.0);
    }

    #[test]
    fn test_zoom_clamp() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.set_zoom(0.01);
        assert_eq!(app.zoom, MIN_ZOOM);
        app.set_zoom(100.0);
        assert_eq!(app.zoom, MAX_ZOOM);
    }

    #[test]
    fn test_pan() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.pan(50.0, -30.0);
        assert_eq!(app.pan_x, 50.0);
        assert_eq!(app.pan_y, -30.0);
        app.reset_pan();
        assert_eq!(app.pan_x, 0.0);
        assert_eq!(app.pan_y, 0.0);
    }

    #[test]
    fn test_zoom_percent_str() {
        let mut app = DiagramApp::new(800.0, 600.0);
        assert_eq!(app.zoom_percent_str(), "100%");
        app.set_zoom(0.5);
        assert_eq!(app.zoom_percent_str(), "50%");
    }

    // ---- Copy / Paste / Duplicate ------------------------------------------

    #[test]
    fn test_copy_paste() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.selection.select_single_node(n1);
        app.copy_selection();
        app.paste();
        assert_eq!(app.nodes.len(), 2);
        // Pasted node should be offset.
        assert!(app.nodes.last().unwrap().x > 100.0);
    }

    #[test]
    fn test_duplicate() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.add_node(NodeShape::Rectangle, 50.0, 50.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 50.0);
        app.selection.select_single_node(n2);
        app.duplicate_selection();
        assert_eq!(app.nodes.len(), 3);
    }

    // ---- Delete selection --------------------------------------------------

    #[test]
    fn test_delete_selection() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        app.add_edge(n1, n2);
        app.selection.nodes = vec![n1, n2];
        app.delete_selection();
        assert!(app.nodes.is_empty());
        assert!(app.edges.is_empty());
    }

    // ---- Templates ---------------------------------------------------------

    #[test]
    fn test_load_flowchart_template() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.load_template(DiagramTemplate::Flowchart);
        assert!(!app.nodes.is_empty());
        assert!(!app.edges.is_empty());
        assert_eq!(app.current_template, DiagramTemplate::Flowchart);
    }

    #[test]
    fn test_load_blank_template() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.load_template(DiagramTemplate::Blank);
        assert!(app.nodes.is_empty());
    }

    #[test]
    fn test_all_templates_load() {
        for tmpl in DiagramTemplate::all() {
            let mut app = DiagramApp::new(800.0, 600.0);
            app.load_template(*tmpl);
            // Should not panic.
        }
    }

    // ---- Auto layout -------------------------------------------------------

    #[test]
    fn test_auto_layout_top_down() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.add_node(NodeShape::Rectangle, 500.0, 500.0);
        app.add_node(NodeShape::Rectangle, 500.0, 500.0);
        app.auto_layout(LayoutDirection::TopDown);
        // Nodes should now be at different positions.
        assert_ne!(app.nodes[0].x, app.nodes[1].x);
    }

    #[test]
    fn test_auto_layout_left_right() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.auto_layout(LayoutDirection::LeftRight);
        // Should not panic and nodes should be repositioned.
        assert!(app.nodes[0].x >= 0.0);
    }

    // ---- Export SVG --------------------------------------------------------

    #[test]
    fn test_export_svg_empty() {
        let app = DiagramApp::new(800.0, 600.0);
        let svg = app.export_svg();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn test_export_svg_with_nodes() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.load_template(DiagramTemplate::Flowchart);
        let svg = app.export_svg();
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<text"));
        assert!(svg.contains("<line"));
    }

    // ---- Export JSON -------------------------------------------------------

    #[test]
    fn test_export_json_empty() {
        let app = DiagramApp::new(800.0, 600.0);
        let json = app.export_json();
        assert!(json.contains("\"nodes\": ["));
        assert!(json.contains("\"edges\": ["));
    }

    #[test]
    fn test_export_json_with_content() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.load_template(DiagramTemplate::OrgChart);
        let json = app.export_json();
        assert!(json.contains("\"id\":"));
        assert!(json.contains("\"label\":"));
    }

    // ---- Rendering ---------------------------------------------------------

    #[test]
    fn test_render_empty_app() {
        let app = DiagramApp::new(1280.0, 800.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_content() {
        let mut app = DiagramApp::new(1280.0, 800.0);
        app.load_template(DiagramTemplate::Flowchart);
        let cmds = app.render();
        assert!(cmds.len() > 50); // Should have many commands for toolbar + palette + nodes + edges.
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = DiagramApp::new(1280.0, 800.0);
        let n1 = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.selection.select_single_node(n1);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ---- Coordinate conversion ---------------------------------------------

    #[test]
    fn test_screen_canvas_roundtrip() {
        let app = DiagramApp::new(1280.0, 800.0);
        let (cx, cy) = app.screen_to_canvas(300.0, 200.0);
        let (sx, sy) = app.canvas_to_screen(cx, cy);
        assert!((sx - 300.0).abs() < 0.01);
        assert!((sy - 200.0).abs() < 0.01);
    }

    // ---- Bounding box ------------------------------------------------------

    #[test]
    fn test_bounding_box_empty() {
        let app = DiagramApp::new(800.0, 600.0);
        let (x1, y1, x2, y2) = app.bounding_box();
        assert_eq!((x1, y1, x2, y2), (0.0, 0.0, 100.0, 100.0));
    }

    #[test]
    fn test_bounding_box_with_nodes() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.snap_to_grid = false;
        app.add_node(NodeShape::Rectangle, 10.0, 20.0);
        app.add_node(NodeShape::Rectangle, 300.0, 400.0);
        let (min_x, min_y, max_x, max_y) = app.bounding_box();
        assert!((min_x - 10.0).abs() < 0.01);
        assert!((min_y - 20.0).abs() < 0.01);
        assert!(max_x > 300.0);
        assert!(max_y > 400.0);
    }

    // ---- Undo / Redo integration -------------------------------------------

    #[test]
    fn test_undo_add_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        assert!(app.nodes.is_empty());
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        assert_eq!(app.nodes.len(), 1);
        app.undo();
        assert!(app.nodes.is_empty());
    }

    #[test]
    fn test_redo_add_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.undo();
        assert!(app.nodes.is_empty());
        app.redo();
        assert_eq!(app.nodes.len(), 1);
    }

    // ---- Node property setters ---------------------------------------------

    #[test]
    fn test_set_node_label() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.set_node_label(id, String::from("Hello"));
        assert_eq!(app.find_node(id).unwrap().label, "Hello");
    }

    #[test]
    fn test_set_node_fill() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.set_node_fill(id, RED);
        assert_eq!(app.find_node(id).unwrap().fill_color, RED);
    }

    #[test]
    fn test_resize_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.resize_node(id, 200.0, 100.0);
        let n = app.find_node(id).unwrap();
        assert_eq!(n.width, 200.0);
        assert_eq!(n.height, 100.0);
    }

    #[test]
    fn test_resize_node_min_clamp() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.resize_node(id, 5.0, 5.0);
        let n = app.find_node(id).unwrap();
        assert_eq!(n.width, 20.0);
        assert_eq!(n.height, 20.0);
    }

    // ---- Edge property setters ---------------------------------------------

    #[test]
    fn test_set_edge_line_style() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        let eid = app.add_edge(n1, n2);
        app.set_edge_line_style(eid, LineStyle::Dashed);
        assert_eq!(app.find_edge(eid).unwrap().line_style, LineStyle::Dashed);
    }

    #[test]
    fn test_set_edge_arrows() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let n1 = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 0.0);
        let eid = app.add_edge(n1, n2);
        app.set_edge_arrows(eid, ArrowHead::Open, ArrowHead::Diamond);
        let e = app.find_edge(eid).unwrap();
        assert_eq!(e.start_arrow, ArrowHead::Open);
        assert_eq!(e.end_arrow, ArrowHead::Diamond);
    }

    // ---- Utility functions -------------------------------------------------

    #[test]
    fn test_color_to_svg_hex() {
        let c = Color::rgb(255, 128, 0);
        assert_eq!(color_to_svg_hex(c), "#FF8000");
    }

    #[test]
    fn test_escape_xml() {
        assert_eq!(escape_xml("a<b>c&d\"e"), "a&lt;b&gt;c&amp;d&quot;e");
    }

    #[test]
    fn test_escape_json() {
        assert_eq!(escape_json("he said \"hi\""), "he said \\\"hi\\\"");
        assert_eq!(escape_json("line\nbreak"), "line\\nbreak");
    }

    // ---- Alignment operations on app ---------------------------------------

    #[test]
    fn test_align_left() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.snap_to_grid = false;
        let n1 = app.add_node(NodeShape::Rectangle, 50.0, 100.0);
        let n2 = app.add_node(NodeShape::Rectangle, 200.0, 100.0);
        app.selection.nodes = vec![n1, n2];
        app.align_selection(AlignOp::Left);
        assert_eq!(app.find_node(n1).unwrap().x, app.find_node(n2).unwrap().x);
    }

    #[test]
    fn test_align_top() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.snap_to_grid = false;
        let n1 = app.add_node(NodeShape::Rectangle, 50.0, 30.0);
        let n2 = app.add_node(NodeShape::Rectangle, 50.0, 200.0);
        app.selection.nodes = vec![n1, n2];
        app.align_selection(AlignOp::Top);
        assert_eq!(app.find_node(n1).unwrap().y, app.find_node(n2).unwrap().y);
    }

    // ---- Grid size ---------------------------------------------------------

    #[test]
    fn test_set_grid_size() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.set_grid_size(50.0);
        assert_eq!(app.grid_size, 50.0);
        app.set_grid_size(1.0);
        assert_eq!(app.grid_size, 5.0);
        app.set_grid_size(999.0);
        assert_eq!(app.grid_size, 100.0);
    }

    // ---- Node at hit test --------------------------------------------------

    #[test]
    fn test_node_at() {
        let mut app = DiagramApp::new(800.0, 600.0);
        app.snap_to_grid = false;
        let n1 = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        assert_eq!(app.node_at(110.0, 110.0), Some(n1));
        assert_eq!(app.node_at(0.0, 0.0), None);
    }

    // ---- Shortcuts list ----------------------------------------------------

    #[test]
    fn test_shortcuts_list() {
        let list = DiagramApp::shortcuts_list();
        assert!(list.len() >= 10);
        assert_eq!(list[0].0, "Ctrl+Z");
    }

    // ---- Move layer --------------------------------------------------------

    #[test]
    fn test_move_layer() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let l2 = app.add_layer(String::from("Layer 2"));
        assert_eq!(app.layers.len(), 2);
        let l1_id = app.layers[0].id;
        app.move_layer_down(l1_id);
        assert_eq!(app.layers[1].id, l1_id);
        app.move_layer_up(l1_id);
        assert_eq!(app.layers[0].id, l1_id);
        let _ = l2;
    }
}
