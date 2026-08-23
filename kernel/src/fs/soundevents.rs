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

use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
            Self::Login,
            Self::Logout,
            Self::Lock,
            Self::Unlock,
            Self::Startup,
            Self::Shutdown,
            Self::Notification,
            Self::NotificationUrgent,
            Self::Error,
            Self::Warning,
            Self::Information,
            Self::Question,
            Self::DeviceConnect,
            Self::DeviceDisconnect,
            Self::MessageReceived,
            Self::MessageSent,
            Self::EmptyTrash,
            Self::ScreenCapture,
            Self::VolumeChange,
            Self::BatteryLow,
            Self::ChargingStart,
        ]
    }
}

/// A sound event mapping.
#[derive(Debug, Clone)]
pub struct SoundMapping {
    pub event: EventKind,
    /// Sound file path.
    ///
    /// `PathBuf`, not `String`: our filesystem forbids only `/` and NUL in a
    /// name, so a `String` cannot hold every legal path, and the loss is
    /// silent.  A lossily decoded spelling does not name a missing file, it
    /// names a *different* one -- so the event plays nothing, or plays
    /// whatever happens to sit at the mangled name, while the settings listing
    /// still shows the file the user chose.  See design-decisions.md §261.
    pub sound_path: PathBuf,
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
    let base = Path::new("/usr/share/sounds/default");
    let mappings = alloc::vec![
        SoundMapping {
            event: EventKind::Login,
            sound_path: base.join("login.wav"),
            volume: 80,
            enabled: true
        },
        SoundMapping {
            event: EventKind::Logout,
            sound_path: base.join("logout.wav"),
            volume: 80,
            enabled: true
        },
        SoundMapping {
            event: EventKind::Notification,
            sound_path: base.join("notification.wav"),
            volume: 70,
            enabled: true
        },
        SoundMapping {
            event: EventKind::NotificationUrgent,
            sound_path: base.join("urgent.wav"),
            volume: 90,
            enabled: true
        },
        SoundMapping {
            event: EventKind::Error,
            sound_path: base.join("error.wav"),
            volume: 80,
            enabled: true
        },
        SoundMapping {
            event: EventKind::Warning,
            sound_path: base.join("warning.wav"),
            volume: 70,
            enabled: true
        },
        SoundMapping {
            event: EventKind::Information,
            sound_path: base.join("info.wav"),
            volume: 60,
            enabled: true
        },
        SoundMapping {
            event: EventKind::DeviceConnect,
            sound_path: base.join("device-added.wav"),
            volume: 60,
            enabled: true
        },
        SoundMapping {
            event: EventKind::DeviceDisconnect,
            sound_path: base.join("device-removed.wav"),
            volume: 60,
            enabled: true
        },
        SoundMapping {
            event: EventKind::EmptyTrash,
            sound_path: base.join("trash-empty.wav"),
            volume: 50,
            enabled: true
        },
        SoundMapping {
            event: EventKind::ScreenCapture,
            sound_path: base.join("screen-capture.wav"),
            volume: 50,
            enabled: true
        },
        SoundMapping {
            event: EventKind::BatteryLow,
            sound_path: base.join("battery-low.wav"),
            volume: 100,
            enabled: true
        },
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
    if guard.is_some() {
        return;
    }

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
pub fn play(event: EventKind) -> Option<PathBuf> {
    let mut guard = STATE.lock();
    let state = guard.as_mut()?;
    state.ops += 1;
    OPS.store(state.ops, Ordering::Relaxed);

    if !state.enabled || state.muted {
        return None;
    }

    // Find active scheme.
    let scheme = state
        .schemes
        .iter()
        .find(|s| s.name == state.active_scheme)?;

    // Find mapping for this event.
    let mapping = scheme
        .mappings
        .iter()
        .find(|m| m.event == event && m.enabled)?;

    state.total_played += 1;
    Some(mapping.sound_path.clone())
}

/// Set global sound enabled.
pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.enabled = enabled;
        Ok(())
    })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.enabled)
}

/// Set global volume (0-100).
pub fn set_volume(volume: u32) -> KernelResult<()> {
    with_state(|state| {
        state.global_volume = volume.min(100);
        Ok(())
    })
}

