//! Disk encryption: encrypted volumes, their key slots, and the keys behind
//! them.
//!
//! ## The keys
//!
//! A volume is encrypted under a random **master key** ([`MASTER_KEY_LEN`]
//! bytes: AES-256-XTS takes two 256-bit keys). The master key is never kept
//! in the clear except while the volume is unlocked. Each **key slot** seals
//! it under a key derived from one secret -- a passphrase, or a recovery key
//! -- as LUKS2's keyslots do:
//!
//! - the secret goes through Argon2id with the slot's own random salt and cost
//!   (`seal::derive_key`, on the vendored RustCrypto `argon2`: design-decisions
//!   §539 -- the primitives are ported, not written here);
//! - the derived key seals the master key with XChaCha20-Poly1305
//!   (`seal::encrypt`) under the slot's random nonce, with the volume's
//!   identity and the slot's number as associated data, so a slot copied to
//!   another volume, or to another position, does not open there.
//!
//! **Unlocking** tries the secret against each slot. The slot whose seal opens
//! gives the master key, which the volume holds in memory -- wiped when it is
//! locked again -- while it is unlocked. A wrong secret opens no seal: the
//! cipher's authentication tag is the check, so nothing derived from a secret
//! is stored to compare against. The derivation is meant to be slow (it is
//! what makes guessing expensive), so it runs with no lock held.
//!
//! **Adding a slot**, or a recovery key, needs the master key -- the volume
//! unlocked -- as `cryptsetup luksAddKey` needs an existing passphrase. The
//! last slot cannot be removed: the data would go with it.
//!
//! A **recovery key** is 256 random bits, shown once as eight groups of eight
//! hex digits and given its own slot. A newer one replaces the older.
//!
//! ## What is not here yet
//!
//! Nothing reads or writes a volume's data through its key: there is no
//! encrypting block layer under the mount path (design-decisions §978's second
//! step; known-issues `A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER`). So volumes and
//! their slots live in memory -- a volume header on the disk is the block
//! layer's to read and write -- and the progress of an in-place encryption is
//! whatever its caller reports through [`update_progress`].

use crate::error::{KernelError, KernelResult};
use crate::fs::path::{Path, PathBuf};
use crate::sync::PreemptSpinMutex as Mutex;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

pub use seal::KdfParams;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Bytes in a master key: AES-256-XTS's two 256-bit keys.
pub const MASTER_KEY_LEN: usize = 64;

/// Key slots a volume may have, as LUKS1 had.
pub const MAX_SLOTS: usize = 8;

/// What opening a passphrase slot costs: Argon2id at OWASP's recommended
/// floor for it (also the `argon2` crate's default) -- 19 MiB, two passes,
/// one lane. `seal::KdfParams::RECOMMENDED` (64 MiB in four lanes) is sized
/// for a desktop process; this runs in the kernel, whose memory is the
/// machine's.
pub const PASSPHRASE_COST: KdfParams = KdfParams {
    memory_kib: 19 * 1024,
    iterations: 2,
    lanes: 1,
};

/// What opening a recovery-key slot costs: the least Argon2 allows. A
/// recovery key is 256 random bits, which no amount of stretching makes
/// harder to guess.
pub const RECOVERY_COST: KdfParams = KdfParams {
    memory_kib: 8,
    iterations: 1,
    lanes: 1,
};

/// How long making a key waits for the random-number generator to have been
/// seeded before refusing: a key drawn before then could be guessed.
const RNG_WAIT_NS: u64 = 5_000_000_000;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Encryption algorithm a volume's data is (to be) encrypted with -- recorded
/// for the block layer, which does not exist yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptAlgorithm {
    Aes256Xts,
    Aes128Xts,
    Serpent256Xts,
    Twofish256Xts,
    ChaCha20,
}

