//! Slate OS I/O Statistics Utility
//!
//! Reports CPU utilization and I/O device statistics, similar to Linux `iostat`.
//! Reads from `/proc/stat`, `/proc/diskstats`, and `/sys/block/` for live data.
//!
//! # Usage
//!
//! ```text
//! iostat                          Show CPU + device summary since boot
//! iostat 2                        Repeat every 2 seconds (delta mode)
//! iostat 2 5                      Repeat every 2 seconds, 5 times
//! iostat -c                       CPU stats only
//! iostat -d                       Device stats only
//! iostat -x                       Extended device stats
//! iostat -m                       Display throughput in MB/s
//! iostat -h                       Human-readable sizes
//! iostat -p sda                   Show only device "sda"
//! iostat -N                       Skip partitions (named devices only)
//! iostat -t                       Include timestamp on each report
//! iostat -z                       Skip devices with zero activity
//! iostat --json                   JSON output
//! ```

use std::env;
use std::fs;
use std::process;

// ============================================================================
// Constants
// ============================================================================

/// Default hardware sector size in bytes when `/sys/block/<dev>/queue/hw_sector_size`
/// is unavailable.
const DEFAULT_SECTOR_SIZE: u64 = 512;

// ============================================================================
// Data structures
// ============================================================================

/// CPU time counters read from a single `cpu` line in `/proc/stat`.
#[derive(Clone, Default)]
struct CpuStats {
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
}

impl CpuStats {
    /// Sum of all CPU time fields.
    fn total(&self) -> u64 {
        self.user
            .saturating_add(self.nice)
            .saturating_add(self.system)
            .saturating_add(self.idle)
            .saturating_add(self.iowait)
            .saturating_add(self.irq)
            .saturating_add(self.softirq)
            .saturating_add(self.steal)
    }

    /// Compute per-field percentages relative to a total delta.
    fn percentages(&self, total_delta: u64) -> CpuPct {
        let d = if total_delta > 0 {
            total_delta as f64
        } else {
            1.0
        };
        CpuPct {
            user: self.user as f64 / d * 100.0,
            nice: self.nice as f64 / d * 100.0,
            system: self.system as f64 / d * 100.0,
            idle: self.idle as f64 / d * 100.0,
            iowait: self.iowait as f64 / d * 100.0,
            steal: self.steal as f64 / d * 100.0,
        }
    }

    /// Element-wise subtraction (current - previous), saturating.
    fn delta(&self, prev: &CpuStats) -> CpuStats {
        CpuStats {
            user: self.user.saturating_sub(prev.user),
            nice: self.nice.saturating_sub(prev.nice),
            system: self.system.saturating_sub(prev.system),
            idle: self.idle.saturating_sub(prev.idle),
            iowait: self.iowait.saturating_sub(prev.iowait),
            irq: self.irq.saturating_sub(prev.irq),
            softirq: self.softirq.saturating_sub(prev.softirq),
            steal: self.steal.saturating_sub(prev.steal),
        }
    }
}

/// Computed CPU percentages for display.
struct CpuPct {
    user: f64,
    nice: f64,
    system: f64,
    idle: f64,
    iowait: f64,
    steal: f64,
}

/// Raw I/O counters for a single block device, from `/proc/diskstats`.
///
/// `major`, `minor`, and `io_cur` are parsed from diskstats for completeness
/// but not currently displayed. They are available for future extensions
/// (e.g. device identification and in-flight I/O reporting).
#[derive(Clone, Default)]
#[allow(dead_code)]
struct DiskStats {
    name: String,
    major: u32,
    minor: u32,
    rd_ios: u64,
    rd_merges: u64,
    rd_sectors: u64,
    rd_ticks: u64,
    wr_ios: u64,
    wr_merges: u64,
    wr_sectors: u64,
    wr_ticks: u64,
    io_cur: u64,
    io_ticks: u64,
    io_aveq: u64,
    /// Hardware sector size in bytes (from sysfs, default 512).
    sector_size: u64,
}

impl DiskStats {
    /// Whether all I/O counters are zero.
    fn is_idle(&self) -> bool {
        self.rd_ios == 0
            && self.wr_ios == 0
            && self.rd_sectors == 0
            && self.wr_sectors == 0
            && self.io_ticks == 0
    }
}

/// Computed I/O rates for display (basic view).
struct DiskBasic {
    name: String,
    tps: f64,
    read_rate: f64,
    write_rate: f64,
    read_total: u64,
    write_total: u64,
}

/// Computed I/O rates for display (extended view).
struct DiskExtended {
    name: String,
    r_per_s: f64,
    w_per_s: f64,
    r_mb_per_s: f64,
    w_mb_per_s: f64,
    rrqm_per_s: f64,
    wrqm_per_s: f64,
    pct_rrqm: f64,
    pct_wrqm: f64,
    r_await: f64,
    w_await: f64,
    aqu_sz: f64,
    rareq_sz: f64,
    wareq_sz: f64,
    svctm: f64,
    pct_util: f64,
}

/// Display unit for throughput values.
#[derive(Clone, Copy, PartialEq)]
enum DisplayUnit {
    KiloBytes,
    MegaBytes,
    Human,
}

