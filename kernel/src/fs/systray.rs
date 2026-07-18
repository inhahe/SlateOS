//! System tray — notification area icon management.
//!
//! Manages the system tray (notification area) on the taskbar.
//! Applications can register tray icons that persist in the tray
//! with optional tooltip, badge, and click action.
//!
//! ## Design Reference
//!
//! design.txt lines 714-717:
//! - "a system tray like on Windows"
//! - "can drag and drop icons into and out of the system tray"
//! - "apps have the option of starting in system tray or minimizing to system tray"
//! - "can override any app to always start in system tray or in taskbar"
//!
//! design.txt line 784:
//! - "minimize to tray / startup in tray"
//!
//! ## Architecture
//!
//! ```text
//! App process
//!   → systray::add_icon(TrayIcon { ... })
//!   → icon appears in notification area
//!
//! User clicks tray icon
//!   → systray::handle_click(id, ClickType)
//!   → dispatched to registered callback
//!
//! User drags icon out of tray
//!   → systray::remove_icon(id)
//!   → icon removed from tray area
//! ```
//!
//! ## Integration
//!
//! Works with `appregistry` for `tray_icon` / `start_hidden` flags,
//! and with `notifcenter` for badge/notification counts.

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

/// Maximum tray icons.
const MAX_ICONS: usize = 128;

/// Maximum menu items per icon.
const MAX_MENU_ITEMS: usize = 32;

/// Maximum overrides (user preferences for tray behavior).
const MAX_OVERRIDES: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// How a tray icon was clicked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickType {
    /// Primary (left) click.
    Primary,
    /// Secondary (right) click — typically opens context menu.
    Secondary,
    /// Double-click — typically restores/focuses the app window.
    Double,
    /// Middle click.
    Middle,
}

/// What should happen when the user clicks the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickAction {
    /// Show/focus the app's main window.
    ShowWindow,
    /// Toggle visibility of the app's main window.
    ToggleWindow,
    /// Open the icon's context menu.
    ContextMenu,
    /// Do nothing (app handles it via IPC).
    Custom,
}

/// User override for how an app uses the tray.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayOverride {
    /// App decides (default).
    Default,
    /// Always start in tray (minimized).
    AlwaysStartInTray,
    /// Always start in taskbar (visible window).
    AlwaysStartInTaskbar,
    /// Never show tray icon (taskbar only).
    NoTrayIcon,
    /// Tray icon only, no taskbar entry.
    TrayOnly,
}

/// Visibility state of the tray icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconVisibility {
    /// Always visible in the tray.
    Visible,
    /// Hidden in overflow area.
    Overflow,
    /// Temporarily hidden by the app.
    Hidden,
}

/// A context menu item for a tray icon.
#[derive(Debug, Clone)]
pub struct TrayMenuItem {
    /// Label text.
    pub label: String,
    /// Action identifier sent back to the app.
    pub action_id: String,
    /// Whether the item is enabled.
    pub enabled: bool,
    /// Whether this is a separator line.
    pub separator: bool,
    /// Whether the item has a checkmark.
    pub checked: bool,
}

/// A tray icon registered by an application.
#[derive(Debug, Clone)]
pub struct TrayIcon {
    /// Unique icon ID (typically app ID).
    pub id: String,
    /// Application ID from appregistry.
    pub app_id: String,
    /// Display tooltip.
    pub tooltip: String,
    /// Icon resource name.
    pub icon: String,
    /// Optional badge text (e.g., unread count).
    pub badge: Option<String>,
    /// What happens on primary click.
    pub click_action: ClickAction,
    /// Context menu items (shown on right-click).
    pub menu_items: Vec<TrayMenuItem>,
    /// Visibility state.
    pub visibility: IconVisibility,
    /// Order position (lower = further left).
    pub order: u32,
    /// Whether the app window is currently visible.
    pub window_visible: bool,
    /// Timestamp when icon was added (nanoseconds).
    pub added_ns: u64,
}

