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

use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    /// The sound file to play for this event.
    ///
    /// `PathBuf`, not `String`: our filesystem forbids only `/` and NUL in a
    /// name, so a `String` cannot hold every legal path, and the loss is
    /// silent. See design-decisions.md §261.
    ///
    /// The consequence here is quieter than for a home directory but is the
    /// same mistake: a lossily decoded spelling does not name a missing file,
    /// it names a *different* one. The user picks a sound, the assignment
    /// stores a mangled path, and the event thereafter plays either nothing or
    /// whatever else happens to sit at the mangled name -- with the settings
    /// UI still displaying the file they chose.
    pub sound_path: PathBuf,
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
        SoundAssignment {
            event: SoundEvent::Startup,
            sound_path: PathBuf::from("/sys/sounds/startup.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::Shutdown,
            sound_path: PathBuf::from("/sys/sounds/shutdown.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::Notification,
            sound_path: PathBuf::from("/sys/sounds/notification.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::Error,
            sound_path: PathBuf::from("/sys/sounds/error.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::Warning,
            sound_path: PathBuf::from("/sys/sounds/warning.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::Information,
            sound_path: PathBuf::from("/sys/sounds/info.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::DeviceConnect,
            sound_path: PathBuf::from("/sys/sounds/device_connect.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::DeviceDisconnect,
            sound_path: PathBuf::from("/sys/sounds/device_disconnect.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::Screenshot,
            sound_path: PathBuf::from("/sys/sounds/screenshot.wav"),
            enabled: true,
            play_count: 0
        },
        SoundAssignment {
            event: SoundEvent::LowBattery,
            sound_path: PathBuf::from("/sys/sounds/low_battery.wav"),
            enabled: true,
            play_count: 0
        },
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    let default_scheme = SoundScheme {
        name: String::from("Default"),
        assignments: default_assignments(),
    };
    let silent = SoundScheme {
        name: String::from("Silent"),
        assignments: default_assignments()
            .into_iter()
            .map(|mut a| {
                a.enabled = false;
                a
            })
            .collect(),
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
///
/// Returns the path of the sound that was played, or `None` when nothing was:
/// system sounds are off globally, the active scheme has no assignment for this
/// event, or the assignment is disabled. Those three are deliberately not
/// distinguished — every one of them means "stay silent", and a caller that
/// treated them differently would be inventing policy the user did not set.
pub fn play(event: SoundEvent) -> KernelResult<Option<PathBuf>> {
    with_state(|state| {
        if !state.global_enabled {
            return Ok(None);
        }
        let scheme = state
            .schemes
            .iter_mut()
            .find(|s| s.name == state.active_scheme)
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
///
/// Takes `impl AsRef<Path>` so a caller holding raw bytes from the shell can
/// pass them through without a lossy trip via `str`.
///
/// An empty path is rejected rather than stored: it names nothing, and an
/// assignment pointing at nothing is indistinguishable at playback time from
/// one pointing at a file that has since been deleted. Callers that want the
/// event to be silent have `set_event_enabled(event, false)`, which says so.
pub fn set_sound<P: AsRef<Path>>(event: SoundEvent, path: P) -> KernelResult<()> {
    let path = path.as_ref();
    if path.as_bytes().is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let scheme = state
            .schemes
            .iter_mut()
            .find(|s| s.name == state.active_scheme)
            .ok_or(KernelError::NotFound)?;
        if let Some(assignment) = scheme.assignments.iter_mut().find(|a| a.event == event) {
            assignment.sound_path = path.to_path_buf();
        } else {
            scheme.assignments.push(SoundAssignment {
                event,
                sound_path: path.to_path_buf(),
                enabled: true,
                play_count: 0,
            });
        }
        Ok(())
    })
}

/// Enable/disable a sound event.
pub fn set_event_enabled(event: SoundEvent, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        let scheme = state
            .schemes
            .iter_mut()
            .find(|s| s.name == state.active_scheme)
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
    STATE
        .lock()
        .as_ref()
        .map_or(String::new(), |s| s.active_scheme.clone())
}

/// List assignments in active scheme.
pub fn list_assignments() -> Vec<SoundAssignment> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.schemes
            .iter()
            .find(|sc| sc.name == s.active_scheme)
            .map_or(Vec::new(), |sc| sc.assignments.clone())
    })
}

/// Statistics: (scheme_count, event_count, total_plays, ops).
pub fn stats() -> (usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let events = s
                .schemes
                .iter()
                .find(|sc| sc.name == s.active_scheme)
                .map_or(0, |sc| sc.assignments.len());
            (s.schemes.len(), events, s.total_plays, s.ops)
        }
        None => (0, 0, 0, 0),
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
    crate::serial_println!("systemsounds::self_test() — running tests...");
    init_defaults();

    // 1: Default scheme active.
    assert_eq!(active_scheme(), "Default");
    let schemes = list_schemes();
    assert_eq!(schemes.len(), 2);
    crate::serial_println!("  [1/9] default scheme: OK");

    // 2: Play sound.
    let path = play(SoundEvent::Notification).expect("play");
    assert_eq!(
        path.as_deref().and_then(Path::file_name),
        Some(Path::new("notification.wav"))
    );
    crate::serial_println!("  [2/9] play: OK");

    // 3: Disable event.
    set_event_enabled(SoundEvent::Notification, false).expect("disable");
    let path = play(SoundEvent::Notification).expect("play2");
    assert!(path.is_none());
    set_event_enabled(SoundEvent::Notification, true).expect("enable");
    crate::serial_println!("  [3/9] disable event: OK");

    // 4: Set custom sound.
    set_sound(SoundEvent::Error, "/custom/error.wav").expect("set");
    let path = play(SoundEvent::Error).expect("play3");
    assert_eq!(path, Some(PathBuf::from("/custom/error.wav")));
    // An empty path names nothing; `set_event_enabled(_, false)` is how a
    // caller asks for silence.
    assert!(set_sound(SoundEvent::Error, "").is_err());
    crate::serial_println!("  [4/9] custom sound: OK");

    // 5: Switch scheme.
    set_scheme("Silent").expect("scheme");
    assert_eq!(active_scheme(), "Silent");
    let path = play(SoundEvent::Startup).expect("play4");
    assert!(path.is_none()); // silent scheme
    crate::serial_println!("  [5/9] switch scheme: OK");

    // 6: Global disable.
    set_scheme("Default").expect("back");
    set_global_enabled(false).expect("global");
    let path = play(SoundEvent::Startup).expect("play5");
    assert!(path.is_none());
    set_global_enabled(true).expect("global2");
    crate::serial_println!("  [6/9] global disable: OK");

    // 7: Assignments list.
    let assignments = list_assignments();
    assert!(assignments.len() >= 10);
    crate::serial_println!("  [7/9] assignments: OK");

    // 8: Stats.
    //
    // `total_plays` counts *audible* plays, not attempts: it is incremented
    // beside the per-assignment `play_count`, inside the `assignment.enabled`
    // branch and downstream of the `global_enabled` gate. A muted event is not
    // a play, and a stats line that said otherwise would be reporting sounds
    // the user never heard.
    //
    // So the exact count is 2, and this asserted `>= 3`. The suite calls
    // `play()` six times, but four of those exist precisely to check that
    // nothing is emitted:
    //
    //   [2] Notification, enabled       -> audible   (+1)
    //   [3] Notification, event disabled -> silent
    //   [4] Error, custom sound          -> audible   (+1)
    //   [5] Startup, "Silent" scheme     -> silent
    //   [6] Startup, global sound off    -> silent
    //
    // Assert the exact value rather than a bound: it is deterministic, and an
    // equality also catches the counter *over*-counting -- a suppressed play
    // that started incrementing would still satisfy any `>=`.
    let (schemes, events, plays, ops) = stats();
    assert_eq!(schemes, 2);
    assert!(events >= 10);
    assert_eq!(plays, 2, "two of the six play() calls are audible");
    assert!(ops > 0);
    crate::serial_println!("  [8/9] stats: OK");

    // 9: A sound path that is not valid UTF-8 survives round-trip.
    //
    // This runs last on purpose: it plays a sound, and test 8 asserts an exact
    // play count.
    //
    // `\xFF` is not a valid UTF-8 byte in any position, so a `String`-backed
    // field could only have stored it by replacing it -- with U+FFFD if the
    // code was honest, or with whatever `from_utf8_lossy` produced if it was
    // not. Either way the assignment would then name a *different* file. The
    // `\xFE` sibling is the half of the test that matters: it proves the two
    // spellings stay distinct, which a lossy conversion would not, since both
    // collapse to the same replacement character.
    let weird = PathBuf::from(&b"/sys/sounds/\xFF-alert.wav"[..]);
    let sibling = PathBuf::from(&b"/sys/sounds/\xFE-alert.wav"[..]);
    assert_ne!(weird, sibling);
    set_sound(SoundEvent::Warning, &weird).expect("set non-UTF-8 path");
    let played = play(SoundEvent::Warning).expect("play non-UTF-8");
    assert_eq!(played.as_ref(), Some(&weird), "byte-for-byte round trip");
    assert_ne!(played.as_ref(), Some(&sibling));
    // And it is reachable through the listing the settings UI reads, not just
    // through `play`'s return value.
    let listed = list_assignments();
    let warning = listed
        .iter()
        .find(|a| a.event == SoundEvent::Warning)
        .expect("warning assignment");
    assert_eq!(warning.sound_path, weird);
    crate::serial_println!("  [9/9] non-UTF-8 sound path: OK");

    crate::serial_println!("systemsounds::self_test() — all 9 tests passed");
}
