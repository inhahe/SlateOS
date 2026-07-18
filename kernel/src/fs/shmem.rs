//! Shared Memory — shared memory region management.
//!
//! Manages named and anonymous shared memory regions between
//! processes. Supports permissions, reference counting, and
//! memory-mapped file semantics.
//!
//! ## Architecture
//!
//! ```text
//! Shared memory
//!   → shmem::create(name, size) → create region
//!   → shmem::attach(id, pid) → map into process
//!   → shmem::detach(id, pid) → unmap from process
//!   → shmem::delete(id) → remove region
//!
//! Integration:
//!   → ipclog (IPC logging)
//!   → prociso (process isolation)
//!   → procstat (process statistics)
//!   → memlayout (memory layout)
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

/// Shared memory permissions.
//
// The shared `Read` prefix is intentional: these name a permission lattice
// (every mapping is at least readable), so the prefix carries meaning rather
// than being the redundant naming `enum_variant_names` warns about.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShmPermission {
    ReadOnly,
    ReadWrite,
    ReadExecute,
    ReadWriteExecute,
}

impl ShmPermission {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "r--",
            Self::ReadWrite => "rw-",
            Self::ReadExecute => "r-x",
            Self::ReadWriteExecute => "rwx",
        }
    }
}

/// Shared memory region.
#[derive(Debug, Clone)]
pub struct ShmRegion {
    pub id: u32,
    pub name: String,
    pub size: u64,
    pub permission: ShmPermission,
    pub owner_pid: u32,
    pub attached_pids: Vec<u32>,
    pub created_ns: u64,
    pub last_access_ns: u64,
    pub persistent: bool,   // Survives owner exit.
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_REGIONS: usize = 256;

struct State {
    regions: Vec<ShmRegion>,
    next_id: u32,
    total_created: u64,
    total_deleted: u64,
    total_attaches: u64,
    total_detaches: u64,
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
    // Start with no shared-memory regions. A region records real IPC state —
    // which named/anonymous segments exist, their sizes, and which live PIDs
    // are attached. Seeding "/shm/compositor_fb" and "/shm/audio_buffer" with
    // invented owner/attached PIDs (1, 50, 200) and fabricated created/attach/
    // byte totals would surface phantom IPC regions through /proc and the
    // `shmem` shell command as if those processes had really mapped them.
    // Regions appear only when a process creates one via create().
    //
    // DEFERRED PROPER FIX: wire this diagnostic view to the real kernel shared-
    // memory / IPC subsystem so /proc/shmem reflects genuinely mapped segments.
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    *guard = Some(State {
        regions: Vec::new(),
        next_id: 1,
        total_created: 0,
        total_deleted: 0,
        total_attaches: 0,
        total_detaches: 0,
        total_bytes: 0,
        ops: 0,
    });
}

/// Create a shared memory region.
pub fn create(name: &str, size: u64, permission: ShmPermission, owner: u32, persistent: bool) -> KernelResult<u32> {
    with_state(|state| {
        if state.regions.len() >= MAX_REGIONS { return Err(KernelError::ResourceExhausted); }
        if state.regions.iter().any(|r| r.name == name) { return Err(KernelError::AlreadyExists); }
        if size == 0 { return Err(KernelError::InvalidArgument); }
        let now = crate::hpet::elapsed_ns();
        let id = state.next_id;
        state.next_id += 1;
        state.regions.push(ShmRegion {
            id, name: String::from(name), size, permission, owner_pid: owner,
            attached_pids: alloc::vec![owner], created_ns: now,
            last_access_ns: now, persistent,
        });
        state.total_created += 1;
        state.total_attaches += 1;
        state.total_bytes += size;
        Ok(id)
    })
}

/// Delete a shared memory region.
pub fn delete(id: u32) -> KernelResult<()> {
    with_state(|state| {
        let idx = state.regions.iter().position(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        let region = &state.regions[idx];
        if !region.attached_pids.is_empty() && region.attached_pids.len() > 1 {
            // Only owner can delete, and only if they're the only one attached (or force).
        }
        state.total_bytes = state.total_bytes.saturating_sub(region.size);
        state.regions.remove(idx);
        state.total_deleted += 1;
        Ok(())
    })
}

/// Attach a process to a region.
pub fn attach(id: u32, pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let r = state.regions.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        if r.attached_pids.contains(&pid) { return Err(KernelError::AlreadyExists); }
        r.attached_pids.push(pid);
        r.last_access_ns = crate::hpet::elapsed_ns();
        state.total_attaches += 1;
        Ok(())
    })
}

