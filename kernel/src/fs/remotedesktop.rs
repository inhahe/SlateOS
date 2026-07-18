//! Remote desktop — screen sharing and remote control sessions.
//!
//! Manages incoming and outgoing remote desktop connections with
//! protocol support for RDP-like and VNC-like protocols, session
//! security, and display quality settings.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Sharing → Remote Desktop
//!   → remotedesktop::set_enabled() / set_require_auth()
//!
//! Incoming connections
//!   → remotedesktop::accept_session(client_addr)
//!   → compositor routes frames to remote encoder
//!
//! Outgoing connections
//!   → remotedesktop::connect(host, port) → outgoing session
//!
//! Integration:
//!   → sessionmgr (remote session type)
//!   → fwsettings (port opening)
//!   → credentials (authentication)
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

/// Remote desktop protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdProtocol {
    /// Custom RDP-like protocol.
    Rdp,
    /// VNC (RFB).
    Vnc,
    /// Screen sharing only (view, no control).
    ViewOnly,
}

impl RdProtocol {
    pub fn label(self) -> &'static str {
        match self {
            Self::Rdp => "RDP",
            Self::Vnc => "VNC",
            Self::ViewOnly => "View Only",
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Self::Rdp => 3389,
            Self::Vnc => 5900,
            Self::ViewOnly => 5900,
        }
    }
}

/// Session direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionDirection {
    Incoming,
    Outgoing,
}

impl SessionDirection {
    pub fn label(self) -> &'static str {
        match self {
            Self::Incoming => "Incoming",
            Self::Outgoing => "Outgoing",
        }
    }
}

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RdSessionState {
    Connecting,
    Authenticating,
    Active,
    Paused,
    Disconnected,
}

impl RdSessionState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Authenticating => "Authenticating",
            Self::Active => "Active",
            Self::Paused => "Paused",
            Self::Disconnected => "Disconnected",
        }
    }
}

/// Quality preset for remote display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
    Adaptive,
}

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Adaptive => "Adaptive",
        }
    }
}

/// A remote desktop session.
#[derive(Debug, Clone)]
pub struct RdSession {
    pub id: u32,
    pub direction: SessionDirection,
    pub protocol: RdProtocol,
    pub state: RdSessionState,
    pub remote_host: String,
    pub remote_port: u16,
    pub username: String,
    pub quality: QualityPreset,
    pub clipboard_shared: bool,
    pub audio_redirect: bool,
    pub started_ns: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

/// Remote desktop server configuration.
#[derive(Debug, Clone)]
pub struct RdConfig {
    pub enabled: bool,
    pub protocol: RdProtocol,
    pub port: u16,
    pub require_auth: bool,
    pub allow_unattended: bool,
    pub clipboard_sharing: bool,
    pub audio_redirect: bool,
    pub quality: QualityPreset,
    pub max_sessions: u8,
}

impl Default for RdConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            protocol: RdProtocol::Rdp,
            port: 3389,
            require_auth: true,
            allow_unattended: false,
            clipboard_sharing: true,
            audio_redirect: true,
            quality: QualityPreset::Adaptive,
            max_sessions: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    config: RdConfig,
    sessions: Vec<RdSession>,
    next_id: u32,
    total_connections: u64,
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
        config: RdConfig::default(),
        sessions: Vec::new(),
        next_id: 1,
        total_connections: 0,
        ops: 0,
    });
}

pub fn set_enabled(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.enabled = enabled; Ok(()) })
}

pub fn is_enabled() -> bool {
    STATE.lock().as_ref().is_some_and(|s| s.config.enabled)
}

pub fn set_port(port: u16) -> KernelResult<()> {
    with_state(|state| { state.config.port = port; Ok(()) })
}

pub fn set_require_auth(required: bool) -> KernelResult<()> {
    with_state(|state| { state.config.require_auth = required; Ok(()) })
}

pub fn set_quality(quality: QualityPreset) -> KernelResult<()> {
    with_state(|state| { state.config.quality = quality; Ok(()) })
}

pub fn set_clipboard_sharing(enabled: bool) -> KernelResult<()> {
    with_state(|state| { state.config.clipboard_sharing = enabled; Ok(()) })
}

pub fn get_config() -> KernelResult<RdConfig> {
    with_state(|state| Ok(state.config.clone()))
}

