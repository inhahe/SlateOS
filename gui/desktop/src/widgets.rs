//! Desktop widget system.
//!
//! Allows small, always-visible widget panels on the desktop surface.
//! Widgets can show live information (clock, weather, CPU, calendar, notes,
//! RSS, stocks, etc.) without opening a full application window.
//!
//! Each widget occupies a fixed-size slot on a grid overlay. The user can
//! add, remove, move, and resize widgets. Third-party apps can provide widgets
//! via a capability-gated registration API.

use appearance::Palette;
use guitk::color::Color;
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;

// ============================================================================
// Colour
// ============================================================================
//
// Every colour drawn here comes from the `&Palette` passed to `render`, so the
// widget layer follows the desktop's mode and accent.  Most of this module is
// **washes** -- `Color::rgba(role.r, role.g, role.b, alpha)` -- because a
// widget panel is translucent over the wallpaper.  Module 20's rule governs
// them: a wash is a role seen through a veil, so the veil is the alpha and the
// role is everything else.  Every alpha below is untouched by the conversion;
// only the three channels in front of it changed hands.
//
// Four judgements had to be made when the hardcoded hexes came out, because a
// literal carries no role until someone assigns one:
//
// *The selected widget's outline takes the accent.*  In edit mode a 2px ring is
// drawn around exactly the widget you have picked, and around nothing else.  It
// was a hardcoded blue that appears in no other state, and a colour that
// appears in exactly one state marks that state -- here, which widget you are
// working on, which is a position, which is what the accent is for.  A ring
// floating on the wallpaper cannot say "here" with a surface step the way a
// hovered list row can, so the accent is also the only mark available to it.
//
// *Everything that reports a measurement is frozen, because a meter reports
// rather than invites.*  Module 19 gave sliders a `surface1` track and an
// accent fill, but a slider is something you drag; the CPU/Memory/Disk bars are
// read-outs nobody can move, and the accent never marks measurement.  Their
// fills are further a **category** set -- blue is CPU, green is Memory, peach
// is Disk -- and three bars told apart by colour stop being three bars the
// moment they all follow one accent.  The tracks stay `surface1`, which is the
// half of the slider rule that does survive: a track is a surface either way.
// The battery glyph's green is the same judgement one widget over: green there
// is not decoration but the reading itself -- it is how the widget says the
// charge is healthy -- so it would be saying something false the day someone
// picked a red accent.
//
// *The picker joins the shared popup shadow; a widget's own shadow does not.*
// The picker is a panel that sits on top of everything else, so its
// `rgba(0, 0, 0, 100)` became `Palette::shadow()` -- the same move
// `context_ext` made, for the same reason.  The per-widget shadow keeps
// `rgba(0, 0, 0, bg_opacity / 3)` deliberately: its depth is a function of the
// widget's own translucency, so a widget you can see through casts a shadow you
// can see through, and pinning it to one shared depth would make a nearly
// invisible widget cast a solid shadow.  Note that the membership sweep waves
// black through at any alpha, so it checks neither shadow; both therefore carry
// their own assertions.
//
// *The picker's row icons stay `p.blue` rather than becoming the accent.*
// Every row in the picker is drawn identically, so an accent there would be
// saying nothing about any particular row -- and it would cost the accent the
// one job it has in this module, which is to say which widget is selected.
// Within a single render the accent has to mean one thing.

// ============================================================================
// Widget types
// ============================================================================

/// Unique widget instance ID.
pub type WidgetInstanceId = u64;

/// Size of a widget in grid cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WidgetSize {
    pub cols: u32,
    pub rows: u32,
}

impl WidgetSize {
    pub const SMALL: Self = Self { cols: 1, rows: 1 };
    pub const MEDIUM: Self = Self { cols: 2, rows: 1 };
    pub const WIDE: Self = Self { cols: 2, rows: 2 };
    pub const TALL: Self = Self { cols: 1, rows: 2 };
    pub const LARGE: Self = Self { cols: 3, rows: 2 };

    pub fn new(cols: u32, rows: u32) -> Self {
        Self {
            cols: cols.max(1),
            rows: rows.max(1),
        }
    }

    /// Pixel dimensions given a cell size.
    pub fn pixels(&self, cell_w: f32, cell_h: f32, gap: f32) -> (f32, f32) {
        let w = self.cols as f32 * cell_w + (self.cols.saturating_sub(1)) as f32 * gap;
        let h = self.rows as f32 * cell_h + (self.rows.saturating_sub(1)) as f32 * gap;
        (w, h)
    }
}

/// Grid position for a widget (column, row — 0-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPos {
    pub col: u32,
    pub row: u32,
}

impl GridPos {
    pub fn new(col: u32, row: u32) -> Self {
        Self { col, row }
    }

    /// Pixel position given cell size, gap, and origin.
    pub fn pixels(
        &self,
        origin_x: f32,
        origin_y: f32,
        cell_w: f32,
        cell_h: f32,
        gap: f32,
    ) -> (f32, f32) {
        let x = origin_x + self.col as f32 * (cell_w + gap);
        let y = origin_y + self.row as f32 * (cell_h + gap);
        (x, y)
    }
}

/// The block of grid cells a widget covers: a position and a size together.
///
/// This type exists because its three questions — does a cell fall inside,
/// does the block fit the grid, does it overlap another block — were each
/// answered by recomputing `pos.col + size.cols` at the call site, six times
/// across five methods, with `overlaps_any` naming the four edges by hand.
/// Six copies of one formula is six chances to write a different one; and
/// because `u32` addition is not total, each copy was a bounds check that
/// could overflow *while computing the value it was about to bounds-check*.
/// `add_widget(kind, GridPos::new(u32::MAX, 0))` panicked in a debug build,
/// and in a release build wrapped to a small number that *passed* the check —
/// admitting a widget at column `u32::MAX`, which `occupies` then answered
/// about wrongly in turn.
///
/// The predicates below are *exact for every input*, because none of them
/// computes an edge. Each compares a distance against a length instead — see
/// [`span_starts_within`] — so there is no value they can produce that is
/// wrong rather than merely `false`. [`Self::right`] and [`Self::bottom`] do
/// still exist, because rendering and callers want the edge as a number, and
/// those two saturate; they are reporting, not deciding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridRect {
    /// Top-left cell.
    pub pos: GridPos,
    /// Extent in cells.
    pub size: WidgetSize,
}

/// Whether two half-open spans `[a, a + a_len)` and `[b, b + b_len)` share a
/// value.
///
/// Written as "how far past the other does each start, and is that less than
/// the other's length" rather than the textbook `a < b + b_len && b < a +
/// a_len`, because the textbook form adds — and the sum is exactly what does
/// not fit in a `u32` for a span near the end of the range. The comparison
/// picks the branch in which the subtraction cannot go negative, so the
/// `saturating_sub` never actually saturates and the result is exact.
const fn span_starts_within(a: u32, a_len: u32, b: u32, b_len: u32) -> bool {
    if a >= b {
        a.saturating_sub(b) < b_len
    } else {
        b.saturating_sub(a) < a_len
    }
}

impl GridRect {
    pub const fn new(pos: GridPos, size: WidgetSize) -> Self {
        Self { pos, size }
    }

    /// One column past the rightmost cell covered, clamped to `u32::MAX`.
    pub const fn right(&self) -> u32 {
        self.pos.col.saturating_add(self.size.cols)
    }

    /// One row past the bottommost cell covered, clamped to `u32::MAX`.
    pub const fn bottom(&self) -> u32 {
        self.pos.row.saturating_add(self.size.rows)
    }

    /// Whether the cell at `(col, row)` is covered.
    pub const fn contains(&self, col: u32, row: u32) -> bool {
        span_starts_within(col, 1, self.pos.col, self.size.cols)
            && span_starts_within(row, 1, self.pos.row, self.size.rows)
    }

    /// Whether the block lies wholly inside a `columns` × `rows` grid.
    pub const fn fits_in(&self, columns: u32, rows: u32) -> bool {
        // "The grid has room for `cols` more columns starting at `col`" —
        // `checked_sub` is both the room and the test that the block even
        // starts inside the grid.
        let (Some(room_right), Some(room_below)) = (
            columns.checked_sub(self.pos.col),
            rows.checked_sub(self.pos.row),
        ) else {
            return false;
        };
        self.size.cols <= room_right && self.size.rows <= room_below
    }

