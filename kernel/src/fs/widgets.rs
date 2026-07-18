//! Desktop widgets — interactive panels on the desktop surface.
//!
//! Manages desktop widgets (small applets pinned to the desktop):
//! clock, weather, system monitor, notes, calendar, etc. Each widget
//! has a defined size, position, and refresh interval.
//!
//! ## Design Reference
//!
//! design.txt line 719: "support widgets? why not?"
//!
//! ## Architecture
//!
//! ```text
//! Desktop compositor
//!   → widgets::active_widgets()
//!   → for each widget:
//!       render at (x, y) with (width, height)
//!       widget.generate_content() → content to display
//!
//! User interaction
//!   → widgets::handle_click(widget_id, x, y)
//!   → Widget-specific action
//! ```
//!
//! ## Widget Types
//!
//! Built-in widgets provide common desktop information:
//! - **Clock**: current time and date
//! - **SystemMonitor**: CPU/RAM/disk usage
//! - **Notes**: sticky notes on desktop
//! - **Calendar**: month view
//! - **DiskUsage**: storage space overview
//! - **RecentFiles**: recently opened files
//!
//! Third-party widgets register via the widget API.

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

/// Maximum active widgets.
const MAX_WIDGETS: usize = 64;

/// Maximum widget types registered.
const MAX_TYPES: usize = 128;

/// Maximum note length.
const MAX_NOTE_LEN: usize = 4096;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Built-in widget type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WidgetKind {
    /// Clock and date display.
    Clock,
    /// CPU/memory/disk usage bars.
    SystemMonitor,
    /// Sticky note.
    Notes,
    /// Month calendar.
    Calendar,
    /// Disk space overview.
    DiskUsage,
    /// Recently opened files.
    RecentFiles,
    /// Weather display.
    Weather,
    /// Network status.
    NetworkStatus,
    /// Custom (third-party).
    Custom,
}

impl WidgetKind {
    /// Display label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Clock => "Clock",
            Self::SystemMonitor => "System Monitor",
            Self::Notes => "Notes",
            Self::Calendar => "Calendar",
            Self::DiskUsage => "Disk Usage",
            Self::RecentFiles => "Recent Files",
            Self::Weather => "Weather",
            Self::NetworkStatus => "Network",
            Self::Custom => "Custom",
        }
    }

    /// Parse from string.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "clock" => Some(Self::Clock),
            "sysmon" | "system-monitor" | "monitor" => Some(Self::SystemMonitor),
            "notes" | "sticky" => Some(Self::Notes),
            "calendar" | "cal" => Some(Self::Calendar),
            "disk" | "disk-usage" => Some(Self::DiskUsage),
            "recent" | "recent-files" => Some(Self::RecentFiles),
            "weather" => Some(Self::Weather),
            "network" | "net" => Some(Self::NetworkStatus),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Default size (width, height) in pixels.
    pub fn default_size(self) -> (u32, u32) {
        match self {
            Self::Clock => (200, 100),
            Self::SystemMonitor => (250, 200),
            Self::Notes => (250, 200),
            Self::Calendar => (250, 250),
            Self::DiskUsage => (200, 150),
            Self::RecentFiles => (250, 200),
            Self::Weather => (200, 150),
            Self::NetworkStatus => (200, 100),
            Self::Custom => (200, 200),
        }
    }

    /// Default refresh interval in milliseconds.
    pub fn default_refresh_ms(self) -> u64 {
        match self {
            Self::Clock => 1000,         // Every second.
            Self::SystemMonitor => 2000, // Every 2 seconds.
            Self::Calendar => 60000,     // Every minute.
            Self::DiskUsage => 30000,    // Every 30 seconds.
            Self::RecentFiles => 10000,  // Every 10 seconds.
            Self::Weather => 600000,     // Every 10 minutes.
            Self::NetworkStatus => 5000, // Every 5 seconds.
            Self::Notes => 0,            // No auto-refresh (user-driven).
            Self::Custom => 5000,
        }
    }

    /// All built-in kinds.
    pub fn all() -> &'static [WidgetKind] {
        &[
            Self::Clock, Self::SystemMonitor, Self::Notes,
            Self::Calendar, Self::DiskUsage, Self::RecentFiles,
            Self::Weather, Self::NetworkStatus,
        ]
    }
}

/// Size policy for a widget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizePolicy {
    /// Fixed pixel size.
    Fixed,
    /// Resizable within min/max bounds.
    Resizable,
}

/// A widget instance placed on the desktop.
#[derive(Debug, Clone)]
pub struct Widget {
    /// Unique widget ID.
    pub id: u64,
    /// Widget kind.
    pub kind: WidgetKind,
    /// Custom type name (for Custom kind).
    pub type_name: String,
    /// Display title.
    pub title: String,
    /// Position X (pixels from left).
    pub x: i32,
    /// Position Y (pixels from top).
    pub y: i32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Size policy.
    pub size_policy: SizePolicy,
    /// Refresh interval (ms, 0 = no auto-refresh).
    pub refresh_ms: u64,
    /// Whether the widget is visible.
    pub visible: bool,
    /// Opacity (0-100, where 100 = fully opaque).
    pub opacity: u8,
    /// Widget-specific data (e.g., note text, settings).
    pub data: String,
    /// Last refresh timestamp (nanoseconds).
    pub last_refresh_ns: u64,
}

