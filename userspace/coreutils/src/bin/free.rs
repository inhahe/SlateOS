//! free — display amount of free and used memory.
//!
//! Usage: free [-h] [-k] [-m] [-g]
//!   -h  human-readable output
//!   -k  show in KiB (default)
//!   -m  show in MiB
//!   -g  show in GiB
//!
//! Reads from /proc/meminfo.

use std::env;
use std::fs;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut human = false;
    let mut unit: u64 = 1; // KiB (meminfo reports in KiB)
    let mut unit_name = "KiB";

    for arg in &args {
        match arg.as_str() {
            "-h" => human = true,
            "-k" => {
                unit = 1;
                unit_name = "KiB";
            }
            "-m" => {
                unit = 1024;
                unit_name = "MiB";
            }
            "-g" => {
                unit = 1024 * 1024;
                unit_name = "GiB";
            }
            _ => {}
        }
    }

    let meminfo = match fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => {
            eprintln!("free: cannot read /proc/meminfo");
            std::process::exit(1);
        }
    };

    let mut total: u64 = 0;
    let mut free: u64 = 0;
    let mut available: u64 = 0;
    let mut buffers: u64 = 0;
    let mut cached: u64 = 0;
    let mut swap_total: u64 = 0;
    let mut swap_free: u64 = 0;

    for line in meminfo.lines() {
        if let Some((key, val)) = line.split_once(':') {
            let val_kb: u64 = val
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            match key.trim() {
                "MemTotal" => total = val_kb,
                "MemFree" => free = val_kb,
                "MemAvailable" => available = val_kb,
                "Buffers" => buffers = val_kb,
                "Cached" => cached = val_kb,
                "SwapTotal" => swap_total = val_kb,
                "SwapFree" => swap_free = val_kb,
                _ => {}
            }
        }
    }

    let used = total.saturating_sub(free).saturating_sub(buffers).saturating_sub(cached);
    let buff_cache = buffers + cached;

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if human {
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "", "total", "used", "free", "shared", "buff/cache"
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "Mem:",
            human_size(total * 1024),
            human_size(used * 1024),
            human_size(free * 1024),
            human_size(0),
            human_size(buff_cache * 1024)
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10}",
            "Swap:",
            human_size(swap_total * 1024),
            human_size(swap_total.saturating_sub(swap_free) * 1024),
            human_size(swap_free * 1024)
        );
    } else {
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "", "total", "used", "free", "shared", "buff/cache", "available"
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            format!("Mem ({unit_name}):"),
            total / unit,
            used / unit,
            free / unit,
            0,
            buff_cache / unit,
            available / unit
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10}",
            format!("Swap ({unit_name}):"),
            swap_total / unit,
            swap_total.saturating_sub(swap_free) / unit,
            swap_free / unit
        );
    }
}

fn human_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}Gi", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1}Mi", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1}Ki", bytes as f64 / 1024.0)
    } else {
        format!("{bytes}B")
    }
}
