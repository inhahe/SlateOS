//! Window snap zones -- Windows 11-style snap layouts for the desktop shell.
//!
//! Provides a zone-based window snapping system that activates when users drag
//! windows near screen edges or corners, or invoke a zone picker via the top
//! edge. Each [`SnapLayout`] defines a set of non-overlapping [`SnapZone`]s
//! covering the work area; the [`SnapManager`] tracks the active layout,
//! performs hit-testing, renders overlays, and maintains per-window snap
//! history so windows can be restored to their pre-snap geometry.
//!
//! # Usage from the desktop shell
//!
//! ```ignore
//! let mut snap = SnapManager::new(WorkArea::new(0.0, 0.0, 1920.0, 1032.0));
//! snap.set_layout(SnapLayoutPreset::TwoEqualHalves);
//!
//! // While user is dragging a window:
//! if cursor_near_top_edge {
//!     snap.show_overlay();
//! }
//! if let Some(zone) = snap.hit_test(cursor_x, cursor_y) {
//!     let highlight = snap.render_zone_highlight(zone.id);
//!     // draw highlight commands
//! }
//!
//! // On drop:
//! let (x, y, w, h) = snap.snap_window(window_id, zone.id);
//! // apply geometry to the window
//! ```

use guitk::color::Color;
use guitk::render::{FontWeightHint, RenderCommand, TextOverflow};
use guitk::style::CornerRadii;
use guitk::text;

use std::collections::HashMap;
use std::num::NonZeroUsize;

// ============================================================================
// Theme -- Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const SURFACE0: Color = Color::from_hex(0x313244);
    pub const BLUE: Color = Color::from_hex(0x89B4FA);
    pub const LAVENDER: Color = Color::from_hex(0xB4BEFE);
    pub const TEXT: Color = Color::from_hex(0xCDD6F4);

    /// Semi-transparent blue fill for zone previews.
    pub const ZONE_FILL: Color = Color::rgba(137, 180, 250, 50);
    /// Slightly more opaque blue for the hovered/highlighted zone.
    pub const ZONE_HIGHLIGHT: Color = Color::rgba(137, 180, 250, 90);
    /// Border colour for zone outlines.
    pub const ZONE_BORDER: Color = Color::rgba(137, 180, 250, 160);
    /// Overlay backdrop (dark scrim behind the zone grid).
    pub const OVERLAY_SCRIM: Color = Color::rgba(30, 30, 46, 140);
    /// Layout picker background.
    pub const PICKER_BG: Color = Color::rgba(30, 30, 46, 230);
    /// Picker item hover.
    pub const PICKER_HOVER: Color = Color::rgba(69, 71, 90, 200);
}

// ============================================================================
// Constants
// ============================================================================

/// Inset (gap) between adjacent zones in pixels.
///
/// Public because the desktop shell snaps through this module and its tests
/// assert the resulting geometry; a private constant would force them to
/// re-state `6.0`, which is how the shell ended up with a second, gapless snap
/// implementation in the first place (see `design-decisions.md` §469).
pub const ZONE_GAP: f32 = 6.0;

/// How close (pixels) the cursor must be to a screen edge to trigger
/// edge/corner snap detection.
const EDGE_THRESHOLD: f32 = 8.0;

/// Distance from top of screen to trigger the zone layout picker.
const TOP_PICKER_THRESHOLD: f32 = 16.0;

/// Width of the layout picker popup.
const PICKER_WIDTH: f32 = 340.0;
/// Padding inside the picker.
const PICKER_PADDING: f32 = 12.0;
/// Size of a single layout thumbnail in the picker.
const THUMB_SIZE: f32 = 72.0;
/// Gap between thumbnails.
const THUMB_GAP: f32 = 10.0;

// ============================================================================
// SnapZone
// ============================================================================

/// Unique identifier for a snap zone within a layout.
pub type ZoneId = u32;

/// A single rectangular zone that a window can snap into.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapZone {
    /// Unique id within the parent layout.
    pub id: ZoneId,
    /// Horizontal position (pixels from left).
    pub x: f32,
    /// Vertical position (pixels from top).
    pub y: f32,
    /// Width in pixels.
    pub width: f32,
    /// Height in pixels.
    pub height: f32,
    /// Human-readable label (e.g. "Left", "Top-Right").
    pub label: String,
}

impl SnapZone {
    /// Returns `true` when the point `(px, py)` lies inside this zone.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width && py >= self.y && py < self.y + self.height
    }

    /// Centre point of the zone.
    pub fn center(&self) -> (f32, f32) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }
}

// ============================================================================
// SnapLayout & Presets
// ============================================================================

/// A named arrangement of zones covering a screen.
#[derive(Clone, Debug)]
pub struct SnapLayout {
    /// Display name for the layout (shown in the picker).
    pub name: String,
    /// The zones that compose this layout.
    pub zones: Vec<SnapZone>,
}

/// Predefined layout presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapLayoutPreset {
    /// Two equal vertical halves (left 50% / right 50%).
    TwoEqualHalves,
    /// Three equal vertical columns (33% each).
    ThreeColumns,
    /// Left column 66%, right column 33%.
    TwoThirdsLeft,
    /// Left column 33%, right column 66%.
    TwoThirdsRight,
    /// Four equal quadrants (2x2 grid).
    FourQuadrants,
    /// Left half + right top/bottom (3 zones).
    ThreeLeftTwoRight,
    /// Six-cell grid (3 columns x 2 rows).
    SixGrid,
}

