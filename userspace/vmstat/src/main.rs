//! OurOS Virtual Memory Statistics Utility
//!
//! Reports virtual memory, CPU, and I/O system statistics by reading
//! `/proc/stat`, `/proc/meminfo`, `/proc/vmstat`, and `/proc/diskstats`.
//! Similar to Linux `vmstat`.
//!
//! # Usage
//!
//! ```text
//! vmstat                   One-shot report (averages since boot)
//! vmstat 2                 Report every 2 seconds
//! vmstat 2 10              Report every 2 seconds, 10 times
//! vmstat -a                Show active/inactive memory instead of buff/cache
//! vmstat -f                Show total forks since boot
//! vmstat -s                Show event counters and memory stats table
//! vmstat -d                Show disk statistics
//! vmstat -D                Show disk summary
//! vmstat -m                Show slab/heap allocator info
//! vmstat -w                Wide output columns
//! vmstat -t                Add timestamp column
//! vmstat -n                Display header only once
//! vmstat -S <unit>         Units: k=1024, K=1000, m=1048576, M=1000000
//! vmstat --json            JSON output
//! vmstat --help            Show help
//! ```

use std::env;
use std::fs;
use std::process;
use std::time::Duration;

// ============================================================================
// Constants
// ============================================================================

/// Default column widths for narrow output.
const NARROW_WIDTH: usize = 6;

/// Column widths for wide output.
const WIDE_WIDTH: usize = 10;

// ============================================================================
// Data structures
// ============================================================================

/// Display unit configuration.
#[derive(Clone, Copy, PartialEq)]
enum DisplayUnit {
    /// 1024 bytes per unit (default).
    K1024,
    /// 1000 bytes per unit.
    K1000,
    /// 1048576 bytes per unit.
    M1024,
    /// 1000000 bytes per unit.
    M1000,
}

/// Which output mode the user requested.
#[derive(Clone, Copy, PartialEq)]
enum OutputMode {
    /// Default vmstat table (procs/memory/swap/io/system/cpu).
    Default,
    /// `-f`: just print forks count.
    Forks,
    /// `-s`: event counters and memory stats table.
    Stats,
    /// `-d`: per-disk statistics.
    Disk,
    /// `-D`: disk summary.
    DiskSummary,
    /// `-m`: slab/heap info.
    Slabs,
}

/// Runtime configuration parsed from CLI arguments.
struct Config {
    mode: OutputMode,
    /// Seconds between reports (None = one-shot).
    interval: Option<u64>,
    /// Number of reports (None = infinite when interval is set).
    count: Option<u64>,
    /// Show active/inactive instead of buff/cache.
    active_mode: bool,
    /// Wide output columns.
    wide: bool,
    /// Add timestamp column.
    timestamp: bool,
    /// Display header only once.
    one_header: bool,
    /// Display unit.
    unit: DisplayUnit,
    /// JSON output.
    json: bool,
}

/// Snapshot of /proc/meminfo fields (stored in KiB as reported by kernel).
struct MemInfo {
    mem_total: u64,
    mem_free: u64,
    buffers: u64,
    cached: u64,
    swap_total: u64,
    swap_free: u64,
    active: u64,
    inactive: u64,
}

/// Snapshot of /proc/stat CPU times (in ticks).
struct CpuTimes {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

/// Snapshot of /proc/stat system counters.
struct StatInfo {
    cpu: CpuTimes,
    interrupts: u64,
    context_switches: u64,
    boot_time: u64,
    processes: u64,
    procs_running: u64,
    procs_blocked: u64,
}

/// Snapshot of /proc/vmstat page counters.
struct VmStatCounters {
    pgpgin: u64,
    pgpgout: u64,
    pswpin: u64,
    pswpout: u64,
    pgfault: u64,
    pgmajfault: u64,
}

/// Individual disk stats from /proc/diskstats.
struct DiskStats {
    name: String,
    reads_completed: u64,
    reads_merged: u64,
    sectors_read: u64,
    ms_reading: u64,
    writes_completed: u64,
    writes_merged: u64,
    sectors_written: u64,
    ms_writing: u64,
    ios_in_progress: u64,
    ms_io: u64,
    weighted_ms_io: u64,
}

/// Complete system snapshot for delta calculations.
struct Snapshot {
    mem: MemInfo,
    stat: StatInfo,
    vmstat: VmStatCounters,
}

// ============================================================================
// File reading helpers
// ============================================================================

/// Read the contents of a file, returning `None` on any I/O error.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok()
}

/// Extract a u64 value for a given key from content with `Key: Value` format.
/// Returns 0 if the key is not found or cannot be parsed.
fn get_kv_value(content: &str, key: &str) -> u64 {
    for line in content.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                let trimmed = v.trim()
                    .trim_end_matches(" kB")
                    .trim_end_matches(" KB")
                    .trim();
                return trimmed.parse().unwrap_or(0);
            }
        }
    }
    0
}

/// Extract a u64 value from /proc/vmstat-style content (`key value` format,
/// space-separated, no colon). Returns 0 if not found.
fn get_space_kv(content: &str, key: &str) -> u64 {
    for line in content.lines() {
        let mut parts = line.split_whitespace();
        if let Some(k) = parts.next() {
            if k == key {
                if let Some(v) = parts.next() {
                    return v.parse().unwrap_or(0);
                }
            }
        }
    }
    0
}

// ============================================================================
// Data source readers
// ============================================================================

/// Parse `/proc/meminfo` into a `MemInfo` struct.
fn read_meminfo() -> Option<MemInfo> {
    let content = read_file("/proc/meminfo")?;
    Some(MemInfo {
        mem_total: get_kv_value(&content, "MemTotal"),
        mem_free: get_kv_value(&content, "MemFree"),
        buffers: get_kv_value(&content, "Buffers"),
        cached: get_kv_value(&content, "Cached"),
        swap_total: get_kv_value(&content, "SwapTotal"),
        swap_free: get_kv_value(&content, "SwapFree"),
        active: get_kv_value(&content, "Active"),
        inactive: get_kv_value(&content, "Inactive"),
    })
}

