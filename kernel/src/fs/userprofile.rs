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

use crate::fs::path::{Path, PathBuf};
use crate::sync::Mutex;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

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
    /// Avatar image, if one has been set.
    ///
    /// `PathBuf`, not `String`: our filesystem forbids only `/` and NUL in a
    /// name, so a `String` cannot hold every legal path. See
    /// design-decisions.md §261.
    pub avatar_path: Option<PathBuf>,
    /// Home directory.
    ///
    /// The most consequential of the three: this is the root of everything the
    /// user owns. A lossily decoded spelling does not point at an empty home,
    /// it points at *a different directory* -- one that may well exist and
    /// belong to someone else.
    pub home_dir: PathBuf,
    /// Login shell.
    ///
    /// A path to an executable, so the same argument as `home_dir`: the wrong
    /// spelling runs the wrong program, or fails to log the user in at all.
    pub shell: PathBuf,
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
    if guard.is_some() {
        return;
    }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        profiles: alloc::vec![
            UserProfile {
                id: 1,
                username: String::from("root"),
                display_name: String::from("System Administrator"),
                account_type: AccountType::Admin,
                avatar_path: None,
                home_dir: PathBuf::from("/root"),
                shell: PathBuf::from("/bin/kshell"),
                login_count: 1,
                last_login_ns: now,
                created_ns: now,
                is_active: true,
                is_locked: false,
            },
            UserProfile {
                id: 2,
                username: String::from("user"),
                display_name: String::from("Default User"),
                account_type: AccountType::Standard,
                avatar_path: None,
                home_dir: PathBuf::from("/home/user"),
                shell: PathBuf::from("/bin/kshell"),
                login_count: 0,
                last_login_ns: 0,
                created_ns: now,
                is_active: false,
                is_locked: false,
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
pub fn create_profile(
    username: &str,
    display_name: &str,
    account_type: AccountType,
) -> KernelResult<u32> {
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
        // `join`, not `format!("/home/{}")`: it carries the username's bytes
        // through unchanged and cannot produce a doubled separator.
        let home = Path::new("/home").join(username);
        state.profiles.push(UserProfile {
            id,
            username: String::from(username),
            display_name: String::from(display_name),
            account_type,
            avatar_path: None,
            home_dir: home,
            shell: PathBuf::from("/bin/kshell"),
            login_count: 0,
            last_login_ns: 0,
            created_ns: now,
            is_active: false,
            is_locked: false,
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
        if state.profiles.len() == before {
            return Err(KernelError::NotFound);
        }
        Ok(())
    })
}

/// Switch active user.
pub fn switch_user(id: u32) -> KernelResult<()> {
    with_state(|state| {
        // Check target exists and is not locked (immutable borrow).
        let target = state
            .profiles
            .iter()
            .find(|p| p.id == id)
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
        let profile = state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
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
        let profile = state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.is_locked = locked;
        Ok(())
    })
}

/// Update display name.
pub fn set_display_name(id: u32, name: &str) -> KernelResult<()> {
    with_state(|state| {
        let profile = state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.display_name = String::from(name);
        Ok(())
    })
}

/// Set the avatar image path. The empty path clears it.
///
/// # Errors
///
/// Returns [`KernelError::NotFound`] if no profile has that id.
pub fn set_avatar(id: u32, path: impl AsRef<Path>) -> KernelResult<()> {
    let path = path.as_ref();
    with_state(|state| {
        let profile = state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.avatar_path = if path.is_empty() {
            None
        } else {
            Some(path.to_path_buf())
        };
        Ok(())
    })
}

/// Set the home directory.
///
/// `create_profile` derives `/home/<username>`, which was previously the only
/// home a profile could ever have: nothing could move a user onto another
/// volume, and the root account's `/root` was unreachable through the API
/// entirely. Dead configuration -- a field the module displays but no caller
/// can write.
///
/// # Errors
///
/// Returns [`KernelError::NotFound`] if no profile has that id, and
/// [`KernelError::InvalidArgument`] for a path that is empty or relative: a
/// home directory is resolved from no particular working directory, so a
/// relative one names nothing in particular.
pub fn set_home_dir(id: u32, dir: impl AsRef<Path>) -> KernelResult<()> {
    let dir = dir.as_ref();
    if !dir.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let profile = state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.home_dir = dir.to_path_buf();
        Ok(())
    })
}

/// Set the login shell.
///
/// Every profile was hard-wired to `/bin/kshell` with no way to change it,
/// for the same reason `home_dir` was: the field existed and was displayed,
/// but had no setter.
///
/// # Errors
///
/// Returns [`KernelError::NotFound`] if no profile has that id, and
/// [`KernelError::InvalidArgument`] for a path that is empty or relative --
/// a login shell is executed with no established working directory.
pub fn set_shell(id: u32, shell: impl AsRef<Path>) -> KernelResult<()> {
    let shell = shell.as_ref();
    if !shell.is_absolute() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let profile = state
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(KernelError::NotFound)?;
        profile.shell = shell.to_path_buf();
        Ok(())
    })
}

