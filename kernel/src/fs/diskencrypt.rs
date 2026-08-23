//! Disk encryption management — full-disk and per-partition encryption UI.
//!
//! Manages encryption status, key slots, and recovery keys for
//! encrypted volumes. Provides the settings panel interface for
//! viewing and managing encrypted drives.
//!
//! ## Architecture
//!
//! ```text
//! Settings panel → Security → Disk Encryption
//!   → diskencrypt::list_volumes() / encryption_status()
//!
//! Boot process
//!   → diskencrypt::unlock_volume(id, passphrase) at startup
//!
//! Integration:
//!   → encrypt (lower-level crypto operations)
//!   → partmgr (partition information)
//!   → credentials (key storage)
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

/// Encryption algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptAlgorithm {
    Aes256Xts,
    Aes128Xts,
    Serpent256Xts,
    Twofish256Xts,
    ChaCha20,
}

impl EncryptAlgorithm {
    pub fn label(self) -> &'static str {
        match self {
            Self::Aes256Xts => "AES-256-XTS",
            Self::Aes128Xts => "AES-128-XTS",
            Self::Serpent256Xts => "Serpent-256-XTS",
            Self::Twofish256Xts => "Twofish-256-XTS",
            Self::ChaCha20 => "ChaCha20",
        }
    }
}

/// Key derivation function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kdf {
    Argon2id,
    Pbkdf2,
    Scrypt,
}

impl Kdf {
    pub fn label(self) -> &'static str {
        match self {
            Self::Argon2id => "Argon2id",
            Self::Pbkdf2 => "PBKDF2",
            Self::Scrypt => "scrypt",
        }
    }
}

/// Volume encryption status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeStatus {
    /// Encrypted and locked (needs passphrase).
    Locked,
    /// Encrypted and unlocked.
    Unlocked,
    /// Not encrypted.
    Unencrypted,
    /// Encryption in progress.
    Encrypting,
    /// Decryption in progress.
    Decrypting,
}

impl VolumeStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Locked => "Locked",
            Self::Unlocked => "Unlocked",
            Self::Unencrypted => "Unencrypted",
            Self::Encrypting => "Encrypting...",
            Self::Decrypting => "Decrypting...",
        }
    }
}

/// A key slot in an encrypted volume.
#[derive(Debug, Clone)]
pub struct KeySlot {
    /// Slot index (0-7 typically).
    pub slot: u8,
    /// Whether this slot is active.
    pub active: bool,
    /// KDF used.
    pub kdf: Kdf,
    /// Label (e.g., "main passphrase", "recovery key").
    pub label: String,
}

/// An encrypted volume.
#[derive(Debug, Clone)]
pub struct EncryptedVolume {
    /// Volume ID.
    pub id: u32,
    /// Device path (e.g., "/dev/sda2").
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): this names a
    /// device node, which is an ordinary filesystem entry and so may contain
    /// any byte but `/` and NUL.  (Contrast `iso9660::mount`'s `device`,
    /// which stays a `&str` because it indexes the block-device registry
    /// rather than naming something in a filesystem.)
    pub device: PathBuf,
    /// Volume label.
    pub label: String,
    /// Encryption algorithm.
    pub algorithm: EncryptAlgorithm,
    /// Current status.
    pub status: VolumeStatus,
    /// Size in bytes.
    pub size_bytes: u64,
    /// Key slots.
    pub key_slots: Vec<KeySlot>,
    /// Has a recovery key generated.
    pub has_recovery_key: bool,
    /// Whether TPM is used to seal the key.
    pub tpm_sealed: bool,
    /// Mount point when unlocked.
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): a mount point
    /// is an ordinary directory.
    pub mount_point: PathBuf,
    /// Encryption progress percentage (0-100, for Encrypting/Decrypting).
    pub progress_pct: u8,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    volumes: Vec<EncryptedVolume>,
    next_id: u32,
    unlock_count: u64,
    failed_unlocks: u64,
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

