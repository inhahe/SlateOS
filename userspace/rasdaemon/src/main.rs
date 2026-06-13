#![deny(clippy::all)]

//! rasdaemon — Slate OS RAS (Reliability, Availability, Serviceability) event logger
//!
//! Multi-personality binary for monitoring and logging hardware error events.
//! Detected via argv[0]:
//!
//! - `rasdaemon` (default) — RAS daemon collecting hardware error events
//! - `ras-mc-ctl` — RAS memory controller status/configuration tool

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _RAS_DB_PATH: &str = "/var/lib/rasdaemon/ras-mc_event.db";
const _RAS_CONF: &str = "/etc/rasdaemon.conf";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct MemoryError {
    timestamp: u64,
    error_count: u32,
    error_type: MemErrorType,
    message: String,
    label: String,
    mc: u32,
    _top_layer: u32,
    _mid_layer: u32,
    _low_layer: u32,
    address: u64,
    grain: u64,
    syndrome: u64,
    _driver_detail: String,
}

#[derive(Clone, Debug, PartialEq)]
enum MemErrorType {
    Corrected,
    Uncorrected,
    Fatal,
    _Info,
}

impl std::fmt::Display for MemErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrected => write!(f, "Corrected"),
            Self::Uncorrected => write!(f, "Uncorrected"),
            Self::Fatal => write!(f, "Fatal"),
            Self::_Info => write!(f, "Info"),
        }
    }
}

#[derive(Clone, Debug)]
struct PcieAerEvent {
    timestamp: u64,
    dev_name: String,
    _severity: AerSeverity,
    error_type: String,
    tlp_header: String,
}

#[derive(Clone, Debug, PartialEq)]
enum AerSeverity {
    _Corrected,
    _Uncorrected,
    _Fatal,
}

impl std::fmt::Display for AerSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::_Corrected => write!(f, "Corrected"),
            Self::_Uncorrected => write!(f, "Uncorrected"),
            Self::_Fatal => write!(f, "Fatal"),
        }
    }
}

#[derive(Clone, Debug)]
struct DiskError {
    timestamp: u64,
    dev: String,
    sector: u64,
    nr_sectors: u32,
    error: String,
    _rwbs: String,
}

#[derive(Clone, Debug)]
struct McController {
    id: u32,
    name: String,
    _size_mb: u64,
    status: String,
    channels: Vec<McChannel>,
}

#[derive(Clone, Debug)]
struct McChannel {
    _id: u32,
    dimm_label: String,
    _size_mb: u64,
    ce_count: u64,
    ue_count: u64,
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_memory_errors() -> Vec<MemoryError> {
    vec![
        MemoryError {
            timestamp: 1716000000,
            error_count: 1,
            error_type: MemErrorType::Corrected,
            message: "DIMM-A1 CE rank 0 bank 5 row 1234 col 567".to_string(),
            label: "DIMM_A1".to_string(),
            mc: 0,
            _top_layer: 0,
            _mid_layer: 0,
            _low_layer: 0,
            address: 0x0000_1234_5678_0000,
            grain: 64,
            syndrome: 0x0000_00AB,
            _driver_detail: "EDAC MC0".to_string(),
        },
        MemoryError {
            timestamp: 1716003600,
            error_count: 1,
            error_type: MemErrorType::Corrected,
            message: "DIMM-A1 CE rank 0 bank 7 row 5678 col 890".to_string(),
            label: "DIMM_A1".to_string(),
            mc: 0,
            _top_layer: 0,
            _mid_layer: 0,
            _low_layer: 1,
            address: 0x0000_1234_9ABC_0000,
            grain: 64,
            syndrome: 0x0000_00CD,
            _driver_detail: "EDAC MC0".to_string(),
        },
    ]
}

fn read_pcie_errors() -> Vec<PcieAerEvent> {
    vec![
        PcieAerEvent {
            timestamp: 1716002000,
            dev_name: "0000:03:00.0 (NVMe SSD)".to_string(),
            _severity: AerSeverity::_Corrected,
            error_type: "Receiver Error".to_string(),
            tlp_header: "00000000 00000000 00000000 00000000".to_string(),
        },
    ]
}

fn read_disk_errors() -> Vec<DiskError> {
    vec![
        DiskError {
            timestamp: 1716001000,
            dev: "sda".to_string(),
            sector: 123456789,
            nr_sectors: 8,
            error: "medium error".to_string(),
            _rwbs: "R".to_string(),
        },
    ]
}

fn read_mc_controllers() -> Vec<McController> {
    vec![
        McController {
            id: 0,
            name: "Skylake IMC".to_string(),
            _size_mb: 32768,
            status: "active".to_string(),
            channels: vec![
                McChannel {
                    _id: 0,
                    dimm_label: "DIMM_A1 (16GB DDR4 3200)".to_string(),
                    _size_mb: 16384,
                    ce_count: 2,
                    ue_count: 0,
                },
                McChannel {
                    _id: 1,
                    dimm_label: "DIMM_B1 (16GB DDR4 3200)".to_string(),
                    _size_mb: 16384,
                    ce_count: 0,
                    ue_count: 0,
                },
            ],
        },
    ]
}

fn format_event_time(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    format!("{:02}:{:02}:{:02}", hours, mins, secs)
}

// ── rasdaemon personality ─────────────────────────────────────────────

fn run_rasdaemon(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--summary".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: rasdaemon [OPTIONS]");
            println!();
            println!("RAS (Reliability, Availability, Serviceability) event daemon.");
            println!();
            println!("Options:");
            println!("  --summary         Show error event summary (default)");
            println!("  --errors          Show all recorded error events");
            println!("  --record          Start recording mode (daemon)");
            println!("  --foreground      Run in foreground (with --record)");
            println!("  --show-ce         Show corrected memory errors");
            println!("  --show-ue         Show uncorrected memory errors");
            println!("  --show-aer        Show PCIe AER events");
            println!("  --show-disk       Show disk error events");
            println!("  --status          Show daemon status");
            println!("  --version         Show version");
            0
        }
        "--version" | "-V" => {
            println!("rasdaemon 0.1.0 (Slate OS)");
            0
        }
        "--summary" | "summary" => cmd_summary(),
        "--errors" | "errors" => cmd_all_errors(),
        "--record" | "record" => cmd_record(&cmd_args),
        "--show-ce" => cmd_show_ce(),
        "--show-ue" => cmd_show_ue(),
        "--show-aer" => cmd_show_aer(),
        "--show-disk" => cmd_show_disk(),
        "--status" | "status" => cmd_status(),
        other => {
            eprintln!("rasdaemon: unknown option '{}'", other);
            eprintln!("Try 'rasdaemon --help' for more information.");
            1
        }
    }
}

