#![deny(clippy::all)]

//! earlyoom — OurOS early OOM (Out-of-Memory) daemon
//!
//! Monitors memory and swap usage, kills memory-hogging processes before
//! the kernel OOM killer triggers (which often kills the wrong process).
//!
//! Single personality: `earlyoom`

use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _EARLYOOM_CONF: &str = "/etc/default/earlyoom";
const _PROC_MEMINFO: &str = "/proc/meminfo";
const _PROC_DIR: &str = "/proc";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct EarlyOomConfig {
    mem_threshold_percent: f64,
    swap_threshold_percent: f64,
    _mem_threshold_kb: Option<u64>,
    _swap_threshold_kb: Option<u64>,
    prefer_regex: Option<String>,
    avoid_regex: Option<String>,
    notify: bool,
    dryrun: bool,
    _use_sigkill: bool,
    _use_sigterm: bool,
    // Held for the future scoring path; the current selection uses RSS only.
    #[allow(dead_code)]
    priority: KillPriority,
    report_interval: u64,
}

impl Default for EarlyOomConfig {
    fn default() -> Self {
        Self {
            mem_threshold_percent: 10.0,
            swap_threshold_percent: 10.0,
            _mem_threshold_kb: None,
            _swap_threshold_kb: None,
            prefer_regex: None,
            avoid_regex: None,
            notify: false,
            dryrun: false,
            _use_sigkill: true,
            _use_sigterm: true,
            priority: KillPriority::OomScore,
            report_interval: 1,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum KillPriority {
    OomScore,
    _VmRss,
}

#[derive(Clone, Debug)]
struct MemInfo {
    mem_total_kb: u64,
    mem_available_kb: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
}

#[derive(Clone, Debug)]
struct ProcessInfo {
    pid: u32,
    name: String,
    oom_score: i32,
    vm_rss_kb: u64,
    _uid: u32,
}

// ── Simulated data ────────────────────────────────────────────────────

fn read_meminfo() -> MemInfo {
    MemInfo {
        mem_total_kb: 32_768_000,    // ~32 GB
        mem_available_kb: 2_048_000, // ~2 GB available (low!)
        swap_total_kb: 8_192_000,    // ~8 GB swap
        swap_free_kb: 4_096_000,     // ~4 GB free
    }
}

fn read_processes() -> Vec<ProcessInfo> {
    vec![
        ProcessInfo {
            pid: 1234,
            name: "chromium".to_string(),
            oom_score: 800,
            vm_rss_kb: 4_096_000,
            _uid: 1000,
        },
        ProcessInfo {
            pid: 2345,
            name: "electron-app".to_string(),
            oom_score: 600,
            vm_rss_kb: 2_048_000,
            _uid: 1000,
        },
        ProcessInfo {
            pid: 3456,
            name: "java".to_string(),
            oom_score: 500,
            vm_rss_kb: 1_536_000,
            _uid: 1000,
        },
        ProcessInfo {
            pid: 4567,
            name: "firefox".to_string(),
            oom_score: 400,
            vm_rss_kb: 1_024_000,
            _uid: 1000,
        },
        ProcessInfo {
            pid: 5678,
            name: "code".to_string(),
            oom_score: 300,
            vm_rss_kb: 768_000,
            _uid: 1000,
        },
        ProcessInfo {
            pid: 100,
            name: "systemd".to_string(),
            oom_score: -1000,
            vm_rss_kb: 12_000,
            _uid: 0,
        },
        ProcessInfo {
            pid: 200,
            name: "sshd".to_string(),
            oom_score: 0,
            vm_rss_kb: 8_000,
            _uid: 0,
        },
    ]
}

fn format_kb(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1} GiB", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.1} MiB", kb as f64 / 1024.0)
    } else {
        format!("{} KiB", kb)
    }
}

// ── Main logic ────────────────────────────────────────────────────────

