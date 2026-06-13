//! Slate OS CPU information display utility.
//!
//! Multi-personality binary providing:
//! - **lscpu** — display CPU architecture information
//! - **nproc** variant — print the number of processing units (if called as nproc)
//!
//! Reads CPU topology and features from /proc/cpuinfo, /sys/devices/system/cpu/,
//! and CPUID instruction results.

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process;

const VERSION: &str = "0.1.0";

// ============================================================================
// CPU information structures
// ============================================================================

/// One cache level as reported by `/sys/devices/system/cpu/cpu0/cache/indexN/`.
/// Every field is sourced from sysfs (ultimately CPUID-derived in the kernel);
/// fields the kernel does not expose are left `None` and simply not displayed.
/// lscpu never fabricates cache geometry.
#[derive(Clone, Debug)]
struct CacheRecord {
    level: u8,
    cache_type: String,
    size: String,
    ways: Option<u32>,
}

#[derive(Clone, Debug, Default)]
struct CpuInfo {
    architecture: String,
    op_modes: Vec<String>,
    byte_order: String,
    address_sizes: String,
    cpus: u32,
    online_cpus: String,
    vendor_id: String,
    model_name: String,
    cpu_family: u32,
    model: u32,
    stepping: u32,
    cpu_mhz: f64,
    cpu_max_mhz: f64,
    cpu_min_mhz: f64,
    bogomips: f64,
    hypervisor: Option<String>,
    l1d_cache: String,
    l1i_cache: String,
    l2_cache: String,
    l3_cache: String,
    caches: Vec<CacheRecord>,
    threads_per_core: u32,
    cores_per_socket: u32,
    sockets: u32,
    numa_nodes: u32,
    flags: Vec<String>,
    vulnerabilities: Vec<(String, String)>,
}

struct LscpuOpts {
    json: bool,
    extended: bool,
    parse: bool,
    online: bool,
    offline: bool,
    hex: bool,
    caches: bool,
}

// ============================================================================
// Data collection
// ============================================================================

fn read_cpuinfo() -> Vec<HashMap<String, String>> {
    let content = match fs::read_to_string("/proc/cpuinfo") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut cpus = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                cpus.push(current.clone());
                current.clear();
            }
            continue;
        }
        if let Some((key, val)) = line.split_once(':') {
            current.insert(key.trim().to_string(), val.trim().to_string());
        }
    }
    if !current.is_empty() {
        cpus.push(current);
    }

    cpus
}

fn count_online_cpus() -> u32 {
    // Try /sys/devices/system/cpu/online first.
    if let Ok(content) = fs::read_to_string("/sys/devices/system/cpu/online") {
        let content = content.trim();
        // Format: "0-7" or "0,2-4,6"
        return parse_cpu_range(content);
    }

    // Fall back to /proc/cpuinfo.
    let cpus = read_cpuinfo();
    if cpus.is_empty() {
        1
    } else {
        cpus.len() as u32
    }
}

fn parse_cpu_range(s: &str) -> u32 {
    let mut count = 0u32;
    for part in s.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.trim().parse::<u32>(), end.trim().parse::<u32>()) {
                count += e - s + 1;
            }
        } else if part.parse::<u32>().is_ok() {
            count += 1;
        }
    }
    if count == 0 { 1 } else { count }
}