/// Snapshot of tray state for rendering.
#[derive(Debug, Clone)]
pub struct TraySnapshot {
    /// Icon ID.
    pub id: String,
    /// Tooltip.
    pub tooltip: String,
    /// Icon resource.
    pub icon: String,
    /// Badge text if any.
    pub badge: Option<String>,
    /// Visibility.
    pub visibility: IconVisibility,
    /// Order position.
    pub order: u32,
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

struct TrayState {
    /// Icon ID → TrayIcon.
    icons: BTreeMap<String, TrayIcon>,
    /// App ID → TrayOverride (user preferences).
    overrides: BTreeMap<String, TrayOverride>,
    /// Ordering counter.
    next_order: u32,
}

impl TrayState {
    const fn new() -> Self {
        Self {
            icons: BTreeMap::new(),
            overrides: BTreeMap::new(),
            next_order: 0,
        }
    }
}

static TRAY: Mutex<TrayState> = Mutex::new(TrayState::new());
static ADD_COUNT: AtomicU64 = AtomicU64::new(0);
static CLICK_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Core API
// ---------------------------------------------------------------------------

/// Add or update a tray icon.
pub fn add_icon(icon: TrayIcon) -> KernelResult<()> {
    if icon.id.is_empty() || icon.app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    if icon.menu_items.len() > MAX_MENU_ITEMS {
        return Err(KernelError::InvalidArgument);
    }

    ADD_COUNT.fetch_add(1, Ordering::Relaxed);
    let mut tray = TRAY.lock();

    if !tray.icons.contains_key(&icon.id) && tray.icons.len() >= MAX_ICONS {
        return Err(KernelError::ResourceExhausted);
    }

    // Check user override — skip if NoTrayIcon.
    if let Some(TrayOverride::NoTrayIcon) = tray.overrides.get(&icon.app_id) {
        return Err(KernelError::PermissionDenied);
    }

    let mut icon = icon;
    if icon.order == 0 {
        icon.order = tray.next_order;
        tray.next_order = tray.next_order.saturating_add(1);
    }

    tray.icons.insert(icon.id.clone(), icon);
    Ok(())
}

/// Remove a tray icon.
pub fn remove_icon(id: &str) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    tray.icons.remove(id).ok_or(KernelError::NotFound)?;
    Ok(())
}

/// Get a tray icon by ID.
pub fn get_icon(id: &str) -> Option<TrayIcon> {
    let tray = TRAY.lock();
    tray.icons.get(id).cloned()
}

/// Get all visible tray icons in order.
pub fn visible_icons() -> Vec<TraySnapshot> {
    let tray = TRAY.lock();
    let mut icons: Vec<TraySnapshot> = tray.icons.values()
        .filter(|i| i.visibility == IconVisibility::Visible)
        .map(|i| TraySnapshot {
            id: i.id.clone(),
            tooltip: i.tooltip.clone(),
            icon: i.icon.clone(),
            badge: i.badge.clone(),
            visibility: i.visibility,
            order: i.order,
        })
        .collect();
    icons.sort_by_key(|i| i.order);
    icons
}

/// Get overflow (hidden in collapsed area) tray icons.
pub fn overflow_icons() -> Vec<TraySnapshot> {
    let tray = TRAY.lock();
    let mut icons: Vec<TraySnapshot> = tray.icons.values()
        .filter(|i| i.visibility == IconVisibility::Overflow)
        .map(|i| TraySnapshot {
            id: i.id.clone(),
            tooltip: i.tooltip.clone(),
            icon: i.icon.clone(),
            badge: i.badge.clone(),
            visibility: i.visibility,
            order: i.order,
        })
        .collect();
    icons.sort_by_key(|i| i.order);
    icons
}

/// Handle a click on a tray icon. Returns the click action.
pub fn handle_click(id: &str, click: ClickType) -> KernelResult<ClickAction> {
    CLICK_COUNT.fetch_add(1, Ordering::Relaxed);
    let tray = TRAY.lock();
    let icon = tray.icons.get(id).ok_or(KernelError::NotFound)?;

    match click {
        ClickType::Primary => Ok(icon.click_action),
        ClickType::Secondary => Ok(ClickAction::ContextMenu),
        ClickType::Double => Ok(ClickAction::ShowWindow),
        ClickType::Middle => Ok(ClickAction::Custom),
    }
}

/// Get the context menu for a tray icon.
pub fn get_menu(id: &str) -> KernelResult<Vec<TrayMenuItem>> {
    let tray = TRAY.lock();
    let icon = tray.icons.get(id).ok_or(KernelError::NotFound)?;
    Ok(icon.menu_items.clone())
}

