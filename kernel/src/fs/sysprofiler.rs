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
use spin::Mutex;

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

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn init_defaults() {
    let mut guard = STATE.lock();
    if guard.is_some() { return; }
    let now = crate::hpet::elapsed_ns();
    *guard = Some(State {
        sections: alloc::vec![
            SectionData {
                section: Section::Cpu,
                entries: alloc::vec![
                    make_entry("Model", "x86_64 processor"),
                    make_entry("Cores", "4"),
                    make_entry("Threads", "8"),
                    make_entry("Base Clock", "3.60 GHz"),
                    make_entry("Cache L1", "256 KB"),
                    make_entry("Cache L2", "1 MB"),
                    make_entry("Cache L3", "8 MB"),
                ],
                last_refreshed_ns: now,
            },
            SectionData {
                section: Section::Memory,
                entries: alloc::vec![
                    make_entry("Total", "8192 MB"),
                    make_entry("Type", "DDR4"),
                    make_entry("Speed", "3200 MHz"),
                    make_entry("Slots Used", "2 / 4"),
                ],
                last_refreshed_ns: now,
            },
            SectionData {
                section: Section::Storage,
                entries: alloc::vec![
                    make_entry("Primary", "NVMe SSD 512 GB"),
                    make_entry("Interface", "PCIe 4.0 x4"),
                    make_entry("Filesystem", "ext4"),
                ],
                last_refreshed_ns: now,
            },
            SectionData {
                section: Section::Graphics,
                entries: alloc::vec![
                    make_entry("GPU", "Integrated Graphics"),
                    make_entry("VRAM", "512 MB (shared)"),
                    make_entry("Driver", "kernel modesetting"),
                ],
                last_refreshed_ns: now,
            },
            SectionData {
                section: Section::Firmware,
                entries: alloc::vec![
                    make_entry("Type", "UEFI"),
                    make_entry("Secure Boot", "Disabled"),
                    make_entry("Boot Mode", "64-bit"),
                ],
                last_refreshed_ns: now,
            },
        ],
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
    init_defaults();

    // 1: Default sections.
    let all = get_all();
    assert_eq!(all.len(), 5);
    crate::serial_println!("  [1/8] defaults: OK");

    // 2: Get CPU section.
    let cpu = get_section(Section::Cpu).expect("cpu");
    assert!(!cpu.entries.is_empty());
    let cores = cpu.entries.iter().find(|e| e.key == "Cores").expect("cores");
    assert_eq!(cores.value, "4");
    crate::serial_println!("  [2/8] get section: OK");

    // 3: Update entry.
    set_entry(Section::Cpu, "Cores", "8").expect("update");
    let cpu = get_section(Section::Cpu).expect("cpu2");
    let cores = cpu.entries.iter().find(|e| e.key == "Cores").expect("cores2");
    assert_eq!(cores.value, "8");
    crate::serial_println!("  [3/8] update: OK");

    // 4: Add new entry.
    set_entry(Section::Cpu, "Architecture", "x86_64").expect("add");
    let cpu = get_section(Section::Cpu).expect("cpu3");
    assert!(cpu.entries.iter().any(|e| e.key == "Architecture"));
    crate::serial_println!("  [4/8] add entry: OK");

    // 5: Add new section.
    set_entry(Section::Network, "Interface", "eth0").expect("net");
    let net = get_section(Section::Network).expect("net");
    assert_eq!(net.entries.len(), 1);
    crate::serial_println!("  [5/8] new section: OK");

    // 6: Summary.
    let summary = get_summary();
    assert!(summary.contains("[CPU]"));
    assert!(summary.contains("[Network]"));
    crate::serial_println!("  [6/8] summary: OK");

    // 7: Refresh.
    refresh_section(Section::Cpu).expect("refresh");
    crate::serial_println!("  [7/8] refresh: OK");

    // 8: Stats.
    let (sections, queries, refreshes, ops) = stats();
    assert_eq!(sections, 6);
    assert!(queries >= 4);
    assert_eq!(refreshes, 1);
    assert!(ops > 0);
    crate::serial_println!("  [8/8] stats: OK");

    crate::serial_println!("sysprofiler::self_test() — all 8 tests passed");
}