fn cmd_summary() -> i32 {
    let mem_errors = read_memory_errors();
    let pcie_errors = read_pcie_errors();
    let disk_errors = read_disk_errors();

    println!("RAS Error Summary");
    println!("=================");
    println!();

    let ce_count = mem_errors.iter().filter(|e| e.error_type == MemErrorType::Corrected).count();
    let ue_count = mem_errors.iter().filter(|e| e.error_type == MemErrorType::Uncorrected).count();
    println!("Memory errors:");
    println!("  Corrected (CE):   {}", ce_count);
    println!("  Uncorrected (UE): {}", ue_count);
    println!();

    println!("PCIe AER events: {}", pcie_errors.len());
    println!("Disk errors: {}", disk_errors.len());
    println!();

    // DIMM-level summary
    let mut dimm_ce: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for e in &mem_errors {
        if e.error_type == MemErrorType::Corrected {
            *dimm_ce.entry(e.label.clone()).or_insert(0) += e.error_count;
        }
    }
    if !dimm_ce.is_empty() {
        println!("Corrected errors by DIMM:");
        let mut entries: Vec<_> = dimm_ce.iter().collect();
        entries.sort_by(|a, b| b.1.cmp(a.1));
        for (label, count) in entries {
            println!("  {}: {} CE", label, count);
        }
    }
    0
}

fn cmd_all_errors() -> i32 {
    println!("All RAS Error Events");
    println!("====================");
    println!();

    cmd_show_ce();
    println!();
    cmd_show_aer();
    println!();
    cmd_show_disk();
    0
}

fn cmd_record(args: &[String]) -> i32 {
    let foreground = args.iter().any(|a| a == "--foreground" || a == "-f");
    println!("rasdaemon: starting RAS event recording");
    println!("  Database: {}", _RAS_DB_PATH);
    println!("  Mode: {}", if foreground { "foreground" } else { "daemon" });
    println!();
    println!("Monitoring tracepoints:");
    println!("  ras:mc_event (memory errors)");
    println!("  ras:aer_event (PCIe errors)");
    println!("  ras:non_standard_event");
    println!("  block:block_rq_complete (disk errors)");
    println!();
    println!("rasdaemon: recording started (simulated)");
    0
}

