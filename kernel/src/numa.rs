//! NUMA topology detection and memory affinity.
//!
//! Parses the ACPI SRAT (System Resource Affinity Table) to discover
//! NUMA nodes and their CPU/memory associations.  On non-NUMA systems
//! (most VMs, single-socket desktop), everything is assigned to node 0.
//!
//! ## Why This Matters
//!
//! On multi-socket systems, memory access time varies depending on which
//! CPU is accessing which memory region.  "Local" memory (same socket) is
//! 1x latency; "remote" memory (different socket) is typically 1.5-3x.
//!
//! Knowing the NUMA topology lets the kernel:
//! - Allocate memory from the local node for a task (reduces latency)
//! - Schedule tasks near their memory (avoid remote accesses)
//! - Balance memory usage across nodes (avoid one node OOMing while
//!   another has free memory)
//! - Inform the scheduler about CPU-to-node mapping for work-stealing
//!   preferences (steal from same node first)
//!
//! ## SRAT Structure (ACPI 6.5)
//!
//! The SRAT contains a sequence of affinity structures:
//! - **Processor Local APIC Affinity** (type 0): maps LAPIC ID → node
//! - **Memory Affinity** (type 1): maps physical address range → node
//! - **Processor Local x2APIC Affinity** (type 2): for >255 CPUs
//!
//! We parse types 0, 1, and 2.
//!
//! ## Design
//!
//! The parsed topology is stored in static arrays (no heap required for
//! the basic structure).  A flat `cpu_to_node` array maps CPU index to
//! node.  A `NumaNode` struct holds per-node metadata (memory ranges,
//! CPU mask, total/free memory statistics).
//!
//! ## References
//!
//! - ACPI Spec 6.5 §5.2.16: System Resource Affinity Table (SRAT)
//! - Linux `drivers/acpi/numa/srat.c` — acpi_numa_init()
//! - Linux `arch/x86/mm/numa.c` — numa_init(), numa_add_memblk()

#![allow(dead_code)]

use core::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, AtomicBool, Ordering};
use crate::serial_println;
use crate::smp;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Maximum number of NUMA nodes supported.
pub const MAX_NODES: usize = 8;

/// Maximum number of memory regions per node.
const MAX_REGIONS_PER_NODE: usize = 8;

/// Maximum CPUs tracked (mirrors smp::MAX_CPUS).
const MAX_CPUS: usize = smp::MAX_CPUS;

// ---------------------------------------------------------------------------
// SRAT structure definitions (packed, matching ACPI spec)
// ---------------------------------------------------------------------------

/// SRAT table header (standard ACPI header).
#[repr(C, packed)]
struct SratHeader {
    signature: [u8; 4],     // "SRAT"
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
    // SRAT-specific fields after standard header
    table_revision: u32,    // must be 1
    reserved: u32,
}

/// SRAT sub-table header (type + length).
#[repr(C, packed)]
struct SratSubHeader {
    structure_type: u8,
    length: u8,
}

/// Processor Local APIC Affinity (SRAT type 0).
#[repr(C, packed)]
struct ProcessorLocalApicAffinity {
    header: SratSubHeader,          // type=0, length=16
    proximity_domain_lo: u8,        // Low byte of proximity domain
    apic_id: u8,                    // Local APIC ID
    flags: u32,                     // Bit 0: enabled
    local_sapic_eid: u8,            // Local SAPIC EID (ignore on x86)
    proximity_domain_hi: [u8; 3],   // High 3 bytes of proximity domain
    clock_domain: u32,              // Clock domain
}

/// Memory Affinity (SRAT type 1).
#[repr(C, packed)]
struct MemoryAffinity {
    header: SratSubHeader,          // type=1, length=40
    proximity_domain: u32,          // Proximity domain
    _reserved1: u16,
    base_address_lo: u32,           // Base address low 32 bits
    base_address_hi: u32,           // Base address high 32 bits
    length_lo: u32,                 // Length low 32 bits
    length_hi: u32,                 // Length high 32 bits
    _reserved2: u32,
    flags: u32,                     // Bit 0: enabled, bit 1: hot-pluggable, bit 2: non-volatile
    _reserved3: u64,
}

/// Processor Local x2APIC Affinity (SRAT type 2).
#[repr(C, packed)]
struct ProcessorX2ApicAffinity {
    header: SratSubHeader,          // type=2, length=24
    _reserved1: u16,
    proximity_domain: u32,          // Full 32-bit proximity domain
    x2apic_id: u32,                 // x2APIC ID
    flags: u32,                     // Bit 0: enabled
    clock_domain: u32,              // Clock domain
    _reserved2: u32,
}