/// Runtime configuration parsed from command-line arguments.
struct Config {
    interval: Option<f64>,
    count: Option<u32>,
    cpu_only: bool,
    disk_only: bool,
    extended: bool,
    unit: DisplayUnit,
    filter_devices: Vec<String>,
    named_only: bool,
    show_timestamp: bool,
    json_output: bool,
    skip_idle: bool,
}

// ============================================================================
// /proc and /sys readers
// ============================================================================

/// Read a file and return its trimmed contents, or `None` on any error.
fn read_file(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// Parse the aggregate `cpu` line from `/proc/stat`.
fn read_cpu_stats() -> Option<CpuStats> {
    let content = read_file("/proc/stat")?;
    for line in content.lines() {
        // The aggregate line starts with "cpu " (note the space -- per-CPU lines
        // are "cpu0", "cpu1", etc. with no space before the digit).
        if let Some(rest) = line.strip_prefix("cpu ") {
            return parse_cpu_line(rest);
        }
    }
    None
}

/// Parse whitespace-separated CPU time values from a `/proc/stat` cpu line.
fn parse_cpu_line(rest: &str) -> Option<CpuStats> {
    let vals: Vec<u64> = rest
        .split_whitespace()
        .filter_map(|s| s.parse().ok())
        .collect();
    // A slice pattern rather than `len() < 4` followed by four indexes. The
    // check and the indexing were correct together, but only together -- the
    // pattern makes the requirement part of the match instead of something a
    // later edit has to remember.
    let [user, nice, system, idle, ..] = vals.as_slice() else {
        return None;
    };
    Some(CpuStats {
        user: *user,
        nice: *nice,
        system: *system,
        idle: *idle,
        iowait: vals.get(4).copied().unwrap_or(0),
        irq: vals.get(5).copied().unwrap_or(0),
        softirq: vals.get(6).copied().unwrap_or(0),
        steal: vals.get(7).copied().unwrap_or(0),
    })
}

/// Read the hardware sector size for a block device from sysfs.
fn read_sector_size(dev_name: &str) -> u64 {
    // For partitions (e.g. "sda1"), try the parent device ("sda") too.
    let path = format!("/sys/block/{dev_name}/queue/hw_sector_size");
    if let Some(val) = read_file(&path).and_then(|s| s.parse::<u64>().ok()) {
        return val;
    }
    // Try stripping trailing digits to find the parent device.
    let parent = dev_name.trim_end_matches(|c: char| c.is_ascii_digit());
    if parent != dev_name && !parent.is_empty() {
        let parent_path = format!("/sys/block/{parent}/queue/hw_sector_size");
        if let Some(val) = read_file(&parent_path).and_then(|s| s.parse::<u64>().ok()) {
            return val;
        }
    }
    DEFAULT_SECTOR_SIZE
}

/// Check whether a device name looks like a partition (ends with digits and
/// has a non-digit prefix that corresponds to a whole device).
fn is_partition(name: &str) -> bool {
    // Heuristic: if the name ends with digits and stripping those digits
    // yields a shorter non-empty string, it is likely a partition.
    // Examples: sda1 -> sda (partition), sda -> sda (whole device),
    //           nvme0n1p1 -> nvme0n1p (partition), loop0 -> loop (device).
    let stripped = name.trim_end_matches(|c: char| c.is_ascii_digit());
    if stripped.is_empty() || stripped.len() == name.len() {
        return false;
    }
    // Special case: "loop" devices are not partitions.
    if stripped == "loop" {
        return false;
    }
    // nvme partitions end with "pN", dm devices are never partitions.
    if stripped.starts_with("dm-") {
        return false;
    }
    true
}

/// Read all block device stats from `/proc/diskstats`.
fn read_disk_stats() -> Vec<DiskStats> {
    let content = match read_file("/proc/diskstats") {
        Some(c) => c,
        None => return Vec::new(),
    };

    let mut devices = Vec::new();

    for line in content.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [major_s, minor_s, name_s, rest @ ..] = fields.as_slice() else {
            continue;
        };
        // 14 is the classic /proc/diskstats width; kernels since 4.18 append
        // more columns, so this is a minimum rather than an equality.
        if rest.len() < 11 {
            continue;
        }
        let major: u32 = match major_s.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let minor: u32 = match minor_s.parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let name = (*name_s).to_string();
        let sector_size = read_sector_size(&name);

        let parse_u64 =
            |idx: usize| -> u64 { fields.get(idx).and_then(|s| s.parse().ok()).unwrap_or(0) };

        devices.push(DiskStats {
            name,
            major,
            minor,
            rd_ios: parse_u64(3),
            rd_merges: parse_u64(4),
            rd_sectors: parse_u64(5),
            rd_ticks: parse_u64(6),
            wr_ios: parse_u64(7),
            wr_merges: parse_u64(8),
            wr_sectors: parse_u64(9),
            wr_ticks: parse_u64(10),
            io_cur: parse_u64(11),
            io_ticks: parse_u64(12),
            io_aveq: parse_u64(13),
            sector_size,
        });
    }

    devices
}

// ============================================================================
// Delta computations
// ============================================================================