impl EncryptAlgorithm {
    /// The algorithm's name, as `cryptsetup` prints it.
    #[must_use]
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

/// A volume's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeStatus {
    /// Encrypted and locked: a secret must open a slot.
    Locked,
    /// Encrypted, with its master key in memory.
    Unlocked,
    /// Not encrypted.
    Unencrypted,
    /// Being encrypted in place (its master key in memory).
    Encrypting,
    /// Being decrypted in place.
    // Entered by in-place decryption, which waits on the block layer
    // (known-issues A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER); `update_progress`
    // already completes it, so the state is real API, just not yet reached.
    #[allow(dead_code)]
    Decrypting,
}

impl VolumeStatus {
    /// The state's name, for `/proc/diskencrypt` and the shell.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Locked => "Locked",
            Self::Unlocked => "Unlocked",
            Self::Unencrypted => "Unencrypted",
            Self::Encrypting => "Encrypting",
            Self::Decrypting => "Decrypting",
        }
    }
}

/// A key slot, as it is shown -- what it opens with is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeySlot {
    /// The slot's number.
    pub slot: u8,
    /// What it is ("Main passphrase", "Recovery key", ...).
    pub label: String,
    /// Whether it holds the recovery key.
    pub recovery: bool,
    /// What opening it costs.
    pub cost: KdfParams,
}

/// A volume, as it is shown: no key material.
#[derive(Debug, Clone)]
pub struct EncryptedVolume {
    /// Volume ID.
    pub id: u32,
    /// Device path (e.g., "/dev/vdb").
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): this names a
    /// device node, which is an ordinary filesystem entry and so may contain
    /// any byte but `/` and NUL.
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
    /// Whether a recovery key has a slot.
    pub has_recovery_key: bool,
    /// Mount point when unlocked; empty when none.
    ///
    /// A `PathBuf`, not a `String` (design-decisions.md 261): a mount point
    /// is an ordinary directory.
    pub mount_point: PathBuf,
    /// Encryption progress percentage (0-100, for Encrypting/Decrypting).
    // Read by nobody yet: `/proc/diskencrypt` and the shell report it once
    // in-place encryption exists (known-issues
    // A-DISK-ENCRYPTION-HAS-NO-BLOCK-LAYER).
    #[allow(dead_code)]
    pub progress_pct: u8,
}

/// Key material, wiped when it is dropped.
struct Secret<const N: usize>([u8; N]);

impl<const N: usize> Secret<N> {
    fn zeroed() -> Self {
        Self([0; N])
    }
}

impl<const N: usize> Drop for Secret<N> {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// Overwrite `bytes` with zeros in a way the compiler may not drop as dead.
fn wipe(bytes: &mut [u8]) {
    for b in bytes.iter_mut() {
        // SAFETY: `b` is a valid, aligned, exclusive reference to one byte.
        unsafe { core::ptr::write_volatile(b, 0) };
    }
    core::sync::atomic::compiler_fence(Ordering::SeqCst);
}

/// A key slot with what it opens with.
#[derive(Clone)]
struct Slot {
    number: u8,
    label: String,
    recovery: bool,
    cost: KdfParams,
    salt: [u8; seal::SALT_LEN],
    nonce: seal::Nonce,
    /// The master key, sealed: [`MASTER_KEY_LEN`] + `seal::TAG_LEN` bytes.
    sealed: Vec<u8>,
}

impl Slot {
    fn shown(&self) -> KeySlot {
        KeySlot {
            slot: self.number,
            label: self.label.clone(),
            recovery: self.recovery,
            cost: self.cost,
        }
    }
}

/// A volume with its secrets.
struct Volume {
    id: u32,
    /// Random, bound into every slot's seal.
    uuid: [u8; 16],
    device: PathBuf,
    label: String,
    algorithm: EncryptAlgorithm,
    status: VolumeStatus,
    size_bytes: u64,
    slots: Vec<Slot>,
    mount_point: PathBuf,
    progress_pct: u8,
    /// The master key, while the volume is unlocked (or being encrypted).
    master: Option<Secret<MASTER_KEY_LEN>>,
}

impl Volume {
    fn shown(&self) -> EncryptedVolume {
        EncryptedVolume {
            id: self.id,
            device: self.device.clone(),
            label: self.label.clone(),
            algorithm: self.algorithm,
            status: self.status,
            size_bytes: self.size_bytes,
            key_slots: self.slots.iter().map(Slot::shown).collect(),
            has_recovery_key: self.slots.iter().any(|s| s.recovery),
            mount_point: self.mount_point.clone(),
            progress_pct: self.progress_pct,
        }
    }