    /// Whether two blocks share any cell.
    pub const fn intersects(&self, other: &Self) -> bool {
        span_starts_within(self.pos.col, self.size.cols, other.pos.col, other.size.cols)
            && span_starts_within(self.pos.row, self.size.rows, other.pos.row, other.size.rows)
    }
}

/// The type of built-in widget content.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetKind {
    /// Digital clock with date.
    Clock,
    /// Weather summary (current conditions).
    Weather,
    /// CPU/memory/disk usage.
    SystemMonitor,
    /// Small calendar (month view).
    Calendar,
    /// Quick notes / sticky text.
    Notes,
    /// RSS feed headlines.
    RssFeed,
    /// Music player controls.
    MusicPlayer,
    /// Photo slideshow.
    PhotoFrame,
    /// World clocks (multiple timezones).
    WorldClock,
    /// Upcoming events/reminders.
    Reminders,
    /// Disk usage summary.
    DiskUsage,
    /// Network traffic monitor.
    NetworkMonitor,
    /// Battery status.
    BatteryStatus,
    /// Custom widget from a third-party app.
    Custom { app_name: String },
}

impl WidgetKind {
    /// Human-readable label.
    pub fn label(&self) -> &str {
        match self {
            Self::Clock => "Clock",
            Self::Weather => "Weather",
            Self::SystemMonitor => "System Monitor",
            Self::Calendar => "Calendar",
            Self::Notes => "Quick Notes",
            Self::RssFeed => "RSS Feed",
            Self::MusicPlayer => "Music Player",
            Self::PhotoFrame => "Photo Frame",
            Self::WorldClock => "World Clock",
            Self::Reminders => "Reminders",
            Self::DiskUsage => "Disk Usage",
            Self::NetworkMonitor => "Network",
            Self::BatteryStatus => "Battery",
            Self::Custom { .. } => "Custom Widget",
        }
    }

    /// Icon character.
    pub fn icon(&self) -> &str {
        match self {
            Self::Clock => "\u{1F552}",
            Self::Weather => "\u{2600}",
            Self::SystemMonitor => "\u{1F4CA}",
            Self::Calendar => "\u{1F4C5}",
            Self::Notes => "\u{1F4DD}",
            Self::RssFeed => "\u{1F4F0}",
            Self::MusicPlayer => "\u{1F3B5}",
            Self::PhotoFrame => "\u{1F5BC}",
            Self::WorldClock => "\u{1F30D}",
            Self::Reminders => "\u{1F514}",
            Self::DiskUsage => "\u{1F4BE}",
            Self::NetworkMonitor => "\u{1F310}",
            Self::BatteryStatus => "\u{1F50B}",
            Self::Custom { .. } => "\u{1F50C}",
        }
    }

    /// Default size.
    pub fn default_size(&self) -> WidgetSize {
        match self {
            Self::Clock => WidgetSize::SMALL,
            Self::Weather => WidgetSize::MEDIUM,
            Self::SystemMonitor => WidgetSize::MEDIUM,
            Self::Calendar => WidgetSize::WIDE,
            Self::Notes => WidgetSize::MEDIUM,
            Self::RssFeed => WidgetSize::TALL,
            Self::MusicPlayer => WidgetSize::MEDIUM,
            Self::PhotoFrame => WidgetSize::WIDE,
            Self::WorldClock => WidgetSize::MEDIUM,
            Self::Reminders => WidgetSize::TALL,
            Self::DiskUsage => WidgetSize::SMALL,
            Self::NetworkMonitor => WidgetSize::SMALL,
            Self::BatteryStatus => WidgetSize::SMALL,
            Self::Custom { .. } => WidgetSize::MEDIUM,
        }
    }

    /// All built-in widget types (for the add-widget picker).
    pub fn all_builtin() -> Vec<Self> {
        vec![
            Self::Clock,
            Self::Weather,
            Self::SystemMonitor,
            Self::Calendar,
            Self::Notes,
            Self::RssFeed,
            Self::MusicPlayer,
            Self::PhotoFrame,
            Self::WorldClock,
            Self::Reminders,
            Self::DiskUsage,
            Self::NetworkMonitor,
            Self::BatteryStatus,
        ]
    }
}

// ============================================================================
// Widget instance
// ============================================================================

/// A placed widget on the desktop.
#[derive(Clone, Debug)]
pub struct WidgetInstance {
    /// Unique ID.
    pub id: WidgetInstanceId,
    /// What kind of widget.
    pub kind: WidgetKind,
    /// Grid position.
    pub position: GridPos,
    /// Grid size.
    pub size: WidgetSize,
    /// Whether the widget is visible.
    pub visible: bool,
    /// Background opacity (0–255).
    pub bg_opacity: u8,
    /// Custom title override.
    pub title_override: Option<String>,
    /// Last updated timestamp (ms since epoch).
    pub last_updated: u64,
    /// Update interval in ms (0 = static).
    pub update_interval_ms: u64,
    /// Widget-specific state (text for Notes, timezone list for WorldClock, etc.).
    pub state_text: String,
    /// Whether the widget is currently being dragged.
    pub dragging: bool,
}

impl WidgetInstance {
    pub fn new(id: WidgetInstanceId, kind: WidgetKind, position: GridPos) -> Self {
        let size = kind.default_size();
        let update_interval = match &kind {
            WidgetKind::Clock | WidgetKind::SystemMonitor | WidgetKind::NetworkMonitor => 1000,
            WidgetKind::Weather => 600_000,
            WidgetKind::RssFeed => 300_000,
            WidgetKind::BatteryStatus => 30_000,
            _ => 0,
        };
        Self {
            id,
            kind,
            position,
            size,
            visible: true,
            bg_opacity: 200,
            title_override: None,
            last_updated: 0,
            update_interval_ms: update_interval,
            state_text: String::new(),
            dragging: false,
        }
    }

    /// Display title.
    pub fn title(&self) -> &str {
        self.title_override
            .as_deref()
            .unwrap_or_else(|| self.kind.label())
    }

    /// Whether the widget needs an update tick.
    pub fn needs_update(&self, now_ms: u64) -> bool {
        if self.update_interval_ms == 0 {
            return false;
        }
        now_ms.saturating_sub(self.last_updated) >= self.update_interval_ms
    }

    /// The block of grid cells this widget covers.
    pub const fn rect(&self) -> GridRect {
        GridRect::new(self.position, self.size)
    }

    /// Check if a position (in grid cells) overlaps this widget.
    pub const fn occupies(&self, col: u32, row: u32) -> bool {
        self.rect().contains(col, row)
    }
}

// ============================================================================
// Widget grid / manager
// ============================================================================

/// Configuration for the widget grid.
#[derive(Clone, Debug)]
pub struct WidgetGridConfig {
    /// Number of columns.
    pub columns: u32,
    /// Number of rows.
    pub rows: u32,
    /// Cell width in pixels.
    pub cell_width: f32,
    /// Cell height in pixels.
    pub cell_height: f32,
    /// Gap between cells in pixels.
    pub gap: f32,
    /// Grid origin (top-left of widget area).
    pub origin_x: f32,
    pub origin_y: f32,
    /// Corner radius for widget panels.
    pub corner_radius: f32,
}

impl Default for WidgetGridConfig {
    fn default() -> Self {
        Self {
            columns: 8,
            rows: 6,
            cell_width: 180.0,
            cell_height: 150.0,
            gap: 12.0,
            origin_x: 40.0,
            origin_y: 40.0,
            corner_radius: 12.0,
        }
    }
}

/// Manages all desktop widgets.
pub struct DesktopWidgetManager {
    /// All widget instances.
    widgets: Vec<WidgetInstance>,
    /// Grid configuration.
    pub grid: WidgetGridConfig,
    /// Whether the widget layer is visible.
    pub layer_visible: bool,
    /// Whether in edit mode (can move/resize/add/remove widgets).
    pub edit_mode: bool,
    /// Source of widget instance IDs.
    ids: IdSeq<WidgetInstanceId>,
    /// Maximum number of widgets.
    pub max_widgets: usize,
    /// Whether the add-widget picker is open.
    pub picker_open: bool,
    /// Currently selected widget for editing.
    pub selected_widget: Option<WidgetInstanceId>,
}

impl DesktopWidgetManager {
    pub fn new() -> Self {
        Self {
            widgets: Vec::new(),
            grid: WidgetGridConfig::default(),
            layer_visible: true,
            edit_mode: false,
            ids: IdSeq::new(),
            max_widgets: 20,
            picker_open: false,
            selected_widget: None,
        }
    }

