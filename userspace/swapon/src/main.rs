//! OurOS swap and memory management utilities.
//!
//! Multi-personality binary providing:
//! - **swapon** — enable swap space
//! - **swapoff** — disable swap space
//! - **free** — display memory and swap usage
//!
//! Reads `/proc/meminfo`, `/proc/swaps`, and `/etc/fstab` for system memory
//! and swap configuration.

#![deny(clippy::all)]
// MemInfo::sreclaimable and FstabEntry::{mountpoint, options} mirror the
// /proc/meminfo and /etc/fstab field vocabulary the real swapon must
// consume. Dead-code lint cannot see across that future boundary.
#![allow(dead_code)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

// ============================================================================
// Constants
// ============================================================================

const VERSION: &str = "0.1.0";
const PROC_SWAPS: &str = "/proc/swaps";
const PROC_MEMINFO: &str = "/proc/meminfo";
const FSTAB_PATH: &str = "/etc/fstab";

// ============================================================================
// Data structures
// ============================================================================

/// A swap entry from /proc/swaps.
#[derive(Clone, Debug)]
struct SwapEntry {
    filename: String,
    swap_type: String,
    size_kb: u64,
    used_kb: u64,
    priority: i32,
}

/// Memory information from /proc/meminfo.
struct MemInfo {
    mem_total: u64,
    mem_free: u64,
    mem_available: u64,
    buffers: u64,
    cached: u64,
    swap_total: u64,
    swap_free: u64,
    shmem: u64,
    sreclaimable: u64,
}

/// Fstab entry.
struct FstabEntry {
    device: String,
    mountpoint: String,
    fstype: String,
    options: String,
    _dump: u32,
    _pass: u32,
}

// ============================================================================
// Parsing
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn parse_proc_swaps() -> Vec<SwapEntry> {
    let content = match read_file(PROC_SWAPS) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 5 {
            entries.push(SwapEntry {
                filename: fields[0].to_string(),
                swap_type: fields[1].to_string(),
                size_kb: fields[2].parse().unwrap_or(0),
                used_kb: fields[3].parse().unwrap_or(0),
                priority: fields[4].parse().unwrap_or(-1),
            });
        }
    }
    entries
}

fn parse_meminfo() -> MemInfo {
    let content = match read_file(PROC_MEMINFO) {
        Some(c) => c,
        None => {
            return MemInfo {
                mem_total: 0, mem_free: 0, mem_available: 0,
                buffers: 0, cached: 0, swap_total: 0, swap_free: 0,
                shmem: 0, sreclaimable: 0,
            };
        }
    };

    let mut values: HashMap<String, u64> = HashMap::new();
    for line in content.lines() {
        if let Some(colon_pos) = line.find(':') {
            let key = line[..colon_pos].trim().to_string();
            let val_str = line[colon_pos + 1..].trim();
            // Remove "kB" suffix if present.
            let val_str = val_str.strip_suffix(" kB").unwrap_or(val_str);
            let val_str = val_str.strip_suffix("kB").unwrap_or(val_str);
            if let Ok(val) = val_str.trim().parse::<u64>() {
                values.insert(key, val);
            }
        }
    }

    MemInfo {
        mem_total: values.get("MemTotal").copied().unwrap_or(0),
        mem_free: values.get("MemFree").copied().unwrap_or(0),
        mem_available: values.get("MemAvailable").copied().unwrap_or(0),
        buffers: values.get("Buffers").copied().unwrap_or(0),
        cached: values.get("Cached").copied().unwrap_or(0),
        swap_total: values.get("SwapTotal").copied().unwrap_or(0),
        swap_free: values.get("SwapFree").copied().unwrap_or(0),
        shmem: values.get("Shmem").copied().unwrap_or(0),
        sreclaimable: values.get("SReclaimable").copied().unwrap_or(0),
    }
}

fn parse_fstab() -> Vec<FstabEntry> {
    let content = match read_file(FSTAB_PATH) {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() >= 4 {
            entries.push(FstabEntry {
                device: fields[0].to_string(),
                mountpoint: fields[1].to_string(),
                fstype: fields[2].to_string(),
                options: fields[3].to_string(),
                _dump: fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                _pass: fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
            });
        }
    }
    entries
}

fn get_swap_fstab_entries() -> Vec<FstabEntry> {
    parse_fstab()
        .into_iter()
        .filter(|e| e.fstype == "swap")
        .collect()
}

