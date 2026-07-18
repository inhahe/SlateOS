//! User accounts management — user creation, authentication, and profiles.
//!
//! Manages OS user accounts, groups, login sessions, and per-user
//! configuration. Provides the data model for the login screen,
//! user settings panel, and `id`/`whoami` commands.
//!
//! ## Design Reference
//!
//! design.txt line 1275: "users" (in Settings panel).
//! Also implied by capabilities/permissions system, per-user home
//! directories, and multi-user session support.
//!
//! ## Architecture
//!
//! ```text
//! Login screen / session manager
//!   → useracct::authenticate(username, password) → SessionToken
//!   → useracct::current_user() → UserInfo
//!
//! Settings panel
//!   → useracct::create_user(...)
//!   → useracct::set_avatar(...)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const MAX_USERS: usize = 64;
const MAX_GROUPS: usize = 128;
const MAX_SESSIONS: usize = 16;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// User account type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    /// Full administrator.
    Administrator,
    /// Standard user (default).
    Standard,
    /// Guest (limited, no persistent data).
    Guest,
    /// System service account (non-interactive).
    System,
}

impl AccountType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Administrator => "administrator",
            Self::Standard => "standard",
            Self::Guest => "guest",
            Self::System => "system",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "admin" | "administrator" => Some(Self::Administrator),
            "standard" | "user" => Some(Self::Standard),
            "guest" => Some(Self::Guest),
            "system" | "service" => Some(Self::System),
            _ => None,
        }
    }
}

/// Login method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    Password,
    Pin,
    Fingerprint,
    NoPassword,
}

impl LoginMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Pin => "PIN",
            Self::Fingerprint => "fingerprint",
            Self::NoPassword => "none",
        }
    }
}

/// A user account.
#[derive(Debug, Clone)]
pub struct UserAccount {
    pub uid: u64,
    pub username: String,
    pub display_name: String,
    pub account_type: AccountType,
    pub login_method: LoginMethod,
    /// Home directory path.
    pub home_dir: String,
    /// Default shell.
    pub shell: String,
    /// Avatar image path (empty = default).
    pub avatar: String,
    /// Whether auto-login is enabled for this user.
    pub auto_login: bool,
    /// Whether the account is enabled.
    pub enabled: bool,
    /// Whether the account is locked (too many failed attempts).
    pub locked: bool,
    /// Password hash (simple hash for simulation).
    password_hash: u64,
    /// Last login timestamp (ns).
    pub last_login_ns: u64,
    /// Creation timestamp (ns).
    pub created_ns: u64,
    /// Groups this user belongs to (by group id).
    pub groups: Vec<u64>,
}

/// A user group.
#[derive(Debug, Clone)]
pub struct Group {
    pub gid: u64,
    pub name: String,
    pub description: String,
    /// Whether this is a system group.
    pub system_group: bool,
}

/// An active login session.
#[derive(Debug, Clone)]
pub struct Session {
    pub session_id: u64,
    pub uid: u64,
    pub username: String,
    /// Login timestamp (ns).
    pub login_ns: u64,
    /// Whether this is the active (foreground) session.
    pub active: bool,
}

// ---------------------------------------------------------------------------
// Global state
// ---------------------------------------------------------------------------

struct State {
    users: Vec<UserAccount>,
    groups: Vec<Group>,
    sessions: Vec<Session>,
    /// Currently active user ID.
    current_uid: Option<u64>,
    /// Maximum failed login attempts before lockout.
    max_failed_attempts: u32,
}

impl State {
    const fn new() -> Self {
        Self {
            users: Vec::new(),
            groups: Vec::new(),
            sessions: Vec::new(),
            current_uid: None,
            max_failed_attempts: 5,
        }
    }
}

