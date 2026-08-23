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

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

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
    ///
    /// A [`PathBuf`], not a `String`: this names the root of everything the
    /// account owns, and a filesystem name may hold any byte but `/` and NUL.
    /// Held as text, a home directory whose bytes are not valid UTF-8 either
    /// fails to round-trip or folds to a *different* directory — one that may
    /// exist and belong to somebody else. See design-decisions.md §261.
    pub home_dir: PathBuf,
    /// Default shell.
    ///
    /// [`PathBuf`] for the same reason as `home_dir`: this is the program the
    /// login path executes, so a silently-substituted spelling is a
    /// silently-substituted program.
    pub shell: PathBuf,
    /// Avatar image path (empty = default).
    pub avatar: PathBuf,
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
pub fn create_user(
    username: &str,
    display_name: &str,
    password: &str,
    account_type: AccountType,
) -> KernelResult<u64> {
    let mut state = STATE.lock();
    if state.users.len() >= MAX_USERS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.users.iter().any(|u| u.username == username) {
        return Err(KernelError::AlreadyExists);
    }
    let uid = NEXT_UID.fetch_add(1, Ordering::Relaxed);
    let ts = crate::hpet::elapsed_ns();
    // `join`, not `format!("/home/{}", …)`: it carries the username's bytes
    // through unchanged and collapses the separator correctly.
    let home = Path::new("/home").join(username);
    let login_method = if password.is_empty() {
        LoginMethod::NoPassword
    } else {
        LoginMethod::Password
    };
    state.users.push(UserAccount {
        uid,
        username: String::from(username),
        display_name: String::from(display_name),
        account_type,
        login_method,
        home_dir: home,
        shell: PathBuf::from("/bin/kshell"),
        avatar: PathBuf::new(),
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
    if state.users.len() == len {
        return Err(KernelError::NotFound);
    }
    // Remove sessions for this user.
    state.sessions.retain(|s| s.uid != uid);
    Ok(())
}

/// Get user by UID.
pub fn get_user(uid: u64) -> KernelResult<UserAccount> {
    STATE
        .lock()
        .users
        .iter()
        .find(|u| u.uid == uid)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// Get user by username.
pub fn get_user_by_name(username: &str) -> KernelResult<UserAccount> {
    STATE
        .lock()
        .users
        .iter()
        .find(|u| u.username == username)
        .cloned()
        .ok_or(KernelError::NotFound)
}

/// List all users.
pub fn list_users() -> Vec<UserAccount> {
    STATE.lock().users.clone()
}

/// Set display name.
pub fn set_display_name(uid: u64, name: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.display_name = String::from(name);
    Ok(())
}

/// Set avatar path. An empty path restores the default avatar.
///
/// Takes `impl AsRef<Path>` so a caller holding raw filesystem bytes can pass
/// them straight through; `&str` still coerces.
///
/// # Errors
/// [`KernelError::NotFound`] if no account has this UID.
pub fn set_avatar(uid: u64, path: impl AsRef<Path>) -> KernelResult<()> {
    let path = path.as_ref();
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.avatar = path.to_path_buf();
    Ok(())
}

/// Set the account's home directory.
///
/// The path must be absolute. `Path::is_absolute` is false for the empty
/// path, so one check rejects both an empty and a relative home — neither of
/// which the login path could chdir into.
///
/// # Errors
/// [`KernelError::InvalidArgument`] if `dir` is not absolute;
/// [`KernelError::NotFound`] if no account has this UID.
pub fn set_home_dir(uid: u64, dir: impl AsRef<Path>) -> KernelResult<()> {
    let dir = dir.as_ref();
    if !dir.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.home_dir = dir.to_path_buf();
    Ok(())
}

/// Set the account's login shell.
///
/// # Errors
/// [`KernelError::InvalidArgument`] if `shell` is not absolute;
/// [`KernelError::NotFound`] if no account has this UID.
pub fn set_shell(uid: u64, shell: impl AsRef<Path>) -> KernelResult<()> {
    let shell = shell.as_ref();
    if !shell.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.shell = shell.to_path_buf();
    Ok(())
}

/// Set account type.
pub fn set_account_type(uid: u64, acct_type: AccountType) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.account_type = acct_type;
    Ok(())
}

/// Enable/disable account.
pub fn set_enabled(uid: u64, enabled: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.enabled = enabled;
    Ok(())
}

/// Unlock a locked account.
pub fn unlock(uid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.locked = false;
    Ok(())
}

/// Change password.
pub fn change_password(uid: u64, new_password: &str) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.password_hash = simple_hash(new_password);
    u.login_method = if new_password.is_empty() {
        LoginMethod::NoPassword
    } else {
        LoginMethod::Password
    };
    Ok(())
}

/// Set auto-login.
pub fn set_auto_login(uid: u64, auto: bool) -> KernelResult<()> {
    let mut state = STATE.lock();
    // Clear all auto-login first (only one user can have it).
    if auto {
        for u in &mut state.users {
            u.auto_login = false;
        }
    }
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    u.auto_login = auto;
    Ok(())
}

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/// Authenticate a user and create a session.
pub fn authenticate(username: &str, password: &str) -> KernelResult<u64> {
    let mut state = STATE.lock();
    let user = state
        .users
        .iter_mut()
        .find(|u| u.username == username)
        .ok_or(KernelError::NotFound)?;

    if !user.enabled {
        return Err(KernelError::PermissionDenied);
    }
    if user.locked {
        return Err(KernelError::PermissionDenied);
    }

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
    for s in &mut state.sessions {
        s.active = false;
    }
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
    if state.sessions.len() == len {
        return Err(KernelError::NotFound);
    }
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
    state
        .current_uid
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
    if state.groups.len() >= MAX_GROUPS {
        return Err(KernelError::ResourceExhausted);
    }
    if state.groups.iter().any(|g| g.name == name) {
        return Err(KernelError::AlreadyExists);
    }
    let gid = NEXT_GID.fetch_add(1, Ordering::Relaxed);
    state.groups.push(Group {
        gid,
        name: String::from(name),
        description: String::from(desc),
        system_group,
    });
    Ok(gid)
}

pub fn remove_group(gid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if let Some(g) = state.groups.iter().find(|g| g.gid == gid) {
        if g.system_group {
            return Err(KernelError::PermissionDenied);
        }
    }
    let len = state.groups.len();
    state.groups.retain(|g| g.gid != gid);
    if state.groups.len() == len {
        return Err(KernelError::NotFound);
    }
    // Remove from all users.
    for u in &mut state.users {
        u.groups.retain(|g| *g != gid);
    }
    Ok(())
}

pub fn add_to_group(uid: u64, gid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    if !state.groups.iter().any(|g| g.gid == gid) {
        return Err(KernelError::NotFound);
    }
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    if !u.groups.contains(&gid) {
        u.groups.push(gid);
    }
    Ok(())
}

pub fn remove_from_group(uid: u64, gid: u64) -> KernelResult<()> {
    let mut state = STATE.lock();
    let u = state
        .users
        .iter_mut()
        .find(|u| u.uid == uid)
        .ok_or(KernelError::NotFound)?;
    let len = u.groups.len();
    u.groups.retain(|g| *g != gid);
    if u.groups.len() == len {
        return Err(KernelError::NotFound);
    }
    Ok(())
}

pub fn list_groups() -> Vec<Group> {
    STATE.lock().groups.clone()
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut state = STATE.lock();
    if !state.users.is_empty() {
        return;
    }

    let ts = crate::hpet::elapsed_ns();

    // System user (UID 0).
    state.users.push(UserAccount {
        uid: 0,
        username: String::from("root"),
        display_name: String::from("System Administrator"),
        account_type: AccountType::System,
        login_method: LoginMethod::Password,
        home_dir: PathBuf::from("/root"),
        shell: PathBuf::from("/bin/kshell"),
        avatar: PathBuf::new(),
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
        home_dir: PathBuf::from("/home/user"),
        shell: PathBuf::from("/bin/kshell"),
        avatar: PathBuf::new(),
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
        state.groups.push(Group {
            gid,
            name: String::from(name),
            description: String::from(desc),
            system_group: sys,
        });
    }

    // Lift the id allocators above every id assigned literally above.
    //
    // This fixes a live collision, not a hypothetical one.  `create_user`
    // hands out `NEXT_UID.fetch_add(1)` from a base of 1000, and the default
    // `user` account is written here with a hard-coded `uid: 1000` — so on
    // any freshly booted system the **first account ever created** was given
    // uid 1000 as well.  Nothing rejected it: `create_user` only checks that
    // the *username* is unique.  From then on every uid-keyed operation —
    // `get_user`, `remove_user`, `set_home_dir`, `set_shell`, `set_avatar`,
    // group membership — resolved by linear scan to whichever of the two
    // accounts came first, so `remove_user(1000)` deleted `user` and left the
    // account the caller meant to delete in place.
    //
    // Derived from the entries actually pushed rather than written as a third
    // literal: a future default with a higher id must not be able to
    // reintroduce this by being added without anyone remembering to bump a
    // constant.  `fetch_max` rather than `store` so a `create_user` that ran
    // before `init_defaults` cannot have its allocator wound backwards.
    let highest_uid = state.users.iter().map(|u| u.uid).max().unwrap_or(0);
    NEXT_UID.fetch_max(highest_uid.saturating_add(1), Ordering::Relaxed);
    let highest_gid = state.groups.iter().map(|g| g.gid).max().unwrap_or(0);
    NEXT_GID.fetch_max(highest_gid.saturating_add(1), Ordering::Relaxed);
}

// ---------------------------------------------------------------------------
// Stats
// ---------------------------------------------------------------------------

pub fn stats() -> (usize, usize, usize, u64) {
    let state = STATE.lock();
    (
        state.users.len(),
        state.groups.len(),
        state.sessions.len(),
        LOGIN_COUNT.load(Ordering::Relaxed),
    )
}

pub fn reset_stats() {
    LOGIN_COUNT.store(0, Ordering::Relaxed);
}

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

/// Fixture account name. Deliberately unusable as a real login: no operator
/// creates an account called this, so a leftover from a crashed run is
/// unambiguously ours to delete.
const FIXTURE_USER: &str = "useracct-selftest";
/// Fixture group name, same reasoning.
const FIXTURE_GROUP: &str = "useracct-selftest-grp";

/// Run the module's self-tests.
///
/// **This suite used to open and close with `clear_all()`.** That made it
/// idempotent, and it made the opening `list_users()` assertion true by
/// construction — by deleting every account, every group and every session on
/// the machine. `useracct test` is a shell command; a user typing it expects
/// to be told whether the subsystem works, not to be logged out of a machine
/// whose accounts have all been erased. See `known-issues.md` →
/// `TD-A-SELFTESTS-NOT-IDEMPOTENT`.
///
/// It is now baseline-relative and cleans up after itself, with one guard the
/// other converted suites do not need: it **declines to run while anybody is
/// logged in**. Authenticating deactivates every other session and moves
/// `current_uid`, so unlike a table of rows this is state the suite cannot
/// restore by deleting what it created.
///
/// # Errors
/// Propagates any [`KernelError`] from the operations under test.
pub fn self_test() -> KernelResult<()> {
    use crate::serial_println;

    if !list_sessions().is_empty() {
        serial_println!(
            "  useracct: self-test skipped: {} session(s) live",
            list_sessions().len()
        );
        return Ok(());
    }

    // Restored on the way out: a self-test must not spend the machine's login
    // counter or leave a stale "current user" behind it.
    let saved_current_uid = STATE.lock().current_uid;
    let baseline_logins = LOGIN_COUNT.load(Ordering::Relaxed);

    // A crashed earlier run can leave the fixtures behind. They are ours by
    // name, so reclaiming them is correct — unlike clearing the whole table.
    if let Ok(stale) = get_user_by_name(FIXTURE_USER) {
        remove_user(stale.uid)?;
    }
    if let Some(stale) = list_groups().iter().find(|g| g.name == FIXTURE_GROUP) {
        remove_group(stale.gid)?;
    }

    let baseline_users = list_users().len();
    let baseline_groups = list_groups().len();

    // Test 1: Init defaults, and that a second call does not duplicate them.
    serial_println!("  useracct::self_test 1: init defaults");
    init_defaults();
    assert!(!list_users().is_empty(), "init_defaults left no users");
    assert!(!list_groups().is_empty(), "init_defaults left no groups");
    let after_first = (list_users().len(), list_groups().len());
    init_defaults();
    assert_eq!(
        (list_users().len(), list_groups().len()),
        after_first,
        "init_defaults must be idempotent, not additive"
    );
    let base_users = list_users().len();
    let base_groups = list_groups().len();

    // Test 2: Create user.
    //
    // The uid assertions are the point, not incidental. A freshly booted
    // system used to hand the first created account uid 1000 — the same uid
    // `init_defaults` writes for `user` — and because only the *username* is
    // checked for uniqueness, nothing complained. `get_user(uid)` then
    // resolved by linear scan to whichever came first, so this test would have
    // read back `user`. Asserting the *name behind the returned uid* is what
    // catches that; asserting only that `create_user` succeeded would not.
    serial_println!("  useracct::self_test 2: create user");
    let uid = create_user(
        FIXTURE_USER,
        "Self-Test Account",
        "pass123",
        AccountType::Standard,
    )?;
    assert_eq!(
        list_users().iter().filter(|u| u.uid == uid).count(),
        1,
        "create_user handed out a uid that was already in use"
    );
    let acct = get_user(uid)?;
    assert_eq!(acct.username, FIXTURE_USER);
    assert_eq!(
        acct.home_dir.as_path(),
        Path::new("/home/useracct-selftest"),
        "home must be derived from the username"
    );

    // Test 3: Authentication.
    serial_println!("  useracct::self_test 3: authentication");
    let sid = authenticate(FIXTURE_USER, "pass123")?;
    assert_eq!(
        current_user().map(|u| u.username).as_deref(),
        Some(FIXTURE_USER)
    );
    assert!(!list_sessions().is_empty());

    // Test 4: Bad password.
    serial_println!("  useracct::self_test 4: bad password");
    assert!(authenticate(FIXTURE_USER, "wrongpass").is_err());

    // Test 5: Logout.
    serial_println!("  useracct::self_test 5: logout");
    logout(sid)?;
    assert!(
        list_sessions().is_empty(),
        "our session must be the last one"
    );

    // Test 6: Groups.
    serial_println!("  useracct::self_test 6: groups");
    let gid = create_group(FIXTURE_GROUP, "Self-test group", false)?;
    add_to_group(uid, gid)?;
    assert!(get_user(uid)?.groups.contains(&gid));
    remove_from_group(uid, gid)?;
    remove_group(gid)?;
    assert_eq!(list_groups().len(), base_groups, "group table restored");

    // Test 7: Account management.
    serial_println!("  useracct::self_test 7: account management");
    set_display_name(uid, "Self-Test Account (renamed)")?;
    set_avatar(uid, "/avatars/selftest.png")?;
    set_enabled(uid, false)?;
    assert!(
        authenticate(FIXTURE_USER, "pass123").is_err(),
        "a disabled account must not authenticate"
    );
    set_enabled(uid, true)?;
    change_password(uid, "newpass")?;
    let sid2 = authenticate(FIXTURE_USER, "newpass")?;
    assert!(current_user().is_some());
    logout(sid2)?;

    // Test 8: non-UTF-8 home, shell and avatar (design-decisions.md §261).
    // 0xFF and 0xFE are the two bytes that can never begin a UTF-8 sequence,
    // so a path holding either is legal on disk and unrepresentable as text.
    serial_println!("  useracct::self_test 8: non-UTF-8 home, shell and avatar");
    set_home_dir(uid, Path::new(b"/home/us\xFFr"))?;
    set_shell(uid, Path::new(b"/bin/k\xFEsh"))?;
    set_avatar(uid, Path::new(b"/avatars/us\xFFr.png"))?;
    let weird = get_user(uid)?;
    assert_eq!(
        weird.home_dir.as_bytes(),
        &b"/home/us\xFFr"[..],
        "home directory must round-trip byte-for-byte"
    );
    assert_eq!(
        weird.shell.as_bytes(),
        &b"/bin/k\xFEsh"[..],
        "login shell must round-trip byte-for-byte"
    );
    assert_eq!(
        weird.avatar.as_bytes(),
        &b"/avatars/us\xFFr.png"[..],
        "avatar path must round-trip byte-for-byte"
    );
    // The failure a String field hides: two homes differing only in a byte
    // with no UTF-8 spelling both become U+FFFD, so the account silently
    // points at a directory that is not the one that was set.
    set_home_dir(uid, Path::new(b"/home/us\xFEr"))?;
    assert_ne!(
        get_user(uid)?.home_dir.as_path(),
        Path::new(b"/home/us\xFFr"),
        "0xFE and 0xFF homes must not fold together"
    );
    // Neither an empty nor a relative path is a directory login can enter.
    assert!(set_home_dir(uid, "").is_err(), "empty home rejected");
    assert!(
        set_home_dir(uid, "home/relative").is_err(),
        "relative home rejected"
    );
    assert!(set_shell(uid, "bin/sh").is_err(), "relative shell rejected");
    // An empty avatar is meaningful, though: it selects the default.
    set_avatar(uid, "")?;
    assert!(get_user(uid)?.avatar.is_empty(), "empty avatar clears");

    // Test 9: Cleanup and stats.
    serial_println!("  useracct::self_test 9: cleanup and stats");
    remove_user(uid)?;
    let (uc, gc, sc, logins) = stats();
    assert_eq!(uc, base_users, "user table restored");
    assert_eq!(gc, base_groups, "group table restored");
    assert_eq!(sc, 0, "no session left behind");
    assert!(
        logins > baseline_logins,
        "the three successful logins must have been counted"
    );
    assert!(
        baseline_users <= base_users && baseline_groups <= base_groups,
        "init_defaults must not remove pre-existing users or groups"
    );

    // Hand the machine back exactly as it was found.
    LOGIN_COUNT.store(baseline_logins, Ordering::Relaxed);
    STATE.lock().current_uid = saved_current_uid;

    serial_println!("  useracct: all 9 tests passed");
    Ok(())
}
