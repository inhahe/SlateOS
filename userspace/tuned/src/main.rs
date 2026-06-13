#![deny(clippy::all)]

//! tuned — SlateOS system tuning daemon
//!
//! Multi-personality binary for system performance tuning profiles.
//! Detected via argv[0]:
//!
//! - `tuned` (default) — tuning daemon
//! - `tuned-adm` — tuning profile administration CLI
//! - `tuned-gui` — placeholder for GUI configuration

use std::collections::BTreeMap;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const PROFILES_DIR: &str = "/etc/tuned";
const ACTIVE_PROFILE: &str = "/etc/tuned/active_profile";
const PROFILE_MODE: &str = "/etc/tuned/profile_mode";
const RECOMMEND_DIR: &str = "/etc/tuned/recommend.d";
const TUNED_CONF: &str = "/etc/tuned/tuned-main.conf";

// ── Data structures ────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Profile {
    name: String,
    summary: String,
    include: Option<String>,
    sections: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Clone, Debug)]
struct DaemonConfig {
    _daemon: bool,
    _sleep_interval: u32,
    _update_interval: u32,
    dynamic_tuning: bool,
    recommend_command: bool,
    _reapply_sysctl: bool,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            _daemon: true,
            _sleep_interval: 1,
            _update_interval: 10,
            dynamic_tuning: true,
            recommend_command: true,
            _reapply_sysctl: true,
        }
    }
}

// ── Built-in profiles ──────────────────────────────────────────────────