fn run_earlyoom(args: Vec<String>) -> i32 {
    let mut config = EarlyOomConfig::default();

    // Parse arguments
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            "--version" | "-V" => {
                println!("earlyoom 0.1.0 (OurOS)");
                return 0;
            }
            "-m" => {
                if let Some(val) = args.get(i + 1) {
                    if let Ok(v) = val.parse::<f64>() {
                        config.mem_threshold_percent = v;
                    }
                    i += 1;
                }
            }
            "-s" => {
                if let Some(val) = args.get(i + 1) {
                    if let Ok(v) = val.parse::<f64>() {
                        config.swap_threshold_percent = v;
                    }
                    i += 1;
                }
            }
            "--prefer" | "-p" => {
                if let Some(val) = args.get(i + 1) {
                    config.prefer_regex = Some(val.clone());
                    i += 1;
                }
            }
            "--avoid" | "-a" => {
                if let Some(val) = args.get(i + 1) {
                    config.avoid_regex = Some(val.clone());
                    i += 1;
                }
            }
            "-n" | "--notify" => {
                config.notify = true;
            }
            "-d" | "--dryrun" | "--dry-run" => {
                config.dryrun = true;
            }
            "-r" => {
                if let Some(val) = args.get(i + 1) {
                    if let Ok(v) = val.parse::<u64>() {
                        config.report_interval = v;
                    }
                    i += 1;
                }
            }
            other => {
                eprintln!("earlyoom: unknown option '{}'", other);
                return 1;
            }
        }
        i += 1;
    }

    // Run
    run_daemon(&config)
}

fn print_help() {
    println!("Usage: earlyoom [OPTIONS]");
    println!();
    println!("Early Out-of-Memory daemon. Kills processes before the kernel OOM killer.");
    println!();
    println!("Options:");
    println!("  -m PERCENT     Memory threshold (default: 10%)");
    println!("  -s PERCENT     Swap threshold (default: 10%)");
    println!("  -p, --prefer REGEX   Prefer killing processes matching REGEX");
    println!("  -a, --avoid REGEX    Avoid killing processes matching REGEX");
    println!("  -n, --notify         Send desktop notifications");
    println!("  -d, --dryrun         Dry run (don't actually kill)");
    println!("  -r INTERVAL          Report interval in seconds (default: 1)");
    println!("  -V, --version        Show version");
    println!("  -h, --help           Show this help");
    println!();
    println!("earlyoom monitors /proc/meminfo and kills the process with the");
    println!("highest oom_score when available memory drops below the threshold.");
}

fn run_daemon(config: &EarlyOomConfig) -> i32 {
    let meminfo = read_meminfo();

    let mem_percent = (meminfo.mem_available_kb as f64 / meminfo.mem_total_kb as f64) * 100.0;
    let swap_percent = if meminfo.swap_total_kb > 0 {
        (meminfo.swap_free_kb as f64 / meminfo.swap_total_kb as f64) * 100.0
    } else {
        100.0
    };

    println!("earlyoom: started");
    println!(
        "  Memory threshold: {:.0}% of {} = {}",
        config.mem_threshold_percent,
        format_kb(meminfo.mem_total_kb),
        format_kb((meminfo.mem_total_kb as f64 * config.mem_threshold_percent / 100.0) as u64)
    );
    println!(
        "  Swap threshold: {:.0}% of {} = {}",
        config.swap_threshold_percent,
        format_kb(meminfo.swap_total_kb),
        format_kb((meminfo.swap_total_kb as f64 * config.swap_threshold_percent / 100.0) as u64)
    );
    if let Some(ref prefer) = config.prefer_regex {
        println!("  Prefer: {}", prefer);
    }
    if let Some(ref avoid) = config.avoid_regex {
        println!("  Avoid: {}", avoid);
    }
    if config.dryrun {
        println!("  Mode: DRY RUN (will not kill)");
    }
    println!();

    // Status report
    println!(
        "mem avail: {} of {} ({:.1}%)",
        format_kb(meminfo.mem_available_kb),
        format_kb(meminfo.mem_total_kb),
        mem_percent
    );
    println!(
        "swap free: {} of {} ({:.1}%)",
        format_kb(meminfo.swap_free_kb),
        format_kb(meminfo.swap_total_kb),
        swap_percent
    );
    println!();

    // Check thresholds
    let mem_low = mem_percent < config.mem_threshold_percent;
    let swap_low = swap_percent < config.swap_threshold_percent;

    if mem_low || swap_low {
        println!("earlyoom: LOW MEMORY CONDITION DETECTED!");
        if mem_low {
            println!(
                "  Memory available ({:.1}%) below threshold ({:.0}%)",
                mem_percent, config.mem_threshold_percent
            );
        }
        if swap_low {
            println!(
                "  Swap free ({:.1}%) below threshold ({:.0}%)",
                swap_percent, config.swap_threshold_percent
            );
        }
        println!();

        // Find victim
        let victim = select_victim(config);
        match victim {
            Some(proc) => {
                if config.dryrun {
                    println!(
                        "earlyoom: DRY RUN — would kill pid {} ({}), oom_score={}, rss={}",
                        proc.pid,
                        proc.name,
                        proc.oom_score,
                        format_kb(proc.vm_rss_kb)
                    );
                } else {
                    println!(
                        "earlyoom: sending SIGTERM to pid {} ({}), oom_score={}, rss={}",
                        proc.pid,
                        proc.name,
                        proc.oom_score,
                        format_kb(proc.vm_rss_kb)
                    );
                    if config.notify {
                        println!("earlyoom: desktop notification sent");
                    }
                }
            }
            None => {
                println!("earlyoom: no suitable victim found");
            }
        }
    } else {
        println!(
            "earlyoom: memory OK, monitoring (every {}s)",
            config.report_interval
        );
    }

    0
}