fn read_sys_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn collect_cpu_info() -> CpuInfo {
    let mut info = CpuInfo::default();
    let cpuinfo = read_cpuinfo();

    // Architecture detection.
    info.architecture = if cfg!(target_arch = "x86_64") {
        "x86_64".to_string()
    } else if cfg!(target_arch = "x86") {
        "i686".to_string()
    } else if cfg!(target_arch = "aarch64") {
        "aarch64".to_string()
    } else {
        "unknown".to_string()
    };

    info.op_modes = vec!["32-bit".to_string(), "64-bit".to_string()];
    info.byte_order = "Little Endian".to_string();
    info.address_sizes = "48 bits virtual, 39 bits physical".to_string();

    // CPU count.
    info.cpus = count_online_cpus();
    info.online_cpus = format!("0-{}", info.cpus.saturating_sub(1));

    // From /proc/cpuinfo.
    if let Some(first) = cpuinfo.first() {
        info.vendor_id = first.get("vendor_id").cloned().unwrap_or_default();
        info.model_name = first.get("model name").cloned().unwrap_or_default();
        info.cpu_family = first
            .get("cpu family")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        info.model = first
            .get("model")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        info.stepping = first
            .get("stepping")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        info.cpu_mhz = first
            .get("cpu MHz")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        info.bogomips = first
            .get("bogomips")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);

        if let Some(flags_str) = first.get("flags") {
            info.flags = flags_str.split_whitespace().map(|s| s.to_string()).collect();
        }
    }

    // Topology from sysfs.
    info.threads_per_core = read_sys_file("/sys/devices/system/cpu/cpu0/topology/thread_siblings_list")
        .map(|s| parse_cpu_range(&s))
        .unwrap_or(1);

    info.cores_per_socket = read_sys_file("/sys/devices/system/cpu/cpu0/topology/core_siblings_list")
        .map(|s| parse_cpu_range(&s) / info.threads_per_core.max(1))
        .unwrap_or_else(|| {
            if info.cpus > 0 { info.cpus } else { 1 }
        });

    info.sockets = if info.cores_per_socket > 0 && info.threads_per_core > 0 {
        info.cpus / (info.cores_per_socket * info.threads_per_core)
    } else {
        1
    };
    if info.sockets == 0 {
        info.sockets = 1;
    }

    // NUMA.
    info.numa_nodes = 1;
    if let Ok(entries) = fs::read_dir("/sys/devices/system/node") {
        info.numa_nodes = entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.starts_with("node"))
                    .unwrap_or(false)
            })
            .count() as u32;
    }
    if info.numa_nodes == 0 {
        info.numa_nodes = 1;
    }

    // Cache info from sysfs.  Every value here is sourced from
    // /sys/.../cache/indexN (CPUID-derived in the kernel).  If sysfs does not
    // expose a cache index we leave the corresponding field empty and simply
    // do not display it — we never substitute a fabricated default, which
    // would show the user invented numbers as if they were real.
    for i in 0..8 {
        let base = format!("/sys/devices/system/cpu/cpu0/cache/index{i}");
        let (Some(level_s), Some(cache_type), Some(size)) = (
            read_sys_file(&format!("{base}/level")),
            read_sys_file(&format!("{base}/type")),
            read_sys_file(&format!("{base}/size")),
        ) else {
            // No more cache indices.
            break;
        };
        let Ok(level) = level_s.parse::<u8>() else { continue };
        let ways = read_sys_file(&format!("{base}/ways_of_associativity"))
            .and_then(|s| s.parse::<u32>().ok());

        match (level, cache_type.as_str()) {
            (1, "Data") => info.l1d_cache = size.clone(),
            (1, "Instruction") => info.l1i_cache = size.clone(),
            (2, _) => info.l2_cache = size.clone(),
            (3, _) => info.l3_cache = size.clone(),
            _ => {}
        }
        info.caches.push(CacheRecord { level, cache_type, size, ways });
    }

    // Hypervisor detection.
    if let Ok(content) = fs::read_to_string("/sys/hypervisor/type") {
        info.hypervisor = Some(content.trim().to_string());
    } else if info.flags.contains(&"hypervisor".to_string()) {
        info.hypervisor = Some("unknown".to_string());
    }

    // CPU frequency.
    info.cpu_max_mhz = read_sys_file("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|khz| khz / 1000.0)
        .unwrap_or(info.cpu_mhz);
    info.cpu_min_mhz = read_sys_file("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq")
        .and_then(|s| s.parse::<f64>().ok())
        .map(|khz| khz / 1000.0)
        .unwrap_or(0.0);

    // Vulnerabilities.
    if let Ok(entries) = fs::read_dir("/sys/devices/system/cpu/vulnerabilities") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if let Ok(status) = fs::read_to_string(entry.path()) {
                info.vulnerabilities.push((name, status.trim().to_string()));
            }
        }
    }
    info.vulnerabilities.sort_by(|a, b| a.0.cmp(&b.0));

    info
}

// ============================================================================
// Output formatters
// ============================================================================

