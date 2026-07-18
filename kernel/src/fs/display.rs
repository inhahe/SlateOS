//! Display settings — screen resolution, DPI, scaling, and monitors.
//!
//! Manages display configuration for the compositor and settings panel:
//! resolution, refresh rate, DPI scaling, multi-monitor layout.
//!
//! ## Design Reference
//!
//! design.txt line 757: "dual/multiple monitor support?"
//! design.txt line 1253: "screen resolution - automatically revert if
//! user doesn't verify it works in N seconds?"
//! design.txt line 1339: "automatically detect monitor DPI and set
//! default font scaling"
//!
//! ## Architecture
//!
//! ```text
//! Compositor / DRM driver
//!   → display::add_monitor(info)
//!   → display::set_resolution(monitor, mode)
//!
//! Settings panel
//!   → display::list_monitors()
//!   → display::config_for(monitor_id)
//!
//! Confirmation dialog (revert-after-timeout)
//!   → display::pending_change() → Some(RevertInfo)
//!   → display::confirm_change() / display::revert_change()
//! ```

#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum monitors.
const MAX_MONITORS: usize = 16;

/// Maximum display modes per monitor.
const MAX_MODES: usize = 128;

/// Default revert timeout (seconds).
const DEFAULT_REVERT_TIMEOUT: u32 = 15;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A display resolution + refresh rate combination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayMode {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Refresh rate in Hz.
    pub refresh_hz: u32,
    /// Whether this is the native/preferred mode.
    pub preferred: bool,
}

impl DisplayMode {
    /// Human-readable label.
    pub fn label(&self) -> String {
        alloc::format!("{}x{}@{}Hz", self.width, self.height, self.refresh_hz)
    }
}

/// Monitor orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    /// Normal landscape.
    Landscape,
    /// Rotated 90 degrees clockwise (portrait).
    Portrait,
    /// Upside down landscape.
    LandscapeFlipped,
    /// Rotated 90 degrees counter-clockwise.
    PortraitFlipped,
}

impl Orientation {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Landscape => "Landscape",
            Self::Portrait => "Portrait",
            Self::LandscapeFlipped => "Landscape (flipped)",
            Self::PortraitFlipped => "Portrait (flipped)",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "landscape" | "normal" | "0" => Some(Self::Landscape),
            "portrait" | "90" | "right" => Some(Self::Portrait),
            "landscape-flipped" | "180" | "flipped" => Some(Self::LandscapeFlipped),
            "portrait-flipped" | "270" | "left" => Some(Self::PortraitFlipped),
            _ => None,
        }
    }
}

/// A connected monitor.
#[derive(Debug, Clone)]
pub struct Monitor {
    /// Unique monitor identifier (e.g., "HDMI-1", "DP-2").
    pub id: String,
    /// Human-readable name (e.g., "Dell U2723QE").
    pub name: String,
    /// Available display modes.
    pub modes: Vec<DisplayMode>,
    /// Currently active mode index.
    pub active_mode: usize,
    /// Whether the monitor is enabled.
    pub enabled: bool,
    /// Whether this is the primary monitor.
    pub primary: bool,
    /// Position in the virtual desktop (x, y).
    pub pos_x: i32,
    pub pos_y: i32,
    /// Orientation.
    pub orientation: Orientation,
    /// DPI scale factor (100 = 1.0x, 200 = 2.0x).
    pub scale_percent: u32,
    /// Physical width in mm (from EDID).
    pub physical_width_mm: u32,
    /// Physical height in mm (from EDID).
    pub physical_height_mm: u32,
    /// Connected (monitor plugged in).
    pub connected: bool,
}

impl Monitor {
    /// Get the current mode.
    pub fn current_mode(&self) -> Option<&DisplayMode> {
        self.modes.get(self.active_mode)
    }

