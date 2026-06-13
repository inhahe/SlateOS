//! SlateOS Interactive Process Monitor
//!
//! Real-time display of system processes, CPU, and memory usage.
//! Similar to Linux `top` or `htop` — reads from /proc for live data.
//!
//! # Usage
//!
//! ```text
//! top                   Interactive mode (updates every 2 seconds)
//! top -d <secs>         Set refresh interval
//! top -n <count>        Run N iterations then exit (batch mode)
//! top -b                Batch mode (no terminal control codes between runs)
//! top -p <pid>          Monitor specific PID(s) only
//! top -s <field>        Sort by field: cpu, mem, pid, name, time (default: cpu)
//! top -r                Reverse sort order
//! top --once            Show one snapshot and exit
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Data structures
// ============================================================================

/// Per-process information scraped from /proc/<pid>/.
#[derive(Clone)]
struct ProcessInfo {
    pid: u32,
    name: String,
    state: char,
    #[allow(dead_code)] // Available for future parent-child tree view.
    ppid: u32,
    /// CPU time in ticks (utime + stime).
    cpu_ticks: u64,
    /// Resident set size in KiB.
    rss_kb: u64,
    /// Virtual memory size in KiB.
    vsize_kb: u64,
    /// Number of threads.
    threads: u32,
    /// Nice value.
    nice: i32,
    /// Priority.
    priority: i32,
    /// User who owns the process (UID).
    uid: u32,
    /// CPU usage percentage (computed between snapshots).
    cpu_pct: f64,
    /// Memory usage percentage.
    mem_pct: f64,
    /// Total CPU time as formatted string.
    time_str: String,
}

/// System-wide summary.
struct SystemSummary {
    uptime_secs: u64,
    load_1: String,
    load_5: String,
    load_15: String,
    total_tasks: u32,
    running: u32,
    sleeping: u32,
    stopped: u32,
    zombie: u32,
    mem_total_kb: u64,
    mem_free_kb: u64,
    mem_available_kb: u64,
    mem_buffers_kb: u64,
    mem_cached_kb: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
    cpu_user: u64,
    cpu_nice: u64,
    cpu_system: u64,
    cpu_idle: u64,
    cpu_iowait: u64,
    cpu_irq: u64,
    cpu_softirq: u64,
}

#[derive(Clone, Copy, PartialEq)]
enum SortField {
    Pid,
    Cpu,
    Mem,
    Name,
    Time,
    Rss,
    Vsize,
    Threads,
}

struct Config {
    delay_secs: u64,
    iterations: Option<u32>,
    batch_mode: bool,
    filter_pids: Vec<u32>,
    sort_field: SortField,
    reverse: bool,
    once: bool,
}

// ============================================================================
// /proc readers
// ============================================================================

fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

fn parse_kb_value(s: &str) -> u64 {
    let s = s.trim()
        .trim_end_matches(" kB")
        .trim_end_matches(" KB")
        .trim();
    s.parse().unwrap_or(0)
}

fn get_meminfo_value(content: &str, key: &str) -> u64 {
    for line in content.lines() {
        if let Some((k, v)) = line.split_once(':')
            && k.trim() == key {
                return parse_kb_value(v);
            }
    }
    0
}

