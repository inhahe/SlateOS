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
use appearance::Surface;
use guitk::color::Color;
use guitk::event::{Key, KeyEvent};
use guitk::idseq::IdSeq;
use guitk::render::{FontWeightHint, RenderCommand, RenderTree, TextOverflow};
use guitk::style::CornerRadii;
use guitk::textarea::{self, TextArea};
use guitk::textinput::KeyEdit;
use yamldoc::Document;

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
/// `span_starts_within` — so there is no value they can produce that is
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
    /// The spelling this kind has in `widgets.yaml`.
    ///
    /// Deliberately *not* [`label`](Self::label). A label is UI text and is
    /// free to change -- "System monitor" could become "Resources" tomorrow --
    /// and if the file spoke labels, that rename would silently discard every
    /// user's placed widget of that kind. The same separation `appearance`
    /// keeps between `label()` and its yaml spelling, for the same reason.
    ///
    /// `Custom` has no spelling of its own: the app name *is* the spelling,
    /// behind a `custom:` prefix so it cannot collide with a built-in.
    #[must_use]
    pub fn yaml_name(&self) -> String {
        match self {
            Self::Clock => "clock".to_string(),
            Self::Weather => "weather".to_string(),
            Self::SystemMonitor => "system_monitor".to_string(),
            Self::Calendar => "calendar".to_string(),
            Self::Notes => "notes".to_string(),
            Self::RssFeed => "rss_feed".to_string(),
            Self::MusicPlayer => "music_player".to_string(),
            Self::PhotoFrame => "photo_frame".to_string(),
            Self::WorldClock => "world_clock".to_string(),
            Self::Reminders => "reminders".to_string(),
            Self::DiskUsage => "disk_usage".to_string(),
            Self::NetworkMonitor => "network_monitor".to_string(),
            Self::BatteryStatus => "battery_status".to_string(),
            Self::Custom { app_name } => format!("custom:{app_name}"),
        }
    }

    /// The kind a `widgets.yaml` spelling names, if this build has it.
    ///
    /// `None` for a name this build does not know, which is how a file written
    /// by a newer desktop degrades: that widget is skipped and the rest load.
    /// Dropping one unknown entry is the right failure -- refusing the whole
    /// file would lose the layout a user does have over one they cannot see.
    #[must_use]
    pub fn from_yaml_name(name: &str) -> Option<Self> {
        if let Some(app) = name.strip_prefix("custom:") {
            // An empty app name is not a widget anyone can draw.
            return (!app.is_empty()).then(|| Self::Custom {
                app_name: app.to_string(),
            });
        }
        Some(match name {
            "clock" => Self::Clock,
            "weather" => Self::Weather,
            "system_monitor" => Self::SystemMonitor,
            "calendar" => Self::Calendar,
            "notes" => Self::Notes,
            "rss_feed" => Self::RssFeed,
            "music_player" => Self::MusicPlayer,
            "photo_frame" => Self::PhotoFrame,
            "world_clock" => Self::WorldClock,
            "reminders" => Self::Reminders,
            "disk_usage" => Self::DiskUsage,
            "network_monitor" => Self::NetworkMonitor,
            "battery_status" => Self::BatteryStatus,
            _ => return None,
        })
    }

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

/// Processor and memory as of the last sample.
///
/// Separated from the reading so the arithmetic can be tested without a
/// `/proc` -- the same seam as `signal_outcome` in the two process managers.
/// A fraction computed from counters is exactly the kind of thing that is
/// plausible when wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SystemSample {
    /// Processor busy over the last interval, 0.0 to 1.0.
    pub cpu_fraction: Option<f32>,
    /// Memory in use, 0.0 to 1.0.
    pub memory_fraction: Option<f32>,
}

impl SystemSample {
    /// Busy time as a fraction of the interval between two `/proc/stat` reads.
    ///
    /// **Two samples, because one cannot answer this.** `/proc/stat` counts
    /// since boot, so a fraction taken from a single sample says how the
    /// machine has spent its life -- after a few hours of uptime that is nearly
    /// constant whatever the machine is doing, and it looks exactly like a live
    /// reading. `CpuTimes::since` exists for this and its own docs say why.
    ///
    /// **`iowait` counts as idle here, and that is a choice.** `top` shows it
    /// as its own column, neither busy nor idle. A desktop meter labelled
    /// "CPU" is read as "how hard is the processor working", and a machine
    /// blocked on a slow disk is not working hard -- so waiting is not busy.
    /// Recorded because a silent convention here becomes unexplainable later.
    ///
    /// `None` when no time passed between the samples: a ratio over a
    /// zero-length interval is not a small number, it is not a number.
    #[must_use]
    pub fn cpu_busy_fraction(prev: &procinfo::CpuTimes, now: &procinfo::CpuTimes) -> Option<f32> {
        let d = now.since(prev);
        let total = d.total();
        if total == 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "tick counts over one second; f32 is exact far beyond this"
        )]
        let idle = (d.idle.saturating_add(d.iowait)) as f32 / total as f32;
        Some((1.0 - idle).clamp(0.0, 1.0))
    }

    /// Memory in use as a fraction of the total.
    ///
    /// **`available`, not `free`.** Free memory excludes the page cache, which
    /// the kernel will hand back the moment anything asks -- reporting it as
    /// "in use" tells the operator their machine is nearly full when it is
    /// doing exactly what it should. `MemAvailable` is the kernel's own answer
    /// to "how much could a new program get", which is the question a meter is
    /// read as answering.
    ///
    /// `None` if either figure is missing or the total is zero, rather than a
    /// fraction of nothing.
    #[must_use]
    pub fn memory_used_fraction(m: procinfo::MemInfo) -> Option<f32> {
        let total = m.total_kib?;
        let available = m.available_kib?;
        if total == 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "kibibytes of RAM; f32 is exact to 16 TiB"
        )]
        let used = total.saturating_sub(available) as f32 / total as f32;
        Some(used.clamp(0.0, 1.0))
    }
}

/// What the battery widget says when there is no battery.
///
/// Distinct from a charge of 0%, which is a battery that is flat. A desktop
/// reader acts differently on the two, and the widget has no business
/// collapsing them.
const NO_BATTERY: &str = "No battery";

/// What a meter's label says when nothing has measured it.
///
/// **Per meter, not per widget, and that distinction is the whole of this
/// change.** The first version set one flag if *any* reading was present and
/// drew a single line underneath. That is correct while all three are absent
/// and wrong the moment one arrives: with CPU and memory measured and disk
/// not, the line disappears and the disk trough sits empty with no
/// explanation -- which reads as *disk at 0%*.
///
/// That is the same argument that made these `Option` rather than `0.0`,
/// resurfacing for the mixed case. A bar at zero is a reading; so is an empty
/// trough with nothing said about it. The label is the one place a reader
/// cannot miss it and cannot attach it to the wrong meter.
///
/// It was found by working out what the *next* change makes true -- wiring
/// `procinfo` gives CPU and memory and cannot give disk, because nothing in
/// this tree reports free space. No test could reach the mixed state before
/// that, so nothing was going to catch it.
const NOT_MEASURED_SUFFIX: &str = " (not measured)";

