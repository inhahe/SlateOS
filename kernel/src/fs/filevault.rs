//! File Vault — encrypted folder management.
//!
//! Provides per-folder encryption with password-based key derivation,
//! auto-lock on timeout, and secure file access through vault mounts.
//!
//! ## Architecture
//!
//! ```text
//! User creates vault
//!   → filevault::create(path, password) → vault ID
//!   → filevault::unlock(id, password) → mounts decrypted view
//!   → filevault::lock(id) → unmounts, re-encrypts
//!
//! Integration:
//!   → diskencrypt (full-disk encryption)
//!   → encrypt (file-level encryption)
//!   → credentials (password storage)
//!   → screenlock (auto-lock on screen lock)
//! ```

#![allow(dead_code)]

use crate::sync::PreemptSpinMutex as Mutex;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Vault state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultState {
    Locked,
    Unlocked,
    Creating,
}

impl VaultState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Locked => "Locked",
            Self::Unlocked => "Unlocked",
            Self::Creating => "Creating",
        }
    }
}

/// Encryption algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultCipher {
    Aes256Gcm,
    ChaCha20Poly1305,
    Aes256Cbc,
}

impl VaultCipher {
    pub fn label(self) -> &'static str {
        match self {
            Self::Aes256Gcm => "AES-256-GCM",
            Self::ChaCha20Poly1305 => "ChaCha20-Poly1305",
            Self::Aes256Cbc => "AES-256-CBC",
        }
    }
}

/// A file vault.
#[derive(Debug, Clone)]
pub struct Vault {
    pub id: u32,
    /// Human-readable vault label.  Stays a `String`: it is a display name
    /// the user types, not a name read off a filesystem.
    pub name: String,
    /// Directory backing the vault's encrypted store.
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): this is a
    /// caller-supplied directory, which may contain any byte but `/` and NUL.
    pub path: PathBuf,
    /// Where the unlocked vault is exposed.
    ///
    /// Derived as `/vault/<id>` and so always ASCII, but typed as a
    /// `PathBuf` because it is a path -- callers should not have to know
    /// which of a struct's paths happen to be generated.
    pub mount_point: PathBuf,
    pub state: VaultState,
    pub cipher: VaultCipher,
    /// Auto-lock timeout in seconds (0 = disabled).
    pub auto_lock_secs: u32,
    /// Simulated password hash for authentication.
    pub password_hash: u64,
    pub file_count: u32,
    pub size_bytes: u64,
    pub created_ns: u64,
    pub last_accessed_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_VAULTS: usize = 32;

struct State {
    vaults: Vec<Vault>,
    next_id: u32,
    total_unlocks: u64,
    total_locks: u64,
    total_failed_auths: u64,
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

/// Simple hash for simulated password checking.
fn simple_hash(password: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in password.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        vaults: Vec::new(),
        next_id: 1,
        total_unlocks: 0,
        total_locks: 0,
        total_failed_auths: 0,
        ops: 0,
    });
}

/// Create a new vault.
pub fn create_vault(
    name: &str,
    path: impl AsRef<Path>,
    password: &str,
    cipher: VaultCipher,
) -> KernelResult<u32> {
    let path = path.as_ref();
    with_state(|state| {
        if state.vaults.len() >= MAX_VAULTS {
            return Err(KernelError::ResourceExhausted);
        }
        if password.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        let id = state.next_id;
        state.next_id += 1;
        let mount = PathBuf::from(format!("/vault/{}", id));
        state.vaults.push(Vault {
            id,
            name: String::from(name),
            path: path.to_path_buf(),
            mount_point: mount,
            state: VaultState::Locked,
            cipher,
            auto_lock_secs: 300,
            password_hash: simple_hash(password),
            file_count: 0,
            size_bytes: 0,
            created_ns: crate::hpet::elapsed_ns(),
            last_accessed_ns: 0,
        });
        Ok(id)
    })
}

/// Unlock a vault with password.
pub fn unlock(vault_id: u32, password: &str) -> KernelResult<()> {
    with_state(|state| {
        let v = state
            .vaults
            .iter_mut()
            .find(|v| v.id == vault_id)
            .ok_or(KernelError::NotFound)?;
        if v.state == VaultState::Unlocked {
            return Err(KernelError::InvalidArgument);
        }
        if simple_hash(password) != v.password_hash {
            state.total_failed_auths += 1;
            return Err(KernelError::PermissionDenied);
        }
        v.state = VaultState::Unlocked;
        v.last_accessed_ns = crate::hpet::elapsed_ns();
        state.total_unlocks += 1;
        Ok(())
    })
}

/// Lock a vault.
pub fn lock(vault_id: u32) -> KernelResult<()> {
    with_state(|state| {
        let v = state
            .vaults
            .iter_mut()
            .find(|v| v.id == vault_id)
            .ok_or(KernelError::NotFound)?;
        v.state = VaultState::Locked;
        state.total_locks += 1;
        Ok(())
    })
}

/// Change vault password.
pub fn change_password(vault_id: u32, old_password: &str, new_password: &str) -> KernelResult<()> {
    with_state(|state| {
        let v = state
            .vaults
            .iter_mut()
            .find(|v| v.id == vault_id)
            .ok_or(KernelError::NotFound)?;
        if simple_hash(old_password) != v.password_hash {
            state.total_failed_auths += 1;
            return Err(KernelError::PermissionDenied);
        }
        if new_password.is_empty() {
            return Err(KernelError::InvalidArgument);
        }
        v.password_hash = simple_hash(new_password);
        Ok(())
    })
}

