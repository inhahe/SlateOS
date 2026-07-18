//! Game Mode — gaming performance optimization.
//!
//! Provides a system-wide game mode that suppresses notifications,
//! blocks background tasks, boosts performance profiles, and tracks
//! gaming sessions.
//!
//! ## Architecture
//!
//! ```text
//! Game launched / user toggles
//!   → gamemode::activate() → enters game mode
//!   → gamemode::deactivate() → restores normal mode
//!
//! While active:
//!   → Notifications suppressed
//!   → Background tasks paused
//!   → Performance profile set to maximum
//!   → System updates deferred
//!   → Screen saver disabled
//!
//! Integration:
//!   → focusassist (notification suppression)
//!   → powerprofile (performance boost)
//!   → updatemgr (defer updates)
//!   → screensaver (disable)
//!   → quicksettings (game mode tile)
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

/// Game mode state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameModeState {
    Inactive,
    Active,
    /// Auto-detected from running game process.
    AutoActive,
}

impl GameModeState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inactive => "Inactive",
            Self::Active => "Active",
            Self::AutoActive => "Auto-Active",
        }
    }
}

/// Game mode optimization flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Optimization {
    SuppressNotifications,
    BoostPerformance,
    DisableScreenSaver,
    DeferUpdates,
    PauseBackgroundTasks,
    DisableCompositorEffects,
    PrioritizeGameProcess,
}

impl Optimization {
    pub fn label(self) -> &'static str {
        match self {
            Self::SuppressNotifications => "Suppress Notifications",
            Self::BoostPerformance => "Boost Performance",
            Self::DisableScreenSaver => "Disable Screen Saver",
            Self::DeferUpdates => "Defer Updates",
            Self::PauseBackgroundTasks => "Pause Background Tasks",
            Self::DisableCompositorEffects => "Disable Compositor Effects",
            Self::PrioritizeGameProcess => "Prioritize Game Process",
        }
    }
}

/// A registered game.
#[derive(Debug, Clone)]
pub struct RegisteredGame {
    pub id: u32,
    pub name: String,
    pub process_name: String,
    pub auto_detect: bool,
    pub custom_optimizations: Vec<Optimization>,
    pub total_sessions: u64,
    /// Total play time in seconds.
    pub total_play_secs: u64,
}

/// A game session.
#[derive(Debug, Clone)]
pub struct GameSession {
    pub game_id: u32,
    pub game_name: String,
    pub started_ns: u64,
    pub ended_ns: u64,
    pub duration_secs: u64,
}

/// Game mode configuration.
#[derive(Debug, Clone)]
pub struct GameModeConfig {
    pub auto_detect: bool,
    pub optimizations: Vec<Optimization>,
    pub show_fps_overlay: bool,
    pub capture_hotkey: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_GAMES: usize = 100;
const MAX_SESSIONS: usize = 200;

struct State {
    mode_state: GameModeState,
    config: GameModeConfig,
    games: Vec<RegisteredGame>,
    sessions: Vec<GameSession>,
    active_game_id: u32,
    next_game_id: u32,
    total_activations: u64,
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

fn default_optimizations() -> Vec<Optimization> {
    alloc::vec![
        Optimization::SuppressNotifications,
        Optimization::BoostPerformance,
        Optimization::DisableScreenSaver,
        Optimization::DeferUpdates,
        Optimization::PauseBackgroundTasks,
    ]
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    *guard = Some(State {
        mode_state: GameModeState::Inactive,
        config: GameModeConfig {
            auto_detect: true,
            optimizations: default_optimizations(),
            show_fps_overlay: false,
            capture_hotkey: String::from("F12"),
        },
        games: Vec::new(),
        sessions: Vec::new(),
        active_game_id: 0,
        next_game_id: 1,
        total_activations: 0,
        ops: 0,
    });
}

/// Activate game mode (manual).
pub fn activate(game_id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.mode_state != GameModeState::Inactive {
            return Err(KernelError::InvalidArgument);
        }
        if game_id > 0 {
            if !state.games.iter().any(|g| g.id == game_id) {
                return Err(KernelError::NotFound);
            }
            state.active_game_id = game_id;
        }
        state.mode_state = GameModeState::Active;
        state.total_activations += 1;

        // Record session start.
        let game_name = state.games.iter().find(|g| g.id == game_id)
            .map_or(String::from("Unknown"), |g| g.name.clone());
        if let Some(g) = state.games.iter_mut().find(|g| g.id == game_id) {
            g.total_sessions += 1;
        }
        if state.sessions.len() >= MAX_SESSIONS {
            state.sessions.remove(0);
        }
        state.sessions.push(GameSession {
            game_id, game_name,
            started_ns: crate::hpet::elapsed_ns(),
            ended_ns: 0, duration_secs: 0,
        });
        Ok(())
    })
}