// ---------------------------------------------------------------------------
// Runtime NUMA state
// ---------------------------------------------------------------------------

/// A physical memory region belonging to a NUMA node.
#[derive(Debug, Clone, Copy)]
pub struct NumaMemRegion {
    /// Physical base address.
    pub base: u64,
    /// Length in bytes.
    pub length: u64,
    /// Whether this region is hot-pluggable.
    pub hotplug: bool,
}

/// Per-NUMA-node information.
pub struct NumaNode {
    /// Whether this node exists (has at least one CPU or memory region).
    present: AtomicBool,
    /// Memory regions belonging to this node.
    regions: [NumaMemRegion; MAX_REGIONS_PER_NODE],
    /// Number of valid entries in `regions`.
    region_count: AtomicU8,
    /// Number of CPUs (logical processors) in this node.
    cpu_count: AtomicU8,
    /// Total memory in this node (bytes, computed from regions).
    total_memory: AtomicU64,
    /// Online CPU bitmask (bit N = CPU N is in this node).
    cpu_mask: AtomicU32,
}

impl NumaNode {
    const fn new() -> Self {
        Self {
            present: AtomicBool::new(false),
            regions: [NumaMemRegion { base: 0, length: 0, hotplug: false }; MAX_REGIONS_PER_NODE],
            region_count: AtomicU8::new(0),
            cpu_count: AtomicU8::new(0),
            total_memory: AtomicU64::new(0),
            cpu_mask: AtomicU32::new(0),
        }
    }
}

// SAFETY: NumaNode uses atomic fields for all shared mutable state.
unsafe impl Sync for NumaNode {}

/// NUMA nodes array.
static NODES: [NumaNode; MAX_NODES] = {
    const INIT: NumaNode = NumaNode::new();
    [INIT; MAX_NODES]
};

/// CPU-to-node mapping.  Index = logical CPU index, value = node ID.
/// Initialized to 0 (all CPUs on node 0 by default — UMA assumption).
static CPU_TO_NODE: [AtomicU8; MAX_CPUS] = {
    const ZERO: AtomicU8 = AtomicU8::new(0);
    [ZERO; MAX_CPUS]
};

/// Total number of NUMA nodes detected.
static NODE_COUNT: AtomicU8 = AtomicU8::new(1); // Default: 1 node (UMA)

/// Whether NUMA topology was detected from SRAT.
static NUMA_DETECTED: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialize NUMA topology.
///
/// Attempts to parse the SRAT from ACPI tables.  If no SRAT is present
/// (common in VMs and single-socket systems), creates a single-node
/// topology with all CPUs and all memory on node 0.
pub fn init() {
    // Try to find and parse SRAT.
    let srat_phys = find_srat();
    if let Some(phys) = srat_phys {
        if parse_srat(phys) {
            NUMA_DETECTED.store(true, Ordering::Release);
            let count = NODE_COUNT.load(Ordering::Relaxed);
            serial_println!(
                "[numa] SRAT parsed: {} NUMA node{} detected",
                count, if count == 1 { "" } else { "s" }
            );
            log_topology();
            return;
        }
    }

    // No SRAT or parse failed — single-node UMA topology.
    init_uma();
}

/// Get the NUMA node for a CPU.
#[inline]
#[must_use]
pub fn cpu_node(cpu: usize) -> usize {
    CPU_TO_NODE.get(cpu)
        .map_or(0, |n| n.load(Ordering::Relaxed) as usize)
}

/// Get the NUMA node for the current CPU.
#[inline]
#[must_use]
pub fn current_node() -> usize {
    cpu_node(smp::current_cpu_index())
}

/// Get the total number of NUMA nodes.
#[inline]
#[must_use]
pub fn node_count() -> usize {
    NODE_COUNT.load(Ordering::Relaxed) as usize
}

/// Check if real NUMA topology was detected (vs. UMA default).
#[inline]
#[must_use]
pub fn is_numa() -> bool {
    NUMA_DETECTED.load(Ordering::Acquire)
}

/// Get the NUMA node for a physical address.
///
/// Returns the node whose memory region contains `phys_addr`, or
/// `None` if no region matches (could be MMIO, reserved, or unmapped).
#[must_use]
pub fn phys_to_node(phys_addr: u64) -> Option<usize> {
    for (node_id, node) in NODES.iter().enumerate() {
        if !node.present.load(Ordering::Relaxed) {
            continue;
        }
        let count = node.region_count.load(Ordering::Relaxed) as usize;
        for i in 0..count.min(MAX_REGIONS_PER_NODE) {
            let region = &node.regions[i];
            if phys_addr >= region.base
                && phys_addr < region.base.saturating_add(region.length)
            {
                return Some(node_id);
            }
        }
    }
    None
}

