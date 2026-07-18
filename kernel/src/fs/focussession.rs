//! Focus Sessions — pomodoro-style timed focus with break management.
//!
//! Provides structured work/break cycles (25 min work, 5 min break) with
//! distraction blocking, progress tracking, and session history.
//!
//! ## Architecture
//!
//! ```text
//! User starts focus session
//!   → focussession::start(minutes) → begin timer
//!   → focussession::end() → record session
//!
//! Integration:
//!   → focusassist (DND activation)
//!   → eyeprotect (break coordination)
//!   → notifcenter (notification suppression)
//!   → taskbar (session indicator)
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

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Focusing,
    ShortBreak,
    LongBreak,
    Paused,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Focusing => "Focusing",
            Self::ShortBreak => "Short Break",
            Self::LongBreak => "Long Break",
            Self::Paused => "Paused",
        }
    }
}

/// A completed session record.
#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: u32,
    pub task_name: String,
    pub focus_mins: u32,
    pub completed: bool,
    pub started_ns: u64,
    pub ended_ns: u64,
}

/// Focus session configuration.
#[derive(Debug, Clone)]
pub struct FocusConfig {
    pub focus_mins: u32,
    pub short_break_mins: u32,
    pub long_break_mins: u32,
    /// Number of focus sessions before long break.
    pub sessions_before_long: u32,
    pub auto_start_break: bool,
    pub auto_start_focus: bool,
    pub block_notifications: bool,
    pub play_sound_on_end: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_HISTORY: usize = 200;

struct State {
    config: FocusConfig,
    state: SessionState,
    current_task: String,
    sessions_completed: u32,
    session_start_ns: u64,
    history: Vec<SessionRecord>,
    next_id: u32,
    total_focus_mins: u64,
    total_sessions: u64,
    total_abandoned: u64,
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
        config: FocusConfig {
            focus_mins: 25,
            short_break_mins: 5,
            long_break_mins: 15,
            sessions_before_long: 4,
            auto_start_break: true,
            auto_start_focus: false,
            block_notifications: true,
            play_sound_on_end: true,
        },
        state: SessionState::Idle,
        current_task: String::new(),
        sessions_completed: 0,
        session_start_ns: 0,
        history: Vec::new(),
        next_id: 1,
        total_focus_mins: 0,
        total_sessions: 0,
        total_abandoned: 0,
        ops: 0,
    });
}

/// Start a focus session.
pub fn start(task_name: &str) -> KernelResult<()> {
    with_state(|state| {
        if state.state == SessionState::Focusing {
            return Err(KernelError::AlreadyExists);
        }
        state.state = SessionState::Focusing;
        state.current_task = String::from(task_name);
        state.session_start_ns = crate::hpet::elapsed_ns();
        Ok(())
    })
}