    /// The lowest slot number not in use.
    fn free_slot_number(&self) -> KernelResult<u8> {
        (0..MAX_SLOTS)
            .filter_map(|n| u8::try_from(n).ok())
            .find(|n| self.slots.iter().all(|s| s.number != *n))
            .ok_or(KernelError::ResourceExhausted)
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    volumes: Vec<Volume>,
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
    state.ops = state.ops.saturating_add(1);
    OPS.store(state.ops, Ordering::Relaxed);
    f(state)
}

fn volume_mut(state: &mut State, id: u32) -> KernelResult<&mut Volume> {
    state
        .volumes
        .iter_mut()
        .find(|v| v.id == id)
        .ok_or(KernelError::NotFound)
}

// ---------------------------------------------------------------------------
// The keys
// ---------------------------------------------------------------------------

/// Fill `buf` from the kernel's random-number generator, once it has been
/// seeded.
///
/// # Errors
///
/// `WouldBlock` if it is still not seeded after [`RNG_WAIT_NS`].
fn random(buf: &mut [u8]) -> KernelResult<()> {
    if !crate::rng::is_ready() && !crate::rng::wait_until_ready(RNG_WAIT_NS) {
        return Err(KernelError::WouldBlock);
    }
    crate::rng::fill(buf);
    Ok(())
}

/// The associated data a slot's seal is bound to: the volume's identity and
/// the slot's number.
fn slot_aad(uuid: &[u8; 16], number: u8) -> [u8; 17] {
    let mut aad = [0u8; 17];
    if let Some(head) = aad.get_mut(..16) {
        head.copy_from_slice(uuid);
    }
    if let Some(last) = aad.get_mut(16) {
        *last = number;
    }
    aad
}

/// A kernel error for `seal`'s refusal of parameters or memory.
fn seal_error(e: seal::Error) -> KernelError {
    match e {
        seal::Error::OutOfMemory => KernelError::OutOfMemory,
        _ => KernelError::InvalidArgument,
    }
}

/// Seal `master` under `secret` as slot `number` of the volume `uuid`.
/// Slow by design (Argon2id): call with no lock held.
fn make_slot(
    uuid: &[u8; 16],
    number: u8,
    label: &str,
    recovery: bool,
    secret: &[u8],
    cost: KdfParams,
    master: &Secret<MASTER_KEY_LEN>,
) -> KernelResult<Slot> {
    let mut salt = [0u8; seal::SALT_LEN];
    let mut nonce = [0u8; seal::NONCE_LEN];
    random(&mut salt)?;
    random(&mut nonce)?;
    let kek = Secret(seal::derive_key(secret, &salt, cost).map_err(seal_error)?);
    let sealed =
        seal::encrypt(&kek.0, &nonce, &slot_aad(uuid, number), &master.0).map_err(seal_error)?;
    Ok(Slot {
        number,
        label: String::from(label),
        recovery,
        cost,
        salt,
        nonce,
        sealed,
    })
}

/// Try `secret` against `slot` of the volume `uuid`: the master key if its
/// seal opens. Slow by design (Argon2id): call with no lock held.
///
/// # Errors
///
/// `OutOfMemory` when the slot's cost cannot be met; `InvalidArgument` for a
/// cost or secret Argon2 refuses.
fn open_slot(
    uuid: &[u8; 16],
    slot: &Slot,
    secret: &[u8],
) -> KernelResult<Option<Secret<MASTER_KEY_LEN>>> {
    let kek = Secret(seal::derive_key(secret, &slot.salt, slot.cost).map_err(seal_error)?);
    let Ok(mut opened) = seal::decrypt(
        &kek.0,
        &slot.nonce,
        &slot_aad(uuid, slot.number),
        &slot.sealed,
    ) else {
        return Ok(None);
    };
    let mut master = Secret::<MASTER_KEY_LEN>::zeroed();
    let fits = opened.len() == MASTER_KEY_LEN;
    if fits {
        master.0.copy_from_slice(&opened);
    }
    wipe(&mut opened);
    Ok(fits.then_some(master))
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Open the volume table, empty -- every volume is registered by whoever
/// finds or formats it. A no-op once open.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() {
        return;
    }
    *guard = Some(State {
        volumes: Vec::new(),
        next_id: 1,
        unlock_count: 0,
        failed_unlocks: 0,
        ops: 0,
    });
}

/// Register a volume that is not encrypted (yet).
///
/// # Errors
///
/// `NotSupported` if the table is not open.
pub fn register_plain_volume(
    device: impl AsRef<Path>,
    label: &str,
    size_bytes: u64,
) -> KernelResult<u32> {
    let device = device.as_ref().to_path_buf();
    with_state(|state| {
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        state.volumes.push(Volume {
            id,
            uuid: [0; 16],
            device,
            label: String::from(label),
            algorithm: EncryptAlgorithm::Aes256Xts,
            status: VolumeStatus::Unencrypted,
            size_bytes,
            slots: Vec::new(),
            mount_point: PathBuf::new(),
            progress_pct: 0,
            master: None,
        });
        Ok(id)
    })
}

/// Register an encrypted volume, as `cryptsetup luksFormat` makes one: a new
/// random master key, sealed in slot 0 under `passphrase` at `cost`
/// ([`PASSPHRASE_COST`] unless there is a reason). The volume starts locked.
///
/// # Errors
///
/// `InvalidArgument` for an empty passphrase or a cost Argon2 refuses;
/// `OutOfMemory` when the cost cannot be met; `WouldBlock` when the
/// random-number generator is not seeded; `NotSupported` if the table is not
/// open.
pub fn register_volume(
    device: impl AsRef<Path>,
    label: &str,
    algorithm: EncryptAlgorithm,
    size_bytes: u64,
    passphrase: &[u8],
    cost: KdfParams,
) -> KernelResult<u32> {
    if passphrase.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    let mut uuid = [0u8; 16];
    random(&mut uuid)?;
    let mut master = Secret::<MASTER_KEY_LEN>::zeroed();
    random(&mut master.0)?;
    let slot = make_slot(
        &uuid,
        0,
        "Main passphrase",
        false,
        passphrase,
        cost,
        &master,
    )?;
    drop(master);
    let device = device.as_ref().to_path_buf();
    with_state(|state| {
        let id = state.next_id;
        state.next_id = state.next_id.saturating_add(1);
        state.volumes.push(Volume {
            id,
            uuid,
            device,
            label: String::from(label),
            algorithm,
            status: VolumeStatus::Locked,
            size_bytes,
            slots: alloc::vec![slot],
            mount_point: PathBuf::new(),
            progress_pct: 0,
            master: None,
        });
        Ok(id)
    })
}

/// Unlock volume `id` with `secret` -- a passphrase or a recovery key -- by
/// opening the first slot it opens.
///
/// # Errors
///
/// `PermissionDenied` when it opens none (counted in the failures);
/// `NotFound`; `InvalidArgument` for a volume that is not encrypted;
/// `AlreadyExists` for one already unlocked; `OutOfMemory` when a slot's cost
/// cannot be met.
pub fn unlock_volume(id: u32, secret: &[u8]) -> KernelResult<()> {
    let (uuid, slots) = with_state(|state| {
        let vol = volume_mut(state, id)?;
        match vol.status {
            VolumeStatus::Unencrypted => Err(KernelError::InvalidArgument),
            VolumeStatus::Unlocked | VolumeStatus::Encrypting => Err(KernelError::AlreadyExists),
            VolumeStatus::Locked | VolumeStatus::Decrypting => Ok((vol.uuid, vol.slots.clone())),
        }
    })?;
    // The derivations run unlocked: each takes as long as its cost says.
    let mut opened = None;
    if !secret.is_empty() {
        for slot in &slots {
            if let Some(master) = open_slot(&uuid, slot, secret)? {
                opened = Some(master);
                break;
            }
        }
    }
    with_state(|state| {
        let Some(master) = opened else {
            state.failed_unlocks = state.failed_unlocks.saturating_add(1);
            return Err(KernelError::PermissionDenied);
        };
        let vol = volume_mut(state, id)?;
        // Re-check: the volume may have changed while the key was derived.
        if vol.uuid != uuid {
            return Err(KernelError::NotFound);
        }
        if vol.master.is_none() {
            vol.master = Some(master);
        }
        if vol.status == VolumeStatus::Locked {
            vol.status = VolumeStatus::Unlocked;
        }
        state.unlock_count = state.unlock_count.saturating_add(1);
        Ok(())
    })
}

/// Lock volume `id`: its master key is wiped from memory.
///
/// # Errors
///
/// `NotFound`; `InvalidArgument` unless it is unlocked.
pub fn lock_volume(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let vol = volume_mut(state, id)?;
        if vol.status != VolumeStatus::Unlocked {
            return Err(KernelError::InvalidArgument);
        }
        vol.status = VolumeStatus::Locked;
        vol.master = None;
        Ok(())
    })
}

