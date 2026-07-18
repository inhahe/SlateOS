//! Notification preferences — per-application notification settings.
//!
//! Controls how each application's notifications are displayed,
//! including sound, banner, lock screen visibility, priority,
//! and do-not-disturb overrides.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Notifications → App Preferences
//!   → notifprefs::set_app_pref() / set_banner_style()
//!
//! Notification delivery flow
//!   1. App sends notification via notifcenter
//!   2. notifprefs::should_show(app_id) checks preferences
//!   3. If allowed, display per banner_style / play sound
//!
//! Integration:
//!   → notifcenter (notification display pipeline)
//!   → focusassist (DND mode overrides)
//!   → appregistry (app metadata)
//!   → soundmixer (notification sounds)
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

const MAX_APP_PREFS: usize = 256;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Banner display style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerStyle {
    /// Full banner with content preview.
    Full,
    /// Brief banner (title only).
    Brief,
    /// No banner — silent delivery to notification center.
    None,
}

impl BannerStyle {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Brief => "Brief",
            Self::None => "None",
        }
    }
}

/// Notification priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifPriority {
    /// Critical — always show, even in DND.
    Critical,
    /// High — show prominently.
    High,
    /// Normal — default behaviour.
    Normal,
    /// Low — bundled, delayed.
    Low,
    /// Silent — no alerts.
    Silent,
}

impl NotifPriority {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::High => "High",
            Self::Normal => "Normal",
            Self::Low => "Low",
            Self::Silent => "Silent",
        }
    }
}

/// Lock screen visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockScreenVisibility {
    /// Show full content on lock screen.
    Full,
    /// Show sender/app only, hide content.
    SenderOnly,
    /// Don't show on lock screen.
    Hidden,
}

impl LockScreenVisibility {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::SenderOnly => "Sender Only",
            Self::Hidden => "Hidden",
        }
    }
}

/// Per-application notification preferences.
#[derive(Debug, Clone)]
pub struct AppNotifPref {
    /// Application ID.
    pub app_id: String,
    /// Whether notifications are enabled for this app.
    pub enabled: bool,
    /// Banner display style.
    pub banner_style: BannerStyle,
    /// Play sound.
    pub sound: bool,
    /// Custom sound name (empty = default).
    pub sound_name: String,
    /// Show badge (unread count).
    pub badge: bool,
    /// Priority level.
    pub priority: NotifPriority,
    /// Lock screen visibility.
    pub lock_screen: LockScreenVisibility,
    /// Allow this app to override DND.
    pub dnd_override: bool,
    /// Group notifications from this app.
    pub group: bool,
    /// Maximum notifications to keep in history.
    pub max_history: u32,
    /// Notification count (lifetime).
    pub total_count: u64,
    /// Suppressed count (blocked by prefs or DND).
    pub suppressed_count: u64,
}

// ---------------------------------------------------------------------------
// Global notification settings
// ---------------------------------------------------------------------------

/// Global notification configuration.
#[derive(Debug, Clone)]
pub struct GlobalNotifConfig {
    /// Show notifications on lock screen.
    pub show_on_lock_screen: bool,
    /// Play sounds for notifications.
    pub sounds_enabled: bool,
    /// Show notification count in status bar.
    pub show_count: bool,
    /// Auto-dismiss timeout (seconds, 0 = persistent).
    pub dismiss_timeout_seconds: u32,
    /// Maximum visible notifications at once.
    pub max_visible: u32,
    /// Position: top-right, top-left, bottom-right, bottom-left.
    pub position: NotifPosition,
}

/// Notification display position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifPosition {
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

impl NotifPosition {
    pub fn label(self) -> &'static str {
        match self {
            Self::TopRight => "Top Right",
            Self::TopLeft => "Top Left",
            Self::BottomRight => "Bottom Right",
            Self::BottomLeft => "Bottom Left",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct NotifPrefState {
    global: GlobalNotifConfig,
    app_prefs: Vec<AppNotifPref>,
    ops: u64,
}

static STATE: Mutex<Option<NotifPrefState>> = Mutex::new(None);
static OPS: AtomicU64 = AtomicU64::new(0);

fn with_state<F, R>(f: F) -> KernelResult<R>
where
    F: FnOnce(&mut NotifPrefState) -> KernelResult<R>,
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

/// Initialize the notification preferences subsystem.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    *guard = Some(NotifPrefState {
        global: GlobalNotifConfig {
            show_on_lock_screen: true,
            sounds_enabled: true,
            show_count: true,
            dismiss_timeout_seconds: 5,
            max_visible: 5,
            position: NotifPosition::TopRight,
        },
        app_prefs: Vec::new(),
        ops: 0,
    });
}

// ---------------------------------------------------------------------------
// Global settings
// ---------------------------------------------------------------------------

/// Get global notification config.
pub fn global_config() -> KernelResult<GlobalNotifConfig> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    Ok(state.global.clone())
}

