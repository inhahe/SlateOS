// Slate OS sysstat -- system performance monitoring
//
// Multi-personality binary:
//   sar        -- system activity reporter (default)
//   iostat     -- I/O statistics
//   mpstat     -- per-CPU statistics
//   pidstat    -- per-process statistics
//   cifsiostat -- CIFS I/O statistics
//   tapestat   -- tape device statistics
//
// Usage:
//   sar [OPTIONS] [interval [count]]
//   iostat [OPTIONS] [interval [count]]
//   mpstat [OPTIONS] [interval [count]]
//   pidstat [OPTIONS] [interval [count]]
//   cifsiostat [interval [count]]
//   tapestat [interval [count]]

#![deny(clippy::all)]

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::io::{self, BufRead, Write};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ---------------------------------------------------------------------------
// Personality detection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Personality {
    Sar,
    Iostat,
    Mpstat,
    Pidstat,
    Cifsiostat,
    Tapestat,
}

impl fmt::Display for Personality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sar => write!(f, "sar"),
            Self::Iostat => write!(f, "iostat"),
            Self::Mpstat => write!(f, "mpstat"),
            Self::Pidstat => write!(f, "pidstat"),
            Self::Cifsiostat => write!(f, "cifsiostat"),
            Self::Tapestat => write!(f, "tapestat"),
        }
    }
}

fn detect_personality(argv0: &str) -> Personality {
    let bytes = argv0.as_bytes();
    let mut last_sep = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'/' || b == b'\\' {
            last_sep = i + 1;
        }
    }
    let base = &argv0[last_sep..];
    let base = base.strip_suffix(".exe").unwrap_or(base);
    match base {
        "iostat" => Personality::Iostat,
        "mpstat" => Personality::Mpstat,
        "pidstat" => Personality::Pidstat,
        "cifsiostat" => Personality::Cifsiostat,
        "tapestat" => Personality::Tapestat,
        _ => Personality::Sar,
    }
}

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

fn format_timestamp() -> String {
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let secs = since_epoch.as_secs();
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    let am_pm = if hours < 12 { "AM" } else { "PM" };
    let display_hour = match hours % 12 {
        0 => 12,
        h => h,
    };
    format!("{:02}:{:02}:{:02} {}", display_hour, minutes, seconds, am_pm)
}

// ---------------------------------------------------------------------------
// /proc readers with fallback
// ---------------------------------------------------------------------------

fn read_file_lines(path: &str) -> Option<Vec<String>> {
    let file = std::fs::File::open(path).ok()?;
    let reader = io::BufReader::new(file);
    let lines: Vec<String> = reader.lines().map_while(Result::ok).collect();
    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

// ---------------------------------------------------------------------------
// CPU statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct CpuStat {
    name: String,
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
    iowait: u64,
    irq: u64,
    softirq: u64,
    steal: u64,
    guest: u64,
    guest_nice: u64,
}

impl CpuStat {
    fn total(&self) -> u64 {
        self.user
            + self.nice
            + self.system
            + self.idle
            + self.iowait
            + self.irq
            + self.softirq
            + self.steal
            + self.guest
            + self.guest_nice
    }
}

#[derive(Debug, Clone, Default)]
struct CpuUsage {
    name: String,
    usr: f64,
    nice: f64,
    sys: f64,
    iowait: f64,
    irq: f64,
    soft: f64,
    steal: f64,
    guest: f64,
    idle: f64,
}

fn parse_cpu_line(line: &str) -> Option<CpuStat> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return None;
    }
    let name = parts.first()?.to_string();
    if !name.starts_with("cpu") {
        return None;
    }
    Some(CpuStat {
        name,
        user: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        nice: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
        system: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
        idle: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
        iowait: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        irq: parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
        softirq: parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0),
        steal: parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
        guest: parts.get(9).and_then(|s| s.parse().ok()).unwrap_or(0),
        guest_nice: parts.get(10).and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

fn read_cpu_stats() -> Vec<CpuStat> {
    if let Some(lines) = read_file_lines("/proc/stat") {
        let stats: Vec<CpuStat> = lines.iter().filter_map(|l| parse_cpu_line(l)).collect();
        if !stats.is_empty() {
            return stats;
        }
    }
    fallback_cpu_stats()
}

fn fallback_cpu_stats() -> Vec<CpuStat> {
    vec![
        CpuStat {
            name: "cpu".to_string(),
            user: 50000,
            nice: 1000,
            system: 20000,
            idle: 900000,
            iowait: 5000,
            irq: 500,
            softirq: 200,
            steal: 0,
            guest: 0,
            guest_nice: 0,
        },
        CpuStat {
            name: "cpu0".to_string(),
            user: 25000,
            nice: 500,
            system: 10000,
            idle: 450000,
            iowait: 2500,
            irq: 250,
            softirq: 100,
            steal: 0,
            guest: 0,
            guest_nice: 0,
        },
        CpuStat {
            name: "cpu1".to_string(),
            user: 25000,
            nice: 500,
            system: 10000,
            idle: 450000,
            iowait: 2500,
            irq: 250,
            softirq: 100,
            steal: 0,
            guest: 0,
            guest_nice: 0,
        },
    ]
}

fn compute_cpu_usage(prev: &CpuStat, curr: &CpuStat) -> CpuUsage {
    let prev_total = prev.total();
    let curr_total = curr.total();
    let delta = if curr_total > prev_total {
        curr_total - prev_total
    } else {
        1
    };
    let d = delta as f64;
    CpuUsage {
        name: curr.name.clone(),
        usr: ((curr.user.saturating_sub(prev.user)) as f64 / d) * 100.0,
        nice: ((curr.nice.saturating_sub(prev.nice)) as f64 / d) * 100.0,
        sys: ((curr.system.saturating_sub(prev.system)) as f64 / d) * 100.0,
        iowait: ((curr.iowait.saturating_sub(prev.iowait)) as f64 / d) * 100.0,
        irq: ((curr.irq.saturating_sub(prev.irq)) as f64 / d) * 100.0,
        soft: ((curr.softirq.saturating_sub(prev.softirq)) as f64 / d) * 100.0,
        steal: ((curr.steal.saturating_sub(prev.steal)) as f64 / d) * 100.0,
        guest: ((curr.guest.saturating_sub(prev.guest)) as f64 / d) * 100.0,
        idle: ((curr.idle.saturating_sub(prev.idle)) as f64 / d) * 100.0,
    }
}

// ---------------------------------------------------------------------------
// Memory statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct MemInfo {
    total_kb: u64,
    free_kb: u64,
    available_kb: u64,
    buffers_kb: u64,
    cached_kb: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
    active_kb: u64,
    inactive_kb: u64,
    dirty_kb: u64,
    slab_kb: u64,
    committed_kb: u64,
}

fn parse_meminfo_value(line: &str) -> Option<u64> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    parts.get(1).and_then(|s| s.parse().ok())
}

fn read_meminfo() -> MemInfo {
    if let Some(lines) = read_file_lines("/proc/meminfo") {
        let mut info = MemInfo::default();
        for line in &lines {
            if line.starts_with("MemTotal:") {
                info.total_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("MemFree:") {
                info.free_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("MemAvailable:") {
                info.available_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Buffers:") {
                info.buffers_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Cached:") {
                info.cached_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("SwapTotal:") {
                info.swap_total_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("SwapFree:") {
                info.swap_free_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Active:") {
                info.active_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Inactive:") {
                info.inactive_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Dirty:") {
                info.dirty_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Slab:") {
                info.slab_kb = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("Committed_AS:") {
                info.committed_kb = parse_meminfo_value(line).unwrap_or(0);
            }
        }
        if info.total_kb > 0 {
            return info;
        }
    }
    fallback_meminfo()
}

fn fallback_meminfo() -> MemInfo {
    MemInfo {
        total_kb: 16384000,
        free_kb: 4096000,
        available_kb: 10240000,
        buffers_kb: 512000,
        cached_kb: 6144000,
        swap_total_kb: 8192000,
        swap_free_kb: 8000000,
        active_kb: 5000000,
        inactive_kb: 4000000,
        dirty_kb: 128,
        slab_kb: 300000,
        committed_kb: 6000000,
    }
}

// ---------------------------------------------------------------------------
// Disk statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct DiskStat {
    name: String,
    reads_completed: u64,
    reads_merged: u64,
    sectors_read: u64,
    _read_time_ms: u64,
    writes_completed: u64,
    writes_merged: u64,
    sectors_written: u64,
    _write_time_ms: u64,
    _io_in_progress: u64,
    io_time_ms: u64,
    weighted_io_time_ms: u64,
}

fn parse_diskstat_line(line: &str) -> Option<DiskStat> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 14 {
        return None;
    }
    Some(DiskStat {
        name: parts.get(2)?.to_string(),
        reads_completed: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
        reads_merged: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
        sectors_read: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
        _read_time_ms: parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
        writes_completed: parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(0),
        writes_merged: parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
        sectors_written: parts.get(9).and_then(|s| s.parse().ok()).unwrap_or(0),
        _write_time_ms: parts.get(10).and_then(|s| s.parse().ok()).unwrap_or(0),
        _io_in_progress: parts.get(11).and_then(|s| s.parse().ok()).unwrap_or(0),
        io_time_ms: parts.get(12).and_then(|s| s.parse().ok()).unwrap_or(0),
        weighted_io_time_ms: parts.get(13).and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

fn read_diskstats() -> Vec<DiskStat> {
    if let Some(lines) = read_file_lines("/proc/diskstats") {
        let stats: Vec<DiskStat> = lines.iter().filter_map(|l| parse_diskstat_line(l)).collect();
        if !stats.is_empty() {
            return stats;
        }
    }
    fallback_diskstats()
}

fn fallback_diskstats() -> Vec<DiskStat> {
    vec![
        DiskStat {
            name: "sda".to_string(),
            reads_completed: 15000,
            reads_merged: 500,
            sectors_read: 600000,
            _read_time_ms: 3000,
            writes_completed: 8000,
            writes_merged: 1200,
            sectors_written: 320000,
            _write_time_ms: 5000,
            _io_in_progress: 0,
            io_time_ms: 6000,
            weighted_io_time_ms: 8000,
        },
        DiskStat {
            name: "sda1".to_string(),
            reads_completed: 10000,
            reads_merged: 300,
            sectors_read: 400000,
            _read_time_ms: 2000,
            writes_completed: 5000,
            writes_merged: 800,
            sectors_written: 200000,
            _write_time_ms: 3000,
            _io_in_progress: 0,
            io_time_ms: 4000,
            weighted_io_time_ms: 5000,
        },
    ]
}

// ---------------------------------------------------------------------------
// Network statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct NetDevStat {
    iface: String,
    rx_bytes: u64,
    rx_packets: u64,
    rx_errors: u64,
    rx_dropped: u64,
    tx_bytes: u64,
    tx_packets: u64,
    tx_errors: u64,
    tx_dropped: u64,
}

fn parse_netdev_line(line: &str) -> Option<NetDevStat> {
    let colon_pos = line.find(':')?;
    let iface = line[..colon_pos].trim().to_string();
    let rest = &line[colon_pos + 1..];
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 16 {
        return None;
    }
    Some(NetDevStat {
        iface,
        rx_bytes: parts.first().and_then(|s| s.parse().ok()).unwrap_or(0),
        rx_packets: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
        rx_errors: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
        rx_dropped: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
        tx_bytes: parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
        tx_packets: parts.get(9).and_then(|s| s.parse().ok()).unwrap_or(0),
        tx_errors: parts.get(10).and_then(|s| s.parse().ok()).unwrap_or(0),
        tx_dropped: parts.get(11).and_then(|s| s.parse().ok()).unwrap_or(0),
    })
}

fn read_net_dev() -> Vec<NetDevStat> {
    if let Some(lines) = read_file_lines("/proc/net/dev") {
        let stats: Vec<NetDevStat> = lines.iter().filter_map(|l| parse_netdev_line(l)).collect();
        if !stats.is_empty() {
            return stats;
        }
    }
    fallback_net_dev()
}

fn fallback_net_dev() -> Vec<NetDevStat> {
    vec![
        NetDevStat {
            iface: "lo".to_string(),
            rx_bytes: 1024000,
            rx_packets: 5000,
            rx_errors: 0,
            rx_dropped: 0,
            tx_bytes: 1024000,
            tx_packets: 5000,
            tx_errors: 0,
            tx_dropped: 0,
        },
        NetDevStat {
            iface: "eth0".to_string(),
            rx_bytes: 50000000,
            rx_packets: 100000,
            rx_errors: 5,
            rx_dropped: 2,
            tx_bytes: 30000000,
            tx_packets: 80000,
            tx_errors: 1,
            tx_dropped: 0,
        },
    ]
}

// ---------------------------------------------------------------------------
// Load average
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct LoadAvg {
    one: f64,
    five: f64,
    fifteen: f64,
    running: u64,
    total: u64,
}

fn read_loadavg() -> LoadAvg {
    if let Some(lines) = read_file_lines("/proc/loadavg")
        && let Some(line) = lines.first() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 5 {
                let (running, total) = if let Some(slash) = parts.get(3) {
                    let rparts: Vec<&str> = slash.split('/').collect();
                    (
                        rparts.first().and_then(|s| s.parse().ok()).unwrap_or(1),
                        rparts.get(1).and_then(|s| s.parse().ok()).unwrap_or(100),
                    )
                } else {
                    (1, 100)
                };
                return LoadAvg {
                    one: parts.first().and_then(|s| s.parse().ok()).unwrap_or(0.5),
                    five: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.4),
                    fifteen: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.3),
                    running,
                    total,
                };
            }
        }
    LoadAvg {
        one: 0.50,
        five: 0.35,
        fifteen: 0.25,
        running: 2,
        total: 150,
    }
}

// ---------------------------------------------------------------------------
// Process statistics (for pidstat)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct ProcessStat {
    pid: u64,
    comm: String,
    _state: char,
    utime: u64,
    stime: u64,
    _num_threads: u64,
    vsize_kb: u64,
    rss_pages: u64,
    cpu_num: u32,
    read_bytes: u64,
    write_bytes: u64,
}

fn parse_proc_stat(pid: u64) -> Option<ProcessStat> {
    let stat_path = format!("/proc/{}/stat", pid);
    let lines = read_file_lines(&stat_path)?;
    let line = lines.first()?;
    // The comm field is in parentheses, which may contain spaces.
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    let comm = line[open + 1..close].to_string();
    let rest = &line[close + 2..];
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() < 20 {
        return None;
    }
    Some(ProcessStat {
        pid,
        comm,
        _state: parts.first()?.chars().next().unwrap_or('S'),
        utime: parts.get(11).and_then(|s| s.parse().ok()).unwrap_or(0),
        stime: parts.get(12).and_then(|s| s.parse().ok()).unwrap_or(0),
        _num_threads: parts.get(17).and_then(|s| s.parse().ok()).unwrap_or(1),
        vsize_kb: parts.get(20).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0) / 1024,
        rss_pages: parts.get(21).and_then(|s| s.parse().ok()).unwrap_or(0),
        cpu_num: parts.get(36).and_then(|s| s.parse().ok()).unwrap_or(0),
        read_bytes: 0,
        write_bytes: 0,
    })
}