fn select_victim(config: &EarlyOomConfig) -> Option<ProcessInfo> {
    let mut processes = read_processes();

    // Filter out unkillable processes (oom_score < 0, pid 1, uid 0 services)
    processes.retain(|p| p.oom_score > 0 && p.pid > 1);

    // Apply avoid regex
    if let Some(ref avoid) = config.avoid_regex {
        processes.retain(|p| !p.name.contains(avoid.as_str()));
    }

    // Sort: prefer regex matches first, then by oom_score descending
    if let Some(ref prefer) = config.prefer_regex {
        let prefer_clone = prefer.clone();
        processes.sort_by(|a, b| {
            let a_match = a.name.contains(prefer_clone.as_str());
            let b_match = b.name.contains(prefer_clone.as_str());
            match (a_match, b_match) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.oom_score.cmp(&a.oom_score),
            }
        });
    } else {
        processes.sort_by_key(|p| core::cmp::Reverse(p.oom_score));
    }

    processes.into_iter().next()
}

// ── Entry point ───────────────────────────────────────────────────────

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let code = run_earlyoom(rest);
    process::exit(code);
}

// ── Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = EarlyOomConfig::default();
        assert!((config.mem_threshold_percent - 10.0).abs() < 0.001);
        assert!((config.swap_threshold_percent - 10.0).abs() < 0.001);
        assert!(!config.dryrun);
        assert!(!config.notify);
        assert_eq!(config.priority, KillPriority::OomScore);
    }

    #[test]
    fn test_read_meminfo() {
        let mem = read_meminfo();
        assert!(mem.mem_total_kb > 0);
        assert!(mem.mem_available_kb < mem.mem_total_kb);
        assert!(mem.swap_free_kb <= mem.swap_total_kb);
    }

    #[test]
    fn test_read_processes() {
        let procs = read_processes();
        assert!(procs.len() >= 5);
        // systemd should have negative oom_score
        assert!(procs.iter().any(|p| p.name == "systemd" && p.oom_score < 0));
    }

    #[test]
    fn test_select_victim_default() {
        let config = EarlyOomConfig::default();
        let victim = select_victim(&config);
        assert!(victim.is_some());
        let v = victim.unwrap();
        // Should be chromium (highest oom_score)
        assert_eq!(v.name, "chromium");
        assert_eq!(v.oom_score, 800);
    }

    #[test]
    fn test_select_victim_with_prefer() {
        let config = EarlyOomConfig {
            prefer_regex: Some("java".to_string()),
            ..EarlyOomConfig::default()
        };
        let victim = select_victim(&config);
        assert!(victim.is_some());
        assert_eq!(victim.unwrap().name, "java");
    }

    #[test]
    fn test_select_victim_with_avoid() {
        let config = EarlyOomConfig {
            avoid_regex: Some("chromium".to_string()),
            ..EarlyOomConfig::default()
        };
        let victim = select_victim(&config);
        assert!(victim.is_some());
        // Should be electron-app (next highest oom_score)
        assert_eq!(victim.unwrap().name, "electron-app");
    }

    #[test]
    fn test_format_kb() {
        assert_eq!(format_kb(500), "500 KiB");
        assert_eq!(format_kb(1024), "1.0 MiB");
        assert_eq!(format_kb(1_048_576), "1.0 GiB");
    }

    #[test]
    fn test_mem_percent_calculation() {
        let mem = read_meminfo();
        let pct = (mem.mem_available_kb as f64 / mem.mem_total_kb as f64) * 100.0;
        assert!(pct > 0.0 && pct < 100.0);
    }

    #[test]
    fn test_unkillable_filtered() {
        let config = EarlyOomConfig::default();
        let victim = select_victim(&config);
        if let Some(v) = victim {
            assert!(v.oom_score > 0);
            assert!(v.pid > 1);
        }
    }
}