    /// Compute effective DPI.
    pub fn effective_dpi(&self) -> u32 {
        if let Some(mode) = self.current_mode() {
            if self.physical_width_mm > 0 {
                // DPI = pixels / inches. 1 inch = 25.4mm.
                let dpi = (mode.width as u64)
                    .saturating_mul(254)
                    / (self.physical_width_mm as u64)
                    .saturating_mul(10)
                    .max(1);
                dpi as u32
            } else {
                96 // Default assumption.
            }
        } else {
            96
        }
    }

    /// Recommended scale factor based on DPI.
    pub fn recommended_scale(&self) -> u32 {
        let dpi = self.effective_dpi();
        if dpi >= 192 { 200 }       // 4K on ~24"
        else if dpi >= 144 { 150 }  // QHD on ~24"
        else if dpi >= 120 { 125 }  // FHD on ~15"
        else { 100 }                // Normal
    }
}

/// Pending resolution change (waiting for user confirmation).
#[derive(Debug, Clone)]
pub struct PendingChange {
    /// Monitor being changed.
    pub monitor_id: String,
    /// Previous mode index (to revert to).
    pub previous_mode: usize,
    /// Previous scale.
    pub previous_scale: u32,
    /// When the change was applied (nanoseconds).
    pub applied_at_ns: u64,
    /// Revert timeout in seconds.
    pub timeout_secs: u32,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct DisplayState {
    monitors: BTreeMap<String, Monitor>,
    pending: Option<PendingChange>,
}

impl DisplayState {
    const fn new() -> Self {
        Self {
            monitors: BTreeMap::new(),
            pending: None,
        }
    }
}

static DISPLAY: Mutex<DisplayState> = Mutex::new(DisplayState::new());
static MODE_CHANGE_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Monitor management
// ---------------------------------------------------------------------------

/// Add a monitor to the display system.
pub fn add_monitor(id: &str, name: &str, phys_w_mm: u32, phys_h_mm: u32) -> KernelResult<()> {
    if id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = DISPLAY.lock();
    if state.monitors.len() >= MAX_MONITORS && !state.monitors.contains_key(id) {
        return Err(KernelError::ResourceExhausted);
    }
    let is_first = state.monitors.is_empty();
    state.monitors.insert(String::from(id), Monitor {
        id: String::from(id),
        name: String::from(name),
        modes: Vec::new(),
        active_mode: 0,
        enabled: true,
        primary: is_first,
        pos_x: 0,
        pos_y: 0,
        orientation: Orientation::Landscape,
        scale_percent: 100,
        physical_width_mm: phys_w_mm,
        physical_height_mm: phys_h_mm,
        connected: true,
    });
    Ok(())
}

/// Remove a monitor.
pub fn remove_monitor(id: &str) -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    state.monitors.remove(id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Add a display mode to a monitor.
pub fn add_mode(monitor_id: &str, width: u32, height: u32,
                refresh: u32, preferred: bool) -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    if mon.modes.len() >= MAX_MODES {
        return Err(KernelError::ResourceExhausted);
    }
    mon.modes.push(DisplayMode { width, height, refresh_hz: refresh, preferred });
    // If this is preferred and we haven't selected one, use it.
    if preferred && mon.active_mode == 0 && mon.modes.len() > 1 {
        mon.active_mode = mon.modes.len().saturating_sub(1);
    }
    Ok(())
}

/// Get info about a specific monitor.
pub fn get_monitor(id: &str) -> Option<Monitor> {
    DISPLAY.lock().monitors.get(id).cloned()
}

/// List all monitors.
pub fn list_monitors() -> Vec<Monitor> {
    DISPLAY.lock().monitors.values().cloned().collect()
}

// ---------------------------------------------------------------------------
// Resolution / mode
// ---------------------------------------------------------------------------

/// Set the display mode for a monitor (creates a pending change).
///
/// The change takes effect immediately but will revert after
/// `timeout_secs` unless confirmed via `confirm_change()`.
pub fn set_mode(monitor_id: &str, mode_index: usize) -> KernelResult<()> {
    MODE_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    if mode_index >= mon.modes.len() {
        return Err(KernelError::InvalidArgument);
    }
    let prev = mon.active_mode;
    let prev_scale = mon.scale_percent;
    mon.active_mode = mode_index;

    state.pending = Some(PendingChange {
        monitor_id: String::from(monitor_id),
        previous_mode: prev,
        previous_scale: prev_scale,
        applied_at_ns: now,
        timeout_secs: DEFAULT_REVERT_TIMEOUT,
    });

    Ok(())
}

/// Set resolution by width/height/refresh (finds matching mode).
pub fn set_resolution(monitor_id: &str, width: u32, height: u32,
                      refresh: u32) -> KernelResult<()> {
    let state = DISPLAY.lock();
    let mon = state.monitors.get(monitor_id).ok_or(KernelError::NotFound)?;
    let idx = mon.modes.iter().position(|m|
        m.width == width && m.height == height && m.refresh_hz == refresh);
    drop(state);
    match idx {
        Some(i) => set_mode(monitor_id, i),
        None => Err(KernelError::NotFound),
    }
}

/// Confirm the pending display change (user accepted).
pub fn confirm_change() -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    if state.pending.is_none() {
        return Err(KernelError::NotFound);
    }
    state.pending = None;
    Ok(())
}