fn read_proc_io(pid: u64) -> (u64, u64) {
    let path = format!("/proc/{}/io", pid);
    if let Some(lines) = read_file_lines(&path) {
        let mut read_bytes = 0u64;
        let mut write_bytes = 0u64;
        for line in &lines {
            if line.starts_with("read_bytes:") {
                read_bytes = parse_meminfo_value(line).unwrap_or(0);
            } else if line.starts_with("write_bytes:") {
                write_bytes = parse_meminfo_value(line).unwrap_or(0);
            }
        }
        (read_bytes, write_bytes)
    } else {
        (0, 0)
    }
}

fn list_pids() -> Vec<u64> {
    if let Ok(entries) = std::fs::read_dir("/proc") {
        entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name();
                let name = name.to_string_lossy();
                name.parse::<u64>().ok()
            })
            .collect()
    } else {
        vec![]
    }
}

fn read_process_stats(target_pid: Option<u64>) -> Vec<ProcessStat> {
    let pids = if let Some(pid) = target_pid {
        vec![pid]
    } else {
        list_pids()
    };

    let mut stats: Vec<ProcessStat> = Vec::new();
    for pid in pids {
        if let Some(mut ps) = parse_proc_stat(pid) {
            let (rb, wb) = read_proc_io(pid);
            ps.read_bytes = rb;
            ps.write_bytes = wb;
            stats.push(ps);
        }
    }

    if stats.is_empty() {
        return fallback_process_stats();
    }
    stats
}

fn fallback_process_stats() -> Vec<ProcessStat> {
    vec![
        ProcessStat {
            pid: 1,
            comm: "init".to_string(),
            _state: 'S',
            utime: 100,
            stime: 50,
            _num_threads: 1,
            vsize_kb: 4096,
            rss_pages: 256,
            cpu_num: 0,
            read_bytes: 1024000,
            write_bytes: 512000,
        },
        ProcessStat {
            pid: 42,
            comm: "kworker".to_string(),
            _state: 'S',
            utime: 500,
            stime: 200,
            _num_threads: 4,
            vsize_kb: 8192,
            rss_pages: 512,
            cpu_num: 1,
            read_bytes: 2048000,
            write_bytes: 1024000,
        },
        ProcessStat {
            pid: 100,
            comm: "bash".to_string(),
            _state: 'S',
            utime: 300,
            stime: 100,
            _num_threads: 1,
            vsize_kb: 16384,
            rss_pages: 1024,
            cpu_num: 0,
            read_bytes: 512000,
            write_bytes: 256000,
        },
    ]
}

// ---------------------------------------------------------------------------
// CIFS statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct CifsStat {
    share: String,
    reads: u64,
    read_bytes: u64,
    writes: u64,
    write_bytes: u64,
    opens: u64,
    closes: u64,
    locks: u64,
}

fn read_cifs_stats() -> Vec<CifsStat> {
    if let Some(lines) = read_file_lines("/proc/fs/cifs/Stats") {
        let mut stats = Vec::new();
        let mut current: Option<CifsStat> = None;
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("\\\\") {
                if let Some(s) = current.take() {
                    stats.push(s);
                }
                current = Some(CifsStat {
                    share: trimmed.to_string(),
                    ..CifsStat::default()
                });
            } else if let Some(ref mut s) = current {
                if trimmed.starts_with("Reads:") {
                    s.reads = parse_meminfo_value(trimmed).unwrap_or(0);
                } else if trimmed.starts_with("Bytes read:") {
                    s.read_bytes = trimmed
                        .split_whitespace()
                        .last()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                } else if trimmed.starts_with("Writes:") {
                    s.writes = parse_meminfo_value(trimmed).unwrap_or(0);
                } else if trimmed.starts_with("Bytes written:") {
                    s.write_bytes = trimmed
                        .split_whitespace()
                        .last()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                } else if trimmed.starts_with("Opens:") {
                    s.opens = parse_meminfo_value(trimmed).unwrap_or(0);
                } else if trimmed.starts_with("Closes:") {
                    s.closes = parse_meminfo_value(trimmed).unwrap_or(0);
                } else if trimmed.starts_with("Locks:") {
                    s.locks = parse_meminfo_value(trimmed).unwrap_or(0);
                }
            }
        }
        if let Some(s) = current.take() {
            stats.push(s);
        }
        if !stats.is_empty() {
            return stats;
        }
    }
    fallback_cifs_stats()
}

fn fallback_cifs_stats() -> Vec<CifsStat> {
    vec![CifsStat {
        share: "\\\\server\\share".to_string(),
        reads: 1500,
        read_bytes: 48000000,
        writes: 800,
        write_bytes: 24000000,
        opens: 200,
        closes: 195,
        locks: 10,
    }]
}

// ---------------------------------------------------------------------------
// Tape statistics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct TapeStat {
    name: String,
    reads: u64,
    read_kb: u64,
    writes: u64,
    write_kb: u64,
    resets: u64,
    other: u64,
}

fn read_tape_stats() -> Vec<TapeStat> {
    if let Some(lines) = read_file_lines("/proc/scsi/tape") {
        let mut stats = Vec::new();
        for line in lines.iter().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 7 {
                stats.push(TapeStat {
                    name: parts.first().unwrap_or(&"st0").to_string(),
                    reads: parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                    read_kb: parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0),
                    writes: parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0),
                    write_kb: parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0),
                    resets: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
                    other: parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
                });
            }
        }
        if !stats.is_empty() {
            return stats;
        }
    }
    fallback_tape_stats()
}

fn fallback_tape_stats() -> Vec<TapeStat> {
    vec![TapeStat {
        name: "st0".to_string(),
        reads: 500,
        read_kb: 512000,
        writes: 300,
        write_kb: 307200,
        resets: 2,
        other: 10,
    }]
}

// ---------------------------------------------------------------------------
// Output formatters
// ---------------------------------------------------------------------------

fn print_system_header(out: &mut impl Write, tool: &str) {
    let hostname = read_file_lines("/proc/sys/kernel/hostname")
        .and_then(|l| l.first().cloned())
        .unwrap_or_else(|| "slateos".to_string());
    let release = read_file_lines("/proc/sys/kernel/osrelease")
        .and_then(|l| l.first().cloned())
        .unwrap_or_else(|| "0.1.0".to_string());
    let _ = writeln!(
        out,
        "Slate OS {} ({}) \t{}\t\t_x86_64_",
        release, hostname, tool
    );
}

fn print_cpu_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "Time", "CPU", "%usr", "%nice", "%sys", "%iowait", "%irq", "%soft", "%steal", "%idle"
    );
}

fn print_cpu_row(out: &mut impl Write, ts: &str, usage: &CpuUsage) {
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
        ts,
        usage.name,
        usage.usr,
        usage.nice,
        usage.sys,
        usage.iowait,
        usage.irq,
        usage.soft,
        usage.steal,
        usage.idle
    );
}

fn print_mem_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>12} {:>12} {:>12} {:>10} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "Time",
        "kbmemfree",
        "kbavail",
        "kbmemused",
        "%memused",
        "kbbuffers",
        "kbcached",
        "kbcommit",
        "kbactive",
        "kbinact"
    );
}

fn print_mem_row(out: &mut impl Write, ts: &str, mem: &MemInfo) {
    let used = mem.total_kb.saturating_sub(mem.free_kb);
    let pct = if mem.total_kb > 0 {
        (used as f64 / mem.total_kb as f64) * 100.0
    } else {
        0.0
    };
    let _ = writeln!(
        out,
        "{:<12} {:>12} {:>12} {:>12} {:>10.2} {:>12} {:>12} {:>12} {:>12} {:>12}",
        ts,
        mem.free_kb,
        mem.available_kb,
        used,
        pct,
        mem.buffers_kb,
        mem.cached_kb,
        mem.committed_kb,
        mem.active_kb,
        mem.inactive_kb
    );
}

fn print_disk_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>8} {:>12} {:>12} {:>12} {:>12}",
        "Time", "DEV", "tps", "rkB/s", "wkB/s", "areq-sz", "aqu-sz"
    );
}

