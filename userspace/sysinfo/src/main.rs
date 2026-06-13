//! SlateOS System Information Utility
//!
//! Queries and displays comprehensive system information from /proc, /sys,
//! and other kernel interfaces. Similar to `neofetch`, `inxi`, or Windows
//! `systeminfo` — a single command for quick system overview.
//!
//! # Commands
//!
//! ```text
//! sysinfo              Full system summary
//! sysinfo cpu          CPU information
//! sysinfo memory       Memory statistics
//! sysinfo disk         Disk/filesystem usage
//! sysinfo network      Network configuration
//! sysinfo os           OS version and kernel info
//! sysinfo process      Process summary
//! sysinfo all          Everything (verbose)
//! sysinfo json         Full system info as JSON
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Proc/Sys readers
// ============================================================================

/// Read a /proc or /sys file, returning its content trimmed.
fn read_proc(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Parse key-value pairs from a /proc file (colon-separated or tab-separated).
fn parse_proc_kv(content: &str) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Try colon separator first, then tab.
        let (key, val) = if let Some((k, v)) = line.split_once(':') {
            (k.trim().to_string(), v.trim().to_string())
        } else if let Some((k, v)) = line.split_once('\t') {
            (k.trim().to_string(), v.trim().to_string())
        } else {
            continue;
        };

        pairs.push((key, val));
    }
    pairs
}

/// Get a specific value from /proc key-value content.
fn get_proc_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once(':')
            && k.trim() == key {
                return Some(v.trim().to_string());
            }
    }
    None
}

// ============================================================================
// CPU info
// ============================================================================

fn show_cpu() {
    println!("=== CPU ===");

    if let Some(content) = read_proc("/proc/cpuinfo") {
        let kv = parse_proc_kv(&content);

        let model = kv.iter()
            .find(|(k, _)| k == "model name")
            .map(|(_, v)| v.as_str())
            .unwrap_or("Unknown");

        let vendor = kv.iter()
            .find(|(k, _)| k == "vendor_id")
            .map(|(_, v)| v.as_str())
            .unwrap_or("Unknown");

        let cores = kv.iter()
            .filter(|(k, _)| k == "processor")
            .count();

        let mhz = kv.iter()
            .find(|(k, _)| k == "cpu MHz")
            .map(|(_, v)| v.as_str())
            .unwrap_or("?");

        let cache = kv.iter()
            .find(|(k, _)| k == "cache size")
            .map(|(_, v)| v.as_str())
            .unwrap_or("?");

        println!("  Model:     {model}");
        println!("  Vendor:    {vendor}");
        println!("  Cores:     {}", if cores > 0 { cores } else { 1 });
        println!("  Frequency: {mhz} MHz");
        println!("  Cache:     {cache}");
    } else {
        println!("  (cpuinfo not available)");
    }

    // Load average from /proc/loadavg.
    if let Some(loadavg) = read_proc("/proc/loadavg") {
        println!("  Load avg:  {loadavg}");
    }
}

// ============================================================================
// Memory info
// ============================================================================

fn show_memory() {
    println!("=== Memory ===");

    if let Some(content) = read_proc("/proc/meminfo") {
        let total = get_proc_value(&content, "MemTotal")
            .unwrap_or_else(|| "?".to_string());
        let free = get_proc_value(&content, "MemFree")
            .unwrap_or_else(|| "?".to_string());
        let available = get_proc_value(&content, "MemAvailable")
            .unwrap_or_else(|| "?".to_string());
        let buffers = get_proc_value(&content, "Buffers")
            .unwrap_or_else(|| "?".to_string());
        let cached = get_proc_value(&content, "Cached")
            .unwrap_or_else(|| "?".to_string());

        println!("  Total:     {total}");
        println!("  Free:      {free}");
        println!("  Available: {available}");
        println!("  Buffers:   {buffers}");
        println!("  Cached:    {cached}");

        // Parse and compute usage percentage.
        if let (Some(total_kb), Some(free_kb)) = (
            parse_kb(&total),
            parse_kb(&free),
        )
            && total_kb > 0 {
                let used_kb = total_kb.saturating_sub(free_kb);
                let pct = (used_kb as f64 / total_kb as f64) * 100.0;
                println!("  Used:      {} kB ({:.1}%)", used_kb, pct);
            }
    } else {
        println!("  (meminfo not available)");
    }

    // Swap info.
    if let Some(content) = read_proc("/proc/swaps") {
        let lines: Vec<&str> = content.lines().skip(1).collect();
        if lines.is_empty() {
            println!("  Swap:      none");
        } else {
            println!("  Swap:");
            for line in lines {
                println!("    {line}");
            }
        }
    }
}

