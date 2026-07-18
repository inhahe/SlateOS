//! Play Media — system-wide media playback controls.
//!
//! Provides a unified media playback interface that aggregates
//! playback state from all media apps, enabling global controls.
//!
//! ## Architecture
//!
//! ```text
//! Media app registers
//!   → playmedia::register_session(app) → session id
//!   → playmedia::update_state(session, state)
//!
//! System controls
//!   → playmedia::play_pause() → toggle active session
//!   → playmedia::next/prev() → track control
//!   → playmedia::get_now_playing() → current track info
//!
//! Integration:
//!   → mediakeys (hardware media keys)
//!   → soundmixer (volume routing)
//!   → notifcenter (now-playing notification)
//!   → lockwallpaper (lock screen media controls)
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
    Stopped,
    Playing,
    Paused,
    Buffering,
}

impl PlaybackState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Playing => "Playing",
            Self::Paused => "Paused",
            Self::Buffering => "Buffering",
        }
    }
}

/// Repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    One,
    All,
}

impl RepeatMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::One => "Repeat One",
            Self::All => "Repeat All",
        }
    }
}

/// Media type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Music,
    Video,
    Podcast,
    Audiobook,
    Stream,
    Other,
}

impl MediaType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Music => "Music",
            Self::Video => "Video",
            Self::Podcast => "Podcast",
            Self::Audiobook => "Audiobook",
            Self::Stream => "Stream",
            Self::Other => "Other",
        }
    }
}

/// A media session.
#[derive(Debug, Clone)]
pub struct MediaSession {
    pub id: u32,
    pub app_name: String,
    pub media_type: MediaType,
    pub state: PlaybackState,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub position_ms: u64,
    pub shuffle: bool,
    pub repeat: RepeatMode,
    pub volume_percent: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SESSIONS: usize = 20;

struct State {
    sessions: Vec<MediaSession>,
    active_session_id: Option<u32>,
    next_id: u32,
    total_play_commands: u64,
    total_track_changes: u64,
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
        active_session_id: None,
        next_id: 1,
        total_play_commands: 0,
        total_track_changes: 0,
        ops: 0,
    });
}

/// Register a media session.
pub fn register_session(app_name: &str, media_type: MediaType) -> KernelResult<u32> {
    with_state(|state| {
        if state.sessions.len() >= MAX_SESSIONS {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_id;
        state.next_id += 1;
        state.sessions.push(MediaSession {
            id,
            app_name: String::from(app_name),
            media_type,
            state: PlaybackState::Stopped,
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            duration_ms: 0,
            position_ms: 0,
            shuffle: false,
            repeat: RepeatMode::Off,
            volume_percent: 100,
        });
        if state.active_session_id.is_none() {
            state.active_session_id = Some(id);
        }
        Ok(id)
    })
}

/// Unregister a session.
pub fn unregister_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.sessions.len();
        state.sessions.retain(|s| s.id != id);
        if state.sessions.len() == before { return Err(KernelError::NotFound); }
        if state.active_session_id == Some(id) {
            state.active_session_id = state.sessions.first().map(|s| s.id);
        }
        Ok(())
    })
}

/// Update track info for a session.
pub fn set_track(id: u32, title: &str, artist: &str, album: &str, duration_ms: u64) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.title = String::from(title);
        session.artist = String::from(artist);
        session.album = String::from(album);
        session.duration_ms = duration_ms;
        session.position_ms = 0;
        state.total_track_changes += 1;
        Ok(())
    })
}

/// Update playback state.
pub fn set_state(id: u32, pstate: PlaybackState) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = pstate;
        if pstate == PlaybackState::Playing {
            state.active_session_id = Some(id);
        }
        Ok(())
    })
}

/// Play/pause the active session.
pub fn play_pause() -> KernelResult<PlaybackState> {
    with_state(|state| {
        let id = state.active_session_id.ok_or(KernelError::NotFound)?;
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = match session.state {
            PlaybackState::Playing => PlaybackState::Paused,
            _ => PlaybackState::Playing,
        };
        state.total_play_commands += 1;
        Ok(session.state)
    })
}

/// Next track on active session.
pub fn next_track() -> KernelResult<()> {
    with_state(|state| {
        let id = state.active_session_id.ok_or(KernelError::NotFound)?;
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.position_ms = 0;
        state.total_track_changes += 1;
        Ok(())
    })
}

/// Previous track on active session.
pub fn prev_track() -> KernelResult<()> {
    with_state(|state| {
        let id = state.active_session_id.ok_or(KernelError::NotFound)?;
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.position_ms = 0;
        state.total_track_changes += 1;
        Ok(())
    })
}

/// Set shuffle on active session.
pub fn set_shuffle(shuffle: bool) -> KernelResult<()> {
    with_state(|state| {
        let id = state.active_session_id.ok_or(KernelError::NotFound)?;
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.shuffle = shuffle;
        Ok(())
    })
}

/// Set repeat mode on active session.
pub fn set_repeat(mode: RepeatMode) -> KernelResult<()> {
    with_state(|state| {
        let id = state.active_session_id.ok_or(KernelError::NotFound)?;
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.repeat = mode;
        Ok(())
    })
}

/// Get now-playing info.
pub fn get_now_playing() -> Option<MediaSession> {
    let guard = STATE.lock();
    let state = guard.as_ref()?;
    let id = state.active_session_id?;
    state.sessions.iter().find(|s| s.id == id).cloned()
}

/// List all sessions.
pub fn list_sessions() -> Vec<MediaSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sessions.clone())
}

/// Statistics: (session_count, play_commands, track_changes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sessions.len(), s.total_play_commands, s.total_track_changes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("playmedia::self_test() — running tests...");
    init_defaults();

    // 1: No sessions.
    assert!(get_now_playing().is_none());
    crate::serial_println!("  [1/8] empty: OK");

    // 2: Register session.
    let s1 = register_session("music_player", MediaType::Music).expect("reg");
    assert!(get_now_playing().is_some());
    crate::serial_println!("  [2/8] register: OK");

    // 3: Set track.
    set_track(s1, "Bohemian Rhapsody", "Queen", "Night at the Opera", 354000).expect("track");
    let np = get_now_playing().expect("np");
    assert_eq!(np.title, "Bohemian Rhapsody");
    crate::serial_println!("  [3/8] track: OK");

    // 4: Play/pause.
    set_state(s1, PlaybackState::Playing).expect("play");
    let state = play_pause().expect("pp");
    assert_eq!(state, PlaybackState::Paused);
    let state = play_pause().expect("pp2");
    assert_eq!(state, PlaybackState::Playing);
    crate::serial_println!("  [4/8] play/pause: OK");

    // 5: Shuffle and repeat.
    set_shuffle(true).expect("shuffle");
    set_repeat(RepeatMode::All).expect("repeat");
    let np = get_now_playing().expect("np2");
    assert!(np.shuffle);
    assert_eq!(np.repeat, RepeatMode::All);
    crate::serial_println!("  [5/8] shuffle/repeat: OK");

    // 6: Next/prev.
    next_track().expect("next");
    prev_track().expect("prev");
    crate::serial_println!("  [6/8] next/prev: OK");

    // 7: Multiple sessions.
    let _s2 = register_session("podcast_app", MediaType::Podcast).expect("reg2");
    assert_eq!(list_sessions().len(), 2);
    crate::serial_println!("  [7/8] multi-session: OK");

    // 8: Stats.
    let (sessions, plays, tracks, ops) = stats();
    assert_eq!(sessions, 2);
    assert_eq!(plays, 2);
    assert!(tracks >= 3);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("playmedia::self_test() — all 8 tests passed");
}
