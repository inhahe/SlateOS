//! Multi-monitor layout — display arrangement and configuration.
//!
//! Manages the spatial arrangement of multiple monitors, per-monitor
//! resolution/refresh rate/scaling, primary display selection, and
//! mirroring modes.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Display → Arrangement
//!   → monitors::set_position() / set_primary()
//!
//! Compositor integration
//!   → monitors::layout() for desktop geometry
//!   → monitors::monitor_at_point(x, y) for focus
//!
//! Integration:
//!   → display (DPI/brightness per monitor)
//!   → wallpaper (per-monitor wallpaper)
//!   → nightlight (per-monitor color temperature)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_MONITORS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Monitor connection type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectorType {
    Hdmi,
    DisplayPort,
    Vga,
    Dvi,
    UsbC,
    Thunderbolt,
    Internal,
    Virtual,
}

impl ConnectorType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hdmi => "HDMI",
            Self::DisplayPort => "DisplayPort",
            Self::Vga => "VGA",
            Self::Dvi => "DVI",
            Self::UsbC => "USB-C",
            Self::Thunderbolt => "Thunderbolt",
            Self::Internal => "Internal",
            Self::Virtual => "Virtual",
        }
    }
}

/// Monitor rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rotation {
    /// Normal (landscape).
    Normal,
    /// 90 degrees clockwise (portrait).
    Right,
    /// 180 degrees (upside down).
    Inverted,
    /// 270 degrees clockwise (portrait, left).
    Left,
}

impl Rotation {
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Right => "Right (90°)",
            Self::Inverted => "Inverted (180°)",
            Self::Left => "Left (270°)",
        }
    }

    pub fn degrees(self) -> u32 {
        match self {
            Self::Normal => 0,
            Self::Right => 90,
            Self::Inverted => 180,
            Self::Left => 270,
        }
    }
}

/// Display mode (resolution + refresh rate).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
}

/// Monitor layout mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Extend desktop across monitors.
    Extend,
    /// Mirror primary to all monitors.
    Mirror,
    /// Single monitor only.
    Single,
}

impl LayoutMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Extend => "Extend",
            Self::Mirror => "Mirror",
            Self::Single => "Single",
        }
    }
}