static STATE: Mutex<State> = Mutex::new(State::new());
static NEXT_UID: AtomicU64 = AtomicU64::new(1000);
static NEXT_GID: AtomicU64 = AtomicU64::new(1000);
static NEXT_SID: AtomicU64 = AtomicU64::new(1);
static LOGIN_COUNT: AtomicU64 = AtomicU64::new(0);

/// Simple hash for password simulation (not cryptographic!).
fn simple_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// User management
// ---------------------------------------------------------------------------

/// Create a new user account.
pub fn create_user(username: &str, display_name: &str, password: &str, account_type: AccountType) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.users.len() >= MAX_USERS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.users.iter().any(|u| u.username == username) {
        return Err(KernelError::AlreadyExists);
    }
    let uid = NEXT_UID.fetch_add(1, Ordering::Relaxed);
    let ts = crate::hpet::elapsed_ns();
    let home = alloc::format!("/home/{}", username);
    let login_method = if password.is_empty() { LoginMethod::NoPassword } else { LoginMethod::Password };
    state.users.push(UserAccount {
        uid,
        username: String::from(username),
        display_name: String::from(display_name),
        account_type,
        login_method,
        home_dir: home,
        shell: String::from("/bin/kshell"),
        avatar: String::new(),
        auto_login: false,
        enabled: true,
        locked: false,
        password_hash: simple_hash(password),
        last_login_ns: 0,
        created_ns: ts,
        groups: Vec::new(),
    });
    Ok(uid)
}

/// Remove a user account.
pub fn remove_user(uid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Can't remove root/system user.
    if let Some(u) = state.users.iter().find(|u| u.uid == uid) {
        if u.account_type == AccountType::System {
            return Err(KernelError::PermissionDenied);
        }
    }
    let len = state.users.len();
    state.users.retain(|u| u.uid != uid);
    if state.users.len() == len { return Err(KernelError::NotFound); }
    // Remove sessions for this user.
    state.sessions.retain(|s| s.uid != uid);
    Ok(())
}

/// Get user by UID.
pub fn get_user(uid: u64) -> KernelResult<UserAccount> {
    STATE.lock().users.iter().find(|u| u.uid == uid).cloned().ok_or(KernelError::NotFound)
}

/// Get user by username.
pub fn get_user_by_name(username: &str) -> KernelResult<UserAccount> {
    STATE.lock().users.iter().find(|u| u.username == username).cloned().ok_or(KernelError::NotFound)
}

/// List all users.
pub fn list_users() -> Vec<UserAccount> {
    STATE.lock().users.clone()
}

/// Set display name.
pub fn set_display_name(uid: u64, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.display_name = String::from(name);
    Ok(())
}

/// Set avatar path.
pub fn set_avatar(uid: u64, path: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.avatar = String::from(path);
    Ok(())
}

/// Set account type.
pub fn set_account_type(uid: u64, acct_type: AccountType) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.account_type = acct_type;
    Ok(())
}

/// Enable/disable account.
pub fn set_enabled(uid: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.enabled = enabled;
    Ok(())
}

/// Unlock a locked account.
pub fn unlock(uid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.locked = false;
    Ok(())
}

/// Change password.
pub fn change_password(uid: u64, new_password: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.password_hash = simple_hash(new_password);
    u.login_method = if new_password.is_empty() { LoginMethod::NoPassword } else { LoginMethod::Password };
    Ok(())
}

