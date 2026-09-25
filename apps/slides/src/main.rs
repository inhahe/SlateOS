//! `Slate OS` Slides
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
//! - Slide transitions: Fade, `SlideLeft`, `SlideRight`, Wipe, Dissolve
//! - Speaker notes per slide
//! - Slide numbering
//! - Export to self-contained HTML slideshow
//! - Copy/paste/duplicate slides, reorder (move up/down)
//! - Undo/redo stack
//! - Keyboard shortcuts (Ctrl+N, Ctrl+D, Ctrl+Z, Ctrl+Y, Ctrl+E, etc.)
//! - Multi-panel UI: slide thumbnail sidebar, main canvas, properties panel
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
use guitk::frame::{Frame, Rect};
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::wheel;
use oswindow::app::{self, App, Response};
use std::process::ExitCode;
use std::time::Duration;

use std::collections::VecDeque;

// ============================================================================
// Catppuccin Mocha theme constants
// ============================================================================

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
            Self::TextBox {
                x,
                y,
                width,
                height,
                ..
            }
            | Self::Shape {
                x,
                y,
                width,
                height,
                ..
            }
            | Self::Image {
                x,
                y,
                width,
                height,
                ..
            }
            | Self::BulletList {
                x,
                y,
                width,
                height,
                ..
            } => (*x, *y, *width, *height),
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
    pub fn mocha(pal: &Palette) -> Self {
        Self {
            name: String::from("Mocha"),
            background: pal.crust,
            title_color: pal.text,
            subtitle_color: pal.subtext1,
            body_color: pal.subtext0,
            accent: pal.blue,
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
///
/// The theme is part of it: a theme change restyles every element, and an
/// undo that brought the old colours back under the new theme would leave the
/// deck in neither.
#[derive(Clone, Debug)]
struct Snapshot {
    slides: Vec<Slide>,
    current_index: usize,
    theme: SlideTheme,
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
    fn save(&mut self, now: Snapshot) {
        if self.undo_stack.len() >= self.max_depth {
            self.undo_stack.pop_front();
        }
        self.undo_stack.push_back(now);
        self.redo_stack.clear();
    }

    /// Undo: return the previous snapshot, saving the current state to redo.
    fn undo(&mut self, now: Snapshot) -> Option<Snapshot> {
        let prev = self.undo_stack.pop_back()?;
        self.redo_stack.push(now);
        Some(prev)
    }

    /// Redo: return the next snapshot, saving the current state to undo.
    fn redo(&mut self, now: Snapshot) -> Option<Snapshot> {
        let next = self.redo_stack.pop()?;
        self.undo_stack.push_back(now);
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

/// Every key this program answers, and what it does.
///
/// Twenty bindings and, until this list existed, no way to learn any of them
/// but reading the source -- including the five that put shapes on a slide,
/// which are the ones somebody wants first.
///
/// **Each row is a key this program actually answers**, which is not a
/// property the list has on its own: `every_advertised_key_does_something`
/// walks it and asserts each one is taken. `apps/rssreader` shipped an
/// overlay of twenty-one shortcuts of which about four worked, and the only
/// thing that keeps a list and a handler together is a test that reads both.
///
/// Kept to what the card can show at the window's opening size without an
/// "and N more" line: related keys share a row.
const SHORTCUTS: &[(&str, &str)] = &[
    ("PgUp / PgDn", "Previous / next slide"),
    ("Home / End", "First / last slide"),
    (
        "Arrows",
        "Move the selected element; Left / Right change slides with none",
    ),
    ("Shift+Arrows", "Resize the selected element"),
    ("Tab / Shift+Tab", "Select the next / previous element"),
    ("Esc", "Select nothing"),
    ("Enter / F2", "Type into the selected element"),
    ("T / I", "Add a text box / an image placeholder"),
    ("S / O / L / A", "Add a rectangle / ellipse / line / arrow"),
    ("Ctrl+B / Ctrl+Shift+C", "Bold / centre the selected text"),
    (
        "Ctrl+] / Ctrl+[",
        "Larger / smaller text, or a thicker / thinner outline",
    ),
    ("C", "Next colour for the selected element"),
    (
        "Delete / Shift+Delete",
        "Delete the selected element (or the slide) / the slide",
    ),
    (
        "Ctrl+N / Ctrl+M",
        "New slide / new slide in a layout you choose",
    ),
    ("Ctrl+D", "Duplicate this slide"),
    ("Ctrl+C / Ctrl+V", "Copy / paste a slide"),
    ("Ctrl+PgUp / Ctrl+PgDn", "Move this slide up / down"),
    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
    ("G", "Next background for this slide"),
    ("Ctrl+R", "Next transition into this slide"),
    ("Ctrl+T", "Next theme"),
    ("Ctrl+Shift+T", "Name the deck"),
    ("N / B", "Type / show or hide the speaker notes"),
    ("1 / 2", "Edit view / sorter view"),
    ("Ctrl+E", "Export a web page"),
    ("F1 / ?", "This list"),
];

/// The colours an element steps through with `C`, after the theme's own four.
const EXTRA_COLOURS: [Color; 6] = [
    Color::rgb(255, 255, 255),
    Color::rgb(17, 17, 27),
    Color::rgb(243, 139, 168),
    Color::rgb(166, 227, 161),
    Color::rgb(249, 226, 175),
    Color::rgb(137, 180, 250),
];

/// The backgrounds a slide steps through with `G`: the theme's own first
/// (`None`), then these.
const BACKGROUNDS: [Option<Color>; 7] = [
    None,
    Some(Color::rgb(17, 17, 27)),
    Some(Color::rgb(30, 58, 95)),
    Some(Color::rgb(45, 74, 43)),
    Some(Color::rgb(74, 29, 46)),
    Some(Color::rgb(245, 245, 240)),
    Some(Color::rgb(255, 255, 255)),
];

/// Everything in the window a pointer can press, as the renderer records it.
///
/// The editor drew a toolbar, thumbnails, a slide with its elements, a
/// property panel with six Insert buttons and a notes panel, and handled no
/// pointer event (`known-issues.md` ->
/// `TD-C-TWENTY-ONE-APPLICATIONS-DRAW-A-UI-THAT-CANNOT-BE-CLICKED`). Slides
/// are named by index and elements by id: an element's index moves when one
/// before it is deleted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Tool(Tool),
    /// A layout in the new-slide menu.
    LayoutChoice(usize),
    /// Around the new-slide menu: a press closes it.
    MenuBackdrop,
    /// The menu's own card, between its rows: a press does nothing.
    Menu,
    /// The thumbnail column, which scrolls.
    Sidebar,
    Thumb(usize),
    /// The grey around the slide: a press selects nothing.
    Canvas,
    /// The slide itself, under its elements: a press selects nothing.
    SlideArea,
    Element(ElementId),
    /// A corner of the selected element, which resizes it.
    Handle(Corner),
    Prop(Prop),
    /// One of the Insert buttons, by `INSERT_KINDS` index.
    Insert(usize),
    /// The speaker notes: a press starts typing them.
    Notes,
    /// The sorter's grid, which scrolls.
    SorterGrid,
    SorterThumb(usize),
    HelpCard,
}

/// A toolbar button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// The deck's name: a press starts naming it.
    Title,
    NewSlide,
    Duplicate,
    DeleteSlide,
    ToggleView,
    Undo,
    Redo,
    Export,
    Theme,
}

/// A property-panel button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prop {
    Transition,
    Background,
    Smaller,
    Larger,
    Bold,
    Centre,
    Colour,
    Edit,
    Delete,
}

/// A corner of an element's box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// What a press held down is doing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Moving an element: where the press was and where the element was, in
    /// slide units. `edit` is a press on the element already selected, which
    /// starts typing if the pointer comes up without having moved it.
    Move {
        id: ElementId,
        from: (f32, f32),
        origin: (f32, f32),
        moved: bool,
        edit: bool,
    },
    /// Resizing from a corner, from the box the element had at the press.
    Resize {
        id: ElementId,
        corner: Corner,
        from: (f32, f32),
        bounds: (f32, f32, f32, f32),
        moved: bool,
    },
    /// A thumbnail on its way to a new place in the deck. `was_current` is a
    /// press on the slide already shown, which in the sorter opens it.
    Slide {
        from: usize,
        at: (f32, f32),
        moved: bool,
        was_current: bool,
    },
}

/// What the Insert buttons add, in their order.
const INSERT_KINDS: [&str; 6] = ["Text Box", "Rectangle", "Ellipse", "Line", "Arrow", "Image"];

/// How far one arrow press moves or resizes an element, in slide units; with
/// Ctrl, one unit.
const NUDGE: f32 = 10.0;

/// The smallest box an element can be resized to. A line or an arrow may be
/// flat in either direction, so it has none.
const MIN_ELEMENT: f32 = 8.0;

/// A resize handle's side, in window pixels.
const HANDLE: f32 = 8.0;

/// Where the toolbar's buttons start, right of the deck's name.
const TOOLS_X: f32 = 216.0;

/// What a typed string is going onto.
///
/// An enum because the deck's own name is not an element and has no id, and a
/// second `Option` beside the first would be a state where both could be set
/// at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditTarget {
    /// A text box on the current slide.
    Element(ElementId),
    /// The deck's name, which the window bar and every export use.
    DeckTitle,
    /// The current slide's speaker notes. They were shown and exported and
    /// could not be written: `set_current_notes` had one caller, the sample
    /// deck.
    Notes,
}

/// The main presentation application state.
#[derive(Debug)]
pub struct SlidesApp {
    /// The export picker.
    ///
    /// `export_html` was written and tested and had no caller: a DOCTYPE, a
    /// charset, a viewport, escaped text, a stylesheet and per-slide `<div>`s
    /// with working navigation controls -- **a whole presentation format that
    /// nothing could ask for.** This crate had no `std::fs` and no dialog, so
    /// a deck lived exactly as long as the window did.
    pub picker: FilePicker,
    /// What the last export did.
    pub status_message: Option<String>,
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
    /// Whether the shortcut list is up.
    show_help: bool,
    /// The thing being typed into, and what has been typed.
    ///
    /// This program could not put a word on a slide: there were zero
    /// assignments to `.text` anywhere in the crate, tests included, and zero
    /// `key.text` sites, so every box said "New Text" or "Presentation Title"
    /// forever. The buffer is held here rather than written straight into the
    /// element so that `Escape` and `Enter` can mean different things -- and
    /// the element is named by id, because the selection can move.
    editing: Option<(EditTarget, String)>,
    /// Title of the presentation.
    title: String,
    /// The user's colours, replaced whenever the theme changes.
    ///
    /// Seeded from the defaults so the field is never absent; the framework
    /// calls `App::theme_changed` before the first frame, so nothing is drawn
    /// with this initial value in a real window.
    palette: Palette,
    /// The new-slide menu, with the layout its keys have reached.
    layout_menu: Option<usize>,
    /// What a press held down is doing.
    drag: Option<Drag>,
    /// How far the thumbnail column and the sorter are scrolled, in pixels.
    sidebar_scroll: f32,
    sorter_scroll: f32,
    /// The wheel's remainder.
    wheel: wheel::Accumulator,
    /// What the pointer is over, so it can be drawn lit.
    hover: Option<Target>,
    /// Every box the last paint recorded, for hover and the wheel.
    last_hits: Vec<(Target, Rect)>,
}

