//! CPU Topology — CPU layout and topology information.
//!
//! Exposes CPU topology: packages, cores, threads, cache hierarchy,
//! NUMA nodes, and inter-core relationships for scheduling and
//! affinity decisions.
//!
//! ## Architecture
//!
//! ```text
//! Topology discovery
//!   → cputopo::discover() → scan CPUID/ACPI for topology
//!   → cputopo::packages() → physical CPU packages
//!   → cputopo::cores(package) → cores per package
//!   → cputopo::threads(core) → threads per core (SMT)
//!
//! Integration:
//!   → schedtune (scheduler tuning)
//!   → cpufreq (frequency scaling)
//!   → sysinfo (system information)
//!   → perfmon (performance monitor)
//! ```

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{KernelError, KernelResult};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Cache type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheType {
    L1Data,
    L1Instruction,
    L2Unified,
    L3Unified,
}

impl CacheType {
    pub fn label(self) -> &'static str {
        match self {
            Self::L1Data => "L1d",
            Self::L1Instruction => "L1i",
            Self::L2Unified => "L2",
            Self::L3Unified => "L3",
        }
    }
}

/// Cache information.
#[derive(Debug, Clone)]
pub struct CacheInfo {
    pub cache_type: CacheType,
    pub size_kb: u32,
    pub line_size: u32,
    pub associativity: u32,
    pub shared_by_threads: u32,
}

/// A logical CPU.
#[derive(Debug, Clone)]
pub struct LogicalCpu {
    pub id: u32,
    pub package_id: u32,
    pub core_id: u32,
    pub thread_id: u32,
    pub numa_node: u32,
    pub online: bool,
}

/// Package (physical CPU) summary.
#[derive(Debug, Clone)]
pub struct PackageInfo {
    pub id: u32,
    pub model_name: String,
    pub cores: u32,
    pub threads_per_core: u32,
    pub total_threads: u32,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

const MAX_CPUS: usize = 256;

struct State {
    cpus: Vec<LogicalCpu>,
    caches: Vec<CacheInfo>,
    packages: Vec<PackageInfo>,
    numa_nodes: u32,
    smt_enabled: bool,
    total_queries: u64,
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
    // Simulate a quad-core with SMT (8 logical CPUs).
    let mut cpus = Vec::new();
    for core in 0..4u32 {
        for thread in 0..2u32 {
            cpus.push(LogicalCpu {
                id: core * 2 + thread,
                package_id: 0,
                core_id: core,
                thread_id: thread,
                numa_node: 0,
                online: true,
            });
        }
    }
    *guard = Some(State {
        cpus,
        caches: alloc::vec![
            CacheInfo { cache_type: CacheType::L1Data, size_kb: 32, line_size: 64, associativity: 8, shared_by_threads: 2 },
            CacheInfo { cache_type: CacheType::L1Instruction, size_kb: 32, line_size: 64, associativity: 8, shared_by_threads: 2 },
            CacheInfo { cache_type: CacheType::L2Unified, size_kb: 256, line_size: 64, associativity: 4, shared_by_threads: 2 },
            CacheInfo { cache_type: CacheType::L3Unified, size_kb: 8192, line_size: 64, associativity: 16, shared_by_threads: 8 },
        ],
        packages: alloc::vec![
            PackageInfo {
                id: 0, model_name: String::from("Simulated x86_64 CPU"),
                cores: 4, threads_per_core: 2, total_threads: 8,
            },
        ],
        numa_nodes: 1,
        smt_enabled: true,
        total_queries: 0,
        ops: 0,
    });
}

/// Get all logical CPUs.
pub fn list_cpus() -> Vec<LogicalCpu> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.cpus.clone())
}

/// Get CPU info.
pub fn get_cpu(id: u32) -> Option<LogicalCpu> {
    STATE.lock().as_ref().and_then(|s| s.cpus.iter().find(|c| c.id == id).cloned())
}