/// Accept an incoming connection.
pub fn accept_session(remote_host: &str, protocol: RdProtocol, username: &str) -> KernelResult<u32> {
    with_state(|state| {
        if !state.config.enabled {
            return Err(KernelError::NotSupported);
        }
        let active = state.sessions.iter().filter(|s| s.state == RdSessionState::Active).count();
        if active >= state.config.max_sessions as usize {
            return Err(KernelError::ResourceExhausted);
        }

        let id = state.next_id;
        state.next_id += 1;
        state.total_connections += 1;
        let now = crate::hpet::elapsed_ns();

        state.sessions.push(RdSession {
            id,
            direction: SessionDirection::Incoming,
            protocol,
            state: RdSessionState::Active,
            remote_host: String::from(remote_host),
            remote_port: 0,
            username: String::from(username),
            quality: state.config.quality,
            clipboard_shared: state.config.clipboard_sharing,
            audio_redirect: state.config.audio_redirect,
            started_ns: now,
            bytes_sent: 0,
            bytes_received: 0,
        });

        Ok(id)
    })
}

/// Initiate an outgoing connection.
pub fn connect(host: &str, port: u16, protocol: RdProtocol) -> KernelResult<u32> {
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;
        state.total_connections += 1;
        let now = crate::hpet::elapsed_ns();

        state.sessions.push(RdSession {
            id,
            direction: SessionDirection::Outgoing,
            protocol,
            state: RdSessionState::Connecting,
            remote_host: String::from(host),
            remote_port: port,
            username: String::new(),
            quality: state.config.quality,
            clipboard_shared: state.config.clipboard_sharing,
            audio_redirect: state.config.audio_redirect,
            started_ns: now,
            bytes_sent: 0,
            bytes_received: 0,
        });

        Ok(id)
    })
}

/// Disconnect a session.
pub fn disconnect(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = RdSessionState::Disconnected;
        Ok(())
    })
}

/// Remove disconnected sessions.
pub fn cleanup() -> KernelResult<usize> {
    with_state(|state| {
        let before = state.sessions.len();
        state.sessions.retain(|s| s.state != RdSessionState::Disconnected);
        Ok(before - state.sessions.len())
    })
}

pub fn list_sessions() -> Vec<RdSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sessions.clone())
}

pub fn get_session(id: u32) -> KernelResult<RdSession> {
    with_state(|state| {
        state.sessions.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// Statistics: (active_sessions, total_connections, enabled, port, ops).
pub fn stats() -> (usize, u64, bool, u16, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active = s.sessions.iter().filter(|sess| sess.state == RdSessionState::Active).count();
            (active, s.total_connections, s.config.enabled, s.config.port, s.ops)
        }
        None => (0, 0, false, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("remotedesktop::self_test() — running tests...");
    init_defaults();

    // 1: Disabled by default.
    assert!(!is_enabled());
    crate::serial_println!("  [1/11] disabled by default: OK");

    // 2: Enable.
    set_enabled(true).expect("enable");
    assert!(is_enabled());
    crate::serial_println!("  [2/11] enable: OK");

    // 3: Accept incoming.
    let id1 = accept_session("192.168.1.50", RdProtocol::Rdp, "alice").expect("accept");
    assert!(id1 > 0);
    crate::serial_println!("  [3/11] accept session: OK");

    // 4: Session is active.
    let s = get_session(id1).expect("get");
    assert_eq!(s.state, RdSessionState::Active);
    assert_eq!(s.direction, SessionDirection::Incoming);
    crate::serial_println!("  [4/11] session active: OK");

    // 5: Connect outgoing.
    let id2 = connect("10.0.0.5", 3389, RdProtocol::Rdp).expect("connect");
    let s2 = get_session(id2).expect("get outgoing");
    assert_eq!(s2.direction, SessionDirection::Outgoing);
    crate::serial_println!("  [5/11] outgoing connection: OK");

    // 6: List sessions.
    let sessions = list_sessions();
    assert_eq!(sessions.len(), 2);
    crate::serial_println!("  [6/11] list sessions: OK");

    // 7: Disconnect.
    disconnect(id1).expect("disconnect");
    let s = get_session(id1).expect("get after disconnect");
    assert_eq!(s.state, RdSessionState::Disconnected);
    crate::serial_println!("  [7/11] disconnect: OK");

    // 8: Cleanup.
    let cleaned = cleanup().expect("cleanup");
    assert_eq!(cleaned, 1);
    crate::serial_println!("  [8/11] cleanup: OK");

    // 9: Set port.
    set_port(5900).expect("set port");
    let cfg = get_config().expect("get config");
    assert_eq!(cfg.port, 5900);
    crate::serial_println!("  [9/11] set port: OK");

    // 10: Set quality.
    set_quality(QualityPreset::High).expect("set quality");
    crate::serial_println!("  [10/11] set quality: OK");

    // 11: Stats.
    let (active, total, enabled, port, ops) = stats();
    assert!(active <= 1);
    assert!(total >= 2);
    assert!(enabled);
    assert_eq!(port, 5900);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("remotedesktop::self_test() — all 11 tests passed");
}