/// A registered widget type (for custom widgets).
#[derive(Debug, Clone)]
pub struct WidgetType {
    /// Type identifier.
    pub type_id: String,
    /// Display name.
    pub display_name: String,
    /// Default width.
    pub default_width: u32,
    /// Default height.
    pub default_height: u32,
    /// App ID that provides this widget.
    pub provider_app: String,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct WidgetState {
    /// Widget ID → Widget.
    widgets: BTreeMap<u64, Widget>,
    /// Custom widget types.
    types: BTreeMap<String, WidgetType>,
    /// Next widget ID.
    next_id: u64,
}

impl WidgetState {
    const fn new() -> Self {
        Self {
            widgets: BTreeMap::new(),
            types: BTreeMap::new(),
            next_id: 1,
        }
    }
}

static WIDGETS: Mutex<WidgetState> = Mutex::new(WidgetState::new());
static ADD_COUNT: AtomicU64 = AtomicU64::new(0);
static REFRESH_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Add a widget to the desktop.
pub fn add(kind: WidgetKind, x: i32, y: i32) -> KernelResult<u64> {
    ADD_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WIDGETS.lock();

    if state.widgets.len() >= MAX_WIDGETS {
        return Err(KernelError::ResourceExhausted);
    }

    let id = state.next_id;
    state.next_id = state.next_id.saturating_add(1);
    let (w, h) = kind.default_size();

    state.widgets.insert(id, Widget {
        id,
        kind,
        type_name: String::from(kind.label()),
        title: String::from(kind.label()),
        x,
        y,
        width: w,
        height: h,
        size_policy: SizePolicy::Resizable,
        refresh_ms: kind.default_refresh_ms(),
        visible: true,
        opacity: 100,
        data: String::new(),
        last_refresh_ns: 0,
    });

    Ok(id)
}

/// Add a custom widget.
pub fn add_custom(type_id: &str, x: i32, y: i32) -> KernelResult<u64> {
    ADD_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut state = WIDGETS.lock();

    if state.widgets.len() >= MAX_WIDGETS {
        return Err(KernelError::ResourceExhausted);
    }

    let wtype = state.types.get(type_id).ok_or(KernelError::NotFound)?.clone();
    let id = state.next_id;
    state.next_id = state.next_id.saturating_add(1);

    state.widgets.insert(id, Widget {
        id,
        kind: WidgetKind::Custom,
        type_name: wtype.type_id.clone(),
        title: wtype.display_name.clone(),
        x,
        y,
        width: wtype.default_width,
        height: wtype.default_height,
        size_policy: SizePolicy::Resizable,
        refresh_ms: 5000,
        visible: true,
        opacity: 100,
        data: String::new(),
        last_refresh_ns: 0,
    });

    Ok(id)
}

/// Remove a widget.
pub fn remove(id: u64) -> KernelResult<()> {
    let mut state = WIDGETS.lock();
    state.widgets.remove(&id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Get widget by ID.
pub fn get(id: u64) -> Option<Widget> {
    let state = WIDGETS.lock();
    state.widgets.get(&id).cloned()
}

/// Get all visible widgets (for rendering).
pub fn active_widgets() -> Vec<Widget> {
    let state = WIDGETS.lock();
    state.widgets.values()
        .filter(|w| w.visible)
        .cloned()
        .collect()
}

/// Move a widget to a new position.
pub fn move_widget(id: u64, x: i32, y: i32) -> KernelResult<()> {
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    widget.x = x;
    widget.y = y;
    Ok(())
}

/// Resize a widget.
pub fn resize(id: u64, width: u32, height: u32) -> KernelResult<()> {
    if width == 0 || height == 0 {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    if widget.size_policy == SizePolicy::Fixed {
        return Err(KernelError::PermissionDenied);
    }
    widget.width = width;
    widget.height = height;
    Ok(())
}

/// Set visibility.
pub fn set_visible(id: u64, visible: bool) -> KernelResult<()> {
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    widget.visible = visible;
    Ok(())
}

/// Set opacity (0-100).
pub fn set_opacity(id: u64, opacity: u8) -> KernelResult<()> {
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    widget.opacity = opacity.min(100);
    Ok(())
}

/// Set widget data (e.g., note text).
pub fn set_data(id: u64, data: &str) -> KernelResult<()> {
    if data.len() > MAX_NOTE_LEN {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    widget.data = String::from(data);
    Ok(())
}

/// Set title.
pub fn set_title(id: u64, title: &str) -> KernelResult<()> {
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    widget.title = String::from(title);
    Ok(())
}

/// Mark a widget as refreshed.
pub fn mark_refreshed(id: u64) -> KernelResult<()> {
    REFRESH_COUNT.fetch_add(1, Ordering::Relaxed);
    let now = crate::timekeeping::clock_monotonic();
    let mut state = WIDGETS.lock();
    let widget = state.widgets.get_mut(&id).ok_or(KernelError::NotFound)?;
    widget.last_refresh_ns = now;
    Ok(())
}

/// Get widgets that need refreshing.
pub fn needs_refresh() -> Vec<u64> {
    let now = crate::timekeeping::clock_monotonic();
    let state = WIDGETS.lock();
    state.widgets.values()
        .filter(|w| {
            w.visible && w.refresh_ms > 0 && {
                let interval_ns = w.refresh_ms.saturating_mul(1_000_000);
                now.saturating_sub(w.last_refresh_ns) >= interval_ns
            }
        })
        .map(|w| w.id)
        .collect()
}

// ---------------------------------------------------------------------------
// Custom widget types
// ---------------------------------------------------------------------------

/// Register a custom widget type.
pub fn register_type(type_id: &str, name: &str, width: u32, height: u32,
                     app: &str) -> KernelResult<()> {
    if type_id.is_empty() || name.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = WIDGETS.lock();
    if state.types.len() >= MAX_TYPES && !state.types.contains_key(type_id) {
        return Err(KernelError::ResourceExhausted);
    }
    state.types.insert(String::from(type_id), WidgetType {
        type_id: String::from(type_id),
        display_name: String::from(name),
        default_width: width,
        default_height: height,
        provider_app: String::from(app),
    });
    Ok(())
}

/// Unregister a custom widget type.
pub fn unregister_type(type_id: &str) -> KernelResult<()> {
    let mut state = WIDGETS.lock();
    state.types.remove(type_id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// List registered widget types.
pub fn list_types() -> Vec<WidgetType> {
    let state = WIDGETS.lock();
    state.types.values().cloned().collect()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (widget_count, type_count, add_ops, refresh_ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let state = WIDGETS.lock();
    (
        state.widgets.len(),
        state.types.len(),
        ADD_COUNT.load(Ordering::Relaxed),
        REFRESH_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    ADD_COUNT.store(0, Ordering::Relaxed);
    REFRESH_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut state = WIDGETS.lock();
    state.widgets.clear();
    state.types.clear();
    state.next_id = 1;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the widget system.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: add widget.
    {
        let id = add(WidgetKind::Clock, 100, 50)?;
        assert!(id > 0);
        let w = get(id).unwrap();
        assert_eq!(w.kind, WidgetKind::Clock);
        assert_eq!(w.x, 100);
        assert_eq!(w.width, 200);
        serial_println!("[widgets] test 1 passed: add");
    }

    // Test 2: active widgets.
    {
        let _ = add(WidgetKind::SystemMonitor, 400, 50)?;
        let active = active_widgets();
        assert_eq!(active.len(), 2);
        serial_println!("[widgets] test 2 passed: active_widgets");
    }

    // Test 3: move and resize.
    {
        let id = 1; // First widget.
        move_widget(id, 200, 100)?;
        let w = get(id).unwrap();
        assert_eq!(w.x, 200);
        assert_eq!(w.y, 100);

        resize(id, 300, 150)?;
        let w = get(id).unwrap();
        assert_eq!(w.width, 300);
        serial_println!("[widgets] test 3 passed: move/resize");
    }

    // Test 4: visibility and opacity.
    {
        let id = 1;
        set_visible(id, false)?;
        let active = active_widgets();
        assert_eq!(active.len(), 1); // Only sysmon visible.

        set_visible(id, true)?;
        set_opacity(id, 75)?;
        let w = get(id).unwrap();
        assert_eq!(w.opacity, 75);
        serial_println!("[widgets] test 4 passed: visibility/opacity");
    }

    // Test 5: notes widget with data.
    {
        let id = add(WidgetKind::Notes, 600, 50)?;
        set_data(id, "Remember to buy milk")?;
        set_title(id, "Shopping List")?;
        let w = get(id).unwrap();
        assert_eq!(w.data, "Remember to buy milk");
        assert_eq!(w.title, "Shopping List");
        serial_println!("[widgets] test 5 passed: notes data");
    }

    // Test 6: custom widget type.
    {
        register_type("com.example.todo", "Todo List", 250, 300, "todo.app")?;
        let types = list_types();
        assert_eq!(types.len(), 1);

        let id = add_custom("com.example.todo", 100, 400)?;
        let w = get(id).unwrap();
        assert_eq!(w.kind, WidgetKind::Custom);
        assert_eq!(w.type_name, "com.example.todo");
        serial_println!("[widgets] test 6 passed: custom type");
    }

    // Test 7: remove.
    {
        let count_before = active_widgets().len();
        remove(1)?;
        let count_after = active_widgets().len();
        assert_eq!(count_after, count_before - 1);
        assert!(get(1).is_none());
        serial_println!("[widgets] test 7 passed: remove");
    }

    clear_all();
    reset_stats();

    serial_println!("[widgets] all 7 self-tests passed");
    Ok(())
}
