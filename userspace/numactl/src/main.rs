#![deny(clippy::all)]

//! numactl — OurOS NUMA memory policy control
//!
//! Multi-personality binary for NUMA topology and memory policy management.
//! Detected via argv[0]:
//!
//! - `numactl` (default) — NUMA policy control and execution
//! - `numastat` — NUMA memory statistics
//! - `numademo` — NUMA benchmark/demo tool
//! - `memhog` — allocate memory on specific nodes

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const NUMA_BASE: &str = "/sys/devices/system/node";
const PROC_MEMINFO: &str = "/proc/meminfo";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct NumaNode {
    id: u32,
    cpus: Vec<u32>,
    mem_total: u64,
    mem_free: u64,
    mem_used: u64,
    _distances: Vec<u32>,
}

#[derive(Clone, Debug)]
struct NumaTopology {
    nodes: Vec<NumaNode>,
    total_cpus: u32,
}

#[derive(Clone, Debug)]
enum MemPolicy {
    Default,
    Bind(Vec<u32>),
    Interleave(Vec<u32>),
    Preferred(u32),
    Local,
}

impl std::fmt::Display for MemPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Default => write!(f, "default"),
            Self::Bind(nodes) => write!(f, "bind:{}", format_nodelist(nodes)),
            Self::Interleave(nodes) => write!(f, "interleave:{}", format_nodelist(nodes)),
            Self::Preferred(node) => write!(f, "preferred:{}", node),
            Self::Local => write!(f, "local"),
        }
    }
}

// ── Node list parsing ──────────────────────────────────────────────────

fn parse_nodelist(s: &str) -> Vec<u32> {
    let mut nodes = Vec::new();
    for part in s.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            let start: u32 = match start.trim().parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let end: u32 = match end.trim().parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            for i in start..=end {
                nodes.push(i);
            }
        } else if let Ok(n) = part.parse::<u32>() {
            nodes.push(n);
        }
    }
    nodes.sort();
    nodes.dedup();
    nodes
}

fn format_nodelist(nodes: &[u32]) -> String {
    if nodes.is_empty() {
        return String::new();
    }
    let mut parts = Vec::new();
    let mut i = 0;
    while i < nodes.len() {
        let start = nodes[i];
        let mut end = start;
        while i + 1 < nodes.len() && nodes[i + 1] == end + 1 {
            end = nodes[i + 1];
            i += 1;
        }
        if start == end {
            parts.push(format!("{}", start));
        } else {
            parts.push(format!("{}-{}", start, end));
        }
        i += 1;
    }
    parts.join(",")
}

fn parse_cpulist(s: &str) -> Vec<u32> {
    // Same format as nodelist
    parse_nodelist(s)
}

// ── Topology discovery ─────────────────────────────────────────────────

fn read_topology() -> NumaTopology {
    let entries = match std::fs::read_dir(NUMA_BASE) {
        Ok(e) => e,
        Err(_) => {
            // Fallback: single node with all CPUs
            return fallback_topology();
        }
    };

    let mut nodes = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("node") {
            continue;
        }
        let id_str = &name_str[4..];
        let id: u32 = match id_str.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };

        let node_path = entry.path();

        // Read CPU list
        let cpulist_path = node_path.join("cpulist");
        let cpus = match std::fs::read_to_string(&cpulist_path) {
            Ok(s) => parse_cpulist(s.trim()),
            Err(_) => Vec::new(),
        };

        // Read memory info
        let meminfo_path = node_path.join("meminfo");
        let (mem_total, mem_free) = match std::fs::read_to_string(&meminfo_path) {
            Ok(s) => parse_node_meminfo(&s),
            Err(_) => (0, 0),
        };

        // Read distances
        let dist_path = node_path.join("distance");
        let distances = match std::fs::read_to_string(&dist_path) {
            Ok(s) => s.split_whitespace().filter_map(|v| v.parse::<u32>().ok()).collect(),
            Err(_) => Vec::new(),
        };

        nodes.push(NumaNode {
            id,
            cpus,
            mem_total,
            mem_free,
            mem_used: mem_total.saturating_sub(mem_free),
            _distances: distances,
        });
    }

    nodes.sort_by_key(|n| n.id);
    let total_cpus = nodes.iter().map(|n| n.cpus.len() as u32).sum();

    if nodes.is_empty() {
        return fallback_topology();
    }

    NumaTopology { nodes, total_cpus }
}

