//! System Sounds — system sound event configuration.
//!
//! Manages system sound scheme: which sounds play for which events,
//! volume overrides, and sound scheme selection.
//!
//! ## Architecture
//!
//! ```text
//! System event occurs
//!   → systemsounds::play(event) → plays assigned sound
//!
//! Configuration
//!   → systemsounds::set_sound(event, path)
//!   → systemsounds::set_scheme(scheme)
//!
//! Integration:
//!   → soundevents (low-level sound playback)
//!   → soundmixer (volume control)
//!   → notifcenter (notification sounds)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// System sound event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    Startup,
    Shutdown,
    Login,
    Logout,
    LockScreen,
    UnlockScreen,
    Notification,
    Error,
    Warning,
    Information,
    DeviceConnect,
    DeviceDisconnect,
    EmptyRecycleBin,
    MessageSend,
    MessageReceive,
    Screenshot,
    VolumeChange,
    LowBattery,
}

impl SoundEvent {
    pub fn label(self) -> &'static str {
        match self {
            Self::Startup => "Startup",
            Self::Shutdown => "Shutdown",
            Self::Login => "Login",
            Self::Logout => "Logout",
            Self::LockScreen => "Lock Screen",
            Self::UnlockScreen => "Unlock Screen",
            Self::Notification => "Notification",
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Information => "Information",
            Self::DeviceConnect => "Device Connect",
            Self::DeviceDisconnect => "Device Disconnect",
            Self::EmptyRecycleBin => "Empty Recycle Bin",
            Self::MessageSend => "Message Send",
            Self::MessageReceive => "Message Receive",
            Self::Screenshot => "Screenshot",
            Self::VolumeChange => "Volume Change",
            Self::LowBattery => "Low Battery",
        }
    }
}

/// A sound assignment.
#[derive(Debug, Clone)]
pub struct SoundAssignment {
    pub event: SoundEvent,
    pub sound_path: String,
    pub enabled: bool,
    pub play_count: u64,
}

/// A sound scheme.
#[derive(Debug, Clone)]
pub struct SoundScheme {
    pub name: String,
    pub assignments: Vec<SoundAssignment>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SCHEMES: usize = 10;

struct State {
    schemes: Vec<SoundScheme>,
    active_scheme: String,
    global_enabled: bool,
    total_plays: u64,
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

fn default_assignments() -> Vec<SoundAssignment> {
    alloc::vec![
        SoundAssignment { event: SoundEvent::Startup, sound_path: String::from("/sys/sounds/startup.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::Shutdown, sound_path: String::from("/sys/sounds/shutdown.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::Notification, sound_path: String::from("/sys/sounds/notification.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::Error, sound_path: String::from("/sys/sounds/error.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::Warning, sound_path: String::from("/sys/sounds/warning.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::Information, sound_path: String::from("/sys/sounds/info.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::DeviceConnect, sound_path: String::from("/sys/sounds/device_connect.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::DeviceDisconnect, sound_path: String::from("/sys/sounds/device_disconnect.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::Screenshot, sound_path: String::from("/sys/sounds/screenshot.wav"), enabled: true, play_count: 0 },
        SoundAssignment { event: SoundEvent::LowBattery, sound_path: String::from("/sys/sounds/low_battery.wav"), enabled: true, play_count: 0 },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let default_scheme = SoundScheme {
        name: String::from("Default"),
        assignments: default_assignments(),
    };
    let silent = SoundScheme {
        name: String::from("Silent"),
        assignments: default_assignments().into_iter().map(|mut a| { a.enabled = false; a }).collect(),
    };

    *guard = Some(State {
        schemes: alloc::vec![default_scheme, silent],
        active_scheme: String::from("Default"),
        global_enabled: true,
        total_plays: 0,
        ops: 0,
    });
}

/// Play a sound event.
pub fn play(event: SoundEvent) -> KernelResult<Option<String>> {
    with_state(|state| {
        if !state.global_enabled {
            return Ok(None);
        }
        let scheme = state.schemes.iter_mut().find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;
        if let Some(assignment) = scheme.assignments.iter_mut().find(|a| a.event == event) {
            if assignment.enabled {
                assignment.play_count += 1;
                state.total_plays += 1;
                return Ok(Some(assignment.sound_path.clone()));
            }
        }
        Ok(None)
    })
}

/// Set sound for an event in the active scheme.
pub fn set_sound(event: SoundEvent, path: &str) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state.schemes.iter_mut().find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;
        if let Some(assignment) = scheme.assignments.iter_mut().find(|a| a.event == event) {
            assignment.sound_path = String::from(path);
        } else {
            scheme.assignments.push(SoundAssignment {
                event, sound_path: String::from(path),
                enabled: true, play_count: 0,
            });
        }
        Ok(())
    })
}

/// Enable/disable a sound event.
pub fn set_event_enabled(event: SoundEvent, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state.schemes.iter_mut().find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;
        if let Some(assignment) = scheme.assignments.iter_mut().find(|a| a.event == event) {
            assignment.enabled = enabled;
        }
        Ok(())
    })
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

/// Enable/disable system sounds globally.
pub fn set_global_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.global_enabled = enabled;
        Ok(())
    })
}

