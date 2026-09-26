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

use appearance::Edge;
use appearance::Palette;
use appearance::Surface;
use guitk::color::Color;
use guitk::dialog::{FilePicker, Picked};
use guitk::event::{Key, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use oswindow::app::{self, App, Response};
use oswindow::{Event, RenderTree};

use std::collections::VecDeque;
use std::process::ExitCode;
use std::time::Duration;
use unsaved::{Choice, Question};

// ============================================================================
// Catppuccin Mocha theme colors
// ============================================================================

// Part of the complete Catppuccin Mocha palette, kept whole even though no
// widget currently paints with these four: a named palette with holes in it is
// not the palette it is named after, and the next widget to want one would
// otherwise re-derive the hex by hand.

// ============================================================================
// Layout constants
// ============================================================================

/// Blank space left around the drawing in an exported SVG.
const SVG_MARGIN: f32 = 20.0;
/// Where a sticky note's text sits inside its rectangle.
const STICKY_TEXT_INSET: f32 = 16.0;

const TOOLBAR_WIDTH: f32 = 52.0;
const TOP_BAR_HEIGHT: f32 = 40.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;
const RIGHT_PANEL_WIDTH: f32 = 200.0;
const LAYER_ROW_HEIGHT: f32 = 30.0;
const PALETTE_SWATCH_SIZE: f32 = 22.0;
const PALETTE_GAP: f32 = 3.0;
const PAGE_TAB_HEIGHT: f32 = 28.0;
/// Narrowest a page tab gets, however short its name.
const PAGE_TAB_MIN_WIDTH: f32 = 76.0;

/// Inset of a sticky note's text from each edge of the note, in canvas units.
const STICKY_PADDING: f32 = 8.0;
/// Point size a sticky note's text is laid out at, before zoom.
const STICKY_FONT_SIZE: f32 = 12.0;
/// Line-to-line spacing of a sticky note's text, before zoom.
const STICKY_LINE_HEIGHT: f32 = 16.0;

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 10.0;
const GRID_SIZE: f32 = 20.0;
const MAX_UNDO_STEPS: usize = 200;
const MAX_THICKNESS: u8 = 20;
/// A window smaller than this has no canvas left between the panels.
const MIN_WINDOW_WIDTH: f32 = 480.0;
const MIN_WINDOW_HEIGHT: f32 = 320.0;
const WINDOW_WIDTH: f32 = 1280.0;
const WINDOW_HEIGHT: f32 = 800.0;
/// Height of a tool button, and the step between two of them.
const TOOL_BUTTON_HEIGHT: f32 = 32.0;
const TOOL_BUTTON_STEP: f32 = 38.0;
/// How far in from the left edge of the toolbar a button starts.
const TOOL_BUTTON_INSET: f32 = 6.0;
const TOOL_BUTTON_WIDTH: f32 = 40.0;
/// Air between the last tool and the rule under it, the rule and the heading,
/// and the heading and the first swatch.
const PALETTE_HEADING_GAP: f32 = 8.0;
const PALETTE_HEADING_HEIGHT: f32 = 16.0;

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

    /// The tool a letter picks, if any.
    ///
    /// Derived from `shortcut` rather than written out again: a second table
    /// is a second chance for the toolbar's letter and the keyboard's letter
    /// to stop agreeing.
    pub fn from_shortcut(ch: char) -> Option<Self> {
        let upper = ch.to_ascii_uppercase();
        Self::all()
            .iter()
            .copied()
            .find(|tool| tool.shortcut() == Some(upper))
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
            // Content, not chrome: this is the default *ink*, the colour the
            // user draws with, and it is stored with the stroke. Like
            // `paint`'s swatches it must not follow the desktop theme -- a
            // drawing would otherwise change colour when the theme did.
            color: Color::from_hex(0xCDD6F4),
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
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_points(p1: Point, p2: Point) -> Self {
        let x = p1.x.min(p2.x);
        let y = p1.y.min(p2.y);
        let width = (p1.x - p2.x).abs();
        let height = (p1.y - p2.y).abs();
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
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
                    if p.x < min_x {
                        min_x = p.x;
                    }
                    if p.y < min_y {
                        min_y = p.y;
                    }
                    if p.x > max_x {
                        max_x = p.x;
                    }
                    if p.y > max_y {
                        max_y = p.y;
                    }
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
                        && point_to_segment_distance(px, py, a.x, a.y, b.x, b.y) <= threshold
                    {
                        return true;
                    }
                }
                false
            }
            ShapeKind::Line { start, end } | ShapeKind::Arrow { start, end } => {
                point_to_segment_distance(px, py, start.x, start.y, end.x, end.y) <= threshold
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

/// A sticky note's content, broken into the lines that fit across the note.
///
/// Laid out in canvas units rather than screen pixels, so zooming moves and
/// scales the note without reflowing it — where the lines break is a property
/// of the note, not of how closely the user happens to be looking at it. A
/// glyph whose scaled advance does not divide evenly then leaves a line a
/// fraction too wide for the note at some zoom levels; the `max_width` on the
/// drawn command clips that fraction, which costs a character at the margin
/// rather than letting the text run off the note.
fn sticky_note_lines(bounds: &Rect, content: &str) -> Vec<String> {
    if content.is_empty() {
        return Vec::new();
    }
    text::wrap(
        content,
        bounds.width - STICKY_PADDING * 2.0,
        STICKY_FONT_SIZE,
        FontWeightHint::Regular,
    )
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
    /// A shape taken off the page, and where it was in the drawing order.
    ///
    /// It carried only the id, and was recorded after the shape was gone, so
    /// undoing a deletion looked for the shape to put back, found nothing,
    /// and did nothing: a deleted shape could not be undeleted.
    DeleteShape {
        shape: Shape,
        index: usize,
    },
    /// The reverse of a deletion: the shape put back where it was.
    RestoreShape {
        shape: Shape,
        index: usize,
    },
    MoveShape {
        shape_id: ShapeId,
        dx: f32,
        dy: f32,
    },
    AddLayer(Layer),
    /// A layer taken away with the shapes on it, each with where it was --
    /// what undoing it needs to put back. It carried only the layer's id.
    DeleteLayer {
        layer: Layer,
        index: usize,
        shapes: Vec<(usize, Shape)>,
    },
    /// The reverse of a layer's deletion.
    RestoreLayer {
        layer: Layer,
        index: usize,
        shapes: Vec<(usize, Shape)>,
    },
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
    /// What can be undone on this page, oldest first.
    ///
    /// One history per page. It was one for the whole window, replayed onto
    /// whichever page was showing: undo after switching pages took a shape
    /// off the wrong page -- ids are per page, so another page's shape of the
    /// same number -- and a deleted page's history went on acting on the
    /// page that replaced it.
    pub undo_stack: VecDeque<Action>,
    pub redo_stack: Vec<Action>,
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
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
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
/// The keys this board answers that are not a tool letter.
///
/// The tool letters are not here on purpose: they live in `Tool::shortcut`,
/// which is what dispatches them, and `shortcut_rows` appends them to this
/// list when the card is drawn. Copying the nine into a second table is the
/// arrangement dd-866 exists to refuse -- two tables that agree until one is
/// edited.
///
/// **No `?` row.** An unmodified character picks a tool or zooms, so `?`
/// would have to be taken away from `handle_typed` to give it here. Fifth app
/// where `?` was not free.
const SHORTCUTS: &[(&str, &str)] = &[
    ("F1", "This list"),
    ("Ctrl+Z", "Undo"),
    ("Ctrl+Shift+Z", "Redo"),
    ("Ctrl+Y", "Redo"),
    ("Ctrl+A", "Select everything on the page"),
    ("Ctrl+S / Ctrl+Shift+S", "Save / save as a new file"),
    ("Ctrl+O", "Open a board"),
    ("Ctrl+E", "Export this page as an SVG picture"),
    ("Ctrl+L", "Show or hide the layers panel"),
    ("Arrows", "Nudge the selection"),
    ("Delete", "Delete the selection"),
    ("Esc", "Drop the selection"),
    ("+", "Zoom in"),
    ("-", "Zoom out"),
    ("0", "Zoom so the whole page fits"),
    ("G", "Grid on or off"),
    ("#", "Snap to the grid"),
];

pub struct WhiteboardApp {
    // Window dimensions
    /// The save picker.
    ///
    /// Until 2026-09-15 this program had no `std::fs`, no `safeio` and no
    /// dialog of any kind: **a drawing lived exactly as long as the window
    /// did.** `export_svg` was written and tested and had no caller.
    pub picker: FilePicker,
    /// What the last save, open or export did, shown in the status bar.
    pub status_message: Option<String>,
    /// The file this board was opened from or last saved to: where Ctrl+S
    /// writes without asking. `None` for one never saved.
    pub document_path: Option<std::path::PathBuf>,
    /// Whether the board has changed since it was opened, saved or begun. Set
    /// by `push_action`, which every change on a page comes through, by undo
    /// and redo, and by adding or deleting a page.
    pub dirty: bool,
    /// What the file picker is up for.
    pub picker_for: PickerFor,
    /// "Unsaved changes -- save them?", while it is asked, and what it holds
    /// up (`apps/unsaved`).
    question: Option<Question<Pending>>,
    /// Set once the window may close; the next answer to the loop is `Exit`.
    pub quit: bool,
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
    /// Whether the shortcut card is up.
    pub show_help: bool,
    pub show_grid: bool,
    pub snap_to_grid: bool,

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

    /// Whether shift was held on the last key seen.
    ///
    /// `on_canvas_press` takes a `shift_held` for multi-select, and a mouse
    /// event carries no modifiers, so the only record of the key state is what
    /// the last keyboard event said.
    pub shift_held: bool,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
}

/// What the window says about what it can and cannot do.
///
/// Added by `scripts/find-silent-incapacity.py` when the answer was "neither":
/// a program that reached nothing outside its own process and never said so.
/// A save door was built afterwards and this text was not revisited, so for a
/// while the window told people "your work is gone when the window closes"
/// while Ctrl+S was writing an SVG — found by
/// `scripts/find-stale-admissions.py`, which looks for exactly that, a
/// standing sentence denying a capability the crate holds.
///
/// It is the worse direction of the two to get wrong. A program that
/// overstates itself is caught the first time someone tries the feature; one
/// that understates itself is believed, and nobody tries. Anyone who read the
/// old text closed the window and lost a drawing they could have kept.
///
/// Both halves are stated because only one changed: writing works, reading
/// does not exist, and a picture you cannot reopen is a different thing from
/// a document.
/// What the file picker is up for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerFor {
    /// A board to open in place of this one.
    Open,
    /// Where to save this board, which then belongs to that file.
    Save,
    /// Where to export the page as an SVG picture: not a save, since nothing
    /// reads one back.
    Export,
    /// Where to save a board with no file yet, before what the unsaved-changes
    /// question held up goes on.
    SaveThen(Pending),
}

/// What the unsaved-changes question is holding up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pending {
    /// Opening another board in its place.
    Open,
    /// Closing the window.
    Close,
}

/// The version of the board file this writes, and the newest it reads.
const BOARD_FORMAT: i64 = 1;

/// The largest board file this will open. A board cut short would be read as
/// a smaller board with no sign anything was missing, so a larger file is
/// refused rather than read in part. Generous: a freehand stroke is a point per
/// line.
const MAX_BOARD_BYTES: usize = 64 * 1024 * 1024;

/// Where the status bar's note on the last save, open or export begins, past
/// the page count.
const STATUS_NOTE_X: f32 = 730.0;

impl WhiteboardApp {
    pub fn new(width: f32, height: f32) -> Self {
        let first_page = Page::new("Board 1".to_string());
        let first_layer_id = first_page.layers.first().map(|l| l.id).unwrap_or(1);

        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::default(),
            status_message: None,
            document_path: None,
            dirty: false,
            picker_for: PickerFor::Export,
            question: None,
            quit: false,
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
            show_help: false,
            show_grid: true,
            snap_to_grid: false,
            custom_r: 205,
            custom_g: 214,
            custom_b: 244,
            text_input_buffer: String::new(),
            active_layer_id: first_layer_id,
            show_layers_panel: true,
            shift_held: false,
        }
    }

    // ========================================================================
    // Page accessors
    // ========================================================================

    /// The page being drawn on.
    ///
    /// A board always has at least one page and `active_page` always names
    /// one: `new` makes the first, `add_page` and `switch_page` set the index
    /// to a page they have just seen, and `delete_page` refuses to remove the
    /// last and re-clamps the index. Returning an `Option` here would hand
    /// that invariant to sixty call sites to re-check.
    #[allow(
        clippy::expect_used,
        reason = "invariant established by `new` and preserved by every writer of `active_page`"
    )]
    pub fn current_page(&self) -> &Page {
        self.pages
            .get(self.active_page)
            .expect("a board always has at least one page and active_page names one")
    }

    /// The page being drawn on, to be changed. See `current_page`.
    #[allow(
        clippy::expect_used,
        reason = "invariant established by `new` and preserved by every writer of `active_page`"
    )]
    pub fn current_page_mut(&mut self) -> &mut Page {
        self.pages
            .get_mut(self.active_page)
            .expect("a board always has at least one page and active_page names one")
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

    /// Record a change on the current page, for undo -- and note that the
    /// board has changed since it was saved. Every change comes through here.
    pub fn push_action(&mut self, action: Action) {
        let page = self.current_page_mut();
        page.redo_stack.clear();
        if page.undo_stack.len() >= MAX_UNDO_STEPS {
            page.undo_stack.pop_front();
        }
        page.undo_stack.push_back(action);
        self.dirty = true;
    }

    pub fn undo(&mut self) {
        if let Some(action) = self.current_page_mut().undo_stack.pop_back() {
            let reverse = self.reverse_action(&action);
            self.apply_action_silent(&reverse);
            self.current_page_mut().redo_stack.push(action);
            // Undoing past a save leaves a board the file does not hold.
            self.dirty = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some(action) = self.current_page_mut().redo_stack.pop() {
            self.apply_action_silent(&action);
            self.current_page_mut().undo_stack.push_back(action);
            self.dirty = true;
        }
    }

    /// Apply an action without recording it in the undo stack.
    fn apply_action_silent(&mut self, action: &Action) {
        match action {
            Action::AddShape(shape) => {
                self.current_page_mut().shapes.push(shape.clone());
            }
            Action::DeleteShape { shape, .. } => {
                let page = self.current_page_mut();
                if let Some(idx) = page.find_shape_index(shape.id) {
                    page.shapes.remove(idx);
                }
                self.selection.shape_ids.retain(|sid| *sid != shape.id);
            }
            Action::RestoreShape { shape, index } => {
                let page = self.current_page_mut();
                let at = (*index).min(page.shapes.len());
                page.shapes.insert(at, shape.clone());
            }
            Action::MoveShape { shape_id, dx, dy } => {
                if let Some(shape) = self.current_page_mut().get_shape_mut(*shape_id) {
                    shape.translate(*dx, *dy);
                }
            }
            Action::AddLayer(layer) => {
                self.current_page_mut().layers.push(layer.clone());
            }
            Action::DeleteLayer { layer, .. } => {
                let page = self.current_page_mut();
                page.layers.retain(|l| l.id != layer.id);
                page.shapes.retain(|s| s.layer_id != layer.id);
            }
            Action::RestoreLayer {
                layer,
                index,
                shapes,
            } => {
                let page = self.current_page_mut();
                let at = (*index).min(page.layers.len());
                page.layers.insert(at, layer.clone());
                // In the order they were taken, so each goes back where it was.
                for (i, shape) in shapes {
                    let at = (*i).min(page.shapes.len());
                    page.shapes.insert(at, shape.clone());
                }
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
            Action::AddShape(shape) => Action::DeleteShape {
                shape: shape.clone(),
                index: self
                    .current_page()
                    .find_shape_index(shape.id)
                    .unwrap_or(usize::MAX),
            },
            Action::DeleteShape { shape, index } => Action::RestoreShape {
                shape: shape.clone(),
                index: *index,
            },
            Action::RestoreShape { shape, index } => Action::DeleteShape {
                shape: shape.clone(),
                index: *index,
            },
            Action::MoveShape { shape_id, dx, dy } => Action::MoveShape {
                shape_id: *shape_id,
                dx: -dx,
                dy: -dy,
            },
            Action::AddLayer(layer) => Action::DeleteLayer {
                layer: layer.clone(),
                index: self
                    .current_page()
                    .find_layer_index(layer.id)
                    .unwrap_or(usize::MAX),
                shapes: Vec::new(),
            },
            Action::DeleteLayer {
                layer,
                index,
                shapes,
            } => Action::RestoreLayer {
                layer: layer.clone(),
                index: *index,
                shapes: shapes.clone(),
            },
            Action::RestoreLayer {
                layer,
                index,
                shapes,
            } => Action::DeleteLayer {
                layer: layer.clone(),
                index: *index,
                shapes: shapes.clone(),
            },
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
                let reversed: Vec<Action> = actions
                    .iter()
                    .rev()
                    .map(|a| self.reverse_action(a))
                    .collect();
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
        // Each with where it was, taken last-first: undo runs a batch
        // backwards, so the lowest position is put back first and every later
        // one lands on the index it was recorded at.
        let page = self.current_page();
        let mut actions: Vec<Action> = page
            .shapes
            .iter()
            .enumerate()
            .filter(|(_, s)| ids.contains(&s.id))
            .map(|(index, shape)| Action::DeleteShape {
                shape: shape.clone(),
                index,
            })
            .collect();
        actions.reverse();
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
        let (Some(index), Some(layer)) = (
            page.find_layer_index(layer_id),
            page.layers.iter().find(|l| l.id == layer_id).cloned(),
        ) else {
            return;
        };
        let shapes: Vec<(usize, Shape)> = page
            .shapes
            .iter()
            .enumerate()
            .filter(|(_, s)| s.layer_id == layer_id)
            .map(|(i, s)| (i, s.clone()))
            .collect();
        let action = Action::DeleteLayer {
            layer,
            index,
            shapes,
        };
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
            && idx.saturating_add(1) < page.layers.len()
        {
            let old_order: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
            page.layers.swap(idx, idx.saturating_add(1));
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
            && idx > 0
        {
            let old_order: Vec<LayerId> = page.layers.iter().map(|l| l.id).collect();
            page.layers.swap(idx, idx.saturating_sub(1));
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
        self.dirty = true;
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
        self.dirty = true;
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
        let move_delta: Option<(f32, f32)> =
            if let DragState::Moving { last_x, last_y, .. } = &self.drag {
                Some((canvas_pt.x - *last_x, canvas_pt.y - *last_y))
            } else {
                None
            };

        // Extract pan delta similarly to avoid borrow conflict.
        let pan_delta: Option<(f32, f32)> =
            if let DragState::Panning { last_x, last_y } = &self.drag {
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
            DragState::Panning { .. } | DragState::Moving { .. } | DragState::None => {}
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
                    .get(
                        self.sticky_color_index
                            .checked_rem(STICKY_COLORS.len())
                            .unwrap_or(0),
                    )
                    .copied()
                    .unwrap_or(self.palette.yellow);
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
                let page = self.current_page_mut();
                if let Some(index) = page.find_shape_index(id) {
                    let shape = page.shapes.remove(index);
                    self.push_action(Action::DeleteShape { shape, index });
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
    /// The current page as a standalone SVG document.
    ///
    /// **This was named `svg` and no SVG reader could have drawn it.** It
    /// emitted a `<whiteboard>` root with `<page>` and `<layer>` inside, and
    /// of the six shape kinds only `<line>` and `<text>` were elements SVG
    /// has. The rest failed in three different ways:
    ///
    /// * `<path points="...">` — `<path>` takes `d`; `points` belongs to
    ///   `<polyline>`. **Every freehand stroke silently vanished**, which is
    ///   most of what a whiteboard holds.
    /// * `<arrow>` and `<sticky>` are not SVG elements at all, so an arrow and
    ///   a sticky note drew nothing.
    /// * `<rect>` and `<ellipse>` carried no `fill`, and SVG's initial fill is
    ///   **black** — so the two shapes that did survive came out as solid
    ///   black boxes over whatever they had been drawn around.
    ///
    /// A file that opens and is wrong is worse than one that does not open: a
    /// reader who sees a page of black rectangles concludes the drawing was
    /// lost, and a reader who sees their freehand notes missing may not notice
    /// at all.
    ///
    /// Colours go out as `rgb()` plus a separate opacity attribute rather than
    /// `rgba()`. Browsers accept `rgba()` in a presentation attribute; SVG 1.1
    /// does not, and the point of this format is the readers that are not a
    /// browser.
    pub fn export_svg(&self) -> String {
        let page = self.current_page();
        let (w, h) = Self::svg_extent(page);

        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        out.push_str(&format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" \
             viewBox=\"0 0 {w:.0} {h:.0}\">\n"
        ));
        // The page name is user-typed and lands inside markup: unescaped, a
        // `<` or `&` makes the document unparseable and a `</title>` injects
        // arbitrary elements. `<title>` is SVG's own name for this and is what
        // a viewer shows in its window bar.
        out.push_str(&format!(
            "  <title>{}</title>\n",
            guitk::escape::xml(&page.name)
        ));
        out.push_str(
            "  <defs>\n    <marker id=\"arrowhead\" markerWidth=\"10\" markerHeight=\"7\" \
             refX=\"9\" refY=\"3.5\" orient=\"auto\">\n      \
             <polygon points=\"0,0 10,3.5 0,7\" fill=\"context-stroke\" />\n    \
             </marker>\n  </defs>\n",
        );

        for layer in &page.layers {
            // A hidden layer is left out rather than written with
            // `display="none"`: the export is what the user is looking at.
            if !layer.visible {
                continue;
            }
            out.push_str(&format!(
                "  <g id=\"{}\" opacity=\"{:.2}\">\n",
                guitk::escape::xml(&layer.name),
                layer.opacity
            ));

            for shape in page.shapes.iter().filter(|s| s.layer_id == layer.id) {
                out.push_str(&Self::svg_shape(shape));
            }
            out.push_str("  </g>\n");
        }
        out.push_str("</svg>\n");
        out
    }

    /// The size of the document: everything drawn, plus a margin.
    ///
    /// A fixed canvas would clip a drawing that ran past it, and a zero-size
    /// viewBox makes a viewer show nothing at all -- so an empty page still
    /// gets a positive extent.
    fn svg_extent(page: &Page) -> (f32, f32) {
        let mut w: f32 = 0.0;
        let mut h: f32 = 0.0;
        for shape in &page.shapes {
            let (x, y) = match &shape.kind {
                ShapeKind::Freehand { points } => points
                    .iter()
                    .fold((0.0_f32, 0.0_f32), |a, p| (a.0.max(p.x), a.1.max(p.y))),
                ShapeKind::Line { start, end } | ShapeKind::Arrow { start, end } => {
                    (start.x.max(end.x), start.y.max(end.y))
                }
                ShapeKind::Rectangle { bounds }
                | ShapeKind::Ellipse { bounds }
                | ShapeKind::StickyNote { bounds, .. } => {
                    (bounds.x + bounds.width, bounds.y + bounds.height)
                }
                ShapeKind::TextLabel { position, .. } => (position.x, position.y),
            };
            w = w.max(x);
            h = h.max(y);
        }
        (
            (w + SVG_MARGIN).max(SVG_MARGIN),
            (h + SVG_MARGIN).max(SVG_MARGIN),
        )
    }

    /// One shape as an SVG element.
    fn svg_shape(shape: &Shape) -> String {
        let color = shape.stroke.effective_color();
        let stroke = format!("rgb({},{},{})", color.r, color.g, color.b);
        let opacity = f32::from(color.a) / 255.0;
        let thickness = shape.stroke.thickness;
        let dash = match shape.stroke.style {
            StrokeStyle::Solid => "none",
            StrokeStyle::Dashed => "5,5",
        };
        // `fill="none"` on every stroked shape. SVG's initial fill is black,
        // so leaving it off is what turned rectangles into filled boxes.
        let pen = format!(
            "fill=\"none\" stroke=\"{stroke}\" stroke-opacity=\"{opacity:.2}\" \
             stroke-width=\"{thickness}\" stroke-dasharray=\"{dash}\""
        );

        match &shape.kind {
            ShapeKind::Freehand { points } => {
                let pts: Vec<String> = points
                    .iter()
                    .map(|p| format!("{:.1},{:.1}", p.x, p.y))
                    .collect();
                format!(
                    "    <polyline points=\"{}\" {pen} stroke-linecap=\"round\" \
                     stroke-linejoin=\"round\" />\n",
                    pts.join(" ")
                )
            }
            ShapeKind::Line { start, end } => format!(
                "    <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" {pen} />\n",
                start.x, start.y, end.x, end.y
            ),
            ShapeKind::Arrow { start, end } => format!(
                "    <line x1=\"{:.1}\" y1=\"{:.1}\" x2=\"{:.1}\" y2=\"{:.1}\" {pen} \
                 marker-end=\"url(#arrowhead)\" />\n",
                start.x, start.y, end.x, end.y
            ),
            ShapeKind::Rectangle { bounds } => format!(
                "    <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" {pen} />\n",
                bounds.x, bounds.y, bounds.width, bounds.height
            ),
            ShapeKind::Ellipse { bounds } => format!(
                "    <ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" {pen} />\n",
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
                bounds.width / 2.0,
                bounds.height / 2.0
            ),
            ShapeKind::TextLabel { position, content } => format!(
                "    <text x=\"{:.1}\" y=\"{:.1}\" fill=\"{stroke}\" \
                 fill-opacity=\"{opacity:.2}\">{}</text>\n",
                position.x,
                position.y,
                guitk::escape::xml(content)
            ),
            // A sticky note is a filled rectangle with the note on top -- two
            // elements, because SVG has no element that is both.
            ShapeKind::StickyNote {
                bounds,
                content,
                bg_color,
            } => format!(
                "    <rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" \
                 fill=\"rgb({},{},{})\" stroke=\"none\" />\n    \
                 <text x=\"{:.1}\" y=\"{:.1}\" fill=\"{stroke}\">{}</text>\n",
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                bg_color.r,
                bg_color.g,
                bg_color.b,
                bounds.x + STICKY_TEXT_INSET,
                bounds.y + STICKY_TEXT_INSET,
                guitk::escape::xml(content)
            ),
        }
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    // ------------------------------------------------------------------
    // Layout the renderer draws and the pointer reads
    // ------------------------------------------------------------------

    /// Adopt a new window size. Returns whether it changed.
    pub fn set_window_size(&mut self, width: f32, height: f32) -> bool {
        let width = width.max(MIN_WINDOW_WIDTH);
        let height = height.max(MIN_WINDOW_HEIGHT);
        if (self.win_width - width).abs() < f32::EPSILON
            && (self.win_height - height).abs() < f32::EPSILON
        {
            return false;
        }
        self.win_width = width;
        self.win_height = height;
        true
    }

    /// Y the tool column starts at.
    fn toolbar_top(&self) -> f32 {
        TOP_BAR_HEIGHT + PAGE_TAB_HEIGHT
    }

    /// Every tool button, with the rectangle it is drawn in.
    ///
    /// The renderer walked a `ty` down the column, so the only record of where
    /// a button was, was the pixels already drawn -- which is why none of them
    /// could be pressed.
    pub fn tool_buttons(&self) -> Vec<(Tool, Rect)> {
        let mut ty = self.toolbar_top() + 8.0;
        Tool::all()
            .iter()
            .map(|tool| {
                let rect = Rect::new(TOOL_BUTTON_INSET, ty, TOOL_BUTTON_WIDTH, TOOL_BUTTON_HEIGHT);
                ty += TOOL_BUTTON_STEP;
                (*tool, rect)
            })
            .collect()
    }

    /// Y the colour swatches start at, under the tools and their heading.
    fn palette_top(&self) -> f32 {
        #[allow(clippy::cast_precision_loss, reason = "there are nine tools")]
        let tools = Tool::all().len() as f32;
        self.toolbar_top()
            + 8.0
            + tools * TOOL_BUTTON_STEP
            + PALETTE_HEADING_GAP * 2.0
            + PALETTE_HEADING_HEIGHT
    }

    /// Every colour swatch, with the rectangle it is drawn in.
    pub fn palette_swatches(&self) -> Vec<(Color, Rect)> {
        let top = self.palette_top();
        PALETTE_COLORS
            .iter()
            .enumerate()
            .map(|(i, color)| {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "the palette has sixteen colours"
                )]
                let (col, row) = ((i % 2) as f32, (i / 2) as f32);
                (
                    *color,
                    Rect::new(
                        TOOL_BUTTON_INSET + col * (PALETTE_SWATCH_SIZE + PALETTE_GAP),
                        top + row * (PALETTE_SWATCH_SIZE + PALETTE_GAP),
                        PALETTE_SWATCH_SIZE,
                        PALETTE_SWATCH_SIZE,
                    ),
                )
            })
            .collect()
    }

    /// Which tool a point in the toolbar is on, if any.
    pub fn tool_at(&self, x: f32, y: f32) -> Option<Tool> {
        self.tool_buttons()
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(tool, _)| tool)
    }

    /// Which colour a point in the palette is on, if any.
    pub fn swatch_at(&self, x: f32, y: f32) -> Option<Color> {
        self.palette_swatches()
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(color, _)| color)
    }

    /// How wide a page's tab is: its name, or a minimum, whichever is more.
    fn page_tab_width(name: &str) -> f32 {
        text::padded_width_any_weight(name, 8.0, 12.0).max(PAGE_TAB_MIN_WIDTH)
    }

    /// The page tabs along the top, with the rectangle each is drawn in.
    pub fn page_tabs(&self) -> Vec<(usize, Rect)> {
        let mut tx = TOOLBAR_WIDTH + 4.0;
        self.pages
            .iter()
            .enumerate()
            .map(|(i, page)| {
                let width = Self::page_tab_width(&page.name);
                let rect = Rect::new(tx, TOP_BAR_HEIGHT + 2.0, width, PAGE_TAB_HEIGHT - 2.0);
                tx += width + 4.0;
                (i, rect)
            })
            .collect()
    }

    /// The button that adds a page, which sits after the last tab.
    pub fn add_page_button(&self) -> Rect {
        let x = self
            .page_tabs()
            .last()
            .map_or(TOOLBAR_WIDTH + 4.0, |(_, r)| r.x + r.width + 4.0);
        Rect::new(x, TOP_BAR_HEIGHT + 4.0, 24.0, 20.0)
    }

    /// Which page tab a point is on, if any.
    pub fn page_tab_at(&self, x: f32, y: f32) -> Option<usize> {
        self.page_tabs()
            .into_iter()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(index, _)| index)
    }

    // ------------------------------------------------------------------
    // Input
    // ------------------------------------------------------------------

    /// Handle one input event. Returns whether anything changed.
    pub fn handle_event(&mut self, event: &Event) -> bool {
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
            return true;
        }
        // The picker takes input first while it is up, or a filename is drawn
        // onto the canvas behind it.
        match self.picker.handle(event, self.win_width, self.win_height) {
            Picked::Chose(path) => {
                self.picked(&path);
                return true;
            }
            Picked::Handled | Picked::Cancelled => return true,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key) if key.pressed => self.handle_key(key),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            _ => false,
        }
    }

    /// Ask where to export the page, as an SVG picture.
    pub fn save_as(&mut self) {
        self.picker_for = PickerFor::Export;
        self.picker.open_to_write(self.offered_name(".svg"));
    }

    /// The board's name: its file's, or "Untitled".
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

    /// The name offered for an export or a first save, with `extension`:
    /// the file's own bytes, not a decoding of them. A name that is not UTF-8
    /// decoded and offered back would save to a different file -- one with
    /// U+FFFD where the bytes were -- beside the one the user opened.
    fn offered_name(&self, extension: &str) -> std::ffi::OsString {
        let mut name = self
            .document_path
            .as_deref()
            .and_then(std::path::Path::file_stem)
            .map_or_else(
                || std::ffi::OsString::from("whiteboard"),
                std::ffi::OsString::from,
            );
        name.push(extension);
        name
    }

    /// The board as a document: every page, its layers, and every shape on
    /// them with its ink. YAML, as `apps/slides` keeps a deck, versioned
    /// under `slateos-whiteboard` so a later format is refused rather than
    /// half-read. Not the SVG, which is a picture of one page and is not read
    /// back.
    pub fn board_document(&self) -> yamldoc::Document {
        let mut doc = yamldoc::Document::new();
        doc.set_i64(&["slateos-whiteboard"], BOARD_FORMAT);
        let id = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        for (pi, page) in self.pages.iter().enumerate() {
            let pk = pi.saturating_add(1).to_string();
            let pk = pk.as_str();
            doc.set_str(&["pages", pk, "name"], &page.name);
            for (li, layer) in page.layers.iter().enumerate() {
                let lk = li.saturating_add(1).to_string();
                let at = |f: &'static str| ["pages", pk, "layers", lk.as_str(), f];
                doc.set_i64(&at("id"), id(layer.id));
                doc.set_str(&at("name"), &layer.name);
                doc.set_bool(&at("visible"), layer.visible);
                doc.set_bool(&at("locked"), layer.locked);
                doc.set_f64(&at("opacity"), f64::from(layer.opacity));
            }
            for (si, shape) in page.shapes.iter().enumerate() {
                let sk = si.saturating_add(1).to_string();
                let at = |f: &'static str| ["pages", pk, "shapes", sk.as_str(), f];
                doc.set_i64(&at("id"), id(shape.id));
                doc.set_i64(&at("layer"), id(shape.layer_id));
                doc.set_str(&at("colour"), &colour_hex(shape.stroke.color));
                doc.set_i64(&at("thickness"), i64::from(shape.stroke.thickness));
                doc.set_f64(&at("opacity"), f64::from(shape.stroke.opacity));
                doc.set_str(
                    &at("style"),
                    match shape.stroke.style {
                        StrokeStyle::Solid => "solid",
                        StrokeStyle::Dashed => "dashed",
                    },
                );
                let point =
                    |doc: &mut yamldoc::Document, x: &'static str, y: &'static str, p: Point| {
                        doc.set_f64(&at(x), f64::from(p.x));
                        doc.set_f64(&at(y), f64::from(p.y));
                    };
                let rect = |doc: &mut yamldoc::Document, r: Rect| {
                    doc.set_f64(&at("x"), f64::from(r.x));
                    doc.set_f64(&at("y"), f64::from(r.y));
                    doc.set_f64(&at("width"), f64::from(r.width));
                    doc.set_f64(&at("height"), f64::from(r.height));
                };
                match &shape.kind {
                    ShapeKind::Freehand { points } => {
                        doc.set_str(&at("kind"), "freehand");
                        let pts: Vec<String> =
                            points.iter().map(|p| format!("{} {}", p.x, p.y)).collect();
                        let pts: Vec<&str> = pts.iter().map(String::as_str).collect();
                        doc.set_seq(&at("points"), &pts);
                    }
                    ShapeKind::Line { start, end } => {
                        doc.set_str(&at("kind"), "line");
                        point(&mut doc, "x1", "y1", *start);
                        point(&mut doc, "x2", "y2", *end);
                    }
                    ShapeKind::Arrow { start, end } => {
                        doc.set_str(&at("kind"), "arrow");
                        point(&mut doc, "x1", "y1", *start);
                        point(&mut doc, "x2", "y2", *end);
                    }
                    ShapeKind::Rectangle { bounds } => {
                        doc.set_str(&at("kind"), "rectangle");
                        rect(&mut doc, *bounds);
                    }
                    ShapeKind::Ellipse { bounds } => {
                        doc.set_str(&at("kind"), "ellipse");
                        rect(&mut doc, *bounds);
                    }
                    ShapeKind::TextLabel { position, content } => {
                        doc.set_str(&at("kind"), "text");
                        point(&mut doc, "x", "y", *position);
                        doc.set_str(&at("text"), content);
                    }
                    ShapeKind::StickyNote {
                        bounds,
                        content,
                        bg_color,
                    } => {
                        doc.set_str(&at("kind"), "note");
                        rect(&mut doc, *bounds);
                        doc.set_str(&at("text"), content);
                        doc.set_str(&at("background"), &colour_hex(*bg_color));
                    }
                }
            }
        }
        doc
    }

    /// Write the board to `path`, which becomes its file. What to say.
    pub fn write_board(&mut self, path: &std::path::Path) -> String {
        match safeio::write_str_atomically(path, &self.board_document().to_text()) {
            Ok(()) => {
                self.document_path = Some(path.to_path_buf());
                self.dirty = false;
                format!("Saved {}", path.display())
            }
            Err(err) => format!("Could not save {}: {err}", path.display()),
        }
    }

    /// Replace the board with the one in `path`. What to say. A file that is
    /// not a board this can read leaves this one as it was.
    pub fn read_board(&mut self, path: &std::path::Path) -> String {
        self.read_board_within(path, MAX_BOARD_BYTES)
    }

    /// [`read_board`](Self::read_board), refusing a file over `max` bytes.
    fn read_board_within(&mut self, path: &std::path::Path, max: usize) -> String {
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
        match board_from_document(&yamldoc::Document::parse(&read.text)) {
            Ok((pages, left_out)) => {
                self.pages = pages;
                self.active_page = 0;
                self.active_layer_id = self.current_page().layers.first().map_or(1, |l| l.id);
                self.selection.clear();
                self.drag = DragState::None;
                self.document_path = Some(path.to_path_buf());
                self.dirty = false;
                // Said, not hidden: saving now would write the board
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

    /// Ctrl+S: over the board's own file, or ask where when it has none.
    pub fn save(&mut self) {
        match self.document_path.clone() {
            Some(path) => self.status_message = Some(self.write_board(&path)),
            None => self.ask_where_to_save(PickerFor::Save),
        }
    }

    /// Put the save picker up, for `purpose`, beside the board's own file
    /// when it has one.
    fn ask_where_to_save(&mut self, purpose: PickerFor) {
        let start = self
            .document_path
            .as_deref()
            .and_then(std::path::Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(FilePicker::default_start, std::path::Path::to_path_buf);
        let name = self.offered_name(".whiteboard");
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
            PickerFor::Open => self.read_board(path),
            PickerFor::Save => self.write_board(path),
            PickerFor::Export => self.write_svg(path),
            PickerFor::SaveThen(pending) => {
                let said = self.write_board(path);
                if !self.dirty {
                    self.go_on(pending);
                }
                said
            }
        };
        self.status_message = Some(said);
    }

    /// Before something replaces or closes the board: ask about unsaved
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
    /// save worked: a board that could not be written is still the only copy.
    fn answer(&mut self, pending: Pending, choice: Choice) {
        match choice {
            Choice::Cancel => {}
            Choice::Discard => self.go_on(pending),
            Choice::Save => match self.document_path.clone() {
                Some(path) => {
                    self.status_message = Some(self.write_board(&path));
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

    /// Write the current page to `path`, and say what happened.
    pub fn write_svg(&mut self, path: &std::path::Path) -> String {
        let text = self.export_svg();
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!(
                "Saved {} shape(s) to {}",
                self.current_page().shapes.len(),
                path.display()
            ),
            Err(err) => format!("Could not write {}: {err}", path.display()),
        }
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> bool {
        let (x, y) = (event.x, event.y);
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                if let Some(tool) = self.tool_at(x, y) {
                    self.current_tool = tool;
                    return true;
                }
                if let Some(color) = self.swatch_at(x, y) {
                    self.set_stroke_color(color);
                    return true;
                }
                if let Some(page) = self.page_tab_at(x, y) {
                    self.switch_page(page);
                    return true;
                }
                if self.add_page_button().contains(x, y) {
                    self.add_page();
                    return true;
                }
                if self.canvas_rect().contains(x, y) {
                    self.on_canvas_press(x, y, self.shift_held);
                    return true;
                }
                false
            }
            MouseEventKind::Press(MouseButton::Middle) => {
                // The middle button pans, whatever tool is chosen -- which is
                // what `start_pan` was written for and nothing called.
                if self.canvas_rect().contains(x, y) {
                    self.start_pan(x, y);
                    return true;
                }
                false
            }
            MouseEventKind::Move => {
                if matches!(self.drag, DragState::None) {
                    return false;
                }
                self.on_canvas_move(x, y);
                true
            }
            MouseEventKind::Release(MouseButton::Left | MouseButton::Middle) => {
                if matches!(self.drag, DragState::None) {
                    return false;
                }
                self.on_canvas_release(x, y);
                true
            }
            MouseEventKind::Scroll { dy, .. } => {
                if !self.canvas_rect().contains(x, y) {
                    return false;
                }
                // `dy` is in notches, positive away from the user, which is
                // the direction every program zooms in. `on_scroll` only reads
                // its sign, so the notch count passes through untouched --
                // multiplying it by a pixel constant is the mistake the
                // toolkit's own doc warns about.
                self.on_scroll(x, y, dy);
                true
            }
            _ => false,
        }
    }

    fn handle_key(&mut self, event: &KeyEvent) -> bool {
        // Shift is read by the canvas press for multi-select, and the only
        // record of it is the modifier on whichever event arrives next.
        self.shift_held = event.modifiers.shift;

        // Above the Ctrl branch, which returns for every chord, and above the
        // typed-character path, which claims every unmodified letter.
        if event.key == Key::F1 {
            self.show_help = !self.show_help;
            return true;
        }
        if self.show_help {
            // Modal. Delete removes the selection and a letter changes tool,
            // and neither should happen behind a list somebody is reading.
            if matches!(event.key, Key::Escape | Key::Enter) {
                self.show_help = false;
            }
            return true;
        }

        if event.modifiers.ctrl {
            return match event.key {
                Key::Z => {
                    if event.modifiers.shift {
                        self.redo();
                    } else {
                        self.undo();
                    }
                    true
                }
                Key::Y => {
                    self.redo();
                    true
                }
                Key::A => {
                    self.select_all();
                    true
                }
                Key::S => {
                    if event.modifiers.shift {
                        self.ask_where_to_save(PickerFor::Save);
                    } else {
                        self.save();
                    }
                    true
                }
                Key::O => {
                    self.unless_unsaved(Pending::Open);
                    true
                }
                // What Ctrl+S did before a board could be saved: a picture of
                // the page, which nothing reads back.
                Key::E => {
                    self.save_as();
                    true
                }
                Key::L => {
                    // Plain `L` is the Line tool, so the panel takes control.
                    self.show_layers_panel = !self.show_layers_panel;
                    true
                }
                _ => false,
            };
        }

        match event.key {
            Key::Delete | Key::Backspace => {
                if self.selection.is_empty() {
                    return false;
                }
                self.delete_selected();
                true
            }
            Key::Escape => {
                if self.selection.is_empty() && matches!(self.drag, DragState::None) {
                    return false;
                }
                self.drag = DragState::None;
                self.selection.clear();
                true
            }
            Key::Left => self.nudge(-1.0, 0.0),
            Key::Right => self.nudge(1.0, 0.0),
            Key::Up => self.nudge(0.0, -1.0),
            Key::Down => self.nudge(0.0, 1.0),
            _ => self.handle_typed(event),
        }
    }

    fn handle_typed(&mut self, event: &KeyEvent) -> bool {
        let Some(ch) = event.typed().next() else {
            return false;
        };
        // Single letters pick a tool, the way every drawing program binds
        // them. `Tool::from_shortcut` is the one table that dispatches them
        // and the one the shortcut card draws from.
        //
        // This comment used to say "both this and the toolbar's tooltips can
        // read". There are no tooltips: the word appeared exactly once in
        // this crate, here, describing a reader that was never built. So the
        // nine tool letters were dispatched and drawn nowhere, and the
        // comment was the reason nobody looked.
        if let Some(tool) = Tool::from_shortcut(ch) {
            self.current_tool = tool;
            return true;
        }
        match ch {
            '+' | '=' => {
                self.zoom_in();
                true
            }
            '-' | '_' => {
                self.zoom_out();
                true
            }
            '0' => {
                self.zoom_to_fit();
                true
            }
            'g' | 'G' => {
                self.show_grid = !self.show_grid;
                true
            }
            '#' => {
                self.snap_to_grid = !self.snap_to_grid;
                true
            }
            _ => false,
        }
    }

    /// The card's tool rows, read from the table that dispatches them.
    ///
    /// Owned strings because the letters come out of `Tool::shortcut` as
    /// `char`. The label is the same abbreviation the toolbar button shows,
    /// deliberately: a reader matching a card row to a button wants the two
    /// to say the same word.
    fn tool_rows() -> Vec<(String, String)> {
        Tool::all()
            .iter()
            .filter_map(|tool| {
                tool.shortcut()
                    .map(|ch| (ch.to_string(), format!("{} tool", tool.label())))
            })
            .collect()
    }

    /// Select every shape on the current page.
    pub fn select_all(&mut self) {
        self.selection.shape_ids = self
            .current_page()
            .shapes
            .iter()
            .map(|shape| shape.id)
            .collect();
        self.selection.marquee = None;
    }

    /// Move the selection by a step. Returns whether anything moved.
    fn nudge(&mut self, dx: f32, dy: f32) -> bool {
        if self.selection.is_empty() {
            return false;
        }
        // The arrow keys move by a whole grid square when the grid is being
        // snapped to, so a nudged shape lands back on it rather than one
        // pixel off it.
        let step = if self.snap_to_grid { GRID_SIZE } else { 1.0 };
        self.move_selected(dx * step, dy * step);
        true
    }

    /// Render the entire UI to a list of render commands.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        let mut cmds = Vec::new();

        // Background
        self.palette.push_surface(
            &mut cmds,
            0.0,
            0.0,
            self.win_width,
            self.win_height,
            0.0,
            Surface::Card,
        );

        self.render_top_bar(&mut cmds);
        self.render_page_tabs(&mut cmds);
        self.render_toolbar(&mut cmds);
        self.render_canvas(&mut cmds);
        if self.show_layers_panel {
            self.render_layers_panel(&mut cmds);
        }
        self.render_status_bar(&mut cmds);

        if self.show_help {
            let tools = Self::tool_rows();
            let mut rows: Vec<(&str, &str)> = SHORTCUTS.to_vec();
            rows.extend(tools.iter().map(|(k, w)| (k.as_str(), w.as_str())));
            guitk::shortcut::render_card(
                &mut cmds,
                &self.palette,
                (self.win_width, self.win_height),
                0.0,
                &rows,
                "F1 closes this",
            );
        }

        // The picker last, so it draws over the canvas rather than under it.
        cmds.extend(
            self.picker
                .render(&self.palette, self.win_width, self.win_height),
        );

        cmds
    }

    // ------ Top bar ------

    fn render_top_bar(&self, cmds: &mut Vec<RenderCommand>) {
        // Background
        self.palette.push_surface(
            cmds,
            0.0,
            0.0,
            self.win_width,
            TOP_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        // Title
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: "Whiteboard".to_string(),
            color: self.palette.text,
            font_size: 15.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Stroke info
        let thickness_label = format!("{}px", self.stroke_props.thickness);
        cmds.push(RenderCommand::Text {
            x: 130.0,
            y: 14.0,
            text: thickness_label,
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Opacity label
        let opacity_label = format!("{}%", (self.stroke_props.opacity * 100.0) as u32);
        cmds.push(RenderCommand::Text {
            x: 180.0,
            y: 14.0,
            text: opacity_label,
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Grid toggle indicator
        let grid_label = if self.show_grid {
            "Grid:ON"
        } else {
            "Grid:OFF"
        };
        cmds.push(RenderCommand::Text {
            x: 300.0,
            y: 14.0,
            text: grid_label.to_string(),
            color: if self.show_grid {
                self.palette.ink(self.palette.green)
            } else {
                self.palette.subtext0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
                self.palette.ink(self.palette.blue)
            } else {
                self.palette.subtext0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            color: self.palette.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(4.0),
        });

        // Zoom display
        let zoom_pct = format!("{:.0}%", self.zoom * 100.0);
        cmds.push(RenderCommand::Text {
            x: 500.0,
            y: 14.0,
            text: zoom_pct,
            color: self.palette.subtext0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOP_BAR_HEIGHT,
            x2: self.win_width,
            y2: TOP_BAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });
    }

    // ------ Page tabs ------

    fn render_page_tabs(&self, cmds: &mut Vec<RenderCommand>) {
        let y = TOP_BAR_HEIGHT;
        self.palette.push_surface(
            cmds,
            0.0,
            y,
            self.win_width,
            PAGE_TAB_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );

        let tabs = self.page_tabs();
        let mut tx = TOOLBAR_WIDTH + 4.0;
        for ((i, rect), page) in tabs.iter().zip(self.pages.iter()) {
            let is_active = *i == self.active_page;
            let (tx_start, tab_width) = (rect.x, rect.width);
            tx = tx_start;

            let bg = if is_active {
                self.palette.base
            } else {
                self.palette.surface0
            };
            cmds.push(RenderCommand::FillRect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                color: bg,
                corner_radii: CornerRadii {
                    top_left: 4.0,
                    top_right: 4.0,
                    bottom_left: 0.0,
                    bottom_right: 0.0,
                },
            });

            let text_color = if is_active {
                self.palette.text
            } else {
                self.palette.subtext0
            };
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
                overflow: TextOverflow::Ellipsis,
            });

            tx = tx_start + tab_width + 4.0;
        }
        let _ = tx;

        // "+" button to add page
        let plus = self.add_page_button();
        self.palette.push_surface(
            cmds,
            plus.x,
            plus.y,
            plus.width,
            plus.height,
            4.0,
            Surface::Card,
        );
        cmds.push(RenderCommand::Text {
            x: plus.x + 7.0,
            y: plus.y + 3.0,
            text: "+".to_string(),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Bottom border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y + PAGE_TAB_HEIGHT,
            x2: self.win_width,
            y2: y + PAGE_TAB_HEIGHT,
            color: self.palette.surface0,
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
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });

        // Tool buttons, from the same rectangles the pointer is tested
        // against.
        let buttons = self.tool_buttons();
        let mut ty = y_start + 8.0;
        for (tool, rect) in &buttons {
            let is_active = *tool == self.current_tool;
            let bg = if is_active {
                self.palette.blue
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
                width: rect.width,
                height: rect.height,
                color: bg,
                corner_radii: CornerRadii::all(6.0),
            });

            cmds.push(RenderCommand::Text {
                x: rect.x + 4.0,
                y: rect.y + 10.0,
                text: tool.label().to_string(),
                color: fg,
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(rect.width - 4.0),
                overflow: TextOverflow::Ellipsis,
            });

            ty = rect.y + TOOL_BUTTON_STEP;
        }

        // Color palette below tools
        ty += PALETTE_HEADING_GAP;
        cmds.push(RenderCommand::Line {
            x1: 6.0,
            y1: ty,
            x2: 46.0,
            y2: ty,
            color: self.palette.surface1,
            width: 1.0,
        });
        ty += 8.0;

        // Title
        cmds.push(RenderCommand::Text {
            x: 6.0,
            y: ty,
            text: "Colors".to_string(),
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // 2-column palette swatches, from the rectangles the click reads.
        for (color, swatch) in self.palette_swatches() {
            let (sx, sy) = (swatch.x, swatch.y);

            cmds.push(RenderCommand::FillRect {
                x: sx,
                y: sy,
                width: PALETTE_SWATCH_SIZE,
                height: PALETTE_SWATCH_SIZE,
                color,
                corner_radii: CornerRadii::all(3.0),
            });

            // Highlight the active color
            if color == self.stroke_props.color {
                cmds.push(RenderCommand::StrokeRect {
                    x: sx - 1.0,
                    y: sy - 1.0,
                    width: PALETTE_SWATCH_SIZE + 2.0,
                    height: PALETTE_SWATCH_SIZE + 2.0,
                    color: self.palette.text,
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
            color: self.palette.surface0,
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
            color: self.palette.base,
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
            let screen_end = self.canvas_to_screen(r.x + r.width, r.y + r.height);
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
                color: self.palette.blue,
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

    fn render_shape(&self, cmds: &mut Vec<RenderCommand>, shape: &Shape, selected: bool) {
        // NOT inked, and this is the whiteboard's answer to
        // `gui/appearance/ink-text.py --blind`, which reports the shorthand
        // below: a stroke's colour is the *drawing*, chosen by the user and
        // saved with the file, in the sense `apps/paint`'s swatch row is the
        // document. Flooring it for contrast would edit the picture.
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
                    overflow: TextOverflow::Clip,
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
                // Text (dark for readability on colored background), one
                // command per wrapped line. `RenderCommand::Text` clips at
                // `max_width` rather than wrapping, so the whole content used
                // to come out as the first line's worth of characters and
                // nothing else — a note is a paragraph the user wrote and
                // expects to read back.
                let text_width = bounds.width - STICKY_PADDING * 2.0;
                for (n, line) in sticky_note_lines(bounds, content).iter().enumerate() {
                    let line_top = STICKY_PADDING + n as f32 * STICKY_LINE_HEIGHT;
                    // A note is a fixed box the user drew; text that does not
                    // fit is left undrawn rather than spilling onto the canvas
                    // over whatever else is there.
                    if line_top + STICKY_LINE_HEIGHT > bounds.height {
                        break;
                    }
                    cmds.push(RenderCommand::Text {
                        x: (bounds.x + STICKY_PADDING) * self.zoom,
                        y: (bounds.y + line_top) * self.zoom,
                        text: line.clone(),
                        color: self.palette.crust,
                        font_size: STICKY_FONT_SIZE * self.zoom,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(text_width * self.zoom),
                        overflow: TextOverflow::Ellipsis,
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
                color: self.palette.blue,
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
                    .get(
                        self.sticky_color_index
                            .checked_rem(STICKY_COLORS.len())
                            .unwrap_or(0),
                    )
                    .copied()
                    .unwrap_or(self.palette.yellow);
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
        self.palette.push_surface(
            cmds,
            panel_x,
            panel_y,
            RIGHT_PANEL_WIDTH,
            panel_h,
            0.0,
            Surface::Strip(Edge::Top),
        );

        // Title
        cmds.push(RenderCommand::Text {
            x: panel_x + 8.0,
            y: panel_y + 8.0,
            text: "Layers".to_string(),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // "+" add layer button
        self.palette.push_surface(
            cmds,
            panel_x + RIGHT_PANEL_WIDTH - 30.0,
            panel_y + 4.0,
            22.0,
            22.0,
            4.0,
            Surface::Card,
        );
        cmds.push(RenderCommand::Text {
            x: panel_x + RIGHT_PANEL_WIDTH - 25.0,
            y: panel_y + 7.0,
            text: "+".to_string(),
            color: self.palette.text,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Layer rows
        let page = self.current_page();
        let mut ly = panel_y + 30.0;
        for layer in page.layers.iter().rev() {
            let is_active = layer.id == self.active_layer_id;
            let row_bg = if is_active {
                self.palette.surface0
            } else {
                self.palette.mantle
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
                self.palette.green
            } else {
                self.palette.overlay0
            };
            cmds.push(RenderCommand::Text {
                x: panel_x + 10.0,
                y: ly + 9.0,
                text: if layer.visible { "V" } else { "-" }.to_string(),
                color: vis_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Lock icon
            let lock_color = if layer.locked {
                self.palette.red
            } else {
                self.palette.overlay0
            };
            cmds.push(RenderCommand::Text {
                x: panel_x + 26.0,
                y: ly + 9.0,
                text: if layer.locked { "L" } else { "." }.to_string(),
                color: lock_color,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            // Layer name
            cmds.push(RenderCommand::Text {
                x: panel_x + 42.0,
                y: ly + 9.0,
                text: layer.name.clone(),
                color: self.palette.text,
                font_size: 11.0,
                font_weight: if is_active {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(RIGHT_PANEL_WIDTH - 80.0),
                overflow: TextOverflow::Ellipsis,
            });

            // Opacity indicator
            let opacity_str = format!("{:.0}%", layer.opacity * 100.0);
            cmds.push(RenderCommand::Text {
                x: panel_x + RIGHT_PANEL_WIDTH - 40.0,
                y: ly + 9.0,
                text: opacity_str,
                color: self.palette.subtext0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            ly += LAYER_ROW_HEIGHT + 2.0;
        }

        // Left border
        cmds.push(RenderCommand::Line {
            x1: panel_x,
            y1: panel_y,
            x2: panel_x,
            y2: panel_y + panel_h,
            color: self.palette.surface0,
            width: 1.0,
        });
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

        // Top border
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.win_width,
            y2: y,
            color: self.palette.surface0,
            width: 1.0,
        });

        // Tool name
        cmds.push(RenderCommand::Text {
            x: 8.0,
            y: y + 6.0,
            text: format!("Tool: {}", self.current_tool.label()),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Shape count
        let shape_count = self.current_page().shapes.len();
        cmds.push(RenderCommand::Text {
            x: 120.0,
            y: y + 6.0,
            text: format!("Shapes: {}", shape_count),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Selection count
        let sel_count = self.selection.shape_ids.len();
        if sel_count > 0 {
            cmds.push(RenderCommand::Text {
                x: 240.0,
                y: y + 6.0,
                text: format!("Selected: {}", sel_count),
                color: self.palette.ink(self.palette.blue),
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
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
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Undo/redo counts
        cmds.push(RenderCommand::Text {
            x: 500.0,
            y: y + 6.0,
            text: format!(
                "Undo:{} Redo:{}",
                self.current_page().undo_stack.len(),
                self.current_page().redo_stack.len()
            ),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
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
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // What the last save, open or export did. It was recorded and drawn
        // nowhere, so a save that failed looked like one that worked.
        if let Some(note) = &self.status_message {
            cmds.push(RenderCommand::Text {
                x: STATUS_NOTE_X,
                y: y + 6.0,
                text: note.clone(),
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some((self.win_width - STATUS_NOTE_X - 8.0).max(0.0)),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

impl App for WhiteboardApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    /// The board's file and the page being drawn on, marked `*` while the
    /// board has changes not saved.
    fn title(&self) -> String {
        let page = self.current_page();
        let shapes = page.shapes.len();
        format!(
            "{}{}: {} ({shapes}) - Whiteboard",
            if self.dirty { "*" } else { "" },
            self.document_name(),
            page.name
        )
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (WINDOW_WIDTH as u32, WINDOW_HEIGHT as u32)
        }
    }

    /// No clock.
    ///
    /// Nothing on a whiteboard moves on its own: a stroke appears when it is
    /// drawn and stays where it was put. Asking the harness for a tick would
    /// wake the machine on a schedule to find the same drawing still there --
    /// `known-issues.md` lesson 47.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        match event {
            // Closing over unsaved changes asks first, and the window waits
            // for the answer: `KeepOpen` declines the close.
            Event::CloseRequested => {
                if self.request_close() {
                    Response::Exit
                } else {
                    Response::KeepOpen
                }
            }
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension in pixels is exact in f32"
                )]
                let (w, h) = (*width as f32, *height as f32);
                if self.set_window_size(w, h) {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
            other => {
                let changed = self.handle_event(other);
                if self.quit {
                    Response::Exit
                } else if changed {
                    Response::Redraw
                } else {
                    Response::Idle
                }
            }
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // The handed size wins over the recorded one: the first frame is drawn
        // before any `Event::Resize` arrives, so a window opened at another
        // size would be laid out for the size that was asked for, and every
        // hit box in it would name the wrong rectangle.
        self.set_window_size(width, height);
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

/// A colour as `#RRGGBB`, or `#RRGGBBAA` when it is not opaque -- the
/// spelling `apps/slides` writes, so the files read alike.
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

/// A point written as "x y", as a freehand stroke's points are.
fn parse_point(text: &str) -> Option<Point> {
    let mut parts = text.split_whitespace();
    let x = parts
        .next()?
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())?;
    let y = parts
        .next()?
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())?;
    parts.next().is_none().then_some(Point::new(x, y))
}

/// Read a board written by [`WhiteboardApp::board_document`].
///
/// Refuses a file that is not a board, one from a later version, one with no
/// pages, and a page whose shapes or layers share an id -- each would be read
/// as some other board. A shape of a kind this does not know, or missing what
/// it is drawn from, is left out: the rest is still the user's board. A page
/// with no layers gets one, since every shape has to be on one. How many
/// things were left out comes back with the board, so the user is told.
fn board_from_document(doc: &yamldoc::Document) -> Result<(Vec<Page>, usize), String> {
    match doc.get_i64(&["slateos-whiteboard"]) {
        None => return Err(String::from("it is not a SlateOS whiteboard")),
        Some(v) if v > BOARD_FORMAT => {
            return Err(format!(
                "it is a later format ({v}) than this version reads ({BOARD_FORMAT})"
            ));
        }
        Some(_) => {}
    }
    let mut left_out = 0_usize;
    let mut pages = Vec::new();
    for pk in positions(doc, &["pages"]) {
        let pk = pk.as_str();
        let name = doc
            .get_str(&["pages", pk, "name"])
            .unwrap_or_else(|| format!("Board {}", pages.len().saturating_add(1)));
        let mut page = Page::new(name);
        page.layers.clear();
        let mut layer_ids = std::collections::HashSet::new();
        for lk in positions(doc, &["pages", pk, "layers"]) {
            let at = |f: &'static str| ["pages", pk, "layers", lk.as_str(), f];
            let Some(id) = read_id(doc, &at("id")) else {
                left_out = left_out.saturating_add(1);
                continue;
            };
            if !layer_ids.insert(id) {
                return Err(format!("two layers on {} share the id {id}", page.name));
            }
            let mut layer = Layer::new(
                id,
                doc.get_str(&at("name"))
                    .unwrap_or_else(|| format!("Layer {id}")),
            );
            layer.visible = doc.get_bool(&at("visible")).unwrap_or(true);
            layer.locked = doc.get_bool(&at("locked")).unwrap_or(false);
            layer.opacity = read_f32(doc, &at("opacity")).map_or(1.0, |o| o.clamp(0.0, 1.0));
            page.layers.push(layer);
        }
        if page.layers.is_empty() {
            page.layers.push(Layer::new(1, String::from("Layer 1")));
            layer_ids.insert(1);
        }
        let first_layer = page.layers.first().map_or(1, |l| l.id);
        let mut shape_ids = std::collections::HashSet::new();
        for sk in positions(doc, &["pages", pk, "shapes"]) {
            let at = |f: &'static str| ["pages", pk, "shapes", sk.as_str(), f];
            let Some(id) = read_id(doc, &at("id")) else {
                left_out = left_out.saturating_add(1);
                continue;
            };
            let point = |x: &'static str, y: &'static str| {
                Some(Point::new(read_f32(doc, &at(x))?, read_f32(doc, &at(y))?))
            };
            let rect = || {
                Some(Rect::new(
                    read_f32(doc, &at("x"))?,
                    read_f32(doc, &at("y"))?,
                    read_f32(doc, &at("width"))?,
                    read_f32(doc, &at("height"))?,
                ))
            };
            let text = || doc.get_str(&at("text")).unwrap_or_default();
            let kind = match doc.get_str(&at("kind")).as_deref() {
                Some("freehand") => {
                    let points: Option<Vec<Point>> = doc
                        .get_seq(&at("points"))
                        .unwrap_or_default()
                        .iter()
                        .map(|p| parse_point(p))
                        .collect();
                    points.map(|points| ShapeKind::Freehand { points })
                }
                Some("line") => point("x1", "y1")
                    .zip(point("x2", "y2"))
                    .map(|(start, end)| ShapeKind::Line { start, end }),
                Some("arrow") => point("x1", "y1")
                    .zip(point("x2", "y2"))
                    .map(|(start, end)| ShapeKind::Arrow { start, end }),
                Some("rectangle") => rect().map(|bounds| ShapeKind::Rectangle { bounds }),
                Some("ellipse") => rect().map(|bounds| ShapeKind::Ellipse { bounds }),
                Some("text") => point("x", "y").map(|position| ShapeKind::TextLabel {
                    position,
                    content: text(),
                }),
                Some("note") => rect().map(|bounds| ShapeKind::StickyNote {
                    bounds,
                    content: text(),
                    bg_color: doc
                        .get_str(&at("background"))
                        .and_then(|t| parse_colour(&t))
                        .unwrap_or(STICKY_COLORS[0]),
                }),
                _ => None,
            };
            let Some(kind) = kind else {
                left_out = left_out.saturating_add(1);
                continue;
            };
            if !shape_ids.insert(id) {
                return Err(format!("two shapes on {} share the id {id}", page.name));
            }
            let mut stroke = StrokeProps::default();
            if let Some(c) = doc.get_str(&at("colour")).and_then(|t| parse_colour(&t)) {
                stroke.color = c;
            }
            if let Some(t) = doc
                .get_i64(&at("thickness"))
                .and_then(|t| u8::try_from(t).ok())
            {
                stroke.thickness = t;
            }
            if let Some(o) = read_f32(doc, &at("opacity")) {
                stroke.opacity = o.clamp(0.0, 1.0);
            }
            if doc.get_str(&at("style")).as_deref() == Some("dashed") {
                stroke.style = StrokeStyle::Dashed;
            }
            let layer_id = read_id(doc, &at("layer"))
                .filter(|l| layer_ids.contains(l))
                .unwrap_or(first_layer);
            page.shapes.push(Shape {
                id,
                kind,
                stroke,
                layer_id,
            });
        }
        // New shapes and layers go past every id the file used.
        page.next_shape_id = shape_ids.iter().max().map_or(1, |m| m.saturating_add(1));
        page.next_layer_id = layer_ids.iter().max().map_or(2, |m| m.saturating_add(1));
        pages.push(page);
    }
    if pages.is_empty() {
        return Err(String::from("it holds no boards"));
    }
    Ok((pages, left_out))
}

fn main() -> ExitCode {
    let mut app = WhiteboardApp::new(WINDOW_WIDTH, WINDOW_HEIGHT);
    app::launch("whiteboard", &mut app)
}

#[cfg(test)]
mod tests {
    // A test that indexes out of range should fail loudly and point at the
    // line that did it -- that is the diagnosis. The defensive lints exist to
    // keep panics out of code that runs on a user's data, which this is not,
    // and a window size is a value the code was handed and stored verbatim.
    #![allow(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::float_cmp,
        clippy::arithmetic_side_effects
    )]

    use super::*;

    // ---- Undo that can undo a deletion, one history per page ----

    /// **A deleted shape comes back on undo**, where it was. The deletion was
    /// recorded after the shape was gone, by id alone, so undo found nothing
    /// to put back.
    #[test]
    fn undo_puts_a_deleted_shape_back_where_it_was() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let a = app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        });
        let b = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        });
        let c = app.add_shape(ShapeKind::Ellipse {
            bounds: Rect::new(9.0, 9.0, 5.0, 5.0),
        });
        let order = |app: &WhiteboardApp| -> Vec<ShapeId> {
            app.current_page().shapes.iter().map(|s| s.id).collect()
        };
        app.selection.shape_ids = vec![a, c];
        app.delete_selected();
        assert_eq!(order(&app), vec![b]);
        app.undo();
        assert_eq!(
            order(&app),
            vec![a, b, c],
            "the deletion was not undone in place"
        );
        app.redo();
        assert_eq!(order(&app), vec![b]);
        app.undo();
        assert_eq!(order(&app), vec![a, b, c]);
    }

    /// A deleted layer comes back with its shapes, where they were.
    #[test]
    fn undo_puts_a_deleted_layer_back_with_its_shapes() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let first = app.active_layer_id;
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        });
        app.add_layer();
        let second = app.active_layer_id;
        let on_second = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        });
        app.active_layer_id = first;
        app.add_shape(ShapeKind::Line {
            start: Point::new(2.0, 2.0),
            end: Point::new(3.0, 3.0),
        });
        let before: Vec<ShapeId> = app.current_page().shapes.iter().map(|s| s.id).collect();
        app.delete_layer(second);
        assert!(app.current_page().get_shape(on_second).is_none());
        app.undo();
        let after: Vec<ShapeId> = app.current_page().shapes.iter().map(|s| s.id).collect();
        assert_eq!(
            after, before,
            "the layer's shapes did not come back in place"
        );
        assert!(app.current_page().layers.iter().any(|l| l.id == second));
    }

    /// Each page keeps its own history: undo on one page never touches
    /// another. It was one history for the window, replayed onto whichever
    /// page was showing.
    #[test]
    fn undo_acts_only_on_the_page_it_is_about() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        });
        app.add_page();
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        });
        app.switch_page(0);
        app.undo();
        assert!(
            app.pages[0].shapes.is_empty(),
            "undo on the first page did nothing"
        );
        assert_eq!(
            app.pages[1].shapes.len(),
            1,
            "undo on the first page took the second page's shape"
        );
        // Redo brings back the first page's line -- one history for the
        // window would put the second page's rectangle here instead, both
        // shapes being number 1 on their own page.
        app.redo();
        assert!(
            matches!(
                app.pages[0].shapes.as_slice(),
                [Shape {
                    kind: ShapeKind::Line { .. },
                    ..
                }]
            ),
            "redo on the first page brought back something else: {:?}",
            app.pages[0].shapes
        );
        app.switch_page(1);
        app.undo();
        assert!(
            app.pages[1].shapes.is_empty(),
            "the second page's own undo did nothing"
        );
        assert_eq!(
            app.pages[0].shapes.len(),
            1,
            "the second page's undo reached the first"
        );
        // Its redo brings back its own rectangle. The undo above cannot tell
        // on its own: the two shapes are both number 1 on their own page, so
        // taking the first page's history would still empty this page -- and
        // leave the line to come back here.
        app.redo();
        assert!(
            matches!(
                app.pages[1].shapes.as_slice(),
                [Shape {
                    kind: ShapeKind::Rectangle { .. },
                    ..
                }]
            ),
            "redo on the second page brought back something else: {:?}",
            app.pages[1].shapes
        );
        app.switch_page(0);
        app.undo();
        assert!(
            app.pages[0].shapes.is_empty(),
            "the first page's history was spent by the second page's undo"
        );
    }

    // ---- A board that can be saved and opened again ----
    //
    // It could be exported as a picture of one page, which nothing read, and
    // the window said so in a banner. Nothing recorded unsaved changes, and
    // the window closed over them.

    /// A scratch directory of the test's own.
    fn scratch(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "slateos-whiteboard-{tag}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// The picker chose `path`, as it does in a window: it takes itself down,
    /// then the choice is acted on.
    fn choose(app: &mut WhiteboardApp, path: &std::path::Path) {
        assert!(app.picker.is_open(), "nothing was asking for a file");
        app.picker.close();
        app.picked(path);
    }

    /// Everything a board holds, in a fixed order.
    fn describe(app: &WhiteboardApp) -> String {
        let mut out = Vec::new();
        for page in &app.pages {
            out.push(format!(
                "page {:?} next {} {}",
                page.name, page.next_shape_id, page.next_layer_id
            ));
            for l in &page.layers {
                out.push(format!(
                    "  layer {} {:?} {} {} {}",
                    l.id, l.name, l.visible, l.locked, l.opacity
                ));
            }
            for sh in &page.shapes {
                out.push(format!(
                    "  shape {} {:?} {:?} {} {} {:?} {}",
                    sh.id,
                    sh.kind,
                    sh.stroke.color,
                    sh.stroke.thickness,
                    sh.stroke.opacity,
                    sh.stroke.style,
                    sh.layer_id
                ));
            }
        }
        out.join("\n")
    }

    /// A board using everything a file has to keep: every kind of shape, ink
    /// set away from its default, a second page, and a hidden, locked layer.
    fn rich() -> WhiteboardApp {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.stroke_props.color = Color::rgba(10, 20, 30, 200);
        app.stroke_props.thickness = 7;
        app.stroke_props.opacity = 0.5;
        app.stroke_props.style = StrokeStyle::Dashed;
        app.add_shape(ShapeKind::Freehand {
            points: vec![
                Point::new(1.5, 2.25),
                Point::new(-3.0, 4.0),
                Point::new(5.0, 6.0),
            ],
        });
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 1.0),
            end: Point::new(2.0, 3.0),
        });
        app.add_shape(ShapeKind::Arrow {
            start: Point::new(4.0, 5.0),
            end: Point::new(6.0, 7.0),
        });
        app.add_layer();
        let layer = app.active_layer_id;
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(1.0, 2.0, 30.0, 40.0),
        });
        app.add_shape(ShapeKind::Ellipse {
            bounds: Rect::new(5.0, 6.0, 7.0, 8.0),
        });
        app.toggle_layer_visibility(layer);
        app.toggle_layer_lock(layer);
        app.set_layer_opacity(layer, 0.25);
        app.add_page();
        app.pages[1].name = String::from("Second: \"quoted\" # page");
        app.add_shape(ShapeKind::TextLabel {
            position: Point::new(9.0, 10.0),
            content: String::from("hello: world # not a comment"),
        });
        app.add_shape(ShapeKind::StickyNote {
            bounds: Rect::new(11.0, 12.0, 100.0, 80.0),
            content: String::from("line one\nline two"),
            bg_color: Color::rgb(166, 227, 161),
        });
        app
    }

    /// **A board saved and opened again is the same board** -- every page,
    /// layer and shape, with its ink.
    #[test]
    fn a_board_saved_and_opened_again_is_the_same_board() {
        let dir = scratch("roundtrip");
        let path = dir.join("plan.whiteboard");
        let mut app = rich();
        let said = app.write_board(&path);
        assert!(said.starts_with("Saved"), "{said}");
        assert!(!app.dirty);

        let mut other = WhiteboardApp::new(800.0, 600.0);
        let said = other.read_board(&path);
        assert!(said.starts_with("Opened"), "{said}");
        assert_eq!(describe(&other), describe(&app));
        assert!(!other.dirty, "a board just opened has nothing unsaved");
        assert!(
            other.title().starts_with("plan.whiteboard: "),
            "{}",
            other.title()
        );
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A file this cannot read as a board is refused, and the board open stays
    /// as it was.
    #[test]
    fn a_file_that_is_not_a_board_is_refused() {
        let dir = scratch("refused");
        let mut app = rich();
        let before = describe(&app);
        let cases: [(&str, &[u8], &str); 5] = [
            ("settings.yaml", b"fonts:\n  size: 13\n", "not a SlateOS whiteboard"),
            ("later.whiteboard", b"slateos-whiteboard: 2\n", "later format"),
            ("empty.whiteboard", b"slateos-whiteboard: 1\n", "holds no boards"),
            ("binary.whiteboard", b"\xff\xfe\x00junk", "Could not open"),
            (
                "twice.whiteboard",
                b"slateos-whiteboard: 1\npages:\n  1:\n    name: P\n    shapes:\n      1:\n        id: 4\n        kind: line\n        x1: 0\n        y1: 0\n        x2: 1\n        y2: 1\n      2:\n        id: 4\n        kind: line\n        x1: 0\n        y1: 0\n        x2: 1\n        y2: 1\n",
                "share the id 4",
            ),
        ];
        for (name, bytes, why) in cases {
            let path = dir.join(name);
            std::fs::write(&path, bytes).expect("fixture");
            let said = app.read_board(&path);
            assert!(
                said.starts_with("Could not open") && said.contains(why),
                "{name}: {said}"
            );
            assert_eq!(describe(&app), before, "{name} changed the board");
        }

        // Larger than the most this reads: refused whole, never read as the
        // smaller board its first part would parse as.
        let big = dir.join("big.whiteboard");
        rich().write_board(&big);
        let said = app.read_board_within(&big, 64);
        assert!(said.contains("larger than the 64"), "{said}");
        assert_eq!(describe(&app), before, "a file cut short changed the board");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// What this version does not know is left out and the rest read; a page
    /// with no layers gets one; new shapes take ids past the file's.
    #[test]
    fn what_is_not_understood_is_left_out_and_the_rest_read() {
        let dir = scratch("partial");
        let path = dir.join("partial.whiteboard");
        std::fs::write(
            &path,
            concat!(
                "slateos-whiteboard: 1\n",
                "pages:\n",
                "  1:\n",
                "    name: Only\n",
                "    shapes:\n",
                "      1:\n        id: 7\n        kind: hologram\n",
                "      2:\n        id: 9\n        kind: line\n        x1: 0\n        y1: 0\n        x2: 1\n        y2: 1\n",
                "      3:\n        id: 11\n        kind: rectangle\n        x: 1\n",
            ),
        )
        .expect("fixture");
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let said = app.read_board(&path);
        assert!(said.starts_with("Opened"), "{said}");
        assert!(
            said.contains("leaving out 2"),
            "what was left out was not said: {said}"
        );
        let ids: Vec<ShapeId> = app.current_page().shapes.iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            vec![9],
            "an unknown kind or a shape missing its size was read"
        );
        assert_eq!(app.current_page().layers.len(), 1);
        let added = app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        });
        assert!(added > 9, "{added} took an id at or below the file's");
        drop(std::fs::remove_dir_all(&dir));
    }

    /// Ctrl+S asks where the first time and saves over the board's own file
    /// after; Ctrl+E exports a picture, which is not a save.
    #[test]
    fn ctrl_s_saves_and_ctrl_e_exports() {
        let dir = scratch("keys");
        let path = dir.join("mine.whiteboard");
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(1.0, 1.0),
        });
        assert!(app.dirty && app.title().starts_with('*'), "{}", app.title());
        app.handle_event(&press_with(Key::S, ctrl()));
        assert_eq!(app.picker_for, PickerFor::Save);
        choose(&mut app, &path);
        assert!(!app.dirty);

        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        });
        app.handle_event(&press_with(Key::S, ctrl()));
        assert!(!app.picker.is_open(), "a board with a file asked again");
        let mut other = WhiteboardApp::new(800.0, 600.0);
        other.read_board(&path);
        assert_eq!(other.current_page().shapes.len(), 2);

        app.add_page();
        assert!(app.dirty, "a new page is a change");
        app.handle_event(&press_with(Key::E, ctrl()));
        assert_eq!(app.picker_for, PickerFor::Export);
        choose(&mut app, &dir.join("page.svg"));
        assert!(app.dirty, "an export cleared the unsaved mark");
        assert_eq!(app.document_path.as_deref(), Some(path.as_path()));
        drop(std::fs::remove_dir_all(&dir));
    }

    /// **Closing or opening over unsaved changes asks**, and each answer is
    /// kept.
    #[test]
    fn closing_or_opening_over_unsaved_changes_asks() {
        let dir = scratch("close");
        let path = dir.join("kept.whiteboard");
        let mut clean = WhiteboardApp::new(800.0, 600.0);
        assert_eq!(clean.on_event(&Event::CloseRequested), Response::Exit);

        let mut app = rich();
        app.write_board(&path);
        let on_disk = std::fs::read(&path).expect("saved");
        app.add_shape(ShapeKind::Line {
            start: Point::new(3.0, 3.0),
            end: Point::new(4.0, 4.0),
        });
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        let text: String = app
            .render(800.0, 600.0)
            .commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("kept.whiteboard has changes that are not saved."),
            "{text}"
        );

        // A tool key goes to the question, not the canvas.
        let tool = app.current_tool;
        app.on_event(&typed(Key::L, 'l'));
        assert_eq!(
            app.current_tool, tool,
            "a key reached the canvas under the question"
        );

        assert_eq!(app.on_event(&press(Key::Escape)), Response::Redraw);
        assert_eq!(std::fs::read(&path).expect("unchanged"), on_disk);
        assert_eq!(app.on_event(&Event::CloseRequested), Response::KeepOpen);
        assert_eq!(app.on_event(&press(Key::S)), Response::Exit);
        assert_ne!(std::fs::read(&path).expect("saved again"), on_disk);

        let mut app = rich();
        app.handle_event(&press_with(Key::O, ctrl()));
        assert!(!app.picker.is_open(), "Ctrl+O opened over unsaved changes");
        app.handle_event(&press(Key::D));
        assert!(app.picker.is_open() && app.picker_for == PickerFor::Open);
        drop(std::fs::remove_dir_all(&dir));
    }

    /// A file name that is not text: "a" and a byte -- on Windows, a lone
    /// UTF-16 surrogate -- that decodes to nothing.
    fn not_text_stem() -> std::ffi::OsString {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStringExt;
            std::ffi::OsString::from_wide(&[0x61, 0xD800])
        }
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(b"a\xff").to_os_string()
        }
    }

    /// The name a save or an export is offered under is the file's own. It
    /// was decoded, so a name that is not text came back with U+FFFD in it,
    /// and saving under it made a second file beside the one opened.
    #[test]
    fn a_name_that_is_not_text_is_offered_as_itself() {
        let stem = not_text_stem();
        assert!(
            stem.to_str().is_none(),
            "control: the name is text after all"
        );
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let mut file = stem.clone();
        file.push(".whiteboard");
        app.document_path = Some(std::env::temp_dir().join(&file));
        let mut want = stem;
        want.push(".svg");
        assert_eq!(app.offered_name(".svg"), want);
        assert_eq!(app.offered_name(".whiteboard"), file);
    }

    /// What a save did is on the screen. It was recorded and drawn nowhere.
    #[test]
    fn the_status_bar_says_what_the_last_save_did() {
        let dir = scratch("status");
        let mut app = rich();
        let said = app.write_board(&dir.join("missing").join("x.whiteboard"));
        app.status_message = Some(said);
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
        let shortcuts: Vec<char> = Tool::all().iter().filter_map(|t| t.shortcut()).collect();
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::StickyNote {
            bounds: Rect::new(50.0, 50.0, 150.0, 100.0),
            content: "Note".to_string(),
            bg_color: pal.yellow,
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
                points: vec![Point::new(10.0, 20.0), Point::new(50.0, 60.0)],
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
        assert!(!app.current_page().redo_stack.is_empty());
        // New action should clear redo
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        });
        assert!(app.current_page().redo_stack.is_empty());
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
        assert!(app.current_page().undo_stack.len() <= MAX_UNDO_STEPS);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.set_stroke_color(pal.red);
        assert_eq!(app.stroke_props.color, pal.red);
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
        assert!(matches!(
            app.current_page().shapes[0].kind,
            ShapeKind::Freehand { .. }
        ));
    }

    #[test]
    fn test_line_draw() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.current_tool = Tool::Line;
        let area = app.canvas_rect();
        app.on_canvas_press(area.x + 10.0, area.y + 10.0, false);
        app.on_canvas_release(area.x + 100.0, area.y + 100.0);
        assert_eq!(app.current_page().shapes.len(), 1);
        assert!(matches!(
            app.current_page().shapes[0].kind,
            ShapeKind::Line { .. }
        ));
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
        assert!(matches!(
            app.current_page().shapes[0].kind,
            ShapeKind::TextLabel { .. }
        ));
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
        assert!(matches!(
            app.current_page().shapes[0].kind,
            ShapeKind::StickyNote { .. }
        ));
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

    // ---- The door ----

    /// An open picker takes the keyboard, and the canvas behind it does not.
    ///
    /// The other door test pins that Ctrl+S *opens* the picker -- but the key
    /// handler is what opens it, so cutting the picker's event routing
    /// entirely leaves that assertion true. This is the half routing actually
    /// decides: with a dialog up, a letter is a filename, not a tool
    /// shortcut, and a drawing program that switched to the Line tool while
    /// you typed `line.svg` is one where the dialog is not modal.
    ///
    /// Found by `scripts/find-unpinned-picker-routing.py`, which cuts the
    /// routing and reports whose tests notice. Sixteen of twenty did not.
    #[test]
    fn an_open_picker_takes_the_keyboard_from_the_canvas() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.current_tool = Tool::Pen;

        app.handle_event(&press_with(Key::S, ctrl()));
        assert!(app.picker.is_open(), "control: the picker must be up");

        // `l` is the Line tool's shortcut, and the first letter of a filename
        // somebody might reasonably type.
        app.handle_event(&typed(Key::L, 'l'));
        assert_eq!(
            app.current_tool,
            Tool::Pen,
            "a letter typed at the save dialog changed the tool behind it"
        );
    }

    /// The drawing reaches the disk, and reads back as what was drawn.
    ///
    /// `export_svg` was written and tested and had no caller: this program had
    /// no `std::fs`, no `safeio` and no dialog of any kind, so **a drawing
    /// lived exactly as long as the window did.**
    #[test]
    fn a_drawing_reaches_the_disk() {
        let dir = std::env::temp_dir().join(format!(
            "whiteboard-save-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("board.svg");
        let _ = std::fs::remove_file(&path);

        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Line {
            start: Point::new(1.0, 2.0),
            end: Point::new(3.0, 4.0),
        });
        let expected = app.export_svg();
        let said = app.write_svg(&path);

        let back = std::fs::read_to_string(&path).expect("the file it said it wrote");
        assert_eq!(back, expected, "what was read back is not what was drawn");
        assert!(said.starts_with("Saved 1 shape(s)"), "said: {said}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ctrl+S asks where to put it rather than saving somewhere of its own.
    #[test]
    fn ctrl_s_opens_the_save_picker() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        assert!(!app.picker.is_open(), "nothing should be open at rest");

        assert!(app.handle_event(&press_with(Key::S, ctrl())));
        assert!(app.picker.is_open(), "Ctrl+S should ask for a destination");
    }

    /// The open picker is drawn, not merely routed to.
    #[test]
    fn the_open_picker_is_drawn() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        let closed = app.render_commands().len();
        app.handle_event(&press_with(Key::S, ctrl()));
        let own = app.render_commands().len().saturating_sub(closed);
        assert!(
            own > 0,
            "the open picker contributed {own} commands; it is not being drawn"
        );
    }

    // ---- Export ----

    /// An empty page is still a valid, openable SVG document.
    ///
    /// It asserted a `<whiteboard>` root, which is what made the old export
    /// un-openable: no SVG reader has any idea what that element is.
    #[test]
    fn test_export_empty_page() {
        let app = WhiteboardApp::new(800.0, 600.0);
        let svg = app.export_svg();
        assert!(svg.starts_with("<?xml"), "{svg}");
        assert!(
            svg.contains("<svg xmlns=\"http://www.w3.org/2000/svg\""),
            "without the namespace it is markup, not an SVG: {svg}"
        );
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("<title>Board 1</title>"));
        assert!(
            !svg.contains("viewBox=\"0 0 0 0\""),
            "a zero viewBox shows a viewer nothing at all: {svg}"
        );
    }

    /// Every shape kind comes out as an element SVG actually has.
    ///
    /// **Three of the six did not.** `<path points="...">` took an attribute
    /// `<path>` does not have -- `points` belongs to `<polyline>` -- so every
    /// freehand stroke silently vanished, which is most of what a whiteboard
    /// holds. `<arrow>` and `<sticky>` are not SVG elements at all. And the
    /// two that did survive, `<rect>` and `<ellipse>`, carried no `fill`,
    /// whose SVG initial value is **black**, so they came out as solid boxes
    /// over whatever they had been drawn around.
    #[test]
    fn every_shape_kind_uses_an_element_svg_has() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Freehand {
            points: vec![Point { x: 1.0, y: 2.0 }, Point { x: 3.0, y: 4.0 }],
        });
        app.add_shape(ShapeKind::Arrow {
            start: Point { x: 0.0, y: 0.0 },
            end: Point { x: 9.0, y: 9.0 },
        });
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect {
                x: 1.0,
                y: 1.0,
                width: 5.0,
                height: 5.0,
            },
        });
        let svg = app.export_svg();

        assert!(
            svg.contains("<polyline points=\"1.0,2.0 3.0,4.0\""),
            "{svg}"
        );
        assert!(
            !svg.contains("<path"),
            "path takes `d`, never `points`: {svg}"
        );
        assert!(!svg.contains("<arrow"), "not an SVG element: {svg}");
        assert!(
            svg.contains("marker-end=\"url(#arrowhead)\"")
                && svg.contains("<marker id=\"arrowhead\""),
            "a marker-end whose id is defined nowhere draws no head, and nothing in the document complains: {svg}"
        );
        // Every stroked element says so, or SVG fills it black.
        assert_eq!(
            svg.matches("fill=\"none\"").count(),
            3,
            "a stroked shape without fill=\"none\" is a filled black one: {svg}"
        );
    }

    /// Text a user types must not be able to change the structure of the
    /// export. A sticky note reading `</sticky><rect/>` previously closed its
    /// own element and injected a sibling.
    #[test]
    fn user_text_cannot_inject_elements_into_the_export() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::TextLabel {
            position: Point { x: 1.0, y: 2.0 },
            content: "</text><rect x=\"0\"/><text>".to_string(),
        });
        app.add_shape(ShapeKind::StickyNote {
            bounds: Rect {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            },
            content: "</sticky><rect x=\"0\"/><sticky>".to_string(),
            bg_color: Color::rgb(1, 2, 3),
        });
        let svg = app.export_svg();

        // Exactly the elements the two shapes imply and no more: a text label
        // is one `<text>`, and a sticky note is a `<rect>` with a `<text>` on
        // it, because SVG has no element that is both. The old assertion of
        // zero `<rect>` cannot be reused for that reason -- what it was really
        // checking is that the user's `<rect x="0"/>` did not become a third
        // element, which is what these counts say now.
        assert_eq!(svg.matches("<text ").count(), 2, "{svg}");
        assert_eq!(svg.matches("</text>").count(), 2, "{svg}");
        assert_eq!(svg.matches("<rect ").count(), 1, "injected element: {svg}");
        assert!(!svg.contains("<sticky"), "{svg}");
        assert!(
            svg.contains("&lt;/text&gt;") && svg.contains("&lt;/sticky&gt;"),
            "the typed markup should appear escaped, not dropped: {svg}"
        );
    }

    /// The same for an attribute: a quote in a page or layer name must not be
    /// able to close the attribute and add another one.
    #[test]
    fn a_quote_in_a_page_name_cannot_close_the_attribute() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.current_page_mut().name = "My\" evil=\"yes".to_string();
        let svg = app.export_svg();

        // The page name is an element's text now, not an attribute value, so
        // a quote cannot close anything -- but it still must not be able to
        // open an element, which is what this checks.
        let line = svg
            .lines()
            .find(|l| l.trim_start().starts_with("<title>"))
            .expect("a title line");
        assert!(line.contains("&quot;") || !line.contains('"'), "{line}");
        assert!(!line.contains("evil=\""), "{line}");
    }

    /// An ampersand is the character that makes the difference between an
    /// export that parses and one that does not.
    #[test]
    fn an_ampersand_in_user_text_is_escaped() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::TextLabel {
            position: Point { x: 0.0, y: 0.0 },
            content: "Tom & Jerry".to_string(),
        });
        let svg = app.export_svg();
        assert!(svg.contains("Tom &amp; Jerry"), "{svg}");
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
        let svg = app.export_svg();
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
        let svg = app.export_svg();
        assert!(svg.contains("<text"));
        assert!(svg.contains("Hello"));
    }

    #[test]
    fn test_export_sticky_note() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::StickyNote {
            bounds: Rect::new(10.0, 10.0, 100.0, 80.0),
            content: "Important".to_string(),
            bg_color: pal.yellow,
        });
        let svg = app.export_svg();
        // `<sticky>` is not an SVG element and never was, so a sticky note
        // drew nothing at all. It is a filled rectangle with the note on top.
        assert!(!svg.contains("<sticky"), "{svg}");
        assert!(svg.contains("<rect "), "{svg}");
        assert!(svg.contains("Important"), "{svg}");
        assert!(
            svg.contains("fill=\"rgb(") && svg.contains("stroke=\"none\""),
            "the note's paper is filled and unstroked: {svg}"
        );
    }

    #[test]
    fn test_export_arrow() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Arrow {
            start: Point::new(0.0, 0.0),
            end: Point::new(50.0, 50.0),
        });
        let svg = app.export_svg();
        // `<arrow>` is not an SVG element. A line with a marker on its end is.
        assert!(!svg.contains("<arrow"), "{svg}");
        assert!(svg.contains("<line "), "{svg}");
        assert!(svg.contains("marker-end=\"url(#arrowhead)\""), "{svg}");
        assert!(
            svg.contains("<marker id=\"arrowhead\""),
            "a marker-end pointing at nothing draws no head: {svg}"
        );
    }

    #[test]
    fn test_export_ellipse() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.add_shape(ShapeKind::Ellipse {
            bounds: Rect::new(0.0, 0.0, 80.0, 60.0),
        });
        let svg = app.export_svg();
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
        let svg = app.export_svg();
        // `<path points=...>` was the worst of the three: `<path>` is a real
        // element, so the document parsed, and `points` is simply ignored --
        // **the stroke vanished without any error anywhere.**
        assert!(!svg.contains("<path"), "{svg}");
        assert!(
            svg.contains("<polyline points=\"0.0,0.0 5.0,5.0 10.0,0.0\""),
            "{svg}"
        );
    }

    #[test]
    fn test_export_dashed_stroke() {
        let mut app = WhiteboardApp::new(800.0, 600.0);
        app.stroke_props.style = StrokeStyle::Dashed;
        app.add_shape(ShapeKind::Line {
            start: Point::new(0.0, 0.0),
            end: Point::new(50.0, 50.0),
        });
        let svg = app.export_svg();
        assert!(svg.contains("5,5"));
    }

    // ---- Rendering ----

    #[test]
    fn test_render_not_empty() {
        let app = WhiteboardApp::new(1280.0, 800.0);
        let cmds = app.render_commands();
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
        let cmds = app.render_commands();
        assert!(cmds.len() > 10);
    }

    /// The `(y, text)` of every line drawn into a sticky note at `bounds`.
    ///
    /// Identified by the note's own left edge, so nothing else drawn in the
    /// note's colour can be mistaken for its body.
    fn sticky_lines_drawn(app: &WhiteboardApp, bounds: &Rect) -> Vec<(f32, String)> {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let left = (bounds.x + STICKY_PADDING) * app.zoom;
        app.render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    x, y, text, color, ..
                } if color == pal.crust && (x - left).abs() < 0.01 => Some((y, text)),
                _ => None,
            })
            .collect()
    }

    /// A note big enough for several lines of the paragraph below.
    fn app_with_sticky(bounds: Rect, content: &str) -> WhiteboardApp {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.add_shape(ShapeKind::StickyNote {
            bounds,
            content: content.to_string(),
            bg_color: pal.yellow,
        });
        app
    }

    const STICKY_PARAGRAPH: &str = "Remember to check the deployment schedule \
        with the release team before the freeze, and to file the rollback plan \
        alongside it so nobody has to invent one under pressure.";

    #[test]
    fn a_long_sticky_note_is_wrapped_not_truncated_to_one_line() {
        // `RenderCommand::Text` clips at `max_width`, so the whole note used to
        // come out as its first line's worth of characters and nothing else.
        let bounds = Rect::new(40.0, 40.0, 220.0, 240.0);
        let app = app_with_sticky(bounds, STICKY_PARAGRAPH);
        let lines = sticky_lines_drawn(&app, &bounds);

        assert!(
            lines.len() > 1,
            "the note was drawn as {} command(s); it needs one per line",
            lines.len()
        );
        // Every word the user typed is still on the note somewhere.
        let drawn: String = lines
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for word in STICKY_PARAGRAPH.split_whitespace() {
            assert!(drawn.contains(word), "the note lost the word {word:?}");
        }
    }

    #[test]
    fn sticky_note_text_stays_inside_the_note() {
        // A note is a fixed box the user drew, so text longer than it fits is
        // dropped rather than spilling onto the canvas over other shapes.
        let bounds = Rect::new(40.0, 40.0, 120.0, 60.0);
        let app = app_with_sticky(bounds, STICKY_PARAGRAPH);

        let bottom = (bounds.y + bounds.height) * app.zoom;
        for (y, text) in sticky_lines_drawn(&app, &bounds) {
            assert!(
                y + STICKY_LINE_HEIGHT * app.zoom <= bottom + 0.01,
                "line {text:?} at {y} runs past the bottom of the note at {bottom}"
            );
        }
    }

    #[test]
    fn zooming_a_sticky_note_does_not_reflow_it() {
        // Line breaks are laid out in canvas units, so they are a property of
        // the note rather than of how closely the user is looking at it.
        let bounds = Rect::new(40.0, 40.0, 220.0, 400.0);
        let mut app = app_with_sticky(bounds, STICKY_PARAGRAPH);
        let at_1x: Vec<String> = sticky_lines_drawn(&app, &bounds)
            .into_iter()
            .map(|(_, t)| t)
            .collect();

        app.set_zoom(2.5);
        let at_2_5x: Vec<String> = sticky_lines_drawn(&app, &bounds)
            .into_iter()
            .map(|(_, t)| t)
            .collect();

        assert_eq!(at_1x, at_2_5x, "zooming re-broke the note's lines");
    }

    #[test]
    fn an_empty_sticky_note_draws_no_text() {
        let bounds = Rect::new(40.0, 40.0, 150.0, 100.0);
        let app = app_with_sticky(bounds, "");
        assert!(sticky_lines_drawn(&app, &bounds).is_empty());
    }

    #[test]
    fn test_render_with_layers_panel_hidden() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.show_layers_panel = false;
        let cmds = app.render_commands();
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
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_drag_preview_freehand() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.drag = DragState::DrawingFreehand {
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
        };
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_marquee_preview() {
        let mut app = WhiteboardApp::new(1280.0, 800.0);
        app.drag = DragState::Marquee {
            start: Point::new(10.0, 10.0),
            current: Point::new(100.0, 100.0),
        };
        let cmds = app.render_commands();
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

    // ======================================================================
    // Window, toolbar and keyboard
    // ======================================================================

    use guitk::event::Modifiers;

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

    /// **Every tool letter is on the card and picks its tool.**
    ///
    /// Both halves read `Tool::shortcut`, which is also what dispatches them,
    /// so there is nothing here that can drift out of step with the app. Nine
    /// letters -- P, L, R, O, A, T, E, S, N -- were dispatched and drawn
    /// nowhere until this card; the comment above `from_shortcut` said the
    /// toolbar's tooltips read them, and the word "tooltip" appeared exactly
    /// once in this crate, in that sentence.
    #[test]
    fn every_tool_letter_is_on_the_card_and_picks_its_tool() {
        let mut app = board();
        app.handle_event(&press_with(Key::F1, Modifiers::NONE));
        let shown = drawn_text(&app);

        let mut letters = 0;
        for tool in Tool::all() {
            let Some(ch) = tool.shortcut() else { continue };
            letters += 1;
            // `Vec::contains`, which compares whole elements -- not
            // `str::contains`, which asks about substrings. The two share a
            // name and do not share a meaning, and the substring one is what
            // this test had.
            // `render_card` draws each row's keys as their own `Text`
            // command, and a one-character `contains` is satisfied by any
            // word holding that letter -- "N" is in "Nudge the selection", so
            // the first draft of this test passed with the Note tool deleted
            // from the card. That is the same defect `names_the_key` exists
            // to fix in the survey, written again here by the person who
            // fixed it there.
            assert!(
                shown.contains(&ch.to_string()),
                "the card never shows {ch:?} for the {} tool",
                tool.label()
            );
            let row = format!("{} tool", tool.label());
            assert!(
                shown.contains(&row),
                "the card never names the {} tool",
                tool.label()
            );

            let mut pressing = board();
            pressing.current_tool = Tool::Select;
            // Lowercase and with `text` set, which is how a keyboard sends a
            // letter: `typed()` reads the text a layout produced and nothing
            // else, so a synthetic event with an empty string reaches no
            // character handler at all.
            pressing.handle_event(&typed(Key::A, ch.to_ascii_lowercase()));
            assert_eq!(
                pressing.current_tool,
                *tool,
                "{ch:?} is on the card and does not pick the {} tool",
                tool.label()
            );
        }
        assert_eq!(
            letters, 9,
            "the tool table changed size; the card follows it"
        );
    }

    /// **Every other key the card advertises is answered.**
    ///
    /// Split from the tool letters because these arrive as *keys* and those
    /// arrive as *text*. `keystrokes` builds a `KeyEvent` with an empty
    /// string, which is right for `Ctrl+S` and useless for `g`.
    #[test]
    fn every_advertised_key_does_something() {
        for (label, what) in SHORTCUTS {
            if matches!(*label, "+" | "-" | "0" | "G" | "#") {
                continue; // typed characters; covered below
            }
            for stroke in guitk::shortcut::keystrokes(label).unwrap_or_else(|e| panic!("{e}")) {
                let answered = [false, true].into_iter().any(|with_selection| {
                    let mut app = board();
                    if with_selection {
                        let id = app.add_shape(ShapeKind::Rectangle {
                            bounds: Rect::new(0.0, 0.0, 40.0, 40.0),
                        });
                        app.selection.shape_ids = vec![id];
                    }
                    app.handle_event(&Event::Key(stroke.clone()))
                });
                assert!(
                    answered,
                    "the card advertises {label:?} for {what:?}, and nothing answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// **The typed-character rows do what the card says.**
    ///
    /// These never reach `handle_typed` as a `Key`, so they are pressed the
    /// way a keyboard sends them, with the character in `text`.
    #[test]
    fn every_typed_row_on_the_card_does_something() {
        for (label, ch) in [("+", '+'), ("-", '-'), ("0", '0'), ("G", 'g'), ("#", '#')] {
            assert!(
                SHORTCUTS.iter().any(|(k, _w)| *k == label),
                "{label:?} is tested here and is not on the card"
            );
            let mut app = board();
            assert!(
                app.handle_event(&typed(Key::A, ch)),
                "the card advertises {label:?} and nothing answers it"
            );
        }
    }

    /// Every string this window draws, kept apart rather than joined.
    ///
    /// Joined into one haystack, `contains` answers a question nobody asked:
    /// whether the letter appears *anywhere*, including inside another row's
    /// description.
    fn drawn_text(app: &WhiteboardApp) -> Vec<String> {
        app.render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    fn press_with(k: Key, modifiers: Modifiers) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers,
            text: String::new(),
        })
    }

    fn typed(k: Key, ch: char) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: ch.to_string(),
        })
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            ctrl: true,
            shift: true,
            ..Modifiers::NONE
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    fn click_at(x: f32, y: f32) -> Event {
        mouse(x, y, MouseEventKind::Press(MouseButton::Left))
    }

    fn board() -> WhiteboardApp {
        WhiteboardApp::new(WINDOW_WIDTH, WINDOW_HEIGHT)
    }

    /// The middle of the canvas, well away from every panel.
    fn canvas_centre(app: &WhiteboardApp) -> (f32, f32) {
        let r = app.canvas_rect();
        (r.x + r.width / 2.0, r.y + r.height / 2.0)
    }

    // --- the toolbar can be clicked ---

    #[test]
    fn clicking_a_tool_button_picks_that_tool() {
        let mut app = board();
        let buttons = app.tool_buttons();
        let (tool, rect) = buttons[3];
        assert_ne!(tool, app.current_tool, "the fixture must change something");
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(app.tool_at(x, y), Some(tool));
        assert!(app.handle_event(&click_at(x, y)));
        assert_eq!(app.current_tool, tool);
    }

    #[test]
    fn every_tool_button_is_reachable_where_it_is_drawn() {
        let app = board();
        for (tool, rect) in app.tool_buttons() {
            assert_eq!(
                app.tool_at(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
                Some(tool),
                "{tool:?} is drawn at {rect:?} and must be clickable there"
            );
        }
    }

    #[test]
    fn the_gap_between_two_tool_buttons_is_not_a_button() {
        let app = board();
        let buttons = app.tool_buttons();
        let first = buttons[0].1;
        let gap_y = first.y + first.height + 1.0;
        assert_eq!(
            app.tool_at(first.x + 4.0, gap_y),
            None,
            "the air between two buttons belongs to neither"
        );
    }

    #[test]
    fn clicking_a_swatch_sets_the_stroke_colour() {
        let mut app = board();
        let swatches = app.palette_swatches();
        let (color, rect) = swatches[5];
        assert_ne!(color, app.stroke_props.color);
        let (x, y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        assert_eq!(app.swatch_at(x, y), Some(color));
        assert!(app.handle_event(&click_at(x, y)));
        assert_eq!(app.stroke_props.color, color);
    }

    #[test]
    fn every_swatch_is_reachable_where_it_is_drawn() {
        let app = board();
        for (color, rect) in app.palette_swatches() {
            assert_eq!(
                app.swatch_at(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
                Some(color),
                "a swatch drawn at {rect:?} must be clickable there"
            );
        }
    }

    #[test]
    fn the_palette_starts_below_the_last_tool() {
        let app = board();
        let last = app
            .tool_buttons()
            .last()
            .map_or(0.0, |(_, r)| r.y + r.height);
        let first_swatch = app.palette_swatches().first().map_or(0.0, |(_, r)| r.y);
        assert!(
            first_swatch > last,
            "the first swatch is at {first_swatch} and the last tool ends at \
             {last}: a swatch drawn over a button is a click that does two \
             things"
        );
    }

    // --- the page strip ---

    #[test]
    fn clicking_a_page_tab_switches_to_it() {
        let mut app = board();
        app.add_page();
        app.switch_page(0);
        let tabs = app.page_tabs();
        let (index, rect) = tabs[1];
        assert!(app.handle_event(&click_at(
            rect.x + rect.width / 2.0,
            rect.y + rect.height / 2.0
        )));
        assert_eq!(app.active_page, index);
    }

    #[test]
    fn the_plus_button_adds_a_page() {
        let mut app = board();
        let before = app.page_count();
        let plus = app.add_page_button();
        assert!(app.handle_event(&click_at(
            plus.x + plus.width / 2.0,
            plus.y + plus.height / 2.0
        )));
        assert_eq!(app.page_count(), before + 1);
    }

    #[test]
    fn the_plus_button_sits_after_the_last_tab() {
        let mut app = board();
        app.add_page();
        app.add_page();
        let last = app.page_tabs().last().map_or(0.0, |(_, r)| r.x + r.width);
        assert!(
            app.add_page_button().x >= last,
            "the button that adds a page must not be drawn over one"
        );
    }

    #[test]
    fn a_wider_tab_pushes_the_next_one_along() {
        // Tab widths follow their names, so a fixed step would overlap them.
        let mut app = board();
        app.add_page();
        if let Some(page) = app.pages.first_mut() {
            page.name = "A page with a very long name indeed".to_string();
        }
        let tabs = app.page_tabs();
        assert!(
            tabs[1].1.x >= tabs[0].1.x + tabs[0].1.width,
            "the second tab starts at {} and the first ends at {}",
            tabs[1].1.x,
            tabs[0].1.x + tabs[0].1.width
        );
    }

    // --- the canvas ---

    #[test]
    fn a_press_and_drag_on_the_canvas_draws_a_shape() {
        let mut app = board();
        app.current_tool = Tool::Rectangle;
        let (x, y) = canvas_centre(&app);
        assert!(app.handle_event(&click_at(x, y)));
        assert!(app.handle_event(&mouse(x + 80.0, y + 60.0, MouseEventKind::Move)));
        assert!(app.handle_event(&mouse(
            x + 80.0,
            y + 60.0,
            MouseEventKind::Release(MouseButton::Left)
        )));
        assert_eq!(
            app.current_page().shapes.len(),
            1,
            "press, move and release are the three halves of drawing and all \
             three had to arrive from somewhere"
        );
    }

    #[test]
    fn a_click_on_the_toolbar_does_not_draw() {
        let mut app = board();
        app.current_tool = Tool::Rectangle;
        let rect = app.tool_buttons()[0].1;
        app.handle_event(&click_at(rect.x + 4.0, rect.y + 4.0));
        assert!(
            matches!(app.drag, DragState::None),
            "pressing a tool button must not also start a rectangle"
        );
        assert!(app.current_page().shapes.is_empty());
    }

    #[test]
    fn a_move_with_nothing_dragging_asks_for_no_frame() {
        let mut app = board();
        let (x, y) = canvas_centre(&app);
        assert_eq!(
            app.on_event(&mouse(x, y, MouseEventKind::Move)),
            Response::Idle,
            "redrawing on every pointer move over an idle canvas is a frame \
             spent drawing what is already there"
        );
    }

    #[test]
    fn the_wheel_zooms_the_canvas() {
        let mut app = board();
        let (x, y) = canvas_centre(&app);
        let before = app.zoom;
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: 1.0 })));
        assert!(app.zoom > before, "a notch away from the user zooms in");
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy: -1.0 })));
        assert!((app.zoom - before).abs() < 0.001);
    }

    #[test]
    fn the_wheel_outside_the_canvas_does_nothing() {
        let mut app = board();
        let before = app.zoom;
        let rect = app.tool_buttons()[0].1;
        assert!(!app.handle_event(&mouse(
            rect.x + 4.0,
            rect.y + 4.0,
            MouseEventKind::Scroll { dx: 0.0, dy: 1.0 }
        )));
        assert!((app.zoom - before).abs() < f32::EPSILON);
    }

    #[test]
    fn the_middle_button_pans() {
        let mut app = board();
        let (x, y) = canvas_centre(&app);
        assert!(app.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Middle))));
        assert!(
            matches!(app.drag, DragState::Panning { .. }),
            "`start_pan` was written for this and nothing called it"
        );
        let before = app.pan_x;
        app.handle_event(&mouse(x + 40.0, y, MouseEventKind::Move));
        assert!((app.pan_x - before).abs() > f32::EPSILON);
    }

    // --- the keyboard ---

    #[test]
    fn a_letter_picks_the_tool_the_toolbar_says_it_does() {
        let mut app = board();
        for tool in Tool::all() {
            let Some(ch) = tool.shortcut() else { continue };
            app.current_tool = Tool::Select;
            let key = match ch {
                'P' => Key::P,
                'L' => Key::L,
                'R' => Key::R,
                'O' => Key::O,
                'A' => Key::A,
                'T' => Key::T,
                'E' => Key::E,
                'S' => Key::S,
                'N' => Key::N,
                _ => continue,
            };
            assert!(app.handle_event(&typed(key, ch.to_ascii_lowercase())));
            assert_eq!(
                app.current_tool, *tool,
                "{ch} is the letter this tool has advertised since it was \
                 written, and nothing dispatched on it"
            );
        }
    }

    #[test]
    fn from_shortcut_agrees_with_shortcut_in_both_cases() {
        for tool in Tool::all() {
            let Some(ch) = tool.shortcut() else { continue };
            assert_eq!(Tool::from_shortcut(ch), Some(*tool));
            assert_eq!(Tool::from_shortcut(ch.to_ascii_lowercase()), Some(*tool));
        }
        assert_eq!(Tool::from_shortcut('9'), None);
    }

    #[test]
    fn ctrl_z_undoes_and_ctrl_shift_z_redoes() {
        let mut app = board();
        app.current_tool = Tool::Rectangle;
        let (x, y) = canvas_centre(&app);
        app.handle_event(&click_at(x, y));
        app.handle_event(&mouse(x + 60.0, y + 40.0, MouseEventKind::Move));
        app.handle_event(&mouse(
            x + 60.0,
            y + 40.0,
            MouseEventKind::Release(MouseButton::Left),
        ));
        assert_eq!(app.current_page().shapes.len(), 1);

        assert!(app.handle_event(&press_with(Key::Z, ctrl())));
        assert_eq!(app.current_page().shapes.len(), 0);
        assert!(app.handle_event(&press_with(Key::Z, ctrl_shift())));
        assert_eq!(app.current_page().shapes.len(), 1);
    }

    #[test]
    fn ctrl_a_selects_everything_and_delete_removes_it() {
        let mut app = board();
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        });
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(20.0, 20.0, 10.0, 10.0),
        });
        assert!(app.handle_event(&press_with(Key::A, ctrl())));
        assert_eq!(app.selection.shape_ids.len(), 2);
        assert!(app.handle_event(&press(Key::Delete)));
        assert!(app.current_page().shapes.is_empty());
        assert!(
            !app.handle_event(&press(Key::Delete)),
            "deleting nothing must not cost a frame"
        );
    }

    #[test]
    fn escape_drops_the_selection_and_any_drag() {
        let mut app = board();
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        });
        app.selection.add(id);
        assert!(app.handle_event(&press(Key::Escape)));
        assert!(app.selection.is_empty());
        assert!(!app.handle_event(&press(Key::Escape)));
    }

    #[test]
    fn the_arrows_nudge_the_selection() {
        let mut app = board();
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 20.0, 20.0),
        });
        app.selection.add(id);
        assert!(app.handle_event(&press(Key::Right)));
        let moved = app
            .current_page()
            .get_shape(id)
            .and_then(|s| match &s.kind {
                ShapeKind::Rectangle { bounds } => Some(bounds.x),
                _ => None,
            })
            .expect("a rectangle");
        assert!(moved > 10.0, "the shape did not move: x={moved}");
    }

    #[test]
    fn the_arrows_do_nothing_with_no_selection() {
        let mut app = board();
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 20.0, 20.0),
        });
        assert!(
            !app.handle_event(&press(Key::Up)),
            "an arrow key with nothing selected must not cost a frame"
        );
    }

    #[test]
    fn a_nudge_moves_a_whole_grid_square_when_snapping() {
        let mut app = board();
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 20.0, 20.0),
        });
        app.selection.add(id);
        app.snap_to_grid = true;
        app.handle_event(&press(Key::Right));
        let x = app
            .current_page()
            .get_shape(id)
            .and_then(|s| match &s.kind {
                ShapeKind::Rectangle { bounds } => Some(bounds.x),
                _ => None,
            })
            .expect("a rectangle");
        assert!(
            (x - GRID_SIZE).abs() < 0.01,
            "a shape nudged while snapping must land on the next grid line, \
             not one pixel off it: x={x}"
        );
    }

    #[test]
    fn the_zoom_keys_work() {
        let mut app = board();
        let before = app.zoom;
        assert!(app.handle_event(&typed(Key::Equals, '+')));
        assert!(app.zoom > before);
        assert!(app.handle_event(&typed(Key::Minus, '-')));
        assert!((app.zoom - before).abs() < 0.001);
        app.pan_x = 500.0;
        assert!(app.handle_event(&typed(Key::Num0, '0')));
    }

    #[test]
    fn g_toggles_the_grid_and_ctrl_l_the_layers_panel() {
        let mut app = board();
        let grid = app.show_grid;
        assert!(app.handle_event(&typed(Key::G, 'g')));
        assert_ne!(app.show_grid, grid);

        let panel = app.show_layers_panel;
        assert!(app.handle_event(&press_with(Key::L, ctrl())));
        assert_ne!(
            app.show_layers_panel, panel,
            "plain L is the Line tool, so the panel takes the modifier"
        );
        app.current_tool = Tool::Select;
        app.handle_event(&typed(Key::L, 'l'));
        assert_eq!(app.current_tool, Tool::Line);
    }

    #[test]
    fn shift_is_remembered_for_the_click_that_follows() {
        let mut app = board();
        let a = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 40.0, 40.0),
        });
        app.selection.add(a);
        assert!(!app.shift_held);
        app.handle_event(&press_with(
            Key::Escape,
            Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
        ));
        assert!(
            app.shift_held,
            "a mouse event carries no modifiers, so the only record of shift \
             is what the last keyboard event said"
        );
    }

    #[test]
    fn an_unbound_key_asks_for_no_frame() {
        let mut app = board();
        assert_eq!(app.on_event(&press(Key::F9)), Response::Idle);
    }

    // --- the strap ---

    #[test]
    fn the_title_names_the_page_and_counts_its_shapes() {
        let mut app = board();
        let name = app.current_page().name.clone();
        assert_eq!(app.title(), format!("Untitled: {name} (0) - Whiteboard"));
        app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(0.0, 0.0, 10.0, 10.0),
        });
        // The file's name, marked while the board has changes not saved.
        assert_eq!(app.title(), format!("*Untitled: {name} (1) - Whiteboard"));
    }

    #[test]
    fn a_whiteboard_asks_for_no_clock() {
        assert_eq!(
            board().tick_interval(),
            None,
            "nothing on a whiteboard moves on its own"
        );
    }

    #[test]
    fn a_resize_relays_out_and_a_repeat_of_it_does_not() {
        let mut app = board();
        let resize = Event::Resize {
            width: 1000,
            height: 700,
        };
        assert_eq!(app.on_event(&resize), Response::Redraw);
        assert_eq!(app.win_width, 1000.0);
        assert_eq!(app.on_event(&resize), Response::Idle);
    }

    #[test]
    fn the_canvas_follows_the_window() {
        let mut app = board();
        let narrow = app.canvas_rect();
        app.set_window_size(1920.0, 1080.0);
        let wide = app.canvas_rect();
        assert!(wide.width > narrow.width);
        assert!(wide.height > narrow.height);
    }

    #[test]
    fn a_window_dragged_tiny_keeps_a_canvas() {
        let mut app = board();
        app.set_window_size(1.0, 1.0);
        assert!(app.win_width >= MIN_WINDOW_WIDTH);
        assert!(app.win_height >= MIN_WINDOW_HEIGHT);
        assert!(app.canvas_rect().width >= 1.0);
        assert!(app.canvas_rect().height >= 1.0);
    }

    #[test]
    fn the_first_frame_uses_the_size_the_compositor_gave() {
        let mut app = board();
        let tree = app.render(1000.0, 700.0);
        assert_eq!(app.win_width, 1000.0);
        assert_eq!(app.win_height, 700.0);
        assert!(!tree.commands.is_empty());
    }

    #[test]
    fn the_close_button_exits() {
        let mut app = board();
        assert_eq!(app.on_event(&Event::CloseRequested), Response::Exit);
    }

    #[test]
    fn the_palette_is_two_columns() {
        let app = board();
        let swatches = app.palette_swatches();
        assert!(swatches.len() >= 4, "the fixture needs a few swatches");
        assert!(
            (swatches[0].1.y - swatches[1].1.y).abs() < f32::EPSILON,
            "the first two swatches share a row"
        );
        assert!(
            swatches[1].1.x > swatches[0].1.x,
            "and differ in x -- a one-column palette would be taller than the toolbar it lives in"
        );
        assert!(
            swatches[2].1.y > swatches[0].1.y,
            "the third starts the second row"
        );
    }

    #[test]
    fn a_long_page_name_gets_a_wider_tab() {
        let mut app = board();
        let narrow = app.page_tabs()[0].1.width;
        if let Some(page) = app.pages.first_mut() {
            page.name = "A page with a very long name indeed".to_string();
        }
        assert!(
            app.page_tabs()[0].1.width > narrow,
            "a tab that does not grow with its name draws the name outside itself"
        );
    }

    #[test]
    fn a_press_below_the_canvas_does_not_draw() {
        let mut app = board();
        app.current_tool = Tool::Rectangle;
        let canvas = app.canvas_rect();
        // In the status bar, under the canvas.
        let y = canvas.y + canvas.height + 4.0;
        assert!(!app.handle_event(&click_at(canvas.x + 40.0, y)));
        assert!(
            matches!(app.drag, DragState::None),
            "a press on the status bar must not start a rectangle"
        );
    }

    #[test]
    fn a_release_with_nothing_dragging_asks_for_no_frame() {
        let mut app = board();
        let (x, y) = canvas_centre(&app);
        assert_eq!(
            app.on_event(&mouse(x, y, MouseEventKind::Release(MouseButton::Left))),
            Response::Idle,
            "a release that ends nothing is not a reason to redraw"
        );
    }

    #[test]
    fn the_arrows_nudge_in_the_direction_they_point() {
        let mut app = board();
        let id = app.add_shape(ShapeKind::Rectangle {
            bounds: Rect::new(100.0, 100.0, 20.0, 20.0),
        });
        app.selection.add(id);
        let pos = |app: &WhiteboardApp| {
            app.current_page()
                .get_shape(id)
                .and_then(|s| match &s.kind {
                    ShapeKind::Rectangle { bounds } => Some((bounds.x, bounds.y)),
                    _ => None,
                })
                .expect("a rectangle")
        };
        let start = pos(&app);
        app.handle_event(&press(Key::Right));
        assert!(pos(&app).0 > start.0, "Right moves right");
        app.handle_event(&press(Key::Left));
        assert!((pos(&app).0 - start.0).abs() < 0.01, "Left brings it back");
        app.handle_event(&press(Key::Down));
        assert!(pos(&app).1 > start.1, "Down moves down");
        app.handle_event(&press(Key::Up));
        assert!((pos(&app).1 - start.1).abs() < 0.01, "Up brings it back");
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

        fn fills(app: &mut WhiteboardApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = WhiteboardApp::new(1000.0, 700.0);

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
