//! Taskbar — application launcher bar and window list management.
//!
//! Manages the taskbar: pinned application shortcuts on the left,
//! running application entries on the right (separated by a divider),
//! with drag-and-drop reordering, badge/progress overlay, and
//! grouping of multiple windows per app.
//!
//! ## Design Reference
//!
//! design.txt lines 708-713:
//! - "can pin apps to taskbar on the left, all launched applications go
//!   to the right of those, with a small space and a divider between the
//!   two sections"
//! - "option to show app name along with app icon in taskbar"
//! - "taskbar and/or window titlebars support blurry transparency"
//! - "optional icons on taskbar like Windows: clock, wifi, ..."
//! - "can drag and reorder icons in pinned section and currently running
//!   apps section"
//!
//! ## Architecture
//!
//! ```text
//! Pinned apps (left)  |  Running apps (right)  |  System tray (far right)
//! [Files] [Term] ...  │  [Editor·2] [Browser]  │  [🔊] [📶] [⏰]
//! ```
//!
//! The taskbar maintains two ordered lists:
//! 1. **Pinned apps** — user-chosen shortcuts, persisted across reboots
//! 2. **Running entries** — one per app (windows grouped), auto-managed
//!
//! When a pinned app is also running, its pinned slot shows the running
//! state (no duplicate entry).

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

/// Maximum pinned apps.
const MAX_PINNED: usize = 64;

/// Maximum running entries.
const MAX_RUNNING: usize = 256;

/// Maximum windows per app entry.
const MAX_WINDOWS_PER_APP: usize = 64;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Display style for taskbar entries.
//
// Every mode shows the icon; the shared `Icon` prefix names what (if anything)
// accompanies it, so it is meaningful rather than the redundant prefix
// `enum_variant_names` targets.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelMode {
    /// Icon only (default).
    IconOnly,
    /// Icon + app name.
    IconAndName,
    /// Icon + window title (for active/hovered).
    IconAndTitle,
}

/// Visual state of a taskbar entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryState {
    /// Normal running state.
    Normal,
    /// Flashing for attention (e.g., completed download).
    Attention,
    /// App is not responding.
    NotResponding,
    /// Loading/starting up.
    Loading,
}

/// Progress overlay on a taskbar button (like Windows 7+ progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// No progress indicator.
    None,
    /// Indeterminate (pulsing bar).
    Indeterminate,
    /// Determinate percentage (0-100).
    Normal(u8),
    /// Paused progress.
    Paused(u8),
    /// Error state (red bar).
    Error(u8),
}

/// Taskbar position on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskbarPosition {
    /// Bottom edge (default).
    Bottom,
    /// Top edge.
    Top,
    /// Left edge.
    Left,
    /// Right edge.
    Right,
}

/// Window information within a grouped taskbar entry.
#[derive(Debug, Clone)]
pub struct WindowEntry {
    /// Window ID.
    pub window_id: u64,
    /// Window title.
    pub title: String,
    /// Whether this is the active/focused window.
    pub active: bool,
    /// Whether this window is minimized.
    pub minimized: bool,
}

/// A pinned app on the taskbar.
#[derive(Debug, Clone)]
pub struct PinnedApp {
    /// Application ID (from appregistry).
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Icon resource.
    pub icon: String,
    /// Position (0-based, left to right).
    pub position: u32,
}

/// A running application entry on the taskbar.
#[derive(Debug, Clone)]
pub struct RunningEntry {
    /// Application ID.
    pub app_id: String,
    /// Display name.
    pub name: String,
    /// Icon resource.
    pub icon: String,
    /// Grouped windows.
    pub windows: Vec<WindowEntry>,
    /// Visual state.
    pub state: EntryState,
    /// Progress overlay.
    pub progress: ProgressState,
    /// Badge text (e.g., unread count).
    pub badge: Option<String>,
    /// Position among running entries.
    pub position: u32,
}

/// Configuration for taskbar behavior.
#[derive(Debug, Clone)]
pub struct TaskbarConfig {
    /// Taskbar position on screen.
    pub position: TaskbarPosition,
    /// Label display mode.
    pub label_mode: LabelMode,
    /// Whether to group windows per app.
    pub group_windows: bool,
    /// Whether to show app names (in addition to icons).
    pub show_names: bool,
    /// Auto-hide the taskbar.
    pub auto_hide: bool,
    /// Use small icons.
    pub small_icons: bool,
}