/// Get cache hierarchy.
pub fn cache_info() -> Vec<CacheInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.caches.clone())
}

/// Get package info.
pub fn packages() -> Vec<PackageInfo> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.packages.clone())
}

/// Get CPUs in a package.
pub fn cpus_in_package(package_id: u32) -> Vec<LogicalCpu> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        s.cpus.iter().filter(|c| c.package_id == package_id).cloned().collect()
    })
}

/// Get sibling threads (same core).
pub fn thread_siblings(cpu_id: u32) -> Vec<u32> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| {
        if let Some(cpu) = s.cpus.iter().find(|c| c.id == cpu_id) {
            s.cpus.iter()
                .filter(|c| c.package_id == cpu.package_id && c.core_id == cpu.core_id && c.id != cpu_id)
                .map(|c| c.id)
                .collect()
        } else {
            Vec::new()
        }
    })
}

/// Check if SMT is enabled.
pub fn smt_enabled() -> bool {
    STATE.lock().as_ref().map_or(false, |s| s.smt_enabled)
}

/// Set CPU online/offline.
pub fn set_online(cpu_id: u32, online: bool) -> KernelResult<()> {
    with_state(|state| {
        let cpu = state.cpus.iter_mut().find(|c| c.id == cpu_id)
            .ok_or(KernelError::NotFound)?;
        // Cannot offline CPU 0 (boot CPU).
        if cpu_id == 0 && !online {
            return Err(KernelError::PermissionDenied);
        }
        cpu.online = online;
        Ok(())
    })
}

/// Total logical CPUs.
pub fn cpu_count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.cpus.len())
}

/// Online CPU count.
pub fn online_count() -> usize {
    STATE.lock().as_ref().map_or(0, |s| s.cpus.iter().filter(|c| c.online).count())
}

/// Statistics: (cpu_count, package_count, numa_nodes, smt, total_queries, ops).
pub fn stats() -> (usize, usize, u32, bool, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.cpus.len(), s.packages.len(), s.numa_nodes, s.smt_enabled, s.total_queries, s.ops),
        None => (0, 0, 0, false, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("cputopo::self_test() — running tests...");
    init_defaults();

    // 1: Default CPUs.
    assert_eq!(list_cpus().len(), 8);
    assert_eq!(cpu_count(), 8);
    assert_eq!(online_count(), 8);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Package info.
    let pkgs = packages();
    assert_eq!(pkgs.len(), 1);
    assert_eq!(pkgs[0].cores, 4);
    assert_eq!(pkgs[0].threads_per_core, 2);
    crate::serial_println!("  [2/8] packages: OK");

    // 3: Cache info.
    let caches = cache_info();
    assert_eq!(caches.len(), 4);
    crate::serial_println!("  [3/8] caches: OK");

    // 4: CPUs in package.
    let pkg_cpus = cpus_in_package(0);
    assert_eq!(pkg_cpus.len(), 8);
    crate::serial_println!("  [4/8] cpus in package: OK");

    // 5: Thread siblings.
    let siblings = thread_siblings(0);
    assert_eq!(siblings.len(), 1); // CPU 0 and 1 share core 0.
    assert_eq!(siblings[0], 1);
    crate::serial_println!("  [5/8] siblings: OK");

    // 6: SMT.
    assert!(smt_enabled());
    crate::serial_println!("  [6/8] smt: OK");

    // 7: Online/offline.
    set_online(7, false).expect("offline");
    assert_eq!(online_count(), 7);
    assert!(set_online(0, false).is_err()); // Can't offline boot CPU.
    set_online(7, true).expect("online");
    assert_eq!(online_count(), 8);
    crate::serial_println!("  [7/8] online/offline: OK");

    // 8: Stats.
    let (cpus, pkgs, numa, smt, _queries, ops) = stats();
    assert_eq!(cpus, 8);
    assert_eq!(pkgs, 1);
    assert_eq!(numa, 1);
    assert!(smt);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("cputopo::self_test() — all 8 tests passed");
}
