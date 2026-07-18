//! User session management — login sessions, screen lock, session persistence.
//!
//! Manages active user sessions with multi-user support, session switching,
//! screen lock/unlock, idle timeouts, and session state persistence.
//!
//! ## Architecture
//!
//! ```text
//! Display Manager / Login Screen
//!   → sessionmgr::create_session(uid)
//!   → sessionmgr::lock_session() / unlock_session()
//!
//! Desktop compositor
//!   → sessionmgr::active_session() for routing input
//!   → sessionmgr::switch_session(id) for user switching
//!
//! Integration:
//!   → loginscreen (login/logout)
//!   → useracct (user verification)
//!   → power (suspend/resume session save)
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
    /// Active and focused.
    Active,
    /// Logged in but in background (another user active).
    Background,
    /// Screen locked.
    Locked,
    /// Closing (saving state).
    Closing,
}

impl SessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Background => "Background",
            Self::Locked => "Locked",
            Self::Closing => "Closing",
        }
    }
}

/// Session type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    /// Local graphical session.
    Graphical,
    /// Local console/TTY session.
    Console,
    /// Remote (SSH, RDP, etc.).
    Remote,
    /// Greeter/login screen session.
    Greeter,
}

impl SessionType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Graphical => "Graphical",
            Self::Console => "Console",
            Self::Remote => "Remote",
            Self::Greeter => "Greeter",
        }
    }
}

/// An active user session.
#[derive(Debug, Clone)]
pub struct Session {
    /// Session ID.
    pub id: u32,
    /// User ID.
    pub uid: u32,
    /// User name.
    pub username: String,
    /// Session state.
    pub state: SessionState,
    /// Session type.
    pub session_type: SessionType,
    /// Display (e.g., ":0", ":1").
    pub display: String,
    /// TTY (e.g., "tty1", "pts/0").
    pub tty: String,
    /// Timestamp when session was created (ns since boot).
    pub created_ns: u64,
    /// Timestamp of last activity (ns since boot).
    pub last_activity_ns: u64,
    /// Whether this is the currently focused session.
    pub is_active: bool,
    /// Remote host (for remote sessions).
    pub remote_host: String,
    /// PID of the session leader process.
    pub leader_pid: u32,
}

/// Session manager configuration.
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Idle timeout before auto-lock (seconds, 0 = disabled).
    pub lock_timeout_secs: u32,
    /// Idle timeout before suspend (seconds, 0 = disabled).
    pub suspend_timeout_secs: u32,
    /// Allow multiple simultaneous sessions.
    pub multi_session: bool,
    /// Show user list on login screen.
    pub show_user_list: bool,
    /// Allow guest sessions.
    pub allow_guest: bool,
    /// Lock on suspend.
    pub lock_on_suspend: bool,
    /// Lock on lid close (laptops).
    pub lock_on_lid_close: bool,
    /// Auto-login user (0 = none).
    pub auto_login_uid: u32,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            lock_timeout_secs: 300,
            suspend_timeout_secs: 900,
            multi_session: true,
            show_user_list: true,
            allow_guest: false,
            lock_on_suspend: true,
            lock_on_lid_close: true,
            auto_login_uid: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    sessions: Vec<Session>,
    config: SessionConfig,
    next_id: u32,
    login_count: u64,
    lock_count: u64,
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

/// Initialise session manager with default configuration.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        sessions: Vec::new(),
        config: SessionConfig::default(),
        next_id: 1,
        login_count: 0,
        lock_count: 0,
        ops: 0,
    });
}

/// Create a new session for a user.
pub fn create_session(
    uid: u32,
    username: &str,
    session_type: SessionType,
    display: &str,
    tty: &str,
) -> KernelResult<u32> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;

        // If multi-session is off and user already has a session, reject.
        if !state.config.multi_session
            && state.sessions.iter().any(|s| s.uid == uid && s.state != SessionState::Closing)
        {
            return Err(KernelError::AlreadyExists);
        }

        // Mark any current active session as background.
        for s in &mut state.sessions {
            if s.is_active {
                s.is_active = false;
                s.state = SessionState::Background;
            }
        }

        state.sessions.push(Session {
            id,
            uid,
            username: String::from(username),
            state: SessionState::Active,
            session_type,
            display: String::from(display),
            tty: String::from(tty),
            created_ns: now,
            last_activity_ns: now,
            is_active: true,
            remote_host: String::new(),
            leader_pid: 0,
        });

        state.login_count += 1;
        Ok(id)
    })
}

/// Destroy a session (logout).
pub fn destroy_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let pos = state.sessions.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        let was_active = state.sessions[pos].is_active;
        state.sessions.remove(pos);

        // If the destroyed session was active, promote another.
        if was_active {
            if let Some(s) = state.sessions.iter_mut().find(|s| s.state != SessionState::Closing) {
                s.is_active = true;
                s.state = SessionState::Active;
            }
        }
        Ok(())
    })
}

/// Lock the active session's screen.
pub fn lock_session() -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.is_active)
            .ok_or(KernelError::NotFound)?;
        session.state = SessionState::Locked;
        state.lock_count += 1;
        Ok(())
    })
}

/// Unlock a locked session.
pub fn unlock_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        if session.state != SessionState::Locked {
            return Err(KernelError::InvalidArgument);
        }
        session.state = if session.is_active { SessionState::Active } else { SessionState::Background };
        Ok(())
    })
}