impl SnapLayoutPreset {
    /// Display name for the picker UI.
    pub fn label(self) -> &'static str {
        match self {
            Self::TwoEqualHalves => "Two Halves",
            Self::ThreeColumns => "Three Columns",
            Self::TwoThirdsLeft => "2/3 + 1/3",
            Self::TwoThirdsRight => "1/3 + 2/3",
            Self::FourQuadrants => "Quadrants",
            Self::ThreeLeftTwoRight => "Left + 2 Right",
            Self::SixGrid => "6-Cell Grid",
        }
    }

    /// All available presets in the order they appear in the picker.
    pub fn all() -> &'static [Self] {
        &[
            Self::TwoEqualHalves,
            Self::ThreeColumns,
            Self::TwoThirdsLeft,
            Self::TwoThirdsRight,
            Self::FourQuadrants,
            Self::ThreeLeftTwoRight,
            Self::SixGrid,
        ]
    }

    /// Build the concrete [`SnapLayout`] filling `area`.
    ///
    /// `area` is the **work area**, not the screen: the region left over once
    /// the taskbar has taken its strip. This used to take a screen width and
    /// height, and every arm below anchored its zones at `(0, 0)` over the
    /// full height — so a window snapped to any zone extended underneath the
    /// taskbar, hiding its own bottom edge. Nothing caught it because nothing
    /// calls this module yet, and because the zone tests assert relationships
    /// between zones (they tile, they do not overlap) which hold just as well
    /// over the wrong rectangle.
    ///
    /// The arms compute in area-local coordinates and the origin is added once
    /// at the end, so a new layout cannot forget to apply it.
    ///
    /// # The gap is an affordance, not an invariant
    ///
    /// Every arm subtracts [`ZONE_GAP`] from the area before dividing it, which
    /// is only meaningful when the area is big enough to give it away. At a
    /// three-pixel-wide work area `(3.0 - 6.0) / 2.0` is a **negative**
    /// half-width, and the second zone starts at 4.5 — outside the very
    /// rectangle it is meant to tile. Degenerate sizes are not hypothetical
    /// here: `DesktopShell::new(1, 1080)` is a legal shell and the
    /// window-manager tests build one at widths down to zero.
    ///
    /// So the gap is *attempted* and kept only if it leaves every zone at
    /// least `ZONE_GAP` in both dimensions; otherwise the layout is rebuilt
    /// edge-to-edge. The rule is applied to the built zones rather than to each
    /// preset's arithmetic so that a preset added later inherits it without
    /// knowing it exists.
    pub fn build(self, area: WorkArea) -> SnapLayout {
        let spaced = self.zones_with_gap(area, ZONE_GAP);
        let zones = if spaced
            .iter()
            .all(|z| z.width >= ZONE_GAP && z.height >= ZONE_GAP)
        {
            spaced
        } else {
            self.zones_with_gap(area, 0.0)
        };

        // The one place the work-area origin is applied. Doing it per-arm
        // would be eleven chances to forget.
        let zones = zones
            .into_iter()
            .map(|z| SnapZone {
                x: z.x + area.x,
                y: z.y + area.y,
                ..z
            })
            .collect();

        SnapLayout {
            name: self.label().to_string(),
            zones,
        }
    }

    /// The preset's zones in **area-local** coordinates, separated by `g`.
    ///
    /// Split out of [`build`](Self::build) so the gap can be chosen by the
    /// caller and the whole layout re-derived without it. Coordinates are
    /// relative to the area's origin; `build` adds it.
    fn zones_with_gap(self, area: WorkArea, g: f32) -> Vec<SnapZone> {
        let screen_w = area.width;
        let screen_h = area.height;

        match self {
            Self::TwoEqualHalves => {
                let half_w = (screen_w - g) / 2.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: half_w,
                        height: screen_h,
                        label: "Left".into(),
                    },
                    SnapZone {
                        id: 1,
                        x: half_w + g,
                        y: 0.0,
                        width: half_w,
                        height: screen_h,
                        label: "Right".into(),
                    },
                ]
            }
            Self::ThreeColumns => {
                let col_w = (screen_w - 2.0 * g) / 3.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: col_w,
                        height: screen_h,
                        label: "Left".into(),
                    },
                    SnapZone {
                        id: 1,
                        x: col_w + g,
                        y: 0.0,
                        width: col_w,
                        height: screen_h,
                        label: "Center".into(),
                    },
                    SnapZone {
                        id: 2,
                        x: 2.0 * (col_w + g),
                        y: 0.0,
                        width: col_w,
                        height: screen_h,
                        label: "Right".into(),
                    },
                ]
            }
            Self::TwoThirdsLeft => {
                let left_w = (screen_w - g) * 2.0 / 3.0;
                let right_w = screen_w - g - left_w;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: left_w,
                        height: screen_h,
                        label: "Left 2/3".into(),
                    },
                    SnapZone {
                        id: 1,
                        x: left_w + g,
                        y: 0.0,
                        width: right_w,
                        height: screen_h,
                        label: "Right 1/3".into(),
                    },
                ]
            }
            Self::TwoThirdsRight => {
                let left_w = (screen_w - g) / 3.0;
                let right_w = screen_w - g - left_w;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: left_w,
                        height: screen_h,
                        label: "Left 1/3".into(),
                    },
                    SnapZone {
                        id: 1,
                        x: left_w + g,
                        y: 0.0,
                        width: right_w,
                        height: screen_h,
                        label: "Right 2/3".into(),
                    },
                ]
            }
            Self::FourQuadrants => {
                let half_w = (screen_w - g) / 2.0;
                let half_h = (screen_h - g) / 2.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: half_w,
                        height: half_h,
                        label: "Top-Left".into(),
                    },
                    SnapZone {
                        id: 1,
                        x: half_w + g,
                        y: 0.0,
                        width: half_w,
                        height: half_h,
                        label: "Top-Right".into(),
                    },
                    SnapZone {
                        id: 2,
                        x: 0.0,
                        y: half_h + g,
                        width: half_w,
                        height: half_h,
                        label: "Bottom-Left".into(),
                    },
                    SnapZone {
                        id: 3,
                        x: half_w + g,
                        y: half_h + g,
                        width: half_w,
                        height: half_h,
                        label: "Bottom-Right".into(),
                    },
                ]
            }
            Self::ThreeLeftTwoRight => {
                let left_w = (screen_w - g) / 2.0;
                let right_w = screen_w - g - left_w;
                let half_h = (screen_h - g) / 2.0;
                vec![
                    SnapZone {
                        id: 0,
                        x: 0.0,
                        y: 0.0,
                        width: left_w,
                        height: screen_h,
                        label: "Left".into(),
                    },
                    SnapZone {
                        id: 1,
                        x: left_w + g,
                        y: 0.0,
                        width: right_w,
                        height: half_h,
                        label: "Top-Right".into(),
                    },
                    SnapZone {
                        id: 2,
                        x: left_w + g,
                        y: half_h + g,
                        width: right_w,
                        height: half_h,
                        label: "Bottom-Right".into(),
                    },
                ]
            }
            Self::SixGrid => {
                let col_w = (screen_w - 2.0 * g) / 3.0;
                let row_h = (screen_h - g) / 2.0;
                // One row of labels per `chunks(3)` row, so the label array's
                // own shape carries the grid. The previous form counted rows
                // and columns and then looked the label back up by index —
                // which needed an `unwrap_or(&"Zone")` fallback for an
                // out-of-range index that the loop bounds already made
                // impossible, i.e. a branch that could never run and could
                // never be tested. Iterating the labels removes the lookup,
                // and with it the dead branch.
                let labels = [
                    ["Top-Left", "Top-Center", "Top-Right"],
                    ["Bottom-Left", "Bottom-Center", "Bottom-Right"],
                ];
                labels
                    .iter()
                    .enumerate()
                    .flat_map(|(row, row_labels)| {
                        row_labels
                            .iter()
                            .enumerate()
                            .map(move |(col, label)| (row, col, *label))
                    })
                    .enumerate()
                    .map(|(idx, (row, col, label))| SnapZone {
                        id: idx as ZoneId,
                        x: col as f32 * (col_w + g),
                        y: row as f32 * (row_h + g),
                        width: col_w,
                        height: row_h,
                        label: label.to_string(),
                    })
                    .collect()
            }
        }
    }
}

/// The rectangle snap zones tile: the screen minus the taskbar's strip.
///
/// A distinct type rather than four loose `f32`s because the previous
/// signature — `build(screen_w, screen_h)` — was *callable* with the work
/// area's size and still wrong, since it had nowhere to put the origin. A
/// caller that has only a screen must now say so explicitly, via
/// [`WorkArea::whole_screen`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkArea {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl WorkArea {
    /// A work area at an explicit origin.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The whole screen, for a display with no reserved chrome.
    ///
    /// Named rather than implied, so that passing a screen size where a work
    /// area belongs is a decision someone wrote down.
    #[must_use]
    pub const fn whole_screen(width: f32, height: f32) -> Self {
        Self::new(0.0, 0.0, width, height)
    }

    /// The x coordinate one past the right edge.
    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    /// The y coordinate one past the bottom edge.
    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

// ============================================================================
// SnapHistory -- per-window pre-snap geometry
// ============================================================================

/// Saved window geometry before snapping, so the window can be restored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedGeometry {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Tracks pre-snap geometry for every snapped window, keyed by window id.
#[derive(Clone, Debug, Default)]
pub struct SnapHistory {
    entries: HashMap<u64, SnapHistoryEntry>,
}

/// A single history record.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SnapHistoryEntry {
    /// The zone the window was snapped to.
    zone_id: ZoneId,
    /// Geometry before the snap.
    saved: SavedGeometry,
}

impl SnapHistory {
    /// Record that `window_id` was snapped to `zone_id` from `geometry`.
    pub fn record(&mut self, window_id: u64, zone_id: ZoneId, geometry: SavedGeometry) {
        self.entries.insert(
            window_id,
            SnapHistoryEntry {
                zone_id,
                saved: geometry,
            },
        );
    }

    /// Retrieve and remove the saved geometry for a window (unsnap).
    pub fn restore(&mut self, window_id: u64) -> Option<SavedGeometry> {
        self.entries.remove(&window_id).map(|e| e.saved)
    }

    /// Check which zone a window is currently snapped to (if any).
    pub fn snapped_zone(&self, window_id: u64) -> Option<ZoneId> {
        self.entries.get(&window_id).map(|e| e.zone_id)
    }

    /// Remove a window from history (e.g. on close).
    pub fn remove(&mut self, window_id: u64) {
        self.entries.remove(&window_id);
    }

    /// Number of tracked windows.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no windows are tracked.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

// ============================================================================
// Edge & corner detection
// ============================================================================

/// Result of detecting which screen edge or corner the cursor is near.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Detect which edge or corner of the work area the cursor is near.
/// Returns `None` if the cursor is not near any edge.
///
/// Measured against the **work area**, not the screen. Against the screen, the
/// bottom edge and both bottom corners sat inside the taskbar's strip: the
/// cursor does travel there during a drag, but it is over the taskbar, and a
/// drag onto the taskbar is a taskbar interaction, not a snap. So the bottom
/// three regions were simultaneously unreachable as snaps and stealing input
/// from the bar. Against the work area they sit just above it, where a user
/// aiming at "the bottom of my desktop" actually points.
pub fn detect_edge(cursor_x: f32, cursor_y: f32, area: WorkArea) -> Option<SnapEdge> {
    let near_left = cursor_x >= area.x && cursor_x < area.x + EDGE_THRESHOLD;
    let near_right = cursor_x < area.right() && cursor_x >= area.right() - EDGE_THRESHOLD;
    let near_top = cursor_y >= area.y && cursor_y < area.y + EDGE_THRESHOLD;
    let near_bottom = cursor_y < area.bottom() && cursor_y >= area.bottom() - EDGE_THRESHOLD;

    match (near_left, near_right, near_top, near_bottom) {
        (true, _, true, _) => Some(SnapEdge::TopLeft),
        (true, _, _, true) => Some(SnapEdge::BottomLeft),
        (_, true, true, _) => Some(SnapEdge::TopRight),
        (_, true, _, true) => Some(SnapEdge::BottomRight),
        (true, _, _, _) => Some(SnapEdge::Left),
        (_, true, _, _) => Some(SnapEdge::Right),
        (_, _, true, _) => Some(SnapEdge::Top),
        (_, _, _, true) => Some(SnapEdge::Bottom),
        _ => None,
    }
}

/// What dropping a window at a given edge or corner should do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeSnap {
    /// Fill the whole work area.
    Maximize,
    /// Occupy one zone of a preset layout.
    Zone(SnapLayoutPreset, ZoneId),
}

