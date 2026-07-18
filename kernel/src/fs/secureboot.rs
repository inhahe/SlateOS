//! Secure Boot — secure boot verification and key management.
//!
//! Manages secure boot state, enrolled keys, and boot
//! verification status for the system.
//!
//! ## Architecture
//!
//! ```text
//! Boot verification
//!   → secureboot::verify_image(hash) → check signature
//!   → secureboot::enroll_key(key) → add trusted key
//!   → secureboot::get_status() → boot state
//!
//! Integration:
//!   → bootcfg (boot configuration)
//!   → certmgr (certificate management)
//!   → diskencrypt (disk encryption)
//!   → kernelbuild (kernel build)
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

/// Secure boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootState {
    Disabled,
    SetupMode,
    Enabled,
    EnforcingStrict,
}

impl BootState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::SetupMode => "Setup Mode",
            Self::Enabled => "Enabled",
            Self::EnforcingStrict => "Enforcing (Strict)",
        }
    }
}

/// Key type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    PlatformKey,       // PK.
    KeyExchangeKey,    // KEK.
    SignatureDatabase, // db.
    ForbiddenSignature, // dbx.
    MachineOwnerKey,   // MOK.
}

impl KeyType {
    pub fn label(self) -> &'static str {
        match self {
            Self::PlatformKey => "PK",
            Self::KeyExchangeKey => "KEK",
            Self::SignatureDatabase => "db",
            Self::ForbiddenSignature => "dbx",
            Self::MachineOwnerKey => "MOK",
        }
    }
}

/// An enrolled key.
#[derive(Debug, Clone)]
pub struct EnrolledKey {
    pub id: u32,
    pub key_type: KeyType,
    pub subject: String,
    pub fingerprint: String,
    pub enrolled_ns: u64,
}

/// A boot verification record.
#[derive(Debug, Clone)]
pub struct VerifyRecord {
    pub image_name: String,
    pub hash: String,
    pub verified: bool,
    pub key_id: Option<u32>,
    pub timestamp_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_KEYS: usize = 100;
const MAX_RECORDS: usize = 200;

struct State {
    boot_state: BootState,
    keys: Vec<EnrolledKey>,
    records: Vec<VerifyRecord>,
    next_key_id: u32,
    total_verified: u64,
    total_rejected: u64,
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
        boot_state: BootState::Disabled,
        keys: alloc::vec![
            EnrolledKey { id: 1, key_type: KeyType::PlatformKey, subject: String::from("OS Vendor PK"), fingerprint: String::from("SHA256:aabb..."), enrolled_ns: now },
            EnrolledKey { id: 2, key_type: KeyType::KeyExchangeKey, subject: String::from("OS Vendor KEK"), fingerprint: String::from("SHA256:ccdd..."), enrolled_ns: now },
            EnrolledKey { id: 3, key_type: KeyType::SignatureDatabase, subject: String::from("Kernel Signing Key"), fingerprint: String::from("SHA256:eeff..."), enrolled_ns: now },
        ],
        records: Vec::new(),
        next_key_id: 4,
        total_verified: 0,
        total_rejected: 0,
        ops: 0,
    });
}

/// Set boot state.
pub fn set_state(state_val: BootState) -> KernelResult<()> {
    with_state(|state| {
        state.boot_state = state_val;
        Ok(())
    })
}

/// Get boot state.
pub fn get_state() -> BootState {
    STATE.lock().as_ref().map_or(BootState::Disabled, |s| s.boot_state)
}

/// Enroll a key.
pub fn enroll_key(key_type: KeyType, subject: &str, fingerprint: &str) -> KernelResult<u32> {
    with_state(|state| {
        if state.keys.len() >= MAX_KEYS {
            return Err(KernelError::ResourceExhausted);
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_key_id;
        state.next_key_id += 1;
        state.keys.push(EnrolledKey {
            id, key_type, subject: String::from(subject),
            fingerprint: String::from(fingerprint), enrolled_ns: now,
        });
        Ok(id)
    })
}

/// Remove a key.
pub fn remove_key(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.keys.len();
        state.keys.retain(|k| k.id != id);
        if state.keys.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Verify a boot image. Simulates checking against enrolled keys.
pub fn verify_image(image_name: &str, hash: &str) -> KernelResult<bool> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        // Simulate: if boot state is disabled, everything passes.
        // If enabled, check if we have a matching db key (by convention, match if any db key exists).
        let has_db_key = state.keys.iter().any(|k| k.key_type == KeyType::SignatureDatabase);
        let verified = match state.boot_state {
            BootState::Disabled | BootState::SetupMode => true,
            BootState::Enabled | BootState::EnforcingStrict => has_db_key,
        };

        let key_id = if verified {
            state.keys.iter().find(|k| k.key_type == KeyType::SignatureDatabase).map(|k| k.id)
        } else {
            None
        };

        if verified {
            state.total_verified += 1;
        } else {
            state.total_rejected += 1;
        }

        if state.records.len() >= MAX_RECORDS { state.records.remove(0); }
        state.records.push(VerifyRecord {
            image_name: String::from(image_name),
            hash: String::from(hash), verified, key_id, timestamp_ns: now,
        });
        Ok(verified)
    })
}

/// List enrolled keys.
pub fn list_keys() -> Vec<EnrolledKey> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.keys.clone())
}

/// Get verification records.
pub fn get_records(max: usize) -> Vec<VerifyRecord> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        let mut r = s.records.clone();
        r.reverse();
        r.truncate(max);
        r
    })
}

/// Statistics: (key_count, record_count, total_verified, total_rejected, ops).
pub fn stats() -> (usize, usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.keys.len(), s.records.len(), s.total_verified, s.total_rejected, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("secureboot::self_test() — running tests...");
    init_defaults();

    // 1: Default state disabled.
    assert_eq!(get_state(), BootState::Disabled);
    assert_eq!(list_keys().len(), 3);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Verify passes when disabled.
    let ok = verify_image("kernel", "SHA256:1234").expect("verify1");
    assert!(ok);
    crate::serial_println!("  [2/8] verify disabled: OK");

    // 3: Enable secure boot.
    set_state(BootState::Enabled).expect("enable");
    assert_eq!(get_state(), BootState::Enabled);
    crate::serial_println!("  [3/8] enable: OK");

    // 4: Verify passes with db key present.
    let ok = verify_image("kernel", "SHA256:5678").expect("verify2");
    assert!(ok);
    crate::serial_println!("  [4/8] verify enabled: OK");

    // 5: Enroll new key.
    let kid = enroll_key(KeyType::MachineOwnerKey, "Custom Module", "SHA256:abcd").expect("enroll");
    assert_eq!(list_keys().len(), 4);
    crate::serial_println!("  [5/8] enroll: OK");

    // 6: Remove key.
    remove_key(kid).expect("remove");
    assert_eq!(list_keys().len(), 3);
    crate::serial_println!("  [6/8] remove: OK");

    // 7: Verification records.
    let records = get_records(10);
    assert_eq!(records.len(), 2);
    assert!(records[0].verified);
    crate::serial_println!("  [7/8] records: OK");

    // 8: Stats.
    let (keys, records, verified, rejected, ops) = stats();
    assert_eq!(keys, 3);
    assert_eq!(records, 2);
    assert_eq!(verified, 2);
    assert_eq!(rejected, 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("secureboot::self_test() — all 8 tests passed");
}