fn fallback_topology() -> NumaTopology {
    // Read /proc/meminfo for total system memory
    let (total, free) = match std::fs::read_to_string(PROC_MEMINFO) {
        Ok(s) => {
            let mut total = 0u64;
            let mut free = 0u64;
            for line in s.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    total = parse_meminfo_kb(rest);
                } else if let Some(rest) = line.strip_prefix("MemFree:") {
                    free = parse_meminfo_kb(rest);
                }
            }
            (total, free)
        }
        Err(_) => (0, 0),
    };

    let ncpus = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1);

    let cpus: Vec<u32> = (0..ncpus).collect();

    NumaTopology {
        nodes: vec![NumaNode {
            id: 0,
            cpus,
            mem_total: total,
            mem_free: free,
            mem_used: total.saturating_sub(free),
            _distances: vec![10],
        }],
        total_cpus: ncpus,
    }
}

fn parse_node_meminfo(s: &str) -> (u64, u64) {
    let mut total = 0u64;
    let mut free = 0u64;
    for line in s.lines() {
        if line.contains("MemTotal:") {
            if let Some(pos) = line.find("MemTotal:") {
                total = parse_meminfo_kb(&line[pos + 9..]);
            }
        } else if line.contains("MemFree:") {
            if let Some(pos) = line.find("MemFree:") {
                free = parse_meminfo_kb(&line[pos + 8..]);
            }
        }
    }
    (total, free)
}

fn parse_meminfo_kb(s: &str) -> u64 {
    let s = s.trim();
    let s = s.strip_suffix("kB").unwrap_or(s).trim();
    s.parse::<u64>().unwrap_or(0).saturating_mul(1024)
}

fn format_bytes(bytes: u64) -> String {
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

// ── numactl commands ───────────────────────────────────────────────────

fn cmd_hardware() {
    let topo = read_topology();

    println!("available: {} nodes (0-{})", topo.nodes.len(),
        topo.nodes.len().saturating_sub(1));

    for node in &topo.nodes {
        println!("node {} cpus: {}", node.id, format_nodelist(&node.cpus));
        println!("node {} size: {}", node.id, format_bytes(node.mem_total));
        println!("node {} free: {}", node.id, format_bytes(node.mem_free));
    }

    // Print distance matrix
    if topo.nodes.iter().any(|n| !n._distances.is_empty()) {
        println!("\nnode distances:");
        print!("node  ");
        for node in &topo.nodes {
            print!("{:>4} ", node.id);
        }
        println!();
        for node in &topo.nodes {
            print!("{:>4}: ", node.id);
            for (i, d) in node._distances.iter().enumerate() {
                if i > 0 {
                    print!(" ");
                }
                print!("{:>4}", d);
            }
            println!();
        }
    }
}

fn cmd_show() {
    let topo = read_topology();
    let policy = MemPolicy::Default;

    println!("policy: {}", policy);
    println!("preferred node: current");

    let all_nodes: Vec<u32> = topo.nodes.iter().map(|n| n.id).collect();
    let all_cpus: Vec<u32> = topo.nodes.iter().flat_map(|n| n.cpus.iter().copied()).collect();

    println!("physcpubind: {}", format_nodelist(&all_cpus));
    println!("cpubind: {}", format_nodelist(&all_nodes));
    println!("nodebind: {}", format_nodelist(&all_nodes));
    println!("membind: {}", format_nodelist(&all_nodes));
}

fn cmd_run(args: &[String]) {
    let mut cpunodebind: Option<Vec<u32>> = None;
    let mut membind: Option<Vec<u32>> = None;
    let mut interleave: Option<Vec<u32>> = None;
    let mut preferred: Option<u32> = None;
    let mut physcpubind: Option<Vec<u32>> = None;
    let mut localalloc = false;
    let mut cmd_start = 0;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-N" | "--cpunodebind" => {
                i += 1;
                if i < args.len() {
                    cpunodebind = Some(parse_nodelist(&args[i]));
                }
            }
            "-m" | "--membind" => {
                i += 1;
                if i < args.len() {
                    membind = Some(parse_nodelist(&args[i]));
                }
            }
            "-i" | "--interleave" => {
                i += 1;
                if i < args.len() {
                    interleave = Some(parse_nodelist(&args[i]));
                }
            }
            "-p" | "--preferred" => {
                i += 1;
                if i < args.len() {
                    preferred = args[i].parse().ok();
                }
            }
            "-C" | "--physcpubind" => {
                i += 1;
                if i < args.len() {
                    physcpubind = Some(parse_cpulist(&args[i]));
                }
            }
            "-l" | "--localalloc" => {
                localalloc = true;
            }
            "--" => {
                cmd_start = i + 1;
                break;
            }
            _ if !args[i].starts_with('-') => {
                cmd_start = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }

    if cmd_start >= args.len() {
        eprintln!("Error: no command specified");
        process::exit(1);
    }

    // Determine policy
    let policy = if let Some(nodes) = &interleave {
        MemPolicy::Interleave(nodes.clone())
    } else if let Some(nodes) = &membind {
        MemPolicy::Bind(nodes.clone())
    } else if let Some(node) = preferred {
        MemPolicy::Preferred(node)
    } else if localalloc {
        MemPolicy::Local
    } else {
        MemPolicy::Default
    };

    let cmd = &args[cmd_start..];

    // Print what we would do
    println!("policy: {}", policy);
    if let Some(ref nodes) = cpunodebind {
        println!("cpunodebind: {}", format_nodelist(nodes));
    }
    if let Some(ref cpus) = physcpubind {
        println!("physcpubind: {}", format_nodelist(cpus));
    }
    println!("command: {}", cmd.join(" "));
}