/// Map a detected edge or corner to what dropping there should do.
///
/// `None` means "no snap" — the drop is an ordinary window move.
///
/// # What this replaced
///
/// The previous mapping sent the *vertical* edges to the *horizontal* halves:
/// `Top` to the left half (commented "maximize hint", which it was not) and
/// `Bottom` to the right half. So dragging a window to the top of the screen
/// moved it to the left, and dragging it to the bottom moved it to the right —
/// with no relationship between the direction the user dragged and the place
/// the window went. The existing test could not see it: it asserted only that
/// each edge maps to a zone that *exists*.
///
/// Top now maximizes, which is what every desktop this imitates does, and
/// Bottom does nothing — also matching, and the honest answer given that a
/// half-height bottom strip is not one of the presets.
fn edge_to_default_snap(edge: SnapEdge) -> Option<EdgeSnap> {
    match edge {
        SnapEdge::Left => Some(EdgeSnap::Zone(SnapLayoutPreset::TwoEqualHalves, 0)),
        SnapEdge::Right => Some(EdgeSnap::Zone(SnapLayoutPreset::TwoEqualHalves, 1)),
        SnapEdge::Top => Some(EdgeSnap::Maximize),
        SnapEdge::Bottom => None,
        SnapEdge::TopLeft => Some(EdgeSnap::Zone(SnapLayoutPreset::FourQuadrants, 0)),
        SnapEdge::TopRight => Some(EdgeSnap::Zone(SnapLayoutPreset::FourQuadrants, 1)),
        SnapEdge::BottomLeft => Some(EdgeSnap::Zone(SnapLayoutPreset::FourQuadrants, 2)),
        SnapEdge::BottomRight => Some(EdgeSnap::Zone(SnapLayoutPreset::FourQuadrants, 3)),
    }
}

// ============================================================================
// SnapManager
// ============================================================================

/// Main snap-zone manager. Owns the active layout and overlay state.
pub struct SnapManager {
    /// The rectangle zones tile — the screen minus the taskbar.
    area: WorkArea,
    /// Current layout preset.
    active_preset: SnapLayoutPreset,
    /// Current built layout.
    layout: SnapLayout,
    /// Whether the zone overlay is visible (e.g. during a drag).
    overlay_visible: bool,
    /// Whether the three-way layout picker popup is visible.
    picker_visible: bool,
    /// Which preset in the picker is currently hovered (index into
    /// `SnapLayoutPreset::all()`), or `None`.
    picker_hover_index: Option<usize>,
    /// Per-window snap history.
    pub history: SnapHistory,
}

impl SnapManager {
    /// Create a new snap manager tiling the given work area.
    pub fn new(area: WorkArea) -> Self {
        let active_preset = SnapLayoutPreset::TwoEqualHalves;
        let layout = active_preset.build(area);
        Self {
            area,
            active_preset,
            layout,
            overlay_visible: false,
            picker_visible: false,
            picker_hover_index: None,
            history: SnapHistory::default(),
        }
    }

    /// The work area zones are tiled over.
    pub fn work_area(&self) -> WorkArea {
        self.area
    }

    /// Currently active layout preset.
    pub fn active_preset(&self) -> SnapLayoutPreset {
        self.active_preset
    }

    /// Reference to the current layout.
    pub fn layout(&self) -> &SnapLayout {
        &self.layout
    }

    /// Whether the overlay is currently visible.
    pub fn is_overlay_visible(&self) -> bool {
        self.overlay_visible
    }

    /// Whether the layout picker popup is showing.
    pub fn is_picker_visible(&self) -> bool {
        self.picker_visible
    }

    // ======================================================================
    // Layout management
    // ======================================================================

    /// Switch to a different layout preset, rebuilding zones.
    pub fn set_layout(&mut self, preset: SnapLayoutPreset) {
        self.active_preset = preset;
        self.layout = preset.build(self.area);
    }

    /// Recalculate zones after the work area changes.
    ///
    /// That is not only a screen resize: it also happens when the taskbar is
    /// resized, hidden or auto-hidden, which changes the height available
    /// without the display changing at all.
    pub fn set_work_area(&mut self, area: WorkArea) {
        self.area = area;
        self.layout = self.active_preset.build(area);
    }

    // ======================================================================
    // Overlay visibility
    // ======================================================================

    /// Show the snap zone overlay (called when a window drag enters
    /// a trigger region).
    pub fn show_overlay(&mut self) {
        self.overlay_visible = true;
    }

    /// Hide the snap zone overlay.
    pub fn hide_overlay(&mut self) {
        self.overlay_visible = false;
        self.picker_visible = false;
        self.picker_hover_index = None;
    }

    /// Show the three-way zone layout picker (hover near top while
    /// dragging).
    pub fn show_picker(&mut self) {
        self.picker_visible = true;
    }

    /// Hide the picker without selecting a layout.
    pub fn hide_picker(&mut self) {
        self.picker_visible = false;
        self.picker_hover_index = None;
    }

    // ======================================================================
    // Hit testing
    // ======================================================================

    /// Find which zone the point `(x, y)` falls within.
    /// Returns `None` if the cursor is outside all zones.
    pub fn hit_test(&self, x: f32, y: f32) -> Option<&SnapZone> {
        self.layout.zones.iter().find(|z| z.contains(x, y))
    }

    /// Find the zone by id.
    pub fn zone_by_id(&self, zone_id: ZoneId) -> Option<&SnapZone> {
        self.layout.zones.iter().find(|z| z.id == zone_id)
    }

    /// Detect edge/corner proximity and return the matching zone from
    /// an appropriate layout (using the implicit edge-snap rules).
    pub fn edge_snap_hit(&self, cursor_x: f32, cursor_y: f32) -> Option<(SnapEdge, SnapZone)> {
        let edge = detect_edge(cursor_x, cursor_y, self.area)?;
        let zone = match edge_to_default_snap(edge)? {
            EdgeSnap::Maximize => SnapZone {
                id: 0,
                x: self.area.x,
                y: self.area.y,
                width: self.area.width,
                height: self.area.height,
                label: "Maximize".into(),
            },
            EdgeSnap::Zone(preset, zone_id) => preset
                .build(self.area)
                .zones
                .into_iter()
                .find(|z| z.id == zone_id)?,
        };
        Some((edge, zone))
    }

    /// Detect whether the cursor is in the top-edge region that
    /// triggers the layout picker.
    ///
    /// Relative to the work area's top, not the screen's. They coincide with a
    /// bottom taskbar and stop coinciding the moment the bar moves to the top,
    /// where the trigger band would otherwise sit behind it.
    ///
    /// Bounded below as well as above, for the same reason [`detect_edge`] is:
    /// an unbounded `cursor_y < top + THRESHOLD` also matches everything
    /// *above* the work area, which is exactly the strip the taskbar occupies.
    pub fn is_in_picker_trigger(&self, _cursor_x: f32, cursor_y: f32) -> bool {
        cursor_y >= self.area.y && cursor_y < self.area.y + TOP_PICKER_THRESHOLD
    }

    /// Update the picker hover state. `cursor_x` / `cursor_y` are
    /// absolute screen coordinates.
    pub fn update_picker_hover(&mut self, cursor_x: f32, cursor_y: f32) {
        if !self.picker_visible {
            self.picker_hover_index = None;
            return;
        }

        for i in 0..SnapLayoutPreset::all().len() {
            let (ix, iy) = self.thumb_origin(i);

            if cursor_x >= ix
                && cursor_x < ix + THUMB_SIZE
                && cursor_y >= iy
                && cursor_y < iy + THUMB_SIZE
            {
                self.picker_hover_index = Some(i);
                return;
            }
        }
        self.picker_hover_index = None;
    }

    /// If the picker is showing and the user clicks, select the hovered
    /// layout. Returns `true` if a selection was made.
    pub fn picker_select(&mut self) -> bool {
        if let Some(idx) = self.picker_hover_index {
            let presets = SnapLayoutPreset::all();
            if let Some(&preset) = presets.get(idx) {
                self.set_layout(preset);
                self.picker_visible = false;
                self.picker_hover_index = None;
                return true;
            }
        }
        false
    }

    // ======================================================================
    // Snapping
    // ======================================================================

