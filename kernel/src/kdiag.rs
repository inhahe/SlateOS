//! Kernel diagnostic report generator.
//!
//! Produces a comprehensive system state report by gathering data from
//! all major subsystems.  This is the "bug report" command — run it when
//! something seems wrong and it collects everything needed for analysis.
//!
//! ## Report Sections
//!
//! 1. **System**: uptime, CPU count, boot info
//! 2. **Memory**: physical frames, heap, fragmentation, pressure
//! 3. **Scheduler**: task counts, context switches, load
//! 4. **IPC**: channel/pipe/futex usage
//! 5. **Objects**: kernel object counts, leaks
//! 6. **Capabilities**: audit summary
//! 7. **Invariants**: pass/fail status of all consistency checks
//! 8. **Health**: overall system health score
//!
//! ## Usage
//!
//! ```text
//! kshell> diag            — full diagnostic report
//! kshell> diag summary    — one-line health summary
//! kshell> diag memory     — memory section only
//! ```
//!
//! ## Design
//!
//! Each section is a pure function that reads subsystem stats and formats
//! them.  No side effects, no allocations on the collection path (except
//! for the final string formatting).  Safe to run at any time.
//!
//! ## References
//!
//! - Linux `dmesg` + `/proc/sysrq-trigger` (SysRq-T for task dump)
//! - Windows `msinfo32` / `windbg !analyze`
//! - macOS `sysdiagnose`

use alloc::string::String;
use alloc::vec::Vec;
use crate::serial_println;

// ---------------------------------------------------------------------------
// Report structure
// ---------------------------------------------------------------------------

/// A single section of the diagnostic report.
#[derive(Debug, Clone)]
pub struct DiagSection {
    /// Section title.
    pub title: &'static str,
    /// Section content lines.
    pub lines: Vec<String>,
    /// Health status for this section.
    pub health: SectionHealth,
}

/// Health status of a diagnostic section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionHealth {
    /// Everything is normal.
    Good,
    /// Some values are noteworthy but not problematic.
    Info,
    /// Potential issues detected.
    Warning,
    /// Problems detected that need attention.
    Critical,
}

impl SectionHealth {
    /// Display label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Good => "OK",
            Self::Info => "INFO",
            Self::Warning => "WARN",
            Self::Critical => "CRIT",
        }
    }
}

/// Full diagnostic report.
#[derive(Debug, Clone)]
pub struct DiagReport {
    /// Report sections.
    pub sections: Vec<DiagSection>,
    /// Overall health (worst of all sections).
    pub overall: SectionHealth,
}

// ---------------------------------------------------------------------------
// Section generators
// ---------------------------------------------------------------------------

/// System overview section.
fn section_system() -> DiagSection {
    let ticks = crate::apic::tick_count();
    let seconds = ticks / 100;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    let cpu_count = crate::smp::cpu_count();

    let lines = alloc::vec![
        alloc::format!("Uptime: {:02}:{:02}:{:02} ({} ticks)", hours, minutes, secs, ticks),
        alloc::format!("CPUs: {} online", cpu_count),
    ];

    DiagSection {
        title: "System",
        lines,
        health: SectionHealth::Good,
    }
}

/// Memory subsystem section.
fn section_memory() -> DiagSection {
    let info = crate::mm::memory_info();
    let pressure = crate::mm::memory_pressure();
    let hs = crate::mm::heap::stats();
    let net_heap = hs.slab_allocs.saturating_sub(hs.slab_frees);

    let mut health = SectionHealth::Good;
    if pressure.score > 80 {
        health = SectionHealth::Critical;
    } else if pressure.score > 50 {
        health = SectionHealth::Warning;
    } else if info.fragmentation_pct > 50 {
        health = SectionHealth::Warning;
    }

    let total_mb = info.total_bytes / (1024 * 1024);
    let used_mb = info.used_bytes / (1024 * 1024);
    let free_mb = info.free_bytes / (1024 * 1024);

    let lines = alloc::vec![
        alloc::format!("Physical: {} MiB total, {} MiB used, {} MiB free",
            total_mb, used_mb, free_mb),
        alloc::format!("Frames: {} total, {} free, {} used",
            info.total_frames, info.free_frames,
            info.total_frames.saturating_sub(info.free_frames)),
        alloc::format!("Fragmentation: {}%", info.fragmentation_pct),
        alloc::format!("Pressure: score={} level={:?}", pressure.score, pressure.level),
        alloc::format!("Heap: {} allocs, {} frees, {} active",
            hs.slab_allocs, hs.slab_frees, net_heap),
    ];

    DiagSection {
        title: "Memory",
        lines,
        health,
    }
}

