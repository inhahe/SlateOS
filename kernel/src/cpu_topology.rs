//! CPU topology detection — core/package/SMT mapping.
//!
//! Decodes APIC IDs into their hierarchical components (package, core,
//! SMT thread) using CPUID extended topology enumeration (leaf 0xB or
//! leaf 0x1F).  This enables the scheduler to make topology-aware
//! decisions:
//!
//! - **Avoid same-core scheduling**: Two CPU-bound tasks on the same
//!   physical core (different SMT threads) each get ~50% throughput.
//!   Spreading them across different cores gives ~100% each.
//! - **Prefer SMT siblings for cache sharing**: Tasks that share data
//!   benefit from running on the same core's SMT threads (shared L1/L2).
//! - **Minimize cross-package migration**: Moving a task between NUMA
//!   packages (sockets) is expensive due to remote memory access.
//!
//! ## Detection Method
//!
//! 1. **CPUID leaf 0x1F** (V2 Extended Topology): preferred, available
//!    on newer Intel CPUs (>= 2019) and AMD Zen 3+.  Reports full
//!    hierarchy including die and module levels.
//! 2. **CPUID leaf 0xB** (x2APIC Topology Enumeration): fallback for
//!    older Intel CPUs.  Reports SMT and core levels.
//! 3. **CPUID leaf 1 + leaf 4**: legacy fallback for CPUs without leaf
//!    0xB.  Uses APIC ID bit-field widths from leaf 1 and 4.
//!
//! ## Data Structures
//!
//! After detection, each online CPU has a [`CpuTopo`] struct recording:
//! - `package_id`: physical socket/package number
//! - `core_id`: core within the package
//! - `smt_id`: SMT thread within the core
//!
//! The module also pre-computes SMT sibling masks and same-package
//! masks for fast lookup from the scheduler.
//!
//! ## References
//!
//! - Intel SDM Vol. 2A: CPUID — leaf 0xB, 0x1F
//! - AMD PPR: CPUID Fn0000_001F, Fn0000_000B
//! - Linux `arch/x86/kernel/cpu/topology.c`
//! - Linux `arch/x86/include/asm/topology.h`

#![allow(dead_code)]

use core::sync::atomic::{AtomicBool, Ordering};
use crate::smp::MAX_CPUS;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of topology levels in the CPUID enumeration.
const MAX_LEVELS: usize = 6;

// ---------------------------------------------------------------------------
// Per-CPU topology data
// ---------------------------------------------------------------------------

/// Topology information for a single logical CPU.
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuTopo {
    /// Physical package (socket) number.
    pub package_id: u16,
    /// Core number within the package.
    pub core_id: u16,
    /// SMT thread number within the core (0 or 1 typically).
    pub smt_id: u8,
    /// The APIC ID for this logical CPU.
    pub apic_id: u32,
    /// Number of SMT threads per core.
    pub threads_per_core: u8,
    /// Number of cores per package.
    pub cores_per_package: u16,
}

/// Per-CPU topology array (indexed by CPU index, not APIC ID).
static mut CPU_TOPO: [CpuTopo; MAX_CPUS] = [CpuTopo {
    package_id: 0,
    core_id: 0,
    smt_id: 0,
    apic_id: 0,
    threads_per_core: 1,
    cores_per_package: 1,
}; MAX_CPUS];

/// Whether topology detection has completed.
static TOPO_DETECTED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// SMT sibling masks (bitmask of CPUs sharing the same core)
// ---------------------------------------------------------------------------

/// Per-CPU bitmask: which other CPUs are SMT siblings (same physical core).
///
/// `SMT_SIBLINGS[i]` has bit `j` set if CPU j shares the same physical
/// core as CPU i (including itself).
static mut SMT_SIBLINGS: [u16; MAX_CPUS] = [0; MAX_CPUS];

/// Per-CPU bitmask: which other CPUs are in the same package.
static mut PKG_SIBLINGS: [u16; MAX_CPUS] = [0; MAX_CPUS];

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// CPUID topology level types (from Intel SDM).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum LevelType {
    Invalid = 0,
    Smt = 1,
    Core = 2,
    Module = 3,
    Tile = 4,
    Die = 5,
}

impl LevelType {
    fn from_u8(val: u8) -> Self {
        match val {
            1 => Self::Smt,
            2 => Self::Core,
            3 => Self::Module,
            4 => Self::Tile,
            5 => Self::Die,
            _ => Self::Invalid,
        }
    }
}

/// Raw CPUID result for a topology level.
#[derive(Debug, Clone, Copy)]
struct TopoLevel {
    /// Number of bits to shift the APIC ID right to get the next-level ID.
    shift: u8,
    /// Level type (SMT, Core, Module, etc.).
    level_type: LevelType,
    /// Number of logical processors at this level.
    #[allow(dead_code)]
    num_processors: u16,
}