/// The readings a widget shows that the widget layer cannot derive.
///
/// Supplied by the caller each frame. Everything here is a *formatted string*
/// rather than a number and a format, because the formatting is the part that
/// has to agree with the rest of the desktop.
// `Eq` is gone with the three `Option<f32>` readings: a float has no total
// equality. `PartialEq` is what the tests compare with and is enough.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveReadings {
    /// The time of day, as the taskbar clock would read it.
    pub clock_time: String,
    /// The date beneath it, in the same zone.
    pub clock_date: String,
    /// Processor use, 0.0 to 1.0. `None` when nothing has measured it.
    ///
    /// **These three were constants.** The system-monitor widget drew its CPU
    /// bar at `width * 0.45`, its memory bar at `0.62` and its disk bar at
    /// `0.38` -- the same three figures on every desktop, every frame, for
    /// every machine. A gauge is read at a glance and believed without
    /// thinking, which is exactly what makes a fabricated one expensive.
    ///
    /// `Option`, not a default of zero: a bar at zero is a reading, and "the
    /// processor is idle" is a different claim from "nothing measured the
    /// processor". The widget says which.
    ///
    /// Nothing supplies these yet -- `gui/desktop` has no `procinfo` -- so the
    /// widget reports that it is not measuring. The day the shell reads
    /// `/proc/stat` and `/proc/meminfo`, filling these in is the whole of the
    /// change, and it is a typed seam rather than a plausible number.
    pub cpu_fraction: Option<f32>,
    /// Memory in use, 0.0 to 1.0. `None` when nothing has measured it.
    pub memory_fraction: Option<f32>,
    /// Disk in use, 0.0 to 1.0. `None` when nothing has measured it.
    pub disk_fraction: Option<f32>,
    /// The battery, as the shell knows it.
    ///
    /// **The widget used to draw the strings `"85%"` and `"3h 42m
    /// remaining"`.** Not constants computed from something -- those two
    /// literals, on every desktop, on machines with no battery at all. The
    /// second is worse than the first: a percentage is a claim about now, and
    /// "3h 42m remaining" is a *prediction* somebody plans around.
    ///
    /// `crate::power::BatteryInfo` was in the same crate the whole time, and
    /// its `Default` is `present: false, state: NoBattery` -- already correct,
    /// already honest, and already what production code constructs. The widget
    /// reached past it to invent numbers.
    pub battery: crate::power::BatteryInfo,
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
    /// The note being written in, if one is: at most one at a time, as a
    /// desktop has one keyboard.
    note: Option<NoteEditor>,
    /// How wide to draw a note's caret -- the user's accessibility setting,
    /// passed in by the shell as it is to the icons' rename field.
    caret_width: f32,
}

/// A note open for writing: which widget, and the field its text is in.
///
/// The text is copied back into the widget's `state_text` at every change,
/// so the layout that is saved is never behind what is on screen, and closing
/// the note has nothing left to write.
struct NoteEditor {
    id: WidgetInstanceId,
    area: TextArea,
}

/// What a key did to the note open for writing. See
/// [`DesktopWidgetManager::note_key`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKey {
    /// No note is open; the key is somebody else's.
    NotWriting,
    /// The note took the key and its text is as it was.
    Handled,
    /// The note's text changed: the layout needs saving.
    Changed,
    /// Escape: the note is closed.
    Closed,
}

