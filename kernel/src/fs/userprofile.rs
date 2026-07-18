//! User Profile — user account profile management.
//!
//! Manages user profiles including avatar, bio, preferences,
//! session history, and profile switching.
//!
//! ## Architecture
//!
//! ```text
//! User profile management
//!   → userprofile::get(username) → profile data
//!   → userprofile::update(username, field, value) → modify profile
//!   → userprofile::switch(username) → switch active profile
//!
//! Integration:
//!   → useracct (user accounts)
//!   → loginscreen (login screen)
//!   → sessionmgr (session management)
//!   → credentials (credential storage)
//! ```

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Account type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountType {
    Admin,
    Standard,
    Guest,
    System,
    Managed,
}

impl AccountType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Admin => "Admin",
            Self::Standard => "Standard",
            Self::Guest => "Guest",
            Self::System => "System",
            Self::Managed => "Managed",
        }
    }
}

/// A user profile.
#[derive(Debug, Clone)]
pub struct UserProfile {
    pub id: u32,
    pub username: String,
    pub display_name: String,
    pub account_type: AccountType,
    pub avatar_path: Option<String>,
    pub home_dir: String,
    pub shell: String,
    pub login_count: u64,
    pub last_login_ns: u64,
    pub created_ns: u64,
    pub is_active: bool,
    pub is_locked: bool,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROFILES: usize = 100;

struct State {
    profiles: Vec<UserProfile>,
    active_user: Option<u32>,
    next_id: u32,
    total_logins: u64,
    total_switches: u64,
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
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        profiles: alloc::vec![
            UserProfile {
                id: 1, username: String::from("root"), display_name: String::from("System Administrator"),
                account_type: AccountType::Admin, avatar_path: None,
                home_dir: String::from("/root"), shell: String::from("/bin/kshell"),
                login_count: 1, last_login_ns: now, created_ns: now,
                is_active: true, is_locked: false,
            },
            UserProfile {
                id: 2, username: String::from("user"), display_name: String::from("Default User"),
                account_type: AccountType::Standard, avatar_path: None,
                home_dir: String::from("/home/user"), shell: String::from("/bin/kshell"),
                login_count: 0, last_login_ns: 0, created_ns: now,
                is_active: false, is_locked: false,
            },
        ],
        active_user: Some(1),
        next_id: 3,
        total_logins: 1,
        total_switches: 0,
        ops: 0,
    });
}

/// Create a new profile.
pub fn create_profile(username: &str, display_name: &str, account_type: AccountType) -> KernelResult<u32> {
    with_state(|state| {
        if state.profiles.len() >= MAX_PROFILES {
            return Err(KernelError::ResourceExhausted);
        }
        if state.profiles.iter().any(|p| p.username == username) {
            return Err(KernelError::AlreadyExists);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        let home = format!("/home/{}", username);
        state.profiles.push(UserProfile {
            id, username: String::from(username), display_name: String::from(display_name),
            account_type, avatar_path: None, home_dir: home,
            shell: String::from("/bin/kshell"), login_count: 0, last_login_ns: 0,
            created_ns: now, is_active: false, is_locked: false,
        });
        Ok(id)
    })
}

/// Delete a profile.
pub fn delete_profile(id: u32) -> KernelResult<()> {
    with_state(|state| {
        if state.active_user == Some(id) {
            return Err(KernelError::PermissionDenied);
        }
        let before = state.profiles.len();
        state.profiles.retain(|p| p.id != id);
        if state.profiles.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Switch active user.
pub fn switch_user(id: u32) -> KernelResult<()> {
    with_state(|state| {
        // Check target exists and is not locked (immutable borrow).
        let target = state.profiles.iter().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        if target.is_locked {
            return Err(KernelError::PermissionDenied);
        }
        let now = crate::hpet::elapsed_ns();
        // Deactivate old user first.
        if let Some(old_id) = state.active_user {
            if let Some(old) = state.profiles.iter_mut().find(|p| p.id == old_id) {
                old.is_active = false;
            }
        }
        // Now activate the new user.
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.is_active = true;
        profile.login_count += 1;
        profile.last_login_ns = now;
        state.active_user = Some(id);
        state.total_logins += 1;
        state.total_switches += 1;
        Ok(())
    })
}

/// Lock/unlock a profile.
pub fn set_locked(id: u32, locked: bool) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.is_locked = locked;
        Ok(())
    })
}

/// Update display name.
pub fn set_display_name(id: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.display_name = String::from(name);
        Ok(())
    })
}

/// Set avatar path.
pub fn set_avatar(id: u32, path: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state.profiles.iter_mut().find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.avatar_path = Some(String::from(path));
        Ok(())
    })
}

/// Get a profile.
pub fn get_profile(id: u32) -> Option<UserProfile> {
    STATE.lock().as_ref().and_then(|s| s.profiles.iter().find(|p| p.id == id).cloned())
}

/// Get active user.
pub fn active_user() -> Option<UserProfile> {
    STATE.lock().as_ref().and_then(|s| {
        s.active_user.and_then(|id| s.profiles.iter().find(|p| p.id == id).cloned())
    })
}

/// List profiles.
pub fn list_profiles() -> Vec<UserProfile> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.profiles.clone())
}

/// Statistics: (profile_count, total_logins, total_switches, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.profiles.len(), s.total_logins, s.total_switches, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("userprofile::self_test() — running tests...");
    init_defaults();

    // 1: Default profiles.
    assert_eq!(list_profiles().len(), 2);
    let active = active_user().expect("active");
    assert_eq!(active.username, "root");
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create profile.
    let id = create_profile("alice", "Alice Smith", AccountType::Standard).expect("create");
    assert_eq!(list_profiles().len(), 3);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Switch user.
    switch_user(id).expect("switch");
    let active = active_user().expect("active2");
    assert_eq!(active.username, "alice");
    assert_eq!(active.login_count, 1);
    crate::serial_println!("  [3/8] switch: OK");

    // 4: Update name.
    set_display_name(id, "Alice Wonderland").expect("rename");
    let p = get_profile(id).expect("get");
    assert_eq!(p.display_name, "Alice Wonderland");
    crate::serial_println!("  [4/8] update name: OK");

    // 5: Set avatar.
    set_avatar(id, "/avatars/alice.png").expect("avatar");
    let p = get_profile(id).expect("get2");
    assert_eq!(p.avatar_path.as_deref(), Some("/avatars/alice.png"));
    crate::serial_println!("  [5/8] avatar: OK");

    // 6: Lock/unlock.
    set_locked(2, true).expect("lock");
    assert!(switch_user(2).is_err()); // Locked.
    set_locked(2, false).expect("unlock");
    crate::serial_println!("  [6/8] lock/unlock: OK");

    // 7: Delete (can't delete active).
    assert!(delete_profile(id).is_err());
    switch_user(1).expect("switch_back");
    delete_profile(id).expect("delete");
    assert_eq!(list_profiles().len(), 2);
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (count, logins, switches, ops) = stats();
    assert_eq!(count, 2);
    assert!(logins >= 3);
    assert!(switches >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("userprofile::self_test() — all 8 tests passed");
}