/// Detect CPU topology for all online CPUs.
///
/// Must be called during boot after SMP initialization (so all CPUs'
/// APIC IDs are known).  Only the BSP runs this.
pub fn detect() {
    let num_cpus = crate::smp::cpu_count().max(1);

    // Try leaf 0x1F first (V2 extended topology), then 0xB.
    let levels = if let Some(l) = enumerate_leaf(0x1F) {
        serial_println!("[topo] Using CPUID leaf 0x1F (V2 Extended Topology)");
        l
    } else if let Some(l) = enumerate_leaf(0x0B) {
        serial_println!("[topo] Using CPUID leaf 0x0B (x2APIC Topology)");
        l
    } else {
        serial_println!("[topo] No topology leaf available, using legacy fallback");
        detect_legacy(num_cpus);
        finalize(num_cpus);
        return;
    };

    // Decode each CPU's APIC ID using the shift widths from topology levels.
    let smt_shift = levels.iter()
        .find(|l| l.level_type == LevelType::Smt)
        .map_or(0, |l| l.shift);
    let core_shift = levels.iter()
        .find(|l| l.level_type == LevelType::Core)
        .map_or(smt_shift, |l| l.shift);

    for cpu in 0..num_cpus {
        let apic_id = crate::smp::cpu_apic_id(cpu).unwrap_or(0) as u32;

        // Decode APIC ID fields using shift widths.
        let smt_id = apic_id & ((1u32 << smt_shift) - 1);
        let core_id = (apic_id >> smt_shift) & ((1u32 << (core_shift - smt_shift)) - 1);
        let package_id = apic_id >> core_shift;

        let threads_per_core = 1u8 << smt_shift;
        let cores_per_package = if core_shift > smt_shift {
            1u16 << (core_shift - smt_shift)
        } else {
            1
        };

        // SAFETY: Single-threaded boot, cpu < MAX_CPUS.
        unsafe {
            CPU_TOPO[cpu] = CpuTopo {
                #[allow(clippy::cast_possible_truncation)]
                package_id: package_id as u16,
                #[allow(clippy::cast_possible_truncation)]
                core_id: core_id as u16,
                #[allow(clippy::cast_possible_truncation)]
                smt_id: smt_id as u8,
                apic_id,
                threads_per_core,
                cores_per_package,
            };
        }
    }

    finalize(num_cpus);
}

/// Enumerate topology levels from a CPUID leaf (0xB or 0x1F).
///
/// Returns None if the leaf is not supported or returns no valid levels.
fn enumerate_leaf(leaf: u32) -> Option<alloc::vec::Vec<TopoLevel>> {
    let max_leaf = cpuid_max_leaf();
    if max_leaf < leaf {
        return None;
    }

    let mut levels = alloc::vec::Vec::new();

    for subleaf in 0..MAX_LEVELS {
        let (eax, ebx, ecx, _edx) = cpuid_topology(leaf, subleaf as u32);

        let shift = (eax & 0x1F) as u8;
        let level_type_raw = ((ecx >> 8) & 0xFF) as u8;
        let level_type = LevelType::from_u8(level_type_raw);
        let num_processors = (ebx & 0xFFFF) as u16;

        if level_type == LevelType::Invalid {
            break;
        }

        levels.push(TopoLevel {
            shift,
            level_type,
            num_processors,
        });
    }

    if levels.is_empty() {
        None
    } else {
        Some(levels)
    }
}

/// Legacy topology detection when CPUID leaf 0xB/0x1F is unavailable.
///
/// Uses CPUID leaf 1 (initial APIC ID, logical processor count) and
/// leaf 4 (core count per package) to infer topology.
fn detect_legacy(num_cpus: usize) {
    // Get logical processors per package from leaf 1 EBX[23:16].
    let (_ecx, _edx, ebx) = cpuid_leaf1_full();
    let logical_per_pkg = ((ebx >> 16) & 0xFF).max(1);

    // Get cores per package from leaf 4 subleaf 0 EAX[31:26] + 1.
    let max_leaf = cpuid_max_leaf();
    let cores_per_pkg = if max_leaf >= 4 {
        let (eax, _, _, _) = cpuid_leaf4_sub(0);
        ((eax >> 26) & 0x3F) + 1
    } else {
        1
    };

    let threads_per_core = if cores_per_pkg > 0 {
        (logical_per_pkg / cores_per_pkg).max(1)
    } else {
        1
    };

    for cpu in 0..num_cpus {
        let apic_id = crate::smp::cpu_apic_id(cpu).unwrap_or(0) as u32;

        // Simple decomposition assuming linear APIC IDs.
        let smt_id = apic_id % threads_per_core;
        let core_id = (apic_id / threads_per_core) % cores_per_pkg;
        let package_id = apic_id / logical_per_pkg;

        // SAFETY: Single-threaded boot, cpu < MAX_CPUS.
        unsafe {
            CPU_TOPO[cpu] = CpuTopo {
                #[allow(clippy::cast_possible_truncation)]
                package_id: package_id as u16,
                #[allow(clippy::cast_possible_truncation)]
                core_id: core_id as u16,
                #[allow(clippy::cast_possible_truncation)]
                smt_id: smt_id as u8,
                apic_id,
                #[allow(clippy::cast_possible_truncation)]
                threads_per_core: threads_per_core as u8,
                #[allow(clippy::cast_possible_truncation)]
                cores_per_package: cores_per_pkg as u16,
            };
        }
    }
}