fn parse_kb(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches(" kB").trim_end_matches(" KB");
    s.trim().parse().ok()
}

// ============================================================================
// Disk info
// ============================================================================

fn show_disk() {
    println!("=== Filesystems ===");

    if let Some(content) = read_proc("/proc/mounts") {
        println!("  {:20} {:10} {:10} Mount", "Device", "Type", "Options");
        println!("  {:20} {:10} {:10} -----", "------", "----", "-------");

        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let device = parts[0];
                let mount = parts[1];
                let fstype = parts[2];
                let options = if parts[3].len() > 20 {
                    &parts[3][..20]
                } else {
                    parts[3]
                };
                println!("  {:20} {:10} {:10} {}", device, fstype, options, mount);
            }
        }
    } else {
        println!("  (mount info not available)");
    }
}

// ============================================================================
// Network info
// ============================================================================

fn show_network() {
    println!("=== Network ===");

    // Read hostname.
    if let Some(hostname) = read_proc("/proc/sys/kernel/hostname")
        .or_else(|| read_proc("/sys/kernel/hostname"))
    {
        println!("  Hostname:  {hostname}");
    }

    // Network interface info from /proc/net or /sys.
    if let Some(content) = read_proc("/proc/net/dev") {
        println!("\n  Interfaces:");
        let mut first = true;
        for line in content.lines() {
            if first { first = false; continue; } // skip header
            let line = line.trim();
            if line.starts_with("Inter") || line.starts_with("face") {
                continue;
            }
            if let Some((iface, stats)) = line.split_once(':') {
                let parts: Vec<&str> = stats.split_whitespace().collect();
                let rx_bytes = parts.first().unwrap_or(&"0");
                let tx_bytes = parts.get(8).unwrap_or(&"0");
                println!(
                    "    {:<10} RX: {} bytes, TX: {} bytes",
                    iface.trim(),
                    rx_bytes,
                    tx_bytes
                );
            }
        }
    }

    // DNS info.
    if let Some(dns) = read_proc("/etc/resolv.conf") {
        for line in dns.lines() {
            if let Some(ns) = line.strip_prefix("nameserver") {
                println!("  DNS:       {}", ns.trim());
            }
        }
    }
}

// ============================================================================
// OS info
// ============================================================================

fn show_os() {
    println!("=== Operating System ===");

    if let Some(version) = read_proc("/proc/version") {
        println!("  Version:   {version}");
    } else {
        println!("  Version:   Slate OS (version info not available)");
    }

    if let Some(uptime_str) = read_proc("/proc/uptime") {
        let parts: Vec<&str> = uptime_str.split_whitespace().collect();
        if let Some(secs_str) = parts.first()
            && let Ok(secs) = secs_str.parse::<f64>() {
                let total_secs = secs as u64;
                let days = total_secs / 86400;
                let hours = (total_secs % 86400) / 3600;
                let mins = (total_secs % 3600) / 60;
                println!("  Uptime:    {}d {}h {}m", days, hours, mins);
            }
    }

    if let Some(cmdline) = read_proc("/proc/cmdline")
        && !cmdline.is_empty() {
            println!("  Cmdline:   {cmdline}");
        }

    // Date from /proc or system.
    println!("  Arch:      x86_64");
    println!("  Page size: 16 KiB");
}

// ============================================================================
// Process info
// ============================================================================