impl SlidesApp {
    /// Create a new presentation with one default title slide.
    pub fn new(width: f32, height: f32) -> Self {
        let theme = SlideTheme::mocha(&Palette::from_settings(
            &appearance::AppearanceSettings::default(),
        ));
        let mut id_gen = IdGen::new(1);
        let slide_id = id_gen.next_id();
        let first = Slide::new(slide_id, SlideLayout::TitleSlide, &theme, &mut id_gen);

        Self {
            palette: Palette::from_settings(&appearance::AppearanceSettings::default()),
            picker: FilePicker::default(),
            status_message: None,
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
            show_help: false,
            editing: None,
            title: String::from("Untitled Presentation"),
            layout_menu: None,
            drag: None,
            sidebar_scroll: 0.0,
            sorter_scroll: 0.0,
            wheel: wheel::Accumulator::default(),
            hover: None,
            last_hits: Vec::new(),
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
        self.checkpoint();
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
        self.checkpoint();
        self.slides.remove(index);
        if self.current_index >= self.slides.len() {
            self.current_index = self.slides.len().saturating_sub(1);
        }
        self.selected_element = None;
    }

    /// Duplicate the current slide, inserting the copy immediately after it.
    pub fn duplicate_current_slide(&mut self) {
        if let Some(slide) = self.slides.get(self.current_index).cloned() {
            self.checkpoint();
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
        self.checkpoint();
        self.slides
            .swap(self.current_index, self.current_index.saturating_sub(1));
        self.current_index = self.current_index.saturating_sub(1);
    }

    /// Move the current slide down (towards the last index).
    pub fn move_slide_down(&mut self) {
        if self.current_index.saturating_add(1) >= self.slides.len() {
            return;
        }
        self.checkpoint();
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
            let mut pasted = slide.clone();
            self.checkpoint();
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
        self.checkpoint();
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
        self.checkpoint();
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
        self.checkpoint();
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
            self.checkpoint();
            if let Some(slide) = self.slides.get_mut(self.current_index) {
                slide.remove_element(eid);
            }
            self.selected_element = None;
        }
    }

    // ---- Undo/Redo ---------------------------------------------------------

    /// The deck as it is, for the undo stack.
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            slides: self.slides.clone(),
            current_index: self.current_index,
            theme: self.theme.clone(),
        }
    }

    /// Remember the deck before a change, so it can be undone.
    fn checkpoint(&mut self) {
        let now = self.snapshot();
        self.undo_mgr.save(now);
    }

    /// Put a remembered deck back.
    fn restore(&mut self, snap: Snapshot) {
        self.slides = snap.slides;
        self.theme = snap.theme;
        self.current_index = snap.current_index.min(self.slides.len().saturating_sub(1));
        self.selected_element = None;
    }

    /// Undo the last action.
    pub fn undo(&mut self) {
        let now = self.snapshot();
        if let Some(snap) = self.undo_mgr.undo(now) {
            self.restore(snap);
        }
    }

    /// Redo the last undone action.
    pub fn redo(&mut self) {
        let now = self.snapshot();
        if let Some(snap) = self.undo_mgr.redo(now) {
            self.restore(snap);
        }
    }

    // ---- Theme -------------------------------------------------------------

    /// Set the presentation theme and re-apply it to all slides.
    /// The words a box is born holding: a prompt, not content.
    ///
    /// Typing into a new box must replace these rather than append to them:
    /// nobody wants "New TextHi", and making the user delete the prompt first is
    /// the friction that stops them writing at all. Every presentation tool on
    /// earth behaves this way. The cost is that a user who genuinely wants a box
    /// reading exactly "New Text" has to type it twice, which is a fair trade.
    const PLACEHOLDER_TEXT: &[&str] = &[
        "New Text",
        "Presentation Title",
        "Section Title",
        "Slide Title",
        "Subtitle goes here",
        "Two Column Layout",
        "Caption and description text goes here.",
    ];

    /// Begin typing into the selected text box, seeded with what it says.
    ///
    /// Seeded because editing is usually an edit: a blank box would make the
    /// existing words something the user has to retype, and the words are
    /// what they came for.
    fn begin_editing(&mut self) -> EventResult {
        let Some(eid) = self.selected_element else {
            return EventResult::Ignored;
        };
        let Some(slide) = self.slides.get(self.current_index) else {
            return EventResult::Ignored;
        };
        // A box still holding its prompt starts empty; one the user has
        // written in starts with what they wrote, because editing is usually
        // an edit and retyping it is not.
        //
        // A bullet list is typed as one line per bullet, and an image
        // placeholder's label as its words. Only text boxes could be typed
        // into, so every Title + Content slide said "First point", "Second
        // point" and "Third point" for good.
        let words = match slide.elements.iter().find(|e| e.id() == eid) {
            Some(SlideElement::TextBox { text, .. }) => {
                if Self::PLACEHOLDER_TEXT.contains(&text.as_str()) {
                    String::new()
                } else {
                    text.clone()
                }
            }
            Some(SlideElement::BulletList { items, .. }) => {
                if items
                    .iter()
                    .all(|i| Self::PLACEHOLDER_BULLETS.contains(&i.as_str()))
                {
                    String::new()
                } else {
                    items.join("\n")
                }
            }
            Some(SlideElement::Image {
                placeholder_label, ..
            }) => {
                if Self::PLACEHOLDER_TEXT.contains(&placeholder_label.as_str())
                    || placeholder_label == "Image"
                    || placeholder_label == "Image Placeholder"
                {
                    String::new()
                } else {
                    placeholder_label.clone()
                }
            }
            // A shape holds no words; saying so beats a mode that silently
            // does nothing.
            Some(SlideElement::Shape { .. }) | None => return EventResult::Ignored,
        };
        self.editing = Some((EditTarget::Element(eid), words));
        EventResult::Consumed
    }

    /// The bullets a layout is born holding: prompts, like `PLACEHOLDER_TEXT`.
    const PLACEHOLDER_BULLETS: &[&str] = &[
        "First point",
        "Second point",
        "Third point",
        "Left column point A",
        "Left column point B",
        "Right column point A",
        "Right column point B",
    ];

    /// Begin typing this slide's speaker notes, seeded with what they say.
    fn begin_notes(&mut self) -> EventResult {
        let seed = self.current_notes().to_owned();
        // Shown while they are typed, whether or not the panel was up.
        self.show_notes = true;
        self.editing = Some((EditTarget::Notes, seed));
        EventResult::Consumed
    }

    /// Begin naming the deck.
    ///
    /// The name is not decoration: it is the window bar, and `export_as`
    /// builds the filename from it -- so while it could not be changed, every
    /// deck anyone exported was called "Untitled Presentation".
    fn begin_deck_title(&mut self) -> EventResult {
        let seed = if self.title == "Untitled Presentation" {
            String::new()
        } else {
            self.title.clone()
        };
        self.editing = Some((EditTarget::DeckTitle, seed));
        EventResult::Consumed
    }

    /// Keys while a text box is being typed into.
    fn handle_editing_key(&mut self, key: &KeyEvent) -> EventResult {
        let Some((target, mut buf)) = self.editing.clone() else {
            return EventResult::Ignored;
        };
        match key.key {
            // Leaving keeps the words, on either key. Losing what was typed
            // because the exit key was the cancelling one is the worst thing
            // an editor can do, and `Escape` is how anyone leaves a box.
            Key::Escape | Key::Enter if !key.modifiers.shift => {
                self.commit_editing(target, &buf);
                self.editing = None;
                EventResult::Consumed
            }
            // Shift+Enter is the second line.
            Key::Enter => {
                buf.push('\n');
                self.editing = Some((target, buf));
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

    /// Write the typed words where they were being typed.
    fn commit_editing(&mut self, target: EditTarget, buf: &str) {
        match target {
            EditTarget::DeckTitle => {
                // An empty name would leave the window bar blank and every
                // export called ".pptx"; refusing keeps whatever it had.
                if !buf.trim().is_empty() {
                    self.title = buf.trim().to_owned();
                }
            }
            EditTarget::Notes => {
                if self.current_notes() == buf {
                    return;
                }
                self.checkpoint();
                self.set_current_notes(buf.to_owned());
            }
            EditTarget::Element(eid) => {
                self.checkpoint();
                let Some(slide) = self.slides.get_mut(self.current_index) else {
                    return;
                };
                match slide.element_by_id_mut(eid) {
                    Some(SlideElement::TextBox { text, .. }) => *text = buf.to_owned(),
                    // One bullet per line; a blank line is not a bullet.
                    Some(SlideElement::BulletList { items, .. }) => {
                        *items = buf
                            .split('\n')
                            .map(str::trim_end)
                            .filter(|line| !line.trim().is_empty())
                            .map(str::to_owned)
                            .collect();
                    }
                    Some(SlideElement::Image {
                        placeholder_label, ..
                    }) => {
                        *placeholder_label = buf.trim().to_owned();
                    }
                    Some(SlideElement::Shape { .. }) | None => {}
                }
            }
        }
    }

    // ---- Element properties -------------------------------------------------

    /// The selected element on the current slide.
    fn selected(&self) -> Option<&SlideElement> {
        let eid = self.selected_element?;
        self.slides.get(self.current_index)?.element_by_id(eid)
    }

    /// Change the selected element, remembering it for undo first. Reports
    /// whether there was one and `change` said it changed.
    fn change_selected(&mut self, change: impl FnOnce(&mut SlideElement) -> bool) -> EventResult {
        let Some(eid) = self.selected_element else {
            return EventResult::Ignored;
        };
        let Some(mut element) = self.selected().cloned() else {
            return EventResult::Ignored;
        };
        if !change(&mut element) {
            return EventResult::Ignored;
        }
        self.checkpoint();
        if let Some(slot) = self
            .slides
            .get_mut(self.current_index)
            .and_then(|s| s.element_by_id_mut(eid))
        {
            *slot = element;
        }
        EventResult::Consumed
    }

    /// Select the next element on the slide, or the previous; the first (or
    /// last) when none is selected.
    ///
    /// Nothing selected an element but adding one, so nothing a layout put on
    /// a slide -- the first slide's title included -- could be selected, and
    /// so none of it could be typed into, moved or deleted.
    fn select_next_element(&mut self, forward: bool) -> EventResult {
        let Some(slide) = self.slides.get(self.current_index) else {
            return EventResult::Ignored;
        };
        let ids: Vec<ElementId> = slide.elements.iter().map(SlideElement::id).collect();
        if ids.is_empty() {
            return EventResult::Ignored;
        }
        let at = self
            .selected_element
            .and_then(|eid| ids.iter().position(|i| *i == eid));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => ids.len().saturating_sub(1),
            (Some(i), true) => i.saturating_add(1).checked_rem(ids.len()).unwrap_or(0),
            (Some(0), false) => ids.len().saturating_sub(1),
            (Some(i), false) => i.saturating_sub(1),
        };
        self.selected_element = ids.get(next).copied();
        EventResult::Consumed
    }

    /// An arrow on the selected element: move it, or with Shift resize it;
    /// with Ctrl by one unit instead of `NUDGE`.
    fn arrow_on_element(&mut self, key: Key, shift: bool, ctrl: bool) -> EventResult {
        let step = if ctrl { 1.0 } else { NUDGE };
        let (dx, dy) = match key {
            Key::Left => (-step, 0.0),
            Key::Right => (step, 0.0),
            Key::Up => (0.0, -step),
            Key::Down => (0.0, step),
            _ => return EventResult::Ignored,
        };
        self.change_selected(|e| {
            let (x, y, w, h) = e.bounds();
            if shift {
                let min = min_size(e);
                let (nw, nh) = ((w + dx).max(min), (h + dy).max(min));
                if (nw - w).abs() < f32::EPSILON && (nh - h).abs() < f32::EPSILON {
                    return false;
                }
                e.set_size(nw, nh);
            } else {
                let (nx, ny) = keep_on_slide(x + dx, y + dy, w, h);
                if (nx - x).abs() < f32::EPSILON && (ny - y).abs() < f32::EPSILON {
                    return false;
                }
                e.set_position(nx, ny);
            }
            true
        })
    }

    /// Bold, for the selected text box.
    fn toggle_bold(&mut self) -> EventResult {
        self.change_selected(|e| match e {
            SlideElement::TextBox { bold, .. } => {
                *bold = !*bold;
                true
            }
            _ => false,
        })
    }

    /// Centred, for the selected text box.
    fn toggle_centred(&mut self) -> EventResult {
        self.change_selected(|e| match e {
            SlideElement::TextBox { centered, .. } => {
                *centered = !*centered;
                true
            }
            _ => false,
        })
    }

    /// Larger or smaller text, or a thicker or thinner outline.
    fn step_weight(&mut self, up: bool) -> EventResult {
        self.change_selected(|e| match e {
            SlideElement::TextBox { font_size, .. }
            | SlideElement::BulletList { font_size, .. } => {
                let next = if up {
                    *font_size + 2.0
                } else {
                    *font_size - 2.0
                };
                let next = next.clamp(8.0, 120.0);
                let changed = (next - *font_size).abs() > f32::EPSILON;
                *font_size = next;
                changed
            }
            SlideElement::Shape { stroke_width, .. } => {
                let next = if up {
                    *stroke_width + 1.0
                } else {
                    *stroke_width - 1.0
                };
                let next = next.clamp(0.0, 20.0);
                let changed = (next - *stroke_width).abs() > f32::EPSILON;
                *stroke_width = next;
                changed
            }
            SlideElement::Image { .. } => false,
        })
    }

    /// The colours `C` steps through: the theme's roles, then `EXTRA_COLOURS`.
    fn colour_choices(&self) -> Vec<Color> {
        let mut out = vec![
            self.theme.title_color,
            self.theme.subtitle_color,
            self.theme.body_color,
            self.theme.accent,
        ];
        for c in EXTRA_COLOURS {
            if !out.contains(&c) {
                out.push(c);
            }
        }
        out
    }

    /// The selected element's next colour. A shape's outline follows its
    /// fill, which is how every shape here is made.
    fn cycle_element_colour(&mut self) -> EventResult {
        let choices = self.colour_choices();
        let next = |c: Color| {
            let at = choices.iter().position(|x| *x == c);
            let i = at.map_or(0, |i| {
                i.saturating_add(1).checked_rem(choices.len()).unwrap_or(0)
            });
            choices.get(i).copied().unwrap_or(c)
        };
        self.change_selected(|e| match e {
            SlideElement::TextBox { color, .. } | SlideElement::BulletList { color, .. } => {
                *color = next(*color);
                true
            }
            SlideElement::Shape {
                fill_color,
                stroke_color,
                ..
            } => {
                let c = next(*fill_color);
                *fill_color = c;
                *stroke_color = c;
                true
            }
            SlideElement::Image { .. } => false,
        })
    }

    /// The current slide's next background, from `BACKGROUNDS`.
    ///
    /// A slide's background was printed in the property panel as a hex value
    /// and a swatch, and nothing could change it.
    fn cycle_background(&mut self) -> EventResult {
        let Some(now) = self.slides.get(self.current_index).map(|s| s.background) else {
            return EventResult::Ignored;
        };
        let at = BACKGROUNDS.iter().position(|b| *b == now);
        let i = at.map_or(0, |i| {
            i.saturating_add(1)
                .checked_rem(BACKGROUNDS.len())
                .unwrap_or(0)
        });
        let next = BACKGROUNDS.get(i).copied().flatten();
        self.checkpoint();
        if let Some(slide) = self.slides.get_mut(self.current_index) {
            slide.background = next;
        }
        EventResult::Consumed
    }

    /// Add what Insert button `index` names.
    fn insert(&mut self, index: usize) -> EventResult {
        match INSERT_KINDS.get(index).copied() {
            Some("Text Box") => self.add_textbox(),
            Some("Rectangle") => self.add_shape(ShapeKind::Rectangle),
            Some("Ellipse") => self.add_shape(ShapeKind::Ellipse),
            Some("Line") => self.add_shape(ShapeKind::Line),
            Some("Arrow") => self.add_shape(ShapeKind::Arrow),
            Some("Image") => self.add_image_placeholder(),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Add a slide in layout `index` of `SlideLayout::all`, and close the menu.
    fn choose_layout(&mut self, index: usize) -> EventResult {
        self.layout_menu = None;
        let Some(&layout) = SlideLayout::all().get(index) else {
            return EventResult::Consumed;
        };
        self.add_slide(layout);
        self.keep_current_visible();
        EventResult::Consumed
    }

    /// Move to the next theme in the set.
    ///
    /// `set_theme` had no caller, so the deck was permanently on Mocha and the
    /// window's "Theme: Mocha" was a label that could not say anything else.
    /// `SlideTheme::light` and `SlideTheme::vibrant` existed and nothing could
    /// ask for them.
    pub fn cycle_theme(&mut self) {
        let next = match self.theme.name.as_str() {
            "Mocha" => SlideTheme::light(),
            "Light" => SlideTheme::vibrant(),
            _ => SlideTheme::mocha(&self.palette),
        };
        self.set_theme(next);
    }

    /// Move the current slide to the next transition.
    ///
    /// `set_current_transition` had no caller either, while the window drew
    /// "Transition: ..." twice -- once on the slide and once in the property
    /// panel. A value printed in a property panel is an offer, not a status.
    pub fn cycle_transition(&mut self) {
        let next = match self.current_transition() {
            Transition::None => Transition::Fade,
            Transition::Fade => Transition::SlideLeft,
            Transition::SlideLeft => Transition::SlideRight,
            Transition::SlideRight => Transition::Wipe,
            Transition::Wipe => Transition::Dissolve,
            Transition::Dissolve => Transition::None,
        };
        self.set_current_transition(next);
    }

    /// The transition the current slide uses.
    fn current_transition(&self) -> Transition {
        self.slides
            .get(self.current_index)
            .map_or(Transition::None, |s| s.transition)
    }

    /// Its doc said it re-applied the theme to every slide; it replaced the
    /// theme and nothing else. An element's colours are copied from the theme
    /// when it is made, so moving from Mocha to Light put Mocha's pale text on
    /// Light's pale background. Now every colour and size that came from the
    /// old theme's roles moves to the new theme's; anything the user chose
    /// stays.
    pub fn set_theme(&mut self, theme: SlideTheme) {
        self.checkpoint();
        let old = std::mem::replace(&mut self.theme, theme);
        let new = &self.theme;
        let colour = |c: Color| {
            if c == old.title_color {
                new.title_color
            } else if c == old.subtitle_color {
                new.subtitle_color
            } else if c == old.body_color {
                new.body_color
            } else if c == old.accent {
                new.accent
            } else {
                c
            }
        };
        let size = |s: f32| {
            if (s - old.title_size).abs() < f32::EPSILON {
                new.title_size
            } else if (s - old.subtitle_size).abs() < f32::EPSILON {
                new.subtitle_size
            } else if (s - old.body_size).abs() < f32::EPSILON {
                new.body_size
            } else if (s - old.bullet_size).abs() < f32::EPSILON {
                new.bullet_size
            } else {
                s
            }
        };
        for slide in &mut self.slides {
            for element in &mut slide.elements {
                match element {
                    SlideElement::TextBox {
                        color, font_size, ..
                    }
                    | SlideElement::BulletList {
                        color, font_size, ..
                    } => {
                        *color = colour(*color);
                        *font_size = size(*font_size);
                    }
                    SlideElement::Shape {
                        fill_color,
                        stroke_color,
                        ..
                    } => {
                        *fill_color = colour(*fill_color);
                        *stroke_color = colour(*stroke_color);
                    }
                    SlideElement::Image { .. } => {}
                }
            }
        }
    }

    // ---- Transition --------------------------------------------------------

    /// Set the transition for the current slide.
    pub fn set_current_transition(&mut self, transition: Transition) {
        self.checkpoint();
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
        html.push_str(
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n",
        );
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
                    SlideElement::TextBox {
                        x,
                        y,
                        width,
                        height,
                        text,
                        font_size,
                        color,
                        bold,
                        centered,
                        ..
                    } => {
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
                    SlideElement::Shape {
                        kind,
                        x,
                        y,
                        width,
                        height,
                        fill_color,
                        stroke_color,
                        stroke_width,
                        ..
                    } => {
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
                    SlideElement::Image {
                        x,
                        y,
                        width,
                        height,
                        placeholder_label,
                        ..
                    } => {
                        // The label is escaped like every other text field
                        // here. It was the one that was not, and the reason is
                        // worth a comment: the other three -- the title, a
                        // text box, a bullet item -- each sit in their own
                        // `push_html_escaped` call, whereas this one was a
                        // `{}` among five geometry values in a larger
                        // `format!`, where an unescaped interpolation does not
                        // look out of place next to `{x}` and `{width}`.
                        // `placeholder_label` is a public field of a public
                        // enum, and the variant's own doc says real images
                        // will reference an asset store -- i.e. this becomes a
                        // filename.
                        html.push_str(&format!(
                            "  <div class=\"img-placeholder\" style=\"left:{x}px;top:{y}px;width:{width}px;\
                             height:{height}px;\">",
                        ));
                        push_html_escaped(&mut html, placeholder_label);
                        html.push_str("</div>\n");
                    }
                    SlideElement::BulletList {
                        x,
                        y,
                        width,
                        height,
                        items,
                        font_size,
                        color,
                        ..
                    } => {
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

    /// The identity of the slide at `index`, for tests that need to tell
    /// reordering from paging.
    #[cfg(test)]
    fn slide_id_at(&self, index: usize) -> Option<SlideId> {
        self.slides.get(index).map(|s| s.id)
    }

    /// Fill the deck with the slides a first run shows.
    ///
    /// This was the body of `main`, and it is a method so that a test can check
    /// the presentation does not open empty — a test cannot call `main`, so a
    /// seed that lives there is a blind spot. One of each layout, so every
    /// layout the renderer knows how to draw appears somewhere.
    #[cfg(test)]
    pub fn seed_sample_deck(&mut self) {
        for layout in [
            SlideLayout::TitleContent,
            SlideLayout::SectionHeader,
            SlideLayout::TwoColumn,
            SlideLayout::ImageCaption,
            SlideLayout::Blank,
        ] {
            self.add_slide(layout);
        }
        self.go_to_slide(0);
        self.set_current_notes(String::from("Welcome the audience. Introduce the topic."));
    }

    // ------------------------------------------------------------------
    // Events
    // ------------------------------------------------------------------

    /// Route a compositor event into the app.
    /// Ask where to put the exported presentation.
    pub fn export_as(&mut self) {
        self.picker.open_to_write("presentation.html");
    }

    /// Write the presentation to `path`, and say what happened.
    pub fn write_html(&mut self, path: &std::path::Path) -> String {
        let text = self.export_html();
        match safeio::write_str_atomically(path, &text) {
            Ok(()) => format!(
                "Exported {} slide(s) to {}",
                self.slides.len(),
                path.display()
            ),
            Err(err) => format!("Could not write {}: {err}", path.display()),
        }
    }

    pub fn handle_event(&mut self, event: &Event) -> EventResult {
        // The picker takes input first while it is up, or a filename is typed
        // into the slide behind it.
        match self
            .picker
            .handle(event, self.window_width, self.window_height)
        {
            Picked::Chose(path) => {
                self.status_message = Some(self.write_html(&path));
                return EventResult::Consumed;
            }
            Picked::Handled | Picked::Cancelled => return EventResult::Consumed,
            Picked::Ignored => {}
        }
        match event {
            Event::Key(key_ev) => self.handle_key(key_ev),
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::Resize { width, height } => {
                #[allow(
                    clippy::cast_precision_loss,
                    reason = "a window dimension is far below f32's integer-exact range"
                )]
                {
                    self.window_width = *width as f32;
                    self.window_height = *height as f32;
                }
                // Not `Consumed`: a resize is not by itself a reason to redraw.
                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    /// Apply a key press.
    ///
    /// There is no presenting branch because there is no presenting view:
    /// `ViewMode` has `Edit` and `Sorter` and nothing else, so this program
    /// edits a deck and cannot show one. That is the largest thing missing
    /// from it, and it is a feature to build rather than a defect to fix.
    pub fn handle_key(&mut self, key: &KeyEvent) -> EventResult {
        if !key.pressed {
            return EventResult::Ignored;
        }
        // Typing into a box takes every key while it is up, or a title
        // containing `s` would drop a rectangle on the slide behind it.
        if self.editing.is_some() {
            return self.handle_editing_key(key);
        }
        let ctrl = key.modifiers.ctrl;
        let shift = key.modifiers.shift;
        // The shortcut list is modal: a key behind it would change a slide
        // nobody can see. What raised it puts it away, and so does Escape.
        if self.show_help {
            let closes =
                matches!(key.key, Key::F1 | Key::Escape) || (key.key == Key::Slash && shift);
            if closes {
                self.show_help = false;
                return EventResult::Consumed;
            }
            return EventResult::Ignored;
        }
        if let Some(reached) = self.layout_menu {
            return self.handle_layout_menu_key(key, reached);
        }
        let result = self.handle_command_key(key, ctrl, shift);
        if result == EventResult::Consumed {
            self.keep_current_visible();
        }
        result
    }

    /// The keys of the window itself, with no box, list or menu up.
    fn handle_command_key(&mut self, key: &KeyEvent, ctrl: bool, shift: bool) -> EventResult {
        let editing_view = self.view == ViewMode::Edit;
        match key.key {
            // The shortcut list. `F1` raises it in every app in this tree,
            // including `apps/spreadsheet`, where `?` is a character the
            // program has to be able to type into a cell -- so somebody who
            // has learned one key is never stuck. `?` as well, wherever the
            // program is not obliged to type one.
            Key::F1 => {
                self.show_help = true;
                EventResult::Consumed
            }
            Key::Slash if shift => {
                self.show_help = true;
                EventResult::Consumed
            }
            // The one key that lets a deck leave this window.
            Key::E if ctrl => {
                self.export_as();
                EventResult::Consumed
            }
            // Undo and redo. The undo stack was kept by every change and read
            // by nothing: no key reached `undo` or `redo`, and the toolbar's
            // two words lit up and could not be pressed.
            Key::Z if ctrl && shift => self.redo_if_any(),
            Key::Z if ctrl => self.undo_if_any(),
            Key::Y if ctrl => self.redo_if_any(),
            // Views.
            Key::Num1 => self.set_view(ViewMode::Edit),
            Key::Num2 => self.set_view(ViewMode::Sorter),
            // In the sorter, Enter opens the slide it is on.
            Key::Enter if self.view == ViewMode::Sorter => self.set_view(ViewMode::Edit),
            // Reordering, before the plain paging keys below: a guard narrows
            // only the arm it is on, so an unguarded `Key::PageUp` listed first
            // swallows the Ctrl case entirely — Ctrl+PageUp would page back
            // rather than move the slide.
            Key::PageUp if ctrl => {
                self.move_slide_up();
                EventResult::Consumed
            }
            Key::PageDown if ctrl => {
                self.move_slide_down();
                EventResult::Consumed
            }
            // Moving through the deck.
            Key::PageDown => self.advance(1),
            Key::PageUp => self.advance(-1),
            Key::Home => self.jump_to(0),
            Key::End => self.jump_to(self.slides.len().saturating_sub(1)),
            // The elements on the slide. Tab was the second key for switching
            // views, beside `1` and `2`; it walks the elements now, as it does
            // in every other slide editor, because nothing else reached them.
            Key::Tab if editing_view => self.select_next_element(!shift),
            Key::Escape if self.selected_element.is_some() => {
                self.selected_element = None;
                EventResult::Consumed
            }
            // With an element selected the arrows move it (Shift resizes),
            // which is what they do in any slide editor; with none, Left and
            // Right still change slides.
            Key::Left | Key::Right | Key::Up | Key::Down
                if editing_view && self.selected_element.is_some() =>
            {
                self.arrow_on_element(key.key, shift, ctrl)
            }
            Key::Right => self.advance(1),
            Key::Left => self.advance(-1),
            // Editing the deck.
            Key::N if ctrl => {
                self.add_slide(SlideLayout::TitleContent);
                EventResult::Consumed
            }
            // Every other layout. Ctrl+N made a Title + Content slide and
            // nothing made any other kind, so five of the six layouts were
            // reachable only as the sample deck `main` opened with.
            Key::M if ctrl => {
                self.layout_menu = Some(1);
                EventResult::Consumed
            }
            Key::D if ctrl => {
                self.duplicate_current_slide();
                EventResult::Consumed
            }
            // `Delete` removes the selected element if there is one, and the
            // slide otherwise. `delete_selected_element` had no caller, so the
            // only way to remove a shape or a textbox was to delete the slide
            // around it -- and erring towards the element is the safe half of
            // the ambiguity, since re-adding an element is cheap and re-making
            // a slide is not. `Shift+Delete` always means the slide.
            Key::Delete if self.selected_element.is_some() && !shift => {
                self.delete_selected_element();
                EventResult::Consumed
            }
            Key::Delete => self.delete_current_slide(),
            // Before the plain `C`, which would otherwise take these.
            Key::C if ctrl && shift => self.toggle_centred(),
            Key::C if ctrl => {
                self.copy_slide();
                EventResult::Consumed
            }
            Key::V if ctrl => {
                let before = self.slides.len();
                self.paste_slide();
                if self.slides.len() == before {
                    EventResult::Ignored
                } else {
                    EventResult::Consumed
                }
            }
            Key::B if ctrl => self.toggle_bold(),
            Key::RightBracket if ctrl => self.step_weight(true),
            Key::LeftBracket if ctrl => self.step_weight(false),
            // Before the unguarded `Key::T` below, which would otherwise
            // take Ctrl+T and add a textbox. The deck's look and the slide's
            // transition are both already printed in the window.
            // Before the theme arm, which has no shift guard and would
            // otherwise take this and cycle the theme instead.
            Key::T if ctrl && shift => self.begin_deck_title(),
            Key::T if ctrl => {
                self.cycle_theme();
                EventResult::Consumed
            }
            Key::R if ctrl => {
                self.cycle_transition();
                EventResult::Consumed
            }
            Key::T => {
                self.add_textbox();
                EventResult::Consumed
            }
            // Writing in the selected box. `F2` is the conventional rename
            // key and `Enter` is what opens a thing; both are free here.
            Key::Enter | Key::F2 => self.begin_editing(),
            // The rest of what a slide can hold. `add_shape` and
            // `add_image_placeholder` had no callers, so `T` was the only
            // thing that could put anything on a slide: this program made
            // decks of textboxes.
            //
            // A key per shape rather than a mode with an armed kind: there are
            // four, they are all mnemonic, and a mode would need its own label
            // on screen to say which kind the next `S` would produce.
            Key::S if !ctrl => {
                self.add_shape(ShapeKind::Rectangle);
                EventResult::Consumed
            }
            Key::O if !ctrl => {
                self.add_shape(ShapeKind::Ellipse);
                EventResult::Consumed
            }
            Key::L => {
                self.add_shape(ShapeKind::Line);
                EventResult::Consumed
            }
            Key::A => {
                self.add_shape(ShapeKind::Arrow);
                EventResult::Consumed
            }
            Key::I => {
                self.add_image_placeholder();
                EventResult::Consumed
            }
            Key::C => self.cycle_element_colour(),
            Key::G => self.cycle_background(),
            Key::N => self.begin_notes(),
            Key::B => {
                self.show_notes = !self.show_notes;
                EventResult::Consumed
            }
            _ => EventResult::Ignored,
        }
    }

    /// Keys while the new-slide menu is up: a layout by its arrow and Enter
    /// or by its number, and Escape to close it.
    fn handle_layout_menu_key(&mut self, key: &KeyEvent, reached: usize) -> EventResult {
        let last = SlideLayout::all().len().saturating_sub(1);
        let number = match key.key {
            Key::Num1 => Some(0),
            Key::Num2 => Some(1),
            Key::Num3 => Some(2),
            Key::Num4 => Some(3),
            Key::Num5 => Some(4),
            Key::Num6 => Some(5),
            _ => None,
        };
        if let Some(index) = number {
            return self.choose_layout(index);
        }
        match key.key {
            Key::Escape => self.layout_menu = None,
            Key::Up => self.layout_menu = Some(reached.saturating_sub(1)),
            Key::Down => self.layout_menu = Some(reached.saturating_add(1).min(last)),
            Key::Enter => return self.choose_layout(reached),
            _ => return EventResult::Ignored,
        }
        EventResult::Consumed
    }

    /// Delete the current slide, unless it is the last one.
    fn delete_current_slide(&mut self) -> EventResult {
        if self.slides.len() < 2 {
            // Refusing to delete the last slide rather than leaving an empty
            // deck with nothing to draw or select.
            return EventResult::Ignored;
        }
        let index = self.current_index;
        self.delete_slide(index);
        EventResult::Consumed
    }

    fn undo_if_any(&mut self) -> EventResult {
        if !self.undo_mgr.can_undo() {
            return EventResult::Ignored;
        }
        self.undo();
        EventResult::Consumed
    }

    fn redo_if_any(&mut self) -> EventResult {
        if !self.undo_mgr.can_redo() {
            return EventResult::Ignored;
        }
        self.redo();
        EventResult::Consumed
    }

    /// Switch views, reporting whether anything changed.
    fn set_view(&mut self, view: ViewMode) -> EventResult {
        if self.view == view {
            return EventResult::Ignored;
        }
        self.view = view;
        EventResult::Consumed
    }

    /// Step forward or back through the deck, stopping at the ends.
    ///
    /// Stopping rather than wrapping: a presentation that jumps from the last
    /// slide back to the title is one the speaker has to notice and undo in
    /// front of the room.
    fn advance(&mut self, delta: isize) -> EventResult {
        let Ok(current) = isize::try_from(self.current_index) else {
            return EventResult::Ignored;
        };
        let Some(moved) = current.checked_add(delta) else {
            return EventResult::Ignored;
        };
        let Ok(moved) = usize::try_from(moved) else {
            return EventResult::Ignored;
        };
        self.jump_to(moved)
    }

    /// Show a particular slide, reporting whether the view moved.
    fn jump_to(&mut self, index: usize) -> EventResult {
        if index >= self.slides.len() || index == self.current_index {
            return EventResult::Ignored;
        }
        self.go_to_slide(index);
        EventResult::Consumed
    }

    /// Named `render_commands` and not `render`: at equal arity an inherent
    /// method silently wins method lookup over `oswindow::app::App::render`, so
    /// an app that keeps the name draws nothing and reports no error.
    ///
    /// Renders the full application UI and returns the list of draw commands.
    pub fn render_commands(&self) -> Vec<RenderCommand> {
        self.frame().into_tree().commands
    }

    /// Draw the window, recording every control where it is drawn: both the
    /// picture and the hit test.
    pub fn frame(&self) -> Frame<Target> {
        let (w, h) = (self.window_width, self.window_height);
        let mut f = Frame::new(w, h);

        // Background fill the entire window.
        self.palette
            .push_surface(&mut f, 0.0, 0.0, w, h, 0.0, Surface::Card);

        match self.view {
            ViewMode::Edit => self.render_edit_mode(&mut f),
            ViewMode::Sorter => self.render_sorter_mode(&mut f),
        }
        if let Some(reached) = self.layout_menu {
            self.render_layout_menu(&mut f, reached);
        }

        // The picker over the slide rather than under it.
        f.extend(self.picker.render(&self.palette, w, h));

        // And the shortcut list over everything, because it is the one thing
        // a reader asked for explicitly. Modal: a press anywhere puts it away.
        if self.show_help {
            guitk::shortcut::render_card(
                &mut f,
                &self.palette,
                (w, h),
                TOOLBAR_HEIGHT,
                SHORTCUTS,
                "F1 or ? closes this",
            );
            f.hit(Target::HelpCard, Rect::new(0.0, 0.0, w, h));
        }
        f
    }

    /// A button, lit while the pointer is on it. One with nothing to do is
    /// drawn dim and records no hit box, so it cannot be pressed.
    fn button(
        &self,
        f: &mut Frame<Target>,
        rect: Rect,
        label: &str,
        target: Target,
        enabled: bool,
    ) {
        let lit = enabled && self.hover == Some(target);
        f.push(RenderCommand::FillRect {
            x: rect.x,
            y: rect.y,
            width: rect.w,
            height: rect.h,
            color: if lit {
                self.palette.surface1
            } else {
                self.palette.surface0
            },
            corner_radii: CornerRadii::all(CORNER_R),
        });
        f.push(RenderCommand::Text {
            x: rect.x + 8.0,
            y: rect.y + (rect.h - 12.0) / 2.0,
            text: label.to_string(),
            color: if enabled {
                self.palette.text
            } else {
                self.palette.overlay0
            },
            font_size: 12.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some((rect.w - 16.0).max(0.0)),
            overflow: TextOverflow::Ellipsis,
        });
        if enabled {
            f.hit(target, rect);
        }
    }

    /// Render the toolbar at the top.
    ///
    /// Its five buttons, and the Undo and Redo beside them, were drawn and
    /// could not be pressed; the Undo and Redo lit up when there was
    /// something to undo and nothing could reach it.
    fn render_toolbar(&self, f: &mut Frame<Target>) {
        self.palette.push_surface(
            f,
            0.0,
            0.0,
            self.window_width,
            TOOLBAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Bottom),
        );
        f.push(RenderCommand::Line {
            x1: 0.0,
            y1: TOOLBAR_HEIGHT,
            x2: self.window_width,
            y2: TOOLBAR_HEIGHT,
            color: self.palette.surface0,
            width: 1.0,
        });

        // The deck's name, which a press lets you change (Ctrl+Shift+T).
        let title = Rect::new(6.0, 6.0, 202.0, 28.0);
        if self.hover == Some(Target::Tool(Tool::Title)) {
            f.push(RenderCommand::FillRect {
                x: title.x,
                y: title.y,
                width: title.w,
                height: title.h,
                color: self.palette.surface0,
                corner_radii: CornerRadii::all(CORNER_R),
            });
        }
        f.push(RenderCommand::Text {
            x: 12.0,
            y: 12.0,
            text: self.title.clone(),
            color: self.palette.text,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(192.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.hit(Target::Tool(Tool::Title), title);

        let other_view = match self.view {
            ViewMode::Edit => "Sorter",
            ViewMode::Sorter => "Edit",
        };
        let theme = format!("Theme: {}", self.theme.name);
        let tools = [
            (Tool::NewSlide, "+ Slide", 66.0, true),
            (Tool::Duplicate, "Duplicate", 80.0, true),
            (Tool::DeleteSlide, "Delete", 62.0, self.slides.len() > 1),
            (Tool::ToggleView, other_view, 62.0, true),
            (Tool::Undo, "Undo", 54.0, self.undo_mgr.can_undo()),
            (Tool::Redo, "Redo", 54.0, self.undo_mgr.can_redo()),
            (Tool::Export, "Export", 62.0, true),
            (Tool::Theme, theme.as_str(), 128.0, true),
        ];
        let mut x = TOOLS_X;
        for (tool, label, w, enabled) in tools {
            self.button(
                f,
                Rect::new(x, 6.0, w, 28.0),
                label,
                Target::Tool(tool),
                enabled,
            );
            x += w + 6.0;
        }
    }

    /// Render the status bar at the bottom.
    fn render_status_bar(&self, f: &mut Frame<Target>) {
        let y = self.window_height - STATUS_BAR_HEIGHT;
        self.palette.push_surface(
            f,
            0.0,
            y,
            self.window_width,
            STATUS_BAR_HEIGHT,
            0.0,
            Surface::Strip(Edge::Top),
        );
        f.push(RenderCommand::Line {
            x1: 0.0,
            y1: y,
            x2: self.window_width,
            y2: y,
            color: self.palette.surface0,
            width: 1.0,
        });

        // While typing, this line says so instead of counting slides. The
        // count is still in the window bar; the mode is nowhere else, and the
        // shape keys are captured in here -- so somebody pressing `S` for a
        // rectangle gets an "S" in their text with nothing to explain it.
        let slide_pos = match &self.editing {
            Some((EditTarget::DeckTitle, _)) => {
                String::from("Naming the deck -- Enter or Esc to finish")
            }
            Some((EditTarget::Notes, _)) => {
                String::from("Typing the notes -- Shift+Enter for a new line, Esc to finish")
            }
            Some((EditTarget::Element(_), _)) => {
                String::from("Typing -- Shift+Enter for a new line, Esc to finish")
            }
            None => format!(
                "Slide {} of {}",
                self.current_index.saturating_add(1),
                self.slides.len(),
            ),
        };
        f.push(RenderCommand::Text {
            x: 12.0,
            y: y + 5.0,
            text: slide_pos,
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(320.0),
            overflow: TextOverflow::Ellipsis,
        });

        if self.editing.is_none()
            && let Some(slide) = self.slides.get(self.current_index)
        {
            let trans = format!("Transition: {} (Ctrl+R)", slide.transition.label());
            f.push(RenderCommand::Text {
                x: 200.0,
                y: y + 5.0,
                text: trans,
                color: self.palette.subtext0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });

            let layout_info = format!("Layout: {}", slide.layout.label());
            f.push(RenderCommand::Text {
                x: 400.0,
                y: y + 5.0,
                text: layout_info,
                color: self.palette.subtext0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
        }

        let view_label = match self.view {
            ViewMode::Edit => "Edit Mode",
            ViewMode::Sorter => "Sorter View",
        };
        f.push(RenderCommand::Text {
            x: self.window_width - 120.0,
            y: y + 5.0,
            text: view_label.to_string(),
            color: self.palette.ink(self.palette.teal),
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    // ---- Edit mode rendering -----------------------------------------------

    /// Render the full edit mode: toolbar, sidebar, canvas, properties, notes, status.
    fn render_edit_mode(&self, f: &mut Frame<Target>) {
        self.render_toolbar(f);
        self.render_sidebar(f);
        self.render_canvas(f);
        self.render_properties_panel(f);
        if self.show_notes {
            self.render_notes_panel(f);
        }
        self.render_status_bar(f);
    }

    /// The thumbnail column: its pane, a thumbnail's width and height, and
    /// the pitch from one thumbnail to the next.
    fn sidebar_pane(&self) -> (Rect, f32, f32, f32) {
        let top = TOOLBAR_HEIGHT;
        let bottom = self.window_height - STATUS_BAR_HEIGHT;
        let thumb_w = SIDEBAR_WIDTH - THUMBNAIL_PAD * 2.0;
        let thumb_h = thumb_w / SLIDE_ASPECT;
        let pitch = thumb_h + THUMBNAIL_PAD + 20.0;
        (
            Rect::new(0.0, top, SIDEBAR_WIDTH, (bottom - top).max(0.0)),
            thumb_w,
            thumb_h,
            pitch,
        )
    }

    /// How far the thumbnail column can scroll.
    fn sidebar_limit(&self) -> f32 {
        let (pane, _, _, pitch) = self.sidebar_pane();
        (self.slides.len() as f32 * pitch + THUMBNAIL_PAD - pane.h).max(0.0)
    }

    /// Render the slide thumbnail sidebar.
    ///
    /// It clipped its thumbnails and never scrolled, so from the sixth slide
    /// on a slide's thumbnail -- the current one's included -- was drawn
    /// below the window.
    fn render_sidebar(&self, f: &mut Frame<Target>) {
        let (pane, thumb_w, thumb_h, pitch) = self.sidebar_pane();
        f.push(RenderCommand::FillRect {
            x: pane.x,
            y: pane.y,
            width: pane.w,
            height: pane.h,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        f.push(RenderCommand::Line {
            x1: SIDEBAR_WIDTH,
            y1: pane.y,
            x2: SIDEBAR_WIDTH,
            y2: pane.bottom(),
            color: self.palette.surface0,
            width: 1.0,
        });
        f.hit(Target::Sidebar, pane);
        f.clip(pane);

        for (i, slide) in self.slides.iter().enumerate() {
            let ty = pane.y + THUMBNAIL_PAD + (i as f32) * pitch - self.sidebar_scroll;
            let cell = Rect::new(THUMBNAIL_PAD - 2.0, ty - 2.0, thumb_w + 4.0, thumb_h + 20.0);
            if f.visible_part(cell).is_none() {
                continue;
            }
            let is_current = i == self.current_index;
            if is_current || self.hover == Some(Target::Thumb(i)) {
                f.push(RenderCommand::StrokeRect {
                    x: THUMBNAIL_PAD - 2.0,
                    y: ty - 2.0,
                    width: thumb_w + 4.0,
                    height: thumb_h + 4.0,
                    color: if is_current {
                        self.palette.blue
                    } else {
                        self.palette.surface2
                    },
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
            }
            let bg = slide.effective_bg(&self.theme);
            f.push(RenderCommand::FillRect {
                x: THUMBNAIL_PAD,
                y: ty,
                width: thumb_w,
                height: thumb_h,
                color: bg,
                corner_radii: CornerRadii::all(CORNER_R),
            });
            let preview = slide_preview_text(slide);
            if !preview.is_empty() {
                f.push(RenderCommand::Text {
                    x: THUMBNAIL_PAD + 4.0,
                    y: ty + 8.0,
                    text: preview,
                    color: self.palette.text,
                    font_size: 8.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(thumb_w - 8.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            f.push(RenderCommand::Text {
                x: THUMBNAIL_PAD,
                y: ty + thumb_h + 2.0,
                text: format!("Slide {}", i.saturating_add(1)),
                color: if is_current {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_size: 10.0,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            // A press goes to the slide; a drag moves it.
            f.hit(Target::Thumb(i), cell);
        }
        f.unclip();
    }

    /// The grey area the slide sits in.
    fn canvas_area(&self) -> Rect {
        let top = TOOLBAR_HEIGHT;
        let notes_h = if self.show_notes { NOTES_HEIGHT } else { 0.0 };
        Rect::new(
            SIDEBAR_WIDTH,
            top,
            (self.window_width - SIDEBAR_WIDTH - PROPERTIES_WIDTH).max(0.0),
            (self.window_height - top - STATUS_BAR_HEIGHT - notes_h).max(0.0),
        )
    }

    /// Where the slide is drawn in the edit view: its top-left corner in the
    /// window, and the scale from slide units (960 by 540) to pixels.
    ///
    /// The drawing and every pointer conversion read this, so a press and the
    /// picture cannot disagree about where an element is.
    fn canvas_geometry(&self) -> (f32, f32, f32) {
        let area = self.canvas_area();
        let scale = ((area.w - 40.0) / SLIDE_W)
            .min((area.h - 40.0) / SLIDE_H)
            .max(0.1);
        let cx = area.x + (area.w - SLIDE_W * scale) / 2.0;
        let cy = area.y + (area.h - SLIDE_H * scale) / 2.0;
        (cx, cy, scale)
    }

    /// A window point in slide units.
    fn to_slide(&self, x: f32, y: f32) -> (f32, f32) {
        let (cx, cy, scale) = self.canvas_geometry();
        ((x - cx) / scale, (y - cy) / scale)
    }

    /// An element's box in the window, widened to a few pixels when it is a
    /// flat line: an empty box records no hit, and a horizontal line has no
    /// height.
    fn element_rect(&self, element: &SlideElement) -> Rect {
        let (cx, cy, scale) = self.canvas_geometry();
        let (x, y, w, h) = element.bounds();
        let mut r = Rect::new(cx + x * scale, cy + y * scale, w * scale, h * scale);
        if r.w < 6.0 {
            r.x -= (6.0 - r.w) / 2.0;
            r.w = 6.0;
        }
        if r.h < 6.0 {
            r.y -= (6.0 - r.h) / 2.0;
            r.h = 6.0;
        }
        r
    }

    /// The four corner handles of a box, in the window.
    fn handles(r: Rect) -> [(Corner, Rect); 4] {
        let at = |x: f32, y: f32| Rect::new(x - HANDLE / 2.0, y - HANDLE / 2.0, HANDLE, HANDLE);
        [
            (Corner::TopLeft, at(r.x, r.y)),
            (Corner::TopRight, at(r.right(), r.y)),
            (Corner::BottomLeft, at(r.x, r.bottom())),
            (Corner::BottomRight, at(r.right(), r.bottom())),
        ]
    }

    /// Render the main slide canvas in the center.
    fn render_canvas(&self, f: &mut Frame<Target>) {
        let area = self.canvas_area();
        let (cx, cy, scale) = self.canvas_geometry();
        let disp_w = SLIDE_W * scale;
        let disp_h = SLIDE_H * scale;

        f.push(RenderCommand::FillRect {
            x: area.x,
            y: area.y,
            width: area.w,
            height: area.h,
            color: self.palette.mantle,
            corner_radii: CornerRadii::ZERO,
        });
        // A press on the grey selects nothing.
        f.hit(Target::Canvas, area);
        f.push(RenderCommand::BoxShadow {
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

        let Some(slide) = self.slides.get(self.current_index) else {
            return;
        };
        let slide_rect = Rect::new(cx, cy, disp_w, disp_h);
        f.push(RenderCommand::FillRect {
            x: cx,
            y: cy,
            width: disp_w,
            height: disp_h,
            color: slide.effective_bg(&self.theme),
            corner_radii: CornerRadii::all(CORNER_R),
        });
        f.hit(Target::SlideArea, slide_rect);

        // The elements, clipped to the slide, each where it is drawn. Later
        // elements are drawn over earlier ones, and their boxes are recorded
        // after, so a press takes the one on top.
        f.clip(slide_rect);
        for element in &slide.elements {
            self.render_element(f, element, cx, cy, scale);
            f.hit(Target::Element(element.id()), self.element_rect(element));
        }
        f.unclip();

        // The selection, with a handle at each corner for resizing.
        if let Some(element) = self.selected() {
            let (ex, ey, ew, eh) = element.bounds();
            let outline = Rect::new(cx + ex * scale, cy + ey * scale, ew * scale, eh * scale);
            f.push(RenderCommand::StrokeRect {
                x: outline.x - 1.0,
                y: outline.y - 1.0,
                width: outline.w + 2.0,
                height: outline.h + 2.0,
                color: self.palette.sky,
                line_width: 1.5,
                corner_radii: CornerRadii::ZERO,
            });
            if self.editing.is_none() {
                for (corner, handle) in Self::handles(outline) {
                    f.push(RenderCommand::FillRect {
                        x: handle.x,
                        y: handle.y,
                        width: handle.w,
                        height: handle.h,
                        color: if self.hover == Some(Target::Handle(corner)) {
                            self.palette.blue
                        } else {
                            self.palette.sky
                        },
                        corner_radii: CornerRadii::ZERO,
                    });
                    f.hit(Target::Handle(corner), handle);
                }
            }
        }

        f.push(RenderCommand::Text {
            x: cx + disp_w - 50.0,
            y: cy + disp_h - 18.0,
            text: format!(
                "{}/{}",
                self.current_index.saturating_add(1),
                self.slides.len(),
            ),
            color: self.palette.subtext0,
            font_size: 10.0,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
    }

    /// `text`'s lines in a box `width` wide: one per line break, and a line
    /// too wide for the box wrapped at its words.
    ///
    /// A text box was drawn as one `Text` command, which draws one line: a
    /// box typed with Shift+Enter showed its lines run together, and a long
    /// line was cut with an ellipsis instead of wrapping.
    fn box_lines(text: &str, width: f32, size: f32, weight: FontWeightHint) -> Vec<String> {
        let mut out = Vec::new();
        for line in text.split('\n') {
            if line.is_empty() || guitk::text::measure(line, size, weight) <= width {
                out.push(line.to_owned());
            } else {
                out.extend(guitk::text::wrap(line, width, size, weight));
            }
        }
        out
    }

    /// Render a single slide element at the given offset and scale.
    fn render_element(
        &self,
        f: &mut Frame<Target>,
        elem: &SlideElement,
        ox: f32,
        oy: f32,
        scale: f32,
    ) {
        // While an element is being typed into, it draws the buffer and not
        // what it holds: the commit happens when the mode is left, so drawing
        // the element would leave the user typing at a slide that never
        // changes.
        let typing = match &self.editing {
            Some((EditTarget::Element(eid), buf)) if *eid == elem.id() => Some(buf.as_str()),
            _ => None,
        };
        match elem {
            SlideElement::TextBox {
                x,
                y,
                width,
                text,
                font_size,
                color,
                bold,
                centered,
                ..
            } => {
                let fx = ox + x * scale;
                let fy = oy + y * scale;
                let fw = width * scale;
                let fs = font_size * scale;
                let weight = if *bold {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                };
                let words = typing.unwrap_or(text);
                let line_h = fs * 1.25;
                let lines = Self::box_lines(words, fw, fs, weight);
                let mut line_y = fy;
                let mut last_end = fx;
                for line in &lines {
                    // Centred on the box: it was nudged a tenth of the way in
                    // and called centred, which no line of any length was.
                    let lx = if *centered {
                        guitk::text::center_x(line, fx + fw / 2.0, fs, weight).max(fx)
                    } else {
                        fx
                    };
                    last_end = lx + guitk::text::measure(line, fs, weight).min(fw);
                    f.push(RenderCommand::Text {
                        x: lx,
                        y: line_y,
                        text: line.clone(),
                        color: *color,
                        font_size: fs,
                        font_weight: weight,
                        max_width: Some(fw),
                        // Slide text is bounded by its box, and a slide that
                        // silently loses the end of a line is worse than one
                        // that visibly runs out of room.
                        overflow: TextOverflow::Ellipsis,
                    });
                    line_y += line_h;
                }
                if typing.is_some() {
                    // Where the next character goes.
                    let caret_y = (line_y - line_h).max(fy);
                    f.push(RenderCommand::FillRect {
                        x: last_end + 1.0,
                        y: caret_y,
                        width: 2.0,
                        height: fs,
                        color: self.palette.sky,
                        corner_radii: CornerRadii::ZERO,
                    });
                }
            }
            SlideElement::Shape {
                kind,
                x,
                y,
                width,
                height,
                fill_color,
                stroke_color,
                stroke_width,
                ..
            } => {
                let sx = ox + x * scale;
                let sy = oy + y * scale;
                let sw = width * scale;
                let sh = height * scale;
                let lw = stroke_width * scale;

                match kind {
                    ShapeKind::Rectangle => {
                        f.push(RenderCommand::FillRect {
                            x: sx,
                            y: sy,
                            width: sw,
                            height: sh,
                            color: *fill_color,
                            corner_radii: CornerRadii::ZERO,
                        });
                        if lw > 0.0 {
                            f.push(RenderCommand::StrokeRect {
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
                        f.push(RenderCommand::FillRect {
                            x: sx,
                            y: sy,
                            width: sw,
                            height: sh,
                            color: *fill_color,
                            corner_radii: CornerRadii::all(r),
                        });
                        if lw > 0.0 {
                            f.push(RenderCommand::StrokeRect {
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
                        f.push(RenderCommand::Line {
                            x1: sx,
                            y1: sy,
                            x2: sx + sw,
                            y2: sy + sh,
                            color: *stroke_color,
                            width: lw.max(1.0),
                        });
                    }
                    ShapeKind::Arrow => {
                        f.push(RenderCommand::Line {
                            x1: sx,
                            y1: sy,
                            x2: sx + sw,
                            y2: sy + sh,
                            color: *stroke_color,
                            width: lw.max(1.0),
                        });
                        // The head, along the line's own direction: it was two
                        // strokes at fixed angles, which pointed the wrong way
                        // on any arrow that was not drawn down and to the right.
                        let (hx, hy) = (sx + sw, sy + sh);
                        let len = sw.hypot(sh).max(f32::EPSILON);
                        let (ux, uy) = (sw / len, sh / len);
                        let head = 10.0 * scale;
                        for side in [-1.0_f32, 1.0] {
                            f.push(RenderCommand::Line {
                                x1: hx,
                                y1: hy,
                                x2: hx - head * (ux + side * 0.5 * uy),
                                y2: hy - head * (uy - side * 0.5 * ux),
                                color: *stroke_color,
                                width: lw.max(1.0),
                            });
                        }
                    }
                }
            }
            SlideElement::Image {
                x,
                y,
                width,
                height,
                placeholder_label,
                ..
            } => {
                let ix = ox + x * scale;
                let iy = oy + y * scale;
                let iw = width * scale;
                let ih = height * scale;
                f.push(RenderCommand::StrokeRect {
                    x: ix,
                    y: iy,
                    width: iw,
                    height: ih,
                    color: self.palette.overlay0,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
                let label = typing.unwrap_or(placeholder_label);
                let size = 14.0 * scale;
                f.push(RenderCommand::Text {
                    x: guitk::text::center_x(label, ix + iw / 2.0, size, FontWeightHint::Regular)
                        .max(ix),
                    y: iy + ih * 0.45,
                    text: label.to_owned(),
                    color: self.palette.subtext0,
                    font_size: size,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(iw),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            SlideElement::BulletList {
                x,
                y,
                width,
                items,
                font_size,
                color,
                ..
            } => {
                let bx = ox + x * scale;
                let by = oy + y * scale;
                let bw = width * scale;
                let fs = font_size * scale;
                let line_h = fs * 1.6;
                let indent = guitk::text::measure("\u{2022} ", fs, FontWeightHint::Regular);
                let typed: Vec<String>;
                let shown: &[String] = match typing {
                    Some(buf) => {
                        typed = buf.split('\n').map(str::to_owned).collect();
                        &typed
                    }
                    None => items,
                };
                let mut line_y = by;
                for item in shown {
                    let lines =
                        Self::box_lines(item, (bw - indent).max(1.0), fs, FontWeightHint::Regular);
                    for (i, line) in lines.iter().enumerate() {
                        let (lx, text) = if i == 0 {
                            (bx, format!("\u{2022} {line}"))
                        } else {
                            (bx + indent, line.clone())
                        };
                        f.push(RenderCommand::Text {
                            x: lx,
                            y: line_y,
                            text,
                            color: *color,
                            font_size: fs,
                            font_weight: FontWeightHint::Regular,
                            max_width: Some(bw),
                            overflow: TextOverflow::Ellipsis,
                        });
                        line_y += line_h;
                    }
                }
            }
        }
    }

    /// Render the properties panel on the right side.
    ///
    /// Every value in it was printed and none could be changed: the
    /// transition and the background of the slide, the size, weight,
    /// alignment and colour of a text box, a shape's outline. And its six
    /// Insert buttons were drawn and could not be pressed.
    fn render_properties_panel(&self, f: &mut Frame<Target>) {
        let top = TOOLBAR_HEIGHT;
        let bot = self.window_height - STATUS_BAR_HEIGHT;
        let px = self.window_width - PROPERTIES_WIDTH;

        f.push(RenderCommand::FillRect {
            x: px,
            y: top,
            width: PROPERTIES_WIDTH,
            height: bot - top,
            color: self.palette.base,
            corner_radii: CornerRadii::ZERO,
        });
        f.push(RenderCommand::Line {
            x1: px,
            y1: top,
            x2: px,
            y2: bot,
            color: self.palette.surface0,
            width: 1.0,
        });

        let mut y = top + 12.0;
        let lx = px + 12.0;
        let val_w = PROPERTIES_WIDTH - 24.0;

        f.push(RenderCommand::Text {
            x: lx,
            y,
            text: String::from("Properties"),
            color: self.palette.text,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += 24.0;

        let Some(slide) = self.slides.get(self.current_index) else {
            return;
        };
        self.render_property_row(f, lx, y, "Layout", slide.layout.label());
        y += 22.0;
        self.prop_button(
            f,
            lx,
            y,
            "Transition",
            slide.transition.label(),
            Prop::Transition,
        );
        y += 24.0;
        let bg = slide.effective_bg(&self.theme);
        let bg_name = match slide.background {
            None => String::from("Theme"),
            Some(c) => format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b),
        };
        self.prop_button(f, lx, y, "Background", &bg_name, Prop::Background);
        f.push(RenderCommand::FillRect {
            x: lx + val_w - 26.0,
            y: y + 4.0,
            width: 20.0,
            height: 12.0,
            color: bg,
            corner_radii: CornerRadii::all(2.0),
        });
        y += 30.0;

        f.push(RenderCommand::Line {
            x1: lx,
            y1: y,
            x2: px + PROPERTIES_WIDTH - 12.0,
            y2: y,
            color: self.palette.surface0,
            width: 1.0,
        });
        y += 12.0;

        let Some(elem) = self.selected() else {
            f.push(RenderCommand::Text {
                x: lx,
                y,
                text: String::from("No element selected (Tab selects one)"),
                color: self.palette.subtext0,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(val_w),
                overflow: TextOverflow::Ellipsis,
            });
            y += 24.0;
            f.push(RenderCommand::Text {
                x: lx,
                y,
                text: String::from("Insert Element:"),
                color: self.palette.subtext1,
                font_size: 12.0,
                font_weight: FontWeightHint::Bold,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            y += 20.0;
            for (i, label) in INSERT_KINDS.iter().enumerate() {
                self.button(
                    f,
                    Rect::new(lx, y, val_w - 4.0, 22.0),
                    label,
                    Target::Insert(i),
                    true,
                );
                y += 26.0;
            }
            return;
        };

        f.push(RenderCommand::Text {
            x: lx,
            y,
            text: String::from("Element"),
            color: self.palette.ink(self.palette.blue),
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        y += 20.0;
        let (ex, ey, ew, eh) = elem.bounds();
        self.render_property_row(f, lx, y, "Position", &format!("{ex:.0}, {ey:.0}"));
        y += 18.0;
        self.render_property_row(f, lx, y, "Size", &format!("{ew:.0} x {eh:.0}"));
        y += 22.0;

        let (kind, weight_label, weight, words, bold, centred, colour) = match elem {
            SlideElement::TextBox {
                font_size,
                bold,
                centered,
                color,
                ..
            } => (
                "Text box",
                "Text size",
                Some(format!("{font_size:.0}")),
                Some("Edit text"),
                Some(*bold),
                Some(*centered),
                Some(*color),
            ),
            SlideElement::BulletList {
                font_size, color, ..
            } => (
                "Bullets",
                "Text size",
                Some(format!("{font_size:.0}")),
                Some("Edit bullets"),
                None,
                None,
                Some(*color),
            ),
            SlideElement::Shape {
                kind,
                stroke_width,
                fill_color,
                ..
            } => (
                kind.label(),
                "Outline",
                Some(format!("{stroke_width:.0}")),
                None,
                None,
                None,
                Some(*fill_color),
            ),
            SlideElement::Image { .. } => ("Image", "", None, Some("Edit label"), None, None, None),
        };
        self.render_property_row(f, lx, y, "Type", kind);
        y += 22.0;
        if let Some(value) = weight {
            self.render_property_row(f, lx, y + 4.0, weight_label, "");
            let minus = Rect::new(lx + 75.0, y, 26.0, 22.0);
            let plus = Rect::new(lx + 145.0, y, 26.0, 22.0);
            self.button(f, minus, "-", Target::Prop(Prop::Smaller), true);
            f.push(RenderCommand::Text {
                x: lx + 108.0,
                y: y + 5.0,
                text: value,
                color: self.palette.text,
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(34.0),
                overflow: TextOverflow::Ellipsis,
            });
            self.button(f, plus, "+", Target::Prop(Prop::Larger), true);
            y += 26.0;
        }
        if let Some(on) = bold {
            self.prop_button(f, lx, y, "Bold", if on { "Yes" } else { "No" }, Prop::Bold);
            y += 26.0;
        }
        if let Some(on) = centred {
            self.prop_button(
                f,
                lx,
                y,
                "Centred",
                if on { "Yes" } else { "No" },
                Prop::Centre,
            );
            y += 26.0;
        }
        if let Some(c) = colour {
            self.prop_button(f, lx, y, "Colour", "Next", Prop::Colour);
            f.push(RenderCommand::FillRect {
                x: lx + val_w - 26.0,
                y: y + 4.0,
                width: 20.0,
                height: 12.0,
                color: c,
                corner_radii: CornerRadii::all(2.0),
            });
            y += 26.0;
        }
        if let Some(label) = words {
            self.button(
                f,
                Rect::new(lx, y, val_w - 4.0, 22.0),
                label,
                Target::Prop(Prop::Edit),
                true,
            );
            y += 26.0;
        }
        self.button(
            f,
            Rect::new(lx, y, val_w - 4.0, 22.0),
            "Delete element",
            Target::Prop(Prop::Delete),
            true,
        );
    }

    /// A property whose value is a button: a label, and the value pressed to
    /// change it.
    fn prop_button(
        &self,
        f: &mut Frame<Target>,
        x: f32,
        y: f32,
        key: &str,
        value: &str,
        prop: Prop,
    ) {
        f.push(RenderCommand::Text {
            x,
            y: y + 4.0,
            text: key.to_string(),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });
        self.button(
            f,
            Rect::new(x + 75.0, y, PROPERTIES_WIDTH - 24.0 - 79.0, 22.0),
            value,
            Target::Prop(prop),
            true,
        );
    }

    /// Render a key-value property row.
    fn render_property_row(&self, f: &mut Frame<Target>, x: f32, y: f32, key: &str, value: &str) {
        f.push(RenderCommand::Text {
            x,
            y,
            text: key.to_string(),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(70.0),
            overflow: TextOverflow::Ellipsis,
        });
        f.push(RenderCommand::Text {
            x: x + 75.0,
            y,
            text: value.to_string(),
            color: self.palette.text,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(120.0),
            overflow: TextOverflow::Ellipsis,
        });
    }

    /// Render the speaker notes area below the canvas: a press on it (or `N`)
    /// starts typing them.
    fn render_notes_panel(&self, f: &mut Frame<Target>) {
        let notes_y = self.window_height - STATUS_BAR_HEIGHT - NOTES_HEIGHT;
        let notes_w = (self.window_width - SIDEBAR_WIDTH - PROPERTIES_WIDTH).max(0.0);
        let nx = SIDEBAR_WIDTH;
        let panel = Rect::new(nx, notes_y, notes_w, NOTES_HEIGHT);
        let typing = match &self.editing {
            Some((EditTarget::Notes, buf)) => Some(buf.as_str()),
            _ => None,
        };

        f.push(RenderCommand::FillRect {
            x: nx,
            y: notes_y,
            width: notes_w,
            height: NOTES_HEIGHT,
            color: if typing.is_some() || self.hover == Some(Target::Notes) {
                self.palette.surface0
            } else {
                self.palette.base
            },
            corner_radii: CornerRadii::ZERO,
        });
        f.push(RenderCommand::Line {
            x1: nx,
            y1: notes_y,
            x2: nx + notes_w,
            y2: notes_y,
            color: self.palette.surface0,
            width: 1.0,
        });
        f.push(RenderCommand::Text {
            x: nx + 10.0,
            y: notes_y + 6.0,
            text: String::from("Speaker Notes (N)"),
            color: self.palette.subtext1,
            font_size: 11.0,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        let words = typing.unwrap_or_else(|| self.current_notes());
        let width = (notes_w - 20.0).max(1.0);
        let lines = if words.is_empty() {
            vec![String::from(if typing.is_some() {
                ""
            } else {
                "(No notes for this slide -- press here or N to write some)"
            })]
        } else {
            Self::box_lines(words, width, 12.0, FontWeightHint::Regular)
        };
        // Three lines fit. While typing, the last three -- where the words
        // are going; otherwise the first three, the last of them marked if
        // there is more.
        const ROOM: usize = 3;
        let skip = if typing.is_some() {
            lines.len().saturating_sub(ROOM)
        } else {
            0
        };
        let more = typing.is_none() && lines.len() > ROOM;
        for (i, line) in lines.iter().skip(skip).take(ROOM).enumerate() {
            let last = i.saturating_add(1) == ROOM;
            f.push(RenderCommand::Text {
                x: nx + 10.0,
                y: notes_y + 24.0 + i as f32 * 16.0,
                text: if more && last {
                    format!("{line}\u{2026}")
                } else {
                    line.clone()
                },
                color: if words.is_empty() {
                    self.palette.subtext0
                } else {
                    self.palette.text
                },
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(width),
                overflow: TextOverflow::Ellipsis,
            });
        }
        f.hit(Target::Notes, panel);
    }

    /// The new-slide menu: every layout, under the + Slide button.
    fn render_layout_menu(&self, f: &mut Frame<Target>, reached: usize) {
        // A press anywhere but on the menu closes it.
        f.hit(
            Target::MenuBackdrop,
            Rect::new(0.0, 0.0, self.window_width, self.window_height),
        );
        let row_h = 28.0;
        let menu = Rect::new(
            TOOLS_X,
            TOOLBAR_HEIGHT + 2.0,
            240.0,
            36.0 + SlideLayout::all().len() as f32 * row_h,
        );
        self.palette
            .push_surface(f, menu.x, menu.y, menu.w, menu.h, 8.0, Surface::Card);
        f.hit(Target::Menu, menu);
        f.push(RenderCommand::Text {
            x: menu.x + 12.0,
            y: menu.y + 10.0,
            text: String::from("New slide -- 1 to 6, or Esc"),
            color: self.palette.subtext0,
            font_size: 11.0,
            font_weight: FontWeightHint::Regular,
            max_width: Some(menu.w - 24.0),
            overflow: TextOverflow::Ellipsis,
        });
        for (i, layout) in SlideLayout::all().iter().enumerate() {
            let row = Rect::new(
                menu.x + 6.0,
                menu.y + 30.0 + i as f32 * row_h,
                menu.w - 12.0,
                row_h - 2.0,
            );
            if i == reached || self.hover == Some(Target::LayoutChoice(i)) {
                f.push(RenderCommand::FillRect {
                    x: row.x,
                    y: row.y,
                    width: row.w,
                    height: row.h,
                    color: self.palette.surface1,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
            }
            f.push(RenderCommand::Text {
                x: row.x + 8.0,
                y: row.y + 7.0,
                text: format!("{}   {}", i.saturating_add(1), layout.label()),
                color: self.palette.text,
                font_size: 12.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(row.w - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
            f.hit(Target::LayoutChoice(i), row);
        }
    }

    // ---- Slide sorter mode -------------------------------------------------

    /// Where the sorter draws: its pane, how many columns, a thumbnail's size,
    /// the grid's left edge and the pitch from one row to the next.
    fn sorter_geometry(&self) -> SorterGeometry {
        let top = TOOLBAR_HEIGHT;
        let bottom = self.window_height - STATUS_BAR_HEIGHT;
        let pane = Rect::new(0.0, top, self.window_width, (bottom - top).max(0.0));
        let thumb_w: f32 = 220.0;
        let thumb_h: f32 = thumb_w / SLIDE_ASPECT;
        let gap: f32 = 16.0;
        let cols = guitk::grid::columns_across(pane.w - 2.0 * gap, thumb_w, gap);
        let grid_w = (cols.get() as f32) * (thumb_w + gap) - gap;
        SorterGeometry {
            pane,
            cols,
            thumb_w,
            thumb_h,
            gap,
            grid_x: (pane.w - grid_w) / 2.0,
            pitch: thumb_h + gap + 24.0,
        }
    }

    /// How far the sorter can scroll.
    fn sorter_limit(&self) -> f32 {
        let g = self.sorter_geometry();
        let rows = self.slides.len().div_ceil(g.cols.get());
        (rows as f32 * g.pitch + g.gap - g.pane.h).max(0.0)
    }

    /// Scroll the thumbnail column and the sorter so the current slide is in
    /// view in both. The column clipped its thumbnails and never scrolled, so
    /// from the sixth slide on the current one was drawn below the window.
    fn keep_current_visible(&mut self) {
        let (pane, _, thumb_h, pitch) = self.sidebar_pane();
        let top = self.current_index as f32 * pitch;
        let bottom = top + thumb_h + 20.0 + THUMBNAIL_PAD;
        if top < self.sidebar_scroll {
            self.sidebar_scroll = top;
        } else if bottom > self.sidebar_scroll + pane.h {
            self.sidebar_scroll = bottom - pane.h;
        }
        self.sidebar_scroll = self.sidebar_scroll.clamp(0.0, self.sidebar_limit());

        let g = self.sorter_geometry();
        let row = self.current_index / g.cols;
        let top = row as f32 * g.pitch;
        let bottom = top + g.pitch + g.gap;
        if top < self.sorter_scroll {
            self.sorter_scroll = top;
        } else if bottom > self.sorter_scroll + g.pane.h {
            self.sorter_scroll = bottom - g.pane.h;
        }
        self.sorter_scroll = self.sorter_scroll.clamp(0.0, self.sorter_limit());
    }

    /// Render the slide sorter grid view.
    ///
    /// It clipped its grid and never scrolled, and no thumbnail answered a
    /// press: a slide could be chosen or moved here only by key.
    fn render_sorter_mode(&self, f: &mut Frame<Target>) {
        self.render_toolbar(f);
        self.render_status_bar(f);

        let g = self.sorter_geometry();
        f.hit(Target::SorterGrid, g.pane);
        f.clip(g.pane);

        for (i, slide) in self.slides.iter().enumerate() {
            // `cols` is a `NonZeroUsize`, so these use the `Rem`/`Div` impls
            // that cannot divide by zero.
            let col = i % g.cols;
            let row = i / g.cols;
            let tx = g.grid_x + (col as f32) * (g.thumb_w + g.gap);
            let ty = g.pane.y + g.gap + (row as f32) * g.pitch - self.sorter_scroll;
            let cell = Rect::new(tx - 2.0, ty - 2.0, g.thumb_w + 4.0, g.thumb_h + 24.0);
            if f.visible_part(cell).is_none() {
                continue;
            }
            let is_current = i == self.current_index;
            if is_current || self.hover == Some(Target::SorterThumb(i)) {
                f.push(RenderCommand::StrokeRect {
                    x: tx - 2.0,
                    y: ty - 2.0,
                    width: g.thumb_w + 4.0,
                    height: g.thumb_h + 4.0,
                    color: if is_current {
                        self.palette.blue
                    } else {
                        self.palette.surface2
                    },
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(CORNER_R),
                });
            }
            f.push(RenderCommand::FillRect {
                x: tx,
                y: ty,
                width: g.thumb_w,
                height: g.thumb_h,
                color: slide.effective_bg(&self.theme),
                corner_radii: CornerRadii::all(CORNER_R),
            });
            let preview = slide_preview_text(slide);
            if !preview.is_empty() {
                f.push(RenderCommand::Text {
                    x: tx + 8.0,
                    y: ty + 12.0,
                    text: preview,
                    color: self.palette.text,
                    font_size: 10.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(g.thumb_w - 16.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            if slide.transition != Transition::None {
                f.push(RenderCommand::Text {
                    x: tx + 4.0,
                    y: ty + g.thumb_h - 14.0,
                    text: slide.transition.label().to_string(),
                    color: self.palette.ink(self.palette.teal),
                    font_size: 8.0,
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
            }
            f.push(RenderCommand::Text {
                x: tx,
                y: ty + g.thumb_h + 4.0,
                text: format!("{}. {}", i.saturating_add(1), slide.layout.label()),
                color: if is_current {
                    self.palette.ink(self.palette.blue)
                } else {
                    self.palette.subtext0
                },
                font_size: 11.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(g.thumb_w),
                overflow: TextOverflow::Ellipsis,
            });
            // A press chooses the slide, a second opens it, and a drag moves it.
            f.hit(Target::SorterThumb(i), cell);
        }
        f.unclip();
    }

    // ---- The pointer ------------------------------------------------------------

    /// What is under `(x, y)` in the frame last shown.
    fn target_at(&self, x: f32, y: f32) -> Option<Target> {
        if self.last_hits.is_empty() {
            return self.frame().hit_test(x, y);
        }
        self.last_hits
            .iter()
            .rev()
            .find(|(_, rect)| rect.contains(x, y))
            .map(|(target, _)| *target)
    }

    fn handle_mouse(&mut self, event: &MouseEvent) -> EventResult {
        match event.kind {
            MouseEventKind::Press(MouseButton::Left) => {
                let target = self.frame().hit_test(event.x, event.y);
                // A press anywhere but on what is being typed into finishes
                // the typing and keeps the words, as Escape does.
                let mut finished = false;
                if let Some((edit, buf)) = self.editing.clone() {
                    let inside = match (edit, target) {
                        (EditTarget::Element(eid), Some(Target::Element(on))) => eid == on,
                        (EditTarget::Notes, Some(Target::Notes)) => true,
                        _ => false,
                    };
                    if inside {
                        return EventResult::Ignored;
                    }
                    self.commit_editing(edit, &buf);
                    self.editing = None;
                    finished = true;
                }
                let result = match target {
                    Some(target) => self.press(target, event.x, event.y),
                    None => EventResult::Ignored,
                };
                if finished {
                    EventResult::Consumed
                } else {
                    result
                }
            }
            MouseEventKind::Move => {
                if self.drag.is_some() {
                    return self.drag_to(event.x, event.y);
                }
                let over = self.target_at(event.x, event.y);
                if over == self.hover {
                    return EventResult::Ignored;
                }
                self.hover = over;
                EventResult::Consumed
            }
            MouseEventKind::Release(MouseButton::Left) => {
                let result = self.release(event.x, event.y);
                self.keep_current_visible();
                result
            }
            // A release outside the window never arrives, so leaving it ends
            // the drag where it had got to.
            MouseEventKind::Leave => {
                let dragged = self.drag.take().is_some();
                let lit = self.hover.take().is_some();
                if dragged || lit {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            MouseEventKind::Scroll { dy, .. } => self.wheel_at(event.x, event.y, dy),
            _ => EventResult::Ignored,
        }
    }

    /// A left press on `target`.
    fn press(&mut self, target: Target, x: f32, y: f32) -> EventResult {
        let result = match target {
            Target::HelpCard => {
                self.show_help = false;
                EventResult::Consumed
            }
            Target::MenuBackdrop => {
                self.layout_menu = None;
                EventResult::Consumed
            }
            Target::Menu | Target::Sidebar | Target::SorterGrid => EventResult::Ignored,
            Target::LayoutChoice(i) => self.choose_layout(i),
            Target::Tool(tool) => self.use_tool(tool),
            Target::Thumb(i) | Target::SorterThumb(i) => {
                let was_current = i == self.current_index;
                if !was_current {
                    self.go_to_slide(i);
                }
                self.drag = Some(Drag::Slide {
                    from: i,
                    at: (x, y),
                    moved: false,
                    was_current,
                });
                EventResult::Consumed
            }
            Target::Element(id) => {
                // A press on the element already selected types into it, if
                // the pointer comes up where it went down.
                let edit = self.selected_element == Some(id);
                self.selected_element = Some(id);
                let origin = self.selected().map_or((0.0, 0.0), |e| {
                    let (ex, ey, _, _) = e.bounds();
                    (ex, ey)
                });
                self.drag = Some(Drag::Move {
                    id,
                    from: self.to_slide(x, y),
                    origin,
                    moved: false,
                    edit,
                });
                EventResult::Consumed
            }
            Target::Handle(corner) => {
                let Some((id, bounds)) = self.selected().map(|e| (e.id(), e.bounds())) else {
                    return EventResult::Ignored;
                };
                self.drag = Some(Drag::Resize {
                    id,
                    corner,
                    from: self.to_slide(x, y),
                    bounds,
                    moved: false,
                });
                EventResult::Consumed
            }
            Target::Canvas | Target::SlideArea => {
                if self.selected_element.take().is_some() {
                    EventResult::Consumed
                } else {
                    EventResult::Ignored
                }
            }
            Target::Prop(prop) => self.use_prop(prop),
            Target::Insert(i) => self.insert(i),
            Target::Notes => self.begin_notes(),
        };
        if result == EventResult::Consumed {
            self.keep_current_visible();
        }
        result
    }

    /// A toolbar button.
    fn use_tool(&mut self, tool: Tool) -> EventResult {
        match tool {
            Tool::Title => self.begin_deck_title(),
            Tool::NewSlide => {
                self.layout_menu = Some(1);
                EventResult::Consumed
            }
            Tool::Duplicate => {
                self.duplicate_current_slide();
                EventResult::Consumed
            }
            Tool::DeleteSlide => self.delete_current_slide(),
            Tool::ToggleView => self.set_view(match self.view {
                ViewMode::Edit => ViewMode::Sorter,
                ViewMode::Sorter => ViewMode::Edit,
            }),
            Tool::Undo => self.undo_if_any(),
            Tool::Redo => self.redo_if_any(),
            Tool::Export => {
                self.export_as();
                EventResult::Consumed
            }
            Tool::Theme => {
                self.cycle_theme();
                EventResult::Consumed
            }
        }
    }

    /// A property-panel button.
    fn use_prop(&mut self, prop: Prop) -> EventResult {
        match prop {
            Prop::Transition => {
                self.cycle_transition();
                EventResult::Consumed
            }
            Prop::Background => self.cycle_background(),
            Prop::Smaller => self.step_weight(false),
            Prop::Larger => self.step_weight(true),
            Prop::Bold => self.toggle_bold(),
            Prop::Centre => self.toggle_centred(),
            Prop::Colour => self.cycle_element_colour(),
            Prop::Edit => self.begin_editing(),
            Prop::Delete => {
                if self.selected_element.is_none() {
                    return EventResult::Ignored;
                }
                self.delete_selected_element();
                EventResult::Consumed
            }
        }
    }

    /// The pointer moved with the button held.
    fn drag_to(&mut self, x: f32, y: f32) -> EventResult {
        let Some(drag) = self.drag else {
            return EventResult::Ignored;
        };
        let (sx, sy) = self.to_slide(x, y);
        let scale = self.canvas_geometry().2;
        match drag {
            Drag::Move {
                id,
                from,
                origin,
                moved,
                edit,
            } => {
                let (dx, dy) = (sx - from.0, sy - from.1);
                // A press that wanders a pixel or two is still a press.
                if !moved && (dx.abs() + dy.abs()) * scale < 3.0 {
                    return EventResult::Ignored;
                }
                if !moved {
                    self.checkpoint();
                }
                self.drag = Some(Drag::Move {
                    id,
                    from,
                    origin,
                    moved: true,
                    edit,
                });
                let Some(element) = self
                    .slides
                    .get_mut(self.current_index)
                    .and_then(|s| s.element_by_id_mut(id))
                else {
                    return EventResult::Ignored;
                };
                let (_, _, w, h) = element.bounds();
                let (nx, ny) = keep_on_slide(origin.0 + dx, origin.1 + dy, w, h);
                element.set_position(nx, ny);
                EventResult::Consumed
            }
            Drag::Resize {
                id,
                corner,
                from,
                bounds,
                moved,
            } => {
                let (dx, dy) = (sx - from.0, sy - from.1);
                if !moved && (dx.abs() + dy.abs()) * scale < 3.0 {
                    return EventResult::Ignored;
                }
                if !moved {
                    self.checkpoint();
                }
                self.drag = Some(Drag::Resize {
                    id,
                    corner,
                    from,
                    bounds,
                    moved: true,
                });
                let Some(element) = self
                    .slides
                    .get_mut(self.current_index)
                    .and_then(|s| s.element_by_id_mut(id))
                else {
                    return EventResult::Ignored;
                };
                let (nx, ny, nw, nh) = resized(bounds, corner, dx, dy, min_size(element));
                element.set_position(nx, ny);
                element.set_size(nw, nh);
                EventResult::Consumed
            }
            Drag::Slide {
                from,
                at,
                moved,
                was_current,
            } => {
                if !moved && (x - at.0).abs() + (y - at.1).abs() < 6.0 {
                    return EventResult::Ignored;
                }
                self.drag = Some(Drag::Slide {
                    from,
                    at,
                    moved: true,
                    was_current,
                });
                // The thumbnail it would land on is lit.
                self.hover = self.target_at(x, y);
                EventResult::Consumed
            }
        }
    }

    /// The button came up.
    fn release(&mut self, x: f32, y: f32) -> EventResult {
        let Some(drag) = self.drag.take() else {
            return EventResult::Ignored;
        };
        match drag {
            Drag::Move { moved, edit, .. } => {
                if !moved && edit {
                    return self.begin_editing();
                }
                EventResult::Consumed
            }
            Drag::Resize { .. } => EventResult::Consumed,
            Drag::Slide {
                from,
                moved,
                was_current,
                ..
            } => {
                let onto = match self.target_at(x, y) {
                    Some(Target::Thumb(j) | Target::SorterThumb(j)) => Some(j),
                    _ => None,
                };
                match onto {
                    Some(to) if moved && to != from => {
                        self.move_slide(from, to);
                        EventResult::Consumed
                    }
                    // In the sorter a second press on the slide opens it.
                    Some(to)
                        if !moved && to == from && was_current && self.view == ViewMode::Sorter =>
                    {
                        self.set_view(ViewMode::Edit)
                    }
                    _ => EventResult::Consumed,
                }
            }
        }
    }

    /// Move slide `from` to position `to`.
    fn move_slide(&mut self, from: usize, to: usize) {
        let len = self.slides.len();
        if from >= len || to >= len || from == to {
            return;
        }
        self.checkpoint();
        let slide = self.slides.remove(from);
        self.slides.insert(to, slide);
        self.current_index = to;
        self.selected_element = None;
    }

    /// The wheel, over the thumbnail column or the sorter.
    fn wheel_at(&mut self, x: f32, y: f32, dy: f32) -> EventResult {
        let over = self.target_at(x, y);
        let (before, pitch, limit, sidebar) = match over {
            Some(Target::Sidebar | Target::Thumb(_)) => {
                let (_, _, _, pitch) = self.sidebar_pane();
                (self.sidebar_scroll, pitch, self.sidebar_limit(), true)
            }
            Some(Target::SorterGrid | Target::SorterThumb(_)) => {
                let g = self.sorter_geometry();
                (self.sorter_scroll, g.pitch, self.sorter_limit(), false)
            }
            _ => return EventResult::Ignored,
        };
        let rows = self.wheel.rows(dy);
        let after = (before + rows as f32 * pitch).clamp(0.0, limit);
        if (after - before).abs() < f32::EPSILON {
            return EventResult::Ignored;
        }
        if sidebar {
            self.sidebar_scroll = after;
        } else {
            self.sorter_scroll = after;
        }
        EventResult::Consumed
    }
}

/// Where the sorter draws; see `SlidesApp::sorter_geometry`.
#[derive(Clone, Copy, Debug)]
struct SorterGeometry {
    pane: Rect,
    cols: std::num::NonZeroUsize,
    thumb_w: f32,
    thumb_h: f32,
    gap: f32,
    grid_x: f32,
    pitch: f32,
}

/// The smallest an element may be resized to: none for a line or an arrow,
/// which may be flat.
fn min_size(element: &SlideElement) -> f32 {
    match element {
        SlideElement::Shape {
            kind: ShapeKind::Line | ShapeKind::Arrow,
            ..
        } => 0.0,
        _ => MIN_ELEMENT,
    }
}

/// A position that keeps at least a sliver of a `w` by `h` element on the
/// slide, so a move cannot lose it off an edge where nothing can reach it.
fn keep_on_slide(x: f32, y: f32, w: f32, h: f32) -> (f32, f32) {
    const KEEP: f32 = 16.0;
    (
        x.clamp(KEEP - w.max(KEEP), SLIDE_W - KEEP),
        y.clamp(KEEP - h.max(KEEP), SLIDE_H - KEEP),
    )
}

/// A box `b` with its `corner` dragged by `(dx, dy)`: the corner's two edges
/// move, stopping at `min` from the edges opposite, which stay put.
fn resized(
    b: (f32, f32, f32, f32),
    corner: Corner,
    dx: f32,
    dy: f32,
    min: f32,
) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = b;
    let (mut left, mut top, mut right, mut bottom) = (x, y, x + w, y + h);
    let moves_left = matches!(corner, Corner::TopLeft | Corner::BottomLeft);
    let moves_top = matches!(corner, Corner::TopLeft | Corner::TopRight);
    if moves_left {
        left += dx;
    } else {
        right += dx;
    }
    if moves_top {
        top += dy;
    } else {
        bottom += dy;
    }
    if right - left < min {
        if moves_left {
            left = right - min;
        } else {
            right = left + min;
        }
    }
    if bottom - top < min {
        if moves_top {
            top = bottom - min;
        } else {
            bottom = top + min;
        }
    }
    (left, top, right - left, bottom - top)
}

// ============================================================================
// Helper functions
// ============================================================================
// Helper functions
// ============================================================================

/// The line of text a slide thumbnail shows: the slide's title, or failing
/// that the first line of its first text element.
///
/// This used to cut the result to 30 characters and append "...". It no longer
/// does, because both call sites draw the result with `max_width` and
/// `TextOverflow::Ellipsis` already set — the renderer measures the string in
/// the real font and cuts it to the thumbnail's actual width, which is a
/// different number in the slide sorter (`thumb_w - 16.0`) than in the sidebar
/// (`thumb_w - 8.0`) and neither of them is 30 characters. Cutting here as
/// well meant the shorter of two answers won, and the shorter one was the
/// guess. The 30 was also compared against `len()` — bytes — so a title in
/// Greek or Japanese was cut at ten characters or fewer.
///
/// What is still done here is take a single *line*: a `Text` command draws one
/// line, so a multi-line text box must not be handed over with its newlines in
/// place.
fn slide_preview_text(slide: &Slide) -> String {
    // Try title first.
    if !slide.title.is_empty() {
        return first_line(&slide.title);
    }
    // Otherwise, grab first text element content.
    for elem in &slide.elements {
        match elem {
            SlideElement::TextBox { text, .. } if !text.is_empty() => {
                return first_line(text);
            }
            SlideElement::BulletList { items, .. } => {
                if let Some(first) = items.first() {
                    return first_line(first);
                }
            }
            _ => {}
        }
    }
    String::new()
}

/// The first line of `s`, without its line ending.
fn first_line(s: &str) -> String {
    s.split('\n').next().unwrap_or(s).trim_end().to_string()
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

impl App for SlidesApp {
    fn theme_changed(&mut self, palette: &Palette) {
        self.palette = *palette;
    }

    fn title(&self) -> String {
        format!(
            "{} — slide {} of {}",
            self.title,
            self.current_index.saturating_add(1),
            self.slides.len()
        )
    }

    fn initial_size(&self) -> (u32, u32) {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "both are positive constants well inside u32"
        )]
        {
            (self.window_width as u32, self.window_height as u32)
        }
    }

    /// No clock.
    ///
    /// Slides advance when the speaker advances them. There are no transitions
    /// and no timed rehearsal mode, so a tick would redraw an identical frame —
    /// and this is a program that runs full-screen in front of an audience,
    /// where a needless wake-up is a dropped frame someone can see.
    fn tick_interval(&self) -> Option<Duration> {
        None
    }

    fn on_event(&mut self, event: &Event) -> Response {
        if matches!(event, Event::CloseRequested) {
            return Response::Exit;
        }
        match self.handle_event(event) {
            EventResult::Consumed => Response::Redraw,
            EventResult::Ignored => Response::Idle,
        }
    }

    fn render(&mut self, width: f32, height: f32) -> RenderTree {
        // Reconciled with the size we are handed rather than trusted from the
        // last `Resize`: the compositor may grant a size that was never asked
        // for, and the first frame is drawn before any `Resize` arrives.
        self.window_width = width;
        self.window_height = height;
        let frame = self.frame();
        self.last_hits = frame.hits().to_vec();
        frame.into_tree()
    }
}

fn main() -> ExitCode {
    // A new deck: one title slide. It opened on a six-slide sample, because
    // that was the only way five of the six layouts could appear at all;
    // Ctrl+M and the + Slide button reach every layout now.
    let mut app = SlidesApp::new(1280.0, 720.0);
    app::launch("slides", &mut app)
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
    // Sizes stepped by whole numbers from whole numbers are exact.
    clippy::float_cmp
)]
mod tests {
    use super::*;

    // ---- The door ----

    fn ctrl_e() -> Event {
        Event::Key(KeyEvent {
            key: Key::E,
            pressed: true,
            modifiers: guitk::event::Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// The presentation reaches the disk, and reads back as what was exported.
    ///
    /// `export_html` was written and tested and had no caller. It emits a
    /// DOCTYPE, a charset, a viewport, escaped text, a stylesheet and
    /// per-slide `<div>`s with working navigation -- **a whole presentation
    /// format nothing could ask for.**
    #[test]
    fn a_presentation_reaches_the_disk() {
        let dir =
            std::env::temp_dir().join(format!("slides-export-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("deck.html");
        let _ = std::fs::remove_file(&path);

        let mut app = SlidesApp::new(1280.0, 720.0);
        app.seed_sample_deck();
        let expected = app.export_html();
        let count = app.slide_count();
        let said = app.write_html(&path);

        let back = std::fs::read_to_string(&path).expect("the file it said it wrote");
        assert_eq!(
            back, expected,
            "what was read back is not what was composed"
        );
        assert!(
            back.starts_with("<!DOCTYPE html>"),
            "not a document: {said}"
        );
        assert!(said.contains(&format!("{count} slide(s)")), "said: {said}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Ctrl+E asks where to put it, and the picker is drawn.
    #[test]
    fn ctrl_e_asks_and_the_picker_is_drawn() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        assert!(!app.picker.is_open(), "nothing should be open at rest");
        let closed = app.render_commands().len();

        assert_eq!(app.handle_event(&ctrl_e()), EventResult::Consumed);
        assert!(app.picker.is_open(), "Ctrl+E should ask for a destination");

        let own = app.render_commands().len().saturating_sub(closed);
        assert!(
            own > 0,
            "the open picker contributed {own} commands; it is not being drawn"
        );
    }

    /// A failed write is reported rather than passed over.
    #[test]
    fn a_failed_export_is_reported() {
        let dir =
            std::env::temp_dir().join(format!("slides-nodir-{}-{}", std::process::id(), line!()));
        let _ = std::fs::remove_dir_all(&dir);

        let mut app = SlidesApp::new(1280.0, 720.0);
        // A path under a directory that does not exist.
        let said = app.write_html(&dir.join("deep").join("deck.html"));
        assert!(said.starts_with("Could not write "), "said: {said}");
    }

    // ------------------------------------------------------------------
    // Events
    //
    // The app had no input handling at all until it was wired to the
    // compositor.
    // ------------------------------------------------------------------

    use guitk::event::Modifiers;
    use guitk::shortcut::keystrokes;

    fn seeded() -> SlidesApp {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.seed_sample_deck();
        app
    }

    fn press(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: String::new(),
        })
    }

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

    /// The status bar says the app is typing, and how to stop.
    ///
    /// The shape keys are captured while typing, so somebody pressing `S` for
    /// a rectangle gets an "S" in their text. A mode with no indicator is one
    /// the user cannot tell they are in.
    #[test]
    fn the_status_bar_says_it_is_typing() {
        let mut app = seeded();
        app.handle_event(&press(Key::T));
        app.handle_event(&press(Key::Enter));

        let shown: Vec<String> = app
            .render_commands()
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            shown.iter().any(|t| t.contains("Esc to finish")),
            "nothing on screen says the app is typing or how to stop"
        );
    }

    /// The deck can be named, and the window bar says so.
    ///
    /// `title` had no writer, so every deck was "Untitled Presentation" --
    /// in the window bar, and in the filename `export_as` builds.
    #[test]
    fn ctrl_shift_t_names_the_deck() {
        let mut app = seeded();
        assert_eq!(app.title, "Untitled Presentation", "control: the default");

        app.handle_event(&press_ctrl_shift(Key::T));
        for c in ["Q", "3"] {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Enter));

        assert_eq!(app.title, "Q3", "the deck was not renamed");
        assert!(
            app.title().starts_with("Q3"),
            "the window bar still says {:?}",
            app.title()
        );
    }

    /// Ctrl+Shift+T does not cycle the theme.
    ///
    /// `Key::T if ctrl` has no shift guard and sits in the same match, so an
    /// arm order that put it first would have changed the theme and left the
    /// name alone -- indistinguishable from a rename key that does nothing.
    #[test]
    fn ctrl_shift_t_does_not_cycle_the_theme() {
        let mut app = seeded();
        let theme = app.theme.name.clone();

        app.handle_event(&press_ctrl_shift(Key::T));

        assert_eq!(app.theme.name, theme, "Ctrl+Shift+T changed the theme");
        assert!(app.editing.is_some(), "and did not start the rename");
    }

    /// An empty name is refused rather than blanking the window bar.
    #[test]
    fn an_empty_deck_name_is_refused() {
        let mut app = seeded();
        app.handle_event(&press_ctrl_shift(Key::T));
        app.handle_event(&press(Key::Enter));

        assert_eq!(
            app.title, "Untitled Presentation",
            "an empty name blanked the deck's name"
        );
    }

    /// **Every key the shortcut list advertises is one this program answers.**
    ///
    /// `apps/rssreader` shipped an overlay of twenty-one shortcuts of which
    /// about four worked, and three named operations that existed nowhere in
    /// the crate. A list and a handler are two things that must agree, and
    /// nothing keeps them agreeing except a test that reads both.
    ///
    /// Two things this test deliberately does *not* do.
    ///
    /// The event is built from the row's own key text by `guitk::shortcut`,
    /// rather than looked up in a parallel table of events. A second table
    /// would be a second list to keep in step -- the defect this test exists
    /// to prevent, rebuilt inside the test. It started as exactly that table,
    /// forty lines of `"Left" => Key::Left`, and `apps/mixer` turned out to
    /// have written the same forty lines already.
    ///
    /// And the claim checked is "some reachable state answers this key", not
    /// "this key is taken right now". Several of these decline on purpose:
    /// `Left` at the first slide, `1` when the edit view is already up, `Ctrl+V`
    /// with nothing copied. Declining from its own arm *is* answering -- the
    /// defect is a row that falls through to the catch-all in every state. So
    /// each key is offered to three decks and has to be taken by one.
    #[test]
    fn every_advertised_key_does_something() {
        for (row, what) in SHORTCUTS {
            for stroke in keystrokes(row).unwrap_or_else(|e| panic!("{e}")) {
                let event = Event::Key(stroke.clone());
                let taken = states()
                    .iter_mut()
                    .any(|app| app.handle_event(&event) == EventResult::Consumed);
                assert!(
                    taken,
                    "the list advertises {row:?} for {what:?}, and no state answers {:?}",
                    stroke.key
                );
            }
        }
    }

    /// Three decks, chosen so that between them every advertised key has
    /// something it could do. A key no one of them takes is a key nothing acts
    /// on.
    fn states() -> Vec<SlidesApp> {
        let plain = seeded();

        // The other view, so `1` has somewhere to return from.
        let mut sorter = seeded();
        sorter.handle_event(&press(Key::Num2));

        // Mid-deck, holding a copied slide and a selected text box: what the
        // paging, paste and element keys each need before they will act.
        let mut working = seeded();
        working.handle_event(&press_ctrl(Key::N));
        working.handle_event(&press(Key::Home));
        working.handle_event(&press(Key::Right));
        working.handle_event(&press_ctrl(Key::C));
        working.handle_event(&press(Key::T));

        // A change undone, so redo has something to do.
        let mut undone = seeded();
        undone.handle_event(&press(Key::T));
        undone.handle_event(&press_ctrl(Key::Z));

        vec![plain, sorter, working, undone]
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
        let mut app = seeded();
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
    fn drawn_text(app: &SlidesApp) -> String {
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

    fn types(text: &str) -> Event {
        Event::Key(KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: Modifiers::NONE,
            text: text.to_owned(),
        })
    }

    /// A user can put words on a slide.
    ///
    /// This program could not: zero assignments to `.text` anywhere in the
    /// crate, tests included, and zero `key.text` sites, so every box said
    /// "New Text" forever and a deck was always somebody else's placeholder.
    ///
    /// End-to-end on purpose. Every piece of this existed -- the element, the
    /// accessor, the renderer -- and the program still could not be used, so
    /// the assertion has to be that the words come out.
    #[test]
    fn a_user_can_put_words_on_a_slide() {
        let mut app = seeded();
        app.handle_event(&press(Key::T));
        let eid = app.selected_element.expect("adding a text box selects it");

        app.handle_event(&press(Key::Enter));
        assert!(app.editing.is_some(), "Enter did not begin typing");
        for c in ["H", "i"] {
            app.handle_event(&types(c));
        }
        app.handle_event(&press(Key::Escape));

        assert!(app.editing.is_none(), "the mode did not close");
        let slide = app.slides.get(app.current_index).expect("a slide");
        let elem = slide
            .elements
            .iter()
            .find(|e| e.id() == eid)
            .expect("the box");
        assert!(
            format!("{elem:?}").contains("\"Hi\""),
            "the words are not in the element: {elem:?}"
        );
    }

    /// The slide shows the words as they are typed.
    #[test]
    fn the_slide_shows_the_words_as_they_are_typed() {
        let mut app = seeded();
        app.handle_event(&press(Key::T));
        app.handle_event(&press(Key::Enter));
        for c in ["H", "i"] {
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
            shown.iter().any(|t| t == "Hi"),
            "what is being typed is nowhere on the slide"
        );
    }

    /// Typing a letter does not also drop a shape on the slide.
    ///
    /// `S`, `O`, `L`, `A` and `I` add shapes outside this mode; a title
    /// containing any of them would otherwise litter the slide while being
    /// written.
    #[test]
    fn typing_does_not_fire_the_shape_keys() {
        let mut app = seeded();
        app.handle_event(&press(Key::T));
        let before = element_count(&app);

        app.handle_event(&press(Key::Enter));
        for c in ["S", "a", "l", "e", "s"] {
            app.handle_event(&types(c));
        }

        assert_eq!(
            element_count(&app),
            before,
            "typing added elements to the slide"
        );
    }

    fn press_ctrl(k: Key) -> Event {
        Event::Key(KeyEvent {
            key: k,
            pressed: true,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        })
    }

    /// The element count of the slide now showing.
    fn element_count(app: &SlidesApp) -> usize {
        app.slides
            .get(app.current_index)
            .map_or(0, |s| s.elements.len())
    }

    /// `S`, `O`, `L` and `A` put the four shapes on a slide.
    ///
    /// `add_shape` had no caller, so `T` was the only thing that could put
    /// anything on a slide: this program made decks of textboxes.
    #[test]
    fn the_four_shape_keys_each_add_their_shape() {
        for (key, kind) in [
            (Key::S, ShapeKind::Rectangle),
            (Key::O, ShapeKind::Ellipse),
            (Key::L, ShapeKind::Line),
            (Key::A, ShapeKind::Arrow),
        ] {
            let mut app = seeded();
            let before = element_count(&app);

            assert_eq!(app.handle_event(&press(key)), EventResult::Consumed);

            assert_eq!(
                element_count(&app),
                before + 1,
                "{key:?} added nothing to the slide"
            );
            let added = app
                .slides
                .get(app.current_index)
                .and_then(|s| s.elements.last())
                .expect("the element just added");
            assert!(
                format!("{added:?}").contains(&format!("{kind:?}")),
                "{key:?} added something that is not a {kind:?}: {added:?}"
            );
        }
    }

    /// `I` adds an image placeholder.
    #[test]
    fn i_adds_an_image_placeholder() {
        let mut app = seeded();
        let before = element_count(&app);

        assert_eq!(app.handle_event(&press(Key::I)), EventResult::Consumed);

        assert_eq!(element_count(&app), before + 1, "I added nothing");
    }

    /// Delete removes the selected element rather than the slide around it.
    ///
    /// `delete_selected_element` had no caller, so removing a shape meant
    /// deleting the whole slide it was on.
    #[test]
    fn delete_removes_the_selected_element_not_the_slide() {
        let mut app = seeded();
        let slides_before = app.slides.len();
        app.handle_event(&press(Key::S));
        let elements_before = element_count(&app);
        assert!(
            app.selected_element.is_some(),
            "control: adding a shape selects it"
        );

        app.handle_event(&press(Key::Delete));

        assert_eq!(app.slides.len(), slides_before, "it deleted the slide");
        assert_eq!(
            element_count(&app),
            elements_before - 1,
            "the element is still there"
        );
    }

    /// Shift+Delete still means the slide, even with an element selected.
    #[test]
    fn shift_delete_still_removes_the_slide() {
        let mut app = seeded();
        app.handle_event(&press(Key::S));
        let slides_before = app.slides.len();
        assert!(slides_before > 1, "control: needs two slides to delete one");

        app.handle_event(&Event::Key(KeyEvent {
            key: Key::Delete,
            pressed: true,
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::NONE
            },
            text: String::new(),
        }));

        assert_eq!(
            app.slides.len(),
            slides_before - 1,
            "Shift+Delete did not remove the slide"
        );
    }

    /// Ctrl+T moves through the themes, which the window prints.
    ///
    /// `set_theme` had no caller, so "Theme: Mocha" could not say anything
    /// else though `light()` and `vibrant()` both existed.
    #[test]
    fn ctrl_t_cycles_the_theme() {
        let mut app = seeded();
        let first = app.theme.name.clone();

        app.handle_event(&press_ctrl(Key::T));
        let second = app.theme.name.clone();
        assert_ne!(second, first, "Ctrl+T did not change the theme");

        app.handle_event(&press_ctrl(Key::T));
        app.handle_event(&press_ctrl(Key::T));
        assert_eq!(app.theme.name, first, "the themes do not come back round");
    }

    /// Ctrl+T does not also add a textbox.
    ///
    /// `Key::T` is unguarded and appears in the same match, so an arm order
    /// that put it first would have taken Ctrl+T and added a textbox while
    /// leaving the theme alone -- which looks exactly like a theme key that
    /// does nothing.
    #[test]
    fn ctrl_t_does_not_add_a_textbox() {
        let mut app = seeded();
        let before = element_count(&app);

        app.handle_event(&press_ctrl(Key::T));

        assert_eq!(element_count(&app), before, "Ctrl+T added a textbox");
    }

    /// Ctrl+R moves the current slide through the transitions.
    #[test]
    fn ctrl_r_cycles_the_transition() {
        let mut app = seeded();
        let before = app
            .slides
            .get(app.current_index)
            .map(|s| s.transition)
            .expect("a slide");

        app.handle_event(&press_ctrl(Key::R));

        let after = app
            .slides
            .get(app.current_index)
            .map(|s| s.transition)
            .expect("a slide");
        assert_ne!(after, before, "Ctrl+R did not change the transition");
    }

    #[test]
    fn the_seeded_deck_has_one_of_every_layout() {
        // The seed is what the window opens on, and a test cannot call `main`,
        // so it lives in a method. One of each layout, so every layout the
        // renderer knows how to draw appears somewhere in the first deck.
        let app = seeded();
        // Six, not five: `SlidesApp::new` already starts with one slide, and
        // the seed adds one of each of the five layouts on top of it.
        assert_eq!(app.slide_count(), 6, "the seeded deck is the wrong size");
    }

    #[test]
    fn paging_moves_through_the_deck_and_stops_at_the_ends() {
        // Stopping rather than wrapping: a presentation that jumps from the
        // last slide back to the title is one the speaker has to notice and
        // undo in front of the room.
        let mut app = seeded();
        assert_eq!(app.current_index(), 0);
        assert_eq!(app.handle_event(&press(Key::Left)), EventResult::Ignored);
        for i in 1..app.slide_count() {
            assert_eq!(app.handle_event(&press(Key::Right)), EventResult::Consumed);
            assert_eq!(app.current_index(), i);
        }
        // At the last slide, forward stops rather than wrapping to the title.
        let last = app.current_index();
        assert_eq!(app.handle_event(&press(Key::Right)), EventResult::Ignored);
        assert_eq!(
            app.current_index(),
            last,
            "paging past the end wrapped to the start"
        );
        assert_eq!(app.handle_event(&press(Key::Home)), EventResult::Consumed);
        assert_eq!(app.current_index(), 0);
        assert_eq!(app.handle_event(&press(Key::End)), EventResult::Consumed);
        assert_eq!(app.current_index(), app.slide_count() - 1);
    }

    #[test]
    fn ctrl_pageup_reorders_rather_than_paging() {
        // A guard narrows only the arm it is on, so an unguarded `Key::PageUp`
        // listed first swallows the Ctrl case entirely — which is what the
        // compiler reported as an unreachable pattern.
        let mut app = seeded();
        app.go_to_slide(2);
        // Moving a slide up *also* moves the cursor with it, so the index
        // alone cannot tell reordering from paging. What distinguishes them is
        // that the deck's order changed: after a move, the slide that was above
        // is now below.
        let above = app.slide_id_at(app.current_index().saturating_sub(1));
        let moved = app.slide_id_at(app.current_index());
        assert_eq!(
            app.handle_event(&press_ctrl(Key::PageUp)),
            EventResult::Consumed
        );
        assert_eq!(
            app.slide_id_at(app.current_index()),
            moved,
            "the cursor should have followed the slide"
        );
        assert_eq!(
            app.slide_id_at(app.current_index().saturating_add(1)),
            above,
            "Ctrl+PageUp paged back instead of reordering"
        );
    }

    #[test]
    fn ctrl_n_adds_a_slide_and_ctrl_d_duplicates_one() {
        let mut app = seeded();
        let before = app.slide_count();
        assert_eq!(app.handle_event(&press_ctrl(Key::N)), EventResult::Consumed);
        assert_eq!(app.slide_count(), before + 1);
        assert_eq!(app.handle_event(&press_ctrl(Key::D)), EventResult::Consumed);
        assert_eq!(app.slide_count(), before + 2);
    }

    #[test]
    fn copy_and_paste_round_trip_a_slide() {
        let mut app = seeded();
        let before = app.slide_count();
        assert_eq!(app.handle_event(&press_ctrl(Key::C)), EventResult::Consumed);
        assert_eq!(app.handle_event(&press_ctrl(Key::V)), EventResult::Consumed);
        assert_eq!(app.slide_count(), before + 1);
    }

    #[test]
    fn pasting_an_empty_clipboard_is_not_a_redraw() {
        let mut app = seeded();
        assert_eq!(app.handle_event(&press_ctrl(Key::V)), EventResult::Ignored);
    }

    #[test]
    fn the_last_slide_cannot_be_deleted() {
        // An empty deck has nothing to draw and nothing to select.
        // `new` already starts with one slide, which is the case under test.
        let mut app = SlidesApp::new(1280.0, 720.0);
        assert_eq!(app.slide_count(), 1);
        assert_eq!(app.handle_event(&press(Key::Delete)), EventResult::Ignored);
        assert_eq!(app.slide_count(), 1);
        app.add_slide(SlideLayout::Blank);
        assert_eq!(app.slide_count(), 2);
        assert_eq!(app.handle_event(&press(Key::Delete)), EventResult::Consumed);
        assert_eq!(app.slide_count(), 1);
    }

    #[test]
    fn the_view_keys_switch_between_the_two_views() {
        let mut app = seeded();
        assert_eq!(app.handle_event(&press(Key::Num2)), EventResult::Consumed);
        assert_eq!(app.view, ViewMode::Sorter);
        // Asking for the view already shown is not a redraw.
        assert_eq!(app.handle_event(&press(Key::Num2)), EventResult::Ignored);
        // Tab walks a slide's elements now, and in the sorter there are none.
        assert_eq!(app.handle_event(&press(Key::Tab)), EventResult::Ignored);
        assert_eq!(app.handle_event(&press(Key::Num1)), EventResult::Consumed);
        assert_eq!(app.view, ViewMode::Edit);
    }

    #[test]
    fn a_key_the_app_has_no_use_for_is_not_consumed() {
        let mut app = seeded();
        assert_eq!(app.handle_event(&press(Key::F9)), EventResult::Ignored);
    }

    #[test]
    fn a_key_release_does_nothing() {
        let mut app = seeded();
        let before = app.slide_count();
        let release = Event::Key(KeyEvent {
            key: Key::N,
            pressed: false,
            modifiers: Modifiers::ctrl(),
            text: String::new(),
        });
        assert_eq!(app.handle_event(&release), EventResult::Ignored);
        assert_eq!(app.slide_count(), before);
    }

    #[test]
    fn the_title_says_which_slide_of_how_many() {
        // A minimised presentation is a taskbar entry and nothing else.
        let mut app = seeded();
        // The whole phrase, not the digits alone: the presentation's own title
        // may contain a number, so `contains('1')` can be satisfied by text
        // that has nothing to do with the slide position.
        let n = app.slide_count();
        assert!(
            app.title().contains(&format!("slide 1 of {n}")),
            "title {:?} does not say which slide of how many",
            app.title()
        );
        app.handle_event(&press(Key::End));
        assert!(
            app.title().contains(&format!("slide {n} of {n}")),
            "the title should follow the current slide, said {:?}",
            app.title()
        );
    }

    #[test]
    fn rendering_draws_something_in_both_views_at_an_awkward_size() {
        let mut app = seeded();
        for view in [ViewMode::Edit, ViewMode::Sorter] {
            let _ = app.handle_event(&press(if view == ViewMode::Edit {
                Key::Num1
            } else {
                Key::Num2
            }));
            for (w, h) in [(1.0, 1.0), (640.0, 480.0), (3840.0, 2160.0)] {
                assert!(
                    !app.render(w, h).commands.is_empty(),
                    "{view:?} drew nothing at {w}x{h}"
                );
            }
        }
    }

    // ---- HTML export escaping ----------------------------------------------

    /// Count the tags a browser would actually see.
    ///
    /// Not a substring search: correctly escaped output legitimately contains
    /// the payload text `<script>` (spelled `&lt;script&gt;`), so
    /// `html.contains("<script>")` would be checking the wrong thing in both
    /// directions. What matters is how many `<` characters survive as tag
    /// openers.
    fn tag_count(html: &str, tag: &str) -> usize {
        html.matches(&format!("<{tag}")).count()
    }

    /// Every text field reachable from `export_html` must be escaped. Driving
    /// them all from one list is deliberate: the bug this test was written for
    /// was a single field among four being missed, so a test that named the
    /// fields individually would have been just as easy to write incompletely.
    #[test]
    fn no_text_field_can_inject_a_tag_into_the_export() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        const PAYLOAD: &str = "<script>alert(1)</script>";

        let mut app = SlidesApp::new(800.0, 600.0);
        app.title = PAYLOAD.to_string();
        let slide = app.slides.get_mut(0).expect("a new deck has one slide");
        slide.elements.clear();
        slide.elements.push(SlideElement::TextBox {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            text: PAYLOAD.to_string(),
            font_size: 12.0,
            color: pal.text,
            bold: false,
            centered: false,
        });
        slide.elements.push(SlideElement::Image {
            id: 2,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            placeholder_label: PAYLOAD.to_string(),
        });
        slide.elements.push(SlideElement::BulletList {
            id: 3,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            items: vec![PAYLOAD.to_string()],
            font_size: 12.0,
            color: pal.text,
        });

        let html = app.export_html();
        // The export ships exactly one <script> block: its own navigation JS.
        assert_eq!(
            tag_count(&html, "script"),
            1,
            "a text field injected a script tag:\n{html}"
        );
        // And the payload must be present in escaped form, so this test cannot
        // pass by the fields simply being dropped.
        assert!(
            html.contains("&lt;script&gt;"),
            "payload was dropped rather than escaped:\n{html}"
        );
    }

    #[test]
    fn a_quote_in_a_label_cannot_escape_the_style_attribute() {
        let mut app = SlidesApp::new(800.0, 600.0);
        let slide = app.slides.get_mut(0).expect("a new deck has one slide");
        slide.elements.clear();
        slide.elements.push(SlideElement::Image {
            id: 1,
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
            placeholder_label: "\"><img onerror=alert(1) src=x>".to_string(),
        });
        let html = app.export_html();
        assert_eq!(tag_count(&html, "img "), 0, "label forged a tag:\n{html}");
    }

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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let e = SlideElement::TextBox {
            id: 1,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            text: String::from("Hi"),
            font_size: 14.0,
            color: pal.text,
            bold: false,
            centered: false,
        };
        assert_eq!(e.bounds(), (10.0, 20.0, 100.0, 50.0));
        assert_eq!(e.id(), 1);
    }

    #[test]
    fn test_element_translate() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut e = SlideElement::Shape {
            id: 2,
            kind: ShapeKind::Rectangle,
            x: 50.0,
            y: 50.0,
            width: 80.0,
            height: 60.0,
            fill_color: pal.blue,
            stroke_color: pal.blue,
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let mut e = SlideElement::BulletList {
            id: 4,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 200.0,
            items: vec![String::from("A")],
            font_size: 16.0,
            color: pal.text,
        };
        e.set_size(300.0, 400.0);
        let (_, _, w, h) = e.bounds();
        assert!((w - 300.0).abs() < f32::EPSILON);
        assert!((h - 400.0).abs() < f32::EPSILON);
    }

    // ---- SlideTheme tests --------------------------------------------------

    #[test]
    fn test_theme_mocha() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let t = SlideTheme::mocha(&pal);
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(100);
        let s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        assert_eq!(s.layout, SlideLayout::TitleSlide);
        assert!(!s.elements.is_empty());
        assert_eq!(s.transition, Transition::None);
        assert!(s.notes.is_empty());
    }

    #[test]
    fn test_slide_blank_layout_has_no_elements() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(200);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        assert!(s.elements.is_empty());
    }

    #[test]
    fn test_slide_effective_bg_default() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(300);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        assert_eq!(s.effective_bg(&theme), theme.background);
    }

    #[test]
    fn test_slide_effective_bg_override() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(400);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let red = Color::from_hex(0xF38BA8);
        s.background = Some(red);
        assert_eq!(s.effective_bg(&theme), red);
    }

    #[test]
    fn test_slide_element_by_id() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(500);
        let s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        let first_id = s.elements[0].id();
        assert!(s.element_by_id(first_id).is_some());
        assert!(s.element_by_id(99999).is_none());
    }

    #[test]
    fn test_slide_remove_element() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
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

    /// A snapshot of `slides` at `current_index`, in the Mocha theme.
    fn snap(slides: &[Slide], current_index: usize) -> Snapshot {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        Snapshot {
            slides: slides.to_vec(),
            current_index,
            theme: SlideTheme::mocha(&pal),
        }
    }

    #[test]
    fn test_undo_redo_basic() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(700);
        let s1 = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let s2 = Slide::new(2, SlideLayout::TitleSlide, &theme, &mut id_gen);

        let mut mgr = UndoManager::new(10);
        assert!(!mgr.can_undo());
        assert!(!mgr.can_redo());

        // Save state [s1], then mutate to [s1, s2].
        mgr.save(snap(std::slice::from_ref(&s1), 0));
        let slides_after = vec![s1.clone(), s2];

        // Undo: should restore [s1].
        let back = mgr.undo(snap(&slides_after, 1));
        assert!(back.is_some());
        let back = back.unwrap();
        assert_eq!(back.slides.len(), 1);
        assert_eq!(back.current_index, 0);

        // Can redo now.
        assert!(mgr.can_redo());
        let redo_snap = mgr.redo(snap(&back.slides, back.current_index));
        assert!(redo_snap.is_some());
        let redo_snap = redo_snap.unwrap();
        assert_eq!(redo_snap.slides.len(), 2);
    }

    #[test]
    fn test_undo_max_depth() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(800);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let mut mgr = UndoManager::new(3);

        for _ in 0..5 {
            mgr.save(snap(std::slice::from_ref(&s), 0));
        }
        // Only 3 saved (max depth).
        assert_eq!(mgr.undo_stack.len(), 3);
    }

    #[test]
    fn test_save_clears_redo() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(900);
        let s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        let mut mgr = UndoManager::new(10);
        mgr.save(snap(std::slice::from_ref(&s), 0));
        let _ = mgr.undo(snap(std::slice::from_ref(&s), 0));
        assert!(mgr.can_redo());
        mgr.save(snap(std::slice::from_ref(&s), 0));
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
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_sorter_mode() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_slide(SlideLayout::Blank);
        app.view = ViewMode::Sorter;
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_selected_element() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.add_textbox();
        let cmds = app.render_commands();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn test_render_with_notes_hidden() {
        let mut app = SlidesApp::new(1280.0, 720.0);
        app.show_notes = false;
        let cmds = app.render_commands();
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

    /// A long title reaches the renderer whole. The removed 30-character cut
    /// was applied before the thumbnail width was known, and the two thumbnail
    /// sizes this app draws are not the same width as each other, let alone as
    /// 30 characters.
    #[test]
    fn a_long_title_is_not_pre_truncated() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(8100);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        s.title = "Q3 Revenue by Region and Segment, with Year-on-Year Comparison".to_string();
        assert_eq!(slide_preview_text(&s), s.title);
    }

    /// A title measured in bytes rather than characters was cut at a third of
    /// its length. Nothing is cut here now, so a non-Latin title survives whole.
    #[test]
    fn a_non_latin_title_is_not_shortened() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(8200);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        s.title = "四半期ごとの売上と地域別の内訳について".to_string();
        assert_eq!(slide_preview_text(&s), s.title);
    }

    /// A `Text` command draws one line, so a multi-line text box contributes
    /// only its first line — otherwise the newline lands in the middle of a
    /// thumbnail label.
    #[test]
    fn a_multi_line_body_previews_only_its_first_line() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(8300);
        let mut s = Slide::new(1, SlideLayout::Blank, &theme, &mut id_gen);
        s.elements.push(SlideElement::TextBox {
            id: id_gen.next_id(),
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            text: String::from("Opening remarks\nand then the rest"),
            font_size: 14.0,
            color: pal.text,
            bold: false,
            centered: false,
        });
        assert_eq!(slide_preview_text(&s), "Opening remarks");
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
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
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
            color: pal.text,
            bold: false,
            centered: false,
        });
        assert_eq!(slide_preview_text(&s), "Preview text");
    }

    #[test]
    fn test_slide_preview_from_bullets() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
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
            color: pal.text,
        });
        assert_eq!(slide_preview_text(&s), "Bullet one");
    }

    // ---- Layout template element count tests -------------------------------

    #[test]
    fn test_title_slide_elements() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(7000);
        let s = Slide::new(1, SlideLayout::TitleSlide, &theme, &mut id_gen);
        // Title + Subtitle + decorative line = 3 elements.
        assert_eq!(s.elements.len(), 3);
    }

    #[test]
    fn test_title_content_elements() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(7100);
        let s = Slide::new(1, SlideLayout::TitleContent, &theme, &mut id_gen);
        // Title + bullet list = 2.
        assert_eq!(s.elements.len(), 2);
    }

    #[test]
    fn test_section_header_elements() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(7200);
        let s = Slide::new(1, SlideLayout::SectionHeader, &theme, &mut id_gen);
        // Title + bottom bar = 2.
        assert_eq!(s.elements.len(), 2);
    }

    #[test]
    fn test_two_column_elements() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(7300);
        let s = Slide::new(1, SlideLayout::TwoColumn, &theme, &mut id_gen);
        // Title + left bullets + right bullets = 3.
        assert_eq!(s.elements.len(), 3);
    }

    #[test]
    fn test_image_caption_elements() {
        let pal = Palette::from_settings(&appearance::AppearanceSettings::default());
        let theme = SlideTheme::mocha(&pal);
        let mut id_gen = IdGen::new(7400);
        let s = Slide::new(1, SlideLayout::ImageCaption, &theme, &mut id_gen);
        // Image placeholder + caption = 2.
        assert_eq!(s.elements.len(), 2);
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

        fn fills(app: &mut SlidesApp) -> Vec<Color> {
            app.render(1000.0, 700.0)
                .commands
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::FillRect { color, .. } => Some(*color),
                    _ => None,
                })
                .collect()
        }

        let mut app = SlidesApp::new(1000.0, 700.0);

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
    // ── The pointer, and what nothing reached ───────────────────────

    use guitk::probe::{self, Probe};

    impl Probe for SlidesApp {
        type Target = Target;
        type Outcome = EventResult;
        const SIZE: (f32, f32) = (1280.0, 720.0);

        /// Drawn at the app's own size, which these tests leave at `SIZE`.
        fn draw(&self, _size: (f32, f32)) -> Frame<Target> {
            self.frame()
        }

        /// A click: the button goes down and comes up where it went down.
        fn click_at(
            &mut self,
            x: f32,
            y: f32,
            button: MouseButton,
            _size: (f32, f32),
        ) -> EventResult {
            let down = self.handle_event(&mouse(x, y, MouseEventKind::Press(button)));
            let up = self.handle_event(&mouse(x, y, MouseEventKind::Release(button)));
            if down == EventResult::Consumed || up == EventResult::Consumed {
                EventResult::Consumed
            } else {
                EventResult::Ignored
            }
        }

        fn key_at(&mut self, key: &KeyEvent, _size: (f32, f32)) -> EventResult {
            self.handle_event(&Event::Key(key.clone()))
        }

        fn scroll_at(&mut self, x: f32, y: f32, dy: f32, _size: (f32, f32)) -> Option<EventResult> {
            Some(self.handle_event(&mouse(x, y, MouseEventKind::Scroll { dx: 0.0, dy })))
        }
    }

    fn mouse(x: f32, y: f32, kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent { x, y, kind })
    }

    /// A new deck: one title slide, as the window opens.
    fn fresh() -> SlidesApp {
        SlidesApp::new(1280.0, 720.0)
    }

    /// The middle of `target`'s box.
    fn centre(app: &SlidesApp, target: Target) -> (f32, f32) {
        let r = probe::rect_of(app, target).unwrap_or_else(|| panic!("{target:?} is not drawn"));
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// A press at `from`, a drag to `to` in two steps, and a release there.
    fn drag(app: &mut SlidesApp, from: (f32, f32), to: (f32, f32)) {
        app.handle_event(&mouse(
            from.0,
            from.1,
            MouseEventKind::Press(MouseButton::Left),
        ));
        app.handle_event(&mouse(
            f32::midpoint(from.0, to.0),
            f32::midpoint(from.1, to.1),
            MouseEventKind::Move,
        ));
        app.handle_event(&mouse(to.0, to.1, MouseEventKind::Move));
        app.handle_event(&mouse(
            to.0,
            to.1,
            MouseEventKind::Release(MouseButton::Left),
        ));
    }

    /// Element `id`'s box on the current slide.
    fn bounds_of(app: &SlidesApp, id: ElementId) -> (f32, f32, f32, f32) {
        app.slides[app.current_index]
            .element_by_id(id)
            .expect("the element is on the slide")
            .bounds()
    }

    /// Every toolbar button does what it says. The five buttons and the Undo
    /// and Redo beside them were drawn and could not be pressed.
    #[test]
    fn every_toolbar_button_answers_the_pointer() {
        let mut app = seeded();
        let count = app.slide_count();
        probe::click(&mut app, Target::Tool(Tool::NewSlide));
        assert!(app.layout_menu.is_some(), "+ Slide opened no menu");
        probe::click(&mut app, Target::LayoutChoice(3));
        assert_eq!(app.slide_count(), count + 1);
        assert_eq!(app.slides[app.current_index].layout, SlideLayout::Blank);
        probe::click(&mut app, Target::Tool(Tool::Duplicate));
        assert_eq!(app.slide_count(), count + 2);
        probe::click(&mut app, Target::Tool(Tool::DeleteSlide));
        assert_eq!(app.slide_count(), count + 1);
        probe::click(&mut app, Target::Tool(Tool::Undo));
        assert_eq!(app.slide_count(), count + 2);
        probe::click(&mut app, Target::Tool(Tool::Redo));
        assert_eq!(app.slide_count(), count + 1);
        let theme = app.theme.name.clone();
        probe::click(&mut app, Target::Tool(Tool::Theme));
        assert_ne!(app.theme.name, theme);
        probe::click(&mut app, Target::Tool(Tool::ToggleView));
        assert_eq!(app.view, ViewMode::Sorter);
        probe::click(&mut app, Target::Tool(Tool::ToggleView));
        assert_eq!(app.view, ViewMode::Edit);
        probe::click(&mut app, Target::Tool(Tool::Title));
        assert!(matches!(app.editing, Some((EditTarget::DeckTitle, _))));
        app.handle_event(&press(Key::Escape));
        probe::click(&mut app, Target::Tool(Tool::Export));
        assert!(app.picker.is_open());
    }

    /// Undo and redo have keys. The stack was kept by every change and read
    /// by nothing.
    #[test]
    fn undo_and_redo_have_keys() {
        let mut app = fresh();
        let before = element_count(&app);
        app.handle_event(&press(Key::T));
        assert_eq!(element_count(&app), before + 1);
        assert_eq!(app.handle_event(&press_ctrl(Key::Z)), EventResult::Consumed);
        assert_eq!(element_count(&app), before);
        assert_eq!(app.handle_event(&press_ctrl(Key::Y)), EventResult::Consumed);
        assert_eq!(element_count(&app), before + 1);
        app.handle_event(&press_ctrl(Key::Z));
        assert_eq!(
            app.handle_event(&press_ctrl_shift(Key::Z)),
            EventResult::Consumed
        );
        assert_eq!(element_count(&app), before + 1);
        // With nothing left to redo, the key is not a redraw.
        assert_eq!(app.handle_event(&press_ctrl(Key::Y)), EventResult::Ignored);
    }

    /// Undo cannot be pressed with nothing to undo: it is drawn dim and
    /// records no box.
    #[test]
    fn undo_cannot_be_pressed_with_nothing_to_undo() {
        let mut app = fresh();
        assert!(probe::rect_of(&app, Target::Tool(Tool::Undo)).is_none());
        app.handle_event(&press(Key::T));
        assert!(probe::rect_of(&app, Target::Tool(Tool::Undo)).is_some());
    }

    /// The first slide's title can be selected and typed into. Nothing
    /// selected an element but adding one, so what a layout put on a slide
    /// could not be changed at all.
    #[test]
    fn a_layouts_title_can_be_selected_and_typed_into() {
        let mut app = fresh();
        let title = app.slides[0].elements[0].id();
        assert_eq!(app.handle_event(&press(Key::Tab)), EventResult::Consumed);
        assert_eq!(app.selected_element, Some(title));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&types("Q3 plans"));
        app.handle_event(&press(Key::Escape));
        let SlideElement::TextBox { text, .. } = &app.slides[0].elements[0] else {
            panic!("the title is not a text box");
        };
        assert_eq!(text, "Q3 plans");
    }

    /// Tab walks the elements and wraps; Shift+Tab walks back.
    #[test]
    fn tab_walks_the_elements_and_wraps() {
        let mut app = fresh();
        let ids: Vec<ElementId> = app.slides[0]
            .elements
            .iter()
            .map(SlideElement::id)
            .collect();
        for id in &ids {
            app.handle_event(&press(Key::Tab));
            assert_eq!(app.selected_element, Some(*id));
        }
        app.handle_event(&press(Key::Tab));
        assert_eq!(app.selected_element, ids.first().copied());
        app.handle_event(&press_shift(Key::Tab));
        assert_eq!(app.selected_element, ids.last().copied());
    }

    /// A press selects an element, and a second press on it types into it.
    #[test]
    fn a_press_selects_and_a_second_types_into_the_element() {
        let mut app = fresh();
        let subtitle = app.slides[0].elements[1].id();
        probe::click(&mut app, Target::Element(subtitle));
        assert_eq!(app.selected_element, Some(subtitle));
        assert!(app.editing.is_none());
        probe::click(&mut app, Target::Element(subtitle));
        assert!(matches!(app.editing, Some((EditTarget::Element(e), _)) if e == subtitle));
        app.handle_event(&types("By the team"));
        // A press anywhere else finishes and keeps the words.
        let area = app.canvas_area();
        app.handle_event(&mouse(
            area.x + 4.0,
            area.y + 4.0,
            MouseEventKind::Press(MouseButton::Left),
        ));
        assert!(app.editing.is_none());
        let SlideElement::TextBox { text, .. } = &app.slides[0].elements[1] else {
            panic!("the subtitle is not a text box");
        };
        assert_eq!(text, "By the team");
    }

    /// A press on the slide or the grey around it selects nothing.
    #[test]
    fn a_press_on_the_slide_or_around_it_selects_nothing() {
        let mut app = fresh();
        app.handle_event(&press(Key::Tab));
        let area = app.canvas_area();
        assert_eq!(
            app.frame().hit_test(area.x + 4.0, area.y + 4.0),
            Some(Target::Canvas)
        );
        app.handle_event(&mouse(
            area.x + 4.0,
            area.y + 4.0,
            MouseEventKind::Press(MouseButton::Left),
        ));
        assert_eq!(app.selected_element, None);
        app.handle_event(&press(Key::Tab));
        let (cx, cy, _) = app.canvas_geometry();
        assert_eq!(
            app.frame().hit_test(cx + 4.0, cy + 4.0),
            Some(Target::SlideArea)
        );
        app.handle_event(&mouse(
            cx + 4.0,
            cy + 4.0,
            MouseEventKind::Press(MouseButton::Left),
        ));
        assert_eq!(app.selected_element, None);
    }

    /// Dragging an element moves it by the distance dragged, in slide units,
    /// and one undo puts it back.
    #[test]
    fn dragging_an_element_moves_it() {
        let mut app = fresh();
        app.handle_event(&press(Key::S));
        let id = app.selected_element.expect("the rectangle is selected");
        let (_, _, scale) = app.canvas_geometry();
        let from = centre(&app, Target::Element(id));
        drag(&mut app, from, (from.0 + 100.0, from.1 + 50.0));
        let (x, y, w, h) = bounds_of(&app, id);
        assert!((x - (200.0 + 100.0 / scale)).abs() < 0.5, "x is {x}");
        assert!((y - (200.0 + 50.0 / scale)).abs() < 0.5, "y is {y}");
        assert_eq!((w, h), (200.0, 120.0));
        app.handle_event(&press_ctrl(Key::Z));
        assert_eq!(bounds_of(&app, id), (200.0, 200.0, 200.0, 120.0));
    }

    /// A move cannot lose an element off the slide.
    #[test]
    fn a_drag_cannot_lose_an_element_off_the_slide() {
        let mut app = fresh();
        app.handle_event(&press(Key::S));
        let id = app.selected_element.unwrap();
        let from = centre(&app, Target::Element(id));
        drag(&mut app, from, (from.0 - 5000.0, from.1 - 5000.0));
        let (x, y, w, h) = bounds_of(&app, id);
        assert!(
            x + w >= 16.0 - 0.01 && y + h >= 16.0 - 0.01,
            "({x}, {y}) is off the slide"
        );
        let from = centre(&app, Target::Element(id));
        drag(&mut app, from, (5000.0, 5000.0));
        let (x, y, _, _) = bounds_of(&app, id);
        assert!(x <= SLIDE_W - 16.0 + 0.01 && y <= SLIDE_H - 16.0 + 0.01);
    }

    /// A corner handle resizes, the opposite corner stays, and the box stops
    /// at its smallest rather than turning inside out.
    #[test]
    fn a_corner_handle_resizes_and_the_opposite_corner_stays() {
        let mut app = fresh();
        app.handle_event(&press(Key::S));
        let id = app.selected_element.unwrap();
        let from = centre(&app, Target::Handle(Corner::TopLeft));
        drag(&mut app, from, (from.0 + 40.0, from.1 + 20.0));
        let (x, y, w, h) = bounds_of(&app, id);
        assert!(
            (x + w - 400.0).abs() < 0.5 && (y + h - 320.0).abs() < 0.5,
            "the far corner moved"
        );
        assert!(w < 200.0 && h < 120.0);
        let from = centre(&app, Target::Handle(Corner::TopLeft));
        drag(&mut app, from, (from.0 + 2000.0, from.1 + 2000.0));
        let (_, _, w, h) = bounds_of(&app, id);
        assert_eq!((w, h), (MIN_ELEMENT, MIN_ELEMENT));
        let from = centre(&app, Target::Handle(Corner::BottomRight));
        drag(&mut app, from, (from.0 + 30.0, from.1 + 30.0));
        let (_, _, w, h) = bounds_of(&app, id);
        assert!(w > MIN_ELEMENT && h > MIN_ELEMENT);
    }

    /// A flat line can be pressed: an empty box records no hit, and the title
    /// slide's rule has no height.
    #[test]
    fn a_flat_line_can_be_pressed() {
        let app = fresh();
        let line = app.slides[0]
            .elements
            .iter()
            .find(|e| {
                matches!(
                    e,
                    SlideElement::Shape {
                        kind: ShapeKind::Line,
                        ..
                    }
                )
            })
            .expect("the title slide has a rule")
            .id();
        let r = probe::rect_of(&app, Target::Element(line)).expect("a flat line has no hit box");
        assert!(r.h >= 5.9, "{r:?}");
    }

    /// The Insert buttons add what they name. They were drawn and could not
    /// be pressed.
    #[test]
    fn the_insert_buttons_add_what_they_name() {
        for (i, name) in INSERT_KINDS.iter().enumerate() {
            let mut app = fresh();
            let before = element_count(&app);
            assert_eq!(
                probe::click(&mut app, Target::Insert(i)),
                EventResult::Consumed,
                "{name}"
            );
            assert_eq!(element_count(&app), before + 1, "{name}");
        }
    }

    /// The property buttons change the selected text box: every value there
    /// was printed and none could be changed.
    #[test]
    fn the_property_buttons_change_the_selected_text_box() {
        let mut app = fresh();
        app.handle_event(&press(Key::T));
        let id = app.selected_element.unwrap();
        let text_box = |app: &SlidesApp| match app.slides[0].element_by_id(id) {
            Some(SlideElement::TextBox {
                font_size,
                bold,
                centered,
                color,
                ..
            }) => (*font_size, *bold, *centered, *color),
            _ => panic!("not a text box"),
        };
        let (size, bold, centred, colour) = text_box(&app);
        probe::click(&mut app, Target::Prop(Prop::Larger));
        assert_eq!(text_box(&app).0, size + 2.0);
        probe::click(&mut app, Target::Prop(Prop::Smaller));
        assert_eq!(text_box(&app).0, size);
        probe::click(&mut app, Target::Prop(Prop::Bold));
        assert_eq!(text_box(&app).1, !bold);
        probe::click(&mut app, Target::Prop(Prop::Centre));
        assert_eq!(text_box(&app).2, !centred);
        probe::click(&mut app, Target::Prop(Prop::Colour));
        assert_ne!(text_box(&app).3, colour);
        probe::click(&mut app, Target::Prop(Prop::Edit));
        assert!(matches!(app.editing, Some((EditTarget::Element(e), _)) if e == id));
        app.handle_event(&press(Key::Escape));
        probe::click(&mut app, Target::Prop(Prop::Delete));
        assert!(app.slides[0].element_by_id(id).is_none());
    }

    /// The slide's transition and background, and a shape's outline, change
    /// from the panel too.
    #[test]
    fn the_panel_changes_the_slide_and_a_shapes_outline() {
        let mut app = fresh();
        let transition = app.slides[0].transition;
        probe::click(&mut app, Target::Prop(Prop::Transition));
        assert_ne!(app.slides[0].transition, transition);
        assert_eq!(app.slides[0].background, None);
        probe::click(&mut app, Target::Prop(Prop::Background));
        assert!(app.slides[0].background.is_some());
        app.handle_event(&press(Key::S));
        let id = app.selected_element.unwrap();
        let outline = |app: &SlidesApp| match app.slides[0].element_by_id(id) {
            Some(SlideElement::Shape { stroke_width, .. }) => *stroke_width,
            _ => panic!("not a shape"),
        };
        let before = outline(&app);
        probe::click(&mut app, Target::Prop(Prop::Larger));
        assert_eq!(outline(&app), before + 1.0);
        probe::click(&mut app, Target::Prop(Prop::Smaller));
        assert_eq!(outline(&app), before);
    }

    /// `N`, or a press on the panel, types the speaker notes. They were
    /// shown and exported and could not be written.
    #[test]
    fn the_speaker_notes_can_be_written() {
        let mut app = fresh();
        assert_eq!(app.handle_event(&press(Key::N)), EventResult::Consumed);
        app.handle_event(&types("Open with the numbers"));
        app.handle_event(&press_shift(Key::Enter));
        app.handle_event(&types("then the plan"));
        app.handle_event(&press(Key::Escape));
        assert_eq!(app.current_notes(), "Open with the numbers\nthen the plan");
        app.handle_event(&press_ctrl(Key::Z));
        assert_eq!(app.current_notes(), "");
        probe::click(&mut app, Target::Notes);
        assert!(matches!(app.editing, Some((EditTarget::Notes, _))));
    }

    /// A bullet list is typed one line per bullet. Only text boxes could be
    /// typed into, so every Title + Content slide said "First point" for good.
    #[test]
    fn a_bullet_list_is_typed_one_line_per_bullet() {
        let mut app = fresh();
        app.handle_event(&press_ctrl(Key::N));
        app.handle_event(&press(Key::Tab));
        app.handle_event(&press(Key::Tab));
        assert!(matches!(
            app.selected(),
            Some(SlideElement::BulletList { .. })
        ));
        app.handle_event(&press(Key::Enter));
        let Some((_, seed)) = &app.editing else {
            panic!("Enter did not start typing into the list");
        };
        assert!(seed.is_empty(), "the list's prompts were kept: {seed:?}");
        app.handle_event(&types("Alpha"));
        app.handle_event(&press_shift(Key::Enter));
        app.handle_event(&press_shift(Key::Enter));
        app.handle_event(&types("Beta"));
        app.handle_event(&press(Key::Escape));
        let Some(SlideElement::BulletList { items, .. }) = app.selected() else {
            panic!("the list is gone");
        };
        assert_eq!(items, &vec![String::from("Alpha"), String::from("Beta")]);
        app.handle_event(&press(Key::Enter));
        assert!(matches!(&app.editing, Some((_, words)) if words == "Alpha\nBeta"));
    }

    /// An image placeholder's label can be typed.
    #[test]
    fn an_image_label_can_be_typed() {
        let mut app = fresh();
        app.handle_event(&press(Key::I));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&types("Revenue chart"));
        app.handle_event(&press(Key::Escape));
        let Some(SlideElement::Image {
            placeholder_label, ..
        }) = app.selected()
        else {
            panic!("the image is gone");
        };
        assert_eq!(placeholder_label, "Revenue chart");
    }

    /// With an element selected the arrows move it and Shift resizes it; with
    /// none, Left and Right change slides.
    #[test]
    fn the_arrows_move_the_selected_element_or_change_slides() {
        let mut app = seeded();
        app.handle_event(&press(Key::Home));
        app.handle_event(&press(Key::T));
        let id = app.selected_element.unwrap();
        let (x, y, w, h) = bounds_of(&app, id);
        app.handle_event(&press(Key::Right));
        app.handle_event(&press_ctrl(Key::Down));
        assert_eq!(bounds_of(&app, id), (x + NUDGE, y + 1.0, w, h));
        app.handle_event(&press_shift(Key::Right));
        app.handle_event(&press_shift(Key::Up));
        assert_eq!(
            bounds_of(&app, id),
            (x + NUDGE, y + 1.0, w + NUDGE, h - NUDGE)
        );
        assert_eq!(
            app.current_index, 0,
            "an arrow on an element changed slides"
        );
        app.handle_event(&press(Key::Escape));
        assert_eq!(app.selected_element, None);
        app.handle_event(&press(Key::Right));
        assert_eq!(app.current_index, 1);
    }

    /// Ctrl+M, and the + Slide button, reach every layout: Ctrl+N made a
    /// Title + Content slide and nothing made any other kind.
    #[test]
    fn every_layout_can_be_added() {
        for (i, layout) in SlideLayout::all().iter().enumerate() {
            let mut app = fresh();
            app.handle_event(&press_ctrl(Key::M));
            let digit = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
            ][i];
            app.handle_event(&press(digit));
            assert_eq!(app.slides[app.current_index].layout, *layout);
            assert!(app.layout_menu.is_none());
        }
        let mut app = fresh();
        app.handle_event(&press_ctrl(Key::M));
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Down));
        app.handle_event(&press(Key::Enter));
        assert_eq!(app.slides[app.current_index].layout, SlideLayout::all()[3]);
        app.handle_event(&press_ctrl(Key::M));
        let count = app.slide_count();
        // The card's own padding: its middle is a row.
        let card = probe::rect_of(&app, Target::Menu).unwrap();
        assert_eq!(
            app.frame().hit_test(card.x + 3.0, card.y + 3.0),
            Some(Target::Menu)
        );
        app.handle_event(&mouse(
            card.x + 3.0,
            card.y + 3.0,
            MouseEventKind::Press(MouseButton::Left),
        ));
        assert!(
            app.layout_menu.is_some(),
            "a press on the card closed the menu"
        );
        probe::click(&mut app, Target::MenuBackdrop);
        assert!(app.layout_menu.is_none());
        assert_eq!(app.slide_count(), count);
    }

    /// A theme change restyles what the old theme styled, and leaves what the
    /// user chose; one undo brings both back.
    #[test]
    fn a_theme_change_restyles_what_the_theme_styled() {
        let mut app = fresh();
        let mocha = app.theme.clone();
        app.handle_event(&press(Key::T));
        let chosen = app.selected_element.unwrap();
        if let Some(SlideElement::TextBox { color, .. }) = app.slides[0].element_by_id_mut(chosen) {
            *color = EXTRA_COLOURS[2];
        }
        let title = app.slides[0].elements[0].id();
        app.handle_event(&press_ctrl(Key::T));
        let light = app.theme.clone();
        assert_ne!(
            light.title_color, mocha.title_color,
            "the next theme has the same title colour"
        );
        let colour_of = |app: &SlidesApp, id| match app.slides[0].element_by_id(id) {
            Some(SlideElement::TextBox { color, .. }) => *color,
            _ => panic!("not a text box"),
        };
        assert_eq!(colour_of(&app, title), light.title_color);
        assert_eq!(colour_of(&app, chosen), EXTRA_COLOURS[2]);
        app.handle_event(&press_ctrl(Key::Z));
        assert_eq!(app.theme.name, mocha.name);
        assert_eq!(colour_of(&app, title), mocha.title_color);
    }

    /// A text box draws each of its lines.
    #[test]
    fn a_text_box_draws_each_of_its_lines() {
        let mut app = fresh();
        app.handle_event(&press(Key::T));
        app.handle_event(&press(Key::Enter));
        app.handle_event(&types("first"));
        app.handle_event(&press_shift(Key::Enter));
        app.handle_event(&types("second"));
        app.handle_event(&press(Key::Escape));
        let texts: Vec<String> = app
            .render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        assert!(texts.iter().any(|t| t == "first"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "second"), "{texts:?}");
    }

    /// Centred text is centred on its box. It was nudged a tenth of the way
    /// in and called centred.
    #[test]
    fn centred_text_is_centred() {
        let app = fresh();
        let title = app.slides[0].elements[0].id();
        let (bx, _, bw, _) = bounds_of(&app, title);
        let (cx, _, scale) = app.canvas_geometry();
        let Some(SlideElement::TextBox {
            text, font_size, ..
        }) = app.slides[0].element_by_id(title)
        else {
            panic!("not a text box");
        };
        let (text, size) = (text.clone(), font_size * scale);
        let drawn = app
            .render_commands()
            .into_iter()
            .find_map(|c| match c {
                // At the slide's size: the thumbnail beside it shows the
                // same words.
                RenderCommand::Text {
                    text: t,
                    x,
                    font_size: s,
                    ..
                } if t == text && (s - size).abs() < 0.01 => Some(x),
                _ => None,
            })
            .expect("the title is drawn");
        let width = guitk::text::measure(&text, size, FontWeightHint::Bold);
        let middle = cx + (bx + bw / 2.0) * scale;
        assert!(
            (drawn + width / 2.0 - middle).abs() < 1.0,
            "drawn at {drawn}"
        );
    }

    /// The thumbnail column scrolls, and follows the current slide. It
    /// clipped its thumbnails and never scrolled.
    #[test]
    fn the_thumbnail_column_scrolls_and_follows_the_current_slide() {
        let mut app = fresh();
        for _ in 0..19 {
            app.handle_event(&press_ctrl(Key::N));
        }
        assert!(probe::rect_of(&app, Target::Thumb(19)).is_some());
        app.handle_event(&press(Key::Home));
        assert!(probe::rect_of(&app, Target::Thumb(0)).is_some());
        assert!(probe::rect_of(&app, Target::Thumb(19)).is_none());
        for _ in 0..40 {
            probe::scroll_at_point(&mut app, Target::Sidebar, -3.0);
        }
        probe::click(&mut app, Target::Thumb(19));
        assert_eq!(app.current_index, 19);
    }

    /// Dragging a thumbnail moves its slide, and one undo puts it back.
    #[test]
    fn dragging_a_thumbnail_moves_its_slide() {
        let mut app = seeded();
        app.handle_event(&press(Key::Home));
        let moving = app.slide_id_at(0);
        let (from, to) = (
            centre(&app, Target::Thumb(0)),
            centre(&app, Target::Thumb(2)),
        );
        drag(&mut app, from, to);
        assert_eq!(app.slide_id_at(2), moving);
        assert_eq!(app.current_index, 2);
        app.handle_event(&press_ctrl(Key::Z));
        assert_eq!(app.slide_id_at(0), moving);
    }

    /// The sorter scrolls, a press chooses a slide, and a second opens it.
    #[test]
    fn the_sorter_scrolls_and_a_second_press_opens_a_slide() {
        let mut app = fresh();
        for _ in 0..30 {
            app.handle_event(&press_ctrl(Key::N));
        }
        app.handle_event(&press(Key::Num2));
        app.handle_event(&press(Key::Home));
        assert!(probe::rect_of(&app, Target::SorterThumb(30)).is_none());
        for _ in 0..60 {
            probe::scroll_at_point(&mut app, Target::SorterGrid, -3.0);
        }
        probe::click(&mut app, Target::SorterThumb(30));
        assert_eq!(app.current_index, 30);
        assert_eq!(app.view, ViewMode::Sorter);
        probe::click(&mut app, Target::SorterThumb(30));
        assert_eq!(app.view, ViewMode::Edit);
    }

    /// The pointer lights what it is over.
    #[test]
    fn hovering_a_button_lights_it() {
        let mut app = fresh();
        let (x, y) = centre(&app, Target::Tool(Tool::Duplicate));
        assert_eq!(
            app.handle_event(&mouse(x, y, MouseEventKind::Move)),
            EventResult::Consumed
        );
        assert_eq!(app.hover, Some(Target::Tool(Tool::Duplicate)));
        app.handle_event(&mouse(0.0, 0.0, MouseEventKind::Leave));
        assert_eq!(app.hover, None);
    }

    /// The shortcut list is modal, and a press puts it away.
    #[test]
    fn the_shortcut_list_is_modal() {
        let mut app = fresh();
        let before = element_count(&app);
        app.handle_event(&press(Key::F1));
        assert_eq!(app.handle_event(&press(Key::T)), EventResult::Ignored);
        assert_eq!(
            element_count(&app),
            before,
            "a key reached the slide behind the list"
        );
        probe::click(&mut app, Target::HelpCard);
        assert!(!app.show_help);
    }

    /// Leaving the window ends a drag: the release would never arrive.
    #[test]
    fn leaving_the_window_ends_a_drag() {
        let mut app = fresh();
        app.handle_event(&press(Key::S));
        let id = app.selected_element.unwrap();
        let (x, y) = centre(&app, Target::Element(id));
        app.handle_event(&mouse(x, y, MouseEventKind::Press(MouseButton::Left)));
        app.handle_event(&mouse(x + 40.0, y, MouseEventKind::Move));
        app.handle_event(&mouse(x + 40.0, y, MouseEventKind::Leave));
        assert!(app.drag.is_none());
        let moved = bounds_of(&app, id);
        app.handle_event(&mouse(x + 200.0, y, MouseEventKind::Move));
        assert_eq!(
            bounds_of(&app, id),
            moved,
            "the element followed a pointer with no button down"
        );
    }

    /// An arrow's head points back along its line. It was two strokes at
    /// fixed angles, lopsided on anything but a diagonal.
    #[test]
    fn an_arrowhead_is_symmetric_about_its_line() {
        let mut app = fresh();
        app.handle_event(&press(Key::A));
        let id = app.selected_element.unwrap();
        if let Some(e) = app.slides[0].element_by_id_mut(id) {
            e.set_size(200.0, 0.0);
        }
        let (cx, cy, scale) = app.canvas_geometry();
        let (x, y, w, _) = bounds_of(&app, id);
        let (tip_x, tip_y) = (cx + (x + w) * scale, cy + y * scale);
        let heads: Vec<f32> = app
            .render_commands()
            .into_iter()
            .filter_map(|c| match c {
                RenderCommand::Line { x1, y1, y2, .. }
                    if (x1 - tip_x).abs() < 0.01 && (y1 - tip_y).abs() < 0.01 =>
                {
                    Some(y2 - tip_y)
                }
                _ => None,
            })
            .collect();
        assert_eq!(heads.len(), 2, "{heads:?}");
        assert!((heads[0] + heads[1]).abs() < 0.01, "{heads:?}");
    }
}