fn is_swap_active(device: &str) -> bool {
    let active = parse_proc_swaps();
    active.iter().any(|s| s.filename == device)
}

// ============================================================================
// Size formatting
// ============================================================================

/// Format a value in KiB to human-readable.
fn format_size_human(kb: u64) -> String {
    if kb >= 1_073_741_824 {
        // TiB
        let tib = kb as f64 / 1_073_741_824.0;
        format!("{tib:.1}Ti")
    } else if kb >= 1_048_576 {
        // GiB
        let gib = kb as f64 / 1_048_576.0;
        format!("{gib:.1}Gi")
    } else if kb >= 1024 {
        // MiB
        let mib = kb as f64 / 1024.0;
        format!("{mib:.1}Mi")
    } else {
        format!("{kb}K")
    }
}

/// Format a value in KiB to bytes, MiB, or GiB depending on unit.
fn format_size_unit(kb: u64, unit: &str) -> String {
    match unit {
        "bytes" | "b" => format!("{}", kb * 1024),
        "kilo" | "k" => format!("{kb}"),
        "mega" | "m" => format!("{}", kb / 1024),
        "giga" | "g" => format!("{}", kb / 1_048_576),
        "tera" | "t" => format!("{}", kb / 1_073_741_824),
        "human" | "h" => format_size_human(kb),
        _ => format!("{kb}"),
    }
}

// ============================================================================
// Personality: swapon
// ============================================================================