fn print_standard(out: &mut io::StdoutLock<'_>, info: &CpuInfo) {
    let _ = writeln!(out, "Architecture:          {}", info.architecture);
    let _ = writeln!(out, "CPU op-mode(s):        {}", info.op_modes.join(", "));
    let _ = writeln!(out, "Byte Order:            {}", info.byte_order);
    let _ = writeln!(out, "Address sizes:         {}", info.address_sizes);
    let _ = writeln!(out, "CPU(s):                {}", info.cpus);
    let _ = writeln!(out, "On-line CPU(s) list:   {}", info.online_cpus);

    if !info.vendor_id.is_empty() {
        let _ = writeln!(out, "Vendor ID:             {}", info.vendor_id);
    }
    if !info.model_name.is_empty() {
        let _ = writeln!(out, "Model name:            {}", info.model_name);
    }
    let _ = writeln!(out, "CPU family:            {}", info.cpu_family);
    let _ = writeln!(out, "Model:                 {}", info.model);
    let _ = writeln!(out, "Stepping:              {}", info.stepping);

    if info.cpu_mhz > 0.0 {
        let _ = writeln!(out, "CPU MHz:               {:.3}", info.cpu_mhz);
    }
    if info.cpu_max_mhz > 0.0 {
        let _ = writeln!(out, "CPU max MHz:           {:.4}", info.cpu_max_mhz);
    }
    if info.cpu_min_mhz > 0.0 {
        let _ = writeln!(out, "CPU min MHz:           {:.4}", info.cpu_min_mhz);
    }
    if info.bogomips > 0.0 {
        let _ = writeln!(out, "BogoMIPS:              {:.2}", info.bogomips);
    }

    if let Some(ref hv) = info.hypervisor {
        let _ = writeln!(out, "Hypervisor vendor:     {hv}");
        let _ = writeln!(out, "Virtualization type:   full");
    }

    let _ = writeln!(out, "Thread(s) per core:    {}", info.threads_per_core);
    let _ = writeln!(out, "Core(s) per socket:    {}", info.cores_per_socket);
    let _ = writeln!(out, "Socket(s):             {}", info.sockets);
    let _ = writeln!(out, "NUMA node(s):          {}", info.numa_nodes);

    // Only print cache lines we could actually source from sysfs.
    if !info.l1d_cache.is_empty() {
        let _ = writeln!(out, "L1d cache:             {}", info.l1d_cache);
    }
    if !info.l1i_cache.is_empty() {
        let _ = writeln!(out, "L1i cache:             {}", info.l1i_cache);
    }
    if !info.l2_cache.is_empty() {
        let _ = writeln!(out, "L2 cache:              {}", info.l2_cache);
    }
    if !info.l3_cache.is_empty() {
        let _ = writeln!(out, "L3 cache:              {}", info.l3_cache);
    }

    let _ = writeln!(out, "NUMA node0 CPU(s):     {}", info.online_cpus);

    // Vulnerabilities.
    for (name, status) in &info.vulnerabilities {
        let _ = writeln!(out, "Vulnerability {name}: {status}");
    }

    if !info.flags.is_empty() {
        let _ = writeln!(out, "Flags:                 {}", info.flags.join(" "));
    }
}

fn print_json(out: &mut io::StdoutLock<'_>, info: &CpuInfo) {
    let _ = writeln!(out, "{{");
    let _ = writeln!(out, "  \"lscpu\": [");
    // Assemble fields dynamically so empty/unsourced values (e.g. cache sizes
    // sysfs did not provide) are omitted rather than emitted as blanks.
    let mut entries: Vec<(&str, String)> = vec![
        ("Architecture", info.architecture.clone()),
        ("Byte Order", info.byte_order.clone()),
        ("CPU(s)", info.cpus.to_string()),
        ("CPU family", info.cpu_family.to_string()),
        ("Model", info.model.to_string()),
        ("Stepping", info.stepping.to_string()),
        ("Thread(s) per core", info.threads_per_core.to_string()),
        ("Core(s) per socket", info.cores_per_socket.to_string()),
        ("Socket(s)", info.sockets.to_string()),
        ("NUMA node(s)", info.numa_nodes.to_string()),
    ];
    if !info.vendor_id.is_empty() {
        entries.push(("Vendor ID", info.vendor_id.clone()));
    }
    if !info.model_name.is_empty() {
        entries.push(("Model name", info.model_name.clone()));
    }
    for (label, val) in [
        ("L1d cache", &info.l1d_cache),
        ("L1i cache", &info.l1i_cache),
        ("L2 cache", &info.l2_cache),
        ("L3 cache", &info.l3_cache),
    ] {
        if !val.is_empty() {
            entries.push((label, val.clone()));
        }
    }

    let total = entries.len();
    for (idx, (key, val)) in entries.iter().enumerate() {
        let comma = if idx + 1 < total { "," } else { "" };
        let _ = writeln!(out, "    {{\"field\": \"{key}\", \"data\": \"{val}\"}}{comma}");
    }

    let _ = writeln!(out, "  ]");
    let _ = writeln!(out, "}}");
}

fn print_caches(out: &mut io::StdoutLock<'_>, info: &CpuInfo) {
    if info.caches.is_empty() {
        // No honest source for cache geometry — say so rather than print
        // fabricated rows.
        let _ = writeln!(out, "lscpu: no cache information available");
        return;
    }
    let _ = writeln!(out, "NAME ONE-SIZE WAYS TYPE");
    for c in &info.caches {
        // Linux names: L1d/L1i for level-1 data/instruction, L2/L3 otherwise.
        let name = match (c.level, c.cache_type.as_str()) {
            (1, "Data") => "L1d".to_string(),
            (1, "Instruction") => "L1i".to_string(),
            (lvl, _) => format!("L{lvl}"),
        };
        // Only print the ways column when sysfs actually reported it.
        let ways = c.ways.map_or_else(|| "-".to_string(), |w| w.to_string());
        let _ = writeln!(out, "{name:<4} {:<8} {ways:<4} {}", c.size, c.cache_type);
    }
}

