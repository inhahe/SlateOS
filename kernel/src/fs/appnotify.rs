//! Per-application notification settings.
//!
//! Controls how each application's notifications behave: sound selection,
//! notification pane visibility, and per-notification-type overrides.
//! Applications register notification types (e.g. "new-message",
//! "download-complete") and users can customise or disable each one
//! individually.
//!
//! ## Design Reference
//!
//! design.txt line 1295: "set notifications - set sound (dropdown of
//!   sounds from OS sounds directory, previewable, includes (no sound)),
//!   whether to show in notifications pane, maybe have applications
//!   register different notifications so that the user can modify or
//!   disable them individually"
//!
//! design.txt line 718: "option for any notification to not show
//!   notifications from that application again"
//!
//! design.txt line 898: "a number of notification sounds for apps to use"
//!
//! ## Architecture
//!
//! ```text
//! Application
//!   → appnotify::register_app("firefox", "Firefox")
//!   → appnotify::register_notification_type("firefox", "new-tab", ...)
//!
//! Settings panel → Per-app notifications
//!   → appnotify::list_apps()
//!   → appnotify::set_app_sound("firefox", "chime.wav")
//!   → appnotify::set_type_enabled("firefox", "new-tab", false)
//!
//! Notification daemon (notifcenter)
//!   → appnotify::effective_settings("firefox", "new-tab")
//!   → decides sound, visibility, priority based on settings
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Sound selection for notifications.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundChoice {
    /// Use the system default notification sound.
    SystemDefault,
    /// Use a specific sound file from the OS sounds directory.
    Named(String),
    /// No sound.
    Silent,
}

/// Priority override for a notification type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityOverride {
    /// Use whatever priority the app specifies.
    AppDefault,
    /// Always treat as low priority (auto-dismiss, no sound).
    ForceLow,
    /// Always treat as normal priority.
    ForceNormal,
    /// Always treat as high priority (persistent toast).
    ForceHigh,
}

/// Display mode for notification toasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Show banner toast and store in notification pane.
    BannerAndPane,
    /// Show only in the notification pane (no toast popup).
    PaneOnly,
    /// Show only as a banner toast (not stored in history).
    BannerOnly,
    /// Completely suppressed — not shown at all.
    Suppressed,
}

/// Per-notification-type settings registered by an application.
#[derive(Debug, Clone)]
pub struct NotificationType {
    /// Machine-readable type key (e.g. "new-message").
    pub type_key: String,
    /// Human-readable description (e.g. "New message received").
    pub description: String,
    /// Default sound for this type.
    pub default_sound: SoundChoice,
    /// Whether this type is enabled (user can disable).
    pub enabled: bool,
    /// User-overridden sound (None = use app default).
    pub user_sound: Option<SoundChoice>,
    /// Priority override.
    pub priority: PriorityOverride,
    /// Display mode override.
    pub display_mode: DisplayMode,
}

/// Per-application notification settings.
#[derive(Debug, Clone)]
pub struct AppNotifyConfig {
    /// Application identifier (e.g. package name or binary name).
    pub app_id: String,
    /// Human-readable display name.
    pub display_name: String,
    /// Whether notifications from this app are globally enabled.
    pub enabled: bool,
    /// App-level sound choice.
    pub sound: SoundChoice,
    /// App-level display mode.
    pub display_mode: DisplayMode,
    /// Per-notification-type settings.
    pub notification_types: Vec<NotificationType>,
    /// Whether this app can send critical (undismissable) notifications.
    pub allow_critical: bool,
    /// Maximum notifications per minute (0 = unlimited).
    pub rate_limit: u32,
    /// Whether to group multiple notifications from this app.
    pub group_notifications: bool,
}

/// Available system sounds.
#[derive(Debug, Clone)]
pub struct SystemSound {
    /// Sound file name (e.g. "chime.wav").
    pub filename: String,
    /// Human-readable label (e.g. "Chime").
    pub label: String,
    /// Category (e.g. "notification", "alert", "message").
    pub category: String,
}