/// Initialise disk encryption manager.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }

    // Default: one system volume, unencrypted.
    let volumes = alloc::vec![EncryptedVolume {
        id: 1,
        device: PathBuf::from("/dev/sda1"),
        label: String::from("System"),
        algorithm: EncryptAlgorithm::Aes256Xts,
        status: VolumeStatus::Unencrypted,
        size_bytes: 512 * 1024 * 1024 * 1024, // 512 GiB
        key_slots: Vec::new(),
        has_recovery_key: false,
        tpm_sealed: false,
        mount_point: PathBuf::from("/"),
        progress_pct: 0,
    },];

    *guard = Some(State {
        volumes,
        next_id: 2,
        unlock_count: 0,
        failed_unlocks: 0,
        ops: 0,
    });
}

/// Register an encrypted volume.
pub fn register_volume(
    device: impl AsRef<Path>,
    label: &str,
    algorithm: EncryptAlgorithm,
    size_bytes: u64,
) -> KernelResult<u32> {
    let device = device.as_ref();
    with_state(|state| {
        let id = state.next_id;
        state.next_id += 1;

        state.volumes.push(EncryptedVolume {
            id,
            device: device.to_path_buf(),
            label: String::from(label),
            algorithm,
            status: VolumeStatus::Locked,
            size_bytes,
            key_slots: alloc::vec![KeySlot {
                slot: 0,
                active: true,
                kdf: Kdf::Argon2id,
                label: String::from("Main passphrase"),
            },],
            has_recovery_key: false,
            tpm_sealed: false,
            mount_point: PathBuf::new(),
            progress_pct: 0,
        });

        Ok(id)
    })
}

/// Unlock a volume (simulated — in reality would verify passphrase).
pub fn unlock_volume(id: u32, _passphrase: &str) -> KernelResult<()> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;

        if vol.status == VolumeStatus::Unencrypted {
            return Err(KernelError::InvalidArgument);
        }
        if vol.status == VolumeStatus::Unlocked {
            return Err(KernelError::AlreadyExists);
        }

        // Simulated passphrase check (in real implementation, derive key and verify).
        if _passphrase.is_empty() {
            state.failed_unlocks += 1;
            return Err(KernelError::PermissionDenied);
        }

        vol.status = VolumeStatus::Unlocked;
        state.unlock_count += 1;
        Ok(())
    })
}

/// Lock a volume.
pub fn lock_volume(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;
        if vol.status != VolumeStatus::Unlocked {
            return Err(KernelError::InvalidArgument);
        }
        vol.status = VolumeStatus::Locked;
        Ok(())
    })
}

/// Add a key slot to a volume.
pub fn add_key_slot(volume_id: u32, kdf: Kdf, label: &str) -> KernelResult<u8> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == volume_id)
            .ok_or(KernelError::NotFound)?;
        if vol.key_slots.len() >= 8 {
            return Err(KernelError::ResourceExhausted);
        }
        let slot_num = vol.key_slots.len() as u8;
        vol.key_slots.push(KeySlot {
            slot: slot_num,
            active: true,
            kdf,
            label: String::from(label),
        });
        Ok(slot_num)
    })
}

/// Remove a key slot.
pub fn remove_key_slot(volume_id: u32, slot: u8) -> KernelResult<()> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == volume_id)
            .ok_or(KernelError::NotFound)?;

        // Must keep at least one active slot.
        let active_count = vol.key_slots.iter().filter(|s| s.active).count();
        let target = vol
            .key_slots
            .iter()
            .find(|s| s.slot == slot)
            .ok_or(KernelError::NotFound)?;
        if target.active && active_count <= 1 {
            return Err(KernelError::InvalidArgument);
        }

        vol.key_slots.retain(|s| s.slot != slot);
        Ok(())
    })
}

/// Generate a recovery key for a volume.
pub fn generate_recovery_key(volume_id: u32) -> KernelResult<String> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == volume_id)
            .ok_or(KernelError::NotFound)?;

        vol.has_recovery_key = true;

        // Add a recovery key slot if not present.
        if !vol.key_slots.iter().any(|s| s.label.contains("Recovery")) {
            let slot_num = vol.key_slots.len() as u8;
            vol.key_slots.push(KeySlot {
                slot: slot_num,
                active: true,
                kdf: Kdf::Pbkdf2,
                label: String::from("Recovery key"),
            });
        }

        // Generate a display-friendly recovery key (simulated).
        let now = crate::hpet::elapsed_ns();
        Ok(format!(
            "{:08X}-{:08X}-{:08X}-{:08X}",
            (now & 0xFFFF_FFFF) as u32,
            ((now >> 16) & 0xFFFF_FFFF) as u32,
            ((now >> 32) & 0xFFFF_FFFF) as u32,
            ((now >> 48) ^ 0xA5A5_A5A5) as u32
        ))
    })
}