fn cmd_swapon(args: &[String]) {
    let mut show_summary = false;
    let mut all_flag = false;
    let mut priority: Option<i32> = None;
    let mut discard = false;
    let mut verbose = false;
    let mut devices: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: swapon [options] [device...]");
                println!();
                println!("Enable devices and files for paging and swapping.");
                println!();
                println!("Options:");
                println!("  -a, --all              Enable all swaps from /etc/fstab");
                println!("  -d, --discard          Enable discard/TRIM on swap");
                println!("  -p, --priority <prio>  Set swap priority (-1 to 32767)");
                println!("  -s, --summary          Display swap usage summary");
                println!("  --show                 Display swap entries (default cols)");
                println!("  -v, --verbose          Verbose output");
                println!("  -h, --help             Show this help");
                println!("  --version              Show version");
                process::exit(0);
            }
            "--version" => {
                println!("swapon {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => all_flag = true,
            "-d" | "--discard" => discard = true,
            "-s" | "--summary" | "--show" => show_summary = true,
            "-v" | "--verbose" => verbose = true,
            "-p" | "--priority" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("swapon: -p requires an argument");
                    process::exit(1);
                }
                priority = Some(args[i].parse().unwrap_or_else(|_| {
                    eprintln!("swapon: invalid priority: {}", args[i]);
                    process::exit(1);
                }));
            }
            s if !s.starts_with('-') => {
                devices.push(s.to_string());
            }
            other => {
                eprintln!("swapon: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    // Default action: show summary if no args.
    if args.is_empty() || show_summary {
        show_swap_summary();
        if args.is_empty() || (show_summary && devices.is_empty() && !all_flag) {
            return;
        }
    }

    if all_flag {
        // Enable all swap entries from fstab.
        let fstab_swaps = get_swap_fstab_entries();
        if fstab_swaps.is_empty() {
            eprintln!("swapon: no swap entries found in {FSTAB_PATH}");
            return;
        }
        for entry in &fstab_swaps {
            if is_swap_active(&entry.device) {
                if verbose {
                    println!("swapon: {}: already active", entry.device);
                }
                continue;
            }
            activate_swap(&entry.device, priority, discard, verbose);
        }
    } else if devices.is_empty() {
        // No devices and no -a: already showed summary above.
    } else {
        for device in &devices {
            if is_swap_active(device) {
                eprintln!("swapon: {device}: already active");
                continue;
            }
            activate_swap(device, priority, discard, verbose);
        }
    }
}

fn show_swap_summary() {
    let entries = parse_proc_swaps();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let _ = writeln!(out, "{:<40} {:>6} {:>10} {:>10} {:>5}",
        "Filename", "Type", "Size", "Used", "Priority");

    for e in &entries {
        let _ = writeln!(out, "{:<40} {:>6} {:>10} {:>10} {:>5}",
            e.filename, e.swap_type, e.size_kb, e.used_kb, e.priority);
    }
}

fn activate_swap(device: &str, priority: Option<i32>, discard: bool, verbose: bool) {
    // In a real kernel, this would invoke swapon(2) syscall.
    // We simulate by writing to a hypothetical /proc/sys/swap/activate.
    let mut cmd = format!("activate {device}");
    if let Some(p) = priority {
        cmd.push_str(&format!(" priority={p}"));
    }
    if discard {
        cmd.push_str(" discard=1");
    }

    match fs::write("/proc/sys/swap/activate", &cmd) {
        Ok(()) => {
            if verbose {
                println!("swapon: {device}: activated{}",
                    priority.map(|p| format!(" (priority {p})")).unwrap_or_default());
            }
        }
        Err(e) => {
            eprintln!("swapon: {device}: failed to activate: {e}");
        }
    }
}

// ============================================================================
// Personality: swapoff
// ============================================================================

fn cmd_swapoff(args: &[String]) {
    let mut all_flag = false;
    let mut verbose = false;
    let mut devices: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: swapoff [options] [device...]");
                println!();
                println!("Disable devices and files for paging and swapping.");
                println!();
                println!("Options:");
                println!("  -a, --all      Disable all swaps");
                println!("  -v, --verbose  Verbose output");
                println!("  -h, --help     Show this help");
                println!("  --version      Show version");
                process::exit(0);
            }
            "--version" => {
                println!("swapoff {VERSION}");
                process::exit(0);
            }
            "-a" | "--all" => all_flag = true,
            "-v" | "--verbose" => verbose = true,
            s if !s.starts_with('-') => {
                devices.push(s.to_string());
            }
            other => {
                eprintln!("swapoff: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    if all_flag {
        let active = parse_proc_swaps();
        if active.is_empty() {
            if verbose {
                println!("swapoff: no swap entries active");
            }
            return;
        }
        for entry in &active {
            deactivate_swap(&entry.filename, verbose);
        }
    } else if devices.is_empty() {
        eprintln!("swapoff: no device specified");
        eprintln!("Try 'swapoff --help' for more information.");
        process::exit(1);
    } else {
        for device in &devices {
            if !is_swap_active(device) {
                eprintln!("swapoff: {device}: not currently active");
                continue;
            }
            deactivate_swap(device, verbose);
        }
    }
}

fn deactivate_swap(device: &str, verbose: bool) {
    let cmd = format!("deactivate {device}");
    match fs::write("/proc/sys/swap/deactivate", &cmd) {
        Ok(()) => {
            if verbose {
                println!("swapoff: {device}: deactivated");
            }
        }
        Err(e) => {
            eprintln!("swapoff: {device}: failed to deactivate: {e}");
        }
    }
}

// ============================================================================
// Personality: free
// ============================================================================

fn cmd_free(args: &[String]) {
    let mut unit = "kilo".to_string();
    let mut wide = false;
    let mut total_line = false;
    let mut lohi = false;
    let mut count: Option<u32> = None;
    let mut seconds: Option<u64> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--human" => unit = "human".to_string(),
            "-b" | "--bytes" => unit = "bytes".to_string(),
            "-k" | "--kilo" | "--kibi" => unit = "kilo".to_string(),
            "-m" | "--mega" | "--mebi" => unit = "mega".to_string(),
            "-g" | "--giga" | "--gibi" => unit = "giga".to_string(),
            "--tera" | "--tebi" => unit = "tera".to_string(),
            "-w" | "--wide" => wide = true,
            "-t" | "--total" => total_line = true,
            "-l" | "--lohi" => lohi = true,
            "-c" | "--count" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("free: -c requires an argument");
                    process::exit(1);
                }
                count = Some(args[i].parse().unwrap_or_else(|_| {
                    eprintln!("free: invalid count: {}", args[i]);
                    process::exit(1);
                }));
            }
            "-s" | "--seconds" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("free: -s requires an argument");
                    process::exit(1);
                }
                seconds = Some(args[i].parse().unwrap_or_else(|_| {
                    eprintln!("free: invalid seconds: {}", args[i]);
                    process::exit(1);
                }));
            }
            "--help" => {
                println!("Usage: free [options]");
                println!();
                println!("Display amount of free and used memory in the system.");
                println!();
                println!("Options:");
                println!("  -b, --bytes     Show output in bytes");
                println!("  -k, --kilo      Show output in kibibytes (default)");
                println!("  -m, --mega      Show output in mebibytes");
                println!("  -g, --giga      Show output in gibibytes");
                println!("  --tera          Show output in tebibytes");
                println!("  -h, --human     Show human-readable output");
                println!("  -w, --wide      Wide output (separate buffers/cache)");
                println!("  -t, --total     Show total line");
                println!("  -l, --lohi      Show low/high memory statistics");
                println!("  -s N, --seconds N  Repeat every N seconds");
                println!("  -c N, --count N    Repeat N times (with -s)");
                println!("  --help          Show this help");
                println!("  --version       Show version");
                process::exit(0);
            }
            "--version" => {
                println!("free {VERSION}");
                process::exit(0);
            }
            other => {
                eprintln!("free: unknown option: {other}");
                process::exit(1);
            }
        }
        i += 1;
    }

    let iterations = count.unwrap_or(if seconds.is_some() { u32::MAX } else { 1 });

    for iter in 0..iterations {
        if iter > 0 {
            if let Some(s) = seconds {
                std::thread::sleep(std::time::Duration::from_secs(s));
            }
            println!(); // Blank line between iterations.
        }
        display_free(&unit, wide, total_line, lohi);
    }
}