fn show_process() {
    println!("=== Processes ===");

    // Try to read /proc to count processes.
    let mut process_count = 0u32;

    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && name.chars().all(|c| c.is_ascii_digit()) {
                    process_count += 1;
                }
        }
        println!("  Running:   {process_count} processes");
    } else {
        println!("  (process info not available)");
    }

    // Load average.
    if let Some(loadavg) = read_proc("/proc/loadavg") {
        println!("  Load avg:  {loadavg}");
    }

    // Task stats from /proc/stat.
    if let Some(stat) = read_proc("/proc/stat") {
        for line in stat.lines() {
            if line.starts_with("procs_running")
                && let Some((_, val)) = line.split_once(' ') {
                    println!("  Running:   {}", val.trim());
                }
            if line.starts_with("procs_blocked")
                && let Some((_, val)) = line.split_once(' ') {
                    println!("  Blocked:   {}", val.trim());
                }
        }
    }
}

// ============================================================================
// JSON output
// ============================================================================

fn show_json() {
    let mut parts = Vec::new();

    // OS.
    let version = read_proc("/proc/version").unwrap_or_default();
    let uptime = read_proc("/proc/uptime").unwrap_or_default();
    parts.push(format!("\"os\":{{\"version\":\"{}\",\"uptime\":\"{}\",\"arch\":\"x86_64\",\"page_size\":16384}}",
        json_escape(&version), json_escape(&uptime)));

    // CPU.
    if let Some(content) = read_proc("/proc/cpuinfo") {
        let model = get_proc_value(&content, "model name").unwrap_or_default();
        let cores = parse_proc_kv(&content).iter()
            .filter(|(k, _)| k == "processor")
            .count();
        parts.push(format!("\"cpu\":{{\"model\":\"{}\",\"cores\":{}}}",
            json_escape(&model), if cores > 0 { cores } else { 1 }));
    }

    // Memory.
    if let Some(content) = read_proc("/proc/meminfo") {
        let total = get_proc_value(&content, "MemTotal").unwrap_or_default();
        let free = get_proc_value(&content, "MemFree").unwrap_or_default();
        parts.push(format!("\"memory\":{{\"total\":\"{}\",\"free\":\"{}\"}}",
            json_escape(&total), json_escape(&free)));
    }

    // Hostname.
    let hostname = read_proc("/proc/sys/kernel/hostname")
        .or_else(|| read_proc("/sys/kernel/hostname"))
        .unwrap_or_else(|| "localhost".to_string());
    parts.push(format!("\"hostname\":\"{}\"", json_escape(&hostname)));

    println!("{{{}}}", parts.join(","));
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out
}

// ============================================================================
// Full summary
// ============================================================================

fn show_summary() {
    show_os();
    println!();
    show_cpu();
    println!();
    show_memory();
    println!();
    show_disk();
    println!();
    show_network();
    println!();
    show_process();
}

fn show_all() {
    show_summary();
    // In the future, add hardware details, PCI devices, etc.
}

// ============================================================================
// Usage and main
// ============================================================================

fn print_usage() {
    println!("Slate OS System Information v0.1.0");
    println!();
    println!("Query and display comprehensive system information.");
    println!();
    println!("USAGE:");
    println!("  sysinfo [command]");
    println!();
    println!("COMMANDS:");
    println!("  (no args)    Full system summary");
    println!("  cpu          CPU information");
    println!("  memory       Memory statistics");
    println!("  disk         Disk and filesystem usage");
    println!("  network      Network configuration");
    println!("  os           OS and kernel version");
    println!("  process      Process summary");
    println!("  all          Everything (verbose)");
    println!("  json         Full info as JSON");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        show_summary();
        process::exit(0);
    }

    match args[1].as_str() {
        "cpu" => show_cpu(),
        "memory" | "mem" | "ram" => show_memory(),
        "disk" | "fs" | "filesystems" => show_disk(),
        "network" | "net" => show_network(),
        "os" | "version" => show_os(),
        "process" | "proc" | "ps" => show_process(),
        "all" | "full" => show_all(),
        "json" => show_json(),
        "help" | "--help" | "-h" => print_usage(),
        other => {
            eprintln!("unknown command: {other}");
            eprintln!("Run 'sysinfo help' for usage.");
            process::exit(1);
        }
    }
}