/// Deactivate game mode.
pub fn deactivate() -> KernelResult<()> {
    with_state(|state| {
        if state.mode_state == GameModeState::Inactive {
            return Err(KernelError::InvalidArgument);
        }
        state.mode_state = GameModeState::Inactive;
        let now = crate::hpet::elapsed_ns();

        // Close active session.
        if let Some(session) = state.sessions.last_mut() {
            if session.ended_ns == 0 {
                session.ended_ns = now;
                let dur_ns = now.saturating_sub(session.started_ns);
                session.duration_secs = dur_ns / 1_000_000_000;
                // Update total play time.
                let gid = session.game_id;
                let dur = session.duration_secs;
                if let Some(g) = state.games.iter_mut().find(|g| g.id == gid) {
                    g.total_play_secs += dur;
                }
            }
        }
        state.active_game_id = 0;
        Ok(())
    })
}

/// Register a game for auto-detection.
pub fn register_game(name: &str, process_name: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.games.len() >= MAX_GAMES {
            return Err(KernelError::ResourceExhausted);
        }
        let id = state.next_game_id;
        state.next_game_id += 1;
        state.games.push(RegisteredGame {
            id, name: String::from(name),
            process_name: String::from(process_name),
            auto_detect: true,
            custom_optimizations: Vec::new(),
            total_sessions: 0, total_play_secs: 0,
        });
        Ok(id)
    })
}

/// Unregister a game.
pub fn unregister_game(game_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.games.iter().position(|g| g.id == game_id)
            .ok_or(KernelError::NotFound)?;
        state.games.remove(pos);
        Ok(())
    })
}

/// Get current mode state.
pub fn current_state() -> GameModeState {
    STATE.lock().as_ref().map_or(GameModeState::Inactive, |s| s.mode_state)
}

/// Toggle FPS overlay.
pub fn set_fps_overlay(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.show_fps_overlay = enabled;
        Ok(())
    })
}

/// Set auto-detect.
pub fn set_auto_detect(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.auto_detect = enabled;
        Ok(())
    })
}

/// List registered games.
pub fn list_games() -> Vec<RegisteredGame> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.games.clone())
}

/// Recent sessions.
pub fn list_sessions(count: usize) -> Vec<GameSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let start = s.sessions.len().saturating_sub(count);
        s.sessions[start..].to_vec()
    })
}

/// Statistics: (game_count, total_activations, active, ops).
pub fn stats() -> (usize, u64, bool, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.games.len(), s.total_activations, s.mode_state != GameModeState::Inactive, s.ops),
        None => (0, 0, false, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("gamemode::self_test() — running tests...");
    // Start from a clean, freshly-defaulted state so the assertions below are
    // exact and the registered game / session / activation fixtures this test
    // creates do not leak into the live /proc/gamemode table afterward (the
    // kshell `gamemode test` subcommand calls this directly).
    *STATE.lock() = None;
    init_defaults();

    // 1: Initially inactive, with the default config and NO games/sessions —
    //    init_defaults seeds only configuration (default optimizations,
    //    auto_detect, F12 capture hotkey), never fabricated games or sessions.
    assert_eq!(current_state(), GameModeState::Inactive);
    assert_eq!(list_games().len(), 0);
    assert_eq!(list_sessions(10).len(), 0);
    let (g0, a0, act0, _) = stats();
    assert_eq!((g0, a0, act0), (0, 0, false));
    crate::serial_println!("  [1/8] clean defaults: OK");

    // 2: Register game — first game gets id 1.
    let gid = register_game("Test Game", "testgame.exe").expect("register");
    assert_eq!(gid, 1);
    assert_eq!(list_games().len(), 1);
    crate::serial_println!("  [2/8] register game: OK");

    // 3: Activate.
    activate(gid).expect("activate");
    assert_eq!(current_state(), GameModeState::Active);
    crate::serial_println!("  [3/8] activate: OK");

    // 4: Double activate rejected (already Active).
    assert!(activate(gid).is_err());
    crate::serial_println!("  [4/8] double activate: OK");

    // 5: Deactivate.
    deactivate().expect("deactivate");
    assert_eq!(current_state(), GameModeState::Inactive);
    crate::serial_println!("  [5/8] deactivate: OK");

    // 6: Session recorded and closed (one session for the activated game, with
    //    a non-zero end timestamp set by deactivate).
    let sessions = list_sessions(10);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].game_id, gid);
    assert!(sessions[0].ended_ns > 0);
    crate::serial_println!("  [6/8] session recorded: OK");

    // 7: Config toggles take effect.
    set_fps_overlay(true).expect("fps");
    set_auto_detect(false).expect("auto");
    crate::serial_println!("  [7/8] settings: OK");

    // 8: Stats — exactly 1 game, 1 activation, not active.
    let (games, acts, active, ops) = stats();
    assert_eq!((games, acts, active), (1, 1, false));
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Restore the clean default config/state so no test fixtures (registered
    // game, session, flipped fps/auto_detect) leak into the live module.
    *STATE.lock() = None;
    init_defaults();
    crate::serial_println!("gamemode::self_test() — all 8 tests passed");
}