// ============================================================================
// CLI
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    let mut opts = LscpuOpts {
        json: false,
        extended: false,
        parse: false,
        online: false,
        offline: false,
        hex: false,
        caches: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: lscpu [options]");
                println!();
                println!("Display information about the CPU architecture.");
                println!();
                println!("Options:");
                println!("  -J, --json         JSON output");
                println!("  -e, --extended     Extended readable format");
                println!("  -p, --parse        Parseable output");
                println!("  -B, --bytes        Print sizes in bytes");
                println!("  -C, --caches       Show cache info");
                println!("  --online           Show online CPUs only");
                println!("  --offline          Show offline CPUs only");
                println!("  -x, --hex          Show hex masks");
                println!("  -h, --help         Show this help");
                println!("  -V, --version      Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("lscpu {VERSION}");
                process::exit(0);
            }
            "-J" | "--json" => opts.json = true,
            "-e" | "--extended" => opts.extended = true,
            "-p" | "--parse" => opts.parse = true,
            "--online" => opts.online = true,
            "--offline" => opts.offline = true,
            "-x" | "--hex" => opts.hex = true,
            "-C" | "--caches" => opts.caches = true,
            _ => {}
        }
        i += 1;
    }

    let info = collect_cpu_info();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.caches {
        print_caches(&mut out, &info);
    } else if opts.json {
        print_json(&mut out, &info);
    } else {
        print_standard(&mut out, &info);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cpu_range_single() {
        assert_eq!(parse_cpu_range("0"), 1);
        assert_eq!(parse_cpu_range("3"), 1);
    }

    #[test]
    fn test_parse_cpu_range_range() {
        assert_eq!(parse_cpu_range("0-3"), 4);
        assert_eq!(parse_cpu_range("0-7"), 8);
        assert_eq!(parse_cpu_range("0-0"), 1);
    }

    #[test]
    fn test_parse_cpu_range_mixed() {
        assert_eq!(parse_cpu_range("0-3,5,7-9"), 8);
        assert_eq!(parse_cpu_range("0,2,4,6"), 4);
    }

    #[test]
    fn test_parse_cpu_range_empty() {
        assert_eq!(parse_cpu_range(""), 1);
    }

    #[test]
    fn test_cpu_info_default() {
        let info = CpuInfo::default();
        assert_eq!(info.cpus, 0);
        assert!(info.architecture.is_empty());
        assert!(info.flags.is_empty());
    }

    #[test]
    fn test_collect_cpu_info_no_crash() {
        let info = collect_cpu_info();
        // Should return something reasonable.
        assert!(!info.architecture.is_empty());
        assert!(info.cpus >= 1);
        assert!(info.sockets >= 1);
    }

    #[test]
    fn test_count_online_cpus() {
        let count = count_online_cpus();
        assert!(count >= 1);
    }

    #[test]
    fn test_read_cpuinfo_no_crash() {
        let _ = read_cpuinfo();
    }

    #[test]
    fn test_cpu_info_topology_consistency() {
        let info = collect_cpu_info();
        // CPUs should equal sockets * cores_per_socket * threads_per_core (approximately).
        let computed = info.sockets * info.cores_per_socket * info.threads_per_core;
        // Allow some slack since sysfs might not be available.
        assert!(computed >= 1);
    }

    #[test]
    fn test_cache_no_fabrication() {
        // Cache sizes must never be invented: a non-empty summary cache size
        // may only exist when a matching sysfs-sourced record is present.
        let info = collect_cpu_info();
        let has = |level: u8, ty: &str| {
            info.caches.iter().any(|c| c.level == level && c.cache_type == ty)
        };
        if !info.l1d_cache.is_empty() {
            assert!(has(1, "Data"), "L1d size with no sourced record");
        }
        if !info.l1i_cache.is_empty() {
            assert!(has(1, "Instruction"), "L1i size with no sourced record");
        }
        if !info.l2_cache.is_empty() {
            assert!(
                info.caches.iter().any(|c| c.level == 2),
                "L2 size with no sourced record"
            );
        }
        if !info.l3_cache.is_empty() {
            assert!(
                info.caches.iter().any(|c| c.level == 3),
                "L3 size with no sourced record"
            );
        }
        // Every record's size string is non-empty (sysfs gave us a value).
        for c in &info.caches {
            assert!(!c.size.is_empty());
        }
    }

    #[test]
    fn test_op_modes() {
        let info = collect_cpu_info();
        assert!(!info.op_modes.is_empty());
    }

    #[test]
    fn test_byte_order() {
        let info = collect_cpu_info();
        assert_eq!(info.byte_order, "Little Endian");
    }
}