/// Get a profile.
pub fn get_profile(id: u32) -> Option<UserProfile> {
    STATE
        .lock()
        .as_ref()
        .and_then(|s| s.profiles.iter().find(|p| p.id == id).cloned())
}

/// Get active user.
pub fn active_user() -> Option<UserProfile> {
    STATE.lock().as_ref().and_then(|s| {
        s.active_user
            .and_then(|id| s.profiles.iter().find(|p| p.id == id).cloned())
    })
}

/// List profiles.
pub fn list_profiles() -> Vec<UserProfile> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.profiles.clone())
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
    crate::serial_println!("  [1/9] defaults: OK");

    // 2: Create profile.
    let id = create_profile("alice", "Alice Smith", AccountType::Standard).expect("create");
    assert_eq!(list_profiles().len(), 3);
    crate::serial_println!("  [2/9] create: OK");

    // 3: Switch user.
    switch_user(id).expect("switch");
    let active = active_user().expect("active2");
    assert_eq!(active.username, "alice");
    assert_eq!(active.login_count, 1);
    crate::serial_println!("  [3/9] switch: OK");

    // 4: Update name.
    set_display_name(id, "Alice Wonderland").expect("rename");
    let p = get_profile(id).expect("get");
    assert_eq!(p.display_name, "Alice Wonderland");
    crate::serial_println!("  [4/9] update name: OK");

    // 5: Set avatar.
    set_avatar(id, "/avatars/alice.png").expect("avatar");
    let p = get_profile(id).expect("get2");
    assert_eq!(
        p.avatar_path.as_deref(),
        Some(Path::new("/avatars/alice.png"))
    );
    // The empty path clears it rather than storing a path that names nothing.
    set_avatar(id, "").expect("clear avatar");
    assert!(get_profile(id).expect("get2b").avatar_path.is_none());
    set_avatar(id, "/avatars/alice.png").expect("avatar again");
    crate::serial_println!("  [5/9] avatar: OK");

    // 6: Lock/unlock.
    set_locked(2, true).expect("lock");
    assert!(switch_user(2).is_err()); // Locked.
    set_locked(2, false).expect("unlock");
    crate::serial_println!("  [6/9] lock/unlock: OK");

    // 7: Non-UTF-8 home directory, shell and avatar survive byte-exact (§261).
    //
    // `\xFF` and `\xFE` have no UTF-8 spelling in any position, so under the
    // old `String` typing both folded to the same U+FFFD-bearing name. For a
    // home directory that is not a display glitch: it is the root of
    // everything the user owns, and the folded spelling names a *different*
    // directory that may well exist and belong to someone else. The shell is
    // an executable path, so the same spelling error runs the wrong program.
    // See design-decisions.md §261.
    let raw_home = Path::new(b"/home/al\xFFce");
    let raw_home_sibling = Path::new(b"/home/al\xFEce");
    let raw_shell = Path::new(b"/bin/sh\xFFll");
    let raw_avatar = Path::new(b"/avatars/al\xFFce.png");
    set_home_dir(id, raw_home).expect("set raw home");
    set_shell(id, raw_shell).expect("set raw shell");
    set_avatar(id, raw_avatar).expect("set raw avatar");
    let p = get_profile(id).expect("get raw");
    assert_eq!(
        p.home_dir.as_path().as_bytes(),
        &b"/home/al\xFFce"[..],
        "the home directory must be stored byte-for-byte"
    );
    assert_eq!(
        p.shell.as_path().as_bytes(),
        &b"/bin/sh\xFFll"[..],
        "the login shell must be stored byte-for-byte"
    );
    assert_eq!(
        p.avatar_path.as_deref().map(Path::as_bytes),
        Some(&b"/avatars/al\xFFce.png"[..]),
        "the avatar path must be stored byte-for-byte"
    );
    assert_ne!(
        p.home_dir.as_path(),
        raw_home_sibling,
        "two homes differing only in an unencodable byte must stay distinct"
    );
    // Relative and empty paths are refused: neither names a home or a shell
    // without a working directory, and there is none at login time.
    assert!(set_home_dir(id, "home/alice").is_err());
    assert!(set_home_dir(id, "").is_err());
    assert!(set_shell(id, "bin/sh").is_err());
    assert!(set_shell(id, "").is_err());
    assert!(set_home_dir(9999, "/home/nobody").is_err());
    crate::serial_println!("  [7/9] non-UTF-8 home, shell and avatar: OK");

    // 8: Delete (can't delete active).
    assert!(delete_profile(id).is_err());
    switch_user(1).expect("switch_back");
    delete_profile(id).expect("delete");
    assert_eq!(list_profiles().len(), 2);
    crate::serial_println!("  [8/9] delete: OK");

    // 9: Stats.
    let (count, logins, switches, ops) = stats();
    assert_eq!(count, 2);
    assert!(logins >= 3);
    assert!(switches >= 2);
    assert!(ops > 0);
    crate::serial_println!("  [9/9] stats: OK");

    crate::serial_println!("userprofile::self_test() — all 9 tests passed");
}
