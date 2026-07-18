//! Sound events — system notification and UI sounds.
//!
//! Maps system events (login, logout, error, notification, etc.) to
//! audio files, with per-event volume control and sound scheme support.
//!
//! ## Architecture
//!
//! ```text
//! System event (notifcenter, sessionmgr, error, etc.)
//!   → soundevents::play(EventKind) → audio output
//!
//! Settings panel → Sounds
//!   → soundevents::set_sound(event, path)
//!   → soundevents::set_scheme(name)
//!
//! Integration:
//!   → audiodevice (audio output)
//!   → soundmixer (volume control)
//!   → notifcenter (notification sounds)
//!   → theme (sound scheme per theme)
//!   → focusassist (mute during DND)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// System event types that can have sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Login,
    Logout,
    Lock,
    Unlock,
    Startup,
    Shutdown,
    Notification,
    NotificationUrgent,
    Error,
    Warning,
    Information,
    Question,
    DeviceConnect,
    DeviceDisconnect,
    MessageReceived,
    MessageSent,
    EmptyTrash,
    ScreenCapture,
    VolumeChange,
    BatteryLow,
    ChargingStart,
}

impl EventKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Login => "Login",
            Self::Logout => "Logout",
            Self::Lock => "Lock",
            Self::Unlock => "Unlock",
            Self::Startup => "Startup",
            Self::Shutdown => "Shutdown",
            Self::Notification => "Notification",
            Self::NotificationUrgent => "Urgent Notification",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Information => "Information",
            Self::Question => "Question",
            Self::DeviceConnect => "Device Connect",
            Self::DeviceDisconnect => "Device Disconnect",
            Self::MessageReceived => "Message Received",
            Self::MessageSent => "Message Sent",
            Self::EmptyTrash => "Empty Trash",
            Self::ScreenCapture => "Screen Capture",
            Self::VolumeChange => "Volume Change",
            Self::BatteryLow => "Battery Low",
            Self::ChargingStart => "Charging Start",
        }
    }

    /// All event kinds.
    pub fn all() -> &'static [EventKind] {
        &[
            Self::Login, Self::Logout, Self::Lock, Self::Unlock,
            Self::Startup, Self::Shutdown, Self::Notification,
            Self::NotificationUrgent, Self::Error, Self::Warning,
            Self::Information, Self::Question, Self::DeviceConnect,
            Self::DeviceDisconnect, Self::MessageReceived, Self::MessageSent,
            Self::EmptyTrash, Self::ScreenCapture, Self::VolumeChange,
            Self::BatteryLow, Self::ChargingStart,
        ]
    }
}

/// A sound event mapping.
#[derive(Debug, Clone)]
pub struct SoundMapping {
    pub event: EventKind,
    /// Sound file path.
    pub sound_path: String,
    /// Relative volume (0-100).
    pub volume: u32,
    /// Whether this event's sound is enabled.
    pub enabled: bool,
}

/// A sound scheme (collection of sound mappings).
#[derive(Debug, Clone)]
pub struct SoundScheme {
    pub name: String,
    pub description: String,
    pub mappings: Vec<SoundMapping>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SCHEMES: usize = 20;

struct State {
    /// Whether system sounds are globally enabled.
    enabled: bool,
    /// Global volume (0-100).
    global_volume: u32,
    /// Active scheme name.
    active_scheme: String,
    /// Available schemes.
    schemes: Vec<SoundScheme>,
    /// Sound play count.
    total_played: u64,
    /// Muted by focus assist.
    muted: bool,
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

/// Build default scheme.
fn default_scheme() -> SoundScheme {
    let base = "/usr/share/sounds/default";
    let mappings = alloc::vec![
        SoundMapping { event: EventKind::Login, sound_path: format!("{}/login.wav", base), volume: 80, enabled: true },
        SoundMapping { event: EventKind::Logout, sound_path: format!("{}/logout.wav", base), volume: 80, enabled: true },
        SoundMapping { event: EventKind::Notification, sound_path: format!("{}/notification.wav", base), volume: 70, enabled: true },
        SoundMapping { event: EventKind::NotificationUrgent, sound_path: format!("{}/urgent.wav", base), volume: 90, enabled: true },
        SoundMapping { event: EventKind::Error, sound_path: format!("{}/error.wav", base), volume: 80, enabled: true },
        SoundMapping { event: EventKind::Warning, sound_path: format!("{}/warning.wav", base), volume: 70, enabled: true },
        SoundMapping { event: EventKind::Information, sound_path: format!("{}/info.wav", base), volume: 60, enabled: true },
        SoundMapping { event: EventKind::DeviceConnect, sound_path: format!("{}/device-added.wav", base), volume: 60, enabled: true },
        SoundMapping { event: EventKind::DeviceDisconnect, sound_path: format!("{}/device-removed.wav", base), volume: 60, enabled: true },
        SoundMapping { event: EventKind::EmptyTrash, sound_path: format!("{}/trash-empty.wav", base), volume: 50, enabled: true },
        SoundMapping { event: EventKind::ScreenCapture, sound_path: format!("{}/screen-capture.wav", base), volume: 50, enabled: true },
        SoundMapping { event: EventKind::BatteryLow, sound_path: format!("{}/battery-low.wav", base), volume: 100, enabled: true },
    ];
    SoundScheme {
        name: String::from("Default"),
        description: String::from("Default system sound scheme"),
        mappings,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let schemes = alloc::vec![
        default_scheme(),
        SoundScheme {
            name: String::from("Silent"),
            description: String::from("No sounds"),
            mappings: Vec::new(),
        },
    ];

    *guard = Some(State {
        enabled: true,
        global_volume: 80,
        active_scheme: String::from("Default"),
        schemes,
        total_played: 0,
        muted: false,
        ops: 0,
    });
}

/// Play a sound for an event (returns the sound path if played).
pub fn play(event: EventKind) -> Option<String> {
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);

    if !state.enabled || state.muted { return None; }

    // Find active scheme.
    let scheme = state.schemes.iter().find(|s| s.name == state.active_scheme)?;

    // Find mapping for this event.
    let mapping = scheme.mappings.iter().find(|m| m.event == event && m.enabled)?;

    state.total_played += 1;
    Some(mapping.sound_path.clone())
}

/// Set global sound enabled.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.enabled = enabled; Ok(()) })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.enabled)
}

/// Set global volume (0-100).
pub fn set_volume(volume: u32) -> KernelResult<()> {
    with_state(|state| { state.global_volume = volume.min(100); Ok(()) })
}

/// Set muted (by focus assist).
pub fn set_muted(muted: bool) -> KernelResult<()> {
    with_state(|state| { state.muted = muted; Ok(()) })
}

/// Set active sound scheme.
pub fn set_scheme(name: &str) -> KernelResult<()> {
    with_state(|state| {
        if !state.schemes.iter().any(|s| s.name == name) {
            return Err(KernelError::NotFound);
        }
        state.active_scheme = String::from(name);
        Ok(())
    })
}

/// Get active scheme name.
pub fn active_scheme() -> String {
    STATE.lock().as_ref().map_or(String::from("Default"), |s| s.active_scheme.clone())
}

/// Set sound for an event in the active scheme.
pub fn set_sound(event: EventKind, path: &str, volume: u32) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state.schemes.iter_mut()
            .find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;