/// List sound schemes.
pub fn list_schemes() -> Vec<String> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.schemes.iter().map(|sc| sc.name.clone()).collect()
    })
}

/// Get active scheme name.
pub fn active_scheme() -> String {
    STATE.lock().as_ref().map_or(String::new(), |s| s.active_scheme.clone())
}

/// List assignments in active scheme.
pub fn list_assignments() -> Vec<SoundAssignment> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.schemes.iter().find(|sc| sc.name == s.active_scheme)
            .map_or(Vec::new(), |sc| sc.assignments.clone())
    })
}

/// Statistics: (scheme_count, event_count, total_plays, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let events = s.schemes.iter().find(|sc| sc.name == s.active_scheme)
                .map_or(0, |sc| sc.assignments.len());
            (s.schemes.len(), events, s.total_plays, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("systemsounds::self_test() — running tests...");
    init_defaults();

    // 1: Default scheme active.
    assert_eq!(active_scheme(), "Default");
    let schemes = list_schemes();
    assert_eq!(schemes.len(), 2);
    crate::serial_println!("  [1/8] default scheme: OK");

    // 2: Play sound.
    let path = play(SoundEvent::Notification).expect("play");
    assert!(path.is_some());
    assert!(path.unwrap().contains("notification"));
    crate::serial_println!("  [2/8] play: OK");

    // 3: Disable event.
    set_event_enabled(SoundEvent::Notification, false).expect("disable");
    let path = play(SoundEvent::Notification).expect("play2");
    assert!(path.is_none());
    set_event_enabled(SoundEvent::Notification, true).expect("enable");
    crate::serial_println!("  [3/8] disable event: OK");

    // 4: Set custom sound.
    set_sound(SoundEvent::Error, "/custom/error.wav").expect("set");
    let path = play(SoundEvent::Error).expect("play3");
    assert_eq!(path, Some(String::from("/custom/error.wav")));
    crate::serial_println!("  [4/8] custom sound: OK");

    // 5: Switch scheme.
    set_scheme("Silent").expect("scheme");
    assert_eq!(active_scheme(), "Silent");
    let path = play(SoundEvent::Startup).expect("play4");
    assert!(path.is_none()); // silent scheme
    crate::serial_println!("  [5/8] switch scheme: OK");

    // 6: Global disable.
    set_scheme("Default").expect("back");
    set_global_enabled(false).expect("global");
    let path = play(SoundEvent::Startup).expect("play5");
    assert!(path.is_none());
    set_global_enabled(true).expect("global2");
    crate::serial_println!("  [6/8] global disable: OK");

    // 7: Assignments list.
    let assignments = list_assignments();
    assert!(assignments.len() >= 10);
    crate::serial_println!("  [7/8] assignments: OK");

    // 8: Stats.
    let (schemes, events, plays, ops) = stats();
    assert_eq!(schemes, 2);
    assert!(events >= 10);
    assert!(plays >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("systemsounds::self_test() — all 8 tests passed");
}
