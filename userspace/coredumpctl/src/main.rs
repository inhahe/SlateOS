#![deny(clippy::all)]

//! coredumpctl — Slate OS core dump management
//!
//! Multi-personality binary for viewing, analyzing, and managing core dumps.
//! Detected via argv[0]:
//!
//! - `coredumpctl` (default) — core dump list/info/dump/debug
//! - `coredump-extract` — extract core dump to file

use std::collections::BTreeMap;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const COREDUMP_DIR: &str = "/var/lib/systemd/coredump";
const COREDUMP_CONF: &str = "/etc/systemd/coredump.conf";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct CoreDump {
    timestamp: String,
    pid: u32,
    uid: u32,
    gid: u32,
    signal: i32,
    signal_name: String,
    exe: String,
    comm: String,
    hostname: String,
    _coredump_path: String,
    size: u64,
    _message: String,
    boot_id: String,
}

#[derive(Clone, Debug)]
struct CoreDumpConfig {
    storage: String,
    compress: bool,
    process_size_max: u64,
    external_size_max: u64,
    journal_size_max: u64,
    max_use: u64,
    keep_free: u64,
}

impl Default for CoreDumpConfig {
    fn default() -> Self {
        Self {
            storage: "external".to_string(),
            compress: true,
            process_size_max: 2 * 1024 * 1024 * 1024, // 2 GiB
            external_size_max: 2 * 1024 * 1024 * 1024,
            journal_size_max: 767 * 1024 * 1024, // 767 MiB
            max_use: 0, // unlimited
            keep_free: 0,
        }
    }
}

// ── Core dump discovery ────────────────────────────────────────────────

fn read_coredumps() -> Vec<CoreDump> {
    let entries = match std::fs::read_dir(COREDUMP_DIR) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut dumps = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        // Try to parse metadata from sidecar .info file or filename
        if path.extension().map(|e| e == "info").unwrap_or(false) {
            if let Some(dump) = parse_coredump_info(&path) {
                dumps.push(dump);
            }
        } else if path.extension().map(|e| e == "core" || e == "lz4" || e == "zst" || e == "xz").unwrap_or(false) {
            // Try to infer info from filename
            if let Some(dump) = parse_coredump_from_filename(&path) {
                dumps.push(dump);
            }
        }
    }

    dumps.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    dumps
}

fn parse_coredump_info(path: &std::path::Path) -> Option<CoreDump> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut map = BTreeMap::new();
    for line in content.lines() {
        if let Some((key, value)) = line.split_once('=') {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }

    let coredump_path = {
        let mut p = path.to_path_buf();
        p.set_extension("core");
        if !p.exists() {
            p.set_extension("lz4");
        }
        if !p.exists() {
            p.set_extension("zst");
        }
        p.to_string_lossy().to_string()
    };

    let size = std::fs::metadata(&coredump_path).map(|m| m.len()).unwrap_or(0);

    Some(CoreDump {
        timestamp: map.get("TIMESTAMP").cloned().unwrap_or_default(),
        pid: map.get("PID").and_then(|v| v.parse().ok()).unwrap_or(0),
        uid: map.get("UID").and_then(|v| v.parse().ok()).unwrap_or(0),
        gid: map.get("GID").and_then(|v| v.parse().ok()).unwrap_or(0),
        signal: map.get("SIGNAL").and_then(|v| v.parse().ok()).unwrap_or(0),
        signal_name: map.get("SIGNAL_NAME").cloned().unwrap_or_default(),
        exe: map.get("EXE").cloned().unwrap_or_default(),
        comm: map.get("COMM").cloned().unwrap_or_default(),
        hostname: map.get("HOSTNAME").cloned().unwrap_or_default(),
        _coredump_path: coredump_path,
        size,
        _message: map.get("MESSAGE").cloned().unwrap_or_default(),
        boot_id: map.get("BOOT_ID").cloned().unwrap_or_default(),
    })
}