/// Read system-wide summary from /proc.
fn read_system_summary() -> SystemSummary {
    let mut summary = SystemSummary {
        uptime_secs: 0,
        load_1: String::new(),
        load_5: String::new(),
        load_15: String::new(),
        total_tasks: 0,
        running: 0,
        sleeping: 0,
        stopped: 0,
        zombie: 0,
        mem_total_kb: 0,
        mem_free_kb: 0,
        mem_available_kb: 0,
        mem_buffers_kb: 0,
        mem_cached_kb: 0,
        swap_total_kb: 0,
        swap_free_kb: 0,
        cpu_user: 0,
        cpu_nice: 0,
        cpu_system: 0,
        cpu_idle: 0,
        cpu_iowait: 0,
        cpu_irq: 0,
        cpu_softirq: 0,
    };

    // Uptime.
    if let Some(uptime) = read_file("/proc/uptime")
        && let Some(secs_str) = uptime.split_whitespace().next()
            && let Ok(secs) = secs_str.parse::<f64>() {
                summary.uptime_secs = secs as u64;
            }

    // Load average.
    if let Some(loadavg) = read_file("/proc/loadavg") {
        let parts: Vec<&str> = loadavg.split_whitespace().collect();
        if parts.len() >= 3 {
            summary.load_1 = parts[0].to_string();
            summary.load_5 = parts[1].to_string();
            summary.load_15 = parts[2].to_string();
        }
    }

    // Memory info.
    if let Some(meminfo) = read_file("/proc/meminfo") {
        summary.mem_total_kb = get_meminfo_value(&meminfo, "MemTotal");
        summary.mem_free_kb = get_meminfo_value(&meminfo, "MemFree");
        summary.mem_available_kb = get_meminfo_value(&meminfo, "MemAvailable");
        summary.mem_buffers_kb = get_meminfo_value(&meminfo, "Buffers");
        summary.mem_cached_kb = get_meminfo_value(&meminfo, "Cached");
        summary.swap_total_kb = get_meminfo_value(&meminfo, "SwapTotal");
        summary.swap_free_kb = get_meminfo_value(&meminfo, "SwapFree");
    }

    // CPU stats from /proc/stat.
    if let Some(stat) = read_file("/proc/stat") {
        for line in stat.lines() {
            if let Some(rest) = line.strip_prefix("cpu ") {
                let vals: Vec<u64> = rest.split_whitespace()
                    .filter_map(|s| s.parse().ok())
                    .collect();
                if vals.len() >= 7 {
                    summary.cpu_user = vals[0];
                    summary.cpu_nice = vals[1];
                    summary.cpu_system = vals[2];
                    summary.cpu_idle = vals[3];
                    summary.cpu_iowait = vals.get(4).copied().unwrap_or(0);
                    summary.cpu_irq = vals.get(5).copied().unwrap_or(0);
                    summary.cpu_softirq = vals.get(6).copied().unwrap_or(0);
                }
                break;
            }
        }
    }

    summary
}

/// Read information about a single process from /proc/<pid>/.
fn read_process(pid: u32, mem_total_kb: u64) -> Option<ProcessInfo> {
    let stat_path = format!("/proc/{pid}/stat");
    let stat_content = read_file(&stat_path)?;

    // /proc/<pid>/stat format:
    // pid (comm) state ppid pgrp session tty_nr tpgid flags
    // minflt cminflt majflt cmajflt utime stime cutime cstime
    // priority nice num_threads itrealvalue starttime vsize rss ...
    //
    // comm can contain spaces and parentheses, so find the last ')'.
    let comm_start = stat_content.find('(')?;
    let comm_end = stat_content.rfind(')')?;
    let name = stat_content[comm_start + 1..comm_end].to_string();
    let rest = &stat_content[comm_end + 2..]; // skip ") "
    let fields: Vec<&str> = rest.split_whitespace().collect();

    if fields.len() < 20 {
        return None;
    }

    let state = fields[0].chars().next().unwrap_or('?');
    let ppid: u32 = fields[1].parse().unwrap_or(0);
    let utime: u64 = fields[11].parse().unwrap_or(0);
    let stime: u64 = fields[12].parse().unwrap_or(0);
    let priority: i32 = fields[15].parse().unwrap_or(0);
    let nice: i32 = fields[16].parse().unwrap_or(0);
    let threads: u32 = fields[17].parse().unwrap_or(1);
    let vsize_bytes: u64 = fields[20].parse().unwrap_or(0);
    let rss_pages: u64 = fields[21].parse().unwrap_or(0);

    // Our OS uses 16 KiB pages.
    let rss_kb = rss_pages * 16;
    let vsize_kb = vsize_bytes / 1024;
    let cpu_ticks = utime + stime;

    let mem_pct = if mem_total_kb > 0 {
        (rss_kb as f64 / mem_total_kb as f64) * 100.0
    } else {
        0.0
    };

    // Format CPU time as HH:MM:SS (assuming 100 ticks per second).
    let total_secs = cpu_ticks / 100;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let time_str = format!("{hours:02}:{mins:02}:{secs:02}");

    // Read UID from /proc/<pid>/status.
    let uid = read_file(&format!("/proc/{pid}/status"))
        .and_then(|content| {
            for line in content.lines() {
                if let Some(val) = line.strip_prefix("Uid:") {
                    return val.split_whitespace()
                        .next()
                        .and_then(|s| s.parse().ok());
                }
            }
            None
        })
        .unwrap_or(0);

    Some(ProcessInfo {
        pid,
        name,
        state,
        ppid,
        cpu_ticks,
        rss_kb,
        vsize_kb,
        threads,
        nice,
        priority,
        uid,
        cpu_pct: 0.0, // Computed later from delta.
        mem_pct,
        time_str,
    })
}