/// Check if two CPUs are on the same NUMA node.
#[inline]
#[must_use]
pub fn same_node(cpu_a: usize, cpu_b: usize) -> bool {
    cpu_node(cpu_a) == cpu_node(cpu_b)
}

/// Get a snapshot of NUMA topology for diagnostics.
#[must_use]
pub fn topology_info() -> TopologyInfo {
    let count = node_count();
    let mut nodes = [NodeInfo::default(); MAX_NODES];

    for i in 0..count.min(MAX_NODES) {
        let node = &NODES[i];
        nodes[i] = NodeInfo {
            present: node.present.load(Ordering::Relaxed),
            cpu_count: node.cpu_count.load(Ordering::Relaxed),
            cpu_mask: node.cpu_mask.load(Ordering::Relaxed),
            total_memory: node.total_memory.load(Ordering::Relaxed),
            region_count: node.region_count.load(Ordering::Relaxed),
        };
    }

    TopologyInfo {
        node_count: count,
        is_numa: is_numa(),
        nodes,
    }
}

/// Get the distance (latency cost) between two NUMA nodes.
///
/// Returns a relative distance:
/// - 10: same node (local access)
/// - 20: adjacent nodes (1 hop)
/// - 30+: distant nodes (2+ hops)
///
/// This is a simplified model.  Real systems use the ACPI SLIT table
/// for accurate inter-node distances.  We don't parse SLIT yet, so
/// we assume uniform remote access cost.
#[must_use]
pub fn distance(node_a: usize, node_b: usize) -> u8 {
    if node_a == node_b {
        10 // Local
    } else {
        20 // Remote (uniform assumption without SLIT)
    }
}

// ---------------------------------------------------------------------------
// Topology info structs
// ---------------------------------------------------------------------------

/// Snapshot of per-node information.
#[derive(Debug, Clone, Copy, Default)]
pub struct NodeInfo {
    /// Whether this node exists.
    pub present: bool,
    /// Number of CPUs on this node.
    pub cpu_count: u8,
    /// CPU bitmask.
    pub cpu_mask: u32,
    /// Total memory (bytes).
    pub total_memory: u64,
    /// Number of memory regions.
    pub region_count: u8,
}

/// Complete NUMA topology snapshot.
#[derive(Debug, Clone, Copy)]
pub struct TopologyInfo {
    /// Total number of nodes.
    pub node_count: usize,
    /// Whether real NUMA was detected.
    pub is_numa: bool,
    /// Per-node information.
    pub nodes: [NodeInfo; MAX_NODES],
}

// ---------------------------------------------------------------------------
// SRAT parsing
// ---------------------------------------------------------------------------

/// Find the SRAT table physical address from ACPI.
fn find_srat() -> Option<u64> {
    // The ACPI module stores discovered table addresses.
    // Look for "SRAT" signature among the tables.
    crate::acpi::find_table(b"SRAT")
}