/// Start encryption of an unencrypted volume.
pub fn start_encryption(id: u32, algorithm: EncryptAlgorithm) -> KernelResult<()> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;
        if vol.status != VolumeStatus::Unencrypted {
            return Err(KernelError::InvalidArgument);
        }
        vol.algorithm = algorithm;
        vol.status = VolumeStatus::Encrypting;
        vol.progress_pct = 0;

        // Add initial key slot.
        if vol.key_slots.is_empty() {
            vol.key_slots.push(KeySlot {
                slot: 0,
                active: true,
                kdf: Kdf::Argon2id,
                label: String::from("Main passphrase"),
            });
        }

        Ok(())
    })
}

/// Update encryption progress (for in-progress encryption).
pub fn update_progress(id: u32, progress_pct: u8) -> KernelResult<()> {
    with_state(|state| {
        let vol = state
            .volumes
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(KernelError::NotFound)?;
        vol.progress_pct = progress_pct.min(100);
        if progress_pct >= 100 {
            match vol.status {
                VolumeStatus::Encrypting => vol.status = VolumeStatus::Unlocked,
                VolumeStatus::Decrypting => vol.status = VolumeStatus::Unencrypted,
                _ => {}
            }
        }
        Ok(())
    })
}

/// Get volume info.
pub fn get_volume(id: u32) -> KernelResult<EncryptedVolume> {
    with_state(|state| {
        state
            .volumes
            .iter()
            .find(|v| v.id == id)
            .cloned()
            .ok_or(KernelError::NotFound)
    })
}

/// List all volumes.
pub fn list_volumes() -> Vec<EncryptedVolume> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.volumes.clone(),
        None => Vec::new(),
    }
}

/// Format size in human-readable form.
///
/// Delegates to [`crate::bytesize::iec`]. The private copy this replaced had no
/// KiB branch at all — it fell from MiB straight to bytes, so a 1 MiB-minus-one
/// volume header printed as `1048575 B` — and truncated to whole units, so two
/// volumes 900 MiB apart could both report `1 GiB`.
fn format_size(bytes: u64) -> String {
    crate::bytesize::iec(bytes)
}