/// Switch to a different session (make it the active one).
pub fn switch_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        // Verify target exists and is not closing.
        let target_pos = state.sessions.iter().position(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        if state.sessions[target_pos].state == SessionState::Closing {
            return Err(KernelError::InvalidArgument);
        }

        // Demote current active.
        for s in &mut state.sessions {
            if s.is_active && s.id != id {
                s.is_active = false;
                if s.state == SessionState::Active {
                    s.state = SessionState::Background;
                }
            }
        }

        // Promote target.
        let target = &mut state.sessions[target_pos];
        target.is_active = true;
        if target.state == SessionState::Background {
            target.state = SessionState::Active;
        }
        Ok(())
    })
}

/// Get the currently active session.
pub fn active_session() -> Option<Session> {
    let guard = STATE.lock();
    guard.as_ref().and_then(|s| s.sessions.iter().find(|sess| sess.is_active).cloned())
}

/// List all sessions.
pub fn list_sessions() -> Vec<Session> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.sessions.clone(),
        None => Vec::new(),
    }
}

/// Get a session by ID.
pub fn get_session(id: u32) -> KernelResult<Session> {
    with_state(|state| {
        state.sessions.iter().find(|s| s.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Record user activity (updates last_activity timestamp).
pub fn touch_activity() -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(s) = state.sessions.iter_mut().find(|s| s.is_active) {
            s.last_activity_ns = now;
        }
        Ok(())
    })
}

/// Check for idle sessions and auto-lock if configured.
/// Returns number of sessions locked.
pub fn check_idle() -> KernelResult<u32> {
    with_state(|state| {
        if state.config.lock_timeout_secs == 0 {
            return Ok(0);
        }
        let now = crate::hpet::elapsed_ns();
        let timeout_ns = (state.config.lock_timeout_secs as u64) * 1_000_000_000;
        let mut locked = 0u32;
        for s in &mut state.sessions {
            if s.state == SessionState::Active || s.state == SessionState::Background {
                if now.saturating_sub(s.last_activity_ns) > timeout_ns {
                    s.state = SessionState::Locked;
                    state.lock_count += 1;
                    locked += 1;
                }
            }
        }
        Ok(locked)
    })
}

/// Update session manager configuration.
pub fn set_config(config: SessionConfig) -> KernelResult<()> {
    with_state(|state| {
        state.config = config;
        Ok(())
    })
}

/// Get current configuration.
pub fn get_config() -> KernelResult<SessionConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Set auto-login user (0 = disabled).
pub fn set_auto_login(uid: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.auto_login_uid = uid;
        Ok(())
    })
}

/// Set lock timeout in seconds (0 = disabled).
pub fn set_lock_timeout(secs: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.lock_timeout_secs = secs;
        Ok(())
    })
}

/// Set lock-on-suspend behaviour.
pub fn set_lock_on_suspend(enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        state.config.lock_on_suspend = enabled;
        Ok(())
    })
}

/// Statistics: (session_count, login_count, lock_count, active_uid, ops).
pub fn stats() -> (usize, u64, u64, u32, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active_uid = s.sessions.iter()
                .find(|sess| sess.is_active)
                .map_or(0, |sess| sess.uid);
            (s.sessions.len(), s.login_count, s.lock_count, active_uid, s.ops)
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sessionmgr::self_test() — running tests...");

    init_defaults();

    // Test 1: Create a session.
    let id1 = create_session(1000, "alice", SessionType::Graphical, ":0", "tty1").expect("create session");
    assert!(id1 > 0);
    crate::serial_println!("  [1/11] create session: OK");

    // Test 2: Active session should be the one we created.
    let active = active_session().expect("active session");
    assert_eq!(active.uid, 1000);
    assert!(active.is_active);
    crate::serial_println!("  [2/11] active session: OK");

    // Test 3: Create second session, first becomes background.
    let id2 = create_session(1001, "bob", SessionType::Console, ":1", "tty2").expect("create session 2");
    let s1 = get_session(id1).expect("get session 1");
    assert_eq!(s1.state, SessionState::Background);
    assert!(!s1.is_active);
    crate::serial_println!("  [3/11] second session backgrounds first: OK");

    // Test 4: Switch back to first session.
    switch_session(id1).expect("switch session");
    let s1 = get_session(id1).expect("get s1");
    assert!(s1.is_active);
    assert_eq!(s1.state, SessionState::Active);
    crate::serial_println!("  [4/11] switch session: OK");

    // Test 5: Lock session.
    lock_session().expect("lock");
    let s1 = get_session(id1).expect("get s1 locked");
    assert_eq!(s1.state, SessionState::Locked);
    crate::serial_println!("  [5/11] lock session: OK");

    // Test 6: Unlock session.
    unlock_session(id1).expect("unlock");
    let s1 = get_session(id1).expect("get s1 unlocked");
    assert_eq!(s1.state, SessionState::Active);
    crate::serial_println!("  [6/11] unlock session: OK");

    // Test 7: Touch activity.
    touch_activity().expect("touch");
    crate::serial_println!("  [7/11] touch activity: OK");

    // Test 8: List sessions.
    let sessions = list_sessions();
    assert_eq!(sessions.len(), 2);
    crate::serial_println!("  [8/11] list sessions: OK");

    // Test 9: Destroy session.
    destroy_session(id2).expect("destroy");
    let sessions = list_sessions();
    assert_eq!(sessions.len(), 1);
    crate::serial_println!("  [9/11] destroy session: OK");

    // Test 10: Set lock timeout.
    set_lock_timeout(600).expect("set timeout");
    let cfg = get_config().expect("get config");
    assert_eq!(cfg.lock_timeout_secs, 600);
    crate::serial_println!("  [10/11] set lock timeout: OK");

    // Test 11: Stats.
    let (count, logins, locks, active_uid, ops) = stats();
    assert_eq!(count, 1);
    assert!(logins >= 2);
    assert!(locks >= 1);
    assert_eq!(active_uid, 1000);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("sessionmgr::self_test() — all 11 tests passed");
}
