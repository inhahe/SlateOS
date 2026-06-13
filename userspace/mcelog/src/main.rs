#![deny(clippy::all)]

//! mcelog — SlateOS machine check exception logger
//!
//! Multi-personality binary for handling CPU hardware errors (MCEs).
//! Detected via argv[0]:
//!
//! - `mcelog` (default) — machine check exception logger/daemon
//! - `mcelog-client` — query running mcelog daemon

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _MCE_LOG_FILE: &str = "/var/log/mcelog";
const _MCE_SOCKET: &str = "/var/run/mcelog-client";
const _MCE_CONF: &str = "/etc/mcelog/mcelog.conf";
const _MCE_TRIGGER_DIR: &str = "/etc/mcelog/triggers";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct MceEvent {
    cpu: u32,
    bank: u32,
    status: u64,
    address: u64,
    misc: u64,
    timestamp: u64,
    severity: MceSeverity,
    error_type: MceErrorType,
    _corrected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MceSeverity {
    Corrected,
    _Deferred,
    Uncorrected,
    Fatal,
}

impl std::fmt::Display for MceSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrected => write!(f, "corrected"),
            Self::_Deferred => write!(f, "deferred"),
            Self::Uncorrected => write!(f, "uncorrected"),
            Self::Fatal => write!(f, "fatal"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MceErrorType {
    MemoryController,
    CacheHierarchy,
    Bus,
    Tlb,
    InternalParity,
    _InternalTimer,
    _ExternalError,
    _FRC,
    Unknown,
}

impl std::fmt::Display for MceErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemoryController => write!(f, "memory controller"),
            Self::CacheHierarchy => write!(f, "cache hierarchy"),
            Self::Bus => write!(f, "bus/interconnect"),
            Self::Tlb => write!(f, "TLB"),
            Self::InternalParity => write!(f, "internal parity"),
            Self::_InternalTimer => write!(f, "internal timer"),
            Self::_ExternalError => write!(f, "external"),
            Self::_FRC => write!(f, "FRC"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Clone, Debug)]
struct _CpuInfo {
    _vendor: String,
    _family: u32,
    _model: u32,
    _stepping: u32,
    _microcode: u64,
}

#[derive(Clone, Debug)]
struct DimmInfo {
    location: String,
    _size_mb: u64,
    corrected_errors: u64,
    uncorrected_errors: u64,
}

#[derive(Clone, Debug)]
struct _MceConfig {
    _daemon_mode: bool,
    _socket_path: String,
    _log_file: String,
    _syslog: bool,
    _filter_memory_errors: bool,
    _trigger_dir: String,
}

impl Default for _MceConfig {
    fn default() -> Self {
        Self {
            _daemon_mode: false,
            _socket_path: _MCE_SOCKET.to_string(),
            _log_file: _MCE_LOG_FILE.to_string(),
            _syslog: true,
            _filter_memory_errors: false,
            _trigger_dir: _MCE_TRIGGER_DIR.to_string(),
        }
    }
}

// ── MCE decoding ──────────────────────────────────────────────────────

fn decode_mce_status(status: u64) -> (MceErrorType, MceSeverity) {
    // MCA status register layout (Intel):
    // Bits 15:0  — MCA error code
    // Bits 31:16 — Model-specific error code
    // Bit 57     — Corrected error count valid
    // Bit 58     — Status register valid
    // Bit 59     — Miscellaneous register valid
    // Bit 60     — Error condition enabled
    // Bit 61     — Uncorrected error
    // Bit 62     — Error overflow
    // Bit 63     — Valid

    let mca_code = (status & 0xFFFF) as u16;
    let uncorrected = (status >> 61) & 1 != 0;
    let overflow = (status >> 62) & 1 != 0;

    let error_type = if mca_code & 0x0800 != 0 {
        MceErrorType::Bus
    } else if mca_code & 0x0100 != 0 {
        // Cache hierarchy error (bits 1:0 = level, bits 3:2 = transaction type)
        MceErrorType::CacheHierarchy
    } else if mca_code & 0x0010 != 0 {
        MceErrorType::Tlb
    } else {
        match mca_code & 0x000F {
            0x0001 => MceErrorType::InternalParity,
            0x0005 => MceErrorType::MemoryController,
            _ => MceErrorType::Unknown,
        }
    };

    let severity = if overflow && uncorrected {
        MceSeverity::Fatal
    } else if uncorrected {
        MceSeverity::Uncorrected
    } else {
        MceSeverity::Corrected
    };

    (error_type, severity)
}

fn format_mce_event(event: &MceEvent) -> String {
    let mut lines = Vec::new();
    lines.push(format!("MCE {} on CPU {}", event.severity, event.cpu));
    lines.push(format!("  Bank: {}", event.bank));
    lines.push(format!("  Status: {:#018x}", event.status));
    if event.address != 0 {
        lines.push(format!("  Address: {:#018x}", event.address));
    }
    if event.misc != 0 {
        lines.push(format!("  Misc: {:#018x}", event.misc));
    }
    lines.push(format!("  Error type: {}", event.error_type));
    lines.push(format!("  Time: {}", format_timestamp(event.timestamp)));
    lines.join("\n")
}

fn format_timestamp(ts: u64) -> String {
    if ts == 0 {
        return "unknown".to_string();
    }
    // Simple formatting — real impl would use proper time conversion
    let secs = ts % 60;
    let mins = (ts / 60) % 60;
    let hours = (ts / 3600) % 24;
    let days = ts / 86400;
    format!("day {} {:02}:{:02}:{:02}", days, hours, mins, secs)
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_mce_events() -> Vec<MceEvent> {
    // In a real kernel, read from /dev/mcelog or kernel MCE ring buffer
    let statuses: &[(u64, u32, u32)] = &[
        (0x8000004000010005, 0, 3),  // Corrected memory controller error
        (0xA000004000010005, 1, 3),  // UC memory controller error
        (0x8000004000010100, 0, 1),  // Corrected cache hierarchy error
    ];

    statuses.iter().map(|&(status, cpu, bank)| {
        let (error_type, severity) = decode_mce_status(status);
        MceEvent {
            cpu,
            bank,
            status,
            address: 0x0000_7F80_1234_0000,
            misc: 0,
            timestamp: 1716000000 + cpu as u64 * 100,
            severity,
            error_type,
            _corrected: severity == MceSeverity::Corrected,
        }
    }).collect()
}

fn read_dimm_info() -> Vec<DimmInfo> {
    vec![
        DimmInfo {
            location: "DIMM_A1 (Channel 0, Slot 0)".to_string(),
            _size_mb: 16384,
            corrected_errors: 3,
            uncorrected_errors: 0,
        },
        DimmInfo {
            location: "DIMM_B1 (Channel 1, Slot 0)".to_string(),
            _size_mb: 16384,
            corrected_errors: 0,
            uncorrected_errors: 0,
        },
    ]
}

// ── mcelog personality ────────────────────────────────────────────────

fn run_mcelog(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "summary".to_string());
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "--help" | "help" => {
            println!("Usage: mcelog [COMMAND]");
            println!();
            println!("Machine check exception logger.");
            println!();
            println!("Commands:");
            println!("  summary          Show MCE event summary (default)");
            println!("  show             Show all MCE events in detail");
            println!("  --daemon         Run as daemon reading /dev/mcelog");
            println!("  --client         Query running mcelog daemon");
            println!("  --ascii          Decode MCE events from stdin");
            println!("  --drop-old-memory Drop old corrected memory errors");
            println!("  --dmi            Show DIMM error information");
            println!("  --version        Show version");
            0
        }
        "--version" | "version" => {
            println!("mcelog 0.1.0 (SlateOS)");
            0
        }
        "summary" => cmd_summary(),
        "show" => cmd_show(),
        "--daemon" | "daemon" => cmd_daemon(),
        "--client" | "client" => cmd_client(),
        "--ascii" | "ascii" => cmd_ascii(),
        "--drop-old-memory" => cmd_drop_old_memory(&cmd_args),
        "--dmi" | "dmi" => cmd_dmi(),
        other => {
            eprintln!("mcelog: unknown command '{}'", other);
            eprintln!("Try 'mcelog --help' for more information.");
            1
        }
    }
}

