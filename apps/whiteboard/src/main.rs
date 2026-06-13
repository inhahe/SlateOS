//! Slate OS Whiteboard
//!
//! A collaborative drawing and diagramming application featuring:
//! - Drawing tools: Pen (freehand), Line, Rectangle, Ellipse, Arrow, Text, Eraser
//! - Stroke properties: 16 preset colors + custom RGB, thickness 1-20px, opacity, dashed/solid
//! - Infinite canvas with pan and zoom (0.1x-10x), optional grid background
//! - Shape creation, selection, move, resize, delete
//! - Multiple layers with show/hide, lock, reorder, per-layer opacity
//! - Full undo/redo action history
//! - Click-to-select, marquee selection, multi-select with Shift
//! - Sticky notes with colored backgrounds and text content
//! - SVG-like text export of canvas contents
//! - Multiple pages/boards with switching
//! - Optional snap-to-grid alignment
//!
//! Uses the guitk library for UI rendering.

#![allow(dead_code)]

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

const MOCHA_BASE: Color = Color::from_hex(0x1E1E2E);
const MOCHA_MANTLE: Color = Color::from_hex(0x181825);
const MOCHA_CRUST: Color = Color::from_hex(0x11111B);
const MOCHA_SURFACE0: Color = Color::from_hex(0x313244);
const MOCHA_SURFACE1: Color = Color::from_hex(0x45475A);
const MOCHA_TEXT: Color = Color::from_hex(0xCDD6F4);
const MOCHA_SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const MOCHA_BLUE: Color = Color::from_hex(0x89B4FA);
const MOCHA_GREEN: Color = Color::from_hex(0xA6E3A1);
const MOCHA_RED: Color = Color::from_hex(0xF38BA8);
const MOCHA_YELLOW: Color = Color::from_hex(0xF9E2AF);
const MOCHA_PEACH: Color = Color::from_hex(0xFAB387);
const MOCHA_LAVENDER: Color = Color::from_hex(0xB4BEFE);
const MOCHA_TEAL: Color = Color::from_hex(0x94E2D5);
const MOCHA_MAUVE: Color = Color::from_hex(0xCBA6F7);
const MOCHA_OVERLAY0: Color = Color::from_hex(0x6C7086);

// ============================================================================
// Layout constants
// ============================================================================

const TOOLBAR_WIDTH: f32 = 52.0;
const TOP_BAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const RIGHT_PANEL_WIDTH: f32 = 200.0;
const LAYER_ROW_HEIGHT: f32 = 30.0;
const PALETTE_SWATCH_SIZE: f32 = 22.0;
const PALETTE_GAP: f32 = 3.0;
const PAGE_TAB_HEIGHT: f32 = 28.0;

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 10.0;
const GRID_SIZE: f32 = 20.0;
const SNAP_THRESHOLD: f32 = 8.0;
const MAX_UNDO_STEPS: usize = 200;
const MAX_THICKNESS: u8 = 20;

// ============================================================================
// Preset palette colors (16 colors)
// ============================================================================

const PALETTE_COLORS: [Color; 16] = [
    Color::from_hex(0x1E1E2E), // Black (mocha base)
    Color::from_hex(0xCDD6F4), // White (mocha text)
    Color::from_hex(0xF38BA8), // Red
    Color::from_hex(0xFAB387), // Orange/Peach
    Color::from_hex(0xF9E2AF), // Yellow
    Color::from_hex(0xA6E3A1), // Green
    Color::from_hex(0x94E2D5), // Teal
    Color::from_hex(0x89B4FA), // Blue
    Color::from_hex(0xCBA6F7), // Mauve/Purple
    Color::from_hex(0xB4BEFE), // Lavender
    Color::from_hex(0xF5C2E7), // Pink
    Color::from_hex(0x74C7EC), // Sapphire
    Color::from_hex(0x89DCEB), // Sky
    Color::from_hex(0xA6ADC8), // Subtext0 / Gray
    Color::from_hex(0x585B70), // Surface2 / Dark Gray
    Color::from_hex(0x313244), // Surface0 / Darker
];

// ============================================================================
// Sticky note colors
// ============================================================================

const STICKY_COLORS: [Color; 6] = [
    Color::from_hex(0xF9E2AF), // Yellow
    Color::from_hex(0xA6E3A1), // Green
    Color::from_hex(0x89B4FA), // Blue
    Color::from_hex(0xF38BA8), // Red/Pink
    Color::from_hex(0xCBA6F7), // Mauve
    Color::from_hex(0xFAB387), // Peach
];

// ============================================================================
// Drawing tool enumeration
// ============================================================================

/// Available drawing tools.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Pen,
    Line,
    Rectangle,
    Ellipse,
    Arrow,
    Text,
    Eraser,
    Select,
    StickyNote,
}

impl Tool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pen => "Pen",
            Self::Line => "Line",
            Self::Rectangle => "Rect",
            Self::Ellipse => "Elli",
            Self::Arrow => "Arrow",
            Self::Text => "Text",
            Self::Eraser => "Eras",
            Self::Select => "Sel",
            Self::StickyNote => "Note",
        }
    }

    pub fn shortcut(self) -> Option<char> {
        match self {
            Self::Pen => Some('P'),
            Self::Line => Some('L'),
            Self::Rectangle => Some('R'),
            Self::Ellipse => Some('O'),
            Self::Arrow => Some('A'),
            Self::Text => Some('T'),
            Self::Eraser => Some('E'),
            Self::Select => Some('S'),
            Self::StickyNote => Some('N'),
        }
    }

    pub fn all() -> &'static [Tool] {
        &[
            Self::Pen,
            Self::Line,
            Self::Rectangle,
            Self::Ellipse,
            Self::Arrow,
            Self::Text,
            Self::Eraser,
            Self::Select,
            Self::StickyNote,
        ]
    }
}

// ============================================================================
// Stroke style
// ============================================================================

/// Whether strokes are solid or dashed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrokeStyle {
    Solid,
    Dashed,
}

/// Stroke properties for drawing operations.
#[derive(Clone, Debug)]
pub struct StrokeProps {
    pub color: Color,
    pub thickness: u8,
    pub opacity: f32,
    pub style: StrokeStyle,
}

impl Default for StrokeProps {
    fn default() -> Self {
        Self {
            color: MOCHA_TEXT,
            thickness: 2,
            opacity: 1.0,
            style: StrokeStyle::Solid,
        }
    }
}

impl StrokeProps {
    /// Returns the color with opacity applied.
    pub fn effective_color(&self) -> Color {
        let alpha = (self.opacity * 255.0).clamp(0.0, 255.0) as u8;
        Color::rgba(self.color.r, self.color.g, self.color.b, alpha)
    }
}

// ============================================================================
// Canvas shapes / elements
// ============================================================================

/// Unique identifier for a shape on the canvas.
pub type ShapeId = u64;

/// A 2D point in canvas space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance_to(self, other: Point) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        (dx * dx + dy * dy).sqrt()
    }
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn from_points(p1: Point, p2: Point) -> Self {
        let x = p1.x.min(p2.x);
        let y = p1.y.min(p2.y);
        let width = (p1.x - p2.x).abs();
        let height = (p1.y - p2.y).abs();
        Self { x, y, width, height }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width
            && py >= self.y && py <= self.y + self.height
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
}

/// The different types of shapes that can appear on the whiteboard.
#[derive(Clone, Debug)]
pub enum ShapeKind {
    /// A freehand path (list of points).
    Freehand { points: Vec<Point> },
    /// A straight line segment.
    Line { start: Point, end: Point },
    /// An axis-aligned rectangle.
    Rectangle { bounds: Rect },
    /// An ellipse inscribed in a bounding rect.
    Ellipse { bounds: Rect },
    /// An arrow from start to end with an arrowhead.
    Arrow { start: Point, end: Point },
    /// A text label at a position.
    TextLabel { position: Point, content: String },
    /// A sticky note with colored background.
    StickyNote {
        bounds: Rect,
        content: String,
        bg_color: Color,
    },
}

/// A single shape on the canvas with its visual properties.
#[derive(Clone, Debug)]
pub struct Shape {
    pub id: ShapeId,
    pub kind: ShapeKind,
    pub stroke: StrokeProps,
    pub layer_id: LayerId,
}

impl Shape {
    /// Compute the bounding box for this shape.
    pub fn bounding_box(&self) -> Rect {
        match &self.kind {
            ShapeKind::Freehand { points } => {
                if points.is_empty() {
                    return Rect::new(0.0, 0.0, 0.0, 0.0);
                }
                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;
                for p in points {
                    if p.x < min_x { min_x = p.x; }
                    if p.y < min_y { min_y = p.y; }
                    if p.x > max_x { max_x = p.x; }
                    if p.y > max_y { max_y = p.y; }
                }
                let pad = self.stroke.thickness as f32 / 2.0;
                Rect::new(
                    min_x - pad,
                    min_y - pad,
                    (max_x - min_x) + self.stroke.thickness as f32,
                    (max_y - min_y) + self.stroke.thickness as f32,
                )
            }
            ShapeKind::Line { start, end } | ShapeKind::Arrow { start, end } => {
                let pad = self.stroke.thickness as f32 / 2.0 + 8.0;
                let x = start.x.min(end.x) - pad;
                let y = start.y.min(end.y) - pad;
                let w = (start.x - end.x).abs() + pad * 2.0;
                let h = (start.y - end.y).abs() + pad * 2.0;
                Rect::new(x, y, w, h)
            }
            ShapeKind::Rectangle { bounds } | ShapeKind::Ellipse { bounds } => {
                let pad = self.stroke.thickness as f32 / 2.0;
                Rect::new(
                    bounds.x - pad,
                    bounds.y - pad,
                    bounds.width + self.stroke.thickness as f32,
                    bounds.height + self.stroke.thickness as f32,
                )
            }
            ShapeKind::TextLabel { position, content } => {
                let approx_width = content.len() as f32 * 8.0;
                Rect::new(position.x, position.y, approx_width, 20.0)
            }
            ShapeKind::StickyNote { bounds, .. } => *bounds,
        }
    }

    /// Test if a canvas point hits this shape.
    pub fn hit_test(&self, px: f32, py: f32) -> bool {
        let threshold = (self.stroke.thickness as f32 / 2.0).max(4.0);
        match &self.kind {
            ShapeKind::Freehand { points } => {
                for window in points.windows(2) {
                    if let (Some(a), Some(b)) = (window.first(), window.get(1))
                        && point_to_segment_distance(px, py, a.x, a.y, b.x, b.y)
                            <= threshold
                        {
                            return true;
                        }
                }
                false
            }
            ShapeKind::Line { start, end } | ShapeKind::Arrow { start, end } => {
                point_to_segment_distance(px, py, start.x, start.y, end.x, end.y)
                    <= threshold
            }
            ShapeKind::Rectangle { bounds } => bounds.contains(px, py),
            ShapeKind::Ellipse { bounds } => {
                let cx = bounds.x + bounds.width / 2.0;
                let cy = bounds.y + bounds.height / 2.0;
                let rx = bounds.width / 2.0;
                let ry = bounds.height / 2.0;
                if rx <= 0.0 || ry <= 0.0 {
                    return false;
                }
                let dx = (px - cx) / rx;
                let dy = (py - cy) / ry;
                (dx * dx + dy * dy) <= 1.0
            }
            ShapeKind::TextLabel { .. } | ShapeKind::StickyNote { .. } => {
                self.bounding_box().contains(px, py)
            }
        }
    }

    /// Translate this shape by a delta.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match &mut self.kind {
            ShapeKind::Freehand { points } => {
                for p in points.iter_mut() {
                    p.x += dx;
                    p.y += dy;
                }
            }
            ShapeKind::Line { start, end } | ShapeKind::Arrow { start, end } => {
                start.x += dx;
                start.y += dy;
                end.x += dx;
                end.y += dy;
            }
            ShapeKind::Rectangle { bounds } | ShapeKind::Ellipse { bounds } => {
                bounds.x += dx;
                bounds.y += dy;
            }
            ShapeKind::TextLabel { position, .. } => {
                position.x += dx;
                position.y += dy;
            }
            ShapeKind::StickyNote { bounds, .. } => {
                bounds.x += dx;
                bounds.y += dy;
            }
        }
    }
}

/// Distance from point (px, py) to the line segment (x1,y1)-(x2,y2).
fn point_to_segment_distance(px: f32, py: f32, x1: f32, y1: f32, x2: f32, y2: f32) -> f32 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;
    if len_sq < 0.001 {
        return ((px - x1).powi(2) + (py - y1).powi(2)).sqrt();
    }
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);
    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;
    ((px - proj_x).powi(2) + (py - proj_y).powi(2)).sqrt()
}

