//! Slate OS IRQ balancing daemon.
//!
//! Multi-personality binary providing:
//! - **irqbalance** — distribute hardware interrupts across CPUs
//!
//! Monitors interrupt counts per CPU and migrates IRQs to achieve
//! balanced CPU utilization. Reads from /proc/interrupts and writes
//! to /proc/irq/<N>/smp_affinity.

#![deny(clippy::all)]
// IrqStat::affinity_mask is part of the /proc/irq/<N>/smp_affinity
// vocabulary the real irqbalance must speak when writing migration
// decisions back to the kernel. Dead-code lint cannot see across
// that future boundary.
#![allow(dead_code)]

use std::env;
use std::fs;
use std::io;
use std::process;
use std::thread;
use std::time::Duration;

const VERSION: &str = "0.1.0";

// ============================================================================
// Data structures
// ============================================================================

#[derive(Clone, Debug)]
struct IrqStat {
    irq: u32,
    _name: String,
    per_cpu: Vec<u64>,
    affinity_mask: u64,
}

#[derive(Clone, Debug)]
struct CpuLoad {
    cpu_id: u32,
    irq_count: u64,
}

struct BalanceOpts {
    oneshot: bool,
    debug: bool,
    foreground: bool,
    interval: u64,
    pid_file: String,
    banned_cpus: u64,
    banned_irqs: Vec<u32>,
    hint_policy: HintPolicy,
    power_thresh: Option<u32>,
    deep_idle: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum HintPolicy {
    Exact,
    Subset,
    Ignore,
}

// ============================================================================
// IRQ reading
// ============================================================================

fn read_irq_stats() -> Vec<IrqStat> {
    let content = match fs::read_to_string("/proc/interrupts") {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut lines = content.lines();
    let header = match lines.next() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let num_cpus = header.split_whitespace().count();
    let mut stats = Vec::new();

    for line in lines {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let irq_str = parts[0].trim_end_matches(':');
        let irq: u32 = match irq_str.parse() {
            Ok(n) => n,
            Err(_) => continue, // Skip named IRQs (NMI, LOC, etc.).
        };

        let mut per_cpu = Vec::new();
        for i in 1..=num_cpus {
            if i < parts.len() {
                per_cpu.push(parts[i].parse::<u64>().unwrap_or(0));
            }
        }

        let name_start = 1 + num_cpus;
        let name = if name_start < parts.len() {
            parts[name_start..].join(" ")
        } else {
            String::new()
        };

        let affinity = read_irq_affinity(irq).unwrap_or((1u64 << num_cpus) - 1);

        stats.push(IrqStat {
            irq,
            _name: name,
            per_cpu,
            affinity_mask: affinity,
        });
    }

    stats
}

fn read_irq_affinity(irq: u32) -> Option<u64> {
    let path = format!("/proc/irq/{irq}/smp_affinity");
    let content = fs::read_to_string(&path).ok()?;
    u64::from_str_radix(content.trim().replace(',', "").as_str(), 16).ok()
}

fn write_irq_affinity(irq: u32, mask: u64) -> io::Result<()> {
    let path = format!("/proc/irq/{irq}/smp_affinity");
    fs::write(&path, format!("{mask:x}\n"))
}

// ============================================================================
// Balancing algorithm
// ============================================================================

fn compute_cpu_loads(stats: &[IrqStat], num_cpus: usize) -> Vec<CpuLoad> {
    let mut loads: Vec<CpuLoad> = (0..num_cpus as u32)
        .map(|id| CpuLoad {
            cpu_id: id,
            irq_count: 0,
        })
        .collect();

    for stat in stats {
        for (cpu_idx, &count) in stat.per_cpu.iter().enumerate() {
            if cpu_idx < loads.len() {
                loads[cpu_idx].irq_count += count;
            }
        }
    }

    loads
}

fn find_least_loaded_cpu(loads: &[CpuLoad], banned_mask: u64) -> Option<u32> {
    loads
        .iter()
        .filter(|l| banned_mask & (1u64 << l.cpu_id) == 0)
        .min_by_key(|l| l.irq_count)
        .map(|l| l.cpu_id)
}

fn find_most_loaded_cpu(loads: &[CpuLoad], banned_mask: u64) -> Option<u32> {
    loads
        .iter()
        .filter(|l| banned_mask & (1u64 << l.cpu_id) == 0)
        .max_by_key(|l| l.irq_count)
        .map(|l| l.cpu_id)
}

/// Calculate load imbalance ratio.
fn load_imbalance(loads: &[CpuLoad]) -> f64 {
    if loads.is_empty() {
        return 0.0;
    }
    let max = loads.iter().map(|l| l.irq_count).max().unwrap_or(0);
    let min = loads.iter().map(|l| l.irq_count).min().unwrap_or(0);
    let avg = loads.iter().map(|l| l.irq_count).sum::<u64>() as f64 / loads.len() as f64;
    if avg < 1.0 {
        return 0.0;
    }
    (max - min) as f64 / avg
}

/// Find the busiest IRQ on a given CPU.
fn busiest_irq_on_cpu(stats: &[IrqStat], cpu: u32, banned_irqs: &[u32]) -> Option<u32> {
    stats
        .iter()
        .filter(|s| !banned_irqs.contains(&s.irq))
        .filter(|s| s.per_cpu.get(cpu as usize).copied().unwrap_or(0) > 0)
        .max_by_key(|s| s.per_cpu.get(cpu as usize).copied().unwrap_or(0))
        .map(|s| s.irq)
}

fn balance_once(opts: &BalanceOpts) -> Vec<(u32, u64)> {
    let stats = read_irq_stats();
    if stats.is_empty() {
        return Vec::new();
    }

    let num_cpus = stats.iter().map(|s| s.per_cpu.len()).max().unwrap_or(1);
    let loads = compute_cpu_loads(&stats, num_cpus);
    let imbalance = load_imbalance(&loads);

    let mut migrations = Vec::new();

    // Only rebalance if imbalance exceeds threshold (>50%).
    if imbalance < 0.5 {
        return migrations;
    }

    if opts.debug {
        eprintln!("irqbalance: imbalance ratio: {imbalance:.2}");
        for load in &loads {
            eprintln!("  CPU{}: {} interrupts", load.cpu_id, load.irq_count);
        }
    }

    // Simple strategy: move the busiest IRQ from the most loaded CPU
    // to the least loaded CPU.
    if let (Some(most), Some(least)) = (
        find_most_loaded_cpu(&loads, opts.banned_cpus),
        find_least_loaded_cpu(&loads, opts.banned_cpus),
    ) && most != least
        && let Some(irq) = busiest_irq_on_cpu(&stats, most, &opts.banned_irqs)
    {
        let new_mask = 1u64 << least;
        if let Err(e) = write_irq_affinity(irq, new_mask) {
            if opts.debug {
                eprintln!("irqbalance: failed to set affinity for IRQ {irq}: {e}");
            }
        } else if opts.debug {
            eprintln!("irqbalance: moved IRQ {irq} from CPU{most} to CPU{least}");
        }
        migrations.push((irq, new_mask));
    }

    migrations
}

// ============================================================================
// CLI
// ============================================================================

fn parse_args(args: &[String]) -> BalanceOpts {
    let mut opts = BalanceOpts {
        oneshot: false,
        debug: false,
        foreground: false,
        interval: 10,
        pid_file: "/var/run/irqbalance.pid".to_string(),
        banned_cpus: 0,
        banned_irqs: Vec::new(),
        hint_policy: HintPolicy::Ignore,
        power_thresh: None,
        deep_idle: false,
    };

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                println!("Usage: irqbalance [options]");
                println!();
                println!("Distribute hardware interrupts across CPUs.");
                println!();
                println!("Options:");
                println!("  -o, --oneshot        Run once and exit");
                println!("  -d, --debug          Debug mode (implies -f)");
                println!("  -f, --foreground     Run in foreground");
                println!("  -t, --interval SECS  Balance interval (default 10)");
                println!("  -p, --pid FILE       PID file path");
                println!("  --banirq=IRQ         Ban IRQ from balancing");
                println!("  --banmod=MOD         Ban module IRQs");
                println!("  --hintpolicy=POLICY  Affinity hint: exact, subset, ignore");
                println!("  --powerthresh=N      Power threshold");
                println!("  --deepidle           Enable deep idle");
                println!("  -h, --help           Show this help");
                println!("  -V, --version        Show version");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("irqbalance {VERSION}");
                process::exit(0);
            }
            "-o" | "--oneshot" => opts.oneshot = true,
            "-d" | "--debug" => {
                opts.debug = true;
                opts.foreground = true;
            }
            "-f" | "--foreground" => opts.foreground = true,
            "-t" | "--interval" => {
                i += 1;
                if i < args.len() {
                    opts.interval = args[i].parse().unwrap_or(10);
                }
            }
            "-p" | "--pid" => {
                i += 1;
                if i < args.len() {
                    opts.pid_file = args[i].clone();
                }
            }
            s if s.starts_with("--banirq=") => {
                if let Some(val) = s.strip_prefix("--banirq=")
                    && let Ok(irq) = val.parse::<u32>()
                {
                    opts.banned_irqs.push(irq);
                }
            }
            s if s.starts_with("--hintpolicy=") => {
                if let Some(val) = s.strip_prefix("--hintpolicy=") {
                    opts.hint_policy = match val {
                        "exact" => HintPolicy::Exact,
                        "subset" => HintPolicy::Subset,
                        _ => HintPolicy::Ignore,
                    };
                }
            }
            s if s.starts_with("--powerthresh=") => {
                if let Some(val) = s.strip_prefix("--powerthresh=") {
                    opts.power_thresh = val.parse().ok();
                }
            }
            "--deepidle" => opts.deep_idle = true,
            _ => {}
        }
        i += 1;
    }

    opts
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let rest: Vec<String> = args.into_iter().skip(1).collect();
    let opts = parse_args(&rest);

    if opts.oneshot {
        let migrations = balance_once(&opts);
        if opts.debug && migrations.is_empty() {
            eprintln!("irqbalance: no migrations needed");
        }
        return;
    }

    // Daemon loop.
    if opts.debug {
        eprintln!("irqbalance: starting with {}s interval", opts.interval);
    }

    // Write PID file.
    let _ = fs::write(&opts.pid_file, format!("{}\n", process::id()));

    loop {
        let migrations = balance_once(&opts);
        if opts.debug {
            eprintln!(
                "irqbalance: balance cycle complete, {} migrations",
                migrations.len()
            );
        }
        thread::sleep(Duration::from_secs(opts.interval));
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stat(irq: u32, per_cpu: Vec<u64>) -> IrqStat {
        IrqStat {
            irq,
            _name: format!("irq{irq}"),
            per_cpu,
            affinity_mask: 0xf,
        }
    }

    #[test]
    fn test_compute_cpu_loads() {
        let stats = vec![
            make_stat(0, vec![100, 0, 0, 0]),
            make_stat(1, vec![0, 50, 0, 0]),
            make_stat(2, vec![0, 0, 200, 0]),
        ];
        let loads = compute_cpu_loads(&stats, 4);
        assert_eq!(loads.len(), 4);
        assert_eq!(loads[0].irq_count, 100);
        assert_eq!(loads[1].irq_count, 50);
        assert_eq!(loads[2].irq_count, 200);
        assert_eq!(loads[3].irq_count, 0);
    }

    #[test]
    fn test_find_least_loaded() {
        let loads = vec![
            CpuLoad {
                cpu_id: 0,
                irq_count: 100,
            },
            CpuLoad {
                cpu_id: 1,
                irq_count: 50,
            },
            CpuLoad {
                cpu_id: 2,
                irq_count: 200,
            },
        ];
        assert_eq!(find_least_loaded_cpu(&loads, 0), Some(1));
    }

    #[test]
    fn test_find_most_loaded() {
        let loads = vec![
            CpuLoad {
                cpu_id: 0,
                irq_count: 100,
            },
            CpuLoad {
                cpu_id: 1,
                irq_count: 50,
            },
            CpuLoad {
                cpu_id: 2,
                irq_count: 200,
            },
        ];
        assert_eq!(find_most_loaded_cpu(&loads, 0), Some(2));
    }

    #[test]
    fn test_find_least_with_banned() {
        let loads = vec![
            CpuLoad {
                cpu_id: 0,
                irq_count: 100,
            },
            CpuLoad {
                cpu_id: 1,
                irq_count: 10,
            },
            CpuLoad {
                cpu_id: 2,
                irq_count: 50,
            },
        ];
        // Ban CPU 1.
        assert_eq!(find_least_loaded_cpu(&loads, 0b010), Some(2));
    }

    #[test]
    fn test_load_imbalance_balanced() {
        let loads = vec![
            CpuLoad {
                cpu_id: 0,
                irq_count: 100,
            },
            CpuLoad {
                cpu_id: 1,
                irq_count: 100,
            },
        ];
        assert!(load_imbalance(&loads) < 0.01);
    }

    #[test]
    fn test_load_imbalance_unbalanced() {
        let loads = vec![
            CpuLoad {
                cpu_id: 0,
                irq_count: 1000,
            },
            CpuLoad {
                cpu_id: 1,
                irq_count: 0,
            },
        ];
        assert!(load_imbalance(&loads) > 1.0);
    }

    #[test]
    fn test_load_imbalance_empty() {
        assert_eq!(load_imbalance(&[]), 0.0);
    }

    #[test]
    fn test_busiest_irq_on_cpu() {
        let stats = vec![
            make_stat(0, vec![100, 0]),
            make_stat(1, vec![50, 0]),
            make_stat(2, vec![200, 0]),
        ];
        assert_eq!(busiest_irq_on_cpu(&stats, 0, &[]), Some(2));
    }

    #[test]
    fn test_busiest_irq_banned() {
        let stats = vec![make_stat(0, vec![100, 0]), make_stat(1, vec![200, 0])];
        assert_eq!(busiest_irq_on_cpu(&stats, 0, &[1]), Some(0));
    }

    #[test]
    fn test_hint_policy() {
        assert_eq!(HintPolicy::Exact, HintPolicy::Exact);
        assert_ne!(HintPolicy::Subset, HintPolicy::Ignore);
    }

    #[test]
    fn test_parse_args_oneshot() {
        let args = vec!["-o".to_string()];
        let opts = parse_args(&args);
        assert!(opts.oneshot);
    }

    #[test]
    fn test_parse_args_debug() {
        let args = vec!["-d".to_string()];
        let opts = parse_args(&args);
        assert!(opts.debug);
        assert!(opts.foreground);
    }

    #[test]
    fn test_parse_args_interval() {
        let args = vec!["-t".to_string(), "5".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.interval, 5);
    }

    #[test]
    fn test_parse_args_ban_irq() {
        let args = vec!["--banirq=16".to_string(), "--banirq=17".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.banned_irqs, vec![16, 17]);
    }

    #[test]
    fn test_parse_args_hint_policy() {
        let args = vec!["--hintpolicy=exact".to_string()];
        let opts = parse_args(&args);
        assert_eq!(opts.hint_policy, HintPolicy::Exact);
    }

    #[test]
    fn test_read_irq_stats_no_crash() {
        let _ = read_irq_stats();
    }
}