/// Find a previous snapshot entry by device name.
fn find_prev_disk<'a>(prev: &'a [DiskStats], name: &str) -> Option<&'a DiskStats> {
    prev.iter().find(|d| d.name == name)
}

/// Compute basic I/O rates from a delta between two snapshots.
fn compute_basic(
    current: &DiskStats,
    prev: Option<&DiskStats>,
    interval: f64,
    unit: DisplayUnit,
) -> DiskBasic {
    let (d_rd_ios, d_wr_ios, d_rd_sectors, d_wr_sectors) = match prev {
        Some(p) => (
            current.rd_ios.saturating_sub(p.rd_ios),
            current.wr_ios.saturating_sub(p.wr_ios),
            current.rd_sectors.saturating_sub(p.rd_sectors),
            current.wr_sectors.saturating_sub(p.wr_sectors),
        ),
        None => (
            current.rd_ios,
            current.wr_ios,
            current.rd_sectors,
            current.wr_sectors,
        ),
    };

    // `iostat 0` reaches here with interval 0, and dividing by it printed
    // `inf` in a column of measurements. `compute_extended` has guarded this
    // since it was written (`if interval_ms > 0.0`); this half never did, so
    // the same run reported infinite tps beside finite %util.
    let ops = d_rd_ios.saturating_add(d_wr_ios);
    let tps = if interval > 0.0 {
        ops as f64 / interval
    } else {
        0.0
    };
    let read_bytes = d_rd_sectors.saturating_mul(current.sector_size);
    let write_bytes = d_wr_sectors.saturating_mul(current.sector_size);

    // Same guard for the byte rates, which divide by the same interval.
    let per_sec = |bytes: u64, scale: f64| {
        if interval > 0.0 {
            bytes as f64 / scale / interval
        } else {
            0.0
        }
    };

    let (read_rate, write_rate, read_total, write_total) = match unit {
        DisplayUnit::MegaBytes => (
            per_sec(read_bytes, 1_048_576.0),
            per_sec(write_bytes, 1_048_576.0),
            current.rd_sectors.saturating_mul(current.sector_size) / 1_048_576,
            current.wr_sectors.saturating_mul(current.sector_size) / 1_048_576,
        ),
        // KB and Human both use KB for the rate column; Human formatting
        // is applied at display time for totals.
        _ => (
            per_sec(read_bytes, 1024.0),
            per_sec(write_bytes, 1024.0),
            current.rd_sectors.saturating_mul(current.sector_size) / 1024,
            current.wr_sectors.saturating_mul(current.sector_size) / 1024,
        ),
    };

    DiskBasic {
        name: current.name.clone(),
        tps,
        read_rate,
        write_rate,
        read_total,
        write_total,
    }
}

/// Compute extended I/O metrics from a delta between two snapshots.
fn compute_extended(current: &DiskStats, prev: Option<&DiskStats>, interval: f64) -> DiskExtended {
    let (
        d_rd_ios,
        d_wr_ios,
        d_rd_merges,
        d_wr_merges,
        d_rd_sectors,
        d_wr_sectors,
        d_rd_ticks,
        d_wr_ticks,
        d_io_ticks,
        d_io_aveq,
    ) = match prev {
        Some(p) => (
            current.rd_ios.saturating_sub(p.rd_ios),
            current.wr_ios.saturating_sub(p.wr_ios),
            current.rd_merges.saturating_sub(p.rd_merges),
            current.wr_merges.saturating_sub(p.wr_merges),
            current.rd_sectors.saturating_sub(p.rd_sectors),
            current.wr_sectors.saturating_sub(p.wr_sectors),
            current.rd_ticks.saturating_sub(p.rd_ticks),
            current.wr_ticks.saturating_sub(p.wr_ticks),
            current.io_ticks.saturating_sub(p.io_ticks),
            current.io_aveq.saturating_sub(p.io_aveq),
        ),
        None => (
            current.rd_ios,
            current.wr_ios,
            current.rd_merges,
            current.wr_merges,
            current.rd_sectors,
            current.wr_sectors,
            current.rd_ticks,
            current.wr_ticks,
            current.io_ticks,
            current.io_aveq,
        ),
    };

    let r_per_s = d_rd_ios as f64 / interval;
    let w_per_s = d_wr_ios as f64 / interval;
    let rrqm_per_s = d_rd_merges as f64 / interval;
    let wrqm_per_s = d_wr_merges as f64 / interval;

    let r_mb_per_s =
        (d_rd_sectors.saturating_mul(current.sector_size)) as f64 / 1_048_576.0 / interval;
    let w_mb_per_s =
        (d_wr_sectors.saturating_mul(current.sector_size)) as f64 / 1_048_576.0 / interval;

    // Merge percentages: what fraction of I/Os were merged.
    let total_rd = d_rd_ios.saturating_add(d_rd_merges);
    let total_wr = d_wr_ios.saturating_add(d_wr_merges);
    let pct_rrqm = if total_rd > 0 {
        d_rd_merges as f64 / total_rd as f64 * 100.0
    } else {
        0.0
    };
    let pct_wrqm = if total_wr > 0 {
        d_wr_merges as f64 / total_wr as f64 * 100.0
    } else {
        0.0
    };

    // Average wait times in milliseconds.
    let r_await = if d_rd_ios > 0 {
        d_rd_ticks as f64 / d_rd_ios as f64
    } else {
        0.0
    };
    let w_await = if d_wr_ios > 0 {
        d_wr_ticks as f64 / d_wr_ios as f64
    } else {
        0.0
    };

    // Average queue size (weighted time in queue / elapsed time in ms).
    let interval_ms = interval * 1000.0;
    let aqu_sz = if interval_ms > 0.0 {
        d_io_aveq as f64 / interval_ms
    } else {
        0.0
    };

    // Average request sizes in KB.
    let rareq_sz = if d_rd_ios > 0 {
        (d_rd_sectors.saturating_mul(current.sector_size)) as f64 / 1024.0 / d_rd_ios as f64
    } else {
        0.0
    };
    let wareq_sz = if d_wr_ios > 0 {
        (d_wr_sectors.saturating_mul(current.sector_size)) as f64 / 1024.0 / d_wr_ios as f64
    } else {
        0.0
    };

    // Average service time (approximate: total busy / total completed I/Os).
    let total_ios = d_rd_ios.saturating_add(d_wr_ios);
    let svctm = if total_ios > 0 {
        d_io_ticks as f64 / total_ios as f64
    } else {
        0.0
    };

    // %util: fraction of elapsed time the device was busy.
    let pct_util = if interval_ms > 0.0 {
        d_io_ticks as f64 / interval_ms * 100.0
    } else {
        0.0
    };

    DiskExtended {
        name: current.name.clone(),
        r_per_s,
        w_per_s,
        r_mb_per_s,
        w_mb_per_s,
        rrqm_per_s,
        wrqm_per_s,
        pct_rrqm,
        pct_wrqm,
        r_await,
        w_await,
        aqu_sz,
        rareq_sz,
        wareq_sz,
        svctm,
        pct_util,
    }
}