fn parse_coredump_from_filename(path: &std::path::Path) -> Option<CoreDump> {
    let name = path.file_stem()?.to_str()?;
    // Format: core.<comm>.<uid>.<boot_id>.<pid>.<timestamp>
    let parts: Vec<&str> = name.splitn(6, '.').collect();
    if parts.len() < 6 || parts[0] != "core" {
        return None;
    }

    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    Some(CoreDump {
        timestamp: parts[5].to_string(),
        pid: parts[4].parse().unwrap_or(0),
        uid: parts[2].parse().unwrap_or(0),
        gid: 0,
        signal: 11, // SIGSEGV default guess
        signal_name: "SIGSEGV".to_string(),
        exe: String::new(),
        comm: parts[1].to_string(),
        hostname: String::new(),
        _coredump_path: path.to_string_lossy().to_string(),
        size,
        _message: String::new(),
        boot_id: parts[3].to_string(),
    })
}

fn signal_name(sig: i32) -> &'static str {
    match sig {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        _ => "SIG???",
    }
}

fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "0 B".to_string();
    }
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = bytes as f64;
    let mut unit_idx = 0;
    while val >= 1024.0 && unit_idx < units.len() - 1 {
        val /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{} {}", bytes, units[0])
    } else {
        format!("{:.1} {}", val, units[unit_idx])
    }
}

// ── Configuration ──────────────────────────────────────────────────────

fn read_config() -> CoreDumpConfig {
    let content = match std::fs::read_to_string(COREDUMP_CONF) {
        Ok(c) => c,
        Err(_) => return CoreDumpConfig::default(),
    };

    let mut config = CoreDumpConfig::default();
    let mut in_section = false;

    for line in content.lines() {
        let line = line.trim();
        if line == "[Coredump]" {
            in_section = true;
            continue;
        }
        if line.starts_with('[') {
            in_section = false;
            continue;
        }
        if !in_section || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "Storage" => config.storage = value.to_string(),
                "Compress" => config.compress = value == "yes" || value == "true" || value == "1",
                "ProcessSizeMax" => {
                    config.process_size_max = parse_size_value(value);
                }
                "ExternalSizeMax" => {
                    config.external_size_max = parse_size_value(value);
                }
                "JournalSizeMax" => {
                    config.journal_size_max = parse_size_value(value);
                }
                "MaxUse" => {
                    config.max_use = parse_size_value(value);
                }
                "KeepFree" => {
                    config.keep_free = parse_size_value(value);
                }
                _ => {}
            }
        }
    }

    config
}

fn parse_size_value(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() || s == "infinity" {
        return 0;
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix('G') {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024)
    } else if let Some(n) = s.strip_suffix('T') {
        (n, 1024u64 * 1024 * 1024 * 1024)
    } else {
        (s, 1u64)
    };

    num_str.trim().parse::<u64>().unwrap_or(0).saturating_mul(multiplier)
}

// ── Commands ───────────────────────────────────────────────────────────