/// Set auto-login.
pub fn set_auto_login(uid: u64, auto: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Clear all auto-login first (only one user can have it).
    if auto { for u in &mut state.users { u.auto_login = false; } }
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    u.auto_login = auto;
    Ok(())
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// Authenticate a user and create a session.
pub fn authenticate(username: &str, password: &str) -> KernelResult<u64> {
    let mut state = STATE.lock();
    let user = state.users.iter_mut().find(|u| u.username == username)
        .ok_or(KernelError::NotFound)?;

    if !user.enabled { return Err(KernelError::PermissionDenied); }
    if user.locked { return Err(KernelError::PermissionDenied); }

    let hash = simple_hash(password);
    if user.login_method == LoginMethod::Password && user.password_hash != hash {
        return Err(KernelError::PermissionDenied);
    }

    let ts = crate::hpet::elapsed_ns();
    user.last_login_ns = ts;
    let uid = user.uid;
    let uname = user.username.clone();

    if state.sessions.len() >= MAX_SESSIONS {
        return Err(KernelError::ResourceExhausted);
    }
    let sid = NEXT_SID.fetch_add(1, Ordering::Relaxed);
    // Deactivate all other sessions.
    for s in &mut state.sessions { s.active = false; }
    state.sessions.push(Session {
        session_id: sid,
        uid,
        username: uname,
        login_ns: ts,
        active: true,
    });
    state.current_uid = Some(uid);
    LOGIN_COUNT.fetch_add(1, Ordering::Relaxed);
    Ok(sid)
}

/// Log out a session.
pub fn logout(session_id: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let len = state.sessions.len();
    state.sessions.retain(|s| s.session_id != session_id);
    if state.sessions.len() == len { return Err(KernelError::NotFound); }
    // Activate the next session if any.
    if let Some(last) = state.sessions.last_mut() {
        last.active = true;
        state.current_uid = Some(last.uid);
    } else {
        state.current_uid = None;
    }
    Ok(())
}

/// Get the currently active user.
pub fn current_user() -> Option<UserAccount> {
    let state = STATE.lock();
    state.current_uid
        .and_then(|uid| state.users.iter().find(|u| u.uid == uid).cloned())
}

/// List active sessions.
pub fn list_sessions() -> Vec<Session> {
    STATE.lock().sessions.clone()
}

// ---------------------------------------------------------------------------
// Group management
// ---------------------------------------------------------------------------

pub fn create_group(name: &str, desc: &str, system_group: bool) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.groups.len() >= MAX_GROUPS { return Err(KernelError::ResourceExhausted); }
    if state.groups.iter().any(|g| g.name == name) { return Err(KernelError::AlreadyExists); }
    let gid = NEXT_GID.fetch_add(1, Ordering::Relaxed);
    state.groups.push(Group { gid, name: String::from(name), description: String::from(desc), system_group });
    Ok(gid)
}

pub fn remove_group(gid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if let Some(g) = state.groups.iter().find(|g| g.gid == gid) {
        if g.system_group { return Err(KernelError::PermissionDenied); }
    }
    let len = state.groups.len();
    state.groups.retain(|g| g.gid != gid);
    if state.groups.len() == len { return Err(KernelError::NotFound); }
    // Remove from all users.
    for u in &mut state.users { u.groups.retain(|g| *g != gid); }
    Ok(())
}

pub fn add_to_group(uid: u64, gid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.groups.iter().any(|g| g.gid == gid) { return Err(KernelError::NotFound); }
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    if !u.groups.contains(&gid) { u.groups.push(gid); }
    Ok(())
}

pub fn remove_from_group(uid: u64, gid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state.users.iter_mut().find(|u| u.uid == uid).ok_or(KernelError::NotFound)?;
    let len = u.groups.len();
    u.groups.retain(|g| *g != gid);
    if u.groups.len() == len { return Err(KernelError::NotFound); }
    Ok(())
}