/// Compute sibling masks and log results.
fn finalize(num_cpus: usize) {
    // Compute SMT and package sibling masks.
    for i in 0..num_cpus {
        // SAFETY: read-only after detect() runs on BSP.
        let topo_i = unsafe { &CPU_TOPO[i] };
        let mut smt_mask: u16 = 0;
        let mut pkg_mask: u16 = 0;

        for j in 0..num_cpus {
            let topo_j = unsafe { &CPU_TOPO[j] };

            // Same physical core = same package + same core_id.
            if topo_j.package_id == topo_i.package_id
                && topo_j.core_id == topo_i.core_id
            {
                smt_mask |= 1u16 << j;
            }

            // Same package.
            if topo_j.package_id == topo_i.package_id {
                pkg_mask |= 1u16 << j;
            }
        }

        // SAFETY: single-threaded boot.
        unsafe {
            SMT_SIBLINGS[i] = smt_mask;
            PKG_SIBLINGS[i] = pkg_mask;
        }
    }

    TOPO_DETECTED.store(true, Ordering::Release);

    // Log topology summary.
    let packages = count_packages(num_cpus);
    let cores = count_physical_cores(num_cpus);

    serial_println!(
        "[topo] Topology: {} package(s), {} physical core(s), {} logical CPU(s)",
        packages, cores, num_cpus
    );

    // Log per-CPU details at debug level.
    for cpu in 0..num_cpus {
        let t = unsafe { &CPU_TOPO[cpu] };
        serial_println!(
            "[topo]   CPU {}: pkg={} core={} smt={} (APIC ID={})",
            cpu, t.package_id, t.core_id, t.smt_id, t.apic_id
        );
    }
}

// ---------------------------------------------------------------------------
// Query API
// ---------------------------------------------------------------------------

/// Get topology info for a specific CPU.
///
/// Returns `None` if topology detection hasn't run or cpu_index is out of range.
#[must_use]
pub fn cpu_topo(cpu_index: usize) -> Option<&'static CpuTopo> {
    if !TOPO_DETECTED.load(Ordering::Acquire) || cpu_index >= MAX_CPUS {
        return None;
    }
    // SAFETY: After detect(), CPU_TOPO is read-only.
    Some(unsafe { &CPU_TOPO[cpu_index] })
}

/// Get the SMT sibling mask for a CPU.
///
/// Returns a bitmask where bit `j` is set if CPU `j` shares the same
/// physical core as `cpu_index`.
#[must_use]
pub fn smt_siblings(cpu_index: usize) -> u16 {
    if !TOPO_DETECTED.load(Ordering::Acquire) || cpu_index >= MAX_CPUS {
        return 1u16 << cpu_index;
    }
    // SAFETY: After detect(), SMT_SIBLINGS is read-only.
    unsafe { SMT_SIBLINGS[cpu_index] }
}

/// Get the package sibling mask for a CPU.
///
/// Returns a bitmask where bit `j` is set if CPU `j` is in the same
/// physical package as `cpu_index`.
#[must_use]
pub fn package_siblings(cpu_index: usize) -> u16 {
    if !TOPO_DETECTED.load(Ordering::Acquire) || cpu_index >= MAX_CPUS {
        return u16::MAX;
    }
    // SAFETY: After detect(), PKG_SIBLINGS is read-only.
    unsafe { PKG_SIBLINGS[cpu_index] }
}

/// Check if two CPUs are SMT siblings (share the same physical core).
#[must_use]
pub fn are_smt_siblings(cpu_a: usize, cpu_b: usize) -> bool {
    if cpu_a >= MAX_CPUS || cpu_b >= MAX_CPUS {
        return false;
    }
    smt_siblings(cpu_a) & (1u16 << cpu_b) != 0
}