/// Enumerate all processes from /proc.
fn read_all_processes(mem_total_kb: u64) -> Vec<ProcessInfo> {
    let mut procs = Vec::new();

    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(pid) = name.parse::<u32>()
                    && let Some(info) = read_process(pid, mem_total_kb) {
                        procs.push(info);
                    }
        }
    }

    procs
}

/// Compute CPU usage percentages by comparing two snapshots.
fn compute_cpu_usage(
    current: &mut [ProcessInfo],
    prev: &[(u32, u64)],
    total_cpu_delta: u64,
) {
    for proc in current.iter_mut() {
        let prev_ticks = prev.iter()
            .find(|(pid, _)| *pid == proc.pid)
            .map(|(_, t)| *t)
            .unwrap_or(0);

        let delta = proc.cpu_ticks.saturating_sub(prev_ticks);

        proc.cpu_pct = if total_cpu_delta > 0 {
            (delta as f64 / total_cpu_delta as f64) * 100.0
        } else {
            0.0
        };
    }
}

// ============================================================================
// Display
// ============================================================================

/// Format KiB value as a human-readable string.
fn format_kb(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1}G", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.1}M", kb as f64 / 1024.0)
    } else {
        format!("{kb}K")
    }
}

/// Format uptime as "up X days, HH:MM".
fn format_uptime(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;

    if days > 0 {
        format!("up {days} day{}, {hours:02}:{mins:02}",
            if days == 1 { "" } else { "s" })
    } else {
        format!("up {hours:02}:{mins:02}")
    }
}

#[allow(dead_code)] // Available for verbose display mode.
fn state_name(c: char) -> &'static str {
    match c {
        'R' => "running",
        'S' => "sleeping",
        'D' => "disk sleep",
        'Z' => "zombie",
        'T' => "stopped",
        't' => "tracing",
        'X' => "dead",
        'I' => "idle",
        _ => "unknown",
    }
}

/// Print the system summary header.
fn print_header(summary: &SystemSummary) {
    // Line 1: uptime and load.
    println!(
        "top - {} load average: {}, {}, {}",
        format_uptime(summary.uptime_secs),
        summary.load_1,
        summary.load_5,
        summary.load_15
    );

    // Line 2: task counts.
    println!(
        "Tasks: {} total, {} running, {} sleeping, {} stopped, {} zombie",
        summary.total_tasks,
        summary.running,
        summary.sleeping,
        summary.stopped,
        summary.zombie
    );

    // Line 3: CPU usage.
    let total = summary.cpu_user + summary.cpu_nice + summary.cpu_system
        + summary.cpu_idle + summary.cpu_iowait + summary.cpu_irq
        + summary.cpu_softirq;
    let total_f = if total > 0 { total as f64 } else { 1.0 };

    println!(
        "%%Cpu(s): {:.1} us, {:.1} sy, {:.1} ni, {:.1} id, {:.1} wa, {:.1} hi, {:.1} si",
        summary.cpu_user as f64 / total_f * 100.0,
        summary.cpu_system as f64 / total_f * 100.0,
        summary.cpu_nice as f64 / total_f * 100.0,
        summary.cpu_idle as f64 / total_f * 100.0,
        summary.cpu_iowait as f64 / total_f * 100.0,
        summary.cpu_irq as f64 / total_f * 100.0,
        summary.cpu_softirq as f64 / total_f * 100.0,
    );

    // Line 4: memory.
    let mem_used = summary.mem_total_kb
        .saturating_sub(summary.mem_free_kb)
        .saturating_sub(summary.mem_buffers_kb)
        .saturating_sub(summary.mem_cached_kb);
    println!(
        "MiB Mem:  {} total, {} free, {} used, {} buff/cache",
        format_kb(summary.mem_total_kb),
        format_kb(summary.mem_free_kb),
        format_kb(mem_used),
        format_kb(summary.mem_buffers_kb + summary.mem_cached_kb),
    );

    // Line 5: swap.
    let swap_used = summary.swap_total_kb.saturating_sub(summary.swap_free_kb);
    println!(
        "MiB Swap: {} total, {} free, {} used, {} avail Mem",
        format_kb(summary.swap_total_kb),
        format_kb(summary.swap_free_kb),
        format_kb(swap_used),
        format_kb(summary.mem_available_kb),
    );
}

/// Print the process table header.
fn print_table_header() {
    println!();
    println!(
        "{:>7} {:>4} {:>3} {:>4} {:>8} {:>8} {:<1} {:>5} {:>5} {:>9} {:<16}",
        "PID", "UID", "PR", "NI", "VIRT", "RES", "S", "%CPU", "%MEM", "TIME+", "COMMAND"
    );
}

