//! OurOS Slides
//!
//! A presentation/slideshow application with:
//! - Slide model with background color, title, subtitle, body text, bullet points
//! - Slide elements: text boxes (positioned x,y,w,h), shapes (rectangle, ellipse,
//!   line, arrow), image placeholders
//! - Multiple slide layouts: Title, Title+Content, Section Header, Blank,
//!   Two Column, Image+Caption
//! - Slide master/theme with consistent colors and font sizes
//! - Slide sorter view: thumbnail overview of all slides
//! - Edit mode: editing selected slide with element manipulation
//! - Slide transitions: Fade, SlideLeft, SlideRight, Wipe, Dissolve
//! - Speaker notes per slide
//! - Slide numbering
//! - Export to self-contained HTML slideshow
//! - Copy/paste/duplicate slides, reorder (move up/down)
//! - Undo/redo stack
//! - Keyboard shortcuts (Ctrl+N, Ctrl+D, Ctrl+Z, Ctrl+Y, Ctrl+E, etc.)
//! - Multi-panel UI: slide thumbnail sidebar, main canvas, properties panel
//!
//! Uses the guitk library for UI rendering.

#![deny(clippy::all, clippy::pedantic)]
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
const TEXT: Color = Color::from_hex(0xCDD6F4);
const SUBTEXT0: Color = Color::from_hex(0xA6ADC8);
const SUBTEXT1: Color = Color::from_hex(0xBAC2DE);
const BLUE: Color = Color::from_hex(0x89B4FA);
const OVERLAY0: Color = Color::from_hex(0x6C7086);
const TEAL: Color = Color::from_hex(0x94E2D5);
const SKY: Color = Color::from_hex(0x89DCEB);

// ============================================================================
// Layout constants
// ============================================================================

/// Width of the slide thumbnail sidebar.
const SIDEBAR_WIDTH: f32 = 180.0;
/// Width of the properties panel on the right.
const PROPERTIES_WIDTH: f32 = 220.0;
/// Height of the top toolbar.
const TOOLBAR_HEIGHT: f32 = 40.0;
/// Height of the bottom status bar.
const STATUS_BAR_HEIGHT: f32 = 24.0;
/// Padding around thumbnails.
const THUMBNAIL_PAD: f32 = 8.0;
/// Height of the notes editor area below the main canvas.
const NOTES_HEIGHT: f32 = 80.0;
/// Aspect ratio of a slide (16:9).
const SLIDE_ASPECT: f32 = 16.0 / 9.0;
/// Maximum undo/redo steps.
const MAX_UNDO: usize = 100;
/// Corner radius for panels and buttons.
const CORNER_R: f32 = 4.0;
/// Default slide width in logical units.
const SLIDE_W: f32 = 960.0;
/// Default slide height in logical units.
const SLIDE_H: f32 = 540.0;

// ============================================================================
// Unique ID generation
// ============================================================================

/// Monotonically increasing unique ID type.
pub type ElementId = u64;
/// Slide ID type.
pub type SlideId = u64;

/// Simple monotonic ID counter.
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
// Slide transition
// ============================================================================

/// Transition effect between slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// No transition (instant cut).
    None,
    /// Fade in/out.
    Fade,
    /// Slide from the right to the left.
    SlideLeft,
    /// Slide from the left to the right.
    SlideRight,
    /// Wipe from left to right.
    Wipe,
    /// Dissolve (pixelated fade).
    Dissolve,
}

impl Transition {
    /// Human-readable label for this transition.
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Fade => "Fade",
            Self::SlideLeft => "Slide Left",
            Self::SlideRight => "Slide Right",
            Self::Wipe => "Wipe",
            Self::Dissolve => "Dissolve",
        }
    }

    /// All available transitions.
    pub fn all() -> &'static [Self] {
        &[
            Self::None,
            Self::Fade,
            Self::SlideLeft,
            Self::SlideRight,
            Self::Wipe,
            Self::Dissolve,
        ]
    }
}

// ============================================================================
// Slide layout templates
// ============================================================================

/// Predefined slide layout templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlideLayout {
    /// Title slide with centered title and subtitle.
    TitleSlide,
    /// Title at top with content body below.
    TitleContent,
    /// Section header — large centered text for section breaks.
    SectionHeader,
    /// Blank slide with no predefined elements.
    Blank,
    /// Two columns of content side by side.
    TwoColumn,
    /// Image placeholder on the left, caption on the right.
    ImageCaption,
}

impl SlideLayout {
    /// Human-readable label for this layout.
    pub fn label(self) -> &'static str {
        match self {
            Self::TitleSlide => "Title Slide",
            Self::TitleContent => "Title + Content",
            Self::SectionHeader => "Section Header",
            Self::Blank => "Blank",
            Self::TwoColumn => "Two Column",
            Self::ImageCaption => "Image + Caption",
        }
    }

    /// All available layouts.
    pub fn all() -> &'static [Self] {
        &[
            Self::TitleSlide,
            Self::TitleContent,
            Self::SectionHeader,
            Self::Blank,
            Self::TwoColumn,
            Self::ImageCaption,
        ]
    }
}

// ============================================================================
// Shape kind
// ============================================================================

/// The kind of shape an element can be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// Filled or stroked rectangle.
    Rectangle,
    /// Filled or stroked ellipse.
    Ellipse,
    /// A straight line.
    Line,
    /// A line with an arrowhead at the end.
    Arrow,
}

impl ShapeKind {
    /// Human-readable label for this shape.
    pub fn label(self) -> &'static str {
        match self {
            Self::Rectangle => "Rectangle",
            Self::Ellipse => "Ellipse",
            Self::Line => "Line",
            Self::Arrow => "Arrow",
        }
    }
}

// ============================================================================
// Slide element
// ============================================================================

/// A single element placed on a slide.
#[derive(Clone, Debug)]
pub enum SlideElement {
    /// A text box with position, size, and styled text content.
    TextBox {
        id: ElementId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        text: String,
        font_size: f32,
        color: Color,
        bold: bool,
        centered: bool,
    },
    /// A geometric shape.
    Shape {
        id: ElementId,
        kind: ShapeKind,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        fill_color: Color,
        stroke_color: Color,
        stroke_width: f32,
    },
    /// An image placeholder (actual images would reference an asset store).
    Image {
        id: ElementId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        /// Placeholder label shown in place of the image.
        placeholder_label: String,
    },
    /// A list of bullet points.
    BulletList {
        id: ElementId,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        items: Vec<String>,
        font_size: f32,
        color: Color,
    },
}

impl SlideElement {
    /// Get the unique ID of this element.
    pub fn id(&self) -> ElementId {
        match self {
            Self::TextBox { id, .. }
            | Self::Shape { id, .. }
            | Self::Image { id, .. }
            | Self::BulletList { id, .. } => *id,
        }
    }

    /// Get the bounding rectangle (x, y, width, height) of this element.
    pub fn bounds(&self) -> (f32, f32, f32, f32) {
        match self {
            Self::TextBox { x, y, width, height, .. }
            | Self::Shape { x, y, width, height, .. }
            | Self::Image { x, y, width, height, .. }
            | Self::BulletList { x, y, width, height, .. } => (*x, *y, *width, *height),
        }
    }

    /// Move the element by a delta.
    pub fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Self::TextBox { x, y, .. }
            | Self::Shape { x, y, .. }
            | Self::Image { x, y, .. }
            | Self::BulletList { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
        }
    }

    /// Set the position of this element.
    pub fn set_position(&mut self, nx: f32, ny: f32) {
        match self {
            Self::TextBox { x, y, .. }
            | Self::Shape { x, y, .. }
            | Self::Image { x, y, .. }
            | Self::BulletList { x, y, .. } => {
                *x = nx;
                *y = ny;
            }
        }
    }

    /// Set the size of this element.
    pub fn set_size(&mut self, nw: f32, nh: f32) {
        match self {
            Self::TextBox { width, height, .. }
            | Self::Shape { width, height, .. }
            | Self::Image { width, height, .. }
            | Self::BulletList { width, height, .. } => {
                *width = nw;
                *height = nh;
            }
        }
    }
}

// ============================================================================
// Slide theme (master slide)
// ============================================================================

/// A presentation theme controlling colors and typography across all slides.
#[derive(Clone, Debug)]
pub struct SlideTheme {
    /// Name of this theme.
    pub name: String,
    /// Default background color for slides.
    pub background: Color,
    /// Title text color.
    pub title_color: Color,
    /// Subtitle text color.
    pub subtitle_color: Color,
    /// Body/content text color.
    pub body_color: Color,
    /// Accent color (for shapes, highlights).
    pub accent: Color,
    /// Title font size.
    pub title_size: f32,
    /// Subtitle font size.
    pub subtitle_size: f32,
    /// Body text font size.
    pub body_size: f32,
    /// Bullet text font size.
    pub bullet_size: f32,
}

impl SlideTheme {
    /// The default "Mocha" dark theme.
    pub fn mocha() -> Self {
        Self {
            name: String::from("Mocha"),
            background: CRUST,
            title_color: TEXT,
            subtitle_color: SUBTEXT1,
            body_color: SUBTEXT0,
            accent: BLUE,
            title_size: 44.0,
            subtitle_size: 28.0,
            body_size: 20.0,
            bullet_size: 18.0,
        }
    }

    /// Light corporate theme.
    pub fn light() -> Self {
        Self {
            name: String::from("Light"),
            background: Color::rgb(245, 245, 250),
            title_color: Color::from_hex(0x1E1E2E),
            subtitle_color: Color::from_hex(0x45475A),
            body_color: Color::from_hex(0x585B70),
            accent: Color::from_hex(0x1E66F5),
            title_size: 44.0,
            subtitle_size: 28.0,
            body_size: 20.0,
            bullet_size: 18.0,
        }
    }