/// Check if two CPUs are in the same package.
#[must_use]
pub fn same_package(cpu_a: usize, cpu_b: usize) -> bool {
    if cpu_a >= MAX_CPUS || cpu_b >= MAX_CPUS {
        return false;
    }
    package_siblings(cpu_a) & (1u16 << cpu_b) != 0
}

/// Count the number of physical packages (sockets) detected.
#[must_use]
pub fn num_packages() -> usize {
    if !TOPO_DETECTED.load(Ordering::Acquire) {
        return 1;
    }
    let num_cpus = crate::smp::cpu_count().max(1);
    count_packages(num_cpus)
}

/// Count physical cores (not including SMT duplicates).
#[must_use]
pub fn num_physical_cores() -> usize {
    if !TOPO_DETECTED.load(Ordering::Acquire) {
        return crate::smp::cpu_count().max(1);
    }
    let num_cpus = crate::smp::cpu_count().max(1);
    count_physical_cores(num_cpus)
}

/// Is SMT (Hyper-Threading) active on this system?
///
/// True if any core has more than one logical CPU.
#[must_use]
pub fn smt_active() -> bool {
    if !TOPO_DETECTED.load(Ordering::Acquire) {
        return false;
    }
    let num_cpus = crate::smp::cpu_count().max(1);
    // SAFETY: read-only after detect().
    unsafe { CPU_TOPO[..num_cpus].iter().any(|t| t.threads_per_core > 1) }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn count_packages(num_cpus: usize) -> usize {
    let mut seen: u16 = 0;
    for cpu in 0..num_cpus {
        let pkg = unsafe { CPU_TOPO[cpu].package_id };
        seen |= 1u16 << pkg;
    }
    seen.count_ones() as usize
}

fn count_physical_cores(num_cpus: usize) -> usize {
    // Count unique (package_id, core_id) pairs.
    // With MAX_CPUS=16 this is trivially small.
    let mut count = 0usize;
    for i in 0..num_cpus {
        let ti = unsafe { &CPU_TOPO[i] };
        // Count this CPU only if it's the first with this (pkg, core) pair.
        let first = (0..i).all(|j| {
            let tj = unsafe { &CPU_TOPO[j] };
            tj.package_id != ti.package_id || tj.core_id != ti.core_id
        });
        if first {
            count += 1;
        }
    }
    count
}

// ---------------------------------------------------------------------------
// CPUID helpers
// ---------------------------------------------------------------------------

/// CPUID topology enumeration (leaf 0xB or 0x1F with subleaf).
fn cpuid_topology(leaf: u32, subleaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: CPUID is safe; we verified the leaf is supported.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            in("eax") leaf,
            in("ecx") subleaf,
            ebx_out = out(reg) ebx,
            lateout("eax") eax,
            lateout("ecx") ecx,
            lateout("edx") edx,
            options(nomem, nostack),
        );
    }
    (eax, ebx, ecx, edx)
}

/// CPUID leaf 0: maximum standard leaf.
fn cpuid_max_leaf() -> u32 {
    let eax: u32;
    // SAFETY: CPUID leaf 0 always valid.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "xor eax, eax",
            "cpuid",
            "pop rbx",
            lateout("eax") eax,
            out("ecx") _,
            out("edx") _,
            options(nomem, nostack),
        );
    }
    eax
}

/// CPUID leaf 1: returns (ECX, EDX, EBX) — feature flags + misc info.
fn cpuid_leaf1_full() -> (u32, u32, u32) {
    let ecx: u32;
    let edx: u32;
    let ebx: u32;
    // SAFETY: CPUID leaf 1 always valid on x86_64.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 1",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            ebx_out = out(reg) ebx,
            lateout("eax") _,
            lateout("ecx") ecx,
            lateout("edx") edx,
            options(nomem, nostack),
        );
    }
    (ecx, edx, ebx)
}

/// CPUID leaf 4, subleaf N: deterministic cache parameters.
fn cpuid_leaf4_sub(subleaf: u32) -> (u32, u32, u32, u32) {
    let eax: u32;
    let ebx: u32;
    let ecx: u32;
    let edx: u32;
    // SAFETY: Caller verified max_leaf >= 4.
    unsafe {
        core::arch::asm!(
            "push rbx",
            "mov eax, 4",
            "cpuid",
            "mov {ebx_out:e}, ebx",
            "pop rbx",
            in("ecx") subleaf,
            ebx_out = out(reg) ebx,
            lateout("eax") eax,
            lateout("ecx") ecx,
            lateout("edx") edx,
            options(nomem, nostack),
        );
    }
    (eax, ebx, ecx, edx)
}

extern crate alloc;