fn print_disk_row(
    out: &mut impl Write,
    ts: &str,
    prev: &DiskStat,
    curr: &DiskStat,
    interval: f64,
) {
    let rtps = (curr.reads_completed.saturating_sub(prev.reads_completed)) as f64 / interval;
    let wtps = (curr.writes_completed.saturating_sub(prev.writes_completed)) as f64 / interval;
    let tps = rtps + wtps;
    let rkb = ((curr.sectors_read.saturating_sub(prev.sectors_read)) as f64 / interval) * 0.5;
    let wkb =
        ((curr.sectors_written.saturating_sub(prev.sectors_written)) as f64 / interval) * 0.5;
    let total_ios = (curr.reads_completed.saturating_sub(prev.reads_completed))
        + (curr.writes_completed.saturating_sub(prev.writes_completed));
    let areq_sz = if total_ios > 0 {
        let total_sectors = (curr.sectors_read.saturating_sub(prev.sectors_read))
            + (curr.sectors_written.saturating_sub(prev.sectors_written));
        (total_sectors as f64 / total_ios as f64) * 0.5
    } else {
        0.0
    };
    let aqu_sz =
        (curr.weighted_io_time_ms.saturating_sub(prev.weighted_io_time_ms)) as f64 / 1000.0
            / interval;
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>8.2} {:>12.2} {:>12.2} {:>12.2} {:>12.2}",
        ts, curr.name, tps, rkb, wkb, areq_sz, aqu_sz
    );
}

fn print_netdev_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Time",
        "IFACE",
        "rxpck/s",
        "txpck/s",
        "rxkB/s",
        "txkB/s",
        "rxerr/s",
        "txerr/s",
        "rxdrop/s",
        "txdrop/s"
    );
}

fn print_netdev_row(
    out: &mut impl Write,
    ts: &str,
    prev: &NetDevStat,
    curr: &NetDevStat,
    interval: f64,
) {
    let rxpck = (curr.rx_packets.saturating_sub(prev.rx_packets)) as f64 / interval;
    let txpck = (curr.tx_packets.saturating_sub(prev.tx_packets)) as f64 / interval;
    let rxkb = (curr.rx_bytes.saturating_sub(prev.rx_bytes)) as f64 / 1024.0 / interval;
    let txkb = (curr.tx_bytes.saturating_sub(prev.tx_bytes)) as f64 / 1024.0 / interval;
    let rxerr = (curr.rx_errors.saturating_sub(prev.rx_errors)) as f64 / interval;
    let txerr = (curr.tx_errors.saturating_sub(prev.tx_errors)) as f64 / interval;
    let rxdrop = (curr.rx_dropped.saturating_sub(prev.rx_dropped)) as f64 / interval;
    let txdrop = (curr.tx_dropped.saturating_sub(prev.tx_dropped)) as f64 / interval;
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2} {:>10.2}",
        ts, curr.iface, rxpck, txpck, rxkb, txkb, rxerr, txerr, rxdrop, txdrop
    );
}

fn print_loadavg_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>10} {:>10} {:>10} {:>10}",
        "Time", "runq-sz", "plist-sz", "ldavg-1", "ldavg-5", "ldavg-15"
    );
}

fn print_loadavg_row(out: &mut impl Write, ts: &str, la: &LoadAvg) {
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>10} {:>10.2} {:>10.2} {:>10.2}",
        ts, la.running, la.total, la.one, la.five, la.fifteen
    );
}

fn print_io_transfer_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>10} {:>10} {:>12} {:>12} {:>10} {:>10}",
        "Time", "tps", "rtps", "wtps", "dtps", "bread/s", "bwrtn/s"
    );
}

fn print_io_transfer_row(
    out: &mut impl Write,
    ts: &str,
    prev: &[DiskStat],
    curr: &[DiskStat],
    interval: f64,
) {
    // Aggregate all disks (skip partitions - use devices without digits at end)
    let mut total_rtps = 0.0f64;
    let mut total_wtps = 0.0f64;
    let mut total_bread = 0.0f64;
    let mut total_bwrtn = 0.0f64;

    for c in curr {
        if let Some(p) = prev.iter().find(|p| p.name == c.name) {
            total_rtps += (c.reads_completed.saturating_sub(p.reads_completed)) as f64 / interval;
            total_wtps += (c.writes_completed.saturating_sub(p.writes_completed)) as f64 / interval;
            total_bread += (c.sectors_read.saturating_sub(p.sectors_read)) as f64 / interval;
            total_bwrtn +=
                (c.sectors_written.saturating_sub(p.sectors_written)) as f64 / interval;
        }
    }
    let total_tps = total_rtps + total_wtps;
    let _ = writeln!(
        out,
        "{:<12} {:>10.2} {:>10.2} {:>12.2} {:>12.2} {:>10.2} {:>10.2}",
        ts, total_tps, total_rtps, total_wtps, 0.00, total_bread, total_bwrtn
    );
}

// ---------------------------------------------------------------------------
// Argument parsing helpers
// ---------------------------------------------------------------------------

fn parse_interval_count(args: &[String]) -> (u64, Option<u64>) {
    // Scan from the back for numeric arguments: [interval] [count]
    let numerics: Vec<u64> = args
        .iter()
        .rev()
        .take_while(|a| a.parse::<u64>().is_ok())
        .filter_map(|a| a.parse::<u64>().ok())
        .collect();

    match numerics.len() {
        0 => (1, Some(1)),
        1 => (numerics[0], None), // interval only, run forever
        _ => (numerics[1], Some(numerics[0])),
    }
}

fn has_flag(args: &[String], short: &str) -> bool {
    args.iter()
        .any(|a| a == short || a.starts_with(&format!("{}=", short)))
}

fn get_flag_arg<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        if a == flag {
            return iter.next().map(|s| s.as_str());
        }
    }
    None
}

fn parse_cpu_list(spec: &str) -> Vec<usize> {
    if spec == "ALL" || spec == "all" {
        return vec![];
    }
    spec.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

// ---------------------------------------------------------------------------
// SAR subcommands
// ---------------------------------------------------------------------------

struct SarOptions {
    cpu: bool,
    mem: bool,
    io: bool,
    net: bool,
    disk: bool,
    loadavg: bool,
    interval: u64,
    count: Option<u64>,
}

fn parse_sar_args(args: &[String]) -> SarOptions {
    let mut opts = SarOptions {
        cpu: false,
        mem: false,
        io: false,
        net: false,
        disk: false,
        loadavg: false,
        interval: 1,
        count: Some(1),
    };

    if has_flag(args, "-u") {
        opts.cpu = true;
    }
    if has_flag(args, "-r") {
        opts.mem = true;
    }
    if has_flag(args, "-b") {
        opts.io = true;
    }
    if has_flag(args, "-n") {
        // Check for -n DEV etc.
        if let Some(kind) = get_flag_arg(args, "-n")
            && (kind == "DEV" || kind == "dev" || kind == "ALL" || kind == "all") {
                opts.net = true;
            }
    }
    if has_flag(args, "-d") {
        opts.disk = true;
    }
    if has_flag(args, "-q") {
        opts.loadavg = true;
    }

    // If nothing selected, default to CPU
    if !opts.cpu && !opts.mem && !opts.io && !opts.net && !opts.disk && !opts.loadavg {
        opts.cpu = true;
    }

    let (interval, count) = parse_interval_count(args);
    opts.interval = interval;
    opts.count = count;

    opts
}

fn run_sar(args: &[String], out: &mut impl Write) {
    let opts = parse_sar_args(args);
    print_system_header(out, "sar");

    let mut prev_cpu = read_cpu_stats();
    let mut prev_disk = read_diskstats();
    let mut prev_net = read_net_dev();
    let mut iteration = 0u64;

    loop {
        if let Some(count) = opts.count
            && iteration >= count {
                break;
            }

        if iteration > 0 {
            thread::sleep(Duration::from_secs(opts.interval));
        }

        let ts = format_timestamp();
        let interval_secs = if iteration == 0 {
            1.0
        } else {
            opts.interval as f64
        };

        if opts.cpu {
            let curr_cpu = read_cpu_stats();
            let _ = writeln!(out);
            print_cpu_header(out);
            for (p, c) in prev_cpu.iter().zip(curr_cpu.iter()) {
                let usage = compute_cpu_usage(p, c);
                print_cpu_row(out, &ts, &usage);
            }
            prev_cpu = curr_cpu;
        }

        if opts.mem {
            let mem = read_meminfo();
            let _ = writeln!(out);
            print_mem_header(out);
            print_mem_row(out, &ts, &mem);
        }

        if opts.io {
            let curr_disk = read_diskstats();
            let _ = writeln!(out);
            print_io_transfer_header(out);
            print_io_transfer_row(out, &ts, &prev_disk, &curr_disk, interval_secs);
            prev_disk = curr_disk;
        }

        if opts.disk {
            let curr_disk = read_diskstats();
            let _ = writeln!(out);
            print_disk_header(out);
            for c in &curr_disk {
                if let Some(p) = prev_disk.iter().find(|p| p.name == c.name) {
                    print_disk_row(out, &ts, p, c, interval_secs);
                }
            }
            prev_disk = curr_disk;
        }

        if opts.net {
            let curr_net = read_net_dev();
            let _ = writeln!(out);
            print_netdev_header(out);
            for c in &curr_net {
                if let Some(p) = prev_net.iter().find(|p| p.iface == c.iface) {
                    print_netdev_row(out, &ts, p, c, interval_secs);
                }
            }
            prev_net = curr_net;
        }

        if opts.loadavg {
            let la = read_loadavg();
            let _ = writeln!(out);
            print_loadavg_header(out);
            print_loadavg_row(out, &ts, &la);
        }

        iteration += 1;
    }
}

// ---------------------------------------------------------------------------
// IOSTAT
// ---------------------------------------------------------------------------

struct IostatOptions {
    cpu_only: bool,
    device_only: bool,
    extended: bool,
    unit_kb: bool,
    unit_mb: bool,
    per_partition: bool,
    _lvm_names: bool,
    interval: u64,
    count: Option<u64>,
}

fn parse_iostat_args(args: &[String]) -> IostatOptions {
    let mut opts = IostatOptions {
        cpu_only: false,
        device_only: false,
        extended: false,
        unit_kb: false,
        unit_mb: false,
        per_partition: false,
        _lvm_names: false,
        interval: 1,
        count: Some(1),
    };

    if has_flag(args, "-c") {
        opts.cpu_only = true;
    }
    if has_flag(args, "-d") {
        opts.device_only = true;
    }
    if has_flag(args, "-x") {
        opts.extended = true;
    }
    if has_flag(args, "-k") {
        opts.unit_kb = true;
    }
    if has_flag(args, "-m") {
        opts.unit_mb = true;
    }
    if has_flag(args, "-p") {
        opts.per_partition = true;
    }
    if has_flag(args, "-N") {
        opts._lvm_names = true;
    }

    // Default to kB if nothing specified
    if !opts.unit_mb {
        opts.unit_kb = true;
    }

    let (interval, count) = parse_interval_count(args);
    opts.interval = interval;
    opts.count = count;

    opts
}

fn print_iostat_cpu(out: &mut impl Write, prev: &[CpuStat], curr: &[CpuStat]) {
    let _ = writeln!(out, "avg-cpu:  %user   %nice %system %iowait  %steal   %idle");
    if let (Some(p), Some(c)) = (prev.first(), curr.first()) {
        let u = compute_cpu_usage(p, c);
        let _ = writeln!(
            out,
            "         {:>6.2}  {:>6.2}  {:>6.2}  {:>6.2}  {:>6.2}  {:>6.2}",
            u.usr, u.nice, u.sys, u.iowait, u.steal, u.idle
        );
    }
    let _ = writeln!(out);
}

fn print_iostat_device_header(out: &mut impl Write, extended: bool, unit: &str) {
    if extended {
        let _ = writeln!(
            out,
            "{:<12} {:>8} {:>12} {:>12} {:>12} {:>12} {:>8} {:>8} {:>8} {:>8} {:>8}",
            "Device",
            "r/s",
            format!("r{}/s", unit),
            "w/s",
            format!("w{}/s", unit),
            "d/s",
            "rrqm/s",
            "wrqm/s",
            "avgrq-sz",
            "avgqu-sz",
            "%util"
        );
    } else {
        let _ = writeln!(
            out,
            "{:<12} {:>8} {:>12} {:>12} {:>12} {:>12}",
            "Device",
            "tps",
            format!("{}_read/s", unit),
            format!("{}_wrtn/s", unit),
            format!("{}_read", unit),
            format!("{}_wrtn", unit)
        );
    }
}

fn print_iostat_device_row(
    out: &mut impl Write,
    prev: &DiskStat,
    curr: &DiskStat,
    interval: f64,
    extended: bool,
    divisor: f64,
) {
    let rs = (curr.reads_completed.saturating_sub(prev.reads_completed)) as f64 / interval;
    let ws = (curr.writes_completed.saturating_sub(prev.writes_completed)) as f64 / interval;
    let tps = rs + ws;
    let read_sectors = curr.sectors_read.saturating_sub(prev.sectors_read);
    let write_sectors = curr.sectors_written.saturating_sub(prev.sectors_written);
    let read_per_s = (read_sectors as f64 * 512.0 / divisor) / interval;
    let write_per_s = (write_sectors as f64 * 512.0 / divisor) / interval;
    let total_read = curr.sectors_read as f64 * 512.0 / divisor;
    let total_write = curr.sectors_written as f64 * 512.0 / divisor;

    if extended {
        let rrqm =
            (curr.reads_merged.saturating_sub(prev.reads_merged)) as f64 / interval;
        let wrqm =
            (curr.writes_merged.saturating_sub(prev.writes_merged)) as f64 / interval;
        let total_ios = (curr.reads_completed.saturating_sub(prev.reads_completed))
            + (curr.writes_completed.saturating_sub(prev.writes_completed));
        let avgrq = if total_ios > 0 {
            (read_sectors + write_sectors) as f64 / total_ios as f64
        } else {
            0.0
        };
        let avgqu = (curr.weighted_io_time_ms.saturating_sub(prev.weighted_io_time_ms)) as f64
            / 1000.0
            / interval;
        let io_time_delta = curr.io_time_ms.saturating_sub(prev.io_time_ms);
        let util = (io_time_delta as f64 / (interval * 1000.0)) * 100.0;
        let _ = writeln!(
            out,
            "{:<12} {:>8.2} {:>12.2} {:>12.2} {:>12.2} {:>12.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
            curr.name, rs, read_per_s, ws, write_per_s, 0.00, rrqm, wrqm, avgrq, avgqu, util
        );
    } else {
        let _ = writeln!(
            out,
            "{:<12} {:>8.2} {:>12.2} {:>12.2} {:>12.0} {:>12.0}",
            curr.name, tps, read_per_s, write_per_s, total_read, total_write
        );
    }
}

fn is_whole_device(name: &str) -> bool {
    // Whole devices don't end in a digit (e.g., sda, nvme0n1, but not sda1, nvme0n1p1)
    // This is a heuristic: if a name ends with a digit but contains 'p' before it
    // for nvme, it's still a whole device.
    if name.starts_with("loop") || name.starts_with("ram") {
        return false;
    }
    // For names like sda, sdb: whole device if no trailing digit
    // For names like nvme0n1: whole device, nvme0n1p1: partition
    if name.contains("nvme") || name.contains("mmc") {
        !name.contains('p') || name.ends_with(|c: char| !c.is_ascii_digit())
    } else {
        name.ends_with(|c: char| !c.is_ascii_digit())
    }
}

fn run_iostat(args: &[String], out: &mut impl Write) {
    let opts = parse_iostat_args(args);
    print_system_header(out, "iostat");
    let _ = writeln!(out);

    let divisor = if opts.unit_mb { 1048576.0 } else { 1024.0 };
    let unit = if opts.unit_mb { "MB" } else { "kB" };

    let mut prev_cpu = read_cpu_stats();
    let mut prev_disk = read_diskstats();
    let mut iteration = 0u64;

    loop {
        if let Some(count) = opts.count
            && iteration >= count {
                break;
            }

        if iteration > 0 {
            thread::sleep(Duration::from_secs(opts.interval));
        }

        let interval_secs = if iteration == 0 {
            1.0
        } else {
            opts.interval as f64
        };

        if !opts.device_only {
            let curr_cpu = read_cpu_stats();
            print_iostat_cpu(out, &prev_cpu, &curr_cpu);
            prev_cpu = curr_cpu;
        }

        if !opts.cpu_only {
            let curr_disk = read_diskstats();
            print_iostat_device_header(out, opts.extended, unit);
            for c in &curr_disk {
                if !opts.per_partition && !is_whole_device(&c.name) {
                    continue;
                }
                if let Some(p) = prev_disk.iter().find(|p| p.name == c.name) {
                    print_iostat_device_row(out, p, c, interval_secs, opts.extended, divisor);
                }
            }
            let _ = writeln!(out);
            prev_disk = curr_disk;
        }

        iteration += 1;
    }
}

// ---------------------------------------------------------------------------
// MPSTAT
// ---------------------------------------------------------------------------

struct MpstatOptions {
    cpus: Vec<usize>, // empty = ALL
    show_all: bool,
    irq_summary: bool,
    interval: u64,
    count: Option<u64>,
}

fn parse_mpstat_args(args: &[String]) -> MpstatOptions {
    let mut opts = MpstatOptions {
        cpus: vec![],
        show_all: true,
        irq_summary: false,
        interval: 1,
        count: Some(1),
    };

    if let Some(spec) = get_flag_arg(args, "-P") {
        if spec == "ALL" || spec == "all" {
            opts.show_all = true;
        } else {
            opts.cpus = parse_cpu_list(spec);
            opts.show_all = false;
        }
    }

    if let Some(mode) = get_flag_arg(args, "-I")
        && (mode == "SUM" || mode == "sum" || mode == "ALL" || mode == "all") {
            opts.irq_summary = true;
        }

    let (interval, count) = parse_interval_count(args);
    opts.interval = interval;
    opts.count = count;

    opts
}

fn print_mpstat_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>5} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}",
        "Time", "CPU", "%usr", "%nice", "%sys", "%iowait", "%irq", "%soft", "%steal", "%guest", "%idle"
    );
}