// ============================================================================
// Layer system
// ============================================================================

pub type LayerId = u64;

/// A drawing layer that contains shapes.
#[derive(Clone, Debug)]
pub struct Layer {
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
}

impl Layer {
    pub fn new(id: LayerId, name: String) -> Self {
        Self {
            id,
            name,
            visible: true,
            locked: false,
            opacity: 1.0,
        }
    }
}

// ============================================================================
// Undo/Redo action
// ============================================================================

/// An undoable action on the whiteboard.
#[derive(Clone, Debug)]
pub enum Action {
    AddShape(Shape),
    DeleteShape(ShapeId),
    MoveShape {
        shape_id: ShapeId,
        dx: f32,
        dy: f32,
    },
    AddLayer(Layer),
    DeleteLayer(LayerId),
    ToggleLayerVisibility(LayerId),
    ToggleLayerLock(LayerId),
    SetLayerOpacity {
        layer_id: LayerId,
        old_opacity: f32,
        new_opacity: f32,
    },
    ReorderLayers {
        old_order: Vec<LayerId>,
        new_order: Vec<LayerId>,
    },
    /// Multiple actions that form a single undoable step.
    Batch(Vec<Action>),
}

// ============================================================================
// Page/Board management
// ============================================================================

/// A single whiteboard page containing shapes and layers.
#[derive(Clone, Debug)]
pub struct Page {
    pub name: String,
    pub shapes: Vec<Shape>,
    pub layers: Vec<Layer>,
    pub next_shape_id: ShapeId,
    pub next_layer_id: LayerId,
}

impl Page {
    pub fn new(name: String) -> Self {
        let first_layer = Layer::new(1, "Layer 1".to_string());
        Self {
            name,
            shapes: Vec::new(),
            layers: vec![first_layer],
            next_shape_id: 1,
            next_layer_id: 2,
        }
    }

    pub fn alloc_shape_id(&mut self) -> ShapeId {
        let id = self.next_shape_id;
        self.next_shape_id = self.next_shape_id.wrapping_add(1);
        id
    }

    pub fn alloc_layer_id(&mut self) -> LayerId {
        let id = self.next_layer_id;
        self.next_layer_id = self.next_layer_id.wrapping_add(1);
        id
    }

    /// Find the index of a shape by ID.
    pub fn find_shape_index(&self, id: ShapeId) -> Option<usize> {
        self.shapes.iter().position(|s| s.id == id)
    }

    /// Find the index of a layer by ID.
    pub fn find_layer_index(&self, id: LayerId) -> Option<usize> {
        self.layers.iter().position(|l| l.id == id)
    }

    /// Get a reference to a shape by ID.
    pub fn get_shape(&self, id: ShapeId) -> Option<&Shape> {
        self.shapes.iter().find(|s| s.id == id)
    }

    /// Get a mutable reference to a shape by ID.
    pub fn get_shape_mut(&mut self, id: ShapeId) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|s| s.id == id)
    }

    /// Get shapes on a specific layer, visible shapes only.
    pub fn visible_shapes(&self) -> Vec<&Shape> {
        let visible_layers: Vec<LayerId> = self
            .layers
            .iter()
            .filter(|l| l.visible)
            .map(|l| l.id)
            .collect();
        self.shapes
            .iter()
            .filter(|s| visible_layers.contains(&s.layer_id))
            .collect()
    }

    /// Get all shape IDs on a given layer.
    pub fn shapes_on_layer(&self, layer_id: LayerId) -> Vec<ShapeId> {
        self.shapes
            .iter()
            .filter(|s| s.layer_id == layer_id)
            .map(|s| s.id)
            .collect()
    }
}

// ============================================================================
// Selection state
// ============================================================================

/// Tracks what is currently selected on the canvas.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    pub shape_ids: Vec<ShapeId>,
    pub marquee: Option<Rect>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.shape_ids.clear();
        self.marquee = None;
    }

    pub fn is_empty(&self) -> bool {
        self.shape_ids.is_empty()
    }

    pub fn contains(&self, id: ShapeId) -> bool {
        self.shape_ids.contains(&id)
    }

    pub fn add(&mut self, id: ShapeId) {
        if !self.contains(id) {
            self.shape_ids.push(id);
        }
    }

    pub fn toggle(&mut self, id: ShapeId) {
        if let Some(pos) = self.shape_ids.iter().position(|&sid| sid == id) {
            self.shape_ids.remove(pos);
        } else {
            self.shape_ids.push(id);
        }
    }
}

// ============================================================================
// Interaction / drag state
// ============================================================================

/// Tracks the current mouse interaction.
#[derive(Clone, Debug)]
pub enum DragState {
    /// No active drag.
    None,
    /// Drawing with pen tool (accumulating points).
    DrawingFreehand { points: Vec<Point> },
    /// Drawing a shape from start point.
    DrawingShape { start: Point, current: Point },
    /// Panning the canvas view.
    Panning { last_x: f32, last_y: f32 },
    /// Moving selected shapes.
    Moving {
        start_x: f32,
        start_y: f32,
        last_x: f32,
        last_y: f32,
    },
    /// Drawing a selection marquee.
    Marquee { start: Point, current: Point },
    /// Placing a sticky note.
    PlacingStickyNote { start: Point, current: Point },
}

// ============================================================================
// Main whiteboard application state
// ============================================================================

/// The whiteboard application.
pub struct WhiteboardApp {
    // Window dimensions
    pub win_width: f32,
    pub win_height: f32,

    // Pages
    pub pages: Vec<Page>,
    pub active_page: usize,

    // View / camera
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,

    // Active tool and stroke props
    pub current_tool: Tool,
    pub stroke_props: StrokeProps,
    pub sticky_color_index: usize,

    // Selection
    pub selection: Selection,

    // Drag
    pub drag: DragState,

    // Grid
    pub show_grid: bool,
    pub snap_to_grid: bool,

    // Undo/redo
    pub undo_stack: VecDeque<Action>,
    pub redo_stack: Vec<Action>,

    // Custom RGB input state
    pub custom_r: u8,
    pub custom_g: u8,
    pub custom_b: u8,

    // Text input for text tool and sticky notes
    pub text_input_buffer: String,

    // Active layer tracking
    pub active_layer_id: LayerId,

    // UI panel visibility
    pub show_layers_panel: bool,
}

impl WhiteboardApp {
    pub fn new(width: f32, height: f32) -> Self {
        let first_page = Page::new("Board 1".to_string());
        let first_layer_id = first_page
            .layers
            .first()
            .map(|l| l.id)
            .unwrap_or(1);

        Self {
            win_width: width,
            win_height: height,
            pages: vec![first_page],
            active_page: 0,
            pan_x: 0.0,
            pan_y: 0.0,
            zoom: 1.0,
            current_tool: Tool::Pen,
            stroke_props: StrokeProps::default(),
            sticky_color_index: 0,
            selection: Selection::default(),
            drag: DragState::None,
            show_grid: true,
            snap_to_grid: false,
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            custom_r: 205,
            custom_g: 214,
            custom_b: 244,
            text_input_buffer: String::new(),
            active_layer_id: first_layer_id,
            show_layers_panel: true,
        }
    }

    // ========================================================================
    // Page accessors
    // ========================================================================

    pub fn current_page(&self) -> &Page {
        self.pages
            .get(self.active_page)
            .expect("active_page out of range")
    }

    pub fn current_page_mut(&mut self) -> &mut Page {
        self.pages
            .get_mut(self.active_page)
            .expect("active_page out of range")
    }

    // ========================================================================
    // Coordinate transforms
    // ========================================================================

    /// Convert screen coordinates to canvas coordinates.
    pub fn screen_to_canvas(&self, sx: f32, sy: f32) -> Point {
        let canvas_area = self.canvas_rect();
        let cx = (sx - canvas_area.x - self.pan_x) / self.zoom;
        let cy = (sy - canvas_area.y - self.pan_y) / self.zoom;
        Point::new(cx, cy)
    }

    /// Convert canvas coordinates to screen coordinates.
    pub fn canvas_to_screen(&self, cx: f32, cy: f32) -> Point {
        let canvas_area = self.canvas_rect();
        let sx = cx * self.zoom + self.pan_x + canvas_area.x;
        let sy = cy * self.zoom + self.pan_y + canvas_area.y;
        Point::new(sx, sy)
    }

    /// The screen-space rectangle available for the canvas drawing area.
    pub fn canvas_rect(&self) -> Rect {
        let right_w = if self.show_layers_panel {
            RIGHT_PANEL_WIDTH
        } else {
            0.0
        };
        Rect::new(
            TOOLBAR_WIDTH,
            TOP_BAR_HEIGHT + PAGE_TAB_HEIGHT,
            (self.win_width - TOOLBAR_WIDTH - right_w).max(1.0),
            (self.win_height - TOP_BAR_HEIGHT - PAGE_TAB_HEIGHT - STATUS_BAR_HEIGHT).max(1.0),
        )
    }

    // ========================================================================
    // Snap helpers
    // ========================================================================

    /// Snap a canvas point to the grid if snap is enabled.
    pub fn snap_point(&self, p: Point) -> Point {
        if !self.snap_to_grid {
            return p;
        }
        let gx = (p.x / GRID_SIZE).round() * GRID_SIZE;
        let gy = (p.y / GRID_SIZE).round() * GRID_SIZE;
        Point::new(gx, gy)
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

    pub fn zoom_to_fit(&mut self) {
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.zoom = 1.0;
    }

    // ========================================================================
    // Undo / Redo
    // ========================================================================

    pub fn push_action(&mut self, action: Action) {
        self.redo_stack.clear();
        if self.undo_stack.len() >= MAX_UNDO_STEPS {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(action);
    }

    pub fn undo(&mut self) {
        if let Some(action) = self.undo_stack.pop_back() {
            let reverse = self.reverse_action(&action);
            self.apply_action_silent(&reverse);
            self.redo_stack.push(action);
        }
    }

    pub fn redo(&mut self) {
        if let Some(action) = self.redo_stack.pop() {
            self.apply_action_silent(&action);
            self.undo_stack.push_back(action);
        }
    }

    /// Apply an action without recording it in the undo stack.
    fn apply_action_silent(&mut self, action: &Action) {
        match action {
            Action::AddShape(shape) => {
                self.current_page_mut().shapes.push(shape.clone());
            }
            Action::DeleteShape(id) => {
                let page = self.current_page_mut();
                if let Some(idx) = page.find_shape_index(*id) {
                    page.shapes.remove(idx);
                }
                self.selection.shape_ids.retain(|sid| sid != id);
            }
            Action::MoveShape { shape_id, dx, dy } => {
                if let Some(shape) = self.current_page_mut().get_shape_mut(*shape_id) {
                    shape.translate(*dx, *dy);
                }
            }
            Action::AddLayer(layer) => {
                self.current_page_mut().layers.push(layer.clone());
            }
            Action::DeleteLayer(id) => {
                let page = self.current_page_mut();
                page.layers.retain(|l| l.id != *id);
                page.shapes.retain(|s| s.layer_id != *id);
            }
            Action::ToggleLayerVisibility(id) => {
                if let Some(layer) = self
                    .current_page_mut()
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *id)
                {
                    layer.visible = !layer.visible;
                }
            }
            Action::ToggleLayerLock(id) => {
                if let Some(layer) = self
                    .current_page_mut()
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *id)
                {
                    layer.locked = !layer.locked;
                }
            }
            Action::SetLayerOpacity {
                layer_id,
                new_opacity,
                ..
            } => {
                if let Some(layer) = self
                    .current_page_mut()
                    .layers
                    .iter_mut()
                    .find(|l| l.id == *layer_id)
                {
                    layer.opacity = *new_opacity;
                }
            }
            Action::ReorderLayers { new_order, .. } => {
                let page = self.current_page_mut();
                let mut reordered = Vec::with_capacity(new_order.len());
                for lid in new_order {
                    if let Some(idx) = page.find_layer_index(*lid) {
                        reordered.push(page.layers.get(idx).cloned());
                    }
                }
                let new_layers: Vec<Layer> = reordered.into_iter().flatten().collect();
                page.layers = new_layers;
            }
            Action::Batch(actions) => {
                for a in actions {
                    self.apply_action_silent(a);
                }
            }
        }
    }