        if let Some(m) = scheme.mappings.iter_mut().find(|m| m.event == event) {
            m.sound_path = String::from(path);
            m.volume = volume.min(100);
        } else {
            scheme.mappings.push(SoundMapping {
                event,
                sound_path: String::from(path),
                volume: volume.min(100),
                enabled: true,
            });
        }
        Ok(())
    })
}

/// Enable/disable sound for a specific event.
pub fn set_event_enabled(event: EventKind, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state.schemes.iter_mut()
            .find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;
        if let Some(m) = scheme.mappings.iter_mut().find(|m| m.event == event) {
            m.enabled = enabled;
            Ok(())
        } else {
            Err(KernelError::NotFound)
        }
    })
}

/// List available schemes.
pub fn list_schemes() -> Vec<(String, String)> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.schemes.iter().map(|sc| (sc.name.clone(), sc.description.clone())).collect()
    })
}

/// List mappings in active scheme.
pub fn list_mappings() -> Vec<SoundMapping> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| {
        s.schemes.iter().find(|sc| sc.name == s.active_scheme).map(|sc| sc.mappings.clone())
    }).unwrap_or_default()
}

/// Statistics: (scheme_count, mapping_count, total_played, enabled, muted, ops).
pub fn stats() -> (usize, usize, u64, bool, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let map_count = s.schemes.iter()
                .find(|sc| sc.name == s.active_scheme)
                .map_or(0, |sc| sc.mappings.len());
            (s.schemes.len(), map_count, s.total_played, s.enabled, s.muted, s.ops)
        }
        None => (0, 0, 0, false, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("soundevents::self_test() — running tests...");
    init_defaults();

    // 1: Enabled by default.
    assert!(is_enabled());
    crate::serial_println!("  [1/11] enabled by default: OK");

    // 2: Default scheme.
    let scheme = active_scheme();
    assert_eq!(scheme, "Default");
    crate::serial_println!("  [2/11] default scheme: OK");

    // 3: Play notification sound.
    let path = play(EventKind::Notification);
    assert!(path.is_some());
    assert!(path.expect("path").contains("notification"));
    crate::serial_println!("  [3/11] play notification: OK");

    // 4: Play error sound.
    let path = play(EventKind::Error);
    assert!(path.is_some());
    crate::serial_println!("  [4/11] play error: OK");

    // 5: Mute suppresses sounds.
    set_muted(true).expect("mute");
    let path = play(EventKind::Notification);
    assert!(path.is_none());
    set_muted(false).expect("unmute");
    crate::serial_println!("  [5/11] mute: OK");

    // 6: Switch scheme.
    set_scheme("Silent").expect("set silent");
    let path = play(EventKind::Notification);
    assert!(path.is_none()); // Silent has no mappings.
    set_scheme("Default").expect("set default back");
    crate::serial_println!("  [6/11] switch scheme: OK");

    // 7: Set sound.
    set_sound(EventKind::Login, "/custom/login.ogg", 90).expect("set sound");
    let mappings = list_mappings();
    let login = mappings.iter().find(|m| m.event == EventKind::Login).expect("find login");
    assert!(login.sound_path.contains("custom"));
    crate::serial_println!("  [7/11] set sound: OK");

    // 8: Disable event sound.
    set_event_enabled(EventKind::Login, false).expect("disable login");
    let path = play(EventKind::Login);
    assert!(path.is_none());
    set_event_enabled(EventKind::Login, true).expect("re-enable login");
    crate::serial_println!("  [8/11] disable event: OK");

    // 9: List schemes.
    let schemes = list_schemes();
    assert_eq!(schemes.len(), 2);
    crate::serial_println!("  [9/11] list schemes: OK");

    // 10: Volume.
    set_volume(50).expect("set vol");
    crate::serial_println!("  [10/11] volume: OK");

    // 11: Stats.
    let (scheme_count, map_count, played, enabled, muted, ops) = stats();
    assert_eq!(scheme_count, 2);
    assert!(map_count >= 10);
    assert!(played >= 2);
    assert!(enabled);
    assert!(!muted);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("soundevents::self_test() — all 11 tests passed");
}
