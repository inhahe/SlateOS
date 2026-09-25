//! Desktop Icons — icon layer sitting beneath windows, above the wallpaper.
//!
//! This module manages a grid of desktop icons (files, folders, shortcuts,
//! system items) that users can click, drag, rename, and double-click to open.
//!
//! # Placement
//!
//! `design.txt` asks for "two options for desktop icon placement: snap to
//! grid, or place freely". Both are here, with auto-arrange as a third, and the
//! user picks between them from the desktop's right-click View menu; see
//! [`ArrangementMode`]. The rules each one keeps:
//!
//! - **Free**: an icon stays exactly where it is dropped, wholly on the
//!   desktop.
//! - **Snap to grid**: every icon has a cell of its own. A dropped icon lands in
//!   the cell it mostly covers, or the free cell nearest that one -- never on
//!   top of another icon, where it would hide it.
//! - **Auto-arrange**: the icons are packed down the columns from the top-left
//!   with no gaps, and a drop moves an icon to a new place in that order.
//!
//! The grid starts `EDGE_PADDING` in from the top-left of the screen, and its
//! pitch follows the icon size ([`cell_for_glyph`]), so a size change moves
//! every icon with its cell. What a drop will do is worked out by one function,
//! `drop_plan`, which both the release and the outline drawn during the drag
//! call -- the outline cannot promise a cell the drop then does not use.
//!
//! The layout is saved to `deskicons.yaml`: each icon's position, the grid those
//! positions were laid out on, and the arrangement.
//!
//! # Integration
//!
//! ```ignore
//! let mut icon_layer = DesktopIconLayer::new(1920, 1080, 40); // screen_w, screen_h, taskbar_h
//! icon_layer.populate_defaults();
//! icon_layer.load_layout();
//!
//! // Each frame:
//! let commands = icon_layer.render(&palette);
//!
//! // Forward mouse/key events:
//! icon_layer.handle_mouse_down(x, y, button, ctrl_held);
//! icon_layer.handle_mouse_move(x, y, ctrl_held);
//! if icon_layer.handle_mouse_up(x, y, button) {
//!     icon_layer.save_layout()?; // something moved
//! }
//! icon_layer.handle_key(key, ctrl_held);
//! icon_layer.handle_double_click(x, y);
//! ```

use appearance::Palette;
use core::num::NonZeroU32;
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use yamldoc::Document;

// ============================================================================
// Colour
// ============================================================================
//
// Sixteen `const … : Color` used to live here, all Catppuccin Mocha, which is
// why the desktop's icon layer stayed dark on a Light desktop. They are gone;
// `render` takes the `&Palette` the shell resolved. See known-issues.md
// `TD-C-FORTY-NINE-SHELL-MODULES-CARRY-THEIR-OWN-COPY-OF-THE-PALETTE`.
//
// Three groups, and only the third needed a decision:
//
// - **The six translucent ones** — selection fill and border, rubber-band fill
//   and border, drop target, label shadow — mapped onto `Palette`'s helpers
//   alpha for alpha (`selection_fill`, `selection_border`, `hint_fill`,
//   `hint_border`, `drop_target`, `text_shadow`). They were written from those
//   helpers' values in the first place; this is the copy going back where it
//   came from, and the selection ones now follow the user's accent, which the
//   hardcoded blue never could.
// - **The nine icon-type hues** stay categorical. A folder is yellow, the
//   recycle bin is red, an executable is peach — that is a legend the user
//   reads, so it must not collapse toward whatever accent they picked. A Red
//   desktop that painted the recycle bin and every shortcut the same colour
//   would be worse than one that ignored the setting.
// - **The labels do not follow the mode at all**, and that is the one real
//   decision here. They are drawn on the *wallpaper* — an arbitrary
//   photograph, not a palette surface — under a black shadow that exists to
//   make them legible against it. `p.text` would be dark on Latte, and dark
//   text under a black shadow is legible against nothing. So they read
//   `p.on_wallpaper()` / `p.on_wallpaper_dim()`, which are pale in both modes
//   for exactly the reason `text_shadow` is black in both.

// ============================================================================
// Constants
// ============================================================================

/// Default grid cell width in pixels.
const DEFAULT_GRID_WIDTH: u32 = 80;
/// Default grid cell height in pixels.
const DEFAULT_GRID_HEIGHT: u32 = 90;
/// The icon size the layer starts at, in pixels: the smallest the icon-size
/// setting offers, and the size every icon was drawn at before the layer read
/// that setting. [`cell_for_glyph`] of this is exactly the default grid, so a
/// layer nobody resizes lays out as it always did.
const DEFAULT_GLYPH_PX: u32 = 32;
/// Label font size.
const LABEL_FONT_SIZE: f32 = 11.0;
/// Maximum label width (for centering and truncation).
const LABEL_MAX_WIDTH: f32 = 72.0;
/// Maximum number of label lines.
const LABEL_MAX_LINES: usize = 2;
/// Drag threshold in pixels (squared, to avoid sqrt).
const DRAG_THRESHOLD_SQ: f32 = 25.0;
/// Padding from top of grid cell to icon glyph.
const ICON_TOP_PADDING: f32 = 8.0;
/// Padding from screen edges.
const EDGE_PADDING: u32 = 8;

/// The grid cell that holds a `px`-pixel icon and its two-line label.
///
/// Derived rather than tabulated, so that every size the setting offers -- and
/// any it offers later -- gets a cell that fits it: a 96-pixel glyph in the
/// 80-pixel default cell would overlap its neighbours. At the default 32 pixels
/// this is exactly [`DEFAULT_GRID_WIDTH`] by [`DEFAULT_GRID_HEIGHT`]; above it,
/// width grows one-for-one with the glyph (the label under it gets the room
/// too) and height grows with the glyph alone, since the label is still two
/// lines of the same text.
#[must_use]
pub fn cell_for_glyph(px: u32) -> (u32, u32) {
    let grow = px.saturating_sub(DEFAULT_GLYPH_PX);
    (
        DEFAULT_GRID_WIDTH.saturating_add(grow),
        DEFAULT_GRID_HEIGHT.saturating_add(grow),
    )
}

/// A pixel count as a drawing coordinate.
///
/// One place for the cast, so its lint is argued once: an icon or a cell is at
/// most a few hundred pixels, far inside `f32`'s exact-integer range.
#[allow(clippy::cast_precision_loss)]
fn px_f32(px: u32) -> f32 {
    px as f32
}

/// A pointer distance in whole pixels, to the nearest.
///
/// `as` from a float saturates at the integer's limits and turns NaN into 0 --
/// for a drag delta, which should be neither, that is no panic and no wrap.
/// The old drop truncated instead, so a drag of 79.9 pixels moved 79.
fn whole_pixels(v: f32) -> i32 {
    v.round() as i32
}

// ============================================================================
// Types
// ============================================================================

/// Unique identifier for a desktop icon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IconId(pub u64);

/// The type/category of a desktop icon, determining its visual glyph and behavior.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconType {
    Folder,
    File,
    Shortcut,
    Drive,
    RecycleBin,
    Computer,
    Document,
    Image,
    Executable,
}

// The spellings are the layout file's, for a shortcut the user added: renaming
// one changes how every saved shortcut of that kind is drawn at the next login.
settingsfile::yaml_enum!(IconType {
    Folder => "folder",
    File => "file",
    Shortcut => "shortcut",
    Drive => "drive",
    RecycleBin => "recycle-bin",
    Computer => "computer",
    Document => "document",
    Image => "image",
    Executable => "executable",
});

impl IconType {
    /// Every type, for the file-spelling round trip.
    pub const ALL: [Self; 9] = [
        Self::Folder,
        Self::File,
        Self::Shortcut,
        Self::Drive,
        Self::RecycleBin,
        Self::Computer,
        Self::Document,
        Self::Image,
        Self::Executable,
    ];

    /// Unicode glyph representing this icon type.
    fn glyph(self) -> &'static str {
        match self {
            Self::Folder => "\u{1F4C1}",     // folder
            Self::File => "\u{1F4C4}",       // page facing up
            Self::Shortcut => "\u{1F517}",   // link
            Self::Drive => "\u{1F4BE}",      // floppy disk (drive)
            Self::RecycleBin => "\u{1F5D1}", // wastebasket
            Self::Computer => "\u{1F4BB}",   // laptop
            Self::Document => "\u{1F4DD}",   // memo
            Self::Image => "\u{1F5BC}",      // framed picture
            Self::Executable => "\u{2699}",  // gear
        }
    }

    /// The hue that means this icon's type, in `p`'s mode.
    ///
    /// Categorical, not accented: these nine are a legend the user reads,
    /// so they stay nine distinguishable colours whatever the desktop is
    /// themed around.
    fn color(self, p: &Palette) -> Color {
        match self {
            Self::Folder => p.yellow,
            Self::File => p.text,
            Self::Shortcut => p.sapphire,
            Self::Drive => p.overlay0,
            Self::RecycleBin => p.red,
            Self::Computer => p.blue,
            Self::Document => p.green,
            Self::Image => p.mauve,
            Self::Executable => p.peach,
        }
    }
}

/// Action associated with a desktop icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconAction {
    /// Open a path (file or directory).
    ///
    /// A `PathBuf`, not a `String`. This held text until 2026-09-16 and was
    /// built with `to_string_lossy`, so a home directory containing bytes that
    /// are not UTF-8 produced an icon that launched a path which does not
    /// exist -- and the bytes were available: the caller reads `HOME` with
    /// `var_os` and already had a `PathBuf` in hand before flattening it.
    OpenPath(PathBuf),
    /// A destination this system defines rather than a path: [`THIS_PC`] or
    /// [`RECYCLE_BIN`]. The shell says what each one opens.
    LaunchSystem(String),
    /// Custom action string.
    Custom(String),
}

/// The "This PC" icon's destination: the machine's filesystem, from the top.
///
/// Spelled as it always was -- like a command line, which it never was run
/// as -- because it is also the icon's storage key, `system:explorer
/// --computer`, in every saved layout, and a new spelling would forget where
/// the user put the icon.
pub const THIS_PC: &str = "explorer --computer";

/// The "Recycle Bin" icon's destination. The spelling is kept for the reason
/// [`THIS_PC`]'s is.
pub const RECYCLE_BIN: &str = "explorer --recycle-bin";

/// The file the desktop icon layout lives in, under the user's config
/// directory.
pub const CONFIG_NAME: &str = "deskicons";

/// The mapping inside that file: icon key -> `{x, y}`.
const POSITIONS_KEY: &str = "positions";

/// The grid pitch the positions were saved on: `{w, h}`, in pixels.
///
/// Positions are pixels, and pixels mean something only at the pitch they were
/// laid out on. Until 2026-09-24 there was one pitch, so the file never said
/// which; once the icon-size setting changed the grid, a layout saved at one
/// size and read at another came back at the old pitch -- rows 90 pixels apart
/// inside cells 154 tall, icons on top of each other. A file without this key
/// was therefore saved on the default grid, which is how it is read.
const GRID_KEY: &str = "grid";

/// How the icons are placed: an [`ArrangementMode`]'s file spelling.
const ARRANGEMENT_KEY: &str = "arrangement";

/// The shortcuts the user added: icon key -> `{label, type}`.
///
/// Keyed by the same key the positions are, which is the icon's *action* in
/// text -- `path:/usr/bin/editor` -- so the action is recovered from the key
/// (`DesktopIconLayer::action_for_key`) and is not written twice.
const SHORTCUTS_KEY: &str = "shortcuts";

/// A single desktop icon.
#[derive(Clone, Debug)]
pub struct DesktopIcon {
    pub id: IconId,
    /// Position on the desktop (top-left of the icon cell).
    pub x: i32,
    pub y: i32,
    /// Display label (file/folder name).
    pub label: String,
    /// Type determines the glyph and accent color.
    pub icon_type: IconType,
    /// What happens when the icon is activated (double-click / Enter).
    pub action: IconAction,
    /// Whether this icon is currently selected.
    pub selected: bool,
    /// Whether the user put this icon here -- "Add to desktop" from the start
    /// menu or the taskbar -- as opposed to its being one of the defaults.
    ///
    /// An added icon can be removed and is written into the layout file, so
    /// that it is still there after a login; a default is recreated at every
    /// login by [`DesktopIconLayer::populate_defaults`] and stays.
    pub added: bool,
}

/// Describes the current interaction state of the icon layer.
#[derive(Clone, Debug)]
enum InteractionState {
    /// No interaction in progress.
    Idle,
    /// Mouse is down, waiting to see if it becomes a drag.
    PendingDrag {
        start_x: f32,
        start_y: f32,
        /// The icon the press landed on. See `Dragging::anchor`.
        anchor: IconId,
        /// Icons being considered for drag.
        icon_ids: Vec<IconId>,
    },
    /// Dragging selected icons.
    Dragging {
        start_x: f32,
        start_y: f32,
        current_x: f32,
        current_y: f32,
        /// The icon the press landed on -- the one under the pointer. On the
        /// grid it is placed first, so it lands where the pointer is and the
        /// rest of the selection fits around it; under auto-arrange, where it
        /// is dropped decides where the selection goes in the order.
        anchor: IconId,
        /// Original positions of dragged icons (id, orig_x, orig_y).
        originals: Vec<(IconId, i32, i32)>,
    },
    /// Rubber-band selection in progress (started on empty desktop area).
    RubberBand {
        start_x: f32,
        start_y: f32,
        current_x: f32,
        current_y: f32,
    },
}

/// What a drop would do: `DesktopIconLayer::drop_plan`'s answer, which the
/// release carries out and the drag outline draws.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DropPlan {
    /// Where each dragged icon ends up.
    targets: Vec<(IconId, i32, i32)>,
    /// Under auto-arrange, every icon in the order it is packed in afterwards.
    order: Option<Vec<IconId>>,
}

/// How icons are placed on the desktop.
///
/// `design.txt` asks for "two options for desktop icon placement: snap to
/// grid, or place freely". The third is the grid with the gaps taken out,
/// which every desktop that offers a grid also offers.
///
/// Three states rather than the two switches the desktop menu shows ("Auto
/// arrange icons", "Align icons to grid"), because two independent switches
/// have a fourth state -- arranged but not aligned -- that means nothing: a
/// packed arrangement is on the grid by construction. The menu maps its
/// switches onto these the way every desktop with both does: turning
/// auto-arrange on aligns, and turning alignment off stops arranging.
///
/// Until 2026-09-25 there was no free placement at all. The two states were
/// `FreeWithSnap`, which despite the name snapped every drop, and
/// `AutoArrange`, which re-sorted by name after every drop and so put a
/// dragged icon straight back -- while `roadmap.md` ticked "free placement +
/// auto-arrange modes" as done and nothing let the user choose either.
///
/// Auto-arrange keeps the *user's* order and packs it; sorting is the separate
/// [`DesktopIconLayer::arrange_by_name`]. `design-decisions.md` §869 has why
/// that and not a mode that stays sorted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ArrangementMode {
    /// Icons stay exactly where they are dropped, anywhere on the desktop.
    Free,
    /// Every icon has a grid cell of its own; a dropped icon settles into the
    /// free cell nearest where it was let go.
    #[default]
    SnapToGrid,
    /// Icons are packed down the columns from the top-left with no gaps, in an
    /// order the user changes by dragging.
    AutoArrange,
}

// The spellings are the file format: renaming one reverts every user who chose
// it to the default at their next login.
settingsfile::yaml_enum!(ArrangementMode {
    Free => "free",
    SnapToGrid => "grid",
    AutoArrange => "auto",
});

impl ArrangementMode {
    /// Every mode.
    pub const ALL: [Self; 3] = [Self::Free, Self::SnapToGrid, Self::AutoArrange];

    /// Whether icons sit in grid cells in this mode.
    #[must_use]
    pub const fn aligns_to_grid(self) -> bool {
        !matches!(self, Self::Free)
    }
}

/// Result of an icon interaction (returned to the desktop shell).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconEvent {
    /// An icon was activated (double-click or Enter).
    Activate(IconId, IconAction),
    /// A context menu was requested at a position.
    ContextMenu {
        x: i32,
        y: i32,
        icon_id: Option<IconId>,
    },
    /// Icons were deleted (moved to recycle bin).
    Delete(Vec<IconId>),
    /// Rename was initiated for an icon.
    BeginRename(IconId),
    /// No event.
    None,
}

/// Grid configuration for icon placement.
///
/// The cell size is held privately and is never zero. It used to be two public
/// `u32`s, and half the methods here defended against a zero — `columns_in` and
/// `rows_in` returned 0 — while the other half divided by it and panicked.
/// There was even a test for the zero grid, which called only the two guarded
/// methods. Making the state unrepresentable is what removes the question:
/// there is one place a cell size is admitted, and it clamps.
///
/// It is `NonZeroU32` rather than a `u32` that the constructor happens to
/// clamp, because those are not the same claim. The second is a promise kept
/// by one function that a reader has to go and find; the first is a fact the
/// compiler carries to every division in this file, and it is what lets
/// [`GridConfig::columns_in`] divide without a guard of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridConfig {
    cell_width: NonZeroU32,
    cell_height: NonZeroU32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self::new(DEFAULT_GRID_WIDTH, DEFAULT_GRID_HEIGHT)
    }
}

impl GridConfig {
    /// A grid of `cell_width` by `cell_height` pixel cells.
    ///
    /// A zero in either dimension is raised to one. A grid of zero-sized cells
    /// has no meaning — every position would be in every cell — so there is
    /// nothing to preserve by honouring it, and a one-pixel cell is the
    /// degenerate case that still answers every question consistently.
    #[must_use]
    pub const fn new(cell_width: u32, cell_height: u32) -> Self {
        Self {
            cell_width: match NonZeroU32::new(cell_width) {
                Some(w) => w,
                None => NonZeroU32::MIN,
            },
            cell_height: match NonZeroU32::new(cell_height) {
                Some(h) => h,
                None => NonZeroU32::MIN,
            },
        }
    }

    /// Cell width in pixels. Never zero.
    #[must_use]
    pub const fn cell_width(&self) -> u32 {
        self.cell_width.get()
    }

    /// Cell height in pixels. Never zero.
    #[must_use]
    pub const fn cell_height(&self) -> u32 {
        self.cell_height.get()
    }

    /// Cell width as a signed value, for mixing with pixel positions.
    fn cell_width_i32(&self) -> i32 {
        i32::try_from(self.cell_width.get()).unwrap_or(i32::MAX)
    }

    /// Cell height as a signed value.
    fn cell_height_i32(&self) -> i32 {
        i32::try_from(self.cell_height.get()).unwrap_or(i32::MAX)
    }

    /// Snap a position to the origin of the cell containing it.
    #[must_use]
    pub fn snap(&self, x: i32, y: i32) -> (i32, i32) {
        let (col, row) = self.to_cell(x, y);
        self.from_cell(col, row)
    }

    /// Convert pixel position to grid column/row.
    ///
    /// `div_euclid` is floor division, which is what a grid coordinate needs:
    /// pixel -1 belongs to cell -1, not to cell 0. The hand-rolled
    /// `(x - w + 1) / w` this replaces computed the same thing for ordinary
    /// inputs and overflowed near `i32::MIN`, where the subtraction wraps.
    // Kept as &self for symmetry with the sibling Grid methods
    // (snap, columns_in, rows_in) — all take &self for a consistent API.
    #[allow(clippy::wrong_self_convention)]
    #[must_use]
    pub fn to_cell(&self, x: i32, y: i32) -> (i32, i32) {
        (
            x.div_euclid(self.cell_width_i32()),
            y.div_euclid(self.cell_height_i32()),
        )
    }

    /// Convert grid column/row to the pixel position of that cell's top-left.
    // Despite the `from_*` name this is an inverse of `to_cell` that needs
    // the grid's cell dimensions, so it stays as a method.
    #[allow(clippy::wrong_self_convention)]
    #[must_use]
    pub fn from_cell(&self, col: i32, row: i32) -> (i32, i32) {
        (
            col.saturating_mul(self.cell_width_i32()),
            row.saturating_mul(self.cell_height_i32()),
        )
    }

    /// Number of whole columns that fit within a given width.
    ///
    /// Dividing by the `NonZeroU32` itself rather than by `.get()` uses the
    /// `Div<NonZeroU32> for u32` impl, which cannot panic. Calling `.get()`
    /// first would hand the compiler a plain `u32` and throw the proof away
    /// one expression before the division that needs it. That costs `const`,
    /// since operator impls are not const — and no caller wanted it.
    #[must_use]
    pub fn columns_in(&self, width: u32) -> u32 {
        width / self.cell_width
    }