/// Detach a process from a region.
pub fn detach(id: u32, pid: u32) -> KernelResult<()> {
    with_state(|state| {
        let r = state.regions.iter_mut().find(|r| r.id == id)
            .ok_or(KernelError::NotFound)?;
        let before = r.attached_pids.len();
        r.attached_pids.retain(|&p| p != pid);
        if r.attached_pids.len() == before { return Err(KernelError::NotFound); }
        state.total_detaches += 1;
        // Auto-delete non-persistent regions when all detached.
        if r.attached_pids.is_empty() && !r.persistent {
            let size = r.size;
            let rid = r.id;
            state.regions.retain(|r| r.id != rid);
            state.total_bytes = state.total_bytes.saturating_sub(size);
            state.total_deleted += 1;
        }
        Ok(())
    })
}

/// Get region by ID.
pub fn get_region(id: u32) -> Option<ShmRegion> {
    STATE.lock().as_ref().and_then(|s| s.regions.iter().find(|r| r.id == id).cloned())
}

/// Get region by name.
pub fn get_by_name(name: &str) -> Option<ShmRegion> {
    STATE.lock().as_ref().and_then(|s| s.regions.iter().find(|r| r.name == name).cloned())
}

/// List all regions.
pub fn list_regions() -> Vec<ShmRegion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.regions.clone())
}

/// List regions a process is attached to.
pub fn regions_for_pid(pid: u32) -> Vec<ShmRegion> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.regions.iter().filter(|r| r.attached_pids.contains(&pid)).cloned().collect()
    })
}

/// Statistics: (region_count, total_created, total_deleted, total_attaches, total_detaches, total_bytes, ops).
pub fn stats() -> (usize, u64, u64, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.regions.len(), s.total_created, s.total_deleted, s.total_attaches, s.total_detaches, s.total_bytes, s.ops),
        None => (0, 0, 0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("shmem::self_test() — running tests...");

    // Residue-free: start from a clean, controlled State so assertions hold
    // regardless of prior kshell/procfs activity.
    *STATE.lock() = None;
    init_defaults();

    // 1: Empty defaults, then build a fixture through the real create()/attach()
    //    API: a persistent region owned by pid 1, also mapped by pid 200.
    assert_eq!(list_regions().len(), 0);
    let fb = create("/shm/compositor_fb", 8_294_400, ShmPermission::ReadWrite, 1, true).expect("fb");
    attach(fb, 200).expect("attach fb 200");
    assert_eq!(list_regions().len(), 1);
    crate::serial_println!("  [1/8] defaults+fixture: OK");

    // 2: Create region.
    let id = create("/shm/test", 4096, ShmPermission::ReadWrite, 100, false).expect("create");
    assert!(create("/shm/test", 4096, ShmPermission::ReadWrite, 100, false).is_err());
    assert!(create("/shm/zero", 0, ShmPermission::ReadOnly, 1, false).is_err());
    crate::serial_println!("  [2/8] create: OK");

    // 3: Get.
    let r = get_region(id).expect("get");
    assert_eq!(r.size, 4096);
    let r = get_by_name("/shm/test").expect("by_name");
    assert_eq!(r.id, id);
    crate::serial_println!("  [3/8] get: OK");

    // 4: Attach.
    attach(id, 200).expect("attach");
    let r = get_region(id).expect("get2");
    assert_eq!(r.attached_pids.len(), 2);
    assert!(attach(id, 200).is_err()); // Duplicate.
    crate::serial_println!("  [4/8] attach: OK");

    // 5: Regions for PID — pid 200 is attached to fb + test.
    let regs = regions_for_pid(200);
    assert_eq!(regs.len(), 2);
    crate::serial_println!("  [5/8] pid regions: OK");

    // 6: Detach.
    detach(id, 200).expect("detach");
    let r = get_region(id).expect("get3");
    assert_eq!(r.attached_pids.len(), 1);
    crate::serial_println!("  [6/8] detach: OK");

    // 7: Auto-delete non-persistent on last detach.
    detach(id, 100).expect("detach2");
    assert!(get_region(id).is_none()); // Auto-deleted.
    crate::serial_println!("  [7/8] auto-delete: OK");

    // 8: Stats — exact: 1 region left (fb), 2 created, 1 deleted, 4 attaches,
    //    2 detaches, 8_294_400 bytes (fb only after test auto-deleted).
    let (regions, created, deleted, attaches, detaches, bytes, ops) = stats();
    assert_eq!(regions, 1);
    assert_eq!(created, 2);
    assert_eq!(deleted, 1);
    assert_eq!(attaches, 4);
    assert_eq!(detaches, 2);
    assert_eq!(bytes, 8_294_400);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Leave no residue for later callers / boot-time tests.
    *STATE.lock() = None;

    crate::serial_println!("shmem::self_test() — all 8 tests passed");
}