/// Statistics: (volume_count, encrypted_count, unlocked_count, failed_unlocks, ops).
pub fn stats() -> (usize, usize, usize, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => {
            let encrypted = s
                .volumes
                .iter()
                .filter(|v| v.status != VolumeStatus::Unencrypted)
                .count();
            let unlocked = s
                .volumes
                .iter()
                .filter(|v| v.status == VolumeStatus::Unlocked)
                .count();
            (
                s.volumes.len(),
                encrypted,
                unlocked,
                s.failed_unlocks,
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
    crate::serial_println!("diskencrypt::self_test() — running tests...");

    // Start from the seeded defaults, not from whatever a previous run left.
    // `init_defaults()` returns early when the state exists, so without this
    // reset a second `dencrypt test` panicked at test 9: the first run leaves
    // volume 1 `Unlocked`, and `start_encryption` only accepts `Unencrypted`.
    *STATE.lock() = None;
    init_defaults();

    // Test 1: Default volume present.
    let vols = list_volumes();
    assert!(!vols.is_empty());
    assert_eq!(vols[0].status, VolumeStatus::Unencrypted);
    crate::serial_println!("  [1/12] default volume: OK");

    // Test 2: Register encrypted volume.
    let id = register_volume(
        "/dev/sdb1",
        "Data",
        EncryptAlgorithm::Aes256Xts,
        256 * 1024 * 1024 * 1024,
    )
    .expect("register");
    assert!(id > 0);
    crate::serial_println!("  [2/12] register volume: OK");

    // Test 3: Volume starts locked.
    let vol = get_volume(id).expect("get");
    assert_eq!(vol.status, VolumeStatus::Locked);
    crate::serial_println!("  [3/12] starts locked: OK");

    // Test 4: Unlock with empty passphrase fails.
    let result = unlock_volume(id, "");
    assert!(result.is_err());
    crate::serial_println!("  [4/12] empty passphrase rejected: OK");

    // Test 5: Unlock with passphrase.
    unlock_volume(id, "correct-horse-battery-staple").expect("unlock");
    let vol = get_volume(id).expect("get unlocked");
    assert_eq!(vol.status, VolumeStatus::Unlocked);
    crate::serial_println!("  [5/12] unlock volume: OK");

    // Test 6: Lock volume.
    lock_volume(id).expect("lock");
    let vol = get_volume(id).expect("get locked");
    assert_eq!(vol.status, VolumeStatus::Locked);
    crate::serial_println!("  [6/12] lock volume: OK");

    // Test 7: Add key slot.
    let slot = add_key_slot(id, Kdf::Argon2id, "Backup passphrase").expect("add slot");
    assert_eq!(slot, 1);
    crate::serial_println!("  [7/12] add key slot: OK");

    // Test 8: Generate recovery key.
    let key = generate_recovery_key(id).expect("gen recovery");
    assert!(key.contains('-'));
    let vol = get_volume(id).expect("get with recovery");
    assert!(vol.has_recovery_key);
    crate::serial_println!("  [8/12] recovery key: OK");

    // Test 9: Start encryption on unencrypted volume.
    start_encryption(1, EncryptAlgorithm::Aes256Xts).expect("start encrypt");
    let vol = get_volume(1).expect("get encrypting");
    assert_eq!(vol.status, VolumeStatus::Encrypting);
    crate::serial_println!("  [9/12] start encryption: OK");

    // Test 10: Update progress to completion.
    update_progress(1, 100).expect("complete");
    let vol = get_volume(1).expect("get after encrypt");
    assert_eq!(vol.status, VolumeStatus::Unlocked);
    crate::serial_println!("  [10/12] encryption complete: OK");

    // Test 11: Stats.
    let (total, encrypted, unlocked, failed, ops) = stats();
    assert!(total >= 2);
    assert!(encrypted >= 2);
    assert!(unlocked >= 1);
    assert!(failed >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [11/12] stats: OK");

    // Test 12: non-UTF-8 device nodes (design-decisions.md 261).
    //
    // A device node is an ordinary filesystem entry, so it may be named with
    // bytes that have no UTF-8 spelling.  While `device` was a `String` such
    // a node could not be registered at all, and two volumes on device nodes
    // differing only in such a byte would have been listed identically --
    // which for "which disk am I about to encrypt?" is the worst possible
    // ambiguity.
    {
        let da = Path::new(&b"/dev/de_\xFFd"[..]);
        let db = Path::new(&b"/dev/de_\xFEd"[..]);
        let ia = register_volume(da, "nu-a", EncryptAlgorithm::Aes256Xts, 1024)
            .expect("register non-utf8 a");
        let ib = register_volume(db, "nu-b", EncryptAlgorithm::Aes256Xts, 1024)
            .expect("register non-utf8 b");
        let va = get_volume(ia).expect("get non-utf8 a");
        let vb = get_volume(ib).expect("get non-utf8 b");
        assert_eq!(va.device.as_path(), da);
        assert_eq!(va.device.as_path().as_bytes(), b"/dev/de_\xFFd");
        assert_eq!(vb.device.as_path(), db);
        assert_ne!(va.device, vb.device);
    }
    crate::serial_println!("  [12/12] non-UTF-8 device nodes: OK");

    // Leave NO residue: the tests above encrypt the seeded system volume and
    // register several more.  Clearing the state restores exactly what a
    // fresh boot has -- there is no lazy init, so nothing re-populates it
    // until a caller asks for `init_defaults()`.
    *STATE.lock() = None;

    crate::serial_println!("diskencrypt::self_test() — all 12 tests passed");
}