/// Print a single process row.
fn print_process(p: &ProcessInfo) {
    println!(
        "{:>7} {:>4} {:>3} {:>4} {:>8} {:>8} {:<1} {:>5.1} {:>5.1} {:>9} {:<16}",
        p.pid,
        p.uid,
        p.priority,
        p.nice,
        format_kb(p.vsize_kb),
        format_kb(p.rss_kb),
        p.state,
        p.cpu_pct,
        p.mem_pct,
        p.time_str,
        if p.name.len() > 16 { &p.name[..16] } else { &p.name },
    );
}

/// Sort processes by the selected field.
fn sort_processes(procs: &mut [ProcessInfo], field: SortField, reverse: bool) {
    procs.sort_by(|a, b| {
        let cmp = match field {
            SortField::Pid => a.pid.cmp(&b.pid),
            SortField::Cpu => a.cpu_pct.partial_cmp(&b.cpu_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortField::Mem => a.mem_pct.partial_cmp(&b.mem_pct)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortField::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortField::Time => a.cpu_ticks.cmp(&b.cpu_ticks),
            SortField::Rss => a.rss_kb.cmp(&b.rss_kb),
            SortField::Vsize => a.vsize_kb.cmp(&b.vsize_kb),
            SortField::Threads => a.threads.cmp(&b.threads),
        };

        // Default: descending for numeric fields, ascending for name.
        match field {
            SortField::Name | SortField::Pid => {
                if reverse { cmp.reverse() } else { cmp }
            }
            _ => {
                if reverse { cmp } else { cmp.reverse() }
            }
        }
    });
}

/// Count processes in each state.
fn count_states(procs: &[ProcessInfo]) -> (u32, u32, u32, u32) {
    let mut running = 0u32;
    let mut sleeping = 0u32;
    let mut stopped = 0u32;
    let mut zombie = 0u32;

    for p in procs {
        match p.state {
            'R' => running += 1,
            'S' | 'D' | 'I' => sleeping += 1,
            'T' | 't' => stopped += 1,
            'Z' => zombie += 1,
            _ => {}
        }
    }

    (running, sleeping, stopped, zombie)
}

/// Run one display cycle.
fn display_snapshot(
    procs: &mut Vec<ProcessInfo>,
    summary: &mut SystemSummary,
    config: &Config,
    prev_ticks: &[(u32, u64)],
    prev_cpu_total: u64,
    max_lines: usize,
) {
    // Count states.
    let (running, sleeping, stopped, zombie) = count_states(procs);
    summary.total_tasks = procs.len() as u32;
    summary.running = running;
    summary.sleeping = sleeping;
    summary.stopped = stopped;
    summary.zombie = zombie;

    // Compute CPU delta.
    let current_cpu_total = summary.cpu_user + summary.cpu_nice + summary.cpu_system
        + summary.cpu_idle + summary.cpu_iowait + summary.cpu_irq
        + summary.cpu_softirq;
    let cpu_delta = current_cpu_total.saturating_sub(prev_cpu_total);

    // Compute per-process CPU usage.
    compute_cpu_usage(procs, prev_ticks, cpu_delta);

    // Filter by PIDs if requested.
    if !config.filter_pids.is_empty() {
        procs.retain(|p| config.filter_pids.contains(&p.pid));
    }

    // Sort.
    sort_processes(procs, config.sort_field, config.reverse);

    // Clear screen (unless batch mode).
    if !config.batch_mode {
        // ANSI: clear screen and move cursor to top-left.
        print!("\x1b[2J\x1b[H");
    }

    // Print header.
    print_header(summary);
    print_table_header();

    // Print processes (limited to terminal height minus header lines).
    let display_count = if max_lines > 7 { max_lines - 7 } else { procs.len() };
    for proc in procs.iter().take(display_count) {
        print_process(proc);
    }
}

// ============================================================================
// Main loop
// ============================================================================

fn run(config: &Config) {
    let mut iteration = 0u32;
    let mut prev_ticks: Vec<(u32, u64)> = Vec::new();
    let mut prev_cpu_total: u64 = 0;

    // Attempt to read terminal height (default to 40 if unavailable).
    let term_height = read_file("/sys/tty/rows")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(40);

    loop {
        let mut summary = read_system_summary();
        let mut procs = read_all_processes(summary.mem_total_kb);

        display_snapshot(
            &mut procs,
            &mut summary,
            config,
            &prev_ticks,
            prev_cpu_total,
            term_height,
        );

        // Save current ticks for next delta computation.
        prev_ticks = procs.iter().map(|p| (p.pid, p.cpu_ticks)).collect();
        prev_cpu_total = summary.cpu_user + summary.cpu_nice + summary.cpu_system
            + summary.cpu_idle + summary.cpu_iowait + summary.cpu_irq
            + summary.cpu_softirq;

        iteration += 1;

        // Check iteration limit.
        if config.once {
            break;
        }
        if let Some(max) = config.iterations
            && iteration >= max {
                break;
            }

        // Sleep until next refresh.
        // Use a simple busy-wait with /proc/uptime polling if sleep syscall
        // isn't available, or std::thread::sleep.
        std::thread::sleep(std::time::Duration::from_secs(config.delay_secs));
    }
}

// ============================================================================
// CLI parsing
// ============================================================================

fn parse_sort_field(s: &str) -> Option<SortField> {
    match s.to_lowercase().as_str() {
        "cpu" | "%cpu" => Some(SortField::Cpu),
        "mem" | "%mem" | "memory" => Some(SortField::Mem),
        "pid" => Some(SortField::Pid),
        "name" | "command" | "cmd" => Some(SortField::Name),
        "time" | "time+" => Some(SortField::Time),
        "res" | "rss" => Some(SortField::Rss),
        "virt" | "vsize" | "vsz" => Some(SortField::Vsize),
        "threads" | "thr" => Some(SortField::Threads),
        _ => None,
    }
}

fn print_usage() {
    println!("Slate OS Process Monitor v0.1.0");
    println!();
    println!("Real-time display of running processes, CPU, and memory usage.");
    println!();
    println!("USAGE:");
    println!("  top [options]");
    println!();
    println!("OPTIONS:");
    println!("  -d <secs>      Refresh delay in seconds (default: 2)");
    println!("  -n <count>     Number of iterations, then exit");
    println!("  -b             Batch mode (no screen clearing)");
    println!("  -p <pid>       Monitor specific PIDs (comma-separated or repeat -p)");
    println!("  -s <field>     Sort field: cpu, mem, pid, name, time, res, virt, threads");
    println!("  -r             Reverse sort order");
    println!("  --once         Show one snapshot and exit");
    println!("  --help, -h     Show this help");
    println!();
    println!("SORT FIELDS:");
    println!("  cpu      CPU usage percentage (default)");
    println!("  mem      Memory usage percentage");
    println!("  pid      Process ID");
    println!("  name     Process name");
    println!("  time     Cumulative CPU time");
    println!("  res      Resident memory (RSS)");
    println!("  virt     Virtual memory size");
    println!("  threads  Thread count");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        delay_secs: 2,
        iterations: None,
        batch_mode: false,
        filter_pids: Vec::new(),
        sort_field: SortField::Cpu,
        reverse: false,
        once: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "-d" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -d requires a value");
                    process::exit(1);
                }
                config.delay_secs = args[i + 1].parse().unwrap_or(2);
                if config.delay_secs == 0 {
                    config.delay_secs = 1;
                }
                i += 2;
            }
            "-n" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -n requires a value");
                    process::exit(1);
                }
                config.iterations = Some(args[i + 1].parse().unwrap_or(1));
                i += 2;
            }
            "-b" => {
                config.batch_mode = true;
                i += 1;
            }
            "-p" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -p requires a PID value");
                    process::exit(1);
                }
                // Support comma-separated PIDs.
                for pid_str in args[i + 1].split(',') {
                    if let Ok(pid) = pid_str.trim().parse::<u32>() {
                        config.filter_pids.push(pid);
                    }
                }
                i += 2;
            }
            "-s" => {
                if i + 1 >= args.len() {
                    eprintln!("error: -s requires a field name");
                    process::exit(1);
                }
                config.sort_field = match parse_sort_field(&args[i + 1]) {
                    Some(f) => f,
                    None => {
                        eprintln!("error: unknown sort field: {}", args[i + 1]);
                        eprintln!("Valid: cpu, mem, pid, name, time, res, virt, threads");
                        process::exit(1);
                    }
                };
                i += 2;
            }
            "-r" => {
                config.reverse = true;
                i += 1;
            }
            "--once" => {
                config.once = true;
                i += 1;
            }
            "--help" | "-h" | "help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                eprintln!("unknown option: {other}");
                eprintln!("Run 'top --help' for usage.");
                process::exit(1);
            }
        }
    }

    // Batch mode with once: just print once.
    if config.once {
        config.batch_mode = true;
        config.iterations = Some(1);
    }

    run(&config);
}