/// Parse the first `cpu` line from `/proc/stat` plus system counters.
fn read_stat() -> Option<StatInfo> {
    let content = read_file("/proc/stat")?;
    let mut cpu = CpuTimes {
        user: 0, nice: 0, system: 0, idle: 0,
        iowait: 0, irq: 0, softirq: 0, steal: 0,
    };
    let mut interrupts: u64 = 0;
    let mut context_switches: u64 = 0;
    let mut boot_time: u64 = 0;
    let mut processes: u64 = 0;
    let mut procs_running: u64 = 0;
    let mut procs_blocked: u64 = 0;

    for line in content.lines() {
        if line.starts_with("cpu ") {
            // "cpu  user nice system idle iowait irq softirq steal guest guest_nice"
            let fields: Vec<&str> = line.split_whitespace().collect();
            cpu.user = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.nice = fields.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.system = fields.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.idle = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.iowait = fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.irq = fields.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.softirq = fields.get(7).and_then(|s| s.parse().ok()).unwrap_or(0);
            cpu.steal = fields.get(8).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("intr ") {
            // Total interrupt count is the first number after "intr".
            let fields: Vec<&str> = line.split_whitespace().collect();
            interrupts = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("ctxt ") {
            let fields: Vec<&str> = line.split_whitespace().collect();
            context_switches = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("btime ") {
            let fields: Vec<&str> = line.split_whitespace().collect();
            boot_time = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("processes ") {
            let fields: Vec<&str> = line.split_whitespace().collect();
            processes = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("procs_running ") {
            let fields: Vec<&str> = line.split_whitespace().collect();
            procs_running = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        } else if line.starts_with("procs_blocked ") {
            let fields: Vec<&str> = line.split_whitespace().collect();
            procs_blocked = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        }
    }

    Some(StatInfo {
        cpu,
        interrupts,
        context_switches,
        boot_time,
        processes,
        procs_running,
        procs_blocked,
    })
}

/// Parse `/proc/vmstat` for page I/O counters.
fn read_vmstat() -> Option<VmStatCounters> {
    let content = read_file("/proc/vmstat")?;
    Some(VmStatCounters {
        pgpgin: get_space_kv(&content, "pgpgin"),
        pgpgout: get_space_kv(&content, "pgpgout"),
        pswpin: get_space_kv(&content, "pswpin"),
        pswpout: get_space_kv(&content, "pswpout"),
        pgfault: get_space_kv(&content, "pgfault"),
        pgmajfault: get_space_kv(&content, "pgmajfault"),
    })
}

/// Parse `/proc/diskstats` for per-disk I/O statistics.
fn read_diskstats() -> Option<Vec<DiskStats>> {
    let content = read_file("/proc/diskstats")?;
    let mut disks = Vec::new();

    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // /proc/diskstats has at least 14 fields:
        // major minor name reads_completed reads_merged sectors_read ms_reading
        // writes_completed writes_merged sectors_written ms_writing ios_in_progress
        // ms_io weighted_ms_io
        if fields.len() < 14 {
            continue;
        }
        let name = match fields.get(2) {
            Some(n) => (*n).to_string(),
            None => continue,
        };
        disks.push(DiskStats {
            name,
            reads_completed: fields.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
            reads_merged: fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
            sectors_read: fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
            ms_reading: fields.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
            writes_completed: fields.get(7).and_then(|s| s.parse().ok()).unwrap_or(0),
            writes_merged: fields.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
            sectors_written: fields.get(9).and_then(|s| s.parse().ok()).unwrap_or(0),
            ms_writing: fields.get(10).and_then(|s| s.parse().ok()).unwrap_or(0),
            ios_in_progress: fields.get(11).and_then(|s| s.parse().ok()).unwrap_or(0),
            ms_io: fields.get(12).and_then(|s| s.parse().ok()).unwrap_or(0),
            weighted_ms_io: fields.get(13).and_then(|s| s.parse().ok()).unwrap_or(0),
        });
    }

    Some(disks)
}

/// Collect a full system snapshot.
fn take_snapshot() -> Option<Snapshot> {
    let mem = read_meminfo()?;
    let stat = read_stat()?;
    let vmstat = read_vmstat().unwrap_or(VmStatCounters {
        pgpgin: 0, pgpgout: 0, pswpin: 0, pswpout: 0,
        pgfault: 0, pgmajfault: 0,
    });
    Some(Snapshot { mem, stat, vmstat })
}

// ============================================================================
// Unit conversion
// ============================================================================

/// Convert a KiB value according to the configured display unit.
fn convert_kib(kib: u64, unit: DisplayUnit) -> u64 {
    match unit {
        // KiB / 1 = KiB (1024-byte units).
        DisplayUnit::K1024 => kib,
        // KiB * 1024 / 1000 for 1000-byte units.
        DisplayUnit::K1000 => kib.saturating_mul(1024) / 1000,
        // KiB / 1024 = MiB (1048576-byte units).
        DisplayUnit::M1024 => kib / 1024,
        // KiB * 1024 / 1000000 for 1000000-byte units.
        DisplayUnit::M1000 => kib.saturating_mul(1024) / 1_000_000,
    }
}

// ============================================================================
// CPU time helpers
// ============================================================================

/// Total CPU ticks across all fields.
fn cpu_total(c: &CpuTimes) -> u64 {
    c.user.saturating_add(c.nice)
        .saturating_add(c.system)
        .saturating_add(c.idle)
        .saturating_add(c.iowait)
        .saturating_add(c.irq)
        .saturating_add(c.softirq)
        .saturating_add(c.steal)
}

/// Compute CPU delta between two snapshots.
fn cpu_delta(cur: &CpuTimes, prev: &CpuTimes) -> CpuTimes {
    CpuTimes {
        user: cur.user.saturating_sub(prev.user),
        nice: cur.nice.saturating_sub(prev.nice),
        system: cur.system.saturating_sub(prev.system),
        idle: cur.idle.saturating_sub(prev.idle),
        iowait: cur.iowait.saturating_sub(prev.iowait),
        irq: cur.irq.saturating_sub(prev.irq),
        softirq: cur.softirq.saturating_sub(prev.softirq),
        steal: cur.steal.saturating_sub(prev.steal),
    }
}

/// Convert CPU times to percentage values. Returns (us, sy, id, wa, st).
fn cpu_percentages(delta: &CpuTimes) -> (u64, u64, u64, u64, u64) {
    let total = cpu_total(delta);
    if total == 0 {
        return (0, 0, 100, 0, 0);
    }
    let us = (delta.user.saturating_add(delta.nice)).saturating_mul(100) / total;
    let sy = (delta.system.saturating_add(delta.irq).saturating_add(delta.softirq))
        .saturating_mul(100) / total;
    let wa = delta.iowait.saturating_mul(100) / total;
    let st = delta.steal.saturating_mul(100) / total;
    // Idle gets the remainder so percentages always sum to 100.
    let id = 100_u64.saturating_sub(us).saturating_sub(sy).saturating_sub(wa).saturating_sub(st);
    (us, sy, id, wa, st)
}

// ============================================================================
// Timestamp formatting
// ============================================================================

/// Produce a simple timestamp string from seconds since epoch.
/// Format: YYYY-MM-DD HH:MM:SS (UTC, computed without libc/chrono).
fn format_timestamp_from_epoch(epoch_secs: u64) -> String {
    // Days from epoch to start of year, simple leap-year calculation.
    let secs_per_day: u64 = 86400;
    let remaining = epoch_secs;

    let time_of_day = remaining % secs_per_day;
    let mut days = remaining / secs_per_day;

    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Compute year.
    let mut year: u64 = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }

    // Compute month and day.
    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month: u64 = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    let day = days + 1;

    let _ = remaining; // suppress unused warning
    format!("{year:04}-{month:02}-{day:02} {hours:02}:{minutes:02}:{seconds:02}")
}

/// Check if a year is a leap year.
fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Read current time as seconds since epoch from /proc/stat btime + /proc/uptime,
/// or fall back to 0. This avoids needing libc clock_gettime on our minimal target.
fn current_epoch_secs() -> u64 {
    // Try /proc/uptime first for elapsed seconds, combined with btime.
    if let Some(content) = read_file("/proc/uptime") {
        let uptime_secs: u64 = content.split_whitespace()
            .next()
            .and_then(|s| {
                // uptime may be a float like "12345.67"
                s.split('.').next().and_then(|int_part| int_part.parse().ok())
            })
            .unwrap_or(0);
        // Read btime from /proc/stat.
        if let Some(stat_content) = read_file("/proc/stat") {
            for line in stat_content.lines() {
                if line.starts_with("btime ") {
                    let btime: u64 = line.split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(0);
                    if btime > 0 {
                        return btime.saturating_add(uptime_secs);
                    }
                }
            }
        }
    }
    0
}

// ============================================================================
// Default mode: vmstat table output
// ============================================================================

/// Print the vmstat header line.
fn print_header(config: &Config) {
    let w = if config.wide { WIDE_WIDTH } else { NARROW_WIDTH };

    // Top grouping line.
    if config.wide {
        print!("procs -----------memory---------- ---swap-- -----io---- -system-- ------cpu-----");
    } else {
        print!("procs -----------memory---------- ---swap-- -----io---- -system-- ------cpu-----");
    }
    if config.timestamp {
        print!("  ---timestamp---");
    }
    println!();

    // Column labels.
    print!(" {:>w$} {:>w$}", "r", "b", w = 2);
    if config.active_mode {
        print!(
            "   {:>w$} {:>w$} {:>w$} {:>w$}",
            "swpd", "free", "inact", "active",
            w = w,
        );
    } else {
        print!(
            "   {:>w$} {:>w$} {:>w$} {:>w$}",
            "swpd", "free", "buff", "cache",
            w = w,
        );
    }
    print!(
        "   {:>w$} {:>w$}    {:>w$} {:>w$}   {:>w$} {:>w$}",
        "si", "so", "bi", "bo", "in", "cs",
        w = w,
    );
    print!(" {:>2} {:>2} {:>2} {:>2} {:>2}", "us", "sy", "id", "wa", "st");
    if config.timestamp {
        print!("  {:>19}", "timestamp");
    }
    println!();
}

/// Print one row of vmstat data.
///
/// If `prev` is `None`, this is the first (since-boot) report: rates are
/// computed by dividing totals by uptime in seconds.
/// If `prev` is `Some`, delta-mode: rates are (cur - prev) / interval.
fn print_row(
    cur: &Snapshot,
    prev: Option<&Snapshot>,
    interval: u64,
    config: &Config,
) {
    let w = if config.wide { WIDE_WIDTH } else { NARROW_WIDTH };

    // Determine the divisor for rate calculations.
    let divisor = if prev.is_some() {
        if interval == 0 { 1 } else { interval }
    } else {
        // First report: rates since boot. Compute uptime from /proc/uptime.
        read_uptime_secs().unwrap_or(1).max(1)
    };

    // Process counts.
    let r = cur.stat.procs_running;
    let b = cur.stat.procs_blocked;

    // Memory values (convert from KiB to display unit).
    let swap_used = cur.mem.swap_total.saturating_sub(cur.mem.swap_free);
    let swpd = convert_kib(swap_used, config.unit);
    let free = convert_kib(cur.mem.mem_free, config.unit);

    let (mem_col3, mem_col4) = if config.active_mode {
        (
            convert_kib(cur.mem.inactive, config.unit),
            convert_kib(cur.mem.active, config.unit),
        )
    } else {
        (
            convert_kib(cur.mem.buffers, config.unit),
            convert_kib(cur.mem.cached, config.unit),
        )
    };

    // Swap I/O rates (pages/s from /proc/vmstat pswpin/pswpout).
    let (si, so) = match prev {
        Some(p) => (
            cur.vmstat.pswpin.saturating_sub(p.vmstat.pswpin) / divisor,
            cur.vmstat.pswpout.saturating_sub(p.vmstat.pswpout) / divisor,
        ),
        None => (
            cur.vmstat.pswpin / divisor,
            cur.vmstat.pswpout / divisor,
        ),
    };

    // Block I/O rates (KiB/s from /proc/vmstat pgpgin/pgpgout which are in KiB).
    let (bi, bo) = match prev {
        Some(p) => (
            cur.vmstat.pgpgin.saturating_sub(p.vmstat.pgpgin) / divisor,
            cur.vmstat.pgpgout.saturating_sub(p.vmstat.pgpgout) / divisor,
        ),
        None => (
            cur.vmstat.pgpgin / divisor,
            cur.vmstat.pgpgout / divisor,
        ),
    };

    // System rates (interrupts/s, context switches/s).
    let (int_rate, cs_rate) = match prev {
        Some(p) => (
            cur.stat.interrupts.saturating_sub(p.stat.interrupts) / divisor,
            cur.stat.context_switches.saturating_sub(p.stat.context_switches) / divisor,
        ),
        None => (
            cur.stat.interrupts / divisor,
            cur.stat.context_switches / divisor,
        ),
    };

    // CPU percentages.
    let delta_cpu = match prev {
        Some(p) => cpu_delta(&cur.stat.cpu, &p.stat.cpu),
        None => {
            // Since-boot: use absolute totals.
            CpuTimes {
                user: cur.stat.cpu.user,
                nice: cur.stat.cpu.nice,
                system: cur.stat.cpu.system,
                idle: cur.stat.cpu.idle,
                iowait: cur.stat.cpu.iowait,
                irq: cur.stat.cpu.irq,
                softirq: cur.stat.cpu.softirq,
                steal: cur.stat.cpu.steal,
            }
        }
    };
    let (us, sy, id, wa, st) = cpu_percentages(&delta_cpu);

    // Print the row.
    print!(" {:>2} {:>2}", r, b);
    print!(
        "   {:>w$} {:>w$} {:>w$} {:>w$}",
        swpd, free, mem_col3, mem_col4,
        w = w,
    );
    print!(
        "   {:>w$} {:>w$}    {:>w$} {:>w$}   {:>w$} {:>w$}",
        si, so, bi, bo, int_rate, cs_rate,
        w = w,
    );
    print!(" {:>2} {:>2} {:>2} {:>2} {:>2}", us, sy, id, wa, st);

    if config.timestamp {
        let ts = format_timestamp_from_epoch(current_epoch_secs());
        print!("  {ts}");
    }

    println!();
}

/// Read system uptime in seconds from `/proc/uptime`.
fn read_uptime_secs() -> Option<u64> {
    let content = read_file("/proc/uptime")?;
    content.split_whitespace()
        .next()
        .and_then(|s| s.split('.').next())
        .and_then(|s| s.parse().ok())
}

/// Run the default vmstat output mode.
fn run_default(config: &Config) -> i32 {
    // First snapshot (since-boot averages).
    let first = match take_snapshot() {
        Some(s) => s,
        None => {
            eprintln!("vmstat: failed to read /proc data");
            return 1;
        }
    };

    print_header(config);
    print_row(&first, None, 0, config);

    // If no interval, we are done after one report.
    let interval = match config.interval {
        Some(i) if i > 0 => i,
        _ => return 0,
    };

    let mut prev = first;
    let mut reports: u64 = 1;

    loop {
        // Check count limit. count includes the first (boot-average) report.
        if let Some(max) = config.count {
            if reports >= max {
                break;
            }
        }

        std::thread::sleep(Duration::from_secs(interval));

        let cur = match take_snapshot() {
            Some(s) => s,
            None => {
                eprintln!("vmstat: failed to read /proc data");
                return 1;
            }
        };

        // Re-print header periodically unless --one-header.
        if !config.one_header && reports % 20 == 0 {
            print_header(config);
        }

        print_row(&cur, Some(&prev), interval, config);
        prev = cur;
        reports = reports.saturating_add(1);
    }

    0
}

// ============================================================================
// JSON mode for default output
// ============================================================================

/// Print default vmstat data as JSON.
fn run_default_json(config: &Config) -> i32 {
    let snap = match take_snapshot() {
        Some(s) => s,
        None => {
            eprintln!("vmstat: failed to read /proc data");
            return 1;
        }
    };

    let uptime = read_uptime_secs().unwrap_or(1).max(1);
    let swap_used = snap.mem.swap_total.saturating_sub(snap.mem.swap_free);
    let (us, sy, id, wa, st) = cpu_percentages(&snap.stat.cpu);

    println!("{{");
    println!("  \"procs\": {{");
    println!("    \"r\": {},", snap.stat.procs_running);
    println!("    \"b\": {}", snap.stat.procs_blocked);
    println!("  }},");
    println!("  \"memory\": {{");
    println!("    \"swpd\": {},", convert_kib(swap_used, config.unit));
    println!("    \"free\": {},", convert_kib(snap.mem.mem_free, config.unit));
    println!("    \"buff\": {},", convert_kib(snap.mem.buffers, config.unit));
    println!("    \"cache\": {},", convert_kib(snap.mem.cached, config.unit));
    println!("    \"active\": {},", convert_kib(snap.mem.active, config.unit));
    println!("    \"inactive\": {}", convert_kib(snap.mem.inactive, config.unit));
    println!("  }},");
    println!("  \"swap\": {{");
    println!("    \"si\": {},", snap.vmstat.pswpin / uptime);
    println!("    \"so\": {}", snap.vmstat.pswpout / uptime);
    println!("  }},");
    println!("  \"io\": {{");
    println!("    \"bi\": {},", snap.vmstat.pgpgin / uptime);
    println!("    \"bo\": {}", snap.vmstat.pgpgout / uptime);
    println!("  }},");
    println!("  \"system\": {{");
    println!("    \"in\": {},", snap.stat.interrupts / uptime);
    println!("    \"cs\": {}", snap.stat.context_switches / uptime);
    println!("  }},");
    println!("  \"cpu\": {{");
    println!("    \"us\": {us},");
    println!("    \"sy\": {sy},");
    println!("    \"id\": {id},");
    println!("    \"wa\": {wa},");
    println!("    \"st\": {st}");
    println!("  }}");
    println!("}}");

    0
}

// ============================================================================
// -f / --forks mode
// ============================================================================

/// Print total number of forks since boot.
fn run_forks(config: &Config) -> i32 {
    let stat = match read_stat() {
        Some(s) => s,
        None => {
            eprintln!("vmstat: failed to read /proc/stat");
            return 1;
        }
    };

    if config.json {
        println!("{{\"forks\": {}}}", stat.processes);
    } else {
        println!("{:>12} forks", stat.processes);
    }
    0
}

// ============================================================================
// -s / --stats mode
// ============================================================================

/// Print a table of event counters and memory statistics.
fn run_stats(config: &Config) -> i32 {
    let mem = match read_meminfo() {
        Some(m) => m,
        None => {
            eprintln!("vmstat: failed to read /proc/meminfo");
            return 1;
        }
    };
    let stat = match read_stat() {
        Some(s) => s,
        None => {
            eprintln!("vmstat: failed to read /proc/stat");
            return 1;
        }
    };
    let vmstat = read_vmstat().unwrap_or(VmStatCounters {
        pgpgin: 0, pgpgout: 0, pswpin: 0, pswpout: 0,
        pgfault: 0, pgmajfault: 0,
    });

    let u = config.unit;

    if config.json {
        println!("{{");
        println!("  \"memory\": {{");
        println!("    \"total\": {},", convert_kib(mem.mem_total, u));
        println!("    \"used\": {},", convert_kib(mem.mem_total.saturating_sub(mem.mem_free), u));
        println!("    \"active\": {},", convert_kib(mem.active, u));
        println!("    \"inactive\": {},", convert_kib(mem.inactive, u));
        println!("    \"free\": {},", convert_kib(mem.mem_free, u));
        println!("    \"buffer\": {},", convert_kib(mem.buffers, u));
        println!("    \"swap_cache\": {},", convert_kib(mem.cached, u));
        println!("    \"swap_total\": {},", convert_kib(mem.swap_total, u));
        println!("    \"swap_used\": {},", convert_kib(mem.swap_total.saturating_sub(mem.swap_free), u));
        println!("    \"swap_free\": {}", convert_kib(mem.swap_free, u));
        println!("  }},");
        println!("  \"cpu_ticks\": {{");
        println!("    \"user\": {},", stat.cpu.user);
        println!("    \"nice\": {},", stat.cpu.nice);
        println!("    \"system\": {},", stat.cpu.system);
        println!("    \"idle\": {},", stat.cpu.idle);
        println!("    \"iowait\": {},", stat.cpu.iowait);
        println!("    \"irq\": {},", stat.cpu.irq);
        println!("    \"softirq\": {},", stat.cpu.softirq);
        println!("    \"steal\": {}", stat.cpu.steal);
        println!("  }},");
        println!("  \"counters\": {{");
        println!("    \"interrupts\": {},", stat.interrupts);
        println!("    \"context_switches\": {},", stat.context_switches);
        println!("    \"boot_time\": {},", stat.boot_time);
        println!("    \"forks\": {},", stat.processes);
        println!("    \"pgpgin\": {},", vmstat.pgpgin);
        println!("    \"pgpgout\": {},", vmstat.pgpgout);
        println!("    \"pswpin\": {},", vmstat.pswpin);
        println!("    \"pswpout\": {},", vmstat.pswpout);
        println!("    \"pgfault\": {},", vmstat.pgfault);
        println!("    \"pgmajfault\": {}", vmstat.pgmajfault);
        println!("  }}");
        println!("}}");
        return 0;
    }

    let unit_label = match u {
        DisplayUnit::K1024 => "K",
        DisplayUnit::K1000 => "K",
        DisplayUnit::M1024 => "M",
        DisplayUnit::M1000 => "M",
    };

    // Memory stats section.
    println!("{:>12} {} total memory", convert_kib(mem.mem_total, u), unit_label);
    println!("{:>12} {} used memory", convert_kib(mem.mem_total.saturating_sub(mem.mem_free), u), unit_label);
    println!("{:>12} {} active memory", convert_kib(mem.active, u), unit_label);
    println!("{:>12} {} inactive memory", convert_kib(mem.inactive, u), unit_label);
    println!("{:>12} {} free memory", convert_kib(mem.mem_free, u), unit_label);
    println!("{:>12} {} buffer memory", convert_kib(mem.buffers, u), unit_label);
    println!("{:>12} {} swap cache", convert_kib(mem.cached, u), unit_label);
    println!("{:>12} {} total swap", convert_kib(mem.swap_total, u), unit_label);
    println!("{:>12} {} used swap", convert_kib(mem.swap_total.saturating_sub(mem.swap_free), u), unit_label);
    println!("{:>12} {} free swap", convert_kib(mem.swap_free, u), unit_label);

    // CPU and event counters.
    println!("{:>12} non-nice user cpu ticks", stat.cpu.user);
    println!("{:>12} nice user cpu ticks", stat.cpu.nice);
    println!("{:>12} system cpu ticks", stat.cpu.system);
    println!("{:>12} idle cpu ticks", stat.cpu.idle);
    println!("{:>12} IO-wait cpu ticks", stat.cpu.iowait);
    println!("{:>12} IRQ cpu ticks", stat.cpu.irq);
    println!("{:>12} softirq cpu ticks", stat.cpu.softirq);
    println!("{:>12} stolen cpu ticks", stat.cpu.steal);
    println!("{:>12} pages paged in", vmstat.pgpgin);
    println!("{:>12} pages paged out", vmstat.pgpgout);
    println!("{:>12} pages swapped in", vmstat.pswpin);
    println!("{:>12} pages swapped out", vmstat.pswpout);
    println!("{:>12} interrupts", stat.interrupts);
    println!("{:>12} CPU context switches", stat.context_switches);
    println!("{:>12} boot time", stat.boot_time);
    println!("{:>12} forks", stat.processes);
    println!("{:>12} page faults", vmstat.pgfault);
    println!("{:>12} major page faults", vmstat.pgmajfault);

    0
}

// ============================================================================
// -d / --disk mode
// ============================================================================

/// Print per-disk I/O statistics.
fn run_disk(config: &Config) -> i32 {
    let disks = match read_diskstats() {
        Some(d) => d,
        None => {
            eprintln!("vmstat: failed to read /proc/diskstats");
            return 1;
        }
    };

    if disks.is_empty() {
        println!("vmstat: no disk statistics available");
        return 0;
    }

    if config.json {
        println!("[");
        for (i, d) in disks.iter().enumerate() {
            let comma = if i + 1 < disks.len() { "," } else { "" };
            println!("  {{");
            println!("    \"name\": \"{}\",", d.name);
            println!("    \"reads\": {{");
            println!("      \"total\": {},", d.reads_completed);
            println!("      \"merged\": {},", d.reads_merged);
            println!("      \"sectors\": {},", d.sectors_read);
            println!("      \"ms\": {}", d.ms_reading);
            println!("    }},");
            println!("    \"writes\": {{");
            println!("      \"total\": {},", d.writes_completed);
            println!("      \"merged\": {},", d.writes_merged);
            println!("      \"sectors\": {},", d.sectors_written);
            println!("      \"ms\": {}", d.ms_writing);
            println!("    }},");
            println!("    \"io\": {{");
            println!("      \"in_progress\": {},", d.ios_in_progress);
            println!("      \"ms\": {},", d.ms_io);
            println!("      \"weighted_ms\": {}", d.weighted_ms_io);
            println!("    }}");
            println!("  }}{comma}");
        }
        println!("]");
        return 0;
    }

    // Header.
    println!(
        "{:<10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "disk", "reads", "r_merged", "r_sectors", "r_ms",
        "writes", "w_merged", "w_sectors", "w_ms",
        "cur_io", "io_ms", "wt_ms",
    );

    for d in &disks {
        println!(
            "{:<10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            d.name,
            d.reads_completed, d.reads_merged, d.sectors_read, d.ms_reading,
            d.writes_completed, d.writes_merged, d.sectors_written, d.ms_writing,
            d.ios_in_progress, d.ms_io, d.weighted_ms_io,
        );
    }

    0
}

// ============================================================================
// -D / --disk-sum mode
// ============================================================================

/// Print a summary of total disk I/O.
fn run_disk_summary(config: &Config) -> i32 {
    let disks = match read_diskstats() {
        Some(d) => d,
        None => {
            eprintln!("vmstat: failed to read /proc/diskstats");
            return 1;
        }
    };

    let mut total_reads: u64 = 0;
    let mut total_reads_merged: u64 = 0;
    let mut total_sectors_read: u64 = 0;
    let mut total_ms_reading: u64 = 0;
    let mut total_writes: u64 = 0;
    let mut total_writes_merged: u64 = 0;
    let mut total_sectors_written: u64 = 0;
    let mut total_ms_writing: u64 = 0;
    let mut total_ios: u64 = 0;
    let mut total_ms_io: u64 = 0;
    let num_disks = disks.len();

    for d in &disks {
        total_reads = total_reads.saturating_add(d.reads_completed);
        total_reads_merged = total_reads_merged.saturating_add(d.reads_merged);
        total_sectors_read = total_sectors_read.saturating_add(d.sectors_read);
        total_ms_reading = total_ms_reading.saturating_add(d.ms_reading);
        total_writes = total_writes.saturating_add(d.writes_completed);
        total_writes_merged = total_writes_merged.saturating_add(d.writes_merged);
        total_sectors_written = total_sectors_written.saturating_add(d.sectors_written);
        total_ms_writing = total_ms_writing.saturating_add(d.ms_writing);
        total_ios = total_ios.saturating_add(d.ios_in_progress);
        total_ms_io = total_ms_io.saturating_add(d.ms_io);
    }

    if config.json {
        println!("{{");
        println!("  \"disks\": {num_disks},");
        println!("  \"reads\": {total_reads},");
        println!("  \"reads_merged\": {total_reads_merged},");
        println!("  \"sectors_read\": {total_sectors_read},");
        println!("  \"ms_reading\": {total_ms_reading},");
        println!("  \"writes\": {total_writes},");
        println!("  \"writes_merged\": {total_writes_merged},");
        println!("  \"sectors_written\": {total_sectors_written},");
        println!("  \"ms_writing\": {total_ms_writing},");
        println!("  \"ios_in_progress\": {total_ios},");
        println!("  \"ms_io\": {total_ms_io}");
        println!("}}");
        return 0;
    }

    println!("{:>12} disks", num_disks);
    println!("{:>12} total reads", total_reads);
    println!("{:>12} merged reads", total_reads_merged);
    println!("{:>12} read sectors", total_sectors_read);
    println!("{:>12} ms reading", total_ms_reading);
    println!("{:>12} total writes", total_writes);
    println!("{:>12} merged writes", total_writes_merged);
    println!("{:>12} written sectors", total_sectors_written);
    println!("{:>12} ms writing", total_ms_writing);
    println!("{:>12} in-progress I/Os", total_ios);
    println!("{:>12} ms spent on I/O", total_ms_io);

    0
}

// ============================================================================
// -m / --slabs mode
// ============================================================================

/// Print slab/heap allocator information.
///
/// On OurOS, this reads from `/proc/slabinfo` if available. Falls back to
/// a "not available" message, which is expected during early OS bring-up.
fn run_slabs(config: &Config) -> i32 {
    let content = match read_file("/proc/slabinfo") {
        Some(c) => c,
        None => {
            if config.json {
                println!("{{\"slabs\": []}}");
            } else {
                println!("vmstat: slab information not available (/proc/slabinfo missing)");
            }
            return 0;
        }
    };

    if config.json {
        // Parse and emit as JSON array.
        println!("[");
        let mut first = true;
        for line in content.lines() {
            // Skip comment/header lines (start with '#' or "slabinfo").
            if line.starts_with('#') || line.starts_with("slabinfo") {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 6 {
                continue;
            }
            if !first {
                println!(",");
            }
            first = false;
            let name = fields.first().copied().unwrap_or("?");
            let active_objs: u64 = fields.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
            let num_objs: u64 = fields.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
            let obj_size: u64 = fields.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
            let objs_per_slab: u64 = fields.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
            let pages_per_slab: u64 = fields.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
            print!(
                "  {{\"name\": \"{name}\", \"active_objs\": {active_objs}, \"num_objs\": {num_objs}, \"obj_size\": {obj_size}, \"objs_per_slab\": {objs_per_slab}, \"pages_per_slab\": {pages_per_slab}}}"
            );
        }
        println!();
        println!("]");
        return 0;
    }

    // Tabular output.
    println!(
        "{:<24} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Cache", "Num", "Total", "Size", "ObjPerSlab", "PagesPerSlab",
    );

    for line in content.lines() {
        if line.starts_with('#') || line.starts_with("slabinfo") {
            continue;
        }
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 6 {
            continue;
        }
        let name = fields.first().copied().unwrap_or("?");
        let active_objs = fields.get(1).copied().unwrap_or("0");
        let num_objs = fields.get(2).copied().unwrap_or("0");
        let obj_size = fields.get(3).copied().unwrap_or("0");
        let objs_per_slab = fields.get(4).copied().unwrap_or("0");
        let pages_per_slab = fields.get(5).copied().unwrap_or("0");

        println!(
            "{:<24} {:>10} {:>10} {:>10} {:>10} {:>10}",
            name, active_objs, num_objs, obj_size, objs_per_slab, pages_per_slab,
        );
    }

    0
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    println!("OurOS Virtual Memory Statistics v0.1.0");
    println!();
    println!("Report virtual memory, CPU, and I/O statistics.");
    println!();
    println!("USAGE:");
    println!("  vmstat [options] [interval [count]]");
    println!();
    println!("OPTIONS:");
    println!("  -a, --active       Show active/inactive memory instead of buff/cache");
    println!("  -f, --forks        Show total number of forks since boot");
    println!("  -m, --slabs        Show slab/heap allocator info");
    println!("  -s, --stats        Show event counters and memory stats table");
    println!("  -d, --disk         Show per-disk I/O statistics");
    println!("  -D, --disk-sum     Show disk I/O summary");
    println!("  -w, --wide         Wider output columns");
    println!("  -t, --timestamp    Add timestamp to each line");
    println!("  -n, --one-header   Display the header only once");
    println!("  -S <unit>          Display units:");
    println!("                       k = 1024 bytes (default)");
    println!("                       K = 1000 bytes");
    println!("                       m = 1048576 bytes");
    println!("                       M = 1000000 bytes");
    println!("      --json         JSON output");
    println!("      --help         Show this help");
    println!();
    println!("FIELDS (default mode):");
    println!("  procs:");
    println!("    r  Runnable processes");
    println!("    b  Blocked processes");
    println!("  memory:");
    println!("    swpd   Used swap (kB)");
    println!("    free   Free memory (kB)");
    println!("    buff   Buffer memory (kB)");
    println!("    cache  Cache memory (kB)");
    println!("  swap:");
    println!("    si  Swap in (pages/s)");
    println!("    so  Swap out (pages/s)");
    println!("  io:");
    println!("    bi  Blocks read/s");
    println!("    bo  Blocks written/s");
    println!("  system:");
    println!("    in  Interrupts/s");
    println!("    cs  Context switches/s");
    println!("  cpu (as percentages of total CPU time):");
    println!("    us  User time");
    println!("    sy  System time");
    println!("    id  Idle time");
    println!("    wa  I/O wait time");
    println!("    st  Stolen time");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        mode: OutputMode::Default,
        interval: None,
        count: None,
        active_mode: false,
        wide: false,
        timestamp: false,
        one_header: false,
        unit: DisplayUnit::K1024,
        json: false,
    };

    // Collect positional arguments (interval, count) separately.
    let mut positionals: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = match args.get(i) {
            Some(a) => a.as_str(),
            None => break,
        };
        match arg {
            "-a" | "--active" => {
                config.active_mode = true;
            }
            "-f" | "--forks" => {
                config.mode = OutputMode::Forks;
            }
            "-m" | "--slabs" => {
                config.mode = OutputMode::Slabs;
            }
            "-s" | "--stats" => {
                config.mode = OutputMode::Stats;
            }
            "-d" | "--disk" => {
                config.mode = OutputMode::Disk;
            }
            "-D" | "--disk-sum" => {
                config.mode = OutputMode::DiskSummary;
            }
            "-w" | "--wide" => {
                config.wide = true;
            }
            "-t" | "--timestamp" => {
                config.timestamp = true;
            }
            "-n" | "--one-header" => {
                config.one_header = true;
            }
            "--json" => {
                config.json = true;
            }
            "-S" => {
                i += 1;
                let unit_arg = match args.get(i) {
                    Some(u) => u.as_str(),
                    None => {
                        eprintln!("vmstat: -S requires a unit argument (k, K, m, M)");
                        process::exit(1);
                    }
                };
                config.unit = match unit_arg {
                    "k" => DisplayUnit::K1024,
                    "K" => DisplayUnit::K1000,
                    "m" => DisplayUnit::M1024,
                    "M" => DisplayUnit::M1000,
                    other => {
                        eprintln!("vmstat: unknown unit '{other}' (use k, K, m, or M)");
                        process::exit(1);
                    }
                };
            }
            "--help" => {
                print_usage();
                process::exit(0);
            }
            other => {
                // Try to parse as a positional numeric argument.
                if other.starts_with('-') {
                    eprintln!("vmstat: unknown option: {other}");
                    eprintln!("Run 'vmstat --help' for usage.");
                    process::exit(1);
                }
                positionals.push(other.to_string());
            }
        }
        i += 1;
    }

    // Parse positional arguments: [interval [count]].
    if let Some(first) = positionals.first() {
        match first.parse::<u64>() {
            Ok(interval) if interval > 0 => {
                config.interval = Some(interval);
            }
            Ok(_) => {
                eprintln!("vmstat: interval must be a positive integer");
                process::exit(1);
            }
            Err(_) => {
                eprintln!("vmstat: invalid interval: {first}");
                process::exit(1);
            }
        }
    }
    if let Some(second) = positionals.get(1) {
        match second.parse::<u64>() {
            Ok(count) if count > 0 => {
                config.count = Some(count);
            }
            Ok(_) => {
                eprintln!("vmstat: count must be a positive integer");
                process::exit(1);
            }
            Err(_) => {
                eprintln!("vmstat: invalid count: {second}");
                process::exit(1);
            }
        }
    }

    let exit_code = match config.mode {
        OutputMode::Default => {
            if config.json {
                run_default_json(&config)
            } else {
                run_default(&config)
            }
        }
        OutputMode::Forks => run_forks(&config),
        OutputMode::Stats => run_stats(&config),
        OutputMode::Disk => run_disk(&config),
        OutputMode::DiskSummary => run_disk_summary(&config),
        OutputMode::Slabs => run_slabs(&config),
    };

    process::exit(exit_code);
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // -- Parser helpers --

    const SAMPLE_MEMINFO: &str = "\
MemTotal:       16384000 kB
MemFree:         8192000 kB
MemAvailable:   12000000 kB
Buffers:          512000 kB
Cached:          2048000 kB
SwapTotal:       4096000 kB
SwapFree:        3072000 kB
Active:          4000000 kB
Inactive:        2000000 kB
Shmem:            256000 kB
SReclaimable:     128000 kB
";

    const SAMPLE_STAT: &str = "\
cpu  10000 500 3000 80000 1000 200 100 50 0 0
cpu0 5000 250 1500 40000 500 100 50 25 0 0
intr 500000 50 0 0 0 0
ctxt 1200000
btime 1700000000
processes 5000
procs_running 3
procs_blocked 1
";

    const SAMPLE_VMSTAT: &str = "\
pgpgin 200000
pgpgout 150000
pswpin 1000
pswpout 2000
pgfault 9000000
pgmajfault 500
";

    #[test]
    fn test_get_kv_value_basic() {
        assert_eq!(get_kv_value(SAMPLE_MEMINFO, "MemTotal"), 16_384_000);
        assert_eq!(get_kv_value(SAMPLE_MEMINFO, "MemFree"), 8_192_000);
        assert_eq!(get_kv_value(SAMPLE_MEMINFO, "SwapFree"), 3_072_000);
        assert_eq!(get_kv_value(SAMPLE_MEMINFO, "Active"), 4_000_000);
        assert_eq!(get_kv_value(SAMPLE_MEMINFO, "Inactive"), 2_000_000);
    }

    #[test]
    fn test_get_kv_value_missing() {
        assert_eq!(get_kv_value(SAMPLE_MEMINFO, "NonExistent"), 0);
    }

    #[test]
    fn test_get_space_kv_basic() {
        assert_eq!(get_space_kv(SAMPLE_VMSTAT, "pgpgin"), 200_000);
        assert_eq!(get_space_kv(SAMPLE_VMSTAT, "pswpin"), 1_000);
        assert_eq!(get_space_kv(SAMPLE_VMSTAT, "pgfault"), 9_000_000);
    }

    #[test]
    fn test_get_space_kv_missing() {
        assert_eq!(get_space_kv(SAMPLE_VMSTAT, "nonexistent"), 0);
    }

    // -- Unit conversion --

    #[test]
    fn test_convert_kib_k1024() {
        assert_eq!(convert_kib(1024, DisplayUnit::K1024), 1024);
    }

    #[test]
    fn test_convert_kib_k1000() {
        // 1024 KiB = 1024 * 1024 bytes = 1048576 bytes / 1000 = 1048
        assert_eq!(convert_kib(1024, DisplayUnit::K1000), 1048);
    }

    #[test]
    fn test_convert_kib_m1024() {
        // 2048 KiB / 1024 = 2 MiB
        assert_eq!(convert_kib(2048, DisplayUnit::M1024), 2);
    }

    #[test]
    fn test_convert_kib_m1000() {
        // 1024 KiB = 1048576 bytes / 1000000 = 1
        assert_eq!(convert_kib(1024, DisplayUnit::M1000), 1);
    }

    #[test]
    fn test_convert_kib_zero() {
        assert_eq!(convert_kib(0, DisplayUnit::K1024), 0);
        assert_eq!(convert_kib(0, DisplayUnit::K1000), 0);
        assert_eq!(convert_kib(0, DisplayUnit::M1024), 0);
        assert_eq!(convert_kib(0, DisplayUnit::M1000), 0);
    }

    // -- CPU calculations --

    #[test]
    fn test_cpu_total() {
        let c = CpuTimes {
            user: 100, nice: 10, system: 30, idle: 800,
            iowait: 20, irq: 5, softirq: 3, steal: 2,
        };
        assert_eq!(cpu_total(&c), 970);
    }

    #[test]
    fn test_cpu_delta() {
        let prev = CpuTimes {
            user: 100, nice: 10, system: 30, idle: 800,
            iowait: 20, irq: 5, softirq: 3, steal: 2,
        };
        let cur = CpuTimes {
            user: 200, nice: 15, system: 50, idle: 900,
            iowait: 25, irq: 7, softirq: 4, steal: 3,
        };
        let d = cpu_delta(&cur, &prev);
        assert_eq!(d.user, 100);
        assert_eq!(d.nice, 5);
        assert_eq!(d.system, 20);
        assert_eq!(d.idle, 100);
        assert_eq!(d.iowait, 5);
        assert_eq!(d.irq, 2);
        assert_eq!(d.softirq, 1);
        assert_eq!(d.steal, 1);
    }

    #[test]
    fn test_cpu_percentages_typical() {
        let delta = CpuTimes {
            user: 50, nice: 0, system: 10, idle: 930,
            iowait: 5, irq: 2, softirq: 1, steal: 2,
        };
        let (us, sy, id, wa, st) = cpu_percentages(&delta);
        // us = (50+0)*100/1000 = 5
        assert_eq!(us, 5);
        // sy = (10+2+1)*100/1000 = 1
        assert_eq!(sy, 1);
        // wa = 5*100/1000 = 0
        assert_eq!(wa, 0);
        // st = 2*100/1000 = 0
        assert_eq!(st, 0);
        // id = 100 - 5 - 1 - 0 - 0 = 94
        assert_eq!(id, 94);
    }

    #[test]
    fn test_cpu_percentages_zero_total() {
        let delta = CpuTimes {
            user: 0, nice: 0, system: 0, idle: 0,
            iowait: 0, irq: 0, softirq: 0, steal: 0,
        };
        let (us, sy, id, wa, st) = cpu_percentages(&delta);
        assert_eq!(us, 0);
        assert_eq!(sy, 0);
        assert_eq!(id, 100);
        assert_eq!(wa, 0);
        assert_eq!(st, 0);
    }

    #[test]
    fn test_cpu_percentages_sum_to_100() {
        let delta = CpuTimes {
            user: 333, nice: 0, system: 333, idle: 0,
            iowait: 0, irq: 0, softirq: 0, steal: 334,
        };
        let (us, sy, id, wa, st) = cpu_percentages(&delta);
        assert_eq!(us + sy + id + wa + st, 100);
    }

    // -- Timestamp formatting --

    #[test]
    fn test_format_timestamp_epoch_zero() {
        let ts = format_timestamp_from_epoch(0);
        assert_eq!(ts, "1970-01-01 00:00:00");
    }

    #[test]
    fn test_format_timestamp_known_date() {
        // 2024-01-01 00:00:00 UTC = 1704067200
        let ts = format_timestamp_from_epoch(1_704_067_200);
        assert_eq!(ts, "2024-01-01 00:00:00");
    }

    #[test]
    fn test_is_leap() {
        assert!(is_leap(2000));
        assert!(is_leap(2024));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
    }

    // -- Swap used calculation --

    #[test]
    fn test_swap_used() {
        let total: u64 = 4_096_000;
        let free: u64 = 3_072_000;
        assert_eq!(total.saturating_sub(free), 1_024_000);
    }

    // -- Stat parsing --

    #[test]
    fn test_parse_stat_cpu_line() {
        // Verify the CPU parsing logic manually against SAMPLE_STAT.
        let content = SAMPLE_STAT;
        for line in content.lines() {
            if line.starts_with("cpu ") {
                let fields: Vec<&str> = line.split_whitespace().collect();
                assert_eq!(fields.get(1).and_then(|s| s.parse::<u64>().ok()), Some(10000));
                assert_eq!(fields.get(2).and_then(|s| s.parse::<u64>().ok()), Some(500));
                assert_eq!(fields.get(3).and_then(|s| s.parse::<u64>().ok()), Some(3000));
                assert_eq!(fields.get(4).and_then(|s| s.parse::<u64>().ok()), Some(80000));
                assert_eq!(fields.get(5).and_then(|s| s.parse::<u64>().ok()), Some(1000));
                break;
            }
        }
    }

    #[test]
    fn test_parse_stat_counters() {
        let content = SAMPLE_STAT;
        for line in content.lines() {
            if line.starts_with("ctxt ") {
                let fields: Vec<&str> = line.split_whitespace().collect();
                assert_eq!(fields.get(1).and_then(|s| s.parse::<u64>().ok()), Some(1_200_000));
            } else if line.starts_with("processes ") {
                let fields: Vec<&str> = line.split_whitespace().collect();
                assert_eq!(fields.get(1).and_then(|s| s.parse::<u64>().ok()), Some(5000));
            } else if line.starts_with("procs_running ") {
                let fields: Vec<&str> = line.split_whitespace().collect();
                assert_eq!(fields.get(1).and_then(|s| s.parse::<u64>().ok()), Some(3));
            }
        }
    }

    // -- Diskstats parsing --

    #[test]
    fn test_parse_diskstats_line() {
        let line = "   8       0 sda 10000 500 200000 5000 8000 300 150000 3000 2 4000 8000";
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 14);
        assert_eq!(fields.get(2), Some(&"sda"));
        assert_eq!(fields.get(3).and_then(|s| s.parse::<u64>().ok()), Some(10000));
        assert_eq!(fields.get(5).and_then(|s| s.parse::<u64>().ok()), Some(200000));
        assert_eq!(fields.get(7).and_then(|s| s.parse::<u64>().ok()), Some(8000));
    }

    #[test]
    fn test_parse_diskstats_short_line() {
        // Lines with fewer than 14 fields should be skipped.
        let line = "   8       0 sda 10000 500";
        let fields: Vec<&str> = line.split_whitespace().collect();
        assert!(fields.len() < 14);
    }
}