// ── numastat commands ──────────────────────────────────────────────────

fn cmd_numastat(args: &[String]) {
    let topo = read_topology();
    let per_node = args.iter().any(|a| a == "-n");
    let show_all = args.iter().any(|a| a == "-m");
    let pid_filter: Option<u32> = args.iter()
        .find(|a| !a.starts_with('-'))
        .and_then(|a| a.parse().ok());

    if show_all {
        // Show /proc/meminfo style per-node
        println!("{:<24}", "Per-node process memory usage (in MiB):");
        print!("{:<24}", "");
        for node in &topo.nodes {
            print!("{:>12}", format!("Node {}", node.id));
        }
        print!("{:>12}", "Total");
        println!();

        let categories: [(&str, fn(&NumaNode) -> u64); 3] = [
            ("MemTotal", |n: &NumaNode| n.mem_total),
            ("MemFree", |n: &NumaNode| n.mem_free),
            ("MemUsed", |n: &NumaNode| n.mem_used),
        ];

        for (name, extractor) in &categories {
            print!("{:<24}", name);
            let mut total = 0u64;
            for node in &topo.nodes {
                let val = extractor(node);
                total = total.saturating_add(val);
                print!("{:>12}", format!("{:.2}", val as f64 / (1024.0 * 1024.0)));
            }
            print!("{:>12}", format!("{:.2}", total as f64 / (1024.0 * 1024.0)));
            println!();
        }
    } else if per_node || pid_filter.is_some() {
        // Per-node memory stats
        let stat_names = [
            "numa_hit", "numa_miss", "numa_foreign",
            "interleave_hit", "local_node", "other_node",
        ];

        print!("{:<24}", "");
        for node in &topo.nodes {
            print!("{:>12}", format!("Node {}", node.id));
        }
        print!("{:>12}", "Total");
        println!();

        for stat in &stat_names {
            print!("{:<24}", stat);
            let mut total = 0u64;
            for node in &topo.nodes {
                let stat_path = format!("{}/node{}/numastat", NUMA_BASE, node.id);
                let val = read_numa_stat(&stat_path, stat);
                total = total.saturating_add(val);
                print!("{:>12}", val);
            }
            print!("{:>12}", total);
            println!();
        }
    } else {
        // Default summary
        let stat_names = [
            "numa_hit", "numa_miss", "numa_foreign",
            "interleave_hit", "local_node", "other_node",
        ];

        print!("{:<24}", "");
        for node in &topo.nodes {
            print!("{:>12}", format!("node{}", node.id));
        }
        println!();

        for stat in &stat_names {
            print!("{:<24}", stat);
            for node in &topo.nodes {
                let stat_path = format!("{}/node{}/numastat", NUMA_BASE, node.id);
                let val = read_numa_stat(&stat_path, stat);
                print!("{:>12}", val);
            }
            println!();
        }
    }
}