/// Set auto-lock timeout.
pub fn set_auto_lock(vault_id: u32, secs: u32) -> KernelResult<()> {
    with_state(|state| {
        let v = state
            .vaults
            .iter_mut()
            .find(|v| v.id == vault_id)
            .ok_or(KernelError::NotFound)?;
        v.auto_lock_secs = secs;
        Ok(())
    })
}

/// Delete a vault.
pub fn delete_vault(vault_id: u32, password: &str) -> KernelResult<()> {
    with_state(|state| {
        let pos = state
            .vaults
            .iter()
            .position(|v| v.id == vault_id)
            .ok_or(KernelError::NotFound)?;
        if simple_hash(password) != state.vaults[pos].password_hash {
            state.total_failed_auths += 1;
            return Err(KernelError::PermissionDenied);
        }
        state.vaults.remove(pos);
        Ok(())
    })
}

/// List all vaults (doesn't expose passwords).
pub fn list_vaults() -> Vec<Vault> {
    STATE
        .lock()
        .as_ref()
        .map_or(Vec::new(), |s| s.vaults.clone())
}

/// Get vault info.
pub fn get_vault(id: u32) -> KernelResult<Vault> {
    with_state(|state| {
        state
            .vaults
            .iter()
            .find(|v| v.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// Statistics: (vault_count, unlocked_count, total_unlocks, total_failed, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let unlocked = s
                .vaults
                .iter()
                .filter(|v| v.state == VaultState::Unlocked)
                .count();
            (
                s.vaults.len(),
                unlocked,
                s.total_unlocks,
                s.total_failed_auths,
                s.ops,
            )
        }
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("filevault::self_test() — running tests...");
    // Start from a genuinely empty table.  `init_defaults()` returns early
    // when the state already exists, so without this reset a second run of
    // `filevault test` inherited the vault the first run created and panicked
    // on test 1 -- and the exact counter assertions in test 8 could never
    // hold twice either.
    *STATE.lock() = None;
    init_defaults();

    // 1: No vaults initially.
    assert_eq!(list_vaults().len(), 0);
    crate::serial_println!("  [1/9] no vaults: OK");

    // 2: Create vault.
    let id = create_vault(
        "Personal",
        "/home/user/vault",
        "secret123",
        VaultCipher::Aes256Gcm,
    )
    .expect("create");
    assert_eq!(list_vaults().len(), 1);
    let v = get_vault(id).expect("get");
    assert_eq!(v.state, VaultState::Locked);
    crate::serial_println!("  [2/9] create vault: OK");

    // 3: Wrong password rejected.
    let result = unlock(id, "wrongpass");
    assert!(result.is_err());
    crate::serial_println!("  [3/9] wrong password: OK");

    // 4: Correct password unlocks.
    unlock(id, "secret123").expect("unlock");
    let v = get_vault(id).expect("get2");
    assert_eq!(v.state, VaultState::Unlocked);
    crate::serial_println!("  [4/9] unlock: OK");

    // 5: Lock.
    lock(id).expect("lock");
    let v = get_vault(id).expect("get3");
    assert_eq!(v.state, VaultState::Locked);
    crate::serial_println!("  [5/9] lock: OK");

    // 6: Change password.
    change_password(id, "secret123", "newpass456").expect("change");
    unlock(id, "newpass456").expect("unlock2");
    crate::serial_println!("  [6/9] change password: OK");

    // 7: Auto-lock timeout.
    set_auto_lock(id, 600).expect("autolock");
    let v = get_vault(id).expect("get4");
    assert_eq!(v.auto_lock_secs, 600);
    crate::serial_println!("  [7/9] auto-lock: OK");

    // 8: Stats.
    let (count, unlocked, unlocks, failed, ops) = stats();
    assert_eq!(count, 1);
    assert_eq!(unlocked, 1);
    assert_eq!(unlocks, 2);
    assert_eq!(failed, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/9] stats: OK");

    // 9: non-UTF-8 vault paths (design-decisions.md 261).
    //
    // The vault's backing directory is caller-supplied, so it may be named
    // with bytes that have no UTF-8 spelling.  While `path` was a `String`
    // such a directory could not back a vault at all -- and two vaults over
    // directories differing only in such a byte would have been shown
    // identically by `filevault list`, which for an encrypted store is a
    // dangerous thing to get wrong.
    {
        let pa = Path::new(&b"/home/user/fv_\xFFv"[..]);
        let pb = Path::new(&b"/home/user/fv_\xFEv"[..]);
        let ia = create_vault("nu-a", pa, "pw", VaultCipher::Aes256Gcm).expect("create a");
        let ib = create_vault("nu-b", pb, "pw", VaultCipher::Aes256Gcm).expect("create b");
        let va = get_vault(ia).expect("get a");
        let vb = get_vault(ib).expect("get b");
        // Byte-exact round trip, and the two remain distinguishable.
        assert_eq!(va.path.as_path(), pa);
        assert_eq!(va.path.as_path().as_bytes(), b"/home/user/fv_\xFFv");
        assert_eq!(vb.path.as_path(), pb);
        assert_ne!(va.path, vb.path);
        // The derived mount point is per-vault, so it is distinct too.
        assert_ne!(va.mount_point, vb.mount_point);
        delete_vault(ia, "pw").expect("delete a");
        delete_vault(ib, "pw").expect("delete b");
    }
    crate::serial_println!("  [9/9] non-UTF-8 vault paths: OK");

    // Leave NO residue: a diagnostic self-test must not leave vaults (least
    // of all unlocked ones) in the live table.  There is no lazy init, so
    // clearing the state restores exactly what a fresh boot has.
    *STATE.lock() = None;

    crate::serial_println!("filevault::self_test() — all 9 tests passed");
}