    /// Create the reverse of an action for undo purposes.
    fn reverse_action(&self, action: &Action) -> Action {
        match action {
            Action::AddShape(shape) => Action::DeleteShape(shape.id),
            Action::DeleteShape(id) => {
                if let Some(shape) = self.current_page().get_shape(*id) {
                    Action::AddShape(shape.clone())
                } else {
                    // Shape already gone; deleting again is a no-op.
                    Action::DeleteShape(*id)
                }
            }
            Action::MoveShape { shape_id, dx, dy } => Action::MoveShape {
                shape_id: *shape_id,
                dx: -dx,
                dy: -dy,
            },
            Action::AddLayer(layer) => Action::DeleteLayer(layer.id),
            Action::DeleteLayer(id) => {
                if let Some(layer) = self
                    .current_page()
                    .layers
                    .iter()
                    .find(|l| l.id == *id)
                {
                    Action::AddLayer(layer.clone())
                } else {
                    Action::DeleteLayer(*id)
                }
            }
            Action::ToggleLayerVisibility(id) => Action::ToggleLayerVisibility(*id),
            Action::ToggleLayerLock(id) => Action::ToggleLayerLock(*id),
            Action::SetLayerOpacity {
                layer_id,
                old_opacity,
                new_opacity,
            } => Action::SetLayerOpacity {
                layer_id: *layer_id,
                old_opacity: *new_opacity,
                new_opacity: *old_opacity,
            },
            Action::ReorderLayers {
                old_order,
                new_order,
            } => Action::ReorderLayers {
                old_order: new_order.clone(),
                new_order: old_order.clone(),
            },
            Action::Batch(actions) => {
                let reversed: Vec<Action> =
                    actions.iter().rev().map(|a| self.reverse_action(a)).collect();
                Action::Batch(reversed)
            }
        }
    }

    // ========================================================================
    // Shape operations
    // ========================================================================

    /// Add a shape to the current page and push an undo action.
    pub fn add_shape(&mut self, kind: ShapeKind) -> ShapeId {
        let id = self.current_page_mut().alloc_shape_id();
        let shape = Shape {
            id,
            kind,
            stroke: self.stroke_props.clone(),
            layer_id: self.active_layer_id,
        };
        let action = Action::AddShape(shape.clone());
        self.current_page_mut().shapes.push(shape);
        self.push_action(action);
        id
    }

    /// Delete selected shapes.
    pub fn delete_selected(&mut self) {
        let ids: Vec<ShapeId> = self.selection.shape_ids.clone();
        if ids.is_empty() {
            return;
        }
        let mut actions = Vec::new();
        for id in &ids {
            if let Some(shape) = self.current_page().get_shape(*id) {
                actions.push(Action::DeleteShape(shape.id));
            }
        }
        // Remove shapes from page
        let page = self.current_page_mut();
        page.shapes.retain(|s| !ids.contains(&s.id));
        self.selection.clear();
        if !actions.is_empty() {
            self.push_action(Action::Batch(actions));
        }
    }

    /// Move selected shapes by a delta.
    pub fn move_selected(&mut self, dx: f32, dy: f32) {
        let ids: Vec<ShapeId> = self.selection.shape_ids.clone();
        let mut actions = Vec::new();
        for id in &ids {
            if let Some(shape) = self.current_page_mut().get_shape_mut(*id) {
                shape.translate(dx, dy);
                actions.push(Action::MoveShape {
                    shape_id: *id,
                    dx,
                    dy,
                });
            }
        }
        if !actions.is_empty() {
            self.push_action(Action::Batch(actions));
        }
    }

    // ========================================================================
    // Layer operations
    // ========================================================================

    pub fn add_layer(&mut self) {
        let page = self.current_page_mut();
        let id = page.alloc_layer_id();
        let name = format!("Layer {}", id);
        let layer = Layer::new(id, name);
        let action = Action::AddLayer(layer.clone());
        page.layers.push(layer);
        self.active_layer_id = id;
        self.push_action(action);
    }

    pub fn delete_layer(&mut self, layer_id: LayerId) {
        let page = self.current_page_mut();
        // Don't delete the last layer
        if page.layers.len() <= 1 {
            return;
        }
        let action = Action::DeleteLayer(layer_id);
        page.layers.retain(|l| l.id != layer_id);
        page.shapes.retain(|s| s.layer_id != layer_id);

        // If the active layer was deleted, switch to the first available
        if self.active_layer_id == layer_id {
            self.active_layer_id = self
                .current_page()
                .layers
                .first()
                .map(|l| l.id)
                .unwrap_or(1);
        }
        self.push_action(action);
    }

    pub fn toggle_layer_visibility(&mut self, layer_id: LayerId) {
        if let Some(layer) = self
            .current_page_mut()
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
        {
            layer.visible = !layer.visible;
            self.push_action(Action::ToggleLayerVisibility(layer_id));
        }
    }

    pub fn toggle_layer_lock(&mut self, layer_id: LayerId) {
        if let Some(layer) = self
            .current_page_mut()
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
        {
            layer.locked = !layer.locked;
            self.push_action(Action::ToggleLayerLock(layer_id));
        }
    }

    pub fn set_layer_opacity(&mut self, layer_id: LayerId, new_opacity: f32) {
        let new_opacity = new_opacity.clamp(0.0, 1.0);
        if let Some(layer) = self
            .current_page_mut()
            .layers
            .iter_mut()
            .find(|l| l.id == layer_id)
        {
            let old_opacity = layer.opacity;
            layer.opacity = new_opacity;
            self.push_action(Action::SetLayerOpacity {
                layer_id,
                old_opacity,
                new_opacity,
            });
        }
    }

    pub fn move_layer_up(&mut self, layer_id: LayerId) {
        let page = self.current_page_mut();
        if let Some(idx) = page.find_layer_index(layer_id)
            && idx + 1 < page.layers.len() {
                let old_order: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
                page.layers.swap(idx, idx + 1);
                let new_order: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
                self.push_action(Action::ReorderLayers {
                    old_order,
                    new_order,
                });
            }
    }

    pub fn move_layer_down(&mut self, layer_id: LayerId) {
        let page = self.current_page_mut();
        if let Some(idx) = page.find_layer_index(layer_id)
            && idx > 0 {
                let old_order: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
                page.layers.swap(idx, idx - 1);
                let new_order: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
                self.push_action(Action::ReorderLayers {
                    old_order,
                    new_order,
                });
            }
    }

    /// Check if the active layer is locked.
    pub fn is_active_layer_locked(&self) -> bool {
        self.current_page()
            .layers
            .iter()
            .find(|l| l.id == self.active_layer_id)
            .map(|l| l.locked)
            .unwrap_or(false)
    }

    // ========================================================================
    // Page management
    // ========================================================================

    pub fn add_page(&mut self) {
        let n = self.pages.len().saturating_add(1_usize);
        let name = format!("Board {}", n);
        self.pages.push(Page::new(name));
        self.active_page = self.pages.len().saturating_sub(1_usize);
        self.active_layer_id = self
            .current_page()
            .layers
            .first()
            .map(|l| l.id)
            .unwrap_or(1);
    }

    pub fn switch_page(&mut self, index: usize) {
        if index < self.pages.len() {
            self.active_page = index;
            self.selection.clear();
            self.active_layer_id = self
                .current_page()
                .layers
                .first()
                .map(|l| l.id)
                .unwrap_or(1);
        }
    }

