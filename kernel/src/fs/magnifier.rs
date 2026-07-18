//! Magnifier — accessibility screen magnification.
//!
//! Provides screen magnification for low-vision users, with smooth
//! zoom, cursor tracking, lens/fullscreen/docked modes, and
//! configurable color filters (inversion, high contrast).
//!
//! ## Architecture
//!
//! ```text
//! Keyboard shortcut / Settings → Accessibility
//!   → magnifier::set_enabled(true) / set_zoom(level)
//!
//! Compositor (every frame)
//!   → magnifier::get_viewport() → source rect to magnify
//!
//! Integration:
//!   → a11y (accessibility framework)
//!   → screenreader (complementary assistive tech)
//!   → display (screen dimensions)
//!   → cursorsettings (large cursor option)
//!   → hotkeys (zoom in/out bindings)
//! ```

#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Magnification view mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MagMode {
    /// Full-screen magnification.
    FullScreen,
    /// Floating lens window.
    Lens,
    /// Docked panel (top/bottom).
    Docked,
}

impl MagMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::FullScreen => "Full Screen",
            Self::Lens => "Lens",
            Self::Docked => "Docked",
        }
    }
}

/// Color filter for enhanced visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorFilter {
    None,
    Inverted,
    GrayScale,
    HighContrast,
    /// Yellow-on-black.
    YellowOnBlack,
    /// White-on-black.
    WhiteOnBlack,
}

impl ColorFilter {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Inverted => "Inverted",
            Self::GrayScale => "Grayscale",
            Self::HighContrast => "High Contrast",
            Self::YellowOnBlack => "Yellow on Black",
            Self::WhiteOnBlack => "White on Black",
        }
    }
}

/// Cursor tracking behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackingMode {
    /// Viewport follows cursor smoothly.
    Continuous,
    /// Viewport jumps when cursor hits edge.
    Edge,
    /// Viewport centered on cursor.
    Centered,
}

impl TrackingMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Continuous => "Continuous",
            Self::Edge => "Edge",
            Self::Centered => "Centered",
        }
    }
}

/// Viewport rectangle (the magnified region of the screen).
#[derive(Debug, Clone, Copy)]
pub struct Viewport {
    /// Top-left X coordinate (screen pixels).
    pub x: i32,
    /// Top-left Y coordinate.
    pub y: i32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Magnifier configuration.
#[derive(Debug, Clone)]
pub struct MagConfig {
    pub enabled: bool,
    pub mode: MagMode,
    /// Zoom level in percent (100 = 1x, 200 = 2x, etc.).
    pub zoom_pct: u32,
    pub min_zoom_pct: u32,
    pub max_zoom_pct: u32,
    pub zoom_step_pct: u32,
    pub color_filter: ColorFilter,
    pub tracking: TrackingMode,
    /// Smooth zooming animation.
    pub smooth_zoom: bool,
    /// Show crosshair at center.
    pub show_crosshair: bool,
    /// Lens size (pixels, for Lens mode).
    pub lens_size: u32,
    /// Focus follows text caret.
    pub follow_caret: bool,
    /// Focus follows keyboard focus.
    pub follow_focus: bool,
}

impl Default for MagConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: MagMode::FullScreen,
            zoom_pct: 200,
            min_zoom_pct: 100,
            max_zoom_pct: 1600,
            zoom_step_pct: 25,
            color_filter: ColorFilter::None,
            tracking: TrackingMode::Continuous,
            smooth_zoom: true,
            show_crosshair: false,
            lens_size: 400,
            follow_caret: true,
            follow_focus: true,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: MagConfig,
    viewport: Viewport,
    /// Screen dimensions.
    screen_width: u32,
    screen_height: u32,
    /// Cursor position.
    cursor_x: i32,
    cursor_y: i32,
    total_zoom_changes: u64,
    ops: u64,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut State) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

/// Recalculate viewport based on cursor and zoom.
fn recalculate_viewport(state: &mut State) {
    if state.config.zoom_pct <= 100 {
        state.viewport = Viewport {
            x: 0, y: 0,
            width: state.screen_width,
            height: state.screen_height,
        };
        return;
    }

    // Viewport size = screen / zoom factor.
    let vw = (state.screen_width * 100) / state.config.zoom_pct;
    let vh = (state.screen_height * 100) / state.config.zoom_pct;

    // Center on cursor.
    let mut vx = state.cursor_x - (vw as i32 / 2);
    let mut vy = state.cursor_y - (vh as i32 / 2);

    // Clamp to screen bounds.
    if vx < 0 { vx = 0; }
    if vy < 0 { vy = 0; }
    if vx + vw as i32 > state.screen_width as i32 {
        vx = state.screen_width as i32 - vw as i32;
    }
    if vy + vh as i32 > state.screen_height as i32 {
        vy = state.screen_height as i32 - vh as i32;
    }

    state.viewport = Viewport { x: vx, y: vy, width: vw, height: vh };
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        config: MagConfig::default(),
        viewport: Viewport { x: 0, y: 0, width: 1920, height: 1080 },
        screen_width: 1920,
        screen_height: 1080,
        cursor_x: 960,
        cursor_y: 540,
        total_zoom_changes: 0,
        ops: 0,
    });
}

pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.enabled = enabled;
        if enabled {
            recalculate_viewport(state);
        }
        Ok(())
    })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.config.enabled)
}