/// Scheduler section.
fn section_scheduler() -> DiagSection {
    let stats = crate::sched::sched_stats();
    let load = crate::sched::load_average_x100();
    let active = stats.total_tasks_spawned.saturating_sub(stats.total_tasks_exited);

    let mut health = SectionHealth::Good;
    // High load relative to CPU count is noteworthy.
    let cpu_count = crate::smp::cpu_count() as u64;
    if load > cpu_count * 200 {
        health = SectionHealth::Warning;
    }

    let lines = alloc::vec![
        alloc::format!("Tasks: {} active ({} spawned, {} exited)",
            active, stats.total_tasks_spawned, stats.total_tasks_exited),
        alloc::format!("Context switches: {} total", stats.total_ctx_switches),
        alloc::format!("Work steals: {}", stats.total_work_steals),
        alloc::format!("Load average: {}.{:02}", load / 100, load % 100),
    ];

    DiagSection {
        title: "Scheduler",
        lines,
        health,
    }
}

/// IPC subsystem section.
fn section_ipc() -> DiagSection {
    let s = crate::ipc::stats::snapshot();
    let total = crate::ipc::stats::total_operations();

    let lines = alloc::vec![
        alloc::format!("Total IPC operations: {}", total),
        alloc::format!("Channels: {} created, {} destroyed, {} sends, {} recvs",
            s.channels_created, s.channels_destroyed,
            s.channel_sends, s.channel_recvs),
        alloc::format!("Pipes: {} created, {} reads, {} writes",
            s.pipes_created, s.pipe_reads, s.pipe_writes),
        alloc::format!("Futex: {} waits, {} wakes",
            s.futex_waits, s.futex_wakes),
    ];

    DiagSection {
        title: "IPC",
        lines,
        health: SectionHealth::Good,
    }
}

/// Kernel objects section.
fn section_objects() -> DiagSection {
    let total_active = crate::kobject::total_active();
    let all = crate::kobject::all_stats();

    let mut health = SectionHealth::Good;

    let mut lines = alloc::vec![
        alloc::format!("Active objects: {}", total_active),
    ];

    for s in &all {
        if s.created > 0 {
            let active = s.created.saturating_sub(s.destroyed);
            lines.push(alloc::format!("  {}: {} created, {} destroyed, {} active, hwm={}",
                s.obj_type.name(), s.created, s.destroyed, active, s.high_water));
            // Large number of active objects might indicate a leak.
            if active > 100 {
                health = SectionHealth::Warning;
            }
        }
    }

    DiagSection {
        title: "Kernel Objects",
        lines,
        health,
    }
}

/// Invariant check section.
fn section_invariants() -> DiagSection {
    let results = crate::invariant::check_all();

    let mut health = if results.failed == 0 {
        SectionHealth::Good
    } else {
        SectionHealth::Critical
    };

    let mut lines = alloc::vec![
        alloc::format!("{}/{} invariants passed", results.passed, results.total),
    ];

    for r in &results.results {
        let status = if r.passed { "PASS" } else { "FAIL" };
        let msg = r.message.as_deref().unwrap_or("");
        lines.push(alloc::format!("  [{}] {}: {}", status, r.name, msg));
        if !r.passed {
            health = SectionHealth::Critical;
        }
    }

    DiagSection {
        title: "Invariants",
        lines,
        health,
    }
}