/// The master key of the unlocked volume `id`, copied out with its identity,
/// for a slot to be sealed with no lock held.
fn master_of(id: u32) -> KernelResult<([u8; 16], Secret<MASTER_KEY_LEN>)> {
    with_state(|state| {
        let vol = volume_mut(state, id)?;
        let master = vol.master.as_ref().ok_or(KernelError::PermissionDenied)?;
        let mut copy = Secret::<MASTER_KEY_LEN>::zeroed();
        copy.0.copy_from_slice(&master.0);
        Ok((vol.uuid, copy))
    })
}

/// Seal the unlocked volume's master key in a new slot and add it.
fn add_slot(
    id: u32,
    label: &str,
    recovery: bool,
    secret: &[u8],
    cost: KdfParams,
) -> KernelResult<u8> {
    let (uuid, master) = master_of(id)?;
    let number = with_state(|state| {
        let vol = volume_mut(state, id)?;
        if recovery {
            // A newer recovery key replaces the older: its slot is reused.
            if let Some(old) = vol.slots.iter().find(|s| s.recovery) {
                return Ok(old.number);
            }
        }
        vol.free_slot_number()
    })?;
    let slot = make_slot(&uuid, number, label, recovery, secret, cost, &master)?;
    drop(master);
    with_state(|state| {
        let vol = volume_mut(state, id)?;
        if vol.uuid != uuid {
            return Err(KernelError::NotFound);
        }
        vol.slots.retain(|s| s.number != number);
        if vol.slots.len() >= MAX_SLOTS {
            return Err(KernelError::ResourceExhausted);
        }
        vol.slots.push(slot);
        vol.slots.sort_by_key(|s| s.number);
        Ok(number)
    })
}