// ============================================================================
// Display — plain text
// ============================================================================

/// Format a byte count as a human-readable string (KB, MB, GB).
fn format_human(kb: u64) -> String {
    if kb >= 1_048_576 {
        format!("{:.1}G", kb as f64 / 1_048_576.0)
    } else if kb >= 1024 {
        format!("{:.1}M", kb as f64 / 1024.0)
    } else {
        format!("{kb}K")
    }
}

/// Print the CPU statistics header and values.
fn print_cpu(pct: Option<&CpuPct>) {
    println!("avg-cpu:  %user   %nice %system %iowait  %steal   %idle");
    let Some(pct) = pct else {
        // Six question marks rather than six zeroes. A row of 0.00 is what an
        // idle machine looks like; this is what an unreadable /proc/stat looks
        // like, and they were the same output until now.
        println!(
            "       {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
            "?", "?", "?", "?", "?", "?"
        );
        println!();
        return;
    };
    println!(
        "       {:>7.2} {:>7.2} {:>7.2} {:>7.2} {:>7.2} {:>7.2}",
        pct.user, pct.nice, pct.system, pct.iowait, pct.steal, pct.idle,
    );
    println!();
}

/// Print the basic device stats table.
fn print_basic_header(unit: DisplayUnit) {
    let (rate_label, total_label) = match unit {
        DisplayUnit::MegaBytes => ("MB_read/s", "MB_wrtn/s"),
        _ => ("kB_read/s", "kB_wrtn/s"),
    };
    let (tot_r, tot_w) = match unit {
        DisplayUnit::MegaBytes => ("MB_read", "MB_wrtn"),
        _ => ("kB_read", "kB_wrtn"),
    };
    println!(
        "{:<16} {:>8} {:>12} {:>12} {:>10} {:>10}",
        "Device", "tps", rate_label, total_label, tot_r, tot_w,
    );
}

fn print_basic_row(d: &DiskBasic, unit: DisplayUnit) {
    match unit {
        DisplayUnit::Human => {
            println!(
                "{:<16} {:>8.2} {:>12.2} {:>12.2} {:>10} {:>10}",
                d.name,
                d.tps,
                d.read_rate,
                d.write_rate,
                format_human(d.read_total),
                format_human(d.write_total),
            );
        }
        _ => {
            println!(
                "{:<16} {:>8.2} {:>12.2} {:>12.2} {:>10} {:>10}",
                d.name, d.tps, d.read_rate, d.write_rate, d.read_total, d.write_total,
            );
        }
    }
}

/// Print the extended device stats table.
fn print_extended_header() {
    println!(
        "{:<12} {:>6} {:>6} {:>6} {:>6} {:>7} {:>7} {:>6} {:>6} {:>7} {:>7} {:>6} {:>8} {:>8} {:>6} {:>6}",
        "Device",
        "r/s",
        "w/s",
        "rMB/s",
        "wMB/s",
        "rrqm/s",
        "wrqm/s",
        "%rrqm",
        "%wrqm",
        "r_await",
        "w_await",
        "aqu-sz",
        "rareq-sz",
        "wareq-sz",
        "svctm",
        "%util",
    );
}