/// Set lock screen notifications.
pub fn set_show_on_lock_screen(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.global.show_on_lock_screen = enabled; Ok(()) })
}

/// Set global notification sounds.
pub fn set_sounds_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.global.sounds_enabled = enabled; Ok(()) })
}

/// Set notification count in status bar.
pub fn set_show_count(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.global.show_count = enabled; Ok(()) })
}

/// Set auto-dismiss timeout.
pub fn set_dismiss_timeout(seconds: u32) -> KernelResult<()> {
    with_state(|state| { state.global.dismiss_timeout_seconds = seconds; Ok(()) })
}

/// Set max visible notifications.
pub fn set_max_visible(count: u32) -> KernelResult<()> {
    if count == 0 || count > 20 {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| { state.global.max_visible = count; Ok(()) })
}

/// Set notification position.
pub fn set_position(pos: NotifPosition) -> KernelResult<()> {
    with_state(|state| { state.global.position = pos; Ok(()) })
}

// ---------------------------------------------------------------------------
// Per-app preferences
// ---------------------------------------------------------------------------

/// Get or create app notification preferences.
pub fn get_app_pref(app_id: &str) -> KernelResult<AppNotifPref> {
    let guard = STATE.lock();
    let state = guard.as_ref().ok_or(KernelError::NotSupported)?;
    if let Some(pref) = state.app_prefs.iter().find(|p| p.app_id == app_id) {
        return Ok(pref.clone());
    }
    // Return default.
    Ok(AppNotifPref {
        app_id: String::from(app_id),
        enabled: true,
        banner_style: BannerStyle::Full,
        sound: true,
        sound_name: String::new(),
        badge: true,
        priority: NotifPriority::Normal,
        lock_screen: LockScreenVisibility::Full,
        dnd_override: false,
        group: true,
        max_history: 50,
        total_count: 0,
        suppressed_count: 0,
    })
}

/// Set app notifications enabled/disabled.
pub fn set_app_enabled(app_id: &str, enabled: bool) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.enabled = enabled;
        Ok(())
    })
}

/// Set app banner style.
pub fn set_banner_style(app_id: &str, style: BannerStyle) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.banner_style = style;
        Ok(())
    })
}

/// Set app notification sound.
pub fn set_app_sound(app_id: &str, enabled: bool) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.sound = enabled;
        Ok(())
    })
}

/// Set app notification priority.
pub fn set_app_priority(app_id: &str, priority: NotifPriority) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.priority = priority;
        Ok(())
    })
}

/// Set app lock screen visibility.
pub fn set_app_lock_screen(app_id: &str, vis: LockScreenVisibility) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.lock_screen = vis;
        Ok(())
    })
}

/// Set DND override for app.
pub fn set_dnd_override(app_id: &str, allowed: bool) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.dnd_override = allowed;
        Ok(())
    })
}

/// Set grouping for app notifications.
pub fn set_app_group(app_id: &str, group: bool) -> KernelResult<()> {
    ensure_app_pref(app_id)?;
    with_state(|state| {
        let pref = state.app_prefs.iter_mut().find(|p| p.app_id == app_id)
            .ok_or(KernelError::NotFound)?;
        pref.group = group;
        Ok(())
    })
}