/// Effective notification settings resolved for a specific notification.
#[derive(Debug, Clone)]
pub struct EffectiveSettings {
    /// Whether this notification should be shown at all.
    pub show: bool,
    /// The sound to play (None = silent).
    pub sound: Option<String>,
    /// The display mode.
    pub display_mode: DisplayMode,
    /// The effective priority override.
    pub priority: PriorityOverride,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_APPS: usize = 256;
const MAX_TYPES_PER_APP: usize = 64;
const MAX_SOUNDS: usize = 128;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    apps: Vec<AppNotifyConfig>,
    sounds: Vec<SystemSound>,
    changes: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    apps: Vec::new(),
    sounds: Vec::new(),
    changes: 0,
});

static OP_COUNT: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// Application registration
// ---------------------------------------------------------------------------

/// Register an application for notification settings.
pub fn register_app(app_id: &str, display_name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.apps.len() >= MAX_APPS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.apps.iter().any(|a| a.app_id == app_id) {
        return Err(KernelError::AlreadyExists);
    }
    state.apps.push(AppNotifyConfig {
        app_id: String::from(app_id),
        display_name: String::from(display_name),
        enabled: true,
        sound: SoundChoice::SystemDefault,
        display_mode: DisplayMode::BannerAndPane,
        notification_types: Vec::new(),
        allow_critical: false,
        rate_limit: 0,
        group_notifications: true,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Unregister an application.
pub fn unregister_app(app_id: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let idx = state.apps.iter().position(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    state.apps.remove(idx);
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Get settings for an application.
pub fn get_app(app_id: &str) -> KernelResult<AppNotifyConfig> {
    let state = STATE.lock();
    state.apps.iter().find(|a| a.app_id == app_id)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all registered applications.
pub fn list_apps() -> Vec<AppNotifyConfig> {
    STATE.lock().apps.clone()
}

// ---------------------------------------------------------------------------
// App-level settings
// ---------------------------------------------------------------------------

/// Enable or disable all notifications from an app.
pub fn set_app_enabled(app_id: &str, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.enabled = enabled;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set app-level sound.
pub fn set_app_sound(app_id: &str, sound: SoundChoice) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.sound = sound;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set app-level display mode.
pub fn set_app_display_mode(app_id: &str, mode: DisplayMode) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.display_mode = mode;
    state.changes += 1;
    Ok(())
}

/// Set whether the app can send critical notifications.
pub fn set_allow_critical(app_id: &str, allow: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.allow_critical = allow;
    state.changes += 1;
    Ok(())
}

/// Set rate limit for an app (0 = unlimited).
pub fn set_rate_limit(app_id: &str, per_minute: u32) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.rate_limit = per_minute;
    state.changes += 1;
    Ok(())
}

/// Set whether to group notifications from an app.
pub fn set_group(app_id: &str, group: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    app.group_notifications = group;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// Notification type registration
// ---------------------------------------------------------------------------

/// Register a notification type for an app.
pub fn register_notification_type(
    app_id: &str,
    type_key: &str,
    description: &str,
    default_sound: SoundChoice,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    if app.notification_types.len() >= MAX_TYPES_PER_APP {
        return Err(KernelError::ResourceExhausted);
    }
    if app.notification_types.iter().any(|t| t.type_key == type_key) {
        return Err(KernelError::AlreadyExists);
    }
    app.notification_types.push(NotificationType {
        type_key: String::from(type_key),
        description: String::from(description),
        default_sound,
        enabled: true,
        user_sound: None,
        priority: PriorityOverride::AppDefault,
        display_mode: DisplayMode::BannerAndPane,
    });
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Unregister a notification type.
pub fn unregister_notification_type(app_id: &str, type_key: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let idx = app.notification_types.iter().position(|t| t.type_key == type_key)
        .ok_or(KernelError::NotFound)?;
    app.notification_types.remove(idx);
    state.changes += 1;
    Ok(())
}

/// Enable or disable a specific notification type.
pub fn set_type_enabled(app_id: &str, type_key: &str, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let ntype = app.notification_types.iter_mut().find(|t| t.type_key == type_key)
        .ok_or(KernelError::NotFound)?;
    ntype.enabled = enabled;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set user sound override for a notification type.
pub fn set_type_sound(
    app_id: &str,
    type_key: &str,
    sound: Option<SoundChoice>,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let ntype = app.notification_types.iter_mut().find(|t| t.type_key == type_key)
        .ok_or(KernelError::NotFound)?;
    ntype.user_sound = sound;
    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

/// Set priority override for a notification type.
pub fn set_type_priority(
    app_id: &str,
    type_key: &str,
    priority: PriorityOverride,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let ntype = app.notification_types.iter_mut().find(|t| t.type_key == type_key)
        .ok_or(KernelError::NotFound)?;
    ntype.priority = priority;
    state.changes += 1;
    Ok(())
}

/// Set display mode for a notification type.
pub fn set_type_display(
    app_id: &str,
    type_key: &str,
    mode: DisplayMode,
) -> KernelResult<()> {
    let mut state = STATE.lock();
    let app = state.apps.iter_mut().find(|a| a.app_id == app_id)
        .ok_or(KernelError::NotFound)?;
    let ntype = app.notification_types.iter_mut().find(|t| t.type_key == type_key)
        .ok_or(KernelError::NotFound)?;
    ntype.display_mode = mode;
    state.changes += 1;
    Ok(())
}

// ---------------------------------------------------------------------------
// System sounds
// ---------------------------------------------------------------------------

/// Register a system sound.
pub fn register_sound(filename: &str, label: &str, category: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    if state.sounds.len() >= MAX_SOUNDS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.sounds.iter().any(|s| s.filename == filename) {
        return Err(KernelError::AlreadyExists);
    }
    state.sounds.push(SystemSound {
        filename: String::from(filename),
        label: String::from(label),
        category: String::from(category),
    });
    state.changes += 1;
    Ok(())
}

/// List available system sounds.
pub fn list_sounds() -> Vec<SystemSound> {
    STATE.lock().sounds.clone()
}

/// List sounds filtered by category.
pub fn sounds_by_category(category: &str) -> Vec<SystemSound> {
    STATE.lock().sounds.iter()
        .filter(|s| s.category == category)
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// Resolution — effective settings for a notification
// ---------------------------------------------------------------------------

/// Resolve effective settings for a specific notification.
///
/// Applies the cascade: app enabled → type enabled → sound/display overrides.
pub fn effective_settings(app_id: &str, type_key: &str) -> EffectiveSettings {
    let state = STATE.lock();
    let Some(app) = state.apps.iter().find(|a| a.app_id == app_id) else {
        // Unknown app — use permissive defaults.
        return EffectiveSettings {
            show: true,
            sound: None,
            display_mode: DisplayMode::BannerAndPane,
            priority: PriorityOverride::AppDefault,
        };
    };

    // App globally disabled.
    if !app.enabled {
        return EffectiveSettings {
            show: false,
            sound: None,
            display_mode: DisplayMode::Suppressed,
            priority: PriorityOverride::AppDefault,
        };
    }

    // Find specific notification type.
    let ntype = app.notification_types.iter().find(|t| t.type_key == type_key);

    if let Some(ntype) = ntype {
        if !ntype.enabled {
            return EffectiveSettings {
                show: false,
                sound: None,
                display_mode: DisplayMode::Suppressed,
                priority: ntype.priority,
            };
        }

        // Resolve sound: type user override → type default → app sound → system default.
        let sound = match &ntype.user_sound {
            Some(SoundChoice::Named(name)) => Some(name.clone()),
            Some(SoundChoice::Silent) => None,
            Some(SoundChoice::SystemDefault) | None => {
                match &ntype.default_sound {
                    SoundChoice::Named(name) => Some(name.clone()),
                    SoundChoice::Silent => None,
                    SoundChoice::SystemDefault => {
                        match &app.sound {
                            SoundChoice::Named(name) => Some(name.clone()),
                            SoundChoice::Silent => None,
                            SoundChoice::SystemDefault => None, // Caller uses system default.
                        }
                    }
                }
            }
        };

        // Display mode: type-level overrides app-level.
        let display_mode = ntype.display_mode;

        EffectiveSettings {
            show: true,
            sound,
            display_mode,
            priority: ntype.priority,
        }
    } else {
        // Unknown type — use app-level defaults.
        let sound = match &app.sound {
            SoundChoice::Named(name) => Some(name.clone()),
            SoundChoice::Silent => None,
            SoundChoice::SystemDefault => None,
        };

        EffectiveSettings {
            show: true,
            sound,
            display_mode: app.display_mode,
            priority: PriorityOverride::AppDefault,
        }
    }
}

// ---------------------------------------------------------------------------
// Init / stats
// ---------------------------------------------------------------------------

/// Initialise with default system sounds and example apps.
pub fn init_defaults() {
    let mut state = STATE.lock();

    // System notification sounds (per design.txt line 898).
    state.sounds = vec![
        SystemSound {
            filename: String::from("chime.wav"),
            label: String::from("Chime"),
            category: String::from("notification"),
        },
        SystemSound {
            filename: String::from("ding.wav"),
            label: String::from("Ding"),
            category: String::from("notification"),
        },
        SystemSound {
            filename: String::from("pop.wav"),
            label: String::from("Pop"),
            category: String::from("notification"),
        },
        SystemSound {
            filename: String::from("bubble.wav"),
            label: String::from("Bubble"),
            category: String::from("notification"),
        },
        SystemSound {
            filename: String::from("alert.wav"),
            label: String::from("Alert"),
            category: String::from("alert"),
        },
        SystemSound {
            filename: String::from("warning.wav"),
            label: String::from("Warning"),
            category: String::from("alert"),
        },
        SystemSound {
            filename: String::from("error.wav"),
            label: String::from("Error"),
            category: String::from("alert"),
        },
        SystemSound {
            filename: String::from("message.wav"),
            label: String::from("Message"),
            category: String::from("message"),
        },
        SystemSound {
            filename: String::from("sent.wav"),
            label: String::from("Sent"),
            category: String::from("message"),
        },
        SystemSound {
            filename: String::from("complete.wav"),
            label: String::from("Complete"),
            category: String::from("system"),
        },
        SystemSound {
            filename: String::from("startup.wav"),
            label: String::from("Startup"),
            category: String::from("system"),
        },
        SystemSound {
            filename: String::from("shutdown.wav"),
            label: String::from("Shutdown"),
            category: String::from("system"),
        },
    ];

    // Example app registrations.
    state.apps = vec![
        AppNotifyConfig {
            app_id: String::from("system"),
            display_name: String::from("System"),
            enabled: true,
            sound: SoundChoice::SystemDefault,
            display_mode: DisplayMode::BannerAndPane,
            notification_types: vec![
                NotificationType {
                    type_key: String::from("update-available"),
                    description: String::from("OS updates available"),
                    default_sound: SoundChoice::Named(String::from("ding.wav")),
                    enabled: true,
                    user_sound: None,
                    priority: PriorityOverride::AppDefault,
                    display_mode: DisplayMode::BannerAndPane,
                },
                NotificationType {
                    type_key: String::from("low-battery"),
                    description: String::from("Battery level low"),
                    default_sound: SoundChoice::Named(String::from("warning.wav")),
                    enabled: true,
                    user_sound: None,
                    priority: PriorityOverride::ForceHigh,
                    display_mode: DisplayMode::BannerAndPane,
                },
                NotificationType {
                    type_key: String::from("disk-full"),
                    description: String::from("Disk space running low"),
                    default_sound: SoundChoice::Named(String::from("alert.wav")),
                    enabled: true,
                    user_sound: None,
                    priority: PriorityOverride::ForceHigh,
                    display_mode: DisplayMode::BannerAndPane,
                },
            ],
            allow_critical: true,
            rate_limit: 0,
            group_notifications: false,
        },
        AppNotifyConfig {
            app_id: String::from("file-explorer"),
            display_name: String::from("File Explorer"),
            enabled: true,
            sound: SoundChoice::Named(String::from("pop.wav")),
            display_mode: DisplayMode::BannerAndPane,
            notification_types: vec![
                NotificationType {
                    type_key: String::from("copy-complete"),
                    description: String::from("File copy completed"),
                    default_sound: SoundChoice::Named(String::from("complete.wav")),
                    enabled: true,
                    user_sound: None,
                    priority: PriorityOverride::AppDefault,
                    display_mode: DisplayMode::BannerAndPane,
                },
                NotificationType {
                    type_key: String::from("trash-full"),
                    description: String::from("Recycle bin is full"),
                    default_sound: SoundChoice::Silent,
                    enabled: true,
                    user_sound: None,
                    priority: PriorityOverride::ForceLow,
                    display_mode: DisplayMode::PaneOnly,
                },
            ],
            allow_critical: false,
            rate_limit: 10,
            group_notifications: true,
        },
    ];

    state.changes += 1;
    OP_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Return (app_count, sound_count, total_types, ops).
pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    let total_types: usize = state.apps.iter()
        .map(|a| a.notification_types.len())
        .sum();
    (state.apps.len(),
     state.sounds.len(),
     total_types,
     OP_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() {
    OP_COUNT.store(0, Ordering::Relaxed);
}

pub fn clear_all() {
    let mut state = STATE.lock();
    state.apps.clear();
    state.sounds.clear();
    state.changes = 0;
    OP_COUNT.store(0, Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    clear_all();

    // Test 1: register app.
    serial_println!("appnotify::self_test 1: register app");
    register_app("test-app", "Test App")?;
    let app = get_app("test-app")?;
    assert!(app.enabled);
    assert_eq!(app.display_name, "Test App");

    // Test 2: duplicate registration.
    serial_println!("appnotify::self_test 2: duplicate");
    assert!(register_app("test-app", "Dup").is_err());

    // Test 3: register notification type.
    serial_println!("appnotify::self_test 3: notification type");
    register_notification_type(
        "test-app",
        "new-msg",
        "New message",
        SoundChoice::Named(String::from("chime.wav")),
    )?;
    let app = get_app("test-app")?;
    assert_eq!(app.notification_types.len(), 1);
    assert_eq!(app.notification_types[0].type_key, "new-msg");

    // Test 4: effective settings — enabled.
    serial_println!("appnotify::self_test 4: effective enabled");
    let eff = effective_settings("test-app", "new-msg");
    assert!(eff.show);
    assert_eq!(eff.sound, Some(String::from("chime.wav")));

    // Test 5: disable type → suppressed.
    serial_println!("appnotify::self_test 5: disable type");
    set_type_enabled("test-app", "new-msg", false)?;
    let eff = effective_settings("test-app", "new-msg");
    assert!(!eff.show);

    // Test 6: disable app → all suppressed.
    serial_println!("appnotify::self_test 6: disable app");
    set_type_enabled("test-app", "new-msg", true)?;
    set_app_enabled("test-app", false)?;
    let eff = effective_settings("test-app", "new-msg");
    assert!(!eff.show);

    // Test 7: user sound override.
    serial_println!("appnotify::self_test 7: sound override");
    set_app_enabled("test-app", true)?;
    set_type_sound("test-app", "new-msg", Some(SoundChoice::Named(String::from("ding.wav"))))?;
    let eff = effective_settings("test-app", "new-msg");
    assert_eq!(eff.sound, Some(String::from("ding.wav")));

    // Test 8: silent override.
    serial_println!("appnotify::self_test 8: silent override");
    set_type_sound("test-app", "new-msg", Some(SoundChoice::Silent))?;
    let eff = effective_settings("test-app", "new-msg");
    assert!(eff.sound.is_none());

    // Test 9: system sounds.
    serial_println!("appnotify::self_test 9: sounds");
    register_sound("test.wav", "Test", "notification")?;
    register_sound("test2.wav", "Test2", "alert")?;
    let sounds = list_sounds();
    assert_eq!(sounds.len(), 2);
    let notif = sounds_by_category("notification");
    assert_eq!(notif.len(), 1);

    // Test 10: unregister.
    serial_println!("appnotify::self_test 10: unregister");
    unregister_notification_type("test-app", "new-msg")?;
    let app = get_app("test-app")?;
    assert!(app.notification_types.is_empty());
    unregister_app("test-app")?;
    assert!(get_app("test-app").is_err());

    // Test 11: init_defaults.
    serial_println!("appnotify::self_test 11: defaults");
    init_defaults();
    let apps = list_apps();
    assert!(apps.len() >= 2);
    let sounds = list_sounds();
    assert!(sounds.len() >= 10);

    clear_all();
    serial_println!("appnotify::self_test: all 11 tests passed");
    Ok(())
}