fn display_free(unit: &str, wide: bool, total_line: bool, lohi: bool) {
    let info = parse_meminfo();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let used = info.mem_total.saturating_sub(info.mem_free)
        .saturating_sub(info.buffers)
        .saturating_sub(info.cached);
    let buff_cache = info.buffers + info.cached;
    let swap_used = info.swap_total.saturating_sub(info.swap_free);

    let fmt = |v: u64| format_size_unit(v, unit);

    if wide {
        let _ = writeln!(out, "{:>16} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
            "", "total", "used", "free", "shared", "buffers", "cache");
        let _ = writeln!(out, "{:<16} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
            "Mem:", fmt(info.mem_total), fmt(used),
            fmt(info.mem_free), fmt(info.shmem),
            fmt(info.buffers), fmt(info.cached));
    } else {
        let _ = writeln!(out, "{:>16} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
            "", "total", "used", "free", "shared", "buff/cache", "available");
        let _ = writeln!(out, "{:<16} {:>12} {:>12} {:>12} {:>12} {:>12} {:>12}",
            "Mem:", fmt(info.mem_total), fmt(used),
            fmt(info.mem_free), fmt(info.shmem),
            fmt(buff_cache), fmt(info.mem_available));
    }

    if lohi {
        // Low memory = total, High memory = 0 for flat memory model.
        let _ = writeln!(out, "{:<16} {:>12} {:>12} {:>12}",
            "Low:", fmt(info.mem_total), fmt(used), fmt(info.mem_free));
        let _ = writeln!(out, "{:<16} {:>12} {:>12} {:>12}",
            "High:", fmt(0), fmt(0), fmt(0));
    }

    let _ = writeln!(out, "{:<16} {:>12} {:>12} {:>12}",
        "Swap:", fmt(info.swap_total), fmt(swap_used), fmt(info.swap_free));

    if total_line {
        let total_total = info.mem_total + info.swap_total;
        let total_used = used + swap_used;
        let total_free = info.mem_free + info.swap_free;
        let _ = writeln!(out, "{:<16} {:>12} {:>12} {:>12}",
            "Total:", fmt(total_total), fmt(total_used), fmt(total_free));
    }
}