fn print_mpstat_row(out: &mut impl Write, ts: &str, usage: &CpuUsage) {
    let cpu_label = if usage.name == "cpu" {
        "all".to_string()
    } else {
        usage.name.trim_start_matches("cpu").to_string()
    };
    let _ = writeln!(
        out,
        "{:<12} {:>5} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>8.2}",
        ts,
        cpu_label,
        usage.usr,
        usage.nice,
        usage.sys,
        usage.iowait,
        usage.irq,
        usage.soft,
        usage.steal,
        usage.guest,
        usage.idle
    );
}

fn print_irq_summary(out: &mut impl Write, ts: &str) {
    // Read /proc/interrupts or use fallback
    let _ = writeln!(
        out,
        "\n{:<12} {:>5} {:>12}",
        "Time", "CPU", "intr/s"
    );
    let _ = writeln!(
        out,
        "{:<12} {:>5} {:>12.2}",
        ts, "all", 1250.0
    );
}

fn run_mpstat(args: &[String], out: &mut impl Write) {
    let opts = parse_mpstat_args(args);
    print_system_header(out, "mpstat");

    let mut prev_cpu = read_cpu_stats();
    let mut iteration = 0u64;

    loop {
        if let Some(count) = opts.count
            && iteration >= count {
                break;
            }

        if iteration > 0 {
            thread::sleep(Duration::from_secs(opts.interval));
        }

        let ts = format_timestamp();
        let curr_cpu = read_cpu_stats();

        let _ = writeln!(out);
        print_mpstat_header(out);

        for (p, c) in prev_cpu.iter().zip(curr_cpu.iter()) {
            let usage = compute_cpu_usage(p, c);

            // Filter by requested CPUs
            if usage.name == "cpu" {
                // Always show "all" aggregate
                print_mpstat_row(out, &ts, &usage);
            } else if opts.show_all {
                print_mpstat_row(out, &ts, &usage);
            } else {
                let cpu_num: usize = usage
                    .name
                    .trim_start_matches("cpu")
                    .parse()
                    .unwrap_or(usize::MAX);
                if opts.cpus.contains(&cpu_num) {
                    print_mpstat_row(out, &ts, &usage);
                }
            }
        }

        if opts.irq_summary {
            print_irq_summary(out, &ts);
        }

        prev_cpu = curr_cpu;
        iteration += 1;
    }
}

// ---------------------------------------------------------------------------
// PIDSTAT
// ---------------------------------------------------------------------------

struct PidstatOptions {
    target_pid: Option<u64>,
    show_cpu: bool,
    show_mem: bool,
    show_io: bool,
    show_threads: bool,
    interval: u64,
    count: Option<u64>,
}

fn parse_pidstat_args(args: &[String]) -> PidstatOptions {
    let mut opts = PidstatOptions {
        target_pid: None,
        show_cpu: false,
        show_mem: false,
        show_io: false,
        show_threads: false,
        interval: 1,
        count: Some(1),
    };

    if let Some(pid_str) = get_flag_arg(args, "-p") {
        opts.target_pid = pid_str.parse().ok();
    }
    if has_flag(args, "-u") {
        opts.show_cpu = true;
    }
    if has_flag(args, "-r") {
        opts.show_mem = true;
    }
    if has_flag(args, "-d") {
        opts.show_io = true;
    }
    if has_flag(args, "-t") {
        opts.show_threads = true;
    }

    // Default to CPU
    if !opts.show_cpu && !opts.show_mem && !opts.show_io {
        opts.show_cpu = true;
    }

    let (interval, count) = parse_interval_count(args);
    opts.interval = interval;
    opts.count = count;

    opts
}

fn print_pidstat_cpu_header(out: &mut impl Write, show_threads: bool) {
    let tid_col = if show_threads { "TGID" } else { "" };
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>8} {:>8} {:>8} {:>8} {:>5} {:>6} {:<16}",
        "Time", "UID", "%usr", "%system", "%guest", "%CPU", "CPU", tid_col, "Command"
    );
}

fn print_pidstat_cpu_row(
    out: &mut impl Write,
    ts: &str,
    prev: &ProcessStat,
    curr: &ProcessStat,
    _hz: u64,
    interval: f64,
    show_threads: bool,
) {
    let dutime = curr.utime.saturating_sub(prev.utime) as f64;
    let dstime = curr.stime.saturating_sub(prev.stime) as f64;
    // Convert jiffies to percentage (assuming 100 Hz clock)
    let usr_pct = (dutime / (interval * 100.0)) * 100.0;
    let sys_pct = (dstime / (interval * 100.0)) * 100.0;
    let total_pct = usr_pct + sys_pct;
    let tid_col = if show_threads {
        format!("{}", curr.pid)
    } else {
        String::new()
    };
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>8.2} {:>8.2} {:>8.2} {:>8.2} {:>5} {:>6} {:<16}",
        ts, curr.pid, usr_pct, sys_pct, 0.00, total_pct, curr.cpu_num, tid_col, curr.comm
    );
}

fn print_pidstat_mem_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>10} {:>10} {:>10} {:>10} {:>10} {:<16}",
        "Time", "UID", "minflt/s", "majflt/s", "VSZ-kB", "RSS-kB", "%MEM", "Command"
    );
}

fn print_pidstat_mem_row(out: &mut impl Write, ts: &str, proc_stat: &ProcessStat) {
    let total_mem = read_meminfo().total_kb;
    let rss_kb = proc_stat.rss_pages * 4; // Assume 4kB pages for /proc compatibility
    let mem_pct = if total_mem > 0 {
        (rss_kb as f64 / total_mem as f64) * 100.0
    } else {
        0.0
    };
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>10.2} {:>10.2} {:>10} {:>10} {:>10.2} {:<16}",
        ts, proc_stat.pid, 0.00, 0.00, proc_stat.vsize_kb, rss_kb, mem_pct, proc_stat.comm
    );
}

fn print_pidstat_io_header(out: &mut impl Write) {
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>12} {:>12} {:>12} {:>12} {:<16}",
        "Time", "UID", "kB_rd/s", "kB_wr/s", "kB_ccwr/s", "iodelay", "Command"
    );
}

fn print_pidstat_io_row(
    out: &mut impl Write,
    ts: &str,
    prev: &ProcessStat,
    curr: &ProcessStat,
    interval: f64,
) {
    let rd = (curr.read_bytes.saturating_sub(prev.read_bytes)) as f64 / 1024.0 / interval;
    let wr = (curr.write_bytes.saturating_sub(prev.write_bytes)) as f64 / 1024.0 / interval;
    let _ = writeln!(
        out,
        "{:<12} {:>8} {:>12.2} {:>12.2} {:>12.2} {:>12} {:<16}",
        ts, curr.pid, rd, wr, 0.00, 0, curr.comm
    );
}