fn cmd_show_ce() -> i32 {
    let errors = read_memory_errors();
    let ces: Vec<_> = errors.iter()
        .filter(|e| e.error_type == MemErrorType::Corrected)
        .collect();

    println!("Corrected Memory Errors ({} total):", ces.len());
    println!("{:<12} {:<8} {:<10} {:<18} {:<8} {:>10}",
        "Time", "Count", "Label", "Address", "Grain", "Syndrome");
    println!("{}", "-".repeat(70));

    for e in &ces {
        println!("{:<12} {:<8} {:<10} {:#018x} {:<8} {:#010x}",
            format_event_time(e.timestamp),
            e.error_count,
            e.label,
            e.address,
            e.grain,
            e.syndrome);
    }
    0
}

fn cmd_show_ue() -> i32 {
    let errors = read_memory_errors();
    let ues: Vec<_> = errors.iter()
        .filter(|e| e.error_type == MemErrorType::Uncorrected || e.error_type == MemErrorType::Fatal)
        .collect();

    if ues.is_empty() {
        println!("No uncorrected memory errors recorded.");
    } else {
        println!("Uncorrected Memory Errors ({} total):", ues.len());
        for e in &ues {
            println!("[{}] MC{} {}: {} @ {:#018x}",
                format_event_time(e.timestamp),
                e.mc, e.error_type, e.message, e.address);
        }
    }
    0
}

fn cmd_show_aer() -> i32 {
    let errors = read_pcie_errors();
    println!("PCIe AER Events ({} total):", errors.len());
    println!("{:<12} {:<30} {:<20}",
        "Time", "Device", "Error");
    println!("{}", "-".repeat(65));

    for e in &errors {
        println!("{:<12} {:<30} {:<20}",
            format_event_time(e.timestamp),
            e.dev_name,
            e.error_type);
        println!("  TLP Header: {}", e.tlp_header);
    }
    0
}

fn cmd_show_disk() -> i32 {
    let errors = read_disk_errors();
    println!("Disk Error Events ({} total):", errors.len());
    println!("{:<12} {:<8} {:>12} {:>8} {:<20}",
        "Time", "Device", "Sector", "Count", "Error");
    println!("{}", "-".repeat(65));

    for e in &errors {
        println!("{:<12} {:<8} {:>12} {:>8} {:<20}",
            format_event_time(e.timestamp),
            e.dev,
            e.sector,
            e.nr_sectors,
            e.error);
    }
    0
}

fn cmd_status() -> i32 {
    println!("rasdaemon status:");
    println!("  Running: yes (simulated)");
    println!("  Database: {}", _RAS_DB_PATH);
    println!("  Events recorded:");
    println!("    Memory CE: 2");
    println!("    Memory UE: 0");
    println!("    PCIe AER:  1");
    println!("    Disk:      1");
    println!("  Uptime: 86400 seconds");
    0
}

// ── ras-mc-ctl personality ────────────────────────────────────────────

fn run_mc_ctl(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "--status".to_string());

    match cmd.as_str() {
        "--help" | "help" | "-h" => {
            println!("Usage: ras-mc-ctl [OPTIONS]");
            println!();
            println!("RAS memory controller status and configuration tool.");
            println!();
            println!("Options:");
            println!("  --status           Show memory controller status (default)");
            println!("  --mainboard        Show mainboard information");
            println!("  --summary          Show error summary");
            println!("  --errors           Show EDAC error counts");
            println!("  --layout           Show DIMM layout");
            println!("  --register-labels  Register DIMM labels");
            println!("  --guess-labels     Auto-detect DIMM labels");
            0
        }
        "--status" | "status" => mc_status(),
        "--mainboard" | "mainboard" => mc_mainboard(),
        "--summary" | "summary" => mc_summary(),
        "--errors" | "errors" => mc_errors(),
        "--layout" | "layout" => mc_layout(),
        "--register-labels" => {
            println!("ras-mc-ctl: registering DIMM labels from DMI data...");
            println!("Registered 2 DIMM labels");
            0
        }
        "--guess-labels" => {
            println!("ras-mc-ctl: auto-detecting DIMM labels...");
            println!("Detected mainboard: ASUS PRIME Z690-P");
            println!("Guessed labels for 2 DIMMs");
            0
        }
        other => {
            eprintln!("ras-mc-ctl: unknown option '{}'", other);
            1
        }
    }
}

fn mc_status() -> i32 {
    let controllers = read_mc_controllers();

    println!("Memory Controller Status");
    println!("========================");
    println!();

    for mc in &controllers {
        println!("MC{}: {} ({})", mc.id, mc.name, mc.status);
        for ch in &mc.channels {
            let status = if ch.ue_count > 0 {
                "FAIL"
            } else if ch.ce_count > 0 {
                "DEGRADED"
            } else {
                "OK"
            };
            println!("  {}: CE={} UE={} [{}]",
                ch.dimm_label, ch.ce_count, ch.ue_count, status);
        }
        println!();
    }
    0
}