/// The height of a widget's title bar, which is also where a widget is taken
/// hold of to move it. A note's writing area is everything below it.
const TITLE_HEIGHT: f32 = 24.0;
/// The margin between a widget's sides and its content.
const CONTENT_INSET: f32 = 8.0;
/// The gap above and below the content.
const CONTENT_GAP: f32 = 4.0;
/// The size a note's text is written at.
const NOTE_FONT_SIZE: f32 = 12.0;
/// What an empty note says, which is what a click on it does.
const NOTE_PLACEHOLDER: &str = "Click to add a note...";

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
            note: None,
            caret_width: guitk::textedit::CARET_WIDTH,
        }
    }

    /// How wide to draw a note's caret, from the user's settings.
    pub fn set_caret_width(&mut self, width: f32) {
        self.caret_width = width;
    }

    /// Where a widget is drawn: `(x, y, width, height)` in pixels.
    fn frame(&self, w: &WidgetInstance) -> (f32, f32, f32, f32) {
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
        (x, y, width, height)
    }

    /// Where a widget's content is drawn, below its title bar: one answer for
    /// the drawing and for the clicks on a note, so a caret cannot land a few
    /// pixels from where the text is.
    fn content_of(&self, w: &WidgetInstance) -> (f32, f32, f32, f32) {
        let (x, y, width, height) = self.frame(w);
        (
            x + CONTENT_INSET,
            y + TITLE_HEIGHT + CONTENT_GAP,
            (width - 2.0 * CONTENT_INSET).max(0.0),
            (height - TITLE_HEIGHT - 2.0 * CONTENT_GAP).max(0.0),
        )
    }

    /// Where a widget's content is drawn, `(x, y, width, height)`, below its
    /// title bar -- a note's writing area.
    #[must_use]
    pub fn content_rect(&self, id: WidgetInstanceId) -> Option<(f32, f32, f32, f32)> {
        self.get(id).map(|w| self.content_of(w))
    }

    /// A note's writing area as the field sees it: its top-left, and the box
    /// and font it is laid out in.
    fn note_box(&self, id: WidgetInstanceId) -> Option<(f32, f32, textarea::Metrics)> {
        let w = self.get(id)?;
        let (x, y, width, height) = self.content_of(w);
        Some((
            x,
            y,
            textarea::Metrics {
                width,
                height,
                font_size: NOTE_FONT_SIZE,
                weight: FontWeightHint::Regular,
            },
        ))
    }

    /// The note, if any, whose writing area is at `(x, y)` -- its body, below
    /// the title bar, which stays where a note is taken hold of to move it.
    #[must_use]
    pub fn note_body_at(&self, x: f32, y: f32) -> Option<WidgetInstanceId> {
        let id = self.hit_test(x, y)?;
        let w = self.get(id)?;
        if !matches!(w.kind, WidgetKind::Notes) {
            return None;
        }
        let (_, top, _, _) = self.frame(w);
        (y >= top + TITLE_HEIGHT).then_some(id)
    }

    /// The note open for writing, if one is.
    #[must_use]
    pub fn writing_note(&self) -> Option<WidgetInstanceId> {
        self.note.as_ref().map(|note| note.id)
    }

    /// A press on a note's writing area: open it for writing, if it is not
    /// already, and put the caret where the press landed. `clicks` is two for
    /// a double click, which selects the word. Answers whether the press was
    /// on a note, which is whether it has been used.
    pub fn note_press(&mut self, x: f32, y: f32, clicks: u8) -> bool {
        let Some(id) = self.note_body_at(x, y) else {
            return false;
        };
        if self.writing_note() != Some(id) {
            let text = self
                .get(id)
                .map(|w| w.state_text.clone())
                .unwrap_or_default();
            self.note = Some(NoteEditor {
                id,
                area: TextArea::with_text(&text),
            });
        }
        let Some((left, top, m)) = self.note_box(id) else {
            return false;
        };
        if let Some(note) = self.note.as_mut() {
            note.area.press(x - left, y - top, clicks, false, &m);
        }
        true
    }

    /// The pointer moved to `(x, y)` with the button held after a press on
    /// the open note: the selection follows it.
    pub fn note_drag(&mut self, x: f32, y: f32) {
        let Some(id) = self.writing_note() else {
            return;
        };
        let Some((left, top, m)) = self.note_box(id) else {
            return;
        };
        if let Some(note) = self.note.as_mut() {
            note.area.drag_to(x - left, y - top, &m);
        }
    }

    /// The wheel over the open note: its text scrolls, `notches` as the
    /// event counts them (positive away from the user). Answers whether the
    /// pointer was over it. A note not open for writing shows its start.
    pub fn note_scroll(&mut self, x: f32, y: f32, notches: f32) -> bool {
        let Some(id) = self.writing_note() else {
            return false;
        };
        if self.note_body_at(x, y) != Some(id) {
            return false;
        }
        let Some((_, _, m)) = self.note_box(id) else {
            return false;
        };
        let by = guitk::wheel::pixels(notches, m.line_height());
        if let Some(note) = self.note.as_mut() {
            note.area.scroll_by(by, &m);
        }
        true
    }

    /// Close the open note, if one is. Nothing is lost: its text is already
    /// the widget's.
    pub fn end_note(&mut self) -> bool {
        self.note.take().is_some()
    }

    /// A key while a note is open. Escape closes it; every other key is the
    /// note's -- typing, the arrows, Enter for a new line, the clipboard and
    /// undo chords -- so that nothing typed into a note can reach the icons
    /// beneath it.
    pub fn note_key(&mut self, key: &KeyEvent) -> NoteKey {
        let Some(id) = self.writing_note() else {
            return NoteKey::NotWriting;
        };
        if key.pressed && key.key == Key::Escape {
            self.note = None;
            return NoteKey::Closed;
        }
        let Some((_, _, m)) = self.note_box(id) else {
            // The widget has gone: nothing is open any more.
            self.note = None;
            return NoteKey::Closed;
        };
        let Some(note) = self.note.as_mut() else {
            return NoteKey::NotWriting;
        };
        if note.area.edit_key(key, &m) != KeyEdit::Changed {
            return NoteKey::Handled;
        }
        let text = note.area.text().to_string();
        if let Some(w) = self.get_mut(id) {
            w.state_text = text;
        }
        NoteKey::Changed
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
        if self.writing_note() == Some(id) {
            self.note = None;
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

    /// Write the placed widgets into a configuration document.
    ///
    /// Keyed by ordinal rather than by the runtime `WidgetInstanceId`, because
    /// that id comes from a counter and means nothing across a restart. The
    /// file says *what widgets exist and where*, which is all a layout is.
    ///
    /// Zero-padded so the keys sort the way they are numbered: a file a user
    /// has hand-edited need not preserve document order, and `"10"` sorting
    /// before `"2"` would reorder the layout on a load-save round trip.
    pub fn write_into(&self, doc: &mut Document) {
        doc.remove(&["widgets"]);
        for (i, w) in self.widgets.iter().enumerate() {
            let key = format!("{i:03}");
            doc.set_str(&["widgets", &key, "kind"], &w.kind.yaml_name());
            doc.set_i64(&["widgets", &key, "col"], i64::from(w.position.col));
            doc.set_i64(&["widgets", &key, "row"], i64::from(w.position.row));
            doc.set_i64(&["widgets", &key, "cols"], i64::from(w.size.cols));
            doc.set_i64(&["widgets", &key, "rows"], i64::from(w.size.rows));
            doc.set_bool(&["widgets", &key, "visible"], w.visible);
            // A note's text is the note: a layout that kept where a note is
            // and not what it says would bring back an empty one.
            if matches!(w.kind, WidgetKind::Notes) && !w.state_text.is_empty() {
                doc.set_str(&["widgets", &key, "text"], &w.state_text);
            }
        }
    }

    /// Replace the placed widgets with those in a configuration document.
    ///
    /// Total: an entry that cannot be understood is skipped and the rest load.
    /// An unknown kind is a file from a newer desktop; a position off the grid
    /// or one overlapping a widget already placed is a file somebody edited by
    /// hand. Neither is a reason to discard the widgets that *do* make sense.
    ///
    /// Every entry goes in through [`add_widget`](Self::add_widget) rather
    /// than being pushed directly, which is what keeps the no-overlap
    /// invariant as true for a hand-written file as for a dragged one.
    pub fn read_from(&mut self, doc: &Document) {
        self.widgets.clear();
        let mut keys = doc.keys(&["widgets"]);
        keys.sort();
        for key in keys {
            let Some(kind) = doc
                .get_str(&["widgets", &key, "kind"])
                .and_then(|n| WidgetKind::from_yaml_name(&n))
            else {
                continue;
            };
            let (Some(col), Some(row)) = (
                doc.get_i64(&["widgets", &key, "col"])
                    .and_then(|v| u32::try_from(v).ok()),
                doc.get_i64(&["widgets", &key, "row"])
                    .and_then(|v| u32::try_from(v).ok()),
            ) else {
                continue;
            };
            let Some(id) = self.add_widget(kind, GridPos::new(col, row)) else {
                continue;
            };
            // Size and visibility are corrections to what `add_widget` chose,
            // and each is independently optional: a file missing them still
            // places the widget at its default size, which beats refusing an
            // entry that named a kind and a place.
            if let (Some(cols), Some(rows)) = (
                doc.get_i64(&["widgets", &key, "cols"])
                    .and_then(|v| u32::try_from(v).ok()),
                doc.get_i64(&["widgets", &key, "rows"])
                    .and_then(|v| u32::try_from(v).ok()),
            ) {
                self.resize_widget(id, WidgetSize::new(cols, rows));
            }
            if doc.get_bool(&["widgets", &key, "visible"]) == Some(false)
                && let Some(w) = self.get_mut(id)
            {
                w.visible = false;
            }
            if let Some(text) = doc.get_str(&["widgets", &key, "text"])
                && let Some(w) = self.get_mut(id)
                && matches!(w.kind, WidgetKind::Notes)
            {
                w.state_text = text;
            }
        }
    }

    /// Whether any widget wants redrawing at `now_ms`.
    ///
    /// The session's wake-up gate: an empty desktop, or one whose widgets are
    /// all still within their interval, must let the loop park with no bound
    /// (design-decisions 812). A clock asks for a wake once a minute, and only
    /// while it is on screen.
    #[must_use]
    pub fn needs_tick(&self, now_ms: u64) -> bool {
        self.layer_visible && self.widgets.iter().any(|w| w.needs_update(now_ms))
    }

    /// Milliseconds until the earliest widget is next due, if any is.
    ///
    /// This, and not [`needs_tick`](Self::needs_tick), is what a caller should
    /// arm a wake-up from. `needs_tick` answers "is one due *now*", which is
    /// false for the whole minute between a clock's updates -- so a loop that
    /// parked on it would park unbounded and never come back, and the clock
    /// would show the minute it was created for ever. Asking *when* instead
    /// gives the loop a bound to sleep until, which is the only shape that is
    /// both correct and idle (design-decisions 812).
    #[must_use]
    pub fn next_due_in(&self, now_ms: u64) -> Option<u64> {
        if !self.layer_visible {
            return None;
        }
        self.widgets
            .iter()
            .filter(|w| w.update_interval_ms > 0)
            .map(|w| {
                w.last_updated
                    .saturating_add(w.update_interval_ms)
                    .saturating_sub(now_ms)
            })
            .min()
    }

    /// Tick all widgets, reporting whether any became due.
    ///
    /// It used to return nothing and stamp a field nobody read, which is a
    /// large part of why nothing ever called it.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let mut any = false;
        for w in &mut self.widgets {
            if w.needs_update(now_ms) {
                w.last_updated = now_ms;
                any = true;
            }
        }
        any
    }

    /// Render all visible widgets into render commands.
    ///
    /// `live` carries the readings this layer cannot work out for itself. The
    /// clock's text is formatted by the shell rather than here, because the
    /// shell knows the user's time zone and their 12-versus-24-hour choice --
    /// the taskbar clock reads both out of `DateTimeSettings`, and a widget
    /// clock that formatted its own would be a second answer to the same
    /// question, free to disagree with the one six inches below it.
    pub fn render(&self, p: &Palette, live: &LiveReadings) -> Vec<RenderCommand> {
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
            self.render_widget(w, p, live, &mut commands);
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

    fn render_widget(
        &self,
        w: &WidgetInstance,
        p: &Palette,
        live: &LiveReadings,
        commands: &mut Vec<RenderCommand>,
    ) {
        let (x, y, width, height) = self.frame(w);
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
        commands.push(RenderCommand::FillRect {
            x,
            y,
            width,
            height: TITLE_HEIGHT,
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
        let (content_x, content_y, content_w, content_h) = self.content_of(w);
        self.render_widget_content(
            w,
            p,
            live,
            content_x,
            content_y,
            content_w,
            content_h,
            w.bg_opacity,
            commands,
        );
    }

    fn render_widget_content(
        &self,
        w: &WidgetInstance,
        p: &Palette,
        live: &LiveReadings,
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
                    text: live.clock_time.clone(),
                    font_size: 36.0,
                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 55.0,
                    text: live.clock_date.clone(),
                    font_size: 12.0,
                    color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                    font_weight: FontWeightHint::Regular,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
            }
            WidgetKind::SystemMonitor => {
                let bar_h = 8.0;
                let mut row = y;
                // Each meter keeps its own role colour. Collapsing the three
                // into one blue was caught by `the_three_meters_never_look_alike`
                // and `nothing_that_reports_a_measurement_follows_the_accent`,
                // which are there because a reader tells the meters apart by
                // colour and because a measurement must not wear the accent --
                // the accent marks what the user CHOSE, not what was measured.
                for (label, reading, role) in [
                    ("CPU", live.cpu_fraction, p.blue),
                    ("Memory", live.memory_fraction, p.green),
                    ("Disk", live.disk_fraction, p.peach),
                ] {
                    let heading = if reading.is_some() {
                        label.to_string()
                    } else {
                        format!("{label}{NOT_MEASURED_SUFFIX}")
                    };
                    commands.push(RenderCommand::Text {
                        x,
                        y: row,
                        text: heading,
                        font_size: 10.0,
                        color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                        font_weight: FontWeightHint::Bold,
                        max_width: None,
                        overflow: TextOverflow::Clip,
                    });
                    // The trough is drawn either way: an empty trough is the
                    // shape of a gauge with no needle, which is what this is.
                    commands.push(RenderCommand::FillRect {
                        x,
                        y: row + 14.0,
                        width,
                        height: bar_h,
                        color: Color::rgba(p.surface1.r, p.surface1.g, p.surface1.b, alpha),
                        corner_radii: CornerRadii::all(4.0),
                    });
                    if let Some(f) = reading {
                        commands.push(RenderCommand::FillRect {
                            x,
                            y: row + 14.0,
                            width: width * f.clamp(0.0, 1.0),
                            height: bar_h,
                            color: Color::rgba(role.r, role.g, role.b, alpha),
                            corner_radii: CornerRadii::all(4.0),
                        });
                    }
                    row += 32.0;
                }
            }
            WidgetKind::Notes => {
                // The open note draws its own field -- caret, selection and
                // scroll; a closed one draws its text the same way, from the
                // top, so opening a note does not move a word of it.
                let open = self.note.as_ref().filter(|note| note.id == w.id);
                let closed;
                let area = match open {
                    Some(note) => &note.area,
                    None => {
                        closed = TextArea::with_text(&w.state_text);
                        &closed
                    }
                };
                let ink = |c: Color| Color::rgba(c.r, c.g, c.b, alpha);
                let mut tree = RenderTree::new();
                textarea::draw(
                    &mut tree,
                    &textarea::MultiLine {
                        area,
                        x,
                        y,
                        metrics: textarea::Metrics {
                            width,
                            height,
                            font_size: NOTE_FONT_SIZE,
                            weight: FontWeightHint::Regular,
                        },
                        color: ink(p.text),
                        selection_bg: p.accent,
                        selection_fg: p.on_accent(),
                        focused: open.is_some(),
                        caret_width: self.caret_width,
                        placeholder: Some((NOTE_PLACEHOLDER, ink(p.subtext0))),
                    },
                );
                commands.extend(tree.commands);
            }
            WidgetKind::BatteryStatus => {
                let b = &live.battery;
                commands.push(RenderCommand::Text {
                    x,
                    y,
                    text: "\u{1F50B}".to_string(),
                    font_size: 28.0,
                    // Green when there is a battery, neutral when there is
                    // not. A green battery glyph over "No battery" is a
                    // small claim of its own -- green is the colour of a
                    // healthy thing, and there is no thing.
                    color: if b.present {
                        let g = p.ink(p.green);
                        Color::rgba(g.r, g.g, g.b, alpha)
                    } else {
                        Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha)
                    },
                    font_weight: FontWeightHint::Regular,
                    max_width: None,
                    overflow: TextOverflow::Clip,
                });
                // `present`, not a charge of zero: "no battery" and "a flat
                // battery" are different facts and a desktop reader acts on
                // them differently.
                let headline = if b.present {
                    format!("{}%", b.charge_pct)
                } else {
                    String::from(NO_BATTERY)
                };
                commands.push(RenderCommand::Text {
                    x,
                    y: y + 34.0,
                    text: headline,
                    font_size: 20.0,
                    color: Color::rgba(p.text.r, p.text.g, p.text.b, alpha),
                    font_weight: FontWeightHint::Bold,
                    max_width: Some(width),
                    overflow: TextOverflow::Ellipsis,
                });
                // The estimate is drawn only when there is one. Nothing in
                // this tree computes a time remaining -- `/proc/battery`
                // publishes a charge percentage and a state and no estimate --
                // so this stays absent rather than becoming a second
                // invented line.
                if let Some(secs) = b.time_remaining_secs {
                    commands.push(RenderCommand::Text {
                        x,
                        y: y + 56.0,
                        text: format!("{}h {:02}m remaining", secs / 3600, (secs % 3600) / 60),
                        font_size: 11.0,
                        color: Color::rgba(p.subtext0.r, p.subtext0.g, p.subtext0.b, alpha),
                        font_weight: FontWeightHint::Regular,
                        max_width: Some(width),
                        overflow: TextOverflow::Ellipsis,
                    });
                }
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
        let mut paint = p.surface_paint(Surface::Card);
        paint.border = Some(paint.border.unwrap_or(p.surface1));
        p.push_paint_radii(
            commands,
            px,
            py,
            picker_w,
            picker_h,
            CornerRadii::all(12.0),
            paint,
        );

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
                color: p.ink(p.blue),
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
                color: p.subtext0,
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

    /// Readings a test can assert against, distinct from anything a widget
    /// would draw by accident.
    fn sample_readings() -> LiveReadings {
        LiveReadings {
            clock_time: "07:05".to_string(),
            clock_date: "Tuesday, 3 June".to_string(),
            // Deliberately none of 0.45, 0.62 or 0.38 -- the three constants
            // the widget used to draw. A fixture that happened to match one
            // could not tell a reading from the fabrication it replaced, which
            // is what this helper's own doc comment is already about.
            cpu_fraction: Some(0.11),
            memory_fraction: Some(0.73),
            disk_fraction: Some(0.24),
            // A present battery, so the sweeps below still walk the charge
            // branch -- and 37%, not 85%, because 85 was the literal the
            // widget used to draw and a fixture matching it could not tell a
            // reading from the fabrication it replaced.
            battery: crate::power::BatteryInfo {
                present: true,
                charge_pct: 37,
                time_remaining_secs: Some(9_000),
                ..crate::power::BatteryInfo::default()
            },
        }
    }
    use appearance::palette_check::assert_drawn_from;

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
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_with_widget() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_hidden_layer() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.layer_visible = false;
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(cmds.is_empty());
    }

    #[test]
    fn render_edit_mode_shows_grid() {
        let mut mgr = make_mgr();
        mgr.edit_mode = true;
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        // Should have grid cells rendered.
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_picker() {
        let mut mgr = make_mgr();
        mgr.picker_open = true;
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_system_monitor() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(cmds.len() > 5);
    }

    #[test]
    fn render_notes_empty() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Notes, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_notes_with_text() {
        let mut mgr = make_mgr();
        let id = mgr
            .add_widget(WidgetKind::Notes, GridPos::new(0, 0))
            .unwrap();
        mgr.get_mut(id).unwrap().state_text = "Hello world".to_string();
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(!cmds.is_empty());
    }

    #[test]
    fn render_battery_status() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(0, 0));
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
        assert!(cmds.len() > 5);
    }

    #[test]
    fn render_multiple_widgets() {
        let mut mgr = make_mgr();
        mgr.add_widget(WidgetKind::Clock, GridPos::new(0, 0));
        mgr.add_widget(WidgetKind::SystemMonitor, GridPos::new(2, 0));
        mgr.add_widget(WidgetKind::BatteryStatus, GridPos::new(4, 0));
        let cmds = mgr.render(&Palette::for_mode(false), &sample_readings());
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
        // `RichText` too: a note's lines are drawn by its text field, which
        // colours a selection by span and so draws every line that way. With
        // no span over it, a line is text in the command's own colour.
        cmds.iter()
            .filter_map(|c| match c {
                RenderCommand::Text {
                    text,
                    font_size,
                    color,
                    ..
                } if text == want && (font_size - size).abs() < 0.01 => Some(*color),
                RenderCommand::RichText {
                    text,
                    font_size,
                    color,
                    spans,
                    ..
                } if text == want && spans.is_empty() && (font_size - size).abs() < 0.01 => {
                    Some(*color)
                }
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

    /// Every box painted in colour `c`, filled or outlined.
    ///
    /// Counting `FillRect` alone would see nothing once a surface becomes an
    /// outline, and an assertion of "exactly 1" would fail loudly -- but an
    /// assertion of "0 of the wrong colour" would pass for the wrong reason.
    fn fills_exactly(cmds: &[RenderCommand], c: Color) -> usize {
        cmds.iter()
            .filter_map(appearance::painted_rect)
            .filter(|(_, _, _, _, got)| *got == c)
            .count()
    }

    #[test]
    fn every_colour_the_widget_layer_draws_comes_from_its_palette() {
        for light in [false, true] {
            for accent in SAFE_ACCENTS {
                let mut p = Palette::for_mode(light);
                p.accent = accent;
                let cmds = full_mgr().render(&p, &sample_readings());
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
        let body = body_mgr().render(&p, &sample_readings());
        let full = full_mgr().render(&p, &sample_readings());

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

        let live = sample_readings();
        for (glyph, size, what) in [
            (live.clock_time.as_str(), 36.0, "the clock's time"),
            (live.clock_date.as_str(), 12.0, "the clock's date"),
            ("CPU", 10.0, "a meter's label"),
            (EMPTY_NOTE, 12.0, "the placeholder an empty note draws"),
            (WRITTEN_NOTE, 12.0, "a written note"),
            (WidgetKind::BatteryStatus.icon(), 28.0, "the battery glyph"),
            ("37%", 20.0, "the battery's reading"),
            ("2h 30m remaining", 11.0, "the battery's estimate"),
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
        assert!(
            mgr.render(&Palette::for_mode(false), &sample_readings())
                .is_empty()
        );
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

                let ring = strokes_of_width(&full_mgr().render(&p, &sample_readings()), 2.0);
                assert_eq!(ring.len(), 1, "expected exactly one selection ring");
                assert_eq!(
                    ring[0], p.accent,
                    "the selected widget's ring is not the accent (light={light})"
                );

                // And only while something is selected.
                let mut none = full_mgr();
                none.selected_widget = None;
                assert!(
                    strokes_of_width(&none.render(&p, &sample_readings()), 2.0).is_empty(),
                    "a ring is drawn with nothing selected (light={light})"
                );

                // And only in edit mode: outside it the ring would be marking a
                // widget the user cannot act on.
                let mut viewing = full_mgr();
                viewing.edit_mode = false;
                assert!(
                    strokes_of_width(&viewing.render(&p, &sample_readings()), 2.0).is_empty(),
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
                let cmds = full_mgr().render(&p, &sample_readings());

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
                    rgb(p.ink(p.green)),
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

    fn ticks(user: u64, system: u64, idle: u64, iowait: u64) -> procinfo::CpuTimes {
        procinfo::CpuTimes {
            user,
            system,
            idle,
            iowait,
            ..procinfo::CpuTimes::default()
        }
    }

    /// Busy time is measured over the interval, not over the machine's life.
    ///
    /// **This is the trap a single sample sets.** `/proc/stat` counts since
    /// boot, so a fraction taken from one sample says how the machine has
    /// spent its whole uptime -- after a few hours that is nearly constant
    /// whatever is happening, and it looks exactly like a live reading. It
    /// would be arithmetically correct, derived from genuinely-read kernel
    /// counters, and would pass any test written about it. It just answers a
    /// different question from the one a gauge is read as asking.
    #[test]
    fn the_processor_reading_is_a_ratio_over_the_interval_not_since_boot() {
        // A long-running machine that has been mostly idle, and is now busy.
        let prev = ticks(1_000, 500, 98_500, 0);
        let now = ticks(1_600, 900, 98_500, 0);

        let f = SystemSample::cpu_busy_fraction(&prev, &now).expect("an interval passed");
        assert!(
            (f - 1.0).abs() < 0.001,
            "the interval was entirely busy and read as {f}"
        );

        // The control, and the point of the test: the same `now` read on its
        // own says 2.5%, because that is what this machine has done since it
        // booted. Nothing is wrong with that number except the question it
        // answers.
        let since_boot =
            SystemSample::cpu_busy_fraction(&procinfo::CpuTimes::default(), &now).expect("a total");
        assert!(
            since_boot < 0.05,
            "control: the since-boot figure should be nearly idle, and was {since_boot}"
        );
    }

    /// Waiting on a disk is not working.
    ///
    /// A choice rather than a fact -- `top` shows `iowait` as its own column,
    /// neither busy nor idle. A meter labelled "CPU" is read as "how hard is
    /// the processor working", and a machine blocked on a slow disk is not
    /// working hard.
    #[test]
    fn time_spent_waiting_on_a_disk_is_not_counted_as_busy() {
        let prev = ticks(0, 0, 0, 0);
        let now = ticks(10, 0, 10, 80);
        let f = SystemSample::cpu_busy_fraction(&prev, &now).expect("an interval");
        assert!((f - 0.1).abs() < 0.001, "iowait was counted as work: {f}");
    }

    /// No time between samples is not a small number; it is not a number.
    #[test]
    fn a_zero_length_interval_reports_nothing() {
        let same = ticks(5, 5, 5, 5);
        assert_eq!(SystemSample::cpu_busy_fraction(&same, &same), None);
    }

    /// Memory in use is measured against `available`, not `free`.
    ///
    /// **Free memory excludes the page cache**, which the kernel hands back the
    /// moment anything asks for it. Reporting that as "in use" tells an
    /// operator their machine is nearly full when it is doing exactly what it
    /// should. `MemAvailable` is the kernel's own answer to "how much could a
    /// new program get", which is what a memory meter is read as showing.
    #[test]
    fn memory_in_use_is_measured_against_available_not_free() {
        let m = procinfo::MemInfo {
            total_kib: Some(8_000_000),
            // Almost nothing free, because the cache has taken it -- but most
            // of it available, because the cache will give it back.
            free_kib: Some(200_000),
            available_kib: Some(6_000_000),
            ..procinfo::MemInfo::default()
        };
        let f = SystemSample::memory_used_fraction(m).expect("both figures");
        assert!(
            (f - 0.25).abs() < 0.001,
            "memory read as {f}; against `free` it would have been 0.975"
        );
    }

    /// A missing figure reports nothing rather than a fraction of nothing.
    #[test]
    fn memory_with_no_total_reports_nothing() {
        let m = procinfo::MemInfo {
            total_kib: None,
            available_kib: Some(1_000),
            ..procinfo::MemInfo::default()
        };
        assert_eq!(SystemSample::memory_used_fraction(m), None);
    }

    /// A total of zero reports nothing too, which is not the same case.
    ///
    /// The test above passes `total_kib: None` and returns at the `?` without
    /// ever reaching the zero guard, so it covers a *missing* total and not a
    /// *zero* one. Sabotaging the guard to `Some(0.0)` left that test green,
    /// which is how the gap was found -- the two look like one case and are
    /// two.
    #[test]
    fn memory_with_a_zero_total_reports_nothing() {
        let m = procinfo::MemInfo {
            total_kib: Some(0),
            available_kib: Some(0),
            ..procinfo::MemInfo::default()
        };
        assert_eq!(SystemSample::memory_used_fraction(m), None);
    }

    /// With no battery, the widget says so and predicts nothing.
    ///
    /// **It used to draw the strings `"85%"` and `"3h 42m remaining"`.** Not
    /// numbers computed from something -- those two literals, on every
    /// desktop, including machines with no battery. The second is the worse
    /// of the pair: a percentage is a claim about now, and a time remaining is
    /// a *prediction* somebody plans around.
    ///
    /// `crate::power::BatteryInfo` was in the same crate the whole time, and
    /// its `Default` is `present: false, state: NoBattery` -- already correct,
    /// and already what production code constructs. The widget reached past a
    /// correct model to invent two strings.
    ///
    /// Keyed on `present` rather than on a charge of zero: "there is no
    /// battery" and "the battery is flat" are different facts, and a desktop
    /// reader acts on them differently.
    #[test]
    fn with_no_battery_the_widget_says_so_and_predicts_nothing() {
        let p = Palette::for_mode(false);
        let mut readings = sample_readings();
        readings.battery = crate::power::BatteryInfo::default();

        let texts: Vec<String> = full_mgr()
            .render(&p, &readings)
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        // The control: with a battery present the fixture draws a charge, so
        // this is the same widget minus the battery rather than a widget that
        // has stopped drawing.
        let with_battery: Vec<String> = full_mgr()
            .render(&p, &sample_readings())
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(
            with_battery.iter().any(|t| t == "37%"),
            "control: the battery widget should draw a charge when there is one"
        );

        assert!(
            texts.iter().any(|t| t == NO_BATTERY),
            "the widget drew no charge and did not say there is no battery"
        );
        assert!(
            !texts.iter().any(|t| t.contains("remaining")),
            "a time remaining was predicted for a battery that is not there"
        );
        assert!(
            !texts.iter().any(|t| t.ends_with('%') && t != "37%"),
            "a charge was drawn for a battery that is not there"
        );

        // **The colour too, and this was a real hole.** The role sweep next
        // door renders `sample_readings()`, which has a battery, so it only
        // ever walks the green branch -- sabotaging the glyph to stay green
        // unconditionally left every test passing. Green is the colour of a
        // healthy thing, and over "No battery" there is no thing.
        let glyph = texts_saying(
            &full_mgr().render(&p, &readings),
            WidgetKind::BatteryStatus.icon(),
            28.0,
        );
        assert_eq!(glyph.len(), 1, "control: the glyph should be drawn once");
        assert_ne!(
            rgb(glyph[0]),
            rgb(p.ink(p.green)),
            "the battery glyph stayed green with no battery to be healthy"
        );
    }

    /// With no readings, the meters are empty and the widget says why.
    ///
    /// **The three bars were constants**: CPU at `width * 0.45`, memory at
    /// `0.62`, disk at `0.38`. The same three figures on every desktop, every
    /// frame, on every machine. A gauge is read at a glance and believed
    /// without thinking -- that is what the shape is for -- so the reader never
    /// forms the question this test exists to answer.
    ///
    /// `Option`, not a default of `0.0`, and that is the point: **a bar at zero
    /// is a reading.** "The processor is idle" is a different claim from
    /// "nothing measured the processor". The trough is still drawn, because an
    /// empty gauge is the honest shape of a gauge with no needle, and the line
    /// below says which it is.
    #[test]
    fn with_no_readings_the_meters_are_empty_and_say_so() {
        let p = Palette::for_mode(false);
        let blank = LiveReadings {
            clock_time: "07:05".to_string(),
            clock_date: "Tuesday, 3 June".to_string(),
            cpu_fraction: None,
            memory_fraction: None,
            disk_fraction: None,
            battery: crate::power::BatteryInfo::default(),
        };
        let cmds = full_mgr().render(&p, &blank);
        let bars = meter_rects(&cmds);

        // The control: with readings there are six rects, a track and a fill
        // for each meter. This is the same fixture minus the readings.
        let with_readings = meter_rects(&full_mgr().render(&p, &sample_readings()));
        assert_eq!(
            with_readings.len(),
            6,
            "control: three meters should draw a track and a fill each"
        );
        assert_eq!(
            bars.len(),
            3,
            "an unmeasured meter drew a fill: {} rect(s)",
            bars.len()
        );

        let texts: Vec<String> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        // Each meter says it for itself. A single line under the widget would
        // pass this too, and would then vanish the moment one reading arrived.
        for label in ["CPU", "Memory", "Disk"] {
            assert!(
                texts
                    .iter()
                    .any(|t| t == &format!("{label} (not measured)")),
                "the {label} meter drew an empty trough and did not say why"
            );
        }
    }

    /// With some readings and not others, the unmeasured meter still says so.
    ///
    /// **This is the state the per-meter notice exists for, and it is the one
    /// no test could reach before.** A single flag set by *any* reading, with
    /// one line underneath, is correct while all three are absent and wrong the
    /// moment one arrives: the line goes away and the remaining empty trough
    /// reads as a measurement of zero.
    ///
    /// It is also the state the tree is about to be in. `procinfo` can supply
    /// processor and memory; nothing anywhere reports free disk space, so the
    /// disk meter is going to be `None` while its neighbours are not.
    #[test]
    fn a_measured_meter_beside_an_unmeasured_one_does_not_hide_it() {
        let p = Palette::for_mode(false);
        let mixed = LiveReadings {
            clock_time: "07:05".to_string(),
            clock_date: "Tuesday, 3 June".to_string(),
            cpu_fraction: Some(0.11),
            memory_fraction: Some(0.73),
            disk_fraction: None,
            battery: crate::power::BatteryInfo::default(),
        };
        let cmds = full_mgr().render(&p, &mixed);
        let texts: Vec<String> = cmds
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Text { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();

        // The control: the two measured meters must be drawing fills, or this
        // is the all-absent case wearing a different fixture.
        assert_eq!(
            meter_rects(&cmds).len(),
            5,
            "control: two measured meters and one not should draw three troughs and two fills"
        );
        assert!(
            texts.iter().any(|t| t == "Disk (not measured)"),
            "the unmeasured meter went quiet once its neighbours had readings"
        );
        for measured in ["CPU", "Memory"] {
            assert!(
                texts.iter().any(|t| t == measured),
                "a measured meter was labelled as unmeasured"
            );
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
            let bars = meter_rects(&full_mgr().render(&p, &sample_readings()));
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
            let cmds = body_mgr().render(&p, &sample_readings());

            let empty = texts_saying(&cmds, EMPTY_NOTE, 12.0);
            let written = texts_saying(&cmds, WRITTEN_NOTE, 12.0);
            assert_eq!(empty.len(), 1);
            assert_eq!(written.len(), 1);
            assert_eq!(
                rgb(empty[0]),
                rgb(p.subtext0),
                "an empty note's placeholder is not subtext0 (light={light})"
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
            let cmds = body_mgr().render(&p, &sample_readings());

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
            let live = sample_readings();
            for (glyph, size, role) in [
                (live.clock_time.as_str(), 36.0, p.text),
                (live.clock_date.as_str(), 12.0, p.subtext0),
                ("CPU", 10.0, p.subtext0),
                ("Memory", 10.0, p.subtext0),
                ("Disk", 10.0, p.subtext0),
                (WRITTEN_NOTE, 12.0, p.text),
                (EMPTY_NOTE, 12.0, p.subtext0),
                (WidgetKind::BatteryStatus.icon(), 28.0, p.ink(p.green)),
                ("37%", 20.0, p.text),
                ("2h 30m remaining", 11.0, p.subtext0),
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
            let s = shadows_with_blur(&full_mgr().render(&p, &sample_readings()), 20.0);
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

        for s in shadows_with_blur(&body_mgr().render(&p, &sample_readings()), 12.0) {
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
        for s in shadows_with_blur(&fainter.render(&p, &sample_readings()), 12.0) {
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
                let cmds = full_mgr().render(&p, &sample_readings());

                assert_eq!(
                    fills_exactly(&cmds, p.painted(appearance::Surface::Card)),
                    1,
                    "the picker's panel is not the card surface (light={light})"
                );
                // Whatever the theme outlines a card with -- `surface1` under
                // cards, the border colour under borders. Naming one of them
                // here would make this test pass under one theme only.
                let edge = p
                    .surface_paint(appearance::Surface::Card)
                    .border
                    .unwrap_or(p.surface1);
                let border = strokes_of_width(&cmds, 1.0);
                assert_eq!(
                    border.iter().filter(|c| **c == edge).count(),
                    1,
                    "the picker's border is not the card edge (light={light})"
                );

                for (glyph, size, role, what) in [
                    ("Add Widget", 16.0, p.text, "the picker's title"),
                    (WidgetKind::Clock.icon(), 16.0, p.blue, "a row's icon"),
                    (WidgetKind::Clock.label(), 13.0, p.text, "a row's label"),
                    ("1x1", 10.0, p.subtext0, "a row's size hint"),
                ] {
                    let t = texts_saying(&cmds, glyph, size);
                    assert!(!t.is_empty(), "{what} is not drawn (light={light})");
                    for c in t {
                        assert_eq!(c, role, "{what} is the wrong role (light={light})");
                        assert_ne!(
                            c,
                            p.ink(p.accent),
                            "{what} followed the accent (light={light})"
                        );
                    }
                }
            }
        }
    }

    // ---- a note you can write in ---------------------------------------------

    /// A note on its own, and the middle of its writing area.
    fn one_note() -> (DesktopWidgetManager, WidgetInstanceId, (f32, f32)) {
        let mut mgr = DesktopWidgetManager::new();
        let id = mgr
            .add_widget(WidgetKind::Notes, GridPos::new(0, 0))
            .expect("the grid has room");
        let w = mgr.get(id).expect("just added");
        let (x, y, width, height) = mgr.content_of(w);
        (mgr, id, (x + width / 2.0, y + height / 2.0))
    }

    fn typed(text: &str) -> KeyEvent {
        KeyEvent {
            key: Key::A,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: text.to_string(),
        }
    }

    fn pressed(key: Key) -> KeyEvent {
        KeyEvent {
            key,
            pressed: true,
            modifiers: guitk::event::Modifiers::NONE,
            text: String::new(),
        }
    }

    /// A note's words come back with it -- lines, and characters that are not
    /// ASCII, included -- and an empty note writes no text at all.
    #[test]
    fn a_notes_text_is_kept_with_the_layout() {
        let (mut mgr, id, _) = one_note();
        mgr.get_mut(id).expect("placed").state_text = "milk\neggs — €3\n\nbread".to_string();
        let empty = mgr
            .add_widget(WidgetKind::Notes, GridPos::new(0, 3))
            .expect("room for a second");
        let mut doc = Document::parse("");
        mgr.write_into(&mut doc);

        let mut back = DesktopWidgetManager::new();
        back.read_from(&doc);
        let texts: Vec<&str> = back
            .all_widgets()
            .iter()
            .map(|w| w.state_text.as_str())
            .collect();
        assert_eq!(texts, ["milk\neggs — €3\n\nbread", ""]);
        let keys = doc.keys(&["widgets"]);
        let with_text = keys
            .iter()
            .filter(|k| doc.get_str(&["widgets", k, "text"]).is_some())
            .count();
        assert_eq!(with_text, 1, "an empty note wrote a text: {empty:?}");
    }

    /// The title bar is where a note is taken hold of to move it; the rest is
    /// where it is written in. Other widgets have no writing area at all.
    #[test]
    fn a_notes_body_is_for_writing_and_its_title_bar_for_moving() {
        let (mut mgr, id, (bx, by)) = one_note();
        assert_eq!(mgr.note_body_at(bx, by), Some(id));
        let (x, y, _, _) = mgr.frame(mgr.get(id).expect("placed"));
        assert_eq!(mgr.note_body_at(x + 20.0, y + TITLE_HEIGHT / 2.0), None);

        let clock = mgr
            .add_widget(WidgetKind::Clock, GridPos::new(4, 0))
            .expect("room");
        let (cx, cy, cw, ch) = mgr.content_of(mgr.get(clock).expect("placed"));
        assert_eq!(mgr.note_body_at(cx + cw / 2.0, cy + ch / 2.0), None);
    }

    /// A press opens the note; what is typed is the note's text at once --
    /// Enter included -- and Escape closes it with the words kept.
    #[test]
    fn writing_in_a_note_changes_it_and_escape_closes_it() {
        let (mut mgr, id, (bx, by)) = one_note();
        assert!(mgr.note_press(bx, by, 1));
        assert_eq!(mgr.writing_note(), Some(id));

        assert_eq!(mgr.note_key(&typed("h")), NoteKey::Changed);
        assert_eq!(mgr.note_key(&typed("i")), NoteKey::Changed);
        assert_eq!(mgr.note_key(&pressed(Key::Enter)), NoteKey::Changed);
        assert_eq!(mgr.note_key(&typed("x")), NoteKey::Changed);
        assert_eq!(mgr.get(id).expect("placed").state_text, "hi\nx");
        assert_eq!(mgr.note_key(&pressed(Key::Left)), NoteKey::Handled);
        // Every key is the note's while it is open, even one it does nothing
        // with, so a Delete meant for a letter cannot reach an icon.
        assert_eq!(mgr.note_key(&pressed(Key::Tab)), NoteKey::Handled);

        assert_eq!(mgr.note_key(&pressed(Key::Escape)), NoteKey::Closed);
        assert_eq!(mgr.writing_note(), None);
        assert_eq!(mgr.get(id).expect("placed").state_text, "hi\nx");
        assert_eq!(mgr.note_key(&typed("y")), NoteKey::NotWriting);
    }

    /// A note opened again starts from its saved words, not an empty field.
    #[test]
    fn opening_a_note_starts_from_what_it_says() {
        let (mut mgr, id, (bx, by)) = one_note();
        mgr.get_mut(id).expect("placed").state_text = "kept".to_string();
        assert!(mgr.note_press(bx, by, 1));
        mgr.note_key(&pressed(Key::End));
        assert_eq!(mgr.note_key(&typed("!")), NoteKey::Changed);
        assert_eq!(mgr.get(id).expect("placed").state_text, "kept!");
    }

    /// A note removed while it is open closes: a field left writing into a
    /// widget that is gone would take the next keystrokes to nowhere.
    #[test]
    fn removing_the_open_note_closes_it() {
        let (mut mgr, id, (bx, by)) = one_note();
        assert!(mgr.note_press(bx, by, 1));
        assert!(mgr.remove_widget(id));
        assert_eq!(mgr.writing_note(), None);
        assert_eq!(mgr.note_key(&typed("x")), NoteKey::NotWriting);
    }

    /// The open note has a caret; a closed one, and every other widget, has
    /// none -- a caret says where the next keystroke goes.
    #[test]
    fn only_the_open_note_draws_a_caret() {
        let carets = |mgr: &DesktopWidgetManager| {
            mgr.render(&Palette::for_mode(false), &sample_readings())
                .iter()
                .filter(|c| matches!(c, RenderCommand::Line { .. }))
                .count()
        };
        let (mut mgr, _, (bx, by)) = one_note();
        let closed = carets(&mgr);
        assert!(mgr.note_press(bx, by, 1));
        assert_eq!(carets(&mgr), closed + 1);
        mgr.end_note();
        assert_eq!(carets(&mgr), closed);
    }

    /// A press elsewhere on the desktop is not a press on the note.
    #[test]
    fn a_press_off_every_note_opens_nothing() {
        let (mut mgr, _, _) = one_note();
        assert!(!mgr.note_press(5_000.0, 5_000.0, 1));
        assert_eq!(mgr.writing_note(), None);
    }
}