/// Update the badge on a tray icon.
pub fn set_badge(id: &str, badge: Option<&str>) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    let icon = tray.icons.get_mut(id).ok_or(KernelError::NotFound)?;
    icon.badge = badge.map(String::from);
    Ok(())
}

/// Update the tooltip on a tray icon.
pub fn set_tooltip(id: &str, tooltip: &str) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    let icon = tray.icons.get_mut(id).ok_or(KernelError::NotFound)?;
    icon.tooltip = String::from(tooltip);
    Ok(())
}

/// Update the icon resource.
pub fn set_icon_resource(id: &str, icon_name: &str) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    let icon = tray.icons.get_mut(id).ok_or(KernelError::NotFound)?;
    icon.icon = String::from(icon_name);
    Ok(())
}

/// Set whether the app window is visible (affects toggle behavior).
pub fn set_window_visible(id: &str, visible: bool) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    let icon = tray.icons.get_mut(id).ok_or(KernelError::NotFound)?;
    icon.window_visible = visible;
    Ok(())
}

/// Change visibility of a tray icon (move to/from overflow).
pub fn set_visibility(id: &str, vis: IconVisibility) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    let icon = tray.icons.get_mut(id).ok_or(KernelError::NotFound)?;
    icon.visibility = vis;
    Ok(())
}

/// Reorder a tray icon to a new position.
pub fn reorder(id: &str, new_order: u32) -> KernelResult<()> {
    let mut tray = TRAY.lock();
    let icon = tray.icons.get_mut(id).ok_or(KernelError::NotFound)?;
    icon.order = new_order;
    Ok(())
}

// ---------------------------------------------------------------------------
// User overrides
// ---------------------------------------------------------------------------

/// Set user override for how an app uses the tray.
pub fn set_override(app_id: &str, ov: TrayOverride) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut tray = TRAY.lock();
    if !tray.overrides.contains_key(app_id) && tray.overrides.len() >= MAX_OVERRIDES {
        return Err(KernelError::ResourceExhausted);
    }
    if ov == TrayOverride::Default {
        tray.overrides.remove(app_id);
    } else {
        tray.overrides.insert(String::from(app_id), ov);
    }
    Ok(())
}

/// Get the override for an app (Default if none set).
pub fn get_override(app_id: &str) -> TrayOverride {
    let tray = TRAY.lock();
    tray.overrides.get(app_id).copied().unwrap_or(TrayOverride::Default)
}

/// List all overrides.
pub fn list_overrides() -> Vec<(String, TrayOverride)> {
    let tray = TRAY.lock();
    tray.overrides.iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect()
}

/// Check whether an app should start in tray based on override + appregistry.
pub fn should_start_in_tray(app_id: &str) -> bool {
    let tray = TRAY.lock();
    match tray.overrides.get(app_id) {
        Some(TrayOverride::AlwaysStartInTray) => true,
        Some(TrayOverride::AlwaysStartInTaskbar) => false,
        Some(TrayOverride::NoTrayIcon) => false,
        Some(TrayOverride::TrayOnly) => true,
        Some(TrayOverride::Default) | None => {
            // Check appregistry for start_hidden flag.
            drop(tray);
            super::appregistry::get(app_id)
                .is_some_and(|app| app.start_hidden)
        }
    }
}

/// Check whether an app should have a tray icon at all.
pub fn should_have_tray_icon(app_id: &str) -> bool {
    let tray = TRAY.lock();
    match tray.overrides.get(app_id) {
        Some(TrayOverride::NoTrayIcon) => false,
        Some(TrayOverride::AlwaysStartInTaskbar) => false,
        Some(TrayOverride::AlwaysStartInTray) => true,
        Some(TrayOverride::TrayOnly) => true,
        Some(TrayOverride::Default) | None => {
            // Check appregistry for tray_icon flag.
            drop(tray);
            super::appregistry::get(app_id)
                .is_some_and(|app| app.tray_icon)
        }
    }
}

// ---------------------------------------------------------------------------
// Bulk operations
// ---------------------------------------------------------------------------

/// Count tray icons.
pub fn icon_count() -> usize {
    let tray = TRAY.lock();
    tray.icons.len()
}