impl Default for TaskbarConfig {
    fn default() -> Self {
        Self {
            position: TaskbarPosition::Bottom,
            label_mode: LabelMode::IconOnly,
            group_windows: true,
            show_names: false,
            auto_hide: false,
            small_icons: false,
        }
    }
}

/// Snapshot for rendering the entire taskbar.
#[derive(Debug, Clone)]
pub struct TaskbarSnapshot {
    /// Pinned apps (ordered).
    pub pinned: Vec<PinnedApp>,
    /// Running apps (ordered).
    pub running: Vec<RunningEntry>,
    /// Config.
    pub config: TaskbarConfig,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct TaskbarState {
    /// Pinned apps, ordered by position.
    pinned: Vec<PinnedApp>,
    /// App ID → RunningEntry.
    running: BTreeMap<String, RunningEntry>,
    /// Configuration.
    config: TaskbarConfig,
    /// Next running position counter.
    next_run_pos: u32,
}

impl TaskbarState {
    const fn new() -> Self {
        Self {
            pinned: Vec::new(),
            running: BTreeMap::new(),
            config: TaskbarConfig {
                position: TaskbarPosition::Bottom,
                label_mode: LabelMode::IconOnly,
                group_windows: true,
                show_names: false,
                auto_hide: false,
                small_icons: false,
            },
            next_run_pos: 0,
        }
    }
}

static TASKBAR: Mutex<TaskbarState> = Mutex::new(TaskbarState::new());
static PIN_COUNT: AtomicU64 = AtomicU64::new(0);
static WINDOW_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Pinned apps API
// ---------------------------------------------------------------------------

/// Pin an app to the taskbar.
pub fn pin(app_id: &str, name: &str, icon: &str) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    PIN_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut tb = TASKBAR.lock();
    // Check if already pinned.
    if tb.pinned.iter().any(|p| p.app_id == app_id) {
        return Err(KernelError::AlreadyExists);
    }
    if tb.pinned.len() >= MAX_PINNED {
        return Err(KernelError::ResourceExhausted);
    }

    let pos = tb.pinned.len() as u32;
    tb.pinned.push(PinnedApp {
        app_id: String::from(app_id),
        name: String::from(name),
        icon: String::from(icon),
        position: pos,
    });
    Ok(())
}

/// Unpin an app from the taskbar.
pub fn unpin(app_id: &str) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let idx = tb.pinned.iter().position(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    tb.pinned.remove(idx);
    // Recalculate positions.
    for (i, p) in tb.pinned.iter_mut().enumerate() {
        p.position = i as u32;
    }
    Ok(())
}

/// Reorder a pinned app to a new position.
pub fn reorder_pinned(app_id: &str, new_pos: u32) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let idx = tb.pinned.iter().position(|p| p.app_id == app_id)
        .ok_or(KernelError::NotFound)?;

    let new_idx = (new_pos as usize).min(tb.pinned.len().saturating_sub(1));
    let item = tb.pinned.remove(idx);
    tb.pinned.insert(new_idx, item);

    // Recalculate positions.
    for (i, p) in tb.pinned.iter_mut().enumerate() {
        p.position = i as u32;
    }
    Ok(())
}

/// Get all pinned apps in order.
pub fn pinned_apps() -> Vec<PinnedApp> {
    let tb = TASKBAR.lock();
    tb.pinned.clone()
}

/// Check if an app is pinned.
pub fn is_pinned(app_id: &str) -> bool {
    let tb = TASKBAR.lock();
    tb.pinned.iter().any(|p| p.app_id == app_id)
}

// ---------------------------------------------------------------------------
// Running apps API
// ---------------------------------------------------------------------------

/// Add a window to the taskbar. Creates a new running entry if needed.
pub fn add_window(app_id: &str, name: &str, icon: &str,
                  window_id: u64, title: &str) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    WINDOW_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut tb = TASKBAR.lock();

    if let Some(entry) = tb.running.get_mut(app_id) {
        // Add window to existing entry.
        if entry.windows.len() >= MAX_WINDOWS_PER_APP {
            return Err(KernelError::ResourceExhausted);
        }
        entry.windows.push(WindowEntry {
            window_id,
            title: String::from(title),
            active: false,
            minimized: false,
        });
    } else {
        // New entry.
        if tb.running.len() >= MAX_RUNNING {
            return Err(KernelError::ResourceExhausted);
        }
        let pos = tb.next_run_pos;
        tb.next_run_pos = tb.next_run_pos.saturating_add(1);
        tb.running.insert(String::from(app_id), RunningEntry {
            app_id: String::from(app_id),
            name: String::from(name),
            icon: String::from(icon),
            windows: alloc::vec![WindowEntry {
                window_id,
                title: String::from(title),
                active: false,
                minimized: false,
            }],
            state: EntryState::Normal,
            progress: ProgressState::None,
            badge: None,
            position: pos,
        });
    }
    Ok(())
}