    /// Bold colorful theme with a teal accent.
    pub fn vibrant() -> Self {
        Self {
            name: String::from("Vibrant"),
            background: Color::from_hex(0x0D1B2A),
            title_color: Color::from_hex(0xE0FBFC),
            subtitle_color: Color::from_hex(0x98C1D9),
            body_color: Color::from_hex(0x98C1D9),
            accent: Color::from_hex(0x3D5A80),
            title_size: 48.0,
            subtitle_size: 30.0,
            body_size: 22.0,
            bullet_size: 20.0,
        }
    }
}

// ============================================================================
// Slide
// ============================================================================

/// A single slide in a presentation.
#[derive(Clone, Debug)]
pub struct Slide {
    /// Unique identifier.
    pub id: SlideId,
    /// Layout template this slide was created from.
    pub layout: SlideLayout,
    /// Override background (if `None`, use theme default).
    pub background: Option<Color>,
    /// Title text (may be empty).
    pub title: String,
    /// Subtitle text (may be empty).
    pub subtitle: String,
    /// Free-form elements placed on this slide.
    pub elements: Vec<SlideElement>,
    /// Transition to play when entering this slide.
    pub transition: Transition,
    /// Speaker notes for this slide.
    pub notes: String,
}

impl Slide {
    /// Create a new slide with the given layout and theme-based defaults.
    pub fn new(id: SlideId, layout: SlideLayout, theme: &SlideTheme, id_gen: &mut IdGen) -> Self {
        let mut slide = Self {
            id,
            layout,
            background: None,
            title: String::new(),
            subtitle: String::new(),
            elements: Vec::new(),
            transition: Transition::None,
            notes: String::new(),
        };
        slide.apply_layout(theme, id_gen);
        slide
    }

    /// Populate the elements vector based on the selected layout template.
    fn apply_layout(&mut self, theme: &SlideTheme, id_gen: &mut IdGen) {
        self.elements.clear();
        match self.layout {
            SlideLayout::TitleSlide => {
                self.elements.push(SlideElement::TextBox {
                    id: id_gen.next_id(),
                    x: 80.0,
                    y: 160.0,
                    width: 800.0,
                    height: 80.0,
                    text: String::from("Presentation Title"),
                    font_size: theme.title_size,
                    color: theme.title_color,
                    bold: true,
                    centered: true,
                });
                self.elements.push(SlideElement::TextBox {
                    id: id_gen.next_id(),
                    x: 160.0,
                    y: 260.0,
                    width: 640.0,
                    height: 50.0,
                    text: String::from("Subtitle goes here"),
                    font_size: theme.subtitle_size,
                    color: theme.subtitle_color,
                    bold: false,
                    centered: true,
                });
                self.elements.push(SlideElement::Shape {
                    id: id_gen.next_id(),
                    kind: ShapeKind::Line,
                    x: 200.0,
                    y: 250.0,
                    width: 560.0,
                    height: 0.0,
                    fill_color: theme.accent,
                    stroke_color: theme.accent,
                    stroke_width: 2.0,
                });
            }
            SlideLayout::TitleContent => {
                self.elements.push(SlideElement::TextBox {
                    id: id_gen.next_id(),
                    x: 40.0,
                    y: 20.0,
                    width: 880.0,
                    height: 60.0,
                    text: String::from("Slide Title"),
                    font_size: theme.title_size,
                    color: theme.title_color,
                    bold: true,
                    centered: false,
                });
                self.elements.push(SlideElement::BulletList {
                    id: id_gen.next_id(),
                    x: 60.0,
                    y: 100.0,
                    width: 840.0,
                    height: 380.0,
                    items: vec![
                        String::from("First point"),
                        String::from("Second point"),
                        String::from("Third point"),
                    ],
                    font_size: theme.bullet_size,
                    color: theme.body_color,
                });
            }
            SlideLayout::SectionHeader => {
                self.elements.push(SlideElement::TextBox {
                    id: id_gen.next_id(),
                    x: 80.0,
                    y: 200.0,
                    width: 800.0,
                    height: 80.0,
                    text: String::from("Section Title"),
                    font_size: theme.title_size.max(48.0),
                    color: theme.title_color,
                    bold: true,
                    centered: true,
                });
                self.elements.push(SlideElement::Shape {
                    id: id_gen.next_id(),
                    kind: ShapeKind::Rectangle,
                    x: 0.0,
                    y: 480.0,
                    width: SLIDE_W,
                    height: 60.0,
                    fill_color: theme.accent,
                    stroke_color: theme.accent,
                    stroke_width: 0.0,
                });
            }
            SlideLayout::Blank => {
                // No predefined elements.
            }
            SlideLayout::TwoColumn => {
                self.elements.push(SlideElement::TextBox {
                    id: id_gen.next_id(),
                    x: 40.0,
                    y: 20.0,
                    width: 880.0,
                    height: 60.0,
                    text: String::from("Two Column Layout"),
                    font_size: theme.title_size,
                    color: theme.title_color,
                    bold: true,
                    centered: false,
                });
                self.elements.push(SlideElement::BulletList {
                    id: id_gen.next_id(),
                    x: 40.0,
                    y: 100.0,
                    width: 420.0,
                    height: 380.0,
                    items: vec![
                        String::from("Left column point A"),
                        String::from("Left column point B"),
                    ],
                    font_size: theme.bullet_size,
                    color: theme.body_color,
                });
                self.elements.push(SlideElement::BulletList {
                    id: id_gen.next_id(),
                    x: 500.0,
                    y: 100.0,
                    width: 420.0,
                    height: 380.0,
                    items: vec![
                        String::from("Right column point A"),
                        String::from("Right column point B"),
                    ],
                    font_size: theme.bullet_size,
                    color: theme.body_color,
                });
            }
            SlideLayout::ImageCaption => {
                self.elements.push(SlideElement::Image {
                    id: id_gen.next_id(),
                    x: 40.0,
                    y: 40.0,
                    width: 500.0,
                    height: 420.0,
                    placeholder_label: String::from("Image Placeholder"),
                });
                self.elements.push(SlideElement::TextBox {
                    id: id_gen.next_id(),
                    x: 570.0,
                    y: 80.0,
                    width: 350.0,
                    height: 340.0,
                    text: String::from("Caption and description text goes here."),
                    font_size: theme.body_size,
                    color: theme.body_color,
                    bold: false,
                    centered: false,
                });
            }
        }
    }

    /// Get the effective background color (slide override or theme default).
    pub fn effective_bg(&self, theme: &SlideTheme) -> Color {
        self.background.unwrap_or(theme.background)
    }

    /// Find an element by ID.
    pub fn element_by_id(&self, eid: ElementId) -> Option<&SlideElement> {
        self.elements.iter().find(|e| e.id() == eid)
    }

    /// Find a mutable element by ID.
    pub fn element_by_id_mut(&mut self, eid: ElementId) -> Option<&mut SlideElement> {
        self.elements.iter_mut().find(|e| e.id() == eid)
    }

    /// Remove an element by ID, returning it if found.
    pub fn remove_element(&mut self, eid: ElementId) -> Option<SlideElement> {
        if let Some(pos) = self.elements.iter().position(|e| e.id() == eid) {
            Some(self.elements.remove(pos))
        } else {
            None
        }
    }
}

// ============================================================================
// Undo/Redo
// ============================================================================

/// A snapshot of the entire slide deck for undo/redo.
#[derive(Clone, Debug)]
struct Snapshot {
    slides: Vec<Slide>,
    current_index: usize,
}

/// Undo/redo manager using a snapshot stack.
#[derive(Debug)]
struct UndoManager {
    undo_stack: VecDeque<Snapshot>,
    redo_stack: Vec<Snapshot>,
    max_depth: usize,
}

impl UndoManager {
    fn new(max_depth: usize) -> Self {
        Self {
            undo_stack: VecDeque::new(),
            redo_stack: Vec::new(),
            max_depth,
        }
    }

    /// Save the current state before a mutation. Clears the redo stack.
    fn save(&mut self, slides: &[Slide], current_index: usize) {
        if self.undo_stack.len() >= self.max_depth {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(Snapshot {
            slides: slides.to_vec(),
            current_index,
        });
        self.redo_stack.clear();
    }

    /// Undo: return the previous snapshot, saving the current state to redo.
    fn undo(&mut self, current_slides: &[Slide], current_index: usize) -> Option<Snapshot> {
        let prev = self.undo_stack.pop_back()?;
        self.redo_stack.push(Snapshot {
            slides: current_slides.to_vec(),
            current_index,
        });
        Some(prev)
    }

    /// Redo: return the next snapshot, saving the current state to undo.
    fn redo(&mut self, current_slides: &[Slide], current_index: usize) -> Option<Snapshot> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push_back(Snapshot {
            slides: current_slides.to_vec(),
            current_index,
        });
        Some(next)
    }

    /// True if there is something to undo.
    fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    /// True if there is something to redo.
    fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}

// ============================================================================
// View mode
// ============================================================================

/// The current view of the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    /// Normal editing view with sidebar, canvas, and properties panel.
    Edit,
    /// Slide sorter: grid of thumbnails for reordering.
    Sorter,
}

// ============================================================================
// Clipboard
// ============================================================================

/// Clipboard for slide copy/paste.
#[derive(Clone, Debug)]
enum Clipboard {
    Empty,
    Slide(Slide),
}

// ============================================================================
// Slides application
// ============================================================================

/// The main presentation application state.
#[derive(Debug)]
pub struct SlidesApp {
    /// All slides in the presentation.
    slides: Vec<Slide>,
    /// The currently selected/displayed slide index.
    current_index: usize,
    /// The presentation theme (master slide styles).
    theme: SlideTheme,
    /// Unique ID generator for slides and elements.
    id_gen: IdGen,
    /// Total window width.
    window_width: f32,
    /// Total window height.
    window_height: f32,
    /// Current view mode.
    view: ViewMode,
    /// Undo/redo manager.
    undo_mgr: UndoManager,
    /// Clipboard (for slide copy/paste).
    clipboard: Clipboard,
    /// Currently selected element ID on the active slide (if any).
    selected_element: Option<ElementId>,
    /// Whether the notes panel is visible.
    show_notes: bool,
    /// Title of the presentation.
    title: String,
}

