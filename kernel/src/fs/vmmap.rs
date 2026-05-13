//! Virtual Memory Map — address space and VMA monitoring.
//!
//! Tracks virtual memory areas (VMAs), memory mappings,
//! address space usage, and mapping operations per process.
//! Essential for diagnosing memory layout issues and
//! monitoring mmap/munmap activity.
//!
//! ## Architecture
//!
//! ```text
//! Virtual memory map
//!   → vmmap::create_vma(pid, start, size, perm) → track mmap
//!   → vmmap::remove_vma(pid, start) → track munmap
//!   → vmmap::list_vmas(pid) → list VMAs for process
//!   → vmmap::address_space(pid) → address space summary
//!
//! Integration:
//!   → memlayout (memory layout)
//!   → pftrack (page fault tracking)
//!   → memcg (memory cgroup)
//!   → procstat (process statistics)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use alloc::format;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// VMA permission flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VmaPerm {
    pub read: bool,
    pub write: bool,
    pub exec: bool,
    pub shared: bool,
}

impl VmaPerm {
    pub fn label(self) -> &'static str {
        match (self.read, self.write, self.exec, self.shared) {
            (true, false, false, false) => "r---",
            (true, true, false, false) => "rw--",
            (true, false, true, false) => "r-x-",
            (true, true, true, false) => "rwx-",
            (true, false, false, true) => "r--s",
            (true, true, false, true) => "rw-s",
            (true, true, true, true) => "rwxs",
            _ => "----",
        }
    }
}

/// VMA type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmaType {
    Anonymous,
    FileBacked,
    Stack,
    Heap,
    SharedMem,
    DeviceMap,
    Vdso,
}

impl VmaType {
    pub fn label(self) -> &'static str {
        match self {
            Self::Anonymous => "anon",
            Self::FileBacked => "file",
            Self::Stack => "stack",
            Self::Heap => "heap",
            Self::SharedMem => "shm",
            Self::DeviceMap => "device",
            Self::Vdso => "vdso",
        }
    }
}

/// A virtual memory area.
#[derive(Debug, Clone)]
pub struct Vma {
    pub start: u64,
    pub end: u64,
    pub perm: VmaPerm,
    pub vma_type: VmaType,
    pub name: String,
    pub resident_pages: u64,
    pub dirty_pages: u64,
}

/// Per-process address space info.
#[derive(Debug, Clone)]
pub struct ProcessAddrSpace {
    pub pid: u32,
    pub vmas: Vec<Vma>,
    pub total_mapped: u64,
    pub total_resident: u64,
    pub maps_count: u64,
    pub unmaps_count: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_PROCESSES: usize = 128;
const MAX_VMAS_PER_PROC: usize = 256;

struct State {
    processes: Vec<ProcessAddrSpace>,
    total_maps: u64,
    total_unmaps: u64,
    total_vmas: u64,
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
    *guard = Some(State {
        processes: alloc::vec![
            ProcessAddrSpace {
                pid: 1,
                vmas: alloc::vec![
                    Vma { start: 0x400000, end: 0x500000, perm: VmaPerm { read: true, write: false, exec: true, shared: false }, vma_type: VmaType::FileBacked, name: String::from("[text]"), resident_pages: 64, dirty_pages: 0 },
                    Vma { start: 0x600000, end: 0x700000, perm: VmaPerm { read: true, write: true, exec: false, shared: false }, vma_type: VmaType::Heap, name: String::from("[heap]"), resident_pages: 128, dirty_pages: 32 },
                    Vma { start: 0x7FFF_FFFF_0000, end: 0x7FFF_FFFF_FFFF, perm: VmaPerm { read: true, write: true, exec: false, shared: false }, vma_type: VmaType::Stack, name: String::from("[stack]"), resident_pages: 4, dirty_pages: 4 },
                ],
                total_mapped: 0x200000,
                total_resident: 196,
                maps_count: 3,
                unmaps_count: 0,
            },
        ],
        total_maps: 3,
        total_unmaps: 0,
        total_vmas: 3,
        ops: 0,
    });
}

/// Create a VMA mapping.
pub fn create_vma(pid: u32, start: u64, size: u64, perm: VmaPerm, vma_type: VmaType, name: &str) -> KernelResult<()> {
    with_state(|state| {
        let proc_space = if let Some(ps) = state.processes.iter_mut().find(|p| p.pid == pid) {
            ps
        } else {
            if state.processes.len() >= MAX_PROCESSES { return Err(KernelError::ResourceExhausted); }
            state.processes.push(ProcessAddrSpace {
                pid, vmas: Vec::new(), total_mapped: 0, total_resident: 0,
                maps_count: 0, unmaps_count: 0,
            });
            state.processes.last_mut().ok_or(KernelError::InternalError)?
        };
        if proc_space.vmas.len() >= MAX_VMAS_PER_PROC { return Err(KernelError::ResourceExhausted); }
        proc_space.vmas.push(Vma {
            start, end: start + size, perm, vma_type,
            name: String::from(name), resident_pages: 0, dirty_pages: 0,
        });
        proc_space.total_mapped += size;
        proc_space.maps_count += 1;
        state.total_maps += 1;
        state.total_vmas += 1;
        Ok(())
    })
}

/// Remove a VMA mapping.
pub fn remove_vma(pid: u32, start: u64) -> KernelResult<()> {
    with_state(|state| {
        let ps = state.processes.iter_mut().find(|p| p.pid == pid)
            .ok_or(KernelError::NotFound)?;
        let idx = ps.vmas.iter().position(|v| v.start == start)
            .ok_or(KernelError::NotFound)?;
        let size = ps.vmas[idx].end - ps.vmas[idx].start;
        ps.vmas.remove(idx);
        ps.total_mapped = ps.total_mapped.saturating_sub(size);
        ps.unmaps_count += 1;
        state.total_unmaps += 1;
        state.total_vmas = state.total_vmas.saturating_sub(1);
        Ok(())
    })
}

/// List VMAs for a process.
pub fn list_vmas(pid: u32) -> Vec<Vma> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.processes.iter().find(|p| p.pid == pid)
            .map_or(Vec::new(), |p| p.vmas.clone())
    })
}

