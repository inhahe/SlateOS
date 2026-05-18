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
//! let mut snap = SnapManager::new(1920.0, 1080.0);
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
use guitk::render::{FontWeightHint, RenderCommand};
use guitk::style::CornerRadii;

use std::collections::HashMap;

// ============================================================================
// Theme -- Catppuccin Mocha palette
// ============================================================================

mod theme {
    use guitk::color::Color;

    pub const BASE: Color = Color::from_hex(0x1E1E2E);
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
const ZONE_GAP: f32 = 6.0;

/// How close (pixels) the cursor must be to a screen edge to trigger
/// edge/corner snap detection.
const EDGE_THRESHOLD: f32 = 8.0;

/// Distance from top of screen to trigger the zone layout picker.
const TOP_PICKER_THRESHOLD: f32 = 16.0;

/// Width of the layout picker popup.
const PICKER_WIDTH: f32 = 340.0;
/// Height of the layout picker popup.
const PICKER_HEIGHT: f32 = 190.0;
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

    /// Build the concrete [`SnapLayout`] for a screen of the given size.
    pub fn build(self, screen_w: f32, screen_h: f32) -> SnapLayout {
        let g = ZONE_GAP;
        let name = self.label().to_string();

        let zones = match self {
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
                let mut zones = Vec::with_capacity(6);
                let labels = [
                    "Top-Left",
                    "Top-Center",
                    "Top-Right",
                    "Bottom-Left",
                    "Bottom-Center",
                    "Bottom-Right",
                ];
                for row in 0..2u32 {
                    for col in 0..3u32 {
                        let idx = row * 3 + col;
                        zones.push(SnapZone {
                            id: idx,
                            x: col as f32 * (col_w + g),
                            y: row as f32 * (row_h + g),
                            width: col_w,
                            height: row_h,
                            label: labels
                                .get(idx as usize)
                                .unwrap_or(&"Zone")
                                .to_string(),
                        });
                    }
                }
                zones
            }
        };