fn cmd_summary() -> i32 {
    let events = read_mce_events();
    if events.is_empty() {
        println!("No machine check events recorded.");
        return 0;
    }

    let corrected = events.iter().filter(|e| e.severity == MceSeverity::Corrected).count();
    let uncorrected = events.iter().filter(|e| e.severity == MceSeverity::Uncorrected).count();
    let fatal = events.iter().filter(|e| e.severity == MceSeverity::Fatal).count();

    println!("Machine Check Event Summary");
    println!("===========================");
    println!();
    println!("Total events: {}", events.len());
    println!("  Corrected:   {}", corrected);
    println!("  Uncorrected: {}", uncorrected);
    println!("  Fatal:       {}", fatal);
    println!();

    // Group by CPU
    let mut cpu_counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
    for e in &events {
        *cpu_counts.entry(e.cpu).or_insert(0) += 1;
    }
    println!("Events by CPU:");
    let mut cpus: Vec<_> = cpu_counts.iter().collect();
    cpus.sort_by_key(|&(cpu, _)| *cpu);
    for (cpu, count) in cpus {
        println!("  CPU {}: {} events", cpu, count);
    }

    // Group by error type
    println!();
    println!("Events by type:");
    let mut type_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for e in &events {
        *type_counts.entry(format!("{}", e.error_type)).or_insert(0) += 1;
    }
    let mut types: Vec<_> = type_counts.iter().collect();
    types.sort_by_key(|&(_, count)| std::cmp::Reverse(*count));
    for (error_type, count) in types {
        println!("  {}: {} events", error_type, count);
    }
    0
}

