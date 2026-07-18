//! System Image — full system snapshot and recovery.
//!
//! Creates and manages system-level snapshots combining filesystem
//! state, package lists, and configuration for disaster recovery.
//!
//! ## Architecture
//!
//! ```text
//! System image management
//!   → systemimage::create(name) → capture system state
//!   → systemimage::restore(id) → roll back to snapshot
//!   → systemimage::verify(id) → check integrity
//!
//! Integration:
//!   → backup (backup system)
//!   → restorepoint (restore points)
//!   → fssnapshot (filesystem snapshots)
//!   → diskencrypt (disk encryption)
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

/// Image type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Full,           // Complete system image.
    Incremental,    // Changes since last full.
    Differential,   // Changes since specific base.
    BootPartition,  // Boot partition only.
    UserData,       // User data only.
}

impl ImageType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "Full",
            Self::Incremental => "Incremental",
            Self::Differential => "Differential",
            Self::BootPartition => "Boot Partition",
            Self::UserData => "User Data",
        }
    }
}

/// Image status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageStatus {
    Creating,
    Ready,
    Restoring,
    Verified,
    Corrupted,
    Expired,
}

impl ImageStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Creating => "Creating",
            Self::Ready => "Ready",
            Self::Restoring => "Restoring",
            Self::Verified => "Verified",
            Self::Corrupted => "Corrupted",
            Self::Expired => "Expired",
        }
    }
}

/// A system image record.
#[derive(Debug, Clone)]
pub struct SystemImage {
    pub id: u32,
    pub name: String,
    pub image_type: ImageType,
    pub status: ImageStatus,
    pub size_bytes: u64,
    pub created_ns: u64,
    pub base_image_id: Option<u32>,
    pub description: String,
    pub checksum: String,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_IMAGES: usize = 100;

struct State {
    images: Vec<SystemImage>,
    next_id: u32,
    total_created: u64,
    total_restored: u64,
    total_verified: u64,
    total_bytes: u64,
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
        images: alloc::vec![
            SystemImage {
                id: 1, name: String::from("Initial Install"),
                image_type: ImageType::Full, status: ImageStatus::Ready,
                size_bytes: 8_589_934_592, created_ns: now, base_image_id: None,
                description: String::from("Factory image after OS installation"),
                checksum: String::from("SHA256:aabb1122"),
            },
        ],
        next_id: 2,
        total_created: 1,
        total_restored: 0,
        total_verified: 0,
        total_bytes: 8_589_934_592,
        ops: 0,
    });
}

/// Create a new system image.
pub fn create_image(name: &str, image_type: ImageType, description: &str, size_bytes: u64, base_id: Option<u32>) -> KernelResult<u32> {
    with_state(|state| {
        if state.images.len() >= MAX_IMAGES {
            return Err(KernelError::ResourceExhausted);
        }
        // If incremental/differential, verify base exists.
        if let Some(bid) = base_id {
            if !state.images.iter().any(|i| i.id == bid) {
                return Err(KernelError::NotFound);
            }
        }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        let checksum = format!("SHA256:{:08x}", now & 0xFFFF_FFFF);
        state.images.push(SystemImage {
            id, name: String::from(name), image_type, status: ImageStatus::Ready,
            size_bytes, created_ns: now, base_image_id: base_id,
            description: String::from(description), checksum,
        });
        state.total_created += 1;
        state.total_bytes += size_bytes;
        Ok(id)
    })
}

/// Delete an image.
pub fn delete_image(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let before = state.images.len();
        state.images.retain(|i| i.id != id);
        if state.images.len() == before { return Err(KernelError::NotFound); }
        Ok(())
    })
}

/// Simulate restoring from an image.
pub fn restore_image(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let img = state.images.iter().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        if img.status == ImageStatus::Corrupted {
            return Err(KernelError::CorruptedData);
        }
        state.total_restored += 1;
        Ok(())
    })
}

/// Verify an image's integrity.
pub fn verify_image(id: u32) -> KernelResult<bool> {
    with_state(|state| {
        let img = state.images.iter_mut().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        // Simulate: images are valid unless marked corrupted.
        let valid = img.status != ImageStatus::Corrupted;
        if valid {
            img.status = ImageStatus::Verified;
        }
        state.total_verified += 1;
        Ok(valid)
    })
}

/// Set image status (e.g., mark as expired or corrupted).
pub fn set_status(id: u32, status: ImageStatus) -> KernelResult<()> {
    with_state(|state| {
        let img = state.images.iter_mut().find(|i| i.id == id)
            .ok_or(KernelError::NotFound)?;
        img.status = status;
        Ok(())
    })
}

/// List all images.
pub fn list_images() -> Vec<SystemImage> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.images.clone())
}

/// Get a specific image.
pub fn get_image(id: u32) -> Option<SystemImage> {
    STATE.lock().as_ref().and_then(|s| s.images.iter().find(|i| i.id == id).cloned())
}

/// Statistics: (image_count, total_created, total_restored, total_verified, total_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.images.len(), s.total_created, s.total_restored, s.total_verified, s.total_bytes, s.ops),
        None => (0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("systemimage::self_test() — running tests...");
    init_defaults();

    // 1: Default image.
    assert_eq!(list_images().len(), 1);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create full image.
    let id = create_image("Pre-Update", ImageType::Full, "Before big update", 4_000_000_000, None).expect("create");
    assert_eq!(list_images().len(), 2);
    crate::serial_println!("  [2/8] create: OK");

    // 3: Create incremental based on previous.
    let inc_id = create_image("Post-Update Delta", ImageType::Incremental, "Incremental after update", 500_000_000, Some(id)).expect("inc");
    assert_eq!(list_images().len(), 3);
    crate::serial_println!("  [3/8] incremental: OK");

    // 4: Verify image.
    let valid = verify_image(id).expect("verify");
    assert!(valid);
    let img = get_image(id).expect("get");
    assert_eq!(img.status, ImageStatus::Verified);
    crate::serial_println!("  [4/8] verify: OK");

    // 5: Restore.
    restore_image(id).expect("restore");
    crate::serial_println!("  [5/8] restore: OK");

    // 6: Mark corrupted and verify fails.
    set_status(inc_id, ImageStatus::Corrupted).expect("corrupt");
    let valid = verify_image(inc_id).expect("verify2");
    assert!(!valid);
    crate::serial_println!("  [6/8] corrupted: OK");

    // 7: Delete image.
    delete_image(inc_id).expect("delete");
    assert_eq!(list_images().len(), 2);
    crate::serial_println!("  [7/8] delete: OK");

    // 8: Stats.
    let (count, created, restored, verified, bytes, ops) = stats();
    assert_eq!(count, 2);
    assert!(created >= 3);
    assert_eq!(restored, 1);
    assert_eq!(verified, 2);
    assert!(bytes > 0);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("systemimage::self_test() — all 8 tests passed");
}
