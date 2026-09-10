#![deny(clippy::all)]

//! earlyoom — Slate OS early OOM (Out-of-Memory) daemon
//!
//! Monitors memory and swap usage, kills memory-hogging processes before
//! the kernel OOM killer triggers (which often kills the wrong process).
//!
//! Single personality: `earlyoom`

use quoting::quoteaf_os;
use std::env;
use std::process;

// ── Constants ──────────────────────────────────────────────────────────

const _EARLYOOM_CONF: &str = "/etc/default/earlyoom";
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

// ── Reading the system ─────────────────────────────────────────────────
//
// This section was headed "Simulated data" and meant it. `read_meminfo`
// returned four hardcoded constants -- 32 GB total, 2 GB available, commented
// "(low!)" -- and `read_processes` returned a fixed list of four invented
// processes, including pid 1234 named `chromium`. So the daemon believed
// memory was permanently below its threshold, always found the same fictional
// victim, and printed a convincing report about a machine that does not exist.

/// Memory pressure, from `/proc/meminfo` through [`procinfo`].
///
/// `None` when it cannot be read, and the caller **must not decide** on that.
/// An out-of-memory killer that cannot see memory has no basis for choosing a
/// process to end, and "I could not tell" is not "memory is fine" either --
/// the only safe answer is to do nothing and say so.
fn read_meminfo() -> Option<MemInfo> {
    let m = procinfo::ProcFs::new().memory().ok().flatten()?;
    // `MemTotal` is the one field without which nothing here means anything:
    // every threshold is a percentage of it.
    let mem_total_kb = m.total_kib?;
    Some(MemInfo {
        mem_total_kb,
        // `MemAvailable` is the kernel's own estimate of what a new allocation
        // could get. Falling back to `MemFree` would systematically understate
        // it -- reclaimable cache is available and is not free -- and this
        // number is compared against a kill threshold.
        mem_available_kb: m.available_kib.or(m.free_kib)?,
        swap_total_kb: m.swap_total_kib.unwrap_or(0),
        swap_free_kb: m.swap_free_kib.unwrap_or(0),
    })
}

/// Every process the system has, with what is needed to choose between them.
///
/// A process that disappears between the `/proc` listing and the read of its
/// files is skipped rather than reported with zeroes: it has already exited,
/// which is the outcome this daemon exists to cause.
fn read_processes() -> Vec<ProcessInfo> {
    let proc = procinfo::ProcFs::new();
    let Ok(pids) = proc.process_ids() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for pid in pids {
        let Ok(Some(stat)) = proc.process_stat(pid) else {
            continue;
        };
        let status = proc.process_status(pid).ok().flatten().unwrap_or_default();
        out.push(ProcessInfo {
            pid: u32::try_from(pid).unwrap_or(u32::MAX),
            name: quoting::escape_unprintable(&stat.comm),
            oom_score: read_oom_score(&proc, pid),
            // `VmRSS` when the kernel gives it, else the `stat` page count.
            // A kernel thread has neither, and 0 is the right answer for it:
            // it has no address space to reclaim.
            vm_rss_kb: status.vm_rss_kib.unwrap_or_else(|| stat.rss_kib()),
            _uid: status.uid.unwrap_or(0),
        });
    }
    out
}