/// Remove app-specific preferences (revert to defaults).
pub fn remove_app_pref(app_id: &str) -> KernelResult<()> {
    with_state(|state| {
        if let Some(pos) = state.app_prefs.iter().position(|p| p.app_id == app_id) {
            state.app_prefs.remove(pos);
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List all app preferences.
pub fn list_app_prefs() -> Vec<AppNotifPref> {
    let guard = STATE.lock();
    guard.as_ref().map_or_else(Vec::new, |s| s.app_prefs.clone())
}

// ---------------------------------------------------------------------------
// Notification check (used by notifcenter)
// ---------------------------------------------------------------------------

/// Check if a notification should be shown for an app.
///
/// Records the notification and returns whether it should be displayed.
pub fn should_show(app_id: &str) -> bool {
    let mut guard = STATE.lock();
    let state = match guard.as_mut() {
        Some(s) => s,
        None => return true,
    };

    if let Some(pref) = state.app_prefs.iter_mut().find(|p| p.app_id == app_id) {
        pref.total_count += 1;
        if !pref.enabled {
            pref.suppressed_count += 1;
            return false;
        }
        true
    } else {
        // No specific pref — allow by default.
        true
    }
}

/// Check if sound should play for an app's notification.
pub fn should_play_sound(app_id: &str) -> bool {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return true,
    };

    if !state.global.sounds_enabled {
        return false;
    }

    if let Some(pref) = state.app_prefs.iter().find(|p| p.app_id == app_id) {
        pref.sound && pref.enabled
    } else {
        true
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn ensure_app_pref(app_id: &str) -> KernelResult<()> {
    if app_id.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        if state.app_prefs.iter().any(|p| p.app_id == app_id) {
            return Ok(());
        }
        if state.app_prefs.len() >= MAX_APP_PREFS {
            return Err(KernelError::ResourceExhausted);
        }
        state.app_prefs.push(AppNotifPref {
            app_id: String::from(app_id),
            enabled: true,
            banner_style: BannerStyle::Full,
            sound: true,
            sound_name: String::new(),
            badge: true,
            priority: NotifPriority::Normal,
            lock_screen: LockScreenVisibility::Full,
            dnd_override: false,
            group: true,
            max_history: 50,
            total_count: 0,
            suppressed_count: 0,
        });
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Statistics
// ---------------------------------------------------------------------------

/// Returns (app_pref_count, sounds_enabled, position, dismiss_timeout, ops).
pub fn stats() -> (usize, bool, &'static str, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (
            s.app_prefs.len(),
            s.global.sounds_enabled,
            s.global.position.label(),
            s.global.dismiss_timeout_seconds,
            s.ops,
        ),
        None => (0, false, "n/a", 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

/// Run self-tests for the notification preferences module.
pub fn self_test() {
    use crate::serial_println;

    serial_println!("[notifprefs] Running self-tests...");

    *STATE.lock() = None;
    init_defaults();

    // Test 1: initial state.
    {
        let cfg = global_config().unwrap();
        assert!(cfg.show_on_lock_screen);
        assert!(cfg.sounds_enabled);
        assert_eq!(cfg.dismiss_timeout_seconds, 5);
        assert_eq!(cfg.max_visible, 5);
        assert_eq!(cfg.position, NotifPosition::TopRight);
    }
    serial_println!("[notifprefs]  1/11 initial state OK");

    // Test 2: global settings.
    {
        set_sounds_enabled(false).unwrap();
        assert!(!global_config().unwrap().sounds_enabled);
        set_sounds_enabled(true).unwrap();
        set_dismiss_timeout(10).unwrap();
        assert_eq!(global_config().unwrap().dismiss_timeout_seconds, 10);
    }
    serial_println!("[notifprefs]  2/11 global settings OK");

    // Test 3: position.
    {
        set_position(NotifPosition::BottomRight).unwrap();
        assert_eq!(global_config().unwrap().position, NotifPosition::BottomRight);
        set_position(NotifPosition::TopRight).unwrap();
    }
    serial_println!("[notifprefs]  3/11 position OK");

    // Test 4: create app pref.
    {
        set_app_enabled("firefox", true).unwrap();
        let pref = get_app_pref("firefox").unwrap();
        assert!(pref.enabled);
        assert_eq!(pref.banner_style, BannerStyle::Full);
    }
    serial_println!("[notifprefs]  4/11 create app pref OK");

    // Test 5: modify banner style.
    {
        set_banner_style("firefox", BannerStyle::Brief).unwrap();
        assert_eq!(get_app_pref("firefox").unwrap().banner_style, BannerStyle::Brief);
    }
    serial_println!("[notifprefs]  5/11 banner style OK");

    // Test 6: disable app.
    {
        set_app_enabled("firefox", false).unwrap();
        assert!(!get_app_pref("firefox").unwrap().enabled);
        assert!(!should_show("firefox"));
    }
    serial_println!("[notifprefs]  6/11 disable app OK");

    // Test 7: priority.
    {
        set_app_priority("firefox", NotifPriority::High).unwrap();
        assert_eq!(get_app_pref("firefox").unwrap().priority, NotifPriority::High);
    }
    serial_println!("[notifprefs]  7/11 priority OK");

    // Test 8: lock screen.
    {
        set_app_lock_screen("firefox", LockScreenVisibility::Hidden).unwrap();
        assert_eq!(get_app_pref("firefox").unwrap().lock_screen, LockScreenVisibility::Hidden);
    }
    serial_println!("[notifprefs]  8/11 lock screen OK");

    // Test 9: DND override.
    {
        set_dnd_override("firefox", true).unwrap();
        assert!(get_app_pref("firefox").unwrap().dnd_override);
    }
    serial_println!("[notifprefs]  9/11 DND override OK");

    // Test 10: sound check.
    {
        set_app_enabled("firefox", true).unwrap();
        set_app_sound("firefox", true).unwrap();
        assert!(should_play_sound("firefox"));
        set_app_sound("firefox", false).unwrap();
        assert!(!should_play_sound("firefox"));
    }
    serial_println!("[notifprefs] 10/11 sound check OK");

    // Test 11: remove pref.
    {
        remove_app_pref("firefox").unwrap();
        // Falls back to default.
        assert!(should_show("firefox"));
        assert!(list_app_prefs().is_empty());
    }
    serial_println!("[notifprefs] 11/11 remove pref OK");

    serial_println!("[notifprefs] All self-tests passed.");
}
