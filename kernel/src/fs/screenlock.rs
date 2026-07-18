//! Screen Lock — user authentication and lock screen management.
//!
//! Manages screen locking with configurable timeouts, authentication
//! methods, and lock screen content (now-playing, notifications, clock).
//!
//! ## Architecture
//!
//! ```text
//! Idle timeout reached / manual lock
//!   → screenlock::lock()
//!     → blanks display, shows lock screen
//!
//! User authenticates
//!   → screenlock::authenticate(method, credential)
//!     → verifies PIN/password/biometric
//!     → screenlock::unlock()
//!
//! Integration:
//!   → loginscreen (lock screen UI)
//!   → power (idle detection)
//!   → mediakeys (now-playing on lock screen)
//!   → notifcenter (lock screen notifications)
//!   → useracct (credential verification)
//! ```

#![allow(dead_code)]

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    Password,
    Pin,
    Fingerprint,
    FaceRecognition,
    SmartCard,
    None,
}

impl AuthMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "Password",
            Self::Pin => "PIN",
            Self::Fingerprint => "Fingerprint",
            Self::FaceRecognition => "Face Recognition",
            Self::SmartCard => "Smart Card",
            Self::None => "None",
        }
    }
}

/// Lock state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockState {
    Unlocked,
    Locked,
    AwaitingAuth,
    Lockout,
}

impl LockState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unlocked => "Unlocked",
            Self::Locked => "Locked",
            Self::AwaitingAuth => "Awaiting Auth",
            Self::Lockout => "Lockout",
        }
    }
}

/// Lock screen configuration.
#[derive(Debug, Clone)]
pub struct LockConfig {
    /// Timeout before auto-lock in seconds (0 = never).
    pub timeout_secs: u32,
    /// Require authentication on wake.
    pub require_on_wake: bool,
    /// Show notifications on lock screen.
    pub show_notifications: bool,
    /// Show now-playing on lock screen.
    pub show_media: bool,
    /// Show clock on lock screen.
    pub show_clock: bool,
    /// Primary auth method.
    pub primary_method: AuthMethod,
    /// Allowed auth methods.
    pub allowed_methods: Vec<AuthMethod>,
    /// Failed attempt limit before lockout (0 = no limit).
    pub max_attempts: u32,
    /// Lockout duration in seconds.
    pub lockout_secs: u32,
}

/// Lock event log entry.
#[derive(Debug, Clone)]
pub struct LockEvent {
    pub id: u32,
    pub event_type: LockEventType,
    pub method: AuthMethod,
    pub success: bool,
    pub timestamp_ns: u64,
}

/// Lock event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockEventType {
    Locked,
    Unlocked,
    AuthAttempt,
    Lockout,
    Timeout,
}

impl LockEventType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Locked => "Locked",
            Self::Unlocked => "Unlocked",
            Self::AuthAttempt => "Auth Attempt",
            Self::Lockout => "Lockout",
            Self::Timeout => "Timeout",
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_EVENTS: usize = 200;

struct State {
    lock_state: LockState,
    config: LockConfig,
    events: Vec<LockEvent>,
    failed_attempts: u32,
    next_event_id: u32,
    locked_ns: u64,
    total_locks: u64,
    total_unlocks: u64,
    total_failed: u64,
    total_lockouts: u64,
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

fn add_event(state: &mut State, event_type: LockEventType, method: AuthMethod, success: bool) {
    if state.events.len() >= MAX_EVENTS {
        state.events.remove(0);
    }
    let id = state.next_event_id;
    state.next_event_id += 1;
    state.events.push(LockEvent {
        id,
        event_type,
        method,
        success,
        timestamp_ns: crate::hpet::elapsed_ns(),
    });
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let config = LockConfig {
        timeout_secs: 300, // 5 minutes
        require_on_wake: true,
        show_notifications: true,
        show_media: true,
        show_clock: true,
        primary_method: AuthMethod::Password,
        allowed_methods: alloc::vec![AuthMethod::Password, AuthMethod::Pin],
        max_attempts: 5,
        lockout_secs: 30,
    };

    *guard = Some(State {
        lock_state: LockState::Unlocked,
        config,
        events: Vec::new(),
        failed_attempts: 0,
        next_event_id: 1,
        locked_ns: 0,
        total_locks: 0,
        total_unlocks: 0,
        total_failed: 0,
        total_lockouts: 0,
        ops: 0,
    });
}

/// Lock the screen.
pub fn lock() -> KernelResult<()> {
    with_state(|state| {
        if state.lock_state == LockState::Locked || state.lock_state == LockState::AwaitingAuth {
            return Ok(()); // Already locked.
        }
        state.lock_state = LockState::Locked;
        state.locked_ns = crate::hpet::elapsed_ns();
        state.total_locks += 1;
        state.failed_attempts = 0;
        add_event(state, LockEventType::Locked, AuthMethod::None, true);
        Ok(())
    })
}

/// Attempt authentication.
pub fn authenticate(method: AuthMethod, _credential: &str) -> KernelResult<bool> {
    with_state(|state| {
        if state.lock_state == LockState::Unlocked {
            return Ok(true);
        }
        if state.lock_state == LockState::Lockout {
            return Err(KernelError::PermissionDenied);
        }

        // Check if method is allowed.
        if !state.config.allowed_methods.contains(&method) {
            return Err(KernelError::InvalidArgument);
        }

        state.lock_state = LockState::AwaitingAuth;

        // Simulated auth: accept any non-empty credential.
        let success = !_credential.is_empty();

        if success {
            state.lock_state = LockState::Unlocked;
            state.total_unlocks += 1;
            state.failed_attempts = 0;
            add_event(state, LockEventType::Unlocked, method, true);
        } else {
            state.total_failed += 1;
            state.failed_attempts += 1;
            add_event(state, LockEventType::AuthAttempt, method, false);

            // Check lockout.
            if state.config.max_attempts > 0 && state.failed_attempts >= state.config.max_attempts {
                state.lock_state = LockState::Lockout;
                state.total_lockouts += 1;
                add_event(state, LockEventType::Lockout, method, false);
            } else {
                state.lock_state = LockState::Locked;
            }
        }
        Ok(success)
    })
}

/// Force unlock (admin/recovery).
pub fn force_unlock() -> KernelResult<()> {
    with_state(|state| {
        state.lock_state = LockState::Unlocked;
        state.failed_attempts = 0;
        state.total_unlocks += 1;
        add_event(state, LockEventType::Unlocked, AuthMethod::None, true);
        Ok(())
    })
}

/// Get current lock state.
pub fn get_state() -> LockState {
    STATE.lock().as_ref().map_or(LockState::Unlocked, |s| s.lock_state)
}

/// Set auto-lock timeout.
pub fn set_timeout(secs: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.timeout_secs = secs;
        Ok(())
    })
}

