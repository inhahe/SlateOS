//! System Profiler — detailed hardware and software inventory.
//!
//! Provides a comprehensive view of the system including CPU, RAM,
//! storage, network, graphics, and installed software details.
//!
//! ## Architecture
//!
//! ```text
//! System query
//!   → sysprofiler::get_section(section) → detailed info
//!   → sysprofiler::get_summary() → overview
//!   → sysprofiler::export() → full report
//!
//! Integration:
//!   → sysinfo (basic system info)
//!   → hwmonitor (hardware sensors)
//!   → devicemgr (device tree)
//!   → disksmart (disk health)
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

/// Profiler section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Cpu,
    Memory,
    Storage,
    Graphics,
    Network,
    Audio,
    Usb,
    Firmware,
    Software,
    Display,
}

impl Section {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Storage => "Storage",
            Self::Graphics => "Graphics",
            Self::Network => "Network",
            Self::Audio => "Audio",
            Self::Usb => "USB",
            Self::Firmware => "Firmware",
            Self::Software => "Software",
            Self::Display => "Display",
        }
    }
}

/// A key-value info entry.
#[derive(Debug, Clone)]
pub struct InfoEntry {
    pub key: String,
    pub value: String,
}

/// A profiler section with entries.
#[derive(Debug, Clone)]
pub struct SectionData {
    pub section: Section,
    pub entries: Vec<InfoEntry>,
    pub last_refreshed_ns: u64,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct State {
    sections: Vec<SectionData>,
    total_queries: u64,
    total_refreshes: u64,
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

fn make_entry(key: &str, value: &str) -> InfoEntry {
    InfoEntry { key: String::from(key), value: String::from(value) }
}

/// Decode a fixed-width CPUID ASCII buffer (vendor/brand string) into a
/// trimmed `String`, stopping at the first NUL.  CPUID strings are ASCII by
/// spec; a non-UTF-8 byte yields an empty string rather than corrupting
/// output.
fn cpuid_str(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = bytes
        .get(..end)
        .and_then(|sl| core::str::from_utf8(sl).ok())
        .unwrap_or("");
    String::from(s.trim())
}

/// Format a byte count as a human-readable power-of-two size.
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * 1024 * 1024;
    if bytes >= GIB {
        format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{} KiB", bytes / KIB)
    } else {
        format!("{bytes} B")
    }
}

/// Build the CPU section from **real** CPUID + topology detection.
///
/// Reads vendor/brand strings, family/model/stepping, logical-CPU and
/// physical-core counts, SMT status, and cache topology from the live
/// `crate::cpu` / `crate::cpu_topology` detection that runs in early boot.
/// Individual fields are skipped when their source is empty (e.g. CPUID does
/// not expose a brand string, or cache topology was not enumerated under the
/// hypervisor).
fn build_cpu_entries() -> Vec<InfoEntry> {
    let mut entries = Vec::new();

    let vendor = cpuid_str(&crate::cpu::vendor_string());
    if !vendor.is_empty() {
        entries.push(make_entry("Vendor", &vendor));
    }
    let brand = cpuid_str(&crate::cpu::brand_string());
    if !brand.is_empty() {
        entries.push(make_entry("Model", &brand));
    }

    let (family, model, stepping) = crate::cpu::cpu_family_model_stepping();
    entries.push(make_entry("Family", &format!("{family}")));
    entries.push(make_entry("Model ID", &format!("{model}")));
    entries.push(make_entry("Stepping", &format!("{stepping}")));

    let logical = crate::smp::cpu_count().max(1);
    entries.push(make_entry("Logical CPUs", &format!("{logical}")));
    let cores = crate::cpu_topology::num_physical_cores();
    entries.push(make_entry("Physical Cores", &format!("{cores}")));
    entries.push(make_entry(
        "SMT",
        if crate::cpu_topology::smt_active() { "Active" } else { "Inactive" },
    ));

    for c in crate::cpu::cache_topology() {
        let key = format!("Cache L{} {}", c.level, c.type_name());
        entries.push(make_entry(&key, &format_size(u64::from(c.size))));
    }

    entries
}

/// Build the Memory section from the **real** buddy-allocator statistics.
///
/// Reports the total RAM the frame allocator manages and the page size.
/// Returns an empty Vec if the allocator is not yet initialised.
fn build_memory_entries() -> Vec<InfoEntry> {
    let mut entries = Vec::new();
    if let Some(s) = crate::mm::frame::stats() {
        let total = (s.total_frames as u64)
            .saturating_mul(crate::mm::frame::FRAME_SIZE as u64);
        entries.push(make_entry("Total", &format_size(total)));
        entries.push(make_entry(
            "Page Size",
            &format_size(crate::mm::frame::FRAME_SIZE as u64),
        ));
    }
    entries
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Populate the profiler with **real** boot-time hardware facts.
///
/// The CPU section is built from live CPUID + topology detection
/// (`crate::cpu`, `crate::cpu_topology`) and the Memory section from the real
/// buddy-allocator statistics (`crate::mm::frame::stats`).  These are static
/// facts captured once at boot, so a cached snapshot stays accurate.
///
/// Sections that require subsystems not yet wired for inventory — Storage,
/// Graphics, Network, Audio, USB, Firmware, Software, Display — are
/// deliberately left ABSENT rather than fabricated; they will be added as the
/// block layer, GPU driver, NIC stack, etc. expose enumeration. See the
/// DEFERRED PROPER FIX note in todo.txt.
///
/// (Previously this seeded entirely FABRICATED hardware specs — a "4-core /
/// 8-thread 3.60 GHz" CPU with invented 256 KB / 1 MB / 8 MB caches, "8192 MB
/// DDR4 3200 MHz" memory in "2 / 4" slots, an "NVMe SSD 512 GB / PCIe 4.0 x4"
/// drive, "Integrated Graphics / 512 MB shared", and "UEFI / Secure Boot
/// Disabled" firmware — none of which were measured.  The `sysprofiler all`
/// /`summary`/`section` kshell views then displayed those placeholders as if
/// they were this machine's real hardware.)
pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    let mut sections: Vec<SectionData> = Vec::new();

    let cpu_entries = build_cpu_entries();
    if !cpu_entries.is_empty() {
        sections.push(SectionData {
            section: Section::Cpu,
            entries: cpu_entries,
            last_refreshed_ns: now,
        });
    }

    let mem_entries = build_memory_entries();
    if !mem_entries.is_empty() {
        sections.push(SectionData {
            section: Section::Memory,
            entries: mem_entries,
            last_refreshed_ns: now,
        });
    }

    *guard = Some(State {
        sections,
        total_queries: 0,
        total_refreshes: 0,
        ops: 0,
    });
}

/// Get a section.
pub fn get_section(section: Section) -> Option<SectionData> {
    let mut guard = STATE.lock();
    if let Some(state) = guard.as_mut() {
        state.ops += 1;
        state.total_queries += 1;
        state.sections.iter().find(|s| s.section == section).cloned()
    } else {
        None
    }
}

/// Get all sections.
pub fn get_all() -> Vec<SectionData> {
    STATE.lock().as_ref().map_or(Vec::new(), |s| s.sections.clone())
}

/// Add or update an entry in a section.
pub fn set_entry(section: Section, key: &str, value: &str) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(s) = state.sections.iter_mut().find(|s| s.section == section) {
            if let Some(e) = s.entries.iter_mut().find(|e| e.key == key) {
                e.value = String::from(value);
            } else {
                s.entries.push(make_entry(key, value));
            }
            s.last_refreshed_ns = now;
        } else {
            state.sections.push(SectionData {
                section,
                entries: alloc::vec![make_entry(key, value)],
                last_refreshed_ns: now,
            });
        }
        Ok(())
    })
}