impl SlidesApp {
    /// Create a new presentation with one default title slide.
    pub fn new(width: f32, height: f32) -> Self {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(1);
        let slide_id = id_gen.next_id();
        let first = Slide::new(slide_id, SlideLayout::TitleSlide, &theme, &mut id_gen);

        Self {
            slides: vec![first],
            current_index: 0,
            theme,
            id_gen,
            window_width: width,
            window_height: height,
            view: ViewMode::Edit,
            undo_mgr: UndoManager::new(MAX_UNDO),
            clipboard: Clipboard::Empty,
            selected_element: None,
            show_notes: true,
            title: String::from("Untitled Presentation"),
        }
    }

    // ---- Slide management --------------------------------------------------

    /// Number of slides in the deck.
    pub fn slide_count(&self) -> usize {
        self.slides.len()
    }

    /// Get the current slide index.
    pub fn current_index(&self) -> usize {
        self.current_index
    }

    /// Navigate to a specific slide by index.
    pub fn go_to_slide(&mut self, index: usize) {
        if index < self.slides.len() {
            self.current_index = index;
            self.selected_element = None;
        }
    }

    /// Navigate to the next slide.
    pub fn next_slide(&mut self) {
        if self.current_index.saturating_add(1) < self.slides.len() {
            self.current_index = self.current_index.saturating_add(1);
            self.selected_element = None;
        }
    }

    /// Navigate to the previous slide.
    pub fn prev_slide(&mut self) {
        if self.current_index > 0 {
            self.current_index = self.current_index.saturating_sub(1);
            self.selected_element = None;
        }
    }

    /// Insert a new slide with the given layout after the current position.
    pub fn add_slide(&mut self, layout: SlideLayout) {
        self.undo_mgr.save(&self.slides, self.current_index);
        let slide_id = self.id_gen.next_id();
        let slide = Slide::new(slide_id, layout, &self.theme, &mut self.id_gen);
        let insert_at = self.current_index.saturating_add(1).min(self.slides.len());
        self.slides.insert(insert_at, slide);
        self.current_index = insert_at;
        self.selected_element = None;
    }

    /// Delete the slide at the given index (will not delete the last remaining slide).
    pub fn delete_slide(&mut self, index: usize) {
        if self.slides.len() <= 1 || index >= self.slides.len() {
            return;
        }
        self.undo_mgr.save(&self.slides, self.current_index);
        self.slides.remove(index);
        if self.current_index >= self.slides.len() {
            self.current_index = self.slides.len().saturating_sub(1);
        }
        self.selected_element = None;
    }

    /// Duplicate the current slide, inserting the copy immediately after it.
    pub fn duplicate_current_slide(&mut self) {
        if let Some(slide) = self.slides.get(self.current_index).cloned() {
            self.undo_mgr.save(&self.slides, self.current_index);
            let mut dup = slide;
            dup.id = self.id_gen.next_id();
            // Give duplicated elements new IDs.
            for elem in &mut dup.elements {
                match elem {
                    SlideElement::TextBox { id, .. }
                    | SlideElement::Shape { id, .. }
                    | SlideElement::Image { id, .. }
                    | SlideElement::BulletList { id, .. } => {
                        *id = self.id_gen.next_id();
                    }
                }
            }
            let pos = self.current_index.saturating_add(1).min(self.slides.len());
            self.slides.insert(pos, dup);
            self.current_index = pos;
        }
    }

    /// Move the current slide up (towards index 0).
    pub fn move_slide_up(&mut self) {
        if self.current_index == 0 {
            return;
        }
        self.undo_mgr.save(&self.slides, self.current_index);
        self.slides.swap(self.current_index, self.current_index.saturating_sub(1));
        self.current_index = self.current_index.saturating_sub(1);
    }

    /// Move the current slide down (towards the last index).
    pub fn move_slide_down(&mut self) {
        if self.current_index.saturating_add(1) >= self.slides.len() {
            return;
        }
        self.undo_mgr.save(&self.slides, self.current_index);
        let next = self.current_index.saturating_add(1);
        self.slides.swap(self.current_index, next);
        self.current_index = next;
    }

    /// Copy the current slide to the clipboard.
    pub fn copy_slide(&mut self) {
        if let Some(slide) = self.slides.get(self.current_index) {
            self.clipboard = Clipboard::Slide(slide.clone());
        }
    }

    /// Paste the slide from the clipboard after the current position.
    pub fn paste_slide(&mut self) {
        if let Clipboard::Slide(slide) = &self.clipboard {
            self.undo_mgr.save(&self.slides, self.current_index);
            let mut pasted = slide.clone();
            pasted.id = self.id_gen.next_id();
            for elem in &mut pasted.elements {
                match elem {
                    SlideElement::TextBox { id, .. }
                    | SlideElement::Shape { id, .. }
                    | SlideElement::Image { id, .. }
                    | SlideElement::BulletList { id, .. } => {
                        *id = self.id_gen.next_id();
                    }
                }
            }
            let pos = self.current_index.saturating_add(1).min(self.slides.len());
            self.slides.insert(pos, pasted);
            self.current_index = pos;
        }
    }

    // ---- Element operations ------------------------------------------------

    /// Add a text box element to the current slide.
    pub fn add_textbox(&mut self) {
        self.undo_mgr.save(&self.slides, self.current_index);
        if let Some(slide) = self.slides.get_mut(self.current_index) {
            let eid = self.id_gen.next_id();
            slide.elements.push(SlideElement::TextBox {
                id: eid,
                x: 100.0,
                y: 200.0,
                width: 300.0,
                height: 60.0,
                text: String::from("New Text"),
                font_size: self.theme.body_size,
                color: self.theme.body_color,
                bold: false,
                centered: false,
            });
            self.selected_element = Some(eid);
        }
    }

    /// Add a shape element to the current slide.
    pub fn add_shape(&mut self, kind: ShapeKind) {
        self.undo_mgr.save(&self.slides, self.current_index);
        if let Some(slide) = self.slides.get_mut(self.current_index) {
            let eid = self.id_gen.next_id();
            slide.elements.push(SlideElement::Shape {
                id: eid,
                kind,
                x: 200.0,
                y: 200.0,
                width: 200.0,
                height: 120.0,
                fill_color: self.theme.accent,
                stroke_color: self.theme.accent,
                stroke_width: 2.0,
            });
            self.selected_element = Some(eid);
        }
    }

    /// Add an image placeholder to the current slide.
    pub fn add_image_placeholder(&mut self) {
        self.undo_mgr.save(&self.slides, self.current_index);
        if let Some(slide) = self.slides.get_mut(self.current_index) {
            let eid = self.id_gen.next_id();
            slide.elements.push(SlideElement::Image {
                id: eid,
                x: 160.0,
                y: 120.0,
                width: 320.0,
                height: 240.0,
                placeholder_label: String::from("Image"),
            });
            self.selected_element = Some(eid);
        }
    }

    /// Delete the currently selected element.
    pub fn delete_selected_element(&mut self) {
        if let Some(eid) = self.selected_element {
            self.undo_mgr.save(&self.slides, self.current_index);
            if let Some(slide) = self.slides.get_mut(self.current_index) {
                slide.remove_element(eid);
            }
            self.selected_element = None;
        }
    }

    // ---- Undo/Redo ---------------------------------------------------------

