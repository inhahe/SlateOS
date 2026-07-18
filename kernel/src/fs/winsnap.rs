//! Window snapping and tiling — snap zones and window organization.
//!
//! Handles window snapping to screen edges and zones, tiling layouts,
//! and drag-to-snap behavior for the window manager.
//!
//! ## Design Reference
//!
//! Implied by design.txt line 743-752 (drop zones) and general desktop
//! window management expectations (Win+Arrow snap behavior).
//!
//! ## Architecture
//!
//! ```text
//! Window manager / compositor
//!   → winsnap::snap(window_id, zone)
//!   → winsnap::get_zone_at(x, y) → SnapZone
//!
//! Hotkey handler (Win+Left/Right/Up)
//!   → winsnap::snap_left(window_id)
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum snap zones per monitor.
const MAX_ZONES: usize = 32;

/// Maximum custom layouts.
const MAX_LAYOUTS: usize = 32;

/// Maximum tracked windows.
const MAX_WINDOWS: usize = 512;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Pre-defined snap positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapPosition {
    /// Full screen (maximize).
    Maximize,
    /// Left half.
    Left,
    /// Right half.
    Right,
    /// Top half.
    Top,
    /// Bottom half.
    Bottom,
    /// Top-left quarter.
    TopLeft,
    /// Top-right quarter.
    TopRight,
    /// Bottom-left quarter.
    BottomLeft,
    /// Bottom-right quarter.
    BottomRight,
    /// Left third.
    LeftThird,
    /// Center third.
    CenterThird,
    /// Right third.
    RightThird,
    /// Restore to pre-snap position.
    Restore,
}

impl SnapPosition {
    pub fn label(self) -> &'static str {
        match self {
            Self::Maximize => "maximize",
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
            Self::LeftThird => "left-third",
            Self::CenterThird => "center-third",
            Self::RightThird => "right-third",
            Self::Restore => "restore",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "max" | "maximize" => Some(Self::Maximize),
            "left" | "l" => Some(Self::Left),
            "right" | "r" => Some(Self::Right),
            "top" | "t" => Some(Self::Top),
            "bottom" | "b" => Some(Self::Bottom),
            "tl" | "top-left" | "topleft" => Some(Self::TopLeft),
            "tr" | "top-right" | "topright" => Some(Self::TopRight),
            "bl" | "bottom-left" | "bottomleft" => Some(Self::BottomLeft),
            "br" | "bottom-right" | "bottomright" => Some(Self::BottomRight),
            "l3" | "left-third" => Some(Self::LeftThird),
            "c3" | "center-third" => Some(Self::CenterThird),
            "r3" | "right-third" => Some(Self::RightThird),
            "restore" | "unsnap" => Some(Self::Restore),
            _ => None,
        }
    }

    /// Compute the target rectangle for this snap position within a screen.
    pub fn rect(self, screen_w: u32, screen_h: u32) -> (i32, i32, u32, u32) {
        match self {
            Self::Maximize => (0, 0, screen_w, screen_h),
            Self::Left => (0, 0, screen_w / 2, screen_h),
            Self::Right => ((screen_w / 2) as i32, 0, screen_w / 2, screen_h),
            Self::Top => (0, 0, screen_w, screen_h / 2),
            Self::Bottom => (0, (screen_h / 2) as i32, screen_w, screen_h / 2),
            Self::TopLeft => (0, 0, screen_w / 2, screen_h / 2),
            Self::TopRight => ((screen_w / 2) as i32, 0, screen_w / 2, screen_h / 2),
            Self::BottomLeft => (0, (screen_h / 2) as i32, screen_w / 2, screen_h / 2),
            Self::BottomRight => ((screen_w / 2) as i32, (screen_h / 2) as i32, screen_w / 2, screen_h / 2),
            Self::LeftThird => (0, 0, screen_w / 3, screen_h),
            Self::CenterThird => ((screen_w / 3) as i32, 0, screen_w / 3, screen_h),
            Self::RightThird => ((screen_w * 2 / 3) as i32, 0, screen_w / 3, screen_h),
            Self::Restore => (0, 0, 0, 0), // Placeholder.
        }
    }
}

