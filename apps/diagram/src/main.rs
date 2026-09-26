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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Event, EventResult, Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::frame::Rect;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;
use unsaved::{Choice, Question};

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

// Pink, flamingo and rosewater are palette roles now, not constants.
// They survived this crate's conversion for one reason: the shared
// palette had no rung for them, which was true of five applications at
// once and so was the palette's gap rather than this file's.

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
/// Where the status bar's note on the last save, open or export begins,
/// clear of the counts before it.
const STATUS_NOTE_X: f32 = 320.0;
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
    pub fn accent_color(self, pal: &Palette) -> Color {
        match self {
            Self::Rectangle => pal.blue,
            Self::RoundedRectangle => pal.teal,
            Self::Diamond => pal.yellow,
            Self::Circle => pal.green,
            Self::Ellipse => pal.lavender,
            Self::Parallelogram => pal.peach,
            Self::Hexagon => pal.mauve,
            Self::Triangle => pal.red,
            Self::Cylinder => pal.sky,
            Self::Cloud => pal.pink,
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
            // Content, not chrome: a node's fill and border are the
            // user's per-node choice and are edited and saved with the
            // diagram, so they do not follow the desktop theme.
            fill_color: Color::from_hex(0x313244),
            border_color: Color::from_hex(0x89B4FA),
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
            // Content, as for a node: an edge keeps its own colour.
            color: Color::from_hex(0xCDD6F4),
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

/// The thing a typed label is going onto.
///
/// An enum rather than two `Option`s: both set at once is a state with no
/// meaning, and a label has to land on exactly one thing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LabelTarget {
    /// A node's label.
    Node(NodeId),
    /// An edge's label.
    Edge(EdgeId),
}

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

    /// Whether there is anything to undo.
    ///
    /// This was `#[cfg(test)]` until the app grew a keyboard: a predicate the
    /// UI needs in order to answer "nothing happened" is not a test helper.
    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// Whether there is anything to redo.
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

/// Every key this program answers, and what it does.
///
/// **Each row is a key this program actually answers**, checked by
/// `every_advertised_key_does_something`, which reads each label with
/// `guitk::shortcut` and presses every key it names.
const SHORTCUTS: &[(&str, &str)] = &[
    ("V / A / H", "Select / draw an arrow / move the canvas"),
    ("R / E / D", "Add a rectangle / ellipse / diamond"),
    ("F2 / Enter", "Name the selected box or arrow"),
    ("Delete / Backspace", "Delete what is selected"),
    ("G", "Show or hide the grid"),
    ("S", "Snap to the grid, or not"),
    ("P", "Show or hide the properties panel"),
    ("= / -", "Zoom in / out"),
    ("Ctrl+0", "Back to actual size"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as a new file"),
    ("Ctrl+O", "Open a diagram"),
    ("Ctrl+E", "Export as SVG, or JSON"),
    ("Escape", "Back to the Select tool"),
    ("F1 / ?", "This list"),
];

/// The diagram editor application.
#[derive(Debug)]

pub struct DiagramApp {
    /// The save picker. Holds the dialog and the routing thirteen
    /// applications used to write out by hand.
    pub picker: FilePicker,
    /// What the last save did, for the status line.
    pub last_save: Option<String>,
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
    /// What is being relabelled, and what has been typed.
    ///
    /// This program could not label anything: `set_node_label` and
    /// `set_edge_label` were written and callerless, and there were zero
    /// `key.text` sites in the crate, so every box said what its template
    /// said -- "Process", "Decision?", "CEO" -- permanently. A user's diagram
    /// of their own system was always a diagram of ours.
    pub editing: Option<(LabelTarget, String)>,
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
    /// Where the current drag started, in canvas coordinates.
    ///
    /// `None` when nothing is being dragged. It is canvas rather than screen
    /// coordinates so that a drag stays correct across a zoom or a pan that
    /// happens mid-drag.
    pub drag_from: Option<(f32, f32)>,
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
    /// The file this diagram was opened from or last saved to: where Ctrl+S
    /// writes without asking. `None` for one never saved.
    pub document_path: Option<std::path::PathBuf>,
    /// Whether the diagram has changed since it was opened, saved or begun.
    /// Set by [`save_undo`](Self::save_undo), which every change calls first,
    /// and by undo and redo.
    pub dirty: bool,
    /// What the file picker is up for.
    pub picker_for: PickerFor,
    /// "Unsaved changes -- save them?", while it is asked, and what it holds
    /// up (`apps/unsaved`).
    question: Option<Question<Pending>>,
    /// Set once the window may close; the next answer to the loop is `Exit`.
    pub quit: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// Whether the shortcut list is up.
    show_help: bool,
}

/// What the window says about what it cannot do.
///
/// Nothing in this crate is invented, which is why the fixture scanner
/// never looked at it. This is the other half of the same discipline,
/// found by `scripts/find-silent-incapacity.py`: a program that reaches
/// nothing outside its own process and never says so.
/// What the file picker is up for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerFor {
    /// A diagram to open in place of this one.
    Open,
    /// Where to save this diagram, which then belongs to that file.
    Save,
    /// Where to export an SVG or a JSON: not a save, since neither is read
    /// back.
    Export,
    /// Where to save a diagram with no file yet, before what the
    /// unsaved-changes question held up goes on.
    SaveThen(Pending),
}

/// What the unsaved-changes question is holding up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    /// Opening another diagram in its place.
    Open,
    /// Closing the window.
    Close,
}

/// The version of the diagram file this writes, and the newest it reads.
const DIAGRAM_FORMAT: i64 = 1;