    /// Add a widget. Returns the instance ID, or None if rejected.
    pub fn add_widget(&mut self, kind: WidgetKind, position: GridPos) -> Option<WidgetInstanceId> {
        if self.widgets.len() >= self.max_widgets {
            return None;
        }

        let rect = GridRect::new(position, kind.default_size());
        if !self.fits(rect) || self.overlaps_any(rect, None) {
            return None;
        }

        let id = self.ids.issue_infallible();
        self.widgets.push(WidgetInstance::new(id, kind, position));
        Some(id)
    }

    /// Remove a widget by ID.
    pub fn remove_widget(&mut self, id: WidgetInstanceId) -> bool {
        let len_before = self.widgets.len();
        self.widgets.retain(|w| w.id != id);
        if self.selected_widget == Some(id) {
            self.selected_widget = None;
        }
        self.widgets.len() < len_before
    }

    /// Move a widget to a new grid position.
    pub fn move_widget(&mut self, id: WidgetInstanceId, new_pos: GridPos) -> bool {
        // Get the widget's size first.
        let size = match self.widgets.iter().find(|w| w.id == id) {
            Some(w) => w.size,
            None => return false,
        };

        let rect = GridRect::new(new_pos, size);
        if !self.fits(rect) || self.overlaps_any(rect, Some(id)) {
            return false;
        }

        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.position = new_pos;
            true
        } else {
            false
        }
    }

    /// Resize a widget.
    pub fn resize_widget(&mut self, id: WidgetInstanceId, new_size: WidgetSize) -> bool {
        let pos = match self.widgets.iter().find(|w| w.id == id) {
            Some(w) => w.position,
            None => return false,
        };

        let rect = GridRect::new(pos, new_size);
        if !self.fits(rect) || self.overlaps_any(rect, Some(id)) {
            return false;
        }

        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.size = new_size;
            true
        } else {
            false
        }
    }

    /// Toggle visibility of a widget.
    pub fn toggle_visibility(&mut self, id: WidgetInstanceId) -> bool {
        if let Some(w) = self.widgets.iter_mut().find(|w| w.id == id) {
            w.visible = !w.visible;
            true
        } else {
            false
        }
    }

    /// Get a widget by ID.
    pub fn get(&self, id: WidgetInstanceId) -> Option<&WidgetInstance> {
        self.widgets.iter().find(|w| w.id == id)
    }

    /// Get a mutable widget by ID.
    pub fn get_mut(&mut self, id: WidgetInstanceId) -> Option<&mut WidgetInstance> {
        self.widgets.iter_mut().find(|w| w.id == id)
    }

    /// All widgets.
    pub fn all_widgets(&self) -> &[WidgetInstance] {
        &self.widgets
    }

    /// Visible widgets.
    pub fn visible_widgets(&self) -> Vec<&WidgetInstance> {
        self.widgets.iter().filter(|w| w.visible).collect()
    }

    /// Count.
    pub fn count(&self) -> usize {
        self.widgets.len()
    }

    /// Hit-test: which widget (if any) is at a pixel coordinate?
    pub fn hit_test(&self, px: f32, py: f32) -> Option<WidgetInstanceId> {
        for w in self.widgets.iter().rev() {
            if !w.visible {
                continue;
            }
            let (wx, wy) = w.position.pixels(
                self.grid.origin_x,
                self.grid.origin_y,
                self.grid.cell_width,
                self.grid.cell_height,
                self.grid.gap,
            );
            let (ww, wh) =
                w.size
                    .pixels(self.grid.cell_width, self.grid.cell_height, self.grid.gap);
            if px >= wx && px < wx + ww && py >= wy && py < wy + wh {
                return Some(w.id);
            }
        }
        None
    }

    /// Which grid cell is at a pixel coordinate?
    pub fn pixel_to_grid(&self, px: f32, py: f32) -> Option<GridPos> {
        let rel_x = px - self.grid.origin_x;
        let rel_y = py - self.grid.origin_y;
        if rel_x < 0.0 || rel_y < 0.0 {
            return None;
        }
        let step_x = self.grid.cell_width + self.grid.gap;
        let step_y = self.grid.cell_height + self.grid.gap;
        let col = (rel_x / step_x) as u32;
        let row = (rel_y / step_y) as u32;
        if col < self.grid.columns && row < self.grid.rows {
            Some(GridPos::new(col, row))
        } else {
            None
        }
    }

    /// Find the first available position for a widget of the given size.
    pub fn find_free_position(&self, size: WidgetSize) -> Option<GridPos> {
        for row in 0..self.grid.rows {
            for col in 0..self.grid.columns {
                let rect = GridRect::new(GridPos::new(col, row), size);
                if self.fits(rect) && !self.overlaps_any(rect, None) {
                    return Some(rect.pos);
                }
            }
        }
        None
    }

    /// Tick all widgets (update those that need it).
    pub fn tick(&mut self, now_ms: u64) {
        for w in &mut self.widgets {
            if w.needs_update(now_ms) {
                w.last_updated = now_ms;
            }
        }
    }

    /// Render all visible widgets into render commands.
    pub fn render(&self, p: &Palette) -> Vec<RenderCommand> {
        if !self.layer_visible {
            return Vec::new();
        }

        let mut commands = Vec::new();

        // In edit mode, render the grid.
        if self.edit_mode {
            self.render_grid(p, &mut commands);
        }

        // Render each visible widget.
        for w in &self.widgets {
            if !w.visible {
                continue;
            }
            self.render_widget(w, p, &mut commands);
        }

        // Widget picker overlay.
        if self.picker_open {
            self.render_picker(p, &mut commands);
        }

        commands
    }

    // ========================================================================
    // Private
    // ========================================================================

    /// Whether a block lies wholly inside this manager's grid.
    fn fits(&self, rect: GridRect) -> bool {
        rect.fits_in(self.grid.columns, self.grid.rows)
    }

    /// Whether a block would land on any widget other than `exclude`.
    fn overlaps_any(&self, rect: GridRect, exclude: Option<WidgetInstanceId>) -> bool {
        self.widgets
            .iter()
            .filter(|w| exclude != Some(w.id))
            .any(|w| rect.intersects(&w.rect()))
    }

    fn render_grid(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        for row in 0..self.grid.rows {
            for col in 0..self.grid.columns {
                let (x, y) = GridPos::new(col, row).pixels(
                    self.grid.origin_x,
                    self.grid.origin_y,
                    self.grid.cell_width,
                    self.grid.cell_height,
                    self.grid.gap,
                );
                commands.push(RenderCommand::StrokeRect {
                    x,
                    y,
                    width: self.grid.cell_width,
                    height: self.grid.cell_height,
                    color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, 80),
                    line_width: 1.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }
    }

    fn render_widget(&self, w: &WidgetInstance, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let (x, y) = w.position.pixels(
            self.grid.origin_x,
            self.grid.origin_y,
            self.grid.cell_width,
            self.grid.cell_height,
            self.grid.gap,
        );
        let (width, height) =
            w.size
                .pixels(self.grid.cell_width, self.grid.cell_height, self.grid.gap);
        let cr = self.grid.corner_radius;

        // Shadow.
        commands.push(RenderCommand::BoxShadow {
            x,
            y,
            width,
            height,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 12.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, w.bg_opacity / 3),
            corner_radii: CornerRadii::all(cr),
        });

        // Background.
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            color: Color::rgba(p.base.r, p.base.g, p.base.b, w.bg_opacity),
            corner_radii: CornerRadii::all(cr),
        });

        // Selection highlight in edit mode.
        if self.edit_mode && self.selected_widget == Some(w.id) {
            commands.push(RenderCommand::StrokeRect {
                x: x - 2.0,
                y: y - 2.0,
                width: width + 4.0,
                height: height + 4.0,
                color: p.accent,
                line_width: 2.0,
                corner_radii: CornerRadii::all(cr + 2.0),
            });
        }

        // Title bar.
        let title_h = 24.0;
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: title_h,
            color: Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, w.bg_opacity),
            corner_radii: CornerRadii {
                top_left: cr,
                top_right: cr,
                bottom_left: 0.0,
                bottom_right: 0.0,
            },
        });

        // Icon and title.
        commands.push(RenderCommand::Text {
            x: x + 8.0,
            y: y + 4.0,
            text: w.kind.icon().to_string(),
            font_size: 12.0,
            color: Color::rgba(
                p.subtext0.r,
                p.subtext0.g,
                p.subtext0.b,
                (w.bg_opacity as f32 * 1.2) as u8,
            ),
            font_weight: FontWeightHint::Regular,
            max_width: None,
            overflow: TextOverflow::Clip,
        });
        commands.push(RenderCommand::Text {
            x: x + 24.0,
            y: y + 5.0,
            text: w.title().to_string(),
            font_size: 11.0,
            color: Color::rgba(
                p.subtext0.r,
                p.subtext0.g,
                p.subtext0.b,
                (w.bg_opacity as f32 * 1.2) as u8,
            ),
            font_weight: FontWeightHint::Bold,
            max_width: Some(width - 32.0),
            overflow: TextOverflow::Ellipsis,
        });

        // Content area.
        let content_y = y + title_h + 4.0;
        let content_h = height - title_h - 8.0;
        self.render_widget_content(
            w,
            p,
            x + 8.0,
            content_y,
            width - 16.0,
            content_h,
            w.bg_opacity,
            commands,
        );
    }

    fn render_widget_content(
        &self,
        w: &WidgetInstance,
        p: &Palette,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        alpha: u8,
        commands: &mut Vec<RenderCommand>,
    ) {
        match &w.kind {
            WidgetKind::Clock => {
                // Large time display.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 10.0,
                    text: "12:34".to_string(),
                    font_size: 36.0,
                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 55.0,
                    text: "Sunday, May 18".to_string(),
                    font_size: 12.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            WidgetKind::SystemMonitor => {
                // CPU bar.
                let bar_h = 8.0;
                commands.push(RenderCommand::Text {
                    x,
                    y,
                    text: "CPU".to_string(),
                    font_size: 10.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 14.0,
                    width,
                    height: bar_h,
                    color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 14.0,
                    width: width * 0.45,
                    height: bar_h,
                    color: Color::rgba(p.blue.r, p.blue.g, p.blue.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                // Memory bar.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 32.0,
                    text: "Memory".to_string(),
                    font_size: 10.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 46.0,
                    width,
                    height: bar_h,
                    color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 46.0,
                    width: width * 0.62,
                    height: bar_h,
                    color: Color::rgba(p.green.r, p.green.g, p.green.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                // Disk bar.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 64.0,
                    text: "Disk".to_string(),
                    font_size: 10.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 78.0,
                    width,
                    height: bar_h,
                    color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
                commands.push(RenderCommand::FillRect {
                    x,
                    y: y + 78.0,
                    width: width * 0.38,
                    height: bar_h,
                    color: Color::rgba(p.peach.r, p.peach.g, p.peach.b, alpha),
                    corner_radii: CornerRadii::all(4.0),
                });
            }
            WidgetKind::Notes => {
                let display = if w.state_text.is_empty() {
                    "Click to add a note..."
                } else {
                    &w.state_text
                };
                commands.push(RenderCommand::Text {
                    x,
                    y,
                    text: display.to_string(),
                    font_size: 12.0,
                    color: Color::rgba(
                        if w.state_text.is_empty() {
                            p.overlay0.r
                        } else {
                            p.text.r
                        },
                        if w.state_text.is_empty() {
                            p.overlay0.g
                        } else {
                            p.text.g
                        },
                        if w.state_text.is_empty() {
                            p.overlay0.b
                        } else {
                            p.text.b
                        },
                        alpha,
                    ),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            WidgetKind::BatteryStatus => {
                // Battery bar placeholder.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 10.0,
                    text: "\u{1F50B}".to_string(),
                    font_size: 28.0,
                    color: Color::rgba(p.green.r, p.green.g, p.green.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::Text {
                    x: x + 40.0,
                    y: y + 16.0,
                    text: "85%".to_string(),
                    font_size: 20.0,
                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 55.0,
                    text: "3h 42m remaining".to_string(),
                    font_size: 11.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            _ => {
                // Generic placeholder for other widget types.
                commands.push(RenderCommand::Text {
                    x,
                    y: y + height / 2.0 - 10.0,
                    text: w.kind.icon().to_string(),
                    font_size: 32.0,
                    color: Color::rgba(p.surface2.r, p.surface2.g, p.surface2.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                commands.push(RenderCommand::Text {
                    x: x + 40.0,
                    y: y + height / 2.0 - 4.0,
                    text: w.kind.label().to_string(),
                    font_size: 13.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width - 44.0),
                    overflow: TextOverflow::Ellipsis,
                });
            }
        }
    }

    fn render_picker(&self, p: &Palette, commands: &mut Vec<RenderCommand>) {
        let picker_w = 300.0;
        let picker_h = 400.0;
        let px = self.grid.origin_x + 50.0;
        let py = self.grid.origin_y + 50.0;

        // Backdrop.
        commands.push(RenderCommand::BoxShadow {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            offset_x: 0.0,
            offset_y: 6.0,
            blur: 20.0,
            spread: 0.0,
            color: p.shadow(),
            corner_radii: CornerRadii::all(12.0),
        });
        commands.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: p.mantle,
            corner_radii: CornerRadii::all(12.0),
        });
        commands.push(RenderCommand::StrokeRect {
            x: px,
            y: py,
            width: picker_w,
            height: picker_h,
            color: p.surface1,
            line_width: 1.0,
            corner_radii: CornerRadii::all(12.0),
        });

        // Title.
        commands.push(RenderCommand::Text {
            x: px + 16.0,
            y: py + 14.0,
            text: "Add Widget".to_string(),
            font_size: 16.0,
            color: p.text,
            font_weight: FontWeightHint::Bold,
            max_width: None,
            overflow: TextOverflow::Clip,
        });

        // Widget list.
        let mut cy = py + 48.0;
        for kind in WidgetKind::all_builtin() {
            if cy + 32.0 > py + picker_h {
                break;
            }
            commands.push(RenderCommand::Text {
                x: px + 16.0,
                y: cy + 4.0,
                text: kind.icon().to_string(),
                font_size: 16.0,
                color: p.blue,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            commands.push(RenderCommand::Text {
                x: px + 40.0,
                y: cy + 6.0,
                text: kind.label().to_string(),
                font_size: 13.0,
                color: p.text,
                font_weight: FontWeightHint::Regular,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            let sz = kind.default_size();
            commands.push(RenderCommand::Text {
                x: px + picker_w - 60.0,
                y: cy + 8.0,
                text: format!("{}x{}", sz.cols, sz.rows),
                font_size: 10.0,
                color: p.overlay0,
                font_weight: FontWeightHint::Light,
                max_width: None,
                overflow: TextOverflow::Clip,
            });
            cy += 26.0;
        }
    }
}

impl Default for DesktopWidgetManager {
    fn default() -> Self {
        Self::new()
    }
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
    use crate::palette_check::assert_drawn_from;

    fn make_mgr() -> DesktopWidgetManager {
        DesktopWidgetManager::new()
    }

    // ---- GridRect ----

    #[test]
    fn a_block_near_the_end_of_the_coordinate_space_never_fits_a_grid() {
        // This is the bug the type was introduced for. `pos.col + size.cols`
        // at column u32::MAX panics in a debug build and wraps in a release
        // one -- and the wrapped value is small, so it *passes* the bounds
        // check it was computed for.
        let far = GridRect::new(GridPos::new(u32::MAX, u32::MAX), WidgetSize::LARGE);
        assert!(!far.fits_in(8, 6));
        assert!(
            !far.fits_in(u32::MAX, u32::MAX),
            "not even in the largest grid"
        );
        // And it covers no cell, rather than covering a wrapped-around range
        // near the origin.
        assert!(!far.contains(0, 0));
        assert!(!far.contains(1, 1));
    }

    #[test]
    fn a_widget_at_the_far_edge_of_the_grid_is_refused_not_wrapped_into_it() {
        let mut mgr = make_mgr(); // 8x6
        assert_eq!(
            mgr.add_widget(WidgetKind::Clock, GridPos::new(u32::MAX, 0)),
            None
        );
        assert_eq!(
            mgr.add_widget(WidgetKind::Clock, GridPos::new(0, u32::MAX)),
            None
        );
        assert_eq!(mgr.count(), 0, "nothing was admitted");

        // And a real widget cannot be *moved* there either.
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(!mgr.move_widget(id, GridPos::new(u32::MAX, u32::MAX)));
        assert_eq!(mgr.get(id).unwrap().position, GridPos::new(0, 0));
    }

    #[test]
    fn a_block_fits_exactly_up_to_the_last_cell_and_no_further() {
        // 3x2 at (5,4) ends at column 8, row 6 -- flush with an 8x6 grid.
        let flush = GridRect::new(GridPos::new(5, 4), WidgetSize::LARGE);
        assert_eq!((flush.right(), flush.bottom()), (8, 6));
        assert!(flush.fits_in(8, 6));
        assert!(!GridRect::new(GridPos::new(6, 4), WidgetSize::LARGE).fits_in(8, 6));
        assert!(!GridRect::new(GridPos::new(5, 5), WidgetSize::LARGE).fits_in(8, 6));
    }

    #[test]
    fn a_block_covers_exactly_the_cells_it_overlaps() {
        // `contains` and `intersects` are two ways of asking one question;
        // they used to be two formulas. Cross-check them cell by cell.
        let rect = GridRect::new(GridPos::new(2, 1), WidgetSize::new(3, 2));
        for col in 0..8 {
            for row in 0..6 {
                let cell = GridRect::new(GridPos::new(col, row), WidgetSize::SMALL);
                assert_eq!(
                    rect.contains(col, row),
                    rect.intersects(&cell),
                    "cell ({col},{row})"
                );
            }
        }
    }

    #[test]
    fn blocks_that_only_share_an_edge_do_not_overlap() {
        let left = GridRect::new(GridPos::new(0, 0), WidgetSize::new(2, 2));
        for (col, row) in [(2, 0), (0, 2), (2, 2)] {
            let neighbour = GridRect::new(GridPos::new(col, row), WidgetSize::new(2, 2));
            assert!(!left.intersects(&neighbour), "abutting at ({col},{row})");
            assert!(!neighbour.intersects(&left), "overlap is symmetric");
        }
        // One cell closer in either axis and they do share cells.
        let overlapping = GridRect::new(GridPos::new(1, 1), WidgetSize::new(2, 2));
        assert!(left.intersects(&overlapping));
        assert!(overlapping.intersects(&left));
    }

    #[test]
    fn a_free_position_is_one_that_actually_fits_and_is_actually_free() {
        let mut mgr = make_mgr();
        // Fill the grid in a ragged pattern so the search has to work.
        for (col, row) in [(0, 0), (3, 0), (1, 2), (5, 4)] {
            mgr.add_widget(WidgetKind::Calendar, GridPos::new(col, row));
        }
        let size = WidgetSize::WIDE; // 2x2
        let pos = mgr.find_free_position(size).expect("the grid is not full");
        let rect = GridRect::new(pos, size);
        assert!(rect.fits_in(mgr.grid.columns, mgr.grid.rows));
        for w in mgr.all_widgets() {
            assert!(!rect.intersects(&w.rect()), "collides with widget {}", w.id);
        }
        // And the position it reports is one `add_widget` will accept.
        assert!(mgr.add_widget(WidgetKind::Notes, pos).is_some());
    }

    // ---- WidgetSize ----

    #[test]
    fn widget_size_pixels() {
        let size = WidgetSize::MEDIUM; // 2x1
        let (w, h) = size.pixels(180.0, 150.0, 12.0);
        assert!((w - 372.0).abs() < 0.01); // 2*180 + 1*12
        assert!((h - 150.0).abs() < 0.01); // 1*150 + 0*12
    }

    #[test]
    fn widget_size_small_pixels() {
        let size = WidgetSize::SMALL;
        let (w, h) = size.pixels(100.0, 100.0, 10.0);
        assert!((w - 100.0).abs() < 0.01);
        assert!((h - 100.0).abs() < 0.01);
    }

    #[test]
    fn widget_size_new_clamps() {
        let size = WidgetSize::new(0, 0);
        assert_eq!(size.cols, 1);
        assert_eq!(size.rows, 1);
    }

    // ---- GridPos ----

    #[test]
    fn grid_pos_pixels() {
        let pos = GridPos::new(2, 1);
        let (x, y) = pos.pixels(40.0, 40.0, 180.0, 150.0, 12.0);
        assert!((x - 424.0).abs() < 0.01); // 40 + 2*(180+12)
        assert!((y - 202.0).abs() < 0.01); // 40 + 1*(150+12)
    }

    // ---- WidgetKind ----

    #[test]
    fn all_builtin_kinds() {
        let kinds = WidgetKind::all_builtin();
        assert_eq!(kinds.len(), 13);
    }

    #[test]
    fn kind_labels_not_empty() {
        for kind in WidgetKind::all_builtin() {
            assert!(!kind.label().is_empty());
            assert!(!kind.icon().is_empty());
        }
    }

    #[test]
    fn kind_default_sizes() {
        assert_eq!(WidgetKind::Clock.default_size(), WidgetSize::SMALL);
        assert_eq!(WidgetKind::Calendar.default_size(), WidgetSize::WIDE);
        assert_eq!(WidgetKind::SystemMonitor.default_size(), WidgetSize::MEDIUM);
    }

    // ---- WidgetInstance ----

    #[test]
    fn widget_instance_new() {
        let w = WidgetInstance::new(1, WidgetKind::Clock, GridPos::new(0, 0));
        assert_eq!(w.id, 1);
        assert_eq!(w.size, WidgetSize::SMALL);
        assert!(w.visible);
        assert!(!w.dragging);
    }

    #[test]
    fn widget_title_default() {
        let w = WidgetInstance::new(1, WidgetKind::Weather, GridPos::new(0, 0));
        assert_eq!(w.title(), "Weather");
    }

    #[test]
    fn widget_title_override() {
        let mut w = WidgetInstance::new(1, WidgetKind::Weather, GridPos::new(0, 0));
        w.title_override = Some("My Weather".to_string());
        assert_eq!(w.title(), "My Weather");
    }

    #[test]
    fn widget_needs_update() {
        let w = WidgetInstance::new(1, WidgetKind::Clock, GridPos::new(0, 0));
        assert!(w.update_interval_ms > 0);
        assert!(w.needs_update(2000));
        assert!(!WidgetInstance::new(2, WidgetKind::Notes, GridPos::new(0, 0)).needs_update(1000));
    }

    #[test]
    fn widget_occupies() {
        let w = WidgetInstance::new(1, WidgetKind::Calendar, GridPos::new(1, 1));
        // Calendar is 2x2.
        assert!(w.occupies(1, 1));
        assert!(w.occupies(2, 2));
        assert!(!w.occupies(0, 0));
        assert!(!w.occupies(3, 1));
    }

    // ---- DesktopWidgetManager ----

    #[test]
    fn add_widget() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        assert!(id.is_some());
        assert_eq!(mgr.count(), 1);
    }

    #[test]
    fn add_widget_out_of_bounds() {
        let mut mgr = make_mgr();
        let id = mgr.add_widget(WidgetKind::Calendar, GridPos::new(7, 5)); // 2x2 at (7,5) exceeds 8x6
        assert!(id.is_none());
    }

    #[test]
    fn add_widget_overlap_rejected() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Calendar, GridPos::new(0, 0)); // 2x2
        let id2 = mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 1)); // overlaps
        assert!(id2.is_none());
    }

    #[test]
    fn add_widget_adjacent_ok() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0)); // 1x1
        let id2 = mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0)); // 1x1 adjacent
        assert!(id2.is_some());
        assert_eq!(mgr.count(), 2);
    }

    #[test]
    fn max_widgets_enforced() {
        let mut mgr = make_mgr();
        mgr.max_widgets = 2;
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0));
        let id3 = mgr.add_widget(WidgetKind::Clock, GridPos::new(2, 0));
        assert!(id3.is_none());
    }

    #[test]
    fn remove_widget() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(mgr.remove_widget(id));
        assert_eq!(mgr.count(), 0);
    }

    #[test]
    fn remove_nonexistent() {
        let mut mgr = make_mgr();
        assert!(!mgr.remove_widget(999));
    }

    #[test]
    fn move_widget() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(mgr.move_widget(id, GridPos::new(3, 3)));
        assert_eq!(mgr.get(id).unwrap().position, GridPos::new(3, 3));
    }

    #[test]
    fn move_widget_out_of_bounds() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(!mgr.move_widget(id, GridPos::new(8, 0)));
    }

    #[test]
    fn move_widget_overlap() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(2, 2));
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(!mgr.move_widget(id, GridPos::new(2, 2))); // occupied
    }

    #[test]
    fn resize_widget() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(mgr.resize_widget(id, WidgetSize::MEDIUM));
        assert_eq!(mgr.get(id).unwrap().size, WidgetSize::MEDIUM);
    }

    #[test]
    fn resize_widget_blocked_by_overlap() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0));
        assert!(!mgr.resize_widget(id, WidgetSize::MEDIUM)); // would overlap
    }

    #[test]
    fn toggle_visibility() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        assert!(mgr.get(id).unwrap().visible);
        mgr.toggle_visibility(id);
        assert!(!mgr.get(id).unwrap().visible);
        assert_eq!(mgr.visible_widgets().len(), 0);
    }

    #[test]
    fn hit_test() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        // Clock is 1x1 at (40,40) with 180x150 cell.
        assert_eq!(mgr.hit_test(50.0, 50.0), Some(id));
        assert_eq!(mgr.hit_test(300.0, 300.0), None);
    }

    #[test]
    fn pixel_to_grid() {
        let mgr = make_mgr();
        // Origin at (40,40), cell 180x150, gap 12.
        let pos = mgr.pixel_to_grid(50.0, 50.0);
        assert_eq!(pos, Some(GridPos::new(0, 0)));
        let pos2 = mgr.pixel_to_grid(250.0, 50.0);
        assert_eq!(pos2, Some(GridPos::new(1, 0)));
    }

    #[test]
    fn pixel_to_grid_out_of_bounds() {
        let mgr = make_mgr();
        assert_eq!(mgr.pixel_to_grid(0.0, 0.0), None);
    }

    #[test]
    fn find_free_position() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        let free = mgr.find_free_position(WidgetSize::SMALL);
        assert!(free.is_some());
        assert_ne!(free.unwrap(), GridPos::new(0, 0));
    }

    #[test]
    fn find_free_position_none_when_full() {
        let mut mgr = make_mgr();
        mgr.grid.columns = 2;
        mgr.grid.rows = 1;
        mgr.max_widgets = 10;
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::Clock, GridPos::new(1, 0));
        let free = mgr.find_free_position(WidgetSize::SMALL);
        assert!(free.is_none());
    }

    #[test]
    fn tick_updates_timestamps() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        mgr.tick(5000);
        assert_eq!(mgr.get(id).unwrap().last_updated, 5000);
    }

    // ---- Rendering ----

    #[test]
    fn render_empty() {
        let mgr = make_mgr();
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_with_widget() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_hidden_layer() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.layer_visible = false;
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_edit_mode_shows_grid() {
        let mut mgr = make_mgr();
        mgr.edit_mode = true;
        let cmds = mgr.render(&Palette::for_mode(false));
        // Should have grid cells rendered.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_picker() {
        let mut mgr = make_mgr();
        mgr.picker_open = true;
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_system_monitor() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(cmds.len() > 5);
    }

    #[test]
    fn render_notes_empty() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Notes, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_notes_with_text() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Notes, GridPos::new(0, 0))
            .unwrap();
        mgr.get_mut(id).unwrap().state_text = "Hello world".to_string();
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_battery_status() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(cmds.len() > 5);
    }

    #[test]
    fn render_multiple_widgets() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(2, 0));
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(4, 0));
        let cmds = mgr.render(&Palette::for_mode(false));
        assert!(cmds.len() > 15);
    }

    // ---- Colour ----
    //
    // The shape of this set is dictated by what the membership sweep in
    // `palette_check` deliberately *cannot* see, because each blind spot is a
    // test the module owes:
    //
    // - it compares roles on RGB only, so every wash needs its own test;
    // - it waves black through at any alpha, so both shadows need their own;
    // - a role belongs to *both* palettes, so anything that must not follow the
    //   mode or the accent needs its own.
    //
    // And one that is not a blind spot but a width: **the sweep is only as
    // wide as the render it is given.** A colour drawn by a branch no fixture
    // takes is a colour no test checks, however strong the assertions are.
    // Module 21 lost three defects to exactly that, so `full_mgr` is built to
    // take every branch and `the_fixture_takes_every_branch_the_widget_layer_has`
    // pins it.

    /// Accents that are none of the colours this module freezes.
    ///
    /// `blue`, `green` and `peach` are the CPU/Memory/Disk category set, and
    /// `green` is the battery glyph as well. An accent equal to any of them
    /// would make "the meter did not follow the accent" true by coincidence,
    /// in the one run where a real failure would be hardest to notice.
    const SAFE_ACCENTS: [Color; 4] = [
        appearance::MAUVE,
        appearance::TEAL,
        appearance::SAPPHIRE,
        appearance::PINK,
    ];

    /// A `bg_opacity` no default produces, so no wash's alpha can be right by
    /// accident. `WidgetInstance::new` uses 200, and 200 shares its low bits
    /// with several of the constants nearby.
    const ODD_OPACITY: u8 = 137;

    /// What the icon and title texts are washed at: `ODD_OPACITY * 1.2`,
    /// truncated, exactly as `render_widget` computes it.
    const ODD_EMPHASIS: u8 = 164;

    /// What a widget's own shadow is washed at: `ODD_OPACITY / 3`.
    const ODD_SHADOW: u8 = 45;

    const EMPTY_NOTE: &str = "Click to add a note...";
    const WRITTEN_NOTE: &str = "Remember the milk";

    fn rgb(c: Color) -> (u8, u8, u8) {
        (c.r, c.g, c.b)
    }

    /// A manager configured so that `render` takes every branch it has.
    ///
    /// The shape is not arbitrary and must not be trimmed. `render` branches on
    /// `layer_visible`, `edit_mode`, each widget's `visible`, and `picker_open`;
    /// `render_widget` branches on whether the widget is the selected one; and
    /// `render_widget_content` has five arms, one of which branches again on
    /// whether the note is empty. Every one of those branches draws a colour
    /// nothing else draws, so a fixture that misses one takes a colour site out
    /// of *every* test below at once.
    ///
    /// So: `Clock`, `SystemMonitor`, `Notes` twice (empty and written),
    /// `BatteryStatus`, and `Weather` for the generic arm — six visible, plus a
    /// `Calendar` hidden to exercise the `visible` filter. The clock is the
    /// selected widget, edit mode and the picker are both on, and every widget
    /// carries `ODD_OPACITY` so the washes have an alpha that cannot be right by
    /// accident.
    ///
    /// The `layer_visible == false` arm is the one branch not taken here: it
    /// draws nothing at all, which is the point, and
    /// `a_hidden_widget_layer_draws_nothing` checks it separately.
    fn full_mgr() -> DesktopWidgetManager {
        let mut mgr = DesktopWidgetManager::new(); // 8 x 6
        let clock = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(0, 0))
            .unwrap();
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(1, 0))
            .unwrap();
        mgr.add_widget(WidgetKind::Notes, GridPos::new(3, 0))
            .unwrap();
        let written = mgr
            .add_widget(WidgetKind::Notes, GridPos::new(5, 0))
            .unwrap();
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(7, 0))
            .unwrap();
        mgr.add_widget(WidgetKind::Weather, GridPos::new(0, 1))
            .unwrap();
        let hidden = mgr
            .add_widget(WidgetKind::Calendar, GridPos::new(2, 1))
            .unwrap();

        mgr.get_mut(written).unwrap().state_text = WRITTEN_NOTE.to_string();
        assert!(mgr.toggle_visibility(hidden));

        let ids: Vec<_> = mgr.all_widgets().iter().map(|w| w.id).collect();
        for id in ids {
            mgr.get_mut(id).unwrap().bg_opacity = ODD_OPACITY;
        }

        mgr.edit_mode = true;
        mgr.selected_widget = Some(clock);
        mgr.picker_open = true;
        mgr
    }

    /// The same manager with the picker shut.
    ///
    /// The picker draws a row per built-in kind, and those rows reuse the font
    /// sizes the widget bodies use. Closing it is cheaper and clearer than
    /// disambiguating every body assertion by x-coordinate.
    fn body_mgr() -> DesktopWidgetManager {
        let mut mgr = full_mgr();
        mgr.picker_open = false;
        mgr
    }

    /// Every `Text` command that says `want` at `size`, as a colour.
    ///
    /// The font size is part of the key because the same string is drawn more
    /// than once at different sizes — a widget's icon appears in its title bar
    /// at 12pt and again as the generic arm's placeholder at 32pt, and every
    /// kind's label appears in the picker as well as on the widget.
    fn texts_saying(cmds: &[RenderCommand], want: &str, size: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    font_size,
                    color,
                    ..
                } if text == want && (font_size - size).abs() < 0.01 => Some(*color),
                _ => None,
            })
            .collect()
    }

    /// The `FillRect`s that make up the three meters, in emission order:
    /// track, CPU, track, Memory, track, Disk.
    ///
    /// Keyed on the 8px bar height, which nothing else in the module draws.
    fn meter_rects(cmds: &[RenderCommand]) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::FillRect { height, color, .. } if (height - 8.0).abs() < 0.01 => {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    fn strokes_of_width(cmds: &[RenderCommand], lw: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::StrokeRect {
                    color, line_width, ..
                } if (line_width - lw).abs() < 0.01 => Some(*color),
                _ => None,
            })
            .collect()
    }

    fn shadows_with_blur(cmds: &[RenderCommand], blur_want: f32) -> Vec<Color> {
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::BoxShadow { color, blur, .. } if (blur - blur_want).abs() < 0.01 => {
                    Some(*color)
                }
                _ => None,
            })
            .collect()
    }

    fn fills_exactly(cmds: &[RenderCommand], c: Color) -> usize {
        cmds.iter()
            .filter(|k| matches!(k, RenderCommand::FillRect { color, .. } if *color == c))
            .count()
    }

    #[test]
    fn every_colour_the_widget_layer_draws_comes_from_its_palette() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = full_mgr().render(&p);
                assert_drawn_from(
                    &p,
                    &cmds,
                    &[],
                    &format!("widget layer (light={light}, accent={:?})", rgb(accent)),
                );
            }
        }
    }

    /// The fixture draws every branch the widget layer has.
    ///
    /// This is checked against the *render* rather than against the manager's
    /// configuration, because a branch can stop drawing without the state that
    /// feeds it changing at all — and because what the other tests need is the
    /// command, not the intention behind it.
    #[test]
    fn the_fixture_takes_every_branch_the_widget_layer_has() {
        let p = Palette::for_mode(false);
        let body = body_mgr().render(&p);
        let full = full_mgr().render(&p);

        assert_eq!(
            strokes_of_width(&body, 1.0).len(),
            8 * 6,
            "the edit-mode grid is not drawn, so no test sees its wash"
        );
        assert_eq!(
            strokes_of_width(&body, 2.0).len(),
            1,
            "no widget is selected, so no test sees the accent"
        );
        assert_eq!(
            shadows_with_blur(&body, 12.0).len(),
            6,
            "expected six visible widgets, each casting its own shadow"
        );
        assert_eq!(
            shadows_with_blur(&body, 20.0).len(),
            0,
            "the picker is open in the render that is supposed to omit it"
        );
        assert_eq!(
            shadows_with_blur(&full, 20.0).len(),
            1,
            "the picker is not open, so no test sees the shared popup shadow"
        );
        assert_eq!(
            meter_rects(&body).len(),
            6,
            "the three meters are not drawn as three tracks and three bars"
        );

        for (glyph, size, what) in [
            ("12:34", 36.0, "the clock's time"),
            ("Sunday, May 18", 12.0, "the clock's date"),
            ("CPU", 10.0, "a meter's label"),
            (EMPTY_NOTE, 12.0, "the placeholder an empty note draws"),
            (WRITTEN_NOTE, 12.0, "a written note"),
            (WidgetKind::BatteryStatus.icon(), 28.0, "the battery glyph"),
            ("85%", 20.0, "the battery's reading"),
            ("3h 42m remaining", 11.0, "the battery's estimate"),
            (
                WidgetKind::Weather.icon(),
                32.0,
                "the generic arm's placeholder icon",
            ),
            (WidgetKind::Weather.label(), 13.0, "the generic arm's label"),
        ] {
            assert_eq!(
                texts_saying(&body, glyph, size).len(),
                1,
                "{what} is not drawn, so no test in this module checks its colour"
            );
        }

        // The picker's own three text colours.
        for (glyph, size, what) in [
            ("Add Widget", 16.0, "the picker's title"),
            (WidgetKind::Clock.icon(), 16.0, "a picker row's icon"),
            (WidgetKind::Clock.label(), 13.0, "a picker row's label"),
            ("1x1", 10.0, "a picker row's size hint"),
        ] {
            assert!(
                !texts_saying(&full, glyph, size).is_empty(),
                "{what} is not drawn, so no test in this module checks its colour"
            );
        }

        assert!(
            texts_saying(&body, WidgetKind::Calendar.label(), 11.0).is_empty(),
            "the hidden widget is drawn, so the visible filter is untested"
        );
    }

    #[test]
    fn a_hidden_widget_layer_draws_nothing() {
        let mut mgr = full_mgr();
        mgr.layer_visible = false;
        assert!(mgr.render(&Palette::for_mode(false)).is_empty());
    }

    /// The ring around the selected widget is the module's one accent site.
    ///
    /// In edit mode a 2px ring is drawn around exactly the widget you have
    /// picked and around nothing else, which makes it a colour that appears in
    /// exactly one state — and a colour that appears in exactly one state marks
    /// that state. Checked as equality with `p.accent` rather than inequality
    /// with the blue it used to be: a ring that had been frozen to some *other*
    /// literal would pass the inequality and fail the user.
    #[test]
    fn the_selected_widgets_outline_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;

                let ring = strokes_of_width(&full_mgr().render(&p), 2.0);
                assert_eq!(ring.len(), 1, "expected exactly one selection ring");
                assert_eq!(
                    ring[0], p.accent,
                    "the selected widget's ring is not the accent (light={light})"
                );

                // And only while something is selected.
                let mut none = full_mgr();
                none.selected_widget = None;
                assert!(
                    strokes_of_width(&none.render(&p), 2.0).is_empty(),
                    "a ring is drawn with nothing selected (light={light})"
                );

                // And only in edit mode: outside it the ring would be marking a
                // widget the user cannot act on.
                let mut viewing = full_mgr();
                viewing.edit_mode = false;
                assert!(
                    strokes_of_width(&viewing.render(&p), 2.0).is_empty(),
                    "a ring is drawn outside edit mode (light={light})"
                );
            }
        }
    }

    /// Nothing that reports a measurement follows the accent.
    ///
    /// Module 19's slider rule — `surface1` track, accent fill — is about
    /// *controls*. These bars are read-outs nobody can drag, and the battery
    /// glyph's green is not decoration but the reading itself: it is how the
    /// widget says the charge is healthy. Both would be saying something false
    /// the day someone picked a red accent.
    ///
    /// Five source sites, so five assertions per configuration — the three bar
    /// fills, the battery glyph, and the tracks, which keep the half of the
    /// slider rule that does survive.
    #[test]
    fn nothing_that_reports_a_measurement_follows_the_accent() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = full_mgr().render(&p);

                let bars = meter_rects(&cmds);
                assert_eq!(bars.len(), 6);
                for (i, (role, name)) in [(p.blue, "CPU"), (p.green, "Memory"), (p.peach, "Disk")]
                    .into_iter()
                    .enumerate()
                {
                    let track = bars[i * 2];
                    let fill = bars[i * 2 + 1];
                    assert_eq!(
                        rgb(track),
                        rgb(p.surface1),
                        "the {name} track is not surface1 (light={light})"
                    );
                    assert_eq!(
                        rgb(fill),
                        rgb(role),
                        "the {name} bar is not its own role (light={light})"
                    );
                    assert_ne!(
                        rgb(fill),
                        rgb(p.accent),
                        "the {name} bar followed the accent (light={light})"
                    );
                }

                let batt = texts_saying(&cmds, WidgetKind::BatteryStatus.icon(), 28.0);
                assert_eq!(batt.len(), 1);
                assert_eq!(
                    rgb(batt[0]),
                    rgb(p.green),
                    "the battery glyph is not green (light={light})"
                );
                assert_ne!(
                    rgb(batt[0]),
                    rgb(p.accent),
                    "the battery glyph followed the accent (light={light})"
                );
            }
        }
    }

    /// The three meters never look alike.
    ///
    /// This is the property the category judgement exists to protect, and it is
    /// stated separately from "they are blue, green and peach" because it is
    /// the part that would survive a future re-colouring: three bars told apart
    /// by colour stop being three bars the moment any two of them agree.
    #[test]
    fn the_three_meters_never_look_alike() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let bars = meter_rects(&full_mgr().render(&p));
            let fills = [rgb(bars[1]), rgb(bars[3]), rgb(bars[5])];
            for i in 0..fills.len() {
                for j in (i + 1)..fills.len() {
                    assert_ne!(
                        fills[i], fills[j],
                        "two meters are the same colour (light={light})"
                    );
                }
            }
        }
    }

    /// An empty note and a written one never look alike.
    ///
    /// The placeholder is `overlay0` and the note is `text`, which is the same
    /// judgement the rest of the shell makes about prompt text: a prompt is not
    /// content, and the difference has to be visible without reading the words.
    #[test]
    fn an_empty_note_and_a_written_one_never_look_alike() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = body_mgr().render(&p);

            let empty = texts_saying(&cmds, EMPTY_NOTE, 12.0);
            let written = texts_saying(&cmds, WRITTEN_NOTE, 12.0);
            assert_eq!(empty.len(), 1);
            assert_eq!(written.len(), 1);
            assert_eq!(
                rgb(empty[0]),
                rgb(p.overlay0),
                "an empty note's placeholder is not overlay0 (light={light})"
            );
            assert_eq!(
                rgb(written[0]),
                rgb(p.text),
                "a written note is not body text (light={light})"
            );
            assert_ne!(
                rgb(empty[0]),
                rgb(written[0]),
                "a prompt and a note are indistinguishable (light={light})"
            );
        }
    }

    /// Every wash is a role under its own veil.
    ///
    /// The membership sweep compares on RGB only, so it would pass a wash whose
    /// alpha had been dropped, doubled, or swapped with another wash's. Each
    /// alpha is therefore read out of the *render* and compared against what
    /// `render_widget` computes from `bg_opacity` — not against a second lookup
    /// of the same palette, which would only be a second opinion about the same
    /// question.
    ///
    /// `ODD_OPACITY` is deliberately not the default 200: an alpha test against
    /// the value the code would have used anyway proves nothing.
    #[test]
    fn every_wash_the_widget_layer_draws_is_a_role_under_its_own_veil() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let cmds = body_mgr().render(&p);

            // The grid's wash is a fixed 80, independent of any widget: it is a
            // property of the grid, which no widget owns.
            for c in strokes_of_width(&cmds, 1.0) {
                assert_eq!(rgb(c), rgb(p.surface0), "a grid cell is not surface0");
                assert_eq!(c.a, 80, "a grid cell's veil moved (light={light})");
            }

            // Six panels and six title bars, all at the widget's own opacity.
            assert_eq!(
                fills_exactly(
                    &cmds,
                    Color::rgba(p.base.r, p.base.g, p.base.b, ODD_OPACITY)
                ),
                6,
                "a widget panel is not base under its own opacity (light={light})"
            );
            assert_eq!(
                fills_exactly(
                    &cmds,
                    Color::rgba(p.surface0.r, p.surface0.g, p.surface0.b, ODD_OPACITY)
                ),
                6,
                "a widget title bar is not surface0 under its own opacity (light={light})"
            );

            // The title bar's icon and text are emphasised: 1.2x the panel's
            // opacity, so a widget you can barely see still has a readable name.
            for (glyph, size) in [
                (WidgetKind::Clock.icon(), 12.0),
                (WidgetKind::Clock.label(), 11.0),
            ] {
                let t = texts_saying(&cmds, glyph, size);
                assert_eq!(t.len(), 1);
                assert_eq!(rgb(t[0]), rgb(p.subtext0), "a title is not subtext0");
                assert_eq!(
                    t[0].a, ODD_EMPHASIS,
                    "a title is not emphasised over its panel (light={light})"
                );
            }

            // Content is drawn at the panel's own opacity, not the title's.
            //
            // This list has one entry per *source* site, not one per kind of
            // site, and must not be shortened to a representative sample. Two
            // defects escaped the first proof run because it was one: "Memory"
            // and "Disk" are drawn by three separate pushes, and asserting on
            // "CPU" alone left the other two checked by nothing, as it left
            // the battery's estimate. n source sites, n assertions.
            for (glyph, size, role) in [
                ("12:34", 36.0, p.text),
                ("Sunday, May 18", 12.0, p.subtext0),
                ("CPU", 10.0, p.subtext0),
                ("Memory", 10.0, p.subtext0),
                ("Disk", 10.0, p.subtext0),
                (WRITTEN_NOTE, 12.0, p.text),
                (EMPTY_NOTE, 12.0, p.overlay0),
                (WidgetKind::BatteryStatus.icon(), 28.0, p.green),
                ("85%", 20.0, p.text),
                ("3h 42m remaining", 11.0, p.subtext0),
                (WidgetKind::Weather.icon(), 32.0, p.surface2),
                (WidgetKind::Weather.label(), 13.0, p.subtext0),
            ] {
                let t = texts_saying(&cmds, glyph, size);
                assert_eq!(t.len(), 1, "{glyph} at {size} is not drawn once");
                assert_eq!(rgb(t[0]), rgb(role), "{glyph} is drawn in the wrong role");
                assert_eq!(
                    t[0].a, ODD_OPACITY,
                    "{glyph} is not washed at its panel's opacity (light={light})"
                );
            }

            for c in meter_rects(&cmds) {
                assert_eq!(
                    c.a, ODD_OPACITY,
                    "a meter is not washed at its panel's opacity (light={light})"
                );
            }
        }
    }

    /// The picker casts the shared popup shadow.
    ///
    /// The sweep waves black through at any alpha, which is right — a shadow is
    /// an absence of light rather than a colour — and is exactly why a shadow
    /// needs a test of its own. The picker is a panel sitting on top of
    /// everything else, so its depth is the one every popup uses.
    #[test]
    fn the_picker_casts_the_shared_popup_shadow() {
        for light in [false, true] {
            let p = Palette::for_mode(light);
            let s = shadows_with_blur(&full_mgr().render(&p), 20.0);
            assert_eq!(s.len(), 1, "expected exactly one picker shadow");
            assert_eq!(
                s[0],
                p.shadow(),
                "the picker does not cast the shared popup shadow (light={light})"
            );
        }
    }

    /// A widget you can see through casts a shadow you can see through.
    ///
    /// This is the one shadow that does *not* join `Palette::shadow()`, and the
    /// reason is that its depth is a function of the widget's own translucency
    /// rather than of the surface it sits on. Pinning it to the shared depth
    /// would make a nearly invisible widget cast a solid shadow.
    #[test]
    fn a_translucent_widget_casts_a_translucent_shadow() {
        let p = Palette::for_mode(false);

        for s in shadows_with_blur(&body_mgr().render(&p), 12.0) {
            assert_eq!(rgb(s), (0, 0, 0), "a widget's shadow is not black");
            assert_eq!(
                s.a, ODD_SHADOW,
                "a widget's shadow does not track its own opacity"
            );
            assert_ne!(
                s.a,
                p.shadow().a,
                "a widget's shadow was pinned to the shared popup depth"
            );
        }

        // Halve the widget's opacity and its shadow follows.
        let mut fainter = body_mgr();
        let ids: Vec<_> = fainter.all_widgets().iter().map(|w| w.id).collect();
        for id in ids {
            fainter.get_mut(id).unwrap().bg_opacity = 60;
        }
        for s in shadows_with_blur(&fainter.render(&p), 12.0) {
            assert_eq!(s.a, 20, "a fainter widget did not cast a fainter shadow");
        }
    }

    /// The picker's own surfaces come from the palette.
    ///
    /// Six source sites, six assertions. The row icons stay `p.blue` on
    /// purpose: every row is drawn identically, so an accent there would be
    /// saying nothing about any particular row — and it would cost the accent
    /// the one job it has in this module, which is to say which widget is
    /// selected. Within a single render the accent has to mean one thing.
    #[test]
    fn the_pickers_own_surfaces_come_from_the_palette() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = full_mgr().render(&p);

                assert_eq!(
                    fills_exactly(&cmds, p.mantle),
                    1,
                    "the picker's panel is not mantle (light={light})"
                );
                let border = strokes_of_width(&cmds, 1.0);
                assert_eq!(
                    border.iter().filter(|c| **c == p.surface1).count(),
                    1,
                    "the picker's border is not surface1 (light={light})"
                );

                for (glyph, size, role, what) in [
                    ("Add Widget", 16.0, p.text, "the picker's title"),
                    (WidgetKind::Clock.icon(), 16.0, p.blue, "a row's icon"),
                    (WidgetKind::Clock.label(), 13.0, p.text, "a row's label"),
                    ("1x1", 10.0, p.overlay0, "a row's size hint"),
                ] {
                    let t = texts_saying(&cmds, glyph, size);
                    assert!(!t.is_empty(), "{what} is not drawn (light={light})");
                    for c in t {
                        assert_eq!(c, role, "{what} is the wrong role (light={light})");
                        assert_ne!(c, p.accent, "{what} followed the accent (light={light})");
                    }
                }
            }
        }
    }
}