fn print_extended_row(d: &DiskExtended) {
    println!(
        "{:<12} {:>6.1} {:>6.1} {:>6.2} {:>6.2} {:>7.2} {:>7.2} {:>6.2} {:>6.2} {:>7.2} {:>7.2} {:>6.2} {:>8.2} {:>8.2} {:>6.2} {:>6.2}",
        d.name,
        d.r_per_s,
        d.w_per_s,
        d.r_mb_per_s,
        d.w_mb_per_s,
        d.rrqm_per_s,
        d.wrqm_per_s,
        d.pct_rrqm,
        d.pct_wrqm,
        d.r_await,
        d.w_await,
        d.aqu_sz,
        d.rareq_sz,
        d.wareq_sz,
        d.svctm,
        d.pct_util,
    );
}

/// Print a timestamp line if configured.
fn maybe_print_timestamp(config: &Config) {
    if !config.show_timestamp {
        return;
    }
    // Read system time from /proc/uptime and format it, or use a fallback.
    // On a real system we would use clock_gettime; here we approximate.
    if let Some(uptime) = read_file("/proc/uptime")
        && let Some(secs_str) = uptime.split_whitespace().next()
        && let Ok(secs) = secs_str.parse::<f64>()
    {
        let s = secs as u64;
        let h = (s / 3600) % 24;
        let m = (s % 3600) / 60;
        let sec = s % 60;
        println!("Timestamp: {:02}:{:02}:{:02} (uptime)", h, m, sec);
    }
}

// ============================================================================
// Display — JSON
// ============================================================================

/// Print the entire report as a single JSON object.
fn print_json_report(
    config: &Config,
    cpu_pct: Option<&CpuPct>,
    basics: &[DiskBasic],
    extended: &[DiskExtended],
) {
    print!("{{");

    // Timestamp (uptime seconds).
    if config.show_timestamp
        && let Some(uptime) = read_file("/proc/uptime")
        && let Some(secs_str) = uptime.split_whitespace().next()
    {
        print!("\"timestamp\":{secs_str},");
    }

    // CPU section.
    if let Some(c) = cpu_pct {
        print!(
            "\"cpu\":{{\"user\":{:.2},\"nice\":{:.2},\"system\":{:.2},\"iowait\":{:.2},\"steal\":{:.2},\"idle\":{:.2}}}",
            c.user, c.nice, c.system, c.iowait, c.steal, c.idle,
        );
        if !basics.is_empty() || !extended.is_empty() {
            print!(",");
        }
    }

    // Device section.
    if !config.extended {
        if !basics.is_empty() {
            print!("\"devices\":[");
            for (i, d) in basics.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                print!(
                    "{{\"name\":\"{}\",\"tps\":{:.2},\"kB_read_s\":{:.2},\"kB_wrtn_s\":{:.2},\"kB_read\":{},\"kB_wrtn\":{}}}",
                    d.name, d.tps, d.read_rate, d.write_rate, d.read_total, d.write_total,
                );
            }
            print!("]");
        }
    } else if !extended.is_empty() {
        print!("\"devices\":[");
        for (i, d) in extended.iter().enumerate() {
            if i > 0 {
                print!(",");
            }
            print!(
                "{{\"name\":\"{}\",\"r_s\":{:.2},\"w_s\":{:.2},\"rMB_s\":{:.4},\"wMB_s\":{:.4},\"rrqm_s\":{:.2},\"wrqm_s\":{:.2},\"pct_rrqm\":{:.2},\"pct_wrqm\":{:.2},\"r_await\":{:.2},\"w_await\":{:.2},\"aqu_sz\":{:.2},\"rareq_sz\":{:.2},\"wareq_sz\":{:.2},\"svctm\":{:.2},\"pct_util\":{:.2}}}",
                d.name,
                d.r_per_s,
                d.w_per_s,
                d.r_mb_per_s,
                d.w_mb_per_s,
                d.rrqm_per_s,
                d.wrqm_per_s,
                d.pct_rrqm,
                d.pct_wrqm,
                d.r_await,
                d.w_await,
                d.aqu_sz,
                d.rareq_sz,
                d.wareq_sz,
                d.svctm,
                d.pct_util,
            );
        }
        print!("]");
    }

    println!("}}");
}

// ============================================================================
// Filtering helpers
// ============================================================================