fn cmd_show() -> i32 {
    let events = read_mce_events();
    if events.is_empty() {
        println!("No machine check events recorded.");
        return 0;
    }

    for (i, event) in events.iter().enumerate() {
        if i > 0 {
            println!();
        }
        println!("Event #{}:", i);
        println!("{}", format_mce_event(event));
    }
    0
}

fn cmd_daemon() -> i32 {
    println!("mcelog: starting daemon mode");
    println!("  Listening on: {}", _MCE_SOCKET);
    println!("  Log file: {}", _MCE_LOG_FILE);
    println!("  Config: {}", _MCE_CONF);
    println!();
    println!("mcelog: daemon ready, monitoring /dev/mcelog");
    println!("mcelog: (simulated — no real /dev/mcelog in this environment)");
    0
}

fn cmd_client() -> i32 {
    println!("mcelog: connecting to daemon at {}", _MCE_SOCKET);
    println!();
    // In real impl, would connect to daemon socket and retrieve status
    cmd_summary()
}

fn cmd_ascii() -> i32 {
    println!("mcelog: reading MCE records from stdin (ASCII mode)");
    println!("mcelog: paste MCE records or pipe from dmesg");
    println!();
    println!("Example MCE record format:");
    println!("  MCE 0");
    println!("  CPU 0 BANK 3");
    println!("  STATUS 0x8000004000010005");
    println!("  ADDR 0x00007f8012340000");
    println!();

    // Demonstrate decoding a sample
    let (error_type, severity) = decode_mce_status(0x8000004000010005);
    println!("Decoded sample STATUS 0x8000004000010005:");
    println!("  Error type: {}", error_type);
    println!("  Severity: {}", severity);
    0
}

fn cmd_drop_old_memory(args: &[String]) -> i32 {
    let threshold_days: u64 = args.first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(30);

    let events = read_mce_events();
    let memory_events: Vec<_> = events.iter()
        .filter(|e| e.error_type == MceErrorType::MemoryController)
        .filter(|e| e.severity == MceSeverity::Corrected)
        .collect();

    println!("Dropping corrected memory errors older than {} days", threshold_days);
    println!("Found {} corrected memory errors", memory_events.len());
    println!("Dropped 0 old entries (simulated)");
    0
}

fn cmd_dmi() -> i32 {
    let dimms = read_dimm_info();

    println!("DIMM Error Information");
    println!("======================");
    println!();

    for dimm in &dimms {
        println!("{}:", dimm.location);
        println!("  Corrected errors:   {}", dimm.corrected_errors);
        println!("  Uncorrected errors: {}", dimm.uncorrected_errors);
        if dimm.corrected_errors > 0 {
            println!("  Status: WARNING — corrected errors detected");
        } else {
            println!("  Status: OK");
        }
        println!();
    }
    0
}

// ── mcelog-client personality ─────────────────────────────────────────

