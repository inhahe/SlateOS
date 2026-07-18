//! Media keys — media playback session management.
//!
//! Tracks active media playback sessions (now playing info),
//! handles media key events (play/pause/next/prev), and provides
//! a unified media control interface across applications.
//!
//! ## Architecture
//!
//! ```text
//! Media player starts playback
//!   → mediakeys::register_session(app, title, artist)
//!
//! User presses media key
//!   → mediakeys::handle_key(Play/Pause/Next/Prev)
//!     → dispatches to focused media session
//!
//! Lock screen / system tray widget
//!   → mediakeys::get_active_session() → now playing info
//!
//! Integration:
//!   → hotkeys (media key capture)
//!   → soundmixer (volume keys)
//!   → notifcenter (track change notifications)
//!   → loginscreen (lock screen now-playing)
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

/// Playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    Stopped,
    Buffering,
}

impl PlaybackState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Stopped => "Stopped",
            Self::Buffering => "Buffering",
        }
    }
}

/// Media key action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKey {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    FastForward,
    Rewind,
}

impl MediaKey {
    pub fn label(self) -> &'static str {
        match self {
            Self::Play => "Play",
            Self::Pause => "Pause",
            Self::PlayPause => "Play/Pause",
            Self::Stop => "Stop",
            Self::Next => "Next",
            Self::Previous => "Previous",
            Self::FastForward => "Fast Forward",
            Self::Rewind => "Rewind",
        }
    }
}

/// A media playback session.
#[derive(Debug, Clone)]
pub struct MediaSession {
    /// Session ID.
    pub id: u32,
    /// Application name.
    pub app_name: String,
    /// Application PID.
    pub app_pid: u32,
    /// Current track title.
    pub title: String,
    /// Artist.
    pub artist: String,
    /// Album.
    pub album: String,
    /// Playback state.
    pub state: PlaybackState,
    /// Duration in milliseconds (0 = unknown/stream).
    pub duration_ms: u64,
    /// Current position in milliseconds.
    pub position_ms: u64,
    /// Whether this is the active (focused) session.
    pub is_active: bool,
    /// Created timestamp.
    pub created_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SESSIONS: usize = 20;

struct State {
    sessions: Vec<MediaSession>,
    next_id: u32,
    active_id: u32,
    total_key_events: u64,
    total_sessions: u64,
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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        sessions: Vec::new(),
        next_id: 1,
        active_id: 0,
        total_key_events: 0,
        total_sessions: 0,
        ops: 0,
    });
}

/// Register a new media session.
pub fn register_session(app_name: &str, app_pid: u32) -> KernelResult<u32> {
    with_state(|state| {
        if state.sessions.len() >= MAX_SESSIONS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.total_sessions += 1;

        let is_first = state.sessions.is_empty();
        state.sessions.push(MediaSession {
            id, app_name: String::from(app_name), app_pid,
            title: String::new(), artist: String::new(), album: String::new(),
            state: PlaybackState::Stopped,
            duration_ms: 0, position_ms: 0,
            is_active: is_first,
            created_ns: crate::hpet::elapsed_ns(),
        });
        if is_first { state.active_id = id; }
        Ok(id)
    })
}

/// Unregister a media session.
pub fn unregister_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.sessions.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        state.sessions.remove(pos);
        if state.active_id == id {
            state.active_id = state.sessions.first().map(|s| s.id).unwrap_or(0);
            if let Some(s) = state.sessions.first_mut() {
                s.is_active = true;
            }
        }
        Ok(())
    })
}

/// Update now-playing info.
pub fn update_now_playing(id: u32, title: &str, artist: &str, album: &str, duration_ms: u64) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.title = String::from(title);
        session.artist = String::from(artist);
        session.album = String::from(album);
        session.duration_ms = duration_ms;
        session.position_ms = 0;
        Ok(())
    })
}

/// Set playback state.
pub fn set_playback_state(id: u32, state_val: PlaybackState) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = state_val;
        Ok(())
    })
}

/// Update position.
pub fn update_position(id: u32, position_ms: u64) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.position_ms = position_ms;
        Ok(())
    })
}

/// Set active (focused) session.
pub fn set_active(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if !state.sessions.iter().any(|s| s.id == id) {
            return Err(KernelError::NotFound);
        }
        for s in state.sessions.iter_mut() {
            s.is_active = s.id == id;
        }
        state.active_id = id;
        Ok(())
    })
}