/// Refresh a section (mark as updated).
pub fn refresh_section(section: Section) -> KernelResult<()> {
    with_state(|state| {
        let now = crate::hpet::elapsed_ns();
        if let Some(s) = state.sections.iter_mut().find(|s| s.section == section) {
            s.last_refreshed_ns = now;
            state.total_refreshes += 1;
        }
        Ok(())
    })
}

/// Generate a summary string.
pub fn get_summary() -> String {
    let guard = STATE.lock();
    let state = match guard.as_ref() {
        Some(s) => s,
        None => return String::from("Not initialised"),
    };
    let mut out = String::new();
    for s in &state.sections {
        out.push_str(&format!("[{}]\n", s.section.label()));
        for e in &s.entries {
            out.push_str(&format!("  {}: {}\n", e.key, e.value));
        }
    }
    out
}

/// Statistics: (section_count, total_queries, total_refreshes, ops).
pub fn stats() -> (usize, u64, u64, u64) {
    let guard = STATE.lock();
    match guard.as_ref() {
        Some(s) => (s.sections.len(), s.total_queries, s.total_refreshes, s.ops),
        None => (0, 0, 0, 0),
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

pub fn self_test() {
    crate::serial_println!("sysprofiler::self_test() — running tests...");
    // Start clean so we exercise the real builders, not a stale snapshot.
    *STATE.lock() = None;
    init_defaults();

    // 1: Real defaults — exactly CPU + Memory, built from live detection.
    //    (CPU is always present on x86_64; the frame allocator is initialised
    //    long before fs self-tests run, so Memory is present too.)
    let all = get_all();
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|s| s.section == Section::Cpu));
    assert!(all.iter().any(|s| s.section == Section::Memory));
    crate::serial_println!("  [1/8] real defaults: OK");

    // 2: CPU section is populated from CPUID/topology (has the CPU counts).
    let cpu = get_section(Section::Cpu).expect("cpu");
    assert!(!cpu.entries.is_empty());
    assert!(cpu.entries.iter().any(|e| e.key == "Logical CPUs"));
    crate::serial_println!("  [2/8] cpu section: OK");

    // 3: Memory section reports a non-empty real total.
    let mem = get_section(Section::Memory).expect("mem");
    let total = mem.entries.iter().find(|e| e.key == "Total").expect("total");
    assert!(!total.value.is_empty());
    crate::serial_println!("  [3/8] memory section: OK");

    // 4: Update an existing entry in place.
    set_entry(Section::Cpu, "Logical CPUs", "test-value").expect("update");
    let cpu = get_section(Section::Cpu).expect("cpu2");
    let lc = cpu.entries.iter().find(|e| e.key == "Logical CPUs").expect("lc");
    assert_eq!(lc.value, "test-value");
    crate::serial_println!("  [4/8] update entry: OK");

    // 5: Add a brand-new section + entry.
    set_entry(Section::Network, "Interface", "eth0").expect("net");
    let net = get_section(Section::Network).expect("net2");
    assert_eq!(net.entries.len(), 1);
    assert_eq!(get_all().len(), 3);
    crate::serial_println!("  [5/8] new section: OK");

    // 6: Summary includes section headers.
    let summary = get_summary();
    assert!(summary.contains("[CPU]"));
    assert!(summary.contains("[Network]"));
    crate::serial_println!("  [6/8] summary: OK");

    // 7: Refresh bumps the refresh counter.
    refresh_section(Section::Cpu).expect("refresh");
    crate::serial_println!("  [7/8] refresh: OK");

    // 8: Stats — 3 sections, exactly 4 queries (one per get_section),
    //    refreshes == 1, ops advanced.
    let (sections, queries, refreshes, ops) = stats();
    assert_eq!(sections, 3);
    assert_eq!(queries, 4);
    assert_eq!(refreshes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    // Rebuild the real snapshot so the live /proc/sysprofiler reflects actual
    // hardware (CPU + Memory), not this test's scratch entries.
    *STATE.lock() = None;
    init_defaults();

    crate::serial_println!("sysprofiler::self_test() — all 8 tests passed");
}