/// Add a key slot opening the unlocked volume `id` with `passphrase`, as
/// `cryptsetup luksAddKey` does.
///
/// # Errors
///
/// `PermissionDenied` unless the volume is unlocked (its master key in
/// hand); `InvalidArgument` for an empty passphrase; `ResourceExhausted` when
/// all [`MAX_SLOTS`] are in use; `NotFound`; as [`register_volume`] for the
/// derivation.
pub fn add_key_slot(
    volume_id: u32,
    passphrase: &[u8],
    label: &str,
    cost: KdfParams,
) -> KernelResult<u8> {
    if passphrase.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    add_slot(volume_id, label, false, passphrase, cost)
}

/// Remove a key slot.
///
/// # Errors
///
/// `NotFound`; `InvalidArgument` for the last slot -- the data would be lost
/// with it.
pub fn remove_key_slot(volume_id: u32, slot: u8) -> KernelResult<()> {
    with_state(|state| {
        let vol = volume_mut(state, volume_id)?;
        if !vol.slots.iter().any(|s| s.number == slot) {
            return Err(KernelError::NotFound);
        }
        if vol.slots.len() <= 1 {
            return Err(KernelError::InvalidArgument);
        }
        vol.slots.retain(|s| s.number != slot);
        Ok(())
    })
}