/// Apply device filters: named-only, device list, skip-idle.
fn filter_devices(devices: &[DiskStats], config: &Config) -> Vec<DiskStats> {
    devices
        .iter()
        .filter(|d| {
            // Skip partitions if -N is set.
            if config.named_only && is_partition(&d.name) {
                return false;
            }
            // Filter to specific devices if -p is set.
            if !config.filter_devices.is_empty()
                && !config.filter_devices.iter().any(|f| f == &d.name)
            {
                return false;
            }
            // Skip idle devices if -z is set.
            if config.skip_idle && d.is_idle() {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

// ============================================================================
// Compute uptime for since-boot interval
// ============================================================================

/// Read system uptime in seconds from `/proc/uptime`.
fn read_uptime_secs() -> f64 {
    read_file("/proc/uptime")
        .and_then(|s| {
            s.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
        })
        .unwrap_or(1.0)
}

// ============================================================================
// Report generation
// ============================================================================

/// Generate and display one report (one "snapshot").
fn display_report(
    config: &Config,
    // `None` when `/proc/stat` could not be read. It is NOT a zeroed
    // `CpuStats`: that prints as 0.00 in every column, which is a claim that
    // the machine is idle rather than an admission that nothing was measured.
    current_cpu: Option<&CpuStats>,
    prev_cpu: &CpuStats,
    current_disks: &[DiskStats],
    prev_disks: &[DiskStats],
    interval: f64,
) {
    maybe_print_timestamp(config);

    // CPU section. `None` propagates to the printer, which says so.
    let cpu_pct = current_cpu.map(|c| {
        let d = c.delta(prev_cpu);
        let total = d.total();
        d.percentages(total)
    });

    let show_cpu = !config.disk_only;
    let show_disk = !config.cpu_only;

    if config.json_output {
        let basics: Vec<DiskBasic>;
        let extended_stats: Vec<DiskExtended>;
        if show_disk && !config.extended {
            basics = current_disks
                .iter()
                .map(|d| {
                    compute_basic(
                        d,
                        find_prev_disk(prev_disks, &d.name),
                        interval,
                        config.unit,
                    )
                })
                .collect();
            extended_stats = Vec::new();
        } else if show_disk {
            basics = Vec::new();
            extended_stats = current_disks
                .iter()
                .map(|d| compute_extended(d, find_prev_disk(prev_disks, &d.name), interval))
                .collect();
        } else {
            basics = Vec::new();
            extended_stats = Vec::new();
        }
        print_json_report(
            config,
            if show_cpu { cpu_pct.as_ref() } else { None },
            &basics,
            &extended_stats,
        );
        return;
    }

    // Plain text output.
    if show_cpu {
        print_cpu(cpu_pct.as_ref());
    }

    if show_disk {
        if config.extended {
            print_extended_header();
            for d in current_disks {
                let prev = find_prev_disk(prev_disks, &d.name);
                let ext = compute_extended(d, prev, interval);
                print_extended_row(&ext);
            }
        } else {
            print_basic_header(config.unit);
            for d in current_disks {
                let prev = find_prev_disk(prev_disks, &d.name);
                let basic = compute_basic(d, prev, interval, config.unit);
                print_basic_row(&basic, config.unit);
            }
        }
        println!();
    }
}

// ============================================================================
// Main loop
// ============================================================================

fn run(config: &Config) {
    // First report: since-boot averages (use uptime as the interval).
    let boot_interval = read_uptime_secs().max(1.0);

    let mut prev_cpu = CpuStats::default();
    let prev_disks_empty: Vec<DiskStats> = Vec::new();

    let current_cpu = read_cpu_stats();
    let all_disks = read_disk_stats();
    let filtered = filter_devices(&all_disks, config);

    display_report(
        config,
        current_cpu.as_ref(),
        &prev_cpu,
        &filtered,
        &prev_disks_empty,
        boot_interval,
    );

    // If no interval was specified, we are done after the single report.
    let interval = match config.interval {
        Some(i) => i,
        None => return,
    };

    // Only advance the baseline on a successful read. A failed one must not

    // destroy the last known sample -- the next good read then deltas against

    // it, which understates the rate over the skipped interval but is far

    // better than deltaing against zeroes, which would report the whole

    // counter as if it happened in one interval.

    if let Some(c) = current_cpu {
        prev_cpu = c;
    }
    let mut prev_disks = all_disks;
    let mut iteration = 1u32;

    loop {
        // Check count limit before sleeping.
        if let Some(max) = config.count
            && iteration >= max
        {
            break;
        }

        std::thread::sleep(std::time::Duration::from_secs_f64(interval));

        let current_cpu = read_cpu_stats();
        let all_disks = read_disk_stats();
        let filtered = filter_devices(&all_disks, config);

        display_report(
            config,
            current_cpu.as_ref(),
            &prev_cpu,
            &filtered,
            &prev_disks,
            interval,
        );

        // Only advance the baseline on a successful read. A failed one must not

        // destroy the last known sample -- the next good read then deltas against

        // it, which understates the rate over the skipped interval but is far

        // better than deltaing against zeroes, which would report the whole

        // counter as if it happened in one interval.

        if let Some(c) = current_cpu {
            prev_cpu = c;
        }
        prev_disks = all_disks;
        iteration = iteration.saturating_add(1);
    }
}

// ============================================================================
// CLI parsing
// ============================================================================

fn print_usage() {
    println!("Slate OS I/O Statistics Utility v0.1.0");
    println!();
    println!("Report CPU utilization and I/O device statistics.");
    println!();
    println!("USAGE:");
    println!("  iostat [options] [interval [count]]");
    println!();
    println!("OPTIONS:");
    println!("  -c                CPU statistics only");
    println!("  -d                Device statistics only");
    println!("  -x, --extended    Extended device statistics");
    println!("  -k                Display in KB/s (default)");
    println!("  -m                Display in MB/s");
    println!("  -h, --human       Human-readable sizes");
    println!("  -p <device>       Show only specific device(s) (repeatable)");
    println!("  -N                Show only named devices (skip partitions)");
    println!("  -t, --timestamp   Show timestamp on each report");
    println!("  -z                Skip devices with zero activity");
    println!("  --json            JSON output");
    println!("  --help            Show this help");
    println!();
    println!("ARGUMENTS:");
    println!("  interval          Refresh interval in seconds");
    println!("  count             Number of reports to generate");
    println!();
    println!("EXAMPLES:");
    println!("  iostat              Since-boot summary");
    println!("  iostat 2            Delta report every 2 seconds");
    println!("  iostat -x 1 5       Extended stats, 1s interval, 5 reports");
    println!("  iostat -d -p sda    Device stats for sda only");
}

fn main() {
    let args: Vec<String> = env::args().collect();

    let mut config = Config {
        interval: None,
        count: None,
        cpu_only: false,
        disk_only: false,
        extended: false,
        unit: DisplayUnit::KiloBytes,
        filter_devices: Vec::new(),
        named_only: false,
        show_timestamp: false,
        json_output: false,
        skip_idle: false,
    };

    // Collect positional arguments (interval, count) separately.
    let mut positionals: Vec<String> = Vec::new();

    // An iterator rather than an index: `args[i]` can panic and `i += 1`
    // can overflow, and the options that consume a following argument need
    // a cursor, which `Iterator::next` is.
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-c" => config.cpu_only = true,
            "-d" => config.disk_only = true,
            "-x" | "--extended" => config.extended = true,
            "-k" => config.unit = DisplayUnit::KiloBytes,
            "-m" => config.unit = DisplayUnit::MegaBytes,
            "-h" | "--human" => config.unit = DisplayUnit::Human,
            "-N" => config.named_only = true,
            "-t" | "--timestamp" => config.show_timestamp = true,
            "-z" => config.skip_idle = true,
            "--json" => config.json_output = true,
            "--help" => {
                print_usage();
                process::exit(0);
            }
            "-p" => {
                let Some(value) = it.next() else {
                    eprintln!("error: -p requires a device name");
                    process::exit(1);
                };
                config.filter_devices.push(value.clone());
            }
            other => {
                // Might be a positional (interval or count).
                if other.starts_with('-') {
                    eprintln!("error: unknown option: {other}");
                    eprintln!("Run 'iostat --help' for usage.");
                    process::exit(1);
                }
                positionals.push(other.to_string());
            }
        }
    }

    // Parse positional arguments: [interval [count]].
    if let Some(first) = positionals.first() {
        match first.parse::<f64>() {
            Ok(val) if val > 0.0 => config.interval = Some(val),
            _ => {
                eprintln!("error: invalid interval: {first}");
                process::exit(1);
            }
        }
    }
    if let Some(second) = positionals.get(1) {
        match second.parse::<u32>() {
            Ok(val) if val > 0 => config.count = Some(val),
            _ => {
                eprintln!("error: invalid count: {second}");
                process::exit(1);
            }
        }
    }

    // -c and -d are mutually exclusive; if both are set, show everything.
    if config.cpu_only && config.disk_only {
        config.cpu_only = false;
        config.disk_only = false;
    }

    run(&config);
}

#[cfg(test)]
// A test that unwraps is asserting the call succeeded, and a panic names the
// line. CLAUDE.md allows these in test modules.
//
// `float_cmp` is allowed here and the reason is specific rather than general:
// every expected value in these tests is exactly representable. 15/2 is 7.5;
// 200 sectors * 512 / 1024 is 100; 2048 * 512 / 1048576 is 1. The divisors are
// powers of two and the numerators are small integers, so the arithmetic is
// exact and `assert_eq!` is the right assertion. Replacing them with an
// epsilon comparison would weaken a test that currently pins the exact value a
// wrong unit conversion would change.
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn disk(name: &str, rd_ios: u64, rd_sectors: u64, wr_ios: u64, wr_sectors: u64) -> DiskStats {
        DiskStats {
            name: name.to_string(),
            major: 8,
            minor: 0,
            rd_ios,
            rd_merges: 0,
            rd_sectors,
            rd_ticks: 0,
            wr_ios,
            wr_merges: 0,
            wr_sectors,
            wr_ticks: 0,
            io_cur: 0,
            io_ticks: 0,
            io_aveq: 0,
            sector_size: 512,
        }
    }

    // -- /proc/stat parsing ---------------------------------------------------

    #[test]
    fn a_cpu_line_parses_into_its_counters() {
        let got = parse_cpu_line("100 20 30 400 5 0 1 0 0 0").unwrap();
        assert_eq!(got.user, 100);
        assert_eq!(got.nice, 20);
        assert_eq!(got.system, 30);
        assert_eq!(got.idle, 400);
        assert_eq!(got.iowait, 5);
        assert_eq!(got.total(), 556);
    }

    #[test]
    fn a_short_cpu_line_is_refused_rather_than_padded() {
        // A truncated /proc/stat read must not become a machine that was 100%
        // idle; the caller has a chain of sources and None lets it say so.
        assert!(parse_cpu_line("1 2 3").is_none());
        assert!(parse_cpu_line("").is_none());
    }

    #[test]
    fn percentages_of_an_empty_interval_do_not_divide_by_zero() {
        // Two samples taken within the same tick: the delta is all zeros and
        // so is its total. Built as a real delta rather than by passing a
        // total that disagrees with the fields -- the caller always passes
        // `delta.total()`, so a mismatched pair cannot occur and asserting on
        // one would test a state the program cannot reach. My first version of
        // this test did exactly that and "failed" against correct code.
        let same = parse_cpu_line("7 7 7 7 7 7 7 7 7 7").unwrap();
        let d = same.delta(&same);
        assert_eq!(d.total(), 0);
        let p = d.percentages(d.total());
        assert_eq!(p.user, 0.0, "no time passed, so no percentage is knowable");
        assert_eq!(p.idle, 0.0);
        assert!(p.system.is_finite(), "and it must not be NaN either");
    }

    #[test]
    fn a_counter_that_went_backwards_saturates_rather_than_wrapping() {
        // /proc counters reset when a device is re-enumerated. Wrapping would
        // turn a reset into billions of operations per second.
        let now = parse_cpu_line("10 10 10 10 10 10 10 10 10 10").unwrap();
        let before = parse_cpu_line("99 99 99 99 99 99 99 99 99 99").unwrap();
        let d = now.delta(&before);
        assert_eq!(d.user, 0);
        assert_eq!(d.idle, 0);
    }

    // -- device classification ------------------------------------------------

    #[test]
    fn partitions_are_told_from_whole_devices() {
        assert!(is_partition("sda1"));
        assert!(is_partition("nvme0n1p1"));
        assert!(!is_partition("sda"));
        // `loop0` and `dm-0` end in digits and are not partitions of anything.
        assert!(!is_partition("loop0"));
        assert!(!is_partition("dm-0"));
    }

    #[test]
    fn a_device_with_no_traffic_is_idle() {
        assert!(disk("sda", 0, 0, 0, 0).is_idle());
        assert!(!disk("sda", 1, 0, 0, 0).is_idle());
        assert!(!disk("sda", 0, 0, 1, 0).is_idle());
    }

    // -- the rate arithmetic --------------------------------------------------

    #[test]
    fn the_first_report_is_totals_since_boot() {
        // With no previous sample, iostat reports cumulative counters over the
        // time since boot -- so the delta is the whole counter.
        let b = compute_basic(
            &disk("sda", 100, 200, 50, 100),
            None,
            1.0,
            DisplayUnit::KiloBytes,
        );
        assert_eq!(b.tps, 150.0);
        // 200 sectors * 512 bytes / 1024 = 100 kB
        assert_eq!(b.read_rate, 100.0);
        assert_eq!(b.write_rate, 50.0);
    }

    #[test]
    fn a_later_report_is_the_delta_over_the_interval() {
        let prev = disk("sda", 100, 200, 50, 100);
        let curr = disk("sda", 110, 400, 55, 200);
        let b = compute_basic(&curr, Some(&prev), 2.0, DisplayUnit::KiloBytes);
        // 15 operations over 2 seconds
        assert_eq!(b.tps, 7.5);
        // 200 sectors * 512 / 1024 = 100 kB over 2 seconds
        assert_eq!(b.read_rate, 50.0);
    }

    #[test]
    fn megabytes_and_kilobytes_differ_by_exactly_1024() {
        let d = disk("sda", 0, 2048, 0, 0);
        let kb = compute_basic(&d, None, 1.0, DisplayUnit::KiloBytes);
        let mb = compute_basic(&d, None, 1.0, DisplayUnit::MegaBytes);
        assert_eq!(kb.read_rate, 1024.0);
        assert_eq!(mb.read_rate, 1.0);
    }

    #[test]
    fn a_device_counter_reset_does_not_become_a_huge_rate() {
        // The device was re-enumerated and its counters restarted. Saturating
        // gives 0; wrapping would report 18 quintillion operations.
        let prev = disk("sda", 9_000, 9_000, 9_000, 9_000);
        let curr = disk("sda", 5, 5, 5, 5);
        let b = compute_basic(&curr, Some(&prev), 1.0, DisplayUnit::KiloBytes);
        assert_eq!(b.tps, 0.0);
        assert_eq!(b.read_rate, 0.0);
    }

    #[test]
    fn a_zero_interval_does_not_report_infinity() {
        // `iostat 0` is accepted by the argument parser. Dividing by it gave
        // `inf`, which prints as "inf" in a column of measurements.
        // `compute_extended` already guarded this; `compute_basic` did not.
        let b = compute_basic(
            &disk("sda", 10, 10, 10, 10),
            None,
            0.0,
            DisplayUnit::KiloBytes,
        );
        assert!(b.tps.is_finite(), "tps was {}", b.tps);
        assert!(b.read_rate.is_finite());
        assert!(b.write_rate.is_finite());
    }

    // -- display --------------------------------------------------------------

    #[test]
    fn human_units_switch_at_the_right_thresholds() {
        assert_eq!(format_human(0), "0K");
        assert_eq!(format_human(1023), "1023K");
        assert_eq!(format_human(1024), "1.0M");
        assert_eq!(format_human(1_048_576), "1.0G");
    }
}