    /// Undo the last action.
    pub fn undo(&mut self) {
        if let Some(snap) = self.undo_mgr.undo(&self.slides, self.current_index) {
            self.slides = snap.slides;
            self.current_index = snap.current_index.min(self.slides.len().saturating_sub(1));
            self.selected_element = None;
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self) {
        if let Some(snap) = self.undo_mgr.redo(&self.slides, self.current_index) {
            self.slides = snap.slides;
            self.current_index = snap.current_index.min(self.slides.len().saturating_sub(1));
            self.selected_element = None;
        }
    }

    // ---- Theme -------------------------------------------------------------

    /// Set the presentation theme and re-apply it to all slides.
    pub fn set_theme(&mut self, theme: SlideTheme) {
        self.undo_mgr.save(&self.slides, self.current_index);
        self.theme = theme;
    }

    // ---- Transition --------------------------------------------------------

    /// Set the transition for the current slide.
    pub fn set_current_transition(&mut self, transition: Transition) {
        self.undo_mgr.save(&self.slides, self.current_index);
        if let Some(slide) = self.slides.get_mut(self.current_index) {
            slide.transition = transition;
        }
    }

    // ---- Notes -------------------------------------------------------------

    /// Set speaker notes for the current slide.
    pub fn set_current_notes(&mut self, notes: String) {
        if let Some(slide) = self.slides.get_mut(self.current_index) {
            slide.notes = notes;
        }
    }

    /// Get the current slide's speaker notes.
    pub fn current_notes(&self) -> &str {
        self.slides
            .get(self.current_index)
            .map_or("", |s| s.notes.as_str())
    }

    // ---- Export to HTML ----------------------------------------------------

    /// Export the entire presentation to a self-contained HTML file.
    pub fn export_html(&self) -> String {
        let mut html = String::with_capacity(4096);
        html.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
        html.push_str("<meta charset=\"UTF-8\">\n");
        html.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n");
        html.push_str("<title>");
        push_html_escaped(&mut html, &self.title);
        html.push_str("</title>\n");
        html.push_str("<style>\n");
        html.push_str(concat!(
            "* { margin: 0; padding: 0; box-sizing: border-box; }\n",
            "body { background: #11111B; display: flex; justify-content: center; ",
            "align-items: center; min-height: 100vh; font-family: sans-serif; }\n",
            ".slide { position: relative; width: 960px; height: 540px; display: none; overflow: hidden; }\n",
            ".slide.active { display: block; }\n",
            ".textbox { position: absolute; white-space: pre-wrap; }\n",
            ".shape { position: absolute; }\n",
            ".bullet-list { position: absolute; }\n",
            ".bullet-list li { margin-bottom: 4px; }\n",
            ".img-placeholder { position: absolute; display: flex; align-items: center; ",
            "justify-content: center; border: 2px dashed #6C7086; color: #A6ADC8; font-size: 14px; }\n",
            ".slide-num { position: absolute; bottom: 10px; right: 20px; font-size: 12px; color: #6C7086; }\n",
            ".controls { position: fixed; bottom: 20px; left: 50%; transform: translateX(-50%); ",
            "display: flex; gap: 10px; z-index: 100; }\n",
            ".controls button { padding: 8px 16px; background: #313244; color: #CDD6F4; ",
            "border: 1px solid #45475A; border-radius: 4px; cursor: pointer; font-size: 14px; }\n",
            ".controls button:hover { background: #45475A; }\n",
        ));
        html.push_str("</style>\n</head>\n<body>\n");

        // Emit each slide as a div.
        for (i, slide) in self.slides.iter().enumerate() {
            let bg = slide.effective_bg(&self.theme);
            let active = if i == 0 { " active" } else { "" };
            html.push_str(&format!(
                "<div class=\"slide{}\" id=\"slide-{}\" style=\"background:{}\">\n",
                active,
                i,
                color_to_css(bg),
            ));

            // Slide number.
            html.push_str(&format!(
                "  <div class=\"slide-num\">{} / {}</div>\n",
                i.saturating_add(1),
                self.slides.len(),
            ));

            // Emit elements.
            for elem in &slide.elements {
                match elem {
                    SlideElement::TextBox { x, y, width, height, text, font_size, color, bold, centered, .. } => {
                        let fw = if *bold { "bold" } else { "normal" };
                        let ta = if *centered { "center" } else { "left" };
                        html.push_str(&format!(
                            "  <div class=\"textbox\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                             height:{height}px;font-size:{font_size}px;color:{};font-weight:{fw};\
                             text-align:{ta};\">",
                            color_to_css(*color),
                        ));
                        push_html_escaped(&mut html, text);
                        html.push_str("</div>\n");
                    }
                    SlideElement::Shape { kind, x, y, width, height, fill_color, stroke_color, stroke_width, .. } => {
                        match kind {
                            ShapeKind::Rectangle => {
                                html.push_str(&format!(
                                    "  <div class=\"shape\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                                     height:{height}px;background:{};border:{stroke_width}px solid {};\"></div>\n",
                                    color_to_css(*fill_color),
                                    color_to_css(*stroke_color),
                                ));
                            }
                            ShapeKind::Ellipse => {
                                html.push_str(&format!(
                                    "  <div class=\"shape\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                                     height:{height}px;background:{};border:{stroke_width}px solid {};\
                                     border-radius:50%;\"></div>\n",
                                    color_to_css(*fill_color),
                                    color_to_css(*stroke_color),
                                ));
                            }
                            ShapeKind::Line | ShapeKind::Arrow => {
                                // Render as a thin div (line) — simplified.
                                html.push_str(&format!(
                                    "  <div class=\"shape\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                                     height:2px;background:{};\"></div>\n",
                                    color_to_css(*stroke_color),
                                ));
                            }
                        }
                    }
                    SlideElement::Image { x, y, width, height, placeholder_label, .. } => {
                        html.push_str(&format!(
                            "  <div class=\"img-placeholder\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                             height:{height}px;\">{}</div>\n",
                            placeholder_label,
                        ));
                    }
                    SlideElement::BulletList { x, y, width, height, items, font_size, color, .. } => {
                        html.push_str(&format!(
                            "  <ul class=\"bullet-list\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                             height:{height}px;font-size:{font_size}px;color:{};list-style:disc inside;\">\n",
                            color_to_css(*color),
                        ));
                        for item in items {
                            html.push_str("    <li>");
                            push_html_escaped(&mut html, item);
                            html.push_str("</li>\n");
                        }
                        html.push_str("  </ul>\n");
                    }
                }
            }

            html.push_str("</div>\n");
        }

        // Navigation controls.
        html.push_str(concat!(
            "<div class=\"controls\">\n",
            "  <button onclick=\"prevSlide()\">&#9664; Prev</button>\n",
            "  <button onclick=\"nextSlide()\">Next &#9654;</button>\n",
            "</div>\n",
        ));

        // Tiny JS for slide navigation.
        html.push_str("<script>\n");
        html.push_str("let cur=0,total=document.querySelectorAll('.slide').length;\n");
        html.push_str("function show(n){document.querySelectorAll('.slide').forEach((s,i)=>");
        html.push_str("s.classList.toggle('active',i===n));cur=n;}\n");
        html.push_str("function nextSlide(){if(cur<total-1)show(cur+1);}\n");
        html.push_str("function prevSlide(){if(cur>0)show(cur-1);}\n");
        html.push_str("document.addEventListener('keydown',e=>{");
        html.push_str("if(e.key==='ArrowRight'||e.key===' ')nextSlide();");
        html.push_str("if(e.key==='ArrowLeft')prevSlide();});\n");
        html.push_str("</script>\n");

        html.push_str("</body>\n</html>\n");
        html
    }

    // ========================================================================
    // Rendering
    // ========================================================================

    /// Render the full application UI and return the list of draw commands.
    pub fn render(&self) -> Vec<RenderCommand> {
        let mut cmds: Vec<RenderCommand> = Vec::with_capacity(256);

        // Background fill the entire window.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: self.window_height,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        match self.view {
            ViewMode::Edit => self.render_edit_mode(&mut cmds),
            ViewMode::Sorter => self.render_sorter_mode(&mut cmds),
        }

        cmds
    }