/// A custom snap zone.
#[derive(Debug, Clone)]
pub struct SnapZone {
    /// Zone name.
    pub name: String,
    /// Target rectangle (x, y, w, h) as fractions of screen (0-1000 = 0.0-1.0).
    pub x_pct: u32,
    pub y_pct: u32,
    pub w_pct: u32,
    pub h_pct: u32,
}

/// A tiling layout (collection of zones).
#[derive(Debug, Clone)]
pub struct TileLayout {
    pub name: String,
    pub description: String,
    pub zones: Vec<SnapZone>,
}

/// Pre-snap position of a window (for restore).
#[derive(Debug, Clone)]
pub struct WindowState {
    pub window_id: u64,
    /// Current snap position (None = unsnapped).
    pub snapped: Option<SnapPosition>,
    /// Position before snap (x, y, w, h).
    pub pre_snap_x: i32,
    pub pre_snap_y: i32,
    pub pre_snap_w: u32,
    pub pre_snap_h: u32,
}

/// Configuration.
#[derive(Debug, Clone)]
pub struct SnapConfig {
    /// Whether snapping is enabled.
    pub enabled: bool,
    /// Snap trigger distance from edge (pixels).
    pub edge_distance: u32,
    /// Whether to show snap preview overlay.
    pub show_preview: bool,
    /// Animation speed (ms, 0 = instant).
    pub animation_ms: u32,
    /// Whether corner snapping (quarter) is enabled.
    pub corner_snap: bool,
    /// Whether thirds layout is enabled.
    pub thirds: bool,
}

impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            edge_distance: 20,
            show_preview: true,
            animation_ms: 150,
            corner_snap: true,
            thirds: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    config: SnapConfig,
    windows: BTreeMap<u64, WindowState>,
    layouts: Vec<TileLayout>,
    /// Screen dimensions (for snap calculations).
    screen_w: u32,
    screen_h: u32,
}