/// Complete a focus session successfully.
pub fn complete() -> KernelResult<()> {
    with_state(|state| {
        if state.state != SessionState::Focusing {
            return Err(KernelError::NotSupported);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(SessionRecord {
            id,
            task_name: state.current_task.clone(),
            focus_mins: state.config.focus_mins,
            completed: true,
            started_ns: state.session_start_ns,
            ended_ns: now,
        });
        state.sessions_completed += 1;
        state.total_sessions += 1;
        state.total_focus_mins += state.config.focus_mins as u64;
        // Start break.
        if state.sessions_completed >= state.config.sessions_before_long {
            state.state = SessionState::LongBreak;
            state.sessions_completed = 0;
        } else {
            state.state = SessionState::ShortBreak;
        }
        Ok(())
    })
}

/// Abandon current session.
pub fn abandon() -> KernelResult<()> {
    with_state(|state| {
        if state.state != SessionState::Focusing && state.state != SessionState::Paused {
            return Err(KernelError::NotSupported);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        if state.history.len() >= MAX_HISTORY {
            state.history.remove(0);
        }
        state.history.push(SessionRecord {
            id,
            task_name: state.current_task.clone(),
            focus_mins: 0,
            completed: false,
            started_ns: state.session_start_ns,
            ended_ns: now,
        });
        state.total_abandoned += 1;
        state.state = SessionState::Idle;
        Ok(())
    })
}

/// End break and return to idle (or auto-start next focus).
pub fn end_break() -> KernelResult<()> {
    with_state(|state| {
        if state.state != SessionState::ShortBreak && state.state != SessionState::LongBreak {
            return Err(KernelError::NotSupported);
        }
        state.state = SessionState::Idle;
        Ok(())
    })
}

/// Pause focus session.
pub fn pause() -> KernelResult<()> {
    with_state(|state| {
        if state.state != SessionState::Focusing {
            return Err(KernelError::NotSupported);
        }
        state.state = SessionState::Paused;
        Ok(())
    })
}

/// Resume paused session.
pub fn resume() -> KernelResult<()> {
    with_state(|state| {
        if state.state != SessionState::Paused {
            return Err(KernelError::NotSupported);
        }
        state.state = SessionState::Focusing;
        Ok(())
    })
}

/// Set focus duration.
pub fn set_focus_duration(mins: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.focus_mins = mins.clamp(1, 120);
        Ok(())
    })
}

/// Set break durations.
pub fn set_break_durations(short_mins: u32, long_mins: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.short_break_mins = short_mins.clamp(1, 30);
        state.config.long_break_mins = long_mins.clamp(5, 60);
        Ok(())
    })
}

/// Get current state.
pub fn current_state() -> SessionState {
    STATE.lock().as_ref().map_or(SessionState::Idle, |s| s.state)
}

/// Get config.
pub fn get_config() -> Option<FocusConfig> {
    STATE.lock().as_ref().map(|s| s.config.clone())
}

/// Get session history.
pub fn get_history(max: usize) -> Vec<SessionRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut h = s.history.clone();
        h.reverse();
        h.truncate(max);
        h
    })
}

/// Statistics: (total_sessions, total_abandoned, total_focus_mins, ops).
pub fn stats() -> (u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_sessions, s.total_abandoned, s.total_focus_mins, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("focussession::self_test() — running tests...");
    init_defaults();

    // 1: Idle state.
    assert_eq!(current_state(), SessionState::Idle);
    crate::serial_println!("  [1/8] idle: OK");

    // 2: Start session.
    start("Write code").expect("start");
    assert_eq!(current_state(), SessionState::Focusing);
    crate::serial_println!("  [2/8] start: OK");

    // 3: Complete session.
    complete().expect("complete");
    assert_eq!(current_state(), SessionState::ShortBreak);
    crate::serial_println!("  [3/8] complete: OK");

    // 4: End break.
    end_break().expect("end_break");
    assert_eq!(current_state(), SessionState::Idle);
    crate::serial_println!("  [4/8] break: OK");

    // 5: Pause/resume.
    start("Review PR").expect("start2");
    pause().expect("pause");
    assert_eq!(current_state(), SessionState::Paused);
    resume().expect("resume");
    assert_eq!(current_state(), SessionState::Focusing);
    crate::serial_println!("  [5/8] pause/resume: OK");

    // 6: Abandon.
    abandon().expect("abandon");
    assert_eq!(current_state(), SessionState::Idle);
    crate::serial_println!("  [6/8] abandon: OK");

    // 7: History.
    let history = get_history(10);
    assert_eq!(history.len(), 2);
    assert!(!history[0].completed); // Most recent = abandoned.
    assert!(history[1].completed);
    crate::serial_println!("  [7/8] history: OK");

    // 8: Stats.
    let (sessions, abandoned, focus_mins, ops) = stats();
    assert_eq!(sessions, 1);
    assert_eq!(abandoned, 1);
    assert!(focus_mins >= 25);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("focussession::self_test() — all 8 tests passed");
}