    /// Snap a window to the given zone. Returns the target geometry
    /// `(x, y, width, height)`.
    ///
    /// The caller should record the window's pre-snap geometry via
    /// `history.record()` before calling this if restore-on-unsnap
    /// is desired.
    pub fn snap_window(&mut self, window_id: u64, zone_id: ZoneId) -> Option<(f32, f32, f32, f32)> {
        let zone = self.zone_by_id(zone_id)?;
        let geom = (zone.x, zone.y, zone.width, zone.height);
        // Ensure zone_id is tracked in history. If the caller already
        // recorded pre-snap geometry we just update the zone reference;
        // if not we record a zero-geometry placeholder (the caller is
        // responsible for providing real geometry via `history.record()`).
        if self.history.snapped_zone(window_id).is_none() {
            self.history.record(
                window_id,
                zone_id,
                SavedGeometry {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            );
        }
        Some(geom)
    }

    /// Snap a window using edge/corner detection instead of the layout
    /// overlay. Returns the same `(x, y, width, height)` tuple on
    /// success.
    pub fn snap_window_to_edge(
        &mut self,
        window_id: u64,
        cursor_x: f32,
        cursor_y: f32,
    ) -> Option<(f32, f32, f32, f32)> {
        let (_edge, zone) = self.edge_snap_hit(cursor_x, cursor_y)?;
        let geom = (zone.x, zone.y, zone.width, zone.height);
        if self.history.snapped_zone(window_id).is_none() {
            self.history.record(
                window_id,
                zone.id,
                SavedGeometry {
                    x: 0.0,
                    y: 0.0,
                    width: 0.0,
                    height: 0.0,
                },
            );
        }
        Some(geom)
    }

    // ======================================================================
    // Rendering -- overlay
    // ======================================================================

    /// Render the full snap zone overlay (semi-transparent zone
    /// previews over the entire screen).
    pub fn render_overlay(&self) -> Vec<RenderCommand> {
        if !self.overlay_visible {
            return Vec::new();
        }

        // Saturating for the same reason as in `render_picker`: a capacity
        // hint that panics is worse than one that is merely wrong.
        let mut cmds =
            Vec::with_capacity(self.layout.zones.len().saturating_mul(3).saturating_add(1));

        // Scrim behind everything — over the work area only, so the taskbar
        // is not dimmed along with the desktop it sits beside.
        cmds.push(RenderCommand::FillRect {
            x: self.area.x,
            y: self.area.y,
            width: self.area.width,
            height: self.area.height,
            color: theme::OVERLAY_SCRIM,
            corner_radii: CornerRadii::ZERO,
        });

        for zone in &self.layout.zones {
            // Zone fill.
            cmds.push(RenderCommand::FillRect {
                x: zone.x,
                y: zone.y,
                width: zone.width,
                height: zone.height,
                color: theme::ZONE_FILL,
                corner_radii: CornerRadii::all(8.0),
            });

            // Zone border.
            cmds.push(RenderCommand::StrokeRect {
                x: zone.x,
                y: zone.y,
                width: zone.width,
                height: zone.height,
                color: theme::ZONE_BORDER,
                line_width: 2.0,
                corner_radii: CornerRadii::all(8.0),
            });

            // Zone label centred.
            let (cx, cy) = zone.center();
            cmds.push(RenderCommand::Text {
                x: text::center_x(&zone.label, cx, 13.0, FontWeightHint::Regular),
                y: cy - 7.0,
                text: zone.label.clone(),
                color: theme::TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(zone.width - 16.0),
                overflow: TextOverflow::Ellipsis,
            });
        }

        cmds
    }

    /// Render a highlight for a single zone (the one the cursor is
    /// hovering over).
    pub fn render_zone_highlight(&self, zone_id: ZoneId) -> Vec<RenderCommand> {
        let zone = match self.zone_by_id(zone_id) {
            Some(z) => z,
            None => return Vec::new(),
        };

        let mut cmds = Vec::with_capacity(3);

        // Highlighted fill.
        cmds.push(RenderCommand::FillRect {
            x: zone.x,
            y: zone.y,
            width: zone.width,
            height: zone.height,
            color: theme::ZONE_HIGHLIGHT,
            corner_radii: CornerRadii::all(8.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: zone.x,
            y: zone.y,
            width: zone.width,
            height: zone.height,
            color: theme::BLUE,
            line_width: 3.0,
            corner_radii: CornerRadii::all(8.0),
        });

        // Label.
        let (cx, cy) = zone.center();
        cmds.push(RenderCommand::Text {
            x: text::center_x(&zone.label, cx, 14.0, FontWeightHint::Bold),
            y: cy - 8.0,
            text: zone.label.clone(),
            color: Color::WHITE,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(zone.width - 16.0),
            overflow: TextOverflow::Ellipsis,
        });

        cmds
    }

    // ======================================================================
    // Rendering -- layout picker popup
    // ======================================================================

    /// Top-left corner of the picker popup, centred at the top of the work
    /// area — the same rectangle the trigger band is measured against, so the
    /// popup appears where the gesture that summons it happens.
    fn picker_origin(&self) -> (f32, f32) {
        let px = self.area.x + (self.area.width - PICKER_WIDTH) / 2.0;
        let py = self.area.y + TOP_PICKER_THRESHOLD + 4.0;
        (px, py)
    }

    /// How many thumbnail items fit in one row.
    ///
    /// `NonZeroUsize` rather than `usize`: the `.max(1)` below already made
    /// zero impossible, but only the type says so, and both callers divide by
    /// this. A guarantee the compiler can see is worth more than one a reader
    /// has to go and check.
    fn picker_items_per_row(&self) -> NonZeroUsize {
        let usable = PICKER_WIDTH - 2.0 * PICKER_PADDING + THUMB_GAP;
        NonZeroUsize::new((usable / (THUMB_SIZE + THUMB_GAP)) as usize).unwrap_or(NonZeroUsize::MIN)
    }

    /// Top-left corner of the `index`th thumbnail in the picker grid.
    ///
    /// Shared by `update_picker_hover` and `render_picker`. They previously
    /// each computed this grid from scratch, with the two copies four hundred
    /// lines apart — an arrangement in which editing one padding constant
    /// gives you a picker that highlights one thumbnail and selects another,
    /// and no test that would notice.
    fn thumb_origin(&self, index: usize) -> (f32, f32) {
        let (px, py) = self.picker_origin();
        let per_row = self.picker_items_per_row();
        // `usize % NonZeroUsize` and `usize / NonZeroUsize`: infallible by
        // type, so there is no divide-by-zero branch to write or to test.
        let col = index % per_row;
        let row = index / per_row;
        (
            px + PICKER_PADDING + col as f32 * (THUMB_SIZE + THUMB_GAP),
            py + PICKER_PADDING + 24.0 + row as f32 * (THUMB_SIZE + THUMB_GAP),
        )
    }

    /// Render the layout picker popup.
    pub fn render_picker(&self) -> Vec<RenderCommand> {
        if !self.picker_visible {
            return Vec::new();
        }

        let (px, py) = self.picker_origin();
        let presets = SnapLayoutPreset::all();
        let per_row = self.picker_items_per_row();

        let rows = presets.len().div_ceil(per_row.get());
        let picker_h =
            PICKER_PADDING * 2.0 + 24.0 + rows as f32 * (THUMB_SIZE + THUMB_GAP) - THUMB_GAP;

        // Saturating, because this is a capacity hint: a wrong answer costs a
        // reallocation, and an overflow panic on a hint would be absurd.
        let mut cmds = Vec::with_capacity(presets.len().saturating_mul(8).saturating_add(4));

        // Shadow.
        cmds.push(RenderCommand::BoxShadow {
            x: px,
            y: py,
            width: PICKER_WIDTH,
            height: picker_h,
            offset_x: 0.0,
            offset_y: 4.0,
            blur: 16.0,
            spread: 0.0,
            color: Color::rgba(0, 0, 0, 100),
            corner_radii: CornerRadii::all(10.0),
        });

        // Background.
        cmds.push(RenderCommand::FillRect {
            x: px,
            y: py,
            width: PICKER_WIDTH,
            height: picker_h,
            color: theme::PICKER_BG,
            corner_radii: CornerRadii::all(10.0),
        });

        // Border.
        cmds.push(RenderCommand::StrokeRect {
            x: px,
            y: py,
            width: PICKER_WIDTH,
            height: picker_h,
            color: theme::SURFACE0,
            line_width: 1.0,
            corner_radii: CornerRadii::all(10.0),
        });

        // Title.
        cmds.push(RenderCommand::Text {
            x: px + PICKER_PADDING,
            y: py + PICKER_PADDING,
            text: "Snap Layout".into(),
            color: theme::LAVENDER,
            font_size: 13.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(PICKER_WIDTH - 2.0 * PICKER_PADDING),
            overflow: TextOverflow::Ellipsis,
        });

        // Thumbnails.
        for (i, &preset) in presets.iter().enumerate() {
            let (ix, iy) = self.thumb_origin(i);

            // Hover highlight.
            if self.picker_hover_index == Some(i) {
                cmds.push(RenderCommand::FillRect {
                    x: ix - 2.0,
                    y: iy - 2.0,
                    width: THUMB_SIZE + 4.0,
                    height: THUMB_SIZE + 4.0,
                    color: theme::PICKER_HOVER,
                    corner_radii: CornerRadii::all(6.0),
                });
            }

            // Thumbnail background.
            cmds.push(RenderCommand::FillRect {
                x: ix,
                y: iy,
                width: THUMB_SIZE,
                height: THUMB_SIZE,
                color: theme::SURFACE0,
                corner_radii: CornerRadii::all(4.0),
            });

            // Mini-zone rectangles inside the thumbnail. Built at the
            // thumbnail's own origin rather than at zero and offset by the
            // caller, so the thumbnail and the real overlay go through the
            // same origin handling.
            let mini_layout = preset.build(WorkArea::new(
                ix + 4.0,
                iy + 4.0,
                THUMB_SIZE - 8.0,
                THUMB_SIZE - 8.0,
            ));
            for zone in &mini_layout.zones {
                cmds.push(RenderCommand::FillRect {
                    x: zone.x,
                    y: zone.y,
                    width: zone.width,
                    height: zone.height,
                    color: if self.active_preset == preset {
                        theme::BLUE
                    } else {
                        theme::LAVENDER
                    },
                    corner_radii: CornerRadii::all(2.0),
                });
            }

            // Active indicator.
            if self.active_preset == preset {
                cmds.push(RenderCommand::StrokeRect {
                    x: ix,
                    y: iy,
                    width: THUMB_SIZE,
                    height: THUMB_SIZE,
                    color: theme::BLUE,
                    line_width: 2.0,
                    corner_radii: CornerRadii::all(4.0),
                });
            }
        }

        cmds
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

    /// A 1080p screen with nothing subtracted.
    ///
    /// Most tests here are about proportions — that two halves are equal, that
    /// six zones do not overlap — and those hold at any origin, so they use
    /// this. The tests that are specifically about the *origin* use [`DESK`]
    /// and [`TOP_BAR_DESK`] below, because a work area that starts at (0, 0)
    /// and fills the screen is exactly the case in which the defect this file
    /// was fixed for is invisible.
    const SCREEN: WorkArea = WorkArea::whole_screen(1920.0, 1080.0);

    /// The same screen with a 48 px taskbar along the bottom: origin still at
    /// (0, 0), but 48 px shorter.
    const DESK: WorkArea = WorkArea::new(0.0, 0.0, 1920.0, 1032.0);

    /// The same screen with the taskbar along the *top* — an arrangement in
    /// which the work area's origin is not (0, 0), and therefore one that can
    /// catch a zone builder that ignores it.
    const TOP_BAR_DESK: WorkArea = WorkArea::new(0.0, 48.0, 1920.0, 1032.0);

    /// The same screen with a 64 px dock down the *left* side and a 64 px
    /// sidebar down the right.
    ///
    /// Present because `TOP_BAR_DESK` alone is not enough: it offsets only the
    /// `y` origin, so a test written against it is blind to code that drops
    /// the `x` one. Verified — reintroducing "`detect_edge` has no left bound"
    /// into a suite whose fixtures all had `x = 0` failed *nothing*.
    ///
    /// Inset on *both* sides deliberately. With only the left inset its
    /// `right()` came to 1920, the screen width, so a right bound measured
    /// against the screen was still indistinguishable from a correct one —
    /// which the same reintroduce-the-defect check duly caught.
    const SIDE_BAR_DESK: WorkArea = WorkArea::new(64.0, 0.0, 1792.0, 1080.0);

    /// Every variant of [`SnapEdge`].
    ///
    /// `edge_to_default_snap`'s match is exhaustive, so a new variant cannot
    /// be added without someone deciding what it does — but it *can* be added
    /// without being listed here, which would silently narrow every test that
    /// iterates this. Keep the two in step.
    const ALL_EDGES: [SnapEdge; 8] = [
        SnapEdge::Left,
        SnapEdge::Right,
        SnapEdge::Top,
        SnapEdge::Bottom,
        SnapEdge::TopLeft,
        SnapEdge::TopRight,
        SnapEdge::BottomLeft,
        SnapEdge::BottomRight,
    ];

    use std::cmp::Ordering;

    // --- zone label centring ---

    #[test]
    fn a_zone_label_is_centred_on_its_zone() {
        // These were centred by subtracting 3.5 px per *byte* — half a guessed
        // seven-pixel character — so "Left Half" sat off-centre and a localised
        // zone name sat off the zone entirely.
        for (label, size, weight) in [
            ("Left Half", 13.0, FontWeightHint::Regular),
            ("Top Right Quarter", 13.0, FontWeightHint::Regular),
            ("Maximise", 14.0, FontWeightHint::Bold),
            ("Linke Hälfte", 14.0, FontWeightHint::Bold),
        ] {
            let centre = 640.0;
            let x = guitk::text::center_x(label, centre, size, weight);
            let w = guitk::text::measure(label, size, weight);
            assert!(
                (x + w / 2.0 - centre).abs() < 0.01,
                "{label:?} is not centred: spans {x}..{}",
                x + w
            );
        }
    }

    // ======================================================================
    // SnapZone
    // ======================================================================

    #[test]
    fn zone_contains_interior_point() {
        let z = SnapZone {
            id: 0,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            label: "Test".into(),
        };
        assert!(z.contains(50.0, 40.0));
    }

    #[test]
    fn zone_contains_top_left_corner() {
        let z = SnapZone {
            id: 0,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            label: "Test".into(),
        };
        assert!(z.contains(10.0, 20.0));
    }

    #[test]
    fn zone_excludes_point_outside() {
        let z = SnapZone {
            id: 0,
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
            label: "Test".into(),
        };
        assert!(!z.contains(5.0, 40.0));
        assert!(!z.contains(50.0, 80.0));
        assert!(!z.contains(111.0, 40.0));
    }

    #[test]
    fn zone_excludes_bottom_right_boundary() {
        // The zone uses exclusive right/bottom (< not <=).
        let z = SnapZone {
            id: 0,
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
            label: "Test".into(),
        };
        assert!(!z.contains(100.0, 25.0));
        assert!(!z.contains(50.0, 50.0));
    }

    #[test]
    fn zone_center_calculation() {
        let z = SnapZone {
            id: 0,
            x: 100.0,
            y: 200.0,
            width: 400.0,
            height: 300.0,
            label: String::new(),
        };
        let (cx, cy) = z.center();
        assert!((cx - 300.0).abs() < f32::EPSILON);
        assert!((cy - 350.0).abs() < f32::EPSILON);
    }

    // ======================================================================
    // SnapLayoutPreset -- zone counts
    // ======================================================================

    #[test]
    fn preset_two_halves_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        assert_eq!(layout.zones.len(), 2);
        assert_eq!(layout.name, "Two Halves");
    }

    #[test]
    fn preset_three_columns_produces_three_zones() {
        let layout = SnapLayoutPreset::ThreeColumns.build(SCREEN);
        assert_eq!(layout.zones.len(), 3);
    }

    #[test]
    fn preset_two_thirds_left_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoThirdsLeft.build(SCREEN);
        assert_eq!(layout.zones.len(), 2);
    }

    #[test]
    fn preset_two_thirds_right_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoThirdsRight.build(SCREEN);
        assert_eq!(layout.zones.len(), 2);
    }

