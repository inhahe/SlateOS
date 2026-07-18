//! Terminal session multiplexer.
//!
//! Provides tmux-like functionality for the kernel debug shell: multiple
//! independent terminal sessions, each with its own screen content,
//! scrollback buffer, command history, working directory, and environment
//! variables.  At any time exactly one session is "attached" to the
//! physical console; the others are detached but their state is preserved.
//!
//! ## Design
//!
//! Each `TerminalSession` holds:
//! - A console snapshot (screen buffer, cursor, colors, attributes)
//! - A scrollback buffer (per-session, swapped with the global on switch)
//! - A user-facing name (for identification in `tsession list`)
//! - Creation / last-active timestamps
//!
//! The kshell manages per-session state that lives outside this module:
//! CWD, environment variables, and command history.  On session switch,
//! the kshell saves those from the outgoing session and restores them
//! for the incoming session via the `ShellContext` struct.
//!
//! ## Usage
//!
//! ```text
//! tsession new [name]       — create a new session
//! tsession list             — list all sessions
//! tsession switch <id>      — switch to session (alias: attach)
//! tsession kill <id>        — destroy a session
//! tsession rename <id> name — rename a session
//! tsession                  — show current session info
//! ```
//!
//! Quick-switch: Ctrl+B then 0-9 switches to that session ID directly.

// Subsystem API surface; not every helper has an in-tree caller yet.
#![allow(dead_code)]

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::PreemptSpinMutex as Mutex;

use crate::console::{self, ConsoleSnapshot, ScrollbackBuffer, ScrollCell};
use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of concurrent terminal sessions.
const MAX_SESSIONS: usize = 10;

/// Default name for the initial session.
const DEFAULT_SESSION_NAME: &str = "main";

// ---------------------------------------------------------------------------
// Shell context (CWD, env, history saved per-session by kshell)
// ---------------------------------------------------------------------------

/// Per-session shell context that kshell saves/restores on switch.
///
/// This struct owns the data; kshell copies its state into/out of
/// the global CWD, ENV_VARS, and History when switching sessions.
pub struct ShellContext {
    /// Working directory.
    pub cwd: String,
    /// Environment variables.
    pub env: BTreeMap<String, String>,
    /// Command history entries (newest last).
    pub history: Vec<String>,
}

