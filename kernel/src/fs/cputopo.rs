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

#![allow(dead_code)]

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::sync::PreemptSpinMutex as Mutex;

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

/// Build the topology snapshot by reading the real, already-detected CPU
/// topology from `crate::cpu_topology` (populated by `cpu_topology::detect()`
/// during early boot, before this lazy init can run).
///
/// We never fabricate topology. Every logical CPU and package is derived from
/// the live per-CPU topology array. Fields we cannot honestly source are left
/// empty rather than invented:
///   - `caches`: no cache enumerator is wired yet. DEFERRED: populate from a
///     CPUID leaf-0x4 (Intel) / leaf-0x8000_001D (AMD) cache descriptor pass.
///   - `model_name`: no CPUID brand-string reader is wired yet. DEFERRED:
///     read the 48-byte brand string from CPUID 0x8000_0002..=0x8000_0004.
///   - `numa_node` / `numa_nodes`: no ACPI SRAT parse yet, so every CPU maps to
///     node 0 and the node count is 1. DEFERRED: derive from ACPI SRAT.
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }

    let num_cpus = crate::smp::cpu_count().max(1);

    // Read each logical CPU straight from the detected topology.
    let mut cpus = Vec::with_capacity(num_cpus.min(MAX_CPUS));
    for i in 0..num_cpus.min(MAX_CPUS) {
        let cpu = crate::cpu_topology::cpu_topo(i).map_or_else(
            // Topology not detected / out of range: present a single-thread
            // core in package 0 — the honest "we don't know better" layout,
            // matching cpu_topology's own no-detect fallbacks.
            || LogicalCpu {
                id: i as u32,
                package_id: 0,
                core_id: i as u32,
                thread_id: 0,
                numa_node: 0,
                online: true,
            },
            |t| LogicalCpu {
                id: i as u32,
                package_id: u32::from(t.package_id),
                core_id: u32::from(t.core_id),
                thread_id: u32::from(t.smt_id),
                numa_node: 0,
                online: true,
            },
        );
        cpus.push(cpu);
    }

    // Summarise packages directly from the CPU list (unique package_ids; per
    // package count its logical threads and distinct cores).
    let mut packages: Vec<PackageInfo> = Vec::new();
    for cpu in &cpus {
        if let Some(pkg) = packages.iter_mut().find(|p| p.id == cpu.package_id) {
            pkg.total_threads = pkg.total_threads.saturating_add(1);
        } else {
            packages.push(PackageInfo {
                id: cpu.package_id,
                model_name: String::new(),
                cores: 0,
                threads_per_core: 0,
                total_threads: 1,
            });
        }
    }
    for pkg in &mut packages {
        // Count distinct core_ids within this package.
        let mut cores: Vec<u32> = Vec::new();
        for cpu in cpus.iter().filter(|c| c.package_id == pkg.id) {
            if !cores.contains(&cpu.core_id) {
                cores.push(cpu.core_id);
            }
        }
        pkg.cores = cores.len() as u32;
        pkg.threads_per_core = pkg.total_threads.checked_div(pkg.cores.max(1)).unwrap_or(1);
    }

    *guard = Some(State {
        cpus,
        caches: Vec::new(),
        packages,
        numa_nodes: 1,
        smt_enabled: crate::cpu_topology::smt_active(),
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
    STATE.lock().as_ref().is_some_and(|s| s.smt_enabled)
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

    // Residue-free: start from a known-empty state.
    *STATE.lock() = None;

    // -- Part A: read-through sanity (hardware-agnostic) --------------------
    // init_defaults() reads the live topology, which on the test machine
    // (QEMU, no -smp) is a single CPU but could be anything. Assert only the
    // invariants that hold regardless of the underlying hardware.
    init_defaults();
    let n = cpu_count();
    assert!(n >= 1, "expected at least one logical CPU");
    assert_eq!(online_count(), n, "every CPU starts online");
    let pkgs = packages();
    assert!(!pkgs.is_empty(), "expected at least one package");
    // Sum of per-package total_threads must equal the logical CPU count.
    let pkg_thread_sum: u32 = pkgs.iter().map(|p| p.total_threads).sum();
    assert_eq!(pkg_thread_sum as usize, n, "package threads must cover all CPUs");
    // Honest unknowns: no cache enumerator, no brand string, no SRAT.
    assert!(cache_info().is_empty(), "caches must be empty (not fabricated)");
    assert!(pkgs.iter().all(|p| p.model_name.is_empty()), "model_name must be empty");
    let (_c, _p, numa, _smt, _q, _o) = stats();
    assert_eq!(numa, 1, "numa_nodes is honestly 1 until SRAT is parsed");
    crate::serial_println!("  [1/8] read-through sanity: OK");

    // -- Part B: deterministic fixture -------------------------------------
    // Install a known 8-CPU / 4-core / 2-thread / 1-package layout directly so
    // the accessors can be exercised with exact expectations independent of the
    // host. (Same split-test pattern used by monitors.rs.)
    {
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
        *STATE.lock() = Some(State {
            cpus,
            caches: alloc::vec![
                CacheInfo { cache_type: CacheType::L1Data, size_kb: 32, line_size: 64, associativity: 8, shared_by_threads: 2 },
                CacheInfo { cache_type: CacheType::L1Instruction, size_kb: 32, line_size: 64, associativity: 8, shared_by_threads: 2 },
                CacheInfo { cache_type: CacheType::L2Unified, size_kb: 256, line_size: 64, associativity: 4, shared_by_threads: 2 },
                CacheInfo { cache_type: CacheType::L3Unified, size_kb: 8192, line_size: 64, associativity: 16, shared_by_threads: 8 },
            ],
            packages: alloc::vec![
                PackageInfo { id: 0, model_name: String::new(), cores: 4, threads_per_core: 2, total_threads: 8 },
            ],
            numa_nodes: 1,
            smt_enabled: true,
            total_queries: 0,
            ops: 0,
        });
    }

    // 2: CPU list.
    assert_eq!(list_cpus().len(), 8);
    assert_eq!(cpu_count(), 8);
    assert_eq!(online_count(), 8);
    crate::serial_println!("  [2/8] fixture cpus: OK");

    // 3: Package info.
    let fpkgs = packages();
    assert_eq!(fpkgs.len(), 1);
    assert_eq!(fpkgs[0].cores, 4);
    assert_eq!(fpkgs[0].threads_per_core, 2);
    crate::serial_println!("  [3/8] packages: OK");

    // 4: Cache info.
    assert_eq!(cache_info().len(), 4);
    crate::serial_println!("  [4/8] caches: OK");

    // 5: CPUs in package / thread siblings.
    assert_eq!(cpus_in_package(0).len(), 8);
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
    let (cpus, pkgs2, numa2, smt, _queries, ops) = stats();
    assert_eq!(cpus, 8);
    assert_eq!(pkgs2, 1);
    assert_eq!(numa2, 1);
    assert!(smt);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Residue-free: leave no fixture behind.
    *STATE.lock() = None;

    crate::serial_println!("cputopo::self_test() — all 8 tests passed");
}