/// Make a recovery key for the unlocked volume `volume_id`: 256 random bits,
/// returned once as eight groups of eight hex digits, and sealed in a slot of
/// their own (replacing an older recovery key's). Unlock with the string as
/// it is returned.
///
/// # Errors
///
/// As [`add_key_slot`]; `WouldBlock` when the random-number generator is not
/// seeded.
pub fn generate_recovery_key(volume_id: u32) -> KernelResult<String> {
    let mut raw = Secret::<32>::zeroed();
    random(&mut raw.0)?;
    let mut key = String::with_capacity(71);
    for (i, word) in raw.0.chunks(4).enumerate() {
        if i > 0 {
            key.push('-');
        }
        for b in word {
            key.push_str(&format!("{b:02X}"));
        }
    }
    let added = add_slot(
        volume_id,
        "Recovery key",
        true,
        key.as_bytes(),
        RECOVERY_COST,
    );
    match added {
        Ok(_) => Ok(key),
        Err(e) => {
            // SAFETY: zeros are valid UTF-8, so the string stays a string.
            wipe(unsafe { key.as_bytes_mut() });
            Err(e)
        }
    }
}

/// Start encrypting the unencrypted volume `id` in place under a new master
/// key, sealed in slot 0 under `passphrase`. The volume holds its master key
/// while it is encrypted; [`update_progress`] reports how far it has got.
///
/// # Errors
///
/// `InvalidArgument` unless the volume is unencrypted, or for an empty
/// passphrase; `NotFound`; as [`register_volume`] for the derivation.
pub fn start_encryption(
    id: u32,
    algorithm: EncryptAlgorithm,
    passphrase: &[u8],
    cost: KdfParams,
) -> KernelResult<()> {
    if passphrase.is_empty() {
        return Err(KernelError::InvalidArgument);
    }
    with_state(|state| {
        let vol = volume_mut(state, id)?;
        if vol.status == VolumeStatus::Unencrypted {
            Ok(())
        } else {
            Err(KernelError::InvalidArgument)
        }
    })?;
    let mut uuid = [0u8; 16];
    random(&mut uuid)?;
    let mut master = Secret::<MASTER_KEY_LEN>::zeroed();
    random(&mut master.0)?;
    let slot = make_slot(
        &uuid,
        0,
        "Main passphrase",
        false,
        passphrase,
        cost,
        &master,
    )?;
    with_state(|state| {
        let vol = volume_mut(state, id)?;
        if vol.status != VolumeStatus::Unencrypted {
            return Err(KernelError::InvalidArgument);
        }
        vol.uuid = uuid;
        vol.algorithm = algorithm;
        vol.status = VolumeStatus::Encrypting;
        vol.progress_pct = 0;
        vol.slots = alloc::vec![slot];
        vol.master = Some(master);
        Ok(())
    })
}

/// Report how far an in-place encryption or decryption has got; at 100 an
/// encrypted volume is unlocked, a decrypted one unencrypted (its slots and
/// key gone).
///
/// # Errors
///
/// `NotFound`.
pub fn update_progress(id: u32, progress_pct: u8) -> KernelResult<()> {
    with_state(|state| {
        let vol = volume_mut(state, id)?;
        vol.progress_pct = progress_pct.min(100);
        if progress_pct >= 100 {
            match vol.status {
                VolumeStatus::Encrypting => vol.status = VolumeStatus::Unlocked,
                VolumeStatus::Decrypting => {
                    vol.status = VolumeStatus::Unencrypted;
                    vol.slots.clear();
                    vol.master = None;
                }
                _ => {}
            }
        }
        Ok(())
    })
}