/// The largest diagram file this will open. A diagram cut short would be
/// read as a smaller diagram with no sign anything was missing, so a larger
/// file is refused rather than read in part.
const MAX_DIAGRAM_BYTES: usize = 32 * 1024 * 1024;

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
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::new(),
            last_save: None,
            window_w,
            window_h,
            nodes: Vec::new(),
            edges: Vec::new(),
            layers,
            groups: Vec::new(),
            selection: Selection::default(),
            editing: None,
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
            drag_from: None,
            clipboard: Clipboard::default(),
            show_properties: true,
            show_help: false,
            current_template: DiagramTemplate::Blank,
            rect_select_start: None,
            rect_select_end: None,
            edge_source: None,
            document_path: None,
            dirty: false,
            picker_for: PickerFor::Export,
            question: None,
            quit: false,
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

    /// Remember the diagram before a change, so it can be undone -- and note
    /// that it has changed since it was saved. Every change calls this first.
    fn save_undo(&mut self) {
        let snap = self.snapshot();
        self.undo.save(snap);
        self.dirty = true;
    }

    /// Undo the last change.
    pub fn undo(&mut self) {
        let current = self.snapshot();
        if let Some(prev) = self.undo.undo(current) {
            self.restore_snapshot(prev);
            // Undoing past a save leaves a diagram the file does not hold.
            self.dirty = true;
        }
    }

    /// Redo a previously undone change.
    pub fn redo(&mut self) {
        let current = self.snapshot();
        if let Some(next) = self.undo.redo(current) {
            self.restore_snapshot(next);
            self.dirty = true;
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
    /// Begin labelling the one selected node or edge.
    ///
    /// Exactly one: a label typed once cannot sensibly land on three boxes,
    /// and picking one of them silently would be a guess.
    pub fn begin_labelling(&mut self) -> EventResult {
        let target = match (
            self.selection.nodes.as_slice(),
            self.selection.edges.as_slice(),
        ) {
            ([id], []) => LabelTarget::Node(*id),
            ([], [id]) => LabelTarget::Edge(*id),
            _ => return EventResult::Ignored,
        };
        // Seeded with what it says, because relabelling is usually an edit --
        // and unlike `apps/slides` these are not prompts: "Process" is what
        // the template meant, so a user renaming it to "Process payment"
        // should not retype the word.
        let existing = match target {
            LabelTarget::Node(id) => self
                .nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.label.clone()),
            LabelTarget::Edge(id) => self
                .edges
                .iter()
                .find(|e| e.id == id)
                .map(|e| e.label.clone()),
        };
        let Some(existing) = existing else {
            return EventResult::Ignored;
        };
        self.editing = Some((target, existing));
        EventResult::Consumed
    }

    /// Keys while a label is being typed.
    fn handle_label_key(&mut self, key: &KeyEvent) -> EventResult {
        let Some((target, mut buf)) = self.editing.clone() else {
            return EventResult::Ignored;
        };
        match key.key {
            // Both keys keep the label: losing the typing because the exit
            // key was the cancelling one is the worst thing an editor can do.
            Key::Escape | Key::Enter => {
                match target {
                    LabelTarget::Node(id) => self.set_node_label(id, buf),
                    LabelTarget::Edge(id) => self.set_edge_label(id, buf),
                }
                self.editing = None;
                EventResult::Consumed
            }
            Key::Backspace => {
                buf.pop();
                self.editing = Some((target, buf));
                EventResult::Consumed
            }
            _ => {
                if key.text.is_empty() || key.modifiers.ctrl {
                    return EventResult::Ignored;
                }
                buf.push_str(&key.text);
                self.editing = Some((target, buf));
                EventResult::Consumed
            }
        }
    }

    pub fn set_node_label(&mut self, id: NodeId, label: String) {
        // Naming a box and leaving the name as it was is no change: no undo
        // step, and nothing to ask about when the window closes.
        if self.find_node(id).is_some_and(|n| n.label == label) {
            return;
        }
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
        if self.find_edge(id).is_some_and(|e| e.label == label) {
            return;
        }
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
                let text_color = color_to_svg_hex(self.palette.text);
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

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Route a compositor event into the app.
    /// Put the save picker up.
    ///
    /// `export_svg` and `export_json` were both written, both tested, and
    /// neither could be called.
    ///
    /// **There is no importer, and the banner says so rather than implying
    /// one.** An export you cannot read back is not a backup. It is still
    /// worth having here, for a reason specific to these two formats: an SVG
    /// opens in anything -- a browser, a viewer, a document -- so the drawing
    /// survives in a form the user can actually use, and the JSON keeps the
    /// shapes for an importer that does not exist yet. What would be wrong is
    /// letting either look like a save the program could reload.
    pub fn open_save_dialog(&mut self) {
        self.picker_for = PickerFor::Export;
        self.picker.open_to_write(self.file_stem() + ".svg");
    }

    /// The diagram's name: its file's, or "Untitled".
    pub fn document_name(&self) -> String {
        self.document_path
            .as_deref()
            .and_then(std::path::Path::file_name)
            // The window bar's label only; the real name is the path.
            .map_or_else(
                || String::from("Untitled"),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// The name offered for an export or a first save, without extension.
    fn file_stem(&self) -> String {
        self.document_path
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .map_or_else(
                || String::from("diagram"),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// The diagram as a document: every box, arrow, layer and group, with
    /// every property the editor lets a user set.
    ///
    /// Not the JSON export, which keeps the shapes and drops their colours,
    /// borders, fonts, arrowheads, layers and groups -- a save that loses them
    /// is not a save. YAML, as `apps/slides` keeps a deck, versioned under
    /// `slateos-diagram` so a later format is refused rather than half-read.
    pub fn diagram_document(&self) -> yamldoc::Document {
        let mut doc = yamldoc::Document::new();
        doc.set_i64(&["slateos-diagram"], DIAGRAM_FORMAT);
        let id = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        for (i, layer) in self.layers.iter().enumerate() {
            let k = i.saturating_add(1).to_string();
            let at = |f: &'static str| ["layers", k.as_str(), f];
            doc.set_i64(&at("id"), id(layer.id));
            doc.set_str(&at("name"), &layer.name);
            doc.set_bool(&at("visible"), layer.visible);
            doc.set_i64(&at("order"), i64::try_from(layer.order).unwrap_or(i64::MAX));
        }
        for (i, node) in self.nodes.iter().enumerate() {
            let k = i.saturating_add(1).to_string();
            let at = |f: &'static str| ["nodes", k.as_str(), f];
            doc.set_i64(&at("id"), id(node.id));
            doc.set_str(&at("shape"), node.shape.label());
            doc.set_f64(&at("x"), f64::from(node.x));
            doc.set_f64(&at("y"), f64::from(node.y));
            doc.set_f64(&at("width"), f64::from(node.width));
            doc.set_f64(&at("height"), f64::from(node.height));
            doc.set_str(&at("label"), &node.label);
            doc.set_str(&at("fill"), &colour_hex(node.fill_color));
            doc.set_str(&at("border"), &colour_hex(node.border_color));
            doc.set_f64(&at("border-width"), f64::from(node.border_width));
            doc.set_f64(&at("font-size"), f64::from(node.font_size));
            doc.set_i64(&at("layer"), id(node.layer_id));
            if let Some(group) = node.group_id {
                doc.set_i64(&at("group"), id(group));
            }
        }
        for (i, edge) in self.edges.iter().enumerate() {
            let k = i.saturating_add(1).to_string();
            let at = |f: &'static str| ["edges", k.as_str(), f];
            doc.set_i64(&at("id"), id(edge.id));
            doc.set_i64(&at("from"), id(edge.from_node));
            doc.set_i64(&at("to"), id(edge.to_node));
            doc.set_str(&at("kind"), edge.kind.label());
            doc.set_str(&at("label"), &edge.label);
            doc.set_str(&at("colour"), &colour_hex(edge.color));
            doc.set_str(&at("line"), edge.line_style.label());
            doc.set_f64(&at("width"), f64::from(edge.line_width));
            doc.set_str(&at("start"), edge.start_arrow.label());
            doc.set_str(&at("end"), edge.end_arrow.label());
            doc.set_i64(&at("layer"), id(edge.layer_id));
        }
        for (i, group) in self.groups.iter().enumerate() {
            let k = i.saturating_add(1).to_string();
            let at = |f: &'static str| ["groups", k.as_str(), f];
            doc.set_i64(&at("id"), id(group.id));
            doc.set_str(&at("name"), &group.name);
            let members: Vec<String> = group.member_ids.iter().map(u64::to_string).collect();
            let members: Vec<&str> = members.iter().map(String::as_str).collect();
            doc.set_seq(&at("members"), &members);
        }
        doc
    }

    /// Write the diagram to `path`, which becomes its file. What to say.
    pub fn write_native(&mut self, path: &std::path::Path) -> String {
        match safeio::write_str_atomically(path, &self.diagram_document().to_text()) {
            Ok(()) => {
                self.document_path = Some(path.to_path_buf());
                self.dirty = false;
                format!("Saved {}", path.display())
            }
            Err(err) => format!("Could not save {}: {err}", path.display()),
        }
    }

    /// Replace the diagram with the one in `path`. What to say. A file that
    /// is not a diagram this can read leaves this one as it was.
    pub fn read_native(&mut self, path: &std::path::Path) -> String {
        self.read_native_within(path, MAX_DIAGRAM_BYTES)
    }

    /// [`read_native`](Self::read_native), refusing a file over `max` bytes.
    fn read_native_within(&mut self, path: &std::path::Path, max: usize) -> String {
        let read = match safeio::read_to_string_capped(path, max) {
            Ok(read) => read,
            Err(err) => return format!("Could not open {}: {err}", path.display()),
        };
        if read.truncated {
            return format!(
                "Could not open {}: at {} bytes it is larger than the {max} this reads",
                path.display(),
                read.whole
            );
        }
        match diagram_from_document(&yamldoc::Document::parse(&read.text)) {
            Ok((snapshot, left_out)) => {
                let highest = snapshot
                    .nodes
                    .iter()
                    .map(|n| n.id)
                    .chain(snapshot.edges.iter().map(|e| e.id))
                    .chain(snapshot.layers.iter().map(|l| l.id))
                    .chain(snapshot.groups.iter().map(|g| g.id))
                    .max()
                    .unwrap_or(0);
                self.active_layer_id = snapshot.layers.first().map_or(0, |l| l.id);
                self.restore_snapshot(snapshot);
                // New ids go past every id the file used, so nothing added
                // later can take the name of something already there.
                self.id_gen = IdGen::new(highest.saturating_add(1));
                self.undo = UndoManager::new(MAX_UNDO);
                self.selection.clear();
                self.editing = None;
                self.edge_source = None;
                self.document_path = Some(path.to_path_buf());
                self.dirty = false;
                // Said, not hidden: saving now would write the diagram
                // without what was left out.
                if left_out == 0 {
                    format!("Opened {}", path.display())
                } else {
                    format!(
                        "Opened {}, leaving out {left_out} thing(s) this version cannot read",
                        path.display()
                    )
                }
            }
            Err(why) => format!("Could not open {}: {why}", path.display()),
        }
    }

    /// Ctrl+S: over the diagram's own file, or ask where when it has none.
    pub fn save(&mut self) {
        match self.document_path.clone() {
            Some(path) => self.last_save = Some(self.write_native(&path)),
            None => self.ask_where_to_save(PickerFor::Save),
        }
    }

    /// Put the save picker up, for `purpose`, beside the diagram's own file
    /// when it has one.
    fn ask_where_to_save(&mut self, purpose: PickerFor) {
        let start = self
            .document_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(FilePicker::default_start, std::path::Path::to_path_buf);
        let name = self.file_stem() + ".diagram";
        self.picker_for = purpose;
        self.picker.put_up(
            guitk::dialog::FileDialog::save()
                .with_initial_path(start)
                .with_filename(name),
            true,
        );
    }

    /// The picker chose `path`: do what it was put up for.
    fn picked(&mut self, path: &std::path::Path) {
        let said = match self.picker_for {
            PickerFor::Open => self.read_native(path),
            PickerFor::Save => self.write_native(path),
            PickerFor::Export => self.write_diagram(path),
            PickerFor::SaveThen(pending) => {
                let said = self.write_native(path);
                if !self.dirty {
                    self.go_on(pending);
                }
                said
            }
        };
        self.last_save = Some(said);
    }

    /// Before something replaces or closes the diagram: ask about unsaved
    /// changes, or with none go straight on.
    pub fn unless_unsaved(&mut self, pending: Pending) {
        if !self.dirty {
            self.go_on(pending);
            return;
        }
        let prompt = match pending {
            Pending::Open => "Save it before opening another?",
            Pending::Close => "Save it before closing?",
        };
        let name = self.document_name();
        self.question = Some(Question::new(
            &unsaved::message_for(&[&name]),
            prompt,
            pending,
        ));
    }

    /// Do what the unsaved-changes question held up.
    fn go_on(&mut self, pending: Pending) {
        match pending {
            Pending::Open => {
                self.picker_for = PickerFor::Open;
                self.picker.open_to_read();
            }
            Pending::Close => self.quit = true,
        }
    }

    /// Answer the question put before `pending`. Save goes on only if the
    /// save worked: a diagram that could not be written is still the only
    /// copy.
    fn answer(&mut self, pending: Pending, choice: Choice) {
        match choice {
            Choice::Cancel => {}
            Choice::Discard => self.go_on(pending),
            Choice::Save => match self.document_path.clone() {
                Some(path) => {
                    self.last_save = Some(self.write_native(&path));
                    if !self.dirty {
                        self.go_on(pending);
                    }
                }
                None => self.ask_where_to_save(PickerFor::SaveThen(pending)),
            },
        }
    }

    /// The window has been asked to close: whether it may go now. If not,
    /// the question is up.
    fn request_close(&mut self) -> bool {
        // A name being typed is part of the diagram.
        if let Some((target, buf)) = self.editing.take() {
            match target {
                LabelTarget::Node(id) => self.set_node_label(id, buf),
                LabelTarget::Edge(id) => self.set_edge_label(id, buf),
            }
        }
        if !self.dirty {
            return true;
        }
        // The question replaces whatever is up: a picker would take the keys
        // it needs, and be drawn over it.
        self.picker.close();
        self.show_help = false;
        self.unless_unsaved(Pending::Close);
        false
    }

    /// Write the diagram to `path`, in the format the filename asks for.
    ///
    /// The extension decides, because **on the way out there is no content to
    /// inspect and the name the user typed is the only statement of intent
    /// there is**. That is the opposite of the rule for opening a file, where
    /// the extension is a claim by whoever named it and the content is the
    /// fact. Anything that is not `.json` is written as SVG, since that is
    /// what the picker offers and what a viewer can open.
    pub fn write_diagram(&mut self, path: &std::path::Path) -> String {
        if self.nodes.is_empty() && self.edges.is_empty() {
            return String::from("Nothing drawn yet -- nothing to write");
        }
        let json = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("json"));
        let text = if json {
            self.export_json()
        } else {
            self.export_svg()
        };
        let what = if json { "JSON" } else { "SVG" };
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!(
                "Wrote {} node(s) as {what} to {}",
                self.nodes.len(),
                path.display()
            ),
            Err(err) => format!("Could not write {}: {err}", path.display()),
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The unsaved-changes question has every key and click while it is
        // up. Each is a redraw: focus and hover move inside it.
        if let Some(question) = self.question.as_mut()
            && matches!(event, Event::Key(_) | Event::Mouse(_))
        {
            if let Some(choice) = question.handle(event) {
                let pending = question.pending();
                self.question = None;
                self.answer(pending, choice);
            }
            return EventResult::Consumed;
        }
        // The picker takes input first while it is up, or a keystroke meant
        // for a filename reaches the canvas -- where single letters select
        // tools and Delete removes the selected shape.
        match self.picker.handle(event, self.window_w, self.window_h) {
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
                    self.window_w = *width as f32;
                    self.window_h = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a mouse event.
    ///
    /// Screen coordinates arrive here and canvas coordinates leave. Mixing the
    /// two is invisible at the default zoom of 1.0 with no pan, and flings a
    /// node across the diagram at any other — which is why both mouse tests run
    /// at a zoom and a pan that are not the identity.
    fn handle_mouse(&mut self, ev: &MouseEvent) -> EventResult {
        // The toolbar and the shape palette, before the canvas. Both were
        // drawn with an active item highlighted and neither was hit-tested,
        // so a click on a tool button was converted to canvas coordinates
        // and handled as a click on the drawing underneath it.
        if matches!(ev.kind, MouseEventKind::Press(MouseButton::Left)) {
            if (0.0..TOOLBAR_HEIGHT).contains(&ev.y) && ev.x >= 0.0 {
                if let Some((_, mode, _)) = self
                    .tool_buttons()
                    .into_iter()
                    .find(|(rect, _, _)| rect.contains(ev.x, ev.y))
                {
                    return self.set_mode(mode);
                }
                // The band is claimed even between the buttons, so a click on
                // the strip does not fall through to the canvas behind it.
                return EventResult::Consumed;
            }
            if (0.0..PALETTE_WIDTH).contains(&ev.x)
                && (TOOLBAR_HEIGHT..self.window_h - STATUS_BAR_HEIGHT).contains(&ev.y)
            {
                if let Some((_, shape)) = self
                    .shape_buttons()
                    .into_iter()
                    .find(|(rect, _)| rect.contains(ev.x, ev.y))
                {
                    return self.set_mode(InteractionMode::AddNode(shape));
                }
                return EventResult::Consumed;
            }
        }

        let (cx, cy) = self.screen_to_canvas(ev.x, ev.y);
        match ev.kind {
            MouseEventKind::Press(MouseButton::Left) => match self.mode {
                // Place one of this program's ten shapes where the pointer
                // is. Seven of them had no other way in: `R`, `E` and `D`
                // reach three, and the palette that offers the rest was
                // decoration.
                InteractionMode::AddNode(shape) => {
                    let id = self.add_node(shape, cx, cy);
                    self.selection.select_single_node(id);
                    EventResult::Consumed
                }
                // Source, then target. `edge_source` was declared and
                // initialised and read by nothing, so this program could draw
                // boxes and never connect two of them -- in a diagram editor,
                // where the connections are the diagram.
                InteractionMode::AddEdge => {
                    let Some(id) = self.node_at(cx, cy) else {
                        // Clicking away cancels a half-drawn edge rather than
                        // leaving it pending invisibly.
                        self.edge_source = None;
                        return EventResult::Consumed;
                    };
                    match self.edge_source.take() {
                        None => {
                            self.edge_source = Some(id);
                            self.selection.select_single_node(id);
                        }
                        Some(from) if from == id => {
                            // A node joined to itself draws an edge with
                            // nowhere to go. Treated as changing your mind
                            // about the source, which is what a second click
                            // on the same box looks like.
                            self.edge_source = Some(id);
                        }
                        Some(from) => {
                            self.add_edge_reporting(from, id);
                        }
                    }
                    EventResult::Consumed
                }
                InteractionMode::Pan => {
                    self.drag_from = Some((cx, cy));
                    EventResult::Consumed
                }
                InteractionMode::Select => {
                    if let Some(id) = self.node_at(cx, cy) {
                        self.selection.select_single_node(id);
                        self.drag_from = Some((cx, cy));
                    } else {
                        self.selection.clear();
                        self.drag_from = None;
                    }
                    EventResult::Consumed
                }
            },
            MouseEventKind::Move => {
                let Some((fx, fy)) = self.drag_from else {
                    // Not dragging: a bare pointer move changes nothing, and
                    // answering `Consumed` would redraw on every pixel crossed.
                    return EventResult::Ignored;
                };
                if self.mode == InteractionMode::Pan {
                    // The canvas moves under the pointer, so the point that
                    // was grabbed stays under it. `screen_to_canvas` divides
                    // by the zoom, so the delta is already in canvas units and
                    // the pan offset is in screen ones.
                    self.pan_x += (cx - fx) * self.zoom;
                    self.pan_y += (cy - fy) * self.zoom;
                    return EventResult::Consumed;
                }
                let ids: Vec<NodeId> = self.selection.nodes.clone();
                if ids.is_empty() {
                    return EventResult::Ignored;
                }
                for id in ids {
                    self.move_node(id, cx - fx, cy - fy);
                }
                self.drag_from = Some((cx, cy));
                EventResult::Consumed
            }
            MouseEventKind::Release(MouseButton::Left) => {
                if self.drag_from.take().is_none() {
                    return EventResult::Ignored;
                }
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
    /// The app had no input handling at all: every shape, edge, layer, group,
    /// the undo stack and both exporters were reachable only by a caller.
    fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        // Relabelling takes every key while it is up. `Backspace` is
        // bound to *delete the selection* out here, so without this a typo
        // while naming a box would delete the box.
        if self.editing.is_some() {
            return self.handle_label_key(key);
        }

        let ctrl = key.modifiers.ctrl;
        match key.key {
            Key::S if ctrl => {
                if key.modifiers.shift {
                    self.ask_where_to_save(PickerFor::Save);
                } else {
                    self.save();
                }
                EventResult::Consumed
            }
            Key::O if ctrl => {
                self.unless_unsaved(Pending::Open);
                EventResult::Consumed
            }
            // What Ctrl+S did before a diagram could be saved: an SVG or a
            // JSON, neither of them read back.
            Key::E if ctrl => {
                self.open_save_dialog();
                EventResult::Consumed
            }
            Key::Z if ctrl => {
                if !self.undo.can_undo() {
                    return EventResult::Ignored;
                }
                self.undo();
                EventResult::Consumed
            }
            Key::Y if ctrl => {
                if !self.undo.can_redo() {
                    return EventResult::Ignored;
                }
                self.redo();
                EventResult::Consumed
            }
            // Naming the selected box or arrow. F2 is the conventional
            // rename key; Enter is what opens a thing.
            Key::F2 | Key::Enter => self.begin_labelling(),
            Key::Delete | Key::Backspace => self.delete_selection_reporting(),
            // Zoom.
            Key::Equals => {
                self.zoom_in();
                EventResult::Consumed
            }
            Key::Minus => {
                self.zoom_out();
                EventResult::Consumed
            }
            Key::Num0 if ctrl => {
                // A hundred percent, without walking the step table: this is
                // the "show me it at actual size" key.
                if (self.zoom - 1.0).abs() < f32::EPSILON {
                    return EventResult::Ignored;
                }
                self.zoom = 1.0;
                EventResult::Consumed
            }
            // The three tools, on the letters drawing programs have used
            // for thirty years: V for the pointer, A for the arrow being
            // drawn, H for the hand. The toolbar offers the same three and
            // could not be clicked until now.
            Key::V => self.set_mode(InteractionMode::Select),
            Key::A => self.set_mode(InteractionMode::AddEdge),
            Key::H => self.set_mode(InteractionMode::Pan),
            // The canvas.
            Key::G => {
                self.show_grid = !self.show_grid;
                EventResult::Consumed
            }
            Key::S => {
                self.snap_to_grid = !self.snap_to_grid;
                EventResult::Consumed
            }
            // The third member of the group `G` and `S` are in.
            // `show_properties` was `true` at construction with no writer
            // anywhere, so the properties panel was permanent and the canvas
            // beside it had that much less room. Found by
            // `scripts/frozen-flag-survey.py`.
            Key::P => {
                self.show_properties = !self.show_properties;
                EventResult::Consumed
            }
            // The shortcut list. The label editor above returns before this,
            // so neither key can be taken out of somebody's typing.
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
            // Out of whatever tool is up, and out of a half-drawn edge.
            // Escape backs out of the smallest thing first, and a pending
            // source is smaller than the tool that picked it.
            Key::Escape if self.edge_source.is_some() => {
                self.edge_source = None;
                EventResult::Consumed
            }
            Key::Escape if self.mode != InteractionMode::Select => {
                self.set_mode(InteractionMode::Select)
            }
            // New shapes, at the middle of the view so they land somewhere
            // visible rather than at the canvas origin.
            Key::R => self.add_shape_in_view(NodeShape::Rectangle),
            Key::E => self.add_shape_in_view(NodeShape::Ellipse),
            Key::D => self.add_shape_in_view(NodeShape::Diamond),
            _ => EventResult::Ignored,
        }
    }

    /// Delete whatever is selected, reporting whether anything went.
    ///
    /// `delete_selection` above returns nothing, and a key that answers
    /// `Consumed` with an empty selection redraws an unchanged frame.
    /// Join two nodes, and select the edge so the next `F2` names it.
    ///
    /// Selecting it is the whole difference between an edge you can label and
    /// one you have to find again: `begin_labelling` works on the selection,
    /// and an edge drawn on a busy canvas is the hardest thing in this
    /// program to click on afterwards.
    fn add_edge_reporting(&mut self, from: NodeId, to: NodeId) -> EventResult {
        let id = self.add_edge(from, to);
        self.selection.select_single_edge(id);
        EventResult::Consumed
    }

    fn delete_selection_reporting(&mut self) -> EventResult {
        if self.selection.nodes.is_empty() && self.selection.edges.is_empty() {
            return EventResult::Ignored;
        }
        self.delete_selection();
        EventResult::Consumed
    }

    // ========================================================================
    // Where the tool buttons and the shape buttons are
    // ========================================================================

    /// The toolbar's tool buttons: where each is drawn, and what it selects.
    ///
    /// One list, walked by the renderer and by the hit test. They were two
    /// things before -- an array of three labels inside `render_toolbar`, and
    /// no hit test at all, so the buttons were drawn with the active one
    /// highlighted and a click on them was passed to the canvas underneath as
    /// though the toolbar were not there.
    fn tool_buttons(&self) -> [(Rect, InteractionMode, &'static str); 3] {
        let mut bx = 8.0;
        let mut next = || {
            let r = Rect::new(bx, 6.0, 60.0, 28.0);
            bx += 68.0;
            r
        };
        [
            (next(), InteractionMode::Select, "Select"),
            (next(), InteractionMode::AddEdge, "Edge"),
            (next(), InteractionMode::Pan, "Pan"),
        ]
    }

    /// The shape palette's buttons: where each is drawn, and what it adds.
    ///
    /// Same story as the toolbar and worse in degree: ten shapes were drawn
    /// down the sidebar, each highlighted when the mode named it, and the mode
    /// was never set -- so seven of this program's ten shapes could not be
    /// drawn at all and the other three only through `R`, `E` and `D`.
    fn shape_buttons(&self) -> Vec<(Rect, NodeShape)> {
        let mut by = TOOLBAR_HEIGHT + 36.0;
        NodeShape::all()
            .iter()
            .map(|shape| {
                let r = Rect::new(8.0, by, PALETTE_WIDTH - 16.0, SHAPE_BTN_H);
                by += SHAPE_BTN_H + 4.0;
                (r, *shape)
            })
            .collect()
    }

    /// Change tool, forgetting any half-drawn edge.
    ///
    /// A source node picked under the edge tool means nothing under the
    /// others, and leaving it set would connect a node picked minutes ago to
    /// the next one clicked.
    fn set_mode(&mut self, mode: InteractionMode) -> EventResult {
        self.mode = mode;
        self.edge_source = None;
        EventResult::Consumed
    }

    /// Add a shape at the middle of what is currently on screen.
    fn add_shape_in_view(&mut self, shape: NodeShape) -> EventResult {
        let (cx, cy) = self.screen_to_canvas(self.window_w / 2.0, self.window_h / 2.0);
        let id = self.add_node(shape, cx, cy);
        self.selection.select_single_node(id);
        EventResult::Consumed
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    ///
    /// Renders the entire application UI and returns draw commands.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds: Vec<RenderCommand> = Vec::with_capacity(512);

        // Background.
        self.palette.push_surface(
            &mut cmds,
            0.0,
            0.0,
            self.window_w,
            self.window_h,
            0.0,
            Surface::Card,
        );

        self.render_toolbar(&mut cmds);
        self.render_palette(&mut cmds);
        self.render_canvas(&mut cmds);
        if self.show_properties {
            self.render_properties_panel(&mut cmds);
        }
        self.render_status_bar(&mut cmds);

        // Last, so it is above everything.
        cmds.extend(
            self.picker
                .render(&self.palette, self.window_w, self.window_h),
        );

        // And the shortcut list over even that.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut cmds,
                &self.palette,
                (self.window_w, self.window_h),
                0.0,
                SHORTCUTS,
                "F1 or ? closes this",
            );
        }

        cmds
    }

    // ========================================================================
    // Rendering: toolbar
    // ========================================================================

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        // Toolbar background.
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.window_w,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_w,
            y2: TOOLBAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Tool buttons, from the list `tool_buttons` lays out -- so a
        // button drawn here is a button a click can reach.
        let tools = self.tool_buttons();
        for (rect, mode, label) in &tools {
            let active = &self.mode == mode;
            let bg = if active {
                self.palette.blue
            } else {
                self.palette.surface0
            };
            let fg = if active {
                self.palette.crust
            } else {
                self.palette.text
            };
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: bg,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });
            cmds.push(RenderCommand::Text {
                x: rect.x + 8.0,
                y: rect.y + 8.0,
                text: String::from(*label),
                color: fg,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(rect.w - 8.0),
                overflow: TextOverflow::Ellipsis,
            });
        }
        // Where the zoom controls start: past the last tool button, asked of
        // the layout rather than accumulated through the loop. A carried
        // `bx` had to be initialised to a value the loop then overwrote,
        // which clippy called what it was -- a value assigned and never read.
        let bx = tools
            .last()
            .map_or(8.0, |(rect, _, _)| rect.x + rect.w + 8.0);

        // Zoom controls.
        let zoom_text = self.zoom_percent_str();
        cmds.push(RenderCommand::Text {
            x: bx + 20.0,
            y: 14.0,
            text: zoom_text,
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(60.0),
            overflow: TextOverflow::Ellipsis,
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
            color: if self.show_grid {
                self.palette.ink(self.palette.green)
            } else {
                self.palette.subtext0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
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
            color: if self.snap_to_grid {
                self.palette.ink(self.palette.green)
            } else {
                self.palette.subtext0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
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
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(150.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    // ========================================================================
    // Rendering: shape palette sidebar
    // ========================================================================

    fn render_palette(&self, cmds: &mut Vec<RenderCommand>) {
        let pal_y = TOOLBAR_HEIGHT;
        let pal_h = self.window_h - TOOLBAR_HEIGHT - STATUS_BAR_HEIGHT;

        // Background.
        self.palette.push_surface(
            cmds,
            0.0,
            pal_y,
            PALETTE_WIDTH,
            pal_h,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: PALETTE_WIDTH,
            y1: pal_y,
            x2: PALETTE_WIDTH,
            y2: pal_y + pal_h,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Section header: Shapes.
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: pal_y + 16.0,
            text: String::from("Shapes"),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PALETTE_WIDTH - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Shape buttons, from the list `shape_buttons` lays out.
        let mut by = pal_y + 36.0;
        for (rect, shape) in self.shape_buttons() {
            by = rect.y;
            let shape = &shape;
            let is_active = matches!(self.mode, InteractionMode::AddNode(s) if s == *shape);
            let bg = if is_active {
                shape.accent_color(&self.palette)
            } else {
                self.palette.surface0
            };
            let fg = if is_active {
                self.palette.crust
            } else {
                self.palette.text
            };

            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.w,
                height: rect.h,
                color: bg,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });

            // Mini shape icon (a small preview colored square).
            cmds.push(RenderCommand::FillRect {
                x: 14.0,
                y: by + 6.0,
                width: 20.0,
                height: 20.0,
                color: shape.accent_color(&self.palette),
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
                overflow: TextOverflow::Ellipsis,
            });

            by += SHAPE_BTN_H + 4.0;
        }

        // Section header: Layers.
        by += 12.0;
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: by,
            text: String::from("Layers"),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PALETTE_WIDTH - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        by += 20.0;

        for layer in &self.layers {
            let is_active = layer.id == self.active_layer_id;
            let bg = if is_active {
                self.palette.surface1
            } else {
                self.palette.surface0
            };

            cmds.push(RenderCommand::FillRect {
                x: 8.0,
                y: by,
                width: PALETTE_WIDTH - 16.0,
                height: LAYER_ROW_H,
                color: bg,
                corner_radii: CornerRadii::all(PANEL_CORNER),
            });

            // Visibility indicator.
            let vis_color = if layer.visible {
                self.palette.green
            } else {
                self.palette.overlay0
            };
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
                color: if layer.visible {
                    self.palette.text
                } else {
                    self.palette.subtext0
                },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PALETTE_WIDTH - 48.0),
                overflow: TextOverflow::Ellipsis,
            });

            by += LAYER_ROW_H + 2.0;
        }

        // Section header: Templates.
        by += 12.0;
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: by,
            text: String::from("Templates"),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PALETTE_WIDTH - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        by += 20.0;

        for tmpl in DiagramTemplate::all() {
            let is_active = self.current_template == *tmpl;
            let fg = if is_active {
                self.palette.blue
            } else {
                self.palette.subtext0
            };

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
                overflow: TextOverflow::Ellipsis,
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
            color: self.palette.base,
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
                color: self.palette.blue,
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

        let grid_color = self.palette.surface0;
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
                // The buffer while this node is being relabelled: the
                // commit is on the way out, so the node still holds the old
                // word until then.
                text: match &self.editing {
                    Some((LabelTarget::Node(id), buf)) if *id == node.id => buf.clone(),
                    _ => node.label.clone(),
                },
                color: self.palette.text,
                font_size: fs,
                font_weight: FontWeightHint::Regular,
                max_width: Some(w * 0.8),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Selection highlight.
        if selected {
            cmds.push(RenderCommand::StrokeRect {
                x: x - 3.0,
                y: y - 3.0,
                width: w + 6.0,
                height: h + 6.0,
                color: self.palette.blue,
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
        let color = if selected {
            self.palette.blue
        } else {
            edge.color
        };

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
                text: match &self.editing {
                    Some((LabelTarget::Edge(id), buf)) if *id == edge.id => buf.clone(),
                    _ => edge.label.clone(),
                },
                color: self.palette.subtext0,
                font_size: 11.0 * z,
                font_weight: FontWeightHint::Regular,
                max_width: Some(120.0 * z),
                overflow: TextOverflow::Ellipsis,
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
        self.palette.push_surface(
            cmds,
            px,
            py,
            PROPERTIES_WIDTH,
            ph,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: px,
            y1: py,
            x2: px,
            y2: py + ph,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: px + 12.0,
            y: py + 16.0,
            text: String::from("Properties"),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PROPERTIES_WIDTH - 24.0),
            overflow: TextOverflow::Ellipsis,
        });

        let mut row_y = py + 40.0;

        // Show properties for single selected node.
        if self.selection.nodes.len() == 1 {
            let nid = self.selection.nodes.first().copied().unwrap_or(0);
            if let Some(node) = self.find_node(nid) {
                self.render_property_row(cmds, px, &mut row_y, "Shape", node.shape.label());
                // The buffer, not the node, while it is being relabelled. The
                // canvas already shows the typing; a properties panel still
                // reading the old word beside it is the same value disagreeing
                // with itself on one screen.
                let shown_label = match &self.editing {
                    Some((LabelTarget::Node(id), buf)) if *id == node.id => buf.as_str(),
                    _ => node.label.as_str(),
                };
                self.render_property_row(cmds, px, &mut row_y, "Label (F2)", shown_label);
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
                let shown_edge_label = match &self.editing {
                    Some((LabelTarget::Edge(id), buf)) if *id == edge.id => buf.as_str(),
                    _ => edge.label.as_str(),
                };
                self.render_property_row(cmds, px, &mut row_y, "Label (F2)", shown_edge_label);
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
                color: self.palette.text,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: Some(PROPERTIES_WIDTH - 24.0),
                overflow: TextOverflow::Ellipsis,
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
                self.palette.push_surface(
                    cmds,
                    px + 12.0,
                    row_y,
                    PROPERTIES_WIDTH - 24.0,
                    22.0,
                    3.0,
                    Surface::Card,
                );
                cmds.push(RenderCommand::Text {
                    x: px + 18.0,
                    y: row_y + 5.0,
                    text: String::from(op.label()),
                    color: self.palette.text,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(PROPERTIES_WIDTH - 36.0),
                    overflow: TextOverflow::Ellipsis,
                });
                row_y += 26.0;
            }
        } else {
            cmds.push(RenderCommand::Text {
                x: px + 12.0,
                y: row_y,
                text: String::from("No selection"),
                color: self.palette.subtext0,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(PROPERTIES_WIDTH - 24.0),
                overflow: TextOverflow::Ellipsis,
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
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
        });
        cmds.push(RenderCommand::Text {
            x: panel_x + 100.0,
            y: *row_y,
            text: String::from(value),
            color: self.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(PROPERTIES_WIDTH - 112.0),
            overflow: TextOverflow::Ellipsis,
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
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(80.0),
            overflow: TextOverflow::Ellipsis,
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
            color: self.palette.surface2,
            line_width: 1.0,
            corner_radii: CornerRadii::all(3.0),
        });
    }

    // ========================================================================
    // Rendering: status bar
    // ========================================================================

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let sy = self.window_h - STATUS_BAR_HEIGHT;

        self.palette.push_surface(
            cmds,
            0.0,
            sy,
            self.window_w,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: sy,
            x2: self.window_w,
            y2: sy,
            color: self.palette.surface0,
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
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((STATUS_NOTE_X - 20.0).min(self.window_w - 24.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });

        // What the last save, open or export did. It was recorded and drawn
        // nowhere, so a save that failed looked like one that worked.
        if let Some(note) = &self.last_save {
            cmds.push(RenderCommand::Text {
                x: STATUS_NOTE_X,
                y: sy + 6.0,
                text: note.clone(),
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((self.window_w - 208.0 - STATUS_NOTE_X).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }

        // Mode indicator.
        let mode_str = match self.mode {
            InteractionMode::Select => "Mode: Select",
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
            // Labelling is a mode and this is where the program says which
            // mode it is in. Without it, typing a label looks exactly like
            // the app ignoring the keyboard -- and `Backspace` meaning
            // something else in here is worth being told, not discovered.
            text: String::from(if self.editing.is_some() {
                "Mode: Labelling -- Enter or Esc to finish"
            } else {
                mode_str
            }),
            color: self.palette.ink(self.palette.lavender),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(190.0),
            overflow: TextOverflow::Ellipsis,
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
    guitk::escape::xml(s)
}

/// Escape JSON special characters in a string value.
///
/// The previous local version handled only `"`, `\`, `\n`, `\r` and `\t`, and
/// passed every other control character through raw. RFC 8259 forbids an
/// unescaped character below `U+0020` inside a string, so a node label
/// containing one produced an export that no JSON parser would accept.
/// A colour as `#RRGGBB`, or `#RRGGBBAA` when it is not opaque -- the
/// spelling `apps/slides` writes, so the two files read alike.
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

/// The variant of `all` whose label is `text`.
fn by_label<T: Copy>(all: &[T], label: impl Fn(T) -> &'static str, text: &str) -> Option<T> {
    all.iter().copied().find(|v| label(*v) == text)
}

/// An id read back: a non-negative whole number.
fn read_id(doc: &yamldoc::Document, path: &[&str]) -> Option<u64> {
    doc.get_i64(path).and_then(|n| u64::try_from(n).ok())
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

/// Take `id` for one thing in a diagram being read: every box, arrow, layer
/// and group has an id no other has, and a file that says otherwise would be
/// read as some other diagram.
fn claim(seen: &mut std::collections::HashSet<u64>, id: u64) -> Result<u64, String> {
    if seen.insert(id) {
        Ok(id)
    } else {
        Err(format!("two things in it share the id {id}"))
    }
}

/// Read a diagram written by [`DiagramApp::diagram_document`].
///
/// Refuses a file that is not a diagram, one from a later version, and one
/// whose ids are not unique -- each would be read as some other diagram. A
/// box of a shape this does not know is left out, and so is an arrow whose
/// ends are not both in the file: the rest is still the user's diagram. A
/// file with no layers gets one, since every box has to be on one. How many
/// things were left out comes back with the diagram, so the user is told.
fn diagram_from_document(doc: &yamldoc::Document) -> Result<(DiagramSnapshot, usize), String> {
    match doc.get_i64(&["slateos-diagram"]) {
        None => return Err(String::from("it is not a SlateOS diagram")),
        Some(v) if v > DIAGRAM_FORMAT => {
            return Err(format!(
                "it is a later format ({v}) than this version reads ({DIAGRAM_FORMAT})"
            ));
        }
        Some(_) => {}
    }
    let mut seen = std::collections::HashSet::new();

    let mut left_out = 0_usize;
    let mut layers = Vec::new();
    for k in positions(doc, &["layers"]) {
        let at = |f: &'static str| ["layers", k.as_str(), f];
        let Some(id) = read_id(doc, &at("id")) else {
            left_out = left_out.saturating_add(1);
            continue;
        };
        let mut layer = Layer::new(
            claim(&mut seen, id)?,
            doc.get_str(&at("name"))
                .unwrap_or_else(|| format!("Layer {id}")),
            doc.get_i64(&at("order"))
                .and_then(|o| usize::try_from(o).ok())
                .unwrap_or(layers.len()),
        );
        layer.visible = doc.get_bool(&at("visible")).unwrap_or(true);
        layers.push(layer);
    }
    if layers.is_empty() {
        // An id past every id in the file: the boxes, arrows and groups are
        // read after this, and one of them taking the same number would be
        // refused as a clash the file never had.
        let highest = ["nodes", "edges", "groups"]
            .iter()
            .flat_map(|section| {
                positions(doc, &[section])
                    .into_iter()
                    .filter_map(|k| read_id(doc, &[section, k.as_str(), "id"]))
            })
            .max()
            .unwrap_or(0);
        let id = claim(&mut seen, highest.saturating_add(1))?;
        layers.push(Layer::new(id, String::from("Layer 1"), 0));
    }
    let first_layer = layers.first().map_or(0, |l| l.id);
    let layer_or_first = |id: Option<u64>| {
        id.filter(|id| layers.iter().any(|l| l.id == *id))
            .unwrap_or(first_layer)
    };

    let mut nodes = Vec::new();
    for k in positions(doc, &["nodes"]) {
        let at = |f: &'static str| ["nodes", k.as_str(), f];
        let shape = doc
            .get_str(&at("shape"))
            .and_then(|t| by_label(NodeShape::all(), NodeShape::label, &t));
        let (Some(id), Some(shape), Some(x), Some(y)) = (
            read_id(doc, &at("id")),
            shape,
            read_f32(doc, &at("x")),
            read_f32(doc, &at("y")),
        ) else {
            left_out = left_out.saturating_add(1);
            continue;
        };
        let mut node = DiagramNode::new(
            claim(&mut seen, id)?,
            shape,
            x,
            y,
            layer_or_first(read_id(doc, &at("layer"))),
        );
        if let Some(w) = read_f32(doc, &at("width")).filter(|w| *w > 0.0) {
            node.width = w;
        }
        if let Some(h) = read_f32(doc, &at("height")).filter(|h| *h > 0.0) {
            node.height = h;
        }
        node.label = doc.get_str(&at("label")).unwrap_or_default();
        if let Some(c) = doc.get_str(&at("fill")).and_then(|t| parse_colour(&t)) {
            node.fill_color = c;
        }
        if let Some(c) = doc.get_str(&at("border")).and_then(|t| parse_colour(&t)) {
            node.border_color = c;
        }
        if let Some(v) = read_f32(doc, &at("border-width")).filter(|v| *v >= 0.0) {
            node.border_width = v;
        }
        if let Some(v) = read_f32(doc, &at("font-size")).filter(|v| *v > 0.0) {
            node.font_size = v;
        }
        node.group_id = read_id(doc, &at("group"));
        nodes.push(node);
    }

    let mut edges = Vec::new();
    for k in positions(doc, &["edges"]) {
        let at = |f: &'static str| ["edges", k.as_str(), f];
        let (Some(id), Some(from), Some(to)) = (
            read_id(doc, &at("id")),
            read_id(doc, &at("from")),
            read_id(doc, &at("to")),
        ) else {
            left_out = left_out.saturating_add(1);
            continue;
        };
        // An arrow is drawn between two boxes; without both it is nothing.
        if !nodes.iter().any(|n| n.id == from) || !nodes.iter().any(|n| n.id == to) {
            left_out = left_out.saturating_add(1);
            continue;
        }
        let mut edge = DiagramEdge::new(
            claim(&mut seen, id)?,
            from,
            to,
            layer_or_first(read_id(doc, &at("layer"))),
        );
        if let Some(kind) = doc
            .get_str(&at("kind"))
            .and_then(|t| by_label(EdgeKind::all(), EdgeKind::label, &t))
        {
            edge.kind = kind;
        }
        edge.label = doc.get_str(&at("label")).unwrap_or_default();
        if let Some(c) = doc.get_str(&at("colour")).and_then(|t| parse_colour(&t)) {
            edge.color = c;
        }
        if let Some(line) = doc
            .get_str(&at("line"))
            .and_then(|t| by_label(LineStyle::all(), LineStyle::label, &t))
        {
            edge.line_style = line;
        }
        if let Some(v) = read_f32(doc, &at("width")).filter(|v| *v > 0.0) {
            edge.line_width = v;
        }
        if let Some(a) = doc
            .get_str(&at("start"))
            .and_then(|t| by_label(ArrowHead::all(), ArrowHead::label, &t))
        {
            edge.start_arrow = a;
        }
        if let Some(a) = doc
            .get_str(&at("end"))
            .and_then(|t| by_label(ArrowHead::all(), ArrowHead::label, &t))
        {
            edge.end_arrow = a;
        }
        edges.push(edge);
    }

    let mut groups = Vec::new();
    for k in positions(doc, &["groups"]) {
        let at = |f: &'static str| ["groups", k.as_str(), f];
        let Some(id) = read_id(doc, &at("id")) else {
            left_out = left_out.saturating_add(1);
            continue;
        };
        let members: Vec<u64> = doc
            .get_seq(&at("members"))
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m.trim().parse::<u64>().ok())
            .filter(|m| nodes.iter().any(|n| n.id == *m))
            .collect();
        let mut group = Group::new(claim(&mut seen, id)?, members);
        if let Some(name) = doc.get_str(&at("name")) {
            group.name = name;
        }
        groups.push(group);
    }
    // A box names a group only if the group is there.
    for node in &mut nodes {
        if node
            .group_id
            .is_some_and(|g| !groups.iter().any(|group| group.id == g))
        {
            node.group_id = None;
        }
    }

    Ok((
        DiagramSnapshot {
            nodes,
            edges,
            layers,
            groups,
        },
        left_out,
    ))
}

fn escape_json(s: &str) -> String {
    guitk::escape::json_string(s)
}

// ============================================================================
// Entry point
// ============================================================================

impl App for DiagramApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    /// The diagram's name, marked `*` while it has changes not saved.
    fn title(&self) -> String {
        format!(
            "{}{} — Diagram",
            if self.dirty { "*" } else { "" },
            self.document_name()
        )
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.window_w as u32, self.window_h as u32)
        }
    }

    /// No clock.
    ///
    /// A node moves when it is dragged and the view zooms when it is zoomed.
    /// There is no animation and no data that ages, so a tick would redraw an
    /// identical frame.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    /// Closing over unsaved changes asks first, and the window waits for the
    /// answer: `KeepOpen` declines the close and draws the question.
    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return if self.request_close() {
                Response::Exit
            } else {
                Response::KeepOpen
            };
        }
        let result = self.handle_event(event);
        if self.quit {
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
        self.window_w = width;
        self.window_h = height;
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
    let mut app = DiagramApp::new(1280.0, 800.0);
    // Until a document can be opened this is what there is to edit.
    app.load_template(DiagramTemplate::Flowchart);
    app::launch("diagram", &mut app)
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
    /// The picker is not merely open: it is DRAWN.
    ///
    /// `is_open()` returning true is not the same claim, and assuming it was
    /// is how `apps/flashcards` shipped a dialog that took every keystroke and
    /// painted nothing. Deleting the `picker.render` line in the renderer
    /// leaves `is_open()` true and every other test green; this is the one
    /// that notices.
    #[test]
    fn the_picker_is_drawn_when_it_is_open() {
        let mut app = DiagramApp::new(1280.0, 800.0);
        let before = app.render_commands().len();
        app.open_save_dialog();
        assert!(app.picker.is_open(), "no picker came up");
        let after = app.render_commands().len();
        let own = app.picker.render(&app.palette, 1280.0, 800.0).len();
        assert!(
            own > 0,
            "the picker itself draws nothing, so this proves nothing"
        );
        assert!(
            after >= before + own,
            "the frame does not contain the picker's own {own} command(s) ({before} before, {after} after) -- something else grew instead"
        );
    }

    fn diagram_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("slateos-diagram-door-test");
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn drawn() -> DiagramApp {
        let mut app = DiagramApp::new(1280.0, 800.0);
        app.add_node(NodeShape::Rectangle, 40.0, 40.0);
        let second = app.add_node(NodeShape::Ellipse, 200.0, 120.0);
        // Selected, so that Delete has something to remove. Without this the
        // keyboard test below has no control: Delete would change nothing
        // whether or not the picker intercepted it.
        app.selection.select_single_node(second);
        app
    }

    /// The extension the user types picks the format.
    ///
    /// On the way OUT there is no content to inspect and the typed name is the
    /// only statement of intent there is -- the opposite of opening a file,
    /// where the content is the fact and the extension is a claim by whoever
    /// named it.
    #[test]
    fn the_typed_extension_decides_the_format() {
        let dir = diagram_dir();
        let svg = dir.join("shapes.svg");
        let json = dir.join("shapes.json");
        let _ = std::fs::remove_file(&svg);
        let _ = std::fs::remove_file(&json);

        let mut app = drawn();
        let said = app.write_diagram(&svg);
        assert!(said.contains("as SVG"), "said: {said}");
        let body = std::fs::read_to_string(&svg).expect("svg written");
        assert!(
            body.contains("<svg"),
            "not an SVG: {:?}",
            &body[..40.min(body.len())]
        );

        let said = app.write_diagram(&json);
        assert!(said.contains("as JSON"), "said: {said}");
        let body = std::fs::read_to_string(&json).expect("json written");
        assert!(
            body.contains("\"nodes\""),
            "not our JSON: {:?}",
            &body[..40.min(body.len())]
        );

        let _ = std::fs::remove_file(&svg);
        let _ = std::fs::remove_file(&json);
    }

    /// An unfamiliar extension gets SVG, which is what the picker offers and
    /// what a viewer can open.
    #[test]
    fn an_unknown_extension_is_written_as_svg() {
        let path = diagram_dir().join("shapes.drawing");
        let _ = std::fs::remove_file(&path);

        let mut app = drawn();
        let said = app.write_diagram(&path);
        assert!(said.contains("as SVG"), "said: {said}");

        let _ = std::fs::remove_file(&path);
    }

    /// An empty canvas is refused rather than written.
    ///
    /// An SVG with no shapes in it is a valid document that opens to nothing,
    /// which the user cannot tell apart from a save that failed -- and by then
    /// it has replaced whatever was at that path. See design-decisions 854.
    #[test]
    fn an_empty_canvas_is_not_written() {
        let path = std::env::temp_dir().join("slateos-diagram-should-not-exist.svg");
        let _ = std::fs::remove_file(&path);

        let mut app = DiagramApp::new(1280.0, 800.0);
        assert!(app.nodes.is_empty(), "the fixture drew something");
        let said = app.write_diagram(&path);
        assert_eq!(said, "Nothing drawn yet -- nothing to write");
        assert!(!path.exists(), "nothing should have been created");
    }

    /// Ctrl+S opens the picker, and while it is up the canvas does not act.
    ///
    /// Single letters select tools here and Delete removes the selected shape,
    /// so a filename typed at an unintercepted picker would redraw the user's
    /// diagram behind it. The control is the first half: it proves the key
    /// DOES reach the canvas when no picker is up, so the second half means
    /// something.
    #[test]
    fn the_picker_takes_the_keyboard_from_the_canvas() {
        // The control: with no picker up, Delete removes the selected node.
        let mut control = drawn();
        let before = control.nodes.len();
        control.handle_event(&press(Key::Delete));
        assert_eq!(
            control.nodes.len(),
            before - 1,
            "the control is broken: Delete does not change the canvas here, so the assertion below would hold for the wrong reason"
        );

        let mut app = drawn();
        app.handle_event(&press_ctrl(Key::S));
        assert!(app.picker.is_open(), "Ctrl+S did not open the picker");
        let before = app.nodes.len();
        app.handle_event(&press(Key::Delete));
        assert_eq!(
            app.nodes.len(),
            before,
            "a keystroke at the picker reached the canvas behind it"
        );
    }

    // ------------------------------------------------------------------
    // A diagram that can be saved and opened again
    //
    // It could be exported -- an SVG, a JSON nothing read -- and never opened
    // again, and the window said so in a banner. Nothing recorded unsaved
    // changes, and the window closed over them.
    // ------------------------------------------------------------------

    /// A scratch directory of the test's own.
    fn scratch(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "slateos-diagram-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// The picker chose `path`, as it does in a window: it takes itself down,
    /// then the choice is acted on.
    fn choose(app: &mut DiagramApp, path: &std::path::Path) {
        assert!(app.picker.is_open(), "nothing was asking for a file");
        app.picker.close();
        app.picked(path);
    }

    /// Everything a diagram holds, one line per thing, in a fixed order.
    fn describe(app: &DiagramApp) -> String {
        let mut out = Vec::new();
        for l in &app.layers {
            out.push(format!(
                "layer {} {:?} {} {}",
                l.id, l.name, l.visible, l.order
            ));
        }
        for n in &app.nodes {
            out.push(format!(
                "node {} {:?} {} {} {} {} {:?} {:?} {:?} {} {} {} {:?}",
                n.id,
                n.shape,
                n.x,
                n.y,
                n.width,
                n.height,
                n.label,
                n.fill_color,
                n.border_color,
                n.border_width,
                n.font_size,
                n.layer_id,
                n.group_id
            ));
        }
        for e in &app.edges {
            out.push(format!(
                "edge {} {}->{} {:?} {:?} {:?} {:?} {} {:?} {:?} {}",
                e.id,
                e.from_node,
                e.to_node,
                e.kind,
                e.label,
                e.color,
                e.line_style,
                e.line_width,
                e.start_arrow,
                e.end_arrow,
                e.layer_id
            ));
        }
        for g in &app.groups {
            out.push(format!("group {} {:?} {:?}", g.id, g.name, g.member_ids));
        }
        out.join("\n")
    }

    /// A diagram using everything a file has to keep: every property set away
    /// from its default, a hidden second layer, and a group.
    fn rich() -> DiagramApp {
        let mut app = DiagramApp::new(1280.0, 800.0);
        let a = app.add_node(NodeShape::Diamond, 40.0, 60.0);
        let b = app.add_node(NodeShape::Cylinder, 300.0, 90.5);
        let hidden = 90;
        app.layers
            .push(Layer::new(hidden, String::from("Notes: draft"), 1));
        app.layers[1].visible = false;
        let c = 91;
        app.nodes
            .push(DiagramNode::new(c, NodeShape::Cloud, -20.25, 400.0, hidden));
        for n in &mut app.nodes {
            n.label = format!("say \"{}\" # not a comment", n.id);
            n.fill_color = Color::rgba(1, 2, 3, 128);
            n.border_color = Color::rgb(250, 128, 7);
            n.border_width = 3.5;
            n.font_size = 17.0;
            n.width += 11.0;
        }
        app.groups.push(Group::new(92, vec![a, b]));
        app.groups[0].name = String::from("the pair");
        for n in app.nodes.iter_mut().filter(|n| n.id == a || n.id == b) {
            n.group_id = Some(92);
        }
        let mut e = DiagramEdge::new(93, a, b, app.active_layer_id);
        e.kind = EdgeKind::Orthogonal;
        e.label = String::from("yes: always");
        e.color = Color::rgb(9, 8, 7);
        e.line_style = LineStyle::Dotted;
        e.line_width = 4.0;
        e.start_arrow = ArrowHead::Diamond;
        e.end_arrow = ArrowHead::Open;
        app.edges.push(e);
        app
    }

    /// **A diagram saved and opened again is the same diagram** -- every box,
    /// arrow, layer and group, with every property.
    #[test]
    fn a_diagram_saved_and_opened_again_is_the_same_diagram() {
        let dir = scratch("roundtrip");
        let path = dir.join("plan.diagram");
        let mut app = rich();
        let said = app.write_native(&path);
        assert!(said.starts_with("Saved"), "{said}");
        assert!(!app.dirty);

        let mut other = DiagramApp::new(1280.0, 800.0);
        let said = other.read_native(&path);
        assert!(said.starts_with("Opened"), "{said}");
        assert_eq!(describe(&other), describe(&app));
        assert!(!other.dirty, "a diagram just opened has nothing unsaved");
        assert_eq!(other.document_path.as_deref(), Some(path.as_path()));
        assert_eq!(other.title(), "plan.diagram — Diagram");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Something added after opening takes an id of its own, never one the
    /// file already used.
    #[test]
    fn new_things_do_not_take_the_ids_of_opened_ones() {
        let dir = scratch("ids");
        let path = dir.join("ids.diagram");
        rich().write_native(&path);
        let mut app = DiagramApp::new(1280.0, 800.0);
        app.read_native(&path);
        let highest = app.nodes.iter().map(|n| n.id).max().unwrap_or(0).max(93);
        let added = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        assert!(added > highest, "{added} is not past {highest}");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A file this cannot read as a diagram is refused, and the diagram open
    /// stays as it was.
    #[test]
    fn a_file_that_is_not_a_diagram_is_refused() {
        let dir = scratch("refused");
        let mut app = rich();
        let before = describe(&app);
        let cases: [(&str, &[u8], &str); 4] = [
            ("settings.yaml", b"fonts:\n  size: 13\n", "not a SlateOS diagram"),
            ("later.diagram", b"slateos-diagram: 2\n", "later format"),
            ("binary.diagram", b"\xff\xfe\x00junk", "Could not open"),
            (
                "twice.diagram",
                b"slateos-diagram: 1\nnodes:\n  1:\n    id: 5\n    shape: Circle\n    x: 1\n    y: 1\n  2:\n    id: 5\n    shape: Circle\n    x: 2\n    y: 2\n",
                "share the id 5",
            ),
        ];
        for (name, bytes, why) in cases {
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("fixture");
            let said = app.read_native(&path);
            assert!(
                said.starts_with("Could not open") && said.contains(why),
                "{name}: {said}"
            );
            assert_eq!(describe(&app), before, "{name} changed the diagram");
        }
        let said = app.read_native(&dir.join("absent.diagram"));
        assert!(said.starts_with("Could not open"), "{said}");

        // Larger than the most this reads: refused whole, never read as the
        // smaller diagram its first part would parse as.
        let big = dir.join("big.diagram");
        rich().write_native(&big);
        let said = app.read_native_within(&big, 64);
        assert!(said.contains("larger than the 64"), "{said}");
        assert_eq!(
            describe(&app),
            before,
            "a file cut short changed the diagram"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// What this version does not know is left out, and the rest is read: a
    /// box of an unknown shape, an arrow to a box that is not there, a group
    /// member that is not there. A file with no layers gets one.
    #[test]
    fn what_is_not_understood_is_left_out_and_the_rest_read() {
        let dir = scratch("partial");
        let path = dir.join("partial.diagram");
        std::fs::write(
            &path,
            concat!(
                "slateos-diagram: 1\n",
                "nodes:\n",
                "  1:\n    id: 1\n    shape: Circle\n    x: 5\n    y: 6\n",
                "  2:\n    id: 2\n    shape: Hologram\n    x: 0\n    y: 0\n",
                "edges:\n",
                "  1:\n    id: 3\n    from: 1\n    to: 2\n",
                "groups:\n",
                "  1:\n    id: 4\n    members:\n      - 1\n      - 2\n",
            ),
        )
        .expect("fixture");
        let mut app = DiagramApp::new(1280.0, 800.0);
        let said = app.read_native(&path);
        assert!(said.starts_with("Opened"), "{said}");
        assert!(
            said.contains("leaving out 2"),
            "what was left out was not said: {said}"
        );
        assert_eq!(app.nodes.len(), 1, "the unknown shape was read");
        assert!(app.edges.is_empty(), "an arrow to nothing was read");
        assert_eq!(
            app.groups.first().map(|g| g.member_ids.clone()),
            Some(vec![1])
        );
        assert_eq!(app.layers.len(), 1, "no layer was made for the box");
        assert_eq!(app.nodes[0].layer_id, app.layers[0].id);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Ctrl+S asks where the first time and saves over the diagram's own file
    /// after; Ctrl+Shift+S always asks.
    #[test]
    fn ctrl_s_saves_over_the_diagrams_own_file_once_it_has_one() {
        let dir = scratch("ctrl-s");
        let path = dir.join("mine.diagram");
        let mut app = DiagramApp::new(1280.0, 800.0);
        app.add_node(NodeShape::Rectangle, 10.0, 10.0);
        assert!(app.dirty);
        assert!(app.title().starts_with('*'), "{}", app.title());
        app.handle_event(&press_ctrl(Key::S));
        assert_eq!(app.picker_for, PickerFor::Save);
        choose(&mut app, &path);
        assert!(!app.dirty);

        app.add_node(NodeShape::Circle, 100.0, 10.0);
        app.handle_event(&press_ctrl(Key::S));
        assert!(!app.picker.is_open(), "a diagram with a file asked again");
        let mut other = DiagramApp::new(1280.0, 800.0);
        other.read_native(&path);
        assert_eq!(other.nodes.len(), 2);

        let mut shift = Modifiers::NONE;
        shift.ctrl = true;
        shift.shift = true;
        app.handle_event(&Event::Key(KeyEvent {
            key: Key::S,
            pressed: true,
            modifiers: shift,
            text: String::new(),
        }));
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Save);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Ctrl+E exports, as Ctrl+S did before a diagram could be saved -- and an
    /// export is not a save: nothing reads it back.
    #[test]
    fn an_export_is_not_a_save() {
        let dir = scratch("export");
        let svg = dir.join("look.svg");
        let mut app = drawn();
        app.handle_event(&press_ctrl(Key::E));
        assert_eq!(app.picker_for, PickerFor::Export);
        choose(&mut app, &svg);
        assert!(
            std::fs::read_to_string(&svg)
                .expect("written")
                .contains("<svg")
        );
        assert!(app.dirty, "an export cleared the unsaved mark");
        assert_eq!(
            app.document_path, None,
            "an export became the diagram's file"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// **Closing or opening over unsaved changes asks**, and each answer is
    /// kept.
    #[test]
    fn closing_or_opening_over_unsaved_changes_asks() {
        let dir = scratch("close");
        let path = dir.join("kept.diagram");
        let mut clean = DiagramApp::new(1280.0, 800.0);
        assert_eq!(clean.on_event(&Event::CloseRequested), Response::Exit);

        let mut app = drawn();
        app.write_native(&path);
        let on_disk = std::fs::read(&path).expect("saved");
        app.add_node(NodeShape::Hexagon, 500.0, 500.0);
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        let text: String = app
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
            text.contains("kept.diagram has changes that are not saved."),
            "{text}"
        );

        // A tool key goes to the question, not the canvas.
        let mode = app.mode;
        app.on_event(&press(Key::R));
        assert_eq!(
            app.mode, mode,
            "a key reached the canvas under the question"
        );

        assert_eq!(app.on_event(&press(Key::Escape)), Response::Redraw);
        assert_eq!(std::fs::read(&path).expect("unchanged"), on_disk);
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(app.on_event(&press(Key::S)), Response::Exit);
        assert_ne!(std::fs::read(&path).expect("saved again"), on_disk);

        // Open over unsaved changes asks too; Don't save goes on to the picker.
        let mut app = drawn();
        app.handle_event(&press_ctrl(Key::O));
        assert!(!app.picker.is_open(), "Ctrl+O opened over unsaved changes");
        app.handle_event(&press(Key::D));
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Open);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// What a save did is on the screen. It was recorded and drawn nowhere.
    #[test]
    fn the_status_bar_says_what_the_last_save_did() {
        let dir = scratch("status");
        let mut app = drawn();
        let said = app.write_native(&dir.join("missing").join("x.diagram"));
        app.last_save = Some(said);
        let text: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            text.iter().any(|t| t.starts_with("Could not save")),
            "the failure is not shown: {text:?}"
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Naming a box and leaving the name as it was is no change.
    #[test]
    fn a_name_left_as_it_was_is_no_change() {
        let dir = scratch("name");
        let mut app = drawn();
        app.write_native(&dir.join("n.diagram"));
        let id = app.nodes[0].id;
        let name = app.nodes[0].label.clone();
        app.set_node_label(id, name);
        assert!(!app.dirty, "an unchanged name marked the diagram");
        app.set_node_label(id, String::from("changed"));
        assert!(app.dirty);
        drop(std::fs::remove_dir_all(&dir));
    }

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no input handling at all until it was wired to the
    // compositor.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;

    fn loaded() -> DiagramApp {
        let mut app = DiagramApp::new(1280.0, 800.0);
        app.load_template(DiagramTemplate::Flowchart);
        app
    }

    fn types(text: &str) -> Event {
        Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    /// A user can say what a box means.
    ///
    /// This program could not: `set_node_label` and `set_edge_label` were
    /// written and callerless, and there were zero `key.text` sites in the
    /// crate, so every box said what its template said -- "Process",
    /// "Decision?", "CEO" -- permanently. A user's diagram of their own
    /// system was always a diagram of ours.
    #[test]
    fn a_user_can_label_a_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.selection.nodes = vec![id];

        app.handle_event(&press(Key::F2));
        assert!(app.editing.is_some(), "F2 did not begin labelling");
        for c in ["P", "a", "y"] {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Escape));

        let node = app.nodes.iter().find(|n| n.id == id).expect("the node");
        assert!(node.label.ends_with("Pay"), "the label is {:?}", node.label);
    }

    /// The status bar says the app is labelling, and how to stop.
    ///
    /// `Backspace` means something else in this mode -- a character rather
    /// than the selected node -- which is worth being told, not discovered.
    #[test]
    fn the_status_bar_says_it_is_labelling() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.selection.nodes = vec![id];
        app.handle_event(&press(Key::F2));

        let shown: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            shown.iter().any(|t| t.contains("Labelling")),
            "nothing on screen says the app is labelling"
        );
    }

    /// An edge can be labelled too, and its panel follows the typing.
    ///
    /// Added after the node row was fixed and the edge row was not: the
    /// canvas showed the new label while the properties panel still showed
    /// the old one, and no test covered edges to say so.
    #[test]
    fn a_user_can_label_an_edge() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let a = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        let b = app.add_node(NodeShape::Rectangle, 300.0, 100.0);
        let e = app.add_edge(a, b);
        app.set_edge_label(e, String::from("Old"));
        app.selection.edges = vec![e];

        app.handle_event(&press(Key::F2));
        for _ in 0..3 {
            app.handle_event(&press(Key::Backspace));
        }
        for c in ["Y", "e", "s"] {
            app.handle_event(&types(c));
        }

        let shown: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            !shown.iter().any(|t| t == "Old"),
            "the old edge label is still drawn somewhere: {shown:?}"
        );

        app.handle_event(&press(Key::Escape));
        let edge = app.edges.iter().find(|x| x.id == e).expect("the edge");
        assert_eq!(edge.label, "Yes", "the edge label was not written");
    }

    /// Backspace while labelling deletes a character, not the box.
    ///
    /// Outside this mode `Backspace` is bound to delete the selection, so
    /// without the mode taking the keyboard first a typo while naming a box
    /// would delete the box -- and the undo stack is the only thing that
    /// would have told anyone.
    #[test]
    fn backspace_while_labelling_does_not_delete_the_node() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.selection.nodes = vec![id];
        let before = app.nodes.len();

        app.handle_event(&press(Key::F2));
        app.handle_event(&types("x"));
        app.handle_event(&press(Key::Backspace));

        assert_eq!(app.nodes.len(), before, "Backspace deleted the node");
        assert!(app.editing.is_some(), "and left the mode");
    }

    /// The canvas shows the label as it is typed.
    #[test]
    fn the_canvas_shows_the_label_as_it_is_typed() {
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.set_node_label(id, String::from("Old"));
        app.selection.nodes = vec![id];

        app.handle_event(&press(Key::F2));
        for _ in 0..3 {
            app.handle_event(&press(Key::Backspace));
        }
        for c in ["N", "e", "w"] {
            app.handle_event(&types(c));
        }

        let shown: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            shown.iter().any(|t| t == "New"),
            "the label being typed is nowhere on the canvas"
        );
        assert!(
            !shown.iter().any(|t| t == "Old"),
            "the old label is still drawn while it is being replaced"
        );
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// The label is read by `guitk::shortcut` rather than matched against a
    /// table beside it here, which would be a third copy of the same fact.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = help_states().iter_mut().any(|app| {
                    app.handle_event(&Event::Key(stroke.clone())) == EventResult::Consumed
                });
                assert!(
                    answered,
                    "the list advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Diagrams chosen so that between them every advertised key has work.
    fn help_states() -> Vec<DiagramApp> {
        let empty = DiagramApp::new(1000.0, 700.0);

        // A shape on the canvas and selected, which is what naming and
        // deleting each need before they will act.
        let mut drawn = DiagramApp::new(1000.0, 700.0);
        drawn.handle_event(&press(Key::R));

        // ...and something done, so undo has work; then undone, so redo does.
        let mut undone = DiagramApp::new(1000.0, 700.0);
        undone.handle_event(&press(Key::R));
        undone.handle_event(&press_ctrl(Key::Z));

        // ...and zoomed away from 100%, so `Ctrl+0` has somewhere to return
        // from: setting the zoom to what it already is changes nothing, and
        // nothing changing is how this app reports `Ignored`.
        let mut zoomed = DiagramApp::new(1000.0, 700.0);
        zoomed.handle_event(&press(Key::Equals));

        // ...and with a tool other than Select in hand, so `Escape` has
        // somewhere to return from. It declines in the Select tool, which is
        // correct -- there is nothing to back out of -- and is not the same
        // thing as being unbound.
        let mut tooled = DiagramApp::new(1000.0, 700.0);
        tooled.handle_event(&press(Key::A));

        vec![empty, drawn, undone, zoomed, tooled]
    }

    /// **The properties panel can be put away.**
    ///
    /// `show_properties` was `true` at construction and written nowhere, so
    /// the panel was permanent and the canvas beside it had that much less
    /// room -- the third member of the group `G` and `S` are in. Asserts the
    /// effect rather than the answer, because `Consumed` cannot tell a toggle
    /// from a fall-through.
    #[test]
    fn the_properties_panel_can_be_hidden() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        let before = (app.show_properties, app.show_grid, app.snap_to_grid);

        app.handle_event(&press(Key::P));

        assert_ne!(app.show_properties, before.0, "P did not move the panel");
        assert_eq!(
            (app.show_grid, app.snap_to_grid),
            (before.1, before.2),
            "P moved one of its neighbours in the same group"
        );
    }

    /// **The shortcut list reaches the window.**
    #[test]
    fn the_shortcut_list_reaches_the_window() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        assert!(
            !drawn_help_text(&app).contains("F1 or ? closes this"),
            "the list is up before anybody asked for it"
        );

        app.handle_event(&press(Key::F1));
        let shown = drawn_help_text(&app);
        for (keys, what) in SHORTCUTS {
            assert!(shown.contains(*keys), "{keys:?} never reached the window");
            assert!(shown.contains(*what), "{what:?} never reached the window");
        }

        app.handle_event(&press(Key::Escape));
        assert!(
            !drawn_help_text(&app).contains("F1 or ? closes this"),
            "Escape did not close it"
        );
    }

    /// Every string the window is drawing, joined.
    fn drawn_help_text(app: &DiagramApp) -> String {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Every tool button can be clicked, and selects its tool.
    ///
    /// Three were drawn with the active one highlighted and the toolbar was
    /// not hit-tested at all, so a click on one was converted to canvas
    /// coordinates and handled as a click on the drawing underneath it.
    #[test]
    fn every_tool_button_selects_its_tool() {
        for (rect, mode, label) in DiagramApp::new(1000.0, 700.0).tool_buttons() {
            let mut app = DiagramApp::new(1000.0, 700.0);
            app.mode = InteractionMode::Pan;
            app.handle_event(&click_at(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
            assert_eq!(
                app.mode, mode,
                "clicking the {label} button did not select that tool"
            );
        }
    }

    /// Every shape the palette offers can be placed on the canvas.
    ///
    /// Ten were drawn down the sidebar, each highlighted when the mode named
    /// it, and nothing set the mode -- so seven of this program's ten shapes
    /// could not be drawn at all, and the other three only through `R`, `E`
    /// and `D`.
    #[test]
    fn every_shape_in_the_palette_can_be_placed() {
        for (rect, shape) in DiagramApp::new(1000.0, 700.0).shape_buttons() {
            let mut app = DiagramApp::new(1000.0, 700.0);
            app.nodes.clear();

            app.handle_event(&click_at(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0));
            assert_eq!(
                app.mode,
                InteractionMode::AddNode(shape),
                "clicking {} in the palette did not arm it",
                shape.label()
            );

            // Well clear of the sidebar and the toolbar.
            app.handle_event(&click_at(500.0, 400.0));
            assert_eq!(
                app.nodes.len(),
                1,
                "clicking the canvas with {} armed placed nothing",
                shape.label()
            );
            assert_eq!(
                app.nodes[0].shape, shape,
                "the placed node is not the shape that was chosen"
            );
        }
    }

    /// Two boxes can be joined.
    ///
    /// `edge_source` was declared, initialised and read by nothing, and
    /// `add_edge` was called only when building the sample diagram -- so this
    /// program could draw boxes and never connect two of them, in an
    /// application where the connections are the diagram.
    #[test]
    fn two_boxes_can_be_joined_with_an_edge() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        app.nodes.clear();
        app.edges.clear();
        let a = app.add_node(NodeShape::Rectangle, 300.0, 300.0);
        let b = app.add_node(NodeShape::Rectangle, 600.0, 300.0);

        app.handle_event(&press(Key::A));
        assert_eq!(
            app.mode,
            InteractionMode::AddEdge,
            "A did not pick the tool"
        );

        let (ax, ay) = app.canvas_to_screen(300.0, 300.0);
        let (bx, by) = app.canvas_to_screen(600.0, 300.0);
        app.handle_event(&click_at(ax, ay));
        assert_eq!(app.edge_source, Some(a), "the first click picked no source");
        assert!(app.edges.is_empty(), "an edge appeared from one click");

        app.handle_event(&click_at(bx, by));
        assert_eq!(app.edges.len(), 1, "the second click drew no edge");
        assert_eq!(app.edges[0].from_node, a);
        assert_eq!(app.edges[0].to_node, b);
        assert_eq!(
            app.edge_source, None,
            "the source is still pending after the edge was drawn"
        );
        assert_eq!(
            app.selection.edges,
            vec![app.edges[0].id],
            "the new edge is not selected, so F2 cannot name it"
        );
    }

    /// A box cannot be joined to itself.
    #[test]
    fn clicking_one_box_twice_does_not_join_it_to_itself() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        app.nodes.clear();
        app.edges.clear();
        let a = app.add_node(NodeShape::Rectangle, 300.0, 300.0);
        app.handle_event(&press(Key::A));
        let (ax, ay) = app.canvas_to_screen(300.0, 300.0);
        app.handle_event(&click_at(ax, ay));
        app.handle_event(&click_at(ax, ay));
        assert!(
            app.edges.is_empty(),
            "a node was joined to itself, drawing an edge with nowhere to go"
        );
        assert_eq!(app.edge_source, Some(a), "the source was forgotten instead");
    }

    /// Escape backs out of a half-drawn edge, then out of the tool.
    #[test]
    fn escape_forgets_the_pending_edge_then_returns_to_select() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        app.nodes.clear();
        app.add_node(NodeShape::Rectangle, 300.0, 300.0);
        app.handle_event(&press(Key::A));
        let (ax, ay) = app.canvas_to_screen(300.0, 300.0);
        app.handle_event(&click_at(ax, ay));
        assert!(app.edge_source.is_some(), "control: no pending source");

        app.handle_event(&press(Key::Escape));
        assert_eq!(app.edge_source, None, "Escape did not forget the source");
        assert_eq!(
            app.mode,
            InteractionMode::AddEdge,
            "Escape left the tool as well as the source, in one press"
        );

        app.handle_event(&press(Key::Escape));
        assert_eq!(
            app.mode,
            InteractionMode::Select,
            "a second Escape did not return to the Select tool"
        );
    }

    /// Dragging with the hand tool moves the canvas.
    #[test]
    fn the_pan_tool_moves_the_canvas() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        app.handle_event(&press(Key::H));
        assert_eq!(app.mode, InteractionMode::Pan, "H did not pick the tool");

        let before = (app.pan_x, app.pan_y);
        app.handle_event(&click_at(500.0, 400.0));
        app.handle_event(&Event::Mouse(MouseEvent {
            x: 560.0,
            y: 430.0,
            kind: MouseEventKind::Move,
        }));
        assert_ne!(
            (app.pan_x, app.pan_y),
            before,
            "dragging with the hand tool moved nothing"
        );
    }

    /// Switching tools forgets a half-drawn edge.
    #[test]
    fn changing_tool_forgets_a_pending_edge_source() {
        let mut app = DiagramApp::new(1000.0, 700.0);
        app.nodes.clear();
        app.add_node(NodeShape::Rectangle, 300.0, 300.0);
        app.handle_event(&press(Key::A));
        let (ax, ay) = app.canvas_to_screen(300.0, 300.0);
        app.handle_event(&click_at(ax, ay));
        assert!(app.edge_source.is_some(), "control: no pending source");

        app.handle_event(&press(Key::V));
        assert_eq!(
            app.edge_source, None,
            "a source picked under the edge tool survived into another one"
        );
    }

    fn click_at(x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent {
            x,
            y,
            kind: MouseEventKind::Press(MouseButton::Left),
        })
    }

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

    fn mouse(kind: MouseEventKind, x: f32, y: f32) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    #[test]
    fn a_click_selects_the_node_under_it_at_a_zoom_that_is_not_one() {
        // The hit test works in canvas space and the mouse arrives in screen
        // space. At zoom 1.0 with no pan the two are identical, so a test at
        // the default view proves nothing about the conversion.
        let mut app = loaded();
        app.zoom = 2.0;
        app.pan_x = 37.0;
        app.pan_y = -19.0;
        let node = app.nodes.first().expect("the template has nodes");
        let (nx, ny, id) = (node.x, node.y, node.id);
        let (sx, sy) = app.canvas_to_screen(nx, ny);
        assert_eq!(
            app.handle_event(&mouse(MouseEventKind::Press(MouseButton::Left), sx, sy)),
            EventResult::Consumed
        );
        assert!(
            app.selection.has_node(id),
            "the click missed the node it was aimed at"
        );
    }

    #[test]
    fn dragging_moves_the_node_by_the_distance_dragged_in_canvas_units() {
        // At zoom 2.0 a 100-pixel drag is a 50-unit move.
        let mut app = loaded();
        app.zoom = 2.0;
        app.snap_to_grid = false;
        let node = app.nodes.first().expect("the template has nodes");
        let (nx, ny, id) = (node.x, node.y, node.id);
        let (sx, sy) = app.canvas_to_screen(nx, ny);
        app.handle_event(&mouse(MouseEventKind::Press(MouseButton::Left), sx, sy));
        app.handle_event(&mouse(MouseEventKind::Move, sx + 100.0, sy));
        let moved = app
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| n.x)
            .expect("still there");
        assert!(
            (moved - (nx + 50.0)).abs() < 0.51,
            "expected a 50-unit move at zoom 2, got {}",
            moved - nx
        );
        assert_eq!(
            app.handle_event(&mouse(
                MouseEventKind::Release(MouseButton::Left),
                sx + 100.0,
                sy
            )),
            EventResult::Consumed
        );
        // And a release with nothing being dragged is not a redraw.
        assert_eq!(
            app.handle_event(&mouse(MouseEventKind::Release(MouseButton::Left), sx, sy)),
            EventResult::Ignored
        );
    }

    #[test]
    fn a_pointer_move_with_no_button_down_is_not_a_redraw() {
        let mut app = loaded();
        assert_eq!(
            app.handle_event(&mouse(MouseEventKind::Move, 100.0, 100.0)),
            EventResult::Ignored
        );
    }

    #[test]
    fn clicking_empty_canvas_clears_the_selection() {
        let mut app = loaded();
        let id = app.nodes.first().map(|n| n.id).expect("nodes");
        app.selection.select_single_node(id);
        app.handle_event(&mouse(
            MouseEventKind::Press(MouseButton::Left),
            -9000.0,
            -9000.0,
        ));
        assert!(
            app.selection.nodes.is_empty(),
            "clicking away should deselect"
        );
    }

    #[test]
    fn the_wheel_zooms_and_a_zero_delta_does_nothing() {
        let mut app = loaded();
        let start = app.zoom;
        app.handle_event(&mouse(
            MouseEventKind::Scroll { dx: 0.0, dy: 1.0 },
            0.0,
            0.0,
        ));
        assert!(app.zoom > start);
        app.handle_event(&mouse(
            MouseEventKind::Scroll { dx: 0.0, dy: -1.0 },
            0.0,
            0.0,
        ));
        assert!((app.zoom - start).abs() < 0.001);
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
    fn ctrl_0_returns_to_actual_size_and_then_does_nothing() {
        let mut app = loaded();
        app.zoom_in();
        assert!((app.zoom - 1.0).abs() > f32::EPSILON);
        assert_eq!(
            app.handle_event(&press_ctrl(Key::Num0)),
            EventResult::Consumed
        );
        assert!((app.zoom - 1.0).abs() < f32::EPSILON);
        assert_eq!(
            app.handle_event(&press_ctrl(Key::Num0)),
            EventResult::Ignored
        );
    }

    #[test]
    fn each_shape_key_adds_its_own_shape_where_it_can_be_seen() {
        let mut app = loaded();
        for (k, shape) in [
            (Key::R, NodeShape::Rectangle),
            (Key::E, NodeShape::Ellipse),
            (Key::D, NodeShape::Diamond),
        ] {
            let before = app.nodes.len();
            assert_eq!(app.handle_event(&press(k)), EventResult::Consumed);
            assert_eq!(app.nodes.len(), before + 1, "{k:?} added nothing");
            let (mx, my) = app.screen_to_canvas(app.window_w / 2.0, app.window_h / 2.0);
            let added = app.nodes.last().expect("just added");
            assert_eq!(added.shape, shape, "{k:?} added the wrong shape");
            // At the middle of the view, not at the canvas origin.
            assert!(
                (added.x - mx).abs() < 1.0 && (added.y - my).abs() < 1.0,
                "the new shape landed at {},{} rather than the view centre",
                added.x,
                added.y
            );
        }
    }

    #[test]
    fn delete_with_nothing_selected_is_not_a_redraw() {
        let mut app = loaded();
        app.selection.clear();
        assert_eq!(app.handle_event(&press(Key::Delete)), EventResult::Ignored);
        let id = app.nodes.first().map(|n| n.id).expect("nodes");
        app.selection.select_single_node(id);
        let before = app.nodes.len();
        assert_eq!(app.handle_event(&press(Key::Delete)), EventResult::Consumed);
        assert_eq!(app.nodes.len(), before - 1);
    }

    #[test]
    fn undo_with_nothing_to_undo_is_not_a_redraw() {
        let mut app = DiagramApp::new(1280.0, 800.0);
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Ignored);
        app.handle_event(&press(Key::R));
        let with_shape = app.nodes.len();
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Consumed);
        assert_eq!(app.nodes.len(), with_shape - 1, "undo did not undo");
    }

    #[test]
    fn the_canvas_toggles_flip_independently() {
        let mut app = loaded();
        let (grid, snap) = (app.show_grid, app.snap_to_grid);
        app.handle_event(&press(Key::G));
        assert_ne!(app.show_grid, grid);
        assert_eq!(app.snap_to_grid, snap, "G touched snapping");
        app.handle_event(&press(Key::S));
        assert_ne!(app.snap_to_grid, snap);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = loaded();
        let before = app.nodes.len();
        let release = Event::Key(KeyEvent {
            key: Key::R,
            pressed: false,
            modifiers: Modifiers::NONE,
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.nodes.len(), before);
    }

    #[test]
    fn rendering_draws_something_at_an_awkward_size() {
        let mut app = loaded();
        for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
            assert!(
                !app.render(w, h).commands.is_empty(),
                "drew nothing at {w}x{h}"
            );
        }
    }

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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let all = NodeShape::all();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate() {
                if i != j {
                    assert_ne!(a.accent_color(&pal), b.accent_color(&pal));
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
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_content() {
        let mut app = DiagramApp::new(1280.0, 800.0);
        app.load_template(DiagramTemplate::Flowchart);
        let cmds = app.render_commands();
        assert!(cmds.len() > 50); // Should have many commands for toolbar + palette + nodes + edges.
    }

    #[test]
    fn test_render_with_selection() {
        let mut app = DiagramApp::new(1280.0, 800.0);
        let n1 = app.add_node(NodeShape::Rectangle, 100.0, 100.0);
        app.selection.select_single_node(n1);
        let cmds = app.render_commands();
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut app = DiagramApp::new(800.0, 600.0);
        let id = app.add_node(NodeShape::Rectangle, 0.0, 0.0);
        app.set_node_fill(id, pal.red);
        assert_eq!(app.find_node(id).unwrap().fill_color, pal.red);
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

    /// The two cases above are the ones with a short escape, and they passed
    /// against a version that emitted every *other* control character raw —
    /// which is invalid JSON, so the export would not parse.
    #[test]
    fn a_control_character_in_a_label_does_not_produce_invalid_json() {
        for code in 0u32..0x20 {
            let ch = char::from_u32(code).expect("valid scalar");
            let escaped = escape_json(&format!("label{ch}end"));
            assert!(
                !escaped.chars().any(|c| c < '\u{20}'),
                "U+{code:04X} was emitted raw: {escaped:?}"
            );
        }
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

        fn fills(app: &mut DiagramApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = DiagramApp::new(1000.0, 700.0);

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
}