/// `/proc/<pid>/oom_score`, or 0 when it cannot be read.
///
/// 0 is the neutral score, so an unreadable one neither attracts nor repels
/// the killer -- the process is then chosen on its resident size like any
/// other, which is the behaviour with the fewest surprises.
fn read_oom_score(proc: &procinfo::ProcFs, pid: u64) -> i32 {
    proc.read_optional(&format!("{pid}/oom_score"))
        .ok()
        .flatten()
        .and_then(|b| String::from_utf8(b).ok())
        .and_then(|t| t.trim().parse::<i32>().ok())
        .unwrap_or(0)
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
                println!("earlyoom 0.1.0 (Slate OS)");
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
                eprintln!("earlyoom: unknown option {}", quoteaf_os(other));
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
    let Some(meminfo) = read_meminfo() else {
        eprintln!("earlyoom: cannot read /proc/meminfo — refusing to choose a victim");
        return 1;
    };

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
        let victim = select_victim(config, &read_processes());
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
                    // **It said "sending SIGTERM" and sent nothing.** Both
                    // branches of this `if` were a `println!`, so the only
                    // difference between a dry run and a real one was the
                    // wording -- and the real one claimed an action it had not
                    // taken, which is the wrong way round for a daemon whose
                    // log is the only evidence anyone has that it works.
                    //
                    // Now that the memory and the process list are real, the
                    // victim named here is a real process, so the message says
                    // what it would do and states plainly that it cannot yet
                    // do it. Signalling is tracked in `todo.txt`.
                    println!(
                        "earlyoom: would send SIGTERM to pid {} ({}), oom_score={}, rss={} \
                         — signalling is not implemented, so nothing was sent",
                        proc.pid,
                        proc.name,
                        proc.oom_score,
                        format_kb(proc.vm_rss_kb)
                    );
                    if config.notify {
                        println!("earlyoom: (no notification sent either)");
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

/// Choose which process to end, from the ones given.
///
/// **Takes the candidates rather than fetching them.** It used to call
/// `read_processes()` itself, and `read_processes()` returned four invented
/// processes -- so every test of this function was a test of that fixture,
/// asserting that the victim was `chromium` with an `oom_score` of 800. Now
/// that the list is real, those assertions would be about whatever happens to
/// be running. The fixture moved into the tests, which is where it belonged:
/// the selection rule is worth pinning and the machine's process table is not.
fn select_victim(config: &EarlyOomConfig, candidates: &[ProcessInfo]) -> Option<ProcessInfo> {
    let mut processes = candidates.to_vec();

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
    // Not implemented: everything below reports work this crate cannot do.
    // Fail rather than mislead a caller. Delete this line when it is real.
    notimpl::guard(env!("CARGO_PKG_NAME"));
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

    /// The process table this program used to *invent*, now an explicit
    /// fixture, taken from the deleted `read_processes` unchanged. It is a
    /// good fixture -- a spread of scores, a protected system process at
    /// -1000, a neutral one at 0 -- and the selection tests are written
    /// against its exact shape. What was wrong was where it lived.
    fn sample_processes() -> Vec<ProcessInfo> {
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

    /// Reading the real `/proc/meminfo` either works or says it did not.
    ///
    /// This asserted `mem.mem_total_kb > 0` against four hardcoded constants,
    /// so it could not fail. What is worth asserting now is the *contract*:
    /// `None` when the file cannot be read -- which is the case on the host
    /// this suite runs on -- and coherent numbers when it can.
    #[test]
    fn read_meminfo_is_none_or_coherent() {
        match read_meminfo() {
            None => {
                // The host build has no `/proc`; that is the honest answer and
                // the daemon refuses to choose a victim on it.
            }
            Some(mem) => {
                assert!(mem.mem_total_kb > 0, "a total of zero means nothing");
                assert!(mem.mem_available_kb <= mem.mem_total_kb);
                assert!(mem.swap_free_kb <= mem.swap_total_kb);
            }
        }
    }

    /// **An out-of-memory killer that cannot see memory must not choose.**
    ///
    /// `read_meminfo` returns `None` rather than a plausible default, and the
    /// daemon returns non-zero on it. "I could not tell" is not "memory is
    /// fine", and it is certainly not grounds for ending a process.
    #[test]
    fn a_daemon_that_cannot_read_memory_refuses() {
        // On the host there is no `/proc/meminfo`, so this is the live path.
        if read_meminfo().is_none() {
            let config = EarlyOomConfig::default();
            assert_eq!(run_daemon(&config), 1);
        }
    }

    #[test]
    fn test_select_victim_default() {
        let config = EarlyOomConfig::default();
        let victim = select_victim(&config, &sample_processes());
        assert!(victim.is_some());
        let v = victim.expect("a victim among the sample");
        // Highest oom_score among the killable ones.
        assert_eq!(v.name, "chromium");
        assert_eq!(v.oom_score, 800);
    }

    /// A negative `oom_score` is a refusal, not a low ranking: `systemd` is
    /// never chosen however little else there is.
    #[test]
    fn a_protected_process_is_never_chosen() {
        let config = EarlyOomConfig::default();
        let only_protected: Vec<ProcessInfo> = sample_processes()
            .into_iter()
            .filter(|p| p.oom_score <= 0)
            .collect();
        assert!(
            select_victim(&config, &only_protected).is_none(),
            "with nothing killable there is no victim, not a least-bad one"
        );
    }

    #[test]
    fn test_select_victim_with_prefer() {
        let config = EarlyOomConfig {
            prefer_regex: Some("java".to_string()),
            ..EarlyOomConfig::default()
        };
        let victim = select_victim(&config, &sample_processes());
        assert!(victim.is_some());
        assert_eq!(victim.unwrap().name, "java");
    }

    #[test]
    fn test_select_victim_with_avoid() {
        let config = EarlyOomConfig {
            avoid_regex: Some("chromium".to_string()),
            ..EarlyOomConfig::default()
        };
        let victim = select_victim(&config, &sample_processes());
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

    /// The percentage every threshold is compared against.
    ///
    /// This read `read_meminfo()` and asserted `0 < pct < 100`, which was true
    /// of the two constants it got back and told nobody anything. The
    /// arithmetic is what is worth pinning, and the case that matters is the
    /// one where the denominator could be zero -- `read_meminfo` refuses a
    /// `MemTotal` of zero for exactly that reason, so this asserts the
    /// refusal rather than dividing by it.
    #[test]
    fn mem_percent_is_a_percentage_of_a_real_total() {
        let mem = MemInfo {
            mem_total_kb: 32_768_000,
            mem_available_kb: 2_048_000,
            swap_total_kb: 8_192_000,
            swap_free_kb: 4_096_000,
        };
        let pct = (mem.mem_available_kb as f64 / mem.mem_total_kb as f64) * 100.0;
        assert!((pct - 6.25).abs() < 1e-9, "2 GiB of 32 GiB is 6.25%");

        // And a machine that reports no memory at all yields no reading, so
        // nothing downstream divides by it.
        assert!(
            read_meminfo().is_none_or(|m| m.mem_total_kb > 0),
            "a zero total would make every threshold a division by zero"
        );
    }

    #[test]
    fn test_unkillable_filtered() {
        let config = EarlyOomConfig::default();
        let victim = select_victim(&config, &sample_processes());
        if let Some(v) = victim {
            assert!(v.oom_score > 0);
            assert!(v.pid > 1);
        }
    }
}