/// Get address space summary for a process.
pub fn address_space(pid: u32) -> Option<ProcessAddrSpace> {
    STATE.lock().as_ref().and_then(|s| s.processes.iter().find(|p| p.pid == pid).cloned())
}

/// List all tracked processes.
pub fn list_processes() -> Vec<(u32, usize, u64)> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.processes.iter().map(|p| (p.pid, p.vmas.len(), p.total_mapped)).collect()
    })
}

/// Statistics: (process_count, total_vmas, total_maps, total_unmaps, ops).
pub fn stats() -> (usize, u64, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.processes.len(), s.total_vmas, s.total_maps, s.total_unmaps, s.ops),
        None => (0, 0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("vmmap::self_test() — running tests...");
    init_defaults();

    // 1: Defaults.
    let procs = list_processes();
    assert_eq!(procs.len(), 1);
    assert_eq!(procs[0].1, 3); // 3 VMAs for pid 1.
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Create VMA.
    let perm = VmaPerm { read: true, write: true, exec: false, shared: false };
    create_vma(1, 0x800000, 0x100000, perm, VmaType::Anonymous, "[anon]").expect("create");
    assert_eq!(list_vmas(1).len(), 4);
    crate::serial_println!("  [2/8] create vma: OK");

    // 3: Remove VMA.
    remove_vma(1, 0x800000).expect("remove");
    assert_eq!(list_vmas(1).len(), 3);
    crate::serial_println!("  [3/8] remove vma: OK");

    // 4: Auto-create process.
    let perm = VmaPerm { read: true, write: false, exec: true, shared: false };
    create_vma(500, 0x400000, 0x10000, perm, VmaType::FileBacked, "[text]").expect("auto_create");
    assert_eq!(list_processes().len(), 2);
    crate::serial_println!("  [4/8] auto-create process: OK");

    // 5: Address space.
    let space = address_space(1).expect("addr_space");
    assert_eq!(space.pid, 1);
    assert!(space.total_mapped > 0);
    crate::serial_println!("  [5/8] address space: OK");

    // 6: VMA permissions.
    let perm = VmaPerm { read: true, write: true, exec: true, shared: true };
    assert_eq!(perm.label(), "rwxs");
    let perm2 = VmaPerm { read: true, write: false, exec: false, shared: false };
    assert_eq!(perm2.label(), "r---");
    crate::serial_println!("  [6/8] permissions: OK");

    // 7: Remove not found.
    assert!(remove_vma(1, 0xDEAD).is_err());
    assert!(remove_vma(999, 0).is_err());
    crate::serial_println!("  [7/8] not found: OK");

    // 8: Stats.
    let (procs, vmas, maps, unmaps, ops) = stats();
    assert_eq!(procs, 2);
    assert!(vmas >= 4);
    assert!(maps >= 5);
    assert!(unmaps >= 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("vmmap::self_test() — all 8 tests passed");
}