/// Set max failed attempts before lockout.
pub fn set_max_attempts(max: u32) -> KernelResult<()> {
    with_state(|state| {
        state.config.max_attempts = max;
        Ok(())
    })
}

/// Set primary authentication method.
pub fn set_primary_method(method: AuthMethod) -> KernelResult<()> {
    with_state(|state| {
        state.config.primary_method = method;
        if !state.config.allowed_methods.contains(&method) {
            state.config.allowed_methods.push(method);
        }
        Ok(())
    })
}

/// Toggle lock screen feature.
pub fn set_lock_screen_option(option: &str, enabled: bool) -> KernelResult<()> {
    with_state(|state| {
        match option {
            "notifications" => state.config.show_notifications = enabled,
            "media" => state.config.show_media = enabled,
            "clock" => state.config.show_clock = enabled,
            "require_on_wake" => state.config.require_on_wake = enabled,
            _ => return Err(KernelError::InvalidArgument),
        }
        Ok(())
    })
}

/// Get config.
pub fn get_config() -> Option<LockConfig> {
    STATE.lock().as_ref().map(|s| s.config.clone())
}

/// Recent lock events.
pub fn list_events() -> Vec<LockEvent> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.events.clone())
}

/// Statistics: (total_locks, total_unlocks, total_failed, total_lockouts, ops).
pub fn stats() -> (u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.total_locks, s.total_unlocks, s.total_failed, s.total_lockouts, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("screenlock::self_test() — running tests...");
    init_defaults();

    // 1: Initially unlocked.
    assert_eq!(get_state(), LockState::Unlocked);
    crate::serial_println!("  [1/10] initial unlocked: OK");

    // 2: Lock.
    lock().expect("lock");
    assert_eq!(get_state(), LockState::Locked);
    crate::serial_println!("  [2/10] lock: OK");

    // 3: Failed auth (empty credential).
    let result = authenticate(AuthMethod::Password, "").expect("auth");
    assert!(!result);
    assert_eq!(get_state(), LockState::Locked);
    crate::serial_println!("  [3/10] failed auth: OK");

    // 4: Successful auth.
    let result = authenticate(AuthMethod::Password, "secret123").expect("auth2");
    assert!(result);
    assert_eq!(get_state(), LockState::Unlocked);
    crate::serial_println!("  [4/10] successful auth: OK");

    // 5: Lockout after max attempts.
    lock().expect("lock2");
    set_max_attempts(3).expect("max");
    authenticate(AuthMethod::Password, "").ok();
    authenticate(AuthMethod::Password, "").ok();
    authenticate(AuthMethod::Password, "").ok();
    assert_eq!(get_state(), LockState::Lockout);
    crate::serial_println!("  [5/10] lockout: OK");

    // 6: Force unlock.
    force_unlock().expect("force");
    assert_eq!(get_state(), LockState::Unlocked);
    crate::serial_println!("  [6/10] force unlock: OK");

    // 7: Set timeout.
    set_timeout(600).expect("timeout");
    let cfg = get_config().expect("config");
    assert_eq!(cfg.timeout_secs, 600);
    crate::serial_println!("  [7/10] set timeout: OK");

    // 8: Set primary method.
    set_primary_method(AuthMethod::Pin).expect("method");
    let cfg = get_config().expect("config2");
    assert_eq!(cfg.primary_method, AuthMethod::Pin);
    crate::serial_println!("  [8/10] set method: OK");

    // 9: Lock screen options.
    set_lock_screen_option("media", false).expect("media");
    let cfg = get_config().expect("config3");
    assert!(!cfg.show_media);
    crate::serial_println!("  [9/10] lock screen options: OK");

    // 10: Events recorded.
    let events = list_events();
    assert!(!events.is_empty());
    let (locks, unlocks, failed, lockouts, ops) = stats();
    assert!(locks >= 2);
    assert!(unlocks >= 2);
    assert!(failed >= 3);
    assert!(lockouts >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [10/10] events & stats: OK");

    crate::serial_println!("screenlock::self_test() — all 10 tests passed");
}