    pub fn delete_page(&mut self, index: usize) {
        if self.pages.len() <= 1 || index >= self.pages.len() {
            return;
        }
        self.pages.remove(index);
        if self.active_page >= self.pages.len() {
            self.active_page = self.pages.len().saturating_sub(1_usize);
        }
        self.selection.clear();
        self.active_layer_id = self
            .current_page()
            .layers
            .first()
            .map(|l| l.id)
            .unwrap_or(1);
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    // ========================================================================
    // Canvas mouse event handlers
    // ========================================================================

    /// Handle mouse press on the canvas area (coordinates are screen space).
    pub fn on_canvas_press(&mut self, sx: f32, sy: f32, shift_held: bool) {
        let canvas_pt = self.screen_to_canvas(sx, sy);
        let snapped = self.snap_point(canvas_pt);

        match self.current_tool {
            Tool::Pen => {
                if self.is_active_layer_locked() {
                    return;
                }
                self.drag = DragState::DrawingFreehand {
                    points: vec![snapped],
                };
            }
            Tool::Line | Tool::Rectangle | Tool::Ellipse | Tool::Arrow => {
                if self.is_active_layer_locked() {
                    return;
                }
                self.drag = DragState::DrawingShape {
                    start: snapped,
                    current: snapped,
                };
            }
            Tool::Text => {
                if self.is_active_layer_locked() {
                    return;
                }
                if !self.text_input_buffer.is_empty() {
                    self.add_shape(ShapeKind::TextLabel {
                        position: snapped,
                        content: self.text_input_buffer.clone(),
                    });
                    self.text_input_buffer.clear();
                }
            }
            Tool::Eraser => {
                if self.is_active_layer_locked() {
                    return;
                }
                // Erase any shape under the cursor
                self.erase_at(canvas_pt.x, canvas_pt.y);
            }
            Tool::Select => {
                // Check if clicking on an existing shape
                let hit = self.hit_test_shapes(canvas_pt.x, canvas_pt.y);
                if let Some(hit_id) = hit {
                    if shift_held {
                        self.selection.toggle(hit_id);
                    } else if !self.selection.contains(hit_id) {
                        self.selection.clear();
                        self.selection.add(hit_id);
                    }
                    self.drag = DragState::Moving {
                        start_x: canvas_pt.x,
                        start_y: canvas_pt.y,
                        last_x: canvas_pt.x,
                        last_y: canvas_pt.y,
                    };
                } else {
                    if !shift_held {
                        self.selection.clear();
                    }
                    self.drag = DragState::Marquee {
                        start: canvas_pt,
                        current: canvas_pt,
                    };
                }
            }
            Tool::StickyNote => {
                if self.is_active_layer_locked() {
                    return;
                }
                self.drag = DragState::PlacingStickyNote {
                    start: snapped,
                    current: snapped,
                };
            }
        }
    }

    /// Handle mouse move on the canvas (coordinates are screen space).
    pub fn on_canvas_move(&mut self, sx: f32, sy: f32) {
        let canvas_pt = self.screen_to_canvas(sx, sy);
        let snapped = self.snap_point(canvas_pt);

        // Extract move delta from drag state without holding a mutable borrow
        // across the shape mutation below.
        let move_delta: Option<(f32, f32)> = if let DragState::Moving {
            last_x, last_y, ..
        } = &self.drag
        {
            Some((canvas_pt.x - *last_x, canvas_pt.y - *last_y))
        } else {
            None
        };

        // Extract pan delta similarly to avoid borrow conflict.
        let pan_delta: Option<(f32, f32)> = if let DragState::Panning {
            last_x, last_y,
        } = &self.drag
        {
            Some((sx - *last_x, sy - *last_y))
        } else {
            None
        };

        if let Some((dx, dy)) = move_delta {
            // Move all selected shapes, then update drag state.
            let ids: Vec<ShapeId> = self.selection.shape_ids.clone();
            for id in &ids {
                if let Some(shape) = self.current_page_mut().get_shape_mut(*id) {
                    shape.translate(dx, dy);
                }
            }
            if let DragState::Moving { last_x, last_y, .. } = &mut self.drag {
                *last_x = canvas_pt.x;
                *last_y = canvas_pt.y;
            }
            return;
        }

        if let Some((dx, dy)) = pan_delta {
            self.pan_x += dx;
            self.pan_y += dy;
            if let DragState::Panning { last_x, last_y } = &mut self.drag {
                *last_x = sx;
                *last_y = sy;
            }
            return;
        }

        match &mut self.drag {
            DragState::DrawingFreehand { points } => {
                points.push(snapped);
            }
            DragState::DrawingShape { current, .. } => {
                *current = snapped;
            }
            DragState::Marquee { current, .. } => {
                *current = canvas_pt;
            }
            DragState::PlacingStickyNote { current, .. } => {
                *current = snapped;
            }
            DragState::Panning { .. }
            | DragState::Moving { .. }
            | DragState::None => {}
        }
    }

    /// Handle mouse release on the canvas (coordinates are screen space).
    pub fn on_canvas_release(&mut self, sx: f32, sy: f32) {
        let canvas_pt = self.screen_to_canvas(sx, sy);
        let snapped = self.snap_point(canvas_pt);

        let old_drag = core::mem::replace(&mut self.drag, DragState::None);

        match old_drag {
            DragState::DrawingFreehand { points } => {
                if points.len() >= 2 {
                    self.add_shape(ShapeKind::Freehand { points });
                }
            }
            DragState::DrawingShape { start, .. } => {
                let end = snapped;
                match self.current_tool {
                    Tool::Line => {
                        self.add_shape(ShapeKind::Line { start, end });
                    }
                    Tool::Rectangle => {
                        let bounds = Rect::from_points(start, end);
                        if bounds.width > 1.0 && bounds.height > 1.0 {
                            self.add_shape(ShapeKind::Rectangle { bounds });
                        }
                    }
                    Tool::Ellipse => {
                        let bounds = Rect::from_points(start, end);
                        if bounds.width > 1.0 && bounds.height > 1.0 {
                            self.add_shape(ShapeKind::Ellipse { bounds });
                        }
                    }
                    Tool::Arrow => {
                        self.add_shape(ShapeKind::Arrow { start, end });
                    }
                    _ => {}
                }
            }
            DragState::Moving {
                start_x,
                start_y,
                last_x,
                last_y,
            } => {
                let total_dx = last_x - start_x;
                let total_dy = last_y - start_y;
                if total_dx.abs() > 0.5 || total_dy.abs() > 0.5 {
                    // Record the total move as a single undoable action.
                    // We already moved them incrementally, so we just record
                    // the move for undo (reverse will move them back).
                    let ids: Vec<ShapeId> = self.selection.shape_ids.clone();
                    let mut actions = Vec::new();
                    for id in ids {
                        actions.push(Action::MoveShape {
                            shape_id: id,
                            dx: total_dx,
                            dy: total_dy,
                        });
                    }
                    if !actions.is_empty() {
                        self.push_action(Action::Batch(actions));
                    }
                }
            }
            DragState::Marquee { start, .. } => {
                let end = canvas_pt;
                let marquee_rect = Rect::from_points(start, end);
                // Select all shapes whose bounding boxes intersect the marquee
                let page = self.current_page();
                let hits: Vec<ShapeId> = page
                    .visible_shapes()
                    .iter()
                    .filter(|s| s.bounding_box().intersects(&marquee_rect))
                    .map(|s| s.id)
                    .collect();
                for id in hits {
                    self.selection.add(id);
                }
            }
            DragState::PlacingStickyNote { start, .. } => {
                let end = snapped;
                let bounds = Rect::from_points(start, end);
                let min_size = 40.0;
                let bounds = if bounds.width < min_size || bounds.height < min_size {
                    Rect::new(start.x, start.y, 150.0, 100.0)
                } else {
                    bounds
                };
                let bg_color = STICKY_COLORS
                    .get(self.sticky_color_index % STICKY_COLORS.len())
                    .copied()
                    .unwrap_or(MOCHA_YELLOW);
                let content = if self.text_input_buffer.is_empty() {
                    String::new()
                } else {
                    let c = self.text_input_buffer.clone();
                    self.text_input_buffer.clear();
                    c
                };
                self.add_shape(ShapeKind::StickyNote {
                    bounds,
                    content,
                    bg_color,
                });
            }
            DragState::Panning { .. } | DragState::None => {}
        }
    }

    /// Start panning the canvas (middle mouse or space+drag).
    pub fn start_pan(&mut self, sx: f32, sy: f32) {
        self.drag = DragState::Panning {
            last_x: sx,
            last_y: sy,
        };
    }

    /// Handle scroll for zoom.
    pub fn on_scroll(&mut self, _sx: f32, _sy: f32, delta_y: f32) {
        if delta_y > 0.0 {
            self.zoom_in();
        } else if delta_y < 0.0 {
            self.zoom_out();
        }
    }

    // ========================================================================
    // Hit testing
    // ========================================================================

    /// Find the topmost visible shape under a canvas point.
    fn hit_test_shapes(&self, cx: f32, cy: f32) -> Option<ShapeId> {
        let page = self.current_page();
        let visible = page.visible_shapes();
        // Check in reverse order so topmost shape is found first.
        for shape in visible.iter().rev() {
            if shape.hit_test(cx, cy) {
                return Some(shape.id);
            }
        }
        None
    }

    /// Erase any shape at the given canvas coordinates.
    fn erase_at(&mut self, cx: f32, cy: f32) {
        let page = self.current_page();
        let hit = page
            .visible_shapes()
            .iter()
            .rev()
            .find(|s| s.hit_test(cx, cy))
            .map(|s| s.id);
        if let Some(id) = hit {
            // Only erase shapes on unlocked layers
            let on_locked = self
                .current_page()
                .get_shape(id)
                .and_then(|s| {
                    self.current_page()
                        .layers
                        .iter()
                        .find(|l| l.id == s.layer_id)
                })
                .map(|l| l.locked)
                .unwrap_or(false);
            if !on_locked {
                let shape_clone = self.current_page().get_shape(id).cloned();
                let page = self.current_page_mut();
                if let Some(idx) = page.find_shape_index(id) {
                    page.shapes.remove(idx);
                }
                if let Some(shape) = shape_clone {
                    self.push_action(Action::DeleteShape(shape.id));
                }
            }
        }
    }

    // ========================================================================
    // Stroke property setters
    // ========================================================================

    pub fn set_stroke_color(&mut self, color: Color) {
        self.stroke_props.color = color;
    }

    pub fn set_stroke_thickness(&mut self, thickness: u8) {
        self.stroke_props.thickness = thickness.clamp(1, MAX_THICKNESS);
    }

    pub fn set_stroke_opacity(&mut self, opacity: f32) {
        self.stroke_props.opacity = opacity.clamp(0.0, 1.0);
    }

    pub fn toggle_stroke_style(&mut self) {
        self.stroke_props.style = match self.stroke_props.style {
            StrokeStyle::Solid => StrokeStyle::Dashed,
            StrokeStyle::Dashed => StrokeStyle::Solid,
        };
    }

    pub fn set_custom_color(&mut self) {
        self.stroke_props.color = Color::rgb(self.custom_r, self.custom_g, self.custom_b);
    }

    // ========================================================================
    // Export (SVG-like text representation)
    // ========================================================================

    /// Export the current page as an SVG-like text representation.
    pub fn export_svg_text(&self) -> String {
        let page = self.current_page();
        let mut out = String::new();
        out.push_str("<whiteboard>\n");
        out.push_str(&format!("  <page name=\"{}\">\n", page.name));

        for layer in &page.layers {
            out.push_str(&format!(
                "    <layer name=\"{}\" visible=\"{}\" locked=\"{}\" opacity=\"{:.2}\">\n",
                layer.name, layer.visible, layer.locked, layer.opacity
            ));

            for shape in &page.shapes {
                if shape.layer_id != layer.id {
                    continue;
                }
                let color = shape.stroke.effective_color();
                let color_str = format!(
                    "rgba({},{},{},{:.2})",
                    color.r, color.g, color.b,
                    color.a as f32 / 255.0
                );
                let thickness = shape.stroke.thickness;
                let dash = match shape.stroke.style {
                    StrokeStyle::Solid => "none",
                    StrokeStyle::Dashed => "5,5",
                };

                match &shape.kind {
                    ShapeKind::Freehand { points } => {
                        let pts: Vec<String> = points
                            .iter()
                            .map(|p| format!("{:.1},{:.1}", p.x, p.y))
                            .collect();
                        out.push_str(&format!(
                            "      <path stroke=\"{}\" stroke-width=\"{}\" \
                             stroke-dasharray=\"{}\" points=\"{}\" />\n",
                            color_str,
                            thickness,
                            dash,
                            pts.join(" ")
                        ));
                    }
                    ShapeKind::Line { start, end } => {
                        out.push_str(&format!(
                            "      <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
                             stroke=\"{}\" stroke-width=\"{}\" stroke-dasharray=\"{}\" />\n",
                            start.x, start.y, end.x, end.y, color_str, thickness, dash
                        ));
                    }
                    ShapeKind::Rectangle { bounds } => {
                        out.push_str(&format!(
                            "      <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" \
                             height=\"{:.1}\" stroke=\"{}\" stroke-width=\"{}\" \
                             stroke-dasharray=\"{}\" />\n",
                            bounds.x,
                            bounds.y,
                            bounds.width,
                            bounds.height,
                            color_str,
                            thickness,
                            dash
                        ));
                    }
                    ShapeKind::Ellipse { bounds } => {
                        let cx = bounds.x + bounds.width / 2.0;
                        let cy = bounds.y + bounds.height / 2.0;
                        let rx = bounds.width / 2.0;
                        let ry = bounds.height / 2.0;
                        out.push_str(&format!(
                            "      <ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" \
                             ry=\"{:.1}\" stroke=\"{}\" stroke-width=\"{}\" \
                             stroke-dasharray=\"{}\" />\n",
                            cx, cy, rx, ry, color_str, thickness, dash
                        ));
                    }
                    ShapeKind::Arrow { start, end } => {
                        out.push_str(&format!(
                            "      <arrow x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" \
                             stroke=\"{}\" stroke-width=\"{}\" stroke-dasharray=\"{}\" />\n",
                            start.x, start.y, end.x, end.y, color_str, thickness, dash
                        ));
                    }
                    ShapeKind::TextLabel { position, content } => {
                        out.push_str(&format!(
                            "      <text x=\"{:.1}\" y=\"{:.1}\" fill=\"{}\">{}</text>\n",
                            position.x, position.y, color_str, content
                        ));
                    }
                    ShapeKind::StickyNote {
                        bounds,
                        content,
                        bg_color,
                    } => {
                        out.push_str(&format!(
                            "      <sticky x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" \
                             height=\"{:.1}\" fill=\"rgba({},{},{},1.00)\">{}</sticky>\n",
                            bounds.x,
                            bounds.y,
                            bounds.width,
                            bounds.height,
                            bg_color.r,
                            bg_color.g,
                            bg_color.b,
                            content
                        ));
                    }
                }
            }
            out.push_str("    </layer>\n");
        }
        out.push_str("  </page>\n");
        out.push_str("</whiteboard>\n");
        out
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the entire UI to a list of render commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.win_width,
            height: self.win_height,
            color: MOCHA_CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        self.render_top_bar(&mut cmds);
        self.render_page_tabs(&mut cmds);
        self.render_toolbar(&mut cmds);
        self.render_canvas(&mut cmds);
        if self.show_layers_panel {
            self.render_layers_panel(&mut cmds);
        }
        self.render_status_bar(&mut cmds);

        cmds
    }

    // ------ Top bar ------

    fn render_top_bar(&self, cmds: &mut Vec<RenderCommand>) {
        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.win_width,
            height: TOP_BAR_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Whiteboard".to_string(),
            color: MOCHA_TEXT,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Stroke info
        let thickness_label = format!("{}px", self.stroke_props.thickness);
        cmds.push(RenderCommand::Text {
            x: 130.0,
            y: 14.0,
            text: thickness_label,
            color: MOCHA_SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Opacity label
        let opacity_label = format!("{}%", (self.stroke_props.opacity * 100.0) as u32);
        cmds.push(RenderCommand::Text {
            x: 180.0,
            y: 14.0,
            text: opacity_label,
            color: MOCHA_SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Stroke style indicator
        let style_label = match self.stroke_props.style {
            StrokeStyle::Solid => "Solid",
            StrokeStyle::Dashed => "Dashed",
        };
        cmds.push(RenderCommand::Text {
            x: 230.0,
            y: 14.0,
            text: style_label.to_string(),
            color: MOCHA_SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Grid toggle indicator
        let grid_label = if self.show_grid { "Grid:ON" } else { "Grid:OFF" };
        cmds.push(RenderCommand::Text {
            x: 300.0,
            y: 14.0,
            text: grid_label.to_string(),
            color: if self.show_grid {
                MOCHA_GREEN
            } else {
                MOCHA_OVERLAY0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Snap indicator
        let snap_label = if self.snap_to_grid {
            "Snap:ON"
        } else {
            "Snap:OFF"
        };
        cmds.push(RenderCommand::Text {
            x: 380.0,
            y: 14.0,
            text: snap_label.to_string(),
            color: if self.snap_to_grid {
                MOCHA_BLUE
            } else {
                MOCHA_OVERLAY0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Current color swatch
        cmds.push(RenderCommand::FillRect {
            x: 460.0,
            y: 8.0,
            width: 24.0,
            height: 24.0,
            color: self.stroke_props.effective_color(),
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::StrokeRect {
            x: 460.0,
            y: 8.0,
            width: 24.0,
            height: 24.0,
            color: MOCHA_SURFACE1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Zoom display
        let zoom_pct = format!("{:.0}%", self.zoom * 100.0);
        cmds.push(RenderCommand::Text {
            x: 500.0,
            y: 14.0,
            text: zoom_pct,
            color: MOCHA_SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOP_BAR_HEIGHT,
            x2: self.win_width,
            y2: TOP_BAR_HEIGHT,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
    }

    // ------ Page tabs ------

    fn render_page_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOP_BAR_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.win_width,
            height: PAGE_TAB_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        let mut tx = TOOLBAR_WIDTH + 4.0;
        for (i, page) in self.pages.iter().enumerate() {
            let is_active = i == self.active_page;
            let tab_width = (page.name.len() as f32 * 8.0).max(60.0) + 16.0;

            let bg = if is_active { MOCHA_BASE } else { MOCHA_SURFACE0 };
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: y + 2.0,
                width: tab_width,
                height: PAGE_TAB_HEIGHT - 2.0,
                color: bg,
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            let text_color = if is_active { MOCHA_TEXT } else { MOCHA_SUBTEXT0 };
            cmds.push(RenderCommand::Text {
                x: tx + 8.0,
                y: y + 8.0,
                text: page.name.clone(),
                color: text_color,
                font_size: 12.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(tab_width - 16.0),
            });

            tx += tab_width + 4.0;
        }

        // "+" button to add page
        cmds.push(RenderCommand::FillRect {
            x: tx,
            y: y + 4.0,
            width: 24.0,
            height: 20.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: tx + 7.0,
            y: y + 7.0,
            text: "+".to_string(),
            color: MOCHA_TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + PAGE_TAB_HEIGHT,
            x2: self.win_width,
            y2: y + PAGE_TAB_HEIGHT,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
    }

    // ------ Left toolbar ------

    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        let y_start = TOP_BAR_HEIGHT + PAGE_TAB_HEIGHT;
        let panel_height = self.win_height - y_start - STATUS_BAR_HEIGHT;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: y_start,
            width: TOOLBAR_WIDTH,
            height: panel_height,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Tool buttons
        let mut ty = y_start + 8.0;
        for tool in Tool::all() {
            let is_active = *tool == self.current_tool;
            let bg = if is_active {
                MOCHA_BLUE
            } else {
                MOCHA_SURFACE0
            };
            let fg = if is_active { MOCHA_CRUST } else { MOCHA_TEXT };

            cmds.push(RenderCommand::FillRect {
                x: 6.0,
                y: ty,
                width: 40.0,
                height: 32.0,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: 10.0,
                y: ty + 10.0,
                text: tool.label().to_string(),
                color: fg,
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(36.0),
            });

            ty += 38.0;
        }

        // Color palette below tools
        ty += 8.0;
        cmds.push(RenderCommand::Line {
            x1: 6.0,
            y1: ty,
            x2: 46.0,
            y2: ty,
            color: MOCHA_SURFACE1,
            width: 1.0,
        });
        ty += 8.0;

        // Title
        cmds.push(RenderCommand::Text {
            x: 6.0,
            y: ty,
            text: "Colors".to_string(),
            color: MOCHA_SUBTEXT0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        ty += 16.0;

        // 2-column palette swatches
        for (i, color) in PALETTE_COLORS.iter().enumerate() {
            let col = i % 2;
            let row = i / 2;
            let sx = 6.0 + col as f32 * (PALETTE_SWATCH_SIZE + PALETTE_GAP);
            let sy = ty + row as f32 * (PALETTE_SWATCH_SIZE + PALETTE_GAP);

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: sy,
                width: PALETTE_SWATCH_SIZE,
                height: PALETTE_SWATCH_SIZE,
                color: *color,
                corner_radii: CornerRadii::all(3.0),
            });

            // Highlight the active color
            if *color == self.stroke_props.color {
                cmds.push(RenderCommand::StrokeRect {
                    x: sx - 1.0,
                    y: sy - 1.0,
                    width: PALETTE_SWATCH_SIZE + 2.0,
                    height: PALETTE_SWATCH_SIZE + 2.0,
                    color: MOCHA_TEXT,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // Right border
        cmds.push(RenderCommand::Line {
            x1: TOOLBAR_WIDTH,
            y1: y_start,
            x2: TOOLBAR_WIDTH,
            y2: y_start + panel_height,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
    }

    // ------ Canvas ------

    fn render_canvas(&self, cmds: &mut Vec<RenderCommand>) {
        let area = self.canvas_rect();

        // Canvas background
        cmds.push(RenderCommand::FillRect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
            color: MOCHA_BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Clip to canvas area
        cmds.push(RenderCommand::PushClip {
            x: area.x,
            y: area.y,
            width: area.width,
            height: area.height,
        });

        // Grid
        if self.show_grid {
            self.render_grid(cmds, &area);
        }

        // Push translate for pan/zoom
        cmds.push(RenderCommand::PushTranslate {
            dx: area.x + self.pan_x,
            dy: area.y + self.pan_y,
        });

        // Render shapes by layer order
        let page = self.current_page();
        for layer in &page.layers {
            if !layer.visible {
                continue;
            }
            for shape in &page.shapes {
                if shape.layer_id != layer.id {
                    continue;
                }
                let is_selected = self.selection.contains(shape.id);
                self.render_shape(cmds, shape, is_selected);
            }
        }

        // Render in-progress drawing
        self.render_drag_preview(cmds);

        cmds.push(RenderCommand::PopTranslate);

        // Selection marquee overlay (screen space inside clip)
        if let DragState::Marquee { start, current } = &self.drag {
            let r = Rect::from_points(*start, *current);
            let screen_start = self.canvas_to_screen(r.x, r.y);
            let screen_end =
                self.canvas_to_screen(r.x + r.width, r.y + r.height);
            cmds.push(RenderCommand::FillRect {
                x: screen_start.x,
                y: screen_start.y,
                width: screen_end.x - screen_start.x,
                height: screen_end.y - screen_start.y,
                color: Color::rgba(137, 180, 250, 40),
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::StrokeRect {
                x: screen_start.x,
                y: screen_start.y,
                width: screen_end.x - screen_start.x,
                height: screen_end.y - screen_start.y,
                color: MOCHA_BLUE,
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    fn render_grid(&self, cmds: &mut Vec<RenderCommand>, area: &Rect) {
        let grid_color = Color::rgba(69, 71, 90, 60);
        let scaled_grid = GRID_SIZE * self.zoom;

        if scaled_grid < 4.0 {
            // Grid too dense to show
            return;
        }

        let start_x = self.pan_x % scaled_grid;
        let start_y = self.pan_y % scaled_grid;

        let mut gx = start_x;
        while gx < area.width {
            cmds.push(RenderCommand::Line {
                x1: area.x + gx,
                y1: area.y,
                x2: area.x + gx,
                y2: area.y + area.height,
                color: grid_color,
                width: 0.5,
            });
            gx += scaled_grid;
        }

        let mut gy = start_y;
        while gy < area.height {
            cmds.push(RenderCommand::Line {
                x1: area.x,
                y1: area.y + gy,
                x2: area.x + area.width,
                y2: area.y + gy,
                color: grid_color,
                width: 0.5,
            });
            gy += scaled_grid;
        }
    }

    fn render_shape(
        &self,
        cmds: &mut Vec<RenderCommand>,
        shape: &Shape,
        selected: bool,
    ) {
        let color = shape.stroke.effective_color();
        let lw = shape.stroke.thickness as f32 * self.zoom;

        match &shape.kind {
            ShapeKind::Freehand { points } => {
                for window in points.windows(2) {
                    if let (Some(a), Some(b)) = (window.first(), window.get(1)) {
                        cmds.push(RenderCommand::Line {
                            x1: a.x * self.zoom,
                            y1: a.y * self.zoom,
                            x2: b.x * self.zoom,
                            y2: b.y * self.zoom,
                            color,
                            width: lw,
                        });
                    }
                }
            }
            ShapeKind::Line { start, end } => {
                cmds.push(RenderCommand::Line {
                    x1: start.x * self.zoom,
                    y1: start.y * self.zoom,
                    x2: end.x * self.zoom,
                    y2: end.y * self.zoom,
                    color,
                    width: lw,
                });
            }
            ShapeKind::Rectangle { bounds } => {
                cmds.push(RenderCommand::StrokeRect {
                    x: bounds.x * self.zoom,
                    y: bounds.y * self.zoom,
                    width: bounds.width * self.zoom,
                    height: bounds.height * self.zoom,
                    color,
                    line_width: lw,
                    corner_radii: CornerRadii::ZERO,
                });
            }
            ShapeKind::Ellipse { bounds } => {
                // Approximate ellipse with a stroke rect with full corner radii.
                let rx = bounds.width * self.zoom / 2.0;
                let ry = bounds.height * self.zoom / 2.0;
                let r = rx.min(ry);
                cmds.push(RenderCommand::StrokeRect {
                    x: bounds.x * self.zoom,
                    y: bounds.y * self.zoom,
                    width: bounds.width * self.zoom,
                    height: bounds.height * self.zoom,
                    color,
                    line_width: lw,
                    corner_radii: CornerRadii::all(r),
                });
            }
            ShapeKind::Arrow { start, end } => {
                // Shaft
                cmds.push(RenderCommand::Line {
                    x1: start.x * self.zoom,
                    y1: start.y * self.zoom,
                    x2: end.x * self.zoom,
                    y2: end.y * self.zoom,
                    color,
                    width: lw,
                });
                // Arrowhead (two short lines)
                let dx = end.x - start.x;
                let dy = end.y - start.y;
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.001 {
                    let ux = dx / len;
                    let uy = dy / len;
                    let head_len = 12.0;
                    let head_w = 6.0;
                    let bx = end.x - ux * head_len;
                    let by = end.y - uy * head_len;
                    let lx = bx - uy * head_w;
                    let ly = by + ux * head_w;
                    let rx = bx + uy * head_w;
                    let ry_val = by - ux * head_w;
                    cmds.push(RenderCommand::Line {
                        x1: end.x * self.zoom,
                        y1: end.y * self.zoom,
                        x2: lx * self.zoom,
                        y2: ly * self.zoom,
                        color,
                        width: lw,
                    });
                    cmds.push(RenderCommand::Line {
                        x1: end.x * self.zoom,
                        y1: end.y * self.zoom,
                        x2: rx * self.zoom,
                        y2: ry_val * self.zoom,
                        color,
                        width: lw,
                    });
                }
            }
            ShapeKind::TextLabel { position, content } => {
                cmds.push(RenderCommand::Text {
                    x: position.x * self.zoom,
                    y: position.y * self.zoom,
                    text: content.clone(),
                    color,
                    font_size: 14.0 * self.zoom,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }
            ShapeKind::StickyNote {
                bounds,
                content,
                bg_color,
            } => {
                // Sticky note shadow
                cmds.push(RenderCommand::BoxShadow {
                    x: bounds.x * self.zoom,
                    y: bounds.y * self.zoom,
                    width: bounds.width * self.zoom,
                    height: bounds.height * self.zoom,
                    offset_x: 2.0,
                    offset_y: 2.0,
                    blur: 8.0,
                    spread: 0.0,
                    color: Color::rgba(0, 0, 0, 80),
                    corner_radii: CornerRadii::all(4.0),
                });
                // Sticky note background
                cmds.push(RenderCommand::FillRect {
                    x: bounds.x * self.zoom,
                    y: bounds.y * self.zoom,
                    width: bounds.width * self.zoom,
                    height: bounds.height * self.zoom,
                    color: *bg_color,
                    corner_radii: CornerRadii::all(4.0),
                });
                // Text (dark for readability on colored background)
                if !content.is_empty() {
                    cmds.push(RenderCommand::Text {
                        x: (bounds.x + 8.0) * self.zoom,
                        y: (bounds.y + 8.0) * self.zoom,
                        text: content.clone(),
                        color: MOCHA_CRUST,
                        font_size: 12.0 * self.zoom,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some((bounds.width - 16.0) * self.zoom),
                    });
                }
            }
        }

        // Selection highlight
        if selected {
            let bb = shape.bounding_box();
            cmds.push(RenderCommand::StrokeRect {
                x: bb.x * self.zoom - 2.0,
                y: bb.y * self.zoom - 2.0,
                width: bb.width * self.zoom + 4.0,
                height: bb.height * self.zoom + 4.0,
                color: MOCHA_BLUE,
                line_width: 1.5,
                corner_radii: CornerRadii::ZERO,
            });
        }
    }

    fn render_drag_preview(&self, cmds: &mut Vec<RenderCommand>) {
        let color = self.stroke_props.effective_color();
        let lw = self.stroke_props.thickness as f32 * self.zoom;

        match &self.drag {
            DragState::DrawingFreehand { points } => {
                for window in points.windows(2) {
                    if let (Some(a), Some(b)) = (window.first(), window.get(1)) {
                        cmds.push(RenderCommand::Line {
                            x1: a.x * self.zoom,
                            y1: a.y * self.zoom,
                            x2: b.x * self.zoom,
                            y2: b.y * self.zoom,
                            color,
                            width: lw,
                        });
                    }
                }
            }
            DragState::DrawingShape { start, current } => match self.current_tool {
                Tool::Line => {
                    cmds.push(RenderCommand::Line {
                        x1: start.x * self.zoom,
                        y1: start.y * self.zoom,
                        x2: current.x * self.zoom,
                        y2: current.y * self.zoom,
                        color,
                        width: lw,
                    });
                }
                Tool::Rectangle => {
                    let r = Rect::from_points(*start, *current);
                    cmds.push(RenderCommand::StrokeRect {
                        x: r.x * self.zoom,
                        y: r.y * self.zoom,
                        width: r.width * self.zoom,
                        height: r.height * self.zoom,
                        color,
                        line_width: lw,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
                Tool::Ellipse => {
                    let r = Rect::from_points(*start, *current);
                    let rx = r.width * self.zoom / 2.0;
                    let ry = r.height * self.zoom / 2.0;
                    let rad = rx.min(ry);
                    cmds.push(RenderCommand::StrokeRect {
                        x: r.x * self.zoom,
                        y: r.y * self.zoom,
                        width: r.width * self.zoom,
                        height: r.height * self.zoom,
                        color,
                        line_width: lw,
                        corner_radii: CornerRadii::all(rad),
                    });
                }
                Tool::Arrow => {
                    cmds.push(RenderCommand::Line {
                        x1: start.x * self.zoom,
                        y1: start.y * self.zoom,
                        x2: current.x * self.zoom,
                        y2: current.y * self.zoom,
                        color,
                        width: lw,
                    });
                }
                _ => {}
            },
            DragState::PlacingStickyNote { start, current } => {
                let r = Rect::from_points(*start, *current);
                let bg = STICKY_COLORS
                    .get(self.sticky_color_index % STICKY_COLORS.len())
                    .copied()
                    .unwrap_or(MOCHA_YELLOW);
                cmds.push(RenderCommand::FillRect {
                    x: r.x * self.zoom,
                    y: r.y * self.zoom,
                    width: r.width * self.zoom,
                    height: r.height * self.zoom,
                    color: Color::rgba(bg.r, bg.g, bg.b, 150),
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            _ => {}
        }
    }

    // ------ Right layers panel ------

    fn render_layers_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let canvas_area = self.canvas_rect();
        let panel_x = canvas_area.x + canvas_area.width;
        let panel_y = TOP_BAR_HEIGHT + PAGE_TAB_HEIGHT;
        let panel_h = self.win_height - panel_y - STATUS_BAR_HEIGHT;

        // Background
        cmds.push(RenderCommand::FillRect {
            x: panel_x,
            y: panel_y,
            width: RIGHT_PANEL_WIDTH,
            height: panel_h,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Title
        cmds.push(RenderCommand::Text {
            x: panel_x + 8.0,
            y: panel_y + 8.0,
            text: "Layers".to_string(),
            color: MOCHA_TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // "+" add layer button
        cmds.push(RenderCommand::FillRect {
            x: panel_x + RIGHT_PANEL_WIDTH - 30.0,
            y: panel_y + 4.0,
            width: 22.0,
            height: 22.0,
            color: MOCHA_SURFACE0,
            corner_radii: CornerRadii::all(4.0),
        });
        cmds.push(RenderCommand::Text {
            x: panel_x + RIGHT_PANEL_WIDTH - 25.0,
            y: panel_y + 7.0,
            text: "+".to_string(),
            color: MOCHA_TEXT,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });

        // Layer rows
        let page = self.current_page();
        let mut ly = panel_y + 30.0;
        for layer in page.layers.iter().rev() {
            let is_active = layer.id == self.active_layer_id;
            let row_bg = if is_active {
                MOCHA_SURFACE0
            } else {
                MOCHA_MANTLE
            };
            cmds.push(RenderCommand::FillRect {
                x: panel_x + 4.0,
                y: ly,
                width: RIGHT_PANEL_WIDTH - 8.0,
                height: LAYER_ROW_HEIGHT,
                color: row_bg,
                corner_radii: CornerRadii::all(4.0),
            });

            // Visibility icon
            let vis_color = if layer.visible {
                MOCHA_GREEN
            } else {
                MOCHA_OVERLAY0
            };
            cmds.push(RenderCommand::Text {
                x: panel_x + 10.0,
                y: ly + 9.0,
                text: if layer.visible { "V" } else { "-" }.to_string(),
                color: vis_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
            });

            // Lock icon
            let lock_color = if layer.locked {
                MOCHA_RED
            } else {
                MOCHA_OVERLAY0
            };
            cmds.push(RenderCommand::Text {
                x: panel_x + 26.0,
                y: ly + 9.0,
                text: if layer.locked { "L" } else { "." }.to_string(),
                color: lock_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            // Layer name
            cmds.push(RenderCommand::Text {
                x: panel_x + 42.0,
                y: ly + 9.0,
                text: layer.name.clone(),
                color: MOCHA_TEXT,
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(RIGHT_PANEL_WIDTH - 80.0),
            });

            // Opacity indicator
            let opacity_str = format!("{:.0}%", layer.opacity * 100.0);
            cmds.push(RenderCommand::Text {
                x: panel_x + RIGHT_PANEL_WIDTH - 40.0,
                y: ly + 9.0,
                text: opacity_str,
                color: MOCHA_SUBTEXT0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            ly += LAYER_ROW_HEIGHT + 2.0;
        }

        // Left border
        cmds.push(RenderCommand::Line {
            x1: panel_x,
            y1: panel_y,
            x2: panel_x,
            y2: panel_y + panel_h,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });
    }

    // ------ Status bar ------

    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.win_height - STATUS_BAR_HEIGHT;

        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.win_width,
            height: STATUS_BAR_HEIGHT,
            color: MOCHA_MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Top border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.win_width,
            y2: y,
            color: MOCHA_SURFACE0,
            width: 1.0,
        });

        // Tool name
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: y + 6.0,
            text: format!("Tool: {}", self.current_tool.label()),
            color: MOCHA_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Shape count
        let shape_count = self.current_page().shapes.len();
        cmds.push(RenderCommand::Text {
            x: 120.0,
            y: y + 6.0,
            text: format!("Shapes: {}", shape_count),
            color: MOCHA_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Selection count
        let sel_count = self.selection.shape_ids.len();
        if sel_count > 0 {
            cmds.push(RenderCommand::Text {
                x: 240.0,
                y: y + 6.0,
                text: format!("Selected: {}", sel_count),
                color: MOCHA_BLUE,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // Layer info
        let layer_name = self
            .current_page()
            .layers
            .iter()
            .find(|l| l.id == self.active_layer_id)
            .map(|l| l.name.clone())
            .unwrap_or_else(|| "?".to_string());
        cmds.push(RenderCommand::Text {
            x: 360.0,
            y: y + 6.0,
            text: format!("Layer: {}", layer_name),
            color: MOCHA_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Undo/redo counts
        cmds.push(RenderCommand::Text {
            x: 500.0,
            y: y + 6.0,
            text: format!(
                "Undo:{} Redo:{}",
                self.undo_stack.len(),
                self.redo_stack.len()
            ),
            color: MOCHA_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Page info
        cmds.push(RenderCommand::Text {
            x: 640.0,
            y: y + 6.0,
            text: format!(
                "Page {}/{}",
                self.active_page.saturating_add(1_usize),
                self.pages.len()
            ),
            color: MOCHA_SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let _app = WhiteboardApp::new(1280.0, 800.0);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Construction ----

    #[test]
    fn test_new_app_defaults() {
        let app = WhiteboardApp::new(1280.0, 800.0);
        assert_eq!(app.win_width, 1280.0);
        assert_eq!(app.win_height, 800.0);
        assert_eq!(app.zoom, 1.0);
        assert_eq!(app.pan_x, 0.0);
        assert_eq!(app.pan_y, 0.0);
        assert_eq!(app.current_tool, Tool::Pen);
        assert!(app.show_grid);
        assert!(!app.snap_to_grid);
        assert_eq!(app.pages.len(), 1);
        assert_eq!(app.active_page, 0);
    }

    #[test]
    fn test_new_app_has_one_layer() {
        let app = WhiteboardApp::new(800.0, 600.0);
        assert_eq!(app.current_page().layers.len(), 1);
        assert_eq!(app.current_page().layers[0].name, "Layer 1");
        assert!(app.current_page().layers[0].visible);
        assert!(!app.current_page().layers[0].locked);
    }

    #[test]
    fn test_new_app_empty_shapes() {
        let app = WhiteboardApp::new(800.0, 600.0);
        assert!(app.current_page().shapes.is_empty());
    }

    // ---- Point and Rect ----

    #[test]
    fn test_point_distance() {
        let a = Point::new(0.0, 0.0);
        let b = Point::new(3.0, 4.0);
        let dist = a.distance_to(b);
        assert!((dist - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_rect_from_points() {
        let r = Rect::from_points(Point::new(10.0, 20.0), Point::new(50.0, 60.0));
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 20.0);
        assert_eq!(r.width, 40.0);
        assert_eq!(r.height, 40.0);
    }

    #[test]
    fn test_rect_from_points_reversed() {
        let r = Rect::from_points(Point::new(50.0, 60.0), Point::new(10.0, 20.0));
        assert_eq!(r.x, 10.0);
        assert_eq!(r.y, 20.0);
    }

    #[test]
    fn test_rect_contains() {
        let r = Rect::new(10.0, 10.0, 100.0, 50.0);
        assert!(r.contains(50.0, 30.0));
        assert!(!r.contains(5.0, 5.0));
        assert!(r.contains(10.0, 10.0));
        assert!(r.contains(110.0, 60.0));
    }

    #[test]
    fn test_rect_intersects() {
        let a = Rect::new(0.0, 0.0, 100.0, 100.0);
        let b = Rect::new(50.0, 50.0, 100.0, 100.0);
        assert!(a.intersects(&b));
        let c = Rect::new(200.0, 200.0, 10.0, 10.0);
        assert!(!a.intersects(&c));
    }

    #[test]
    fn test_rect_center() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        let c = r.center();
        assert!((c.x - 60.0).abs() < 0.01);
        assert!((c.y - 45.0).abs() < 0.01);
    }

    #[test]
    fn test_rect_right_bottom() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.right(), 40.0);
        assert_eq!(r.bottom(), 60.0);
    }

    // ---- Point to segment distance ----

    #[test]
    fn test_point_to_segment_on_segment() {
        let d = point_to_segment_distance(5.0, 0.0, 0.0, 0.0, 10.0, 0.0);
        assert!(d < 0.01);
    }

    #[test]
    fn test_point_to_segment_perpendicular() {
        let d = point_to_segment_distance(5.0, 3.0, 0.0, 0.0, 10.0, 0.0);
        assert!((d - 3.0).abs() < 0.01);
    }

    #[test]
    fn test_point_to_segment_endpoint() {
        let d = point_to_segment_distance(0.0, 0.0, 3.0, 4.0, 3.0, 4.0);
        assert!((d - 5.0).abs() < 0.01);
    }

    // ---- Stroke props ----

    #[test]
    fn test_stroke_effective_color_full_opacity() {
        let s = StrokeProps::default();
        let c = s.effective_color();
        assert_eq!(c.a, 255);
    }

    #[test]
    fn test_stroke_effective_color_half_opacity() {
        let s = StrokeProps {
            opacity: 0.5,
            ..StrokeProps::default()
        };
        let c = s.effective_color();
        assert_eq!(c.a, 127);
    }

    #[test]
    fn test_stroke_effective_color_zero_opacity() {
        let s = StrokeProps {
            opacity: 0.0,
            ..StrokeProps::default()
        };
        let c = s.effective_color();
        assert_eq!(c.a, 0);
    }

    // ---- Tool enumeration ----

    #[test]
    fn test_tool_all_count() {
        assert_eq!(Tool::all().len(), 9);
    }

    #[test]
    fn test_tool_labels_not_empty() {
        for tool in Tool::all() {
            assert!(!tool.label().is_empty());
        }
    }

    #[test]
    fn test_tool_shortcuts_unique() {
        let shortcuts: Vec<char> = Tool::all()
            .iter()
            .filter_map(|t| t.shortcut())
            .collect();
        for (i, a) in shortcuts.iter().enumerate() {
            for b in shortcuts.iter().skip(i + 1) {
                assert_ne!(a, b, "Duplicate shortcut");
            }
        }
    }

    // ---- Zoom ----

    #[test]
    fn test_zoom_in() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.zoom_in();
        assert!(app.zoom > 1.0);
    }

    #[test]
    fn test_zoom_out() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.zoom_out();
        assert!(app.zoom < 1.0);
    }

    #[test]
    fn test_zoom_clamp_min() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_zoom(0.001);
        assert!((app.zoom - MIN_ZOOM).abs() < 0.001);
    }

    #[test]
    fn test_zoom_clamp_max() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_zoom(999.0);
        assert!((app.zoom - MAX_ZOOM).abs() < 0.001);
    }

    #[test]
    fn test_zoom_to_fit_resets() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_zoom(3.0);
        app.pan_x = 100.0;
        app.pan_y = -50.0;
        app.zoom_to_fit();
        assert_eq!(app.zoom, 1.0);
        assert_eq!(app.pan_x, 0.0);
        assert_eq!(app.pan_y, 0.0);
    }

    // ---- Snap ----

    #[test]
    fn test_snap_disabled() {
        let app = WhiteboardApp::new(800.0, 600.0);
        let p = app.snap_point(Point::new(13.0, 17.0));
        assert_eq!(p.x, 13.0);
        assert_eq!(p.y, 17.0);
    }

    #[test]
    fn test_snap_enabled() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.snap_to_grid = true;
        let p = app.snap_point(Point::new(13.0, 17.0));
        assert_eq!(p.x, 20.0);
        assert_eq!(p.y, 20.0);
    }

    #[test]
    fn test_snap_at_grid_boundary() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.snap_to_grid = true;
        let p = app.snap_point(Point::new(40.0, 60.0));
        assert_eq!(p.x, 40.0);
        assert_eq!(p.y, 60.0);
    }

    // ---- Coordinate transforms ----

    #[test]
    fn test_screen_to_canvas_identity() {
        let app = WhiteboardApp::new(1280.0, 800.0);
        let area = app.canvas_rect();
        let p = app.screen_to_canvas(area.x, area.y);
        assert!((p.x).abs() < 0.01);
        assert!((p.y).abs() < 0.01);
    }

    #[test]
    fn test_canvas_to_screen_roundtrip() {
        let app = WhiteboardApp::new(1280.0, 800.0);
        let canvas_p = Point::new(100.0, 200.0);
        let screen_p = app.canvas_to_screen(canvas_p.x, canvas_p.y);
        let back = app.screen_to_canvas(screen_p.x, screen_p.y);
        assert!((back.x - canvas_p.x).abs() < 0.01);
        assert!((back.y - canvas_p.y).abs() < 0.01);
    }

    #[test]
    fn test_screen_to_canvas_with_zoom() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.zoom = 2.0;
        let area = app.canvas_rect();
        let p = app.screen_to_canvas(area.x + 100.0, area.y + 50.0);
        assert!((p.x - 50.0).abs() < 0.01);
        assert!((p.y - 25.0).abs() < 0.01);
    }

    // ---- Shape creation ----

    #[test]
    fn test_add_freehand_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let points = vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)];
        let id = app.add_shape(ShapeKind::Freehand { points });
        assert_eq!(app.current_page().shapes.len(), 1);
        assert_eq!(app.current_page().shapes[0].id, id);
    }

    #[test]
    fn test_add_line_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 100.0),
        });
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_add_rectangle_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 50.0, 30.0),
        });
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_add_ellipse_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Ellipse {
            bounds: Rect::new(10.0, 10.0, 80.0, 40.0),
        });
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_add_arrow_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Arrow {
            start: Point::new(0.0, 0.0),
            end: Point::new(80.0, 40.0),
        });
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_add_text_label() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::TextLabel {
            position: Point::new(50.0, 50.0),
            content: "Hello".to_string(),
        });
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_add_sticky_note() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::StickyNote {
            bounds: Rect::new(50.0, 50.0, 150.0, 100.0),
            content: "Note".to_string(),
            bg_color: MOCHA_YELLOW,
        });
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_shape_ids_increment() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id1 = app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        let id2 = app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(20.0, 20.0),
        });
        assert_ne!(id1, id2);
        assert!(id2 > id1);
    }

    // ---- Shape bounding box ----

    #[test]
    fn test_freehand_bounding_box() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Freehand {
                points: vec![
                    Point::new(10.0, 20.0),
                    Point::new(50.0, 60.0),
                ],
            },
            stroke: StrokeProps {
                thickness: 2,
                ..StrokeProps::default()
            },
            layer_id: 1,
        };
        let bb = shape.bounding_box();
        assert!(bb.x <= 10.0);
        assert!(bb.y <= 20.0);
        assert!(bb.right() >= 50.0);
        assert!(bb.bottom() >= 60.0);
    }

    #[test]
    fn test_line_bounding_box() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Line {
                start: Point::new(0.0, 0.0),
                end: Point::new(100.0, 50.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        let bb = shape.bounding_box();
        assert!(bb.width > 100.0); // includes padding
        assert!(bb.height > 50.0);
    }

    #[test]
    fn test_rectangle_bounding_box() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Rectangle {
                bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        let bb = shape.bounding_box();
        assert!(bb.contains(25.0, 40.0));
    }

    #[test]
    fn test_empty_freehand_bounding_box() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Freehand { points: vec![] },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        let bb = shape.bounding_box();
        assert_eq!(bb.width, 0.0);
        assert_eq!(bb.height, 0.0);
    }

    // ---- Hit testing ----

    #[test]
    fn test_hit_rect_inside() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Rectangle {
                bounds: Rect::new(10.0, 10.0, 100.0, 50.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        assert!(shape.hit_test(50.0, 30.0));
    }

    #[test]
    fn test_hit_rect_outside() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Rectangle {
                bounds: Rect::new(10.0, 10.0, 100.0, 50.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        assert!(!shape.hit_test(0.0, 0.0));
    }

    #[test]
    fn test_hit_line_close() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Line {
                start: Point::new(0.0, 0.0),
                end: Point::new(100.0, 0.0),
            },
            stroke: StrokeProps {
                thickness: 4,
                ..StrokeProps::default()
            },
            layer_id: 1,
        };
        assert!(shape.hit_test(50.0, 1.0));
    }

    #[test]
    fn test_hit_line_far() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Line {
                start: Point::new(0.0, 0.0),
                end: Point::new(100.0, 0.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        assert!(!shape.hit_test(50.0, 50.0));
    }

    #[test]
    fn test_hit_ellipse_inside() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Ellipse {
                bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        assert!(shape.hit_test(50.0, 50.0));
    }

    #[test]
    fn test_hit_ellipse_outside() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Ellipse {
                bounds: Rect::new(0.0, 0.0, 100.0, 100.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        assert!(!shape.hit_test(0.0, 0.0));
    }

    #[test]
    fn test_hit_freehand_close_to_segment() {
        let shape = Shape {
            id: 1,
            kind: ShapeKind::Freehand {
                points: vec![Point::new(0.0, 0.0), Point::new(100.0, 0.0)],
            },
            stroke: StrokeProps {
                thickness: 6,
                ..StrokeProps::default()
            },
            layer_id: 1,
        };
        assert!(shape.hit_test(50.0, 2.0));
        assert!(!shape.hit_test(50.0, 20.0));
    }

    // ---- Shape translate ----

    #[test]
    fn test_translate_rectangle() {
        let mut shape = Shape {
            id: 1,
            kind: ShapeKind::Rectangle {
                bounds: Rect::new(10.0, 20.0, 30.0, 40.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        shape.translate(5.0, -3.0);
        if let ShapeKind::Rectangle { bounds } = &shape.kind {
            assert_eq!(bounds.x, 15.0);
            assert_eq!(bounds.y, 17.0);
        } else {
            panic!("Expected Rectangle");
        }
    }

    #[test]
    fn test_translate_line() {
        let mut shape = Shape {
            id: 1,
            kind: ShapeKind::Line {
                start: Point::new(0.0, 0.0),
                end: Point::new(100.0, 50.0),
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        shape.translate(10.0, 20.0);
        if let ShapeKind::Line { start, end } = &shape.kind {
            assert_eq!(start.x, 10.0);
            assert_eq!(start.y, 20.0);
            assert_eq!(end.x, 110.0);
            assert_eq!(end.y, 70.0);
        } else {
            panic!("Expected Line");
        }
    }

    #[test]
    fn test_translate_freehand() {
        let mut shape = Shape {
            id: 1,
            kind: ShapeKind::Freehand {
                points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
            },
            stroke: StrokeProps::default(),
            layer_id: 1,
        };
        shape.translate(5.0, 5.0);
        if let ShapeKind::Freehand { points } = &shape.kind {
            assert_eq!(points[0].x, 5.0);
            assert_eq!(points[1].y, 15.0);
        } else {
            panic!("Expected Freehand");
        }
    }

    // ---- Selection ----

    #[test]
    fn test_selection_empty_initially() {
        let sel = Selection::default();
        assert!(sel.is_empty());
    }

    #[test]
    fn test_selection_add() {
        let mut sel = Selection::default();
        sel.add(1);
        assert!(!sel.is_empty());
        assert!(sel.contains(1));
    }

    #[test]
    fn test_selection_add_no_duplicate() {
        let mut sel = Selection::default();
        sel.add(1);
        sel.add(1);
        assert_eq!(sel.shape_ids.len(), 1);
    }

    #[test]
    fn test_selection_toggle() {
        let mut sel = Selection::default();
        sel.toggle(1);
        assert!(sel.contains(1));
        sel.toggle(1);
        assert!(!sel.contains(1));
    }

    #[test]
    fn test_selection_clear() {
        let mut sel = Selection::default();
        sel.add(1);
        sel.add(2);
        sel.clear();
        assert!(sel.is_empty());
    }

    // ---- Undo / Redo ----

    #[test]
    fn test_undo_add_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        assert_eq!(app.current_page().shapes.len(), 1);
        app.undo();
        assert_eq!(app.current_page().shapes.len(), 0);
    }

    #[test]
    fn test_redo_after_undo() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        app.undo();
        assert_eq!(app.current_page().shapes.len(), 0);
        app.redo();
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_undo_empty_stack() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        // Should not panic
        app.undo();
        assert!(app.current_page().shapes.is_empty());
    }

    #[test]
    fn test_redo_empty_stack() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.redo();
        assert!(app.current_page().shapes.is_empty());
    }

    #[test]
    fn test_undo_clears_redo_on_new_action() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        app.undo();
        assert!(!app.redo_stack.is_empty());
        // New action should clear redo
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        });
        assert!(app.redo_stack.is_empty());
    }

    #[test]
    fn test_undo_stack_limit() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        for i in 0..MAX_UNDO_STEPS + 50 {
            app.add_shape(ShapeKind::Line {
                start: Point::new(0.0, i as f32),
                end: Point::new(10.0, i as f32),
            });
        }
        assert!(app.undo_stack.len() <= MAX_UNDO_STEPS);
    }

    // ---- Delete selected ----

    #[test]
    fn test_delete_selected() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 50.0, 50.0),
        });
        app.selection.add(id);
        app.delete_selected();
        assert!(app.current_page().shapes.is_empty());
        assert!(app.selection.is_empty());
    }

    #[test]
    fn test_delete_selected_empty() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 50.0, 50.0),
        });
        // Nothing selected
        app.delete_selected();
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    // ---- Move selected ----

    #[test]
    fn test_move_selected() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 50.0, 50.0),
        });
        app.selection.add(id);
        app.move_selected(20.0, 30.0);
        let shape = app.current_page().get_shape(id).unwrap();
        if let ShapeKind::Rectangle { bounds } = &shape.kind {
            assert_eq!(bounds.x, 30.0);
            assert_eq!(bounds.y, 40.0);
        }
    }

    // ---- Layer operations ----

    #[test]
    fn test_add_layer() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_layer();
        assert_eq!(app.current_page().layers.len(), 2);
    }

    #[test]
    fn test_delete_layer() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_layer();
        let second_id = app.current_page().layers[1].id;
        app.delete_layer(second_id);
        assert_eq!(app.current_page().layers.len(), 1);
    }

    #[test]
    fn test_delete_last_layer_prevented() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let only_id = app.current_page().layers[0].id;
        app.delete_layer(only_id);
        assert_eq!(app.current_page().layers.len(), 1);
    }

    #[test]
    fn test_toggle_layer_visibility() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.current_page().layers[0].id;
        assert!(app.current_page().layers[0].visible);
        app.toggle_layer_visibility(id);
        assert!(!app.current_page().layers[0].visible);
        app.toggle_layer_visibility(id);
        assert!(app.current_page().layers[0].visible);
    }

    #[test]
    fn test_toggle_layer_lock() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.current_page().layers[0].id;
        assert!(!app.current_page().layers[0].locked);
        app.toggle_layer_lock(id);
        assert!(app.current_page().layers[0].locked);
    }

    #[test]
    fn test_set_layer_opacity() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.current_page().layers[0].id;
        app.set_layer_opacity(id, 0.5);
        assert!((app.current_page().layers[0].opacity - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_set_layer_opacity_clamp() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.current_page().layers[0].id;
        app.set_layer_opacity(id, 2.0);
        assert!((app.current_page().layers[0].opacity - 1.0).abs() < 0.01);
        app.set_layer_opacity(id, -0.5);
        assert!((app.current_page().layers[0].opacity).abs() < 0.01);
    }

    #[test]
    fn test_move_layer_up() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_layer();
        let first_id = app.current_page().layers[0].id;
        app.move_layer_up(first_id);
        assert_eq!(app.current_page().layers[1].id, first_id);
    }

    #[test]
    fn test_move_layer_down() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_layer();
        let second_id = app.current_page().layers[1].id;
        app.move_layer_down(second_id);
        assert_eq!(app.current_page().layers[0].id, second_id);
    }

    #[test]
    fn test_is_active_layer_locked() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        assert!(!app.is_active_layer_locked());
        let id = app.active_layer_id;
        app.toggle_layer_lock(id);
        assert!(app.is_active_layer_locked());
    }

    // ---- Page management ----

    #[test]
    fn test_add_page() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_page();
        assert_eq!(app.pages.len(), 2);
        assert_eq!(app.active_page, 1);
    }

    #[test]
    fn test_switch_page() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_page();
        app.switch_page(0);
        assert_eq!(app.active_page, 0);
    }

    #[test]
    fn test_switch_page_out_of_bounds() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.switch_page(99);
        assert_eq!(app.active_page, 0);
    }

    #[test]
    fn test_delete_page() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_page();
        app.delete_page(0);
        assert_eq!(app.pages.len(), 1);
    }

    #[test]
    fn test_delete_last_page_prevented() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.delete_page(0);
        assert_eq!(app.pages.len(), 1);
    }

    #[test]
    fn test_page_count() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        assert_eq!(app.page_count(), 1);
        app.add_page();
        assert_eq!(app.page_count(), 2);
    }

    // ---- Stroke property setters ----

    #[test]
    fn test_set_stroke_color() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_stroke_color(MOCHA_RED);
        assert_eq!(app.stroke_props.color, MOCHA_RED);
    }

    #[test]
    fn test_set_stroke_thickness() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_stroke_thickness(10);
        assert_eq!(app.stroke_props.thickness, 10);
    }

    #[test]
    fn test_set_stroke_thickness_clamp() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_stroke_thickness(0);
        assert_eq!(app.stroke_props.thickness, 1);
        app.set_stroke_thickness(50);
        assert_eq!(app.stroke_props.thickness, MAX_THICKNESS);
    }