/// Volume `id`, as it is shown.
///
/// # Errors
///
/// `NotFound`; `NotSupported` if the table is not open.
pub fn get_volume(id: u32) -> KernelResult<EncryptedVolume> {
    with_state(|state| volume_mut(state, id).map(|v| v.shown()))
}

/// Every volume, as it is shown.
#[must_use]
pub fn list_volumes() -> Vec<EncryptedVolume> {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => s.volumes.iter().map(Volume::shown).collect(),
        None => Vec::new(),
    }
}

/// Statistics: (volume_count, encrypted_count, unlocked_count, failed_unlocks, ops).
#[must_use]
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

/// The keys, end to end, at the least Argon2 cost (the derivation is the
/// vendored crate's, tested by its own known-answer vectors; what is tested
/// here is how this module uses it): a volume that opens with its passphrase
/// and not another, a second slot, a recovery key, a slot moved to another
/// volume refusing to open, the last slot kept, an in-place encryption, and
/// non-UTF-8 device names.
///
/// # Errors
///
/// `InternalError` naming the first check that failed.
#[allow(clippy::too_many_lines)] // one linear script; splitting it hides the order
pub fn self_test() -> crate::error::KernelResult<()> {
    crate::serial_println!("diskencrypt::self_test() — running tests...");
    // Start from an empty table, not from whatever a previous run left.
    *STATE.lock() = None;
    init_defaults();
    let result = run_self_test();
    // Leave the table EMPTY, not DEAD (known-issues
    // A-FS-ACCOUNTING-TABLES-ARE-CLOSED-FOR-THE-WHOLE-BOOT).
    *STATE.lock() = None;
    init_defaults();
    if let Err(why) = result {
        crate::serial_println!("  FAIL: {}", why);
        return Err(KernelError::InternalError);
    }
    crate::serial_println!("diskencrypt::self_test() — all tests passed");
    Ok(())
}