/// Revert the pending display change.
pub fn revert_change() -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    let pending = state.pending.take().ok_or(KernelError::NotFound)?;
    if let Some(mon) = state.monitors.get_mut(&pending.monitor_id) {
        mon.active_mode = pending.previous_mode;
        mon.scale_percent = pending.previous_scale;
    }
    Ok(())
}

/// Check if a pending change has timed out and should revert.
pub fn check_pending_timeout() -> bool {
    let now = crate::timekeeping::clock_monotonic();
    let state = DISPLAY.lock();
    if let Some(ref p) = state.pending {
        let elapsed_ns = now.saturating_sub(p.applied_at_ns);
        let timeout_ns = (p.timeout_secs as u64).saturating_mul(1_000_000_000);
        elapsed_ns >= timeout_ns
    } else {
        false
    }
}

/// Get pending change info.
pub fn pending_change() -> Option<PendingChange> {
    DISPLAY.lock().pending.clone()
}

// ---------------------------------------------------------------------------
// Scale / DPI
// ---------------------------------------------------------------------------

/// Set DPI scale factor (as percentage: 100 = 1x, 150 = 1.5x, 200 = 2x).
pub fn set_scale(monitor_id: &str, percent: u32) -> KernelResult<()> {
    if percent < 50 || percent > 400 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    mon.scale_percent = percent;
    Ok(())
}

/// Auto-detect and set recommended scale for a monitor.
pub fn auto_scale(monitor_id: &str) -> KernelResult<u32> {
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    let rec = mon.recommended_scale();
    mon.scale_percent = rec;
    Ok(rec)
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Set monitor position in the virtual desktop.
pub fn set_position(monitor_id: &str, x: i32, y: i32) -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    mon.pos_x = x;
    mon.pos_y = y;
    Ok(())
}

/// Set orientation.
pub fn set_orientation(monitor_id: &str, orient: Orientation) -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    mon.orientation = orient;
    Ok(())
}

/// Set primary monitor.
pub fn set_primary(monitor_id: &str) -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    if !state.monitors.contains_key(monitor_id) {
        return Err(KernelError::NotFound);
    }
    // Clear old primary.
    for mon in state.monitors.values_mut() {
        mon.primary = false;
    }
    if let Some(mon) = state.monitors.get_mut(monitor_id) {
        mon.primary = true;
    }
    Ok(())
}