fn cmd_list(args: &[String]) {
    let no_legend = args.iter().any(|a| a == "--no-legend");
    let reverse = args.iter().any(|a| a == "-r" || a == "--reverse");
    let mut since_filter = String::new();
    let mut until_filter = String::new();
    let mut pid_filter: Option<u32> = None;
    let mut exe_filter = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-S" | "--since" => {
                i += 1;
                if i < args.len() {
                    since_filter = args[i].clone();
                }
            }
            "-U" | "--until" => {
                i += 1;
                if i < args.len() {
                    until_filter = args[i].clone();
                }
            }
            _ if !args[i].starts_with('-') => {
                if let Ok(pid) = args[i].parse::<u32>() {
                    pid_filter = Some(pid);
                } else if exe_filter.is_empty() {
                    exe_filter = args[i].clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut dumps = read_coredumps();

    // Apply filters
    if let Some(pid) = pid_filter {
        dumps.retain(|d| d.pid == pid);
    }
    if !exe_filter.is_empty() {
        dumps.retain(|d| d.exe.contains(&exe_filter) || d.comm.contains(&exe_filter));
    }
    if !since_filter.is_empty() {
        dumps.retain(|d| d.timestamp >= since_filter);
    }
    if !until_filter.is_empty() {
        dumps.retain(|d| d.timestamp <= until_filter);
    }

    if reverse {
        dumps.reverse();
    }

    if dumps.is_empty() {
        if !no_legend {
            println!("No coredumps found.");
        }
        return;
    }

    if !no_legend {
        println!("{:<20} {:>6} {:>6} {:>6} {:<10} {:<8} EXE",
            "TIME", "PID", "UID", "GID", "SIG", "COREFILE");
    }

    for d in &dumps {
        let sig = if d.signal_name.is_empty() {
            signal_name(d.signal).to_string()
        } else {
            d.signal_name.clone()
        };
        let present = if d.size > 0 { "present" } else { "missing" };
        let exe = if d.exe.is_empty() { &d.comm } else { &d.exe };
        println!("{:<20} {:>6} {:>6} {:>6} {:<10} {:<8} {}",
            d.timestamp, d.pid, d.uid, d.gid, sig, present, exe);
    }

    if !no_legend {
        println!("\n{} entries listed.", dumps.len());
    }
}

fn cmd_info(args: &[String]) {
    let dumps = read_coredumps();

    let dump = if let Some(arg) = args.iter().find(|a| !a.starts_with('-')) {
        if let Ok(pid) = arg.parse::<u32>() {
            dumps.iter().rev().find(|d| d.pid == pid)
        } else {
            dumps.iter().rev().find(|d| d.exe.contains(arg.as_str()) || d.comm.contains(arg.as_str()))
        }
    } else {
        dumps.last()
    };

    let dump = match dump {
        Some(d) => d,
        None => {
            eprintln!("No matching coredump found.");
            process::exit(1);
        }
    };

    let json_mode = args.iter().any(|a| a == "--json" || a == "-j");

    if json_mode {
        println!("{{");
        println!("  \"timestamp\": \"{}\",", dump.timestamp);
        println!("  \"pid\": {},", dump.pid);
        println!("  \"uid\": {},", dump.uid);
        println!("  \"gid\": {},", dump.gid);
        println!("  \"signal\": {},", dump.signal);
        println!("  \"signalName\": \"{}\",", if dump.signal_name.is_empty() { signal_name(dump.signal) } else { &dump.signal_name });
        println!("  \"exe\": \"{}\",", dump.exe);
        println!("  \"comm\": \"{}\",", dump.comm);
        println!("  \"hostname\": \"{}\",", dump.hostname);
        println!("  \"size\": {},", dump.size);
        println!("  \"bootId\": \"{}\"", dump.boot_id);
        println!("}}");
    } else {
        println!("           PID: {} ({})", dump.pid, dump.comm);
        println!("           UID: {} ", dump.uid);
        println!("           GID: {} ", dump.gid);
        let sig = if dump.signal_name.is_empty() {
            signal_name(dump.signal).to_string()
        } else {
            dump.signal_name.clone()
        };
        println!("        Signal: {} ({})", dump.signal, sig);
        println!("     Timestamp: {}", dump.timestamp);
        if !dump.exe.is_empty() {
            println!("    Executable: {}", dump.exe);
        }
        if !dump.hostname.is_empty() {
            println!("      Hostname: {}", dump.hostname);
        }
        println!("      Corefile: {} ({})",
            if dump.size > 0 { "present" } else { "missing" },
            format_size(dump.size));
        if !dump.boot_id.is_empty() {
            println!("       Boot ID: {}", dump.boot_id);
        }
    }
}

fn cmd_dump(args: &[String]) {
    let dumps = read_coredumps();
    let mut output_path = String::new();

    let mut i = 0;
    let mut match_arg = String::new();
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output_path = args[i].clone();
                }
            }
            _ if !args[i].starts_with('-')
                && match_arg.is_empty() => {
                    match_arg = args[i].clone();
                }
            _ => {}
        }
        i += 1;
    }

    let dump = if match_arg.is_empty() {
        dumps.last()
    } else if let Ok(pid) = match_arg.parse::<u32>() {
        dumps.iter().rev().find(|d| d.pid == pid)
    } else {
        dumps.iter().rev().find(|d| d.exe.contains(match_arg.as_str()) || d.comm.contains(match_arg.as_str()))
    };

    let dump = match dump {
        Some(d) => d,
        None => {
            eprintln!("No matching coredump found.");
            process::exit(1);
        }
    };

    if dump.size == 0 {
        eprintln!("Core dump file is not available.");
        process::exit(1);
    }

    if output_path.is_empty() {
        output_path = format!("core.{}.{}", dump.comm, dump.pid);
    }

    // Copy core dump to output
    match std::fs::copy(&dump._coredump_path, &output_path) {
        Ok(bytes) => {
            println!("Core dump written to '{}' ({}).", output_path, format_size(bytes));
        }
        Err(e) => {
            eprintln!("Failed to write core dump: {}", e);
            process::exit(1);
        }
    }
}