/// Set muted (by focus assist).
pub fn set_muted(muted: bool) -> KernelResult<()> {
    with_state(|state| {
        state.muted = muted;
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

/// Get active scheme name.
pub fn active_scheme() -> String {
    STATE
        .lock()
        .as_ref()
        .map_or(String::from("Default"), |s| s.active_scheme.clone())
}

/// Set sound for an event in the active scheme.
pub fn set_sound<P: AsRef<Path>>(event: EventKind, path: P, volume: u32) -> KernelResult<()> {
    let path = path.as_ref();
    // An empty path names nothing, and at playback time an event mapped to
    // nothing is indistinguishable from one whose file has been deleted.
    // `set_event_enabled(event, false)` is how a caller asks for silence.
    if path.as_bytes().is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let scheme = state
            .schemes
            .iter_mut()
            .find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;

        if let Some(m) = scheme.mappings.iter_mut().find(|m| m.event == event) {
            m.sound_path = path.to_path_buf();
            m.volume = volume.min(100);
        } else {
            scheme.mappings.push(SoundMapping {
                event,
                sound_path: path.to_path_buf(),
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
        let scheme = state
            .schemes
            .iter_mut()
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
        s.schemes
            .iter()
            .map(|sc| (sc.name.clone(), sc.description.clone()))
            .collect()
    })
}

/// List mappings in active scheme.
pub fn list_mappings() -> Vec<SoundMapping> {
    let guard = STATE.lock();
    guard
        .as_ref()
        .and_then(|s| {
            s.schemes
                .iter()
                .find(|sc| sc.name == s.active_scheme)
                .map(|sc| sc.mappings.clone())
        })
        .unwrap_or_default()
}

/// Statistics: (scheme_count, mapping_count, total_played, enabled, muted, ops).
pub fn stats() -> (usize, usize, u64, bool, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let map_count = s
                .schemes
                .iter()
                .find(|sc| sc.name == s.active_scheme)
                .map_or(0, |sc| sc.mappings.len());
            (
                s.schemes.len(),
                map_count,
                s.total_played,
                s.enabled,
                s.muted,
                s.ops,
            )
        }
        None => (0, 0, 0, false, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Run the module's self-test suite against a table of its own.
///
/// The suite mutates module state and asserts exact contents, and it used to
/// do that to the *live* table -- which, since it is also a kernel-shell
/// subcommand, changed or destroyed whatever the user had here and then
/// reported success.  The live state is moved aside for the duration and put
/// back afterwards; `crate::fs::selftest` records why this shape rather than
/// the alternatives.
///
/// The pristine value is `None` rather than a table: this module initialises
/// lazily, and `None` is exactly what a fresh boot holds.
pub fn self_test() {
    // `OPS` is a lock-free mirror of `state.ops`, which lives *inside* the
    // table. `with_pristine` restores the table and so restores `state.ops`,
    // but it cannot know about the mirror -- leave it and the two disagree
    // permanently, with `<module> stats` reporting the suite's activity as
    // the user's.
    let saved_ops = OPS.load(Ordering::Relaxed);
    crate::fs::selftest::with_pristine(&STATE, None, self_test_inner);
    OPS.store(saved_ops, Ordering::Relaxed);
}

fn self_test_inner() {
    crate::serial_println!("soundevents::self_test() — running tests...");
    init_defaults();

    // 1: Enabled by default.
    assert!(is_enabled());
    crate::serial_println!("  [1/12] enabled by default: OK");

    // 2: Default scheme.
    let scheme = active_scheme();
    assert_eq!(scheme, "Default");
    crate::serial_println!("  [2/12] default scheme: OK");

    // 3: Play notification sound.
    let path = play(EventKind::Notification);
    assert_eq!(
        path.as_deref().and_then(Path::file_name),
        Some(Path::new("notification.wav"))
    );
    crate::serial_println!("  [3/12] play notification: OK");

    // 4: Play error sound.
    let path = play(EventKind::Error);
    assert!(path.is_some());
    crate::serial_println!("  [4/12] play error: OK");

    // 5: Mute suppresses sounds.
    set_muted(true).expect("mute");
    let path = play(EventKind::Notification);
    assert!(path.is_none());
    set_muted(false).expect("unmute");
    crate::serial_println!("  [5/12] mute: OK");

    // 6: Switch scheme.
    set_scheme("Silent").expect("set silent");
    let path = play(EventKind::Notification);
    assert!(path.is_none()); // Silent has no mappings.
    set_scheme("Default").expect("set default back");
    crate::serial_println!("  [6/12] switch scheme: OK");

    // 7: Set sound.
    set_sound(EventKind::Login, "/custom/login.ogg", 90).expect("set sound");
    let mappings = list_mappings();
    let login = mappings
        .iter()
        .find(|m| m.event == EventKind::Login)
        .expect("find login");
    assert_eq!(login.sound_path, PathBuf::from("/custom/login.ogg"));
    // An empty path names nothing; `set_event_enabled(_, false)` is the way
    // to ask for silence.
    assert!(set_sound(EventKind::Login, "", 90).is_err());
    crate::serial_println!("  [7/12] set sound: OK");

    // 8: Disable event sound.
    set_event_enabled(EventKind::Login, false).expect("disable login");
    let path = play(EventKind::Login);
    assert!(path.is_none());
    set_event_enabled(EventKind::Login, true).expect("re-enable login");
    crate::serial_println!("  [8/12] disable event: OK");

    // 9: List schemes.
    let schemes = list_schemes();
    assert_eq!(schemes.len(), 2);
    crate::serial_println!("  [9/12] list schemes: OK");

    // 10: Volume.
    set_volume(50).expect("set vol");
    crate::serial_println!("  [10/12] volume: OK");

    // 11: Stats.
    //
    // `total_played` counts sounds actually emitted, not `play()` calls: of the
    // five calls above, only tests 3 and 4 reach a mapping that is enabled in
    // an unmuted scheme.  Tests 5, 6 and 8 exist precisely to check that
    // nothing is emitted.  Assert the exact value rather than a bound -- it is
    // deterministic, and an equality also catches the counter *over*-counting,
    // which no `>=` would.  (systemsounds asserted an impossible `>= 3` here
    // for the same counter and nobody noticed until its suite first ran at
    // boot; see known-issues.md ->
    // FIXED-A-SYSTEMSOUNDS-A-SUITE-ASSERTED-A-PLAY-COUNT-IT-COULD-NOT-REACH.)
    let (scheme_count, map_count, played, enabled, muted, ops) = stats();
    assert_eq!(scheme_count, 2);
    assert!(map_count >= 10);
    assert_eq!(played, 2, "two of the five play() calls are audible");
    assert!(enabled);
    assert!(!muted);
    assert!(ops > 0);
    crate::serial_println!("  [11/12] stats: OK");

    // 12: A sound path that is not valid UTF-8 survives round-trip.
    //
    // Runs last on purpose: it plays a sound, and test 11 asserts an exact
    // play count.
    //
    // 0xFF is not a valid UTF-8 byte in any position, so a `String`-backed
    // field could only ever have stored it by replacing it, and the mapping
    // would then name a different file.  The 0xFE sibling is the half that
    // matters: it proves the two spellings stay distinct, which a lossy
    // conversion would not, since both collapse to the same replacement
    // character.
    let weird = PathBuf::from(&b"/usr/share/sounds/\xFF-login.wav"[..]);
    let sibling = PathBuf::from(&b"/usr/share/sounds/\xFE-login.wav"[..]);
    assert_ne!(weird, sibling);
    set_sound(EventKind::Login, &weird, 70).expect("set non-UTF-8 path");
    assert_eq!(play(EventKind::Login).as_ref(), Some(&weird));
    let listed = list_mappings();
    let login = listed
        .iter()
        .find(|m| m.event == EventKind::Login)
        .expect("login mapping");
    assert_eq!(login.sound_path, weird);
    assert_ne!(login.sound_path, sibling);
    crate::serial_println!("  [12/12] non-UTF-8 sound path: OK");

    crate::serial_println!("soundevents::self_test() — all 12 tests passed");
}