/// Remove a window from the taskbar. Removes the entry if no windows remain.
pub fn remove_window(app_id: &str, window_id: u64) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;

    let idx = entry.windows.iter().position(|w| w.window_id == window_id)
        .ok_or(KernelError::NotFound)?;
    entry.windows.remove(idx);

    if entry.windows.is_empty() {
        tb.running.remove(app_id);
    }
    Ok(())
}

/// Set the active/focused window.
pub fn set_active_window(app_id: &str, window_id: u64) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    // Clear active on all entries.
    for entry in tb.running.values_mut() {
        for w in &mut entry.windows {
            w.active = false;
        }
    }
    // Set the target window active.
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;
    for w in &mut entry.windows {
        if w.window_id == window_id {
            w.active = true;
            return Ok(());
        }
    }
    Err(KernelError::NotFound)
}

/// Update window title.
pub fn set_window_title(app_id: &str, window_id: u64, title: &str) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;
    for w in &mut entry.windows {
        if w.window_id == window_id {
            w.title = String::from(title);
            return Ok(());
        }
    }
    Err(KernelError::NotFound)
}

/// Set minimize state for a window.
pub fn set_minimized(app_id: &str, window_id: u64, minimized: bool) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;
    for w in &mut entry.windows {
        if w.window_id == window_id {
            w.minimized = minimized;
            return Ok(());
        }
    }
    Err(KernelError::NotFound)
}

/// Set the visual state of a running entry.
pub fn set_state(app_id: &str, state: EntryState) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;
    entry.state = state;
    Ok(())
}

/// Set progress overlay on a running entry.
pub fn set_progress(app_id: &str, progress: ProgressState) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;
    entry.progress = progress;
    Ok(())
}

/// Set badge on a running entry.
pub fn set_badge(app_id: &str, badge: Option<&str>) -> KernelResult<()> {
    let mut tb = TASKBAR.lock();
    let entry = tb.running.get_mut(app_id).ok_or(KernelError::NotFound)?;
    entry.badge = badge.map(String::from);
    Ok(())
}

/// Flash a running entry for attention.
pub fn request_attention(app_id: &str) -> KernelResult<()> {
    set_state(app_id, EntryState::Attention)
}

/// Get all running entries in order.
pub fn running_apps() -> Vec<RunningEntry> {
    let tb = TASKBAR.lock();
    let mut entries: Vec<RunningEntry> = tb.running.values().cloned().collect();
    entries.sort_by_key(|e| e.position);
    entries
}

/// Get a specific running entry.
pub fn get_running(app_id: &str) -> Option<RunningEntry> {
    let tb = TASKBAR.lock();
    tb.running.get(app_id).cloned()
}

// ---------------------------------------------------------------------------
// Configuration API
// ---------------------------------------------------------------------------

/// Get current taskbar configuration.
pub fn config() -> TaskbarConfig {
    let tb = TASKBAR.lock();
    tb.config.clone()
}

/// Set taskbar position.
pub fn set_position(pos: TaskbarPosition) {
    let mut tb = TASKBAR.lock();
    tb.config.position = pos;
}

/// Set label display mode.
pub fn set_label_mode(mode: LabelMode) {
    let mut tb = TASKBAR.lock();
    tb.config.label_mode = mode;
}

/// Toggle window grouping.
pub fn set_grouping(enabled: bool) {
    let mut tb = TASKBAR.lock();
    tb.config.group_windows = enabled;
}

/// Toggle app name display.
pub fn set_show_names(enabled: bool) {
    let mut tb = TASKBAR.lock();
    tb.config.show_names = enabled;
}

/// Toggle auto-hide.
pub fn set_auto_hide(enabled: bool) {
    let mut tb = TASKBAR.lock();
    tb.config.auto_hide = enabled;
}

/// Toggle small icons.
pub fn set_small_icons(enabled: bool) {
    let mut tb = TASKBAR.lock();
    tb.config.small_icons = enabled;
}