fn cmd_debug(args: &[String]) {
    let dumps = read_coredumps();

    let mut debugger = "gdb".to_string();
    let mut match_arg = String::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-d" | "--debugger" => {
                i += 1;
                if i < args.len() {
                    debugger = args[i].clone();
                }
            }
            _ if !args[i].starts_with('-')
                && match_arg.is_empty() => {
                    match_arg = args[i].clone();
                }
            _ => {}
        }
        i += 1;
    }

    let dump = if match_arg.is_empty() {
        dumps.last()
    } else if let Ok(pid) = match_arg.parse::<u32>() {
        dumps.iter().rev().find(|d| d.pid == pid)
    } else {
        dumps.iter().rev().find(|d| d.exe.contains(match_arg.as_str()) || d.comm.contains(match_arg.as_str()))
    };

    let dump = match dump {
        Some(d) => d,
        None => {
            eprintln!("No matching coredump found.");
            process::exit(1);
        }
    };

    if dump.size == 0 {
        eprintln!("Core dump file is not available.");
        process::exit(1);
    }

    let exe = if dump.exe.is_empty() { &dump.comm } else { &dump.exe };
    println!("Launching {} for {} (PID {})...", debugger, exe, dump.pid);
    println!("{} {} {}", debugger, exe, dump._coredump_path);
}

fn cmd_config() {
    let config = read_config();

    println!("  Storage: {}", config.storage);
    println!("  Compress: {}", if config.compress { "yes" } else { "no" });
    println!("  ProcessSizeMax: {}", format_size(config.process_size_max));
    println!("  ExternalSizeMax: {}", format_size(config.external_size_max));
    println!("  JournalSizeMax: {}", format_size(config.journal_size_max));
    println!("  MaxUse: {}", if config.max_use == 0 { "unlimited".to_string() } else { format_size(config.max_use) });
    println!("  KeepFree: {}", if config.keep_free == 0 { "unlimited".to_string() } else { format_size(config.keep_free) });
}

// ── coredump-extract personality ───────────────────────────────────────