/// A configured monitor.
#[derive(Debug, Clone)]
pub struct Monitor {
    /// Unique monitor ID.
    pub id: u32,
    /// Monitor name/model.
    pub name: String,
    /// Connector type.
    pub connector: ConnectorType,
    /// Connector output name (e.g., "HDMI-1").
    pub output: String,
    /// Whether this is the primary display.
    pub primary: bool,
    /// Whether the monitor is enabled.
    pub enabled: bool,
    /// Current resolution.
    pub width: u32,
    pub height: u32,
    /// Refresh rate (Hz).
    pub refresh_hz: u32,
    /// Position on the virtual desktop (pixels).
    pub x: i32,
    pub y: i32,
    /// Scale factor (100 = 1x, 200 = 2x).
    pub scale_pct: u32,
    /// Rotation.
    pub rotation: Rotation,
    /// Available modes.
    pub modes: Vec<DisplayMode>,
    /// Physical size (mm).
    pub width_mm: u32,
    pub height_mm: u32,
    /// EDID manufacturer.
    pub manufacturer: String,
    /// Serial number.
    pub serial: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct MonitorState {
    monitors: Vec<Monitor>,
    layout_mode: LayoutMode,
    next_id: u32,
    ops: u64,
}

static STATE: Mutex<Option<MonitorState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut MonitorState) -> KernelResult<R>,
{
    let mut guard = STATE.lock();
    let state = guard.as_mut().ok_or(KernelError::NotSupported)?;
    let result = f(state)?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);
    Ok(result)
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the multi-monitor subsystem from the real boot framebuffer.
///
/// Rather than fabricating a "Built-in Display" with invented EDID data — a
/// phantom eDP-1 panel, manufacturer "Generic", serial "0000000001", a
/// 344x194 mm physical size, and a five-entry mode list (including a 144 Hz
/// mode) — none of which the kernel actually knows, this reads the one
/// display the kernel genuinely has: the framebuffer Limine handed us, which
/// the text console is actively drawing to. We honestly fill only what the
/// framebuffer reports (width/height and the single active mode) and leave
/// every EDID-derived field empty/zero, because Limine gives us no connector
/// type, refresh rate, manufacturer, serial, or physical dimensions.
///
/// If no framebuffer is available, no monitors are seeded. Real monitors then
/// arrive via add_monitor() from DRM/compositor hotplug enumeration, which
/// parses genuine EDID.
///
/// DEFERRED PROPER FIX: enumerate monitors (and parse real EDID for connector/
/// refresh/manufacturer/serial/physical size/mode list) from the DRM-KMS
/// driver instead of approximating a single framebuffer monitor.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let mut monitors = Vec::new();
    let mut next_id: u32 = 1;
    if let Some((_addr, width, height, _pitch)) = crate::console::framebuffer_info() {
        monitors.push(Monitor {
            id: 1,
            name: String::from("Framebuffer"),
            // Limine does not report the physical connector; "Virtual" honestly
            // marks this as the bootloader framebuffer, not a probed port.
            connector: ConnectorType::Virtual,
            output: String::from("fb0"),
            primary: true,
            enabled: true,
            width,
            height,
            refresh_hz: 0, // Unknown — Limine reports no refresh rate.
            x: 0,
            y: 0,
            scale_pct: 100,
            rotation: Rotation::Normal,
            // The only mode we know is the one the framebuffer is using now.
            modes: alloc::vec![DisplayMode { width, height, refresh_hz: 0 }],
            width_mm: 0, // Unknown — no EDID.
            height_mm: 0,
            manufacturer: String::new(),
            serial: String::new(),
        });
        next_id = 2;
    }

    *guard = Some(MonitorState {
        monitors,
        layout_mode: LayoutMode::Extend,
        next_id,
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Monitor management
// ---------------------------------------------------------------------------

/// Add a monitor (hotplug).
pub fn add_monitor(
    name: &str,
    connector: ConnectorType,
    output: &str,
    width: u32,
    height: u32,
    refresh_hz: u32,
) -> KernelResult<u32> {
    with_state(|state| {
        if state.monitors.len() >= MAX_MONITORS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;

        // Position to the right of existing monitors.
        let max_x = state.monitors.iter()
            .filter(|m| m.enabled)
            .map(|m| m.x.saturating_add(m.width as i32))
            .max()
            .unwrap_or(0);

        state.monitors.push(Monitor {
            id,
            name: String::from(name),
            connector,
            output: String::from(output),
            primary: false,
            enabled: true,
            width,
            height,
            refresh_hz,
            x: max_x,
            y: 0,
            scale_pct: 100,
            rotation: Rotation::Normal,
            modes: alloc::vec![
                DisplayMode { width, height, refresh_hz },
            ],
            width_mm: 0,
            height_mm: 0,
            manufacturer: String::new(),
            serial: String::new(),
        });
        Ok(id)
    })
}

/// Remove a monitor (hotunplug).
pub fn remove_monitor(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.monitors.iter().position(|m| m.id == id) {
            let was_primary = state.monitors[pos].primary;
            state.monitors.remove(pos);
            // If primary was removed, promote first remaining.
            if was_primary {
                if let Some(first) = state.monitors.first_mut() {
                    first.primary = true;
                }
            }
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// Get a monitor by ID.
pub fn get_monitor(id: u32) -> KernelResult<Monitor> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.monitors.iter()
        .find(|m| m.id == id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all monitors.
pub fn list_monitors() -> Vec<Monitor> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.monitors.clone())
}

/// Get the primary monitor.
pub fn primary_monitor() -> KernelResult<Monitor> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    state.monitors.iter()
        .find(|m| m.primary)
        .cloned()
        .ok_or(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Set the primary monitor.
pub fn set_primary(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.monitors.iter().any(|m| m.id == id) {
            return Err(KernelError::NotFound);
        }
        for m in &mut state.monitors {
            m.primary = m.id == id;
        }
        Ok(())
    })
}

/// Enable or disable a monitor.
pub fn set_enabled(id: u32, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.monitors.iter()
            .position(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        // Cannot disable the only enabled monitor.
        if !enabled && state.monitors[pos].primary {
            let enabled_count = state.monitors.iter().filter(|m| m.enabled).count();
            if enabled_count <= 1 {
                return Err(KernelError::InvalidArgument);
            }
        }
        state.monitors[pos].enabled = enabled;
        Ok(())
    })
}

/// Set monitor resolution and refresh rate.
pub fn set_mode(id: u32, width: u32, height: u32, refresh_hz: u32) -> KernelResult<()> {
    with_state(|state| {
        let monitor = state.monitors.iter_mut()
            .find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        monitor.width = width;
        monitor.height = height;
        monitor.refresh_hz = refresh_hz;
        Ok(())
    })
}

/// Set monitor position on virtual desktop.
pub fn set_position(id: u32, x: i32, y: i32) -> KernelResult<()> {
    with_state(|state| {
        let monitor = state.monitors.iter_mut()
            .find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        monitor.x = x;
        monitor.y = y;
        Ok(())
    })
}

/// Set monitor scale factor (100 = 1x, 125 = 1.25x, 200 = 2x).
pub fn set_scale(id: u32, scale_pct: u32) -> KernelResult<()> {
    if scale_pct < 50 || scale_pct > 400 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let monitor = state.monitors.iter_mut()
            .find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        monitor.scale_pct = scale_pct;
        Ok(())
    })
}