/// Set zoom level in percent.
pub fn set_zoom(pct: u32) -> KernelResult<()> {
    with_state(|state| {
        let clamped = pct.max(state.config.min_zoom_pct).min(state.config.max_zoom_pct);
        state.config.zoom_pct = clamped;
        state.total_zoom_changes += 1;
        recalculate_viewport(state);
        Ok(())
    })
}

/// Zoom in by the configured step.
pub fn zoom_in() -> KernelResult<u32> {
    with_state(|state| {
        let new = (state.config.zoom_pct + state.config.zoom_step_pct).min(state.config.max_zoom_pct);
        state.config.zoom_pct = new;
        state.total_zoom_changes += 1;
        recalculate_viewport(state);
        Ok(new)
    })
}

/// Zoom out by the configured step.
pub fn zoom_out() -> KernelResult<u32> {
    with_state(|state| {
        let new = state.config.zoom_pct.saturating_sub(state.config.zoom_step_pct).max(state.config.min_zoom_pct);
        state.config.zoom_pct = new;
        state.total_zoom_changes += 1;
        recalculate_viewport(state);
        Ok(new)
    })
}

/// Get current zoom level.
pub fn zoom_level() -> u32 {
    STATE.lock().as_ref().map_or(100, |s| s.config.zoom_pct)
}

/// Set magnification mode.
pub fn set_mode(mode: MagMode) -> KernelResult<()> {
    with_state(|state| { state.config.mode = mode; Ok(()) })
}

/// Set color filter.
pub fn set_color_filter(filter: ColorFilter) -> KernelResult<()> {
    with_state(|state| { state.config.color_filter = filter; Ok(()) })
}

/// Set tracking mode.
pub fn set_tracking(tracking: TrackingMode) -> KernelResult<()> {
    with_state(|state| { state.config.tracking = tracking; Ok(()) })
}

/// Update cursor position (called by input subsystem).
pub fn update_cursor(x: i32, y: i32) -> KernelResult<()> {
    with_state(|state| {
        state.cursor_x = x;
        state.cursor_y = y;
        if state.config.enabled {
            recalculate_viewport(state);
        }
        Ok(())
    })
}

/// Get current viewport for compositor.
pub fn get_viewport() -> Viewport {
    STATE.lock().as_ref().map_or(
        Viewport { x: 0, y: 0, width: 1920, height: 1080 },
        |s| s.viewport,
    )
}

/// Get configuration.
pub fn get_config() -> KernelResult<MagConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Set screen dimensions.
pub fn set_screen_size(width: u32, height: u32) -> KernelResult<()> {
    with_state(|state| {
        state.screen_width = width;
        state.screen_height = height;
        recalculate_viewport(state);
        Ok(())
    })
}

/// Statistics: (enabled, zoom_pct, mode_label, filter_label, zoom_changes, ops).
pub fn stats() -> (bool, u32, &'static str, &'static str, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.config.enabled, s.config.zoom_pct, s.config.mode.label(),
            s.config.color_filter.label(), s.total_zoom_changes, s.ops,
        ),
        None => (false, 100, "N/A", "N/A", 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("magnifier::self_test() — running tests...");
    init_defaults();

    // 1: Disabled by default.
    assert!(!is_enabled());
    crate::serial_println!("  [1/11] disabled by default: OK");

    // 2: Enable.
    set_enabled(true).expect("enable");
    assert!(is_enabled());
    crate::serial_println!("  [2/11] enable: OK");

    // 3: Default zoom.
    assert_eq!(zoom_level(), 200);
    crate::serial_println!("  [3/11] default zoom: OK");

    // 4: Zoom in.
    let level = zoom_in().expect("zoom in");
    assert_eq!(level, 225);
    crate::serial_println!("  [4/11] zoom in: OK");

    // 5: Zoom out.
    let level = zoom_out().expect("zoom out");
    assert_eq!(level, 200);
    crate::serial_println!("  [5/11] zoom out: OK");

    // 6: Set zoom.
    set_zoom(400).expect("set zoom");
    assert_eq!(zoom_level(), 400);
    crate::serial_println!("  [6/11] set zoom: OK");

    // 7: Zoom clamped to max.
    set_zoom(5000).expect("set zoom max");
    assert_eq!(zoom_level(), 1600);
    crate::serial_println!("  [7/11] zoom clamped: OK");

    // 8: Set mode.
    set_mode(MagMode::Lens).expect("set mode");
    let cfg = get_config().expect("get config");
    assert_eq!(cfg.mode, MagMode::Lens);
    crate::serial_println!("  [8/11] set mode: OK");

    // 9: Color filter.
    set_color_filter(ColorFilter::HighContrast).expect("set filter");
    let cfg = get_config().expect("get config 2");
    assert_eq!(cfg.color_filter, ColorFilter::HighContrast);
    crate::serial_println!("  [9/11] color filter: OK");

    // 10: Viewport.
    update_cursor(500, 300).expect("cursor");
    let vp = get_viewport();
    // At 1600% zoom, viewport should be small.
    assert!(vp.width < 1920);
    assert!(vp.height < 1080);
    crate::serial_println!("  [10/11] viewport: OK");

    // 11: Stats.
    let (enabled, zoom, mode, filter, changes, ops) = stats();
    assert!(enabled);
    assert_eq!(zoom, 1600);
    assert!(changes >= 4);
    assert!(ops > 0);
    let _ = (mode, filter);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("magnifier::self_test() — all 11 tests passed");
}