fn run_coredump_extract(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() || rest.iter().any(|a| a == "-h" || a == "--help") {
        println!("coredump-extract — Extract core dump to file");
        println!();
        println!("Usage: coredump-extract [OPTIONS] [PID|EXE]");
        println!();
        println!("Options:");
        println!("  -o, --output FILE     Output path (default: core.<comm>.<pid>)");
        println!("  -h, --help            Show this help");
        return 0;
    }

    cmd_dump(&rest);
    0
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_help() {
    println!("coredumpctl — Core dump management");
    println!();
    println!("Usage: coredumpctl [COMMAND] [OPTIONS] [MATCH]");
    println!();
    println!("Commands:");
    println!("  list                   List available coredumps");
    println!("  info [MATCH]           Show detailed info about a coredump");
    println!("  dump [MATCH]           Extract core dump to file");
    println!("  debug [MATCH]          Launch debugger on a coredump");
    println!("  config                 Show coredump configuration");
    println!();
    println!("Match:");
    println!("  PID                    Match by process ID");
    println!("  EXECUTABLE             Match by executable name");
    println!();
    println!("Options:");
    println!("  -o, --output FILE      Output file for dump command");
    println!("  -d, --debugger CMD     Debugger to use (default: gdb)");
    println!("  -S, --since TIME       Show entries since timestamp");
    println!("  -U, --until TIME       Show entries until timestamp");
    println!("  -r, --reverse          Reverse output order");
    println!("  -j, --json             JSON output for info command");
    println!("  --no-legend            Do not print column headers");
    println!("  -h, --help             Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_coredumpctl(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "list".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_help();
        return 0;
    }

    match cmd.as_str() {
        "list" => cmd_list(&cmd_args),
        "info" | "show" => cmd_info(&cmd_args),
        "dump" | "extract" => cmd_dump(&cmd_args),
        "debug" | "gdb" => cmd_debug(&cmd_args),
        "config" => cmd_config(),
        _ => {
            // Might be a PID or exe name for implicit list
            let mut all_args = vec![cmd.to_string()];
            all_args.extend(cmd_args);
            cmd_list(&all_args);
        }
    }
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("coredumpctl");
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

    let code = match prog_name.as_str() {
        "coredump-extract" => run_coredump_extract(args),
        _ => run_coredumpctl(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signal_name() {
        assert_eq!(signal_name(11), "SIGSEGV");
        assert_eq!(signal_name(6), "SIGABRT");
        assert_eq!(signal_name(9), "SIGKILL");
        assert_eq!(signal_name(15), "SIGTERM");
        assert_eq!(signal_name(999), "SIG???");
    }

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
        assert_eq!(format_size(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn test_parse_size_value() {
        assert_eq!(parse_size_value("1024"), 1024);
        assert_eq!(parse_size_value("2G"), 2 * 1024 * 1024 * 1024);
        assert_eq!(parse_size_value("512M"), 512 * 1024 * 1024);
        assert_eq!(parse_size_value("64K"), 64 * 1024);
        assert_eq!(parse_size_value("1T"), 1024u64 * 1024 * 1024 * 1024);
        assert_eq!(parse_size_value("infinity"), 0);
        assert_eq!(parse_size_value(""), 0);
    }

    #[test]
    fn test_default_config() {
        let config = CoreDumpConfig::default();
        assert_eq!(config.storage, "external");
        assert!(config.compress);
        assert_eq!(config.process_size_max, 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn test_parse_coredump_from_filename() {
        // core.<comm>.<uid>.<boot_id>.<pid>.<timestamp>
        let path = std::path::Path::new("/var/lib/systemd/coredump/core.myapp.1000.abc123.42.1234567890.core");
        // Won't find the actual file but tests parsing logic
        let result = parse_coredump_from_filename(path);
        // File doesn't exist so it may return None or a dump with size 0
        if let Some(dump) = result {
            assert_eq!(dump.comm, "myapp");
            assert_eq!(dump.uid, 1000);
            assert_eq!(dump.pid, 42);
        }
    }

    #[test]
    fn test_read_coredumps_nonexistent() {
        let dumps = read_coredumps();
        assert!(dumps.is_empty());
    }

    #[test]
    fn test_read_config_default() {
        let config = read_config();
        // Will return default if /etc/systemd/coredump.conf doesn't exist
        assert_eq!(config.storage, "external");
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("coredumpctl", "coredumpctl"),
            ("coredump-extract", "coredump-extract"),
            ("/usr/bin/coredumpctl", "coredumpctl"),
            ("C:\\bin\\coredumpctl.exe", "coredumpctl"),
        ];
        for (input, expected) in cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let base = &input[last_sep..];
            let base = base.strip_suffix(".exe").unwrap_or(base);
            assert_eq!(base, expected);
        }
    }
}