fn run_client(args: Vec<String>) -> i32 {
    let cmd = args.first().cloned().unwrap_or_else(|| "status".to_string());

    match cmd.as_str() {
        "--help" | "help" => {
            println!("Usage: mcelog-client [COMMAND]");
            println!();
            println!("Query running mcelog daemon.");
            println!();
            println!("Commands:");
            println!("  status    Show daemon status (default)");
            println!("  ping      Check if daemon is running");
            println!("  dump      Dump all events from daemon");
            0
        }
        "status" => {
            println!("mcelog daemon status:");
            println!("  Socket: {}", _MCE_SOCKET);
            println!("  Running: yes (simulated)");
            println!("  Events recorded: 3");
            println!("  Uptime: 12345 seconds");
            0
        }
        "ping" => {
            println!("mcelog daemon: alive (simulated)");
            0
        }
        "dump" => {
            cmd_show()
        }
        other => {
            eprintln!("mcelog-client: unknown command '{}'", other);
            1
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("mcelog");
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
        "mcelog-client" => run_client(rest),
        _ => run_mcelog(rest),
    };

    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_corrected_memory() {
        let (error_type, severity) = decode_mce_status(0x8000004000010005);
        assert_eq!(error_type, MceErrorType::MemoryController);
        assert_eq!(severity, MceSeverity::Corrected);
    }

    #[test]
    fn test_decode_uncorrected_memory() {
        let (error_type, severity) = decode_mce_status(0xA000004000010005);
        assert_eq!(error_type, MceErrorType::MemoryController);
        assert_eq!(severity, MceSeverity::Uncorrected);
    }

    #[test]
    fn test_decode_cache_hierarchy() {
        let (error_type, severity) = decode_mce_status(0x8000004000010100);
        assert_eq!(error_type, MceErrorType::CacheHierarchy);
        assert_eq!(severity, MceSeverity::Corrected);
    }

    #[test]
    fn test_decode_bus_error() {
        let (error_type, severity) = decode_mce_status(0xA000004000010800);
        assert_eq!(error_type, MceErrorType::Bus);
        assert_eq!(severity, MceSeverity::Uncorrected);
    }

    #[test]
    fn test_decode_tlb_error() {
        let (error_type, severity) = decode_mce_status(0x8000004000010010);
        assert_eq!(error_type, MceErrorType::Tlb);
        assert_eq!(severity, MceSeverity::Corrected);
    }

    #[test]
    fn test_decode_fatal_overflow() {
        // UC + overflow = fatal
        let (_, severity) = decode_mce_status(0xE000004000010005);
        assert_eq!(severity, MceSeverity::Fatal);
    }

    #[test]
    fn test_decode_internal_parity() {
        let (error_type, _) = decode_mce_status(0x8000004000010001);
        assert_eq!(error_type, MceErrorType::InternalParity);
    }

    #[test]
    fn test_decode_unknown() {
        let (error_type, _) = decode_mce_status(0x8000004000010003);
        assert_eq!(error_type, MceErrorType::Unknown);
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(0), "unknown");
        assert_eq!(format_timestamp(86400 + 3661), "day 1 01:01:01");
    }

    #[test]
    fn test_format_mce_event() {
        let event = MceEvent {
            cpu: 0,
            bank: 3,
            status: 0x8000004000010005,
            address: 0x1000,
            misc: 0,
            timestamp: 100,
            severity: MceSeverity::Corrected,
            error_type: MceErrorType::MemoryController,
            _corrected: true,
        };
        let output = format_mce_event(&event);
        assert!(output.contains("corrected"));
        assert!(output.contains("CPU 0"));
        assert!(output.contains("Bank: 3"));
        assert!(output.contains("memory controller"));
    }

    #[test]
    fn test_read_mce_events() {
        let events = read_mce_events();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].severity, MceSeverity::Corrected);
        assert_eq!(events[1].severity, MceSeverity::Uncorrected);
    }

    #[test]
    fn test_read_dimm_info() {
        let dimms = read_dimm_info();
        assert_eq!(dimms.len(), 2);
        assert!(dimms[0].corrected_errors > 0);
        assert_eq!(dimms[1].corrected_errors, 0);
    }

    #[test]
    fn test_severity_display() {
        assert_eq!(format!("{}", MceSeverity::Corrected), "corrected");
        assert_eq!(format!("{}", MceSeverity::Fatal), "fatal");
    }

    #[test]
    fn test_error_type_display() {
        assert_eq!(format!("{}", MceErrorType::Bus), "bus/interconnect");
        assert_eq!(format!("{}", MceErrorType::Tlb), "TLB");
    }
}