pub fn list_groups() -> Vec<Group> { STATE.lock().groups.clone() }

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.users.is_empty() { return; }

    let ts = crate::hpet::elapsed_ns();

    // System user (UID 0).
    state.users.push(UserAccount {
        uid: 0,
        username: String::from("root"),
        display_name: String::from("System Administrator"),
        account_type: AccountType::System,
        login_method: LoginMethod::Password,
        home_dir: String::from("/root"),
        shell: String::from("/bin/kshell"),
        avatar: String::new(),
        auto_login: false,
        enabled: true,
        locked: false,
        password_hash: simple_hash(""),
        last_login_ns: 0,
        created_ns: ts,
        groups: Vec::new(),
    });

    // Default user.
    state.users.push(UserAccount {
        uid: 1000,
        username: String::from("user"),
        display_name: String::from("Default User"),
        account_type: AccountType::Administrator,
        login_method: LoginMethod::Password,
        home_dir: String::from("/home/user"),
        shell: String::from("/bin/kshell"),
        avatar: String::new(),
        auto_login: true,
        enabled: true,
        locked: false,
        password_hash: simple_hash(""),
        last_login_ns: 0,
        created_ns: ts,
        groups: Vec::new(),
    });

    // Default groups.
    let grps = [
        (0, "root", "System administrators", true),
        (1, "users", "All regular users", true),
        (2, "audio", "Audio device access", true),
        (3, "video", "Video device access", true),
        (4, "network", "Network configuration", true),
        (5, "storage", "Disk/USB access", true),
    ];
    for &(gid, name, desc, sys) in &grps {
        state.groups.push(Group { gid, name: String::from(name), description: String::from(desc), system_group: sys });
    }
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    (state.users.len(), state.groups.len(), state.sessions.len(), LOGIN_COUNT.load(Ordering::Relaxed))
}

pub fn reset_stats() { LOGIN_COUNT.store(0, Ordering::Relaxed); }

pub fn clear_all() {
    let mut state = STATE.lock();
    state.users.clear();
    state.groups.clear();
    state.sessions.clear();
    state.current_uid = None;
}

// ---------------------------------------------------------------------------
// Self-tests
// ---------------------------------------------------------------------------

pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;
    clear_all();
    reset_stats();

    // Test 1: Init defaults.
    serial_println!("  useracct::self_test 1: init defaults");
    init_defaults();
    assert!(list_users().len() >= 2);
    assert!(list_groups().len() >= 6);

    // Test 2: Create user.
    serial_println!("  useracct::self_test 2: create user");
    let uid = create_user("alice", "Alice Smith", "pass123", AccountType::Standard)?;
    let alice = get_user(uid)?;
    assert_eq!(alice.username, "alice");
    assert_eq!(alice.home_dir, "/home/alice");

    // Test 3: Authentication.
    serial_println!("  useracct::self_test 3: authentication");
    let sid = authenticate("alice", "pass123")?;
    assert!(current_user().is_some());
    assert_eq!(current_user().unwrap().username, "alice");
    let sessions = list_sessions();
    assert!(!sessions.is_empty());

    // Test 4: Bad password.
    serial_println!("  useracct::self_test 4: bad password");
    assert!(authenticate("alice", "wrongpass").is_err());

    // Test 5: Logout.
    serial_println!("  useracct::self_test 5: logout");
    logout(sid)?;
    assert!(list_sessions().is_empty());

    // Test 6: Groups.
    serial_println!("  useracct::self_test 6: groups");
    let gid = create_group("developers", "Software developers", false)?;
    add_to_group(uid, gid)?;
    let alice2 = get_user(uid)?;
    assert!(alice2.groups.contains(&gid));
    remove_from_group(uid, gid)?;
    remove_group(gid)?;

    // Test 7: Account management.
    serial_println!("  useracct::self_test 7: account management");
    set_display_name(uid, "Alice B. Smith")?;
    set_avatar(uid, "/avatars/alice.png")?;
    set_enabled(uid, false)?;
    assert!(authenticate("alice", "pass123").is_err()); // Disabled.
    set_enabled(uid, true)?;
    change_password(uid, "newpass")?;
    let sid2 = authenticate("alice", "newpass")?;
    assert!(current_user().is_some());
    logout(sid2)?;
    remove_user(uid)?;

    let (uc, gc, sc, logins) = stats();
    assert!(uc >= 2);
    assert!(gc >= 6);
    assert_eq!(sc, 0);
    assert!(logins > 0);

    clear_all();
    reset_stats();
    serial_println!("  useracct: all tests passed");
    Ok(())
}