impl State {
    const fn new() -> Self {
        Self {
            config: SnapConfig {
                enabled: true,
                edge_distance: 20,
                show_preview: true,
                animation_ms: 150,
                corner_snap: true,
                thirds: false,
            },
            windows: BTreeMap::new(),
            layouts: Vec::new(),
            screen_w: 1920,
            screen_h: 1080,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static SNAP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Snap operations
// ---------------------------------------------------------------------------

/// Snap a window to a position.
///
/// Saves the window's current bounds for restoration.
pub fn snap(window_id: u64, pos: SnapPosition, cur_x: i32, cur_y: i32, cur_w: u32, cur_h: u32) -> (i32, i32, u32, u32) {
    let mut state = STATE.lock();
    let (sw, sh) = (state.screen_w, state.screen_h);

    if pos == SnapPosition::Restore {
        // Restore to pre-snap position.
        if let Some(ws) = state.windows.get(&window_id) {
            let result = (ws.pre_snap_x, ws.pre_snap_y, ws.pre_snap_w, ws.pre_snap_h);
            state.windows.remove(&window_id);
            return result;
        }
        return (cur_x, cur_y, cur_w, cur_h);
    }

    // Save current position for restore.
    let ws = state.windows.entry(window_id).or_insert(WindowState {
        window_id,
        snapped: None,
        pre_snap_x: cur_x,
        pre_snap_y: cur_y,
        pre_snap_w: cur_w,
        pre_snap_h: cur_h,
    });

    // Only save pre-snap if not already snapped.
    if ws.snapped.is_none() {
        ws.pre_snap_x = cur_x;
        ws.pre_snap_y = cur_y;
        ws.pre_snap_w = cur_w;
        ws.pre_snap_h = cur_h;
    }
    ws.snapped = Some(pos);

    SNAP_COUNT.fetch_add(1, Ordering::Relaxed);
    pos.rect(sw, sh)
}

/// Unsnap a window (restore to pre-snap position).
pub fn unsnap(window_id: u64) -> Option<(i32, i32, u32, u32)> {
    let mut state = STATE.lock();
    state.windows.remove(&window_id).map(|ws| (ws.pre_snap_x, ws.pre_snap_y, ws.pre_snap_w, ws.pre_snap_h))
}

/// Get the current snap state of a window.
pub fn window_snap_state(window_id: u64) -> Option<SnapPosition> {
    STATE.lock().windows.get(&window_id).and_then(|ws| ws.snapped)
}

/// Detect which snap zone the cursor is in (for drag preview).
pub fn detect_zone(cursor_x: i32, cursor_y: i32) -> Option<SnapPosition> {
    let state = STATE.lock();
    if !state.config.enabled {
        return None;
    }
    let ed = state.config.edge_distance as i32;
    let sw = state.screen_w as i32;
    let sh = state.screen_h as i32;

    let at_left = cursor_x < ed;
    let at_right = cursor_x >= sw - ed;
    let at_top = cursor_y < ed;
    let at_bottom = cursor_y >= sh - ed;

    // Corner detection.
    if state.config.corner_snap {
        if at_left && at_top { return Some(SnapPosition::TopLeft); }
        if at_right && at_top { return Some(SnapPosition::TopRight); }
        if at_left && at_bottom { return Some(SnapPosition::BottomLeft); }
        if at_right && at_bottom { return Some(SnapPosition::BottomRight); }
    }

    // Edge detection.
    if at_left { return Some(SnapPosition::Left); }
    if at_right { return Some(SnapPosition::Right); }
    if at_top { return Some(SnapPosition::Maximize); }

    None
}

/// Remove tracking for a window (when closed).
pub fn remove_window(window_id: u64) {
    STATE.lock().windows.remove(&window_id);
}

// ---------------------------------------------------------------------------
// Tiling layouts
// ---------------------------------------------------------------------------

/// Add a custom tiling layout.
pub fn add_layout(name: &str, desc: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.layouts.len() >= MAX_LAYOUTS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.layouts.iter().any(|l| l.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    state.layouts.push(TileLayout {
        name: String::from(name),
        description: String::from(desc),
        zones: Vec::new(),
    });
    Ok(())
}

/// Add a zone to a layout (coordinates as 0-1000 representing 0%-100%).
pub fn add_zone(layout: &str, zone_name: &str, x_pct: u32, y_pct: u32, w_pct: u32, h_pct: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let lay = state.layouts.iter_mut().find(|l| l.name == layout)
        .ok_or(KernelError::NotFound)?;
    if lay.zones.len() >= MAX_ZONES {
        return Err(KernelError::ResourceExhausted);
    }
    lay.zones.push(SnapZone {
        name: String::from(zone_name),
        x_pct: x_pct.min(1000),
        y_pct: y_pct.min(1000),
        w_pct: w_pct.min(1000),
        h_pct: h_pct.min(1000),
    });
    Ok(())
}

/// Remove a layout.
pub fn remove_layout(name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.layouts.len();
    state.layouts.retain(|l| l.name != name);
    if state.layouts.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

/// List layouts.
pub fn list_layouts() -> Vec<TileLayout> {
    STATE.lock().layouts.clone()
}

/// Initialize default layouts.
pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.layouts.is_empty() { return; }

    // Two-column layout.
    state.layouts.push(TileLayout {
        name: String::from("2-col"),
        description: String::from("Two equal columns"),
        zones: alloc::vec![
            SnapZone { name: String::from("left"), x_pct: 0, y_pct: 0, w_pct: 500, h_pct: 1000 },
            SnapZone { name: String::from("right"), x_pct: 500, y_pct: 0, w_pct: 500, h_pct: 1000 },
        ],
    });

    // Three-column layout.
    state.layouts.push(TileLayout {
        name: String::from("3-col"),
        description: String::from("Three equal columns"),
        zones: alloc::vec![
            SnapZone { name: String::from("left"), x_pct: 0, y_pct: 0, w_pct: 333, h_pct: 1000 },
            SnapZone { name: String::from("center"), x_pct: 333, y_pct: 0, w_pct: 334, h_pct: 1000 },
            SnapZone { name: String::from("right"), x_pct: 667, y_pct: 0, w_pct: 333, h_pct: 1000 },
        ],
    });

    // Main + sidebar.
    state.layouts.push(TileLayout {
        name: String::from("main-side"),
        description: String::from("Large main + sidebar"),
        zones: alloc::vec![
            SnapZone { name: String::from("main"), x_pct: 0, y_pct: 0, w_pct: 700, h_pct: 1000 },
            SnapZone { name: String::from("side"), x_pct: 700, y_pct: 0, w_pct: 300, h_pct: 1000 },
        ],
    });
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

pub fn config() -> SnapConfig { STATE.lock().config.clone() }
pub fn set_enabled(v: bool) { STATE.lock().config.enabled = v; }
pub fn set_edge_distance(px: u32) { STATE.lock().config.edge_distance = px.clamp(5, 100); }
pub fn set_show_preview(v: bool) { STATE.lock().config.show_preview = v; }
pub fn set_animation_ms(ms: u32) { STATE.lock().config.animation_ms = ms; }
pub fn set_corner_snap(v: bool) { STATE.lock().config.corner_snap = v; }
pub fn set_thirds(v: bool) { STATE.lock().config.thirds = v; }
pub fn set_screen(w: u32, h: u32) { let mut s = STATE.lock(); s.screen_w = w; s.screen_h = h; }

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

/// Returns (snapped_count, layout_count, snap_ops).
pub fn stats() -> (usize, usize, u64) {
    let state = STATE.lock();
    (state.windows.len(), state.layouts.len(), SNAP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() { SNAP_COUNT.store(0, Ordering::Relaxed); }

pub fn clear_all() {
    let mut state = STATE.lock();
    state.windows.clear();
    state.layouts.clear();
    state.config = SnapConfig::default();
    state.screen_w = 1920;
    state.screen_h = 1080;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Basic snap.
    serial_println!("  winsnap::self_test 1: basic snap");
    set_screen(1920, 1080);
    let (x, y, w, h) = snap(1, SnapPosition::Left, 100, 100, 800, 600);
    assert_eq!(x, 0);
    assert_eq!(y, 0);
    assert_eq!(w, 960);
    assert_eq!(h, 1080);

    // Test 2: Quarter snap.
    serial_println!("  winsnap::self_test 2: quarter snap");
    let (x2, y2, w2, h2) = snap(2, SnapPosition::TopRight, 200, 200, 640, 480);
    assert_eq!(x2, 960);
    assert_eq!(y2, 0);
    assert_eq!(w2, 960);
    assert_eq!(h2, 540);

    // Test 3: Restore.
    serial_println!("  winsnap::self_test 3: restore");
    let (rx, ry, rw, rh) = snap(1, SnapPosition::Restore, 0, 0, 960, 1080);
    assert_eq!(rx, 100);
    assert_eq!(ry, 100);
    assert_eq!(rw, 800);
    assert_eq!(rh, 600);

    // Test 4: Detect zone.
    serial_println!("  winsnap::self_test 4: zone detection");
    let z1 = detect_zone(5, 500); // Near left edge.
    assert_eq!(z1, Some(SnapPosition::Left));
    let z2 = detect_zone(1915, 5); // Near top-right corner.
    assert_eq!(z2, Some(SnapPosition::TopRight));
    let z3 = detect_zone(960, 540); // Center — no snap.
    assert_eq!(z3, None);

    // Test 5: Tiling layouts.
    serial_println!("  winsnap::self_test 5: layouts");
    init_defaults();
    let layouts = list_layouts();
    assert!(layouts.len() >= 3);
    assert!(layouts.iter().any(|l| l.name == "2-col"));

    // Test 6: Custom layout.
    serial_println!("  winsnap::self_test 6: custom layout");
    add_layout("custom", "My layout")?;
    add_zone("custom", "main", 0, 0, 600, 1000)?;
    add_zone("custom", "top-right", 600, 0, 400, 500)?;
    add_zone("custom", "bottom-right", 600, 500, 400, 500)?;
    let custom = list_layouts().iter().find(|l| l.name == "custom").cloned();
    assert!(custom.is_some());
    assert_eq!(custom.unwrap().zones.len(), 3);
    remove_layout("custom")?;

    // Test 7: Config.
    serial_println!("  winsnap::self_test 7: config");
    set_enabled(false);
    assert!(detect_zone(5, 500).is_none()); // Disabled.
    set_enabled(true);
    set_edge_distance(30);
    set_corner_snap(false);
    let cfg = config();
    assert_eq!(cfg.edge_distance, 30);
    assert!(!cfg.corner_snap);

    let (sc, _lc, ops) = stats();
    assert!(sc > 0 || ops > 0);

    clear_all();
    reset_stats();
    serial_println!("  winsnap: all tests passed");
    Ok(())
}