fn mc_mainboard() -> i32 {
    println!("Mainboard Information");
    println!("=====================");
    println!("  Manufacturer: Slate OS Virtual Hardware");
    println!("  Product: Virtual Desktop Board");
    println!("  BIOS: Slate OS BIOS v1.0");
    println!("  Memory controller: Integrated IMC");
    0
}

fn mc_summary() -> i32 {
    let controllers = read_mc_controllers();
    let mut total_ce: u64 = 0;
    let mut total_ue: u64 = 0;

    for mc in &controllers {
        for ch in &mc.channels {
            total_ce += ch.ce_count;
            total_ue += ch.ue_count;
        }
    }

    println!("EDAC Error Summary");
    println!("==================");
    println!("  Total corrected (CE): {}", total_ce);
    println!("  Total uncorrected (UE): {}", total_ue);
    println!("  Memory controllers: {}", controllers.len());

    if total_ue > 0 {
        println!();
        println!("  WARNING: Uncorrected errors detected! Replace affected DIMM.");
    } else if total_ce > 10 {
        println!();
        println!("  WARNING: High corrected error count. Monitor for further degradation.");
    }
    0
}

fn mc_errors() -> i32 {
    let controllers = read_mc_controllers();

    println!("EDAC Error Counts by DIMM");
    println!("=========================");
    println!();
    println!("{:<8} {:<30} {:>8} {:>8}",
        "MC", "DIMM", "CE", "UE");
    println!("{}", "-".repeat(58));

    for mc in &controllers {
        for ch in &mc.channels {
            println!("mc{:<5} {:<30} {:>8} {:>8}",
                mc.id, ch.dimm_label, ch.ce_count, ch.ue_count);
        }
    }
    0
}

fn mc_layout() -> i32 {
    let controllers = read_mc_controllers();

    println!("DIMM Layout");
    println!("===========");
    println!();

    for mc in &controllers {
        println!("Memory Controller {} ({}):", mc.id, mc.name);
        for (i, ch) in mc.channels.iter().enumerate() {
            println!("  Channel {}: {}", i, ch.dimm_label);
        }
        println!();
    }
    0
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("rasdaemon");
        let bytes = s.as_bytes();
        let mut last_sep = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == b'/' || b == b'\\' {
                last_sep = i + 1;
            }
        }
        let base = &s[last_sep..];
        let base = base.strip_suffix(".exe").unwrap_or(base);
        base.to_string()
    };

    let rest: Vec<String> = args.into_iter().skip(1).collect();

    let code = match prog_name.as_str() {
        "ras-mc-ctl" => run_mc_ctl(rest),
        _ => run_rasdaemon(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_memory_errors() {
        let errors = read_memory_errors();
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].error_type, MemErrorType::Corrected);
        assert_eq!(errors[0].label, "DIMM_A1");
    }

    #[test]
    fn test_read_pcie_errors() {
        let errors = read_pcie_errors();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].dev_name.contains("NVMe"));
    }

    #[test]
    fn test_read_disk_errors() {
        let errors = read_disk_errors();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].dev, "sda");
    }

    #[test]
    fn test_read_mc_controllers() {
        let mcs = read_mc_controllers();
        assert_eq!(mcs.len(), 1);
        assert_eq!(mcs[0].channels.len(), 2);
        assert!(mcs[0].channels[0].ce_count > 0);
    }

    #[test]
    fn test_format_event_time() {
        assert_eq!(format_event_time(0), "unknown");
        let t = format_event_time(3661);
        assert_eq!(t, "01:01:01");
    }

    #[test]
    fn test_mem_error_type_display() {
        assert_eq!(format!("{}", MemErrorType::Corrected), "Corrected");
        assert_eq!(format!("{}", MemErrorType::Uncorrected), "Uncorrected");
        assert_eq!(format!("{}", MemErrorType::Fatal), "Fatal");
    }

    #[test]
    fn test_aer_severity_display() {
        assert_eq!(format!("{}", AerSeverity::_Corrected), "Corrected");
        assert_eq!(format!("{}", AerSeverity::_Fatal), "Fatal");
    }

    #[test]
    fn test_mc_channel_status_logic() {
        let ch = McChannel {
            _id: 0,
            dimm_label: "DIMM_A1".to_string(),
            _size_mb: 16384,
            ce_count: 5,
            ue_count: 0,
        };
        let status = if ch.ue_count > 0 { "FAIL" }
            else if ch.ce_count > 0 { "DEGRADED" }
            else { "OK" };
        assert_eq!(status, "DEGRADED");
    }
}