// ============================================================================
// Entry point
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("swapon");
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

    match prog_name.as_str() {
        "swapoff" => cmd_swapoff(&rest),
        "free" => cmd_free(&rest),
        _ => cmd_swapon(&rest),
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_human() {
        assert_eq!(format_size_human(512), "512K");
        assert_eq!(format_size_human(1024), "1.0Mi");
        assert_eq!(format_size_human(1536), "1.5Mi");
        assert_eq!(format_size_human(1048576), "1.0Gi");
        assert_eq!(format_size_human(2097152), "2.0Gi");
        assert_eq!(format_size_human(1073741824), "1.0Ti");
    }

    #[test]
    fn test_format_size_unit_bytes() {
        assert_eq!(format_size_unit(1, "bytes"), "1024");
        assert_eq!(format_size_unit(1024, "bytes"), "1048576");
    }

    #[test]
    fn test_format_size_unit_kilo() {
        assert_eq!(format_size_unit(1024, "kilo"), "1024");
        assert_eq!(format_size_unit(0, "kilo"), "0");
    }

    #[test]
    fn test_format_size_unit_mega() {
        assert_eq!(format_size_unit(1024, "mega"), "1");
        assert_eq!(format_size_unit(2048, "mega"), "2");
        assert_eq!(format_size_unit(512, "mega"), "0"); // truncated
    }

    #[test]
    fn test_format_size_unit_giga() {
        assert_eq!(format_size_unit(1048576, "giga"), "1");
        assert_eq!(format_size_unit(0, "giga"), "0");
    }

    #[test]
    fn test_format_size_unit_tera() {
        assert_eq!(format_size_unit(1073741824, "tera"), "1");
    }

    #[test]
    fn test_format_size_unit_human() {
        assert_eq!(format_size_unit(1024, "human"), "1.0Mi");
        assert_eq!(format_size_unit(1048576, "human"), "1.0Gi");
    }

    #[test]
    fn test_parse_meminfo_values() {
        // parse_meminfo reads from /proc/meminfo which may not exist in test env.
        // Just verify the function doesn't panic.
        let info = parse_meminfo();
        // Values should be >= 0 (they're u64).
        let _ = info.mem_total;
        let _ = info.mem_free;
    }

    #[test]
    fn test_parse_proc_swaps() {
        // parse_proc_swaps reads from /proc/swaps which may not exist.
        // Just verify it doesn't panic and returns a vec.
        let entries = parse_proc_swaps();
        // entries may be empty or populated.
        let _ = entries.len();
    }

    #[test]
    fn test_parse_fstab() {
        let entries = parse_fstab();
        // Just verify no panic.
        let _ = entries.len();
    }

    #[test]
    fn test_get_swap_fstab_entries() {
        let entries = get_swap_fstab_entries();
        for e in &entries {
            assert_eq!(e.fstype, "swap");
        }
    }

    #[test]
    fn test_swap_entry_fields() {
        let entry = SwapEntry {
            filename: "/dev/sda2".to_string(),
            swap_type: "partition".to_string(),
            size_kb: 8388604,
            used_kb: 1024,
            priority: -2,
        };
        assert_eq!(entry.filename, "/dev/sda2");
        assert_eq!(entry.swap_type, "partition");
        assert_eq!(entry.size_kb, 8388604);
        assert_eq!(entry.used_kb, 1024);
        assert_eq!(entry.priority, -2);
    }

    #[test]
    fn test_mem_info_defaults() {
        let info = MemInfo {
            mem_total: 16777216,
            mem_free: 8388608,
            mem_available: 12582912,
            buffers: 524288,
            cached: 3145728,
            swap_total: 8388608,
            swap_free: 8388608,
            shmem: 262144,
            sreclaimable: 131072,
        };

        let used = info.mem_total - info.mem_free - info.buffers - info.cached;
        assert_eq!(used, 4718592); // 16M - 8M - 512K - 3M
        assert_eq!(info.swap_total - info.swap_free, 0);
    }

    #[test]
    fn test_fstab_entry() {
        let entry = FstabEntry {
            device: "/dev/sda2".to_string(),
            mountpoint: "none".to_string(),
            fstype: "swap".to_string(),
            options: "sw".to_string(),
            _dump: 0,
            _pass: 0,
        };
        assert_eq!(entry.fstype, "swap");
        assert_eq!(entry.mountpoint, "none");
    }

    #[test]
    fn test_format_size_boundary() {
        // Exactly at boundary values.
        assert_eq!(format_size_human(0), "0K");
        assert_eq!(format_size_human(1), "1K");
        assert_eq!(format_size_human(1023), "1023K");
    }

    #[test]
    fn test_format_size_large_values() {
        // Very large swap (e.g., 64 TiB).
        let huge = 64u64 * 1073741824;
        assert!(format_size_human(huge).contains("Ti"));
    }

    #[test]
    fn test_is_swap_active() {
        // Test with a non-existent device.
        assert!(!is_swap_active("/dev/nonexistent"));
    }

    #[test]
    fn test_personality_detection() {
        // Test basename extraction logic.
        let test_cases = [
            ("/usr/sbin/swapon", "swapon"),
            ("/usr/sbin/swapoff", "swapoff"),
            ("free", "free"),
            ("C:\\Windows\\swapon.exe", "swapon"),
            ("/bin/free.exe", "free"),
        ];

        for (input, expected) in &test_cases {
            let bytes = input.as_bytes();
            let mut last_sep = 0;
            for (i, &b) in bytes.iter().enumerate() {
                if b == b'/' || b == b'\\' {
                    last_sep = i + 1;
                }
            }
            let basename = &input[last_sep..];
            let basename = basename.strip_suffix(".exe").unwrap_or(basename);
            assert_eq!(basename, *expected, "Failed for input: {input}");
        }
    }
}