    /// Render the toolbar at the top.
    fn render_toolbar(&self, cmds: &mut Vec<RenderCommand>) {
        // Toolbar background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.window_width,
            height: TOOLBAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator line.
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: SURFACE0,
            width: 1.0,
        });

        // Presentation title.
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: self.title.clone(),
            color: TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(200.0),
        });

        // Toolbar buttons.
        let buttons: &[(&str, f32)] = &[
            ("+ Slide", 230.0),
            ("Duplicate", 310.0),
            ("Delete", 400.0),
            ("Sorter", 470.0),
            ("Export", 540.0),
        ];
        for &(label, bx) in buttons {
            self.render_button(cmds, bx, 6.0, 70.0, 28.0, label, SURFACE0, TEXT);
        }

        // Undo/Redo indicators.
        let undo_col = if self.undo_mgr.can_undo() { BLUE } else { OVERLAY0 };
        let redo_col = if self.undo_mgr.can_redo() { BLUE } else { OVERLAY0 };
        cmds.push(RenderCommand::Text {
            x: 630.0,
            y: 12.0,
            text: String::from("Undo"),
            color: undo_col,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
        cmds.push(RenderCommand::Text {
            x: 680.0,
            y: 12.0,
            text: String::from("Redo"),
            color: redo_col,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        // Theme label.
        cmds.push(RenderCommand::Text {
            x: 740.0,
            y: 12.0,
            text: format!("Theme: {}", self.theme.name),
            color: SUBTEXT0,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    /// Render a simple button (rounded rect + label).
    fn render_button(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        label: &str,
        bg: Color,
        fg: Color,
    ) {
        cmds.push(RenderCommand::FillRect {
            x,
            y,
            width: w,
            height: h,
            color: bg,
            corner_radii: CornerRadii::all(CORNER_R),
        });
        cmds.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 8.0,
            text: label.to_string(),
            color: fg,
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(w - 16.0),
        });
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, cmds: &mut Vec<RenderCommand>) {
        let y = self.window_height - STATUS_BAR_HEIGHT;
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y,
            width: self.window_width,
            height: STATUS_BAR_HEIGHT,
            color: CRUST,
            corner_radii: CornerRadii::ZERO,
        });
        cmds.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: SURFACE0,
            width: 1.0,
        });

        // Slide position and transition info.
        let slide_pos = format!(
            "Slide {} of {}",
            self.current_index.saturating_add(1),
            self.slides.len(),
        );
        cmds.push(RenderCommand::Text {
            x: 12.0,
            y: y + 5.0,
            text: slide_pos,
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });

        if let Some(slide) = self.slides.get(self.current_index) {
            let trans = format!("Transition: {}", slide.transition.label());
            cmds.push(RenderCommand::Text {
                x: 200.0,
                y: y + 5.0,
                text: trans,
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });

            let layout_info = format!("Layout: {}", slide.layout.label());
            cmds.push(RenderCommand::Text {
                x: 400.0,
                y: y + 5.0,
                text: layout_info,
                color: SUBTEXT0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        // View mode indicator.
        let view_label = match self.view {
            ViewMode::Edit => "Edit Mode",
            ViewMode::Sorter => "Sorter View",
        };
        cmds.push(RenderCommand::Text {
            x: self.window_width - 120.0,
            y: y + 5.0,
            text: view_label.to_string(),
            color: TEAL,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
        });
    }

    // ---- Edit mode rendering -----------------------------------------------

    /// Render the full edit mode: toolbar, sidebar, canvas, properties, notes, status.
    fn render_edit_mode(&self, cmds: &mut Vec<RenderCommand>) {
        self.render_toolbar(cmds);
        self.render_sidebar(cmds);
        self.render_canvas(cmds);
        self.render_properties_panel(cmds);
        if self.show_notes {
            self.render_notes_panel(cmds);
        }
        self.render_status_bar(cmds);
    }

    /// Render the slide thumbnail sidebar.
    fn render_sidebar(&self, cmds: &mut Vec<RenderCommand>) {
        let top = TOOLBAR_HEIGHT;
        let bot = self.window_height - STATUS_BAR_HEIGHT;
        let panel_h = bot - top;

        // Sidebar background.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: top,
            width: SIDEBAR_WIDTH,
            height: panel_h,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });

        // Separator.
        cmds.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: top,
            x2: SIDEBAR_WIDTH,
            y2: bot,
            color: SURFACE0,
            width: 1.0,
        });

        // Clip to sidebar region.
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: top,
            width: SIDEBAR_WIDTH,
            height: panel_h,
        });

        // Render each slide thumbnail.
        let thumb_w = SIDEBAR_WIDTH - THUMBNAIL_PAD * 2.0;
        let thumb_h = thumb_w / SLIDE_ASPECT;
        for (i, slide) in self.slides.iter().enumerate() {
            let ty = top + THUMBNAIL_PAD + (i as f32) * (thumb_h + THUMBNAIL_PAD + 20.0);
            let is_current = i == self.current_index;

            // Selection highlight.
            if is_current {
                cmds.push(RenderCommand::StrokeRect {
                    x: THUMBNAIL_PAD - 2.0,
                    y: ty - 2.0,
                    width: thumb_w + 4.0,
                    height: thumb_h + 4.0,
                    color: BLUE,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
            }

            // Thumbnail background.
            let bg = slide.effective_bg(&self.theme);
            cmds.push(RenderCommand::FillRect {
                x: THUMBNAIL_PAD,
                y: ty,
                width: thumb_w,
                height: thumb_h,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_R),
            });

            // Mini title preview.
            let preview = slide_preview_text(slide);
            if !preview.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: THUMBNAIL_PAD + 4.0,
                    y: ty + 8.0,
                    text: preview,
                    color: TEXT,
                    font_size: 8.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(thumb_w - 8.0),
                });
            }

            // Slide number label.
            cmds.push(RenderCommand::Text {
                x: THUMBNAIL_PAD,
                y: ty + thumb_h + 2.0,
                text: format!("Slide {}", i.saturating_add(1)),
                color: if is_current { BLUE } else { OVERLAY0 },
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }

        cmds.push(RenderCommand::PopClip);
    }

    /// Render the main slide canvas in the center.
    fn render_canvas(&self, cmds: &mut Vec<RenderCommand>) {
        let top = TOOLBAR_HEIGHT;
        let notes_h = if self.show_notes { NOTES_HEIGHT } else { 0.0 };
        let avail_w = self.window_width - SIDEBAR_WIDTH - PROPERTIES_WIDTH;
        let avail_h = self.window_height - top - STATUS_BAR_HEIGHT - notes_h;

        // Compute the slide display size to fit the available area while
        // maintaining the 16:9 aspect ratio.
        let scale_w = (avail_w - 40.0) / SLIDE_W;
        let scale_h = (avail_h - 40.0) / SLIDE_H;
        let scale = scale_w.min(scale_h).max(0.1);
        let disp_w = SLIDE_W * scale;
        let disp_h = SLIDE_H * scale;
        let cx = SIDEBAR_WIDTH + (avail_w - disp_w) / 2.0;
        let cy = top + (avail_h - disp_h) / 2.0;

        // Canvas background area (dark behind the slide).
        cmds.push(RenderCommand::FillRect {
            x: SIDEBAR_WIDTH,
            y: top,
            width: avail_w,
            height: avail_h,
            color: MANTLE,
            corner_radii: CornerRadii::ZERO,
        });

        // Drop shadow behind the slide.
        cmds.push(RenderCommand::BoxShadow {
            x: cx,
            y: cy,
            width: disp_w,
            height: disp_h,
            offset_x: 3.0,
            offset_y: 3.0,
            blur: 12.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(CORNER_R),
        });

        if let Some(slide) = self.slides.get(self.current_index) {
            let bg = slide.effective_bg(&self.theme);

            // Slide rectangle.
            cmds.push(RenderCommand::FillRect {
                x: cx,
                y: cy,
                width: disp_w,
                height: disp_h,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_R),
            });

            // Clip to the slide area.
            cmds.push(RenderCommand::PushClip {
                x: cx,
                y: cy,
                width: disp_w,
                height: disp_h,
            });

            // Render each element, scaled.
            for elem in &slide.elements {
                self.render_element(cmds, elem, cx, cy, scale);
            }

            cmds.push(RenderCommand::PopClip);

            // Selection highlight around selected element.
            if let Some(eid) = self.selected_element {
                if let Some(elem) = slide.element_by_id(eid) {
                    let (ex, ey, ew, eh) = elem.bounds();
                    cmds.push(RenderCommand::StrokeRect {
                        x: cx + ex * scale - 1.0,
                        y: cy + ey * scale - 1.0,
                        width: ew * scale + 2.0,
                        height: eh * scale + 2.0,
                        color: SKY,
                        line_width: 1.5,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }

            // Slide number overlay.
            cmds.push(RenderCommand::Text {
                x: cx + disp_w - 50.0,
                y: cy + disp_h - 18.0,
                text: format!(
                    "{}/{}",
                    self.current_index.saturating_add(1),
                    self.slides.len(),
                ),
                color: OVERLAY0,
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
            });
        }
    }

    /// Render a single slide element at the given offset and scale.
    fn render_element(
        &self,
        cmds: &mut Vec<RenderCommand>,
        elem: &SlideElement,
        ox: f32,
        oy: f32,
        scale: f32,
    ) {
        match elem {
            SlideElement::TextBox { x, y, width, text, font_size, color, bold, centered, .. } => {
                let fx = ox + x * scale;
                let fy = oy + y * scale;
                let fw = width * scale;
                let fs = font_size * scale;
                let weight = if *bold { FontWeightHint::Bold } else { FontWeightHint::Regular };

                // For centered text, add a half-width offset (simplified).
                let text_x = if *centered { fx + fw * 0.1 } else { fx };
                let text_max = if *centered { Some(fw * 0.8) } else { Some(fw) };

                cmds.push(RenderCommand::Text {
                    x: text_x,
                    y: fy,
                    text: text.clone(),
                    color: *color,
                    font_size: fs,
                    font_weight: weight,
                    max_width: text_max,
                });
            }
            SlideElement::Shape { kind, x, y, width, height, fill_color, stroke_color, stroke_width, .. } => {
                let sx = ox + x * scale;
                let sy = oy + y * scale;
                let sw = width * scale;
                let sh = height * scale;
                let lw = stroke_width * scale;

                match kind {
                    ShapeKind::Rectangle => {
                        cmds.push(RenderCommand::FillRect {
                            x: sx,
                            y: sy,
                            width: sw,
                            height: sh,
                            color: *fill_color,
                            corner_radii: CornerRadii::ZERO,
                        });
                        if lw > 0.0 {
                            cmds.push(RenderCommand::StrokeRect {
                                x: sx,
                                y: sy,
                                width: sw,
                                height: sh,
                                color: *stroke_color,
                                line_width: lw,
                                corner_radii: CornerRadii::ZERO,
                            });
                        }
                    }
                    ShapeKind::Ellipse => {
                        // Approximate ellipse with a heavily rounded rect.
                        let r = sw.min(sh) / 2.0;
                        cmds.push(RenderCommand::FillRect {
                            x: sx,
                            y: sy,
                            width: sw,
                            height: sh,
                            color: *fill_color,
                            corner_radii: CornerRadii::all(r),
                        });
                        if lw > 0.0 {
                            cmds.push(RenderCommand::StrokeRect {
                                x: sx,
                                y: sy,
                                width: sw,
                                height: sh,
                                color: *stroke_color,
                                line_width: lw,
                                corner_radii: CornerRadii::all(r),
                            });
                        }
                    }
                    ShapeKind::Line => {
                        cmds.push(RenderCommand::Line {
                            x1: sx,
                            y1: sy,
                            x2: sx + sw,
                            y2: sy + sh,
                            color: *stroke_color,
                            width: lw.max(1.0),
                        });
                    }
                    ShapeKind::Arrow => {
                        // Line body.
                        cmds.push(RenderCommand::Line {
                            x1: sx,
                            y1: sy,
                            x2: sx + sw,
                            y2: sy + sh,
                            color: *stroke_color,
                            width: lw.max(1.0),
                        });
                        // Simple arrowhead (two short lines).
                        let head_len = 10.0 * scale;
                        let ex = sx + sw;
                        let ey = sy + sh;
                        cmds.push(RenderCommand::Line {
                            x1: ex,
                            y1: ey,
                            x2: ex - head_len,
                            y2: ey - head_len,
                            color: *stroke_color,
                            width: lw.max(1.0),
                        });
                        cmds.push(RenderCommand::Line {
                            x1: ex,
                            y1: ey,
                            x2: ex - head_len,
                            y2: ey + head_len * 0.5,
                            color: *stroke_color,
                            width: lw.max(1.0),
                        });
                    }
                }
            }
            SlideElement::Image { x, y, width, height, placeholder_label, .. } => {
                let ix = ox + x * scale;
                let iy = oy + y * scale;
                let iw = width * scale;
                let ih = height * scale;

                // Dashed border placeholder.
                cmds.push(RenderCommand::StrokeRect {
                    x: ix,
                    y: iy,
                    width: iw,
                    height: ih,
                    color: OVERLAY0,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
                // Placeholder label.
                cmds.push(RenderCommand::Text {
                    x: ix + iw * 0.25,
                    y: iy + ih * 0.45,
                    text: placeholder_label.clone(),
                    color: SUBTEXT0,
                    font_size: 14.0 * scale,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(iw * 0.5),
                });
            }
            SlideElement::BulletList { x, y, width, items, font_size, color, .. } => {
                let bx = ox + x * scale;
                let by = oy + y * scale;
                let bw = width * scale;
                let fs = font_size * scale;
                let line_h = fs * 1.6;
                for (i, item) in items.iter().enumerate() {
                    let iy = by + (i as f32) * line_h;
                    let bullet_text = format!("\u{2022} {item}");
                    cmds.push(RenderCommand::Text {
                        x: bx,
                        y: iy,
                        text: bullet_text,
                        color: *color,
                        font_size: fs,
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(bw),
                    });
                }
            }
        }
    }

    /// Render the properties panel on the right side.
    fn render_properties_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let top = TOOLBAR_HEIGHT;
        let bot = self.window_height - STATUS_BAR_HEIGHT;
        let px = self.window_width - PROPERTIES_WIDTH;
        let ph = bot - top;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: top,
            width: PROPERTIES_WIDTH,
            height: ph,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });
        // Separator.
        cmds.push(RenderCommand::Line {
            x1: px,
            y1: top,
            x2: px,
            y2: bot,
            color: SURFACE0,
            width: 1.0,
        });

        let mut y = top + 12.0;
        let lx = px + 12.0;
        let val_w = PROPERTIES_WIDTH - 24.0;

        // Properties header.
        cmds.push(RenderCommand::Text {
            x: lx,
            y,
            text: String::from("Properties"),
            color: TEXT,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        y += 24.0;

        // Slide properties.
        if let Some(slide) = self.slides.get(self.current_index) {
            // Layout.
            self.render_property_row(cmds, lx, y, val_w, "Layout", slide.layout.label());
            y += 22.0;

            // Transition.
            self.render_property_row(cmds, lx, y, val_w, "Transition", slide.transition.label());
            y += 22.0;

            // Background.
            let bg = slide.effective_bg(&self.theme);
            let bg_hex = format!("#{:02X}{:02X}{:02X}", bg.r, bg.g, bg.b);
            self.render_property_row(cmds, lx, y, val_w, "Background", &bg_hex);
            y += 22.0;

            // Color swatch.
            cmds.push(RenderCommand::FillRect {
                x: lx + 80.0,
                y: y - 16.0,
                width: 40.0,
                height: 14.0,
                color: bg,
                corner_radii: CornerRadii::all(2.0),
            });
            y += 12.0;

            // Separator.
            cmds.push(RenderCommand::Line {
                x1: lx,
                y1: y,
                x2: px + PROPERTIES_WIDTH - 12.0,
                y2: y,
                color: SURFACE0,
                width: 1.0,
            });
            y += 12.0;

            // Selected element properties.
            if let Some(eid) = self.selected_element {
                if let Some(elem) = slide.element_by_id(eid) {
                    cmds.push(RenderCommand::Text {
                        x: lx,
                        y,
                        text: String::from("Element"),
                        color: BLUE,
                        font_size: 13.0,
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                    });
                    y += 20.0;

                    let (ex, ey, ew, eh) = elem.bounds();
                    self.render_property_row(cmds, lx, y, val_w, "X", &format!("{ex:.0}"));
                    y += 18.0;
                    self.render_property_row(cmds, lx, y, val_w, "Y", &format!("{ey:.0}"));
                    y += 18.0;
                    self.render_property_row(cmds, lx, y, val_w, "Width", &format!("{ew:.0}"));
                    y += 18.0;
                    self.render_property_row(cmds, lx, y, val_w, "Height", &format!("{eh:.0}"));
                    y += 18.0;

                    match elem {
                        SlideElement::TextBox { text, font_size, bold, centered, .. } => {
                            self.render_property_row(cmds, lx, y, val_w, "Type", "TextBox");
                            y += 18.0;
                            self.render_property_row(cmds, lx, y, val_w, "Size", &format!("{font_size:.0}"));
                            y += 18.0;
                            self.render_property_row(cmds, lx, y, val_w, "Bold", if *bold { "Yes" } else { "No" });
                            y += 18.0;
                            self.render_property_row(cmds, lx, y, val_w, "Center", if *centered { "Yes" } else { "No" });
                            y += 18.0;
                            let preview: String = text.chars().take(20).collect();
                            self.render_property_row(cmds, lx, y, val_w, "Text", &preview);
                        }
                        SlideElement::Shape { kind, stroke_width, .. } => {
                            self.render_property_row(cmds, lx, y, val_w, "Type", kind.label());
                            y += 18.0;
                            self.render_property_row(cmds, lx, y, val_w, "Stroke", &format!("{stroke_width:.1}"));
                        }
                        SlideElement::Image { placeholder_label, .. } => {
                            self.render_property_row(cmds, lx, y, val_w, "Type", "Image");
                            y += 18.0;
                            self.render_property_row(cmds, lx, y, val_w, "Label", placeholder_label);
                        }
                        SlideElement::BulletList { items, font_size, .. } => {
                            self.render_property_row(cmds, lx, y, val_w, "Type", "Bullets");
                            y += 18.0;
                            self.render_property_row(
                                cmds, lx, y, val_w,
                                "Items",
                                &format!("{}", items.len()),
                            );
                            y += 18.0;
                            self.render_property_row(cmds, lx, y, val_w, "Size", &format!("{font_size:.0}"));
                        }
                    }
                }
            } else {
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y,
                    text: String::from("No element selected"),
                    color: OVERLAY0,
                    font_size: 11.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(val_w),
                });
                y += 24.0;

                // Layout buttons for quick insert.
                cmds.push(RenderCommand::Text {
                    x: lx,
                    y,
                    text: String::from("Insert Element:"),
                    color: SUBTEXT1,
                    font_size: 12.0,
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                });
                y += 20.0;

                let insert_items: &[&str] = &["Text Box", "Rectangle", "Ellipse", "Line", "Arrow", "Image"];
                for label in insert_items {
                    self.render_button(cmds, lx, y, val_w - 4.0, 22.0, label, SURFACE0, TEXT);
                    y += 26.0;
                }
            }
        }
    }

    /// Render a key-value property row.
    fn render_property_row(
        &self,
        cmds: &mut Vec<RenderCommand>,
        x: f32,
        y: f32,
        _max_w: f32,
        key: &str,
        value: &str,
    ) {
        cmds.push(RenderCommand::Text {
            x,
            y,
            text: key.to_string(),
            color: SUBTEXT0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
        });
        cmds.push(RenderCommand::Text {
            x: x + 75.0,
            y,
            text: value.to_string(),
            color: TEXT,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
        });
    }

    /// Render the speaker notes area below the canvas.
    fn render_notes_panel(&self, cmds: &mut Vec<RenderCommand>) {
        let notes_y = self.window_height - STATUS_BAR_HEIGHT - NOTES_HEIGHT;
        let notes_w = self.window_width - SIDEBAR_WIDTH - PROPERTIES_WIDTH;
        let nx = SIDEBAR_WIDTH;

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: nx,
            y: notes_y,
            width: notes_w,
            height: NOTES_HEIGHT,
            color: BASE,
            corner_radii: CornerRadii::ZERO,
        });
        // Separator.
        cmds.push(RenderCommand::Line {
            x1: nx,
            y1: notes_y,
            x2: nx + notes_w,
            y2: notes_y,
            color: SURFACE0,
            width: 1.0,
        });
        // Header.
        cmds.push(RenderCommand::Text {
            x: nx + 10.0,
            y: notes_y + 6.0,
            text: String::from("Speaker Notes"),
            color: SUBTEXT1,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
        });
        // Notes content.
        let notes_text = self.current_notes();
        let display = if notes_text.is_empty() {
            "(No notes for this slide)"
        } else {
            notes_text
        };
        cmds.push(RenderCommand::Text {
            x: nx + 10.0,
            y: notes_y + 24.0,
            text: display.to_string(),
            color: if notes_text.is_empty() { OVERLAY0 } else { TEXT },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(notes_w - 20.0),
        });
    }

    // ---- Slide sorter mode -------------------------------------------------

    /// Render the slide sorter grid view.
    fn render_sorter_mode(&self, cmds: &mut Vec<RenderCommand>) {
        self.render_toolbar(cmds);
        self.render_status_bar(cmds);

        let top = TOOLBAR_HEIGHT;
        let bot = self.window_height - STATUS_BAR_HEIGHT;
        let avail_w = self.window_width;
        let avail_h = bot - top;

        // How many columns can we fit?
        let thumb_w: f32 = 220.0;
        let thumb_h: f32 = thumb_w / SLIDE_ASPECT;
        let gap: f32 = 16.0;
        let cols = ((avail_w - gap) / (thumb_w + gap)).max(1.0) as usize;
        let total_grid_w = (cols as f32) * (thumb_w + gap) - gap;
        let grid_x = (avail_w - total_grid_w) / 2.0;

        // Clip to the sorter area.
        cmds.push(RenderCommand::PushClip {
            x: 0.0,
            y: top,
            width: avail_w,
            height: avail_h,
        });

        for (i, slide) in self.slides.iter().enumerate() {
            let col = i % cols;
            let row = i / cols;
            let tx = grid_x + (col as f32) * (thumb_w + gap);
            let ty = top + gap + (row as f32) * (thumb_h + gap + 24.0);

            let is_current = i == self.current_index;

            // Selection highlight.
            if is_current {
                cmds.push(RenderCommand::StrokeRect {
                    x: tx - 2.0,
                    y: ty - 2.0,
                    width: thumb_w + 4.0,
                    height: thumb_h + 4.0,
                    color: BLUE,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
            }

            // Thumbnail background.
            let bg = slide.effective_bg(&self.theme);
            cmds.push(RenderCommand::FillRect {
                x: tx,
                y: ty,
                width: thumb_w,
                height: thumb_h,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_R),
            });

            // Preview text.
            let preview = slide_preview_text(slide);
            if !preview.is_empty() {
                cmds.push(RenderCommand::Text {
                    x: tx + 8.0,
                    y: ty + 12.0,
                    text: preview,
                    color: TEXT,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(thumb_w - 16.0),
                });
            }

            // Transition label.
            if slide.transition != Transition::None {
                cmds.push(RenderCommand::Text {
                    x: tx + 4.0,
                    y: ty + thumb_h - 14.0,
                    text: slide.transition.label().to_string(),
                    color: TEAL,
                    font_size: 8.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                });
            }

            // Slide number.
            cmds.push(RenderCommand::Text {
                x: tx,
                y: ty + thumb_h + 4.0,
                text: format!("{}. {}", i.saturating_add(1), slide.layout.label()),
                color: if is_current { BLUE } else { SUBTEXT0 },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(thumb_w),
            });
        }

        cmds.push(RenderCommand::PopClip);
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Get a short preview text for a slide thumbnail.
fn slide_preview_text(slide: &Slide) -> String {
    // Try title first.
    if !slide.title.is_empty() {
        return truncate_str(&slide.title, 30);
    }
    // Otherwise, grab first text element content.
    for elem in &slide.elements {
        match elem {
            SlideElement::TextBox { text, .. } if !text.is_empty() => {
                return truncate_str(text, 30);
            }
            SlideElement::BulletList { items, .. } => {
                if let Some(first) = items.first() {
                    return truncate_str(first, 30);
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// Truncate a string to at most `max` characters, appending "..." if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let end = s.char_indices()
            .nth(max.saturating_sub(3))
            .map_or(s.len(), |(i, _)| i);
        let mut result = s[..end].to_string();
        result.push_str("...");
        result
    }
}

/// Convert a `Color` to a CSS `rgb()` string.
fn color_to_css(c: Color) -> String {
    format!("rgb({},{},{})", c.r, c.g, c.b)
}

/// Append HTML-escaped text to the output buffer.
fn push_html_escaped(out: &mut String, text: &str) {
    for ch in text.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let mut app = SlidesApp::new(1280.0, 720.0);

    // Add sample slides for demonstration.
    app.add_slide(SlideLayout::TitleContent);
    app.add_slide(SlideLayout::SectionHeader);
    app.add_slide(SlideLayout::TwoColumn);
    app.add_slide(SlideLayout::ImageCaption);
    app.add_slide(SlideLayout::Blank);

    // Set some notes.
    app.go_to_slide(0);
    app.set_current_notes(String::from("Welcome the audience. Introduce the topic."));

    // Render one frame.
    let cmds = app.render();
    let _ = cmds.len();

    // In a real OS environment, we would enter the event loop here:
    // loop {
    //     let event = wait_for_event();
    //     app.handle_event(event);
    //     let cmds = app.render();
    //     submit_render_commands(&cmds);
    // }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ---- IdGen tests -------------------------------------------------------

    #[test]
    fn test_id_gen_monotonic() {
        let mut g = IdGen::new(10);
        assert_eq!(g.next_id(), 10);
        assert_eq!(g.next_id(), 11);
        assert_eq!(g.next_id(), 12);
    }

    #[test]
    fn test_id_gen_saturating() {
        let mut g = IdGen::new(u64::MAX);
        assert_eq!(g.next_id(), u64::MAX);
        assert_eq!(g.next_id(), u64::MAX);
    }

    // ---- Transition tests --------------------------------------------------

    #[test]
    fn test_transition_label() {
        assert_eq!(Transition::None.label(), "None");
        assert_eq!(Transition::Fade.label(), "Fade");
        assert_eq!(Transition::SlideLeft.label(), "Slide Left");
        assert_eq!(Transition::SlideRight.label(), "Slide Right");
        assert_eq!(Transition::Wipe.label(), "Wipe");
        assert_eq!(Transition::Dissolve.label(), "Dissolve");
    }

    #[test]
    fn test_transition_all() {
        let all = Transition::all();
        assert_eq!(all.len(), 6);
        assert_eq!(all[0], Transition::None);
        assert_eq!(all[5], Transition::Dissolve);
    }

    // ---- SlideLayout tests -------------------------------------------------

    #[test]
    fn test_layout_label() {
        assert_eq!(SlideLayout::TitleSlide.label(), "Title Slide");
        assert_eq!(SlideLayout::TitleContent.label(), "Title + Content");
        assert_eq!(SlideLayout::SectionHeader.label(), "Section Header");
        assert_eq!(SlideLayout::Blank.label(), "Blank");
        assert_eq!(SlideLayout::TwoColumn.label(), "Two Column");
        assert_eq!(SlideLayout::ImageCaption.label(), "Image + Caption");
    }

    #[test]
    fn test_layout_all() {
        let all = SlideLayout::all();
        assert_eq!(all.len(), 6);
    }

    // ---- ShapeKind tests ---------------------------------------------------

    #[test]
    fn test_shape_kind_label() {
        assert_eq!(ShapeKind::Rectangle.label(), "Rectangle");
        assert_eq!(ShapeKind::Ellipse.label(), "Ellipse");
        assert_eq!(ShapeKind::Line.label(), "Line");
        assert_eq!(ShapeKind::Arrow.label(), "Arrow");
    }

    // ---- SlideElement tests ------------------------------------------------

    #[test]
    fn test_textbox_bounds() {
        let e = SlideElement::TextBox {
            id: 1,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            text: String::from("Hi"),
            font_size: 14.0,
            color: TEXT,
            bold: false,
            centered: false,
        };
        assert_eq!(e.bounds(), (10.0, 20.0, 100.0, 50.0));
        assert_eq!(e.id(), 1);
    }

    #[test]
    fn test_element_translate() {
        let mut e = SlideElement::Shape {
            id: 2,
            kind: ShapeKind::Rectangle,
            x: 50.0,
            y: 50.0,
            width: 80.0,
            height: 60.0,
            fill_color: BLUE,
            stroke_color: BLUE,
            stroke_width: 1.0,
        };
        e.translate(10.0, -5.0);
        assert_eq!(e.bounds(), (60.0, 45.0, 80.0, 60.0));
    }

    #[test]
    fn test_element_set_position() {
        let mut e = SlideElement::Image {
            id: 3,
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 150.0,
            placeholder_label: String::from("Photo"),
        };
        e.set_position(100.0, 200.0);
        let (x, y, _, _) = e.bounds();
        assert!((x - 100.0).abs() < f32::EPSILON);
        assert!((y - 200.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_element_set_size() {
        let mut e = SlideElement::BulletList {
            id: 4,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 200.0,
            items: vec![String::from("A")],
            font_size: 16.0,
            color: TEXT,
        };
        e.set_size(300.0, 400.0);
        let (_, _, w, h) = e.bounds();
        assert!((w - 300.0).abs() < f32::EPSILON);
        assert!((h - 400.0).abs() < f32::EPSILON);
    }

    // ---- SlideTheme tests --------------------------------------------------

    #[test]
    fn test_theme_mocha() {
        let t = SlideTheme::mocha();
        assert_eq!(t.name, "Mocha");
        assert!(t.title_size > t.subtitle_size);
        assert!(t.subtitle_size > t.body_size);
    }

    #[test]
    fn test_theme_light() {
        let t = SlideTheme::light();
        assert_eq!(t.name, "Light");
    }

    #[test]
    fn test_theme_vibrant() {
        let t = SlideTheme::vibrant();
        assert_eq!(t.name, "Vibrant");
    }

    // ---- Slide tests -------------------------------------------------------

    #[test]
    fn test_slide_new_title_layout() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(100);
        let s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        assert_eq!(s.layout, SlideLayout::TitleSlide);
        assert!(!s.elements.is_empty());
        assert_eq!(s.transition, Transition::None);
        assert!(s.notes.is_empty());
    }

    #[test]
    fn test_slide_blank_layout_has_no_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(200);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        assert!(s.elements.is_empty());
    }

    #[test]
    fn test_slide_effective_bg_default() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(300);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        assert_eq!(s.effective_bg(&theme), theme.background);
    }

    #[test]
    fn test_slide_effective_bg_override() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(400);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let red = Color::from_hex(0xF38BA8);
        s.background = Some(red);
        assert_eq!(s.effective_bg(&theme), red);
    }

    #[test]
    fn test_slide_element_by_id() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(500);
        let s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        let first_id = s.elements[0].id();
        assert!(s.element_by_id(first_id).is_some());
        assert!(s.element_by_id(99999).is_none());
    }

    #[test]
    fn test_slide_remove_element() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(600);
        let mut s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        let count_before = s.elements.len();
        let first_id = s.elements[0].id();
        let removed = s.remove_element(first_id);
        assert!(removed.is_some());
        assert_eq!(s.elements.len(), count_before - 1);
        assert!(s.remove_element(99999).is_none());
    }

    // ---- UndoManager tests -------------------------------------------------

    #[test]
    fn test_undo_redo_basic() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(700);
        let s1 = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let s2 = Slide::new(2, SlideLayout::TitleSlide, &theme, &mut id_gen);

        let mut mgr = UndoManager::new(10);
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());

        // Save state [s1], then mutate to [s1, s2].
        mgr.save(&[s1.clone()], 0);
        let slides_after = vec![s1.clone(), s2];

        // Undo: should restore [s1].
        let snap = mgr.undo(&slides_after, 1);
        assert!(snap.is_some());
        let snap = snap.unwrap();
        assert_eq!(snap.slides.len(), 1);
        assert_eq!(snap.current_index, 0);

        // Can redo now.
        assert!(mgr.can_redo());
        let redo_snap = mgr.redo(&snap.slides, snap.current_index);
        assert!(redo_snap.is_some());
        let redo_snap = redo_snap.unwrap();
        assert_eq!(redo_snap.slides.len(), 2);
    }

    #[test]
    fn test_undo_max_depth() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(800);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let mut mgr = UndoManager::new(3);

        for _ in 0..5 {
            mgr.save(&[s.clone()], 0);
        }
        // Only 3 saved (max depth).
        assert_eq!(mgr.undo_stack.len(), 3);
    }

    #[test]
    fn test_save_clears_redo() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(900);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let mut mgr = UndoManager::new(10);
        mgr.save(&[s.clone()], 0);
        let _ = mgr.undo(&[s.clone()], 0);
        assert!(mgr.can_redo());
        mgr.save(&[s.clone()], 0);
        assert!(!mgr.can_redo());
    }

    // ---- SlidesApp tests ---------------------------------------------------

    #[test]
    fn test_app_new() {
        let app = SlidesApp::new(1280.0, 720.0);
        assert_eq!(app.slide_count(), 1);
        assert_eq!(app.current_index(), 0);
        assert_eq!(app.view, ViewMode::Edit);
        assert_eq!(app.title, "Untitled Presentation");
    }

    #[test]
    fn test_add_slide() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::TitleContent);
        assert_eq!(app.slide_count(), 2);
        assert_eq!(app.current_index(), 1);
    }

    #[test]
    fn test_delete_slide() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.add_slide(SlideLayout::SectionHeader);
        assert_eq!(app.slide_count(), 3);
        app.delete_slide(1);
        assert_eq!(app.slide_count(), 2);
    }

    #[test]
    fn test_delete_last_slide_prevented() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.delete_slide(0);
        // Should still have 1 slide.
        assert_eq!(app.slide_count(), 1);
    }

    #[test]
    fn test_navigate_slides() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.add_slide(SlideLayout::Blank);
        app.go_to_slide(0);
        assert_eq!(app.current_index(), 0);
        app.next_slide();
        assert_eq!(app.current_index(), 1);
        app.next_slide();
        assert_eq!(app.current_index(), 2);
        app.next_slide();
        assert_eq!(app.current_index(), 2); // At end, no change.
        app.prev_slide();
        assert_eq!(app.current_index(), 1);
        app.prev_slide();
        assert_eq!(app.current_index(), 0);
        app.prev_slide();
        assert_eq!(app.current_index(), 0); // At start, no change.
    }

    #[test]
    fn test_go_to_invalid_slide() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.go_to_slide(999);
        assert_eq!(app.current_index(), 0);
    }

    #[test]
    fn test_duplicate_slide() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.go_to_slide(0);
        app.duplicate_current_slide();
        assert_eq!(app.slide_count(), 2);
        assert_eq!(app.current_index(), 1);
        // The duplicated slide should have a different ID.
        assert_ne!(app.slides[0].id, app.slides[1].id);
    }

    #[test]
    fn test_move_slide_up() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.add_slide(SlideLayout::SectionHeader);
        // current_index is 2 (SectionHeader).
        let id_at_2 = app.slides[2].id;
        app.move_slide_up();
        assert_eq!(app.current_index(), 1);
        assert_eq!(app.slides[1].id, id_at_2);
    }

    #[test]
    fn test_move_slide_down() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.go_to_slide(0);
        let id_at_0 = app.slides[0].id;
        app.move_slide_down();
        assert_eq!(app.current_index(), 1);
        assert_eq!(app.slides[1].id, id_at_0);
    }

    #[test]
    fn test_move_slide_up_at_start() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.go_to_slide(0);
        app.move_slide_up();
        assert_eq!(app.current_index(), 0); // No change.
    }

    #[test]
    fn test_move_slide_down_at_end() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.move_slide_down();
        assert_eq!(app.current_index(), 0); // No change.
    }

    #[test]
    fn test_copy_paste_slide() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.copy_slide();
        app.paste_slide();
        assert_eq!(app.slide_count(), 2);
        assert_ne!(app.slides[0].id, app.slides[1].id);
    }

    #[test]
    fn test_paste_without_copy() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.paste_slide();
        assert_eq!(app.slide_count(), 1); // No change.
    }

    #[test]
    fn test_add_textbox() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        let before = app.slides[0].elements.len();
        app.add_textbox();
        assert_eq!(app.slides[0].elements.len(), before + 1);
        assert!(app.selected_element.is_some());
    }

    #[test]
    fn test_add_shape() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_shape(ShapeKind::Ellipse);
        assert!(app.selected_element.is_some());
        let last = app.slides[0].elements.last().unwrap();
        if let SlideElement::Shape { kind, .. } = last {
            assert_eq!(*kind, ShapeKind::Ellipse);
        } else {
            panic!("Expected Shape element");
        }
    }

    #[test]
    fn test_add_image_placeholder() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        let before = app.slides[0].elements.len();
        app.add_image_placeholder();
        assert_eq!(app.slides[0].elements.len(), before + 1);
    }

    #[test]
    fn test_delete_selected_element() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_textbox();
        let eid = app.selected_element;
        assert!(eid.is_some());
        let before = app.slides[0].elements.len();
        app.delete_selected_element();
        assert_eq!(app.slides[0].elements.len(), before - 1);
        assert!(app.selected_element.is_none());
    }

    #[test]
    fn test_delete_no_selection() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        let before = app.slides[0].elements.len();
        app.delete_selected_element();
        assert_eq!(app.slides[0].elements.len(), before); // No change.
    }

    #[test]
    fn test_undo_redo_integration() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        assert_eq!(app.slide_count(), 1);
        app.add_slide(SlideLayout::Blank);
        assert_eq!(app.slide_count(), 2);
        app.undo();
        assert_eq!(app.slide_count(), 1);
        app.redo();
        assert_eq!(app.slide_count(), 2);
    }

    #[test]
    fn test_undo_empty() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.undo(); // Should not panic.
        assert_eq!(app.slide_count(), 1);
    }

    #[test]
    fn test_set_transition() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.set_current_transition(Transition::Fade);
        assert_eq!(app.slides[0].transition, Transition::Fade);
    }

    #[test]
    fn test_set_notes() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.set_current_notes(String::from("Test note"));
        assert_eq!(app.current_notes(), "Test note");
    }

    #[test]
    fn test_set_theme() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        let light = SlideTheme::light();
        app.set_theme(light);
        assert_eq!(app.theme.name, "Light");
    }

    // ---- Render tests ------------------------------------------------------

    #[test]
    fn test_render_edit_mode() {
        let app = SlidesApp::new(1280.0, 720.0);
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_sorter_mode() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.view = ViewMode::Sorter;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selected_element() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_textbox();
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_notes_hidden() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.show_notes = false;
        let cmds = app.render();
        assert!(!cmds.is_empty());
    }

    // ---- Export HTML tests -------------------------------------------------

    #[test]
    fn test_export_html_basic() {
        let app = SlidesApp::new(1280.0, 720.0);
        let html = app.export_html();
        assert!(html.contains("<!DOCTYPE html>"));
        assert!(html.contains("</html>"));
        assert!(html.contains("slide-0"));
        assert!(html.contains("nextSlide"));
        assert!(html.contains("prevSlide"));
    }

    #[test]
    fn test_export_html_multiple_slides() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.add_slide(SlideLayout::TitleContent);
        let html = app.export_html();
        assert!(html.contains("slide-0"));
        assert!(html.contains("slide-1"));
        assert!(html.contains("slide-2"));
    }

    #[test]
    fn test_export_html_escapes() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.title = String::from("Test <>&\"' title");
        let html = app.export_html();
        assert!(html.contains("&lt;"));
        assert!(html.contains("&gt;"));
        assert!(html.contains("&amp;"));
    }

    // ---- Helper function tests ---------------------------------------------

    #[test]
    fn test_truncate_str_short() {
        let s = truncate_str("hello", 10);
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_truncate_str_exact() {
        let s = truncate_str("hello", 5);
        assert_eq!(s, "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        let s = truncate_str("hello world, this is a long string", 10);
        assert!(s.ends_with("..."));
        assert!(s.len() <= 13); // 10-3 chars + "..."
    }

    #[test]
    fn test_color_to_css() {
        let c = Color::rgb(255, 128, 0);
        assert_eq!(color_to_css(c), "rgb(255,128,0)");
    }

    #[test]
    fn test_push_html_escaped() {
        let mut out = String::new();
        push_html_escaped(&mut out, "a<b>c&d\"e");
        assert_eq!(out, "a&lt;b&gt;c&amp;d&quot;e");
    }

    #[test]
    fn test_slide_preview_text_from_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(5000);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        assert!(slide_preview_text(&s).is_empty());
        s.elements.push(SlideElement::TextBox {
            id: id_gen.next_id(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 30.0,
            text: String::from("Preview text"),
            font_size: 14.0,
            color: TEXT,
            bold: false,
            centered: false,
        });
        assert_eq!(slide_preview_text(&s), "Preview text");
    }

    #[test]
    fn test_slide_preview_from_bullets() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(6000);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        s.elements.push(SlideElement::BulletList {
            id: id_gen.next_id(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            items: vec![String::from("Bullet one")],
            font_size: 14.0,
            color: TEXT,
        });
        assert_eq!(slide_preview_text(&s), "Bullet one");
    }

    // ---- Layout template element count tests -------------------------------

    #[test]
    fn test_title_slide_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(7000);
        let s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        // Title + Subtitle + decorative line = 3 elements.
        assert_eq!(s.elements.len(), 3);
    }

    #[test]
    fn test_title_content_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(7100);
        let s = Slide::new(1, SlideLayout::TitleContent, &theme, &mut id_gen);
        // Title + bullet list = 2.
        assert_eq!(s.elements.len(), 2);
    }

    #[test]
    fn test_section_header_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(7200);
        let s = Slide::new(1, SlideLayout::SectionHeader, &theme, &mut id_gen);
        // Title + bottom bar = 2.
        assert_eq!(s.elements.len(), 2);
    }

    #[test]
    fn test_two_column_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(7300);
        let s = Slide::new(1, SlideLayout::TwoColumn, &theme, &mut id_gen);
        // Title + left bullets + right bullets = 3.
        assert_eq!(s.elements.len(), 3);
    }

    #[test]
    fn test_image_caption_elements() {
        let theme = SlideTheme::mocha();
        let mut id_gen = IdGen::new(7400);
        let s = Slide::new(1, SlideLayout::ImageCaption, &theme, &mut id_gen);
        // Image placeholder + caption = 2.
        assert_eq!(s.elements.len(), 2);
    }
}