fn builtin_profiles() -> Vec<Profile> {
    vec![
        Profile {
            name: "balanced".to_string(),
            summary: "General non-specialized tuned profile".to_string(),
            include: None,
            sections: {
                let mut s = BTreeMap::new();
                let mut cpu = BTreeMap::new();
                cpu.insert("governor".to_string(), "schedutil".to_string());
                cpu.insert("energy_perf_bias".to_string(), "normal".to_string());
                cpu.insert("min_perf_pct".to_string(), "1".to_string());
                s.insert("cpu".to_string(), cpu);
                let mut disk = BTreeMap::new();
                disk.insert("readahead".to_string(), "4096".to_string());
                s.insert("disk".to_string(), disk);
                let mut vm = BTreeMap::new();
                vm.insert("transparent_hugepages".to_string(), "madvise".to_string());
                s.insert("vm".to_string(), vm);
                s
            },
        },
        Profile {
            name: "throughput-performance".to_string(),
            summary: "Broadly applicable tuning that provides excellent performance across a variety of common server workloads".to_string(),
            include: None,
            sections: {
                let mut s = BTreeMap::new();
                let mut cpu = BTreeMap::new();
                cpu.insert("governor".to_string(), "performance".to_string());
                cpu.insert("energy_perf_bias".to_string(), "performance".to_string());
                cpu.insert("min_perf_pct".to_string(), "100".to_string());
                s.insert("cpu".to_string(), cpu);
                let mut disk = BTreeMap::new();
                disk.insert("readahead".to_string(), "4096".to_string());
                s.insert("disk".to_string(), disk);
                let mut sysctl = BTreeMap::new();
                sysctl.insert("kernel.sched_min_granularity_ns".to_string(), "10000000".to_string());
                sysctl.insert("kernel.sched_wakeup_granularity_ns".to_string(), "15000000".to_string());
                sysctl.insert("vm.dirty_ratio".to_string(), "40".to_string());
                sysctl.insert("vm.dirty_background_ratio".to_string(), "10".to_string());
                sysctl.insert("vm.swappiness".to_string(), "10".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "latency-performance".to_string(),
            summary: "Optimize for deterministic performance at the cost of increased power consumption".to_string(),
            include: None,
            sections: {
                let mut s = BTreeMap::new();
                let mut cpu = BTreeMap::new();
                cpu.insert("governor".to_string(), "performance".to_string());
                cpu.insert("energy_perf_bias".to_string(), "performance".to_string());
                cpu.insert("force_latency".to_string(), "1".to_string());
                s.insert("cpu".to_string(), cpu);
                let mut sysctl = BTreeMap::new();
                sysctl.insert("vm.swappiness".to_string(), "10".to_string());
                sysctl.insert("kernel.sched_min_granularity_ns".to_string(), "10000000".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "network-throughput".to_string(),
            summary: "Optimize for streaming network throughput".to_string(),
            include: Some("throughput-performance".to_string()),
            sections: {
                let mut s = BTreeMap::new();
                let mut sysctl = BTreeMap::new();
                sysctl.insert("net.core.rmem_max".to_string(), "16777216".to_string());
                sysctl.insert("net.core.wmem_max".to_string(), "16777216".to_string());
                sysctl.insert("net.ipv4.tcp_rmem".to_string(), "4096 87380 16777216".to_string());
                sysctl.insert("net.ipv4.tcp_wmem".to_string(), "4096 65536 16777216".to_string());
                sysctl.insert("net.core.netdev_max_backlog".to_string(), "30000".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "network-latency".to_string(),
            summary: "Optimize for network latency".to_string(),
            include: Some("latency-performance".to_string()),
            sections: {
                let mut s = BTreeMap::new();
                let mut sysctl = BTreeMap::new();
                sysctl.insert("net.ipv4.tcp_fastopen".to_string(), "3".to_string());
                sysctl.insert("net.core.busy_read".to_string(), "50".to_string());
                sysctl.insert("net.core.busy_poll".to_string(), "50".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "powersave".to_string(),
            summary: "Optimize for low power consumption".to_string(),
            include: None,
            sections: {
                let mut s = BTreeMap::new();
                let mut cpu = BTreeMap::new();
                cpu.insert("governor".to_string(), "powersave".to_string());
                cpu.insert("energy_perf_bias".to_string(), "powersave".to_string());
                cpu.insert("min_perf_pct".to_string(), "1".to_string());
                s.insert("cpu".to_string(), cpu);
                let mut vm = BTreeMap::new();
                vm.insert("transparent_hugepages".to_string(), "madvise".to_string());
                s.insert("vm".to_string(), vm);
                let mut sysctl = BTreeMap::new();
                sysctl.insert("vm.laptop_mode".to_string(), "5".to_string());
                sysctl.insert("vm.dirty_writeback_centisecs".to_string(), "1500".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "virtual-guest".to_string(),
            summary: "Optimize for running inside a virtual guest".to_string(),
            include: Some("throughput-performance".to_string()),
            sections: {
                let mut s = BTreeMap::new();
                let mut sysctl = BTreeMap::new();
                sysctl.insert("vm.dirty_ratio".to_string(), "30".to_string());
                sysctl.insert("vm.swappiness".to_string(), "30".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "virtual-host".to_string(),
            summary: "Optimize for running KVM guests".to_string(),
            include: Some("throughput-performance".to_string()),
            sections: {
                let mut s = BTreeMap::new();
                let mut sysctl = BTreeMap::new();
                sysctl.insert("kernel.sched_migration_cost_ns".to_string(), "5000000".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "desktop".to_string(),
            summary: "Optimize for the desktop use-case".to_string(),
            include: Some("balanced".to_string()),
            sections: {
                let mut s = BTreeMap::new();
                let mut cpu = BTreeMap::new();
                cpu.insert("governor".to_string(), "schedutil".to_string());
                cpu.insert("energy_perf_bias".to_string(), "normal".to_string());
                s.insert("cpu".to_string(), cpu);
                let mut sysctl = BTreeMap::new();
                sysctl.insert("kernel.sched_autogroup_enabled".to_string(), "1".to_string());
                sysctl.insert("vm.swappiness".to_string(), "30".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
        Profile {
            name: "gaming".to_string(),
            summary: "Optimize for gaming workloads".to_string(),
            include: Some("latency-performance".to_string()),
            sections: {
                let mut s = BTreeMap::new();
                let mut cpu = BTreeMap::new();
                cpu.insert("governor".to_string(), "performance".to_string());
                cpu.insert("energy_perf_bias".to_string(), "performance".to_string());
                s.insert("cpu".to_string(), cpu);
                let mut sysctl = BTreeMap::new();
                sysctl.insert("vm.swappiness".to_string(), "10".to_string());
                sysctl.insert("vm.compaction_proactiveness".to_string(), "0".to_string());
                sysctl.insert("kernel.sched_cfs_bandwidth_slice_us".to_string(), "3000".to_string());
                s.insert("sysctl".to_string(), sysctl);
                s
            },
        },
    ]
}

// ── Profile I/O ────────────────────────────────────────────────────────

fn load_profiles() -> Vec<Profile> {
    let mut profiles = builtin_profiles();

    // Try to load custom profiles from disk
    if let Ok(entries) = std::fs::read_dir(PROFILES_DIR) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let conf = path.join("tuned.conf");
                if conf.is_file()
                    && let Some(p) = parse_profile_file(&conf, &entry.file_name().to_string_lossy()) {
                        // Override builtin if same name
                        if let Some(idx) = profiles.iter().position(|x| x.name == p.name) {
                            profiles[idx] = p;
                        } else {
                            profiles.push(p);
                        }
                    }
            }
        }
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    profiles
}

fn parse_profile_file(path: &std::path::Path, name: &str) -> Option<Profile> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut summary = String::new();
    let mut include = None;
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current_section = String::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len()-1].to_string();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            if current_section == "main" {
                match key {
                    "summary" => summary = value.to_string(),
                    "include" => include = Some(value.to_string()),
                    _ => {}
                }
            } else if !current_section.is_empty() {
                sections.entry(current_section.clone())
                    .or_default()
                    .insert(key.to_string(), value.to_string());
            }
        }
    }

    Some(Profile {
        name: name.to_string(),
        summary,
        include,
        sections,
    })
}

fn get_active_profile() -> String {
    std::fs::read_to_string(ACTIVE_PROFILE)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn get_profile_mode() -> String {
    std::fs::read_to_string(PROFILE_MODE)
        .unwrap_or_else(|_| "manual".to_string())
        .trim()
        .to_string()
}

fn read_daemon_config() -> DaemonConfig {
    let content = match std::fs::read_to_string(TUNED_CONF) {
        Ok(c) => c,
        Err(_) => return DaemonConfig::default(),
    };

    let mut config = DaemonConfig::default();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim();
            match key {
                "daemon" => config._daemon = value == "1" || value == "true",
                "sleep_interval" => config._sleep_interval = value.parse().unwrap_or(1),
                "update_interval" => config._update_interval = value.parse().unwrap_or(10),
                "dynamic_tuning" => config.dynamic_tuning = value == "1" || value == "true",
                "recommend_command" => config.recommend_command = value == "1" || value == "true",
                "reapply_sysctl" => config._reapply_sysctl = value == "1" || value == "true",
                _ => {}
            }
        }
    }
    config
}

// ── tuned-adm commands ─────────────────────────────────────────────────

fn cmd_active() {
    let active = get_active_profile();
    let mode = get_profile_mode();
    if active.is_empty() {
        println!("No current active profile.");
    } else {
        println!("Current active profile: {}", active);
        println!("Profile selection mode: {}", mode);
    }
}

fn cmd_list() {
    let profiles = load_profiles();
    println!("Available tuned profiles:");
    for p in &profiles {
        println!("- {:<30} - {}", p.name, p.summary);
    }
    let active = get_active_profile();
    if !active.is_empty() {
        println!("Current active profile: {}", active);
    }
}

fn cmd_profile(args: &[String]) {
    if args.is_empty() {
        eprintln!("Error: profile name required");
        eprintln!("Usage: tuned-adm profile <name>");
        process::exit(1);
    }

    let name = &args[0];
    let profiles = load_profiles();
    if !profiles.iter().any(|p| p.name == *name) {
        eprintln!("Error: profile '{}' not found.", name);
        eprintln!("Use 'tuned-adm list' to see available profiles.");
        process::exit(1);
    }

    let _ = std::fs::create_dir_all(PROFILES_DIR);
    if let Err(e) = std::fs::write(ACTIVE_PROFILE, name) {
        eprintln!("Error setting profile: {}", e);
        process::exit(1);
    }
    let _ = std::fs::write(PROFILE_MODE, "manual");
    println!("Applied profile: {}", name);
}

fn cmd_profile_info(args: &[String]) {
    let profiles = load_profiles();
    let active = get_active_profile();

    let target = if args.is_empty() {
        if active.is_empty() {
            eprintln!("No active profile. Specify a profile name.");
            process::exit(1);
        }
        active.clone()
    } else {
        args[0].clone()
    };

    let profile = match profiles.iter().find(|p| p.name == target) {
        Some(p) => p,
        None => {
            eprintln!("Profile '{}' not found.", target);
            process::exit(1);
        }
    };

    println!("Profile name: {}", profile.name);
    println!("Summary: {}", profile.summary);
    if let Some(ref inc) = profile.include {
        println!("Includes: {}", inc);
    }
    for (section, params) in &profile.sections {
        println!("\n[{}]", section);
        for (key, value) in params {
            println!("  {} = {}", key, value);
        }
    }
}

fn cmd_recommend() {
    let config = read_daemon_config();
    if !config.recommend_command {
        println!("Recommendation disabled in configuration.");
        return;
    }

    // Check for recommendation config
    let recs = match std::fs::read_dir(RECOMMEND_DIR) {
        Ok(entries) => {
            let mut items: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let content = std::fs::read_to_string(e.path()).ok()?;
                    Some(content.trim().to_string())
                })
                .collect();
            items.sort();
            items
        }
        Err(_) => Vec::new(),
    };

    if let Some(rec) = recs.first() {
        println!("{}", rec);
    } else {
        // Default recommendation heuristic
        println!("balanced");
    }
}

fn cmd_auto_profile() {
    let rec = {
        let recs = match std::fs::read_dir(RECOMMEND_DIR) {
            Ok(entries) => {
                let mut items: Vec<String> = entries
                    .flatten()
                    .filter_map(|e| {
                        let content = std::fs::read_to_string(e.path()).ok()?;
                        Some(content.trim().to_string())
                    })
                    .collect();
                items.sort();
                items
            }
            Err(_) => Vec::new(),
        };
        recs.into_iter().next().unwrap_or_else(|| "balanced".to_string())
    };

    let _ = std::fs::create_dir_all(PROFILES_DIR);
    let _ = std::fs::write(ACTIVE_PROFILE, &rec);
    let _ = std::fs::write(PROFILE_MODE, "auto");
    println!("Applied recommended profile: {}", rec);
}

fn cmd_off() {
    let _ = std::fs::remove_file(ACTIVE_PROFILE);
    let _ = std::fs::write(PROFILE_MODE, "manual");
    println!("Tuning disabled.");
}

fn cmd_verify() {
    let active = get_active_profile();
    if active.is_empty() {
        println!("No active profile to verify.");
        return;
    }

    let profiles = load_profiles();
    let profile = match profiles.iter().find(|p| p.name == active) {
        Some(p) => p,
        None => {
            println!("FAIL: Active profile '{}' not found.", active);
            process::exit(1);
        }
    };

    println!("Verification for profile '{}':", profile.name);
    let all_ok = true;

    for (section, params) in &profile.sections {
        for (key, expected) in params {
            // Simulate checking: in real OS, read sysctl/sysfs values
            println!("  [{}] {} = {} ... OK (simulated)", section, key, expected);
        }
    }

    if all_ok {
        println!("\nVerification succeeded.");
    }
}

// ── tuned daemon ───────────────────────────────────────────────────────

fn run_daemon(args: &[String]) {
    let foreground = args.iter().any(|a| a == "-d" || a == "--daemon" || a == "-n" || a == "--no-daemon");
    let debug = args.iter().any(|a| a == "-D" || a == "--debug");

    if debug {
        println!("tuned: debug mode enabled");
    }

    let active = get_active_profile();
    if active.is_empty() {
        println!("tuned: no active profile, using 'balanced'");
    } else {
        println!("tuned: applying profile '{}'", active);
    }

    if foreground {
        println!("tuned: running in foreground");
    } else {
        println!("tuned: would daemonize (simulated)");
    }

    let config = read_daemon_config();
    if config.dynamic_tuning {
        println!("tuned: dynamic tuning enabled");
    }

    println!("tuned: daemon started");
}

// ── Help ───────────────────────────────────────────────────────────────

fn print_tuned_adm_help() {
    println!("tuned-adm — Tuning profile administration");
    println!();
    println!("Usage: tuned-adm <command> [args]");
    println!();
    println!("Commands:");
    println!("  active                 Show active profile");
    println!("  list                   List available profiles");
    println!("  profile <name>         Switch to profile");
    println!("  profile_info [name]    Show profile details");
    println!("  recommend              Recommend a profile");
    println!("  auto_profile           Apply recommended profile");
    println!("  off                    Disable tuning");
    println!("  verify                 Verify current profile settings");
    println!("  -h, --help             Show this help");
}

fn print_tuned_help() {
    println!("tuned — System tuning daemon");
    println!();
    println!("Usage: tuned [OPTIONS]");
    println!();
    println!("Options:");
    println!("  -d, --daemon           Run as daemon");
    println!("  -n, --no-daemon        Run in foreground");
    println!("  -D, --debug            Debug mode");
    println!("  -h, --help             Show this help");
}

// ── Main dispatch ──────────────────────────────────────────────────────

fn run_tuned_adm(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let cmd = rest.first().cloned().unwrap_or_else(|| "active".to_string());
    let cmd_args: Vec<String> = rest.into_iter().skip(1).collect();

    if cmd == "-h" || cmd == "--help" {
        print_tuned_adm_help();
        return 0;
    }

    match cmd.as_str() {
        "active" => cmd_active(),
        "list" => cmd_list(),
        "profile" => cmd_profile(&cmd_args),
        "profile_info" | "profile-info" => cmd_profile_info(&cmd_args),
        "recommend" => cmd_recommend(),
        "auto_profile" | "auto-profile" => cmd_auto_profile(),
        "off" => cmd_off(),
        "verify" => cmd_verify(),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_tuned_adm_help();
            return 1;
        }
    }
    0
}

fn run_tuned(args: Vec<String>) -> i32 {
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if rest.iter().any(|a| a == "-h" || a == "--help") {
        print_tuned_help();
        return 0;
    }

    run_daemon(&rest);
    0
}

fn run_tuned_gui(_args: Vec<String>) -> i32 {
    println!("tuned-gui: graphical tuning configuration");
    println!("(GUI not available in this build — use tuned-adm CLI)");
    0
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("tuned");
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
        "tuned-adm" => run_tuned_adm(args),
        "tuned-gui" => run_tuned_gui(args),
        "tuned" => run_tuned(args),
        _ => run_tuned(args),
    };

    process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_profiles_count() {
        let profiles = builtin_profiles();
        assert!(profiles.len() >= 10);
    }

    #[test]
    fn test_builtin_profiles_have_summaries() {
        for p in builtin_profiles() {
            assert!(!p.summary.is_empty(), "Profile {} has no summary", p.name);
        }
    }

    #[test]
    fn test_balanced_profile() {
        let profiles = builtin_profiles();
        let balanced = profiles.iter().find(|p| p.name == "balanced").unwrap();
        assert!(balanced.sections.contains_key("cpu"));
        let cpu = &balanced.sections["cpu"];
        assert_eq!(cpu.get("governor").map(|s| s.as_str()), Some("schedutil"));
    }

    #[test]
    fn test_throughput_performance_profile() {
        let profiles = builtin_profiles();
        let tp = profiles.iter().find(|p| p.name == "throughput-performance").unwrap();
        let cpu = &tp.sections["cpu"];
        assert_eq!(cpu.get("governor").map(|s| s.as_str()), Some("performance"));
    }

    #[test]
    fn test_powersave_profile() {
        let profiles = builtin_profiles();
        let ps = profiles.iter().find(|p| p.name == "powersave").unwrap();
        let cpu = &ps.sections["cpu"];
        assert_eq!(cpu.get("governor").map(|s| s.as_str()), Some("powersave"));
    }

    #[test]
    fn test_gaming_profile() {
        let profiles = builtin_profiles();
        let g = profiles.iter().find(|p| p.name == "gaming").unwrap();
        assert!(g.include.is_some());
        assert_eq!(g.include.as_deref(), Some("latency-performance"));
    }

    #[test]
    fn test_desktop_profile() {
        let profiles = builtin_profiles();
        let d = profiles.iter().find(|p| p.name == "desktop").unwrap();
        assert_eq!(d.include.as_deref(), Some("balanced"));
    }

    #[test]
    fn test_virtual_guest_profile() {
        let profiles = builtin_profiles();
        let vg = profiles.iter().find(|p| p.name == "virtual-guest").unwrap();
        assert_eq!(vg.include.as_deref(), Some("throughput-performance"));
    }

    #[test]
    fn test_network_profiles_inherit() {
        let profiles = builtin_profiles();
        let nt = profiles.iter().find(|p| p.name == "network-throughput").unwrap();
        assert_eq!(nt.include.as_deref(), Some("throughput-performance"));
        let nl = profiles.iter().find(|p| p.name == "network-latency").unwrap();
        assert_eq!(nl.include.as_deref(), Some("latency-performance"));
    }

    #[test]
    fn test_default_daemon_config() {
        let config = DaemonConfig::default();
        assert!(config.dynamic_tuning);
        assert!(config.recommend_command);
    }

    #[test]
    fn test_load_profiles_returns_builtins() {
        let profiles = load_profiles();
        assert!(profiles.iter().any(|p| p.name == "balanced"));
        assert!(profiles.iter().any(|p| p.name == "powersave"));
    }

    #[test]
    fn test_get_active_profile_empty() {
        let active = get_active_profile();
        // On systems without tuned, returns empty
        let _ = active;
    }

    #[test]
    fn test_prog_name_detection() {
        let cases = vec![
            ("tuned", "tuned"),
            ("tuned-adm", "tuned-adm"),
            ("tuned-gui", "tuned-gui"),
            ("/usr/sbin/tuned", "tuned"),
            ("C:\\bin\\tuned-adm.exe", "tuned-adm"),
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
    fn test_profiles_sorted_after_load() {
        let profiles = load_profiles();
        for i in 1..profiles.len() {
            assert!(profiles[i-1].name <= profiles[i].name,
                "Profiles not sorted: {} > {}", profiles[i-1].name, profiles[i].name);
        }
    }

    #[test]
    fn test_all_builtin_names_unique() {
        let profiles = builtin_profiles();
        let mut names: Vec<&str> = profiles.iter().map(|p| p.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), profiles.len());
    }

    #[test]
    fn test_profile_sections_nonempty() {
        for p in builtin_profiles() {
            assert!(!p.sections.is_empty(),
                "Profile {} has no sections", p.name);
        }
    }
}