/// Set monitor rotation.
pub fn set_rotation(id: u32, rotation: Rotation) -> KernelResult<()> {
    with_state(|state| {
        let monitor = state.monitors.iter_mut()
            .find(|m| m.id == id)
            .ok_or(KernelError::NotFound)?;
        monitor.rotation = rotation;
        Ok(())
    })
}

/// Set layout mode (extend/mirror/single).
pub fn set_layout_mode(mode: LayoutMode) -> KernelResult<()> {
    with_state(|state| {
        state.layout_mode = mode;
        Ok(())
    })
}

/// Get current layout mode.
pub fn layout_mode() -> LayoutMode {
    let guard = STATE.lock();
    guard.as_ref().map_or(LayoutMode::Extend, |s| s.layout_mode)
}

// ---------------------------------------------------------------------------
// Layout queries
// ---------------------------------------------------------------------------

/// Get the total virtual desktop bounding box.
pub fn desktop_bounds() -> (i32, i32, u32, u32) {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return (0, 0, 0, 0),
    };

    let enabled: Vec<_> = state.monitors.iter().filter(|m| m.enabled).collect();
    if enabled.is_empty() {
        return (0, 0, 0, 0);
    }

    let min_x = enabled.iter().map(|m| m.x).min().unwrap_or(0);
    let min_y = enabled.iter().map(|m| m.y).min().unwrap_or(0);
    let max_x = enabled.iter().map(|m| m.x.saturating_add(m.width as i32)).max().unwrap_or(0);
    let max_y = enabled.iter().map(|m| m.y.saturating_add(m.height as i32)).max().unwrap_or(0);

    let w = (max_x - min_x) as u32;
    let h = (max_y - min_y) as u32;
    (min_x, min_y, w, h)
}

/// Find which monitor contains a point.
pub fn monitor_at_point(x: i32, y: i32) -> Option<u32> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;
    for m in &state.monitors {
        if !m.enabled {
            continue;
        }
        if x >= m.x && x < m.x.saturating_add(m.width as i32)
            && y >= m.y && y < m.y.saturating_add(m.height as i32)
        {
            return Some(m.id);
        }
    }
    None
}