/// Parse the SRAT at the given physical address.
///
/// Returns true on success, false if the table is corrupt or empty.
fn parse_srat(phys: u64) -> bool {
    let hhdm = match crate::mm::page_table::hhdm() {
        Some(h) => h,
        None => return false,
    };

    let virt = phys.wrapping_add(hhdm) as *const u8;

    // Read the header to get the table length.
    // SAFETY: ACPI tables are in memory mapped by HHDM (bootloader guarantee).
    let header = unsafe { &*(virt as *const SratHeader) };
    let table_length = header.length as usize;

    if table_length < core::mem::size_of::<SratHeader>() {
        return false;
    }

    // Verify signature.
    if &header.signature != b"SRAT" {
        return false;
    }

    // Parse sub-tables starting after the header.
    let mut offset = core::mem::size_of::<SratHeader>();
    let mut max_node: u32 = 0;

    while offset + 2 <= table_length {
        // SAFETY: offset is within the validated table_length.
        let sub_header = unsafe {
            &*((virt as usize + offset) as *const SratSubHeader)
        };

        let sub_type = sub_header.structure_type;
        let sub_len = sub_header.length as usize;

        if sub_len < 2 || offset + sub_len > table_length {
            break; // Corrupt entry — stop parsing.
        }

        // SAFETY (group — covers all SRAT entry casts below): each cast is
        // guarded by a sub_len >= size_of::<T>() check, and offset + sub_len
        // is within the SRAT table.  The SRAT is mapped via HHDM and read-only.
        match sub_type {
            0 => {
                // Processor Local APIC Affinity.
                if sub_len >= core::mem::size_of::<ProcessorLocalApicAffinity>() {
                    let entry = unsafe {
                        &*((virt as usize + offset) as *const ProcessorLocalApicAffinity)
                    };
                    let flags = entry.flags;
                    if flags & 1 != 0 {
                        // Enabled.
                        let domain = u32::from(entry.proximity_domain_lo)
                            | (u32::from(entry.proximity_domain_hi[0]) << 8)
                            | (u32::from(entry.proximity_domain_hi[1]) << 16)
                            | (u32::from(entry.proximity_domain_hi[2]) << 24);
                        let apic_id = entry.apic_id;
                        add_cpu_to_node(apic_id as u32, domain);
                        if domain > max_node {
                            max_node = domain;
                        }
                    }
                }
            }
            1 => {
                // Memory Affinity.
                if sub_len >= core::mem::size_of::<MemoryAffinity>() {
                    let entry = unsafe {
                        &*((virt as usize + offset) as *const MemoryAffinity)
                    };
                    let flags = entry.flags;
                    if flags & 1 != 0 {
                        // Enabled.
                        let domain = entry.proximity_domain;
                        let base = u64::from(entry.base_address_lo)
                            | (u64::from(entry.base_address_hi) << 32);
                        let length = u64::from(entry.length_lo)
                            | (u64::from(entry.length_hi) << 32);
                        let hotplug = flags & 2 != 0;
                        add_memory_to_node(domain, base, length, hotplug);
                        if domain > max_node {
                            max_node = domain;
                        }
                    }
                }
            }
            2 => {
                // Processor Local x2APIC Affinity.
                if sub_len >= core::mem::size_of::<ProcessorX2ApicAffinity>() {
                    let entry = unsafe {
                        &*((virt as usize + offset) as *const ProcessorX2ApicAffinity)
                    };
                    let flags = entry.flags;
                    if flags & 1 != 0 {
                        let domain = entry.proximity_domain;
                        let x2apic_id = entry.x2apic_id;
                        add_cpu_to_node(x2apic_id, domain);
                        if domain > max_node {
                            max_node = domain;
                        }
                    }
                }
            }
            _ => {
                // Unknown type — skip.
            }
        }

        offset += sub_len;
    }

    if max_node == 0 && !NODES[0].present.load(Ordering::Relaxed) {
        return false; // No valid entries found.
    }

    // Set node count.
    #[allow(clippy::cast_possible_truncation)]
    let count = (max_node + 1).min(MAX_NODES as u32) as u8;
    NODE_COUNT.store(count, Ordering::Release);

    true
}

/// Add a CPU to a NUMA node (from SRAT parsing).
fn add_cpu_to_node(apic_id: u32, domain: u32) {
    let node_id = domain as usize;
    if node_id >= MAX_NODES {
        return;
    }

    // Find the logical CPU index for this APIC ID.
    let cpu_idx = apic_to_cpu_index(apic_id);

    let node = &NODES[node_id];
    node.present.store(true, Ordering::Relaxed);

    if let Some(idx) = cpu_idx {
        if idx < MAX_CPUS {
            CPU_TO_NODE[idx].store(node_id as u8, Ordering::Relaxed);
            node.cpu_count.fetch_add(1, Ordering::Relaxed);
            // Set bit in CPU mask.
            let bit = 1u32 << (idx as u32);
            node.cpu_mask.fetch_or(bit, Ordering::Relaxed);
        }
    }
}

/// Add a memory region to a NUMA node (from SRAT parsing).
fn add_memory_to_node(domain: u32, base: u64, length: u64, hotplug: bool) {
    let node_id = domain as usize;
    if node_id >= MAX_NODES {
        return;
    }

    let node = &NODES[node_id];
    node.present.store(true, Ordering::Relaxed);

    let idx = node.region_count.load(Ordering::Relaxed) as usize;
    if idx >= MAX_REGIONS_PER_NODE {
        return; // Too many regions for this node.
    }

    // SAFETY: We're the only writer during init (single-threaded boot).
    // The region array is initialized to zeros, and we write before
    // incrementing the count that others read.
    let region_ptr = &node.regions[idx] as *const NumaMemRegion as *mut NumaMemRegion;
    unsafe {
        (*region_ptr).base = base;
        (*region_ptr).length = length;
        (*region_ptr).hotplug = hotplug;
    }
    node.region_count.fetch_add(1, Ordering::Release);
    node.total_memory.fetch_add(length, Ordering::Relaxed);
}