/// Capability audit section.
fn section_capabilities() -> DiagSection {
    let s = crate::cap::audit::stats();

    let mut health = SectionHealth::Good;
    if s.total_denials > 0 {
        health = SectionHealth::Info;
    }

    let lines = alloc::vec![
        alloc::format!("Audit events: {}", s.total_events),
        alloc::format!("Denials: {}", s.total_denials),
    ];

    DiagSection {
        title: "Capabilities",
        lines,
        health,
    }
}

/// Migration tracking section.
fn section_migrations() -> DiagSection {
    let s = crate::sched_migrate::stats();

    let mut health = SectionHealth::Good;
    // High migration count might indicate poor affinity.
    if s.total > 1000 {
        health = SectionHealth::Info;
    }

    let mut lines = alloc::vec![
        alloc::format!("Total migrations: {}", s.total),
    ];

    if s.total > 0 {
        lines.push(alloc::format!("  Work steal: {}, Push balance: {}, Wake-up: {}",
            s.by_reason[0], s.by_reason[1], s.by_reason[4]));
        if let Some((from, to, count)) = crate::sched_migrate::hottest_path() {
            lines.push(alloc::format!("  Hottest path: CPU{} → CPU{} ({}x)",
                from, to, count));
        }
    }

    DiagSection {
        title: "Migrations",
        lines,
        health,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a full diagnostic report.
pub fn full_report() -> DiagReport {
    let sections = alloc::vec![
        section_system(),
        section_memory(),
        section_scheduler(),
        section_ipc(),
        section_objects(),
        section_capabilities(),
        section_migrations(),
        section_invariants(),
    ];

    // Overall health = worst across all sections.
    let overall = sections.iter()
        .map(|s| s.health)
        .max_by_key(|h| *h as u8)
        .unwrap_or(SectionHealth::Good);

    DiagReport { sections, overall }
}

/// Generate a one-line health summary.
pub fn health_summary() -> (SectionHealth, String) {
    let report = full_report();
    let warnings: Vec<&str> = report.sections.iter()
        .filter(|s| s.health as u8 >= SectionHealth::Warning as u8)
        .map(|s| s.title)
        .collect();

    let msg = if warnings.is_empty() {
        String::from("All systems nominal")
    } else {
        alloc::format!("Issues in: {}", warnings.join(", "))
    };

    (report.overall, msg)
}

/// Get a specific section by name.
pub fn section(name: &str) -> Option<DiagSection> {
    match name {
        "system" => Some(section_system()),
        "memory" | "mem" | "mm" => Some(section_memory()),
        "scheduler" | "sched" => Some(section_scheduler()),
        "ipc" => Some(section_ipc()),
        "objects" | "obj" => Some(section_objects()),
        "capabilities" | "cap" => Some(section_capabilities()),
        "migrations" | "migrate" => Some(section_migrations()),
        "invariants" | "invar" => Some(section_invariants()),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Self-test
// ---------------------------------------------------------------------------

/// Self-test for the diagnostic report generator.
pub fn self_test() {
    serial_println!("[kdiag] Running self-test...");

    // Test 1: Full report generates all sections.
    let report = full_report();
    assert!(!report.sections.is_empty(), "report should have sections");
    assert!(report.sections.len() >= 7, "should have at least 7 sections");
    serial_println!("[kdiag]   Full report: OK ({} sections, overall={})",
        report.sections.len(), report.overall.label());

    // Test 2: Each section has content.
    for s in &report.sections {
        assert!(!s.lines.is_empty(),
            "section '{}' should have content", s.title);
    }
    serial_println!("[kdiag]   Section content: OK");

    // Test 3: Health summary works.
    let (health, msg) = health_summary();
    assert!(!msg.is_empty());
    serial_println!("[kdiag]   Health summary: {} ({})", health.label(), msg);

    // Test 4: Section lookup works.
    assert!(section("memory").is_some());
    assert!(section("sched").is_some());
    assert!(section("nonexistent").is_none());
    serial_println!("[kdiag]   Section lookup: OK");

    // Test 5: During boot, everything should be healthy.
    assert!(report.overall as u8 <= SectionHealth::Info as u8,
        "boot-time report should not have warnings");
    serial_println!("[kdiag]   Boot health: OK");

    serial_println!("[kdiag] Self-test PASSED");
}