    /// Number of whole rows that fit within a given height.
    #[must_use]
    pub fn rows_in(&self, height: u32) -> u32 {
        height / self.cell_height
    }

    // The desktop's grid is this one moved `EDGE_PADDING` in from the corner
    // of the screen. The three methods below are that grid; the public ones
    // above are the bare pitch, which starts at 0 and which the tests of
    // snapping and cell arithmetic are written against. They are methods here
    // rather than on the layer so that a grid other than the layer's own --
    // the one a size change is leaving, the one a saved file was written on --
    // can answer the same questions.

    /// Top-left of cell `(col, row)` of the desktop's grid.
    fn desktop_origin(&self, col: i32, row: i32) -> (i32, i32) {
        let (x, y) = self.from_cell(col, row);
        (
            x.saturating_add(EDGE_PADDING_I32),
            y.saturating_add(EDGE_PADDING_I32),
        )
    }

    /// The desktop cell an icon whose top-left is `(x, y)` belongs to: the
    /// one its centre is over.
    ///
    /// The centre rather than the corner, for two reasons. A dropped icon
    /// should land in the cell it mostly covers; by the corner, it had to be
    /// dragged a whole cell before it moved one, and dragging it a pixel up
    /// or left moved it a whole cell back. And a position written before
    /// 2026-09-25 sat on the bare cell origin, `EDGE_PADDING` up and to the
    /// left of where the grid now puts it -- by the corner that reads as the
    /// cell before, by the centre as the same cell.
    fn desktop_cell(&self, x: i32, y: i32) -> (i32, i32) {
        let half_w = i32::try_from(self.cell_width().checked_div(2).unwrap_or(0)).unwrap_or(0);
        let half_h = i32::try_from(self.cell_height().checked_div(2).unwrap_or(0)).unwrap_or(0);
        self.to_cell(
            x.saturating_add(half_w).saturating_sub(EDGE_PADDING_I32),
            y.saturating_add(half_h).saturating_sub(EDGE_PADDING_I32),
        )
    }

    /// Where a position laid out on this grid belongs on `to`: the same place
    /// relative to the grid, scaled by the change in pitch.
    ///
    /// An icon on a cell lands on the same cell -- the grid's origin maps to
    /// itself and each cell's origin to that cell's origin -- and an icon
    /// placed freely keeps its place among the cells around it, so a free
    /// arrangement grows and shrinks with its icons instead of piling up
    /// (bigger icons, same pixels) or spreading out.
    ///
    /// Rounded to the nearest pixel, which makes a size change followed by the
    /// reverse one a round trip whenever the first step grew the grid: each
    /// scaled offset is within half a pixel of exact, and scaling back shrinks
    /// that error below half a pixel again.
    fn rescale_to(&self, to: Self, x: i32, y: i32) -> (i32, i32) {
        if *self == to {
            return (x, y);
        }
        (
            rescale_axis(x, self.cell_width(), to.cell_width()),
            rescale_axis(y, self.cell_height(), to.cell_height()),
        )
    }
}

/// `EDGE_PADDING` as a signed pixel offset. The cast is exact: the value is 8.
const EDGE_PADDING_I32: i32 = EDGE_PADDING as i32;

/// One axis of [`GridConfig::rescale_to`]: `v`, measured from the desktop
/// grid's origin in cells `from` pixels long, moved to cells `to` pixels long.
fn rescale_axis(v: i32, from: u32, to: u32) -> i32 {
    let offset = i64::from(v).saturating_sub(i64::from(EDGE_PADDING_I32));
    let scaled = offset.saturating_mul(i64::from(to));
    let from = i64::from(from.max(1));
    // Floor division plus one when the remainder is at least half the
    // divisor: round half up, on negatives as on positives.
    let floor = scaled.checked_div_euclid(from).unwrap_or(0);
    let rem = scaled.checked_rem_euclid(from).unwrap_or(0);
    let rounded = if rem.saturating_mul(2) >= from {
        floor.saturating_add(1)
    } else {
        floor
    };
    let moved = rounded.saturating_add(i64::from(EDGE_PADDING_I32));
    i32::try_from(moved).unwrap_or(if moved < 0 { i32::MIN } else { i32::MAX })
}

// ============================================================================
// Desktop Icon Layer
// ============================================================================

/// The desktop icon layer — manages placement, selection, drag, and rendering.
pub struct DesktopIconLayer {
    /// All icons on the desktop.
    icons: Vec<DesktopIcon>,
    /// Source of icon IDs.
    ids: IdSeq,
    /// The grid's pitch. Private, with [`grid`](Self::grid) to read it,
    /// because it is derived from the icon size (see `glyph_px`) and every
    /// icon's position is laid out on it: a caller that assigned a new one
    /// would leave every icon at the old pitch, which is the bug the saved
    /// layout's `grid` key exists to stop.
    grid: GridConfig,
    /// How icons are placed. Private, with
    /// [`set_arrangement`](Self::set_arrangement), because changing it has to
    /// move the icons: switching to the grid settles them into cells, and
    /// switching to auto-arrange packs them.
    arrangement: ArrangementMode,
    /// Current interaction state.
    interaction: InteractionState,
    /// Screen dimensions.
    screen_width: u32,
    screen_height: u32,
    /// Taskbar height (icons must not overlap the taskbar).
    taskbar_height: u32,
    /// How tall an icon is drawn, in pixels -- the user's icon-size setting.
    /// Private, with [`set_icon_size`](Self::set_icon_size), because the grid
    /// is derived from it and the two must change together.
    glyph_px: u32,
}

impl DesktopIconLayer {
    /// Create a new icon layer for the given screen dimensions.
    pub fn new(screen_width: u32, screen_height: u32, taskbar_height: u32) -> Self {
        Self {
            icons: Vec::new(),
            ids: IdSeq::new(),
            grid: GridConfig::default(),
            arrangement: ArrangementMode::default(),
            interaction: InteractionState::Idle,
            screen_width,
            screen_height,
            taskbar_height,
            glyph_px: DEFAULT_GLYPH_PX,
        }
    }

    /// How tall icons are drawn, in pixels.
    #[must_use]
    pub fn icon_px(&self) -> u32 {
        self.glyph_px
    }

    /// The grid icons are laid out on, which follows the icon size.
    #[must_use]
    pub fn grid(&self) -> GridConfig {
        self.grid
    }

    /// How icons are placed.
    #[must_use]
    pub fn arrangement(&self) -> ArrangementMode {
        self.arrangement
    }

    /// Place icons by `mode` from now on, moving them to suit it. Answers
    /// whether anything changed, so the caller knows whether there is a layout
    /// to save -- which is whenever the mode is new, since the mode is part of
    /// the layout, whether or not an icon moved.
    ///
    /// - To **snap to grid**: every icon settles into a cell of its own, as
    ///   near as it can get to where it is. Coming from auto-arrange that moves
    ///   nothing, since packed icons are already in cells of their own.
    /// - To **auto-arrange**: the icons are packed in the order they are seen
    ///   in -- down each column, then the next -- so turning it on closes the
    ///   gaps without shuffling what the user put where.
    /// - To **free placement**: nothing moves. Every position a grid can hold
    ///   is also a free one.
    pub fn set_arrangement(&mut self, mode: ArrangementMode) -> bool {
        if mode == self.arrangement {
            return false;
        }
        self.arrangement = mode;
        match mode {
            ArrangementMode::Free => {}
            ArrangementMode::SnapToGrid => self.settle_all(),
            ArrangementMode::AutoArrange => {
                self.sort_into_reading_order();
                self.pack();
            }
        }
        true
    }

    /// Draw icons `px` pixels tall -- the user's icon-size setting.
    ///
    /// The grid grows or shrinks with them, and every icon moves *with its
    /// cell*: an icon in the third column, second row stays in the third
    /// column, second row, at the new pitch. That keeps the user's arrangement,
    /// which is the one thing a size change must not scramble. A freely placed
    /// icon keeps its place among the cells around it, the same rule at a
    /// finer grain (see [`GridConfig`]'s `rescale_to`).
    ///
    /// An icon whose cell no longer fits on the desktop -- larger cells, fewer
    /// of them -- moves to the free cell nearest the edge it went over, rather
    /// than off the edge of the screen, where it would still exist and could
    /// not be seen or clicked.
    pub fn set_icon_size(&mut self, px: u32) {
        let px = px.max(1);
        if px == self.glyph_px {
            return;
        }
        let old = self.grid;
        self.glyph_px = px;
        let (w, h) = cell_for_glyph(px);
        self.grid = GridConfig::new(w, h);
        let moved: Vec<(i32, i32)> = self
            .icons
            .iter()
            .map(|icon| old.rescale_to(self.grid, icon.x, icon.y))
            .collect();
        for (icon, (x, y)) in self.icons.iter_mut().zip(moved) {
            icon.x = x;
            icon.y = y;
        }
        self.refit();
    }

    /// How wide an icon's label may be: its cell, less the same margin the
    /// default cell leaves.
    fn label_width(&self) -> f32 {
        px_f32(self.grid.cell_width()) - (px_f32(DEFAULT_GRID_WIDTH) - LABEL_MAX_WIDTH)
    }

    /// Usable area height (excluding taskbar).
    fn usable_height(&self) -> u32 {
        self.screen_height.saturating_sub(self.taskbar_height)
    }

    // ======================================================================
    // Icon management
    // ======================================================================

    /// Add an icon at `(x, y)` and return its ID.
    ///
    /// Where it actually lands follows the arrangement, the same rule a drop
    /// there would: exactly there when placing freely (kept wholly on the
    /// desktop), the free cell nearest there on the grid, or the end of the
    /// order under auto-arrange. It used to snap to whichever cell held the
    /// point even when another icon was in it, and the one added second hid
    /// the first.
    pub fn add_icon(
        &mut self,
        label: &str,
        icon_type: IconType,
        action: IconAction,
        x: i32,
        y: i32,
    ) -> IconId {
        let id = IconId(self.ids.issue_infallible());
        let (x, y) = match self.arrangement {
            ArrangementMode::Free => self.clamp_free(x, y),
            ArrangementMode::SnapToGrid => {
                let taken = self.taken_cells(&[]);
                let (col, row) = self.nearest_free_cell(self.nearest_cell(x, y), &taken);
                self.cell_origin(col, row)
            }
            ArrangementMode::AutoArrange => {
                let (col, row) = self.packed_cell(self.icons.len());
                self.cell_origin(col, row)
            }
        };

        self.icons.push(DesktopIcon {
            id,
            x,
            y,
            label: label.to_string(),
            icon_type,
            action,
            selected: false,
            added: false,
        });

        id
    }

    /// Put a shortcut the user asked for on the desktop, in the next free
    /// place the arrangement gives it -- or, if one for the same thing is
    /// already there, select that one instead of adding a second. Answers the
    /// icon and whether it is new.
    ///
    /// Two icons that open the same thing would be one icon the user has to
    /// find twice, and they would share a position key in the layout file, so
    /// one of them could never be saved where it was left.
    pub fn add_shortcut(
        &mut self,
        label: &str,
        icon_type: IconType,
        action: IconAction,
    ) -> (IconId, bool) {
        let key = Self::storage_key(&action);
        if let Some(existing) = self
            .icons
            .iter()
            .find(|icon| Self::storage_key(&icon.action) == key)
            .map(|icon| icon.id)
        {
            self.select_single(existing);
            return (existing, false);
        }
        let id = self.add_icon_auto(label, icon_type, action);
        if let Some(icon) = self.get_icon_mut(id) {
            icon.added = true;
        }
        self.select_single(id);
        (id, true)
    }

    /// Take the icons in `ids` off the desktop, of those the user added.
    /// Answers how many went.
    ///
    /// A default in `ids` stays: it is recreated at every login, so removing
    /// it would last until the next one and then be undone without a word.
    pub fn remove_added(&mut self, ids: &[IconId]) -> usize {
        let before = self.icons.len();
        self.icons
            .retain(|icon| !(icon.added && ids.contains(&icon.id)));
        let removed = before.saturating_sub(self.icons.len());
        if removed > 0 && self.arrangement == ArrangementMode::AutoArrange {
            self.pack();
        }
        removed
    }

    /// Add an icon at the next available grid position.
    pub fn add_icon_auto(
        &mut self,
        label: &str,
        icon_type: IconType,
        action: IconAction,
    ) -> IconId {
        let pos = self.next_free_cell();
        self.add_icon(label, icon_type, action, pos.0, pos.1)
    }

    /// Remove an icon by ID.
    ///
    /// Under auto-arrange the icons after it close up, since a gap is what
    /// that arrangement exists not to have.
    pub fn remove_icon(&mut self, id: IconId) {
        self.icons.retain(|icon| icon.id != id);
        if self.arrangement == ArrangementMode::AutoArrange {
            self.pack();
        }
    }

    /// Get a reference to an icon by ID.
    pub fn get_icon(&self, id: IconId) -> Option<&DesktopIcon> {
        self.icons.iter().find(|icon| icon.id == id)
    }

    /// Get a mutable reference to an icon by ID.
    pub fn get_icon_mut(&mut self, id: IconId) -> Option<&mut DesktopIcon> {
        self.icons.iter_mut().find(|icon| icon.id == id)
    }

    /// Populate default desktop icons.
    ///
    /// Stacked down the first column. Written as a list rather than four calls
    /// each spelling out its own row offset, because `y_start + cell_height *
    /// 3` is a row number encoded in arithmetic, and the fourth one is where a
    /// typo hides.
    ///
    /// **Home and Documents appear only if `HOME` is set.** They used to point
    /// at the literal strings `/home/user` and `/home/user/Documents` --
    /// correct for a user named "user" and wrong for everyone else, which is a
    /// class of wrong that is invisible in every test and on every machine
    /// where the developer happens to be called "user".
    ///
    /// Omitted rather than defaulted, which is lane A's emit-or-omit rule from
    /// `/sys/devices`: an icon pointing at a guessed home is indistinguishable
    /// from one pointing at a real one until it is clicked, and the guess is
    /// wrong far more often than it is right.
    ///
    /// "This PC" and "Recycle Bin" are unconditional and are not paths: they
    /// name destinations this system defines ([`THIS_PC`], [`RECYCLE_BIN`])
    /// rather than make a claim about a filesystem, and the shell says what
    /// each opens.
    pub fn populate_defaults(&mut self) {
        let mut defaults = vec![
            (
                "This PC".to_string(),
                IconType::Computer,
                IconAction::LaunchSystem(THIS_PC.to_string()),
            ),
            (
                "Recycle Bin".to_string(),
                IconType::RecycleBin,
                IconAction::LaunchSystem(RECYCLE_BIN.to_string()),
            ),
        ];

        if let Some(home) = std::env::var_os("HOME") {
            let home = std::path::PathBuf::from(home);
            let docs = home.join("Documents");
            defaults.push((
                "Documents".to_string(),
                IconType::Folder,
                IconAction::OpenPath(docs),
            ));
            defaults.push((
                "Home".to_string(),
                IconType::Folder,
                IconAction::OpenPath(home),
            ));
        }

        for (row, (label, icon_type, action)) in defaults.into_iter().enumerate() {
            let (x, y) = self.cell_origin(0, i32::try_from(row).unwrap_or(i32::MAX));
            self.add_icon(&label, icon_type, action, x, y);
        }
    }

    // ======================================================================
    // Selection
    // ======================================================================

    /// Select a single icon, deselecting all others.
    pub fn select_single(&mut self, id: IconId) {
        for icon in &mut self.icons {
            icon.selected = icon.id == id;
        }
    }

    /// Toggle selection of a single icon (Ctrl+Click behavior).
    pub fn toggle_selection(&mut self, id: IconId) {
        if let Some(icon) = self.icons.iter_mut().find(|i| i.id == id) {
            icon.selected = !icon.selected;
        }
    }

    /// Select all icons.
    pub fn select_all(&mut self) {
        for icon in &mut self.icons {
            icon.selected = true;
        }
    }

    /// Deselect all icons.
    pub fn deselect_all(&mut self) {
        for icon in &mut self.icons {
            icon.selected = false;
        }
    }

    /// Select icons within a rectangle (rubber-band selection).
    pub fn select_in_rect(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, additive: bool) {
        let min_x = x1.min(x2);
        let max_x = x1.max(x2);
        let min_y = y1.min(y2);
        let max_y = y1.max(y2);

        for icon in &mut self.icons {
            let icon_cx = icon.x as f32 + self.grid.cell_width() as f32 / 2.0;
            let icon_cy = icon.y as f32 + self.grid.cell_height() as f32 / 2.0;

            let in_rect =
                icon_cx >= min_x && icon_cx <= max_x && icon_cy >= min_y && icon_cy <= max_y;

            if additive {
                if in_rect {
                    icon.selected = true;
                }
            } else {
                icon.selected = in_rect;
            }
        }
    }

    /// Get all currently selected icon IDs.
    /// Whether a press, a drag or a rubber-band is in progress.
    ///
    /// The shell asks this before forwarding a move or a release: a pointer
    /// that never pressed on the desktop has nothing to do with this layer,
    /// and forwarding its motion would start work for a gesture that is not
    /// happening. It is also what makes the release safe to route -- a press
    /// leaves this layer non-idle, and only a release returns it.
    #[must_use]
    pub fn is_interacting(&self) -> bool {
        !matches!(self.interaction, InteractionState::Idle)
    }

    /// Every icon on the desktop, in draw order.
    ///
    /// The sibling of [`selected_ids`](Self::selected_ids), and the only way
    /// out of this layer for a caller that wants to walk the icons: the `Vec`
    /// itself stays private so nothing outside can reorder or drop one behind
    /// the layer's back.
    #[must_use]
    pub fn icon_ids(&self) -> Vec<IconId> {
        self.icons.iter().map(|icon| icon.id).collect()
    }