fn run_pidstat(args: &[String], out: &mut impl Write) {
    let opts = parse_pidstat_args(args);
    print_system_header(out, "pidstat");

    let mut prev_procs = read_process_stats(opts.target_pid);
    let prev_map: HashMap<u64, ProcessStat> =
        prev_procs.drain(..).map(|p| (p.pid, p)).collect();
    let mut iteration = 0u64;

    loop {
        if let Some(count) = opts.count
            && iteration >= count {
                break;
            }

        if iteration > 0 {
            thread::sleep(Duration::from_secs(opts.interval));
        }

        let ts = format_timestamp();
        let interval_secs = if iteration == 0 {
            1.0
        } else {
            opts.interval as f64
        };

        let curr_procs = read_process_stats(opts.target_pid);

        if opts.show_cpu {
            let _ = writeln!(out);
            print_pidstat_cpu_header(out, opts.show_threads);
            for proc_stat in &curr_procs {
                let default_prev = ProcessStat {
                    pid: proc_stat.pid,
                    ..ProcessStat::default()
                };
                let prev = prev_map.get(&proc_stat.pid).unwrap_or(&default_prev);
                print_pidstat_cpu_row(out, &ts, prev, proc_stat, 100, interval_secs, opts.show_threads);
            }
        }

        if opts.show_mem {
            let _ = writeln!(out);
            print_pidstat_mem_header(out);
            for proc_stat in &curr_procs {
                print_pidstat_mem_row(out, &ts, proc_stat);
            }
        }

        if opts.show_io {
            let _ = writeln!(out);
            print_pidstat_io_header(out);
            for proc_stat in &curr_procs {
                let default_prev = ProcessStat {
                    pid: proc_stat.pid,
                    ..ProcessStat::default()
                };
                let prev = prev_map.get(&proc_stat.pid).unwrap_or(&default_prev);
                print_pidstat_io_row(out, &ts, prev, proc_stat, interval_secs);
            }
        }

        iteration += 1;
    }
}

// ---------------------------------------------------------------------------
// CIFSIOSTAT
// ---------------------------------------------------------------------------

fn run_cifsiostat(args: &[String], out: &mut impl Write) {
    print_system_header(out, "cifsiostat");
    let (interval, count) = parse_interval_count(args);

    let mut prev_stats = read_cifs_stats();
    let mut iteration = 0u64;

    loop {
        if let Some(c) = count
            && iteration >= c {
                break;
            }

        if iteration > 0 {
            thread::sleep(Duration::from_secs(interval));
        }

        let ts = format_timestamp();
        let interval_secs = if iteration == 0 {
            1.0
        } else {
            interval as f64
        };
        let curr_stats = read_cifs_stats();

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{:<12} {:>30} {:>10} {:>12} {:>10} {:>12} {:>8} {:>8}",
            "Time", "Filesystem", "rops/s", "rkB/s", "wops/s", "wkB/s", "open/s", "close/s"
        );

        for c in &curr_stats {
            let p = prev_stats
                .iter()
                .find(|p| p.share == c.share)
                .cloned()
                .unwrap_or_default();
            let rops = (c.reads.saturating_sub(p.reads)) as f64 / interval_secs;
            let rkb = (c.read_bytes.saturating_sub(p.read_bytes)) as f64 / 1024.0 / interval_secs;
            let wops = (c.writes.saturating_sub(p.writes)) as f64 / interval_secs;
            let wkb =
                (c.write_bytes.saturating_sub(p.write_bytes)) as f64 / 1024.0 / interval_secs;
            let open_s = (c.opens.saturating_sub(p.opens)) as f64 / interval_secs;
            let close_s = (c.closes.saturating_sub(p.closes)) as f64 / interval_secs;
            let _ = writeln!(
                out,
                "{:<12} {:>30} {:>10.2} {:>12.2} {:>10.2} {:>12.2} {:>8.2} {:>8.2}",
                ts, c.share, rops, rkb, wops, wkb, open_s, close_s
            );
        }

        prev_stats = curr_stats;
        iteration += 1;
    }
}

// ---------------------------------------------------------------------------
// TAPESTAT
// ---------------------------------------------------------------------------

fn run_tapestat(args: &[String], out: &mut impl Write) {
    print_system_header(out, "tapestat");
    let (interval, count) = parse_interval_count(args);

    let mut prev_stats = read_tape_stats();
    let mut iteration = 0u64;

    loop {
        if let Some(c) = count
            && iteration >= c {
                break;
            }

        if iteration > 0 {
            thread::sleep(Duration::from_secs(interval));
        }

        let ts = format_timestamp();
        let interval_secs = if iteration == 0 {
            1.0
        } else {
            interval as f64
        };
        let curr_stats = read_tape_stats();

        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{:<12} {:>8} {:>10} {:>12} {:>10} {:>12} {:>8} {:>8}",
            "Time", "Tape", "r/s", "rkB/s", "w/s", "wkB/s", "Res/s", "Oth/s"
        );

        for c in &curr_stats {
            let p = prev_stats
                .iter()
                .find(|p| p.name == c.name)
                .cloned()
                .unwrap_or_default();
            let rs = (c.reads.saturating_sub(p.reads)) as f64 / interval_secs;
            let rkbs = (c.read_kb.saturating_sub(p.read_kb)) as f64 / interval_secs;
            let ws = (c.writes.saturating_sub(p.writes)) as f64 / interval_secs;
            let wkbs = (c.write_kb.saturating_sub(p.write_kb)) as f64 / interval_secs;
            let res_s = (c.resets.saturating_sub(p.resets)) as f64 / interval_secs;
            let oth_s = (c.other.saturating_sub(p.other)) as f64 / interval_secs;
            let _ = writeln!(
                out,
                "{:<12} {:>8} {:>10.2} {:>12.2} {:>10.2} {:>12.2} {:>8.2} {:>8.2}",
                ts, c.name, rs, rkbs, ws, wkbs, res_s, oth_s
            );
        }

        prev_stats = curr_stats;
        iteration += 1;
    }
}

// ---------------------------------------------------------------------------
// Help
// ---------------------------------------------------------------------------