/// Enable or disable a monitor.
pub fn set_enabled(monitor_id: &str, enabled: bool) -> KernelResult<()> {
    let mut state = DISPLAY.lock();
    let mon = state.monitors.get_mut(monitor_id).ok_or(KernelError::NotFound)?;
    mon.enabled = enabled;
    Ok(())
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (monitor_count, mode_change_count).
pub fn stats() -> (usize, u64) {
    let state = DISPLAY.lock();
    (state.monitors.len(), MODE_CHANGE_COUNT.load(Ordering::Relaxed))
}

/// Reset counters.
pub fn reset_stats() {
    MODE_CHANGE_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = DISPLAY.lock();
    state.monitors.clear();
    state.pending = None;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the display system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: Add monitors and modes.
    serial_println!("  display::test 1: monitors and modes");
    add_monitor("HDMI-1", "Dell U2723QE", 597, 336)?;
    add_mode("HDMI-1", 3840, 2160, 60, true)?;
    add_mode("HDMI-1", 2560, 1440, 60, false)?;
    add_mode("HDMI-1", 1920, 1080, 60, false)?;
    let mon = get_monitor("HDMI-1");
    assert!(mon.is_some());
    let m = mon.unwrap();
    assert_eq!(m.modes.len(), 3);
    assert!(m.primary);

    // Test 2: Set resolution with revert timeout.
    serial_println!("  display::test 2: resolution change with timeout");
    set_mode("HDMI-1", 2)?; // Switch to 1080p.
    let mon2 = get_monitor("HDMI-1").unwrap();
    assert_eq!(mon2.active_mode, 2);
    assert!(pending_change().is_some());
    confirm_change()?;
    assert!(pending_change().is_none());

    // Test 3: Revert change.
    serial_println!("  display::test 3: revert");
    set_mode("HDMI-1", 1)?; // Switch to 1440p.
    revert_change()?;
    let mon3 = get_monitor("HDMI-1").unwrap();
    assert_eq!(mon3.active_mode, 2); // Reverted to 1080p.

    // Test 4: Scale / DPI.
    serial_println!("  display::test 4: scale and DPI");
    set_mode("HDMI-1", 0)?; // Back to 4K.
    confirm_change()?;
    let dpi = get_monitor("HDMI-1").unwrap().effective_dpi();
    assert!(dpi > 100); // 4K on ~24" should be > 100 DPI.
    let rec = auto_scale("HDMI-1")?;
    assert!(rec >= 100);
    set_scale("HDMI-1", 150)?;
    assert_eq!(get_monitor("HDMI-1").unwrap().scale_percent, 150);

    // Test 5: Multi-monitor layout.
    serial_println!("  display::test 5: multi-monitor");
    add_monitor("DP-1", "LG 27GP950", 597, 336)?;
    add_mode("DP-1", 3840, 2160, 144, true)?;
    set_position("DP-1", 3840, 0)?; // Right of first.
    set_primary("DP-1")?;
    let dp = get_monitor("DP-1").unwrap();
    assert!(dp.primary);
    let hdmi = get_monitor("HDMI-1").unwrap();
    assert!(!hdmi.primary);
    assert_eq!(list_monitors().len(), 2);

    // Test 6: Orientation and enable/disable.
    serial_println!("  display::test 6: orientation and enable");
    set_orientation("DP-1", Orientation::Portrait)?;
    assert_eq!(get_monitor("DP-1").unwrap().orientation, Orientation::Portrait);
    set_enabled("DP-1", false)?;
    assert!(!get_monitor("DP-1").unwrap().enabled);
    set_enabled("DP-1", true)?;

    // Test 7: Resolution by dimensions.
    serial_println!("  display::test 7: set by resolution");
    set_resolution("HDMI-1", 2560, 1440, 60)?;
    confirm_change()?;
    let mon7 = get_monitor("HDMI-1").unwrap();
    let mode = mon7.current_mode().unwrap();
    assert_eq!(mode.width, 2560);
    assert_eq!(mode.height, 1440);

    // Cleanup.
    clear_all();
    reset_stats();

    serial_println!("  display: all tests passed");
    Ok(())
}