// ---------------------------------------------------------------------------
// Snapshot / rendering
// ---------------------------------------------------------------------------

/// Build a full taskbar snapshot for rendering.
pub fn snapshot() -> TaskbarSnapshot {
    let tb = TASKBAR.lock();
    let mut running: Vec<RunningEntry> = tb.running.values().cloned().collect();
    running.sort_by_key(|e| e.position);

    TaskbarSnapshot {
        pinned: tb.pinned.clone(),
        running,
        config: tb.config.clone(),
    }
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (pinned_count, running_count, total_windows, pin_ops, window_ops).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let tb = TASKBAR.lock();
    let total_windows: usize = tb.running.values()
        .map(|e| e.windows.len())
        .sum();
    (
        tb.pinned.len(),
        tb.running.len(),
        total_windows,
        PIN_COUNT.load(Ordering::Relaxed),
        WINDOW_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    PIN_COUNT.store(0, Ordering::Relaxed);
    WINDOW_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut tb = TASKBAR.lock();
    tb.pinned.clear();
    tb.running.clear();
    tb.next_run_pos = 0;
    tb.config = TaskbarConfig::default();
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the taskbar.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: pin and list.
    {
        pin("org.os.files", "Files", "icon-files")?;
        pin("org.os.terminal", "Terminal", "icon-terminal")?;
        let pins = pinned_apps();
        assert_eq!(pins.len(), 2);
        assert_eq!(pins[0].app_id, "org.os.files");
        assert_eq!(pins[1].app_id, "org.os.terminal");
        assert!(is_pinned("org.os.files"));
        serial_println!("[taskbar] test 1 passed: pin/list");
    }

    // Test 2: reorder pinned.
    {
        reorder_pinned("org.os.terminal", 0)?;
        let pins = pinned_apps();
        assert_eq!(pins[0].app_id, "org.os.terminal");
        assert_eq!(pins[1].app_id, "org.os.files");
        serial_println!("[taskbar] test 2 passed: reorder pinned");
    }

    // Test 3: add running windows.
    {
        add_window("org.os.editor", "Editor", "icon-editor", 100, "untitled.txt")?;
        add_window("org.os.editor", "Editor", "icon-editor", 101, "readme.md")?;
        add_window("org.os.browser", "Browser", "icon-browser", 200, "Home Page")?;

        let running = running_apps();
        assert_eq!(running.len(), 2);

        let editor = get_running("org.os.editor").unwrap();
        assert_eq!(editor.windows.len(), 2);
        assert_eq!(editor.windows[0].title, "untitled.txt");
        serial_println!("[taskbar] test 3 passed: add windows");
    }

    // Test 4: active window and title update.
    {
        set_active_window("org.os.editor", 101)?;
        let editor = get_running("org.os.editor").unwrap();
        assert!(editor.windows[1].active);
        assert!(!editor.windows[0].active);

        set_window_title("org.os.editor", 100, "main.rs")?;
        let editor = get_running("org.os.editor").unwrap();
        assert_eq!(editor.windows[0].title, "main.rs");
        serial_println!("[taskbar] test 4 passed: active/title");
    }

    // Test 5: progress and badge.
    {
        set_progress("org.os.browser", ProgressState::Normal(50))?;
        set_badge("org.os.browser", Some("3"))?;
        let browser = get_running("org.os.browser").unwrap();
        assert_eq!(browser.progress, ProgressState::Normal(50));
        assert_eq!(browser.badge.as_deref(), Some("3"));
        serial_println!("[taskbar] test 5 passed: progress/badge");
    }

    // Test 6: remove window, entry auto-removal.
    {
        remove_window("org.os.browser", 200)?;
        assert!(get_running("org.os.browser").is_none());
        serial_println!("[taskbar] test 6 passed: remove window");
    }

    // Test 7: snapshot and config.
    {
        set_position(TaskbarPosition::Top);
        set_show_names(true);
        let snap = snapshot();
        assert_eq!(snap.pinned.len(), 2);
        assert_eq!(snap.running.len(), 1);
        assert_eq!(snap.config.position, TaskbarPosition::Top);
        assert!(snap.config.show_names);

        // Unpin.
        unpin("org.os.files")?;
        assert!(!is_pinned("org.os.files"));
        serial_println!("[taskbar] test 7 passed: snapshot/config/unpin");
    }

    clear_all();
    reset_stats();

    serial_println!("[taskbar] all 7 self-tests passed");
    Ok(())
}