impl ShellContext {
    /// Create a default shell context.
    fn new() -> Self {
        let mut env = BTreeMap::new();
        env.insert(String::from("PWD"), String::from("/"));
        env.insert(String::from("HOME"), String::from("/"));
        env.insert(String::from("SHELL"), String::from("kshell"));
        env.insert(String::from("USER"), String::from("root"));
        env.insert(String::from("PATH"), String::from("/bin:/usr/bin"));
        Self {
            cwd: String::from("/"),
            env,
            history: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Terminal session
// ---------------------------------------------------------------------------

/// A single terminal session.
///
/// When the session is not the active (attached) session, it holds a
/// frozen copy of the console state and scrollback.  When active, these
/// fields are `None` because the live console *is* the session's state.
struct TerminalSession {
    /// Unique session identifier (0-based, never reused).
    id: u32,
    /// User-facing name.
    name: String,
    /// Console state snapshot (None while this session is active/attached).
    snapshot: Option<ConsoleSnapshot>,
    /// Scrollback buffer (None while this session is active/attached).
    scrollback: Option<ScrollbackBuffer>,
    /// Shell context (CWD, env, history) — always present.
    /// When active, kshell reads from/writes to its own globals; on
    /// switch, the outgoing session's context is updated from those globals.
    pub shell_ctx: ShellContext,
    /// Monotonic timestamp (ticks) when the session was created.
    created_ticks: u64,
    /// Monotonic timestamp (ticks) of last attach.
    last_active_ticks: u64,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

/// Session table: maps session ID → session.
struct SessionTable {
    sessions: BTreeMap<u32, TerminalSession>,
    /// ID of the currently attached session.
    active_id: u32,
    /// Next session ID to allocate.
    next_id: u32,
    /// Whether the session system has been initialized.
    initialized: bool,
}

impl SessionTable {
    const fn new() -> Self {
        Self {
            sessions: BTreeMap::new(),
            active_id: 0,
            next_id: 0,
            initialized: false,
        }
    }
}

static TABLE: Mutex<SessionTable> = Mutex::new(SessionTable::new());

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialize the terminal session system.
///
/// Creates session 0 ("main") as the initial active session.  Must be
/// called after the console and heap are initialized but before the
/// shell starts.
pub fn init() {
    let mut table = TABLE.lock();
    if table.initialized {
        return;
    }

    let session = TerminalSession {
        id: 0,
        name: String::from(DEFAULT_SESSION_NAME),
        snapshot: None, // Active session — state is live in the console.
        scrollback: None,
        shell_ctx: ShellContext::new(),
        created_ticks: current_ticks(),
        last_active_ticks: current_ticks(),
    };

    table.sessions.insert(0, session);
    table.active_id = 0;
    table.next_id = 1;
    table.initialized = true;

    crate::serial_println!("[termsession] Initialized (session 0 \"main\" active)");
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Information about a session, returned by [`list`].
pub struct SessionInfo {
    pub id: u32,
    pub name: String,
    pub active: bool,
    pub created_ticks: u64,
    pub last_active_ticks: u64,
}

/// Create a new terminal session.
///
/// The session starts with a blank screen and default shell context.
/// Returns the new session's ID.
pub fn create(name: &str) -> KernelResult<u32> {
    let mut table = TABLE.lock();
    if !table.initialized {
        return Err(KernelError::NotSupported);
    }
    if table.sessions.len() >= MAX_SESSIONS {
        return Err(KernelError::ResourceExhausted);
    }

    let id = table.next_id;
    table.next_id = table.next_id.wrapping_add(1);

    let session_name = if name.is_empty() {
        alloc::format!("session-{}", id)
    } else {
        String::from(name)
    };

    // Build an initial blank screen snapshot.
    let (cols, rows) = console::dimensions();
    let total_cells = (cols as usize).saturating_mul(rows as usize);
    let blank_screen = alloc::vec![ScrollCell::EMPTY; total_cells];

    let snapshot = ConsoleSnapshot {
        screen: blank_screen,
        cursor_col: 0,
        cursor_row: 0,
        fg_color: 0x00CC_CCCC, // DEFAULT_FG
        bg_color: 0x0000_0000, // DEFAULT_BG
        default_fg: 0x00CC_CCCC,
        default_bg: 0x0000_0000,
        palette: default_palette(),
        bold: false,
        dim: false,
        underline: false,
        reverse: false,
        invisible: false,
        strikethrough: false,
        scroll_top: 0,
        scroll_bottom: rows.saturating_sub(1),
        cols,
        rows,
    };

    let scrollback = ScrollbackBuffer {
        lines: Vec::new(),
        start: 0,
        count: 0,
        cols: cols as usize,
    };

    let session = TerminalSession {
        id,
        name: session_name,
        snapshot: Some(snapshot),
        scrollback: Some(scrollback),
        shell_ctx: ShellContext::new(),
        created_ticks: current_ticks(),
        last_active_ticks: 0,
    };

    table.sessions.insert(id, session);
    Ok(id)
}

/// Destroy a terminal session.
///
/// Cannot destroy the currently active session — switch away first.
pub fn destroy(id: u32) -> KernelResult<()> {
    let mut table = TABLE.lock();
    if !table.initialized {
        return Err(KernelError::NotSupported);
    }
    if id == table.active_id {
        return Err(KernelError::InvalidArgument);
    }
    if table.sessions.remove(&id).is_none() {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

/// Switch to a different terminal session.
///
/// Saves the current session's console state and scrollback, then
/// restores the target session's state.  The caller (kshell) is
/// responsible for saving/restoring shell context (CWD, env, history)
/// using [`save_shell_context`] and [`take_shell_context`] before and
/// after calling this function.
pub fn switch(target_id: u32) -> KernelResult<()> {
    let table = TABLE.lock();
    if !table.initialized {
        return Err(KernelError::NotSupported);
    }
    if target_id == table.active_id {
        // Already on this session — no-op.
        return Ok(());
    }
    if !table.sessions.contains_key(&target_id) {
        return Err(KernelError::NotFound);
    }

    let old_id = table.active_id;

    // Save current session's console state.
    // We must drop the table lock before calling console functions
    // (they acquire the CONSOLE lock — avoid lock ordering issues).
    drop(table);

    let snapshot = console::snapshot_state();
    let scrollback = console::take_scrollback();

    let mut table = TABLE.lock();

    // Store saved state in the outgoing session.
    if let Some(old_session) = table.sessions.get_mut(&old_id) {
        old_session.snapshot = snapshot;
        old_session.scrollback = Some(scrollback);
    }

    // Extract target session's saved state.
    let target = table.sessions.get_mut(&target_id)
        .ok_or(KernelError::NotFound)?;

    let target_snapshot = target.snapshot.take();
    let target_scrollback = target.scrollback.take();
    target.last_active_ticks = current_ticks();
    table.active_id = target_id;

    // Drop table lock before calling console functions.
    drop(table);

    // Restore target session's console state.
    if let Some(snap) = &target_snapshot {
        console::restore_state(snap);
    } else {
        // Target was previously active but has no snapshot — shouldn't
        // happen, but clear the screen as a safe fallback.
        console::clear();
    }

    // Restore target session's scrollback.
    if let Some(sb) = target_scrollback {
        console::put_scrollback(sb);
    }

    Ok(())
}

/// List all sessions.
pub fn list() -> Vec<SessionInfo> {
    let table = TABLE.lock();
    let mut result = Vec::with_capacity(table.sessions.len());
    for session in table.sessions.values() {
        result.push(SessionInfo {
            id: session.id,
            name: session.name.clone(),
            active: session.id == table.active_id,
            created_ticks: session.created_ticks,
            last_active_ticks: session.last_active_ticks,
        });
    }
    result
}

/// Get the currently active session ID.
pub fn active_id() -> u32 {
    TABLE.lock().active_id
}

/// Rename a session.
pub fn rename(id: u32, new_name: &str) -> KernelResult<()> {
    let mut table = TABLE.lock();
    let session = table.sessions.get_mut(&id)
        .ok_or(KernelError::NotFound)?;
    session.name.clear();
    session.name.push_str(new_name);
    Ok(())
}

/// Get the name of a session.
pub fn session_name(id: u32) -> Option<String> {
    TABLE.lock().sessions.get(&id).map(|s| s.name.clone())
}

/// Get a mutable reference to the shell context for the given session.
///
/// The caller should save current kshell state into this context before
/// a switch and read from it after a switch.
pub fn save_shell_context(id: u32, ctx: ShellContext) {
    let mut table = TABLE.lock();
    if let Some(session) = table.sessions.get_mut(&id) {
        session.shell_ctx = ctx;
    }
}

/// Get the shell context for a session (clones it).
///
/// Returns `None` if the session doesn't exist.
pub fn get_shell_context(id: u32) -> Option<ShellContext> {
    let table = TABLE.lock();
    table.sessions.get(&id).map(|s| ShellContext {
        cwd: s.shell_ctx.cwd.clone(),
        env: s.shell_ctx.env.clone(),
        history: s.shell_ctx.history.clone(),
    })
}

/// Get the number of active sessions.
pub fn session_count() -> usize {
    TABLE.lock().sessions.len()
}

/// Check if the terminal session system is initialized.
pub fn is_initialized() -> bool {
    TABLE.lock().initialized
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get current tick count for timestamps.
fn current_ticks() -> u64 {
    crate::apic::tick_count()
}

/// Default 16-color ANSI palette (matches console.rs DEFAULT_PALETTE).
fn default_palette() -> [u32; 16] {
    [
        0x0000_0000, // 0  Black
        0x00AA_0000, // 1  Red
        0x0000_AA00, // 2  Green
        0x00AA_5500, // 3  Brown/Yellow
        0x0000_00AA, // 4  Blue
        0x00AA_00AA, // 5  Magenta
        0x0000_AAAA, // 6  Cyan
        0x00AA_AAAA, // 7  White (light gray)
        0x0055_5555, // 8  Bright black (dark gray)
        0x00FF_5555, // 9  Bright red
        0x0055_FF55, // 10 Bright green
        0x00FF_FF55, // 11 Bright yellow
        0x0055_55FF, // 12 Bright blue
        0x00FF_55FF, // 13 Bright magenta
        0x0055_FFFF, // 14 Bright cyan
        0x00FF_FFFF, // 15 Bright white
    ]
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the terminal session multiplexer.
///
/// Tests session creation, listing, renaming, switching (without a real
/// console — validates state management), and destruction.
pub fn self_test() -> KernelResult<()> {
    crate::serial_println!("[termsession] Running self-test...");

    // Ensure initialized.
    if !is_initialized() {
        init();
    }

    test_initial_state()?;
    test_create_session()?;
    test_rename_session()?;
    test_list_sessions()?;
    test_destroy_session()?;
    test_cannot_destroy_active()?;
    test_max_sessions()?;
    test_switch_to_self()?;

    crate::serial_println!("[termsession] Self-test PASSED (8 tests)");
    Ok(())
}

fn test_initial_state() -> KernelResult<()> {
    let id = active_id();
    assert_eq!(id, 0, "initial active session should be 0");
    let name = session_name(0).ok_or(KernelError::NotFound)?;
    assert_eq!(name, DEFAULT_SESSION_NAME, "initial session name");
    let count = session_count();
    // Count may be > 1 if previous tests left sessions; just verify >= 1.
    assert!(count >= 1, "should have at least 1 session");
    crate::serial_println!("[termsession]   Initial state: OK");
    Ok(())
}

fn test_create_session() -> KernelResult<()> {
    let id = create("test-session")?;
    assert!(id > 0, "new session ID should be > 0");
    let name = session_name(id).ok_or(KernelError::NotFound)?;
    assert_eq!(name, "test-session", "session name mismatch");
    // Clean up.
    destroy(id)?;
    crate::serial_println!("[termsession]   Create session: OK");
    Ok(())
}

fn test_rename_session() -> KernelResult<()> {
    let id = create("rename-me")?;
    rename(id, "renamed")?;
    let name = session_name(id).ok_or(KernelError::NotFound)?;
    assert_eq!(name, "renamed", "rename failed");
    destroy(id)?;
    crate::serial_println!("[termsession]   Rename session: OK");
    Ok(())
}

fn test_list_sessions() -> KernelResult<()> {
    let id1 = create("list-a")?;
    let id2 = create("list-b")?;
    let sessions = list();
    // Should contain at least session 0, id1, id2.
    let ids: Vec<u32> = sessions.iter().map(|s| s.id).collect();
    assert!(ids.contains(&0), "list should contain session 0");
    assert!(ids.contains(&id1), "list should contain session id1");
    assert!(ids.contains(&id2), "list should contain session id2");
    // Active flag should be set only for session 0.
    for s in &sessions {
        if s.id == 0 {
            assert!(s.active, "session 0 should be active");
        } else {
            assert!(!s.active, "non-zero session should not be active");
        }
    }
    destroy(id1)?;
    destroy(id2)?;
    crate::serial_println!("[termsession]   List sessions: OK");
    Ok(())
}

fn test_destroy_session() -> KernelResult<()> {
    let id = create("destroy-me")?;
    destroy(id)?;
    // Session should no longer exist.
    let result = rename(id, "ghost");
    assert!(result.is_err(), "rename of destroyed session should fail");
    crate::serial_println!("[termsession]   Destroy session: OK");
    Ok(())
}

fn test_cannot_destroy_active() -> KernelResult<()> {
    let result = destroy(active_id());
    assert!(result.is_err(), "should not be able to destroy active session");
    crate::serial_println!("[termsession]   Cannot destroy active: OK");
    Ok(())
}

fn test_max_sessions() -> KernelResult<()> {
    // Create sessions up to the limit (we already have session 0, so
    // create MAX_SESSIONS - current_count more).
    let current = session_count();
    let mut created = Vec::new();
    for i in 0..(MAX_SESSIONS.saturating_sub(current)) {
        let name = alloc::format!("max-test-{}", i);
        match create(&name) {
            Ok(id) => created.push(id),
            Err(_) => break,
        }
    }
    // Now the table should be full.
    let result = create("one-too-many");
    assert!(result.is_err(), "should fail when at max sessions");
    // Clean up.
    for id in created {
        // Ignore errors — test cleanup is best-effort.
        let _ = destroy(id);
    }
    crate::serial_println!("[termsession]   Max sessions limit: OK");
    Ok(())
}

fn test_switch_to_self() -> KernelResult<()> {
    // Switching to the already-active session should be a no-op success.
    let current = active_id();
    switch(current)?;
    assert_eq!(active_id(), current, "active should not change");
    crate::serial_println!("[termsession]   Switch to self: OK");
    Ok(())
}