        SnapLayout { name, zones }
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

/// Detect which edge or corner the cursor is near, given the screen dimensions.
/// Returns `None` if the cursor is not near any edge.
pub fn detect_edge(
    cursor_x: f32,
    cursor_y: f32,
    screen_w: f32,
    screen_h: f32,
) -> Option<SnapEdge> {
    let near_left = cursor_x < EDGE_THRESHOLD;
    let near_right = cursor_x >= screen_w - EDGE_THRESHOLD;
    let near_top = cursor_y < EDGE_THRESHOLD;
    let near_bottom = cursor_y >= screen_h - EDGE_THRESHOLD;

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

/// Map a detected edge/corner to a zone id within the `FourQuadrants` layout
/// for corner snaps, or `TwoEqualHalves` for edge snaps.
fn edge_to_default_zone(edge: SnapEdge) -> (SnapLayoutPreset, ZoneId) {
    match edge {
        SnapEdge::Left => (SnapLayoutPreset::TwoEqualHalves, 0),
        SnapEdge::Right => (SnapLayoutPreset::TwoEqualHalves, 1),
        SnapEdge::Top => (SnapLayoutPreset::TwoEqualHalves, 0), // maximize hint
        SnapEdge::Bottom => (SnapLayoutPreset::TwoEqualHalves, 1),
        SnapEdge::TopLeft => (SnapLayoutPreset::FourQuadrants, 0),
        SnapEdge::TopRight => (SnapLayoutPreset::FourQuadrants, 1),
        SnapEdge::BottomLeft => (SnapLayoutPreset::FourQuadrants, 2),
        SnapEdge::BottomRight => (SnapLayoutPreset::FourQuadrants, 3),
    }
}

// ============================================================================
// SnapManager
// ============================================================================

/// Main snap-zone manager. Owns the active layout and overlay state.
pub struct SnapManager {
    screen_width: f32,
    screen_height: f32,
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
    /// Create a new snap manager for the given screen dimensions.
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        let active_preset = SnapLayoutPreset::TwoEqualHalves;
        let layout = active_preset.build(screen_width, screen_height);
        Self {
            screen_width,
            screen_height,
            active_preset,
            layout,
            overlay_visible: false,
            picker_visible: false,
            picker_hover_index: None,
            history: SnapHistory::default(),
        }
    }

    /// Current screen dimensions.
    pub fn screen_size(&self) -> (f32, f32) {
        (self.screen_width, self.screen_height)
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
        self.layout = preset.build(self.screen_width, self.screen_height);
    }

    /// Recalculate zones after a screen resize.
    pub fn resize_screen(&mut self, width: f32, height: f32) {
        self.screen_width = width;
        self.screen_height = height;
        self.layout = self.active_preset.build(width, height);
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
    pub fn edge_snap_hit(
        &self,
        cursor_x: f32,
        cursor_y: f32,
    ) -> Option<(SnapEdge, SnapZone)> {
        let edge = detect_edge(cursor_x, cursor_y, self.screen_width, self.screen_height)?;
        let (preset, zone_id) = edge_to_default_zone(edge);
        let layout = preset.build(self.screen_width, self.screen_height);
        let zone = layout.zones.into_iter().find(|z| z.id == zone_id)?;
        Some((edge, zone))
    }

    /// Detect whether the cursor is in the top-edge region that
    /// triggers the layout picker.
    pub fn is_in_picker_trigger(&self, _cursor_x: f32, cursor_y: f32) -> bool {
        cursor_y < TOP_PICKER_THRESHOLD
    }

    /// Update the picker hover state. `cursor_x` / `cursor_y` are
    /// absolute screen coordinates.
    pub fn update_picker_hover(&mut self, cursor_x: f32, cursor_y: f32) {
        if !self.picker_visible {
            self.picker_hover_index = None;
            return;
        }

        let (px, py) = self.picker_origin();
        let presets = SnapLayoutPreset::all();
        let per_row = self.picker_items_per_row();
        let total = presets.len();

        for i in 0..total {
            let col = i % per_row;
            let row = i / per_row;
            let ix = px + PICKER_PADDING + col as f32 * (THUMB_SIZE + THUMB_GAP);
            let iy = py + PICKER_PADDING + 24.0 + row as f32 * (THUMB_SIZE + THUMB_GAP);

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
        let (_edge, zone) =
            self.edge_snap_hit(cursor_x, cursor_y)?;
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

        let mut cmds = Vec::with_capacity(self.layout.zones.len() * 3 + 1);

        // Scrim behind everything.
        cmds.push(RenderCommand::FillRect {
            x: 0.0,
            y: 0.0,
            width: self.screen_width,
            height: self.screen_height,
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
                x: cx - zone.label.len() as f32 * 3.5,
                y: cy - 7.0,
                text: zone.label.clone(),
                color: theme::TEXT,
                font_size: 13.0,
                font_weight: FontWeightHint::Regular,
                max_width: Some(zone.width - 16.0),
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
            x: cx - zone.label.len() as f32 * 4.0,
            y: cy - 8.0,
            text: zone.label.clone(),
            color: Color::WHITE,
            font_size: 14.0,
            font_weight: FontWeightHint::Bold,
            max_width: Some(zone.width - 16.0),
        });

        cmds
    }

    // ======================================================================
    // Rendering -- layout picker popup
    // ======================================================================

    /// Top-left corner of the picker popup (centred at top of screen).
    fn picker_origin(&self) -> (f32, f32) {
        let px = (self.screen_width - PICKER_WIDTH) / 2.0;
        let py = TOP_PICKER_THRESHOLD + 4.0;
        (px, py)
    }

    /// How many thumbnail items fit in one row.
    fn picker_items_per_row(&self) -> usize {
        let usable = PICKER_WIDTH - 2.0 * PICKER_PADDING + THUMB_GAP;
        ((usable / (THUMB_SIZE + THUMB_GAP)) as usize).max(1)
    }

    /// Render the layout picker popup.
    pub fn render_picker(&self) -> Vec<RenderCommand> {
        if !self.picker_visible {
            return Vec::new();
        }

        let (px, py) = self.picker_origin();
        let presets = SnapLayoutPreset::all();
        let per_row = self.picker_items_per_row();

        let rows = (presets.len() + per_row - 1) / per_row;
        let picker_h = PICKER_PADDING * 2.0
            + 24.0
            + rows as f32 * (THUMB_SIZE + THUMB_GAP)
            - THUMB_GAP;

        let mut cmds = Vec::with_capacity(presets.len() * 8 + 4);

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
        });

        // Thumbnails.
        for (i, &preset) in presets.iter().enumerate() {
            let col = i % per_row;
            let row = i / per_row;
            let ix = px + PICKER_PADDING + col as f32 * (THUMB_SIZE + THUMB_GAP);
            let iy = py + PICKER_PADDING + 24.0 + row as f32 * (THUMB_SIZE + THUMB_GAP);

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

            // Mini-zone rectangles inside the thumbnail.
            let mini_layout = preset.build(THUMB_SIZE - 8.0, THUMB_SIZE - 8.0);
            for zone in &mini_layout.zones {
                cmds.push(RenderCommand::FillRect {
                    x: ix + 4.0 + zone.x,
                    y: iy + 4.0 + zone.y,
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
    use super::*;

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
            label: "".into(),
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
        let layout = SnapLayoutPreset::TwoEqualHalves.build(1920.0, 1080.0);
        assert_eq!(layout.zones.len(), 2);
        assert_eq!(layout.name, "Two Halves");
    }

    #[test]
    fn preset_three_columns_produces_three_zones() {
        let layout = SnapLayoutPreset::ThreeColumns.build(1920.0, 1080.0);
        assert_eq!(layout.zones.len(), 3);
    }

    #[test]
    fn preset_two_thirds_left_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoThirdsLeft.build(1920.0, 1080.0);
        assert_eq!(layout.zones.len(), 2);
    }

    #[test]
    fn preset_two_thirds_right_produces_two_zones() {
        let layout = SnapLayoutPreset::TwoThirdsRight.build(1920.0, 1080.0);
        assert_eq!(layout.zones.len(), 2);
    }

    #[test]
    fn preset_four_quadrants_produces_four_zones() {
        let layout = SnapLayoutPreset::FourQuadrants.build(1920.0, 1080.0);
        assert_eq!(layout.zones.len(), 4);
    }

    #[test]
    fn preset_three_left_two_right_produces_three_zones() {
        let layout = SnapLayoutPreset::ThreeLeftTwoRight.build(1920.0, 1080.0);
        assert_eq!(layout.zones.len(), 3);
    }

    #[test]
    fn preset_six_grid_produces_six_zones() {
        let layout = SnapLayoutPreset::SixGrid.build(1920.0, 1080.0);
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
        let layout = SnapLayoutPreset::TwoEqualHalves.build(1920.0, 1080.0);
        let left = &layout.zones[0];
        let right = &layout.zones[1];
        let total = left.width + ZONE_GAP + right.width;
        assert!((total - 1920.0).abs() < 0.1);
    }

    #[test]
    fn two_halves_zones_are_equal_width() {
        let layout = SnapLayoutPreset::TwoEqualHalves.build(1920.0, 1080.0);
        let diff = (layout.zones[0].width - layout.zones[1].width).abs();
        assert!(diff < 0.1);
    }

    #[test]
    fn three_columns_cover_full_width() {
        let layout = SnapLayoutPreset::ThreeColumns.build(1920.0, 1080.0);
        let total: f32 = layout.zones.iter().map(|z| z.width).sum::<f32>()
            + 2.0 * ZONE_GAP;
        assert!((total - 1920.0).abs() < 0.5);
    }

    #[test]
    fn two_thirds_left_ratio_approximately_correct() {
        let layout = SnapLayoutPreset::TwoThirdsLeft.build(1920.0, 1080.0);
        let left = &layout.zones[0];
        let right = &layout.zones[1];
        // left should be roughly twice the right.
        let ratio = left.width / right.width;
        assert!(ratio > 1.8 && ratio < 2.2, "ratio was {ratio}");
    }

    #[test]
    fn four_quadrants_cover_full_area() {
        let layout = SnapLayoutPreset::FourQuadrants.build(1920.0, 1080.0);
        // Sum of zone areas + gap areas should approximately equal screen area.
        let zone_area: f32 = layout
            .zones
            .iter()
            .map(|z| z.width * z.height)
            .sum();
        let screen_area = 1920.0 * 1080.0;
        // Allow for gap space.
        assert!(zone_area > screen_area * 0.98);
    }

    #[test]
    fn six_grid_zones_have_unique_ids() {
        let layout = SnapLayoutPreset::SixGrid.build(1920.0, 1080.0);
        let mut ids: Vec<ZoneId> = layout.zones.iter().map(|z| z.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn six_grid_zones_do_not_overlap() {
        let layout = SnapLayoutPreset::SixGrid.build(1920.0, 1080.0);
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
    // Edge detection
    // ======================================================================

    #[test]
    fn detect_edge_left() {
        assert_eq!(detect_edge(2.0, 500.0, 1920.0, 1080.0), Some(SnapEdge::Left));
    }

    #[test]
    fn detect_edge_right() {
        assert_eq!(
            detect_edge(1916.0, 500.0, 1920.0, 1080.0),
            Some(SnapEdge::Right)
        );
    }

    #[test]
    fn detect_edge_top() {
        assert_eq!(detect_edge(960.0, 3.0, 1920.0, 1080.0), Some(SnapEdge::Top));
    }

    #[test]
    fn detect_edge_bottom() {
        assert_eq!(
            detect_edge(960.0, 1076.0, 1920.0, 1080.0),
            Some(SnapEdge::Bottom)
        );
    }

    #[test]
    fn detect_edge_top_left_corner() {
        assert_eq!(
            detect_edge(2.0, 3.0, 1920.0, 1080.0),
            Some(SnapEdge::TopLeft)
        );
    }

    #[test]
    fn detect_edge_bottom_right_corner() {
        assert_eq!(
            detect_edge(1916.0, 1076.0, 1920.0, 1080.0),
            Some(SnapEdge::BottomRight)
        );
    }

    #[test]
    fn detect_edge_none_in_centre() {
        assert_eq!(detect_edge(960.0, 540.0, 1920.0, 1080.0), None);
    }

    // ======================================================================
    // SnapManager -- construction & basic state
    // ======================================================================

    fn make_manager() -> SnapManager {
        SnapManager::new(1920.0, 1080.0)
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
    fn resize_screen_rebuilds_zones() {
        let mut mgr = make_manager();
        mgr.set_layout(SnapLayoutPreset::TwoEqualHalves);
        let old_width = mgr.layout().zones[0].width;

        mgr.resize_screen(3840.0, 2160.0);
        let new_width = mgr.layout().zones[0].width;
        assert!(new_width > old_width);
        assert_eq!(mgr.screen_size(), (3840.0, 2160.0));
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
    fn all_edges_map_to_valid_zones() {
        let edges = [
            SnapEdge::Left,
            SnapEdge::Right,
            SnapEdge::Top,
            SnapEdge::Bottom,
            SnapEdge::TopLeft,
            SnapEdge::TopRight,
            SnapEdge::BottomLeft,
            SnapEdge::BottomRight,
        ];
        for edge in &edges {
            let (preset, zone_id) = edge_to_default_zone(*edge);
            let layout = preset.build(1920.0, 1080.0);
            assert!(
                layout.zones.iter().any(|z| z.id == zone_id),
                "edge {edge:?} mapped to nonexistent zone {zone_id} in preset {preset:?}"
            );
        }
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