/// Auto-arrange monitors left-to-right.
pub fn auto_arrange() -> KernelResult<()> {
    with_state(|state| {
        let mut x = 0i32;
        for m in &mut state.monitors {
            if !m.enabled {
                continue;
            }
            m.x = x;
            m.y = 0;
            x = x.saturating_add(m.width as i32);
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (monitor_count, enabled_count, layout_mode, primary_id, ops).
pub fn stats() -> (usize, usize, &'static str, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let enabled = s.monitors.iter().filter(|m| m.enabled).count();
            let primary_id = s.monitors.iter()
                .find(|m| m.primary)
                .map_or(0, |m| m.id);
            (s.monitors.len(), enabled, s.layout_mode.label(), primary_id, s.ops)
        }
        None => (0, 0, "n/a", 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the multi-monitor module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[monitors] Running self-tests...");

    // Test 1: init_defaults reads the REAL boot framebuffer (no fabricated
    // EDID). Whatever it produces must match console::framebuffer_info()
    // exactly, with every EDID-derived field left empty/zero — never invented.
    *STATE.lock() = None;
    init_defaults();
    {
        let monitors = list_monitors();
        match crate::console::framebuffer_info() {
            Some((_addr, w, h, _pitch)) => {
                assert_eq!(monitors.len(), 1);
                let m = &monitors[0];
                assert_eq!(m.width, w);
                assert_eq!(m.height, h);
                assert_eq!(m.refresh_hz, 0); // unknown, not fabricated
                assert!(m.manufacturer.is_empty()); // no invented EDID
                assert!(m.serial.is_empty());
                assert_eq!(m.width_mm, 0);
                assert_eq!(m.height_mm, 0);
                assert!(m.primary);
            }
            None => {
                assert_eq!(monitors.len(), 0);
            }
        }
    }
    serial_println!("[monitors]  1/11 framebuffer read-through OK");

    // Tests 2–11 exercise the management API against a DETERMINISTIC fixture
    // installed directly, so they don't depend on the real framebuffer
    // geometry: a single 1920x1080 primary monitor at the origin.
    *STATE.lock() = Some(MonitorState {
        monitors: alloc::vec![Monitor {
            id: 1,
            name: String::from("Primary"),
            connector: ConnectorType::Virtual,
            output: String::from("fb0"),
            primary: true,
            enabled: true,
            width: 1920,
            height: 1080,
            refresh_hz: 60,
            x: 0,
            y: 0,
            scale_pct: 100,
            rotation: Rotation::Normal,
            modes: alloc::vec![DisplayMode { width: 1920, height: 1080, refresh_hz: 60 }],
            width_mm: 0,
            height_mm: 0,
            manufacturer: String::new(),
            serial: String::new(),
        }],
        layout_mode: LayoutMode::Extend,
        next_id: 2,
        ops: 0,
    });
    assert_eq!(list_monitors().len(), 1);

    // Test 2: add monitor.
    {
        let id = add_monitor("Dell U2723QE", ConnectorType::Hdmi, "HDMI-1", 3840, 2160, 60).unwrap();
        assert_eq!(list_monitors().len(), 2);
        let m = get_monitor(id).unwrap();
        assert_eq!(m.name, "Dell U2723QE");
        assert_eq!(m.x, 1920); // auto-positioned to the right
    }
    serial_println!("[monitors]  2/11 add monitor OK");

    // Test 3: set primary.
    {
        let monitors = list_monitors();
        let second_id = monitors[1].id;
        set_primary(second_id).unwrap();
        assert!(get_monitor(second_id).unwrap().primary);
        assert!(!get_monitor(monitors[0].id).unwrap().primary);
    }
    serial_println!("[monitors]  3/11 set primary OK");

    // Test 4: set mode.
    {
        let monitors = list_monitors();
        let id = monitors[1].id;
        set_mode(id, 2560, 1440, 144).unwrap();
        let m = get_monitor(id).unwrap();
        assert_eq!(m.width, 2560);
        assert_eq!(m.height, 1440);
        assert_eq!(m.refresh_hz, 144);
    }
    serial_println!("[monitors]  4/11 set mode OK");

    // Test 5: set position.
    {
        let monitors = list_monitors();
        let id = monitors[1].id;
        set_position(id, 1920, -200).unwrap();
        let m = get_monitor(id).unwrap();
        assert_eq!(m.x, 1920);
        assert_eq!(m.y, -200);
    }
    serial_println!("[monitors]  5/11 set position OK");

    // Test 6: scale.
    {
        let monitors = list_monitors();
        let id = monitors[0].id;
        set_scale(id, 150).unwrap();
        assert_eq!(get_monitor(id).unwrap().scale_pct, 150);
        assert!(set_scale(id, 0).is_err());
        assert!(set_scale(id, 500).is_err());
    }
    serial_println!("[monitors]  6/11 scale OK");

    // Test 7: rotation.
    {
        let monitors = list_monitors();
        let id = monitors[0].id;
        set_rotation(id, Rotation::Right).unwrap();
        assert_eq!(get_monitor(id).unwrap().rotation, Rotation::Right);
        set_rotation(id, Rotation::Normal).unwrap();
    }
    serial_println!("[monitors]  7/11 rotation OK");

    // Test 8: layout mode.
    {
        set_layout_mode(LayoutMode::Mirror).unwrap();
        assert_eq!(layout_mode(), LayoutMode::Mirror);
        set_layout_mode(LayoutMode::Extend).unwrap();
    }
    serial_println!("[monitors]  8/11 layout mode OK");

    // Test 9: desktop bounds.
    {
        auto_arrange().unwrap();
        let (x, y, w, _) = desktop_bounds();
        assert_eq!(x, 0);
        assert_eq!(y, 0);
        assert!(w > 1920); // two monitors side by side
    }
    serial_println!("[monitors]  9/11 desktop bounds OK");

    // Test 10: monitor at point.
    {
        let id = monitor_at_point(100, 100);
        assert!(id.is_some());
        let id = monitor_at_point(-1000, -1000);
        assert!(id.is_none());
    }
    serial_println!("[monitors] 10/11 monitor at point OK");

    // Test 11: remove monitor.
    {
        let monitors = list_monitors();
        let second_id = monitors[1].id;
        remove_monitor(second_id).unwrap();
        assert_eq!(list_monitors().len(), 1);
        // Primary should have been promoted.
        assert!(list_monitors().first().unwrap().primary);
    }
    serial_println!("[monitors] 11/11 remove monitor OK");

    // Leave no residue for later callers / the live /proc/monitors view.
    *STATE.lock() = None;

    serial_println!("[monitors] All self-tests passed.");
}
