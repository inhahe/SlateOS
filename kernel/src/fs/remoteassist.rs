//! Remote assist — remote assistance and screen sharing.
//!
//! Allows a helper to view/control a user's desktop for troubleshooting.
//! Separate from remotedesktop (which is general-purpose RDP/VNC) —
//! this is specifically for one-time help sessions with safety controls.
//!
//! ## Architecture
//!
//! ```text
//! User requests help
//!   → remoteassist::generate_code() → invitation code
//!
//! Helper enters code
//!   → remoteassist::connect(code) → establish session
//!
//! During session
//!   → remoteassist::grant_control() / revoke_control()
//!   → remoteassist::send_file() / receive_file()
//!
//! Integration:
//!   → remotedesktop (underlying screen share protocol)
//!   → notifcenter (session request notifications)
//!   → credentials (session authentication)
//!   → syslog (audit trail)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Session mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistMode {
    /// Helper can only view.
    ViewOnly,
    /// Helper can control mouse/keyboard.
    FullControl,
    /// Helper can control with user's permission per action.
    PromptedControl,
}

impl AssistMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ViewOnly => "View Only",
            Self::FullControl => "Full Control",
            Self::PromptedControl => "Prompted Control",
        }
    }
}

/// Session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistState {
    WaitingForHelper,
    Connected,
    Paused,
    Ended,
}

impl AssistState {
    pub fn label(self) -> &'static str {
        match self {
            Self::WaitingForHelper => "Waiting",
            Self::Connected => "Connected",
            Self::Paused => "Paused",
            Self::Ended => "Ended",
        }
    }
}

/// An assistance session.
#[derive(Debug, Clone)]
pub struct AssistSession {
    /// Session ID.
    pub id: u32,
    /// Invitation code (6 digits).
    pub code: String,
    /// Current state.
    pub state: AssistState,
    /// Control mode.
    pub mode: AssistMode,
    /// Helper's name/identifier.
    pub helper_name: String,
    /// Host user name.
    pub host_name: String,
    /// Created timestamp (ns).
    pub created_ns: u64,
    /// Connected timestamp (ns, 0 if not yet).
    pub connected_ns: u64,
    /// Duration limit in seconds (0 = unlimited).
    pub duration_limit_secs: u64,
    /// Files transferred.
    pub files_transferred: u32,
    /// Bytes transferred.
    pub bytes_transferred: u64,
    /// Whether clipboard sharing is allowed.
    pub clipboard_shared: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_SESSIONS: usize = 10;

struct State {
    sessions: Vec<AssistSession>,
    next_id: u32,
    /// Simple counter for generating unique codes.
    code_counter: u32,
    total_sessions: u64,
    total_files: u64,
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

/// Generate a 6-digit code from counter + timestamp.
fn generate_code(counter: u32) -> String {
    let ns = crate::hpet::elapsed_ns();
    let code = ((ns / 1000) as u32).wrapping_add(counter * 7919) % 1_000_000;
    format!("{:06}", code)
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
        code_counter: 0,
        total_sessions: 0,
        total_files: 0,
        ops: 0,
    });
}

/// Generate an invitation code and create a waiting session.
pub fn create_invitation(host_name: &str, mode: AssistMode, duration_limit_secs: u64) -> KernelResult<(u32, String)> {
    with_state(|state| {
        if state.sessions.len() >= MAX_SESSIONS {
            return Err(KernelError::ResourceExhausted);
        }

        state.code_counter += 1;
        let code = generate_code(state.code_counter);
        let id = state.next_id;
        state.next_id += 1;

        state.sessions.push(AssistSession {
            id, code: code.clone(),
            state: AssistState::WaitingForHelper,
            mode, helper_name: String::new(),
            host_name: String::from(host_name),
            created_ns: crate::hpet::elapsed_ns(),
            connected_ns: 0,
            duration_limit_secs,
            files_transferred: 0,
            bytes_transferred: 0,
            clipboard_shared: false,
        });

        state.total_sessions += 1;
        Ok((id, code))
    })
}

/// Helper connects using an invitation code.
pub fn connect(code: &str, helper_name: &str) -> KernelResult<u32> {
    with_state(|state| {
        let session = state.sessions.iter_mut()
            .find(|s| s.code == code && s.state == AssistState::WaitingForHelper)
            .ok_or(KernelError::NotFound)?;

        session.state = AssistState::Connected;
        session.helper_name = String::from(helper_name);
        session.connected_ns = crate::hpet::elapsed_ns();

        Ok(session.id)
    })
}

/// Grant control to helper.
pub fn grant_control(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        if session.state != AssistState::Connected {
            return Err(KernelError::InvalidArgument);
        }
        session.mode = AssistMode::FullControl;
        Ok(())
    })
}