    #[test]
    fn preset_four_quadrants_produces_four_zones() {
        let layout = SnapLayoutPreset::FourQuadrants.build(SCREEN);
        assert_eq!(layout.zones.len(), 4);
    }

    #[test]
    fn preset_three_left_two_right_produces_three_zones() {
        let layout = SnapLayoutPreset::ThreeLeftTwoRight.build(SCREEN);
        assert_eq!(layout.zones.len(), 3);
    }

    #[test]
    fn preset_six_grid_produces_six_zones() {
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        assert_eq!(layout.zones.len(), 6);
    }

    #[test]
    fn all_presets_returns_seven() {
        assert_eq!(SnapLayoutPreset::all().len(), 7);
    }

    // ======================================================================
    // Layout geometry correctness
    // ======================================================================

    #[test]
    fn two_halves_covers_full_width() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        let left = &layout.zones[0];
        let right = &layout.zones[1];
        let total = left.width + ZONE_GAP + right.width;
        assert!((total - 1920.0).abs() < 0.1);
    }

    #[test]
    fn two_halves_zones_are_equal_width() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        let diff = (layout.zones[0].width - layout.zones[1].width).abs();
        assert!(diff < 0.1);
    }

    #[test]
    fn three_columns_cover_full_width() {
        let layout = SnapLayoutPreset::ThreeColumns.build(SCREEN);
        let total: f32 = layout.zones.iter().map(|z| z.width).sum::<f32>() + 2.0 * ZONE_GAP;
        assert!((total - 1920.0).abs() < 0.5);
    }

    #[test]
    fn two_thirds_left_ratio_approximately_correct() {
        let layout = SnapLayoutPreset::TwoThirdsLeft.build(SCREEN);
        let left = &layout.zones[0];
        let right = &layout.zones[1];
        // left should be roughly twice the right.
        let ratio = left.width / right.width;
        assert!(ratio > 1.8 && ratio < 2.2, "ratio was {ratio}");
    }

    #[test]
    fn four_quadrants_cover_full_area() {
        let layout = SnapLayoutPreset::FourQuadrants.build(SCREEN);
        // Sum of zone areas + gap areas should approximately equal screen area.
        let zone_area: f32 = layout.zones.iter().map(|z| z.width * z.height).sum();
        let screen_area = 1920.0 * 1080.0;
        // Allow for gap space.
        assert!(zone_area > screen_area * 0.98);
    }

    #[test]
    fn six_grid_zones_have_unique_ids() {
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        let mut ids: Vec<ZoneId> = layout.zones.iter().map(|z| z.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn six_grid_ids_run_row_major_and_match_their_labels() {
        // The id is what `edge_to_default_snap` and the caller's persisted
        // window state refer to, and it is now the label array's index rather
        // than a separately-computed `row * 3 + col`. Pin the correspondence:
        // renumbering these silently sends every remembered window to a
        // different corner.
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        let named: Vec<(ZoneId, &str)> = layout
            .zones
            .iter()
            .map(|z| (z.id, z.label.as_str()))
            .collect();
        assert_eq!(
            named,
            vec![
                (0, "Top-Left"),
                (1, "Top-Center"),
                (2, "Top-Right"),
                (3, "Bottom-Left"),
                (4, "Bottom-Center"),
                (5, "Bottom-Right"),
            ]
        );
        // And the labels describe where the zones actually are.
        let mid_x = SCREEN.x + SCREEN.width / 2.0;
        let mid_y = SCREEN.y + SCREEN.height / 2.0;
        for z in &layout.zones {
            let (cx, cy) = z.center();
            assert_eq!(
                cy < mid_y,
                z.label.starts_with("Top"),
                "{} is labelled {:?} but its centre y is {cy}",
                z.id,
                z.label
            );
            if z.label.ends_with("Left") {
                assert!(cx < mid_x, "{:?} is not on the left", z.label);
            } else if z.label.ends_with("Right") {
                assert!(cx > mid_x, "{:?} is not on the right", z.label);
            }
        }
    }

    #[test]
    fn six_grid_zones_do_not_overlap() {
        let layout = SnapLayoutPreset::SixGrid.build(SCREEN);
        for (i, a) in layout.zones.iter().enumerate() {
            for b in layout.zones.iter().skip(i + 1) {
                let overlap_x = a.x < b.x + b.width && a.x + a.width > b.x;
                let overlap_y = a.y < b.y + b.height && a.y + a.height > b.y;
                assert!(
                    !(overlap_x && overlap_y),
                    "zones {} and {} overlap",
                    a.id,
                    b.id
                );
            }
        }
    }

    // ======================================================================
    // Zones stay inside the work area
    // ======================================================================

    #[test]
    fn every_zone_of_every_preset_stays_inside_the_work_area() {
        // The defect this file was fixed for. `build` anchored every zone at
        // (0, 0) with the *full screen* height, though the module doc has
        // always said the zones cover the work area — so every snapped window
        // ran underneath the taskbar and hid its own bottom edge. Nothing in
        // the suite could see it: the zone tests all assert *relationships*
        // between zones (they tile, they do not overlap, the halves are
        // equal), and those hold just as well over the wrong rectangle.
        //
        // Checked against work areas whose origin is *not* (0, 0), on both
        // axes separately: against an area that begins at the origin, a
        // builder that ignores the origin is indistinguishable from one that
        // honours it, and against a top-bar area alone, one that drops only
        // the `x` offset is equally invisible.
        for area in [TOP_BAR_DESK, SIDE_BAR_DESK, DESK] {
            for &preset in SnapLayoutPreset::all() {
                let layout = preset.build(area);
                assert!(
                    !layout.zones.is_empty(),
                    "{preset:?} produced no zones at all"
                );
                for z in &layout.zones {
                    assert!(
                        z.x >= area.x - 0.01,
                        "{preset:?} zone {} starts left of work area {area:?} ({} < {})",
                        z.id,
                        z.x,
                        area.x
                    );
                    assert!(
                        z.y >= area.y - 0.01,
                        "{preset:?} zone {} starts above work area {area:?} ({} < {}) \
                         — it would sit behind a top taskbar",
                        z.id,
                        z.y,
                        area.y
                    );
                    assert!(
                        z.x + z.width <= area.right() + 0.01,
                        "{preset:?} zone {} runs past the right edge of {area:?} ({} > {})",
                        z.id,
                        z.x + z.width,
                        area.right()
                    );
                    assert!(
                        z.y + z.height <= area.bottom() + 0.01,
                        "{preset:?} zone {} runs past the bottom edge of {area:?} ({} > {}) \
                         — a window snapped here would hide its own bottom edge \
                         behind the taskbar",
                        z.id,
                        z.y + z.height,
                        area.bottom()
                    );
                }
            }
        }
    }

    #[test]
    fn a_work_area_too_small_for_the_gap_gives_it_up_rather_than_going_negative() {
        // Found by wiring this module into the shell, which snaps on work areas
        // the shell's own tests take down to zero width. Every arm of
        // `zones_with_gap` subtracts the gap before dividing, so at three
        // pixels wide `(3 - 6) / 2` is a *negative* half-width and the second
        // zone starts at 4.5 — outside the rectangle it is tiling. The
        // in-bounds test above could not see it because it only ever ran on
        // desktop-sized areas, where the subtraction is free.
        for w in [
            0.0_f32, 1.0, 2.0, 3.0, 7.0, 11.0, 12.0, 17.0, 18.0, 19.0, 40.0,
        ] {
            for h in [0.0_f32, 1.0, 5.0, 6.0, 13.0, 40.0] {
                let area = WorkArea::new(3.0, 4.0, w, h);
                for &preset in SnapLayoutPreset::all() {
                    for z in &preset.build(area).zones {
                        assert!(
                            z.width >= 0.0 && z.height >= 0.0,
                            "{preset:?} zone {} of {area:?} has negative extent {}x{}",
                            z.id,
                            z.width,
                            z.height
                        );
                        assert!(
                            z.x >= area.x - 0.01 && z.y >= area.y - 0.01,
                            "{preset:?} zone {} of {area:?} starts outside it at ({}, {})",
                            z.id,
                            z.x,
                            z.y
                        );
                        assert!(
                            z.x + z.width <= area.right() + 0.01
                                && z.y + z.height <= area.bottom() + 0.01,
                            "{preset:?} zone {} of {area:?} ends outside it at ({}, {})",
                            z.id,
                            z.x + z.width,
                            z.y + z.height
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_work_area_with_room_for_the_gap_still_gets_one() {
        // The other half of the rule above: giving the gap up whenever it is
        // inconvenient would satisfy that test completely, and would silently
        // undo the separation between zones on every real screen.
        let zones = SnapLayoutPreset::TwoEqualHalves.build(DESK).zones;
        let seam = zones[0].x + zones[0].width;
        assert!(
            (zones[1].x - seam - ZONE_GAP).abs() < 0.01,
            "expected a {ZONE_GAP}px gap, zone 0 ends at {seam} and zone 1 starts at {}",
            zones[1].x
        );
    }

    #[test]
    fn a_work_area_shorter_than_the_screen_produces_shorter_zones() {
        // The bottom-taskbar case: the origin is unchanged, so only the
        // *height* can reveal whether the taskbar was subtracted.
        let full = SnapLayoutPreset::TwoEqualHalves.build(SCREEN);
        let desk = SnapLayoutPreset::TwoEqualHalves.build(DESK);
        assert!(
            (full.zones[0].height - desk.zones[0].height - 48.0).abs() < 0.01,
            "full-screen zone is {} tall, work-area zone is {} — expected a 48 px difference",
            full.zones[0].height,
            desk.zones[0].height
        );
    }

    // ======================================================================
    // Edge detection
    // ======================================================================

    #[test]
    fn detect_edge_left() {
        assert_eq!(detect_edge(2.0, 500.0, SCREEN), Some(SnapEdge::Left));
    }

    #[test]
    fn detect_edge_right() {
        assert_eq!(detect_edge(1916.0, 500.0, SCREEN), Some(SnapEdge::Right));
    }

    #[test]
    fn detect_edge_top() {
        assert_eq!(detect_edge(960.0, 3.0, SCREEN), Some(SnapEdge::Top));
    }

    #[test]
    fn detect_edge_bottom() {
        assert_eq!(detect_edge(960.0, 1076.0, SCREEN), Some(SnapEdge::Bottom));
    }

    #[test]
    fn detect_edge_top_left_corner() {
        assert_eq!(detect_edge(2.0, 3.0, SCREEN), Some(SnapEdge::TopLeft));
    }

    #[test]
    fn detect_edge_bottom_right_corner() {
        assert_eq!(
            detect_edge(1916.0, 1076.0, SCREEN),
            Some(SnapEdge::BottomRight)
        );
    }

    #[test]
    fn detect_edge_none_in_centre() {
        assert_eq!(detect_edge(960.0, 540.0, SCREEN), None);
    }

    #[test]
    fn edges_are_measured_from_the_work_area_not_the_screen() {
        // With the taskbar at the top, the work area's top edge is 48 px down
        // the screen. A screen-relative `detect_edge` finds the top edge at
        // y=0..8 — behind the taskbar, where a drag is a taskbar interaction
        // and never reaches the snap code — and finds nothing at all where the
        // desktop actually begins.
        assert_eq!(
            detect_edge(960.0, TOP_BAR_DESK.y + 2.0, TOP_BAR_DESK),
            Some(SnapEdge::Top),
            "the top of the work area should be the top edge"
        );
        assert_eq!(
            detect_edge(960.0, 2.0, TOP_BAR_DESK),
            None,
            "y=2 is inside the taskbar, above the work area — not an edge"
        );
        assert_eq!(
            detect_edge(2.0, TOP_BAR_DESK.y + 2.0, TOP_BAR_DESK),
            Some(SnapEdge::TopLeft)
        );
    }

    #[test]
    fn the_left_edge_sits_right_of_a_left_hand_taskbar() {
        // The `x` half of the property above. It needs its own fixture: every
        // other work area in this module starts at x = 0, where a missing
        // left bound (`cursor_x < area.x + THRESHOLD`, with no `>=`) behaves
        // identically to a correct one. That gap was found by reintroducing
        // the defect and watching the whole suite stay green.
        assert_eq!(
            detect_edge(SIDE_BAR_DESK.x + 2.0, 540.0, SIDE_BAR_DESK),
            Some(SnapEdge::Left),
            "the left of the work area should be the left edge"
        );
        assert_eq!(
            detect_edge(2.0, 540.0, SIDE_BAR_DESK),
            None,
            "x=2 is inside the taskbar, left of the work area — not an edge"
        );
        assert_eq!(
            detect_edge(SIDE_BAR_DESK.right() - 2.0, 540.0, SIDE_BAR_DESK),
            Some(SnapEdge::Right)
        );
    }

    #[test]
    fn the_bottom_edge_sits_above_a_bottom_taskbar() {
        // The mirror of the above, and the case that motivated the change: the
        // screen's bottom 48 px belong to the taskbar, so measuring the bottom
        // edge against the screen put it — and both bottom corners — inside
        // the bar. Three of the eight edges were simultaneously unreachable as
        // snaps and stealing input from the bar.
        assert_eq!(
            detect_edge(960.0, DESK.bottom() - 2.0, DESK),
            Some(SnapEdge::Bottom)
        );
        assert_eq!(
            detect_edge(960.0, 1078.0, DESK),
            None,
            "y=1078 is inside the taskbar, below the work area — not an edge"
        );
    }

    #[test]
    fn the_picker_trigger_follows_the_work_areas_top() {
        let mgr = SnapManager::new(TOP_BAR_DESK);
        assert!(mgr.is_in_picker_trigger(960.0, TOP_BAR_DESK.y + 5.0));
        assert!(
            !mgr.is_in_picker_trigger(960.0, TOP_BAR_DESK.y + 200.0),
            "200 px into the desktop is not the trigger band"
        );
        assert!(
            !mgr.is_in_picker_trigger(960.0, 5.0),
            "y=5 is inside a top taskbar, not the desktop's top edge"
        );
    }

    // ======================================================================
    // SnapManager -- construction & basic state
    // ======================================================================

    fn make_manager() -> SnapManager {
        SnapManager::new(SCREEN)
    }

    #[test]
    fn manager_starts_with_default_layout() {
        let mgr = make_manager();
        assert_eq!(mgr.active_preset(), SnapLayoutPreset::TwoEqualHalves);
        assert_eq!(mgr.layout().zones.len(), 2);
    }

    #[test]
    fn manager_overlay_initially_hidden() {
        let mgr = make_manager();
        assert!(!mgr.is_overlay_visible());
        assert!(!mgr.is_picker_visible());
    }

    #[test]
    fn show_and_hide_overlay() {
        let mut mgr = make_manager();
        mgr.show_overlay();
        assert!(mgr.is_overlay_visible());
        mgr.hide_overlay();
        assert!(!mgr.is_overlay_visible());
    }

    // ======================================================================
    // SnapManager -- set_layout & resize
    // ======================================================================

    #[test]
    fn set_layout_changes_preset() {
        let mut mgr = make_manager();
        mgr.set_layout(SnapLayoutPreset::SixGrid);
        assert_eq!(mgr.active_preset(), SnapLayoutPreset::SixGrid);
        assert_eq!(mgr.layout().zones.len(), 6);
    }

    #[test]
    fn setting_the_work_area_rebuilds_zones() {
        let mut mgr = make_manager();
        mgr.set_layout(SnapLayoutPreset::TwoEqualHalves);
        let old_width = mgr.layout().zones[0].width;

        let bigger = WorkArea::whole_screen(3840.0, 2160.0);
        mgr.set_work_area(bigger);
        let new_width = mgr.layout().zones[0].width;
        assert!(new_width > old_width);
        assert_eq!(mgr.work_area(), bigger);
    }

    #[test]
    fn moving_the_taskbar_moves_the_zones_rather_than_only_resizing_them() {
        // A resize that keeps the zones' *size* right and their *position*
        // wrong is precisely what the old screen-relative builder did, so
        // asserting on width alone (as the test above does, and as the whole
        // module used to) cannot see it. The bar moving from the bottom to the
        // top does not change the work area's size at all — only its origin.
        let mut mgr = SnapManager::new(DESK);
        let before = mgr.layout().zones[0].y;
        mgr.set_work_area(TOP_BAR_DESK);
        let after = mgr.layout().zones[0].y;
        assert!(
            (after - before - 48.0).abs() < 0.01,
            "zone top moved from {before} to {after}; expected a 48 px shift"
        );
    }

    // ======================================================================
    // SnapManager -- hit_test
    // ======================================================================

    #[test]
    fn hit_test_finds_left_zone() {
        let mgr = make_manager();
        let zone = mgr.hit_test(100.0, 540.0);
        assert!(zone.is_some());
        assert_eq!(zone.map(|z| z.id), Some(0));
    }

    #[test]
    fn hit_test_finds_right_zone() {
        let mgr = make_manager();
        let zone = mgr.hit_test(1800.0, 540.0);
        assert!(zone.is_some());
        assert_eq!(zone.map(|z| z.id), Some(1));
    }

    #[test]
    fn hit_test_returns_none_in_gap() {
        let mgr = make_manager();
        // The gap is right at the centre of 1920: (1920 - 6) / 2 = 957
        // so the gap is at x=957..963. Check the centre of the gap.
        let gap_x = (1920.0 - ZONE_GAP) / 2.0 + ZONE_GAP / 2.0;
        let result = mgr.hit_test(gap_x, 540.0);
        assert!(result.is_none(), "expected None in the gap area");
    }

    // ======================================================================
    // SnapManager -- snap_window
    // ======================================================================

    #[test]
    fn snap_window_returns_zone_geometry() {
        let mut mgr = make_manager();
        let result = mgr.snap_window(42, 0);
        assert!(result.is_some());
        let (x, y, w, h) = result.expect("already checked");
        assert!((x - 0.0).abs() < 0.1);
        assert!((y - 0.0).abs() < 0.1);
        assert!(w > 900.0); // roughly half of 1920
        assert!((h - 1080.0).abs() < 0.1);
    }

    #[test]
    fn snap_window_records_history() {
        let mut mgr = make_manager();
        mgr.snap_window(42, 0);
        assert_eq!(mgr.history.snapped_zone(42), Some(0));
    }

    #[test]
    fn snap_window_invalid_zone_returns_none() {
        let mut mgr = make_manager();
        let result = mgr.snap_window(42, 99);
        assert!(result.is_none());
    }

    // ======================================================================
    // SnapManager -- edge_snap_hit
    // ======================================================================

    #[test]
    fn edge_snap_hit_left_edge() {
        let mgr = make_manager();
        let result = mgr.edge_snap_hit(2.0, 540.0);
        assert!(result.is_some());
        let (edge, zone) = result.expect("already checked");
        assert_eq!(edge, SnapEdge::Left);
        assert_eq!(zone.id, 0);
    }

    #[test]
    fn edge_snap_hit_top_right_corner() {
        let mgr = make_manager();
        let result = mgr.edge_snap_hit(1916.0, 3.0);
        assert!(result.is_some());
        let (edge, _zone) = result.expect("already checked");
        assert_eq!(edge, SnapEdge::TopRight);
    }

    #[test]
    fn edge_snap_hit_centre_returns_none() {
        let mgr = make_manager();
        assert!(mgr.edge_snap_hit(960.0, 540.0).is_none());
    }

    // ======================================================================
    // SnapHistory
    // ======================================================================

    #[test]
    fn history_record_and_restore() {
        let mut hist = SnapHistory::default();
        let geom = SavedGeometry {
            x: 100.0,
            y: 200.0,
            width: 800.0,
            height: 600.0,
        };
        hist.record(1, 0, geom);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist.snapped_zone(1), Some(0));

        let restored = hist.restore(1);
        assert_eq!(restored, Some(geom));
        assert!(hist.is_empty());
    }

    #[test]
    fn history_restore_nonexistent_returns_none() {
        let mut hist = SnapHistory::default();
        assert!(hist.restore(999).is_none());
    }

    #[test]
    fn history_remove_clears_entry() {
        let mut hist = SnapHistory::default();
        hist.record(
            1,
            0,
            SavedGeometry {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        hist.remove(1);
        assert!(hist.is_empty());
    }

    #[test]
    fn history_clear_removes_all() {
        let mut hist = SnapHistory::default();
        for i in 0..5 {
            hist.record(
                i,
                0,
                SavedGeometry {
                    x: 0.0,
                    y: 0.0,
                    width: 100.0,
                    height: 100.0,
                },
            );
        }
        assert_eq!(hist.len(), 5);
        hist.clear();
        assert!(hist.is_empty());
    }

    // ======================================================================
    // Rendering -- overlay
    // ======================================================================

    #[test]
    fn render_overlay_empty_when_hidden() {
        let mgr = make_manager();
        assert!(mgr.render_overlay().is_empty());
    }

    #[test]
    fn render_overlay_nonempty_when_visible() {
        let mut mgr = make_manager();
        mgr.show_overlay();
        let cmds = mgr.render_overlay();
        // Scrim + (fill + stroke + text) * 2 zones = 7.
        assert!(cmds.len() >= 7);
    }

    #[test]
    fn render_zone_highlight_returns_commands() {
        let mgr = make_manager();
        let cmds = mgr.render_zone_highlight(0);
        assert_eq!(cmds.len(), 3); // fill + stroke + text
    }

    #[test]
    fn render_zone_highlight_invalid_zone_empty() {
        let mgr = make_manager();
        assert!(mgr.render_zone_highlight(99).is_empty());
    }

    // ======================================================================
    // Rendering -- picker
    // ======================================================================

    #[test]
    fn render_picker_empty_when_hidden() {
        let mgr = make_manager();
        assert!(mgr.render_picker().is_empty());
    }

    #[test]
    fn render_picker_nonempty_when_visible() {
        let mut mgr = make_manager();
        mgr.show_picker();
        let cmds = mgr.render_picker();
        // At least shadow + bg + border + title + 7 thumbnails.
        assert!(cmds.len() >= 11);
    }

    // ======================================================================
    // Picker interaction
    // ======================================================================

    #[test]
    fn picker_trigger_near_top() {
        let mgr = make_manager();
        assert!(mgr.is_in_picker_trigger(960.0, 5.0));
        assert!(!mgr.is_in_picker_trigger(960.0, 200.0));
    }

    #[test]
    fn picker_select_changes_layout() {
        let mut mgr = make_manager();
        mgr.show_picker();
        // Manually set hover to the SixGrid preset (index 6).
        mgr.picker_hover_index = Some(6);
        assert!(mgr.picker_select());
        assert_eq!(mgr.active_preset(), SnapLayoutPreset::SixGrid);
        assert!(!mgr.is_picker_visible());
    }

    #[test]
    fn hovering_each_thumbnail_selects_that_preset() {
        // Hit-testing and drawing used to compute the picker grid from
        // separate copies of the same expression, four hundred lines apart.
        // They now share `thumb_origin`, and this walks every thumbnail's
        // centre to confirm the grid the user points at is the grid they get
        // — including for a picker that is not at the screen origin.
        let mut mgr = SnapManager::new(TOP_BAR_DESK);
        mgr.show_picker();
        for (i, &preset) in SnapLayoutPreset::all().iter().enumerate() {
            let (ix, iy) = mgr.thumb_origin(i);
            mgr.update_picker_hover(ix + THUMB_SIZE / 2.0, iy + THUMB_SIZE / 2.0);
            assert_eq!(
                mgr.picker_hover_index,
                Some(i),
                "the centre of thumbnail {i} ({preset:?}) did not hover it"
            );
        }
    }

    #[test]
    fn the_gap_between_thumbnails_hovers_nothing() {
        // The complement of the test above: a grid whose cells are too large
        // would pass it and still hover the wrong preset near a boundary.
        let mut mgr = SnapManager::new(TOP_BAR_DESK);
        mgr.show_picker();
        let (ix, iy) = mgr.thumb_origin(0);
        mgr.update_picker_hover(ix + THUMB_SIZE + THUMB_GAP / 2.0, iy + THUMB_SIZE / 2.0);
        assert_eq!(mgr.picker_hover_index, None);
    }

    #[test]
    fn picker_select_without_hover_returns_false() {
        let mut mgr = make_manager();
        mgr.show_picker();
        assert!(!mgr.picker_select());
    }

    // ======================================================================
    // Zone-by-id lookup
    // ======================================================================

    #[test]
    fn zone_by_id_found() {
        let mgr = make_manager();
        let z = mgr.zone_by_id(1);
        assert!(z.is_some());
        assert_eq!(z.map(|zz| &zz.label), Some(&"Right".to_string()));
    }

    #[test]
    fn zone_by_id_not_found() {
        let mgr = make_manager();
        assert!(mgr.zone_by_id(99).is_none());
    }

    // ======================================================================
    // Edge-to-zone mapping completeness
    // ======================================================================

    #[test]
    fn all_edges_map_to_zones_that_exist() {
        for edge in ALL_EDGES {
            match edge_to_default_snap(edge) {
                None | Some(EdgeSnap::Maximize) => {}
                Some(EdgeSnap::Zone(preset, zone_id)) => {
                    let layout = preset.build(SCREEN);
                    assert!(
                        layout.zones.iter().any(|z| z.id == zone_id),
                        "edge {edge:?} mapped to nonexistent zone {zone_id} in preset {preset:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_window_dragged_to_an_edge_lands_on_that_side() {
        // The property the old test was missing entirely. It asserted only
        // that each edge mapped to a zone that *exists*, which is true of
        // every mapping including the one that was actually in the file:
        // `Top` sent the window to the left half and `Bottom` sent it to the
        // right half. Both zones exist. Neither is where the user dragged.
        //
        // Each expectation below is a *sign* — which side of the work area's
        // centre the resulting zone's centre must fall on — so it holds for
        // any work area and any zone sizes, and cannot be satisfied by a
        // mapping that points the wrong way.
        let mgr = SnapManager::new(TOP_BAR_DESK);
        let (mid_x, mid_y) = (
            TOP_BAR_DESK.x + TOP_BAR_DESK.width / 2.0,
            TOP_BAR_DESK.y + TOP_BAR_DESK.height / 2.0,
        );

        // (edge, expected horizontal side, expected vertical side); `None`
        // means the zone should straddle the centre on that axis.
        let cases: [(SnapEdge, Option<Ordering>, Option<Ordering>); 6] = [
            (SnapEdge::Left, Some(Ordering::Less), None),
            (SnapEdge::Right, Some(Ordering::Greater), None),
            (
                SnapEdge::TopLeft,
                Some(Ordering::Less),
                Some(Ordering::Less),
            ),
            (
                SnapEdge::TopRight,
                Some(Ordering::Greater),
                Some(Ordering::Less),
            ),
            (
                SnapEdge::BottomLeft,
                Some(Ordering::Less),
                Some(Ordering::Greater),
            ),
            (
                SnapEdge::BottomRight,
                Some(Ordering::Greater),
                Some(Ordering::Greater),
            ),
        ];

        for (edge, want_x, want_y) in cases {
            let snap = edge_to_default_snap(edge)
                .unwrap_or_else(|| panic!("{edge:?} should map to a snap"));
            let EdgeSnap::Zone(preset, zone_id) = snap else {
                panic!("{edge:?} should map to a zone, not {snap:?}");
            };
            let zone = preset
                .build(mgr.work_area())
                .zones
                .into_iter()
                .find(|z| z.id == zone_id)
                .unwrap_or_else(|| panic!("{edge:?} mapped to a missing zone"));
            let (cx, cy) = zone.center();

            if let Some(want) = want_x {
                assert_eq!(
                    cx.partial_cmp(&mid_x),
                    Some(want),
                    "{edge:?}: zone centre x={cx} is on the wrong side of {mid_x}"
                );
            }
            if let Some(want) = want_y {
                assert_eq!(
                    cy.partial_cmp(&mid_y),
                    Some(want),
                    "{edge:?}: zone centre y={cy} is on the wrong side of {mid_y}"
                );
            }
        }
    }

    #[test]
    fn the_top_edge_maximizes_and_the_bottom_edge_does_nothing() {
        // Stated as its own test because these two are the ones that changed
        // meaning, and because "does nothing" is a result no `Ordering` test
        // above can express. A bottom-edge drop is an ordinary window move:
        // there is no half-height bottom strip among the presets to snap to,
        // and inventing one to fill the gap would be a worse answer than
        // leaving the drag alone.
        assert_eq!(
            edge_to_default_snap(SnapEdge::Top),
            Some(EdgeSnap::Maximize)
        );
        assert_eq!(edge_to_default_snap(SnapEdge::Bottom), None);

        let mgr = SnapManager::new(TOP_BAR_DESK);
        let (edge, zone) = mgr
            .edge_snap_hit(960.0, TOP_BAR_DESK.y + 2.0)
            .expect("the top edge of the work area should snap");
        assert_eq!(edge, SnapEdge::Top);
        assert!((zone.x - TOP_BAR_DESK.x).abs() < 0.01);
        assert!((zone.y - TOP_BAR_DESK.y).abs() < 0.01);
        assert!((zone.width - TOP_BAR_DESK.width).abs() < 0.01);
        assert!((zone.height - TOP_BAR_DESK.height).abs() < 0.01);
    }

    // ======================================================================
    // Preset labels are non-empty
    // ======================================================================

    #[test]
    fn all_preset_labels_nonempty() {
        for preset in SnapLayoutPreset::all() {
            assert!(!preset.label().is_empty(), "{preset:?} has empty label");
        }
    }
}