/// Handle a media key event (dispatches to active session).
pub fn handle_key(key: MediaKey) -> KernelResult<()> {
    with_state(|state| {
        state.total_key_events += 1;
        let active = state.sessions.iter_mut().find(|s| s.id == state.active_id)
            .ok_or(KernelError::NotFound)?;
        match key {
            MediaKey::Play => active.state = PlaybackState::Playing,
            MediaKey::Pause => active.state = PlaybackState::Paused,
            MediaKey::PlayPause => {
                active.state = match active.state {
                    PlaybackState::Playing => PlaybackState::Paused,
                    _ => PlaybackState::Playing,
                };
            }
            MediaKey::Stop => {
                active.state = PlaybackState::Stopped;
                active.position_ms = 0;
            }
            _ => {} // Next/Prev/FF/Rewind handled by app.
        }
        Ok(())
    })
}

/// Get the active media session (now playing).
pub fn get_active_session() -> Option<MediaSession> {
    STATE.lock().as_ref().and_then(|s| {
        s.sessions.iter().find(|sess| sess.id == s.active_id).cloned()
    })
}

/// Get session by ID.
pub fn get_session(id: u32) -> KernelResult<MediaSession> {
    with_state(|state| {
        state.sessions.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List all sessions.
pub fn list_sessions() -> Vec<MediaSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sessions.clone())
}

/// Statistics: (session_count, total_sessions, total_key_events, active_id, ops).
pub fn stats() -> (usize, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sessions.len(), s.total_sessions, s.total_key_events, s.active_id, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("mediakeys::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_sessions().is_empty());
    assert!(get_active_session().is_none());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Register session.
    let id1 = register_session("music_player", 100).expect("register");
    assert!(id1 > 0);
    crate::serial_println!("  [2/11] register session: OK");

    // 3: Auto-active first session.
    let active = get_active_session().expect("active");
    assert_eq!(active.id, id1);
    assert!(active.is_active);
    crate::serial_println!("  [3/11] auto-active: OK");

    // 4: Update now playing.
    update_now_playing(id1, "Bohemian Rhapsody", "Queen", "A Night at the Opera", 354000).expect("update");
    let s = get_session(id1).expect("get");
    assert_eq!(s.title, "Bohemian Rhapsody");
    assert_eq!(s.artist, "Queen");
    crate::serial_println!("  [4/11] now playing: OK");

    // 5: Play/Pause toggle.
    set_playback_state(id1, PlaybackState::Playing).expect("play");
    handle_key(MediaKey::PlayPause).expect("pp");
    let s = get_session(id1).expect("get2");
    assert_eq!(s.state, PlaybackState::Paused);
    crate::serial_println!("  [5/11] play/pause: OK");

    // 6: Second session.
    let id2 = register_session("podcast_app", 200).expect("register2");
    assert_eq!(list_sessions().len(), 2);
    crate::serial_println!("  [6/11] second session: OK");

    // 7: Switch active.
    set_active(id2).expect("switch");
    let active = get_active_session().expect("active2");
    assert_eq!(active.id, id2);
    crate::serial_println!("  [7/11] switch active: OK");

    // 8: Position update.
    update_position(id1, 120000).expect("pos");
    let s = get_session(id1).expect("get3");
    assert_eq!(s.position_ms, 120000);
    crate::serial_println!("  [8/11] position: OK");

    // 9: Stop.
    handle_key(MediaKey::Stop).expect("stop");
    let active = get_active_session().expect("active3");
    assert_eq!(active.state, PlaybackState::Stopped);
    assert_eq!(active.position_ms, 0);
    crate::serial_println!("  [9/11] stop: OK");

    // 10: Unregister.
    unregister_session(id2).expect("unreg");
    assert_eq!(list_sessions().len(), 1);
    let active = get_active_session().expect("active4");
    assert_eq!(active.id, id1); // Falls back.
    crate::serial_println!("  [10/11] unregister: OK");

    // 11: Stats.
    let (count, total, keys, active_id, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(total, 2);
    assert!(keys >= 2);
    assert_eq!(active_id, id1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("mediakeys::self_test() — all 11 tests passed");
}