/// Revoke control from helper.
pub fn revoke_control(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.mode = AssistMode::ViewOnly;
        Ok(())
    })
}

/// Pause session.
pub fn pause_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = AssistState::Paused;
        Ok(())
    })
}

/// Resume session.
pub fn resume_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = AssistState::Connected;
        Ok(())
    })
}

/// End session.
pub fn end_session(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.state = AssistState::Ended;
        Ok(())
    })
}

/// Record a file transfer.
pub fn record_file_transfer(id: u32, size_bytes: u64) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.files_transferred += 1;
        session.bytes_transferred += size_bytes;
        state.total_files += 1;
        Ok(())
    })
}

/// Toggle clipboard sharing.
pub fn set_clipboard_sharing(id: u32, shared: bool) -> KernelResult<()> {
    with_state(|state| {
        let session = state.sessions.iter_mut().find(|s| s.id == id)
            .ok_or(KernelError::NotFound)?;
        session.clipboard_shared = shared;
        Ok(())
    })
}

/// Get session by ID.
pub fn get_session(id: u32) -> KernelResult<AssistSession> {
    with_state(|state| {
        state.sessions.iter().find(|s| s.id == id).cloned().ok_or(KernelError::NotFound)
    })
}

/// List sessions.
pub fn list_sessions() -> Vec<AssistSession> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sessions.clone())
}

/// Statistics: (active_sessions, total_sessions, total_files, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let active = s.sessions.iter()
                .filter(|ss| matches!(ss.state, AssistState::Connected | AssistState::WaitingForHelper))
                .count();
            (active, s.total_sessions, s.total_files, s.ops)
        }
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("remoteassist::self_test() — running tests...");
    init_defaults();

    // 1: Empty initial.
    assert!(list_sessions().is_empty());
    crate::serial_println!("  [1/11] empty initial: OK");

    // 2: Create invitation.
    let (id, code) = create_invitation("Alice", AssistMode::ViewOnly, 3600).expect("create");
    assert!(id > 0);
    assert_eq!(code.len(), 6);
    crate::serial_println!("  [2/11] create invitation: OK");

    // 3: Session waiting.
    let s = get_session(id).expect("get session");
    assert_eq!(s.state, AssistState::WaitingForHelper);
    assert_eq!(s.host_name, "Alice");
    crate::serial_println!("  [3/11] session waiting: OK");

    // 4: Helper connects.
    let sid = connect(&code, "Bob").expect("connect");
    assert_eq!(sid, id);
    let s = get_session(id).expect("get session 2");
    assert_eq!(s.state, AssistState::Connected);
    assert_eq!(s.helper_name, "Bob");
    crate::serial_println!("  [4/11] helper connects: OK");

    // 5: Grant control.
    grant_control(id).expect("grant");
    let s = get_session(id).expect("get 3");
    assert_eq!(s.mode, AssistMode::FullControl);
    crate::serial_println!("  [5/11] grant control: OK");

    // 6: Revoke control.
    revoke_control(id).expect("revoke");
    let s = get_session(id).expect("get 4");
    assert_eq!(s.mode, AssistMode::ViewOnly);
    crate::serial_println!("  [6/11] revoke control: OK");

    // 7: File transfer.
    record_file_transfer(id, 5000).expect("transfer");
    let s = get_session(id).expect("get 5");
    assert_eq!(s.files_transferred, 1);
    assert_eq!(s.bytes_transferred, 5000);
    crate::serial_println!("  [7/11] file transfer: OK");

    // 8: Pause/resume.
    pause_session(id).expect("pause");
    let s = get_session(id).expect("get 6");
    assert_eq!(s.state, AssistState::Paused);
    resume_session(id).expect("resume");
    let s = get_session(id).expect("get 7");
    assert_eq!(s.state, AssistState::Connected);
    crate::serial_println!("  [8/11] pause/resume: OK");

    // 9: Clipboard sharing.
    set_clipboard_sharing(id, true).expect("clip");
    let s = get_session(id).expect("get 8");
    assert!(s.clipboard_shared);
    crate::serial_println!("  [9/11] clipboard sharing: OK");

    // 10: End session.
    end_session(id).expect("end");
    let s = get_session(id).expect("get 9");
    assert_eq!(s.state, AssistState::Ended);
    crate::serial_println!("  [10/11] end session: OK");

    // 11: Stats.
    let (active, total, files, ops) = stats();
    assert_eq!(active, 0); // Session ended.
    assert_eq!(total, 1);
    assert_eq!(files, 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/11] stats: OK");

    crate::serial_println!("remoteassist::self_test() — all 11 tests passed");
}