/// Map APIC ID to logical CPU index.
fn apic_to_cpu_index(apic_id: u32) -> Option<usize> {
    // The SMP module maps APIC IDs to CPU indices.
    // Check each known CPU's APIC ID.
    let count = smp::cpu_count();
    for i in 0..count {
        if let Some(id) = smp::cpu_apic_id(i) {
            if u32::from(id) == apic_id {
                return Some(i);
            }
        }
    }
    None
}

/// Initialize a single-node UMA topology (no SRAT).
fn init_uma() {
    let cpu_count = smp::cpu_count();

    // All CPUs on node 0.
    NODES[0].present.store(true, Ordering::Relaxed);
    #[allow(clippy::cast_possible_truncation)]
    NODES[0].cpu_count.store(cpu_count as u8, Ordering::Relaxed);

    let mut mask = 0u32;
    for i in 0..cpu_count.min(32) {
        mask |= 1u32 << (i as u32);
    }
    NODES[0].cpu_mask.store(mask, Ordering::Relaxed);

    // Total memory from frame allocator stats.
    if let Some(stats) = crate::mm::frame::stats() {
        let total = (stats.total_frames as u64).saturating_mul(crate::mm::frame::FRAME_SIZE as u64);
        NODES[0].total_memory.store(total, Ordering::Relaxed);
    }

    // Single memory region covering all of RAM (simplified).
    // The exact regions don't matter for UMA — all accesses are local.
    NODES[0].region_count.store(1, Ordering::Relaxed);
    let region_ptr = &NODES[0].regions[0] as *const NumaMemRegion as *mut NumaMemRegion;
    // SAFETY: NODES[0] is a static; region_ptr points to its first region slot.
    // init_uma runs once at boot before any concurrent access.
    unsafe {
        (*region_ptr).base = 0;
        (*region_ptr).length = NODES[0].total_memory.load(Ordering::Relaxed);
        (*region_ptr).hotplug = false;
    }

    NODE_COUNT.store(1, Ordering::Release);

    serial_println!(
        "[numa] No SRAT found — UMA topology ({} CPUs, {} MiB on node 0)",
        cpu_count,
        NODES[0].total_memory.load(Ordering::Relaxed) / (1024 * 1024)
    );
}

/// Log the detected NUMA topology.
fn log_topology() {
    let count = node_count();
    for i in 0..count {
        let node = &NODES[i];
        if !node.present.load(Ordering::Relaxed) {
            continue;
        }
        let cpus = node.cpu_count.load(Ordering::Relaxed);
        let mem_mb = node.total_memory.load(Ordering::Relaxed) / (1024 * 1024);
        let regions = node.region_count.load(Ordering::Relaxed);
        serial_println!(
            "[numa]   Node {}: {} CPUs (mask={:#06x}), {} MiB ({} region{})",
            i, cpus, node.cpu_mask.load(Ordering::Relaxed),
            mem_mb, regions, if regions == 1 { "" } else { "s" }
        );
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the NUMA subsystem.
pub fn self_test() {
    serial_println!("[numa] Running self-test...");

    // Test 1: Node count is at least 1.
    let count = node_count();
    assert!(count >= 1, "must have at least 1 node");
    serial_println!("[numa]   Node count >= 1: OK ({})", count);

    // Test 2: All CPUs map to a valid node.
    let cpu_count = smp::cpu_count();
    for i in 0..cpu_count {
        let node = cpu_node(i);
        assert!(node < count, "CPU {} maps to invalid node {}", i, node);
    }
    serial_println!("[numa]   CPU-to-node mapping valid: OK ({} CPUs)", cpu_count);

    // Test 3: Current node is valid.
    let cur = current_node();
    assert!(cur < count);
    serial_println!("[numa]   Current node: OK (node {})", cur);

    // Test 4: same_node is reflexive.
    assert!(same_node(0, 0));
    serial_println!("[numa]   same_node reflexive: OK");

    // Test 5: distance to self is 10.
    assert_eq!(distance(0, 0), 10);
    if count > 1 {
        assert_eq!(distance(0, 1), 20);
    }
    serial_println!("[numa]   Distance: OK (local=10, remote=20)");

    // Test 6: Node 0 has memory.
    let info = topology_info();
    assert!(info.nodes[0].present);
    assert!(info.nodes[0].total_memory > 0 || !info.is_numa);
    serial_println!("[numa]   Node 0 present with memory: OK");

    // Test 7: Topology info is consistent.
    assert_eq!(info.node_count, count);
    serial_println!("[numa]   Topology info consistent: OK");

    serial_println!("[numa] Self-test PASSED");
}