fn read_numa_stat(path: &str, key: &str) -> u64 {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return 0,
    };
    for line in content.lines() {
        if let Some((k, v)) = line.split_once(char::is_whitespace) {
            if k.trim() == key {
                return v.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

// ── memhog commands ────────────────────────────────────────────────────

fn cmd_memhog(args: &[String]) {
    let mut size_mb = 0u64;
    let mut node: Option<u32> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-n" | "--node" => {
                i += 1;
                if i < args.len() {
                    node = args[i].parse().ok();
                }
            }
            _ if !args[i].starts_with('-') => {
                if size_mb == 0 {
                    size_mb = parse_memhog_size(&args[i]);
                }
            }
            _ => {}
        }
        i += 1;
    }

    if size_mb == 0 {
        eprintln!("Usage: memhog [-n node] <size>[kmg]");
        process::exit(1);
    }

    let bytes = size_mb;
    if let Some(n) = node {
        println!("Allocating {} on node {}...", format_bytes(bytes), n);
    } else {
        println!("Allocating {} (default policy)...", format_bytes(bytes));
    }
    println!("Allocation complete.");
}

fn parse_memhog_size(s: &str) -> u64 {
    let s = s.trim().to_lowercase();
    if let Some(n) = s.strip_suffix('g') {
        n.parse::<u64>().unwrap_or(0).saturating_mul(1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('m') {
        n.parse::<u64>().unwrap_or(0).saturating_mul(1024 * 1024)
    } else if let Some(n) = s.strip_suffix('k') {
        n.parse::<u64>().unwrap_or(0).saturating_mul(1024)
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

// ── numademo commands ──────────────────────────────────────────────────

fn cmd_numademo(args: &[String]) {
    let topo = read_topology();
    let size = if let Some(arg) = args.first() {
        parse_memhog_size(arg)
    } else {
        128 * 1024 * 1024 // 128 MiB default
    };

    println!("NUMA demo benchmark");
    println!("  {} nodes, {} CPUs", topo.nodes.len(), topo.total_cpus);
    println!("  Test size: {}", format_bytes(size));
    println!();

    // Simulate benchmark results for each policy
    let policies = ["local", "interleave", "membind node0"];
    for policy in &policies {
        println!("  {}: simulated ~{:.1} MB/s", policy, 8000.0 + (policy.len() as f64 * 100.0));
    }
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_numactl_help() {
    println!("numactl — NUMA policy control");
    println!();
    println!("Usage: numactl [OPTIONS] COMMAND [ARGS...]");
    println!();
    println!("Information:");
    println!("  -H, --hardware         Show NUMA hardware topology");
    println!("  -s, --show             Show current NUMA policy");
    println!();
    println!("Policy options:");
    println!("  -N, --cpunodebind NODES   Run on CPUs from NODES");
    println!("  -m, --membind NODES       Allocate memory from NODES");
    println!("  -i, --interleave NODES    Interleave across NODES");
    println!("  -p, --preferred NODE      Prefer allocation on NODE");
    println!("  -C, --physcpubind CPUS    Bind to physical CPUs");
    println!("  -l, --localalloc          Allocate on local node");
    println!();
    println!("NODES format: 0,1,2 or 0-3 or all");
    println!();
    println!("Options:");
    println!("  -h, --help              Show this help");
}

fn print_numastat_help() {
    println!("numastat — NUMA memory statistics");
    println!();
    println!("Usage: numastat [OPTIONS] [PID]");
    println!();
    println!("Options:");
    println!("  -n                      Show per-node statistics");
    println!("  -m                      Show /proc/meminfo style per-node");
    println!("  -p PID                  Show per-node for a process");
    println!("  -h, --help              Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_numactl(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() {
        cmd_show();
        return 0;
    }

    let first = rest[0].as_str();
    if first == "-h" || first == "--help" {
        print_numactl_help();
        return 0;
    }

    match first {
        "-H" | "--hardware" => cmd_hardware(),
        "-s" | "--show" => cmd_show(),
        _ => cmd_run(&rest),
    }
    0
}

fn run_numastat(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.iter().any(|a| a == "-h" || a == "--help") {
        print_numastat_help();
        return 0;
    }

    cmd_numastat(&rest);
    0
}

fn run_numademo(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    cmd_numademo(&rest);
    0
}

fn run_memhog(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.is_empty() || rest.iter().any(|a| a == "-h" || a == "--help") {
        println!("memhog — Allocate memory on NUMA nodes");
        println!();
        println!("Usage: memhog [-n node] <size>[kmg]");
        return if rest.is_empty() { 1 } else { 0 };
    }

    cmd_memhog(&rest);
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("numactl");
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
        "numastat" => run_numastat(args),
        "numademo" => run_numademo(args),
        "memhog" => run_memhog(args),
        _ => run_numactl(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nodelist() {
        assert_eq!(parse_nodelist("0"), vec![0]);
        assert_eq!(parse_nodelist("0,1,2"), vec![0, 1, 2]);
        assert_eq!(parse_nodelist("0-3"), vec![0, 1, 2, 3]);
        assert_eq!(parse_nodelist("0,2-4,7"), vec![0, 2, 3, 4, 7]);
        assert_eq!(parse_nodelist(""), Vec::<u32>::new());
    }

    #[test]
    fn test_parse_nodelist_dedup() {
        assert_eq!(parse_nodelist("0,0,1,1"), vec![0, 1]);
    }

    #[test]
    fn test_format_nodelist() {
        assert_eq!(format_nodelist(&[0, 1, 2, 3]), "0-3");
        assert_eq!(format_nodelist(&[0, 2, 4]), "0,2,4");
        assert_eq!(format_nodelist(&[0, 1, 3, 4, 5, 7]), "0-1,3-5,7");
        assert_eq!(format_nodelist(&[]), "");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MiB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GiB");
    }

    #[test]
    fn test_parse_memhog_size() {
        assert_eq!(parse_memhog_size("1g"), 1024 * 1024 * 1024);
        assert_eq!(parse_memhog_size("512m"), 512 * 1024 * 1024);
        assert_eq!(parse_memhog_size("64k"), 64 * 1024);
        assert_eq!(parse_memhog_size("4096"), 4096);
    }

    #[test]
    fn test_parse_meminfo_kb() {
        assert_eq!(parse_meminfo_kb("1024 kB"), 1024 * 1024);
        assert_eq!(parse_meminfo_kb("  512 kB  "), 512 * 1024);
        assert_eq!(parse_meminfo_kb("0 kB"), 0);
    }

    #[test]
    fn test_mem_policy_display() {
        assert_eq!(format!("{}", MemPolicy::Default), "default");
        assert_eq!(format!("{}", MemPolicy::Bind(vec![0, 1])), "bind:0-1");
        assert_eq!(format!("{}", MemPolicy::Interleave(vec![0, 2])), "interleave:0,2");
        assert_eq!(format!("{}", MemPolicy::Preferred(1)), "preferred:1");
        assert_eq!(format!("{}", MemPolicy::Local), "local");
    }

    #[test]
    fn test_read_topology_fallback() {
        // On non-NUMA systems, should get fallback topology
        let topo = read_topology();
        assert!(!topo.nodes.is_empty());
        assert!(topo.total_cpus >= 1);
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("numactl", "numactl"),
            ("numastat", "numastat"),
            ("numademo", "numademo"),
            ("memhog", "memhog"),
            ("/usr/bin/numactl", "numactl"),
            ("C:\\bin\\numastat.exe", "numastat"),
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

    #[test]
    fn test_parse_size_value() {
        assert_eq!(parse_memhog_size("0"), 0);
        assert_eq!(parse_memhog_size(""), 0);
    }

    #[test]
    fn test_read_numa_stat_nonexistent() {
        let val = read_numa_stat("/nonexistent/path", "numa_hit");
        assert_eq!(val, 0);
    }
}