fn run_self_test() -> Result<(), &'static str> {
    /// The least Argon2 allows: what is tested is this module, not the hash.
    const CHEAP: KdfParams = KdfParams {
        memory_kib: 8,
        iterations: 1,
        lanes: 1,
    };
    let status = |id| {
        get_volume(id)
            .map(|v| v.status)
            .map_err(|_| "a volume went missing")
    };

    if !list_volumes().is_empty() {
        return Err("the table did not start empty");
    }
    let a = register_volume(
        "/dev/vdx",
        "Data",
        EncryptAlgorithm::Aes256Xts,
        1 << 30,
        b"correct horse",
        CHEAP,
    )
    .map_err(|_| "a volume could not be registered")?;
    if status(a)? != VolumeStatus::Locked || get_volume(a).map(|v| v.key_slots.len()) != Ok(1) {
        return Err("a new volume is not locked with one slot");
    }
    if unlock_volume(a, b"battery staple") != Err(KernelError::PermissionDenied)
        || unlock_volume(a, b"") != Err(KernelError::PermissionDenied)
        || status(a)? != VolumeStatus::Locked
    {
        return Err("a wrong or empty passphrase unlocked a volume");
    }
    unlock_volume(a, b"correct horse").map_err(|_| "the right passphrase did not unlock")?;
    if status(a)? != VolumeStatus::Unlocked {
        return Err("an unlocked volume does not say so");
    }
    if add_key_slot(a, b"second", "Backup", CHEAP) != Ok(1) {
        return Err("a second slot was not added as slot 1");
    }
    lock_volume(a).map_err(|_| "lock failed")?;
    if add_key_slot(a, b"third", "No", CHEAP) != Err(KernelError::PermissionDenied) {
        return Err("a slot was added to a locked volume");
    }
    unlock_volume(a, b"second").map_err(|_| "the second slot's passphrase did not unlock")?;
    let recovery = generate_recovery_key(a).map_err(|_| "no recovery key")?;
    if recovery.len() != 71 || recovery.split('-').count() != 8 {
        return Err("a recovery key is not eight groups of eight hex digits");
    }
    if !get_volume(a).is_ok_and(|v| v.has_recovery_key) {
        return Err("a volume with a recovery key does not say so");
    }
    lock_volume(a).map_err(|_| "lock failed")?;
    unlock_volume(a, recovery.as_bytes()).map_err(|_| "the recovery key did not unlock")?;
    lock_volume(a).map_err(|_| "lock failed")?;

    // A slot moved to another volume does not open there: its seal is bound
    // to the volume's identity.
    let b = register_volume(
        "/dev/vdy",
        "Other",
        EncryptAlgorithm::Aes256Xts,
        1 << 20,
        b"b-pass",
        CHEAP,
    )
    .map_err(|_| "a second volume could not be registered")?;
    with_state(|state| {
        let moved = volume_mut(state, a)?
            .slots
            .first()
            .cloned()
            .ok_or(KernelError::NotFound)?;
        let vb = volume_mut(state, b)?;
        let mut moved = moved;
        moved.number = 5;
        vb.slots.push(moved);
        Ok(())
    })
    .map_err(|_| "the moved slot could not be planted")?;
    if unlock_volume(b, b"correct horse") != Err(KernelError::PermissionDenied) {
        return Err("a slot copied from another volume opened");
    }

    // The last slot stays. Slot 1 is not b's (NotFound, ignored: only the
    // count left matters here); slot 5 is the planted one.
    for n in [1u8, 5] {
        let _ = remove_key_slot(b, n);
    }
    if remove_key_slot(b, 0) != Err(KernelError::InvalidArgument) {
        return Err("the last slot was removed");
    }

    // An in-place encryption.
    let plain =
        register_plain_volume("/dev/vdz", "Plain", 1 << 20).map_err(|_| "register_plain failed")?;
    start_encryption(plain, EncryptAlgorithm::Aes256Xts, b"inplace", CHEAP)
        .map_err(|_| "start_encryption failed")?;
    if status(plain)? != VolumeStatus::Encrypting {
        return Err("an encryption did not start");
    }
    update_progress(plain, 100).map_err(|_| "update_progress failed")?;
    if status(plain)? != VolumeStatus::Unlocked {
        return Err("a finished encryption did not leave the volume unlocked");
    }
    lock_volume(plain).map_err(|_| "lock failed")?;
    unlock_volume(plain, b"inplace").map_err(|_| "an encrypted-in-place volume did not unlock")?;

    // Non-UTF-8 device nodes (design-decisions.md 261): a device node is an
    // ordinary filesystem entry, so its name may have no UTF-8 spelling, and
    // two differing only in such a byte must stay two.
    let da = Path::new(&b"/dev/de_\xFFd"[..]);
    let db = Path::new(&b"/dev/de_\xFEd"[..]);
    let ia = register_plain_volume(da, "nu-a", 1024).map_err(|_| "non-UTF-8 a")?;
    let ib = register_plain_volume(db, "nu-b", 1024).map_err(|_| "non-UTF-8 b")?;
    let va = get_volume(ia).map_err(|_| "non-UTF-8 a missing")?;
    let vb = get_volume(ib).map_err(|_| "non-UTF-8 b missing")?;
    if va.device.as_path() != da || vb.device.as_path() != db || va.device == vb.device {
        return Err("non-UTF-8 device names were not kept apart");
    }

    let (total, encrypted, unlocked, failed, _) = stats();
    if total != 5 || encrypted != 3 || unlocked != 1 || failed < 3 {
        return Err("the statistics do not add up");
    }
    Ok(())
}