    #[test]
    fn test_set_stroke_opacity() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_stroke_opacity(0.7);
        assert!((app.stroke_props.opacity - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_set_stroke_opacity_clamp() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_stroke_opacity(5.0);
        assert!((app.stroke_props.opacity - 1.0).abs() < 0.01);
        app.set_stroke_opacity(-1.0);
        assert!(app.stroke_props.opacity.abs() < 0.01);
    }

    #[test]
    fn test_toggle_stroke_style() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        assert_eq!(app.stroke_props.style, StrokeStyle::Solid);
        app.toggle_stroke_style();
        assert_eq!(app.stroke_props.style, StrokeStyle::Dashed);
        app.toggle_stroke_style();
        assert_eq!(app.stroke_props.style, StrokeStyle::Solid);
    }

    #[test]
    fn test_set_custom_color() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.custom_r = 100;
        app.custom_g = 150;
        app.custom_b = 200;
        app.set_custom_color();
        assert_eq!(app.stroke_props.color, Color::rgb(100, 150, 200));
    }

    // ---- Visible shapes / layer filtering ----

    #[test]
    fn test_visible_shapes_with_hidden_layer() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let layer_id = app.active_layer_id;
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        assert_eq!(app.current_page().visible_shapes().len(), 1);
        app.toggle_layer_visibility(layer_id);
        assert_eq!(app.current_page().visible_shapes().len(), 0);
    }

    #[test]
    fn test_shapes_on_layer() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let layer1 = app.active_layer_id;
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        app.add_layer();
        let layer2 = app.active_layer_id;
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 20.0, 20.0),
        });
        assert_eq!(app.current_page().shapes_on_layer(layer1).len(), 1);
        assert_eq!(app.current_page().shapes_on_layer(layer2).len(), 1);
    }

    // ---- Canvas interaction (press/move/release) ----

    #[test]
    fn test_pen_draw_creates_freehand() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::Pen;
        let area = app.canvas_rect();
        let sx = area.x + 50.0;
        let sy = area.y + 50.0;
        app.on_canvas_press(sx, sy, false);
        app.on_canvas_move(sx + 20.0, sy + 20.0);
        app.on_canvas_move(sx + 40.0, sy + 10.0);
        app.on_canvas_release(sx + 40.0, sy + 10.0);
        assert_eq!(app.current_page().shapes.len(), 1);
        assert!(
            matches!(app.current_page().shapes[0].kind, ShapeKind::Freehand { .. })
        );
    }

    #[test]
    fn test_line_draw() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::Line;
        let area = app.canvas_rect();
        app.on_canvas_press(area.x + 10.0, area.y + 10.0, false);
        app.on_canvas_release(area.x + 100.0, area.y + 100.0);
        assert_eq!(app.current_page().shapes.len(), 1);
        assert!(
            matches!(app.current_page().shapes[0].kind, ShapeKind::Line { .. })
        );
    }

    #[test]
    fn test_rectangle_draw() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::Rectangle;
        let area = app.canvas_rect();
        app.on_canvas_press(area.x + 10.0, area.y + 10.0, false);
        app.on_canvas_release(area.x + 200.0, area.y + 150.0);
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn test_locked_layer_prevents_drawing() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        let id = app.active_layer_id;
        app.toggle_layer_lock(id);
        app.current_tool = Tool::Pen;
        let area = app.canvas_rect();
        app.on_canvas_press(area.x + 10.0, area.y + 10.0, false);
        // Drag should be None because layer is locked
        assert!(matches!(app.drag, DragState::None));
    }

    #[test]
    fn test_select_tool_click_selects() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(50.0, 50.0, 100.0, 80.0),
        });
        app.current_tool = Tool::Select;
        // Click on shape (convert canvas coords to screen coords)
        let sp = app.canvas_to_screen(75.0, 70.0);
        app.on_canvas_press(sp.x, sp.y, false);
        app.on_canvas_release(sp.x, sp.y);
        assert!(app.selection.contains(id));
    }

    #[test]
    fn test_select_shift_toggles() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        let id1 = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(50.0, 50.0, 100.0, 80.0),
        });
        let _id2 = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(200.0, 200.0, 100.0, 80.0),
        });
        app.current_tool = Tool::Select;
        // Click first shape
        let sp1 = app.canvas_to_screen(75.0, 70.0);
        app.on_canvas_press(sp1.x, sp1.y, false);
        app.on_canvas_release(sp1.x, sp1.y);
        assert!(app.selection.contains(id1));
        // Shift-click second shape
        let sp2 = app.canvas_to_screen(250.0, 240.0);
        app.on_canvas_press(sp2.x, sp2.y, true);
        app.on_canvas_release(sp2.x, sp2.y);
        assert_eq!(app.selection.shape_ids.len(), 2);
    }

    #[test]
    fn test_eraser_removes_shape() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(50.0, 50.0, 100.0, 80.0),
        });
        app.current_tool = Tool::Eraser;
        let sp = app.canvas_to_screen(75.0, 70.0);
        app.on_canvas_press(sp.x, sp.y, false);
        assert!(app.current_page().shapes.is_empty());
    }

    #[test]
    fn test_text_tool_places_label() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::Text;
        app.text_input_buffer = "Hello World".to_string();
        let area = app.canvas_rect();
        app.on_canvas_press(area.x + 50.0, area.y + 50.0, false);
        assert_eq!(app.current_page().shapes.len(), 1);
        assert!(
            matches!(app.current_page().shapes[0].kind, ShapeKind::TextLabel { .. })
        );
        assert!(app.text_input_buffer.is_empty());
    }

    #[test]
    fn test_sticky_note_creation() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::StickyNote;
        app.text_input_buffer = "My Note".to_string();
        let area = app.canvas_rect();
        app.on_canvas_press(area.x + 50.0, area.y + 50.0, false);
        app.on_canvas_move(area.x + 250.0, area.y + 200.0);
        app.on_canvas_release(area.x + 250.0, area.y + 200.0);
        assert_eq!(app.current_page().shapes.len(), 1);
        assert!(
            matches!(app.current_page().shapes[0].kind, ShapeKind::StickyNote { .. })
        );
    }

    // ---- Pan / scroll ----

    #[test]
    fn test_pan() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.start_pan(100.0, 100.0);
        app.on_canvas_move(120.0, 130.0);
        assert!((app.pan_x - 20.0).abs() < 0.01);
        assert!((app.pan_y - 30.0).abs() < 0.01);
    }

    #[test]
    fn test_scroll_zoom_in() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let old_zoom = app.zoom;
        app.on_scroll(400.0, 300.0, 1.0);
        assert!(app.zoom > old_zoom);
    }

    #[test]
    fn test_scroll_zoom_out() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let old_zoom = app.zoom;
        app.on_scroll(400.0, 300.0, -1.0);
        assert!(app.zoom < old_zoom);
    }

    // ---- Export ----

    #[test]
    fn test_export_empty_page() {
        let app = WhiteboardApp::new(800.0, 600.0);
        let svg = app.export_svg_text();
        assert!(svg.contains("<whiteboard>"));
        assert!(svg.contains("</whiteboard>"));
        assert!(svg.contains("Board 1"));
    }

    #[test]
    fn test_export_with_shapes() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 50.0),
        });
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 50.0, 30.0),
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("<line"));
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn test_export_contains_text_label() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::TextLabel {
            position: Point::new(50.0, 50.0),
            content: "Hello".to_string(),
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("<text"));
        assert!(svg.contains("Hello"));
    }

    #[test]
    fn test_export_sticky_note() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::StickyNote {
            bounds: Rect::new(10.0, 10.0, 100.0, 80.0),
            content: "Important".to_string(),
            bg_color: MOCHA_YELLOW,
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("<sticky"));
        assert!(svg.contains("Important"));
    }

    #[test]
    fn test_export_arrow() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Arrow {
            start: Point::new(0.0, 0.0),
            end: Point::new(50.0, 50.0),
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("<arrow"));
    }

    #[test]
    fn test_export_ellipse() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Ellipse {
            bounds: Rect::new(0.0, 0.0, 80.0, 60.0),
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("<ellipse"));
    }

    #[test]
    fn test_export_freehand() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Freehand {
            points: vec![
                Point::new(0.0, 0.0),
                Point::new(5.0, 5.0),
                Point::new(10.0, 0.0),
            ],
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("<path"));
    }

    #[test]
    fn test_export_dashed_stroke() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.stroke_props.style = StrokeStyle::Dashed;
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(50.0, 50.0),
        });
        let svg = app.export_svg_text();
        assert!(svg.contains("5,5"));
    }

    // ---- Rendering ----

    #[test]
    fn test_render_not_empty() {
        let app = WhiteboardApp::new(1280.0, 800.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_shapes() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 100.0),
        });
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 50.0, 50.0),
        });
        let cmds = app.render();
        assert!(cmds.len() > 10);
    }

    #[test]
    fn test_render_with_layers_panel_hidden() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.show_layers_panel = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_drag_preview_line() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::Line;
        app.drag = DragState::DrawingShape {
            start: Point::new(10.0, 10.0),
            current: Point::new(100.0, 100.0),
        };
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_drag_preview_freehand() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.drag = DragState::DrawingFreehand {
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
        };
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_marquee_preview() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.drag = DragState::Marquee {
            start: Point::new(10.0, 10.0),
            current: Point::new(100.0, 100.0),
        };
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_canvas_rect_dimensions() {
        let app = WhiteboardApp::new(1280.0, 800.0);
        let r = app.canvas_rect();
        assert!(r.width > 0.0);
        assert!(r.height > 0.0);
        assert_eq!(r.x, TOOLBAR_WIDTH);
    }

    #[test]
    fn test_canvas_rect_with_layers_hidden() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        let r1 = app.canvas_rect();
        app.show_layers_panel = false;
        let r2 = app.canvas_rect();
        assert!(r2.width > r1.width);
    }

    // ---- Page get_shape helpers ----

    #[test]
    fn test_page_get_shape() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        assert!(app.current_page().get_shape(id).is_some());
        assert!(app.current_page().get_shape(999).is_none());
    }

    #[test]
    fn test_page_find_shape_index() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let id = app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(10.0, 10.0),
        });
        assert_eq!(app.current_page().find_shape_index(id), Some(0));
        assert_eq!(app.current_page().find_shape_index(999), None);
    }

    #[test]
    fn test_page_find_layer_index() {
        let app = WhiteboardApp::new(800.0, 600.0);
        let id = app.current_page().layers[0].id;
        assert_eq!(app.current_page().find_layer_index(id), Some(0));
        assert_eq!(app.current_page().find_layer_index(999), None);
    }
}