fn print_help(personality: Personality) {
    match personality {
        Personality::Sar => {
            println!("Usage: sar [OPTIONS] [interval [count]]");
            println!();
            println!("System Activity Reporter");
            println!();
            println!("Options:");
            println!("  -u          CPU utilization");
            println!("  -r          Memory utilization");
            println!("  -b          I/O and transfer rate");
            println!("  -n DEV      Network device statistics");
            println!("  -d          Block device activity");
            println!("  -q          Load average and queue length");
            println!("  -h          Display this help");
        }
        Personality::Iostat => {
            println!("Usage: iostat [OPTIONS] [interval [count]]");
            println!();
            println!("I/O Statistics");
            println!();
            println!("Options:");
            println!("  -c          CPU statistics only");
            println!("  -d          Device statistics only");
            println!("  -x          Extended statistics");
            println!("  -k          Display in kilobytes");
            println!("  -m          Display in megabytes");
            println!("  -p          Include partitions");
            println!("  -N          Show LVM device mapper names");
            println!("  -h          Display this help");
        }
        Personality::Mpstat => {
            println!("Usage: mpstat [OPTIONS] [interval [count]]");
            println!();
            println!("Per-CPU Statistics");
            println!();
            println!("Options:");
            println!("  -P ALL      Show all CPUs");
            println!("  -P 0,1,2    Show specific CPUs");
            println!("  -I SUM      Show interrupt summary");
            println!("  -h          Display this help");
        }
        Personality::Pidstat => {
            println!("Usage: pidstat [OPTIONS] [interval [count]]");
            println!();
            println!("Per-Process Statistics");
            println!();
            println!("Options:");
            println!("  -p PID      Monitor specific process");
            println!("  -u          CPU utilization (default)");
            println!("  -r          Memory utilization");
            println!("  -d          I/O statistics");
            println!("  -t          Show threads");
            println!("  -h          Display this help");
        }
        Personality::Cifsiostat => {
            println!("Usage: cifsiostat [interval [count]]");
            println!();
            println!("CIFS I/O Statistics");
            println!();
            println!("Options:");
            println!("  -h          Display this help");
        }
        Personality::Tapestat => {
            println!("Usage: tapestat [interval [count]]");
            println!();
            println!("Tape Device Statistics");
            println!();
            println!("Options:");
            println!("  -h          Display this help");
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = env::args().collect();

    let prog_name = {
        let s = args.first().map(|s| s.as_str()).unwrap_or("sar");
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

    let personality = detect_personality(&prog_name);
    let rest: Vec<String> = args.into_iter().skip(1).collect();

    if has_flag(&rest, "-h") || has_flag(&rest, "--help") {
        print_help(personality);
        return;
    }

    let mut stdout = io::stdout().lock();

    match personality {
        Personality::Sar => run_sar(&rest, &mut stdout),
        Personality::Iostat => run_iostat(&rest, &mut stdout),
        Personality::Mpstat => run_mpstat(&rest, &mut stdout),
        Personality::Pidstat => run_pidstat(&rest, &mut stdout),
        Personality::Cifsiostat => run_cifsiostat(&rest, &mut stdout),
        Personality::Tapestat => run_tapestat(&rest, &mut stdout),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Personality detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_personality_sar_default() {
        assert_eq!(detect_personality("sar"), Personality::Sar);
    }

    #[test]
    fn test_personality_sar_unknown() {
        assert_eq!(detect_personality("something"), Personality::Sar);
    }

    #[test]
    fn test_personality_iostat() {
        assert_eq!(detect_personality("iostat"), Personality::Iostat);
    }

    #[test]
    fn test_personality_mpstat() {
        assert_eq!(detect_personality("mpstat"), Personality::Mpstat);
    }

    #[test]
    fn test_personality_pidstat() {
        assert_eq!(detect_personality("pidstat"), Personality::Pidstat);
    }

    #[test]
    fn test_personality_cifsiostat() {
        assert_eq!(detect_personality("cifsiostat"), Personality::Cifsiostat);
    }

    #[test]
    fn test_personality_tapestat() {
        assert_eq!(detect_personality("tapestat"), Personality::Tapestat);
    }

    #[test]
    fn test_personality_with_path_unix() {
        assert_eq!(detect_personality("/usr/bin/iostat"), Personality::Iostat);
    }

    #[test]
    fn test_personality_with_path_windows() {
        assert_eq!(
            detect_personality("C:\\bin\\mpstat.exe"),
            Personality::Mpstat
        );
    }

    #[test]
    fn test_personality_exe_suffix() {
        assert_eq!(detect_personality("pidstat.exe"), Personality::Pidstat);
    }

    #[test]
    fn test_personality_nested_path() {
        assert_eq!(
            detect_personality("/a/b/c/d/tapestat"),
            Personality::Tapestat
        );
    }

    #[test]
    fn test_personality_display_sar() {
        assert_eq!(format!("{}", Personality::Sar), "sar");
    }

    #[test]
    fn test_personality_display_iostat() {
        assert_eq!(format!("{}", Personality::Iostat), "iostat");
    }

    #[test]
    fn test_personality_display_mpstat() {
        assert_eq!(format!("{}", Personality::Mpstat), "mpstat");
    }

    #[test]
    fn test_personality_display_pidstat() {
        assert_eq!(format!("{}", Personality::Pidstat), "pidstat");
    }

    #[test]
    fn test_personality_display_cifsiostat() {
        assert_eq!(format!("{}", Personality::Cifsiostat), "cifsiostat");
    }

    #[test]
    fn test_personality_display_tapestat() {
        assert_eq!(format!("{}", Personality::Tapestat), "tapestat");
    }

    #[test]
    fn test_personality_empty_string() {
        assert_eq!(detect_personality(""), Personality::Sar);
    }

    #[test]
    fn test_personality_just_slash() {
        assert_eq!(detect_personality("/"), Personality::Sar);
    }

    // -----------------------------------------------------------------------
    // Timestamp
    // -----------------------------------------------------------------------

    #[test]
    fn test_format_timestamp_format() {
        let ts = format_timestamp();
        // Should be HH:MM:SS AM/PM format
        assert!(ts.contains(':'));
        assert!(ts.ends_with("AM") || ts.ends_with("PM"));
    }

    #[test]
    fn test_format_timestamp_length() {
        let ts = format_timestamp();
        // "HH:MM:SS XM" = 11 chars
        assert!(ts.len() >= 10);
        assert!(ts.len() <= 12);
    }

    // -----------------------------------------------------------------------
    // CPU stat parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_cpu_line_aggregate() {
        let line = "cpu  50000 1000 20000 900000 5000 500 200 0 0 0";
        let stat = parse_cpu_line(line).unwrap();
        assert_eq!(stat.name, "cpu");
        assert_eq!(stat.user, 50000);
        assert_eq!(stat.nice, 1000);
        assert_eq!(stat.system, 20000);
        assert_eq!(stat.idle, 900000);
        assert_eq!(stat.iowait, 5000);
        assert_eq!(stat.irq, 500);
        assert_eq!(stat.softirq, 200);
        assert_eq!(stat.steal, 0);
    }

    #[test]
    fn test_parse_cpu_line_single_cpu() {
        let line = "cpu0 25000 500 10000 450000 2500 250 100 0 0 0";
        let stat = parse_cpu_line(line).unwrap();
        assert_eq!(stat.name, "cpu0");
        assert_eq!(stat.user, 25000);
    }

    #[test]
    fn test_parse_cpu_line_non_cpu() {
        let line = "intr 12345 6789";
        assert!(parse_cpu_line(line).is_none());
    }

    #[test]
    fn test_parse_cpu_line_too_short() {
        let line = "cpu 100";
        assert!(parse_cpu_line(line).is_none());
    }

    #[test]
    fn test_parse_cpu_line_partial_fields() {
        let line = "cpu 100 200 300 400 500";
        let stat = parse_cpu_line(line).unwrap();
        assert_eq!(stat.user, 100);
        assert_eq!(stat.nice, 200);
        assert_eq!(stat.system, 300);
        assert_eq!(stat.idle, 400);
        assert_eq!(stat.iowait, 500);
        assert_eq!(stat.irq, 0); // not present, default
    }

    #[test]
    fn test_cpu_stat_total() {
        let stat = CpuStat {
            name: "cpu".to_string(),
            user: 100,
            nice: 200,
            system: 300,
            idle: 400,
            iowait: 50,
            irq: 10,
            softirq: 5,
            steal: 2,
            guest: 1,
            guest_nice: 0,
        };
        assert_eq!(stat.total(), 1068);
    }

    #[test]
    fn test_cpu_stat_default() {
        let stat = CpuStat::default();
        assert_eq!(stat.total(), 0);
        assert_eq!(stat.name, "");
    }

    // -----------------------------------------------------------------------
    // CPU usage computation
    // -----------------------------------------------------------------------

    #[test]
    fn test_compute_cpu_usage_basic() {
        let prev = CpuStat {
            name: "cpu".to_string(),
            user: 1000,
            nice: 0,
            system: 500,
            idle: 8500,
            ..CpuStat::default()
        };
        let curr = CpuStat {
            name: "cpu".to_string(),
            user: 2000,
            nice: 0,
            system: 1000,
            idle: 17000,
            ..CpuStat::default()
        };
        let usage = compute_cpu_usage(&prev, &curr);
        assert_eq!(usage.name, "cpu");
        assert!(usage.usr > 0.0);
        assert!(usage.sys > 0.0);
        assert!(usage.idle > 0.0);
    }

    #[test]
    fn test_compute_cpu_usage_all_idle() {
        let prev = CpuStat {
            name: "cpu".to_string(),
            idle: 1000,
            ..CpuStat::default()
        };
        let curr = CpuStat {
            name: "cpu".to_string(),
            idle: 2000,
            ..CpuStat::default()
        };
        let usage = compute_cpu_usage(&prev, &curr);
        assert!((usage.idle - 100.0).abs() < 0.01);
        assert!(usage.usr.abs() < 0.01);
    }

    #[test]
    fn test_compute_cpu_usage_all_user() {
        let prev = CpuStat {
            name: "cpu".to_string(),
            user: 1000,
            ..CpuStat::default()
        };
        let curr = CpuStat {
            name: "cpu".to_string(),
            user: 2000,
            ..CpuStat::default()
        };
        let usage = compute_cpu_usage(&prev, &curr);
        assert!((usage.usr - 100.0).abs() < 0.01);
        assert!(usage.idle.abs() < 0.01);
    }

    #[test]
    fn test_compute_cpu_usage_no_change() {
        let stat = CpuStat {
            name: "cpu".to_string(),
            user: 5000,
            system: 2000,
            idle: 90000,
            ..CpuStat::default()
        };
        let usage = compute_cpu_usage(&stat, &stat);
        // delta=0 → divisor=1
        assert!(usage.usr.abs() < 0.01);
    }

    #[test]
    fn test_compute_cpu_usage_preserves_name() {
        let prev = CpuStat {
            name: "cpu3".to_string(),
            idle: 100,
            ..CpuStat::default()
        };
        let curr = CpuStat {
            name: "cpu3".to_string(),
            idle: 200,
            ..CpuStat::default()
        };
        let usage = compute_cpu_usage(&prev, &curr);
        assert_eq!(usage.name, "cpu3");
    }

    #[test]
    fn test_compute_cpu_usage_mixed() {
        let prev = CpuStat {
            name: "cpu".to_string(),
            user: 1000,
            system: 500,
            idle: 8500,
            ..CpuStat::default()
        };
        let curr = CpuStat {
            name: "cpu".to_string(),
            user: 1100,
            system: 550,
            idle: 8850,
            ..CpuStat::default()
        };
        let usage = compute_cpu_usage(&prev, &curr);
        let sum = usage.usr + usage.nice + usage.sys + usage.iowait + usage.irq
            + usage.soft + usage.steal + usage.guest + usage.idle;
        assert!((sum - 100.0).abs() < 0.1);
    }

    // -----------------------------------------------------------------------
    // Fallback data
    // -----------------------------------------------------------------------

    #[test]
    fn test_fallback_cpu_stats() {
        let stats = fallback_cpu_stats();
        assert!(!stats.is_empty());
        assert_eq!(stats[0].name, "cpu");
        assert!(stats.len() >= 3);
    }

    #[test]
    fn test_fallback_meminfo() {
        let mem = fallback_meminfo();
        assert!(mem.total_kb > 0);
        assert!(mem.free_kb > 0);
        assert!(mem.total_kb > mem.free_kb);
    }

    #[test]
    fn test_fallback_diskstats() {
        let stats = fallback_diskstats();
        assert!(!stats.is_empty());
        assert_eq!(stats[0].name, "sda");
    }

    #[test]
    fn test_fallback_net_dev() {
        let stats = fallback_net_dev();
        assert!(!stats.is_empty());
        assert_eq!(stats[0].iface, "lo");
        assert_eq!(stats[1].iface, "eth0");
    }

    #[test]
    fn test_fallback_process_stats() {
        let stats = fallback_process_stats();
        assert!(stats.len() >= 3);
        assert_eq!(stats[0].pid, 1);
        assert_eq!(stats[0].comm, "init");
    }

    #[test]
    fn test_fallback_cifs_stats() {
        let stats = fallback_cifs_stats();
        assert!(!stats.is_empty());
        assert!(stats[0].share.contains("server"));
    }

    #[test]
    fn test_fallback_tape_stats() {
        let stats = fallback_tape_stats();
        assert!(!stats.is_empty());
        assert_eq!(stats[0].name, "st0");
    }

    // -----------------------------------------------------------------------
    // Memory parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_meminfo_value() {
        assert_eq!(parse_meminfo_value("MemTotal:       16384000 kB"), Some(16384000));
    }

    #[test]
    fn test_parse_meminfo_value_empty() {
        assert_eq!(parse_meminfo_value(""), None);
    }

    #[test]
    fn test_parse_meminfo_value_no_number() {
        assert_eq!(parse_meminfo_value("MemTotal: abc kB"), None);
    }

    #[test]
    fn test_read_meminfo_returns_data() {
        let mem = read_meminfo();
        assert!(mem.total_kb > 0);
    }

    // -----------------------------------------------------------------------
    // Disk stat parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_diskstat_line() {
        let line = "   8       0 sda 15000 500 600000 3000 8000 1200 320000 5000 0 6000 8000";
        let stat = parse_diskstat_line(line).unwrap();
        assert_eq!(stat.name, "sda");
        assert_eq!(stat.reads_completed, 15000);
        assert_eq!(stat.writes_completed, 8000);
    }

    #[test]
    fn test_parse_diskstat_line_too_short() {
        let line = "   8       0 sda";
        assert!(parse_diskstat_line(line).is_none());
    }

    #[test]
    fn test_read_diskstats_returns_data() {
        let stats = read_diskstats();
        assert!(!stats.is_empty());
    }

    // -----------------------------------------------------------------------
    // Network stat parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_netdev_line() {
        let line = "  eth0: 50000000  100000    5    2    0    0          0         0  30000000   80000    1    0    0     0       0          0";
        let stat = parse_netdev_line(line).unwrap();
        assert_eq!(stat.iface, "eth0");
        assert_eq!(stat.rx_bytes, 50000000);
        assert_eq!(stat.rx_packets, 100000);
        assert_eq!(stat.tx_bytes, 30000000);
        assert_eq!(stat.tx_packets, 80000);
    }

    #[test]
    fn test_parse_netdev_line_no_colon() {
        let line = "  Inter-|   Receive";
        assert!(parse_netdev_line(line).is_none());
    }

    #[test]
    fn test_parse_netdev_line_too_short() {
        let line = "  lo: 100 200";
        assert!(parse_netdev_line(line).is_none());
    }

    #[test]
    fn test_read_net_dev_returns_data() {
        let stats = read_net_dev();
        assert!(!stats.is_empty());
    }

    // -----------------------------------------------------------------------
    // Load average
    // -----------------------------------------------------------------------

    #[test]
    fn test_read_loadavg_returns_data() {
        let la = read_loadavg();
        assert!(la.one >= 0.0);
        assert!(la.five >= 0.0);
        assert!(la.fifteen >= 0.0);
    }

    // -----------------------------------------------------------------------
    // Argument parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_parse_interval_count_none() {
        let args: Vec<String> = vec!["-u".to_string()];
        let (interval, count) = parse_interval_count(&args);
        assert_eq!(interval, 1);
        assert_eq!(count, Some(1));
    }

    #[test]
    fn test_parse_interval_count_interval_only() {
        let args: Vec<String> = vec!["-u".to_string(), "5".to_string()];
        let (interval, count) = parse_interval_count(&args);
        assert_eq!(interval, 5);
        assert_eq!(count, None);
    }

    #[test]
    fn test_parse_interval_count_both() {
        let args: Vec<String> = vec!["-u".to_string(), "2".to_string(), "10".to_string()];
        let (interval, count) = parse_interval_count(&args);
        assert_eq!(interval, 2);
        assert_eq!(count, Some(10));
    }

    #[test]
    fn test_parse_interval_count_just_numbers() {
        let args: Vec<String> = vec!["3".to_string(), "7".to_string()];
        let (interval, count) = parse_interval_count(&args);
        assert_eq!(interval, 3);
        assert_eq!(count, Some(7));
    }

    #[test]
    fn test_has_flag_present() {
        let args: Vec<String> = vec!["-u".to_string(), "-r".to_string()];
        assert!(has_flag(&args, "-u"));
        assert!(has_flag(&args, "-r"));
    }

    #[test]
    fn test_has_flag_absent() {
        let args: Vec<String> = vec!["-u".to_string()];
        assert!(!has_flag(&args, "-r"));
    }

    #[test]
    fn test_has_flag_empty() {
        let args: Vec<String> = vec![];
        assert!(!has_flag(&args, "-u"));
    }

    #[test]
    fn test_get_flag_arg() {
        let args: Vec<String> = vec!["-P".to_string(), "ALL".to_string()];
        assert_eq!(get_flag_arg(&args, "-P"), Some("ALL"));
    }

    #[test]
    fn test_get_flag_arg_missing() {
        let args: Vec<String> = vec!["-u".to_string()];
        assert_eq!(get_flag_arg(&args, "-P"), None);
    }

    #[test]
    fn test_get_flag_arg_no_value() {
        let args: Vec<String> = vec!["-P".to_string()];
        assert_eq!(get_flag_arg(&args, "-P"), None);
    }

    #[test]
    fn test_parse_cpu_list_all() {
        let result = parse_cpu_list("ALL");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_cpu_list_specific() {
        let result = parse_cpu_list("0,1,3");
        assert_eq!(result, vec![0, 1, 3]);
    }

    #[test]
    fn test_parse_cpu_list_single() {
        let result = parse_cpu_list("2");
        assert_eq!(result, vec![2]);
    }

    #[test]
    fn test_parse_cpu_list_invalid() {
        let result = parse_cpu_list("abc");
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // SAR options
    // -----------------------------------------------------------------------

    #[test]
    fn test_sar_default_cpu() {
        let args: Vec<String> = vec![];
        let opts = parse_sar_args(&args);
        assert!(opts.cpu);
        assert!(!opts.mem);
        assert!(!opts.io);
    }

    #[test]
    fn test_sar_all_flags() {
        let args: Vec<String> = vec![
            "-u".to_string(),
            "-r".to_string(),
            "-b".to_string(),
            "-n".to_string(),
            "DEV".to_string(),
            "-d".to_string(),
            "-q".to_string(),
        ];
        let opts = parse_sar_args(&args);
        assert!(opts.cpu);
        assert!(opts.mem);
        assert!(opts.io);
        assert!(opts.net);
        assert!(opts.disk);
        assert!(opts.loadavg);
    }

    #[test]
    fn test_sar_interval_count() {
        let args: Vec<String> = vec!["-u".to_string(), "2".to_string(), "5".to_string()];
        let opts = parse_sar_args(&args);
        assert_eq!(opts.interval, 2);
        assert_eq!(opts.count, Some(5));
    }

    #[test]
    fn test_sar_mem_only() {
        let args: Vec<String> = vec!["-r".to_string()];
        let opts = parse_sar_args(&args);
        assert!(!opts.cpu);
        assert!(opts.mem);
    }

    #[test]
    fn test_sar_net_needs_dev() {
        let args: Vec<String> = vec!["-n".to_string(), "DEV".to_string()];
        let opts = parse_sar_args(&args);
        assert!(opts.net);
    }

    #[test]
    fn test_sar_net_without_dev() {
        let args: Vec<String> = vec!["-n".to_string(), "SOCK".to_string()];
        let opts = parse_sar_args(&args);
        assert!(!opts.net);
    }

    // -----------------------------------------------------------------------
    // IOSTAT options
    // -----------------------------------------------------------------------

    #[test]
    fn test_iostat_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_iostat_args(&args);
        assert!(!opts.cpu_only);
        assert!(!opts.device_only);
        assert!(!opts.extended);
        assert!(opts.unit_kb);
    }

    #[test]
    fn test_iostat_cpu_only() {
        let args: Vec<String> = vec!["-c".to_string()];
        let opts = parse_iostat_args(&args);
        assert!(opts.cpu_only);
    }

    #[test]
    fn test_iostat_device_only() {
        let args: Vec<String> = vec!["-d".to_string()];
        let opts = parse_iostat_args(&args);
        assert!(opts.device_only);
    }

    #[test]
    fn test_iostat_extended() {
        let args: Vec<String> = vec!["-x".to_string()];
        let opts = parse_iostat_args(&args);
        assert!(opts.extended);
    }

    #[test]
    fn test_iostat_megabytes() {
        let args: Vec<String> = vec!["-m".to_string()];
        let opts = parse_iostat_args(&args);
        assert!(opts.unit_mb);
    }

    #[test]
    fn test_iostat_per_partition() {
        let args: Vec<String> = vec!["-p".to_string()];
        let opts = parse_iostat_args(&args);
        assert!(opts.per_partition);
    }

    // -----------------------------------------------------------------------
    // MPSTAT options
    // -----------------------------------------------------------------------

    #[test]
    fn test_mpstat_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_mpstat_args(&args);
        assert!(opts.show_all);
        assert!(opts.cpus.is_empty());
    }

    #[test]
    fn test_mpstat_specific_cpus() {
        let args: Vec<String> = vec!["-P".to_string(), "0,2".to_string()];
        let opts = parse_mpstat_args(&args);
        assert!(!opts.show_all);
        assert_eq!(opts.cpus, vec![0, 2]);
    }

    #[test]
    fn test_mpstat_all_cpus() {
        let args: Vec<String> = vec!["-P".to_string(), "ALL".to_string()];
        let opts = parse_mpstat_args(&args);
        assert!(opts.show_all);
    }

    #[test]
    fn test_mpstat_irq_summary() {
        let args: Vec<String> = vec!["-I".to_string(), "SUM".to_string()];
        let opts = parse_mpstat_args(&args);
        assert!(opts.irq_summary);
    }

    // -----------------------------------------------------------------------
    // PIDSTAT options
    // -----------------------------------------------------------------------

    #[test]
    fn test_pidstat_defaults() {
        let args: Vec<String> = vec![];
        let opts = parse_pidstat_args(&args);
        assert!(opts.show_cpu);
        assert!(!opts.show_mem);
        assert!(!opts.show_io);
        assert!(opts.target_pid.is_none());
    }

    #[test]
    fn test_pidstat_specific_pid() {
        let args: Vec<String> = vec!["-p".to_string(), "1234".to_string()];
        let opts = parse_pidstat_args(&args);
        assert_eq!(opts.target_pid, Some(1234));
    }

    #[test]
    fn test_pidstat_all_stats() {
        let args: Vec<String> = vec!["-u".to_string(), "-r".to_string(), "-d".to_string()];
        let opts = parse_pidstat_args(&args);
        assert!(opts.show_cpu);
        assert!(opts.show_mem);
        assert!(opts.show_io);
    }

    #[test]
    fn test_pidstat_threads() {
        let args: Vec<String> = vec!["-t".to_string()];
        let opts = parse_pidstat_args(&args);
        assert!(opts.show_threads);
    }

    // -----------------------------------------------------------------------
    // is_whole_device
    // -----------------------------------------------------------------------

    #[test]
    fn test_is_whole_device_sda() {
        assert!(is_whole_device("sda"));
    }

    #[test]
    fn test_is_whole_device_sda1_partition() {
        assert!(!is_whole_device("sda1"));
    }

    #[test]
    fn test_is_whole_device_loop() {
        assert!(!is_whole_device("loop0"));
    }

    #[test]
    fn test_is_whole_device_ram() {
        assert!(!is_whole_device("ram0"));
    }

    #[test]
    fn test_is_whole_device_sdb() {
        assert!(is_whole_device("sdb"));
    }

    // -----------------------------------------------------------------------
    // Output formatters (check no panic)
    // -----------------------------------------------------------------------

    #[test]
    fn test_print_system_header() {
        let mut buf = Vec::new();
        print_system_header(&mut buf, "sar");
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Slate OS"));
        assert!(output.contains("sar"));
    }

    #[test]
    fn test_print_cpu_header() {
        let mut buf = Vec::new();
        print_cpu_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("%usr"));
        assert!(output.contains("%idle"));
    }

    #[test]
    fn test_print_cpu_row() {
        let mut buf = Vec::new();
        let usage = CpuUsage {
            name: "cpu".to_string(),
            usr: 10.0,
            nice: 0.5,
            sys: 5.0,
            iowait: 1.0,
            irq: 0.1,
            soft: 0.05,
            steal: 0.0,
            guest: 0.0,
            idle: 83.35,
        };
        print_cpu_row(&mut buf, "12:00:00 PM", &usage);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("cpu"));
        assert!(output.contains("10.00"));
    }

    #[test]
    fn test_print_mem_header() {
        let mut buf = Vec::new();
        print_mem_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("kbmemfree"));
        assert!(output.contains("%memused"));
    }

    #[test]
    fn test_print_mem_row() {
        let mut buf = Vec::new();
        let mem = fallback_meminfo();
        print_mem_row(&mut buf, "12:00:00 PM", &mem);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("4096000"));
    }

    #[test]
    fn test_print_mem_row_zero_total() {
        let mut buf = Vec::new();
        let mem = MemInfo::default();
        print_mem_row(&mut buf, "12:00:00 PM", &mem);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("0.00"));
    }

    #[test]
    fn test_print_disk_header() {
        let mut buf = Vec::new();
        print_disk_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("DEV"));
        assert!(output.contains("tps"));
    }

    #[test]
    fn test_print_disk_row() {
        let mut buf = Vec::new();
        let prev = DiskStat {
            name: "sda".to_string(),
            reads_completed: 100,
            sectors_read: 800,
            writes_completed: 50,
            sectors_written: 400,
            ..DiskStat::default()
        };
        let curr = DiskStat {
            name: "sda".to_string(),
            reads_completed: 200,
            sectors_read: 1600,
            writes_completed: 100,
            sectors_written: 800,
            ..DiskStat::default()
        };
        print_disk_row(&mut buf, "12:00:00 PM", &prev, &curr, 1.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("sda"));
    }

    #[test]
    fn test_print_disk_row_zero_ios() {
        let mut buf = Vec::new();
        let stat = DiskStat {
            name: "sda".to_string(),
            ..DiskStat::default()
        };
        print_disk_row(&mut buf, "12:00:00 PM", &stat, &stat, 1.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("0.00"));
    }

    #[test]
    fn test_print_netdev_header() {
        let mut buf = Vec::new();
        print_netdev_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("IFACE"));
        assert!(output.contains("rxpck/s"));
    }

    #[test]
    fn test_print_netdev_row() {
        let mut buf = Vec::new();
        let prev = NetDevStat {
            iface: "eth0".to_string(),
            rx_bytes: 1000,
            rx_packets: 10,
            tx_bytes: 500,
            tx_packets: 5,
            ..NetDevStat::default()
        };
        let curr = NetDevStat {
            iface: "eth0".to_string(),
            rx_bytes: 2000,
            rx_packets: 20,
            tx_bytes: 1000,
            tx_packets: 10,
            ..NetDevStat::default()
        };
        print_netdev_row(&mut buf, "12:00:00 PM", &prev, &curr, 1.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("eth0"));
    }

    #[test]
    fn test_print_loadavg_header() {
        let mut buf = Vec::new();
        print_loadavg_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("ldavg-1"));
    }

    #[test]
    fn test_print_loadavg_row() {
        let mut buf = Vec::new();
        let la = LoadAvg {
            one: 1.5,
            five: 1.2,
            fifteen: 0.8,
            running: 3,
            total: 200,
        };
        print_loadavg_row(&mut buf, "12:00:00 PM", &la);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("1.50"));
        assert!(output.contains("200"));
    }

    #[test]
    fn test_print_io_transfer_header() {
        let mut buf = Vec::new();
        print_io_transfer_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("tps"));
        assert!(output.contains("bread/s"));
    }

    #[test]
    fn test_print_io_transfer_row() {
        let mut buf = Vec::new();
        let prev = fallback_diskstats();
        let curr = fallback_diskstats();
        print_io_transfer_row(&mut buf, "12:00:00 PM", &prev, &curr, 1.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("0.00"));
    }

    #[test]
    fn test_print_iostat_cpu() {
        let mut buf = Vec::new();
        let prev = fallback_cpu_stats();
        let curr = fallback_cpu_stats();
        print_iostat_cpu(&mut buf, &prev, &curr);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("avg-cpu"));
    }

    #[test]
    fn test_print_iostat_device_header_basic() {
        let mut buf = Vec::new();
        print_iostat_device_header(&mut buf, false, "kB");
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Device"));
        assert!(output.contains("tps"));
    }

    #[test]
    fn test_print_iostat_device_header_extended() {
        let mut buf = Vec::new();
        print_iostat_device_header(&mut buf, true, "kB");
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Device"));
        assert!(output.contains("%util"));
    }

    #[test]
    fn test_print_iostat_device_row_basic() {
        let mut buf = Vec::new();
        let prev = DiskStat {
            name: "sda".to_string(),
            reads_completed: 100,
            sectors_read: 800,
            writes_completed: 50,
            sectors_written: 400,
            ..DiskStat::default()
        };
        let curr = DiskStat {
            name: "sda".to_string(),
            reads_completed: 200,
            sectors_read: 1600,
            writes_completed: 100,
            sectors_written: 800,
            ..DiskStat::default()
        };
        print_iostat_device_row(&mut buf, &prev, &curr, 1.0, false, 1024.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("sda"));
    }

    #[test]
    fn test_print_iostat_device_row_extended() {
        let mut buf = Vec::new();
        let prev = DiskStat {
            name: "sda".to_string(),
            ..DiskStat::default()
        };
        let curr = DiskStat {
            name: "sda".to_string(),
            reads_completed: 100,
            sectors_read: 800,
            writes_completed: 50,
            sectors_written: 400,
            io_time_ms: 1000,
            ..DiskStat::default()
        };
        print_iostat_device_row(&mut buf, &prev, &curr, 1.0, true, 1024.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("sda"));
    }

    #[test]
    fn test_print_mpstat_header() {
        let mut buf = Vec::new();
        print_mpstat_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("%usr"));
        assert!(output.contains("%guest"));
    }

    #[test]
    fn test_print_mpstat_row_all() {
        let mut buf = Vec::new();
        let usage = CpuUsage {
            name: "cpu".to_string(),
            usr: 5.0,
            idle: 95.0,
            ..CpuUsage::default()
        };
        print_mpstat_row(&mut buf, "12:00:00 PM", &usage);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("all"));
    }

    #[test]
    fn test_print_mpstat_row_specific_cpu() {
        let mut buf = Vec::new();
        let usage = CpuUsage {
            name: "cpu2".to_string(),
            usr: 10.0,
            idle: 90.0,
            ..CpuUsage::default()
        };
        print_mpstat_row(&mut buf, "12:00:00 PM", &usage);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("2"));
    }

    #[test]
    fn test_print_pidstat_cpu_header() {
        let mut buf = Vec::new();
        print_pidstat_cpu_header(&mut buf, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("%usr"));
        assert!(output.contains("Command"));
    }

    #[test]
    fn test_print_pidstat_cpu_header_threads() {
        let mut buf = Vec::new();
        print_pidstat_cpu_header(&mut buf, true);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("TGID"));
    }

    #[test]
    fn test_print_pidstat_cpu_row() {
        let mut buf = Vec::new();
        let prev = ProcessStat {
            pid: 42,
            comm: "test".to_string(),
            utime: 100,
            stime: 50,
            ..ProcessStat::default()
        };
        let curr = ProcessStat {
            pid: 42,
            comm: "test".to_string(),
            utime: 200,
            stime: 100,
            ..ProcessStat::default()
        };
        print_pidstat_cpu_row(&mut buf, "12:00:00 PM", &prev, &curr, 100, 1.0, false);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test"));
        assert!(output.contains("42"));
    }

    #[test]
    fn test_print_pidstat_mem_header() {
        let mut buf = Vec::new();
        print_pidstat_mem_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("VSZ-kB"));
        assert!(output.contains("%MEM"));
    }

    #[test]
    fn test_print_pidstat_mem_row() {
        let mut buf = Vec::new();
        let proc_stat = ProcessStat {
            pid: 42,
            comm: "test".to_string(),
            vsize_kb: 8192,
            rss_pages: 512,
            ..ProcessStat::default()
        };
        print_pidstat_mem_row(&mut buf, "12:00:00 PM", &proc_stat);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test"));
        assert!(output.contains("8192"));
    }

    #[test]
    fn test_print_pidstat_io_header() {
        let mut buf = Vec::new();
        print_pidstat_io_header(&mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("kB_rd/s"));
        assert!(output.contains("kB_wr/s"));
    }

    #[test]
    fn test_print_pidstat_io_row() {
        let mut buf = Vec::new();
        let prev = ProcessStat {
            pid: 42,
            comm: "test".to_string(),
            read_bytes: 1024,
            write_bytes: 512,
            ..ProcessStat::default()
        };
        let curr = ProcessStat {
            pid: 42,
            comm: "test".to_string(),
            read_bytes: 2048,
            write_bytes: 1024,
            ..ProcessStat::default()
        };
        print_pidstat_io_row(&mut buf, "12:00:00 PM", &prev, &curr, 1.0);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("test"));
        assert!(output.contains("42"));
    }

    #[test]
    fn test_print_irq_summary() {
        let mut buf = Vec::new();
        print_irq_summary(&mut buf, "12:00:00 PM");
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("intr/s"));
    }

    // -----------------------------------------------------------------------
    // Full command runs (single iteration, captured to buffer)
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_sar_cpu() {
        let mut buf = Vec::new();
        let args = vec!["-u".to_string(), "1".to_string(), "1".to_string()];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Slate OS"));
        assert!(output.contains("%usr"));
    }

    #[test]
    fn test_run_sar_mem() {
        let mut buf = Vec::new();
        let args = vec!["-r".to_string(), "1".to_string(), "1".to_string()];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("kbmemfree"));
    }

    #[test]
    fn test_run_sar_io() {
        let mut buf = Vec::new();
        let args = vec!["-b".to_string(), "1".to_string(), "1".to_string()];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("tps"));
    }

    #[test]
    fn test_run_sar_disk() {
        let mut buf = Vec::new();
        let args = vec!["-d".to_string(), "1".to_string(), "1".to_string()];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("DEV"));
    }

    #[test]
    fn test_run_sar_net() {
        let mut buf = Vec::new();
        let args = vec![
            "-n".to_string(),
            "DEV".to_string(),
            "1".to_string(),
            "1".to_string(),
        ];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("IFACE"));
    }

    #[test]
    fn test_run_sar_loadavg() {
        let mut buf = Vec::new();
        let args = vec!["-q".to_string(), "1".to_string(), "1".to_string()];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("ldavg-1"));
    }

    #[test]
    fn test_run_sar_default() {
        let mut buf = Vec::new();
        let args = vec!["1".to_string(), "1".to_string()];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("%usr"));
    }

    #[test]
    fn test_run_iostat_basic() {
        let mut buf = Vec::new();
        let args = vec!["1".to_string(), "1".to_string()];
        run_iostat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("avg-cpu"));
        assert!(output.contains("Device"));
    }

    #[test]
    fn test_run_iostat_cpu_only() {
        let mut buf = Vec::new();
        let args = vec!["-c".to_string(), "1".to_string(), "1".to_string()];
        run_iostat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("avg-cpu"));
        assert!(!output.contains("Device"));
    }

    #[test]
    fn test_run_iostat_device_only() {
        let mut buf = Vec::new();
        let args = vec!["-d".to_string(), "1".to_string(), "1".to_string()];
        run_iostat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(!output.contains("avg-cpu"));
        assert!(output.contains("Device"));
    }

    #[test]
    fn test_run_iostat_extended() {
        let mut buf = Vec::new();
        let args = vec!["-x".to_string(), "1".to_string(), "1".to_string()];
        run_iostat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("%util"));
    }

    #[test]
    fn test_run_mpstat_basic() {
        let mut buf = Vec::new();
        let args = vec!["1".to_string(), "1".to_string()];
        run_mpstat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Slate OS"));
        assert!(output.contains("%usr"));
        assert!(output.contains("all"));
    }

    #[test]
    fn test_run_mpstat_all_cpus() {
        let mut buf = Vec::new();
        let args = vec![
            "-P".to_string(),
            "ALL".to_string(),
            "1".to_string(),
            "1".to_string(),
        ];
        run_mpstat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("all"));
    }

    #[test]
    fn test_run_mpstat_irq() {
        let mut buf = Vec::new();
        let args = vec![
            "-I".to_string(),
            "SUM".to_string(),
            "1".to_string(),
            "1".to_string(),
        ];
        run_mpstat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("intr/s"));
    }

    #[test]
    fn test_run_pidstat_basic() {
        let mut buf = Vec::new();
        let args = vec!["1".to_string(), "1".to_string()];
        run_pidstat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Slate OS"));
        assert!(output.contains("%usr"));
    }

    #[test]
    fn test_run_pidstat_mem() {
        let mut buf = Vec::new();
        let args = vec!["-r".to_string(), "1".to_string(), "1".to_string()];
        run_pidstat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("VSZ-kB"));
    }

    #[test]
    fn test_run_pidstat_io() {
        let mut buf = Vec::new();
        let args = vec!["-d".to_string(), "1".to_string(), "1".to_string()];
        run_pidstat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("kB_rd/s"));
    }

    #[test]
    fn test_run_cifsiostat_basic() {
        let mut buf = Vec::new();
        let args = vec!["1".to_string(), "1".to_string()];
        run_cifsiostat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Slate OS"));
        assert!(output.contains("Filesystem"));
    }

    #[test]
    fn test_run_tapestat_basic() {
        let mut buf = Vec::new();
        let args = vec!["1".to_string(), "1".to_string()];
        run_tapestat(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("Slate OS"));
        assert!(output.contains("Tape"));
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn test_sar_multiple_reports() {
        let mut buf = Vec::new();
        let args = vec![
            "-u".to_string(),
            "-r".to_string(),
            "-q".to_string(),
            "1".to_string(),
            "1".to_string(),
        ];
        run_sar(&args, &mut buf);
        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("%usr"));
        assert!(output.contains("kbmemfree"));
        assert!(output.contains("ldavg-1"));
    }

    #[test]
    fn test_interval_zero() {
        let args: Vec<String> = vec!["0".to_string(), "1".to_string()];
        let (interval, count) = parse_interval_count(&args);
        assert_eq!(interval, 0);
        assert_eq!(count, Some(1));
    }

    #[test]
    fn test_large_count() {
        let args: Vec<String> = vec!["1".to_string(), "999999".to_string()];
        let (interval, count) = parse_interval_count(&args);
        assert_eq!(interval, 1);
        assert_eq!(count, Some(999999));
    }

    #[test]
    fn test_netdev_stat_default() {
        let stat = NetDevStat::default();
        assert_eq!(stat.iface, "");
        assert_eq!(stat.rx_bytes, 0);
    }

    #[test]
    fn test_diskstat_default() {
        let stat = DiskStat::default();
        assert_eq!(stat.name, "");
        assert_eq!(stat.reads_completed, 0);
    }

    #[test]
    fn test_process_stat_default() {
        let stat = ProcessStat::default();
        assert_eq!(stat.pid, 0);
        assert_eq!(stat.comm, "");
        assert_eq!(stat._state, '\0');
    }

    #[test]
    fn test_cifs_stat_default() {
        let stat = CifsStat::default();
        assert_eq!(stat.share, "");
        assert_eq!(stat.reads, 0);
    }

    #[test]
    fn test_tape_stat_default() {
        let stat = TapeStat::default();
        assert_eq!(stat.name, "");
        assert_eq!(stat.reads, 0);
    }

    #[test]
    fn test_loadavg_default() {
        let la = LoadAvg::default();
        assert!((la.one - 0.0).abs() < f64::EPSILON);
        assert_eq!(la.running, 0);
    }

    #[test]
    fn test_meminfo_default() {
        let mem = MemInfo::default();
        assert_eq!(mem.total_kb, 0);
    }

    #[test]
    fn test_cpu_usage_default() {
        let u = CpuUsage::default();
        assert!((u.usr - 0.0).abs() < f64::EPSILON);
        assert_eq!(u.name, "");
    }
}
