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

#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct FreeOpts {
    human: bool,
    unit: u64,
    unit_name: &'static str,
}

#[derive(Default)]
#[cfg_attr(test, derive(Debug, PartialEq, Eq))]
struct MemInfo {
    total: u64,
    free: u64,
    available: u64,
    buffers: u64,
    cached: u64,
    swap_total: u64,
    swap_free: u64,
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let opts = parse_args(&args);

    let meminfo_text = match fs::read_to_string("/proc/meminfo") {
        Ok(c) => c,
        Err(_) => {
            eprintln!("free: cannot read /proc/meminfo");
            std::process::exit(1);
        }
    };

    let mi = parse_meminfo(&meminfo_text);
    let used = mi.total.saturating_sub(mi.free).saturating_sub(mi.buffers).saturating_sub(mi.cached);
    let buff_cache = mi.buffers.saturating_add(mi.cached);

    let stdout = io::stdout();
    let mut out = stdout.lock();

    if opts.human {
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "", "total", "used", "free", "shared", "buff/cache"
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "Mem:",
            human_size(mi.total.saturating_mul(1024)),
            human_size(used.saturating_mul(1024)),
            human_size(mi.free.saturating_mul(1024)),
            human_size(0),
            human_size(buff_cache.saturating_mul(1024))
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10}",
            "Swap:",
            human_size(mi.swap_total.saturating_mul(1024)),
            human_size(mi.swap_total.saturating_sub(mi.swap_free).saturating_mul(1024)),
            human_size(mi.swap_free.saturating_mul(1024))
        );
    } else {
        let unit = opts.unit;
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "", "total", "used", "free", "shared", "buff/cache", "available"
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10}",
            format!("Mem ({}):", opts.unit_name),
            mi.total / unit,
            used / unit,
            mi.free / unit,
            0,
            buff_cache / unit,
            mi.available / unit
        );
        let _ = writeln!(
            out,
            "{:>15} {:>10} {:>10} {:>10}",
            format!("Swap ({}):", opts.unit_name),
            mi.swap_total / unit,
            mi.swap_total.saturating_sub(mi.swap_free) / unit,
            mi.swap_free / unit
        );
    }
}

/// Parse free's argv. Later flags override earlier ones for the unit.
fn parse_args(args: &[String]) -> FreeOpts {
    let mut human = false;
    let mut unit: u64 = 1;
    let mut unit_name: &'static str = "KiB";

    for arg in args {
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

    FreeOpts { human, unit, unit_name }
}

/// Parse `/proc/meminfo` content into the named fields we care about.
fn parse_meminfo(content: &str) -> MemInfo {
    let mut mi = MemInfo::default();
    for line in content.lines() {
        let Some((key, val)) = line.split_once(':') else {
            continue;
        };
        let val_kb: u64 = val
            .split_whitespace()
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        match key.trim() {
            "MemTotal" => mi.total = val_kb,
            "MemFree" => mi.free = val_kb,
            "MemAvailable" => mi.available = val_kb,
            "Buffers" => mi.buffers = val_kb,
            "Cached" => mi.cached = val_kb,
            "SwapTotal" => mi.swap_total = val_kb,
            "SwapFree" => mi.swap_free = val_kb,
            _ => {}
        }
    }
    mi
}

/// Format a byte count as a SI-ish IEC string: e.g. "1.5Gi".
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    fn s(items: &[&str]) -> Vec<String> {
        items.iter().map(|x| (*x).to_string()).collect()
    }

    #[test]
    fn parse_no_args_defaults_to_kib() {
        let o = parse_args(&s(&[]));
        assert!(!o.human);
        assert_eq!(o.unit, 1);
        assert_eq!(o.unit_name, "KiB");
    }

    #[test]
    fn parse_dash_h() {
        let o = parse_args(&s(&["-h"]));
        assert!(o.human);
    }

    #[test]
    fn parse_dash_m_mib() {
        let o = parse_args(&s(&["-m"]));
        assert_eq!(o.unit, 1024);
        assert_eq!(o.unit_name, "MiB");
    }

    #[test]
    fn parse_dash_g_gib() {
        let o = parse_args(&s(&["-g"]));
        assert_eq!(o.unit, 1024 * 1024);
        assert_eq!(o.unit_name, "GiB");
    }

    #[test]
    fn parse_later_unit_overrides() {
        let o = parse_args(&s(&["-m", "-g"]));
        assert_eq!(o.unit, 1024 * 1024);
        assert_eq!(o.unit_name, "GiB");
    }

    #[test]
    fn parse_unknown_arg_ignored() {
        let o = parse_args(&s(&["--unknown"]));
        assert_eq!(o.unit, 1);
        assert!(!o.human);
    }

    #[test]
    fn meminfo_parse_typical() {
        let input = "\
MemTotal:       16384 kB
MemFree:         2048 kB
MemAvailable:    8192 kB
Buffers:          512 kB
Cached:          1024 kB
SwapTotal:       4096 kB
SwapFree:        4000 kB
SomethingElse:   9999 kB
";
        let mi = parse_meminfo(input);
        assert_eq!(mi.total, 16384);
        assert_eq!(mi.free, 2048);
        assert_eq!(mi.available, 8192);
        assert_eq!(mi.buffers, 512);
        assert_eq!(mi.cached, 1024);
        assert_eq!(mi.swap_total, 4096);
        assert_eq!(mi.swap_free, 4000);
    }

    #[test]
    fn meminfo_parse_empty_gives_default() {
        let mi = parse_meminfo("");
        assert_eq!(mi, MemInfo::default());
    }

    #[test]
    fn meminfo_parse_skips_unrelated() {
        let input = "Foo: 100 kB\nBar: 200 kB\nMemTotal: 4096 kB\n";
        let mi = parse_meminfo(input);
        assert_eq!(mi.total, 4096);
        assert_eq!(mi.free, 0);
    }

    #[test]
    fn meminfo_parse_unparseable_value_zero() {
        let input = "MemTotal: abc kB\n";
        let mi = parse_meminfo(input);
        assert_eq!(mi.total, 0);
    }

    #[test]
    fn meminfo_parse_no_colon_skipped() {
        let mi = parse_meminfo("MemTotal 4096 kB\n");
        assert_eq!(mi.total, 0);
    }

    #[test]
    fn human_size_bytes_under_kib() {
        assert_eq!(human_size(0), "0B");
        assert_eq!(human_size(512), "512B");
        assert_eq!(human_size(1023), "1023B");
    }

    #[test]
    fn human_size_kib_range() {
        assert_eq!(human_size(1024), "1.0Ki");
        assert_eq!(human_size(2048), "2.0Ki");
        assert_eq!(human_size(1024 * 1023), "1023.0Ki");
    }

    #[test]
    fn human_size_mib_range() {
        assert_eq!(human_size(1024 * 1024), "1.0Mi");
        assert_eq!(human_size(1024 * 1024 * 2), "2.0Mi");
    }

    #[test]
    fn human_size_gib_range() {
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0Gi");
        assert_eq!(human_size(1024 * 1024 * 1024 * 3), "3.0Gi");
    }

    #[test]
    fn human_size_fractional_kib() {
        assert_eq!(human_size(1536), "1.5Ki");
    }

    #[test]
    fn human_size_fractional_mib() {
        assert_eq!(human_size(1024 * 1024 + 512 * 1024), "1.5Mi");
    }
}