/// List all icon IDs.
pub fn list_ids() -> Vec<String> {
    let tray = TRAY.lock();
    tray.icons.keys().cloned().collect()
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (icon_count, override_count, add_ops, click_ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let tray = TRAY.lock();
    (
        tray.icons.len(),
        tray.overrides.len(),
        ADD_COUNT.load(Ordering::Relaxed),
        CLICK_COUNT.load(Ordering::Relaxed),
    )
}

/// Reset statistics.
pub fn reset_stats() {
    ADD_COUNT.store(0, Ordering::Relaxed);
    CLICK_COUNT.store(0, Ordering::Relaxed);
}

/// Clear all data.
pub fn clear_all() {
    let mut tray = TRAY.lock();
    tray.icons.clear();
    tray.overrides.clear();
    tray.next_order = 0;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the system tray.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();
    reset_stats();

    // Test 1: add and get icon.
    {
        add_icon(TrayIcon {
            id: String::from("test.tray"),
            app_id: String::from("test.app"),
            tooltip: String::from("Test App"),
            icon: String::from("icon-test"),
            badge: None,
            click_action: ClickAction::ToggleWindow,
            menu_items: Vec::new(),
            visibility: IconVisibility::Visible,
            order: 0,
            window_visible: true,
            added_ns: 0,
        })?;
        let icon = get_icon("test.tray").unwrap();
        assert_eq!(icon.tooltip, "Test App");
        assert_eq!(icon.click_action, ClickAction::ToggleWindow);
        serial_println!("[systray] test 1 passed: add/get icon");
    }

    // Test 2: visible icons.
    {
        let vis = visible_icons();
        assert_eq!(vis.len(), 1);
        assert_eq!(vis[0].id, "test.tray");
        serial_println!("[systray] test 2 passed: visible_icons");
    }

    // Test 3: set badge and tooltip.
    {
        set_badge("test.tray", Some("3"))?;
        set_tooltip("test.tray", "3 new messages")?;
        let icon = get_icon("test.tray").unwrap();
        assert_eq!(icon.badge.as_deref(), Some("3"));
        assert_eq!(icon.tooltip, "3 new messages");
        serial_println!("[systray] test 3 passed: badge/tooltip update");
    }

    // Test 4: handle click.
    {
        let action = handle_click("test.tray", ClickType::Primary)?;
        assert_eq!(action, ClickAction::ToggleWindow);
        let action = handle_click("test.tray", ClickType::Secondary)?;
        assert_eq!(action, ClickAction::ContextMenu);
        serial_println!("[systray] test 4 passed: click handling");
    }

    // Test 5: visibility change.
    {
        set_visibility("test.tray", IconVisibility::Overflow)?;
        let vis = visible_icons();
        assert!(vis.is_empty());
        let overflow = overflow_icons();
        assert_eq!(overflow.len(), 1);
        set_visibility("test.tray", IconVisibility::Visible)?;
        serial_println!("[systray] test 5 passed: visibility");
    }

    // Test 6: user overrides.
    {
        set_override("test.app", TrayOverride::AlwaysStartInTray)?;
        assert!(should_start_in_tray("test.app"));

        set_override("test.app", TrayOverride::NoTrayIcon)?;
        assert!(!should_have_tray_icon("test.app"));

        // With NoTrayIcon override, adding icon should fail.
        let result = add_icon(TrayIcon {
            id: String::from("test.blocked"),
            app_id: String::from("test.app"),
            tooltip: String::from("Blocked"),
            icon: String::from("icon-blocked"),
            badge: None,
            click_action: ClickAction::ShowWindow,
            menu_items: Vec::new(),
            visibility: IconVisibility::Visible,
            order: 0,
            window_visible: false,
            added_ns: 0,
        });
        assert!(result.is_err());

        set_override("test.app", TrayOverride::Default)?;
        serial_println!("[systray] test 6 passed: user overrides");
    }

    // Test 7: remove icon.
    {
        let count_before = icon_count();
        remove_icon("test.tray")?;
        assert_eq!(icon_count(), count_before - 1);
        assert!(get_icon("test.tray").is_none());
        serial_println!("[systray] test 7 passed: remove_icon");
    }

    clear_all();
    reset_stats();

    serial_println!("[systray] all 7 self-tests passed");
    Ok(())
}