    pub fn selected_ids(&self) -> Vec<IconId> {
        self.icons
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.id)
            .collect()
    }

    // ======================================================================
    // Hit testing
    // ======================================================================

    /// Find the icon at a given pixel position, if any.
    pub fn icon_at(&self, x: f32, y: f32) -> Option<IconId> {
        // Iterate in reverse so topmost (last-added) icon wins on overlap.
        for icon in self.icons.iter().rev() {
            let ix = icon.x as f32;
            let iy = icon.y as f32;
            let iw = self.grid.cell_width() as f32;
            let ih = self.grid.cell_height() as f32;

            if x >= ix && x < ix + iw && y >= iy && y < iy + ih {
                return Some(icon.id);
            }
        }
        None
    }

    // ======================================================================
    // Arrangement
    // ======================================================================

    /// Pixel position of the top-left of grid cell `(col, row)`: the grid's
    /// pitch, started `EDGE_PADDING` in from the corner of the screen.
    ///
    /// Every icon placed by the shell goes through here, so that "where does
    /// cell (2, 3) start" has one answer. It did not: `auto_arrange` measured
    /// the desktop with `self.grid` and then placed with
    /// `GridConfig::default()`, so on any layer with a non-default cell size
    /// the icons were laid out at the wrong pitch — overlapping if the real
    /// cells were larger, gapped if smaller. No test caught it because they
    /// all used the default grid. And until 2026-09-25 it had a rival:
    /// `add_icon` and a drop both snapped to the *bare* cell origin, without
    /// the padding, so the same cell was two positions 8 pixels apart
    /// depending on how the icon got there.
    pub(crate) fn cell_origin(&self, col: i32, row: i32) -> (i32, i32) {
        self.grid.desktop_origin(col, row)
    }

    /// The cell an icon at `(x, y)` is in: the one its centre is over. May be
    /// off the desktop; [`nearest_cell`](Self::nearest_cell) is the one on it.
    fn cell_of(&self, x: i32, y: i32) -> (i32, i32) {
        self.grid.desktop_cell(x, y)
    }

    /// How many columns and rows of cells fit on the usable desktop.
    ///
    /// Never fewer than one of each. A screen smaller than one cell still gets
    /// a cell, overhanging its edge, rather than none: with none, every
    /// placement would need a case for "nowhere", and the only answer it could
    /// give is the cell this one is.
    fn grid_extent(&self) -> (i32, i32) {
        let margin = EDGE_PADDING.saturating_mul(2);
        let cols = self
            .grid
            .columns_in(self.screen_width.saturating_sub(margin))
            .max(1);
        let rows = self
            .grid
            .rows_in(self.usable_height().saturating_sub(margin))
            .max(1);
        (
            i32::try_from(cols).unwrap_or(i32::MAX),
            i32::try_from(rows).unwrap_or(i32::MAX),
        )
    }

    /// The cell an icon at `(x, y)` would snap to: the one its centre is over,
    /// or the nearest on the desktop when that one is not.
    fn nearest_cell(&self, x: i32, y: i32) -> (i32, i32) {
        let (col, row) = self.cell_of(x, y);
        let (cols, rows) = self.grid_extent();
        (
            col.clamp(0, cols.saturating_sub(1)),
            row.clamp(0, rows.saturating_sub(1)),
        )
    }

    /// The cells the icons are in, leaving out those in `except`.
    fn taken_cells(&self, except: &[IconId]) -> BTreeSet<(i32, i32)> {
        self.icons
            .iter()
            .filter(|icon| !except.contains(&icon.id))
            .map(|icon| self.cell_of(icon.x, icon.y))
            .collect()
    }

    /// The cell on the desktop nearest `want` that is not in `taken`.
    ///
    /// Nearest in pixels between cells, not in cells: a cell is taller than it
    /// is wide, so the cell beside is nearer than the cell below. Ties go to
    /// the earlier cell in column order -- the order the desktop fills in --
    /// so the answer does not depend on how the search happens to walk.
    ///
    /// With every cell taken there is no good answer, and `want` itself is
    /// returned: the icon overlaps another rather than leaving the screen,
    /// where it would exist and could not be clicked.
    fn nearest_free_cell(&self, want: (i32, i32), taken: &BTreeSet<(i32, i32)>) -> (i32, i32) {
        let (cols, rows) = self.grid_extent();
        let (cw, ch) = (
            i64::from(self.grid.cell_width()),
            i64::from(self.grid.cell_height()),
        );
        let mut best: Option<(i64, (i32, i32))> = None;
        for col in 0..cols {
            for row in 0..rows {
                if taken.contains(&(col, row)) {
                    continue;
                }
                let dx = i64::from(col)
                    .saturating_sub(i64::from(want.0))
                    .saturating_mul(cw);
                let dy = i64::from(row)
                    .saturating_sub(i64::from(want.1))
                    .saturating_mul(ch);
                let distance = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
                if best.is_none_or(|(nearest, _)| distance < nearest) {
                    best = Some((distance, (col, row)));
                }
            }
        }
        best.map_or(want, |(_, cell)| cell)
    }

    /// The top-left furthest right and down an icon can be placed with all of
    /// it still on the usable desktop.
    fn free_limit(&self) -> (i32, i32) {
        (
            i32::try_from(self.screen_width.saturating_sub(self.grid.cell_width()))
                .unwrap_or(i32::MAX),
            i32::try_from(self.usable_height().saturating_sub(self.grid.cell_height()))
                .unwrap_or(i32::MAX),
        )
    }

    /// `(x, y)` moved as little as possible to put the whole icon on the
    /// usable desktop.
    fn clamp_free(&self, x: i32, y: i32) -> (i32, i32) {
        let (max_x, max_y) = self.free_limit();
        (x.clamp(0, max_x), y.clamp(0, max_y))
    }

    /// Every icon's id and position.
    fn positions(&self) -> Vec<(IconId, i32, i32)> {
        self.icons.iter().map(|i| (i.id, i.x, i.y)).collect()
    }

    /// Whether any icon is somewhere other than `before` says it was.
    ///
    /// By id rather than by comparing the two lists: an operation that
    /// reorders the icons without moving one has changed nothing the user can
    /// see or the layout file records.
    fn moved_since(&self, before: &[(IconId, i32, i32)]) -> bool {
        self.icons
            .iter()
            .any(|icon| !before.contains(&(icon.id, icon.x, icon.y)))
    }

    /// Find the next free grid cell (scanning top-to-bottom, left-to-right).
    pub fn next_free_cell(&self) -> (i32, i32) {
        let (cols, rows) = self.grid_extent();
        // By the centre, so an icon placed freely a little off its cell still
        // counts as being in it; see `GridConfig::desktop_cell`.
        let taken = self.taken_cells(&[]);
        // Columns first: down the first column, then the next.
        for col in 0..cols {
            for row in 0..rows {
                if !taken.contains(&(col, row)) {
                    return self.cell_origin(col, row);
                }
            }
        }
        // Every cell taken: the first one, overlapping, rather than off-screen.
        self.cell_origin(0, 0)
    }

    /// The cell the `index`th icon occupies when the icons are packed: down the
    /// first column, then the next.
    ///
    /// Past the last cell the count starts again at the top-left and icons
    /// overlap, rather than being drawn off the edge of the screen where
    /// nothing could reach them.
    fn packed_cell(&self, index: usize) -> (i32, i32) {
        let (cols, rows) = self.grid_extent();
        let slots = cols.saturating_mul(rows).max(1);
        let index = i32::try_from(index).unwrap_or(i32::MAX);
        let index = index.checked_rem(slots).unwrap_or(0);
        (
            index.checked_div(rows).unwrap_or(0),
            index.checked_rem(rows).unwrap_or(0),
        )
    }

    /// Which packed slot a cell is: its place in the column-first order.
    fn slot_of(&self, (col, row): (i32, i32)) -> usize {
        let (_, rows) = self.grid_extent();
        let slot = col.saturating_mul(rows).saturating_add(row).max(0);
        usize::try_from(slot).unwrap_or(0)
    }

    /// Lay the icons out in cells in their current order, down the columns
    /// from the top-left with no gaps.
    fn pack(&mut self) {
        // Computed up front because `cell_origin` reads `self` and the loop
        // below holds the icons mutably.
        let cells: Vec<(i32, i32)> = (0..self.icons.len())
            .map(|index| {
                let (col, row) = self.packed_cell(index);
                self.cell_origin(col, row)
            })
            .collect();
        for (icon, (x, y)) in self.icons.iter_mut().zip(cells) {
            icon.x = x;
            icon.y = y;
        }
    }

    /// Reorder the icons into the order they are seen in: down each column,
    /// then the next, by the cell each is in.
    ///
    /// Stable, so icons that share a cell keep the order they had.
    fn sort_into_reading_order(&mut self) {
        let grid = self.grid;
        self.icons
            .sort_by_key(|icon| grid.desktop_cell(icon.x, icon.y));
    }

    /// Put the icons in the order `order` names them, keeping any it does not
    /// name, in the order they had, after those it does.
    fn reorder(&mut self, order: &[IconId]) {
        let mut rest = core::mem::take(&mut self.icons);
        let mut sorted = Vec::with_capacity(rest.len());
        for id in order {
            // `remove`, not `swap_remove`: the icons left over keep their
            // order, and they are appended in it below.
            if let Some(at) = rest.iter().position(|icon| icon.id == *id) {
                sorted.push(rest.remove(at));
            }
        }
        sorted.append(&mut rest);
        self.icons = sorted;
    }

    /// Give each icon in `order` a cell of its own, as near as it can get to
    /// where it is now. Earlier icons in `order` win a contested cell, and the
    /// cells in `taken` are already spoken for.
    fn settle(&mut self, order: &[usize], mut taken: BTreeSet<(i32, i32)>) {
        let mut placed = Vec::with_capacity(order.len());
        for &index in order {
            let Some(icon) = self.icons.get(index) else {
                continue;
            };
            let cell = self.nearest_free_cell(self.nearest_cell(icon.x, icon.y), &taken);
            taken.insert(cell);
            placed.push((index, self.cell_origin(cell.0, cell.1)));
        }
        for (index, (x, y)) in placed {
            if let Some(icon) = self.icons.get_mut(index) {
                icon.x = x;
                icon.y = y;
            }
        }
    }

    /// Put every icon on the grid, one to a cell.
    ///
    /// The icon most squarely in a cell keeps it and the others move to the
    /// free cells nearest them, so an icon that was already aligned is never
    /// moved to make room for one that was not.
    fn settle_all(&mut self) {
        let mut order: Vec<usize> = (0..self.icons.len()).collect();
        // Stable, so equally-placed icons keep draw order between them.
        order.sort_by_key(|&index| {
            self.icons.get(index).map_or(i64::MAX, |icon| {
                let (col, row) = self.nearest_cell(icon.x, icon.y);
                let (cx, cy) = self.cell_origin(col, row);
                let dx = i64::from(icon.x).saturating_sub(i64::from(cx));
                let dy = i64::from(icon.y).saturating_sub(i64::from(cy));
                dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
            })
        });
        self.settle(&order, BTreeSet::new());
    }

    /// Bring every icon onto the desktop and into line with the arrangement,
    /// after something moved the goalposts: a new icon size, a new screen
    /// size, a layout read from a file.
    ///
    /// Free icons are moved just far enough to be wholly on the desktop; grid
    /// icons settle one to a cell, keeping theirs where they can; auto-arranged
    /// icons are packed again in the order they had.
    fn refit(&mut self) {
        match self.arrangement {
            ArrangementMode::Free => {
                let clamped: Vec<(i32, i32)> = self
                    .icons
                    .iter()
                    .map(|icon| self.clamp_free(icon.x, icon.y))
                    .collect();
                for (icon, (x, y)) in self.icons.iter_mut().zip(clamped) {
                    icon.x = x;
                    icon.y = y;
                }
            }
            ArrangementMode::SnapToGrid => self.settle_all(),
            ArrangementMode::AutoArrange => self.pack(),
        }
    }

    /// Sort the icons by name and lay them out from the top-left: the
    /// desktop menu's "Sort by name". Answers whether any icon moved.
    ///
    /// A one-off, in every arrangement. Under auto-arrange the sorted order is
    /// the order from then on, until the user drags an icon somewhere else in
    /// it; placing freely, the icons land on grid cells and stay free to move.
    ///
    /// Case-insensitive, and by the label -- the name the user reads -- so
    /// "documents" sorts beside "Documents" rather than after every capital.
    pub fn arrange_by_name(&mut self) -> bool {
        let before = self.positions();
        self.icons.sort_by_key(|i| i.label.to_lowercase());
        self.pack();
        self.moved_since(&before)
    }

    // ======================================================================
    // Mouse interaction
    // ======================================================================

    /// Handle mouse button press. Returns an event if one is generated.
    pub fn handle_mouse_down(
        &mut self,
        x: f32,
        y: f32,
        button: MouseButton,
        ctrl_held: bool,
    ) -> IconEvent {
        if button == MouseButton::Right {
            let hit = self.icon_at(x, y);
            if let Some(id) = hit {
                // If right-clicking an unselected icon, select it alone.
                if !self.icons.iter().any(|i| i.id == id && i.selected) {
                    self.select_single(id);
                }
            }
            return IconEvent::ContextMenu {
                x: x as i32,
                y: y as i32,
                icon_id: hit,
            };
        }

        if button != MouseButton::Left {
            return IconEvent::None;
        }

        if let Some(id) = self.icon_at(x, y) {
            // Clicked on an icon.
            if ctrl_held {
                self.toggle_selection(id);
            } else if !self.icons.iter().any(|i| i.id == id && i.selected) {
                self.select_single(id);
            }

            // Begin pending drag.
            let selected = self.selected_ids();
            self.interaction = InteractionState::PendingDrag {
                start_x: x,
                start_y: y,
                anchor: id,
                icon_ids: selected,
            };
        } else {
            // Clicked on empty desktop — start rubber-band.
            if !ctrl_held {
                self.deselect_all();
            }
            self.interaction = InteractionState::RubberBand {
                start_x: x,
                start_y: y,
                current_x: x,
                current_y: y,
            };
        }

        IconEvent::None
    }

    /// Handle mouse movement (drag tracking).
    pub fn handle_mouse_move(&mut self, x: f32, y: f32, ctrl_held: bool) {
        match &mut self.interaction {
            InteractionState::PendingDrag {
                start_x,
                start_y,
                anchor,
                icon_ids,
            } => {
                let dx = x - *start_x;
                let dy = y - *start_y;
                if dx * dx + dy * dy >= DRAG_THRESHOLD_SQ {
                    // Exceeded drag threshold — transition to dragging.
                    let originals: Vec<(IconId, i32, i32)> = icon_ids
                        .iter()
                        .filter_map(|id| {
                            self.icons
                                .iter()
                                .find(|i| i.id == *id)
                                .map(|i| (i.id, i.x, i.y))
                        })
                        .collect();

                    self.interaction = InteractionState::Dragging {
                        start_x: *start_x,
                        start_y: *start_y,
                        current_x: x,
                        current_y: y,
                        anchor: *anchor,
                        originals,
                    };
                }
            }
            // In place: this runs on every motion event of a drag, and the
            // version it replaces cloned the whole list of dragged icons each
            // time to rebuild the state around one changed coordinate.
            InteractionState::Dragging {
                current_x,
                current_y,
                ..
            } => {
                *current_x = x;
                *current_y = y;
            }
            InteractionState::RubberBand {
                start_x,
                start_y,
                current_x,
                current_y,
            } => {
                *current_x = x;
                *current_y = y;
                let (sx, sy) = (*start_x, *start_y);
                // Update selection based on rubber-band rectangle.
                self.select_in_rect(sx, sy, x, y, ctrl_held);
            }
            InteractionState::Idle => {}
        }
    }

    /// Handle mouse button release. Answers whether any icon moved -- which is
    /// when there is a layout to save.
    ///
    /// A drop is carried out by [`drop_plan`](Self::drop_plan), the same
    /// function that draws the outline of where it will land.
    #[must_use = "a drop that moved icons has a layout to save"]
    pub fn handle_mouse_up(&mut self, _x: f32, _y: f32, button: MouseButton) -> bool {
        if button != MouseButton::Left {
            return false;
        }
        // Whatever the gesture was, the release ends it.
        let InteractionState::Dragging {
            start_x,
            start_y,
            current_x,
            current_y,
            anchor,
            originals,
        } = core::mem::replace(&mut self.interaction, InteractionState::Idle)
        else {
            // A press that never became a drag, or a rubber band: selection
            // only, and nothing moved.
            return false;
        };
        let before = self.positions();
        let plan = self.drop_plan(
            anchor,
            &originals,
            whole_pixels(current_x - start_x),
            whole_pixels(current_y - start_y),
        );
        self.apply_drop(&plan);
        self.moved_since(&before)
    }

    /// What a drop would do with the pointer moved `(dx, dy)` from where the
    /// drag started.
    ///
    /// The release carries it out and the outline drawn during the drag shows
    /// it, so the outline cannot promise a cell the drop then does not use.
    ///
    /// The move is first limited so the whole selection stays on the desktop
    /// -- limited as a block, so a selection dragged into a corner keeps its
    /// shape instead of piling up on itself. That limit is also what stops an
    /// icon grabbed by its far edge and dragged to the edge of the screen from
    /// being dropped into a cell *off* it: the drop used to snap the icon's
    /// corner, which by then was left of the screen, and it landed in column
    /// -1, where it could not be seen or clicked until the next login clamped
    /// it back.
    fn drop_plan(
        &self,
        anchor: IconId,
        originals: &[(IconId, i32, i32)],
        dx: i32,
        dy: i32,
    ) -> DropPlan {
        let (dx, dy) = self.clamp_group_delta(originals, dx, dy);
        match self.arrangement {
            ArrangementMode::Free => DropPlan {
                targets: originals
                    .iter()
                    .map(|&(id, x, y)| {
                        let (x, y) = self.clamp_free(x.saturating_add(dx), y.saturating_add(dy));
                        (id, x, y)
                    })
                    .collect(),
                order: None,
            },
            ArrangementMode::SnapToGrid => {
                let dragged: Vec<IconId> = originals.iter().map(|&(id, _, _)| id).collect();
                let mut taken = self.taken_cells(&dragged);
                // The anchor first: it is under the pointer, and the cell under
                // the pointer is the one the user is aiming at. The rest keep
                // their order behind it (the sort is stable).
                let mut ordered: Vec<&(IconId, i32, i32)> = originals.iter().collect();
                ordered.sort_by_key(|&&(id, _, _)| id != anchor);
                let mut targets = Vec::with_capacity(ordered.len());
                for &(id, x, y) in ordered {
                    let want = self.nearest_cell(x.saturating_add(dx), y.saturating_add(dy));
                    let cell = self.nearest_free_cell(want, &taken);
                    taken.insert(cell);
                    let (x, y) = self.cell_origin(cell.0, cell.1);
                    targets.push((id, x, y));
                }
                DropPlan {
                    targets,
                    order: None,
                }
            }
            ArrangementMode::AutoArrange => self.reorder_plan(anchor, originals, dx, dy),
        }
    }

    /// A drop under auto-arrange: the dragged icons move, as a block in the
    /// order they had, to the place in the order they were dropped on.
    ///
    /// Placed so that the anchor -- the icon under the pointer -- lands in the
    /// slot it was dropped over, which is where the user was looking. Dropped
    /// past the last icon, the block goes last.
    fn reorder_plan(
        &self,
        anchor: IconId,
        originals: &[(IconId, i32, i32)],
        dx: i32,
        dy: i32,
    ) -> DropPlan {
        let dragged: Vec<IconId> = originals.iter().map(|&(id, _, _)| id).collect();
        let (moving, staying): (Vec<IconId>, Vec<IconId>) = self
            .icons
            .iter()
            .map(|icon| icon.id)
            .partition(|id| dragged.contains(id));
        let anchor_at = originals
            .iter()
            .find(|&&(id, _, _)| id == anchor)
            .or_else(|| originals.first())
            .map(|&(_, x, y)| (x.saturating_add(dx), y.saturating_add(dy)));
        let slot = anchor_at.map_or(usize::MAX, |(x, y)| self.slot_of(self.nearest_cell(x, y)));
        let lead = moving.iter().position(|id| *id == anchor).unwrap_or(0);
        let at = slot.saturating_sub(lead).min(staying.len());

        let (before, after) = staying.split_at(at);
        let mut order = Vec::with_capacity(self.icons.len());
        order.extend_from_slice(before);
        order.extend_from_slice(&moving);
        order.extend_from_slice(after);

        let targets = moving
            .iter()
            .enumerate()
            .map(|(offset, id)| {
                let (col, row) = self.packed_cell(at.saturating_add(offset));
                let (x, y) = self.cell_origin(col, row);
                (*id, x, y)
            })
            .collect();
        DropPlan {
            targets,
            order: Some(order),
        }
    }

    /// `(dx, dy)` limited so that no icon in `originals` would leave the
    /// desktop, the limit shared by all of them so they move as one.
    ///
    /// When no single move could keep them all on -- they are already partly
    /// off, after a screen got smaller -- the move is left alone and each icon
    /// is brought back on its own by the placement that follows.
    fn clamp_group_delta(&self, originals: &[(IconId, i32, i32)], dx: i32, dy: i32) -> (i32, i32) {
        let (max_x, max_y) = self.free_limit();
        let limit = |d: i32, coords: Vec<i32>, max: i32| -> i32 {
            match (coords.iter().min(), coords.iter().max()) {
                (Some(&low), Some(&high)) => {
                    let least = low.saturating_neg();
                    let most = max.saturating_sub(high);
                    if least <= most {
                        d.clamp(least, most)
                    } else {
                        d
                    }
                }
                _ => d,
            }
        };
        (
            limit(dx, originals.iter().map(|&(_, x, _)| x).collect(), max_x),
            limit(dy, originals.iter().map(|&(_, _, y)| y).collect(), max_y),
        )
    }

    /// Carry out a [`DropPlan`].
    fn apply_drop(&mut self, plan: &DropPlan) {
        if let Some(order) = &plan.order {
            self.reorder(order);
            self.pack();
            return;
        }
        for &(id, x, y) in &plan.targets {
            if let Some(icon) = self.get_icon_mut(id) {
                icon.x = x;
                icon.y = y;
            }
        }
        // Placed freely, what was dropped is drawn over what it was dropped
        // on: the last thing the user put down is the one they expect to see
        // and to click. On the grid nothing overlaps, so the order is left as
        // it was.
        if self.arrangement == ArrangementMode::Free {
            let (dropped, rest): (Vec<DesktopIcon>, Vec<DesktopIcon>) =
                core::mem::take(&mut self.icons)
                    .into_iter()
                    .partition(|icon| plan.targets.iter().any(|&(id, _, _)| id == icon.id));
            self.icons = rest;
            self.icons.extend(dropped);
        }
    }

    /// Handle double-click at a position.
    pub fn handle_double_click(&mut self, x: f32, y: f32) -> IconEvent {
        if let Some(id) = self.icon_at(x, y)
            && let Some(icon) = self.icons.iter().find(|i| i.id == id)
        {
            return IconEvent::Activate(id, icon.action.clone());
        }
        IconEvent::None
    }

    /// Handle keyboard input. Returns an event if one is generated.
    pub fn handle_key(&mut self, key: DesktopKey, ctrl_held: bool) -> IconEvent {
        match key {
            DesktopKey::SelectAll if ctrl_held => {
                self.select_all();
                IconEvent::None
            }
            DesktopKey::Delete => {
                let selected = self.selected_ids();
                if selected.is_empty() {
                    return IconEvent::None;
                }
                IconEvent::Delete(selected)
            }
            DesktopKey::F2 => {
                // The slice pattern is both halves of the old test at once:
                // exactly one selection, and a name bound to it.
                if let [only] = self.selected_ids().as_slice() {
                    IconEvent::BeginRename(*only)
                } else {
                    IconEvent::None
                }
            }
            DesktopKey::Enter => {
                if let [only] = self.selected_ids().as_slice()
                    && let Some(icon) = self.icons.iter().find(|i| i.id == *only)
                {
                    return IconEvent::Activate(*only, icon.action.clone());
                }
                IconEvent::None
            }
            DesktopKey::Escape => {
                self.deselect_all();
                IconEvent::None
            }
            DesktopKey::Arrow(direction) => {
                self.select_toward(direction);
                IconEvent::None
            }
            // Select-all without Ctrl is the letter A, which the desktop does
            // nothing with.
            DesktopKey::SelectAll => IconEvent::None,
        }
    }

    /// Move the selection to the icon nearest the selected one in
    /// `direction`, answering whether it moved.
    ///
    /// "Nearest" weighs sideways distance three times as heavily as distance
    /// in the direction of travel, so Right from an icon goes to the next icon
    /// along the same row even when a diagonal neighbour is closer as the crow
    /// flies -- on a grid that is the icon the user is looking at -- and to
    /// a diagonal one only when nothing is on the row. Ties go to reading
    /// order, down each column and then the next.
    ///
    /// From nothing selected, the first icon in reading order is selected: an
    /// arrow key on a desktop with no selection has to start somewhere, and
    /// the top-left is where the eye does. From several selected, the
    /// journey starts at the first of them in reading order, and ends with
    /// the one icon it reached selected.
    pub fn select_toward(&mut self, direction: Direction) -> bool {
        let grid = self.grid;
        let reading = |icon: &DesktopIcon| grid.desktop_cell(icon.x, icon.y);
        let from = self
            .icons
            .iter()
            .filter(|icon| icon.selected)
            .min_by_key(|icon| reading(icon))
            .map(|icon| (icon.id, self.centre_of(icon)));
        let Some((from_id, (fx, fy))) = from else {
            let first = self
                .icons
                .iter()
                .min_by_key(|icon| reading(icon))
                .map(|icon| icon.id);
            return first.is_some_and(|id| {
                self.select_single(id);
                true
            });
        };
        let best = self
            .icons
            .iter()
            .filter(|icon| icon.id != from_id)
            .filter_map(|icon| {
                let (cx, cy) = self.centre_of(icon);
                let (dx, dy) = (cx.saturating_sub(fx), cy.saturating_sub(fy));
                let (along, across) = match direction {
                    Direction::Right => (dx, dy),
                    Direction::Left => (dx.saturating_neg(), dy),
                    Direction::Down => (dy, dx),
                    Direction::Up => (dy.saturating_neg(), dx),
                };
                (along > 0).then(|| {
                    let score = i64::from(along)
                        .saturating_add(i64::from(across.unsigned_abs()).saturating_mul(3));
                    (score, reading(icon), icon.id)
                })
            })
            .min_by_key(|&(score, cell, _)| (score, cell));
        best.is_some_and(|(_, _, id)| {
            self.select_single(id);
            true
        })
    }

    /// The pixel centre of an icon's cell-sized footprint.
    fn centre_of(&self, icon: &DesktopIcon) -> (i32, i32) {
        let half_w = i32::try_from(self.grid.cell_width().checked_div(2).unwrap_or(0)).unwrap_or(0);
        let half_h =
            i32::try_from(self.grid.cell_height().checked_div(2).unwrap_or(0)).unwrap_or(0);
        (icon.x.saturating_add(half_w), icon.y.saturating_add(half_h))
    }

    // ======================================================================
    // Rendering
    // ======================================================================

    /// Produce render commands for the entire icon layer.
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        let mut cmds: Vec<RenderCommand> = Vec::new();

        // Render each icon.
        for icon in &self.icons {
            self.render_icon(p, icon, &mut cmds);
        }

        // Render drag ghosts (translucent copies at drag position).
        if let InteractionState::Dragging {
            start_x,
            start_y,
            current_x,
            current_y,
            anchor,
            originals,
        } = &self.interaction
        {
            let dx = *current_x - *start_x;
            let dy = *current_y - *start_y;

            for (id, orig_x, orig_y) in originals {
                if let Some(icon) = self.icons.iter().find(|i| i.id == *id) {
                    let ghost_x = *orig_x as f32 + dx;
                    let ghost_y = *orig_y as f32 + dy;

                    // Ghost background (translucent).
                    cmds.push(RenderCommand::FillRect {
                        x: ghost_x,
                        y: ghost_y,
                        width: self.grid.cell_width() as f32,
                        height: self.grid.cell_height() as f32,
                        // Not in the deleted `theme` block — this one was
                        // written inline, which is how a hardcoded palette
                        // spreads: the same blue at the same alpha as
                        // `hint_fill`, out of reach of anything that could
                        // have renamed it.
                        color: p.hint_fill(),
                        corner_radii: CornerRadii::all(4.0),
                    });

                    // Ghost glyph.
                    let glyph = px_f32(self.glyph_px);
                    let glyph_x = ghost_x + (self.grid.cell_width() as f32 - glyph) / 2.0;
                    let glyph_y = ghost_y + ICON_TOP_PADDING;
                    cmds.push(RenderCommand::Text {
                        x: glyph_x,
                        y: glyph_y,
                        text: icon.icon_type.glyph().to_string(),
                        color: {
                            let c = p.ink(icon.icon_type.color(p));
                            Color::rgba(c.r, c.g, c.b, 120)
                        },
                        font_size: glyph,
                        font_weight: FontWeightHint::Regular,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                }
            }

            // Where each dragged icon will land, from the plan the release
            // carries out. One outline per icon: it used to be one, for the
            // first icon only, at a snap the drop did not use once two icons
            // could not share a cell.
            let plan = self.drop_plan(*anchor, originals, whole_pixels(dx), whole_pixels(dy));
            for &(_, x, y) in &plan.targets {
                cmds.push(RenderCommand::StrokeRect {
                    x: x as f32,
                    y: y as f32,
                    width: self.grid.cell_width() as f32,
                    height: self.grid.cell_height() as f32,
                    color: p.drop_target(),
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        // Render rubber-band selection rectangle.
        if let InteractionState::RubberBand {
            start_x,
            start_y,
            current_x,
            current_y,
        } = &self.interaction
        {
            let rx = start_x.min(*current_x);
            let ry = start_y.min(*current_y);
            let rw = (current_x - start_x).abs();
            let rh = (current_y - start_y).abs();

            cmds.push(RenderCommand::FillRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: p.hint_fill(),
                corner_radii: CornerRadii::ZERO,
            });
            cmds.push(RenderCommand::StrokeRect {
                x: rx,
                y: ry,
                width: rw,
                height: rh,
                color: p.hint_border(),
                line_width: 1.0,
                corner_radii: CornerRadii::ZERO,
            });
        }

        cmds
    }

    /// Render a single icon into the command list.
    fn render_icon(&self, p: &Palette, icon: &DesktopIcon, cmds: &mut Vec<RenderCommand>) {
        let ix = icon.x as f32;
        let iy = icon.y as f32;
        let cw = self.grid.cell_width() as f32;
        let ch = self.grid.cell_height() as f32;

        // Selection highlight.
        if icon.selected {
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: iy,
                width: cw,
                height: ch,
                color: p.selection_fill(),
                corner_radii: CornerRadii::all(4.0),
            });
            cmds.push(RenderCommand::StrokeRect {
                x: ix,
                y: iy,
                width: cw,
                height: ch,
                color: p.selection_border(),
                line_width: 1.0,
                corner_radii: CornerRadii::all(4.0),
            });
        }

        // Icon glyph (centered horizontally within the cell).
        let glyph = px_f32(self.glyph_px);
        let glyph_x = ix + (cw - glyph) / 2.0;
        let glyph_y = iy + ICON_TOP_PADDING;

        cmds.push(RenderCommand::Text {
            x: glyph_x,
            y: glyph_y,
            text: icon.icon_type.glyph().to_string(),
            color: p.ink(icon.icon_type.color(p)),
            font_size: glyph,
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Label text below icon (centered, 2-line max with ellipsis).
        //
        // Centred for real since 2026-09-24: the comment said so for as long
        // as it has existed, and every line was drawn from the cell's left
        // edge -- invisible while one width fitted every cell, and plain once
        // the icon size started to change the cell.
        let label_w = self.label_width();
        let label_y = iy + ICON_TOP_PADDING + glyph + 6.0;
        let lines = wrap_label(&icon.label, label_w, LABEL_MAX_LINES);

        for (line_idx, line) in lines.iter().enumerate() {
            let ly = label_y + line_idx as f32 * (LABEL_FONT_SIZE + 2.0);
            let line_w =
                guitk::text::measure(line, LABEL_FONT_SIZE, FontWeightHint::Regular).min(label_w);
            let lx = ix + (cw - line_w) / 2.0;

            // Shadow for readability against varied backgrounds.
            cmds.push(RenderCommand::Text {
                x: lx + 1.0,
                y: ly + 1.0,
                text: line.clone(),
                color: p.text_shadow(),
                font_size: LABEL_FONT_SIZE,
                font_weight: FontWeightHint::Regular,
                max_width: Some(label_w),
                overflow: TextOverflow::Ellipsis,
            });

            // Actual label text.
            cmds.push(RenderCommand::Text {
                x: lx,
                y: ly,
                text: line.clone(),
                color: if icon.selected {
                    p.on_wallpaper()
                } else {
                    p.on_wallpaper_dim()
                },
                font_size: LABEL_FONT_SIZE,
                font_weight: if icon.selected {
                    FontWeightHint::Bold
                } else {
                    FontWeightHint::Regular
                },
                max_width: Some(label_w),
                overflow: TextOverflow::Ellipsis,
            });
        }
    }

    /// Update screen dimensions (e.g., on resolution change), bringing every
    /// icon back onto the desktop that is now there.
    ///
    /// It used to record the size and move nothing, so after the resolution
    /// dropped an icon could sit past the new edge with nothing able to click
    /// it until the next login's clamp brought it back.
    pub fn set_screen_size(&mut self, width: u32, height: u32) {
        self.screen_width = width;
        self.screen_height = height;
        self.refit();
    }

    // ======================================================================
    // Where the icons were left
    //
    // `design-decisions.md` 933 (open-questions A-Q8): desktop icon layout is
    // not a kernel concern. This layer is the authority and persists positions
    // in userspace; `fs::deskicons` and `/proc/deskicons` are gone.
    //
    // The order was deliberate -- lane C wired first and lane A deleted after,
    // so no reboot lost positions in between, which is why this half was
    // written before that half. Both halves landed; confirmed 2026-09-16 in
    // `requests/a-c-deskicons-is-a-persistence-layer-the-shell-is-the-layout-authority.md`.
    // Stated in the past tense on purpose: it described a deletion that had
    // already happened as though it were still owed, which is how a comment
    // becomes a claim nobody re-checks.
    // ======================================================================

    /// The key an icon's position is filed under.
    ///
    /// The *action*, not the label. A label is what the icon is called and can
    /// be renamed; the action is what it does. This is the rule the taskbar's
    /// pinned apps already follow -- a pin is an executable path, and the name
    /// on it is looked up when it is drawn -- and for the same reason: storing
    /// the label too would be a second copy of it, stale the first time the
    /// thing is renamed.
    ///
    /// A path is filed under its text where it has one, so every key a file
    /// already holds still matches, and under its percent-encoded bytes
    /// (`design-decisions.md` §426, [`pathcodec`]) behind a prefix of its own
    /// where it does not. The two prefixes cannot collide: a path with a text
    /// form never gets the second. Until 2026-09-25 a path that was not text
    /// got no key at all -- flattening it would have filed the position under
    /// a *different* path, so it was skipped instead -- and its icon's
    /// position was silently never saved, though `design.txt` allows every
    /// byte in a name but `/` and NUL. With that gone every action has a key,
    /// and the `Option` this returned went with it.
    fn storage_key(action: &IconAction) -> String {
        match action {
            IconAction::OpenPath(path) => match path.to_str() {
                Some(text) => format!("path:{text}"),
                None => format!("pathx:{}", pathcodec::encode_path(path)),
            },
            IconAction::LaunchSystem(name) => format!("system:{name}"),
            IconAction::Custom(name) => format!("custom:{name}"),
        }
    }

    /// A name for an icon that has none: the file or folder it opens, or the
    /// destination it names.
    fn name_for(action: &IconAction) -> String {
        match action {
            IconAction::OpenPath(path) => path.file_name().map_or_else(
                || path.display().to_string(),
                |name| Path::new(name).display().to_string(),
            ),
            IconAction::LaunchSystem(name) | IconAction::Custom(name) => name.clone(),
        }
    }

    /// The action a storage key names: [`storage_key`](Self::storage_key)
    /// run backwards. `None` for a key this build does not know the shape
    /// of -- a newer desktop's -- which is skipped rather than guessed at.
    fn action_for_key(key: &str) -> Option<IconAction> {
        if let Some(text) = key.strip_prefix("path:") {
            Some(IconAction::OpenPath(PathBuf::from(text)))
        } else if let Some(encoded) = key.strip_prefix("pathx:") {
            Some(IconAction::OpenPath(pathcodec::decode_path(encoded)))
        } else if let Some(name) = key.strip_prefix("system:") {
            Some(IconAction::LaunchSystem(name.to_string()))
        } else {
            key.strip_prefix("custom:")
                .map(|name| IconAction::Custom(name.to_string()))
        }
    }

    /// Fold the layout into a configuration document: every icon's position,
    /// the grid those positions are on, the arrangement, and the shortcuts the
    /// user added.
    ///
    /// Entries for icons that are no longer on the desktop are **removed**,
    /// not merely left unwritten: an icon deleted and later recreated would
    /// otherwise jump back to where its ghost had been.
    pub fn write_layout(&self, doc: &mut Document) {
        let live: Vec<String> = self
            .icons
            .iter()
            .map(|icon| Self::storage_key(&icon.action))
            .collect();
        for key in doc.keys(&[POSITIONS_KEY]) {
            if !live.contains(&key) {
                doc.remove(&[POSITIONS_KEY, &key]);
            }
        }
        for icon in &self.icons {
            let key = Self::storage_key(&icon.action);
            doc.set_i64(&[POSITIONS_KEY, &key, "x"], i64::from(icon.x));
            doc.set_i64(&[POSITIONS_KEY, &key, "y"], i64::from(icon.y));
        }
        doc.set_i64(&[GRID_KEY, "w"], i64::from(self.grid.cell_width()));
        doc.set_i64(&[GRID_KEY, "h"], i64::from(self.grid.cell_height()));
        doc.set_str(&[ARRANGEMENT_KEY], self.arrangement.yaml_name());

        // Removed ones out, as with the positions: a shortcut the user took
        // off the desktop must not come back at the next login.
        let added: Vec<String> = self
            .icons
            .iter()
            .filter(|icon| icon.added)
            .map(|icon| Self::storage_key(&icon.action))
            .collect();
        for key in doc.keys(&[SHORTCUTS_KEY]) {
            if !added.contains(&key) {
                doc.remove(&[SHORTCUTS_KEY, &key]);
            }
        }
        for icon in self.icons.iter().filter(|icon| icon.added) {
            let key = Self::storage_key(&icon.action);
            doc.set_str(&[SHORTCUTS_KEY, &key, "label"], &icon.label);
            doc.set_str(&[SHORTCUTS_KEY, &key, "type"], icon.icon_type.yaml_name());
        }
    }

    /// The grid a document's positions were laid out on: what it says, or the
    /// default grid for a file written before it said anything.
    fn saved_grid(doc: &Document) -> GridConfig {
        let dimension = |which| {
            doc.get_i64(&[GRID_KEY, which])
                .and_then(|v| u32::try_from(v).ok())
                .filter(|&v| v > 0)
        };
        match (dimension("w"), dimension("h")) {
            (Some(w), Some(h)) => GridConfig::new(w, h),
            _ => GridConfig::default(),
        }
    }

    /// Lay the icons out as a document says they were left.
    ///
    /// The arrangement is read first, because it decides what the positions
    /// mean. Each position is then moved from the grid it was saved on to this
    /// layer's (see [`GridConfig`]'s `rescale_to` and the `grid` key's note),
    /// and the whole layout brought into line with the arrangement:
    ///
    /// - **Free**: positions as saved, moved onto the desktop if the screen
    ///   has since got smaller.
    /// - **Snap to grid**: the icons the file places keep their cells, and an
    ///   icon it says nothing about -- a default added since -- keeps its own
    ///   unless one of those took it, in which case it moves to the nearest
    ///   free cell rather than hiding under it.
    /// - **Auto-arrange**: the file's icons in the order they were seen in,
    ///   then any it does not mention, packed.
    ///
    /// **Everything lands on the visible desktop.** A saved layout outlives
    /// the screen it was made on: the same file is read after the resolution
    /// drops, or with a taller taskbar, and an icon restored at its old
    /// coordinates would sit outside the desktop with nothing able to click it.
    ///
    /// A coordinate too large for an `i32` is a corrupt file, and that icon
    /// keeps the position it has. An arrangement this build has no word for
    /// -- a newer desktop's -- leaves the arrangement as it is.
    pub fn read_layout(&mut self, doc: &Document) {
        if let Some(mode) = doc
            .get_str(&[ARRANGEMENT_KEY])
            .and_then(|word| ArrangementMode::from_yaml_name(&word))
        {
            self.arrangement = mode;
        }
        // The user's shortcuts before the positions, which are filed against
        // the icons that exist: a shortcut has to be back on the desktop
        // before it can be put where it was left.
        for key in doc.keys(&[SHORTCUTS_KEY]) {
            let Some(action) = Self::action_for_key(&key) else {
                continue;
            };
            if self
                .icons
                .iter()
                .any(|icon| Self::storage_key(&icon.action) == key)
            {
                continue;
            }
            // A shortcut saved without a label -- hand-edited -- is named for
            // what it opens rather than coming back blank.
            let label = doc
                .get_str(&[SHORTCUTS_KEY, &key, "label"])
                .unwrap_or_else(|| Self::name_for(&action));
            // A type this build has no word for draws as a plain shortcut
            // rather than dropping the icon.
            let icon_type = doc
                .get_str(&[SHORTCUTS_KEY, &key, "type"])
                .and_then(|word| IconType::from_yaml_name(&word))
                .unwrap_or(IconType::Shortcut);
            let id = self.add_icon_auto(&label, icon_type, action);
            if let Some(icon) = self.get_icon_mut(id) {
                icon.added = true;
            }
        }
        let saved = Self::saved_grid(doc);
        let mut restored = Vec::new();
        for (index, icon) in self.icons.iter().enumerate() {
            let key = Self::storage_key(&icon.action);
            let (Some(x), Some(y)) = (
                doc.get_i64(&[POSITIONS_KEY, &key, "x"]),
                doc.get_i64(&[POSITIONS_KEY, &key, "y"]),
            ) else {
                continue;
            };
            let (Ok(x), Ok(y)) = (i32::try_from(x), i32::try_from(y)) else {
                continue;
            };
            let (x, y) = saved.rescale_to(self.grid, x, y);
            restored.push((index, x, y));
        }
        for &(index, x, y) in &restored {
            if let Some(icon) = self.icons.get_mut(index) {
                icon.x = x;
                icon.y = y;
            }
        }
        let from_file = |index: usize| restored.iter().any(|&(i, _, _)| i == index);

        match self.arrangement {
            ArrangementMode::Free => self.refit(),
            ArrangementMode::SnapToGrid => {
                let (mut order, rest): (Vec<usize>, Vec<usize>) =
                    (0..self.icons.len()).partition(|&index| from_file(index));
                order.extend(rest);
                self.settle(&order, BTreeSet::new());
            }
            ArrangementMode::AutoArrange => {
                // The file's icons by where they were, then the rest in the
                // order they have. Keyed on a pair so one stable sort does
                // both: `false` sorts first.
                let grid = self.grid;
                let known: Vec<IconId> = restored
                    .iter()
                    .filter_map(|&(index, _, _)| self.icons.get(index).map(|icon| icon.id))
                    .collect();
                self.icons.sort_by_key(|icon| {
                    if known.contains(&icon.id) {
                        (false, grid.desktop_cell(icon.x, icon.y))
                    } else {
                        (true, (0, 0))
                    }
                });
                self.pack();
            }
        }
    }

    /// Read the saved layout from the user's configuration.
    pub fn load_layout(&mut self) {
        let doc = appearance::config::load(CONFIG_NAME);
        self.read_layout(&doc);
    }

    /// Write the layout back, answering whether it reached the disk.
    ///
    /// The document is loaded and edited rather than rebuilt, so a comment the
    /// user put in the file survives being saved over.
    pub fn save_layout(&self) -> std::io::Result<()> {
        let mut doc = appearance::config::load(CONFIG_NAME);
        self.write_layout(&mut doc);
        appearance::config::store(CONFIG_NAME, &doc)
    }
}

// ============================================================================
// Helper types
// ============================================================================

/// Mouse button (simplified).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Simplified key events relevant to the icon layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesktopKey {
    Delete,
    F2,
    Enter,
    SelectAll,
    Escape,
    /// An arrow key: move the selection to the nearest icon that way.
    Arrow(Direction),
}

/// A way to move across the desktop, for the arrow keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

// ============================================================================
// Label wrapping
// ============================================================================

/// A label broken into at most `max_lines` lines, none wider than `max_width`,
/// with the cut marked if the whole label did not fit.
///
/// # Why a width and not a character budget
///
/// This took `max_chars: usize` — 12 characters per line — and broke the label
/// by counting them. A desktop icon is 72 px wide, and 12 characters is 72 px
/// only for text of average width: "WWWWWWWWWWWW" is nearly twice that and ran
/// out under the neighbouring icon, while "iiiiiiiiiiii" left a third of the
/// cell empty. The renderer was already being told
/// `max_width: Some(LABEL_MAX_WIDTH)` for the same text, so a wide label was
/// then cut a second time, by the renderer, in the middle of a word the wrapper
/// had decided was fine. Two disagreeing answers to one question.
///
/// Now there is one answer, and it is measured: the breaks come from
/// [`text::wrap_hard`], which places them with the same font cache the
/// compositor draws with, so what this function thinks fits is what is drawn.
///
/// # Why `wrap_hard` and not `wrap`
///
/// [`text::wrap`] deliberately does *not* break inside a word — where to do
/// that is a per-script decision belonging to a real line breaker. A desktop
/// icon cannot take that answer: its cell is a fixed 72 px, an over-long line
/// runs under the next icon, and a Japanese file name contains no space at all,
/// so *every* CJK label would come back as one over-long line.
/// [`text::wrap_hard`] breaks by measured fit when a run has no break
/// opportunity in it, which is right for the Han and Kana that make up most
/// such labels and merely inelegant for a long Latin word — which is what the
/// renderer would have done to it anyway.
///
/// The last line kept is marked when anything was dropped, so a truncated file
/// name is distinguishable from a short one.
fn wrap_label(text: &str, max_width: f32, max_lines: usize) -> Vec<String> {
    /// The mark that says text was dropped.
    const ELLIPSIS: &str = "\u{2026}";

    if text.is_empty() || max_lines == 0 || max_width <= 0.0 {
        return vec![String::new()];
    }

    let weight = FontWeightHint::Regular;
    let mut lines = text::wrap_hard(text, max_width, LABEL_FONT_SIZE, weight);
    if lines.len() <= max_lines {
        return lines;
    }

    lines.truncate(max_lines);
    if let Some(last) = lines.last_mut() {
        // The mark has to fit *with* the text it marks, so the room for the
        // text is the cell less the mark. Appending it to a line that already
        // filled the cell is how a label ends up one glyph wider than the cell
        // it was carefully wrapped into.
        let room = (max_width - text::measure(ELLIPSIS, LABEL_FONT_SIZE, weight)).max(0.0);
        let cut = text::fit(last, room, LABEL_FONT_SIZE, weight);
        let mut marked = last.get(..cut).unwrap_or("").trim_end().to_string();
        marked.push_str(ELLIPSIS);
        *last = marked;
    }
    lines
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    // A test module's job is to fail loudly the instant the code under test is
    // wrong, so the defensive lints that forbid exactly that in production code
    // are off here — as `CLAUDE.md` prescribes.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects
    )]

    use super::*;
    use appearance::palette_check;

    // ------------------------------------------------------------------
    // Where the icons were left
    // ------------------------------------------------------------------

    /// A layer with the default icons on a 1920x1080 screen.
    fn populated() -> DesktopIconLayer {
        // `populate_defaults` reads `HOME`, and `settingsfile::testing`
        // removes it for the duration of a scratch-config turn -- which four
        // tests in `idle_lock.rs` take. The environment is process-global and
        // these tests are threads, so without holding the same lock this
        // reads `HOME` while another thread is deleting it and sees two icons
        // instead of four. `ENV_LOCK` only serialises the tests that take it,
        // and a reader is the side that forgets: see known-issues
        // `TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`.
        let _turn = settingsfile::testing::config_turn();
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.populate_defaults();
        layer
    }

    /// The position of the icon whose action is `key`.
    fn position_of(layer: &DesktopIconLayer, key: &str) -> Option<(i32, i32)> {
        layer
            .icons
            .iter()
            .find(|i| DesktopIconLayer::storage_key(&i.action) == key)
            .map(|i| (i.x, i.y))
    }

    /// A path that is not text: bytes no UTF-8 decoder accepts.
    #[cfg(unix)]
    fn not_text_path() -> PathBuf {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(b"/home/caf\xE9"))
    }

    /// A path that is not text: "C:\" followed by a lone high surrogate, which
    /// is not valid Unicode. The host's version of the same case.
    #[cfg(windows)]
    fn not_text_path() -> PathBuf {
        use std::os::windows::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_wide(&[
            0x0043_u16, 0x003A, 0x005C, 0xD800,
        ]))
    }

    /// **An icon on a path that is not text is saved under its exact bytes,
    /// and comes back to where it was left.**
    ///
    /// It used to get no key at all and so was never saved -- the choice then
    /// was between that and flattening the path, which would have filed the
    /// position under a *different* path and put a stranger's icon where the
    /// user left theirs. Percent-encoding (`design-decisions.md` §426) is the
    /// third option that does neither.
    #[cfg(any(unix, windows))]
    #[test]
    fn an_icon_whose_path_has_no_text_form_is_saved_under_its_exact_bytes() {
        let odd = not_text_path();
        assert!(
            odd.to_str().is_none(),
            "the fixture is not the case under test"
        );
        let with_odd_icon = || {
            let mut layer = populated();
            layer.add_icon(
                "Odd",
                IconType::Folder,
                IconAction::OpenPath(odd.clone()),
                0,
                0,
            );
            layer
        };

        let mut layer = with_odd_icon();
        let key = DesktopIconLayer::storage_key(&IconAction::OpenPath(odd.clone()));
        assert!(key.starts_with("pathx:"), "{key}");
        assert!(
            !key.contains(char::REPLACEMENT_CHARACTER),
            "a flattened key: {key:?}"
        );
        let (x, y) = layer.cell_origin(6, 4);
        let odd_icon = layer.icons.len() - 1;
        layer.icons[odd_icon].x = x;
        layer.icons[odd_icon].y = y;
        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        let mut restarted = with_odd_icon();
        assert_ne!(
            position_of(&restarted, &key),
            Some((x, y)),
            "proves nothing"
        );
        restarted.read_layout(&doc);
        assert_eq!(position_of(&restarted, &key), Some((x, y)));
    }

    /// A path that *is* text keeps the key every existing file already uses,
    /// or every saved layout would be forgotten at the next login.
    #[test]
    fn a_text_path_keeps_the_key_it_always_had() {
        assert_eq!(
            DesktopIconLayer::storage_key(&IconAction::OpenPath(PathBuf::from("/home/u"))),
            "path:/home/u"
        );
        assert_eq!(
            DesktopIconLayer::storage_key(&IconAction::OpenPath(PathBuf::from("/home/josé"))),
            "path:/home/josé",
            "text that is not ASCII is still text, and keeps its old key"
        );
    }

    /// **An icon dragged somewhere is still there after a restart.**
    ///
    /// The whole point of `design-decisions.md` 933: the layout leaves the
    /// kernel and this layer becomes the authority, so it has to be able to
    /// answer where things were. Goes through a `Document` rather than a file,
    /// which is the same split `InputFile` and the taskbar's pinned apps use --
    /// the format is exercised without a filesystem.
    ///
    /// A cell rather than an arbitrary pixel, because on the grid -- the
    /// default arrangement -- a cell is the only place a drag can leave an
    /// icon. `free_positions_survive_a_restart_to_the_pixel` is the other case.
    #[test]
    fn an_icon_stays_where_it_was_dragged() {
        let mut layer = populated();
        let key = DesktopIconLayer::storage_key(&layer.icons[0].action);
        let (x, y) = layer.cell_origin(5, 3);
        layer.icons[0].x = x;
        layer.icons[0].y = y;

        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        // A second desktop, built from defaults, reading what the first wrote.
        let mut restarted = populated();
        assert_ne!(
            position_of(&restarted, &key),
            Some((x, y)),
            "the default layout already had it there, so this proves nothing"
        );
        restarted.read_layout(&doc);
        assert_eq!(position_of(&restarted, &key), Some((x, y)));
    }

    /// The key is the action, so renaming an icon does not lose its place.
    #[test]
    fn renaming_an_icon_does_not_move_it() {
        let mut layer = populated();
        let (x, y) = layer.cell_origin(7, 4);
        layer.icons[0].x = x;
        layer.icons[0].y = y;
        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        let mut restarted = populated();
        restarted.icons[0].label = "Something Else Entirely".to_string();
        let key = DesktopIconLayer::storage_key(&restarted.icons[0].action);
        restarted.read_layout(&doc);

        assert_eq!(position_of(&restarted, &key), Some((x, y)));
    }

    /// **A position saved on a bigger screen is brought back onto this one.**
    ///
    /// A saved layout outlives the screen it was made on. `set_screen_size`
    /// records a new size without moving anything, so without this an icon
    /// restored at its old coordinates would sit off the desktop with nothing
    /// able to click it.
    #[test]
    fn a_position_off_this_screen_is_clamped_onto_it() {
        // `populate_defaults` reads `HOME`, and `settingsfile::testing`
        // removes it for the duration of a scratch-config turn -- which four
        // tests in `idle_lock.rs` take. The environment is process-global and
        // these tests are threads, so without holding the same lock this
        // reads `HOME` while another thread is deleting it and sees two icons
        // instead of four. `ENV_LOCK` only serialises the tests that take it,
        // and a reader is the side that forgets: see known-issues
        // `TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`.
        let _turn = settingsfile::testing::config_turn();
        let mut wide = DesktopIconLayer::new(3840, 2160, 40);
        wide.populate_defaults();
        wide.icons[0].x = 3600;
        wide.icons[0].y = 2000;
        let key = DesktopIconLayer::storage_key(&wide.icons[0].action);
        let mut doc = Document::new();
        wide.write_layout(&mut doc);

        let mut small = DesktopIconLayer::new(1024, 768, 40);
        small.populate_defaults();
        small.read_layout(&doc);

        let (x, y) = position_of(&small, &key).expect("the icon is still there");
        // The whole icon, not just its corner: a corner on the desktop with
        // the rest of the cell past the edge is a label nobody can read.
        let (w, h) = (
            small.grid.cell_width() as i32,
            small.grid.cell_height() as i32,
        );
        assert!(
            x >= 0 && y >= 0 && x + w <= 1024 && y + h <= 768 - 40,
            "restored at ({x}, {y}), which is not wholly on a 1024x768 desktop"
        );
    }

    /// **Placing freely, a position survives a restart to the pixel.**
    #[test]
    fn free_positions_survive_a_restart_to_the_pixel() {
        let mut layer = populated();
        assert!(layer.set_arrangement(ArrangementMode::Free));
        let key = DesktopIconLayer::storage_key(&layer.icons[0].action);
        layer.icons[0].x = 413;
        layer.icons[0].y = 297;
        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        let mut restarted = populated();
        restarted.read_layout(&doc);
        assert_eq!(restarted.arrangement(), ArrangementMode::Free);
        assert_eq!(position_of(&restarted, &key), Some((413, 297)));
    }

    /// **Every arrangement survives a restart**, and so does what it means:
    /// read back, each lays the icons out the way it did when it was saved.
    #[test]
    fn the_arrangement_survives_a_restart() {
        for mode in ArrangementMode::ALL {
            let mut layer = populated();
            layer.set_arrangement(mode);
            let mut doc = Document::new();
            layer.write_layout(&mut doc);
            let saved: Vec<(i32, i32)> = layer.icons.iter().map(|i| (i.x, i.y)).collect();

            let mut restarted = populated();
            restarted.read_layout(&doc);
            assert_eq!(restarted.arrangement(), mode);
            let mut read: Vec<(i32, i32)> = restarted.icons.iter().map(|i| (i.x, i.y)).collect();
            let mut saved = saved;
            read.sort_unstable();
            saved.sort_unstable();
            assert_eq!(read, saved, "{mode:?} came back laid out differently");
        }
    }

    /// A file that names no arrangement is one written before there was a
    /// choice, which was the grid; a word this build does not know is a
    /// newer desktop's, and changes nothing rather than failing the load.
    #[test]
    fn a_missing_or_unknown_arrangement_leaves_the_grid() {
        let mut layer = populated();
        layer.read_layout(&Document::new());
        assert_eq!(layer.arrangement(), ArrangementMode::SnapToGrid);

        let mut doc = Document::new();
        doc.set_str(&[ARRANGEMENT_KEY], "spiral");
        layer.read_layout(&doc);
        assert_eq!(layer.arrangement(), ArrangementMode::SnapToGrid);
    }

    /// Every mode has a spelling in the file, and the spelling reads back as
    /// the mode -- the round trip `yaml_enum!` exists to keep.
    #[test]
    fn every_arrangement_has_a_word_in_the_file() {
        for mode in ArrangementMode::ALL {
            assert_eq!(
                ArrangementMode::from_yaml_name(mode.yaml_name()),
                Some(mode)
            );
        }
        // And `ALL` is every mode: this match stops compiling when one is
        // added, which is the moment `ALL` needs it too.
        for mode in ArrangementMode::ALL {
            match mode {
                ArrangementMode::Free
                | ArrangementMode::SnapToGrid
                | ArrangementMode::AutoArrange => {}
            }
        }
        assert_eq!(ArrangementMode::ALL.len(), 3);
    }

    // ------------------------------------------------------------------
    // Saved positions across an icon-size change
    // ------------------------------------------------------------------

    /// **A layout saved at one icon size is read at another in the same
    /// cells**, the padding included.
    ///
    /// Saved at a size that is *not* the default: a file with no `grid` key
    /// is read on the default grid, so a layout saved at the default size
    /// comes back right whether or not the key was written -- which is how the
    /// first version of this test passed with the key deleted.
    #[test]
    fn positions_saved_at_one_icon_size_are_read_at_another() {
        let mut small = DesktopIconLayer::new(1920, 1080, 40);
        small.set_icon_size(64);
        assert_ne!(small.grid, GridConfig::default(), "proves nothing");
        small.add_icon("docs", IconType::Folder, custom("docs"), 0, 0);
        let (x, y) = small.cell_origin(3, 2);
        small.icons[0].x = x;
        small.icons[0].y = y;
        let mut doc = Document::new();
        small.write_layout(&mut doc);

        let mut big = DesktopIconLayer::new(1920, 1080, 40);
        big.set_icon_size(96);
        big.add_icon("docs", IconType::Folder, custom("docs"), 0, 0);
        big.read_layout(&doc);
        let icon = &big.icons[0];
        assert_eq!(
            (icon.x, icon.y),
            big.cell_origin(3, 2),
            "the same cell, padded the same way, at the new pitch"
        );
    }

    /// A file written before the grid key existed was laid out on the one
    /// grid there was then, the default.
    #[test]
    fn a_file_that_predates_the_grid_key_is_read_on_the_default_grid() {
        let default_layer = DesktopIconLayer::new(1920, 1080, 40);
        // The bare cell origin, unpadded, as drops wrote them then.
        let (x, y) = default_layer.grid.from_cell(2, 1);
        let key = DesktopIconLayer::storage_key(&custom("docs"));
        let mut doc = Document::new();
        doc.set_i64(&[POSITIONS_KEY, &key, "x"], i64::from(x));
        doc.set_i64(&[POSITIONS_KEY, &key, "y"], i64::from(y));

        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.set_icon_size(64);
        layer.add_icon("docs", IconType::Folder, custom("docs"), 0, 0);
        layer.read_layout(&doc);
        let icon = &layer.icons[0];
        assert_eq!((icon.x, icon.y), layer.cell_origin(2, 1));
    }

    #[test]
    fn the_file_records_the_grid_its_positions_are_on() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.set_icon_size(48);
        let mut doc = Document::new();
        layer.write_layout(&mut doc);
        assert_eq!(DesktopIconLayer::saved_grid(&doc), layer.grid);
        assert_ne!(layer.grid, GridConfig::default(), "proves nothing");
    }

    /// A grid key with a zero or a negative in it is a damaged file; the
    /// positions are read on the default grid rather than divided by zero.
    #[test]
    fn a_damaged_grid_key_is_read_as_the_default_grid() {
        for (w, h) in [(0, 90), (80, -5), (-1, -1)] {
            let mut doc = Document::new();
            doc.set_i64(&[GRID_KEY, "w"], w);
            doc.set_i64(&[GRID_KEY, "h"], h);
            assert_eq!(DesktopIconLayer::saved_grid(&doc), GridConfig::default());
        }
    }

    /// A corrupt coordinate leaves the icon where it is rather than moving it
    /// somewhere meaningless.
    #[test]
    fn a_coordinate_too_large_for_the_screen_type_is_ignored() {
        let mut layer = populated();
        let key = DesktopIconLayer::storage_key(&layer.icons[0].action);
        let before = position_of(&layer, &key).expect("an icon");

        let mut doc = Document::new();
        doc.set_i64(&[POSITIONS_KEY, &key, "x"], i64::from(i32::MAX) + 1);
        doc.set_i64(&[POSITIONS_KEY, &key, "y"], 10);
        layer.read_layout(&doc);

        assert_eq!(position_of(&layer, &key), Some(before));
    }

    /// An icon that is gone is taken out of the file, not left behind.
    ///
    /// The half that is easy to miss in a writer: writing only what is present
    /// leaves the old entry in place, and an icon deleted and later recreated
    /// jumps back to where its ghost had been.
    #[test]
    fn a_removed_icon_is_taken_out_of_the_file() {
        let mut layer = populated();
        let key = DesktopIconLayer::storage_key(&layer.icons[0].action);
        let mut doc = Document::new();
        layer.write_layout(&mut doc);
        assert!(doc.get_i64(&[POSITIONS_KEY, &key, "x"]).is_some());

        let id = layer.icons[0].id;
        layer.remove_icon(id);
        layer.write_layout(&mut doc);

        assert!(
            doc.get_i64(&[POSITIONS_KEY, &key, "x"]).is_none(),
            "the removed icon is still in the file: {}",
            doc.to_text()
        );
    }

    /// An icon the file says nothing about keeps the place it was given.
    #[test]
    fn an_icon_the_file_does_not_mention_is_left_alone() {
        let mut layer = populated();
        let key = DesktopIconLayer::storage_key(&layer.icons[0].action);
        let before = position_of(&layer, &key).expect("an icon");

        layer.read_layout(&Document::new());

        assert_eq!(position_of(&layer, &key), Some(before));
    }

    // ------------------------------------------------------------------
    // Grid snapping tests
    // ------------------------------------------------------------------

    #[test]
    fn grid_snap_positive_aligned() {
        let grid = GridConfig::new(80, 90);
        assert_eq!(grid.snap(0, 0), (0, 0));
        assert_eq!(grid.snap(80, 90), (80, 90));
        assert_eq!(grid.snap(160, 180), (160, 180));
    }

    #[test]
    fn grid_snap_positive_unaligned() {
        let grid = GridConfig::new(80, 90);
        // Should snap to nearest lower-left cell origin.
        assert_eq!(grid.snap(10, 10), (0, 0));
        assert_eq!(grid.snap(79, 89), (0, 0));
        assert_eq!(grid.snap(81, 91), (80, 90));
        assert_eq!(grid.snap(120, 135), (80, 90));
        assert_eq!(grid.snap(159, 179), (80, 90));
    }

    #[test]
    fn grid_snap_negative_coords() {
        let grid = GridConfig::new(80, 90);
        assert_eq!(grid.snap(-1, -1), (-80, -90));
        assert_eq!(grid.snap(-80, -90), (-80, -90));
        assert_eq!(grid.snap(-81, -91), (-160, -180));
    }

    #[test]
    fn grid_to_cell_and_back() {
        let grid = GridConfig::new(80, 90);
        assert_eq!(grid.to_cell(0, 0), (0, 0));
        assert_eq!(grid.to_cell(80, 90), (1, 1));
        assert_eq!(grid.to_cell(160, 270), (2, 3));
        assert_eq!(grid.from_cell(2, 3), (160, 270));
    }

    #[test]
    fn grid_columns_and_rows() {
        let grid = GridConfig::new(80, 90);
        assert_eq!(grid.columns_in(1920), 24);
        assert_eq!(grid.rows_in(1040), 11); // 1080 - 40 taskbar
        assert_eq!(grid.columns_in(0), 0);
        assert_eq!(grid.rows_in(0), 0);
    }

    #[test]
    fn a_zero_cell_size_is_raised_to_one_rather_than_dividing_by_it() {
        // This used to build a grid of zero-sized cells and check the two
        // methods that guarded against it. The other two — `snap` and
        // `to_cell` — divided by the same zero and panicked; the test simply
        // did not call them. Now the state cannot be built.
        let grid = GridConfig::new(0, 0);
        assert_eq!(grid.cell_width(), 1);
        assert_eq!(grid.cell_height(), 1);
        assert_eq!(grid.snap(37, -12), (37, -12));
        assert_eq!(grid.to_cell(37, -12), (37, -12));
        assert_eq!(grid.columns_in(1920), 1920);
        assert_eq!(grid.rows_in(1080), 1080);
    }

    // ------------------------------------------------------------------
    // Selection tests
    // ------------------------------------------------------------------

    #[test]
    fn select_single_deselects_others() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id1 = layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );
        let id2 = layer.add_icon(
            "B",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/b")),
            80,
            0,
        );

        layer.select_all();
        assert_eq!(layer.selected_ids().len(), 2);

        layer.select_single(id1);
        assert_eq!(layer.selected_ids(), vec![id1]);

        let icon2 = layer.get_icon(id2).unwrap();
        assert!(!icon2.selected);
    }

    #[test]
    fn toggle_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id1 = layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );

        assert!(!layer.get_icon(id1).unwrap().selected);

        layer.toggle_selection(id1);
        assert!(layer.get_icon(id1).unwrap().selected);

        layer.toggle_selection(id1);
        assert!(!layer.get_icon(id1).unwrap().selected);
    }

    #[test]
    fn select_all_and_deselect_all() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );
        layer.add_icon(
            "B",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/b")),
            80,
            0,
        );
        layer.add_icon(
            "C",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/c")),
            160,
            0,
        );

        layer.select_all();
        assert_eq!(layer.selected_ids().len(), 3);

        layer.deselect_all();
        assert_eq!(layer.selected_ids().len(), 0);
    }

    #[test]
    fn rubber_band_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        // Place icons in a known grid.
        layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );
        layer.add_icon(
            "B",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/b")),
            80,
            0,
        );
        layer.add_icon(
            "C",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/c")),
            0,
            90,
        );

        // Select a rectangle that covers A and C (first column). The cells
        // start 8 pixels in, so the centres are A = (48, 53), C = (48, 143)
        // and B = (128, 53).
        layer.select_in_rect(0.0, 0.0, 79.0, 180.0, false);

        let selected = layer.selected_ids();
        assert_eq!(selected.len(), 2);
        // B should not be selected (its centre is at x=128, outside rect).
        assert!(!layer.get_icon(IconId(2)).unwrap().selected);
    }

    // ------------------------------------------------------------------
    // Arrangement tests
    // ------------------------------------------------------------------

    #[test]
    fn sorting_by_name_orders_alphabetically() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon(
            "Zebra",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/z")),
            500,
            500,
        );
        layer.add_icon(
            "Apple",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            300,
            300,
        );
        layer.add_icon(
            "Mango",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/m")),
            100,
            100,
        );

        assert!(
            layer.arrange_by_name(),
            "the icons were not in name order, so they moved"
        );

        // Alphabetical order: Apple, Mango, Zebra.
        assert_eq!(layer.icons[0].label, "Apple");
        assert_eq!(layer.icons[1].label, "Mango");
        assert_eq!(layer.icons[2].label, "Zebra");
    }

    #[test]
    fn sorting_by_name_packs_down_the_columns() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        for i in 0..5 {
            layer.add_icon(
                &format!("Icon{i}"),
                IconType::File,
                IconAction::OpenPath(PathBuf::from(format!("/{i}"))),
                999,
                999,
            );
        }

        layer.arrange_by_name();

        // With default grid (80x90), screen 1920x1080, taskbar 40:
        // usable height = 1040, minus 16 edge padding = 1024
        // rows = 1024 / 90 = 11
        // So 5 icons should fill first column (rows 0..4).
        let grid = GridConfig::default();
        for (idx, icon) in layer.icons.iter().enumerate() {
            let expected_col = idx as u32 / 11;
            let expected_row = idx as u32 % 11;
            let (ex, ey) = grid.from_cell(expected_col as i32, expected_row as i32);
            assert_eq!(icon.x, ex + EDGE_PADDING as i32, "icon {idx} x mismatch");
            assert_eq!(icon.y, ey + EDGE_PADDING as i32, "icon {idx} y mismatch");
        }
    }

    #[test]
    fn packing_lays_icons_out_at_the_layers_own_pitch() {
        // The bug this covers: `auto_arrange` counted the rows that fit using
        // `self.grid` and then placed each icon with `GridConfig::default()`.
        // Every existing test used a layer whose grid *was* the default, so
        // the two agreed by accident and the mismatch was invisible.
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.grid = GridConfig::new(120, 140);
        for i in 0..3 {
            layer.add_icon(
                &format!("Icon{i}"),
                IconType::File,
                IconAction::OpenPath(PathBuf::from(format!("/{i}"))),
                999,
                999,
            );
        }

        layer.arrange_by_name();

        // Three icons fit in the first column at this pitch, so the row index
        // is the icon index and the spacing is the layer's cell height.
        for (row, icon) in layer.icons.iter().enumerate() {
            let (ex, ey) = layer.grid.from_cell(0, row as i32);
            assert_eq!(icon.x, ex + EDGE_PADDING as i32, "icon {row} x");
            assert_eq!(icon.y, ey + EDGE_PADDING as i32, "icon {row} y");
        }
        // And spelled out, so the assertion above cannot pass by comparing the
        // wrong grid against itself: 140, not the default 90.
        assert_eq!(layer.icons[1].y - layer.icons[0].y, 140);
    }

    #[test]
    fn the_default_icons_are_stacked_at_the_layers_own_pitch() {
        // `populate_defaults` reads `HOME`, and `settingsfile::testing`
        // removes it for the duration of a scratch-config turn -- which four
        // tests in `idle_lock.rs` take. The environment is process-global and
        // these tests are threads, so without holding the same lock this
        // reads `HOME` while another thread is deleting it and sees two icons
        // instead of four. `ENV_LOCK` only serialises the tests that take it,
        // and a reader is the side that forgets: see known-issues
        // `TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`.
        let _turn = settingsfile::testing::config_turn();
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.grid = GridConfig::new(120, 140);
        layer.populate_defaults();

        assert_eq!(layer.icons.len(), 4);
        // `EDGE_PADDING` in, the same as every other way onto the grid. Until
        // 2026-09-25 this asserted the bare cell origin: `add_icon` snapped the
        // padding away while `auto_arrange` kept it, so one cell was two
        // positions 8 pixels apart depending on how the icon got there.
        for (row, icon) in layer.icons.iter().enumerate() {
            let (ex, ey) = layer.grid.from_cell(0, row as i32);
            assert_eq!(icon.x, ex + EDGE_PADDING as i32, "icon {row} x");
            assert_eq!(icon.y, ey + EDGE_PADDING as i32, "icon {row} y");
        }
        assert_eq!(layer.icons[1].y - layer.icons[0].y, 140);
    }

    #[test]
    fn a_grid_coarser_than_the_screen_still_places_every_icon() {
        // One cell taller and wider than the usable desktop: no whole cell
        // fits, and `grid_extent` answers one of each rather than none, so
        // there is somewhere to put an icon. Every mode is walked, because
        // each divides by that extent somewhere.
        let mut layer = DesktopIconLayer::new(200, 200, 40);
        layer.grid = GridConfig::new(4096, 4096);
        for i in 0..3 {
            layer.add_icon(
                &format!("Icon{i}"),
                IconType::File,
                IconAction::OpenPath(PathBuf::from(format!("/{i}"))),
                0,
                0,
            );
        }

        layer.arrange_by_name();
        layer.next_free_cell();
        for mode in [
            ArrangementMode::AutoArrange,
            ArrangementMode::Free,
            ArrangementMode::SnapToGrid,
        ] {
            layer.set_arrangement(mode);
            // A refit at the same size, which walks the mode's placement.
            layer.set_screen_size(200, 200);
        }

        assert_eq!(layer.icons.len(), 3);
        assert_eq!(layer.grid_extent(), (1, 1));
    }

    #[test]
    fn next_free_cell_avoids_occupied() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        // Occupy the first cell.
        layer.add_icon(
            "First",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/first")),
            EDGE_PADDING as i32,
            EDGE_PADDING as i32,
        );

        let next = layer.next_free_cell();
        // Should be second row, first column.
        let (expected_x, expected_y) = layer.grid.from_cell(0, 1);
        assert_eq!(
            next,
            (
                expected_x + EDGE_PADDING as i32,
                expected_y + EDGE_PADDING as i32
            )
        );
    }

    // ------------------------------------------------------------------
    // Hit testing
    // ------------------------------------------------------------------

    #[test]
    fn icon_at_hit() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon(
            "Test",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/t")),
            0,
            0,
        );

        // Click inside the cell (grid snaps to 0,0).
        assert_eq!(layer.icon_at(40.0, 45.0), Some(id));
        // Click outside.
        assert_eq!(layer.icon_at(200.0, 200.0), None);
    }

    #[test]
    fn icon_at_returns_topmost() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        // Two icons at the same position (overlapping) -- which only free
        // placement allows; on the grid the second would get a cell of its
        // own.
        layer.set_arrangement(ArrangementMode::Free);
        let _id1 = layer.add_icon(
            "Under",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/u")),
            0,
            0,
        );
        let id2 = layer.add_icon(
            "Over",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/o")),
            0,
            0,
        );

        // Should return the later-added (topmost) icon.
        assert_eq!(layer.icon_at(40.0, 45.0), Some(id2));
    }

    // ------------------------------------------------------------------
    // Label wrapping tests
    // ------------------------------------------------------------------

    /// Every line the wrapper produces has to fit the width it will be drawn
    /// into. That is the only property that matters here, and it is the one a
    /// character budget could not state.
    fn assert_lines_fit(lines: &[String], max_width: f32) {
        for line in lines {
            let w = guitk::text::measure(line, LABEL_FONT_SIZE, FontWeightHint::Regular);
            assert!(
                w <= max_width + 0.5,
                "line {line:?} measures {w}, wider than the {max_width} it is drawn into"
            );
        }
    }

    #[test]
    fn wrap_short_label() {
        let lines = wrap_label("Hello", LABEL_MAX_WIDTH, 2);
        assert_eq!(lines, vec!["Hello"]);
    }

    /// A label too wide for one line but small enough for two is broken at the
    /// space and shown whole — nothing is dropped and nothing is marked, because
    /// nothing was cut.
    ///
    /// The label here is deliberately short. "My Documents Folder", which this
    /// test used to carry, does *not* fit two 72 px lines at 11 px — the old
    /// character budget of 12 per line claimed it did, which is precisely the
    /// error being corrected. It survives below as the label that must be marked.
    #[test]
    fn wrap_long_label_two_lines() {
        let label = "Holiday Photos";
        // Stated, not assumed: this label really is too wide for one line, so
        // the two-line result below is the wrapper working rather than the
        // label happening to be short.
        assert!(
            guitk::text::measure(label, LABEL_FONT_SIZE, FontWeightHint::Regular) > LABEL_MAX_WIDTH,
            "the test label must not fit one line"
        );
        let lines = wrap_label(label, LABEL_MAX_WIDTH, 2);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_lines_fit(&lines, LABEL_MAX_WIDTH);
        assert_eq!(
            lines.join(" "),
            label,
            "nothing was cut, so nothing is lost"
        );
    }

    /// A label that does not fit even in `max_lines` lines is marked, so a
    /// truncated file name is distinguishable from a short one.
    #[test]
    fn wrap_marks_a_label_that_does_not_fit() {
        let lines = wrap_label(
            "Quarterly Revenue Projections Final v3 reviewed.xlsx",
            LABEL_MAX_WIDTH,
            2,
        );
        assert_eq!(lines.len(), 2);
        assert_lines_fit(&lines, LABEL_MAX_WIDTH);
        assert!(lines[1].ends_with('\u{2026}'), "{lines:?}");
    }

    #[test]
    fn wrap_empty_label() {
        let lines = wrap_label("", LABEL_MAX_WIDTH, 2);
        assert_eq!(lines, vec![""]);
    }

    #[test]
    fn wrap_single_line_truncation() {
        let lines = wrap_label("VeryLongFileNameThatExceedsLimit", LABEL_MAX_WIDTH, 1);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].ends_with('\u{2026}'));
        assert_lines_fit(&lines, LABEL_MAX_WIDTH);
    }

    /// What the character budget hid: twelve wide letters and twelve narrow
    /// ones are not the same width, so no single count can be the rule. Both
    /// now wrap to lines that fit, and they wrap differently.
    #[test]
    fn wrapping_follows_the_width_of_the_letters_not_their_number() {
        let wide = wrap_label("WWWW WWWW WWWW WWWW", LABEL_MAX_WIDTH, 2);
        let narrow = wrap_label("iiii iiii iiii iiii", LABEL_MAX_WIDTH, 2);
        assert_lines_fit(&wide, LABEL_MAX_WIDTH);
        assert_lines_fit(&narrow, LABEL_MAX_WIDTH);
        assert!(
            narrow.join(" ").chars().count() > wide.join(" ").chars().count(),
            "the narrow label shows more characters in the same space: {narrow:?} against {wide:?}"
        );
    }

    /// A label in a script whose characters are far from average width must
    /// still produce lines that fit. Under the old count, three ideographs were
    /// "3 of 12 characters" and were drawn about twice the cell wide.
    #[test]
    fn a_cjk_label_wraps_to_lines_that_fit() {
        let lines = wrap_label("日本語のファイル名です", LABEL_MAX_WIDTH, 2);
        assert_lines_fit(&lines, LABEL_MAX_WIDTH);
    }

    // ------------------------------------------------------------------
    // Double-click / action tests
    // ------------------------------------------------------------------

    #[test]
    fn double_click_activates_icon() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon(
            "Test",
            IconType::Computer,
            IconAction::LaunchSystem("explorer --computer".into()),
            0,
            0,
        );

        let event = layer.handle_double_click(40.0, 45.0);
        assert_eq!(
            event,
            IconEvent::Activate(id, IconAction::LaunchSystem("explorer --computer".into()))
        );
    }

    #[test]
    fn double_click_on_empty_returns_none() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon(
            "Test",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/t")),
            0,
            0,
        );

        let event = layer.handle_double_click(500.0, 500.0);
        assert_eq!(event, IconEvent::None);
    }

    // ------------------------------------------------------------------
    // Keyboard action tests
    // ------------------------------------------------------------------

    #[test]
    fn delete_key_returns_selected_ids() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id1 = layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );
        let id2 = layer.add_icon(
            "B",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/b")),
            80,
            0,
        );

        layer.select_all();
        let event = layer.handle_key(DesktopKey::Delete, false);
        assert_eq!(event, IconEvent::Delete(vec![id1, id2]));
    }

    #[test]
    fn f2_begins_rename_for_single_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );

        layer.select_single(id);
        let event = layer.handle_key(DesktopKey::F2, false);
        assert_eq!(event, IconEvent::BeginRename(id));
    }

    #[test]
    fn f2_does_nothing_for_multi_selection() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.add_icon(
            "A",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/a")),
            0,
            0,
        );
        layer.add_icon(
            "B",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/b")),
            80,
            0,
        );

        layer.select_all();
        let event = layer.handle_key(DesktopKey::F2, false);
        assert_eq!(event, IconEvent::None);
    }

    // ------------------------------------------------------------------
    // Default icons test
    // ------------------------------------------------------------------

    #[test]
    fn populate_defaults_creates_four_icons() {
        // `populate_defaults` reads `HOME`, and `settingsfile::testing`
        // removes it for the duration of a scratch-config turn -- which four
        // tests in `idle_lock.rs` take. The environment is process-global and
        // these tests are threads, so without holding the same lock this
        // reads `HOME` while another thread is deleting it and sees two icons
        // instead of four. `ENV_LOCK` only serialises the tests that take it,
        // and a reader is the side that forgets: see known-issues
        // `TD-C-A-TEST-LOCK-SERIALISES-WRITERS-AGAINST-EACH-OTHER-BUT-NOT-AGAINST-READERS`.
        let _turn = settingsfile::testing::config_turn();
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.populate_defaults();
        assert_eq!(layer.icons.len(), 4);

        // Verify expected icons exist.
        assert!(layer.icons.iter().any(|i| i.label == "This PC"));
        assert!(layer.icons.iter().any(|i| i.label == "Recycle Bin"));
        assert!(layer.icons.iter().any(|i| i.label == "Documents"));
        assert!(layer.icons.iter().any(|i| i.label == "Home"));
    }

    // ------------------------------------------------------------------
    // Render produces output
    // ------------------------------------------------------------------

    #[test]
    fn render_produces_commands_for_icons() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.populate_defaults();

        for light in [false, true] {
            let cmds = layer.render(&Palette::for_mode(light));
            // Each icon produces: glyph text + label shadow + label text
            // (minimum). Selected icons also get highlight rect + border.
            assert!(!cmds.is_empty());
            // At least 3 commands per icon (glyph + shadow + text) * 4 icons.
            assert!(cmds.len() >= 12);
        }
    }

    #[test]
    fn render_selected_icon_has_highlight() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon(
            "Sel",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/s")),
            0,
            0,
        );
        layer.select_single(id);

        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = layer.render(&p);
            // First command for a selected icon should be the selection
            // FillRect. Comparing against `p.selection_fill()` rather than a
            // literal is what makes this survive the accent moving: the
            // highlight is the user's accent at a wash alpha now, not a fixed
            // blue.
            let has_fill = cmds.iter().any(
                |c| matches!(c, RenderCommand::FillRect { color, .. } if *color == p.selection_fill()),
            );
            assert!(has_fill, "Selected icon should have a selection highlight");
        }
    }

    /// An icon's label is the same colour in both modes, because the wallpaper
    /// is.
    ///
    /// This exists because the conversion sweep below *cannot* catch the
    /// mistake it guards. The sweep finds a Mocha constant left behind in a
    /// light render; a label wrongly converted to `p.text` is not a leftover
    /// constant but a wrong role, and a wrong role is a member of both
    /// palettes, so it passes the sweep in both modes. Measured, not assumed:
    /// harness defect EE in `scripts/reintro-palette.py` swapped
    /// `on_wallpaper`/`on_wallpaper_dim` for `text`/`subtext0` and the sweep
    /// reported green.
    ///
    /// So this asserts the property directly at the call site. A label lands on
    /// an arbitrary photograph under a black shadow — the shadow is the only
    /// background it is guaranteed to have — which is why it must stay pale
    /// whatever the desktop's mode is. `appearance`'s
    /// `a_label_on_the_wallpaper_does_not_follow_the_mode` makes the same
    /// assertion about the palette method; this one makes it about the module
    /// that has to remember to call it.
    #[test]
    fn an_icon_label_does_not_change_colour_with_the_mode() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let id = layer.add_icon(
            "Selected",
            IconType::File,
            IconAction::OpenPath(PathBuf::from("/s")),
            0,
            0,
        );
        layer.add_icon(
            "Unselected",
            IconType::Folder,
            IconAction::OpenPath(PathBuf::from("/u")),
            80,
            0,
        );
        layer.select_single(id);

        // The glyphs are Text commands too, and they *do* follow the mode
        // (`p.text` for a plain file), so match on the label strings rather
        // than on the command kind.
        let labels_of = |light: bool| -> Vec<(String, Color)> {
            layer
                .render(&Palette::for_mode(light))
                .into_iter()
                .filter_map(|c| match c {
                    RenderCommand::Text { text, color, .. }
                        if text == "Selected" || text == "Unselected" =>
                    {
                        Some((text, color))
                    }
                    _ => None,
                })
                .collect()
        };
        let dark = labels_of(false);
        let light = labels_of(true);
        assert!(
            !dark.is_empty(),
            "no label was drawn, so this test asserts nothing"
        );
        assert_eq!(
            dark, light,
            "an icon label changed colour with the mode; the wallpaper it is \
             drawn on did not, and the black shadow under it did not either — \
             a dark label there is legible against nothing. Use \
             `Palette::on_wallpaper`, not `Palette::text`."
        );
    }

    /// Every colour the icon layer draws comes from the palette it was handed.
    ///
    /// See `security_dialog`'s equivalent for the reasoning. The states below
    /// exist because three of this module's colours are only reachable from an
    /// in-progress gesture: the rubber-band pair needs a marquee being dragged,
    /// and the drop-target outline needs icons mid-drag. A sweep of a resting
    /// desktop would cover nine of the sixteen deleted constants and quietly
    /// certify the other seven.
    ///
    /// `derived` names the wallpaper inks. They are the pale extreme in *both*
    /// modes on purpose — the wallpaper is not a surface the shell knows the
    /// colour of, so the label does not flip with the theme — which means in a
    /// dark render they are the one thing here that is not a role of the
    /// palette in play, and therefore the one thing that must be declared.
    #[test]
    fn every_colour_the_icon_layer_draws_comes_from_its_palette() {
        // One of each icon type, so all nine categorical hues are emitted.
        let types = [
            IconType::Folder,
            IconType::File,
            IconType::Shortcut,
            IconType::Drive,
            IconType::RecycleBin,
            IconType::Computer,
            IconType::Document,
            IconType::Image,
            IconType::Executable,
        ];
        for light in [false, true] {
            let p = Palette::for_mode(light);
            for gesture in 0..3 {
                let mut layer = DesktopIconLayer::new(1920, 1080, 40);
                let mut first = None;
                for (i, ty) in types.into_iter().enumerate() {
                    let id = layer.add_icon(
                        &format!("Icon {i}"),
                        ty,
                        IconAction::OpenPath(PathBuf::from(format!("/i{i}"))),
                        (i as i32) * 80,
                        0,
                    );
                    if first.is_none() {
                        first = Some(id);
                    }
                }
                match gesture {
                    // At rest, with one icon selected: selection fill/border.
                    0 => {
                        if let Some(id) = first {
                            layer.select_single(id);
                        }
                    }
                    // Drawing a marquee: rubber-band fill and border.
                    1 => {
                        layer.interaction = InteractionState::RubberBand {
                            start_x: 10.0,
                            start_y: 10.0,
                            current_x: 400.0,
                            current_y: 300.0,
                        };
                    }
                    // Dragging the selection: the drop-target outline.
                    _ => {
                        if let Some(id) = first {
                            layer.select_single(id);
                            layer.interaction = InteractionState::Dragging {
                                start_x: 10.0,
                                start_y: 10.0,
                                current_x: 200.0,
                                current_y: 200.0,
                                anchor: id,
                                originals: vec![(id, 0, 0)],
                            };
                        }
                    }
                }
                let cmds = layer.render(&p);
                assert!(!cmds.is_empty());
                palette_check::assert_drawn_from(
                    &p,
                    &cmds,
                    &[p.on_wallpaper(), p.on_wallpaper_dim()],
                    "icons",
                );
            }
        }
    }

    // ------------------------------------------------------------------
    // The icon-size setting
    // ------------------------------------------------------------------

    fn custom(label: &str) -> IconAction {
        IconAction::Custom(label.to_string())
    }

    #[test]
    fn the_default_icon_size_lays_out_exactly_as_before() {
        assert_eq!(
            cell_for_glyph(DEFAULT_GLYPH_PX),
            (DEFAULT_GRID_WIDTH, DEFAULT_GRID_HEIGHT)
        );
        let layer = DesktopIconLayer::new(1920, 1080, 40);
        assert_eq!(layer.icon_px(), DEFAULT_GLYPH_PX);
        assert_eq!(layer.grid, GridConfig::default());
    }

    #[test]
    fn every_size_the_setting_offers_gets_a_cell_it_fits() {
        for size in [
            appearance::IconSize::Small,
            appearance::IconSize::Medium,
            appearance::IconSize::Large,
            appearance::IconSize::ExtraLarge,
        ] {
            let px = size.pixels();
            let (w, h) = cell_for_glyph(px);
            assert!(w >= px && h > px, "{px}px in a {w}x{h} cell");
        }
    }

    #[test]
    fn a_size_change_keeps_every_icon_in_its_row_and_column() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let grid = layer.grid;
        let (ax, ay) = grid.from_cell(2, 1);
        let (bx, by) = grid.from_cell(0, 3);
        let a = layer.add_icon("a", IconType::File, custom("a"), ax, ay);
        let b = layer.add_icon("b", IconType::Folder, custom("b"), bx, by);
        let placed = {
            let icon = layer.icons.iter().find(|i| i.id == a).unwrap();
            (icon.x, icon.y)
        };

        layer.set_icon_size(64);
        assert_eq!(layer.icon_px(), 64);
        let (w, h) = cell_for_glyph(64);
        assert_eq!(layer.grid, GridConfig::new(w, h));
        let at = |id| {
            let icon = layer.icons.iter().find(|i| i.id == id).unwrap();
            layer.grid.to_cell(icon.x, icon.y)
        };
        assert_eq!(at(a), (2, 1), "same column and row, at the new pitch");
        assert_eq!(at(b), (0, 3));

        // And back again: nothing drifted on the way.
        layer.set_icon_size(DEFAULT_GLYPH_PX);
        let icon = layer.icons.iter().find(|i| i.id == a).unwrap();
        assert_eq!((icon.x, icon.y), placed);
    }

    #[test]
    fn an_icon_whose_cell_no_longer_fits_moves_to_a_free_one() {
        // A short desktop: at 32px there are rows below what fits at 96px.
        let mut layer = DesktopIconLayer::new(800, 600, 40);
        let (x, y) = layer.grid.from_cell(0, 5);
        let low = layer.add_icon("low", IconType::File, custom("low"), x, y);
        layer.set_icon_size(96);
        let (cols, rows) = layer.grid_extent();
        let icon = layer.icons.iter().find(|i| i.id == low).unwrap();
        let (col, row) = layer.grid.to_cell(icon.x, icon.y);
        assert!(
            col < cols && row < rows,
            "left at ({col}, {row}) on a {cols}x{rows} desktop"
        );
        // In the same column, at the bottom -- the free cell nearest the edge
        // it went over, not the first free cell on the desktop.
        assert_eq!((col, row), (0, rows - 1));
    }

    #[test]
    fn the_glyph_is_drawn_at_the_chosen_size_and_the_label_is_centred() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let (x, y) = layer.grid.from_cell(0, 0);
        layer.add_icon("Readme", IconType::File, custom("r"), x, y);
        layer.set_icon_size(48);
        let cmds = layer.render(&Palette::for_mode(false));
        let glyph_sizes: Vec<f32> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { font_size, .. } if *font_size > LABEL_FONT_SIZE => {
                    Some(*font_size)
                }
                _ => None,
            })
            .collect();
        assert_eq!(glyph_sizes, [48.0], "one glyph, at the setting's size");

        // The last one: each line is drawn twice, its shadow first, one
        // pixel right and down -- and the shadow is not what is centred.
        let label_x = cmds
            .iter()
            .rev()
            .find_map(|c| match c {
                RenderCommand::Text { text, x, .. } if text == "Readme" => Some(*x),
                _ => None,
            })
            .expect("the label is drawn");
        let icon = &layer.icons[0];
        let cell_w = px_f32(layer.grid.cell_width());
        let line_w = guitk::text::measure("Readme", LABEL_FONT_SIZE, FontWeightHint::Regular);
        let left = label_x - px_f32(u32::try_from(icon.x).unwrap());
        let right = cell_w - (left + line_w);
        assert!(
            (left - right).abs() <= 1.0,
            "centred: {left} on the left, {right} on the right"
        );
    }

    #[test]
    fn auto_arranged_icons_are_re_arranged_at_the_new_pitch() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.set_arrangement(ArrangementMode::AutoArrange);
        for name in ["c", "a", "b"] {
            let (x, y) = layer.next_free_cell();
            layer.add_icon(name, IconType::File, custom(name), x, y);
        }
        layer.arrange_by_name();
        layer.set_icon_size(96);
        let labels: Vec<&str> = layer.icons.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["a", "b", "c"]);
        for (row, icon) in layer.icons.iter().enumerate() {
            assert_eq!(
                layer.grid.to_cell(icon.x, icon.y),
                (0, i32::try_from(row).unwrap()),
                "{} is not in its arranged cell",
                icon.label
            );
        }
    }

    // ------------------------------------------------------------------
    // Placement: free, on the grid, auto-arranged
    // ------------------------------------------------------------------

    /// A 1920x1080 layer placing icons by `mode`, with one icon per name down
    /// the first column in the order given.
    fn column_of(names: &[&str], mode: ArrangementMode) -> DesktopIconLayer {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.set_arrangement(mode);
        for (row, name) in names.iter().enumerate() {
            let (x, y) = layer.cell_origin(0, row as i32);
            layer.add_icon(name, IconType::File, custom(name), x, y);
        }
        layer
    }

    fn id_of(layer: &DesktopIconLayer, label: &str) -> IconId {
        layer
            .icons
            .iter()
            .find(|i| i.label == label)
            .map(|i| i.id)
            .unwrap_or_else(|| panic!("no icon labelled {label}"))
    }

    /// Where the icon labelled `label` is.
    fn at(layer: &DesktopIconLayer, label: &str) -> (i32, i32) {
        let id = id_of(layer, label);
        let icon = layer.get_icon(id).unwrap();
        (icon.x, icon.y)
    }

    /// Which cell the icon labelled `label` is in.
    fn cell(layer: &DesktopIconLayer, label: &str) -> (i32, i32) {
        let (x, y) = at(layer, label);
        layer.cell_of(x, y)
    }

    fn labels(layer: &DesktopIconLayer) -> Vec<&str> {
        layer.icons.iter().map(|i| i.label.as_str()).collect()
    }

    /// Press on the icon labelled `label`, `grab` pixels in from its
    /// top-left, move the pointer by `(dx, dy)` and let go -- the gesture as
    /// the shell delivers it. Answers what the release answered.
    fn drag(
        layer: &mut DesktopIconLayer,
        label: &str,
        grab: (f32, f32),
        (dx, dy): (f32, f32),
    ) -> bool {
        let (x, y) = at(layer, label);
        let (sx, sy) = (x as f32 + grab.0, y as f32 + grab.1);
        layer.handle_mouse_down(sx, sy, MouseButton::Left, false);
        layer.handle_mouse_move(sx + dx, sy + dy, false);
        layer.handle_mouse_up(sx + dx, sy + dy, MouseButton::Left)
    }

    /// **Placing freely, an icon stays exactly where it is dropped.**
    #[test]
    fn a_free_drop_lands_exactly_where_it_was_let_go() {
        let mut layer = column_of(&["a", "b"], ArrangementMode::Free);
        let before = at(&layer, "a");
        assert!(drag(&mut layer, "a", (20.0, 20.0), (337.0, 211.0)));
        assert_eq!(at(&layer, "a"), (before.0 + 337, before.1 + 211));
    }

    /// Placing freely still keeps the whole icon on the desktop: dropped past
    /// an edge, it stops flush against it.
    #[test]
    fn a_free_drop_past_the_edge_keeps_the_whole_icon_on_the_desktop() {
        let mut layer = column_of(&["a"], ArrangementMode::Free);
        drag(&mut layer, "a", (20.0, 20.0), (5000.0, 5000.0));
        let (x, y) = at(&layer, "a");
        let (w, h) = (
            layer.grid.cell_width() as i32,
            layer.grid.cell_height() as i32,
        );
        assert_eq!(
            (x + w, y + h),
            (1920, 1080 - 40),
            "flush with the bottom right"
        );
        drag(&mut layer, "a", (20.0, 20.0), (-9000.0, -9000.0));
        assert_eq!(at(&layer, "a"), (0, 0), "flush with the top left");
    }

    /// Placing freely, a dropped icon is drawn over the one it was dropped on,
    /// and is the one a click there finds -- the last thing put down is the
    /// one the user expects to see.
    #[test]
    fn a_freely_dropped_icon_is_drawn_on_top() {
        let mut layer = column_of(&["a", "b"], ArrangementMode::Free);
        let (ax, ay) = at(&layer, "a");
        let (bx, by) = at(&layer, "b");
        drag(
            &mut layer,
            "a",
            (10.0, 10.0),
            ((bx - ax) as f32, (by - ay) as f32),
        );
        assert_eq!(at(&layer, "a"), (bx, by));
        assert_eq!(
            layer.icon_at(bx as f32 + 20.0, by as f32 + 20.0),
            Some(id_of(&layer, "a"))
        );
    }

    /// **A selection dragged past the edge keeps its shape**: the move is
    /// limited as a block, so the icons stop together at the edge rather than
    /// piling on top of each other there.
    #[test]
    fn a_free_selection_dragged_past_the_edge_keeps_its_shape() {
        let mut layer = column_of(&["a", "b"], ArrangementMode::Free);
        for label in ["a", "b"] {
            let id = id_of(&layer, label);
            layer.toggle_selection(id);
        }
        let (ax, ay) = at(&layer, "a");
        let (bx, by) = at(&layer, "b");
        assert!(drag(&mut layer, "a", (10.0, 10.0), (-500.0, -500.0)));
        let (nax, nay) = at(&layer, "a");
        let (nbx, nby) = at(&layer, "b");
        assert_eq!(
            (nbx - nax, nby - nay),
            (bx - ax, by - ay),
            "the selection changed shape"
        );
        assert_eq!((nax, nay), (0, 0), "and stopped at the edge");
    }

    /// **On the grid, a drop lands in the cell the icon mostly covers.**
    ///
    /// By its centre, not its corner: a little under half a cell leaves it
    /// where it was, and a little over moves it one cell. By the corner it
    /// took a whole cell's drag to move one, and the smallest nudge up or left
    /// moved it a whole cell back.
    #[test]
    fn a_grid_drop_lands_in_the_cell_the_icon_mostly_covers() {
        let mut layer = column_of(&["a"], ArrangementMode::SnapToGrid);
        let w = layer.grid.cell_width() as f32;
        assert!(!drag(&mut layer, "a", (10.0, 10.0), (w * 0.4, 0.0)));
        assert_eq!(cell(&layer, "a"), (0, 0), "under half a cell stays put");
        assert!(drag(&mut layer, "a", (10.0, 10.0), (w * 0.6, 0.0)));
        assert_eq!(cell(&layer, "a"), (1, 0), "over half a cell moves one");
        assert_eq!(at(&layer, "a"), layer.cell_origin(1, 0), "squarely in it");
        assert!(!drag(&mut layer, "a", (10.0, 10.0), (-3.0, -3.0)));
        assert_eq!(cell(&layer, "a"), (1, 0), "a nudge is not a move");
    }

    /// **On the grid, a drop onto another icon does not hide it.** The dropped
    /// icon takes the free cell nearest the one it was aimed at, and the icon
    /// already there stays. The old drop snapped both into one cell.
    #[test]
    fn a_grid_drop_onto_an_icon_takes_the_nearest_free_cell() {
        let mut layer = column_of(&["a", "b", "c"], ArrangementMode::SnapToGrid);
        let (ax, ay) = at(&layer, "a");
        let (cx, cy) = at(&layer, "c");
        assert!(drag(
            &mut layer,
            "c",
            (10.0, 10.0),
            ((ax - cx) as f32, (ay - cy) as f32)
        ));
        assert_eq!(cell(&layer, "a"), (0, 0), "the icon that was there stays");
        assert_eq!(cell(&layer, "b"), (0, 1));
        // Beside it rather than below: 80 pixels to the next column, where
        // the nearest free cell in this one -- c's own, now empty -- is 180.
        assert_eq!(cell(&layer, "c"), (1, 0));
    }

    /// **An icon grabbed by its far edge and dragged to the edge of the
    /// screen stays on the screen.** The old drop snapped the icon's corner,
    /// which by then was past the left edge, into column -1 -- off the screen,
    /// where it could not be seen or clicked until the next login.
    #[test]
    fn an_icon_dragged_by_its_far_edge_to_the_screen_edge_stays_on_it() {
        let mut layer = column_of(&["a"], ArrangementMode::SnapToGrid);
        drag(&mut layer, "a", (5.0, 5.0), (400.0, 300.0));
        let (x, _) = at(&layer, "a");
        assert!(
            x > 300,
            "the fixture did not move the icon away from the edge"
        );
        let grab_x = layer.grid.cell_width() as f32 - 5.0;
        let pointer = x as f32 + grab_x;
        drag(&mut layer, "a", (grab_x, 10.0), (1.0 - pointer, 0.0));
        let (col, _) = cell(&layer, "a");
        assert_eq!(col, 0, "dropped into column {col}");
        let (x, y) = at(&layer, "a");
        assert!(x >= 0 && y >= 0, "at ({x}, {y})");
    }

    /// **A selection dropped on the grid keeps every icon in a cell of its
    /// own** -- apart from each other and from the icons it landed among.
    #[test]
    fn a_selection_dropped_on_the_grid_keeps_every_icon_in_its_own_cell() {
        let mut layer = column_of(&["a", "b", "c", "d"], ArrangementMode::SnapToGrid);
        for label in ["a", "b"] {
            let id = id_of(&layer, label);
            layer.toggle_selection(id);
        }
        // Grab b and drop the pair one row down: onto b's old cell and c.
        let h = layer.grid.cell_height() as f32;
        assert!(drag(&mut layer, "b", (10.0, 10.0), (0.0, h)));
        let cells: BTreeSet<(i32, i32)> = ["a", "b", "c", "d"]
            .iter()
            .map(|l| cell(&layer, l))
            .collect();
        assert_eq!(cells.len(), 4, "two icons share a cell: {cells:?}");
        assert_eq!(
            cell(&layer, "c"),
            (0, 2),
            "an icon not dragged does not move"
        );
        assert_eq!(cell(&layer, "d"), (0, 3));
        assert_eq!(
            cell(&layer, "a"),
            (0, 1),
            "a moved down the row it was dragged"
        );
    }

    /// **The outline drawn during a drag is where the drop lands**, in every
    /// arrangement, for a drop onto an occupied cell -- the case where the
    /// cell under the pointer and the cell used disagree. Both come from
    /// `drop_plan`; this holds them to it.
    #[test]
    fn the_drag_outline_shows_where_the_drop_will_land() {
        let p = Palette::for_mode(false);
        assert_ne!(
            p.drop_target(),
            p.selection_border(),
            "the outline cannot be told from the selection's border"
        );
        for mode in ArrangementMode::ALL {
            let mut layer = column_of(&["a", "b", "c"], mode);
            let (ax, ay) = at(&layer, "a");
            let (cx, cy) = at(&layer, "c");
            let (sx, sy) = (cx as f32 + 10.0, cy as f32 + 10.0);
            let (ex, ey) = (sx + (ax - cx) as f32 + 3.0, sy + (ay - cy) as f32 + 2.0);
            layer.handle_mouse_down(sx, sy, MouseButton::Left, false);
            layer.handle_mouse_move(ex, ey, false);
            let outlines: Vec<(i32, i32)> = layer
                .render(&p)
                .iter()
                .filter_map(|c| match c {
                    RenderCommand::StrokeRect { x, y, color, .. } if *color == p.drop_target() => {
                        Some((*x as i32, *y as i32))
                    }
                    _ => None,
                })
                .collect();
            let _ = layer.handle_mouse_up(ex, ey, MouseButton::Left);
            assert_eq!(
                outlines,
                [at(&layer, "c")],
                "{mode:?}: the outline promised one place and the drop used another"
            );
        }
    }

    /// **Turning auto-arrange on closes the gaps without shuffling**: the
    /// icons are packed in the order they are seen in, down each column and
    /// then the next -- not in the order they were added.
    #[test]
    fn auto_arrange_packs_the_icons_in_the_order_they_are_seen() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        for (name, col, row) in [("second", 0, 5), ("third", 3, 0), ("first", 0, 2)] {
            let (x, y) = layer.cell_origin(col, row);
            layer.add_icon(name, IconType::File, custom(name), x, y);
        }
        assert!(layer.set_arrangement(ArrangementMode::AutoArrange));
        assert_eq!(labels(&layer), ["first", "second", "third"]);
        for (row, name) in ["first", "second", "third"].iter().enumerate() {
            assert_eq!(cell(&layer, name), (0, row as i32), "{name}");
        }
    }

    /// **Under auto-arrange, a drag moves an icon to a new place in the
    /// order.** It used to re-sort by name after every drop, which put the
    /// dragged icon straight back where it started: dragging did nothing.
    #[test]
    fn under_auto_arrange_a_drag_reorders() {
        let mut layer = column_of(&["a", "b", "c", "d"], ArrangementMode::AutoArrange);
        let h = layer.grid.cell_height() as f32;
        // a, dropped on c's cell, lands in it -- where the user was looking.
        assert!(drag(&mut layer, "a", (10.0, 10.0), (0.0, 2.0 * h)));
        assert_eq!(labels(&layer), ["b", "c", "a", "d"]);
        for (row, name) in ["b", "c", "a", "d"].iter().enumerate() {
            assert_eq!(
                cell(&layer, name),
                (0, row as i32),
                "{name}: a gap was left"
            );
        }
        // Dropped far past the last icon, it goes last.
        assert!(drag(&mut layer, "b", (10.0, 10.0), (900.0, 0.0)));
        assert_eq!(labels(&layer), ["c", "a", "d", "b"]);
        // Dropped where it already is, nothing changes and nothing is saved.
        assert!(!drag(&mut layer, "d", (10.0, 10.0), (6.0, 6.0)));
        assert_eq!(labels(&layer), ["c", "a", "d", "b"]);
    }

    /// Under auto-arrange a dragged *selection* moves as a block, in its own
    /// order, and the icon under the pointer lands where it was dropped.
    #[test]
    fn under_auto_arrange_a_selection_moves_as_a_block() {
        let mut layer = column_of(&["a", "b", "c", "d", "e"], ArrangementMode::AutoArrange);
        for label in ["a", "b"] {
            let id = id_of(&layer, label);
            layer.toggle_selection(id);
        }
        let h = layer.grid.cell_height() as f32;
        // Grab b (row 1) and drop it on d (row 3).
        assert!(drag(&mut layer, "b", (10.0, 10.0), (0.0, 2.0 * h)));
        assert_eq!(labels(&layer), ["c", "d", "a", "b", "e"]);
        assert_eq!(
            cell(&layer, "b"),
            (0, 3),
            "the anchor lands where it was dropped"
        );
    }

    /// Under auto-arrange a new icon goes at the end of the order, and a
    /// removed one leaves no gap.
    #[test]
    fn under_auto_arrange_icons_are_added_last_and_removed_without_a_gap() {
        let mut layer = column_of(&["a", "b", "c"], ArrangementMode::AutoArrange);
        layer.add_icon("new", IconType::File, custom("new"), 900, 500);
        assert_eq!(
            cell(&layer, "new"),
            (0, 3),
            "at the end, not where it was asked for"
        );
        let b = id_of(&layer, "b");
        layer.remove_icon(b);
        assert_eq!(labels(&layer), ["a", "c", "new"]);
        assert_eq!(cell(&layer, "c"), (0, 1));
        assert_eq!(cell(&layer, "new"), (0, 2));
    }

    /// **On the grid, adding an icon where one already is puts it beside
    /// it**. It used to snap into the same cell, and the one added second hid
    /// the first.
    #[test]
    fn adding_an_icon_on_the_grid_never_hides_another() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let (x, y) = layer.cell_origin(2, 2);
        layer.add_icon("first", IconType::File, custom("first"), x, y);
        layer.add_icon("second", IconType::File, custom("second"), x, y);
        assert_eq!(cell(&layer, "first"), (2, 2));
        assert_ne!(cell(&layer, "second"), (2, 2));
    }

    /// **Aligning free icons to the grid gives each a cell of its own**, and
    /// the icon most squarely in a cell is the one that keeps it.
    #[test]
    fn aligning_free_icons_to_the_grid_gives_each_a_cell_of_its_own() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.set_arrangement(ArrangementMode::Free);
        let (x, y) = layer.cell_origin(4, 4);
        // Both over cell (4, 4); "near" is 6 pixels off it, "far" 30.
        layer.add_icon("far", IconType::File, custom("far"), x + 30, y + 20);
        layer.add_icon("near", IconType::File, custom("near"), x + 6, y + 4);
        assert_eq!(cell(&layer, "far"), (4, 4), "the fixture is not a contest");
        assert!(layer.set_arrangement(ArrangementMode::SnapToGrid));
        assert_eq!(
            at(&layer, "near"),
            (x, y),
            "the squarer icon keeps the cell"
        );
        let (col, row) = cell(&layer, "far");
        assert_ne!((col, row), (4, 4));
        assert!(
            (col - 4).abs() <= 1 && (row - 4).abs() <= 1,
            "the other goes next door, not across the desktop: ({col}, {row})"
        );
    }

    /// Turning alignment off moves nothing -- every position the grid holds
    /// is also a free one -- and asking for the mode already in force is no
    /// change at all.
    #[test]
    fn turning_alignment_off_moves_nothing() {
        let mut layer = column_of(&["a", "b", "c"], ArrangementMode::SnapToGrid);
        let before = layer.positions();
        assert!(layer.set_arrangement(ArrangementMode::Free));
        assert_eq!(layer.positions(), before);
        assert!(!layer.set_arrangement(ArrangementMode::Free));
    }

    /// **Sort by name orders the icons and packs them, in every
    /// arrangement**, case-insensitively, and leaves the arrangement as it
    /// was. Sorted already, nothing moves and it says so.
    #[test]
    fn sorting_by_name_packs_in_every_arrangement() {
        for mode in ArrangementMode::ALL {
            let mut layer = DesktopIconLayer::new(1920, 1080, 40);
            layer.set_arrangement(mode);
            for (name, col) in [("pear", 0), ("Apple", 3), ("fig", 6)] {
                let (x, y) = layer.cell_origin(col, 2);
                layer.add_icon(name, IconType::File, custom(name), x, y);
            }
            assert!(layer.arrange_by_name(), "{mode:?}");
            assert_eq!(labels(&layer), ["Apple", "fig", "pear"], "{mode:?}");
            for (row, name) in ["Apple", "fig", "pear"].iter().enumerate() {
                assert_eq!(cell(&layer, name), (0, row as i32), "{mode:?}: {name}");
            }
            assert_eq!(layer.arrangement(), mode);
            assert!(!layer.arrange_by_name(), "{mode:?}: sorted already");
        }
    }

    /// **A free layout grows and shrinks with its icons**: a size change and
    /// back puts every icon where it was, to the pixel.
    #[test]
    fn a_free_layout_survives_a_size_change_and_back() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.set_arrangement(ArrangementMode::Free);
        for (name, x, y) in [("a", 413, 297), ("b", 77, 401), ("c", 903, 11)] {
            layer.add_icon(name, IconType::File, custom(name), x, y);
        }
        let before = layer.positions();
        layer.set_icon_size(96);
        assert_ne!(
            layer.positions(),
            before,
            "nothing moved, so this proves nothing"
        );
        layer.set_icon_size(32);
        assert_eq!(layer.positions(), before);
    }

    /// **A smaller screen brings every icon back onto the desktop**, in each
    /// arrangement's own way -- and on the grid, still one icon to a cell.
    #[test]
    fn a_smaller_screen_brings_every_icon_back_onto_the_desktop() {
        for mode in ArrangementMode::ALL {
            let mut layer = DesktopIconLayer::new(3840, 2160, 40);
            layer.set_arrangement(mode);
            for (i, name) in ["a", "b", "c"].iter().enumerate() {
                let (x, y) = layer.cell_origin(40 - i as i32, 20);
                layer.add_icon(name, IconType::File, custom(name), x, y);
            }
            layer.set_screen_size(1024, 768);
            let (w, h) = (
                layer.grid.cell_width() as i32,
                layer.grid.cell_height() as i32,
            );
            let mut cells = BTreeSet::new();
            for icon in &layer.icons {
                assert!(
                    icon.x >= 0 && icon.y >= 0 && icon.x + w <= 1024 && icon.y + h <= 768 - 40,
                    "{mode:?}: {} at ({}, {})",
                    icon.label,
                    icon.x,
                    icon.y
                );
                cells.insert(layer.cell_of(icon.x, icon.y));
            }
            if mode.aligns_to_grid() {
                assert_eq!(cells.len(), 3, "{mode:?}: two icons share a cell");
            }
        }
    }

    /// A press and release with no drag between selects, moves nothing, and
    /// says so -- there is no layout to save.
    #[test]
    fn a_click_moves_nothing_and_says_so() {
        let mut layer = column_of(&["a"], ArrangementMode::SnapToGrid);
        let (x, y) = at(&layer, "a");
        let (px, py) = (x as f32 + 10.0, y as f32 + 10.0);
        layer.handle_mouse_down(px, py, MouseButton::Left, false);
        assert!(!layer.handle_mouse_up(px, py, MouseButton::Left));
        assert!(!layer.is_interacting());
        assert_eq!(layer.selected_ids(), [id_of(&layer, "a")]);
    }

    /// Read under auto-arrange, the file's icons keep their order, and an
    /// icon the file does not mention -- a default added since -- goes last.
    #[test]
    fn an_auto_arranged_layout_keeps_its_order_and_puts_new_icons_last() {
        let mut layer = column_of(&["a", "b", "c"], ArrangementMode::AutoArrange);
        let h = layer.grid.cell_height() as f32;
        assert!(drag(&mut layer, "c", (10.0, 10.0), (0.0, -2.0 * h)));
        assert_eq!(labels(&layer), ["c", "a", "b"]);
        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        let mut restarted = column_of(&["new", "a", "b", "c"], ArrangementMode::SnapToGrid);
        restarted.read_layout(&doc);
        assert_eq!(restarted.arrangement(), ArrangementMode::AutoArrange);
        assert_eq!(labels(&restarted), ["c", "a", "b", "new"]);
    }

    /// Read on the grid, an icon the file places wins its cell over one the
    /// file does not mention, which moves beside it rather than under it.
    #[test]
    fn on_the_grid_a_saved_position_wins_its_cell() {
        let layer = column_of(&["a"], ArrangementMode::SnapToGrid);
        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        let mut restarted = column_of(&["new", "a"], ArrangementMode::SnapToGrid);
        assert_eq!(
            cell(&restarted, "new"),
            (0, 0),
            "the fixture is not a contest"
        );
        restarted.read_layout(&doc);
        assert_eq!(cell(&restarted, "a"), (0, 0), "where the file put it");
        assert_ne!(cell(&restarted, "new"), (0, 0));
    }

    // ------------------------------------------------------------------
    // The arrow keys
    // ------------------------------------------------------------------

    /// A layer with an icon in each of the given cells, named by its cell.
    fn icons_at(cells: &[(i32, i32)]) -> DesktopIconLayer {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        for &(col, row) in cells {
            let (x, y) = layer.cell_origin(col, row);
            let name = format!("{col},{row}");
            layer.add_icon(&name, IconType::File, custom(&name), x, y);
        }
        layer
    }

    fn selected_names(layer: &DesktopIconLayer) -> Vec<String> {
        layer
            .icons
            .iter()
            .filter(|i| i.selected)
            .map(|i| i.label.clone())
            .collect()
    }

    /// **An arrow key with nothing selected starts at the top-left**, and
    /// from there each press moves one icon.
    #[test]
    fn an_arrow_with_nothing_selected_starts_at_the_top_left() {
        let mut layer = icons_at(&[(2, 0), (0, 1), (0, 0), (1, 0)]);
        assert!(layer.select_toward(Direction::Down));
        assert_eq!(selected_names(&layer), ["0,0"]);
        assert!(layer.select_toward(Direction::Down));
        assert_eq!(selected_names(&layer), ["0,1"]);
        assert!(layer.select_toward(Direction::Up));
        assert!(layer.select_toward(Direction::Right));
        assert_eq!(selected_names(&layer), ["1,0"]);
    }

    /// **Right goes along the row**, past a diagonal neighbour that is nearer
    /// as the crow flies, and to a diagonal one only when the row is empty.
    #[test]
    fn the_arrows_keep_to_the_row_before_taking_a_diagonal() {
        let mut layer = icons_at(&[(0, 0), (1, 1), (3, 0)]);
        layer.select_single(layer.icons[0].id);
        assert!(layer.select_toward(Direction::Right));
        assert_eq!(
            selected_names(&layer),
            ["3,0"],
            "along the row, not down to (1, 1)"
        );

        let mut layer = icons_at(&[(0, 0), (1, 1)]);
        layer.select_single(layer.icons[0].id);
        assert!(layer.select_toward(Direction::Right));
        assert_eq!(
            selected_names(&layer),
            ["1,1"],
            "the row is empty, so the diagonal"
        );
    }

    /// An arrow with nowhere to go leaves the selection where it is and says
    /// nothing moved.
    #[test]
    fn an_arrow_with_nowhere_to_go_keeps_the_selection() {
        let mut layer = icons_at(&[(0, 0), (0, 1)]);
        layer.select_single(layer.icons[0].id);
        assert!(!layer.select_toward(Direction::Up));
        assert!(!layer.select_toward(Direction::Left));
        assert_eq!(selected_names(&layer), ["0,0"]);
        // And on an empty desktop there is nothing to start from.
        assert!(!DesktopIconLayer::new(1920, 1080, 40).select_toward(Direction::Down));
    }

    /// From several selected icons the journey starts at the first in
    /// reading order and ends with one icon selected.
    #[test]
    fn an_arrow_from_a_multiple_selection_selects_one() {
        let mut layer = icons_at(&[(0, 0), (0, 1), (0, 2)]);
        layer.select_all();
        assert!(layer.select_toward(Direction::Down));
        assert_eq!(selected_names(&layer), ["0,1"]);
    }

    /// Escape clears the selection.
    #[test]
    fn escape_clears_the_selection() {
        let mut layer = icons_at(&[(0, 0), (0, 1)]);
        layer.select_all();
        assert_eq!(layer.handle_key(DesktopKey::Escape, false), IconEvent::None);
        assert!(layer.selected_ids().is_empty());
    }

    // ------------------------------------------------------------------
    // Shortcuts the user added
    // ------------------------------------------------------------------

    /// Every action's storage key reads back as the same action -- which is
    /// what lets the layout file keep a shortcut's action in its key alone.
    #[test]
    fn a_storage_key_reads_back_as_its_action() {
        let actions = [
            IconAction::OpenPath(PathBuf::from("/usr/bin/editor")),
            IconAction::OpenPath(PathBuf::from("/home/u/My Stuff")),
            IconAction::LaunchSystem(THIS_PC.to_string()),
            IconAction::Custom("weather".to_string()),
        ];
        for action in actions {
            let key = DesktopIconLayer::storage_key(&action);
            assert_eq!(
                DesktopIconLayer::action_for_key(&key),
                Some(action),
                "{key}"
            );
        }
        assert_eq!(DesktopIconLayer::action_for_key("hologram:x"), None);
    }

    /// A path that is not text reads back as the same bytes.
    #[cfg(unix)]
    #[test]
    fn a_non_text_key_reads_back_as_its_bytes() {
        let action = IconAction::OpenPath(not_text_path());
        let key = DesktopIconLayer::storage_key(&action);
        assert_eq!(DesktopIconLayer::action_for_key(&key), Some(action));
    }

    /// Every icon type has a word in the file, and reads back from it.
    #[test]
    fn every_icon_type_has_a_word_in_the_file() {
        for ty in IconType::ALL {
            assert_eq!(IconType::from_yaml_name(ty.yaml_name()), Some(ty));
        }
        // `ALL` is every type: this match stops compiling when one is added.
        for ty in IconType::ALL {
            match ty {
                IconType::Folder
                | IconType::File
                | IconType::Shortcut
                | IconType::Drive
                | IconType::RecycleBin
                | IconType::Computer
                | IconType::Document
                | IconType::Image
                | IconType::Executable => {}
            }
        }
    }

    /// **A shortcut the user added is still there after a login**, where it
    /// was left, and still removable; a default is not written as one.
    #[test]
    fn an_added_shortcut_survives_a_restart() {
        let mut layer = populated();
        let editor = IconAction::OpenPath(PathBuf::from("/usr/bin/editor"));
        let (id, new) = layer.add_shortcut("Editor", IconType::Executable, editor.clone());
        assert!(new);
        let (x, y) = layer.cell_origin(4, 2);
        let icon = layer.get_icon_mut(id).unwrap();
        icon.x = x;
        icon.y = y;
        let mut doc = Document::new();
        layer.write_layout(&mut doc);
        assert_eq!(
            doc.keys(&[SHORTCUTS_KEY]).len(),
            1,
            "only the added icon is a shortcut"
        );

        let mut restarted = populated();
        restarted.read_layout(&doc);
        let back = restarted
            .icons
            .iter()
            .find(|i| i.action == editor)
            .expect("the shortcut came back");
        assert_eq!(
            (back.label.as_str(), back.icon_type),
            ("Editor", IconType::Executable)
        );
        assert!(back.added, "and it can still be removed");
        assert_eq!((back.x, back.y), (x, y));
    }

    /// Adding a shortcut for something already on the desktop selects it
    /// rather than making a second.
    #[test]
    fn adding_a_shortcut_twice_selects_the_first() {
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        let action = IconAction::OpenPath(PathBuf::from("/usr/bin/editor"));
        let (first, new) = layer.add_shortcut("Editor", IconType::Executable, action.clone());
        assert!(new);
        layer.deselect_all();
        let (again, new) = layer.add_shortcut("Editor", IconType::Executable, action);
        assert!(!new);
        assert_eq!(again, first);
        assert_eq!(layer.icons.len(), 1);
        assert_eq!(layer.selected_ids(), [first]);
    }

    /// **Removing takes only what the user added**, and a removed shortcut
    /// is taken out of the file too.
    #[test]
    fn removing_takes_only_added_icons_and_the_file_forgets_them() {
        let mut layer = populated();
        let (added, _) = layer.add_shortcut(
            "Editor",
            IconType::Executable,
            IconAction::OpenPath(PathBuf::from("/usr/bin/editor")),
        );
        let defaults = layer.icons.len() - 1;
        let mut doc = Document::new();
        layer.write_layout(&mut doc);

        let everything = layer.icon_ids();
        assert_eq!(layer.remove_added(&everything), 1);
        assert_eq!(layer.icons.len(), defaults, "a default went too");
        assert!(layer.get_icon(added).is_none());
        layer.write_layout(&mut doc);
        assert!(
            doc.keys(&[SHORTCUTS_KEY]).is_empty(),
            "the file still has it"
        );
        assert_eq!(layer.remove_added(&everything), 0, "nothing left to remove");
    }

    /// A hand-edited shortcut with no label is named for what it opens, and
    /// one with an unknown type is drawn as a plain shortcut.
    #[test]
    fn a_hand_edited_shortcut_is_named_and_drawn_sensibly() {
        let mut doc = Document::new();
        doc.set_str(&[SHORTCUTS_KEY, "path:/usr/bin/editor", "type"], "hologram");
        let mut layer = DesktopIconLayer::new(1920, 1080, 40);
        layer.read_layout(&doc);
        let icon = &layer.icons[0];
        assert_eq!(icon.label, "editor");
        assert_eq!(icon.icon_type, IconType::Shortcut);
        assert!(icon.added);
    }
}
